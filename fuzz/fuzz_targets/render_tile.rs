//! Tile against page: a generated page drawn whole and in generated regions
//! at the same scale, every region the whole render's rectangle byte for
//! byte (ruling 5).
//!
//! This target reads its input as choices (`tinker_pdf_fuzz::Choices`) and
//! writes `content_stream`'s page with `tinker_pdf_fuzz::content::tiled`:
//! every operator of Annex A over fonts, images, forms, patterns, shadings,
//! colour spaces and annotations, with the render's scale, pixel format,
//! anti-aliasing, annotations, transparency and alpha convention, and one to
//! four regions at that scale, from inside the page to past its far edges.
//!
//! One shape is not compared: ruling 5's named exception, RC-21's image run,
//! which H-15's default keeps — two images that overlap outside a tile and
//! abut inside it are one run in the tile and two on the page. A page that
//! may draw two images is left out, decided from the generator's count of
//! the operators that may draw one (`content::Drawn`) before anything is
//! drawn: `BI`, or `Do` naming anything but a form, two of them in the page's
//! own content, or one in a form, a pattern, a glyph procedure or an
//! appearance, which may run more than once. It is a superset of the class,
//! and errs toward comparing less, never toward passing a mismatch outside
//! it. RC-21's fix deletes it.
//!
//! The caps are `tinker_pdf_fuzz::content`'s — 192 operators in all, images of
//! eight samples a side, a page of 72 points a side.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The whole render is the page's pixel size.**
//! - **Every region is the page under it.** Its bitmap is the region clamped
//!   to the page (`PixelRegion::clamped_to`), in the whole render's format and
//!   alpha convention, and every row of it is the whole render's bytes there:
//!   one frame for the page whatever part of it is asked for, a canvas that
//!   carries its origin, and the decisions a rectangle makes taken on the
//!   page's rectangle (ruling 5).
//!
//! # What this target cannot find, and what covers it instead
//!
//! Both renders are this engine's, so a page drawn wrongly the same way whole
//! and in a tile passes. `crates/tinker-pdf/tests/render_regions.rs` holds
//! the same relation over fixtures chosen for the rasterizer's paths, and the
//! analytic tier and the goldens what a page draws.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_fuzz::{content, Choices};

fuzz_target!(|data: &[u8]| {
    let tiled = content::tiled(&mut Choices::new(data));
    // RC-21's image run, the one shape ruling 5 names as an exception.
    if tiled.drawn.page > 1 || tiled.drawn.nested > 0 {
        return;
    }
    let doc = Document::open(tiled.bytes)
        .unwrap_or_else(|error| panic!("a generated page did not open: {error}"));
    let Some(page) = doc.page(0) else {
        panic!("a generated document has no page");
    };
    let whole = page.render(&tiled.options);
    let (width, height) = page.pixel_size(&tiled.options);
    assert_eq!(
        (whole.width, whole.height),
        (width, height),
        "the whole render is not the page's pixel size"
    );
    let components = whole.components();
    for region in &tiled.regions {
        let tile = page.render(&RenderOptions {
            region: Some(*region),
            ..tiled.options.clone()
        });
        let at = region.clamped_to(width, height);
        assert_eq!(
            (tile.width, tile.height),
            (at.width, at.height),
            "the region {region:?} came back another size than {at:?}"
        );
        assert_eq!(tile.format, whole.format, "a region changed the format");
        assert_eq!(
            tile.premultiplied, whole.premultiplied,
            "a region changed the alpha convention"
        );
        let span = tile.width as usize * components;
        for y in 0..tile.height as usize {
            let mine = y * tile.stride;
            let theirs = (at.y as usize + y) * whole.stride + at.x as usize * components;
            let (Some(mine), Some(theirs)) = (
                tile.data.get(mine..mine + span),
                whole.data.get(theirs..theirs + span),
            ) else {
                panic!("row {y} of the region {region:?} is past a bitmap's data");
            };
            if mine != theirs {
                let x = mine
                    .iter()
                    .zip(theirs)
                    .position(|(a, b)| a != b)
                    .unwrap_or(0)
                    / components.max(1);
                panic!(
                    "the region {region:?} is not the page under it: pixel ({x}, {y}) of the \
                     region, ({}, {}) on the page",
                    at.x as usize + x,
                    at.y as usize + y
                );
            }
        }
    }
});
