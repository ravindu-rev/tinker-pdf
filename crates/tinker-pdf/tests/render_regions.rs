//! **A tile is byte-equal to the full-page render under it** — ruling 5.
//!
//! Ruling 5 has said since it was written that "a tile must be pinned
//! byte-equal to the full-page subregion, and that test is the permanent
//! guard". Until this file there was no such test anywhere in the tree, and the
//! ruling's September 2026 correction says so in its own voice. This is the
//! test. It is the more important half of the roadmap's region row: the API is
//! four numbers, and the property is the reason the four numbers are worth
//! having.
//!
//! # What the guard actually asserts
//!
//! For every fixture, at several tile sizes, the page is rendered once whole
//! and then again one tile at a time through [`RenderOptions::region`]. Each
//! tile must equal the corresponding rectangle of the whole render **byte for
//! byte**. Not within a tolerance, and deliberately not: a budget here would
//! pass a renderer whose tile seams are a shade different from its interiors,
//! which is exactly the defect a tiling caller assembles into a visible grid
//! across their output and the one thing this guard exists to see.
//!
//! # Why these fixtures and these sizes
//!
//! - **Text, a shading and an image are three different paths** through the
//!   rasterizer, and only one of them is anti-aliased by the scanline filler.
//!   Glyphs go through `fill` and accumulate sixteen sub-scanlines a row; a
//!   shading is its own per-pixel sampler with no coverage at all; an image
//!   goes through the sampler and, below a size threshold, through a *run* that
//!   accumulates fragments over a canvas-sized buffer before compositing. A
//!   guard that tiled one page of rectangles would cover none of the three.
//! - **Tile sizes that do not divide the page**, so the last row and column are
//!   partial. A lattice that fits exactly is the one case where an off-by-one
//!   in the clamp is invisible.
//! - **A rotated page**, because a region applied on the wrong side of
//!   `/Rotate` still produces a clean picture of the page and simply shows the
//!   wrong part of it. Tile equality catches that, and
//!   [`a_rotated_page_region_is_the_corner_of_the_displayed_picture`] catches it
//!   again in a form a reader can check by eye.
//! - **A crop box offset from the media box**, because the region's translation
//!   and the crop box's translation compose, and a sign error in either is a
//!   plausible-looking page.
//! - **Scales other than 1.0**, because a region that is silently ignored
//!   whenever the scale is not 1 is a defect no unscaled fixture can see.
//!
//! # Does it hold unconditionally? Nearly, and the exception is stated
//!
//! **Exactly, at every scale whose arithmetic lets the two frames agree** —
//! every fixture, every tile size, down to a one-pixel lattice, at 0.5, 1, 2
//! and 4. At 0.75, 1.5 and 3 it is exact on **twenty-nine of the thirty**
//! lattices, and the thirtieth differs on a single pixel by one level of 255;
//! [`at_the_scales_where_two_frames_round_apart_the_gap_is_one_level`] measures
//! that, bounds it at exactly what it measures, and says why the last pixel is
//! not reachable without moving the canvas's origin into the rasterizer.
//!
//! # What this guard found
//!
//! It is worth recording that the first run of this file did not report a
//! tiling defect. It reported **two** defects in `tinker-pdf-raster`'s
//! scanline filler, both of which were wrong for a whole page exactly as much
//! as for a tile, and neither of which anything else in the tree could see:
//!
//! 1. **An edge's slope was quantised to a whole 1/256 pixel per
//!    sub-scanline**, truncated. Every slope shallower than one pixel of `x`
//!    per sixteen of `y` became **zero** and the edge was drawn vertical: the
//!    parallelogram in `tinker-pdf-raster/tests/analytic_coverage.rs` is 256
//!    pixels tall and drifts fourteen across, and it came out a straight bar,
//!    fourteen pixels wrong at its far end.
//! 2. **Edge positions were truncated toward zero**, which is a different
//!    operation on the two sides of the origin, so a shape quantised
//!    differently once it moved across one — which is precisely a tile, whose
//!    content sits at negative `x` where the page had it at positive.
//!
//! **A third was suspected and is recorded as refuted**, because a suspicion
//! left in a comment is read later as a finding. The theory was that a
//! crossing could land outside the segment that produced it, since `dx / dy`
//! keeps few bits when `dy` is an ulp or two, and a clamp went into
//! `build_edges` and into the sweep on the strength of it. It was instrumented
//! instead: with a check for a crossing outside its own segment's `x` range,
//! rounded outward, compiled into `fill`, the whole workspace suite fires it
//! **zero times on the arithmetic this commit installs and zero times on the
//! arithmetic it replaces** (15 September 2026, 4 653 tests). Both clamps were
//! removed; the one on the *start* was doing harm, and `fill.rs` says where.
//!
//! That is what a guard is for, and none of it is about tiles.

use tinker_pdf::{
    DeviceSpace, Document, DocumentBuilder, Function, ImageData, Page, PixelRegion, RenderOptions,
    RenderWarning, Shading, WriteOptions,
};

mod render_support;
use render_support::{curvy_font, ink};

/// The fixture page, in points. Not round, and coprime with every tile size
/// below, so no lattice in this file ever divides it evenly.
const PAGE_W: f64 = 91.0;
/// Taller than it is wide, so a quarter turn is visible in the bitmap's shape.
const PAGE_H: f64 = 131.0;

// ---- fixtures ---------------------------------------------------------------

/// How a fixture's page is shaped: its `/Rotate`, and its `/CropBox` when it
/// has one that is not the media box.
#[derive(Clone, Copy)]
struct Shape {
    rotation: i64,
    crop: Option<(f64, f64, f64, f64)>,
}

/// The crop box every cropped fixture uses: offset from the media box on both
/// axes and ending inside it, so the page's own translation is non-zero in `x`
/// and in `y` before the region's is composed onto it.
const CROP: (f64, f64, f64, f64) = (11.0, 17.0, 79.0, 109.0);

impl Shape {
    const PLAIN: Shape = Shape {
        rotation: 0,
        crop: None,
    };

    const fn turned(rotation: i64) -> Shape {
        Shape {
            rotation,
            crop: None,
        }
    }

    const fn cropped(rotation: i64) -> Shape {
        Shape {
            rotation,
            crop: Some(CROP),
        }
    }
}

/// Applies a [`Shape`] to a built document, through the editor.
///
/// Through the editor and not by splicing bytes into the page dictionary,
/// which is what this did first and is worth recording: inserting
/// `/Rotate 90` after `/Type /Page` moves every byte after it, so every offset
/// in the cross-reference table is wrong by the length of the insertion. A page
/// of shapes survives that on the repair path; a page whose font program is
/// reached through the xref does not, and the `text` fixture came back
/// **blank** — and a blank page tiles perfectly. The ink floor in
/// [`with_page`] is what caught it.
fn shaped(bytes: Vec<u8>, shape: Shape) -> Vec<u8> {
    if shape.rotation == 0 && shape.crop.is_none() {
        return bytes;
    }
    let document = Document::open(bytes).expect("the fixture opens");
    let mut editor = document.editor();
    if shape.rotation != 0 {
        assert!(editor.rotate_page(0, shape.rotation), "the page rotates");
    }
    if let Some((x0, y0, x1, y1)) = shape.crop {
        assert!(editor.set_crop_box(0, x0, y0, x1, y1), "the crop box sets");
    }
    editor.save(&WriteOptions::default())
}

/// Seven lines of the six-shape synthetic face, at two sizes and two sub-pixel
/// phases.
///
/// The face is `render_support::curvy_font` — curves, diagonals and a thin
/// apex, chosen so that partial coverage is everywhere rather than at four
/// axis-aligned edges. Subsetting is off so that nothing between these bytes
/// and the pixels is a subsetter.
fn text_page(shape: Shape) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(
        builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()),
        "the synthetic face parses as a TrueType program"
    );
    builder.add_page(PAGE_W, PAGE_H, |page| {
        for line in 0..6 {
            let y = PAGE_H - 18.0 - f64::from(line) * 19.0;
            // The half point on alternate lines puts the same outlines on a
            // different sub-pixel phase, which is where a seam would show.
            let x = if line % 2 == 0 { 5.0 } else { 5.5 };
            page.text(b"F0", 15.0, x, y, "tiled#page");
        }
    });
    shaped(builder.finish(), shape)
}

/// The page flooded by one diagonal axial shading (8.7.4.5.3).
///
/// Diagonal rather than along an axis: a gradient whose parameter is a function
/// of one coordinate agrees with itself under a shift in the other, so an axis
/// -aligned ramp is the one gradient a broken horizontal translation cannot
/// disturb.
fn shading_page(shape: Shape) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 0.0, PAGE_W, PAGE_H],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![0.95, 0.15, 0.05],
                c1: vec![0.05, 0.25, 0.95],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(PAGE_W, PAGE_H, |page| {
        page.raw(format!("q 0 0 {PAGE_W} {PAGE_H} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    shaped(builder.finish(), shape)
}

/// A 5x7 colour image stretched over the page at a ratio that is not a whole
/// number, so every device pixel is a sample between sample points.
fn image_page(shape: Shape) -> Vec<u8> {
    let (w, h) = (5u32, 7u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            // A pattern with no axis of symmetry, so a mirrored or transposed
            // region is not the same picture.
            data.push((x * 47 + y * 11) as u8);
            data.push((x * 13 + y * 61) as u8);
            data.push(255 - (x * 29 + y * 23) as u8);
        }
    }
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: w,
            height: h,
            data: &data,
        }
    ));
    builder.add_page(PAGE_W, PAGE_H, |page| {
        page.image(b"Im0", 3.0, 4.0, PAGE_W - 7.0, PAGE_H - 9.0);
    });
    shaped(builder.finish(), shape)
}

/// Curves and strokes at an angle: the stroker's joins and the flattening
/// tolerance, which are a fourth path and the one that puts long anti-aliased
/// edges across the middle of a page rather than around glyph-sized shapes.
fn strokes_page(shape: Shape) -> Vec<u8> {
    let mut content = String::new();
    content.push_str("0.15 0.35 0.85 RG 2.7 w 1 J 1 j\n");
    content.push_str("4 6 m 33 118 62 12 87 96 c S\n");
    content.push_str("0.85 0.2 0.15 rg\n");
    content.push_str("9 14 m 57 101 l 81 21 l f\n");
    content.push_str("0 0 0 RG 0.6 w\n");
    for step in 0..11 {
        let y = 8.0 + f64::from(step) * 11.3;
        content.push_str(&format!(
            "3 {y:.3} m {:.3} {:.3} l S\n",
            PAGE_W - 3.0,
            y + 4.7
        ));
    }
    let mut builder = DocumentBuilder::new();
    builder.add_page(PAGE_W, PAGE_H, |page| page.raw(content.as_bytes()));
    shaped(builder.finish(), shape)
}

/// Every fixture: a builder, the page dictionary entry it is built with, a
/// name, and the least ink it must draw.
///
/// The ink floor is not decoration. `determinism.rs` enrolled a text fixture
/// whose face was missing, hashed a blank page, and passed on every target for
/// months; a tiling guard has the same failure mode and it is worse here,
/// because a blank page tiles perfectly. Every fixture is held to a floor
/// before it is allowed to be evidence.
struct Fixture {
    name: &'static str,
    bytes: Vec<u8>,
    least_ink: usize,
}

fn fixtures() -> Vec<Fixture> {
    let mut out = Vec::new();
    let mut add = |name: &'static str, bytes: Vec<u8>, least_ink: usize| {
        out.push(Fixture {
            name,
            bytes,
            least_ink,
        });
    };

    add("text", text_page(Shape::PLAIN), 1_000);
    add("shading", shading_page(Shape::PLAIN), 11_000);
    add("image", image_page(Shape::PLAIN), 9_000);
    add("strokes", strokes_page(Shape::PLAIN), 2_500);
    // A quarter turn: the bitmap is 131x91 and the region indexes *that*.
    add("text rotated 90", text_page(Shape::turned(90)), 1_000);
    add("image rotated 90", image_page(Shape::turned(90)), 9_000);
    add(
        "strokes rotated 270",
        strokes_page(Shape::turned(270)),
        2_000,
    );
    // A crop box offset from the media box on both axes.
    add("text cropped", text_page(Shape::cropped(0)), 400);
    add("shading cropped", shading_page(Shape::cropped(0)), 6_000);
    // Both at once, which is where the two translations and the rotation have
    // to compose in the one right order.
    add(
        "strokes cropped and rotated",
        strokes_page(Shape::cropped(90)),
        700,
    );
    out
}

// ---- the comparator ---------------------------------------------------------

/// Where a tile and the page under it disagree, measured rather than described.
struct Divergence {
    /// How many pixels hold at least one differing component.
    pixels: usize,
    /// The largest difference in any component, 0 to 255.
    worst: u8,
    /// The first differing pixel, in the tile's own coordinates.
    at: (u32, u32),
    /// Whether every differing pixel is on the tile's outermost ring, which is
    /// the signature of an anti-aliasing seam rather than of a misplaced tile.
    all_on_the_seam: bool,
}

/// Compares one tile against the rectangle of `full` it claims to be.
///
/// Returns `None` when they are identical, which is what every call here
/// expects. The measurement exists because the alternative to reporting it is
/// loosening the assertion, and ruling 5 is not a claim that admits a budget:
/// a difference found here is a finding about the renderer.
fn compare(
    tile: &tinker_pdf::Bitmap,
    full: &tinker_pdf::Bitmap,
    at: PixelRegion,
) -> Option<Divergence> {
    let components = full.components();
    let mut pixels = 0usize;
    let mut worst = 0u8;
    let mut first = None;
    let mut all_on_the_seam = true;

    for y in 0..tile.height {
        for x in 0..tile.width {
            let a = (y as usize) * tile.stride + (x as usize) * components;
            let b = ((at.y + y) as usize) * full.stride + ((at.x + x) as usize) * components;
            let (Some(mine), Some(theirs)) = (
                tile.data.get(a..a + components),
                full.data.get(b..b + components),
            ) else {
                continue;
            };
            if mine == theirs {
                continue;
            }
            pixels += 1;
            for (m, t) in mine.iter().zip(theirs.iter()) {
                worst = worst.max(m.abs_diff(*t));
            }
            first.get_or_insert((x, y));
            let on_edge = x == 0 || y == 0 || x + 1 == tile.width || y + 1 == tile.height;
            all_on_the_seam &= on_edge;
        }
    }

    first.map(|at| Divergence {
        pixels,
        worst,
        at,
        all_on_the_seam,
    })
}

/// Renders `page` whole, then one tile at a time, and asserts each tile is the
/// rectangle of the whole it stands for.
///
/// The lattice runs past the page's far edges on purpose when the size does not
/// divide it: the last column and row are *partial* tiles, trimmed by
/// [`PixelRegion::clamped_to`], and they are where an off-by-one in the clamp
/// or in the translation lands.
#[track_caller]
fn tile_divergence(page: &Page, options: &RenderOptions, tile: u32, what: &str) -> Option<Report> {
    assert!(options.region.is_none(), "{what}: the whole page, to tile");
    let full = page.render(options);
    let (width, height) = page.pixel_size(options);
    assert_eq!(
        (full.width, full.height),
        (width, height),
        "{what}: pixel_size and render disagree about the page"
    );

    let mut covered = 0u64;
    let mut worst: Option<(Divergence, u32, u32, u64)> = None;
    let mut differing = 0u64;
    for top in (0..height).step_by(tile as usize) {
        for left in (0..width).step_by(tile as usize) {
            let asked = PixelRegion::new(left, top, tile, tile);
            let bitmap = page.render(&RenderOptions {
                region: Some(asked),
                ..options.clone()
            });
            let expected = asked.clamped_to(width, height);
            assert_eq!(
                (bitmap.width, bitmap.height),
                (expected.width, expected.height),
                "{what}: a {tile}px tile at ({left}, {top}) came back the wrong size"
            );
            assert_eq!(
                bitmap.format, full.format,
                "{what}: a tile changed the pixel format"
            );
            let area = u64::from(bitmap.width) * u64::from(bitmap.height);
            covered += area;

            if let Some(d) = compare(&bitmap, &full, expected) {
                differing += d.pixels as u64;
                if worst.as_ref().is_none_or(|(w, ..)| d.worst > w.worst) {
                    worst = Some((d, left, top, area));
                }
            }
        }
    }

    assert_eq!(
        covered,
        u64::from(width) * u64::from(height),
        "{what}: the {tile}px lattice did not cover the page exactly once"
    );

    worst.map(|(d, left, top, area)| Report {
        message: format!(
            "{what}: the {tile}px tile at ({left}, {top}) is not the page under \
             it -- {} of {area} pixels of that tile differ, worst component {} \
             levels, first at ({}, {}) in the tile, which is ({}, {}) on the \
             page; every difference {} on the tile's outer ring",
            d.pixels,
            d.worst,
            d.at.0,
            d.at.1,
            left + d.at.0,
            top + d.at.1,
            if d.all_on_the_seam { "is" } else { "is not" },
        ),
        worst: d.worst,
        pixels: differing,
        of: u64::from(width) * u64::from(height),
    })
}

/// What a whole lattice's worth of comparison came to.
struct Report {
    /// The worst tile, said in full.
    message: String,
    /// The largest difference in any component anywhere, 0 to 255.
    worst: u8,
    /// How many pixels of the page differed, summed over every tile.
    pixels: u64,
    /// How many there were.
    of: u64,
}

/// [`tile_divergence`], asserted away: the lattice **is** the page.
#[track_caller]
fn tiles_are_the_page(page: &Page, options: &RenderOptions, tile: u32, what: &str) {
    if let Some(report) = tile_divergence(page, options, tile, what) {
        panic!(
            "{}. Ruling 5 says this is byte-equal, so this is a finding about \
             the renderer and not a reason to add a budget.",
            report.message
        );
    }
}

/// Opens a fixture, checks it is drawing something, and hands over its page.
#[track_caller]
fn with_page(fixture: &Fixture, body: impl FnOnce(&Page)) {
    let document = Document::open(fixture.bytes.clone()).expect("the fixture opens");
    let page = document.page(0).expect("a page");
    let drawn = ink(&page.render(&RenderOptions::default()));
    assert!(
        drawn >= fixture.least_ink,
        "the {} fixture painted {drawn} pixels, fewer than the {} it is \
         supposed to -- a blank page tiles perfectly and proves nothing",
        fixture.name,
        fixture.least_ink
    );
    body(&page);
}

// ---- the guard --------------------------------------------------------------

/// **Ruling 5, at three tile sizes none of which divides the page.**
///
/// 91x131 pixels at scale 1. 64 leaves a 27-pixel column and a 3-pixel row;
/// 37 leaves 17 and 20; 23 leaves 22 and 16. Every fixture, every tile.
#[test]
fn every_tile_is_byte_equal_to_the_page_under_it() {
    let options = RenderOptions::default();
    for fixture in fixtures() {
        with_page(&fixture, |page| {
            for tile in [64u32, 37, 23] {
                tiles_are_the_page(page, &options, tile, fixture.name);
            }
        });
    }
}

/// **The same guard at scales other than 1**, where the page's pixel grid and
/// its point grid are no longer the same lattice.
///
/// A region that is ignored, halved or doubled whenever the scale is not 1 is a
/// defect the unscaled guard above cannot see, and scaling is the whole reason
/// a caller tiles: nobody tiles a 72 dpi page.
#[test]
fn tiles_at_other_scales_are_byte_equal_too() {
    for scale in [0.5f64, 2.0, 4.0] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        for fixture in fixtures() {
            with_page(&fixture, |page| {
                tiles_are_the_page(page, &options, 53, &format!("{} at {scale}x", fixture.name));
            });
        }
    }
}

/// **Where byte equality stops, measured rather than assumed.**
///
/// The two guards above run at scales whose binary representation lets the page
/// frame and the tile frame compute the same device coordinates. At other
/// scales they do not, and this is the only honest thing to do about it: state
/// the limit, bound it, and let it fail if it grows.
///
/// # What is actually different
///
/// A tile's transform is the page's with an integer number of pixels
/// subtracted from `e` and `f` — that is ruling 5's translated viewport, and it
/// is exact as arithmetic. It is not exact as *floating point*: a point's
/// device coordinate is `a·x + c·y + e`, and `fl(u + e)` and `fl(u + e − tx)`
/// are two roundings at two magnitudes. They differ in the last ulp or two,
/// which is 1e-14 of a pixel and reaches a byte only where the exact value sits
/// **on** one of the rasterizer's 1/256 steps — which "nice" geometry does
/// often, and which is why the effect shows at all rather than never.
///
/// The fix is not a tolerance and it is not available here: the only way to
/// make two frames agree to the bit is to compute in one of them, which means
/// the canvas carrying an origin through the sampler, the mesh and the image
/// run. That is a change to `tinker-pdf-raster`'s shape rather than to this
/// row, and until it happens this bound is what the guard honestly claims.
///
/// # Which sampler is left, which is the useful half of the measurement
///
/// The two paths that *could* have carried this and do not are the ones that
/// already defend against it, and the third is the one that does not:
///
/// - `fill` reduces every crossing to the nearest 1/256 unit by `+ half, >>
///   bits`, which is a `floor` of a shifted value and therefore commutes with
///   moving the shape a whole number of pixels;
/// - `draw_image` snaps its quad's corners to the same 1/256 grid before
///   rasterising, which `docs/features/rasterizer.md` records was put there for
///   exactly this reason — a cropped page and the page under it once disagreed
///   on 240 pixels;
/// - **a shading sampler does neither.** It evaluates the axial parameter from
///   `a·x + c·y + e` per pixel straight into a colour, with no grid between
///   the affine and the byte, so the last ulp reaches the output wherever the
///   parameter lands on a colour step.
///
/// The one lattice that diverges is a shading, which is what that predicts.
///
/// # The bound
///
/// Measured 15 September 2026 on this branch, over every fixture in this file
/// at 0.75, 1.5 and 3.0, tiling at 53 — thirty lattices. **Twenty-nine are
/// exact.** The whole of the divergence is:
///
/// | Fixture and scale | Pixels | Of | Worst |
/// | --- | ---: | ---: | ---: |
/// | `shading` at 3x | 1 | 107 289 | 1 |
///
/// The bound is set to exactly that and not above it. A slacker bound here
/// would be a budget, and the header of this file says why this property does
/// not get one: the numbers are the measurement, so if either moves the cause
/// is found rather than the constant raised. `WORST_LEVEL` is the claim that
/// matters most — a difference of more than one level is not two frames
/// rounding apart and would not be this.
///
/// *An earlier draft of this comment carried four rows, three of them `text`.
/// They were measured before `fill` stopped truncating, and the slope and
/// rounding fixes removed them; the table is re-measured here rather than
/// carried forward, which is the only reason it is a table and not a sentence.*
#[test]
fn at_the_scales_where_two_frames_round_apart_the_gap_is_one_level() {
    /// The largest difference in any component, anywhere, at any of these
    /// scales. One level of 255.
    const WORST_LEVEL: u8 = 1;
    /// How many pixels of one page's lattice may differ at all. One, which is
    /// what is measured.
    const MOST_PIXELS: u64 = 1;

    let mut table = Vec::new();
    for scale in [0.75f64, 1.5, 3.0] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        for fixture in fixtures() {
            with_page(&fixture, |page| {
                let what = format!("{} at {scale}x", fixture.name);
                let Some(report) = tile_divergence(page, &options, 53, &what) else {
                    return;
                };
                table.push(format!(
                    "  {what}: {} of {} pixels, worst {} levels",
                    report.pixels, report.of, report.worst
                ));
                assert!(
                    report.worst <= WORST_LEVEL && report.pixels <= MOST_PIXELS,
                    "{}. Past the measured bound of {MOST_PIXELS} pixels at \
                     {WORST_LEVEL} level: this is no longer the last ulp of an \
                     affine evaluated twice, and the cause has to be found \
                     rather than the bound raised.",
                    report.message
                );
            });
        }
    }
    // Not an assertion about the table's contents -- the bound above is that.
    // This is so a reader of a passing run can still see what the gap is.
    if !table.is_empty() {
        println!("the frames round apart here:\n{}", table.join("\n"));
    }
}

/// **A one-pixel lattice**, on the smallest fixture, because a tile whose
/// canvas is a single pixel is the degenerate case of every clamp and every
/// translation at once.
///
/// One fixture and a small page, since this is one render per pixel: the cost
/// is quadratic in the page and the property is not.
#[test]
fn single_pixel_tiles_are_byte_equal() {
    let document = Document::open(strokes_page(Shape::PLAIN)).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions {
        scale: 0.22,
        ..RenderOptions::default()
    };
    tiles_are_the_page(&page, &options, 1, "strokes at one pixel a tile");
}

// ---- the region's own contract ----------------------------------------------

/// A region covering the whole page is the same bitmap as no region at all.
///
/// The two reach the canvas by different spellings — `None` becomes the full
/// rectangle, a full rectangle stays itself — and if they ever disagreed, every
/// number in this file would be measuring the spelling rather than the page.
#[test]
fn a_region_covering_the_page_renders_the_page() {
    for fixture in fixtures() {
        with_page(&fixture, |page| {
            let options = RenderOptions::default();
            let whole = page.render(&options);
            let (width, height) = page.pixel_size(&options);
            let asked = page.render(&RenderOptions {
                region: Some(PixelRegion::new(0, 0, width, height)),
                ..options
            });
            assert_eq!(
                (asked.width, asked.height),
                (whole.width, whole.height),
                "{}: a full-page region changed the size",
                fixture.name
            );
            assert!(
                asked.data == whole.data,
                "{}: a full-page region is not the page",
                fixture.name
            );
            assert!(
                !asked
                    .warnings
                    .iter()
                    .any(|w| matches!(w, RenderWarning::RegionClamped { .. })),
                "{}: a region that fits was reported as clamped",
                fixture.name
            );
        });
    }
}

/// **A region hanging off the edge is trimmed, not moved** — and the trimmed
/// part is still byte-equal to the page under it.
///
/// This is the half of the guard a tiler actually depends on at the far edge,
/// and the one a clamp that *slid* the rectangle back onto the page would
/// silently break: it would return a full-sized bitmap of real pixels from the
/// wrong place, which looks like success at every call site.
#[test]
fn a_region_past_the_edge_is_trimmed_to_what_is_there() {
    for fixture in fixtures() {
        with_page(&fixture, |page| {
            let options = RenderOptions::default();
            let full = page.render(&options);
            let (width, height) = page.pixel_size(&options);

            let asked = PixelRegion::new(width - 9, height - 5, 40, 40);
            let bitmap = page.render(&RenderOptions {
                region: Some(asked),
                ..options
            });
            assert_eq!(
                (bitmap.width, bitmap.height),
                (9, 5),
                "{}: the overhang was not trimmed to what is on the page",
                fixture.name
            );
            let applied = asked.clamped_to(width, height);
            assert!(
                compare(&bitmap, &full, applied).is_none(),
                "{}: a trimmed region is not the corner of the page",
                fixture.name
            );
            assert!(
                bitmap.warnings.contains(&RenderWarning::RegionClamped {
                    requested: asked,
                    applied,
                }),
                "{}: the trim was performed without being named (ruling 10): {:?}",
                fixture.name,
                bitmap.warnings
            );
        });
    }
}

/// A region entirely off the page renders no pixels and says why.
///
/// The alternative — growing it to a single pixel so that the result is always
/// "a picture" — would return a pixel the caller did not ask for, and that
/// pixel is not the sub-rectangle of the full render their coordinates name.
/// An empty answer is the honest one, and `Bitmap::to_png` already calls a
/// bitmap with a zero dimension not a picture.
#[test]
fn a_region_off_the_page_renders_nothing_and_names_it() {
    let document = Document::open(strokes_page(Shape::PLAIN)).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let (width, height) = page.pixel_size(&options);

    // Three that miss the page, and one that asks for nothing. The last is
    // *not* a clamp: a caller who asks for a zero-wide rectangle is handed
    // exactly the rectangle they named, so there is nothing to report and
    // reporting it anyway would make the warning mean "the bitmap is empty"
    // rather than "your rectangle was trimmed".
    for (asked, trimmed) in [
        (PixelRegion::new(width, 0, 16, 16), true),
        (PixelRegion::new(0, height, 16, 16), true),
        (PixelRegion::new(width + 500, height + 500, 16, 16), true),
        (PixelRegion::new(4, 4, 0, 9), false),
    ] {
        let bitmap = page.render(&RenderOptions {
            region: Some(asked),
            ..options.clone()
        });
        assert!(
            bitmap.width == 0 || bitmap.height == 0,
            "{asked:?} is off the page and rendered {}x{}",
            bitmap.width,
            bitmap.height
        );
        assert!(bitmap.data.is_empty(), "{asked:?}: pixels out of nowhere");
        assert!(
            bitmap.to_png().is_none(),
            "{asked:?}: a PNG with no pixels is not a smaller PNG"
        );
        assert_eq!(
            bitmap
                .warnings
                .iter()
                .any(|w| matches!(w, RenderWarning::RegionClamped { .. })),
            trimmed,
            "{asked:?}: the trim and what was said about it disagree"
        );
    }
}

/// **A region on a rotated page is a corner of the picture a reader sees**, not
/// a corner of the upright sheet.
///
/// The one assertion in this file that a person can check by eye, and it is
/// here because tile equality alone is satisfied by a consistent mistake: a
/// region composed *before* `/Rotate` moves the viewport in user space, which
/// still yields a clean picture of the page and simply shows the wrong part of
/// it. The full render and the tiles would agree with each other about the
/// wrong part. Only a fixture that knows which part it asked for can tell, so
/// this one puts one mark on the page and asserts which quadrant of the
/// *displayed* bitmap it lands in, at all four rotations.
#[test]
fn a_rotated_page_region_is_the_corner_of_the_displayed_picture() {
    // The mark sits in the page's bottom-left corner in user space. Turned
    // clockwise for display, that corner travels: 0 leaves it bottom-left of
    // the bitmap, 90 sends it top-left, 180 top-right, 270 bottom-right.
    let expected = [
        (0i64, (0u32, 1u32)),
        (90, (0, 0)),
        (180, (1, 0)),
        (270, (1, 1)),
    ];
    for (rotation, (column, row)) in expected {
        let mut builder = DocumentBuilder::new();
        builder.add_page(PAGE_W, PAGE_H, |page| {
            page.fill_rect(0.0, 0.0, 18.0, 18.0, 0.0);
        });
        let bytes = shaped(builder.finish(), Shape::turned(rotation));
        let document = Document::open(bytes).expect("it opens");
        let page = document.page(0).expect("a page");
        let options = RenderOptions::default();
        let (width, height) = page.pixel_size(&options);
        let (half_w, half_h) = (width / 2, height / 2);

        for quadrant_column in 0..2u32 {
            for quadrant_row in 0..2u32 {
                let region = PixelRegion::new(
                    quadrant_column * half_w,
                    quadrant_row * half_h,
                    half_w,
                    half_h,
                );
                let bitmap = page.render(&RenderOptions {
                    region: Some(region),
                    ..options.clone()
                });
                let drawn = ink(&bitmap);
                let wanted = quadrant_column == column && quadrant_row == row;
                assert_eq!(
                    drawn > 0,
                    wanted,
                    "/Rotate {rotation}: the quadrant at ({quadrant_column}, \
                     {quadrant_row}) of the {width}x{height} picture has \
                     {drawn} painted pixels and the mark belongs in \
                     ({column}, {row}) -- a region composed before the \
                     rotation rather than after it lands in a different \
                     corner and still looks like the page"
                );
            }
        }
    }
}

/// **A crop box offset from the media box does not shift the region.**
///
/// The page's own translation and the region's compose, and a sign error in
/// either produces a page that looks entirely reasonable. The mark is placed at
/// the crop box's own bottom-left corner, so it must land at the bottom-left of
/// the *bitmap* and therefore in the bottom-left quadrant's region.
#[test]
fn a_crop_box_does_not_move_the_region() {
    let crop = (11.0, 17.0, 79.0, 109.0);
    let mut builder = DocumentBuilder::new();
    builder.add_page(PAGE_W, PAGE_H, |page| {
        page.set_crop_box(crop.0, crop.1, crop.2, crop.3);
        page.fill_rect(crop.0, crop.1, 14.0, 14.0, 0.0);
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let page = document.page(0).expect("a page");
    assert_eq!(page.crop_box(), crop, "the crop box reached the page");

    let options = RenderOptions::default();
    let (width, height) = page.pixel_size(&options);
    let (half_w, half_h) = (width / 2, height / 2);

    for column in 0..2u32 {
        for row in 0..2u32 {
            let bitmap = page.render(&RenderOptions {
                region: Some(PixelRegion::new(
                    column * half_w,
                    row * half_h,
                    half_w,
                    half_h,
                )),
                ..options.clone()
            });
            let drawn = ink(&bitmap);
            let wanted = column == 0 && row == 1;
            assert_eq!(
                drawn > 0,
                wanted,
                "a mark at the crop box's own corner: quadrant ({column}, {row}) \
                 of the {width}x{height} picture has {drawn} painted pixels"
            );
        }
    }
}

/// **A region is a rectangle of the page's *clamped* pixel lattice**, which is
/// the one thing `Page::pixel_size` exists to say and the one thing no other
/// fixture here can see.
///
/// Every page above is small, so `page_scale` returns the scale it was given
/// and the lattice is just the page. This one is not: 9 000 by 8 000 points at
/// scale 1 is 72 million pixels, past `MAX_PAGE_PIXELS`, so the render is
/// scaled down to fit (ruling 2 — a whole page smaller beats a fragment at
/// full size) and the lattice a caller must tile is the **smaller** one, which
/// is 8 688 by 7 723 and not 8 689 by 7 724.
///
/// **The one-pixel difference is the whole fixture, and the shape is
/// deliberate.** `page_pixels` rounds each side outward *and then* shrinks the
/// pair to fit the ceiling, so the lattice is not `ceil(side × scale)` and a
/// caller who computes it that way is one row and one column too big. They
/// would ask for a strip that is not on the page, be handed empty bitmaps, and
/// assemble a page with a missing edge — which is exactly what
/// `Page::pixel_size` exists to prevent, and what nothing in the tree could see
/// before this fixture: injected into `pixel_size`, it failed **nothing**
/// anywhere in the workspace. With this fixture it fails exactly one test, and
/// that one is this.
///
/// The sides are coprime-ish on purpose. A square page, or any page whose
/// clamped area is a perfect square, shrinks by a factor that divides its own
/// sides exactly — 9 000 square shrinks by exactly 8192/9000 — and then the two
/// spellings agree and the fixture proves nothing. That was the first draft.
///
/// It costs almost nothing to run: every render here is a region of a few
/// pixels, and a region-sized canvas is all that is ever allocated.
#[test]
fn a_region_indexes_the_clamped_lattice_of_an_enormous_page() {
    const WIDE: f64 = 9_000.0;
    const TALL: f64 = 8_000.0;

    let mut builder = DocumentBuilder::new();
    builder.add_page(WIDE, TALL, |page| {
        // Ink in both far corners of user space, so neither the top-left nor
        // the bottom-right of the picture is blank by construction.
        page.fill_rect(0.0, 0.0, 400.0, 400.0, 0.0);
        page.fill_rect(WIDE - 400.0, TALL - 400.0, 400.0, 400.0, 0.0);
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let (width, height) = page.pixel_size(&options);

    assert!(
        u64::from(width) * u64::from(height) <= 1 << 26,
        "pixel_size reported {width}x{height}, which is past the page-pixel          ceiling the renderer actually applies"
    );
    assert!(
        width < WIDE as u32 && height < TALL as u32,
        "pixel_size reported {width}x{height} for a {WIDE}x{TALL}pt page at          scale 1: that is the unclamped answer, and a caller tiling it would          ask for rows and columns the render does not have"
    );

    // The last pixel of the reported lattice is on the page: a full tile, no
    // trim, and the scale-down named as ruling 2 requires.
    let corner = PixelRegion::new(width - 1, height - 1, 1, 1);
    let last = page.render(&RenderOptions {
        region: Some(corner),
        ..options.clone()
    });
    assert_eq!(
        (last.width, last.height),
        (1, 1),
        "the last pixel pixel_size names is not on the page"
    );
    assert!(
        !last
            .warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::RegionClamped { .. })),
        "the last pixel of the lattice was reported as trimmed: {:?}",
        last.warnings
    );
    assert!(
        last.warnings
            .iter()
            .any(|w| matches!(w, RenderWarning::PageScaledDown { .. })),
        "a 72-megapixel page rendered without saying it had been scaled down          (ruling 2, ruling 10): {:?}",
        last.warnings
    );

    // And the pixel after it is not, which is what makes the bound exact
    // rather than merely large enough.
    let past = page.render(&RenderOptions {
        region: Some(PixelRegion::new(width, height, 1, 1)),
        ..options.clone()
    });
    assert!(
        past.width == 0 || past.height == 0,
        "a region one pixel past the lattice rendered {}x{}",
        past.width,
        past.height
    );
}

/// A page whose only ink comes from an annotation appearance stream (12.5.5).
///
/// Hand-written rather than built, because the builder writes no `/Annots` and
/// `annotation_appearances.rs` writes its fixtures the same way for the same
/// reason. The appearance is a curve and a diagonal so that its edges are
/// anti-aliased, and its `/BBox` is fitted onto a `/Rect` of a different size
/// so the annotation's own transform is not the identity.
fn annotated_page() -> Vec<u8> {
    let stream = "0.1 0.2 0.8 rg 0 0 m 30 44 l 60 0 l f \
                  0 0 0 RG 2 w 3 40 m 30 6 57 46 57 8 c S";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}]\n\
   /Annots [5 0 R 7 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [7 11 84 79]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 60 44]\n\
   /Length {len} >>\nstream\n{stream}\nendstream\nendobj\n\
7 0 obj\n<< /Type /Annot /Subtype /Square /Rect [21 86 70 124]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        len = stream.len()
    )
    .into_bytes()
}

/// The annotation layer is inside the region too.
///
/// Annotations are drawn after the content through the same renderer, so this
/// could only fail by their being given a transform of their own — which is a
/// mistake nothing else in this file would see, since every other fixture draws
/// its ink from a content stream.
#[test]
fn annotations_are_tiled_with_the_page() {
    let document = Document::open(annotated_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let drawn = ink(&page.render(&options));
    assert!(
        drawn > 2_000,
        "the annotation fixture painted {drawn} pixels and draws nothing else"
    );
    tiles_are_the_page(&page, &options, 29, "annotations");
}
