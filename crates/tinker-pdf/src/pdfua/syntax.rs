//! The syntax group: what ISO 14289 asks of the catalog and the object graph
//! that no machinery past the COS document is needed to decide.
//!
//! Milestone 2 of `docs/design/pdfua.md`. Each rule is a dictionary read, and
//! each cites the clause veraPDF's published rules give it
//! (`veraPDF-validation-profiles` `070d39f`, and the wiki's statement of each
//! rule at `109b482`; read as data, ruling 13). Where the two parts differ the
//! difference is in the clause table, or — for optional content and embedded
//! files, whose requirements themselves differ — in the code, with the
//! sentence each part states quoted beside it.

use std::collections::BTreeSet;

use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef, Object, XrefEntry};

use super::{clauses, UaPart, UaRaw};
use crate::pdfa::FindingKind;

/// How many indirect objects the object-graph rules visit.
const MAX_OBJECTS: usize = 1 << 20;

/// How deep the walk into one object's direct values goes.
const MAX_NESTING: u32 = 32;

/// How many findings one rule contributes, for the reason
/// `super::structure::MAX_FINDINGS_PER_RULE` gives.
const MAX_FINDINGS_PER_RULE: usize = 64;

/// How many optional content configurations are read.
const MAX_CONFIGS: usize = 1 << 10;

/// Runs the syntax group.
pub(super) fn rules(doc: &CosDocument, part: UaPart, out: &mut Vec<UaRaw>) {
    if let Some(catalog) = doc.catalog() {
        metadata_stream(doc, &catalog, out);
        display_doc_title(doc, &catalog, out);
        optional_content(doc, &catalog, part, out);
        xfa(doc, &catalog, out);
        if part == UaPart::Two {
            embedded_file_descriptions(doc, &catalog, out);
        }
    }
    encryption(doc, out);
    objects(doc, part, out);
}

/// UA-1 7.1-8, UA-2 8.11.1-2: "The Catalog dictionary of a conforming file
/// shall contain the Metadata key whose value is a metadata stream … The
/// metadata stream dictionary shall contain entry Type with value /Metadata
/// and entry Subtype with value /XML".
fn metadata_stream(doc: &CosDocument, catalog: &Dict, out: &mut Vec<UaRaw>) {
    let value = doc.resolve_key(catalog, doc.intern(b"Metadata"));
    let Some(stream) = value.as_stream() else {
        out.push(UaRaw::file(
            clauses::METADATA_STREAM,
            FindingKind::MetadataMissing,
        ));
        return;
    };
    let object = catalog.get_ref(doc.intern(b"Metadata"));
    for (key, wanted) in [(&b"Type"[..], &b"Metadata"[..]), (b"Subtype", b"XML")] {
        let ok = doc
            .resolve_key(&stream.dict, doc.intern(key))
            .as_name()
            .and_then(|name| doc.name_bytes(name))
            .is_some_and(|name| name.as_ref() == wanted);
        if !ok {
            out.push(UaRaw {
                rule: clauses::METADATA_STREAM,
                object,
                kind: FindingKind::MetadataStreamMalformed {
                    key: String::from_utf8_lossy(key).into_owned(),
                },
            });
        }
    }
}

/// UA-1 7.1-10, UA-2 8.11.2-1: "The document catalog dictionary shall
/// include a ViewerPreferences dictionary containing a DisplayDocTitle key,
/// whose value shall be true."
fn display_doc_title(doc: &CosDocument, catalog: &Dict, out: &mut Vec<UaRaw>) {
    let preferences = doc.resolve_key(catalog, doc.intern(b"ViewerPreferences"));
    let set = preferences.as_dict().is_some_and(|preferences| {
        doc.resolve_key(preferences, doc.intern(b"DisplayDocTitle"))
            .as_bool()
            == Some(true)
    });
    if !set {
        out.push(UaRaw::file(
            clauses::DISPLAY_DOC_TITLE,
            FindingKind::DisplayDocTitleNotSet,
        ));
    }
}

/// Optional content configurations: a `/Name` on each, and `/AS` on none.
///
/// **The two parts ask the naming half differently**, and both sentences are
/// quoted because the difference is the whole rule:
///
/// - UA-1 7.10-1: "Each optional content configuration dictionary that forms
///   the value of the D key, or that is an element in the array that forms
///   the value of the Configs key … shall contain the Name key" — veraPDF's
///   test asks for a non-empty one;
/// - UA-2 8.7-1: the same, "when: a) a document contains a Configs entry …
///   and b) the Configs entry contains at least one optional content
///   configuration dictionary" — a lone default configuration may go
///   unnamed.
///
/// The `/AS` half is one sentence in both (7.10-2, 8.7-2): "The AS key shall
/// not appear in any optional content configuration dictionary."
fn optional_content(doc: &CosDocument, catalog: &Dict, part: UaPart, out: &mut Vec<UaRaw>) {
    let properties = doc.resolve_key(catalog, doc.intern(b"OCProperties"));
    let Some(properties) = properties.as_dict() else {
        return;
    };
    let mut configs: Vec<(Option<ObjRef>, Dict)> = Vec::new();
    let default = doc.resolve_key(properties, doc.intern(b"D"));
    if let Some(default) = default.as_dict() {
        configs.push((properties.get_ref(doc.intern(b"D")), default.clone()));
    }
    let listed = doc.resolve_key(properties, doc.intern(b"Configs"));
    let mut alternatives = 0usize;
    if let Some(listed) = listed.as_array() {
        for entry in listed.iter().take(MAX_CONFIGS) {
            let resolved = doc.resolve(entry);
            if let Some(config) = resolved.as_dict() {
                alternatives += 1;
                configs.push((entry.as_objref(), config.clone()));
            }
        }
    }
    let names_required = match part {
        UaPart::One => true,
        UaPart::Two => alternatives > 0,
    };
    for (object, config) in configs.iter().take(MAX_FINDINGS_PER_RULE) {
        if names_required {
            let name = doc.resolve_key(config, doc.intern(b"Name"));
            let named = name
                .as_string()
                .is_some_and(|name| !decode_text_string(&name.bytes).is_empty());
            if !named {
                out.push(UaRaw {
                    rule: clauses::OPTIONAL_CONTENT,
                    object: *object,
                    kind: FindingKind::OptionalContentConfigUnnamed,
                });
            }
        }
        if !doc.resolve_key(config, doc.intern(b"AS")).is_null() {
            out.push(UaRaw {
                rule: clauses::OPTIONAL_CONTENT,
                object: *object,
                kind: FindingKind::OptionalContentConfigAutoState,
            });
        }
    }
}

/// UA-2 8.10.1-3: "XFA forms shall not be present." Part 1 forbids dynamic
/// XFA only, and its row in the clause table is empty: telling dynamic from
/// static needs the XFA packet's `dynamicRender`, which is staged.
fn xfa(doc: &CosDocument, catalog: &Dict, out: &mut Vec<UaRaw>) {
    let form = doc.resolve_key(catalog, doc.intern(b"AcroForm"));
    let Some(form) = form.as_dict() else {
        return;
    };
    if !doc.resolve_key(form, doc.intern(b"XFA")).is_null() {
        out.push(UaRaw {
            rule: clauses::XFA,
            object: catalog.get_ref(doc.intern(b"AcroForm")),
            kind: FindingKind::XfaForbidden,
        });
    }
}

/// UA-2 8.14.1-1: "The Desc entry shall be present on all file
/// specification dictionaries present in the EmbeddedFiles name tree".
fn embedded_file_descriptions(doc: &CosDocument, catalog: &Dict, out: &mut Vec<UaRaw>) {
    let names = doc.resolve_key(catalog, doc.intern(b"Names"));
    let Some(names) = names.as_dict() else {
        return;
    };
    // A root written in place rather than by reference is not one the tree
    // reader takes, and this rule abstains on it rather than guessing.
    let Some(root) = names.get_ref(doc.intern(b"EmbeddedFiles")) else {
        return;
    };
    let mut reported = 0usize;
    for (_, value) in tinker_pdf_cos::trees::name_tree(doc, root) {
        if reported >= MAX_FINDINGS_PER_RULE {
            break;
        }
        let resolved = doc.resolve(&value);
        let Some(spec) = resolved.as_dict() else {
            continue;
        };
        if doc.resolve_key(spec, doc.intern(b"Desc")).is_null() {
            reported += 1;
            out.push(UaRaw {
                rule: clauses::EMBEDDED_FILE_DESCRIPTION,
                object: value.as_objref(),
                kind: FindingKind::EmbeddedFileKeyMissing {
                    key: "Desc".to_string(),
                },
            });
        }
    }
}

/// UA-1 7.16-1: "An encrypted conforming file shall contain a P key in its
/// encryption dictionary … The 10th bit position of the P key shall be
/// true" — ISO 32000-1 Table 22's bit 10, "extract text and graphics … in
/// support of accessibility to users with disabilities".
fn encryption(doc: &CosDocument, out: &mut Vec<UaRaw>) {
    let encrypt = doc.resolve_key(doc.trailer(), doc.intern(b"Encrypt"));
    let Some(encrypt) = encrypt.as_dict() else {
        return;
    };
    let permissions = doc.resolve_key(encrypt, doc.intern(b"P")).as_int();
    if permissions.is_none_or(|p| p & (1 << 9) == 0) {
        out.push(UaRaw {
            rule: clauses::SECURITY,
            object: doc.trailer().get_ref(doc.intern(b"Encrypt")),
            kind: FindingKind::AccessibilityPermissionWithheld { permissions },
        });
    }
}

/// The rules over every object: embedded file specifications under part 1,
/// and reference XObjects.
fn objects(doc: &CosDocument, part: UaPart, out: &mut Vec<UaRaw>) {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut counts = Counts::default();
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
        if let Object::Stream(stream) = object.as_ref() {
            reference_xobject(doc, &stream.dict, reference, &mut counts, out);
        }
        if part == UaPart::One {
            walk(doc, &object, reference, 0, &mut counts, out);
        }
    }
}

#[derive(Default)]
struct Counts {
    references: usize,
    names: usize,
}

/// UA-1 7.20-1: "A conforming file shall not contain any reference XObjects"
/// — a form XObject carrying `/Ref` (ISO 32000-1 8.10.4).
fn reference_xobject(
    doc: &CosDocument,
    dict: &Dict,
    at: ObjRef,
    counts: &mut Counts,
    out: &mut Vec<UaRaw>,
) {
    if counts.references >= MAX_FINDINGS_PER_RULE {
        return;
    }
    let is_form = doc
        .resolve_key(dict, doc.intern(b"Subtype"))
        .as_name()
        .and_then(|name| doc.name_bytes(name))
        .is_some_and(|name| name.as_ref() == b"Form");
    if is_form && !doc.resolve_key(dict, doc.intern(b"Ref")).is_null() {
        counts.references += 1;
        out.push(UaRaw {
            rule: clauses::REFERENCE_XOBJECTS,
            object: Some(at),
            kind: FindingKind::ReferenceXObjectForbidden,
        });
    }
}

/// Every dictionary inside one object, for the file specification rule.
fn walk(
    doc: &CosDocument,
    object: &Object,
    at: ObjRef,
    depth: u32,
    counts: &mut Counts,
    out: &mut Vec<UaRaw>,
) {
    if depth > MAX_NESTING {
        return;
    }
    match object {
        Object::Array(values) => {
            for value in values {
                walk(doc, value, at, depth + 1, counts, out);
            }
        }
        Object::Dict(dict) => {
            file_specification(doc, dict, at, counts, out);
            for (_, value) in dict.entries() {
                walk(doc, value, at, depth + 1, counts, out);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.entries() {
                walk(doc, value, at, depth + 1, counts, out);
            }
        }
        _ => {}
    }
}

/// UA-1 7.11-1: "The file specification dictionary for an embedded file
/// shall contain the non-empty F and UF keys" — a specification is one for
/// an embedded file when it carries `/EF` (ISO 32000-1 7.11.4), which is
/// veraPDF's `containsEF` too.
fn file_specification(
    doc: &CosDocument,
    dict: &Dict,
    at: ObjRef,
    counts: &mut Counts,
    out: &mut Vec<UaRaw>,
) {
    if doc.resolve_key(dict, doc.intern(b"EF")).as_dict().is_none() {
        return;
    }
    for key in ["F", "UF"] {
        if counts.names >= MAX_FINDINGS_PER_RULE {
            return;
        }
        let value = doc.resolve_key(dict, doc.intern(key.as_bytes()));
        let present = value
            .as_string()
            .is_some_and(|value| !value.bytes.is_empty());
        if !present {
            counts.names += 1;
            out.push(UaRaw {
                rule: clauses::EMBEDDED_FILE_NAMES,
                object: Some(at),
                kind: FindingKind::EmbeddedFileKeyMissing {
                    key: key.to_string(),
                },
            });
        }
    }
}
