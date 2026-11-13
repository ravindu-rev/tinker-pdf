//! Font measurement, behind [`FontMetrics`].
//!
//! Design: `docs/design/font-metrics.md`; feature documentation:
//! `docs/features/fonts.md`.
//!
//! A program's bytes — a TrueType or OpenType face, a collection's face, a
//! WOFF or WOFF 2.0 container, a Type 42 program, a bare CFF, a Type 1
//! program — or a document's embedded program, read through
//! [`crate::DocumentFont::metrics`], answer what a caller measures text with:
//! a glyph's advance and box, the font's box, the glyph a character draws,
//! every character there is one for, glyph names both ways, 9.8.2's flags and
//! a text's unshaped length. Each answer's definition, unit and source table
//! are the design's, and each is a public commitment.
//!
//! # What is read when
//!
//! The names, the character map, the font's box and its flags are read once,
//! when the bytes are, in work linear in the program. A glyph's advance and
//! box are read when a glyph is asked for, 256 glyphs at a time and kept, one
//! parse of the program a block: no answer runs every charstring of a face,
//! and a face's every glyph costs one parse a block.
//!
//! # What it reports
//!
//! Nothing. A [`FontMetrics`] holds no document, and the one
//! [`crate::DocumentFont::metrics`] builds decodes its stream through
//! `CosDocument::stream_decoded_unrecorded`, which records nothing, so
//! measuring a font changes nothing [`crate::Document::warnings`] answers.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use tinker_pdf_cos::Rect;
use tinker_pdf_font::{woff, BuiltInEncoding, Cff, Cff2, Outline, Segment, Sfnt, Type1, Type42};

/// How many glyphs a block of per-glyph answers holds, read with one parse.
const BLOCK: usize = 256;

/// The most bytes a WOFF or WOFF 2.0 container is decoded to: the bound the
/// EPUB reader holds a book's faces to.
const MAX_UNPACKED_FACE: usize = 16 << 20;

const TAG_GLYF: u32 = u32::from_be_bytes(*b"glyf");
const TAG_CFF: u32 = u32::from_be_bytes(*b"CFF ");
const TAG_CFF2: u32 = u32::from_be_bytes(*b"CFF2");

/// A font program, measured (`docs/design/font-metrics.md`).
///
/// Owns its bytes, and is cheap to clone: the clones share what has been
/// read. Every answer in font units is in the face's own units
/// ([`FontMetrics::units_per_em`]); every `_at` answer is in text space at a
/// size, the unit the size is in.
#[derive(Clone)]
pub struct FontMetrics {
    inner: Arc<Measured>,
}

/// The program's format, as [`FontMetrics::format`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FontFormat {
    /// An sfnt with `glyf` outlines, or with no outline table this engine
    /// reads; a Type 42 program's face.
    TrueType,
    /// An sfnt with `CFF ` outlines (`OTTO`).
    OpenTypeCff,
    /// An sfnt with `CFF2` outlines.
    OpenTypeCff2,
    /// A bare CFF whose Top DICT has no `ROS`.
    Cff,
    /// A bare CFF whose Top DICT has `ROS` (TN #5176 Table 10).
    CidKeyedCff,
    /// A Type 1 program: PFA, PFB, or as a PDF embeds it.
    Type1,
}

/// Why bytes were not measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FontMetricsError {
    /// No reader of this engine takes the bytes as a font program.
    NotAFont,
    /// A collection has no face of the index asked for.
    NoSuchFace {
        /// How many faces it has.
        faces: u32,
    },
    /// A WOFF or WOFF 2.0 container that does not decode within its limit.
    Container,
    /// The document embeds no program for the font.
    NotEmbedded,
    /// The document's program stream does not decode.
    Undecodable,
}

impl core::fmt::Display for FontMetricsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FontMetricsError::NotAFont => {
                f.write_str("no reader takes the bytes as a font program")
            }
            FontMetricsError::NoSuchFace { faces } => {
                write!(f, "the collection has no such face; it has {faces}")
            }
            FontMetricsError::Container => {
                f.write_str("the WOFF container does not decode within its limit")
            }
            FontMetricsError::NotEmbedded => f.write_str("the document embeds no program"),
            FontMetricsError::Undecodable => f.write_str("the program's stream does not decode"),
        }
    }
}

impl std::error::Error for FontMetricsError {}

/// 9.8.2's font flags, Table 121's bits at their positions: bit 1 the lowest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontFlags(u32);

impl FontFlags {
    /// Bit 1: all glyphs have the same width.
    pub const FIXED_PITCH: FontFlags = FontFlags(1);
    /// Bit 2: glyphs have serifs.
    pub const SERIF: FontFlags = FontFlags(1 << 1);
    /// Bit 3: the font contains glyphs outside the Standard Latin character
    /// set, or does not use its standard names.
    pub const SYMBOLIC: FontFlags = FontFlags(1 << 2);
    /// Bit 4: glyphs resemble cursive handwriting.
    pub const SCRIPT: FontFlags = FontFlags(1 << 3);
    /// Bit 6: the font uses the Standard Latin character set or a subset of
    /// it, with its standard names.
    pub const NONSYMBOLIC: FontFlags = FontFlags(1 << 5);
    /// Bit 7: glyphs have dominant vertical strokes that are slanted.
    pub const ITALIC: FontFlags = FontFlags(1 << 6);
    /// Bit 17: the font contains no lowercase letters.
    pub const ALL_CAP: FontFlags = FontFlags(1 << 16);
    /// Bit 18: the font's lowercase letters are small capitals.
    pub const SMALL_CAP: FontFlags = FontFlags(1 << 17);
    /// Bit 19: bold glyphs are thickened at small sizes.
    pub const FORCE_BOLD: FontFlags = FontFlags(1 << 18);

    /// The flags as a descriptor's `/Flags` integer writes them.
    #[must_use]
    pub fn bits(self) -> u32 {
        self.0
    }

    /// Whether every flag of `other` is set here.
    #[must_use]
    pub fn contains(self, other: FontFlags) -> bool {
        self.0 & other.0 == other.0
    }

    fn with(self, flag: FontFlags, set: bool) -> FontFlags {
        if set {
            FontFlags(self.0 | flag.0)
        } else {
            self
        }
    }
}

/// What is read once, when the bytes are.
struct Measured {
    program: Program,
    format: FontFormat,
    glyph_count: u16,
    units_per_em: f64,
    /// Font units to the em for the font as a whole: what its box scales by.
    font_scale: f64,
    font_box: Option<[f64; 4]>,
    /// Every character with a glyph, ascending, and the glyph.
    chars: Vec<(char, u16)>,
    /// Each glyph's name, by glyph.
    names: Vec<Option<String>>,
    /// The lowest glyph each name is given to.
    by_name: BTreeMap<String, u16>,
    /// The glyph a character with none draws.
    notdef: Option<u16>,
    flags: FontFlags,
    /// Each block of [`BLOCK`] glyphs' advances and boxes, read at the first
    /// question about one of them.
    blocks: Vec<OnceLock<Vec<GlyphFacts>>>,
}

/// The program, as each block of per-glyph answers reads it again.
enum Program {
    /// An sfnt's bytes — decoded from a container, or reassembled from a
    /// Type 42 program, where they were one — and its face in a collection.
    Sfnt {
        bytes: Vec<u8>,
        face: u32,
        outlines: Outlines,
    },
    /// A bare CFF.
    Cff(Vec<u8>),
    /// A Type 1 program, read.
    Type1(Box<Type1<'static>>),
}

/// An sfnt's outline table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outlines {
    Glyf,
    Cff,
    Cff2,
    None,
}

/// One glyph's advance and box, in font units, and its scale to the em.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GlyphFacts {
    advance: Option<f64>,
    bounds: Option<[f64; 4]>,
    scale: f64,
}

impl FontMetrics {
    /// Measures a font program's bytes: an sfnt (a collection's first face),
    /// a WOFF or WOFF 2.0 container's face, a Type 42 program's face, a bare
    /// CFF or a Type 1 program, recognised in that order
    /// (`docs/design/font-metrics.md`, *The type and its constructors*).
    ///
    /// # Errors
    ///
    /// [`FontMetricsError::Container`] for a container that does not decode,
    /// and [`FontMetricsError::NotAFont`] for bytes no reader takes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<FontMetrics, FontMetricsError> {
        FontMetrics::read(bytes, 0)
    }

    /// Measures one face of a collection, by its index in the `ttcf` header,
    /// whether the bytes are the collection or a WOFF 2.0 container of one;
    /// any other program is face 0 alone.
    ///
    /// # Errors
    ///
    /// [`FontMetricsError::NoSuchFace`] for an index past the faces, and
    /// [`FontMetrics::from_bytes`]'s.
    pub fn from_collection(bytes: Vec<u8>, face: u32) -> Result<FontMetrics, FontMetricsError> {
        FontMetrics::read(bytes, face)
    }

    fn read(mut bytes: Vec<u8>, face: u32) -> Result<FontMetrics, FontMetricsError> {
        if woff::packaging(&bytes).is_some() {
            bytes =
                woff::decode(&bytes, MAX_UNPACKED_FACE).map_err(|_| FontMetricsError::Container)?;
        }
        let mut charstring_names = None;
        if Type42::is_type42(&bytes) {
            let program = Type42::parse(&bytes).map_err(|_| FontMetricsError::NotAFont)?;
            charstring_names = Some(
                program
                    .glyph_names()
                    .map(|(name, glyph)| (name.to_vec(), glyph))
                    .collect::<Vec<_>>(),
            );
            bytes = program.face().to_vec();
        }
        let faces = Sfnt::faces(&bytes);
        let measured = if faces > 0 || bytes.get(..4) == Some(b"ttcf") {
            if face != 0 && face >= faces {
                return Err(FontMetricsError::NoSuchFace { faces });
            }
            measure_sfnt(bytes, face, charstring_names)?
        } else if face != 0 {
            return Err(FontMetricsError::NoSuchFace { faces: 1 });
        } else if is_cff(&bytes) && Cff::parse(&bytes).is_some() {
            measure_cff(bytes)?
        } else if let Some(program) = Type1::parse(&bytes) {
            measure_type1(program)
        } else {
            return Err(FontMetricsError::NotAFont);
        };
        Ok(FontMetrics {
            inner: Arc::new(measured),
        })
    }

    /// The program's format.
    #[must_use]
    pub fn format(&self) -> FontFormat {
        self.inner.format
    }

    /// How many glyphs the program holds: an sfnt's `maxp` count, or what its
    /// outline table holds where it has no `maxp` to say; a CFF's
    /// CharStrings count; a Type 1 program's `/CharStrings` entries, which
    /// are its glyphs in the order the dictionary lists them.
    #[must_use]
    pub fn glyph_count(&self) -> u16 {
        self.inner.glyph_count
    }

    /// The font units in an em: an sfnt's `head`'s units per em, a CFF's or
    /// a Type 1 program's font matrix's `a`, inverted. A CID-keyed CFF's Font
    /// DICT may scale a glyph otherwise, which every `_at` answer takes.
    #[must_use]
    pub fn units_per_em(&self) -> f64 {
        self.inner.units_per_em
    }

    /// A glyph's horizontal advance, in font units: `hmtx`'s for an sfnt,
    /// the charstring's width for a CFF or a Type 1 program. `None` for a
    /// glyph at or past the count, or one whose metrics the program does not
    /// hold.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<f64> {
        self.facts(glyph)?.advance
    }

    /// [`FontMetrics::advance`] in text space at `size`.
    #[must_use]
    pub fn advance_at(&self, glyph: u16, size: f64) -> Option<f64> {
        let facts = self.facts(glyph)?;
        Some(facts.advance? * facts.scale * size)
    }

    /// A glyph's box, in font units: a `glyf` glyph's header as written, its
    /// corners ordered; a CFF, `CFF2` or Type 1 glyph's the smallest
    /// rectangle enclosing the shape its charstring draws (9.2.4). A glyph
    /// that draws nothing is bounded at its origin. `None` for a glyph at or
    /// past the count, or one the program cannot draw.
    #[must_use]
    pub fn glyph_box(&self, glyph: u16) -> Option<Rect> {
        self.facts(glyph)?.bounds.map(rect)
    }

    /// [`FontMetrics::glyph_box`] in text space at `size`.
    #[must_use]
    pub fn glyph_box_at(&self, glyph: u16, size: f64) -> Option<Rect> {
        let facts = self.facts(glyph)?;
        let [x0, y0, x1, y1] = facts.bounds?;
        let scaled = |value: f64| value * facts.scale * size;
        Some(rect([scaled(x0), scaled(y0), scaled(x1), scaled(y1)]))
    }

    /// The font's box, in font units: `head`'s, the CFF Top DICT's
    /// `FontBBox`, or a Type 1 program's `/FontBBox`, the last two `0 0 0 0`
    /// where absent. `None` for an sfnt with no `head` to state one.
    #[must_use]
    pub fn font_box(&self) -> Option<Rect> {
        self.inner.font_box.map(rect)
    }

    /// [`FontMetrics::font_box`] in text space at `size`.
    #[must_use]
    pub fn font_box_at(&self, size: f64) -> Option<Rect> {
        let [x0, y0, x1, y1] = self.inner.font_box?;
        let scale = self.inner.font_scale;
        let scaled = |value: f64| value * scale * size;
        Some(rect([scaled(x0), scaled(y0), scaled(x1), scaled(y1)]))
    }

    /// The glyph a character draws: through an sfnt's text `cmap` subtable,
    /// read as the engine's text lookup reads it, or a name-keyed program's
    /// glyph names; `None` where the face has no glyph for it but glyph 0.
    #[must_use]
    pub fn glyph_for_char(&self, c: char) -> Option<u16> {
        let chars = &self.inner.chars;
        chars
            .binary_search_by_key(&c, |(c, _)| *c)
            .ok()
            .and_then(|at| chars.get(at))
            .map(|(_, glyph)| *glyph)
    }

    /// Whether the face has a glyph for a character:
    /// [`FontMetrics::glyph_for_char`] answers one.
    #[must_use]
    pub fn has_glyph(&self, c: char) -> bool {
        self.glyph_for_char(c).is_some()
    }

    /// Every character the face has a glyph for, ascending.
    #[must_use]
    pub fn code_points(&self) -> Vec<char> {
        self.inner.chars.iter().map(|(c, _)| *c).collect()
    }

    /// A glyph's name: `post`'s for a `glyf` or `CFF2` face, the charset's
    /// for a CFF, the `/CharStrings` key for a Type 1 or Type 42 program.
    #[must_use]
    pub fn glyph_name(&self, glyph: u16) -> Option<&str> {
        self.inner
            .names
            .get(usize::from(glyph))
            .and_then(Option::as_deref)
    }

    /// The lowest glyph a name is given to.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u16> {
        self.inner.by_name.get(name).copied()
    }

    /// 9.8.2's flags, each from the table that states it.
    #[must_use]
    pub fn flags(&self) -> FontFlags {
        self.inner.flags
    }

    /// A text's length in text space at `size`, unshaped: each character's
    /// glyph, or the face's `.notdef` where it has none, its advance in font
    /// units times the glyph's scale, summed left to right from zero, then
    /// times `size`. No ligature, kerning or reordering.
    #[must_use]
    pub fn text_length(&self, text: &str, size: f64) -> f64 {
        let mut total = 0.0;
        for c in text.chars() {
            let glyph = self.glyph_for_char(c).or(self.inner.notdef);
            let Some(facts) = glyph.and_then(|glyph| self.facts(glyph)) else {
                continue;
            };
            if let Some(advance) = facts.advance {
                total += advance * facts.scale;
            }
        }
        total * size
    }

    /// A glyph's advance and box, read with its block.
    fn facts(&self, glyph: u16) -> Option<GlyphFacts> {
        if glyph >= self.inner.glyph_count {
            return None;
        }
        let index = usize::from(glyph);
        let block = self.inner.blocks.get(index / BLOCK)?;
        let facts = block.get_or_init(|| self.inner.read_block(index / BLOCK));
        facts.get(index % BLOCK).copied()
    }
}

impl core::fmt::Debug for FontMetrics {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FontMetrics")
            .field("format", &self.inner.format)
            .field("glyph_count", &self.inner.glyph_count)
            .field("units_per_em", &self.inner.units_per_em)
            .finish()
    }
}

impl Measured {
    /// The advances and boxes of block `block`'s glyphs, one parse of the
    /// program for all of them.
    fn read_block(&self, block: usize) -> Vec<GlyphFacts> {
        let first = block * BLOCK;
        let last = (first + BLOCK).min(usize::from(self.glyph_count));
        let glyphs = (first..last).filter_map(|glyph| u16::try_from(glyph).ok());
        let none = GlyphFacts {
            advance: None,
            bounds: None,
            scale: 1.0 / 1000.0,
        };
        match &self.program {
            Program::Sfnt {
                bytes,
                face,
                outlines,
            } => {
                let Some(sfnt) = Sfnt::parse_face(bytes, *face) else {
                    return glyphs.map(|_| none).collect();
                };
                let scale = 1.0 / f64::from(sfnt.units_per_em);
                let cff = (*outlines == Outlines::Cff)
                    .then(|| sfnt.table(TAG_CFF).and_then(Cff::parse))
                    .flatten();
                let cff2 = (*outlines == Outlines::Cff2)
                    .then(|| {
                        sfnt.table(TAG_CFF2)
                            .and_then(|table| Cff2::parse(table).ok())
                    })
                    .flatten();
                glyphs
                    .map(|glyph| GlyphFacts {
                        advance: sfnt.advance(glyph).map(f64::from),
                        bounds: match outlines {
                            Outlines::Glyf => sfnt.glyph_box(glyph).map(|b| {
                                ordered([
                                    f64::from(b.x_min),
                                    f64::from(b.y_min),
                                    f64::from(b.x_max),
                                    f64::from(b.y_max),
                                ])
                            }),
                            Outlines::Cff => cff
                                .as_ref()
                                .and_then(|cff| cff.outline(glyph))
                                .map(|outline| shape_bounds(&outline)),
                            Outlines::Cff2 => cff2
                                .as_ref()
                                .and_then(|cff2| cff2.outline(glyph).ok())
                                .map(|outline| shape_bounds(&outline)),
                            Outlines::None => None,
                        },
                        scale,
                    })
                    .collect()
            }
            Program::Cff(bytes) => {
                let Some(cff) = Cff::parse(bytes) else {
                    return glyphs.map(|_| none).collect();
                };
                glyphs
                    .map(|glyph| GlyphFacts {
                        advance: cff.advance(glyph),
                        bounds: cff.outline(glyph).map(|outline| shape_bounds(&outline)),
                        scale: matrix_scale(cff.font_matrix_for(glyph)[0]),
                    })
                    .collect()
            }
            Program::Type1(program) => {
                let scale = matrix_scale(program.font_matrix[0]);
                glyphs
                    .map(|glyph| GlyphFacts {
                        advance: program.advance(glyph),
                        bounds: program.outline(glyph).map(|outline| shape_bounds(&outline)),
                        scale,
                    })
                    .collect()
            }
        }
    }
}

/// Whether bytes begin as a CFF header does (TN #5176 section 6): major
/// version 1, a header of at least four bytes, and an offset size of one to
/// four.
fn is_cff(bytes: &[u8]) -> bool {
    bytes.first() == Some(&1)
        && bytes.get(2).is_some_and(|size| *size >= 4)
        && bytes.get(3).is_some_and(|size| (1..=4).contains(size))
}

/// A font matrix's `a` as a scale: itself where finite and not zero, else
/// 1/1000, glyph space's own ratio (9.2.4).
fn matrix_scale(a: f64) -> f64 {
    if a.is_finite() && a != 0.0 {
        a
    } else {
        0.001
    }
}

/// Four edges, `[x0, y0, x1, y1]`, with each pair ordered.
fn ordered([x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

/// Four edges as the object model's rectangle, its corners ordered.
fn rect(edges: [f64; 4]) -> Rect {
    let [x0, y0, x1, y1] = ordered(edges);
    Rect { x0, y0, x1, y1 }
}

fn measure_sfnt(
    bytes: Vec<u8>,
    face: u32,
    charstring_names: Option<Vec<(Vec<u8>, u16)>>,
) -> Result<Measured, FontMetricsError> {
    let sfnt = Sfnt::parse_face(&bytes, face).ok_or(if face == 0 {
        FontMetricsError::NotAFont
    } else {
        FontMetricsError::NoSuchFace {
            faces: Sfnt::faces(&bytes),
        }
    })?;
    let outlines = if sfnt.table(TAG_GLYF).is_some() {
        Outlines::Glyf
    } else if sfnt.table(TAG_CFF).is_some() {
        Outlines::Cff
    } else if sfnt.table(TAG_CFF2).is_some() {
        Outlines::Cff2
    } else {
        Outlines::None
    };
    let format = match outlines {
        Outlines::Glyf | Outlines::None => FontFormat::TrueType,
        Outlines::Cff => FontFormat::OpenTypeCff,
        Outlines::Cff2 => FontFormat::OpenTypeCff2,
    };
    let cff = (outlines == Outlines::Cff)
        .then(|| sfnt.table(TAG_CFF).and_then(Cff::parse))
        .flatten();
    let glyph_count = sfnt
        .glyph_count()
        .or_else(|| match outlines {
            Outlines::Glyf => sfnt.loca_glyph_count(),
            Outlines::Cff => cff
                .as_ref()
                .map(|cff| u16::try_from(cff.glyph_count()).unwrap_or(u16::MAX)),
            Outlines::Cff2 => sfnt
                .table(TAG_CFF2)
                .and_then(|table| Cff2::parse(table).ok())
                .map(|cff2| u16::try_from(cff2.glyph_count()).unwrap_or(u16::MAX)),
            Outlines::None => None,
        })
        .unwrap_or(0);
    let units_per_em = f64::from(sfnt.units_per_em);
    let font_box = sfnt.font_box().map(|b| {
        ordered([
            f64::from(b.x_min),
            f64::from(b.y_min),
            f64::from(b.x_max),
            f64::from(b.y_max),
        ])
    });
    let chars: Vec<(char, u16)> = sfnt
        .char_map()
        .into_iter()
        .filter(|(_, glyph)| *glyph < glyph_count)
        .collect();

    // Names: a Type 42 program's `/CharStrings` keys, the first written for
    // a glyph; an OpenType CFF face's charset; and `post` otherwise.
    let count = usize::from(glyph_count);
    let mut names: Vec<Option<String>> = vec![None; count];
    let from_post = |glyph: usize| {
        u16::try_from(glyph)
            .ok()
            .and_then(|glyph| sfnt.glyph_name(glyph))
            .map(str::to_owned)
    };
    // Every name given to a glyph: a Type 42 program's keys give a glyph
    // each of theirs, where a glyph's name is only the first.
    let mut given: Vec<(String, u16)> = Vec::new();
    match (&charstring_names, &cff) {
        (Some(keys), _) => {
            for (name, glyph) in keys {
                let (Ok(name), Some(slot)) = (
                    String::from_utf8(name.clone()),
                    names.get_mut(usize::from(*glyph)),
                ) else {
                    continue;
                };
                given.push((name.clone(), *glyph));
                if slot.is_none() {
                    *slot = Some(name);
                }
            }
            for (glyph, slot) in names.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = from_post(glyph);
                }
            }
        }
        (None, Some(cff)) => {
            for (glyph, slot) in names.iter_mut().enumerate() {
                *slot = u16::try_from(glyph)
                    .ok()
                    .and_then(|glyph| cff.glyph_name(glyph))
                    .map(str::to_owned);
            }
        }
        (None, None) => {
            for (glyph, slot) in names.iter_mut().enumerate() {
                *slot = from_post(glyph);
            }
        }
    }
    given.extend(named_pairs(&names));

    // 9.8.2, each flag from the table that states it.
    let style = sfnt.style();
    let class = style.family_class.map(|class| (class as u16 >> 8) as u8);
    let panose = style.panose;
    let serif = match class {
        Some(1..=5 | 7) => true,
        Some(0) => panose.is_some_and(|digits| digits[0] == 2 && (2..=10).contains(&digits[1])),
        _ => false,
    };
    let script = match class {
        Some(10) => true,
        Some(0) => panose.is_some_and(|digits| digits[0] == 3),
        _ => false,
    };
    let italic = style.italic_angle.is_some_and(|angle| angle != 0.0)
        || style
            .selection
            .is_some_and(|selection| selection & (1 | (1 << 9)) != 0)
        || style.mac_style.is_some_and(|mac| mac & 2 != 0);
    let latin = latin();
    let nonsymbolic = sfnt.text_subtable() != Some((3, 0))
        && !chars.is_empty()
        && chars.iter().all(|(c, _)| latin.chars.contains(c));
    let force_bold = cff.as_ref().is_some_and(|cff| cff.style().force_bold);
    let flags = FontFlags::default()
        .with(FontFlags::FIXED_PITCH, style.fixed_pitch == Some(true))
        .with(FontFlags::SERIF, serif)
        .with(FontFlags::SCRIPT, script)
        .with(FontFlags::ITALIC, italic)
        .with(FontFlags::FORCE_BOLD, force_bold);
    let flags = symbolic_or_not(flags, nonsymbolic);
    drop(cff);
    drop(sfnt);

    Ok(Measured {
        program: Program::Sfnt {
            bytes,
            face,
            outlines,
        },
        format,
        glyph_count,
        units_per_em,
        font_scale: 1.0 / units_per_em,
        font_box,
        chars,
        by_name: lowest_glyphs(given),
        names,
        notdef: (glyph_count > 0).then_some(0),
        flags,
        blocks: blocks(glyph_count),
    })
}

fn measure_cff(bytes: Vec<u8>) -> Result<Measured, FontMetricsError> {
    let cff = Cff::parse(&bytes).ok_or(FontMetricsError::NotAFont)?;
    let glyph_count = u16::try_from(cff.glyph_count()).unwrap_or(u16::MAX);
    let names: Vec<Option<String>> = (0..glyph_count)
        .map(|glyph| cff.glyph_name(glyph).map(str::to_owned))
        .collect();
    let style = cff.style();
    let scale = matrix_scale(cff.font_matrix[0]);
    let chars = if cff.is_cid() {
        Vec::new()
    } else {
        name_chars(&names)
    };
    let nonsymbolic = !cff.is_cid() && standard_names(&names);
    let flags = FontFlags::default()
        .with(FontFlags::FIXED_PITCH, style.is_fixed_pitch)
        .with(FontFlags::ITALIC, style.italic_angle != 0.0)
        .with(FontFlags::FORCE_BOLD, style.force_bold);
    let format = if cff.is_cid() {
        FontFormat::CidKeyedCff
    } else {
        FontFormat::Cff
    };
    let font_box = Some(ordered(style.font_bbox));
    drop(cff);
    Ok(Measured {
        program: Program::Cff(bytes),
        format,
        glyph_count,
        units_per_em: 1.0 / scale,
        font_scale: scale,
        font_box,
        chars,
        by_name: lowest_glyphs(named_pairs(&names)),
        names,
        notdef: (glyph_count > 0).then_some(0),
        flags: symbolic_or_not(flags, nonsymbolic),
        blocks: blocks(glyph_count),
    })
}

fn measure_type1(program: Type1<'static>) -> Measured {
    let glyph_count = u16::try_from(program.glyph_count()).unwrap_or(u16::MAX);
    let names: Vec<Option<String>> = (0..usize::from(glyph_count))
        .map(|glyph| {
            program
                .glyph_name(glyph)
                .and_then(|name| String::from_utf8(name.to_vec()).ok())
        })
        .collect();
    let style = program.style().clone();
    let scale = matrix_scale(program.font_matrix[0]);
    let flags = FontFlags::default()
        .with(FontFlags::FIXED_PITCH, style.is_fixed_pitch)
        .with(FontFlags::ITALIC, style.italic_angle != 0.0)
        .with(FontFlags::FORCE_BOLD, style.force_bold);
    let by_name = lowest_glyphs(named_pairs(&names));
    Measured {
        format: FontFormat::Type1,
        glyph_count,
        units_per_em: 1.0 / scale,
        font_scale: scale,
        font_box: Some(ordered(style.font_bbox)),
        chars: name_chars(&names),
        notdef: by_name.get(".notdef").copied(),
        by_name,
        flags: symbolic_or_not(flags, standard_names(&names)),
        names,
        blocks: blocks(glyph_count),
        program: Program::Type1(Box::new(program)),
    }
}

/// Exactly one of Symbolic and Nonsymbolic (9.8.2).
fn symbolic_or_not(flags: FontFlags, nonsymbolic: bool) -> FontFlags {
    flags
        .with(FontFlags::NONSYMBOLIC, nonsymbolic)
        .with(FontFlags::SYMBOLIC, !nonsymbolic)
}

/// An empty cell for each block of glyphs.
fn blocks(glyph_count: u16) -> Vec<OnceLock<Vec<GlyphFacts>>> {
    let count = usize::from(glyph_count).div_ceil(BLOCK);
    (0..count).map(|_| OnceLock::new()).collect()
}

/// The lowest glyph each name is given to.
fn lowest_glyphs(given: impl IntoIterator<Item = (String, u16)>) -> BTreeMap<String, u16> {
    let mut out: BTreeMap<String, u16> = BTreeMap::new();
    for (name, glyph) in given {
        out.entry(name)
            .and_modify(|lowest| *lowest = (*lowest).min(glyph))
            .or_insert(glyph);
    }
    out
}

/// Each glyph's own name, with the glyph.
fn named_pairs(names: &[Option<String>]) -> Vec<(String, u16)> {
    names
        .iter()
        .enumerate()
        .filter_map(|(glyph, name)| Some((name.clone()?, u16::try_from(glyph).ok()?)))
        .collect()
}

/// Whether a name-keyed program uses the Standard Latin set's names alone
/// (9.8.2's Nonsymbolic): it names a glyph besides `.notdef`, and every name
/// but `.notdef` is a name of Table D.2.
fn standard_names(names: &[Option<String>]) -> bool {
    let latin = latin();
    let mut named = false;
    for name in names {
        match name.as_deref() {
            Some(".notdef") => {}
            Some(name) if latin.names.contains_key(name) => named = true,
            _ => return false,
        }
    }
    named
}

/// Every character a name-keyed program's names stand for, ascending, with
/// the lowest glyph whose name stands for it.
fn name_chars(names: &[Option<String>]) -> Vec<(char, u16)> {
    let mut out: BTreeMap<char, u16> = BTreeMap::new();
    for (glyph, name) in names.iter().enumerate() {
        let (Some(c), Ok(glyph)) = (name.as_deref().and_then(name_char), u16::try_from(glyph))
        else {
            continue;
        };
        out.entry(c).or_insert(glyph);
    }
    out.into_iter().collect()
}

/// The character a glyph name stands for (`docs/design/font-metrics.md`,
/// *Characters*): its part before the first period, holding no underscore,
/// as a name of Table D.2's Standard Latin set, of D.5's Symbol set or
/// D.6's ZapfDingbats set, or as the Glyph List specification's `uniXXXX`
/// (four upper-case hexadecimal digits) or `uXXXX` to `uXXXXXX`, a scalar
/// value that is not a surrogate.
fn name_char(name: &str) -> Option<char> {
    let base = name.split('.').next()?;
    if base.is_empty() || base.contains('_') {
        return None;
    }
    if let Some(c) = latin().names.get(base) {
        return Some(*c);
    }
    if let Some(c) = BuiltInEncoding::Symbol.unicode_of(base) {
        return Some(c);
    }
    if let Some(c) = BuiltInEncoding::ZapfDingbats.unicode_of(base) {
        return Some(c);
    }
    let hex = |digits: &str| -> Option<char> {
        if !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        {
            return None;
        }
        char::from_u32(u32::from_str_radix(digits, 16).ok()?)
    };
    if let Some(digits) = base.strip_prefix("uni") {
        return if digits.len() == 4 { hex(digits) } else { None };
    }
    let digits = base.strip_prefix('u')?;
    if (4..=6).contains(&digits.len()) {
        hex(digits)
    } else {
        None
    }
}

/// Table D.2's Standard Latin character set: each name and the character it
/// names, and the characters.
struct Latin {
    names: BTreeMap<&'static str, char>,
    chars: std::collections::BTreeSet<char>,
}

/// [`STANDARD_LATIN`], keyed both ways, built once.
fn latin() -> &'static Latin {
    static LATIN: OnceLock<Latin> = OnceLock::new();
    LATIN.get_or_init(|| Latin {
        names: STANDARD_LATIN.iter().copied().collect(),
        chars: STANDARD_LATIN.iter().map(|(_, c)| *c).collect(),
    })
}

/// Table D.2, the Standard Latin character set: its 229 names, each with the
/// character its CHAR column shows, by name. Transcribed whole, because the
/// font crate's base encodings name only StandardEncoding's codes and the
/// printable ASCII ones; the second codes D.2's notes give `space` and
/// `hyphen` in WinAnsiEncoding and MacRomanEncoding name no character of
/// their own.
const STANDARD_LATIN: [(&str, char); 229] = [
    ("A", '\u{0041}'),
    ("AE", '\u{00C6}'),
    ("Aacute", '\u{00C1}'),
    ("Acircumflex", '\u{00C2}'),
    ("Adieresis", '\u{00C4}'),
    ("Agrave", '\u{00C0}'),
    ("Aring", '\u{00C5}'),
    ("Atilde", '\u{00C3}'),
    ("B", '\u{0042}'),
    ("C", '\u{0043}'),
    ("Ccedilla", '\u{00C7}'),
    ("D", '\u{0044}'),
    ("E", '\u{0045}'),
    ("Eacute", '\u{00C9}'),
    ("Ecircumflex", '\u{00CA}'),
    ("Edieresis", '\u{00CB}'),
    ("Egrave", '\u{00C8}'),
    ("Eth", '\u{00D0}'),
    ("Euro", '\u{20AC}'),
    ("F", '\u{0046}'),
    ("G", '\u{0047}'),
    ("H", '\u{0048}'),
    ("I", '\u{0049}'),
    ("Iacute", '\u{00CD}'),
    ("Icircumflex", '\u{00CE}'),
    ("Idieresis", '\u{00CF}'),
    ("Igrave", '\u{00CC}'),
    ("J", '\u{004A}'),
    ("K", '\u{004B}'),
    ("L", '\u{004C}'),
    ("Lslash", '\u{0141}'),
    ("M", '\u{004D}'),
    ("N", '\u{004E}'),
    ("Ntilde", '\u{00D1}'),
    ("O", '\u{004F}'),
    ("OE", '\u{0152}'),
    ("Oacute", '\u{00D3}'),
    ("Ocircumflex", '\u{00D4}'),
    ("Odieresis", '\u{00D6}'),
    ("Ograve", '\u{00D2}'),
    ("Oslash", '\u{00D8}'),
    ("Otilde", '\u{00D5}'),
    ("P", '\u{0050}'),
    ("Q", '\u{0051}'),
    ("R", '\u{0052}'),
    ("S", '\u{0053}'),
    ("Scaron", '\u{0160}'),
    ("T", '\u{0054}'),
    ("Thorn", '\u{00DE}'),
    ("U", '\u{0055}'),
    ("Uacute", '\u{00DA}'),
    ("Ucircumflex", '\u{00DB}'),
    ("Udieresis", '\u{00DC}'),
    ("Ugrave", '\u{00D9}'),
    ("V", '\u{0056}'),
    ("W", '\u{0057}'),
    ("X", '\u{0058}'),
    ("Y", '\u{0059}'),
    ("Yacute", '\u{00DD}'),
    ("Ydieresis", '\u{0178}'),
    ("Z", '\u{005A}'),
    ("Zcaron", '\u{017D}'),
    ("a", '\u{0061}'),
    ("aacute", '\u{00E1}'),
    ("acircumflex", '\u{00E2}'),
    ("acute", '\u{00B4}'),
    ("adieresis", '\u{00E4}'),
    ("ae", '\u{00E6}'),
    ("agrave", '\u{00E0}'),
    ("ampersand", '\u{0026}'),
    ("aring", '\u{00E5}'),
    ("asciicircum", '\u{005E}'),
    ("asciitilde", '\u{007E}'),
    ("asterisk", '\u{002A}'),
    ("at", '\u{0040}'),
    ("atilde", '\u{00E3}'),
    ("b", '\u{0062}'),
    ("backslash", '\u{005C}'),
    ("bar", '\u{007C}'),
    ("braceleft", '\u{007B}'),
    ("braceright", '\u{007D}'),
    ("bracketleft", '\u{005B}'),
    ("bracketright", '\u{005D}'),
    ("breve", '\u{02D8}'),
    ("brokenbar", '\u{00A6}'),
    ("bullet", '\u{2022}'),
    ("c", '\u{0063}'),
    ("caron", '\u{02C7}'),
    ("ccedilla", '\u{00E7}'),
    ("cedilla", '\u{00B8}'),
    ("cent", '\u{00A2}'),
    ("circumflex", '\u{02C6}'),
    ("colon", '\u{003A}'),
    ("comma", '\u{002C}'),
    ("copyright", '\u{00A9}'),
    ("currency", '\u{00A4}'),
    ("d", '\u{0064}'),
    ("dagger", '\u{2020}'),
    ("daggerdbl", '\u{2021}'),
    ("degree", '\u{00B0}'),
    ("dieresis", '\u{00A8}'),
    ("divide", '\u{00F7}'),
    ("dollar", '\u{0024}'),
    ("dotaccent", '\u{02D9}'),
    ("dotlessi", '\u{0131}'),
    ("e", '\u{0065}'),
    ("eacute", '\u{00E9}'),
    ("ecircumflex", '\u{00EA}'),
    ("edieresis", '\u{00EB}'),
    ("egrave", '\u{00E8}'),
    ("eight", '\u{0038}'),
    ("ellipsis", '\u{2026}'),
    ("emdash", '\u{2014}'),
    ("endash", '\u{2013}'),
    ("equal", '\u{003D}'),
    ("eth", '\u{00F0}'),
    ("exclam", '\u{0021}'),
    ("exclamdown", '\u{00A1}'),
    ("f", '\u{0066}'),
    ("fi", '\u{FB01}'),
    ("five", '\u{0035}'),
    ("fl", '\u{FB02}'),
    ("florin", '\u{0192}'),
    ("four", '\u{0034}'),
    ("fraction", '\u{2044}'),
    ("g", '\u{0067}'),
    ("germandbls", '\u{00DF}'),
    ("grave", '\u{0060}'),
    ("greater", '\u{003E}'),
    ("guillemotleft", '\u{00AB}'),
    ("guillemotright", '\u{00BB}'),
    ("guilsinglleft", '\u{2039}'),
    ("guilsinglright", '\u{203A}'),
    ("h", '\u{0068}'),
    ("hungarumlaut", '\u{02DD}'),
    ("hyphen", '\u{002D}'),
    ("i", '\u{0069}'),
    ("iacute", '\u{00ED}'),
    ("icircumflex", '\u{00EE}'),
    ("idieresis", '\u{00EF}'),
    ("igrave", '\u{00EC}'),
    ("j", '\u{006A}'),
    ("k", '\u{006B}'),
    ("l", '\u{006C}'),
    ("less", '\u{003C}'),
    ("logicalnot", '\u{00AC}'),
    ("lslash", '\u{0142}'),
    ("m", '\u{006D}'),
    ("macron", '\u{00AF}'),
    ("minus", '\u{2212}'),
    ("mu", '\u{00B5}'),
    ("multiply", '\u{00D7}'),
    ("n", '\u{006E}'),
    ("nine", '\u{0039}'),
    ("ntilde", '\u{00F1}'),
    ("numbersign", '\u{0023}'),
    ("o", '\u{006F}'),
    ("oacute", '\u{00F3}'),
    ("ocircumflex", '\u{00F4}'),
    ("odieresis", '\u{00F6}'),
    ("oe", '\u{0153}'),
    ("ogonek", '\u{02DB}'),
    ("ograve", '\u{00F2}'),
    ("one", '\u{0031}'),
    ("onehalf", '\u{00BD}'),
    ("onequarter", '\u{00BC}'),
    ("onesuperior", '\u{00B9}'),
    ("ordfeminine", '\u{00AA}'),
    ("ordmasculine", '\u{00BA}'),
    ("oslash", '\u{00F8}'),
    ("otilde", '\u{00F5}'),
    ("p", '\u{0070}'),
    ("paragraph", '\u{00B6}'),
    ("parenleft", '\u{0028}'),
    ("parenright", '\u{0029}'),
    ("percent", '\u{0025}'),
    ("period", '\u{002E}'),
    ("periodcentered", '\u{00B7}'),
    ("perthousand", '\u{2030}'),
    ("plus", '\u{002B}'),
    ("plusminus", '\u{00B1}'),
    ("q", '\u{0071}'),
    ("question", '\u{003F}'),
    ("questiondown", '\u{00BF}'),
    ("quotedbl", '\u{0022}'),
    ("quotedblbase", '\u{201E}'),
    ("quotedblleft", '\u{201C}'),
    ("quotedblright", '\u{201D}'),
    ("quoteleft", '\u{2018}'),
    ("quoteright", '\u{2019}'),
    ("quotesinglbase", '\u{201A}'),
    ("quotesingle", '\u{0027}'),
    ("r", '\u{0072}'),
    ("registered", '\u{00AE}'),
    ("ring", '\u{02DA}'),
    ("s", '\u{0073}'),
    ("scaron", '\u{0161}'),
    ("section", '\u{00A7}'),
    ("semicolon", '\u{003B}'),
    ("seven", '\u{0037}'),
    ("six", '\u{0036}'),
    ("slash", '\u{002F}'),
    ("space", '\u{0020}'),
    ("sterling", '\u{00A3}'),
    ("t", '\u{0074}'),
    ("thorn", '\u{00FE}'),
    ("three", '\u{0033}'),
    ("threequarters", '\u{00BE}'),
    ("threesuperior", '\u{00B3}'),
    ("tilde", '\u{02DC}'),
    ("trademark", '\u{2122}'),
    ("two", '\u{0032}'),
    ("twosuperior", '\u{00B2}'),
    ("u", '\u{0075}'),
    ("uacute", '\u{00FA}'),
    ("ucircumflex", '\u{00FB}'),
    ("udieresis", '\u{00FC}'),
    ("ugrave", '\u{00F9}'),
    ("underscore", '\u{005F}'),
    ("v", '\u{0076}'),
    ("w", '\u{0077}'),
    ("x", '\u{0078}'),
    ("y", '\u{0079}'),
    ("yacute", '\u{00FD}'),
    ("ydieresis", '\u{00FF}'),
    ("yen", '\u{00A5}'),
    ("z", '\u{007A}'),
    ("zcaron", '\u{017E}'),
    ("zero", '\u{0030}'),
];

/// The smallest rectangle enclosing the shape an outline draws (9.2.4): each
/// contour that draws something — its start, every segment's end, and each
/// curve's extremes inside it — and the origin for an outline that draws
/// nothing (`docs/design/font-metrics.md`, *Glyph boxes*).
fn shape_bounds(outline: &Outline) -> [f64; 4] {
    let mut edges = Edges::default();
    let mut current = (0.0f64, 0.0f64);
    let mut start = (0.0f64, 0.0f64);
    let mut started = false;
    for segment in &outline.segments {
        match *segment {
            Segment::MoveTo { x, y } => {
                current = (x, y);
                start = (x, y);
                started = false;
            }
            Segment::LineTo { x, y } => {
                if !started {
                    edges.point(start.0, start.1);
                    started = true;
                }
                edges.point(x, y);
                current = (x, y);
            }
            Segment::QuadTo { cx, cy, x, y } => {
                if !started {
                    edges.point(start.0, start.1);
                    started = true;
                }
                edges.point(x, y);
                if let Some(t) = quadratic_extreme(current.0, cx, x) {
                    edges.x(quadratic_at(current.0, cx, x, t));
                }
                if let Some(t) = quadratic_extreme(current.1, cy, y) {
                    edges.y(quadratic_at(current.1, cy, y, t));
                }
                current = (x, y);
            }
            Segment::CurveTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                if !started {
                    edges.point(start.0, start.1);
                    started = true;
                }
                edges.point(x, y);
                for t in cubic_extremes(current.0, c1x, c2x, x) {
                    edges.x(cubic_at(current.0, c1x, c2x, x, t));
                }
                for t in cubic_extremes(current.1, c1y, c2y, y) {
                    edges.y(cubic_at(current.1, c1y, c2y, y, t));
                }
                current = (x, y);
            }
            Segment::Close => current = start,
        }
    }
    edges.box_or_origin()
}

/// The extremes a box accumulates, each axis on its own.
#[derive(Default)]
struct Edges {
    x: Option<(f64, f64)>,
    y: Option<(f64, f64)>,
}

impl Edges {
    fn point(&mut self, x: f64, y: f64) {
        self.x(x);
        self.y(y);
    }

    fn x(&mut self, value: f64) {
        Edges::widen(&mut self.x, value);
    }

    fn y(&mut self, value: f64) {
        Edges::widen(&mut self.y, value);
    }

    /// A value past the box widens it; one that is not finite is passed over.
    fn widen(axis: &mut Option<(f64, f64)>, value: f64) {
        if !value.is_finite() {
            return;
        }
        *axis = Some(match *axis {
            None => (value, value),
            Some((low, high)) => (low.min(value), high.max(value)),
        });
    }

    fn box_or_origin(&self) -> [f64; 4] {
        match (self.x, self.y) {
            (Some((x0, x1)), Some((y0, y1))) => [x0, y0, x1, y1],
            _ => [0.0; 4],
        }
    }
}

/// The parameters in `(0, 1)` where a cubic's coordinate, of control values
/// `p0` to `p3`, turns: the roots of `A t² + B t + C`, with `d0 = p1 - p0`,
/// `d1 = p2 - p1`, `d2 = p3 - p2`, `A = d0 - 2 d1 + d2`, `B = 2 (d1 - d0)` and
/// `C = d0`.
fn cubic_extremes(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let (d0, d1, d2) = (p1 - p0, p2 - p1, p3 - p2);
    let a = d0 - 2.0 * d1 + d2;
    let b = 2.0 * (d1 - d0);
    let c = d0;
    let mut roots = Vec::with_capacity(2);
    if a == 0.0 {
        if b != 0.0 {
            roots.push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            roots.push((-b + root) / (2.0 * a));
            roots.push((-b - root) / (2.0 * a));
        }
    }
    roots.retain(|t| *t > 0.0 && *t < 1.0);
    roots
}

/// A cubic's coordinate at `t`, by de Casteljau's construction.
fn cubic_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let q0 = p0 + (p1 - p0) * t;
    let q1 = p1 + (p2 - p1) * t;
    let q2 = p2 + (p3 - p2) * t;
    let r0 = q0 + (q1 - q0) * t;
    let r1 = q1 + (q2 - q1) * t;
    r0 + (r1 - r0) * t
}

/// The parameter in `(0, 1)` where a quadratic's coordinate, of control
/// values `p0` to `p2`, turns: `(p0 - p1) / (p0 - 2 p1 + p2)`.
fn quadratic_extreme(p0: f64, p1: f64, p2: f64) -> Option<f64> {
    let denominator = p0 - 2.0 * p1 + p2;
    if denominator == 0.0 {
        return None;
    }
    let t = (p0 - p1) / denominator;
    (t > 0.0 && t < 1.0).then_some(t)
}

/// A quadratic's coordinate at `t`, by de Casteljau's construction.
fn quadratic_at(p0: f64, p1: f64, p2: f64, t: f64) -> f64 {
    let q0 = p0 + (p1 - p0) * t;
    let q1 = p1 + (p2 - p1) * t;
    q0 + (q1 - q0) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f64, y0: f64, size: f64) -> Outline {
        let mut outline = Outline::default();
        outline.push(Segment::MoveTo { x: x0, y: y0 });
        outline.push(Segment::LineTo {
            x: x0 + size,
            y: y0,
        });
        outline.push(Segment::LineTo {
            x: x0 + size,
            y: y0 + size,
        });
        outline.push(Segment::LineTo {
            x: x0,
            y: y0 + size,
        });
        outline.push(Segment::Close);
        outline
    }

    /// A box of points is its points; a moveto that draws nothing bounds
    /// nothing; an outline that draws nothing is bounded at its origin.
    #[test]
    fn a_shape_is_bounded_by_what_it_draws() {
        assert_eq!(
            shape_bounds(&square(10.0, 20.0, 100.0)),
            [10.0, 20.0, 110.0, 120.0]
        );
        let mut trailing = square(10.0, 20.0, 100.0);
        trailing.push(Segment::MoveTo {
            x: 900.0,
            y: -900.0,
        });
        assert_eq!(
            shape_bounds(&trailing),
            [10.0, 20.0, 110.0, 120.0],
            "a last moveto draws nothing"
        );
        let mut lone = Outline::default();
        lone.push(Segment::MoveTo { x: 50.0, y: 50.0 });
        lone.push(Segment::Close);
        assert_eq!(shape_bounds(&lone), [0.0; 4]);
        assert_eq!(shape_bounds(&Outline::default()), [0.0; 4]);
        // A line drawn with no moveto starts at the origin.
        let mut unmoved = Outline::default();
        unmoved.push(Segment::LineTo { x: 30.0, y: 40.0 });
        assert_eq!(shape_bounds(&unmoved), [0.0, 0.0, 30.0, 40.0]);
    }

    /// A curve's extreme between its points widens the box to the extreme,
    /// and its control points, off the shape, do not.
    #[test]
    fn a_curves_extreme_bounds_it_and_its_control_points_do_not() {
        // The arc from (0, 0) to (100, 0) over control points at height 100:
        // its top is at t = 1/2, three quarters of the control height.
        let mut arch = Outline::default();
        arch.push(Segment::MoveTo { x: 0.0, y: 0.0 });
        arch.push(Segment::CurveTo {
            c1x: 0.0,
            c1y: 100.0,
            c2x: 100.0,
            c2y: 100.0,
            x: 100.0,
            y: 0.0,
        });
        arch.push(Segment::Close);
        assert_eq!(shape_bounds(&arch), [0.0, 0.0, 100.0, 75.0]);
        // A quadratic's top is half its control point's height.
        let mut hump = Outline::default();
        hump.push(Segment::MoveTo { x: 0.0, y: 0.0 });
        hump.push(Segment::QuadTo {
            cx: 50.0,
            cy: 100.0,
            x: 100.0,
            y: 0.0,
        });
        assert_eq!(shape_bounds(&hump), [0.0, 0.0, 100.0, 50.0]);
        // A curve whose extremes are its ends adds nothing between them.
        assert_eq!(cubic_extremes(0.0, 10.0, 20.0, 30.0), Vec::<f64>::new());
        // A point that is not finite is passed over.
        let mut broken = square(0.0, 0.0, 10.0);
        broken.push(Segment::LineTo {
            x: f64::NAN,
            y: f64::INFINITY,
        });
        assert_eq!(shape_bounds(&broken), [0.0, 0.0, 10.0, 10.0]);
    }

    /// The Glyph List specification's algorithmic names, and the Annex D
    /// names, read as the design states; a suffix and a ligature's
    /// underscore.
    #[test]
    fn a_name_stands_for_its_character() {
        assert_eq!(name_char("A"), Some('A'));
        assert_eq!(name_char("quoteright"), Some('\u{2019}'));
        assert_eq!(name_char("Euro"), Some('\u{20AC}'));
        assert_eq!(name_char("space"), Some(' '), "StandardEncoding's space");
        assert_eq!(name_char("alpha"), Some('\u{03B1}'), "D.5's Symbol set");
        assert_eq!(name_char("a1"), Some('\u{2701}'), "D.6's ZapfDingbats set");
        assert_eq!(name_char("uni20AC"), Some('\u{20AC}'));
        assert_eq!(name_char("u1F600"), Some('\u{1F600}'));
        assert_eq!(name_char("a.alt"), Some('a'), "a suffix is dropped");
        assert_eq!(name_char("f_i"), None, "a ligature is several characters");
        assert_eq!(name_char("uni2269FE00"), None, "a sequence is several");
        assert_eq!(name_char("uni20ac"), None, "lower-case digits");
        assert_eq!(name_char("uniD800"), None, "a surrogate");
        assert_eq!(name_char("u110000"), None, "past U+10FFFF");
        assert_eq!(name_char(".notdef"), None);
        assert_eq!(name_char("g42"), None);
        assert_eq!(name_char("sun"), None, "in no table this reads");
    }

    /// D.2's set, transcribed, against the font crate's base encodings,
    /// transcribed apart: StandardEncoding's every name with the character
    /// its code stands for, and every character WinAnsiEncoding encodes. It
    /// holds its names' characters and not the second codes its notes give
    /// `space` and `hyphen`.
    #[test]
    fn the_standard_latin_set_is_d2s_names() {
        use tinker_pdf_font::{base_char, base_glyph_name, BaseEncoding};

        let latin = latin();
        assert_eq!(latin.names.len(), STANDARD_LATIN.len(), "each name once");
        for code in 0..=u8::MAX {
            if let (Some(name), Some(c)) = (
                base_glyph_name(BaseEncoding::Standard, code),
                base_char(BaseEncoding::Standard, code),
            ) {
                assert_eq!(
                    latin.names.get(name),
                    Some(&c),
                    "StandardEncoding's /{name}"
                );
            }
            let encoded =
                base_char(BaseEncoding::WinAnsi, code).filter(|c| *c >= ' ' && *c != '\u{7F}');
            if let Some(c) = encoded {
                assert!(
                    latin.chars.contains(&c),
                    "WinAnsiEncoding's {code:#04X}, {c:?}"
                );
            }
        }
        assert_eq!(latin.names.get("eacute"), Some(&'\u{E9}'));
        assert_eq!(
            latin.names.get("minus"),
            Some(&'\u{2212}'),
            "PDFDocEncoding's"
        );
        assert_eq!(latin.names.get("space"), Some(&' '));
        assert_eq!(latin.names.get("hyphen"), Some(&'-'));
        assert!(
            !latin.chars.contains(&'\u{A0}'),
            "no-break space is no name"
        );
        assert!(!latin.chars.contains(&'\u{AD}'), "soft hyphen is no name");
        assert!(latin.chars.contains(&'\u{0141}'), "Lslash");
        assert!(!latin.chars.contains(&'\u{0410}'), "Cyrillic is outside");
    }

    /// Exactly one of Symbolic and Nonsymbolic, and `contains` reads bits.
    #[test]
    fn the_flags_are_table_121s_bits() {
        let flags = symbolic_or_not(FontFlags::SERIF, true);
        assert_eq!(flags.bits(), 2 | 32);
        assert!(flags.contains(FontFlags::NONSYMBOLIC));
        assert!(!flags.contains(FontFlags::SYMBOLIC));
        let flags = symbolic_or_not(FontFlags::default(), false);
        assert_eq!(flags.bits(), 4);
        assert_eq!(FontFlags::FORCE_BOLD.bits(), 1 << 18, "bit 19");
        assert_eq!(FontFlags::ALL_CAP.bits(), 1 << 16, "bit 17");
    }

    /// Bytes no reader takes are refused, a collection's absent face named,
    /// and a degenerate matrix scales by glyph space's own ratio.
    #[test]
    fn the_constructors_refuse_by_name() {
        assert_eq!(
            FontMetrics::from_bytes(b"not a font".to_vec()).err(),
            Some(FontMetricsError::NotAFont)
        );
        assert_eq!(
            FontMetrics::from_bytes(Vec::new()).err(),
            Some(FontMetricsError::NotAFont)
        );
        assert_eq!(
            FontMetrics::from_collection(b"not a font".to_vec(), 2).err(),
            Some(FontMetricsError::NoSuchFace { faces: 1 })
        );
        assert_eq!(
            FontMetrics::from_bytes(b"wOFF\0\0".to_vec()).err(),
            Some(FontMetricsError::Container)
        );
        assert_eq!(matrix_scale(0.0), 0.001);
        assert_eq!(matrix_scale(f64::NAN), 0.001);
        assert_eq!(matrix_scale(0.0005), 0.0005);
        assert!(!is_cff(b"%!PS"));
        assert!(is_cff(&[1, 0, 4, 1]));
    }
}
