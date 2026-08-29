//! A form's own helpers, from `/Names /JavaScript` (7.7.4).
//!
//! **This is the milestone that made real calculating forms computable.** A
//! generated form keeps its arithmetic in document-level functions and its
//! `/AA /C` scripts call them, so until this landed the interpreter met the
//! first call to one, raised `ScriptError::UnknownName`, and refused the
//! whole pass — a correctly authored file this build could not compute at
//! all. Both directions are asserted below, in one test, because "it works
//! now" and "it did not work before" are two claims and only the pair is
//! evidence.
//!
//! # What is taken from document scope, and what is not
//!
//! Function definitions, and nothing else. Anything else there is
//! `ScriptError::NotADefinition`, named and refusing rather than skipped: a
//! skipped statement would build a name table silently missing whatever it
//! would have defined, and a calculation running against a half-built table
//! is the form that lies this whole subset is written against.
//!
//! `function` also stays a reserved word everywhere a *field* script is
//! parsed. The only thing a calculate action gained is the ability to
//! **call** a helper, never to declare one — one place declares, and it is
//! the one `ScriptPolicy` gates.
//!
//! # What is denied by default, and why it is not a refusal
//!
//! `Trigger::Document` is denied by `ScriptPolicy::default()`, and denying it
//! gives an **empty table** rather than a `CalcError::Refused`. A
//! document-level script is not something the pass is asked to run; it is a
//! resource the pass may consult. Refusing would make every form carrying a
//! `/Names /JavaScript` block uncomputable under the default policy, which is
//! most of them.
//!
//! # The policy defaults, flipped and counted against this suite
//!
//! `form_calculations.rs` runs the same six flips and reports a zero for
//! `document`, because nothing in that file calls a helper. This is the file
//! where that row stops being a zero.
//!
//! | Default flipped | Assertions that fired |
//! | --- | --- |
//! | `calculate` allowed → denied | 10 of 13 |
//! | `format` allowed → denied | 1 of 13 |
//! | `document` denied → allowed | 3 of 13 |
//! | `keystroke` denied → allowed | 0 of 13 |
//! | `validate` denied → allowed | 0 of 13 |
//! | `catalog` denied → allowed | 0 of 13 |
//!
//! The `catalog` zero is not a hole either, and
//! `allowing_the_catalog_trigger_changes_no_answer` is the assertion that
//! says so out loud rather than leaving it to be assumed.

use std::sync::Arc;

use tinker_pdf_cos::{
    calc, CalcError, CosDocument, DocumentEditor, Recalculation, ScriptError, ScriptPolicy,
    ScriptScope, Trigger, WriteMode, WriteOptions,
};

/// Hand-written bytes carry no cross-reference table, so they open through
/// the repair scanner; a rewrite gives the same graph inside a well-formed
/// file.
fn normalize(bytes: Vec<u8>) -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(bytes).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// The shape a generated form actually has: `net` and `rate` typed in, `vat`
/// and `total` computed by `/AA /C` scripts that call helpers the document
/// defines once in `/Names /JavaScript`.
fn helped(document_js: &str, vat_js: &str, total_js: &str) -> Arc<CosDocument> {
    normalize(
        format!(
            "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R
   /AcroForm << /Fields [10 0 R 11 0 R 12 0 R 13 0 R] /CO [12 0 R 13 0 R]
                /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >>
   /Names << /JavaScript 20 0 R >> >>
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
   /AA << /C << /S /JavaScript /JS ({vat_js}) >> >> >>
endobj
13 0 obj
<< /FT /Tx /T (total) /V (0) /Rect [10 160 290 180] /Subtype /Widget /Type /Annot
   /AA << /C << /S /JavaScript /JS ({total_js}) >> >> >>
endobj
20 0 obj
<< /Names [(helpers) 21 0 R] >>
endobj
21 0 obj
<< /S /JavaScript /JS ({document_js}) >>
endobj
trailer
<< /Size 22 /Root 1 0 R >>
%%EOF
"
        )
        .into_bytes(),
    )
}

/// The ordinary case: two helpers, two calls.
fn ordinary() -> Arc<CosDocument> {
    helped(
        "function vatOf\\(net, rate\\) { return net * rate; } \
         function withVat\\(net, vat\\) { return net + vat; }",
        "event.value = vatOf\\(getField('net').value, getField('rate').value\\);",
        "event.value = withVat\\(getField('net').value, getField('vat').value\\);",
    )
}

/// The policy that allows the document's own helpers on top of the default.
fn with_helpers() -> ScriptPolicy {
    ScriptPolicy::default().allow(Trigger::Document)
}

fn value_of(editor: &DocumentEditor, name: &str) -> String {
    editor
        .fields()
        .into_iter()
        .find(|f| f.name == name)
        .map(|f| f.value.as_text())
        .unwrap_or_else(|| panic!("no field {name}"))
}

// ---------------------------------------------------------------------------
// The exit criterion
// ---------------------------------------------------------------------------

/// **Both directions, in one test.** The form computes when the policy allows
/// its own helpers, and refuses by name when it does not.
///
/// The second half is the one that matters: it is the exact failure this
/// milestone existed to fix, and asserting it here means a future change that
/// makes document scripts run by default cannot pass unnoticed.
#[test]
fn a_form_computes_through_its_own_helpers_and_refuses_without_them() {
    let mut editor = DocumentEditor::new(ordinary());
    let pass = editor
        .recalculate_under(with_helpers())
        .expect("the helpers are in scope");
    assert_eq!(value_of(&editor, "vat"), "20");
    assert_eq!(value_of(&editor, "total"), "120");
    assert_eq!(
        pass.changed,
        vec![
            ("vat".to_string(), "20".to_string()),
            ("total".to_string(), "120".to_string()),
        ]
    );

    // And without the trigger, exactly what happened before this milestone.
    let mut editor = DocumentEditor::new(ordinary());
    assert_eq!(
        editor.recalculate(),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::UnknownName,
        })
    );
    assert_eq!(value_of(&editor, "vat"), "0", "nothing was written");
    assert!(!editor.is_dirty());
}

/// A helper reached through a format action too, and gated the same way.
#[test]
fn a_format_action_reaches_the_same_helpers() {
    let editor = DocumentEditor::new(formatted_by_helper());
    assert_eq!(
        calc::formatted_value_under(&editor, "shown", with_helpers())
            .map(|shown| shown.map(|text| text.text().to_string())),
        Ok(Some("84".to_string()))
    );
    assert_eq!(
        calc::formatted_value(&editor, "shown"),
        Err(CalcError::Script {
            field: "shown".to_string(),
            reason: ScriptError::UnknownName,
        })
    );
}

/// A one-field form whose `/AA /F` calls a document helper.
fn formatted_by_helper() -> Arc<CosDocument> {
    normalize(
        b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >>
   /Names << /JavaScript 20 0 R >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [10 0 R] >>
endobj
10 0 obj
<< /FT /Tx /T (shown) /V (42) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot
   /AA << /F << /S /JavaScript /JS (event.value = twice\\(event.value\\);) >> >> >>
endobj
20 0 obj
<< /Names [(helpers) 21 0 R] >>
endobj
21 0 obj
<< /S /JavaScript /JS (function twice\\(n\\) { return n * 2; }) >>
endobj
trailer
<< /Size 22 /Root 1 0 R >>
%%EOF
"
        .to_vec(),
    )
}

// ---------------------------------------------------------------------------
// What document scope will not take
// ---------------------------------------------------------------------------

/// Anything at document scope that is not a definition is named, and refuses
/// the pass. `var doc = this;` is what a generator writes and what this
/// refuses.
#[test]
fn a_statement_at_document_scope_is_named_and_refuses_the_pass() {
    let mut editor = DocumentEditor::new(helped(
        "var shared = 1; function vatOf\\(net, rate\\) { return net * rate; }",
        "event.value = 1;",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::DocumentScript {
            name: "helpers".to_string(),
            reason: ScriptError::NotADefinition,
        })
    );
    assert!(!editor.is_dirty(), "a refused pass writes nothing");

    // And under the default policy the same document computes, because the
    // table is never built: refusing to read a resource nobody asked for
    // would make an ordinary form uncomputable.
    let mut editor = DocumentEditor::new(helped(
        "var shared = 1;",
        "event.value = 1;",
        "event.value = 2;",
    ));
    assert!(editor.recalculate().is_ok());
}

/// A construct the subset excludes is excluded inside a helper too, because a
/// helper's body is parsed by the same productions a calculate action is.
#[test]
fn a_helper_body_refuses_what_a_field_script_refuses() {
    for body in ["eval('1')", "try { } catch \\(e\\) { }", "var r = /a/"] {
        let mut editor = DocumentEditor::new(helped(
            &format!("function f\\(\\) {{ {body}; }}"),
            "event.value = 1;",
            "event.value = 2;",
        ));
        let result = editor.recalculate_under(with_helpers());
        assert!(
            matches!(
                result,
                Err(CalcError::DocumentScript {
                    reason: ScriptError::Syntax | ScriptError::BadToken,
                    ..
                })
            ),
            "{body} was accepted: {result:?}"
        );
    }
}

/// A field script still cannot declare a function. `function` is reserved
/// everywhere a field script is parsed, and this milestone did not change it.
#[test]
fn a_field_script_still_cannot_declare_a_function() {
    let mut editor = DocumentEditor::new(helped(
        "function vatOf\\(net, rate\\) { return net * rate; }",
        "function sneak\\(\\) { return 1; } event.value = sneak\\(\\);",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::Syntax,
        })
    );
}

/// A document cannot redefine a builtin by declaring a function of that name:
/// the table is consulted after every builtin, never before.
#[test]
fn a_helper_cannot_take_a_builtin_name() {
    let mut editor = DocumentEditor::new(helped(
        "function getField\\(name\\) { return 999; }",
        "event.value = getField\\('net'\\).value;",
        "event.value = 2;",
    ));
    let pass = editor
        .recalculate_under(with_helpers())
        .expect("the real getField is still the one that runs");
    assert_eq!(value_of(&editor, "vat"), "100");
    assert!(pass.changed.iter().any(|(n, v)| n == "vat" && v == "100"));
}

// ---------------------------------------------------------------------------
// Termination
// ---------------------------------------------------------------------------

/// Recursion is refused **by name**, not bounded by a counter.
///
/// A depth cap was the alternative and was rejected for the reason the
/// cascade rule rejected bounded re-entry: it makes the answer depend on a
/// number nobody can predict from the file.
#[test]
fn a_recursive_helper_is_refused_by_name() {
    let mut editor = DocumentEditor::new(helped(
        "function down\\(n\\) { if \\(n <= 0\\) { return 0; } return down\\(n - 1\\); }",
        "event.value = down\\(3\\);",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::Recursion,
        })
    );
    assert!(!editor.is_dirty());
}

/// And through another function, which is the case a "does it call itself"
/// check written the obvious way would miss.
#[test]
fn a_mutually_recursive_pair_is_refused_by_name() {
    let mut editor = DocumentEditor::new(helped(
        "function a\\(n\\) { return b\\(n\\); } function b\\(n\\) { return a\\(n\\); }",
        "event.value = a\\(1\\);",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::Recursion,
        })
    );
}

/// A helper that does not terminate cheaply stops the pass, not the helper.
/// Its body charges the same steps a calculate action's does.
#[test]
fn a_helper_that_outruns_the_step_budget_refuses_the_pass() {
    let mut editor = DocumentEditor::new(helped(
        "function spin\\(\\) { var i = 0; while \\(true\\) { i = i + 1; } return i; }",
        "event.value = spin\\(\\);",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::OutOfSteps,
        })
    );
    assert!(
        !editor.is_dirty(),
        "a form that loops is left as it was saved"
    );
}

/// A helper's frame is its own: it cannot read or overwrite the locals of
/// whatever called it.
#[test]
fn a_helper_cannot_see_its_callers_locals() {
    let mut editor = DocumentEditor::new(helped(
        "function peek\\(\\) { return mine; }",
        "var mine = 7; event.value = peek\\(\\);",
        "event.value = 2;",
    ));
    assert_eq!(
        editor.recalculate_under(with_helpers()),
        Err(CalcError::Script {
            field: "vat".to_string(),
            reason: ScriptError::UnknownName,
        })
    );
}

// ---------------------------------------------------------------------------
// The table itself
// ---------------------------------------------------------------------------

/// `ScriptScope` is readable without running anything, so a host can say what
/// a document defined before deciding whether to allow it.
#[test]
fn the_name_table_can_be_read_without_running_a_line() {
    let mut scope = ScriptScope::empty();
    assert!(scope.is_empty());
    assert_eq!(
        scope.define("function a() { return 1; } function b() { return 2; }"),
        Ok(2)
    );
    assert_eq!(scope.len(), 2);
    assert_eq!(scope.names(), ["a", "b"]);
    assert!(scope.defines("a"));
    assert!(!scope.defines("c"));

    // A later declaration of a name replaces an earlier one, which is what a
    // reader does with two `function f` in one scope.
    assert_eq!(scope.define("function a() { return 3; }"), Ok(1));
    assert_eq!(scope.len(), 2);
    assert_eq!(scope.names(), ["a", "b"]);

    assert_eq!(scope.define("var x = 1;"), Err(ScriptError::NotADefinition));
    assert_eq!(scope.define(""), Ok(0));
}

/// The catalog's `/AA` (12.6.3 table 200) is the sixth trigger class and
/// **nothing in this build runs one**, so allowing it changes no answer
/// anywhere.
///
/// That is a stated fact rather than an omission. `WC`, `WS`, `DS`, `WP` and
/// `DP` are will-close, will-save, did-save, will-print and did-print — every
/// one of them needs an event a reader has no notion of, and none of them is
/// document-open, which is the one `/Names /JavaScript` covers. The bit is in
/// the policy because it names a real trigger class a host has to be able to
/// deny; this test is what stops it being quietly believed to gate something.
#[test]
fn allowing_the_catalog_trigger_changes_no_answer() {
    let with_catalog = with_helpers().allow(Trigger::Catalog);
    let mut a = DocumentEditor::new(ordinary());
    let mut b = DocumentEditor::new(ordinary());
    assert_eq!(
        a.recalculate_under(with_helpers()),
        b.recalculate_under(with_catalog)
    );

    let editor = DocumentEditor::new(ordinary());
    assert_eq!(
        calc::formatted_value_under(&editor, "vat", with_helpers()),
        calc::formatted_value_under(&editor, "vat", with_catalog)
    );
    assert_eq!(
        calc::formatted_value_under(&editor, "vat", with_catalog),
        Ok(None)
    );
}

/// A form whose field scripts call no helper computes identically under
/// either setting of the trigger, so allowing it costs a document nothing it
/// was not already getting.
#[test]
fn a_form_that_calls_no_helper_computes_the_same_either_way() {
    let doc = helped(
        "function unused\\(\\) { return 1; }",
        "event.value = getField\\('net'\\).value * getField\\('rate'\\).value;",
        "event.value = getField\\('net'\\).value + getField\\('vat'\\).value;",
    );
    let mut allowed = DocumentEditor::new(doc.clone());
    let mut denied = DocumentEditor::new(doc);
    assert_eq!(
        allowed.recalculate_under(with_helpers()),
        denied.recalculate()
    );
    assert_eq!(value_of(&allowed, "total"), "120");
    assert_eq!(value_of(&denied, "total"), "120");

    // A second pass over the same editor computes what is already there, so
    // it changes nothing — which is the contract, not an accident of this
    // fixture.
    assert_eq!(allowed.recalculate(), Ok(Recalculation::default()));
}
