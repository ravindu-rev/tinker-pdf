//! Long-term validation: the document security store (ISO 32000-2 12.8.4.3),
//! written from host-supplied CRL and OCSP bytes and read back.
//!
//! # What adjudicates what
//!
//! **The material is OpenSSL's.** `signature_support/no-signed-attributes-
//! crl.der` and `-ocsp.der` are a CRL and an OCSP response OpenSSL 3.0.13
//! issued for that fixture's signer from the CA that issued it (`openssl ca
//! -gencrl`, `openssl ocsp -index`; `signature_support/README.md`). The
//! engine parses neither and judges neither — the store is surfaced and never
//! evaluated — so what this file holds the writer to is that the bytes go in
//! and come back out unchanged, filed where ETSI EN 319 142-1 says they are
//! filed, without disturbing the signature they are for.
//!
//! **The layout is this engine's on both sides.** The `/VRI` key is one
//! function shared by the writer and the reader, so agreement between them is
//! not evidence that the key is the one another validator computes; the key
//! is held to SHA-1 of the stored `/Contents` written out a second way here.

use tinker_pdf::{
    Chain, Coverage, Date, Document, DocumentDigest, SecurityStoreWarning, SignatureCheck,
    TrustAnchors, ValidationData, WriteMode, WriteOptions,
};

const BASE: &[u8] = include_bytes!("signature_support/no-signed-attributes.pdf");
const ROOT: &[u8] = include_bytes!("signature_support/no-signed-attributes-root.der");
const CRL: &[u8] = include_bytes!("signature_support/no-signed-attributes-crl.der");
const OCSP: &[u8] = include_bytes!("signature_support/no-signed-attributes-ocsp.der");

/// Inside every certificate's validity window: 1 January 2027.
const AT: i64 = 1_798_761_600;

const GATHERED: Date = Date {
    year: 2026,
    month: 10,
    day: 2,
    hour: 10,
    minute: 13,
    second: 38,
    utc_offset_minutes: Some(0),
};

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

/// The signer's own certificate, out of its CMS.
fn signer_certificate(document: &Document) -> Vec<u8> {
    let signatures = document.signatures();
    let content = tinker_pdf_pki::ContentInfo::parse(signatures[0].cms()).expect("the CMS parses");
    let certificates: Vec<Vec<u8>> = content
        .signed_data()
        .x509_certificates()
        .map(<[u8]>::to_vec)
        .collect();
    certificates
        .into_iter()
        .find(|der| der != ROOT)
        .expect("the signer's certificate is in the blob")
}

/// `BASE` with a store holding the signer's certificate, the root, the CRL and
/// the OCSP response, all filed under the one signature.
fn with_store() -> Vec<u8> {
    let document = Document::open(BASE.to_vec()).expect("opens");
    let mut data = ValidationData::new();
    data.certificates = vec![signer_certificate(&document), ROOT.to_vec()];
    data.crls = vec![CRL.to_vec()];
    data.ocsp_responses = vec![OCSP.to_vec()];
    data.signatures = vec![document.signatures()[0].contents.clone()];
    data.gathered_at = Some(GATHERED);
    let mut editor = document.editor();
    assert!(editor.add_validation_data(&data));
    editor.save(&incremental())
}

fn decoded(document: &Document, references: &[tinker_pdf::ObjRef]) -> Vec<Vec<u8>> {
    references
        .iter()
        .map(|r| document.cos().stream_decoded(*r).expect("a stream"))
        .collect()
}

#[test]
fn the_material_goes_in_and_comes_back_out_unchanged() {
    let out = with_store();
    assert!(
        out.starts_with(BASE),
        "an incremental save keeps the signed prefix"
    );
    let document = Document::open(out).expect("reopens");
    let store = document.security_store().expect("a /DSS");
    assert!(store.warnings.is_empty(), "{:?}", store.warnings);
    assert!(store.object.is_some(), "written as an object of its own");

    let base = Document::open(BASE.to_vec()).expect("opens");
    assert_eq!(
        decoded(&document, &store.certificates),
        [signer_certificate(&base), ROOT.to_vec()]
    );
    assert_eq!(decoded(&document, &store.crls), [CRL.to_vec()]);
    assert_eq!(decoded(&document, &store.ocsp_responses), [OCSP.to_vec()]);
}

#[test]
fn the_material_is_filed_under_the_signature_it_validates() {
    let document = Document::open(with_store()).expect("reopens");
    let store = document.security_store().expect("a /DSS");
    let signature = &document.signatures()[0];

    // ETSI EN 319 142-1 §5.4.2.2: SHA-1 of the signature's `/Contents`, as
    // uppercase hexadecimal — written out here a second way.
    let digest = tinker_pdf_crypto::sha1::sha1(&signature.contents);
    let expected: String = digest.iter().map(|byte| format!("{byte:02X}")).collect();
    assert_eq!(signature.validation_key(), expected);

    assert_eq!(store.entries.len(), 1);
    let entry = store.entry_for(signature).expect("filed under its key");
    assert_eq!(entry.key, expected);
    assert_eq!(entry.certificates, store.certificates);
    assert_eq!(entry.crls, store.crls);
    assert_eq!(entry.ocsp_responses, store.ocsp_responses);
    assert_eq!(entry.updated, Some(GATHERED));
}

#[test]
fn the_signature_the_store_is_for_still_verifies() {
    let document = Document::open(with_store()).expect("reopens");
    let mut anchors = TrustAnchors::new();
    anchors.add(ROOT.to_vec()).expect("parses");
    let verdicts = document.verify_signatures(&anchors, Some(AT));
    assert_eq!(verdicts.len(), 1);
    let verdict = &verdicts[0];
    assert!(
        matches!(verdict.coverage, Coverage::Revision { .. }),
        "the store is an update after the signature: {:?}",
        verdict.coverage
    );
    assert_eq!(verdict.document_digest, DocumentDigest::Matches);
    assert_eq!(verdict.signature, SignatureCheck::Verified);
    assert!(matches!(verdict.chain, Chain::AnchoredTo { .. }));
    let modifications = document.signatures()[0].modifications(&document);
    assert!(
        !modifications.changes.is_empty(),
        "the update is visible as an update"
    );
}

#[test]
fn a_version_1_7_document_declares_the_store_with_the_esic_extension() {
    let document = Document::open(with_store()).expect("reopens");
    let cos = document.cos();
    let catalog = cos.catalog().expect("a catalog");
    let extensions = cos.resolve_key(&catalog, cos.intern(b"Extensions"));
    let extensions = extensions.as_dict().expect("/Extensions");
    let esic = cos.resolve_key(extensions, cos.intern(b"ESIC"));
    let esic = esic.as_dict().expect("/ESIC");
    let base = esic
        .get_name(cos.intern(b"BaseVersion"))
        .and_then(|n| cos.name_bytes(n));
    assert_eq!(base.as_deref(), Some(&b"1.7"[..]));
    assert_eq!(
        esic.get(cos.intern(b"ExtensionLevel"))
            .and_then(|o| o.as_int()),
        Some(5)
    );
}

#[test]
fn a_second_round_extends_the_store_and_writes_nothing_twice() {
    let first = Document::open(with_store()).expect("reopens");
    let before = first.security_store().expect("a /DSS");

    // The same CRL again, and an OCSP response it does not have yet (the CRL's
    // bytes stand in for one: nothing here parses either).
    let mut data = ValidationData::new();
    data.crls = vec![CRL.to_vec()];
    data.ocsp_responses = vec![b"a second response".to_vec()];
    data.signatures = vec![first.signatures()[0].contents.clone()];
    let mut editor = first.editor();
    assert!(editor.add_validation_data(&data));
    let second = Document::open(editor.save(&incremental())).expect("reopens");
    let after = second.security_store().expect("a /DSS");

    assert_eq!(
        after.object, before.object,
        "the same store, extended in place"
    );
    assert_eq!(after.certificates, before.certificates);
    assert_eq!(
        after.crls, before.crls,
        "an identical CRL is not written twice"
    );
    assert_eq!(after.ocsp_responses.len(), 2);
    assert_eq!(after.ocsp_responses[0], before.ocsp_responses[0]);
    assert_eq!(
        decoded(&second, &after.ocsp_responses[1..]),
        [b"a second response".to_vec()]
    );

    let entry = after
        .entry_for(&second.signatures()[0])
        .expect("still filed");
    assert_eq!(
        entry.ocsp_responses, after.ocsp_responses,
        "the entry gained the new one"
    );
    assert_eq!(entry.crls, after.crls);
    assert_eq!(
        entry.updated,
        Some(GATHERED),
        "kept where the second round gave none"
    );
}

#[test]
fn a_document_without_a_store_has_none() {
    let document = Document::open(BASE.to_vec()).expect("opens");
    assert!(document.security_store().is_none());
}

#[test]
fn a_malformed_store_is_read_leniently_and_says_what_it_skipped() {
    let document = Document::open(BASE.to_vec()).expect("opens");
    let mut editor = document.editor();
    let cos = document.cos();
    let stream = editor.allocate();
    editor.put_stream(
        stream,
        tinker_pdf_cos::write::StreamData {
            dict: tinker_pdf_cos::Dict::new(),
            data: CRL.to_vec(),
        },
    );
    let mut entry = tinker_pdf_cos::Dict::new();
    entry.insert(
        cos.intern(b"CRL"),
        tinker_pdf_cos::Object::Array(vec![tinker_pdf_cos::Object::Ref(stream)]),
    );
    let mut vri = tinker_pdf_cos::Dict::new();
    vri.insert(
        cos.intern(b"NOT-A-DIGEST"),
        tinker_pdf_cos::Object::Dict(entry),
    );
    vri.insert(
        cos.intern(b"0000000000000000000000000000000000000000"),
        tinker_pdf_cos::Object::Int(7),
    );
    // A reference to something that is not a stream: a dictionary.
    let not_a_stream = editor.allocate();
    editor.put(
        not_a_stream,
        tinker_pdf_cos::Object::Dict(tinker_pdf_cos::Dict::new()),
    );
    let mut dss = tinker_pdf_cos::Dict::new();
    dss.insert(
        cos.intern(b"CRLs"),
        tinker_pdf_cos::Object::Array(vec![
            tinker_pdf_cos::Object::Ref(stream),
            tinker_pdf_cos::Object::Int(5),
            tinker_pdf_cos::Object::Ref(not_a_stream),
        ]),
    );
    dss.insert(cos.intern(b"VRI"), tinker_pdf_cos::Object::Dict(vri));
    let dss_key = cos.intern(b"DSS");
    assert!(editor.update_catalog(|catalog| {
        catalog.insert(dss_key, tinker_pdf_cos::Object::Dict(dss));
    }));
    let out = editor.save(&incremental());

    let document = Document::open(out).expect("reopens");
    let store = document
        .security_store()
        .expect("a /DSS, direct in the catalog");
    assert_eq!(store.object, None);
    assert_eq!(
        store.crls.len(),
        1,
        "the stream kept; the integer and the dictionary skipped"
    );
    assert_eq!(decoded(&document, &store.crls), [CRL.to_vec()]);
    assert_eq!(store.entries.len(), 1, "the entry that is a dictionary");
    assert_eq!(store.entries[0].key, "NOT-A-DIGEST");
    assert!(store.warnings.iter().any(|warning| matches!(
        warning,
        SecurityStoreWarning::NotAStream { array, .. } if array == "CRLs"
    )));
    assert!(store.warnings.iter().any(|warning| matches!(
        warning,
        SecurityStoreWarning::KeyNotADigest { key, .. } if key == "NOT-A-DIGEST"
    )));
    assert!(store.warnings.iter().any(|warning| matches!(
        warning,
        SecurityStoreWarning::EntryNotADictionary { key, .. }
            if key == "0000000000000000000000000000000000000000"
    )));
}

/// `BASE` with the catalog's `/DSS` set to what `dss` builds, saved
/// incrementally. Names are interned per document, so `dss` is handed the
/// document it builds for.
fn with_dss(dss: impl FnOnce(&tinker_pdf_cos::CosDocument) -> tinker_pdf_cos::Object) -> Vec<u8> {
    let document = Document::open(BASE.to_vec()).expect("opens");
    let mut editor = document.editor();
    let cos = document.cos();
    let (dss_key, dss) = (cos.intern(b"DSS"), dss(cos));
    assert!(editor.update_catalog(|catalog| {
        catalog.insert(dss_key, dss);
    }));
    editor.save(&incremental())
}

/// Ruling 10, for the members whose *type* is wrong: each skip is named,
/// where an entry that is not an array, a `/VRI` that is not a dictionary and
/// a `/TU` that is not a date all used to read as nothing at all. This is the
/// reviewer's probe — `/Certs 5 /CRLs true /VRI 7` — kept.
#[test]
fn a_store_of_the_wrong_types_names_every_member_it_could_not_read() {
    let probe = with_dss(|cos| {
        let mut dss = tinker_pdf_cos::Dict::new();
        dss.insert(cos.intern(b"Certs"), tinker_pdf_cos::Object::Int(5));
        dss.insert(cos.intern(b"CRLs"), tinker_pdf_cos::Object::Bool(true));
        dss.insert(cos.intern(b"VRI"), tinker_pdf_cos::Object::Int(7));
        tinker_pdf_cos::Object::Dict(dss)
    });
    let store = Document::open(probe)
        .expect("reopens")
        .security_store()
        .expect("a /DSS, direct in the catalog");
    assert!(store.certificates.is_empty() && store.crls.is_empty() && store.entries.is_empty());
    assert_eq!(
        store.warnings,
        [
            SecurityStoreWarning::NotAnArray {
                store: None,
                array: "Certs".into()
            },
            SecurityStoreWarning::NotAnArray {
                store: None,
                array: "CRLs".into()
            },
            SecurityStoreWarning::VriNotADictionary { store: None },
        ],
        "and nothing for the /OCSPs that is absent"
    );

    // The same inside a `/VRI` entry, and a `/TU` that is not a date.
    let key = "0000000000000000000000000000000000000000";
    let inside = with_dss(|cos| {
        let mut entry = tinker_pdf_cos::Dict::new();
        entry.insert(cos.intern(b"Cert"), tinker_pdf_cos::Object::Int(5));
        entry.insert(
            cos.intern(b"TU"),
            tinker_pdf_cos::Object::String(tinker_pdf_cos::PdfString::literal(
                b"last Tuesday".to_vec(),
            )),
        );
        let mut vri = tinker_pdf_cos::Dict::new();
        vri.insert(
            cos.intern(key.as_bytes()),
            tinker_pdf_cos::Object::Dict(entry),
        );
        let mut dss = tinker_pdf_cos::Dict::new();
        dss.insert(cos.intern(b"VRI"), tinker_pdf_cos::Object::Dict(vri));
        tinker_pdf_cos::Object::Dict(dss)
    });
    let store = Document::open(inside)
        .expect("reopens")
        .security_store()
        .expect("a /DSS");
    assert_eq!(store.entries.len(), 1, "the entry is kept");
    assert_eq!(store.entries[0].updated, None);
    assert_eq!(
        store.warnings,
        [
            SecurityStoreWarning::DateUnreadable {
                store: None,
                key: key.into()
            },
            SecurityStoreWarning::NotAnArray {
                store: None,
                array: "Cert".into()
            },
        ]
    );
}

/// A `/DSS` that is not a dictionary is a store that could not be read, and
/// says so. One that is null — or a reference to an object the file does not
/// have, which 7.3.10 makes null — is no store, as an absent one is.
#[test]
fn a_store_that_is_not_a_dictionary_is_named_and_a_null_one_is_none() {
    let store = Document::open(with_dss(|_| tinker_pdf_cos::Object::Int(5)))
        .expect("reopens")
        .security_store()
        .expect("a /DSS is there");
    assert!(store.certificates.is_empty() && store.entries.is_empty());
    assert_eq!(
        store.warnings,
        [SecurityStoreWarning::NotADictionary { store: None }]
    );

    let nowhere = tinker_pdf_cos::ObjRef { num: 9_999, gen: 0 };
    let document =
        Document::open(with_dss(|_| tinker_pdf_cos::Object::Ref(nowhere))).expect("reopens");
    assert!(document.security_store().is_none());
    let document = Document::open(with_dss(|_| tinker_pdf_cos::Object::Null)).expect("reopens");
    assert!(document.security_store().is_none());
}

#[test]
fn the_update_adds_nothing_the_strict_validator_refuses() {
    let rendered = |pdf: &[u8]| {
        Document::open(pdf.to_vec())
            .expect("opens")
            .validate()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(rendered(&with_store()), rendered(BASE));
}
