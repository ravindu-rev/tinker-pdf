//! `testdata/form-fields.pdf`, adjudicated by this engine (gap 32 milestone 1).
//!
//! The fixture is written by a layout aid that computes cross-reference
//! offsets and nothing else; ruling 13 puts the line between *supplying* and
//! *adjudicating*, so this file is the adjudication. If the committed bytes
//! ever stop being the document the write-parity scripts assume, these
//! assertions are what says so — not a comment in the generator.
//!
//! `testdata/` held no form before this: the four canonical fixtures are
//! simple-text, outline-3level, permissions-noprint and encrypted-aes256, and
//! none of them carries an `/AcroForm`. So the fill-and-save half of the
//! write-parity suite had nothing to open, which is why this exists.
//!
//! The one deliberate defect is a widget with no `/Rect`. 12.5.2 Table 164
//! makes it required, and a field one of whose widgets lacks it is the
//! **fourth outcome** the design is about: the value is written, one widget is
//! drawn, and one is left showing what it showed before. Ruling 2 degrades
//! rather than failing and ruling 10 makes the degradation name its object, so
//! `fill_field` reports it. Without a fixture that has one, every binding's
//! skipped-widget path would be reachable in principle and untravelled in
//! fact.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{
    fields, validate, CosDocument, DocumentEditor, LadderLevel, ObjRef, WidgetDefect, WriteMode,
    WriteOptions,
};

fn fixture_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join("form-fields.pdf");
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn fixture() -> Arc<CosDocument> {
    Arc::new(CosDocument::open(fixture_bytes()).expect("the form fixture opens"))
}

/// The widget the fixture is damaged at. Object 7, the second kid of the text
/// field, and the only object in the file with no `/Rect`.
const RECTLESS_WIDGET: ObjRef = ObjRef { num: 7, gen: 0 };

/// The fixture is well formed apart from the one thing it is damaged at.
///
/// This is the assertion that lets the parity gate demand a *clean* artefact
/// from every surface. If the fixture itself carried a structural defect, an
/// incremental save would carry it forward — the original bytes survive as a
/// prefix (7.5.6) — and "passes the strict validator" could never be the bar.
#[test]
fn the_committed_fixture_opens_on_trust_and_validates_clean() {
    let doc = fixture();
    assert_eq!(
        doc.ladder_level(),
        LadderLevel::Trust,
        "the cross-reference table's offsets are used as written, so the \
         generator's arithmetic is right"
    );
    assert!(
        doc.warnings().is_empty(),
        "and it opened cleanly rather than merely opened (ruling 10): {:?}",
        doc.warnings()
    );

    let defects = validate(&doc);
    assert!(
        defects.is_empty(),
        "the strict validator finds nothing: {:?}",
        defects.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
    );
}

/// The damaged widget is *not* in the page's `/Annots`, and that is deliberate.
///
/// The strict validator walks `/Annots` and reports `annot-rect-malformed` for
/// a widget with no usable `/Rect`. A fixture that listed this one there would
/// make every artefact saved from it carry a defect for ever. A widget a field
/// claims through `/Kids` and a page does not show is a real damaged-form
/// shape, and it is the one that isolates the defect under test from the
/// validator's own verdict.
#[test]
fn the_damaged_widget_is_claimed_by_the_field_and_not_by_the_page() {
    let doc = fixture();
    let field = fields(&doc)
        .into_iter()
        .find(|f| f.name == "name")
        .expect("the text field is there");
    assert_eq!(
        field.widgets,
        vec![ObjRef::new(6, 0), RECTLESS_WIDGET],
        "two widget kids, in the order /Kids gives them"
    );

    let page = tinker_pdf_cos::pages::collect(&doc)
        .into_iter()
        .next()
        .expect("one page");
    let annots = doc
        .resolve_key(
            doc.get(page.reference)
                .expect("the page object")
                .as_dict()
                .expect("a dictionary"),
            doc.intern(b"Annots"),
        )
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(tinker_pdf_cos::Object::as_objref)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        !annots.contains(&RECTLESS_WIDGET),
        "the page does not show it, which is what keeps the file valid"
    );
    assert!(
        annots.contains(&ObjRef::new(6, 0)),
        "and shows the other one"
    );
}

/// The milestone's own exit criterion: exactly one `SkippedWidget`, naming the
/// object and the defect.
#[test]
fn filling_the_damaged_field_reports_exactly_one_skipped_widget() {
    let mut editor = DocumentEditor::new(fixture());
    let skipped = editor
        .fill_field("name", "Ada Lovelace")
        .expect("the value is taken -- ruling 2 degrades rather than failing");

    assert_eq!(skipped.len(), 1, "one widget, not zero and not two");
    assert_eq!(skipped[0].widget, RECTLESS_WIDGET);
    assert_eq!(skipped[0].reason, WidgetDefect::RectMissing);
    assert_eq!(
        skipped[0].to_string(),
        "7 0 R: no usable /Rect (12.5.2)",
        "and it renders with the provenance ruling 10 requires"
    );

    // And the value really was written, which is what makes this the fourth
    // outcome rather than a failure wearing a report.
    assert!(editor.is_dirty());
}

/// The control: a text field whose one widget is well formed reports an
/// **empty** list.
///
/// Without this the only fill in the fixture is the damaged one, and "the
/// report was non-empty" would be indistinguishable from "the report is always
/// non-empty" — which is the shape a binding that inverted the condition, or
/// one that reported every widget it drew, would pass.
#[test]
fn filling_the_undamaged_text_field_reports_nothing() {
    let mut editor = DocumentEditor::new(fixture());
    let skipped = editor
        .fill_field("notes", "every widget of this field can be drawn")
        .expect("the value is taken");
    assert!(
        skipped.is_empty(),
        "an undamaged field skips no widget: {skipped:?}"
    );
    assert!(editor.is_dirty());
}

/// The undamaged fields fill with nothing skipped, so the report above is
/// about the fixture's one defect rather than about this engine.
#[test]
fn the_undamaged_fields_fill_with_nothing_skipped() {
    let mut editor = DocumentEditor::new(fixture());
    assert!(
        editor.set_checkbox("agree", true),
        "the checkbox's on state is /On rather than /Yes, which a naive filler \
         gets wrong"
    );
    assert!(editor.select_radio("colour", "red"));
    assert!(editor
        .fill_field("notes", "seen")
        .expect("taken")
        .is_empty());

    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    let reopened = CosDocument::open(saved).expect("the saved document opens");
    let states: Vec<String> = fields(&reopened)
        .into_iter()
        .map(|f| format!("{}={}", f.name, f.value.as_text()))
        .collect();
    assert_eq!(states, ["name=", "agree=On", "colour=red", "notes=seen"]);
}

/// 7.5.6: an incremental update appends, so the original bytes survive as a
/// prefix. Asserted on the fixture because every fill-and-save artefact in the
/// parity suite is one of these, on all four surfaces.
#[test]
fn an_incremental_save_keeps_the_original_bytes_as_a_prefix() {
    let original = fixture_bytes();
    let mut editor = DocumentEditor::new(fixture());
    assert!(editor.fill_field("name", "Ada Lovelace").is_ok());

    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });

    assert!(saved.len() > original.len(), "it grew");
    assert_eq!(
        &saved[..original.len()],
        &original[..],
        "and every original byte is where it was -- which is what makes a \
         signature over the original still cover what it covered (12.8.1)"
    );

    let reopened = CosDocument::open(saved).expect("the saved document opens");
    assert_eq!(reopened.ladder_level(), LadderLevel::Trust);
    let defects = validate(&reopened);
    assert!(
        defects.is_empty(),
        "the artefact is clean too: {:?}",
        defects.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
    );
}
