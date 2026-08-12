//! Content-stream tokenizer and interpreter, the `Device` trait seam, and
//! the text-extraction device (which needs no rasterizer).
//!
//! Scope, design and exit criteria: `docs/plans/06-content-and-text.md`.

pub mod device;
pub mod interpret;
pub mod state;
pub mod text;
pub mod tokenizer;

pub use device::{Device, Glyph, ImageRef, PathSegment};
pub use interpret::{interpret, FontSource, Form, Layer};
pub use state::{
    BlendMode, GraphicsState, LineCap, LineJoin, Matrix, Rgb, TextRenderMode, TextState,
};
pub use text::{
    Quad, TextBlock, TextChar, TextDevice, TextLine, TextPage, TextWarning, WritingMode,
};
pub use tokenizer::{Token, Tokenizer};
