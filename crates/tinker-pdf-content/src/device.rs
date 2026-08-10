//! The `Device` seam (ruling 7).
//!
//! Everything that consumes a content stream implements this: the text device
//! here, the rasterizing device in `tinker-pdf-render`, and any future one.
//! The interpreter never knows which it is talking to, which is exactly why
//! text extraction needs no rasterizer and Checkpoint A can exist.

use crate::state::{GraphicsState, Matrix};

/// One glyph, as the interpreter resolved it.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// The character code from the string.
    pub code: u32,
    /// The text this code stands for; empty when nothing maps it.
    pub text: String,
    /// The transform from glyph space to device space, including the font
    /// size, horizontal scaling and rise.
    pub transform: Matrix,
    /// The advance in text space, before the transform.
    pub advance: f64,
    /// The font size in effect.
    pub size: f64,
    /// Whether the font's writing mode is vertical.
    pub vertical: bool,
    /// An identifier for the font, stable within one interpretation.
    pub font_id: u64,
}

/// What a content stream asks to be drawn.
///
/// Every method has a default that does nothing, so a device implements only
/// what it cares about — the text device ignores paths, the rasterizer does
/// not.
pub trait Device {
    /// A glyph was shown.
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        let _ = (glyph, state);
    }

    /// A text object began (`BT`).
    fn begin_text(&mut self) {}

    /// A text object ended (`ET`).
    fn end_text(&mut self) {}

    /// A path was filled. `even_odd` selects the rule (8.5.3.3).
    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let _ = (path, state, even_odd);
    }

    /// A path was stroked.
    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        let _ = (path, state);
    }

    /// A path was added to the clipping path by `W` or `W*` (8.5.4).
    ///
    /// Arrives after the painting operator that ended the path, because that
    /// is when the clip takes effect.
    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let _ = (path, state, even_odd);
    }

    /// `q`: the graphics state was saved.
    ///
    /// A device holding state the interpreter does not model — a clip mask,
    /// say — saves it here.
    fn save_state(&mut self) {}

    /// `Q`: the graphics state was restored.
    fn restore_state(&mut self) {}

    /// An image XObject was drawn into the unit square of the current
    /// transform.
    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        let _ = (image, state);
    }

    /// `sh`: a named shading was painted over the current clip (8.7.4.2).
    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        let _ = (name, state);
    }

    /// A form XObject is about to be interpreted; returning false skips it.
    fn begin_form(&mut self, id: u64) -> bool {
        let _ = id;
        true
    }

    /// A form XObject finished.
    fn end_form(&mut self, id: u64) {
        let _ = id;
    }

    /// Whether interpretation should stop — a cancellation hook checked
    /// between operators.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// One piece of a path, in device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    /// Start a new subpath.
    MoveTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A straight line.
    LineTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A cubic Bézier.
    CurveTo {
        /// First control point.
        x1: f64,
        /// First control point.
        y1: f64,
        /// Second control point.
        x2: f64,
        /// Second control point.
        y2: f64,
        /// Destination.
        x3: f64,
        /// Destination.
        y3: f64,
    },
    /// Close the current subpath.
    Close,
}

/// An image the interpreter reached, identified rather than decoded — decoding
/// is the rasterizing device's business.
#[derive(Clone, Debug)]
pub struct ImageRef {
    /// The resource name, or empty for an inline image.
    pub name: Vec<u8>,
    /// Whether the image was inline (8.9.7).
    pub inline: bool,
}
