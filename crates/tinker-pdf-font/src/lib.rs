//! Font-program parsing: bytes in, metrics and outlines out; no PDF types.
//!
//! Feature documentation: `docs/features/fonts.md`.

pub mod base14;
/// The twelve faces a `bundled-fonts` build carries (9.6.2.2's standard 14,
/// less the two symbolic ones).
#[cfg(feature = "bundled-fonts")]
pub mod bundled;
pub mod cff;
pub mod cmap;
pub mod encoding;
pub mod glyf;
pub mod outline;
mod predefined;
pub mod sfnt;
pub mod subset;
pub mod type1;

pub use base14::Standard14;
pub use cff::Cff;
pub use cmap::CMap;
pub use encoding::{
    base_char, base_glyph_name, glyph_name_for_char, glyph_name_to_char, BaseEncoding,
};
pub use outline::{Outline, Segment};
pub use sfnt::Sfnt;
pub use subset::{glyphs_for, subset};
pub use type1::Type1;
