//! Text strings this crate writes read back as the text they were given
//! (7.9.2.2), through the reader's own metadata, outline and field APIs.
//!
//! The defect this pins: `/Info` entries and outline titles were written as
//! their UTF-8 bytes with no byte-order mark, and 7.9.2.2 has a reader decode
//! an unmarked string as PDFDocEncoding — so "Ä" came back as "Ã—" from this
//! repository's own reader, and from everybody else's. Every assertion here is
//! on the string the reader returns, compared byte for byte with what was set,
//! at a 1.7 header and at a 2.0 one; the encodings are asserted as well,
//! because a writer that reached UTF-8 at 1.7 would read back perfectly here
//! and show three stray characters in a 1.x viewer.

use std::path::PathBuf;
use std::sync::Arc;

use proptest::prelude::*;
use tinker_pdf_cos::{
    decode_text_string, encode_text_string, fields, metadata, outline, CosDocument,
    DocumentBuilder, DocumentEditor, FieldValue, Name, Object, OutlineEntry, WriteMode,
    WriteOptions,
};

/// Text the three encodings between them have to carry: PDFDocEncoding's
/// Latin-1 half and its substitution range, a character only UTF-16 or UTF-8
/// can hold, one outside the Basic Multilingual Plane, and the two openings
/// that PDFDocEncoding would spell as a byte-order mark.
const SAMPLES: [&str; 8] = [
    "Plain ASCII",
    "Ästhetik und Übermaß",
    "“Quoted” — en–dash, €5, ﬁ, Œuvre",
    "日本語のタイトル",
    "Mixed: Ärger 日本 \u{1F600}",
    "þÿ looks like a byte-order mark",
    "ï»¿ looks like the other one",
    "",
];

fn entry(title: &str, children: Vec<OutlineEntry>) -> OutlineEntry {
    OutlineEntry {
        title: title.to_string(),
        target: None,
        open: true,
        children,
    }
}

/// A document built at `version` with every sample in `/Info` and in a
/// two-level outline.
fn built(version: (u8, u8)) -> CosDocument {
    let mut builder = if version == (1, 7) {
        DocumentBuilder::new()
    } else {
        DocumentBuilder::with_version(version.0, version.1)
    };
    builder.add_page(100.0, 100.0, |_| {});
    assert!(builder.set_info(b"Title", SAMPLES[1]));
    assert!(builder.set_info(b"Author", SAMPLES[2]));
    assert!(builder.set_info(b"Subject", SAMPLES[3]));
    assert!(builder.set_info(b"Keywords", SAMPLES[4]));
    assert!(builder.set_info(b"Creator", SAMPLES[5]));
    assert!(builder.set_info(b"Producer", SAMPLES[6]));
    let children = SAMPLES.iter().map(|s| entry(s, Vec::new())).collect();
    assert!(builder.set_outline(vec![entry(SAMPLES[4], children)]));
    CosDocument::open(builder.finish()).expect("the built document opens")
}

fn assert_reads_back(doc: &CosDocument) {
    let info = metadata(doc);
    assert_eq!(info.title.as_deref(), Some(SAMPLES[1]));
    assert_eq!(info.author.as_deref(), Some(SAMPLES[2]));
    assert_eq!(info.subject.as_deref(), Some(SAMPLES[3]));
    assert_eq!(info.keywords.as_deref(), Some(SAMPLES[4]));
    assert_eq!(info.creator.as_deref(), Some(SAMPLES[5]));
    assert_eq!(info.producer.as_deref(), Some(SAMPLES[6]));

    let tree = outline(doc);
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].title, SAMPLES[4]);
    let titles: Vec<&str> = tree[0].children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, SAMPLES);
}

/// The raw bytes of one `/Info` entry, as the file holds them.
fn info_bytes(doc: &CosDocument, key: &[u8]) -> Vec<u8> {
    let info = doc
        .trailer()
        .get_ref(Name::INFO)
        .and_then(|r| doc.get(r).ok())
        .expect("an /Info dictionary");
    match info.as_dict().and_then(|d| d.get(doc.intern(key))) {
        Some(Object::String(s)) => s.bytes.clone(),
        other => panic!("/{} is {other:?}", String::from_utf8_lossy(key)),
    }
}

#[test]
fn info_and_outline_titles_read_back_at_one_seven() {
    let doc = built((1, 7));
    assert_eq!(doc.header_version().as_deref(), Some("1.7"));
    assert_reads_back(&doc);

    // The forms: PDFDocEncoding where it carries the text, UTF-16 otherwise,
    // and never 2.0's UTF-8 in a file that does not declare 2.0.
    assert_eq!(
        info_bytes(&doc, b"Title"),
        b"\xC4sthetik und \xDCberma\xDF",
        "PDFDocEncoding carries Latin-1"
    );
    assert_eq!(
        info_bytes(&doc, b"Author")[..4],
        *b"\x8DQuo",
        "and its own substitutions, the left double quote at 0x8D"
    );
    assert_eq!(info_bytes(&doc, b"Subject")[..2], [0xFE, 0xFF]);
    assert_eq!(info_bytes(&doc, b"Creator")[..2], [0xFE, 0xFF]);
    assert_eq!(info_bytes(&doc, b"Producer")[..2], [0xFE, 0xFF]);
}

#[test]
fn info_and_outline_titles_read_back_at_two_zero() {
    let doc = built((2, 0));
    assert_eq!(doc.header_version().as_deref(), Some("2.0"));
    assert_reads_back(&doc);

    assert_eq!(
        info_bytes(&doc, b"Title"),
        b"\xC4sthetik und \xDCberma\xDF",
        "PDFDocEncoding is still preferred where it carries the text"
    );
    let subject = info_bytes(&doc, b"Subject");
    assert_eq!(subject[..3], [0xEF, 0xBB, 0xBF], "2.0's UTF-8 form");
    assert_eq!(&subject[3..], SAMPLES[3].as_bytes());
    assert_eq!(info_bytes(&doc, b"Creator")[..3], [0xEF, 0xBB, 0xBF]);
}

/// The editor writes a field value as the same kind of text string, for the
/// version the document it is editing declares.
#[test]
fn a_filled_field_reads_back_at_both_versions() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/form-fields.pdf");
    let original = std::fs::read(&path).expect("the form fixture");
    assert!(original.starts_with(b"%PDF-1.7"), "the premise");
    // The same file declaring 2.0: the header is the same length, so every
    // offset in it still holds.
    let mut two = original.clone();
    two[5..8].copy_from_slice(b"2.0");

    for (bytes, mark) in [
        (original, &[0xFE, 0xFF][..]),
        (two, &[0xEF, 0xBB, 0xBF][..]),
    ] {
        let doc = Arc::new(CosDocument::open(bytes).expect("it opens"));
        let mut editor = DocumentEditor::new(doc);
        editor
            .fill_field("notes", SAMPLES[4])
            .expect("the field takes the value");
        let saved = CosDocument::open(editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        }))
        .expect("the save reopens");
        let field = fields(&saved)
            .into_iter()
            .find(|f| f.name == "notes")
            .expect("the field");
        assert_eq!(field.value, FieldValue::Text(SAMPLES[4].to_string()));

        let raw = saved
            .get(field.reference)
            .ok()
            .and_then(|o| o.as_dict().and_then(|d| d.get(saved.intern(b"V")).cloned()));
        let Some(Object::String(raw)) = raw else {
            panic!("/V is a string");
        };
        assert!(raw.bytes.starts_with(mark), "{:02X?}", &raw.bytes[..4]);
    }
}

fn text() -> impl Strategy<Value = String> {
    // The two openings PDFDocEncoding would spell as a byte-order mark, often
    // enough that every run meets them rather than one in a few hundred.
    let opening = prop_oneof![Just(""), Just("þÿ"), Just("ï»¿"), Just("þ"), Just("ï»")];
    let rest = prop_oneof![
        any::<String>(),
        // Weighted towards the characters the encodings disagree about.
        proptest::collection::vec(
            prop_oneof![
                Just('þ'),
                Just('ÿ'),
                Just('ï'),
                Just('»'),
                Just('¿'),
                Just('€'),
                Just('\u{A0}'),
                Just('\u{AD}'),
                Just('\u{7F}'),
                Just('\u{9F}'),
                Just('\u{FFFD}'),
                Just('\u{FEFF}'),
                Just('\u{2022}'),
                Just('\u{02D8}'),
                Just('\r'),
                Just('\n'),
                Just('\u{1}'),
                any::<char>(),
            ],
            0..12,
        )
        .prop_map(|chars| chars.into_iter().collect::<String>()),
    ];
    (opening, rest).prop_map(|(opening, rest)| format!("{opening}{rest}"))
}

proptest! {
    /// Encode then decode is the identity, at every version, and the form is
    /// the one the version allows.
    #[test]
    fn encoding_is_the_decoders_inverse(s in text(), two in any::<bool>()) {
        let version = if two { (2, 0) } else { (1, 7) };
        let encoded = encode_text_string(&s, version);
        prop_assert_eq!(decode_text_string(&encoded.bytes), s);
        if !two {
            prop_assert!(!encoded.bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        }
        if encoded.bytes.starts_with(&[0xFE, 0xFF]) {
            prop_assert!(!two, "2.0 takes UTF-8 where it needs a mark");
        }
    }
}
