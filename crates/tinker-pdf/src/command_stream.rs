//! A page's drawing as a command stream: versioned bytes a host keeps, sends
//! and reads without the document (`docs/design/command-stream.md`, RC-18).
//!
//! Feature documentation: `docs/features/rendering.md`, "The command
//! stream".
//!
//! # What a stream holds
//!
//! The calls a display list records — `tinker-pdf-content`'s codec writes
//! them, each with its graphics state — and the answers to every question a
//! render of them asks the renderer's `GlyphSource`, in the scope it asks
//! them in: every glyph's outline and its font's identity, every image's
//! decode at every reduction a draw can ask for, every shading and pattern,
//! each tiling cell as a stream of its own, and the form and appearance
//! scopes names resolve in, each answer with what it noted (ruling 10). The
//! page's geometry and its group's space ride in the `Page` record, so a
//! stream draws with no document behind it.
//!
//! # Writing
//!
//! [`DisplayList::command_stream`] writes the list's calls as it holds them,
//! and what its interpretation met after the content, where its renders note
//! it; a list that is not retained interprets the page into the writer, as
//! `DisplayList::to_svg` writes such a page, every note where the
//! interpretation met it (`write.rs`).
//!
//! [`DisplayList::command_stream`]: crate::DisplayList::command_stream

mod read;
mod tables;
mod write;

pub(crate) use read::{parse, Layer, Note, Parsed};
pub(crate) use tables::{
    CellEntry, DecodeEntry, IdentityEntry, ImageAnswer, ImageEntry, ImageKey, Notes, OutlineEntry,
    PageRecord, PatternAnswer, PatternEntry, ScopeEntry, ShadingAnswer, ShadingEntry, Tables,
};
pub(crate) use write::write_list;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tinker_pdf_content::commands::{write_stream, Budgets, Meter};
    use tinker_pdf_content::{replay, RenderingIntent};
    use tinker_pdf_font::{Outline, Segment};
    use tinker_pdf_raster::canvas::{Canvas, Color, PixelFormat};
    use tinker_pdf_render::{
        DecodedImage, FontIdentity, GlyphSource, ImageRequest, PatternPaint, Renderer, Shading,
        Tile, TileRequest,
    };

    use super::*;
    use crate::resources::{paint_cell, PageResources};
    use crate::{Document, RenderOptions};

    /// Liberation Serif, the face the page's text is embedded in.
    const SERIF: &[u8] =
        include_bytes!("../../tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf");

    /// A stream object of `dict` holding `data`.
    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    }

    /// A file of `objects`, numbered from 1, the first the catalog.
    fn file(objects: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// RGB samples of a `w` by `h` image, by formula.
    fn rgb(w: u32, h: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [
                    (x * 255 / w) as u8,
                    (y * 255 / h) as u8,
                    ((x + y) * 9) as u8,
                ]
            })
            .collect()
    }

    /// A 16 by 16 baseline JPEG of [`rgb`]'s samples.
    fn jpeg() -> Vec<u8> {
        use tinker_pdf_filters::{
            jpeg_encode, JpegOptions, JpegQuantisation, JpegSampling, JpegSource, JpegSourceColour,
        };
        let samples = rgb(16, 16);
        jpeg_encode(
            &JpegSource {
                width: 16,
                height: 16,
                colour: JpegSourceColour::Rgb,
                stride: 48,
                data: &samples,
            },
            &JpegOptions {
                quantisation: JpegQuantisation::AnnexK,
                sampling: JpegSampling::FourFourFour,
                restart_interval: 0,
            },
        )
        .expect("the JPEG encodes")
    }

    /// The page every question a render asks is asked on: text in an embedded
    /// face and in a Type 3 font, three kinds of image XObject — RGB samples,
    /// a JPEG and a stencil mask — and an inline image, an axial and a mesh
    /// shading, a tiling pattern whose cell, with resources of its own, draws
    /// an image, and a form with its own resources drawing an image and text.
    fn every_question() -> Vec<u8> {
        let widths: Vec<String> = (32..=126).map(|_| "500".to_string()).collect();
        let mut content = b"q 1 0 0 RG 2 w 10 10 m 190 10 l S Q\n\
             BT /F0 18 Tf 10 170 Td (Hello) Tj ET\n\
             BT /T3 12 Tf 120 170 Td (AA) Tj ET\n\
             q 40 0 0 40 10 110 cm /Im0 Do Q\n\
             q 40 0 0 40 60 110 cm /Im1 Do Q\n\
             q 0 0.5 0 rg 40 0 0 40 110 110 cm /Im2 Do Q\n\
             q 10 10 80 40 re W n /Sh0 sh Q\n\
             q 100 10 90 40 re W n /Sh1 sh Q\n\
             q /Pattern cs /P0 scn 10 60 80 40 re f Q\n\
             q 1 0 0 1 120 60 cm /Fm0 Do Q\n\
             q 20 0 0 20 170 60 cm BI /W 2 /H 2 /CS /G /BPC 8 ID "
            .to_vec();
        content.extend_from_slice(&[0x00, 0x40, 0x80, 0xff]);
        content.extend_from_slice(b" EI Q\n");
        let mesh: Vec<u8> = vec![
            0, 10, 10, 255, 0, 0, 0, 100, 10, 0, 255, 0, 0, 50, 90, 0, 0, 255,
        ];
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources 4 0 R \
              /Contents 5 0 R >>"
                .to_vec(),
            b"<< /Font << /F0 6 0 R /T3 9 0 R >> \
              /XObject << /Im0 11 0 R /Im1 12 0 R /Im2 13 0 R /Fm0 14 0 R >> \
              /Shading << /Sh0 16 0 R /Sh1 17 0 R >> /Pattern << /P0 18 0 R >> >>"
                .to_vec(),
            stream("", &content),
            format!(
                "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 \
                 /LastChar 126 /Widths [{}] /FontDescriptor 7 0 R /Encoding /WinAnsiEncoding >>",
                widths.join(" ")
            )
            .into_bytes(),
            b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 \
              /FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 \
              /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
                .to_vec(),
            stream(&format!("/Length1 {}", SERIF.len()), SERIF),
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
              /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /sq 10 0 R >> \
              /Encoding << /Type /Encoding /Differences [65 /sq] >> /FirstChar 65 \
              /LastChar 65 /Widths [1000] >>"
                .to_vec(),
            stream("", b"1000 0 d0 0 0 800 800 re f"),
            stream(
                "/Type /XObject /Subtype /Image /Width 4 /Height 4 /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8",
                &rgb(4, 4),
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /DCTDecode",
                &jpeg(),
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ImageMask true \
                 /BitsPerComponent 1",
                &[0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81],
            ),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 50 50] \
                 /Resources << /XObject << /ImF 15 0 R >> /Font << /F0 6 0 R >> >>",
                b"q 20 0 0 20 0 0 cm /ImF Do Q BT /F0 10 Tf 0 30 Td (Fm) Tj ET",
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray \
                 /BitsPerComponent 8",
                &[0, 85, 170, 255],
            ),
            b"<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 90 0] \
              /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> \
              /Extend [true true] >>"
                .to_vec(),
            stream(
                "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
                 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 200 0 200 0 1 0 1 0 1]",
                &mesh,
            ),
            stream(
                "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 20 20] /XStep 20 \
                 /YStep 20 /Resources << /XObject << /ImP 19 0 R >> >>",
                b"q 10 0 0 10 0 0 cm /ImP Do Q 0 0 1 rg 10 10 10 10 re f",
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8",
                &rgb(2, 2),
            ),
        ];
        file(&objects)
    }

    /// Where a scope stands: the steps a render takes from the page to it.
    #[derive(Clone, Debug, PartialEq)]
    enum Step {
        Annotation(usize),
        Form(Vec<u8>),
        Cell(Vec<u8>),
    }

    /// One question a render asked, where, and what the page's resources
    /// answered.
    #[derive(Debug)]
    enum Asked {
        Outline(Vec<Step>, u64, u32, Option<Outline>),
        Identity(Vec<Step>, u64, Option<FontIdentity>),
        Image(
            Vec<Step>,
            Vec<u8>,
            ImageRequest,
            RenderingIntent,
            Result<Option<DecodedImage>, String>,
        ),
        Inline(
            Vec<Step>,
            Vec<u8>,
            Vec<u8>,
            ImageRequest,
            RenderingIntent,
            Result<Option<DecodedImage>, String>,
        ),
        Shading(Vec<Step>, Vec<u8>, Result<Option<Shading>, i64>),
        Pattern(Vec<Step>, Vec<u8>, Option<PatternPaint>),
        Tile(Vec<Step>, Vec<u8>, bool),
        FormScope(Vec<Step>, Vec<u8>, bool),
    }

    /// The page's resources, every question to them logged with its answer.
    struct Logging {
        inner: Arc<PageResources>,
        path: Vec<Step>,
        log: Arc<Mutex<Vec<Asked>>>,
    }

    impl Logging {
        fn ask(&self, asked: Asked) {
            if let Ok(mut log) = self.log.lock() {
                log.push(asked);
            }
        }

        fn below(&self, step: Step, inner: Arc<PageResources>) -> Logging {
            let mut path = self.path.clone();
            path.push(step);
            Logging {
                inner,
                path,
                log: Arc::clone(&self.log),
            }
        }
    }

    impl GlyphSource for Logging {
        fn outline(&self, font_id: u64, code: u32) -> Option<Outline> {
            let answer = self.inner.outline(font_id, code);
            self.ask(Asked::Outline(
                self.path.clone(),
                font_id,
                code,
                answer.clone(),
            ));
            answer
        }

        fn font_identity(&self, font_id: u64) -> Option<FontIdentity> {
            let answer = self.inner.font_identity(font_id);
            self.ask(Asked::Identity(self.path.clone(), font_id, answer));
            answer
        }

        fn image(
            &self,
            name: &[u8],
            request: &ImageRequest,
            intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            let answer = self.inner.image(name, request, intent);
            self.ask(Asked::Image(
                self.path.clone(),
                name.to_vec(),
                *request,
                intent,
                answer.clone(),
            ));
            answer
        }

        fn inline_image(
            &self,
            dict: &[u8],
            data: &[u8],
            request: &ImageRequest,
            intent: RenderingIntent,
        ) -> Result<Option<DecodedImage>, String> {
            let answer = self.inner.inline_image(dict, data, request, intent);
            self.ask(Asked::Inline(
                self.path.clone(),
                dict.to_vec(),
                data.to_vec(),
                *request,
                intent,
                answer.clone(),
            ));
            answer
        }

        fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
            let answer = self.inner.shading(name);
            self.ask(Asked::Shading(
                self.path.clone(),
                name.to_vec(),
                answer.clone(),
            ));
            answer
        }

        fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
            let answer = self.inner.pattern(name);
            self.ask(Asked::Pattern(
                self.path.clone(),
                name.to_vec(),
                answer.clone(),
            ));
            answer
        }

        /// The cell drawn as `PageResources::tile` draws it, through a logging
        /// scope over the resources it draws in, so its questions are logged
        /// too.
        fn tile(&self, name: &[u8], request: &TileRequest) -> Option<Tile> {
            let cell = self.inner.cell(name);
            self.ask(Asked::Tile(
                self.path.clone(),
                name.to_vec(),
                cell.is_some(),
            ));
            let (content, own) = cell?;
            let resources = own.map_or_else(|| Arc::clone(&self.inner), Arc::new);
            let below = self.below(Step::Cell(name.to_vec()), Arc::clone(&resources));
            let (canvas, warnings, _, opaque) = paint_cell(request, &below, |renderer| {
                tinker_pdf_content::interpret(
                    &content,
                    tinker_pdf_content::Matrix::IDENTITY,
                    renderer,
                    &*resources,
                );
            });
            Some(Tile {
                canvas: Arc::new(canvas),
                warnings,
                opaque,
            })
        }

        fn form_scope(&self, name: &[u8]) -> Option<Arc<Logging>> {
            let answer = self.inner.form_scope(name);
            self.ask(Asked::FormScope(
                self.path.clone(),
                name.to_vec(),
                answer.is_some(),
            ));
            answer.map(|scope| Arc::new(self.below(Step::Form(name.to_vec()), scope)))
        }
    }

    /// The stream and scope a path reaches.
    fn locate<'p>(parsed: &'p Parsed, path: &[Step]) -> Option<(&'p Parsed, u64)> {
        let mut at = (parsed, 0u64);
        for step in path {
            at = match step {
                Step::Annotation(k) => (at.0, at.0.annotations.get(*k)?.scope),
                Step::Form(name) => {
                    let id = at.0.tables.scopes.iter().position(|scope| {
                        scope.parent == Some(at.1) && scope.name.as_deref() == Some(name.as_slice())
                    })?;
                    (at.0, id as u64 + 1)
                }
                Step::Cell(name) => {
                    let index =
                        at.0.tables
                            .cells
                            .iter()
                            .position(|cell| cell.scope == at.1 && &cell.name == name)?;
                    (at.0.cells.get(index)?.as_ref(), 0)
                }
            };
        }
        Some(at)
    }

    fn same_outline(a: &Option<Outline>, b: &Option<Outline>) -> bool {
        let bits = |s: &Segment| -> Vec<u64> {
            match *s {
                Segment::MoveTo { x, y } => vec![0, x.to_bits(), y.to_bits()],
                Segment::LineTo { x, y } => vec![1, x.to_bits(), y.to_bits()],
                Segment::QuadTo { cx, cy, x, y } => {
                    vec![2, cx.to_bits(), cy.to_bits(), x.to_bits(), y.to_bits()]
                }
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => vec![
                    3,
                    c1x.to_bits(),
                    c1y.to_bits(),
                    c2x.to_bits(),
                    c2y.to_bits(),
                    x.to_bits(),
                    y.to_bits(),
                ],
                Segment::Close => vec![4],
            }
        };
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.segments.len() == b.segments.len()
                    && a.segments
                        .iter()
                        .zip(&b.segments)
                        .all(|(p, q)| bits(p) == bits(q))
            }
            _ => false,
        }
    }

    fn same_image(a: &DecodedImage, b: &DecodedImage) -> bool {
        a.width == b.width
            && a.height == b.height
            && a.rgb == b.rgb
            && a.alpha == b.alpha
            && a.two_colour == b.two_colour
            && a.stencil == b.stencil
            && a.interpolate == b.interpolate
            && a.reduced == b.reduced
            && a.family == b.family
            && a.soft_masked == b.soft_masked
            && a.window == b.window
    }

    fn same_answer(
        a: &Result<Option<DecodedImage>, String>,
        b: &Result<Option<DecodedImage>, String>,
    ) -> bool {
        match (a, b) {
            (Ok(None), Ok(None)) => true,
            (Ok(Some(a)), Ok(Some(b))) => same_image(a, b),
            (Err(a), Err(b)) => a == b,
            _ => false,
        }
    }

    fn same_shading(a: &Shading, b: &Shading) -> bool {
        match (a, b) {
            (
                Shading::FunctionBased {
                    space: s1,
                    function: f1,
                    domain: d1,
                },
                Shading::FunctionBased {
                    space: s2,
                    function: f2,
                    domain: d2,
                },
            ) => s1 == s2 && f1 == f2 && d1 == d2,
            (
                Shading::Axial {
                    space: s1,
                    function: f1,
                    coords: c1,
                    extend: e1,
                },
                Shading::Axial {
                    space: s2,
                    function: f2,
                    coords: c2,
                    extend: e2,
                },
            ) => s1 == s2 && f1 == f2 && c1 == c2 && e1 == e2,
            (
                Shading::Radial {
                    space: s1,
                    function: f1,
                    coords: c1,
                    extend: e1,
                },
                Shading::Radial {
                    space: s2,
                    function: f2,
                    coords: c2,
                    extend: e2,
                },
            ) => s1 == s2 && f1 == f2 && c1 == c2 && e1 == e2,
            (Shading::Mesh(m1), Shading::Mesh(m2)) => {
                m1.kind == m2.kind
                    && m1.space == m2.space
                    && m1.function == m2.function
                    && m1.components == m2.components
                    && m1.positions == m2.positions
                    && m1.values == m2.values
                    && m1.triangles == m2.triangles
                    && m1.patches.len() == m2.patches.len()
                    && m1
                        .patches
                        .iter()
                        .zip(&m2.patches)
                        .all(|(p, q)| p.points == q.points && p.corners == q.corners)
            }
            _ => false,
        }
    }

    fn same_pattern(a: &Option<PatternPaint>, b: &Option<PatternPaint>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(PatternPaint::Unsupported), Some(PatternPaint::Unsupported)) => true,
            (Some(PatternPaint::Shading(s1, m1)), Some(PatternPaint::Shading(s2, m2))) => {
                same_shading(s1, s2) && m1 == m2
            }
            (Some(PatternPaint::Tiling(t1)), Some(PatternPaint::Tiling(t2))) => {
                t1.matrix == t2.matrix
                    && t1.bbox == t2.bbox
                    && t1.xstep == t2.xstep
                    && t1.ystep == t2.ystep
                    && t1.uncolored == t2.uncolored
            }
            _ => false,
        }
    }

    /// The stream's answer to one logged question, held to the page's:
    /// `None` where it answers it alike, and what differs otherwise.
    fn check(parsed: &Parsed, asked: &Asked) -> Option<String> {
        let path = match asked {
            Asked::Outline(path, ..)
            | Asked::Identity(path, ..)
            | Asked::Image(path, ..)
            | Asked::Inline(path, ..)
            | Asked::Shading(path, ..)
            | Asked::Pattern(path, ..)
            | Asked::Tile(path, ..)
            | Asked::FormScope(path, ..) => path,
        };
        let Some((stream, scope)) = locate(parsed, path) else {
            return Some(format!("no scope at {path:?} for {asked:?}"));
        };
        let tables = &stream.tables;
        let decode = |image: usize,
                      request: &ImageRequest,
                      answer: &Result<Option<DecodedImage>, String>| {
            let entry = &tables.images[image];
            let room = entry.room.map(crate::resources::Room);
            let reduction = entry
                .frame
                .map_or(1, |frame| frame.reduction(request, room));
            match tables
                .decodes
                .iter()
                .find(|d| d.image == image as u64 && d.reduction == reduction)
            {
                Some(held) if same_answer(&held.answer.value.handed(), answer) => None,
                Some(_) => Some(format!("another decode at {reduction} for {asked:?}")),
                None => Some(format!("no decode at {reduction} for {asked:?}")),
            }
        };
        match asked {
            Asked::Outline(_, font, code, answer) => match tables
                .outlines
                .iter()
                .find(|o| o.scope == scope && o.font_id == *font && o.code == *code)
            {
                Some(held) if same_outline(&held.answer.value, answer) => None,
                _ => Some(format!("outline {font} {code} at {path:?}")),
            },
            Asked::Identity(_, font, answer) => match tables
                .identities
                .iter()
                .find(|i| i.scope == scope && i.font_id == *font)
            {
                Some(held) if held.identity == *answer => None,
                _ => Some(format!("identity {font} at {path:?}")),
            },
            Asked::Image(_, name, request, intent, answer) => {
                let image = tables.images.iter().position(|i| {
                    i.scope == scope
                        && i.key == ImageKey::Named(name.clone())
                        && i.intent == *intent
                });
                match image {
                    Some(image) => decode(image, request, answer),
                    None => Some(format!("no image for {asked:?}")),
                }
            }
            Asked::Inline(_, dict, data, request, intent, answer) => {
                let image = tables.images.iter().position(|i| {
                    i.scope == scope
                        && i.intent == *intent
                        && matches!(i.key, ImageKey::Inline(o)
                            if stream.inline.get(o as usize) == Some(&(dict.clone(), data.clone())))
                });
                match image {
                    Some(image) => decode(image, request, answer),
                    None => Some(format!("no inline image for {asked:?}")),
                }
            }
            Asked::Shading(_, name, answer) => {
                let Some(held) = tables
                    .shadings
                    .iter()
                    .find(|s| s.scope == scope && s.name == *name)
                else {
                    return Some(format!("no shading for {asked:?}"));
                };
                let alike = match (&held.answer.value.handed(), answer) {
                    (Ok(None), Ok(None)) => true,
                    (Err(a), Err(b)) => a == b,
                    (Ok(Some(a)), Ok(Some(b))) => same_shading(a, b),
                    _ => false,
                };
                (!alike).then(|| format!("another shading for {asked:?}"))
            }
            Asked::Pattern(_, name, answer) => {
                let held = tables
                    .patterns
                    .iter()
                    .find(|p| p.scope == scope && p.name == *name);
                match held {
                    Some(held) if same_pattern(&held.answer.value.handed(), answer) => None,
                    _ => Some(format!("pattern for {asked:?}")),
                }
            }
            Asked::Tile(_, name, answered) => {
                let held = tables
                    .cells
                    .iter()
                    .any(|c| c.scope == scope && c.name == *name);
                (held != *answered).then(|| format!("a cell held {held} for {asked:?}"))
            }
            Asked::FormScope(_, name, answered) => {
                let held = tables
                    .scopes
                    .iter()
                    .any(|s| s.parent == Some(scope) && s.name.as_deref() == Some(name.as_slice()));
                (held != *answered).then(|| format!("a scope held {held} for {asked:?}"))
            }
        }
    }

    /// **The stream answers every question a render asks**: a page holding
    /// text in an embedded face and a Type 3 font, three image kinds and an
    /// inline image, an axial and a mesh shading, a tiling cell holding an
    /// image and a form with its own resources, rendered from its list
    /// through a logging `GlyphSource` at 0.5 and 2.25 times, asks only
    /// questions the stream's tables answer, each with the value the page's
    /// resources gave — outlines to the bit, images byte for byte, shadings
    /// and patterns field for field — and the tables read back from the
    /// bytes are the records written, cells' streams included.
    #[test]
    fn the_stream_answers_every_question_a_render_asks() {
        let doc = Document::open(every_question()).expect("the page opens");
        let page = doc.page(0).expect("a page");
        let list = page.display_list();
        assert!(list.is_retained(), "the page is small enough to keep");
        let (bytes, written) = write_list(&list);
        let mut meter = Meter::new(Budgets::DEFAULT);
        let parsed = parse(&bytes, &mut meter, 0, false).expect("the stream reads");

        // The tables read back are the ones written, record for record, and
        // so is each cell's stream.
        assert_eq!(
            write_stream(&parsed.records),
            bytes,
            "read and written back, the same bytes"
        );
        assert_eq!(parsed.tables.scopes, written.scopes);
        assert_eq!(parsed.tables.images, written.images);
        assert_eq!(parsed.tables.cells, written.cells);
        assert_eq!(parsed.tables.outlines.len(), written.outlines.len());
        for (read, wrote) in parsed.tables.outlines.iter().zip(&written.outlines) {
            assert!(
                read.scope == wrote.scope
                    && read.font_id == wrote.font_id
                    && read.code == wrote.code
                    && same_outline(&read.answer.value, &wrote.answer.value)
                    && read.answer.notes == wrote.answer.notes,
                "outline {} {}",
                wrote.font_id,
                wrote.code
            );
        }
        assert_eq!(parsed.tables.decodes.len(), written.decodes.len());
        for (read, wrote) in parsed.tables.decodes.iter().zip(&written.decodes) {
            assert!(
                read.image == wrote.image
                    && read.reduction == wrote.reduction
                    && same_answer(&read.answer.value.handed(), &wrote.answer.value.handed())
                    && read.answer.notes == wrote.answer.notes,
                "decode of image {} at {}",
                wrote.image,
                wrote.reduction
            );
        }
        for (read, wrote) in parsed.tables.shadings.iter().zip(&written.shadings) {
            let alike = match (read.answer.value.handed(), wrote.answer.value.handed()) {
                (Ok(Some(a)), Ok(Some(b))) => same_shading(&a, &b),
                (a, b) => format!("{a:?}") == format!("{b:?}"),
            };
            assert!(alike && read.name == wrote.name, "shading {:?}", wrote.name);
        }
        for (read, wrote) in parsed.tables.patterns.iter().zip(&written.patterns) {
            assert!(
                same_pattern(&read.answer.value.handed(), &wrote.answer.value.handed())
                    && read.name == wrote.name,
                "pattern {:?}",
                wrote.name
            );
        }
        for (cell, read) in written.cells.iter().zip(&parsed.cells) {
            assert_eq!(
                write_stream(&read.records),
                cell.stream.to_vec(),
                "cell {:?}",
                cell.name
            );
        }

        // What the stream must hold, by kind: the page asks each.
        assert!(
            !parsed.tables.outlines.is_empty(),
            "the embedded face's glyphs"
        );
        assert!(
            parsed
                .tables
                .images
                .iter()
                .any(|i| matches!(i.frame, Some(crate::resources::Frame::Jpeg { .. }))),
            "the JPEG and its frame"
        );
        assert!(
            parsed
                .tables
                .decodes
                .iter()
                .filter(|d| d.image as usize
                    == parsed
                        .tables
                        .images
                        .iter()
                        .position(|i| i.frame.is_some())
                        .unwrap_or(usize::MAX))
                .count()
                == 4,
            "a JPEG at each of its four scales"
        );
        assert_eq!(parsed.tables.cells.len(), 1, "the tiling cell");
        assert!(parsed
            .tables
            .scopes
            .iter()
            .any(|s| s.name.as_deref() == Some(&b"Fm0"[..])));
        assert!(parsed.inline.len() == 1, "the inline image");

        for scale in [0.5, 2.25] {
            let log = Arc::new(Mutex::new(Vec::new()));
            let root = Logging {
                inner: Arc::new(list.resources().for_one_render()),
                path: Vec::new(),
                log: Arc::clone(&log),
            };
            let options = RenderOptions {
                scale,
                ..RenderOptions::default()
            };
            let (width, height) = page.pixel_size(&options);
            let base = page.base_at(scale);
            let canvas = Canvas::new(width, height, PixelFormat::Rgb8, Color::WHITE);
            let mut renderer = Renderer::new(canvas, base, &root);
            replay(list.content(), &mut renderer);
            for (k, annotation) in list.annotation_layers().iter().enumerate() {
                // The page holds no appearance a render places.
                if let crate::annots::Kept::Calls { scope, events } = &annotation.kept {
                    renderer.push_resources(Arc::new(
                        root.below(Step::Annotation(k), Arc::clone(scope)),
                    ));
                    replay(events, &mut renderer);
                    renderer.pop_resources();
                }
            }
            let _ = renderer.finish_counted();

            let asked = log
                .lock()
                .map(|mut log| std::mem::take(&mut *log))
                .unwrap_or_default();
            let kinds = |pick: fn(&Asked) -> bool| asked.iter().filter(|a| pick(a)).count();
            assert!(
                kinds(|a| matches!(a, Asked::Outline(..))) > 0,
                "{scale}: outlines asked"
            );
            assert!(
                kinds(|a| matches!(a, Asked::Image(..))) >= 3,
                "{scale}: three images asked"
            );
            assert!(
                kinds(|a| matches!(a, Asked::Inline(..))) > 0,
                "{scale}: the inline image"
            );
            assert!(
                kinds(|a| matches!(a, Asked::Shading(..))) >= 2,
                "{scale}: two shadings"
            );
            assert!(
                kinds(|a| matches!(a, Asked::Tile(_, _, true))) > 0,
                "{scale}: the cell"
            );
            assert!(
                kinds(
                    |a| matches!(a, Asked::Image(path, ..) if path.iter().any(|s| matches!(s, Step::Cell(_))))
                ) > 0,
                "{scale}: the cell's image, asked inside it"
            );
            assert!(
                kinds(
                    |a| matches!(a, Asked::Image(path, ..) if path.iter().any(|s| matches!(s, Step::Form(_))))
                ) > 0,
                "{scale}: the form's own image, asked in its scope"
            );
            let wrong: Vec<String> = asked.iter().filter_map(|a| check(&parsed, a)).collect();
            assert!(
                wrong.is_empty(),
                "{scale}: the stream answers otherwise:\n{}",
                wrong.join("\n")
            );
        }
    }

    /// A page that is not retained is written by interpreting it into the
    /// writer, and the stream is the one its retained list writes.
    #[test]
    fn a_page_interpreted_into_the_writer_writes_its_lists_stream() {
        let doc = Document::open(every_question()).expect("the page opens");
        let page = doc.page(0).expect("a page");
        let list = page.display_list();
        let interpreted = crate::display::not_retained(&page);
        assert!(!interpreted.is_retained());
        assert_eq!(
            write_list(&interpreted).0,
            write_list(&list).0,
            "one stream whichever way it was written"
        );
        assert_eq!(page.command_stream(), list.command_stream());
    }
}
