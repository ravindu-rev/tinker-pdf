//! Redaction (phase 10).
//!
//! The whole point is that the content is **gone**, not covered. A black
//! rectangle drawn over text leaves the text in the content stream, where any
//! extractor finds it — that is how redaction failures reach the news.
//!
//! So this rewrites the content stream itself: every text-showing operator
//! whose glyphs fall inside a redaction rectangle has those glyphs removed and
//! is re-emitted with a displacement in their place, so the surviving text on
//! the line keeps its position.
//!
//! It lives in the facade rather than in `cos` because deciding *which* glyphs
//! a rectangle covers needs both the content tokenizer and the font metrics,
//! and the leaf crates do not depend on each other (ruling 8).
//!
//! The acceptance test is not "does it look right" but "decompress every
//! stream in the output and assert the needle bytes are absent".

use std::collections::HashMap;
use std::sync::Arc;

use tinker_pdf_content::{Token, Tokenizer};
use tinker_pdf_cos::{
    font as cos_font, pages as cos_pages, CosDocument, Dict, DocumentEditor, Font, Name, ObjRef,
    Object, Rect, StreamData,
};

/// What a redaction covers and how it is marked.
#[derive(Clone, Copy, Debug)]
pub struct Redaction {
    /// The area to clear, in unrotated page space.
    pub area: Rect,
    /// Whether to paint the area black after clearing it.
    ///
    /// Cosmetic: the content is already gone by the time this draws. Its value
    /// is telling a reader that something was removed, rather than leaving a
    /// gap that reads as though nothing was ever there.
    pub mark: bool,
}

/// What a redaction did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactionReport {
    /// How many text-showing operations had glyphs removed.
    pub operations: usize,
    /// How many glyphs were removed in total.
    pub glyphs: usize,
}

/// Applies redactions to one page of an open editor.
///
/// Returns what was removed, or `None` when the page does not exist. Applying
/// an empty list, or one that covers no text, is a no-op that still reports
/// success — a redaction that finds nothing is not an error.
pub fn apply(
    editor: &mut DocumentEditor,
    page: u32,
    areas: &[Redaction],
) -> Option<RedactionReport> {
    let reference = editor.page_refs().get(page as usize).copied()?;

    let (content, existing, fonts) = {
        let doc = editor.document();
        let collected = cos_pages::collect(doc);
        let info = collected.get(page as usize)?;
        let content = cos_pages::content_bytes(doc, info);
        let existing = cos_pages::contents(doc, info);
        let resources = page_resources(doc, reference).unwrap_or_default();

        // Re-keyed by the raw name bytes, because the rewrite matches against
        // what the `Tf` operator literally says rather than against an
        // interned name it has no document to intern with.
        let fonts = cos_font::from_resources(doc, &resources)
            .into_iter()
            .filter_map(|(name, font)| doc.name_bytes(name).map(|bytes| (bytes.to_vec(), font)))
            .collect::<HashMap<Vec<u8>, Arc<Font>>>();
        (content, existing, fonts)
    };

    let (mut data, report) = rewrite(&content, areas, &fonts);

    if areas.iter().any(|r| r.mark) {
        // Painted last, so it covers whatever remains beneath it.
        data.extend_from_slice(b"q 0 g\n");
        for area in areas.iter().filter(|r| r.mark).map(|r| r.area) {
            let line = format!(
                "{} {} {} {} re f\n",
                area.x0,
                area.y0,
                area.x1 - area.x0,
                area.y1 - area.y0
            );
            data.extend_from_slice(line.as_bytes());
        }
        data.extend_from_slice(b"Q\n");
    }

    // The redacted stream **overwrites** the object the old one lived in, and
    // any further parts of a split `/Contents` array are deleted outright.
    //
    // Writing to a freshly allocated object instead would leave the original
    // bytes sitting in the file, unreferenced but perfectly readable — which
    // is the exact failure this whole module exists to prevent, and which the
    // decompress-every-stream test caught when it was written that way.
    let content_ref = match existing.split_first() {
        Some((first, rest)) => {
            for &stale in rest {
                editor.delete(stale);
            }
            *first
        }
        None => editor.allocate(),
    };

    editor.put_stream(
        content_ref,
        StreamData {
            dict: Dict::new(),
            data,
        },
    );

    let Some(Object::Dict(mut dict)) = editor.get(reference) else {
        return None;
    };
    let contents = editor.intern(b"Contents");
    dict.insert(contents, Object::Ref(content_ref));
    editor.put(reference, Object::Dict(dict));

    Some(report)
}

fn page_resources(doc: &CosDocument, page: ObjRef) -> Option<Dict> {
    let object = doc.get(page).ok()?;
    let dict = object.as_dict()?;
    doc.resolve_key(dict, Name::RESOURCES).as_dict().cloned()
}

/// The text state needed to place a glyph: everything in 9.4.4's displacement
/// formula, and nothing else.
#[derive(Clone, Copy)]
struct Pen {
    /// Where the next glyph goes, along the baseline.
    x: f64,
    /// The baseline itself.
    y: f64,
    line_x: f64,
    line_y: f64,
    /// The scale and translation `cm` applies, as `(sx, sy, tx, ty)`.
    ///
    /// A redaction rectangle is given in *page* space, so content drawn under
    /// a `cm` has to be mapped into it. Ignoring the transform measures every
    /// glyph in the wrong space: `q 2 0 0 2 0 0 cm` halves every computed
    /// position, so the rectangle covers the wrong half of the line.
    ctm: (f64, f64, f64, f64),
    /// The scale `Tm` applies, as `(x, y)`.
    ///
    /// A text matrix is not only a translation: `12 0 0 12 x y Tm` with a
    /// `/F0 1 Tf` is a perfectly ordinary way to write twelve-point text, and
    /// reading only the translation leaves every advance twelve times too
    /// small — so the glyphs a rectangle covers are computed from positions
    /// that drift further wrong across the line, and the wrong text is cut.
    scale: (f64, f64),
    /// Whether the text or transformation matrix is rotated or skewed.
    ///
    /// The pen here is one-dimensional and cannot follow rotated text. Rather
    /// than cut the wrong glyphs, a rotated run is left alone and reported:
    /// under-redacting is visible, and over-redacting destroys content that
    /// was never meant to go.
    skewed: bool,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
}

impl Default for Pen {
    fn default() -> Pen {
        Pen {
            x: 0.0,
            y: 0.0,
            line_x: 0.0,
            line_y: 0.0,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            ctm: (1.0, 1.0, 0.0, 0.0),
            scale: (1.0, 1.0),
            skewed: false,
        }
    }
}

impl Pen {
    /// Moves to the next line, per `T*`.
    fn next_line(&mut self) {
        self.line_y -= self.leading;
        self.x = self.line_x;
        self.y = self.line_y;
    }

    /// The displacement of one decoded code, per 9.4.4.
    ///
    /// In *text space*, then scaled by the text matrix's horizontal factor,
    /// because the rectangle a caller gave is in page space.
    fn advance(&self, code: &tinker_pdf_cos::DecodedCode) -> f64 {
        // Word spacing applies to single-byte code 32 only — the classic bug
        // is applying it to a two-byte CID that happens to equal 32.
        let word = if code.code == 32 && code.bytes == 1 {
            self.word_spacing
        } else {
            0.0
        };
        (code.width / 1000.0 * self.size + self.char_spacing + word)
            * self.horizontal_scale
            * self.scale.0
    }

    /// The displacement of a whole string.
    fn advance_of(&self, bytes: &[u8], font: Option<&Font>) -> f64 {
        let Some(font) = font else { return 0.0 };
        font.decode(bytes).iter().map(|c| self.advance(c)).sum()
    }
}

/// Rewrites a content stream with redacted glyphs removed.
fn rewrite(
    content: &[u8],
    areas: &[Redaction],
    fonts: &HashMap<Vec<u8>, Arc<Font>>,
) -> (Vec<u8>, RedactionReport) {
    let mut out = Vec::with_capacity(content.len());
    let mut tokens = Tokenizer::new(content);
    let mut operands: Vec<Token> = Vec::new();
    let mut pen = Pen::default();
    let mut saved: Vec<Pen> = Vec::new();
    let mut font: Option<Arc<Font>> = None;
    let mut report = RedactionReport::default();

    while let Some(token) = tokens.next_token() {
        let Token::Operator(op) = &token else {
            operands.push(token);
            continue;
        };

        // Operands are counted from the end, because that is where the
        // operator's own arguments are regardless of what preceded them.
        let number = |back: usize| -> f64 {
            let index = operands.len().checked_sub(back + 1);
            match index.and_then(|i| operands.get(i)) {
                Some(Token::Number(v)) if v.is_finite() => *v,
                _ => 0.0,
            }
        };

        let mut rewritten = false;

        match op.as_slice() {
            b"cm" => {
                let (a, b, c, d) = (number(5), number(4), number(3), number(2));
                let (e, f) = (number(1), number(0));
                // Composed with whatever is already in force, because `cm`
                // concatenates rather than replaces (8.4.4).
                pen.ctm = (
                    pen.ctm.0 * a,
                    pen.ctm.1 * d,
                    pen.ctm.2 + e * pen.ctm.0,
                    pen.ctm.3 + f * pen.ctm.1,
                );
                if b.abs() > 1e-9 || c.abs() > 1e-9 {
                    pen.skewed = true;
                }
            }
            b"q" => saved.push(pen),
            b"Q" => {
                if let Some(restored) = saved.pop() {
                    pen = restored;
                }
            }
            b"BT" => {
                // The text matrices reset; the text *state* does not.
                pen.x = 0.0;
                pen.y = 0.0;
                pen.line_x = 0.0;
                pen.line_y = 0.0;
            }
            b"Tf" => {
                pen.size = number(0);
                font = operands
                    .len()
                    .checked_sub(2)
                    .and_then(|i| operands.get(i))
                    .and_then(|t| match t {
                        Token::Name(name) => Some(name.as_slice()),
                        _ => None,
                    })
                    .and_then(|name| fonts.get(name).map(Arc::clone));
            }
            b"Tc" => pen.char_spacing = number(0),
            b"Tw" => pen.word_spacing = number(0),
            b"Tz" => pen.horizontal_scale = number(0) / 100.0,
            b"TL" => pen.leading = number(0),
            b"Ts" => pen.rise = number(0),
            b"Td" => {
                pen.line_x += number(1);
                pen.line_y += number(0);
                pen.x = pen.line_x;
                pen.y = pen.line_y;
            }
            b"TD" => {
                pen.leading = -number(0);
                pen.line_x += number(1);
                pen.line_y += number(0);
                pen.x = pen.line_x;
                pen.y = pen.line_y;
            }
            b"Tm" => {
                let (a, b, c, d) = (number(5), number(4), number(3), number(2));
                pen.line_x = number(1);
                pen.line_y = number(0);
                pen.x = pen.line_x;
                pen.y = pen.line_y;
                // 9.4.2: the matrix scales the glyph space as well as placing
                // it, and `b`/`c` rotate or skew it.
                pen.scale = (a, d);
                pen.skewed = b.abs() > 1e-9 || c.abs() > 1e-9;
            }
            b"T*" => pen.next_line(),
            b"Tj" | b"'" | b"\"" => {
                if op.as_slice() != b"Tj" {
                    pen.next_line();
                }
                if op.as_slice() == b"\"" {
                    // `aw ac string "` sets both spacings before showing.
                    pen.word_spacing = number(2);
                    pen.char_spacing = number(1);
                }
                if let Some(Token::String(bytes)) = operands.last().cloned() {
                    let cut = redact_string(&bytes, &pen, font.as_deref(), areas);
                    if cut.removed > 0 {
                        report.operations += 1;
                        report.glyphs += cut.removed;
                        emit_array(&mut out, &cut.runs, pen.size);
                        rewritten = true;
                    }
                    pen.x += pen.advance_of(&bytes, font.as_deref());
                }
            }
            b"TJ" => {
                let hit = operands.iter().any(|t| match t {
                    Token::String(s) => redact_string(s, &pen, font.as_deref(), areas).removed > 0,
                    _ => false,
                });

                if hit {
                    report.operations += 1;
                    let mut runs: Vec<Run> = Vec::new();
                    let mut local = pen;
                    for token in &operands {
                        match token {
                            Token::String(s) => {
                                let cut = redact_string(s, &local, font.as_deref(), areas);
                                report.glyphs += cut.removed;
                                runs.extend(cut.runs);
                                local.x += local.advance_of(s, font.as_deref());
                            }
                            Token::Number(v) if v.is_finite() => {
                                let shift = v / 1000.0 * local.size * local.horizontal_scale;
                                runs.push(Run::Gap(shift));
                                local.x -= shift;
                            }
                            _ => {}
                        }
                    }
                    emit_array(&mut out, &runs, pen.size);
                    rewritten = true;
                    pen = local;
                } else {
                    for token in &operands {
                        match token {
                            Token::String(s) => pen.x += pen.advance_of(s, font.as_deref()),
                            Token::Number(v) if v.is_finite() => {
                                pen.x -= v / 1000.0 * pen.size * pen.horizontal_scale;
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        if !rewritten {
            for operand in &operands {
                write_token(&mut out, operand);
                out.push(b' ');
            }
            out.extend_from_slice(op);
            out.push(b'\n');
        }
        operands.clear();
    }

    (out, report)
}

/// A piece of a rewritten showing operation.
enum Run {
    /// Bytes that survived.
    Text(Vec<u8>),
    /// Horizontal space, in text-space units, where glyphs used to be — or an
    /// adjustment that was already in the original `TJ` array.
    Gap(f64),
}

/// Writes runs as a single `TJ` array.
///
/// Everything becomes `TJ` because that is the only showing operator that can
/// express "move by this much without drawing", which is exactly what a
/// removed glyph leaves behind.
fn emit_array(out: &mut Vec<u8>, runs: &[Run], size: f64) {
    out.push(b'[');
    for run in runs {
        match run {
            Run::Text(bytes) if bytes.is_empty() => {}
            Run::Text(bytes) => {
                out.push(b'(');
                escape_into(out, bytes);
                out.extend_from_slice(b") ");
            }
            Run::Gap(shift) => {
                // A `TJ` number moves *backwards* by its value in thousandths
                // of the font size, so a forward gap is a negative number.
                if size.abs() > f64::EPSILON && shift.abs() > 1e-6 {
                    let thousandths = -shift / size * 1000.0;
                    out.extend_from_slice(format!("{thousandths:.3} ").as_bytes());
                }
            }
        }
    }
    out.extend_from_slice(b"] TJ\n");
}

/// The result of cutting one string.
struct Cut {
    runs: Vec<Run>,
    removed: usize,
}

/// Removes the glyphs of `bytes` that fall inside a redaction.
fn redact_string(bytes: &[u8], pen: &Pen, font: Option<&Font>, areas: &[Redaction]) -> Cut {
    // A rotated or skewed run cannot be measured by a pen that only moves
    // along one axis. Leaving it is the safe failure: an under-redaction is
    // visible to whoever checks, while cutting glyphs computed from wrong
    // positions destroys text that was never meant to go.
    let Some(font) = font.filter(|_| !pen.skewed) else {
        return Cut {
            runs: vec![Run::Text(bytes.to_vec())],
            removed: 0,
        };
    };

    let mut runs: Vec<Run> = Vec::new();
    let mut kept: Vec<u8> = Vec::new();
    let mut gap = 0.0f64;
    let mut removed = 0usize;
    let mut x = pen.x;

    for code in font.decode(bytes) {
        let advance = pen.advance(&code);

        // The glyph's box, approximated from its advance and the font size.
        // Approximating is right here: an exact outline would let a descender
        // poking one hundredth of a point into the box decide the redaction,
        // and erring towards removal is the safe direction anyway.
        // Text space to page space: the transformation matrix in force when
        // the glyph was shown. The rectangle a caller gave is in page space,
        // so the glyph has to be measured there and not in whatever space the
        // content stream happened to be using.
        let to_page_x = |v: f64| v * pen.ctm.0 + pen.ctm.2;
        let to_page_y = |v: f64| v * pen.ctm.1 + pen.ctm.3;

        let x0 = to_page_x(x.min(x + advance));
        let x1 = to_page_x(x.max(x + advance));
        let y0 = to_page_y(pen.y + pen.rise * pen.scale.1);
        // The height scales with the text matrix too, so a `Tm` that sets the
        // size leaves a box of the right height rather than one em tall.
        let y1 = to_page_y(pen.y + pen.rise * pen.scale.1 + pen.size * pen.scale.1);
        let inside = areas.iter().any(|redaction| {
            let a = redaction.area;
            x1 > a.x0 && x0 < a.x1 && y1 > a.y0 && y0 < a.y1
        });

        if inside {
            removed += 1;
            if !kept.is_empty() {
                runs.push(Run::Text(std::mem::take(&mut kept)));
            }
            gap += advance;
        } else {
            if gap != 0.0 {
                runs.push(Run::Gap(std::mem::take(&mut gap)));
            }
            encode_code(&mut kept, code.code, code.bytes);
        }
        x += advance;
    }

    if !kept.is_empty() {
        runs.push(Run::Text(kept));
    }
    if gap != 0.0 {
        // A trailing gap still matters: it holds the pen's position for
        // whatever the next operator shows.
        runs.push(Run::Gap(gap));
    }

    Cut { runs, removed }
}

/// Re-encodes a code as the bytes it was decoded from.
fn encode_code(out: &mut Vec<u8>, code: u32, width: u8) {
    let width = width.clamp(1, 4);
    for shift in (0..width).rev() {
        out.push(((code >> (u32::from(shift) * 8)) & 0xFF) as u8);
    }
}

fn escape_into(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

fn write_token(out: &mut Vec<u8>, token: &Token) {
    match token {
        Token::Number(v) => out.extend_from_slice(format!("{v}").as_bytes()),
        Token::String(s) => {
            out.push(b'(');
            escape_into(out, s);
            out.push(b')');
        }
        Token::Name(n) => {
            out.push(b'/');
            out.extend_from_slice(n);
        }
        Token::ArrayOpen => out.push(b'['),
        Token::ArrayClose => out.push(b']'),
        Token::DictOpen => out.extend_from_slice(b"<<"),
        Token::DictClose => out.extend_from_slice(b">>"),
        Token::Bool(true) => out.extend_from_slice(b"true"),
        Token::Bool(false) => out.extend_from_slice(b"false"),
        Token::Null => out.extend_from_slice(b"null"),
        Token::Operator(o) => out.extend_from_slice(o),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::{DocumentBuilder, WriteMode, WriteOptions};

    fn document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, text);
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Every stream in a document, decompressed. The acceptance test for a
    /// redaction is that the needle is absent from all of them — not from the
    /// one stream we happen to look at.
    fn all_streams(doc: &CosDocument) -> String {
        let mut out = Vec::new();
        for number in 1..=doc.max_object_number() {
            if let Ok(bytes) = doc.stream_decoded(ObjRef::new(number, 0)) {
                out.extend_from_slice(&bytes);
                out.push(b'\n');
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn redact(doc: Arc<CosDocument>, areas: &[Redaction]) -> (Vec<u8>, RedactionReport) {
        let mut editor = DocumentEditor::new(doc);
        let report = apply(&mut editor, 0, areas).expect("the page exists");
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        (bytes, report)
    }

    /// Redacts, reopens, and returns every stream the result contains.
    fn redact_to_streams(doc: Arc<CosDocument>, areas: &[Redaction]) -> (String, RedactionReport) {
        let (bytes, report) = redact(doc, areas);
        let reopened = CosDocument::open(bytes).expect("it reopens");
        (all_streams(&reopened), report)
    }

    /// Covers the second word of `PUBLIC SECRET` and nothing else.
    ///
    /// `PUBLIC` ends at about x=55.3 in 12pt Helvetica, so the band starts
    /// past that. A glyph that merely touches the band is removed, which is
    /// the safe direction but does mean the boundary is not arbitrary.
    fn second_word() -> Redaction {
        Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        }
    }

    /// The test that matters.
    #[test]
    fn redacted_bytes_are_absent_from_every_stream() {
        let doc = document("PUBLIC SECRET");
        assert!(
            all_streams(&doc).contains("SECRET"),
            "the needle starts out present, or the test proves nothing"
        );

        let (streams, report) = redact_to_streams(doc, &[second_word()]);

        assert!(report.glyphs > 0, "glyphs were removed");
        assert!(
            !streams.contains("SECRET"),
            "the redacted bytes must be gone from the file, got: {streams}"
        );
    }

    #[test]
    fn text_outside_the_area_survives() {
        let doc = document("PUBLIC SECRET");
        let (streams, _) = redact_to_streams(doc, &[second_word()]);
        assert!(
            streams.contains("PUBLIC"),
            "text outside the rectangle is untouched"
        );
    }

    /// Reading the page back through the engine's own extractor, which is how
    /// anyone looking for the secret would read it.
    #[test]
    fn extraction_no_longer_finds_it() {
        let doc = document("PUBLIC SECRET");
        let (bytes, _) = redact(doc, &[second_word()]);

        let reopened = crate::Document::open(bytes).expect("it reopens");
        let page = reopened.page(0).expect("the page is there");
        let text = page.text().plain_text();
        assert!(
            !text.contains("SECRET"),
            "the extractor must not find it either, got: {text:?}"
        );
        assert!(
            text.contains("PUBLIC"),
            "but it still finds the rest, got: {text:?}"
        );
    }

    #[test]
    fn a_redaction_covering_nothing_removes_nothing() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                area: Rect {
                    x0: 1000.0,
                    y0: 1000.0,
                    x1: 1100.0,
                    y1: 1100.0,
                },
                mark: false,
            }],
        );

        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn an_empty_redaction_list_is_a_no_op() {
        let doc = document("PUBLIC SECRET");
        let (streams, report) = redact_to_streams(doc, &[]);
        assert_eq!(report, RedactionReport::default());
        assert!(streams.contains("PUBLIC SECRET"));
    }

    #[test]
    fn a_marked_redaction_paints_the_area() {
        let doc = document("PUBLIC SECRET");
        let area = Redaction {
            mark: true,
            ..second_word()
        };
        let (streams, _) = redact_to_streams(doc, &[area]);

        assert!(streams.contains(" re f"), "a rectangle is painted");
        assert!(streams.contains("0 g"), "in black");
        assert!(!streams.contains("SECRET"), "and the text is still gone");
    }

    /// Builds a page whose text is placed by a scaling `Tm` rather than by
    /// `Td` with a sized font — `/F0 1 Tf` plus `12 0 0 12 x y Tm` is an
    /// ordinary way to write twelve-point text.
    fn scaled_document(text: &str) -> Arc<CosDocument> {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 100.0, |page| {
            page.raw(
                format!(
                    "BT /F0 1 Tf 12 0 0 12 10 50 Tm ({text}) Tj ET
"
                )
                .as_bytes(),
            );
        });
        Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
    }

    /// Reading only `Tm`'s translation left every advance twelve times too
    /// small, so the positions drifted further wrong across the line and the
    /// rectangle cut the wrong glyphs — or none at all.
    #[test]
    fn a_scaling_text_matrix_places_the_glyphs() {
        let doc = scaled_document("PUBLIC SECRET");
        assert!(all_streams(&doc).contains("SECRET"));

        let (streams, report) = redact_to_streams(doc, &[second_word()]);
        assert!(report.glyphs > 0, "something was found to cut");
        assert!(
            !streams.contains("SECRET"),
            "the second word is gone: {streams}"
        );
        assert!(streams.contains("PUBLIC"), "and the first survives");
    }

    /// A rotated run cannot be measured by a pen that moves along one axis.
    /// Leaving it is the safe failure: an under-redaction is visible, while
    /// cutting glyphs from wrong positions destroys text that was never meant
    /// to go.
    #[test]
    fn a_rotated_run_is_left_alone_rather_than_cut_wrongly() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 400.0, |page| {
            // A quarter turn: b and c are non-zero.
            page.raw(
                b"BT /F0 12 Tf 0 1 -1 0 200 50 Tm (SECRET) Tj ET
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        let everywhere = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 400.0,
                y1: 400.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[everywhere]);
        assert_eq!(report.glyphs, 0, "nothing was cut");
        assert!(
            streams.contains("SECRET"),
            "and the text is intact rather than half-removed"
        );
    }

    /// A redaction rectangle is in page space. Content drawn under a `cm` is
    /// in some other space, and ignoring the transform measured every glyph in
    /// the wrong one — so the rectangle covered the wrong part of the line, or
    /// nothing at all.
    #[test]
    fn a_transform_is_mapped_into_page_space() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            // Everything at double scale: the text is written at (5, 25) in
            // its own space, which is (10, 50) on the page.
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 25 Td (PUBLIC SECRET) Tj ET Q
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));
        assert!(all_streams(&doc).contains("SECRET"));

        // In page space the text starts at x = 10 and is 12pt tall. `PUBLIC`
        // ends near x = 65, so a band from 57 rightwards takes the second word.
        let band = Redaction {
            area: Rect {
                x0: 57.0,
                y0: 45.0,
                x1: 400.0,
                y1: 70.0,
            },
            mark: false,
        };

        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0, "the transform was followed");
        assert!(!streams.contains("SECRET"), "got: {streams}");
        assert!(streams.contains("PUBLIC"), "and the first word survives");
    }

    /// The transform is restored by `Q`, so a rectangle over content outside
    /// the `q`/`Q` pair is not measured through it.
    #[test]
    fn a_transform_does_not_outlive_its_q() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(400.0, 200.0, |page| {
            page.raw(
                b"q 2 0 0 2 0 0 cm BT /F0 6 Tf 5 60 Td (SCALED) Tj ET Q
                  BT /F0 12 Tf 10 30 Td (PLAIN) Tj ET
",
            );
        });
        let doc = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

        // A band over the lower line only, in page space.
        let band = Redaction {
            area: Rect {
                x0: 0.0,
                y0: 25.0,
                x1: 400.0,
                y1: 45.0,
            },
            mark: false,
        };
        let (streams, report) = redact_to_streams(doc, &[band]);
        assert!(report.glyphs > 0);
        assert!(!streams.contains("PLAIN"), "the lower line went");
        assert!(
            streams.contains("SCALED"),
            "and the scaled line, which sits higher, stayed: {streams}"
        );
    }

    #[test]
    fn a_page_that_does_not_exist_is_refused() {
        let doc = document("text");
        let mut editor = DocumentEditor::new(doc);
        assert!(apply(&mut editor, 9, &[second_word()]).is_none());
    }

    /// Removing the middle of a line must not shift what follows it: the gap
    /// is re-emitted as a `TJ` displacement.
    #[test]
    fn surviving_text_keeps_its_position() {
        let doc = document("AAA BBB CCC");
        let (streams, report) = redact_to_streams(
            doc,
            &[Redaction {
                // A band over the middle word only. In 12pt Helvetica
                // `AAA ` ends at 37.35 and ` CCC` starts at 61.36, so this
                // takes all three Bs and neither space.
                area: Rect {
                    x0: 37.5,
                    y0: 45.0,
                    x1: 61.0,
                    y1: 70.0,
                },
                mark: false,
            }],
        );

        assert!(report.glyphs > 0);
        assert!(streams.contains("] TJ"), "re-emitted as an array");
        assert_eq!(report.glyphs, 3, "the three Bs and nothing else");
        assert!(
            streams.contains("AAA") && streams.contains("CCC"),
            "the outer words survive: {streams}"
        );
        assert!(!streams.contains("BBB"), "the middle word is gone");
    }

    /// A content stream the tokenizer cannot make sense of must come back
    /// intact rather than truncated — never fail the page (ruling 2).
    #[test]
    fn garbage_content_survives_the_rewrite() {
        let fonts = HashMap::new();
        let content = b"q 1 0 0 1 0 0 cm ) ) ) >> BI garbage EI Q";
        let (out, report) = rewrite(content, &[second_word()], &fonts);
        assert_eq!(report, RedactionReport::default());
        assert!(out.contains(&b'q'), "the operators survive");
    }
}
