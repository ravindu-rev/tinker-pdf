//! Name trees (7.9.6) and number trees (7.9.7).
//!
//! One module, because `/Dests`, `/EmbeddedFiles` and `/PageLabels` are the
//! same balanced-tree structure with different key types and different leaves.
//!
//! Keys are compared as raw byte strings in lexicographic order, not as UTF-8:
//! 7.9.6 says byte strings and real files carry keys that are not valid UTF-8.
//! Lookup descends by `/Limits`, and the first sign that `/Limits` lies or that
//! `/Names` is not sorted — generators that sort case-insensitively produce
//! exactly that — degrades the lookup to an exhaustive scan with a warning. A
//! lying index can cost time; it can never hide a key.

use std::collections::HashSet;
use std::sync::Arc;

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, Object};
use crate::warn::{WarningKind, WarningSink};

/// A name tree (7.9.6) rooted at a dictionary.
pub struct NameTree<'d> {
    doc: &'d CosDocument,
    root: Dict,
}

/// A number tree (7.9.7) rooted at a dictionary.
pub struct NumberTree<'d> {
    doc: &'d CosDocument,
    root: Dict,
}

/// How a node's own leaf array is spelled: `/Names` in a name tree, `/Nums` in
/// a number tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Names,
    Nums,
}

impl<'d> NameTree<'d> {
    /// The tree rooted at `root`.
    pub fn new(doc: &'d CosDocument, root: Dict) -> NameTree<'d> {
        NameTree { doc, root }
    }

    /// The value stored under `key`, or `None` when the tree does not hold it.
    pub fn get(&self, key: &[u8]) -> Option<Arc<Object>> {
        let mut sink = WarningSink::new();
        let found = descend(
            self.doc,
            &self.root,
            Flavor::Names,
            &Key::Bytes(key.to_vec()),
            &mut sink,
        );
        let found = match found {
            Some(value) => Some(value),
            // A miss is only believable once the exhaustive walk agrees.
            None => {
                let scanned = self
                    .entries_with(&mut sink)
                    .into_iter()
                    .find(|(k, _)| k.as_slice() == key)
                    .map(|(_, v)| v);
                if scanned.is_some() {
                    sink.warn(0, WarningKind::TreeLimitsLied);
                }
                scanned
            }
        };
        self.doc.absorb(sink);
        found
    }

    /// Every entry, in tree order, with cycles and caps guarded.
    pub fn entries(&self) -> Vec<(Vec<u8>, Arc<Object>)> {
        let mut sink = WarningSink::new();
        let entries = self.entries_with(&mut sink);
        self.doc.absorb(sink);
        entries
    }

    fn entries_with(&self, sink: &mut WarningSink) -> Vec<(Vec<u8>, Arc<Object>)> {
        walk(self.doc, &self.root, Flavor::Names, sink)
            .into_iter()
            .filter_map(|(k, v)| match k {
                Key::Bytes(bytes) => Some((bytes, v)),
                Key::Number(_) => None,
            })
            .collect()
    }
}

impl<'d> NumberTree<'d> {
    /// The tree rooted at `root`.
    pub fn new(doc: &'d CosDocument, root: Dict) -> NumberTree<'d> {
        NumberTree { doc, root }
    }

    /// The value stored under `key`, or `None` when the tree does not hold it.
    pub fn get(&self, key: i64) -> Option<Arc<Object>> {
        let mut sink = WarningSink::new();
        let found = descend(self.doc, &self.root, Flavor::Nums, &Key::Number(key), &mut sink);
        let found = match found {
            Some(value) => Some(value),
            None => {
                let scanned = self
                    .entries_with(&mut sink)
                    .into_iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v);
                if scanned.is_some() {
                    sink.warn(0, WarningKind::TreeLimitsLied);
                }
                scanned
            }
        };
        self.doc.absorb(sink);
        found
    }

    /// Every entry, in tree order, with cycles and caps guarded.
    pub fn entries(&self) -> Vec<(i64, Arc<Object>)> {
        let mut sink = WarningSink::new();
        let entries = self.entries_with(&mut sink);
        self.doc.absorb(sink);
        entries
    }

    fn entries_with(&self, sink: &mut WarningSink) -> Vec<(i64, Arc<Object>)> {
        walk(self.doc, &self.root, Flavor::Nums, sink)
            .into_iter()
            .filter_map(|(k, v)| match k {
                Key::Number(n) => Some((n, v)),
                Key::Bytes(_) => None,
            })
            .collect()
    }
}

/// A key of either flavor, ordered the way its tree orders it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Bytes(Vec<u8>),
    Number(i64),
}

impl Key {
    fn from_object(object: &Object, flavor: Flavor) -> Option<Key> {
        match (flavor, object) {
            // 7.9.6: name-tree keys are strings, never names.
            (Flavor::Names, Object::String(s)) => Some(Key::Bytes(s.bytes.clone())),
            (Flavor::Nums, Object::Int(n)) => Some(Key::Number(*n)),
            _ => None,
        }
    }

    fn cmp_key(&self, other: &Key) -> core::cmp::Ordering {
        match (self, other) {
            (Key::Bytes(a), Key::Bytes(b)) => a.as_slice().cmp(b.as_slice()),
            (Key::Number(a), Key::Number(b)) => a.cmp(b),
            (Key::Bytes(_), Key::Number(_)) => core::cmp::Ordering::Less,
            (Key::Number(_), Key::Bytes(_)) => core::cmp::Ordering::Greater,
        }
    }
}

/// The `/Names` or `/Nums` array of one node, resolved.
fn leaf_array(doc: &CosDocument, node: &Dict, flavor: Flavor) -> Option<Arc<Object>> {
    let key = match flavor {
        Flavor::Names => doc.names.sem.names,
        Flavor::Nums => doc.names.sem.nums,
    };
    let value = doc.resolve(node.get(key)?);
    value.as_array().is_some().then_some(value)
}

/// The `[low, high]` bounds a node claims (7.9.6), when it claims usable ones.
fn limits_of(doc: &CosDocument, node: &Dict, flavor: Flavor) -> Option<(Key, Key)> {
    let value = doc.resolve(node.get(doc.names.sem.limits)?);
    let items = value.as_array()?;
    let low = Key::from_object(&doc.resolve(items.first()?), flavor)?;
    let high = Key::from_object(&doc.resolve(items.get(1)?), flavor)?;
    Some((low, high))
}

/// The pairs of one node's leaf array. An odd length drops the trailing key.
fn pairs(
    doc: &CosDocument,
    node: &Dict,
    flavor: Flavor,
    sink: &mut WarningSink,
) -> Vec<(Key, Arc<Object>)> {
    let Some(array) = leaf_array(doc, node, flavor) else {
        return Vec::new();
    };
    let Some(items) = array.as_array() else {
        return Vec::new();
    };
    if items.len() % 2 != 0 {
        sink.warn(0, WarningKind::TreeNamesOddLength);
    }
    let mut out = Vec::with_capacity(items.len() / 2);
    let mut chunks = items.chunks_exact(2);
    for pair in chunks.by_ref() {
        if out.len() >= limits::MAX_TREE_ENTRIES {
            sink.warn(0, WarningKind::TreeCapHit);
            break;
        }
        if let [key, value] = pair {
            if let Some(key) = Key::from_object(&doc.resolve(key), flavor) {
                out.push((key, doc.resolve(value)));
            }
        }
    }
    out
}

/// The `/Kids` of one node, resolved to dictionaries, with their object
/// numbers for the cycle guard.
fn kids(doc: &CosDocument, node: &Dict) -> Vec<(Option<u32>, Dict)> {
    let Some(kids) = node.get(Name::KIDS) else {
        return Vec::new();
    };
    let kids = doc.resolve(kids);
    let Some(items) = kids.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(items.len().min(limits::MAX_TREE_NODES));
    for item in items.iter().take(limits::MAX_TREE_NODES) {
        let num = item.as_objref().map(|r| r.num);
        if let Some(dict) = doc.resolve(item).as_dict() {
            out.push((num, dict.clone()));
        }
    }
    out
}

/// `/Limits`-guided descent (7.9.6). `None` means "not found here", which the
/// caller is expected to double-check with [`walk`].
fn descend(
    doc: &CosDocument,
    root: &Dict,
    flavor: Flavor,
    key: &Key,
    sink: &mut WarningSink,
) -> Option<Arc<Object>> {
    let mut node = root.clone();
    let mut visited: HashSet<u32> = HashSet::new();
    for _ in 0..limits::MAX_TREE_DEPTH {
        // A leaf array is searched linearly: an unsorted /Names array is
        // common enough that a binary search here would be a silent miss.
        for (candidate, value) in pairs(doc, &node, flavor, sink) {
            if candidate == *key {
                return Some(value);
            }
        }
        let mut next: Option<Dict> = None;
        for (num, kid) in kids(doc, &node) {
            if let Some(num) = num {
                if visited.contains(&num) {
                    sink.warn(0, WarningKind::TreeCycle);
                    continue;
                }
            }
            let inside = match limits_of(doc, &kid, flavor) {
                Some((low, high)) => {
                    key.cmp_key(&low) != core::cmp::Ordering::Less
                        && key.cmp_key(&high) != core::cmp::Ordering::Greater
                }
                // A kid with no usable /Limits cannot be excluded.
                None => true,
            };
            if inside {
                if let Some(num) = num {
                    visited.insert(num);
                }
                next = Some(kid);
                break;
            }
        }
        node = next?;
    }
    sink.warn(0, WarningKind::TreeCapHit);
    None
}

/// The exhaustive, cycle-guarded walk. Tree order, so a caller that wants
/// sorted output of an unsorted tree sorts it itself.
fn walk(
    doc: &CosDocument,
    root: &Dict,
    flavor: Flavor,
    sink: &mut WarningSink,
) -> Vec<(Key, Arc<Object>)> {
    let mut out: Vec<(Key, Arc<Object>)> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut nodes = 0usize;
    // Depth is carried per frame so one deep branch cannot borrow the budget
    // of the branches beside it.
    let mut stack: Vec<(Dict, u32)> = vec![(root.clone(), 0)];
    while let Some((node, depth)) = stack.pop() {
        nodes += 1;
        if nodes > limits::MAX_TREE_NODES {
            sink.warn(0, WarningKind::TreeCapHit);
            break;
        }
        for entry in pairs(doc, &node, flavor, sink) {
            if out.len() >= limits::MAX_TREE_ENTRIES {
                sink.warn(0, WarningKind::TreeCapHit);
                return out;
            }
            out.push(entry);
        }
        if depth + 1 >= limits::MAX_TREE_DEPTH {
            if !kids(doc, &node).is_empty() {
                sink.warn(0, WarningKind::TreeCapHit);
            }
            continue;
        }
        let children = kids(doc, &node);
        for (num, kid) in children.into_iter().rev() {
            if let Some(num) = num {
                if !visited.insert(num) {
                    sink.warn(0, WarningKind::TreeCycle);
                    continue;
                }
            }
            stack.push((kid, depth + 1));
        }
    }
    out
}
