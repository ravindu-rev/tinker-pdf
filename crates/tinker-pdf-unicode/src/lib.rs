//! Unicode text segmentation: where a line may break, and what a character
//! is, over the vendored Unicode Character Database.
//!
//! Feature documentation: `docs/features/epub.md` (line breaking) and
//! `docs/features/editing.md` (free text appearances).
//!
//! **A leaf with nothing under it.** Text and plain parameters in, byte
//! offsets and property values out: no PDF, no CSS, no font. Two crates break
//! text by it — `tinker-pdf-layout`, which lays out a book's lines, and
//! `tinker-pdf-cos`, which wraps a free text annotation's `/Contents` inside
//! its box — and neither may depend on the other, which is why the
//! algorithms are a crate of their own rather than a module of either
//! (`xtask/src/main.rs`'s sixteenth amendment).
//!
//! # The three modules
//!
//! - [`uax14`]: UAX #14, the Unicode line breaking algorithm, with
//!   `css-text-3` §5's tailorings, held to every case of Unicode's own
//!   `LineBreakTest.txt` (`tests/uax14_conformance.rs`). [`uax14::decided`]
//!   answers for the start of a longer text, so a caller that breaks only as
//!   many lines as it takes reads only as much of its text as those lines.
//! - [`grapheme`]: UAX #29's extended grapheme clusters and the typographic
//!   character units no line breaks inside (`css-text-3` §5.5), held to every
//!   case of `GraphemeBreakTest.txt` (`tests/grapheme_conformance.rs`).
//!   [`grapheme::Units`] walks them lazily.
//! - [`unicode`]: the character properties both read, and the ones
//!   `text-transform` and the facade's Markdown reader read from the same
//!   files — the full case mappings, `Cased`, `Case_Ignorable`, `Lowercase`,
//!   and the general categories they ask about — compiled by `build.rs` into
//!   static range tables, so nothing under `data/` is opened at run time.
//!
//! # Using it
//!
//! ```
//! use tinker_pdf_unicode::grapheme::units;
//! use tinker_pdf_unicode::uax14::{opportunities, Tailoring};
//!
//! let text = "e\u{301}t\u{e9} 2026";
//! let breaks: Vec<usize> = opportunities(text, Tailoring::UAX14)
//!     .iter()
//!     .map(|o| o.at)
//!     .collect();
//! assert_eq!(breaks, [7, text.len()]);
//! assert_eq!(units(text), [0, 3, 4, 6, 7, 8, 9, 10, 11]);
//! ```

#![forbid(unsafe_code)]

pub mod grapheme;
pub mod uax14;
pub mod unicode;
