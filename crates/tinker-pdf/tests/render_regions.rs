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
//! - **One-bit images**, five ways at four sizes (`render_support::bilevel_page`,
//!   the page `determinism.rs` fingerprints as `bilevel`): a fax, a palette,
//!   an inverted `/Decode` and a stencil reach the sampler by the bilevel
//!   paths, and minified they sample partial samples at a tile's edge.
//! - **JPEGs decoded at the size they are drawn**
//!   (`render_support::jpeg_minified_page`, the page `determinism.rs`
//!   fingerprints as `jpeg_minified`): a baseline and a progressive frame at a
//!   half, a quarter and an eighth, flipped, turned and clipped. A draw asks
//!   for its size in the page frame, so a tile asks what the page asks and is
//!   decoded at the same scale, and the reduced grid is cut where the image
//!   ends in a tile as on the page.
//! - **JPEG 2000 codestreams decoded with the levels their size drops**
//!   (`render_support::jpx_minified_page`, the page `determinism.rs`
//!   fingerprints as `jpx_minified`): both wavelets, odd sizes and subsampled
//!   components, with one level dropped and two, flipped, turned, sheared and
//!   clipped. A tile asks for the page frame's size, so it drops the levels
//!   the page drops, and the reduced grid is cut where the image ends in a
//!   tile as on the page.
//! - **Canvases that stand somewhere in the page**: a non-isolated group under
//!   a blend mode, a knockout group and a luminosity soft mask; a tiling
//!   pattern's cell; a Gouraud mesh's own buffer. Each is a buffer the size of
//!   a rectangle of the page rather than of the page, so each is a second place
//!   a tile's arithmetic could part from the page's — and two of them did.
//! - **The stroker's whole pen**, the page of the `strokes` fingerprint: every
//!   cap and join, a miter past its limit, dashes and an even-odd star. Their
//!   outlines overlap by construction, so the filler's windings pass one
//!   wherever a pen turns.
//! - **Scales other than 1.0**, because a region that is silently ignored
//!   whenever the scale is not 1 is a defect no unscaled fixture can see.
//!
//! # Does it hold at every scale? For every fixture here, since September 2026
//!
//! Every fixture, every tile size, down to a one-pixel lattice, at 0.5, 0.75,
//! 1, 1.5, 2, 3 and 4. Until September 2026 the scales 0.75, 1.5 and 3 carried
//! a measured exception — one shading pixel at 3x, one level of 255 — and a
//! stroked rectangle found elsewhere carried another at 1x, both because a tile
//! had a frame of its own an ulp away from the page's.
//! [`tiles_at_the_scales_where_two_frames_rounded_apart_are_byte_equal`] says
//! why that stopped being true, and asserts equality where it used to assert a
//! bound; [`a_stroke_whose_corners_sit_on_a_sub_scanline_tiles_exactly_at_1x`]
//! is the stroke.
//!
//! **It does not hold for every page**, and the shape that parts them is
//! geometric rather than a scale: two images that overlap outside a tile and
//! abut inside it are one image run in the tile and two on the page.
//! [`an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`]
//! pins it as measured — 40 pixels, 63 levels, at 1x — and ruling 5 and a
//! ROADMAP row carry it until it is gone.
//!
//! # Bands on threads
//!
//! Every guard above draws one tile at a time on the test's own thread.
//! [`bands_drawn_on_two_three_four_and_seven_threads_are_the_page`] draws a
//! caller's bands: each fixture cut into 2, 3, 4 and 7 horizontal bands, all
//! drawn at once on as many scoped threads through one `Page`, stitched back
//! together and compared with the page drawn whole, byte for byte. The
//! engine's own bands of the same pages — `region_support`'s, which this file
//! and `guards.rs` share — are A9's band half,
//! `guards.rs`'s `a9_band_renders_are_the_serial_page_at_2_3_4_and_7_threads`.
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
//!
//! **The canvases-in-the-page fixtures found two more, and both were about
//! tiles**, found the day the tile stopped having a frame of its own and could
//! therefore be wrong only by deciding something differently from the page:
//!
//! 3. **A tiling lattice was indexed from the tile's own rectangle.** A cell's
//!    buffer is its box rounded outward, landed at a rounded offset, so a cell
//!    whose box stops just short of a rectangle still puts anti-aliased edge
//!    inside it; the page's lattice held that cell and a tile's did not. Ten
//!    levels on one pixel at 1x.
//! 4. **A mesh spread colour into its fringe from the tile's pixels only.** The
//!    spread reads two pixels of neighbours, and a silhouette pixel on a tile's
//!    edge had fewer of them than the same pixel on the page. Ten levels at 1x,
//!    thirty at 4x.
//!
//! Both are now decided from rectangles the renderer keeps in the page's frame
//! (`Bounds` in `tinker-pdf-render`), which also puts the lattice's and the
//! mesh's budgets on the page's side of the decision.

mod written_support;

use std::sync::Barrier;

use tinker_pdf::{
    Color, Document, DocumentBuilder, ForcedColours, HardEdge, Page, PixelRegion, RenderOptions,
    RenderWarning,
};

mod region_support;
mod render_support;
use region_support::{
    annotated_page, fixtures, framed_page, image_run_page, raw_page, shaped, strokes_page, Fixture,
    Shape, PAGE_H, PAGE_W,
};
use render_support::ink;

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
fn tile_divergence(page: &Page, options: &RenderOptions, tile: u32, what: &str) -> Option<String> {
    tile_divergence_through(page, options, tile, what, &|asked| page.render(asked))
}

/// [`tile_divergence`] with the tiles drawn by `draw` rather than by
/// [`Page::render`] — a [`tinker_pdf::DisplayList`]'s replay, say — and the
/// page they are compared with still drawn directly.
#[track_caller]
fn tile_divergence_through(
    page: &Page,
    options: &RenderOptions,
    tile: u32,
    what: &str,
    draw: &dyn Fn(&RenderOptions) -> tinker_pdf::Bitmap,
) -> Option<String> {
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
    let whole = u64::from(width) * u64::from(height);
    for top in (0..height).step_by(tile as usize) {
        for left in (0..width).step_by(tile as usize) {
            let asked = PixelRegion::new(left, top, tile, tile);
            let bitmap = draw(&RenderOptions {
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
        covered, whole,
        "{what}: the {tile}px lattice did not cover the page exactly once"
    );

    // The worst tile said in full, and the lattice's total beside it.
    worst.map(|(d, left, top, area)| {
        format!(
            "{what}: the {tile}px tile at ({left}, {top}) is not the page under \
             it -- {} of {area} pixels of that tile differ, worst component {} \
             levels, first at ({}, {}) in the tile, which is ({}, {}) on the \
             page; every difference {} on the tile's outer ring; {differing} of \
             the page's {whole} pixels differ across the lattice",
            d.pixels,
            d.worst,
            d.at.0,
            d.at.1,
            left + d.at.0,
            top + d.at.1,
            if d.all_on_the_seam { "is" } else { "is not" },
        )
    })
}

/// [`tile_divergence`], asserted away: the lattice **is** the page.
#[track_caller]
fn tiles_are_the_page(page: &Page, options: &RenderOptions, tile: u32, what: &str) {
    if let Some(message) = tile_divergence(page, options, tile, what) {
        panic!(
            "{message}. Ruling 5 says this is byte-equal, so this is a finding \
             about the renderer and not a reason to add a budget."
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

/// **A page under a coarse flatness tiles as the page does** (10.7.2): curves
/// filled, stroked and used as a clip under `/FL` from a `gs` and from `i`,
/// at the three tile sizes and at 2x. The tolerance is a device-pixel measure
/// of device geometry, decided per paint and never per tile, so each tile is
/// its rectangle of the page byte for byte.
#[test]
fn flattened_tiles_are_the_flattened_page() {
    let content = "/F gs 0.2 0.4 0.8 rg 45 120 m 85 120 85 70 45 70 c 5 70 5 120 45 120 c f \
                   9 i 0 0 1 RG 3 w 10 10 m 80 60 10 60 80 10 c S \
                   q 45 40 m 75 40 75 5 45 5 c 15 5 15 40 45 40 c W n \
                   0.9 0.2 0.1 rg 0 0 91 131 re f Q";
    let bytes = raw_page(content, "<< /ExtGState << /F << /FL 7 >> >> >>", &[]);
    let document = Document::open(bytes).expect("the page opens");
    let page = document.page(0).expect("a page");
    for scale in [1.0f64, 2.0] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        for tile in [64u32, 37, 23] {
            tiles_are_the_page(&page, &options, tile, &format!("flatness at {scale}x"));
        }
    }
}

/// **A page under stroke adjustment tiles as the page does** (10.7.5): thin
/// strokes at fractional offsets, horizontal, vertical, diagonal and curved,
/// and a dashed one, under `/SA true` from a `gs`, at the three tile sizes and
/// at 2x. The adjustment moves the page's device points to the page's pixel
/// grid, the frame a tile's canvas stands in, so each tile is its rectangle of
/// the page byte for byte.
#[test]
fn stroke_adjusted_tiles_are_the_adjusted_page() {
    let content = "/A gs 0 0 1 RG 1.3 w 5.3 7.7 m 85.4 7.7 l S \
                   2.4 w 20.2 10.1 m 20.2 120.6 l S \
                   0.7 w 10.3 20.6 m 80.1 110.9 l S \
                   1.6 w 45.2 60.4 m 85 60 85 100 45 100 c S \
                   0 w [3.3 1.7] 0.4 d 3.1 125.6 m 88.7 125.6 l S";
    let bytes = raw_page(content, "<< /ExtGState << /A << /SA true >> >> >>", &[]);
    let document = Document::open(bytes).expect("the page opens");
    let page = document.page(0).expect("a page");
    for scale in [1.0f64, 2.0] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        for tile in [64u32, 37, 23] {
            tiles_are_the_page(
                &page,
                &options,
                tile,
                &format!("stroke adjustment at {scale}x"),
            );
        }
    }
}

/// **A blue-first page tiles as the page does**: every fixture, at the three
/// tile sizes, in `Bgra8` and in `Bgr8`, each tile the page's rectangle in that
/// order byte for byte. The order is made once, at the end, per pixel, from
/// the canvas a tile and the page both composite red first, so there is
/// nothing a tile could decide differently; this holds it.
#[test]
fn bgra_tiles_are_the_bgra_page() {
    for format in [
        tinker_pdf::PixelFormat::Bgra8,
        tinker_pdf::PixelFormat::Bgr8,
    ] {
        let options = RenderOptions {
            format,
            ..RenderOptions::default()
        };
        for fixture in fixtures() {
            with_page(&fixture, |page| {
                for tile in [64u32, 37, 23] {
                    tiles_are_the_page(
                        page,
                        &options,
                        tile,
                        &format!("{} in {format:?}", fixture.name),
                    );
                }
            });
        }
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

/// **The scales where two frames used to round apart — byte-equal too.**
///
/// The two guards above run at scales whose binary representation let a page
/// frame and a tile frame compute the same device coordinates. At 0.75, 1.5 and
/// 3 they did not, and until September 2026 this test was a *bound* rather than
/// an equality: one shading pixel at 3x, one level of 255, measured and pinned.
///
/// The cause was that a tile had a frame of its own: the page's transform with
/// a whole number of pixels taken off `e` and `f`. That is exact as arithmetic
/// and not as floating point — a point's device coordinate is `a·x + c·y + e`,
/// and `fl(u + e)` and `fl(u + e − tx)` are two roundings at two magnitudes —
/// and the last ulp reached a byte wherever the exact value sat on one of the
/// rasterizer's grids. A shading sampler has no grid to absorb it at all, which
/// is why the one lattice that diverged was a shading.
///
/// **There is one frame now.** The page is drawn through the one transform
/// whatever part of it is asked for, and the canvas says which pixels of that
/// picture it holds (`Canvas::origin`); a group's buffer, a soft mask's and a
/// mesh's stand in the same frame. Every coordinate the renderer computes is
/// the same number in a tile as in the page, so the only arithmetic that
/// differs is an integer subtraction where a pixel is written. So this is the
/// same assertion as the two above: no bound, no table, no budget.
#[test]
fn tiles_at_the_scales_where_two_frames_rounded_apart_are_byte_equal() {
    for scale in [0.75f64, 1.5, 3.0] {
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

/// **A stroked rectangle at 1x, whose corners sit on a sub-scanline.**
///
/// Ruling 5 used to say `fill` was immune to the two-frames rounding, and it
/// was — across. A crossing's `x` is reduced to the nearest 1/256 of a pixel,
/// which absorbs an ulp; an edge's first sub-scanline is `ceil(y × 16)`, which
/// does not. A stroker's miter corner computed in two frames came out an ulp
/// either side of a sub-scanline, and the page and the 60 x 40 tile over this
/// frame differed on four corner pixels by fifteen levels — one sixteenth of a
/// pixel's coverage — **at scale 1**, where every other fixture here was exact.
/// With one frame the corner is one number, and `ceil` takes it one way.
///
/// The region is the one `render_parts.rs` asked for — the annotation's
/// `/Rect` — and then two lattices over the whole page, one of which puts a
/// tile edge through each side of the frame.
#[test]
fn a_stroke_whose_corners_sit_on_a_sub_scanline_tiles_exactly_at_1x() {
    let document = Document::open(framed_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let full = page.render(&options);
    let drawn = ink(&full);
    assert!(
        drawn > 600,
        "the frame fixture painted {drawn} pixels, fewer than its stroke covers"
    );

    let region = PixelRegion::new(10, 50, 60, 40);
    let tile = page.render(&RenderOptions {
        region: Some(region),
        ..options.clone()
    });
    if let Some(d) = compare(&tile, &full, region) {
        panic!(
            "the 60 x 40 tile over the frame is not the page under it: {} pixels \
             differ, worst component {} levels, first at ({}, {}) in the tile",
            d.pixels, d.worst, d.at.0, d.at.1
        );
    }
    for size in [60u32, 17] {
        tiles_are_the_page(&page, &options, size, "the stroked frame at 1x");
    }
}

/// **Ruling 5's one named exception, pinned to what it measures.**
///
/// Whether an image joins the run held back so that abutting images do not
/// conflate is decided by whether it overlaps what the run holds, and the run
/// holds fragments over the canvas only. On the page the third image overlaps
/// the second, so the run of the first two is composited before the third is
/// drawn and the seam between the first and the third conflates. In the tile
/// over the top half the second image is off the canvas, the third joins the
/// first's run, and the seam does not conflate. Measured 2 October 2026: 40
/// pixels — column 40, rows 0 to 39 — differ by 63 levels at 1x.
///
/// Pinned exactly rather than bounded, so the day this changes in either
/// direction the test says so: if it fails because the tile *is* the page,
/// the ROADMAP row "A tile byte-equal to the page when an image run's overlap
/// falls outside it" is done — delete it and ruling 5's paragraph, and turn
/// this into a byte-equality assertion.
#[test]
fn an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception() {
    let document = Document::open(image_run_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let full = page.render(&options);
    assert!(
        ink(&full) > 4_000,
        "the three images cover most of the page"
    );
    let region = PixelRegion::new(0, 0, 100, 50);
    let tile = page.render(&RenderOptions {
        region: Some(region),
        ..options
    });
    let d = compare(&tile, &full, region)
        .expect("the exception is gone: delete its ROADMAP row and assert byte equality here");
    let components = full.components();
    let mut columns = std::collections::BTreeSet::new();
    for y in 0..tile.height as usize {
        for x in 0..tile.width as usize {
            let at = y * tile.stride + x * components;
            let theirs = y * full.stride + x * components;
            if tile.data.get(at..at + components) != full.data.get(theirs..theirs + components) {
                columns.insert(x);
            }
        }
    }
    assert_eq!(
        (
            d.pixels,
            d.worst,
            d.at,
            columns.into_iter().collect::<Vec<_>>()
        ),
        (40, 63, (40, 0), vec![40]),
        "the run decided in the canvas is the named seam and nothing else"
    );
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
        let bytes = shaped(
            built(
                "a rotated page region is the corner of the displayed picture",
                builder,
            ),
            Shape::turned(rotation),
        );
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
    let document =
        Document::open(built("a crop box does not move the region", builder)).expect("it opens");
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
    let document = Document::open(built(
        "a region indexes the clamped lattice of an enormous page",
        builder,
    ))
    .expect("it opens");
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

/// **A `/NoZoom` page tiles as the page does** (RC-13.4): an annotation
/// flagged `/NoZoom` and `/NoRotate` on a page turned a quarter, at
/// magnifications 2 and 0.5, where it is drawn at half and at twice its size
/// about its fixed corner and upright, is placed in the page's frame from the
/// magnification and `/Rotate` alone — never from a tile — so at four tile
/// sizes, none of which divides the page, each tile is the page under it,
/// directly and from a display list.
#[test]
fn a_no_zoom_page_tiles_as_the_page_does() {
    let document = Document::open(render_support::placed_page(
        90,
        4 | 8 | 16,
        render_support::PLACED_RECT,
        None,
    ))
    .expect("it opens");
    let page = document.page(0).expect("a page");
    let list = page.display_list();
    for magnification in [2.0, 0.5] {
        let options = RenderOptions {
            magnification,
            ..RenderOptions::default()
        };
        let drawn = ink(&page.render(&options));
        assert!(
            drawn > 700,
            "the placed page painted {drawn} pixels at magnification {magnification}"
        );
        for tile in [64, 37, 23, 9] {
            tiles_are_the_page(
                &page,
                &options,
                tile,
                &format!("a /NoZoom page at magnification {magnification}"),
            );
        }
        let what = format!("a /NoZoom page replayed at magnification {magnification}");
        if let Some(message) =
            tile_divergence_through(&page, &options, 37, &what, &|asked| list.render(asked))
        {
            panic!("{message}: a tile of the recording is not the page under it");
        }
    }
}

/// **A retained page tiles as the page does.** Every fixture here and the
/// annotated page, recorded once with [`Page::display_list`], then drawn a
/// tile at a time *from the recording* at two scales, each tile byte-equal to
/// its rectangle of a direct render of the whole page.
///
/// Two claims at once, which is the point of putting them together: the
/// replay is the page (the retained-page row), and a region of a replay is a
/// region of the page (ruling 5) — so a viewer that records a page once and
/// tiles it while zooming is drawing the same pixels a direct render would.
/// The annotated page is here because no determinism fixture draws an
/// annotation, and a replay that forgot the appearance's resource scope would
/// pass every other test in the tree.
#[test]
fn a_display_list_tiles_as_the_page_does() {
    let annotated = Document::open(annotated_page()).expect("it opens");
    let annotated = annotated.page(0).expect("a page");
    let check = |page: &Page, name: &str| {
        let list = page.display_list();
        for scale in [1.0f64, 1.5] {
            let options = RenderOptions {
                scale,
                ..RenderOptions::default()
            };
            let what = format!("{name} replayed at {scale}x");
            if let Some(message) =
                tile_divergence_through(page, &options, 37, &what, &|asked| list.render(asked))
            {
                panic!("{message}: a tile of the recording is not the page under it");
            }
        }
    };
    check(&annotated, "annotations");
    for fixture in fixtures() {
        with_page(&fixture, |page| check(page, fixture.name));
    }
}

/// **A forced-colour tile is the forced-colour page under it.** Every fixture
/// and the annotated page in two colours, with fill-to-stroke and without, at
/// two scales, a tile at a time, directly and from a display list: the
/// coverage is drawn through the one frame every render shares and the tint
/// is a function of one pixel, so each tile is its rectangle of the whole
/// forced render byte for byte (ruling 5).
#[test]
fn a_forced_colour_tile_is_the_page_under_it() {
    let annotated = Document::open(annotated_page()).expect("it opens");
    let annotated = annotated.page(0).expect("a page");
    let check = |page: &Page, name: &str| {
        let list = page.display_list();
        for (scale, tile) in [(1.0f64, 37u32), (1.5, 53)] {
            for fill_to_stroke in [false, true] {
                let options = RenderOptions {
                    scale,
                    forced_colours: Some(ForcedColours {
                        background: Color::rgb(16, 24, 64),
                        foreground: Color::rgb(255, 240, 160),
                        fill_to_stroke,
                    }),
                    ..RenderOptions::default()
                };
                let what = format!("{name} in forced colours at {scale}x, {fill_to_stroke}");
                tiles_are_the_page(page, &options, tile, &what);
                if let Some(message) =
                    tile_divergence_through(page, &options, tile, &what, &|asked| {
                        list.render(asked)
                    })
                {
                    panic!("{message}: a tile of the recording is not the page under it");
                }
            }
        }
    };
    check(&annotated, "annotations");
    for fixture in fixtures() {
        with_page(&fixture, |page| check(page, fixture.name));
    }
}

/// **A tile at every level and rule is the page under it** (ruling 5,
/// RC-11.5): every fixture, its graphics and its text at levels apart and
/// together — 4 and 8, 8 and 2, 1 and 1, 7 and 3 — at level 0 under the
/// centre rule and under the any-part rule, each with the other class
/// anti-aliased, and with anti-aliasing off under all three rules, at two
/// tile sizes and two scales: each tile its rectangle of the page, byte for
/// byte. A level is a function of one pixel's coverage; the centre rule reads
/// one sample of the page's device pixels and the any-part rule the shape's
/// own edges there, neither a function of where a tile stands.
#[test]
fn a_tile_at_every_level_and_rule_is_the_page_under_it() {
    let settings = [
        (4u8, 8u8, HardEdge::Half, true),
        (8, 2, HardEdge::Half, true),
        (1, 1, HardEdge::Half, true),
        (7, 3, HardEdge::Half, true),
        (0, 8, HardEdge::Centre, true),
        (8, 0, HardEdge::AnyPart, true),
        (0, 0, HardEdge::AnyPart, true),
        (8, 8, HardEdge::Centre, false),
        (8, 8, HardEdge::AnyPart, false),
    ];
    for fixture in fixtures() {
        with_page(&fixture, |page| {
            for (index, (graphics_bits, text_bits, hard_edge, antialias)) in
                settings.into_iter().enumerate()
            {
                let (scale, tile) = if index % 2 == 0 { (1.0, 37) } else { (1.5, 53) };
                let options = RenderOptions {
                    scale,
                    antialias,
                    graphics_bits,
                    text_bits,
                    hard_edge,
                    ..RenderOptions::default()
                };
                let what = format!(
                    "{} with graphics at {graphics_bits} bits, text at {text_bits},                      {hard_edge:?}, anti-aliasing {antialias}, at {scale}x",
                    fixture.name
                );
                tiles_are_the_page(page, &options, tile, &what);
            }
        });
    }
}

// ---- bands on threads -------------------------------------------------------

/// `count` horizontal bands over a `width` by `height` bitmap, top to bottom:
/// the whole width each, cut at row `height × i / count`, so no two heights
/// differ by more than a row and the cuts fall wherever that arithmetic puts
/// them, through a glyph or a shading's steps as readily as between them.
fn bands(width: u32, height: u32, count: u32) -> Vec<PixelRegion> {
    (0..count)
        .map(|band| {
            let top = height * band / count;
            let bottom = height * (band + 1) / count;
            PixelRegion::new(0, top, width, bottom - top)
        })
        .collect()
}

/// Draws each of `bands` of `page` on a scoped thread of its own, and hands
/// them back in the order asked, whatever order the threads finished in.
///
/// Every thread waits at a barrier until all of them are running, so the
/// bands are drawn at the same moment, through the one `Page` and the one
/// `Document` behind it, rather than one after another as each thread
/// happens to start.
fn drawn_on_threads(
    page: &Page,
    options: &RenderOptions,
    bands: &[PixelRegion],
) -> Vec<tinker_pdf::Bitmap> {
    let start = Barrier::new(bands.len());
    std::thread::scope(|scope| {
        let threads: Vec<_> = bands
            .iter()
            .map(|&band| {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    page.render(&RenderOptions {
                        region: Some(band),
                        ..options.clone()
                    })
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().expect("a band's thread panicked"))
            .collect()
    })
}

/// **A9 for bands: a page drawn as 2, 3, 4 and 7 horizontal bands, each on a
/// thread of its own and all at once, then stitched back together row after
/// row, is the page drawn whole on one thread, byte for byte.**
///
/// The guards above hold one tile at a time against the page, on the test's
/// own thread. A caller who bands a page to use more cores asks for what none
/// of them does: several regions of one `Page` drawn at the same moment,
/// through one shared `Document`. So the bands here are drawn concurrently
/// ([`drawn_on_threads`]), and what is compared is the stitched buffer
/// against the serial render's, so a band that came back a row short or long
/// fails too, and not only a band whose pixels moved.
///
/// Every page of [`fixtures`], and the two pages held to their tiles by tests
/// of their own: the annotated page, whose ink is all annotation, and the
/// stroked frame whose corners sit on a sub-scanline. Not the page of
/// [`an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`]:
/// that is ruling 5's named exception, which bands meet as tiles do (in two
/// bands the top one differs from the page on 40 pixels by 63 levels,
/// measured 25 October 2026), and that test pins it.
#[test]
fn bands_drawn_on_two_three_four_and_seven_threads_are_the_page() {
    let options = RenderOptions::default();
    let mut pages = fixtures();
    pages.push(Fixture {
        name: "annotations",
        bytes: annotated_page(),
        least_ink: 2_000,
    });
    pages.push(Fixture {
        name: "the stroked frame",
        bytes: framed_page(),
        least_ink: 600,
    });
    for fixture in pages {
        with_page(&fixture, |page| {
            let full = page.render(&options);
            for count in [2u32, 3, 4, 7] {
                let what = format!("{} in {count} bands", fixture.name);
                let cut = bands(full.width, full.height, count);
                let drawn = drawn_on_threads(page, &options, &cut);

                let mut stitched = Vec::with_capacity(full.data.len());
                for (band, bitmap) in cut.iter().zip(&drawn) {
                    assert_eq!(
                        (bitmap.width, bitmap.height),
                        (band.width, band.height),
                        "{what}: the band from row {} came back the wrong size",
                        band.y
                    );
                    assert_eq!(
                        bitmap.format, full.format,
                        "{what}: a band changed the pixel format"
                    );
                    stitched.extend_from_slice(&bitmap.data);
                }
                if stitched == full.data {
                    continue;
                }

                // Which band parted from the page, said in full, and whether
                // it still does drawn alone: a band on one thread is a tile,
                // which is ruling 5's to answer for, not the threads'.
                let parted = cut
                    .iter()
                    .zip(&drawn)
                    .find_map(|(band, bitmap)| compare(bitmap, &full, *band).map(|d| (band, d)));
                match parted {
                    Some((band, d)) => {
                        let alone = page.render(&RenderOptions {
                            region: Some(*band),
                            ..options.clone()
                        });
                        let why = match compare(&alone, &full, *band) {
                            None => {
                                "drawn again alone on this thread it is the page \
                                 there, so the threads parted them"
                            }
                            Some(_) => {
                                "drawn again alone on this thread it still is not, \
                                 so a region of this shape parts from the page"
                            }
                        };
                        panic!(
                            "{what}: the band over rows {} to {} is not the page \
                             there -- {} of its {} pixels differ, worst component \
                             {} levels, first at ({}, {}) on the page; {why}",
                            band.y,
                            band.y + band.height - 1,
                            d.pixels,
                            u64::from(band.width) * u64::from(band.height),
                            d.worst,
                            d.at.0,
                            band.y + d.at.1,
                        )
                    }
                    None => panic!(
                        "{what}: every band is its rectangle of the page, yet \
                         the stitched buffer is {} bytes against the page's {}",
                        stitched.len(),
                        full.data.len()
                    ),
                }
            }
        });
    }
}

// ---- JPEG 2000 windows (P-10.9) ---------------------------------------------

/// One codestream, read from `dir` under the crate: the JPEG 2000 decoder's
/// own fixtures, or the benchmark set's committed ones.
fn codestream_in(dir: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(dir)
            .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A page whose JPEG 2000 images each tile and the page decode in part
/// (`docs/design/jpx-speed.md`, "The visible window"), 171 by 233 points,
/// which no tile size here divides: the benchmark set's 620 by 877
/// codestream drawn at a quarter over most of the page, two levels dropped;
/// under a clip, the decoder's 32 by 24 colour fixture drawn magnified twice
/// side by side, so the two abut in one run and the clip shows a part of
/// each; and the 620 by 877 one again at an eighth under a clip that shows a
/// corner of it, three levels dropped. Every placement is at a fractional
/// offset.
fn jpx_window_page() -> Vec<u8> {
    let big = codestream_in("benches/data", "irreversible-620x877.jp2");
    let small = codestream_in("../tinker-pdf-filters/tests/jpx", "c1-3.jp2");
    let content = "q 155 0 0 219 7.3 5.1 cm /Big Do Q\n\
         q 45.1 70.3 65.1 19.8 re W n\n\
         q 48 0 0 36 31.5 60.2 cm /A Do Q q 48 0 0 36 79.5 60.2 cm /A Do Q Q\n\
         q 120.3 150.6 40.1 50.2 re W n 77 0 0 109 100.1 130.4 cm /Big Do Q\n";
    let image = |width: u32, height: u32, data: &[u8]| {
        let mut out = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /Filter /JPXDecode /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    };
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 171 233]\n   \
          /Resources << /XObject << /Big 5 0 R /A 6 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        image(620, 877, &big),
        image(32, 24, &small),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    for (index, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// **A region of a JPEG 2000 page is the crop of the whole page** (ruling
/// 5), though each tile decodes only the part of each image it reaches and
/// the page the parts the clips show: the window is widened by the sampler's
/// reach, the pyramid's pairs and the sampling policy are the whole reduced
/// image's, and a sample a window leaves out is read as its nearest held one,
/// so every pixel a tile shows reads the samples the page's reads and a run
/// covers what it would over the whole image. At 1x over lattices of 64, 37
/// and 23 pixels, none of which divides the page, and at 0.5x and 2x, where
/// the page drops other levels; and a tile's render decodes fewer
/// code-blocks than the page's.
#[test]
fn a_region_of_a_jpx_page_is_the_crop_of_the_whole_page() {
    let document = Document::open(jpx_window_page()).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions::default();
    let full = page.render(&RenderOptions {
        stats: true,
        ..options.clone()
    });
    assert!(full.warnings.is_empty(), "{:?}", full.warnings);
    assert!(ink(&full) > 25_000, "the images cover most of the page");
    for tile in [64u32, 37, 23] {
        tiles_are_the_page(&page, &options, tile, "the JPEG 2000 page at 1x");
    }
    for scale in [0.5f64, 2.0] {
        let options = RenderOptions {
            scale,
            ..RenderOptions::default()
        };
        tiles_are_the_page(
            &page,
            &options,
            41,
            &format!("the JPEG 2000 page at {scale}x"),
        );
    }

    let blocks = |stats: Option<tinker_pdf::Stats>| stats.expect("counted").jpx_code_blocks_decoded;
    let whole = blocks(full.stats);
    let corner = page.render(&RenderOptions {
        region: Some(PixelRegion::new(0, 0, 37, 37)),
        stats: true,
        ..options.clone()
    });
    assert!(
        blocks(corner.stats) < whole,
        "a corner decodes fewer code-blocks than the page"
    );
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
