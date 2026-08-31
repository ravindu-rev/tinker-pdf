//! `css-cascade-5` §7.1's five explicit defaulting keywords.
//!
//! Five keywords, **five different answers**, and the tests are arranged around
//! the two pairs that are easy to collapse into one:
//!
//! * `unset` against `inherit` and `initial`. §7.1 defines it as whichever of
//!   the two the property's own inheritance names, so a build that picked one
//!   is right on about a third of the properties and wrong on the rest with
//!   nothing to show for it.
//! * `revert` against `revert-layer`. They differ in **what they roll back
//!   past** — a whole cascade origin, or one `@layer` inside the origin the
//!   declaration is already in. `the_two_rollbacks_are_not_the_same_keyword`
//!   is the fixture where the two give different answers, which is the only
//!   kind of fixture that can tell them apart.
//!
//! # Counted injections
//!
//! **Eight defects, reintroduced one at a time, and the tests each one broke.**
//! Every count below was measured by making the edit and running the suite; not
//! one of them is the number that was expected before it was run, which is the
//! reason they are measured.
//!
//! | injected | tests fired |
//! | --- | --- |
//! | `unset` resolved as `inherit` for every property | 2 |
//! | `unset` resolved as `initial` for every property | 3 |
//! | `inherit` reading the initial style rather than the parent's | 3 |
//! | `revert` implemented as `revert-layer` | 1 |
//! | `revert-layer` implemented as `revert` | 2 |
//! | a `revert` with nothing beneath it resolved as `initial` rather than `unset` | 1 |
//! | a longhand missing from `Longhand::ALL` | 2 |
//! | a shorthand that defaults fewer longhands than it sets | 1 |
//!
//! **None of the eight fires zero**, and that is worth saying rather than
//! leaving to be inferred: a zero would mean a branch this file claims to cover
//! and does not. The two that come closest to one are the pair that matters
//! most. `revert` implemented as `revert-layer` breaks exactly one test —
//! [`the_two_rollbacks_are_not_the_same_keyword`] — because it is the only
//! fixture here in which the two keywords are asked the same question in a tree
//! where they have different answers. Every other rollback fixture has one
//! layer or one origin, and under those the two *are* the same program. That
//! one test is therefore carrying the whole distinction, which is why it is
//! written as a single fixture asserting both answers and their inequality
//! rather than as two tests that could each pass alone.
//!
//! The sixth was expected to fire zero and fires one. With nothing beneath a
//! `revert`, `unset` and `initial` differ only on an inherited property whose
//! parent set a value — which is precisely the fixture
//! [`a_revert_with_nothing_beneath_it_is_unset`] is built out of, and it
//! catches it.

use super::{sheet, tree, Node};
use crate::cascade::{cascade, ComputedStyle, Origin};
use crate::longhand::Longhand;
use crate::property::{
    Color, Declaration, Defaulting, LengthPercentage, MarginValue, DEFAULTABLE_SHORTHANDS,
    IMPLEMENTED_NAMES,
};
use crate::{Budget, Limits};

/// An opaque colour, which the fixtures write as a hex triple.
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

/// A parent and one child, which is the smallest tree inheritance can be seen
/// in.
fn pair() -> Vec<Node> {
    tree(&[("div", None), ("p", Some(0))])
}

/// Cascades one author sheet over a parent and child, and returns the child's
/// style.
fn child(source: &str) -> ComputedStyle {
    let nodes = pair();
    let parsed = sheet(source);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
        .styles
        .remove(1)
}

/// The same, over as many origins as the caller names.
fn child_over(sheets: &[(Origin, &str)]) -> ComputedStyle {
    let nodes = pair();
    let parsed: Vec<_> = sheets.iter().map(|(o, s)| (*o, sheet(s))).collect();
    let refs: Vec<_> = parsed.iter().map(|(o, s)| (*o, s)).collect();
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&refs, &nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
        .styles
        .remove(1)
}

// ---- inherit, initial, unset -------------------------------------------------

/// **`inherit` takes the parent's value even when the property does not
/// inherit.**
///
/// `border-top-width` is not inherited, so the child starts at the initial
/// value whatever the parent has. That is what makes it the property worth
/// asserting on: on an inherited one, `inherit` and doing nothing agree, and a
/// build that implemented `inherit` as a no-op would pass.
#[test]
fn inherit_takes_the_parent_value_on_a_property_that_does_not_inherit() {
    let declared = child("div { border-top-width: 7px } p { border-top-width: inherit }");
    let control = child("div { border-top-width: 7px }");
    assert_eq!(declared.border_width.top, 7.0, "the parent's value");
    assert_ne!(
        control.border_width.top, 7.0,
        "the fixture is worthless unless the control differs"
    );
    assert_eq!(
        control.border_width.top,
        ComputedStyle::initial().border_width.top,
        "and the control is the initial value"
    );
}

/// **`initial` drops the parent's value even when the property does inherit.**
///
/// The mirror of the test above, and it needs an inherited property for the
/// same reason: on a non-inherited one `initial` and doing nothing agree.
#[test]
fn initial_drops_the_parent_value_on_a_property_that_does_inherit() {
    let declared = child("div { color: #ff0000 } p { color: initial }");
    let control = child("div { color: #ff0000 }");
    assert_eq!(
        declared.color,
        ComputedStyle::initial().color,
        "back to the initial colour"
    );
    assert_eq!(
        control.color,
        rgb(255, 0, 0),
        "the fixture is worthless unless the control inherits"
    );
}

/// **`unset` is `inherit` on an inherited property and `initial` on the rest**,
/// asserted on one of each in the same test.
///
/// Two properties because one cannot tell the three keywords apart: on `color`
/// alone, `unset` and `inherit` agree, and on `border-top-width` alone `unset`
/// and `initial` do. Only the pair pins it.
#[test]
fn unset_is_inherit_for_an_inherited_property_and_initial_for_the_rest() {
    let source = "div { color: #ff0000; border-top-width: 7px } \
                  p { color: unset; border-top-width: unset }";
    let style = child(source);
    assert_eq!(
        style.color,
        rgb(255, 0, 0),
        "`color` inherits, so `unset` is `inherit`"
    );
    assert_eq!(
        style.border_width.top,
        ComputedStyle::initial().border_width.top,
        "`border-top-width` does not, so `unset` is `initial`"
    );
    // And the two really are different answers on this fixture, which is what
    // says the assertions above are not both true by accident.
    assert_ne!(style.border_width.top, 7.0);
}

/// **`unset` is exactly not declaring the property**, on every property this
/// build implements.
///
/// §7.1's definition and [`ComputedStyle::inherit_from`]'s behaviour are the
/// same rule written twice, so this asserts they agree — over all eighty-three
/// longhands rather than a sample, because the two are only the same rule if
/// they are the same rule everywhere.
#[test]
fn unset_is_the_same_as_never_declaring_it_for_every_longhand() {
    let control = child("div { color: #ff0000; border-top-width: 7px; font-size: 30px }");
    let mut checked = 0usize;
    for longhand in Longhand::ALL {
        let source = format!(
            "div {{ color: #ff0000; border-top-width: 7px; font-size: 30px }} \
             p {{ {}: unset }}",
            longhand.name()
        );
        let style = child(&source);
        assert_eq!(
            style,
            control,
            "`{}: unset` changed something",
            longhand.name()
        );
        checked += 1;
    }
    assert_eq!(checked, 83, "every longhand, not a sample");
}

/// **`inherit` takes the parent's computed value, not its specified one.**
///
/// The parent's `font-size` is `2em` against a `10px` root, so its specified
/// value is `2em` and its computed value is `20px`. A child that inherited the
/// *specified* value would resolve `2em` against itself and land on `40px`,
/// which is a plausible number and the wrong one.
#[test]
fn inherit_takes_the_computed_value_and_not_the_specified_one() {
    let style = child("div { font-size: 2em } p { font-size: inherit }");
    let parent = {
        let nodes = pair();
        let parsed = sheet("div { font-size: 2em }");
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget)
            .expect("the fixture is under every cap")
            .styles
            .remove(0)
    };
    assert_eq!(style.font_size, parent.font_size, "the parent's pixels");
    assert!(
        style.font_size > 0.0 && style.font_size < parent.font_size * 1.5,
        "not the specified value re-resolved: {} against {}",
        style.font_size,
        parent.font_size
    );
}

/// On the root there is no parent, and §7.2 gives `inherit` the initial value —
/// so `inherit` and `initial` agree there, and this asserts they do.
#[test]
fn inherit_on_the_root_is_the_initial_value() {
    let nodes = tree(&[("html", None)]);
    let run = |source: &str| {
        let parsed = sheet(source);
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget)
            .expect("the fixture is under every cap")
            .styles
            .remove(0)
    };
    let inherited = run("html { color: inherit }");
    let initialised = run("html { color: initial }");
    assert_eq!(inherited.color, initialised.color);
    assert_eq!(inherited.color, ComputedStyle::initial().color);
}

/// A shorthand defaults every longhand it sets, and each one cascades on its
/// own.
///
/// The second half is the one worth a fixture: a `margin-top` written after
/// `margin: inherit` has to beat one of the four and leave the other three
/// alone, which is only true if the shorthand expanded.
#[test]
fn a_shorthand_defaults_every_longhand_and_each_cascades_alone() {
    let style = child("div { margin: 7px } p { margin: inherit; margin-top: 1px }");
    assert_eq!(
        style.margin.top,
        MarginValue::Length(LengthPercentage::Px(1.0))
    );
    for side in [style.margin.right, style.margin.bottom, style.margin.left] {
        assert_eq!(side, MarginValue::Length(LengthPercentage::Px(7.0)));
    }
}

// ---- revert and revert-layer -------------------------------------------------

/// **`revert` rolls back to the previous origin.**
///
/// The author's `revert` is resolved as though the author sheet had said
/// nothing about `color` on this element, which leaves the user-agent's rule —
/// not the initial value, and not the author's other rule.
#[test]
fn revert_rolls_back_to_the_previous_origin() {
    let style = child_over(&[
        (Origin::UserAgent, "p { color: #00ff00 }"),
        (Origin::Author, "p { color: #ff0000 } p { color: revert }"),
    ]);
    assert_eq!(
        style.color,
        rgb(0, 255, 0),
        "the user-agent value, not the author's and not the initial one"
    );
    assert_ne!(style.color, ComputedStyle::initial().color);
}

/// **`revert` with nothing beneath it is `unset`.**
///
/// §7.1: a `revert` in the weakest origin has nothing to roll back to, and the
/// property is computed as though it had not been declared. On an inherited
/// property with a parent that set one, that is the parent's value — which is
/// what tells `unset` apart from `initial` here.
#[test]
fn a_revert_with_nothing_beneath_it_is_unset() {
    let style = child("div { color: #ff0000 } p { color: revert }");
    assert_eq!(
        style.color,
        rgb(255, 0, 0),
        "unset on an inherited property is the parent's value"
    );
    assert_ne!(
        style.color,
        ComputedStyle::initial().color,
        "and not the initial value, which is what `initial` would have given"
    );
}

/// **`revert-layer` rolls back one layer, and `revert` rolls back the whole
/// origin.** The fixture where the two give different answers.
///
/// Three levels are in play: a user-agent rule, an author rule in layer `a`,
/// and an author rule in layer `b`. The declaration in `b` reverts.
///
/// * `revert-layer` in `b` must land on **`a`** — the previous layer of the
///   same origin.
/// * `revert` in `b` must land on **the user agent** — past every author layer.
///
/// A build with one implementation for both gives the same answer twice, and
/// this is the only shape of fixture that notices.
#[test]
fn the_two_rollbacks_are_not_the_same_keyword() {
    let ua = "p { color: #0000ff }";
    let author = |keyword: &str| {
        format!(
            "@layer a, b; \
             @layer a {{ p {{ color: #00ff00 }} }} \
             @layer b {{ p {{ color: {keyword} }} }}"
        )
    };

    let layered = child_over(&[
        (Origin::UserAgent, ua),
        (Origin::Author, &author("revert-layer")),
    ]);
    assert_eq!(
        layered.color,
        rgb(0, 255, 0),
        "`revert-layer` lands on the previous layer"
    );

    let origin = child_over(&[(Origin::UserAgent, ua), (Origin::Author, &author("revert"))]);
    assert_eq!(
        origin.color,
        rgb(0, 0, 255),
        "`revert` goes past every author layer to the user agent"
    );

    assert_ne!(
        layered.color, origin.color,
        "the fixture cannot tell the two keywords apart"
    );
}

/// A `revert-layer` with no earlier layer falls through to the previous origin,
/// which is what makes an unlayered one behave as `revert`.
#[test]
fn a_revert_layer_with_no_earlier_layer_falls_through_to_the_origin() {
    let style = child_over(&[
        (Origin::UserAgent, "p { color: #0000ff }"),
        (Origin::Author, "@layer a { p { color: revert-layer } }"),
    ]);
    assert_eq!(style.color, rgb(0, 0, 255), "the user-agent value");
}

/// Rollbacks chain: the declaration a `revert` lands on may itself be one.
///
/// Three author layers, the last two both reverting one layer. The answer is
/// the first layer's value, reached in two steps — and the loop that walks it
/// is bounded, which is what stops a cycle of them from hanging (ruling 1).
#[test]
fn a_rollback_that_lands_on_another_rollback_keeps_going() {
    let style = child_over(&[(
        Origin::Author,
        "@layer a, b, c; \
         @layer a { p { color: #00ff00 } } \
         @layer b { p { color: revert-layer } } \
         @layer c { p { color: revert-layer } }",
    )]);
    assert_eq!(style.color, rgb(0, 255, 0), "two steps back");
}

// ---- the tables `rustc` cannot check ----------------------------------------

/// **Every name this build implements can carry a defaulting keyword.**
///
/// `Longhand::ALL` is a list rather than a `match`, so nothing in the compiler
/// holds it to the property set; this does, against
/// [`IMPLEMENTED_NAMES`] — the same list the parser decides by. A name that
/// parses and that no keyword can be written on is a hole, and it is reported
/// here by name rather than discovered in a book.
#[test]
fn every_implemented_name_is_defaultable() {
    let mut longhands = 0usize;
    let mut shorthands = 0usize;
    for name in IMPLEMENTED_NAMES {
        if Longhand::from_name(name).is_some() {
            longhands += 1;
        } else if DEFAULTABLE_SHORTHANDS.iter().any(|(n, _)| n == name) {
            shorthands += 1;
        } else {
            panic!("`{name}: inherit` names a property nothing here can default");
        }
    }
    assert_eq!(longhands, 83, "eighty-three longhands");
    assert_eq!(shorthands, 16, "sixteen shorthands");
    assert_eq!(longhands + shorthands, IMPLEMENTED_NAMES.len());

    // And the other direction: nothing in the tables is a name the parser does
    // not know, which would be a keyword accepted on a property that does not
    // exist.
    for longhand in Longhand::ALL {
        assert!(
            IMPLEMENTED_NAMES.contains(&longhand.name()),
            "{} is a longhand and not an implemented name",
            longhand.name()
        );
    }
    for (name, expansion) in DEFAULTABLE_SHORTHANDS {
        assert!(IMPLEMENTED_NAMES.contains(name), "{name}");
        assert!(!expansion.is_empty(), "{name} expands to nothing");
        for target in *expansion {
            assert!(
                Longhand::from_name(target).is_some(),
                "{name} expands to {target}, which is not a longhand"
            );
        }
    }
}

/// **A shorthand defaults the same longhands it sets.**
///
/// [`DEFAULTABLE_SHORTHANDS`] is a second table beside the expansion
/// `property::implemented` performs, and two tables that must agree are worth
/// one test. Each shorthand is parsed twice — once with a real value, once with
/// `inherit` — and the two longhand sets are compared.
///
/// The sample values are the point of failure if this ever goes red: a value
/// that stopped parsing would make the left-hand side empty and the assertion
/// would say so rather than passing on two empty sets.
#[test]
fn every_shorthand_expands_the_same_way_for_a_value_and_a_keyword() {
    let samples: [(&str, &str); 16] = [
        ("background", "#ff0000"),
        ("border", "1px solid #ff0000"),
        ("border-bottom", "1px solid #ff0000"),
        ("border-color", "#ff0000"),
        ("border-left", "1px solid #ff0000"),
        ("border-right", "1px solid #ff0000"),
        ("border-style", "solid"),
        ("border-top", "1px solid #ff0000"),
        ("border-width", "1px"),
        ("column-rule", "1px solid #ff0000"),
        ("columns", "2 auto"),
        ("flex", "1 1 auto"),
        ("flex-flow", "row wrap"),
        ("gap", "10px"),
        ("margin", "0"),
        ("padding", "0"),
    ];
    assert_eq!(
        samples.len(),
        DEFAULTABLE_SHORTHANDS.len(),
        "a sample per shorthand"
    );

    for (name, value) in samples {
        let declarations = |source: String| {
            let parsed = sheet(&format!("p {{ {source} }}"));
            parsed.rules[0]
                .declarations
                .iter()
                .map(|d| match &d.declaration {
                    Declaration::Known(property) => property.longhand(),
                    Declaration::Defaulted { longhand, .. } => *longhand,
                    other => panic!("{name}: {other:?}"),
                })
                .collect::<Vec<_>>()
        };
        let mut from_value = declarations(format!("{name}: {value}"));
        let mut from_keyword = declarations(format!("{name}: inherit"));
        assert!(
            !from_value.is_empty(),
            "the sample value for `{name}` stopped parsing"
        );
        from_value.sort();
        from_value.dedup();
        from_keyword.sort();
        from_keyword.dedup();
        assert_eq!(
            from_value, from_keyword,
            "`{name}` sets one set of longhands and defaults another"
        );
    }
}

/// The five keywords are five, and each parses to its own variant.
///
/// Cheap, and it is the assertion that catches the edit where two of them are
/// mapped onto one because they "do the same thing" — which two of them very
/// nearly do, on most fixtures.
#[test]
fn the_five_keywords_are_five() {
    let names = ["inherit", "initial", "unset", "revert", "revert-layer"];
    let mut seen = Vec::new();
    for name in names {
        let keyword = Defaulting::from_name(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(keyword.name(), name, "the name round-trips");
        assert!(!seen.contains(&keyword), "{name} is a duplicate of another");
        seen.push(keyword);
    }
    assert_eq!(seen.len(), 5);
    assert_eq!(Defaulting::from_name("inherits"), None);
    assert_eq!(Defaulting::from_name("revertlayer"), None);
    assert_eq!(Defaulting::from_name(""), None);
}
