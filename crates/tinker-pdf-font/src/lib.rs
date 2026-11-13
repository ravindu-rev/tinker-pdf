//! Font-program parsing: bytes in, metrics and outlines out; no PDF types.
//!
//! Feature documentation: `docs/features/fonts.md`.

#![forbid(unsafe_code)]

pub mod base14;
/// The twelve faces a `bundled-fonts` build carries (9.6.2.2's standard 14,
/// less the two symbolic ones).
#[cfg(feature = "bundled-fonts")]
pub mod bundled;
pub mod cbdt;
pub mod cff;
pub mod cff2;
pub mod cff_subset;
pub mod cmap;
pub mod colr;
pub mod cpal;
pub mod encoding;
pub mod facts;
pub mod glyf;
pub mod hint;
pub mod outline;
mod predefined;
pub mod sbix;
pub mod sfnt;
pub mod subset;
pub mod svg_table;
pub mod symbolic;
pub mod type1;
pub mod type42;
pub mod variation;
pub mod woff;

pub use base14::Standard14;
pub use cbdt::{Bearings, BitmapMetrics, BitmapStrike, CbdtGlyph, CbdtRefusal, ColourBitmaps};
pub use cff::Cff;
pub use cff2::Cff2;
pub use cff_subset::{subset_cff, subset_cff_with};
pub use cmap::CMap;
pub use colr::{ColourIndex, Colr, Layer, LayerColour};
pub use cpal::{Cpal, PaletteChoice, PaletteType, Rgba};
pub use encoding::{
    base_char, base_glyph_name, glyph_name_for_char, glyph_name_to_char, BaseEncoding, BuiltInCode,
    BuiltInEncoding,
};
pub use facts::{face_facts, FaceFacts};
pub use outline::{Outline, Segment};
pub use sbix::{Graphic, Sbix, SbixGlyph, SbixRefusal, SbixStrike, StrikeChoice};
pub use sfnt::{Sfnt, VerticalOrigins};
pub use subset::{glyphs_for, subset, subset_with, symbol_cmap, Reshape};
pub use svg_table::{SvgDocument, SvgRecord, SvgRefusal, SvgTable, SvgWarning, MAX_SVG_DOCUMENT};
pub use symbolic::{substitute_glyph, SymbolRoute};
pub use type1::{Blend, Instance, InstanceRefusal, Type1};
pub use type42::{Type42, Type42Refusal};
pub use woff::{packaging, Packaging, WoffError};
