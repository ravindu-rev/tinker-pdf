//! Certified documents, and what later revisions were allowed to do (12.8.2).
//!
//! Every fixture here is produced by this engine's own signing path and then
//! read back by its own analysis, which is the shape ruling 13 leaves and the
//! limit it imposes: what these tests establish is that the writer and the
//! reader agree about `/DocMDP`, not that a third party would agree with
//! either. The corpus cannot help — of its eighteen signatures, none carries a
//! `/DocMDP` reference at all.
//!
//! What makes them worth more than a tautology is that the two halves are
//! written against different things. The writer emits 12.8.2.2's `/Reference`
//! array from a `Certification` enum; the reader classifies objects by walking
//! cross-reference entries and the form tree, and has no idea what the writer
//! intended. A disagreement about which objects a fill touches shows up here.

use std::cell::RefCell;

use tinker_pdf::{
    Certification, Change, Coverage, DigestAlgorithm, Document, FieldLock, SignRefused, Signer,
    SigningRequest, SigningTarget, Touched, WriteMode, WriteOptions,
};

struct Stub;

impl Signer for Stub {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }
    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        Ok(digest.to_vec())
    }
}

/// Counts how many times it was asked, so a test can prove a save happened.
struct Counting(RefCell<usize>);

impl Signer for Counting {
    fn digest_algorithm(&self) -> DigestAlgorithm {
        DigestAlgorithm::Sha256
    }
    fn sign(&self, digest: &[u8]) -> Result<Vec<u8>, SignRefused> {
        *self.0.borrow_mut() += 1;
        Ok(digest.to_vec())
    }
}

fn incremental() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Incremental,
        ..Default::default()
    }
}

/// A one-page document with one text field called `Name`.
///
/// Hand-built because `testdata/` carries no form and `DocumentBuilder` has no
/// field API — and because a fixture whose every byte is visible in this file
/// is the right kind for a test about which objects changed.
fn form_document() -> Vec<u8> {
    let objects: [(u32, &str); 5] = [
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R \
             /AcroForm << /Fields [4 0 R] /DA (/Helv 12 Tf 0 g) >> >>",
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [4 0 R] >>",
        ),
        (
            4,
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /V () \
             /Rect [10 10 200 40] /F 4 /P 3 0 R /DA (/Helv 12 Tf 0 g) >>",
        ),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ];

    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0u64; objects.len() + 1];
    for (num, body) in objects {
        offsets[num as usize] = out.len() as u64;
        out.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = out.len() as u64;
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for entry in offsets.iter().take(6).skip(1) {
        out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    out
}

/// Certifies `bytes` at `level`, optionally locking fields.
fn certify(bytes: Vec<u8>, level: Certification, lock: Option<FieldLock>) -> Vec<u8> {
    let signer = Stub;
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Certification".to_string(),
        },
        &signer,
    );
    request.reserve = 1024;
    request.certification = Some(level);
    request.field_lock = lock;
    Document::open(bytes)
        .expect("the fixture opens")
        .editor()
        .save_signed(&incremental(), &request)
        .expect("certifying succeeds")
}

fn only_signature(document: &Document) -> tinker_pdf::Signature {
    let signatures = document.signatures();
    assert_eq!(signatures.len(), 1, "one certifying signature");
    signatures.into_iter().next().expect("one")
}

// ---- what the writer wrote, read back --------------------------------------

#[test]
fn a_certified_document_reports_its_level_and_its_perms_entry() {
    for level in [
        Certification::NoChanges,
        Certification::FormFillAndSigning,
        Certification::FormFillSigningAndAnnotations,
    ] {
        let certified = certify(form_document(), level, None);
        let document = Document::open(certified).expect("reopens");

        assert_eq!(
            document.certification(),
            Some(level),
            "{level:?} must survive the round trip"
        );
        let signature = only_signature(&document);
        assert_eq!(signature.certification, Some(level));
        assert_eq!(signature.coverage, Coverage::WholeFile);
        assert!(
            signature.modifications(&document).is_unmodified(),
            "nothing has happened to it yet"
        );

        // 12.8.4: the catalog's `/Perms /DocMDP` must name the signature, and
        // a second signature reachable only that way would be a duplicate.
        assert_eq!(
            document.signatures().len(),
            1,
            "the /Perms entry is the same object as the field's /V"
        );
    }
}

#[test]
fn a_field_lock_survives_the_round_trip_in_all_three_shapes() {
    for lock in [
        FieldLock::All,
        FieldLock::Include(vec!["Name".to_string()]),
        FieldLock::Exclude(vec!["Name".to_string(), "Other".to_string()]),
    ] {
        let certified = certify(
            form_document(),
            Certification::FormFillAndSigning,
            Some(lock.clone()),
        );
        let document = Document::open(certified).expect("reopens");
        assert_eq!(only_signature(&document).field_lock, Some(lock));
    }
}

// ---- and what later revisions did to it ------------------------------------

/// The milestone's first case: a form fill under level 2 is permitted.
#[test]
fn filling_a_field_under_level_two_is_permitted() {
    let certified = certify(form_document(), Certification::FormFillAndSigning, None);

    let mut editor = Document::open(certified).expect("reopens").editor();
    assert!(
        editor.fill_field("Name", "Ada Lovelace").is_ok(),
        "the field fills"
    );
    let filled = editor.save(&incremental());

    let document = Document::open(filled).expect("the filled document opens");
    let signature = only_signature(&document);
    assert_eq!(
        signature.coverage,
        Coverage::Revision { index: 1 },
        "the fill is a revision after the signature"
    );

    let report = signature.modifications(&document);
    assert_eq!(
        report.certification,
        Some(Certification::FormFillAndSigning)
    );
    assert!(!report.is_unmodified(), "the fill changed something");
    let disallowed: Vec<_> = report.disallowed().collect();
    assert!(
        disallowed.is_empty(),
        "level 2 permits a fill; refused {disallowed:?}"
    );
    assert!(
        report.changes.iter().any(|change| matches!(
            &change.what,
            Touched::FormField { name, .. } if name.as_deref() == Some("Name")
        )),
        "and the field itself is among the changes: {:?}",
        report.changes
    );
}

/// The milestone's second case: a page edit under level 1 is not, and the
/// object is named.
#[test]
fn editing_a_page_under_level_one_is_disallowed_and_names_the_object() {
    let certified = certify(form_document(), Certification::NoChanges, None);

    let mut editor = Document::open(certified).expect("reopens").editor();
    assert!(editor.rotate_page(0, 90), "the page rotates");
    let rotated = editor.save(&incremental());

    let document = Document::open(rotated).expect("opens");
    let signature = only_signature(&document);
    let report = signature.modifications(&document);

    let disallowed: Vec<_> = report.disallowed().collect();
    assert!(!disallowed.is_empty(), "level 1 permits nothing");
    let page = disallowed
        .iter()
        .find(|change| change.what == Touched::Page)
        .expect("the page is named among the disallowed changes");
    assert_eq!(page.change, Change::Altered);
    assert_eq!(
        page.object.num, 3,
        "and it is object 3, the page, by number"
    );
}

/// The same page edit under level 3, which still refuses it — the levels are
/// about *which* changes, not about how many.
#[test]
fn a_page_edit_is_refused_at_every_level() {
    for level in [
        Certification::NoChanges,
        Certification::FormFillAndSigning,
        Certification::FormFillSigningAndAnnotations,
    ] {
        let certified = certify(form_document(), level, None);
        let mut editor = Document::open(certified).expect("reopens").editor();
        assert!(editor.rotate_page(0, 90));
        let rotated = editor.save(&incremental());

        let document = Document::open(rotated).expect("opens");
        let report = only_signature(&document).modifications(&document);
        assert!(
            report
                .disallowed()
                .any(|change| change.what == Touched::Page),
            "{level:?} must not permit a page edit"
        );
    }
}

/// The milestone's third case: a locked field's edit is detected, even at a
/// level that would otherwise permit it.
#[test]
fn filling_a_locked_field_is_disallowed_though_the_level_permits_filling() {
    let certified = certify(
        form_document(),
        Certification::FormFillSigningAndAnnotations,
        Some(FieldLock::Include(vec!["Name".to_string()])),
    );

    let mut editor = Document::open(certified).expect("reopens").editor();
    assert!(editor.fill_field("Name", "Ada Lovelace").is_ok());
    let filled = editor.save(&incremental());

    let document = Document::open(filled).expect("opens");
    let report = only_signature(&document).modifications(&document);
    let locked: Vec<_> = report
        .disallowed()
        .filter(|change| {
            matches!(&change.what, Touched::FormField { name, .. } if name.as_deref() == Some("Name"))
        })
        .collect();
    assert!(
        !locked.is_empty(),
        "12.8.2.4 overrides 12.8.2.2: {:?}",
        report.changes
    );
}

/// And the same fill with the lock naming a *different* field is permitted,
/// so the previous test is not passing because everything is refused.
#[test]
fn a_lock_on_another_field_leaves_this_one_fillable() {
    let certified = certify(
        form_document(),
        Certification::FormFillAndSigning,
        Some(FieldLock::Include(vec!["SomethingElse".to_string()])),
    );

    let mut editor = Document::open(certified).expect("reopens").editor();
    assert!(editor.fill_field("Name", "Ada Lovelace").is_ok());
    let filled = editor.save(&incremental());

    let document = Document::open(filled).expect("opens");
    let report = only_signature(&document).modifications(&document);
    let disallowed: Vec<_> = report.disallowed().collect();
    assert!(
        disallowed.is_empty(),
        "only the named field is locked; refused {disallowed:?}"
    );
}

/// Signing again after a level-2 certification is what level 2 exists to
/// permit, and it is the case with the most moving parts: a second signature
/// adds a field, a widget, a signature dictionary and rewrites the form.
#[test]
fn adding_a_second_signature_under_level_two_is_permitted() {
    let certified = certify(form_document(), Certification::FormFillAndSigning, None);

    let signer = Counting(RefCell::new(0));
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Approval".to_string(),
        },
        &signer,
    );
    request.reserve = 1024;
    let twice = Document::open(certified)
        .expect("reopens")
        .editor()
        .save_signed(&incremental(), &request)
        .expect("the approval signature");
    assert_eq!(*signer.0.borrow(), 1, "the signer ran exactly once");

    let document = Document::open(twice).expect("opens");
    let mut signatures = document.signatures();
    signatures.sort_by(|a, b| a.field.cmp(&b.field));
    assert_eq!(signatures.len(), 2);

    let certification = signatures
        .iter()
        .find(|signature| signature.certification.is_some())
        .expect("the certifying signature");
    let report = certification.modifications(&document);
    let disallowed: Vec<_> = report.disallowed().collect();
    assert!(
        disallowed.is_empty(),
        "level 2 permits signing; refused {disallowed:?}"
    );
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.what == Touched::SignatureValue),
        "and the new signature dictionary is among the changes: {:?}",
        report.changes
    );
}

/// **The blind spot, measured rather than claimed.**
///
/// `confine` narrows a page whose only changed key is `/Annots` to plumbing,
/// because otherwise every legitimate countersignature would report itself as
/// a page edit. The cost is that it cannot see *inside* `/Annots`: an
/// annotation removed and an annotation added look identical to it.
///
/// So under level 2 — which does not permit annotation changes — emptying the
/// page's `/Annots` is reported as permitted. That is wrong, it is a
/// consequence of object granularity rather than an oversight, and this test
/// exists so the day it stops being true is a failing test rather than a
/// silent improvement nobody noticed. It also fails if `confine` is ever
/// widened, which is the direction that would matter.
#[test]
fn emptying_the_annotation_list_is_not_caught_and_this_is_the_limit() {
    let certified = certify(form_document(), Certification::FormFillAndSigning, None);

    let mut editor = Document::open(certified).expect("reopens").editor();
    let page = *editor.page_refs().first().expect("a page");
    let annots = editor.intern(b"Annots");
    let Some(tinker_pdf::Object::Dict(mut dict)) = editor.get(page) else {
        panic!("the page is a dictionary");
    };
    dict.insert(annots, tinker_pdf::Object::Array(Vec::new()));
    editor.put(page, tinker_pdf::Object::Dict(dict));
    let stripped = editor.save(&incremental());

    let document = Document::open(stripped).expect("opens");
    let report = only_signature(&document).modifications(&document);
    let page_change = report
        .changes
        .iter()
        .find(|change| change.object.num == page.num)
        .expect("the page is reported as changed");

    assert_eq!(
        page_change.what,
        Touched::FormPlumbing,
        "the change is confined to /Annots, so it reads as plumbing"
    );
    assert!(
        page_change.permitted,
        "and level 2 permits plumbing — which is the limit this test records"
    );
}

/// An uncertified signature restricts nothing, so the same page edit that
/// level 1 refuses is reported and permitted.
#[test]
fn an_uncertified_signature_reports_changes_without_refusing_them() {
    let signer = Stub;
    let mut request = SigningRequest::new(
        SigningTarget::NewInvisibleField {
            name: "Approval".to_string(),
        },
        &signer,
    );
    request.reserve = 1024;
    let signed = Document::open(form_document())
        .expect("opens")
        .editor()
        .save_signed(&incremental(), &request)
        .expect("signing");

    let mut editor = Document::open(signed).expect("reopens").editor();
    assert!(editor.rotate_page(0, 90));
    let rotated = editor.save(&incremental());

    let document = Document::open(rotated).expect("opens");
    let report = only_signature(&document).modifications(&document);
    assert_eq!(report.certification, None);
    assert!(!report.is_unmodified(), "the change is still reported");
    assert_eq!(
        report.disallowed().count(),
        0,
        "a signature that certifies nothing forbids nothing"
    );
}
