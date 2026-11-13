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

use std::collections::{BTreeMap, BTreeSet};

use crate::device::{Device, Glyph, GlyphQueries, MarkedProps};
use crate::plain::{PlainText, PlainTextOptions};
use crate::state::GraphicsState;
use crate::words::{reads_right_to_left, word_break, WordBreak};

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
    /// The innermost `/MCID` in scope when it was shown (14.7.4.2).
    ///
    /// **Innermost, not outermost.** Marked content nests, and 14.7.4.2 makes
    /// the enclosing sequence the one that owns the content: a `/Span` with
    /// its own `/MCID` inside a `/P` with another belongs to the span, and
    /// reporting the paragraph instead puts the span's words in the wrong
    /// place in reading order while leaving the page's text unchanged — a
    /// reordering that looks like a layout opinion rather than a bug.
    ///
    /// `None` outside every marked sequence, and also inside one whose
    /// property list carried no `/MCID` — an `/Artifact` scope, a bare `BMC`,
    /// a `/Span` that only sets `/Lang`. Those characters are not orphans;
    /// they are content no structure element ever claimed, and
    /// `Page::structured_text` counts the two apart.
    pub mcid: Option<u32>,
    /// Which content stream the sequence carrying [`TextChar::mcid`] was
    /// opened in: a form XObject's packed indirect reference, or `0` for the
    /// page's own stream.
    ///
    /// 14.7.4.2 numbers marked-content sequences within a content stream, so
    /// this is half of the identifier and not decoration: a page invoking two
    /// forms that each write `/MCID 0` has two sequences numbered 0, and a
    /// structure join keyed on the number alone gives every element both.
    ///
    /// Taken from the enclosing `BDC` rather than from where the glyph was
    /// drawn, which is the same distinction [`TextChar::mcid`] draws: a `BDC`
    /// in the page's stream whose scope encloses a `Do` owns the form's
    /// glyphs, and the sequence still resides in the page's stream.
    ///
    /// `0` whenever [`TextChar::mcid`] is `None`, where it means nothing.
    pub stream: u64,
    /// The name of the font it was shown in — `/BaseFont` as the dictionary
    /// writes it, subset tag included, since two subsets of one face are two
    /// fonts to a file — or `None` where the font has no name this build could
    /// read. See [`Glyph::font_name`] for why it travels with the character.
    pub font: Option<std::sync::Arc<str>>,
    /// The outermost `/ActualText` span open when it was shown (14.9.4): an
    /// index into [`TextPage::stated`], or `None` outside every span.
    ///
    /// **Outermost, not innermost**, the other way round from
    /// [`TextChar::mcid`]: a span's text replaces everything it encloses,
    /// the spans inside it too. A span is recorded whether its property list
    /// carries an `/MCID` or not, and the facade's `Page::text` puts its text
    /// where its glyphs stand.
    pub stated: Option<u32>,
    /// Whether an `/Artifact` scope (14.8.2.2) drew it: only ever `true` on a
    /// page read by [`TextDevice::keeping_artifacts`], since every other
    /// reading drops what such a scope draws.
    pub artifact: bool,
    /// Which glyph drew it: the glyph's place among every glyph the content
    /// showed [`TextDevice`] on the page, in the content stream's order,
    /// counted from 0 — the ones it dropped among them, so a place says
    /// where in the stream the glyph was — or `None` for a character no
    /// glyph draws, an `/ActualText`'s character placed on its span's box.
    ///
    /// The key a box option's boxes are put in by ([`HeldBoxes`]), once
    /// ruling 14 has read the page with the em box.
    pub glyph: Option<u32>,
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
    /// The characters, in the order [`TextDevice`] collected them — the order
    /// the content stream showed them.
    ///
    /// The facade's `Page::text` puts a line holding a right-to-left
    /// character into **logical** order before a caller sees it (ruling 14,
    /// `docs/rulings.md`), so for a page read through it this is reading
    /// order; `Page::text_with` keeps the stream's.
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
    ///
    /// [`TextDevice`] counts letters in the right-to-left scripts' blocks —
    /// Hebrew to Arabic Extended-A, the presentation forms and the
    /// supplementary planes' right-to-left ranges (`rtl_char`) — against
    /// alphabetic ones. The facade's `Page::text` replaces that
    /// count, for a line holding a right-to-left character, with the paragraph
    /// direction it read the line in (ruling 14), which comes from every
    /// character's `Bidi_Class` and from which end of the line its strong
    /// characters sit at.
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
///
/// Its blocks are read through [`TextPage::blocks`] and changed through
/// [`TextPage::blocks_mut`] alone, because a search keeps what it folded
/// them into ([`crate::search`]): `blocks_mut` drops every kept form before
/// it hands the blocks out, so a search after any change reads the blocks as
/// they are.
#[derive(Clone, Debug, Default)]
pub struct TextPage {
    /// The blocks, in reading order.
    blocks: Vec<TextBlock>,
    /// The 14.9 values each marked sequence's property list carried, keyed by
    /// the pair 14.7.4.2 identifies a sequence with: the content stream the
    /// `BDC` was written in — a form XObject's packed indirect reference, or
    /// `0` for the page's own — and the `/MCID`.
    ///
    /// Keyed on the pair and not on the `/MCID` alone for the reason
    /// [`TextChar::stream`] exists: two forms on one page may each number a
    /// sequence 0, and a map keyed on the number would give whichever drew
    /// first its `/ActualText` to both.
    ///
    /// 14.9 lets `/ActualText`, `/Alt`, `/Lang` and `/E` sit on a *property
    /// list* as well as on a structure element, and a producer that writes
    /// them there writes nothing about them in the structure tree. Without
    /// this the two placements would not be equivalent, and a page whose soft
    /// hyphens are removed by an `/ActualText` on the `BDC` would extract
    /// them anyway.
    ///
    /// A `BTreeMap` because ruling 4 makes iteration order a correctness
    /// question everywhere in this engine. **First writer wins**: 14.7.4.2
    /// makes an `/MCID` unique within its content stream, so a repeat is a
    /// malformation, and letting the later one overwrite would make what a
    /// page says depend on how far down its own damage sits.
    pub mcid_props: BTreeMap<(u64, u32), MarkedProps>,
    /// Each `/ActualText` span's text (14.9.4), by the index
    /// [`TextChar::stated`] names, in the order the spans opened.
    pub stated: Vec<String>,
    /// What extraction had to tolerate.
    ///
    /// Ruling 2 applies to text as much as to pixels: a page whose font could
    /// not be read still extracts what it can, and a caller comparing two
    /// extractions needs to know which one was guessing. Without this the
    /// difference between "this page has no text" and "this page has text this
    /// build could not decode" is invisible.
    pub warnings: Vec<TextWarning>,
    /// The page's text in each of the four folds a search reads it in, each
    /// built at the first search that asks for it and kept for every search
    /// after (`docs/design/performance.md`, P-24).
    pub(crate) forms: crate::search::Forms,
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
    /// A glyph's code had no `/ToUnicode` entry and no encoding that named it,
    /// so the glyph stands for no text this build could read (9.10.2).
    ///
    /// Said once for each such code a page shows, before the glyph is
    /// dropped, so a page whose text could not all be decoded does not read
    /// as a page with less text (ruling 2).
    UnmappedCode {
        /// The code, as the font's encoding reads it: a simple font's byte, a
        /// composite font's CMap code.
        code: u32,
    },
}

impl TextPage {
    /// A page of `blocks`, in reading order, with the property lists, the
    /// stated texts and the warnings [`TextDevice::finish`] would give it, and
    /// no form folded yet.
    #[must_use]
    pub fn new(
        blocks: Vec<TextBlock>,
        mcid_props: BTreeMap<(u64, u32), MarkedProps>,
        stated: Vec<String>,
        warnings: Vec<TextWarning>,
    ) -> TextPage {
        TextPage {
            blocks,
            mcid_props,
            stated,
            warnings,
            forms: crate::search::Forms::default(),
        }
    }

    /// The blocks, in reading order.
    #[must_use]
    pub fn blocks(&self) -> &[TextBlock] {
        &self.blocks
    }

    /// The blocks, to change: every form a search kept of them is dropped
    /// first, so the next search folds the blocks as they then are.
    pub fn blocks_mut(&mut self) -> &mut Vec<TextBlock> {
        self.forms.clear();
        &mut self.blocks
    }

    /// The blocks, taken out of the page.
    #[must_use]
    pub fn into_blocks(self) -> Vec<TextBlock> {
        self.blocks
    }

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

    /// The page's text, assembled as `options` asks, with a count of what
    /// assembling it changed.
    ///
    /// With [`PlainTextOptions::default`] the text is [`TextPage::plain_text`]
    /// to the byte and every count is zero. With
    /// [`PlainTextOptions::rejoin_hyphens`] soft hyphens are removed and words
    /// hyphenated across a line end are rejoined — the rule, and why the
    /// inferred joins are counted apart from the certain ones, is in
    /// [`crate::plain`].
    #[must_use]
    pub fn plain_text_with(&self, options: &PlainTextOptions) -> PlainText {
        crate::plain::assemble(
            self.blocks
                .iter()
                .flat_map(|b| b.lines.iter())
                .map(|l| l.text.as_str()),
            options,
        )
    }

    /// Finds `needle`, case-insensitively and literally.
    ///
    /// Matches within a line, returning one quad per match. Case folding is
    /// the simple ASCII-plus-Unicode-lowercase kind, which is what the engine
    /// being replaced did and what callers have calibrated against: both
    /// sides through `str::to_lowercase`, the page's a glyph at a time. The
    /// page's lower-cased text is folded at the first search that reads it
    /// and kept ([`crate::search`]), so a search after it folds its needle
    /// alone.
    #[must_use]
    pub fn search(&self, needle: &str) -> Vec<Quad> {
        self.find(needle, &crate::search::SearchOptions::default(), None)
    }
}

/// The quad covering glyphs `a..=b` of a line.
///
/// The left edge is taken from whichever of the two ends starts first along
/// the first one's baseline. For a line in the order it was drawn that is `a`,
/// and nothing changes; for a right-to-left line in logical order (ruling 14)
/// the logically first glyph is the rightmost, and a quad taken from it as
/// though it were the left end would be turned inside out.
pub(crate) fn span_quad(line: &TextLine, a: usize, b: usize) -> Option<Quad> {
    let mut first = line.chars.get(a)?;
    let mut last = line.chars.get(b)?;
    let (ux, uy) = (
        first.quad.lr.0 - first.quad.ll.0,
        first.quad.lr.1 - first.quad.ll.1,
    );
    let along = |p: (f64, f64)| p.0 * ux + p.1 * uy;
    if along(last.quad.lr) < along(first.quad.ll) {
        core::mem::swap(&mut first, &mut last);
    }
    Some(Quad {
        ul: first.quad.ul,
        ll: first.quad.ll,
        ur: last.quad.ur,
        lr: last.quad.lr,
    })
}

/// What a glyph whose code maps to no text becomes
/// (`docs/design/extraction-options.md`, "5. Code or glyph-id fallback").
///
/// Whatever the mode, the code is reported ([`TextWarning::UnmappedCode`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Unmapped {
    /// Dropped: the page's text is what its codes name, and nothing else.
    #[default]
    Drop,
    /// Kept as one character standing for its code ([`Glyph::code`]): U+F0000
    /// plus the code, in Supplementary Private Use Area-A, for a code up to
    /// 0xFFFD, and U+FFFD for a larger one.
    Code,
    /// Kept as one character standing for the glyph its font program selects
    /// for the code ([`Glyph::glyph_id`]): U+100000 plus the glyph index, in
    /// Supplementary Private Use Area-B, for an index up to 0xFFFD, and U+FFFD
    /// for a larger one and for a glyph whose font cannot say.
    Glyph,
}

/// The first code point of the area [`Unmapped::Code`] writes a code in.
pub const UNMAPPED_CODE_AREA: u32 = 0xF_0000;

/// The first code point of the area [`Unmapped::Glyph`] writes a glyph index
/// in.
pub const UNMAPPED_GLYPH_AREA: u32 = 0x10_0000;

/// The largest code or glyph index a private-use area holds: each area ends
/// at its plane's U+xFFFD, the planes' last two code points being
/// noncharacters.
const LARGEST_IN_AN_AREA: u32 = 0xFFFD;

impl Unmapped {
    /// The text a glyph of `code`, selecting `glyph_id`, stands for under
    /// this mode, where its code maps to none: `None` for [`Unmapped::Drop`].
    #[must_use]
    pub fn text(self, code: u32, glyph_id: Option<u16>) -> Option<char> {
        let in_area = |area: u32, value: u32| {
            (value <= LARGEST_IN_AN_AREA)
                .then(|| char::from_u32(area + value))
                .flatten()
                .unwrap_or('\u{FFFD}')
        };
        match self {
            Unmapped::Drop => None,
            Unmapped::Code => Some(in_area(UNMAPPED_CODE_AREA, code)),
            Unmapped::Glyph => Some(glyph_id.map_or('\u{FFFD}', |glyph| {
                in_area(UNMAPPED_GLYPH_AREA, u32::from(glyph))
            })),
        }
    }
}

/// Which box a character is reported with
/// (`docs/design/extraction-options.md`, options 6 and 7), in the glyph's own
/// space, where an em is one unit along each axis and the glyph's transform
/// takes it to the page.
///
/// Every decision extraction makes from geometry reads the em box whatever
/// this says — which glyphs are a line, which lines a block, and ruling 14's
/// reading — so a box option changes the quads a caller reads and nothing
/// else ([`HeldBoxes`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CharBoxes {
    /// The em box: the advance wide (at least a thousandth of an em), 0.2 of
    /// an em below the baseline to 0.8 above; a vertical glyph one em wide and
    /// its advance high.
    #[default]
    EmBox,
    /// Ascenders and side bearings: a horizontal glyph from where its
    /// outline's bounds start to where they end, or across its advance where
    /// it draws nothing, and from the font's descent to its ascent, or as the
    /// em box where the font states neither. A vertical glyph keeps the em
    /// box: ascent and descent are horizontal metrics.
    FontMetrics,
    /// The outline's bounds, where the glyph draws something; the
    /// [`CharBoxes::FontMetrics`] box, or a vertical glyph's em box, where it
    /// draws nothing.
    Outline,
}

impl CharBoxes {
    /// The quad `glyph` is reported with under this option, other than its
    /// em box: `w` and `h` the em box's width and height in glyph space, as
    /// [`TextDevice`] measures them. `None` under [`CharBoxes::EmBox`], and
    /// wherever the glyph's extent was not read — a Type 3 glyph — so the
    /// em box stands.
    fn quad(self, glyph: &Glyph, w: f64, h: f64) -> Option<Quad> {
        if self == CharBoxes::EmBox {
            return None;
        }
        let extent = glyph.extent.as_deref()?;
        let em = (0.0, -0.2 * h, w, 0.8 * h);
        let (x0, y0, x1, y1) = match (self, glyph.vertical, extent.ink) {
            (CharBoxes::Outline, _, Some(ink)) => ink,
            (_, true, _) => em,
            (_, false, ink) => {
                let (x0, x1) = ink.map_or((0.0, w), |(x0, _, x1, _)| (x0, x1));
                let (y0, y1) = extent
                    .ascent_descent
                    .map_or((-0.2, 0.8), |(ascent, descent)| (descent, ascent));
                (x0, y0, x1, y1)
            }
        };
        let t = &glyph.transform;
        Some(Quad {
            ll: t.apply(x0, y0),
            lr: t.apply(x1, y0),
            ul: t.apply(x0, y1),
            ur: t.apply(x1, y1),
        })
    }
}

/// How [`TextDevice`] reads a page (`docs/design/extraction-options.md`):
/// the extraction options the facade's `TextOptions` carries for this crate.
///
/// [`ExtractionOptions::default`] reads every page as it has always been read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExtractionOptions {
    /// What a glyph whose code maps to no text becomes.
    pub unmapped: Unmapped,
    /// Which box a character is reported with.
    pub boxes: CharBoxes,
    /// Whether a character whose text is several letters — a ligature its
    /// `/ToUnicode` spells out — or is a ligature presentation form is split
    /// into its letters, each with its share of the character's box
    /// ([`apply_character_options`]). `false` keeps ligatures, as the page
    /// states them.
    pub split_ligatures: bool,
    /// Whether every `White_Space` character of a character's text but
    /// U+0020 SPACE is written U+0020 ([`apply_character_options`]). `false`
    /// keeps white space as the page states it.
    pub normalize_whitespace: bool,
}

include!(concat!(env!("OUT_DIR"), "/ligatures.rs"));

/// The letters a ligature presentation form stands for
/// (`docs/design/extraction-options.md`, option 1): its tagged
/// decomposition's, from [`LIGATURE_FORMS`]; `None` for a character that is
/// no such form.
#[must_use]
pub fn ligature_letters(c: char) -> Option<&'static [char]> {
    LIGATURE_FORMS
        .binary_search_by_key(&c, |(form, _)| *form)
        .ok()
        .and_then(|at| LIGATURE_FORMS.get(at))
        .map(|(_, letters)| *letters)
}

/// `text` in its pieces: each character with every character after it whose
/// UAX #29 `Word_Break` is `Extend`, `Format` or `ZWJ`, which ride on what
/// precedes them ([`strong`]). A text that opens with such a character keeps
/// it in its first piece.
fn pieces(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    // Whether the piece being read holds a character that rides on nothing.
    let mut base = false;
    for (at, c) in text.char_indices() {
        let rides = matches!(
            word_break(c),
            WordBreak::Extend | WordBreak::Format | WordBreak::ZWJ
        );
        if !rides {
            if base {
                out.extend(text.get(start..at));
                start = at;
            }
            base = true;
        }
    }
    if start < text.len() {
        out.extend(text.get(start..));
    }
    out
}

/// A point `fraction` of the way from `a` to `b`.
fn between(a: (f64, f64), b: (f64, f64), fraction: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * fraction, a.1 + (b.1 - a.1) * fraction)
}

/// `c`, a character of a line of `wmode`, split into its letters where its
/// text is two pieces or more, or one ligature presentation form whose
/// letters are; `None` where it is one letter, kept as it is.
///
/// Each letter takes an equal share of the quad along the baseline — the
/// first at the quad's start where the letters hold no right-to-left
/// character, and at its end where they hold one, as each is drawn; on a
/// vertical line from the top down — and its origin is the character's moved
/// along the baseline by its share's offset from the quad's start. Every other
/// field is the character's.
fn split_ligature(c: &TextChar, wmode: WritingMode) -> Option<Vec<TextChar>> {
    let mut chars = c.text.chars();
    let form = match (chars.next(), chars.next()) {
        (Some(only), None) => {
            ligature_letters(only).map(|letters| letters.iter().collect::<String>())
        }
        _ => None,
    };
    let letters = form.as_deref().unwrap_or(&c.text);
    let pieces = pieces(letters);
    if pieces.len() < 2 {
        return None;
    }
    let n = pieces.len() as f64;
    let backwards = wmode == WritingMode::Horizontal && letters.chars().any(reads_right_to_left);
    let q = &c.quad;
    let mut out = Vec::with_capacity(pieces.len());
    for (k, piece) in pieces.iter().enumerate() {
        let share = if backwards { pieces.len() - 1 - k } else { k } as f64;
        let (from, to) = (share / n, (share + 1.0) / n);
        // The share's corners, and where it starts, against where the quad
        // does: along the baseline, or down the column.
        let (quad, start, quad_start) = match wmode {
            WritingMode::Horizontal => (
                Quad {
                    ll: between(q.ll, q.lr, from),
                    lr: between(q.ll, q.lr, to),
                    ul: between(q.ul, q.ur, from),
                    ur: between(q.ul, q.ur, to),
                },
                between(q.ll, q.lr, from),
                q.ll,
            ),
            WritingMode::Vertical => (
                Quad {
                    ul: between(q.ul, q.ll, from),
                    ur: between(q.ur, q.lr, from),
                    ll: between(q.ul, q.ll, to),
                    lr: between(q.ur, q.lr, to),
                },
                between(q.ul, q.ll, from),
                q.ul,
            ),
        };
        let offset = (start.0 - quad_start.0, start.1 - quad_start.1);
        out.push(TextChar {
            text: (*piece).to_owned(),
            quad,
            origin: (c.origin.0 + offset.0, c.origin.1 + offset.1),
            ..c.clone()
        });
    }
    Some(out)
}

/// Writes every `White_Space` character of `text` but U+0020 as U+0020, and
/// says whether it wrote any.
fn normalize_whitespace(text: &mut String) -> bool {
    if !text.chars().any(|c| c.is_whitespace() && c != ' ') {
        return false;
    }
    *text = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    true
}

/// The character options of `options` applied to every character of `page`
/// (`docs/design/extraction-options.md`, options 1 and 2): ligatures split
/// into their letters ([`ExtractionOptions::split_ligatures`]), then white
/// space normalised ([`ExtractionOptions::normalize_whitespace`]), each line's
/// text made again from its characters where any changed. Its boxes and its
/// order are the page's: a split character's letters tile its quad.
///
/// Run after ruling 14 has read the page and its boxes are in, so that the
/// characters an `/ActualText` states are split and normalised as the
/// glyphs' are, and a share is of the box the caller reads. With both off
/// the page is left as it is, to the bit.
pub fn apply_character_options(page: &mut TextPage, options: &ExtractionOptions) {
    if !options.split_ligatures && !options.normalize_whitespace {
        return;
    }
    for block in page.blocks_mut() {
        for line in &mut block.lines {
            let mut changed = false;
            if options.split_ligatures {
                let chars = std::mem::take(&mut line.chars);
                let mut out = Vec::with_capacity(chars.len());
                for c in chars {
                    match split_ligature(&c, line.wmode) {
                        Some(letters) => {
                            out.extend(letters);
                            changed = true;
                        }
                        None => out.push(c),
                    }
                }
                line.chars = out;
            }
            if options.normalize_whitespace {
                for c in &mut line.chars {
                    changed |= normalize_whitespace(&mut c.text);
                }
            }
            if changed {
                line.text = line.chars.iter().map(|c| c.text.as_str()).collect();
            }
        }
    }
}

/// The boxes a box option asked for ([`CharBoxes`]), held apart from a page
/// read with the em box until ruling 14 has read it
/// ([`TextDevice::finish_held`]): each glyph's, by its place
/// ([`TextChar::glyph`]), and the union of each `/ActualText` span's, by the
/// span ([`TextChar::stated`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeldBoxes {
    glyphs: BTreeMap<u32, Quad>,
    spans: BTreeMap<u32, Quad>,
}

impl HeldBoxes {
    /// Whether nothing is held: what [`CharBoxes::EmBox`] holds.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Holds `quad`, the box asked for of the glyph at `place`, under the
    /// outermost `/ActualText` span `stated` names where one is open.
    fn hold(&mut self, place: u32, stated: Option<u32>, quad: Quad) {
        self.glyphs.insert(place, quad);
        if let Some(span) = stated {
            self.spans
                .entry(span)
                .and_modify(|union| {
                    if let Some(both) = enclosing([*union, quad].into_iter()) {
                        *union = both;
                    }
                })
                .or_insert(quad);
        }
    }

    /// Puts the held boxes into `page`: each character a glyph drew takes its
    /// glyph's box, a character an `/ActualText` placed on its span's box
    /// rather than on a glyph takes the union of its span's boxes, and every
    /// line's and block's quad is made again from its characters', as
    /// [`TextDevice::finish`] makes them. Nothing is done where nothing is
    /// held, so a page read with the em box is left as it is, to the bit.
    pub fn apply(self, page: &mut TextPage) {
        if self.is_empty() {
            return;
        }
        for block in page.blocks_mut() {
            for line in &mut block.lines {
                for c in &mut line.chars {
                    let held = match (c.glyph, c.stated) {
                        (Some(place), _) => self.glyphs.get(&place),
                        (None, Some(span)) => self.spans.get(&span),
                        (None, None) => None,
                    };
                    if let Some(quad) = held {
                        c.quad = *quad;
                    }
                }
                if let Some(quad) = enclosing(line.chars.iter().map(|c| c.quad)) {
                    line.quad = quad;
                }
            }
            if let Some(quad) = enclosing(block.lines.iter().map(|l| l.quad)) {
                block.quad = quad;
            }
        }
    }
}

/// Collects glyphs into a [`TextPage`].
#[derive(Default)]
pub struct TextDevice {
    lines: Vec<PendingLine>,
    current: Option<PendingLine>,
    warnings: Vec<TextWarning>,
    /// How the page is read.
    options: ExtractionOptions,
    /// How many glyphs the device has been handed: the next one's place
    /// ([`TextChar::glyph`]).
    shown: u32,
    /// The boxes a box option asked for, by glyph, held until the page is
    /// read ([`TextDevice::finish_held`]).
    held: HeldBoxes,
    /// How many `/Artifact` scopes are open (14.8.2.2).
    ///
    /// A **counter and not a flag**, because marked content nests and an
    /// `/Artifact` inside an `/Artifact` closes one of them: a flag cleared by
    /// the inner `EMC` would put the rest of the outer artifact back into the
    /// text. The interpreter guarantees one `end_marked_content` per accepted
    /// begin, so this cannot go negative and cannot leak past a form XObject.
    artifacts: usize,
    /// Whether what an `/Artifact` scope draws is kept, marked
    /// [`TextChar::artifact`], rather than dropped
    /// ([`TextDevice::keeping_artifacts`]).
    keep_artifacts: bool,
    /// One entry per open scope, innermost last.
    ///
    /// `EMC` names no tag and carries no property list, so closing correctly
    /// needs the stack rather than a pair of counts — a `/Span` opened inside
    /// an `/Artifact` and closed first would otherwise decrement the artifact
    /// count and let the rest of the artifact through, and the same mistake
    /// with `/MCID` would attribute the rest of a paragraph to a span that
    /// had already ended.
    scopes: Vec<Scope>,
    /// The `/MCID`s in scope, innermost last (14.7.4.2).
    ///
    /// Separate from [`TextDevice::scopes`] rather than searched for in it:
    /// most scopes carry no `/MCID` at all, and the innermost one that does
    /// is wanted per glyph. A stack answers that in one lookup; scanning the
    /// scope stack backwards answers it in as many as the nesting is deep,
    /// on the hottest path in extraction.
    /// Paired with the stream each `BDC` was written in, because 14.7.4.2
    /// numbers sequences *within* a stream: `/MCID 0` in one form XObject and
    /// `/MCID 0` in another are two sequences, and the identifier alone
    /// cannot say which one a glyph belongs to.
    mcids: Vec<(u64, u32)>,
    /// The 14.9 values seen per sequence, first writer winning.
    mcid_props: BTreeMap<(u64, u32), MarkedProps>,
    /// The `/ActualText` spans open, outermost first, each an index into
    /// [`TextDevice::stated_texts`] (14.9.4). A glyph is held by the first.
    stated: Vec<u32>,
    /// Each span's text, in the order the spans opened.
    stated_texts: Vec<String>,
    /// The codes reported as [`TextWarning::UnmappedCode`], so that a glyph
    /// whose code was reported costs one lookup and not a walk of the
    /// warnings: a page of a font with no `/ToUnicode` shows such a code at
    /// every glyph.
    unmapped: BTreeSet<u32>,
}

/// How many warnings of one kind a page keeps: a page whose font is unknown
/// says so once rather than once a glyph, and a page of thousands of codes
/// nothing maps names the first of them and not every one. Each kind is
/// bounded apart, so a page that shows many unmapped codes still names every
/// font it could not resolve.
const MAX_WARNINGS_OF_A_KIND: usize = 64;

/// One open marked-content scope.
struct Scope {
    /// Whether its tag was `/Artifact` (14.8.2.2).
    artifact: bool,
    /// Whether it pushed onto [`TextDevice::mcids`], so `EMC` knows whether
    /// to pop. A scope with no `/MCID` must not pop its parent's.
    carried_mcid: bool,
    /// Whether it pushed onto [`TextDevice::stated`], for the same reason.
    carried_stated: bool,
}

/// How many `/ActualText` spans one page records (14.9.4).
///
/// An honest page states a span a line at most. The cap exists so that a
/// stream of nothing but `BDC` cannot turn a bounded page into unbounded
/// memory; past it a span is not recorded, and its glyphs read as drawn.
const MAX_STATED: usize = 1 << 16;

/// How many distinct `/MCID`s one page retains 14.9 values for.
///
/// 14.7.4.2 numbers marked sequences within a content stream, so an honest
/// page's count is its paragraph count. The cap is far above that and exists
/// only so a stream of nothing but `BDC` cannot turn a bounded page into
/// unbounded memory; past it the property lists are dropped and the
/// characters keep their `/MCID`, which loses alternate descriptions and
/// never loses text.
const MAX_MCID_PROPS: usize = 1 << 16;

struct PendingLine {
    chars: Vec<TextChar>,
    wmode: WritingMode,
    /// The baseline direction, used to decide whether a glyph continues the
    /// line or starts a new one.
    direction: (f64, f64),
    /// The axes the line is measured on ([`Frame`]): its first glyph's
    /// direction's, or, for the rest of a line ([`Self::rest`]), the frame of
    /// the line it is the rest of, so that [`mends`] compares the two lines'
    /// ends on one axis.
    frame: Frame,
    origin: (f64, f64),
    /// Where the last glyph's box ended along the line — the furthest place
    /// of its corners ([`Frame::along`]) — which is where the pen would be if
    /// the run continued.
    pen: f64,
    /// Where `Tc`, and after a word space `Tw`, left the pen, past
    /// [`Self::pen`], after the last glyph of a horizontal line that holds a
    /// word — see [`Stretch`]; `None` otherwise.
    stretched: Option<Stretch>,
    /// Whether the line holds a left-to-right letter, counting the word a cut
    /// on its baseline interrupted where the spacing put the next glyph
    /// ([`Stretch`]), which counts against a carry.
    holds_ltr: bool,
    /// Whether the line holds a right-to-left letter of its own — one of its
    /// glyphs, not of the word a cut interrupted ([`Stretch`]).
    holds_rtl: bool,
    /// The direction of the last word on the line — its glyphs since the
    /// white space before it, taken together ([`Strong::with`]) — counting
    /// the word a cut on its baseline interrupted where the spacing put the
    /// next glyph ([`Stretch`]); `None` before the line holds one, of
    /// anything but white space and marks with no letter before them.
    word: Option<Strong>,
    /// Whether the last word holds a right-to-left letter of the line's own,
    /// not only of the word a cut interrupted ([`Stretch`]).
    word_rtl: bool,
    /// Whether the last glyph was part of a word, not white space, so that
    /// the next one that is not continues that word.
    in_word: bool,
    /// Whether the line is **the rest of the one before**: started at a cut
    /// [`TextDevice::show_glyph`] made for the slack alone — not for a turn,
    /// another baseline or a jump back — after a word space that `Tw` moved
    /// the pen on past ([`Stretch::word_space`]), at a glyph that starts
    /// where the stretch left the pen and is not carried. As it ends it is
    /// appended to the line before where [`mends`] says.
    rest: bool,
    /// The leftmost and the rightmost of the line's own glyphs that read
    /// either way ([`Strong`]), kept as glyphs arrive; `None` before it
    /// holds one.
    ends: Option<(End, End)>,
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
    /// A device with nothing collected yet, reading as
    /// [`ExtractionOptions::default`] reads.
    #[must_use]
    pub fn new() -> TextDevice {
        TextDevice::default()
    }

    /// A device with nothing collected yet, reading as `options` asks.
    #[must_use]
    pub fn with_options(options: &ExtractionOptions) -> TextDevice {
        TextDevice {
            options: *options,
            ..TextDevice::default()
        }
    }

    /// A device that reads `/Artifact` scopes (14.8.2.2) as content, as an
    /// untagged reader sees them: each glyph one draws is kept and marked
    /// [`TextChar::artifact`], and a line is broken at the scope's edges as
    /// at a dropped scope's, so an artifact's text is lines of its own.
    #[must_use]
    pub fn keeping_artifacts() -> TextDevice {
        TextDevice {
            keep_artifacts: true,
            ..TextDevice::default()
        }
    }

    /// Records something extraction had to tolerate.
    ///
    /// Deduplicated: a page whose font is unknown says so once rather than
    /// once per glyph, which would bury the report under its own noise. At
    /// most [`MAX_WARNINGS_OF_A_KIND`] of each kind are kept, each kind
    /// bounded apart from the other, in the order first met.
    pub fn warn(&mut self, warning: TextWarning) {
        if let TextWarning::UnmappedCode { code } = warning {
            if self.unmapped.len() < MAX_WARNINGS_OF_A_KIND && self.unmapped.insert(code) {
                self.warnings.push(warning);
            }
            return;
        }
        let kind = core::mem::discriminant(&warning);
        let of_its_kind = self
            .warnings
            .iter()
            .filter(|kept| core::mem::discriminant(*kept) == kind)
            .count();
        if of_its_kind < MAX_WARNINGS_OF_A_KIND && !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// The page assembled from everything shown so far, its characters in
    /// the boxes [`ExtractionOptions::boxes`] asks for.
    #[must_use]
    pub fn finish(self) -> TextPage {
        let (mut page, held) = self.finish_held();
        held.apply(&mut page);
        page
    }

    /// The page assembled from everything shown so far, its characters in
    /// their em boxes, and the boxes a box option asked for held beside it
    /// ([`HeldBoxes::apply`] puts them in): for a caller that reads the page
    /// with the em box first — ruling 14's ordering — so that the boxes
    /// change no decision it makes.
    #[must_use]
    pub fn finish_held(mut self) -> (TextPage, HeldBoxes) {
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

        let page = TextPage::new(
            blocks,
            std::mem::take(&mut self.mcid_props),
            std::mem::take(&mut self.stated_texts),
            std::mem::take(&mut self.warnings),
        );
        (page, std::mem::take(&mut self.held))
    }

    /// Ends the line being collected: as the next starts, at an artifact's
    /// edge, or in [`TextDevice::finish`]. A line that is the rest of the
    /// one before ([`PendingLine::rest`]) is appended to it where the two
    /// read as one left-to-right line ([`mends`]).
    fn flush(&mut self) {
        if let Some(line) = self.current.take() {
            match self.lines.last_mut() {
                Some(before) if line.rest && mends(before, &line) => {
                    before.ends = End::joined(before.ends, line.ends);
                    before.holds_rtl |= line.holds_rtl;
                    before.holds_ltr |= line.holds_ltr;
                    before.chars.extend(line.chars);
                }
                _ => self.lines.push(line),
            }
        }
    }
}

/// Which way a glyph's text reads: right to left (a letter of `Bidi_Class`
/// `R` or `AL`, or the ARABIC COMMA — [`reads_right_to_left`]), left to right
/// (any other letter), or neither (digits — an Arabic-Indic one too, which is
/// `AN` — other punctuation, the right-to-left scripts' own among it but for
/// that comma, symbols).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strong {
    RightToLeft,
    LeftToRight,
    Neither,
}

impl Strong {
    /// The direction of a word holding glyphs of both directions: left to
    /// right if either is, else right to left if either is.
    fn with(self, other: Strong) -> Strong {
        match (self, other) {
            (Strong::LeftToRight, _) | (_, Strong::LeftToRight) => Strong::LeftToRight,
            (Strong::RightToLeft, _) | (_, Strong::RightToLeft) => Strong::RightToLeft,
            _ => Strong::Neither,
        }
    }
}

/// A glyph text's [`Strong`] direction: right to left if any character is,
/// else left to right if any is a letter — or `None` for a glyph of nothing
/// but combining marks and joiners (UAX #29's `Extend`, `Format` and `ZWJ`),
/// which rides on the glyph before it and takes its direction: `ố` drawn as an
/// `o` and two accents is a Latin word's end.
fn strong(text: &str) -> Option<Strong> {
    if text.chars().all(|c| {
        matches!(
            word_break(c),
            WordBreak::Extend | WordBreak::Format | WordBreak::ZWJ
        )
    }) {
        None
    } else if text.chars().any(reads_right_to_left) {
        Some(Strong::RightToLeft)
    } else if text.chars().any(char::is_alphabetic) {
        Some(Strong::LeftToRight)
    } else {
        Some(Strong::Neither)
    }
}

/// How far from where spacing left the pen the next glyph may start and still
/// be carried there ([`Stretch::carries`]), in ems: a rounding, not a gap.
const STRETCH_ROUNDING: f64 = 0.05;

/// **Where spacing left the pen** after a glyph — a word space stretched by
/// justification, or a letter followed by `letter-spacing`.
///
/// A line closed by an `ET` is resumed only where the last glyph's box
/// ended, give or take half an em. A glyph's box ends where its own width
/// does, and 9.4.4 moves the pen on past it by `Tc`, and past a word space by
/// `Tw` as well: resumed from the box alone, a justified line set as one text
/// object per font — its words in the overflow font and its spaces in a
/// standard one, as this engine's EPUB writer sets a book's Arabic — would be
/// cut at every word space justification stretches past half an em. Cut so,
/// a right-to-left line drawn in visual order is read a word at a time in the
/// order the words were drawn, left to right: backwards (a narrowed
/// right-to-left paragraph justified at `margin: 0 35%`). And a line
/// letter-spaced by half an em would be cut where one text object ends and
/// the next begins, at a gap that is the spacing and nothing else:
/// `مُدَّة مُعلِّم.` in monospace at `letter-spacing: 8px`, its full stop
/// drawn left of its word space as UAX #9 draws it, would be cut after the
/// stop and read `مُدَّة مُعلِّم .`.
///
/// **So within right-to-left text spacing carries the pen**
/// ([`Stretch::carries`]), and nowhere else. Every piece a carry joins is one
/// the device would otherwise cut apart, and read apart each reads right: a
/// carry is worth taking only where the line it makes is right-to-left text
/// that ruling 14 reads as such, and every other carry reads wrong a line
/// whose pieces read right apart:
///
/// - carried everywhere, the stretch joins a right-to-left word to a longer
///   Latin run (`hello world שלום`), read by its majority as a left-to-right
///   line, and `The word` to a longer Arabic phrase, read as a right-to-left
///   one;
/// - carried after a right-to-left word into a neutral, the pen goes on into
///   a Latin word the neutral opens — `שָׁלוֹם עוֹלָם <b>(HDAC)</b>.`,
///   `⟨HDAC⟩`, `( עוֹלָם` after `(x) and` — and the line so joined falls into
///   ruling 14's limits;
/// - carried after a word of neutrals, `- ` in `שָׁלוֹם - (HDAC) - עוֹלָם`
///   with no `dir`, it joins the Hebrew after it to a line that holds the
///   Latin, read right to left as its two Hebrew ends say;
/// - carried a little further than the spacing reaches, it crosses from
///   one table cell to the next: `td { letter-spacing: 0.5em }` with
///   `border-spacing: 2px` reads two Hebrew cells as one word, `גדולספר`;
/// - carried after neutrals on a line that holds no left-to-right letter, it
///   joins a neutral the device cut from the Latin text it belongs to, and
///   that starts a line afresh, to the right-to-left word after it — an
///   opening `«`, `"`, `(`, `#` or `...` in a text object of its own, `<i>?!</i>`
///   and its stretched space before `שָׁלוֹם`, a table cell's `[` — and the
///   line reads its mark at the wrong end: `«שָׁלוֹם עוֹלָם»` letter-spaced as
///   `שָׁלוֹם עוֹלָם«`, `<td>[שָׁלוֹם]</td>` as `שָׁלוֹם[`;
/// - carried into an Arabic-Indic digit, which the block table counts as
///   right to left, it reads `(١٢٣)` after a Hebrew word as `٢٣ (١)`, and a
///   search for `١٢٣` finds nothing;
/// - and carried out of or into an ORNATE PARENTHESIS, which is `ON` like
///   `(` but which the table takes in with the Arabic block's punctuation, a
///   `﴿` in a text object of its own goes on into the word after it,
///   `﴿عوامل عدّة﴾` letter-spaced reading `﴾عوامل عدّة﴿`; and the ARABIC
///   PERCENT SIGN, `ET`, so taken in, joins a justified line whole that
///   then reads the sign at its far end.
///
/// So the pen is carried only into a glyph that reads right to left by its
/// own class ([`reads_right_to_left`]), only as far as the spacing reaches,
/// and only after a right-to-left word or, after neutrals, on a line that
/// holds a right-to-left letter **of its own** and no left-to-right one: a
/// line the device starts at a cut holds nothing from before the cut but the
/// word the cut interrupted where the spacing put the next glyph, which
/// counts against a carry and never for one.
/// A left-to-right page is collected exactly as if spacing carried nothing,
/// and so is every line that holds a left-to-right letter but after a
/// right-to-left word on it: the two words of a Hebrew gloss in an English
/// sentence, `peace (עוֹלָם מִלְחָמָה)`, justified, are one piece of
/// right-to-left text as they are in a right-to-left paragraph. A run of
/// neutrals the EPUB writer draws at the start of a right-to-left run, the
/// ` (` of a sans-serif book's Helvetica, is a text object no right-to-left
/// letter precedes, so the writer draws its word space first, where the
/// stretch stays inside it (`paint`'s `opens_with_its_word_space`).
///
/// **A word cut on one baseline goes on into the next piece where the spacing
/// put it.** A word is all of its glyphs since the white space before it; a
/// line the device cut inside a word, at a gap the spacing accounts for
/// ([`Stretch::reaches`]), holds that word's left-to-right letter, if it has
/// one, from its first glyph: `[ŋ̊ə̃] חו`, letter-spaced by half an em and cut
/// before its `]`, does not carry the pen from the `] ` into the Hebrew word.
/// A gap no spacing accounts for — a table cell's, a column's — ends the word
/// as white space does: two cells of a right-to-left table row at
/// `word-spacing: 1em`, `<td>def</td><td>أصبحنا نعرف</td>`, have no white
/// space between them, and the Arabic cell's first drawn glyph starts 4.7 em
/// right of `def`'s end. Taken on into it, `def`'s left-to-right word would
/// take in the Arabic letters drawn after it, the stretched space after them
/// would carry nothing, and the cell would read in two, cut at that space:
/// ruling 14 rejoins two pieces only where they meet end to end.
///
/// **A cut at a word space's stretch is mended once the piece after it
/// ends** (`docs/design/rtl-read-back.md`, "A word space styled apart
/// across a two-em stretch"). Where the carry refuses a gap that a word
/// space's `Tw` accounts for, the line the next glyph starts is the
/// rest of the one before ([`PendingLine::rest`]), and as it ends it is
/// appended to it where the two hold a right-to-left letter of their own and
/// the leftmost and the rightmost of their glyphs that read either way both
/// read left to right ([`mends`]): `αλφα <b>βήτα</b> ספר בית xyz.` at
/// `word-spacing: 2em` with no `dir`, cut at `β` and at the gloss's first
/// drawn glyph, reads `αλφα ` and `βήτα ספר בית xyz.`. The mend waits for
/// the line's far end, which the gap does not show: taken at the gap,
/// `αλφα βήτα ספר בית` is a line with a Hebrew right end, which ruling 14
/// reads by its majority.
#[derive(Clone, Copy, Debug)]
struct Stretch {
    /// Where the pen stood after the glyph, along the line ([`Frame::along`]).
    pen: f64,
    /// Whether the glyph was a word space that `Tw` moved the pen on past:
    /// a code 32 that decodes to white space, under a `Tw` above zero.
    word_space: bool,
}

impl Stretch {
    /// Whether a glyph of direction `next`, drawn at `origin` along the
    /// baseline by a font of `size`, continues `line` from the stretched pen.
    ///
    /// Three things, each of which a carry once crossed (see [`Stretch`]):
    /// the word before the gap reads right to left by a letter of the line's
    /// own, or, after neutrals, the line holds a right-to-left letter of its
    /// own and no left-to-right one; the glyph after the gap is itself right
    /// to left — not a neutral, which says nothing of the word it opens; and
    /// it starts where the spacing left the pen ([`Stretch::reaches`]), so
    /// that a gap the spacing does not account for, a cell's or a column's,
    /// is never carried across. A word a cut on the baseline interrupted, at
    /// a gap the spacing accounts for, counts against a carry — a
    /// left-to-right letter of it is the line's — and never for one; at any
    /// other gap it ended there, and counts for nothing.
    fn carries(self, line: &PendingLine, next: Option<Strong>, origin: f64, size: f64) -> bool {
        let after_its_word = line.word == Some(Strong::RightToLeft) && line.word_rtl;
        let after_neutrals = line.holds_rtl && !line.holds_ltr;
        (after_its_word || after_neutrals)
            && next == Some(Strong::RightToLeft)
            && self.reaches(origin, size)
    }

    /// Whether a glyph drawn at `origin` along the baseline by a font of
    /// `size` starts where the spacing left the pen, give or take
    /// [`STRETCH_ROUNDING`] of an em: a gap the spacing accounts for, and
    /// no other — not a cell's, a column's or a margin's.
    fn reaches(self, origin: f64, size: f64) -> bool {
        (origin - self.pen).abs() <= size.max(1.0) * STRETCH_ROUNDING
    }
}

/// A glyph of a line that reads either way ([`Strong`]), and where its box
/// starts along the line: the nearest place of its corners in the line's
/// [`Frame`], which the rest of a line shares with the line it is the rest
/// of.
#[derive(Clone, Copy, Debug)]
struct End {
    at: f64,
    strong: Strong,
}

impl End {
    /// The leftmost and the rightmost of two sets of ends, `b`'s glyphs
    /// collected after `a`'s. At a tie the earlier glyph is the left one and
    /// the later the right, as `text_order`'s stable sort by where each base
    /// starts puts them.
    fn joined(a: Option<(End, End)>, b: Option<(End, End)>) -> Option<(End, End)> {
        match (a, b) {
            (Some((a_left, a_right)), Some((b_left, b_right))) => Some((
                if b_left.at < a_left.at {
                    b_left
                } else {
                    a_left
                },
                if b_right.at >= a_right.at {
                    b_right
                } else {
                    a_right
                },
            )),
            (ends, None) | (None, ends) => ends,
        }
    }
}

/// Whether `rest`, the rest of `before` ([`PendingLine::rest`]), is appended
/// to it as it ends: where the two hold a right-to-left letter of their own
/// ([`PendingLine::holds_rtl`]) and the leftmost and the rightmost of their
/// glyphs that read either way both read left to right, so that ruling 14
/// reads the line they make by its ends, left to right. Left-to-right text
/// is never joined, nor a line with a right-to-left glyph at either end,
/// which ruling 14 reads by its majority or from the right: `The word` before
/// a longer Arabic phrase, `שלום abcdef <b>עולם</b>` with no `dir`.
fn mends(before: &PendingLine, rest: &PendingLine) -> bool {
    (before.holds_rtl || rest.holds_rtl)
        && End::joined(before.ends, rest.ends).is_some_and(|(left, right)| {
            left.strong == Strong::LeftToRight && right.strong == Strong::LeftToRight
        })
}

/// **The axes a horizontal line is measured on**
/// (`docs/design/rtl-read-back.md`, "A line under a half turn"): `u`, the
/// line's direction over its length, and `n`, `u` turned a quarter
/// anticlockwise. A point's place along the line is its dot product with
/// `u`, and across it its dot product with `n`.
///
/// Measured on the page's axes, a line under a half turn runs to the page's
/// left: its pen would stand where its last glyph starts, no [`Stretch`]
/// would be kept, a text object's end would part a glyph gone back along the
/// line past half an em and none gone on, and `backwards` would cut one gone
/// on past three ems and none gone back — `25٪ <b>W</b> ١٢٣ … 2010،` in an
/// Arabic paragraph turned by `rotate(180deg)` at `word-spacing: 1em` would
/// read `25` alone. Measured in its own frame, it is continued and cut as it
/// is unturned.
///
/// A line whose direction is `(a, 0)` with `a > 0` — every unturned line and
/// every one under a positive scale — takes the page's `x` and `y`
/// themselves, not a dot product, so that its lines, characters and order are
/// those the page's axes give, to the bit; so does a vertical line, and one
/// whose direction has no finite, non-zero length.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Frame {
    /// The page's own `x` and `y`.
    Page,
    /// `u`, a unit vector along the line.
    Along((f64, f64)),
}

impl Frame {
    /// The frame of a line of `wmode` whose first glyph runs along
    /// `direction`, the first column of its transform. Its length is the
    /// square root of the sum of squares, which IEEE 754 rounds correctly and
    /// so alike on every target (ruling 4), where `hypot` is rounded as each
    /// platform's library rounds it; a direction too long or too short for
    /// its squares to be finite and above zero takes the page's axes.
    fn of(wmode: WritingMode, (a, b): (f64, f64)) -> Frame {
        if wmode == WritingMode::Vertical || (a > 0.0 && b == 0.0) {
            return Frame::Page;
        }
        let length = (a * a + b * b).sqrt();
        if length.is_finite() && length > 0.0 {
            Frame::Along((a / length, b / length))
        } else {
            Frame::Page
        }
    }

    /// A point's place along the line.
    fn along(self, (x, y): (f64, f64)) -> f64 {
        match self {
            Frame::Page => x,
            Frame::Along((ux, uy)) => x * ux + y * uy,
        }
    }

    /// A point's place across the line, along `n`.
    fn across(self, (x, y): (f64, f64)) -> f64 {
        match self {
            Frame::Page => y,
            Frame::Along((ux, uy)) => y * ux - x * uy,
        }
    }

    /// Where a glyph's box starts and ends along the line: the nearest and
    /// the furthest places of its corners. `bounds` is the quad's
    /// [`Quad::bounds`], which the page's axes read as they always have.
    fn extent(self, quad: &Quad, (x0, _, x1, _): (f64, f64, f64, f64)) -> (f64, f64) {
        match self {
            Frame::Page => (x0, x1.max(x0)),
            Frame::Along(_) => [quad.ll, quad.lr, quad.ul, quad.ur].into_iter().fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(lo, hi), corner| {
                    let at = self.along(corner);
                    (lo.min(at), hi.max(at))
                },
            ),
        }
    }
}

/// Whether a character is in one of the right-to-left scripts' blocks — the
/// ranges `DerivedBidiClass.txt` gives `Bidi_Class` `R` or `AL` by default:
/// Hebrew to Arabic Extended-A, whole (U+0590 to U+08FF: Syriac, the Arabic
/// Supplement, Thaana, N'Ko, Samaritan, Mandaic and the Syriac Supplement and
/// Arabic Extended-B among them), the Hebrew and Arabic presentation forms,
/// and the supplementary planes' right-to-left blocks (U+10800 to U+10FFF,
/// Arabic Extended-C and Sogdian among them, and U+1E800 to U+1EFFF, Adlam's).
///
/// By block, for [`is_rtl`]'s count of a line's direction alone, in which a
/// character of another class inside these blocks — an Arabic-Indic digit, a
/// mark, a separator — is counted in with the script it belongs to. Whether
/// spacing carries the pen into a glyph is asked of its own class instead
/// ([`reads_right_to_left`]): carried into an Arabic-Indic digit, which is
/// `AN`, a Hebrew paragraph's `(١٢٣)` would read `٢٣ (١)`.
fn rtl_char(c: char) -> bool {
    let code = u32::from(c);
    (0x0590..=0x08FF).contains(&code)
        || (0xFB1D..=0xFDFF).contains(&code)
        || (0xFE70..=0xFEFF).contains(&code)
        || (0x1_0800..=0x1_0FFF).contains(&code)
        || (0x1_E800..=0x1_EFFF).contains(&code)
}

/// Whether a run of text is predominantly right-to-left.
///
/// Counts strong directional characters: those of the right-to-left
/// scripts' blocks ([`rtl_char`]) against everything with a left-to-right
/// property. Deliberately not a full bidi implementation — reordering is a
/// caller's concern, and reporting the dominant direction is what a text
/// layer needs.
fn is_rtl(text: &str) -> bool {
    let mut rtl = 0usize;
    let mut ltr = 0usize;
    for c in text.chars() {
        if rtl_char(c) {
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
    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        _visible: bool,
        _hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        let artifact = tag == b"Artifact";
        // 14.7.4.2's `/MCID` is recorded even for an artifact scope. A
        // producer that tags an artifact is writing something malformed, and
        // dropping the identifier here would make it an *orphan* in the
        // structured view — a character the structure tree claims and
        // extraction cannot find — rather than what it is, which is content
        // 14.8.2.2 excludes. The glyphs are still dropped; only the
        // bookkeeping is honest about why.
        let key = props.and_then(|p| p.mcid.map(|mcid| (p.stream, mcid)));
        if let Some(key) = key {
            self.mcids.push(key);
            if let Some(props) = props.filter(|p| p.mcid.is_some()) {
                if self.mcid_props.len() < MAX_MCID_PROPS {
                    self.mcid_props.entry(key).or_insert_with(|| props.clone());
                }
            }
        }
        // 14.9.4: a span's `/ActualText` replaces what it encloses, with an
        // `/MCID` on the same list or without one.
        let stated = props
            .and_then(|p| p.actual_text.as_ref())
            .filter(|_| self.stated_texts.len() < MAX_STATED);
        if let Some(text) = stated {
            let at = u32::try_from(self.stated_texts.len()).unwrap_or(u32::MAX);
            self.stated_texts.push(text.clone());
            self.stated.push(at);
        }
        self.scopes.push(Scope {
            artifact,
            carried_mcid: key.is_some(),
            carried_stated: stated.is_some(),
        });
        if artifact {
            // A line may not straddle the boundary: the artifact's glyphs are
            // dropped, and the ones before it must not be joined to the ones
            // after as though nothing had been between them; or they are kept,
            // and are lines of their own.
            self.flush();
            self.artifacts += 1;
        }
    }

    fn end_marked_content(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        if scope.carried_mcid {
            self.mcids.pop();
        }
        if scope.carried_stated {
            self.stated.pop();
        }
        if scope.artifact {
            self.flush();
            self.artifacts = self.artifacts.saturating_sub(1);
        }
    }

    /// 9.6.4: a Type 3 glyph stands for its text as any glyph does, though
    /// what it draws is its procedure's; it is read where it is placed, as a
    /// shown glyph is, and a code that maps to nothing adds nothing and is
    /// reported, as a shown glyph's is.
    fn show_type3_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.show_glyph(glyph, state);
    }

    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        // Every glyph handed counts toward the next one's place, the ones
        // dropped among them ([`TextChar::glyph`]).
        let ordinal = self.shown;
        self.shown = self.shown.saturating_add(1);
        if self.artifacts > 0 && !self.keep_artifacts {
            return;
        }
        // 9.10.2: a code no `/ToUnicode` entry and no encoding names stands
        // for no text this build can read. It is said, once a code, before
        // the glyph is dropped: a page with text that could not be decoded
        // must not read as a page with less text (ruling 2). A caller who
        // asked for a fallback has the glyph kept, standing for its code or
        // its glyph ([`Unmapped`]).
        let fallback: String;
        let text: &str = if glyph.text.is_empty() {
            self.warn(TextWarning::UnmappedCode { code: glyph.code });
            let Some(stands_for) = self.options.unmapped.text(glyph.code, glyph.glyph_id) else {
                return;
            };
            fallback = stands_for.to_string();
            &fallback
        } else {
            &glyph.text
        };
        if !glyph.transform.is_finite() {
            return;
        }

        // The glyph's em box: the advance wide, 0.8 of an em above the
        // baseline and 0.2 below, which is close enough for a selection
        // rectangle and needs no font program to compute. Every decision the
        // device makes reads it, under every box option
        // (`docs/design/extraction-options.md`, "The em box").
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
        // The box the caller asked for, held apart until ruling 14 has read
        // the page with the em box ([`HeldBoxes`]).
        let asked = self.options.boxes.quad(glyph, w, h).filter(Quad::is_finite);

        let origin = (t.e, t.f);
        // Which line this glyph belongs to is asked at the **baseline**: 9.4.3's
        // `Ts` displaces the glyph and leaves the pen where it was, so a
        // superscript marker, a subscript and a `GPOS` mark raised onto its base
        // are all on the line they interrupt. Without this a base-mark-base
        // sequence splits the line twice — once where the rise starts and once
        // where it returns to zero, because `line.origin` below is the previous
        // glyph's.
        //
        // The rule considered and **rejected** is *"a zero-advance glyph never
        // starts a line"*. It is not equivalent and it is not safe: a producer
        // that writes every `/W` as zero and positions with `TJ` is in the
        // corpus, and that rule would collapse such a page into one line. The
        // baseline rule is **monotone** — it changes nothing when the rise is
        // zero, so it can only join lines a `Ts` splits today and can never
        // split a line that is joined.
        //
        // `quad` and [`TextChar::origin`] keep the risen position, which is
        // where the ink is, so nothing a caller reports moves.
        let anchor = glyph.baseline.unwrap_or(origin);
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

        let (starts_new_line, word, rest) = match &self.current {
            None => (true, None, false),
            Some(line) => {
                let same_font = line.font_id == glyph.font_id;
                // A glyph continues the line when it runs the same way and sits
                // near where the previous one ended — measured horizontally in
                // the line's own frame ([`Frame`]): along it and across it.
                let frame = line.frame;
                let turned =
                    (line.direction.0 * direction.1 - line.direction.1 * direction.0).abs() > 0.01;
                let expected_gap = size.max(1.0) * 3.0;
                let far = match wmode {
                    WritingMode::Horizontal => {
                        (frame.across(anchor) - frame.across(line.origin)).abs()
                            > size.max(1.0) * 0.5
                    }
                    WritingMode::Vertical => (anchor.0 - line.origin.0).abs() > size.max(1.0) * 0.5,
                };
                let backwards = match wmode {
                    WritingMode::Horizontal => {
                        frame.along(anchor) + expected_gap < frame.along(line.origin)
                    }
                    WritingMode::Vertical => anchor.1 - expected_gap > line.origin.1,
                };
                // A line that an `ET` closed is resumed only where the last one
                // stopped. Half an em of slack, which is a space and is not a
                // column: two cells of a table row sit on one baseline and are
                // two lines, and a paragraph that changed font mid-word is one.
                let slack = size.max(1.0) * 0.5;
                let along = frame.along(origin);
                let carried = wmode == WritingMode::Horizontal
                    && line
                        .stretched
                        .is_some_and(|stretch| stretch.carries(line, strong(text), along, size));
                let disjoint = line.closed
                    && !carried
                    && match wmode {
                        WritingMode::Horizontal => along > line.pen + slack,
                        WritingMode::Vertical => origin.1 < line.pen - slack,
                    };
                let elsewhere = turned || far || backwards || (line.wmode != wmode && !same_font);
                // A line cut only where the pen moved on along the same
                // baseline goes on with the word its last glyph was in, unless
                // white space ended it or a gap no spacing accounts for did:
                // `[ŋ̊ə̃]` letter-spaced by half an em and cut before its `]`
                // is still one left-to-right word, and the `]` begins a line
                // that holds it, but a table cell that starts past where the
                // spacing left the pen begins a word of its own ([`Stretch`]).
                let spaced = wmode == WritingMode::Horizontal
                    && line
                        .stretched
                        .is_some_and(|stretch| stretch.reaches(along, size));
                let word = if elsewhere || !line.in_word || (disjoint && !spaced) {
                    None
                } else {
                    line.word
                };
                // A cut for the slack alone, after a word space `Tw` moved
                // the pen on past, at a glyph that starts where the stretch
                // left the pen and is not carried: the line it starts is the
                // rest of this one ([`PendingLine::rest`]).
                let rest = !elsewhere
                    && disjoint
                    && spaced
                    && line.stretched.is_some_and(|stretch| stretch.word_space);
                (elsewhere || disjoint, word, rest)
            }
        };

        if starts_new_line {
            // The rest of a line is measured in the frame of the line it is
            // the rest of ([`PendingLine::frame`]).
            let frame = match &self.current {
                Some(before) if rest => before.frame,
                _ => Frame::of(wmode, direction),
            };
            self.flush();
            self.current = Some(PendingLine {
                chars: Vec::new(),
                wmode,
                direction,
                frame,
                origin: anchor,
                pen: match wmode {
                    WritingMode::Horizontal => frame.along(anchor),
                    WritingMode::Vertical => anchor.1,
                },
                font_id: glyph.font_id,
                closed: false,
                stretched: None,
                holds_ltr: word == Some(Strong::LeftToRight),
                holds_rtl: false,
                word,
                word_rtl: false,
                in_word: word.is_some(),
                rest,
                ends: None,
            });
        }

        if let Some(line) = &mut self.current {
            line.origin = anchor;
            line.font_id = glyph.font_id;
            line.closed = false;
            let bounds = quad.bounds();
            let (nearest, furthest) = line.frame.extent(&quad, bounds);
            line.pen = match wmode {
                WritingMode::Horizontal => furthest,
                WritingMode::Vertical => bounds.1.min(bounds.3),
            };
            let blank = text.chars().all(char::is_whitespace);
            // The word this glyph is in, taken whole: `(HDAC).` holds a
            // left-to-right letter whatever its last glyph. A space ends it; a
            // mark or a joiner belongs to the word before it.
            if blank {
                line.in_word = false;
            } else if let Some(own) = strong(text) {
                let rtl = own == Strong::RightToLeft;
                (line.word, line.word_rtl) = match line.word {
                    Some(word) if line.in_word => (Some(word.with(own)), line.word_rtl || rtl),
                    _ => (Some(own), rtl),
                };
                line.in_word = true;
                line.holds_ltr |= own == Strong::LeftToRight;
                line.holds_rtl |= rtl;
                if own != Strong::Neither {
                    let end = End {
                        at: nearest,
                        strong: own,
                    };
                    line.ends = End::joined(line.ends, Some((end, end)));
                }
            }
            line.stretched = match wmode {
                // 9.4.4: `tx = (w0 × Tfs + Tc + Tw) × Th`, which is
                // `w0 + (Tc + Tw) / Tfs` in glyph space; `Tw` only after a
                // code 32 that decodes to white space — not after a letter a
                // font draws at code 32, after which the interpreter moves
                // the pen on by `Tw` all the same — and only horizontally. As
                // the interpreter moves the pen otherwise:
                // [`crate::FontSource::decode`] does not say how many bytes a
                // code took, so a two-byte code 32 of a Type 0 font is moved
                // by `Tw` there too, which 9.3.3 applies to a single-byte code
                // alone — and the pen is carried to where the interpreter drew
                // the next glyph.
                WritingMode::Horizontal if line.word.is_some() => {
                    let tw = if glyph.code == 32 && blank {
                        state.text.word_spacing
                    } else {
                        0.0
                    };
                    let spacing = (state.text.char_spacing + tw) / glyph.size;
                    let at = line.frame.along(corner(advance + spacing, 0.0));
                    (at.is_finite() && at > line.pen).then_some(Stretch {
                        pen: at,
                        word_space: tw > 0.0,
                    })
                }
                _ => None,
            };
            let stated = self.stated.first().copied();
            if let Some(asked) = asked {
                self.held.hold(ordinal, stated, asked);
            }
            line.chars.push(TextChar {
                text: text.to_owned(),
                quad,
                size,
                origin,
                mcid: self.mcids.last().map(|(_, mcid)| *mcid),
                stream: self.mcids.last().map_or(0, |(stream, _)| *stream),
                font: glyph.font_name.clone(),
                stated,
                artifact: self.artifacts > 0,
                glyph: Some(ordinal),
            });
        }
    }

    /// The glyph a code with no text selects, where a fallback stands for
    /// it ([`Unmapped::Glyph`]), and each glyph's extent, where a box option
    /// builds a box from it ([`CharBoxes`]); nothing otherwise, so the
    /// default reading parses no font program.
    fn glyph_queries(&self) -> GlyphQueries {
        GlyphQueries {
            unmapped_glyph_ids: self.options.unmapped == Unmapped::Glyph,
            extents: self.options.boxes != CharBoxes::EmBox,
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
    use crate::device::GlyphExtent;
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
            baseline: None,
            advance: size * 0.5,
            size,
            vertical: false,
            font_id: 1,
            font_name: None,
            glyph_id: None,
            extent: None,
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
        d.begin_marked_content(b"Artifact", true, None, None);
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
        d.begin_marked_content(b"Artifact", true, None, None);
        d.show_glyph(&glyph("X", 0.0, 700.0, 10.0), &state);
        d.begin_marked_content(b"Span", true, None, None);
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
        d.begin_marked_content(b"Span", true, None, None);
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

    /// Each of `pieces` shown as a text object of its own — a word, then a
    /// word space, then the next word, as one text object per font draws a
    /// justified line — under `Tw` of `word_spacing`, and the page's lines.
    fn justified(pieces: &[(&str, f64)], word_spacing: f64) -> Vec<String> {
        spaced(pieces, 0.0, word_spacing)
    }

    /// [`justified`], under `Tc` of `char_spacing` as well.
    fn spaced(pieces: &[(&str, f64)], char_spacing: f64, word_spacing: f64) -> Vec<String> {
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.char_spacing = char_spacing;
        state.text.word_spacing = word_spacing;
        for (text, x) in pieces {
            device.show_glyph(&glyph(text, *x, 700.0, 10.0), &state);
            device.end_text();
        }
        device
            .finish()
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect()
    }

    /// **A justified word space carries the pen within right-to-left text.**
    /// Two Hebrew words, each a text object, with a space between them in a
    /// third: the space's box is half an em wide, and `Tw` of eight points
    /// moves the pen on to where the next word is written, 0.8 em past the
    /// box — more than the half em a line closed by `ET` is resumed within.
    /// Read from the box, the line would be cut there, and a right-to-left
    /// line cut so is read a word at a time in the order the words were
    /// drawn: backwards.
    #[test]
    fn a_justified_word_space_carries_the_pen_within_right_to_left_text() {
        let pieces = [("\u{5D0}", 0.0), (" ", 5.0), ("\u{5D1}", 18.0)];
        assert_eq!(justified(&pieces, 8.0), ["\u{5D0} \u{5D1}"]);
        // With no `Tw` the same gap is a gap, and cuts the line as before.
        assert_eq!(justified(&pieces, 0.0), ["\u{5D0} ", "\u{5D1}"]);
        // And a neutral before the space carries it too, on a line that holds
        // a right-to-left letter of its own: the `(` that opens a bracketed
        // Arabic phrase, in a face of its own.
        let neutral: &[(&str, f64)] = &[("\u{5D0}", 0.0), (" ", 5.0), ("(", 18.0), (" ", 23.0)];
        let hebrew: &[(&str, f64)] = &[("\u{5D1}", 36.0)];
        assert_eq!(
            objects_spaced(&[neutral, hebrew], 0.0, 8.0),
            ["\u{5D0} ( \u{5D1}"]
        );
        // Every letter of `Bidi_Class` `R` or `AL` reads right to left: the
        // Arabic Supplement's letters, N'Ko's and Adlam's among them.
        for (first, second) in [
            ("\u{750}", "\u{751}"),
            ("\u{7CA}", "\u{7CB}"),
            ("\u{1E900}", "\u{1E901}"),
        ] {
            let pieces = [(first, 0.0), (" ", 5.0), (second, 18.0)];
            assert_eq!(justified(&pieces, 8.0), [format!("{first} {second}")]);
        }
    }

    /// **And nowhere else**: not beside a left-to-right word on either side,
    /// not after a space that ends no word, and not in left-to-right text at
    /// all — every such line is cut where it was, because carried there the
    /// stretch joined pieces that each read right alone and together read
    /// wrong (a Hebrew word and a longer Latin run, by their majority). A
    /// combining mark ends the word its base letter does.
    #[test]
    fn a_justified_word_space_beside_left_to_right_text_cuts_as_before() {
        for pieces in [
            [("a", 0.0), (" ", 5.0), ("\u{5D1}", 18.0)],
            [("\u{5D0}", 0.0), (" ", 5.0), ("b", 18.0)],
            [("a", 0.0), (" ", 5.0), ("b", 18.0)],
            [("\u{301}", 0.0), (" ", 5.0), ("\u{5D1}", 18.0)],
        ] {
            assert_eq!(
                justified(&pieces, 8.0).len(),
                2,
                "{pieces:?} was joined across its stretched space"
            );
        }
        // An accent ends the Latin word its letter does.
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.word_spacing = 8.0;
        device.show_glyph(&glyph("o", 0.0, 700.0, 10.0), &state);
        device.show_glyph(&glyph("\u{302}", 5.0, 700.0, 10.0), &state);
        device.end_text();
        device.show_glyph(&glyph(" ", 10.0, 700.0, 10.0), &state);
        device.end_text();
        device.show_glyph(&glyph("\u{5D1}", 23.0, 700.0, 10.0), &state);
        device.end_text();
        assert_eq!(device.finish().lines().len(), 2);
        // A space alone on its line ends no word.
        let pieces = [(" ", 5.0), ("\u{5D1}", 18.0)];
        assert_eq!(justified(&pieces, 8.0), ["\u{5D1}"]);
        // A word is all its glyphs: `a.` is a left-to-right word, though its
        // last glyph is a neutral — `(HDAC). ` before a Hebrew word in a
        // justified paragraph with no `dir`.
        let pieces = [("a", 0.0), (".", 5.0), (" ", 10.0), ("\u{5D1}", 23.0)];
        assert_eq!(justified(&pieces, 8.0), ["a. ", "\u{5D1}"]);
        // And a word of neutrals and right-to-left letters is a
        // right-to-left one, whichever glyph ends it.
        let pieces = [("\u{5D0}", 0.0), (".", 5.0), (" ", 10.0), ("\u{5D1}", 23.0)];
        assert_eq!(justified(&pieces, 8.0), ["\u{5D0}. \u{5D1}"]);
    }

    /// **`Tw` is carried only after a code 32** (9.3.3). A space a font draws
    /// with another code — a Type 0 font's CID 3, which its `/ToUnicode`
    /// names a space — is not moved by `Tw`, so a gap after it is a gap the
    /// content stream drew, and the line is cut there. (A two-byte code 32 is
    /// moved by `Tw` as the interpreter moves it; see `show_glyph`.)
    #[test]
    fn word_spacing_carries_the_pen_only_after_a_code_32() {
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.word_spacing = 8.0;
        let mut space = glyph(" ", 5.0, 700.0, 10.0);
        space.code = 3;
        for shown in [
            glyph("\u{5D0}", 0.0, 700.0, 10.0),
            space,
            glyph("\u{5D1}", 18.0, 700.0, 10.0),
        ] {
            device.show_glyph(&shown, &state);
            device.end_text();
        }
        assert_eq!(device.finish().lines().len(), 2);
    }

    /// The lines of a page whose text objects are `objects`, each its glyphs
    /// at their places, under `Tc` of `char_spacing`.
    fn objects(objects: &[&[(&str, f64)]], char_spacing: f64) -> Vec<String> {
        objects_spaced(objects, char_spacing, 0.0)
    }

    /// [`objects`], under `Tw` of `word_spacing` as well.
    fn objects_spaced(
        objects: &[&[(&str, f64)]],
        char_spacing: f64,
        word_spacing: f64,
    ) -> Vec<String> {
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.char_spacing = char_spacing;
        state.text.word_spacing = word_spacing;
        for object in objects {
            for (text, x) in *object {
                device.show_glyph(&glyph(text, *x, 700.0, 10.0), &state);
            }
            device.end_text();
        }
        device
            .finish()
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect()
    }

    /// **Letter-spacing carries the pen within right-to-left text too.** A
    /// Hebrew letter, a space and a full stop in one text object and another
    /// letter in the next, under `Tc` of six points: each glyph's box is half
    /// an em wide and the next is written 0.6 em past it, where `Tc` left the
    /// pen — more than the half em a line closed by `ET` is resumed within.
    /// Read from the box, the line would be cut there: `مُدَّة مُعلِّم.` in
    /// monospace at `letter-spacing: 8px`, its stop drawn left of its space
    /// as UAX #9 draws it, would read `مُدَّة مُعلِّم .`.
    #[test]
    fn letter_spacing_carries_the_pen_within_right_to_left_text() {
        let first: &[(&str, f64)] = &[("\u{5D0}", 0.0), (" ", 11.0), (".", 22.0)];
        let second: &[(&str, f64)] = &[("\u{5D1}", 33.0)];
        assert_eq!(objects(&[first, second], 6.0), ["\u{5D0} .\u{5D1}"]);
        // With no `Tc` the same gap is a gap, and cuts the line as before.
        assert_eq!(objects(&[first, second], 0.0).len(), 2);
        // A space after a right-to-left word carries both its spacings.
        let pieces = [("\u{5D0}", 0.0), (" ", 6.0), ("\u{5D1}", 20.0)];
        assert_eq!(spaced(&pieces, 1.0, 8.0), ["\u{5D0} \u{5D1}"]);
    }

    /// **And only into a right-to-left glyph, where `Tc` alone moved the
    /// pen.** The glyph after a word's last letter is often its space, a
    /// neutral that says nothing of the word after it: carried into it,
    /// `<p>חו x …</p>` letter-spaced by half an em joined its Hebrew word to the
    /// Latin text after it, a line whose majority read it backwards. Beside a
    /// left-to-right letter, or in left-to-right text, nothing is carried
    /// either: every such line is cut where it always was.
    #[test]
    fn letter_spacing_carries_nothing_into_a_neutral_or_beside_left_to_right_text() {
        for pieces in [
            [("\u{5D0}", 0.0), (".", 11.0)],
            [("a", 0.0), ("b", 11.0)],
            [("\u{5D0}", 0.0), ("b", 11.0)],
            [("a", 0.0), ("\u{5D1}", 11.0)],
        ] {
            let lines = spaced(&pieces, 6.0, 0.0);
            assert_eq!(lines.len(), 2, "{pieces:?}: {lines:?}");
        }
        // A Hebrew word, then a space and a Latin letter in a text object of
        // their own: the line is cut before the space, as it was.
        let first: &[(&str, f64)] = &[("\u{5D0}", 0.0)];
        let second: &[(&str, f64)] = &[(" ", 11.0), ("x", 16.0)];
        assert_eq!(objects(&[first, second], 6.0), ["\u{5D0}", " x"]);
        // A word the device cut goes on in the line after the cut: `a]` is a
        // left-to-right word though its `]` begins a line, so the space after
        // it does not carry the pen into the Hebrew letter — `[ŋ̊ə̃] חו`,
        // letter-spaced by half an em, read `חו ]` when it did.
        let word: &[(&str, f64)] = &[("a", 0.0)];
        let bracket: &[(&str, f64)] = &[("]", 11.0), (" ", 16.0)];
        let hebrew: &[(&str, f64)] = &[("\u{5D1}", 27.0)];
        assert_eq!(
            objects(&[word, bracket, hebrew], 6.0),
            ["a", "] ", "\u{5D1}"]
        );
    }

    /// **Spacing carries the pen only into a glyph that is itself right to
    /// left.** After a right-to-left word and a stretched space, the next
    /// text object's first glyph is here a neutral — the `(` of
    /// `<b>(HDAC)</b>`, the `⟨` of `⟨HDAC⟩` — and a neutral says nothing of
    /// the word it opens: carried into it, a dir-less Hebrew line would be
    /// joined to its Latin run and read by its Hebrew ends as a right-to-left
    /// paragraph's, `.מִלְחָמָה … .)HDAC( …`.
    #[test]
    fn spacing_carries_the_pen_only_into_a_right_to_left_glyph() {
        for opening in ["(", "\u{27E8}", "-"] {
            let pieces = [("\u{5D0}", 0.0), (" ", 5.0), (opening, 18.0)];
            assert_eq!(
                justified(&pieces, 8.0).len(),
                2,
                "{opening:?}: carried into a neutral"
            );
        }
        let pieces = [("\u{5D0}", 0.0), (" ", 5.0), ("\u{5D1}", 18.0)];
        assert_eq!(justified(&pieces, 8.0), ["\u{5D0} \u{5D1}"]);
    }

    /// **And after neutrals only on a line holding a right-to-left letter of
    /// its own and no left-to-right one, or after a right-to-left word.** `שָׁלוֹם - (HDAC) - עוֹלָם` with no `dir`, its
    /// `- (HDAC) - ` one text object: the stretched space after the second
    /// `-` carried the pen into the Hebrew, and the line, which held the
    /// Latin, was read by its two Hebrew ends as a right-to-left paragraph's.
    /// After a right-to-left word the pen is carried still, on any line: the
    /// two words of a Hebrew gloss in an English sentence, `peace (עוֹלָם
    /// מִלְחָמָה)`, justified.
    #[test]
    fn spacing_carries_nothing_after_neutrals_on_a_line_that_holds_a_latin_letter() {
        let hebrew: &[(&str, f64)] = &[("\u{5D1}", 28.0)];
        let latin_dash: &[(&str, f64)] = &[("a", 0.0), (" ", 5.0), ("-", 10.0), (" ", 15.0)];
        assert_eq!(objects_spaced(&[latin_dash, hebrew], 0.0, 8.0).len(), 2);
        let hebrew_dash: &[(&str, f64)] = &[("\u{5D0}", 0.0), (" ", 5.0), ("-", 10.0), (" ", 15.0)];
        assert_eq!(objects_spaced(&[hebrew_dash, hebrew], 0.0, 8.0).len(), 1);
        let latin_word: &[(&str, f64)] = &[("a", 0.0), (" ", 5.0), ("\u{5D0}", 10.0), (" ", 15.0)];
        assert_eq!(objects_spaced(&[latin_word, hebrew], 0.0, 8.0).len(), 1);
    }

    /// **After neutrals, only on a line that holds a right-to-left letter of
    /// its own.** The device starts a line afresh at every cut, so a neutral
    /// cut from the Latin text it belongs to — an opening `«`, `"`, `(` or `#`
    /// in a text object of its own, `<p><i>«</i>שָׁלוֹם עוֹלָם<i>»</i></p>`
    /// letter-spaced by `0.55em` — begins a line that holds no left-to-right
    /// letter, and carried from it into the Hebrew word the line would read
    /// right to left with its `«` last, `שָׁלוֹם עוֹלָם«`; and `<i>?!</i> שָׁלוֹם`
    /// at `word-spacing: 1em`, its `?!` and a stretched space before the word,
    /// would read `שָׁלוֹם !?`. Taken where the line holds a right-to-left
    /// letter of its own, the pen still goes on after `- ` or `( ` in a
    /// right-to-left line.
    #[test]
    fn spacing_carries_nothing_out_of_a_line_of_neutrals_alone() {
        let hebrew: &[(&str, f64)] = &[("\u{5D1}", 11.0)];
        for opening in ["\u{AB}", "\"", "(", "#"] {
            let mark: &[(&str, f64)] = &[(opening, 0.0)];
            assert_eq!(
                objects(&[mark, hebrew], 6.0),
                [opening, "\u{5D1}"],
                "{opening:?}: carried out of a line of neutrals"
            );
        }
        let marks: &[(&str, f64)] = &[("?", 0.0), ("!", 5.0), (" ", 10.0)];
        let hebrew: &[(&str, f64)] = &[("\u{5D1}", 23.0)];
        assert_eq!(objects_spaced(&[marks, hebrew], 0.0, 8.0).len(), 2);
    }

    /// **A word a cut interrupted counts against a carry, and never for
    /// one.** Two table cells with no space between them, letter-spaced: the
    /// first cell's Hebrew word and the second's `[` are one word to the
    /// device, cut where the cell ends, and the line the `[` begins goes on
    /// with that right-to-left word — were the word counted for a carry, the
    /// pen would go on from the `[` into the second cell's Hebrew, and
    /// `[שָׁלוֹם]` would read `שָׁלוֹם[`. A left-to-right word interrupted so
    /// still stops a carry: `[ŋ̊ə̃] חו`
    /// (`letter_spacing_carries_nothing_into_a_neutral_or_beside_left_to_right_text`).
    #[test]
    fn a_word_a_cut_interrupted_never_enables_a_carry() {
        let first: &[(&str, f64)] = &[("\u{5D0}", 0.0)];
        let bracket: &[(&str, f64)] = &[("[", 11.0)];
        let second: &[(&str, f64)] = &[("\u{5D1}", 22.0)];
        assert_eq!(
            objects(&[first, bracket, second], 6.0),
            ["\u{5D0}", "[", "\u{5D1}"]
        );
    }

    /// **A word goes on across a cut only where the spacing put the glyph
    /// after it.** Two cells of a right-to-left table row at
    /// `word-spacing: 1em`, `<td>def</td><td>أصبحنا نعرف</td>`, have no white
    /// space between them, and the Arabic cell's first drawn glyph starts 4.7
    /// em past the end of `def`, where no spacing put it. If the line it
    /// begins went on with `def`'s left-to-right word, the Arabic letters
    /// would join that word, the stretched space after them would carry
    /// nothing, and the device would cut the cell in two at that space. A gap
    /// no spacing accounts for ends the word as white space does: here 0.8 em
    /// with no spacing at all, and 0.45 em past where a tenth of an em of
    /// letter-spacing left the pen. Where the spacing did put the next glyph
    /// the word goes on, and still counts against a carry: `a` letter-spaced
    /// by 0.6 em and cut before a Hebrew letter is one left-to-right word with
    /// it, as `[ŋ̊ə̃]` cut before its `]` is
    /// (`letter_spacing_carries_nothing_into_a_neutral_or_beside_left_to_right_text`).
    #[test]
    fn a_gap_no_spacing_accounts_for_ends_the_word_before_it() {
        let pieces = [
            ("a", 0.0),
            ("\u{5D0}", 13.0),
            (" ", 18.0),
            ("\u{5D1}", 31.0),
        ];
        assert_eq!(justified(&pieces, 8.0), ["a", "\u{5D0} \u{5D1}"]);
        let latin: &[(&str, f64)] = &[("a", 0.0)];
        let hebrew: &[(&str, f64)] = &[("\u{5D0}", 10.5), (" ", 15.5)];
        let next: &[(&str, f64)] = &[("\u{5D1}", 29.5)];
        assert_eq!(
            objects_spaced(&[latin, hebrew, next], 1.0, 8.0),
            ["a", "\u{5D0} \u{5D1}"]
        );
        let hebrew: &[(&str, f64)] = &[("\u{5D0}", 11.0), (" ", 22.0)];
        let next: &[(&str, f64)] = &[("\u{5D1}", 41.0)];
        assert_eq!(
            objects_spaced(&[latin, hebrew, next], 6.0, 8.0),
            ["a", "\u{5D0} ", "\u{5D1}"]
        );
    }

    /// **A glyph reads right to left by its own class, not its block.** A
    /// digit is no right-to-left letter, an Arabic-Indic one either: its
    /// `Bidi_Class` is `AN`, though it stands in the Arabic block. Carried
    /// into `(١٢٣)` after a Hebrew word in a justified paragraph, the digits
    /// would read `٢٣ (١)`, and a search for `١٢٣` would find nothing. The
    /// punctuation of the right-to-left scripts does read right to left, the
    /// ARABIC COMMA among it though its class is `CS`: a right-to-left word
    /// that ends in one is drawn from it, and a justified line of a
    /// right-to-left paragraph narrowed by `margin: 0 35%`, not carried into
    /// it, would read `منخلايا،`, its words apart.
    #[test]
    fn a_glyph_reads_right_to_left_by_its_class() {
        for digit in ["\u{661}", "\u{6F1}", "1"] {
            let pieces = [("\u{5D0}", 0.0), (" ", 5.0), (digit, 18.0)];
            assert_eq!(justified(&pieces, 8.0).len(), 2, "{digit:?}");
        }
        let pieces = [("\u{627}", 0.0), (" ", 5.0), ("\u{60C}", 18.0)];
        assert_eq!(justified(&pieces, 8.0), ["\u{627} \u{60C}"]);
        assert!(!reads_right_to_left('\u{661}'));
        assert!(!reads_right_to_left('\u{64B}'));
        assert!(reads_right_to_left('\u{627}') && reads_right_to_left('\u{5D0}'));
        assert!(reads_right_to_left('\u{200F}'), "RIGHT-TO-LEFT MARK is `R`");
    }

    /// **And only as far as the spacing reaches**, give or take a rounding.
    /// Two Hebrew table cells letter-spaced by half an em, `border-spacing:
    /// 2px` apart: the second cell starts a quarter of an em past where `Tc`
    /// left the pen, and carried there the two would read as one word,
    /// `גדולספר`.
    #[test]
    fn spacing_carries_the_pen_only_as_far_as_it_reaches() {
        let first: &[(&str, f64)] = &[("\u{5D0}", 0.0)];
        // The box ends at 5, and `Tc` leaves the pen at 11.
        for (at, lines) in [(11.0, 1), (11.4, 1), (10.6, 1), (14.0, 2), (12.0, 2)] {
            let second: &[(&str, f64)] = &[("\u{5D1}", at)];
            assert_eq!(objects(&[first, second], 6.0).len(), lines, "at {at}");
        }
    }

    /// **A cut at a word space's stretch is mended once the piece after it
    /// ends** (`docs/design/rtl-read-back.md`, "A word space styled apart
    /// across a two-em stretch"). `a` and its word space, `Tw` of eight
    /// points moving the pen on to 18, and `ב c` in a text object of its own
    /// at the stretched pen: the carry refuses the gap after a left-to-right
    /// word, and the piece after it, ending in `c`, makes with it a line
    /// whose ends both read left to right.
    #[test]
    fn a_cut_at_a_word_spaces_stretch_is_mended_where_the_line_reads_left_to_right() {
        let a: &[(&str, f64)] = &[("a", 0.0), (" ", 5.0)];
        let cell = |at: f64| [("\u{5D1}", at), (" ", at + 5.0), ("c", at + 18.0)];
        let mended = cell(18.0);
        assert_eq!(objects_spaced(&[a, &mended], 0.0, 8.0), ["a \u{5D1} c"]);
        // Three pieces, as `αλφα <b>βήτα</b> ספר בית xyz.` is cut at
        // `word-spacing: 2em`: `x` and `y` apart, since the two hold no
        // right-to-left letter, and `y` and `ב z` one line.
        let x: &[(&str, f64)] = &[("x", 0.0), (" ", 5.0)];
        let y: &[(&str, f64)] = &[("y", 18.0), (" ", 23.0)];
        let gloss: &[(&str, f64)] = &[("\u{5D1}", 36.0), (" ", 41.0), ("z", 54.0)];
        assert_eq!(
            objects_spaced(&[x, y, gloss], 0.0, 8.0),
            ["x ", "y \u{5D1} z"]
        );
        // With no `Tw` the gap is one the spacing does not account for.
        assert_eq!(objects_spaced(&[a, &mended], 0.0, 0.0).len(), 2);
    }

    /// **And nowhere else**: not a cell set two points past or short of the
    /// stretched pen, where padding or `border-spacing` put it; not a line
    /// with Hebrew at either end, which ruling 14 reads by its majority or
    /// from the right; and not left-to-right text, collected as before.
    #[test]
    fn a_cut_at_a_word_spaces_stretch_stays_where_the_line_would_not_read_left_to_right() {
        let a: &[(&str, f64)] = &[("a", 0.0), (" ", 5.0)];
        for at in [20.0, 16.0] {
            let cell = [("\u{5D1}", at), (" ", at + 5.0), ("c", at + 18.0)];
            assert_eq!(
                objects_spaced(&[a, &cell], 0.0, 8.0).len(),
                2,
                "a cell at {at} was joined"
            );
        }
        let hebrew: &[(&str, f64)] = &[("\u{5D0}", 0.0), (" ", 5.0), ("b", 18.0), (" ", 23.0)];
        let last: &[(&str, f64)] = &[("\u{5D2}", 36.0)];
        assert_eq!(
            objects_spaced(&[hebrew, last], 0.0, 8.0),
            ["\u{5D0} b ", "\u{5D2}"]
        );
        let latin: &[(&str, f64)] = &[("b", 18.0), (" ", 23.0), ("c", 36.0)];
        assert_eq!(objects_spaced(&[a, latin], 0.0, 8.0), ["a ", "b c"]);
    }

    /// The texts of the lines `device` collected.
    fn read_lines(device: TextDevice) -> Vec<String> {
        device
            .finish()
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect()
    }

    /// The lines of a page whose text objects are `objects`, each glyph
    /// `(text, x, y)` drawn at `(x, y)` in text space under `page`, under `Tc`
    /// of `char_spacing` and `Tw` of `word_spacing`.
    fn objects_under(
        objects: &[Vec<(&str, f64, f64)>],
        char_spacing: f64,
        word_spacing: f64,
        page: &Matrix,
    ) -> Vec<String> {
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.char_spacing = char_spacing;
        state.text.word_spacing = word_spacing;
        for object in objects {
            for (text, x, y) in object {
                let mut shown = glyph(text, *x, *y, 10.0);
                shown.transform = shown.transform.then(page);
                device.show_glyph(&shown, &state);
            }
            device.end_text();
        }
        read_lines(device)
    }

    /// A page's text objects, each its glyphs `(text, x, y)`.
    type Objects = Vec<Vec<(&'static str, f64, f64)>>;

    /// `objects`, each glyph on the baseline `y = 700`.
    fn on_700(objects: &[&[(&'static str, f64)]]) -> Objects {
        objects
            .iter()
            .map(|object| object.iter().map(|(text, x)| (*text, *x, 700.0)).collect())
            .collect()
    }

    /// Each of `pieces` a text object of its own, as [`justified`] draws them.
    fn each_alone(pieces: &[(&'static str, f64)]) -> Objects {
        pieces
            .iter()
            .map(|(text, x)| vec![(*text, *x, 700.0)])
            .collect()
    }

    /// **A line under a half turn is continued and cut as it is unturned**
    /// (`docs/design/rtl-read-back.md`, "A line under a half turn"). The
    /// glyphs of the tests above that hold where a line is cut — a text
    /// object's end resumed and a gap kept, a stretched word space and
    /// letter-spacing carried and refused, a word taken whole across a cut, a
    /// cut at a word space's stretch mended and kept — and a new baseline and
    /// glyphs gone back past three ems and within them, drawn under a half
    /// turn and under a quarter turn at places exact in binary, read into the
    /// same lines, characters and order as unturned: each line is measured
    /// along its own baseline and across it ([`Frame`]). Measured on the
    /// page's `x` and `y`, a half turn's pen would stand where its last glyph
    /// starts, so a text object's end would part none of these gaps, and a
    /// quarter turn's every glyph would stand half an em up the page from the
    /// one before.
    #[test]
    fn a_line_under_a_half_turn_is_continued_and_cut_as_it_is_unturned() {
        let (alef, bet, gimel) = ("\u{5D0}", "\u{5D1}", "\u{5D2}");
        let mut sets: Vec<(f64, f64, Objects)> = vec![
            (
                0.0,
                0.0,
                on_700(&[
                    &[("A", 0.0), ("B", 5.0)],
                    &[("C", 10.0), ("D", 15.0)],
                    &[("E", 20.0), ("F", 25.0)],
                ]),
            ),
            (0.0, 0.0, on_700(&[&[("A", 0.0)], &[("B", 200.0)]])),
            (0.0, 0.0, vec![vec![("A", 0.0, 700.0), ("B", 0.0, 680.0)]]),
            (0.0, 0.0, on_700(&[&[("a", 100.0)], &[("b", 60.0)]])),
            (0.0, 0.0, on_700(&[&[("a", 100.0), ("b", 75.0)]])),
            (
                0.0,
                8.0,
                on_700(&[
                    &[(alef, 0.0), (" ", 5.0), ("(", 18.0), (" ", 23.0)],
                    &[(bet, 36.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("o", 0.0), ("\u{302}", 5.0)],
                    &[(" ", 10.0)],
                    &[(bet, 23.0)],
                ]),
            ),
            (
                6.0,
                0.0,
                on_700(&[&[(alef, 0.0), (" ", 11.0), (".", 22.0)], &[(bet, 33.0)]]),
            ),
            (
                0.0,
                0.0,
                on_700(&[&[(alef, 0.0), (" ", 11.0), (".", 22.0)], &[(bet, 33.0)]]),
            ),
            (
                6.0,
                0.0,
                on_700(&[&[(alef, 0.0)], &[(" ", 11.0), ("x", 16.0)]]),
            ),
            (
                6.0,
                0.0,
                on_700(&[&[("a", 0.0)], &[("]", 11.0), (" ", 16.0)], &[(bet, 27.0)]]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0), ("-", 10.0), (" ", 15.0)],
                    &[(bet, 28.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[(alef, 0.0), (" ", 5.0), ("-", 10.0), (" ", 15.0)],
                    &[(bet, 28.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0), (alef, 10.0), (" ", 15.0)],
                    &[(bet, 28.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[&[("?", 0.0), ("!", 5.0), (" ", 10.0)], &[(bet, 23.0)]]),
            ),
            (
                6.0,
                0.0,
                on_700(&[&[(alef, 0.0)], &[("[", 11.0)], &[(bet, 22.0)]]),
            ),
            (
                1.0,
                8.0,
                on_700(&[&[("a", 0.0)], &[(alef, 10.5), (" ", 15.5)], &[(bet, 29.5)]]),
            ),
            (
                6.0,
                8.0,
                on_700(&[&[("a", 0.0)], &[(alef, 11.0), (" ", 22.0)], &[(bet, 41.0)]]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0)],
                    &[(bet, 18.0), (" ", 23.0), ("c", 36.0)],
                ]),
            ),
            (
                0.0,
                0.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0)],
                    &[(bet, 18.0), (" ", 23.0), ("c", 36.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("x", 0.0), (" ", 5.0)],
                    &[("y", 18.0), (" ", 23.0)],
                    &[(bet, 36.0), (" ", 41.0), ("z", 54.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[(alef, 0.0), (" ", 5.0), ("b", 18.0), (" ", 23.0)],
                    &[(gimel, 36.0)],
                ]),
            ),
            (
                0.0,
                8.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0)],
                    &[("b", 18.0), (" ", 23.0), ("c", 36.0)],
                ]),
            ),
        ];
        for word_spacing in [8.0, 0.0] {
            sets.push((
                0.0,
                word_spacing,
                each_alone(&[(alef, 0.0), (" ", 5.0), (bet, 18.0)]),
            ));
        }
        for (first, second) in [
            ("\u{750}", "\u{751}"),
            ("\u{7CA}", "\u{7CB}"),
            ("\u{1E900}", "\u{1E901}"),
        ] {
            sets.push((
                0.0,
                8.0,
                each_alone(&[(first, 0.0), (" ", 5.0), (second, 18.0)]),
            ));
        }
        for (before, after) in [("a", bet), (alef, "b"), ("a", "b"), ("\u{301}", bet)] {
            sets.push((
                0.0,
                8.0,
                each_alone(&[(before, 0.0), (" ", 5.0), (after, 18.0)]),
            ));
        }
        for opening in ["(", "\u{27E8}", "-", "\u{661}", "\u{6F1}", "1"] {
            sets.push((
                0.0,
                8.0,
                each_alone(&[(alef, 0.0), (" ", 5.0), (opening, 18.0)]),
            ));
        }
        sets.push((
            0.0,
            8.0,
            each_alone(&[("\u{627}", 0.0), (" ", 5.0), ("\u{60C}", 18.0)]),
        ));
        sets.push((0.0, 8.0, each_alone(&[(" ", 5.0), (bet, 18.0)])));
        for first in ["a", alef] {
            sets.push((
                0.0,
                8.0,
                each_alone(&[(first, 0.0), (".", 5.0), (" ", 10.0), (bet, 23.0)]),
            ));
        }
        sets.push((
            0.0,
            8.0,
            each_alone(&[("a", 0.0), (alef, 13.0), (" ", 18.0), (bet, 31.0)]),
        ));
        sets.push((
            1.0,
            8.0,
            each_alone(&[(alef, 0.0), (" ", 6.0), (bet, 20.0)]),
        ));
        for (first, second) in [(alef, "."), ("a", "b"), (alef, "b"), ("a", bet)] {
            sets.push((6.0, 0.0, each_alone(&[(first, 0.0), (second, 11.0)])));
        }
        for opening in ["\u{AB}", "\"", "(", "#"] {
            sets.push((6.0, 0.0, each_alone(&[(opening, 0.0), (bet, 11.0)])));
        }
        for at in [11.0, 11.25, 10.75, 14.0, 12.0] {
            sets.push((6.0, 0.0, each_alone(&[(alef, 0.0), (bet, at)])));
        }
        for at in [20.0, 16.0] {
            sets.push((
                0.0,
                8.0,
                on_700(&[
                    &[("a", 0.0), (" ", 5.0)],
                    &[(bet, at), (" ", at + 5.0), ("c", at + 18.0)],
                ]),
            ));
        }
        let half = Matrix {
            a: -1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 1024.0,
            f: 1400.0,
        };
        let quarter = Matrix {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 800.0,
            f: 0.0,
        };
        for (char_spacing, word_spacing, objects) in &sets {
            let flat = objects_under(objects, *char_spacing, *word_spacing, &Matrix::IDENTITY);
            for (turn, page) in [("a half turn", &half), ("a quarter turn", &quarter)] {
                assert_eq!(
                    objects_under(objects, *char_spacing, *word_spacing, page),
                    flat,
                    "{objects:?} under {turn}, Tc {char_spacing}, Tw {word_spacing}"
                );
            }
        }
        // A loop over nothing would pass: every set is here, and most of
        // them are more than one line.
        assert_eq!(sets.len(), 59);
        let cut = sets
            .iter()
            .filter(|(tc, tw, objects)| {
                objects_under(objects, *tc, *tw, &Matrix::IDENTITY).len() > 1
            })
            .count();
        assert_eq!(cut, 41);
    }

    /// The lines of a page whose text objects are `objects`, each glyph
    /// `(text, x, y)` set at `(x, y)` on the page, its transform `linear`
    /// scaled by a size of 10, under `Tw` of `word_spacing`.
    fn placed(
        objects: &[Vec<(&str, f64, f64)>],
        linear: &Matrix,
        word_spacing: f64,
    ) -> Vec<String> {
        let mut device = TextDevice::new();
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.text.word_spacing = word_spacing;
        let sized = Matrix::scale(10.0, 10.0).then(linear);
        for object in objects {
            for (text, x, y) in object {
                let mut shown = glyph(text, *x, *y, 10.0);
                shown.transform = Matrix {
                    e: *x,
                    f: *y,
                    ..sized
                };
                device.show_glyph(&shown, &state);
            }
            device.end_text();
        }
        read_lines(device)
    }

    /// **An unturned line reads as before**: a line whose direction is
    /// `(a, 0)` with `a > 0` is measured on the page's `x` and `y`
    /// themselves, not by a dot product ([`Frame`]). Each threshold a line is
    /// cut at, met exactly — a text object's end resumed half an em past the
    /// pen, a glyph three ems back, a glyph half an em up the page, a word
    /// space's stretch carried and mended within a twentieth of an em — under
    /// the identity, a shift and `scaleX(2)`, continues the line, and passed,
    /// cuts it, as the page's `x` and `y` decide: each place below is the sum
    /// the device itself makes, so a tie is a tie in its own arithmetic.
    #[test]
    fn an_unturned_line_reads_as_before() {
        let (alef, bet) = ("\u{5D0}", "\u{5D1}");
        for (name, linear, (x0, y0)) in [
            ("the identity", Matrix::IDENTITY, (0.0, 0.0)),
            ("a shift", Matrix::IDENTITY, (64.5, -32.25)),
            ("scaleX(2)", Matrix::scale(2.0, 1.0), (-35.0, 0.0)),
        ] {
            let sized = Matrix::scale(10.0, 10.0).then(&linear);
            let em = sized.expansion().max(1.0);
            let (slack, back, rounding) = (em * 0.5, em * 3.0, em * STRETCH_ROUNDING);
            // Where a glyph set at `x` ends, and where a word space set at `x`
            // under `Tw` of 7.5 leaves the pen.
            let end = |x: f64| Matrix { e: x, ..sized }.apply(0.5, 0.0).0;
            let stretch = |x: f64| Matrix { e: x, ..sized }.apply(1.25, 0.0).0;
            for past in [false, true] {
                let lines = |tie: Vec<String>, cut: Vec<String>| if past { cut } else { tie };
                let by = if past { 1.0 } else { 0.0 };
                let resumed = vec![vec![("a", x0, y0)], vec![("b", end(x0) + slack + by, y0)]];
                assert_eq!(
                    placed(&resumed, &linear, 0.0),
                    lines(vec!["ab".into()], vec!["a".into(), "b".into()]),
                    "{name}: a text object's end resumed half an em past the pen, past {past}"
                );
                let jumped = vec![vec![("a", x0 + back + by, y0)], vec![("b", x0, y0)]];
                assert_eq!(
                    placed(&jumped, &linear, 0.0),
                    lines(vec!["ab".into()], vec!["a".into(), "b".into()]),
                    "{name}: a glyph three ems back, past {past}"
                );
                let raised = vec![vec![("a", x0, y0), ("b", end(x0), y0 + slack + by)]];
                assert_eq!(
                    placed(&raised, &linear, 0.0),
                    lines(vec!["ab".into()], vec!["a".into(), "b".into()]),
                    "{name}: a glyph half an em up the page, past {past}"
                );
                let reach = if past { 2.0 * rounding } else { rounding };
                let carried = vec![
                    vec![(alef, x0, y0)],
                    vec![(" ", end(x0), y0)],
                    vec![(bet, stretch(end(x0)) + reach, y0)],
                ];
                assert_eq!(
                    placed(&carried, &linear, 7.5),
                    lines(
                        vec![format!("{alef} {bet}")],
                        vec![format!("{alef} "), bet.to_string()]
                    ),
                    "{name}: a word space's stretch carried, past {past}"
                );
                let at = stretch(end(x0)) + reach;
                let mended = vec![
                    vec![("a", x0, y0), (" ", end(x0), y0)],
                    vec![(bet, at, y0), (" ", end(at), y0), ("c", end(end(at)), y0)],
                ];
                assert_eq!(
                    placed(&mended, &linear, 7.5),
                    lines(
                        vec![format!("a {bet} c")],
                        vec!["a ".into(), format!("{bet} c")]
                    ),
                    "{name}: a cut at a word space's stretch mended, past {past}"
                );
            }
        }
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

    /// A run of glyphs, one per character, half an em apart on one baseline.
    fn run(text: &str, x: f64, y: f64) -> Vec<Glyph> {
        text.chars()
            .enumerate()
            .map(|(i, c)| glyph(&c.to_string(), x + i as f64 * 5.0, y, 10.0))
            .collect()
    }

    /// Hyphen rejoining through a page the device assembled, not only through
    /// [`crate::plain::assemble`]: the lines it joins are the lines
    /// extraction made, and the default is `plain_text` to the byte.
    #[test]
    fn plain_text_with_rejoins_hyphens_across_the_lines_extraction_made() {
        let mut glyphs = run("a hyphen-", 0.0, 700.0);
        glyphs.extend(run("ation and a soft\u{AD}", 0.0, 688.0));
        glyphs.extend(run("ware well-", 0.0, 676.0));
        glyphs.extend(run("Known", 0.0, 664.0));
        let p = page(&glyphs);
        assert_eq!(p.lines().len(), 4);

        let default = p.plain_text_with(&PlainTextOptions::default());
        assert_eq!(default.text, p.plain_text(), "the default changes nothing");
        assert_eq!(default.hyphens.joins(), 0);

        let joined = p.plain_text_with(&PlainTextOptions {
            rejoin_hyphens: true,
        });
        assert_eq!(
            joined.text, "a hyphenation and a software well-\nKnown\n",
            "the soft hyphen joined, the hard one before a capital did not"
        );
        assert_eq!(joined.hyphens.hard_joins, 1);
        assert_eq!(joined.hyphens.soft_joins, 1);
        assert_eq!(joined.hyphens.soft_removed, 1);
    }

    #[test]
    fn non_finite_glyphs_are_dropped_rather_than_poisoning_the_page() {
        let mut bad = glyph("X", 0.0, 0.0, 10.0);
        bad.transform.e = f64::NAN;
        let p = page(&[bad, glyph("A", 0.0, 700.0, 10.0)]);
        assert_eq!(p.plain_text(), "A\n");
    }

    /// A glyph of `code` that maps to no text, drawn at `x`.
    fn unmapped(code: u32, x: f64) -> Glyph {
        Glyph {
            code,
            text: String::new(),
            ..glyph("x", x, 700.0, 10.0)
        }
    }

    /// The text and the quad of every character of `page`, in order.
    fn drawn_chars(page: &TextPage) -> Vec<(String, Quad)> {
        page.lines()
            .iter()
            .flat_map(|line| line.chars.iter())
            .map(|c| (c.text.clone(), c.quad))
            .collect()
    }

    /// **A code nothing maps is said, once**: two glyphs of one unmapped
    /// code and one of another give two warnings, in the order first met,
    /// and the text, the lines and the quads are the page's without them.
    #[test]
    fn a_code_nothing_maps_is_reported_once() {
        let reported = page(&[
            glyph("a", 0.0, 700.0, 10.0),
            unmapped(1, 5.0),
            unmapped(1, 10.0),
            unmapped(2, 15.0),
            glyph("b", 20.0, 700.0, 10.0),
        ]);
        assert_eq!(
            reported.warnings,
            vec![
                TextWarning::UnmappedCode { code: 1 },
                TextWarning::UnmappedCode { code: 2 },
            ]
        );
        let drawn = page(&[glyph("a", 0.0, 700.0, 10.0), glyph("b", 20.0, 700.0, 10.0)]);
        assert!(drawn.warnings.is_empty(), "{:?}", drawn.warnings);
        assert_eq!(reported.plain_text(), "ab\n");
        assert_eq!(reported.plain_text(), drawn.plain_text());
        assert_eq!(reported.lines().len(), drawn.lines().len());
        assert_eq!(drawn_chars(&reported), drawn_chars(&drawn));
    }

    /// A non-finite glyph whose code maps to nothing is said as any other
    /// is: what it stands for is the question, not where it was drawn.
    #[test]
    fn a_non_finite_glyph_of_an_unmapped_code_is_reported() {
        let mut bad = unmapped(7, 0.0);
        bad.transform.f = f64::INFINITY;
        let p = page(&[bad, glyph("A", 0.0, 700.0, 10.0)]);
        assert_eq!(p.plain_text(), "A\n");
        assert_eq!(p.warnings, vec![TextWarning::UnmappedCode { code: 7 }]);
    }

    /// **Each kind is bounded apart**: a page of a hundred unmapped codes
    /// keeps the first sixty-four, and still names every font it could not
    /// resolve after them; and a page of seventy unknown fonts keeps
    /// sixty-four of those.
    #[test]
    fn unmapped_codes_are_bounded_apart_from_unknown_fonts() {
        let mut d = TextDevice::new();
        let state = GraphicsState::new(Matrix::IDENTITY);
        for code in 0..100u32 {
            d.show_glyph(&unmapped(code, f64::from(code)), &state);
        }
        for name in ["F1", "F2", "F1", "F3"] {
            d.warn(TextWarning::UnknownFont { name: name.into() });
        }
        let p = d.finish();
        let codes: Vec<u32> = p
            .warnings
            .iter()
            .filter_map(|w| match w {
                TextWarning::UnmappedCode { code } => Some(*code),
                TextWarning::UnknownFont { .. } => None,
            })
            .collect();
        assert_eq!(codes, (0..64).collect::<Vec<u32>>());
        let fonts: Vec<&str> = p
            .warnings
            .iter()
            .filter_map(|w| match w {
                TextWarning::UnknownFont { name } => Some(name.as_str()),
                TextWarning::UnmappedCode { .. } => None,
            })
            .collect();
        assert_eq!(fonts, vec!["F1", "F2", "F3"]);
        assert!(p.plain_text().is_empty());

        let mut d = TextDevice::new();
        for at in 0..70 {
            d.warn(TextWarning::UnknownFont {
                name: format!("F{at}"),
            });
        }
        d.warn(TextWarning::UnmappedCode { code: 9 });
        let p = d.finish();
        assert_eq!(p.warnings.len(), 65);
        assert_eq!(
            p.warnings.last(),
            Some(&TextWarning::UnmappedCode { code: 9 })
        );
    }

    /// A device reading as `unmapped` asks of its glyphs.
    fn falling_back(unmapped: Unmapped) -> TextDevice {
        TextDevice::with_options(&ExtractionOptions {
            unmapped,
            ..ExtractionOptions::default()
        })
    }

    /// **An unmapped glyph falls back as its options ask**
    /// (`docs/design/extraction-options.md`, option 5): dropped by default;
    /// kept as U+F0000 plus its code, or U+100000 plus its glyph index, with
    /// the box a mapped glyph there would have; U+FFFD past an area's end and
    /// for a glyph whose font cannot say; and reported in every mode.
    #[test]
    fn an_unmapped_code_falls_back_as_its_options_ask() {
        let state = GraphicsState::new(Matrix::IDENTITY);
        let unmapped_at = |code: u32, glyph_id: Option<u16>, x: f64| Glyph {
            glyph_id,
            ..unmapped(code, x)
        };
        let read = |mode: Unmapped| {
            let mut d = falling_back(mode);
            d.show_glyph(&glyph("a", 0.0, 700.0, 10.0), &state);
            d.show_glyph(&unmapped_at(0x41, Some(7), 5.0), &state);
            d.show_glyph(&unmapped_at(0x1_0000, Some(0xFFFE), 10.0), &state);
            d.show_glyph(&unmapped_at(0xFFFD, None, 15.0), &state);
            d.show_glyph(&glyph("b", 20.0, 700.0, 10.0), &state);
            d.finish()
        };
        let reported = vec![
            TextWarning::UnmappedCode { code: 0x41 },
            TextWarning::UnmappedCode { code: 0x1_0000 },
            TextWarning::UnmappedCode { code: 0xFFFD },
        ];

        let dropped = read(Unmapped::Drop);
        assert_eq!(dropped.plain_text(), "ab\n");
        assert_eq!(dropped.warnings, reported);

        let code = read(Unmapped::Code);
        assert_eq!(code.plain_text(), "a\u{F0041}\u{FFFD}\u{FFFFD}b\n");
        assert_eq!(code.warnings, reported);

        let glyphs = read(Unmapped::Glyph);
        assert_eq!(glyphs.plain_text(), "a\u{100007}\u{FFFD}\u{FFFD}b\n");
        assert_eq!(glyphs.warnings, reported);

        // Each kept glyph has the box a glyph mapped to text there has.
        let mapped = page(&[
            glyph("a", 0.0, 700.0, 10.0),
            glyph("x", 5.0, 700.0, 10.0),
            glyph("y", 10.0, 700.0, 10.0),
            glyph("z", 15.0, 700.0, 10.0),
            glyph("b", 20.0, 700.0, 10.0),
        ]);
        let quads = |page: &TextPage| -> Vec<Quad> {
            drawn_chars(page)
                .into_iter()
                .map(|(_, quad)| quad)
                .collect()
        };
        assert_eq!(quads(&code), quads(&mapped));
        assert_eq!(quads(&glyphs), quads(&mapped));
        assert_eq!(code.lines().len(), 1);
    }

    /// The areas end where their planes' noncharacters begin, and the
    /// value is the character less its area's first code point.
    #[test]
    fn an_unmapped_codes_character_is_its_area_plus_its_value() {
        assert_eq!(Unmapped::Drop.text(5, Some(5)), None);
        assert_eq!(Unmapped::Code.text(0, None), Some('\u{F0000}'));
        assert_eq!(Unmapped::Code.text(0xFFFD, None), Some('\u{FFFFD}'));
        assert_eq!(Unmapped::Code.text(0xFFFE, None), Some('\u{FFFD}'));
        assert_eq!(Unmapped::Code.text(u32::MAX, None), Some('\u{FFFD}'));
        assert_eq!(Unmapped::Glyph.text(9, Some(0)), Some('\u{100000}'));
        assert_eq!(Unmapped::Glyph.text(9, Some(0xFFFD)), Some('\u{10FFFD}'));
        assert_eq!(Unmapped::Glyph.text(9, Some(0xFFFF)), Some('\u{FFFD}'));
        assert_eq!(Unmapped::Glyph.text(9, None), Some('\u{FFFD}'));
        for value in [0u32, 0x41, 0x1234, 0xFFFD] {
            let code = Unmapped::Code.text(value, None).expect("kept");
            assert_eq!(u32::from(code) - UNMAPPED_CODE_AREA, value);
            let glyph = Unmapped::Glyph
                .text(0, u16::try_from(value).ok())
                .expect("kept");
            assert_eq!(u32::from(glyph) - UNMAPPED_GLYPH_AREA, value);
        }
    }

    /// The device asks for glyph indices only where its mode stands a glyph
    /// for its index: the default reading asks nothing of a font program.
    #[test]
    fn the_device_asks_for_glyph_ids_only_for_the_glyph_fallback() {
        assert_eq!(TextDevice::new().glyph_queries(), GlyphQueries::default());
        assert_eq!(
            falling_back(Unmapped::Code).glyph_queries(),
            GlyphQueries::default()
        );
        assert!(
            falling_back(Unmapped::Glyph)
                .glyph_queries()
                .unmapped_glyph_ids
        );
        assert!(
            !TextDevice::keeping_artifacts()
                .glyph_queries()
                .unmapped_glyph_ids
        );
    }

    /// A source that stands each byte for itself but `0x01` and `0x02`,
    /// which stand for nothing, selects glyph `3 * code` for every code, and
    /// counts how often it is asked.
    struct Selecting {
        asked: std::cell::Cell<usize>,
    }

    impl crate::interpret::FontSource for Selecting {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| {
                    let text = if b <= 2 {
                        String::new()
                    } else {
                        char::from(b).to_string()
                    };
                    (u32::from(b), text, 500.0)
                })
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn glyph_id(&self, _font: &[u8], code: u32) -> Option<u16> {
            self.asked.set(self.asked.get() + 1);
            u16::try_from(code * 3).ok()
        }
    }

    /// **The interpreter asks for a glyph index only where the device wants
    /// one, and only for a code with no text**: a default device's reading
    /// asks the source nothing, and one standing glyphs for their indices
    /// asks once for each glyph whose code maps to nothing, and stands each
    /// for the index the source selects.
    #[test]
    fn the_interpreter_asks_for_a_glyph_id_only_where_the_device_wants_one() {
        let content = b"BT /F 10 Tf 100 700 Td (a\\001b\\002\\001) Tj ET";
        let source = Selecting {
            asked: std::cell::Cell::new(0),
        };
        let mut plain = TextDevice::new();
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut plain, &source);
        assert_eq!(plain.finish().plain_text(), "ab\n");
        assert_eq!(source.asked.get(), 0, "the default asks nothing");

        let mut code = falling_back(Unmapped::Code);
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut code, &source);
        assert_eq!(
            code.finish().plain_text(),
            "a\u{F0001}b\u{F0002}\u{F0001}\n"
        );
        assert_eq!(source.asked.get(), 0, "standing for its code asks nothing");

        let mut glyphs = falling_back(Unmapped::Glyph);
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut glyphs, &source);
        let read = glyphs.finish();
        assert_eq!(read.plain_text(), "a\u{100003}b\u{100006}\u{100003}\n");
        assert_eq!(source.asked.get(), 3, "once for each glyph with no text");
        assert_eq!(
            read.warnings,
            vec![
                TextWarning::UnmappedCode { code: 1 },
                TextWarning::UnmappedCode { code: 2 },
            ]
        );
    }

    /// A Type 3 glyph is a procedure, with no program glyph: under the glyph
    /// fallback one whose code maps to nothing stands as U+FFFD, and the
    /// fallback for its code is its code's.
    #[test]
    fn a_type3_glyph_has_no_glyph_id_to_fall_back_to() {
        let read = |mode: Unmapped| {
            let mut device = falling_back(mode);
            crate::interpret::interpret(
                b"BT /T3 10 Tf 100 700 Td (azb) Tj ET",
                Matrix::IDENTITY,
                &mut device,
                &Threes,
            );
            device.finish().plain_text()
        };
        assert_eq!(read(Unmapped::Drop), "Hi\n");
        assert_eq!(read(Unmapped::Glyph), "H\u{FFFD}i\n");
        assert_eq!(read(Unmapped::Code), "H\u{F007A}i\n");
    }

    /// A device reporting characters in `boxes`.
    fn boxed(boxes: CharBoxes) -> TextDevice {
        TextDevice::with_options(&ExtractionOptions {
            boxes,
            ..ExtractionOptions::default()
        })
    }

    /// `glyph` with `extent` read for it.
    fn with_extent(glyph: Glyph, extent: Option<GlyphExtent>) -> Glyph {
        Glyph {
            extent: extent.map(Box::new),
            ..glyph
        }
    }

    /// The quad a glyph of size 10 at `(x, 700)`, upright, has for the glyph
    /// space box `(x0, y0, x1, y1)`.
    fn upright(x: f64, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Quad {
        let at = |gx: f64, gy: f64| (x + 10.0 * gx, 700.0 + 10.0 * gy);
        Quad {
            ll: at(x0, y0),
            lr: at(x1, y0),
            ul: at(x0, y1),
            ur: at(x1, y1),
        }
    }

    /// Whether two quads are one, corner by corner.
    fn same_quad(a: &Quad, b: &Quad) -> bool {
        near(a.ll, b.ll) && near(a.lr, b.lr) && near(a.ul, b.ul) && near(a.ur, b.ur)
    }

    /// The one character a device in `boxes` reads from `glyph`.
    fn one_char(boxes: CharBoxes, glyph: &Glyph) -> TextChar {
        let mut d = boxed(boxes);
        d.show_glyph(glyph, &GraphicsState::new(Matrix::IDENTITY));
        let page = d.finish();
        let lines = page.lines();
        assert_eq!(lines.len(), 1, "{boxes:?}");
        lines[0].chars[0].clone()
    }

    /// **Each box option builds its quad from the glyph's extent**
    /// (`docs/design/extraction-options.md`, options 6 and 7): the em box by
    /// default; the outline's sides and the font's ascent and descent for
    /// the metrics; the outline's bounds for the outline. A glyph that draws
    /// nothing takes its advance, a font that states no ascent the em box's
    /// heights, and a glyph whose extent was not read the em box.
    #[test]
    fn each_box_option_builds_its_quad_from_the_glyphs_extent() {
        let extent = GlyphExtent {
            ascent_descent: Some((0.9, -0.25)),
            ink: Some((0.05, -0.1, 0.45, 0.7)),
        };
        // Half an em wide: an advance of 5 at a size of 10.
        let g = with_extent(glyph("g", 10.0, 700.0, 10.0), Some(extent));
        for (boxes, expected) in [
            (CharBoxes::EmBox, (0.0, -0.2, 0.5, 0.8)),
            (CharBoxes::FontMetrics, (0.05, -0.25, 0.45, 0.9)),
            (CharBoxes::Outline, (0.05, -0.1, 0.45, 0.7)),
        ] {
            let c = one_char(boxes, &g);
            assert!(
                same_quad(&c.quad, &upright(10.0, expected)),
                "{boxes:?}: {:?}",
                c.quad
            );
            assert!(near(c.origin, (10.0, 700.0)), "{boxes:?}: the origin stays");
        }

        // A space: no ink, so its advance; and a font with no ascent.
        let space = with_extent(
            glyph(" ", 10.0, 700.0, 10.0),
            Some(GlyphExtent {
                ascent_descent: None,
                ink: None,
            }),
        );
        let mut d = boxed(CharBoxes::Outline);
        let state = GraphicsState::new(Matrix::IDENTITY);
        d.show_glyph(
            &with_extent(glyph("a", 0.0, 700.0, 10.0), Some(extent)),
            &state,
        );
        d.show_glyph(&space, &state);
        let page = d.finish();
        let held = &page.lines()[0].chars[1];
        assert!(
            same_quad(&held.quad, &upright(10.0, (0.0, -0.2, 0.5, 0.8))),
            "a space in no font's metrics: {:?}",
            held.quad
        );
        let spaced = with_extent(
            glyph(" ", 10.0, 700.0, 10.0),
            Some(GlyphExtent {
                ascent_descent: Some((0.9, -0.25)),
                ink: None,
            }),
        );
        for boxes in [CharBoxes::FontMetrics, CharBoxes::Outline] {
            let mut d = boxed(boxes);
            d.show_glyph(
                &with_extent(glyph("a", 0.0, 700.0, 10.0), Some(extent)),
                &state,
            );
            d.show_glyph(&spaced, &state);
            let page = d.finish();
            let held = &page.lines()[0].chars[1];
            assert!(
                same_quad(&held.quad, &upright(10.0, (0.0, -0.25, 0.5, 0.9))),
                "{boxes:?}: a space across its advance: {:?}",
                held.quad
            );
        }

        // No extent read — a Type 3 glyph's — keeps the em box.
        for boxes in [CharBoxes::FontMetrics, CharBoxes::Outline] {
            let c = one_char(boxes, &glyph("g", 10.0, 700.0, 10.0));
            assert!(same_quad(&c.quad, &upright(10.0, (0.0, -0.2, 0.5, 0.8))));
        }
    }

    /// A vertical glyph keeps its em box for the metrics, since ascent and
    /// descent are horizontal metrics, and takes its outline for the outline.
    #[test]
    fn a_vertical_glyph_takes_its_outline_and_keeps_its_em_box_otherwise() {
        let mut v = glyph("v", 10.0, 700.0, 10.0);
        v.vertical = true;
        v.advance = -10.0;
        let ink = (0.1, -0.9, 0.8, -0.1);
        let v = with_extent(
            v,
            Some(GlyphExtent {
                ascent_descent: Some((0.9, -0.25)),
                ink: Some(ink),
            }),
        );
        let em = upright(10.0, (0.0, -0.2, 1.0, 0.8));
        assert!(same_quad(&one_char(CharBoxes::EmBox, &v).quad, &em));
        assert!(same_quad(&one_char(CharBoxes::FontMetrics, &v).quad, &em));
        assert!(same_quad(
            &one_char(CharBoxes::Outline, &v).quad,
            &upright(10.0, ink)
        ));
    }

    /// **The boxes are held until the page is read**: `finish_held` hands the
    /// page back in its em boxes with the boxes asked for beside it, and
    /// `apply` puts them in, each line's and block's quad made again from
    /// its characters'; under the em box nothing is held, and `apply` leaves
    /// the page to the bit; `finish` is the two at once.
    #[test]
    fn a_box_option_is_held_and_put_in_after() {
        let extent = GlyphExtent {
            ascent_descent: Some((0.9, -0.25)),
            ink: Some((0.1, 0.0, 0.4, 0.6)),
        };
        let glyphs = [
            with_extent(glyph("a", 0.0, 700.0, 10.0), Some(extent)),
            with_extent(glyph("b", 5.0, 700.0, 10.0), Some(extent)),
        ];
        let read = |boxes: CharBoxes| {
            let mut d = boxed(boxes);
            let state = GraphicsState::new(Matrix::IDENTITY);
            for g in &glyphs {
                d.show_glyph(g, &state);
            }
            d.finish_held()
        };
        let (em_page, em_held) = read(CharBoxes::EmBox);
        assert!(em_held.is_empty());
        let mut applied = em_page.clone();
        em_held.apply(&mut applied);
        assert_eq!(format!("{applied:?}"), format!("{em_page:?}"));

        let (page, held) = read(CharBoxes::Outline);
        assert!(!held.is_empty());
        assert_eq!(drawn_chars(&page), drawn_chars(&em_page), "held, not in");
        let mut outlined = page.clone();
        held.apply(&mut outlined);
        let chars: Vec<&TextChar> = outlined
            .lines()
            .into_iter()
            .flat_map(|l| l.chars.iter())
            .collect();
        assert!(same_quad(
            &chars[0].quad,
            &upright(0.0, (0.1, 0.0, 0.4, 0.6))
        ));
        assert!(same_quad(
            &chars[1].quad,
            &upright(5.0, (0.1, 0.0, 0.4, 0.6))
        ));
        let line = outlined.lines()[0];
        let (x0, y0, x1, y1) = line.quad.bounds();
        assert!(
            near((x0, y0), (1.0, 700.0)) && near((x1, y1), (9.0, 706.0)),
            "{:?}",
            line.quad
        );
        assert_eq!(outlined.blocks()[0].quad, line.quad);
        assert_eq!(chars[0].glyph, Some(0));
        assert_eq!(chars[1].glyph, Some(1));

        let mut d = boxed(CharBoxes::Outline);
        let state = GraphicsState::new(Matrix::IDENTITY);
        for g in &glyphs {
            d.show_glyph(g, &state);
        }
        assert_eq!(
            format!("{:?}", d.finish()),
            format!("{outlined:?}"),
            "finish is finish_held and apply"
        );
    }

    /// **Every decision reads the em box**: two lines whose em boxes are
    /// within a block's gap of each other and whose glyphs' ink is not are
    /// one block under every box option, and read the same text.
    #[test]
    fn a_box_option_changes_no_line_and_no_block() {
        // Ink a tenth of an em high, six tenths up: twenty-three points
        // between the two lines' ink, fourteen between their em boxes.
        let extent = GlyphExtent {
            ascent_descent: Some((0.7, -0.1)),
            ink: Some((0.1, 0.6, 0.4, 0.7)),
        };
        let glyphs = [
            with_extent(glyph("a", 0.0, 700.0, 10.0), Some(extent)),
            with_extent(glyph("b", 5.0, 700.0, 10.0), Some(extent)),
            with_extent(glyph("c", 0.0, 676.0, 10.0), Some(extent)),
            with_extent(glyph("d", 5.0, 676.0, 10.0), Some(extent)),
        ];
        let mut texts = Vec::new();
        for boxes in [CharBoxes::EmBox, CharBoxes::FontMetrics, CharBoxes::Outline] {
            let mut d = boxed(boxes);
            let state = GraphicsState::new(Matrix::IDENTITY);
            for g in &glyphs {
                d.show_glyph(g, &state);
            }
            let page = d.finish();
            assert_eq!(page.blocks().len(), 1, "{boxes:?}: one block");
            assert_eq!(page.lines().len(), 2, "{boxes:?}: two lines");
            texts.push(page.plain_text());
        }
        assert!(texts.iter().all(|text| text == "ab\ncd\n"), "{texts:?}");
    }

    /// An `/ActualText` character placed on its span's box rather than on a
    /// glyph takes the union of its span's held boxes; a character of no
    /// glyph and no span keeps its quad.
    #[test]
    fn a_stated_character_on_its_span_takes_its_spans_boxes() {
        let extent = GlyphExtent {
            ascent_descent: None,
            ink: Some((0.1, 0.0, 0.4, 0.6)),
        };
        let mut d = boxed(CharBoxes::Outline);
        let state = GraphicsState::new(Matrix::IDENTITY);
        let props = MarkedProps {
            actual_text: Some("xy".into()),
            ..MarkedProps::default()
        };
        d.begin_marked_content(b"Span", true, None, Some(&props));
        d.show_glyph(
            &with_extent(glyph("a", 0.0, 700.0, 10.0), Some(extent)),
            &state,
        );
        d.show_glyph(
            &with_extent(glyph("b", 5.0, 700.0, 10.0), Some(extent)),
            &state,
        );
        d.end_marked_content();
        let (mut page, held) = d.finish_held();
        let lone = Quad {
            ll: (50.0, 50.0),
            lr: (51.0, 50.0),
            ul: (50.0, 51.0),
            ur: (51.0, 51.0),
        };
        if let Some(line) = page
            .blocks_mut()
            .first_mut()
            .and_then(|b| b.lines.first_mut())
        {
            let mut stated = line.chars[0].clone();
            stated.glyph = None;
            stated.text = "x".into();
            let mut neither = stated.clone();
            neither.stated = None;
            neither.quad = lone;
            line.chars.push(stated);
            line.chars.push(neither);
        }
        held.apply(&mut page);
        let chars: Vec<&TextChar> = page
            .lines()
            .into_iter()
            .flat_map(|l| l.chars.iter())
            .collect();
        let (x0, y0, x1, y1) = chars[2].quad.bounds();
        assert!(
            near((x0, y0), (1.0, 700.0)) && near((x1, y1), (9.0, 706.0)),
            "{:?}",
            chars[2].quad
        );
        assert_eq!(chars[3].quad, lone);
    }

    /// A source that answers every glyph's extent and counts the questions.
    struct Measuring {
        fonts: std::cell::Cell<usize>,
        inks: std::cell::Cell<usize>,
    }

    impl crate::interpret::FontSource for Measuring {
        fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            crate::interpret::FontSource::decode(&Simple, font, bytes)
        }
        fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
            crate::interpret::FontSource::vertical_metrics(&Simple, font, code)
        }
        fn font_extent(&self, _font: &[u8]) -> Option<(f64, f64)> {
            self.fonts.set(self.fonts.get() + 1);
            Some((0.75, -0.25))
        }
        fn glyph_ink(&self, _font: &[u8], code: u32) -> Option<(f64, f64, f64, f64)> {
            self.inks.set(self.inks.get() + 1);
            (code != u32::from(b' ')).then_some((0.0, 0.0, 0.5, 0.5))
        }
    }

    /// **The interpreter asks for extents only where the device wants
    /// them**: the font's once a showing operator and the ink once a glyph,
    /// under a box option, and nothing by default.
    #[test]
    fn the_interpreter_asks_for_extents_only_where_the_device_wants_them() {
        let content = b"BT /F 10 Tf 100 700 Td (ab) Tj [(c) -100 ( d)] TJ ET";
        let source = Measuring {
            fonts: std::cell::Cell::new(0),
            inks: std::cell::Cell::new(0),
        };
        let mut plain = TextDevice::new();
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut plain, &source);
        assert_eq!((source.fonts.get(), source.inks.get()), (0, 0));
        let em = plain.finish();

        let mut outlined = boxed(CharBoxes::Outline);
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut outlined, &source);
        // `(ab)`, `(c)` and `( d)`: three strings shown, five glyphs.
        assert_eq!((source.fonts.get(), source.inks.get()), (3, 5));
        let outlined = outlined.finish();
        assert_eq!(outlined.plain_text(), em.plain_text());
        let quads: Vec<Quad> = drawn_chars(&outlined).into_iter().map(|(_, q)| q).collect();
        let at_a = &quads[0];
        assert!(
            near(at_a.ll, (100.0, 700.0)) && near(at_a.ur, (105.0, 705.0)),
            "{at_a:?}"
        );
    }

    /// A character of `text` at `x` along an upright baseline at 700, at a
    /// size of 10, `ems` wide, carrying a sequence, a stream, a font, a span
    /// and a glyph, so that a split is seen to keep them.
    fn drawn(text: &str, x: f64, ems: f64) -> TextChar {
        TextChar {
            text: text.into(),
            quad: upright(x, (0.0, -0.2, ems, 0.8)),
            size: 10.0,
            origin: (x, 700.0),
            mcid: Some(4),
            stream: 7,
            font: Some(std::sync::Arc::from("Lig")),
            stated: Some(2),
            artifact: false,
            glyph: Some(9),
        }
    }

    /// A page of one line of `chars`, in `wmode`, read left to right.
    fn line_of(chars: Vec<TextChar>, wmode: WritingMode) -> TextPage {
        let text: String = chars.iter().map(|c| c.text.as_str()).collect();
        let quad = enclosing(chars.iter().map(|c| c.quad)).expect("a box");
        TextPage::new(
            vec![TextBlock {
                lines: vec![TextLine {
                    chars,
                    text,
                    quad,
                    wmode,
                    rtl: false,
                    size: 10.0,
                }],
                quad,
            }],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// The options that split ligatures, or normalise white space, alone.
    fn character_options(split_ligatures: bool, normalize_whitespace: bool) -> ExtractionOptions {
        ExtractionOptions {
            split_ligatures,
            normalize_whitespace,
            ..ExtractionOptions::default()
        }
    }

    /// The text, the box's left and right and the origin's `x` of each of
    /// `page`'s characters.
    fn spans_of(page: &TextPage) -> Vec<(String, f64, f64, f64)> {
        page.lines()
            .iter()
            .flat_map(|line| line.chars.iter())
            .map(|c| {
                let (x0, _, x1, _) = c.quad.bounds();
                (c.text.clone(), x0, x1, c.origin.0)
            })
            .collect()
    }

    /// Whether two lists of characters' texts, boxes and origins are one, to
    /// a millionth of a point.
    fn same_spans(a: &[(String, f64, f64, f64)], b: &[(&str, f64, f64, f64)]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|((t, x0, x1, o), (u, y0, y1, p))| {
                t == u && near((*x0, *x1), (*y0, *y1)) && (o - p).abs() < 1e-6
            })
    }

    /// **A ligature splits into its letters, tiling its quad**
    /// (`docs/design/extraction-options.md`, option 1): a character whose
    /// text is `ffi` comes back as three, U+FB01 as two, each an equal share
    /// along the baseline in reading order, its origin at its share's start
    /// and every other field the character's; an Arabic and a Hebrew
    /// ligature form give their first letter the share at the right; a
    /// letter and its mark, and a mark opening a text, stay one piece.
    #[test]
    fn a_ligature_splits_into_its_letters_tiling_its_quad() {
        let mut page = line_of(
            vec![
                drawn("ffi", 0.0, 1.5),
                drawn("\u{FB01}", 20.0, 1.0),
                drawn("\u{FEFB}", 40.0, 1.0),
                drawn("\u{FB4F}", 60.0, 1.0),
                drawn("e\u{301}", 80.0, 0.5),
                drawn("\u{301}x", 90.0, 0.5),
            ],
            WritingMode::Horizontal,
        );
        apply_character_options(&mut page, &character_options(true, false));
        assert!(
            same_spans(
                &spans_of(&page),
                &[
                    ("f", 0.0, 5.0, 0.0),
                    ("f", 5.0, 10.0, 5.0),
                    ("i", 10.0, 15.0, 10.0),
                    ("f", 20.0, 25.0, 20.0),
                    ("i", 25.0, 30.0, 25.0),
                    ("\u{644}", 45.0, 50.0, 45.0),
                    ("\u{627}", 40.0, 45.0, 40.0),
                    ("\u{5D0}", 65.0, 70.0, 65.0),
                    ("\u{5DC}", 60.0, 65.0, 60.0),
                    ("e\u{301}", 80.0, 85.0, 80.0),
                    ("\u{301}x", 90.0, 95.0, 90.0),
                ]
            ),
            "{:?}",
            spans_of(&page)
        );
        let line = page.lines()[0];
        assert_eq!(
            line.text,
            "ffifi\u{644}\u{627}\u{5D0}\u{5DC}e\u{301}\u{301}x"
        );
        for c in &line.chars {
            assert_eq!(
                (
                    c.mcid,
                    c.stream,
                    c.font.as_deref(),
                    c.stated,
                    c.glyph,
                    c.size
                ),
                (Some(4), 7, Some("Lig"), Some(2), Some(9), 10.0),
                "{:?} keeps its fields",
                c.text
            );
            let (_, y0, _, y1) = c.quad.bounds();
            assert!(near((y0, y1), (698.0, 708.0)), "{:?}: its height", c.text);
        }
        assert_eq!(ligature_letters('\u{FB03}'), Some(&['f', 'f', 'i'][..]));
        assert_eq!(ligature_letters('f'), None);
        assert_eq!(
            ligature_letters('\u{FB2A}'),
            None,
            "a letter with its point"
        );
    }

    /// A vertical line's letters run down its column, the first at the top.
    #[test]
    fn a_vertical_ligature_splits_down_its_column() {
        let mut c = drawn("fl", 0.0, 1.0);
        c.quad = Quad {
            ul: (0.0, 720.0),
            ur: (10.0, 720.0),
            ll: (0.0, 700.0),
            lr: (10.0, 700.0),
        };
        c.origin = (5.0, 720.0);
        let mut page = line_of(vec![c], WritingMode::Vertical);
        apply_character_options(&mut page, &character_options(true, false));
        let chars = &page.lines()[0].chars;
        assert_eq!(chars.len(), 2);
        assert_eq!((chars[0].text.as_str(), chars[1].text.as_str()), ("f", "l"));
        assert!(near(chars[0].quad.ul, (0.0, 720.0)) && near(chars[0].quad.ll, (0.0, 710.0)));
        assert!(near(chars[1].quad.ul, (0.0, 710.0)) && near(chars[1].quad.lr, (10.0, 700.0)));
        assert!(near(chars[0].origin, (5.0, 720.0)) && near(chars[1].origin, (5.0, 710.0)));
    }

    /// **White space is normalised**: a tab, a no-break space and an
    /// ideographic space each read as one U+0020, an em space inside a text
    /// too, each character keeping its box; a zero-width space, which is no
    /// white space, is kept; and with the splits a ligature's white space
    /// reads so as well.
    #[test]
    fn white_space_is_normalised_one_for_one() {
        let fresh = || {
            line_of(
                vec![
                    drawn("a", 0.0, 0.5),
                    drawn("\t", 5.0, 0.5),
                    drawn("b", 10.0, 0.5),
                    drawn("\u{A0}", 15.0, 0.5),
                    drawn("c", 20.0, 0.5),
                    drawn("\u{3000}", 25.0, 1.0),
                    drawn("d\u{2003}e", 35.0, 1.0),
                    drawn("\u{200B}", 45.0, 0.5),
                ],
                WritingMode::Horizontal,
            )
        };
        let mut page = fresh();
        apply_character_options(&mut page, &character_options(false, true));
        assert_eq!(page.plain_text(), "a b c d e\u{200B}\n");
        let quads = |page: &TextPage| -> Vec<Quad> {
            page.lines()
                .iter()
                .flat_map(|line| line.chars.iter())
                .map(|c| c.quad)
                .collect()
        };
        assert_eq!(quads(&page), quads(&fresh()));

        let mut both = fresh();
        apply_character_options(&mut both, &character_options(true, true));
        assert_eq!(both.plain_text(), "a b c d e\u{200B}\n");
        assert_eq!(both.lines()[0].chars.len(), 10, "d, the space and e split");
    }

    /// **With both options off the page is left as it is**, to the bit;
    /// and a search after a split finds a match inside a ligature boxed
    /// inside it, where before it the match was the whole glyph or none.
    #[test]
    fn the_character_options_change_nothing_unasked_and_search_reads_the_letters() {
        let fresh = || {
            line_of(
                vec![drawn("ffi", 0.0, 1.5), drawn("\u{FB01}", 20.0, 1.0)],
                WritingMode::Horizontal,
            )
        };
        let mut kept = fresh();
        apply_character_options(&mut kept, &ExtractionOptions::default());
        assert_eq!(format!("{kept:?}"), format!("{:?}", fresh()));

        let before = fresh();
        assert_eq!(before.search("fi").len(), 1, "inside the ffi glyph only");
        let mut split = fresh();
        apply_character_options(&mut split, &character_options(true, false));
        let hits = split.search("fi");
        assert_eq!(hits.len(), 2, "{hits:?}");
        let (x0, _, x1, _) = hits[0].bounds();
        assert!(near((x0, x1), (5.0, 15.0)), "inside ffi: {x0}..{x1}");
        let (x0, _, x1, _) = hits[1].bounds();
        assert!(near((x0, x1), (20.0, 30.0)), "the fi ligature: {x0}..{x1}");
    }

    /// What an artifact scope draws is not the page's text (14.8.2.2), so a
    /// code nothing maps inside a dropped one is not said; a device that
    /// keeps artifacts reads it, and says it.
    #[test]
    fn an_unmapped_code_only_counts_where_its_glyph_would_be_read() {
        let state = GraphicsState::new(Matrix::IDENTITY);
        for keep in [false, true] {
            let mut d = if keep {
                TextDevice::keeping_artifacts()
            } else {
                TextDevice::new()
            };
            d.show_glyph(&glyph("a", 0.0, 700.0, 10.0), &state);
            d.begin_marked_content(b"Artifact", true, None, None);
            d.show_glyph(&unmapped(3, 10.0), &state);
            d.end_marked_content();
            let p = d.finish();
            let expected = if keep {
                vec![TextWarning::UnmappedCode { code: 3 }]
            } else {
                Vec::new()
            };
            assert_eq!(p.warnings, expected, "keeping artifacts: {keep}");
        }
    }

    /// Through the interpreter: a byte the font maps to nothing, between two
    /// it maps, is said once however often it is shown, and the text reads
    /// the two that map, as one line.
    #[test]
    fn an_unmapped_byte_of_a_shown_string_is_reported() {
        let page = extract_threes(b"BT /F 10 Tf 100 700 Td (a\\001\\001b) Tj (\\001) Tj ET");
        assert_eq!(page.plain_text(), "ab\n");
        assert_eq!(page.lines().len(), 1);
        assert_eq!(page.warnings, vec![TextWarning::UnmappedCode { code: 1 }]);
    }

    /// Every byte is one code, half an em wide, standing for itself.
    struct Simple;

    impl crate::interpret::FontSource for Simple {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    fn extract(content: &[u8]) -> TextPage {
        let mut device = TextDevice::new();
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut device, &Simple);
        device.finish()
    }

    /// **A rise is not a line break**, and one raised glyph between two
    /// ordinary ones is the case that says so.
    ///
    /// `A`, then `B` twelve units up, then `C` back on the baseline, at a size
    /// of ten — so the rise is more than half an em and a build that asked
    /// where the *ink* was would split the line at `B` and again at `C`, since
    /// the line's remembered origin is the previous glyph's. Three lines out
    /// of one sentence, and the sentence is a footnote marker in the middle of
    /// a paragraph or a `GPOS` mark on its base.
    ///
    /// The glyphs are counted as well as the lines, because a build that
    /// dropped the raised glyph altogether would also produce one line.
    #[test]
    fn a_rise_does_not_start_a_new_line() {
        let page = extract(b"BT /F0 10 Tf 0 700 Td (A) Tj 12 Ts (B) Tj 0 Ts (C) Tj ET");
        let lines = page.lines();
        assert_eq!(
            lines.len(),
            1,
            "a superscript split the line: {:?}",
            page.plain_text()
        );
        assert_eq!(page.plain_text(), "ABC\n");
    }

    /// And the geometry a caller reads back is still the **risen** one.
    ///
    /// The baseline is used to decide line membership and for nothing else: a
    /// selection rectangle drawn on the baseline would not cover the
    /// superscript it is selecting, so `TextChar::origin` and the quad keep
    /// where the ink is.
    #[test]
    fn a_risen_glyph_keeps_the_position_it_was_drawn_at() {
        let page = extract(b"BT /F0 10 Tf 0 700 Td (A) Tj 12 Ts (B) Tj 0 Ts (C) Tj ET");
        let line = page.lines().first().copied().expect("one line");
        let raised = line
            .chars
            .iter()
            .find(|c| c.text == "B")
            .expect("the raised glyph");
        assert!(
            (raised.origin.1 - 712.0).abs() < 1e-9,
            "the raised glyph moved to {:?}",
            raised.origin
        );
        let (_, y0, _, _) = raised.quad.bounds();
        assert!(y0 > 700.0, "its quad is on the baseline: {y0}");
    }

    /// A scope with one ordinary font, `/F`, every byte of which stands for
    /// itself but `0x01`, which stands for nothing, and one Type 3 font,
    /// `/T3`, whose codes each test a way a glyph procedure can go:
    ///
    /// - `a` and `b` stand for `H` and `i`, a full em and half an em wide,
    ///   and fill a square;
    /// - `s` stands for `S` and shows `x` in `/F`;
    /// - `u` stands for `U` and shows `/F`'s code that stands for nothing;
    /// - `z` stands for nothing, takes no width, and fills;
    /// - `r` stands for `R` and shows itself, with no identity to be taken
    ///   for a cycle by, so it runs until the depth cap stops it.
    struct Threes;

    impl crate::interpret::FontSource for Threes {
        fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| {
                    let (text, width) = match (font, b) {
                        (b"T3", b'a') => ("H".to_string(), 1000.0),
                        (b"T3", b'b') => ("i".to_string(), 500.0),
                        (b"T3", b's') => ("S".to_string(), 1000.0),
                        (b"T3", b'u') => ("U".to_string(), 1000.0),
                        (b"T3", b'r') => ("R".to_string(), 1000.0),
                        (b"T3", _) => (String::new(), 0.0),
                        (_, 1) => (String::new(), 600.0),
                        _ => (char::from(b).to_string(), 600.0),
                    };
                    (u32::from(b), text, width)
                })
                .collect()
        }

        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }

        fn type3_glyph(&self, font: &[u8], code: u32) -> Option<crate::interpret::Type3Glyph> {
            if font != b"T3" {
                return None;
            }
            let content: &[u8] = match u8::try_from(code).ok()? {
                b's' => b"1000 0 d0 BT /F 1000 Tf (x) Tj ET",
                b'u' => b"1000 0 d0 BT /F 1000 Tf <01> Tj ET",
                b'r' => b"1000 0 d0 0 0 1000 500 re f BT /T3 1000 Tf (r) Tj ET",
                _ => b"1000 0 d0 0 0 1000 1000 re f",
            };
            Some(crate::interpret::Type3Glyph {
                content: content.to_vec(),
                matrix: Matrix::scale(0.001, 0.001),
                stream: 0,
            })
        }
    }

    fn extract_threes(content: &[u8]) -> TextPage {
        let mut device = TextDevice::new();
        crate::interpret::interpret(content, Matrix::IDENTITY, &mut device, &Threes);
        device.finish()
    }

    fn near(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    /// 9.6.4: a Type 3 glyph's text is read where the glyph is placed — the
    /// text matrix and the size, the font matrix taking its width to text
    /// space — though nothing of the glyph but its procedure's square is
    /// drawn.
    #[test]
    fn a_type3_glyph_is_read_where_it_is_placed() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (ab) Tj ET");
        assert_eq!(page.plain_text(), "Hi\n");
        let line = page.lines().first().copied().expect("one line");
        assert_eq!(line.chars.len(), 2);
        let (h, i) = (&line.chars[0], &line.chars[1]);
        assert!(near(h.origin, (100.0, 700.0)), "{:?}", h.origin);
        // A full em of a ten-unit size, through the font matrix's 0.001.
        assert!(near(i.origin, (110.0, 700.0)), "{:?}", i.origin);
        assert!(near(h.quad.ll, (100.0, 698.0)), "{:?}", h.quad);
        assert!(near(h.quad.ur, (110.0, 708.0)), "{:?}", h.quad);
        // Half an em wide.
        assert!(near(i.quad.lr, (115.0, 698.0)), "{:?}", i.quad);
        assert_eq!(page.search("hi").len(), 1, "and it is found");
    }

    /// A procedure that draws its glyph with text of its own has handed that
    /// text over already, so the glyph's own is not read again: `s` reads as
    /// the `x` its procedure shows, once.
    #[test]
    fn a_procedure_that_shows_text_is_read_once() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (s) Tj ET");
        assert_eq!(page.plain_text(), "x\n");
    }

    /// But text that stands for nothing is no text: `u`'s procedure shows a
    /// code that maps to nothing, and the glyph reads as its own `U`.
    #[test]
    fn a_procedure_whose_own_text_maps_nothing_is_read_as_its_glyph() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (u) Tj ET");
        assert_eq!(page.plain_text(), "U\n");
    }

    /// A code that maps to nothing adds nothing, as a shown glyph's does, and
    /// the glyphs either side of it stay one line.
    #[test]
    fn a_type3_code_that_maps_nothing_adds_nothing() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (azb) Tj ET");
        assert_eq!(page.plain_text(), "Hi\n");
        assert!(extract_threes(b"BT /T3 10 Tf 100 700 Td (zz) Tj ET")
            .plain_text()
            .is_empty());
    }

    /// And a Type 3 code that maps to nothing is said, as a shown glyph's is;
    /// a procedure that shows a code its own font maps to nothing says that
    /// code, and its glyph still reads as its own `U`.
    #[test]
    fn a_type3_code_that_maps_nothing_is_reported() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (azbz) Tj ET");
        assert_eq!(page.plain_text(), "Hi\n");
        assert_eq!(
            page.warnings,
            vec![TextWarning::UnmappedCode {
                code: u32::from(b'z')
            }]
        );
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (u) Tj ET");
        assert_eq!(page.plain_text(), "U\n");
        assert_eq!(page.warnings, vec![TextWarning::UnmappedCode { code: 1 }]);
        assert!(extract_threes(b"BT /T3 10 Tf 100 700 Td (ab) Tj ET")
            .warnings
            .is_empty());
    }

    /// A procedure passed over — here at the depth cap, the innermost of the
    /// sixteen runs of a glyph that shows itself — is a drawing's limit and
    /// not the text's: the innermost glyph is read, and every glyph around it
    /// has that text as its procedure's own, so `R` is read once.
    #[test]
    fn a_glyph_passed_over_at_the_depth_cap_is_read_once() {
        let page = extract_threes(b"BT /T3 10 Tf 100 700 Td (r) Tj ET");
        assert_eq!(page.plain_text(), "R\n");
    }

    /// A Type 3 glyph's text is the page's as any glyph's is: inside an
    /// artifact it is dropped (14.8.2.2), and beside an ordinary font's
    /// glyphs it joins their line.
    #[test]
    fn a_type3_glyph_reads_as_any_glyph_reads() {
        assert!(
            extract_threes(b"/Artifact BMC BT /T3 10 Tf 100 700 Td (ab) Tj ET EMC")
                .plain_text()
                .is_empty()
        );
        let page = extract_threes(b"BT /F 10 Tf 100 700 Td (x) Tj /T3 10 Tf (ab) Tj ET");
        assert_eq!(page.plain_text(), "xHi\n");
        assert_eq!(page.lines().len(), 1);
    }

    /// Every drawing device leaves the report alone: a recording of a Type 3
    /// run is the procedure's paints and nothing else, the same calls in the
    /// same order whatever the glyph's codes stand for.
    #[test]
    fn a_recording_keeps_none_of_a_type3_glyphs_text() {
        use crate::record::{EventKind, RecordingDevice};
        let kinds = |content: &[u8]| {
            let mut device = RecordingDevice::new();
            crate::interpret::interpret(content, Matrix::IDENTITY, &mut device, &Threes);
            device.kinds()
        };
        let mapped = kinds(b"BT /T3 10 Tf 100 700 Td (ab) Tj ET");
        let unmapped = kinds(b"BT /T3 10 Tf 100 700 Td (zz) Tj ET");
        assert_eq!(mapped, unmapped, "the text a code stands for draws nothing");
        assert_eq!(
            mapped
                .iter()
                .filter(|kind| **kind == EventKind::FillPath)
                .count(),
            2,
            "each glyph's square: {mapped:?}"
        );
        assert!(
            !mapped.contains(&EventKind::ShowGlyph),
            "a Type 3 glyph is run, not shown: {mapped:?}"
        );
    }
}
