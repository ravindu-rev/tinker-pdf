//! The page and its tables as records (`docs/design/command-stream.md`, "The
//! page and its tables"): each table entry a writer and a reader share, and
//! each one's record, written and read.
//!
//! Entries are kept in the order the writer met them, which is the order
//! their records stand in, so a stream written twice from one page is the
//! same bytes and a stream read back holds the entries it was written from.

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_color::icc::Transforms;
use tinker_pdf_color::{ColorSpace, Colorant, Colorants, Function, Process};
use tinker_pdf_content::commands::{
    color_family_number, color_family_of, flag, group_space_at, group_space_field, matrix_at,
    matrix_to_field, optional_bytes, reals, reference, rendering_intent_number,
    rendering_intent_of, CommandError, CommandKind, EventReader, EventWriter, Field, Fields, Meter,
    Record,
};
use tinker_pdf_content::{GroupSpace, Matrix, RenderingIntent};
use tinker_pdf_font::{Outline, Segment};
use tinker_pdf_raster::image::SourceWindow;
use tinker_pdf_render::mesh::Patch;
use tinker_pdf_render::{
    DecodedImage, FontIdentity, Mesh, PatternPaint, Reduced, Shading, TilingPattern,
    TwoColourSamples,
};

use crate::resources::{Frame, JpxFrame, PageResources};
use crate::Usage;

// ---- notes ------------------------------------------------------------------------

/// What answering one question noted in the scope it was asked in (ruling
/// 10): the fonts it could not resolve, the images it decoded with damage and
/// the colour spaces it read with a repair, each as the scope's lists hold
/// them. A replay notes them again in its scope at every answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Notes {
    pub(crate) missing: Vec<String>,
    pub(crate) damaged: Vec<(String, String)>,
    pub(crate) repaired: Vec<(String, String)>,
}

impl Notes {
    /// Everything `resources` has noted: an asker built for one answer holds
    /// that answer's notes alone.
    pub(crate) fn of(resources: &PageResources) -> Notes {
        Notes {
            missing: resources.missing_fonts(),
            damaged: resources.damaged_images(),
            repaired: resources.repaired_spaces(),
        }
    }

    /// The five fields that end a record answering a question.
    fn fields(&self) -> [Field; 5] {
        let strings = |list: &mut dyn Iterator<Item = &String>| {
            Field::Strings(list.map(|s| s.as_bytes().to_vec()).collect())
        };
        [
            strings(&mut self.missing.iter()),
            strings(&mut self.damaged.iter().map(|(name, _)| name)),
            strings(&mut self.damaged.iter().map(|(_, what)| what)),
            strings(&mut self.repaired.iter().map(|(name, _)| name)),
            strings(&mut self.repaired.iter().map(|(_, what)| what)),
        ]
    }

    /// The notes a record holds from its field `at` on.
    fn read(fields: &Fields<'_>, at: usize) -> Result<Notes, CommandError> {
        let text = |at: usize| -> Result<Vec<String>, CommandError> {
            fields
                .strings(at)?
                .iter()
                .map(|s| {
                    String::from_utf8(s.clone()).map_err(|_| {
                        fields.wrong(format!("a note in field {at} that is not UTF-8"))
                    })
                })
                .collect()
        };
        let pairs = |names: usize, whats: usize| -> Result<Vec<(String, String)>, CommandError> {
            let (names, whats) = (text(names)?, text(whats)?);
            if names.len() != whats.len() {
                return Err(fields.wrong("notes whose names and reasons disagree"));
            }
            Ok(names.into_iter().zip(whats).collect())
        };
        Ok(Notes {
            missing: text(at)?,
            damaged: pairs(at + 1, at + 2)?,
            repaired: pairs(at + 3, at + 4)?,
        })
    }

    fn with(self, record: Record) -> Record {
        let mut record = record;
        record.fields.extend(self.fields());
        record
    }
}

/// One answer with what it noted.
#[derive(Clone, Debug)]
pub(crate) struct Answered<T> {
    pub(crate) value: T,
    pub(crate) notes: Notes,
}

// ---- the page --------------------------------------------------------------------

/// What a render reads of the page itself: its geometry, the space it
/// composites in (11.4.7), the usage its optional content was recorded for,
/// and which page it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageRecord {
    pub(crate) crop: (f64, f64, f64, f64),
    pub(crate) rotation: u16,
    pub(crate) group: Option<GroupSpace>,
    pub(crate) usage: Option<Usage>,
    pub(crate) index: u32,
}

fn usage_field(usage: Option<Usage>) -> Field {
    match usage {
        None => Field::Nothing,
        Some(Usage::View) => Field::Unsigned(0),
        Some(Usage::Print) => Field::Unsigned(1),
        Some(Usage::Export) => Field::Unsigned(2),
    }
}

impl PageRecord {
    pub(crate) fn record(&self) -> Record {
        let (x0, y0, x1, y1) = self.crop;
        Record::new(CommandKind::Page)
            .with(reals(&[x0, y0, x1, y1]))
            .with(Field::Unsigned(u64::from(self.rotation)))
            .with(group_space_field(self.group))
            .with(usage_field(self.usage))
            .with(Field::Unsigned(u64::from(self.index)))
    }

    pub(crate) fn read(fields: &Fields<'_>) -> Result<PageRecord, CommandError> {
        let crop = fields.exact_reals(0, 4)?;
        let rotation = match fields.unsigned(1)? {
            r @ (0 | 90 | 180 | 270) => r as u16,
            other => return Err(fields.wrong(format!("a rotation of {other}"))),
        };
        let usage = match fields.optional_unsigned(3)? {
            None => None,
            Some(0) => Some(Usage::View),
            Some(1) => Some(Usage::Print),
            Some(2) => Some(Usage::Export),
            Some(other) => return Err(fields.wrong(format!("a usage {other}"))),
        };
        Ok(PageRecord {
            crop: (crop[0], crop[1], crop[2], crop[3]),
            rotation,
            group: group_space_at(fields, 2)?,
            usage,
            index: fields.unsigned32(4)?,
        })
    }
}

// ---- scopes, outlines and font identities ----------------------------------------

/// A scope a renderer resolves names in, beside the page's (scope 0): a
/// form's own resources, by the scope that names the form and the form's
/// name, or an annotation appearance's, by neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScopeEntry {
    pub(crate) parent: Option<u64>,
    pub(crate) name: Option<Vec<u8>>,
}

impl ScopeEntry {
    fn record(&self, id: u64) -> Record {
        Record::new(CommandKind::Scope)
            .with(Field::Unsigned(id))
            .with(reference(self.parent))
            .with(optional_bytes(self.name.as_deref()))
    }

    fn read(fields: &Fields<'_>, expected: u64) -> Result<ScopeEntry, CommandError> {
        if fields.unsigned(0)? != expected {
            return Err(fields.wrong("a scope out of its order"));
        }
        let parent = fields.reference(1)?;
        let name = fields.optional_bytes(2)?.map(|n| n.to_vec());
        if parent.is_some() != name.is_some() {
            return Err(
                fields.wrong("a scope with a parent and no form name, or a name and no parent")
            );
        }
        if parent.is_some_and(|parent| parent >= expected) {
            return Err(fields.wrong("a scope whose parent does not precede it"));
        }
        Ok(ScopeEntry { parent, name })
    }
}

/// One glyph's outline in one scope, as `GlyphSource::outline` answered it.
#[derive(Clone, Debug)]
pub(crate) struct OutlineEntry {
    pub(crate) scope: u64,
    pub(crate) font_id: u64,
    pub(crate) code: u32,
    pub(crate) answer: Answered<Option<Outline>>,
}

fn outline_fields(outline: &Outline) -> (Field, Field) {
    let mut ops = Vec::with_capacity(outline.segments.len());
    let mut numbers = Vec::new();
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                ops.push(0u8);
                numbers.extend_from_slice(&[x, y]);
            }
            Segment::LineTo { x, y } => {
                ops.push(1);
                numbers.extend_from_slice(&[x, y]);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                ops.push(2);
                numbers.extend_from_slice(&[cx, cy, x, y]);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                ops.push(3);
                numbers.extend_from_slice(&[c1x, c1y, c2x, c2y, x, y]);
            }
            Segment::Close => ops.push(4),
        }
    }
    (Field::Bytes(Arc::from(ops)), Field::Reals(numbers))
}

/// An outline read back from its operators and numbers, which must agree.
pub(crate) fn read_outline(
    fields: &Fields<'_>,
    ops_at: usize,
    numbers_at: usize,
) -> Result<Outline, CommandError> {
    let ops = fields.bytes(ops_at)?;
    let numbers = fields.reals(numbers_at)?;
    let mut segments = Vec::with_capacity(ops.len());
    let mut at = 0usize;
    for &op in ops.iter() {
        let take = match op {
            0 | 1 => 2,
            2 => 4,
            3 => 6,
            4 => 0,
            other => return Err(fields.wrong(format!("an outline operator {other}"))),
        };
        let v = numbers
            .get(at..at + take)
            .ok_or_else(|| fields.wrong("an outline with fewer numbers than its operators take"))?;
        at += take;
        segments.push(match op {
            0 => Segment::MoveTo { x: v[0], y: v[1] },
            1 => Segment::LineTo { x: v[0], y: v[1] },
            2 => Segment::QuadTo {
                cx: v[0],
                cy: v[1],
                x: v[2],
                y: v[3],
            },
            3 => Segment::CurveTo {
                c1x: v[0],
                c1y: v[1],
                c2x: v[2],
                c2y: v[3],
                x: v[4],
                y: v[5],
            },
            _ => Segment::Close,
        });
    }
    if at != numbers.len() {
        return Err(fields.wrong("an outline with more numbers than its operators take"));
    }
    Ok(Outline { segments })
}

impl OutlineEntry {
    fn record(&self) -> Record {
        let record = Record::new(CommandKind::Outline)
            .with(Field::Unsigned(self.scope))
            .with(Field::Unsigned(self.font_id))
            .with(Field::Unsigned(u64::from(self.code)));
        let record = match &self.answer.value {
            Some(outline) => {
                let (ops, numbers) = outline_fields(outline);
                record.with(flag(true)).with(ops).with(numbers)
            }
            None => record
                .with(flag(false))
                .with(Field::bytes(&[]))
                .with(Field::Reals(Vec::new())),
        };
        self.answer.notes.clone().with(record)
    }

    fn read(fields: &Fields<'_>, meter: &mut Meter) -> Result<OutlineEntry, CommandError> {
        let found = fields.flag(3)?;
        let outline = if found {
            let outline = read_outline(fields, 4, 5)?;
            meter.spend(
                (outline.segments.len() as u64)
                    .saturating_mul(core::mem::size_of::<Segment>() as u64),
            )?;
            Some(outline)
        } else {
            None
        };
        Ok(OutlineEntry {
            scope: fields.unsigned(0)?,
            font_id: fields.unsigned(1)?,
            code: fields.unsigned32(2)?,
            answer: Answered {
                value: outline,
                notes: Notes::read(fields, 6)?,
            },
        })
    }
}

/// What a font id selects in one scope, for the glyph-mask cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IdentityEntry {
    pub(crate) scope: u64,
    pub(crate) font_id: u64,
    pub(crate) identity: Option<FontIdentity>,
}

impl IdentityEntry {
    fn record(&self) -> Record {
        let (kind, object) = match self.identity {
            None => (0, Field::Nothing),
            Some(FontIdentity::Object(object)) => (1, Field::Unsigned(object)),
            Some(FontIdentity::Scoped) => (2, Field::Nothing),
        };
        Record::new(CommandKind::FontIdentity)
            .with(Field::Unsigned(self.scope))
            .with(Field::Unsigned(self.font_id))
            .with(Field::Unsigned(kind))
            .with(object)
    }

    fn read(fields: &Fields<'_>) -> Result<IdentityEntry, CommandError> {
        let identity = match fields.unsigned(2)? {
            0 => None,
            1 => Some(FontIdentity::Object(fields.unsigned(3)?)),
            2 => Some(FontIdentity::Scoped),
            other => return Err(fields.wrong(format!("a font identity {other}"))),
        };
        Ok(IdentityEntry {
            scope: fields.unsigned(0)?,
            font_id: fields.unsigned(1)?,
            identity,
        })
    }
}

// ---- images -------------------------------------------------------------------------

/// Which image a table entry answers for: an XObject by its name in the
/// scope, or an inline image by its place among the stream's inline images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImageKey {
    Named(Vec<u8>),
    Inline(u64),
}

/// One image a draw names in one scope under one intent, and the frame its
/// draws are read against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageEntry {
    pub(crate) scope: u64,
    pub(crate) key: ImageKey,
    pub(crate) intent: RenderingIntent,
    pub(crate) frame: Option<Frame>,
    /// The bytes the writing document's budget left the image's decode
    /// (`PageResources::image_room`), which a draw reads the reduction it
    /// takes against beside its size; `None` where no budget reduces it.
    pub(crate) room: Option<u64>,
}

fn frame_fields(frame: Option<Frame>) -> [Field; 8] {
    match frame {
        None => [
            Field::Unsigned(0),
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
        ],
        Some(Frame::Jpeg { width, height }) => [
            Field::Unsigned(1),
            Field::Unsigned(u64::from(width)),
            Field::Unsigned(u64::from(height)),
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
            Field::Nothing,
        ],
        Some(Frame::Jpx(frame)) => [
            Field::Unsigned(2),
            Field::Unsigned(u64::from(frame.width)),
            Field::Unsigned(u64::from(frame.height)),
            Field::Unsigned(u64::from(frame.origin.0)),
            Field::Unsigned(u64::from(frame.origin.1)),
            Field::Unsigned(u64::from(frame.levels)),
            flag(frame.opaque),
            flag(frame.interpolate),
        ],
    }
}

fn read_frame(fields: &Fields<'_>, at: usize) -> Result<Option<Frame>, CommandError> {
    Ok(match fields.unsigned(at)? {
        0 => None,
        1 => Some(Frame::Jpeg {
            width: fields.unsigned32(at + 1)?,
            height: fields.unsigned32(at + 2)?,
        }),
        2 => Some(Frame::Jpx(JpxFrame {
            width: fields.unsigned32(at + 1)?,
            height: fields.unsigned32(at + 2)?,
            origin: (fields.unsigned32(at + 3)?, fields.unsigned32(at + 4)?),
            levels: u8::try_from(fields.unsigned(at + 5)?)
                .map_err(|_| fields.wrong("a codestream's levels past 255"))?,
            opaque: fields.flag(at + 6)?,
            interpolate: fields.flag(at + 7)?,
        })),
        other => return Err(fields.wrong(format!("a frame of kind {other}"))),
    })
}

impl ImageEntry {
    fn record(&self) -> Record {
        let (kind, key) = match &self.key {
            ImageKey::Named(name) => (CommandKind::Image, Field::bytes(name)),
            ImageKey::Inline(ordinal) => (CommandKind::InlineImage, Field::Unsigned(*ordinal)),
        };
        let mut record = Record::new(kind)
            .with(Field::Unsigned(self.scope))
            .with(key)
            .with(Field::Unsigned(rendering_intent_number(self.intent)));
        record.fields.extend(frame_fields(self.frame));
        record.with(reference(self.room))
    }

    fn read(fields: &Fields<'_>, inline: bool) -> Result<ImageEntry, CommandError> {
        let key = if inline {
            ImageKey::Inline(fields.unsigned(1)?)
        } else {
            ImageKey::Named(fields.bytes(1)?.to_vec())
        };
        Ok(ImageEntry {
            scope: fields.unsigned(0)?,
            key,
            intent: rendering_intent_of(fields.unsigned(2)?)
                .ok_or_else(|| fields.wrong("a rendering intent past Table 69's"))?,
            frame: read_frame(fields, 3)?,
            room: fields.optional_unsigned(11)?,
        })
    }
}

/// What a decode of an image hands a draw: `GlyphSource::image`'s
/// `Result<Option<DecodedImage>, String>`, the image shared.
#[derive(Clone, Debug)]
pub(crate) enum ImageAnswer {
    None,
    Image(Arc<DecodedImage>),
    Refused(String),
}

impl ImageAnswer {
    pub(crate) fn of(answer: Result<Option<DecodedImage>, String>) -> ImageAnswer {
        match answer {
            Ok(None) => ImageAnswer::None,
            Ok(Some(image)) => ImageAnswer::Image(Arc::new(image)),
            Err(codec) => ImageAnswer::Refused(codec),
        }
    }

    /// The answer as a draw is handed it.
    pub(crate) fn handed(&self) -> Result<Option<DecodedImage>, String> {
        match self {
            ImageAnswer::None => Ok(None),
            ImageAnswer::Image(image) => Ok(Some((**image).clone())),
            ImageAnswer::Refused(codec) => Err(codec.clone()),
        }
    }
}

/// One image entry's decode at one reduction.
#[derive(Clone, Debug)]
pub(crate) struct DecodeEntry {
    pub(crate) image: u64,
    pub(crate) reduction: u32,
    pub(crate) answer: Answered<ImageAnswer>,
}

impl DecodeEntry {
    fn record(&self) -> Record {
        let record = Record::new(CommandKind::ImageDecode)
            .with(Field::Unsigned(self.image))
            .with(Field::Unsigned(u64::from(self.reduction)));
        let mut record = match &self.answer.value {
            ImageAnswer::None => {
                let mut record = record.with(Field::Unsigned(0)).with(Field::Nothing);
                record
                    .fields
                    .extend(std::iter::repeat_n(Field::Nothing, 12));
                record
            }
            ImageAnswer::Refused(codec) => {
                let mut record = record
                    .with(Field::Unsigned(2))
                    .with(Field::bytes(codec.as_bytes()));
                record
                    .fields
                    .extend(std::iter::repeat_n(Field::Nothing, 12));
                record
            }
            ImageAnswer::Image(image) => {
                let (samples, colours) = match &image.two_colour {
                    Some(two) => (
                        Field::bytes(&two.samples),
                        Field::Bytes(Arc::from(
                            two.colours.iter().flatten().copied().collect::<Vec<u8>>(),
                        )),
                    ),
                    None => (Field::Nothing, Field::Nothing),
                };
                let reduced = image.reduced.map_or(Field::Nothing, |grid| {
                    Field::Naturals(vec![
                        u64::from(grid.full_width),
                        u64::from(grid.full_height),
                        u64::from(grid.factor),
                        u64::from(grid.skip.0),
                        u64::from(grid.skip.1),
                    ])
                });
                let window = image.window.map_or(Field::Nothing, |w| {
                    Field::Naturals(vec![
                        u64::from(w.x),
                        u64::from(w.y),
                        u64::from(w.width),
                        u64::from(w.height),
                    ])
                });
                record
                    .with(Field::Unsigned(1))
                    .with(Field::Nothing)
                    .with(Field::Unsigned(u64::from(image.width)))
                    .with(Field::Unsigned(u64::from(image.height)))
                    .with(Field::bytes(&image.rgb))
                    .with(Field::bytes(&image.alpha))
                    .with(samples)
                    .with(colours)
                    .with(flag(image.stencil))
                    .with(flag(image.interpolate))
                    .with(reduced)
                    .with(Field::Unsigned(color_family_number(image.family)))
                    .with(flag(image.soft_masked))
                    .with(window)
            }
        };
        record.fields.extend(self.answer.notes.fields());
        record
    }

    fn read(fields: &Fields<'_>, images: u64) -> Result<DecodeEntry, CommandError> {
        let image = fields.unsigned(0)?;
        if image >= images {
            return Err(fields.wrong("a decode of an image no record before defines"));
        }
        let reduction = fields.unsigned32(1)?;
        let answer = match fields.unsigned(2)? {
            0 => ImageAnswer::None,
            2 => ImageAnswer::Refused(fields.text(3)?),
            1 => ImageAnswer::Image(Arc::new(read_decoded(fields)?)),
            other => return Err(fields.wrong(format!("an image answer {other}"))),
        };
        Ok(DecodeEntry {
            image,
            reduction,
            answer: Answered {
                value: answer,
                notes: Notes::read(fields, 16)?,
            },
        })
    }
}

/// A decoded image read back, held to the sizes its fields declare, so a
/// draw never reads past its samples.
fn read_decoded(fields: &Fields<'_>) -> Result<DecodedImage, CommandError> {
    let width = fields.unsigned32(4)?;
    let height = fields.unsigned32(5)?;
    let rgb = fields.bytes(6)?.to_vec();
    let alpha = fields.bytes(7)?.to_vec();
    let window = match fields.optional_naturals(15)? {
        None => None,
        Some([x, y, w, h]) => {
            let fit = |v: u64| u32::try_from(v).map_err(|_| fields.wrong("a window past 32 bits"));
            let window = SourceWindow {
                x: fit(*x)?,
                y: fit(*y)?,
                width: fit(*w)?,
                height: fit(*h)?,
            };
            let inside = u64::from(window.x) + u64::from(window.width) <= u64::from(width)
                && u64::from(window.y) + u64::from(window.height) <= u64::from(height);
            if !inside {
                return Err(fields.wrong("a window past its image"));
            }
            Some(window)
        }
        Some(_) => return Err(fields.wrong("a window of other than four numbers")),
    };
    let (held_w, held_h) = window.map_or((width, height), |w| (w.width, w.height));
    let samples = u64::from(held_w) * u64::from(held_h);
    let two_colour = match fields.optional_bytes(8)? {
        None => None,
        Some(samples_held) => {
            let colours = fields.exact_bytes(9, 8)?;
            if samples_held.len() as u64 != samples {
                return Err(fields.wrong("two-colour samples of another size than the image"));
            }
            Some(TwoColourSamples {
                samples: samples_held.to_vec(),
                colours: [
                    [colours[0], colours[1], colours[2], colours[3]],
                    [colours[4], colours[5], colours[6], colours[7]],
                ],
            })
        }
    };
    let rgb_ok = if two_colour.is_some() {
        rgb.is_empty() && alpha.is_empty()
    } else {
        rgb.len() as u64 == samples.saturating_mul(3)
            && (alpha.is_empty() || alpha.len() as u64 == samples)
    };
    if !rgb_ok {
        return Err(fields.wrong("samples of another size than the image"));
    }
    let reduced = match fields.optional_naturals(12)? {
        None => None,
        Some([w, h, factor, sx, sy]) => {
            let fit =
                |v: u64| u32::try_from(v).map_err(|_| fields.wrong("a reduced grid past 32 bits"));
            Some(Reduced {
                full_width: fit(*w)?,
                full_height: fit(*h)?,
                factor: fit(*factor)?,
                skip: (fit(*sx)?, fit(*sy)?),
            })
        }
        Some(_) => return Err(fields.wrong("a reduced grid of other than five numbers")),
    };
    Ok(DecodedImage {
        width,
        height,
        rgb,
        alpha,
        two_colour,
        stencil: fields.flag(10)?,
        interpolate: fields.flag(11)?,
        reduced,
        family: color_family_of(fields.unsigned(13)?)
            .ok_or_else(|| fields.wrong("a colour family past 3"))?,
        soft_masked: fields.flag(14)?,
        window,
    })
}

// ---- colour spaces, profiles and shadings -----------------------------------------

/// The colour spaces, profiles and shading definitions a writer has written,
/// each once, and their places.
#[derive(Default)]
pub(crate) struct Definitions {
    records: Vec<Record>,
    profiles: Vec<(usize, u64)>,
    profile_bytes: HashMap<Vec<u8>, u64>,
    profile_count: u64,
    spaces: HashMap<Vec<u8>, u64>,
    space_count: u64,
    shadings: HashMap<Vec<u8>, u64>,
    shading_count: u64,
}

impl Definitions {
    /// The records, in the order written.
    pub(crate) fn into_records(self) -> Vec<Record> {
        self.records
    }

    fn profile(&mut self, transforms: &Arc<Transforms>, bytes: &[u8]) -> u64 {
        let address = Arc::as_ptr(transforms) as usize;
        if let Some((_, index)) = self.profiles.iter().find(|(held, _)| *held == address) {
            return *index;
        }
        let index = match self.profile_bytes.get(bytes) {
            Some(index) => *index,
            None => {
                let index = self.profile_count;
                self.profile_count += 1;
                self.profile_bytes.insert(bytes.to_vec(), index);
                self.records
                    .push(Record::new(CommandKind::Profile).with(Field::bytes(bytes)));
                index
            }
        };
        self.profiles.push((address, index));
        index
    }

    fn dedupe(
        table: &mut HashMap<Vec<u8>, u64>,
        count: &mut u64,
        records: &mut Vec<Record>,
        record: Record,
    ) -> u64 {
        let mut bytes = Vec::new();
        tinker_pdf_content::commands::put_record(&mut bytes, &record);
        if let Some(index) = table.get(&bytes) {
            return *index;
        }
        let index = *count;
        *count += 1;
        table.insert(bytes, index);
        records.push(record);
        index
    }

    /// The place of `space` among the stream's colour spaces, written with
    /// what it refers to where it is new.
    pub(crate) fn space(&mut self, space: &ColorSpace, events: &mut EventWriter) -> u64 {
        let record = Record::new(CommandKind::ColorSpace);
        let record = match space {
            ColorSpace::DeviceGray => record.with(Field::Unsigned(0)),
            ColorSpace::DeviceRgb => record.with(Field::Unsigned(1)),
            ColorSpace::DeviceCmyk => record.with(Field::Unsigned(2)),
            ColorSpace::Indexed { base, lookup, high } => {
                let base = self.space(base, events);
                record
                    .with(Field::Unsigned(3))
                    .with(Field::Unsigned(base))
                    .with(Field::bytes(lookup))
                    .with(Field::Unsigned(u64::from(*high)))
            }
            ColorSpace::Separation {
                components,
                alternate,
                tint,
                colorants,
            } => {
                let alternate = self.space(alternate, events);
                let tint = events.function(tint);
                let mut kinds = Vec::with_capacity(colorants.names.len());
                let mut names = Vec::new();
                for colorant in &colorants.names {
                    match colorant {
                        Colorant::Named(name) => {
                            kinds.push(0);
                            names.push(name.clone());
                        }
                        Colorant::All => kinds.push(1),
                        Colorant::None => kinds.push(2),
                    }
                }
                let separation_names: Vec<Vec<u8>> = colorants
                    .separations
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect();
                let separation_spaces: Vec<u64> = colorants
                    .separations
                    .iter()
                    .map(|(_, space)| self.space(space, events))
                    .collect();
                let (process, process_names) = match &colorants.process {
                    Some(process) => (
                        Field::Unsigned(self.space(&process.space, events)),
                        process.components.clone(),
                    ),
                    None => (Field::Nothing, Vec::new()),
                };
                record
                    .with(Field::Unsigned(4))
                    .with(Field::Unsigned(*components as u64))
                    .with(Field::Unsigned(alternate))
                    .with(Field::Unsigned(tint))
                    .with(Field::Naturals(kinds))
                    .with(Field::Strings(names))
                    .with(flag(colorants.n_channel))
                    .with(Field::Strings(separation_names))
                    .with(Field::Naturals(separation_spaces))
                    .with(process)
                    .with(Field::Strings(process_names))
            }
            ColorSpace::Icc {
                transforms,
                components,
            } => match transforms.source() {
                Some(bytes) => {
                    let profile = self.profile(transforms, bytes);
                    record
                        .with(Field::Unsigned(5))
                        .with(Field::Unsigned(profile))
                        .with(Field::Unsigned(*components as u64))
                }
                // Transforms compiled from a profile handed over parsed, which
                // only a space built by hand has: 8.6.5.5's approximation by
                // the component count (`docs/design/command-stream.md`).
                None => record
                    .with(Field::Unsigned(6))
                    .with(Field::Unsigned(*components as u64)),
            },
            ColorSpace::Approximated { components } => record
                .with(Field::Unsigned(6))
                .with(Field::Unsigned(*components as u64)),
            ColorSpace::CalGray { white, gamma } => record
                .with(Field::Unsigned(7))
                .with(reals(white))
                .with(Field::Real(*gamma)),
            ColorSpace::CalRgb {
                white,
                gamma,
                matrix,
            } => record
                .with(Field::Unsigned(8))
                .with(reals(white))
                .with(reals(gamma))
                .with(reals(matrix)),
            ColorSpace::Lab { range } => record.with(Field::Unsigned(9)).with(reals(range)),
            ColorSpace::Pattern { base } => {
                let base = base.as_deref().map(|base| self.space(base, events));
                record.with(Field::Unsigned(10)).with(reference(base))
            }
        };
        Definitions::dedupe(
            &mut self.spaces,
            &mut self.space_count,
            &mut self.records,
            record,
        )
    }

    /// The place of `shading` among the stream's shading definitions,
    /// written with its space and function where it is new.
    pub(crate) fn shading(&mut self, shading: &Shading, events: &mut EventWriter) -> u64 {
        let record = Record::new(CommandKind::ShadingDefinition);
        let nothing = || Field::Nothing;
        let record = match shading {
            Shading::FunctionBased {
                space,
                function,
                domain,
            } => {
                let space = self.space(space, events);
                let function = events.function(function);
                let mut record = record
                    .with(Field::Unsigned(1))
                    .with(Field::Unsigned(space))
                    .with(Field::Unsigned(function))
                    .with(reals(domain))
                    .with(nothing());
                record.fields.extend(std::iter::repeat_n(Field::Nothing, 7));
                record
            }
            Shading::Axial {
                space,
                function,
                coords,
                extend,
            } => {
                let space = self.space(space, events);
                let function = events.function(function);
                let mut record = record
                    .with(Field::Unsigned(2))
                    .with(Field::Unsigned(space))
                    .with(Field::Unsigned(function))
                    .with(reals(coords))
                    .with(Field::Naturals(vec![
                        u64::from(extend.0),
                        u64::from(extend.1),
                    ]));
                record.fields.extend(std::iter::repeat_n(Field::Nothing, 7));
                record
            }
            Shading::Radial {
                space,
                function,
                coords,
                extend,
            } => {
                let space = self.space(space, events);
                let function = events.function(function);
                let mut record = record
                    .with(Field::Unsigned(3))
                    .with(Field::Unsigned(space))
                    .with(Field::Unsigned(function))
                    .with(reals(coords))
                    .with(Field::Naturals(vec![
                        u64::from(extend.0),
                        u64::from(extend.1),
                    ]));
                record.fields.extend(std::iter::repeat_n(Field::Nothing, 7));
                record
            }
            Shading::Mesh(mesh) => {
                let space = self.space(&mesh.space, events);
                let function = mesh.function.as_ref().map(|f| events.function(f));
                let positions: Vec<f64> =
                    mesh.positions.iter().flat_map(|&(x, y)| [x, y]).collect();
                let triangles: Vec<u64> = mesh
                    .triangles
                    .iter()
                    .flat_map(|t| t.iter().map(|&i| u64::from(i)))
                    .collect();
                let points: Vec<f64> = mesh
                    .patches
                    .iter()
                    .flat_map(|patch| patch.points.iter().flatten().flat_map(|&(x, y)| [x, y]))
                    .collect();
                let corners: Vec<f64> = mesh
                    .patches
                    .iter()
                    .flat_map(|patch| patch.corners.iter().copied())
                    .collect();
                record
                    .with(Field::Unsigned(4))
                    .with(Field::Unsigned(space))
                    .with(reference(function))
                    .with(Field::Nothing)
                    .with(Field::Nothing)
                    .with(Field::Signed(mesh.kind))
                    .with(Field::Unsigned(mesh.components as u64))
                    .with(Field::Reals(positions))
                    .with(reals(&mesh.values))
                    .with(Field::Naturals(triangles))
                    .with(Field::Reals(points))
                    .with(Field::Reals(corners))
            }
        };
        Definitions::dedupe(
            &mut self.shadings,
            &mut self.shading_count,
            &mut self.records,
            record,
        )
    }
}

/// The definitions a reader has read: colour spaces with their nesting and
/// weight, compiled profiles, and shadings.
#[derive(Default)]
pub(crate) struct Defined {
    profiles: Vec<Arc<[u8]>>,
    compiled: Vec<Option<Arc<Transforms>>>,
    spaces: Vec<(ColorSpace, u32, u64)>,
    pub(crate) shadings: Vec<(Arc<Shading>, u64)>,
}

/// The bytes a copy of `space` holds, its parts included.
fn space_weight(space: &ColorSpace) -> u64 {
    let own = core::mem::size_of::<ColorSpace>() as u64;
    let n = |len: usize| len as u64;
    own.saturating_add(match space {
        ColorSpace::Indexed { base, lookup, .. } => {
            space_weight(base).saturating_add(n(lookup.len()))
        }
        ColorSpace::Separation {
            alternate,
            colorants,
            ..
        } => {
            let names: u64 = colorants
                .names
                .iter()
                .map(|c| match c {
                    Colorant::Named(name) => n(name.len()).saturating_add(24),
                    _ => 24,
                })
                .fold(0, u64::saturating_add);
            let separations: u64 = colorants
                .separations
                .iter()
                .map(|(name, space)| n(name.len()).saturating_add(space_weight(space)))
                .fold(0, u64::saturating_add);
            let process = colorants.process.as_ref().map_or(0, |p| {
                space_weight(&p.space).saturating_add(
                    p.components
                        .iter()
                        .map(|c| n(c.len()).saturating_add(24))
                        .fold(0, u64::saturating_add),
                )
            });
            // The tint function is copied too; its weight is the reader's to
            // charge where it copies it.
            space_weight(alternate)
                .saturating_add(names)
                .saturating_add(separations)
                .saturating_add(process)
        }
        ColorSpace::Pattern { base } => base.as_deref().map_or(0, space_weight),
        _ => 0,
    })
}

impl Defined {
    fn space_at(
        &self,
        fields: &Fields<'_>,
        index: u64,
    ) -> Result<&(ColorSpace, u32, u64), CommandError> {
        usize::try_from(index)
            .ok()
            .and_then(|at| self.spaces.get(at))
            .ok_or_else(|| {
                fields.wrong(format!(
                    "colour space {index}, which no record before defines"
                ))
            })
    }

    /// A colour space a field names, copied once the copy is known to fit.
    fn space_copy(
        &self,
        fields: &Fields<'_>,
        at: usize,
        meter: &Meter,
    ) -> Result<(ColorSpace, u32), CommandError> {
        let index = fields.unsigned(at)?;
        let (space, depth, weight) = self.space_at(fields, index)?;
        if !meter.can_hold(*weight) {
            return Err(CommandError::OverBudget { budget: "bytes" });
        }
        Ok((space.clone(), *depth))
    }

    fn function_copy(
        &self,
        fields: &Fields<'_>,
        at: usize,
        events: &EventReader,
        meter: &Meter,
    ) -> Result<Function, CommandError> {
        let index = fields.unsigned(at)?;
        let (function, weight) = events.function(index).ok_or_else(|| {
            fields.wrong(format!("function {index}, which no record before defines"))
        })?;
        if !meter.can_hold(weight) {
            return Err(CommandError::OverBudget { budget: "bytes" });
        }
        Ok((**function).clone())
    }

    pub(crate) fn read_profile(&mut self, fields: &Fields<'_>) -> Result<(), CommandError> {
        self.profiles.push(Arc::clone(fields.bytes(0)?));
        self.compiled.push(None);
        Ok(())
    }

    /// The transforms of profile `index`, compiled the first time a space
    /// asks for them and shared by every space after.
    fn transforms(
        &mut self,
        fields: &Fields<'_>,
        index: u64,
        meter: &mut Meter,
    ) -> Result<Arc<Transforms>, CommandError> {
        let at = usize::try_from(index)
            .ok()
            .filter(|at| *at < self.profiles.len())
            .ok_or_else(|| {
                fields.wrong(format!("profile {index}, which no record before defines"))
            })?;
        if let Some(Some(compiled)) = self.compiled.get(at) {
            return Ok(Arc::clone(compiled));
        }
        // The transforms keep a copy of the profile's bytes as their source,
        // charged before it is made.
        meter.spend(self.profiles[at].len() as u64)?;
        let compiled = Transforms::compile_bytes(&self.profiles[at])
            .ok_or_else(|| fields.wrong("a profile that compiles no transform"))?;
        meter.spend(compiled.bytes_bound() as u64)?;
        let compiled = Arc::new(compiled);
        self.compiled[at] = Some(Arc::clone(&compiled));
        Ok(compiled)
    }

    pub(crate) fn read_space(
        &mut self,
        fields: &Fields<'_>,
        events: &EventReader,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let limit = meter.budgets().spaces;
        let (space, depth) = match fields.unsigned(0)? {
            0 => (ColorSpace::DeviceGray, 0),
            1 => (ColorSpace::DeviceRgb, 0),
            2 => (ColorSpace::DeviceCmyk, 0),
            3 => {
                let (base, depth) = self.space_copy(fields, 1, meter)?;
                let high = fields.unsigned32(3)?;
                (
                    ColorSpace::Indexed {
                        base: Box::new(base),
                        lookup: fields.bytes(2)?.to_vec(),
                        high,
                    },
                    depth + 1,
                )
            }
            4 => {
                let components = usize::try_from(fields.unsigned(1)?)
                    .ok()
                    .filter(|n| *n <= 32)
                    .ok_or_else(|| fields.wrong("a separation of more than 32 components"))?;
                let (alternate, mut depth) = self.space_copy(fields, 2, meter)?;
                let tint = self.function_copy(fields, 3, events, meter)?;
                let kinds = fields.naturals(4)?;
                let mut names = fields.strings(5)?.iter();
                let mut colorants = Vec::with_capacity(kinds.len());
                for &kind in kinds {
                    colorants.push(match kind {
                        0 => Colorant::Named(
                            names
                                .next()
                                .ok_or_else(|| fields.wrong("a named colorant with no name"))?
                                .clone(),
                        ),
                        1 => Colorant::All,
                        2 => Colorant::None,
                        other => return Err(fields.wrong(format!("a colorant kind {other}"))),
                    });
                }
                if names.next().is_some() {
                    return Err(fields.wrong("colorant names past the named colorants"));
                }
                let separation_names = fields.strings(7)?;
                let separation_spaces = fields.naturals(8)?;
                if separation_names.len() != separation_spaces.len() {
                    return Err(fields.wrong("separations whose names and spaces disagree"));
                }
                let mut separations = Vec::with_capacity(separation_names.len());
                for (name, &index) in separation_names.iter().zip(separation_spaces) {
                    let (space, at_depth, weight) = self.space_at(fields, index)?;
                    if !meter.can_hold(*weight) {
                        return Err(CommandError::OverBudget { budget: "bytes" });
                    }
                    depth = depth.max(*at_depth);
                    separations.push((name.clone(), space.clone()));
                }
                let process = match fields.reference(9)? {
                    None => None,
                    Some(index) => {
                        let (space, at_depth, weight) = self.space_at(fields, index)?;
                        if !meter.can_hold(*weight) {
                            return Err(CommandError::OverBudget { budget: "bytes" });
                        }
                        depth = depth.max(*at_depth);
                        Some(Process {
                            space: Box::new(space.clone()),
                            components: fields.strings(10)?.to_vec(),
                        })
                    }
                };
                (
                    ColorSpace::Separation {
                        components,
                        alternate: Box::new(alternate),
                        tint: Box::new(tint),
                        colorants: Colorants {
                            names: colorants,
                            n_channel: fields.flag(6)?,
                            separations,
                            process,
                        },
                    },
                    depth + 1,
                )
            }
            5 => {
                let index = fields.unsigned(1)?;
                let transforms = self.transforms(fields, index, meter)?;
                let components = usize::try_from(fields.unsigned(2)?)
                    .ok()
                    .filter(|n| (1..=4).contains(n))
                    .ok_or_else(|| {
                        fields.wrong("an ICC space of other than one to four components")
                    })?;
                (
                    ColorSpace::Icc {
                        transforms,
                        components,
                    },
                    0,
                )
            }
            6 => {
                let components = usize::try_from(fields.unsigned(1)?)
                    .ok()
                    .filter(|n| *n <= 32)
                    .ok_or_else(|| {
                        fields.wrong("an approximated space of more than 32 components")
                    })?;
                (ColorSpace::Approximated { components }, 0)
            }
            7 => {
                let white = fields.exact_reals(1, 3)?;
                (
                    ColorSpace::CalGray {
                        white: [white[0], white[1], white[2]],
                        gamma: fields.real(2)?,
                    },
                    0,
                )
            }
            8 => {
                let white = fields.exact_reals(1, 3)?;
                let gamma = fields.exact_reals(2, 3)?;
                let matrix = fields.exact_reals(3, 9)?;
                let mut m = [0.0; 9];
                m.copy_from_slice(matrix);
                (
                    ColorSpace::CalRgb {
                        white: [white[0], white[1], white[2]],
                        gamma: [gamma[0], gamma[1], gamma[2]],
                        matrix: m,
                    },
                    0,
                )
            }
            9 => {
                let range = fields.exact_reals(1, 4)?;
                (
                    ColorSpace::Lab {
                        range: [range[0], range[1], range[2], range[3]],
                    },
                    0,
                )
            }
            10 => match fields.reference(1)? {
                None => (ColorSpace::Pattern { base: None }, 0),
                Some(_) => {
                    let (base, depth) = self.space_copy(fields, 1, meter)?;
                    (
                        ColorSpace::Pattern {
                            base: Some(Box::new(base)),
                        },
                        depth + 1,
                    )
                }
            },
            other => return Err(fields.wrong(format!("a colour space of type {other}"))),
        };
        if depth > limit {
            return Err(CommandError::OverBudget { budget: "spaces" });
        }
        let weight = space_weight(&space);
        meter.spend(weight)?;
        self.spaces.push((space, depth, weight));
        Ok(())
    }

    pub(crate) fn read_shading(
        &mut self,
        fields: &Fields<'_>,
        events: &EventReader,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let extend = |at: usize| -> Result<(bool, bool), CommandError> {
            match fields.naturals(at)? {
                [a, b] if *a <= 1 && *b <= 1 => Ok((*a == 1, *b == 1)),
                _ => Err(fields.wrong("an extend of other than two flags")),
            }
        };
        let shading = match fields.unsigned(0)? {
            1 => {
                let (space, _) = self.space_copy(fields, 1, meter)?;
                let function = self.function_copy(fields, 2, events, meter)?;
                let domain = fields.exact_reals(3, 4)?;
                Shading::FunctionBased {
                    space,
                    function,
                    domain: [domain[0], domain[1], domain[2], domain[3]],
                }
            }
            2 => {
                let (space, _) = self.space_copy(fields, 1, meter)?;
                let function = self.function_copy(fields, 2, events, meter)?;
                let c = fields.exact_reals(3, 4)?;
                Shading::Axial {
                    space,
                    function,
                    coords: [c[0], c[1], c[2], c[3]],
                    extend: extend(4)?,
                }
            }
            3 => {
                let (space, _) = self.space_copy(fields, 1, meter)?;
                let function = self.function_copy(fields, 2, events, meter)?;
                let c = fields.exact_reals(3, 6)?;
                Shading::Radial {
                    space,
                    function,
                    coords: [c[0], c[1], c[2], c[3], c[4], c[5]],
                    extend: extend(4)?,
                }
            }
            4 => {
                let (space, _) = self.space_copy(fields, 1, meter)?;
                let function = match fields.reference(2)? {
                    None => None,
                    Some(_) => Some(self.function_copy(fields, 2, events, meter)?),
                };
                let components = usize::try_from(fields.unsigned(6)?)
                    .ok()
                    .filter(|n| (1..=32).contains(n))
                    .ok_or_else(|| fields.wrong("a mesh of other than one to 32 components"))?;
                let positions = fields.reals(7)?;
                if positions.len() % 2 != 0 {
                    return Err(fields.wrong("mesh positions of an odd count"));
                }
                let vertices = positions.len() / 2;
                let values = fields.reals(8)?;
                if values.len() != vertices.saturating_mul(components) {
                    return Err(fields.wrong("mesh values of another count than its vertices"));
                }
                let indices = fields.naturals(9)?;
                if indices.len() % 3 != 0 {
                    return Err(fields.wrong("mesh triangles of a count not a multiple of three"));
                }
                let mut triangles = Vec::with_capacity(indices.len() / 3);
                for corner in indices.chunks_exact(3) {
                    let at = |i: u64| {
                        u32::try_from(i)
                            .ok()
                            .filter(|i| (*i as usize) < vertices)
                            .ok_or_else(|| fields.wrong("a mesh triangle past its vertices"))
                    };
                    triangles.push([at(corner[0])?, at(corner[1])?, at(corner[2])?]);
                }
                let points = fields.reals(10)?;
                let corners = fields.reals(11)?;
                if points.len() % 32 != 0 {
                    return Err(fields.wrong("patch points of a count not a multiple of 32"));
                }
                let patches_count = points.len() / 32;
                if corners.len() != patches_count.saturating_mul(4).saturating_mul(components) {
                    return Err(fields.wrong("patch corners of another count than its patches"));
                }
                let per = 4 * components;
                let patches = (0..patches_count)
                    .map(|p| {
                        let v = &points[p * 32..p * 32 + 32];
                        let mut grid = [[(0.0, 0.0); 4]; 4];
                        for (u, row) in grid.iter_mut().enumerate() {
                            for (w, point) in row.iter_mut().enumerate() {
                                let at = (u * 4 + w) * 2;
                                *point = (v[at], v[at + 1]);
                            }
                        }
                        Patch {
                            points: grid,
                            corners: corners[p * per..p * per + per].to_vec(),
                        }
                    })
                    .collect();
                Shading::Mesh(Box::new(Mesh {
                    kind: fields.signed(5)?,
                    space,
                    function,
                    components,
                    positions: positions.chunks_exact(2).map(|p| (p[0], p[1])).collect(),
                    values: values.to_vec(),
                    triangles,
                    patches,
                }))
            }
            other => return Err(fields.wrong(format!("a shading of type {other}"))),
        };
        let weight = shading_weight(&shading);
        meter.spend(weight)?;
        self.shadings.push((Arc::new(shading), weight));
        Ok(())
    }

    /// The shading a field names, shared.
    pub(crate) fn shading_at(
        &self,
        fields: &Fields<'_>,
        at: usize,
    ) -> Result<Arc<Shading>, CommandError> {
        let index = fields.unsigned(at)?;
        usize::try_from(index)
            .ok()
            .and_then(|i| self.shadings.get(i))
            .map(|(shading, _)| Arc::clone(shading))
            .ok_or_else(|| fields.wrong(format!("shading {index}, which no record before defines")))
    }
}

/// The bytes a shading holds beside its space and function, which are
/// charged where they are copied.
fn shading_weight(shading: &Shading) -> u64 {
    let own = core::mem::size_of::<Shading>() as u64;
    own.saturating_add(match shading {
        Shading::Mesh(mesh) => (core::mem::size_of::<Mesh>() as u64)
            .saturating_add((mesh.positions.len() as u64).saturating_mul(16))
            .saturating_add((mesh.values.len() as u64).saturating_mul(8))
            .saturating_add((mesh.triangles.len() as u64).saturating_mul(12))
            .saturating_add(
                (mesh.patches.len() as u64)
                    .saturating_mul(core::mem::size_of::<Patch>() as u64 + 32),
            ),
        _ => 0,
    })
}

// ---- shadings, patterns and cells -----------------------------------------------------

/// `GlyphSource::shading`'s answer for one name in one scope: a shading
/// definition, a type this engine does not paint, or nothing.
#[derive(Clone, Debug)]
pub(crate) enum ShadingAnswer {
    None,
    Unsupported(i64),
    Shading(Arc<Shading>),
}

impl ShadingAnswer {
    pub(crate) fn handed(&self) -> Result<Option<Shading>, i64> {
        match self {
            ShadingAnswer::None => Ok(None),
            ShadingAnswer::Unsupported(kind) => Err(*kind),
            ShadingAnswer::Shading(shading) => Ok(Some((**shading).clone())),
        }
    }
}

/// One name's shading in one scope.
#[derive(Clone, Debug)]
pub(crate) struct ShadingEntry {
    pub(crate) scope: u64,
    pub(crate) name: Vec<u8>,
    pub(crate) answer: Answered<ShadingAnswer>,
}

/// `GlyphSource::pattern`'s answer for one name in one scope.
#[derive(Clone, Debug)]
pub(crate) enum PatternAnswer {
    None,
    Unsupported,
    Shading(Arc<Shading>, Matrix),
    Tiling(TilingPattern),
}

impl PatternAnswer {
    pub(crate) fn handed(&self) -> Option<PatternPaint> {
        match self {
            PatternAnswer::None => None,
            PatternAnswer::Unsupported => Some(PatternPaint::Unsupported),
            PatternAnswer::Shading(shading, matrix) => Some(PatternPaint::Shading(
                Box::new((**shading).clone()),
                *matrix,
            )),
            PatternAnswer::Tiling(tiling) => Some(PatternPaint::Tiling(Box::new(*tiling))),
        }
    }
}

/// One name's pattern in one scope.
#[derive(Clone, Debug)]
pub(crate) struct PatternEntry {
    pub(crate) scope: u64,
    pub(crate) name: Vec<u8>,
    pub(crate) answer: Answered<PatternAnswer>,
}

/// One tiling pattern's cell in one scope: whether it brings resources of
/// its own, and its nested stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellEntry {
    pub(crate) scope: u64,
    pub(crate) name: Vec<u8>,
    pub(crate) own: bool,
    pub(crate) stream: Arc<[u8]>,
}

impl CellEntry {
    fn record(&self) -> Record {
        Record::new(CommandKind::Cell)
            .with(Field::Unsigned(self.scope))
            .with(Field::bytes(&self.name))
            .with(flag(self.own))
            .with(Field::Bytes(Arc::clone(&self.stream)))
    }

    fn read(fields: &Fields<'_>) -> Result<CellEntry, CommandError> {
        Ok(CellEntry {
            scope: fields.unsigned(0)?,
            name: fields.bytes(1)?.to_vec(),
            own: fields.flag(2)?,
            stream: Arc::clone(fields.bytes(3)?),
        })
    }
}

// ---- the tables together ---------------------------------------------------------------

/// Every table a stream holds, in the order its records stand.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tables {
    pub(crate) scopes: Vec<ScopeEntry>,
    pub(crate) outlines: Vec<OutlineEntry>,
    pub(crate) identities: Vec<IdentityEntry>,
    pub(crate) images: Vec<ImageEntry>,
    pub(crate) decodes: Vec<DecodeEntry>,
    pub(crate) shadings: Vec<ShadingEntry>,
    pub(crate) patterns: Vec<PatternEntry>,
    pub(crate) cells: Vec<CellEntry>,
}

impl Tables {
    /// The tables' records, after the definitions they refer to, which go to
    /// `definitions` as each is met.
    pub(crate) fn records(
        &self,
        definitions: &mut Definitions,
        events: &mut EventWriter,
    ) -> Vec<Record> {
        let mut out = Vec::new();
        for (at, scope) in self.scopes.iter().enumerate() {
            out.push(scope.record(at as u64 + 1));
        }
        out.extend(self.outlines.iter().map(OutlineEntry::record));
        out.extend(self.identities.iter().map(IdentityEntry::record));
        out.extend(self.images.iter().map(ImageEntry::record));
        out.extend(self.decodes.iter().map(DecodeEntry::record));
        for entry in &self.shadings {
            let record = Record::new(CommandKind::Shading)
                .with(Field::Unsigned(entry.scope))
                .with(Field::bytes(&entry.name));
            let record = match &entry.answer.value {
                ShadingAnswer::None => record
                    .with(Field::Unsigned(0))
                    .with(Field::Nothing)
                    .with(Field::Nothing),
                ShadingAnswer::Unsupported(kind) => record
                    .with(Field::Unsigned(1))
                    .with(Field::Signed(*kind))
                    .with(Field::Nothing),
                ShadingAnswer::Shading(shading) => {
                    let definition = definitions.shading(shading, events);
                    record
                        .with(Field::Unsigned(2))
                        .with(Field::Nothing)
                        .with(Field::Unsigned(definition))
                }
            };
            out.push(entry.answer.notes.clone().with(record));
        }
        for entry in &self.patterns {
            let record = Record::new(CommandKind::Pattern)
                .with(Field::Unsigned(entry.scope))
                .with(Field::bytes(&entry.name));
            let nothing = |record: Record, count: usize| {
                let mut record = record;
                record
                    .fields
                    .extend(std::iter::repeat_n(Field::Nothing, count));
                record
            };
            let record = match &entry.answer.value {
                PatternAnswer::None => nothing(record.with(Field::Unsigned(0)), 6),
                PatternAnswer::Unsupported => nothing(record.with(Field::Unsigned(1)), 6),
                PatternAnswer::Shading(shading, matrix) => {
                    let definition = definitions.shading(shading, events);
                    nothing(
                        record
                            .with(Field::Unsigned(2))
                            .with(Field::Unsigned(definition))
                            .with(matrix_to_field(matrix)),
                        4,
                    )
                }
                PatternAnswer::Tiling(tiling) => record
                    .with(Field::Unsigned(3))
                    .with(Field::Nothing)
                    .with(matrix_to_field(&tiling.matrix))
                    .with(reals(&tiling.bbox))
                    .with(Field::Real(tiling.xstep))
                    .with(Field::Real(tiling.ystep))
                    .with(flag(tiling.uncolored)),
            };
            out.push(entry.answer.notes.clone().with(record));
        }
        out.extend(self.cells.iter().map(CellEntry::record));
        out
    }

    /// Reads `record`, one of the tables' kinds, into its table.
    pub(crate) fn read(
        &mut self,
        kind: CommandKind,
        fields: &Fields<'_>,
        defined: &Defined,
        meter: &mut Meter,
    ) -> Result<(), CommandError> {
        let scopes = self.scopes.len() as u64;
        let scope_of = |at: usize| -> Result<u64, CommandError> {
            let scope = fields.unsigned(at)?;
            if scope > scopes {
                return Err(fields.wrong(format!("scope {scope}, which no record before defines")));
            }
            Ok(scope)
        };
        match kind {
            CommandKind::Scope => {
                let entry = ScopeEntry::read(fields, scopes + 1)?;
                self.scopes.push(entry);
            }
            CommandKind::Outline => {
                let entry = OutlineEntry::read(fields, meter)?;
                scope_of(0)?;
                self.outlines.push(entry);
            }
            CommandKind::FontIdentity => {
                scope_of(0)?;
                self.identities.push(IdentityEntry::read(fields)?);
            }
            CommandKind::Image | CommandKind::InlineImage => {
                scope_of(0)?;
                self.images
                    .push(ImageEntry::read(fields, kind == CommandKind::InlineImage)?);
            }
            CommandKind::ImageDecode => {
                self.decodes
                    .push(DecodeEntry::read(fields, self.images.len() as u64)?);
            }
            CommandKind::Shading => {
                let scope = scope_of(0)?;
                let answer = match fields.unsigned(2)? {
                    0 => ShadingAnswer::None,
                    1 => ShadingAnswer::Unsupported(fields.signed(3)?),
                    2 => ShadingAnswer::Shading(defined.shading_at(fields, 4)?),
                    other => return Err(fields.wrong(format!("a shading answer {other}"))),
                };
                self.shadings.push(ShadingEntry {
                    scope,
                    name: fields.bytes(1)?.to_vec(),
                    answer: Answered {
                        value: answer,
                        notes: Notes::read(fields, 5)?,
                    },
                });
            }
            CommandKind::Pattern => {
                let scope = scope_of(0)?;
                let answer = match fields.unsigned(2)? {
                    0 => PatternAnswer::None,
                    1 => PatternAnswer::Unsupported,
                    2 => PatternAnswer::Shading(
                        defined.shading_at(fields, 3)?,
                        matrix_at(fields, 4)?,
                    ),
                    3 => {
                        let bbox = fields.exact_reals(5, 4)?;
                        PatternAnswer::Tiling(TilingPattern {
                            matrix: matrix_at(fields, 4)?,
                            bbox: [bbox[0], bbox[1], bbox[2], bbox[3]],
                            xstep: fields.real(6)?,
                            ystep: fields.real(7)?,
                            uncolored: fields.flag(8)?,
                        })
                    }
                    other => return Err(fields.wrong(format!("a pattern answer {other}"))),
                };
                self.patterns.push(PatternEntry {
                    scope,
                    name: fields.bytes(1)?.to_vec(),
                    answer: Answered {
                        value: answer,
                        notes: Notes::read(fields, 9)?,
                    },
                });
            }
            CommandKind::Cell => {
                scope_of(0)?;
                self.cells.push(CellEntry::read(fields)?);
            }
            other => return Err(fields.wrong(format!("{} is no table", other.name()))),
        }
        Ok(())
    }
}
