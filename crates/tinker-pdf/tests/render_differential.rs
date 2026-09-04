//! Two documents that must draw the same picture (ruling 13, roadmap step 6).
//!
//! Milestone 5 of [`docs/design/render-verification.md`]. The analytic tier
//! evaluates a formula, which reaches the operators ISO 32000 publishes as an
//! expression and no further: nothing in the standard says what a *tiling
//! pattern* should look like, because a tiling pattern looks like whatever its
//! cell draws. What the standard does say is that the cell is drawn at each
//! lattice point — and that is a **relation between two documents**, which
//! needs no ground truth at all.
//!
//! So each fixture below is a pair. One half reaches the picture through the
//! construct; the other reaches it through the operators the construct is
//! defined as. The two share the parser, the writer and the rasteriser, and
//! differ only in the plumbing the construct owns — which is exactly the code
//! a pair can adjudicate and an expression cannot.
//!
//! # Byte-equal, and what that costs the fixtures
//!
//! Every pair asserts the **whole bitmap**, because "these two are the same
//! document" is the entire claim and a sampled pixel is satisfied by two
//! documents that agree in one place. The price is that the geometry has to be
//! chosen so the two halves are *entitled* to be byte-equal:
//!
//! - **The tiling pair runs at scale 1, on an identity `/Matrix`, with integer
//!   `/BBox` and integer steps.** The renderer rasterises a cell once and blits
//!   the copies at rounded device offsets, which is what makes a thousand-cell
//!   page affordable; at a fractional offset the blit and a direct paint are
//!   entitled to differ by the rounding, and a pair that failed there would be
//!   reporting the optimisation rather than a defect.
//! - **The form pair uses no `/Group`.** A form with one composites into a
//!   buffer of its own with the alphas reset, so the inlined operators are a
//!   different rendering by construction rather than a different spelling.
//! - **Every edge in every fixture lands on an integer**, so no pair rests on
//!   two anti-aliased edges agreeing to the byte.
//!
//! Each of those is a constraint on the *fixture*, not a tolerance on the
//! comparison: there is no budget here and there is not meant to be one.
//!
//! # The injections that were counted
//!
//! Over the whole workspace, 4 413 tests, `--no-fail-fast` before `-p`. The
//! second column is how many of the first are the four pairs above.
//!
//! | Injected | Caught by the workspace | Of which here |
//! | --- | ---: | ---: |
//! | a tiling lattice stepped by its `/BBox` rather than by `/XStep` | 11 | 1 |
//! | a shading pattern sampled through the CTM rather than its own `/Matrix` | 3 | 1 |
//! | a form's `/BBox` not clipping | 3 | 1 |
//! | a Type 3 `/FontMatrix` applied after the text matrix rather than inside it | 4 | 1 |
//! | a tile blitted at `floor` rather than `round` | **1** | **0** |
//!
//! The last row is the honest one. That defect is invisible here **by
//! construction**: every lattice offset in the tiling fixture is an integer,
//! because that is the condition under which a blit and a direct paint are
//! entitled to be byte-equal at all. Only the determinism fingerprint sees it,
//! and it sees it the way a fingerprint sees everything — a pixel moved, with
//! no opinion about whether it should have. A pair that could catch it would be
//! a pair with a budget, and a budget is what this file exists not to have.

use tinker_pdf::{
    DeviceSpace, DocumentBuilder, FormXObject, Function, Shading, ShadingPattern, TilingPattern,
    TilingType,
};

mod render_support;
use render_support::{ink, render, same_picture, SIZE};

/// The least ink a pair may draw before it stops being evidence.
///
/// A pair of blank pages agrees perfectly. `determinism.rs` learned this the
/// expensive way — a fixture whose font was missing hashed a blank page and
/// passed on every target for months — so each half is held to a floor here
/// for the same reason, and the floor is checked on **both** halves rather
/// than on the one that is easier to reach.
#[track_caller]
fn both_drew(built: &tinker_pdf::Bitmap, written: &tinker_pdf::Bitmap, least: usize, what: &str) {
    for (which, bitmap) in [("the construct", built), ("the operators", written)] {
        let drawn = ink(bitmap);
        assert!(
            drawn >= least,
            "{what}: {which} painted {drawn} pixels, fewer than the {least} \
             it is supposed to -- two pages that agree because neither drew \
             anything agree about nothing"
        );
    }
}

// ---- tiling patterns --------------------------------------------------------

/// **A tiling pattern is its cell at every lattice point** (8.7.3.1).
///
/// The cell is four points and the step is **five**, and that gap is the whole
/// design of the fixture. 8.7.3.1 makes `/XStep` and `/YStep` the lattice's
/// spacing and the `/BBox` the cell's own size, and the two are independent —
/// so a build that derived the spacing from the box draws a picture that is
/// still a tiling, still plausible, and one point out per column. A fixture
/// whose step equalled its box could not tell the two apart, and the first
/// draft of this one did not.
///
/// A cell that leaves most of itself white matters for the same reason: the
/// page is part painted and part not, so a build that filled the path with the
/// cell's colour instead of tiling it is rejected here too.
#[test]
fn a_tiling_pattern_draws_what_its_cells_unrolled_draw() {
    const CELL: &str = "0 0 0.8 rg 0 0 2 2 re f 0.8 0 0 rg 2 2 2 2 re f";
    const CELL_SIZE: f64 = 4.0;
    const STEP: f64 = 5.0;

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, CELL_SIZE, CELL_SIZE],
            x_step: STEP,
            y_step: STEP,
            matrix: None,
            tiling_type: TilingType::NoDistortion,
            content: CELL.as_bytes(),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(format!("0 0 {SIZE} {SIZE} re f").as_bytes());
    });
    let built = render(builder.finish());

    // The lattice runs while a cell can still meet the page: at a step of five
    // that is columns 0 to 3, whose cells cover 0-4, 5-9, 10-14 and 15-19, the
    // last of them mostly off the page and clipped by it on both halves.
    let mut unrolled = String::new();
    let steps = (SIZE / STEP).ceil() as u32;
    for row in 0..steps {
        for column in 0..steps {
            let (x, y) = (f64::from(column) * STEP, f64::from(row) * STEP);
            unrolled.push_str(&format!("q 1 0 0 1 {x} {y} cm {CELL} Q\n"));
        }
    }
    let mut by_hand = DocumentBuilder::new();
    by_hand.add_page(SIZE, SIZE, |page| page.raw(unrolled.as_bytes()));
    let written = render(by_hand.finish());

    both_drew(&built, &written, 42, "a tiling pattern");
    same_picture(built, written, "a tiling pattern against its unrolled cells");
}

// ---- form XObjects ----------------------------------------------------------

/// **A form XObject is its operators under its `/Matrix`, clipped to its
/// `/BBox`** (8.10.1).
///
/// Both halves of that sentence are load-bearing and the fixture makes each
/// visible: the `/Matrix` is a translation of three points, so a build that
/// ignored it draws in the wrong place; and the content deliberately paints
/// past the `/BBox` on every side, so a build that did not clip draws too much.
#[test]
fn a_form_xobject_draws_what_its_operators_inlined_draw() {
    // The rectangle runs from 1 to 13 and the box is 2 to 10, so the clip has
    // something to remove on all four sides.
    const INSIDE: &str = "0 0 0.7 rg 1 1 12 12 re f";
    const BBOX: [f64; 4] = [2.0, 2.0, 10.0, 10.0];
    const MATRIX: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 3.0, 3.0];

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: BBOX,
            matrix: Some(MATRIX),
            group: None,
            content: INSIDE.as_bytes(),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        assert!(page.form(b"Fm0"));
    });
    let built = render(builder.finish());

    // 8.10.1: the matrix maps form space into the space in force, and the box
    // is a clip *in form space*, so it is stated after the `cm` and before the
    // content.
    let inlined = format!(
        "q {} {} {} {} {} {} cm {} {} {} {} re W n {INSIDE} Q",
        MATRIX[0],
        MATRIX[1],
        MATRIX[2],
        MATRIX[3],
        MATRIX[4],
        MATRIX[5],
        BBOX[0],
        BBOX[1],
        BBOX[2] - BBOX[0],
        BBOX[3] - BBOX[1],
    );
    let mut by_hand = DocumentBuilder::new();
    by_hand.add_page(SIZE, SIZE, |page| page.raw(inlined.as_bytes()));
    let written = render(by_hand.finish());

    both_drew(&built, &written, 32, "a form XObject");
    same_picture(built, written, "a form XObject against its inlined operators");
}

// ---- shading patterns -------------------------------------------------------

/// The axis both halves share, diagonal so that a build projecting onto the
/// wrong axis has somewhere to be wrong.
const AXIS: [f64; 4] = [0.0, 0.0, SIZE, SIZE];

/// The pattern's own `/Matrix`, and the reason the pair has teeth.
///
/// 8.7.3.1 maps pattern space through **this** into the page's default space,
/// ignoring whatever transform is in force when the pattern is set. `sh` maps
/// through the transform in force instead. At the identity the two rules are
/// the same rule and a pair cannot part them — which is what the first draft of
/// this fixture did, and the injection matrix said so: the defect was caught by
/// two assertions elsewhere and by nothing here.
const PATTERN_MATRIX: [f64; 6] = [1.0, 0.0, 0.0, 1.0, -4.0, 2.0];

fn axial() -> Shading {
    Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: AXIS,
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.9, 0.1, 0.1],
            c1: vec![0.1, 0.1, 0.9],
            n: 1.0,
        },
        extend: (true, true),
    }
}

/// **A shading pattern fill is `sh` inside the same region** (8.7.4.5.5).
///
/// This is the pair worth the most in the file, because the two halves are two
/// independent copies of one loop in the renderer: `sh` maps the shading
/// through the CTM in force, and a pattern maps it through its own `/Matrix`
/// into the page's default space (8.7.3.1). At the identity they are the same
/// map and must produce the same pixels; the day one loop learns something the
/// other does not, this fails.
///
/// The region is a rectangle rather than the whole page so that the pattern's
/// half is a *fill* and not a flood: `sh` paints the clip and a pattern paints
/// a path, and a pair over the whole page could not tell a build that ignored
/// the path from one that honoured it.
#[test]
fn a_shading_pattern_fill_draws_what_sh_draws_in_the_same_region() {
    const REGION: &str = "2 3 12 10 re";

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_shading_pattern(
        b"P0",
        &ShadingPattern {
            shading: axial(),
            matrix: Some(PATTERN_MATRIX),
        }
    ));
    builder.add_page(SIZE, SIZE, |page| {
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(format!("{REGION} f").as_bytes());
    });
    let built = render(builder.finish());

    let mut by_hand = DocumentBuilder::new();
    assert!(by_hand.add_shading(b"Sh0", &axial()));
    by_hand.add_page(SIZE, SIZE, |page| {
        // The clip is the pattern's region, stated in the space the fill was
        // stated in; the `cm` after it is the pattern's own `/Matrix`, which is
        // how `sh` is made to sample where the pattern samples.
        page.raw(format!("q {REGION} W n").as_bytes());
        page.raw(
            format!(
                "{} {} {} {} {} {} cm",
                PATTERN_MATRIX[0],
                PATTERN_MATRIX[1],
                PATTERN_MATRIX[2],
                PATTERN_MATRIX[3],
                PATTERN_MATRIX[4],
                PATTERN_MATRIX[5]
            )
            .as_bytes(),
        );
        assert!(page.shading(b"Sh0"));
        page.raw(b"Q");
    });
    let written = render(by_hand.finish());

    both_drew(&built, &written, 100, "a shading pattern");
    same_picture(built, written, "a shading pattern against sh under a clip");
}

// ---- Type 3 glyphs ----------------------------------------------------------

/// A one-page document, written by hand, with an optional Type 3 font.
///
/// Both halves of the Type 3 pair are hand-written because there is no builder
/// API for a Type 3 font — `DocumentBuilder` embeds font *programs* and this
/// font has none, its glyphs being content streams. Writing the other half by
/// hand too keeps the writer out of the comparison entirely.
fn page(font: Option<(&str, &str)>, content: &str) -> Vec<u8> {
    let (resources, extra) = match font {
        Some((matrix, procedure)) => (
            "/Font << /F0 5 0 R >>".to_string(),
            format!(
                "5 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 16 16]\n\
                   /FontMatrix {matrix}\n\
                   /CharProcs << /a 6 0 R >>\n\
                   /Encoding << /Type /Encoding /Differences [97 /a] >>\n\
                   /FirstChar 97 /LastChar 97 /Widths [16] /Resources << >> >>\nendobj\n\
                 6 0 obj\n<< /Length {} >>\nstream\n{procedure}\nendstream\nendobj\n",
                procedure.len() + 1
            ),
        ),
        None => (String::new(), String::new()),
    };
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {SIZE} {SIZE}]\n\
   /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
{extra}\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1
    )
    .into_bytes()
}

/// **A Type 3 glyph is its procedure under the font matrix and the text
/// matrix** (9.6.5).
///
/// The `/FontMatrix` is a sixteenth and the size is sixteen, so one glyph-space
/// unit is one user-space unit and the arithmetic that places the glyph is
/// visible in the fixture rather than buried in a thousandth. `d0` takes the
/// colour from the graphics state, so both halves state the same `rg` and the
/// glyph procedure carries only geometry.
#[test]
fn a_type3_glyph_draws_what_the_same_path_draws() {
    let glyph = render(page(
        Some(("[0.0625 0 0 0.0625 0 0]", "16 0 d0 2 2 12 9 re f")),
        "0.1 0.5 0.2 rg BT /F0 16 Tf 1 0 0 1 2 3 Tm (a) Tj ET",
    ));

    // The procedure's box is 2..14 by 2..11 in glyph space; the font matrix is
    // the identity at this size, and the text matrix translates by (2, 3).
    let path = render(page(None, "0.1 0.5 0.2 rg 4 5 12 9 re f"));

    both_drew(&glyph, &path, 64, "a Type 3 glyph");
    same_picture(glyph, path, "a Type 3 glyph against the same path");
}
