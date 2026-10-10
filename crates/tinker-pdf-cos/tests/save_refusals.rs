//! The two decisions `tpdf` used to make on its own, made by the editor so
//! that every surface can ask them: whether a save would write an encrypted
//! document's plaintext unasked, and whether replacing or removing its
//! encryption lifts what the owner withheld from the user who opened it
//! (7.6.4.2, Table 22). `DocumentEditor::check_save` and
//! `DocumentEditor::check_decrypt` answer; the save doors do not ask.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{
    AuthLevel, CosDocument, DocumentEditor, Encryption, Object, SaveRefusal, WriteMode,
    WriteOptions,
};

fn open(name: &str, password: Option<&str>) -> Arc<CosDocument> {
    open_bytes(fixture(name), password)
}

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn open_bytes(bytes: Vec<u8>, password: Option<&str>) -> Arc<CosDocument> {
    let doc = CosDocument::open(bytes).expect("the fixture opens");
    if let Some(password) = password {
        doc.authenticate(password).expect("the password opens it");
    }
    Arc::new(doc)
}

/// `encrypted-aes256.pdf` with `entry` (`StmF` or `StrF`) naming `/Identity`
/// rather than its AES-256 filter, opened with the user's password: an
/// encrypted document whose key passes that half of what it is given
/// through unchanged (7.6.5 Table 25). The `/Encrypt` dictionary sits in the
/// trailer, after the cross-reference table, so no offset moves; and
/// revision 6's `/Perms` covers `/P` and `/EncryptMetadata`, not the
/// filters, so the password still opens it.
fn identity(entry: &str) -> Arc<CosDocument> {
    let bytes = fixture("encrypted-aes256.pdf");
    let from = format!("/{entry}/StdCF");
    let at = bytes
        .windows(from.len())
        .position(|window| window == from.as_bytes())
        .unwrap_or_else(|| panic!("the fixture names {from}"));
    let mut edited = bytes[..at].to_vec();
    edited.extend_from_slice(format!("/{entry}/Identity").as_bytes());
    edited.extend_from_slice(&bytes[at + from.len()..]);
    let doc = open_bytes(edited, Some("open-sesame"));
    assert!(
        doc.file_key().is_some(),
        "{entry}: authenticated, with a key"
    );
    doc
}

/// A hex string as a PDF writes one.
fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("<{digits}>")
}

/// A document under the standard handler at revision 3 or 4, whose user
/// password is `u` and owner password `o`, with an `n`-byte file key. Objects
/// 1 onwards are what `objects` makes of the file key, object 1 the catalog;
/// the `/Encrypt` dictionary follows them, `encrypt` giving its entries
/// before `/O`, `/U` and `/P`.
///
/// Built here from Algorithms 2, 3 and 5 (7.6.4.3, 7.6.4.4) with the
/// engine's own MD5 and RC4, so no outside program made it.
fn standard_handler(
    version: &str,
    n: usize,
    encrypt: &str,
    objects: impl FnOnce(&[u8]) -> Vec<Vec<u8>>,
) -> Vec<u8> {
    use tinker_pdf_crypto::md5::md5;
    use tinker_pdf_crypto::rc4::rc4;

    const PAD: [u8; 32] = [
        0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01,
        0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53,
        0x69, 0x7A,
    ];
    let pad = |password: &[u8]| -> Vec<u8> {
        password
            .iter()
            .chain(PAD.iter())
            .take(32)
            .copied()
            .collect()
    };
    let stepped = |key: &[u8], i: u8| -> Vec<u8> { key.iter().map(|b| b ^ i).collect() };
    let p: i32 = -4;
    let id: Vec<u8> = (0..16).collect();

    // Algorithm 3: /O from the owner password `o` and the user password `u`.
    let mut digest = md5(&pad(b"o"));
    for _ in 0..50 {
        digest = md5(&digest);
    }
    let owner_key = &digest[..n];
    let mut o = rc4(owner_key, &pad(b"u"));
    for i in 1..=19 {
        o = rc4(&stepped(owner_key, i), &o);
    }

    // Algorithm 2: the file key from `u`.
    let mut input = pad(b"u");
    input.extend_from_slice(&o);
    input.extend_from_slice(&p.to_le_bytes());
    input.extend_from_slice(&id);
    let mut digest = md5(&input);
    for _ in 0..50 {
        digest = md5(&digest[..n]);
    }
    let key = &digest[..n];

    // Algorithm 5: /U.
    let mut seed = PAD.to_vec();
    seed.extend_from_slice(&id);
    let mut u = rc4(key, &md5(&seed));
    for i in 1..=19 {
        u = rc4(&stepped(key, i), &u);
    }
    u.resize(32, 0);

    let mut objects = objects(key);
    objects.push(
        format!(
            "<< /Filter /Standard {encrypt} /O {} /U {} /P {p} >>",
            hex(&o),
            hex(&u)
        )
        .into_bytes(),
    );
    let mut bytes = format!("%PDF-{version}\n").into_bytes();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Encrypt {} 0 R /ID [{} {}] >>\n\
             startxref\n{xref}\n%%EOF\n",
            objects.len() + 1,
            objects.len(),
            hex(&id),
            hex(&id)
        )
        .as_bytes(),
    );
    bytes
}

/// A one-page document under the standard handler at `/V 4 /R 4` whose crypt
/// filter is `/AESV2` with a 40-bit key, opened with its user password `u`.
/// Algorithm 1 keys AES-128 with the first n + 5 bytes of a hash, here ten,
/// which AES does not take — so the key authenticates and every encryption
/// under it hands its bytes back unchanged. `/Length 40` is written at the
/// top level (Table 20) and in the crypt filter (Table 25, where the
/// standard handler counts bytes), so the key is five bytes whichever the
/// reader takes; an absent `/Length` reads as 40 too.
fn forty_bit_aesv2() -> Arc<CosDocument> {
    let n = 5;
    let bytes = standard_handler(
        "1.6",
        n,
        "/V 4 /R 4 /Length 40 \
         /CF << /StdCF << /CFM /AESV2 /AuthEvent /DocOpen /Length 5 >> >> \
         /StmF /StdCF /StrF /StdCF",
        |_| {
            [
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
            ]
            .map(|object| object.as_bytes().to_vec())
            .to_vec()
        },
    );

    let doc = open_bytes(bytes, Some("u"));
    let key = doc.file_key().expect("authenticated, with a key");
    assert_eq!(key.key().len(), n, "a 40-bit file key");
    doc
}

/// The string [`rc4_with_a_string_in_a_stream_dictionary`] keeps in its
/// content stream's dictionary, and the text its content stream shows.
const DICTIONARY_SECRET: &[u8] = b"DICTSTRINGSECRET";
const VISIBLE_TEXT: &[u8] = b"VISIBLETEXT";

/// A one-page document under the standard handler at `/V 2 /R 3` with a
/// 128-bit RC4 key, opened with its user password `u`, whose content
/// stream's dictionary carries a string, `/Secret` — where an embedded
/// file's `/Params` keeps `/CheckSum` and `/ModDate` (7.11.4 Table 45), or a
/// form its `/PieceInfo` — encrypted with object 4's key, as 7.6.2 has every
/// string encrypted.
fn rc4_with_a_string_in_a_stream_dictionary() -> Arc<CosDocument> {
    use tinker_pdf_crypto::md5::md5;
    use tinker_pdf_crypto::rc4::rc4;

    let bytes = standard_handler("1.4", 16, "/V 2 /R 3 /Length 128", |key| {
        // Algorithm 1: object 4's key, the file key salted with the low three
        // bytes of its number and two of its generation.
        let mut salted = key.to_vec();
        salted.extend_from_slice(&[4, 0, 0, 0, 0]);
        let digest = md5(&salted);
        let object_key = &digest[..(key.len() + 5).min(16)];
        let content = rc4(object_key, b"BT /F1 12 Tf 72 700 Td (VISIBLETEXT) Tj ET");
        let mut stream = format!(
            "<< /Length {} /Secret {} >>\nstream\n",
            content.len(),
            hex(&rc4(object_key, DICTIONARY_SECRET))
        )
        .into_bytes();
        stream.extend_from_slice(&content);
        stream.extend_from_slice(b"\nendstream");
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
              /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_vec(),
            stream,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ]
    });
    open_bytes(bytes, Some("u"))
}

fn rewrite() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    }
}

fn encrypted() -> WriteOptions {
    WriteOptions {
        encryption: Some(Encryption {
            user_password: "u".to_string(),
            owner_password: "o".to_string(),
            permissions: -1,
            entropy: [7u8; 48],
        }),
        ..rewrite()
    }
}

/// A document that is not encrypted has nothing a save could undo, and
/// nothing to decrypt.
#[test]
fn a_document_that_is_not_encrypted_saves_any_way_and_has_nothing_to_decrypt() {
    let editor = DocumentEditor::new(open("simple-text.pdf", None));
    for options in [rewrite(), incremental(), encrypted()] {
        assert_eq!(editor.check_save(&options), Ok(()));
    }
    assert_eq!(editor.check_decrypt(), Err(SaveRefusal::NotEncrypted));
}

/// A rewrite asking for no encryption drops `/Encrypt` and writes the
/// plaintext, so it is refused; an incremental update is sealed with the
/// file's own key, and a rewrite with encryption of its own is sealed with
/// that, so neither is. A user the owner withheld nothing from may replace
/// the encryption, and may decrypt on purpose.
#[test]
fn a_rewrite_of_an_encrypted_document_asking_for_no_encryption_would_decrypt_it() {
    let doc = open("encrypted-aes256.pdf", Some("open-sesame"));
    assert_eq!(doc.auth_level(), AuthLevel::User);
    let editor = DocumentEditor::new(doc);
    assert_eq!(
        editor.check_save(&rewrite()),
        Err(SaveRefusal::WouldDecrypt)
    );
    assert_eq!(editor.check_save(&incremental()), Ok(()));
    assert_eq!(editor.check_save(&encrypted()), Ok(()));
    assert_eq!(editor.check_decrypt(), Ok(()));
}

/// `permissions-noprint.pdf` withholds printing from its user. Opened with
/// the user's password, replacing or removing its encryption would lift
/// that, and both are refused with what is withheld named; opened with the
/// owner's, both are allowed.
#[test]
fn changing_the_encryption_needs_the_owner_where_the_user_is_restricted() {
    let user = DocumentEditor::new(open("permissions-noprint.pdf", Some("user")));
    let refusal = user.check_save(&encrypted()).expect_err("refused");
    assert!(matches!(refusal, SaveRefusal::OwnerAuthorityNeeded { .. }));
    assert_eq!(refusal.withheld(), ["print", "print at high resolution"]);
    assert_eq!(user.check_decrypt(), Err(refusal));
    assert_eq!(user.check_save(&incremental()), Ok(()), "nothing changes");
    assert_eq!(
        refusal.to_string(),
        "opened as its user, from whom the owner withholds print, print at high resolution; \
         changing the encryption would lift that, so it needs the owner password"
    );

    let owner = DocumentEditor::new(open("permissions-noprint.pdf", Some("owner")));
    assert_eq!(owner.check_save(&encrypted()), Ok(()));
    assert_eq!(owner.check_decrypt(), Ok(()));
}

/// A page copied in from an encrypted document arrives as plaintext, so a
/// save of the document it joined that writes no encryption writes it so —
/// and a rollback that undoes the import undoes that too.
#[test]
fn a_page_imported_from_an_encrypted_document_would_be_written_decrypted() {
    let source = open("encrypted-aes256.pdf", Some("owner-secret"));
    let mut editor = DocumentEditor::new(open("simple-text.pdf", None));
    let before = editor.checkpoint();
    editor.import_page(&source, 0, 1).expect("imported");
    assert_eq!(
        editor.check_save(&rewrite()),
        Err(SaveRefusal::WouldDecrypt)
    );
    assert_eq!(
        editor.check_save(&incremental()),
        Err(SaveRefusal::WouldDecrypt),
        "the document it joined has no key to seal it with"
    );
    assert_eq!(editor.check_save(&encrypted()), Ok(()));

    editor.restore(&before);
    assert_eq!(editor.check_save(&rewrite()), Ok(()));

    let plain = open("simple-text.pdf", None);
    editor.import_page(&plain, 0, 1).expect("imported");
    assert_eq!(
        editor.check_save(&rewrite()),
        Ok(()),
        "a source that is not encrypted"
    );
}

/// The decoded first content stream of `doc`'s first page: what a reader
/// holding the password sees. The fixture's `/Contents` is an array of one.
fn first_page_content(doc: &CosDocument) -> Vec<u8> {
    let pages = tinker_pdf_cos::pages::collect(doc);
    let page = pages.first().expect("a page");
    let object = doc.get(page.reference).expect("the page");
    let page = object.as_dict().expect("a page dictionary");
    let key = doc.intern(b"Contents");
    let contents = page
        .get_ref(key)
        .or_else(|| {
            page.get_array(key)
                .and_then(|streams| streams.first())
                .and_then(|first| first.as_objref())
        })
        .expect("a content stream");
    doc.stream_decoded(contents).expect("decodes")
}

/// Whether `saved` holds `plaintext` as written, unencrypted.
fn holds(saved: &[u8], plaintext: &[u8]) -> bool {
    let probe = &plaintext[..plaintext.len().min(24)];
    saved.windows(probe.len()).any(|window| window == probe)
}

/// The answer is held to what the save door does, arm by arm: whenever
/// `check_save` answers `Ok`, the saved bytes do not hold the plaintext of
/// the encrypted page copied in. An incremental update is sealed with the key
/// the document was opened with and with nothing else — it does not read
/// `WriteOptions::encryption`, and without a key it writes in the clear — so
/// encryption asked of an incremental update seals nothing, and an encrypted
/// document opened without its password has no key to seal with. Nor does a
/// key seal what its `/Identity` stream or string method passes through, or
/// what an AES method under a key AES does not take hands back unchanged.
#[test]
fn an_incremental_update_is_sealed_only_with_the_key_the_document_was_opened_with() {
    let source = open("encrypted-aes256.pdf", Some("owner-secret"));
    let plaintext = first_page_content(&source);
    assert!(plaintext.starts_with(b"BT"), "{plaintext:?}");
    let incremental_encrypted = WriteOptions {
        mode: WriteMode::Incremental,
        ..encrypted()
    };

    let targets = [
        ("a plain document", open("simple-text.pdf", None)),
        (
            "an encrypted document opened without its password",
            open("encrypted-aes256.pdf", None),
        ),
        (
            "an encrypted document opened with its password",
            open("encrypted-aes256.pdf", Some("open-sesame")),
        ),
        (
            "an encrypted document whose streams' filter is /Identity",
            identity("StmF"),
        ),
        (
            "an encrypted document whose strings' filter is /Identity",
            identity("StrF"),
        ),
        (
            "an encrypted document whose AESV2 key is 40 bits",
            forty_bit_aesv2(),
        ),
    ];
    let mut answers = Vec::new();
    for (target, doc) in targets {
        let mut editor = DocumentEditor::new(doc);
        editor.import_page(&source, 0, 1).expect("imported");
        for (how, options) in [
            ("rewrite", rewrite()),
            ("incremental", incremental()),
            (
                "incremental asking for encryption",
                incremental_encrypted.clone(),
            ),
            ("rewrite with encryption", encrypted()),
        ] {
            let answer = editor.check_save(&options);
            let saved = editor.save(&options);
            if answer.is_ok() {
                assert!(
                    !holds(&saved, &plaintext),
                    "{target}, {how}: Ok, and the saved file holds the plaintext"
                );
            }
            answers.push((target, how, answer));
        }
    }
    let refused = |target: &str, how: &str| {
        answers
            .iter()
            .find(|(t, h, _)| *t == target && *h == how)
            .map(|(_, _, answer)| *answer)
    };
    assert_eq!(
        refused("a plain document", "incremental asking for encryption"),
        Some(Err(SaveRefusal::WouldDecrypt)),
        "an incremental update does not read the encryption asked of it"
    );
    assert_eq!(
        refused(
            "an encrypted document opened without its password",
            "incremental"
        ),
        Some(Err(SaveRefusal::WouldDecrypt)),
        "no key to seal the import with"
    );
    assert_eq!(
        refused(
            "an encrypted document opened with its password",
            "incremental"
        ),
        Some(Ok(())),
        "sealed with the file's own key"
    );
    // A key whose stream or string method is `/Identity` passes those bytes
    // through, so the update writes what was copied in as the target writes
    // its own: in the clear. The page copied in carries no string, so the
    // loop above sees only the stream half leak; the string half is the same
    // refusal, for an import that does carry one.
    // So does a key under which AES cannot run: Algorithm 1 gives a 40-bit
    // file key a ten-byte AES key, and encryption hands the bytes back as
    // given rather than fail.
    for target in [
        "an encrypted document whose streams' filter is /Identity",
        "an encrypted document whose strings' filter is /Identity",
        "an encrypted document whose AESV2 key is 40 bits",
    ] {
        assert_eq!(
            refused(target, "incremental"),
            Some(Err(SaveRefusal::WouldDecrypt)),
            "{target}: a key that passes the import through seals nothing"
        );
    }
}

/// An incremental update leaves the document's own encryption standing, so
/// encryption asked of one replaces nothing and lifts nothing the owner
/// withheld; and an encrypted document opened without its password was never
/// decrypted, so an update of it writes nothing it protected.
#[test]
fn an_incremental_update_replaces_no_encryption() {
    let user = DocumentEditor::new(open("permissions-noprint.pdf", Some("user")));
    let options = WriteOptions {
        mode: WriteMode::Incremental,
        ..encrypted()
    };
    assert_eq!(user.check_save(&options), Ok(()));

    let locked = DocumentEditor::new(open("encrypted-aes256.pdf", None));
    assert_eq!(locked.check_save(&incremental()), Ok(()));
    assert_eq!(
        locked.check_save(&rewrite()),
        Err(SaveRefusal::WouldDecrypt),
        "a rewrite drops /Encrypt whatever it could read"
    );
}

/// Whether `saved` holds `plaintext` in either form a string is written in:
/// as given, or as the digits of a hex string.
fn holds_string(saved: &[u8], plaintext: &[u8]) -> bool {
    let upper: String = plaintext.iter().map(|b| format!("{b:02X}")).collect();
    holds(saved, plaintext)
        || holds(saved, upper.as_bytes())
        || holds(saved, upper.to_ascii_lowercase().as_bytes())
}

/// Every `/Secret` string on a content stream's dictionary in `doc`, page by
/// page, as the reader decrypts it.
fn dictionary_secrets(doc: &CosDocument) -> Vec<Vec<u8>> {
    let key = doc.intern(b"Secret");
    tinker_pdf_cos::pages::collect(doc)
        .iter()
        .flat_map(|page| tinker_pdf_cos::pages::contents(doc, page))
        .filter_map(|reference| doc.get(reference).ok())
        .filter_map(|object| match object.as_ref() {
            Object::Stream(stream) => stream.dict.get_string(key).map(|s| s.bytes.clone()),
            _ => None,
        })
        .collect()
}

/// Whatever `check_save` answers `Ok` seals every string copied in, the ones
/// in a stream's own dictionary among them (7.6.2). The writer sealed a
/// stream's bytes and wrote its dictionary as given, so each save below
/// wrote `/Secret` in the clear, and the file reopened read it as clear bytes
/// decrypted as if they were ciphertext: a rewrite with encryption of its
/// own, ordinary or linearized, of the encrypted source or of a plain
/// document its page joined; an update of an AES-256 document under its own
/// key; and an update of the RC4 source under its RC4 key.
#[test]
fn a_string_in_a_stream_dictionary_is_sealed_wherever_the_answer_is_ok() {
    let source = rc4_with_a_string_in_a_stream_dictionary();
    assert_eq!(
        dictionary_secrets(&source),
        [DICTIONARY_SECRET],
        "the source reads its own string"
    );
    let linearized = WriteOptions {
        linearize: true,
        ..encrypted()
    };
    let cases = [
        (
            "the source, rewritten with encryption",
            Arc::clone(&source),
            encrypted(),
            "u",
            2,
        ),
        (
            "the source, rewritten linearized with encryption",
            Arc::clone(&source),
            linearized.clone(),
            "u",
            2,
        ),
        (
            "the source, updated under its RC4 key",
            Arc::clone(&source),
            incremental(),
            "u",
            2,
        ),
        (
            "an encrypted document opened with its password, updated",
            open("encrypted-aes256.pdf", Some("open-sesame")),
            incremental(),
            "open-sesame",
            1,
        ),
        (
            "a plain document, rewritten with encryption",
            open("simple-text.pdf", None),
            encrypted(),
            "u",
            1,
        ),
        (
            "a plain document, rewritten linearized with encryption",
            open("simple-text.pdf", None),
            linearized,
            "u",
            1,
        ),
    ];
    for (target, doc, options, password, pages) in cases {
        let mut editor = DocumentEditor::new(doc);
        editor.import_page(&source, 0, 1).expect("imported");
        assert_eq!(editor.check_save(&options), Ok(()), "{target}");
        let saved = editor.save(&options);
        assert!(
            !holds_string(&saved, DICTIONARY_SECRET),
            "{target}: Ok, and the saved file holds the string in the clear"
        );
        assert!(
            !holds(&saved, VISIBLE_TEXT),
            "{target}: Ok, and the saved file holds the content in the clear"
        );
        let reopened = open_bytes(saved, Some(password));
        assert_eq!(
            dictionary_secrets(&reopened),
            vec![DICTIONARY_SECRET.to_vec(); pages],
            "{target}: the string reads back as it was"
        );
    }
}
