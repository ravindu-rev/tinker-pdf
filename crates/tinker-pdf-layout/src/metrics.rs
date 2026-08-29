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

use tinker_pdf_css::property::{FontFamily, FontStyle};

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
