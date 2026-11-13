use super::*;
use tinker_pdf_css::cascade::{cascade, Origin};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::selector::{matches, Index};
use tinker_pdf_css::{parse, Budget, Element, Limits, NoImports, Stylesheet};

fn inputs(count: usize, longest: usize) -> impl Iterator<Item = Vec<u8>> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..count).map(move |n| {
        let len = (n * 89) % longest;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    })
}

/// Both writings of a generated sheet, parsed as `css_structured` parses
/// them, or `None` where a cap stops both.
fn both(generated: &Generated) -> Option<(Stylesheet, Stylesheet)> {
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
    match (
        read(&generated.sheet, false),
        read(&generated.wrapped, true),
    ) {
        (Ok(plain), Ok(wrapped)) => Some((plain, wrapped)),
        (Err(_), Err(_)) => None,
        (plain, wrapped) => panic!("{:?} {:?}", plain.err(), wrapped.err()),
    }
}

fn wide() -> Limits {
    Limits {
        max_matches: 1 << 28,
        ..Limits::DEFAULT
    }
}

/// Every generated sheet parses the same way in both writings: as many
/// rules, each with its declarations, its layer and its selectors'
/// specificities and pseudo-elements.
#[test]
fn both_writings_parse_to_the_same_rules() {
    let mut parsed = 0;
    for (n, input) in inputs(800, 1500).enumerate() {
        let generated = generate(&mut Choices::new(&input));
        let Some((plain, wrapped)) = both(&generated) else {
            continue;
        };
        parsed += 1;
        assert_eq!(plain.rules.len(), wrapped.rules.len(), "input {n}");
        for (a, b) in plain.rules.iter().zip(&wrapped.rules) {
            assert_eq!(a.declarations, b.declarations, "input {n}");
            assert_eq!(a.layer, b.layer, "input {n}");
            let specificities = |r: &tinker_pdf_css::StyleRule| {
                r.selectors
                    .iter()
                    .map(|s| (s.specificity, s.pseudo_element))
                    .collect::<Vec<_>>()
            };
            assert_eq!(specificities(a), specificities(b), "input {n}");
        }
    }
    assert!(parsed > 700, "only {parsed} sheets parsed");
}

/// The rule index's candidates hold every selector brute force matches,
/// and the indexed cascade is the brute-force one, element for element.
#[test]
fn the_index_and_brute_force_give_one_cascade() {
    let mut styled = 0;
    for (n, input) in inputs(400, 1500).enumerate() {
        let generated = generate(&mut Choices::new(&input));
        let Some((plain, wrapped)) = both(&generated) else {
            continue;
        };
        let limits = wide();
        let flat: Vec<_> = plain.rules.iter().flat_map(|r| &r.selectors).collect();
        let mut index = Index::default();
        for (handle, selector) in flat.iter().enumerate() {
            index.insert(selector, handle);
        }
        let mut budget = Budget::new(&limits);
        for (at, node) in generated.nodes.iter().enumerate() {
            let brute: Vec<usize> = (0..flat.len())
                .filter(|h| matches(flat[*h], &generated.nodes, at, &mut budget).unwrap())
                .collect();
            let mut indexed: Vec<usize> = index
                .candidates(node)
                .into_iter()
                .filter(|h| matches(flat[*h], &generated.nodes, at, &mut budget).unwrap())
                .collect();
            indexed.sort_unstable();
            assert_eq!(indexed, brute, "input {n}, element {at}");
        }
        let run = |sheet: &Stylesheet| {
            let mut budget = Budget::new(&limits);
            cascade(
                &[(Origin::Author, sheet)],
                &generated.nodes,
                &limits,
                &mut budget,
            )
            .expect("a cascade under the wide budget")
        };
        let (a, b) = (run(&plain), run(&wrapped));
        assert_eq!(
            format!("{:?}", a.styles),
            format!("{:?}", b.styles),
            "input {n}"
        );
        assert_eq!(
            format!("{:?}", a.generated),
            format!("{:?}", b.generated),
            "input {n}"
        );
        styled += 1;
    }
    assert!(styled > 300);
}

/// `:is(S)` is the premise both writings stand on: over one tree, `S` and
/// `:is(S)` match the same elements, at the same specificity, and only `S`
/// is bucketed under its key.
#[test]
fn a_selector_and_its_is_wrapping_match_alike() {
    // div > (p.a, p > em), built by hand.
    let make = |name: &'static str, parent: Option<usize>, class: &str| Node {
        name,
        id: None,
        classes: if class.is_empty() {
            Vec::new()
        } else {
            vec![class.to_string()]
        },
        attributes: Vec::new(),
        style: None,
        parent,
        previous: None,
        next: None,
        children: 0,
    };
    let mut nodes = vec![
        make("div", None, ""),
        make("p", Some(0), "a"),
        make("p", Some(0), ""),
        make("em", Some(2), ""),
    ];
    nodes[1].next = Some(2);
    nodes[2].previous = Some(1);
    nodes[0].children = 2;
    nodes[2].children = 1;
    for selector in [
        "p.a",
        "div > p > em:first-child",
        "p:last-child em",
        "div p ~ p",
    ] {
        let sheet = |text: &str| {
            let limits = Limits::DEFAULT;
            let mut budget = Budget::new(&limits);
            let context = MediaContext::screen(432.0, 648.0);
            parse(
                format!("{text} {{ color: red }}").as_bytes(),
                None,
                &NoImports,
                &context,
                &limits,
                &mut budget,
            )
            .expect("parses")
        };
        let plain = sheet(selector);
        let wrapped = sheet(&format!(":is({selector})"));
        let (s, w) = (&plain.rules[0].selectors[0], &wrapped.rules[0].selectors[0]);
        assert_eq!(s.specificity, w.specificity, "{selector}");
        let limits = Limits::DEFAULT;
        let mut budget = Budget::new(&limits);
        for at in 0..nodes.len() {
            assert_eq!(
                matches(s, &nodes, at, &mut budget).unwrap(),
                matches(w, &nodes, at, &mut budget).unwrap(),
                "{selector} at element {at}"
            );
        }
        let mut index = Index::default();
        index.insert(w, 0);
        for node in &nodes {
            assert_eq!(
                index.candidates(node),
                vec![0],
                "{selector} is not universal"
            );
        }
    }
}

/// The tree is in document order: every parent before its children, and
/// the sibling links the previous and next of one another under one parent.
#[test]
fn the_tree_is_in_document_order_with_its_sibling_links() {
    for input in inputs(500, 600) {
        let nodes = generate(&mut Choices::new(&input)).nodes;
        assert!(nodes[0].parent.is_none());
        for (at, node) in nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                assert!(parent < at);
            }
            if let Some(previous) = node.previous {
                assert_eq!(nodes[previous].next, Some(at));
                assert_eq!(nodes[previous].parent, node.parent);
            }
            let children = nodes.iter().filter(|n| n.parent == Some(at)).count();
            assert_eq!(node.children, children);
            assert_eq!(node.is_empty(), children == 0);
        }
    }
}

/// Every selector form, combinator and at-rule the generator counts is
/// written by some input.
#[test]
fn every_selector_form_and_at_rule_is_written() {
    let all: Vec<Generated> = inputs(1500, 1500)
        .map(|input| generate(&mut Choices::new(&input)))
        .collect();
    for bit in 0..4 {
        assert!(
            all.iter().any(|g| g.combinators & (1 << bit) != 0),
            "combinator {bit}"
        );
    }
    for bit in 0..7 {
        assert!(
            all.iter().any(|g| g.at_rules & (1 << bit) != 0),
            "at-rule {bit}"
        );
    }
    assert!(all.iter().any(|g| g.functional > 0));
    assert!(all.iter().any(|g| g.simple > 0));
    assert!(all.iter().any(|g| g.pseudo_elements > 0));
    assert!(all.iter().any(|g| g.attribute_selectors > 0));
    assert!(all.iter().any(|g| g.cycles > 0));
    assert!(all.iter().any(|g| g.important > 0));
    assert!(all.iter().any(|g| g.defaulted > 0));
    assert!(all.iter().any(|g| g.unknown > 0));
    assert!(all.iter().any(|g| g.inline > 0));
}

/// The plainest input is an empty sheet over one element.
#[test]
fn the_empty_input_is_an_empty_sheet_over_one_element() {
    let generated = generate(&mut Choices::new(&[]));
    assert!(generated.sheet.is_empty());
    assert!(generated.wrapped.is_empty());
    assert!(generated.imports.is_empty());
    assert_eq!(generated.nodes.len(), 1);
}

type Feature = fn(&Generated) -> bool;

/// Rewrites `fuzz/corpus/css_structured/`: one seed per feature, each the
/// first input of `first_input`'s sequence whose sheet has it and parses.
#[test]
#[ignore = "writes fuzz/corpus/css_structured"]
fn write_the_fuzz_seeds() {
    let features: &[(&str, Feature)] = &[
        ("every-combinator", |g| g.combinators == 0b1111),
        ("functional-pseudo-classes", |g| g.functional >= 6),
        ("pseudo-elements", |g| g.pseudo_elements >= 3),
        ("attribute-selectors", |g| g.attribute_selectors >= 4),
        ("imports-and-cycles", |g| {
            g.cycles >= 2 && g.imports.len() >= 2
        }),
        ("media-supports-layers", |g| g.at_rules & 0b1110 == 0b1110),
        ("layer-statements-and-font-faces", |g| {
            g.at_rules & 0b11_0000 == 0b11_0000
        }),
        ("important-and-defaulting", |g| {
            g.important >= 3 && g.defaulted >= 3
        }),
        ("unknown-properties", |g| {
            g.unknown >= 3 && g.at_rules & 64 != 0
        }),
        ("inline-styles", |g| g.inline >= 3 && g.nodes.len() >= 12),
        ("full-tree", |g| g.nodes.len() == MAX_ELEMENTS),
    ];
    let read = |data: &[u8]| {
        let mut choices = Choices::new(data);
        let _ = generate(&mut choices);
        choices.consumed()
    };
    let mut seeds: Vec<(&str, Vec<u8>)> = vec![("plain", Vec::new())];
    for (name, has) in features {
        let taken: Vec<Vec<u8>> = seeds.iter().map(|(_, seed)| seed.clone()).collect();
        let seed = crate::first_input(
            1500,
            200_000,
            |data| {
                let generated = generate(&mut Choices::new(data));
                has(&generated) && both(&generated).is_some()
            },
            read,
            &taken,
        )
        .unwrap_or_else(|| panic!("no input has {name}"));
        assert!(
            has(&generate(&mut Choices::new(&seed))),
            "{name} lost its feature when cut"
        );
        seeds.push((name, seed));
    }
    crate::write_seeds("css_structured", &seeds);
}
