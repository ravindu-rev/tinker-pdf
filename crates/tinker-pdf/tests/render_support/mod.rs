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

use tinker_pdf::{
    Bitmap, BlendMode, DeviceSpace, Document, DocumentBuilder, ExtGState, Function, ImageData,
    RenderOptions, Shading,
};

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
/// A fixture that has stopped drawing must fail rather than become a baseline:
/// `determinism.rs`'s `text` fixture named a standard-14 face, embedded no
/// outlines, rendered a blank page and committed the hash of one, and passed on
/// every target for months. Every tier here holds a floor against that for the
/// same reason, so the counter belongs in one place.
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

// ---- the analytic pages -----------------------------------------------------
//
// Four pages whose right answer is a formula. They live here rather than inside
// the `#[test]` bodies that adjudicate them because `determinism.rs` fingerprints
// **these same bytes**: a fixture the formula checks and a fixture the hash
// covers have to be one document, or the enrolment is a claim about a page
// nothing evaluated.

/// The axial fixture's two stops.
pub const AXIAL_STOPS: ([f64; 3], [f64; 3]) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

/// Its axis: the page's bottom-left corner to its top-right.
///
/// Diagonal on purpose. An axis along `x` makes `t` a function of one
/// coordinate, so a build that projected onto the wrong axis, or that used
/// distance instead of projection, draws the same picture — every one of those
/// mistakes needs a slope to show.
pub const AXIAL_AXIS: [f64; 4] = [0.0, 0.0, SIZE, SIZE];

fn ramp(c0: [f64; 3], c1: [f64; 3]) -> Function {
    Function::Exponential {
        domain: [0.0, 1.0],
        c0: c0.to_vec(),
        c1: c1.to_vec(),
        n: 1.0,
    }
}

fn flooded(shading: &Shading) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(b"Sh0", shading));
    builder.add_page(SIZE, SIZE, |page| {
        page.raw(format!("q 0 0 {SIZE} {SIZE} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    builder.finish()
}

/// A page flooded by one axial shading (8.7.4.5.3).
pub fn axial_page() -> Vec<u8> {
    let (c0, c1) = AXIAL_STOPS;
    flooded(&Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: AXIAL_AXIS,
        function: ramp(c0, c1),
        extend: (true, true),
    })
}

/// The radial fixture's stops, centre and radii.
pub const RADIAL_STOPS: ([f64; 3], [f64; 3]) = ([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);
/// Concentric, so `t` is a distance rather than a projection.
pub const RADIAL_CENTRE: (f64, f64) = (SIZE / 2.0, SIZE / 2.0);
/// The inner radius.
pub const RADIAL_R0: f64 = 1.0;
/// The outer radius.
pub const RADIAL_R1: f64 = 7.0;

/// A page flooded by one radial shading (8.7.4.5.4).
pub fn radial_page() -> Vec<u8> {
    let (c0, c1) = RADIAL_STOPS;
    flooded(&Shading::Radial {
        color_space: DeviceSpace::Rgb,
        coords: [
            RADIAL_CENTRE.0,
            RADIAL_CENTRE.1,
            RADIAL_R0,
            RADIAL_CENTRE.0,
            RADIAL_CENTRE.1,
            RADIAL_R1,
        ],
        function: ramp(c0, c1),
        extend: (true, true),
    })
}

/// The image fixture's samples: a two-by-two checker, row-major from the top,
/// which is the order 8.9.5.2 states.
pub const CHECKER: [[u8; 3]; 4] = [[0, 0, 0], [255, 255, 255], [255, 255, 255], [0, 0, 0]];

/// A page holding [`CHECKER`] scaled by a whole number.
pub fn image_page() -> Vec<u8> {
    let data: Vec<u8> = CHECKER.iter().flatten().copied().collect();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &data,
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.image(b"Im0", 0.0, 0.0, SIZE, SIZE);
    });
    builder.finish()
}

/// 11.3.5.2's twelve separable modes, which are the ones with a closed form
/// over one channel. The four non-separable ones need all three at once and are
/// a different claim.
pub const MODES: [BlendMode; 12] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::HardLight,
    BlendMode::SoftLight,
    BlendMode::Difference,
    BlendMode::Exclusion,
];

/// One cell of the blend grid, in points.
pub const BLEND_CELL: f64 = 12.0;
/// The two backdrops a cell carries, left half and right half.
pub const BLEND_BACKDROPS: [f64; 2] = [0.25, 0.8];
/// The source laid over both of them.
pub const BLEND_SOURCE: f64 = 0.5;

/// Where a cell's own origin is, in page space, for mode `index`.
///
/// Four columns and three rows, filled left to right and **top to bottom**, so
/// the picture reads in the order [`MODES`] is written in rather than in the
/// order PDF's upward axis would give.
pub fn blend_cell_origin(index: usize) -> (f64, f64) {
    let (column, row) = (index % 4, index / 4);
    (
        column as f64 * BLEND_CELL,
        (2 - row) as f64 * BLEND_CELL,
    )
}

/// One page carrying all twelve separable modes over two backdrops each.
///
/// The per-mode fixtures build one document apiece and sample one pixel, which
/// is the right shape for an expression and the wrong shape for a fingerprint:
/// a hundred and eight documents cannot be enrolled and one of them would not
/// be worth enrolling. This is the same arithmetic laid out as a picture, so a
/// single hash covers every mode.
pub fn blend_grid_page() -> Vec<u8> {
    let width = 4.0 * BLEND_CELL;
    let height = 3.0 * BLEND_CELL;
    let mut builder = DocumentBuilder::new();
    for (index, mode) in MODES.iter().enumerate() {
        assert!(builder.add_ext_gstate(
            format!("GS{index}").as_bytes(),
            &ExtGState {
                blend_mode: Some(*mode),
                ..ExtGState::default()
            }
        ));
    }
    builder.add_page(width, height, |page| {
        for index in 0..MODES.len() {
            let (x, y) = blend_cell_origin(index);
            let half = BLEND_CELL / 2.0;
            for (which, backdrop) in BLEND_BACKDROPS.iter().enumerate() {
                let left = x + which as f64 * half;
                page.raw(b"q ");
                page.set_fill_rgb(*backdrop, *backdrop, *backdrop);
                page.raw(format!("{left} {y} {half} {BLEND_CELL} re f").as_bytes());
                assert!(page.set_ext_gstate(format!("GS{index}").as_bytes()));
                page.set_fill_rgb(BLEND_SOURCE, BLEND_SOURCE, BLEND_SOURCE);
                page.raw(format!("{left} {y} {half} {BLEND_CELL} re f").as_bytes());
                page.raw(b"Q ");
            }
        }
    });
    builder.finish()
}
