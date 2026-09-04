//! Which constructs survive `tpdf probe`'s relations, and which cannot.
//!
//! The corpus's fourth axis asks a relation of two renders of one file and
//! records how many files hold it. What it cannot say is **why** a file broke
//! one, because it has no ground truth and no ablation: 181 files fail
//! `rotate`, 43 `crop` and 51 `dpi`, and until this file they were a number.
//!
//! `docs/design/image-edges.md` attributes one class — anti-aliased image edges
//! — by the method this file copies: isolate on a synthetic case, ablate the
//! one thing that is suspected, and give the numbers. What isolation buys over
//! the corpus is that a synthetic page has exactly one construct in it, so a
//! relation that breaks names the construct rather than the document.
//!
//! # What the corpus said first
//!
//! Grouping the 51 `dpi` failures by the directory they sit in:
//!
//! | Directory | Files |
//! | --- | ---: |
//! | `PDF_A-2b/6.2 Graphics/6.2.10 Transparency` | 7 |
//! | `PDF_A-4/6.2 Graphics/6.2.9 Transparency` | 7 |
//! | `PDF_A-4/6.2 Graphics/6.2.4 Colour spaces` | 5 |
//! | `PDF_A-2b/.../6.2.4.3 Uncalibrated device colour spaces` | 3 |
//! | everything else, one or two apiece | 29 |
//!
//! and four *identical* differing-pixel counts — 50 325, 52 503, 105 735 and
//! 18 183 of 250 000 — cover 25 of the 51 between them, which is one document
//! appearing in many variants rather than 25 causes. So transparency is where
//! the largest coherent `dpi` class is, and this file is what settles whether
//! that is the transparency or the documents.

use tinker_pdf::{
    Bitmap, DeviceSpace, Document, DocumentBuilder, ExtGState, FormXObject, Function, ImageData,
    PixelFormat, RenderOptions, Shading, TransparencyGroup,
};

mod render_support;
use render_support::curvy_font;

/// The relation's own budget, from `tools/tpdf/src/main.rs`.
const DPI_BUDGET: f64 = 0.02;
/// And its own per-channel tolerance.
const CHANNEL_TOLERANCE: i32 = 8;

const SIZE: f64 = 64.0;

fn channels(bitmap: &Bitmap, x: u32, y: u32) -> (i32, i32, i32) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]))
}

/// `tpdf probe`'s `dpi` relation, transcribed rather than called.
///
/// The probe is a binary and this is a test; ruling 13 forbids a test spawning
/// a program, and a relation copied from the source is a relation this file can
/// be wrong about on its own. The arithmetic is the same: render at twice the
/// scale, box-filter the four pixels down to one, and count the pixels that
/// move by more than the tolerance on any channel.
fn dpi_relation(bytes: Vec<u8>) -> (u64, u64) {
    let document = Document::open(bytes).expect("it opens");
    let page = document.page(0).expect("a page");
    let options = RenderOptions {
        scale: 1.0,
        format: PixelFormat::Rgb8,
        cancel: None,
        annotations: true,
    };
    let base = page.render(&options);
    let doubled = page.render(&RenderOptions {
        scale: 2.0,
        ..options
    });

    let across = base.width.min(doubled.width / 2);
    let down = base.height.min(doubled.height / 2);
    let mut moved = 0u64;
    for y in 0..down {
        for x in 0..across {
            let mut sum = (0i32, 0i32, 0i32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = channels(&doubled, x * 2 + dx, y * 2 + dy);
                    sum = (sum.0 + p.0, sum.1 + p.1, sum.2 + p.2);
                }
            }
            let filtered = (sum.0 / 4, sum.1 / 4, sum.2 / 4);
            let here = channels(&base, x, y);
            if (filtered.0 - here.0).abs() > CHANNEL_TOLERANCE
                || (filtered.1 - here.1).abs() > CHANNEL_TOLERANCE
                || (filtered.2 - here.2).abs() > CHANNEL_TOLERANCE
            {
                moved += 1;
            }
        }
    }
    (moved, u64::from(across) * u64::from(down))
}

fn share(moved: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        moved as f64 / total as f64
    }
}

/// Two overlapping circles' worth of curve, so the page has soft edges to
/// resample and is not a rectangle on integers.
const SHAPES: &str = "0.2 0.4 0.9 rg 8 8 m 40 8 l 24 44 l h f \
                      0.9 0.4 0.2 rg 20 20 m 56 20 l 38 52 l h f";

/// The page without the construct: the same two shapes, drawn plainly.
fn plain_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f\n").as_bytes());
        page.raw(SHAPES.as_bytes());
    });
    builder.finish()
}

/// The same two shapes inside an isolated transparency group at half alpha.
fn group_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_ext_gstate(
        b"GS0",
        &ExtGState {
            fill_alpha: Some(0.5),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, SIZE, SIZE],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Rgb,
                isolated: true,
                knockout: false,
            }),
            content: SHAPES.as_bytes(),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f\n").as_bytes());
        page.raw(b"q ");
        assert!(page.set_ext_gstate(b"GS0"));
        assert!(page.form(b"Fm0"));
        page.raw(b"Q\n");
    });
    builder.finish()
}

/// **The `dpi` relation is an edge measurement, and a transparency group is
/// not why it fails.**
///
/// The ablation is the point: the two pages draw the same two shapes over the
/// same backdrop, and differ only in whether the shapes go through an isolated
/// group at half alpha. If a group were the cause of the corpus's largest
/// coherent `dpi` class, this pair would separate — and it does not. Both stay
/// inside the relation's own budget, and the group's residue is within a
/// hair of the plain page's.
///
/// So the transparency directories are where those files *live*, not what
/// breaks them: they are one veraPDF document reissued per clause, and four
/// identical pixel counts across 25 files say so. What the relation actually
/// measures is what `image-edges.md` already named — a soft edge does not
/// survive a change of sampling grid to the byte — and a page whose edges are
/// diagonal has more of them.
#[test]
fn a_transparency_group_is_not_why_the_dpi_relation_breaks() {
    let (plain_moved, plain_total) = dpi_relation(plain_page());
    let (group_moved, group_total) = dpi_relation(group_page());
    println!(
        "  plain {plain_moved} of {plain_total} ({:.2}%), group {group_moved} of {group_total} ({:.2}%)",
        share(plain_moved, plain_total) * 100.0,
        share(group_moved, group_total) * 100.0
    );

    assert!(
        share(plain_moved, plain_total) <= DPI_BUDGET,
        "the plain page holds the relation: {plain_moved} of {plain_total}"
    );
    assert!(
        share(group_moved, group_total) <= DPI_BUDGET,
        "and so does the same page inside a transparency group: \
         {group_moved} of {group_total} -- if this ever fails, the group *is* a \
         class and this file's conclusion has to be rewritten"
    );
}

/// A backdrop and one rectangle whose every edge is an integer.
fn rectangle_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f
").as_bytes());
        page.set_fill_rgb(0.2, 0.4, 0.9);
        page.raw(b"8 8 32 32 re f
");
    });
    builder.finish()
}

/// An axial shading, which is evaluated at each pixel's own centre -- and the
/// centres of a doubled grid are not the centres of this one.
fn shading_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading(
        b"Sh0",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 0.0, SIZE, SIZE],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![0.9, 0.2, 0.1],
                c1: vec![0.1, 0.2, 0.9],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.raw(format!("q 0 0 {SIZE} {SIZE} re W n").as_bytes());
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    builder.finish()
}

/// A small image at a scale that is not a whole number of source pixels per
/// device pixel, which is the case `image-edges.md` is about.
fn image_page() -> Vec<u8> {
    let mut data = Vec::new();
    for row in 0..5u32 {
        for column in 0..5u32 {
            let on = (row + column) % 2 == 0;
            let (r, g, b) = if on { (20, 40, 160) } else { (240, 200, 60) };
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 5,
            height: 5,
            data: &data,
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f
").as_bytes());
        // 37 points over five samples: 7.4 device pixels a sample at one
        // scale and 14.8 at two, so neither grid lands on a sample boundary.
        page.image(b"Im0", 9.0, 9.0, 37.0, 37.0);
    });
    builder.finish()
}

/// Text in the face this repository writes for itself. A glyph outline is
/// flattened to a tolerance in *device* space, so the curve a doubled render
/// draws is not the curve this one draws scaled up.
fn text_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f
").as_bytes());
        page.set_fill_rgb(0.1, 0.1, 0.1);
        page.text(b"F0", 13.0, 4.0, 40.0, "Abcde");
        page.text(b"F0", 9.0, 4.0, 20.0, "fghijkl");
    });
    builder.finish()
}

/// **Nothing this file can build moves a single pixel**, which is the result.
///
/// Six constructs, each on its own page over the same backdrop, each put
/// through the relation: a rectangle on integers, diagonal path edges, the same
/// inside an isolated transparency group at half alpha, an axial shading, an
/// image at a scale that lands on no sample boundary at either resolution, and
/// text in a face whose outlines are flattened in device space. **All six move
/// zero pixels.**
///
/// That is worth more than a confirmation would have been. The plausible
/// stories about the `dpi` residue — anti-aliased edges, a transparency group
/// composited at two resolutions, a shading sampled at two sets of pixel
/// centres, an outline flattened to a device-space tolerance — are each
/// eliminated here at this page size, and the corpus's own numbers say why they
/// were never likely: the files that fail move **20 % to 42 %** of the page,
/// and no edge effect is a fifth of a page.
///
/// So the attribution is not finished, and this is where it stands: the cause
/// is something a 64-point synthetic page does not contain, and the next step
/// is to bisect a real witness rather than to build another guess. The four
/// signatures in this file's header name 25 files to start from.
///
/// The zeros are asserted rather than printed, because they are now a claim:
/// the day one of these constructs starts moving pixels under a change of
/// sampling grid, that is a rasteriser change worth knowing about, and this is
/// the only test in the tree that would say so.
#[test]
fn what_moves_under_a_change_of_sampling_grid() {
    let mut zero = Vec::new();
    let mut moves = Vec::new();
    for (name, bytes) in [
        ("a rectangle on integers", rectangle_page()),
        ("diagonal edges", plain_page()),
        ("the same, in a transparency group", group_page()),
        ("an axial shading", shading_page()),
        ("an image at a non-integer scale", image_page()),
        ("text", text_page()),
    ] {
        let (moved, total) = dpi_relation(bytes);
        println!(
            "  {name:<36} {moved:>6} of {total} ({:.2}%)",
            share(moved, total) * 100.0
        );
        if moved == 0 {
            zero.push(name);
        } else {
            moves.push((name, share(moved, total)));
        }
    }
    assert!(
        moves.is_empty(),
        "one of these constructs has started moving pixels under a change of          sampling grid, which it did not on 4 September 2026: {moves:?}"
    );
    assert_eq!(zero.len(), 6, "and all six are still measured");
}

/// **A page of diagonal edges is not where the residue is** -- kept as the
/// negative result it turned out to be.
///
/// The first draft of this file asserted the opposite, on the strength of
/// `image-edges.md` naming anti-aliased edges as a class. Measured, a diagonal
/// path edge moves **nothing**: the box-filtered doubled render agrees with the
/// direct one everywhere, within the relation's own eight-level tolerance. So
/// coverage anti-aliasing survives a change of sampling grid, and whatever the
/// 51 corpus files are failing on, it is not that.
///
/// Old doc comment, for the record, which is the class
/// `image-edges.md` named for images arriving for glyphs and paths too.
///
/// Not an assertion that the relation fails — it holds on this page — but a
/// measurement of where its residue comes from: the pixels that move are the
/// edges, and a page with more edge per unit area moves more of them. The
/// count is printed rather than pinned, because pinning it would make an
/// ordinary rasteriser change look like a regression in a document about why
/// files fail.
#[test]
fn the_dpi_residue_is_edges_and_not_area() {
    let (edges_moved, edges_total) = dpi_relation(plain_page());

    // The same page with one axis-aligned rectangle instead of two triangles:
    // the same ink, almost none of the edge.
    let mut builder = DocumentBuilder::new();
    builder.add_page(SIZE, SIZE, |page| {
        page.set_fill_rgb(0.95, 0.95, 0.6);
        page.raw(format!("0 0 {SIZE} {SIZE} re f\n").as_bytes());
        page.set_fill_rgb(0.2, 0.4, 0.9);
        page.raw(b"8 8 32 32 re f\n");
    });
    let (flat_moved, flat_total) = dpi_relation(builder.finish());

    println!(
        "  diagonal edges {edges_moved} of {edges_total}, axis-aligned {flat_moved} of {flat_total}"
    );
    assert_eq!(
        flat_moved, 0,
        "a rectangle on integers resamples exactly, at any scale"
    );
    assert_eq!(
        edges_moved, 0,
        "and neither does a diagonal one -- which is the negative result this          test exists to keep: {edges_moved} moved"
    );
}
