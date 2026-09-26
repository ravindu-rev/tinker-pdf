use super::*;
use crate::build::DocumentBuilder;
use crate::form;
use crate::pages::{self, Rect};
use crate::write::{WriteMode, WriteOptions};

fn document(pages: usize) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for i in 0..pages {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {i}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn reopen(editor: &DocumentEditor, mode: WriteMode) -> CosDocument {
    let bytes = editor.save(&WriteOptions {
        mode,
        ..WriteOptions::default()
    });
    CosDocument::open(bytes).expect("the saved document opens")
}

/// The shape of `pdfjs/test/pdfs/bug1980958.pdf`, transcribed.
///
/// 219 bytes, four objects, and the last of them numbered `2147483647` —
/// `i32::MAX`, which is legal: 7.3.10 puts no ceiling on an object number
/// beyond the ten-digit field of a cross-reference entry. There is no
/// `startxref` and no `trailer`, so the file opens through the rescan
/// ladder with a synthesised root, and it renders its 10 x 10 page in
/// under two seconds.
///
/// The bytes are hand-written here rather than read from the corpus,
/// because the corpus is fetched and this test has to run without it.
fn numbered_to_the_ceiling() -> Arc<CosDocument> {
    let text = concat!(
        "%PDF-1.7\n",
        "1 0 obj <</Type /Catalog /Pages 2 0 R>>\nendobj\n",
        "2 0 obj <</Type /Pages /Kids [3 0 R] /Count 1>>\nendobj\n",
        "3 0 obj <</Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]>>\nendobj\n",
        "\n2147483647 0 obj <</Root 1 0 R>>\nendobj\n",
    );
    let doc = CosDocument::open(text.as_bytes().to_vec()).expect("it opens");
    assert_eq!(doc.max_object_number(), 2_147_483_647, "the premise");
    Arc::new(doc)
}

/// **A rewrite costs what the document holds, not what its numbering
/// allows.**
///
/// This document holds four objects. Walking `1..=max_object_number()` to
/// find them is two thousand million lookups, which is not a hang in the
/// sense of a loop that never ends — it is a loop over the numbers the
/// file *could* have used instead of the four it did. The corpus runner
/// reads the difference as a slow file and the fuzzers prove no crash
/// rather than progress, so nothing in the suite covered it until this.
///
/// Asserted on the *size of the output* rather than on a clock. A dense
/// implementation cannot produce a small file: it would have to write
/// two thousand million cross-reference entries before it could write the
/// trailer. So this measures work done, and stays true on a fast machine
/// and a slow one — the discipline `bounds_ledger.rs` states by banning
/// `Instant::now` from itself.
#[test]
fn a_rewrite_carries_the_objects_the_document_has() {
    let editor = DocumentEditor::new(numbered_to_the_ceiling());
    let bytes = editor.save(&WriteOptions::default());
    assert!(
        bytes.len() < 2048,
        "a four-object rewrite is {} bytes",
        bytes.len()
    );

    let saved = CosDocument::open(bytes).expect("the rewrite reopens");
    assert_eq!(pages::count(&saved), 1, "and it is still the same page");
}

/// The same, on the path that writes a cross-reference **stream**.
///
/// A separate test because `WriteOptions::default()` has `object_streams`
/// off, so the classic table is what the test above exercises — and the
/// classic table has been written in subsections, and therefore sparse,
/// since it was written. The stream form was the dense one, and 7.5.8.2's
/// `/Index` is the same subsection device under another name.
#[test]
fn a_packed_rewrite_carries_the_objects_the_document_has() {
    let editor = DocumentEditor::new(numbered_to_the_ceiling());
    let bytes = editor.save(&WriteOptions {
        object_streams: true,
        compress: true,
        ..WriteOptions::default()
    });
    assert!(
        bytes.len() < 2048,
        "a four-object rewrite is {} bytes",
        bytes.len()
    );

    let saved = CosDocument::open(bytes).expect("the rewrite reopens");
    assert_eq!(pages::count(&saved), 1, "and it is still the same page");
}

#[test]
fn an_untouched_editor_is_not_dirty() {
    let editor = DocumentEditor::new(document(2));
    assert!(!editor.is_dirty());
    assert_eq!(editor.page_refs().len(), 2);
}

#[test]
fn deleting_a_page_removes_it() {
    let mut editor = DocumentEditor::new(document(3));
    assert!(editor.delete_page(1));
    assert!(editor.is_dirty());

    let saved = reopen(&editor, WriteMode::Incremental);
    assert_eq!(pages::count(&saved), 2);

    // The page that remains in the middle is the one that was third.
    let text = pages::content_bytes(&saved, &pages::collect(&saved)[1]);
    assert!(String::from_utf8_lossy(&text).contains("page 2"));
}

#[test]
fn moving_a_page_reorders_it() {
    let mut editor = DocumentEditor::new(document(3));
    assert!(editor.move_page(2, 0));

    let saved = reopen(&editor, WriteMode::Incremental);
    let first = pages::content_bytes(&saved, &pages::collect(&saved)[0]);
    assert!(
        String::from_utf8_lossy(&first).contains("page 2"),
        "the last page is now first"
    );
    assert_eq!(pages::count(&saved), 3, "and none were lost");
}

#[test]
fn rotating_a_page_accumulates_and_normalizes() {
    let mut editor = DocumentEditor::new(document(1));
    assert!(editor.rotate_page(0, 90));
    assert!(editor.rotate_page(0, 90));

    let saved = reopen(&editor, WriteMode::Incremental);
    assert_eq!(pages::collect(&saved)[0].rotation, 180);

    // And a further turn wraps rather than growing.
    let mut editor = DocumentEditor::new(Arc::new(saved));
    assert!(editor.rotate_page(0, 270));
    let saved = reopen(&editor, WriteMode::Incremental);
    assert_eq!(pages::collect(&saved)[0].rotation, 90);
}

/// **A crop box reaches the file as the rectangle the caller stated.**
///
/// Read back through `pages::collect`, which clips a crop box to the media
/// box — so a rectangle inside the page comes back untouched and this test
/// says the writer put it there rather than that the reader invented it.
#[test]
fn a_crop_box_is_written_as_the_rectangle_it_was_given() {
    let mut editor = DocumentEditor::new(document(1));
    assert!(editor.set_crop_box(0, 10.0, 20.0, 90.0, 80.0));

    let saved = reopen(&editor, WriteMode::Incremental);
    let crop = pages::collect(&saved)[0].crop_box;
    assert!((crop.x0 - 10.0).abs() < 1e-9, "{crop:?}");
    assert!((crop.y0 - 20.0).abs() < 1e-9, "{crop:?}");
    assert!((crop.x1 - 90.0).abs() < 1e-9, "{crop:?}");
    assert!((crop.y1 - 80.0).abs() < 1e-9, "{crop:?}");
}

/// **Corners in any order are the same rectangle**, and a degenerate one is
/// refused rather than written.
///
/// 7.9.5 wants two distinct corners and says nothing about which is which,
/// so a caller passing the top-right first is not making a mistake. A
/// caller passing the same corner twice is: a crop box of no area is a page
/// no viewer can show, and writing it would put the refusal off until
/// somebody opened the file.
#[test]
fn a_crop_box_normalizes_its_corners_and_refuses_a_degenerate_one() {
    let mut editor = DocumentEditor::new(document(1));
    assert!(editor.set_crop_box(0, 90.0, 80.0, 10.0, 20.0));
    let saved = reopen(&editor, WriteMode::Incremental);
    let crop = pages::collect(&saved)[0].crop_box;
    assert!(crop.x0 < crop.x1 && crop.y0 < crop.y1, "{crop:?}");
    assert!((crop.x0 - 10.0).abs() < 1e-9, "{crop:?}");

    let mut editor = DocumentEditor::new(document(1));
    assert!(!editor.set_crop_box(0, 10.0, 20.0, 10.0, 80.0), "no width");
    assert!(!editor.set_crop_box(0, 10.0, 20.0, 90.0, 20.0), "no height");
    assert!(
        !editor.set_crop_box(0, f64::NAN, 0.0, 1.0, 1.0),
        "not a number"
    );
    assert!(!editor.set_crop_box(9, 0.0, 0.0, 1.0, 1.0), "no such page");
    assert!(
        !editor.is_dirty(),
        "a refused crop box changes nothing at all"
    );
}

#[test]
fn out_of_range_page_operations_are_refused() {
    let mut editor = DocumentEditor::new(document(2));
    assert!(!editor.delete_page(9));
    assert!(!editor.move_page(0, 9));
    assert!(!editor.move_page(9, 0));
    assert!(!editor.rotate_page(9, 90));
    assert!(!editor.append_content(9, b"0 0 1 1 re f"));
    assert!(!editor.is_dirty(), "a refused edit changes nothing");
}

/// 7.5.5 Table 15: `/Prev` names the previous cross-reference *section*.
/// Naming the end of the file instead leaves every object the update did
/// not carry unreachable — and the reader hides it, because a document it
/// cannot walk falls to the repair scanner and finds the objects anyway.
/// So the assertion is on the **ladder level**, not on the content: every
/// page-operation test in this file reads correctly either way.
#[test]
fn an_incremental_save_chains_to_the_table_it_updates() {
    let mut editor = DocumentEditor::new(document(3));
    assert!(editor.delete_page(1));

    let saved = reopen(&editor, WriteMode::Incremental);
    assert_eq!(
        saved.ladder_level(),
        crate::LadderLevel::Trust,
        "the update was walked, not rescanned: {:?}",
        saved.warnings()
    );
    assert!(saved.warnings().is_empty(), "{:?}", saved.warnings());
    assert_eq!(saved.revisions().len(), 2, "two revisions, chained");
    assert_eq!(pages::count(&saved), 2);
}

#[test]
fn an_incremental_save_keeps_the_original_bytes() {
    let doc = document(2);
    let original = doc.bytes().to_vec();

    let mut editor = DocumentEditor::new(doc);
    editor.rotate_page(0, 90);
    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });

    assert!(
        saved.starts_with(&original),
        "the signable prefix must survive an edit"
    );
}

#[test]
fn appended_content_does_not_disturb_what_was_there() {
    let mut editor = DocumentEditor::new(document(1));
    assert!(editor.append_content(0, b"1 0 0 rg 10 10 50 20 re f"));

    let saved = reopen(&editor, WriteMode::Incremental);
    let content = pages::content_bytes(&saved, &pages::collect(&saved)[0]);
    let text = String::from_utf8_lossy(&content);

    assert!(text.contains("page 0"), "the original drawing survives");
    assert!(text.contains("1 0 0 rg"), "and the addition is present");
    assert!(
        text.matches('q').count() >= 2,
        "each part is bracketed so neither can disturb the other"
    );
}

#[test]
fn annotations_reach_the_page() {
    let doc = document(1);
    let mut editor = DocumentEditor::new(doc.clone());

    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 60.0,
        y1: 30.0,
    };
    let note = annot::text_note(&doc, rect, "a remark", false);
    assert!(editor.add_annotation(0, note).is_some());

    let saved = reopen(&editor, WriteMode::Incremental);
    let page = pages::collect(&saved)[0].reference;
    let object = saved.get(page).expect("the page loads");
    let annots = object
        .as_dict()
        .and_then(|d| d.get_array(saved.intern(b"Annots")))
        .map(<[Object]>::to_vec)
        .unwrap_or_default();

    assert_eq!(annots.len(), 1, "one annotation");
    let annot_ref = annots[0].as_objref().expect("a reference");
    let annot = saved.get(annot_ref).expect("it loads");
    let dict = annot.as_dict().expect("a dictionary");

    let subtype = dict
        .get(saved.intern(b"Subtype"))
        .and_then(Object::as_name)
        .and_then(|n| saved.name_bytes(n));
    assert_eq!(subtype.as_deref(), Some(b"Text".as_slice()));
    assert_eq!(
        dict.get_int(saved.intern(b"F")),
        Some(4),
        "the print flag is set, or it will not appear on paper"
    );
}

/// An annotation without an appearance stream is drawn by guesswork, and
/// viewers guess differently. One is attached on the way in, and it has to
/// survive being written and read back as a real form.
#[test]
fn an_annotation_is_given_an_appearance_stream() {
    let doc = document(1);
    let mut editor = DocumentEditor::new(doc.clone());

    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 60.0,
        y1: 30.0,
    };
    let square = annot::square(
        &doc,
        rect,
        annot::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
        2.0,
    );
    editor.add_annotation(0, square).expect("it is added");

    let saved = reopen(&editor, WriteMode::Incremental);
    let page = pages::collect(&saved)[0].reference;
    let annot_ref = saved
        .get(page)
        .ok()
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| d.get_array(saved.intern(b"Annots")).map(<[Object]>::to_vec))
        .and_then(|a| a.first().and_then(Object::as_objref))
        .expect("the annotation is on the page");

    let annot = saved.get(annot_ref).expect("it loads");
    let form_ref = annot
        .as_dict()
        .and_then(|d| d.get_dict(saved.intern(b"AP")))
        .and_then(|ap| ap.get_ref(saved.intern(b"N")))
        .expect("with a normal appearance");

    let form = saved.get(form_ref).expect("the form loads");
    let form_dict = form.as_dict().expect("a stream dictionary");
    assert_eq!(
        form_dict
            .get_name(saved.intern(b"Subtype"))
            .and_then(|n| saved.name_bytes(n))
            .as_deref(),
        Some(b"Form".as_slice()),
        "the appearance is a form XObject"
    );

    let content = saved.stream_decoded(form_ref).expect("its content decodes");
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains(" re"), "which draws the square: {text}");
}

/// An appearance supplied by the caller is theirs, not ours to replace.
#[test]
fn a_supplied_appearance_is_left_alone() {
    let doc = document(1);
    let mut editor = DocumentEditor::new(doc.clone());

    let rect = Rect {
        x0: 10.0,
        y0: 10.0,
        x1: 60.0,
        y1: 30.0,
    };
    let mut square = annot::square(
        &doc,
        rect,
        annot::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
        2.0,
    );
    let mine = ObjRef::new(4242, 0);
    let mut ap = Dict::new();
    ap.insert(doc.intern(b"N"), Object::Ref(mine));
    square.insert(doc.intern(b"AP"), Object::Dict(ap));

    let annot_ref = editor.add_annotation(0, square).expect("it is added");
    let kept = editor
        .get(annot_ref)
        .and_then(|o| o.as_dict().cloned())
        .and_then(|d| d.get_dict(doc.intern(b"AP")).cloned())
        .and_then(|ap| ap.get_ref(doc.intern(b"N")))
        .expect("the appearance survives");
    assert_eq!(kept, mine);
}

#[test]
fn a_highlights_quadpoints_follow_the_specifications_order() {
    let doc = document(1);
    // Upper-left, upper-right, lower-left, lower-right.
    let quad = [10.0, 30.0, 60.0, 30.0, 10.0, 10.0, 60.0, 10.0];
    let dict = annot::highlight(
        &doc,
        &[quad],
        annot::Color {
            r: 1.0,
            g: 1.0,
            b: 0.0,
        },
    );

    let points = dict
        .get_array(doc.intern(b"QuadPoints"))
        .expect("quad points");
    assert_eq!(points.len(), 8);
    assert_eq!(points[0].as_number(), Some(10.0));
    assert_eq!(points[1].as_number(), Some(30.0), "upper edge first");

    // The rectangle encloses the quad.
    let rect = dict.get_array(doc.intern(b"Rect")).expect("a rect");
    assert_eq!(rect[0].as_number(), Some(10.0));
    assert_eq!(rect[3].as_number(), Some(30.0));
}

#[test]
fn a_deleted_object_reads_as_null_afterwards() {
    let doc = document(1);
    let mut editor = DocumentEditor::new(doc);
    let victim = editor.allocate();
    editor.put(victim, Object::Int(5));
    assert_eq!(editor.get(victim), Some(Object::Int(5)));

    editor.delete(victim);
    assert_eq!(
        editor.get(victim),
        Some(Object::Null),
        "a deleted object must not read as its old value"
    );
}

#[test]
fn a_rewrite_carries_everything_not_only_the_changes() {
    let mut editor = DocumentEditor::new(document(2));
    editor.rotate_page(0, 90);

    let saved = reopen(&editor, WriteMode::Rewrite);
    assert_eq!(pages::count(&saved), 2, "both pages survive a rewrite");
    assert_eq!(pages::collect(&saved)[0].rotation, 90);

    let content = pages::content_bytes(&saved, &pages::collect(&saved)[1]);
    assert!(
        String::from_utf8_lossy(&content).contains("page 1"),
        "including content streams, whose data lives outside the object"
    );
}

/// A form with a text field, a checkbox whose on state is not `/Yes`, and
/// a radio pair — the shapes that trip a naive filler.
fn form_document() -> Arc<CosDocument> {
    let bytes: &[u8] = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R 30 0 R]
   /NeedAppearances true /DA (/Helv 0 Tf 0 g)
   /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]
   /Annots [10 0 R 20 0 R 31 0 R 32 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (name) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot
   /MaxLen 10 >>
endobj
20 0 obj
<< /FT /Btn /T (agree) /Rect [10 120 30 140] /Subtype /Widget /Type /Annot
   /AP << /N << /On 21 0 R /Off 22 0 R >> >> >>
endobj
21 0 obj
<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>
stream

endstream
endobj
22 0 obj
<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>
stream

endstream
endobj
30 0 obj
<< /FT /Btn /Ff 32768 /T (colour) /Kids [31 0 R 32 0 R] >>
endobj
31 0 obj
<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [10 90 30 110] /AS /Off
   /AP << /N << /red 21 0 R /Off 22 0 R >> >> >>
endobj
32 0 obj
<< /Parent 30 0 R /Subtype /Widget /Type /Annot /Rect [40 90 60 110] /AS /Off
   /AP << /N << /blue 21 0 R /Off 22 0 R >> >> >>
endobj
trailer
<< /Size 33 /Root 1 0 R >>
%%EOF
";
    Arc::new(CosDocument::open(bytes).expect("the form opens"))
}

fn field_named(doc: &CosDocument, name: &str) -> form::Field {
    form::fields(doc)
        .into_iter()
        .find(|f| f.name == name)
        .expect("the field is there")
}

/// The half that usually gets skipped: the value must come with an
/// appearance, or the field shows filled in some viewers and blank in the
/// rest.
#[test]
fn filling_a_text_field_writes_a_value_and_an_appearance() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    assert!(editor.set_field_value("name", "Ada"));

    let saved = reopen(&editor, WriteMode::Incremental);
    let field = field_named(&saved, "name");
    assert_eq!(field.value, form::FieldValue::Text("Ada".to_string()));

    let widget = saved.get(field.widgets[0]).expect("the widget loads");
    let form_ref = widget
        .as_dict()
        .and_then(|d| d.get_dict(saved.intern(b"AP")))
        .and_then(|ap| ap.get_ref(saved.intern(b"N")))
        .expect("an appearance was written");
    let content = saved.stream_decoded(form_ref).expect("it decodes");
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains("(Ada) Tj"), "which draws the value: {text}");
    assert!(text.starts_with("/Tx BMC"), "marked as a field appearance");
}

/// Leaving `/NeedAppearances` set asks every viewer to throw away what was
/// just written and rebuild it from its own idea of the field.
#[test]
fn filling_clears_the_rebuild_request() {
    let doc = form_document();
    assert!(form::needs_appearances(&doc), "the fixture sets it");

    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor.set_field_value("name", "Ada");
    let saved = reopen(&editor, WriteMode::Incremental);
    assert!(!form::needs_appearances(&saved));
}

#[test]
fn a_value_the_field_refuses_changes_nothing() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));

    assert!(
        !editor.set_field_value("name", "far too long for ten"),
        "over /MaxLen"
    );
    assert!(!editor.set_field_value("nonesuch", "x"), "no such field");
    assert!(!editor.is_dirty(), "and nothing was written");
}

/// `/Yes` is a convention, not a rule, and assuming it ticks a box the
/// file has no appearance for.
#[test]
fn a_checkbox_uses_the_on_state_its_appearance_declares() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    assert!(editor.set_checkbox("agree", true));

    let saved = reopen(&editor, WriteMode::Incremental);
    let field = field_named(&saved, "agree");
    assert_eq!(field.value, form::FieldValue::State("On".to_string()));

    let widget = saved.get(field.widgets[0]).expect("the widget");
    let state = widget
        .as_dict()
        .and_then(|d| d.get_name(saved.intern(b"AS")))
        .and_then(|n| saved.name_bytes(n));
    assert_eq!(
        state.as_deref(),
        Some(b"On".as_slice()),
        "the shown state follows the value"
    );
}

#[test]
fn a_checkbox_turned_off_shows_off() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor.set_checkbox("agree", true);
    assert!(editor.set_checkbox("agree", false));

    let saved = reopen(&editor, WriteMode::Incremental);
    let field = field_named(&saved, "agree");
    assert!(!field.value.is_on());
}

/// Setting only the chosen widget leaves the previous one still drawn,
/// which is how two radio options end up looking selected at once.
#[test]
fn selecting_a_radio_turns_its_siblings_off() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    assert!(editor.select_radio("colour", "blue"));

    let saved = reopen(&editor, WriteMode::Incremental);
    let field = field_named(&saved, "colour");
    assert_eq!(field.value, form::FieldValue::State("blue".to_string()));

    let state_of = |widget: ObjRef| -> String {
        saved
            .get(widget)
            .ok()
            .and_then(|o| o.as_dict().and_then(|d| d.get_name(saved.intern(b"AS"))))
            .and_then(|n| saved.name_bytes(n))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    assert_eq!(state_of(field.widgets[0]), "Off", "the red one is off");
    assert_eq!(state_of(field.widgets[1]), "blue", "the blue one is on");
}

#[test]
fn selecting_a_radio_option_that_does_not_exist_is_refused() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    assert!(!editor.select_radio("colour", "green"));
    assert!(!editor.is_dirty());
}

/// A field that was never filled and one filled with nothing are
/// different states, and a submitted form distinguishes them.
#[test]
fn resetting_removes_a_value_that_has_no_default() {
    let doc = form_document();
    let mut editor = DocumentEditor::new(Arc::clone(&doc));
    editor.set_field_value("name", "Ada");
    editor.set_checkbox("agree", true);
    editor.reset_form();

    let saved = reopen(&editor, WriteMode::Incremental);
    assert_eq!(field_named(&saved, "name").value, form::FieldValue::None);
    assert!(!field_named(&saved, "agree").value.is_on());
}
