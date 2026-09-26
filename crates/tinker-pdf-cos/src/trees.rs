//! Name trees and number trees (7.9.6, 7.9.7).
//!
//! Both are balanced search trees whose leaves hold a flat array of key/value
//! pairs. The specification promises the keys are sorted; enough producers
//! break that promise that this walks every leaf and sorts afterwards, which
//! costs one pass and removes a whole class of "the destination exists but
//! cannot be found" bug.
//!
//! The writers are here too, beside the readers they must satisfy:
//! [`write_name_tree`] and [`write_number_tree`] sort, split into leaves under
//! `/Kids` with `/Limits`, and refuse what the readers above would truncate.

use crate::doc::CosDocument;
use crate::limits;
use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::resolve::Resolve;
use crate::warn::WarningKind;
use std::collections::HashSet;

/// Collects every entry of a name tree rooted at `root` (7.9.6).
///
/// Keys are byte strings, not text: a destination name is matched literally,
/// never after a text decoding that might normalize it.
#[must_use]
pub fn name_tree(doc: &CosDocument, root: ObjRef) -> Vec<(Vec<u8>, Object)> {
    name_tree_in(doc, root)
}

/// [`name_tree`] through a [`Resolve`] view — an editor reading a tree it
/// has written or changed and not yet saved.
#[must_use]
pub fn name_tree_in<R: Resolve + ?Sized>(doc: &R, root: ObjRef) -> Vec<(Vec<u8>, Object)> {
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
    name_tree_lookup_in(doc, root, key)
}

/// [`name_tree_lookup`] through a [`Resolve`] view.
#[must_use]
pub fn name_tree_lookup_in<R: Resolve + ?Sized>(
    doc: &R,
    root: ObjRef,
    key: &[u8],
) -> Option<Object> {
    let mut visited = HashSet::new();
    descend(doc, root, key, 0, &mut visited)
}

fn descend<R: Resolve + ?Sized>(
    doc: &R,
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
fn within_limits<R: Resolve + ?Sized>(doc: &R, node: ObjRef, key: &[u8]) -> bool {
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
    number_tree_in(doc, root)
}

/// [`number_tree`] through a [`Resolve`] view.
#[must_use]
pub fn number_tree_in<R: Resolve + ?Sized>(doc: &R, root: ObjRef) -> Vec<(i64, Object)> {
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

fn walk<R: Resolve + ?Sized>(
    doc: &R,
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

/// Entries in one leaf of a tree this module writes, and kids in one of its
/// intermediate nodes.
///
/// 7.9.6 leaves the shape to the writer. Sixty-four keeps a leaf a few
/// kilobytes for ordinary keys, and puts the reader's whole budget of
/// [`limits::MAX_TREE_ENTRIES`] three levels below the root: 4 096 leaves
/// under 64 intermediate nodes. A layout choice, not a limit on input, so it
/// is not public.
const TREE_FANOUT: usize = 64;

/// Why a name or number tree was not written.
///
/// Every variant means **nothing was added**: the entries are checked before
/// the first node is handed to the caller's sink, so a refusal leaves no
/// orphaned leaves behind.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TreeWriteError {
    /// Two entries carry this name. 7.9.6 maps each key to one value, and a
    /// reader given two finds whichever its search happens to reach first, so
    /// which one the caller meant is a decision for the caller, not a policy
    /// to hide here.
    DuplicateName(Vec<u8>),
    /// Two entries carry this number (7.9.7), for the same reason.
    DuplicateNumber(i64),
    /// More entries than [`limits::MAX_TREE_ENTRIES`], past which this
    /// repository's own reader stops and warns. Writing a tree that reads back
    /// truncated would be a file that looks complete and is not.
    TooManyEntries(usize),
}

impl core::fmt::Display for TreeWriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TreeWriteError::DuplicateName(key) => {
                write!(
                    f,
                    "the name ({}) appears twice",
                    String::from_utf8_lossy(key)
                )
            }
            TreeWriteError::DuplicateNumber(key) => write!(f, "the number {key} appears twice"),
            TreeWriteError::TooManyEntries(count) => write!(
                f,
                "{count} entries, more than the {} a reader walks",
                limits::MAX_TREE_ENTRIES
            ),
        }
    }
}

impl std::error::Error for TreeWriteError {}

/// Writes a name tree (7.9.6) holding `entries`, and returns its root.
///
/// Keys are byte strings and are sorted by their bytes, which is the order
/// 7.9.6 requires and the one [`name_tree_lookup`] compares `/Limits` in.
///
/// Every node is handed to `add`, which stores it as an indirect object and
/// returns the reference: the way both writers in this crate hold objects, so
/// `DocumentEditor::add_name_tree` passes its `allocate` and `put`, and a
/// caller assembling an [`crate::write::ObjectSet`] passes a counter and
/// `insert`. Children are added before the nodes that list them.
///
/// The shape (Table 36): up to 64 entries are one root node with `/Names`.
/// More are leaves of 64 with `/Names` and `/Limits`, gathered under
/// intermediate nodes of up to 64 `/Kids` with `/Limits`, as many levels as
/// it takes, under a root with `/Kids` alone — the root is the one node Table
/// 36 gives no `/Limits`. The root is indirect as well, because the readers
/// here take a tree by reference.
///
/// # Errors
///
/// [`TreeWriteError`], before anything is added: a key given twice, or more
/// entries than [`limits::MAX_TREE_ENTRIES`], past which the reader stops.
pub fn write_name_tree(
    entries: Vec<(Vec<u8>, Object)>,
    names: &NameTable,
    add: impl FnMut(Object) -> ObjRef,
) -> Result<ObjRef, TreeWriteError> {
    if entries.len() > limits::MAX_TREE_ENTRIES {
        return Err(TreeWriteError::TooManyEntries(entries.len()));
    }
    let mut entries = entries;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(key) = entries.windows(2).find_map(|pair| match pair {
        [a, b] if a.0 == b.0 => Some(a.0.clone()),
        _ => None,
    }) {
        return Err(TreeWriteError::DuplicateName(key));
    }
    let keyed = entries
        .into_iter()
        .map(|(key, value)| (Object::String(PdfString::literal(key)), value))
        .collect();
    Ok(write_tree(keyed, names.intern(b"Names"), names, add))
}

/// Writes a number tree (7.9.7) holding `entries`, and returns its root.
///
/// The same shape and the same `add` as [`write_name_tree`], with integer
/// keys in ascending numeric order under `/Nums`.
///
/// # Errors
///
/// As [`write_name_tree`].
pub fn write_number_tree(
    entries: Vec<(i64, Object)>,
    names: &NameTable,
    add: impl FnMut(Object) -> ObjRef,
) -> Result<ObjRef, TreeWriteError> {
    if entries.len() > limits::MAX_TREE_ENTRIES {
        return Err(TreeWriteError::TooManyEntries(entries.len()));
    }
    let mut entries = entries;
    entries.sort_by_key(|entry| entry.0);
    if let Some(key) = entries.windows(2).find_map(|pair| match pair {
        [a, b] if a.0 == b.0 => Some(a.0),
        _ => None,
    }) {
        return Err(TreeWriteError::DuplicateNumber(key));
    }
    let keyed = entries
        .into_iter()
        .map(|(key, value)| (Object::Int(key), value))
        .collect();
    Ok(write_tree(keyed, names.intern(b"Nums"), names, add))
}

/// One node already added, with the least and greatest key beneath it.
struct Placed {
    reference: ObjRef,
    least: Object,
    greatest: Object,
}

/// The shape both trees share, over keys already sorted and unique.
fn write_tree(
    entries: Vec<(Object, Object)>,
    leaf_key: Name,
    names: &NameTable,
    mut add: impl FnMut(Object) -> ObjRef,
) -> ObjRef {
    let pairs = |chunk: Vec<(Object, Object)>| -> Vec<Object> {
        chunk.into_iter().flat_map(|(k, v)| [k, v]).collect()
    };
    if entries.len() <= TREE_FANOUT {
        let mut root = Dict::new();
        root.insert(leaf_key, Object::Array(pairs(entries)));
        return add(Object::Dict(root));
    }

    let limits_key = names.intern(b"Limits");
    let node = |kind: Name, body: Vec<Object>, least: &Object, greatest: &Object| -> Object {
        let mut dict = Dict::new();
        dict.insert(kind, Object::Array(body));
        dict.insert(
            limits_key,
            Object::Array(vec![least.clone(), greatest.clone()]),
        );
        Object::Dict(dict)
    };

    let mut level: Vec<Placed> = Vec::new();
    let mut rest = entries.into_iter();
    loop {
        let chunk: Vec<(Object, Object)> = rest.by_ref().take(TREE_FANOUT).collect();
        let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else {
            break;
        };
        let (least, greatest) = (first.0.clone(), last.0.clone());
        let leaf = node(leaf_key, pairs(chunk), &least, &greatest);
        level.push(Placed {
            reference: add(leaf),
            least,
            greatest,
        });
    }

    while level.len() > TREE_FANOUT {
        let mut above = Vec::with_capacity(level.len().div_ceil(TREE_FANOUT));
        for group in level.chunks(TREE_FANOUT) {
            let (Some(first), Some(last)) = (group.first(), group.last()) else {
                continue;
            };
            let kids = group.iter().map(|w| Object::Ref(w.reference)).collect();
            let interior = node(Name::KIDS, kids, &first.least, &last.greatest);
            above.push(Placed {
                reference: add(interior),
                least: first.least.clone(),
                greatest: last.greatest.clone(),
            });
        }
        level = above;
    }

    let mut root = Dict::new();
    root.insert(
        Name::KIDS,
        Object::Array(level.iter().map(|w| Object::Ref(w.reference)).collect()),
    );
    add(Object::Dict(root))
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
