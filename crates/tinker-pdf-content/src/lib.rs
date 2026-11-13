//! Content-stream tokenizer and interpreter, the `Device` trait seam, the
//! text-extraction device (which needs no rasterizer), the recording device
//! every later consumer of a page's content starts from, the filtering
//! device that drops classes of content or forces them black, and the codec
//! that writes a recording as a command stream and reads it back
//! (`docs/design/command-stream.md`).
//!
//! Feature documentation: `docs/features/content-and-text.md`.

#![forbid(unsafe_code)]

pub mod commands;
pub mod cull;
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
pub mod word_boxes;
pub mod words;

pub use cull::{Black, Cull, Culled};
pub use device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
pub use interpret::{
    interpret, interpret_counted, DeviceDefaults, FontSource, Form, FormDrawn, FormInvocation,
    FormRecording, Group, GroupSpace, Layer, MaskGroup, SoftMask, StateParameters, Type3Glyph,
    MAX_GROUP_RUNS, NESTED_RUN_COST, NESTED_WORK_FLOOR, NESTED_WORK_RATE,
};
pub use plain::{HyphenCounts, PlainText, PlainTextOptions};
pub use record::{Answers, Capture, Event, EventKind, MarkedScope, RecordingDevice};
pub use replay::{replay, Cursor, Replayed};
pub use search::{fold_diacritics, SearchOptions};
pub use serialize::{PageFrame, TextFormat, TextWriter, TEXT_FORMAT_VERSION};
pub use state::{
    BlackPointCompensation, BlendMode, ColorFamily, ColorSource, ColorantKind, ColorantTint,
    ColorantTints, GraphicsState, Halftone, LineCap, LineJoin, Matrix, RenderingIntent, Rgb, Rgb16,
    SpotFunction, StoredParameters, TextRenderMode, TextState, Transfer,
};
pub use text::{
    Quad, TextBlock, TextChar, TextDevice, TextLine, TextPage, TextWarning, WritingMode,
};
pub use tokenizer::{Token, Tokenizer};
pub use word_boxes::{
    LineDirection, WordBlock, WordBox, WordBoxes, WordLine, WordPage, WordParagraph, TSV_HEADER,
};
pub use words::{word_boundaries, TextWord};
