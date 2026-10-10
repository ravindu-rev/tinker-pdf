//! Replacing a document-level structure on a file whose old one is damaged
//! or written in a shape the setters did not expect (tier 5, "Document
//! operations").
//!
//! `set_outline`, `set_page_labels` and `attach_file` delete the nodes of the
//! structure they replace, because an old outline's titles are content. The
//! old structure is read from the file, so its links are whatever a producer
//! wrote: a last item whose `/Next` names an object that is not there — the
//! number the editor hands out next — or a node whose link leads into the
//! page tree. What is deleted must be the old structure's own nodes, and only
//! those nothing reaches afterwards. And a tree written directly into the
//! catalog is still a tree, with nodes to delete and entries a new attachment
//! files beside.
//!
//! The last test is the dates these setters write: a caller's UTC offset is
//! any `i32`, and one no zone has is refused like any other date 7.9.4 cannot
//! spell, `i32::MIN` included.

use std::sync::Arc;

use tinker_pdf_cos::{
    attachments, outline, page_labels, pages, validate, AttachError, CosDocument, Date, Dict,
    DocumentEditor, EmbeddedFile, LabelStyle, Name, ObjRef, Object, OutlineEntry, PageLabelRange,
    WriteMode, WriteOptions,
};

const MODES: [WriteMode; 2] = [WriteMode::Incremental, WriteMode::Rewrite];

/// Two pages and whatever `extra` objects and catalog entries a test adds.
///
/// Hand-written bytes carry no cross-reference table, so they open through
/// the repair scanner; a rewrite gives the same graph, at the same object
/// numbers, inside a well-formed file an incremental save can append to. The
/// trailer names its `/Info` (object 5) because, when it names none, the
/// scanner synthesises one from a dictionary carrying a 14.3.3 key — and an
/// outline item's `/Title` is one, which would keep an old item alive as the
/// document information dictionary.
fn document(catalog: &str, extra: &str) -> Arc<CosDocument> {
    let text = format!(
        "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R {catalog} >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>
endobj
4 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>
endobj
5 0 obj
<< /Producer (by hand) >>
endobj
{extra}
trailer
<< /Root 1 0 R /Info 5 0 R >>
%%EOF
"
    );
    let raw = Arc::new(CosDocument::open(text.into_bytes()).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// Saves `editor` in `mode`, reopens it, and holds it to the strict
/// validator.
#[track_caller]
fn saved(editor: &DocumentEditor, mode: WriteMode) -> CosDocument {
    let bytes = editor.save(&WriteOptions {
        mode,
        ..WriteOptions::default()
    });
    let reopened = CosDocument::open(bytes).expect("the save reopens");
    let defects = validate(&reopened);
    assert!(defects.is_empty(), "{mode:?}: {defects:?}");
    reopened
}

fn entry(title: &str) -> OutlineEntry {
    OutlineEntry {
        title: title.to_owned(),
        target: None,
        open: false,
        children: Vec::new(),
    }
}

// ---- the outline ------------------------------------------------------------

/// The old outline's last item names `52 0 R`, which the file does not have
/// — and which is the first number the editor allocates, so it is the new
/// outline's root. Replacing the outline must not delete it.
#[test]
fn a_dangling_next_does_not_delete_the_new_outline() {
    let doc = document(
        "/Outlines 50 0 R",
        "50 0 obj
<< /Type /Outlines /First 51 0 R /Last 51 0 R /Count 1 >>
endobj
51 0 obj
<< /Title (One) /Parent 50 0 R /Next 52 0 R >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    let first = {
        let mut probe = DocumentEditor::new(Arc::clone(&editor.shared_document()));
        probe.allocate()
    };
    assert_eq!(first, ObjRef::new(52, 0), "the premise");
    assert!(editor.set_outline(&[entry("New")]));
    assert!(
        editor.get(ObjRef::new(51, 0)) == Some(Object::Null),
        "the old item is deleted"
    );
    for mode in MODES {
        let after = saved(&editor, mode);
        let titles: Vec<String> = outline(&after).into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["New"], "{mode:?}");
    }
}

/// The same dangling `/Next`, but an earlier edit has already given `52` to
/// a page it inserted — one only the editor's pending page order holds until
/// a save writes it into `/Kids`. The page is in the document, so replacing
/// the outline leaves it.
#[test]
fn a_dangling_next_does_not_delete_a_page_inserted_before() {
    let doc = document(
        "/Outlines 50 0 R",
        "50 0 obj
<< /Type /Outlines /First 51 0 R /Last 51 0 R /Count 1 >>
endobj
51 0 obj
<< /Title (One) /Parent 50 0 R /Next 52 0 R >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    let page = editor.insert_page(2, 300.0, 300.0).expect("inserted");
    assert_eq!(page, ObjRef::new(52, 0), "the premise");
    assert!(editor.set_outline(&[entry("New")]));
    assert_eq!(editor.get(ObjRef::new(51, 0)), Some(Object::Null));
    assert!(
        editor
            .get(page)
            .is_some_and(|page| page.as_dict().is_some()),
        "the inserted page is still there"
    );
    for mode in MODES {
        let after = saved(&editor, mode);
        let widths: Vec<f64> = pages::collect(&after)
            .iter()
            .map(|p| p.media_box.x1 - p.media_box.x0)
            .collect();
        assert_eq!(widths, [200.0, 200.0, 300.0], "{mode:?}");
    }
}

/// And once more with `52` given to an object the caller has put and not yet
/// linked into anything — an annotation it is about to add to a page.
/// Nothing reaches it yet, but it is no outline item (it has a `/Type`, which
/// Table 153 gives none), so it is not the old outline's to delete.
#[test]
fn a_dangling_next_does_not_delete_an_object_the_caller_has_not_linked_yet() {
    let doc = document(
        "/Outlines 50 0 R",
        "50 0 obj
<< /Type /Outlines /First 51 0 R /Last 51 0 R /Count 1 >>
endobj
51 0 obj
<< /Title (One) /Parent 50 0 R /Next 52 0 R >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    let pending = editor.allocate();
    assert_eq!(pending, ObjRef::new(52, 0), "the premise");
    let mut annot = Dict::new();
    annot.insert(Name::TYPE, Object::Name(editor.intern(b"Annot")));
    annot.insert(
        editor.intern(b"Subtype"),
        Object::Name(editor.intern(b"Square")),
    );
    editor.put(pending, Object::Dict(annot.clone()));
    assert!(editor.set_outline(&[entry("New")]));
    assert_eq!(editor.get(ObjRef::new(51, 0)), Some(Object::Null));
    assert_eq!(editor.get(pending), Some(Object::Dict(annot)));
}

/// The old outline's item says its `/Next` is page 4. The page is not an
/// outline item, and the document still reaches it: it stays.
#[test]
fn a_malformed_next_into_a_page_does_not_delete_the_page() {
    let doc = document(
        "/Outlines 50 0 R",
        "50 0 obj
<< /Type /Outlines /First 51 0 R /Last 51 0 R /Count 1 >>
endobj
51 0 obj
<< /Title (One) /Parent 50 0 R /Next 4 0 R >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    assert!(editor.set_outline(&[entry("New")]));
    assert!(
        editor
            .get(ObjRef::new(4, 0))
            .is_some_and(|page| page.as_dict().is_some()),
        "the page is still there"
    );
    for mode in MODES {
        let after = saved(&editor, mode);
        assert_eq!(pages::collect(&after).len(), 2, "{mode:?}");
    }
}

/// An outline written directly into the catalog — 7.7.2 says it shall be
/// indirect, and a reader takes it either way — is replaced like any other,
/// and its items are deleted, not left in the file.
#[test]
fn a_direct_outline_root_still_has_its_items_deleted() {
    let doc = document(
        "/Outlines << /Type /Outlines /First 51 0 R /Last 51 0 R /Count 1 >>",
        "51 0 obj
<< /Title (Old) >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    assert!(editor.set_outline(&[entry("New")]));
    assert_eq!(editor.get(ObjRef::new(51, 0)), Some(Object::Null));
}

// ---- page labels ------------------------------------------------------------

/// An old label tree whose `/Kids` names the page tree's root. Following
/// `/Kids` from there reaches every page; none of them is a label tree
/// node, and the document still reaches every one.
#[test]
fn a_label_tree_whose_kids_reach_the_page_tree_leaves_the_pages() {
    let doc = document(
        "/PageLabels 60 0 R",
        "60 0 obj
<< /Kids [2 0 R 61 0 R] >>
endobj
61 0 obj
<< /Nums [0 << /S /D >>] >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    let ranges = [PageLabelRange {
        first_page: 0,
        style: LabelStyle::RomanLower,
        prefix: None,
        start: 1,
    }];
    editor.set_page_labels(&ranges).expect("the labels are set");
    assert_eq!(editor.get(ObjRef::new(61, 0)), Some(Object::Null));
    for mode in MODES {
        let after = saved(&editor, mode);
        assert_eq!(pages::collect(&after).len(), 2, "{mode:?}");
        assert_eq!(page_labels(&after, 2), ["i", "ii"], "{mode:?}");
    }
}

/// A label tree written directly into the catalog: replacing it deletes its
/// leaves, which nothing reaches afterwards.
#[test]
fn a_direct_label_tree_still_has_its_leaves_deleted() {
    let doc = document(
        "/PageLabels << /Kids [61 0 R] >>",
        "61 0 obj
<< /Limits [0 0] /Nums [0 << /S /D >>] >>
endobj",
    );
    let mut editor = DocumentEditor::new(doc);
    let ranges = [PageLabelRange {
        first_page: 0,
        style: LabelStyle::LettersUpper,
        prefix: None,
        start: 1,
    }];
    editor.set_page_labels(&ranges).expect("the labels are set");
    assert_eq!(editor.get(ObjRef::new(61, 0)), Some(Object::Null));
    for mode in MODES {
        let after = saved(&editor, mode);
        assert_eq!(page_labels(&after, 2), ["A", "B"], "{mode:?}");
    }
}

// ---- attachments ------------------------------------------------------------

fn new_file() -> EmbeddedFile {
    EmbeddedFile {
        name: "new.txt".to_owned(),
        filename: "new.txt".to_owned(),
        data: b"new".to_vec(),
        ..EmbeddedFile::default()
    }
}

const OLD_FILE: &str = "17 0 obj
<< /Type /Filespec /F (old.txt) /UF (old.txt) /EF << /F 18 0 R >> >>
endobj
18 0 obj
<< /Type /EmbeddedFile /Length 3 >>
stream
old
endstream
endobj";

/// `/EmbeddedFiles` written directly into `/Names`: its entries are the
/// document's attachments all the same, and a new one is filed beside them.
#[test]
fn a_direct_attachment_tree_keeps_its_files() {
    let doc = document(
        "/Names << /EmbeddedFiles << /Names [(old.txt) 17 0 R] >> >>",
        OLD_FILE,
    );
    let mut editor = DocumentEditor::new(doc);
    editor.attach_file(&new_file()).expect("it attaches");
    for mode in MODES {
        let after = saved(&editor, mode);
        let names: Vec<String> = attachments(&after).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["new.txt", "old.txt"], "{mode:?}");
    }
}

/// The same with the direct root's leaves written as objects of their own:
/// the entries are carried into the new tree, and the old leaf, which
/// nothing reaches afterwards, is deleted.
#[test]
fn a_direct_attachment_tree_with_indirect_leaves_keeps_its_files() {
    let doc = document(
        "/Names << /EmbeddedFiles << /Kids [19 0 R] >> >>",
        &format!(
            "{OLD_FILE}
19 0 obj
<< /Limits [(old.txt) (old.txt)] /Names [(old.txt) 17 0 R] >>
endobj"
        ),
    );
    let mut editor = DocumentEditor::new(doc);
    editor.attach_file(&new_file()).expect("it attaches");
    assert_eq!(editor.get(ObjRef::new(19, 0)), Some(Object::Null));
    for mode in MODES {
        let after = saved(&editor, mode);
        let names: Vec<String> = attachments(&after).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["new.txt", "old.txt"], "{mode:?}");
    }
}

// ---- dates ------------------------------------------------------------------

fn at_offset(minutes: i32) -> Date {
    Date {
        year: 2026,
        month: 9,
        day: 26,
        hour: 12,
        minute: 0,
        second: 0,
        utc_offset_minutes: Some(minutes),
    }
}

/// An offset no zone has is refused like any other date 7.9.4 cannot spell
/// — `i32::MIN` among them, whose absolute value an `i32` cannot hold.
#[test]
fn an_offset_out_of_every_range_is_refused_not_a_panic() {
    let mut editor = DocumentEditor::new(document("", ""));
    for offset in [i32::MIN, i32::MIN + 1, -(24 * 60), 24 * 60, i32::MAX] {
        let date = at_offset(offset);
        assert_eq!(editor.set_creation_date(date), None, "{offset}");
        assert_eq!(editor.set_modification_date(date), None, "{offset}");
        let file = EmbeddedFile {
            created: Some(date),
            ..new_file()
        };
        assert_eq!(
            editor.attach_file(&file),
            Err(AttachError::Date(date)),
            "{offset}"
        );
    }
    assert!(!editor.is_dirty(), "every refusal wrote nothing");

    // The widest offsets the syntax spells still are spelled.
    for offset in [-(24 * 60 - 1), 24 * 60 - 1] {
        assert!(
            editor.set_creation_date(at_offset(offset)).is_some(),
            "{offset}"
        );
    }
}
