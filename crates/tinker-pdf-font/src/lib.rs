//! Font-program parsing: bytes in, metrics and outlines out; no PDF types.
//!
//! Scope, design and exit criteria: `docs/plans/05-fonts.md`.

pub mod base14;
pub mod cmap;
pub mod encoding;

pub use base14::Standard14;
pub use cmap::CMap;
pub use encoding::{base_char, glyph_name_to_char, BaseEncoding};
