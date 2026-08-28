//! Tagged PDF on the write side (14.7): `PageBuilder::tagged` builds a
//! structure tree, and `Document::structure()` reads it back.
//!
//! The pairing is the point. A writer checked only against the specification
//! is checked against one person's reading of it; a writer checked against
//! this repository's own reader is checked against a second reading that was
//! written first, from the other side, and against the 716 corpus documents
//! that reader was measured on. It is not an outside opinion — nothing here
//! escapes ruling 13's limit — but it is not the same reading twice either.
//!
//! These live in the facade's test directory rather than the writer's,
//! because the round trip needs both halves and `tinker-pdf-cos` cannot see
//! the facade — the dependency runs the other way, and ruling 8 keeps it
//! there. So the writer's own tests assert bytes and these assert meaning.

use tinker_pdf::{Document, DocumentBuilder, StructKid, Tier};

/// The strict structural validator (ruling 13's third axis), on a document
/// this builder produced.
///
/// The reader's own round trip says the tree means what it should. This says
/// the file holds up to ISO 32000 read strictly, which is a different claim:
/// a `/K` array of the right shape can still sit in a document whose offsets
/// or extents are wrong, and the round trip would never notice.
fn structurally_clean(bytes: Vec<u8>) {
    let doc = Document::open(bytes).expect("the document opens");
    let defects: Vec<_> = doc
        .validate()
        .into_iter()
        .filter(|defect| defect.kind.tier() == Tier::Structure)
        .collect();
    assert!(defects.is_empty(), "{defects:?}");
}

/// The milestone's exit criterion: two paragraphs in, two paragraphs out,
/// both marked-content ids matched to their element and nothing orphaned.
#[test]
fn a_two_paragraph_document_round_trips_with_no_orphans() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "First paragraph.");
        });
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 120.0, "Second paragraph.");
        });
    });

    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("the document opens");
    let tree = doc.structure().expect("it carries a structure tree");
    assert!(tree.marked, "/MarkInfo /Marked is written");
    assert!(
        tree.warnings.is_empty(),
        "the walk tolerated nothing: {:?}",
        tree.warnings
    );

    // One `/Document` holding two `/P`s.
    assert_eq!(tree.kids.len(), 1);
    let StructKid::Element(document) = &tree.kids[0] else {
        panic!("the root kid is an element");
    };
    assert_eq!(document.standard_type, "Document");
    assert_eq!(document.kids.len(), 2, "two paragraphs");

    let page = doc.page(0).expect("one page");
    let structured = tree.text_for_page(0, &page.text());
    assert_eq!(structured.orphans, 0, "every id is claimed");
    assert_eq!(structured.unmarked, 0, "nothing was drawn outside a tag");
    assert!(structured.matched > 0, "characters reached the tree");
    assert_eq!(
        structured.plain_text().trim(),
        "First paragraph.\nSecond paragraph.",
        "in structure order"
    );
}

/// Content drawn *after* a nested child still belongs to the parent, and
/// reads in the place it was drawn rather than after everything.
///
/// This is what the fresh id on resumption buys. Without it the parent has
/// one sequence spanning the child, and its trailing text reads before the
/// child's — the wrong order, and the kind of wrong that looks right until a
/// document has a footnote in the middle of a sentence.
#[test]
fn text_after_a_nested_element_reads_after_it() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "before");
            page.tagged(b"Span", |page| {
                page.text(b"F1", 12.0, 20.0, 130.0, "inside");
            });
            page.text(b"F1", 12.0, 20.0, 110.0, "after");
        });
    });

    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    let page = doc.page(0).expect("one page");
    let structured = tree.text_for_page(0, &page.text());
    assert_eq!(structured.orphans, 0);
    assert_eq!(
        structured.plain_text().trim(),
        "before\ninside\nafter",
        "the parent resumes after its child rather than before it"
    );
}

/// A `tagged` whose closure draws nothing writes no element and no sequence.
///
/// Both halves matter: an empty `BDC`/`EMC` pair in the content stream would
/// be a marked sequence the reader reports, and an element claiming it would
/// be a node nobody asked for.
#[test]
fn an_element_that_draws_nothing_is_not_written() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "only this");
        });
        page.tagged(b"Figure", |_| {});
    });

    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    let StructKid::Element(document) = &tree.kids[0] else {
        panic!("the root kid is an element");
    };
    assert_eq!(document.kids.len(), 1, "the empty Figure is not there");
    assert_eq!(tree.content_count(), 1, "and neither is its sequence");
}

/// Untagged drawing beside tagged drawing: the untagged characters are
/// counted as unmarked rather than claimed by whichever element is nearest.
#[test]
fn untagged_content_beside_tagged_content_is_counted_not_claimed() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 170.0, "loose");
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "tagged");
        });
    });

    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    let page = doc.page(0).expect("one page");
    let structured = tree.text_for_page(0, &page.text());
    assert_eq!(structured.orphans, 0);
    assert_eq!(structured.unmarked, 5, "`loose`");
    assert_eq!(structured.plain_text().trim(), "tagged");
}

/// A tag needing 7.3.5 escaping is escaped rather than refused, and reads
/// back as the bytes it was given.
#[test]
fn a_tag_that_needs_escaping_survives_the_round_trip() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"Odd Tag#1", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "x");
        });
    });

    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    let StructKid::Element(document) = &tree.kids[0] else {
        panic!("the root kid is an element");
    };
    let StructKid::Element(odd) = &document.kids[0] else {
        panic!("one kid");
    };
    assert_eq!(odd.raw_type, "Odd Tag#1");
}

/// Two pages, two `/StructParents` keys, and each page's ids resolved against
/// its own key rather than a document-wide counter.
#[test]
fn each_page_gets_its_own_parent_tree_key() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for word in ["one", "two"] {
        builder.add_page(300.0, 200.0, |page| {
            page.tagged(b"P", |page| {
                page.text(b"F1", 12.0, 20.0, 150.0, word);
            });
        });
    }

    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    for (index, expected) in ["one", "two"].iter().enumerate() {
        let index = index as u32;
        let page = doc.page(index).expect("the page");
        let structured = tree.text_for_page(index, &page.text());
        assert_eq!(structured.orphans, 0, "page {index}");
        assert_eq!(structured.plain_text().trim(), *expected);
    }
}

/// An untagged document is written exactly as it was before this feature: no
/// `/StructTreeRoot`, no `/MarkInfo`, no `/StructParents`.
///
/// A document that gains keys it does not use is a document whose bytes moved
/// for a feature it did not ask for, which is what the determinism hashes
/// exist to notice — and this says it here, where the reason is legible,
/// rather than only as a hash that did not change.
#[test]
fn an_untagged_document_gains_nothing() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 150.0, "plain");
    });
    let bytes = builder.finish();
    for key in [&b"StructTreeRoot"[..], b"MarkInfo", b"StructParents"] {
        assert!(
            !bytes.windows(key.len()).any(|window| window == key),
            "an untagged document names /{}",
            String::from_utf8_lossy(key)
        );
    }
    assert!(Document::open(bytes).expect("opens").structure().is_none());
}
