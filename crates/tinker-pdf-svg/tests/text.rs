//! Milestone 6: §10's text, carried unshaped.
//!
//! # What this file can and cannot assert
//!
//! Everything about the *document*: the characters, which run opens a chunk,
//! where a chunk's anchor is, the composed matrix, and the resolved font
//! properties. Nothing about a *font* — because `tinker-pdf-svg` has no edge to
//! one and ruling 8 says it must not. Where a continuing run begins, and where
//! `text-anchor` moves a chunk to, are asserted in the facade's own suite,
//! against the same `choose` and the same shaper that set the rest of the book.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | a `<tspan>` with no position opens a chunk anyway | 3 |
//! | a missing axis defaults to zero instead of the pen | 1 |
//! | the pen is not remembered across chunks | 1 |
//! | a `font-family` list is split on spaces as well as commas | 1 |
//! | an `em` `font-size` resolves against the initial size | 1 |
//! | `font-weight: bold` is not 700 | 1 |
//! | `font-style: italic` is not read | 1 |
//! | `text-anchor: middle` is not read | 1 |
//! | a position list is taken silently | 1 |
//! | `<textPath>` is drawn as ordinary text | 1 |
//! | a newline becomes a space rather than being removed | 1 |
//! | leading white space is kept | 1 |
//! | a hidden run is still drawn | 1 |
//! | a continuing run's `dx`/`dy` is dropped | 1 |
//! | a `<tspan>` does not compose its own `transform` | 1 |
//!
//! Fifteen injections, no zeros — after two were found. The fixture's only
//! multi-word family was a **quoted** one, which is a single token, so the
//! injection that splits an unquoted family on spaces produced the same list;
//! and no `<tspan>` carried a `transform` of its own, so composing it was
//! proved by the `<text>` above it.

use tinker_pdf_svg::{Colour, Limits, Node, Paint, Scene, TextAnchor, Warning};

const TEXT: &[u8] = include_bytes!("fixtures/text.svg");

fn scene(bytes: &[u8]) -> Scene {
    tinker_pdf_svg::read(bytes, Some((200.0, 200.0)), &Limits::DEFAULT).expect("the fixture reads")
}

/// Every text run of a scene, as `(characters, anchor)`.
fn runs(scene: &Scene) -> Vec<(String, Option<[f64; 2]>)> {
    scene
        .nodes
        .iter()
        .filter_map(|node| match node {
            Node::Text { text, anchor, .. } => Some((text.clone(), *anchor)),
            _ => None,
        })
        .collect()
}

fn font(scene: &Scene, at: usize) -> tinker_pdf_svg::TextStyle {
    match scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Text { .. }))
        .nth(at)
    {
        Some(Node::Text { font, .. }) => font.clone(),
        other => panic!("no text run {at}: {other:?}"),
    }
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

/// A `<text>` with an absolute position is one run at that point.
#[test]
fn a_text_element_opens_a_chunk_at_its_own_position() {
    let scene = scene(TEXT);
    let runs = runs(&scene);
    assert_eq!(runs[0].0, "Hello");
    let anchor = runs[0].1.expect("an absolute position opens a chunk");
    near(anchor[0], 10.0, "x");
    near(anchor[1], 20.0, "y");
}

/// §10.9: a chunk opens at an **absolute** position and nowhere else.
///
/// The four runs of the second `<text>` are the whole rule in one line. `One`
/// opens a chunk; `two` states nothing and continues it; `three` states an `x`
/// and opens a new one; `four` states only a `y` and **keeps the `x` the chunk
/// before it had** — which is §10.4's rule and the one a build that defaulted
/// the missing axis to zero gets wrong, by putting the word at the left margin.
#[test]
fn a_chunk_opens_only_at_an_absolute_position() {
    let scene = scene(TEXT);
    let runs = runs(&scene);
    assert_eq!(runs[1].0, "One");
    assert!(runs[1].1.is_some(), "the `<text>` opened a chunk");

    assert_eq!(runs[2].0, "two");
    assert_eq!(
        runs[2].1, None,
        "a `<tspan>` that states nothing continues, and where it begins is a \
         metric this crate does not have"
    );

    assert_eq!(runs[3].0, "three");
    let third = runs[3].1.expect("an `x` opens a chunk");
    near(third[0], 80.0, "the x it stated");
    near(
        third[1],
        40.0,
        "and the y it inherited from the chunk before",
    );

    assert_eq!(runs[4].0, "four");
    let fourth = runs[4].1.expect("a `y` opens a chunk too");
    near(fourth[0], 80.0, "§10.4: the x the previous chunk had");
    near(fourth[1], 60.0, "and the y it stated");
}

/// The font properties resolve through the same §6.4 machinery as everything
/// else, and they inherit.
#[test]
fn the_font_properties_are_resolved_and_inherited() {
    let scene = scene(TEXT);
    let outer = font(&scene, 0);
    assert_eq!(
        outer.families,
        ["Georgia", "Times New Roman", "Zapf Chancery", "serif"],
        "`css-fonts-4` §2.2: an **unquoted** family is a sequence of \
         identifiers joined by single spaces, so `Times New Roman` with no \
         quotes is one family and not three — and a quoted one is one name \
         however many spaces it holds"
    );
    near(outer.size, 12.0, "the root's own size");
    assert_eq!(outer.weight, 400);
    assert!(!outer.italic);
    assert_eq!(outer.anchor, TextAnchor::Start);

    // The third `<text>` is bold, and its second `<tspan>` is italic at `2em`
    // of the parent's twelve.
    let heavy = font(&scene, 5);
    assert_eq!(heavy.weight, 700, "inherited from the `<text>`");
    assert!(!heavy.italic);
    let slanted = font(&scene, 6);
    assert_eq!(slanted.weight, 700, "still inherited");
    assert!(slanted.italic, "and overridden on the `<tspan>`");
    near(slanted.size, 24.0, "`2em` of the parent's twelve");
}

/// The paint reaches a run, through the same resolution a shape uses.
#[test]
fn a_run_carries_the_paint_it_resolved() {
    let scene = scene(TEXT);
    let Some(Node::Text { fill, .. }) = scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Text { .. }))
        .nth(6)
    else {
        panic!("the slanted run");
    };
    assert_eq!(
        *fill,
        Paint::Solid(Colour {
            rgb: [1.0, 0.0, 0.0]
        })
    );
}

/// §10.9's `text-anchor` reaches the caller **unapplied**.
///
/// It cannot be applied here: centring a chunk needs its width, and a width is
/// a font metric. A build that applied it with a guessed advance would place
/// every centred label slightly wrong and look entirely plausible.
#[test]
fn text_anchor_is_carried_rather_than_applied() {
    let scene = scene(TEXT);
    let centred = font(&scene, 7);
    assert_eq!(centred.anchor, TextAnchor::Middle);
    let runs = runs(&scene);
    let anchor = runs[7].1.expect("a chunk");
    near(
        anchor[0],
        100.0,
        "the anchor is where the file put it, not where the centred text starts",
    );
}

/// §10.4: an `x` with more than one number is per-glyph positioning, which is
/// named rather than taken silently.
#[test]
fn a_position_list_is_named_and_its_first_number_used() {
    let scene = scene(TEXT);
    assert!(
        scene.warnings.contains(&Warning::TextPositionListIgnored),
        "{:?}",
        scene.warnings
    );
    let runs = runs(&scene);
    let abc = runs
        .iter()
        .find(|(text, _)| text == "abc")
        .expect("the run");
    near(abc.1.expect("a chunk")[0], 10.0, "the first of the three");
}

/// §10.13's `<textPath>` and its relatives are refused by name.
#[test]
fn text_on_a_path_is_refused_by_name() {
    let scene = scene(TEXT);
    assert!(
        scene.warnings.contains(&Warning::TextLayoutUnsupported),
        "{:?}",
        scene.warnings
    );
    assert!(
        !runs(&scene).iter().any(|(text, _)| text == "along"),
        "and its characters are not set at the wrong place instead"
    );
}

/// §10.15's `xml:space="default"`, in the order §10.15 states it.
///
/// The order is load-bearing: a newline **removed** joins two words that a
/// newline *turned into a space* would have kept apart, and only a fixture
/// with a newline inside a run can tell the two apart.
#[test]
fn white_space_is_collapsed_the_way_section_10_15_says() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text x=\"0\" y=\"0\">   spaced\tout\n   again   </text></svg>";
    let spaced = scene(markup.as_bytes());
    assert_eq!(runs(&spaced)[0].0, "spaced out again");

    // A newline is removed rather than collapsed, so a word broken across two
    // source lines is one word.
    let joined = "<svg xmlns=\"http://www.w3.org/2000/svg\"><text>abc\ndef</text></svg>";
    let joined = scene(joined.as_bytes());
    assert_eq!(runs(&joined)[0].0, "abcdef");
}

/// A run whose text collapses to nothing is not a run.
#[test]
fn a_run_of_pure_white_space_is_not_a_node() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text x=\"0\" y=\"0\">\n   \n</text></svg>";
    assert!(scene(markup.as_bytes()).nodes.is_empty());
}

/// `visibility: hidden` keeps a run out of the display list, exactly as it
/// keeps a shape out.
#[test]
fn a_hidden_run_does_not_reach_the_scene() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text visibility=\"hidden\">gone</text><text>here</text></svg>";
    let scene = scene(markup.as_bytes());
    assert_eq!(runs(&scene).len(), 1);
    assert_eq!(runs(&scene)[0].0, "here");
}

/// A `<text>` inside a transformed group carries the composed matrix, so the
/// caller sets the glyphs where the document put them.
#[test]
fn a_run_carries_every_transform_above_it() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <g transform=\"translate(5,7)\"><text x=\"1\" y=\"2\" transform=\"scale(2)\">t</text>\
        </g></svg>";
    let scene = scene(markup.as_bytes());
    let Some(Node::Text { matrix, anchor, .. }) = scene.nodes.first() else {
        panic!("a run");
    };
    let anchor = anchor.expect("a chunk");
    let placed = tinker_pdf_svg::transform::apply(*matrix, anchor);
    near(placed[0], 7.0, "scale(2) of x=1, then translate(5)");
    near(placed[1], 11.0, "and the same on y");
}

/// A `<tspan>`'s own `transform` composes onto the `<text>`'s.
///
/// §7.6 makes a child's matrix apply first, in its parent's space, and a
/// `<tspan>` is no exception — so a scale inside a translate scales and then
/// moves. A build that took only the `<text>`'s matrix would set every
/// transformed span at the parent's scale, which is a line of text that is
/// simply the wrong size.
#[test]
fn a_tspan_composes_its_own_transform_onto_the_texts() {
    let scene = scene(TEXT);
    let moved = scene
        .nodes
        .iter()
        .find_map(|node| match node {
            Node::Text { text, matrix, .. } if text == "moved" => Some(*matrix),
            _ => None,
        })
        .expect("the transformed span");
    let placed = tinker_pdf_svg::transform::apply(moved, [1.0, 1.0]);
    near(placed[0], 5.0, "scale(2) of one, then translate(3)");
    near(placed[1], 6.0, "and the same on y");
}

/// `dx`/`dy` on a **continuing** run travel in the matrix, because the pen
/// they offset from is the caller's.
#[test]
fn a_shift_on_a_continuing_run_travels_in_the_matrix() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text x=\"0\" y=\"0\">a<tspan dx=\"3\" dy=\"4\">b</tspan></text></svg>";
    let scene = scene(markup.as_bytes());
    let Some(Node::Text { matrix, anchor, .. }) = scene.nodes.get(1) else {
        panic!("the second run");
    };
    assert_eq!(*anchor, None, "it still continues the chunk");
    let shifted = tinker_pdf_svg::transform::apply(*matrix, [0.0, 0.0]);
    near(shifted[0], 3.0, "the dx");
    near(shifted[1], 4.0, "and the dy");
}
