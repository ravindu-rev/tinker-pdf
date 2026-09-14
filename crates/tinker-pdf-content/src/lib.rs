//! Content-stream tokenizer and interpreter, the `Device` trait seam, the
//! text-extraction device (which needs no rasterizer) and the recording
//! device every later consumer of a page's content starts from.
//!
//! Feature documentation: `docs/features/content-and-text.md`.

pub mod device;
pub mod interpret;
pub mod record;
pub mod state;
pub mod text;
pub mod tokenizer;

pub use device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
pub use interpret::{interpret, FontSource, Form, Group, GroupSpace, Layer, MaskGroup, SoftMask};
pub use record::{Answers, Capture, Event, EventKind, MarkedScope, RecordingDevice};
pub use state::{
    BlendMode, GraphicsState, LineCap, LineJoin, Matrix, Rgb, TextRenderMode, TextState,
};
pub use text::{
    Quad, TextBlock, TextChar, TextDevice, TextLine, TextPage, TextWarning, WritingMode,
};
pub use tokenizer::{Token, Tokenizer};
