//! Tagged PDF, from the `BDC` to the reading order (14.6.2, 14.7, 14.9).
//!
//! Everything here is built from bytes rather than from `testdata/`, whose
//! four files are mutool-written, are not to be modified, and carry no
//! structure tree anyway.
//!
//! **Every fixture draws its content in an order the structure tree
//! contradicts.** A page whose stream order and structure order agree is one a
//! build that ignores the structure tree entirely passes, and this whole file
//! exists to tell those two builds apart.

use tinker_pdf::{Document, StructureWarning, TextSource};

/// A one-page document with a structure tree.
///
/// `catalog` supplies whatever goes beside `/StructTreeRoot 5 0 R`,
/// `structure` the tree root's body, `content` the page's content stream, and
/// `objects` everything the tree points at, numbered from 10 by convention.
/// The page is 100×60 with `/StructParents 0` and a Helvetica at `/F0`.
fn build(catalog: &str, structure: &str, content: &str, objects: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R {catalog} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 60] /StructParents 0\n\
   /Resources << /Font << /F0 4 0 R >> /Properties << /MC9 9 0 R >> >>\n\
   /Contents 6 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
5 0 obj\n<< /Type /StructTreeRoot {structure} >>\nendobj\n\
6 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
9 0 obj\n<< /MCID 4 /Alt (a named list) /Lang (cy) >>\nendobj\n\
{objects}\
trailer\n<< /Size 400 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

fn page(bytes: Vec<u8>) -> tinker_pdf::Page {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
}

/// One marked sequence per line, at descending baselines.
fn marked(mcid: u32, y: u32, text: &str) -> String {
    format!(
        "/P << /MCID {mcid} >> BDC BT /F0 12 Tf 5 {y} Td ({text}) Tj ET EMC\n"
    )
}

/// **The milestone's first exit criterion.** Content-stream order and
/// structure order disagree, and the structured view follows the structure.
///
/// The stream draws `beta` above `alpha`; the tree says the paragraph holding
/// `alpha` comes first. Flat extraction reports the stream and the structured
/// view reports the tree, and asserting both is the point: a build that
/// returned the flat answer from `structured_text` passes any test that only
/// checks the structured one against a page whose two orders agree.
#[test]
fn a_page_extracts_in_structure_order_and_flatly_in_stream_order() {
    let content = format!("{}{}", marked(0, 40, "beta"), marked(1, 20, "alpha"));
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [12 0 R 11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /P /Pg 3 0 R /K [1] >>\nendobj\n",
    ));

    assert_eq!(
        page.text().plain_text(),
        "beta\nalpha\n",
        "flat extraction is the content stream's order, unchanged"
    );
    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(
        structured.plain_text(),
        "alpha\nbeta\n",
        "and the structured view is the tree's"
    );
    assert_eq!(structured.orphans, 0);
    assert_eq!(structured.unmarked, 0);
    assert_eq!(structured.matched, 9, "`alpha` and `beta`");
    assert!(structured.warnings.is_empty(), "{:?}", structured.warnings);
}

/// An element's content and a child element's content interleave, and reading
/// order runs *through* the child rather than around it.
///
/// `/P [ 0 /Span[1] 2 ]` reads 0, 1, 2. A view that reported one node per
/// element would have to put the span's words after the paragraph's, which is
/// a reordering in the middle of a sentence and reads as a layout opinion
/// rather than as a bug — so the paragraph produces two runs and the span one
/// between them.
#[test]
fn a_child_element_reads_in_the_middle_of_its_parents_content() {
    let content = format!(
        "{}{}{}",
        marked(0, 40, "one "),
        marked(1, 40, "two "),
        marked(2, 40, "three")
    );
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0 12 0 R 2] >>\nendobj\n\
         12 0 obj\n<< /S /Span /Pg 3 0 R /K [1] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    let shape: Vec<(&str, &str)> = structured
        .nodes
        .iter()
        .map(|n| (n.standard_type.as_str(), n.text.as_str()))
        .collect();
    assert_eq!(
        shape,
        vec![("P", "one "), ("Span", "two "), ("P", "three")],
        "three runs, and the span sits between the paragraph's two"
    );
}

/// 14.9.4: `/ActualText` on a structure element replaces everything it
/// encloses, and what it replaced does not become an orphan.
///
/// The element spells the ligature the page draws as `ffi`. Both halves
/// matter: a build that emitted the replacement *and* the glyphs says `ffi`
/// twice, and a build that skipped the subtree without claiming it reports its
/// characters as content nothing accounts for.
#[test]
fn actual_text_replaces_what_it_encloses_without_orphaning_it() {
    let content = format!("{}{}", marked(0, 40, "o"), marked(1, 20, "tail"));
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /Span /Pg 3 0 R /ActualText (ffi) /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /P /Pg 3 0 R /K [1] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "ffi\ntail\n");
    assert_eq!(
        structured.nodes[0].source,
        TextSource::ActualText,
        "and the view says the text is not the glyphs"
    );
    assert!(
        structured.nodes[0].chars.is_empty(),
        "there are no glyphs behind an /ActualText to select"
    );
    assert_eq!(
        structured.orphans, 0,
        "the replaced sequence was accounted for, not lost"
    );
}

/// 14.9.4 again, on the *property list* rather than on the element.
///
/// A producer may write the replacement either place and 14.9 treats them
/// alike, so this engine has to as well — which is only possible because the
/// property list crosses the `Device` seam.
#[test]
fn actual_text_on_a_property_list_replaces_its_own_sequence() {
    let content = "/Span << /MCID 0 /ActualText (fi) >> BDC \
                   BT /F0 12 Tf 5 40 Td (o) Tj ET EMC\n"
        .to_string()
        + &marked(1, 20, "tail");
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /Span /Pg 3 0 R /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /P /Pg 3 0 R /K [1] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "fi\ntail\n");
    assert_eq!(structured.nodes[0].source, TextSource::ActualText);
    assert_eq!(
        page.text().plain_text(),
        "o\ntail\n",
        "and flat extraction still reports the glyphs, unchanged"
    );
}

/// 14.9.3: a `Figure` describes content that draws no glyphs, and the
/// description surfaces anyway.
///
/// The figure's marked sequence contains a filled rectangle and nothing else,
/// so a view that only emitted nodes with text would report nothing at all for
/// it — which is exactly the content a reader most needs the description of.
#[test]
fn a_figures_alt_text_surfaces_with_no_glyphs_behind_it() {
    let content = "/Figure << /MCID 0 >> BDC 10 10 30 30 re f EMC\n".to_string()
        + &marked(1, 20, "caption");
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /Figure /Pg 3 0 R /Alt (a photograph of a cat) /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /Caption /Pg 3 0 R /K [1] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    let figure = structured
        .nodes
        .iter()
        .find(|n| n.standard_type == "Figure")
        .expect("the figure has a node of its own");
    assert_eq!(figure.alt.as_deref(), Some("a photograph of a cat"));
    assert!(figure.text.is_empty(), "and no text was invented for it");
    assert_eq!(structured.plain_text(), "caption\n");
}

/// 14.9.5's `/E` and 14.9.2's `/Lang`, arriving through the *named* property
/// list rather than from the element.
#[test]
fn a_named_property_list_supplies_the_accessibility_values() {
    let content = "/Span /MC9 BDC BT /F0 12 Tf 5 40 Td (BSI) Tj ET EMC\n";
    let page = page(build(
        "",
        "/K 10 0 R",
        content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /Span /Pg 3 0 R /K [4] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "BSI\n");
    assert_eq!(structured.nodes[0].alt.as_deref(), Some("a named list"));
    assert_eq!(structured.nodes[0].lang.as_deref(), Some("cy"));
}

/// Characters the structure tree does not claim are **counted**, not
/// appended — and untagged characters are counted separately again.
///
/// Three sources on one page: a sequence the tree claims, a sequence with an
/// identifier no element names, and text drawn outside every `BDC`. Appending
/// either of the last two would make the structured view a superset of the
/// tagged content that reads like reading order and is not.
#[test]
fn unclaimed_content_is_counted_and_never_appended() {
    let content = format!(
        "{}{}BT /F0 12 Tf 5 5 Td (loose) Tj ET\n",
        marked(0, 45, "kept"),
        marked(7, 25, "lost")
    );
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "kept\n");
    assert_eq!(structured.matched, 4);
    assert_eq!(structured.orphans, 4, "`lost` carries an unclaimed /MCID");
    assert_eq!(structured.unmarked, 5, "`loose` carries none at all");
    assert_eq!(
        page.text().plain_text(),
        "kept\nlost\nloose\n",
        "and the flat view still has all three"
    );
}

/// 14.7.4.4: the `/ParentTree` and the `/K` walk disagree, the walk wins, and
/// the disagreement is a warning naming the page and both counts.
///
/// The parent tree claims three content items for the page; the tree names
/// two. A build that trusted the parent tree would report a paragraph nobody
/// wrote, and one that read neither would report nothing at all — so the
/// warning has to carry the numbers, not just the fact.
#[test]
fn a_parent_tree_disagreement_is_a_warning_and_the_walk_wins() {
    let content = format!("{}{}", marked(0, 40, "one"), marked(1, 20, "two"));
    let page = page(build(
        "",
        "/K 10 0 R /ParentTree 20 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /P /Pg 3 0 R /K [1] >>\nendobj\n\
         20 0 obj\n<< /Nums [0 [11 0 R 12 0 R 13 0 R]] >>\nendobj\n\
         13 0 obj\n<< /S /P /Pg 3 0 R /K [2] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(
        structured.plain_text(),
        "one\ntwo\n",
        "the /K walk decided what the page says"
    );
    assert_eq!(
        structured.warnings,
        vec![StructureWarning::ParentTreeDisagreement {
            page: 0,
            walk: 2,
            parent_tree: 3,
        }],
    );
}

/// And a `/ParentTree` that agrees says nothing, which is what makes the
/// warning above evidence about the file rather than about the check.
#[test]
fn a_parent_tree_that_agrees_is_silent() {
    let content = format!("{}{}", marked(0, 40, "one"), marked(1, 20, "two"));
    let page = page(build(
        "",
        "/K 10 0 R /ParentTree 20 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /P /Pg 3 0 R /K [1] >>\nendobj\n\
         20 0 obj\n<< /Nums [0 [11 0 R 12 0 R]] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert!(structured.warnings.is_empty(), "{:?}", structured.warnings);
}

/// 14.8.2.2: an `/Artifact` written with an *inline* property list is an
/// artifact, and is excluded from the text.
///
/// **This is a behaviour change, and it is the point.** `BDC` takes
/// `tag properties`, and the tag sits before the `<<`; the two-token peek this
/// build used before gap 14 read the flattened dictionary's last *value*
/// instead, so a running head written `/Artifact << /Type /Pagination >> BDC`
/// reported its tag as `Pagination` and was extracted as though the author had
/// written it. Only the reassembly milestone 2 restores can find the `<<`.
#[test]
fn an_artifact_with_an_inline_property_list_is_excluded_from_the_text() {
    let content = "/Artifact << /Type /Pagination /BBox [0 50 100 60] >> BDC \
                   BT /F0 12 Tf 5 50 Td (page 7) Tj ET EMC\n"
        .to_string()
        + &marked(0, 20, "body");
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n",
    ));

    assert_eq!(
        page.text().plain_text(),
        "body\n",
        "the running head is drawn and not read (14.8.2.2)"
    );
    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "body\n");
    assert_eq!(
        structured.unmarked, 0,
        "an excluded artifact is not untagged content; it is not content"
    );
}

/// A `BMC` scope, which carries no property list at all, leaves its
/// characters untagged rather than attaching them to whatever `/MCID` was last
/// seen.
#[test]
fn a_bare_bmc_leaves_its_characters_unmarked() {
    let content = format!(
        "{}/Span BMC BT /F0 12 Tf 5 20 Td (plain) Tj ET EMC\n",
        marked(0, 40, "tagged")
    );
    let page = page(build(
        "",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n",
    ));

    let structured = page.structured_text().expect("a structure tree");
    assert_eq!(structured.plain_text(), "tagged\n");
    assert_eq!(structured.unmarked, 5, "`plain`");
    assert_eq!(structured.orphans, 0);
}

/// The whole surface, on a document that has no structure tree: `None`, twice,
/// and the flat text unchanged.
#[test]
fn an_untagged_document_reports_no_structure_rather_than_an_empty_one() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/simple-text.pdf"),
    )
    .expect("the fixture is committed");
    let doc = Document::open(bytes).expect("it opens");

    assert!(doc.structure().is_none());
    let page = doc.page(0).expect("a page");
    assert!(page.structured_text().is_none());
    assert!(
        !page.text().plain_text().is_empty(),
        "and it still has text"
    );
}

/// `Document::structure` reports the tree's own counts, which is what the
/// corpus census in `tagged_corpus.rs` aggregates.
#[test]
fn the_tree_counts_its_three_kinds_of_kid() {
    let content = marked(0, 40, "body");
    let doc = Document::open(build(
        "/MarkInfo << /Marked true /Suspects false >>",
        "/K 10 0 R",
        &content,
        "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R 12 0 R] >>\nendobj\n\
         11 0 obj\n<< /S /P /Pg 3 0 R /K [0] >>\nendobj\n\
         12 0 obj\n<< /S /Link /Pg 3 0 R /K [<< /Type /OBJR /Obj 99 0 R >>] >>\nendobj\n",
    ))
    .expect("it opens");

    let tree = doc.structure().expect("a structure tree");
    assert!(tree.marked);
    assert!(!tree.suspects);
    assert_eq!(tree.element_count(), 3);
    assert_eq!(tree.content_count(), 1);
    assert_eq!(tree.object_count(), 1);
    assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
}
