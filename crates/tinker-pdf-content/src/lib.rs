//! Content-stream tokenizer and interpreter, the `Device` trait seam, the
//! text-extraction device (which needs no rasterizer) and the recording
//! device every later consumer of a page's content starts from.
//!
//! Feature documentation: `docs/features/content-and-text.md`.

#![forbid(unsafe_code)]

pub mod device;
pub mod interpret;
pub mod plain;
pub mod record;
pub mod replay;
pub mod search;
pub mod serialize;
pub mod state;
pub mod text;
pub mod tokenizer;
pub mod words;

pub use device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
pub use interpret::{
    interpret, DeviceDefaults, FontSource, Form, Group, GroupSpace, Layer, MaskGroup, SoftMask,
    StateParameters, MAX_GROUP_RUNS,
};
pub use plain::{HyphenCounts, PlainText, PlainTextOptions};
pub use record::{Answers, Capture, Event, EventKind, MarkedScope, RecordingDevice};
pub use replay::{replay, Replayed};
pub use search::{fold_diacritics, SearchOptions};
pub use serialize::{PageFrame, TextFormat, TextWriter, TEXT_FORMAT_VERSION};
pub use state::{
    BlackPointCompensation, BlendMode, GraphicsState, Halftone, LineCap, LineJoin, Matrix,
    RenderingIntent, Rgb, SpotFunction, StoredParameters, TextRenderMode, TextState, Transfer,
};
pub use text::{
    Quad, TextBlock, TextChar, TextDevice, TextLine, TextPage, TextWarning, WritingMode,
};
pub use tokenizer::{Token, Tokenizer};
pub use words::{word_boundaries, TextWord};
