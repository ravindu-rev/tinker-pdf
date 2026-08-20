//! CSS: the tokenizer, `css-syntax-3`'s grammar and its error recovery,
//! `selectors-4` matching, and `css-cascade-5`'s whole sorting order.
//!
//! The repository's **twenty-third** target. Like `xml`, the input is *text*
//! rather than a record layout, so there is no offset to corrupt and no length
//! field to lie about; what a mutator finds instead is the seams — a `;` inside
//! a function, a `}` inside a string, an `@media` whose block never closes, a
//! `:not()` inside a `:is()` inside a `:not()`, an `@import` that names itself.
//! Very little of that is reachable from hand-built fixtures, because a fixture
//! author writes the stylesheet they already had in mind.
//!
//! The control byte picks the **bounds** rather than the input, for gap 18a
//! milestone 8's reason: a target whose limits are all at their shipped
//! defaults never explores a refusal, because a four-million-token cap cannot
//! fire inside one iteration. All eight bits are spent, and the last pair
//! carries three related knobs rather than one — taking a second control byte
//! would shift the body by one and change the meaning of every corpus entry
//! ever written, which is a cost this target will not pay twice.
//!
//! The stylesheet is fuzzed **and so is the tree it cascades over**: the body's
//! own bytes name the elements, their classes and their ids, so a mutator that
//! writes `.lead` into the stylesheet can also write the byte that puts `lead`
//! on an element. A target that cascaded over a fixed tree would explore the
//! parser and stop at the door of the matcher.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **Every budget holds rather than being exceeded and reported.** Tokens,
//!   rules, declarations and selector matches, each checked against the cap the
//!   control byte chose rather than against the shipped one.
//! - **The index and brute force agree.** The matcher buckets rules by their
//!   rightmost compound and only tests the candidates; a bucketing bug produces
//!   a book styled slightly *less* than it should be, which reads as a plain
//!   stylesheet rather than as a defect. This is the same comparison the
//!   crate's own tests make, over stylesheets nobody wrote.
//! - **Decision 5's two name tables stay disjoint from the answer.** A
//!   `Known` declaration never carries a name from the unsupported list, an
//!   `Unsupported` one always carries a name from one of the two lists, and an
//!   `Unknown` one never does. Those three together are what makes the
//!   `Unsupported` census a number rather than a mood.
//! - **The report is deduplicated.** One entry per warning and per property
//!   name, with a count — never four hundred copies.
//! - **A selector's shape is internally consistent.** Exactly one fewer
//!   combinator than compounds, and never more compounds than the cap allows.
//! - **Parsing is deterministic.** The same bytes twice give the identical
//!   stylesheet, which is ruling 4's requirement asserted on the parser rather
//!   than on a rendered page.
//! - **`@import` terminates.** The resolver the target supplies will hand back
//!   the same body for ever, and whether that is a cycle or an infinite chain
//!   is chosen by the *input*: an href beginning `cycle` resolves to a constant
//!   address and must be refused by the cycle guard, and any other href
//!   resolves to a deeper one and must be stopped by the depth cap. A build
//!   with either guard missing does not fail an assertion here — it hangs, and
//!   libFuzzer's `-timeout` is what turns that into a finding.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_css::cascade::{cascade, Origin};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::property::{Declaration, IMPLEMENTED_NAMES, UNSUPPORTED_PROPERTIES};
use tinker_pdf_css::selector::{matches, Index, Selector};
use tinker_pdf_css::{parse, Budget, Element, ImportResolver, Limits};

/// The element the target cascades over.
struct Node {
    name: String,
    id: Option<String>,
    classes: Vec<String>,
    attributes: Vec<(String, String)>,
    style: Option<String>,
    parent: Option<usize>,
    previous: Option<usize>,
    next: Option<usize>,
}

impl Element for Node {
    fn local_name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn classes(&self) -> &[String] {
        &self.classes
    }
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
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
    fn inline_style(&self) -> Option<&str> {
        self.style.as_deref()
    }
}

/// A resolver that always answers, so `@import` can only be stopped by a guard.
///
/// Which guard is the **input's** choice: an href beginning `cycle` comes back
/// at a constant address, so the second visit is a cycle; anything else comes
/// back at an address one segment deeper, so only the depth cap can stop it.
/// Neither is a soft failure — a build missing either guard hangs rather than
/// returning a wrong answer, and the nightly's `-timeout` is what reports it.
struct Always(Vec<u8>);

impl ImportResolver for Always {
    fn resolve(&self, href: &str, base: Option<&str>) -> Option<(String, Vec<u8>)> {
        if href.starts_with("cycle") {
            return Some(("cycle".to_string(), self.0.clone()));
        }
        let address = match base {
            Some(base) => format!("{base}/{href}"),
            None => href.to_string(),
        };
        Some((address, self.0.clone()))
    }
}

/// Builds a small document-ordered tree out of the input's own bytes.
fn tree(seed: &[u8]) -> Vec<Node> {
    const NAMES: [&str; 6] = ["p", "div", "span", "a", "li", "em"];
    const CLASSES: [&str; 4] = ["lead", "a", "b", "note"];
    const IDS: [&str; 2] = ["top", "x"];

    let count = (seed.len() / 2).clamp(1, 24);
    let mut nodes: Vec<Node> = Vec::with_capacity(count);
    for index in 0..count {
        let a = seed.get(index * 2).copied().unwrap_or(0);
        let b = seed.get(index * 2 + 1).copied().unwrap_or(0);
        // The parent is always *before* this element, which is the document
        // order `cascade` refuses without.
        let parent = if index == 0 {
            None
        } else {
            Some(usize::from(a) % index)
        };
        let mut classes = Vec::new();
        if b & 1 != 0 {
            classes.push(CLASSES[usize::from(b >> 1) % CLASSES.len()].to_string());
        }
        if b & 2 != 0 {
            classes.push(CLASSES[usize::from(b >> 3) % CLASSES.len()].to_string());
        }
        nodes.push(Node {
            name: NAMES[usize::from(a >> 4) % NAMES.len()].to_string(),
            id: (b & 4 != 0).then(|| IDS[usize::from(b >> 5) % IDS.len()].to_string()),
            classes,
            attributes: vec![
                ("href".to_string(), "chapter.xhtml".to_string()),
                ("lang".to_string(), "en-GB".to_string()),
            ],
            style: (b & 8 != 0).then(|| "color: red; float: left".to_string()),
            parent,
            previous: None,
            next: None,
        });
    }
    for index in 0..nodes.len() {
        let parent = nodes[index].parent;
        let previous = (0..index).rev().find(|earlier| nodes[*earlier].parent == parent);
        nodes[index].previous = previous;
        if let Some(previous) = previous {
            nodes[previous].next = Some(index);
        }
    }
    nodes
}

/// Every property name a declaration may carry, from the two tables.
fn is_named(name: &str) -> bool {
    IMPLEMENTED_NAMES.contains(&name) || UNSUPPORTED_PROPERTIES.contains(&name)
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // Small enough that every cap is crossable inside one iteration, and varied
    // enough that both sides of each are reachable from one corpus. The last
    // pair drives three related knobs because all eight bits are spent; they
    // move together on purpose, so "a tight matcher" is one corpus dimension
    // rather than three.
    // Both refusals in this pair have to be reachable, which took a correction:
    // a tightest value of zero bytes refuses every non-empty body at the byte
    // cap and the token total is then never reached at all. So the tightest
    // value reads sixty-four bytes and stops at four tokens, and the *second*
    // value is the one that fires the byte cap.
    let (max_bytes, max_tokens) = match knobs & 3 {
        0 => (64, 4),
        1 => (16, 8),
        2 => (1024, 512),
        _ => (8 << 20, 4_000_000),
    };
    let max_rules = match (knobs >> 2) & 3 {
        0 => 0,
        1 => 2,
        2 => 32,
        _ => 20_000,
    };
    let max_declarations = match (knobs >> 4) & 3 {
        0 => 0,
        1 => 2,
        2 => 32,
        _ => 100_000,
    };
    let (max_selector_parts, max_matches, max_elements) = match (knobs >> 6) & 3 {
        0 => (1, 0, 1),
        1 => (2, 8, 4),
        2 => (8, 256, 32),
        _ => (64, 4_000_000, 65_536),
    };
    let limits = Limits {
        max_bytes,
        max_tokens,
        max_rules,
        max_declarations,
        max_selector_parts,
        max_import_depth: 8,
        max_elements,
        max_matches,
    };

    let context = MediaContext::screen(432.0, 648.0);
    let resolver = Always(body.to_vec());
    let mut budget = Budget::new(&limits);
    let Ok(sheet) = parse(
        body,
        Some("root.css"),
        &resolver,
        &context,
        &limits,
        &mut budget,
    ) else {
        return;
    };

    // Ruling 4, on the parser: the same bytes twice are the same stylesheet.
    let mut again_budget = Budget::new(&limits);
    let again = parse(
        body,
        Some("root.css"),
        &resolver,
        &context,
        &limits,
        &mut again_budget,
    )
    .expect("the same input refused on a second run");
    assert!(sheet == again, "parsing is not deterministic");

    assert!(budget.tokens() <= limits.max_tokens, "the token total was exceeded rather than refused");
    assert!(budget.rules() <= limits.max_rules, "the rule total was exceeded rather than refused");
    assert!(
        budget.declarations() <= limits.max_declarations,
        "the declaration total was exceeded rather than refused"
    );

    // The report is deduplicated: one entry per key, with a count.
    for (index, (warning, count)) in sheet.report.warnings.iter().enumerate() {
        assert!(*count > 0, "a warning was recorded zero times");
        assert!(
            !sheet.report.warnings[..index]
                .iter()
                .any(|(earlier, _)| earlier == warning),
            "a warning was recorded twice instead of counted"
        );
    }
    for (index, (name, count)) in sheet.report.unsupported.iter().enumerate() {
        assert!(*count > 0);
        assert!(is_named(name), "an unsupported property is in neither table");
        assert!(
            !sheet.report.unsupported[..index]
                .iter()
                .any(|(earlier, _)| earlier == name),
            "an unsupported property was recorded twice instead of counted"
        );
    }
    for (index, (name, count)) in sheet.report.unknown.iter().enumerate() {
        assert!(*count > 0);
        assert!(
            !is_named(name),
            "a property in one of the two tables was reported as unknown"
        );
        assert!(
            !sheet.report.unknown[..index]
                .iter()
                .any(|(earlier, _)| earlier == name),
            "an unknown property was recorded twice instead of counted"
        );
    }

    // Decision 5's split, on every declaration that survived.
    let mut flat: Vec<&Selector> = Vec::new();
    for rule in &sheet.rules {
        for selector in &rule.selectors {
            assert_eq!(
                selector.combinators.len() + 1,
                selector.compounds.len(),
                "a selector has the wrong number of combinators for its compounds"
            );
            assert!(
                selector.compounds.len() <= limits.max_selector_parts,
                "a selector past the compound cap was admitted"
            );
            flat.push(selector);
        }
        for declared in &rule.declarations {
            match &declared.declaration {
                Declaration::Known(property) => assert!(
                    !UNSUPPORTED_PROPERTIES.contains(&property.name()),
                    "a property is both implemented and listed as unsupported"
                ),
                Declaration::Unsupported { property, .. } => {
                    assert!(is_named(property), "an unsupported name is in neither table");
                }
                Declaration::Unknown { property } => {
                    assert!(!is_named(property), "a named property was reported unknown");
                }
            }
        }
    }

    let nodes = tree(body);
    if nodes.len() > limits.max_elements {
        return;
    }

    // The index is a *superset*: everything it leaves out cannot match. This is
    // the assertion that stands between a bucketing bug and a book styled
    // slightly less than it should be.
    let mut index = Index::default();
    for (handle, selector) in flat.iter().enumerate() {
        index.insert(selector, handle);
    }
    let generous = Limits {
        max_matches: 1 << 20,
        ..limits
    };
    let mut spare = Budget::new(&generous);
    for (at, node) in nodes.iter().enumerate() {
        let mut brute = Vec::new();
        for (handle, selector) in flat.iter().enumerate() {
            match matches(selector, &nodes, at, &mut spare) {
                Ok(true) => brute.push(handle),
                Ok(false) => {}
                Err(_) => return,
            }
        }
        let mut indexed = Vec::new();
        for handle in index.candidates(node) {
            match matches(flat[handle], &nodes, at, &mut spare) {
                Ok(true) => indexed.push(handle),
                Ok(false) => {}
                Err(_) => return,
            }
        }
        indexed.sort_unstable();
        assert_eq!(indexed, brute, "the index and brute force disagree");
    }

    let mut cascade_budget = Budget::new(&limits);
    if let Ok(styled) = cascade(
        &[(Origin::UserAgent, &sheet), (Origin::Author, &sheet)],
        &nodes,
        &limits,
        &mut cascade_budget,
    ) {
        assert_eq!(styled.styles.len(), nodes.len(), "an element lost its style");
        assert!(
            cascade_budget.matches() <= limits.max_matches,
            "the match total was exceeded rather than refused"
        );
        // Every computed font size is a real number: `em` compounds down a
        // tree, and a NaN or an infinity here would reach the layout engine as
        // a page of nothing.
        for style in &styled.styles {
            assert!(
                style.font_size.is_finite(),
                "a computed font size is not finite"
            );
        }
    }

    // The same bytes under the shipped bounds, which is the configuration that
    // ships. Nothing about a wider cap may turn a refusal into a panic.
    let shipped = Limits::DEFAULT;
    let mut shipped_budget = Budget::new(&shipped);
    if let Ok(sheet) = parse(
        body,
        Some("root.css"),
        &resolver,
        &context,
        &shipped,
        &mut shipped_budget,
    ) {
        let mut budget = Budget::new(&shipped);
        let _ = cascade(&[(Origin::Author, &sheet)], &nodes, &shipped, &mut budget);
    }
});
