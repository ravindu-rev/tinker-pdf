//! `css-syntax-3` §5's grammar, its error recovery, the at-rules, and decision
//! 5's three-way split.

use super::sheet;
use crate::media::{MediaContext, MediaType};
use crate::parser::{parse, Declared};
use crate::property::{
    AlignContent, AlignItems, AlignSelf, BorderStyle, Color, Declaration, Display, FlexDirection,
    FlexWrap, Float, JustifyContent, Len, LengthPercentage, MarginValue, Property, Side, Size,
    SpecifiedMargin, SpecifiedSize, IMPLEMENTED_NAMES, UNSUPPORTED_PROPERTIES,
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

/// `@layer` is refused **by name** rather than ignored.
///
/// `css-cascade-5` §6.1 sorts layers above specificity, so reading the block as
/// ordinary rules would invert the cascade for a book that uses one — and
/// dropping it silently would lose the rules with no number saying how many.
#[test]
fn layer_is_refused_by_name() {
    let parsed = sheet("@layer base { p { float: left } } p { float: right }");
    assert_eq!(parsed.report.warnings, vec![(Warning::LayerRefused, 1)]);
    assert_eq!(parsed.rules.len(), 1);
    assert_eq!(
        parsed.rules[0].declarations[0].declaration,
        Declaration::Known(Property::Float(Float::Right))
    );
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
        ("p { color: inherit }", "color", "inherit"),
        ("p { display: initial }", "display", "initial"),
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
            column-count: 2;
            -webkit-column-count: 2;
            -epub-text-emphasis-style: dot;
            -ah-margin-start: 1em;
            --brand: #333;
            colour: red;
            hyphens: auto;
         }",
    );
    assert_eq!(
        parsed.report.unsupported,
        vec![("column-count", 1), ("hyphens", 1)]
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
            "-webkit-column-count",
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

/// `css-cascade-5` §7.1's explicit defaulting keywords are **this build's gap**
/// on every property, including the ones whose values are lengths.
///
/// The other survivor, and it is milestone 3's shape exactly: the rule was
/// enforced twice and only one half was reachable. Disabling the CSS-wide
/// keyword branch entirely changed no answer for `color: inherit` or
/// `display: initial`, because a colour that is not a colour and a keyword that
/// is not one of a property's own keywords are **already** `Unsupported` by the
/// (property, value) rule. The half nobody reached is a *length*-valued
/// property, where an identifier that is not one of its keywords is `Invalid` —
/// so `margin-top: inherit` would have been filed as the author's typo rather
/// than as a gap in this engine, and the one number the milestone is judged on
/// would have been short by every `inherit` in every real book.
#[test]
fn a_css_wide_keyword_is_a_gap_on_a_length_valued_property_too() {
    for (source, property, value) in [
        ("p { margin-top: inherit }", "margin-top", "inherit"),
        ("p { width: initial }", "width", "initial"),
        ("p { padding: unset }", "padding", "unset"),
        ("p { text-indent: revert }", "text-indent", "revert"),
        (
            "p { border-top-width: inherit }",
            "border-top-width",
            "inherit",
        ),
        (
            "p { line-height: revert-layer }",
            "line-height",
            "revert-layer",
        ),
        ("p { letter-spacing: inherit }", "letter-spacing", "inherit"),
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
    // And the direction that says this is about the five keywords and not about
    // identifiers in general: an identifier that is not one of them is not CSS
    // for a length at all, and is the author's error rather than this build's.
    let typo = sheet("p { margin-top: red }");
    assert_eq!(typo.report.discarded_declarations, 1);
    assert!(typo.report.unsupported.is_empty());
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
        source.push_str(&format!(".c{index} {{ column-count: 2 }}\n"));
    }
    let parsed = sheet(&source);
    assert_eq!(parsed.report.unsupported, vec![("column-count", 400)]);
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
