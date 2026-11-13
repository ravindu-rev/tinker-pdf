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

pub use canvas::{
    Canvas, Color, InkTables, MaskKind, PixelFormat, PlateInk, SpotColour, Spots, TooManySpots,
    MAX_SPOT_PLATES,
};
pub use fill::{
    fill, fill_hard, fill_hard_with, fill_outline, fill_outline_hard, fill_with, FillScratch,
    HardEdge, Mask,
};
pub use fragments::Fragments;
pub use geom::{flatten, FillRule, Path, Point, Verb};
pub use image::{
    accumulate_image, accumulate_plate_image, draw_image, draw_plate_image, image_bounds,
    image_bounds_in, image_coverage, resample, sampling_for_size, Filter, ImageDraw, ImageSource,
    PlatePyramid, PlateSource, Pyramid, Sampling, SourceWindow, Transform, TwoColour,
    MAX_PLATE_CHANNELS,
};
pub use mesh::{
    draw_mesh, draw_mesh_over, draw_mesh_tints_over, MeshBuffer, MeshDraw, MeshTintBuffer,
    MeshTints,
};
pub use stripes::{fill_stripes, fill_stripes_with, StripeMask, SubpixelOrder};
pub use stroke::{
    adjusted_to_pixels, dash, stretches, stroke, stroke_mapped, stroke_mapped_with, stroke_with,
    whole_pixel_width, LineCap, LineJoin, StrokeStyle,
};
