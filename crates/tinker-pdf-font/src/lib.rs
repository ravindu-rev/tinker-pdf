//! Font-program parsing: bytes in, metrics and outlines out; no PDF types.
//!
//! Feature documentation: `docs/features/fonts.md`.

#![forbid(unsafe_code)]

pub mod base14;
/// The twelve faces a `bundled-fonts` build carries (9.6.2.2's standard 14,
/// less the two symbolic ones).
#[cfg(feature = "bundled-fonts")]
pub mod bundled;
pub mod cff;
pub mod cff2;
pub mod cff_subset;
pub mod cmap;
pub mod colr;
pub mod cpal;
pub mod encoding;
pub mod glyf;
pub mod outline;
mod predefined;
pub mod sfnt;
pub mod subset;
pub mod symbolic;
pub mod type1;
pub mod variation;
pub mod woff;

pub use base14::Standard14;
pub use cff::Cff;
pub use cff2::Cff2;
pub use cff_subset::subset_cff;
pub use cmap::CMap;
pub use colr::{ColourIndex, Colr, Layer, LayerColour};
pub use cpal::{Cpal, PaletteChoice, PaletteType, Rgba};
pub use encoding::{
    base_char, base_glyph_name, glyph_name_for_char, glyph_name_to_char, BaseEncoding, BuiltInCode,
    BuiltInEncoding,
};
pub use outline::{Outline, Segment};
pub use sfnt::{Sfnt, VerticalOrigins};
pub use subset::{glyphs_for, subset};
pub use symbolic::{substitute_glyph, SymbolRoute};
pub use type1::Type1;
pub use woff::{packaging, Packaging, WoffError};
