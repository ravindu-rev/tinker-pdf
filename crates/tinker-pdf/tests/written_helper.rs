//! The validate-on-save helper's own tests (`docs/design/validate-on-save.md`).
//!
//! Each door admits what its form admits and fails the rest by name, at its
//! caller; the cos reading and the facade reading give one strict verdict for
//! the same bytes; and the cos reading, which runs no profile validator,
//! refuses what it cannot hold.
//!
//! A door that fails panics, so a refusal is read here as the panic it is,
//! by [`refusal`].

mod written_support;

/// The cos reading, which the cos crate's tests reach as their own
/// `written_support`, taken by `#[path]` so the two readings can be held
/// against each other on the same bytes. Both readings compile the one policy
/// file, `held.rs`, which is the design rather than an accident.
#[allow(clippy::duplicate_mod)]
#[path = "../../tinker-pdf-cos/tests/written_support/mod.rs"]
mod cos_reading;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use tinker_pdf::{
    CosDocument, Defect, DefectKind, DocumentBuilder, DocumentEditor, Encryption, LadderLevel,
    ObjRef, WarningKind, WriteMode, WriteOptions,
};
use written_support::{authored, expecting, rewritten, Expected, Key, Locked};

const PASSWORD: &str = "open-me";

/// A PDF/A-2b claim, and nothing else.
const PDFA_CLAIM: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

/// A PDF/UA-1 claim, and nothing else.
const PDFUA_CLAIM: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
 pdfuaid:part="1"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

/// A packet that claims nothing: a title, in no identification schema.
const NO_CLAIM: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title><rdf:Alt>
<rdf:li xml:lang="x-default">held</rdf:li></rdf:Alt></dc:title></rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

// ---- the documents under test ----------------------------------------------

/// A small document with one standard font shared by its three pages, and one
/// `/Info` entry.
fn document(key: &[u8], value: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_base_font(b"F0", b"Helvetica"));
    assert!(builder.set_info(key, value));
    for index in 0..3 {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    builder.finish()
}

fn built() -> Vec<u8> {
    document(b"Title", "held")
}

/// `bytes` saved through the editor with nothing changed.
fn saved(bytes: &[u8], options: &WriteOptions) -> Vec<u8> {
    let doc = Arc::new(CosDocument::open(bytes.to_vec()).expect("it opens"));
    DocumentEditor::new(doc).save(options)
}

/// `bytes` with its title changed, saved as an incremental update, so the
/// update appends objects of its own.
fn updated(bytes: &[u8]) -> Vec<u8> {
    let doc = Arc::new(CosDocument::open(bytes.to_vec()).expect("it opens"));
    let mut editor = DocumentEditor::new(doc);
    let _ = editor.set_title("updated");
    editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    })
}

/// The built document carrying `packet` as its XMP metadata.
fn with_packet(packet: &[u8]) -> Vec<u8> {
    let doc = Arc::new(CosDocument::open(built()).expect("it opens"));
    let mut editor = DocumentEditor::new(doc);
    let _ = editor.set_xmp_metadata(packet).expect("a catalog");
    editor.save(&WriteOptions::default())
}

/// The deterministic entropy the strict validator's own tests seal with.
fn encryption() -> Encryption {
    let mut entropy = [0u8; 48];
    for (index, byte) in entropy.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    Encryption {
        user_password: PASSWORD.to_string(),
        owner_password: "owner-me".to_string(),
        permissions: -1,
        entropy,
    }
}

/// A one-page file whose page carries an annotation with no `/Subtype`, which
/// 12.5.2 requires: a semantics defect of somebody else's file, which a
/// rewrite copies through. Hand-built, because this engine's writer does not
/// write one.
fn unsubtyped() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Annots [4 0 R] >>",
        "<< /Type /Annot /Rect [10 10 50 50] >>",
    ];
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let table = out.len();
    let size = bodies.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n").as_bytes(),
    );
    out
}

// ---- byte surgery -----------------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// Replaces the first occurrence of `needle` with something exactly as long, so
/// every offset in the file still means what it meant.
fn patch(bytes: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    assert_eq!(needle.len(), with.len(), "a patch may not move offsets");
    let at = find(bytes, needle).expect("the needle is in the file");
    let mut out = bytes.to_vec();
    out[at..at + with.len()].copy_from_slice(with);
    out
}

/// Where the classic table's entries begin: twenty bytes each from object
/// zero (7.5.4).
fn entries(bytes: &[u8]) -> usize {
    let table = find(bytes, b"xref\n").expect("a classic table") + 5;
    table + find(&bytes[table..], b"\n").expect("a subsection header") + 1
}

fn entry_offset(bytes: &[u8], num: u32) -> u64 {
    let at = entries(bytes) + num as usize * 20;
    let digits = std::str::from_utf8(&bytes[at..at + 10]).expect("ten digits");
    digits.parse().expect("an offset")
}

/// The built document with object 3's entry moved to the offset of object 4's
/// header, in its own ten digits, so no other byte moves.
fn entry_moved(bytes: &[u8]) -> Vec<u8> {
    let at = entries(bytes) + 3 * 20;
    let mut out = bytes.to_vec();
    out[at..at + 10].copy_from_slice(format!("{:010}", entry_offset(bytes, 4)).as_bytes());
    out
}

/// Everything the strict validator finds in [`entry_moved`]: the ladder that
/// opening it needed, the two repairs it made on the way, and the header the
/// entry names.
fn entry_moved_defects() -> Vec<Expected> {
    vec![
        Expected::Defect(DefectKind::OpenedLenient(LadderLevel::Patch)),
        Expected::Defect(DefectKind::Repaired(WarningKind::ObjectHeaderMismatch)),
        Expected::Defect(DefectKind::Repaired(WarningKind::ObjectRepaired)),
        Expected::Defect(DefectKind::ObjectHeaderMismatch {
            found: ObjRef::new(4, 0),
        }),
    ]
}

/// What a door said when it failed, which it must.
#[track_caller]
fn refusal(door: impl FnOnce()) -> String {
    let Err(payload) = catch_unwind(AssertUnwindSafe(door)) else {
        panic!("the door admitted it");
    };
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
        })
        .unwrap_or_default()
}

fn utf16be(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

// ---- the doors --------------------------------------------------------------

/// Every layout this engine writes from a document it built is `authored`:
/// the built document, its full rewrite, its incremental save, its linearized
/// save, and its encrypted save opened with its key.
#[test]
fn an_undamaged_written_document_passes() {
    let bytes = built();
    let doc = authored("built", &bytes);
    assert_eq!(doc.page_count(), 3, "the door hands back what it opened");

    authored("rewritten in full", saved(&bytes, &WriteOptions::default()));
    let incremental = updated(&bytes);
    assert!(incremental.starts_with(&bytes) && incremental.len() > bytes.len());
    authored("saved incrementally", incremental);
    authored(
        "linearized",
        saved(
            &bytes,
            &WriteOptions {
                linearize: true,
                ..WriteOptions::default()
            },
        ),
    );
    let encrypted = saved(
        &bytes,
        &WriteOptions {
            encryption: Some(encryption()),
            ..WriteOptions::default()
        },
    );
    let doc = authored(
        "encrypted",
        Locked {
            bytes: encrypted.clone(),
            key: Key::Password(PASSWORD),
        },
    );
    assert!(doc.is_encrypted() && doc.page_count() == 3);

    let message = refusal(|| {
        authored("encrypted, handed over with no key", &encrypted);
    });
    assert!(message.contains("Locked { bytes, key }"), "{message}");
}

/// A cross-reference entry moved to another object's header is refused by
/// `authored`, which names the defect under the verdict that owns it, where
/// the undamaged twin passes.
#[test]
fn one_injected_structure_defect_fails_by_name() {
    let bytes = built();
    authored("the undamaged twin", &bytes);

    let message = refusal(|| {
        authored("a cross-reference offset moved", entry_moved(&bytes));
    });
    assert!(
        message.contains("`a cross-reference offset moved` fails `authored`"),
        "{message}"
    );
    assert!(message.contains("\n  structure:"), "{message}");
    assert!(message.contains("[object-header-mismatch]"), "{message}");
}

/// A PDF/A claim over a font no program is embedded for fails the facade
/// reading, naming the font finding under the claim; the same document with a
/// packet that claims nothing passes.
#[test]
fn a_claimed_profile_with_a_finding_fails() {
    let claimed = with_packet(PDFA_CLAIM.as_bytes());
    let message = refusal(|| {
        authored("a PDF/A claim over a standard font", &claimed);
    });
    assert!(message.contains("\n  PDF/A (PDF/A-2B;"), "{message}");
    assert!(message.contains("FontNotEmbedded"), "{message}");

    authored(
        "the twin whose packet claims nothing",
        with_packet(NO_CLAIM.as_bytes()),
    );
}

/// The expected-defects form passes on exactly the moved entry's defects, and
/// fails on one fewer, one more, or one field wrong.
#[test]
fn the_expected_defects_form_passes_only_on_exactly_those_defects() {
    let damaged = entry_moved(&built());
    let exact = entry_moved_defects();
    let doc = expecting("the moved entry, listed exactly", &damaged, &exact);
    assert_eq!(doc.page_count(), 3, "the door hands back what it opened");

    let mut reordered = exact.clone();
    reordered.reverse();
    expecting(
        "the moved entry, listed in another order",
        &damaged,
        &reordered,
    );

    let message = refusal(|| {
        expecting("one fewer", &damaged, &exact[..exact.len() - 1]);
    });
    assert!(message.contains("found and not listed"), "{message}");

    let mut more = exact.clone();
    more.push(Expected::Defect(DefectKind::ObjectHeaderAbsent));
    let message = refusal(|| {
        expecting("one more", &damaged, &more);
    });
    assert!(
        message.contains("listed and not found:\n    structure: ObjectHeaderAbsent"),
        "{message}"
    );

    let mut twice = exact.clone();
    twice.push(exact[3].clone());
    let message = refusal(|| {
        expecting("one listed twice", &damaged, &twice);
    });
    assert!(message.contains("listed and not found"), "{message}");

    let mut wrong = exact.clone();
    wrong[3] = Expected::Defect(DefectKind::ObjectHeaderMismatch {
        found: ObjRef::new(5, 0),
    });
    let message = refusal(|| {
        expecting("one field wrong", &damaged, &wrong);
    });
    assert!(
        message.contains("found and not listed: ObjectHeaderMismatch { found: ObjRef { num: 4"),
        "{message}"
    );
    assert!(
        message.contains("structure: ObjectHeaderMismatch { found: ObjRef { num: 5"),
        "{message}"
    );

    expecting("an empty list is `authored`", built(), &[]);
    let message = refusal(|| {
        expecting("an empty list over damage", &damaged, &[]);
    });
    assert!(message.contains("found and not listed"), "{message}");
}

/// A rewrite of a file this engine did not write is held to its input: a
/// semantics defect the input carries passes `rewritten` in a full rewrite and
/// in an incremental one, and fails `authored`; a rewrite that adds a class of
/// defect fails, and so does an update damaged past the input's last byte.
#[test]
fn a_rewrite_is_held_to_its_input() {
    let input = unsubtyped();
    expecting(
        "the hand-built input",
        &input,
        &[Expected::Defect(DefectKind::AnnotSubtypeMissing)],
    );

    let full = saved(&input, &WriteOptions::default());
    rewritten("rewritten in full", &input, &full);
    let message = refusal(|| {
        authored("rewritten in full", &full);
    });
    assert!(message.contains("[annot-subtype-missing]"), "{message}");

    let incremental = updated(&input);
    assert!(incremental.starts_with(&input) && incremental.len() > input.len());
    rewritten("saved incrementally", &input, &incremental);
    let message = refusal(|| {
        authored("saved incrementally", &incremental);
    });
    assert!(message.contains("[annot-subtype-missing]"), "{message}");

    let parentless = patch(&full, b"/Parent", b"/Parenx");
    let message = refusal(|| {
        rewritten("a rewrite that adds a defect", &input, &parentless);
    });
    assert!(message.contains("held as a full rewrite"), "{message}");
    assert!(
        message.contains("`page-parent-wrong`: 1 in the output, 0 in the input"),
        "{message}"
    );

    let mut appended = incremental.clone();
    appended.extend_from_slice(b"junk");
    let message = refusal(|| {
        rewritten("an update damaged past its input", &input, &appended);
    });
    assert!(
        message.contains("held as an incremental update"),
        "{message}"
    );
    assert!(message.contains("[bytes-after-eof]"), "{message}");
    assert!(message.contains("past the input's last byte"), "{message}");

    // With a clean input, no more than nothing is nothing: `rewritten` is
    // `authored`, so choosing it cannot loosen a test whose input is this
    // engine's.
    let bytes = built();
    rewritten(
        "a rewrite of a clean input",
        &bytes,
        saved(&bytes, &WriteOptions::default()),
    );
    let message = refusal(|| {
        rewritten("a clean input, damaged", &bytes, entry_moved(&bytes));
    });
    assert!(
        message.contains("a full rewrite owns the structure it writes"),
        "{message}"
    );
}

/// A repair is classed by the warning it repaired, whichever phase of the
/// reading met it. The validator reports each warning's label once, in the
/// first phase that meets it — the open, an object re-parsed at its entry's
/// offset, or reading on — and a rewrite changes what the reader reads first.
/// So the three phases of one warning are one class in both readings, apart
/// from a rule that shares the warning's name; a rewrite whose input met a
/// repair at open and whose output met it while reading passes `rewritten`; a
/// repair of another warning fails it; the expected form still names the
/// phase; and `authored` still admits no repair at all.
#[test]
fn a_repair_is_one_class_whichever_phase_met_it() {
    let met = |kind: DefectKind| Defect {
        object: Some(ObjRef::new(5, 0)),
        offset: Some(40),
        kind,
    };
    let unknown = WarningKind::FilterUnknown;
    for kind in [
        DefectKind::Repaired(unknown),
        DefectKind::ObjectRepaired(unknown),
        DefectKind::RepairedWhileReading(unknown),
    ] {
        let facade = written_support::item(&met(kind));
        let cos = cos_reading::item(&met(kind));
        assert_eq!(format!("{facade:?}"), format!("{cos:?}"), "{kind:?}");
        assert_eq!(facade.class, "repair: filter-unknown", "{kind:?}");
        assert_eq!(facade.verdict, "semantics", "{kind:?}");
        assert!(!facade.owned, "{kind:?}");
        assert_eq!(facade.exact, format!("{kind:?}"), "the expected form's key");
    }

    // `ObjectHeaderMismatch` is a warning and a rule: the rule keeps its own
    // label, and a repair of the warning is not counted as the rule.
    let rule = written_support::item(&met(DefectKind::ObjectHeaderMismatch {
        found: ObjRef::new(4, 0),
    }));
    assert_eq!(rule.class, "object-header-mismatch");
    let repair = written_support::item(&met(DefectKind::Repaired(
        WarningKind::ObjectHeaderMismatch,
    )));
    assert_eq!(repair.class, "repair: object-header-mismatch");
    assert_eq!(
        format!("{repair:?}"),
        format!(
            "{:?}",
            cos_reading::item(&met(DefectKind::Repaired(
                WarningKind::ObjectHeaderMismatch
            )))
        )
    );

    let read = |bytes: &[u8], kinds: &[DefectKind]| written_support::held::Read {
        bytes: Arc::from(bytes),
        items: kinds
            .iter()
            .map(|&kind| written_support::item(&met(kind)))
            .collect(),
        notes: Vec::new(),
    };
    let input = read(b"the input".as_slice(), &[DefectKind::Repaired(unknown)]);
    let output = read(
        b"its rewrite".as_slice(),
        &[DefectKind::RepairedWhileReading(unknown)],
    );
    written_support::held::rewritten("a repair met at open, then while reading", &input, &output);
    written_support::held::rewritten(
        "a repair met re-parsing an object, then at open",
        &read(
            b"the input".as_slice(),
            &[DefectKind::ObjectRepaired(unknown)],
        ),
        &read(b"its rewrite".as_slice(), &[DefectKind::Repaired(unknown)]),
    );

    let other = read(
        b"its rewrite".as_slice(),
        &[DefectKind::RepairedWhileReading(
            WarningKind::FilterParamsBad,
        )],
    );
    let message = refusal(|| {
        written_support::held::rewritten("a repair of another warning", &input, &other);
    });
    assert!(
        message.contains("`repair: filter-params-bad`: 1 in the output, 0 in the input"),
        "{message}"
    );

    let message = refusal(|| {
        written_support::held::authored("a repair", "authored", &output);
    });
    assert!(message.contains("[repaired-while-reading]"), "{message}");
    written_support::held::expecting(
        "a repair, listed by its phase",
        &output,
        &[written_support::held::Want {
            verdict: "semantics",
            exact: format!("{:?}", DefectKind::RepairedWhileReading(unknown)),
        }],
    );
    let message = refusal(|| {
        written_support::held::expecting(
            "a repair, listed under another phase",
            &output,
            &[written_support::held::Want {
                verdict: "semantics",
                exact: format!("{:?}", DefectKind::Repaired(unknown)),
            }],
        );
    });
    assert!(message.contains("found and not listed"), "{message}");
}

// ---- the two readings -------------------------------------------------------

/// The cos reading and the facade reading give the same findings for the same
/// bytes, damaged and not, locked and not: one strict implementation, over
/// one parse, in every tree.
#[test]
fn the_two_readings_give_one_strict_verdict() {
    let bytes = built();
    let mut junk = bytes.clone();
    junk.extend_from_slice(b"junk");
    let cases = [
        ("built", bytes.clone()),
        ("rewritten", saved(&bytes, &WriteOptions::default())),
        ("updated", updated(&bytes)),
        ("an entry moved", entry_moved(&bytes)),
        ("an annotation with no subtype", unsubtyped()),
        ("bytes after the end", junk),
    ];
    let mut found = 0;
    for (label, case) in &cases {
        let facade = written_support::items(label, case);
        let cos = cos_reading::items(label, case);
        assert_eq!(format!("{facade:?}"), format!("{cos:?}"), "{label}");
        found += facade.len();
    }
    assert!(found > 0, "the cases are not all clean");

    let encrypted = saved(
        &bytes,
        &WriteOptions {
            encryption: Some(encryption()),
            ..WriteOptions::default()
        },
    );
    for case in [encrypted.clone(), entry_moved(&encrypted)] {
        let facade = written_support::items(
            "encrypted",
            Locked {
                bytes: case.clone(),
                key: Key::Password(PASSWORD),
            },
        );
        let cos = cos_reading::items(
            "encrypted",
            cos_reading::Locked {
                bytes: case,
                key: cos_reading::Key::Password(PASSWORD),
            },
        );
        assert_eq!(format!("{facade:?}"), format!("{cos:?}"));
    }
}

/// The cos reading runs no profile validator, so a document that might claim
/// one fails it, naming the facade tree, while the same document claiming
/// nothing passes.
#[test]
fn the_cos_reading_refuses_a_profile_claim() {
    let facade_tree = "crates/tinker-pdf/tests/written_support";

    let pdfa = with_packet(PDFA_CLAIM.as_bytes());
    let message = refusal(|| {
        cos_reading::authored("a PDF/A claim", &pdfa);
    });
    assert!(message.contains(facade_tree), "{message}");
    cos_reading::authored(
        "the twin whose packet claims nothing",
        with_packet(NO_CLAIM.as_bytes()),
    );

    // The identification schemas' own spellings, in UTF-16 as well as ASCII.
    let pdfua = with_packet(&utf16be(PDFUA_CLAIM));
    let message = refusal(|| {
        cos_reading::authored("a PDF/UA claim in UTF-16", &pdfua);
    });
    assert!(message.contains("pdfuaid"), "{message}");
    assert!(message.contains(facade_tree), "{message}");
    cos_reading::authored(
        "the twin in UTF-16 that claims nothing",
        with_packet(&utf16be(NO_CLAIM)),
    );

    let pdfx = document(b"GTS_PDFXVersion", "PDF/X-1a:2003");
    let message = refusal(|| {
        cos_reading::authored("a PDF/X claim", &pdfx);
    });
    assert!(message.contains("/GTS_PDFXVersion"), "{message}");
    assert!(message.contains(facade_tree), "{message}");
    cos_reading::authored("the twin that claims nothing", built());
}
