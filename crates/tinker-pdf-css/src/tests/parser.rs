//! `css-syntax-3` §5's grammar, its error recovery, the at-rules, and decision
//! 5's three-way split.

use super::sheet;
use crate::longhand::Longhand;
use crate::media::{MediaContext, MediaType};
use crate::parser::{parse, Declared, LayerName, LayerPart};
use crate::property::{
    AlignContent, AlignItems, AlignSelf, BorderStyle, Color, ColumnCount, ColumnFill, ColumnSpan,
    Declaration, Defaulting, Display, FlexDirection, FlexWrap, Float, JustifyContent, Len,
    LengthPercentage, MarginValue, Position, Property, Side, Size, SpecifiedColumnWidth,
    SpecifiedGap, SpecifiedInset, SpecifiedMargin, SpecifiedMaxSize, SpecifiedMinSize,
    SpecifiedSize, SpecifiedVerticalAlign, ZIndex, IMPLEMENTED_NAMES, UNSUPPORTED_PROPERTIES,
};
use crate::{Budget, ImportResolver, Limits, NoImports, Warning};

fn declarations(source: &str) -> Vec<Declared> {
    let parsed = sheet(source);
    assert_eq!(parsed.rules.len(), 1, "{:?}", parsed.report);
    parsed.rules[0].declarations.clone()
}

fn known(source: &str) -> Vec<Property> {
    declarations(source)
        .into_iter()
        .filter_map(|declared| match declared.declaration {
            Declaration::Known(property) => Some(property),
            _ => None,
        })
        .collect()
}

/// CSS 2.2 §17.6.1's `border-spacing` is `<length> <length>?`, and **the two
/// lengths are two directions**.
///
/// One value copies to both and two do not. A build that kept the first number
/// twice is right about every stylesheet written with one value — which is
/// almost all of them — and puts the wrong gap between the rows of every table
/// whose author wrote two. The injection matrix found that nothing here
/// asserted it: every layout fixture set the computed value directly and never
/// went through this grammar.
///
/// A percentage is **`Malformed` and not `Unsupported`**, because §17.6.1's
/// grammar has no percentage in it at all: it is the author's mistake rather
/// than this build's gap, which is the same distinction `orphans: 2.5` is on
/// the other side of.
#[test]
fn border_spacing_takes_one_length_or_two_and_they_are_two_directions() {
    assert_eq!(
        known("table { border-spacing: 4px }"),
        vec![Property::BorderSpacing(Len::Px(4.0), Len::Px(4.0))]
    );
    assert_eq!(
        known("table { border-spacing: 2px 8px }"),
        vec![Property::BorderSpacing(Len::Px(2.0), Len::Px(8.0))]
    );
    // Three is not a form the grammar has.
    assert!(known("table { border-spacing: 1px 2px 3px }").is_empty());
    assert!(known("table { border-spacing: 10% }").is_empty());
}

/// §5.4.4: a malformed declaration is discarded to the next semicolon, the
/// ones either side of it survive, and the discard is **counted**.
///
/// The count is the half that matters. A build that silently discarded would
/// render the same page and have no way to say how much of the author's
/// stylesheet it threw away.
#[test]
fn a_malformed_declaration_is_discarded_to_the_next_semicolon() {
    let parsed = sheet("p { color: red; not a declaration; float: left }");
    assert_eq!(parsed.report.discarded_declarations, 1);
    let names: Vec<&'static str> = parsed.rules[0]
        .declarations
        .iter()
        .filter_map(|d| match &d.declaration {
            Declaration::Known(property) => Some(property.name()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["color", "float"]);
}

/// A semicolon inside a block or a function does not end a declaration.
///
/// §5.4.5 consumes the remnants of a bad declaration through balanced blocks,
/// which is why the recovery point is *the next top-level* semicolon and not
/// the next byte that happens to be one.
#[test]
fn a_semicolon_inside_a_block_does_not_end_a_declaration() {
    let parsed = sheet("p { color: rgb(1;2;3); float: left }");
    // The whole `color` declaration is one chunk and is discarded once, and
    // `float` survives — which it would not if the `;`s inside the function
    // had split the block into four.
    assert_eq!(parsed.report.discarded_declarations, 1);
    assert_eq!(known("p { color: rgb(1;2;3); float: left }").len(), 1);
}

/// §5.4.2: a qualified rule that reaches EOF with no block is a parse error
/// and everything read is discarded — counted, and the rules before it stand.
#[test]
fn a_rule_with_no_block_is_discarded_and_counted() {
    let parsed = sheet("p { color: red } span, div");
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(parsed.report.discarded_rules, 1);
}

/// A malformed rule is discarded **to the end of its block**, so the rule after
/// it is read. The two halves are asserted separately: how many rules survived,
/// and that the survivor is the right one.
#[test]
fn a_malformed_rule_is_discarded_to_the_end_of_its_block() {
    let parsed = sheet("!!! { color: red } p { float: left }");
    assert_eq!(parsed.report.discarded_rules, 1);
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Float(Float::Left))
    );
}

/// §5.4.4's `!important`, at the end and case-insensitively — and not
/// anywhere else.
#[test]
fn important_is_the_last_two_values_and_is_case_insensitive() {
    let red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    for source in ["p { color: red !important }", "p { color: red !IMPORTANT }"] {
        let declared = declarations(source);
        assert!(declared[0].important, "{source}");
        assert_eq!(
            declared[0].declaration,
            Declaration::Known(Property::Color(red)),
            "the value survives the `!important` being stripped: {source}"
        );
    }
    // Not important, and still a perfectly good declaration.
    let ordinary = declarations("p { color: red }");
    assert!(!ordinary[0].important);
    // `!important` in the middle is part of the value, which makes the value
    // invalid — so this is a discarded declaration rather than an important one.
    let middle = sheet("p { color: red !important blue }");
    assert_eq!(middle.report.discarded_declarations, 1);
    // `!` alone is not `!important`.
    let bang = sheet("p { color: red ! }");
    assert_eq!(bang.report.discarded_declarations, 1);
}

/// `@media` is **evaluated**, and both wrong answers are excluded.
///
/// A build that ignored it would apply every rule inside every block; one that
/// dropped it would apply none. Each is asserted on its own, in both
/// directions, because a test that only checked the matching case cannot tell
/// "evaluated" from "always true".
#[test]
fn media_queries_are_evaluated_in_both_directions() {
    let source = "
        @media screen { p { float: left } }
        @media print { p { float: right } }
        @media (min-width: 100px) { div { float: left } }
        @media (min-width: 10000px) { div { float: right } }
    ";
    let parsed = sheet(source);
    let floats: Vec<&Property> = parsed
        .rules
        .iter()
        .flat_map(|rule| rule.declarations.iter())
        .filter_map(|d| match &d.declaration {
            Declaration::Known(property) => Some(property),
            _ => None,
        })
        .collect();
    assert_eq!(
        floats,
        vec![&Property::Float(Float::Left), &Property::Float(Float::Left)],
        "the screen block and the satisfiable width block, and neither other"
    );
}

/// This engine evaluates `@media` as `screen`, and the decision is asserted
/// rather than left to the module header.
#[test]
fn the_medium_is_screen() {
    assert_eq!(MediaContext::screen(1.0, 1.0).media, MediaType::Screen);
    let parsed = sheet("@media print { p { float: left } } @media screen { p { float: right } }");
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Float(Float::Right))
    );
}

/// A media feature this build does not read makes **its own** query false and
/// leaves the rest of the list alone.
#[test]
fn an_unreadable_media_query_is_false_and_does_not_spread() {
    let parsed = sheet(
        "@media (hover: hover) { p { float: left } }
         @media (hover: hover), screen { p { float: right } }
         @media (400px < width) { div { float: left } }",
    );
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Float(Float::Right)),
        "the comma list's second query still matches"
    );
}

/// A layer name, spelled out, for the fixtures below.
fn named(parts: &[&str]) -> LayerName {
    parts
        .iter()
        .map(|part| LayerPart::Named((*part).to_string()))
        .collect()
}

/// `@layer name { … }`: the rules inside are **kept**, and each carries the
/// layer it was written in.
///
/// This is the whole of what the parser owes the cascade. `css-cascade-5` §6.1
/// sorts on a position and the position is computed from the layer *tree* of a
/// whole origin, which one sheet cannot see — so what a sheet records is which
/// layer, and `cascade.rs` decides what that is worth.
#[test]
fn a_layer_block_keeps_its_rules_and_names_their_layer() {
    let parsed = sheet("@layer base { p { float: left } } p { float: right }");
    assert!(parsed.report.warnings.is_empty(), "{:?}", parsed.report);
    assert_eq!(parsed.layers, vec![named(&["base"])]);
    assert_eq!(parsed.rules.len(), 2);
    assert_eq!(parsed.rules[0].layer, Some(0));
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Float(Float::Left))
    );
    // And the rule after the block is unlayered, which is a different fact
    // from "in the last layer": §6.4.2 puts it in the implicit final layer,
    // and `None` is how that is spelled here.
    assert_eq!(parsed.rules[1].layer, None);
}

/// `@layer a, b, c;` declares the order and produces no rules at all.
///
/// The statement form exists so a sheet can fix its layer order at the top and
/// then write the blocks in whatever order suits it, which is exactly the case
/// a build that ordered layers by their *blocks* gets backwards.
#[test]
fn a_layer_statement_declares_order_and_no_rules() {
    let parsed = sheet("@layer a, b, c;");
    assert!(parsed.report.warnings.is_empty(), "{:?}", parsed.report);
    assert!(parsed.rules.is_empty());
    assert_eq!(
        parsed.layers,
        vec![named(&["a"]), named(&["b"]), named(&["c"])]
    );

    // And a block that follows it re-opens the layer that is already there
    // rather than declaring a second one — **first mention wins**, so the list
    // does not grow and `b` does not move.
    let reopened =
        sheet("@layer a, b; @layer b { p { float: left } } @layer a { p { float: right } }");
    assert_eq!(reopened.layers, vec![named(&["a"]), named(&["b"])]);
    assert_eq!(reopened.rules[0].layer, Some(1), "the `b` block is layer b");
    assert_eq!(reopened.rules[1].layer, Some(0), "the `a` block is layer a");
}

/// `@layer { … }` is a fresh layer every time, and **two of them are two
/// layers**.
///
/// A build that gave every anonymous layer the same identity would merge the
/// two blocks below into one, which is not a subtle wrong answer: it makes the
/// second block's rules lose to the first's on order alone.
#[test]
fn an_anonymous_layer_is_a_fresh_one_every_time() {
    let parsed = sheet("@layer { p { float: left } } @layer { p { float: right } }");
    assert_eq!(
        parsed.layers,
        vec![vec![LayerPart::Anonymous(0)], vec![LayerPart::Anonymous(1)]]
    );
    assert_eq!(parsed.rules[0].layer, Some(0));
    assert_eq!(parsed.rules[1].layer, Some(1));
}

/// A nested block and the dotted spelling are **the same layer**.
///
/// `@layer a { @layer b { … } }` is `a.b`, so a sheet that opens `a.b` later by
/// its dotted name adds to the layer the nesting already made rather than
/// making a second one beside it.
#[test]
fn a_nested_layer_resolves_to_its_dotted_name() {
    let parsed = sheet(
        "@layer a { @layer b { p { float: left } } }
         @layer a.b { p { float: right } }",
    );
    assert!(parsed.report.warnings.is_empty(), "{:?}", parsed.report);
    assert_eq!(parsed.layers, vec![named(&["a"]), named(&["a", "b"])]);
    assert_eq!(parsed.rules[0].layer, Some(1));
    assert_eq!(
        parsed.rules[1].layer,
        Some(1),
        "the same layer, not a second"
    );
}

/// `@media` and `@layer` nest **both ways round**, because both are books
/// somebody writes.
///
/// The at-rules inside a block used to be a second, shorter list than the one
/// at the top level, and a second list is how the two come to disagree: this
/// fixture is one construct from each direction, and the `@font-face` is there
/// because it is the at-rule that was already special-cased inside `@media`.
#[test]
fn media_and_layer_nest_in_either_order() {
    let parsed = sheet(
        "@layer a { @media screen { p { float: left } } }
         @media screen { @layer b { p { float: right } } }
         @layer c { @media print { p { float: left } } }
         @media screen { @font-face { font-family: X; src: url(x.ttf) } }",
    );
    assert!(parsed.report.warnings.is_empty(), "{:?}", parsed.report);
    assert_eq!(
        parsed.layers,
        vec![named(&["a"]), named(&["b"]), named(&["c"])]
    );
    assert_eq!(parsed.rules.len(), 2, "the print block does not match");
    assert_eq!(parsed.rules[0].layer, Some(0));
    assert_eq!(parsed.rules[1].layer, Some(1));
    assert_eq!(parsed.font_faces.len(), 1, "and @font-face still survives");
}

/// An `@layer` prelude the grammar does not admit discards the rule, and
/// **§6.4.1's no-whitespace clause is why one of these is not two layers**.
///
/// `@layer a b` is two names with no comma: invalid. A reader that skipped
/// whitespace would take it for `a.b` — a layer the author never wrote, holding
/// the rules that were meant for two.
#[test]
fn an_invalid_layer_prelude_discards_the_rule() {
    for source in [
        "@layer a b { p { float: left } }",
        "@layer a . b { p { float: left } }",
        "@layer a, b { p { float: left } }",
        "@layer 3 { p { float: left } }",
        "@layer a. { p { float: left } }",
        "@layer;",
    ] {
        let parsed = sheet(source);
        assert!(parsed.layers.is_empty(), "{source}");
        assert!(parsed.rules.is_empty(), "{source}");
        assert_eq!(parsed.report.discarded_rules, 1, "{source}");
    }
}

/// §6.4.1: an `@import` may name the layer its sheet lands in, and the clause
/// is taken off **before** the media query list is read.
///
/// A build that left it in hands `layer(a)` to the media evaluator, which
/// cannot read it, calls that query false and drops the whole sheet — a book
/// that layers its imports arriving unstyled with nothing anywhere saying why.
#[test]
fn an_import_may_name_the_layer_it_lands_in() {
    let table = Table(&[("one.css", "p { float: left }")]);

    let into_named = parse_with("@import url(one.css) layer(a);", &table);
    assert!(
        into_named.report.warnings.is_empty(),
        "{:?}",
        into_named.report
    );
    assert_eq!(into_named.layers, vec![named(&["a"])]);
    assert_eq!(into_named.rules.len(), 1);
    assert_eq!(into_named.rules[0].layer, Some(0));

    // Bare `layer` is the anonymous form, exactly as `@layer { … }` is.
    let anonymous = parse_with("@import url(one.css) layer;", &table);
    assert_eq!(anonymous.layers, vec![vec![LayerPart::Anonymous(0)]]);
    assert_eq!(anonymous.rules[0].layer, Some(0));

    // The media query list after the clause is still read — this is the pair
    // that says the clause was removed rather than the query skipped.
    let unmatched = parse_with("@import url(one.css) layer(a) print;", &table);
    assert!(unmatched.rules.is_empty(), "print does not match");
    let matched = parse_with("@import url(one.css) layer(a) screen;", &table);
    assert_eq!(matched.rules.len(), 1);
    assert_eq!(matched.rules[0].layer, Some(0));

    // And an unlayered `@import` is unchanged.
    let plain = parse_with("@import url(one.css);", &table);
    assert!(plain.layers.is_empty());
    assert_eq!(plain.rules[0].layer, None);
}

/// §3.3: a `@layer` **statement** does not close the `@import` window and a
/// `@layer` **block** does.
///
/// The clause names `@charset` and `@layer` as the two an `@import` may follow,
/// and it means the statement form — a block holds rules, and a rule is what
/// the window closes on. A build that closed the window on both would refuse
/// the `@import` in exactly the sheet that ordered its layers first, which is
/// the sheet most likely to have been written by somebody who read the spec.
#[test]
fn a_layer_statement_leaves_the_import_window_open() {
    let after_statement = parse_with(
        "@layer a, b; @import url(one.css);",
        &Table(&[("one.css", "p { float: left }")]),
    );
    assert!(
        after_statement.report.warnings.is_empty(),
        "{:?}",
        after_statement.report
    );
    assert_eq!(after_statement.rules.len(), 1);

    let after_block = parse_with(
        "@layer a { } @import url(one.css);",
        &Table(&[("one.css", "p { float: left }")]),
    );
    assert_eq!(
        after_block.report.warnings,
        vec![(Warning::ImportOutOfOrder, 1)]
    );
    assert!(after_block.rules.is_empty());
}

/// Every other at-rule is dropped **with its name**, which is decision 5's
/// shape one level up from a property.
///
/// `@font-face` used to be one of these and is read as of milestone 9, so the
/// fixture now uses two at-rules that are still unimplemented — and
/// `a_font_face_is_no_longer_an_unsupported_at_rule` below is what says the
/// name left this list rather than the warning quietly changing shape.
#[test]
fn an_unsupported_at_rule_carries_its_name() {
    let parsed = sheet("@page { margin: 1cm } @supports (x: y) { p { float: left } } @page { }");
    assert_eq!(
        parsed.report.warnings,
        vec![
            (Warning::AtRuleUnsupported("page".to_string()), 2),
            (Warning::AtRuleUnsupported("supports".to_string()), 1),
        ],
        "deduplicated by name, with the count beside each"
    );
}

/// A resolver over a fixed table, for the `@import` tests.
struct Table(&'static [(&'static str, &'static str)]);

impl ImportResolver for Table {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        self.0
            .iter()
            .find(|(name, _)| *name == href)
            .map(|(name, body)| ((*name).to_string(), body.as_bytes().to_vec()))
    }
}

fn parse_with(source: &str, resolver: &dyn ImportResolver) -> crate::Stylesheet {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    parse(
        source.as_bytes(),
        Some("root.css"),
        resolver,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("the fixture is under every cap")
}

/// `@import` splices the imported rules in at its own position, which is what
/// `css-cascade-5` §6.4.1's order of appearance requires.
#[test]
fn an_import_splices_its_rules_in_at_its_own_position() {
    let table = Table(&[("a.css", "p { float: left }")]);
    let parsed = parse_with("@import url(a.css); p { float: right }", &table);
    let floats: Vec<&Property> = parsed
        .rules
        .iter()
        .flat_map(|rule| rule.declarations.iter())
        .filter_map(|d| match &d.declaration {
            Declaration::Known(property) => Some(property),
            _ => None,
        })
        .collect();
    assert_eq!(
        floats,
        vec![
            &Property::Float(Float::Left),
            &Property::Float(Float::Right)
        ],
        "imported first, then the importing sheet's own"
    );
}

/// All three spellings of an `@import` target resolve, because real
/// stylesheets use all three.
#[test]
fn the_three_import_spellings_all_resolve() {
    let table = Table(&[("a.css", "p { float: left }")]);
    for source in [
        "@import url(a.css);",
        "@import url(\"a.css\");",
        "@import \"a.css\";",
    ] {
        assert_eq!(parse_with(source, &table).rules.len(), 1, "{source}");
    }
}

/// An `@import` after a qualified rule is invalid, and says so by name rather
/// than being read anyway.
#[test]
fn an_import_after_a_rule_is_named_rather_than_read() {
    let table = Table(&[("a.css", "p { float: left }")]);
    let parsed = parse_with("p { float: right } @import url(a.css);", &table);
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(parsed.report.warnings, vec![(Warning::ImportOutOfOrder, 1)]);
}

/// A cycle is **refused**, not recursed — and it is a different warning from
/// the depth cap, because it is a different fact about a book.
#[test]
fn an_import_cycle_is_refused_rather_than_recursed() {
    let table = Table(&[
        ("a.css", "@import url(b.css); p { float: left }"),
        ("b.css", "@import url(a.css); div { float: right }"),
    ]);
    let parsed = parse_with("@import url(a.css);", &table);
    assert_eq!(parsed.report.warnings, vec![(Warning::ImportCycle, 1)]);
    // Both sheets were still read once each, which is what "refused rather
    // than recursed" means: the cycle is cut, not the content.
    assert_eq!(parsed.rules.len(), 2);
    // A sheet importing itself is the same rule at depth one.
    let self_import = Table(&[("a.css", "@import url(a.css); p { float: left }")]);
    let direct = parse_with("@import url(a.css);", &self_import);
    assert_eq!(direct.report.warnings, vec![(Warning::ImportCycle, 1)]);
    assert_eq!(direct.rules.len(), 1);
}

/// An `@import` whose media query does not match is not fetched at all.
#[test]
fn an_import_is_gated_by_its_own_media_query() {
    let table = Table(&[("a.css", "p { float: left }")]);
    assert_eq!(
        parse_with("@import url(a.css) print;", &table).rules.len(),
        0
    );
    assert_eq!(
        parse_with("@import url(a.css) screen;", &table).rules.len(),
        1
    );
}

/// A target the resolver cannot find warns by its own name — not the cycle's
/// and not the depth cap's.
#[test]
fn an_unresolvable_import_warns_by_its_own_name() {
    let parsed = parse_with("@import url(missing.css);", &NoImports);
    assert_eq!(parsed.report.warnings, vec![(Warning::ImportUnresolved, 1)]);
}

/// CSS 2.1 §8.3's one-to-four-value expansion, all four arities.
///
/// The three-value case is the one a first implementation gets wrong: `1px 2px
/// 3px` is top, horizontal, bottom — the *left* comes from the second value,
/// not from a default.
#[test]
fn the_box_shorthand_expands_at_every_arity() {
    let px = |n: f64| SpecifiedMargin::Length(Len::Px(n));
    let sides = |source: &str| -> Vec<(Side, SpecifiedMargin)> {
        known(source)
            .into_iter()
            .map(|property| match property {
                Property::Margin(side, value) => (side, value),
                other => panic!("not a margin: {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        sides("p { margin: 1px }"),
        vec![
            (Side::Top, px(1.0)),
            (Side::Right, px(1.0)),
            (Side::Bottom, px(1.0)),
            (Side::Left, px(1.0)),
        ]
    );
    assert_eq!(
        sides("p { margin: 1px 2px }"),
        vec![
            (Side::Top, px(1.0)),
            (Side::Right, px(2.0)),
            (Side::Bottom, px(1.0)),
            (Side::Left, px(2.0)),
        ]
    );
    assert_eq!(
        sides("p { margin: 1px 2px 3px }"),
        vec![
            (Side::Top, px(1.0)),
            (Side::Right, px(2.0)),
            (Side::Bottom, px(3.0)),
            (Side::Left, px(2.0)),
        ]
    );
    assert_eq!(
        sides("p { margin: 1px 2px 3px 4px }"),
        vec![
            (Side::Top, px(1.0)),
            (Side::Right, px(2.0)),
            (Side::Bottom, px(3.0)),
            (Side::Left, px(4.0)),
        ]
    );
    // Five values is not a box.
    assert_eq!(
        sheet("p { margin: 1px 2px 3px 4px 5px }").rules[0]
            .declarations
            .len(),
        0
    );
}

/// The `border` shorthand sets all three longhands on every side it names, and
/// the ones the author omitted go to their **initial** values.
///
/// That is what makes `border: none` clear a border rather than leaving its
/// width behind — a build that only set what was written would keep a 3px
/// solid border and paint it in `none`'s absence.
#[test]
fn the_border_shorthand_resets_what_it_does_not_name() {
    let properties = known("p { border-top-width: 9px; border: none }");
    assert!(
        properties.contains(&Property::BorderWidth(Side::Top, Len::Px(3.0))),
        "the shorthand's own initial width, not the 9px above it: {properties:?}"
    );
    assert_eq!(
        properties
            .iter()
            .filter(|p| matches!(p, Property::BorderStyle(_, BorderStyle::None)))
            .count(),
        4
    );
    // One side only, and in any order.
    let one = known("p { border-left: solid 2px red }");
    assert!(one.contains(&Property::BorderWidth(Side::Left, Len::Px(2.0))));
    assert!(one.contains(&Property::BorderStyle(Side::Left, BorderStyle::Solid)));
    assert_eq!(one.len(), 3, "one side, three longhands: {one:?}");
}

/// **Decision 5's second device.** `float: inline-start` is not `float: left`.
///
/// The property is implemented and the value is not, so the declaration is
/// `Unsupported` and named — not mapped onto its nearest implemented
/// neighbour, which would produce a page that looks entirely reasonable and is
/// laid out for a writing mode this build refuses.
#[test]
fn a_value_outside_a_supported_property_is_unsupported_and_not_its_neighbour() {
    for (source, property, value) in [
        ("p { float: inline-start }", "float", "inline-start"),
        // `flex` stood here until milestone 12 implemented it. `grid` is the
        // successor and it is the same kind of value: a real `display` keyword,
        // one whose nearest implemented neighbour is now `flex` rather than
        // `block`, and one that would lay a two-dimensional layout out in one
        // dimension and look right on every grid with one row in it.
        ("p { display: grid }", "display", "grid"),
        // `table-cell` stood here until milestone 11 implemented it.
        // `inline-table` is the successor and it is the same kind of value: a
        // real CSS 2.2 §17.2 keyword, one this build's `Display` deliberately
        // does not have, and one whose nearest implemented neighbour --
        // `table` -- would put a table on a line of its own and look right.
        ("p { display: inline-table }", "display", "inline-table"),
        (
            "p { text-align: match-parent }",
            "text-align",
            "match-parent",
        ),
        ("p { color: rebeccapurple }", "color", "rebeccapurple"),
        ("p { width: 50vw }", "width", "50vw"),
    ] {
        let declared = declarations(source);
        assert_eq!(
            declared[0].declaration,
            Declaration::Unsupported {
                property,
                value: value.to_string()
            },
            "{source}"
        );
    }
    // And the implemented values still are implemented, which is what says the
    // assertions above are about the value and not about the property.
    assert_eq!(
        known("p { float: left }"),
        vec![Property::Float(Float::Left)]
    );
    assert_eq!(
        known("p { display: block }"),
        vec![Property::Display(Display::Block)]
    );
}

/// `Unsupported` and `Unknown` are different facts and are counted separately.
#[test]
fn unsupported_is_this_builds_gap_and_unknown_is_somebody_elses() {
    let parsed = sheet(
        "p {
            box-shadow: 0 0 2px #000;
            -webkit-box-shadow: 0 0 2px #000;
            -epub-text-emphasis-style: dot;
            -ah-margin-start: 1em;
            --brand: #333;
            colour: red;
            hyphens: auto;
         }",
    );
    assert_eq!(
        parsed.report.unsupported,
        vec![("box-shadow", 1), ("hyphens", 1)]
    );
    let unknown: Vec<&str> = parsed
        .report
        .unknown
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        unknown,
        vec![
            "-webkit-box-shadow",
            "-epub-text-emphasis-style",
            "-ah-margin-start",
            "--brand",
            "colour"
        ]
    );
}

/// A declaration whose **first** value is not an identifier is discarded and
/// counted too.
///
/// A survivor of the injection matrix, and it is worth the paragraph. §5.4.4
/// has two ways for a declaration to fail — the name is not an identifier, and
/// the identifier is not followed by a colon — and every fixture in this file
/// took the second: `not a declaration` starts with the identifier `not`, so
/// the branch that rejects a non-identifier name had never been run by
/// anything. Deleting its count changed no answer in the whole suite. A
/// fixture for one of two ways is a fixture for one of two ways.
#[test]
fn a_declaration_that_does_not_start_with_an_identifier_is_counted() {
    for source in [
        "p { 42px; color: red }",
        "p { \"quoted\"; color: red }",
        "p { #hash: 1; color: red }",
        "p { (parens): 1; color: red }",
        "p { 50%; color: red }",
    ] {
        let parsed = sheet(source);
        assert_eq!(parsed.report.discarded_declarations, 1, "{source}");
        assert_eq!(known(source).len(), 1, "{source}");
    }
}
/// **§7.1's five keywords are values, on a length-valued property too.**
///
/// This test was `a_css_wide_keyword_is_a_gap_on_a_length_valued_property_too`
/// and asserted the opposite, for a reason worth keeping: the keywords used to
/// be a gap, and the branch that filed them as one was reachable only here. A
/// colour that is not a colour is already `Unsupported` by the (property,
/// value) rule, so `color: inherit` reached the right answer either way; but an
/// identifier that is not one of a *length's* keywords is `Invalid`, so
/// `margin-top: inherit` would have been filed as the author's typo. The same
/// asymmetry is why this is still the test that matters now the keywords are
/// implemented -- if the defaulting branch were removed, `color: inherit` would
/// quietly become `Unsupported` and `margin-top: inherit` would quietly become
/// a discarded typo, and only the second of those loses the declaration
/// entirely.
///
/// All five keywords, on both shapes of property, and on a shorthand.
#[test]
fn a_css_wide_keyword_is_a_value_on_a_length_valued_property_too() {
    for (source, longhand, keyword) in [
        (
            "p { margin-top: inherit }",
            Longhand::MarginTop,
            Defaulting::Inherit,
        ),
        ("p { width: initial }", Longhand::Width, Defaulting::Initial),
        (
            "p { text-indent: revert }",
            Longhand::TextIndent,
            Defaulting::Revert,
        ),
        (
            "p { border-top-width: inherit }",
            Longhand::BorderWidthTop,
            Defaulting::Inherit,
        ),
        (
            "p { line-height: revert-layer }",
            Longhand::LineHeight,
            Defaulting::RevertLayer,
        ),
        (
            "p { letter-spacing: inherit }",
            Longhand::LetterSpacing,
            Defaulting::Inherit,
        ),
        ("p { color: inherit }", Longhand::Color, Defaulting::Inherit),
        (
            "p { display: initial }",
            Longhand::Display,
            Defaulting::Initial,
        ),
    ] {
        assert_eq!(
            declarations(source)[0].declaration,
            Declaration::Defaulted { longhand, keyword },
            "{source}"
        );
    }

    // A shorthand expands, exactly as it does for a value: four declarations
    // and not one, so a `padding-top` written after it beats one of them.
    let padding = declarations("p { padding: unset }");
    assert_eq!(padding.len(), 4, "padding expands to four longhands");
    assert!(padding.iter().all(|d| matches!(
        d.declaration,
        Declaration::Defaulted {
            keyword: Defaulting::Unset,
            ..
        }
    )));

    // And the direction that says this is about the five keywords and not about
    // identifiers in general: an identifier that is not one of them is not CSS
    // for a length at all, and is the author's error rather than this build's.
    let typo = sheet("p { margin-top: red }");
    assert_eq!(typo.report.discarded_declarations, 1);
    assert!(typo.report.unsupported.is_empty());

    // Nor is a keyword a keyword when it is only part of the value: §7.1 makes
    // the five valid *instead of* a property's own syntax, never inside it.
    // `margin: 0 inherit` reaches the `margin` grammar, which does not have
    // `inherit` in it, and comes out as this build's gap in `margin` rather
    // than as four defaulted longhands.
    let partial = declarations("p { margin: 0 inherit }");
    assert!(
        !partial
            .iter()
            .any(|d| matches!(d.declaration, Declaration::Defaulted { .. })),
        "a keyword inside a value is not §7.1 defaulting: {partial:?}"
    );
}

/// A property this build implements, at a value that is not CSS at all, is a
/// **discarded declaration** rather than an `Unsupported` one.
///
/// The distinction is the whole point of the `Unsupported` count: it is meant
/// to be a census of this build's gaps, and a stylesheet's own typos are not
/// gaps in this build.
#[test]
fn a_value_that_is_not_css_is_discarded_rather_than_counted_as_a_gap() {
    let parsed = sheet("p { margin-top: red; color: ; width: 3 }");
    assert_eq!(parsed.report.discarded_declarations, 3);
    assert!(parsed.report.unsupported.is_empty());
    assert!(parsed.report.unknown.is_empty());
}

/// The warning surface is deduplicated with counts, per device 3.
///
/// Four hundred elements with `float: left` must produce **one** warning with
/// the number beside it. Here it is four hundred rules with one unsupported
/// property.
#[test]
fn the_report_deduplicates_with_counts() {
    let mut source = String::new();
    for index in 0..400 {
        source.push_str(&format!(".c{index} {{ box-shadow: 0 0 2px #000 }}\n"));
    }
    let parsed = sheet(&source);
    assert_eq!(parsed.report.unsupported, vec![("box-shadow", 400)]);
}

/// The two name tables are disjoint.
///
/// A name in both would be reported as a gap this build does not have, and the
/// `As built` figure the whole gap is judged on would be wrong in the
/// flattering direction.
#[test]
fn no_property_is_both_implemented_and_unsupported() {
    for name in IMPLEMENTED_NAMES {
        assert!(
            !UNSUPPORTED_PROPERTIES.contains(name),
            "{name} is in both tables"
        );
    }
    // Both tables are sorted, so a new name has one obvious place to go and a
    // duplicate is visible in a diff.
    for table in [IMPLEMENTED_NAMES, UNSUPPORTED_PROPERTIES] {
        let mut sorted = table.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), table);
    }
}

/// Every name in `IMPLEMENTED_NAMES` actually parses to something, and every
/// name in `UNSUPPORTED_PROPERTIES` actually reports itself.
///
/// This is the test that keeps the two tables honest against the code rather
/// than against each other: a property removed from the parser and left in the
/// list would otherwise be counted as implemented for ever.
#[test]
fn both_tables_agree_with_the_parser() {
    for name in IMPLEMENTED_NAMES {
        let parsed = sheet(&format!("p {{ {name}: zzz }}"));
        let declared = &parsed.rules[0].declarations;
        let reported_as_unknown = declared
            .iter()
            .any(|d| matches!(&d.declaration, Declaration::Unknown { .. }));
        assert!(
            !reported_as_unknown,
            "{name} is in IMPLEMENTED_NAMES and the parser does not know it"
        );
    }
    for name in UNSUPPORTED_PROPERTIES {
        let parsed = sheet(&format!("p {{ {name}: zzz }}"));
        assert_eq!(
            parsed.report.unsupported,
            vec![(*name, 1)],
            "{name} is in UNSUPPORTED_PROPERTIES and did not report itself"
        );
    }
}

/// The colour syntaxes, including the two `hsl()` forms and both hex lengths
/// with alpha.
#[test]
fn the_colour_syntaxes() {
    let colour = |source: &str| match &known(&format!("p {{ color: {source} }}"))[0] {
        Property::Color(c) => *c,
        other => panic!("not a colour: {other:?}"),
    };
    let rgba = |r, g, b, a| Color { r, g, b, a };
    assert_eq!(colour("#f00"), rgba(255, 0, 0, 255));
    assert_eq!(colour("#ff0000"), rgba(255, 0, 0, 255));
    assert_eq!(colour("#ff000080"), rgba(255, 0, 0, 128));
    assert_eq!(colour("#f008"), rgba(255, 0, 0, 136));
    assert_eq!(colour("red"), rgba(255, 0, 0, 255));
    assert_eq!(colour("transparent"), rgba(0, 0, 0, 0));
    assert_eq!(colour("rgb(255, 0, 0)"), rgba(255, 0, 0, 255));
    assert_eq!(colour("rgba(255, 0, 0, 0.5)"), rgba(255, 0, 0, 128));
    assert_eq!(colour("rgb(100%, 0%, 0%)"), rgba(255, 0, 0, 255));
    assert_eq!(colour("hsl(0, 100%, 50%)"), rgba(255, 0, 0, 255));
    assert_eq!(colour("hsl(120, 100%, 50%)"), rgba(0, 255, 0, 255));
    assert_eq!(colour("hsl(240, 100%, 50%)"), rgba(0, 0, 255, 255));
    assert_eq!(colour("hsl(0, 0%, 100%)"), rgba(255, 255, 255, 255));
    assert_eq!(colour("hsla(0, 100%, 50%, 0.5)"), rgba(255, 0, 0, 128));
    // A hue outside 0–360 wraps rather than clamping, which is `css-color-4`'s
    // own rule and the one an implementation with a `clamp` gets wrong.
    assert_eq!(colour("hsl(480, 100%, 50%)"), colour("hsl(120, 100%, 50%)"));
    assert_eq!(
        colour("hsl(-120, 100%, 50%)"),
        colour("hsl(240, 100%, 50%)")
    );
}

/// `css-values-3` §5's absolute units, each converted to CSS pixels.
#[test]
fn the_absolute_length_units() {
    let indent = |source: &str| match &known(&format!("p {{ text-indent: {source} }}"))[0] {
        Property::TextIndent(len) => *len,
        other => panic!("not a text-indent: {other:?}"),
    };
    // The four whose arithmetic is exact in binary floating point, asserted
    // exactly — `in`, `pt` and `pc` are all whole-number ratios of 96.
    assert_eq!(indent("1px"), Len::Px(1.0));
    assert_eq!(indent("1in"), Len::Px(96.0));
    assert_eq!(indent("72pt"), Len::Px(96.0));
    assert_eq!(indent("6pc"), Len::Px(96.0));
    // The metric three are a division by a value that is not a binary fraction,
    // so `2.54cm` is 95.999999999999989 and saying otherwise would be asserting
    // something untrue about IEEE 754. Ruling 4 asks for the *same* answer on
    // every target, which a correctly-rounded multiply and divide give; it does
    // not ask for the decimal one.
    let near = |len: Len, wanted: f64| match len {
        Len::Px(px) => assert!((px - wanted).abs() < 1e-9, "{px} is not near {wanted}"),
        other => panic!("not an absolute length: {other:?}"),
    };
    near(indent("2.54cm"), 96.0);
    near(indent("25.4mm"), 96.0);
    near(indent("101.6q"), 96.0);
    assert_eq!(indent("2em"), Len::Em(2.0));
    assert_eq!(indent("2rem"), Len::Rem(2.0));
    // §5.1.1's own fallbacks, because this crate has no font by ruling 8.
    assert_eq!(indent("2ex"), Len::Em(1.0));
    assert_eq!(indent("2ch"), Len::Em(1.0));
    assert_eq!(indent("50%"), Len::Percent(50.0));
    // A unitless zero is a length; a unitless anything else is not.
    assert_eq!(indent("0"), Len::Px(0.0));
    assert_eq!(
        sheet("p { text-indent: 3 }").report.discarded_declarations,
        1
    );
}

/// A percentage margin stays a percentage all the way to the computed value,
/// because what it is a percentage *of* is the layout's business.
#[test]
fn a_percentage_margin_survives_computation() {
    let styles = super::cascade::styles("p { margin-left: 10% }", &super::tree(&[("p", None)]));
    assert_eq!(
        styles[0].margin.left,
        MarginValue::Length(LengthPercentage::Percent(10.0))
    );
}

// ---- `css-flexbox-1`, milestone 12 -----------------------------------------

/// Each of the ten flexbox properties parses to its own longhand, and a value
/// outside each one's set is `Unsupported` **by name** rather than mapped onto
/// its nearest neighbour.
#[test]
fn every_flexbox_longhand_reads_its_own_values() {
    assert_eq!(
        known("p { display: flex }"),
        vec![Property::Display(Display::Flex)]
    );
    assert_eq!(
        known("p { display: inline-flex }"),
        vec![Property::Display(Display::InlineFlex)]
    );
    assert_eq!(
        known("p { flex-direction: column-reverse }"),
        vec![Property::FlexDirection(FlexDirection::ColumnReverse)]
    );
    assert_eq!(
        known("p { flex-wrap: wrap-reverse }"),
        vec![Property::FlexWrap(FlexWrap::WrapReverse)]
    );
    assert_eq!(
        known("p { justify-content: space-evenly }"),
        vec![Property::JustifyContent(JustifyContent::SpaceEvenly)]
    );
    assert_eq!(
        known("p { align-items: baseline }"),
        vec![Property::AlignItems(AlignItems::Baseline)]
    );
    assert_eq!(
        known("p { align-self: auto }"),
        vec![Property::AlignSelf(AlignSelf::Auto)]
    );
    assert_eq!(
        known("p { align-content: space-between }"),
        vec![Property::AlignContent(AlignContent::SpaceBetween)]
    );
    assert_eq!(known("p { flex-grow: 2 }"), vec![Property::FlexGrow(2.0)]);
    assert_eq!(
        known("p { flex-shrink: 0 }"),
        vec![Property::FlexShrink(0.0)]
    );
    assert_eq!(
        known("p { flex-basis: 30% }"),
        vec![Property::FlexBasis(SpecifiedSize::Length(Len::Percent(
            30.0
        )))]
    );
    assert_eq!(known("p { order: -1 }"), vec![Property::Order(-1)]);

    // And the values outside each set, by name.
    for (source, property, value) in [
        (
            "p { flex-direction: inline-axis }",
            "flex-direction",
            "inline-axis",
        ),
        ("p { justify-content: start }", "justify-content", "start"),
        ("p { align-items: start }", "align-items", "start"),
        ("p { flex-basis: content }", "flex-basis", "content"),
    ] {
        assert_eq!(
            declarations(source)[0].declaration,
            Declaration::Unsupported {
                property,
                value: value.to_string()
            },
            "{source}"
        );
    }
}

/// `order` is a **signed** integer, which the `<integer>` reader used by
/// `orphans` and `widows` refuses.
///
/// A build that reused that reader parses `order: 2` and discards `order: -1`
/// and `order: 0` — and `order: -1` is exactly what a book writes to put a
/// figure first.
#[test]
fn order_takes_the_negative_integers_the_other_integer_reader_refuses() {
    assert_eq!(known("p { order: 0 }"), vec![Property::Order(0)]);
    assert_eq!(known("p { order: -3 }"), vec![Property::Order(-3)]);
    assert_eq!(sheet("p { order: 1.5 }").report.discarded_declarations, 1);
    assert_eq!(sheet("p { orphans: -1 }").report.discarded_declarations, 1);
}

/// A negative flex factor is the **author's** mistake and not this build's gap,
/// so it is discarded rather than counted in the census.
#[test]
fn a_negative_flex_factor_is_malformed_and_not_a_gap() {
    assert_eq!(
        sheet("p { flex-grow: -1 }").report.discarded_declarations,
        1
    );
    assert_eq!(
        sheet("p { flex-shrink: -2 }").report.discarded_declarations,
        1
    );
    assert_eq!(
        sheet("p { flex-basis: -5px }")
            .report
            .discarded_declarations,
        0
    );
}

/// **The `flex` shorthand's omitted `flex-basis` is `0%` and not `auto`**,
/// which is §7.2's own sentence and the one difference that decides what
/// `flex: 1` does.
///
/// With `auto` an item is sized to its content and then grown; with `0%` the
/// whole line is shared out in proportion to the factors. Every three-column
/// layout on the web depends on the second, and a build that expanded the
/// shorthand to its longhands' initial values gets the first.
#[test]
fn the_flex_shorthands_omitted_basis_is_zero_and_not_auto() {
    assert_eq!(
        known("p { flex: 1 }"),
        vec![
            Property::FlexGrow(1.0),
            Property::FlexShrink(1.0),
            Property::FlexBasis(SpecifiedSize::Length(Len::Percent(0.0))),
        ]
    );
    // And the longhand on its own leaves `flex-basis` alone entirely, which is
    // what makes the two different declarations.
    assert_eq!(known("p { flex-grow: 1 }"), vec![Property::FlexGrow(1.0)]);
}

/// §7.2's whole grammar: `none`, one number, two numbers, a basis, and the
/// `||` that lets the basis come first.
#[test]
fn the_flex_shorthand_reads_every_form_its_grammar_has() {
    assert_eq!(
        known("p { flex: none }"),
        vec![
            Property::FlexGrow(0.0),
            Property::FlexShrink(0.0),
            Property::FlexBasis(SpecifiedSize::Auto),
        ]
    );
    assert_eq!(
        known("p { flex: auto }"),
        vec![
            Property::FlexGrow(1.0),
            Property::FlexShrink(1.0),
            Property::FlexBasis(SpecifiedSize::Auto),
        ]
    );
    assert_eq!(
        known("p { flex: 2 3 }"),
        vec![
            Property::FlexGrow(2.0),
            Property::FlexShrink(3.0),
            Property::FlexBasis(SpecifiedSize::Length(Len::Percent(0.0))),
        ]
    );
    assert_eq!(
        known("p { flex: 1 30px }"),
        vec![
            Property::FlexGrow(1.0),
            Property::FlexShrink(1.0),
            Property::FlexBasis(SpecifiedSize::Length(Len::Px(30.0))),
        ]
    );
    assert_eq!(
        known("p { flex: 2 0 40px }"),
        vec![
            Property::FlexGrow(2.0),
            Property::FlexShrink(0.0),
            Property::FlexBasis(SpecifiedSize::Length(Len::Px(40.0))),
        ]
    );
    // The `||` in `[ <'flex-grow'> <'flex-shrink'>? || <'flex-basis'> ]` means
    // the basis may be written first, and a build that read the components
    // left to right reports a real declaration as malformed.
    assert_eq!(
        known("p { flex: 30px 1 }"),
        vec![
            Property::FlexGrow(1.0),
            Property::FlexShrink(1.0),
            Property::FlexBasis(SpecifiedSize::Length(Len::Px(30.0))),
        ]
    );
}

/// `flex-flow` resets **both** longhands, not only the one that was written.
///
/// §5.3's own note. Without it an earlier `flex-wrap: wrap` stands under a
/// later `flex-flow: column`, which is a container that wraps when its author
/// stopped asking for it.
#[test]
fn flex_flow_resets_the_longhand_that_was_left_out() {
    assert_eq!(
        known("p { flex-flow: column }"),
        vec![
            Property::FlexDirection(FlexDirection::Column),
            Property::FlexWrap(FlexWrap::NoWrap),
        ]
    );
    assert_eq!(
        known("p { flex-flow: wrap }"),
        vec![
            Property::FlexDirection(FlexDirection::Row),
            Property::FlexWrap(FlexWrap::Wrap),
        ]
    );
    assert_eq!(
        known("p { flex-flow: wrap-reverse row-reverse }"),
        vec![
            Property::FlexDirection(FlexDirection::RowReverse),
            Property::FlexWrap(FlexWrap::WrapReverse),
        ]
    );
}

/// None of the ten inherits, and `order` is the one worth asserting twice: an
/// inherited `order` would reorder a paragraph's `<em>` against its siblings.
#[test]
fn no_flexbox_property_inherits() {
    let tree = super::tree(&[("div", None), ("p", Some(0))]);
    let styles = super::cascade::styles(
        "div { display: flex; flex-direction: column; flex-wrap: wrap; order: 3; \
         flex-grow: 4; flex-shrink: 0; flex-basis: 20px; justify-content: center; \
         align-items: center; align-self: flex-end; align-content: center }",
        &tree,
    );
    assert_eq!(styles[0].flex_direction, FlexDirection::Column);
    assert_eq!(styles[0].order, 3);
    let child = &styles[1];
    assert_eq!(child.flex_direction, FlexDirection::Row);
    assert_eq!(child.flex_wrap, FlexWrap::NoWrap);
    assert_eq!(child.order, 0);
    assert_eq!(child.flex_grow, 0.0);
    assert_eq!(child.flex_shrink, 1.0);
    assert_eq!(child.justify_content, JustifyContent::FlexStart);
    assert_eq!(child.align_items, AlignItems::Stretch);
    assert_eq!(child.align_self, AlignSelf::Auto);
    assert_eq!(child.align_content, AlignContent::Stretch);
    assert_eq!(child.display, Display::Inline);
}

/// `flex-basis` computes to a **size**, and `auto` computes to `auto`.
///
/// The injection matrix asked for this: nothing asserted the computed value at
/// all, so a build that computed `auto` to zero — which is what the `flex`
/// shorthand's *omitted* basis is, one function away — passed everything. The
/// two are different declarations and this is where the difference is stored.
#[test]
fn flex_basis_computes_to_a_size_and_auto_stays_auto() {
    let tree = super::tree(&[("p", None)]);
    assert_eq!(
        super::cascade::styles("p { flex-basis: auto }", &tree)[0].flex_basis,
        Size::Auto
    );
    assert_eq!(
        super::cascade::styles("p { flex-basis: 30px }", &tree)[0].flex_basis,
        Size::Length(LengthPercentage::Px(30.0))
    );
    assert_eq!(
        super::cascade::styles("p { flex-basis: 25% }", &tree)[0].flex_basis,
        Size::Length(LengthPercentage::Percent(25.0)),
        "a percentage stays one: what it is a percentage of is the layout's"
    );
    // And an `em` is resolved against this element's own font size, which is
    // what makes `flex-basis` a `<'width'>` rather than a bare number.
    assert_eq!(
        super::cascade::styles("p { font-size: 20px; flex-basis: 2em }", &tree)[0].flex_basis,
        Size::Length(LengthPercentage::Px(40.0))
    );
    // §7.2.3: a negative basis is invalid, and the used value is clamped where
    // `padding` is clamped -- at the computed value rather than at the parser.
    assert_eq!(
        super::cascade::styles("p { flex-basis: -5px }", &tree)[0].flex_basis,
        Size::Length(LengthPercentage::Px(0.0))
    );
    // The default, which is what an item with no declaration on it is sized
    // from.
    assert_eq!(
        super::cascade::styles("p { color: red }", &tree)[0].flex_basis,
        Size::Auto
    );
}

/// CSS 2.2 §10.4 and §10.7's four, and the three answers a length can get.
///
/// A **negative** minimum or maximum is `Malformed` and the author's, because
/// both grammars are `<length-percentage [0,inf]>` and a negative number is not
/// a value of the property at all. `min-content` is `BadValue` and this
/// build's, because `css-sizing-3` §5.1 defines it and this build has not
/// implemented it. Putting the second in the author's column is the mistake
/// this split exists to prevent: it is the one figure the census is judged on,
/// and it would move in the flattering direction.
#[test]
fn min_and_max_sizing_tell_the_authors_mistake_from_this_builds_gap() {
    assert_eq!(
        known("p { min-width: 0 }"),
        vec![Property::MinWidth(SpecifiedMinSize::Length(Len::Px(0.0)))]
    );
    assert_eq!(
        known("p { min-width: auto }"),
        vec![Property::MinWidth(SpecifiedMinSize::Auto)]
    );
    assert_eq!(
        known("img { max-width: 100% }"),
        vec![Property::MaxWidth(SpecifiedMaxSize::Length(Len::Percent(
            100.0
        )))]
    );
    assert_eq!(
        known("p { max-height: none }"),
        vec![Property::MaxHeight(SpecifiedMaxSize::None)]
    );
    assert_eq!(
        known("p { min-height: 2em }"),
        vec![Property::MinHeight(SpecifiedMinSize::Length(Len::Em(2.0)))]
    );
    // The author's: discarded by §5.4.4, counted nowhere as a gap.
    for source in ["p { min-width: -1px }", "p { max-width: -3em }"] {
        let parsed = sheet(source);
        assert!(parsed.report.unsupported.is_empty(), "{source}");
        assert_eq!(parsed.report.discarded_declarations, 1, "{source}");
    }
    // This build's: named, with the value beside it.
    let parsed = sheet("p { min-width: min-content }");
    assert_eq!(parsed.report.unsupported, vec![("min-width", 1)]);
}

/// §10.8.1's ten values, and the two things a first implementation folds.
///
/// `sub` and `super` are keywords rather than lengths, `text-top` is not `top`,
/// and a **negative** length is valid where a negative `min-width` is not --
/// `vertical-align: -0.4em` is how a book sets a chemical subscript, so the
/// absence of a non-negative check here is the grammar rather than an omission.
#[test]
fn vertical_align_takes_ten_values_and_a_negative_length_is_one_of_them() {
    for (source, expected) in [
        ("baseline", SpecifiedVerticalAlign::Baseline),
        ("sub", SpecifiedVerticalAlign::Sub),
        ("super", SpecifiedVerticalAlign::Super),
        ("top", SpecifiedVerticalAlign::Top),
        ("middle", SpecifiedVerticalAlign::Middle),
        ("bottom", SpecifiedVerticalAlign::Bottom),
        ("text-top", SpecifiedVerticalAlign::TextTop),
        ("text-bottom", SpecifiedVerticalAlign::TextBottom),
    ] {
        assert_eq!(
            known(&format!("sup {{ vertical-align: {source} }}")),
            vec![Property::VerticalAlign(expected)],
            "{source}"
        );
    }
    assert_eq!(
        known("sub { vertical-align: -0.4em }"),
        vec![Property::VerticalAlign(SpecifiedVerticalAlign::Length(
            Len::Em(-0.4)
        ))]
    );
    assert_eq!(
        known("sup { vertical-align: 30% }"),
        vec![Property::VerticalAlign(SpecifiedVerticalAlign::Length(
            Len::Percent(30.0)
        ))]
    );
    // `css-inline-3`'s keyword, which this build does not have.
    let parsed = sheet("sup { vertical-align: first }");
    assert_eq!(parsed.report.unsupported, vec![("vertical-align", 1)]);
}

/// §9.3.1's five, §9.3.2's four insets and §9.9.1's `z-index`.
///
/// **All five `position` values are `Known`**, including the three no box in
/// this build is placed by. The alternative -- reporting `absolute` as a value
/// gap -- would have had to refuse the five longhands with it, and then the
/// report could say nothing about the box at all; cascading it lets
/// `tinker_pdf_layout` count it per **box** instead of per declaration.
#[test]
fn position_its_insets_and_z_index() {
    for (source, expected) in [
        ("static", Position::Static),
        ("relative", Position::Relative),
        ("absolute", Position::Absolute),
        ("fixed", Position::Fixed),
        ("sticky", Position::Sticky),
    ] {
        assert_eq!(
            known(&format!("figure {{ position: {source} }}")),
            vec![Property::Position(expected)],
            "{source}"
        );
    }
    assert_eq!(
        known("figure { top: 0; right: auto; bottom: -2px; left: 50% }"),
        vec![
            Property::Inset(Side::Top, SpecifiedInset::Length(Len::Px(0.0))),
            Property::Inset(Side::Right, SpecifiedInset::Auto),
            Property::Inset(Side::Bottom, SpecifiedInset::Length(Len::Px(-2.0))),
            Property::Inset(Side::Left, SpecifiedInset::Length(Len::Percent(50.0))),
        ]
    );
    assert_eq!(
        known("figure { z-index: auto }"),
        vec![Property::ZIndex(ZIndex::Auto)]
    );
    assert_eq!(
        known("figure { z-index: -1 }"),
        vec![Property::ZIndex(ZIndex::Layer(-1))]
    );
    // §9.9.1's grammar is `<integer>`; `2.5` is not one.
    assert!(known("figure { z-index: 2.5 }").is_empty());
}

/// `css-multicol-1`'s longhands and its two shorthands, and `css-align-3`'s
/// `gap`.
///
/// Three separate claims, and the third is the one to get wrong. §3.3's
/// `columns` **resets the omitted longhand**, so both come out of it whatever
/// the author wrote; §5.4's `column-rule` is `border`'s three-in-any-order; and
/// §8.2's `gap` is **row first**, which is the opposite of every
/// `<length> <length>?` in CSS 2.2 and would look entirely reasonable read the
/// other way round.
#[test]
fn the_multi_column_longhands_and_the_three_shorthands() {
    assert_eq!(
        known("div { column-count: 3 }"),
        vec![Property::ColumnCount(ColumnCount::Count(3))]
    );
    assert_eq!(
        known("div { column-width: 20em }"),
        vec![Property::ColumnWidth(SpecifiedColumnWidth::Length(
            Len::Em(20.0)
        ))]
    );
    // §3.1's grammar has no percentage in it: the author's, not this build's.
    assert!(known("div { column-width: 40% }").is_empty());
    assert_eq!(
        known("div { columns: 20em 3 }"),
        vec![
            Property::ColumnWidth(SpecifiedColumnWidth::Length(Len::Em(20.0))),
            Property::ColumnCount(ColumnCount::Count(3)),
        ]
    );
    // §3.3 resets the omitted one, so a bare count still says `auto` out loud.
    assert_eq!(
        known("div { columns: 2 }"),
        vec![
            Property::ColumnWidth(SpecifiedColumnWidth::Auto),
            Property::ColumnCount(ColumnCount::Count(2)),
        ]
    );
    assert_eq!(
        known("div { column-rule: 2px solid red }"),
        vec![
            Property::ColumnRuleWidth(Len::Px(2.0)),
            Property::ColumnRuleStyle(BorderStyle::Solid),
            Property::ColumnRuleColor(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }),
        ]
    );
    assert_eq!(
        known("div { column-span: all; column-fill: auto }"),
        vec![
            Property::ColumnSpan(ColumnSpan::All),
            Property::ColumnFill(ColumnFill::Auto),
        ]
    );
    // §8.2: `<'row-gap'> <'column-gap'>?`, and the order is the assertion.
    assert_eq!(
        known("div { gap: 1em 2em }"),
        vec![
            Property::RowGap(SpecifiedGap::Length(Len::Em(1.0))),
            Property::ColumnGap(SpecifiedGap::Length(Len::Em(2.0))),
        ]
    );
    assert_eq!(
        known("div { gap: 4px }"),
        vec![
            Property::RowGap(SpecifiedGap::Length(Len::Px(4.0))),
            Property::ColumnGap(SpecifiedGap::Length(Len::Px(4.0))),
        ]
    );
    assert_eq!(
        known("div { column-gap: normal }"),
        vec![Property::ColumnGap(SpecifiedGap::Normal)]
    );
    assert!(known("div { gap: -1px }").is_empty());
}

/// **`break-before`, `break-after` and `break-inside` are the `page-break-*`
/// longhands under their modern names** (`css-break-3` §3.4).
///
/// §3.4's mapping table, row by row, and the refusals by value beside it: a
/// `column` break or a `verso` page read as its nearest neighbour would be a
/// break this build put somewhere the author did not ask for.
#[test]
fn the_break_properties_are_the_page_break_longhands_under_their_modern_names() {
    use crate::property::{PageBreak, PageBreakInside};
    for (value, expected) in [
        ("auto", PageBreak::Auto),
        ("page", PageBreak::Always),
        ("avoid", PageBreak::Avoid),
        ("avoid-page", PageBreak::Avoid),
        ("left", PageBreak::Left),
        ("right", PageBreak::Right),
    ] {
        assert_eq!(
            known(&format!("p {{ break-before: {value} }}")),
            vec![Property::PageBreakBefore(expected)],
            "break-before: {value}"
        );
        assert_eq!(
            known(&format!("p {{ break-after: {value} }}")),
            vec![Property::PageBreakAfter(expected)],
            "break-after: {value}"
        );
    }
    for (value, expected) in [
        ("auto", PageBreakInside::Auto),
        ("avoid", PageBreakInside::Avoid),
        ("avoid-page", PageBreakInside::Avoid),
    ] {
        assert_eq!(
            known(&format!("p {{ break-inside: {value} }}")),
            vec![Property::PageBreakInside(expected)],
            "break-inside: {value}"
        );
    }
    // **The legacy name keeps its legacy grammar.** `page` is the modern
    // spelling of `always` and is not a `page-break-before` value at all.
    assert!(known("p { page-break-before: page }").is_empty());

    for (name, value) in [
        ("break-before", "column"),
        ("break-before", "avoid-column"),
        ("break-before", "region"),
        ("break-before", "avoid-region"),
        ("break-before", "recto"),
        ("break-after", "verso"),
        ("break-inside", "avoid-column"),
        ("break-inside", "avoid-region"),
    ] {
        assert_eq!(
            declarations(&format!("p {{ {name}: {value} }}"))[0].declaration,
            Declaration::Unsupported {
                property: name,
                value: value.to_owned(),
            },
            "{name}: {value} is this build's gap, by value"
        );
    }
}

/// **`text-transform` at its four casing values, and its two others refused by
/// value** (`css-text-3` §2.1).
///
/// `full-width` and `full-size-kana` are inside the grammar and this build's
/// gap, so they are `Unsupported` whether alone or beside a casing keyword;
/// two casing keywords, or `none` beside anything, are outside it and the
/// author's, so they are discarded.
#[test]
fn text_transform_reads_its_casing_values_and_refuses_the_rest_by_value() {
    use crate::property::TextTransform;
    for (value, expected) in [
        ("none", TextTransform::None),
        ("uppercase", TextTransform::Uppercase),
        ("LOWERCASE", TextTransform::Lowercase),
        ("capitalize", TextTransform::Capitalize),
    ] {
        assert_eq!(
            known(&format!("p {{ text-transform: {value} }}")),
            vec![Property::TextTransform(expected)],
            "text-transform: {value}"
        );
    }
    for value in [
        "full-width",
        "uppercase full-width",
        "full-size-kana capitalize",
    ] {
        assert_eq!(
            declarations(&format!("p {{ text-transform: {value} }}"))[0].declaration,
            Declaration::Unsupported {
                property: "text-transform",
                value: value.to_owned(),
            },
            "text-transform: {value}"
        );
    }
    for value in ["uppercase lowercase", "none uppercase", "bold", "3"] {
        let parsed = sheet(&format!("p {{ text-transform: {value} }}"));
        assert!(
            parsed.rules.iter().all(|rule| rule.declarations.is_empty()),
            "text-transform: {value} is not CSS and is discarded"
        );
    }
}

/// **`border-radius` expands clockwise from the top left, and `/` separates
/// the horizontal radii from the vertical ones** (`css-backgrounds-3` §5.2);
/// a negative radius is not CSS.
#[test]
fn the_border_radius_shorthand_expands_two_lists_clockwise() {
    use crate::property::{Corner, SpecifiedRadius};
    let radius = |h: f64, v: f64| SpecifiedRadius {
        horizontal: Len::Px(h),
        vertical: Len::Px(v),
    };
    assert_eq!(
        known("div { border-radius: 1px 2px 3px / 4px 5px }"),
        vec![
            Property::BorderRadius(Corner::TopLeft, radius(1.0, 4.0)),
            Property::BorderRadius(Corner::TopRight, radius(2.0, 5.0)),
            Property::BorderRadius(Corner::BottomRight, radius(3.0, 4.0)),
            Property::BorderRadius(Corner::BottomLeft, radius(2.0, 5.0)),
        ]
    );
    assert_eq!(
        known("div { border-top-right-radius: 10% 2em }"),
        vec![Property::BorderRadius(
            Corner::TopRight,
            SpecifiedRadius {
                horizontal: Len::Percent(10.0),
                vertical: Len::Em(2.0),
            }
        )]
    );
    assert!(known("div { border-radius: -1px }").is_empty());
    assert!(known("div { border-radius: 1px / }").is_empty());
}

/// **`background-image` is `none` or one `url()`, in either spelling**
/// (`css-backgrounds-3` §2.2, `css-values-4` §4.5); a gradient and a second
/// layer are CSS this build does not draw, refused by value.
#[test]
fn background_image_is_none_or_one_url() {
    use crate::property::ImageRef;
    let image = |href: &str| {
        Property::BackgroundImage(Some(ImageRef {
            href: href.to_owned(),
            base: None,
        }))
    };
    assert_eq!(
        known("div { background-image: url(paper.png) }"),
        vec![image("paper.png")]
    );
    assert_eq!(
        known(r#"div { background-image: url("img/paper.png") }"#),
        vec![image("img/paper.png")]
    );
    assert_eq!(
        known("div { background-image: none }"),
        vec![Property::BackgroundImage(None)]
    );
    for refused in [
        "linear-gradient(red, blue)",
        "url(a.png), url(b.png)",
        "image-set(url(a.png) 1x)",
    ] {
        assert!(
            matches!(
                declarations(&format!("div {{ background-image: {refused} }}"))[0].declaration,
                Declaration::Unsupported {
                    property: "background-image",
                    ..
                }
            ),
            "{refused}"
        );
    }
    assert!(known("div { background-image: paper.png }").is_empty());
}

/// **A relative `url()` remembers the sheet it was written in** — the sheet's
/// own address, and an `@import`ed sheet's its own — and a `<style>` sheet,
/// which has none, leaves it to the document.
#[test]
fn a_background_url_carries_the_address_of_its_sheet() {
    struct Table;
    impl ImportResolver for Table {
        fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
            (href == "inner.css").then(|| {
                (
                    "styles/inner.css".to_owned(),
                    b"p { background-image: url(dots.png) }".to_vec(),
                )
            })
        }
    }
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let parsed = crate::parse(
        b"@import url(inner.css); div { background: url(paper.png) }",
        Some("styles/book.css"),
        &Table,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("under every cap");
    let bases: Vec<(String, Option<String>)> = parsed
        .rules
        .iter()
        .flat_map(|rule| &rule.declarations)
        .filter_map(|declared| match &declared.declaration {
            Declaration::Known(Property::BackgroundImage(Some(image))) => {
                Some((image.href.clone(), image.base.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        bases,
        [
            ("dots.png".to_owned(), Some("styles/inner.css".to_owned())),
            ("paper.png".to_owned(), Some("styles/book.css".to_owned())),
        ]
    );
    let inline = known("div { background-image: url(paper.png) }");
    assert!(matches!(
        &inline[0],
        Property::BackgroundImage(Some(image)) if image.base.is_none()
    ));
}

/// **`background-repeat`'s two one-word forms and its per-axis pairs**
/// (§2.3): `repeat-x` is `repeat no-repeat`, and one keyword is both axes.
#[test]
fn background_repeat_is_one_keyword_per_axis() {
    use crate::property::{BackgroundRepeat, RepeatStyle as R};
    let repeat = |x, y| vec![Property::BackgroundRepeat(BackgroundRepeat { x, y })];
    assert_eq!(
        known("div { background-repeat: repeat-x }"),
        repeat(R::Repeat, R::NoRepeat)
    );
    assert_eq!(
        known("div { background-repeat: repeat-y }"),
        repeat(R::NoRepeat, R::Repeat)
    );
    assert_eq!(
        known("div { background-repeat: space }"),
        repeat(R::Space, R::Space)
    );
    assert_eq!(
        known("div { background-repeat: round no-repeat }"),
        repeat(R::Round, R::NoRepeat)
    );
    assert!(known("div { background-repeat: repeat-x repeat }").is_empty());
}

/// **`<bg-position>`'s one-, two-, three- and four-value forms** (§2.6):
/// one value centres the other axis, two keywords may come either way round,
/// and an offset after `right` or `bottom` is measured from that edge.
#[test]
fn background_position_reads_every_form() {
    use crate::property::{PositionOffset, SpecifiedBackgroundPosition};
    let at = |x: (bool, Len), y: (bool, Len)| {
        vec![Property::BackgroundPosition(SpecifiedBackgroundPosition {
            x: PositionOffset {
                from_end: x.0,
                offset: x.1,
            },
            y: PositionOffset {
                from_end: y.0,
                offset: y.1,
            },
        })]
    };
    let pct = |value: f64| (false, Len::Percent(value));
    assert_eq!(
        known("div { background-position: top }"),
        at(pct(50.0), pct(0.0))
    );
    assert_eq!(
        known("div { background-position: 10px }"),
        at((false, Len::Px(10.0)), pct(50.0))
    );
    assert_eq!(
        known("div { background-position: bottom left }"),
        at(pct(0.0), pct(100.0))
    );
    assert_eq!(
        known("div { background-position: 25% 2em }"),
        at(pct(25.0), (false, Len::Em(2.0)))
    );
    assert_eq!(
        known("div { background-position: right 10px bottom 20% }"),
        at((true, Len::Px(10.0)), (true, Len::Percent(20.0)))
    );
    assert_eq!(
        known("div { background-position: bottom 5px center }"),
        at(pct(50.0), (true, Len::Px(5.0)))
    );
    assert!(known("div { background-position: top 10px }").is_empty());
    assert!(known("div { background-position: left right }").is_empty());
    assert!(known("div { background-position: center 5px left }").is_empty());
}

/// **`background-size`'s keywords and its one or two lengths** (§2.4): one
/// length is the width, the height `auto`.
#[test]
fn background_size_is_cover_contain_or_two_lengths() {
    use crate::property::SpecifiedBackgroundSize as S;
    assert_eq!(
        known("div { background-size: cover }"),
        vec![Property::BackgroundSize(S::Cover)]
    );
    assert_eq!(
        known("div { background-size: 50% }"),
        vec![Property::BackgroundSize(S::Explicit(
            Some(Len::Percent(50.0)),
            None
        ))]
    );
    assert_eq!(
        known("div { background-size: auto 2em }"),
        vec![Property::BackgroundSize(S::Explicit(
            None,
            Some(Len::Em(2.0))
        ))]
    );
    assert!(known("div { background-size: -1px }").is_empty());
}

/// **The `background` shorthand sets all five longhands this build has**,
/// each one it does not name at its initial value (§2.11) — so a colour alone
/// takes away an image — and refuses an attachment or a box by value.
#[test]
fn the_background_shorthand_resets_what_it_does_not_name() {
    use crate::property::{
        BackgroundRepeat, ImageRef, PositionOffset, RepeatStyle as R, SpecifiedBackgroundPosition,
        SpecifiedBackgroundSize as S,
    };
    let start = |offset| PositionOffset {
        from_end: false,
        offset,
    };
    assert_eq!(
        known("div { background: #ff0000 }"),
        vec![
            Property::BackgroundColor(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            }),
            Property::BackgroundImage(None),
            Property::BackgroundRepeat(BackgroundRepeat::REPEAT),
            Property::BackgroundPosition(SpecifiedBackgroundPosition {
                x: start(Len::Percent(0.0)),
                y: start(Len::Percent(0.0)),
            }),
            Property::BackgroundSize(S::Explicit(None, None)),
        ]
    );
    assert_eq!(
        known("div { background: url(a.png) no-repeat center / contain transparent }"),
        vec![
            Property::BackgroundColor(Color::TRANSPARENT),
            Property::BackgroundImage(Some(ImageRef {
                href: "a.png".to_owned(),
                base: None,
            })),
            Property::BackgroundRepeat(BackgroundRepeat {
                x: R::NoRepeat,
                y: R::NoRepeat,
            }),
            Property::BackgroundPosition(SpecifiedBackgroundPosition {
                x: start(Len::Percent(50.0)),
                y: start(Len::Percent(50.0)),
            }),
            Property::BackgroundSize(S::Contain),
        ]
    );
    for refused in [
        "url(a.png) fixed",
        "url(a.png) padding-box",
        "url(a.png), url(b.png)",
    ] {
        assert!(
            matches!(
                declarations(&format!("div {{ background: {refused} }}"))[0].declaration,
                Declaration::Unsupported {
                    property: "background",
                    ..
                }
            ),
            "{refused}"
        );
    }
    assert!(known("div { background: url(a.png) / cover }").is_empty());
}

/// **`overflow` is `overflow-x` and then `overflow-y`**, one value standing
/// for both (`css-overflow-3` §3.1), and `overlay` is §3.1's legacy alias of
/// `auto`.
#[test]
fn the_overflow_shorthand_is_x_then_y() {
    use crate::property::Overflow;
    assert_eq!(
        known("div { overflow: hidden }"),
        vec![
            Property::OverflowX(Overflow::Hidden),
            Property::OverflowY(Overflow::Hidden),
        ]
    );
    assert_eq!(
        known("div { overflow: clip auto }"),
        vec![
            Property::OverflowX(Overflow::Clip),
            Property::OverflowY(Overflow::Auto),
        ]
    );
    assert_eq!(
        known("div { overflow-y: overlay }"),
        vec![Property::OverflowY(Overflow::Auto)]
    );
    assert!(known("div { overflow: hidden hidden hidden }").is_empty());
    assert!(known("div { overflow: 2px }").is_empty());
}

/// **`outline` is its three longhands**, the omitted ones at their initial
/// values; `hidden` is not an outline style and `invert` is refused by value.
#[test]
fn the_outline_shorthand_is_its_three_longhands() {
    use crate::property::OutlineStyle;
    assert_eq!(
        known("p { outline: thin dotted }"),
        vec![
            Property::OutlineWidth(Len::Px(1.0)),
            Property::OutlineStyle(OutlineStyle::Border(BorderStyle::Dotted)),
            Property::OutlineColor(None),
        ]
    );
    assert_eq!(
        known("p { outline-style: auto; outline-offset: -2px }"),
        vec![
            Property::OutlineStyle(OutlineStyle::Auto),
            Property::OutlineOffset(Len::Px(-2.0)),
        ]
    );
    assert!(known("p { outline-style: hidden }").is_empty());
    assert_eq!(
        declarations("p { outline-color: invert }")[0].declaration,
        Declaration::Unsupported {
            property: "outline-color",
            value: "invert".to_owned(),
        }
    );
}

/// **A shadow is two to four lengths in a row, a colour and `inset`, in any
/// order**, a list of them comma-separated (`css-backgrounds-3` §7.1); a text
/// shadow has no spread and no `inset` (`css-text-decor-3` §4). An omitted
/// colour and `currentColor` are the same value.
#[test]
fn a_shadow_is_its_lengths_a_colour_and_inset_in_any_order() {
    use crate::property::SpecifiedShadow;
    let red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    let hard = |x: f64, y: f64, spread: f64, color: Option<Color>, inset: bool| SpecifiedShadow {
        color,
        x: Len::Px(x),
        y: Len::Px(y),
        blur: Len::Px(0.0),
        spread: Len::Px(spread),
        inset,
    };
    assert_eq!(
        known("p { box-shadow: 2px 3px 0 4px red, inset red -1px 0 }"),
        vec![Property::BoxShadow(vec![
            hard(2.0, 3.0, 4.0, Some(red), false),
            hard(-1.0, 0.0, 0.0, Some(red), true),
        ])]
    );
    assert_eq!(
        known("p { box-shadow: currentColor 1px 1px inset }"),
        vec![Property::BoxShadow(vec![hard(1.0, 1.0, 0.0, None, true)])]
    );
    assert_eq!(
        known("p { text-shadow: 1px 2px; box-shadow: none }"),
        vec![
            Property::TextShadow(vec![hard(1.0, 2.0, 0.0, None, false)]),
            Property::BoxShadow(Vec::new()),
        ]
    );
    // Grammar: lengths broken by a colour, a second colour, `inset` twice, a
    // percentage, one length, a fourth length or `inset` on text, a trailing
    // comma, a negative blur — each is invalid, and dropped as invalid rather
    // than counted as a gap.
    for malformed in [
        "box-shadow: 1px red 2px",
        "box-shadow: 1px 2px red blue",
        "box-shadow: inset 1px 2px inset",
        "box-shadow: 10% 2px",
        "box-shadow: 1px",
        "box-shadow: 1px 1px 0 1px 1px",
        "text-shadow: 1px 1px 0 1px",
        "text-shadow: 1px 1px,",
        "box-shadow: 1px 1px -2px",
    ] {
        assert!(
            known(&format!("p {{ {malformed} }}")).is_empty(),
            "{malformed}"
        );
        assert!(
            !declarations(&format!("p {{ {malformed} }}"))
                .iter()
                .any(|declared| matches!(declared.declaration, Declaration::Unsupported { .. })),
            "{malformed} is malformed, not a gap"
        );
    }
    // **A blur is refused by value**, in any list position and in any unit, and
    // the whole declaration with it — named, not drawn hard.
    for blurred in [
        ("box-shadow", "1px 1px 2px"),
        ("text-shadow", "0 0 0 red, 1px 1px 0.5em"),
    ] {
        assert!(
            matches!(
                &declarations(&format!("p {{ {}: {} }}", blurred.0, blurred.1))[0].declaration,
                Declaration::Unsupported { property, .. } if *property == blurred.0
            ),
            "{blurred:?}"
        );
    }
}
