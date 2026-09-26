//! Content-stream tokenizer and interpreter, the `Device` trait seam, the
//! text-extraction device (which needs no rasterizer) and the recording
//! device every later consumer of a page's content starts from.
//!
//! Feature documentation: `docs/features/content-and-text.md`.

pub mod device;
pub mod interpret;
pub mod plain;
pub mod record;
pub mod search;
pub mod state;
pub mod text;
pub mod tokenizer;
pub mod words;

pub use device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
pub use interpret::{interpret, FontSource, Form, Group, GroupSpace, Layer, MaskGroup, SoftMask};
pub use plain::{HyphenCounts, PlainText, PlainTextOptions};
pub use record::{Answers, Capture, Event, EventKind, MarkedScope, RecordingDevice};
pub use search::{fold_diacritics, SearchOptions};
pub use state::{
    BlendMode, GraphicsState, LineCap, LineJoin, Matrix, Rgb, TextRenderMode, TextState,
};
pub use text::{
    Quad, TextBlock, TextChar, TextDevice, TextLine, TextPage, TextWarning, WritingMode,
};
pub use tokenizer::{Token, Tokenizer};
pub use words::{word_boundaries, TextWord};
