//! Size-scaling complexity tests for CSS: tokenising, parsing, selector
//! matching and the cascade (`docs/design/resource-exhaustion.md`, "The
//! size-scaling law").
//!
//! Each shape is a stylesheet and a tree this file writes at a size n and at
//! four times n, parsed and cascaded under one [`Budget`], whose four totals
//! are the work: the tokens the tokenizer produced, the rules and the
//! declarations the parser admitted, and the compound-against-element tests
//! and tree steps selector matching charged. The law is the design's: the
//! work at 4 n is at most 1.1 × 4 times the work at n. A quadratic shape
//! costs sixteen times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: each
//! cascade gives the computed style the sheet says. Every size keeps four
//! times n inside the caps on the shape's path — the 64 compounds of one
//! selector, the book's rules, declarations and tests, the elements of one
//! document — so no shape measures a cap. No clock is read here.

use tinker_pdf_css::cascade::{self, Origin, StyleTree};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::{parse, Budget, Element, Limits, NoImports, Stylesheet};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// The law for a shape whose parser reads each element a bounded number of
/// times, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:40} work at n {n:>8}  at 4n {kn:>8}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

/// An element of the tree a shape cascades over.
#[derive(Clone, Debug, Default)]
struct Node {
    name: String,
    classes: Vec<String>,
    parent: Option<usize>,
    previous: Option<usize>,
    next: Option<usize>,
}

impl Element for Node {
    fn local_name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> Option<&str> {
        None
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
}

/// The four totals the budget has spent: the work of everything done under
/// it.
fn work(budget: &Budget) -> u64 {
    [
        budget.tokens(),
        budget.rules(),
        budget.declarations(),
        budget.matches(),
    ]
    .into_iter()
    .map(|count| u64::try_from(count).unwrap_or(u64::MAX))
    .fold(0, u64::saturating_add)
}

/// `source` parsed under `budget`, with nothing discarded.
fn sheet(source: &str, budget: &mut Budget) -> Stylesheet {
    let context = MediaContext::screen(432.0, 648.0);
    let sheet = parse(
        source.as_bytes(),
        None,
        &NoImports,
        &context,
        &Limits::DEFAULT,
        budget,
    )
    .expect("the sheet is inside every cap");
    assert_eq!(
        sheet.report.discarded_rules, 0,
        "every rule the shape wrote is read"
    );
    assert_eq!(
        sheet.report.discarded_declarations, 0,
        "and every declaration"
    );
    sheet
}

/// `tree` cascaded under `sheet` and `budget`.
fn styled(sheet: &Stylesheet, tree: &[Node], budget: &mut Budget) -> StyleTree {
    cascade::cascade(&[(Origin::Author, sheet)], tree, &Limits::DEFAULT, budget)
        .expect("the cascade is inside every cap")
}

/// A root and `n` children under it, siblings in document order, the child
/// at `i` named `name` with the class `c{i}`.
fn siblings(n: usize, name: &str) -> Vec<Node> {
    let mut tree = vec![Node {
        name: "body".to_owned(),
        ..Node::default()
    }];
    for i in 0..n {
        tree.push(Node {
            name: name.to_owned(),
            classes: vec![format!("c{i}")],
            parent: Some(0),
            previous: i.checked_sub(1).map(|p| p + 1),
            next: (i + 1 < n).then_some(i + 2),
        });
    }
    tree
}

/// `n` elements nested one inside another under a root, the one at depth
/// `i` a `div` with the class `c{i}`.
fn chain(n: usize) -> Vec<Node> {
    let mut tree = vec![Node {
        name: "body".to_owned(),
        ..Node::default()
    }];
    for i in 0..n {
        tree.push(Node {
            name: "div".to_owned(),
            classes: vec![format!("c{i}")],
            parent: Some(i),
            ..Node::default()
        });
    }
    tree
}

// ---------------------------------------------------------------------------
// Tokenising and parsing.
// ---------------------------------------------------------------------------

/// A stylesheet of `n` rules, each of a shorthand and a longhand.
fn many_rules(n: usize) -> u64 {
    let source: String = (0..n)
        .map(|i| format!(".c{i} {{ margin: 0 {i}px; font-weight: bold }}\n"))
        .collect();
    let mut budget = Budget::new(&Limits::DEFAULT);
    let parsed = sheet(&source, &mut budget);
    assert_eq!(parsed.rules.len(), n, "every rule is read");
    assert_eq!(budget.rules(), n, "and charged once");
    work(&budget)
}

#[test]
fn a_stylesheet_of_more_rules_costs_work_linear_in_its_rules() {
    holds_the_law(
        "CSS stylesheet of n rules",
        many_rules(1000),
        many_rules(4000),
    );
}

// ---------------------------------------------------------------------------
// Selector matching.
// ---------------------------------------------------------------------------

/// One rule whose selector is `n` compounds joined by `combinator`, matched
/// against a chain of `n` elements whose deepest is its subject.
fn compounds(n: usize, combinator: &str) -> u64 {
    let selector = (0..n)
        .map(|i| format!("div.c{i}"))
        .collect::<Vec<_>>()
        .join(combinator);
    let source = format!("{selector} {{ font-weight: bold }}");
    let mut budget = Budget::new(&Limits::DEFAULT);
    let parsed = sheet(&source, &mut budget);
    assert_eq!(
        parsed.rules.len(),
        1,
        "the selector is inside its compound cap"
    );
    let tree = chain(n);
    let styles = styled(&parsed, &tree, &mut budget).styles;
    assert_eq!(styles.len(), n + 1);
    for (at, style) in styles.iter().enumerate() {
        let subject = at == n;
        assert_eq!(
            style.font_weight,
            if subject { 700 } else { 400 },
            "element {at} matches only as the subject"
        );
    }
    work(&budget)
}

#[test]
fn a_selector_of_more_compounds_costs_work_linear_in_its_compounds() {
    // Inside the 64 compounds of one selector at both sizes.
    holds_the_law(
        "CSS selector of n child compounds",
        compounds(12, " > "),
        compounds(48, " > "),
    );
    holds_the_law(
        "CSS selector of n descendant compounds",
        compounds(12, " "),
        compounds(48, " "),
    );
}

/// `n` elements, each matched by the one rule of `n` that names its class.
fn elements_against_rules(n: usize) -> u64 {
    let source: String = (0..n)
        .map(|i| format!(".c{i} {{ font-size: {}px }}\n", i + 1))
        .collect();
    let mut budget = Budget::new(&Limits::DEFAULT);
    let parsed = sheet(&source, &mut budget);
    let tree = siblings(n, "p");
    let styles = styled(&parsed, &tree, &mut budget).styles;
    for i in [0, n / 2, n - 1] {
        let size = styles.get(i + 1).map(|s| s.font_size);
        assert_eq!(size, Some((i + 1) as f64), "element {i} takes its own rule");
    }
    work(&budget)
}

#[test]
fn more_elements_against_more_rules_cost_work_linear_in_both() {
    // Each element's class finds its one rule in the index: the rules-times-
    // elements product is what a stylesheet of one shared key costs, and
    // `MAX_SELECTOR_MATCHES` bounds that.
    holds_the_law(
        "CSS n elements against n rules",
        elements_against_rules(1000),
        elements_against_rules(4000),
    );
}

// ---------------------------------------------------------------------------
// The cascade.
// ---------------------------------------------------------------------------

/// One rule of `n` declarations of one property, cascaded over a few
/// elements it matches: the last declaration wins.
fn declarations(n: usize) -> u64 {
    let body: String = (0..n)
        .map(|i| format!("font-size: {}px; ", i + 1))
        .collect();
    let source = format!("p {{ {body}}}");
    let mut budget = Budget::new(&Limits::DEFAULT);
    let parsed = sheet(&source, &mut budget);
    assert_eq!(budget.declarations(), n, "every declaration is admitted");
    let tree = siblings(4, "p");
    let styles = styled(&parsed, &tree, &mut budget).styles;
    for style in styles.iter().skip(1) {
        assert_eq!(
            style.font_size, n as f64,
            "the last declaration written wins"
        );
    }
    work(&budget)
}

#[test]
fn a_cascade_over_more_declarations_costs_work_linear_in_the_declarations() {
    holds_the_law(
        "CSS cascade over n declarations",
        declarations(1000),
        declarations(4000),
    );
}
