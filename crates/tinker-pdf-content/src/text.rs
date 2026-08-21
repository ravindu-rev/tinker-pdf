//! The text-extraction device.
//!
//! Glyphs arrive from the interpreter with a transform each; this assembles
//! them into characters, lines and blocks, and reports the quads selection and
//! search need. It implements [`Device`] and touches no rasterizer, which is
//! the whole reason text parity can be reached before pixels exist.
//!
//! One naming decision worth stating: **writing mode and directionality are
//! separate fields**. Tinker's current DTO carries a single `rtl` flag that is
//! actually `wmode != Horizontal` — vertical Japanese and right-to-left Arabic
//! are not the same property, and conflating them makes both wrong. The
//! engine reports them apart and lets the caller map as it needs.

use crate::device::{Device, Glyph};
use crate::state::GraphicsState;

/// Four corners, in device space (9.4.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// Upper-left.
    pub ul: (f64, f64),
    /// Upper-right.
    pub ur: (f64, f64),
    /// Lower-left.
    pub ll: (f64, f64),
    /// Lower-right.
    pub lr: (f64, f64),
}

impl Quad {
    /// The enclosing axis-aligned rectangle, as `(x0, y0, x1, y1)`.
    ///
    /// The helper the engine this replaces never had: its `Quad` exposed four
    /// points and left every caller to write this, correctly or otherwise.
    #[must_use]
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let xs = [self.ul.0, self.ur.0, self.ll.0, self.lr.0];
        let ys = [self.ul.1, self.ur.1, self.ll.1, self.lr.1];
        (
            xs.iter().copied().fold(f64::INFINITY, f64::min),
            ys.iter().copied().fold(f64::INFINITY, f64::min),
            xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    /// Whether every corner is finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        [self.ul, self.ur, self.ll, self.lr]
            .iter()
            .all(|(x, y)| x.is_finite() && y.is_finite())
    }
}

/// One extracted character.
#[derive(Clone, Debug)]
pub struct TextChar {
    /// The text it stands for; more than one character for a ligature.
    pub text: String,
    /// Where it sits.
    pub quad: Quad,
    /// The font size in device units.
    pub size: f64,
    /// The origin of the glyph, which is where the next one starts from.
    pub origin: (f64, f64),
}

/// Which way a line runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WritingMode {
    /// Horizontal, the default (9.7.4.3 wmode 0).
    #[default]
    Horizontal,
    /// Vertical (wmode 1).
    Vertical,
}

/// One line of text.
#[derive(Clone, Debug)]
pub struct TextLine {
    /// The characters, in the order the content stream showed them.
    pub chars: Vec<TextChar>,
    /// The whole line's text.
    pub text: String,
    /// The line's bounding quad.
    pub quad: Quad,
    /// The font's writing mode.
    pub wmode: WritingMode,
    /// Whether the line's characters are predominantly right-to-left.
    ///
    /// Determined from the characters themselves, not from the writing mode:
    /// they are different properties and a vertical Japanese line is not
    /// right-to-left.
    pub rtl: bool,
    /// The largest font size on the line, which is what the eye reads it as.
    pub size: f64,
}

/// One block of lines.
#[derive(Clone, Debug)]
pub struct TextBlock {
    /// The lines, in reading order.
    pub lines: Vec<TextLine>,
    /// The block's bounding quad.
    pub quad: Quad,
}

/// A page's text.
#[derive(Clone, Debug, Default)]
pub struct TextPage {
    /// The blocks, in reading order.
    pub blocks: Vec<TextBlock>,
    /// What extraction had to tolerate.
    ///
    /// Ruling 2 applies to text as much as to pixels: a page whose font could
    /// not be read still extracts what it can, and a caller comparing two
    /// extractions needs to know which one was guessing. Without this the
    /// difference between "this page has no text" and "this page has text this
    /// build could not decode" is invisible.
    pub warnings: Vec<TextWarning>,
}

/// Something extraction could not do exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextWarning {
    /// A font resource the page named could not be resolved, so its codes were
    /// decoded by their bytes rather than by its encoding.
    UnknownFont {
        /// The resource name.
        name: String,
    },
    /// A code had no `/ToUnicode` entry and no encoding that named it, so the
    /// character it stands for is a guess.
    UnmappedCode {
        /// The code.
        code: u32,
    },
}

impl TextPage {
    /// Every line, flattened.
    #[must_use]
    pub fn lines(&self) -> Vec<&TextLine> {
        self.blocks.iter().flat_map(|b| b.lines.iter()).collect()
    }

    /// The page's text, one line per line.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            for line in &block.lines {
                out.push_str(&line.text);
                out.push('\n');
            }
        }
        out
    }

    /// Finds `needle`, case-insensitively and literally.
    ///
    /// Matches within a line, returning one quad per match. Case folding is
    /// the simple ASCII-plus-Unicode-lowercase kind, which is what the engine
    /// being replaced did and what callers have calibrated against.
    #[must_use]
    pub fn search(&self, needle: &str) -> Vec<Quad> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle_lower = needle.to_lowercase();
        let mut hits = Vec::new();

        for line in self.lines() {
            // Index by character so a match maps back to the glyphs it covers.
            let chars: Vec<String> = line.chars.iter().map(|c| c.text.to_lowercase()).collect();
            let joined: String = chars.concat();

            let mut from = 0usize;
            while let Some(found) = joined.get(from..).and_then(|s| s.find(&needle_lower)) {
                let start_byte = from + found;
                let end_byte = start_byte + needle_lower.len();

                // Map byte offsets back to glyph indices.
                let mut at = 0usize;
                let mut first = None;
                let mut last = None;
                for (i, piece) in chars.iter().enumerate() {
                    let next = at + piece.len();
                    if first.is_none() && next > start_byte {
                        first = Some(i);
                    }
                    if at < end_byte {
                        last = Some(i);
                    }
                    at = next;
                }

                if let (Some(a), Some(b)) = (first, last) {
                    if let Some(quad) = span_quad(line, a, b) {
                        hits.push(quad);
                    }
                }

                from = start_byte + needle_lower.len().max(1);
                if from >= joined.len() {
                    break;
                }
            }
        }

        hits
    }
}

/// The quad covering glyphs `a..=b` of a line.
fn span_quad(line: &TextLine, a: usize, b: usize) -> Option<Quad> {
    let first = line.chars.get(a)?;
    let last = line.chars.get(b)?;
    Some(Quad {
        ul: first.quad.ul,
        ll: first.quad.ll,
        ur: last.quad.ur,
        lr: last.quad.lr,
    })
}

/// Collects glyphs into a [`TextPage`].
#[derive(Default)]
pub struct TextDevice {
    lines: Vec<PendingLine>,
    current: Option<PendingLine>,
    warnings: Vec<TextWarning>,
    /// How many `/Artifact` scopes are open (14.8.2.2).
    ///
    /// A **counter and not a flag**, because marked content nests and an
    /// `/Artifact` inside an `/Artifact` closes one of them: a flag cleared by
    /// the inner `EMC` would put the rest of the outer artifact back into the
    /// text. The interpreter guarantees one `end_marked_content` per accepted
    /// begin, so this cannot go negative and cannot leak past a form XObject.
    artifacts: usize,
    /// One `depth` entry per open scope: whether it was an artifact.
    ///
    /// `EMC` names no tag, so closing correctly needs the stack rather than
    /// the count — a `/Span` opened inside an `/Artifact` and closed first
    /// would otherwise decrement the artifact count and let the rest of the
    /// artifact through.
    scopes: Vec<bool>,
}

struct PendingLine {
    chars: Vec<TextChar>,
    wmode: WritingMode,
    /// The baseline direction, used to decide whether a glyph continues the
    /// line or starts a new one.
    direction: (f64, f64),
    origin: (f64, f64),
    /// Where the last glyph's box ended along the baseline, which is where the
    /// pen would be if the run continued.
    pen: f64,
    font_id: u64,
    /// Whether an `ET` closed the text object this line was collected in.
    ///
    /// Not a line break by itself — see [`TextDevice::end_text`] — but the next
    /// glyph has to **continue** the line rather than merely share its
    /// baseline, because within a text object the pen carries the position and
    /// across one it does not.
    closed: bool,
}

impl TextDevice {
    /// A device with nothing collected yet.
    #[must_use]
    pub fn new() -> TextDevice {
        TextDevice::default()
    }

    /// Records something extraction had to tolerate.
    ///
    /// Deduplicated: a page whose font is unknown says so once rather than
    /// once per glyph, which would bury the report under its own noise.
    pub fn warn(&mut self, warning: TextWarning) {
        if !self.warnings.contains(&warning) && self.warnings.len() < 64 {
            self.warnings.push(warning);
        }
    }

    /// The page assembled from everything shown so far.
    #[must_use]
    pub fn finish(mut self) -> TextPage {
        self.flush();

        let lines: Vec<TextLine> = self
            .lines
            .into_iter()
            .filter_map(|pending| {
                if pending.chars.is_empty() {
                    return None;
                }
                let text: String = pending.chars.iter().map(|c| c.text.as_str()).collect();
                if text.trim().is_empty() && text.chars().all(char::is_whitespace) {
                    return None;
                }
                let quad = enclosing(pending.chars.iter().map(|c| c.quad))?;
                let size = pending.chars.iter().map(|c| c.size).fold(0.0f64, f64::max);
                let rtl = is_rtl(&text);
                Some(TextLine {
                    chars: pending.chars,
                    text,
                    quad,
                    wmode: pending.wmode,
                    rtl,
                    size,
                })
            })
            .collect();

        // Blocks: consecutive lines whose vertical gap is no more than about
        // one line height belong together. A cheap heuristic, and the one that
        // matches how paragraphs are actually set.
        let mut blocks: Vec<TextBlock> = Vec::new();
        for line in lines {
            let joins = blocks
                .last()
                .and_then(|b| b.lines.last())
                .is_some_and(|prev| {
                    let (_, py0, _, py1) = prev.quad.bounds();
                    let (_, ly0, _, ly1) = line.quad.bounds();
                    let height = (py1 - py0).max(line.size).max(1.0);
                    let gap = (py0 - ly1).abs().min((ly0 - py1).abs());
                    gap <= height * 1.5
                });

            match (joins, blocks.last_mut()) {
                (true, Some(block)) => block.lines.push(line),
                _ => blocks.push(TextBlock {
                    quad: line.quad,
                    lines: vec![line],
                }),
            }
        }

        for block in &mut blocks {
            if let Some(quad) = enclosing(block.lines.iter().map(|l| l.quad)) {
                block.quad = quad;
            }
        }

        TextPage {
            blocks,
            warnings: std::mem::take(&mut self.warnings),
        }
    }

    fn flush(&mut self) {
        if let Some(line) = self.current.take() {
            self.lines.push(line);
        }
    }
}

/// Whether a run of text is predominantly right-to-left.
///
/// Counts strong directional characters: Hebrew, Arabic, Syriac and Thaana
/// against everything with a left-to-right property. Deliberately not a full
/// bidi implementation — reordering is a caller's concern, and reporting the
/// dominant direction is what a text layer needs.
fn is_rtl(text: &str) -> bool {
    let mut rtl = 0usize;
    let mut ltr = 0usize;
    for c in text.chars() {
        let code = u32::from(c);
        let strong_rtl = (0x0590..=0x05FF).contains(&code)   // Hebrew
            || (0x0600..=0x06FF).contains(&code)             // Arabic
            || (0x0700..=0x074F).contains(&code)             // Syriac
            || (0x0780..=0x07BF).contains(&code)             // Thaana
            || (0x08A0..=0x08FF).contains(&code)             // Arabic Extended-A
            || (0xFB1D..=0xFDFF).contains(&code)             // Hebrew/Arabic presentation
            || (0xFE70..=0xFEFF).contains(&code);
        if strong_rtl {
            rtl += 1;
        } else if c.is_alphabetic() {
            ltr += 1;
        }
    }
    rtl > ltr
}

fn enclosing(quads: impl Iterator<Item = Quad>) -> Option<Quad> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for quad in quads {
        if !quad.is_finite() {
            continue;
        }
        let (x0, y0, x1, y1) = quad.bounds();
        bounds = Some(match bounds {
            None => (x0, y0, x1, y1),
            Some((ax0, ay0, ax1, ay1)) => (ax0.min(x0), ay0.min(y0), ax1.max(x1), ay1.max(y1)),
        });
    }
    let (x0, y0, x1, y1) = bounds?;
    Some(Quad {
        ul: (x0, y1),
        ur: (x1, y1),
        ll: (x0, y0),
        lr: (x1, y0),
    })
}

impl Device for TextDevice {
    /// 14.8.2.2: an artifact is *"a graphics object that is not part of the
    /// author's original content"* — a running head, a rule, a generated list
    /// marker — and 14.8.2's whole point is that it is **excluded** from the
    /// logical content a consumer reads.
    ///
    /// So text inside one is not extracted, and the asymmetry with the
    /// renderer is deliberate and is the reason the tag is passed at all: an
    /// artifact is drawn and not read, and an invisible optional-content layer
    /// is read and not drawn.
    fn begin_marked_content(&mut self, tag: &[u8], _visible: bool, _hidden_layer: Option<&str>) {
        let artifact = tag == b"Artifact";
        self.scopes.push(artifact);
        if artifact {
            // A line may not straddle the boundary: the artifact's glyphs are
            // dropped, and the ones before it must not be joined to the ones
            // after as though nothing had been between them.
            self.flush();
            self.artifacts += 1;
        }
    }

    fn end_marked_content(&mut self) {
        if let Some(true) = self.scopes.pop() {
            self.flush();
            self.artifacts = self.artifacts.saturating_sub(1);
        }
    }

    fn show_glyph(&mut self, glyph: &Glyph, _state: &GraphicsState) {
        if self.artifacts > 0 {
            return;
        }
        if !glyph.transform.is_finite() || glyph.text.is_empty() {
            return;
        }

        // The glyph box in text space is the unit square scaled by the font's
        // ascent and descent; 0.0 to 1.0 vertically is close enough for a
        // selection rectangle and needs no font program to compute.
        let t = &glyph.transform;
        let advance = if glyph.advance.is_finite() {
            glyph.advance / glyph.size.max(f64::MIN_POSITIVE)
        } else {
            0.0
        };
        let (w, h) = if glyph.vertical {
            (1.0, advance.abs().max(0.001))
        } else {
            (advance.max(0.001), 1.0)
        };

        let corner = |x: f64, y: f64| t.apply(x, y);
        let quad = Quad {
            ll: corner(0.0, -0.2 * h),
            lr: corner(w, -0.2 * h),
            ul: corner(0.0, 0.8 * h),
            ur: corner(w, 0.8 * h),
        };
        if !quad.is_finite() {
            return;
        }

        let origin = (t.e, t.f);
        // The transform already carries the font size (9.4.4 builds it from
        // `Tf`), so its expansion *is* the device-space size. Multiplying by
        // `glyph.size` again would square it.
        let size = t.expansion();
        let wmode = if glyph.vertical {
            WritingMode::Vertical
        } else {
            WritingMode::Horizontal
        };
        let direction = (t.a, t.b);

        let starts_new_line = match &self.current {
            None => true,
            Some(line) => {
                let same_font = line.font_id == glyph.font_id;
                // A glyph continues the line when it runs the same way and sits
                // near where the previous one ended.
                let turned =
                    (line.direction.0 * direction.1 - line.direction.1 * direction.0).abs() > 0.01;
                let expected_gap = size.max(1.0) * 3.0;
                let far = match wmode {
                    WritingMode::Horizontal => {
                        (origin.1 - line.origin.1).abs() > size.max(1.0) * 0.5
                    }
                    WritingMode::Vertical => (origin.0 - line.origin.0).abs() > size.max(1.0) * 0.5,
                };
                let backwards = match wmode {
                    WritingMode::Horizontal => origin.0 + expected_gap < line.origin.0,
                    WritingMode::Vertical => origin.1 - expected_gap > line.origin.1,
                };
                // A line that an `ET` closed is resumed only where the last one
                // stopped. Half an em of slack, which is a space and is not a
                // column: two cells of a table row sit on one baseline and are
                // two lines, and a paragraph that changed font mid-word is one.
                let slack = size.max(1.0) * 0.5;
                let disjoint = line.closed
                    && match wmode {
                        WritingMode::Horizontal => origin.0 > line.pen + slack,
                        WritingMode::Vertical => origin.1 < line.pen - slack,
                    };
                turned || far || backwards || disjoint || (line.wmode != wmode && !same_font)
            }
        };

        if starts_new_line {
            self.flush();
            self.current = Some(PendingLine {
                chars: Vec::new(),
                wmode,
                direction,
                origin,
                pen: match wmode {
                    WritingMode::Horizontal => origin.0,
                    WritingMode::Vertical => origin.1,
                },
                font_id: glyph.font_id,
                closed: false,
            });
        }

        if let Some(line) = &mut self.current {
            line.origin = origin;
            line.font_id = glyph.font_id;
            line.closed = false;
            let (x0, y0, x1, y1) = quad.bounds();
            line.pen = match wmode {
                WritingMode::Horizontal => x1.max(x0),
                WritingMode::Vertical => y0.min(y1),
            };
            line.chars.push(TextChar {
                text: glyph.text.clone(),
                quad,
                size,
                origin,
            });
        }
    }

    fn end_text(&mut self) {
        // **A text object's end is not a line break.** It was, until gap 31's
        // milestone 9 made this build produce the counter-example: a PDF string
        // is bytes in *one* font, so `css-fonts-4` §5.3's per-character
        // matching turns one paragraph into one `BT … ET` object per stretch of
        // characters sharing a face — and a rule that broke a line at every
        // `ET` broke that paragraph wherever its font changed, which is
        // wherever the fallback did its job. It is not an EPUB-only shape: a
        // producer that emits a text object per styled span writes the same
        // page, and every bold word in the middle of a sentence is one.
        //
        // What an `ET` does mean is that the pen is put down. So the line is
        // held open and the next glyph has to **continue** it — same baseline,
        // same direction, and within half an em of where the last one ended —
        // rather than merely land somewhere on the same line, which is what two
        // cells of a table row do.
        if let Some(line) = &mut self.current {
            line.closed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Matrix;

    fn glyph(text: &str, x: f64, y: f64, size: f64) -> Glyph {
        Glyph {
            code: text.chars().next().map_or(0, u32::from),
            text: text.to_string(),
            transform: Matrix {
                a: size,
                b: 0.0,
                c: 0.0,
                d: size,
                e: x,
                f: y,
            },
            advance: size * 0.5,
            size,
            vertical: false,
            font_id: 1,
        }
    }

    fn page(glyphs: &[Glyph]) -> TextPage {
        let mut d = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        for g in glyphs {
            d.show_glyph(g, &state);
        }
        d.finish()
    }

    /// 14.8.2.2's artifact: drawn, and **not** part of the logical content.
    ///
    /// Three claims, because three different mistakes give three different
    /// answers: the artifact's own glyphs are dropped, the glyphs either side
    /// of it are kept, and they are not joined into one line across the hole.
    #[test]
    fn an_artifact_is_drawn_and_is_not_extracted() {
        let mut d = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        d.show_glyph(&glyph("a", 0.0, 700.0, 10.0), &state);
        d.begin_marked_content(b"Artifact", true, None);
        d.show_glyph(&glyph("X", 20.0, 700.0, 10.0), &state);
        d.end_marked_content();
        d.show_glyph(&glyph("b", 40.0, 700.0, 10.0), &state);
        let text = d.finish().plain_text();
        assert!(!text.contains('X'), "an artifact was extracted: {text:?}");
        assert!(text.contains('a') && text.contains('b'), "{text:?}");
    }

    /// A scope opened **inside** an artifact does not end it.
    ///
    /// A counter — or worse, a flag cleared by any `EMC` — lets the rest of an
    /// artifact back into the text at the first nested scope's end, and marked
    /// content nests by construction: 14.6.2's own examples put a `/Span`
    /// inside a structure element. No content stream this repository writes
    /// nests one today, which is why a defect that reduced the stack to a flag
    /// survived the whole suite until this test existed.
    #[test]
    fn a_scope_inside_an_artifact_does_not_end_it() {
        let mut d = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        d.begin_marked_content(b"Artifact", true, None);
        d.show_glyph(&glyph("X", 0.0, 700.0, 10.0), &state);
        d.begin_marked_content(b"Span", true, None);
        d.show_glyph(&glyph("Y", 10.0, 700.0, 10.0), &state);
        d.end_marked_content();
        d.show_glyph(&glyph("Z", 20.0, 700.0, 10.0), &state);
        d.end_marked_content();
        d.show_glyph(&glyph("a", 30.0, 700.0, 10.0), &state);
        let text = d.finish().plain_text();
        for gone in ['X', 'Y', 'Z'] {
            assert!(
                !text.contains(gone),
                "{gone} escaped the artifact: {text:?}"
            );
        }
        assert!(text.contains('a'), "the artifact never ended: {text:?}");
    }

    /// And an ordinary scope changes nothing, which is what says the two tests
    /// above are about `/Artifact` rather than about marked content.
    #[test]
    fn an_ordinary_marked_content_scope_extracts_normally() {
        let mut d = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        d.begin_marked_content(b"Span", true, None);
        d.show_glyph(&glyph("k", 0.0, 700.0, 10.0), &state);
        d.end_marked_content();
        assert!(d.finish().plain_text().contains('k'));
    }

    #[test]
    fn glyphs_on_one_baseline_become_one_line() {
        let p = page(&[glyph("H", 0.0, 700.0, 10.0), glyph("i", 5.0, 700.0, 10.0)]);
        assert_eq!(p.lines().len(), 1);
        assert_eq!(p.plain_text(), "Hi\n");
    }

    #[test]
    fn a_new_baseline_starts_a_new_line() {
        let p = page(&[glyph("A", 0.0, 700.0, 10.0), glyph("B", 0.0, 680.0, 10.0)]);
        assert_eq!(p.lines().len(), 2);
        assert_eq!(p.plain_text(), "A\nB\n");
    }

    /// **A text object's end is not a line break, and a gap on one baseline
    /// still is.**
    ///
    /// Two independent consequences of [`TextDevice::end_text`], and a test for
    /// one of them is not a test: a build that flushed at every `ET` breaks a
    /// paragraph wherever its font changes, and a build that never broke at all
    /// joins two cells of a table row into one line.
    ///
    /// The first half is not hypothetical. Gap 31's milestone 9 makes this
    /// build write one `BT … ET` object per stretch of characters sharing a
    /// face, because a PDF string is bytes in one font — so `ABCDEFGHI` set in
    /// three faces is three objects on one baseline, and used to extract as
    /// three lines.
    #[test]
    fn a_text_object_boundary_is_not_a_line_break_and_a_gap_still_is() {
        let mut resumed = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        // Three objects, each ending where the last left off: 10-point glyphs
        // at half an em.
        for (at, text) in [(0.0, "AB"), (10.0, "CD"), (20.0, "EF")] {
            for (offset, ch) in text.chars().enumerate() {
                let x = at + offset as f64 * 5.0;
                resumed.show_glyph(&glyph(&ch.to_string(), x, 700.0, 10.0), &state);
            }
            resumed.end_text();
        }
        assert_eq!(
            resumed.finish().plain_text(),
            "ABCDEF\n",
            "a run split across three text objects extracted as three lines"
        );

        // And the same baseline with a real gap in it stays two lines: two
        // cells of a table row, or a heading and a page number.
        let mut apart = TextDevice::new();
        apart.show_glyph(&glyph("A", 0.0, 700.0, 10.0), &state);
        apart.end_text();
        apart.show_glyph(&glyph("B", 200.0, 700.0, 10.0), &state);
        apart.end_text();
        assert_eq!(
            apart.finish().plain_text(),
            "A\nB\n",
            "two text objects far apart on one baseline became one line"
        );
    }

    #[test]
    fn quads_enclose_their_glyphs_and_bound_correctly() {
        let p = page(&[glyph("A", 10.0, 700.0, 12.0)]);
        let line = p.lines().first().copied().expect("a line");
        let (x0, y0, x1, y1) = line.quad.bounds();
        assert!(x0 >= 9.9 && x1 > x0, "x range {x0}..{x1}");
        assert!(y0 < y1, "y range {y0}..{y1}");
        assert!(line.quad.is_finite());
    }

    #[test]
    fn search_is_literal_and_case_insensitive() {
        let p = page(&[
            glyph("T", 0.0, 700.0, 10.0),
            glyph("i", 5.0, 700.0, 10.0),
            glyph("n", 10.0, 700.0, 10.0),
            glyph("k", 15.0, 700.0, 10.0),
        ]);
        assert_eq!(p.search("Tink").len(), 1);
        assert_eq!(p.search("TINK").len(), 1, "case-insensitive by default");
        assert_eq!(p.search("tink").len(), 1);
        assert_eq!(p.search("ink").len(), 1);
        assert_eq!(p.search("zzz").len(), 0);
        assert_eq!(p.search("").len(), 0, "an empty needle finds nothing");
    }

    #[test]
    fn a_search_hit_covers_the_glyphs_it_matched() {
        let p = page(&[
            glyph("A", 0.0, 700.0, 10.0),
            glyph("B", 10.0, 700.0, 10.0),
            glyph("C", 20.0, 700.0, 10.0),
        ]);
        let hits = p.search("BC");
        let quad = hits.first().copied().expect("a hit");
        let (x0, _, x1, _) = quad.bounds();
        assert!(x0 >= 9.9, "the hit starts at B, not A: {x0}");
        assert!(x1 > 20.0, "and extends past C's origin: {x1}");
    }

    #[test]
    fn writing_mode_and_direction_are_separate_properties() {
        // Vertical Latin text is vertical but not right-to-left.
        let mut vertical = glyph("A", 0.0, 700.0, 10.0);
        vertical.vertical = true;
        let p = page(&[vertical]);
        let line = p.lines().first().copied().expect("a line");
        assert_eq!(line.wmode, WritingMode::Vertical);
        assert!(!line.rtl, "vertical is not the same as right-to-left");

        // Hebrew is right-to-left but horizontal.
        let p = page(&[glyph("\u{05D0}", 0.0, 700.0, 10.0)]);
        let line = p.lines().first().copied().expect("a line");
        assert_eq!(line.wmode, WritingMode::Horizontal);
        assert!(line.rtl);
    }

    #[test]
    fn an_empty_page_is_empty_not_an_error() {
        let p = page(&[]);
        assert!(p.blocks.is_empty());
        assert_eq!(p.plain_text(), "");
        assert!(p.search("anything").is_empty());
    }

    #[test]
    fn non_finite_glyphs_are_dropped_rather_than_poisoning_the_page() {
        let mut bad = glyph("X", 0.0, 0.0, 10.0);
        bad.transform.e = f64::NAN;
        let p = page(&[bad, glyph("A", 0.0, 700.0, 10.0)]);
        assert_eq!(p.plain_text(), "A\n");
    }
}
