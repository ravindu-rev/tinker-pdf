//! FDF (ISO 32000-1 12.7.8) and XFDF, both directions (`tinker_pdf::form_data`).
//!
//! The roadmap row's exit criterion: **export then import reproduces every
//! terminal field's value**, for both formats — on `testdata/form-fields.pdf`
//! and on an equivalent form made here with `add_field`, whose names are
//! hierarchical (`applicant.name.first`) and whose values carry every
//! character the two formats have to escape. Beside it, the four hand-authored
//! fixtures in `tests/form_data/` are read and each value checked, and hostile
//! input is fed to both readers.
//!
//! What adjudicates: the round trips are this engine agreeing with itself, and
//! say so; what pins either format to its specification is the hand-authored
//! fixtures, written from the clauses' structure by hand rather than by this
//! writer (their README says what was and was not available to write them
//! from). No outside program reads or writes anything here (ruling 13).
//!
//! # Injections
//!
//! Put back one at a time, `cargo test --no-fail-fast -p tinker-pdf --test
//! form_data`, 26 September 2026:
//!
//! | Defect | Fires (of 14) |
//! | --- | --- |
//! | FDF export flattens the tree, writing each qualified name as one `/T` | 1 |
//! | FDF `/Kids` joined without the period | 4 |
//! | a literal string's `(` `)` `\` left unescaped | 2 |
//! | XFDF values with `&` and `<` left unescaped | 2 |
//! | XFDF's `&#13;` for a carriage return dropped | 2 |
//! | a button's state written as a text string instead of a name | 1 |
//! | a field with no value allowed to parent a group, so a reader loses it | 1 |
//!
//! The button injection is caught only because
//! `a_round_trip_reproduces_every_value_of_the_form_fixture` compares the FDF
//! read back with the data written. The import alone could not catch it —
//! `fill_field` takes a state's name whether it arrived as a name or a string —
//! so that assertion was added for this defect before the campaign ran; it is
//! the one about the file, which is what a reader other than this one sees.

use tinker_pdf::form_data::{
    apply, read_fdf, read_xfdf, FieldData, FormData, FormDataError, FormDataWarning,
};
use tinker_pdf::{
    Document, FieldValue, FillError, NewField, NewFieldKind, RadioButton, Rect, WriteMode,
    WriteOptions,
};

fn testdata(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../testdata/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("testdata/{name}: {e}"))
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/form_data/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("tests/form_data/{name}: {e}"))
}

fn text(value: &str) -> FieldValue {
    FieldValue::Text(value.to_string())
}

fn state(value: &str) -> FieldValue {
    FieldValue::State(value.to_string())
}

fn entry(name: &str, value: FieldValue) -> FieldData {
    FieldData {
        name: name.to_string(),
        value,
    }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// Every named terminal field's value, by name.
fn values(fields: &[tinker_pdf::Field]) -> Vec<(String, FieldValue)> {
    fields
        .iter()
        .filter(|f| !f.name.is_empty())
        .map(|f| (f.name.clone(), f.value.clone()))
        .collect()
}

// ---- the hand-authored fixtures --------------------------------------------

/// `form-fields.fdf`, written by hand for `testdata/form-fields.pdf`: a text
/// string, two names and a UTF-16BE string with a byte-order mark (7.9.2.2),
/// and no cross-reference table, which 12.7.8 makes optional.
#[test]
fn a_hand_written_fdf_is_read() {
    let data = read_fdf(&fixture("form-fields.fdf")).expect("the fixture reads");
    assert_eq!(
        data.fields,
        vec![
            entry("name", text("Ada Lovelace")),
            entry("agree", state("On")),
            entry("colour", state("blue")),
            entry("notes", text("Note \u{2014} 2")),
        ]
    );
    assert_eq!(data.source.as_deref(), Some("form-fields.pdf"));
    assert!(data.warnings.is_empty(), "{:?}", data.warnings);
}

/// `hierarchy.fdf`: `/Kids` two deep, an array value, escapes in a literal,
/// a field with no value, a field reached through a reference, a file
/// specification dictionary — and keys this reader does not read, each named.
#[test]
fn a_hand_written_fdf_tree_is_read_and_its_unread_keys_named() {
    let data = read_fdf(&fixture("hierarchy.fdf")).expect("the fixture reads");
    assert_eq!(
        data.fields,
        vec![
            entry("applicant.name.first", text("Ada")),
            entry("applicant.name.last", text("Lovelace")),
            entry("applicant.born", text("1815-12-10")),
            entry(
                "languages",
                FieldValue::Many(vec!["English".into(), "French".into()])
            ),
            entry("consent", state("Yes")),
            entry("remarks", text("a (bracketed) aside, and a back\\slash")),
            entry("untouched", FieldValue::None),
            entry("styled", text("plain")),
            entry("indirect", text("through a reference")),
        ]
    );
    assert_eq!(
        data.source.as_deref(),
        Some("application.pdf"),
        "/UF before /F (7.11.3)"
    );
    let named: Vec<(&str, &str)> = data
        .warnings
        .iter()
        .filter_map(|w| match w {
            FormDataWarning::NotRead { what, field } => Some((what.as_str(), field.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        named,
        vec![("Status", ""), ("Ff", "styled"), ("SetFf", "styled")]
    );
}

/// `form-fields.xfdf`: the same four values as XML, with a character
/// reference and an `<ids>` this reader names and does not read.
#[test]
fn a_hand_written_xfdf_is_read() {
    let data = read_xfdf(&fixture("form-fields.xfdf")).expect("the fixture reads");
    assert_eq!(
        data.fields,
        vec![
            entry("name", text("Ada Lovelace")),
            entry("agree", text("On")),
            entry("colour", text("blue")),
            entry("notes", text("Note \u{2014} 2")),
        ],
        "XFDF values are text: the field decides what `On` means"
    );
    assert_eq!(data.source.as_deref(), Some("form-fields.pdf"));
    assert_eq!(
        data.warnings,
        vec![FormDataWarning::NotRead {
            what: "ids".into(),
            field: String::new()
        }]
    );
}

/// `hierarchy.xfdf`: nested `<field>`s and a flat dotted name, which mean the
/// same thing; repeated `<value>`s; an empty `<field/>`; a rich-text value and
/// `<annots>`, named and not read.
#[test]
fn a_hand_written_xfdf_tree_is_read() {
    let data = read_xfdf(&fixture("hierarchy.xfdf")).expect("the fixture reads");
    assert_eq!(
        data.fields,
        vec![
            entry("applicant.name.first", text("Ada")),
            entry("applicant.name.last", text("Lovelace")),
            entry("applicant.born", text("1815-12-10")),
            entry(
                "languages",
                FieldValue::Many(vec!["English".into(), "French".into()])
            ),
            entry("consent", text("Yes")),
            entry("remarks", text("a <bracketed> aside & more")),
            entry("untouched", FieldValue::None),
            entry("address.city", text("London")),
            entry("styled", FieldValue::None),
        ]
    );
    assert_eq!(
        data.warnings,
        vec![
            FormDataWarning::NotRead {
                what: "value-richtext".into(),
                field: "styled".into()
            },
            FormDataWarning::NotRead {
                what: "annots".into(),
                field: String::new()
            },
        ]
    );
}

/// The hand-written files land in the document they were written for, and
/// the two formats land the same values.
#[test]
fn a_hand_written_file_imports_into_its_form() {
    for (label, data) in [
        ("fdf", read_fdf(&fixture("form-fields.fdf")).expect("reads")),
        (
            "xfdf",
            read_xfdf(&fixture("form-fields.xfdf")).expect("reads"),
        ),
    ] {
        let document = Document::open(testdata("form-fields.pdf")).expect("opens");
        let mut editor = document.editor();
        let skipped = apply(&mut editor, &data).expect("every field takes its value");
        assert_eq!(
            skipped.len(),
            1,
            "{label}: the fixture's one rectless widget"
        );
        let reopened = Document::open(editor.save(&incremental())).expect("reopens");
        let got = values(&reopened.form_fields());
        assert_eq!(
            got,
            vec![
                ("name".to_string(), text("Ada Lovelace")),
                ("agree".to_string(), state("On")),
                ("colour".to_string(), state("blue")),
                ("notes".to_string(), text("Note \u{2014} 2")),
            ],
            "{label}"
        );
    }
}

// ---- the round trip: the exit criterion ------------------------------------

/// Export, import into the unfilled file, and every terminal field's value is
/// the one exported — `testdata/form-fields.pdf`, both formats, both before
/// and after a fill.
#[test]
fn a_round_trip_reproduces_every_value_of_the_form_fixture() {
    let original = testdata("form-fields.pdf");
    let document = Document::open(original.clone()).expect("opens");
    let mut filled = document.editor();
    filled
        .set_field_values(&[
            ("name", "Grace (Hopper) \\ admiral"),
            ("agree", "On"),
            ("colour", "red"),
            ("notes", "line one\r\nline two & <three>"),
        ])
        .expect("filled");

    for source in [document.form_fields(), filled.fields()] {
        let data = FormData::from_fields(&source);
        let expected = values(&source);
        assert_eq!(data.fields.len(), 4, "every field of the fixture exported");

        let fdf = read_fdf(&data.to_fdf()).expect("our FDF reads back");
        // Exactly, not merely as the document receives it: a button's state
        // travels as a name (12.7.8.3.2), which is what a reader of FDF other
        // than this one looks for, and a text value as a string.
        assert_eq!(fdf.fields, data.fields, "FDF keeps each value's type");
        let xfdf = read_xfdf(data.to_xfdf().expect("representable").as_bytes())
            .expect("our XFDF reads back");
        for (label, imported) in [("fdf", fdf), ("xfdf", xfdf)] {
            let document = Document::open(original.clone()).expect("opens");
            let mut editor = document.editor();
            apply(&mut editor, &imported).expect("imports");
            let reopened = Document::open(editor.save(&incremental())).expect("reopens");
            assert_eq!(values(&reopened.form_fields()), expected, "{label}");
        }
    }
}

/// The equivalent form with everything the fixture lacks: a hierarchy three
/// deep, a list box, a combo box, a check box and a radio group made with
/// `add_field`, and text that needs every escape both formats have —
/// brackets and a backslash (7.3.4.2), non-Latin text (UTF-16BE, 7.9.2.2),
/// `&`, `<`, `"` and a carriage return (XML 1.0 2.11).
#[test]
fn a_round_trip_reproduces_a_hierarchical_form() {
    let base = Document::open(testdata("simple-text.pdf")).expect("opens");
    let mut editor = base.editor();
    let r = |y: f64| Rect {
        x0: 50.0,
        y0: y,
        x1: 250.0,
        y1: y + 20.0,
    };
    let text_field = |name: &str, y: f64| {
        NewField::new(
            name,
            NewFieldKind::Text {
                page: 0,
                rect: r(y),
                value: None,
                max_len: None,
            },
        )
    };
    for spec in [
        text_field("applicant.name.first", 100.0),
        text_field("applicant.name.last", 130.0),
        text_field("applicant.remarks", 160.0),
        NewField::new(
            "applicant.consent",
            NewFieldKind::Checkbox {
                page: 0,
                rect: Rect {
                    x0: 300.0,
                    y0: 100.0,
                    x1: 315.0,
                    y1: 115.0,
                },
                export: "Given".into(),
                checked: false,
            },
        ),
        NewField::new(
            "size",
            NewFieldKind::Radio {
                buttons: vec![
                    RadioButton {
                        export: "S".into(),
                        page: 0,
                        rect: Rect {
                            x0: 300.0,
                            y0: 140.0,
                            x1: 315.0,
                            y1: 155.0,
                        },
                    },
                    RadioButton {
                        export: "L".into(),
                        page: 0,
                        rect: Rect {
                            x0: 330.0,
                            y0: 140.0,
                            x1: 345.0,
                            y1: 155.0,
                        },
                    },
                ],
                selected: None,
            },
        ),
        NewField::new(
            "prefs.language",
            NewFieldKind::Choice {
                page: 0,
                rect: r(200.0),
                options: vec!["English".into(), "Fran\u{e7}ais".into()],
                combo: false,
                editable: false,
                value: None,
            },
        ),
        NewField::new(
            "prefs.city",
            NewFieldKind::Choice {
                page: 0,
                rect: r(230.0),
                options: vec!["London".into()],
                combo: true,
                editable: true,
                value: None,
            },
        ),
    ] {
        editor.add_field(&spec).expect("created");
    }
    let blank = Document::open(editor.save(&incremental())).expect("reopens");
    let mut editor = blank.editor();
    editor
        .set_field_values(&[
            ("applicant.name.first", "\u{674E} \u{767D}"),
            ("applicant.name.last", "O'Brien \"the (second)\" \\ jr"),
            ("applicant.remarks", "a & b < c\r\nd"),
            ("applicant.consent", "Given"),
            ("size", "L"),
            ("prefs.language", "Fran\u{e7}ais"),
            ("prefs.city", "Z\u{fc}rich"),
        ])
        .expect("filled");
    let fields = editor.fields();
    let expected = values(&fields);
    assert_eq!(expected.len(), 7);
    let data = FormData::from_fields(&fields);

    let fdf_bytes = data.to_fdf();
    // 12.7.8.3.2: `/T` is a partial name, so the qualified names went back
    // into a tree rather than into single `/T` strings with periods in them.
    let fdf_text = String::from_utf8_lossy(&fdf_bytes);
    assert!(
        !fdf_text.contains("(applicant.name"),
        "no qualified name is written as one /T: {fdf_text}"
    );
    let fdf = read_fdf(&fdf_bytes).expect("reads back");
    let xfdf = read_xfdf(data.to_xfdf().expect("representable").as_bytes()).expect("reads back");
    for (label, imported) in [("fdf", fdf), ("xfdf", xfdf)] {
        let mut editor = blank.editor();
        apply(&mut editor, &imported).expect("imports");
        let reopened = Document::open(editor.save(&incremental())).expect("reopens");
        assert_eq!(values(&reopened.form_fields()), expected, "{label}");
        assert!(reopened.validate().is_empty(), "{label}: strict-clean");
    }
}

/// Our own FDF opens without a repair: it carries the cross-reference table
/// 12.7.8 makes optional, so a reader that insists on one reads it too.
#[test]
fn a_written_fdf_is_a_well_formed_file() {
    let data = FormData {
        fields: vec![entry("x", text("y"))],
        source: Some("doc.pdf".into()),
        warnings: Vec::new(),
    };
    let bytes = data.to_fdf();
    assert!(bytes.starts_with(b"%FDF-1.2\n"));
    let cos = tinker_pdf::CosDocument::open(bytes.clone()).expect("opens");
    assert_eq!(
        cos.ladder_level(),
        tinker_pdf::LadderLevel::Trust,
        "the offsets are used as written"
    );
    let read = read_fdf(&bytes).expect("reads");
    assert_eq!(read.fields, data.fields);
    assert_eq!(read.source, data.source);
}

/// A name of more partial names than the written tree nests keeps its tail
/// whole and reads back as the same qualified name — and ten thousand
/// periods, which a caller can hand over, are written without recursing ten
/// thousand frames deep.
#[test]
fn a_name_deeper_than_the_written_tree_reads_back_whole() {
    let name = vec!["p"; 10_000].join(".");
    let data = FormData {
        fields: vec![entry(&name, text("deep")), entry("p.q", text("shallow"))],
        ..FormData::default()
    };
    let fdf = read_fdf(&data.to_fdf()).expect("reads");
    assert_eq!(fdf.fields, data.fields);
    assert!(fdf.warnings.is_empty(), "{:?}", fdf.warnings);
    let xfdf = read_xfdf(data.to_xfdf().expect("representable").as_bytes()).expect("reads");
    assert_eq!(xfdf.fields.len(), 2);
    assert!(xfdf.fields.contains(&entry(&name, text("deep"))));
}

/// Two shapes the fuzz target's round-trip property found in the writers
/// before this file was committed, run as a sweep on the stable toolchain
/// (the module comment of `fuzz/fuzz_targets/form_data.rs` has the numbers).
///
/// A field with **no value** and fields beneath it: a reader takes a node
/// with kids and no value for a group, so the writer used to lose the field.
/// It now stands beside the group, and both read back. A name with an
/// **empty partial name** — `.x`, `a..b`, `c.` — split into an empty `/T`,
/// which reads back as no name at all: `.x` came back as `x`. Such a name is
/// now written whole.
#[test]
fn what_the_round_trip_property_found_reads_back() {
    let data = FormData {
        fields: vec![
            entry("group", FieldValue::None),
            entry("group.kid", text("x")),
            entry(".x", text("leading")),
            entry("a..b", text("doubled")),
            entry("c.", text("trailing")),
        ],
        ..FormData::default()
    };
    let sorted = |mut fields: Vec<FieldData>| {
        fields.sort_by(|a, b| a.name.cmp(&b.name));
        fields
    };
    let fdf = read_fdf(&data.to_fdf()).expect("reads");
    assert_eq!(sorted(fdf.fields), sorted(data.fields.clone()));
    let xfdf = read_xfdf(data.to_xfdf().expect("representable").as_bytes()).expect("reads");
    assert_eq!(sorted(xfdf.fields), sorted(data.fields));
}

// ---- refusals --------------------------------------------------------------

/// Import is all-or-nothing and names the field that refused.
#[test]
fn an_import_the_form_will_not_take_writes_nothing() {
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let mut editor = document.editor();
    let unknown = FormData {
        fields: vec![
            entry("notes", text("fine")),
            entry("nowhere", text("no such field")),
        ],
        ..FormData::default()
    };
    let refusal = apply(&mut editor, &unknown).expect_err("refused");
    assert_eq!(refusal.field, "nowhere");
    assert_eq!(refusal.reason, FillError::NoSuchField);
    assert!(!editor.is_dirty(), "and `notes` rolled back");

    let many = FormData {
        fields: vec![entry(
            "notes",
            FieldValue::Many(vec!["a".into(), "b".into()]),
        )],
        ..FormData::default()
    };
    let refusal = apply(&mut editor, &many).expect_err("refused");
    assert_eq!(refusal.reason, FillError::ValueRefused);

    let off_state = FormData {
        fields: vec![entry("agree", state("Yes"))],
        ..FormData::default()
    };
    let refusal = apply(&mut editor, &off_state).expect_err("the box is /On");
    assert_eq!(refusal.reason, FillError::ValueRefused);
    assert!(!editor.is_dirty());
}

/// A control character XML 1.0 cannot carry is refused by name, not dropped.
#[test]
fn a_value_xml_cannot_carry_is_refused() {
    let data = FormData {
        fields: vec![entry("bell", text("ding\u{7}"))],
        ..FormData::default()
    };
    assert_eq!(
        data.to_xfdf(),
        Err(FormDataError::NotRepresentable("bell".into()))
    );
    // FDF carries it: a string is bytes.
    let back = read_fdf(&data.to_fdf()).expect("reads");
    assert_eq!(back.fields, data.fields);
}

/// Each reader refuses what is not its format, by name.
#[test]
fn the_wrong_format_is_refused_by_name() {
    assert_eq!(
        read_xfdf(b"<?xml version=\"1.0\"?><html/>"),
        Err(FormDataError::NotXfdf)
    );
    assert!(matches!(
        read_xfdf(b"<xfdf><fields>"),
        Err(FormDataError::Xml(_))
    ));
    assert!(matches!(
        read_xfdf(b"<!DOCTYPE xfdf [<!ENTITY a \"b\">]><xfdf/>"),
        Err(FormDataError::Xml(_))
    ));
    assert_eq!(
        read_fdf(&testdata("simple-text.pdf")),
        Err(FormDataError::NoFdfDictionary),
        "a PDF is not an FDF"
    );
    assert!(read_fdf(b"").is_err());
}

/// Ruling 1, on every prefix of every fixture and a byte flipped at every
/// position: both readers answer, neither panics.
#[test]
fn hostile_form_data_never_panics() {
    for name in [
        "form-fields.fdf",
        "hierarchy.fdf",
        "form-fields.xfdf",
        "hierarchy.xfdf",
    ] {
        let bytes = fixture(name);
        for end in 0..bytes.len() {
            let _ = read_fdf(&bytes[..end]);
            let _ = read_xfdf(&bytes[..end]);
        }
        for at in 0..bytes.len() {
            for flip in [0x01u8, 0x20, 0x80] {
                let mut mutated = bytes.clone();
                mutated[at] ^= flip;
                let _ = read_fdf(&mutated);
                let _ = read_xfdf(&mutated);
            }
        }
    }
    // A /Kids that contains itself, and one nested past the field tree's
    // depth cap: both stop, and say so.
    let cyclic = b"%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [2 0 R] >> >>\nendobj\n\
2 0 obj\n<< /T (loop) /Kids [2 0 R] >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";
    let data = read_fdf(cyclic).expect("reads");
    assert!(data
        .warnings
        .iter()
        .any(|w| matches!(w, FormDataWarning::TreeCut { .. })));
    // Three hundred fields deep, each its own object so the object parser's
    // nesting bound is not what stops it: the walk's own is.
    let mut deep = String::from("%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [2 0 R] >> >>\nendobj\n");
    for n in 2..302 {
        deep.push_str(&format!(
            "{n} 0 obj\n<< /T (d) /Kids [{} 0 R] >>\nendobj\n",
            n + 1
        ));
    }
    deep.push_str("302 0 obj\n<< /T (leaf) /V (x) >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    let data = read_fdf(deep.as_bytes()).expect("reads");
    assert!(data.fields.is_empty(), "the leaf is past the cap");
    assert!(data
        .warnings
        .iter()
        .any(|w| matches!(w, FormDataWarning::TreeCut { .. })));
}
