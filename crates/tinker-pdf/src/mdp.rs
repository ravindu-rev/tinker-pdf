//! What changed after a signature, and whether it was allowed to (12.8.2).
//!
//! A signature over an earlier revision still proves what it proved. The
//! question this module answers is the next one: *given* that later revisions
//! exist, which objects did they touch, and does the signature's own
//! certification permit that?
//!
//! # Why cross-reference entries rather than objects
//!
//! The obvious method is to reparse the signed prefix as its own document and
//! compare object values. It has two faults and they are both fatal. An
//! encrypted document's strings and streams come back *decrypted* from the
//! authenticated original and *encrypted* from a freshly opened prefix, so
//! every object would compare unequal — the analysis would report a document
//! wholly rewritten whenever it was merely encrypted. And comparing values
//! means parsing every object of both documents, twice, to answer a question
//! about which bytes were written.
//!
//! Comparing **cross-reference entries** answers the real question directly.
//! 7.5.6 says an incremental update writes a new entry for exactly the objects
//! it changed, so an entry that differs between the prefix's table and the
//! whole file's table *is* an object a later revision wrote. It needs no
//! decryption, no key, and one parse of each object that actually changed.
//!
//! # What this is not
//!
//! It is not a verdict. `/DocMDP` is a claim the signer made about what
//! *should* be permitted; a reader learns from it what the signer intended,
//! not whether a change was legitimate. So the result is a list with each
//! entry classified and marked, and there is no boolean that discards the
//! list. Where the classification is uncertain the entry says
//! [`Touched::Other`] and is not permitted at any level, which errs toward
//! reporting a change rather than toward excusing one.

use std::collections::BTreeMap;

use tinker_pdf_cos::sign::{Certification, FieldLock};
use tinker_pdf_cos::{CosDocument, Dict, FieldKind, Name, ObjRef, Object};

use crate::signature::{Coverage, Signature};
use crate::Document;

/// How an object differs between the signed bytes and the whole file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// The object did not exist when the signature was made.
    Added,
    /// It existed and a later revision wrote it again.
    Altered,
    /// It existed and a later revision freed it.
    Removed,
}

/// What the changed object turned out to be.
///
/// Classified from the object as the *whole file* has it, because that is the
/// state a reader is being asked about. An object a later revision freed is
/// classified from the signed prefix instead, since the whole file no longer
/// has one to look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Touched {
    /// A form field, named as 12.7.3.2 qualifies it. Filling one is what
    /// certification level 2 exists to permit.
    FormField {
        /// The fully qualified field name, when it could be resolved.
        name: Option<String>,
        /// What kind of control it is.
        kind: FieldKind,
    },
    /// A signature dictionary — the object a later signing operation adds.
    SignatureValue,
    /// An annotation (12.5), which level 3 permits and level 2 does not.
    Annotation,
    /// The interactive form dictionary, or a page's `/Annots` list: the
    /// objects a permitted fill or signing operation *necessarily* rewrites.
    /// Grouped together because they are permitted for the same reason and
    /// neither carries content of its own.
    FormPlumbing,
    /// A page dictionary or its content.
    Page,
    /// The document catalog.
    Catalog,
    /// Anything else, including an object this build could not classify.
    /// Not permitted at any level, deliberately: an unclassified change is a
    /// change, and treating it as harmless is the failure that matters.
    Other,
}

/// One object a later revision touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modification {
    /// Which object.
    pub object: ObjRef,
    /// How it differs.
    pub change: Change,
    /// What it is.
    pub what: Touched,
    /// Whether the signature's own `/DocMDP` and `/FieldMDP` permit it.
    ///
    /// `true` when the signature certifies nothing, because a signature that
    /// certifies nothing forbids nothing — it simply does not cover the
    /// change. [`Modifications::certification`] is what distinguishes the two.
    pub permitted: bool,
}

/// Everything later revisions did to a signed document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modifications {
    /// The certification in force, when this signature certifies the document.
    pub certification: Option<Certification>,
    /// The fields this signature locks, when it locks any.
    pub field_lock: Option<FieldLock>,
    /// Every object a revision after the signed bytes wrote, freed or added.
    pub changes: Vec<Modification>,
}

impl Modifications {
    /// Nothing came after the signature.
    #[must_use]
    pub fn is_unmodified(&self) -> bool {
        self.changes.is_empty()
    }

    /// The changes the certification does not permit.
    pub fn disallowed(&self) -> impl Iterator<Item = &Modification> {
        self.changes.iter().filter(|change| !change.permitted)
    }
}

/// What later revisions changed, for one signature.
pub(crate) fn modifications(document: &Document, signature: &Signature) -> Modifications {
    let mut report = Modifications {
        certification: signature.certification,
        field_lock: signature.field_lock.clone(),
        changes: Vec::new(),
    };

    // A signature over the whole file has nothing after it, and one whose
    // coverage did not hold up has no trustworthy prefix to compare against —
    // reporting "unmodified" for the latter would be the worst possible
    // reading, so it reports nothing and `coverage` is what says why.
    let covered = match &signature.coverage {
        Coverage::WholeFile | Coverage::Suspicious(_) => return report,
        Coverage::Revision { .. } => match signature.spans.last() {
            Some(last) => last.end,
            None => return report,
        },
    };

    let cos = document.cos();
    let bytes = cos.bytes();
    let Ok(prefix) = usize::try_from(covered) else {
        return report;
    };
    let Some(head) = bytes.get(..prefix) else {
        return report;
    };
    // The prefix is a whole document in its own right — that is what an
    // incremental update means — so it opens the same way any file does.
    let Ok(before) = CosDocument::open(head.to_vec()) else {
        return report;
    };

    let names = Classifier::new(cos, &before);
    let mut touched: Vec<(u32, Change)> = Vec::new();
    for (num, entry) in cos.xref().iter() {
        if num == 0 {
            continue;
        }
        match before.xref().get(num) {
            Some(previous) if previous == entry => {}
            Some(_) => touched.push((num, Change::Altered)),
            None => touched.push((num, Change::Added)),
        }
    }
    for (num, _) in before.xref().iter() {
        if num != 0 && cos.xref().get(num).is_none() {
            touched.push((num, Change::Removed));
        }
    }
    touched.sort_unstable_by_key(|(num, _)| *num);

    for (num, change) in touched {
        let object = ObjRef::new(num, 0);
        let what = match change {
            Change::Removed => names.classify(&before, object),
            Change::Added => names.classify(cos, object),
            Change::Altered => confine(cos, &before, object, names.classify(cos, object)),
        };
        let permitted = permits(signature, &what);
        report.changes.push(Modification {
            object,
            change,
            what,
            permitted,
        });
    }
    report
}

/// Narrows a container's classification when its change is confined to the
/// keys a permitted operation is *obliged* to touch.
///
/// # Why this is needed, and what it costs
///
/// An object is the finest grain a cross-reference table has, and two of the
/// containers a document must have are rewritten by operations 12.8.2.2
/// explicitly permits. Adding a signature appends its widget to a page's
/// `/Annots`, and if that array is written directly in the page dictionary —
/// which is the common shape — the *page object* changes. Filling a field
/// rewrites `/AcroForm`, and if that is direct in the catalog, the *catalog*
/// changes. Classifying those as `Page` and `Catalog` would make every
/// legitimate fill and every legitimate countersignature report itself as a
/// violation, which is a check nobody can use.
///
/// So for an altered container this compares the two dictionaries key by key
/// and asks whether *everything else* is unchanged. If it is, the change is
/// confined to the plumbing and is classified as such; if anything else moved,
/// the strict classification stands.
///
/// **What it costs is stated rather than hidden.** This cannot see inside the
/// allowed key: a `/Annots` array that lost an annotation and a `/Annots` array
/// that gained one look the same here, and a `/AcroForm` whose `/Fields` were
/// emptied reads as plumbing. Object granularity is what a cross-reference
/// table offers, and a finer analysis would have to diff the values — which is
/// the thing that cannot be done on an encrypted document.
///
/// Keys are compared by their **bytes**, because a [`Name`] is only meaningful
/// against the table that issued it and these are two documents.
///
/// One property falls out of the comparison and is worth keeping: in an
/// encrypted document the prefix is unauthenticated, so its strings and
/// streams are ciphertext where the whole file's are plaintext. Unchanged
/// entries then compare *unequal*, the confinement fails, and the strict
/// classification stands. Encryption makes this stricter and never looser.
fn confine(after: &CosDocument, before: &CosDocument, object: ObjRef, kind: Touched) -> Touched {
    let allowed: &[&[u8]] = match kind {
        Touched::Page => &[b"Annots"],
        Touched::Catalog => &[b"AcroForm", b"Perms"],
        _ => return kind,
    };

    let (Ok(new), Ok(old)) = (after.get(object), before.get(object)) else {
        return kind;
    };
    let (Some(new), Some(old)) = (new.as_dict(), old.as_dict()) else {
        return kind;
    };

    let new = entries(after, new);
    let old = entries(before, old);

    let mut keys: Vec<&Vec<u8>> = new.keys().chain(old.keys()).collect();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        if allowed.contains(&key.as_slice()) {
            continue;
        }
        match (new.get(key), old.get(key)) {
            (Some(new), Some(old)) if same(after, before, new, old) => {}
            _ => return kind,
        }
    }
    Touched::FormPlumbing
}

fn entries(document: &CosDocument, dict: &Dict) -> BTreeMap<Vec<u8>, Object> {
    let mut map = BTreeMap::new();
    for (key, value) in dict.iter() {
        if let Some(bytes) = document.name_bytes(*key) {
            map.insert(bytes.to_vec(), value.clone());
        }
    }
    map
}

/// Whether two objects from **two different documents** mean the same thing.
///
/// `Object::Name` holds an interned symbol that is only meaningful against the
/// table that issued it, so `/Type /Page` in one document and `/Type /Page` in
/// another are unequal by `PartialEq` whenever the two tables happened to
/// intern in different orders. That is a trap rather than a bug in `PartialEq`
/// — the ids genuinely are not comparable — and it is why this exists.
///
/// A stream is answered `false` without looking: comparing one means comparing
/// its bytes, and in an encrypted document the prefix's are ciphertext where
/// the whole file's are plaintext. Refusing to compare keeps
/// [`confine`] strict in exactly the case where being wrong would be worst.
fn same(after: &CosDocument, before: &CosDocument, new: &Object, old: &Object) -> bool {
    match (new, old) {
        (Object::Name(new), Object::Name(old)) => {
            match (after.name_bytes(*new), before.name_bytes(*old)) {
                (Some(new), Some(old)) => new == old,
                _ => false,
            }
        }
        (Object::Array(new), Object::Array(old)) => {
            new.len() == old.len()
                && new
                    .iter()
                    .zip(old)
                    .all(|(new, old)| same(after, before, new, old))
        }
        (Object::Dict(new), Object::Dict(old)) => {
            let new = entries(after, new);
            let old = entries(before, old);
            new.len() == old.len()
                && new.iter().all(|(key, value)| {
                    old.get(key)
                        .is_some_and(|old| same(after, before, value, old))
                })
        }
        (Object::Stream(_), _) | (_, Object::Stream(_)) => false,
        (new, old) => new == old,
    }
}

/// Whether the signature's own claims permit a change of this kind.
///
/// 12.8.2.2's three levels, plus 12.8.2.4's field lock, which overrides them:
/// a locked field's value may not change however permissive `/P` is.
fn permits(signature: &Signature, what: &Touched) -> bool {
    if let (Some(lock), Touched::FormField { name, .. }) = (&signature.field_lock, what) {
        // A field with no resolvable name is treated as locked when anything
        // is locked, because the alternative is excusing a change to a field
        // that could not be identified.
        let locked = name.as_deref().is_none_or(|name| lock.locks(name));
        if locked {
            return false;
        }
    }

    let Some(certification) = signature.certification else {
        // Not a certifying signature: it restricts nothing, so nothing it
        // failed to restrict is a violation of it.
        return true;
    };
    match certification {
        Certification::NoChanges => false,
        Certification::FormFillAndSigning => matches!(
            what,
            Touched::FormField { .. } | Touched::SignatureValue | Touched::FormPlumbing
        ),
        Certification::FormFillSigningAndAnnotations => matches!(
            what,
            Touched::FormField { .. }
                | Touched::SignatureValue
                | Touched::FormPlumbing
                | Touched::Annotation
        ),
    }
}

/// The object numbers of every appearance stream a widget names (12.5.5).
///
/// `/AP` holds `/N`, `/R` and `/D`, each of which is either the stream itself
/// or — for a widget with states, which is how a checkbox is drawn — a
/// dictionary of them. One level of nesting is all 12.5.5 defines, so one
/// level is all this walks.
fn appearances(document: &CosDocument, widget: ObjRef) -> Vec<u32> {
    let Ok(object) = document.get(widget) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };
    let streams = document.resolve_key(dict, document.intern(b"AP"));
    let Some(streams) = streams.as_dict() else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for key in [b"N".as_slice(), b"R".as_slice(), b"D".as_slice()] {
        let key = document.intern(key);
        match streams.get(key) {
            Some(object) => {
                if let Some(reference) = object.as_objref() {
                    found.push(reference.num);
                }
                // A state dictionary, whose values are the per-state streams.
                let resolved = document.resolve_key(streams, key);
                if let Some(states) = resolved.as_dict() {
                    for (_, state) in states.iter() {
                        if let Some(reference) = state.as_objref() {
                            found.push(reference.num);
                        }
                    }
                }
            }
            None => continue,
        }
    }
    found
}

/// Turns an object number into what that object is.
///
/// The field map is built once from each document's own field walk rather than
/// guessed at from `/FT`, because a field's kind can be inherited from a
/// parent and its name is assembled from the whole chain — neither is visible
/// in the object alone.
struct Classifier {
    fields: BTreeMap<u32, (Option<String>, FieldKind)>,
    plumbing: BTreeMap<u32, ()>,
}

impl Classifier {
    fn new(after: &CosDocument, before: &CosDocument) -> Classifier {
        let mut fields = BTreeMap::new();
        let mut plumbing = BTreeMap::new();
        for document in [after, before] {
            for field in tinker_pdf_cos::fields(document) {
                let entry = (Some(field.name.clone()), field.kind);
                fields.insert(field.reference.num, entry.clone());
                for widget in &field.widgets {
                    fields.entry(widget.num).or_insert_with(|| entry.clone());
                    // A widget's appearance streams belong to the field, and
                    // filling a field rewrites them (12.7.4.3). Left
                    // unclassified they would be `Other`, and an ordinary
                    // level-2 form fill would report itself as a violation.
                    for appearance in appearances(document, *widget) {
                        fields.entry(appearance).or_insert_with(|| entry.clone());
                    }
                }
            }
            // The interactive form dictionary and every page's `/Annots` are
            // rewritten by any operation that adds a field or an annotation,
            // so they are named here rather than falling to `Other` and making
            // every legitimate fill look like a violation.
            let Some(catalog) = document.catalog() else {
                continue;
            };
            let acroform = document.intern(b"AcroForm");
            if let Some(form) = catalog.get_ref(acroform) {
                plumbing.insert(form.num, ());
                if let Ok(dict) = document.get(form) {
                    if let Some(dict) = dict.as_dict() {
                        if let Some(list) = dict.get_ref(document.intern(b"Fields")) {
                            plumbing.insert(list.num, ());
                        }
                    }
                }
            }
            let annots = document.intern(b"Annots");
            for page in tinker_pdf_cos::pages::collect(document) {
                if let Ok(dict) = document.get(page.reference) {
                    if let Some(list) = dict.as_dict().and_then(|dict| dict.get_ref(annots)) {
                        plumbing.insert(list.num, ());
                    }
                }
            }
        }
        Classifier { fields, plumbing }
    }

    fn classify(&self, document: &CosDocument, object: ObjRef) -> Touched {
        if let Some((name, kind)) = self.fields.get(&object.num) {
            return Touched::FormField {
                name: name.clone(),
                kind: *kind,
            };
        }
        if self.plumbing.contains_key(&object.num) {
            return Touched::FormPlumbing;
        }
        let Ok(value) = document.get(object) else {
            return Touched::Other;
        };
        let Some(dict) = value.as_dict() else {
            return Touched::Other;
        };
        // A signature dictionary is recognised by what it carries rather than
        // by `/Type`, which producers omit: `/ByteRange` says what it is.
        if dict.get(document.intern(b"ByteRange")).is_some() {
            return Touched::SignatureValue;
        }
        let kind = dict
            .get_name(Name::TYPE)
            .and_then(|name| document.name_bytes(name));
        match kind.as_deref() {
            Some(b"Annot") => Touched::Annotation,
            Some(b"Page") => Touched::Page,
            Some(b"Catalog") => Touched::Catalog,
            Some(b"Sig") => Touched::SignatureValue,
            _ => Touched::Other,
        }
    }
}

/// Convenience for a document with one certifying signature: the strictest
/// certification any of its signatures declares.
///
/// 12.8.2.2 allows only one certifying signature and requires it to be the
/// first, but nothing in a file enforces that, so this reports the strictest
/// rather than the first — a document claiming two certifications is claiming
/// both, and the stricter is the one a reader must not ignore.
#[must_use]
pub(crate) fn strictest(document: &Document) -> Option<Certification> {
    document
        .signatures()
        .iter()
        .filter_map(|signature| signature.certification)
        .min_by_key(|certification| certification.level())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_one_certification_permits_nothing() {
        for what in [
            Touched::FormField {
                name: Some("A".into()),
                kind: FieldKind::Text,
            },
            Touched::SignatureValue,
            Touched::Annotation,
            Touched::FormPlumbing,
            Touched::Page,
            Touched::Catalog,
            Touched::Other,
        ] {
            assert!(
                !permits(&certifying(Some(Certification::NoChanges), None), &what),
                "{what:?}"
            );
        }
    }

    #[test]
    fn level_two_permits_filling_and_signing_and_nothing_else() {
        let signature = certifying(Some(Certification::FormFillAndSigning), None);
        assert!(permits(&signature, &Touched::SignatureValue));
        assert!(permits(&signature, &Touched::FormPlumbing));
        assert!(permits(
            &signature,
            &Touched::FormField {
                name: Some("A".into()),
                kind: FieldKind::Text
            }
        ));
        assert!(!permits(&signature, &Touched::Annotation));
        assert!(!permits(&signature, &Touched::Page));
        assert!(!permits(&signature, &Touched::Other));
    }

    #[test]
    fn level_three_adds_annotations_and_still_refuses_a_page_edit() {
        let signature = certifying(Some(Certification::FormFillSigningAndAnnotations), None);
        assert!(permits(&signature, &Touched::Annotation));
        assert!(!permits(&signature, &Touched::Page));
    }

    #[test]
    fn an_uncertified_signature_forbids_nothing() {
        let signature = certifying(None, None);
        assert!(permits(&signature, &Touched::Page));
        assert!(permits(&signature, &Touched::Other));
    }

    /// 12.8.2.4 overrides 12.8.2.2: a locked field may not change however
    /// permissive the certification is, and an unnamed field counts as locked.
    #[test]
    fn a_field_lock_overrides_the_certification_level() {
        let signature = certifying(
            Some(Certification::FormFillSigningAndAnnotations),
            Some(FieldLock::Include(vec!["Locked".into()])),
        );
        assert!(!permits(
            &signature,
            &Touched::FormField {
                name: Some("Locked".into()),
                kind: FieldKind::Text
            }
        ));
        assert!(permits(
            &signature,
            &Touched::FormField {
                name: Some("Open".into()),
                kind: FieldKind::Text
            }
        ));
        assert!(
            !permits(
                &signature,
                &Touched::FormField {
                    name: None,
                    kind: FieldKind::Text
                }
            ),
            "a field that could not be named is not excused"
        );
    }

    #[test]
    fn a_lock_on_an_uncertified_signature_still_locks() {
        let signature = certifying(None, Some(FieldLock::All));
        assert!(!permits(
            &signature,
            &Touched::FormField {
                name: Some("Anything".into()),
                kind: FieldKind::Text
            }
        ));
        assert!(
            permits(&signature, &Touched::Page),
            "and locks only the fields"
        );
    }

    /// A `Signature` with only the two fields `permits` reads, so the table
    /// above is tested without building a document.
    fn certifying(certification: Option<Certification>, lock: Option<FieldLock>) -> Signature {
        Signature {
            anchor: crate::signature::Anchor::Field,
            field: None,
            field_ref: None,
            value_ref: None,
            filter: None,
            sub_filter: None,
            sub_filter_name: None,
            spans: Vec::new(),
            coverage: Coverage::WholeFile,
            contents: Vec::new(),
            contents_at: None,
            signed_at: None,
            reason: None,
            location: None,
            name: None,
            contact: None,
            certification,
            field_lock: lock,
            warnings: Vec::new(),
        }
    }
}
