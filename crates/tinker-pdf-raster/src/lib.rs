//! Pure 2D rasterizer with zero PDF knowledge. Deterministic: fixed-point
//! coverage, bit-identical across platforms.
//!
//! Scope, design and exit criteria: `docs/plans/07-rasterizer.md`.

pub mod blend;
pub mod canvas;
pub mod fill;
pub mod geom;
pub mod image;
pub mod stroke;

pub use canvas::{Canvas, Color, PixelFormat};
pub use fill::{fill, Mask};
pub use geom::{flatten, FillRule, Path, Point, Verb};
pub use image::{draw_image, ImageDraw, ImageSource, Transform};
pub use stroke::{stroke, LineCap, LineJoin, StrokeStyle};
