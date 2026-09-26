//! Keystroke and validate, the two actions that need an event
//! (12.6.4.16 table 196).
//!
//! **This is why they sat surfaced-and-never-run.** A calculate action runs
//! against the document as it stands, so a reader can offer it one. A
//! keystroke action runs against what is being typed, where the caret is, and
//! whether this is the commit at the end — three facts a host has and a
//! reader does not — and a validate action runs against a value somebody
//! committed. Neither could ever fire implicitly, which is not a limit of the
//! interpreter but of what a document reader knows. So they are entry points
//! a host calls with an event it built, gated by `ScriptPolicy` like every
//! other trigger class.
//!
//! # What a refusal is, and what it is not
//!
//! A script setting `event.rc = false` is `EventVerdict::Refused`, and that is the
//! action **working**: a form that rejects a date in the wrong century is
//! doing its job. `CalcError` is for scripts that could not run at all. The
//! two are never the same answer, which is what lets a host tell "the form
//! says no" from "this build could not ask".
//!
//! # Where validate meets the all-or-nothing contract
//!
//! 12.7.2 puts a computed value through the field's own validate action
//! before it is committed, and `recalculate` does that at the last moment
//! nothing has been written. A refusal there aborts the **whole** pass —
//! `CalcError::Invalid` — because a form whose validation rejects one total
//! and whose other nine were written anyway is a document that disagrees with
//! itself, which is the outcome this module exists to prevent.
//!
//! Under a policy that denies validation the pass still runs, and every field
//! whose validate action it did **not** consult comes back in
//! `Recalculation::refused`. That is ruling 10 applied to a check rather than
//! a repair: a pass that silently skipped a form's own validation would be
//! indistinguishable from a pass over a form that has none, and the
//! difference is whether the numbers now in the document were ever looked at.
//!
//! # The policy defaults, flipped and counted against this suite
//!
//! | Default flipped | Assertions that fired |
//! | --- | --- |
//! | `calculate` allowed → denied | 4 of 13 |
//! | `validate` denied → allowed | 2 of 13 |
//! | `keystroke` denied → allowed | 1 of 13 |
//! | `format` allowed → denied | 0 of 13 |
//! | `document` denied → allowed | 0 of 13 |
//! | `catalog` denied → allowed | 0 of 13 |
//!
//! **The keystroke and validate rows are low on purpose and it is worth
//! saying why.** Most of this file passes an explicit policy, so a change to
//! the *default* cannot reach it — which is the right shape for a suite about
//! entry points a host calls deliberately. The two tests that do move are the
//! ones about the default itself, and they are what the rows measure. The
//! previous milestone's tables reported zeroes in both columns;
//! `form_calculations.rs` still does, and this is the file where they stop.

use std::sync::Arc;

use tinker_pdf_cos::{
    CalcError, CosDocument, DocumentEditor, EventVerdict, Keystroke, Recalculation, ScriptError,
    ScriptPolicy, Trigger, WriteMode, WriteOptions,
};

fn normalize(bytes: Vec<u8>) -> Arc<CosDocument> {
    let raw = Arc::new(CosDocument::open(bytes).expect("the fixture opens"));
    let written = DocumentEditor::new(raw).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });
    Arc::new(CosDocument::open(written).expect("the rewrite reopens"))
}

/// One typed field carrying whichever of `/AA /K` and `/AA /V` the test wants,
/// plus a plain field that carries neither.
fn typed(keystroke_js: &str, validate_js: &str) -> Arc<CosDocument> {
    let aa = format!(
        "/AA << {}{} >>",
        if keystroke_js.is_empty() {
            String::new()
        } else {
            format!("/K << /S /JavaScript /JS ({keystroke_js}) >> ")
        },
        if validate_js.is_empty() {
            String::new()
        } else {
            format!("/V << /S /JavaScript /JS ({validate_js}) >> ")
        },
    );
    normalize(
        format!(
            "%PDF-1.7
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
<< /FT /Tx /T (amount) /V (100) /Rect [10 150 190 170] /Subtype /Widget /Type /Annot
   {aa} >>
endobj
11 0 obj
<< /FT /Tx /T (plain) /V (x) /Rect [10 120 190 140] /Subtype /Widget /Type /Annot >>
endobj
trailer
<< /Size 12 /Root 1 0 R >>
%%EOF
"
        )
        .into_bytes(),
    )
}

/// An invoice whose `total` is calculated **and** validated: the shape 12.7.2
/// describes, and the one the all-or-nothing contract is tested against.
fn calculated_and_validated(validate_js: &str) -> Arc<CosDocument> {
    normalize(
        format!(
            "%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 11 0 R] /CO [11 0 R]
   /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>
endobj
2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [10 0 R 11 0 R] >>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
10 0 obj
<< /FT /Tx /T (net) /V (100) /Rect [10 250 290 270] /Subtype /Widget /Type /Annot >>
endobj
11 0 obj
<< /FT /Tx /T (total) /V (0) /Rect [10 190 290 210] /Subtype /Widget /Type /Annot
   /AA << /C << /S /JavaScript /JS (event.value = getField\\('net'\\).value * 2;) >>
          /V << /S /JavaScript /JS ({validate_js}) >> >> >>
endobj
trailer
<< /Size 12 /Root 1 0 R >>
%%EOF
"
        )
        .into_bytes(),
    )
}

fn allowing(trigger: Trigger) -> ScriptPolicy {
    ScriptPolicy::default().allow(trigger)
}

fn value_of(editor: &DocumentEditor, name: &str) -> String {
    editor
        .fields()
        .into_iter()
        .find(|f| f.name == name)
        .map(|f| f.value.as_text())
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn typing(change: &str) -> Keystroke {
    Keystroke {
        change: change.to_string(),
        selection: (3, 3),
        will_commit: false,
    }
}

// ---------------------------------------------------------------------------
// Keystroke
// ---------------------------------------------------------------------------

/// **Both directions.** A keystroke action that accepts, and one that
/// refuses, with the policy allowing it in each case.
///
/// The script is the commonest keystroke action there is — digits only, by
/// character range — and writing it that way is what found 11.8.5: this
/// engine compared two strings **by number** until this milestone, so `'!'`
/// was `NaN`, every comparison against it was false, and a digits-only field
/// accepted everything.
#[test]
fn a_keystroke_action_accepts_and_refuses() {
    let digits = "if \\(event.change != '' && \\(event.change < '0' || event.change > '9'\\)\\) \
         { event.rc = false; }";
    let editor = DocumentEditor::new(typed(digits, ""));
    let policy = allowing(Trigger::Keystroke);

    assert_eq!(
        editor.keystroke("amount", &typing("7"), policy),
        Ok(EventVerdict::Accepted("7".to_string()))
    );
    assert_eq!(
        editor.keystroke("amount", &typing("!"), policy),
        Ok(EventVerdict::Refused)
    );
    assert_eq!(
        editor.keystroke("amount", &typing("z"), policy),
        Ok(EventVerdict::Refused),
        "past '9', which numeric comparison could never have seen"
    );
    // A deletion offers an empty change, and the guard lets it through.
    assert_eq!(
        editor.keystroke("amount", &typing(""), policy),
        Ok(EventVerdict::Accepted(String::new()))
    );
    assert!(!editor.is_dirty(), "asking changes nothing");
}

/// A keystroke action may rewrite what is being typed, which is how every
/// upper-casing field in the wild works.
#[test]
fn a_keystroke_action_can_rewrite_the_change() {
    let editor = DocumentEditor::new(typed("event.change = event.change + '!';", ""));
    assert_eq!(
        editor.keystroke("amount", &typing("a"), allowing(Trigger::Keystroke)),
        Ok(EventVerdict::Accepted("a!".to_string()))
    );
}

/// The event a keystroke action sees is the one the host built, including the
/// selection and the commit flag — facts a reader could not invent, which is
/// the whole reason this needs an entry point of its own.
#[test]
fn the_event_carries_what_only_a_host_knows() {
    let editor = DocumentEditor::new(typed(
        "event.change = event.selStart + '/' + event.selEnd + '/' + event.willCommit \
         + '/' + event.value;",
        "",
    ));
    let event = Keystroke {
        change: "z".to_string(),
        selection: (2, 5),
        will_commit: true,
    };
    assert_eq!(
        editor.keystroke("amount", &event, allowing(Trigger::Keystroke)),
        Ok(EventVerdict::Accepted("2/5/true/100".to_string()))
    );
}

/// The default policy denies both, and the denial names the field rather than
/// silently accepting — which would tell a host the form checked when it did
/// not.
#[test]
fn the_default_policy_refuses_both_events_by_name() {
    let editor = DocumentEditor::new(typed("event.rc = false;", "event.rc = false;"));
    assert_eq!(
        editor.keystroke("amount", &typing("7"), ScriptPolicy::default()),
        Err(CalcError::Refused {
            trigger: Trigger::Keystroke,
            subject: "amount".to_string(),
        })
    );
    assert_eq!(
        editor.validate("amount", "7", ScriptPolicy::default()),
        Err(CalcError::Refused {
            trigger: Trigger::Validate,
            subject: "amount".to_string(),
        })
    );
}

/// A field carrying no action of that class accepts, under every policy:
/// there is nothing to consult, so the keystroke stands and the value is
/// taken.
#[test]
fn a_field_with_no_action_accepts_under_every_policy() {
    let editor = DocumentEditor::new(typed("", ""));
    for policy in [
        ScriptPolicy::nothing(),
        ScriptPolicy::default(),
        ScriptPolicy::everything(),
    ] {
        assert_eq!(
            editor.keystroke("plain", &typing("q"), policy),
            Ok(EventVerdict::Accepted("q".to_string()))
        );
        assert_eq!(
            editor.validate("plain", "anything", policy),
            Ok(EventVerdict::Accepted("anything".to_string()))
        );
    }
    assert_eq!(
        editor.keystroke("nope", &typing("q"), ScriptPolicy::everything()),
        Err(CalcError::NoSuchField)
    );
}

/// An event script must not change data. It gets the read-only host a format
/// action gets, so a write is `FieldRefused` rather than a silent staging
/// nobody applies.
#[test]
fn an_event_script_cannot_write_a_field() {
    let editor = DocumentEditor::new(typed("getField\\('plain'\\).value = 'moved';", ""));
    assert_eq!(
        editor.keystroke("amount", &typing("7"), allowing(Trigger::Keystroke)),
        Err(CalcError::Script {
            field: "amount".to_string(),
            reason: ScriptError::FieldRefused,
        })
    );
    assert_eq!(value_of(&editor, "plain"), "x");
}

/// A keystroke action that does not terminate cheaply stops on the call that
/// asked (ruling 1), and says which field.
#[test]
fn a_keystroke_action_that_never_terminates_is_refused() {
    let editor = DocumentEditor::new(typed("while \\(true\\) { }", ""));
    assert_eq!(
        editor.keystroke("amount", &typing("7"), allowing(Trigger::Keystroke)),
        Err(CalcError::Script {
            field: "amount".to_string(),
            reason: ScriptError::OutOfSteps,
        })
    );
}

// ---------------------------------------------------------------------------
// Validate
// ---------------------------------------------------------------------------

/// **Both directions**, asked directly rather than through a pass.
#[test]
fn a_validate_action_accepts_and_refuses() {
    let editor = DocumentEditor::new(typed(
        "",
        "if \\(event.value > 500\\) { event.rc = false; }",
    ));
    let policy = allowing(Trigger::Validate);

    assert_eq!(
        editor.validate("amount", "120", policy),
        Ok(EventVerdict::Accepted("120".to_string()))
    );
    assert_eq!(
        editor.validate("amount", "900", policy),
        Ok(EventVerdict::Refused)
    );
    assert!(!editor.is_dirty(), "asking changes nothing");
}

// ---------------------------------------------------------------------------
// Validate inside the pass
// ---------------------------------------------------------------------------

/// A validate action that refuses a computed value aborts the **whole** pass.
///
/// The field it names is the one that refused, and the editor is left exactly
/// as it was saved — not "correct so far", which is the outcome this module's
/// contract exists to prevent.
#[test]
fn a_refusing_validation_aborts_the_whole_pass() {
    let mut editor = DocumentEditor::new(calculated_and_validated(
        "if \\(event.value > 150\\) { event.rc = false; }",
    ));
    assert_eq!(
        editor.recalculate_under(allowing(Trigger::Validate)),
        Err(CalcError::Invalid {
            field: "total".to_string(),
            value: "200".to_string(),
        })
    );
    assert_eq!(value_of(&editor, "total"), "0", "nothing was written");
    assert!(!editor.is_dirty());

    // And an accepting one lets the same pass through, with nothing left
    // unconsulted.
    let mut editor = DocumentEditor::new(calculated_and_validated(
        "if \\(event.value > 5000\\) { event.rc = false; }",
    ));
    let pass = editor
        .recalculate_under(allowing(Trigger::Validate))
        .expect("the form accepts its own total");
    assert_eq!(value_of(&editor, "total"), "200");
    assert!(pass.refused.is_empty(), "every validation was run");
}

/// Under a policy that denies validation the pass still runs, and names every
/// field it wrote **without** checking (ruling 10).
///
/// The alternative is a pass that silently skips a form's own validation and
/// reads exactly like a pass over a form that has none.
#[test]
fn a_pass_names_the_validations_it_did_not_run() {
    let mut editor = DocumentEditor::new(calculated_and_validated("event.rc = false;"));
    let pass = editor
        .recalculate()
        .expect("the default policy computes without validating");
    assert_eq!(pass.refused, vec!["total".to_string()]);
    assert_eq!(
        value_of(&editor, "total"),
        "200",
        "written, and written unchecked"
    );

    // The same pass with validation allowed refuses outright, which is what
    // makes the list above a report of something that mattered.
    let mut editor = DocumentEditor::new(calculated_and_validated("event.rc = false;"));
    assert!(matches!(
        editor.recalculate_under(allowing(Trigger::Validate)),
        Err(CalcError::Invalid { .. })
    ));
}

/// A form whose calculated fields carry no validate action reports nothing,
/// under either policy: the list is what was skipped, not what was absent.
#[test]
fn a_form_with_no_validation_reports_none() {
    let mut editor = DocumentEditor::new(typed("", ""));
    assert_eq!(editor.recalculate(), Ok(Recalculation::default()));
    assert_eq!(
        editor.recalculate_under(ScriptPolicy::everything()),
        Ok(Recalculation::default())
    );
}

/// A validate action inside the pass reads the values the pass computed, not
/// the ones the file still holds — a total is checked against the inputs that
/// produced it.
#[test]
fn a_validation_inside_a_pass_sees_the_staged_values() {
    let mut editor = DocumentEditor::new(calculated_and_validated(
        "if \\(event.value != getField\\('net'\\).value * 2\\) { event.rc = false; }",
    ));
    let pass = editor
        .recalculate_under(allowing(Trigger::Validate))
        .expect("the total agrees with its input");
    assert_eq!(pass.changed, vec![("total".to_string(), "200".to_string())]);
}

/// And it cannot write through that host either: the staging map is sealed
/// once the pass has finished computing.
#[test]
fn a_validation_inside_a_pass_cannot_write() {
    let mut editor = DocumentEditor::new(calculated_and_validated(
        "getField\\('net'\\).value = 1; event.rc = true;",
    ));
    assert_eq!(
        editor.recalculate_under(allowing(Trigger::Validate)),
        Err(CalcError::Script {
            field: "total".to_string(),
            reason: ScriptError::FieldRefused,
        })
    );
    assert_eq!(value_of(&editor, "net"), "100");
    assert!(!editor.is_dirty());
}
