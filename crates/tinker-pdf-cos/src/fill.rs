//! Filling form fields and rebuilding their appearances (12.7.4.3).
//!
//! Setting `/V` is the easy half and the useless half: a value with no
//! matching appearance stream shows up in a viewer that regenerates
//! appearances and nowhere else, which is why filled forms so often print
//! blank. So every fill here rewrites the widget's `/AP` to match.
//!
//! The alternative — setting `/NeedAppearances` and hoping — is what produces
//! those files. This module clears that flag rather than setting it, because
//! once the appearances are right, asking viewers to rebuild them can only
//! make things worse.

use tinker_pdf_font::Sfnt;
use tinker_pdf_shape::bidi::{reorder, BaseDirection, Paragraph};
use tinker_pdf_shape::shape::{itemize, Shaper};
use tinker_pdf_shape::ShapedGlyph;

use crate::build::{close_array, number};
use crate::doc::CosDocument;
use crate::font::{self, Font, FontKind};
use crate::form::{self, FieldKind};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::pages::Rect;
use crate::warn::{WarningKind, WarningSink};
use crate::write::StreamData;

/// Splits a `/DA` string into its font name, size, and everything else.
///
/// The remainder is replayed verbatim rather than interpreted, so a colour
/// this build does not understand still comes out right. The `Tf` operands
/// must be removed as *tokens*: dropping bytes leaves a stray number on the
/// stack, and the next operator consumes the wrong one.
fn operators(da: &[u8]) -> (Vec<u8>, f64, Vec<u8>) {
    let tokens: Vec<Vec<u8>> = da
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| !t.is_empty())
        .map(<[u8]>::to_vec)
        .collect();

    let mut font = Vec::new();
    let mut size = 0.0;
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == b"Tf" {
            if index >= 2 {
                font = tokens[index - 2]
                    .strip_prefix(b"/")
                    .unwrap_or(&tokens[index - 2])
                    .to_vec();
                size = std::str::from_utf8(&tokens[index - 1])
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);
                rest.truncate(rest.len().saturating_sub(2));
            }
            index += 1;
            continue;
        }
        rest.push(tokens[index].clone());
        index += 1;
    }

    if font.is_empty() {
        font = b"Helv".to_vec();
    }
    let mut joined = rest.join(&b' ');
    if joined.is_empty() {
        joined = b"0 g".to_vec();
    }
    (font, size, joined)
}

/// The width of a string in text-space units per 1000, using the font's own
/// metrics.
fn width_of(font: Option<&Font>, text: &str) -> f64 {
    let Some(font) = font else {
        // Half an em per character is close enough to keep auto-sizing sane
        // when the font cannot be read at all.
        return text.chars().count() as f64 * 500.0;
    };
    // A field's text is written as a single-byte string, so the code is the
    // byte, and characters outside the encoding are dropped by the writer too.
    text.chars()
        .map(|c| {
            let code = u32::from(c);
            font.width_of(code).0
        })
        .sum()
}

/// Escapes a string for a literal in a content stream (7.3.4.2), naming every
/// character it could not write.
///
/// A simple font addresses a *byte*, so a character above the single-byte
/// range has no code here at all. It is still drawn — ruling 2 degrades rather
/// than failing, and a field that vanished would be worse than one that shows
/// a mark — but the mark is now accompanied by a
/// [`WarningKind::FieldCharacterUnrepresentable`] naming the character, which
/// is the half ruling 10 adds. Before milestone 8 this function wrote `b'?'`
/// and said nothing, so a form whose Arabic value had become a row of question
/// marks was indistinguishable from one that had been filled correctly.
///
/// The shaped path is the *other* half of the answer, and where a composite
/// `/DA` font makes it available nothing reaches here; see [`Composite`].
fn escape(out: &mut Vec<u8>, text: &str, unwritable: &mut Vec<char>) {
    for c in text.chars() {
        let code = u32::from(c);
        let byte = if code < 256 {
            code as u8
        } else {
            if !unwritable.contains(&c) {
                unwritable.push(c);
            }
            b'?'
        };
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
}

/// A `/DA` font this build can shape a field's value against.
///
/// Milestone 8 of `docs/design/shaping.md`, and the reason the milestone was
/// blocked until now: `text_appearance` reaches its font through the AcroForm
/// `/DR`, and a [`Font`] that knew every width and no outline had nothing for
/// a shaper to work on. [`Font::program`] is the entry that changed.
///
/// # The conditions, each checked rather than assumed
///
/// 1. **The font is composite** (9.7). A simple font addresses a byte, so it
///    cannot name a glyph beyond 255 whatever is embedded in it.
/// 2. **The writing mode is horizontal.** 9.7.4.3's vertical CMaps advance
///    the pen downward and this module places every glyph along a baseline.
///    Selecting the right glyphs and drawing them in a row a viewer will
///    stack is a worse answer than a question mark, so a `-V` encoding keeps
///    the single-byte path.
/// 3. **Its `/Encoding` can be read backwards to a code.** This is the
///    condition that used to say `/Identity-H` and no longer does. A writer
///    needs to go from a glyph to a *code*, and 9.7.5's CMaps are written to
///    be read the other way — but "written to be read forwards" is not "not
///    invertible". `tinker_pdf_font::CMap::code_for_cid` gathers every code
///    the CMap's own tables could have meant by a CID and returns the first that maps
///    **back**, so the round trip is checked rather than assumed and a
///    `cidchar` override cannot be inverted into a code that now means
///    something else. That covers `/Identity-H` and `/Identity-V` (where the
///    code is the CID outright), every embedded CMap stream, and — where this
///    build compiled the tables in — every registry CMap of 9.7.5.2.
///    [`Font::cid_for_gid`] inverts the remaining step, `/CIDToGIDMap`.
/// 4. **The descriptor embeds a program `tinker_pdf_font::Sfnt` reads.** A
///    bare CFF (`/FontFile3 /Subtype /Type1C` or `/CIDFontType0C`) is not an
///    sfnt and carries no `GSUB`/`GPOS` for this crate to execute, so a
///    CIDFontType0 face is still outside what this claims.
struct Composite {
    /// The embedded program, decoded once per appearance rather than once per
    /// line: a multiline field would otherwise inflate a megabyte per row.
    program: Vec<u8>,
}

/// Whether a field's value can be shaped against its `/DA` font, and where it
/// cannot, whether that is worth saying out loud.
enum Shaping {
    /// It can, against this program.
    Yes(Composite),
    /// It cannot, and the single-byte path is the right answer for this font
    /// — a simple font, a vertical one, one that embeds no sfnt. Every
    /// character that path cannot write is still named (ruling 10); there is
    /// just nothing to say about the *font* beyond what the file already
    /// says.
    No,
    /// It cannot, and the reason is this **build** rather than this document.
    ///
    /// The registry's code-to-CID tables are a megabyte and live behind the
    /// `cmap-predefined` cargo feature, so a `--no-default-features` build
    /// can read a `UniJIS-UCS2-H` field's widths and codespaces and still
    /// have nothing to invert. That refusal is typed and named against the
    /// field, because a capability that quietly depends on a feature is the
    /// failure this repository already named once in PDF/A: a verdict that
    /// depends on a feature is not a verdict, and neither is a fill.
    Refused(WarningKind),
}

/// One glyph of a shaped line, in the thousandths of an em [`width_of`]
/// answers in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    /// The character code to write. Under `/Identity-H` this is the CID;
    /// under any other CMap it is whatever code maps to it (9.7.5.2).
    code: u32,
    /// How many bytes the code occupies, from the CMap's codespace ranges
    /// (9.7.6.2).
    ///
    /// Carried per glyph rather than per font because one CMap may have both
    /// widths: `90ms-RKSJ-H` takes one byte for ASCII and two for kanji, and
    /// writing `<0041>` where `<41>` was meant mis-splits every code after
    /// it.
    bytes: u8,
    /// Where the glyph's origin sits along the baseline, from the line's
    /// start.
    x: f64,
    /// Its origin off the baseline — 9.4.3's `Ts`.
    rise: f64,
}

/// What one line of a field's value shapes to.
struct ShapedLine {
    /// The glyphs, in the order they are **drawn** — left to right, whichever
    /// way the text reads.
    glyphs: Vec<Placed>,
    /// The line's whole advance, in thousandths of an em, which is what
    /// quadding and auto-sizing measure with.
    advance: f64,
    /// Characters the face has no glyph for, or that the font's own
    /// `/CIDToGIDMap` cannot address. Each becomes a warning naming the field
    /// (ruling 10).
    unwritable: Vec<char>,
}

impl Composite {
    /// The font behind a `/DA`, if the conditions above hold, and the typed
    /// reason where one of them fails for a reason this build owns.
    fn of(doc: &CosDocument, dict: &Dict, font: &Font) -> Shaping {
        if font.kind() != FontKind::Type0 {
            return Shaping::No;
        }
        if font.is_vertical() {
            return Shaping::No;
        }
        // A composite font whose `/Encoding` names nothing 9.7.5.2 defines
        // has no codes to write at all. `font::read` has already said which
        // name it was (`PredefinedCMapUnknown`); repeating it here would only
        // double it, and the single-byte path still names every character.
        if !font.has_encoding_cmap() {
            return Shaping::No;
        }
        // 9.7.5.2: the registry defines this CMap and this build left its
        // code-to-CID table out, so there is nothing to invert. Declared
        // before the program is decoded, because the answer does not depend
        // on the program and a megabyte should not be inflated to reach it.
        if font.encoding_is_approximate() {
            // The name is what makes the warning actionable, and the
            // dictionary is where a name lives — `Font` models the CMap's
            // forward direction and has no name to give.
            return match doc.resolve_key(dict, doc.intern(b"Encoding")).as_name() {
                Some(name) => Shaping::Refused(WarningKind::PredefinedCMapApproximate(name)),
                // An embedded CMap stream that inherited approximateness up a
                // `usecmap` chain: real, and with no name of its own to
                // report.
                None => Shaping::No,
            };
        }
        let Some(program) = font
            .program()
            .and_then(|p| doc.stream_decoded(p.stream).ok())
        else {
            return Shaping::No;
        };
        // Parsed here and thrown away, so that a program which is not an sfnt
        // — a bare CFF, or bytes that are not a font at all — declines now
        // rather than per line.
        if Sfnt::parse(&program).is_none() {
            return Shaping::No;
        }
        Shaping::Yes(Composite { program })
    }

    /// Shapes one line and places every glyph, in visual order.
    ///
    /// # Two steps, and they are the consumer's two steps
    ///
    /// `docs/design/shaping.md`'s pipeline ends with the caller reordering:
    /// the shaper returns **logical** order, UAX #9's rule L2 orders the runs,
    /// and a right-to-left run's glyphs are then walked backwards. Both happen
    /// here, which is what puts an Arabic value's last letter at the left of
    /// the box where a reader of the script expects it.
    ///
    /// A line with no glyphs at all comes back empty rather than as `None`:
    /// an empty field value is not a failure.
    fn line(&self, font: &Font, text: &str) -> ShapedLine {
        let mut out = ShapedLine {
            glyphs: Vec::new(),
            advance: 0.0,
            unwritable: Vec::new(),
        };
        let Some(face) = Sfnt::parse(&self.program) else {
            return out;
        };
        let upem = f64::from(face.units_per_em.max(1));
        let scale = |units: i32| f64::from(units) * 1000.0 / upem;
        let shaper = Shaper::new(&face);
        let paragraph = Paragraph::new(text, BaseDirection::Auto);
        let runs = itemize(text, &paragraph);
        let shaped: Vec<_> = runs.iter().map(|run| shaper.shape(text, run)).collect();
        let levels: Vec<_> = runs.iter().map(|run| run.level).collect();

        let mut pen = 0.0f64;
        for index in reorder(&levels) {
            let Some(run) = shaped.get(index) else {
                continue;
            };
            let glyphs: Vec<ShapedGlyph> = if run.direction().is_forward() {
                run.glyphs().to_vec()
            } else {
                run.glyphs().iter().rev().copied().collect()
            };
            for glyph in glyphs {
                match self.code_for(font, glyph.glyph) {
                    Some((code, bytes)) => out.glyphs.push(Placed {
                        code,
                        bytes,
                        x: pen + scale(glyph.x_offset),
                        rise: scale(glyph.y_offset),
                    }),
                    None => {
                        // The cluster is a byte offset into the text this run
                        // was shaped from, which is the line, so the character
                        // the reader typed is recoverable and is what the
                        // warning names.
                        let at = usize::try_from(glyph.cluster).unwrap_or(0);
                        if let Some(c) = text.get(at..).and_then(|rest| rest.chars().next()) {
                            if !out.unwritable.contains(&c) {
                                out.unwritable.push(c);
                            }
                        }
                    }
                }
                pen += scale(glyph.x_advance);
            }
        }
        out.advance = pen;
        out
    }

    /// The code that draws `glyph` and its byte width, or `None` where the
    /// font cannot name it.
    ///
    /// Two inversions, in the order 9.7.4 composes them forwards: the glyph
    /// back through `/CIDToGIDMap` to a CID, the CID back through the
    /// encoding CMap to a code. Either may refuse, and a refusal at either
    /// step is a character this appearance will not draw.
    ///
    /// Glyph 0 is `.notdef` and is refused rather than written: it is the
    /// face saying it has nothing for that character, and drawing the empty
    /// box while reporting success is the invisible failure ruling 10 exists
    /// to prevent.
    fn code_for(&self, font: &Font, glyph: u16) -> Option<(u32, u8)> {
        if glyph == 0 {
            return None;
        }
        font.code_for_cid(font.cid_for_gid(glyph)?)
    }
}

/// Writes one shaped line as a composite text run at `(x, y)`.
///
/// The `TJ` numbers are the difference between where the *reader's* pen will
/// be — which advances by `/W`, not by the shaper's own `hmtx` figure — and
/// where this crate put each glyph. Absorbing the difference glyph by glyph
/// rather than letting it accumulate is what `DocumentBuilder::glyph_run`
/// does for the same reason, and it is `docs/design/shaping.md`'s answer to
/// its own third risk: what was measured is what is drawn.
fn write_shaped(out: &mut Vec<u8>, font: &Font, line: &ShapedLine, x: f64, y: f64) {
    out.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
    let mut pen = 0.0f64;
    let mut rise = 0.0f64;
    let mut open = false;
    for placed in &line.glyphs {
        if placed.rise != rise {
            close_array(out, &mut open);
            rise = placed.rise;
            out.extend_from_slice(format!("{} Ts\n", number(rise / 1000.0)).as_bytes());
        }
        if !open {
            out.push(b'[');
            open = true;
        }
        // 9.4.3: a `TJ` number moves the pen **backwards** by that many
        // thousandths of a text space unit. Both sides here are already in
        // thousandths of an em, so the number is the gap outright.
        let adjust = pen - placed.x;
        if number(adjust) != "0" {
            if out.last() == Some(&b'>') {
                out.push(b' ');
            }
            out.extend_from_slice(number(adjust).as_bytes());
            out.push(b' ');
        }
        // 9.7.6.2: a code is as many bytes as its codespace range says, and
        // the string carries no separators — so the width is what tells a
        // reader where this code ends. Two hex digits per byte, zero-padded,
        // whatever the code's magnitude; `/Identity-H`'s two bytes are the
        // common case rather than the only one.
        let digits = usize::from(placed.bytes).clamp(1, 4) * 2;
        let mask = u32::MAX >> (32 - digits * 4);
        out.extend_from_slice(
            format!("<{:0digits$X}>", placed.code & mask, digits = digits).as_bytes(),
        );
        pen = placed.x + font.width_of(placed.code).0;
    }
    close_array(out, &mut open);
    if rise != 0.0 {
        // `Ts` is graphics state and outlives the text object, so a run that
        // left one set would tilt whatever a viewer drew next.
        out.extend_from_slice(b"0 Ts\n");
    }
}

/// How a text field lays its value out.
///
/// A struct rather than five more parameters: the call had grown to eight,
/// which is the point at which the order of `bool, i64, Option<i64>` stops
/// being checkable by eye and a transposition compiles.
pub struct TextLayout<'a> {
    /// The default appearance string: font, size and colour.
    pub da: &'a [u8],
    /// `/Q`: 0 left, 1 centred, 2 right.
    pub quadding: i64,
    /// `/Ff` bit 13: the field wraps.
    pub multiline: bool,
    /// `/MaxLen`, when `/Ff` bit 25 makes the field a comb.
    pub comb: Option<i64>,
    /// The form's `/DR`, where the `/DA` finds its font.
    pub resources: Option<&'a Dict>,
    /// The field object this appearance is for, so that a character the font
    /// cannot draw is reported against something (ruling 10).
    ///
    /// `None` is a caller building an appearance outside a document's field
    /// tree — the tests below do it — and costs only the address on the
    /// warning, never the warning itself.
    pub field: Option<ObjRef>,
}

/// Builds the appearance stream for a text field's widget.
///
/// `rect` is the widget's rectangle; the stream is written in a box of the
/// same size anchored at the origin, which is what 12.5.5's mapping expects
/// and what every other producer writes.
#[must_use]
pub fn text_appearance(
    doc: &CosDocument,
    rect: Rect,
    value: &str,
    layout: &TextLayout<'_>,
) -> StreamData {
    let TextLayout {
        da,
        quadding,
        multiline,
        comb,
        resources,
        field,
    } = *layout;
    let (font_name, mut size, colour) = operators(da);
    let (w, h) = (rect.x1 - rect.x0, rect.y1 - rect.y0);

    let font_dict = resources
        .and_then(|dr| doc.resolve_key(dr, doc.intern(b"Font")).as_dict().cloned())
        .and_then(|fonts| fonts.get_ref(doc.intern(&font_name)))
        .and_then(|r| doc.get(r).ok())
        .and_then(|object| object.as_dict().cloned());
    let font = font_dict.as_ref().map(|dict| font::read(doc, dict));
    // Milestone 8: whether this field's value can be shaped and written as
    // glyphs rather than as bytes. `None` keeps every line on the single-byte
    // path this module has always had, which is still right for the `/Helv`
    // most forms name.
    let mut refusals: Vec<WarningKind> = Vec::new();
    let composite = match font_dict
        .as_ref()
        .zip(font.as_ref())
        .map_or(Shaping::No, |(dict, font)| Composite::of(doc, dict, font))
    {
        Shaping::Yes(composite) => Some(composite),
        Shaping::No => None,
        Shaping::Refused(kind) => {
            refusals.push(kind);
            None
        }
    };

    // 12.7.3.3: two units of padding on each side is the convention, and
    // matching it is what keeps a regenerated appearance from jumping.
    const PAD: f64 = 2.0;
    let inner_w = (w - PAD * 2.0).max(0.0);
    let inner_h = (h - PAD * 2.0).max(0.0);

    let lines: Vec<&str> = if multiline {
        value.split('\n').collect()
    } else {
        // A single-line field shows one line whatever the value contains.
        vec![value.lines().next().unwrap_or(value)]
    };

    // Shaped **once**, before auto-sizing, because auto-sizing measures the
    // same runs that are about to be drawn. Measuring one way and drawing
    // another is the two-paths-disagree failure `metrics.rs` warns about, and
    // a joined Arabic word is narrower than its letters by enough to see.
    let shaped: Option<Vec<ShapedLine>> = composite
        .as_ref()
        .zip(font.as_ref())
        .map(|(composite, font)| lines.iter().map(|l| composite.line(font, l)).collect());
    let widths: Vec<f64> = match &shaped {
        Some(shaped) => shaped.iter().map(|line| line.advance).collect(),
        None => lines
            .iter()
            .map(|line| width_of(font.as_ref(), line))
            .collect(),
    };
    let mut unwritable: Vec<char> = Vec::new();
    if let Some(shaped) = &shaped {
        for line in shaped {
            for c in &line.unwritable {
                if !unwritable.contains(c) {
                    unwritable.push(*c);
                }
            }
        }
    }

    if size <= 0.0 {
        // Auto-size. The height budget is what makes it legible; the width
        // budget is what keeps it inside the box.
        let widest = widths.iter().copied().fold(0.0f64, f64::max) / 1000.0;
        let by_height = if multiline {
            inner_h / (lines.len() as f64).max(1.0) / 1.15
        } else {
            inner_h * 0.72
        };
        let by_width = if widest > 0.0 {
            inner_w / widest
        } else {
            by_height
        };
        size = by_height.min(by_width).clamp(1.0, 12.0);
    }

    let leading = size * 1.15;
    let mut content = Vec::new();
    // 12.7.4.3: the marked-content pair is how a viewer recognises a
    // regenerated field appearance as its own rather than as page content.
    content.extend_from_slice(b"/Tx BMC\nq\n");
    // Clipped to the box, so an over-long value is cut off rather than
    // spilling across the page.
    content.extend_from_slice(format!("{PAD} {PAD} {inner_w:.2} {inner_h:.2} re W n\n").as_bytes());
    content.extend_from_slice(b"BT\n");
    content.extend_from_slice(b"/");
    content.extend_from_slice(&font_name);
    content.extend_from_slice(format!(" {size:.2} Tf\n").as_bytes());
    content.extend_from_slice(&colour);
    content.push(b'\n');

    // 12.7.4.3: a comb field divides its width into /MaxLen equal cells and
    // centres one character in each. Laid out as ordinary text it drifts out
    // of the printed boxes it exists to sit inside, character by character,
    // which is the whole reason the flag is there.
    if let Some(cells) = comb.filter(|n| *n > 0) {
        let cell = inner_w / cells as f64;
        let line = lines.first().copied().unwrap_or("");
        for (index, ch) in line.chars().take(cells as usize).enumerate() {
            let mut text = String::new();
            text.push(ch);
            // Each cell is shaped on its own, and that is the right answer
            // rather than a shortcut: a comb field draws one character per
            // printed box, so a joining script's letters are in the isolated
            // form in one whatever their neighbours are.
            let cell_shaped = composite
                .as_ref()
                .zip(font.as_ref())
                .map(|(composite, font)| composite.line(font, &text));
            let width = cell_shaped
                .as_ref()
                .map_or_else(|| width_of(font.as_ref(), &text), |line| line.advance)
                / 1000.0
                * size;
            // Centred in its cell, which is what makes the column line up
            // whatever the character is.
            let x = PAD + cell * index as f64 + (cell - width) / 2.0;
            let y = (h - size * 0.72) / 2.0;

            match (&cell_shaped, font.as_ref()) {
                (Some(cell_shaped), Some(font)) => {
                    for c in &cell_shaped.unwritable {
                        if !unwritable.contains(c) {
                            unwritable.push(*c);
                        }
                    }
                    write_shaped(&mut content, font, cell_shaped, x, y);
                }
                _ => {
                    content.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
                    content.push(b'(');
                    escape(&mut content, &text, &mut unwritable);
                    content.extend_from_slice(b") Tj\n");
                }
            }
        }

        content.extend_from_slice(b"ET\nQ\nEMC\n");
        report(doc, field, &refusals, &unwritable);
        return finish_appearance(doc, content, w, h, resources);
    }

    for (index, line) in lines.iter().enumerate() {
        let line_width = widths.get(index).copied().unwrap_or(0.0) / 1000.0 * size;
        // 12.7.4.3: /Q is 0 left, 1 centred, 2 right.
        let x = match quadding {
            1 => PAD + (inner_w - line_width) / 2.0,
            2 => PAD + inner_w - line_width,
            _ => PAD,
        }
        .max(PAD);

        let y = if multiline {
            // Multiline runs from the top down.
            rect_top_baseline(inner_h, size) - index as f64 * leading + PAD
        } else {
            // A single line is centred vertically, which is what a viewer
            // does and what makes a regenerated field sit where it did.
            (h - size * 0.72) / 2.0
        };

        match shaped
            .as_ref()
            .and_then(|s| s.get(index))
            .zip(font.as_ref())
        {
            Some((shaped, font)) => write_shaped(&mut content, font, shaped, x, y),
            None => {
                content.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
                content.push(b'(');
                escape(&mut content, line, &mut unwritable);
                content.extend_from_slice(b") Tj\n");
            }
        }
    }

    content.extend_from_slice(b"ET\nQ\nEMC\n");
    report(doc, field, &refusals, &unwritable);
    finish_appearance(doc, content, w, h, resources)
}

/// What this appearance could not do, against the field it could not do it to
/// (ruling 10).
///
/// Two kinds, and the order is the order a reader wants them in: the
/// **refusal** first, because it is the cause and it names the font or the
/// build, then one warning per character the appearance could not draw.
///
/// Distinct characters and not occurrences: a value of two hundred Devanagari
/// letters in a Latin face is one problem with one fix, and two hundred
/// warnings would spend the sink's whole budget saying so.
///
/// Both carry the field as their object, which is what makes a
/// `cmap-predefined`-off build's `predefined-cmap-approximate` here
/// distinguishable from the one `font::read` emits when the same font is
/// merely *read*: this one names a field, and it means a fill was refused.
fn report(doc: &CosDocument, field: Option<ObjRef>, refusals: &[WarningKind], unwritable: &[char]) {
    if refusals.is_empty() && unwritable.is_empty() {
        return;
    }
    let mut sink = WarningSink::new();
    sink.set_context(field);
    for kind in refusals {
        sink.warn(0, *kind);
    }
    for c in unwritable {
        sink.warn(
            0,
            WarningKind::FieldCharacterUnrepresentable { character: *c },
        );
    }
    doc.absorb(sink);
}

/// Wraps a field's operators in the form XObject that carries them.
fn finish_appearance(
    doc: &CosDocument,
    content: Vec<u8>,
    w: f64,
    h: f64,
    resources: Option<&Dict>,
) -> StreamData {
    let mut dict = Dict::new();
    dict.insert(Name::TYPE, Object::Name(doc.intern(b"XObject")));
    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Form")));
    dict.insert(
        doc.intern(b"BBox"),
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    dict.insert(
        doc.intern(b"Matrix"),
        Object::Array(vec![
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
            Object::Int(1),
            Object::Int(0),
            Object::Int(0),
        ]),
    );
    if let Some(dr) = resources {
        dict.insert(Name::RESOURCES, Object::Dict(dr.clone()));
    }

    StreamData {
        dict,
        data: content,
    }
}

/// Where the first baseline of a multiline field sits.
fn rect_top_baseline(inner_h: f64, size: f64) -> f64 {
    (inner_h - size * 0.85).max(0.0)
}

/// The rectangle of a widget annotation.
#[must_use]
pub fn widget_rect(doc: &CosDocument, widget: ObjRef) -> Option<Rect> {
    let object = doc.get(widget).ok()?;
    let dict = object.as_dict()?;
    doc.resolve_key(dict, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
        .filter(|r| !r.is_empty())
}

/// The `/DA` a widget should use: its own, its field's, or the form's.
#[must_use]
pub fn appearance_string(doc: &CosDocument, field: &form::Field, widget: ObjRef) -> Vec<u8> {
    if let Ok(object) = doc.get(widget) {
        if let Some(da) = object
            .as_dict()
            .and_then(|d| d.get(doc.intern(b"DA")))
            .and_then(Object::as_string)
        {
            return da.bytes.clone();
        }
    }
    field
        .default_appearance
        .clone()
        .unwrap_or_else(|| b"/Helv 0 Tf 0 g".to_vec())
}

/// A text field's quadding, inherited from the form when it says nothing.
#[must_use]
pub fn quadding(doc: &CosDocument, field: &form::Field) -> i64 {
    doc.get(field.reference)
        .ok()
        .and_then(|o| o.as_dict().and_then(|d| d.get_int(doc.intern(b"Q"))))
        .or_else(|| form::acro_form(doc).and_then(|f| f.get_int(doc.intern(b"Q"))))
        .unwrap_or(0)
}

/// 12.7.4.3 table 231: a text field that wraps.
pub const MULTILINE: i64 = 1 << 12;
/// A text field whose characters sit in fixed cells.
pub const COMB: i64 = 1 << 24;

/// Whether a value is one the field will accept from a user.
///
/// Refusing is better than truncating silently: a form filled with a value the
/// field rejects is a data error, and writing half of it hides that.
#[must_use]
pub fn accepts(field: &form::Field, value: &str) -> bool {
    if field.is_read_only() {
        return false;
    }
    accepts_value(field, value)
}

/// Whether a value fits the field, leaving aside who is writing it.
///
/// 12.7.4.1 table 227 says ReadOnly means the field "shall not be modified by
/// **the user**", and a calculate action is the document's own script rather
/// than a user — a calculated total is read-only precisely so that nothing
/// *but* the script writes it. So the read-only test lives in [`accepts`],
/// which is the user's door, and everything that is true of a value whoever
/// writes it lives here, where both doors share it. Splitting it any other way
/// gives the calculation path a second copy of the rules to drift from.
#[must_use]
pub fn accepts_value(field: &form::Field, value: &str) -> bool {
    match field.kind {
        FieldKind::Text => field
            .max_len
            .is_none_or(|max| max <= 0 || value.chars().count() as i64 <= max),
        FieldKind::ComboBox | FieldKind::ListBox => {
            // An editable combo takes anything; a list takes what it offers.
            const EDIT: i64 = 1 << 18;
            field.flags & EDIT != 0
                || field.options.is_empty()
                || field.options.iter().any(|o| o == value)
        }
        _ => false,
    }
}

/// Builds the `/V` object for a text or choice value.
#[must_use]
pub fn value_object(value: &str) -> Object {
    // 7.9.2.2: UTF-16BE with a byte-order mark is the only encoding that
    // covers everything, and it is what viewers write. Pure ASCII stays a
    // plain literal so simple files stay readable.
    if value.is_ascii() {
        return Object::String(PdfString::literal(value.as_bytes().to_vec()));
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(PdfString::hex(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> CosDocument {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
        CosDocument::open(bytes).expect("it opens")
    }

    fn rect() -> Rect {
        Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 20.0,
        }
    }

    fn text_of(stream: &StreamData) -> String {
        String::from_utf8_lossy(&stream.data).into_owned()
    }

    #[test]
    fn a_default_appearance_splits_into_font_size_and_colour() {
        let (font, size, colour) = operators(b"/Helv 12 Tf 0 0 1 rg");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 12.0);
        assert_eq!(colour, b"0 0 1 rg", "the colour is replayed verbatim");
    }

    /// The operands of the `Tf` must not be replayed as though they were
    /// colour operators, which would push a stray number onto the stack.
    #[test]
    fn the_font_operands_are_not_replayed() {
        let (_, _, colour) = operators(b"0 g /Helv 9 Tf");
        assert_eq!(colour, b"0 g");
    }

    #[test]
    fn a_missing_or_broken_appearance_string_still_yields_something_usable() {
        let (font, size, colour) = operators(b"");
        assert_eq!(font, b"Helv");
        assert_eq!(size, 0.0, "which means auto-size");
        assert_eq!(colour, b"0 g");

        let (font, size, _) = operators(b"/ 1e999 Tf");
        assert_eq!(font, b"Helv", "an empty name falls back");
        assert!(size.is_finite());
    }

    #[test]
    fn an_appearance_is_marked_as_a_field_appearance() {
        let doc = doc();
        let stream = text_appearance(
            &doc,
            rect(),
            "Ada",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        );
        let content = text_of(&stream);
        assert!(content.starts_with("/Tx BMC"), "got: {content}");
        assert!(content.ends_with("EMC\n"));
        assert!(content.contains("(Ada) Tj"));
    }

    /// The box is anchored at the origin whatever the widget's rectangle is,
    /// because 12.5.5 maps it onto that rectangle.
    #[test]
    fn the_box_is_the_widgets_size_at_the_origin() {
        let doc = doc();
        let offset = Rect {
            x0: 300.0,
            y0: 400.0,
            x1: 400.0,
            y1: 420.0,
        };
        let stream = text_appearance(
            &doc,
            offset,
            "x",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        );
        let bbox: Vec<f64> = stream
            .dict
            .get_array(doc.intern(b"BBox"))
            .expect("a box")
            .iter()
            .filter_map(Object::as_number)
            .collect();
        assert_eq!(bbox, vec![0.0, 0.0, 100.0, 20.0]);
    }

    #[test]
    fn the_value_is_clipped_to_the_box() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a very long value indeed",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert!(content.contains(" re W n"), "a clip is set: {content}");
    }

    #[test]
    fn quadding_moves_the_line() {
        let doc = doc();
        let left = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let centre = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 1,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let right = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 2,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));

        let x = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tm"))
                .and_then(|l| l.split_whitespace().nth(4))
                .and_then(|v| v.parse().ok())
                .expect("a text matrix")
        };
        assert!(x(&left) < x(&centre), "centred sits right of left");
        assert!(x(&centre) < x(&right), "and right of centred");
    }

    #[test]
    fn a_multiline_field_writes_a_line_each() {
        let doc = doc();
        let tall = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 60.0,
        };
        let content = text_of(&text_appearance(
            &doc,
            tall,
            "one\ntwo\nthree",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: true,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 3);
        assert!(content.contains("(one)") && content.contains("(three)"));
    }

    #[test]
    fn a_single_line_field_shows_one_line_of_a_multiline_value() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "one\ntwo",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 1);
        assert!(content.contains("(one)") && !content.contains("(two)"));
    }

    /// Size zero means auto, and it has to come out as a real number a
    /// tokenizer accepts rather than a zero that draws nothing.
    #[test]
    fn an_auto_sized_field_picks_a_real_size() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "Ada",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let size: f64 = content
            .lines()
            .find(|l| l.ends_with(" Tf"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .expect("a font size");
        assert!(size > 0.0 && size <= 12.0, "got {size}");
    }

    /// A value far too long for its box must shrink rather than run out of it.
    #[test]
    fn a_long_auto_sized_value_shrinks_to_fit() {
        let doc = doc();
        let short = text_of(&text_appearance(
            &doc,
            rect(),
            "hi",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let long = text_of(&text_appearance(
            &doc,
            rect(),
            "a value far longer than the box it has to fit inside of",
            &TextLayout {
                da: b"/Helv 0 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        let size = |content: &str| -> f64 {
            content
                .lines()
                .find(|l| l.ends_with(" Tf"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .expect("a size")
        };
        assert!(size(&long) < size(&short), "the long value shrank");
    }

    #[test]
    fn parentheses_in_a_value_are_escaped() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "a (b) \\ c",
            &TextLayout {
                da: b"/Helv 9 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert!(content.contains("(a \\(b\\) \\\\ c) Tj"), "got: {content}");
    }

    /// 12.7.4.3: a comb field divides its width into /MaxLen cells and centres
    /// one character in each. Laid out as ordinary text it drifts out of the
    /// printed boxes it exists to sit inside, character by character.
    #[test]
    fn a_comb_field_spreads_its_characters_across_cells() {
        let doc = doc();
        let wide = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 20.0,
        };
        let content = text_of(&text_appearance(
            &doc,
            wide,
            "ABCD",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: Some(4),
                resources: None,
                field: None,
            },
        ));

        let xs: Vec<f64> = content
            .lines()
            .filter(|l| l.ends_with(" Tm"))
            .filter_map(|l| l.split_whitespace().nth(4)?.parse().ok())
            .collect();

        assert_eq!(xs.len(), 4, "one placement per character: {content}");
        for pair in xs.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (40.0..=58.0).contains(&gap),
                "cells are the field width over /MaxLen apart, got {gap}"
            );
        }
    }

    /// More characters than cells cannot be laid out, so the overflow is
    /// dropped rather than drawn outside the last box.
    #[test]
    fn a_comb_field_stops_at_its_cell_count() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "ABCDEFGH",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: Some(3),
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 3);
    }

    /// The flag means nothing without /MaxLen, so the field lays out as
    /// ordinary text rather than as one enormous cell.
    #[test]
    fn a_comb_field_without_maxlen_lays_out_normally() {
        let doc = doc();
        let content = text_of(&text_appearance(
            &doc,
            rect(),
            "ABCD",
            &TextLayout {
                da: b"/Helv 10 Tf 0 g",
                quadding: 0,
                multiline: false,
                comb: None,
                resources: None,
                field: None,
            },
        ));
        assert_eq!(content.matches(" Tj").count(), 1, "one run, not four");
    }

    #[test]
    fn non_ascii_values_are_written_as_utf16() {
        let Object::String(ascii) = value_object("plain") else {
            panic!("a string");
        };
        assert!(!ascii.hex && ascii.bytes == b"plain");

        let Object::String(wide) = value_object("naïve") else {
            panic!("a string");
        };
        assert_eq!(&wide.bytes[..2], &[0xFE, 0xFF], "a byte-order mark");
    }

    #[test]
    fn a_value_over_maxlen_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 0,
            widgets: Vec::new(),
            options: Vec::new(),
            max_len: Some(3),
            default_appearance: None,
            scripts: form::FieldScripts::default(),
        };
        assert!(accepts(&field, "abc"));
        assert!(
            !accepts(&field, "abcd"),
            "silently truncating hides an error"
        );
    }

    #[test]
    fn a_read_only_field_is_refused() {
        let field = form::Field {
            reference: ObjRef::new(1, 0),
            name: "x".to_string(),
            kind: FieldKind::Text,
            value: form::FieldValue::None,
            default: form::FieldValue::None,
            flags: 1,
            widgets: Vec::new(),
            options: Vec::new(),
            scripts: form::FieldScripts::default(),
            max_len: None,
            default_appearance: None,
        };
        assert!(!accepts(&field, "anything"));
    }
}
