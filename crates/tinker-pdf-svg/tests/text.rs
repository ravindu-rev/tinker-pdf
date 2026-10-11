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
//! | a list's later numbers are dropped | 3 |
//! | an ancestor's list does not reach through a `<tspan>` | 1 |
//! | a `y` without an `x` resets `x` to the chunk's | 2 |
//! | a `dx` shifts only the run it is written on | 1 |
//! | `rotate` past its list's end is zero | 1 |
//! | `<textPath>` is drawn as ordinary text | 1 |
//! | a newline becomes a space rather than being removed | 1 |
//! | leading white space is kept | 1 |
//! | a hidden run is still drawn | 1 |
//! | a continuing run's `dx`/`dy` is dropped | 1 |
//! | a `<tspan>` does not compose its own `transform` | 1 |
//! | a `<text>`'s own `opacity` is dropped | 1 |
//! | a `<tspan>`'s own `opacity` is dropped | 1 |
//!
//! Twenty-one injections, no zeros — after two were found at milestone 6. The fixture's only
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
/// and opens a new one; `four` states only a `y`, which opens a chunk that
/// **continues in `x`** from where `three` left the pen — §10.5's rule (b).
///
/// *Corrected after the milestones*: this test said `four` *"keeps the `x`
/// the chunk before it had"* and asserted 80, which put the word back under
/// `three`. §10.5 says the `x` of a character with no `x` of its own is the
/// current text position's, and the current text position is past `three`.
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
    near(fourth[0], 0.0, "no `dx`: the pen, wherever `three` left it");
    near(fourth[1], 60.0, "and the y it stated");
    let Some(Node::Text { continues_x, .. }) = scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Text { .. }))
        .nth(4)
    else {
        panic!("the fourth run");
    };
    assert!(*continues_x, "its x is an offset from the pen");
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

/// §10.4: an `x` with more than one number is per-glyph positioning — each
/// character at its own number, and each one a chunk of its own.
#[test]
fn an_x_per_character_places_each_one() {
    let scene = scene(TEXT);
    let runs = runs(&scene);
    let at = runs
        .iter()
        .position(|(text, _)| text == "a")
        .expect("the first of the three");
    let placed: Vec<(&str, [f64; 2])> = runs[at..at + 3]
        .iter()
        .map(|(text, anchor)| (text.as_str(), anchor.expect("a chunk each")))
        .collect();
    assert_eq!(
        placed,
        [
            ("a", [10.0, 140.0]),
            ("b", [20.0, 140.0]),
            ("c", [30.0, 140.0])
        ]
    );
}

/// One run, as `(text, anchor, continues_x, x shift of the matrix, rotate)`.
type Laid = (String, Option<[f64; 2]>, bool, f64, f64);

/// Every run of a small document, as [`Laid`]s.
fn laid(markup: &str) -> Vec<Laid> {
    let scene =
        scene(format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{markup}</svg>").as_bytes());
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    scene
        .nodes
        .iter()
        .filter_map(|node| match node {
            Node::Text {
                text,
                anchor,
                continues_x,
                matrix,
                rotate,
                ..
            } => Some((text.clone(), *anchor, *continues_x, matrix[4], *rotate)),
            _ => None,
        })
        .collect()
}

/// §10.5: an ancestor's list goes on applying **through** a `<tspan>` that
/// states none, and a `<tspan>`'s own list wins for its characters and then
/// gives way to the ancestor's again.
///
/// The `<text>`'s four numbers belong to its four characters in document
/// order, whichever element each sits in; in the second line the `<tspan>`'s
/// one number takes `b`, and `c` — past its list — takes the `<text>`'s third.
#[test]
fn an_ancestors_list_reaches_through_its_descendants() {
    let first = laid("<text x=\"0 10 20 30\" y=\"0\">a<tspan>bc</tspan>d</text>");
    let xs: Vec<(String, f64)> = first
        .iter()
        .map(|(text, anchor, ..)| (text.clone(), anchor.expect("a chunk")[0]))
        .collect();
    assert_eq!(
        xs,
        [
            ("a".to_owned(), 0.0),
            ("b".to_owned(), 10.0),
            ("c".to_owned(), 20.0),
            ("d".to_owned(), 30.0)
        ]
    );
    let second = laid("<text x=\"0 10 20 30\" y=\"0\">a<tspan x=\"100\">bc</tspan>d</text>");
    let xs: Vec<f64> = second
        .iter()
        .map(|(_, anchor, ..)| anchor.expect("a chunk")[0])
        .collect();
    assert_eq!(xs, [0.0, 100.0, 20.0, 30.0]);
}

/// §10.5's rule (b): a character with a `y` and no `x` opens a chunk at that
/// `y` and **continues** in `x` from where the previous glyph left the pen —
/// which is the caller's to know, so the anchor's `x` is the `dx` to add.
#[test]
fn a_y_without_an_x_continues_along_the_line() {
    let runs = laid("<text x=\"10\" y=\"40\">One<tspan y=\"60\" dx=\"2\">four</tspan></text>");
    assert_eq!(
        runs[1],
        ("four".to_owned(), Some([2.0, 60.0]), true, 0.0, 0.0),
        "a chunk at y = 60 whose x is the pen's plus the dx"
    );
}

/// `dx` and `dy` **move the current text position**, and every glyph after
/// them stands where they put it — not only the glyph they were written on.
///
/// `c` follows a `<tspan dx="3">`, so it is three units along as well, and
/// its run carries the shift in its matrix exactly as `b`'s does. The first
/// draft of this crate carried a continuing run's shift on that run alone, so
/// the text after a nudged word slid back under it.
#[test]
fn a_shift_persists_for_the_glyphs_after_it() {
    let runs = laid("<text x=\"0\" y=\"0\">a<tspan dx=\"3\">b</tspan>c</text>");
    let shifts: Vec<(String, f64)> = runs
        .iter()
        .map(|(text, _, _, shift, _)| (text.clone(), *shift))
        .collect();
    assert_eq!(
        shifts,
        [
            ("a".to_owned(), 0.0),
            ("b".to_owned(), 3.0),
            ("c".to_owned(), 3.0)
        ]
    );
    // A `dx` list is per character, and once it is spent the rest join one
    // run: `b` and `c` have one shift between them.
    let listed = laid("<text x=\"0\" y=\"0\" dx=\"1 2\">abc</text>");
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(
        listed[0].1,
        Some([1.0, 0.0]),
        "the first dx is in the anchor"
    );
    assert_eq!((listed[1].0.as_str(), listed[1].3), ("bc", 2.0));
    // And a `dy` moves the line for the chunks after it.
    let lowered = laid("<text x=\"0 10\" y=\"5\" dy=\"0 3\">ab</text>");
    assert_eq!(lowered[1].1, Some([10.0, 8.0]), "five, and the three since");
}

/// §10.5's `rotate`: each glyph turns about its own origin, and past the end
/// of the list the **last** number goes on applying.
#[test]
fn rotate_turns_each_glyph_and_its_last_number_persists() {
    let runs = laid("<text x=\"0\" y=\"0\" rotate=\"10 20\">abc<tspan>d</tspan></text>");
    let turned: Vec<(String, f64)> = runs
        .iter()
        .map(|(text, _, _, _, rotate)| (text.clone(), *rotate))
        .collect();
    assert_eq!(
        turned,
        [
            ("a".to_owned(), 10.0),
            ("b".to_owned(), 20.0),
            ("c".to_owned(), 20.0),
            ("d".to_owned(), 20.0)
        ],
        "one run per turned glyph, the last number reaching into the tspan"
    );
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

/// `visibility: hidden` **lays a run out and paints nothing** (§11.5, and SVG
/// 2's *Controlling visibility*: a hidden element still affects text layout).
/// The run reaches the scene marked hidden, with no paint, so the caller moves
/// the pen past it: a hidden `<tspan>` between two visible runs keeps the
/// third where it would be were the second visible, and a hidden first run
/// still opens its chunk at the `<text>`'s position. A shape, which moves
/// nothing, is still left out (`paint.rs`).
///
/// *Corrected 9 October 2026, on the review of the formats lane*: this was
/// `a_hidden_run_does_not_reach_the_scene`, and the run left out set the text
/// after it where the hidden text began — the defect the roadmap's SVG row
/// had recorded as found and not fixed.
#[test]
fn a_hidden_run_is_laid_out_and_not_painted() {
    let hidden = |scene: &Scene| -> Vec<(String, bool, bool)> {
        scene
            .nodes
            .iter()
            .filter_map(|node| match node {
                Node::Text {
                    text,
                    hidden,
                    fill,
                    stroke,
                    ..
                } => Some((
                    text.clone(),
                    *hidden,
                    *fill == Paint::None && stroke.is_none(),
                )),
                _ => None,
            })
            .collect()
    };
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text visibility=\"hidden\" stroke=\"red\">gone</text><text>here</text></svg>";
    let first = scene(markup.as_bytes());
    assert_eq!(
        hidden(&first),
        [
            ("gone".to_owned(), true, true),
            ("here".to_owned(), false, false)
        ]
    );
    assert_eq!(runs(&first)[0].1, Some([0.0, 0.0]), "it opens its chunk");

    let between = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text x=\"5\" y=\"9\">AB<tspan visibility=\"hidden\">CD</tspan>EF</text></svg>";
    let middle = scene(between.as_bytes());
    assert_eq!(
        runs(&middle),
        [
            ("AB".to_owned(), Some([5.0, 9.0])),
            ("CD".to_owned(), None),
            ("EF".to_owned(), None)
        ],
        "one chunk, the hidden run in it"
    );
    assert_eq!(
        hidden(&middle).iter().map(|r| r.1).collect::<Vec<_>>(),
        [false, true, false]
    );

    // A descendant may turn it back on.
    let back = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text visibility=\"hidden\">A<tspan visibility=\"visible\">B</tspan></text></svg>";
    assert_eq!(
        hidden(&scene(back.as_bytes()))
            .iter()
            .map(|r| r.1)
            .collect::<Vec<_>>(),
        [true, false]
    );
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

/// A `<text>`'s own `opacity` is of its whole rendering: one filled run takes
/// it as an alpha, and two runs are a group.
///
/// One run painted once at `a` is that run at alpha `a`, so the first `<text>`
/// is a run and no group. The second has two runs, which a reader composites
/// against each other wherever their glyphs meet, so it is a group of two
/// opaque runs at the text's half — and the continuation still continues,
/// because a chunk is a property of the runs and not of where they are kept.
#[test]
fn a_texts_opacity_is_an_alpha_for_one_run_and_a_group_for_two() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <text x=\"0\" y=\"10\" opacity=\"0.5\">alone</text>\
        <text x=\"0\" y=\"30\" opacity=\"0.5\">a<tspan>b</tspan></text>\
        <text x=\"0\" y=\"50\">c<tspan opacity=\"0.25\">d</tspan></text></svg>";
    let scene = scene(markup.as_bytes());
    let Some(Node::Text { fill_opacity, .. }) = scene.nodes.first() else {
        panic!("the first text is a run: {:?}", scene.nodes);
    };
    near(*fill_opacity, 0.5, "folded into its alpha");
    let Some(Node::Group { nodes, opacity, .. }) = scene.nodes.get(1) else {
        panic!("the second text is a group: {:?}", scene.nodes);
    };
    near(*opacity, 0.5, "the text's own");
    let runs: Vec<(&str, Option<[f64; 2]>, f64)> = nodes
        .iter()
        .filter_map(|node| match node {
            Node::Text {
                text,
                anchor,
                fill_opacity,
                ..
            } => Some((text.as_str(), *anchor, *fill_opacity)),
            _ => None,
        })
        .collect();
    assert_eq!(
        runs,
        [("a", Some([0.0, 30.0]), 1.0), ("b", None, 1.0)],
        "two opaque runs, the second continuing the first"
    );
    // A `<tspan>`'s own opacity is its run's, and the run still continues.
    let Some(Node::Text {
        text,
        anchor,
        fill_opacity,
        ..
    }) = scene.nodes.get(3)
    else {
        panic!("the faded span: {:?}", scene.nodes);
    };
    assert_eq!((text.as_str(), *anchor), ("d", None));
    near(*fill_opacity, 0.25, "the span's own opacity, folded");
}
