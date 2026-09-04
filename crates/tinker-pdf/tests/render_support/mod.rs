//! What the render tiers of [`docs/design/render-verification.md`] share.
//!
//! Four test binaries draw a page and then say something about the pixels:
//! `render_analytic.rs` compares them against a formula, `render_differential.rs`
//! compares two documents that must draw the same picture, `render_goldens.rs`
//! compares them against a reviewed raster, and `writer_graphics.rs` compares
//! the writer's output against a hand-written twin. All four had their own copy
//! of `fn render`, and two had their own copy of the comparator.
//!
//! Only the helpers those four need live here. The twenty-odd other test
//! binaries that open a document and look at a pixel keep their own copies
//! deliberately: pulling them in would be a diff across the whole `tests/`
//! directory for no property gained, and a fixture whose `render` is three
//! files away is harder to read than one whose `render` is at the top.
//!
//! # Why the allow
//!
//! Each binary compiles its own copy of this module and each uses a different
//! subset — `render_differential.rs` never evaluates a formula, so it has no
//! use for [`byte`] or [`centre`]; `writer_graphics.rs` draws on a 60-point
//! page and has no use for [`SIZE`]. Every one of those reads as dead code in
//! the binaries that do not use it.

#![allow(
    dead_code,
    reason = "shared by four test binaries; each uses a different subset"
)]

use tinker_pdf::{Bitmap, Document, RenderOptions};

/// The page an analytic fixture draws into, in points and in pixels alike.
///
/// Sixteen, so that a whole-page per-pixel comparison is 256 evaluations of a
/// formula rather than a million, and so that the default 72 dpi puts one pixel
/// on one point and no scale factor stands between the geometry and the
/// expectation.
pub const SIZE: f64 = 16.0;

/// The first page of a document, rendered with the default options.
pub fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("the document opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// One pixel's three colour components.
pub fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).expect("three components");
    (p[0], p[1], p[2])
}

/// A component, as the page carries it.
pub fn byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Where a pixel is sampled, in the page's own space, on a [`SIZE`] page.
///
/// `y` is flipped because a bitmap's rows run down and PDF user space runs up,
/// which is the one conversion every expectation needs and the one a fixture
/// that got it wrong would still find plausible — a gradient upside down is a
/// gradient.
pub fn centre(x: u32, y: u32) -> (f64, f64) {
    (f64::from(x) + 0.5, SIZE - f64::from(y) - 0.5)
}

/// Pixels that are not the white the page started as.
///
/// The same body as `determinism.rs`'s, and for the same reason: a fixture that
/// has stopped drawing must fail rather than become a baseline. That file keeps
/// its own copy because it is the one binary the wasm and Linux legs run on
/// their own, and a target leg that fails to build a support module reads as a
/// determinism failure.
pub fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

/// Asserts two documents draw the same picture, pixel for pixel.
///
/// A count and an offset rather than a boolean, because the two numbers say
/// which kind of divergence it is: a handful of bytes at a high offset is an
/// edge, and half the buffer from byte zero is a document that drew something
/// else entirely.
pub fn same_picture(built: Bitmap, written: Bitmap, what: &str) {
    assert_eq!(
        (built.width, built.height),
        (written.width, written.height),
        "{what}: different sizes"
    );
    if built.data == written.data {
        return;
    }
    let differing = built
        .data
        .iter()
        .zip(written.data.iter())
        .filter(|(a, b)| a != b)
        .count();
    let first = built
        .data
        .iter()
        .zip(written.data.iter())
        .position(|(a, b)| a != b)
        .unwrap_or(0);
    panic!(
        "{what}: the two documents drew different pictures -- {differing} of \
         {} bytes differ, first at {first} ({} against {})",
        built.data.len(),
        built.data.get(first).copied().unwrap_or(0),
        written.data.get(first).copied().unwrap_or(0),
    );
}
