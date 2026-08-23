//! Reading a document back the way an outside reader would have to.
//!
//! The deleted qpdf oracles asked another program what a synthesised document
//! says. Ruling 13 retires that, and what replaces it is this: the same
//! questions asked of this engine's own object model, but asked of the
//! *dictionaries* rather than of the typed readers above them.
//!
//! That distinction is the whole value. `Document::page` hands back a page
//! whose `/MediaBox` may have been invented, whose `/Rect` has been normalised
//! and whose missing `/Extend` has been defaulted — every one of those is a
//! mistake the writer is allowed to make and the reader will hide. Nothing
//! here goes through them: the page tree is walked from the catalog's own
//! `/Kids`, and every value is read out of a [`Dict`] as the file spells it.
//!
//! What it cannot do is what left with the oracles: this is still this
//! project's reader, so a clause misread here and in the writer agrees with
//! itself. `docs/verification.md` says so in its own voice.

#![allow(dead_code)]

use std::sync::Arc;

use tinker_pdf::{CosDocument, Dict, Name, ObjRef, Object};

/// Every page object, in page order, walked from the catalog.
///
/// Deliberately not `Document::page_count` or the `pages` module: those repair
/// a broken tree, assume US Letter for a missing box and cut a cycle. A test
/// that used them would be asking the tolerant reader whether the writer was
/// tolerable.
pub fn pages(doc: &CosDocument) -> Vec<(ObjRef, Dict)> {
    let mut out = Vec::new();
    let Some(catalog) = doc.catalog() else {
        return out;
    };
    let Some(root) = catalog.get_ref(Name::PAGES) else {
        return out;
    };
    walk(doc, root, 0, &mut out);
    out
}

fn walk(doc: &CosDocument, node: ObjRef, depth: u32, out: &mut Vec<(ObjRef, Dict)>) {
    if depth > 32 {
        return;
    }
    let Ok(object) = doc.get(node) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };
    let kind = dict.get_name(Name::TYPE).and_then(|n| doc.name_bytes(n));
    if kind.as_deref() == Some(&b"Page"[..]) {
        out.push((node, dict.clone()));
        return;
    }
    let Some(kids) = dict.get_array(Name::KIDS).map(<[Object]>::to_vec) else {
        return;
    };
    for kid in kids {
        if let Some(kid) = kid.as_objref() {
            walk(doc, kid, depth + 1, out);
        }
    }
}

/// The value under a key, resolved but not interpreted.
pub fn value(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Arc<Object> {
    doc.resolve_key(dict, doc.intern(key))
}

/// Whether a key is present at all.
///
/// 7.7.3.3 makes "absent" and "equal to the default" different documents, and
/// this is how a test says which one it wants.
pub fn has(doc: &CosDocument, dict: &Dict, key: &[u8]) -> bool {
    dict.contains_key(doc.intern(key))
}

/// A name's bytes, for comparing against a literal.
pub fn name(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    dict.get_name(doc.intern(key))
        .and_then(|n| doc.name_bytes(n))
        .map(|bytes| bytes.to_vec())
}

/// An array of numbers, as the file spells it — never normalised.
pub fn numbers(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<f64>> {
    value(doc, dict, key)
        .as_array()?
        .iter()
        .map(Object::as_number)
        .collect()
}

/// One named resource of a page, resolved.
pub fn resource(
    doc: &CosDocument,
    page: &Dict,
    category: &[u8],
    entry: &[u8],
) -> Option<(ObjRef, Arc<Object>)> {
    let resources = value(doc, page, b"Resources");
    let resources = resources.as_dict()?;
    let group = value(doc, resources, category);
    let group = group.as_dict()?;
    let reference = group.get_ref(doc.intern(entry));
    let object = doc.resolve_key(group, doc.intern(entry));
    Some((reference?, object))
}

/// Every entry of one resource category, by name.
pub fn category(doc: &CosDocument, page: &Dict, category: &[u8]) -> Vec<(Vec<u8>, Arc<Object>)> {
    let resources = value(doc, page, b"Resources");
    let Some(resources) = resources.as_dict() else {
        return Vec::new();
    };
    let group = value(doc, resources, category);
    let Some(group) = group.as_dict() else {
        return Vec::new();
    };
    group
        .entries()
        .iter()
        .map(|(key, _)| {
            let bytes = doc.name_bytes(*key).map(|b| b.to_vec()).unwrap_or_default();
            (bytes, doc.resolve_key(group, *key))
        })
        .collect()
}

/// A dictionary as one line of text, for a failure message.
///
/// Only ever for the message: an assertion that matched on this would be
/// asserting against this crate's own spelling of a value rather than against
/// the value.
pub fn flat(doc: &CosDocument, object: &Object) -> String {
    let mut out = String::new();
    write(doc, object, &mut out, 0);
    out
}

fn write(doc: &CosDocument, object: &Object, out: &mut String, depth: u32) {
    if depth > 8 {
        out.push_str("...");
        return;
    }
    match object {
        Object::Null => out.push_str("null"),
        Object::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Object::Int(value) => out.push_str(&value.to_string()),
        Object::Real(value) => out.push_str(&value.to_string()),
        Object::String(value) => {
            out.push('(');
            out.push_str(&String::from_utf8_lossy(&value.bytes));
            out.push(')');
        }
        Object::Name(value) => {
            out.push('/');
            out.push_str(&String::from_utf8_lossy(
                &doc.name_bytes(*value).unwrap_or_default(),
            ));
        }
        Object::Array(values) => {
            out.push_str("[ ");
            for value in values {
                write(doc, value, out, depth + 1);
                out.push(' ');
            }
            out.push(']');
        }
        Object::Dict(dict) | Object::Stream(tinker_pdf::StreamObj { dict, .. }) => {
            out.push_str("<< ");
            for (key, value) in dict.entries() {
                out.push('/');
                out.push_str(&String::from_utf8_lossy(
                    &doc.name_bytes(*key).unwrap_or_default(),
                ));
                out.push(' ');
                write(doc, value, out, depth + 1);
                out.push(' ');
            }
            out.push_str(">>");
        }
        Object::Ref(reference) => {
            out.push_str(&format!("{} {} R", reference.num, reference.gen));
        }
    }
}
