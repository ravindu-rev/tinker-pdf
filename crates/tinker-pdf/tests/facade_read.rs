//! The facade's read surface for fonts, layers and annotations, exercised the
//! way a caller reaches it (ruling 11).
//!
//! The unit tests beside `src/fontlist.rs`, `src/layers.rs` and
//! `src/annotations.rs` can reach `pub(crate)` entry points and private
//! helpers; these cannot, and that is the point. Ruling 11 makes the facade
//! the only public surface, so the contract is what `Document::fonts`,
//! `Document::layers` and `Page::annotations` return to a crate outside — and
//! a projection that compiles inside the crate but cannot be *named* outside
//! it would pass every unit test and still be unusable.
//!
//! Every test here is an injection guard: each one fails if one specific
//! defect is put back. They are named for the defect, not for the feature, so
//! a failure says which one returned:
//!
//! | Defect | Guard |
//! | --- | --- |
//! | a font reported embedded when it is not | `a_dangling_program_is_not_embedded` |
//! | the subset tag left on the reported name | `a_subset_tag_is_stripped_and_kept_beside_the_name` |
//! | a layer's default visibility inverted | `a_layers_visibility_is_the_default_configurations` |
//! | `/Contents` read through `/Popup` | `a_notes_contents_is_its_own_not_its_popups` |
//! | a subtype the model does not cover, dropped | `an_unmodelled_subtype_is_named_not_dropped` |
//! | `annotations()` answering widgets only | `annotations_are_not_only_widgets` |
//!
//! One fixture serves all six. Three surfaces over one document is also the
//! thing worth checking: they read the same file through the same reader, and
//! a change that made one of them rebuild the document would show up here as
//! the others disagreeing with it.

use tinker_pdf::{AnnotationKind, Document, FontKind, ProgramKey};

/// A one-page document carrying two fonts, two layers and five annotations.
///
/// Every value in it is chosen so that a wrong answer is a *different* answer
/// rather than a missing one: the pop-up's text differs from its parent's, the
/// hidden layer is the second and not the first, and the font that is not
/// embedded has a descriptor that claims it is.
fn fixture() -> Document {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties \
<< /OCGs [10 0 R 11 0 R] /D << /OFF [11 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
/Resources << /Font << /F1 6 0 R /F2 9 0 R >> >> \
/Annots [20 0 R 21 0 R 22 0 R 23 0 R 24 0 R] >>\nendobj\n\
6 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Arial \
/FontDescriptor 7 0 R >>\nendobj\n\
7 0 obj\n<< /Type /FontDescriptor /FontName /ABCDEF+Arial /FontFile2 8 0 R >>\nendobj\n\
8 0 obj\n<< /Length 8 >>\nstream\nglyphsxx\nendstream\nendobj\n\
9 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
/FontDescriptor 30 0 R >>\nendobj\n\
30 0 obj\n<< /Type /FontDescriptor /FontName /Helvetica /FontFile3 31 0 R >>\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (Visible layer) >>\nendobj\n\
11 0 obj\n<< /Type /OCG /Name (Hidden layer) >>\nendobj\n\
20 0 obj\n<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] \
/Contents (the note itself) /T (Ada) /M (D:20260915120000Z) /Popup 21 0 R >>\nendobj\n\
21 0 obj\n<< /Type /Annot /Subtype /Popup /Rect [30 10 90 60] \
/Contents (the window, not the note) /T (Nobody) /Parent 20 0 R >>\nendobj\n\
22 0 obj\n<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] /T (a field) >>\nendobj\n\
23 0 obj\n<< /Type /Annot /Subtype /Square /Rect [40 40 60 60] >>\nendobj\n\
24 0 obj\n<< /Type /Annot /Subtype /LiveFormXfa /Rect [0 0 1 1] >>\nendobj\n\
trailer\n<< /Size 40 /Root 1 0 R >>\n%%EOF\n"
        .to_vec();
    Document::open(bytes).expect("the fixture opens")
}

/// 9.9: a `/FontFile3` that names no stream is a descriptor claiming a program
/// the file does not carry.
///
/// The injection is reporting that font as embedded. A caller extracting every
/// embedded face would then get an entry whose bytes are always `None`, and a
/// self-containment check would believe a file complete that is not.
#[test]
fn a_dangling_program_is_not_embedded() {
    let fonts = fixture().fonts();
    assert_eq!(fonts.len(), 2, "two fonts on the page's resources");

    let arial = fonts.iter().find(|f| f.name == "Arial").expect("Arial");
    assert!(arial.is_embedded(), "/FontFile2 8 0 R is a stream");
    assert_eq!(arial.kind, FontKind::TrueType);
    assert_eq!(arial.program.expect("a program").key, ProgramKey::FontFile2);
    assert_eq!(arial.program_bytes().as_deref(), Some(&b"glyphsxx"[..]));

    let helvetica = fonts
        .iter()
        .find(|f| f.name == "Helvetica")
        .expect("Helvetica");
    assert!(
        !helvetica.is_embedded(),
        "a /FontFile3 pointing at no object is not an embedded program"
    );
    assert_eq!(helvetica.program, None);
    assert_eq!(helvetica.program_bytes(), None);
}

/// 9.6.4: six upper-case letters and a `+` are a subset tag, and the name a
/// person means is what follows it.
///
/// The injection is leaving the tag on. `ABCDEF+Arial` matches no face on any
/// host and no other tool reports a font that way; the untouched `/BaseFont`
/// stays beside it because a caller writing the file back out needs the exact
/// bytes.
#[test]
fn a_subset_tag_is_stripped_and_kept_beside_the_name() {
    let fonts = fixture().fonts();
    let arial = fonts.iter().find(|f| f.name == "Arial").expect("Arial");

    assert_eq!(arial.name, "Arial", "the tag is off the reported name");
    assert_eq!(
        arial.base_font, "ABCDEF+Arial",
        "and the file's own spelling"
    );
    assert_eq!(arial.subset_tag.as_deref(), Some("ABCDEF"));
    assert_eq!(arial.resource_names, vec!["F1"]);

    let helvetica = fonts
        .iter()
        .find(|f| f.name == "Helvetica")
        .expect("Helvetica");
    assert_eq!(helvetica.subset_tag, None, "an untagged name has no tag");
    assert_eq!(helvetica.base_font, "Helvetica");
}

/// 8.11.4.3 Table 101: `/OFF` names the groups the default configuration
/// hides.
///
/// The injection is inverting it — reporting a group `/OFF` names as visible.
/// The list and the rendered page would then disagree about the same document,
/// which is the one thing a layer list must never do.
#[test]
fn a_layers_visibility_is_the_default_configurations() {
    let layers = fixture().layers();
    assert_eq!(layers.len(), 2, "both groups /OCGs lists");

    assert_eq!(layers[0].name, "Visible layer");
    assert!(layers[0].visible, "a group /OFF does not name stays on");
    assert_eq!(layers[1].name, "Hidden layer");
    assert!(!layers[1].visible, "/OFF names this one");

    assert_eq!(layers[0].reference.num, 10, "the order is /OCGs' own");
    assert_eq!(layers[1].reference.num, 11);
}

/// 12.5.6.14 Table 183 runs one way: the parent's entries override the
/// pop-up's, never the reverse.
///
/// The injection is a markup annotation reading its `/Contents` through its
/// `/Popup`. The fixture's pop-up says something different on purpose, so the
/// wrong reading produces the wrong text rather than an empty one — which is
/// what the mistake usually looks like in the wild, since most pop-ups carry
/// no `/Contents` at all.
#[test]
fn a_notes_contents_is_its_own_not_its_popups() {
    let document = fixture();
    let page = document.page(0).expect("a page");
    let annotations = page.annotations();

    let note = &annotations[0];
    assert_eq!(note.kind, AnnotationKind::Text);
    assert_eq!(
        note.contents.as_deref(),
        Some("the note itself"),
        "reading through /Popup would say `the window, not the note`"
    );
    assert_eq!(note.title.as_deref(), Some("Ada"));
    assert_eq!(note.popup.expect("a /Popup").num, 21);
    assert_eq!(note.modified_date.expect("a /M date").year, 2026);

    let popup = &annotations[1];
    assert_eq!(popup.kind, AnnotationKind::Popup);
    assert_eq!(
        popup.contents.as_deref(),
        Some("the note itself"),
        "12.5.6.14: the parent's /Contents overrides the pop-up's own"
    );
    assert_eq!(popup.title.as_deref(), Some("Ada"));
    assert_eq!(popup.parent.expect("a /Parent").num, 20);
}

/// A `/Subtype` no edition of ISO 32000 defines is named and counted, not
/// dropped.
///
/// The injection is dropping it. The roadmap row asks for the refused ones
/// counted **by subtype**, and a list that silently shortened could answer
/// neither the count nor the name — a caller auditing a file would be left
/// comparing lengths against the raw `/Annots` array to find out what went
/// missing.
#[test]
fn an_unmodelled_subtype_is_named_not_dropped() {
    let document = fixture();
    let page = document.page(0).expect("a page");
    let annotations = page.annotations();

    assert_eq!(annotations.len(), 5, "one entry out per /Annots entry in");

    let unknown = annotations.last().expect("the last entry");
    assert_eq!(
        unknown.kind,
        AnnotationKind::Other("LiveFormXfa".to_string())
    );
    assert!(!unknown.kind.is_covered(), "and it says it is not covered");
    assert_eq!(unknown.kind.as_name(), "LiveFormXfa", "by name");

    // The count the census takes, taken here on a fixture whose answer is
    // known: four covered, one refused.
    let refused = annotations.iter().filter(|a| !a.kind.is_covered()).count();
    assert_eq!(refused, 1);
}

/// `Page::annotations` is every entry of `/Annots`, not the widgets.
///
/// The injection is filtering to `/Widget`. Before this method the facade's
/// page-level views were `links()` and the form field tree, so a `/Square` on
/// a page with a form was invisible from outside the crate — which is exactly
/// what a widget-only list would restore while still looking like it worked on
/// every form.
#[test]
fn annotations_are_not_only_widgets() {
    let document = fixture();
    let page = document.page(0).expect("a page");
    let kinds: Vec<AnnotationKind> = page.annotations().into_iter().map(|a| a.kind).collect();

    assert_eq!(
        kinds,
        vec![
            AnnotationKind::Text,
            AnnotationKind::Popup,
            AnnotationKind::Widget,
            AnnotationKind::Square,
            AnnotationKind::Other("LiveFormXfa".to_string()),
        ],
        "every subtype on the page, in the array's own order"
    );

    // `links()` stays the narrower navigation view over the same array and is
    // unchanged by any of this: this page has no `/Link`, and it says so.
    assert!(page.links().is_empty());
}

/// Reading the document does not change what it reports about itself.
///
/// All three methods are reads. If any of them ran a reader that absorbs its
/// leniencies into the document's warnings, asking twice would differ from
/// asking once — and a caller that listed fonts before checking warnings would
/// get a different verdict from one that checked first.
#[test]
fn the_read_surface_adds_no_warnings() {
    let document = fixture();
    let before = document.warnings().len();

    let _ = document.fonts();
    let _ = document.layers();
    let _ = document.page(0).expect("a page").annotations();

    assert_eq!(
        document.warnings().len(),
        before,
        "a read surface that mutates the thing it reads is not a read surface"
    );
}
