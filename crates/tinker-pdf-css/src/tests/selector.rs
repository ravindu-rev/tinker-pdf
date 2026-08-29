//! `selectors-4`: §6.1–§6.4, §14's combinators, §15's specificity, matching.
//!
//! # The counted injection, and the hole it found
//!
//! One defect, in one place in the `An+B` parser: `n_tail` returning
//! `NTail::Complete(value)` where it returns `NTail::Complete(-value)` — the
//! sign of the offset a `<dimension-token>` already carries inside its unit,
//! which is how `3n-2` reaches this crate as *one* token whose unit is `n-2`.
//! It is the most plausible single mistake in the whole microsyntax, because
//! nothing in the source looks like a negative number.
//!
//! **Five assertions fire, out of 88, across three tests.**
//!
//! It fired **one**, in one test, when it was first injected, and the fix is
//! in the suite rather than in the count. `:nth-child()`'s table had the only
//! `n-<digits>` production in the file: `nth_last_child_counts_from_the_other_
//! end` and `the_of_type_family_counts_only_its_own_type` both wrote their
//! offsets with an explicit `+`, so neither could see the defect at all. The
//! rows that closed it had to be chosen rather than added — `2n-1` and `2n+1`
//! select the *same* positions, so a row with a negative offset only fires if
//! the sign changes the answer, which is why the new rows are `3n-2` and `n-3`
//! and not the obvious ones.

use super::{sheet, tree};
use crate::selector::{matches, Index, Specificity, UiState};
use crate::{Budget, Element, Limits, Warning};

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
fn hits<E: Element>(selector: &str, nodes: &[E], at: usize) -> bool {
    let parsed = sheet(&format!("{selector} {{ color: red }}"));
    assert_eq!(parsed.rules.len(), 1, "`{selector}` did not parse");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    matches(&parsed.rules[0].selectors[0], nodes, at, &mut budget).expect("under every cap")
}

/// The element a pseudo-class fixture is matched against.
///
/// A second node type beside the shared `Node` this module's other tests use,
/// and not duplication for its own sake: several of the questions
/// `selectors-4` asks about an element are ones the cascade's node has never
/// had to answer. `:empty` is about **text** children, which `Node` does not
/// carry at all; `:lang()`, `:dir()`, `:link` and §12's
/// seven are the document language's, which is the whole reason they are
/// provided methods with defaults. A fixture that could not distinguish "the
/// caller said nothing" from "the caller said no" would not be testing the
/// distinction this lane is about.
#[derive(Clone, Debug, Default)]
struct Doc {
    name: String,
    id: Option<String>,
    classes: Vec<String>,
    parent: Option<usize>,
    previous: Option<usize>,
    next: Option<usize>,
    empty: bool,
    language: Option<String>,
    direction: Option<String>,
    link: bool,
    ui: UiState,
}

impl Element for Doc {
    fn local_name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn classes(&self) -> &[String] {
        &self.classes
    }
    fn attribute(&self, _: &str) -> Option<&str> {
        None
    }
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn previous_sibling(&self) -> Option<usize> {
        self.previous
    }
    fn next_sibling(&self) -> Option<usize> {
        self.next
    }
    fn is_empty(&self) -> bool {
        self.empty
    }
    fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
    fn direction(&self) -> Option<&str> {
        self.direction.as_deref()
    }
    fn is_link(&self) -> bool {
        self.link
    }
    fn ui_state(&self) -> UiState {
        self.ui
    }
}

/// The same `(name, parent)` spec [`tree`] takes, with the sibling links
/// computed rather than written down for the same reason.
fn doc(spec: &[(&str, Option<usize>)]) -> Vec<Doc> {
    let mut nodes: Vec<Doc> = spec
        .iter()
        .map(|(name, parent)| Doc {
            name: (*name).to_string(),
            parent: *parent,
            ..Doc::default()
        })
        .collect();
    for index in 0..nodes.len() {
        let parent = nodes[index].parent;
        let previous = (0..index)
            .rev()
            .find(|earlier| nodes[*earlier].parent == parent);
        nodes[index].previous = previous;
        if let Some(previous) = previous {
            nodes[previous].next = Some(index);
        }
    }
    nodes
}

/// A parent with `count` children of one name, which is the shape every
/// positional test wants. Element 0 is the parent; the children are 1..=count.
fn row(name: &str, count: usize) -> Vec<Doc> {
    let mut spec: Vec<(&str, Option<usize>)> = vec![("ol", None)];
    spec.extend(std::iter::repeat_n((name, Some(0)), count));
    doc(&spec)
}

/// Which of a row's children the selector matches, as one-based positions.
fn positions<E: Element>(selector: &str, nodes: &[E]) -> Vec<usize> {
    (1..nodes.len())
        .filter(|at| hits(selector, nodes, *at))
        .collect()
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
        // A pseudo-class is a B, including one that names a state this
        // document does not have.
        ("p:hover", spec(0, 1, 1)),
        ("li:nth-child(2n)", spec(0, 1, 1)),
        ("p:lang(en)", spec(0, 1, 1)),
        ("p:dir(rtl)", spec(0, 1, 1)),
        // `:has()` takes its most specific argument, and a leading combinator
        // changes what the argument *means* without changing what it is worth.
        (":has(#x)", spec(1, 0, 0)),
        (":has(> #x)", spec(1, 0, 0)),
        ("p:has(.a, #b)", spec(1, 0, 1)),
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

/// A pseudo-class naming a state this document does not have never matches,
/// and is named in a counted warning.
///
/// The alternative — treating it as invalid — is worse in a specific way, and
/// the second half of this test is that way: `a:hover, a` would take the whole
/// list down with it and a book would lose the rule for `a` as well.
#[test]
fn a_pseudo_class_with_no_such_state_never_matches_and_is_named() {
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
        p:nth-child(2n+1) { color: red }
        div:has(> p) { color: red }
        p:lang(en) { color: red }
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

/// **A repeated class does not return its rules twice** (gap 31, milestone 13).
///
/// `cargo fuzz run css` found this in 428 executions of its first real session,
/// as `the index and brute force disagree`, and minimised it to nine bytes:
/// a `.note` rule and an element carrying `note` twice.
///
/// `class="note note"` is valid HTML — the DOM's `classList` is a *set*, and a
/// producer that writes one by accident is writing an ordinary book. The visible
/// consequence is nothing: applying the same declaration twice lands on the same
/// computed value. The consequence that is not nothing is the **budget**, and it
/// is asserted here beside the list rather than left implied, because the two
/// are independent: `MAX_DOM_NODES` counts elements and nothing counts class
/// tokens, so one attribute of a thousand repeats used to multiply the whole
/// cascade's cost by a thousand.
#[test]
fn an_index_does_not_return_a_rule_twice_for_a_repeated_class() {
    let parsed = sheet(".note { color: red } p { color: blue } * { color: green }");
    let flat: Vec<&crate::selector::Selector> = parsed
        .rules
        .iter()
        .flat_map(|rule| rule.selectors.iter())
        .collect();
    let mut index = Index::default();
    for (handle, selector) in flat.iter().enumerate() {
        index.insert(selector, handle);
    }

    let mut nodes = tree(&[("p", None)]);
    nodes[0].classes = vec!["note".to_string()];
    let once = index.candidates(&nodes[0]);
    // The control: three rules, three candidates, and the element really does
    // match all three -- so a comparison against the repeated case is a
    // comparison of something.
    assert_eq!(once.len(), 3, "{once:?}");

    nodes[0].classes = vec!["note".to_string(), "note".to_string()];
    let twice = index.candidates(&nodes[0]);
    assert_eq!(
        twice, once,
        "a class written twice returned its bucket twice: {twice:?}"
    );

    // And the budget, which is the half a rendered page cannot show. Ten
    // repetitions of one class cost what one costs.
    nodes[0].classes = vec!["note".to_string(); 10];
    let ten = index.candidates(&nodes[0]);
    assert_eq!(ten, once, "ten repeats cost more than one: {ten:?}");

    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    for handle in index.candidates(&nodes[0]) {
        matches(flat[handle], &nodes, 0, &mut budget).expect("under every cap");
    }
    let repeated = budget.matches();
    nodes[0].classes = vec!["note".to_string()];
    let mut budget = Budget::new(&limits);
    for handle in index.candidates(&nodes[0]) {
        matches(flat[handle], &nodes, 0, &mut budget).expect("under every cap");
    }
    assert_eq!(
        repeated,
        budget.matches(),
        "a repeated class charged the match budget more than once"
    );
}

/// §6.6.2's `An+B`, over a row of ten, against the positions computed by hand.
///
/// The table is a table because the microsyntax's difficulty is entirely in
/// the tokenizer's hands and none of it is visible in the source: `2n` is one
/// dimension token, `2n-1` is *also* one dimension token whose unit is `n-1`,
/// `2n - 1` is three tokens and `-n+3` starts with an identifier. Every row
/// below is a different path through the parser and they are all the same
/// grammar.
#[test]
fn nth_child_reads_the_whole_an_plus_b_grammar() {
    let nodes = row("li", 10);
    let table: &[(&str, &[usize])] = &[
        // The two keywords, which §6.6.2 defines *as* `2n+1` and `2n`.
        ("odd", &[1, 3, 5, 7, 9]),
        ("even", &[2, 4, 6, 8, 10]),
        ("2n+1", &[1, 3, 5, 7, 9]),
        ("2n", &[2, 4, 6, 8, 10]),
        // `A` of one, spelled four ways, all of them everything.
        ("n", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        ("1n", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        ("+n", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        ("n+0", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        // `A` of zero: not a step at all, one position.
        ("3", &[3]),
        ("0n+3", &[3]),
        ("+3", &[3]),
        ("0n", &[]),
        // The four spellings of the same negative `B`, which is where the
        // tokenizer's ident rules bite: `n-2` and `2n-2` are single tokens.
        ("3n-2", &[1, 4, 7, 10]),
        ("3n - 2", &[1, 4, 7, 10]),
        ("3n -2", &[1, 4, 7, 10]),
        ("3n- 2", &[1, 4, 7, 10]),
        // A negative `A` counts down, and it is the row a plain remainder test
        // gets wrong: without §6.6.2's non-negative `n` this is every element.
        ("-n+3", &[1, 2, 3]),
        ("-n+0", &[]),
        ("-2n+9", &[1, 3, 5, 7, 9]),
        // `B` past the row still parses and matches nothing, which is not the
        // same fact as a selector that failed to parse.
        ("n+11", &[]),
        ("11", &[]),
        // Keywords are ASCII-case-insensitive, and the `N` is a keyword.
        ("2N+1", &[1, 3, 5, 7, 9]),
        ("ODD", &[1, 3, 5, 7, 9]),
        // Whitespace inside the parentheses is not part of the grammar.
        (" odd ", &[1, 3, 5, 7, 9]),
        ("2n + 1", &[1, 3, 5, 7, 9]),
    ];
    for (argument, wanted) in table {
        let got = positions(&format!("li:nth-child({argument})"), &nodes);
        assert_eq!(got, wanted.to_vec(), ":nth-child({argument})");
    }
    // The negative twin the table cannot state: no argument matches *nothing
    // and everything*, so a matcher that answered `true` for every element
    // fails the rows that expect a subset and one that answered `false` fails
    // every other row.
    assert_ne!(
        positions("li:nth-child(1)", &nodes),
        positions("li:nth-child(2)", &nodes)
    );
}

/// `:nth-last-child()` is the same arithmetic from the other end, and the
/// asymmetry is the test: `3n+1` from the front and from the back pick
/// different elements out of a row of ten.
#[test]
fn nth_last_child_counts_from_the_other_end() {
    let nodes = row("li", 10);
    assert_eq!(positions("li:nth-last-child(1)", &nodes), vec![10]);
    assert_eq!(positions("li:nth-last-child(2)", &nodes), vec![9]);
    assert_eq!(
        positions("li:nth-last-child(odd)", &nodes),
        vec![2, 4, 6, 8, 10]
    );
    assert_eq!(
        positions("li:nth-last-child(3n+1)", &nodes),
        vec![1, 4, 7, 10]
    );
    assert_ne!(
        positions("li:nth-last-child(3n+2)", &nodes),
        positions("li:nth-child(3n+2)", &nodes),
        "counting from the end is not counting from the start"
    );
    assert_eq!(positions("li:nth-last-child(-n+3)", &nodes), vec![8, 9, 10]);
    // A negative `B` from this end too. The counted injection is why it is
    // here: a sign defect in the `n-3` production fired in one test out of
    // three, and the two it missed were the two that never wrote one.
    assert_eq!(
        positions("li:nth-last-child(3n-2)", &nodes),
        vec![1, 4, 7, 10]
    );
    assert_eq!(
        positions("li:nth-last-child(n-3)", &nodes),
        (1..=10).collect::<Vec<_>>(),
        "every position is at or past -3, so a negative offset admits the row"
    );
}

/// The `of-type` family counts **only siblings of the same type**, which is
/// the whole of what makes it a second family rather than an alias.
///
/// The fixture interleaves two names so that every positive has a negative
/// twin at the same index: `dt` number two is `:nth-of-type(2)` and is *not*
/// `:nth-child(2)`, and a build that ignored the type would match both.
#[test]
fn the_of_type_family_counts_only_its_own_type() {
    // dl > dt dd dt dd dt dd
    let nodes = doc(&[
        ("dl", None),
        ("dt", Some(0)),
        ("dd", Some(0)),
        ("dt", Some(0)),
        ("dd", Some(0)),
        ("dt", Some(0)),
        ("dd", Some(0)),
    ]);
    assert_eq!(positions("dt:nth-of-type(2)", &nodes), vec![3]);
    assert_eq!(positions("dd:nth-of-type(2)", &nodes), vec![4]);
    // A negative `B` here as well, and for the counted injection's reason:
    // `2n-1` and `2n+1` pick the same positions, so the row has to be one
    // where the sign changes the answer.
    assert_eq!(positions("dt:nth-of-type(3n-2)", &nodes), vec![1]);
    assert_eq!(positions("dd:nth-last-of-type(3n-2)", &nodes), vec![6]);
    assert_eq!(positions("dt:nth-child(2)", &nodes), Vec::<usize>::new());
    assert_eq!(positions("dt:nth-of-type(odd)", &nodes), vec![1, 5]);
    assert_eq!(
        positions("dt:nth-last-of-type(1)", &nodes),
        vec![5],
        "the last dt is not the last child"
    );
    assert_eq!(positions("dt:first-of-type", &nodes), vec![1]);
    assert_eq!(positions("dd:first-of-type", &nodes), vec![2]);
    assert_eq!(positions("dt:last-of-type", &nodes), vec![5]);
    assert_eq!(positions("dd:last-of-type", &nodes), vec![6]);
    // The negative twins: `dd` is never first *child* and `dt` is never last.
    assert!(!hits("dd:first-child", &nodes, 2));
    assert!(!hits("dt:last-child", &nodes, 5));
    // `:only-of-type` needs both halves, and this row gives neither.
    assert_eq!(positions("dt:only-of-type", &nodes), Vec::<usize>::new());
    let lonely = doc(&[("dl", None), ("dt", Some(0)), ("dd", Some(0))]);
    assert!(hits("dt:only-of-type", &lonely, 1));
    assert!(hits("dd:only-of-type", &lonely, 2));
    assert!(
        !hits("dt:only-child", &lonely, 1),
        "one of its type is not one of any type"
    );
}

/// An element with no siblings at all is at position **one**, from both ends.
///
/// The off-by-one this pins is the one that turns every `odd` into an `even`,
/// and it hides: a document whose rows all match is indistinguishable from one
/// whose rows all do not until you look at which.
#[test]
fn a_lone_element_is_at_position_one_from_either_end() {
    let nodes = doc(&[("body", None), ("p", Some(0))]);
    assert!(hits("p:nth-child(1)", &nodes, 1));
    assert!(hits("p:nth-last-child(1)", &nodes, 1));
    assert!(hits("p:nth-of-type(1)", &nodes, 1));
    assert!(!hits("p:nth-child(0)", &nodes, 1));
    assert!(!hits("p:nth-child(2)", &nodes, 1));
    // And the root, which has no parent either.
    assert!(hits("body:nth-child(1)", &nodes, 0));
    assert!(!hits("body:nth-child(2)", &nodes, 0));
}

/// An `An+B` the grammar rejects takes its rule down, per §3.1.
///
/// Every row is a shape a plausible parser accepts by accident: a trailing
/// sign with nothing after it, a fraction, an identifier that is not `n`, two
/// offsets, and `selectors-4`'s `of S` clause — which is **valid** selectors-4
/// this build does not read, and is refused rather than silently read as the
/// `An+B` without it. Reading `:nth-child(2n of .a)` as `:nth-child(2n)` would
/// style every second row instead of every second `.a`.
#[test]
fn an_an_plus_b_the_grammar_rejects_takes_its_rule_down() {
    for argument in [
        "", " ", "2n+", "+", "-", "1.5n", "n+1.5", "foo", "2x+1", "n+1+2", "2n+-1", "odd even",
        "2n of .a", "even 1", "--n", "n-", "2n-",
    ] {
        let parsed = sheet(&format!("li:nth-child({argument}) {{ color: red }}"));
        assert!(
            parsed.rules.is_empty(),
            "`:nth-child({argument})` should not have produced a rule"
        );
        assert_eq!(parsed.report.discarded_rules, 1, ":nth-child({argument})");
    }
}

/// §6.6.3's `:empty` is the document language's answer, and the trait's
/// default is *not* "empty".
///
/// The negative twin is the important one: a build whose `:empty` was "no
/// element children" would call `<p>text</p>` empty, and a book's every
/// paragraph would take the rule.
#[test]
fn empty_is_the_document_languages_answer() {
    let mut nodes = doc(&[("body", None), ("td", Some(0)), ("td", Some(0))]);
    nodes[1].empty = true;
    assert!(hits("td:empty", &nodes, 1));
    assert!(
        !hits("td:empty", &nodes, 2),
        "the caller said it has content"
    );
    // The default. A caller with no notion of children says nothing, and
    // saying nothing is not saying yes.
    let silent = doc(&[("td", None)]);
    assert!(!hits("td:empty", &silent, 0));
}

/// §6.5.1's `:lang()`, which is RFC 4647 §3.3.2's extended filtering over the
/// language the **nearest ancestor** declared.
///
/// Two things are being tested at once and they are separable on purpose: the
/// walk is this crate's (it has the tree) and the attribute is the caller's
/// (it has the document language), so the fixture declares a language on a
/// grandparent and asks about a grandchild.
#[test]
fn lang_is_extended_filtering_over_an_inherited_language() {
    let table: &[(&str, &str, bool)] = &[
        ("en", "en", true),
        ("en", "en-GB", true),
        ("en", "en-GB-oed", true),
        ("en", "eng", false),
        ("en", "fr", false),
        ("en-GB", "en-GB", true),
        ("en-GB", "en", false),
        ("en-GB", "en-GB-oed", true),
        // Case is not significant in a language tag or a range.
        ("EN-gb", "en-GB", true),
        // §3.3.2's wildcards, which is the half that is not a prefix test.
        ("*", "en", true),
        ("*-CH", "de-CH", true),
        ("*-CH", "de-DE", false),
        ("de-*-DE", "de-Latn-DE", true),
        ("de-*-DE", "de-DE", true),
        ("de-*-DE", "de-Latn-FR", false),
        // A subtag the range skips past may not be a singleton, which is what
        // stops a range from reaching across an extension boundary.
        ("de-DE", "de-x-DE", false),
    ];
    for (range, tag, wanted) in table {
        let mut nodes = doc(&[("html", None), ("p", Some(0))]);
        nodes[0].language = Some((*tag).to_string());
        assert_eq!(
            hits(&format!("p:lang({range})"), &nodes, 1),
            *wanted,
            ":lang({range}) against lang={tag}"
        );
    }
    // A list of ranges matches if any of them does.
    let mut nodes = doc(&[("html", None), ("p", Some(0))]);
    nodes[0].language = Some("fr-CA".to_string());
    assert!(hits("p:lang(en, fr)", &nodes, 1));
    assert!(!hits("p:lang(en, de)", &nodes, 1));
    assert!(hits("p:lang(\"fr\")", &nodes, 1));
    // The nearer declaration wins over the further one.
    let mut nested = doc(&[("html", None), ("div", Some(0)), ("p", Some(1))]);
    nested[0].language = Some("en".to_string());
    nested[1].language = Some("fr".to_string());
    assert!(hits("p:lang(fr)", &nested, 2));
    assert!(!hits("p:lang(en)", &nested, 2));
    assert!(hits("div:lang(fr)", &nested, 1));
    assert!(hits("html:lang(en)", &nested, 0));
    // An explicitly *unknown* language stops the inheritance and matches
    // nothing, which is a different answer from having said nothing at all.
    let mut stopped = doc(&[("html", None), ("p", Some(0))]);
    stopped[0].language = Some("en".to_string());
    stopped[1].language = Some(String::new());
    assert!(!hits("p:lang(en)", &stopped, 1));
    assert!(!hits("p:lang(*)", &stopped, 1));
    // And a document that never said: no range matches, including `*`.
    let silent = doc(&[("p", None)]);
    assert!(!hits("p:lang(en)", &silent, 0));
    assert!(!hits("p:lang(*)", &silent, 0));
}

/// §6.6's `:dir()` inherits the same way, and a directionality the document
/// language leaves to the content matches neither keyword.
#[test]
fn dir_inherits_and_an_unresolved_direction_matches_neither() {
    let mut nodes = doc(&[("html", None), ("p", Some(0)), ("span", Some(1))]);
    nodes[0].direction = Some("rtl".to_string());
    assert!(hits("span:dir(rtl)", &nodes, 2));
    assert!(!hits("span:dir(ltr)", &nodes, 2));
    nodes[1].direction = Some("ltr".to_string());
    assert!(
        hits("span:dir(ltr)", &nodes, 2),
        "the nearer declaration wins"
    );
    assert!(!hits("span:dir(rtl)", &nodes, 2));
    assert!(hits("html:dir(RTL)", &nodes, 0), "a keyword is case-folded");
    // `auto` is reported verbatim rather than guessed at, so it matches
    // neither — and, being a declaration, it stops the inheritance too.
    let mut auto = doc(&[("html", None), ("p", Some(0))]);
    auto[0].direction = Some("rtl".to_string());
    auto[1].direction = Some("auto".to_string());
    assert!(!hits("p:dir(rtl)", &auto, 1));
    assert!(!hits("p:dir(ltr)", &auto, 1));
    let silent = doc(&[("p", None)]);
    assert!(!hits("p:dir(ltr)", &silent, 0));
}

/// §6.6.1: with no history, `:link` and `:any-link` are the same set and
/// `:visited` is empty.
#[test]
fn link_is_every_hyperlink_and_visited_is_none() {
    let mut nodes = doc(&[("body", None), ("a", Some(0)), ("a", Some(0))]);
    nodes[1].link = true;
    for name in [":link", ":any-link"] {
        assert!(
            hits(&format!("a{name}"), &nodes, 1),
            "a{name} on a hyperlink"
        );
        assert!(
            !hits(&format!("a{name}"), &nodes, 2),
            "a{name} on an anchor that is not a link"
        );
    }
    // The one that stays empty, and the reason: a page has been nowhere.
    assert!(!hits("a:visited", &nodes, 1));
    assert!(!hits("a:visited", &nodes, 2));
}

/// §12's states, and the rule a build gets wrong by writing `!`.
///
/// `:enabled` is **not** the negation of `:disabled`, `:optional` is not the
/// negation of `:required` and `:read-write` is not the negation of
/// `:read-only`: each pair leaves out every element the document language does
/// not classify at all. The fixture's fourth element is a paragraph, and it
/// must match none of the seven.
#[test]
fn the_form_states_are_not_negations_of_each_other() {
    let mut nodes = doc(&[
        ("form", None),
        ("input", Some(0)),
        ("input", Some(0)),
        ("p", Some(0)),
    ]);
    nodes[1].ui = UiState {
        checked: true,
        disabled: Some(true),
        required: Some(true),
        read_only: Some(true),
    };
    nodes[2].ui = UiState {
        checked: false,
        disabled: Some(false),
        required: Some(false),
        read_only: Some(false),
    };
    assert!(hits("input:checked", &nodes, 1));
    assert!(!hits("input:checked", &nodes, 2));
    assert!(hits("input:disabled", &nodes, 1));
    assert!(!hits("input:enabled", &nodes, 1));
    assert!(hits("input:enabled", &nodes, 2));
    assert!(!hits("input:disabled", &nodes, 2));
    assert!(hits("input:required", &nodes, 1));
    assert!(hits("input:optional", &nodes, 2));
    assert!(!hits("input:optional", &nodes, 1));
    assert!(hits("input:read-only", &nodes, 1));
    assert!(hits("input:read-write", &nodes, 2));
    assert!(!hits("input:read-write", &nodes, 1));
    // The paragraph, which is neither half of any of the three pairs.
    for selector in [
        "p:checked",
        "p:disabled",
        "p:enabled",
        "p:required",
        "p:optional",
        "p:read-only",
        "p:read-write",
    ] {
        assert!(
            !hits(selector, &nodes, 3),
            "`{selector}` matched an element the document language does not classify"
        );
    }
}

/// §4.2's `:has()`, including the case that separates a relational selector
/// from a search.
///
/// `section:has(.a .b)` asks whether the section contains a `.b` **whose `.a`
/// ancestor is also inside it**. The fixture puts the `.a` outside on purpose:
/// a build that matched `.a .b` against every descendant and stopped there
/// says yes, and it is the only test here that would catch it.
#[test]
fn has_is_relative_to_its_scope() {
    // div.a > section > p.b, and a second section with its own div.a.
    let mut nodes = doc(&[
        ("div", None),
        ("section", Some(0)),
        ("p", Some(1)),
        ("section", Some(0)),
        ("div", Some(3)),
        ("p", Some(4)),
    ]);
    nodes[0].classes = vec!["a".to_string()];
    nodes[2].classes = vec!["b".to_string()];
    nodes[4].classes = vec!["a".to_string()];
    nodes[5].classes = vec!["b".to_string()];
    assert!(hits("section:has(.b)", &nodes, 1), "a plain descendant");
    assert!(
        !hits("section:has(.a .b)", &nodes, 1),
        "the `.a` is outside the section, so the relative selector does not match"
    );
    assert!(
        hits("section:has(.a .b)", &nodes, 3),
        "and inside the second section it does"
    );
    // The three explicit combinators, each with the twin that must fail.
    assert!(hits("section:has(> p)", &nodes, 1));
    assert!(!hits("section:has(> .a)", &nodes, 1));
    assert!(
        !hits("section:has(> p)", &nodes, 3),
        "the p is a grandchild"
    );
    assert!(hits("section:has(> div)", &nodes, 3));
    assert!(hits("section:has(+ section)", &nodes, 1));
    assert!(
        !hits("section:has(+ section)", &nodes, 3),
        "nothing follows"
    );
    assert!(hits("section:has(~ section)", &nodes, 1));
    assert!(!hits("section:has(~ section)", &nodes, 3));
    // A subject outside the scope's own run is not found: the first section
    // has no `div` inside it, however many the document holds.
    assert!(!hits("section:has(div)", &nodes, 1));
    // A comma-separated argument matches if any member does.
    assert!(hits("section:has(.q, > p)", &nodes, 1));
    assert!(!hits("section:has(.q, > em)", &nodes, 1));
    // Nested, which is what says the scope does not leak into the inner one.
    assert!(hits("div:has(section:has(> p))", &nodes, 0));
    assert!(!hits("div:has(section:has(> em))", &nodes, 0));
}

/// A `:not()` inside a `:has()` argument tests the **candidate**, not the
/// scope — which is what "the scope reaches only the leftmost compound" means
/// in practice.
#[test]
fn a_scope_does_not_leak_into_a_nested_selector_list() {
    let mut nodes = doc(&[("ul", None), ("li", Some(0)), ("li", Some(0))]);
    nodes[1].classes = vec!["done".to_string()];
    assert!(hits("ul:has(li:not(.done))", &nodes, 0));
    nodes[2].classes = vec!["done".to_string()];
    assert!(
        !hits("ul:has(li:not(.done))", &nodes, 0),
        "every li is now .done, so there is no li that is not"
    );
    assert!(hits("ul:has(li.done)", &nodes, 0));
}

/// **The seven, asserted by count and by name.**
///
/// This is the exit criterion the whole lane turns on. Every pseudo-class this
/// build recognises is parsed here, and exactly seven of them may produce
/// [`Warning::PseudoClassUnsupported`]. A list that shrinks — someone
/// implements `:target` — fails the count until the number is edited, and a
/// list that *grows* fails it too, which is the direction that matters: a
/// pseudo-class quietly demoted back to never-matching cannot hide inside a
/// suite that only checks the seven it knows about.
#[test]
fn seven_pseudo_classes_name_a_state_this_document_does_not_have() {
    // Every name `simple_pseudo_class` and `functional_pseudo_class` accept.
    let every: &[&str] = &[
        ":root",
        ":first-child",
        ":last-child",
        ":only-child",
        ":empty",
        ":first-of-type",
        ":last-of-type",
        ":only-of-type",
        ":link",
        ":any-link",
        ":checked",
        ":disabled",
        ":enabled",
        ":required",
        ":optional",
        ":read-only",
        ":read-write",
        ":hover",
        ":focus",
        ":focus-within",
        ":focus-visible",
        ":active",
        ":target",
        ":visited",
        ":not(.a)",
        ":is(.a)",
        ":matches(.a)",
        ":any(.a)",
        ":where(.a)",
        ":has(.a)",
        ":nth-child(2n)",
        ":nth-last-child(2n)",
        ":nth-of-type(2n)",
        ":nth-last-of-type(2n)",
        ":lang(en)",
        ":dir(ltr)",
    ];
    let mut stateless: Vec<&'static str> = Vec::new();
    for name in every {
        let parsed = sheet(&format!("p{name} {{ color: red }}"));
        assert_eq!(
            parsed.rules.len(),
            1,
            "`p{name}` did not parse: {:?}",
            parsed.report
        );
        for (warning, count) in &parsed.report.warnings {
            if let Warning::PseudoClassUnsupported(named) = warning {
                assert_eq!(*count, 1);
                stateless.push(named);
            }
        }
    }
    stateless.sort_unstable();
    assert_eq!(
        stateless,
        [
            ":active",
            ":focus",
            ":focus-visible",
            ":focus-within",
            ":hover",
            ":target",
            ":visited",
        ],
        "the set of pseudo-classes that name a state this document does not have"
    );
    assert_eq!(
        stateless.len(),
        7,
        "the count, asserted on its own so a shrinking list cannot read as a passing one"
    );

    // And the other half of the claim: none of the seven matches anything.
    let nodes = doc(&[("body", None), ("p", Some(0))]);
    for name in [
        ":hover",
        ":focus",
        ":focus-within",
        ":focus-visible",
        ":active",
        ":target",
        ":visited",
    ] {
        assert!(!hits(&format!("p{name}"), &nodes, 1), "p{name} matched");
    }
}

/// A cyclic `parent()` link is a caller error, and it ends in a refusal rather
/// than in a hang.
///
/// Ruling 1 is about untrusted *input*, and a slice is not input — but the
/// inheritance walk `:lang()` needs is unbounded in exactly the way a parser
/// loop is, so it is charged to the same budget and stops for the same reason.
#[test]
fn a_cyclic_parent_link_refuses_rather_than_hanging() {
    let mut nodes = doc(&[("a", None), ("b", Some(0))]);
    nodes[0].parent = Some(1);
    let parsed = sheet("b:lang(en) { color: red }");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let result = matches(&parsed.rules[0].selectors[0], &nodes, 1, &mut budget);
    assert!(
        result.is_err(),
        "an unbounded ancestor walk returned {result:?}"
    );
}
