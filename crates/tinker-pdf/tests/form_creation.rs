//! Creating form fields: `DocumentEditor::add_field` (12.7.3, 12.7.4).
//!
//! The roadmap row's exit criterion, one assertion group per clause of it:
//! each of the four kinds — text, check box, radio group, choice — is
//! **created**, **visible** to `fields()` in the editor that created it,
//! **filled** through `fill_field`, **saved**, **reopened**, **read back** with
//! the value it was given, **rendered** non-blank where it should be and blank
//! where it should not, and the saved file is **clean** under the strict
//! structural validator.
//!
//! Two fixtures, because a form has two shapes to be created into: a file with
//! no `/AcroForm` at all (`simple-text.pdf`), which has to be given one and a
//! `/DR` with a font in it, and a file whose form already has a `/DR /Helv`
//! (`form-fields.pdf`), which has to be joined rather than shadowed.
//!
//! What adjudicates the rendering is this engine's own rasterizer; what
//! adjudicates the structure is its own strict validator (ruling 13). Neither
//! says that another reader draws these fields the same way.
//!
//! # Injections
//!
//! Put back one at a time, `cargo test --no-fail-fast -p tinker-pdf --test
//! form_creation`, 26 September 2026:
//!
//! | Defect | Fires (of 9) |
//! | --- | --- |
//! | a created check box's on appearance draws no tick (`if on` → `if false`) | 1 |
//! | `/Ff` left off a terminal field when zero, so it inherits the parent's | 1 |
//! | the created field not added to `/Fields` | 5 |
//! | a text field's appearance laid out against the file rather than the editor, so the `/DR` font it just added is unseen | 1 |
//! | a button fill accepting a state no widget offers | 1 |
//! | every radio widget showing its own on state rather than `/Off` | 1 |

use std::sync::Arc;

use tinker_pdf::{
    AddFieldError, Document, FieldKind, FieldValue, FontProvider, NewField, NewFieldKind,
    RadioButton, Rect, RenderOptions, SimpleFontProvider, WriteMode, WriteOptions,
};

mod render_support;
use render_support::curvy_font;

fn testdata(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../testdata/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("testdata/{name}: {e}"))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect { x0, y0, x1, y1 }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

fn rewrite() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    }
}

/// The four kinds, placed in empty parts of page 0 of an A4 page.
fn four_kinds() -> Vec<NewField> {
    vec![
        NewField::new(
            "applicant",
            NewFieldKind::Text {
                page: 0,
                rect: rect(50.0, 400.0, 300.0, 425.0),
                value: None,
                max_len: Some(40),
            },
        ),
        NewField::new(
            "consent",
            NewFieldKind::Checkbox {
                page: 0,
                rect: rect(50.0, 350.0, 70.0, 370.0),
                export: "Agreed".to_string(),
                checked: false,
            },
        ),
        NewField::new(
            "size",
            NewFieldKind::Radio {
                buttons: vec![
                    RadioButton {
                        export: "small".to_string(),
                        page: 0,
                        rect: rect(50.0, 300.0, 70.0, 320.0),
                    },
                    RadioButton {
                        export: "large".to_string(),
                        page: 0,
                        rect: rect(100.0, 300.0, 120.0, 320.0),
                    },
                ],
                selected: None,
            },
        ),
        NewField::new(
            "country",
            NewFieldKind::Choice {
                page: 0,
                rect: rect(50.0, 240.0, 250.0, 262.0),
                options: vec![
                    "France".to_string(),
                    "Japan".to_string(),
                    "Peru".to_string(),
                ],
                combo: true,
                editable: false,
                value: None,
            },
        ),
    ]
}

/// The four values filled into [`four_kinds`], by name.
const FILLS: [(&str, &str); 4] = [
    ("applicant", "Ada Lovelace"),
    ("consent", "Agreed"),
    ("size", "large"),
    ("country", "Japan"),
];

fn value_of(document: &Document, name: &str) -> FieldValue {
    document
        .form_fields()
        .into_iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("{name} is a field of the saved file"))
        .value
}

/// How many pixels inside `r` (page space, one pixel a point) are not white.
fn inked(bitmap: &tinker_pdf::Bitmap, page_height: f64, r: Rect) -> usize {
    let components = bitmap.components();
    let mut count = 0;
    let (x0, x1) = (r.x0.ceil() as u32 + 1, r.x1.floor() as u32 - 1);
    let (top, bottom) = (
        (page_height - r.y1).ceil() as u32 + 1,
        (page_height - r.y0).floor() as u32 - 1,
    );
    for y in top..bottom {
        for x in x0..x1 {
            let at = y as usize * bitmap.stride + x as usize * components;
            if bitmap
                .data
                .get(at..at + 3)
                .is_some_and(|p| p.iter().any(|&c| c < 200))
            {
                count += 1;
            }
        }
    }
    count
}

/// The whole exit criterion, on a file that has no form at all.
#[test]
fn every_kind_is_created_found_filled_saved_reopened_and_read_back() {
    for fixture in ["simple-text.pdf", "form-fields.pdf"] {
        let document = Document::open(testdata(fixture)).expect("the fixture opens");
        let before = document.form_fields().len();
        let mut editor = document.editor();

        for spec in four_kinds() {
            editor
                .add_field(&spec)
                .unwrap_or_else(|e| panic!("{fixture}: {} refused: {e}", spec.name));
        }

        // Visible in the same editor, before anything is saved, with the kind
        // each spec asked for.
        let fields = editor.fields();
        assert_eq!(fields.len(), before + 4, "{fixture}: four more fields");
        let kind = |name: &str| {
            fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{fixture}: fields() sees {name}"))
                .kind
        };
        assert_eq!(kind("applicant"), FieldKind::Text);
        assert_eq!(kind("consent"), FieldKind::Checkbox);
        assert_eq!(kind("size"), FieldKind::Radio);
        assert_eq!(kind("country"), FieldKind::ComboBox);
        let size = fields.iter().find(|f| f.name == "size").expect("size");
        assert_eq!(size.widgets.len(), 2, "one widget per radio button");
        let country = fields
            .iter()
            .find(|f| f.name == "country")
            .expect("country");
        assert_eq!(country.options, vec!["France", "Japan", "Peru"]);

        // Filled through the ordinary door, every kind of them.
        for (name, value) in FILLS {
            let skipped = editor
                .fill_field(name, value)
                .unwrap_or_else(|e| panic!("{fixture}: filling {name}: {e}"));
            assert!(
                skipped.is_empty(),
                "{fixture}: every widget of {name} drawn"
            );
        }

        for (label, options) in [("incremental", incremental()), ("rewrite", rewrite())] {
            let saved = editor.save(&options);
            let reopened = Document::open(saved).expect("the saved file reopens");

            assert_eq!(
                value_of(&reopened, "applicant"),
                FieldValue::Text("Ada Lovelace".to_string()),
                "{fixture} {label}"
            );
            assert_eq!(
                value_of(&reopened, "consent"),
                FieldValue::State("Agreed".to_string()),
                "{fixture} {label}"
            );
            assert_eq!(
                value_of(&reopened, "size"),
                FieldValue::State("large".to_string()),
                "{fixture} {label}"
            );
            assert_eq!(
                value_of(&reopened, "country"),
                FieldValue::Text("Japan".to_string()),
                "{fixture} {label}"
            );

            let defects = reopened.validate();
            assert!(
                defects.is_empty(),
                "{fixture} {label}: the strict validator finds nothing: {:?}",
                defects.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
            );
        }
    }
}

/// Rendered non-blank where a value is shown, and blank where none is.
///
/// Before the fill: the empty text field and the empty combo box draw nothing
/// inside their rectangles, while the check box and both radio buttons draw a
/// frame — an unticked box still shows where it is. After it: the text is
/// there, the tick is there, and the selected radio button carries more ink
/// than the one left off.
///
/// The created fields name Helvetica, which is not embedded, so the page is
/// drawn with the synthetic face `render_support` builds standing in for it —
/// the way `determinism.rs` draws a document that embeds no face. Without one
/// this build has no outline for a standard font and the text would be
/// correctly placed and invisible.
#[test]
fn a_created_field_draws_what_it_holds() {
    let document = Document::open(testdata("simple-text.pdf")).expect("opens");
    let mut editor = document.editor();
    for spec in four_kinds() {
        editor.add_field(&spec).expect("created");
    }

    let face: Arc<dyn FontProvider> = Arc::new(SimpleFontProvider::new(curvy_font()));
    let render = |bytes: Vec<u8>| {
        let document = Document::open(bytes)
            .expect("reopens")
            .with_fonts(Arc::clone(&face));
        let page = document.page(0).expect("a page");
        let height = page.size().1;
        (page.render(&RenderOptions::default()), height)
    };

    let (empty, height) = render(editor.save(&incremental()));
    assert_eq!(
        inked(&empty, height, rect(50.0, 400.0, 300.0, 425.0)),
        0,
        "an empty text field draws nothing"
    );
    let box_off = inked(&empty, height, rect(50.0, 350.0, 70.0, 370.0));
    assert!(box_off > 0, "an unticked box still draws its frame");
    let small_off = inked(&empty, height, rect(50.0, 300.0, 70.0, 320.0));
    let large_off = inked(&empty, height, rect(100.0, 300.0, 120.0, 320.0));
    assert!(
        small_off > 0 && large_off > 0,
        "both radio frames are drawn"
    );

    for (name, value) in FILLS {
        editor.fill_field(name, value).expect("filled");
    }
    let (filled, height) = render(editor.save(&incremental()));
    assert!(
        inked(&filled, height, rect(50.0, 400.0, 300.0, 425.0)) > 20,
        "the text is drawn"
    );
    assert!(
        inked(&filled, height, rect(50.0, 240.0, 250.0, 262.0)) > 20,
        "the chosen option is drawn"
    );
    let box_on = inked(&filled, height, rect(50.0, 350.0, 70.0, 370.0));
    assert!(
        box_on > box_off + 10,
        "the tick adds ink: {box_on} against {box_off}"
    );
    let small = inked(&filled, height, rect(50.0, 300.0, 70.0, 320.0));
    let large = inked(&filled, height, rect(100.0, 300.0, 120.0, 320.0));
    assert!(
        large > small + 10,
        "the selected button carries its dot: {large} against {small}"
    );
    assert_eq!(small, small_off, "and the other is left as it was");
}

/// Initial values are `/V` and `/DV` both: read back at once, and what a
/// reset (12.7.5.3) returns to.
#[test]
fn initial_values_are_the_defaults_a_reset_returns_to() {
    let document = Document::open(testdata("simple-text.pdf")).expect("opens");
    let mut editor = document.editor();
    let specs = [
        NewField::new(
            "note",
            NewFieldKind::Text {
                page: 1,
                rect: rect(50.0, 50.0, 250.0, 70.0),
                value: Some("first".to_string()),
                max_len: None,
            },
        ),
        NewField::new(
            "ok",
            NewFieldKind::Checkbox {
                page: 1,
                rect: rect(50.0, 100.0, 64.0, 114.0),
                export: "Yes".to_string(),
                checked: true,
            },
        ),
        NewField::new(
            "pick",
            NewFieldKind::Radio {
                buttons: vec![
                    RadioButton {
                        export: "a".to_string(),
                        page: 1,
                        rect: rect(50.0, 150.0, 64.0, 164.0),
                    },
                    RadioButton {
                        export: "b".to_string(),
                        page: 2,
                        rect: rect(50.0, 150.0, 64.0, 164.0),
                    },
                ],
                selected: Some("b".to_string()),
            },
        ),
        NewField::new(
            "list",
            NewFieldKind::Choice {
                page: 1,
                rect: rect(50.0, 200.0, 150.0, 260.0),
                options: vec!["one".to_string(), "two".to_string()],
                combo: false,
                editable: false,
                value: Some("two".to_string()),
            },
        ),
    ];
    for spec in &specs {
        editor.add_field(spec).expect("created");
    }
    let read = |editor: &tinker_pdf::DocumentEditor, name: &str| {
        editor
            .fields()
            .into_iter()
            .find(|f| f.name == name)
            .map(|f| (f.value, f.default))
            .expect("the field")
    };
    assert_eq!(
        read(&editor, "note"),
        (
            FieldValue::Text("first".into()),
            FieldValue::Text("first".into())
        )
    );
    assert_eq!(read(&editor, "ok").0, FieldValue::State("Yes".into()));
    assert_eq!(read(&editor, "pick").0, FieldValue::State("b".into()));
    assert_eq!(read(&editor, "list").0, FieldValue::Text("two".into()));
    let pick = editor
        .fields()
        .into_iter()
        .find(|f| f.name == "pick")
        .expect("pick");
    assert_eq!(pick.kind, FieldKind::Radio);

    editor.fill_field("note", "second").expect("filled");
    editor.fill_field("ok", "Off").expect("unticked");
    editor.fill_field("pick", "a").expect("moved");
    editor.fill_field("list", "one").expect("chosen");
    assert!(editor.reset_form().is_empty(), "every widget redrawn");
    assert_eq!(read(&editor, "note").0, FieldValue::Text("first".into()));
    assert_eq!(read(&editor, "ok").0, FieldValue::State("Yes".into()));
    assert_eq!(read(&editor, "pick").0, FieldValue::State("b".into()));
    assert_eq!(read(&editor, "list").0, FieldValue::Text("two".into()));

    let reopened = Document::open(editor.save(&incremental())).expect("reopens");
    assert!(reopened.validate().is_empty(), "clean across three pages");
}

/// A dotted name is a hierarchy (12.7.3.2): the ancestors are created once
/// and joined after that, and the qualified name reads back whole.
#[test]
fn a_dotted_name_creates_its_ancestors_once() {
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let roots_before = document.form_fields().len();
    let mut editor = document.editor();
    for (name, y) in [("person.name.first", 100.0), ("person.name.last", 130.0)] {
        editor
            .add_field(&NewField::new(
                name,
                NewFieldKind::Text {
                    page: 0,
                    rect: rect(150.0, y, 290.0, y + 20.0),
                    value: Some(name.to_string()),
                    max_len: None,
                },
            ))
            .expect("created");
    }
    let names: Vec<String> = editor.fields().into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"person.name.first".to_string()));
    assert!(names.contains(&"person.name.last".to_string()));
    assert_eq!(names.len(), roots_before + 2);

    let reopened = Document::open(editor.save(&incremental())).expect("reopens");
    assert_eq!(
        value_of(&reopened, "person.name.last"),
        FieldValue::Text("person.name.last".to_string())
    );
    // One `person` node in `/Fields`, not two: the second field joined the
    // hierarchy the first one created.
    let cos = reopened.cos();
    let catalog = cos.catalog().expect("a catalog");
    let form = cos.resolve_key(&catalog, cos.intern(b"AcroForm"));
    let roots = cos.resolve_key(form.as_dict().expect("a form"), cos.intern(b"Fields"));
    assert_eq!(
        roots.as_array().expect("an array").len(),
        5,
        "the fixture's four roots and one person"
    );
    assert!(reopened.validate().is_empty());

    // A field beneath a terminal field is refused, and so is its twin.
    let mut editor = reopened.editor();
    let under = NewField::new(
        "person.name.first.initial",
        NewFieldKind::Text {
            page: 0,
            rect: rect(10.0, 10.0, 40.0, 30.0),
            value: None,
            max_len: None,
        },
    );
    assert_eq!(
        editor.add_field(&under),
        Err(AddFieldError::AncestorIsTerminal(
            "person.name.first".into()
        ))
    );
    let twin = NewField {
        name: "person.name".into(),
        ..under.clone()
    };
    assert_eq!(
        editor.add_field(&twin),
        Err(AddFieldError::NameTaken("person.name".into()))
    );
    assert!(!editor.is_dirty(), "and a refusal writes nothing");
}

/// Each refusal by name, and none of them leaves anything behind.
#[test]
fn a_field_that_cannot_be_made_is_refused_and_writes_nothing() {
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let mut editor = document.editor();
    let text = |name: &str, page: u32, r: Rect| {
        NewField::new(
            name,
            NewFieldKind::Text {
                page,
                rect: r,
                value: None,
                max_len: None,
            },
        )
    };
    let good = rect(10.0, 10.0, 60.0, 30.0);
    let checkbox = |export: &str| {
        NewField::new(
            "box",
            NewFieldKind::Checkbox {
                page: 0,
                rect: good,
                export: export.to_string(),
                checked: false,
            },
        )
    };
    let cases: Vec<(NewField, AddFieldError)> = vec![
        (text("", 0, good), AddFieldError::NameMalformed),
        (text("a..b", 0, good), AddFieldError::NameMalformed),
        (
            text("notes", 0, good),
            AddFieldError::NameTaken("notes".into()),
        ),
        (
            text("agree.more", 0, good),
            AddFieldError::AncestorIsTerminal("agree".into()),
        ),
        (text("x", 7, good), AddFieldError::NoSuchPage(7)),
        (
            text("x", 0, rect(10.0, 10.0, 10.0, 30.0)),
            AddFieldError::RectUnusable,
        ),
        (
            text("x", 0, rect(f64::NAN, 10.0, 60.0, 30.0)),
            AddFieldError::RectUnusable,
        ),
        (checkbox("Off"), AddFieldError::ExportUnusable("Off".into())),
        (checkbox(""), AddFieldError::ExportUnusable(String::new())),
        (
            NewField::new(
                "r",
                NewFieldKind::Radio {
                    buttons: vec![
                        RadioButton {
                            export: "a".into(),
                            page: 0,
                            rect: good,
                        },
                        RadioButton {
                            export: "a".into(),
                            page: 0,
                            rect: good,
                        },
                    ],
                    selected: None,
                },
            ),
            AddFieldError::ExportUnusable("a".into()),
        ),
        (
            NewField::new(
                "r",
                NewFieldKind::Radio {
                    buttons: Vec::new(),
                    selected: None,
                },
            ),
            AddFieldError::NoButtons,
        ),
        (
            NewField::new(
                "c",
                NewFieldKind::Choice {
                    page: 0,
                    rect: good,
                    options: vec!["a".into()],
                    combo: true,
                    editable: false,
                    value: Some("b".into()),
                },
            ),
            AddFieldError::ValueRefused,
        ),
        (
            NewField::new(
                "c",
                NewFieldKind::Choice {
                    page: 0,
                    rect: good,
                    options: vec!["a".into()],
                    combo: false,
                    editable: true,
                    value: None,
                },
            ),
            AddFieldError::FlagsContradictKind,
        ),
        (
            NewField::new(
                "t",
                NewFieldKind::Text {
                    page: 0,
                    rect: good,
                    value: Some("four".into()),
                    max_len: Some(3),
                },
            ),
            AddFieldError::ValueRefused,
        ),
        (
            NewField {
                flags: 1 << 15,
                ..text("t", 0, good)
            },
            AddFieldError::FlagsContradictKind,
        ),
        (
            NewField {
                font_size: -1.0,
                ..text("t", 0, good)
            },
            AddFieldError::FontSizeUnusable,
        ),
    ];
    for (spec, expected) in cases {
        assert_eq!(
            editor.add_field(&spec),
            Err(expected.clone()),
            "{:?}",
            spec.name
        );
        assert!(!editor.is_dirty(), "{expected:?} wrote nothing");
    }

    // An editable combo box takes a value it does not list.
    let editable = NewField::new(
        "c",
        NewFieldKind::Choice {
            page: 0,
            rect: good,
            options: vec!["a".into()],
            combo: true,
            editable: true,
            value: Some("typed".into()),
        },
    );
    assert!(editor.add_field(&editable).is_ok());
}

/// A form that has a `/DR /Helv` keeps it and every created field names it;
/// a file with none is given one, and exactly one however many fields are
/// created.
#[test]
fn the_default_font_is_joined_or_added_once() {
    // `form-fields.pdf`'s `/DR /Font /Helv` is object 4.
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let mut editor = document.editor();
    for spec in four_kinds() {
        editor.add_field(&spec).expect("created");
    }
    let reopened = Document::open(editor.save(&incremental())).expect("reopens");
    let cos = reopened.cos();
    let form = cos.resolve_key(&cos.catalog().expect("catalog"), cos.intern(b"AcroForm"));
    let dr = cos.resolve_key(form.as_dict().expect("form"), cos.intern(b"DR"));
    let fonts = cos.resolve_key(dr.as_dict().expect("a /DR"), cos.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a /Font");
    assert_eq!(fonts.len(), 1, "no second font beside the form's own");
    assert_eq!(
        fonts.get_ref(cos.intern(b"Helv")).map(|r| r.num),
        Some(4),
        "the file's own /Helv, joined"
    );

    let document = Document::open(testdata("simple-text.pdf")).expect("opens");
    let mut editor = document.editor();
    for spec in four_kinds() {
        editor.add_field(&spec).expect("created");
    }
    let reopened = Document::open(editor.save(&incremental())).expect("reopens");
    let cos = reopened.cos();
    let form = cos.resolve_key(&cos.catalog().expect("catalog"), cos.intern(b"AcroForm"));
    let dr = cos.resolve_key(form.as_dict().expect("form"), cos.intern(b"DR"));
    let fonts = cos.resolve_key(dr.as_dict().expect("a /DR"), cos.intern(b"Font"));
    let fonts = fonts.as_dict().expect("a /Font");
    assert_eq!(fonts.len(), 1, "one font, however many fields");
    let helv = cos.resolve_key(fonts, cos.intern(b"Helv"));
    let base = helv
        .as_dict()
        .and_then(|d| d.get_name(cos.intern(b"BaseFont")))
        .and_then(|n| cos.name_bytes(n));
    assert_eq!(base.as_deref(), Some(b"Helvetica".as_slice()));
}

/// The known limit the prerequisites left: an appearance laid out against a
/// `/DR` font the editor had only just added.
///
/// Auto-sizing measures the value in the `/DA` font. Forty `i`s are 8.88 em in
/// Helvetica and would be 20 em under the half-an-em guess a font that cannot
/// be found gets, so in a 100-point box the size comes out near 10.8 with the
/// font and 4.8 without it — which is the whole test.
#[test]
fn a_created_field_is_laid_out_in_the_font_the_editor_added() {
    let document = Document::open(testdata("simple-text.pdf")).expect("opens");
    let mut editor = document.editor();
    let field = editor
        .add_field(&NewField::new(
            "narrow",
            NewFieldKind::Text {
                page: 0,
                rect: rect(50.0, 50.0, 150.0, 70.0),
                value: Some("i".repeat(40)),
                max_len: None,
            },
        ))
        .expect("created");
    let widget = editor.get(field).expect("the widget");
    let ap = widget
        .as_dict()
        .and_then(|d| d.get_dict(editor.intern(b"AP")))
        .and_then(|ap| ap.get_ref(editor.intern(b"N")))
        .expect("an /AP /N");
    let content = String::from_utf8(editor.stream_bytes(ap).expect("the stream")).expect("ascii");
    let size: f64 = content
        .split_once(" Tf")
        .and_then(|(before, _)| before.rsplit(' ').next())
        .and_then(|size| size.parse().ok())
        .expect("a Tf size");
    assert!(
        size > 10.0,
        "sized by Helvetica's metrics, not by a guess: {size} in {content}"
    );
}

/// A field created beneath an existing parent is the kind it was asked to
/// be, whatever the parent carries.
///
/// `/Ff` is inheritable (12.7.3.1), so a check box created under a node that
/// carries the Radio bit — a shape producers do write, to share flags across a
/// group — would read back as a radio group if its own `/Ff` were left out.
#[test]
fn a_created_field_does_not_inherit_its_kind() {
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let mut editor = document.editor();
    let group = editor.allocate();
    let mut dict = tinker_pdf::Dict::new();
    dict.insert(
        editor.intern(b"T"),
        tinker_pdf::Object::String(tinker_pdf::PdfString::literal(b"grp".to_vec())),
    );
    dict.insert(editor.intern(b"Ff"), tinker_pdf::Object::Int(1 << 15));
    dict.insert(
        editor.intern(b"Kids"),
        tinker_pdf::Object::Array(Vec::new()),
    );
    editor.put(group, tinker_pdf::Object::Dict(dict));
    let acroform = editor.intern(b"AcroForm");
    let fields_key = editor.intern(b"Fields");
    assert!(editor.update_catalog(|catalog| {
        if let Some(tinker_pdf::Object::Dict(mut form)) = catalog.get(acroform).cloned() {
            let mut list = form
                .get_array(fields_key)
                .map(<[tinker_pdf::Object]>::to_vec)
                .unwrap_or_default();
            list.push(tinker_pdf::Object::Ref(group));
            form.insert(fields_key, tinker_pdf::Object::Array(list));
            catalog.insert(acroform, tinker_pdf::Object::Dict(form));
        }
    }));

    editor
        .add_field(&NewField::new(
            "grp.box",
            NewFieldKind::Checkbox {
                page: 0,
                rect: rect(150.0, 100.0, 170.0, 120.0),
                export: "Yes".into(),
                checked: true,
            },
        ))
        .expect("created beneath the group");
    let field = editor
        .fields()
        .into_iter()
        .find(|f| f.name == "grp.box")
        .expect("found under its qualified name");
    assert_eq!(field.kind, FieldKind::Checkbox, "not the parent's radio");
    assert_eq!(field.value, FieldValue::State("Yes".into()));
}

/// A button takes the name of a state some widget offers, or `Off`, and
/// nothing else — a `/V` no widget can draw is refused, not written.
#[test]
fn a_button_takes_only_a_state_it_can_draw() {
    let document = Document::open(testdata("form-fields.pdf")).expect("opens");
    let mut editor = document.editor();
    assert_eq!(
        editor.fill_field("agree", "Yes"),
        Err(tinker_pdf::FillError::ValueRefused),
        "the fixture's box is /On, not /Yes"
    );
    assert!(!editor.is_dirty());
    editor.fill_field("agree", "On").expect("its own on state");
    editor.fill_field("colour", "blue").expect("a radio option");
    let fields = editor.fields();
    let value = |name: &str| {
        fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.value.clone())
            .expect("field")
    };
    assert_eq!(value("agree"), FieldValue::State("On".into()));
    assert_eq!(value("colour"), FieldValue::State("blue".into()));
    let states: Vec<Option<String>> = fields
        .iter()
        .find(|f| f.name == "colour")
        .expect("colour")
        .widgets
        .iter()
        .map(|w| {
            editor
                .get(*w)
                .and_then(|o| o.as_dict().and_then(|d| d.get_name(editor.intern(b"AS"))))
                .and_then(|n| editor.document().name_bytes(n))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        })
        .collect();
    assert_eq!(
        states,
        vec![Some("Off".into()), Some("blue".into())],
        "every widget's /AS follows /V"
    );
}
