//! `selectors-4`: §6.1–§6.4, §14's combinators, §15's specificity, matching.

use super::{sheet, tree, Node};
use crate::selector::{matches, Index, Specificity};
use crate::{Budget, Limits, Warning};

/// The specificity of the first selector of the first rule of a one-rule sheet.
fn specificity(selector: &str) -> Specificity {
    let parsed = sheet(&format!("{selector} {{ color: red }}"));
    assert_eq!(
        parsed.rules.len(),
        1,
        "`{selector}` did not survive parsing: {:?}",
        parsed.report
    );
    parsed.rules[0].selectors[0].specificity
}

fn spec(a: u32, b: u32, c: u32) -> Specificity {
    Specificity { a, b, c }
}

/// Whether the sheet's first selector matches element `at` of `nodes`.
fn hits(selector: &str, nodes: &[Node], at: usize) -> bool {
    let parsed = sheet(&format!("{selector} {{ color: red }}"));
    assert_eq!(parsed.rules.len(), 1, "`{selector}` did not parse");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    matches(&parsed.rules[0].selectors[0], nodes, at, &mut budget).expect("under every cap")
}

/// §15, against a table of twenty-seven selectors.
///
/// Nine of the rows are the ones a naive A/B/C gets wrong, and they are the
/// reason the table is a table rather than four assertions: `:not(.a)` and
/// `.a` are **equal**, `:is()` takes its most specific argument rather than its
/// sum or its first, `:where()` is always zero, a pseudo-element counts in C
/// like a type selector rather than in B like a pseudo-class, and the universal
/// selector counts nowhere at all.
///
/// Two rows — `#s12:not(foo)` and `.foo :is(.bar, #baz)` — are copied verbatim
/// from `selectors-4` §15's own worked table, so at least part of this is the
/// specification's arithmetic and not this author's.
#[test]
fn the_specificity_table() {
    let table: &[(&str, Specificity)] = &[
        // The ordinary rows, which any implementation gets right.
        ("*", spec(0, 0, 0)),
        ("li", spec(0, 0, 1)),
        ("ul li", spec(0, 0, 2)),
        ("ul ol + li", spec(0, 0, 3)),
        ("h1 + *[rel=up]", spec(0, 1, 1)),
        ("ul ol li.red", spec(0, 1, 3)),
        ("li.red.level", spec(0, 2, 1)),
        ("#x34y", spec(1, 0, 0)),
        ("a[href]", spec(0, 1, 1)),
        ("a[href=\"x\" i]", spec(0, 1, 1)),
        ("#a#b", spec(2, 0, 0)),
        ("p > *", spec(0, 0, 1)),
        ("p ~ span", spec(0, 0, 2)),
        (":root", spec(0, 1, 0)),
        ("p:first-child", spec(0, 1, 1)),
        // §15's own two worked examples.
        ("#s12:not(foo)", spec(1, 0, 1)),
        (".foo :is(.bar, #baz)", spec(1, 1, 0)),
        // `:not()` contributes its **argument**, so these two are equal — the
        // row a build that counted `:not` as a pseudo-class gets wrong, and it
        // only shows when the two meet in one cascade.
        (".a", spec(0, 1, 0)),
        (":not(.a)", spec(0, 1, 0)),
        (":not(#a)", spec(1, 0, 0)),
        // The **most specific** argument, not the first and not the sum.
        (":not(em, strong#foo)", spec(1, 0, 1)),
        (":is(#x, p)", spec(1, 0, 0)),
        (":is(p, #x)", spec(1, 0, 0)),
        // `:where()` is zero however specific its argument is.
        (":where(#x, p)", spec(0, 0, 0)),
        // A pseudo-element is a C, like a type selector.
        ("::before", spec(0, 0, 1)),
        ("p::before", spec(0, 0, 2)),
        ("p:before", spec(0, 0, 2)),
        // A pseudo-class is a B, including one this build never matches.
        ("p:hover", spec(0, 1, 1)),
        ("li:nth-child(2n)", spec(0, 1, 1)),
        // `:has()` takes its most specific argument even though nothing here
        // evaluates it.
        (":has(#x)", spec(1, 0, 0)),
        // Nesting: the inner `:not` decides the outer `:is`.
        (":is(:not(#a), .b)", spec(1, 0, 0)),
    ];
    assert!(
        table.len() >= 20,
        "the exit criterion asks for at least twenty selectors"
    );
    for (selector, wanted) in table {
        assert_eq!(
            specificity(selector),
            *wanted,
            "specificity of `{selector}`"
        );
    }
}

/// The tuple is compared lexicographically, so no amount of B beats one A.
///
/// A build that packed it into `a * 100 + b * 10 + c` passes every stylesheet
/// with fewer than ten classes on a selector and is wrong on the eleventh, and
/// nothing in a book announces that it has one.
#[test]
fn specificity_is_a_tuple_and_not_a_base_ten_number() {
    let eleven_classes = ".a.b.c.d.e.f.g.h.i.j.k";
    assert_eq!(specificity(eleven_classes), spec(0, 11, 0));
    assert!(specificity("#x") > specificity(eleven_classes));
    assert!(specificity(".a") > specificity("a b c d e f g h i j k"));
}

/// §14's four combinators, each matching what it should and **not** matching
/// what its neighbour would.
#[test]
fn the_four_combinators() {
    // section > p, section > span, span (a child of the first p)
    let nodes = tree(&[
        ("section", None),
        ("p", Some(0)),
        ("em", Some(1)),
        ("span", Some(0)),
    ]);
    assert!(hits("section p", &nodes, 1));
    assert!(
        hits("section em", &nodes, 2),
        "descendant is not just child"
    );
    assert!(hits("section > p", &nodes, 1));
    assert!(!hits("section > em", &nodes, 2), "child is not descendant");
    assert!(hits("p + span", &nodes, 3));
    assert!(!hits("em + span", &nodes, 3), "em is not a sibling of span");
    assert!(hits("p ~ span", &nodes, 3));
    // A subsequent-sibling is not a next-sibling: put another element between.
    let spaced = tree(&[
        ("section", None),
        ("p", Some(0)),
        ("hr", Some(0)),
        ("span", Some(0)),
    ]);
    assert!(hits("p ~ span", &spaced, 3));
    assert!(
        !hits("p + span", &spaced, 3),
        "`+` is the immediately preceding sibling and `hr` is in the way"
    );
}

/// A descendant combinator backtracks: the match may be any ancestor, not the
/// nearest one that could have started it.
#[test]
fn a_descendant_match_tries_every_ancestor() {
    // div > section > div > p, where `div p` must find the *outer* div once the
    // inner one has been tried and the chain `div div p` needs both.
    let nodes = tree(&[
        ("div", None),
        ("section", Some(0)),
        ("div", Some(1)),
        ("p", Some(2)),
    ]);
    assert!(hits("div p", &nodes, 3));
    assert!(hits("div div p", &nodes, 3));
    assert!(hits("div section p", &nodes, 3));
    assert!(!hits("section div section p", &nodes, 3));
}

/// §6.2: `:not()` matches when **none** of its arguments does.
///
/// A build that negated each argument on its own would read `:not(a, b)` as
/// "not a, or not b", which is everything — and every book would then be
/// styled by every `:not()` rule in it.
#[test]
fn not_is_a_conjunction_of_negations() {
    let nodes = tree(&[("p", None), ("em", Some(0))]);
    assert!(!hits(":not(p, em)", &nodes, 0));
    assert!(!hits(":not(p, em)", &nodes, 1));
    assert!(hits(":not(span, div)", &nodes, 0));
    assert!(hits(":not(em)", &nodes, 0));
    assert!(!hits(":not(p)", &nodes, 0));
}

/// `:is()` matches when **any** argument does, and `:where()` matches the same
/// set at zero specificity — so the two differ in the cascade and not here.
#[test]
fn is_and_where_match_the_same_set() {
    let nodes = tree(&[("p", None), ("em", Some(0))]);
    for functional in [":is(p, span)", ":where(p, span)"] {
        assert!(hits(functional, &nodes, 0), "{functional} on p");
        assert!(!hits(functional, &nodes, 1), "{functional} on em");
    }
    assert_ne!(specificity(":is(#x)"), specificity(":where(#x)"));
}

/// §6.3's seven matchers, including the three whose empty operand matches
/// **nothing** — the case where "starts with the empty string" would be true.
#[test]
fn the_attribute_matchers() {
    let mut nodes = tree(&[("a", None)]);
    nodes[0].attributes = vec![
        ("href".to_string(), "chapter-01.xhtml".to_string()),
        ("lang".to_string(), "en-GB".to_string()),
        ("rel".to_string(), "up next".to_string()),
        ("empty".to_string(), String::new()),
    ];
    assert!(hits("a[href]", &nodes, 0));
    assert!(!hits("a[title]", &nodes, 0));
    assert!(hits("a[href=\"chapter-01.xhtml\"]", &nodes, 0));
    assert!(!hits("a[href=\"chapter-01\"]", &nodes, 0));
    assert!(hits("a[rel~=\"next\"]", &nodes, 0));
    assert!(!hits("a[rel~=\"ne\"]", &nodes, 0));
    assert!(hits("a[lang|=\"en\"]", &nodes, 0));
    assert!(hits("a[lang|=\"en-GB\"]", &nodes, 0));
    assert!(!hits("a[lang|=\"e\"]", &nodes, 0));
    assert!(hits("a[href^=\"chapter\"]", &nodes, 0));
    assert!(hits("a[href$=\".xhtml\"]", &nodes, 0));
    assert!(hits("a[href*=\"-01.\"]", &nodes, 0));
    // §6.3.2 to §6.3.5: an empty operand matches nothing, on all four.
    assert!(!hits("a[href^=\"\"]", &nodes, 0));
    assert!(!hits("a[href$=\"\"]", &nodes, 0));
    assert!(!hits("a[href*=\"\"]", &nodes, 0));
    assert!(!hits("a[rel~=\"\"]", &nodes, 0));
    // An operand with whitespace can never be one of a whitespace-separated
    // list's members, however the value is spelled.
    assert!(!hits("a[rel~=\"up next\"]", &nodes, 0));
    // But `=` on the same pair is true, which is what says the two matchers are
    // not the same code.
    assert!(hits("a[rel=\"up next\"]", &nodes, 0));
    // §6.3.6's flags. The default is case-sensitive, which is XML's rule.
    assert!(!hits("a[lang=\"EN-GB\"]", &nodes, 0));
    assert!(hits("a[lang=\"EN-GB\" i]", &nodes, 0));
    assert!(!hits("a[lang=\"EN-GB\" s]", &nodes, 0));
    // An attribute that is present and empty exists.
    assert!(hits("a[empty]", &nodes, 0));
    assert!(hits("a[empty=\"\"]", &nodes, 0));
}

/// Type names and classes are compared case-sensitively, which is XML's rule
/// and therefore an XHTML content document's.
#[test]
fn names_are_compared_case_sensitively() {
    let mut nodes = tree(&[("p", None)]);
    nodes[0].classes = vec!["Lead".to_string()];
    nodes[0].id = Some("Top".to_string());
    assert!(hits("p", &nodes, 0));
    assert!(!hits("P", &nodes, 0));
    assert!(hits(".Lead", &nodes, 0));
    assert!(!hits(".lead", &nodes, 0));
    assert!(hits("#Top", &nodes, 0));
    assert!(!hits("#top", &nodes, 0));
}

/// The four structural pseudo-classes this build evaluates.
#[test]
fn the_structural_pseudo_classes() {
    let nodes = tree(&[
        ("body", None),
        ("p", Some(0)),
        ("p", Some(0)),
        ("p", Some(0)),
        ("span", Some(3)),
    ]);
    assert!(hits(":root", &nodes, 0));
    assert!(!hits(":root", &nodes, 1));
    assert!(hits("p:first-child", &nodes, 1));
    assert!(!hits("p:first-child", &nodes, 2));
    assert!(hits("p:last-child", &nodes, 3));
    assert!(!hits("p:last-child", &nodes, 2));
    assert!(hits("span:only-child", &nodes, 4));
    assert!(!hits("p:only-child", &nodes, 1));
}

/// A rule whose subject is a pseudo-element matches **nothing**, and warns by
/// name.
///
/// The plausible wrong answer is to apply it to the originating element, which
/// would colour a paragraph red for `p::before { color: red }` — a book that
/// renders beautifully and is wrong.
#[test]
fn a_pseudo_element_matches_nothing_and_is_named() {
    let nodes = tree(&[("p", None)]);
    assert!(!hits("p::before", &nodes, 0));
    assert!(hits("p", &nodes, 0));
    let parsed = sheet("p::before { color: red }");
    assert_eq!(
        parsed.report.warnings,
        vec![(Warning::PseudoElementUnsupported("::before"), 1)]
    );
    assert_eq!(
        parsed.rules.len(),
        1,
        "the rule parses; it just matches none"
    );
}

/// A pseudo-class `selectors-4` defines and this build never evaluates is
/// **inert**: it never matches, and it is named in a counted warning.
///
/// The alternative — treating it as invalid — is worse in a specific way, and
/// the second half of this test is that way: `a:hover, a` would take the whole
/// list down with it and a book would lose the rule for `a` as well.
#[test]
fn an_inert_pseudo_class_never_matches_and_is_named() {
    let nodes = tree(&[("a", None)]);
    assert!(!hits("a:hover", &nodes, 0));
    let parsed = sheet("a:hover { color: red }");
    assert_eq!(
        parsed.report.warnings,
        vec![(Warning::PseudoClassUnsupported(":hover"), 1)]
    );
    let both = sheet("a:hover, a { color: red }");
    assert_eq!(both.rules.len(), 1);
    assert_eq!(both.rules[0].selectors.len(), 2);
    assert!(
        matches(
            &both.rules[0].selectors[1],
            &nodes,
            0,
            &mut Budget::new(&Limits::DEFAULT)
        )
        .expect("under every cap"),
        "the second selector of the list still matches"
    );
}

/// A pseudo no specification this build cites defines invalidates its rule,
/// per §3.1 — and takes the **whole** list with it, which is the half a build
/// that kept the selectors that parsed would get wrong.
#[test]
fn an_unknown_pseudo_invalidates_the_whole_list() {
    let parsed = sheet("a:quantum, p { color: red }");
    assert!(parsed.rules.is_empty());
    assert_eq!(parsed.report.discarded_rules, 1);
    assert_eq!(
        parsed.report.warnings,
        vec![(Warning::PseudoUnknown(":quantum".to_string()), 1)]
    );
    // And the rule *after* it survives, which is what error recovery is for.
    let recovered = sheet("a:quantum { color: red } p { color: blue }");
    assert_eq!(recovered.rules.len(), 1);
    assert_eq!(recovered.report.discarded_rules, 1);
}

/// The malformed shapes a selector parser has to refuse, each on its own.
#[test]
fn malformed_selectors_are_refused_one_at_a_time() {
    for source in [
        "> p",         // a leading combinator
        "p >",         // a trailing one
        "p > > q",     // a doubled one
        "p q,",        // an empty member of a list
        ",p",          // the same at the front
        "p|q",         // a namespace separator, which this build does not read
        "#0f0",        // a hash that is not an identifier
        "p..a",        // a dot with no name
        "p::before q", // something after a pseudo-element
        "[href",       // an unclosed attribute selector, whose block runs to EOF
    ] {
        let parsed = sheet(&format!("{source} {{ color: red }}"));
        assert!(
            parsed.rules.is_empty(),
            "`{source}` should not have produced a rule"
        );
        assert_eq!(
            parsed.report.discarded_rules, 1,
            "`{source}` should have been counted once"
        );
    }
}

/// The index returns a **superset**: everything it leaves out cannot match.
///
/// This is the only thing standing between a bucketing bug and a book that is
/// styled slightly less than it should be — which reads as a plain stylesheet
/// rather than as a defect, and is exactly the class of failure gap 31 exists
/// for. The comparison is against brute force over every selector, which is
/// the implementation the index replaces.
#[test]
fn an_indexed_lookup_and_a_brute_force_one_agree() {
    let source = "
        p { color: red }
        .lead { color: red }
        #top { color: red }
        * { color: red }
        div p { color: red }
        div > .lead { color: red }
        p.lead#top { color: red }
        span, p { color: red }
        [data-x] { color: red }
        p:first-child { color: red }
        div .lead span { color: red }
    ";
    let parsed = sheet(source);
    let mut nodes = tree(&[
        ("div", None),
        ("p", Some(0)),
        ("span", Some(1)),
        ("section", Some(0)),
        ("p", Some(3)),
    ]);
    nodes[1].classes = vec!["lead".to_string()];
    nodes[1].id = Some("top".to_string());
    nodes[4].attributes = vec![("data-x".to_string(), "1".to_string())];

    let flat: Vec<&crate::selector::Selector> = parsed
        .rules
        .iter()
        .flat_map(|rule| rule.selectors.iter())
        .collect();
    let mut index = Index::default();
    for (handle, selector) in flat.iter().enumerate() {
        index.insert(selector, handle);
    }

    let limits = Limits::DEFAULT;
    for at in 0..nodes.len() {
        let mut budget = Budget::new(&limits);
        let mut brute: Vec<usize> = Vec::new();
        for (handle, selector) in flat.iter().enumerate() {
            if matches(selector, &nodes, at, &mut budget).expect("under every cap") {
                brute.push(handle);
            }
        }
        let mut indexed: Vec<usize> = Vec::new();
        for handle in index.candidates(&nodes[at]) {
            if matches(flat[handle], &nodes, at, &mut budget).expect("under every cap") {
                indexed.push(handle);
            }
        }
        indexed.sort_unstable();
        assert_eq!(indexed, brute, "element {at}");
        assert!(
            !brute.is_empty(),
            "element {at} matched nothing, so the comparison proves nothing"
        );
    }
}
