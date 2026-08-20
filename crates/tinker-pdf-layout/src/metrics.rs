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
