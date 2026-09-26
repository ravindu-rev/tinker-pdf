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
    (column as f64 * BLEND_CELL, (2 - row) as f64 * BLEND_CELL)
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

// ---- the face this repository writes for itself -----------------------------
//
// Moved here from `determinism.rs` when the goldens arrived: the text family's
// golden needs a face for the same reason that file's `text` fixture does, and
// two synthetic faces in one test directory would be one too many. The bytes
// are unchanged, which the `text` fingerprint proves rather than asserts.

/// One outline point: its position in font units, and whether it lies on the
/// curve.
type Point = (i16, i16, bool);
/// A closed contour.
type Contour = &'static [Point];
/// One glyph, as its contours.
type Shape = &'static [Contour];

/// The six outlines of [`curvy_font`], glyph 1 upward; glyph 0 is `.notdef`
/// and empty.
///
/// Chosen for what they make the rasteriser do, not for looking like letters.
/// A box outline — four axis-aligned edges — exercises almost nothing: every
/// span is full or empty and no coverage value between 0 and 1 ever arises.
/// These do, in six different ways:
///
/// 1. a chevron: long diagonals meeting at a thin apex, with a notch;
/// 2. a ring: two curved contours wound in opposite directions, so the hole
///    depends on the fill rule as well as on the arithmetic;
/// 3. a wedge: one quadratic spanning the whole em against two straight
///    edges, which is flattening tolerance on its own;
/// 4. a slash: a parallelogram at a shallow angle, nothing but partial
///    coverage down both sides;
/// 5. a ribbon: consecutive off-curve points, so the implied on-curve
///    midpoint rule decides where the curve actually goes;
/// 6. a dot over a stem: two contours of very different size in one glyph,
///    the small one curved and the thin one diagonal.
const SHAPES: &[Shape] = &[
    // 1. Chevron.
    &[&[
        (20, 0, true),
        (240, 700, true),
        (320, 700, true),
        (540, 0, true),
        (420, 0, true),
        (280, 380, true),
        (140, 0, true),
    ]],
    // 2. Ring: the outer contour runs clockwise and the inner one
    // anticlockwise, which is what makes the middle a hole.
    &[
        &[
            (280, 630, true),
            (560, 630, false),
            (560, 350, true),
            (560, 70, false),
            (280, 70, true),
            (0, 70, false),
            (0, 350, true),
            (0, 630, false),
        ],
        &[
            (280, 500, true),
            (130, 500, false),
            (130, 350, true),
            (130, 200, false),
            (280, 200, true),
            (430, 200, false),
            (430, 350, true),
            (430, 500, false),
        ],
    ],
    // 3. Wedge.
    &[&[
        (0, 0, true),
        (560, 0, true),
        (560, 700, false),
        (0, 700, true),
    ]],
    // 4. Slash.
    &[&[
        (0, 0, true),
        (200, 0, true),
        (560, 700, true),
        (360, 700, true),
    ]],
    // 5. Ribbon. Each edge is two quadratics meeting at a point the font
    // never states — halfway between the two off-curve points.
    &[&[
        (40, 0, true),
        (40, 340, false),
        (520, 360, false),
        (520, 700, true),
        (400, 700, true),
        (360, 300, false),
        (200, 260, false),
        (160, 0, true),
    ]],
    // 6. Dot over a stem.
    &[
        &[
            (140, 680, true),
            (260, 680, false),
            (260, 560, true),
            (260, 440, false),
            (140, 440, true),
            (20, 440, false),
            (20, 560, true),
            (20, 680, false),
        ],
        &[
            (240, 0, true),
            (380, 0, true),
            (560, 420, true),
            (420, 420, true),
        ],
    ],
];

/// How many glyphs the face has, `.notdef` included.
const GLYPHS: u16 = SHAPES.len() as u16 + 1;
/// The advance of every shape, in font units, and of the space.
const ADVANCE: u16 = 640;
const SPACE_ADVANCE: u16 = 320;

/// Which glyph a character code selects: the space is empty, and every other
/// printable code takes the six shapes in turn.
fn glyph_for(code: u16) -> u16 {
    if code == 0x20 {
        return 0;
    }
    1 + (code - 0x21) % (GLYPHS - 1)
}

/// A synthetic TrueType face of curves and diagonals.
///
/// Built here rather than read from the system, because ruling 4 is a claim
/// about every target — including `wasm32-unknown-unknown`, where there are
/// no font directories to read — and because a repository that carries no
/// font carries nobody's licence.
pub fn curvy_font() -> Vec<u8> {
    // Glyph 0 is `.notdef` and has no outline: an empty `loca` range, which
    // is how a font says "no shape" (a repeated offset, rather than a zero
    // one).
    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0];
    for shape in SHAPES {
        glyf.extend_from_slice(&glyph_data(shape));
        loca.push(glyf.len() as u32);
    }

    let mut loca_bytes = Vec::new();
    for offset in &loca {
        loca_bytes.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca offsets

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&GLYPHS.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&GLYPHS.to_be_bytes()); // numberOfHMetrics

    // The advances the builder reads out to write /Widths with, so the text
    // is spaced by the same numbers the outlines are drawn from.
    let mut hmtx = Vec::new();
    for glyph in 0..GLYPHS {
        let advance = if glyph == 0 { SPACE_ADVANCE } else { ADVANCE };
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
    }

    let cmap = cmap();
    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // search hints, unread

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// One glyph's `glyf` entry.
fn glyph_data(shape: Shape) -> Vec<u8> {
    let points: Vec<Point> = shape.iter().flat_map(|c| c.iter().copied()).collect();
    let xs = || points.iter().map(|p| p.0);
    let ys = || points.iter().map(|p| p.1);

    let mut out = Vec::new();
    out.extend_from_slice(&(shape.len() as i16).to_be_bytes());
    out.extend_from_slice(&xs().min().unwrap_or(0).to_be_bytes()); // xMin
    out.extend_from_slice(&ys().min().unwrap_or(0).to_be_bytes()); // yMin
    out.extend_from_slice(&xs().max().unwrap_or(0).to_be_bytes()); // xMax
    out.extend_from_slice(&ys().max().unwrap_or(0).to_be_bytes()); // yMax

    let mut end = 0usize;
    for contour in shape {
        end += contour.len();
        out.extend_from_slice(&((end - 1) as u16).to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions

    // Bit 0 is the on-curve flag. None of the short-coordinate or repeat bits
    // are set, so every delta below is a signed 16-bit word — larger than a
    // real font would write, and far easier to read.
    for (_, _, on_curve) in &points {
        out.push(u8::from(*on_curve));
    }
    let mut previous = 0i16;
    for (x, _, _) in &points {
        out.extend_from_slice(&(x - previous).to_be_bytes());
        previous = *x;
    }
    let mut previous = 0i16;
    for (_, y, _) in &points {
        out.extend_from_slice(&(y - previous).to_be_bytes());
        previous = *y;
    }

    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// A `cmap` covering printable ASCII (9.6.6.4).
///
/// Format 4 through its `idRangeOffset` branch — the one where the segment
/// points into a glyph index array at an offset measured from its own slot,
/// which is the awkward part of the format and the part a real font uses.
/// Going through the array rather than a plain delta is what lets every
/// character in the fixture's text draw, out of a face with six shapes.
fn cmap() -> Vec<u8> {
    const FIRST: u16 = 0x20;
    const LAST: u16 = 0x7E;
    // The real segment, and the terminating one at 0xFFFF the format requires.
    const SEGMENTS: u16 = 2;

    let mut sub = Vec::new();
    for value in [4u16, 0, 0, SEGMENTS * 2, 0, 0, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    sub.extend_from_slice(&LAST.to_be_bytes()); // endCode
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    sub.extend_from_slice(&FIRST.to_be_bytes()); // startCode
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // idDelta: the array is absolute
    sub.extend_from_slice(&1u16.to_be_bytes());
    // idRangeOffset: the glyph array begins immediately after this array, and
    // the offset is counted from this slot, so it is the distance to the end
    // of the array — two bytes for each segment from this one on.
    sub.extend_from_slice(&(SEGMENTS * 2).to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes());
    for code in FIRST..=LAST {
        sub.extend_from_slice(&glyph_for(code).to_be_bytes());
    }

    let mut cmap = Vec::new();
    // One (3,1) Windows Unicode BMP subtable, which is the one a reader
    // prefers and the one a Latin face would carry.
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    cmap
}
