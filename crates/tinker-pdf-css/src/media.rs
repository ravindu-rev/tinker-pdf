//! `mediaqueries-4`, evaluated against a plain struct — and the decision that
//! this engine is a **screen**.
//!
//! # Why the medium is `screen` and not `print`
//!
//! It is tempting to say `print`, because the output of this engine is a PDF.
//! That is an implementation fact about synthesis and not a statement about the
//! medium. An EPUB is authored for a **reading system**: EPUB 3.3 §8 and the
//! whole `rendition:*` vocabulary are about screens, reading systems paginate a
//! screen rather than a sheet, and a book whose `@media print` block hides its
//! navigation would lose it.
//!
//! It is recorded here, in the module header, so it is not flipped by whoever
//! next notices the output is a PDF. Flipping it is a one-word change with
//! book-wide consequences that no page comparison would attribute to it.
//!
//! # Why this is evaluated at all rather than skipped
//!
//! A build that ignores `@media` applies every rule inside every block, or
//! none. **Both are plausible and both are wrong**, and neither announces
//! itself: the first gives a book its print stylesheet on top of its screen
//! one, the second gives it neither. Real books use both — milestone 1's census
//! found `@media` in both producers' output.
//!
//! # What is evaluated, and what a query this build cannot read evaluates to
//!
//! Media types, `not` and `only`, `and`-joined feature tests, and a
//! comma-separated list. A feature this build does not know, or a construct it
//! cannot read — `or`, `not` inside a condition, a range form such as
//! `(400px < width)` — makes **that query** false, which is
//! `mediaqueries-4` §3.1's own answer for an unknown feature (*"an unknown
//! media feature evaluates to false"*). It is the safe direction: a query that
//! cannot be read does not apply rules the author scoped away.

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;

/// The medium a query is evaluated against.
///
/// Four values because `mediaqueries-4` §2.1 has four, and `all` is a value a
/// *query* may name rather than a value a context may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    /// A screen, which is what this engine is. See the module header.
    Screen,
    /// Paged output on a printer.
    Print,
    /// A speech synthesiser.
    Speech,
}

/// The state a media query is asked about.
///
/// A plain struct with no interior state, so the same context gives the same
/// answer for every sheet in a book — which is what makes the cascade a
/// function of the file and the [`crate::OpenBox`] rather than of the order the
/// sheets happened to arrive in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaContext {
    /// The page box's width in CSS pixels.
    pub width: f64,
    /// The page box's height in CSS pixels.
    pub height: f64,
    /// The medium. See the module header for why this engine sets `Screen`.
    pub media: MediaType,
    /// Bits per colour component. Zero means monochrome.
    pub color_bits: u8,
    /// Device pixels per CSS pixel.
    pub resolution: f64,
}

impl MediaContext {
    /// A page box in CSS pixels, on a screen, in colour, at 1 dppx.
    ///
    /// The two numbers are the caller's because the page box is the caller's:
    /// gap 31's decision 2 puts pagination at `open`, and `@media
    /// (max-width: 30em)` in a real book is asked about **that** box.
    pub fn screen(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            media: MediaType::Screen,
            color_bits: 8,
            resolution: 1.0,
        }
    }

    /// `portrait` unless the box is wider than it is tall. A square box is
    /// `portrait`, which is `mediaqueries-4` §4.5's own rule.
    fn orientation_is_portrait(&self) -> bool {
        self.height >= self.width
    }
}

/// Evaluates a media query list against a context.
///
/// An **empty** list is true: `@media { … }` has no query to fail, and
/// `mediaqueries-4` §2.1 says an empty query means `all`.
pub fn evaluate(prelude: &[ComponentValue], context: &MediaContext) -> bool {
    let significant: Vec<&ComponentValue> = prelude.iter().filter(|v| !v.is_whitespace()).collect();
    if significant.is_empty() {
        return true;
    }
    // A comma-separated list is a disjunction, and it is the *only* disjunction
    // this build reads — `or` inside a condition is unsupported and makes its
    // own query false, which is not the same thing.
    prelude
        .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
        .any(|query| evaluate_one(query, context))
}

fn evaluate_one(query: &[ComponentValue], context: &MediaContext) -> bool {
    let values: Vec<&ComponentValue> = query.iter().filter(|v| !v.is_whitespace()).collect();
    if values.is_empty() {
        return false;
    }
    let mut at = 0usize;
    let mut negated = false;
    let mut expect_and = false;
    if let Some(ComponentValue::Token(Token::Ident(word))) = values.first() {
        if word.eq_ignore_ascii_case("not") {
            negated = true;
            at = 1;
        } else if word.eq_ignore_ascii_case("only") {
            // `only` exists to hide a query from a 1999 parser and means
            // nothing to one that understands the grammar.
            at = 1;
        }
    }
    // An optional media type, then `and`-joined feature tests.
    if let Some(ComponentValue::Token(Token::Ident(word))) = values.get(at) {
        if !word.eq_ignore_ascii_case("and") {
            let matched = match word.to_ascii_lowercase().as_str() {
                "all" => true,
                "screen" => context.media == MediaType::Screen,
                "print" => context.media == MediaType::Print,
                "speech" | "aural" => context.media == MediaType::Speech,
                // §3's rule for an unknown media type: the query is false, and
                // `not unknown` is therefore true.
                _ => false,
            };
            if !matched {
                return negated;
            }
            at += 1;
            expect_and = true;
        }
    }
    while at < values.len() {
        if expect_and {
            match values.get(at) {
                Some(ComponentValue::Token(Token::Ident(word)))
                    if word.eq_ignore_ascii_case("and") =>
                {
                    at += 1;
                }
                // `or` and a bare juxtaposition are both unreadable here, and
                // an unreadable query is false rather than true.
                _ => return negated,
            }
        }
        let Some(value) = values.get(at) else {
            return negated;
        };
        let ComponentValue::Block {
            kind: BlockKind::Paren,
            values: inner,
        } = value
        else {
            return negated;
        };
        if !feature(inner, context) {
            return negated;
        }
        at += 1;
        expect_and = true;
    }
    !negated
}

/// One `(name)` or `(name: value)` feature test.
fn feature(inner: &[ComponentValue], context: &MediaContext) -> bool {
    let values: Vec<&ComponentValue> = inner.iter().filter(|v| !v.is_whitespace()).collect();
    let Some(ComponentValue::Token(Token::Ident(name))) = values.first() else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    // The boolean form: `(color)` is true when the value is non-zero.
    if values.len() == 1 {
        return match name.as_str() {
            "color" => context.color_bits > 0,
            "monochrome" => context.color_bits == 0,
            "width" => context.width != 0.0,
            "height" => context.height != 0.0,
            "orientation" => true,
            _ => false,
        };
    }
    if values.len() != 3 || !matches!(values[1], ComponentValue::Token(Token::Colon)) {
        // A range form such as `(400px < width)` is `mediaqueries-4` §2.4's
        // and this build does not read it. False, per §3.1's unknown rule.
        return false;
    }
    let value = values[2];
    match name.as_str() {
        "orientation" => match value {
            ComponentValue::Token(Token::Ident(word)) => {
                if word.eq_ignore_ascii_case("portrait") {
                    context.orientation_is_portrait()
                } else if word.eq_ignore_ascii_case("landscape") {
                    !context.orientation_is_portrait()
                } else {
                    false
                }
            }
            _ => false,
        },
        "color" | "min-color" | "max-color" | "monochrome" => {
            let Some(number) = integer(value) else {
                return false;
            };
            let actual = if name == "monochrome" {
                i64::from(u8::from(context.color_bits == 0))
            } else {
                i64::from(context.color_bits)
            };
            compare(&name, actual as f64, number as f64)
        }
        "width" | "min-width" | "max-width" => {
            let Some(px) = absolute_length(value) else {
                return false;
            };
            compare(&name, context.width, px)
        }
        "height" | "min-height" | "max-height" => {
            let Some(px) = absolute_length(value) else {
                return false;
            };
            compare(&name, context.height, px)
        }
        "resolution" | "min-resolution" | "max-resolution" => {
            let Some(dppx) = resolution(value) else {
                return false;
            };
            compare(&name, context.resolution, dppx)
        }
        _ => false,
    }
}

/// `min-`/`max-` decide the direction; a bare name is equality.
fn compare(name: &str, actual: f64, wanted: f64) -> bool {
    if let Some(rest) = name.strip_prefix("min-") {
        let _ = rest;
        actual >= wanted
    } else if let Some(rest) = name.strip_prefix("max-") {
        let _ = rest;
        actual <= wanted
    } else {
        actual == wanted
    }
}

fn integer(value: &ComponentValue) -> Option<i64> {
    match value {
        ComponentValue::Token(Token::Number {
            value,
            integer: true,
        }) => Some(*value as i64),
        _ => None,
    }
}

/// An absolute length in CSS pixels. A media query cannot use `em`, because
/// there is no element for it to be relative to — except on the *root*, where
/// `mediaqueries-4` §1.3 says font-relative units refer to the initial value.
/// This build has no initial font size until the cascade runs, so `em` here is
/// resolved against 16px, which is the CSS initial value and is what every
/// browser uses for the same reason.
fn absolute_length(value: &ComponentValue) -> Option<f64> {
    match value {
        ComponentValue::Token(Token::Number { value, .. }) if *value == 0.0 => Some(0.0),
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            crate::property::absolute_px(*value, unit, 16.0, 16.0)
        }
        _ => None,
    }
}

fn resolution(value: &ComponentValue) -> Option<f64> {
    match value {
        ComponentValue::Token(Token::Dimension { value, unit }) => {
            match unit.to_ascii_lowercase().as_str() {
                "dppx" | "x" => Some(*value),
                "dpi" => Some(*value / 96.0),
                "dpcm" => Some(*value / 96.0 * 2.54),
                _ => None,
            }
        }
        _ => None,
    }
}
