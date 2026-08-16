//! Running a form's calculations end to end (gap 27, option A).
//!
//! The claim this file exists to hold up is the one gap 27 names as the
//! deciding argument against building an interpreter at all: **a calculation
//! that partially applies is worse than none.** So the assertions are as much
//! about what is *not* written as about what is.
//!
//! The second claim is termination. A script is untrusted input from a
//! document (ruling 1), so a form that loops forever, cascades forever, or
//! carries a script the size of the file must leave the editor exactly as it
//! was — not eventually, but on the call that asked.

use std::sync::Arc;

use tinker_pdf_cos::{
    calc, fields, CalcError, CosDocument, DocumentEditor, FillError, ScriptError, WidgetDefect,
    WriteMode, WriteOptions,
};

/// Hand-written bytes carry no cross-reference table, so they open through the
/// repair scanner; a rewrite gives the same graph inside a well-formed file.
fn normalize(bytes: Vec<u8>) -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(bytes).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// An invoice: `net` and `rate` are typed in, `vat` is `net * rate` and
/// `total` is `net + vat`. `/CO` puts `vat` first, because `total` depends on
/// it — which is the whole reason `/CO` exists.
///
/// `total` is ReadOnly, as a calculated field is in every properly authored
/// form, so this fixture also pins that a calculation writes one and a user
/// does not (12.7.4.1 table 227).
fn invoice(order: &str, vat_script: &str, total_script: &str) -> Arc<CosDocument> {
    normalize(
        format!(
            "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 11 0 R 12 0 R 13 0 R]
   {order} /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300]
   /Annots [10 0 R 11 0 R 12 0 R 13 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (net) /V (100) /Rect [10 250 290 270] /Subtype /Widget /Type /Annot >>
endobj
11 0 obj
<< /FT /Tx /T (rate) /V (0.2) /Rect [10 220 290 240] /Subtype /Widget /Type /Annot >>
endobj
12 0 obj
<< /FT /Tx /T (vat) /V (0) /Rect [10 190 290 210] /Subtype /Widget /Type /Annot
   /AA << /C << /S /JavaScript /JS ({vat_script}) >> >> >>
endobj
13 0 obj
<< /FT /Tx /T (total) /V (0) /Ff 1 /Rect [10 160 290 180] /Subtype /Widget /Type /Annot
   /AA << /C << /S /JavaScript /JS ({total_script}) >> >> >>
endobj
trailer
<< /Size 14 /Root 1 0 R >>
%%EOF
"
        )
        .into_bytes(),
    )
}

/// The ordinary invoice: `/CO` correct, both scripts arithmetic over
/// `getField(...).value`.
fn ordinary() -> Arc<CosDocument> {
    invoice(
        "/CO [12 0 R 13 0 R]",
        "event.value = getField('net').value * getField('rate').value;",
        "event.value = getField('net').value + getField('vat').value;",
    )
}

fn value_of(editor: &DocumentEditor, name: &str) -> String {
    editor
        .fields()
        .into_iter()
        .find(|f| f.name == name)
        .map(|f| f.value.as_text())
        .unwrap_or_else(|| panic!("no field {name}"))
}

/// The bytes of every appearance stream the editor would save, so an
/// assertion can say what a widget actually draws rather than what its `/V`
/// claims.
fn saved(editor: &DocumentEditor) -> String {
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------------------
// The happy path, and the order
// ---------------------------------------------------------------------------

#[test]
fn a_calculated_total_is_computed_and_stored() {
    let mut editor = DocumentEditor::new(ordinary());
    let result = editor.recalculate().expect("the pass runs");

    assert_eq!(value_of(&editor, "vat"), "20");
    assert_eq!(value_of(&editor, "total"), "120");
    assert_eq!(
        result.changed,
        vec![
            ("vat".to_string(), "20".to_string()),
            ("total".to_string(), "120".to_string())
        ],
        "in the order the pass computed them"
    );
    assert!(result.skipped.is_empty());
    assert!(result.cascades_cut.is_empty());
}

/// The value and the drawing have to agree, which is the whole purpose of the
/// fill layer this pass writes through.
#[test]
fn the_appearance_follows_the_computed_value() {
    let mut editor = DocumentEditor::new(ordinary());
    editor.recalculate().expect("the pass runs");
    let text = saved(&editor);
    assert!(
        text.contains("(120) Tj"),
        "the total is drawn, not just stored"
    );
    assert!(text.contains("(20) Tj"), "and so is the VAT");
}

/// `/CO` order **is** the semantics. Reversing it computes `total` from a
/// `vat` that has not been calculated yet, so the total comes out stale — 100
/// rather than 120 — and every value in the file is individually defensible.
/// That is the failure this ordering exists to prevent, and it is invisible
/// without an assertion on the number.
#[test]
fn the_calculation_order_decides_whether_a_total_is_stale() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [13 0 R 12 0 R]",
        "event.value = getField('net').value * getField('rate').value;",
        "event.value = getField('net').value + getField('vat').value;",
    ));
    editor.recalculate().expect("the pass runs");

    assert_eq!(value_of(&editor, "vat"), "20");
    assert_eq!(
        value_of(&editor, "total"),
        "100",
        "total ran first, so it added a VAT of zero -- which is exactly what \
         honouring /CO prevents"
    );
}

/// A producer that leaves `/CO` out still has a calculating form, and document
/// order is the only other order the file offers.
#[test]
fn a_form_without_a_calculation_order_still_calculates() {
    let mut editor = DocumentEditor::new(invoice(
        "",
        "event.value = getField('net').value * getField('rate').value;",
        "event.value = getField('net').value + getField('vat').value;",
    ));
    editor.recalculate().expect("the pass runs");
    assert_eq!(value_of(&editor, "total"), "120");
}

/// `AFSimple_Calculate` is the most common calculate script there is, and the
/// form Acrobat's own field dialogue writes uses `new Array`.
#[test]
fn af_simple_calculate_drives_a_real_form() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R 13 0 R]",
        "event.value = getField('net').value * getField('rate').value;",
        "AFSimple_Calculate(\"SUM\", new Array(\"net\", \"vat\"));",
    ));
    editor.recalculate().expect("the pass runs");
    assert_eq!(value_of(&editor, "total"), "120");
}

/// A calculated total is ReadOnly precisely so that nothing but the script
/// writes it. Both halves of that sentence are assertions.
#[test]
fn a_read_only_total_is_written_by_a_calculation_and_not_by_a_user() {
    let mut editor = DocumentEditor::new(ordinary());
    assert_eq!(
        editor.fill_field("total", "999"),
        Err(FillError::ValueRefused),
        "12.7.4.1: ReadOnly binds the user"
    );
    assert_eq!(value_of(&editor, "total"), "0", "and nothing was written");

    editor.recalculate().expect("the pass runs");
    assert_eq!(value_of(&editor, "total"), "120");
}

/// A pass that computes what is already stored writes nothing at all: there is
/// no appearance to regenerate for a value that did not move, and an editor
/// that reports itself dirty after a no-op recalculation makes every caller
/// save a file it did not need to.
#[test]
fn a_calculation_that_changes_nothing_writes_nothing() {
    let mut editor = DocumentEditor::new(ordinary());
    editor.recalculate().expect("the first pass runs");

    let mut second = DocumentEditor::new(ordinary());
    second.recalculate().expect("the first pass runs");
    let before = saved(&second);
    let result = second.recalculate().expect("the second pass runs");
    assert!(result.changed.is_empty());
    assert_eq!(saved(&second), before, "byte for byte");
}

/// Most documents. A form with no `/AA` anywhere costs one tree walk and
/// changes nothing, which is what a caller that recalculates unconditionally
/// depends on.
#[test]
fn a_form_with_no_calculations_is_left_alone() {
    let mut editor = DocumentEditor::new(normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R] >>
endobj
10 0 obj
<< /FT /Tx /T (plain) /V (x) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot >>
endobj
trailer
<< /Size 11 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    ));
    let result = editor.recalculate().expect("the pass runs");
    assert!(result.changed.is_empty());
    assert!(!editor.is_dirty(), "nothing was touched");
}

// ---------------------------------------------------------------------------
// Nothing, or everything
// ---------------------------------------------------------------------------

/// The failure gap 27 is written about. `vat` computes fine; `total`'s script
/// names a field that is not there, so the pass refuses — and `vat` must be
/// exactly as it was saved, not "correct so far".
#[test]
fn a_calculation_that_fails_on_the_second_field_writes_neither() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R 13 0 R]",
        "event.value = getField('net').value * getField('rate').value;",
        "event.value = getField('net').value + getField('missing').value;",
    ));
    let before = saved(&editor);

    let error = editor.recalculate().expect_err("the pass refuses");
    assert_eq!(
        error,
        CalcError::Script {
            field: "total".to_string(),
            reason: ScriptError::NoSuchField,
        }
    );

    assert_eq!(
        value_of(&editor, "vat"),
        "0",
        "the first field is untouched"
    );
    assert_eq!(value_of(&editor, "total"), "0");
    assert!(!editor.is_dirty(), "the editor has no overlay at all");
    assert_eq!(saved(&editor), before, "byte for byte");
}

/// The same proof one level up: a script that writes another field directly
/// and *then* fails leaves that write undone too. This is the case a staging
/// map is for — the write never reached the document to be rolled back.
#[test]
fn a_direct_write_before_a_failure_is_also_undone() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R]",
        "getField('net').value = 7; event.value = nonsense;",
        "",
    ));
    let before = saved(&editor);

    let error = editor.recalculate().expect_err("the pass refuses");
    assert_eq!(
        error,
        CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::UnknownName,
        }
    );
    assert_eq!(value_of(&editor, "net"), "100", "the direct write is gone");
    assert!(!editor.is_dirty());
    assert_eq!(saved(&editor), before);
}

/// An edit made before the pass survives a pass that refuses. The transaction
/// restores what the *pass* did, not what the editor had done beforehand.
#[test]
fn a_refused_pass_leaves_earlier_edits_alone() {
    let mut editor = DocumentEditor::new(invoice("/CO [12 0 R]", "event.value = nonsense;", ""));
    editor.fill_field("net", "250").expect("the fill applies");

    assert!(editor.recalculate().is_err());
    assert_eq!(value_of(&editor, "net"), "250");
}

/// A script whose result the fill layer will not take refuses the whole pass
/// rather than storing a truncated value.
#[test]
fn a_value_the_field_would_refuse_refuses_the_pass() {
    let mut editor = DocumentEditor::new(normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] /CO [10 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R] >>
endobj
10 0 obj
<< /FT /Tx /T (code) /V (ab) /MaxLen 4 /Rect [10 150 190 170]
   /Subtype /Widget /Type /Annot
   /AA << /C << /JS (event.value = 'far too long for this field';) >> >> >>
endobj
trailer
<< /Size 11 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    ));

    assert_eq!(
        editor.recalculate(),
        Err(CalcError::Script {
            field: "code".to_string(),
            reason: ScriptError::FieldRefused,
        })
    );
    assert_eq!(value_of(&editor, "code"), "ab");
    assert!(!editor.is_dirty());
}

/// A widget with no `/Rect` is reported, not refused (ruling 2 degrades,
/// ruling 10 names) — so a calculation over a damaged file still computes, and
/// still says what it could not draw.
#[test]
fn a_widget_that_cannot_be_drawn_is_reported_rather_than_refused() {
    let mut editor = DocumentEditor::new(normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R] /CO [20 0 R]
   /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R 21 0 R 22 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (net) /V (5) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot >>
endobj
20 0 obj
<< /FT /Tx /T (total) /V (0) /Kids [21 0 R 22 0 R]
   /AA << /C << /JS (event.value = getField('net').value * 2;) >> >> >>
endobj
21 0 obj
<< /Parent 20 0 R /Subtype /Widget /Type /Annot /Rect [10 120 190 140] >>
endobj
22 0 obj
<< /Parent 20 0 R /Subtype /Widget /Type /Annot >>
endobj
trailer
<< /Size 23 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    ));

    let result = editor.recalculate().expect("the pass runs");
    assert_eq!(value_of(&editor, "total"), "10");
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].reason, WidgetDefect::RectMissing);
}

// ---------------------------------------------------------------------------
// The cascade
// ---------------------------------------------------------------------------

/// Two fields whose calculations write each other. One pass, each calculate
/// action once, so it terminates by construction rather than by a counter —
/// and the point at which the cascade was cut is reported instead of being
/// iterated on.
#[test]
fn a_calculation_cycle_terminates_and_says_where_it_was_cut() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R 13 0 R]",
        "event.value = 1; getField('rate').value = 5;",
        "event.value = 2; getField('vat').value = 9;",
    ));

    let result = editor.recalculate().expect("the pass terminates");
    assert_eq!(
        result.cascades_cut,
        vec!["vat".to_string()],
        "total wrote vat after vat's own calculation had already run"
    );
    assert_eq!(
        value_of(&editor, "vat"),
        "9",
        "the later write wins, deterministically"
    );
    assert_eq!(value_of(&editor, "total"), "2");
    assert_eq!(value_of(&editor, "rate"), "5");
}

/// A write to a field whose calculation has *not* yet run is not a cut: it is
/// the ordinary way one calculation feeds another, and reporting it would
/// make the signal useless.
#[test]
fn a_write_that_a_later_calculation_will_see_is_not_a_cut() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R 13 0 R]",
        "getField('net').value = 200; event.value = 1;",
        "event.value = getField('net').value;",
    ));
    let result = editor.recalculate().expect("the pass runs");
    assert!(result.cascades_cut.is_empty());
    assert_eq!(value_of(&editor, "total"), "200");
}

// ---------------------------------------------------------------------------
// Termination, on hostile input
// ---------------------------------------------------------------------------

/// The budget is the reason this returns at all. Without it the call does not
/// come back, and no assertion in this file would ever run.
#[test]
fn a_script_that_never_terminates_refuses_the_pass() {
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R]",
        "var i = 0; while (true) { i = i + 1; } event.value = i;",
        "",
    ));
    assert_eq!(
        editor.recalculate(),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::OutOfSteps,
        })
    );
    assert!(!editor.is_dirty());
}

/// Per-script and per-pass budgets are different bounds. Two scripts that each
/// fit inside `MAX_SCRIPT_STEPS` can still exhaust `MAX_CALC_STEPS` between
/// them, which is what stops a document from multiplying its way past the
/// per-script cap by carrying more scripts.
#[test]
fn the_pass_budget_binds_across_scripts() {
    let loop_script = "var i = 0; while (i < 100000) { i = i + 1; } event.value = i;";
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R 13 0 R]",
        loop_script,
        "event.value = 1;",
    ));
    let error = editor.recalculate().expect_err("the pass refuses");
    assert!(
        matches!(
            error,
            CalcError::Script {
                reason: ScriptError::OutOfSteps,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(!editor.is_dirty());
}

/// A script too big to surface is a script this build will not run a prefix
/// of, so the pass refuses rather than computing part of the form.
#[test]
fn an_oversized_script_refuses_the_pass() {
    let huge = "x".repeat(70_000);
    let mut editor = DocumentEditor::new(invoice(
        "/CO [12 0 R]",
        &format!("event.value = 1; /* {huge} */"),
        "",
    ));
    assert_eq!(
        editor.recalculate(),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::TooLong,
        })
    );
    assert!(!editor.is_dirty());
}

/// Ruling 1 over the whole pass: arbitrary script text in a real document is
/// an error, never a panic and never a hang.
#[test]
fn hostile_scripts_in_a_document_never_panic() {
    let sources = [
        "",
        "(((",
        "eval('x')",
        "while(1){}",
        "var s='xx'; while(1){s=s+s;}",
        "event.value=event.value+event.value+event.value",
        "getField()",
        "AFSimple_Calculate()",
        "AFNumber_Format(1e400,1e400,1e400,1e400,1e400,1e400)",
        "AFDate_Format(-1)",
        "AFSpecial_Format(99)",
        "this.x.y.z",
        "\u{0}",
    ];
    for source in sources {
        let mut editor = DocumentEditor::new(invoice("/CO [12 0 R 13 0 R]", source, source));
        let _ = editor.recalculate();
    }
}

// ---------------------------------------------------------------------------
// Format actions
// ---------------------------------------------------------------------------

/// 12.7.3.3 keeps the value and its appearance apart, and so does this: the
/// format action produces what a viewer shows and `/V` keeps the number. A
/// `/V` of "GBP 1,234.00" is a form whose export is unusable.
#[test]
fn a_format_action_produces_a_display_string_and_changes_nothing() {
    let editor = DocumentEditor::new(normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R] >>
endobj
10 0 obj
<< /FT /Tx /T (amount) /V (1234.5) /Rect [10 150 190 170]
   /Subtype /Widget /Type /Annot
   /AA << /F << /JS (AFNumber_Format\\(2, 0, 0, 0, \"GBP \", true\\);) >> >> >>
endobj
trailer
<< /Size 11 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    ));

    assert_eq!(
        calc::formatted_value(&editor, "amount").expect("the action runs"),
        Some("GBP 1,234.50".to_string())
    );
    assert_eq!(value_of(&editor, "amount"), "1234.5", "/V is untouched");
    assert!(!editor.is_dirty());
}

#[test]
fn a_field_with_no_format_action_formats_to_nothing() {
    let editor = DocumentEditor::new(ordinary());
    assert_eq!(calc::formatted_value(&editor, "total"), Ok(None));
    assert_eq!(
        calc::formatted_value(&editor, "nope"),
        Err(CalcError::NoSuchField)
    );
}

/// A format action that tries to change data is refused. A format event
/// changing a value is a defect wherever it appears, and the host it runs
/// against simply has no door.
#[test]
fn a_format_action_cannot_write_a_field() {
    let editor = DocumentEditor::new(normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 11 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R 11 0 R] >>
endobj
10 0 obj
<< /FT /Tx /T (a) /V (1) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot >>
endobj
11 0 obj
<< /FT /Tx /T (b) /V (2) /Rect [10 120 190 140] /Subtype /Widget /Type /Annot
   /AA << /F << /JS (getField('a').value = 99;) >> >> >>
endobj
trailer
<< /Size 12 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    ));

    assert_eq!(
        calc::formatted_value(&editor, "b"),
        Err(CalcError::Script {
            field: "b".to_string(),
            reason: ScriptError::FieldRefused,
        })
    );
    assert_eq!(value_of(&editor, "a"), "1");
}

// ---------------------------------------------------------------------------
// The reader underneath
// ---------------------------------------------------------------------------

/// Option B's surface still answers on a document option A has run over: a
/// host that wants to show the scripts rather than run them is not deprived
/// of them by the interpreter existing.
#[test]
fn the_scripts_are_still_readable_as_data() {
    let doc = ordinary();
    let found = fields(&doc);
    let total = found.iter().find(|f| f.name == "total").expect("total");
    assert!(total.scripts.calculate.is_some());
    assert_eq!(tinker_pdf_cos::calculation_order(&doc).len(), 2);
    assert_eq!(tinker_pdf_cos::script_summary(&doc).calculate_actions, 2);
}
