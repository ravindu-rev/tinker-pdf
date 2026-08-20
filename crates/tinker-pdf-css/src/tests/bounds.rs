//! Every cap fires, by its own refusal or its own warning, never by a clock.
//!
//! Gap 18a's milestone 8 found `MAX_JPX_WORK` set *above* the most its own
//! inputs could ask for, so it could never fire — and nothing but a test that
//! makes it fire would have said so. Every one of these builds the input at the
//! **shipped** default rather than lowering the constant, because a cap proved
//! only against a lowered copy of itself has not been proved to fire.

use super::{sheet, tree, Node};
use crate::cascade::{cascade, Origin};
use crate::limits::{
    MAX_CSS_BYTES, MAX_CSS_DECLARATIONS, MAX_CSS_IMPORT_DEPTH, MAX_CSS_RULES,
    MAX_CSS_SELECTOR_PARTS, MAX_CSS_TOKENS, MAX_DOM_NODES, MAX_SELECTOR_MATCHES,
};
use crate::media::MediaContext;
use crate::parser::parse;
use crate::{Budget, ImportResolver, Limits, NoImports, Refusal, Warning};

fn parse_at_defaults(source: &[u8]) -> Result<crate::Stylesheet, Refusal> {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    parse(
        source,
        None,
        &NoImports,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
}

/// `MAX_CSS_BYTES`, charged before a byte is tokenized.
#[test]
fn a_stylesheet_past_the_byte_cap_is_refused_by_name() {
    let source = vec![b' '; MAX_CSS_BYTES + 1];
    assert_eq!(
        parse_at_defaults(&source),
        Err(Refusal::StylesheetTooLong {
            bytes: MAX_CSS_BYTES + 1
        })
    );
    // And one byte under it is read, which is what says the cap is where the
    // constant says and not somewhere near it.
    let under = vec![b' '; MAX_CSS_BYTES];
    assert!(parse_at_defaults(&under).is_ok());
}

/// `MAX_CSS_TOKENS`, the book's total, reached from **one** sheet.
///
/// A comma is one byte and one token, so the input is 4 000 001 bytes — half
/// of `MAX_CSS_BYTES`, which is what makes the total reachable from a single
/// entry rather than only from a hostile manifest.
#[test]
fn the_token_total_refuses_by_name() {
    let source = vec![b','; MAX_CSS_TOKENS + 1];
    assert!(
        source.len() < MAX_CSS_BYTES,
        "the byte cap must not fire first"
    );
    match parse_at_defaults(&source) {
        Err(Refusal::TooManyTokens { tokens }) => {
            assert_eq!(tokens, MAX_CSS_TOKENS + 1);
        }
        other => panic!("expected the token total to fire, got {other:?}"),
    }
}

/// `MAX_CSS_TOKENS` is a **total**: two sheets that each fit still cross it.
///
/// This is the assertion that separates a work cap from a per-item one. A
/// per-sheet cap of the same number passes the test above and fails this one,
/// and an EPUB's manifest chooses how many sheets there are.
#[test]
fn the_token_total_is_spent_across_sheets_and_not_per_sheet() {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let half = vec![b','; MAX_CSS_TOKENS / 2 + 1];
    let context = MediaContext::screen(432.0, 648.0);
    assert!(parse(&half, None, &NoImports, &context, &limits, &mut budget).is_ok());
    assert!(
        matches!(
            parse(&half, None, &NoImports, &context, &limits, &mut budget),
            Err(Refusal::TooManyTokens { .. })
        ),
        "the second sheet started from zero, so the total is not a total"
    );
}

/// `MAX_CSS_RULES`, the book's total.
#[test]
fn the_rule_total_refuses_by_name() {
    let source = "a{}".repeat(MAX_CSS_RULES + 1);
    match parse_at_defaults(source.as_bytes()) {
        Err(Refusal::TooManyRules { rules }) => assert_eq!(rules, MAX_CSS_RULES + 1),
        other => panic!("expected the rule total to fire, got {other:?}"),
    }
    let under = "a{}".repeat(MAX_CSS_RULES);
    assert!(parse_at_defaults(under.as_bytes()).is_ok());
}

/// `MAX_CSS_DECLARATIONS`, the book's total.
#[test]
fn the_declaration_total_refuses_by_name() {
    let mut source = String::from("a{");
    source.push_str(&"float:left;".repeat(MAX_CSS_DECLARATIONS + 1));
    source.push('}');
    match parse_at_defaults(source.as_bytes()) {
        Err(Refusal::TooManyDeclarations { declarations }) => {
            assert_eq!(declarations, MAX_CSS_DECLARATIONS + 1);
        }
        other => panic!("expected the declaration total to fire, got {other:?}"),
    }
}

/// `MAX_CSS_SELECTOR_PARTS`, which drops the **rule** with a named warning
/// rather than refusing the sheet.
///
/// `css-syntax-3`'s recovery is normative and ruling 2 degrades rather than
/// fails, so the rest of the stylesheet is still read — which is asserted here
/// as well, because a cap that took the whole sheet with it would pass a test
/// that only looked for the warning.
#[test]
fn a_selector_past_the_compound_cap_is_dropped_with_its_own_warning() {
    let long = vec!["a"; MAX_CSS_SELECTOR_PARTS + 1].join(" ");
    let parsed =
        parse_at_defaults(format!("{long} {{ float: left }} p {{ float: right }}").as_bytes())
            .expect("the sheet is still read");
    assert_eq!(
        parsed.report.warnings,
        vec![(Warning::SelectorTooComplex, 1)]
    );
    assert_eq!(parsed.report.discarded_rules, 1);
    assert_eq!(parsed.rules.len(), 1, "the rule after it survives");
    // Exactly at the cap it parses, which is what puts the boundary where the
    // constant says it is.
    let at_cap = vec!["a"; MAX_CSS_SELECTOR_PARTS].join(" ");
    let ok = parse_at_defaults(format!("{at_cap} {{ float: left }}").as_bytes()).expect("read");
    assert!(ok.report.warnings.is_empty());
    assert_eq!(ok.rules.len(), 1);
}

/// A chain of sheets, each importing the next, for the depth cap.
struct Chain;

impl ImportResolver for Chain {
    fn resolve(&self, href: &str, _base: Option<&str>) -> Option<(String, Vec<u8>)> {
        let depth: usize = href.strip_prefix("s")?.strip_suffix(".css")?.parse().ok()?;
        let body = format!(
            "@import url(s{}.css); a{depth} {{ float: left }}",
            depth + 1
        );
        Some((href.to_string(), body.into_bytes()))
    }
}

/// `MAX_CSS_IMPORT_DEPTH`, by its own warning — and it is a **different** name
/// from the cycle's, because they are different facts about a book.
#[test]
fn an_import_chain_past_the_depth_cap_warns_by_its_own_name() {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let parsed = parse(
        b"@import url(s0.css);",
        Some("root.css"),
        &Chain,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("a depth cap warns rather than refusing");
    assert_eq!(parsed.report.warnings, vec![(Warning::ImportTooDeep, 1)]);
    // The chain was read to exactly the cap and no further: one rule per level.
    assert_eq!(parsed.rules.len(), MAX_CSS_IMPORT_DEPTH);
    assert!(!parsed
        .report
        .warnings
        .iter()
        .any(|(warning, _)| *warning == Warning::ImportCycle));
}

/// `MAX_DOM_NODES`, refused before a single selector is tested.
#[test]
fn a_tree_past_the_element_cap_is_refused_by_name() {
    let spec: Vec<(&str, Option<usize>)> = (0..=MAX_DOM_NODES).map(|_| ("p", None)).collect();
    let nodes = tree(&spec);
    let parsed = sheet("p { float: left }");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    assert_eq!(
        cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget),
        Err(Refusal::TooManyElements {
            elements: MAX_DOM_NODES + 1
        })
    );
    assert_eq!(budget.matches(), 0, "refused before any work was done");
}

/// `MAX_SELECTOR_MATCHES`, from a stylesheet that **defeats the index**.
///
/// Every rule names the same class and every element carries it, so the
/// bucketing that keeps an ordinary book cheap does nothing at all and the
/// cascade spends the full rules-times-elements product. That is the input a
/// hostile book would write, and it is why the cap exists despite the index.
///
/// The rules carry no declarations on purpose: what is being bounded is the
/// **matching**, and a fixture that also built four million cascade entries
/// would be measuring the wrong thing and taking a minute to do it.
#[test]
fn the_match_budget_refuses_a_stylesheet_that_defeats_the_index() {
    let rules = 2_001;
    let elements = 2_000;
    assert!(
        rules * elements > MAX_SELECTOR_MATCHES,
        "the fixture must be able to cross the cap"
    );
    let source = ".c{}".repeat(rules);
    let parsed = sheet(&source);
    assert_eq!(parsed.rules.len(), rules);

    let mut nodes: Vec<Node> = tree(&vec![("p", None); elements]);
    for node in &mut nodes {
        node.classes = vec!["c".to_string()];
    }
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    match cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget) {
        Err(Refusal::TooManySelectorMatches { matches }) => {
            assert_eq!(matches, MAX_SELECTOR_MATCHES + 1);
        }
        other => panic!("expected the match total to fire, got {other:?}"),
    }
}

/// The whole committed corpus's worth of CSS spends a small fraction of the
/// match budget, which is the second of the three numbers each cap carries.
///
/// Without it the cap is only known to be crossable; this is what says an
/// ordinary book is nowhere near it, and it is the figure the ledger's book
/// yardstick column is built from.
#[test]
fn an_ordinary_book_is_far_under_the_match_budget() {
    // A stylesheet the shape of a real one: forty rules over a handful of
    // buckets, against a content document of a thousand elements.
    let mut source = String::new();
    for index in 0..40 {
        source.push_str(&format!("p.c{index} span {{ float: left }}\n"));
    }
    let parsed = sheet(&source);
    let spec: Vec<(&str, Option<usize>)> = (0..1_000)
        .map(|index| {
            if index == 0 {
                ("p", None)
            } else {
                ("span", Some(0))
            }
        })
        .collect();
    let nodes = tree(&spec);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget).expect("under every cap");
    assert!(
        budget.matches() * 50 < MAX_SELECTOR_MATCHES,
        "a thousand-element document spent {} of {MAX_SELECTOR_MATCHES}",
        budget.matches()
    );
}
