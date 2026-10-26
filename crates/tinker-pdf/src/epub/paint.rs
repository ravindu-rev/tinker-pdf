//! Measurement, encoding and the page a laid-out book is drawn on (gap 31,
//! milestones 8 and 9).
//!
//! Two things live here because they are one decision seen from either end:
//! **which face a run is measured with** and **which font resource it is drawn
//! with** have to be the same answer, and a build with two of them sets a page
//! whose glyphs do not fit the boxes it computed. [`choose`] is that one
//! answer, and [`BookMetrics`] and [`draw_page`] both go through it.
//!
//! # The matching is per character, not per run
//!
//! `css-fonts-4` §5.3 makes font matching a **per-character** algorithm: the
//! `font-family` list is walked for each character in turn and the first family
//! with a face that *has a glyph for that character* wins. A build that
//! resolved the list once per run — which milestone 8's did, and said so —
//! draws a run in one face and leaves a notdef wherever that face is short,
//! which is the difference between fallback and a face with holes in it.
//!
//! The consequence is visible in the content stream rather than only in the
//! picture. A PDF string is bytes in **one** font, so a run needing three faces
//! is three `BT … ET` text objects at three origins, each starting where the
//! one before it left off. `a_run_needing_three_faces_becomes_three_text_objects`
//! is what says so, and it asserts the count rather than the appearance because
//! a single face with two notdefs also looks like something.
//!
//! # The metrics are the face's own, whichever face that is
//!
//! For an embedded face the advances come from the file's own `hmtx` and the
//! line height from its `hhea`. For the standard 14 they are Adobe's published
//! AFM numbers, which `tinker-pdf-font` already holds because a PDF may omit
//! `/Widths` for those faces. **Both are real numbers and neither is a
//! placeholder**, which is what makes the pagination the pagination a reader
//! will see — and what makes [`BookMetrics::STANDARD`] a complete answer for a
//! book with no `@font-face` in it rather than a fallback that happens to
//! produce a page.
//!
//! That is also the answer to milestone 4's question. `OpenOptions::fonts` may
//! be `None` and the page count is the same either way, because nothing in this
//! file asks whether a [`crate::FontProvider`] is attached;
//! `the_page_count_does_not_depend_on_whether_a_provider_is_attached` holds it.
//!
//! # A character no face covers
//!
//! Milestone 1's corpus contains a line of Japanese in five of its six books,
//! placed there so that a space-only line breaker would be caught. It catches
//! something else too: a simple font maps one byte to one glyph, and 25 kanji
//! and kana are not in Windows code page 1252.
//!
//! So a character outside the encoding that **no available face covers** is
//! given a code in an **overflow font** — the same base face under an
//! `/Encoding` whose `/Differences` names the glyph by the Adobe Glyph List's
//! algorithmic `uniXXXX` form. 9.10.2's second step resolves that back to the
//! character, so the text extracts correctly; the standard face has no such
//! glyph, so the page shows a notdef. **That asymmetry is stated rather than
//! hidden**: [`Fonts::uncovered`] counts every character drawn that way and the
//! caller warns by name, which is the gap milestone 8 recorded as owed.
//!
//! A character an embedded face *does* cover never reaches the overflow font at
//! all, so a book that brings its own CJK face has neither the notdef nor the
//! 224-code ceiling. That ceiling is `/Differences`'s own size rather than a
//! cap invented here — the array is allocated at that size whatever the input
//! says, so it is not a bound in ruling 1's sense and does not join
//! `bounds_ledger.rs`.
//!
//! # The CID-keyed fallback, where this build carries a face
//!
//! With `bundled-fonts` on, a character outside `WinAnsiEncoding` that the
//! standard face's Liberation stand-in covers is drawn in **that** face,
//! embedded as a composite font under `/Identity-H` ([`Fonts::register`]):
//! the code is the glyph index, so there are as many codes as the face has
//! glyphs rather than 224, the page shows the real glyph rather than a notdef,
//! and `/ToUnicode` carries the character. Liberation is what every reader
//! substitutes for Times, Helvetica and Courier anyway, metric-compatible with
//! them over the Latin set, so the line a book was set in does not change face
//! where it leaves the encoding. Its advance is the face's own `hmtx`, in
//! [`BookMetrics`] as on the page — one path owns the run. A character the
//! stand-in does not cover either (the Japanese line above) still goes to the
//! overflow font and is still counted. Without the feature nothing changes:
//! the standard-14 limit stays, and [`Fonts::unrepresented`] counts what it
//! costs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use tinker_pdf_cos::build::{
    DeviceSpace, DocumentBuilder, ExtGState, Function, Glyph, PageBuilder, PieceText, Shading,
    Target, TextPiece, TilingPattern, TilingType,
};
use tinker_pdf_css::cascade::StyleTree;

use tinker_pdf_css::property::{
    BackgroundSize, BorderStyle, Color, ColorStop, ComputedOffset, Direction as CssDirection,
    Display, FeatureSetting, Float, FontFamily, FontKerning, FontStyle, Gradient, GradientOffset,
    GradientShape, Image, ImageRef, LengthPercentage, LinearDirection, Position, RadialSize,
    RepeatStyle, Side, TextDecoration, Transform, TransformOrigin, UnicodeBidi,
};
use tinker_pdf_font::base14::Standard14;
use tinker_pdf_font::encoding::{base_char, glyph_name_for_char, glyph_name_to_char, BaseEncoding};
use tinker_pdf_font::Sfnt;
use tinker_pdf_layout::limits::MAX_EMBEDDING_DEPTH;
use tinker_pdf_layout::metrics::{
    FirstStrong, FontRequest, Metrics, Neighbour, PlacedGlyph, ShapedText, Shaper, ShapingContext,
    Vertical, CONTEXT_BYTES,
};
use tinker_pdf_layout::style::{consume, Consumed};
use tinker_pdf_layout::{
    BackgroundLayer, BoxFragment, BoxNode, ClipFragment, Content, Embedding, EmbeddingKind,
    Page as LayoutPage, ReplacedFragment, TextRun,
};
use tinker_pdf_shape::bidi::{
    drawn_direction, order_units, reorder, BaseDirection, Level, Paragraph,
};
use tinker_pdf_shape::shape::itemize;
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};
use tinker_pdf_shape::Tag;
use tinker_pdf_svg::transform::{concat, invert, rotation, IDENTITY};

use super::read::PX_TO_PT;
use super::tagging::{ancestry, draw_figure, figure_orders, table_cells, tag_runs, Tagging};
use super::typeface::FaceSet;
use super::xhtml::Dom;
// `Placed` and not `tinker_pdf_layout::metrics::PlacedGlyph`, which is
// imported above under that name and is a different thing: layout's is a
// measurement and this one is a position on a page.
use crate::shaping::Placed;

/// How many codes one overflow font holds: 32 through 255.
///
/// `/Differences` may start at any code and a simple font has 256 of them;
/// starting at 32 keeps every code out of the range a PDF lexer would have to
/// escape twice and leaves the largest contiguous run available.
pub const OVERFLOW_CODES: usize = 224;

/// The first code an overflow font uses.
pub const OVERFLOW_FIRST: u8 = 32;

/// Which of the three generic families a run resolved to.
///
/// `cursive` and `fantasy` are `css-fonts-4` generic families this build has
/// no face for, and they resolve to `serif` — **which is what a reading system
/// with no such face does**, and is the one place here where a value is mapped
/// onto a neighbour. It is recorded rather than silent: the resolution is a
/// property of having only the standard 14, and a book that embeds a face
/// under a family name it also lists gets that face instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Generic {
    /// `serif`, and the initial value.
    Serif,
    /// `sans-serif`.
    SansSerif,
    /// `monospace`.
    Monospace,
}

/// One of the twelve text faces of the standard 14.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Face {
    /// The generic family.
    pub generic: Generic,
    /// `font-weight` at or above 600, which is `css-fonts-4` §2.2's own
    /// threshold for a face that has only two weights.
    pub bold: bool,
    /// `font-style` other than `normal`. Times has an italic and Helvetica an
    /// oblique, and `css-fonts-4` §5.2 makes either an acceptable match for
    /// the other.
    pub italic: bool,
}

/// Which face one character is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// One of the book's own `@font-face` faces, by index into
    /// [`FaceSet::faces`].
    Embedded(usize),
    /// One of the standard 14.
    Standard(Face),
}

/// Whether the standard 14 can draw a character.
///
/// The `WinAnsiEncoding` repertoire, exactly — which is not an approximation of
/// the Adobe Standard Latin set but the set a simple font under that encoding
/// can *address*, and therefore the set this build can put on a page through
/// one. A character outside it has no code in the primary font and no glyph in
/// the face, which are the same fact seen twice.
#[must_use]
pub fn standard_covers(ch: char) -> bool {
    winansi_code(ch).is_some()
}

/// `css-fonts-4` §5's matching algorithm, for one character.
///
/// The family list is walked **in the author's order** and each family is asked
/// twice: for one of the book's own `@font-face` faces, and then for a standard
/// face this build substitutes for the name. The first that can draw `ch` wins.
///
/// `ch` of `None` asks the same question ignoring coverage, which is what a
/// **line height** needs: leading is a property of a run's nominal face and
/// must not change part way along because one character fell through to
/// another face. A build that used the per-character answer for `vertical` too
/// would give a paragraph a different line height depending on where its
/// accented characters happened to fall.
///
/// When the list is exhausted, §5.3's system fallback is the book's own faces —
/// the only ones a reading system with no installed fonts has — and after those
/// the initial `serif` face, which may not cover `ch` at all. The caller is
/// what says so: [`Fonts::note`] counts a character that arrives here
/// uncovered, and the reader warns by name rather than leaving a blank.
#[must_use]
pub fn choose(faces: &FaceSet, font: &FontRequest<'_>, ch: Option<char>) -> Chosen {
    let bold = font.weight >= 600;
    let italic = font.style != FontStyle::Normal;
    for family in font.families {
        if let FontFamily::Named(name) = family {
            if let Some(index) = faces.best(name, font.weight, font.style, ch) {
                return Chosen::Embedded(index);
            }
        }
        let generic = match family {
            FontFamily::Serif | FontFamily::Cursive | FontFamily::Fantasy => Generic::Serif,
            FontFamily::SansSerif => Generic::SansSerif,
            FontFamily::Monospace => Generic::Monospace,
            FontFamily::Named(name) => match Standard14::from_base_font(name) {
                Some(Standard14::Courier) => Generic::Monospace,
                Some(Standard14::Helvetica | Standard14::HelveticaBold) => Generic::SansSerif,
                Some(
                    Standard14::TimesRoman
                    | Standard14::TimesBold
                    | Standard14::TimesItalic
                    | Standard14::TimesBoldItalic,
                ) => Generic::Serif,
                // Symbol and ZapfDingbats are not text faces, and a family
                // this build has never heard of is not a match at all:
                // §5's algorithm moves to the next entry rather than
                // stopping, which is what makes `Georgia, serif` fall
                // through to `serif` when `Georgia` is absent.
                _ => continue,
            },
        };
        if ch.is_none_or(standard_covers) {
            return Chosen::Standard(Face {
                generic,
                bold,
                italic,
            });
        }
    }
    // §5.3's last resort. The book's own faces are tried before giving up,
    // because they are the only faces this reading system has that the standard
    // 14 are not — a book that ships a CJK face under a family its `body` rule
    // never mentions still gets its Japanese line drawn.
    if let Some(ch) = ch {
        if let Some(index) = faces.any_covering(ch) {
            return Chosen::Embedded(index);
        }
    }
    Chosen::Standard(Face {
        generic: Generic::Serif,
        bold,
        italic,
    })
}

/// The maximal stretches of `text` that [`choose`] answers the same for, as
/// byte ranges.
///
/// # Why fallback is resolved before shaping and not after
///
/// Font fallback is per **character** (`css-fonts-4` §5.3) and shaping is per
/// **run of one face** (ISO/IEC 14496-22): a `GSUB` rule names glyph indices,
/// and a glyph index means nothing outside the face it came from. So a
/// paragraph whose characters need three faces is three shaped runs, and a
/// build that shaped it as one — through whichever face its first character
/// happened to resolve to — would ask two faces' worth of characters of a face
/// that has neither and get `.notdef` for both.
///
/// It is also what keeps the measured advance and the drawn advance the same
/// number: [`BookMetrics::shape`] and [`draw_run`] walk **these** segments, so
/// the one-path-owns-a-run rule `tinker-pdf-layout`'s `metrics.rs` states holds
/// across the seam rather than only inside it.
#[must_use]
pub fn face_runs(
    faces: &FaceSet,
    font: &FontRequest<'_>,
    text: &str,
) -> Vec<(core::ops::Range<usize>, Chosen)> {
    let mut out: Vec<(core::ops::Range<usize>, Chosen)> = Vec::new();
    for (at, ch) in text.char_indices() {
        let chosen = choose(faces, font, Some(ch));
        let end = at + ch.len_utf8();
        match out.last_mut() {
            Some((range, last)) if *last == chosen => range.end = end,
            _ => out.push((at..end, chosen)),
        }
    }
    out
}

impl Face {
    /// Which of the standard 14 a request resolves to, with no embedded face
    /// in the question.
    ///
    /// [`choose`] with an empty [`FaceSet`] and no character, which is what
    /// makes this one reading of §5 rather than a second: a build with the
    /// family walk written out twice would eventually have one copy learn
    /// something the other did not.
    #[must_use]
    pub fn of(font: &FontRequest<'_>) -> Face {
        match choose(&FaceSet::new(), font, None) {
            Chosen::Standard(face) => face,
            // Unreachable: an empty face set has no embedded face to return.
            Chosen::Embedded(_) => Face {
                generic: Generic::Serif,
                bold: font.weight >= 600,
                italic: font.style != FontStyle::Normal,
            },
        }
    }

    /// Every face, in a stable order, so the resource names a document uses do
    /// not depend on the order a book happened to need them in.
    #[must_use]
    pub fn all() -> Vec<Face> {
        let mut out = Vec::with_capacity(12);
        for generic in [Generic::Serif, Generic::SansSerif, Generic::Monospace] {
            for bold in [false, true] {
                for italic in [false, true] {
                    out.push(Face {
                        generic,
                        bold,
                        italic,
                    });
                }
            }
        }
        out
    }

    /// This face's index among [`Face::all`].
    #[must_use]
    pub fn index(self) -> usize {
        let generic = match self.generic {
            Generic::Serif => 0,
            Generic::SansSerif => 1,
            Generic::Monospace => 2,
        };
        generic * 4 + usize::from(self.bold) * 2 + usize::from(self.italic)
    }

    /// The `/BaseFont` name, from Annex D.1's table.
    #[must_use]
    pub fn base_font(self) -> &'static [u8] {
        match (self.generic, self.bold, self.italic) {
            (Generic::Serif, false, false) => b"Times-Roman",
            (Generic::Serif, false, true) => b"Times-Italic",
            (Generic::Serif, true, false) => b"Times-Bold",
            (Generic::Serif, true, true) => b"Times-BoldItalic",
            (Generic::SansSerif, false, false) => b"Helvetica",
            (Generic::SansSerif, false, true) => b"Helvetica-Oblique",
            (Generic::SansSerif, true, false) => b"Helvetica-Bold",
            (Generic::SansSerif, true, true) => b"Helvetica-BoldOblique",
            (Generic::Monospace, false, false) => b"Courier",
            (Generic::Monospace, false, true) => b"Courier-Oblique",
            (Generic::Monospace, true, false) => b"Courier-Bold",
            (Generic::Monospace, true, true) => b"Courier-BoldOblique",
        }
    }

    /// The face whose **advances** this one shares.
    ///
    /// Helvetica and Helvetica-Oblique publish the same widths and so do the
    /// four Couriers, which is why [`Standard14`] has nine variants for twelve
    /// faces rather than twelve.
    #[must_use]
    pub fn standard(self) -> Standard14 {
        match (self.generic, self.bold, self.italic) {
            (Generic::Serif, false, false) => Standard14::TimesRoman,
            (Generic::Serif, false, true) => Standard14::TimesItalic,
            (Generic::Serif, true, false) => Standard14::TimesBold,
            (Generic::Serif, true, true) => Standard14::TimesBoldItalic,
            (Generic::SansSerif, false, _) => Standard14::Helvetica,
            (Generic::SansSerif, true, _) => Standard14::HelveticaBold,
            (Generic::Monospace, _, _) => Standard14::Courier,
        }
    }

    /// The ascent and descent, as fractions of the em.
    ///
    /// Adobe's published AFM `Ascender` and `Descender` for the three families,
    /// with the descender's sign flipped because
    /// [`tinker_pdf_layout::metrics::Vertical`] measures a depth below the
    /// baseline as a positive number — a provider that returned the sfnt's own
    /// convention and one that returned the absolute value would both look
    /// plausible and one of them would set every line on top of the next.
    #[must_use]
    pub fn vertical_fractions(self) -> (f64, f64) {
        match self.generic {
            Generic::Serif => (0.683, 0.217),
            Generic::SansSerif => (0.718, 0.207),
            Generic::Monospace => (0.629, 0.157),
        }
    }

    /// The resource name the primary, `WinAnsiEncoding` font is registered
    /// under.
    #[must_use]
    pub fn resource(self) -> Vec<u8> {
        format!("Bk{}", self.index()).into_bytes()
    }

    /// The resource name of this face's overflow font.
    #[must_use]
    pub fn overflow_resource(self) -> Vec<u8> {
        format!("Bx{}", self.index()).into_bytes()
    }

    /// The resource name of this face's CID-keyed fallback: its bundled
    /// stand-in, embedded as a composite font.
    #[must_use]
    pub fn fallback_resource(self) -> Vec<u8> {
        format!("By{}", self.index()).into_bytes()
    }

    /// The `/BaseFont` the fallback is written under: the stand-in's own
    /// PostScript name, which is what it is.
    #[must_use]
    pub fn fallback_base_font(self) -> &'static [u8] {
        match (self.generic, self.bold, self.italic) {
            (Generic::Serif, false, false) => b"LiberationSerif",
            (Generic::Serif, false, true) => b"LiberationSerif-Italic",
            (Generic::Serif, true, false) => b"LiberationSerif-Bold",
            (Generic::Serif, true, true) => b"LiberationSerif-BoldItalic",
            (Generic::SansSerif, false, false) => b"LiberationSans",
            (Generic::SansSerif, false, true) => b"LiberationSans-Italic",
            (Generic::SansSerif, true, false) => b"LiberationSans-Bold",
            (Generic::SansSerif, true, true) => b"LiberationSans-BoldItalic",
            (Generic::Monospace, false, false) => b"LiberationMono",
            (Generic::Monospace, false, true) => b"LiberationMono-Italic",
            (Generic::Monospace, true, false) => b"LiberationMono-Bold",
            (Generic::Monospace, true, true) => b"LiberationMono-BoldItalic",
        }
    }
}

/// The character a code stands for in `WinAnsiEncoding`, backwards.
///
/// Three ranges rather than a table, because `tinker-pdf-font`'s own table for
/// 0x80–0x9F is private and duplicating it here would be two tables that could
/// disagree. Below 0x80 and at or above 0xA0 the encoding **is** Latin-1 by
/// construction, and the thirty-two codes in between are found by asking the
/// one table there is.
#[must_use]
pub fn winansi_code(c: char) -> Option<u8> {
    let code = u32::from(c);
    if code < 0x80 {
        return u8::try_from(code).ok();
    }
    if (0xA0..=0xFF).contains(&code) {
        return u8::try_from(code).ok();
    }
    (0x80..=0x9F).find(|code| base_char(BaseEncoding::WinAnsi, *code) == Some(c))
}

/// The bundled stand-in for one of the standard faces, where this build
/// carries one.
#[cfg(feature = "bundled-fonts")]
fn fallback_program(face: Face) -> Option<&'static [u8]> {
    use tinker_pdf_font::bundled::{self, Family};
    let family = match face.generic {
        Generic::Serif => Family::Serif,
        Generic::SansSerif => Family::Sans,
        Generic::Monospace => Family::Mono,
    };
    Some(bundled::face(family, face.bold, face.italic))
}

/// The bundled stand-in for one of the standard faces: none, in a build
/// without `bundled-fonts`, which is what keeps the standard-14 limit there.
#[cfg(not(feature = "bundled-fonts"))]
fn fallback_program(_face: Face) -> Option<&'static [u8]> {
    None
}

/// The glyph, and its advance in ems, that `face`'s bundled stand-in draws
/// `ch` with — for a character **outside** `WinAnsiEncoding` only, which is
/// the one case the fallback exists for. `None` where there is no stand-in,
/// where it has no glyph for `ch`, and for every character the simple font
/// already has a code for, so the Latin text of a book is never moved off
/// the standard face it was always set in.
fn fallback_glyph(face: Face, ch: char) -> Option<(u16, f64)> {
    if winansi_code(ch).is_some() {
        return None;
    }
    let sfnt = Sfnt::parse(fallback_program(face)?)?;
    let glyph = sfnt.glyph_for_char(ch).filter(|g| *g != 0)?;
    let advance = f64::from(sfnt.advance(glyph)?) / f64::from(sfnt.units_per_em.max(1));
    Some((glyph, advance))
}

/// How wide `face` sets `ch` where no stand-in draws it — through the simple
/// font's own encoding or through its overflow font — in thousandths of an
/// em. The one number for both ends: [`BookMetrics::advance`] measures with
/// it and [`Fonts::register`] writes the overflow font's `/Widths` from it,
/// so the pen layout moved and the pen a reader moves agree.
///
/// - **A nonspacing mark (`Bidi_Class` `NSM`) is zero.** It is the kind of
///   character that has no advance of its own, and the standard 14 have no
///   glyph for one to say otherwise; [`Standard14::advance`] answers with a
///   space's width for any character its table lacks, and a Hebrew point or
///   an Arabic haraka measured that way was a glyph as wide as a letter,
///   drawn after its letter. Extraction pairs a mark with the base it sits
///   on, and a letter's second such mark sat past its letter's box, nearer
///   the glyph drawn next, and was read with it:
///   `sample-regime-anticancer-arabic.epub`'s `يًّا` came back with its
///   FATHATAN on the letter before (CI run 38041540464). Zero, it is drawn
///   on its letter ([`draw_coded`]). The class is the one [`mark_clusters`]
///   and ruling 14's pairing both test, so a character measured as a mark
///   is drawn and read as one. Zero comes first: a combining kana voiced
///   sound mark is `W` to UAX #11 and a mark all the same.
/// - **An East Asian character is one em**, in every face that has one, and
///   the standard 14 have none at all — so the number cannot come from
///   `Standard14`, which would set a Japanese line at a third of its width.
///   UAX #11's own classification is what decides, through the table
///   `tinker-pdf-layout` already vendors for UAX #14.
/// - **Anything else is `Standard14`'s**: Adobe's published advance, or its
///   base letter's, or a space's.
///
/// Not [`Standard14::advance`] itself, which a PDF *read* without `/Widths`
/// also asks: a leaf crate has no `Bidi_Class` table, and whether a document
/// that names a standard face and maps a code to a mark meant it to advance
/// is that document's to say, not this painter's.
fn standard_width(face: Face, ch: char) -> f64 {
    if bidi_class(ch) == BidiClass::NSM {
        return 0.0;
    }
    if tinker_pdf_layout::unicode::is_east_asian(ch) {
        return 1000.0;
    }
    f64::from(face.standard().advance(ch).0)
}

/// Advances and line heights for a book, through whichever face each character
/// resolves to.
#[derive(Clone, Copy, Debug)]
pub struct BookMetrics<'a> {
    faces: Option<&'a FaceSet>,
}

impl BookMetrics<'_> {
    /// The standard 14 alone, which is what a book with no `@font-face` in it
    /// is set with.
    ///
    /// A `const` rather than a `Default`, and the difference is the point:
    /// `tinker-pdf-layout` deliberately has no `Default` metrics so that a
    /// caller cannot get a pagination without saying what it was measured
    /// with. This is a caller saying so.
    pub const STANDARD: BookMetrics<'static> = BookMetrics { faces: None };

    /// Metrics that may reach a book's own embedded faces.
    #[must_use]
    pub fn with(faces: &FaceSet) -> BookMetrics<'_> {
        BookMetrics { faces: Some(faces) }
    }

    /// The face set, or the empty one.
    #[must_use]
    pub fn faces(&self) -> &FaceSet {
        self.faces.unwrap_or(FaceSet::EMPTY)
    }
}

impl Metrics for BookMetrics<'_> {
    fn advance(&self, ch: char, font: &FontRequest<'_>) -> f64 {
        match choose(self.faces(), font, Some(ch)) {
            // The file's own `hmtx`, which is the only number that can agree
            // with the glyphs a viewer will draw from the program this
            // document embeds.
            Chosen::Embedded(index) => match self.faces().faces().get(index) {
                Some(face) => face.advance_em(ch) * font.size,
                None => font.size * 0.5,
            },
            Chosen::Standard(face) => {
                // Drawn in the bundled stand-in, so measured in it: one path
                // owns the run (see the module's CID-keyed fallback).
                if let Some((_, advance)) = fallback_glyph(face, ch) {
                    return advance * font.size;
                }
                standard_width(face, ch) / 1000.0 * font.size
            }
        }
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        let (ascent, descent) = match choose(self.faces(), font, None) {
            Chosen::Embedded(index) => self
                .faces()
                .faces()
                .get(index)
                .and_then(super::typeface::EmbeddedFace::vertical_fractions)
                .unwrap_or_else(|| Face::of(font).vertical_fractions()),
            Chosen::Standard(face) => face.vertical_fractions(),
        };
        Vertical {
            ascent: ascent * font.size,
            descent: descent * font.size,
        }
    }

    fn first_strong(&self, text: &str) -> Option<FirstStrong> {
        Some(first_strong(text))
    }

    fn letter_spaced(&self, ch: char) -> bool {
        letter_spaced(ch)
    }

    fn shaper(&self) -> Option<&dyn Shaper> {
        Some(self)
    }
}

/// Whether `letter-spacing` is added for `ch`: every character but a
/// nonspacing mark (`Bidi_Class` `NSM`), which belongs to the typographic
/// character unit of the letter before it (`css-text-3` §10.2).
///
/// The one answer layout ([`Metrics::letter_spaced`]) and every painter ask:
/// [`draw_coded`] moves its pen by it, [`shaped_glyphs`] charges a cluster
/// by it, and [`cut_run`] cuts a run's width by it, so a run is
/// drawn as wide as it was measured. Spaced a character at a time, as it
/// was, a letter with a mark was followed by two spacings and one with four
/// marks by five; drawn on its letter, the mark left all of them as a gap
/// after it, and an Arabic or pointed Hebrew word set with `letter-spacing`
/// was wider than the same word unpointed, by a spacing a mark. The class is
/// the one [`mark_clusters`] and ruling 14's pairing test, so what is drawn
/// as a mark is spaced as one.
fn letter_spaced(ch: char) -> bool {
    bidi_class(ch) != BidiClass::NSM
}

/// UAX #9's P2 over one box's text, for a `unicode-bidi: plaintext`
/// container: the first strong character outside the text's own isolates, or
/// the separator that ended the paragraph before one was found. A separator
/// inside an isolate ends it too, since P1 splits before any isolate opens.
fn first_strong(text: &str) -> FirstStrong {
    let mut depth = 0usize;
    for c in text.chars() {
        match bidi_class(c) {
            BidiClass::B => return FirstStrong::Separator,
            class if class.is_isolate_initiator() => depth += 1,
            BidiClass::PDI => depth = depth.saturating_sub(1),
            BidiClass::L if depth == 0 => return FirstStrong::Left,
            BidiClass::R | BidiClass::AL if depth == 0 => return FirstStrong::Right,
            _ => {}
        }
    }
    FirstStrong::Neither
}

/// Milestone 6 of `docs/design/shaping.md`: the book's own faces, shaped.
///
/// # Why this answers for every run and not only for the ones it can shape
///
/// [`tinker_pdf_layout::metrics::Metrics::shaper`] is asked once per provider
/// and not once per run, so a provider that is a shaper owns **all** of its
/// runs. That is not a limitation to work around; it is the rule. A run
/// measured by the shaper and drawn from `Metrics::advance` — or the other way
/// round — is the two-paths-disagree failure `metrics.rs` warns about, and the
/// only way to make it unreachable is for one of the two to own everything.
///
/// So a run set in one of the standard 14, which has no sfnt in this process
/// to shape against, is measured here by summing [`BookMetrics::advance`] —
/// the same number, produced by the same provider, **once**. What changes is
/// not the arithmetic but who did it.
///
/// # The clusters are the run's own
///
/// `tinker_pdf_shape` numbers a cluster by byte offset into the paragraph it
/// was given, and the paragraph here *is* the run, so the offsets come back
/// indexed from the start of `text` and need no adjustment. Milestone 7 turns
/// them into `/ToUnicode`.
impl Shaper for BookMetrics<'_> {
    fn shape(&self, text: &str, font: &FontRequest<'_>, rtl: bool) -> ShapedText {
        self.shape_in(text, font, rtl, &ShapingContext::NONE)
    }

    /// The run in its context: the same rule the painter draws it by
    /// ([`Fonts::set_contexts`]), so a run is measured as it is drawn.
    ///
    /// A neighbour is a context only where the characters either side of the
    /// boundary resolve to **one embedded face** ([`one_embedded_face`]), and
    /// only its near [`CONTEXT_CHARS`] are shaped. It belongs to the run's
    /// logically first face segment if before, and last if after, as in
    /// [`draw_run_against`]. A segment with a context is shaped with it either
    /// side and keeps its own glyphs ([`shape_in_context`]); every other
    /// segment is shaped alone, as before.
    fn shape_in(
        &self,
        text: &str,
        font: &FontRequest<'_>,
        rtl: bool,
        context: &ShapingContext<'_>,
    ) -> ShapedText {
        let before = context
            .before
            .filter(|n| {
                one_embedded_face(
                    self.faces(),
                    (n.text.chars().last(), &n.font),
                    (text.chars().next(), font),
                )
            })
            .map(|n| tail(n.text, CONTEXT_CHARS))
            .unwrap_or_default();
        let after = context
            .after
            .filter(|n| {
                one_embedded_face(
                    self.faces(),
                    (text.chars().last(), font),
                    (n.text.chars().next(), &n.font),
                )
            })
            .map(|n| head(n.text, CONTEXT_CHARS))
            .unwrap_or_default();
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        for (range, chosen) in face_runs(self.faces(), font, text) {
            let slice = text.get(range.clone()).unwrap_or("");
            let near = (
                if range.start == 0 {
                    before.as_str()
                } else {
                    ""
                },
                if range.end == text.len() {
                    after.as_str()
                } else {
                    ""
                },
            );
            let shaped = match chosen {
                Chosen::Embedded(index) => {
                    self.faces().faces().get(index).and_then(|face| match near {
                        ("", "") => shape_with(&face.program, slice, font, rtl),
                        _ => shape_in_context(&face.program, slice, font, rtl, near),
                    })
                }
                Chosen::Standard(_) => None,
            };
            let mut shaped = shaped.unwrap_or_else(|| self.unshaped(slice, font, rtl));
            // A cluster is a byte offset into the text the run was shaped
            // from, and that was the *segment*. Adding the segment's own start
            // is what makes the offsets index the caller's string, which is
            // what milestone 7 rebuilds `/ToUnicode` from.
            let base = u32::try_from(range.start).unwrap_or(u32::MAX);
            for glyph in &mut shaped.glyphs {
                glyph.cluster = glyph.cluster.saturating_add(base);
            }
            advance += shaped.advance;
            glyphs.extend(shaped.glyphs);
        }
        ShapedText {
            glyphs,
            advance,
            rtl,
        }
    }
}

impl BookMetrics<'_> {
    /// One glyph per character, at this provider's own advances.
    ///
    /// The answer for a standard-14 run, and for an embedded face whose sfnt
    /// this build could not read. The glyph index is zero throughout because
    /// there is none to give: a simple font addresses a *code* and the
    /// consumer that draws this run resolves that itself, through
    /// [`Coded`]. What layout needs from this is the advance, and that is the
    /// provider's own.
    fn unshaped(&self, text: &str, font: &FontRequest<'_>, rtl: bool) -> ShapedText {
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        for (at, ch) in text.char_indices() {
            let width = self.advance(ch, font);
            glyphs.push(PlacedGlyph {
                glyph: 0,
                cluster: u32::try_from(at).unwrap_or(u32::MAX),
                x_advance: width,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            advance += width;
        }
        ShapedText {
            glyphs,
            advance,
            rtl,
        }
    }
}

/// Shapes one run against one embedded face, scaling design units to points.
///
/// `None` where the bytes are not an sfnt this build reads, which is the same
/// answer [`EmbeddedFace::advance_em`] gives for the same face and leaves the
/// caller to fall back.
///
/// The scale is `units * size / units_per_em`: one multiply and one divide,
/// both correctly rounded by IEEE 754, which is where ruling 4's integer
/// pipeline is allowed to end. Every number is an integer until this line.
fn shape_with(bytes: &[u8], text: &str, font: &FontRequest<'_>, rtl: bool) -> Option<ShapedText> {
    let sfnt = Sfnt::parse(bytes)?;
    let settings = settings_of(font.kerning, font.features);
    let shaper = tinker_pdf_shape::Shaper::new(&sfnt).with_settings(&settings);
    let direction = if rtl {
        BaseDirection::RightToLeft
    } else {
        BaseDirection::Auto
    };
    let (_, runs) = shaper.shape_text(text, direction);
    let mut glyphs = Vec::new();
    let mut advance = 0.0;
    for run in &runs {
        let units = f64::from(run.units_per_em().max(1));
        let scale = |value: i32| f64::from(value) * font.size / units;
        for glyph in run.glyphs() {
            glyphs.push(PlacedGlyph {
                glyph: glyph.glyph,
                cluster: glyph.cluster,
                x_advance: scale(glyph.x_advance),
                y_advance: scale(glyph.y_advance),
                x_offset: scale(glyph.x_offset),
                y_offset: scale(glyph.y_offset),
            });
            advance += scale(glyph.x_advance);
        }
    }
    Some(ShapedText {
        glyphs,
        advance,
        rtl,
    })
}

/// [`shape_with`], with the text either side of `text` shaped beside it:
/// `text`'s own glyphs and their advances, as the painter's
/// [`shaped_glyphs`] places them.
///
/// The paragraph direction is `text`'s own ([`own_direction`]), as it is
/// where the run is drawn, and the clusters are put back to index `text`.
/// Unlike the painter this keeps the context's shaping even where L2 would
/// lay a context between the run's own glyphs: the painter cuts such a run at
/// its level boundary before drawing it ([`split_at_levels`]), and each piece
/// is then shaped in the context this measured it in — so the run's width
/// here is the sum of what its pieces are drawn at.
fn shape_in_context(
    bytes: &[u8],
    text: &str,
    font: &FontRequest<'_>,
    rtl: bool,
    (before, after): (&str, &str),
) -> Option<ShapedText> {
    let sfnt = Sfnt::parse(bytes)?;
    let settings = settings_of(font.kerning, font.features);
    let shaper = tinker_pdf_shape::Shaper::new(&sfnt).with_settings(&settings);
    let whole = format!("{before}{text}{after}");
    let own = before.len()..before.len() + text.len();
    let paragraph = Paragraph::new(&whole, own_direction(text));
    let mut glyphs = Vec::new();
    let mut advance = 0.0;
    for run in itemize(&whole, &paragraph) {
        let shaped = shaper.shape(&whole, &run);
        let units = f64::from(shaped.units_per_em().max(1));
        let scale = |value: i32| f64::from(value) * font.size / units;
        for glyph in shaped.glyphs() {
            let Some(at) = usize::try_from(glyph.cluster)
                .ok()
                .filter(|at| own.contains(at))
            else {
                continue;
            };
            glyphs.push(PlacedGlyph {
                glyph: glyph.glyph,
                cluster: u32::try_from(at - own.start).unwrap_or(u32::MAX),
                x_advance: scale(glyph.x_advance),
                y_advance: scale(glyph.y_advance),
                x_offset: scale(glyph.x_offset),
                y_offset: scale(glyph.y_offset),
            });
            advance += scale(glyph.x_advance);
        }
    }
    Some(ShapedText {
        glyphs,
        advance,
        rtl,
    })
}

/// How one character reaches the page: a code in a simple font, or a glyph
/// index in a composite one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coded {
    /// A byte in a `WinAnsiEncoding` or `/Differences` font.
    Simple {
        /// The resource name.
        resource: Vec<u8>,
        /// The code.
        code: u8,
    },
    /// A glyph index in a `/Identity-H` composite font.
    ///
    /// A composite font is what lets an embedded face draw a character the
    /// standard 14 have no code for at all: under `/Identity-H` the two-byte
    /// code *is* the glyph index, so the 224-code ceiling the overflow font
    /// lives under does not exist here.
    Composite {
        /// The resource name.
        resource: Vec<u8>,
        /// The glyph index in the embedded program.
        id: u16,
    },
}

impl Coded {
    /// The resource name, whichever kind this is.
    #[must_use]
    pub fn resource(&self) -> &[u8] {
        match self {
            Coded::Simple { resource, .. } | Coded::Composite { resource, .. } => resource,
        }
    }
}

/// The font resources a book needs, the codes its out-of-encoding characters
/// were given, and what no face could draw.
#[derive(Clone, Debug)]
pub struct Fonts<'a> {
    faces: &'a FaceSet,
    /// Per standard face, in [`Face::all`] order, the characters that needed an
    /// overflow code, in the order they were first met.
    overflow: Vec<Vec<char>>,
    /// Which standard faces drew anything at all, so a book of Times does not
    /// carry twelve font dictionaries.
    used: Vec<bool>,
    /// Which standard faces drew a character in their CID-keyed fallback, so
    /// only those embed a stand-in.
    fallback: Vec<bool>,
    /// Which embedded faces drew anything, so a book that declares six faces
    /// and uses two embeds two.
    used_embedded: Vec<bool>,
    /// Characters that could not be given a code at all.
    unrepresented: usize,
    /// The page being drawn's shaping context: for each run, the text of its
    /// logical neighbours on the line where they are set in the same face.
    /// See [`Fonts::set_contexts`].
    contexts: RefCell<BTreeMap<RunKey, (String, String)>>,
    /// The page being drawn's runs on a line that holds another paragraph's
    /// text too ([`holds_two_paragraphs`]). See [`Fonts::set_contexts`].
    shared: RefCell<std::collections::BTreeSet<RunKey>>,
    /// The page being drawn's runs drawn otherwise than by the rules, where a
    /// reader read their line back otherwise. See [`Fonts::set_readings`].
    drawings: RefCell<BTreeMap<RunKey, Drawing>>,
    /// How long an em of each run's own spacing is in a reader's ems on the
    /// page being drawn, where it is not one ([`along_the_line`]). See
    /// [`Fonts::set_contexts`].
    along: RefCell<BTreeMap<RunKey, f64>>,
    /// The page being drawn's runs as [`Fonts::set_readings`] planned them,
    /// by each run's place in the page's list of runs and the drawing it was
    /// planned in: planned once however often a search reads them back, and
    /// written by [`draw_run`], which is handed the place it draws, as
    /// planned. See [`Fonts::plan_run`].
    plans: RefCell<BTreeMap<(usize, Drawing), RunPlan>>,
    /// The character a reader reads for each character the overflow font
    /// has drawn, by its `/Differences` name ([`overflow_reads_as`]): looked
    /// up once a book, not once a glyph a reading.
    overflow_reads: RefCell<BTreeMap<char, char>>,
    /// The reading in logical order of each line of units the page being
    /// drawn's read-back has read ([`crate::text_order::Readings`]): searched
    /// for once a page however often a reading meets the units again. See
    /// [`Fonts::read_back`].
    readings: RefCell<crate::text_order::Readings>,
    /// Characters drawn in a face that has no glyph for them.
    uncovered: usize,
}

impl<'a> Fonts<'a> {
    /// An empty registry over a book's faces.
    #[must_use]
    pub fn new(faces: &'a FaceSet) -> Fonts<'a> {
        Fonts {
            faces,
            overflow: vec![Vec::new(); 12],
            used: vec![false; 12],
            fallback: vec![false; 12],
            used_embedded: vec![false; faces.faces().len()],
            unrepresented: 0,
            uncovered: 0,
            contexts: RefCell::new(BTreeMap::new()),
            shared: RefCell::new(std::collections::BTreeSet::new()),
            drawings: RefCell::new(BTreeMap::new()),
            along: RefCell::new(BTreeMap::new()),
            plans: RefCell::new(BTreeMap::new()),
            overflow_reads: RefCell::new(BTreeMap::new()),
            readings: RefCell::new(crate::text_order::Readings::default()),
        }
    }

    /// The faces this registry chooses from.
    #[must_use]
    pub fn faces(&self) -> &FaceSet {
        self.faces
    }

    /// Records every character one run will draw.
    pub fn note(&mut self, run: &TextRun) {
        let font = request(run);
        for ch in run.text.chars() {
            match choose(self.faces, &font, Some(ch)) {
                Chosen::Embedded(index) => {
                    if let Some(slot) = self.used_embedded.get_mut(index) {
                        *slot = true;
                    }
                }
                Chosen::Standard(face) => {
                    let index = face.index();
                    self.used[index] = true;
                    if winansi_code(ch).is_some() {
                        continue;
                    }
                    // Drawn, with its own glyph and its own `/ToUnicode`
                    // entry, in the stand-in: neither a code spent nor a
                    // notdef.
                    if fallback_glyph(face, ch).is_some() {
                        self.fallback[index] = true;
                        continue;
                    }
                    if !self.overflow[index].contains(&ch) {
                        if self.overflow[index].len() >= OVERFLOW_CODES {
                            // No code at all: it is not on the page and not in
                            // `Page::text()` either, which is a different fact
                            // from being drawn as a notdef and is counted
                            // separately.
                            self.unrepresented += 1;
                            continue;
                        }
                        self.overflow[index].push(ch);
                    }
                    // It has a code, so it extracts; the face has no glyph, so
                    // the page shows a notdef. Counted **per occurrence**,
                    // because the number a host wants is how much of the book
                    // is blank rather than how many distinct characters are.
                    self.uncovered += 1;
                }
            }
        }
    }

    /// Records, for every run on a page, the text of its logical neighbours
    /// that a shaper needs to see — what **GSUB** joins across and what
    /// **GPOS** positions against — so that a styled span inside a word is
    /// shaped in its context rather than as a word of its own.
    ///
    /// # Why the run is not the unit of shaping any more
    ///
    /// A span is a `TextRun` of its own, and a run shaped alone sees nothing
    /// either side of it: an Arabic word whose middle letter is coloured was
    /// drawn as three isolated letters, and a mark or the second glyph of a
    /// kerning pair in a span of its own lost the offset its neighbour gives
    /// it. Both are shaping decisions that depend on a glyph **below the
    /// run's own level** — in another run — and the shaper can make them only
    /// if it is shown that glyph.
    ///
    /// So each run's neighbours are recorded here, by logical order on the
    /// page — which is the order the runs are in, whatever [`visual_lines`]
    /// did to their `x` — when they **touch on one line** and the
    /// characters at the boundary resolve to the **same embedded face**: a
    /// glyph index means nothing in another face, and the standard 14 are not
    /// shaped. See [`Fonts::continues`] for why touching, and not nearness.
    /// [`draw_shaped`] shapes the run with that text either side and draws
    /// only its own glyphs, each where the shaper put it relative to them.
    ///
    /// # And the measurement agrees
    ///
    /// Layout measures each run with the same neighbours
    /// (`tinker_pdf_layout::metrics::Shaper::shape_in`, through
    /// [`BookMetrics`]): its painted neighbours on the line, the same face
    /// at the boundary decided by the one function both sides call
    /// ([`one_embedded_face`]), the same [`CONTEXT_CHARS`]. So a contextual
    /// form whose advance differs from the isolated one's, or a pair that
    /// kerns, is the width layout gave the run — it used to leave the
    /// difference between this run and the next.
    ///
    /// Through `&self`, because drawing holds the registry shared; it is
    /// replaced whole per page, and read by nothing but [`draw_run`].
    pub fn set_contexts(&self, runs: &[TextRun], effects: &OnPage<'_>) {
        let painted: Vec<&TextRun> = runs
            .iter()
            .filter(|run| run.painted && !run.generated)
            .collect();
        // A run's spacing as a reader measures it on the page: first, since
        // which runs share a line is measured so too.
        *self.along.borrow_mut() = painted
            .iter()
            .map(|run| (run_key(run), along_the_line(effects.matrix(run.anchor))))
            .filter(|(_, along)| *along != 1.0)
            .collect();
        let mut map: BTreeMap<RunKey, (String, String)> = BTreeMap::new();
        for (at, run) in painted.iter().enumerate() {
            let before = at
                .checked_sub(1)
                .and_then(|p| painted.get(p))
                .filter(|previous| self.continues(previous, run))
                .map(|previous| tail(&previous.text, CONTEXT_CHARS))
                .unwrap_or_default();
            let after = painted
                .get(at + 1)
                .filter(|next| self.continues(run, next))
                .map(|next| head(&next.text, CONTEXT_CHARS))
                .unwrap_or_default();
            if !before.is_empty() || !after.is_empty() {
                map.insert(run_key(run), (before, after));
            }
        }
        *self.contexts.borrow_mut() = map;
        *self.shared.borrow_mut() = sharing_a_line(&painted, &|run| self.along(run));
    }

    /// How long an em of `run`'s own spacing is in a reader's ems on the page
    /// being drawn ([`along_the_line`]): one but under a transform that scales
    /// the baseline otherwise than the glyphs.
    fn along(&self, run: &TextRun) -> f64 {
        self.along
            .borrow()
            .get(&run_key(run))
            .copied()
            .unwrap_or(1.0)
    }

    /// Whether `run` is on a line that holds another paragraph's text too,
    /// as [`Fonts::set_contexts`] found the page ([`holds_two_paragraphs`]).
    fn shares_its_line(&self, run: &TextRun) -> bool {
        self.shared.borrow().contains(&run_key(run))
    }

    /// How [`Fonts::set_readings`] has `run` drawn: by the rules, or
    /// otherwise where its line read back otherwise.
    fn drawing_of(&self, run: &TextRun) -> Drawing {
        self.drawings
            .borrow()
            .get(&run_key(run))
            .copied()
            .unwrap_or_default()
    }

    /// **Reads every line of a page that holds a right-to-left character back
    /// as a reader of the page reads it, and draws it otherwise where that
    /// reads it right and the rules' drawing does not.**
    ///
    /// The rules above — which neutrals [`coded_order`] draws in their
    /// level's order ([`neutrals_drawn_as_typed`]), which slices are one text
    /// object ([`draw_coded`]) — each model what `tinker-pdf-content`'s
    /// `TextDevice` and ruling 14's reading (`text_order.rs`) will do with the
    /// line, and each was measured to model it on the books it was found on.
    /// But the reader cuts a line where its text objects leave a gap and
    /// resumes one it went back over within three ems, carries its pen across
    /// a stretch only into a right-to-left letter, rejoins pieces that meet,
    /// and reads each piece's direction off its ends and its majority — and a
    /// model of all of that drifts from it, as every review of the overflow
    /// font at `0 Tw` found (ROADMAP CD-19): `<p dir="rtl"><i>?!</i> عوامل</p>`
    /// letter-spaced and justified, its `?!` drawn as typed for a reader the
    /// model said read it alone, was read with the word before it, `!?`.
    ///
    /// So the line is drawn into the reader itself — the text objects
    /// [`plan_coded`] would write, each glyph where the page has it, under
    /// the matrix the page draws its run under ([`OnPage::matrix`]), and read
    /// back as the page's font gives it back ([`Segment::read_as`]), fed to a
    /// `TextDevice` and read in logical order — and the text it reads is
    /// held to the line's own so read, its runs' text in the order written
    /// ([`Words`]). A line the rules' drawing reads with its words is drawn
    /// by the rules; one it reads with its letters in order, a word cut where
    /// a reader parts the spacing or two run together ([`Read::Letters`]), is
    /// searched no further on the page, as 935b077, which held a reading to
    /// the letters alone, searched it no further. Where the rules' drawing
    /// does not read the letters in order, each run is drawn another way in
    /// turn: its neutrals in the other order ([`Drawing::level_order`]), or
    /// its slice cut otherwise into text objects ([`Cut`]); then two runs at
    /// a time, within [`READINGS_PER_LINE`] ([`Fonts::search`]). The first
    /// drawing that reads the line's letters in order is the one drawn, as
    /// 935b077 drew it, though a reader cut a word of it or ran two
    /// together; and none that sets more glyphs over others than the rules'
    /// ([`Overprint`]). Under a transform, a line nothing tried reads right
    /// on the page is searched again in its runs' own space, and what that
    /// finds is drawn only where it reads closer to the line on the page
    /// than the rules' drawing ([`Fonts::reads_closer`]); and a line the
    /// rules' drawing reads there with its letters in order and not its
    /// words is searched in its runs' own space too, as 935b077 searched
    /// every line under a transform, and what that finds is drawn only where
    /// it reads the words right on the page ([`Fonts::reads_whole`]). A line
    /// nothing tried reads right, one of whose characters the page's fonts
    /// give back otherwise than typed, is searched again with each character
    /// read as typed, as 05cea0b read every line ([`Given::AsTyped`]), and
    /// what that finds is drawn only where it reads closer to the line on
    /// the page than the rules' drawing: a NO-BREAK SPACE is a word space on
    /// the page, which parts `1 234` into two numbers a right-to-left line
    /// reads the other way round in every drawing tried, and no drawing read
    /// the line's letters in order, though one read the styled `?!` after the
    /// number as typed where the rules' drawing turns it. A line under a
    /// matrix that turns its baseline, which nothing tried reads with its
    /// letters in order, is drawn as the first drawing that reads more of its
    /// runs with their own characters in order on the page than the rules'
    /// drawing does, and sets no more glyphs over others
    /// ([`Fonts::search_closer`]). A line the rules' drawing reads right, or
    /// that nothing tried reads closer, is drawn by the rules; and a line
    /// with no right-to-left character, no drawing to try ([`drawings_of`]),
    /// or a run of a face of the book's own, which [`plan_coded`] does not
    /// draw, is not read.
    ///
    /// The work is linear in the line, once a reading, and a line is read at
    /// most [`READINGS_PER_LINE`] times, twice that and twice more under a
    /// transform, and that again and twice more where a character is read
    /// otherwise than typed. Each run is planned once a page for each
    /// drawing a reading tries it in, and drawn as planned
    /// ([`Fonts::plan_run`]); and a line of units a reading puts in logical
    /// order is searched for its reading once a page, however often a reading
    /// meets those units again, but for a chain of pieces the bracket
    /// tie-break reads whole ([`Fonts::readings`]). It is not free: a line
    /// that no drawing reads right spends the whole budget (ROADMAP CD-19).
    pub fn set_readings(&self, runs: &[TextRun], frame: &Frame, effects: &OnPage<'_>) {
        let painted: Vec<&TextRun> = runs
            .iter()
            .filter(|run| run.painted && !run.generated)
            .collect();
        let order: BTreeMap<RunKey, usize> = painted
            .iter()
            .enumerate()
            .map(|(at, run)| (run_key(run), at))
            .collect();
        let mut drawings: BTreeMap<RunKey, Drawing> = BTreeMap::new();
        // The page's runs are planned anew ([`Fonts::plan_run`]), and its
        // lines of units read anew ([`Fonts::readings`]).
        self.plans.borrow_mut().clear();
        *self.readings.borrow_mut() = crate::text_order::Readings::default();
        // How the page's fonts give each character back, by the font a run
        // asks for, which is all [`Fonts::read_as`] depends on: worked out
        // once a page for each, where a line's characters are a few dozen
        // met again on every line.
        let mut reads: Vec<(&TextRun, BTreeMap<char, Option<char>>)> = Vec::new();
        for mut line in page_lines(&painted, &|run| self.along(run)) {
            if !line
                .iter()
                .any(|run| run.text.chars().any(undoes_its_levels))
            {
                continue;
            }
            // In the order drawn, which is the order written.
            line.sort_by_key(|run| order.get(&run_key(run)).copied().unwrap_or(usize::MAX));
            // Each run's place in the page's list, which its plans are kept
            // by ([`Fonts::plans`]).
            let Some(places) = line
                .iter()
                .map(|run| place_in(runs, run))
                .collect::<Option<Vec<usize>>>()
            else {
                continue;
            };
            // Each run's drawings other than the rules'. A line with none is
            // drawn by the rules whatever a reading of it finds, and is not
            // read, as 935b077 did not read it.
            let options: Vec<(RunKey, Drawing)> = line
                .iter()
                .flat_map(|run| drawings_of(run, self.along(run)))
                .collect();
            if options.is_empty() {
                continue;
            }
            // Each run under the matrix the page draws it under: a reader's
            // thresholds are in the page's ems, not the run's.
            let matrices: Vec<[f64; 6]> =
                line.iter().map(|run| effects.matrix(run.anchor)).collect();
            // The line's text as the page's fonts give it back, and as typed,
            // the characters they draw; and whether the two differ.
            let mut typed = String::new();
            let mut as_typed = String::new();
            for run in &line {
                let font = match reads.iter().position(|(seen, _)| same_font(seen, run)) {
                    Some(font) => font,
                    None => {
                        reads.push((run, BTreeMap::new()));
                        reads.len() - 1
                    }
                };
                let Some((_, known)) = reads.get_mut(font) else {
                    continue;
                };
                for ch in run.text.chars() {
                    if let Some(read) = *known.entry(ch).or_insert_with(|| self.read_as(run, ch)) {
                        typed.push(read);
                        as_typed.push(ch);
                    }
                }
            }
            let retyped = typed != as_typed;
            let written = Words::of(&typed);
            let page = self.search(
                &line,
                &places,
                &matrices,
                frame,
                &written,
                &options,
                Given::ByTheFont,
            );
            let nothing = matches!(page, Search::Nothing);
            let transformed = matrices.iter().any(|matrix| *matrix != IDENTITY);
            let own = || vec![IDENTITY; line.len()];
            let found = match page {
                Search::Rules => continue,
                Search::Found(found) => Some(found),
                // Under a transform, where nothing tried reads the line right
                // on the page — glyphs the transform sets over others, whose
                // letters no drawing reads in order — the line is read back in
                // its runs' own space, as it was before the page's matrix was
                // modelled.
                Search::Nothing if transformed => {
                    match self.search(
                        &line,
                        &places,
                        &own(),
                        frame,
                        &written,
                        &options,
                        Given::ByTheFont,
                    ) {
                        Search::Found(found)
                            if self.reads_closer(
                                &line, &places, &matrices, frame, &written, &found,
                            ) =>
                        {
                            Some(found)
                        }
                        Search::Found(_) | Search::Rules | Search::Letters | Search::Nothing => {
                            None
                        }
                    }
                }
                // Under a transform, where the rules' drawing reads the line
                // on the page with its letters in order and not its words,
                // the line is searched in its runs' own space too, as every
                // line under a transform was searched before the page's
                // matrix was modelled, and what that finds is drawn where it
                // reads the words right on the page.
                Search::Letters if transformed => {
                    match self.search(
                        &line,
                        &places,
                        &own(),
                        frame,
                        &written,
                        &options,
                        Given::ByTheFont,
                    ) {
                        Search::Found(found)
                            if self.reads_whole(
                                &line, &places, &matrices, frame, &written, &found,
                            ) =>
                        {
                            Some(found)
                        }
                        Search::Found(_) | Search::Rules | Search::Letters | Search::Nothing => {
                            None
                        }
                    }
                }
                Search::Letters | Search::Nothing => None,
            };
            // Where nothing tried reads the line right as the page's fonts
            // give its characters back, and they give one back otherwise
            // than typed — a NO-BREAK SPACE between two digits, a word space
            // on the page, which parts the number in every drawing tried —
            // the line is searched again with each character read as typed,
            // as 05cea0b read it, and what that finds is drawn where it reads
            // closer to the line on the page than the rules' drawing.
            let found = match found {
                None if nothing && retyped => {
                    let typed_words = Words::of(&as_typed);
                    match self.search(
                        &line,
                        &places,
                        &matrices,
                        frame,
                        &typed_words,
                        &options,
                        Given::AsTyped,
                    ) {
                        Search::Found(retyping)
                            if self.reads_closer(
                                &line, &places, &matrices, frame, &written, &retyping,
                            ) =>
                        {
                            Some(retyping)
                        }
                        Search::Found(_) | Search::Rules | Search::Letters | Search::Nothing => {
                            None
                        }
                    }
                }
                found => found,
            };
            // And where still nothing reads the line's letters in order under
            // a matrix that turns its baseline off the page's `x` axis — where
            // `TextDevice`, which asks whether a glyph continues a line along
            // the page's own `x` and `y`, parts the line at its word spaces
            // into pieces it keeps in the order drawn — a drawing that reads
            // more of its runs with their own characters in order on the page
            // is drawn ([`Fonts::search_closer`]): a right-to-left run's words
            // written in reading order ([`Cut::Reading`]).
            let turned = matrices.iter().any(|matrix| matrix[1] != 0.0);
            let found = match found {
                None if nothing && turned => {
                    self.search_closer(&line, &places, &matrices, frame, &written, &options)
                }
                found => found,
            };
            if let Some(found) = found {
                drawings.extend(found);
            }
        }
        *self.drawings.borrow_mut() = drawings;
        *self.readings.borrow_mut() = crate::text_order::Readings::default();
    }

    /// [`Fonts::set_readings`]'s search for one line under `matrices`: the
    /// rules' drawing where it reads the line's words right, and
    /// [`Search::Letters`] where it reads its letters in order; else the
    /// first drawing of one run, then of two, within [`READINGS_PER_LINE`]
    /// readings, that reads the letters in order, as 935b077 took it; else
    /// nothing. A drawing that sets more glyphs over others than the rules'
    /// is never taken. Each glyph is read back as `given` says.
    #[allow(clippy::too_many_arguments)]
    fn search(
        &self,
        line: &[&TextRun],
        places: &[usize],
        matrices: &[[f64; 6]],
        frame: &Frame,
        written: &Words,
        options: &[(RunKey, Drawing)],
        given: Given,
    ) -> Search {
        let tried = std::cell::Cell::new(0usize);
        let reads_right = |chosen: &BTreeMap<RunKey, Drawing>| {
            tried.set(tried.get() + 1);
            self.read_back(line, places, matrices, frame, chosen, given)
                .map(|(read, over)| (written.read_as(&read), over))
        };
        // How many glyphs the rules' drawing sets over another: a drawing
        // that sets more is not taken, however it reads.
        let over_by_rules = match reads_right(&BTreeMap::new()) {
            // A reading of the rules' drawing that holds the line's letters
            // in order is not searched further, as 935b077, which held a
            // reading to the letters alone, searched it no further: searching
            // on for a drawing that reads such a line's words right spends the
            // whole budget of many a line a reader cuts at its spacing or runs
            // two words of together, a cost in time that is the owner's to
            // price (ROADMAP CD-19).
            None | Some((Read::Words, _)) => return Search::Rules,
            Some((Read::Letters, _)) => return Search::Letters,
            Some((Read::Otherwise, over)) => over.count(),
        };
        // A drawing that reads the line's letters in order is taken, as
        // 935b077 took it, though a reader cut a word of it or ran two
        // together: holding it to the words spends the whole budget of many
        // a line no drawing reads whole, a cost in time that is the owner's
        // to price (ROADMAP CD-19). None is taken that sets more glyphs over
        // others than the rules' drawing, which is counted only where it
        // decides, of a drawing that reads the letters in order.
        let taken = |chosen: &BTreeMap<RunKey, Drawing>| {
            reads_right(chosen)
                .is_some_and(|(how, over)| how != Read::Otherwise && over.count() <= over_by_rules)
        };
        for &(key, drawing) in options {
            if tried.get() >= READINGS_PER_LINE {
                break;
            }
            let one = BTreeMap::from([(key, drawing)]);
            if taken(&one) {
                return Search::Found(one);
            }
        }
        for (at, &(first, one)) in options.iter().enumerate() {
            for &(second, other) in options.get(at + 1..).unwrap_or_default() {
                if second == first {
                    continue;
                }
                if tried.get() >= READINGS_PER_LINE {
                    break;
                }
                let two = BTreeMap::from([(first, one), (second, other)]);
                if taken(&two) {
                    return Search::Found(two);
                }
            }
        }
        Search::Nothing
    }

    /// Whether `found`, a drawing [`Fonts::search`] found reading `line` right
    /// otherwise than on the page as the page's fonts give it back — in its
    /// runs' own space, or read as typed — where nothing it tried reads it
    /// right so, reads the line closer to the order written on the page than
    /// the rules' drawing does there: more of the line's runs with their own
    /// characters in the order typed ([`Fonts::runs_in_order`]), as a reader
    /// of the structure tree reads each element's text, and where as many,
    /// more of the text's pairs of letters side by side ([`Words::pairs_in`]).
    /// A drawing found right in a space that is not the page's is no better
    /// than a guess on the page: a Hebrew paragraph with no `dir` of inline
    /// blocks squeezed by `scaleX(0.5)`, turned a quarter at
    /// `word-spacing: 0.6em`, drawn as that search found it, as 935b077 drew
    /// it, lost the structure tree's order the rules' drawing holds. Held to
    /// the pairs alone, a styled `?!` in an inline block squeezed by
    /// `scaleX(0.6)` to `0.8` at `0.5em`, which the drawing found reads in
    /// its order with a letter of the next word between and the rules'
    /// drawing turns about, kept the rules' drawing and lost the order
    /// 935b077 held (round 6's review).
    fn reads_closer(
        &self,
        line: &[&TextRun],
        places: &[usize],
        matrices: &[[f64; 6]],
        frame: &Frame,
        written: &Words,
        found: &BTreeMap<RunKey, Drawing>,
    ) -> bool {
        let rules = self.read_back(
            line,
            places,
            matrices,
            frame,
            &BTreeMap::new(),
            Given::ByTheFont,
        );
        let Some((rules, _)) = rules else {
            return false;
        };
        let Some((read, _)) =
            self.read_back(line, places, matrices, frame, found, Given::ByTheFont)
        else {
            return false;
        };
        let (found_pairs, rules_pairs) = (written.pairs_in(&read), written.pairs_in(&rules));
        let (found_runs, rules_runs) = (
            self.runs_in_order(line, &read),
            self.runs_in_order(line, &rules),
        );
        found_runs > rules_runs || (found_runs == rules_runs && found_pairs > rules_pairs)
    }

    /// Where nothing [`Fonts::search`] tried reads `line`'s letters in order
    /// on the page: the first drawing of one run, then of two, within
    /// [`READINGS_PER_LINE`] readings, that reads it closer to the order
    /// written than the rules' drawing, as [`Fonts::reads_closer`] measures
    /// it, and sets no more glyphs over others; `None` where none does.
    fn search_closer(
        &self,
        line: &[&TextRun],
        places: &[usize],
        matrices: &[[f64; 6]],
        frame: &Frame,
        written: &Words,
        options: &[(RunKey, Drawing)],
    ) -> Option<BTreeMap<RunKey, Drawing>> {
        let (rules, over) = self.read_back(
            line,
            places,
            matrices,
            frame,
            &BTreeMap::new(),
            Given::ByTheFont,
        )?;
        let over = over.count();
        let (runs, pairs) = (self.runs_in_order(line, &rules), written.pairs_in(&rules));
        let tried = std::cell::Cell::new(1usize);
        let closer = |chosen: &BTreeMap<RunKey, Drawing>| {
            tried.set(tried.get() + 1);
            self.read_back(line, places, matrices, frame, chosen, Given::ByTheFont)
                .is_some_and(|(read, more)| {
                    let found = self.runs_in_order(line, &read);
                    more.count() <= over
                        && (found > runs || (found == runs && written.pairs_in(&read) > pairs))
                })
        };
        for &(key, drawing) in options {
            if tried.get() >= READINGS_PER_LINE {
                return None;
            }
            let one = BTreeMap::from([(key, drawing)]);
            if closer(&one) {
                return Some(one);
            }
        }
        for (at, &(first, one)) in options.iter().enumerate() {
            for &(second, other) in options.get(at + 1..).unwrap_or_default() {
                if second == first {
                    continue;
                }
                if tried.get() >= READINGS_PER_LINE {
                    return None;
                }
                let two = BTreeMap::from([(first, one), (second, other)]);
                if closer(&two) {
                    return Some(two);
                }
            }
        }
        None
    }

    /// How many of `line`'s runs `read` reads with their own characters in
    /// the order typed, side by side or not: what a reader of the structure
    /// tree reads of each element's text ([`Fonts::reads_closer`]).
    fn runs_in_order(&self, line: &[&TextRun], read: &str) -> usize {
        line.iter()
            .filter(|run| {
                let mut rest = read.chars().filter(|c| !c.is_whitespace());
                run.text
                    .chars()
                    .filter_map(|ch| self.read_as(run, ch))
                    .filter(|c| !c.is_whitespace())
                    .all(|ch| rest.any(|r| r == ch))
            })
            .count()
    }

    /// Whether `found`, a drawing [`Fonts::search`] found in `line`'s runs'
    /// own space where the rules' drawing reads the line on the page with its
    /// letters in order and not its words ([`Search::Letters`]), reads its
    /// words right on the page.
    /// A letter-spaced and justified Arabic line under `scaleX(2)` whose
    /// rules' drawing a reader parts inside `25٪` on the page was drawn so,
    /// and read whole, before the page's matrix was modelled; a styled `?!`
    /// with no `dir`, turned for a reading in which it joined its word in
    /// that space, is not.
    fn reads_whole(
        &self,
        line: &[&TextRun],
        places: &[usize],
        matrices: &[[f64; 6]],
        frame: &Frame,
        written: &Words,
        found: &BTreeMap<RunKey, Drawing>,
    ) -> bool {
        self.read_back(line, places, matrices, frame, found, Given::ByTheFont)
            .is_some_and(|(read, _)| written.read_as(&read) == Read::Words)
    }

    /// The text objects [`plan_coded`] writes for the whole of `run` drawn as
    /// `drawing` says, each face segment's in the order drawn, and where the
    /// pen ends: what [`Fonts::read_back`] reads and [`draw_run`] writes.
    /// `None` for a run with a character in a face of the book's own, which
    /// [`draw_shaped`] draws.
    ///
    /// Planned once a page for each drawing ([`Fonts::plans`]): a plan
    /// depends on neither the drawings of the run's neighbours nor the space
    /// a line is read in, and a search reads a run back in as many of the
    /// one as it tries and, under a transform, in both of the other. Kept by
    /// the run's `place` in the page's list of runs. Returns the plan's key.
    fn plan_run(&self, place: usize, run: &TextRun, drawing: Drawing) -> (usize, Drawing) {
        let key = (place, drawing);
        if !self.plans.borrow().contains_key(&key) {
            let plan = self.plan_anew(run, drawing);
            self.plans.borrow_mut().insert(key, plan);
        }
        key
    }

    /// [`Fonts::plan_run`]'s plan, made.
    fn plan_anew(&self, run: &TextRun, drawing: Drawing) -> RunPlan {
        let font = request(run);
        let mut segments = face_runs(self.faces, &font, &run.text);
        if segments
            .iter()
            .any(|(_, chosen)| matches!(chosen, Chosen::Embedded(_)))
        {
            return None;
        }
        if reads_right_to_left(run) {
            segments.reverse();
        }
        let mut x = run.x;
        let mut out = Vec::new();
        for (range, _) in segments {
            let slice = run.text.get(range).unwrap_or("");
            let (objects, end) = plan_coded(run, self, slice, x, drawing);
            x = end;
            out.extend(objects);
        }
        Some((out, x))
    }

    /// The character a reader of the page reads where `run` draws `ch`, as
    /// the font this registry draws it in gives it back ([`Segment::read_as`]),
    /// or `None` where it has no code and is not drawn ([`Fonts::encode`]).
    fn read_as(&self, run: &TextRun, ch: char) -> Option<char> {
        let chosen = choose(self.faces, &request(run), Some(ch));
        match self.encode(chosen, ch)? {
            Coded::Composite { .. } => Some(ch),
            Coded::Simple { resource, code } => Some(match chosen {
                Chosen::Standard(face) if resource == face.overflow_resource() => {
                    self.overflow_read(ch)
                }
                _ => base_char(BaseEncoding::WinAnsi, code).unwrap_or(ch),
            }),
        }
    }

    /// [`overflow_reads_as`], looked up once a book for each character.
    fn overflow_read(&self, ch: char) -> char {
        let known = self.overflow_reads.borrow().get(&ch).copied();
        if let Some(read) = known {
            return read;
        }
        let read = overflow_reads_as(ch);
        self.overflow_reads.borrow_mut().insert(ch, read);
        read
    }

    /// What a reader of the page reads off `line`, one line's runs in the
    /// order they are drawn, drawn as `drawings` say and the rest by the rules
    /// ([`Fonts::set_readings`]): the text objects [`plan_coded`] writes, each
    /// glyph where the page has it and as wide as its font says, fed to a
    /// `TextDevice` and read in logical order (ruling 14), and the glyphs it
    /// draws, to count those it sets over others ([`Overprint`]). Each glyph
    /// is read as `given` says. `None` for a line with a run in a face of the
    /// book's own, which [`draw_shaped`] draws.
    ///
    /// A line of units the page's read-back has put in logical order before
    /// is read as it was then ([`Fonts::readings`]): its reading is a
    /// function of its units alone, and the device parts a letter-spaced line
    /// at every text object into pieces of a word or two that the line
    /// repeats, read again in every drawing tried and, under a transform, in
    /// the runs' own space too ([`crate::text_order::Readings`]).
    fn read_back(
        &self,
        line: &[&TextRun],
        places: &[usize],
        matrices: &[[f64; 6]],
        frame: &Frame,
        drawings: &BTreeMap<RunKey, Drawing>,
        given: Given,
    ) -> Option<(String, Overprint)> {
        #[cfg(test)]
        READINGS.with(|readings| readings.set(readings.get().saturating_add(1)));
        let keys: Vec<(usize, Drawing)> = line
            .iter()
            .zip(places)
            .map(|(run, &place)| {
                let drawing = drawings.get(&run_key(run)).copied().unwrap_or_default();
                self.plan_run(place, run, drawing)
            })
            .collect();
        let plans = self.plans.borrow();
        if keys
            .iter()
            .any(|key| plans.get(key).is_none_or(Option::is_none))
        {
            return None;
        }
        let mut device = tinker_pdf_content::TextDevice::new();
        for ((at, run), key) in line.iter().enumerate().zip(&keys) {
            let Some(Some((objects, _))) = plans.get(key) else {
                continue;
            };
            // The `cm`s [`OnPage::open`] writes before the run's text object.
            let [a, b, c, d, e, f] = matrices.get(at).copied().unwrap_or(IDENTITY);
            let page = tinker_pdf_content::Matrix { a, b, c, d, e, f };
            let size = run.font_size * PX_TO_PT;
            let y = frame.y(run.y);
            let letters = run.letter_spacing * PX_TO_PT;
            let words = run.word_spacing * PX_TO_PT;
            for object in objects {
                for segment in object {
                    // As [`flush`] writes it: the overflow font and the
                    // stand-in at no word spacing.
                    let spacing = if segment.composite || (segment.unspaced && words != 0.0) {
                        0.0
                    } else {
                        words
                    };
                    let mut state = tinker_pdf_content::GraphicsState::new(page);
                    state.text.size = size;
                    state.text.char_spacing = letters;
                    state.text.word_spacing = spacing;
                    let font_id = segment
                        .resource
                        .iter()
                        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
                        });
                    let mut pen = frame.x(segment.x);
                    for (at, ch) in segment.characters.chars().enumerate() {
                        let code = if segment.composite {
                            segment.glyphs.get(at).map_or(0, |(id, _)| u32::from(*id))
                        } else {
                            segment.codes.get(at).map_or(0, |code| u32::from(*code))
                        };
                        // As wide as its font says: a simple font's
                        // `/Widths` are the widths layout measured with,
                        // and the stand-in's `/W` that width rounded to a
                        // thousandth of an em ([`EXACT_MARK_INSET`]).
                        let measured = segment.advances.get(at).copied().unwrap_or_default();
                        let advance = if segment.composite && run.font_size > 0.0 {
                            (measured / run.font_size * 1000.0).round() / 1000.0 * run.font_size
                        } else {
                            measured
                        } * PX_TO_PT;
                        let text = match given {
                            Given::ByTheFont => segment.read_as(ch, code, self),
                            Given::AsTyped => ch,
                        };
                        let glyph = tinker_pdf_content::Glyph {
                            code,
                            text: text.to_string(),
                            // 9.4.4's text rendering matrix: the glyph
                            // in text space, then the page's matrix.
                            transform: tinker_pdf_content::Matrix {
                                a: size,
                                b: 0.0,
                                c: 0.0,
                                d: size,
                                e: pen,
                                f: y,
                            }
                            .then(&page),
                            baseline: None,
                            advance,
                            size,
                            vertical: false,
                            font_id,
                            font_name: None,
                        };
                        tinker_pdf_content::Device::show_glyph(&mut device, &glyph, &state);
                        pen += advance
                            + letters
                            + if code == 32 && !segment.composite {
                                spacing
                            } else {
                                0.0
                            };
                    }
                }
                tinker_pdf_content::Device::end_text(&mut device);
            }
        }
        let mut page = device.finish();
        let over = Overprint::of(&page);
        crate::text_order::into_logical_order_knowing(&mut page, &mut self.readings.borrow_mut());
        Some((
            page.blocks
                .iter()
                .flat_map(|block| &block.lines)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            over,
        ))
    }

    /// The context [`Fonts::set_contexts`] recorded for `run`.
    fn context_of(&self, run: &TextRun) -> (String, String) {
        self.contexts
            .borrow()
            .get(&run_key(run))
            .cloned()
            .unwrap_or_default()
    }

    /// Whether `b` follows `a` on one line in one embedded face, so that each
    /// is the other's shaping context.
    ///
    /// **On one line, and touching**, which is what [`visual_lines`] calls a
    /// line: baselines within the larger font size, which a `vertical-align`
    /// may move them by, and `b` starting where `a` ends — or ending where `a`
    /// starts, a right-to-left neighbour once [`visual_lines`] has laid the
    /// line out. The baseline alone is not enough, and it was all this asked
    /// first: at a `line-height` of 1 or less the **next line's** baseline is
    /// within a font size too, so the word ending one line was shaped with
    /// the word starting the next as its context, and an Arabic letter was
    /// joined across the break (review of lane 6C). A new line starts at the
    /// line's own edge rather than where the last one ended, so it does not
    /// touch it. Two logical neighbours one line holds apart — a left-to-right
    /// pair of runs inside a right-to-left line puts the pair between its
    /// neighbours — have no glyph between them for a shaper to join or
    /// position against either.
    fn continues(&self, a: &TextRun, b: &TextRun) -> bool {
        if a.generated || b.generated || (a.y - b.y).abs() > a.font_size.max(b.font_size) {
            return false;
        }
        if !near(a.x + a.width, b.x) && !near(b.x + b.width, a.x) {
            return false;
        }
        one_embedded_face(
            self.faces,
            (a.text.chars().last(), &request(a)),
            (b.text.chars().next(), &request(b)),
        )
    }

    /// Whether `run` asks a face this build does not shape for kerning, and
    /// for a feature switched on: the two halves of `css-fonts-4` §6.4's and
    /// §6.12's requests that cannot be met, `(font-kerning, font-feature-settings)`.
    ///
    /// The standard 14 are drawn a character at a time from their widths —
    /// no `GSUB`, no `GPOS`, and none of their AFM kerning pairs, which this
    /// build does not carry — so a run with a visible character set in one of
    /// them gets no kerning and no feature whatever its style says. A
    /// feature switched **off** is met there trivially, and `auto` kerning is
    /// the user agent's to decide, so only `normal` and a setting above zero
    /// are asked and not given.
    #[must_use]
    pub fn unshaped_settings(&self, run: &TextRun) -> (bool, bool) {
        let kerning = run.kerning == FontKerning::Normal;
        let features = run.features.iter().any(|setting| setting.value > 0);
        if !kerning && !features {
            return (false, false);
        }
        let font = request(run);
        let unshaped = run
            .text
            .chars()
            .filter(|c| !c.is_whitespace())
            .any(|c| matches!(choose(self.faces, &font, Some(c)), Chosen::Standard(_)));
        (kerning && unshaped, features && unshaped)
    }

    /// How many characters had no code, and are therefore not on any page.
    #[must_use]
    pub fn unrepresented(&self) -> usize {
        self.unrepresented
    }

    /// How many characters are drawn as a notdef because no available face has
    /// a glyph for them.
    ///
    /// Distinct from [`Fonts::unrepresented`] and the distinction is the whole
    /// of milestone 9's honesty here: an unrepresented character is missing
    /// from the page **and** from the text, and an uncovered one is missing
    /// from the picture and present in the text. A reader that reported them as
    /// one number would make a book whose text can still be searched
    /// indistinguishable from one whose cannot.
    #[must_use]
    pub fn uncovered(&self) -> usize {
        self.uncovered
    }

    /// How many overflow fonts the document carries.
    #[must_use]
    pub fn overflow_fonts(&self) -> usize {
        self.overflow.iter().filter(|set| !set.is_empty()).count()
    }

    /// How many CID-keyed fallback faces the document carries — none in a
    /// build without `bundled-fonts`.
    #[must_use]
    pub fn fallback_fonts(&self) -> usize {
        self.fallback.iter().filter(|used| **used).count()
    }

    /// How many embedded faces the document actually carries.
    #[must_use]
    pub fn embedded_fonts(&self) -> usize {
        self.used_embedded.iter().filter(|used| **used).count()
    }

    /// How many codes one face's overflow font spends.
    ///
    /// Not the same number as how many characters were *met*, and the
    /// difference is the whole of [`Fonts::note`]'s duplicate check: a build
    /// that pushed a code per occurrence rather than per character would draw
    /// every book identically — `encode` finds the first entry either way —
    /// and would run out of the 224 on the first paragraph of Japanese. The
    /// injection matrix is what found that nothing could see it.
    #[must_use]
    pub fn codes(&self, face: Face) -> usize {
        self.overflow[face.index()].len()
    }

    /// Registers every face this book used on the document.
    pub fn register(&self, builder: &mut DocumentBuilder) {
        for face in Face::all() {
            let index = face.index();
            if !self.used[index] {
                continue;
            }
            builder.add_base_font(&face.resource(), face.base_font());
            if self.fallback[index] {
                if let Some(program) = fallback_program(face) {
                    // `/Identity-H` over the stand-in: the code is the glyph,
                    // `/W` comes from its `hmtx` and `/ToUnicode` from the
                    // characters drawn, and the writer subsets it to them.
                    builder.add_cid_font(
                        &face.fallback_resource(),
                        face.fallback_base_font(),
                        program,
                    );
                }
            }
            if self.overflow[index].is_empty() {
                continue;
            }
            let names: Vec<String> = self.overflow[index]
                .iter()
                .map(|c| glyph_name_for_char(*c).unwrap_or_else(|| "space".to_owned()))
                .collect();
            let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
            // The width a code is written with is the width this build
            // **measured** it at, not the width the standard face publishes for
            // whatever glyph it has at that code. They differ for every
            // character in this array by construction, and a `/Widths` that
            // disagreed with the layout would put a viewer's text cursor
            // somewhere the text is not — or, for a mark, put it past its
            // letter. [`standard_width`] is the number layout measured.
            let widths: Vec<u16> = self.overflow[index]
                .iter()
                .map(|c| standard_width(face, *c).round().clamp(0.0, 65535.0) as u16)
                .collect();
            builder.add_named_font(
                &face.overflow_resource(),
                face.base_font(),
                OVERFLOW_FIRST,
                &borrowed,
                &widths,
            );
        }
        for (index, face) in self.faces.faces().iter().enumerate() {
            if !self.used_embedded.get(index).copied().unwrap_or(false) {
                continue;
            }
            // A composite font, not a simple one, and the reason is the whole
            // point of embedding: `/Identity-H` makes the code the glyph index,
            // so a face can draw a character no encoding this build writes has
            // a code for. `add_cid_font` writes `/W` from the program's own
            // `hmtx` and `/ToUnicode` from the characters each glyph was drawn
            // with, so the widths cannot disagree with the outlines and the
            // text still extracts.
            builder.add_cid_font(&face.resource, &base_font_name(&face.family), &face.program);
        }
    }

    /// The resource and the code or glyph one character is drawn with.
    ///
    /// `None` for a character that got neither, which is the only case a page
    /// silently loses text in — and it is counted, not silent.
    #[must_use]
    pub fn encode(&self, chosen: Chosen, ch: char) -> Option<Coded> {
        match chosen {
            Chosen::Embedded(index) => {
                let face = self.faces.faces().get(index)?;
                Some(Coded::Composite {
                    resource: face.resource.clone(),
                    id: face.glyph(ch)?,
                })
            }
            Chosen::Standard(face) => {
                if let Some(code) = winansi_code(ch) {
                    return Some(Coded::Simple {
                        resource: face.resource(),
                        code,
                    });
                }
                if let Some((id, _)) = fallback_glyph(face, ch) {
                    return Some(Coded::Composite {
                        resource: face.fallback_resource(),
                        id,
                    });
                }
                let at = self.overflow[face.index()].iter().position(|c| *c == ch)?;
                let code = OVERFLOW_FIRST.checked_add(u8::try_from(at).ok()?)?;
                Some(Coded::Simple {
                    resource: face.overflow_resource(),
                    code,
                })
            }
        }
    }
}

/// A `/BaseFont` name for an embedded face, from the family the book declared.
///
/// Reduced to the characters a PDF name may hold without escaping, because a
/// family name is an author's string and may contain a space, a `#` or a
/// parenthesis. An empty result becomes `Embedded`, so a face declared under a
/// family of punctuation still gets a name rather than an empty one.
fn base_font_name(family: &str) -> Vec<u8> {
    let cleaned: String = family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if cleaned.is_empty() {
        b"Embedded".to_vec()
    } else {
        cleaned.into_bytes()
    }
}

/// The font a run asks for, rebuilt from what the run carries.
#[must_use]
pub fn request(run: &TextRun) -> FontRequest<'_> {
    FontRequest {
        families: &run.families,
        weight: run.weight,
        style: run.style,
        size: run.font_size,
        kerning: run.kerning,
        features: &run.features,
    }
}

/// The features a run's shaper switches on or off over its own plan:
/// `font-kerning` as `kern`, then `font-feature-settings` in the order
/// written — `css-fonts-4` §7.2's precedence, the general property first and
/// the low-level one after it, so `font-kerning: none; font-feature-settings:
/// "kern"` kerns. `auto` adds nothing: the default plan already kerns.
fn settings_of(kerning: FontKerning, features: &[FeatureSetting]) -> Vec<(Tag, u32)> {
    let mut out = Vec::with_capacity(features.len() + 1);
    match kerning {
        FontKerning::Auto => {}
        FontKerning::Normal => out.push((Tag::new(b"kern"), 1)),
        FontKerning::None => out.push((Tag::new(b"kern"), 0)),
    }
    out.extend(
        features
            .iter()
            .map(|setting| (Tag::new(&setting.tag), setting.value)),
    );
    out
}

/// What the painter applies to an **element** rather than to a box: its
/// `opacity`, composed down the element tree.
///
/// # Per fragment, and where that is the group
///
/// `css-color-4` §15.1 composites an element and its descendants as one group
/// and then applies the opacity once. This painter writes the opacity on each
/// fragment the element's subtree draws — its background, its pictures, its
/// glyphs — as an `/ExtGState` whose `/ca` and `/CA` are the product of every
/// opacity on the way up, so a paragraph at 0.5 inside a section at 0.5 is at a
/// quarter. Wherever nothing in the subtree paints over anything else in it, the
/// two are the same picture; where something does — text over its own box's
/// background — they are not, and the cascade counts that element against
/// `opacity` (`tinker_pdf_css::cascade`) rather than letting it read as exact.
/// The group would be a transparency-group form XObject, and the element's
/// glyphs are tagged marked content that the structure writer puts in the page
/// stream and not in a form.
///
/// Resources are registered **before the chapter's first page begins**, which
/// is `begin_page`'s snapshot rule (see [`super::svg::Registry`]): one
/// `/ExtGState` per distinct alpha the chapter needs, named for the alpha so
/// that the same alpha on two chapters is one resource.
///
/// # And every overflow clip, by the element tree
///
/// `css-overflow-3` §3.1 clips an element's **descendants** to its padding
/// box, and CSS 2.2 §11.1.1 says which: *"all descendants except those whose
/// containing block is the viewport or an ancestor of the element"*. The
/// layout reports each clipping box's padding box per page
/// ([`tinker_pdf_layout::Page::clips`]) and nothing about who is inside it,
/// because a float, a positioned box and a table cell inside it are drawn from
/// lists of their own; the element tree knows. So every fragment drawn here is
/// clipped by the chain of clipping elements above it — a box by its parent's
/// chain, its own text by its own — and an absolutely positioned element by
/// its **containing block's** chain, which is how a figure positioned against
/// the page escapes a clipping section it sits in.
#[derive(Clone, Debug, Default)]
pub struct Effects {
    /// Per element, the product of every `opacity` from the root down,
    /// quantised to ten-thousandths, which is the resource's name and its
    /// value: a `/ca` closer than that to another is not a different page.
    alpha: Vec<u16>,
    /// Per element, whether some page of the chapter carries a clip for it.
    /// A box whose content fitted has none and clips nothing, which is the
    /// layout's decision and not repeated here.
    clips: Vec<bool>,
    /// Per element, the nearest element whose clip cuts **this element's own
    /// box**; following it from there gives the whole chain.
    clipped_by: Vec<Option<u32>>,
    /// Per page of the chapter, each box fragment's background image as it
    /// will be drawn, by the fragment's index in [`LayoutPage::boxes`]. See
    /// [`Effects::plan_backgrounds`].
    backgrounds: Vec<Vec<(usize, Plan)>>,
    /// Per element, its `text-shadow` list with each colour resolved and the
    /// offsets in CSS pixels, first on top; empty for nearly every element.
    text_shadows: Vec<Vec<(Color, f64, f64)>>,
    /// The alphas a translucent shadow colour needs — the colour's own alpha
    /// times its element's composed opacity, since an `/ExtGState`'s `/ca`
    /// replaces the one in force rather than multiplying it — with the
    /// property and the element each is for, so a refusal can be counted.
    shadow_alphas: Vec<(u16, &'static str, u32)>,
    /// Per element, the nearest element **at or above** it whose `transform`
    /// is not `none`; following [`Effects::transformed_above`] from there
    /// gives every transform its fragments are drawn under.
    transformed_by: Vec<Option<u32>>,
    /// Per element, the nearest transformed element strictly above it.
    transformed_above: Vec<Option<u32>>,
    /// Each transformed element's list and origin, by element.
    transforms: BTreeMap<u32, (Vec<Transform>, TransformOrigin)>,
    /// Links whose active area a transform above them would move: the
    /// annotation's rectangle is the run's, untransformed. Counted against
    /// `transform` by [`Effects::register`].
    turned_links: usize,
    /// The elements one of whose gradient fragments was not drawn — a
    /// geometry the book's numbers made infinite, or a shading or pattern
    /// the writer refused — by element, `u32::MAX` for a fragment nobody
    /// anchored. Counted against `background-image` by
    /// [`Effects::register`].
    refused_gradients: std::collections::BTreeSet<u32>,
}

/// What [`Effects::register`] could not register, by the property it was for,
/// counted by element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refused {
    /// Elements drawn opaque that asked for `opacity`.
    pub opacity: usize,
    /// Elements whose translucent `box-shadow` is drawn opaque.
    pub box_shadow: usize,
    /// Elements whose translucent `text-shadow` is drawn opaque.
    pub text_shadow: usize,
    /// Links inside a transformed element, whose active area is the run's
    /// rectangle before the transform.
    pub transform: usize,
    /// Elements a gradient of which was not drawn: its geometry was not
    /// finite, or the writer refused its shading or its pattern.
    pub background_image: usize,
}

/// A background image as one page draws it, in page points.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// One image, `w 0 0 h x y cm`: `no-repeat` on both axes, or a `space`
    /// with room for fewer than two.
    Once {
        /// The image's resource name.
        image: Vec<u8>,
        /// `(left, bottom, width, height)`.
        rect: (f64, f64, f64, f64),
    },
    /// A tiling pattern registered for this fragment, filling a rectangle.
    Tiled {
        /// The pattern's resource name.
        pattern: Vec<u8>,
        /// `(left, bottom, width, height)`: the painting area, or the one row
        /// or column of it an axis that does not repeat leaves.
        fill: (f64, f64, f64, f64),
    },
}

/// A background image's geometry on one fragment, `css-backgrounds-3` §2, in
/// page points: what [`Effects::plan_backgrounds`] turns into a [`Plan`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tiling {
    /// One image's width and height.
    pub tile: (f64, f64),
    /// The top-left corner of the image §2.6 positions.
    pub origin: (f64, f64),
    /// The distance from one image to the next on each axis: the image's own
    /// size for `repeat` and `round`, and that plus the gap for `space`.
    pub step: (f64, f64),
    /// Whether each axis repeats at all.
    pub repeats: (bool, bool),
    /// `(left, bottom, width, height)` of what is filled: the border box —
    /// `background-clip`'s initial value — cut to one row or column on an axis
    /// that does not repeat.
    pub fill: (f64, f64, f64, f64),
}

/// `css-backgrounds-3` §2's geometry for one image on one fragment.
///
/// The positioning area is the fragment's **padding box**
/// (`background-origin: padding-box`, the initial value) and the painting area
/// its border box; a box cut across pages positions its image against each
/// page's fragment, for [`BoxFragment::radius`]'s reason. `intrinsic` is the
/// image's own size in CSS pixels. `None` where §2.4 sizes the image to
/// nothing, which draws nothing.
#[must_use]
pub fn tiling(
    fragment: &BoxFragment,
    layer: &BackgroundLayer,
    intrinsic: (f64, f64),
    frame: &Frame,
) -> Option<Tiling> {
    let border = &fragment.border_width;
    let area = (
        fragment.x + border.left,
        fragment.y + border.top,
        (fragment.width - border.left - border.right).max(0.0),
        (fragment.height - border.top - border.bottom).max(0.0),
    );
    let (iw, ih) = intrinsic;
    if iw <= 0.0 || ih <= 0.0 {
        return None;
    }
    let resolve = |length: LengthPercentage, of: f64| match length {
        LengthPercentage::Px(px) => px,
        LengthPercentage::Percent(percent) => of * percent / 100.0,
    };
    // §2.4: `cover` and `contain` keep the ratio; one `auto` beside a stated
    // size takes the ratio; two take the image's own size.
    let (mut width, mut height) = match layer.size {
        BackgroundSize::Cover | BackgroundSize::Contain => {
            let across = area.2 / iw;
            let down = area.3 / ih;
            let scale = if layer.size == BackgroundSize::Cover {
                across.max(down)
            } else {
                across.min(down)
            };
            (iw * scale, ih * scale)
        }
        BackgroundSize::Explicit(w, h) => match (w, h) {
            (Some(w), Some(h)) => (resolve(w, area.2), resolve(h, area.3)),
            (Some(w), None) => {
                let w = resolve(w, area.2);
                (w, w * ih / iw)
            }
            (None, Some(h)) => {
                let h = resolve(h, area.3);
                (h * iw / ih, h)
            }
            (None, None) => (iw, ih),
        },
    };
    // §2.4's last paragraph: `round` rescales the image so a whole number of
    // them fills the area, and an `auto` other dimension follows the ratio.
    let auto_height = matches!(layer.size, BackgroundSize::Explicit(_, None));
    let auto_width = matches!(layer.size, BackgroundSize::Explicit(None, _));
    if layer.repeat.x == RepeatStyle::Round && width > 0.0 && area.2 > 0.0 {
        let rounded = area.2 / (area.2 / width).round().max(1.0);
        if layer.repeat.y != RepeatStyle::Round && auto_height {
            height *= rounded / width;
        }
        width = rounded;
    }
    if layer.repeat.y == RepeatStyle::Round && height > 0.0 && area.3 > 0.0 {
        let rounded = area.3 / (area.3 / height).round().max(1.0);
        if layer.repeat.x != RepeatStyle::Round && auto_width {
            width *= rounded / height;
        }
        height = rounded;
    }
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return None;
    }
    // §2.6: a percentage aligns that point of the image with that point of
    // the area; a length is the distance from the stated edge.
    let place = |offset: ComputedOffset, room: f64| {
        let from_start = match offset.offset {
            LengthPercentage::Px(px) => px,
            LengthPercentage::Percent(percent) => room * percent / 100.0,
        };
        if offset.from_end {
            room - from_start
        } else {
            from_start
        }
    };
    // §2.3's `space`: as many whole images as fit, the first and last against
    // the edges and the rest spread between; fewer than two is one image,
    // positioned.
    let axis = |style: RepeatStyle, start: f64, extent: f64, size: f64, offset: ComputedOffset| {
        match style {
            RepeatStyle::Space => {
                let count = (extent / size).floor();
                if count >= 2.0 {
                    let gap = (extent - count * size) / (count - 1.0);
                    (start, size + gap, true)
                } else {
                    (start + place(offset, extent - size), size, false)
                }
            }
            RepeatStyle::NoRepeat => (start + place(offset, extent - size), size, false),
            RepeatStyle::Repeat | RepeatStyle::Round => {
                (start + place(offset, extent - size), size, true)
            }
        }
    };
    let (x, step_x, repeat_x) = axis(layer.repeat.x, area.0, area.2, width, layer.position.x);
    let (y, step_y, repeat_y) = axis(layer.repeat.y, area.1, area.3, height, layer.position.y);
    // The painting area, cut to the image's own row or column on an axis that
    // does not repeat.
    let (mut left, mut top) = (fragment.x, fragment.y);
    let (mut right, mut bottom) = (fragment.x + fragment.width, fragment.y + fragment.height);
    if !repeat_x {
        left = left.max(x);
        right = right.min(x + width);
    }
    if !repeat_y {
        top = top.max(y);
        bottom = bottom.min(y + height);
    }
    if right <= left || bottom <= top {
        return None;
    }
    let pt = |px: f64| px * PX_TO_PT;
    Some(Tiling {
        tile: (pt(width), pt(height)),
        origin: (frame.x(x), frame.y(y)),
        step: (pt(step_x), pt(step_y)),
        repeats: (repeat_x, repeat_y),
        fill: (
            frame.x(left),
            frame.y(bottom),
            pt(right - left),
            pt(bottom - top),
        ),
    })
}

/// A gradient's geometry on one fragment: `css-images-3` §5.3's default
/// sizing, then [`tiling`]'s.
///
/// A gradient has no size and no ratio of its own, so an `auto` dimension is
/// the positioning area's and `cover` and `contain` are that area exactly;
/// the size that comes out is handed to [`tiling`] as if it were an image's
/// own, which places and repeats it as it does a raster one.
fn gradient_tiling(
    fragment: &BoxFragment,
    layer: &BackgroundLayer,
    frame: &Frame,
) -> Option<Tiling> {
    let border = &fragment.border_width;
    let area = (
        (fragment.width - border.left - border.right).max(0.0),
        (fragment.height - border.top - border.bottom).max(0.0),
    );
    let resolve = |length: LengthPercentage, of: f64| match length {
        LengthPercentage::Px(px) => px,
        LengthPercentage::Percent(percent) => of * percent / 100.0,
    };
    let size = match layer.size {
        BackgroundSize::Cover | BackgroundSize::Contain => area,
        BackgroundSize::Explicit(width, height) => (
            width.map_or(area.0, |width| resolve(width, area.0)),
            height.map_or(area.1, |height| resolve(height, area.1)),
        ),
    };
    let sized = BackgroundLayer {
        image: layer.image.clone(),
        repeat: layer.repeat,
        position: layer.position,
        size: BackgroundSize::Explicit(
            Some(LengthPercentage::Px(size.0)),
            Some(LengthPercentage::Px(size.1)),
        ),
    };
    tiling(fragment, &sized, size, frame)
}

/// What one gradient tile draws: a shading, through `cm` (an ellipse's
/// squash and its centre) then `sh`, or one colour where the gradient has
/// no extent to blend over.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientPaint {
    /// A PDF shading, in the space `matrix` makes of the tile's.
    Shaded {
        /// The axial or radial shading.
        shading: Box<Shading>,
        /// The `cm` before `sh`, where the shading's space is not the tile's.
        matrix: Option<[f64; 6]>,
    },
    /// One colour over the whole tile: a radial gradient whose ending shape
    /// has width and no height (§3.2.4 draws it as its last colour), or
    /// whose every stop is at or before its centre.
    Solid(Color),
}

/// §3.5.3's fix-up over stop positions already resolved to points along the
/// gradient line or ray: the first defaults to 0 and the last to `length`,
/// a position before an earlier one is moved up to it, and a run without
/// positions is spread evenly between the stops either side.
fn fix_up(positions: &[Option<f64>], length: f64) -> Vec<f64> {
    let count = positions.len();
    let mut fixed: Vec<Option<f64>> = positions.to_vec();
    if let Some(first) = fixed.first_mut() {
        first.get_or_insert(0.0);
    }
    if count > 1 {
        if let Some(last) = fixed.last_mut() {
            last.get_or_insert(length);
        }
    }
    let mut highest = f64::NEG_INFINITY;
    for position in fixed.iter_mut().flatten() {
        highest = highest.max(*position);
        *position = highest;
    }
    let mut out: Vec<f64> = Vec::with_capacity(count);
    let mut at = 0usize;
    while at < count {
        match fixed[at] {
            Some(position) => {
                out.push(position);
                at += 1;
            }
            None => {
                // A run of unpositioned stops: the first and last stops are
                // positioned, so one stands either side of it.
                let start = out.last().copied().unwrap_or(0.0);
                let mut end_at = at;
                while end_at < count && fixed[end_at].is_none() {
                    end_at += 1;
                }
                let end = fixed.get(end_at).copied().flatten().unwrap_or(start);
                let steps = (end_at - at + 1) as f64;
                for k in 1..=(end_at - at) {
                    out.push(start + (end - start) * k as f64 / steps);
                }
                at = end_at;
            }
        }
    }
    out
}

/// One stop's colour as `DeviceRGB` components.
fn components(color: Color) -> Vec<f64> {
    [color.r, color.g, color.b]
        .iter()
        .map(|channel| f64::from(*channel) / 255.0)
        .collect()
}

/// The stitching function a stop list is along `[from, to]` of a gradient
/// line or ray: one straight ramp per pair of stops that are apart, the
/// stitch at each join, so two stops at one place are a hard edge.
///
/// `stops` are `(position, colour)` in order, non-decreasing, and `to` is
/// greater than `from`.
fn ramp(stops: &[(f64, Color)], from: f64, to: f64) -> Option<Function> {
    let span = to - from;
    let mut pieces: Vec<(f64, Function)> = Vec::new();
    for pair in stops.windows(2) {
        let [(start, first), (end, second)] = pair else {
            continue;
        };
        if end - start <= 0.0 {
            continue;
        }
        pieces.push((
            (start - from) / span,
            Function::Exponential {
                domain: [0.0, 1.0],
                c0: components(*first),
                c1: components(*second),
                n: 1.0,
            },
        ));
    }
    match pieces.len() {
        0 => None,
        1 => pieces.pop().map(|(_, function)| function),
        _ => {
            let bounds: Vec<f64> = pieces.iter().skip(1).map(|(at, _)| *at).collect();
            let encode = vec![[0.0, 1.0]; pieces.len()];
            Some(Function::Stitching {
                domain: [0.0, 1.0],
                functions: pieces.into_iter().map(|(_, function)| function).collect(),
                bounds,
                encode,
            })
        }
    }
}

/// The stops of `gradient` placed along a line or ray `length` points long,
/// fixed up; a pixel position is converted to points, a percentage is of
/// `length`.
fn placed_stops(stops: &[ColorStop<LengthPercentage>], length: f64) -> Vec<(f64, Color)> {
    let positions: Vec<Option<f64>> = stops
        .iter()
        .map(|stop| {
            stop.position.map(|position| match position {
                LengthPercentage::Px(px) => px * PX_TO_PT,
                LengthPercentage::Percent(percent) => length * percent / 100.0,
            })
        })
        .collect();
    fix_up(&positions, length)
        .into_iter()
        .zip(stops.iter().map(|stop| stop.color))
        .collect()
}

/// How far, in points, the end stops' colours are held past them: see
/// [`held`].
const HARD_EDGE: f64 = 1e-3;

/// `stops` with the first colour held for [`HARD_EDGE`] before the first
/// stop — where `before` allows it — and the last colour for as long after
/// the last.
///
/// A PDF shading extends the value its function has at either end of its
/// domain, and two stops at one place are an edge (§3.5.3), not a ramp: a
/// gradient whose first two stops coincide is the first colour up to them
/// and the second after, and its function's value at the start must be the
/// first colour, which the ramp alone — the coincident pair contributing no
/// piece — would not make it. Held, every coincident pair is inside the
/// domain, a stitch, and a gradient whose every stop is at one place is an
/// edge there rather than nothing.
fn held(stops: &[(f64, Color)], before: bool) -> Vec<(f64, Color)> {
    let mut out: Vec<(f64, Color)> = Vec::with_capacity(stops.len() + 2);
    if let (true, Some(&(at, color))) = (before, stops.first()) {
        out.push((at - HARD_EDGE, color));
    }
    out.extend_from_slice(stops);
    if let Some(&(at, color)) = stops.last() {
        out.push((at + HARD_EDGE, color));
    }
    out
}

/// `sqrt(x² + y²)` from IEEE 754's basic operations and `sqrt`, which every
/// target rounds alike (ruling 4) — a platform `hypot` need not — scaled by
/// the larger magnitude first, so a length near `f64::MAX` squares without
/// overflowing. A NaN stays a NaN.
fn hypot(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let (x, y) = (x.abs(), y.abs());
    let big = x.max(y);
    if big == 0.0 || big.is_infinite() {
        return big;
    }
    let (a, b) = (x / big, y / big);
    big * (a * a + b * b).sqrt()
}

/// Whether every number is finite: what a shading's geometry and a `cm` must
/// be before they are written, since the writer spells a non-finite real
/// `0` and a pattern's content is written here, past its checks.
fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

/// How tall, per point of the tile's height, the ellipse is that stands in
/// for §3.2.4's ending shape of no width: tall enough that within the tile a
/// point's ring is its horizontal distance from the centre to a
/// ten-thousandth of a point.
const TALL: f64 = 1e4;

/// `gradient` as one tile `width` by `height` points draws it, in the tile's
/// space: its origin the tile's bottom-left corner, y up.
///
/// `None` where the book's numbers make a geometry that is not finite — a
/// stop at `1e308%`, a centre beyond any page — which the caller counts
/// rather than writes (review of lane 8C).
///
/// # Linear, §3.1
///
/// The gradient line passes through the tile's centre at the angle given —
/// clockwise from up, and for a corner the angle that puts the other two
/// corners on the 50% line — and is `|w sin A| + |h cos A|` long, so its
/// ends' perpendiculars pass through two corners. The PDF axis is the
/// stretch of that line from the first stop to the last, extended both
/// ways, so a stop before 0% or past 100% is where it says, and the end
/// colours held a hair past the end stops ([`held`]).
///
/// **The angle's sine and cosine are the SVG crate's** (`rotation`, through
/// `tinker-pdf-math`), as `transform: rotate()`'s are, and a corner's are
/// the box's sides over its diagonal: a platform `sin` or `atan2` rounds its
/// last bit its own way, and the shading's coordinates are bytes in the file
/// (ruling 4; review of lane 8C).
///
/// # Radial, §3.2
///
/// The ending shape's radii come from its size (§3.2.1, a corner keyword's
/// ellipse keeping the side keyword's ratio), and a PDF radial shading
/// blends between two concentric circles in a space squashed by `ry / rx`,
/// so an ellipse is a circle of radius `rx` there. A stop before the centre
/// is not a circle PDF can draw: the colour at the centre is interpolated
/// and the stops before it dropped.
///
/// §3.2.4's degenerate shapes are its three cases (review of lane 8C): a
/// circle of no radius is a circle of a vanishing one, so a stop placed by a
/// length still rings out from the centre and one placed by a percentage is
/// at it; a shape of no width is an ellipse of vanishing width and great
/// height — a horizontal gradient mirrored about the centre, its percentages
/// at the centre too — drawn as the circles of a space stretched [`TALL`]
/// times the tile's height; and only a shape of no height, with width, is
/// its last colour throughout.
pub fn gradient_paint(
    gradient: &Gradient<LengthPercentage>,
    (width, height): (f64, f64),
) -> Option<GradientPaint> {
    match gradient.shape {
        GradientShape::Linear(direction) => {
            let (sin, cos) = match direction {
                LinearDirection::Angle(degrees) => {
                    // A whole turn off first, which `%` does exactly, so a
                    // huge angle is the angle it names rather than whatever
                    // a reduction of a huge argument makes of it.
                    let [cos, sin, ..] = rotation(degrees % 360.0);
                    (sin, cos)
                }
                LinearDirection::Corner { right, bottom } => {
                    // The angle whose sine is `across · h` and cosine
                    // `−down · w`, over the diagonal: no `atan2` and back.
                    let across = if right { 1.0 } else { -1.0 };
                    let down = if bottom { 1.0 } else { -1.0 };
                    let diagonal = hypot(width, height);
                    if diagonal > 0.0 {
                        (across * height / diagonal, -down * width / diagonal)
                    } else {
                        (0.0, 1.0)
                    }
                }
            };
            let length = (width * sin).abs() + (height * cos).abs();
            let stops = held(&placed_stops(&gradient.stops, length), true);
            let (first, to) = (stops.first()?.0, stops.last()?.0);
            // CSS's down is the tile's up turned over: the line's direction
            // in the tile's space is (sin, cos).
            let start = (
                width / 2.0 - sin * length / 2.0,
                height / 2.0 - cos * length / 2.0,
            );
            let point = |along: f64| (start.0 + sin * along, start.1 + cos * along);
            let (a, b) = (point(first), point(to));
            let positions: Vec<f64> = stops.iter().map(|(at, _)| *at).collect();
            if !all_finite(&[a.0, a.1, b.0, b.1, to - first]) || !all_finite(&positions) {
                return None;
            }
            let function = ramp(&stops, first, to)?;
            Some(GradientPaint::Shaded {
                shading: Box::new(Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [a.0, a.1, b.0, b.1],
                    function,
                    extend: (true, true),
                }),
                matrix: None,
            })
        }
        GradientShape::Radial(radial) => {
            let resolve = |length: LengthPercentage, of: f64| match length {
                LengthPercentage::Px(px) => px * PX_TO_PT,
                LengthPercentage::Percent(percent) => of * percent / 100.0,
            };
            let place = |offset: GradientOffset<LengthPercentage>, of: f64| {
                let along = resolve(offset.offset, of);
                if offset.from_end {
                    of - along
                } else {
                    along
                }
            };
            // The centre, from the tile's top left in CSS's orientation.
            let (cx, cy) = (place(radial.at[0], width), place(radial.at[1], height));
            let sides = (
                cx.abs().min((width - cx).abs()),
                cy.abs().min((height - cy).abs()),
            );
            let far_sides = (
                cx.abs().max((width - cx).abs()),
                cy.abs().max((height - cy).abs()),
            );
            let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
                .map(|(x, y): (f64, f64)| ((x - cx).abs(), (y - cy).abs()));
            let distance = |(dx, dy): (f64, f64)| hypot(dx, dy);
            let nearest = corners
                .iter()
                .copied()
                .min_by(|p, q| distance(*p).total_cmp(&distance(*q)))?;
            let farthest = corners
                .iter()
                .copied()
                .max_by(|p, q| distance(*p).total_cmp(&distance(*q)))?;
            // §3.2.1: a corner keyword's ellipse has the ratio the side
            // keyword's would, and passes through that corner.
            let through = |(dx, dy): (f64, f64), (sx, sy): (f64, f64)| {
                if sx <= 0.0 || sy <= 0.0 {
                    return (0.0, 0.0);
                }
                let ratio = sx / sy;
                let ry = hypot(dx / ratio, dy);
                (ratio * ry, ry)
            };
            let (rx, ry) = match (radial.size, radial.circle) {
                (RadialSize::ClosestSide, true) => {
                    let r = sides.0.min(sides.1);
                    (r, r)
                }
                (RadialSize::FarthestSide, true) => {
                    let r = far_sides.0.max(far_sides.1);
                    (r, r)
                }
                (RadialSize::ClosestCorner, true) => (distance(nearest), distance(nearest)),
                (RadialSize::FarthestCorner, true) => (distance(farthest), distance(farthest)),
                (RadialSize::ClosestSide, false) => sides,
                (RadialSize::FarthestSide, false) => far_sides,
                (RadialSize::ClosestCorner, false) => through(nearest, sides),
                (RadialSize::FarthestCorner, false) => through(farthest, far_sides),
                (RadialSize::Explicit(x, y), _) => (resolve(x, width), resolve(y, height)),
            };
            if !all_finite(&[cx, cy, rx, ry]) {
                return None;
            }
            // The ray percentages are of, and how the circles of the
            // shading's space are squashed into the ending shape (§3.2.4).
            let (ray, squash) = if rx > 0.0 && ry > 0.0 {
                (rx, ry / rx)
            } else if radial.circle && rx <= 0.0 {
                (0.0, 1.0)
            } else if rx <= 0.0 {
                (0.0, TALL * height.max(1.0))
            } else {
                let last = *placed_stops(&gradient.stops, rx).last()?;
                return Some(GradientPaint::Solid(last.1));
            };
            let stops = placed_stops(&gradient.stops, ray);
            let last = *stops.last()?;
            if last.0 <= 0.0 {
                return Some(GradientPaint::Solid(last.1));
            }
            // The colour at the centre, where a stop lies before it.
            let mut kept: Vec<(f64, Color)> = Vec::with_capacity(stops.len());
            for pair in stops.windows(2) {
                let [(start, first), (end, second)] = pair else {
                    continue;
                };
                if *start < 0.0 && *end > 0.0 && kept.is_empty() {
                    let t = -start / (end - start);
                    let mix = |a: u8, b: u8| {
                        (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8
                    };
                    kept.push((
                        0.0,
                        Color {
                            r: mix(first.r, second.r),
                            g: mix(first.g, second.g),
                            b: mix(first.b, second.b),
                            a: 255,
                        },
                    ));
                }
            }
            kept.extend(stops.iter().copied().filter(|(at, _)| *at >= 0.0));
            // A radius is not negative: the first colour is held inward only
            // where there is room for it.
            let room = kept.first().is_some_and(|(at, _)| *at >= HARD_EDGE);
            let kept = held(&kept, room);
            let (first, to) = (kept.first()?.0, kept.last()?.0);
            // The centre turned over into the tile's up, and the circle
            // squashed into the ellipse.
            let matrix = [1.0, 0.0, 0.0, squash, cx, height - cy];
            let positions: Vec<f64> = kept.iter().map(|(at, _)| *at).collect();
            if !all_finite(&matrix) || !all_finite(&positions) || !(to - first).is_finite() {
                return None;
            }
            let function = ramp(&kept, first, to)?;
            Some(GradientPaint::Shaded {
                shading: Box::new(Shading::Radial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, first, 0.0, 0.0, to],
                    function,
                    extend: (true, true),
                }),
                matrix: Some(matrix),
            })
        }
    }
}

/// One gradient tile as a tiling pattern filling the painting area: the
/// shading registered as a resource the cell's `sh` names, clipped to the
/// cell, or the cell filled with one colour.
///
/// A pattern even where the gradient does not repeat, for the raster path's
/// tiled reason: 8.7.3.1 maps a pattern onto the page's default space, so the
/// box's transform is in its matrix, and one path is one place for a
/// gradient to be placed wrong.
///
/// `None` where the gradient's geometry is not finite or the writer refuses
/// its shading or its pattern; [`Effects::plan_backgrounds`] counts each
/// such element against `background-image` (ruling 10).
fn gradient_plan(
    builder: &mut DocumentBuilder,
    gradient: &Gradient<LengthPercentage>,
    geometry: &Tiling,
    turned: [f64; 6],
    counter: &mut usize,
) -> Option<Plan> {
    let (width, height) = geometry.tile;
    let paint = gradient_paint(gradient, (width, height))?;
    let content = match paint {
        GradientPaint::Shaded { shading, matrix } => {
            let name = format!("BgSh{counter}").into_bytes();
            if !builder.add_shading(&name, &shading) {
                return None;
            }
            let squash = matrix.map_or(String::new(), |m| {
                format!("{} {} {} {} {} {} cm ", m[0], m[1], m[2], m[3], m[4], m[5])
            });
            let mut content = format!("0 0 {width} {height} re W n {squash}/").into_bytes();
            content.extend_from_slice(&name);
            content.extend_from_slice(b" sh");
            content
        }
        GradientPaint::Solid(color) => format!(
            "{} {} {} rg 0 0 {width} {height} re f",
            f64::from(color.r) / 255.0,
            f64::from(color.g) / 255.0,
            f64::from(color.b) / 255.0
        )
        .into_bytes(),
    };
    let pattern = format!("BgP{counter}").into_bytes();
    *counter += 1;
    let registered = builder.add_tiling_pattern(
        &pattern,
        &TilingPattern {
            bbox: [0.0, 0.0, width, height],
            x_step: geometry.step.0,
            y_step: geometry.step.1,
            matrix: Some(concat(
                [
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    geometry.origin.0,
                    geometry.origin.1 - height,
                ],
                turned,
            )),
            tiling_type: TilingType::ConstantSpacing,
            content: &content,
        },
    );
    registered.then_some(Plan::Tiled {
        pattern,
        fill: geometry.fill,
    })
}

/// The quantum of [`Effects`]' alphas: ten thousand steps between clear and
/// opaque.
const ALPHA_STEPS: f64 = 10_000.0;

impl Effects {
    /// Every element's composed opacity and clip chain, from the cascade's
    /// tree, the document's, and the chapter's laid-out pages.
    #[must_use]
    pub fn of(dom: &Dom, styles: &StyleTree, pages: &[LayoutPage]) -> Effects {
        let mut composed: Vec<f64> = Vec::with_capacity(dom.nodes.len());
        for (at, node) in dom.nodes.iter().enumerate() {
            let own = styles.styles.get(at).map_or(1.0, |style| style.opacity);
            // `parent` is always less than the node's own index (the cascade
            // refuses a tree where it is not), so the parent's product is
            // already here.
            let above = node
                .parent
                .and_then(|parent| composed.get(parent).copied())
                .unwrap_or(1.0);
            composed.push((above * own).clamp(0.0, 1.0));
        }
        let mut clips = vec![false; dom.nodes.len()];
        for page in pages {
            for clip in &page.clips {
                if let Some(slot) = clips.get_mut(clip.anchor as usize) {
                    *slot = true;
                }
            }
        }
        // Two values per element, parents first: whose clip cuts its own box,
        // and whose cuts an absolutely positioned descendant that has it — or
        // an ancestor of it — as containing block (§10.1's nearest positioned
        // ancestor, the page where there is none).
        let mut clipped_by: Vec<Option<u32>> = Vec::with_capacity(dom.nodes.len());
        let mut for_absolute: Vec<Option<u32>> = Vec::with_capacity(dom.nodes.len());
        for (at, node) in dom.nodes.iter().enumerate() {
            let position = styles
                .styles
                .get(at)
                .map_or(Position::Static, |style| style.position);
            let inner = |of: usize| match clips.get(of) {
                Some(true) => u32::try_from(of).ok(),
                _ => clipped_by.get(of).copied().flatten(),
            };
            let by = match position {
                Position::Fixed => None,
                Position::Absolute => node
                    .parent
                    .and_then(|parent| for_absolute.get(parent).copied().flatten()),
                _ => node.parent.and_then(inner),
            };
            let own = if clips[at] {
                u32::try_from(at).ok()
            } else {
                by
            };
            // `css-transforms-1` §2: a transformed box is a containing block
            // for its absolutely positioned descendants, as a positioned one
            // is — the layout places them so, and their clips follow.
            let transformed = styles
                .styles
                .get(at)
                .is_some_and(|style| !style.transform.is_empty());
            let absolute = match position {
                Position::Static if !transformed => node
                    .parent
                    .and_then(|parent| for_absolute.get(parent).copied().flatten()),
                _ => own,
            };
            clipped_by.push(by);
            for_absolute.push(absolute);
        }
        let alpha: Vec<u16> = composed
            .into_iter()
            .map(|alpha| (alpha * ALPHA_STEPS).round() as u16)
            .collect();
        let mut text_shadows: Vec<Vec<(Color, f64, f64)>> = Vec::with_capacity(alpha.len());
        let mut shadow_alphas = Vec::new();
        for (at, steps) in alpha.iter().enumerate() {
            let Some(style) = styles.styles.get(at) else {
                text_shadows.push(Vec::new());
                continue;
            };
            let element = u32::try_from(at).unwrap_or(u32::MAX);
            for (property, list) in [
                ("box-shadow", &style.box_shadow),
                ("text-shadow", &style.text_shadow),
            ] {
                for shadow in list {
                    let colour = shadow.color.unwrap_or(style.color);
                    if colour.a < u8::MAX {
                        shadow_alphas.push((translucent(*steps, colour), property, element));
                    }
                }
            }
            text_shadows.push(
                style
                    .text_shadow
                    .iter()
                    .map(|shadow| (shadow.color.unwrap_or(style.color), shadow.x, shadow.y))
                    .collect(),
            );
        }
        // Parents first, as above.
        let mut transformed_by: Vec<Option<u32>> = Vec::with_capacity(dom.nodes.len());
        let mut transformed_above: Vec<Option<u32>> = Vec::with_capacity(dom.nodes.len());
        let mut transforms = BTreeMap::new();
        let mut turned_links = 0usize;
        for (at, node) in dom.nodes.iter().enumerate() {
            let above = node
                .parent
                .and_then(|parent| transformed_by.get(parent).copied().flatten());
            let own = match styles.styles.get(at) {
                Some(style) if !style.transform.is_empty() => {
                    let element = u32::try_from(at).ok();
                    if let Some(element) = element {
                        transforms
                            .insert(element, (style.transform.clone(), style.transform_origin));
                    }
                    element
                }
                _ => above,
            };
            if own.is_some() && node.is_html() && node.name == "a" && node.attr("href").is_some() {
                turned_links += 1;
            }
            transformed_by.push(own);
            transformed_above.push(above);
        }
        Effects {
            alpha,
            clips,
            clipped_by,
            backgrounds: Vec::new(),
            text_shadows,
            shadow_alphas,
            transformed_by,
            transformed_above,
            transforms,
            turned_links,
            refused_gradients: std::collections::BTreeSet::new(),
        }
    }

    /// Every transformed element's own matrix on one page, in page points,
    /// about its origin: `None` for one that flattens the plane, which draws
    /// nothing. Only the elements with a fragment on the page are here — a
    /// non-replaced inline box has none, and `css-transforms-1` §3 does not
    /// transform it — and the reference box is that fragment, so a box cut
    /// across pages turns each page's slice about the slice's own origin.
    fn locals(&self, laid: &LayoutPage, frame: &Frame) -> BTreeMap<u32, Option<[f64; 6]>> {
        let mut out = BTreeMap::new();
        if self.transforms.is_empty() {
            return out;
        }
        let boxes = laid
            .boxes
            .iter()
            .map(|b| (b.anchor, (b.x, b.y, b.width, b.height)));
        let replaced = laid
            .replaced
            .iter()
            .map(|r| (r.anchor, (r.x, r.y, r.width, r.height)));
        for (anchor, rect) in boxes.chain(replaced) {
            let Some(element) = anchor else {
                continue;
            };
            if out.contains_key(&element) {
                continue;
            }
            if let Some((list, origin)) = self.transforms.get(&element) {
                out.insert(element, local_matrix(list, *origin, rect, frame));
            }
        }
        out
    }

    /// The transformed elements a fragment anchored here is drawn under that
    /// have a matrix in `locals`, **nearest first**.
    fn turns(&self, locals: &BTreeMap<u32, Option<[f64; 6]>>, anchor: Option<u32>) -> Vec<u32> {
        let mut out = Vec::new();
        if locals.is_empty() {
            return out;
        }
        let mut next =
            anchor.and_then(|at| self.transformed_by.get(at as usize).copied().flatten());
        // Each step is to a strict ancestor, whose index is lower, so the walk
        // ends within the tree's depth.
        while let Some(element) = next {
            if locals.contains_key(&element) {
                out.push(element);
            }
            next = self
                .transformed_above
                .get(element as usize)
                .copied()
                .flatten()
                .filter(|above| *above < element);
        }
        out
    }

    /// The whole matrix a fragment anchored here is drawn under: its
    /// transformed ancestors' own matrices, innermost applied first. `None`
    /// where one of them flattens the plane; the identity where there are none.
    fn composed(
        &self,
        locals: &BTreeMap<u32, Option<[f64; 6]>>,
        anchor: Option<u32>,
    ) -> Option<[f64; 6]> {
        let mut out = IDENTITY;
        for element in self.turns(locals, anchor) {
            out = concat(out, locals.get(&element).copied().flatten()?);
        }
        Some(out)
    }

    /// Plans every background image a chapter's pages draw, and registers a
    /// tiling pattern for each one that repeats — **before the chapter's first
    /// page begins**, for `begin_page`'s snapshot rule, which is why the plan
    /// is made here and not while drawing.
    ///
    /// `image` names a reference's registered image and its size in CSS
    /// pixels, or nothing for one that did not resolve; `counter` numbers the
    /// patterns across the whole document. A pattern the writer refuses leaves
    /// its fragment without an image rather than naming a resource the page
    /// does not hold.
    pub fn plan_backgrounds<'i>(
        &mut self,
        builder: &mut DocumentBuilder,
        pages: &[LayoutPage],
        frame: &Frame,
        image: impl Fn(&ImageRef) -> Option<(&'i [u8], (f64, f64))>,
        counter: &mut usize,
    ) {
        let mut refused = std::collections::BTreeSet::new();
        let planned: Vec<Vec<(usize, Plan)>> = pages
            .iter()
            .map(|page| {
                let mut plans = Vec::new();
                let locals = self.locals(page, frame);
                for (index, fragment) in page.boxes.iter().enumerate() {
                    let Some(layer) = &fragment.image else {
                        continue;
                    };
                    let reference = match &layer.image {
                        Image::Url(reference) => reference,
                        Image::Gradient(gradient) => {
                            let Some(geometry) = gradient_tiling(fragment, layer, frame) else {
                                continue;
                            };
                            let Some(turned) = self.composed(&locals, fragment.anchor) else {
                                continue;
                            };
                            match gradient_plan(builder, gradient, &geometry, turned, counter) {
                                Some(plan) => plans.push((index, plan)),
                                None => {
                                    refused.insert(fragment.anchor.unwrap_or(u32::MAX));
                                }
                            }
                            continue;
                        }
                    };
                    let Some((name, intrinsic)) = image(reference) else {
                        continue;
                    };
                    let Some(geometry) = tiling(fragment, layer, intrinsic, frame) else {
                        continue;
                    };
                    let (width, height) = geometry.tile;
                    if !geometry.repeats.0 && !geometry.repeats.1 {
                        plans.push((
                            index,
                            Plan::Once {
                                image: name.to_vec(),
                                rect: (
                                    geometry.origin.0,
                                    geometry.origin.1 - height,
                                    width,
                                    height,
                                ),
                            },
                        ));
                        continue;
                    }
                    // 8.7.3.1 maps a pattern onto the page's **default**
                    // space, which no `cm` reaches: a transformed box's
                    // pattern carries its transform itself. One that flattens
                    // the plane draws nothing, and needs no pattern.
                    let Some(turned) = self.composed(&locals, fragment.anchor) else {
                        continue;
                    };
                    let pattern = format!("BgP{counter}").into_bytes();
                    *counter += 1;
                    let mut content = format!("{width} 0 0 {height} 0 0 cm /").into_bytes();
                    content.extend_from_slice(name);
                    content.extend_from_slice(b" Do");
                    // 8.7.3.1: the pattern's matrix maps its cell, whose origin
                    // is the image's bottom-left corner, onto the page's default
                    // space, which is the space this painter draws in.
                    let registered = builder.add_tiling_pattern(
                        &pattern,
                        &TilingPattern {
                            bbox: [0.0, 0.0, width, height],
                            x_step: geometry.step.0,
                            y_step: geometry.step.1,
                            matrix: Some(concat(
                                [
                                    1.0,
                                    0.0,
                                    0.0,
                                    1.0,
                                    geometry.origin.0,
                                    geometry.origin.1 - height,
                                ],
                                turned,
                            )),
                            tiling_type: TilingType::ConstantSpacing,
                            content: &content,
                        },
                    );
                    if registered {
                        plans.push((
                            index,
                            Plan::Tiled {
                                pattern,
                                fill: geometry.fill,
                            },
                        ));
                    }
                }
                plans
            })
            .collect();
        self.backgrounds = planned;
        self.refused_gradients = refused;
    }

    /// This chapter's effects on one laid-out page, the `offset`-th of the
    /// chapter.
    #[must_use]
    pub fn on<'a>(&'a self, laid: &'a LayoutPage, frame: &'a Frame, offset: usize) -> OnPage<'a> {
        OnPage {
            effects: self,
            clips: &laid.clips,
            frame,
            backgrounds: self.backgrounds.get(offset).map_or(&[], Vec::as_slice),
            locals: self.locals(laid, frame),
        }
    }

    /// The composed alpha of the element a fragment was anchored to, in
    /// steps; opaque for a fragment nobody anchored.
    fn steps(&self, anchor: Option<u32>) -> u16 {
        anchor
            .and_then(|at| self.alpha.get(at as usize).copied())
            .unwrap_or(ALPHA_STEPS as u16)
    }

    /// Registers one `/ExtGState` per distinct alpha below one. Returns how
    /// many **elements** were left opaque because the writer refused their
    /// alpha — an archival profile that forbids transparency refuses every
    /// one — so the caller can say so, by element, as it says every other
    /// property it did not honour.
    pub fn register(&self, builder: &mut DocumentBuilder) -> Refused {
        let mut wanted: Vec<u16> = self
            .alpha
            .iter()
            .copied()
            .chain(self.shadow_alphas.iter().map(|(steps, _, _)| *steps))
            .filter(|steps| f64::from(*steps) < ALPHA_STEPS)
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut refused = Refused::default();
        // A set and not a list: a refused alpha is every element of an
        // archival book whose inherited shadow is translucent, and a `contains`
        // per element would be quadratic in the elements.
        let mut shadowed: std::collections::BTreeSet<(&'static str, u32)> =
            std::collections::BTreeSet::new();
        for steps in wanted {
            let alpha = f64::from(steps) / ALPHA_STEPS;
            let state = ExtGState {
                fill_alpha: Some(alpha),
                stroke_alpha: Some(alpha),
                ..ExtGState::default()
            };
            if !builder.add_ext_gstate(&alpha_name(steps), &state) {
                refused.opacity += self.alpha.iter().filter(|at| **at == steps).count();
                for (_, property, element) in self
                    .shadow_alphas
                    .iter()
                    .filter(|(wanted, _, _)| *wanted == steps)
                {
                    shadowed.insert((property, *element));
                }
            }
        }
        refused.transform = self.turned_links;
        refused.background_image = self.refused_gradients.len();
        refused.box_shadow = shadowed.iter().filter(|(p, _)| *p == "box-shadow").count();
        refused.text_shadow = shadowed.iter().filter(|(p, _)| *p == "text-shadow").count();
        refused
    }

    /// Closes what [`OnPage::open`] opened, if it opened anything.
    pub(crate) fn close(page: &mut PageBuilder, opened: bool) {
        if opened {
            page.raw(b"Q");
        }
    }
}

/// What a fragment drawn on one page is wrapped in: its element's composed
/// alpha, the clips of the elements above it, and the transforms. See
/// [`Effects`].
#[derive(Clone, Debug)]
pub struct OnPage<'a> {
    effects: &'a Effects,
    clips: &'a [ClipFragment],
    frame: &'a Frame,
    backgrounds: &'a [(usize, Plan)],
    /// [`Effects::locals`] for this page.
    locals: BTreeMap<u32, Option<[f64; 6]>>,
}

impl OnPage<'_> {
    /// Sets the alpha a translucent shadow colour needs, inside a `q` the
    /// caller has opened; nothing for an opaque one. Where the writer refused
    /// the resource nothing is set and the shadow is drawn opaque — counted by
    /// [`Effects::register`].
    fn shadow_alpha(&self, page: &mut PageBuilder, anchor: Option<u32>, colour: Color) {
        if colour.a < u8::MAX {
            let steps = translucent(self.effects.steps(anchor), colour);
            page.set_ext_gstate(&alpha_name(steps));
        }
    }

    /// The `text-shadow` of the element a run's characters came from.
    fn text_shadows(&self, anchor: Option<u32>) -> &[(Color, f64, f64)] {
        anchor
            .and_then(|at| self.effects.text_shadows.get(at as usize))
            .map_or(&[], Vec::as_slice)
    }

    /// The background image the `index`-th box fragment of the page draws.
    fn background(&self, index: usize) -> Option<&Plan> {
        self.backgrounds
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, plan)| plan)
    }

    /// Opens what a fragment anchored here needs, returning whether anything
    /// was opened and so has to be closed with [`Effects::close`].
    ///
    /// `inside` is whether the fragment is the element's **content** — its
    /// text — rather than its own box, which an element's own clip does not
    /// cut: `css-overflow-3` clips the content to the padding box and leaves
    /// the background and border where they are.
    ///
    /// The clips and the transforms are written **outermost first**, by
    /// element: an ancestor's index is below its descendants', and a clipping
    /// element's padding box is in its own coordinates — inside every
    /// transform above it, and inside its own. So each transform is its own
    /// matrix, `cm`'d after the ones above it, and the page's matrix at the
    /// fragment is their product without a matrix ever being inverted.
    pub(crate) fn open(&self, page: &mut PageBuilder, anchor: Option<u32>, inside: bool) -> bool {
        let chain = self.chain(anchor, inside);
        let turns = self.effects.turns(&self.locals, anchor);
        let steps = self.effects.steps(anchor);
        let translucent = f64::from(steps) < ALPHA_STEPS;
        if chain.is_empty() && turns.is_empty() && !translucent {
            return false;
        }
        page.raw(b"q");
        let mut order: Vec<(u32, bool)> = turns
            .iter()
            .map(|element| (*element, false))
            .chain(chain.iter().map(|element| (*element, true)))
            .collect();
        // `false` before `true`: an element's own transform before its clip.
        order.sort_unstable();
        for (element, clips) in order {
            if clips {
                self.clip_to(page, element);
                continue;
            }
            match self.locals.get(&element).copied().flatten() {
                Some(matrix) if matrix != IDENTITY => {
                    let [a, b, c, d, e, f] = matrix;
                    page.raw(format!("{a} {b} {c} {d} {e} {f} cm").as_bytes());
                }
                Some(_) => {}
                // A transform that flattens the plane draws nothing of what is
                // inside it (`css-transforms-1` §6.1's non-invertible matrix).
                None => page.raw(b"0 0 0 0 re W n"),
            }
        }
        // False when the resource was refused at registration — an archival
        // profile's — and then the fragment is drawn opaque rather than the
        // page naming a resource it does not carry.
        if !translucent
            || page.set_ext_gstate(&alpha_name(steps))
            || !chain.is_empty()
            || !turns.is_empty()
        {
            return true;
        }
        page.raw(b"Q");
        false
    }

    /// The matrix a fragment anchored here is drawn under on the page: the
    /// product of the `cm`s [`OnPage::open`] writes for it, each transformed
    /// element's own matrix but those that flatten the plane, which write a
    /// clip and no `cm`, and the identity where there are none. What a reader
    /// of the page measures the fragment's text in ([`Fonts::set_readings`]).
    pub(crate) fn matrix(&self, anchor: Option<u32>) -> [f64; 6] {
        let mut out = IDENTITY;
        // Nearest first, so each is applied in the space of the ones above it.
        for element in self.effects.turns(&self.locals, anchor) {
            match self.locals.get(&element).copied().flatten() {
                Some(matrix) if matrix != IDENTITY => out = concat(out, matrix),
                _ => {}
            }
        }
        out
    }

    /// The clipping elements above a fragment, nearest first.
    fn chain(&self, anchor: Option<u32>, inside: bool) -> Vec<u32> {
        let effects = self.effects;
        let Some(at) = anchor
            .map(|at| at as usize)
            .filter(|at| *at < effects.clipped_by.len())
        else {
            return Vec::new();
        };
        let mut next = if inside && effects.clips[at] {
            u32::try_from(at).ok()
        } else {
            effects.clipped_by[at]
        };
        let mut chain = Vec::new();
        // Each step is to a strict ancestor, whose index is lower, so the walk
        // ends within the tree's depth.
        while let Some(element) = next {
            chain.push(element);
            next = effects
                .clipped_by
                .get(element as usize)
                .copied()
                .flatten()
                .filter(|above| *above < element);
        }
        chain
    }

    /// Intersects the clip with one element's padding boxes on this page —
    /// the union of them, for a box this page holds more than one fragment of
    /// — or with nothing at all where this page holds none: the element's
    /// content is then wholly outside its box, which is what clipping it to
    /// its box removes.
    fn clip_to(&self, page: &mut PageBuilder, element: u32) {
        let mut path = String::new();
        for clip in self.clips.iter().filter(|clip| clip.anchor == element) {
            if !path.is_empty() {
                path.push(' ');
            }
            path.push_str(&clip_path(clip, self.frame));
        }
        if path.is_empty() {
            path.push_str("0 0 0 0 re");
        }
        path.push_str(" W n");
        page.raw(path.as_bytes());
    }
}

/// One clip's padding box as a path in page points: a rectangle, or the
/// rounded shape where it has curved corners. An axis it does not clip is the
/// page's whole extent in that axis.
fn clip_path(clip: &ClipFragment, frame: &Frame) -> String {
    let (page_width, page_height) = frame.page;
    let (left, right) = if clip.x.is_finite() && clip.width.is_finite() {
        (frame.x(clip.x), frame.x(clip.x + clip.width))
    } else {
        (0.0, page_width)
    };
    let (top, bottom) = if clip.y.is_finite() && clip.height.is_finite() {
        (frame.y(clip.y), frame.y(clip.y + clip.height))
    } else {
        (page_height, 0.0)
    };
    let rect = (
        left,
        bottom,
        (right - left).max(0.0),
        (top - bottom).max(0.0),
    );
    let radii = clip
        .radius
        .map(|(horizontal, vertical)| (horizontal * PX_TO_PT, vertical * PX_TO_PT));
    if radii
        .iter()
        .any(|(horizontal, vertical)| *horizontal > 0.0 && *vertical > 0.0)
    {
        return rounded_path(rect, radii);
    }
    format!("{} {} {} {} re", rect.0, rect.1, rect.2, rect.3)
}

/// One transformed element's own matrix in page points, `css-transforms-1`
/// §6's *"transformation matrix"*: translate to the origin, the list leftmost
/// outermost, translate back — with the list's matrix, which is written in CSS
/// pixels on a downward axis, carried onto the page's upward one. `rect` is
/// the reference box, the element's border box on this page, in CSS pixels.
///
/// `None` where the product does not invert (`scale(0)`, a `matrix()` of
/// zeros) or is not finite, and the element then draws nothing.
fn local_matrix(
    list: &[Transform],
    origin: TransformOrigin,
    rect: (f64, f64, f64, f64),
    frame: &Frame,
) -> Option<[f64; 6]> {
    let (x, y, width, height) = rect;
    let resolve = |value: LengthPercentage, of: f64| match value {
        LengthPercentage::Px(px) => px,
        LengthPercentage::Percent(percent) => of * percent / 100.0,
    };
    // §13.1's functions as matrices in CSS pixels, y downwards. Each later
    // function is applied first: `translate(10px) rotate(45deg)` rotates and
    // then translates.
    let mut css = IDENTITY;
    for function in list {
        let own = match *function {
            Transform::Matrix(matrix) => matrix,
            Transform::Translate(tx, ty) => {
                [1.0, 0.0, 0.0, 1.0, resolve(tx, width), resolve(ty, height)]
            }
            Transform::Scale(sx, sy) => [sx, 0.0, 0.0, sy, 0.0, 0.0],
            // Clockwise on a downward axis, which is SVG's `rotate()` exactly
            // and through the same deterministic sine (ruling 4).
            Transform::Rotate(degrees) => rotation(degrees),
            Transform::Skew(ax, ay) => [1.0, tangent(ay), tangent(ax), 1.0, 0.0, 0.0],
        };
        css = concat(own, css);
    }
    let [a, b, c, d, e, f] = css;
    // The page flips the y axis and scales by the pixel: the linear part's
    // off-diagonal terms change sign and the offsets become points.
    let (pa, pb, pc, pd) = (a, -b, -c, d);
    let (ox, oy) = (
        frame.x(x + resolve(origin.x, width)),
        frame.y(y + resolve(origin.y, height)),
    );
    let matrix = [
        pa,
        pb,
        pc,
        pd,
        ox - (pa * ox + pc * oy) + e * PX_TO_PT,
        oy - (pb * ox + pd * oy) - f * PX_TO_PT,
    ];
    invert(matrix).map(|_| matrix)
}

/// `tan` of an angle in degrees, as the sine over the cosine of the one
/// deterministic implementation (ruling 4) — `skew(90deg)` is infinite, and
/// [`local_matrix`] refuses the product.
fn tangent(degrees: f64) -> f64 {
    let turn = rotation(degrees);
    turn[1] / turn[0]
}

/// A translucent colour's alpha over an element's composed opacity, in steps.
fn translucent(steps: u16, colour: Color) -> u16 {
    (f64::from(steps) * f64::from(colour.a) / f64::from(u8::MAX)).round() as u16
}

/// An alpha's resource name: `EA` and its ten-thousandths, so `EA5000` is
/// half.
fn alpha_name(steps: u16) -> Vec<u8> {
    format!("EA{steps}").into_bytes()
}

/// Where a laid-out page's coordinates land on a PDF page.
///
/// `tinker-pdf-layout` measures in CSS pixels with `y` growing **downward**
/// from the top of the content area; a PDF page is points with `y` growing
/// upward from the bottom. The flip and the scale happen here, once, which is
/// the reason this is a struct rather than four arguments passed around.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The page box, in points.
    pub page: (f64, f64),
    /// The margin around the content area, in points.
    pub margin: f64,
}

impl Frame {
    /// The content area, in CSS pixels, which is what `Options` takes.
    #[must_use]
    pub fn content_px(&self) -> (f64, f64) {
        (
            (self.page.0 - self.margin * 2.0).max(1.0) / PX_TO_PT,
            (self.page.1 - self.margin * 2.0).max(1.0) / PX_TO_PT,
        )
    }

    /// A horizontal offset in CSS pixels, as a PDF x coordinate.
    #[must_use]
    pub fn x(&self, px: f64) -> f64 {
        self.margin + px * PX_TO_PT
    }

    /// A downward offset in CSS pixels, as a PDF y coordinate.
    #[must_use]
    pub fn y(&self, px: f64) -> f64 {
        self.page.1 - self.margin - px * PX_TO_PT
    }
}

/// Draws one laid-out page.
///
/// Decorations first, in the order `tinker-pdf-layout` produced them — an
/// ancestor before its descendants, so a child's background covers its
/// parent's — and then the text, in **reading order**, which is what makes
/// `Page::text()` return the words in the order the book wrote them rather
/// than in the order a painter found convenient.
///
/// Returns how many shaped pieces the writer refused, which the caller turns
/// into [`crate::ArchiveWarning::UnwritableTextRun`] (ruling 10).
///
/// `dom` is the element tree the runs came from, when the caller has one, and
/// the page is then tagged into it the way the book path tags a page — each
/// element under its standard structure type, with what its markup states of
/// `/Lang` and Table 349's attributes — with `chapter` the base its keys and
/// reading positions are offset by. The book path itself tags through a
/// crate-internal form that also knows the chapter's links, its pictures'
/// places across pages and the document's role map; this one keeps the
/// signature this function has always had, and knows only the one page.
///
/// `effects` is what the page's elements apply to what they draw — opacity,
/// clips, transforms, background images and shadows ([`Effects::on`]).
#[allow(clippy::too_many_arguments)]
pub fn draw_page(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    laid: &LayoutPage,
    frame: &Frame,
    fonts: &Fonts<'_>,
    pictures: &[(u32, Vec<u8>)],
    dom: Option<&Dom>,
    chapter: u64,
    effects: &OnPage<'_>,
) -> usize {
    let Some(dom) = dom else {
        return draw_page_tagged(builder, page, laid, frame, fonts, pictures, None, effects);
    };
    let pages = std::slice::from_ref(laid);
    let figures = figure_orders(dom, pages);
    let cells = table_cells(dom, pages, pictures);
    let none = std::collections::BTreeSet::new();
    let tagging = Tagging {
        dom,
        chapter,
        document_language: None,
        figures: &figures,
        links: &none,
        path: "",
        cells: &cells,
        roles: &std::collections::BTreeSet::new(),
    };
    draw_page_tagged(
        builder,
        page,
        laid,
        frame,
        fonts,
        pictures,
        Some(&tagging),
        effects,
    )
}

/// [`draw_page`], for the book path: `tagging` is the structure the page is
/// tagged into, when the caller has the element tree the runs came from
/// ([`super::tagging`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_page_tagged(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    laid: &LayoutPage,
    frame: &Frame,
    fonts: &Fonts<'_>,
    pictures: &[(u32, Vec<u8>)],
    tagging: Option<&Tagging<'_>>,
    effects: &OnPage<'_>,
) -> usize {
    let mut refused = 0usize;
    // What each run is shaped against, from its neighbours on the page, and
    // how each line holding a right-to-left character reads back.
    fonts.set_contexts(&laid.runs, effects);
    fonts.set_readings(&laid.runs, frame, effects);
    for (index, fragment) in laid.boxes.iter().enumerate() {
        let opened = effects.open(page, fragment.anchor, false);
        draw_box(page, fragment, frame, effects.background(index), effects);
        Effects::close(page, opened);
    }
    // After the backgrounds and before the text, which is CSS 2.2 §E.2's
    // painting order for a replaced element's content: it goes in the same
    // layer as in-flow inline content, above its own background and below
    // nothing the flow put on top of it. A picture registered nowhere — the
    // writer refused the bytes after the box was already laid out — leaves its
    // box empty and is named by `ArchiveWarning::ImageNotDrawn`.
    //
    // Tagged as a `/Figure` where it is drawn, rather than moved among the
    // text: the content stream keeps the painting order, and the structure
    // tree carries the reading order (`tagging::figure_orders`).
    for fragment in &laid.replaced {
        let Some(anchor) = fragment.anchor else {
            continue;
        };
        let Some((_, name)) = pictures.iter().find(|(at, _)| *at == anchor) else {
            continue;
        };
        match tagging {
            Some(tagging) => draw_figure(page, tagging, fragment, frame, name, effects),
            None => {
                let opened = effects.open(page, fragment.anchor, false);
                draw_replaced(page, fragment, frame, name);
                Effects::close(page, opened);
            }
        }
    }
    // `css-text-decor-3` §4: each shadow is the run drawn again, offset and
    // in the shadow's colour, **under** the text — and an artifact (14.8.2.2),
    // since it is not the author's content a second time: extraction reads the
    // run once. Every shadow on the page is drawn before any of its text,
    // which is the order §4 gives within an element and an approximation
    // between two elements whose shadows reach each other's text.
    for run in &laid.runs {
        if !run.painted {
            continue;
        }
        for (colour, dx, dy) in effects.text_shadows(run.anchor).iter().rev() {
            // An offset past what a number holds (`1e400px` reads as
            // infinite) has no place on the page: the shadow draws nothing,
            // as a transform with no inverse does, rather than writing an
            // operand that is not a PDF number.
            if !finite(&[run.x + dx, run.y + dy]) {
                continue;
            }
            let mut shadow = run.clone();
            shadow.x += dx;
            shadow.y += dy;
            shadow.color = Color {
                a: u8::MAX,
                ..*colour
            };
            page.raw(b"/Artifact BMC");
            let opened = effects.open(page, run.anchor, true);
            if colour.a < u8::MAX {
                if !opened {
                    page.raw(b"q");
                }
                effects.shadow_alpha(page, run.anchor, *colour);
            }
            // Shaped against the run's own neighbours and drawn as the run
            // is, not as the shadow's place would have it: the same glyphs
            // again, in the same order ([`draw_run_against`]).
            refused += draw_run_against(
                builder,
                page,
                &shadow,
                None,
                frame,
                fonts,
                fonts.context_of(run),
                fonts.drawing_of(run),
            );
            if colour.a < u8::MAX && !opened {
                page.raw(b"Q");
            }
            Effects::close(page, opened);
            page.raw(b"EMC");
        }
    }
    // 14.7's structure tree, when the caller has the element tree the runs
    // came from. Every run carries the index of the element that wrote it, so
    // the tree this builds is the **document's** tree and not a description of
    // the page: the order is source order, which is what `TextRun::order`
    // already sorted these runs into.
    let Some(tagging) = tagging else {
        for (place, run) in laid.runs.iter().enumerate() {
            if !run.painted {
                continue;
            }
            refused += artifact_or_run(builder, page, run, Some(place), frame, fonts, effects);
        }
        draw_outlines(page, laid, frame, effects);
        return refused;
    };

    // Each run with its place in the page's list, which its plan is kept by.
    let (places, drawn): (Vec<usize>, Vec<&TextRun>) = laid
        .runs
        .iter()
        .enumerate()
        .filter(|(_, run)| run.painted)
        .unzip();
    let chains: Vec<Vec<usize>> = drawn
        .iter()
        .map(|run| match run.generated {
            // An artifact belongs to no element: 14.8.2.2 puts it outside the
            // structure entirely, which is `/Artifact` and not a tag.
            true => Vec::new(),
            false => ancestry(tagging.dom, run.anchor),
        })
        .collect();
    tag_runs(
        builder,
        page,
        frame,
        fonts,
        tagging,
        &drawn,
        &places,
        &chains,
        0,
        &mut refused,
        effects,
    );
    draw_outlines(page, laid, frame, effects);
    refused
}

/// Every outline on the page, after the text: CSS 2.2 Appendix E's tenth and
/// last step, so an outline is drawn over what is beside it rather than under.
fn draw_outlines(page: &mut PageBuilder, laid: &LayoutPage, frame: &Frame, effects: &OnPage<'_>) {
    for fragment in &laid.boxes {
        if fragment.outline.is_none() {
            continue;
        }
        let opened = effects.open(page, fragment.anchor, false);
        draw_outline(page, fragment, frame);
        Effects::close(page, opened);
    }
}

/// One run, marked as an artifact where it is one: `place` is where it
/// stands in the page's list of runs ([`draw_run`]).
pub(crate) fn artifact_or_run(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
    place: Option<usize>,
    frame: &Frame,
    fonts: &Fonts<'_>,
    effects: &OnPage<'_>,
) -> usize {
    // 14.8.2.2: a list marker is *"a graphics object that is not part of
    // the author's original content"*, which is what 14.8.2 calls an
    // artifact and what `TextRun::generated` already says one crate down.
    // Marking it is what lets a bullet be **drawn and not extracted**, and
    // it is the only reason text conservation can stay an equality: a
    // marker on the page and not in the spine would be one extra character
    // per list item, on every book with a list in it.
    if run.generated {
        page.raw(b"/Artifact BMC");
    }
    // Inside the marked-content sequence, so a `q`/`Q` pair never straddles
    // a `BDC`/`EMC` one: the two nest.
    // The run's characters are its element's content, so its own clip cuts
    // them — a marker hung outside an `overflow: hidden` list item included.
    let opened = effects.open(page, run.anchor, true);
    let refused = draw_run(builder, page, run, place, frame, fonts);
    Effects::close(page, opened);
    if run.generated {
        page.raw(b"EMC");
    }
    refused
}

fn set_fill(page: &mut PageBuilder, colour: Color) {
    page.set_fill_rgb(
        f64::from(colour.r) / 255.0,
        f64::from(colour.g) / 255.0,
        f64::from(colour.b) / 255.0,
    );
}

/// Fills a rectangle in the current colour.
///
/// `PageBuilder::fill_rect` takes a grey and would overwrite the colour set
/// above it, so the operators are written out — which is what
/// `PageBuilder::raw` is for and what its documentation says it is for.
fn fill(page: &mut PageBuilder, x: f64, y: f64, width: f64, height: f64) {
    if width <= 0.0 || height <= 0.0 || !finite(&[x, y, width, height]) {
        return;
    }
    page.raw(format!("{x} {y} {width} {height} re f").as_bytes());
}

/// Whether every operand is a number a content stream can hold.
///
/// A CSS number token past `f64`'s range reads as infinite (`1e400px`), and
/// arithmetic on one makes `NaN`; `inf` and `NaN` are not PDF numbers (7.3.3),
/// so geometry built from them is not written. What would have drawn it
/// draws nothing, as [`local_matrix`]'s singular product does.
fn finite(operands: &[f64]) -> bool {
    operands.iter().all(|operand| operand.is_finite())
}

fn draw_box(
    page: &mut PageBuilder,
    fragment: &BoxFragment,
    frame: &Frame,
    image: Option<&Plan>,
    effects: &OnPage<'_>,
) {
    let x = frame.x(fragment.x);
    let top = frame.y(fragment.y);
    let width = fragment.width * PX_TO_PT;
    let height = fragment.height * PX_TO_PT;
    if fragment
        .radius
        .iter()
        .any(|(horizontal, vertical)| *horizontal > 0.0 && *vertical > 0.0)
    {
        draw_rounded_box(
            page,
            fragment,
            (x, top - height, width, height),
            image,
            effects,
        );
        return;
    }
    let rect = (x, top - height, width, height);
    draw_shadows(page, fragment, rect, [(0.0, 0.0); 4], false, effects);
    if fragment.background.a != 0 {
        set_fill(page, fragment.background);
        fill(page, x, top - height, width, height);
    }
    if let Some(plan) = image {
        let area = format!("{x} {} {width} {height} re", top - height);
        draw_background_image(page, plan, &area);
    }
    draw_shadows(page, fragment, rect, [(0.0, 0.0); 4], true, effects);
    // A border is drawn as four filled rectangles rather than as a stroked
    // path, because CSS's border box is defined by its **edges** and a stroke
    // is centred on a path: a one-pixel stroke round the border box would put
    // half a pixel outside it on all four sides.
    let widths = &fragment.border_width;
    let styles = &fragment.border_style;
    let colours = &fragment.border_color;
    for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
        let thickness = widths.get(side) * PX_TO_PT;
        if thickness <= 0.0 || !drawable(styles.get(side)) {
            continue;
        }
        set_fill(page, colours.get(side));
        let (bx, by, bw, bh) = match side {
            Side::Top => (x, top - thickness, width, thickness),
            Side::Bottom => (x, top - height, width, thickness),
            Side::Left => (x, top - height, thickness, height),
            Side::Right => (x + width - thickness, top - height, thickness, height),
        };
        fill(page, bx, by, bw, bh);
    }
}

/// `css-backgrounds-3` §7.1.1's spread applied to one corner semi-axis: grown
/// by the spread, except that a radius smaller than a positive spread grows
/// by less — `r + s(1 + (r/s − 1)³)` — so a nearly square corner does not
/// suddenly round; a square one stays square, and a negative spread shrinks
/// it to no less than zero.
fn spread_radius(radius: f64, spread: f64) -> f64 {
    if radius <= 0.0 {
        return 0.0;
    }
    if spread < 0.0 {
        return (radius + spread).max(0.0);
    }
    if radius >= spread {
        return radius + spread;
    }
    let ratio = radius / spread;
    radius + spread * (1.0 + (ratio - 1.0).powi(3))
}

/// `box-shadow`, `css-backgrounds-3` §7.1, without blur: the outer shadows
/// (`inset` false) under the background and **only outside the border box**
/// — §7.1.1 clips them there, so a box with no background does not show its
/// own shadow through itself — and the `inset` ones over the background and
/// image and inside the padding box. The first in the list is on top, so the
/// list is drawn backwards.
///
/// An outer shadow's shape is the border box offset and grown by the spread,
/// its corners by [`spread_radius`]; an inset one fills the padding box less
/// that box offset and shrunk by the spread.
fn draw_shadows(
    page: &mut PageBuilder,
    fragment: &BoxFragment,
    rect: (f64, f64, f64, f64),
    outer: [(f64, f64); 4],
    inset: bool,
    effects: &OnPage<'_>,
) {
    let (left, bottom, width, height) = rect;
    let widths = &fragment.border_width;
    let (bt, br, bb, bl) = (
        widths.top * PX_TO_PT,
        widths.right * PX_TO_PT,
        widths.bottom * PX_TO_PT,
        widths.left * PX_TO_PT,
    );
    let padding = (
        left + bl,
        bottom + bb,
        (width - bl - br).max(0.0),
        (height - bt - bb).max(0.0),
    );
    let inner = [
        ((outer[0].0 - bl).max(0.0), (outer[0].1 - bt).max(0.0)),
        ((outer[1].0 - br).max(0.0), (outer[1].1 - bt).max(0.0)),
        ((outer[2].0 - br).max(0.0), (outer[2].1 - bb).max(0.0)),
        ((outer[3].0 - bl).max(0.0), (outer[3].1 - bb).max(0.0)),
    ];
    for shadow in fragment
        .shadows
        .iter()
        .rev()
        .filter(|shadow| shadow.inset == inset)
    {
        let Some(colour) = shadow.color else {
            continue;
        };
        let (dx, dy, spread) = (
            shadow.x * PX_TO_PT,
            -shadow.y * PX_TO_PT,
            shadow.spread * PX_TO_PT,
        );
        if inset {
            let hole = (
                padding.0 + dx + spread,
                padding.1 + dy + spread,
                padding.2 - 2.0 * spread,
                padding.3 - 2.0 * spread,
            );
            let hole_radii =
                inner.map(|(h, v)| (spread_radius(h, -spread), spread_radius(v, -spread)));
            if !finite(&[hole.0, hole.1, hole.2, hole.3]) || !finite(&corners(hole_radii)) {
                continue;
            }
            let area = rounded_path(padding, inner);
            page.raw(format!("q {area} W n").as_bytes());
            effects.shadow_alpha(page, fragment.anchor, colour);
            set_fill(page, colour);
            if hole.2 > 0.0 && hole.3 > 0.0 {
                page.raw(format!("{area} {} f*", rounded_path(hole, hole_radii)).as_bytes());
            } else {
                page.raw(format!("{area} f").as_bytes());
            }
            page.raw(b"Q");
        } else {
            let shape = (
                left + dx - spread,
                bottom + dy - spread,
                width + 2.0 * spread,
                height + 2.0 * spread,
            );
            if shape.2 <= 0.0 || shape.3 <= 0.0 {
                continue;
            }
            let radii = outer.map(|(h, v)| (spread_radius(h, spread), spread_radius(v, spread)));
            if !finite(&[shape.0, shape.1, shape.2, shape.3]) || !finite(&corners(radii)) {
                continue;
            }
            let (page_width, page_height) = effects.frame.page;
            page.raw(
                format!(
                    "q 0 0 {page_width} {page_height} re {} W* n",
                    rounded_path(rect, outer)
                )
                .as_bytes(),
            );
            effects.shadow_alpha(page, fragment.anchor, colour);
            set_fill(page, colour);
            page.raw(format!("{} f", rounded_path(shape, radii)).as_bytes());
            page.raw(b"Q");
        }
    }
}

/// A box's four corner radii as eight operands, for [`finite`].
fn corners(radii: [(f64, f64); 4]) -> [f64; 8] {
    let [a, b, c, d] = radii;
    [a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1]
}

/// One planned background image, clipped to the painting area `area` — a
/// path, the border box's rectangle or its rounded shape.
fn draw_background_image(page: &mut PageBuilder, plan: &Plan, area: &str) {
    page.raw(format!("q {area} W n").as_bytes());
    match plan {
        Plan::Once { image, rect } => page.image(image, rect.0, rect.1, rect.2, rect.3),
        Plan::Tiled { pattern, fill } => {
            if page.set_fill_pattern(pattern) {
                page.raw(format!("{} {} {} {} re f", fill.0, fill.1, fill.2, fill.3).as_bytes());
            }
        }
    }
    page.raw(b"Q");
}

/// `4(√2 − 1) / 3`: the distance along each tangent, as a fraction of the
/// radius, at which a cubic Bézier's control points make the closest quarter
/// circle — and, scaled per axis, the closest quarter ellipse.
pub const QUARTER_ARC: f64 = 0.552_284_749_830_793_4;

/// A rectangle with elliptical corners as a closed path, in page points.
///
/// `rect` is `(left, bottom, width, height)` and `radii` the four corners'
/// `(horizontal, vertical)` semi-axes in [`tinker_pdf_css::property::Corner::ALL`]'s
/// order. Clockwise from the top edge; a corner with either semi-axis zero is
/// square and its curve is not written. Each corner is one cubic whose control
/// points sit [`QUARTER_ARC`] of the way along the two tangents, which is the
/// closed form `epub_paint.rs` checks the operands against.
#[must_use]
pub fn rounded_path(rect: (f64, f64, f64, f64), radii: [(f64, f64); 4]) -> String {
    let (left, bottom, width, height) = rect;
    let (right, top) = (left + width, bottom + height);
    let k = 1.0 - QUARTER_ARC;
    let round = |(rx, ry): (f64, f64)| rx > 0.0 && ry > 0.0;
    let [tl, tr, br, bl] = radii.map(|r| if round(r) { r } else { (0.0, 0.0) });
    let mut path = String::new();
    path.push_str(&format!("{} {} m ", left + tl.0, top));
    path.push_str(&format!("{} {} l ", right - tr.0, top));
    if round(tr) {
        path.push_str(&format!(
            "{} {} {} {} {} {} c ",
            right - tr.0 * k,
            top,
            right,
            top - tr.1 * k,
            right,
            top - tr.1
        ));
    }
    path.push_str(&format!("{} {} l ", right, bottom + br.1));
    if round(br) {
        path.push_str(&format!(
            "{} {} {} {} {} {} c ",
            right,
            bottom + br.1 * k,
            right - br.0 * k,
            bottom,
            right - br.0,
            bottom
        ));
    }
    path.push_str(&format!("{} {} l ", left + bl.0, bottom));
    if round(bl) {
        path.push_str(&format!(
            "{} {} {} {} {} {} c ",
            left + bl.0 * k,
            bottom,
            left,
            bottom + bl.1 * k,
            left,
            bottom + bl.1
        ));
    }
    path.push_str(&format!("{} {} l ", left, top - tl.1));
    if round(tl) {
        path.push_str(&format!(
            "{} {} {} {} {} {} c ",
            left,
            top - tl.1 * k,
            left + tl.0 * k,
            top,
            left + tl.0,
            top
        ));
    }
    path.push('h');
    path
}

/// A box with rounded corners, `css-backgrounds-3` §5.
///
/// The background fills the border box's rounded shape — `background-clip`'s
/// initial `border-box`. The border is the region between that shape and the
/// padding box's, whose radii are §5.3's: each outer radius less the border
/// width on its own axis, never below zero. Each side's colour is filled inside
/// a clip that runs from the outer corner to the inner one, which is where §5.4
/// puts the transition between two sides' colours — so four sides of one
/// colour draw one ring, and four of four colours meet on the diagonals.
fn draw_rounded_box(
    page: &mut PageBuilder,
    fragment: &BoxFragment,
    rect: (f64, f64, f64, f64),
    image: Option<&Plan>,
    effects: &OnPage<'_>,
) {
    let (left, bottom, width, height) = rect;
    let (right, top) = (left + width, bottom + height);
    let outer: [(f64, f64); 4] = fragment
        .radius
        .map(|(horizontal, vertical)| (horizontal * PX_TO_PT, vertical * PX_TO_PT));
    draw_shadows(page, fragment, rect, outer, false, effects);
    if fragment.background.a != 0 {
        set_fill(page, fragment.background);
        page.raw(format!("{} f", rounded_path(rect, outer)).as_bytes());
    }
    // §5.3: the background is clipped to the curve, the image with it.
    if let Some(plan) = image {
        draw_background_image(page, plan, &rounded_path(rect, outer));
    }
    draw_shadows(page, fragment, rect, outer, true, effects);
    let widths = &fragment.border_width;
    let (bt, br, bb, bl) = (
        widths.top * PX_TO_PT,
        widths.right * PX_TO_PT,
        widths.bottom * PX_TO_PT,
        widths.left * PX_TO_PT,
    );
    let inner_rect = (
        left + bl,
        bottom + bb,
        (width - bl - br).max(0.0),
        (height - bt - bb).max(0.0),
    );
    // §5.3: the padding edge's radius is the border edge's less the border's
    // width, per axis.
    let inner = [
        ((outer[0].0 - bl).max(0.0), (outer[0].1 - bt).max(0.0)),
        ((outer[1].0 - br).max(0.0), (outer[1].1 - bt).max(0.0)),
        ((outer[2].0 - br).max(0.0), (outer[2].1 - bb).max(0.0)),
        ((outer[3].0 - bl).max(0.0), (outer[3].1 - bb).max(0.0)),
    ];
    let ring = format!(
        "{} {} f*",
        rounded_path(rect, outer),
        rounded_path(inner_rect, inner)
    );
    let (il, ib, iw, ih) = inner_rect;
    let (ir, it) = (il + iw, ib + ih);
    for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
        if widths.get(side) <= 0.0 || !drawable(fragment.border_style.get(side)) {
            continue;
        }
        // The side's own region: its outer edge, and the two diagonals from
        // the box's corners to the padding box's.
        let polygon = match side {
            Side::Top => [(left, top), (right, top), (ir, it), (il, it)],
            Side::Right => [(right, top), (right, bottom), (ir, ib), (ir, it)],
            Side::Bottom => [(right, bottom), (left, bottom), (il, ib), (ir, ib)],
            Side::Left => [(left, bottom), (left, top), (il, it), (il, ib)],
        };
        let [a, b, c, d] = polygon;
        set_fill(page, fragment.border_color.get(side));
        page.raw(
            format!(
                "q {} {} m {} {} l {} {} l {} {} l h W n {ring} Q",
                a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1
            )
            .as_bytes(),
        );
    }
}

/// A box's outline, `css-ui-4` §5: four filled bands outside the border edge,
/// `outline-offset` out from it and `outline-width` wide.
///
/// **Rectangular around a rounded box.** §5 leaves it to the user agent
/// whether an outline follows `border-radius`, and this one draws the
/// rectangle, which is what CSS 2.1's outline was.
fn draw_outline(page: &mut PageBuilder, fragment: &BoxFragment, frame: &Frame) {
    let Some(outline) = fragment.outline else {
        return;
    };
    if !drawable(outline.style) || outline.width <= 0.0 {
        return;
    }
    let spread = outline.offset * PX_TO_PT;
    let thickness = outline.width * PX_TO_PT;
    let left = frame.x(fragment.x) - spread - thickness;
    let top = frame.y(fragment.y) + spread + thickness;
    let width = fragment.width * PX_TO_PT + 2.0 * (spread + thickness);
    let height = fragment.height * PX_TO_PT + 2.0 * (spread + thickness);
    if !finite(&[left, top, width, height, thickness]) {
        return;
    }
    if width <= 2.0 * thickness || height <= 2.0 * thickness {
        // An offset negative enough to turn the outline inside out draws the
        // whole rectangle.
        set_fill(page, outline.color);
        fill(page, left, top - height, width.max(0.0), height.max(0.0));
        return;
    }
    set_fill(page, outline.color);
    fill(page, left, top - thickness, width, thickness);
    fill(page, left, top - height, width, thickness);
    fill(page, left, top - height, thickness, height);
    fill(
        page,
        left + width - thickness,
        top - height,
        thickness,
        height,
    );
}

/// Draws one replaced element's picture into its content box.
///
/// # What it does not do
///
/// **It does not scale to fit and it does not letterbox.** A raster's intrinsic
/// aspect ratio is already in the box `tinker-pdf-layout` gave it — CSS 2.2
/// §10.3.2 and §10.6.2 put it there — so the picture fills the content box
/// exactly, and where an author stated a `width` and a `height` that disagree
/// with the picture's proportions, it is stretched. That is what CSS says
/// happens: `object-fit` is the property that would say otherwise and it is not
/// implemented here. The SVG path's `preserveAspectRatio` is a different
/// question about a different element and `epub::svg::place_image` answers it
/// there.
///
/// **It draws untagged.** 14.8.4.4 would put a picture in a `/Figure` with an
/// `/Alt`, and the structure this file builds is built out of *text runs* —
/// every element of it is opened around a run's ancestry. See the refusal table
/// in `docs/features/epub.md`.
pub(crate) fn draw_replaced(
    page: &mut PageBuilder,
    fragment: &ReplacedFragment,
    frame: &Frame,
    name: &[u8],
) {
    let width = fragment.width * PX_TO_PT;
    let height = fragment.height * PX_TO_PT;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    // 8.9.5.2 puts an image in the unit square with its first row at the top,
    // and `PageBuilder::image` writes the `cm` that maps the square onto a
    // rectangle given by its **bottom** left corner — which is `frame.y` of the
    // content box's bottom edge, the one place the downward `y` of a flow and
    // the upward `y` of a page have to meet.
    page.image(
        name,
        frame.x(fragment.x),
        frame.y(fragment.y + fragment.height),
        width,
        height,
    );
}

/// Whether a border style puts ink on the page at all.
///
/// `dashed`, `dotted` and `double` are drawn **solid**, and that is a
/// partiality worth naming rather than hiding: the border is in the right
/// place at the right width in the right colour and the pattern is wrong.
/// `tinker-pdf-layout` already warns for the properties it does not honour;
/// this one is honoured approximately, which is a third thing, and milestone
/// 13's `As built` is where it is recorded.
fn drawable(style: BorderStyle) -> bool {
    !matches!(style, BorderStyle::None | BorderStyle::Hidden)
}

/// One stretch of a run sharing a font resource, one string of codes from one
/// position: a text object of its own, or a piece of one that holds a mark
/// ([`flush`]). Two plans of the same segments are written byte for byte alike
/// ([`Fonts::plan_run`]).
#[derive(Clone, Debug, PartialEq)]
struct Segment {
    resource: Vec<u8>,
    composite: bool,
    /// Whether it is the overflow font's ([`Face::overflow_resource`]),
    /// whose code 32 is a character of the book's and not a word space, so
    /// that it is written at no word spacing ([`flush`]).
    unspaced: bool,
    codes: Vec<u8>,
    glyphs: Vec<(u16, char)>,
    characters: String,
    /// Each character's advance as layout measured it, which
    /// [`Fonts::read_back`] draws it at.
    advances: Vec<f64>,
    x: f64,
}

/// What [`Fonts::plan_run`] plans for a run: the text objects
/// [`plan_coded`] writes for the whole of it, each face segment's in the
/// order drawn, and where the pen ends — `None` for a run with a character
/// in a face of the book's own, which [`draw_shaped`] draws.
type RunPlan = Option<(Vec<Vec<Segment>>, f64)>;

impl Segment {
    /// The character a reader of the page reads for `ch`, drawn at `code` in
    /// this segment, as its font gives it back: a composite font by the
    /// `/ToUnicode` written from the characters drawn, so `ch`; the overflow
    /// font by its `/Differences` name ([`overflow_reads_as`]); and a
    /// standard face by `WinAnsiEncoding`, which 9.6.5 reads by glyph name,
    /// so that its NO-BREAK SPACE is a space and its SOFT HYPHEN a hyphen
    /// (`base_char`). Read back as typed, a NO-BREAK SPACE between two digits,
    /// `CS` and so part of one number to UAX #9, was read where the page has
    /// a word space, and a drawing that read right in the model read the
    /// number's halves the other way round on the page (round 6, lane F).
    /// The overflow font's names are looked up once a book, through `fonts`
    /// ([`Fonts::overflow_read`]).
    fn read_as(&self, ch: char, code: u32, fonts: &Fonts<'_>) -> char {
        if self.composite {
            ch
        } else if self.unspaced {
            fonts.overflow_read(ch)
        } else {
            u8::try_from(code)
                .ok()
                .and_then(|code| base_char(BaseEncoding::WinAnsi, code))
                .unwrap_or(ch)
        }
    }
}

/// The character a reader reads for `ch` drawn in the overflow font: the one
/// its glyph name in the font's `/Differences` names, as
/// [`Fonts::register`] writes it — `space` for a character with no name.
fn overflow_reads_as(ch: char) -> char {
    glyph_name_to_char(&glyph_name_for_char(ch).unwrap_or_else(|| "space".to_owned())).unwrap_or(ch)
}

/// How [`plan_coded`] draws a run whose line, as the rules draw it, a reader
/// of the page reads back otherwise ([`Fonts::set_readings`]); the default
/// is the rules' drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Drawing {
    /// Whether a slice of neutrals at a right-to-left level is drawn in its
    /// level's order ([`coded_order`]) — `None`, as the line decided
    /// ([`TextRun::bidi_line_rtl`]).
    level_order: Option<bool>,
    /// How a spaced slice is cut into text objects — `None`, by
    /// [`draw_coded`]'s rules.
    cut: Option<Cut>,
}

/// A cut of a spaced slice into text objects other than the rules'
/// ([`Drawing::cut`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cut {
    /// The whole slice one text object, the white space at its ends too.
    Whole,
    /// A text object at every change of resource, as every slice was cut
    /// before the inside of a spaced slice was one object and before the
    /// neutrals a reader undoes were.
    Pieces,
    /// [`Cut::Whole`], and the white space at the slice's right end drawn at
    /// the end of its stretch rather than its start: a word space's `Tw` is
    /// part of its advance (9.3.3), and drawn where that advance ends the
    /// space stands against what is drawn next, so a reader's pen stands
    /// where the next run starts — no further than the slice's
    /// `letter-spacing` — and does not cut the line there.
    Carried,
    /// A slice of white space alone — a word space styled apart from the
    /// words either side of it, a run of its own — drawn at the middle of
    /// its stretch: a space draws nothing, and drawn there the gaps either
    /// side of it are half the stretch each, which a reader resumes a line
    /// across where the whole of it, on one side, is a cut.
    Centered,
    /// [`Cut::Whole`], and the white space at the slice's left end drawn at
    /// the end of its stretch, against the glyph after it, rather than at
    /// its start: a space draws nothing, and drawn there its box does not
    /// reach towards what is drawn left of the slice, so a reader does not
    /// join the two where the stretch between them is wider than half an
    /// em. The word space a right-to-left run ends with, beside the atomic
    /// inline that follows it on the line, is drawn so where the inline's
    /// text reads alone, as its own paragraph does ([`visual_lines`]).
    Hugged,
    /// A text object for each word and each stretch of white space, written
    /// in reading order — the right-to-left slice's rightmost first — each
    /// glyph where the rules draw it: a reader that parts the slice at its
    /// word spaces into lines of their own keeps them in the order written.
    Reading,
}

/// One embedded face's shaped slice: where every glyph goes, and how wide the
/// whole of it is.
struct Shaped {
    /// The glyphs **in the order they are drawn**, positioned in text space
    /// from the run's own origin — which is what
    /// [`tinker_pdf_cos::build::DocumentBuilder::glyph_run`] takes.
    glyphs: Vec<Placed>,
    /// The slice's whole advance, in **points**, `letter-spacing` included.
    ///
    /// Not `glyphs.last()`'s position: the last glyph may be a mark with no
    /// advance sitting behind its base, and the pen is not where the ink
    /// stopped.
    advance: f64,
}

/// Whether the last character of one side and the first of the other resolve
/// to the **same embedded face**: the condition for either to be the other's
/// shaping context, since a glyph index means nothing in another face and the
/// standard 14 are not shaped. One function, so the painter ([`Fonts`]) and
/// the measurement ([`BookMetrics`]) cannot disagree about it.
fn one_embedded_face(
    faces: &FaceSet,
    (last, left): (Option<char>, &FontRequest<'_>),
    (first, right): (Option<char>, &FontRequest<'_>),
) -> bool {
    let (Some(last), Some(first)) = (last, first) else {
        return false;
    };
    match (
        choose(faces, left, Some(last)),
        choose(faces, right, Some(first)),
    ) {
        (Chosen::Embedded(x), Chosen::Embedded(y)) => x == y,
        _ => false,
    }
}

/// The runs of `painted`, a page's, on a line that holds the text of more
/// than one paragraph ([`holds_two_paragraphs`]), a line as [`page_lines`]
/// finds it.
fn sharing_a_line(
    painted: &[&TextRun],
    along: &dyn Fn(&TextRun) -> f64,
) -> std::collections::BTreeSet<RunKey> {
    page_lines(painted, along)
        .into_iter()
        .filter(|line| holds_two_paragraphs(line.iter().copied()))
        .flatten()
        .map(run_key)
        .collect()
}

/// The lines of `painted`, a page's runs, as a reader of the page finds them:
/// the runs on one baseline, within half the larger font size, each starting
/// no more than half an em past where the ones left of it end — as
/// `TextDevice` asks of a glyph that continues a line after a text object's
/// end, so that two table cells half a point apart are one line, as they are
/// to it — each line left to right. Found by sorting, so that a page of many
/// runs costs its sort.
fn page_lines<'r>(
    painted: &[&'r TextRun],
    along: &dyn Fn(&TextRun) -> f64,
) -> Vec<Vec<&'r TextRun>> {
    let mut order: Vec<&TextRun> = painted.to_vec();
    order.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    // Rows of one baseline, then each row by `x`. A run is on the row of the
    // run above it within half an em, as `TextDevice` asks each glyph of the
    // one before it: under a `line-height` of half an em or less every line
    // of a paragraph is within that of the next, and a reader runs them all
    // together, so they are one row.
    let mut rows: Vec<Vec<&TextRun>> = Vec::new();
    for run in order {
        match rows.last_mut() {
            Some(row)
                if row.first().is_some_and(|first| {
                    (first.y - run.y).abs() <= first.font_size.max(run.font_size) * 0.5
                }) =>
            {
                row.push(run);
            }
            _ => rows.push(vec![run]),
        }
    }
    let mut lines: Vec<Vec<&TextRun>> = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut line: Vec<&TextRun> = Vec::new();
        let mut end = f64::NEG_INFINITY;
        let mut size = 0.0f64;
        for run in row {
            // The gap in a reader's ems on the page: the wider of the two
            // runs' measures of it, so that no line a reader joins is parted.
            let gap = (run.x - end) * along(run).max(line.last().map_or(1.0, |last| along(last)));
            if !line.is_empty() && gap > size.max(run.font_size) * RESUMED_WITHIN {
                lines.push(std::mem::take(&mut line));
                end = f64::NEG_INFINITY;
                size = 0.0;
            }
            end = end.max(run.x + run.width);
            size = size.max(run.font_size);
            line.push(run);
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// A line's text as [`Fonts::set_readings`] holds a reading to it: its
/// letters, white space aside, and its words, each run of white space one
/// space and none at either end.
struct Words {
    letters: String,
    words: String,
}

/// The glyphs a reading of a line draws ([`Fonts::read_back`]) that draw
/// something but a nonspacing mark, which rides on its letter: each one's
/// baseline, to a tenth of a point, and its box, in the order drawn — taken
/// before the reading is put in logical order, and counted
/// ([`Overprint::count`]) only where the count decides whether a drawing is
/// taken ([`Fonts::search`]).
struct Overprint(Vec<(i64, f64, f64, f64)>);

impl Overprint {
    /// The glyphs of `page`, a reading in the order drawn.
    fn of(page: &tinker_pdf_content::TextPage) -> Overprint {
        Overprint(
            page.blocks
                .iter()
                .flat_map(|block| &block.lines)
                .flat_map(|line| &line.chars)
                .filter(|c| {
                    !c.text.trim().is_empty()
                        && c.text
                            .chars()
                            .next()
                            .is_none_or(|first| bidi_class(first) != BidiClass::NSM)
                })
                .map(|c| {
                    let (x0, _, x1, _) = c.quad.bounds();
                    ((c.origin.1 * 10.0).round() as i64, x0, x1, c.size)
                })
                .collect(),
        )
    }

    /// How many of the glyphs stand over the one before them on their
    /// baseline, as a reader of the page sees them: by their boxes, rows of
    /// one baseline, a glyph over another where it starts more than a
    /// fiftieth of its size inside the one left of it. The box is the
    /// glyph's quad's bounds, so under a
    /// skew a slanted glyph's box reaches over its neighbour's where nothing
    /// is drawn over anything (V7 of round 6's review): every drawing tried
    /// is counted so, the rules' among them.
    fn count(self) -> usize {
        let Overprint(mut boxes) = self;
        boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        boxes
            .windows(2)
            .filter(|pair| pair[0].0 == pair[1].0 && pair[1].1 < pair[0].2 - 0.02 * pair[1].3)
            .count()
    }
}

/// What [`Fonts::search`] found for a line.
enum Search {
    /// The rules' drawing reads the line right, or the line is not read back.
    Rules,
    /// The rules' drawing reads the line's letters in order and not its
    /// words ([`Read::Letters`]).
    Letters,
    /// Draw the line so.
    Found(BTreeMap<RunKey, Drawing>),
    /// Nothing tried reads the line right.
    Nothing,
}

/// How a reading of a line compares with its text ([`Words::read_as`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    /// Its words, each where it was written, and so its letters.
    Words,
    /// Its letters in the order written and not its words: a word cut where
    /// the reader parts the spacing, or two run together.
    Letters,
    /// Anything else.
    Otherwise,
}

/// How [`Fonts::read_back`] gives each glyph of a reading back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Given {
    /// As the page's font gives it back ([`Segment::read_as`]), as a reader
    /// of the page reads it.
    ByTheFont,
    /// As typed, as 05cea0b read every line back: where nothing tried reads
    /// a line right as the page's fonts give it back
    /// ([`Fonts::set_readings`]).
    AsTyped,
}

impl Words {
    fn of(text: &str) -> Words {
        Words {
            letters: text.chars().filter(|c| !c.is_whitespace()).collect(),
            words: text.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    /// How many of the line's pairs of letters, each letter and the one
    /// written after it, `read` reads in that order, each pair counted as
    /// often as the text has it: how near a reading that is not the text's
    /// comes to it, in time linear in the two, near enough.
    fn pairs_in(&self, read: &str) -> usize {
        let mut wanted: BTreeMap<(char, char), usize> = BTreeMap::new();
        let mut before: Option<char> = None;
        for ch in self.letters.chars() {
            if let Some(first) = before {
                *wanted.entry((first, ch)).or_default() += 1;
            }
            before = Some(ch);
        }
        let mut found = 0;
        let mut before: Option<char> = None;
        for ch in read.chars().filter(|c| !c.is_whitespace()) {
            if let Some(left) = before.and_then(|first| wanted.get_mut(&(first, ch))) {
                if *left > 0 {
                    *left -= 1;
                    found += 1;
                }
            }
            before = Some(ch);
        }
        found
    }

    /// How `read`, a reading of the line (its lines joined by a line feed),
    /// compares with the line's text.
    fn read_as(&self, read: &str) -> Read {
        if !read
            .chars()
            .filter(|c| !c.is_whitespace())
            .eq(self.letters.chars())
        {
            return Read::Otherwise;
        }
        if read.split_whitespace().eq(self.words.split_whitespace()) {
            return Read::Words;
        }
        Read::Letters
    }
}

/// How many drawings [`Fonts::set_readings`] reads one line back in before
/// it keeps the rules' drawing: the rules' own, each run drawn otherwise in
/// turn, then two runs at a time. A bound on the work per line, which
/// reading back is linear in.
const READINGS_PER_LINE: usize = 128;

/// The drawings [`Fonts::set_readings`] tries `run` in other than the rules':
/// its neutrals in the other order where it is at a right-to-left level, and
/// where it is spaced so that a reader may part its text objects
/// ([`PARTED_FROM`]) its slice cut each way [`Cut`] names — centred
/// ([`Cut::Centered`]) only where it is white space alone — by itself and
/// with its neutrals turned; then, where its slice opens with a word space,
/// that space drawn against the glyph after it ([`Cut::Hugged`]), and where
/// it is right to left and of more than one word, its words written in
/// reading order ([`Cut::Reading`]).
fn drawings_of(run: &TextRun, along: f64) -> Vec<(RunKey, Drawing)> {
    let key = run_key(run);
    let odd = run.bidi_level.is_some_and(|level| level % 2 == 1);
    let turns = odd && run.text.chars().any(|c| !opens_right_to_left(c));
    let spaced = parted(run, along);
    let mut out = Vec::new();
    let level_orders: &[Option<bool>] = if turns {
        &[None, Some(!run.bidi_line_rtl)]
    } else {
        &[None]
    };
    // A run of white space alone is centred in its stretch too.
    let alone = !run.text.is_empty() && run.text.chars().all(char::is_whitespace);
    for &level_order in level_orders {
        let cuts: &[Option<Cut>] = match (spaced, alone) {
            (true, true) => &[
                None,
                Some(Cut::Whole),
                Some(Cut::Pieces),
                Some(Cut::Carried),
                Some(Cut::Centered),
            ],
            (true, false) => &[
                None,
                Some(Cut::Whole),
                Some(Cut::Pieces),
                Some(Cut::Carried),
            ],
            (false, _) => &[None],
        };
        for &cut in cuts {
            if level_order.is_some() || cut.is_some() {
                out.push((key, Drawing { level_order, cut }));
            }
        }
    }
    // A spaced run whose slice opens, drawn, with a word space: the space
    // drawn against the glyph after it ([`Cut::Hugged`]).
    let opens_with_space = if reads_right_to_left(run) {
        run.text.ends_with(' ')
    } else {
        run.text.starts_with(' ')
    };
    if spaced && !alone && opens_with_space {
        out.push((
            key,
            Drawing {
                level_order: None,
                cut: Some(Cut::Hugged),
            },
        ));
    }
    // A spaced right-to-left run of more than one word: its words written in
    // reading order ([`Cut::Reading`]).
    let words = run.text.split_whitespace().nth(1).is_some();
    if spaced && odd && words {
        out.push((
            key,
            Drawing {
                level_order: None,
                cut: Some(Cut::Reading),
            },
        ));
    }
    out
}

/// Which run a context belongs to: its document order and where it is drawn,
/// which no two runs on a page share.
type RunKey = (usize, u64, u64);

fn run_key(run: &TextRun) -> RunKey {
    (run.order, run.x.to_bits(), run.y.to_bits())
}

/// Where `run`, a reference into `runs`, stands in it: the place its plans
/// are kept by ([`Fonts::plans`]). Not [`run_key`], which two runs of one
/// page may share, a run of format characters alone set where the next one
/// starts: each run is written as it was planned, its own characters. The
/// page draws each run by the same place ([`draw_run`]), wherever the run is
/// held by then; a copy drawn somewhere else, a text shadow, has no place and
/// is planned anew.
fn place_in(runs: &[TextRun], run: &TextRun) -> Option<usize> {
    let size = std::mem::size_of::<TextRun>().max(1);
    let offset = (std::ptr::from_ref(run) as usize).checked_sub(runs.as_ptr() as usize)?;
    let at = offset / size;
    runs.get(at)
        .is_some_and(|held| std::ptr::eq(held, run))
        .then_some(at)
}

/// Whether two runs ask for one font for every character they draw: the
/// family list, weight and style [`choose`] reads, which is all that
/// [`Fonts::read_as`] reads of a run.
fn same_font(a: &TextRun, b: &TextRun) -> bool {
    a.families == b.families && a.weight == b.weight && a.style == b.style
}

/// How many characters of a neighbour a run is shaped against.
///
/// Enough for every context the default features look at — joining reaches
/// past transparent marks to the nearest letter, and a pair or a mark looks
/// one glyph away — and small enough that a page of short spans is not
/// shaped twice over.
const CONTEXT_CHARS: usize = 8;

// Layout hands a measured slice at most `CONTEXT_BYTES` of a neighbour, and
// the painter shapes a drawn run against `CONTEXT_CHARS` of its neighbour
// run: the two agree only while the first holds the second whatever the
// script, a character being at most four bytes.
const _: () = assert!(CONTEXT_CHARS * 4 <= CONTEXT_BYTES);

/// The last `n` characters of `text`, read from its end.
///
/// **From the end, and not by counting first.** A neighbour layout hands a
/// measured slice is at most [`CONTEXT_BYTES`], so either would do there;
/// the painter's neighbour is a whole run ([`layout_context`], [`Fonts::set_contexts`]),
/// which a paragraph at a tiny `font-size` makes a whole line, and reading
/// it from the end costs what is taken (review of lane 8C).
fn tail(text: &str, n: usize) -> String {
    let start = match n.checked_sub(1) {
        None => text.len(),
        Some(last) => text.char_indices().rev().nth(last).map_or(0, |(at, _)| at),
    };
    text.get(start..).unwrap_or_default().to_owned()
}

/// The first `n` characters of `text`.
fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// One embedded face's glyphs for a slice, **in the order they are drawn**.
///
/// Milestone 6 of `docs/design/shaping.md`. Four things happen here that the
/// per-character path this replaced could not do:
///
/// - `GSUB` runs, so a joining script's letters take their initial, medial and
///   final forms instead of the isolated glyph a `cmap` lookup returns;
/// - UAX #9's rule L2 orders the runs and a right-to-left run's glyphs are
///   walked backwards, so an Arabic line is drawn from its last letter;
/// - each glyph carries the text of its own cluster, so a ligature extracts as
///   the characters it replaced rather than as one of them;
/// - **`GPOS`'s per-glyph offsets are carried**, so a mark sits where its
///   anchor puts it rather than where its advance does.
///
/// The direction is `Auto` rather than the caller's flag, matching
/// [`shape_with`]: `flow.rs` resolves no levels and passes `false` for every
/// run, so P2/P3 over the run's own text is the only answer either side has.
///
/// # The pen model, and why it is the shaper's own
///
/// `x = pen + x_offset` and `rise = y_offset`, with `pen` accumulating the
/// advances **in draw order** — the model
/// `crates/tinker-pdf-shape/tests/text_rendering.rs` measures the vendored
/// corpus with, so the positions this writes are the positions that suite
/// adjudicates. `glyph_run` then works out each `TJ` adjustment against its
/// own `/W`-rounded pen, so the difference between the shaper's advance and
/// the one a reader will use is absorbed glyph by glyph rather than
/// accumulating.
///
/// # `letter-spacing` is folded in here rather than left to `Tc`
///
/// Not a preference. `glyph_run`'s pen model does not know about `Tc`, so a
/// non-zero one would push glyph *k* by *k* × `Tc` past where this put it. And
/// `Tc` is applied by a reader per **glyph** while `tinker-pdf-layout`
/// measures `letter_spacing` per **character** that starts a typographic
/// character unit (`flow.rs`'s `measure`, asking [`letter_spaced`]), so a
/// ligature or a joined Arabic word was drawn *narrower* than the line box
/// it was measured into. Folding the spacing in at cluster boundaries — one
/// spacing for each character of the cluster that is not a nonspacing mark,
/// none between a mark and its base — makes the drawn width the measured
/// width by construction.
///
/// # Context
///
/// `context` is the text of the run's logical neighbours in the same face
/// ([`Fonts::set_contexts`]): the slice is shaped with it either side, and
/// only the slice's own glyphs come back, placed relative to the pen where
/// the first of them is drawn — so a glyph a neighbour offsets (a mark, the
/// second glyph of a pair) keeps the offset, and one a neighbour joins to
/// takes the joined form.
///
/// That placement holds only while the slice's own glyphs are **one stretch**
/// of the drawing order. A context in the other direction can be put between
/// them by L2 — `a ب` before `حم b` draws the context's `م` and `ح` between
/// the run's space and its `ب` — and the run then drew a gap the context's
/// width inside itself and pushed a glyph past its box onto its neighbour's
/// (review of lane 6C). Such a slice is shaped **alone**, as it was before
/// context existed: its glyphs are then all its own, and contiguous.
///
/// # `word-spacing` inside a piece
///
/// `word_spacing` is added to the pen after every own glyph that stands for a
/// space, in drawing order, and **not** to [`Shaped::advance`]: the caller
/// pays for the space at the end of a piece, where a left-to-right piece's
/// space is drawn and nothing follows it. A right-to-left piece draws its
/// space first, so its word is moved right by the extra, which is where the
/// gap belongs.
fn shaped_glyphs(
    program: &[u8],
    text: &str,
    size: f64,
    spacing: (f64, f64),
    context: (&str, &str),
    settings: &[(Tag, u32)],
    direction: BaseDirection,
) -> Option<Shaped> {
    let (letter_spacing, word_spacing) = spacing;
    let sfnt = Sfnt::parse(program)?;
    let upem = f64::from(sfnt.units_per_em.max(1));
    let scale = |units: i32| f64::from(units) * size / upem;
    let shaper = tinker_pdf_shape::Shaper::new(&sfnt).with_settings(settings);
    let (before, after) = context;
    let whole = format!("{before}{text}{after}");
    let own = before.len()..before.len() + text.len();
    let mine = |cluster: u32| usize::try_from(cluster).is_ok_and(|at| own.contains(&at));
    let paragraph = Paragraph::new(&whole, direction);
    let runs = itemize(&whole, &paragraph);
    let shaped: Vec<_> = runs.iter().map(|run| shaper.shape(&whole, run)).collect();
    let levels: Vec<_> = runs.iter().map(|run| run.level).collect();

    // The slice's own glyphs must be one stretch of the drawing order, or the
    // pen model below draws a context's width inside the run: shaped alone
    // instead, where every glyph is its own.
    if !(before.is_empty() && after.is_empty()) {
        let order = reorder(&levels);
        let mut stretches = 0usize;
        let mut inside = false;
        for index in &order {
            let Some(run) = shaped.get(*index) else {
                continue;
            };
            let glyphs = run.glyphs();
            let walk: Vec<usize> = if run.direction().is_forward() {
                (0..glyphs.len()).collect()
            } else {
                (0..glyphs.len()).rev().collect()
            };
            for at in walk {
                let own = glyphs.get(at).is_some_and(|glyph| mine(glyph.cluster));
                if own && !inside {
                    stretches += 1;
                }
                inside = own;
            }
        }
        if stretches > 1 {
            return shaped_glyphs(program, text, size, spacing, ("", ""), settings, direction);
        }
    }

    let mut out: Vec<Placed> = Vec::new();
    let mut pen = 0.0f64;
    // Where the pen stood at the first of this slice's own glyphs, in drawing
    // order: the slice's origin, which is where the run is put.
    let mut origin: Option<f64> = None;
    // `word-spacing` paid so far, after own spaces already drawn.
    let mut widened = 0.0f64;
    // Spaced characters whose clusters are already behind the pen, and those
    // of the cluster it is inside. `letter-spacing` is charged once per
    // character that is not a mark ([`letter_spaced`]) and paid at the
    // cluster boundary, so a mark keeps the position its anchor gave it.
    let mut spaced = 0usize;
    let mut pending = 0usize;
    let mut cluster: Option<u32> = None;
    for index in reorder(&levels) {
        let Some(run) = shaped.get(index) else {
            continue;
        };
        let glyphs = run.glyphs();
        // The text each glyph stands for is worked out in **logical** order,
        // because that is the order clusters are monotonic in; the reversal
        // for drawing happens after.
        let texts = crate::shaping::cluster_texts(&whole, run);
        let order: Vec<usize> = if run.direction().is_forward() {
            (0..glyphs.len()).collect()
        } else {
            (0..glyphs.len()).rev().collect()
        };
        for at in order {
            let Some(glyph) = glyphs.get(at) else {
                continue;
            };
            // A context glyph is shaped and not drawn: its neighbour run
            // draws it. It still moves the pen, because the pen is how the
            // shaper's positions are stated.
            if !mine(glyph.cluster) {
                pen += scale(glyph.x_advance);
                continue;
            }
            let start = *origin.get_or_insert(pen);
            if cluster.is_some_and(|last| last != glyph.cluster) {
                spaced = spaced.saturating_add(pending);
                pending = 0;
            }
            let stands_for = texts.get(at).copied().unwrap_or("");
            if !stands_for.is_empty() {
                pending = stands_for.chars().filter(|c| letter_spaced(*c)).count();
            }
            cluster = Some(glyph.cluster);
            out.push(Placed {
                id: glyph.glyph,
                text: stands_for.to_string(),
                x: pen - start + letter_spacing * spaced as f64 + widened + scale(glyph.x_offset),
                rise: scale(glyph.y_offset),
            });
            pen += scale(glyph.x_advance);
            let spaces = stands_for.chars().filter(|c| *c == ' ').count();
            if spaces > 0 {
                widened += word_spacing * spaces as f64;
            }
        }
    }
    // The slice's own advance: the pen's travel over its own glyphs, which is
    // every glyph's travel when there is no context.
    let own_advance: f64 = shaped
        .iter()
        .flat_map(|run| run.glyphs().iter())
        .filter(|glyph| mine(glyph.cluster))
        .map(|glyph| scale(glyph.x_advance))
        .sum();
    Some(Shaped {
        // The whole slice's `letter-spacing` rather than the sum of the
        // clusters', so the pen agrees with `flow.rs`'s `measure` exactly even
        // where a shaper dropped a character that started no cluster of its
        // own.
        advance: own_advance
            + letter_spacing * text.chars().filter(|c| letter_spaced(*c)).count() as f64,
        glyphs: out,
    })
}

/// `place` is where `run` stands in the page's list of runs, which its plan
/// is kept by ([`Fonts::plans`]).
fn draw_run(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
    place: Option<usize>,
    frame: &Frame,
    fonts: &Fonts<'_>,
) -> usize {
    draw_run_against(
        builder,
        page,
        run,
        place,
        frame,
        fonts,
        fonts.context_of(run),
        fonts.drawing_of(run),
    )
}

/// [`draw_run`], shaped against `context` — the text of a run's neighbours
/// ([`Fonts::set_contexts`]) — and drawn as `drawing` says
/// ([`Fonts::set_readings`]), rather than as recorded for the place `run` is
/// drawn at.
///
/// **For a text shadow**, which is its run drawn again somewhere else: the
/// context is found by where a run is drawn, so the shadow, a copy moved by
/// its offset, found none and was shaped alone — under a word whose middle
/// letter is a span of its own, the text joined and its shadow did not.
/// `css-text-decor-3` §4 makes the shadow the run's own glyphs again, so it is
/// shaped against the run's own neighbours. A shadow has no `place`, and is
/// planned anew.
#[allow(clippy::too_many_arguments)]
fn draw_run_against(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
    place: Option<usize>,
    frame: &Frame,
    fonts: &Fonts<'_>,
    (before, after): (String, String),
    drawing: Drawing,
) -> usize {
    let font = request(run);
    let size = run.font_size * PX_TO_PT;
    let baseline = frame.y(run.y);
    let mut x = run.x;
    let mut refused = 0usize;

    set_fill(page, run.color);
    // A run whose line [`Fonts::set_readings`] read back is written as it
    // planned the drawing it read, a standard-14 run's every face segment's
    // text objects in the order drawn ([`Fonts::plan_run`]): planned once.
    if let Some(place) = place {
        if let Some(Some((objects, end))) = fonts.plans.borrow().get(&(place, drawing)) {
            for object in objects {
                flush(page, object, size, baseline, run, frame);
            }
            decorate(page, run, frame, *end);
            return refused;
        }
    }
    #[cfg(test)]
    UNPLANNED.with(|unplanned| unplanned.set(unplanned.get().saturating_add(1)));
    // `css-fonts-4` §5.3 first, then shaping: see [`face_runs`]. An embedded
    // face's stretch is shaped whole; a standard-14 one keeps the
    // character-at-a-time path, because a simple font addresses a code and
    // there is no sfnt in this process to shape against.
    let mut segments = face_runs(fonts.faces(), &font, &run.text);
    // The neighbours' text belongs to the logically first and last segments,
    // which [`Fonts::continues`] has already checked are in the neighbours'
    // face; worked out before the drawing order reverses them.
    let first = segments.first().map(|(range, _)| range.start);
    let last = segments.last().map(|(range, _)| range.end);
    if reads_right_to_left(run) {
        segments.reverse();
    }
    for (range, chosen) in segments {
        let context = (
            if Some(range.start) == first {
                before.as_str()
            } else {
                ""
            },
            if Some(range.end) == last {
                after.as_str()
            } else {
                ""
            },
        );
        let slice = run.text.get(range).unwrap_or("");
        match chosen {
            Chosen::Embedded(index) => {
                let drawn = draw_shaped(
                    builder, page, run, frame, fonts, index, slice, context, size, baseline, x,
                );
                x = drawn.0;
                refused += drawn.1;
            }
            Chosen::Standard(_) => {
                x = draw_coded(page, run, frame, fonts, slice, size, baseline, x, drawing);
            }
        }
    }

    decorate(page, run, frame, x);
    refused
}

/// Whether a run reads right to left, by UAX #9's own P2 and P3 over its text.
///
/// # Why the question is asked here at all
///
/// [`face_runs`] resolves fallback **before** shaping, because a glyph index
/// means nothing outside the face it came from — so a right-to-left line whose
/// characters need two faces is two segments, and rule L2 has already been
/// applied *inside* each of them by the time either is drawn. Reversing the
/// glyphs of a segment orders the segment; it does not order the segments, and
/// a build that stopped there drew a two-face Arabic line as two left-to-right
/// pieces, each internally correct.
///
/// So L2 is applied at two levels: the segments of a right-to-left run are
/// drawn in reverse, and each keeps the glyph order its own shaping gave it.
/// That is the same two-step [`shaped_glyphs`] performs over one segment's
/// bidi runs, one level out.
///
/// **Across runs, the unit is the visual line**: [`visual_lines`] has already
/// put a right-to-left line's styled spans in L2's order before anything is
/// drawn, so what is ordered here is one run's own segments. What this closes
/// is the case fallback creates — one run, one style, several faces — which is
/// the case `docs/features/fonts.md` named.
///
/// **And a standard-14 segment is put in L2's order by [`coded_order`]**
/// before [`draw_coded`] writes it a character at a time: there is no sfnt
/// to shape against, but a code is drawn where the pen is, so the order the
/// codes are written in is the order the page shows. It was written as
/// typed, a right-to-left word backwards on the page, until ruling 14's
/// extraction read it back that way — see [`coded_order`].
fn right_to_left(text: &str) -> bool {
    Paragraph::new(text, BaseDirection::Auto)
        .base_level()
        .is_rtl()
}

/// Whether a run is drawn right to left: by the level its line resolved it
/// at ([`TextRun::bidi_level`]) where [`split_at_levels`] gave it one, and
/// by [`right_to_left`] over its own text where its line was left as it was.
///
/// The level is the answer and the text is only a proxy for it. The two
/// agree on any run with a strong character, and part on a run of neutrals:
/// the space and `!` that end a right-to-left paragraph are at level 1, and
/// read by their own text — no strong character, so P3's left to right —
/// they were drawn ` !` where L2 draws `! `.
fn reads_right_to_left(run: &TextRun) -> bool {
    run.bidi_level
        .map_or_else(|| right_to_left(&run.text), |level| level % 2 == 1)
}

/// The paragraph direction a slice is shaped in, context and all: the
/// slice's **own** P2 and P3, stated rather than left to the text it is
/// shaped beside.
///
/// Without a context the two are one answer. With one they are not, and the
/// difference is a run drawn backwards: ` b`, shaped after the Arabic word its
/// line draws before it, is a right-to-left paragraph by P2 — the first strong
/// character of `بحم b` is Arabic — so its space and its `b` came back in that
/// paragraph's visual order, `b` first, inside a run that reads left to right.
/// A context is there to be joined and positioned against; it decides no
/// direction.
fn own_direction(text: &str) -> BaseDirection {
    if right_to_left(text) {
        BaseDirection::RightToLeft
    } else {
        BaseDirection::LeftToRight
    }
}

/// Each run's text as it is drawn, `css-text-3` §5.4: its soft hyphens
/// removed, since one is invisible where no line breaks at it, and a hyphen
/// after a run whose line breaks at one ([`TextRun::hyphenated`]).
///
/// Layout keeps the soft hyphens in a run's text, because the text is the
/// book's and conservation counts every character of it, and measured each
/// as nothing and the line-ending one as a hyphen. This is the one place the
/// drawn text is made from it, before the line's levels are resolved, so
/// shaping, ordering, drawing, tagging and links all read what the page
/// shows. The hyphen is U+002D and reads back as one: extraction's opt-in
/// rejoining infers it, as it does any hyphen at a line end before a
/// lower-case letter, where a soft hyphen it would join for certain.
pub fn hyphenate(runs: &mut [TextRun]) {
    for run in runs {
        if run.text.contains('\u{AD}') {
            run.text.retain(|c| c != '\u{AD}');
        }
        if run.hyphenated {
            run.text.push('-');
        }
    }
}

/// UAX #9's rule L2 over each **visual line** of a page's runs, rather than
/// inside each run.
///
/// `flow.rs` breaks lines over logical text and resolves no levels, so a line
/// made of two styled spans is two `TextRun`s laid out left to right in the
/// order they were written — and an Arabic line whose second word is in a
/// different colour was drawn with that word on the right, reading backwards.
/// A run in one direction already drew its own glyphs in the right order; the
/// runs were not in it.
///
/// So this gives each run of the line a level — the one [`split_at_levels`]
/// cut it at, its paragraph's; for a line nobody cut, the level of its strong
/// characters (or of all of them, for a run of neutrals) with the line's text
/// resolved by itself — orders the runs by L2, an isolate's formatting
/// characters between two runs standing between them at their own level
/// ([`TextRun::bidi_gap`]), and lays them out again from
/// the line's left edge in that order, each at its own measured width. The
/// line's extent does not change, so its alignment does not either; only
/// which run sits where.
///
/// # What a line is, here
///
/// Layout places a line's runs **contiguously** — each `x` is the previous
/// one's `x` plus its width, computed in that order (`flow.rs`'s alignment
/// pass) — on baselines a `vertical-align` may move by at most a few ems. So
/// consecutive runs whose ends meet, and whose baselines are within the larger
/// font size of each other, are one line; a new line starts at the left edge
/// again and does not meet the last one's end, and two table cells on one
/// baseline are separated by their cells' own edges. An `outside` list
/// marker is not part of any line's reordering: it sits where the list put
/// it ([`beside_the_line`]).
///
/// # An `inside` marker is one unit, at the paragraph's level
///
/// It is the first inline box of its item (CSS 2.2 §12.5.1), and
/// `css-lists-3` §3.1's user-agent rule makes every marker `unicode-bidi:
/// isolate`: `LRI` or `RLI` before it and `PDI` after, in the item's
/// direction. Seen from outside, an isolate is one neutral (UAX #9 X5a to
/// X6a), and one at its paragraph's start lies between `sos` and the next
/// strong character, so N1 or N2 gives it the paragraph's own direction
/// whatever follows it: it is at the paragraph's level, the lowest on the
/// line. So [`split_line`] orders it whole at that level and draws it in
/// the paragraph's direction — first on the left of a left-to-right line
/// and first on the right of a right-to-left one, its item's text after it
/// — and leaves its characters out of the text it resolves.
///
/// A line with no right-to-left character is not touched, so no
/// left-to-right page moves. Each run is still drawn in its own direction
/// inside itself, by [`draw_run`]. A run that **mixes** directions is not one
/// unit L2 can place, so [`split_at_levels`] cuts it at its line's level
/// boundaries first, and every run this orders is at one level.
///
/// # An atomic inline is one unit, and moves whole
///
/// An inline block or a replaced element set inline ([`Atomics`]) is one
/// unit of the line it is set on, which UAX #9 reads as U+FFFC OBJECT
/// REPLACEMENT CHARACTER, a neutral (`css-writing-modes-3` §2.4):
/// [`split_at_levels`] sets a stand-in for it on that line ([`set_objects`]),
/// its runs are ordered on lines of their own, as the paragraph of their own
/// they are, and where L2 moves the stand-in, everything set in the atomic
/// inline — its runs, its box fragments, its picture and its clips — moves
/// with it by the same distance ([`carry`]), so its transform's origin, its
/// background, its border and its outline stand where its text is drawn.
/// The stand-ins are taken off the page here.
///
/// Returns how many lines moved.
pub fn visual_lines(page: &mut LayoutPage, atomics: &Atomics) -> usize {
    if !page.runs.iter().any(is_object) {
        return lines_in_order(&mut page.runs);
    }
    let levels: Vec<Option<u32>> = page.runs.iter().map(|run| atomics.level(run)).collect();
    let was: Vec<f64> = page.runs.iter().map(|run| run.x).collect();
    let mut moved = 0usize;
    for line in level_lines(&page.runs, &levels) {
        if line.len() > 1 && reorder_at(&mut page.runs, &line) {
            moved += 1;
        }
    }
    let mut shift: BTreeMap<u32, f64> = BTreeMap::new();
    for (run, was) in page.runs.iter().zip(&was) {
        let Some(atomic) = run.anchor.filter(|_| is_object(run)) else {
            continue;
        };
        let by = run.x - was;
        if by != 0.0 && by.is_finite() {
            shift.insert(atomic, by);
        }
    }
    page.runs.retain(|run| !is_object(run));
    if !shift.is_empty() {
        carry(page, atomics, &shift);
    }
    moved
}

/// [`visual_lines`] over runs that hold no stand-in: each line is the runs
/// in order whose ends meet ([`same_line`]).
fn lines_in_order(runs: &mut [TextRun]) -> usize {
    let mut moved = 0usize;
    let mut start = 0usize;
    while start < runs.len() {
        let mut end = start + 1;
        while end < runs.len() && same_line(&runs[end - 1], &runs[end]) {
            end += 1;
        }
        if let Some(line) = runs.get_mut(start..end) {
            if line.len() > 1 && reorder_line(line) {
                moved += 1;
            }
        }
        start = end;
    }
    moved
}

/// The lines of `runs`, each a list of indices in order: the runs of one
/// level ([`Atomics::level`]) whose ends meet ([`same_line`]), so that an
/// atomic inline's own runs are lines of their own and its stand-in is one
/// unit of the line it is set on.
fn level_lines(runs: &[TextRun], levels: &[Option<u32>]) -> Vec<Vec<usize>> {
    let mut open: BTreeMap<Option<u32>, Vec<usize>> = BTreeMap::new();
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for (at, (run, level)) in runs.iter().zip(levels).enumerate() {
        let continues = open
            .get(level)
            .and_then(|line| line.last())
            .and_then(|last| runs.get(*last))
            .is_some_and(|last| same_line(last, run));
        if !continues {
            if let Some(done) = open.remove(level) {
                lines.push(done);
            }
        }
        open.entry(*level).or_default().push(at);
    }
    lines.extend(open.into_values());
    lines
}

/// Moves everything set in each atomic inline `shift` names by its distance:
/// the runs, box fragments, pictures and clips anchored in it, through every
/// atomic inline it is set in ([`Atomics::chain`]).
fn carry(page: &mut LayoutPage, atomics: &Atomics, shift: &BTreeMap<u32, f64>) {
    let by = |anchor: Option<u32>| -> f64 {
        atomics
            .chain(anchor)
            .filter_map(|atomic| shift.get(&atomic))
            .sum()
    };
    for run in &mut page.runs {
        let dx = by(run.anchor);
        if dx != 0.0 {
            run.x += dx;
        }
    }
    for fragment in &mut page.boxes {
        let dx = by(fragment.anchor);
        if dx != 0.0 {
            fragment.x += dx;
        }
    }
    for fragment in &mut page.replaced {
        let dx = by(fragment.anchor);
        if dx != 0.0 {
            fragment.x += dx;
        }
    }
    for clip in &mut page.clips {
        let dx = by(Some(clip.anchor));
        if dx != 0.0 {
            clip.x += dx;
        }
    }
}

/// The [`TextRun::order`] of an atomic inline's stand-in on its line
/// ([`set_objects`]). No run layout makes carries it: its stamps count up
/// from one.
const OBJECT: usize = usize::MAX;

/// Whether `run` is an atomic inline's stand-in ([`set_objects`]).
fn is_object(run: &TextRun) -> bool {
    run.order == OBJECT
}

/// How far a fragment's edge may lie from a run's and still meet it, in
/// layout pixels: the two are sums of different terms.
const SLACK: f64 = 1e-3;

/// How many comparisons [`set_objects`] makes on one page before it sets no
/// stand-in there. Finding where each atomic inline stands compares the
/// page's runs with its atomic inlines, which a page a book crafts with many
/// of both makes quadratic: a page past this orders its lines by their runs
/// alone, each atomic inline's box where layout set it, as a page with none
/// does ([`visual_lines`]).
pub(crate) const OBJECT_WORK: usize = 4_000_000;

/// The comparisons [`set_objects`] has made on a page, against
/// [`OBJECT_WORK`].
struct Work(usize);

impl Work {
    /// Charges `n` comparisons; false once the page's are spent.
    fn charge(&mut self, n: usize) -> bool {
        self.0 = self.0.saturating_add(n);
        #[cfg(test)]
        OBJECT_PEAK.with(|peak| peak.set(peak.get().max(self.0)));
        self.0 <= OBJECT_WORK
    }
}

/// The atomic inlines of a chapter, CSS 2.2 §9.2.2 — an `inline-block`, an
/// `inline-flex` and a replaced element set inline — each one unit of the
/// line it is set on ([`visual_lines`]), and what is set in each.
///
/// Found in the box tree layout was given, as layout finds them (`flow.rs`'s
/// `Builder::children` and `Builder::gather`): a box gathered into a line —
/// an inline-level child of a block container, or a child of an inline box —
/// that is not `display: none`, floated or absolutely positioned, and is an
/// `inline-block`, an `inline-flex` or a replaced element. The page holds no
/// mark of one: a fragment says whose box it is ([`BoxFragment::anchor`]) and
/// a run whose characters it holds ([`TextRun::anchor`]), and only the tree
/// says which elements are set inline and which is inside which.
#[derive(Clone, Debug, Default)]
pub struct Atomics {
    /// For each anchor set inside an atomic inline, the innermost one: the
    /// element itself, where it is one.
    within: BTreeMap<u32, u32>,
    /// For each atomic inline, the atomic inline its own box is set in.
    around: BTreeMap<u32, Option<u32>>,
    /// For each atomic inline, the embeddings its inline ancestors open
    /// round it, as `flow.rs`'s `embedding_of` opens them: what the text of
    /// its line writes round its U+FFFC.
    embeddings: BTreeMap<u32, Arc<[Embedding]>>,
}

impl Atomics {
    /// The atomic inlines of `tree`, the box tree a chapter is laid out from.
    #[must_use]
    pub fn of(tree: &BoxNode) -> Atomics {
        /// How a box's parent sets it.
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Set {
            /// A child of a block container, gathered into its lines where it
            /// is inline-level.
            Block,
            /// A child of an inline box, gathered into the line.
            Inline,
            /// The root, a flex item, a table's row or a row's cell: on no
            /// line.
            Apart,
        }
        /// A box still to walk: how its parent sets it, the atomic inline
        /// it is set in, and the embeddings open round it.
        type Pending<'t> = (&'t BoxNode, Set, Option<u32>, Arc<[Embedding]>);
        let mut out = Atomics::default();
        let mut stack: Vec<Pending<'_>> = vec![(tree, Set::Apart, None, Arc::default())];
        while let Some((node, set, within, open)) = stack.pop() {
            let style = consume(&node.style);
            if style.is_none() {
                continue;
            }
            let out_of_flow = style.float != Float::None
                || matches!(style.position, Position::Absolute | Position::Fixed);
            let gathered = !out_of_flow
                && match set {
                    Set::Inline => true,
                    Set::Block => !style.is_block_level() && !style.is_internal_table(),
                    Set::Apart => false,
                };
            let atomic = gathered
                && (matches!(style.display, Display::InlineBlock | Display::InlineFlex)
                    || matches!(node.content, Content::Replaced(_)));
            let here = match node.anchor {
                Some(anchor) if atomic => {
                    out.around.insert(anchor, within);
                    out.embeddings.insert(anchor, open.clone());
                    Some(anchor)
                }
                _ => within,
            };
            if let (Some(anchor), Some(inner)) = (node.anchor, here) {
                out.within.entry(anchor).or_insert(inner);
            }
            let Content::Children(children) = &node.content else {
                continue;
            };
            let (set, open) = if atomic {
                let set = if style.is_flex() {
                    Set::Apart
                } else {
                    Set::Block
                };
                (set, Arc::default())
            } else if gathered {
                match opened_by(&style, node.anchor).filter(|_| open.len() < MAX_EMBEDDING_DEPTH) {
                    Some(embedding) => {
                        let mut more = open.to_vec();
                        more.push(embedding);
                        (Set::Inline, more.into())
                    }
                    None => (Set::Inline, open),
                }
            } else if style.is_flex()
                || style.is_table()
                || (style.is_internal_table()
                    && !matches!(style.display, Display::TableCell | Display::TableCaption))
            {
                (Set::Apart, Arc::default())
            } else {
                (Set::Block, Arc::default())
            };
            for child in children.iter().rev() {
                stack.push((child, set, here, open.clone()));
            }
        }
        out
    }

    /// Whether the chapter holds no atomic inline.
    fn is_empty(&self) -> bool {
        self.around.is_empty()
    }

    /// Whether the element `anchor` is an atomic inline.
    fn is_atomic(&self, anchor: u32) -> bool {
        self.around.contains_key(&anchor)
    }

    /// The innermost atomic inline the element `anchor` is set in, the
    /// element itself included.
    fn innermost(&self, anchor: Option<u32>) -> Option<u32> {
        anchor.and_then(|anchor| self.within.get(&anchor).copied())
    }

    /// The atomic inline `atomic`'s own box is set in.
    fn around(&self, atomic: u32) -> Option<u32> {
        self.around.get(&atomic).copied().flatten()
    }

    /// Which lines `run` is ordered with: an atomic inline's own (`Some`),
    /// or those set in none (`None`). A stand-in is on the line of the atomic
    /// inline it stands for is set in.
    fn level(&self, run: &TextRun) -> Option<u32> {
        if is_object(run) {
            run.anchor.and_then(|atomic| self.around(atomic))
        } else {
            self.innermost(run.anchor)
        }
    }

    /// The atomic inlines the element `anchor` is set in, innermost first.
    /// Each is set in one whose element comes before it, so the walk ends;
    /// it is held to the count of them besides.
    fn chain(&self, anchor: Option<u32>) -> impl Iterator<Item = u32> + '_ {
        let mut next = self.innermost(anchor);
        let mut left = self.around.len();
        core::iter::from_fn(move || {
            let at = next?;
            left = left.checked_sub(1)?;
            next = self.around(at);
            Some(at)
        })
    }
}

/// The embedding an inline box opens round its content, `css-writing-modes-3`
/// §2.4.2, as `flow.rs`'s `embedding_of` opens it.
fn opened_by(style: &Consumed, anchor: Option<u32>) -> Option<Embedding> {
    let kind = match style.unicode_bidi {
        UnicodeBidi::Normal => return None,
        UnicodeBidi::Embed => EmbeddingKind::Embed,
        UnicodeBidi::Isolate => EmbeddingKind::Isolate,
        UnicodeBidi::Plaintext => EmbeddingKind::FirstStrong,
    };
    Some(Embedding {
        kind,
        rtl: style.direction == CssDirection::Rtl,
        anchor,
    })
}

/// A box on the page in layout pixels, grown to hold each rectangle it takes.
#[derive(Clone, Copy, Debug)]
struct Hull {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Default for Hull {
    fn default() -> Self {
        Hull {
            left: f64::INFINITY,
            top: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::NEG_INFINITY,
        }
    }
}

impl Hull {
    /// Grows to hold `(x, y, width, height)`.
    fn take(&mut self, (x, y, width, height): (f64, f64, f64, f64)) {
        self.left = self.left.min(x);
        self.top = self.top.min(y);
        self.right = self.right.max(x + width);
        self.bottom = self.bottom.max(y + height);
    }

    /// Whether it holds anything, at finite places.
    fn holds(&self) -> bool {
        finite(&[self.left, self.top, self.right, self.bottom])
            && self.left <= self.right
            && self.top <= self.bottom
    }

    /// Whether it stands on `run`'s baseline: the baseline crosses it, as a
    /// box set on a line at its baseline is crossed by the line's — a picture
    /// at its bottom edge, an inline block at its last line's baseline. A
    /// box of the line above or below, whose text stands a line height away,
    /// is not crossed by it.
    fn on(&self, run: &TextRun) -> bool {
        self.top - SLACK <= run.y && run.y <= self.bottom + SLACK
    }
}

/// Sets each atomic inline of a page on its line as one U+FFFC OBJECT
/// REPLACEMENT CHARACTER, a stand-in [`split_at_levels`] and
/// [`visual_lines`] order with the line's runs ([`OBJECT`]): unpainted, in
/// its line's paragraph, inside the embeddings its inline ancestors open
/// ([`Atomics`]), and as wide as the stretch of the line it takes.
///
/// Its box on the page is its own fragments' — its border box, its picture's
/// content box — or, for one with none, the runs set in it. It is on the
/// line, of the runs set in what it is set in, that it stands on
/// ([`Hull::on`]) and either lies between two of whose runs, of one
/// paragraph, or meets an end of ([`lines_across`], [`stands_on`]). It takes
/// the stretch from the unit before it to the one after; at the line's ends,
/// its own box's edge. A line of nothing but atomic inlines holds no run to
/// find it by, and an atomic inline set apart from its neighbours by its
/// margins only between two runs; it is not set. A page whose search passes
/// [`OBJECT_WORK`] comparisons sets none.
fn set_objects(page: &mut LayoutPage, atomics: &Atomics) {
    if atomics.is_empty() {
        return;
    }
    let mut boxes: BTreeMap<u32, Hull> = BTreeMap::new();
    let own = page
        .boxes
        .iter()
        .map(|b| (b.anchor, (b.x, b.y, b.width, b.height)))
        .chain(
            page.replaced
                .iter()
                .map(|r| (r.anchor, (r.x, r.y, r.width, r.height))),
        );
    for (anchor, rect) in own {
        if let Some(atomic) = anchor.filter(|anchor| atomics.is_atomic(*anchor)) {
            boxes.entry(atomic).or_default().take(rect);
        }
    }
    let mut texts: BTreeMap<u32, Hull> = BTreeMap::new();
    for run in &page.runs {
        for atomic in atomics.chain(run.anchor) {
            if !boxes.contains_key(&atomic) {
                texts.entry(atomic).or_default().take((
                    run.x,
                    run.y - run.font_size,
                    run.width,
                    run.font_size * 1.25,
                ));
            }
        }
    }
    for (atomic, hull) in texts {
        boxes.entry(atomic).or_insert(hull);
    }
    let mut kids: BTreeMap<Option<u32>, Vec<(u32, Hull)>> = BTreeMap::new();
    for (atomic, hull) in boxes {
        if hull.holds() {
            kids.entry(atomics.around(atomic))
                .or_default()
                .push((atomic, hull));
        }
    }
    if kids.is_empty() {
        return;
    }
    let levels: Vec<Option<u32>> = page
        .runs
        .iter()
        .map(|run| atomics.innermost(run.anchor))
        .collect();
    let mut objects: Vec<(usize, TextRun)> = Vec::new();
    let mut work = Work(0);
    for (level, kids) in &kids {
        if !work.charge(levels.len()) {
            return;
        }
        let own: Vec<usize> = levels
            .iter()
            .enumerate()
            .filter(|(_, at)| *at == level)
            .map(|(at, _)| at)
            .collect();
        let mut placed = vec![false; kids.len()];
        let Some(lines) = lines_across(&page.runs, &own, kids, &mut work) else {
            return;
        };
        for line in lines {
            let set = stands_on(
                &page.runs,
                &line,
                kids,
                &mut placed,
                atomics,
                &mut objects,
                &mut work,
            );
            if set.is_none() {
                return;
            }
        }
    }
    if objects.is_empty() {
        return;
    }
    objects.sort_by_key(|(at, _)| *at);
    let taken = std::mem::take(&mut page.runs);
    page.runs.reserve(taken.len() + objects.len());
    let mut objects = objects.into_iter().peekable();
    for (at, run) in taken.into_iter().enumerate() {
        while let Some((_, object)) = objects.next_if(|(before, _)| *before <= at) {
            page.runs.push(object);
        }
        page.runs.push(run);
    }
    page.runs.extend(objects.map(|(_, object)| object));
}

/// The lines of the runs `own` of `runs`, in order, as [`same_line`] finds
/// them, but that two runs of one paragraph are one line across a stretch
/// one of `kids` lies in, on their baseline ([`set_objects`]). `None` once
/// the page's `work` is spent.
fn lines_across(
    runs: &[TextRun],
    own: &[usize],
    kids: &[(u32, Hull)],
    work: &mut Work,
) -> Option<Vec<Vec<usize>>> {
    let bridged = |a: &TextRun, b: &TextRun| {
        let end = a.x + a.width;
        !beside_the_line(a)
            && !beside_the_line(b)
            && a.paragraph == b.paragraph
            && (a.y - b.y).abs() <= a.font_size.max(b.font_size)
            && end <= b.x + SLACK
            && kids.iter().any(|(_, hull)| {
                hull.left >= end - SLACK && hull.right <= b.x + SLACK && hull.on(b)
            })
    };
    let mut lines: Vec<Vec<usize>> = Vec::new();
    for &at in own {
        let Some(run) = runs.get(at) else {
            continue;
        };
        let continues = match lines
            .last()
            .and_then(|line| line.last())
            .and_then(|last| runs.get(*last))
        {
            Some(last) if same_line(last, run) => true,
            Some(last) => {
                if !work.charge(kids.len()) {
                    return None;
                }
                bridged(last, run)
            }
            None => false,
        };
        match lines.last_mut() {
            Some(line) if continues => line.push(at),
            _ => lines.push(vec![at]),
        }
    }
    Some(lines)
}

/// One unit of a line as [`stands_on`] sets it: a run, by its index on the
/// page, or an atomic inline, by its index among the kids.
#[derive(Clone, Copy)]
enum Unit {
    Run(usize),
    Kid(usize),
}

/// Finds the atomic inlines among `kids` not yet `placed` that stand on
/// `line` — between two of its runs, or meeting one of its ends, through
/// each other — and adds a stand-in for each to `objects`, with the index of
/// the run it goes before on the page ([`set_objects`]). `None` once the
/// page's `work` is spent.
fn stands_on(
    runs: &[TextRun],
    line: &[usize],
    kids: &[(u32, Hull)],
    placed: &mut [bool],
    atomics: &Atomics,
    objects: &mut Vec<(usize, TextRun)>,
    work: &mut Work,
) -> Option<()> {
    let (Some(first), Some(last)) = (
        line.first().and_then(|at| runs.get(*at)),
        line.last().and_then(|at| runs.get(*at)),
    ) else {
        return Some(());
    };
    let mut members: Vec<usize> = Vec::new();
    for pair in line.windows(2) {
        let (Some(a), Some(b)) = (
            pair.first().and_then(|at| runs.get(*at)),
            pair.get(1).and_then(|at| runs.get(*at)),
        ) else {
            continue;
        };
        let end = a.x + a.width;
        if !work.charge(kids.len()) {
            return None;
        }
        for (kid, (_, hull)) in kids.iter().enumerate() {
            let free = placed.get(kid).is_some_and(|placed| !placed);
            if free && hull.left >= end - SLACK && hull.right <= b.x + SLACK && hull.on(b) {
                if let Some(slot) = placed.get_mut(kid) {
                    *slot = true;
                }
                members.push(kid);
            }
        }
    }
    let (mut left, mut right) = (first.x, last.x + last.width);
    loop {
        if !work.charge(kids.len()) {
            return None;
        }
        let mut grew = false;
        for (kid, (_, hull)) in kids.iter().enumerate() {
            if placed.get(kid).is_none_or(|placed| *placed) {
                continue;
            }
            let before = (hull.right - left).abs() <= SLACK && hull.on(first);
            let after = (hull.left - right).abs() <= SLACK && hull.on(last);
            if before || after {
                if let Some(slot) = placed.get_mut(kid) {
                    *slot = true;
                }
                members.push(kid);
                if before {
                    left = hull.left;
                } else {
                    right = hull.right;
                }
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    if members.is_empty() {
        return Some(());
    }
    // In the order layout set them, left to right: a kid before a run at the
    // same edge, since only an empty one can start where a run does.
    let mut units: Vec<(f64, u8, Unit)> = line
        .iter()
        .filter_map(|at| runs.get(*at).map(|run| (run.x, 1u8, Unit::Run(*at))))
        .chain(members.iter().filter_map(|kid| {
            kids.get(*kid)
                .map(|(_, hull)| (hull.left, 0u8, Unit::Kid(*kid)))
        }))
        .collect();
    units.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut edge: Option<f64> = None;
    for (at, (_, _, unit)) in units.iter().enumerate() {
        let kid = match unit {
            Unit::Run(index) => {
                edge = runs.get(*index).map(|run| run.x + run.width);
                continue;
            }
            Unit::Kid(kid) => *kid,
        };
        let Some(&(atomic, hull)) = kids.get(kid) else {
            continue;
        };
        let from = edge.unwrap_or(hull.left);
        let to = match units.get(at + 1).map(|next| next.2) {
            Some(Unit::Run(index)) => runs.get(index).map_or(hull.right, |run| run.x),
            _ => hull.right,
        }
        .max(from);
        edge = Some(to);
        let after = units
            .get(at + 1..)
            .unwrap_or(&[])
            .iter()
            .find_map(|unit| match unit.2 {
                Unit::Run(index) => Some(index),
                Unit::Kid(_) => None,
            });
        let before = units
            .get(..at)
            .unwrap_or(&[])
            .iter()
            .rev()
            .find_map(|unit| match unit.2 {
                Unit::Run(index) => Some(index),
                Unit::Kid(_) => None,
            });
        let (template, goes) = match (after, before) {
            (Some(index), _) => (index, index),
            (None, Some(index)) => (index, index + 1),
            (None, None) => continue,
        };
        let Some(template) = runs.get(template) else {
            continue;
        };
        let mut object = template.clone();
        object.x = from;
        object.width = to - from;
        object.text = "\u{FFFC}".to_owned();
        object.embeddings = atomics.embeddings.get(&atomic).cloned().unwrap_or_default();
        object.bidi_level = None;
        object.bidi_gap = None;
        object.bidi_line_rtl = false;
        object.hyphenated = false;
        object.painted = false;
        object.generated = false;
        object.letter_spacing = 0.0;
        object.word_spacing = 0.0;
        object.anchor = Some(atomic);
        object.order = OBJECT;
        objects.push((goes, object));
    }
    Some(())
}

/// Cuts every run that crosses one of its line's UAX #9 level boundaries
/// into one run per level, before [`visual_lines`] orders the line — every
/// page of a chapter at once, because the levels are the **paragraph's**.
///
/// # Why the run cannot be the unit L2 moves
///
/// L2 reverses stretches of characters by level, and [`visual_lines`] can only
/// move whole runs. A run is one element's text on one line, and an element
/// boundary falls wherever the markup put it: in `a ب<span>ح</span>م b` the
/// first run is `a ب` and the last `م b`, each half one direction and half the
/// other. Giving each one level — the lowest of its strong characters, which
/// is what [`visual_lines`] did with them — left `ب` and `م` with the Latin
/// either side of them, and the word was drawn `ب ح م` from the left: in the
/// order it was typed, so it read backwards. Each run ordered inside itself by
/// its own P2 and P3 cannot fix that, because the levels that matter are the
/// line's: the run `a ب` resolved alone puts `ب` at level 1 and has no idea
/// that the `ح` it joins is in the next run.
///
/// So the line's text is resolved, each run is cut wherever the level
/// changes inside it — `a ` and `ب`, `م` and ` b` — and every piece is at one
/// level, which is a unit L2 can place. The pieces keep the run's style,
/// anchor and document order, so drawing, links, tags and text extraction see
/// two runs of one element where there was one; and shaping across them is
/// [`Fonts::set_contexts`]'s, so `ب`, `ح` and `م` still join.
///
/// # The levels are the paragraph's, and only L1 and L2 the line's
///
/// UAX #9 resolves a paragraph — X1 to I2 — and breaks it into lines after:
/// a weak or neutral character at a line's start or end takes its level from
/// the strong characters either side of it **in the paragraph**, which may be
/// on the line before or after. Each line used to be resolved as a paragraph
/// of its own, its start and end against `sos` and `eos`, so where a line
/// wrapped changed the order inside it: `abc (de` in a right-to-left
/// paragraph draws `(de` unwrapped — `(` between `c` and `d`, both `L`, is
/// `L` by N1 — and drew `de(` when the line broke before `(`, which N2 then
/// put at the paragraph's level (review of lane 8C).
///
/// So the chapter's runs are gathered by the paragraph `flow.rs` set them in
/// ([`TextRun::paragraph`]), across pages, and each paragraph is resolved
/// once over its whole text ([`paragraphs`]); a line then takes its
/// characters' levels from [`Paragraph::line`], which applies L1 — trailing
/// whitespace back to the paragraph's level — at that line's end. Two limits
/// stand: the white space a line's end hangs is in no run, so it is not in
/// the resolved text either — a neutral, which L1 resets anyway — and a line
/// whose runs are not all of one paragraph (an inline block's own text
/// touching the line it sits in) is resolved by itself, as before.
///
/// # The pieces' widths are the run's, partitioned
///
/// Layout measured the run whole, and the line's other runs were placed
/// against that width. The pieces are not measured again on their own: the
/// run is shaped once, as layout shaped it, and each glyph's advance goes to
/// the piece its cluster starts in, with `letter-spacing` per character and
/// `word-spacing` per space as `flow.rs` charges them. The last piece takes
/// what the others leave of the run's width, so the pieces end exactly where
/// the run did and the line's extent — and with it its alignment — does not
/// move.
///
/// A character UAX #9's X9 removes (a joiner, a format control) has no level
/// of its own; it stays with the character before it, so a `ZWJ` inside a
/// word does not cut the word.
///
/// # An atomic inline is one U+FFFC of its line
///
/// Each atomic inline is set on its line first, as a stand-in of one U+FFFC
/// ([`set_objects`]), which is in its paragraph's text as UAX #9 reads it, a
/// neutral; its own runs are lines of their own ([`level_lines`]).
/// [`visual_lines`] takes the stand-ins off again.
///
/// A line with no right-to-left character is not touched. Returns how many
/// runs were cut.
pub fn split_at_levels(
    pages: &mut [LayoutPage],
    metrics: &BookMetrics<'_>,
    atomics: &Atomics,
) -> usize {
    for page in pages.iter_mut() {
        set_objects(page, atomics);
    }
    let (resolved, places) = paragraphs(pages);
    let mut cut = 0usize;
    for (page, places) in pages.iter_mut().zip(places) {
        let taken = std::mem::take(&mut page.runs);
        // Padded rather than zipped short: a run with no place is resolved by
        // its line alone, and a run the zip dropped would be text lost.
        let places = places.into_iter().chain(std::iter::repeat(None));
        if taken.iter().any(is_object) {
            let places: Vec<Place> = places.take(taken.len()).collect();
            cut += split_lines_of_levels(taken, &places, &resolved, metrics, atomics, page);
            continue;
        }
        page.runs.reserve(taken.len());
        let mut line: Vec<TextRun> = Vec::new();
        let mut at: Vec<Place> = Vec::new();
        for (run, place) in taken.into_iter().zip(places) {
            if line.last().is_some_and(|last| !same_line(last, &run)) {
                cut += split_line(&mut line, &at, &resolved, metrics, &mut page.runs, None);
                at.clear();
            }
            line.push(run);
            at.push(place);
        }
        cut += split_line(&mut line, &at, &resolved, metrics, &mut page.runs, None);
    }
    cut
}

/// [`split_at_levels`] over a page that holds a stand-in: each line is one
/// level's ([`level_lines`]), and what [`split_line`] makes of each run is
/// put back where the run was, so the page keeps its order.
fn split_lines_of_levels(
    taken: Vec<TextRun>,
    places: &[Place],
    resolved: &BTreeMap<usize, Paragraph>,
    metrics: &BookMetrics<'_>,
    atomics: &Atomics,
    page: &mut LayoutPage,
) -> usize {
    let levels: Vec<Option<u32>> = taken.iter().map(|run| atomics.level(run)).collect();
    let lines = level_lines(&taken, &levels);
    let mut slots: Vec<Option<TextRun>> = taken.into_iter().map(Some).collect();
    let mut made: Vec<Vec<TextRun>> = Vec::new();
    made.resize_with(slots.len(), Vec::new);
    let mut cut = 0usize;
    for indices in lines {
        let mut line: Vec<TextRun> = Vec::with_capacity(indices.len());
        let mut at: Vec<Place> = Vec::with_capacity(indices.len());
        let mut kept: Vec<usize> = Vec::with_capacity(indices.len());
        for index in indices {
            if let Some(run) = slots.get_mut(index).and_then(Option::take) {
                line.push(run);
                at.push(places.get(index).copied().flatten());
                kept.push(index);
            }
        }
        let mut out: Vec<TextRun> = Vec::with_capacity(line.len());
        let mut ends: Vec<usize> = Vec::with_capacity(line.len());
        cut += split_line(&mut line, &at, resolved, metrics, &mut out, Some(&mut ends));
        let mut out = out.into_iter();
        let mut from = 0usize;
        for (index, end) in kept.into_iter().zip(ends) {
            let pieces: Vec<TextRun> = out.by_ref().take(end.saturating_sub(from)).collect();
            if let Some(slot) = made.get_mut(index) {
                slot.extend(pieces);
            }
            from = end;
        }
    }
    page.runs = made.into_iter().flatten().collect();
    cut
}

/// Where a run's characters are in its bidi paragraph: the paragraph's
/// number ([`TextRun::paragraph`]) and the index, in [`paragraphs`]'s text
/// for it, of the run's first character. `None` for a generated run or one
/// set outside every paragraph.
type Place = Option<(usize, usize)>;

/// Every bidi paragraph of a chapter's pages that UAX #9 has anything to do
/// with — a right-to-left character, embedding or base in it — resolved once,
/// X1 to I2, over its whole text; and, per page and run, where the run's
/// characters are in it.
///
/// A paragraph's text is its runs' text in the order `flow.rs` set them,
/// which is logical order — [`visual_lines`] has not moved anything yet —
/// with each run's embeddings written round it as formatting characters, as
/// [`line_levels`] writes a line's: the ones a run shares with the run
/// before it left open, across a line's end and a page's as well. Generated
/// runs stay out of it: an `outside` marker is in no line, and an `inside`
/// one is an isolate, whose own characters decide no level outside it
/// ([`visual_lines`]).
fn paragraphs(pages: &[LayoutPage]) -> (BTreeMap<usize, Paragraph>, Vec<Vec<Place>>) {
    struct Gathered {
        text: String,
        count: usize,
        open: Vec<Embedding>,
        base: Option<bool>,
        right_to_left: bool,
    }
    let mut gathered: BTreeMap<usize, Gathered> = BTreeMap::new();
    let mut places: Vec<Vec<Place>> = Vec::with_capacity(pages.len());
    for page in pages {
        let mut here: Vec<Place> = Vec::with_capacity(page.runs.len());
        for run in &page.runs {
            if run.generated || run.paragraph == 0 {
                here.push(None);
                continue;
            }
            let paragraph = gathered.entry(run.paragraph).or_insert_with(|| Gathered {
                text: String::new(),
                count: 0,
                open: Vec::new(),
                base: run.paragraph_rtl,
                right_to_left: run.paragraph_rtl == Some(true),
            });
            let shared = paragraph
                .open
                .iter()
                .zip(run.embeddings.iter())
                .take_while(|(a, b)| a == b)
                .count();
            while paragraph.open.len() > shared {
                if let Some(e) = paragraph.open.pop() {
                    paragraph.text.push(closer(&e));
                    paragraph.count += 1;
                }
            }
            for e in run.embeddings.iter().skip(shared) {
                paragraph.text.push(opener(e));
                paragraph.count += 1;
                paragraph.open.push(*e);
                paragraph.right_to_left |= e.rtl && e.kind != EmbeddingKind::FirstStrong;
            }
            here.push(Some((run.paragraph, paragraph.count)));
            for c in run.text.chars() {
                paragraph.text.push(c);
                paragraph.count += 1;
                paragraph.right_to_left |= opens_right_to_left(c);
            }
        }
        places.push(here);
    }
    let resolved = gathered
        .into_iter()
        .filter(|(_, paragraph)| paragraph.right_to_left)
        .map(|(number, mut paragraph)| {
            while let Some(e) = paragraph.open.pop() {
                paragraph.text.push(closer(&e));
            }
            let direction = match paragraph.base {
                Some(true) => BaseDirection::RightToLeft,
                Some(false) => BaseDirection::LeftToRight,
                None => BaseDirection::Auto,
            };
            (number, Paragraph::new(&paragraph.text, direction))
        })
        .collect();
    (resolved, places)
}

/// The formatting character that opens an embedding (`css-writing-modes-3`
/// §2.4.2's table).
fn opener(e: &Embedding) -> char {
    match (e.kind, e.rtl) {
        (EmbeddingKind::Embed, false) => '\u{202A}',
        (EmbeddingKind::Embed, true) => '\u{202B}',
        (EmbeddingKind::Isolate, false) => '\u{2066}',
        (EmbeddingKind::Isolate, true) => '\u{2067}',
        (EmbeddingKind::FirstStrong, _) => '\u{2068}',
    }
}

/// The formatting character that closes one.
fn closer(e: &Embedding) -> char {
    match e.kind {
        EmbeddingKind::Embed => '\u{202C}',
        EmbeddingKind::Isolate | EmbeddingKind::FirstStrong => '\u{2069}',
    }
}

/// Whether `c` is a character whose own class reads right to left, or opens
/// a right-to-left embedding, override or isolate.
fn opens_right_to_left(c: char) -> bool {
    matches!(
        bidi_class(c),
        BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
    )
}

/// Whether a line holding `c` is one whose right-to-left levels ruling 14's
/// reading undoes, so that a run of neutrals at such a level is drawn in that
/// level's order ([`TextRun::bidi_line_rtl`]): a character whose own class
/// reads right to left, `R` or `AL`, or a right-to-left isolate's `RLI`,
/// which stays on the line as a unit of its own and is read back with what it
/// encloses.
///
/// **Not `RLE` or `RLO`.** Ruling 14 reads a line holding one too, but rule
/// X9 removes both before anything is resolved, so its reading never sees
/// the embedding they opened, and a run of neutrals drawn in that
/// embedding's order reads back reversed: `x RLE ?! PDF y`, its `?!` drawn
/// `!?` because its line held an `RLE`, extracted `x RLE PDF !? y` (review of
/// the K-11 fix). Drawn as typed, as it was before, it reads as typed.
fn undoes_its_levels(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL | BidiClass::RLI)
}

/// Which pieces of a line that holds a right-to-left character — each run's
/// [`level_pieces`], `pieces`, one list per run of `line` — are runs of
/// nothing but neutrals at a right-to-left level drawn as typed all the same
/// ([`TextRun::bidi_line_rtl`]), one list per run. `levels` are the line's
/// characters'.
///
/// Such a piece is drawn in its level's order because a reader of the page
/// undoes that order: ruling 14 reads a line holding a right-to-left
/// character in the order UAX #9 draws it. Where the reader would not undo
/// it, a piece so drawn reads back reversed, and drawn as typed, as every
/// such piece was before ROADMAP K-11, it reads back as typed. Two kinds,
/// each measured (reviews of the K-11 fix), and a third whose order a
/// reader does not read:
///
/// - **A piece read alone for its spacing.** `TextDevice` resumes a line
///   across a text object's end only where the next glyph starts within half
///   an em of where the last one's box ended, and the join of the pieces it
///   cut only within the same half em; a line is written a run at a time in
///   reading order, each run a text object of its own, and spacing carries
///   the pen only into a right-to-left letter, never into or out of a run of
///   neutrals alone. So a piece is read on its own, in the order the content
///   stream drew it, where it is half an em or more from what it is drawn
///   beside on both sides — or on one side, the other being the line's end —
///   counting the spacing after each glyph from the piece to the nearest one
///   that is not white space: its run's `letter-spacing`, and after a word
///   space its `word-spacing`, which justification stretches. A run
///   letter-spaced by half an em is read run by run, whatever stands beside
///   it: at `letter-spacing: 0.5em`, `שָׁלוֹם עוֹלָם (HDAC) (SIRT).` drew its
///   `) (` `( )` and read it so (`bundled-fonts`), and in monospace at `8px`
///   `שָׁלוֹם <i>?!</i> עוֹלָם` read its `?!` on a line with the word after
///   it, by the comma tie-break, `עוֹלָם !?`. And a run whose word spaces
///   stretch apart from the words either side of it is read alone too:
///   `عوامل <i>?!</i> عدّة` at `letter-spacing: 0.3em`, justified, and at
///   `word-spacing: 1em`, drawn `!?`, read back `!?` (third review of the
///   K-11 fix, 15 books in a default build and 18 with `bundled-fonts`).
/// - **A piece its line alone resolves otherwise.** Ruling 14 reads a line by
///   itself, in the direction [`drawn_direction`] reads it in as it is drawn,
///   and UAX #9 resolves a paragraph: where the levels the line alone gives a
///   piece are not the ones its paragraph gave it, no reader of the page can
///   undo the paragraph's. `عوامل عدّة ($1,234.56). وقد` broken before the
///   bracket: W2 makes the digits `AN` for the Arabic letter before them on
///   the line above, so the `$` is a neutral at the paragraph's level, drawn
///   right of its number, `1,234.56$(`; resolved alone, the line has no
///   Arabic letter before the digits, the `$` is a European number's (W5),
///   and the line read back `(1,234.56$)`. And `<p dir="rtl"><i>?!</i>
///   שלום</p>`, which the comma tie-break reads left to right: resolved with
///   the line alone as a left-to-right paragraph's, its `?!` is at level 0,
///   not 1, and drawn `!?` in its paragraph's level order it read back
///   `שלום !?` where as typed it reads `שלום ?!` — the price the comma
///   tie-break costs the line in one run, which ruling 14 accepted, and no
///   more. The direction is the one
///   the reader reads the line in, not its paragraph's: resolved in a
///   paragraph's with no `dir` — left to right, for a line with Arabic at
///   both ends that a reader reads from the right — the `). ` after a
///   bracketed year is a piece of other levels too, drawn as typed, and
///   `… عدّة (2010). وقد …` read its full stop away from its bracket — CI's
///   K-11 failure itself, `Missing ").وقد…"` (73 probe books, 75 with
///   `bundled-fonts`). So too a piece that holds a letter, where any of its
///   other characters the line alone resolves otherwise: a face that draws
///   its neutrals and not its letters draws them as a slice of neutrals of
///   their own. `<p dir="rtl">جميل بيت كبير نور 5% يوم</p>` in sans-serif,
///   broken before the `5`: W2 makes it `AN` for the Arabic letter on the
///   line above, so the `%` is a neutral at the paragraph's level, in a
///   piece with `يوم`, and Helvetica's `% ` drawn in its level's order read
///   back `%5`, where alone the `5` is a European number and W5 makes the
///   `%` one too; as typed it reads as at 9297d2d (review of the third review's
///   fix, 85 books in a default build and 86 with `bundled-fonts`, `%`, `‰`,
///   `°`, `¢` and `£` after a number in Arabic and Persian).
/// - **A piece that ends, on its right, what a reader reads together, where
///   that reads left to right** ([`Gaps::left_to_right_ends`]): a reader
///   reads the units between two gaps of half an em by themselves, and read
///   left to right the neutrals that end them stand at level 0, in the order
///   drawn — the comma tie-break's shape where they follow nothing but
///   right-to-left letters.
/// - **A slice of a word-spaced run whose glyphs read alike either way
///   round**, ` (` or ` #`, which [`coded_order`] draws as typed itself, its
///   word space first ([`opens_with_its_word_space`]): what a reader reads is
///   the same either way, and the space is stretched inside the slice's own
///   text object. (It also stood where the overflow font's string before it
///   overran, until that font was written at `0 Tw`, ROADMAP CD-19.)
fn neutrals_drawn_as_typed(
    line: &[TextRun],
    pieces: &[Vec<(core::ops::Range<usize>, Level)>],
    levels: &[Level],
) -> Vec<Vec<bool>> {
    let mut typed: Vec<Vec<bool>> = pieces.iter().map(|own| vec![false; own.len()]).collect();
    let strong = |c: char| matches!(bidi_class(c), BidiClass::L | BidiClass::R | BidiClass::AL);
    // Every piece of nothing but neutrals at a right-to-left level: its run,
    // its place among the run's pieces, and where its characters are in
    // `levels`.
    let mut neutral: Vec<(usize, usize, core::ops::Range<usize>)> = Vec::new();
    // And every piece at a right-to-left level that holds a strong character
    // too, with where its other characters are in `levels`: a face that
    // draws its neutrals and not its letters draws them as a slice of
    // neutrals of their own ([`coded_order`]).
    let mut mixed: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    let mut first = 0usize;
    for (at, (run, own)) in line.iter().zip(pieces).enumerate() {
        // The pieces cover the run's text in order, so each starts where the
        // one before it ended.
        let mut start = first;
        for (piece, (range, level)) in own.iter().enumerate() {
            let text = run.text.get(range.clone()).unwrap_or("");
            let count = text.chars().count();
            if !run.generated && level.is_rtl() && count > 0 {
                if text.chars().any(strong) {
                    let others = text
                        .chars()
                        .enumerate()
                        .filter(|(_, c)| !strong(*c))
                        .map(|(offset, _)| start + offset)
                        .collect::<Vec<_>>();
                    if !others.is_empty() {
                        mixed.push((at, piece, others));
                    }
                } else {
                    neutral.push((at, piece, start..start + count));
                }
            }
            start += count;
        }
        first += run.text.chars().count();
    }
    if neutral.is_empty() && mixed.is_empty() {
        return typed;
    }
    let mark = |typed: &mut Vec<Vec<bool>>, at: usize, piece: usize| {
        if let Some(slot) = typed.get_mut(at).and_then(|own| own.get_mut(piece)) {
            *slot = true;
        }
    };
    // Where each piece stands in the order the line is drawn, every piece in
    // its level's order: its first and last place.
    let units = line_units(line, pieces, levels);
    let order = reorder(&units.iter().map(|unit| unit.level).collect::<Vec<_>>());
    let mut stands: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
    for (place, unit) in order.iter().enumerate() {
        let Some(unit) = units.get(*unit) else {
            continue;
        };
        let (lo, hi) = stands
            .get(&(unit.run, unit.piece))
            .copied()
            .unwrap_or((place, place));
        stands.insert((unit.run, unit.piece), (lo.min(place), hi.max(place)));
    }
    let drawn: Vec<&LineUnit> = order.iter().filter_map(|unit| units.get(*unit)).collect();
    let gaps = Gaps::of(line, &drawn);
    // Asked only where a piece of neutrals alone may end one.
    let ends = if neutral.is_empty() {
        Vec::new()
    } else {
        gaps.left_to_right_ends(line, &drawn)
    };
    for (at, piece, _) in &neutral {
        let (Some(run), Some(&(lo, hi))) = (line.get(*at), stands.get(&(*at, *piece))) else {
            continue;
        };
        let ends_one = ends
            .get(lo..=hi)
            .is_some_and(|places| places.iter().any(|end| *end));
        if gaps.read_alone(run, lo, hi) || ends_one {
            mark(&mut typed, *at, *piece);
        }
    }
    // The levels the line alone gives, resolved in the direction a reader
    // reads it in as it is drawn ([`drawn_direction`]).
    let drawn = drawn_units(line, pieces, levels, &typed);
    let drawn: Vec<&str> = drawn.iter().map(String::as_str).collect();
    let alone = levels_alone(line, drawn_direction(&drawn));
    for (at, piece, chars) in &neutral {
        if levels.get(chars.clone()) != alone.levels.get(chars.clone()) {
            mark(&mut typed, *at, *piece);
        }
    }
    for (at, piece, others) in &mixed {
        if others
            .iter()
            .any(|place| levels.get(*place) != alone.levels.get(*place))
        {
            mark(&mut typed, *at, *piece);
        }
    }
    typed
}

/// The widest gap a reader sees between each place of a line, drawn in its
/// level's order, and the nearest unit on each side of it that draws
/// something, through the white space between ([`Gaps::read_alone`]) —
/// found for every place in two passes, so that asking it of every piece of
/// a line costs the line once.
struct Gaps {
    /// For each place, the widest spacing after a unit from the nearest unit
    /// left of it that draws something up to the place, or `None` where the
    /// line holds nothing but white space left of it.
    left: Vec<Option<f64>>,
    /// For each place, the widest spacing after a unit of white space from
    /// the place up to the nearest unit that draws something, that unit
    /// included as `0.0`, or `None` where the line ends first.
    right: Vec<Option<f64>>,
    /// The spacing after the unit at each place.
    after: Vec<f64>,
    /// Whether the unit at each place is white space, drawing nothing.
    blank: Vec<bool>,
}

impl Gaps {
    /// The gaps of `drawn`, the units of `line` in the order they are drawn.
    fn of(line: &[TextRun], drawn: &[&LineUnit]) -> Gaps {
        // The spacing 9.4.4 moves the pen on by after a unit: `Tc` but after
        // a mark, and `Tw` after a word space.
        let after: Vec<f64> = drawn
            .iter()
            .map(|unit| {
                let Some(own) = line.get(unit.run) else {
                    return 0.0;
                };
                let first = unit.text.chars().next();
                let letter = if first.is_some_and(letter_spaced) {
                    own.letter_spacing
                } else {
                    0.0
                };
                letter
                    + if first == Some(' ') {
                        own.word_spacing
                    } else {
                        0.0
                    }
            })
            .collect();
        let blank: Vec<bool> = drawn
            .iter()
            .map(|unit| unit.text.chars().all(char::is_whitespace))
            .collect();
        // Left to right: what stands left of each place is what stood left of
        // the place before, widened by the spacing after the unit there if
        // it is white space, or that spacing alone if it draws something.
        let mut left: Vec<Option<f64>> = Vec::with_capacity(after.len());
        let mut carried: Option<f64> = None;
        for (gap, blank) in after.iter().zip(&blank) {
            left.push(carried);
            carried = if *blank {
                carried.map(|wider| wider.max(*gap))
            } else {
                Some(*gap)
            };
        }
        // And right to left for what stands right of each place.
        let mut right: Vec<Option<f64>> = Vec::with_capacity(after.len());
        let mut carried: Option<f64> = None;
        for (gap, blank) in after.iter().zip(&blank).rev() {
            carried = if *blank {
                carried.map(|wider| wider.max(*gap))
            } else {
                Some(0.0)
            };
            right.push(carried);
        }
        right.reverse();
        Gaps {
            left,
            right,
            after,
            blank,
        }
    }

    /// For each place of `drawn`, the units of `line` in the order they are
    /// drawn, whether the unit there is the last that draws something of a
    /// stretch a reader reads together and reads left to right
    /// ([`drawn_direction`]): a stretch is the units between two that draw
    /// something half an em or more apart, counting the spacing after each
    /// unit from the one to the other as [`Gaps::read_alone`] does —
    /// `TextDevice` joins nothing further apart, nor the pieces it cut.
    ///
    /// A piece of neutrals that ends such a stretch on its right is read in
    /// the order drawn: read left to right, the last neutrals of a line stand
    /// between what is before them and its end, a left-to-right paragraph's,
    /// at level 0 (N1, N2). `كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.`,
    /// justified at `letter-spacing: 0.25em`, has the bold `?!` drawn against
    /// the word after it and a stretched word space before it, so a reader
    /// reads `الطالب` and the `?!` together, and by the comma tie-break left
    /// to right: drawn `!?`, in its level's order, it read `!?` (review of the
    /// third review's fix, 3 books in a default build and 4 with
    /// `bundled-fonts` that conserved at 9297d2d). Linear in the line: each
    /// stretch is read once.
    fn left_to_right_ends(&self, line: &[TextRun], drawn: &[&LineUnit]) -> Vec<bool> {
        let mut ends = vec![false; drawn.len()];
        let mut close = |from: usize, to: usize, last: Option<usize>| {
            let Some(last) = last else {
                return;
            };
            let texts: Vec<&str> = drawn
                .get(from..to)
                .unwrap_or_default()
                .iter()
                .map(|unit| unit.text.as_str())
                .collect();
            if drawn_direction(&texts) == BaseDirection::LeftToRight {
                if let Some(end) = ends.get_mut(last) {
                    *end = true;
                }
            }
        };
        let mut from = 0usize;
        let mut last: Option<usize> = None;
        for (place, unit) in drawn.iter().enumerate() {
            if self.blank.get(place).copied().unwrap_or(true) {
                continue;
            }
            let half = line.get(unit.run).map_or(0.0, |run| run.font_size * 0.5);
            let parted = self
                .left
                .get(place)
                .copied()
                .flatten()
                .is_some_and(|gap| gap >= half);
            if parted {
                close(from, place, last);
                from = place;
            }
            last = Some(place);
        }
        close(from, drawn.len(), last);
        ends
    }

    /// Whether the piece of `run` drawn from place `lo` to place `hi` of its
    /// line, in its level's order, is read on its own for its spacing
    /// ([`neutrals_drawn_as_typed`]): a run letter-spaced by half an em or
    /// more, or one half an em or more from what is drawn beside it on each
    /// side — or on one side, the line ending on the other — counting the
    /// spacing after each glyph from the piece to the nearest one that is not
    /// white space.
    fn read_alone(&self, run: &TextRun, lo: usize, hi: usize) -> bool {
        let half = run.font_size * 0.5;
        if run.letter_spacing >= half {
            return true;
        }
        let left = self.left.get(lo).copied().flatten();
        let own = self.after.get(hi).copied().unwrap_or(0.0);
        let right = self
            .right
            .get(hi + 1)
            .copied()
            .flatten()
            .map(|wider| wider.max(own));
        let apart = |side: Option<f64>| side.is_none_or(|gap| gap >= half);
        (left.is_some() || right.is_some()) && apart(left) && apart(right)
    }
}

/// Whether `runs`, the runs of one line, hold the text of more than one
/// paragraph ([`TextRun::paragraph`]): table cells set touching on one
/// baseline, `border-spacing: 0` and no padding, which a reader of the page
/// reads as one line. Generated runs and runs nobody paints are no one's.
fn holds_two_paragraphs<'r>(runs: impl Iterator<Item = &'r TextRun>) -> bool {
    let mut first: Option<usize> = None;
    for run in runs.filter(|run| run.painted && !run.generated) {
        match first {
            None => first = Some(run.paragraph),
            Some(paragraph) if paragraph != run.paragraph => return true,
            Some(_) => {}
        }
    }
    false
}

/// A unit of a line as a reader of the page finds it ([`line_units`]): a
/// character and the nonspacing marks after it, its level, and the run and
/// the piece of that run's [`level_pieces`] it is in.
struct LineUnit {
    text: String,
    level: Level,
    run: usize,
    piece: usize,
}

/// The [`LineUnit`]s of `line`, its runs cut into `pieces` and its
/// characters at `levels`, in the order written — less a generated run's,
/// which no reader reads.
fn line_units(
    line: &[TextRun],
    pieces: &[Vec<(core::ops::Range<usize>, Level)>],
    levels: &[Level],
) -> Vec<LineUnit> {
    let mut units: Vec<LineUnit> = Vec::new();
    let mut place = 0usize;
    for (at, (run, own)) in line.iter().zip(pieces).enumerate() {
        // The piece the character is in, walked with the characters.
        let mut piece = 0usize;
        for (offset, c) in run.text.char_indices() {
            let level = levels.get(place).copied();
            place += 1;
            let Some(level) = level.filter(|_| !run.generated) else {
                continue;
            };
            while own.get(piece).is_some_and(|(range, _)| offset >= range.end) {
                piece += 1;
            }
            if bidi_class(c) == BidiClass::NSM {
                if let Some(unit) = units
                    .last_mut()
                    .filter(|unit| (unit.run, unit.piece) == (at, piece))
                {
                    unit.text.push(c);
                    continue;
                }
            }
            units.push(LineUnit {
                text: c.to_string(),
                level,
                run: at,
                piece,
            });
        }
    }
    units
}

/// The units a reader of the page finds on `line`, its runs cut into
/// `pieces` and its characters at `levels` ([`line_units`]), in the order
/// UAX #9's L2 draws them — but for the pieces `typed` marks, each drawn as
/// typed where it stands.
fn drawn_units(
    line: &[TextRun],
    pieces: &[Vec<(core::ops::Range<usize>, Level)>],
    levels: &[Level],
    typed: &[Vec<bool>],
) -> Vec<String> {
    let units = line_units(line, pieces, levels);
    let as_typed = |unit: &LineUnit| {
        typed
            .get(unit.run)
            .and_then(|own| own.get(unit.piece))
            .copied()
            .unwrap_or(false)
    };
    let mut order = reorder(&units.iter().map(|unit| unit.level).collect::<Vec<_>>());
    // Where each typed piece's units stand in the drawn order: a piece is at
    // one level, so it stands whole, and is put back as typed there.
    let mut stands: BTreeMap<(usize, usize), (usize, usize, usize)> = BTreeMap::new();
    for (drawn_at, at) in order.iter().enumerate() {
        let Some(unit) = units.get(*at).filter(|unit| as_typed(unit)) else {
            continue;
        };
        let (lo, hi, count) = stands
            .get(&(unit.run, unit.piece))
            .copied()
            .unwrap_or((drawn_at, drawn_at, 0));
        stands.insert(
            (unit.run, unit.piece),
            (lo.min(drawn_at), hi.max(drawn_at), count + 1),
        );
    }
    for (lo, hi, count) in stands.into_values() {
        if hi + 1 - lo == count {
            if let Some(whole) = order.get_mut(lo..=hi) {
                whole.sort_unstable();
            }
        }
    }
    order
        .iter()
        .filter_map(|at| units.get(*at).map(|unit| unit.text.clone()))
        .collect()
}

/// [`split_at_levels`] over one line, draining `line` into `out`. `places`
/// are the line's runs' places in their paragraph, one per run; `ends`, where
/// given, takes the length of `out` after each run's pieces.
fn split_line(
    line: &mut Vec<TextRun>,
    places: &[Place],
    paragraphs: &BTreeMap<usize, Paragraph>,
    metrics: &BookMetrics<'_>,
    out: &mut Vec<TextRun>,
    mut ends: Option<&mut Vec<usize>>,
) -> usize {
    let Some(LineLevels { levels, base, gaps }) = line_levels(line, places, paragraphs) else {
        for run in line.drain(..) {
            out.push(run);
            if let Some(ends) = ends.as_deref_mut() {
                ends.push(out.len());
            }
        }
        return 0;
    };
    // Whether a reader of the page finds a right-to-left character on this
    // line, which is what decides whether it reads a run of neutrals at a
    // right-to-left level in that level's order or in the order it was drawn
    // ([`coded_order`]). A generated run is an artifact, which it does not
    // read: it neither makes a line right to left nor is reordered on one,
    // so a right-to-left `inside` marker's `1. ` beside an Arabic word is
    // drawn as written, as it was. See [`undoes_its_levels`] for which
    // characters say so.
    let rtl_line = line
        .iter()
        .any(|run| !run.generated && run.text.chars().any(undoes_its_levels));
    // Each run's pieces, one per level; none for a generated run.
    let mut first_char = 0usize;
    let pieces: Vec<Vec<(core::ops::Range<usize>, Level)>> = line
        .iter()
        .map(|run| {
            let count = run.text.chars().count();
            let own = levels.get(first_char..first_char + count).unwrap_or(&[]);
            first_char += count;
            if run.generated {
                Vec::new()
            } else {
                level_pieces(&run.text, own)
            }
        })
        .collect();
    let typed = if rtl_line {
        neutrals_drawn_as_typed(line, &pieces, &levels)
    } else {
        Vec::new()
    };
    let reordered = |at: usize, piece: usize| {
        rtl_line
            && !typed
                .get(at)
                .and_then(|own| own.get(piece))
                .copied()
                .unwrap_or(false)
    };
    for (at, run) in line.iter_mut().enumerate() {
        run.bidi_line_rtl = !run.generated && reordered(at, 0);
    }
    let mut cut = 0usize;
    let mut placed: Vec<Vec<TextRun>> = Vec::with_capacity(line.len());
    let mut whole: Vec<Option<u8>> = Vec::with_capacity(line.len());
    for (at, (run, pieces)) in line.iter().zip(&pieces).enumerate() {
        if run.generated {
            // An `inside` marker — the only generated run a line holds (see
            // [`beside_the_line`]) — is one isolate at its paragraph's
            // start, which UAX #9 puts at the paragraph's level whatever
            // follows it; it is ordered whole at that level and drawn in
            // that level's direction. See [`visual_lines`].
            whole.push(Some(base.number()));
            placed.push(Vec::new());
            continue;
        }
        if pieces.len() < 2 {
            whole.push(pieces.first().map(|(_, level)| level.number()));
            placed.push(Vec::new());
            continue;
        }
        cut += 1;
        whole.push(None);
        let mut cut_pieces = cut_run(run, pieces, metrics, &layout_context(line, at));
        for (piece, cut_piece) in cut_pieces.iter_mut().enumerate() {
            cut_piece.bidi_line_rtl = reordered(at, piece);
        }
        placed.push(cut_pieces);
    }
    // The gap before a run stands before its first piece; the pieces of one
    // run have nothing between them.
    let gaps = gaps.into_iter().chain(core::iter::repeat(None));
    for (((mut run, pieces), level), gap) in line.drain(..).zip(placed).zip(whole).zip(gaps) {
        let gap = gap.map(Level::number);
        if pieces.is_empty() {
            run.bidi_level = level;
            run.bidi_gap = gap;
            out.push(run);
        } else {
            let mut pieces = pieces.into_iter();
            if let Some(mut first) = pieces.next() {
                first.bidi_gap = gap;
                out.push(first);
            }
            out.extend(pieces);
        }
        if let Some(ends) = ends.as_deref_mut() {
            ends.push(out.len());
        }
    }
    cut
}

/// The context `tinker-pdf-layout` measured run `at` of a line in: its
/// painted neighbours either side on the line, in logical order, which is
/// the order a line's runs are in before [`visual_lines`] moves them. The
/// provider decides which of them share its face.
fn layout_context(line: &[TextRun], at: usize) -> ShapingContext<'_> {
    if !line.get(at).is_some_and(|run| run.painted) {
        return ShapingContext::NONE;
    }
    ShapingContext {
        before: as_neighbour(at.checked_sub(1).and_then(|p| line.get(p))),
        after: as_neighbour(line.get(at + 1)),
    }
}

/// A run as a shaping neighbour, where it can be one: painted, from the
/// source, and holding text.
fn as_neighbour(other: Option<&TextRun>) -> Option<Neighbour<'_>> {
    other
        .filter(|other| other.painted && !other.generated && !other.text.is_empty())
        .map(|other| Neighbour {
            text: other.text.as_str(),
            font: request(other),
        })
}

/// What [`line_levels`] finds for one line.
struct LineLevels {
    /// UAX #9's level for every character of the line's runs, in order.
    levels: Vec<Level>,
    /// The paragraph's base level.
    base: Level,
    /// One per run: the lowest level of the isolate formatting characters
    /// between it and the run before it ([`TextRun::bidi_gap`]).
    gaps: Vec<Option<Level>>,
}

/// UAX #9's level for every character of a line's runs, in order, the
/// paragraph's base level and the gaps between the runs — or `None` for a
/// line UAX #9 would leave as it is.
///
/// # What the line is resolved as
///
/// **The paragraph's direction is the block's** (`css-writing-modes-3` §2.1):
/// each run carries its block container's `direction`, and `unicode-bidi:
/// plaintext` on the container leaves it to P2 and P3. Before `direction` was
/// read every line was resolved by P2 and P3, so a left-to-right paragraph
/// whose line began with an Arabic word was laid out right to left.
///
/// **And the inline boxes' embeddings are the formatting characters §2.4.2
/// says they are**, written into the text resolved here and nowhere else: a
/// run's [`TextRun::embeddings`] are opened before it and closed after, the
/// ones a run shares with the run before it left open across the boundary,
/// and each is told apart from a sibling's by the box that opened it. They
/// are in no run, so the runs' own characters are the only levels returned
/// per character. X9 removes an embedding's characters; an isolate's it
/// keeps, and L2 reverses them with the rest, so the lowest level of the
/// ones between two runs is returned as the gap between them
/// ([`TextRun::bidi_gap`]): without it an isolate and the text beside it
/// were reversed together wherever no lower character stood between them
/// (review of lane 8C). A character X9 removes in a run's own text (a
/// joiner, a format control) has no level of its own either; it takes the
/// level of the character before it, since after L1 it carries the
/// paragraph's, which would cut its word in three.
///
/// A line with no right-to-left character, in a left-to-right paragraph,
/// with no right-to-left embedding, is `None`: no left-to-right page moves.
///
/// # And whose levels they are
///
/// Where every run of the line has a place in one resolved paragraph
/// ([`paragraphs`]), the levels are that paragraph's, through
/// [`Paragraph::line`] over the stretch of it the line holds — X1 to I2 over
/// the paragraph, L1 at this line's end. Otherwise, and for a caller with no
/// paragraphs (`places` empty), the line is resolved as a paragraph of its
/// own, as below.
fn line_levels(
    line: &[TextRun],
    places: &[Place],
    paragraphs: &BTreeMap<usize, Paragraph>,
) -> Option<LineLevels> {
    let base = line
        .iter()
        .find(|run| !run.generated)
        .map_or(Some(false), |run| run.paragraph_rtl);
    let rtl_embedding = line.iter().any(|run| {
        run.embeddings
            .iter()
            .any(|e| e.rtl && e.kind != EmbeddingKind::FirstStrong)
    });
    let rtl_text = line
        .iter()
        .any(|run| !run.generated && run.text.chars().any(opens_right_to_left));
    if !rtl_text && !rtl_embedding && base != Some(true) {
        return None;
    }
    if let Some(levels) = paragraph_levels(line, places, paragraphs) {
        return Some(levels);
    }
    let direction = match base {
        Some(true) => BaseDirection::RightToLeft,
        Some(false) => BaseDirection::LeftToRight,
        None => BaseDirection::Auto,
    };
    Some(levels_alone(line, direction))
}

/// [`line_levels`] for `line` resolved as a paragraph of its own, in
/// `direction`: what a reader of the page, which reads a line by itself
/// ([`neutrals_drawn_as_typed`]), resolves it as.
fn levels_alone(line: &[TextRun], direction: BaseDirection) -> LineLevels {
    let mut text = String::new();
    let mut count = 0usize;
    // Where each character of the line's runs is in `text`, or `None` for an
    // `inside` marker's, which are not in it: see below.
    let mut own: Vec<Option<usize>> = Vec::new();
    // Each run's first character in `text` and how many it has, for the
    // gaps between runs.
    let mut spans: Vec<(Option<usize>, usize)> = Vec::with_capacity(line.len());
    let mut open: Vec<Embedding> = Vec::new();
    for run in line {
        let chars = run.text.chars().count();
        if run.generated {
            // An `inside` marker is an isolate (`css-lists-3` §3.1) at its
            // paragraph's start, which X5a to X6a and N1 make one neutral at
            // the paragraph's level; its own characters decide nothing out
            // here. [`split_line`] gives it that level.
            own.extend(core::iter::repeat_n(None, chars));
            spans.push((None, chars));
            continue;
        }
        let shared = open
            .iter()
            .zip(run.embeddings.iter())
            .take_while(|(a, b)| a == b)
            .count();
        while open.len() > shared {
            if let Some(e) = open.pop() {
                text.push(closer(&e));
                count += 1;
            }
        }
        for e in run.embeddings.iter().skip(shared) {
            text.push(opener(e));
            count += 1;
            open.push(*e);
        }
        spans.push((Some(count), chars));
        for c in run.text.chars() {
            own.push(Some(count));
            text.push(c);
            count += 1;
        }
    }
    while let Some(e) = open.pop() {
        text.push(closer(&e));
    }
    let paragraph = Paragraph::new(&text, direction);
    let resolved = paragraph.line(0..paragraph.len());
    let all = resolved.levels();
    let mut levels: Vec<Level> = Vec::with_capacity(own.len());
    for at in own {
        let Some(at) = at else {
            levels.push(paragraph.base_level());
            continue;
        };
        let level = all.get(at).copied().unwrap_or(paragraph.base_level());
        let kept = if paragraph.is_removed(at) {
            levels.last().copied().unwrap_or(level)
        } else {
            level
        };
        levels.push(kept);
    }
    let gaps = gaps_between(&spans, |at| {
        all.get(at).copied().filter(|_| !paragraph.is_removed(at))
    });
    LineLevels {
        levels,
        base: paragraph.base_level(),
        gaps,
    }
}

/// [`line_levels`] from the line's paragraph, resolved whole: `None` unless
/// every run of the line has a place in one paragraph [`paragraphs`]
/// resolved — every run but an `inside` marker, which is in no paragraph's
/// text and takes the paragraph's level, as [`line_levels`] says.
///
/// The line is the stretch of the paragraph from its first run's first
/// character to its last run's last; the formatting characters between two
/// runs are in that stretch and in no run, and a character X9 removes inside
/// a run takes the level of the character before it, as below.
fn paragraph_levels(
    line: &[TextRun],
    places: &[Place],
    paragraphs: &BTreeMap<usize, Paragraph>,
) -> Option<LineLevels> {
    if places.len() != line.len() {
        return None;
    }
    let mut number = None;
    for (run, place) in line.iter().zip(places) {
        let Some((id, _)) = *place else {
            if run.generated {
                continue;
            }
            return None;
        };
        if number.is_some_and(|n| n != id) {
            return None;
        }
        number = Some(id);
    }
    let paragraph = paragraphs.get(&number?)?;
    // Each run's stretch of the paragraph, `None` for a marker's.
    let spans: Vec<(Option<usize>, usize)> = line
        .iter()
        .zip(places)
        .map(|(run, place)| (place.map(|(_, start)| start), run.text.chars().count()))
        .collect();
    let placed = || {
        spans
            .iter()
            .filter_map(|(start, count)| start.map(|s| (s, s + count)))
            .filter(|(s, e)| s < e)
    };
    let first = placed().map(|(s, _)| s).min()?;
    let last = placed().map(|(_, e)| e).max()?;
    let resolved = paragraph.line(first..last);
    let base = paragraph.base_level();
    let level_at = |at: usize| {
        at.checked_sub(first)
            .and_then(|offset| resolved.levels().get(offset))
            .copied()
    };
    let mut levels: Vec<Level> = Vec::with_capacity(last - first);
    for &(start, count) in &spans {
        let Some(start) = start else {
            levels.extend(core::iter::repeat_n(base, count));
            continue;
        };
        for at in start..start + count {
            let level = level_at(at).unwrap_or(base);
            let kept = if paragraph.is_removed(at) {
                levels.last().copied().unwrap_or(level)
            } else {
                level
            };
            levels.push(kept);
        }
    }
    let gaps = gaps_between(&spans, |at| {
        level_at(at).filter(|_| !paragraph.is_removed(at))
    });
    Some(LineLevels { levels, base, gaps })
}

/// [`TextRun::bidi_gap`] for each run of a line, from `spans` — each run's
/// first character's index in the text resolved and how many it has, `None`
/// for a run that is not in it — and `kept`, the level of the character at
/// an index, `None` where X9 removed it.
///
/// What lies between a run and the last run before it with any characters
/// is the formatting characters written between them; X9 keeps an
/// isolate's, and the lowest of their levels is the gap. Linear: each
/// character of the text is looked at once at most.
fn gaps_between(
    spans: &[(Option<usize>, usize)],
    kept: impl Fn(usize) -> Option<Level>,
) -> Vec<Option<Level>> {
    let mut gaps = Vec::with_capacity(spans.len());
    let mut end: Option<usize> = None;
    for &(start, count) in spans {
        let gap = match (start, end) {
            (Some(start), Some(end)) if count > 0 => (end..start).filter_map(&kept).min(),
            _ => None,
        };
        gaps.push(gap);
        if let Some(start) = start.filter(|_| count > 0) {
            end = Some(start + count);
        }
    }
    gaps
}

/// The byte ranges of `text` over which `levels` — one per character — is
/// constant, in logical order, each with its level.
fn level_pieces(text: &str, levels: &[Level]) -> Vec<(core::ops::Range<usize>, Level)> {
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut current: Option<Level> = None;
    for ((at, _), level) in text.char_indices().zip(levels.iter().copied()) {
        if let Some(c) = current.filter(|c| *c != level) {
            pieces.push((start..at, c));
            start = at;
        }
        current = Some(level);
    }
    if let Some(c) = current.filter(|_| start < text.len()) {
        pieces.push((start..text.len(), c));
    }
    pieces
}

/// One run as one run per piece, each carrying its part of the run's
/// measured width — shaped in the `context` layout measured it in. See
/// [`split_at_levels`].
pub(crate) fn cut_run(
    run: &TextRun,
    pieces: &[(core::ops::Range<usize>, Level)],
    metrics: &BookMetrics<'_>,
    context: &ShapingContext<'_>,
) -> Vec<TextRun> {
    let shaped = metrics.shape_in(&run.text, &request(run), false, context);
    let mut widths = vec![0.0f64; pieces.len()];
    for glyph in &shaped.glyphs {
        let at = usize::try_from(glyph.cluster).unwrap_or(usize::MAX);
        // A search and not a scan: the pieces are [`level_pieces`]', in
        // logical order, each starting where the one before it ended, so the
        // first one ending past `at` holds it. A scan from the first made a
        // run cut into a piece a word cost `O(glyphs x pieces)`, and one line
        // can be such a run (review of lane 8C). A cluster past the text
        // goes to the last piece, as it did.
        let piece = pieces
            .partition_point(|(range, _)| {
                step();
                range.end <= at
            })
            .min(pieces.len().saturating_sub(1));
        if let Some(width) = widths.get_mut(piece) {
            *width += glyph.x_advance;
        }
    }
    for (width, (range, _)) in widths.iter_mut().zip(pieces) {
        let slice = run.text.get(range.clone()).unwrap_or("");
        *width += run.letter_spacing * slice.chars().filter(|c| letter_spaced(*c)).count() as f64
            + run.word_spacing * slice.chars().filter(|c| *c == ' ').count() as f64;
    }
    let mut x = run.x;
    let mut out = Vec::with_capacity(pieces.len());
    for (at, ((range, level), width)) in pieces.iter().zip(&widths).enumerate() {
        let mut piece = run.clone();
        piece.text = run.text.get(range.clone()).unwrap_or("").to_owned();
        piece.bidi_level = Some(level.number());
        piece.x = x;
        // The last piece ends where the run did, whatever the rounding of the
        // partition: the run after it was placed against the run's width.
        piece.width = if at + 1 == pieces.len() {
            run.x + run.width - x
        } else {
            *width
        };
        x += piece.width;
        out.push(piece);
    }
    out
}

#[cfg(test)]
thread_local! {
    /// [`step`]'s count, on this thread.
    pub(crate) static STEPS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    /// The most comparisons [`set_objects`] made on any page, on this thread,
    /// against [`OBJECT_WORK`].
    pub(crate) static OBJECT_PEAK: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    /// How many times [`Fonts::read_back`] read a line back, on this thread:
    /// counted under `cfg(test)` only, so that `epub/tests.rs` holds the
    /// read-back's work by count where a clock would only run slowly.
    pub(crate) static READINGS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    /// How many runs [`draw_run_against`] wrote with no plan
    /// [`Fonts::set_readings`] made for them, on this thread: a copy drawn
    /// somewhere else, a run of a line nothing read back, or a run whose
    /// plan was not found where it was kept.
    pub(crate) static UNPLANNED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// One step of a lookup a crafted line could make quadratic: a piece
/// [`cut_run`] looks at to find a glyph's. Counted only under `cfg(test)`,
/// where `epub/tests.rs` holds the total to a search's, so that a scan put
/// back **fails** rather than runs slowly — which `cargo test`, having no
/// timeout, would not notice. Everywhere else it is nothing.
#[inline]
fn step() {
    #[cfg(test)]
    STEPS.with(|steps| steps.set(steps.get().saturating_add(1)));
}

/// Whether two layout coordinates are one, to the rounding of the sums that
/// placed them: `flow.rs` and [`visual_lines`] both place a run at the
/// previous one's `x` plus its width.
fn near(p: f64, q: f64) -> bool {
    (p - q).abs() <= 1e-6 * p.abs().max(q.abs()).max(1.0)
}

/// Whether `b` continues the line `a` is on. See [`visual_lines`].
fn same_line(a: &TextRun, b: &TextRun) -> bool {
    if beside_the_line(a) || beside_the_line(b) {
        return false;
    }
    let end = a.x + a.width;
    let tolerance = 1e-6 * end.abs().max(1.0);
    (end - b.x).abs() <= tolerance && (a.y - b.y).abs() <= a.font_size.max(b.font_size)
}

/// Whether `run` stands beside its line rather than in it: generated text set
/// in no paragraph, which is an `outside` list marker, hung beside its item's
/// first line box on the item's start side (`css-lists-3` §3.1) where layout
/// put it.
///
/// An `inside` marker is generated text too, but it is the first inline box
/// of its item's first line (CSS 2.2 §12.5.1), set in the item's paragraph
/// ([`TextRun::paragraph`]), so it is part of that line and of its order. It
/// was kept out of the line with the outside one, and so stayed at the left
/// of a right-to-left line, its item's text moved past it to the right
/// (review of lane 8C).
fn beside_the_line(run: &TextRun) -> bool {
    run.generated && run.paragraph == 0
}

/// Lays one line's runs out again in L2's order. Returns whether any moved.
///
/// A line [`split_at_levels`] cut is ordered by the level each of its runs
/// was cut at — its paragraph's — and an empty run among them, which has no
/// level and no width, by the line's lowest; with each run's
/// [`TextRun::bidi_gap`] between it and the run before it. A line nobody
/// resolved is resolved here, by itself.
fn reorder_line(line: &mut [TextRun]) -> bool {
    let mut run_levels: Vec<Level> = Vec::with_capacity(line.len());
    let gaps: Vec<Option<Level>> = if line.iter().any(|run| run.bidi_level.is_some()) {
        let lowest = line
            .iter()
            .filter_map(|run| run.bidi_level)
            .min()
            .unwrap_or(0);
        for run in line.iter() {
            let number = run.bidi_level.unwrap_or(lowest);
            run_levels.push(Level::from_number(number).unwrap_or(Level::LTR));
        }
        line.iter()
            .map(|run| run.bidi_gap.and_then(Level::from_number))
            .collect()
    } else {
        let Some(found) = line_levels(line, &[], &BTreeMap::new()) else {
            return false;
        };
        let mut at = 0usize;
        for run in line.iter() {
            run_levels.push(level_of(&run.text, &found.levels, at, found.base));
            at += run.text.chars().count();
        }
        found.gaps
    };
    let order = order_with_gaps(&run_levels, &gaps);
    if order.iter().enumerate().all(|(i, k)| i == *k) {
        return false;
    }
    let mut cursor = line.first().map_or(0.0, |run| run.x);
    let mut xs: Vec<f64> = line.iter().map(|run| run.x).collect();
    for k in &order {
        if let (Some(run), Some(slot)) = (line.get(*k), xs.get_mut(*k)) {
            *slot = cursor;
            cursor += run.width;
        }
    }
    for (run, x) in line.iter_mut().zip(xs) {
        run.x = x;
    }
    true
}

/// [`reorder_line`] over the runs of `runs` at `line`, a [`level_lines`]
/// line: in place where they stand together, and over a copy of them where
/// another level's runs stand between.
fn reorder_at(runs: &mut [TextRun], line: &[usize]) -> bool {
    let (Some(&first), Some(&last)) = (line.first(), line.last()) else {
        return false;
    };
    if last.checked_sub(first) == Some(line.len() - 1) {
        return runs.get_mut(first..=last).is_some_and(reorder_line);
    }
    let mut copy: Vec<TextRun> = line
        .iter()
        .filter_map(|at| runs.get(*at).cloned())
        .collect();
    if copy.len() != line.len() || !reorder_line(&mut copy) {
        return false;
    }
    for (at, moved) in line.iter().zip(copy) {
        if let Some(run) = runs.get_mut(*at) {
            run.x = moved.x;
        }
    }
    true
}

/// L2's order of a line's runs, each one unit at its level in `levels`,
/// with the gap before each ([`TextRun::bidi_gap`], one per run) standing
/// between it and the run before it as a unit of its own that is drawn
/// nowhere.
///
/// Every character of a gap lies between the same two runs, so the gap's
/// lowest level is all L2 needs of it: at each level at or below that one
/// the gap is inside the stretch L2 reverses and joins its neighbours, and
/// at each level above it the gap ends the stretch on one side and starts
/// it on the other. A gap at the line's start would order nothing and is
/// not one.
fn order_with_gaps(levels: &[Level], gaps: &[Option<Level>]) -> Vec<usize> {
    let mut units: Vec<Level> = Vec::with_capacity(levels.len() * 2);
    let mut runs: Vec<Option<usize>> = Vec::with_capacity(levels.len() * 2);
    for (at, level) in levels.iter().enumerate() {
        if let Some(gap) = gaps.get(at).copied().flatten().filter(|_| at > 0) {
            units.push(gap);
            runs.push(None);
        }
        units.push(*level);
        runs.push(Some(at));
    }
    reorder(&units)
        .into_iter()
        .filter_map(|unit| runs.get(unit).copied().flatten())
        .collect()
}

/// One embedded face's stretch: shaped, ordered, positioned, and drawn as text
/// objects.
///
/// Returns where the pen ended, in layout pixels, and how many pieces the
/// writer refused.
///
/// A word boundary starts a new text object whenever `word-spacing` is in
/// force, for the reason the character path gives in as many words: 9.3.3
/// applies `Tw` to **byte code 32 in a single-byte encoding**, and a composite
/// font under `/Identity-H` has none, so the space has to be paid by moving
/// the origin. Splitting there costs nothing a joining script would notice —
/// a space is `Joining_Type` `U` and breaks a cursive connection anyway.
///
/// The pieces are drawn in the order UAX #9 lays them ([`piece_order`]), not
/// the order they were written: a right-to-left run with `word-spacing` —
/// every justified line but the last — drew its words left to right in
/// written order, the line reading backwards (review of lane 6C). Each piece
/// pays its space where the space is drawn ([`shaped_glyphs`]).
///
/// # Why `DocumentBuilder::glyph_run` and not `PageBuilder::glyphs`
///
/// `glyphs` shows one hex string at one origin and lets the font's advances
/// place everything after the first glyph, so a `GPOS` offset is
/// **inexpressible** through it: a mark is drawn where its advance puts it and
/// not where its anchor does. `glyph_run` takes the position of every glyph
/// and writes 9.4.3's `TJ` adjustments and `Ts` for them, which is the shape
/// this needs and the shape XPS has been writing since gap 30.
///
/// The borrow that used to make this impossible is gone: `begin_page` takes
/// `&self` and hands back an owned page, so the document and the page it is
/// drawing are two independent borrows.
#[expect(
    clippy::too_many_arguments,
    reason = "the pen state a segment needs; a struct here would be eight \
              fields written once and read once"
)]
fn draw_shaped(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
    frame: &Frame,
    fonts: &Fonts<'_>,
    index: usize,
    slice: &str,
    context: (&str, &str),
    size: f64,
    baseline: f64,
    mut x: f64,
) -> (f64, usize) {
    let Some(face) = fonts.faces().faces().get(index) else {
        return (x, 0);
    };
    let settings = settings_of(run.kerning, &run.features);
    let pieces: Vec<&str> = if run.word_spacing == 0.0 {
        vec![slice]
    } else {
        split_after_spaces(slice)
    };
    let mut refused = 0usize;
    let count = pieces.len();
    // The pieces in the order they are drawn: UAX #9's rule L2 over the
    // slice, each piece at the level of its strong characters, so a
    // right-to-left run's words are laid right to left and not in the order
    // they were written (review of lane 6C). A left-to-right slice is every
    // piece at one even level, and its order does not move.
    let direction = run.bidi_level.map(|level| {
        if level % 2 == 1 {
            BaseDirection::RightToLeft
        } else {
            BaseDirection::LeftToRight
        }
    });
    let order = match direction {
        Some(BaseDirection::RightToLeft) => (0..pieces.len()).rev().collect(),
        Some(_) => (0..pieces.len()).collect(),
        None => piece_order(slice, &pieces),
    };
    for at in order {
        let Some(piece) = pieces.get(at).copied() else {
            continue;
        };
        // The neighbours are the slice's, so the logically first piece is
        // shaped against what comes before it and the last against what comes
        // after.
        let piece_context = (
            if at == 0 { context.0 } else { "" },
            if at + 1 == count { context.1 } else { "" },
        );
        let Some(shaped) = shaped_glyphs(
            &face.program,
            piece,
            size,
            (run.letter_spacing * PX_TO_PT, run.word_spacing * PX_TO_PT),
            piece_context,
            &settings,
            direction.unwrap_or_else(|| own_direction(piece)),
        ) else {
            return (x, refused);
        };
        if !shaped.glyphs.is_empty() {
            // Neither writer sets `Tc` or `Tw`, and both are **text state**
            // that survives a `BT`/`ET` pair — so a composite draw after a
            // simple one would inherit the simple one's spacing. Zero for both
            // here: `letter-spacing` is already in the glyph positions, and a
            // `Tc` on top of them would be charged twice.
            page.raw(b"0 Tc 0 Tw");
            let placed: Vec<tinker_pdf_cos::build::PlacedGlyph<'_>> =
                shaped.glyphs.iter().map(Placed::as_glyph).collect();
            let mut bytes = Vec::new();
            if builder.glyph_run(
                &mut bytes,
                &face.resource,
                size,
                [1.0, 0.0, 0.0, 1.0, frame.x(x), baseline],
                &placed,
            ) {
                page.raw(&bytes);
            } else {
                // Ruling 10: the writer refusing a run is a fact about the
                // page, and a page short of a word that says nothing is the
                // failure this whole file is organised against. Counted here
                // and named by the caller.
                refused += 1;
            }
        }
        x += shaped.advance / PX_TO_PT
            + if piece.ends_with(' ') {
                run.word_spacing
            } else {
                0.0
            };
    }
    (x, refused)
}

/// The order `pieces` — consecutive substrings of `slice`, in logical order —
/// are drawn in, left to right.
///
/// Each piece takes the lowest level of its strong characters (of all of
/// them, for a piece of neutrals), which is how [`visual_lines`] levels a run,
/// and L2 orders the pieces. A slice with no right-to-left character keeps the
/// order it was written in without resolving anything.
fn piece_order(slice: &str, pieces: &[&str]) -> Vec<usize> {
    if pieces.len() < 2 || !slice.chars().any(opens_right_to_left) {
        return (0..pieces.len()).collect();
    }
    let paragraph = Paragraph::new(slice, BaseDirection::Auto);
    let resolved = paragraph.line(0..paragraph.len());
    let levels = resolved.levels();
    let mut piece_levels: Vec<Level> = Vec::with_capacity(pieces.len());
    let mut at = 0usize;
    for piece in pieces {
        piece_levels.push(level_of(piece, levels, at, paragraph.base_level()));
        at += piece.chars().count();
    }
    reorder(&piece_levels)
}

/// The level a stretch of a line is reordered at as one unit: the lowest
/// level of its strong characters, or of all of them for a stretch of
/// neutrals, or `base` where `levels` has none. `from` is where `text` starts
/// in `levels`, counted in characters.
fn level_of(text: &str, levels: &[Level], from: usize, base: Level) -> Level {
    let mut strong: Option<Level> = None;
    let mut any: Option<Level> = None;
    for (offset, c) in text.chars().enumerate() {
        let Some(level) = levels.get(from + offset).copied() else {
            continue;
        };
        any = Some(any.map_or(level, |l| l.min(level)));
        if matches!(
            bidi_class(c),
            BidiClass::L | BidiClass::R | BidiClass::AL | BidiClass::EN | BidiClass::AN
        ) {
            strong = Some(strong.map_or(level, |l| l.min(level)));
        }
    }
    strong.or(any).unwrap_or(base)
}

/// `slice`, cut after every space, with the space kept on the piece it ends.
fn split_after_spaces(slice: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (at, ch) in slice.char_indices() {
        if ch == ' ' {
            out.push(&slice[start..at + 1]);
            start = at + 1;
        }
    }
    if start < slice.len() {
        out.push(&slice[start..]);
    }
    out
}

/// How far short of where its letter ended a mark is drawn when nothing
/// says how exactly that letter was drawn, in ems: a hundredth, or half the
/// letter where it is narrower than two hundredths. A mark that opens a
/// slice, its letter drawn by the run or face segment before
/// ([`leading_mark_at`]), is drawn so until the spacing nears half an em; a
/// mark riding on a letter in its own slice takes [`EXACT_MARK_INSET`],
/// since that letter is drawn where layout put it.
///
/// Extraction (`tinker-pdf-content`) reads a glyph of no advance as a box a
/// thousandth of an em wide running right from where it is drawn, and ruling
/// 14 (`text_order.rs`) pairs a mark with the base whose box holds that box's
/// centre, or failing that the nearer one. At its base's end exactly, a
/// mark's box lies in the glyph drawn next, which is what read a decomposed
/// `e\u{301}` beside a right-to-left word as an `e` and an accented space. A
/// hundredth of an em inside, its centre is in its base's box by nine and a
/// half thousandths, more than the half thousandth a composite font's `/W`
/// rounds an advance by, and the distance is not one a reader sees: the
/// Liberation stand-in's Latin marks are drawn left of their origin, to sit
/// over the letter before them, and move a tenth of a point at twelve.
const MARK_INSET: f64 = 0.01;

/// How far short of its letter's end a mark is drawn on a letter drawn where
/// layout put it, in ems: nine hundred and ninety-nine millionths, so that
/// the mark's box, a thousandth wide, ends a millionth of an em **past**
/// where layout measured the letter to end, and its centre lies inside the
/// letter, very nearly half a thousandth from that end.
///
/// Every letter a mark rides on in its slice is drawn where layout put it: a
/// simple font's, whose widths — the standard 14's, and the overflow font's
/// `/Widths` — are whole thousandths of an em and are the numbers layout
/// measured with ([`standard_width`]), and the Liberation stand-in's, which
/// [`draw_coded`] draws as a piece of its own from the pen. A stand-in letter
/// is drawn as wide as its `/W`, layout's width rounded to a whole
/// thousandth, so it ends up to half a thousandth from where it was measured
/// to — and the rounding of a 2048-unit face's advance takes off at most
/// 127/256 of a thousandth, less than the 0.4995 thousandth that would put
/// the centre outside it.
///
/// A text object that ends with a mark leaves a reader's pen where the mark's
/// box ends, and the next text object — the next run, on the same line —
/// resumes the line only within half an em of it (`tinker-pdf-content`'s
/// `TextDevice`). The box ending a millionth of an em past where the letter
/// is measured to end, the next run is a spacing less a millionth from the
/// mark: whole up to a spacing of half an em and a millionth, and at half an
/// em exactly whole by that millionth, not by the last place of two sums. So
/// a mark moves where its line is cut by that millionth, and by no more,
/// after a simple font's letter, whose drawn box ends where layout measured
/// it to; after a stand-in's, which a reader measures by its `/W`, the letter
/// alone ends up to half a thousandth of an em either side of where layout
/// measured it, so near half an em a run ending on the letter and the same
/// run ending on a mark over it can be cut differently, within that
/// rounding (`0.5001em`, `bundled-fonts`: `x <b>ə</b>y z.` reads
/// `x \nəy z.` and `x <b>ə\u{301}</b>y z.` reads `x \nə\u{301}\ny z.`, the
/// schwa's `/W` rounding layout's width up). A run boundary with no mark at it, at a
/// spacing of half an em exactly, is cut or not by the last place of the
/// reader's sum and the painter's, marks elsewhere on the line or none
/// (`epub.md`, class A of the marks fix's round 4). bf081ca took a
/// ten-thousandth, which put the boundary after a mark at `0.5001em`, and a
/// hundredth on a stand-in's letter, which left the next run nine
/// thousandths of an em further from the mark's box than from the letter's
/// and cut the line from `0.491em`
/// (`a_standard_14_run_ending_on_a_mark_keeps_to_what_follows_to_half_an_em`).
const EXACT_MARK_INSET: f64 = 0.000_999;

/// Where a mark of no advance riding on the glyph drawn at `start`, `width`
/// wide, is drawn: short of the glyph's end, inside its box —
/// [`EXACT_MARK_INSET`] where `exact`, the glyph drawn where layout put it,
/// [`MARK_INSET`] where not, or half the glyph if it is narrower than twice
/// that.
pub(super) fn mark_at(start: f64, width: f64, font_size: f64, exact: bool) -> f64 {
    let width = width.max(0.0);
    let inset = if exact { EXACT_MARK_INSET } else { MARK_INSET };
    start + width - (font_size * inset).min(width / 2.0)
}

/// How far past where a text object's last glyph's box ends
/// `tinker-pdf-content`'s `TextDevice` resumes the line in the next text
/// object, in ems: half an em. Further on, the next glyph starts a line of
/// its own, and ruling 14's rejoin allows the same.
const RESUMED_WITHIN: f64 = 0.5;

/// How far apart, in ems, two text objects of a slice may start before a
/// reader of the page may part them ([`draw_coded`]'s *One text object for
/// the inside of a spaced slice*): [`RESUMED_WITHIN`], less a tenth of it so
/// that no gap a rounding takes past half an em is left out. A run whose
/// `letter-spacing`, and after a word space its `word-spacing` too, opens no
/// gap that wide is written as it always was.
const PARTED_FROM: f64 = RESUMED_WITHIN * 0.9;

/// Whether `run` is spaced so that a reader may part two text objects of it
/// ([`PARTED_FROM`]): its `letter-spacing`, and after a word space its
/// `word-spacing` too, as wide on the page in a reader's ems, under a matrix
/// whose [`along_the_line`] is `along`.
fn parted(run: &TextRun, along: f64) -> bool {
    run.letter_spacing
        .max(run.letter_spacing + run.word_spacing)
        * along
        > run.font_size * PARTED_FROM
}

/// How long one em of a run's own spacing is on the page in a reader's ems,
/// under `matrix`, the one the page draws the run under ([`OnPage::matrix`]):
/// the matrix's scale along the baseline over the scale it gives a glyph's
/// size, which is what `TextDevice` measures every gap against (its
/// `Matrix::expansion`, the square root of the determinant's magnitude).
/// One under every matrix that scales both axes alike — a shift, a turn,
/// `scale(2)` — and `√2` under `scaleX(2)`, whose gaps are twice as wide on
/// the page and whose glyphs are `√2` times as large; one too where the
/// matrix is not finite or flattens the plane, which draws nothing.
///
/// The rules that ask whether a reader parts two text objects of a run, or
/// two runs, ask it in these ems ([`parted`], [`page_lines`]):
/// in the run's own, a styled `?!` glued to a Hebrew word at
/// `word-spacing: 0.6em` under `transform: scaleX(2)` was drawn for a page
/// where it read with the word and read alone on the page written (lane V
/// of round 6).
fn along_the_line(matrix: [f64; 6]) -> f64 {
    let [a, b, c, d, _, _] = matrix;
    let size = (a * d - b * c).abs().sqrt();
    let along = hypot(a, b) / size;
    if along.is_finite() && along > 0.0 {
        along
    } else {
        1.0
    }
}

/// Where [`draw_coded`] draws a mark of no advance that opens a
/// left-to-right slice and rides on nothing in it, its letter drawn by the
/// run or face segment before — styled apart from it, or, in a sans-serif or
/// monospace run, a `WinAnsiEncoding` letter its own face draws before a
/// mark only the serif fallback has (`choose`): the letter ended at `start`,
/// where the slice starts, less `letter_spacing` after it.
///
/// [`MARK_INSET`] inside that end, since nothing here says what drew the
/// letter or how exactly — **except** where that would leave the pen at
/// `start`, where the next text object begins when the slice is nothing but
/// marks, more than half an em past the mark's box ([`RESUMED_WITHIN`]).
/// There the mark is drawn as though its letter ended no more than half an
/// em before the pen, [`EXACT_MARK_INSET`] inside that, and never nearer the
/// letter's end than [`EXACT_MARK_INSET`]: from a `letter-spacing` of
/// `0.490999em` to `0.5em` it moves from a hundredth to nine hundred and
/// ninety-nine millionths of an em inside the letter's end, and past half an em
/// it stays there, where the line is cut as after a mark that rides on its
/// letter. Drawn a hundredth inside at every spacing, as it was at bf081ca,
/// the pen was more than half an em on from `0.491em` and a line cd407d5
/// read whole was cut there: `<p>x<b>e</b><i>\u{301}</i>x y.</p>` at
/// `0.495em`, and at half an em a sans-serif `Nguye\u{302}\u{303}n` and
/// `x<b>e</b>\u{301}x` (round 3 of the marks fix). Within half an em of
/// spacing the mark's centre is still the nearer of its letter's box and the
/// next glyph's by most of the spacing, whatever rounding set its letter's
/// end.
fn leading_mark_at(start: f64, letter_spacing: f64, font_size: f64) -> f64 {
    let end = start - letter_spacing;
    let near = start - letter_spacing.max(font_size * RESUMED_WITHIN);
    (end - font_size * MARK_INSET).max(near - font_size * EXACT_MARK_INSET)
}

/// One standard-14 stretch, a character at a time.
///
/// A character is drawn where the pen stands, and the pen moves by its
/// advance and, but for a nonspacing mark, `letter-spacing`
/// ([`letter_spaced`]: a letter and its marks are one typographic character
/// unit, spaced once, as layout measured them). The one exception to where
/// is a nonspacing mark of no advance drawn after its letter
/// ([`coded_order`]'s `rides`): where the pen stood, at its letter's end,
/// its box lay in the next glyph's and extraction read it there. So it is
/// drawn **inside its letter's box**, at [`mark_at`], and is read with its
/// letter — a letter's second, third and fourth marks too, in either
/// direction — while the pen moves as layout measured.
///
/// # One text object for a slice that holds a mark
///
/// A segment is one string of codes from one position, which states no
/// position for one glyph inside it, so a mark is a segment of its own and
/// so is what follows it, from the pen. Those segments are written into
/// **one text object** ([`PageBuilder::text_pieces`]), as is every segment
/// of a slice that holds a mark: `tinker-pdf-content`'s `TextDevice`
/// resumes a line after an `ET` only within half an em of where the last
/// glyph's box ended, and ruling 14's rejoin allows the same half em. Drawn
/// in a text object of its own (6d79fa4), a mark left a reader's pen at
/// its own box, short of its letter's end, and the glyph after it was every
/// spacing past that: a word's pointed letters became lines of their own
/// from a quarter of an em of spacing (`The word ךְ\nלֶ\nמֶ\n, quoted`;
/// review of 6d79fa4). A slice holding no mark keeps a text object per
/// segment, and the pen moves by `advance + letter-spacing` in one sum as it
/// always did, so a slice with no mark is written as cd407d5 wrote it
/// (measured on the round-3 and round-4 probes' mark-free books,
/// letter-spaced ones among them) but for the overflow font's `Tw` under
/// `word-spacing` (below), and for two more kinds of slice, under spacing
/// that may part their text objects ([`PARTED_FROM`]).
///
/// # One text object for neutrals a reader undoes
///
/// A slice of neutrals at a right-to-left level, on a line that holds a
/// right-to-left character, is drawn in its level's order for a reader to
/// undo with its line ([`coded_order`]), and is one text object too. A
/// neutral past `WinAnsiEncoding` is the overflow font's and the word space
/// beside it a standard face's: two resources, and drawn as two text
/// objects, the reader's pen stopped at the space's box and the neutral
/// started a stretched word space later. `TextDevice` carries a pen across
/// spacing only into a right-to-left letter, so past half an em the line was
/// cut at the neutral: `<p>… أصبحنا ٥٪ <b>نعرف</b> كتاب</p>`, Arabic with no
/// `dir`, at `word-spacing: 0.5em`, drew its ` ٪` in level order, space
/// first, and read `٥ \nنعرف٪` (V3 of the review of the third review's K-11
/// fix; 9297d2d drew the slice as typed and conserved). In one text object
/// the space and the sign are pieces of it, the space at the run's `Tw` and
/// the sign at none ([`PieceText::UnspacedCodes`]), so the sign reads as a
/// `WinAnsiEncoding` one does, a glyph of the space's string
/// (`a_non_winansi_neutral_after_a_word_space_reads_with_its_number`). A
/// slice of one resource is written as before, and so is one whose run's
/// spacing parts no two text objects: written whole under any spacing, as
/// first fixed, a book with none was other bytes and read the same (16
/// books of the hunt's; review of CD-19's fix). Two shapes read worse for it
/// than at 56dfd93 until their lines were read back
/// ([`Fonts::set_readings`]): a sign after European digits before a bold
/// word in Arabic at `word-spacing: 1em`, `25٪ <b>W</b>`, read before its
/// number, and is drawn with the word before it whole now; and with
/// `bundled-fonts` an ARABIC COMMA after a number in justified Hebrew,
/// `N، <b>W</b>`, whose space started at the bold word's end and drew the
/// comma's line up to it, and is drawn in pieces now.
///
/// # One text object for the inside of a spaced slice
///
/// A slice that holds a character past `WinAnsiEncoding` needs two
/// resources, the overflow font for that character and the face's own for
/// its word spaces, and each change of resource started a text object. Under
/// spacing each starts a spacing past where the one before it ended, and a
/// reader cut the slice wherever that passed half an em and `TextDevice`
/// carries no pen, which is everywhere but into a right-to-left letter after
/// right-to-left text: a Greek or Cyrillic word ended at every stretched word
/// space, and an Arabic paragraph letter-spaced by `0.55em` and justified at
/// every word space, a text object `0.55em` past its word. While the
/// overflow font was written at the run's `Tw` the glyphs after its code-32
/// letter were moved over some of those gaps (below); written at `0 Tw`
/// every gap is where layout put it, and the pieces were read apart — the
/// last word of a Greek table cell glued to the right-to-left cell beside
/// it and read with that cell, `αβ | ספרγδ` (H2 of the review of CD-19's
/// fix), and an Arabic paragraph's words in the order drawn. So **the inside
/// of a spaced slice is one text object**, from its first unit that is not
/// white space to its last, each change of resource a piece of it
/// ([`PageBuilder::text_pieces`]), and is read as one line, as a slice of one
/// resource, one string, always was
/// (`the_inside_of_a_spaced_slice_is_one_text_object`). The white space at
/// either end of it stays a text object of its own, and so does every change
/// of resource of a slice [`joins_its_inside`] leaves in pieces, each read in
/// the order drawn, the order written: written whole, the word space after a
/// right-to-left word drew the word's line up to a Latin run beside it, and
/// ruling 14 read the two as one line whose Latin letters were the more
/// (`a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole`).
/// A run whose spacing opens no gap a reader parts two objects at
/// ([`PARTED_FROM`]) is written as before. A right-to-left slice of a
/// left-to-right paragraph on a line that holds another paragraph's text,
/// a table cell touching the next, is left in pieces too: whole, a table
/// with no `dir` whose cell `abc (x).` touches a right-to-left cell
/// (`border-spacing: 0; padding: 0`) drew what a right-to-left paragraph
/// draws for `أصبحنا نعرف.abc (x)`, and read so ([`joins_its_inside`]).
///
/// # Where the letter a mark rides on is drawn
///
/// A simple font's letter is drawn as wide as layout measured it. A
/// letter the Liberation stand-in draws is drawn as wide as its `/W` says,
/// layout's width rounded to a thousandth of an em, and in a string of them
/// each starts where the rounded widths before it put it; one that carries a
/// mark of no advance is a piece of its own, from the pen, so it starts
/// where layout put it and ends within that one rounding of where it was
/// measured to. (bf081ca drew it in its string and its mark a hundredth of
/// an em inside, which the review of bf081ca found cut a line from
/// `0.491em`; moving the letter so that it **ended** exactly, tried first,
/// moved where it **started** by the same rounding, and a line cut there
/// before a right-to-left word at exactly half an em.) Every mark that rides
/// on a letter in its slice is placed [`EXACT_MARK_INSET`] inside the end
/// layout measured for it.
///
/// # The overflow font's code 32
///
/// 9.3.3 applies `Tw` to any single-byte code 32, and the overflow font's
/// code 32 is a character of the book's — the first past `WinAnsiEncoding`
/// its face met ([`OVERFLOW_FIRST`]) — not a word space, which layout did
/// not space. **So the overflow font is written at no word spacing**
/// ([`Segment::unspaced`]): a segment of it alone at `0 Tw`, and a piece of
/// a text object of several as [`PieceText::UnspacedCodes`], which
/// [`PageBuilder::text_pieces`] writes at `0 Tw` between the pieces that
/// take the run's. Every glyph of it is drawn where layout put it, under
/// `word-spacing` and justification alike. A word spacing that is a zero is
/// written as it was, so a book with no mark that sets no word spacing, and
/// no `letter-spacing` that parts two text objects ([`PARTED_FROM`]), is
/// written byte for byte as before; in a slice that holds a mark the glyph
/// after the code-32 letter goes on in its string now, where it was a piece
/// of its own at the same place, so such a book is other bytes even with no
/// word spacing, and reads the same.
///
/// Until October 2026 (ROADMAP CD-19) the font was written at the run's
/// `Tw`, and in its string every glyph after the code 32 was drawn a
/// word spacing right of where layout put it — into the gap before the next
/// word (`مدة معلم.` at `word-spacing: 0.5em` read `مدةم علم.`), onto
/// punctuation another resource draws right after the word (`معلم.`'s MEM
/// on its full stop), onto the glyph after a run that ends inside a word
/// (`x <b>αλ</b>φα` at `word-spacing: 4px` drew the λ on the φ in a default
/// build), and onto a styled neutral glued to a right-to-left word, whose
/// line K-11's carry then read interleaved (`<p dir="rtl">ا <b>.</b>أصبحنا
/// ن</p>` at `0.6em` read `اأص.بحنا ن`, and search lost the word). The
/// fixes before it each placed the glyph after the code 32 where layout put
/// it in one kind of slice, as a piece of its own: one that holds a mark,
/// where the moved LAM of `مُدَّةَ مُعلِّم.` was drawn on its AIN at
/// `word-spacing: 4px` (re-review of f529a47), and one with no mark drawn left
/// to right inside right-to-left text, where with `bundled-fonts` the THREE
/// of `(١٢٣),` was drawn on its `(` (third review of the K-11 fix). A
/// right-to-left word's string kept moving: placed where layout put it,
/// table cells at `word-spacing: 1em` after a cell ending in a Latin word
/// left a stretched word gap only the moved glyph had closed, which
/// `tinker-pdf-content`'s `TextDevice` bridges since a gap no spacing
/// accounts for ends the word before it. Written at `0 Tw`, the probes of
/// ROADMAP K-11's reviews draw no glyph over another under `word-spacing` or
/// justification. Not a few books read right only because a moved glyph
/// closed or opened a gap — hundreds on the reviews' probes, of a dozen
/// shapes — and each line of them is read back as a reader of the page
/// reads it and drawn for that reading where some drawing of its runs reads
/// it right ([`Fonts::set_readings`]); the books no drawing tried reads
/// right ROADMAP CD-19 counts, shape by shape.
///
/// # Where a slice ends on a mark
///
/// The next slice, or the next run, is another text object, and its first
/// glyph starts one spacing past the letter's end. The mark's box ends a
/// millionth of an em past that end ([`EXACT_MARK_INSET`]), so that glyph is
/// a spacing less a millionth of an em from the reader's pen: a run that ends
/// on a mark is not cut from what follows up to half an em of spacing, half
/// an em included, in either build — at `0.495em` and `8px` too, where
/// bf081ca cut a stand-in letter's line (review of bf081ca) — and is cut past
/// half an em and a millionth. After its letter alone the run is cut past
/// half an em, or at it by a last place, for a simple font's letter, and
/// within half a thousandth of an em of it, by the `/W` rounding, for a
/// stand-in's. cd407d5 drew a stand-in's mark a spacing past its letter, its
/// box a thousandth of an em wide, and with `bundled-fonts` cut such a line
/// only past half an em and a thousandth: a known limit `epub.md` names
/// (`a_standard_14_run_ending_on_a_mark_is_cut_past_half_an_em_as_its_letter_is`).
///
/// # A mark whose letter is not in the slice
///
/// Styled apart from it, left to the standard 14 by a face of the book's
/// own that draws its letter, or — in a sans-serif or monospace run — drawn
/// by the serif fallback after a `WinAnsiEncoding` letter its own face
/// draws, a mark rides on nothing here. In a left-to-right run its letter
/// was drawn just before the slice, ending where the slice starts less a
/// `letter-spacing` (this run's is the one to hand), so a mark that opens
/// the slice is drawn inside that end ([`leading_mark_at`]): drawn at the
/// pen, it was read with the glyph after it on a line ruling 14 reorders,
/// `x<b>e</b>\u{301}x` beside a right-to-left word reading `xex\u{301}`. In a
/// right-to-left run the letter is drawn to the right of the slice, by a run
/// the content stream may have written earlier, and the mark is drawn where
/// the pen stands: a known limit `epub.md` names.
///
/// Returns where the pen ended, in layout pixels.
#[expect(
    clippy::too_many_arguments,
    reason = "the same pen state [`draw_shaped`] takes, for the same reason"
)]
fn draw_coded(
    page: &mut PageBuilder,
    run: &TextRun,
    frame: &Frame,
    fonts: &Fonts<'_>,
    slice: &str,
    size: f64,
    baseline: f64,
    x: f64,
    drawing: Drawing,
) -> f64 {
    let (objects, x) = plan_coded(run, fonts, slice, x, drawing);
    for object in &objects {
        flush(page, object, size, baseline, run, frame);
    }
    x
}

/// The text objects [`draw_coded`] writes for `slice` of `run` from `x`,
/// each its pieces in the order written, and where the pen ends — drawn as
/// `drawing` says where a reader read the line back otherwise
/// ([`Fonts::set_readings`]), and by the rules above where it says nothing.
fn plan_coded(
    run: &TextRun,
    fonts: &Fonts<'_>,
    slice: &str,
    mut x: f64,
    drawing: Drawing,
) -> (Vec<Vec<Segment>>, f64) {
    let mut objects: Vec<Vec<Segment>> = Vec::new();
    let font = request(run);
    let metrics = BookMetrics::with(fonts.faces());
    // Whether a slice of neutrals at a right-to-left level is drawn in its
    // level's order ([`coded_order`]): as its line decided
    // ([`TextRun::bidi_line_rtl`]), or as its reading back asked.
    let level_order = drawing.level_order.unwrap_or(run.bidi_line_rtl);
    // The text object being written: its pieces, each a stretch of one font
    // resource from one position. A PDF string is bytes in **one** font, so
    // a stretch that spills into the overflow font is a piece of its own,
    // and the second's origin is wherever the first's advance left it. A
    // stretch of a new resource starts a new text object, as it always has,
    // except in a slice that holds a mark or is neutrals a reader undoes
    // (see above).
    let mut object: Vec<Segment> = Vec::new();
    // Where the last character drawn that is not a mark started, its
    // advance, and whether a reader sets it as wide as it was measured
    // ([`mark_at`]).
    let mut base: Option<(f64, f64, bool)> = None;
    // Whether only marks have been drawn so far in a left-to-right run: a
    // mark whose letter ended the run or the face segment drawn just before
    // this one, at its left.
    let mut leading = !reads_right_to_left(run);
    // Whether the last character drawn was a mark: what follows it is a
    // piece of its own, from the pen, in the same text object.
    let mut after_mark = false;
    // Whether the slice holds a mark: a mark, and what follows one, is a
    // piece of the object open.
    let whole = slice.chars().any(|c| bidi_class(c) == BidiClass::NSM);
    // Whether the run is spaced so that a reader may part two text objects
    // of it: each starts a spacing past where the one before it ended, and
    // `TextDevice` resumes a line across an object's end only within half an
    // em ([`PARTED_FROM`]).
    let along = fonts.along(run);
    let spaced = parted(run, along);
    // Whether the slice is neutrals [`coded_order`] draws in their level's
    // order for a reader to undo with their line: at a right-to-left level,
    // on a line that holds a right-to-left character, with none of their
    // own. Only a spaced run's: under less spacing no reader parts its
    // objects, and the slice is written as it always was.
    let undone = spaced
        && level_order
        && drawing.cut != Some(Cut::Pieces)
        && run.bidi_level.is_some_and(|level| level % 2 == 1)
        && !slice.chars().any(opens_right_to_left);
    // Whether the slice is one text object, each stretch of a resource a
    // piece of it (see above): one that holds a mark, or neutrals a reader
    // undoes, or a spaced slice a reading back drew whole. Any other starts
    // an object at a new resource, but inside a spaced slice (below).
    let one_object = whole
        || undone
        || (spaced
            && matches!(
                drawing.cut,
                Some(Cut::Whole | Cut::Carried | Cut::Centered | Cut::Hugged)
            ));
    let start = x;
    let order = coded_order(run, slice, level_order);
    // Where a reading back asked for it ([`Cut::Hugged`]), the end of the
    // white space at the slice's left end — the first unit after it, which
    // starts a piece of its own where the pen stands — and how far right of
    // where the pen stands the white space is drawn: by the spacing after
    // its last unit, so that its box ends where the next glyph starts.
    let head = (spaced && drawing.cut == Some(Cut::Hugged))
        .then(|| order.iter().position(|(c, _)| !c.is_whitespace()))
        .flatten()
        .filter(|head| *head > 0);
    let lead = head
        .and_then(|head| order.get(head - 1))
        .map_or(0.0, |&(last, _)| {
            (if letter_spaced(last) {
                run.letter_spacing
            } else {
                0.0
            }) + if last == ' ' { run.word_spacing } else { 0.0 }
        });
    // Where a reading back asked for it ([`Cut::Carried`], [`Cut::Centered`]),
    // the white space at the slice's right end, from its first unit, and how
    // far right of where the pen stands it is drawn: by the word spacing of
    // its spaces, so that its last box ends a letter-spacing short of where
    // the pen ends, or by half of it.
    let tail = (spaced && matches!(drawing.cut, Some(Cut::Carried | Cut::Centered)))
        .then(|| {
            order
                .iter()
                .rposition(|(c, _)| !c.is_whitespace())
                .map_or(0, |last| last + 1)
        })
        .filter(|tail| *tail < order.len());
    let shift = tail.map_or(0.0, |tail| {
        let spaces = order
            .get(tail..)
            .unwrap_or_default()
            .iter()
            .filter(|(c, _)| *c == ' ')
            .count();
        // A count of characters, far below 2^52.
        #[allow(clippy::cast_precision_loss)]
        let spaces = spaces as f64;
        let share = if drawing.cut == Some(Cut::Centered) {
            0.5
        } else {
            1.0
        };
        spaces * run.word_spacing * share
    });
    // The inside of a spaced slice, from its first unit that is not white
    // space to its last, is one text object where [`joins_its_inside`]; the
    // white space at either end stays a text object of its own (see above).
    let inside = if spaced
        && drawing.cut.is_none()
        && joins_its_inside(run, slice, fonts.shares_its_line(run))
    {
        order
            .iter()
            .position(|(c, _)| !c.is_whitespace())
            .zip(order.iter().rposition(|(c, _)| !c.is_whitespace()))
    } else {
        None
    };

    for (at, &(ch, rides)) in order.iter().enumerate() {
        let mark = rides || bidi_class(ch) == BidiClass::NSM;
        if !mark {
            base = None;
            leading = false;
        }
        let chosen = choose(fonts.faces(), &font, Some(ch));
        let Some(coded) = fonts.encode(chosen, ch) else {
            // No code at all: the character is not drawn. Counted by
            // `Fonts::note` when the run was walked, so the page is short of
            // exactly as many characters as the report says.
            continue;
        };
        let advance = metrics.advance(ch, &font);
        let placed = match (mark && advance == 0.0, rides) {
            (false, _) => None,
            (true, true) => base.map(|(at, width, exact)| mark_at(at, width, run.font_size, exact)),
            // Its letter ended where this slice starts, less the spacing
            // after it — this run's, the one to hand — so the mark goes
            // inside it as though it rode on it ([`leading_mark_at`]).
            (true, false) if leading => {
                Some(leading_mark_at(start, run.letter_spacing, run.font_size))
            }
            (true, false) => None,
        };
        // A letter the stand-in draws, carrying a mark of no advance: a piece
        // of its own, from the pen, so that it is drawn where layout put it
        // and not where the rounded `/W` of the letters before it in its
        // string would — and ends within a rounding of where it was measured
        // to ([`EXACT_MARK_INSET`]).
        let alone = matches!(coded, Coded::Composite { .. })
            && !mark
            && order
                .get(at + 1)
                .is_some_and(|&(next, rides)| rides && metrics.advance(next, &font) == 0.0);
        let same = object
            .last()
            .is_some_and(|open| open.resource == coded.resource());
        // Where a reading back asked for the words in reading order
        // ([`Cut::Reading`]): a text object at every edge of a word.
        let edge = drawing.cut == Some(Cut::Reading)
            && at > 0
            && order
                .get(at - 1)
                .is_some_and(|(before, _)| before.is_whitespace() != ch.is_whitespace());
        let continues =
            !mark && !after_mark && !alone && same && !edge && Some(at) != tail && Some(at) != head;
        if !continues {
            let joined = one_object || inside.is_some_and(|(lo, hi)| lo < at && at <= hi);
            if !joined && !object.is_empty() {
                objects.push(std::mem::take(&mut object));
            }
            object.push(Segment {
                resource: coded.resource().to_vec(),
                composite: matches!(coded, Coded::Composite { .. }),
                // The overflow font's code 32 is a character of the book's,
                // not a word space, and 9.3.3 moves what follows it by `Tw`
                // all the same: the font is written at no word spacing.
                unspaced: matches!(
                    (&coded, chosen),
                    (Coded::Simple { resource, .. }, Chosen::Standard(face))
                        if *resource == face.overflow_resource()
                ),
                codes: Vec::new(),
                glyphs: Vec::new(),
                characters: String::new(),
                advances: Vec::new(),
                x: placed.unwrap_or(if Some(at) == tail {
                    x + shift
                } else if at == 0 && head.is_some() {
                    x + lead
                } else {
                    x
                }),
            });
        }
        if !mark {
            // Drawn where layout put it, as wide as measured or within a
            // rounding of it: a simple font's letter, or a stand-in's alone.
            let exact = matches!(coded, Coded::Simple { .. }) || alone;
            base = Some((x, advance, exact));
        }
        if let Some(open) = object.last_mut() {
            match coded {
                Coded::Simple { code, .. } => open.codes.push(code),
                Coded::Composite { id, .. } => open.glyphs.push((id, ch)),
            }
            open.characters.push(ch);
            open.advances.push(advance);
        }
        after_mark = mark;
        // One sum, `advance + letter-spacing`, as before a mark took none:
        // adding the two to the pen one at a time rounds differently in the
        // last place, and a letter-spaced book with no mark in it was written
        // with other bytes (review of bf081ca).
        x += advance
            + if letter_spaced(ch) {
                run.letter_spacing
            } else {
                0.0
            };
        if ch == ' ' {
            x += run.word_spacing;
        }
    }
    if !object.is_empty() {
        objects.push(object);
    }
    if drawing.cut == Some(Cut::Reading) && reads_right_to_left(run) {
        objects.reverse();
    }
    (objects, x)
}

/// Whether the inside of a spaced `slice` of `run` is written as one text
/// object ([`draw_coded`]'s *One text object for the inside of a spaced
/// slice*): a slice at its paragraph's own left-to-right level or at a
/// right-to-left level that holds no formatting character.
///
/// Not a left-to-right slice inside right-to-left text, at level 2 or above:
/// read whole, `Łódź Kraków ő` in a right-to-left paragraph joined the
/// Hebrew either side of it into a line whose Latin letters are the more,
/// which ruling 14 reads as a left-to-right paragraph's, and the paragraph's
/// words came back out of order; read in its pieces, each read in the order
/// drawn, which is the order written (6 books of the K-11 review's hunt in
/// a default build). And not a slice that holds an embedding, override or
/// isolate formatting character (`Bidi_Class` `LRE` to `PDI`), which the
/// overflow font draws: read whole, `שלום \u{202D}ab\u{202C} עולם
/// \u{202E}cd\u{202C} מלחמה.` at `word-spacing: 1em` or letter-spaced and
/// justified read its Hebrew words backwards and `ab` and `cd` were found
/// no more, where in its pieces it read whole (4 books of the hunt).
///
/// Nor, where its paragraph is left to right, a right-to-left slice of a
/// run on a line that holds another paragraph's text too (`shared`,
/// [`sharing_a_line`]): table cells set touching, which a reader reads as
/// one line. Whole, the Arabic or Hebrew cell of a table with no `dir`
/// touching a cell `abc (x).` at `word-spacing: 1em` drew the row what a
/// right-to-left paragraph draws for `أصبحنا نعرف.abc (x)`, and read so; in
/// its pieces the row reads whole, as it did before the inside of a spaced
/// slice was one object (2 books in a default build and 4 with
/// `bundled-fonts` of the review of b73550b;
/// `a_right_to_left_cell_touching_a_left_to_right_one_is_left_in_pieces`).
/// A right-to-left paragraph's slice on such a line is still one object,
/// since there the row reads whole either way and a right-to-left table at
/// `word-spacing: 1em` reads whole only so.
fn joins_its_inside(run: &TextRun, slice: &str, shared: bool) -> bool {
    inside_joins(run.bidi_level, run.paragraph_rtl, shared, slice)
}

/// [`joins_its_inside`] for a slice at `level` of a paragraph whose base
/// direction is `paragraph_rtl` ([`TextRun::paragraph_rtl`]), on a line that
/// holds another paragraph's text where `shared`.
fn inside_joins(level: Option<u8>, paragraph_rtl: Option<bool>, shared: bool, slice: &str) -> bool {
    level.is_none_or(|level| {
        level == 0 || (level % 2 == 1 && !(shared && paragraph_rtl == Some(false)))
    }) && !slice.chars().any(|c| {
        matches!(
            bidi_class(c),
            BidiClass::LRE
                | BidiClass::RLE
                | BidiClass::LRO
                | BidiClass::RLO
                | BidiClass::PDF
                | BidiClass::LRI
                | BidiClass::RLI
                | BidiClass::FSI
                | BidiClass::PDI
        )
    })
}

/// The characters of a standard-14 `slice` of `run` in the order they are
/// drawn, left to right: UAX #9's rule L2. Each comes with whether it
/// **rides** on the character drawn before it — a nonspacing mark after its
/// own base, which [`draw_coded`] draws inside that base's box.
///
/// [`draw_coded`] writes a code where the pen stands and moves the pen
/// right, so the order it is handed is the order the page shows. Handed
/// `slice` as written, it drew a right-to-left word backwards — `חו` with
/// its `ח` at the left — which nobody saw while extraction read the content
/// stream's order, since that order was the logical one too. Ruling 14
/// (dd50471) reads the line the page **draws**, as it must for every other
/// producer, and read the word back as `וח`. Every character past
/// `WinAnsiEncoding` in a standard-14 run comes here — the overflow font in
/// a default build, the Liberation stand-in with `bundled-fonts` — so a
/// book's Hebrew or Arabic that no face of its own covers extracted
/// reversed: the etymology of `pg2701-images.epub` in `epub_fetched.rs`'s
/// two conservation sweeps, and `epub_fallback.rs`'s
/// `a_standard_14_hebrew_word_extracts_as_written`.
///
/// The level is the run's, as for a shaped slice ([`draw_shaped`]):
/// [`TextRun::bidi_level`] once its line has cut it to one level, its own
/// P2 and P3 where nothing has. A slice with no character that reads or
/// opens right to left, on a line that holds none either or in a generated
/// run, which no reader reads, keeps the order it was written in without
/// resolving anything, as [`piece_order`] does — so a left-to-right page, and
/// a right-to-left `inside` marker's `1. ` beside an Arabic word, are drawn
/// exactly as before.
///
/// **A run of nothing but neutrals at a right-to-left level is drawn in its
/// level's order on a line that holds a right-to-left character, and as
/// typed on one that holds none** ([`TextRun::bidi_line_rtl`], which
/// [`split_line`] sets). On a line with no right-to-left character as typed
/// is the right answer — `<p dir="rtl">?!</p>` reads `?!`, since ruling 14
/// reads such a line in content order. On a line that holds one it was a
/// known limit until ROADMAP K-11: the `.,` between two right-to-left words
/// (`<p dir="rtl"><i>חו</i>.,<i>וח</i></p>`) is a run of its own that N1 puts
/// at the paragraph's level, where L2 draws it `,.`, and drawn as typed the
/// line read back `חו,.וח`; and in a sans-serif book whose Arabic the
/// overflow font draws, the ` (` and `). ` that Helvetica draws are face
/// segments of their own inside an Arabic run, drawn as typed, so the page
/// itself showed `عدة( النازعة` and read `.)وقد` — CI's
/// `sample-regime-anticancer-arabic.epub` at character 20 963. Reordering
/// every slice at an odd level, tried on the review of 6d08c6b, drew those
/// lines right, and the next review measured what it broke: every line of
/// neutrals alone at a right-to-left level read back reversed —
/// `<p dir="rtl">?!</p>` as `!?`, a heading's `?!`, `(...)` (drawn `)...(`,
/// each bracket's hollow turned away from the dots), the first line of
/// `?!<br/>חו`. Which of the two a slice is on is whether its **line** holds
/// a right-to-left character, and the line says so now (`epub_fallback.rs`
/// pins both sides) — but for the runs of neutrals on such a line that
/// a reader of the page would not undo, which [`neutrals_drawn_as_typed`]
/// names and which are drawn as typed as before (review of the K-11 fix).
///
/// The units L2 orders are **a character and the nonspacing marks after
/// it** (`Bidi_Class` `NSM`), kept together as rule L3 keeps a mark with its
/// base: reversed a character at a time, the marks of a right-to-left run
/// left their letters, and `שָׁלוֹם` read back `שָלׁוםֹ` in a default build
/// (review of 6d08c6b). The order is [`order_units`]'s, the function
/// `bidi_conformance.rs` runs the whole of Unicode's test files through, so
/// this draws what the reader of the page checks against — a character X9
/// removes, a joiner or a formatting character, kept at the level of the
/// unit before it (UAX #9 §5.2, *Retaining BNs and Explicit Formatting
/// Characters*): the overflow font draws what it is given, and a page short
/// of a character is what conservation counts.
///
/// **Inside a unit, the marks are drawn after their letter, in the order
/// written, in either direction** — L3's arrangement — and each rides on the
/// letter. A mark the overflow font draws has no advance ([`standard_width`])
/// and nor has one the Liberation stand-in draws, but for thirteen of its
/// bold serif's (the medieval superscript letters, U+0363 to U+036F, which
/// it sets as glyphs of their own after their letter); no mark takes
/// `letter-spacing` ([`letter_spaced`]), so a mark of no advance does not
/// move the pen, and [`draw_coded`] draws it inside its letter's box, where
/// ruling 14's extraction pairs it with that letter: the neighbour, in
/// content order, whose box holds its centre. How that came to be, since
/// each step was a fix of its own:
///
/// - The overflow font gave a mark a space's advance, and a mark drawn
///   after its letter had its centre exactly as far outside its letter's box
///   as outside the next glyph's: rounding decided which read it (`a מֶלֶךְ,`
///   came back `a מלֶךְ,ֶ` at a `font-size` of `12.5px` and of `14px`), and a
///   letter's second mark was nearer the next glyph and was read with it —
///   `sample-regime-anticancer-arabic.epub`'s `يًّا`, its FATHATAN on the
///   letter before (CI run 38041540464).
/// - A mark of no advance drawn where the pen stood after its letter lies in
///   the next glyph's box, since `tinker-pdf-content` reads a glyph of no
///   advance as a box a thousandth of an em wide running right from where it
///   is drawn: every point of `מֶלֶךְ` was read with its neighbour in a
///   `bundled-fonts` build. Drawn before its letter, where the letter
///   starts, it was read with the letter — until `letter-spacing`, which was
///   added after a mark as after any character, moved it off: `שָׁלוֹם` at
///   `0.5px` came back `שׁלָוֹם` and `מֶלֶךְ` at `-0.5px` `מלֶךְֶ`. And a
///   left-to-right slice, which keeps its marks after their letters, had
///   nowhere else to draw them: a decomposed `e\u{301}` beside a
///   right-to-left word read as an `e` and an accented space.
/// - Drawn inside its letter by a text object of its own (6d79fa4), a mark
///   left a reader's pen at its own box, and the glyph after it started
///   every spacing past — the letter's and each mark's: from a quarter of an
///   em of `letter-spacing` a pointed word was read a letter a line. A slice
///   holding a mark is one text object now ([`draw_coded`]), and a mark is
///   not spaced (`css-text-3` §10.2 spaces typographic character units, and
///   a letter with its marks is one).
/// - Drawn a hundredth of an em inside a letter the stand-in draws (bf081ca),
///   whose `/W` is rounded, a mark that ended a run left the next run nine
///   thousandths of an em further from a reader's pen than its letter would
///   have, and the line was cut from `0.491em` of spacing. Such a letter is
///   drawn where layout put it now, a piece of its own, and every mark on a
///   letter in its slice ends its box a millionth of an em past the end
///   layout measured for the letter ([`EXACT_MARK_INSET`]).
///
/// Positioned inside its letter, a mark is read with it at every size and
/// spacing the tests sweep, four to a letter, and its line is not cut
/// inside its slice; where a slice ends on a mark, [`draw_coded`] says how
/// far the next run may be. Where on its letter it stands is still not where
/// a point should, over the middle: that is `GPOS`'s to say, and an unshaped
/// run reads none. Marks with nothing before them in the slice, their letter
/// in another run or face segment, ride on nothing; [`draw_coded`] says
/// where those are drawn.
///
/// Mirroring (rule L4) is not applied: a simple font's code names one
/// character, so a mirrored glyph would extract as the other bracket. A
/// bracket pair at a right-to-left level is drawn with each bracket's hollow
/// turned away from what it encloses — the middle of `חו (וח) חו.` is drawn,
/// left to right, `)` `ח` `ו` `(` — and extracts as written.
fn coded_order(run: &TextRun, slice: &str, level_order: bool) -> Vec<(char, bool)> {
    let units = mark_clusters(slice);
    let as_written = || units.iter().flat_map(|unit| riding(unit)).collect();
    let neutrals_on_a_right_to_left_line =
        level_order && run.bidi_level.is_some_and(|level| level % 2 == 1);
    let neutrals = !slice.chars().any(opens_right_to_left);
    if neutrals && (!neutrals_on_a_right_to_left_line || opens_with_its_word_space(run, &units)) {
        return as_written();
    }
    let direction = match run.bidi_level {
        Some(level) if level % 2 == 1 => BaseDirection::RightToLeft,
        Some(_) => BaseDirection::LeftToRight,
        None => own_direction(slice),
    };
    let order: Vec<(char, bool)> = order_units(&units, direction)
        .into_iter()
        .filter_map(|at| units.get(at))
        .flat_map(|unit| riding(unit))
        .collect();
    // L2 is a permutation of the units, which cover the slice, so this
    // holds; were it ever not to, the slice is drawn as written rather than
    // short.
    if order.len() == slice.chars().count() {
        order
    } else {
        as_written()
    }
}

/// Whether a slice of neutrals of `run`, cut into `units` ([`mark_clusters`]),
/// is drawn as typed on a right-to-left line all the same ([`coded_order`]):
/// where its run is word-spaced, it opens with a word space as typed, and its
/// glyphs read alike either way round — one glyph, or a run of them that
/// reads the same reversed — so that drawn in its level's order or as typed
/// it reads the same, and the two differ only in which end its word space is
/// drawn at (third review of the K-11 fix).
///
/// Drawn as typed, the space is drawn first and the glyph after it, where
/// layout put the slice's end:
///
/// - **No glyph was drawn where the one before it overran** — the first
///   reason, and history since the overflow font is written at `0 Tw`
///   (ROADMAP CD-19). The overflow font's code
///   32 is a letter of the book's, and while the font was written at the
///   run's `Tw` the last glyphs of its string were drawn up to a word
///   spacing right of where layout put them, over whatever was drawn next:
///   drawn in its level's order the monospace `#` of `أصبحنا #نعرف`,
///   justified, stood on the NOON the overflow font moved, and the line read
///   `أصبحنا ن#عرف` (4 books in each build); drawn as typed, the space stood
///   there, which draws nothing, as at 9297d2d. The font is written at
///   `0 Tw` now and moves nothing.
/// - **The space is stretched inside the slice's own text object**, and the
///   glyph meets what is drawn after it. `TextDevice` carries a reader's pen
///   across a gap `Tw` made into a right-to-left letter only on a line that
///   holds one of its own, and the ` (` that a sans-serif book's Helvetica
///   draws at the left end of a justified Arabic line's run, `( ` in its
///   level's order, is a text object no right-to-left letter precedes: its
///   stretched space parted it from the Arabic word after it, and the `(`
///   was read as a line of its own, out of its place (the K-11 probe's
///   sans-serif books at `margin: 0 15%`, `(histone deacetylase)` three and
///   six times over, pointed and not).
///
/// Its glyphs, read in either order, are the same, so nothing a reader reads
/// moves but the space. Where that space parts a bracket from what it
/// encloses the bracket tie-break counts the pair no more: with
/// `bundled-fonts`, the third review's sans-serif Hebrew paragraph quoting
/// `(histone deacetylase)` four times, pointed and not, which 7330cd5 read
/// whole, reads one line left to right again, as 9297d2d did.
fn opens_with_its_word_space(run: &TextRun, units: &[&str]) -> bool {
    if run.word_spacing <= 0.0 || units.first() != Some(&" ") {
        return false;
    }
    let glyphs: Vec<&str> = units
        .iter()
        .copied()
        .filter(|unit| !unit.chars().all(char::is_whitespace))
        .collect();
    glyphs.iter().eq(glyphs.iter().rev())
}

/// One of [`mark_clusters`]' units as written, each character with whether
/// it rides on the one before: every mark after the unit's first character,
/// where that character is a base and not a mark whose base is elsewhere.
fn riding(unit: &str) -> impl Iterator<Item = (char, bool)> + '_ {
    let based = unit
        .chars()
        .next()
        .is_some_and(|first| bidi_class(first) != BidiClass::NSM);
    unit.chars()
        .enumerate()
        .map(move |(at, ch)| (ch, based && at > 0))
}

/// `slice` cut into a character and the nonspacing marks (`Bidi_Class`
/// `NSM`) that follow it, in order. Marks with nothing before them in the
/// slice — their base in another run — are a unit of their own.
///
/// Every cut is at a `char_indices` boundary, so no `get` here can miss; if
/// one ever did, the units would not cover the slice and [`coded_order`]
/// would draw it as written rather than short.
fn mark_clusters(slice: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (at, ch) in slice.char_indices() {
        if at > start && bidi_class(ch) != BidiClass::NSM {
            out.extend(slice.get(start..at));
            start = at;
        }
    }
    out.extend(slice.get(start..).filter(|rest| !rest.is_empty()));
    out
}

/// Writes one text object [`draw_coded`] assembled: its one segment as
/// [`PageBuilder::encoded_text`] or [`PageBuilder::glyphs`] writes it, or
/// its several — a mark and what follows it, each at its own position — as
/// one text object through [`PageBuilder::text_pieces`]. The overflow
/// font's segment is written at no word spacing, alone or as a
/// [`PieceText::UnspacedCodes`] piece ([`draw_coded`]'s *The overflow font's
/// code 32*).
///
/// # This is the caller of [`PageBuilder::glyphs`] that survived, and it is
/// the right one
///
/// [`draw_shaped`] went to `DocumentBuilder::glyph_run` because a shaped run
/// has per-glyph `GPOS` offsets and `glyphs` cannot spell one. **This segment
/// has none and never will**: it is the standard-14 overflow composite, whose
/// run is unshaped and one glyph per character at the face's own advances, so
/// letting the font's advances place it is not a limitation here but the whole
/// of what it needs. Writing it through `glyph_run` would state a position for
/// every glyph that the advances already give, and would buy nothing. The
/// one position the advances do not give — a mark drawn back inside its
/// letter, and the glyph after it, from the pen — is a segment of its own,
/// and [`PageBuilder::text_pieces`] states it.
///
/// Recorded rather than left to be found, because "one path was migrated and
/// one was not" reads as an unfinished migration until someone works out that
/// the two paths are not the same path.
fn flush(
    page: &mut PageBuilder,
    object: &[Segment],
    size: f64,
    y: f64,
    run: &TextRun,
    frame: &Frame,
) {
    let spacing = (run.letter_spacing * PX_TO_PT, run.word_spacing * PX_TO_PT);
    let texts: Vec<Vec<String>> = object
        .iter()
        .map(|segment| {
            segment
                .glyphs
                .iter()
                .map(|(_, ch)| ch.to_string())
                .collect()
        })
        .collect();
    let drawn: Vec<Vec<Glyph<'_>>> = object
        .iter()
        .zip(&texts)
        .map(|(segment, texts)| {
            segment
                .glyphs
                .iter()
                .zip(texts)
                .map(|((id, _), text)| Glyph {
                    id: *id,
                    text: text.as_str(),
                })
                .collect()
        })
        .collect();
    if let [segment] = object {
        let x = frame.x(segment.x);
        if segment.composite {
            let Some(glyphs) = drawn.first().filter(|glyphs| !glyphs.is_empty()) else {
                return;
            };
            // `PageBuilder::glyphs` writes no `Tc` and no `Tw` of its own, and
            // both are **text state** that survives a `BT`/`ET` pair — so a
            // composite draw after a simple one would inherit the simple one's
            // spacing. Setting them here is what keeps a composite segment's
            // spacing the run's own rather than whatever was set last.
            page.raw(format!("{} Tc 0 Tw", spacing.0).as_bytes());
            page.glyphs(&segment.resource, size, x, y, glyphs);
            return;
        }
        if segment.codes.is_empty() {
            return;
        }
        // The overflow font at no word spacing: its code 32 is a character of
        // the book's. A zero is written as it was, either sign, so a book that
        // sets no word spacing writes the `Tw` it always wrote.
        let (letters, words) = spacing;
        let spacing = if segment.unspaced && words != 0.0 {
            (letters, 0.0)
        } else {
            spacing
        };
        page.encoded_text(
            &segment.resource,
            size,
            x,
            y,
            spacing,
            &segment.codes,
            &segment.characters,
        );
        return;
    }
    let pieces: Vec<TextPiece<'_>> = object
        .iter()
        .zip(&drawn)
        .filter(|(segment, glyphs)| {
            if segment.composite {
                !glyphs.is_empty()
            } else {
                !segment.codes.is_empty()
            }
        })
        .map(|(segment, glyphs)| TextPiece {
            font: &segment.resource,
            x: frame.x(segment.x),
            text: if segment.composite {
                PieceText::Glyphs(glyphs)
            } else if segment.unspaced {
                PieceText::UnspacedCodes {
                    codes: &segment.codes,
                    characters: &segment.characters,
                }
            } else {
                PieceText::Codes {
                    codes: &segment.codes,
                    characters: &segment.characters,
                }
            },
        })
        .collect();
    if !pieces.is_empty() {
        page.text_pieces(size, y, spacing, &pieces);
    }
}

/// `text-decoration`, as a filled rectangle at the position CSS 2.2 §16.3.1
/// leaves to the user agent.
fn decorate(page: &mut PageBuilder, run: &TextRun, frame: &Frame, end_px: f64) {
    if run.decoration == TextDecoration::None {
        return;
    }
    let width = (end_px - run.x).max(0.0) * PX_TO_PT;
    if width <= 0.0 {
        return;
    }
    let size = run.font_size * PX_TO_PT;
    let thickness = (size / 14.0).max(0.4);
    let baseline = frame.y(run.y);
    let y = match run.decoration {
        // A tenth of an em below the baseline clears a descender's stem
        // without crossing it, which is what a reading system's own underline
        // does.
        TextDecoration::Underline => baseline - size * 0.1 - thickness,
        TextDecoration::Overline => baseline + size * 0.75,
        TextDecoration::LineThrough => baseline + size * 0.25,
        TextDecoration::None => return,
    };
    set_fill(page, run.color);
    fill(page, frame.x(run.x), y, width, thickness);
}

/// A rectangle a link annotation covers, in PDF points.
///
/// Padded by a fifth of the font size above the baseline and a tenth below,
/// which is the box a reader expects to be able to click: a rectangle exactly
/// on the baseline has no height at all, and `DocumentBuilder::link` refuses
/// one that encloses no area.
#[must_use]
pub fn run_rect(run: &TextRun, frame: &Frame) -> (f64, f64, f64, f64) {
    let size = run.font_size * PX_TO_PT;
    let baseline = frame.y(run.y);
    (
        frame.x(run.x),
        baseline - size * 0.25,
        frame.x(run.x) + run.width * PX_TO_PT,
        baseline + size * 0.85,
    )
}

/// Where a link goes, from an `href` that has already been classified.
#[must_use]
pub fn page_target(index: u32) -> Target {
    Target::Page {
        index,
        // `/Fit` rather than `/XYZ`: an EPUB cross-reference names a chapter
        // and this build has one chapter's worth of page, so scrolling to a
        // coordinate inside it would be a precision the source does not have.
        view: tinker_pdf_cos::dest::DestKind::Fit,
    }
}
