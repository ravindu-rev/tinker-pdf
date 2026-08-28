//! Digital signatures, read (12.8).
//!
//! A signature in a PDF is a dictionary naming two things: a `/ByteRange`
//! saying which of the file's bytes it covers, and a `/Contents` holding a CMS
//! blob over the digest of those bytes. This module is the first half — it
//! finds the signatures, works out what each one actually covers, and hands
//! back the stored CMS bytes and a digest over the covered spans. Nothing here
//! verifies anything; that arrives with the certificate and CMS parsing in
//! `tinker-pdf-pki`.
//!
//! # Why the coverage is classified rather than reported
//!
//! `/ByteRange` is four numbers a producer wrote, and a reader that trusts
//! them has already lost. The interesting attack is not a forged signature but
//! an honest signature over a range that omits the part of the file that
//! matters — a range that stops short of the end, a range whose gap is not
//! where `/Contents` actually sits. So every span is checked against the file
//! it claims to describe and the answer is a [`Coverage`], not a pair of
//! offsets: `WholeFile` for the one honest shape, `Revision` for a signature
//! over an earlier revision that later incremental updates left intact
//! (7.5.6), and `Suspicious` with the reason named for everything else.
//!
//! # Why `/Contents` is read from the file rather than from the object model
//!
//! Two reasons, and both matter. The gap between the two `/ByteRange` spans is
//! *defined* to be the `/Contents` string, so reading it from the gap is the
//! only reading that can disagree with a lying `/ByteRange` — and a
//! disagreement is exactly what this module exists to notice. And in an
//! encrypted document the object model hands back a *decrypted* string, while
//! a signature covers the bytes as stored; digesting one and parsing the other
//! would be two different documents.
//!
//! # Why two roots rather than one
//!
//! 12.7.4.5 says a signature is the `/V` of a signature field, and a reader
//! that believes only that finds 13 of the 18 signatures in the fetched
//! corpora. The other five are reachable only through the catalog's `/Perms`
//! (12.8.4): three files carry a `/UR3` usage-rights signature and no
//! signature field at all, and one has a `/FT /Sig` annotation in a document
//! with no `/AcroForm`, so no field walk can reach it.
//!
//! Both roots are walked, results deduplicated by object, and every entry says
//! which root found it — because a usage-rights signature grants a reader
//! capabilities and makes no claim about the document's content, and a reader
//! that cannot tell the two apart will report that it does.
//!
//! [`Anchor::MergedField`] is the third shape and **no corpus file needs it**.
//! It is here because a field dictionary carrying `/ByteRange` with no `/V` is
//! unambiguous about what it is, and dropping it would drop a signature
//! silently; it is exercised by a fixture rather than by a document, and this
//! sentence is the record of that.
//!
//! # A trap worth naming
//!
//! An encrypted document's object streams do not decompress until the file key
//! exists, so a signature living in one is invisible until the caller
//! authenticates — `form_fields()` returns nothing and this module honestly
//! reports no signatures. Two corpus files behave that way, and the first
//! reading of one of them made it look as though its producer merged the
//! signature dictionary into the field. It does not; it was simply unread.

use std::collections::BTreeSet;
use std::ops::Range;

use tinker_pdf_cos::sign::{digest_spans, Certification, DigestAlgorithm, FieldLock};
use tinker_pdf_cos::{CosDocument, Date, Dict, FieldKind, Name, ObjRef, Object};

use crate::Document;

/// How a signature dictionary was reached, which is part of what it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The `/V` of a `/FT /Sig` form field (12.7.4.5) — an author or approval
    /// signature over the document.
    Field,
    /// The field dictionary *is* the signature dictionary: `/ByteRange` and
    /// `/FT /Sig` are siblings and there is no `/V`.
    ///
    /// Not what 12.7.4.5 describes, and no file in the fetched corpora emits
    /// it — this is read because the shape is unambiguous and refusing it
    /// would drop a signature without saying so, not because anything was
    /// measured needing it.
    MergedField,
    /// A catalog `/Perms` entry (12.8.4). `/DocMDP` certifies the document and
    /// restricts what later changes are allowed; `/UR3` and `/UR` grant a
    /// reader extra usage rights and say **nothing** about the content.
    Permissions(String),
}

/// What a signature's `/ByteRange` covers, after checking it against the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Every byte of the file except the one gap holding `/Contents`.
    ///
    /// This is the only shape that means "this signature covers the document
    /// you are looking at".
    WholeFile,
    /// Every byte up to the end of revision `index` of
    /// [`CosDocument::revisions`], with later revisions outside it.
    ///
    /// Legitimate and common: a document signed, then filled in, then signed
    /// again. The earlier signature still proves what it proved; it just does
    /// not prove anything about the bytes that came after. `index` counts the
    /// same way `revisions()` does, newest first.
    Revision {
        /// Index into [`CosDocument::revisions`].
        index: usize,
    },
    /// Anything else, with the reason named.
    Suspicious(CoverageDefect),
}

/// Why a `/ByteRange` could not be read as honest coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageDefect {
    /// No `/ByteRange` at all.
    Missing,
    /// `/ByteRange` is not four numbers. 12.8.1 fixes the count at four — a
    /// longer array would describe a second gap, which is the shape of the
    /// attack this rejects rather than tries to accommodate.
    NotFourNumbers {
        /// How many entries were present.
        count: usize,
    },
    /// A start or a length was not a non-negative integer that fits a file
    /// offset.
    NotAnOffset,
    /// The first span does not start at byte zero, so the file's header is
    /// outside the signature.
    DoesNotStartAtZero {
        /// Where the first span does start.
        first: u64,
    },
    /// The second span starts before the first one ends.
    SpansOverlap,
    /// A span runs past the end of the file. Seen in the corpus on a document
    /// edited after signing, where the `/ByteRange` still describes the longer
    /// file it was written against.
    PastEndOfFile {
        /// Where the coverage claims to end.
        end: u64,
        /// How long the file actually is.
        file: u64,
    },
    /// The gap between the spans is not a hexadecimal string, so it is not
    /// where `/Contents` sits — whatever the dictionary says. Seen in the
    /// corpus with the gap landing in an XMP packet and in XFA markup.
    GapIsNotContents,
    /// Coverage stops before the end of the file at a place that is not a
    /// revision boundary, leaving unsigned bytes that no update explains.
    EndsMidFile {
        /// The last byte offset the signature covers.
        covered_to: u64,
    },
}

/// Something read leniently, or not read at all, while inventorying one
/// signature (ruling 10: every leniency names what it touched).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureWarning {
    /// The field's `/V` is present but is not a dictionary.
    ValueNotADictionary,
    /// `/Contents` is absent from the dictionary. The gap is still decoded if
    /// `/ByteRange` describes one, because the gap is the authority.
    ContentsMissing,
    /// The gap held characters that are neither hexadecimal digits nor
    /// whitespace.
    ContentsNotHexadecimal,
    /// The gap held an odd number of hexadecimal digits, so the last one names
    /// half a byte. 7.3.4.3 pads a trailing odd digit with zero and this does
    /// the same, rather than dropping a byte from a signature.
    ContentsOddDigitCount,
    /// The gap holds the hexadecimal digits but not the `<` and `>` around
    /// them, so the signature covers its own delimiters.
    ///
    /// Accepted, and named, because the two conventions are both in the
    /// corpus and the difference is two bytes — but it is two bytes inside or
    /// outside the digest, so a reader that silently normalised it would be
    /// computing a different number from the one the signer computed.
    ContentsGapExcludesDelimiters,
    /// `/SubFilter` names a scheme this build does not know how to verify. It
    /// is still inventoried, because knowing a signature is there matters even
    /// when its scheme does not verify.
    SubFilterUnknown(String),
    /// `/SubFilter` is absent, so nothing says how `/Contents` is encoded.
    SubFilterMissing,
}

/// The `/SubFilter` schemes 12.8.3 defines, and what this build makes of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubFilter {
    /// `adbe.pkcs7.detached` (12.8.3.3.2) — `/Contents` is a detached CMS
    /// `SignedData` whose message digest is over the covered bytes. Seventeen
    /// of the eighteen signed documents in the fetched corpora use this.
    Pkcs7Detached,
    /// `adbe.pkcs7.sha1` (12.8.3.3.1) — the legacy shape, where the CMS
    /// encapsulates the SHA-1 digest of the covered bytes rather than being
    /// detached from it. Deprecated in ISO 32000-2.
    Pkcs7Sha1,
    /// `adbe.x509.rsa_sha1` (12.8.3.2) — a bare PKCS#1 signature, with the
    /// certificate chain in `/Cert` rather than in `/Contents`.
    X509RsaSha1,
    /// `ETSI.CAdES.detached` — the PAdES profile, a CMS `SignedData` with
    /// additional signed attributes.
    EtsiCadesDetached,
    /// `ETSI.RFC3161` — a document timestamp rather than an author signature.
    EtsiRfc3161,
}

impl SubFilter {
    /// The `/SubFilter` name this stands for.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SubFilter::Pkcs7Detached => "adbe.pkcs7.detached",
            SubFilter::Pkcs7Sha1 => "adbe.pkcs7.sha1",
            SubFilter::X509RsaSha1 => "adbe.x509.rsa_sha1",
            SubFilter::EtsiCadesDetached => "ETSI.CAdES.detached",
            SubFilter::EtsiRfc3161 => "ETSI.RFC3161",
        }
    }

    fn from_bytes(bytes: &[u8]) -> Option<SubFilter> {
        match bytes {
            b"adbe.pkcs7.detached" => Some(SubFilter::Pkcs7Detached),
            b"adbe.pkcs7.sha1" => Some(SubFilter::Pkcs7Sha1),
            b"adbe.x509.rsa_sha1" => Some(SubFilter::X509RsaSha1),
            b"ETSI.CAdES.detached" => Some(SubFilter::EtsiCadesDetached),
            b"ETSI.RFC3161" => Some(SubFilter::EtsiRfc3161),
            _ => None,
        }
    }
}

/// One signature found in a document.
///
/// Everything here is what the file says plus what checking it against the
/// file established. No field is a verdict.
#[derive(Clone, Debug)]
pub struct Signature {
    /// How the dictionary was reached, which is part of what it claims.
    pub anchor: Anchor,
    /// The fully qualified name of the field holding it (12.7.3.2), when it
    /// was reached through one.
    pub field: Option<String>,
    /// The field object, when there was one.
    pub field_ref: Option<ObjRef>,
    /// The signature dictionary's own object, when it is an indirect one.
    pub value_ref: Option<ObjRef>,
    /// `/Filter` — the handler that produced it, `/Adobe.PPKLite` in practice.
    pub filter: Option<String>,
    /// `/SubFilter`, recognised.
    pub sub_filter: Option<SubFilter>,
    /// `/SubFilter` exactly as written, recognised or not.
    pub sub_filter_name: Option<String>,
    /// The spans `/ByteRange` names, as offsets into the file. Empty when
    /// `/ByteRange` was missing or unreadable.
    pub spans: Vec<Range<u64>>,
    /// What those spans amount to, checked against the file.
    pub coverage: Coverage,
    /// The stored `/Contents` bytes, read from the gap between the spans.
    ///
    /// For `adbe.pkcs7.*` and the ETSI profiles this is DER. Empty when there
    /// was no readable gap.
    pub contents: Vec<u8>,
    /// The byte range of the gap the spans leave.
    pub contents_at: Option<Range<u64>>,
    /// `/M`, the time the signer claims. Unverified by construction: it is a
    /// string the signer wrote, not a timestamp anything countersigned.
    pub signed_at: Option<Date>,
    /// `/Reason`.
    pub reason: Option<String>,
    /// `/Location`.
    pub location: Option<String>,
    /// `/Name`, the signer's own claim about who they are.
    pub name: Option<String>,
    /// `/ContactInfo`.
    pub contact: Option<String>,
    /// `/Reference` with `/TransformMethod /DocMDP` (12.8.2.2): this signature
    /// certifies the document and says what later revisions may change.
    ///
    /// `None` for an ordinary approval signature, which restricts nothing —
    /// and also for a `/DocMDP` whose `/P` is a value 12.8.2.2 does not
    /// define, which is a document saying something meaningless rather than
    /// something strict.
    pub certification: Option<Certification>,
    /// `/Reference` with `/TransformMethod /FieldMDP` (12.8.2.4): the form
    /// fields this signature locks against later change.
    pub field_lock: Option<FieldLock>,
    /// What was read leniently, or not at all.
    pub warnings: Vec<SignatureWarning>,
}

impl Signature {
    /// The digest of the covered bytes, which is what a CMS `messageDigest`
    /// has to match.
    ///
    /// Returns `None` when the spans do not fit the file — the same condition
    /// that makes [`Coverage::Suspicious`] carry
    /// [`CoverageDefect::PastEndOfFile`] — because digesting a truncated
    /// approximation of what a signature covers produces a number that looks
    /// like an answer and is not one.
    ///
    /// The algorithm is a parameter rather than a field because nothing knows
    /// it until the CMS is parsed, and computing all four eagerly would read
    /// the file four times for the three that get thrown away.
    #[must_use]
    pub fn digest(&self, document: &Document, algorithm: DigestAlgorithm) -> Option<Vec<u8>> {
        digest_spans(document.cos().bytes(), &self.spans, algorithm)
    }

    /// Whether the signature covers every byte of the file it was read from.
    #[must_use]
    pub fn covers_whole_file(&self) -> bool {
        self.coverage == Coverage::WholeFile
    }

    /// [`Signature::contents`] with the reservation's trailing fill removed,
    /// which is what a CMS parser must be handed.
    ///
    /// A signer reserves space before the file is laid out and cannot shrink
    /// it afterwards, so `/Contents` is the blob followed by zero fill
    /// (12.8.1). A DER parser reading the whole reservation refuses it as
    /// trailing bytes — correctly, which is why the trimming happens here
    /// rather than by loosening the parser.
    ///
    /// **Only zeros are trimmed.** If the bytes after the outer structure's
    /// declared length are anything else they are not fill, and the whole
    /// reservation is returned so the parser refuses it. Trimming whatever
    /// follows would be a second reading of a signed structure, which is the
    /// shape of a signature bypass.
    ///
    /// Falls back to the whole of `contents` when no outer structure can be
    /// read, because then there is no declared length to trust.
    #[must_use]
    pub fn cms(&self) -> &[u8] {
        let budget = tinker_pdf_pki::Budget::new(tinker_pdf_pki::Limits::CMS);
        let mut cursor = tinker_pdf_pki::Cursor::new(&self.contents, &budget);
        let Ok(outer) = cursor.read() else {
            return &self.contents;
        };
        let declared = outer.raw().len();
        match self.contents.get(declared..) {
            Some(rest) if rest.iter().all(|byte| *byte == 0) => &self.contents[..declared],
            _ => &self.contents,
        }
    }

    /// What revisions after this signature changed, and whether its own
    /// `/DocMDP` and `/FieldMDP` permit it (12.8.2).
    ///
    /// Empty for a signature covering the whole file, because nothing came
    /// after it, and empty for one whose coverage did not hold up, because
    /// there is then no trustworthy prefix to compare against — the coverage
    /// is what says which.
    #[must_use]
    pub fn modifications(&self, document: &Document) -> crate::mdp::Modifications {
        crate::mdp::modifications(document, self)
    }

    /// Whether this is a usage-rights signature (12.8.4), which grants a
    /// reader capabilities and makes no claim about the document's content.
    #[must_use]
    pub fn is_usage_rights(&self) -> bool {
        matches!(&self.anchor, Anchor::Permissions(key) if key == "UR" || key == "UR3")
    }
}

/// One signature dictionary and how it was reached, before it is read.
struct Candidate {
    anchor: Anchor,
    field: Option<String>,
    field_ref: Option<ObjRef>,
    value_ref: Option<ObjRef>,
    dict: Dict,
    warnings: Vec<SignatureWarning>,
}

/// Every signature in the document, field signatures first, then `/Perms`.
///
/// A signature field with no `/V` and no `/ByteRange` of its own is not a
/// signature — it is a place for one — and is left to
/// [`Document::form_fields`], which already lists it.
pub(crate) fn signatures(document: &Document) -> Vec<Signature> {
    let cos = document.cos();
    let keys = Keys::new(cos);
    let mut seen: BTreeSet<(u32, u16)> = BTreeSet::new();
    let mut candidates = Vec::new();

    collect_fields(document, cos, &keys, &mut seen, &mut candidates);
    collect_permissions(cos, &keys, &mut seen, &mut candidates);

    candidates
        .into_iter()
        .map(|candidate| read(cos, &keys, candidate))
        .collect()
}

/// The dictionary keys this module interns once rather than per signature.
struct Keys {
    byte_range: Name,
    sub_filter: Name,
    modified: Name,
    reason: Name,
    location: Name,
    signer: Name,
    contact: Name,
    value: Name,
    perms: Name,
    reference: Name,
    transform_method: Name,
    transform_params: Name,
    permissions: Name,
    action: Name,
    fields: Name,
}

impl Keys {
    fn new(cos: &CosDocument) -> Keys {
        Keys {
            byte_range: cos.intern(b"ByteRange"),
            sub_filter: cos.intern(b"SubFilter"),
            modified: cos.intern(b"M"),
            reason: cos.intern(b"Reason"),
            location: cos.intern(b"Location"),
            signer: cos.intern(b"Name"),
            contact: cos.intern(b"ContactInfo"),
            value: cos.intern(b"V"),
            perms: cos.intern(b"Perms"),
            reference: cos.intern(b"Reference"),
            transform_method: cos.intern(b"TransformMethod"),
            transform_params: cos.intern(b"TransformParams"),
            permissions: cos.intern(b"P"),
            action: cos.intern(b"Action"),
            fields: cos.intern(b"Fields"),
        }
    }
}

fn remember(seen: &mut BTreeSet<(u32, u16)>, reference: Option<ObjRef>) -> bool {
    match reference {
        Some(r) => seen.insert((r.num, r.gen)),
        // A direct dictionary has no identity to deduplicate on, and two
        // direct signature dictionaries in one file are two signatures.
        None => true,
    }
}

fn collect_fields(
    document: &Document,
    cos: &CosDocument,
    keys: &Keys,
    seen: &mut BTreeSet<(u32, u16)>,
    into: &mut Vec<Candidate>,
) {
    for field in document.form_fields() {
        if field.kind != FieldKind::Signature {
            continue;
        }
        let Ok(object) = cos.get(field.reference) else {
            continue;
        };
        let Some(field_dict) = object.as_dict() else {
            continue;
        };

        let value_ref = field_dict.get(keys.value).and_then(Object::as_objref);
        let resolved = cos.resolve_key(field_dict, keys.value);
        if let Some(sig) = resolved.as_dict() {
            if !remember(seen, value_ref) {
                continue;
            }
            into.push(Candidate {
                anchor: Anchor::Field,
                field: Some(field.name.clone()),
                field_ref: Some(field.reference),
                value_ref,
                dict: sig.clone(),
                warnings: Vec::new(),
            });
            continue;
        }

        // 12.7.4.5 says the dictionary is the `/V`. One commercial signing
        // service writes `/ByteRange` and `/FT /Sig` as siblings instead, and
        // refusing to read that would drop a real signature on the floor.
        if field_dict.get(keys.byte_range).is_some() {
            if !remember(seen, Some(field.reference)) {
                continue;
            }
            into.push(Candidate {
                anchor: Anchor::MergedField,
                field: Some(field.name.clone()),
                field_ref: Some(field.reference),
                value_ref: Some(field.reference),
                dict: field_dict.clone(),
                warnings: Vec::new(),
            });
            continue;
        }

        if field_dict.get(keys.value).is_some() {
            into.push(Candidate {
                anchor: Anchor::Field,
                field: Some(field.name.clone()),
                field_ref: Some(field.reference),
                value_ref,
                dict: Dict::default(),
                warnings: vec![SignatureWarning::ValueNotADictionary],
            });
        }
    }
}

/// 12.8.4: the catalog's `/Perms` entries, each of which is a signature
/// dictionary that need not be any field's value.
///
/// Three corpus files carry a signature this way and no other, and one more is
/// a `/FT /Sig` annotation in a document with no `/AcroForm` at all, so no
/// field walk can reach it.
fn collect_permissions(
    cos: &CosDocument,
    keys: &Keys,
    seen: &mut BTreeSet<(u32, u16)>,
    into: &mut Vec<Candidate>,
) {
    let Some(catalog) = cos.catalog() else {
        return;
    };
    let perms = cos.resolve_key(&catalog, keys.perms);
    let Some(perms) = perms.as_dict() else {
        return;
    };
    for (key, value) in perms.iter() {
        let Some(label) = cos.name_bytes(*key) else {
            continue;
        };
        let value_ref = value.as_objref();
        let resolved = cos.resolve(value);
        let Some(sig) = resolved.as_dict() else {
            continue;
        };
        if sig.get(keys.byte_range).is_none() {
            continue;
        }
        if !remember(seen, value_ref) {
            continue;
        }
        into.push(Candidate {
            anchor: Anchor::Permissions(String::from_utf8_lossy(&label).into_owned()),
            field: None,
            field_ref: None,
            value_ref,
            dict: sig.clone(),
            warnings: Vec::new(),
        });
    }
}

fn read(cos: &CosDocument, keys: &Keys, candidate: Candidate) -> Signature {
    let bytes = cos.bytes();
    let file_len = bytes.len() as u64;
    let sig = &candidate.dict;
    let mut warnings = candidate.warnings;

    let (spans, from_range) = read_byte_range(cos, sig, keys.byte_range, file_len);
    let (contents, contents_at, gap) = read_contents(bytes, &spans, &mut warnings);
    if sig.get(Name::CONTENTS).is_none() && !spans.is_empty() {
        warnings.push(SignatureWarning::ContentsMissing);
    }

    let coverage = match from_range {
        Ok(()) => classify(cos, bytes, &spans, gap),
        Err(defect) => Coverage::Suspicious(defect),
    };

    let (certification, field_lock) = transforms(cos, sig, keys);

    let sub_filter_bytes = sig
        .get_name(keys.sub_filter)
        .and_then(|name| cos.name_bytes(name));
    let recognised = sub_filter_bytes.as_deref().and_then(SubFilter::from_bytes);
    let sub_filter_name = sub_filter_bytes
        .as_deref()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
    match (&sub_filter_name, recognised) {
        (None, _) => warnings.push(SignatureWarning::SubFilterMissing),
        (Some(name), None) => warnings.push(SignatureWarning::SubFilterUnknown(name.clone())),
        (Some(_), Some(_)) => {}
    }

    Signature {
        anchor: candidate.anchor,
        field: candidate.field,
        field_ref: candidate.field_ref,
        value_ref: candidate.value_ref,
        filter: sig
            .get_name(Name::FILTER)
            .and_then(|name| cos.name_bytes(name))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        sub_filter: recognised,
        sub_filter_name,
        spans,
        coverage,
        contents,
        contents_at,
        signed_at: text_of(cos, sig, keys.modified)
            .as_deref()
            .and_then(tinker_pdf_cos::parse_date),
        reason: text_of(cos, sig, keys.reason),
        location: text_of(cos, sig, keys.location),
        name: text_of(cos, sig, keys.signer),
        contact: text_of(cos, sig, keys.contact),
        certification,
        field_lock,
        warnings,
    }
}

/// 12.8.2: the `/Reference` array's transforms, which is where a signature
/// stops being a claim about bytes and becomes a claim about what may change.
///
/// Both methods are read from the same array in one pass, because a signature
/// may carry both and reading it twice would be two chances to disagree about
/// what it said.
fn transforms(
    cos: &CosDocument,
    sig: &Dict,
    keys: &Keys,
) -> (Option<Certification>, Option<FieldLock>) {
    let array = cos.resolve_key(sig, keys.reference);
    let Some(entries) = array.as_array() else {
        return (None, None);
    };
    let (mut certification, mut lock) = (None, None);
    for entry in entries {
        let resolved = cos.resolve(entry);
        let Some(reference) = resolved.as_dict() else {
            continue;
        };
        let Some(method) = reference
            .get_name(keys.transform_method)
            .and_then(|name| cos.name_bytes(name))
        else {
            continue;
        };
        let params = cos.resolve_key(reference, keys.transform_params);
        let Some(params) = params.as_dict() else {
            continue;
        };
        match method.as_ref() {
            b"DocMDP" => {
                certification = params
                    .get_int(keys.permissions)
                    .and_then(Certification::from_level);
            }
            b"FieldMDP" => lock = field_lock(cos, params, keys),
            _ => {}
        }
    }
    (certification, lock)
}

/// 12.8.2.4 Table 257's `/Action` and `/Fields`.
fn field_lock(cos: &CosDocument, params: &Dict, keys: &Keys) -> Option<FieldLock> {
    let action = params
        .get_name(keys.action)
        .and_then(|name| cos.name_bytes(name))?;
    let named = || -> Vec<String> {
        let fields = cos.resolve_key(params, keys.fields);
        fields
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        cos.resolve(item)
                            .as_string()
                            .map(|text| tinker_pdf_cos::decode_text_string(&text.bytes))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    match action.as_ref() {
        b"All" => Some(FieldLock::All),
        b"Include" => Some(FieldLock::Include(named())),
        b"Exclude" => Some(FieldLock::Exclude(named())),
        // An `/Action` outside the three 12.8.2.4 defines locks nothing, which
        // is the reading that does not invent a restriction the file did not
        // ask for.
        _ => None,
    }
}

/// A text string entry, decoded per 7.9.2.2.
fn text_of(cos: &CosDocument, dict: &Dict, key: Name) -> Option<String> {
    let object = cos.resolve_key(dict, key);
    let string = object.as_string()?;
    Some(tinker_pdf_cos::decode_text_string(&string.bytes))
}

/// `/ByteRange` as spans, with the checks that do not need the file's
/// revisions.
///
/// Splitting the checks in two is deliberate: everything here is arithmetic on
/// four numbers and holds whatever the document is, while [`classify`] needs
/// to know where the revision boundaries are.
fn read_byte_range(
    cos: &CosDocument,
    sig: &Dict,
    key: Name,
    file_len: u64,
) -> (Vec<Range<u64>>, Result<(), CoverageDefect>) {
    let object = cos.resolve_key(sig, key);
    let Some(array) = object.as_array() else {
        return (Vec::new(), Err(CoverageDefect::Missing));
    };
    if array.len() != 4 {
        return (
            Vec::new(),
            Err(CoverageDefect::NotFourNumbers { count: array.len() }),
        );
    }

    let mut numbers = [0u64; 4];
    for (slot, entry) in numbers.iter_mut().zip(array) {
        let resolved = cos.resolve(entry);
        let Some(value) = resolved
            .as_int()
            .and_then(|value| u64::try_from(value).ok())
        else {
            return (Vec::new(), Err(CoverageDefect::NotAnOffset));
        };
        *slot = value;
    }

    let [first_at, first_len, second_at, second_len] = numbers;
    let (Some(first_end), Some(second_end)) = (
        first_at.checked_add(first_len),
        second_at.checked_add(second_len),
    ) else {
        return (Vec::new(), Err(CoverageDefect::NotAnOffset));
    };

    if first_at != 0 {
        return (
            Vec::new(),
            Err(CoverageDefect::DoesNotStartAtZero { first: first_at }),
        );
    }
    if second_at < first_end {
        return (Vec::new(), Err(CoverageDefect::SpansOverlap));
    }
    if second_end > file_len {
        return (
            Vec::new(),
            Err(CoverageDefect::PastEndOfFile {
                end: second_end,
                file: file_len,
            }),
        );
    }

    (vec![first_at..first_end, second_at..second_end], Ok(()))
}

/// What the gap between the spans turned out to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gap {
    /// `<` hex `>`, the delimiters inside the gap.
    Delimited,
    /// The hexadecimal digits alone, with `<` and `>` covered by the
    /// signature.
    Tight,
    /// Anything else, which means `/ByteRange` does not point at `/Contents`.
    Elsewhere,
}

/// The `/Contents` bytes, taken from the gap the spans leave.
///
/// # The two conventions, both in the corpus
///
/// Seven of the nine well-formed signed files put the `<` and `>` **inside**
/// the gap, so the signature does not cover its own delimiters. One puts them
/// outside, covering them. Both are read; the second is named, because the
/// difference is two bytes and two bytes inside or outside a digest is a
/// different digest.
fn read_contents(
    bytes: &[u8],
    spans: &[Range<u64>],
    warnings: &mut Vec<SignatureWarning>,
) -> (Vec<u8>, Option<Range<u64>>, Gap) {
    let [first, second] = spans else {
        return (Vec::new(), None, Gap::Elsewhere);
    };
    let at = first.end..second.start;
    let (Ok(start), Ok(end)) = (usize::try_from(at.start), usize::try_from(at.end)) else {
        return (Vec::new(), None, Gap::Elsewhere);
    };
    let Some(gap) = bytes.get(start..end) else {
        return (Vec::new(), None, Gap::Elsewhere);
    };

    let trimmed = trim(gap);
    let (digits, shape) = match (trimmed.first(), trimmed.last()) {
        (Some(b'<'), Some(b'>')) if trimmed.len() >= 2 => {
            (&trimmed[1..trimmed.len() - 1], Gap::Delimited)
        }
        // 7.3.4.2: the delimiters sit just outside the gap, so the bytes
        // bracketing it have to be the ones the dictionary would have written.
        _ if start > 0 && bytes.get(start - 1) == Some(&b'<') && bytes.get(end) == Some(&b'>') => {
            warnings.push(SignatureWarning::ContentsGapExcludesDelimiters);
            (trimmed, Gap::Tight)
        }
        _ => return (Vec::new(), Some(at), Gap::Elsewhere),
    };

    let mut nibbles: Vec<u8> = Vec::with_capacity(digits.len());
    let mut clean = true;
    for byte in digits {
        if byte.is_ascii_whitespace() {
            continue;
        }
        match hex_value(*byte) {
            Some(value) => nibbles.push(value),
            None => clean = false,
        }
    }
    if !clean {
        warnings.push(SignatureWarning::ContentsNotHexadecimal);
        return (Vec::new(), Some(at), Gap::Elsewhere);
    }
    if nibbles.len() % 2 == 1 {
        warnings.push(SignatureWarning::ContentsOddDigitCount);
        nibbles.push(0);
    }

    let decoded = nibbles
        .chunks_exact(2)
        .map(|pair| (pair[0] << 4) | pair[1])
        .collect();
    (decoded, Some(at), shape)
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |at| at + 1);
    &bytes[start..end]
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Whether coverage ending at `covered` reaches `boundary`, allowing for the
/// end-of-line marker after `%%EOF`.
///
/// 7.5.5 ends the `%%EOF` line with an EOL marker, and `Revision::byte_range`
/// stops just past `%%EOF` itself — so a signer who included the marker and
/// one who did not are describing the same revision, two bytes apart. The
/// corpus contains the difference: `prefilled_f1040.pdf` covers to 299 340
/// where the revision ends at 299 339, and reading that as unsigned trailing
/// bytes would call a perfectly ordinary sign-then-update document suspicious.
///
/// The tolerance is exactly the EOL bytes and no others: at most two, and only
/// if they really are carriage returns or line feeds in the file.
fn reaches(bytes: &[u8], covered: u64, boundary: u64) -> bool {
    if covered == boundary {
        return true;
    }
    let Some(extra) = covered.checked_sub(boundary) else {
        return false;
    };
    if extra > 2 {
        return false;
    }
    let (Ok(from), Ok(to)) = (usize::try_from(boundary), usize::try_from(covered)) else {
        return false;
    };
    bytes
        .get(from..to)
        .is_some_and(|run| run.iter().all(|byte| *byte == b'\r' || *byte == b'\n'))
}

/// What the spans amount to, once the file's revision boundaries are known.
fn classify(cos: &CosDocument, bytes: &[u8], spans: &[Range<u64>], gap: Gap) -> Coverage {
    if gap == Gap::Elsewhere {
        return Coverage::Suspicious(CoverageDefect::GapIsNotContents);
    }
    let Some(last) = spans.last() else {
        return Coverage::Suspicious(CoverageDefect::Missing);
    };
    let file_len = bytes.len() as u64;
    // The whole file, allowing the signer to have stopped before the final
    // end-of-line marker rather than after it.
    if last.end == file_len || reaches(bytes, file_len, last.end) {
        return Coverage::WholeFile;
    }
    // 7.5.6: a signature over an earlier revision covers exactly that
    // revision's bytes, and the updates after it are legitimately outside.
    for (index, revision) in cos.revisions().iter().enumerate() {
        if reaches(bytes, last.end, revision.byte_range.end) {
            return Coverage::Revision { index };
        }
    }
    Coverage::Suspicious(CoverageDefect::EndsMidFile {
        covered_to: last.end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property milestone 1 exists to establish: a byte inside the covered
    /// range changes the digest, and a byte inside the gap does not.
    #[test]
    fn a_flipped_byte_inside_the_range_flips_the_digest_and_one_in_the_gap_does_not() {
        let original = b"0123456789".to_vec();
        let spans = [0..4u64, 6..10u64];
        let before = digest_spans(&original, &spans, DigestAlgorithm::Sha256).unwrap();

        let mut inside = original.clone();
        inside[2] ^= 0x01;
        let after_inside = digest_spans(&inside, &spans, DigestAlgorithm::Sha256).unwrap();
        assert_ne!(
            before, after_inside,
            "a covered byte must change the digest"
        );

        let mut in_gap = original.clone();
        in_gap[5] ^= 0x01;
        let after_gap = digest_spans(&in_gap, &spans, DigestAlgorithm::Sha256).unwrap();
        assert_eq!(before, after_gap, "the gap is outside the signature");
    }

    #[test]
    fn the_subfilter_names_round_trip() {
        for filter in [
            SubFilter::Pkcs7Detached,
            SubFilter::Pkcs7Sha1,
            SubFilter::X509RsaSha1,
            SubFilter::EtsiCadesDetached,
            SubFilter::EtsiRfc3161,
        ] {
            assert_eq!(
                SubFilter::from_bytes(filter.name().as_bytes()),
                Some(filter),
                "{filter:?}"
            );
        }
    }

    /// Both conventions the corpus contains, and the difference named.
    #[test]
    fn a_delimited_gap_and_a_tight_one_both_decode_and_only_one_is_silent() {
        let delimited = b"AAAA<0a0B>BBBB".to_vec();
        let mut warnings = Vec::new();
        let (contents, at, shape) = read_contents(&delimited, &[0..4u64, 10..14u64], &mut warnings);
        assert_eq!(contents, vec![0x0a, 0x0b]);
        assert_eq!(at, Some(4..10));
        assert!(shape == Gap::Delimited && warnings.is_empty());

        let tight = b"AAAA<0a0B>BBBB".to_vec();
        let mut warnings = Vec::new();
        let (contents, _, shape) = read_contents(&tight, &[0..5u64, 9..14u64], &mut warnings);
        assert_eq!(contents, vec![0x0a, 0x0b], "the same bytes either way");
        assert_eq!(shape, Gap::Tight);
        assert_eq!(
            warnings,
            vec![SignatureWarning::ContentsGapExcludesDelimiters],
            "covering its own delimiters is worth saying"
        );
    }

    #[test]
    fn a_gap_that_is_not_a_hexadecimal_string_yields_no_contents() {
        let prose = b"AAAAnot hex!BBBB".to_vec();
        let mut warnings = Vec::new();
        let (contents, _, shape) = read_contents(&prose, &[0..4u64, 12..16u64], &mut warnings);
        assert!(contents.is_empty() && shape == Gap::Elsewhere);
        assert!(warnings.is_empty(), "a missing gap is not a lenient read");

        // The shape the corpus actually shows: brackets in the right places
        // and something that is not hexadecimal between them.
        let markup = b"AAAA<rdf:Desc>BBBB".to_vec();
        let mut warnings = Vec::new();
        let (contents, _, shape) = read_contents(&markup, &[0..4u64, 14..18u64], &mut warnings);
        assert!(contents.is_empty() && shape == Gap::Elsewhere);
        assert_eq!(warnings, vec![SignatureWarning::ContentsNotHexadecimal]);
    }

    #[test]
    fn an_odd_digit_count_is_padded_and_named() {
        let file = b"AAAA<0a0>BBBB".to_vec();
        let mut warnings = Vec::new();
        let (contents, _, _) = read_contents(&file, &[0..4u64, 9..13u64], &mut warnings);
        assert_eq!(contents, vec![0x0a, 0x00], "7.3.4.3 pads with zero");
        assert_eq!(warnings, vec![SignatureWarning::ContentsOddDigitCount]);
    }
}
