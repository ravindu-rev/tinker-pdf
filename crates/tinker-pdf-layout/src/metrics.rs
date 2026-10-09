//! Measurement, as a trait, so nothing here depends on `tinker-pdf-font`.
//!
//! Row 7 of gap 31's milestone table asks for *"the `Metrics` trait, so nothing
//! here depends on `font`"*, and the reason is ruling 8 rather than tidiness: a
//! leaf takes bytes and plain parameters and returns values, and a crate that
//! parsed an sfnt to find an advance width would have acquired a font reader,
//! a subsetter and a `FontProvider` along with it.
//!
//! It is also what makes the fuzz target possible. A structured generator can
//! hand this crate a tree and a [`FixedPitch`] and get a full pagination out,
//! with no font file anywhere — which is the difference between fuzzing layout
//! and fuzzing a font parser through layout.
//!
//! # What the trait deliberately does not have
//!
//! **No kerning and no shaping.** An advance width here is per character and
//! the sum over a run is the run's width, which is what `tinker-pdf-content`
//! already assumes when it writes a `Tj`. A face whose real advance depends on
//! the pair would be measured wrong by this crate and drawn right by the
//! renderer, and the two disagreeing is worse than both being simple. Gap 31's
//! non-goals refuse shaping by name.
//!
//! **No fallback.** [`Metrics::advance`] is asked for one character in one
//! request and answers; *which* face that is came from `css-fonts-4` §5's
//! matching, which is milestone 9's and lives above this crate.

use tinker_pdf_css::property::{FeatureSetting, FontFamily, FontKerning, FontStyle};

/// Which face a run wants, and at what size.
///
/// A borrowed view rather than an owned struct because it is built once per
/// run and passed per character; cloning a `Vec<FontFamily>` per glyph would
/// be the whole cost of measuring a paragraph.
#[derive(Clone, Copy, Debug)]
pub struct FontRequest<'a> {
    /// The `font-family` list, in the author's order.
    pub families: &'a [FontFamily],
    /// The computed `font-weight`, 1 to 1000.
    pub weight: u16,
    /// The computed `font-style`.
    pub style: FontStyle,
    /// The computed `font-size`, in points.
    pub size: f64,
    /// The computed `font-kerning`, which a shaper turns into its `kern`
    /// feature and a provider that does not shape has no use for.
    pub kerning: FontKerning,
    /// The computed `font-feature-settings`, in the order written: the
    /// features a shaper switches on or off over its own plan.
    pub features: &'a [FeatureSetting],
}

/// How tall a line of one face is.
///
/// Both are positive and in points: `descent` is a **depth below the
/// baseline**, not a negative number, because a provider that returned the
/// sfnt's own sign convention and one that returned the absolute value would
/// both look plausible and one of them would put every line on top of the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertical {
    /// Above the baseline.
    pub ascent: f64,
    /// Below the baseline.
    pub descent: f64,
}

impl Vertical {
    /// `css-inline-3`'s content area: the sum of the two.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.ascent + self.descent
    }
}

/// What UAX #9's rule P2 found first in some text: [`Metrics::first_strong`]'s
/// answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstStrong {
    /// A strong left-to-right character (`Bidi_Class` `L`).
    Left,
    /// A strong right-to-left character (`R` or `AL`).
    Right,
    /// A paragraph separator (`B`) before any strong character: the
    /// paragraph ended with none, and P3 makes it left to right.
    Separator,
    /// None of the three before the text ran out.
    Neither,
}

/// One glyph, positioned, in points.
///
/// The projection of `tinker_pdf_shape::ShapedGlyph` into this crate's units
/// and this crate's vocabulary. It is a **copy** rather than a re-export
/// because ruling 8 keeps `tinker-pdf-layout` a leaf: nothing here may depend
/// on the shaping crate, so a provider that has one converts at the seam, and
/// a provider that has no shaper at all never sees this type.
///
/// The conversion the provider does is `units * size / units_per_em` — one
/// multiply and one divide, both correctly rounded by IEEE 754 and therefore
/// identical on every target, which is the side of ruling 4's line the rule
/// allows. The integers stay integers until exactly there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    /// The glyph index in the face the run was shaped against.
    pub glyph: u16,
    /// The byte offset, in the run's own text, of the character this glyph
    /// stands for. One glyph may stand for several characters, in which case
    /// it carries the first — which is what lets a consumer rebuild the text
    /// behind a ligature.
    pub cluster: u32,
    /// How far the pen moves after drawing this glyph.
    pub x_advance: f64,
    /// The same vertically, which is zero in horizontal text.
    pub y_advance: f64,
    /// Where this glyph is drawn relative to the pen.
    pub x_offset: f64,
    /// The same vertically.
    pub y_offset: f64,
}

/// One run of text, shaped.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedText {
    /// The glyphs, in **logical** order — the order the text was typed in,
    /// whichever way it reads. Turning that into the order they are drawn in
    /// is the consumer's, per line, after breaking.
    pub glyphs: Vec<PlacedGlyph>,
    /// The sum of the glyphs' horizontal advances.
    ///
    /// Carried rather than recomputed because it is what layout asks for
    /// nine times out of ten, and because a caller that summed the glyphs
    /// itself would be a second measurement of one run — the failure this
    /// module's own documentation warns about.
    pub advance: f64,
    /// Whether the run reads right to left.
    pub rtl: bool,
}

/// How much of a neighbour's text a shaper is handed ([`Neighbour::text`]):
/// its near 64 bytes, cut back to a character boundary — sixteen characters
/// of any script at the least, four bytes being UTF-8's longest.
///
/// **Not a cap: nothing is refused at it.** It is the reach of a shaping
/// context, and more than the shaping above this crate looks at — joining
/// reaches past a few transparent marks to the nearest letter, and a pair or
/// a mark one glyph away; the EPUB provider shapes eight characters of a
/// neighbour. It is fixed **here** because of what a neighbour is: before a
/// slice it is everything on the line so far, and a slice is measured at
/// every break opportunity, so a neighbour handed over whole made the cost
/// of filling a line depend on every provider reading only its near end — a
/// provider that counted it to find its last few characters made the line
/// quadratic in its length, and a paragraph on one line is any book's for a
/// tiny `font-size` (review of lane 8C). Cut here, no provider can.
pub const CONTEXT_BYTES: usize = 64;

/// One neighbour of a run on its line: its text and the face it asks for.
///
/// The font travels with the text because whether a neighbour is a context
/// at all is the **provider's** question, not this crate's: a glyph means
/// something only in the face it came from, so a shaper joins across a span
/// boundary or kerns a pair across it only where both sides resolve to one
/// face — and resolving a face is `css-fonts-4` §5.3's matching, which lives
/// above this crate.
#[derive(Clone, Copy, Debug)]
pub struct Neighbour<'a> {
    /// The near end of the neighbour's text on this line: at most
    /// [`CONTEXT_BYTES`] of it, cut back to a character boundary — the last
    /// bytes of what comes before, the first of what comes after. The
    /// provider takes as much of it as its shaping can see.
    pub text: &'a str,
    /// The neighbour's own face request.
    pub font: FontRequest<'a>,
}

/// What touches a run on its line, either side: the text a shaper may join
/// across or position against.
///
/// `None` on a side is a line's edge, an atomic box, generated content, text
/// that is not painted, or no neighbour at all — the places a painter shapes
/// a run with nothing beside it, so that a run measured with a context is
/// drawn with the same one.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapingContext<'a> {
    /// The text before the run, in logical order.
    pub before: Option<Neighbour<'a>>,
    /// The text after it.
    pub after: Option<Neighbour<'a>>,
}

impl ShapingContext<'_> {
    /// No neighbours: a run shaped alone.
    pub const NONE: ShapingContext<'static> = ShapingContext {
        before: None,
        after: None,
    };
}

/// Text in, positioned glyphs out: the seam a shaping engine plugs into.
///
/// # One path owns a run
///
/// [`Metrics`]'s own documentation warns that a face whose real advance
/// depends on the pair *"would be measured wrong by this crate and drawn right
/// by the renderer, and the two disagreeing is worse than both being simple"*.
/// A shaper makes that warning sharp, because now there really are two ways to
/// measure a run and they really do disagree — a ligature is narrower than its
/// components and a joined Arabic word is narrower still.
///
/// So the rule is absolute: **an item measured through a `Shaper` is never
/// also measured through [`Metrics::measure`] or [`Metrics::advance`]**, and
/// which of the two owns a run is decided once, by whether
/// [`Metrics::shaper`] answers. `flow.rs` asks it in exactly one place, and
/// `tests/shaper.rs` asserts that a provider whose `advance` panics still
/// paginates.
///
/// # Why the trait is here and not in the shaping crate
///
/// Ruling 8. `tinker-pdf-layout` is a leaf and gains no dependency edge for
/// this: the trait is plain structs and `f64`, and the provider that
/// implements it is above both crates. `docs/design/shaping.md` puts it
/// *"next to `Metrics` in `metrics.rs`"* for that reason.
pub trait Shaper {
    /// Shapes one run of one style.
    ///
    /// `rtl` is the direction the caller resolved, so that a provider does not
    /// re-run UAX #9 per run — the paragraph's levels were resolved once,
    /// above.
    fn shape(&self, text: &str, font: &FontRequest<'_>, rtl: bool) -> ShapedText;

    /// Shapes one run **in its context**: the glyphs and advances of `text`
    /// alone, as they come out when its neighbours on the line are shaped
    /// beside it.
    ///
    /// # Why a run is not measured alone
    ///
    /// A styled span is a run of its own, and a shaper's decisions reach
    /// across it: an Arabic letter takes its joined form from the letter in
    /// the next span, and `GPOS` kerns a pair whose second glyph is coloured.
    /// A painter that shapes each run against its neighbours draws those
    /// forms and that kerning; a layout that measured each run alone placed
    /// the next run where the isolated form, or the unkerned pair, would have
    /// left it, and the difference was a gap or an overlap between the two.
    /// So a run is measured here with the context it will be drawn with, and
    /// the line breaker sees a joined form's advance and a kerned pair's.
    ///
    /// Only the run's **own** glyphs come back, their clusters indexing
    /// `text`; a neighbour's glyphs are shaped and dropped, because the
    /// neighbour's own run is measured — with this run as its context — on
    /// its own turn.
    ///
    /// The default is [`Shaper::shape`], context unread: a provider that has
    /// no shaping across runs measures every run alone, as every provider did
    /// before this method existed.
    fn shape_in(
        &self,
        text: &str,
        font: &FontRequest<'_>,
        rtl: bool,
        context: &ShapingContext<'_>,
    ) -> ShapedText {
        let _ = context;
        self.shape(text, font, rtl)
    }
}

/// Where advance widths and line heights come from.
pub trait Metrics {
    /// One character's advance, in points, at the request's size.
    fn advance(&self, ch: char, font: &FontRequest<'_>) -> f64;

    /// The face's ascent and descent, in points, at the request's size.
    fn vertical(&self, font: &FontRequest<'_>) -> Vertical;

    /// A whole string's advance.
    ///
    /// Provided in terms of [`Metrics::advance`] so the two can never
    /// disagree, and overridable by a provider that can measure a run more
    /// cheaply than a character at a time.
    fn measure(&self, text: &str, font: &FontRequest<'_>) -> f64 {
        text.chars().map(|ch| self.advance(ch, font)).sum()
    }

    /// What UAX #9's rule P2 finds first in `text`, skipping what the text's
    /// own isolate initiators enclose.
    ///
    /// Asked for a block container whose `unicode-bidi` is `plaintext`
    /// (`css-writing-modes-3` §2.2), whose paragraphs take their direction
    /// from their text — and with it the side `start` aligns to
    /// (`css-text-3` §7.1). It is asked a box's text at a time, so an inline
    /// box's own isolate is skipped by the caller rather than found here.
    /// This crate has no `Bidi_Class` table and is not the place for one, so
    /// a provider that has UAX #9 answers; `None` — the default — is a
    /// provider that cannot, and the paragraph is then aligned by
    /// `direction` and resolved by whoever orders its lines.
    fn first_strong(&self, text: &str) -> Option<FirstStrong> {
        let _ = text;
        None
    }

    /// The [`Shaper`] this provider is, if it is one.
    ///
    /// `None` — the default — is a provider that measures a character at a
    /// time, which is every provider that existed before shaping did and is
    /// still the right answer for [`FixedPitch`] and for the fuzz target.
    ///
    /// This is a method on `Metrics` rather than a second parameter threaded
    /// through the layout entry point because **the choice has to be made in
    /// one place or it is not a rule**. A run is measured by the shaper or by
    /// `measure`, never both, and one accessor is what makes that checkable
    /// rather than a convention.
    fn shaper(&self) -> Option<&dyn Shaper> {
        None
    }
}

/// Every character the same width — a monospaced face, in effect.
///
/// **This is a real answer and not a stub**, and the distinction matters
/// because gap 31 is a plan about builds that produce something plausible
/// rather than something true. A book laid out through this is laid out
/// correctly *for a monospaced face*: the line breaks are where UAX #14 puts
/// them, the justification is right, the pagination is right, and the only
/// thing that is not the reader's is which face it was. That is what makes it
/// usable by the fuzz target and by every test in this crate that is about the
/// algorithm rather than about a font.
///
/// What it must not be is a **default**. `tinker-pdf-layout` has no
/// `impl Default for` anything that reaches for it, and a caller that wants a
/// real book supplies real metrics; milestone 9 is where the standard-14
/// widths arrive so that a book's pagination does not depend on whether a
/// provider was attached.
#[derive(Clone, Copy, Debug)]
pub struct FixedPitch {
    /// The advance of every character, as a fraction of the font size.
    pub advance: f64,
    /// The ascent, as a fraction of the font size.
    pub ascent: f64,
    /// The descent, as a fraction of the font size.
    pub descent: f64,
}

impl FixedPitch {
    /// Courier's proportions: a 600/1000 advance, and the ascender and
    /// descender the standard 14 publish.
    pub const COURIER: Self = Self {
        advance: 0.6,
        ascent: 0.629,
        descent: 0.157,
    };
}

impl Metrics for FixedPitch {
    fn advance(&self, _ch: char, font: &FontRequest<'_>) -> f64 {
        self.advance * font.size
    }

    fn vertical(&self, font: &FontRequest<'_>) -> Vertical {
        Vertical {
            ascent: self.ascent * font.size,
            descent: self.descent * font.size,
        }
    }
}
