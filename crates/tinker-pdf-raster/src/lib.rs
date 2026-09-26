//! Pure 2D rasterizer with zero PDF knowledge. Deterministic: fixed-point
//! coverage, bit-identical across platforms.
//!
//! Feature documentation: `docs/features/rasterizer.md`.

pub mod blend;
pub mod canvas;
pub mod fill;
pub mod fragments;
pub mod geom;
pub mod image;
pub mod mesh;
pub mod stroke;

pub use canvas::{Canvas, Color, MaskKind, PixelFormat};
pub use fill::{fill, Mask};
pub use fragments::Fragments;
pub use geom::{flatten, FillRule, Path, Point, Verb};
pub use image::{
    accumulate_image, draw_image, image_bounds, image_coverage, Filter, ImageDraw, ImageSource,
    Pyramid, Sampling, Transform,
};
pub use mesh::{draw_mesh, MeshBuffer, MeshDraw};
pub use stroke::{stroke, LineCap, LineJoin, StrokeStyle};
