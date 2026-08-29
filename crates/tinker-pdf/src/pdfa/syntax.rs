//! The syntax-only rule group: ISO 19005 clauses that need nothing but the
//! COS document (milestone 2 of `docs/design/pdfa.md`).
//!
//! # Why this group exists as a group
//!
//! The design doc's requirement is not "run these rules first", it is that a
//! sweep of these rules over the 2 907-file corpus **never builds the
//! machinery the other groups need** — no font program parsed, no ICC profile
//! read. That is what [`super::Machinery`] counts and what the unit tests in
//! [`super`] assert. This module names neither `tinker_pdf_font` nor
//! `tinker_pdf_color`, and a test greps this file to keep it that way, because
//! a counter proves what ran and a grep proves what *could* run.
//!
//! # Where the rules come from
//!
//! Each rule below cites the ISO 19005 clause it implements and paraphrases
//! the requirement it is reading. Where this build's reading of a clause is a
//! judgement call rather than a transcription, the comment says so in those
//! words — a rule whose provenance is a guess is worth less than one whose
//! provenance is the clause, and the difference has to be legible or it will
//! be forgotten.
//!
//! Nothing here consults any other validator's opinion about any file. The
//! corpus supplies bytes, and its publishers' `-pass-`/`-fail-` annotation is
//! the *bar* agreement is measured against (`tests/pdfa_ledger.rs`); it is not
//! where the rules come from, and where the two disagree the ledger records
//! this reading against its clause rather than moving the rule.

use std::collections::BTreeSet;

use tinker_pdf_cos::{CosDocument, Dict, Name, ObjRef, Object, XrefEntry};

use super::{clauses, FindingKind, Flavour, Level, Machinery, Part, Raw, RuleGroup};

/// How deep a directly nested array or dictionary is walked.
///
/// Indirect references are *not* followed here — every indirect object is
/// visited in its own right by the cross-reference sweep — so this bounds only
/// the nesting a single object's own bytes can declare, and stopping at the
/// bottom of it drops a subtree rather than the file.
const MAX_NESTING: u32 = 32;

/// How many objects the sweep examines.
///
/// The cross-reference table is already bounded by the reader, so this is belt
/// to that brace: a validator is asked to run over untrusted input, and a
/// sweep that cannot be bounded is a denial of service with a clause number.
const MAX_OBJECTS: usize = 1 << 20;

/// How many members of a filter array are read.
const MAX_FILTER_NAMES: usize = 64;

/// The actions ISO 19005-1 6.6.1 and ISO 19005-2/3 6.5.1 forbid outright.
///
/// The clause text names the action types a conforming file "shall not
/// contain". `SetState` and `NOP` are the two deprecated types the same
/// sentence adds; they are spelled here as they appear as an `/S` name.
const FORBIDDEN_ACTIONS: &[&[u8]] = &[
    b"Launch",
    b"Sound",
    b"Movie",
    b"ResetForm",
    b"ImportData",
    b"Hide",
    b"SetOCGState",
    b"Rendition",
    b"Trans",
    b"GoTo3DView",
    b"SetState",
    b"NOP",
];

/// The four `/Named` actions ISO 19005 permits.
///
/// The clause admits the page-navigation set and nothing else, so this list is
/// a whitelist rather than a blacklist — a named action nobody has heard of is
/// forbidden by the same sentence that forbids `/Print`.
const PERMITTED_NAMED_ACTIONS: &[&[u8]] = &[b"NextPage", b"PrevPage", b"FirstPage", b"LastPage"];

/// The keys ISO 19005-2 6.1.12 and ISO 19005-4 6.1.11 admit in the document
/// catalog's `/Perms` dictionary.
const PERMITTED_PERMISSIONS: &[&[u8]] = &[b"DocMDP", b"UR3"];

/// The filters ISO 32000-2 7.4 defines, long and abbreviated spellings.
const STANDARD_FILTERS: &[&[u8]] = &[
    b"ASCIIHexDecode",
    b"ASCII85Decode",
    b"LZWDecode",
    b"FlateDecode",
    b"RunLengthDecode",
    b"CCITTFaxDecode",
    b"JBIG2Decode",
    b"DCTDecode",
    b"JPXDecode",
    b"Crypt",
    b"AHx",
    b"A85",
    b"LZW",
    b"Fl",
    b"RL",
    b"CCF",
    b"DCT",
];

/// Runs every syntax rule that applies to `part`.
///
/// `part` is `None` when the file claimed no flavour. The part-independent
/// rules still run — a file with no claim can still carry an `/Encrypt` — and
/// the part-specific ones do not, because a rule that does not know which
/// standard it is enforcing is not enforcing one.
pub(super) fn rules(
    doc: &CosDocument,
    machinery: &Machinery,
    flavour: Option<Flavour>,
    out: &mut Vec<Raw>,
) {
    if !machinery.reach(RuleGroup::Syntax) {
        return;
    }
    let part = flavour.map(|f| f.part);
    header(doc, part, out);
    trailer(doc, part, out);
    catalog(doc, part, out);
    objects(doc, flavour, out);
}

// ---- 6.1.2 File header ----------------------------------------------------

/// ISO 19005-1 6.1.2, and the same clause number in parts 2 to 4.
///
/// The clause has three requirements and each is a separate finding, because
/// "the header is wrong" is not something a caller can act on:
///
/// 1. the header begins at byte zero;
/// 2. it names a version the part admits — `1.n` for parts 1 to 3, `2.n` for
///    part 4, which is the version each part is defined on top of;
/// 3. the line after it is a comment whose first four bytes are all above 127,
///    so that a transfer program sniffing the first bytes calls the file
///    binary rather than text.
///
/// **A reading, named as one.** Part 1's clause says the header shall consist
/// of `%PDF-1.n`. Read literally that forbids a part-1 file carrying a `2.0`
/// header, and this build reads it literally. Where the veraPDF corpus
/// annotates such a file `pass`, the difference is recorded in
/// `tests/pdfa_ledger.rs` against clause 6.1.2 rather than settled by moving
/// the rule.
fn header(doc: &CosDocument, part: Option<Part>, out: &mut Vec<Raw>) {
    let bytes = doc.bytes();
    let Some(at) = find_header(bytes) else {
        // No `%PDF-` at all within the scan window. The reader will have
        // repaired its way in; the clause is still broken.
        out.push(Raw::file(clauses::FILE_HEADER, FindingKind::HeaderMissing));
        return;
    };

    if at != 0 {
        out.push(Raw::file(
            clauses::FILE_HEADER,
            FindingKind::HeaderNotAtStart { at: at as u64 },
        ));
    }

    let digits: Vec<u8> = bytes
        .get(at + 5..)
        .unwrap_or_default()
        .iter()
        .copied()
        .take_while(|b| b.is_ascii_digit() || *b == b'.')
        .collect();
    let declared = String::from_utf8_lossy(&digits).into_owned();
    let admitted = match part {
        Some(Part::Four) => version_shaped(&digits, b'2'),
        // Parts 1 to 3 are defined on PDF 1.4, 1.7 and 1.7 respectively, and
        // all three spell the header requirement as `%PDF-1.n`.
        Some(_) => version_shaped(&digits, b'1'),
        // No claim: nothing to check the version against.
        None => true,
    };
    if !admitted {
        out.push(Raw::file(
            clauses::FILE_HEADER,
            FindingKind::HeaderVersionNotInPart { declared },
        ));
    }

    // The clause says the header line is **immediately** followed by a single
    // EOL marker and then by the comment. Both halves of that matter and this
    // rule got both wrong at first: it looked for the next line rather than
    // requiring the next bytes, so `%PDF-1.7` followed by three spaces and a
    // newline passed, and so did a header with a blank line between it and its
    // comment. Two corpus fixtures are exactly those two shapes.
    let after_version = at + 5 + digits.len();
    let rest = bytes.get(after_version..).unwrap_or_default();
    let eol = match (rest.first(), rest.get(1)) {
        (Some(b'\r'), Some(b'\n')) => 2,
        (Some(b'\r' | b'\n'), _) => 1,
        _ => {
            out.push(Raw::file(
                clauses::FILE_HEADER,
                FindingKind::HeaderNotFollowedBySingleEol,
            ));
            // Carry on from the next line anyway: a trailing space is one
            // defect and a missing binary comment would be another, and a
            // caller wants both rather than the first.
            second_line_offset(rest)
        }
    };
    let comment = rest.get(eol..).unwrap_or_default();
    if comment.first() != Some(&b'%') {
        out.push(Raw::file(
            clauses::FILE_HEADER,
            FindingKind::HeaderCommentMissing,
        ));
        return;
    }
    let four = comment.get(1..5).unwrap_or_default();
    if four.len() < 4 || !four.iter().all(|b| *b > 127) {
        out.push(Raw::file(
            clauses::FILE_HEADER,
            FindingKind::HeaderCommentNotBinary,
        ));
    }
}

/// `%PDF-`'s offset, searched only within a window at the front of the file.
fn find_header(bytes: &[u8]) -> Option<usize> {
    const WINDOW: usize = 4096;
    let window = bytes.get(..WINDOW.min(bytes.len()))?;
    window.windows(5).position(|w| w == b"%PDF-")
}

/// Whether `digits` is exactly `major`, `.`, and one decimal digit.
fn version_shaped(digits: &[u8], major: u8) -> bool {
    digits.len() == 3 && digits[0] == major && digits[1] == b'.' && digits[2].is_ascii_digit()
}

/// Where the next line begins, having skipped whatever ends this one.
fn second_line_offset(rest: &[u8]) -> usize {
    let Some(eol) = rest.iter().position(|b| *b == b'\r' || *b == b'\n') else {
        return rest.len();
    };
    let mut start = eol + 1;
    if rest.get(eol) == Some(&b'\r') && rest.get(start) == Some(&b'\n') {
        start += 1;
    }
    start
}

// ---- 6.1.3 File trailer ---------------------------------------------------

/// ISO 19005-1 6.1.3 and its siblings.
///
/// Two requirements this build reads out of the clause: the trailer dictionary
/// contains `/ID`, and it does not contain `/Encrypt`. The `/Encrypt` half is
/// answered by `CosDocument::is_encrypted` in [`super::validate`], where it is
/// reported before any other rule; here is the `/ID` half.
///
/// The `/ID` value is two strings (ISO 32000-1 14.4). A trailer carrying an
/// `/ID` that is not two strings has the keyword without the identifier, which
/// is a different defect from not having it, so the two are different kinds.
///
/// **A reading, named as one.** The trailer consulted is the reader's merged
/// view across revisions, not the last section's bytes. An incremental update
/// that drops `/ID` from its own trailer therefore reads as having one. The
/// clause is about the file's identifier rather than about any one section's
/// spelling of it, which is why the merged view is the one used; a build that
/// wanted the stricter reading would need the per-revision trailers, and they
/// are available (`CosDocument::revisions`).
fn trailer(doc: &CosDocument, part: Option<Part>, out: &mut Vec<Raw>) {
    let trailer = doc.trailer();
    let id = doc.resolve_key(trailer, doc.intern(b"ID"));
    match id.as_array() {
        None if id.is_null() => out.push(Raw::file(
            clauses::FILE_TRAILER,
            FindingKind::FileIdentifierMissing,
        )),
        None => out.push(Raw::file(
            clauses::FILE_TRAILER,
            FindingKind::FileIdentifierMalformed,
        )),
        Some(values) if values.len() != 2 || !values.iter().all(|v| v.as_string().is_some()) => {
            out.push(Raw::file(
                clauses::FILE_TRAILER,
                FindingKind::FileIdentifierMalformed,
            ));
        }
        Some(_) => {}
    }

    // ISO 19005-4 6.1.3: a PDF/A-4 file carries no document information
    // dictionary unless its catalog has a `/PieceInfo`, and then the only
    // entry admitted is `/ModDate`. Parts 1 to 3 have no such restriction —
    // they require the opposite, that whatever `/Info` says agrees with the
    // XMP, which is the metadata group's rule.
    if part != Some(Part::Four) {
        return;
    }
    let info_ref = trailer.get_ref(doc.intern(b"Info"));
    let info = doc.resolve_key(trailer, doc.intern(b"Info"));
    let Some(info) = info.as_dict() else {
        return;
    };
    let has_piece_info = doc
        .catalog()
        .is_some_and(|c| !doc.resolve_key(&c, doc.intern(b"PieceInfo")).is_null());
    if !has_piece_info {
        out.push(Raw {
            rule: clauses::FILE_TRAILER,
            object: info_ref,
            kind: FindingKind::InfoDictionaryForbidden,
        });
        return;
    }
    for (key, _) in info.entries() {
        let Some(name) = doc.name_bytes(*key) else {
            continue;
        };
        if name.as_ref() != b"ModDate" {
            out.push(Raw {
                rule: clauses::FILE_TRAILER,
                object: info_ref,
                kind: FindingKind::InfoEntryForbidden {
                    key: String::from_utf8_lossy(&name).into_owned(),
                },
            });
        }
    }
}

// ---- the document catalog -------------------------------------------------

/// Catalog-level prohibitions, each from its own clause.
fn catalog(doc: &CosDocument, part: Option<Part>, out: &mut Vec<Raw>) {
    let Some(catalog) = doc.catalog() else {
        return;
    };
    let at = doc.trailer().get_ref(Name::ROOT);

    // ISO 19005-1 6.1.13: optional content is not permitted in a part 1 file.
    // Parts 2 to 4 admit it under their own constraints, which are not syntax
    // rules and are not checked here.
    if part == Some(Part::One)
        && !doc
            .resolve_key(&catalog, doc.intern(b"OCProperties"))
            .is_null()
    {
        out.push(Raw {
            rule: clauses::OPTIONAL_CONTENT,
            object: at,
            kind: FindingKind::OptionalContentForbidden,
        });
    }

    // ISO 19005-1 6.1.11: no embedded files in a part 1 document. The
    // catalog's `/Names /EmbeddedFiles` tree is the document-level way to have
    // one; the per-file-specification way is caught in the object sweep.
    if part == Some(Part::One) {
        let names = doc.resolve_key(&catalog, doc.intern(b"Names"));
        if let Some(names) = names.as_dict() {
            if !doc
                .resolve_key(names, doc.intern(b"EmbeddedFiles"))
                .is_null()
            {
                out.push(Raw {
                    rule: clauses::EMBEDDED_FILES,
                    object: at,
                    kind: FindingKind::EmbeddedFileForbidden,
                });
            }
        }
    }

    // ISO 19005-1 6.9, ISO 19005-2/3 6.4, ISO 19005-4 6.4: an interactive form
    // shall not be an XFA form, and the catalog shall not ask a reader to
    // render one. The rule is the same shape in every part; the clause table
    // is what differs.
    let acro = doc.resolve_key(&catalog, doc.intern(b"AcroForm"));
    if let Some(acro) = acro.as_dict() {
        if !doc.resolve_key(acro, doc.intern(b"XFA")).is_null() {
            out.push(Raw {
                rule: clauses::INTERACTIVE_FORMS,
                object: catalog.get_ref(doc.intern(b"AcroForm")).or(at),
                kind: FindingKind::XfaForbidden,
            });
        }
    }
    if doc
        .resolve_key(&catalog, doc.intern(b"NeedsRendering"))
        .as_bool()
        == Some(true)
    {
        out.push(Raw {
            rule: clauses::INTERACTIVE_FORMS,
            object: at,
            kind: FindingKind::NeedsRenderingForbidden,
        });
    }

    // ISO 19005-2 6.1.12 / ISO 19005-4 6.1.11: the permissions dictionary
    // admits `/DocMDP` and `/UR3` and no other key. Part 1 predates `/Perms`
    // and says nothing about it, so the rule does not run there.
    if matches!(part, Some(Part::Two | Part::Three | Part::Four)) {
        let perms = doc.resolve_key(&catalog, doc.intern(b"Perms"));
        if let Some(perms) = perms.as_dict() {
            for (key, _) in perms.entries() {
                let Some(name) = doc.name_bytes(*key) else {
                    continue;
                };
                if !PERMITTED_PERMISSIONS.contains(&name.as_ref()) {
                    out.push(Raw {
                        rule: clauses::PERMISSIONS,
                        object: catalog.get_ref(doc.intern(b"Perms")).or(at),
                        kind: FindingKind::PermissionsEntryForbidden {
                            key: String::from_utf8_lossy(&name).into_owned(),
                        },
                    });
                }
            }
        }
    }

    // ISO 19005-4 6.1.12: if the catalog declares a `/Version` it is `2.n`,
    // three characters, no more.
    if part == Some(Part::Four) {
        let version = doc.resolve_key(&catalog, doc.intern(b"Version"));
        if let Some(bytes) = version.as_name().and_then(|n| doc.name_bytes(n)) {
            if !version_shaped(&bytes, b'2') {
                out.push(Raw {
                    rule: clauses::CATALOG,
                    object: at,
                    kind: FindingKind::CatalogVersionMalformed {
                        declared: String::from_utf8_lossy(&bytes).into_owned(),
                    },
                });
            }
        }
    }
}

// ---- the object sweep -----------------------------------------------------

/// Every rule whose subject is an object rather than the file.
///
/// One pass over the cross-reference table, with each object's own direct
/// nesting walked without following references — every referenced object has
/// its own entry, so following them would visit the same dictionaries twice
/// and would need a cycle guard to do it.
fn objects(doc: &CosDocument, flavour: Option<Flavour>, out: &mut Vec<Raw>) {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for (num, entry) in doc.xref().iter().take(MAX_OBJECTS) {
        let gen = match entry {
            XrefEntry::Free { .. } => continue,
            XrefEntry::Offset { gen, .. } => gen,
            XrefEntry::InStream { .. } => 0,
        };
        if !seen.insert(num) {
            continue;
        }
        let reference = ObjRef::new(num, gen);
        let Ok(object) = doc.get(reference) else {
            continue;
        };
        walk(doc, flavour, &object, reference, 0, out);
    }
}

fn walk(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    object: &Object,
    at: ObjRef,
    depth: u32,
    out: &mut Vec<Raw>,
) {
    if depth > MAX_NESTING {
        return;
    }
    match object {
        Object::Array(values) => {
            for value in values {
                walk(doc, flavour, value, at, depth + 1, out);
            }
        }
        Object::Dict(dict) => {
            dictionary(doc, flavour, dict, at, false, out);
            for (_, value) in dict.entries() {
                walk(doc, flavour, value, at, depth + 1, out);
            }
        }
        Object::Stream(stream) => {
            dictionary(doc, flavour, &stream.dict, at, true, out);
            for (_, value) in stream.dict.entries() {
                walk(doc, flavour, value, at, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The rules whose subject is one dictionary.
fn dictionary(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    at: ObjRef,
    is_stream: bool,
    out: &mut Vec<Raw>,
) {
    let part = flavour.map(|f| f.part);
    if is_stream {
        stream_rules(doc, dict, at, out);
        // 6.1.10 is about a *stream's* filter chain, and only a stream's — a
        // signature dictionary's `/Filter` names a security handler
        // (`/Adobe.PPKLite`) and an encryption dictionary's names a crypt
        // filter. Reading either as a stream filter reported a conforming
        // part 4 file for carrying a signature, which is what this branch
        // exists to stop.
        filters(doc, part, dict, at, out);
    }
    action_rules(doc, flavour, dict, at, out);
    trigger_rules(doc, flavour, dict, at, out);
    embedded_file_rules(doc, part, dict, at, out);
}

/// ISO 19005-1 6.1.7 (parts 2 and 3: 6.1.7.1; part 4: 6.1.6).
///
/// A stream whose data lives in another file is not archival: the clause
/// forbids `/F`, `/FFilter` and `/FDecodeParms` in a stream dictionary. Each
/// key present is its own finding, so a caller reading the list knows which
/// were there.
fn stream_rules(doc: &CosDocument, dict: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    for key in [&b"F"[..], b"FFilter", b"FDecodeParms"] {
        if dict.contains_key(doc.intern(key)) {
            out.push(Raw {
                rule: clauses::STREAM_OBJECTS,
                object: Some(at),
                kind: FindingKind::ExternalStream {
                    key: String::from_utf8_lossy(key).into_owned(),
                },
            });
        }
    }
}

/// ISO 19005-1 6.1.10 (parts 2 and 3: 6.1.7.1; part 4: 6.1.6).
///
/// Three requirements. `LZWDecode` is forbidden in every part — the clause
/// names it, and the reason it names it is a patent encumbrance the standard
/// would not inherit. A `/Crypt` filter is admitted only when it names the
/// `/Identity` filter, since anything else is encryption under another name
/// and 6.1.3 has already forbidden that. And part 4, defined on ISO 32000-2,
/// admits only the filters that standard defines.
///
/// The abbreviated inline-image spellings are checked too: an inline image
/// dictionary writes `/F /LZW`, and a rule that knew only the long name would
/// pass the same filter written short.
fn filters(doc: &CosDocument, part: Option<Part>, dict: &Dict, at: ObjRef, out: &mut Vec<Raw>) {
    let mut names: Vec<Vec<u8>> = Vec::new();
    collect_names(
        doc,
        &doc.resolve_key(dict, doc.intern(b"Filter")),
        &mut names,
    );

    for name in &names {
        if matches!(name.as_slice(), b"LZWDecode" | b"LZW") {
            out.push(Raw {
                rule: clauses::FILTERS,
                object: Some(at),
                kind: FindingKind::FilterForbidden {
                    filter: String::from_utf8_lossy(name).into_owned(),
                },
            });
        }
    }

    if names.iter().any(|n| n.as_slice() == b"Crypt") {
        let params = doc.resolve_key(dict, doc.intern(b"DecodeParms"));
        let mut dicts: Vec<&Dict> = Vec::new();
        if let Some(d) = params.as_dict() {
            dicts.push(d);
        }
        if let Some(values) = params.as_array() {
            dicts.extend(values.iter().filter_map(Object::as_dict));
        }
        // No `/DecodeParms` at all means the default, which ISO 32000-1 7.4.10
        // gives as `/Identity`. So only a *named* non-identity filter is a
        // finding, and a bare `/Crypt` is not.
        let named = dicts.iter().any(|d| d.contains_key(doc.intern(b"Name")));
        let identity = dicts.iter().any(|d| {
            doc.resolve_key(d, doc.intern(b"Name"))
                .as_name()
                .and_then(|n| doc.name_bytes(n))
                .is_some_and(|n| n.as_ref() == b"Identity")
        });
        if named && !identity {
            out.push(Raw {
                rule: clauses::FILTERS,
                object: Some(at),
                kind: FindingKind::CryptFilterNotIdentity,
            });
        }
    }

    if part == Some(Part::Four) {
        for name in &names {
            if !STANDARD_FILTERS.contains(&name.as_slice()) {
                out.push(Raw {
                    rule: clauses::FILTERS,
                    object: Some(at),
                    kind: FindingKind::FilterNotStandard {
                        filter: String::from_utf8_lossy(name).into_owned(),
                    },
                });
            }
        }
    }
}

/// Every name in an object that is a name or an array of them.
fn collect_names(doc: &CosDocument, object: &Object, into: &mut Vec<Vec<u8>>) {
    match object {
        Object::Name(name) => {
            if let Some(bytes) = doc.name_bytes(*name) {
                into.push(bytes.to_vec());
            }
        }
        Object::Array(values) => {
            for value in values.iter().take(MAX_FILTER_NAMES) {
                let resolved = doc.resolve(value);
                if let Object::Name(name) = resolved.as_ref() {
                    if let Some(bytes) = doc.name_bytes(*name) {
                        into.push(bytes.to_vec());
                    }
                }
            }
        }
        _ => {}
    }
}

/// ISO 19005-1 6.6.1, ISO 19005-2/3 6.5.1, ISO 19005-4 6.6.1.
///
/// A dictionary is treated as an action when its `/S` names one, which is what
/// ISO 32000-1 12.6.2 says an action dictionary is. The alternative — walking
/// only the `/A`, `/AA`, `/OpenAction` and `/Next` slots — misses an action
/// reached through a slot this build does not enumerate, and the cost of the
/// broader reading is that a non-action dictionary carrying an `/S` whose
/// value happened to be one of these thirteen names would be reported. No such
/// name is used as an `/S` value elsewhere in ISO 32000: `/Transparency`,
/// `/GTS_PDFA1` and the border styles are all outside the set.
///
/// **Part 4 does not forbid `/JavaScript`.** ISO 19005-4's list drops it,
/// which is the one difference between part 4's sentence and part 2's, and it
/// is why `part` reaches this rule at all. Named as a reading rather than a
/// transcription: it is this build's understanding of the clause, and if it is
/// wrong the ledger row for a part 4 JavaScript fixture is where it shows.
fn action_rules(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    // ISO 19005-4 level E is the engineering conformance level, and it exists
    // to permit what the other levels forbid — 3D artwork and rich media. How
    // far that reaches into 6.6.1's action list is not something this build
    // has established from the clause, so the prohibition **does not run for
    // level E at all** rather than running on a guess. `super::STAGED` names
    // the refusal; enforcing a list read off two fixtures would have been a
    // transcription of somebody else's reading of a clause we have not read.
    if flavour.map(|f| f.level) == Some(Some(Level::E)) {
        return;
    }
    let part = flavour.map(|f| f.part);
    let Some(subtype) = doc
        .resolve_key(dict, doc.intern(b"S"))
        .as_name()
        .and_then(|n| doc.name_bytes(n))
    else {
        return;
    };

    if FORBIDDEN_ACTIONS.contains(&subtype.as_ref()) {
        out.push(Raw {
            rule: clauses::ACTIONS,
            object: Some(at),
            kind: FindingKind::ActionForbidden {
                action: String::from_utf8_lossy(&subtype).into_owned(),
            },
        });
        return;
    }

    if subtype.as_ref() == b"JavaScript" && part != Some(Part::Four) {
        out.push(Raw {
            rule: clauses::ACTIONS,
            object: Some(at),
            kind: FindingKind::ActionForbidden {
                action: "JavaScript".to_string(),
            },
        });
        return;
    }

    if subtype.as_ref() == b"Named" {
        let named = doc
            .resolve_key(dict, doc.intern(b"N"))
            .as_name()
            .and_then(|n| doc.name_bytes(n));
        let permitted = named
            .as_ref()
            .is_some_and(|n| PERMITTED_NAMED_ACTIONS.contains(&n.as_ref()));
        if !permitted {
            out.push(Raw {
                rule: clauses::ACTIONS,
                object: Some(at),
                kind: FindingKind::NamedActionForbidden {
                    name: named
                        .map(|n| String::from_utf8_lossy(&n).into_owned())
                        .unwrap_or_default(),
                },
            });
        }
    }
}

/// ISO 19005-1 6.6.2 and ISO 19005-2/3 6.5.2: no additional-actions
/// dictionaries.
///
/// The clause forbids `/AA` on the document catalog, on a page, on an
/// annotation and on a form field — which between them is every dictionary
/// ISO 32000-1 lets carry one, so the rule is spelled as "no `/AA` anywhere"
/// rather than as four rules that would each have to identify their subject.
///
/// **Part 4 is staged, deliberately.** ISO 19005-4 6.6.3 permits some trigger
/// events and forbids others rather than forbidding the entry, and this build
/// has not read that list closely enough to enforce it. A rule reverse
/// engineered from a handful of fixtures would be a transcription of somebody
/// else's reading, so there is no part 4 rule here and [`super::STAGED`] names
/// the gap.
fn trigger_rules(
    doc: &CosDocument,
    flavour: Option<Flavour>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    if !matches!(
        flavour.map(|f| f.part),
        Some(Part::One | Part::Two | Part::Three)
    ) {
        return;
    }
    if doc.resolve_key(dict, doc.intern(b"AA")).as_dict().is_some() {
        out.push(Raw {
            rule: clauses::TRIGGERS,
            object: Some(at),
            kind: FindingKind::TriggerEventsForbidden,
        });
    }
}

/// ISO 19005-1 6.1.11, ISO 19005-3 6.8, ISO 19005-4 6.9.
///
/// The four parts say four different things about a file specification that
/// carries an `/EF`:
///
/// - **Part 1** forbids it outright.
/// - **Part 2** admits it only when the embedded file is itself PDF/A, which
///   needs a recursive validation this milestone does not do — staged, and
///   named in [`super::STAGED`] rather than passed silently.
/// - **Part 3** requires an `/AFRelationship`, which is what makes an
///   attachment's role legible without opening it.
/// - **Part 4** requires the same, and additionally the `/F` and `/UF` names,
///   because PDF 2.0 made `/UF` the file name a reader is to believe.
fn embedded_file_rules(
    doc: &CosDocument,
    part: Option<Part>,
    dict: &Dict,
    at: ObjRef,
    out: &mut Vec<Raw>,
) {
    if doc.resolve_key(dict, doc.intern(b"EF")).as_dict().is_none() {
        return;
    }
    match part {
        Some(Part::One) => out.push(Raw {
            rule: clauses::EMBEDDED_FILES,
            object: Some(at),
            kind: FindingKind::EmbeddedFileForbidden,
        }),
        Some(Part::Three) => missing_keys(doc, dict, at, &[b"AFRelationship"], out),
        Some(Part::Four) => missing_keys(doc, dict, at, &[b"AFRelationship", b"F", b"UF"], out),
        _ => {}
    }
}

fn missing_keys(doc: &CosDocument, dict: &Dict, at: ObjRef, keys: &[&[u8]], out: &mut Vec<Raw>) {
    for key in keys {
        if doc.resolve_key(dict, doc.intern(key)).is_null() {
            out.push(Raw {
                rule: clauses::EMBEDDED_FILES,
                object: Some(at),
                kind: FindingKind::EmbeddedFileKeyMissing {
                    key: String::from_utf8_lossy(key).into_owned(),
                },
            });
        }
    }
}
