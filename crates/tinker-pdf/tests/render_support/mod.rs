//! What the render tiers of [`docs/design/render-verification.md`] share.
//!
//! Four test binaries draw a page and then say something about the pixels:
//! `render_analytic.rs` compares them against a formula, `render_differential.rs`
//! compares two documents that must draw the same picture, `render_goldens.rs`
//! compares them against a reviewed raster, and `writer_graphics.rs` compares
//! the writer's output against a hand-written twin. All four had their own copy
//! of `fn render`, and two had their own copy of the comparator.
//!
//! It also builds the pages two files must agree on byte for byte: the analytic
//! pages `determinism.rs` fingerprints and `render_analytic.rs` evaluates, and
//! the pen, bilevel, minified JPEG and minified JPEG 2000 pages
//! `determinism.rs` fingerprints and `render_regions.rs` tiles.
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

/// The validate-on-save helper's facade reading, for the pages this module
/// builds; a test that takes this module may take the reading itself too.
#[allow(clippy::duplicate_mod)]
#[path = "../written_support/mod.rs"]
mod written_support;

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
    render_document(&Document::open(bytes).expect("the document opens"))
}

/// The first page of a document already open — the one a validate-on-save
/// door hands back — rendered with the default options.
pub fn render_document(document: &Document) -> Bitmap {
    document
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
    built("flooded", builder)
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
    built("image page", builder)
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
    built("blend grid page", builder)
}

/// The pen's whole range on one 91 x 131 page: the `strokes` fingerprint's
/// page in `determinism.rs`, and a fixture `render_regions.rs` tiles at its own
/// page size.
///
/// The three caps of 8.4.3.3 on lines 6.2 points wide; the three joins of
/// 8.4.3.4 on zigzags; a corner sharp enough that a miter limit of 2 bevels it
/// (its miter is 5.2 times the width) beside a blunter one that a limit of 10
/// lets through (3.8 times) (8.4.3.5); two dash patterns at two phases along
/// two S-shaped curves, one under round caps and joins and one under butt caps
/// and miters (8.4.3.6); and a five-pointed star filled under the even-odd
/// rule, so its middle stays empty (8.5.3.3.3), and stroked with mitred
/// points. Every coordinate is fractional, so partial coverage is everywhere
/// and the outlines overlap wherever a pen turns.
pub fn pen_page() -> Vec<u8> {
    const CONTENT: &str = "\
0.12 0.31 0.78 RG 6.2 w 0 j
0 J 9.3 119.4 m 39.7 123.1 l S
1 J 9.3 107.6 m 39.7 111.3 l S
2 J 9.3 95.8 m 39.7 99.5 l S
0.78 0.24 0.09 RG 4.6 w 0 J
0 j 49.2 110.3 m 58.9 124.6 l 69.4 108.1 l 81.7 121.9 l S
1 j 49.2 95.1 m 58.9 109.4 l 69.4 92.9 l 81.7 106.7 l S
2 j 49.2 79.9 m 58.9 94.2 l 69.4 77.7 l 81.7 91.5 l S
0.09 0.55 0.27 RG 3.4 w 0 j 2 M
6.6 72.3 m 38.2 77.9 l 6.6 84.7 l S
10 M 6.6 53.2 m 33.5 60.6 l 6.6 68.0 l S
0.46 0.12 0.62 RG 2.3 w 1 J 1 j [6 3] 0 d
5.4 38.7 m 27.9 58.3 58.6 19.4 85.2 43.6 c S
0 J 0 j [5 2 1.5 2] 3.7 d
5.4 27.1 m 29.3 46.9 57.2 9.8 85.2 31.2 c S
[] 0 d
0.93 0.71 0.08 rg 0.35 0.2 0.05 RG 0.8 w
45.5 24.5 m 38.74 3.7 l 56.44 16.55 l 34.56 16.55 l 52.26 3.7 l h B*
";
    let mut builder = DocumentBuilder::new();
    builder.add_page(91.0, 131.0, |page| page.raw(CONTENT.as_bytes()));
    built("pen page", builder)
}

// ---- the bilevel page --------------------------------------------------------
//
// One-bit images, which `determinism.rs` fingerprints and `render_regions.rs`
// tiles. Built here so the page the hash covers and the page the tile guard
// holds are one document.

/// The bilevel raster's width in samples: not a multiple of eight, so every row
/// ends inside a byte and its padding bits are there to be ignored.
pub const BILEVEL_WIDTH: u32 = 21;
/// Its height in samples.
pub const BILEVEL_HEIGHT: u32 = 15;

/// Whether the bilevel raster is black at `(x, y)`, row 0 the top.
///
/// A ring, a diagonal stripe and a bar down the right-hand side only: no axis
/// of symmetry, so a mirrored or transposed draw is a different picture, and
/// runs of every length for the fax coder's horizontal and vertical modes.
fn bilevel_black(x: u32, y: u32) -> bool {
    let (dx, dy) = (x as i32 - 7, y as i32 - 6);
    let ring = (16..=36).contains(&(dx * dx + dy * dy));
    let stripe = (x + 2 * y) % 7 == 0;
    let bar = x >= 16 && y % 4 < 2;
    ring || stripe || bar
}

/// The bilevel raster as one-bit DeviceGray samples, 0 for black, each row
/// padded to a byte with zeros (8.9.3).
pub fn bilevel_samples() -> Vec<u8> {
    let stride = BILEVEL_WIDTH.div_ceil(8) as usize;
    let mut packed = vec![0u8; stride * BILEVEL_HEIGHT as usize];
    for y in 0..BILEVEL_HEIGHT {
        for x in 0..BILEVEL_WIDTH {
            if !bilevel_black(x, y) {
                packed[y as usize * stride + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    packed
}

/// Bytes as `ASCIIHexDecode` text, end-of-data marker included.
fn ascii_hex(bytes: &[u8]) -> String {
    let mut out: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    out.push('>');
    out
}

/// The bilevel page: one raster five ways, each drawn magnified, at 1:1 and
/// minified 2:1 and 3:1, at fractional offsets.
///
/// The five are the ways a one-bit picture reaches the sampler: DeviceGray
/// samples as they stand, the same raster T.6-coded as a CCITT G4 fax, the
/// samples under `/Decode [1 0]`, a two-colour one-bit `/Indexed` palette, and
/// an `/ImageMask` painted in a fill colour (8.9.6.2). One raster, so the plain
/// row and the fax row draw one picture through two decoders and the inverted
/// row its negative. Magnified by 1.75, so each sample's edge falls inside a
/// pixel; the minified draws average whole and partial samples, which is where
/// a one-bit image's greys come from.
///
/// The fax is coded by the engine's own `ccitt_g4_encode`. That says nothing
/// about T.6 — `ccitt.rs` holds the decoder to hand-coded bits — and need not:
/// what this page pins is how a decoded one-bit raster is drawn, and a coder
/// that changed its codes without changing the raster would leave the page as
/// it is.
///
/// 93 by 165 points, both coprime with every tile size `render_regions.rs`
/// cuts, so no lattice of its divides the page.
pub fn bilevel_page() -> Vec<u8> {
    let samples = bilevel_samples();
    let fax = tinker_pdf_filters::ccitt_g4_encode(&tinker_pdf_filters::CcittSource {
        columns: BILEVEL_WIDTH,
        rows: BILEVEL_HEIGHT,
        black_is_1: false,
        stride: BILEVEL_WIDTH.div_ceil(8) as usize,
        end_of_block: true,
        data: &samples,
    })
    .expect("the raster codes as G4");
    let (plain, coded) = (ascii_hex(&samples), ascii_hex(&fax));

    // One row of the page per image, top to bottom; across each row the four
    // sizes, from 1.75 device pixels a sample down to a third of one.
    let mut content = String::new();
    for (row, name) in ["Im0", "Im1", "Im2", "Im3", "Im4"].iter().enumerate() {
        if *name == "Im4" {
            content.push_str("0.8 0.1 0.25 rg\n");
        }
        let base = 134.65 - 32.2 * row as f64;
        for (width, height, x, rise) in [
            (36.75, 26.25, 2.3, 0.0),
            (21.0, 15.0, 42.15, 2.6),
            (10.5, 7.5, 66.35, 7.45),
            (7.0, 5.0, 80.05, 10.3),
        ] {
            content.push_str(&format!(
                "q {width} 0 0 {height} {x} {:.2} cm /{name} Do Q\n",
                base + rise
            ));
        }
    }

    let image = |extra: &str, data: &str| {
        format!(
            "<< /Type /XObject /Subtype /Image /Width {BILEVEL_WIDTH} /Height {BILEVEL_HEIGHT} \
             /BitsPerComponent 1 {extra} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 93 165]\n   \
         /Resources << /XObject << /Im0 5 0 R /Im1 6 0 R /Im2 7 0 R /Im3 8 0 R /Im4 9 0 R >> >>\n   \
         /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        image("/ColorSpace /DeviceGray /Filter /ASCIIHexDecode", &plain),
        image(
            &format!(
                "/ColorSpace /DeviceGray /Filter [/ASCIIHexDecode /CCITTFaxDecode] \
                 /DecodeParms [null << /K -1 /Columns {BILEVEL_WIDTH} /Rows {BILEVEL_HEIGHT} >>]"
            ),
            &coded,
        ),
        image(
            "/ColorSpace /DeviceGray /Decode [1 0] /Filter /ASCIIHexDecode",
            &plain,
        ),
        image(
            "/ColorSpace [/Indexed /DeviceRGB 1 <1f3a93f2c94c>] /Filter /ASCIIHexDecode",
            &plain,
        ),
        image("/ImageMask true /Filter /ASCIIHexDecode", &plain),
    ];
    let mut out = String::from("%PDF-1.7\n");
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

// ---- the minified JPEG page --------------------------------------------------
//
// JPEGs drawn at a half, a quarter and an eighth of their size, which a render
// decodes at those scales (`docs/design/jpeg-speed.md`, "The drawn-size
// request"): the page `determinism.rs` fingerprints as `jpeg_minified` and
// `render_regions.rs` tiles.

/// The baseline JPEG's size: odd both ways, so its last block column and row
/// are partly past the image and its reduced grid's last sample stands for
/// less than a whole one at every scale.
pub const MINIFIED_BASELINE: (u32, u32) = (101, 77);

/// The progressive JPEG's size, odd both ways too.
pub const MINIFIED_PROGRESSIVE: (u32, u32) = (93, 61);

/// A 4:2:0 baseline JPEG of `width` by `height` samples from the engine's own
/// encoder: hue and lightness ramps crossed by a ripple, so its blocks carry
/// AC terms each reduced transform keeps a part of, and no sample is the
/// page's white.
pub fn minified_baseline_jpeg(width: u32, height: u32) -> Vec<u8> {
    use tinker_pdf_filters::{JpegOptions, JpegSampling, JpegSource, JpegSourceColour};

    let samples: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let ripple = ((x * 7 + y * 3) % 23) * 3;
                [
                    (30 + x * 150 / width + ripple) as u8,
                    (20 + y * 120 / height + ripple / 2) as u8,
                    (90 + (x * 5 + y * 11) % 70) as u8,
                ]
            })
        })
        .collect();
    tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Rgb,
            stride: width as usize * 3,
            data: &samples,
        },
        &JpegOptions {
            sampling: JpegSampling::FourTwoZero,
            ..JpegOptions::default()
        },
    )
    .expect("the raster encodes")
}

/// Bits written most significant first, with T.81 B.1.1.5's stuffed zero
/// after every `X'FF'` and the last byte padded with ones.
#[derive(Default)]
struct EntropyBits {
    out: Vec<u8>,
    byte: u32,
    filled: u32,
}

impl EntropyBits {
    /// The low `count` bits of `value`, the highest first.
    fn put(&mut self, value: u32, count: u32) {
        for at in (0..count).rev() {
            self.byte = (self.byte << 1) | ((value >> at) & 1);
            self.filled += 1;
            if self.filled == 8 {
                self.flush_byte();
            }
        }
    }

    fn flush_byte(&mut self) {
        let byte = (self.byte & 0xFF) as u8;
        self.out.push(byte);
        if byte == 0xFF {
            self.out.push(0x00);
        }
        self.byte = 0;
        self.filled = 0;
    }

    /// The segment, its last byte padded with ones.
    fn finish(mut self) -> Vec<u8> {
        while self.filled != 0 {
            self.put(1, 1);
        }
        self.out
    }
}

/// F.1.2.1's size category of a difference or coefficient `value`, and the
/// bits that follow its code: the value itself, or one less than it for a
/// negative one, in that many bits.
fn magnitude(value: i32) -> (u32, u32) {
    let size = 32 - value.unsigned_abs().leading_zeros();
    let bits = if value < 0 {
        value.wrapping_sub(1)
    } else {
        value
    };
    let mask = if size == 0 {
        0
    } else {
        u32::MAX >> (32 - size)
    };
    (size, (bits as u32) & mask)
}

/// The AC table's symbols in the order the DHT segment lists them, each coded
/// by its index in eight bits: end-of-band, the sixteen-zero run, then every
/// run of 0 to 15 zeros before a coefficient of size 1 to 10.
fn ac_symbols() -> Vec<u8> {
    let mut symbols = vec![0x00, 0xF0];
    for run in 0..16u8 {
        for size in 1..=10u8 {
            symbols.push((run << 4) | size);
        }
    }
    symbols
}

/// A 4:2:0 progressive JPEG (SOF2) of `width` by `height` samples, written
/// here because the engine's encoder writes baseline only: one interleaved DC
/// first scan of the three components, then one AC first scan of each, the
/// band 1 to 63 with no successive approximation (T.81 G.1.2).
///
/// The quantised coefficients are stated rather than transformed from a
/// picture: each block's DC a ramp across the component, and the first six
/// AC terms in zigzag order — all inside the four by four corner a half-scale
/// decode keeps, three of them in the two by two a quarter keeps — a pattern
/// of its position, between -5 and 5. The DC table codes each size in four
/// bits and the AC table each symbol in eight, so the entropy coding is
/// arithmetic a reader can follow, and T.81's own Huffman procedure is not
/// what is under test.
///
/// The width and height must make each component's own block grid its share
/// of the MCU grid — `⌈width / 8⌉` and `⌈height / 8⌉` even — so that the AC
/// scans, which code a component's own blocks (A.2.2), code exactly the blocks
/// the DC scan did.
pub fn minified_progressive_jpeg(width: u32, height: u32) -> Vec<u8> {
    let (mcus_x, mcus_y) = (width.div_ceil(16), height.div_ceil(16));
    assert_eq!(
        (width.div_ceil(8), height.div_ceil(8)),
        (mcus_x * 2, mcus_y * 2),
        "each component's own grid is its share of the MCU grid"
    );
    // Each component's blocks across and down: luma two a unit each way,
    // chroma one.
    let grids = [(mcus_x * 2, mcus_y * 2), (mcus_x, mcus_y), (mcus_x, mcus_y)];
    // A block's quantised coefficients in zigzag order, the first seven.
    let block = |component: u32, bx: u32, by: u32| -> [i32; 7] {
        let dc = match component {
            0 => -60 + (bx * 5 + by * 4) as i32,
            1 => -20 + (bx * 6) as i32,
            _ => 30 - (by * 8) as i32,
        };
        let mut coefficients = [dc, 0, 0, 0, 0, 0, 0];
        for (k, slot) in coefficients.iter_mut().enumerate().skip(1) {
            let k = k as u32;
            *slot = ((bx * 7 + by * 13 + component * 5 + k * 3) % 11) as i32 - 5;
        }
        coefficients
    };

    let mut out = vec![0xFF, 0xD8];
    // DQT, table 0: 8 for the DC term and 4 for every AC one.
    let mut quant = [4u8; 64];
    quant[0] = 8;
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend_from_slice(&quant);
    // SOF2: eight bits, three components, luma at 2 x 2 and chroma at 1 x 1.
    out.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x11, 0x08]);
    out.extend_from_slice(&(height as u16).to_be_bytes());
    out.extend_from_slice(&(width as u16).to_be_bytes());
    out.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
    // DHT: DC table 0, sizes 0 to 11 in four bits; AC table 0, every symbol in
    // eight.
    let mut dc = vec![0x00];
    let mut counts = [0u8; 16];
    counts[3] = 12;
    dc.extend_from_slice(&counts);
    dc.extend(0..12u8);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dc.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dc);
    let symbols = ac_symbols();
    let mut ac = vec![0x10];
    let mut counts = [0u8; 16];
    counts[7] = symbols.len() as u8;
    ac.extend_from_slice(&counts);
    ac.extend_from_slice(&symbols);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((ac.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&ac);

    // The DC first scan: every component, MCU by MCU, each block's
    // difference from the last DC of its component.
    out.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00,
    ]);
    let mut bits = EntropyBits::default();
    let mut predicted = [0i32; 3];
    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            let mut units = Vec::new();
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                units.push((0u32, mx * 2 + dx, my * 2 + dy));
            }
            units.push((1, mx, my));
            units.push((2, mx, my));
            for (component, bx, by) in units {
                let dc = block(component, bx, by)[0];
                let (size, extra) = magnitude(dc - predicted[component as usize]);
                predicted[component as usize] = dc;
                bits.put(size, 4);
                bits.put(extra, size);
            }
        }
    }
    out.extend_from_slice(&bits.finish());

    // One AC first scan a component, over its own blocks in raster order.
    for (component, &(across, down)) in grids.iter().enumerate() {
        let component = component as u32;
        out.extend_from_slice(&[
            0xFF,
            0xDA,
            0x00,
            0x08,
            0x01,
            component as u8 + 1,
            0x00,
            0x01,
            0x3F,
            0x00,
        ]);
        let mut bits = EntropyBits::default();
        for by in 0..down {
            for bx in 0..across {
                let coefficients = block(component, bx, by);
                let mut run = 0u8;
                for &value in &coefficients[1..] {
                    if value == 0 {
                        run += 1;
                        continue;
                    }
                    let (size, extra) = magnitude(value);
                    let symbol = (run << 4) | size as u8;
                    let code = symbols
                        .iter()
                        .position(|&s| s == symbol)
                        .expect("every run and size is a symbol");
                    bits.put(code as u32, 8);
                    bits.put(extra, size);
                    run = 0;
                }
                // The rest of the band is zero: end-of-band, a run of one.
                bits.put(0, 8);
            }
        }
        out.extend_from_slice(&bits.finish());
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// The minified JPEG page: the baseline JPEG drawn at exactly a half, a
/// quarter and an eighth of its size, and the progressive one at the same
/// three — the half flipped left to right, the quarter turned a quarter, the
/// eighth clipped through its middle — on a 119 by 157 page no tile size of
/// `render_regions.rs` divides. Drawn at 72 dpi, each is decoded at the scale
/// it is drawn at; at another scale each is decoded at the one that scale
/// draws.
pub fn jpeg_minified_page() -> Vec<u8> {
    let (bw, bh) = MINIFIED_BASELINE;
    let (pw, ph) = MINIFIED_PROGRESSIVE;
    let baseline = ascii_hex(&minified_baseline_jpeg(bw, bh));
    let progressive = ascii_hex(&minified_progressive_jpeg(pw, ph));
    let (bw, bh, pw, ph) = (f64::from(bw), f64::from(bh), f64::from(pw), f64::from(ph));
    let content = format!(
        "q {} 0 0 {} 3.3 113.1 cm /B Do Q\n\
         q {} 0 0 {} 106.7 118.4 cm /P Do Q\n\
         q {} 0 0 {} 4.6 82.3 cm /B Do Q\n\
         q 0 {} {} 0 56.4 78.7 cm /P Do Q\n\
         q {} 0 0 {} 8.1 50.6 cm /B Do Q\n\
         q 40.3 52.2 6.1 4.4 re W n {} 0 0 {} 40.3 52.2 cm /P Do Q\n",
        bw / 2.0,
        bh / 2.0,
        -pw / 2.0,
        ph / 2.0,
        bw / 4.0,
        bh / 4.0,
        pw / 4.0,
        -ph / 4.0,
        bw / 8.0,
        bh / 8.0,
        pw / 8.0,
        ph / 8.0,
    );
    let image = |width: u32, height: u32, data: &str| {
        format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 \
             /Filter [/ASCIIHexDecode /DCTDecode] /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    };
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 119 157]\n   \
         /Resources << /XObject << /B 5 0 R /P 6 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        image(MINIFIED_BASELINE.0, MINIFIED_BASELINE.1, &baseline),
        image(MINIFIED_PROGRESSIVE.0, MINIFIED_PROGRESSIVE.1, &progressive),
    ];
    let mut out = String::from("%PDF-1.7\n");
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

// ---- the rendering-intent pages ----------------------------------------------
//
// RC-01's pages: ICC colours painted under each rendering intent, through a
// profile carrying a table for each intent and through a matrix/TRC one, both
// on a media white other than D50. Here rather than in `rendering_intents.rs`
// because `determinism.rs` is to fingerprint these same bytes.

/// The media white the intent profiles carry: a warm paper, darker than D50,
/// every channel its own ratio to it, so absolute colorimetric is a transform
/// of its own.
pub const INTENT_PAPER: [f64; 3] = [0.88, 0.91, 0.72];

/// sRGB's primaries adapted to D50, as an sRGB ICC profile stores them.
pub const SRGB_COLUMNS: [[f64; 3]; 3] = [
    [0.436_065_674, 0.222_488_403, 0.013_916_015],
    [0.385_147_095, 0.716_873_169, 0.097_076_416],
    [0.143_066_406, 0.060_607_910, 0.714_096_069],
];

/// The intent pages' components: a fill, a stroke and a glyph, each a
/// quarter, a half or three quarters, which a two-point grid reads at whole
/// fractions of 256, and each a node of the 33-point grid a three-input
/// table converts through (`docs/design/icc.md`, P-22), so a conversion is
/// the table's pipeline exactly.
pub const INTENT_PAINTS: [[f64; 3]; 3] = [[0.25, 0.5, 0.75], [0.75, 0.25, 0.5], [0.5, 0.75, 0.25]];

/// An `XYZType` tag.
fn xyz_tag(value: [f64; 3]) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&[0; 4]);
    for v in value {
        out.extend_from_slice(&(((v * 65536.0).round() as i32) as u32).to_be_bytes());
    }
    out
}

/// A `curv` tag holding one gamma.
fn gamma_tag(gamma: f64) -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    out
}

/// A version 2 display profile in `space` and `pcs` carrying `tags`.
fn icc_profile(space: &[u8; 4], pcs: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0u8; 128];
    out[8..12].copy_from_slice(&0x0200_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(pcs);
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    let mut body = Vec::new();
    let start = 132 + tags.len() * 12;
    for (signature, data) in tags {
        out.extend_from_slice(signature);
        out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// Node `node` of [`intent_lut_profile`]'s table `table`, the first input's
/// axis slowest, as CIE L*a*b*: each table its own colours.
pub fn intent_lut_lab(table: usize, node: usize) -> [f64; 3] {
    let bit = |shift: usize| ((node >> shift) & 1) as f64;
    let (i, j, k, t) = (bit(2), bit(1), bit(0), table as f64);
    [
        18.0 + 30.0 * i + 25.0 * j + 15.0 * k + 4.0 * t,
        -40.0 + 70.0 * j + 10.0 * k - 30.0 * t,
        -10.0 + 50.0 * k - 30.0 * i + 35.0 * t,
    ]
}

/// [`intent_lut_lab`] in v2's legacy 16-bit Lab, the encoding an `mft2`
/// holds: `L*` of a hundred and `a*`, `b*` of 127 at `0xFF00`.
pub fn intent_lut_encoded(table: usize, node: usize) -> [u16; 3] {
    let [l, a, b] = intent_lut_lab(table, node);
    let encode = |v: f64| (v * 65280.0).round().clamp(0.0, 65535.0) as u16;
    [
        encode(l / 100.0),
        encode((a + 128.0) / 255.0),
        encode((b + 128.0) / 255.0),
    ]
}

/// An `mft2` tag over three inputs on a two-point grid holding table
/// `table`'s nodes, its input and output curves the identity.
fn intent_table(table: usize) -> Vec<u8> {
    let mut out = b"mft2".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[3, 3, 2, 0]);
    for row in 0..3 {
        for column in 0..3 {
            let value: i32 = if row == column { 65536 } else { 0 };
            out.extend_from_slice(&(value as u32).to_be_bytes());
        }
    }
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..3 {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    for node in 0..8 {
        for value in intent_lut_encoded(table, node) {
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    for _ in 0..3 {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    out
}

/// An RGB profile into Lab carrying a table for each intent — `A2B0`,
/// `A2B1` and `A2B2`, three different grids — and [`INTENT_PAPER`] for its
/// media white.
pub fn intent_lut_profile() -> Vec<u8> {
    icc_profile(
        b"RGB ",
        b"Lab ",
        &[
            (*b"A2B0", intent_table(0)),
            (*b"A2B1", intent_table(1)),
            (*b"A2B2", intent_table(2)),
            (*b"wtpt", xyz_tag(INTENT_PAPER)),
        ],
    )
}

/// A matrix/TRC profile on sRGB's primaries under gamma 2 curves, its media
/// white [`INTENT_PAPER`].
pub fn intent_matrix_profile() -> Vec<u8> {
    icc_profile(
        b"RGB ",
        b"XYZ ",
        &[
            (*b"rXYZ", xyz_tag(SRGB_COLUMNS[0])),
            (*b"gXYZ", xyz_tag(SRGB_COLUMNS[1])),
            (*b"bXYZ", xyz_tag(SRGB_COLUMNS[2])),
            (*b"rTRC", gamma_tag(2.0)),
            (*b"gTRC", gamma_tag(2.0)),
            (*b"bTRC", gamma_tag(2.0)),
            (*b"wtpt", xyz_tag(INTENT_PAPER)),
        ],
    )
}

/// The intent page's height, and its width, in points and in pixels alike at
/// the default 72 dpi.
pub const INTENT_SIZE: f64 = 240.0;

/// Where [`intent_page`] paints `paint` — 0 the fill, 1 the stroke, 2 the
/// glyph — in row `row`, 0 the table profile's and 1 the matrix's: a point
/// well inside it, in the page's own space.
pub fn intent_spot(row: usize, paint: usize) -> (f64, f64) {
    let base = 10.0 + 120.0 * row as f64;
    match paint {
        0 => (35.5, base + 35.5),
        1 => (110.5, base + 35.5),
        _ => (175.5, base + 25.5),
    }
}

/// The page painting each of [`INTENT_PAINTS`] — a filled square, a stroked
/// bar and a glyph of [`curvy_font`], its wedge — in the table profile's
/// space `/L` along the bottom and the matrix profile's `/M` above it, under
/// the rendering intent `ri` names, `intent`, set before every colour.
pub fn intent_page(intent: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()),
        "the synthetic face parses"
    );
    assert!(builder.add_icc_color_space(b"L", &intent_lut_profile(), 3));
    assert!(builder.add_icc_color_space(b"M", &intent_matrix_profile(), 3));
    builder.add_page(INTENT_SIZE, INTENT_SIZE, |page| {
        page.raw(format!("/{intent} ri").as_bytes());
        for (row, space) in ["L", "M"].iter().enumerate() {
            let base = 10.0 + 120.0 * row as f64;
            let [fill, stroke, glyph] =
                INTENT_PAINTS.map(|c| format!("{} {} {}", c[0], c[1], c[2]));
            page.raw(
                format!(
                    "/{space} cs {fill} scn 10 {} 50 50 re f\n\
                     /{space} CS {stroke} SCN 20 w 80 {} m 140 {} l S\n\
                     /{space} cs {glyph} scn",
                    base + 10.0,
                    base + 35.5,
                    base + 35.5,
                )
                .as_bytes(),
            );
            // The wedge's lower left, well inside its outline, is the spot.
            page.text(b"F0", 100.0, 160.0, base + 10.0, "#");
        }
    });
    builder.finish()
}

/// A page painting through [`intent_lut_profile`]'s space `/L` with
/// `content`, and graphics states `/GSp`, `/GSs`, `/GSa` and `/GSx` naming
/// perceptual, saturation, absolute colorimetric and an intent 8.6.5.8 does
/// not list by `/RI`: the twin of a page that names its intents by `ri`.
pub fn intent_states_page(content: &str) -> Vec<u8> {
    let profile = intent_lut_profile();
    let mut bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 40]\n\
   /Resources << /ColorSpace << /L [/ICCBased 5 0 R] >>\n\
     /ExtGState << /GSp << /Type /ExtGState /RI /Perceptual >>\n\
       /GSs << /Type /ExtGState /RI /Saturation >>\n\
       /GSa << /Type /ExtGState /RI /AbsoluteColorimetric >>\n\
       /GSx << /Type /ExtGState /RI /Ultraviolet >> >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /N 3 /Length {} >>\nstream\n",
        content.len(),
        profile.len()
    )
    .into_bytes();
    bytes.extend_from_slice(&profile);
    bytes.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

// ---- the minified JPEG 2000 page ---------------------------------------------
//
// JPEG 2000 codestreams drawn at a half, a quarter and less of their size,
// which a render decodes with that many resolution levels dropped
// (`docs/design/jpx-speed.md`, "The page's request"): the page
// `determinism.rs` fingerprints as `jpx_minified` and `render_regions.rs`
// tiles.

/// One of the JPEG 2000 decoder's own fixtures
/// (`crates/tinker-pdf-filters/tests/jpx/`), by its file name.
fn jpx_fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tinker-pdf-filters/tests/jpx"
        ))
        .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The codestreams [`jpx_minified_page`] draws, by resource name: the file,
/// and its width and height.
///
/// - `/A`, `c1-3.jp2`: 32 by 24 RGB, the reversible 5/3 and the RCT, two
///   levels.
/// - `/B`, `r1-i3.jp2`: 32 by 24 grey, the irreversible 9/7, two levels.
/// - `/C`, `c2-3.jp2`: 17 by 13 RGB, the 5/3 and the RCT, two levels, odd
///   both ways, so its reduced grid's last sample stands for less than a
///   whole one at either discard.
/// - `/D`, `r2-i3.jp2`: 17 by 13 grey, the 9/7, two levels, odd both ways.
/// - `/E`, `c1-i2.jp2`: 32 by 24 RGB, the 9/7 and the ICT, one level.
/// - `/S`, `s1.jp2`: 63 by 47 RGB whose three components are each sampled
///   every second column and row, the 5/3, one level, so a reduced grid
///   replicates subsampled components as the full one does.
pub const MINIFIED_JPX: [(&str, &str, u32, u32); 6] = [
    ("A", "c1-3.jp2", 32, 24),
    ("B", "r1-i3.jp2", 32, 24),
    ("C", "c2-3.jp2", 17, 13),
    ("D", "r2-i3.jp2", 17, 13),
    ("E", "c1-i2.jp2", 32, 24),
    ("S", "s1.jp2", 63, 47),
];

/// The minified JPEG 2000 page, 96 by 80 points, which no tile size of
/// `render_regions.rs` divides: [`MINIFIED_JPX`]'s codestreams each drawn at
/// the sizes that drop each number of levels it has — `/A` at a half, at a
/// quarter flipped left to right and at a tenth, whose size would drop a
/// third level the codestream does not have;
/// `/B` at a half quarter-turned and at a quarter; `/C` at the floor of a
/// half and at the floor of a quarter, the second partly clipped; `/D` at
/// the floor of a half and sheared; `/E` at a half; and `/S` at the floor of
/// a half, a third of it clipped away — every draw at fractional offsets.
/// Drawn at 72 dpi each is decoded with the levels its size drops; at another
/// scale, with the levels that scale's size drops.
pub fn jpx_minified_page() -> Vec<u8> {
    let content = "q 16 0 0 12 3.3 64.1 cm /A Do Q\n\
         q -8 0 0 6 32.6 66.2 cm /A Do Q\n\
         q 0 16 -12 0 50.4 58.7 cm /B Do Q\n\
         q 8 0 0 6 56.8 67.3 cm /B Do Q\n\
         q 16 0 0 12 70.2 63.5 cm /E Do Q\n\
         q 4.7 31.9 20.3 23 re W n 31 0 0 23 4.7 31.9 cm /S Do Q\n\
         q 8 0 0 6 41.3 45.6 cm /C Do Q\n\
         q 8 0 1 5 54.1 44.2 cm /D Do Q\n\
         q 67.4 44.9 2.6 2.1 re W n 4 0 0 3 66.1 44.3 cm /C Do Q\n\
         q 3.2 0 0 2.4 12.2 12.8 cm /A Do Q\n";
    let images: Vec<String> = MINIFIED_JPX
        .iter()
        .map(|(_, file, width, height)| {
            let data = ascii_hex(&jpx_fixture(file));
            format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
                 /Filter [/ASCIIHexDecode /JPXDecode] /Length {} >>\nstream\n{data}\nendstream",
                data.len()
            )
        })
        .collect();
    let names: String = MINIFIED_JPX
        .iter()
        .enumerate()
        .map(|(index, (name, ..))| format!("/{name} {} 0 R ", index + 5))
        .collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 96 80]\n   \
             /Resources << /XObject << {names}>> >> /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    objects.extend(images);
    let mut out = String::from("%PDF-1.7\n");
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

/// A stream object: `entries` and its `/Length`, then `data`.
pub fn raw_stream(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {entries} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A document of `objects`, numbered from one, the first its catalog, with
/// its cross-reference table.
pub fn raw_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The `/Rect` [`placed_page`]'s annotation stands on unless a twin moves it,
/// in default user space, as `[x0, y0, x1, y1]`.
pub const PLACED_RECT: [f64; 4] = [50.0, 50.0, 70.0, 70.0];

/// The fit 12.5.5 gives [`placed_page`]'s appearance, `/BBox [0 0 40 40]`
/// with no `/Matrix`, onto [`PLACED_RECT`]: halved and moved to its corner.
pub const PLACED_FIT: [f64; 6] = [0.5, 0.0, 0.0, 0.5, 50.0, 50.0];

/// A 120 x 100 page turned by `rotation` (`/Rotate`) with a grey bar of its
/// own, and one annotation whose `/F` is `flags`, whose `/Rect` is `rect`,
/// and whose appearance — a red square and a blue triangle in its lower left
/// corner, in a `/BBox [0 0 40 40]`, so that a turn or a mirror of it shows —
/// carries `matrix` as its `/Matrix` where that is some. What
/// `annotation_appearances.rs` places at each magnification and turn and
/// holds to its twin, `render_regions.rs` tiles and `render_options.rs`
/// fingerprints (`docs/design/render-usage.md`, 12.5.3's `/NoZoom` and
/// `/NoRotate`). Every number is written as Rust prints it, the shortest
/// decimal that reads back as the same `f64`.
pub fn placed_page(rotation: u16, flags: i64, rect: [f64; 4], matrix: Option<[f64; 6]>) -> Vec<u8> {
    let appearance = "1 0 0 rg 0 0 40 40 re f 0 0 1 rg 0 0 m 30 0 l 0 12 l h f";
    let content = "0.5 g 10 10 40 20 re f";
    let numbers = |values: &[f64]| {
        values
            .iter()
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let matrix = matrix.map_or_else(String::new, |m| format!("/Matrix [{}]", numbers(&m)));
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 100] /Rotate {rotation} \
             /Annots [5 0 R] /Contents 4 0 R >>"
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
        format!(
            "<< /Type /Annot /Subtype /Square /F {flags} /Rect [{}] /AP << /N 6 0 R >> >>",
            numbers(&rect)
        )
        .into_bytes(),
        raw_stream(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 40 40] {matrix}"),
            appearance.as_bytes(),
        ),
    ];
    raw_pdf(&objects)
}

/// The four intents Table 69 names, in [`intents_page`]'s column order.
pub const INTENT_NAMES: [&str; 4] = [
    "Perceptual",
    "RelativeColorimetric",
    "Saturation",
    "AbsoluteColorimetric",
];

/// The width of each of [`intents_page`]'s columns and the height of each of
/// its rows, in points and in pixels alike at the default 72 dpi.
pub const INTENTS_CELL: (f64, f64) = (40.0, 20.0);

/// [`intents_page`]'s image samples, row-major from the top (8.9.5.2): three
/// corners of the table profile's cube — nodes of the grid its table converts
/// through — and a colour between nodes.
pub const INTENTS_SAMPLES: [[u8; 3]; 4] =
    [[0, 255, 0], [255, 0, 255], [255, 255, 0], [128, 64, 191]];

/// [`intents_page`]'s axial shading's two ends, in the table profile's space:
/// the colour at `t = 0` and at `t = 1`, `t` stepping by 1/32 from one
/// column to the next so that each column's colour is a node of the table's
/// 33-point grid (`docs/design/icc.md`, P-22).
pub const INTENTS_AXIAL: ([f64; 3], [f64; 3]) = ([0.0, 1.0, 0.0], [1.0, 0.0, 1.0]);

/// The page painting, in each of four columns one of [`INTENT_NAMES`] by `ri`,
/// a fill, an image and an axial shading in [`intent_lut_profile`]'s space:
/// along the top a fill of [`INTENT_PAINTS`]'s first colour, in the middle
/// [`INTENTS_SAMPLES`] as a 2 × 2 image XObject, and along the bottom
/// [`INTENTS_AXIAL`] painted by `sh` across the column, its axis from the
/// column's centre of pixel 0 to that of pixel 32 and extended both ways.
/// What `determinism.rs` fingerprints as `intents` and `rendering_intents.rs`
/// holds to the transforms recomputed from ICC.1.
pub fn intents_page() -> Vec<u8> {
    let (width, height) = INTENTS_CELL;
    let [fill, ..] = INTENT_PAINTS;
    let mut content = String::new();
    for (column, intent) in INTENT_NAMES.iter().enumerate() {
        let x = width * column as f64;
        content.push_str(&format!(
            "q /{intent} ri\n\
             /L cs {} {} {} scn {x} {} {width} {height} re f\n\
             q {width} 0 0 {height} {x} {height} cm /Im0 Do Q\n\
             q {x} 0 {width} {height} re W n 1 0 0 1 {x} 0 cm /Sh0 sh Q\n\
             Q\n",
            fill[0],
            fill[1],
            fill[2],
            2.0 * height,
        ));
    }
    let samples: Vec<u8> = INTENTS_SAMPLES.iter().flatten().copied().collect();
    let (start, end) = INTENTS_AXIAL;
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}]\n   \
             /Resources << /ColorSpace << /L [/ICCBased 5 0 R] >>\n     \
             /XObject << /Im0 6 0 R >> /Shading << /Sh0 7 0 R >> >>\n   \
             /Contents 4 0 R >>",
            4.0 * width,
            3.0 * height
        )
        .into_bytes(),
        raw_stream("", content.as_bytes()),
        raw_stream("/N 3", &intent_lut_profile()),
        raw_stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 \
             /ColorSpace [/ICCBased 5 0 R] /BitsPerComponent 8",
            &samples,
        ),
        format!(
            "<< /ShadingType 2 /ColorSpace [/ICCBased 5 0 R] /Coords [0.5 0 32.5 0]\n   \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >>\n   \
             /Extend [true true] >>",
            start[0], start[1], start[2], end[0], end[1], end[2]
        )
        .into_bytes(),
    ];
    raw_pdf(&objects)
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

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
