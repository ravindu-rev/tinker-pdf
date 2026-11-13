//! Pure 2D rasterizer with zero PDF knowledge. Deterministic: fixed-point
//! coverage, bit-identical across platforms.
//!
//! Feature documentation: `docs/features/rasterizer.md`.

#![forbid(unsafe_code)]

pub mod blend;
pub mod canvas;
pub mod fill;
#[cfg(test)]
mod fill_reference;
pub mod fragments;
pub mod geom;
pub mod image;
pub mod mesh;
pub mod stripes;
pub mod stroke;
pub mod toolbox;

pub use canvas::{Canvas, Color, MaskKind, PixelFormat};
pub use fill::{fill, fill_outline, fill_with, FillScratch, Mask};
pub use fragments::Fragments;
pub use geom::{flatten, FillRule, Path, Point, Verb};
pub use image::{
    accumulate_image, draw_image, image_bounds, image_bounds_in, image_coverage, Filter, ImageDraw,
    ImageSource, Pyramid, Sampling, Transform, TwoColour,
};
pub use mesh::{draw_mesh, draw_mesh_over, MeshBuffer, MeshDraw};
pub use stripes::{fill_stripes, fill_stripes_with, StripeMask, SubpixelOrder};
pub use stroke::{
    dash, stretches, stroke, stroke_mapped, stroke_mapped_with, stroke_with, LineCap, LineJoin,
    StrokeStyle,
};
