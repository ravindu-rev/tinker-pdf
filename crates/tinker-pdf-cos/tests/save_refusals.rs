//! The two decisions `tpdf` used to make on its own, made by the editor so
//! that every surface can ask them: whether a save would write an encrypted
//! document's plaintext unasked, and whether replacing or removing its
//! encryption lifts what the owner withheld from the user who opened it
//! (7.6.4.2, Table 22). `DocumentEditor::check_save` and
//! `DocumentEditor::check_decrypt` answer; the save doors do not ask.

use std::path::PathBuf;
use std::sync::Arc;

use tinker_pdf_cos::{
    AuthLevel, CosDocument, DocumentEditor, Encryption, SaveRefusal, WriteMode, WriteOptions,
};

fn open(name: &str, password: Option<&str>) -> Arc<CosDocument> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let doc = CosDocument::open(bytes).expect("the fixture opens");
    if let Some(password) = password {
        doc.authenticate(password).expect("the password opens it");
    }
    Arc::new(doc)
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
