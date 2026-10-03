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

// ---- the general tagging API (`Tag`, `open_tag`, `close_tag`) --------------

use tinker_pdf::{StructElement, Tag};

/// Every element in the tree, depth-first.
fn elements(doc: &Document) -> Vec<StructElement> {
    doc.structure()
        .expect("a tree")
        .elements()
        .into_iter()
        .cloned()
        .collect()
}

/// The one element of a type, asserting there is exactly one.
fn only(doc: &Document, kind: &str) -> StructElement {
    let found: Vec<StructElement> = elements(doc)
        .into_iter()
        .filter(|element| element.standard_type == kind)
        .collect();
    assert_eq!(found.len(), 1, "{} elements of type {kind}", found.len());
    found.into_iter().next().expect("asserted above")
}

/// 14.9's four properties and `/T`, written by the builder and read back by
/// the reader as the text they were given.
///
/// Through both encodings a text string can take below 2.0 — PDFDocEncoding
/// for the ASCII ones and UTF-16BE for the ones that need it — and through
/// UTF-8 in a document declaring 2.0, which is 7.9.2.2's third form.
#[test]
fn the_accessibility_properties_are_written_and_read_back() {
    for (major, minor) in [(1u8, 7u8), (2, 0)] {
        let mut builder = DocumentBuilder::with_version(major, minor);
        builder.add_base_font(b"F1", b"Helvetica");
        builder.add_page(300.0, 200.0, |page| {
            page.tagged_with(
                &Tag::new(b"Figure")
                    .alt("Un chat endormi sur un tapis \u{2014} \u{263A}")
                    .title("Plate 1")
                    .lang("fr-CA"),
                |page| page.fill_rect(10.0, 10.0, 50.0, 50.0, 0.5),
            );
            page.tagged_with(
                &Tag::new(b"Span")
                    .actual_text("\u{FB01}sh")
                    .expansion("fish"),
                |page| page.text(b"F1", 12.0, 20.0, 150.0, "fish"),
            );
        });
        let bytes = builder.finish();
        structurally_clean(bytes.clone());
        let doc = Document::open(bytes).expect("opens");

        let figure = only(&doc, "Figure");
        assert_eq!(
            figure.alt.as_deref(),
            Some("Un chat endormi sur un tapis \u{2014} \u{263A}"),
            "{major}.{minor}"
        );
        assert_eq!(figure.title.as_deref(), Some("Plate 1"));
        assert_eq!(figure.lang.as_deref(), Some("fr-CA"));
        assert_eq!(figure.actual_text, None, "nothing unstated is written");

        let span = only(&doc, "Span");
        assert_eq!(span.actual_text.as_deref(), Some("\u{FB01}sh"));
        assert_eq!(span.expansion.as_deref(), Some("fish"));
        assert_eq!(span.alt, None);

        // The join: `/ActualText` replaces the glyphs (14.9.4).
        let tree = doc.structure().expect("a tree");
        let structured = tree.text_for_page(0, &doc.page(0).expect("a page").text());
        assert!(
            structured.nodes.iter().any(|node| node.text == "\u{FB01}sh"
                && node.source == tinker_pdf::TextSource::ActualText),
            "{:?}",
            structured.nodes
        );
    }
}

/// **An element that draws nothing is kept when it says something.**
///
/// `an_element_that_draws_nothing_is_not_written` above is the other half and
/// still holds: an empty `Figure` that states nothing is dropped. One that
/// carries an `/Alt` is the case that wants keeping — a picture described in
/// words — and so is a table cell asked to be kept, whose absence would move
/// every cell after it one column left.
#[test]
fn an_empty_element_that_says_something_is_kept() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "only this");
        });
        page.tagged_with(&Tag::new(b"Figure").alt("an empty frame"), |_| {});
        page.tagged_with(&Tag::new(b"TD").keep_empty(), |_| {});
        page.tagged_with(&Tag::new(b"Div"), |_| {});
    });

    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let StructKid::Element(document) = &tree.kids[0] else {
        panic!("the root kid is an element");
    };
    let kinds: Vec<&str> = document
        .kids
        .iter()
        .filter_map(|kid| match kid {
            StructKid::Element(element) => Some(element.standard_type.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        ["P", "Figure", "TD"],
        "the bare empty Div is dropped"
    );
    let figure = only(&doc, "Figure");
    assert!(figure.kids.is_empty(), "an empty element claims nothing");
    assert_eq!(figure.page, None, "and so states no default page");
    assert_eq!(tree.content_count(), 1, "and writes no sequence");
}

/// `open_tag` and `close_tag` span **content-stream calls and pages**: a
/// paragraph opened on one page and closed on the next is one `/P`, whose
/// kids on its second page carry their own `/Pg`.
#[test]
fn an_element_opened_on_one_page_and_closed_on_the_next_is_one_element() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        assert!(page.open_tag(&Tag::new(b"Sect").title("Chapter")));
        page.tagged(b"H1", |page| page.text(b"F1", 18.0, 20.0, 170.0, "Title"));
        assert!(page.open_tag(&Tag::new(b"P").lang("en")));
        page.text(b"F1", 12.0, 20.0, 150.0, "begins here");
        page.text(b"F1", 12.0, 20.0, 130.0, "and goes on");
        // Left open: the page is pushed with the paragraph and the section
        // both unclosed.
    });
    builder.add_page(300.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 170.0, "and ends here.");
        assert!(page.close_tag(), "the paragraph");
        page.tagged(b"P", |page| page.text(b"F1", 12.0, 20.0, 150.0, "Next."));
        assert!(page.close_tag(), "the section");
        assert!(!page.close_tag(), "nothing is left open");
    });

    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

    let section = only(&doc, "Sect");
    assert_eq!(section.title.as_deref(), Some("Chapter"));
    let paragraphs: Vec<StructElement> = elements(&doc)
        .into_iter()
        .filter(|element| element.standard_type == "P")
        .collect();
    assert_eq!(paragraphs.len(), 2, "the broken paragraph is one element");
    let broken = &paragraphs[0];
    assert_eq!(broken.lang.as_deref(), Some("en"), "stated once, kept once");
    let pages: Vec<Option<u32>> = broken
        .kids
        .iter()
        .filter_map(|kid| match kid {
            StructKid::Content { page, .. } => Some(*page),
            _ => None,
        })
        .collect();
    assert_eq!(pages, [Some(0), Some(1)], "one sequence on each page");

    for (index, expected) in [
        (0u32, "Title\nbegins hereand goes on"),
        (1, "and ends here.\nNext."),
    ] {
        let structured = tree.text_for_page(index, &doc.page(index).expect("a page").text());
        assert_eq!(structured.orphans, 0, "page {index}");
        assert_eq!(structured.unmarked, 0, "page {index}");
        assert_eq!(structured.plain_text().trim(), expected, "page {index}");
    }
}

/// A closure's element is the closure's to close: `close_tag` inside it
/// cannot reach it, and what the closure opens and leaves open is closed when
/// it returns.
#[test]
fn a_closure_closes_what_it_opened_and_nothing_outside_it() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"Div", |page| {
            assert!(!page.close_tag(), "the Div is the closure's");
            assert!(page.open_tag(&Tag::new(b"P")));
            page.text(b"F1", 12.0, 20.0, 150.0, "inside");
            // The P is left open, and the closure closes it.
        });
        page.text(b"F1", 12.0, 20.0, 120.0, "outside");
    });

    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    let div = only(&doc, "Div");
    assert!(
        matches!(&div.kids[..], [StructKid::Element(p)] if p.standard_type == "P"),
        "{:?}",
        div.kids
    );
    let structured = tree.text_for_page(0, &doc.page(0).expect("a page").text());
    assert_eq!(structured.plain_text().trim(), "inside");
    assert_eq!(
        structured.unmarked, 7,
        "`outside` is drawn after both closed"
    );
}

/// Past the depth the reader walks, `open_tag` opens nothing and its
/// `close_tag` closes nothing — and the elements below are still closed by
/// their own calls, in order.
///
/// The depth is one less than the reader's cap because the writer puts every
/// page's elements under one `/Document`. This test is what found that the
/// writer's cap used to be the reader's own: an element nested exactly that
/// deep was written and its text came back orphaned under a `DepthCapped`
/// warning — the one thing the cap existed to prevent.
#[test]
fn opens_past_the_depth_cap_are_refused_and_still_paired() {
    let depth = tinker_pdf_cos::limits::MAX_NEST_DEPTH as usize - 1;
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        for _ in 0..depth {
            assert!(page.open_tag(&Tag::new(b"Div")));
        }
        assert!(!page.open_tag(&Tag::new(b"Span")), "past the cap");
        assert!(!page.open_tag(&Tag::new(b"Span")), "still past it");
        page.text(b"F1", 12.0, 20.0, 150.0, "deep");
        assert!(page.close_tag(), "the second refused open");
        assert!(page.close_tag(), "the first");
        for _ in 0..depth {
            assert!(page.close_tag(), "a Div");
        }
        assert!(!page.close_tag());
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    assert_eq!(tree.element_count(), depth + 1, "the Divs and the Document");
    let structured = tree.text_for_page(0, &doc.page(0).expect("a page").text());
    assert_eq!(structured.plain_text().trim(), "deep");
}

/// The catalog's `/Lang` (14.9.2): the language of everything no element
/// states one for.
#[test]
fn the_documents_language_is_written_on_the_catalog() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.set_language("en-GB");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| page.text(b"F1", 12.0, 20.0, 150.0, "colour"));
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let cos = doc.cos();
    let catalog = cos.catalog().expect("a catalog");
    let lang = cos.resolve_key(&catalog, cos.intern(b"Lang"));
    let text = lang
        .as_string()
        .map(|s| tinker_pdf_cos::decode_text_string(&s.bytes));
    assert_eq!(text.as_deref(), Some("en-GB"));
}

// ---- `/Link` with its `/OBJR` (14.7.4.3, 14.8.4.4.2) -----------------------

use tinker_pdf::{ObjRef, Target};

/// The structure tree root's `/ParentTree`, as key to value.
fn parent_tree(doc: &Document) -> Vec<(i64, tinker_pdf::Object)> {
    let cos = doc.cos();
    let catalog = cos.catalog().expect("a catalog");
    let root = cos.resolve_key(&catalog, cos.intern(b"StructTreeRoot"));
    let root = root.as_dict().expect("a structure tree root");
    let tree = root
        .get_ref(cos.intern(b"ParentTree"))
        .expect("an indirect parent tree");
    tinker_pdf_cos::number_tree(cos, tree)
}

/// A page's annotations, by reference, each with its `/StructParent`.
fn annotations(doc: &Document, page: u32) -> Vec<(ObjRef, Option<i64>)> {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages
        .iter()
        .find(|candidate| candidate.index == page)
        .expect("the page");
    let object = cos.get(page.reference).expect("the page object");
    let dict = object.as_dict().expect("a dictionary");
    let annots = cos.resolve_key(dict, cos.intern(b"Annots"));
    let Some(annots) = annots.as_array() else {
        return Vec::new();
    };
    annots
        .iter()
        .map(|entry| {
            let reference = entry.as_objref().expect("an indirect annotation");
            let annotation = cos.resolve(entry);
            let key = annotation
                .as_dict()
                .and_then(|dict| cos.resolve_key(dict, cos.intern(b"StructParent")).as_int());
            (reference, key)
        })
        .collect()
}

/// The object references among an element's kids.
fn objects(element: &StructElement) -> Vec<ObjRef> {
    element
        .kids
        .iter()
        .filter_map(|kid| match kid {
            StructKid::Object(reference) => Some(*reference),
            _ => None,
        })
        .collect()
}

/// **A `/Link` element holds its annotation, and the annotation names the
/// element back.** 14.8.4.4.2's shape: the text the link encloses, and an
/// `/OBJR` to the annotation that makes it go somewhere; 14.7.4.4's other
/// half: the annotation's `/StructParent` is a `/ParentTree` key whose value
/// is that element — a reference, not an array, because an annotation is a
/// content item in its own right.
#[test]
fn a_link_element_holds_its_annotation_and_the_annotation_names_it_back() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "See ");
            page.tagged(b"Link", |page| {
                page.text(b"F1", 12.0, 50.0, 150.0, "the site");
                assert!(page.link(
                    50.0,
                    145.0,
                    100.0,
                    160.0,
                    &Target::Uri("https://example.org/".into())
                ));
            });
        });
        // Outside every element: written as it always was, in no structure.
        assert!(page.link(
            0.0,
            0.0,
            10.0,
            10.0,
            &Target::Uri("https://example.org/b".into())
        ));
    });

    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

    let link = only(&doc, "Link");
    let held = objects(&link);
    let annots = annotations(&doc, 0);
    assert_eq!(annots.len(), 2, "both annotations are written");
    assert_eq!(held, [annots[0].0], "the Link holds the first and only it");
    assert_eq!(annots[1].1, None, "the untagged one has no /StructParent");
    let key = annots[0].1.expect("the held one has a /StructParent");
    assert_eq!(key, 1, "after the one page's key");

    let entry = parent_tree(&doc)
        .into_iter()
        .find(|(k, _)| *k == key)
        .expect("a /ParentTree entry under the annotation's key");
    assert_eq!(
        entry.1.as_objref(),
        link.reference,
        "the entry is a reference to the Link element itself"
    );

    // The join is untouched by the annotation: the text still reads in order.
    let structured = tree.text_for_page(0, &doc.page(0).expect("a page").text());
    assert_eq!(structured.orphans, 0);
    assert_eq!(structured.plain_text().trim(), "See \nthe site");
    assert!(structured.warnings.is_empty(), "{:?}", structured.warnings);
}

/// `link_for` attaches an annotation to the element its key names, drawn
/// before or after, on any page — and a link broken across a page break is
/// one `/Link` holding both annotations, each `/OBJR` naming its own page.
#[test]
fn a_keyed_link_holds_its_annotations_on_both_pages() {
    let uri = Target::Uri("https://example.org/".into());
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    for (at, word) in ["across", "pages"].iter().enumerate() {
        let uri = uri.clone();
        builder.add_page(300.0, 200.0, move |page| {
            // Before the element is drawn on the first page, after on the
            // second: the key, not the call order, decides.
            if at == 0 {
                assert!(page.link_for(7, 20.0, 145.0, 80.0, 160.0, &uri));
            }
            page.tagged_keyed(b"Link", 7, at as u64, |page| {
                page.text(b"F1", 12.0, 20.0, 150.0, word);
            });
            if at == 1 {
                assert!(page.link_for(7, 20.0, 145.0, 80.0, 160.0, &uri));
                assert!(page.link_for(99, 0.0, 0.0, 5.0, 5.0, &uri), "no element");
            }
        });
    }
    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let link = only(&doc, "Link");
    let first = annotations(&doc, 0);
    let second = annotations(&doc, 1);
    assert_eq!(objects(&link), [first[0].0, second[0].0]);
    assert_eq!(second[1].1, None, "a key naming no element: no structure");
    for (key, _) in [first[0], second[0]].map(|(_, key)| (key.expect("a key"), ())) {
        let (_, value) = parent_tree(&doc)
            .into_iter()
            .find(|(k, _)| *k == key)
            .expect("an entry");
        assert_eq!(value.as_objref(), link.reference);
    }
}

/// A `/Link` with no text of its own — a link over a picture drawn
/// elsewhere — is an element holding only its `/OBJR`: kept, with no
/// marked-content id claimed, and its `/OBJR` carrying the `/Pg` the element
/// has no content to lend it.
///
/// This is the case that found `close_marked` taking back the wrong kid: the
/// element's opening sequence was empty when it closed, and the take-back
/// removed the last kid — which was the annotation's — leaving the element
/// claiming an id the page no longer had.
#[test]
fn a_link_with_no_text_still_holds_its_annotation() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"P", |page| page.text(b"F1", 12.0, 20.0, 150.0, "text"));
        page.tagged(b"Link", |page| {
            assert!(page.link(
                0.0,
                0.0,
                10.0,
                10.0,
                &Target::Uri("https://example.org/".into())
            ));
        });
    });
    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let link = only(&doc, "Link");
    assert_eq!(objects(&link), [annotations(&doc, 0)[0].0]);
    assert_eq!(link.kids.len(), 1, "the annotation and nothing else");
    assert_eq!(tree.content_count(), 1, "only the paragraph's sequence");
    let structured = tree.text_for_page(0, &doc.page(0).expect("a page").text());
    assert_eq!(structured.orphans, 0);
    assert!(structured.warnings.is_empty(), "{:?}", structured.warnings);
}

/// An annotation `finish` does not write — a link naming a destination that
/// never arrives — leaves no `/OBJR`, takes no key, and the element that held
/// it keeps its text.
#[test]
fn a_link_that_is_not_written_leaves_no_object_reference() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"Link", |page| {
            page.text(b"F1", 12.0, 20.0, 150.0, "nowhere");
            assert!(page.link(
                20.0,
                145.0,
                80.0,
                160.0,
                &Target::Named(b"never-registered".to_vec())
            ));
        });
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    assert_eq!(tree.object_count(), 0);
    assert!(annotations(&doc, 0).is_empty());
    assert_eq!(
        parent_tree(&doc).len(),
        1,
        "the page's key and nothing else"
    );
    assert_eq!(only(&doc, "Link").kids.len(), 1, "its text");
}

// ---- `/RoleMap` (14.7.3) ---------------------------------------------------

/// A custom type is written as itself and read as the type it maps to — both
/// names kept, which is the reader's `raw_type`/`standard_type` pair — and a
/// mapping may go through another custom type (ISO 14289-1 7.1).
#[test]
fn a_custom_type_is_written_as_itself_and_read_through_the_role_map() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.map_role(b"Chapitre", b"Sect"));
    assert!(builder.map_role(b"aside", b"sidebar"));
    assert!(builder.map_role(b"sidebar", b"Note"));
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"Chapitre", |page| {
            page.tagged(b"aside", |page| page.text(b"F1", 12.0, 20.0, 150.0, "x"));
        });
    });
    let bytes = builder.finish();
    structurally_clean(bytes.clone());
    let doc = Document::open(bytes).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    let chapter = only(&doc, "Sect");
    assert_eq!(chapter.raw_type, "Chapitre");
    let aside = only(&doc, "Note");
    assert_eq!(aside.raw_type, "aside", "two hops, both followed");
}

/// What `map_role` refuses, each for the reason a reader could not use it.
#[test]
fn a_role_map_entry_a_reader_could_not_use_is_refused() {
    let mut builder = DocumentBuilder::new();
    // ISO 14289-1 7.1: standard tags shall not be remapped.
    assert!(!builder.map_role(b"P", b"Div"));
    assert!(!builder.map_role(b"Figure", b"Span"));
    assert!(!builder.map_role(b"", b"P"), "an empty name");
    assert!(!builder.map_role(b"x", b""), "an empty target");
    assert!(!builder.map_role(b"x", b"x"), "a name mapped to itself");
    assert!(builder.map_role(b"a", b"b"));
    assert!(builder.map_role(b"b", b"c"));
    assert!(!builder.map_role(b"c", b"a"), "a loop through two entries");
    assert!(
        !builder.map_role(b"a", b"Span"),
        "the first statement stands"
    );
    assert!(builder.map_role(b"a", b"b"), "and restating it is accepted");
    assert!(
        builder.map_role(b"c", b"P"),
        "the chain ends at a standard type"
    );

    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(300.0, 200.0, |page| {
        page.tagged(b"a", |page| page.text(b"F1", 12.0, 20.0, 150.0, "x"));
    });
    let doc = Document::open(builder.finish()).expect("opens");
    let tree = doc.structure().expect("a tree");
    assert!(tree.warnings.is_empty(), "no loop reached the file");
    assert_eq!(only(&doc, "P").raw_type, "a");
}

/// A role map is part of a structure tree, and a document without one gains
/// no `/RoleMap` however many mappings were registered.
#[test]
fn a_role_map_without_a_tree_is_not_written() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.map_role(b"Chapitre", b"Sect"));
    builder.add_page(300.0, 200.0, |page| {
        page.text(b"F1", 12.0, 20.0, 150.0, "plain");
    });
    let bytes = builder.finish();
    assert!(!bytes.windows(7).any(|window| window == b"RoleMap"));
}

/// The shape check the `lang` documentation sends a caller to.
#[test]
fn a_language_tag_has_the_shape_bcp_47_gives_one() {
    for good in ["en", "fr-CA", "zh-Hant-TW", "x-klingon", "ru-petr1708", ""] {
        assert!(tinker_pdf::is_language_tag(good), "{good:?}");
    }
    for bad in [
        "e n",
        "en-",
        "-en",
        "1en",
        "toolongtag",
        "en-toolongsub",
        "\u{430}\u{43D}",
    ] {
        assert!(!tinker_pdf::is_language_tag(bad), "{bad:?}");
    }
}
