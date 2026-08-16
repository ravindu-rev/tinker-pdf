//! Transactions over the editor, and the atomicity of one field's fill.
//!
//! Two defects motivate this file, and both were reproduced against the code
//! that preceded it (PRE-E in `docs/plans/gaps/00-execution-order.md`):
//!
//! 1. A refused second field left the first one written. There was no
//!    begin/commit/rollback anywhere on `DocumentEditor`, so "these values
//!    apply together or not at all" was inexpressible.
//! 2. A field with two widgets, one of which had no `/Rect`, regenerated one
//!    appearance, left the other stale, and returned `true`. Measured: the
//!    first widget drew `(Ada) Tj` and the second still drew `(Bob) Tj`, in a
//!    file whose `/V` said `Ada`. That is a document that looks filled and is
//!    wrong.

use std::sync::Arc;

use tinker_pdf_cos::{
    fields, CosDocument, DocumentEditor, FillError, ObjRef, SkippedWidget, WidgetDefect, WriteMode,
    WriteOptions,
};

/// FNV-1a, 64 bit. Hand-rolled so the byte-identity assertions depend on
/// nothing, and stable across platforms because it is integer arithmetic.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Hand-written bytes carry no cross-reference table, so they open through the
/// repair scanner. A rewrite gives the same object graph inside a well-formed
/// file, which is what a fixture has to be for a saved edit to be readable.
fn normalize(bytes: &[u8]) -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(bytes).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// Two text fields, each merged with its single widget: `name` unbounded and
/// `short` capped at four characters, so one value can be refused while the
/// other is accepted.
fn form_document() -> Arc<CosDocument> {
    normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R]
   /NeedAppearances true /DA (/Helv 0 Tf 0 g)
   /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R 20 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (name) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot >>
endobj
20 0 obj
<< /FT /Tx /T (short) /Rect [10 120 190 140] /Subtype /Widget /Type /Annot
   /MaxLen 4 >>
endobj
trailer
<< /Size 21 /Root 1 0 R >>
%%EOF
",
    )
}

/// One field, **two** widgets, on two pages — the ordinary shape of a field
/// that appears twice — and the second widget has no `/Rect`. 12.5.2 Table 164
/// makes `/Rect` required for every annotation, so this is a damaged file; it
/// is also the exact shape that reported success while leaving a stale
/// appearance. Both widgets start out drawing `(Bob)`, which is what makes the
/// stale one identifiable after a fill.
fn two_widget_form() -> Arc<CosDocument> {
    normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R]
   /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [11 0 R] >>
endobj
4 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [12 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (name) /V (Bob) /Kids [11 0 R 12 0 R] >>
endobj
11 0 obj
<< /Parent 10 0 R /Subtype /Widget /Type /Annot /Rect [10 150 190 170]
   /AP << /N 13 0 R >> >>
endobj
12 0 obj
<< /Parent 10 0 R /Subtype /Widget /Type /Annot /AP << /N 13 0 R >> >>
endobj
13 0 obj
<< /Type /XObject /Subtype /Form /BBox [0 0 180 20] /Length 20 >>
stream
/Tx BMC (Bob) Tj EMC
endstream
endobj
trailer
<< /Size 14 /Root 1 0 R >>
%%EOF
",
    )
}

fn save(editor: &DocumentEditor, mode: WriteMode) -> Vec<u8> {
    editor.save(&WriteOptions {
        mode,
        ..WriteOptions::default()
    })
}

fn reopen(editor: &DocumentEditor, mode: WriteMode) -> CosDocument {
    CosDocument::open(save(editor, mode)).expect("the saved document opens")
}

/// What a widget's normal appearance actually draws.
fn drawn(doc: &CosDocument, widget: ObjRef) -> String {
    doc.get(widget)
        .ok()
        .and_then(|o| {
            o.as_dict()
                .and_then(|d| d.get_dict(doc.intern(b"AP")).cloned())
        })
        .and_then(|ap| ap.get_ref(doc.intern(b"N")))
        .and_then(|r| doc.stream_decoded(r).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn value_of(doc: &CosDocument, name: &str) -> String {
    fields(doc)
        .into_iter()
        .find(|f| f.name == name)
        .map(|f| f.value.as_text())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The happy path is byte-identical
// ---------------------------------------------------------------------------

/// Recorded from the implementation that preceded the transaction primitive,
/// on the same fixtures, and asserted here so the rewrite cannot have changed
/// a byte of what a successful fill produces. Length and hash both, because a
/// hash alone says nothing about *how* two files differ and a length alone
/// misses a reordering.
///
/// The fixture's own length is pinned beside them so a change in the writer is
/// separable from a change in the filler: if only the last three move, the
/// filler moved; if all four move, the writer did.
#[test]
fn a_successful_fill_produces_the_bytes_it_always_did() {
    assert_eq!(form_document().bytes().len(), 776, "the fixture itself");

    let mut editor = DocumentEditor::new(form_document());
    assert!(editor.set_field_value("name", "Ada"));
    let incremental = save(&editor, WriteMode::Incremental);
    assert_eq!(incremental.len(), 1428);
    assert_eq!(fnv(&incremental), 0x8232_6b9a_7160_4130, "incremental save");

    let mut editor = DocumentEditor::new(form_document());
    assert!(editor.set_field_value("name", "Ada"));
    let rewritten = save(&editor, WriteMode::Rewrite);
    assert_eq!(rewritten.len(), 1047);
    assert_eq!(fnv(&rewritten), 0xfda2_4c36_1d03_5f1d, "full rewrite");

    // Two fills and a reset, so the pin covers `reset_form` and the second
    // allocation path as well as the first.
    let mut editor = DocumentEditor::new(form_document());
    assert!(editor.set_field_value("name", "Ada"));
    assert!(editor.set_field_value("short", "abcd"));
    assert!(editor.reset_form().is_empty());
    let reset = save(&editor, WriteMode::Incremental);
    assert_eq!(reset.len(), 2350);
    assert_eq!(fnv(&reset), 0xb8a4_57c7_c992_a54f, "fill, fill, reset");
}

// ---------------------------------------------------------------------------
// The transaction restores all four fields
// ---------------------------------------------------------------------------

/// `overlay`. A test that checks only this one passes with the other three
/// restores deleted, which is why there are four tests rather than one.
#[test]
fn a_rollback_restores_written_objects() {
    let mut editor = DocumentEditor::new(form_document());
    let outcome: Result<(), ()> = editor.transaction(|tx| {
        assert!(tx.fill_field("name", "Ada").is_ok());
        assert!(tx.is_dirty(), "written inside the transaction");
        Err(())
    });

    assert!(outcome.is_err());
    assert!(!editor.is_dirty(), "and gone again outside it");
    assert_eq!(value_of(&reopen(&editor, WriteMode::Rewrite), "name"), "");
}

/// `deleted`. Deletion is not stored in the overlay — a deleted object is
/// *removed* from it — so a rollback that restores only the overlay leaves the
/// object reading as null for ever.
#[test]
fn a_rollback_restores_deletions() {
    let mut editor = DocumentEditor::new(form_document());
    let font = ObjRef::new(5, 0);
    assert!(editor.get(font).is_some_and(|o| o.as_dict().is_some()));

    let outcome: Result<(), ()> = editor.transaction(|tx| {
        tx.delete(font);
        assert_eq!(tx.get(font), Some(tinker_pdf_cos::Object::Null));
        Err(())
    });

    assert!(outcome.is_err());
    assert!(!editor.is_dirty(), "the deletion was undone");
    assert!(
        editor.get(font).is_some_and(|o| o.as_dict().is_some()),
        "and the object reads as itself again, not as null"
    );
}

/// `page_order`. It is `None` until something disturbs it, and a rollback that
/// leaves it `Some` makes the editor dirty for ever and rewrites `/Kids` on
/// every subsequent save.
#[test]
fn a_rollback_restores_the_page_order() {
    let mut editor = DocumentEditor::new(two_widget_form());
    assert_eq!(editor.page_refs().len(), 2);

    let outcome: Result<(), ()> = editor.transaction(|tx| {
        assert!(tx.delete_page(0));
        assert_eq!(tx.page_refs().len(), 1);
        Err(())
    });

    assert!(outcome.is_err());
    assert_eq!(editor.page_refs().len(), 2, "both pages are back");
    assert!(
        !editor.is_dirty(),
        "and the order is untouched, not restored"
    );
    assert_eq!(
        tinker_pdf_cos::pages::count(&reopen(&editor, WriteMode::Rewrite)),
        2
    );
}

/// `next`. Nothing else observes it, so a rollback that leaves it advanced is
/// invisible until the numbers show up in a saved file.
#[test]
fn a_rollback_hands_back_the_object_numbers_it_took() {
    let mut editor = DocumentEditor::new(form_document());
    let inside = editor
        .transaction(|tx| {
            let taken = tx.allocate();
            Err::<(), ObjRef>(taken)
        })
        .expect_err("the transaction rolled back");

    let after = editor.allocate();
    assert_eq!(
        after, inside,
        "an abandoned edit hands its numbers back; see the field's own comment \
         for what that costs a caller holding one"
    );
}

/// The consequence of not restoring it, in the only place it is visible: the
/// file. Fifty abandoned fills must cost nothing, and an editor that keeps
/// their numbers writes a bigger file with a higher `/Size` and one more
/// cross-reference subsection (7.5.4) for every gap it left.
#[test]
fn abandoned_edits_do_not_grow_the_next_saved_file() {
    let clean = {
        let mut editor = DocumentEditor::new(form_document());
        assert!(editor.set_field_value("name", "Ada"));
        save(&editor, WriteMode::Incremental)
    };

    let after_failures = {
        let mut editor = DocumentEditor::new(form_document());
        for _ in 0..50 {
            let outcome: Result<(), ()> = editor.transaction(|tx| {
                assert!(tx.fill_field("name", "discarded").is_ok());
                Err(())
            });
            assert!(outcome.is_err());
        }
        assert!(editor.set_field_value("name", "Ada"));
        save(&editor, WriteMode::Incremental)
    };

    assert_eq!(
        after_failures.len(),
        clean.len(),
        "fifty abandoned edits changed the size of the file that followed"
    );
    assert_eq!(fnv(&after_failures), fnv(&clean), "byte for byte");
}

#[test]
fn a_committed_transaction_keeps_everything() {
    let mut editor = DocumentEditor::new(form_document());
    let skipped = editor
        .transaction(|tx| {
            tx.fill_field("name", "Ada")?;
            tx.fill_field("short", "abcd")
        })
        .expect("both fields accept");

    assert!(skipped.is_empty());
    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "Ada");
    assert_eq!(value_of(&saved, "short"), "abcd");
}

/// An inner rollback restores the inner start; the outer transaction is
/// untouched and can still commit what it did before.
#[test]
fn transactions_nest() {
    let mut editor = DocumentEditor::new(form_document());
    let outcome: Result<(), FillError> = editor.transaction(|tx| {
        tx.fill_field("name", "Ada")?;
        let inner: Result<(), ()> = tx.transaction(|inner| {
            assert!(inner.fill_field("short", "abcd").is_ok());
            Err(())
        });
        assert!(inner.is_err());
        Ok(())
    });

    assert!(outcome.is_ok());
    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "Ada", "the outer edit survives");
    assert_eq!(value_of(&saved, "short"), "", "the inner one does not");
}

// ---------------------------------------------------------------------------
// One field, applied wholly or reported precisely
// ---------------------------------------------------------------------------

/// Ruling 2 degrades rather than failing, so the value is written and the
/// widget that *can* be drawn is drawn. Ruling 10 requires the degradation to
/// name its object, which is what makes this different from the `continue` it
/// replaces: the caller can see that one widget on page two is showing
/// something else.
#[test]
fn a_widget_without_a_rect_is_reported_by_object_number() {
    let mut editor = DocumentEditor::new(two_widget_form());
    let skipped = editor
        .fill_field("name", "Ada")
        .expect("the value is taken");

    assert_eq!(
        skipped,
        vec![SkippedWidget {
            widget: ObjRef::new(12, 0),
            reason: WidgetDefect::RectMissing,
        }],
        "12.5.2 requires /Rect; the widget that lacks one is named"
    );

    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "Ada");
    assert!(
        drawn(&saved, ObjRef::new(11, 0)).contains("(Ada) Tj"),
        "the widget with a rectangle draws the new value"
    );
    assert!(
        drawn(&saved, ObjRef::new(12, 0)).contains("(Bob) Tj"),
        "and the one without keeps what it had, which is the thing that used \
         to be silent"
    );
}

/// The boolean API cannot say "partly", and what it used to say was `true`.
/// Now it says no and leaves the document alone, so the file is never one that
/// looks filled and is wrong.
#[test]
fn set_field_value_refuses_a_field_it_cannot_wholly_draw() {
    let mut editor = DocumentEditor::new(two_widget_form());
    assert!(!editor.set_field_value("name", "Ada"));
    assert!(!editor.is_dirty(), "and nothing at all was written");

    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "Bob", "the old value stands");
    assert!(drawn(&saved, ObjRef::new(11, 0)).contains("(Bob) Tj"));
    assert!(drawn(&saved, ObjRef::new(12, 0)).contains("(Bob) Tj"));
}

/// A field whose widgets all have rectangles is unaffected, so the refusal
/// above is about the damage and not about having two widgets.
#[test]
fn a_field_whose_widgets_can_all_be_drawn_still_succeeds() {
    let mut editor = DocumentEditor::new(form_document());
    assert!(editor.set_field_value("name", "Ada"));
    assert_eq!(editor.fill_field("short", "abcd"), Ok(Vec::new()));
}

// ---------------------------------------------------------------------------
// Several fields, applied wholly or not at all
// ---------------------------------------------------------------------------

/// The shape gap 27 needs. A calculation that sets three fields and fails on
/// the fourth must not leave a document whose totals disagree with its inputs.
#[test]
fn a_refused_field_rolls_back_the_ones_before_it() {
    let mut editor = DocumentEditor::new(form_document());
    let rejection = editor
        .set_field_values(&[("name", "Ada"), ("short", "far too long")])
        .expect_err("the second value is over /MaxLen");

    assert_eq!(rejection.field, "short");
    assert_eq!(rejection.reason, FillError::ValueRefused);
    assert!(
        !editor.is_dirty(),
        "the first field was written and then unwritten"
    );

    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "", "no half-filled form");
}

#[test]
fn an_unknown_field_names_itself() {
    let mut editor = DocumentEditor::new(form_document());
    let rejection = editor
        .set_field_values(&[("name", "Ada"), ("nonesuch", "x")])
        .expect_err("there is no such field");
    assert_eq!(rejection.field, "nonesuch");
    assert_eq!(rejection.reason, FillError::NoSuchField);
    assert!(!editor.is_dirty());
}

#[test]
fn every_accepted_field_lands_together() {
    let mut editor = DocumentEditor::new(form_document());
    let skipped = editor
        .set_field_values(&[("name", "Ada"), ("short", "abcd")])
        .expect("both are accepted");
    assert!(skipped.is_empty());

    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(value_of(&saved, "name"), "Ada");
    assert_eq!(value_of(&saved, "short"), "abcd");
}

/// A damaged widget is reported, not refused, so a multi-field calculation
/// over a file with one broken annotation still applies — and still says which
/// annotation it could not draw.
#[test]
fn a_damaged_widget_is_carried_through_a_multi_field_apply() {
    let mut editor = DocumentEditor::new(two_widget_form());
    let skipped = editor
        .set_field_values(&[("name", "Ada")])
        .expect("the value is taken");
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].widget, ObjRef::new(12, 0));
    assert!(editor.is_dirty());
}
