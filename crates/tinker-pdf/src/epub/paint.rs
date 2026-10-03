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

use std::collections::BTreeMap;
use tinker_pdf_cos::build::{
    DocumentBuilder, ExtGState, Glyph, PageBuilder, Target, TilingPattern, TilingType,
};
use tinker_pdf_css::cascade::StyleTree;

use tinker_pdf_css::property::{
    BackgroundSize, BorderStyle, Color, ComputedOffset, FontFamily, FontStyle, ImageRef,
    LengthPercentage, Position, RepeatStyle, Side, TextDecoration, Transform, TransformOrigin,
};
use tinker_pdf_font::base14::Standard14;
use tinker_pdf_font::encoding::{base_char, glyph_name_for_char, BaseEncoding};
use tinker_pdf_font::Sfnt;
use tinker_pdf_layout::metrics::{FontRequest, Metrics, PlacedGlyph, ShapedText, Shaper, Vertical};
use tinker_pdf_layout::{
    BackgroundLayer, BoxFragment, ClipFragment, Page as LayoutPage, ReplacedFragment, TextRun,
};
use tinker_pdf_shape::bidi::{reorder, BaseDirection, Paragraph};
use tinker_pdf_shape::shape::itemize;
use tinker_pdf_svg::transform::{concat, invert, rotation, IDENTITY};

use super::read::PX_TO_PT;
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
                // An East Asian character is one em wide in every face that has
                // one, and the standard 14 have none at all — so the number
                // cannot come from `Standard14`, which would answer with a
                // Latin space's advance and set a Japanese line at a third of
                // its width. UAX #11's own classification is what decides,
                // through the table `tinker-pdf-layout` already vendors for
                // UAX #14.
                if tinker_pdf_layout::unicode::is_east_asian(ch) {
                    return font.size;
                }
                let (advance, _) = face.standard().advance(ch);
                f64::from(advance) / 1000.0 * font.size
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

    fn shaper(&self) -> Option<&dyn Shaper> {
        Some(self)
    }
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
        let mut glyphs = Vec::new();
        let mut advance = 0.0;
        for (range, chosen) in face_runs(self.faces(), font, text) {
            let slice = text.get(range.clone()).unwrap_or("");
            let shaped = match chosen {
                Chosen::Embedded(index) => self
                    .faces()
                    .faces()
                    .get(index)
                    .and_then(|face| shape_with(&face.program, slice, font, rtl)),
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
    let shaper = tinker_pdf_shape::Shaper::new(&sfnt);
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
    /// Which embedded faces drew anything, so a book that declares six faces
    /// and uses two embeds two.
    used_embedded: Vec<bool>,
    /// Characters that could not be given a code at all.
    unrepresented: usize,
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
            used_embedded: vec![false; faces.faces().len()],
            unrepresented: 0,
            uncovered: 0,
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
            // somewhere the text is not.
            let widths: Vec<u16> = self.overflow[index]
                .iter()
                .map(|c| {
                    let em = if tinker_pdf_layout::unicode::is_east_asian(*c) {
                        1000.0
                    } else {
                        f64::from(face.standard().advance(*c).0)
                    };
                    em.round().clamp(0.0, 65535.0) as u16
                })
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
    }
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
        let planned: Vec<Vec<(usize, Plan)>> = pages
            .iter()
            .map(|page| {
                let mut plans = Vec::new();
                let locals = self.locals(page, frame);
                for (index, fragment) in page.boxes.iter().enumerate() {
                    let Some(layer) = &fragment.image else {
                        continue;
                    };
                    let Some((name, intrinsic)) = image(&layer.image) else {
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
        refused.box_shadow = shadowed.iter().filter(|(p, _)| *p == "box-shadow").count();
        refused.text_shadow = shadowed.iter().filter(|(p, _)| *p == "text-shadow").count();
        refused
    }

    fn close(page: &mut PageBuilder, opened: bool) {
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
    fn open(&self, page: &mut PageBuilder, anchor: Option<u32>, inside: bool) -> bool {
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
    let mut refused = 0usize;
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
    for fragment in &laid.replaced {
        let Some(anchor) = fragment.anchor else {
            continue;
        };
        let Some((_, name)) = pictures.iter().find(|(at, _)| *at == anchor) else {
            continue;
        };
        let opened = effects.open(page, fragment.anchor, false);
        draw_replaced(page, fragment, frame, name);
        Effects::close(page, opened);
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
            refused += draw_run(builder, page, &shadow, frame, fonts);
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
    let Some(dom) = dom else {
        for run in &laid.runs {
            if !run.painted {
                continue;
            }
            refused += artifact_or_run(builder, page, run, frame, fonts, effects);
        }
        draw_outlines(page, laid, frame, effects);
        return refused;
    };

    let drawn: Vec<&TextRun> = laid.runs.iter().filter(|run| run.painted).collect();
    let chains: Vec<Vec<usize>> = drawn
        .iter()
        .map(|run| match run.generated {
            // An artifact belongs to no element: 14.8.2.2 puts it outside the
            // structure entirely, which is `/Artifact` and not a tag.
            true => Vec::new(),
            false => ancestry(dom, run.anchor),
        })
        .collect();
    tag_runs(
        builder,
        page,
        frame,
        fonts,
        dom,
        chapter,
        &drawn,
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

/// One run, marked as an artifact where it is one.
fn artifact_or_run(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
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
    let refused = draw_run(builder, page, run, frame, fonts);
    Effects::close(page, opened);
    if run.generated {
        page.raw(b"EMC");
    }
    refused
}

/// The element chain a run sits under, outermost first.
///
/// From the run's own element up to — but not including — `<body>`, then
/// reversed. `<body>` is left out because [`DocumentBuilder`] already wraps
/// every page's roots in a `/Document`, and a `/Sect` per page under it that
/// meant "this chapter's body" would be a level that says nothing.
///
/// An element with no anchor gets an empty chain and is drawn untagged rather
/// than guessed at, which is the same refusal the reader makes: this build
/// does not invent structure (`docs/design/tagged-pdf.md`).
fn ancestry(dom: &Dom, anchor: Option<u32>) -> Vec<usize> {
    let Some(anchor) = anchor else {
        return Vec::new();
    };
    let mut at = anchor as usize;
    if at >= dom.nodes.len() {
        return Vec::new();
    }
    let body = dom.body();
    let mut chain = Vec::new();
    loop {
        if Some(at) == body {
            break;
        }
        chain.push(at);
        match dom.nodes[at].parent {
            // `parent` is always less than the node's own index, so this
            // terminates without a visited set.
            Some(parent) => at = parent,
            None => break,
        }
    }
    chain.reverse();
    chain
}

/// Draws a run of runs, opening one structure element per level they share.
///
/// **Grouped rather than one element per run.** Consecutive runs of one
/// paragraph share its whole chain, and opening a `/P` for each of them would
/// make a paragraph of three runs three paragraphs. The runs arrive in reading
/// order — `fragment::order` sorted them — so equal chains are adjacent and a
/// partition by the level's element is all the grouping there is to do.
#[allow(clippy::too_many_arguments)]
fn tag_runs(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    frame: &Frame,
    fonts: &Fonts<'_>,
    dom: &Dom,
    chapter: u64,
    runs: &[&TextRun],
    chains: &[Vec<usize>],
    level: usize,
    refused: &mut usize,
    effects: &OnPage<'_>,
) {
    let mut at = 0usize;
    while at < runs.len() {
        // A run whose chain has run out belongs to the element opened around
        // it, so it is drawn here rather than descended into.
        if chains[at].len() <= level {
            *refused += artifact_or_run(builder, page, runs[at], frame, fonts, effects);
            at += 1;
            continue;
        }
        let element = chains[at][level];
        let mut end = at + 1;
        while end < runs.len() && chains[end].get(level) == Some(&element) {
            end += 1;
        }
        let tag = structure_type(&dom.nodes[element].name);
        let (slice, tails) = (&runs[at..end], &chains[at..end]);
        // **The key is the element and the order is the reading position**,
        // and they are two numbers because they answer two questions. The key
        // has to be the same on every page this element appears on or its
        // halves never merge, so it is the element's own index. The order has
        // to ascend with the document or the halves merge into the wrong
        // place, so it is the reading-order stamp of the first run under it —
        // which for a float is where it was *met*, not where its box landed.
        let key = chapter + element as u64;
        let order = chapter + runs[at].order as u64;
        page.tagged_keyed(tag.as_bytes(), key, order, |page| {
            tag_runs(
                builder,
                page,
                frame,
                fonts,
                dom,
                chapter,
                slice,
                tails,
                level + 1,
                refused,
                effects,
            );
        });
        at = end;
    }
}

/// ISO 32000 Table 333's standard structure type for an XHTML element.
///
/// **Every arm returns a standard type, which is why no `/RoleMap` is
/// written.** 14.7.3's role map exists to say what a non-standard tag means;
/// a producer that only ever emits standard tags has nothing to declare, and
/// a role map mapping `/P` to `/P` is the loop the reader counts as a warning.
/// The cost is that the XHTML element name is not recoverable from the PDF —
/// `<em>` and `<strong>` are both `/Span` — which is named in the refusal
/// table rather than hidden.
fn structure_type(name: &str) -> &'static str {
    match name {
        "p" => "P",
        "h1" => "H1",
        "h2" => "H2",
        "h3" => "H3",
        "h4" => "H4",
        "h5" => "H5",
        "h6" => "H6",
        "ul" | "ol" | "dl" => "L",
        "li" | "dt" | "dd" => "LI",
        "table" => "Table",
        "thead" => "THead",
        "tbody" => "TBody",
        "tfoot" => "TFoot",
        "tr" => "TR",
        "td" => "TD",
        "th" => "TH",
        "caption" | "figcaption" => "Caption",
        "blockquote" => "BlockQuote",
        "code" | "kbd" | "samp" | "var" | "pre" => "Code",
        "sub" => "Sub",
        "figure" => "Figure",
        "section" | "article" | "nav" | "aside" | "header" | "footer" | "main" => "Sect",
        // **`<a>` is a `/Span` and not a `/Link`**, which is a refusal rather
        // than an oversight. 14.8.4.4.2 requires a `/Link` element to contain
        // an `/OBJR` referencing the link annotation it stands for, and this
        // writer cannot emit one; a bare `/Link` would claim an association to
        // assistive technology that is not in the file. The annotation itself
        // is still written and still works.
        //
        // §14.8.4.2's two inline defaults. Anything block-level this build
        // does not name is a `/Div` and anything else is a `/Span`, which is
        // what a reader does with an unknown tag anyway — and is honest,
        // because the alternative is inventing a type from a class attribute.
        "div" | "body" | "html" | "form" | "fieldset" => "Div",
        _ => "Span",
    }
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
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    page.raw(format!("{x} {y} {width} {height} re f").as_bytes());
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
fn draw_replaced(page: &mut PageBuilder, fragment: &ReplacedFragment, frame: &Frame, name: &[u8]) {
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

/// One stretch of a run sharing a font resource: what will become one text
/// object.
struct Segment {
    resource: Vec<u8>,
    composite: bool,
    codes: Vec<u8>,
    glyphs: Vec<(u16, char)>,
    characters: String,
    x: f64,
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
/// measures `letter_spacing × chars().count()` per **character**
/// (`flow.rs`'s `measure`), so a ligature or a joined Arabic word was drawn
/// *narrower* than the line box it was measured into. Folding the spacing in
/// at cluster boundaries — one character's worth per character, none between a
/// mark and its base — makes the drawn width the measured width by
/// construction.
fn shaped_glyphs(program: &[u8], text: &str, size: f64, letter_spacing: f64) -> Option<Shaped> {
    let sfnt = Sfnt::parse(program)?;
    let upem = f64::from(sfnt.units_per_em.max(1));
    let scale = |units: i32| f64::from(units) * size / upem;
    let shaper = tinker_pdf_shape::Shaper::new(&sfnt);
    let paragraph = Paragraph::new(text, BaseDirection::Auto);
    let runs = itemize(text, &paragraph);
    let shaped: Vec<_> = runs.iter().map(|run| shaper.shape(text, run)).collect();
    let levels: Vec<_> = runs.iter().map(|run| run.level).collect();

    let mut out: Vec<Placed> = Vec::new();
    let mut pen = 0.0f64;
    // Characters whose clusters are already behind the pen, and the characters
    // of the cluster it is inside. `letter-spacing` is charged once per
    // character and paid at the cluster boundary, so a mark keeps the position
    // its anchor gave it.
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
        let texts = crate::shaping::cluster_texts(text, run);
        let order: Vec<usize> = if run.direction().is_forward() {
            (0..glyphs.len()).collect()
        } else {
            (0..glyphs.len()).rev().collect()
        };
        for at in order {
            let Some(glyph) = glyphs.get(at) else {
                continue;
            };
            if cluster.is_some_and(|last| last != glyph.cluster) {
                spaced = spaced.saturating_add(pending);
                pending = 0;
            }
            let stands_for = texts.get(at).copied().unwrap_or("");
            if !stands_for.is_empty() {
                pending = stands_for.chars().count();
            }
            cluster = Some(glyph.cluster);
            out.push(Placed {
                id: glyph.glyph,
                text: stands_for.to_string(),
                x: pen + letter_spacing * spaced as f64 + scale(glyph.x_offset),
                rise: scale(glyph.y_offset),
            });
            pen += scale(glyph.x_advance);
        }
    }
    Some(Shaped {
        // The whole slice's `letter-spacing` rather than the sum of the
        // clusters', so the pen agrees with `flow.rs`'s `measure` exactly even
        // where a shaper dropped a character that started no cluster of its
        // own.
        advance: pen + letter_spacing * text.chars().count() as f64,
        glyphs: out,
    })
}

fn draw_run(
    builder: &mut DocumentBuilder,
    page: &mut PageBuilder,
    run: &TextRun,
    frame: &Frame,
    fonts: &Fonts<'_>,
) -> usize {
    let font = request(run);
    let size = run.font_size * PX_TO_PT;
    let baseline = frame.y(run.y);
    let mut x = run.x;
    let mut refused = 0usize;

    set_fill(page, run.color);
    // `css-fonts-4` §5.3 first, then shaping: see [`face_runs`]. An embedded
    // face's stretch is shaped whole; a standard-14 one keeps the
    // character-at-a-time path, because a simple font addresses a code and
    // there is no sfnt in this process to shape against.
    let mut segments = face_runs(fonts.faces(), &font, &run.text);
    if right_to_left(&run.text) {
        segments.reverse();
    }
    for (range, chosen) in segments {
        let slice = run.text.get(range).unwrap_or("");
        match chosen {
            Chosen::Embedded(index) => {
                let drawn = draw_shaped(
                    builder, page, run, frame, fonts, index, slice, size, baseline, x,
                );
                x = drawn.0;
                refused += drawn.1;
            }
            Chosen::Standard(_) => {
                x = draw_coded(page, run, frame, fonts, slice, size, baseline, x);
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
/// **The unit is the `TextRun` and not the visual line**, and that is a real
/// limit rather than a simplification. `flow.rs` breaks lines over logical
/// text and resolves no levels, so a line made of two styled spans is two
/// `TextRun`s at two `x`s this file did not choose; reordering across them
/// would mean moving boxes layout placed. What this closes is the case
/// fallback creates — one run, one style, several faces — which is the case
/// `docs/features/fonts.md` named.
///
/// **And a standard-14 segment is still drawn a character at a time in
/// logical order**, because [`draw_coded`] addresses codes rather than glyphs
/// and there is no sfnt in this process to shape or reorder against. A
/// right-to-left run that falls partly to the standard 14 therefore has its
/// segments in visual order and that segment's letters in logical order.
/// It was that way before this: the segments were in logical order too, so
/// what changes is that half of the answer is now right rather than none of
/// it. Named rather than implied.
fn right_to_left(text: &str) -> bool {
    Paragraph::new(text, BaseDirection::Auto)
        .base_level()
        .is_rtl()
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
    size: f64,
    baseline: f64,
    mut x: f64,
) -> (f64, usize) {
    let Some(face) = fonts.faces().faces().get(index) else {
        return (x, 0);
    };
    let pieces: Vec<&str> = if run.word_spacing == 0.0 {
        vec![slice]
    } else {
        split_after_spaces(slice)
    };
    let mut refused = 0usize;
    for piece in pieces {
        let Some(shaped) = shaped_glyphs(&face.program, piece, size, run.letter_spacing * PX_TO_PT)
        else {
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

/// One standard-14 stretch, a character at a time.
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
    mut x: f64,
) -> f64 {
    let font = request(run);
    let metrics = BookMetrics::with(fonts.faces());
    // One text object per contiguous stretch of characters sharing a font
    // resource, because a PDF string is bytes in **one** font: a stretch that
    // spills into the overflow font is two show operations and not one, and
    // the second's origin is wherever the first's advance left it.
    let mut segment: Option<Segment> = None;

    for ch in slice.chars() {
        let chosen = choose(fonts.faces(), &font, Some(ch));
        let Some(coded) = fonts.encode(chosen, ch) else {
            // No code at all: the character is not drawn. Counted by
            // `Fonts::note` when the run was walked, so the page is short of
            // exactly as many characters as the report says.
            continue;
        };
        let same = segment
            .as_ref()
            .is_some_and(|open| open.resource == coded.resource());
        if !same {
            flush(page, segment.take(), size, baseline, run, frame);
            segment = Some(Segment {
                resource: coded.resource().to_vec(),
                composite: matches!(coded, Coded::Composite { .. }),
                codes: Vec::new(),
                glyphs: Vec::new(),
                characters: String::new(),
                x,
            });
        }
        if let Some(open) = segment.as_mut() {
            match coded {
                Coded::Simple { code, .. } => open.codes.push(code),
                Coded::Composite { id, .. } => open.glyphs.push((id, ch)),
            }
            open.characters.push(ch);
        }
        x += metrics.advance(ch, &font) + run.letter_spacing;
        if ch == ' ' {
            x += run.word_spacing;
        }
    }
    flush(page, segment.take(), size, baseline, run, frame);
    x
}

/// Writes one segment as one text object.
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
/// every glyph that the advances already give, and would buy nothing.
///
/// Recorded rather than left to be found, because "one path was migrated and
/// one was not" reads as an unfinished migration until someone works out that
/// the two paths are not the same path.
fn flush(
    page: &mut PageBuilder,
    segment: Option<Segment>,
    size: f64,
    y: f64,
    run: &TextRun,
    frame: &Frame,
) {
    let Some(segment) = segment else { return };
    let x = frame.x(segment.x);
    if segment.composite {
        if segment.glyphs.is_empty() {
            return;
        }
        // `PageBuilder::glyphs` writes no `Tc` and no `Tw` of its own, and both
        // are **text state** that survives a `BT`/`ET` pair — so a composite
        // draw after a simple one would inherit the simple one's spacing.
        // Setting them here is what keeps a composite segment's spacing the
        // run's own rather than whatever was set last.
        page.raw(format!("{} Tc 0 Tw", run.letter_spacing * PX_TO_PT).as_bytes());
        let texts: Vec<String> = segment
            .glyphs
            .iter()
            .map(|(_, ch)| ch.to_string())
            .collect();
        let drawn: Vec<Glyph<'_>> = segment
            .glyphs
            .iter()
            .zip(texts.iter())
            .map(|((id, _), text)| Glyph {
                id: *id,
                text: text.as_str(),
            })
            .collect();
        page.glyphs(&segment.resource, size, x, y, &drawn);
        return;
    }
    if segment.codes.is_empty() {
        return;
    }
    page.encoded_text(
        &segment.resource,
        size,
        x,
        y,
        (run.letter_spacing * PX_TO_PT, run.word_spacing * PX_TO_PT),
        &segment.codes,
        &segment.characters,
    );
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
