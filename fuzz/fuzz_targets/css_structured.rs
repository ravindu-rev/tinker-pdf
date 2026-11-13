//! Stylesheets, generated: well-formed rules of every selector form and
//! at-rule, cascaded over a generated tree by the rule index and by brute
//! force, which must give one cascade.
//!
//! `css` hands mutated text to the parser, and most mutations of a stylesheet
//! land in its recovery; what reaches the matcher is what survives. This
//! target reads its input as choices (`tinker_pdf_fuzz::Choices`) and writes,
//! with `tinker_pdf_fuzz::css`, a stylesheet whose every rule is well formed:
//! selectors of all four combinators, compounds of a type or `*`, ids,
//! classes, attribute selectors of every matcher and case flag, and the
//! pseudo-classes the matcher implements — the tree-structural ones, `:nth-*`
//! with `of S`, `:not()`, `:is()`, `:where()`, `:has()` with relative
//! selectors, `:lang()`, `:dir()`, the user-interface states and the seven of
//! a reading session — with `::before`, `::after`, `::first-letter` and
//! `::first-line`; declarations of implemented, unsupported and unknown
//! properties, the defaulting keywords and `!important`; and `@import`s with
//! layers and media naming one another and the sheet that imports them,
//! `@media`, `@supports` and `@layer` blocks nested, `@layer` statements,
//! `@font-face` and an at-rule nothing defines. The tree is generated with it:
//! elements in document order with ids, classes repeated and not, links,
//! languages, directions and `style=""`.
//!
//! The stylesheet is written twice: as written, and with every selector `S`
//! written `:is(S)`, a pseudo-element kept outside. `:is(S)` matches what `S`
//! matches at `S`'s specificity, and the rule index, which buckets a selector
//! by its rightmost compound's id, class or type, puts every one of them in
//! the bucket every element is a candidate of — so the second writing is
//! cascaded by brute force.
//!
//! The caps — 16 items a sheet, three imported sheets, two levels of block
//! and of functional pseudo-class, four compounds a selector, six declarations
//! a block and 24 elements — are `tinker_pdf_fuzz::css`'s constants.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **Both writings parse to the same rules.** One parses exactly when the
//!   other does, to as many rules, each with the same declarations, layer,
//!   selectors' specificities and pseudo-elements: wrapping a selector in
//!   `:is()` changes nothing a cascade sorts by.
//! - **The index and brute force match the same rules for every element**, as
//!   `css` asserts, over well-formed selectors.
//! - **The two writings give one cascade**: every element's computed style
//!   and its generated boxes are the same from the indexed cascade and the
//!   brute-force one. A bucketing defect styles a book slightly less than it
//!   should be, which reads as a plain stylesheet; here it is a difference.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Both cascades are this crate's, so a selector both match wrongly, or a
//! declaration both apply wrongly, passes. `crates/tinker-pdf-css`'s own tests
//! hold matching and the cascade to the specifications' examples, and the
//! EPUB reftests (`crates/tinker-pdf/tests/epub_reftest.rs`) compare rendered
//! pages.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_css::cascade::{cascade, Origin};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::selector::{matches, Index, Selector};
use tinker_pdf_css::{parse, Budget, Limits};
use tinker_pdf_fuzz::css::{generate, Imports};
use tinker_pdf_fuzz::Choices;

fuzz_target!(|data: &[u8]| {
    let generated = generate(&mut Choices::new(data));
    let context = MediaContext::screen(432.0, 648.0);
    let limits = Limits::DEFAULT;
    let read = |bytes: &[u8], wrapped: bool| {
        let imports = Imports {
            sheets: &generated.imports,
            wrapped,
            root: bytes,
        };
        let mut budget = Budget::new(&limits);
        parse(
            bytes,
            Some("root.css"),
            &imports,
            &context,
            &limits,
            &mut budget,
        )
    };
    let (plain, wrapped) = match (
        read(&generated.sheet, false),
        read(&generated.wrapped, true),
    ) {
        (Ok(plain), Ok(wrapped)) => (plain, wrapped),
        // A cap both reach: the rules are the same rules, and so is the count.
        (Err(_), Err(_)) => return,
        (plain, wrapped) => panic!(
            "one writing parsed and the other did not: {:?}, {:?}",
            plain.err(),
            wrapped.err()
        ),
    };

    assert_eq!(
        plain.rules.len(),
        wrapped.rules.len(),
        "wrapping the selectors kept another number of rules"
    );
    for (at, (a, b)) in plain.rules.iter().zip(&wrapped.rules).enumerate() {
        assert_eq!(a.declarations, b.declarations, "rule {at}'s declarations");
        assert_eq!(a.layer, b.layer, "rule {at}'s layer");
        assert_eq!(
            a.selectors.len(),
            b.selectors.len(),
            "rule {at}'s selectors"
        );
        for (sa, sb) in a.selectors.iter().zip(&b.selectors) {
            assert_eq!(sa.specificity, sb.specificity, "rule {at}: a specificity");
            assert_eq!(
                sa.pseudo_element, sb.pseudo_element,
                "rule {at}: a pseudo-element"
            );
        }
    }

    // The index is a superset: everything it leaves out cannot match.
    let flat: Vec<&Selector> = plain.rules.iter().flat_map(|r| &r.selectors).collect();
    let mut index = Index::default();
    for (handle, selector) in flat.iter().enumerate() {
        index.insert(selector, handle);
    }
    // Brute force does every element's work for every selector, so the match
    // budget is widened past what the indexed side alone could spend.
    let wide = Limits {
        max_matches: 1 << 28,
        ..limits
    };
    let mut spare = Budget::new(&wide);
    for (at, node) in generated.nodes.iter().enumerate() {
        let mut brute = Vec::new();
        for (handle, selector) in flat.iter().enumerate() {
            if matches(selector, &generated.nodes, at, &mut spare)
                .unwrap_or_else(|refusal| panic!("matching was refused: {refusal:?}"))
            {
                brute.push(handle);
            }
        }
        let mut indexed = Vec::new();
        for handle in index.candidates(node) {
            if matches(flat[handle], &generated.nodes, at, &mut spare)
                .unwrap_or_else(|refusal| panic!("matching was refused: {refusal:?}"))
            {
                indexed.push(handle);
            }
        }
        indexed.sort_unstable();
        assert_eq!(
            indexed, brute,
            "the index and brute force disagree at element {at}"
        );
    }

    let styled = |sheet| {
        let mut budget = Budget::new(&wide);
        cascade(
            &[(Origin::Author, sheet)],
            &generated.nodes,
            &wide,
            &mut budget,
        )
    };
    match (styled(&plain), styled(&wrapped)) {
        (Ok(indexed), Ok(brute)) => {
            assert_eq!(
                indexed.styles.len(),
                generated.nodes.len(),
                "an element lost its style"
            );
            // Compared as written out, so a value both cascades compute as NaN
            // compares as the same value.
            for (at, (a, b)) in indexed.styles.iter().zip(&brute.styles).enumerate() {
                assert_eq!(
                    format!("{a:?}"),
                    format!("{b:?}"),
                    "element {at} is styled otherwise by brute force"
                );
            }
            assert_eq!(
                format!("{:?}", indexed.generated),
                format!("{:?}", brute.generated),
                "brute force generates other boxes"
            );
        }
        (indexed, brute) => panic!(
            "a cascade under the wide budget was refused: {:?}, {:?}",
            indexed.err(),
            brute.err()
        ),
    }
});
