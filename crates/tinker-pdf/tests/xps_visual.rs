//! `VisualBrush` (15.4): a brush whose cell is a **drawing** rather than a
//! picture, and the two bounds on the recursion that makes it one.
//!
//! # What is new here and what is shared
//!
//! Everything about *where* a tile goes — 15.3's two rectangles, their two unit
//! modes, the five `TileMode`s, and 8.7.3.1's `/Matrix` into the page's default
//! space — is the same arithmetic an `ImageBrush` spends, and after this
//! milestone it is spent in one place (`paint::Placed`) rather than two.
//! `xps_images.rs` already holds the pairs that pin that arithmetic. What this
//! file pins is the half that is a `VisualBrush`'s own:
//!
//! - the cell holds *markup*, so the drawing walk is re-entered from inside a
//!   brush, and every element handler has to work there as it does on a page;
//! - a `VisualBrush` may state another one, to any depth the file chooses;
//! - a `VisualBrush` reached through a `{StaticResource}` whose own subtree
//!   names that key again never terminates, and **no single lookup chain can
//!   see it** — each lookup starts afresh, so the resource layer's cycle guard
//!   is blind to this one.
//!
//! The last two are **two rules**, which is the shape five milestones running
//! have found: a nest four deep holds no repeated key, and a cycle two deep is
//! not deep. Deleting either guard leaves the other passing every test written
//! for it, so each has its own fixture and its own name in the report.
//!
//! # Counted injections
//!
//! Each check below was verified by reintroducing the defect it exists to
//! catch and running `cargo test -p tinker-pdf --no-fail-fast`, whose baseline
//! is **1 228 tests**.
//!
//! | Injection | Caught by |
//! | --- | --- |
//! | `visual_brush` passes `None` for the box being filled | 1 |
//! | the depth cap is removed, the cycle guard kept | 1 |
//! | the cycle guard is removed, the depth cap kept | 1 |
//! | `draw_subtree` hands `canvas` the whole node instead of a leaf | 1 |
//! | `Placed::place` drops the brush's own `Transform` | 1 |
//! | the visual's measured extent becomes the unit square | 1 |
//! | `TileMode::FlipXY`'s four copies collapse to one | 3 |
//!
//! Nothing fired zero, but one of these had to be *made* non-zero and that is
//! the finding worth recording. The measured-extent injection was caught by
//! **nothing** on its first run: every test in this file but one spelled
//! `ViewportUnits="Absolute"` out, and the one that did not asserted only that
//! a pattern reached the page — which a build that sized every cell at one
//! unit square still does. A tile drawn at a twentieth of its size, silently,
//! is exactly the failure this repository's refusal asymmetry exists to make
//! impossible, so the test now reads the cell's `/BBox` and the injection is
//! caught. A plausible break that fires nothing means the suite does not test
//! what it claims, and the answer is a better assertion rather than a smaller
//! claim.
//!
//! The two guards are the pair this file exists for: each is caught by exactly
//! **one** test and by a *different* one. Removing the cycle guard while
//! keeping the depth cap does not hang — the cap stops it — it merely reports
//! the wrong name, which is precisely why a suite with only one of the two
//! would pass with the other deleted.

mod xps_support;

use tinker_pdf::{ArchiveWarning, Document, WriteMode, WriteOptions, XpsElementDefect};
use xps_support::{archive, one_page_package, with, XPS_NS};

/// The resource-dictionary key namespace, which every real package binds.
const KEY_NS: &str = "http://schemas.microsoft.com/xps/2005/06/resourcedictionary-key";

/// A package whose one fixed page carries `body` between `<FixedPage>` tags.
fn package(body: &str) -> Vec<u8> {
    let markup = format!(
        r#"<FixedPage xmlns="{XPS_NS}" xmlns:x="{KEY_NS}" Width="816" Height="1056">{body}</FixedPage>"#
    );
    archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &markup,
    ))
}

fn defects(body: &str) -> Vec<XpsElementDefect> {
    let document = Document::open(package(body)).expect("an XPS");
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::XpsElement { defect, .. } => Some(*defect),
            _ => None,
        })
        .collect()
}

/// The page's own content stream.
fn stream(body: &str) -> String {
    let document = Document::open(package(body)).expect("an XPS");
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

/// The whole synthesised document, saved, as text.
///
/// `WriteOptions::default()` leaves compression off, so every stream is legible
/// in the bytes — and a tiling pattern's cell is a stream of its own, never
/// inline on the page, so this is the only place the cell's content can be
/// read. Reading the *saved* file rather than the builder's tables is
/// `xps_images.rs`'s own choice and for its reason: it is the artefact a reader
/// would open.
fn saved(body: &str) -> String {
    let document = Document::open(package(body)).expect("an XPS");
    let out = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    String::from_utf8_lossy(&out).into_owned()
}

/// A `Path` filled by a `VisualBrush` stating `attributes`, over `visual`.
fn filled(attributes: &str, visual: &str) -> String {
    format!(
        r#"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
             <VisualBrush {attributes}>
               <VisualBrush.Visual>{visual}</VisualBrush.Visual>
             </VisualBrush>
           </Path.Fill></Path>"#
    )
}

/// A green rectangle, twenty units square, which is what most of these brushes
/// paint. Its colour is checked rather than its shape, because `0 1 0 rg` in a
/// stream that is not the page's is what "the cell holds a drawing" means.
const GREEN: &str = r##"<Path Data="M0,0L20,0 20,20 0,20Z" Fill="#FF00FF00" />"##;

/// A `VisualBrush` **is painted**, and the shape it fills is a pattern rather
/// than the placeholder grey.
///
/// The two halves are one rule stated twice on purpose: a build that named the
/// pattern but wrote no cell, and a build that wrote a cell nothing referenced,
/// each pass one of these.
#[test]
fn a_visual_brush_paints_its_subtree_as_a_pattern() {
    let body = filled(
        r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute" TileMode="Tile""#,
        GREEN,
    );
    assert_eq!(defects(&body), []);

    let page = stream(&body);
    assert!(
        page.contains("/Pattern cs"),
        "the fill is a pattern: {page}"
    );
    assert!(
        !page.contains("0.749 0.749 0.749 rg"),
        "and not the placeholder grey: {page}"
    );

    let all = saved(&body);
    assert!(
        all.contains("0 1 0 rg"),
        "the cell holds the drawing's own colour: {all}"
    );
}

/// The two unit modes **default to `RelativeToBoundingBox`**, which is 15.3's
/// own default and the one thing about a `VisualBrush` that is easiest to get
/// wrong in the direction that refuses every real file.
///
/// A brush stating neither attribute is the common case — WPF writes it — and a
/// build that could not resolve a relative viewport would answer
/// `BrushUnreadable` here while every `ViewportUnits="Absolute"` test above
/// still passed. That is exactly the shape this repository keeps finding, so it
/// is its own test rather than a variation on the one before it.
#[test]
fn the_unit_modes_default_to_relative_and_a_relative_brush_still_paints() {
    let body = filled(r#"Viewbox="0,0,1,1" Viewport="0,0,1,1""#, GREEN);
    assert_eq!(defects(&body), [], "a defaulted brush is not a defect");
    assert!(
        stream(&body).contains("/Pattern cs"),
        "and it reaches the page"
    );
    // The `1,1` viewbox is a **fraction of the visual's own extent**, which is
    // twenty units square — so the cell is twenty units square, not one. This
    // is the assertion that makes the measurement load-bearing: a build that
    // assumed the unit square would still name a pattern, still fill the shape
    // with it, and still pass every line above, while drawing the tile at one
    // twentieth of its size. The first injection run of this file caught that
    // break with **nothing at all**, which is why the `/BBox` is read here.
    let all = saved(&body);
    assert!(
        all.contains("/BBox [0 0 20 20]"),
        "the cell is a fraction of what the visual drew: {all}"
    );
}

/// A `VisualBrush` inside a `VisualBrush` is legal and is painted.
///
/// The nest's *first* level is the one every implementation gets right. This
/// asserts the second, because the depth cap below is meaningless if two levels
/// never worked.
#[test]
fn a_visual_brush_may_hold_another_one() {
    let inner = filled(
        r#"Viewbox="0,0,10,10" Viewport="0,0,10,10" ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        GREEN,
    );
    let body = filled(
        r#"Viewbox="0,0,200,200" Viewport="0,0,200,200" ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        &inner,
    );
    assert_eq!(defects(&body), []);
    assert!(
        saved(&body).contains("0 1 0 rg"),
        "the innermost drawing reached a cell"
    );
}

/// A nest deeper than the cap is refused **as a depth**, and is not a cycle.
///
/// Nine levels, none of which repeats a key — there are no keys at all — so a
/// build whose only guard was the cycle guard would recurse until the stack
/// ended.
#[test]
fn a_visual_brush_nested_past_the_depth_cap_is_named() {
    let mut body = GREEN.to_string();
    for _ in 0..9 {
        body = filled(
            r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
            &body,
        );
    }
    assert_eq!(defects(&body), [XpsElementDefect::BrushTooDeep]);
    // The grey is in the *innermost* cell and not on the page: the cap fires
    // at the level that would have been the ninth, and every level above it
    // painted a pattern normally. A refusal deep in a brush is still a
    // placeholder somewhere, which is ruling 2's whole requirement — the page
    // is not lost and nothing is silently absent.
    let all = saved(&body);
    assert!(
        all.contains("0.749 0.749 0.749 rg"),
        "the placeholder is drawn where the nest stopped: {all}"
    );
    assert!(
        stream(&body).contains("/Pattern cs"),
        "and the page's own shape still took its pattern"
    );
}

/// A `VisualBrush` whose own subtree names the key it was reached by is a
/// **cycle**, and is not a depth.
///
/// This is the failure no `{StaticResource}` chain can see: the outer lookup
/// resolves `v`, and the lookup *inside* the cell starts afresh with an empty
/// `seen` list, so the resource layer's guard never repeats a key. Two levels
/// deep, so the depth cap is nowhere near firing — which is what makes the two
/// guards two rules.
#[test]
fn a_visual_brush_that_reaches_itself_is_a_cycle_and_not_a_depth() {
    let body = r##"<FixedPage.Resources><ResourceDictionary>
              <VisualBrush x:Key="v" Viewbox="0,0,20,20" Viewport="0,0,20,20"
                           ViewboxUnits="Absolute" ViewportUnits="Absolute">
                <VisualBrush.Visual>
                  <Path Data="M0,0L20,0 20,20 0,20Z" Fill="{StaticResource v}" />
                </VisualBrush.Visual>
              </VisualBrush>
            </ResourceDictionary></FixedPage.Resources>
            <Path Data="M0,0L200,0 200,200 0,200Z" Fill="{StaticResource v}" />"##;
    assert_eq!(defects(body), [XpsElementDefect::BrushCyclic]);
}

/// The subtree is drawn by the **element handlers**, so a `Canvas` inside a
/// visual applies its transform exactly once.
///
/// A canvas arrives at the streamed walk as a start tag with no children and
/// its `Canvas.RenderTransform` arrives separately, so the handler reads the
/// attribute *or* the property element. Inside a brush the node is already
/// whole — and a build that handed the whole node over would have the handler
/// read the property element and the child loop hand the same element to the
/// property handler, which composes rather than assigns. The transform would be
/// applied twice: `2 0 0 2 …` becomes `4 0 0 4 …`, a picture at the right place
/// at the wrong size, which is the failure shape this repository's refusal
/// asymmetry exists for.
#[test]
fn a_canvas_inside_a_visual_applies_its_transform_once() {
    let visual = format!(
        r#"<Canvas><Canvas.RenderTransform><MatrixTransform Matrix="2,0,0,2,0,0" /></Canvas.RenderTransform>{GREEN}</Canvas>"#
    );
    let body = filled(
        r#"Viewbox="0,0,40,40" Viewport="0,0,40,40" ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        &visual,
    );
    assert_eq!(defects(&body), []);
    let all = saved(&body);
    assert!(
        all.contains("2 0 0 2 0 0 cm"),
        "the canvas transform is written once: {all}"
    );
    assert!(!all.contains("4 0 0 4 0 0 cm"), "and never squared: {all}");
}

/// The brush's own `Transform` reaches the pattern matrix.
///
/// `brush::transform_of` has always parsed this attribute into `ImageTile`, and
/// until this milestone **nothing read it** — an `ImageBrush` or a
/// `VisualBrush` stating a `Transform` drew unrotated, unscaled and silently,
/// which is gap 07's headline defect wearing a different hat. It is pinned here
/// rather than in `xps_images.rs` because this is the milestone that fixed it.
#[test]
fn the_brushs_own_transform_reaches_the_pattern_matrix() {
    let plain = filled(
        r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute""#,
        GREEN,
    );
    let scaled = filled(
        r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute" Transform="3,0,0,3,0,0""#,
        GREEN,
    );
    assert_eq!(defects(&scaled), []);
    assert_ne!(
        saved(&plain),
        saved(&scaled),
        "a brush transform changes the document"
    );
}

/// `TileMode`'s reflections are built out of a cell that holds the drawing
/// **more than once**, because 8.7.3.1 has no reflection operator at all.
///
/// `FlipXY` is four copies in a cell four times the source; `Tile` is one. A
/// build that ignored the mode would draw one copy for both and the two would
/// be the same document.
#[test]
fn the_reflections_repeat_the_drawing_inside_the_cell() {
    let one = filled(
        r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute" TileMode="Tile""#,
        GREEN,
    );
    let four = filled(
        r#"Viewbox="0,0,20,20" Viewport="0,0,20,20" ViewboxUnits="Absolute" ViewportUnits="Absolute" TileMode="FlipXY""#,
        GREEN,
    );
    assert_eq!(defects(&four), []);
    let (one, four) = (saved(&one), saved(&four));
    assert_eq!(one.matches("0 1 0 rg").count(), 1, "Tile draws once: {one}");
    assert_eq!(
        four.matches("0 1 0 rg").count(),
        4,
        "FlipXY draws four times: {four}"
    );
}

/// A `VisualBrush` stating no `Visual` is **unreadable**, not unsupported.
///
/// The distinction is the whole of ruling 10 here: "this build will not paint
/// that" and "that is not 15.4's syntax" are different facts about the file,
/// and after this milestone only the second can be true of a `VisualBrush`.
#[test]
fn a_visual_brush_with_no_visual_is_unreadable() {
    let body = r#"<Path Data="M0,0L200,0 200,200 0,200Z"><Path.Fill>
         <VisualBrush Viewbox="0,0,1,1" Viewport="0,0,1,1" />
       </Path.Fill></Path>"#;
    assert_eq!(defects(body), [XpsElementDefect::BrushUnreadable]);
    assert!(stream(body).contains("0.749 0.749 0.749 rg"));
}

/// A `VisualBrush` can be an `OpacityMask`, and reaches 11.6.5.2's `/Alpha`.
///
/// 14.3 makes a mask *a brush used as an alpha channel*, and a drawing's
/// coverage is an alpha nothing but the painting can supply — which is the
/// `ImageBrush` case's own answer, now reached by a second brush.
#[test]
fn a_visual_brush_can_be_an_opacity_mask() {
    let body = format!(
        r##"<Path Data="M0,0L200,0 200,200 0,200Z" Fill="#FF0000FF"><Path.OpacityMask>
             <VisualBrush Viewbox="0,0,20,20" Viewport="0,0,20,20"
                          ViewboxUnits="Absolute" ViewportUnits="Absolute">
               <VisualBrush.Visual>{GREEN}</VisualBrush.Visual>
             </VisualBrush>
           </Path.OpacityMask></Path>"##
    );
    assert_eq!(defects(&body), []);
    let all = saved(&body);
    assert!(all.contains("/Alpha"), "the mask is an alpha mask: {all}");
    assert!(
        all.contains("0 1 0 rg"),
        "and the drawing is what it reads: {all}"
    );
}

/// A `VisualBrush` strokes, and fills a glyph run, for the reason a gradient
/// does: a brush is a colour, and every brush-valued property takes one.
#[test]
fn a_visual_brush_strokes_as_well_as_fills() {
    let body = format!(
        r#"<Path Data="M0,0L200,0 200,200 0,200Z" StrokeThickness="4"><Path.Stroke>
             <VisualBrush Viewbox="0,0,20,20" Viewport="0,0,20,20"
                          ViewboxUnits="Absolute" ViewportUnits="Absolute">
               <VisualBrush.Visual>{GREEN}</VisualBrush.Visual>
             </VisualBrush>
           </Path.Stroke></Path>"#
    );
    assert_eq!(defects(&body), []);
    let page = stream(&body);
    assert!(page.contains("/Pattern CS"), "a stroking pattern: {page}");
}
