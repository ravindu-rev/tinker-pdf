//! Name trees and number trees (7.9.6, 7.9.7).
//!
//! Both are balanced search trees whose leaves hold a flat array of key/value
//! pairs. The specification promises the keys are sorted; enough producers
//! break that promise that this walks every leaf and sorts afterwards, which
//! costs one pass and removes a whole class of "the destination exists but
//! cannot be found" bug.

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{ObjRef, Object};
use crate::warn::WarningKind;
use std::collections::HashSet;

/// Collects every entry of a name tree rooted at `root` (7.9.6).
///
/// Keys are byte strings, not text: a destination name is matched literally,
/// never after a text decoding that might normalize it.
#[must_use]
pub fn name_tree(doc: &CosDocument, root: ObjRef) -> Vec<(Vec<u8>, Object)> {
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    walk(doc, root, 0, &mut visited, &mut out, Key::Names);
    out.into_iter()
        .filter_map(|(k, v)| match k {
            KeyValue::Bytes(b) => Some((b, v)),
            KeyValue::Number(_) => None,
        })
        .collect()
}

/// Finds one entry of a name tree, descending by `/Limits` (7.9.6).
///
/// A tree exists so a lookup does not have to read all of it: each interior
/// node declares the least and greatest key beneath it, and a key outside that
/// range means the whole subtree can be skipped. Collecting every entry and
/// searching the result — the only thing available before this — turns a
/// structure designed for thousands of destinations into a linear scan of
/// them, and reads every object on the way.
///
/// A node whose `/Limits` are missing or malformed is descended into anyway.
/// The entry is likely still there, and refusing to look because the index is
/// damaged is the opposite of what the leniency ladder is for.
#[must_use]
pub fn name_tree_lookup(doc: &CosDocument, root: ObjRef, key: &[u8]) -> Option<Object> {
    let mut visited = HashSet::new();
    descend(doc, root, key, 0, &mut visited)
}

fn descend(
    doc: &CosDocument,
    node: ObjRef,
    key: &[u8],
    depth: u32,
    visited: &mut HashSet<u32>,
) -> Option<Object> {
    if depth > limits::MAX_NEST_DEPTH || !visited.insert(node.num) {
        return None;
    }

    let object = doc.get(node).ok()?;
    let dict = object.as_dict()?;

    // A leaf: /Names is a flat [key value key value] array, sorted, so the
    // key is found by comparison rather than by scanning for equality.
    if let Some(names) = doc.resolve_key(dict, doc.intern(b"Names")).as_array() {
        for pair in names.chunks_exact(2) {
            let Some(name) = doc.resolve(&pair[0]).as_string().map(|s| s.bytes.clone()) else {
                continue;
            };
            if name == key {
                return Some(doc.resolve(&pair[1]).as_ref().clone());
            }
        }
        return None;
    }

    let kids = doc.resolve_key(dict, Name::KIDS);
    let kids = kids.as_array()?;
    for kid in kids.iter().take(limits::MAX_ARRAY_LEN) {
        let Some(reference) = kid.as_objref() else {
            continue;
        };
        if !within_limits(doc, reference, key) {
            continue;
        }
        if let Some(found) = descend(doc, reference, key, depth + 1, visited) {
            return Some(found);
        }
    }
    None
}

/// Whether a node's `/Limits` admit a key.
///
/// Absent or malformed limits admit everything: the index being damaged is no
/// reason to conclude the entry is not there.
fn within_limits(doc: &CosDocument, node: ObjRef, key: &[u8]) -> bool {
    let Ok(object) = doc.get(node) else {
        return true;
    };
    let Some(dict) = object.as_dict() else {
        return true;
    };
    let value = doc.resolve_key(dict, doc.intern(b"Limits"));
    let Some(pair) = value.as_array() else {
        return true;
    };
    let (Some(low), Some(high)) = (
        pair.first()
            .and_then(|o| doc.resolve(o).as_string().map(|s| s.bytes.clone())),
        pair.get(1)
            .and_then(|o| doc.resolve(o).as_string().map(|s| s.bytes.clone())),
    ) else {
        return true;
    };
    key >= low.as_slice() && key <= high.as_slice()
}

/// Collects every entry of a number tree rooted at `root` (7.9.7).
#[must_use]
pub fn number_tree(doc: &CosDocument, root: ObjRef) -> Vec<(i64, Object)> {
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    walk(doc, root, 0, &mut visited, &mut out, Key::Nums);
    let mut entries: Vec<(i64, Object)> = out
        .into_iter()
        .filter_map(|(k, v)| match k {
            KeyValue::Number(n) => Some((n, v)),
            KeyValue::Bytes(_) => None,
        })
        .collect();
    // 7.9.7: the keys ascend. Sorting here means a producer that emits them
    // out of order still gets correct lookups.
    entries.sort_by_key(|(n, _)| *n);
    entries
}

#[derive(Clone, Copy)]
enum Key {
    Names,
    Nums,
}

enum KeyValue {
    Bytes(Vec<u8>),
    Number(i64),
}

fn walk(
    doc: &CosDocument,
    node: ObjRef,
    depth: u32,
    visited: &mut HashSet<u32>,
    out: &mut Vec<(KeyValue, Object)>,
    kind: Key,
) {
    if depth > limits::MAX_NEST_DEPTH || out.len() >= limits::MAX_TREE_ENTRIES {
        doc.warn(WarningKind::TreeTruncated);
        return;
    }
    if !visited.insert(node.num) {
        doc.warn(WarningKind::TreeCycle);
        return;
    }

    let Ok(object) = doc.get(node) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };

    // 7.9.6: an intermediate node has /Kids, a leaf has /Names or /Nums. A
    // node may legally have both when it is the root of a one-level tree.
    let leaf_key = match kind {
        Key::Names => doc.intern(b"Names"),
        Key::Nums => doc.intern(b"Nums"),
    };
    if let Some(pairs) = dict.get_array(leaf_key) {
        let pairs: Vec<Object> = pairs.to_vec();
        for pair in pairs.chunks(2) {
            let (Some(key), Some(value)) = (pair.first(), pair.get(1)) else {
                // An odd-length array: the last key has no value.
                doc.warn(WarningKind::TreeOddEntries);
                break;
            };
            let key = match (kind, key) {
                (Key::Names, Object::String(s)) => KeyValue::Bytes(s.bytes.clone()),
                (Key::Nums, Object::Int(n)) => KeyValue::Number(*n),
                _ => continue,
            };
            out.push((key, value.clone()));
        }
    }

    if let Some(kids) = dict.get_array(Name::KIDS) {
        let kids: Vec<ObjRef> = kids.iter().filter_map(Object::as_objref).collect();
        for kid in kids {
            walk(doc, kid, depth + 1, visited, out, kind);
        }
    }

    visited.remove(&node.num);
}

/// Looks one key up in a name tree.
///
/// Linear over the collected entries: a document has tens of destinations, not
/// thousands, and building an index for one lookup costs more than it saves.
#[must_use]
pub fn lookup_name(doc: &CosDocument, root: ObjRef, key: &[u8]) -> Option<Object> {
    name_tree(doc, root)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// The number-tree value governing `index`: the entry with the greatest key
/// not exceeding it (7.9.7, and how `/PageLabels` is defined to work).
#[must_use]
pub fn lookup_number_range(entries: &[(i64, Object)], index: i64) -> Option<&Object> {
    entries
        .iter()
        .rev()
        .find(|(k, _)| *k <= index)
        .map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_lookup_takes_the_greatest_key_not_over() {
        let entries = vec![
            (0i64, Object::Int(10)),
            (5, Object::Int(20)),
            (10, Object::Int(30)),
        ];
        assert_eq!(lookup_number_range(&entries, 0), Some(&Object::Int(10)));
        assert_eq!(lookup_number_range(&entries, 4), Some(&Object::Int(10)));
        assert_eq!(lookup_number_range(&entries, 5), Some(&Object::Int(20)));
        assert_eq!(lookup_number_range(&entries, 999), Some(&Object::Int(30)));
        assert_eq!(
            lookup_number_range(&entries, -1),
            None,
            "before the first range there is nothing"
        );
        assert_eq!(lookup_number_range(&[], 0), None);
    }
}
