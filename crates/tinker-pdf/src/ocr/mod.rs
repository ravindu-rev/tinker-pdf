//! OCR: the tier's facade half (`docs/design/ocr.md`, `docs/features/ocr.md`).
//!
//! The import seam (ROADMAP row OC-01) is here: [`import`] reads the words an
//! outside recogniser wrote in hOCR, ALTO or PAGE XML into FT-24's word-box
//! model, and [`add_text_layer`] writes an imported page over a PDF page as
//! an invisible text layer. Everything that knows what a page is belongs to
//! the facade, so the stages that need a document live in this module and the
//! ones that take pixels in the tier's own crate.

pub mod import;
pub mod layer;

pub use import::{
    import, Direction, Extent, ImportError, ImportFormat, ImportWarning, Imported, ImportedPage,
    LineSetting, Located, Unit, ALTO_NAMESPACES, MAX_IMPORTED_WORDS, PAGE_NAMESPACE_PREFIX,
};
pub use layer::{add_text_layer, LayerError, LayerWarning, Layered, MAX_LAYER_CHARACTERS};
