//! Name and number trees written by this crate read back through its own
//! readers (7.9.6, 7.9.7).
//!
//! The readers are the adjudicators here, and they were written first and
//! against other producers' files: `name_tree` and `number_tree` walk every
//! leaf, `name_tree_lookup` descends by `/Limits` and skips a subtree whose
//! range excludes the key. A tree whose `/Limits` were wrong would still read
//! back whole through the first two and lose entries through the third, so
//! every entry is looked up as well as collected.

use std::sync::Arc;

use proptest::prelude::*;
use tinker_pdf_cos::limits::MAX_TREE_ENTRIES;
use tinker_pdf_cos::{
    name_tree, name_tree_lookup, number_tree, CosDocument, DocumentBuilder, DocumentEditor, Name,
    ObjRef, Object, TreeWriteError, WriteMode, WriteOptions,
};

fn blank() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 100.0, |_| {});
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

/// Saves the editor and reopens the result, so what is read is what was
/// written rather than what the overlay holds.
fn reopen(editor: &DocumentEditor) -> CosDocument {
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    let doc = CosDocument::open(bytes).expect("the save reopens");
    assert!(doc.warnings().is_empty(), "{:?}", doc.warnings());
    doc
}

/// How deep the tree below `root` goes: 1 for a root that is its own leaf.
fn depth(doc: &CosDocument, root: ObjRef) -> usize {
    let object = doc.get(root).expect("the node loads");
    let dict = object.as_dict().expect("a node is a dictionary");
    match dict.get_array(Name::KIDS).and_then(|kids| kids.first()) {
        Some(first) => 1 + depth(doc, first.as_objref().expect("a kid is indirect")),
        None => 1,
    }
}

fn names_round_trip(keys: &[Vec<u8>]) -> (CosDocument, ObjRef) {
    let mut editor = DocumentEditor::new(blank());
    let entries = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), Object::Int(i as i64)))
        .collect();
    let root = editor.add_name_tree(entries).expect("the tree is written");
    (reopen(&editor), root)
}

/// A tree key as the specification orders it: bytes for a name tree (7.9.6),
/// integers for a number tree (7.9.7).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Bytes(Vec<u8>),
    Number(i64),
}

fn key_of(object: &Object) -> Key {
    match object {
        Object::String(s) => Key::Bytes(s.bytes.clone()),
        Object::Int(n) => Key::Number(*n),
        other => panic!("a key is a string or an integer, not {other:?}"),
    }
}

/// Walks the tree as written and checks what the readers do not: that every
/// leaf ascends strictly, that every node but the root states `/Limits`
/// equal to the least and greatest key beneath it, and that siblings are in
/// order. `number_tree` sorts whatever it reads, so without this a number
/// tree written in the wrong order would read back perfectly.
fn assert_well_formed(doc: &CosDocument, node: ObjRef, leaf: &[u8], root: bool) -> (Key, Key) {
    let object = doc.get(node).expect("the node loads");
    let dict = object.as_dict().expect("a node is a dictionary");
    let limits = dict.get_array(doc.intern(b"Limits"));
    let range = if let Some(pairs) = dict.get_array(doc.intern(leaf)) {
        assert!(dict.get(Name::KIDS).is_none(), "a leaf has no /Kids");
        let keys: Vec<Key> = pairs.chunks(2).map(|pair| key_of(&pair[0])).collect();
        assert!(
            keys.windows(2).all(|w| w[0] < w[1]),
            "a leaf's keys ascend strictly"
        );
        match (keys.first(), keys.last()) {
            (Some(first), Some(last)) => (first.clone(), last.clone()),
            // Only an empty tree has an empty leaf, and it is the root.
            _ => {
                assert!(root, "only the root may be empty");
                return (Key::Number(0), Key::Number(0));
            }
        }
    } else {
        let kids = dict.get_array(Name::KIDS).expect("a node has /Kids");
        let ranges: Vec<(Key, Key)> = kids
            .iter()
            .map(|kid| assert_well_formed(doc, kid.as_objref().expect("indirect"), leaf, false))
            .collect();
        assert!(
            ranges.windows(2).all(|w| w[0].1 < w[1].0),
            "siblings are in key order and do not overlap"
        );
        let first = ranges.first().expect("a node has a kid").0.clone();
        let last = ranges.last().expect("a node has a kid").1.clone();
        (first, last)
    };
    if root {
        assert!(limits.is_none(), "Table 36: the root has no /Limits");
    } else {
        let limits = limits.expect("every other node states /Limits");
        assert_eq!(limits.len(), 2);
        assert_eq!(
            (key_of(&limits[0]), key_of(&limits[1])),
            range,
            "/Limits bound exactly what is beneath"
        );
    }
    range
}

fn check_names(keys: &[Vec<u8>]) {
    let (doc, root) = names_round_trip(keys);
    let mut expected: Vec<(Vec<u8>, Object)> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), Object::Int(i as i64)))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_well_formed(&doc, root, b"Names", true);
    assert_eq!(name_tree(&doc, root), expected, "collected in key order");
    for (key, value) in &expected {
        assert_eq!(
            name_tree_lookup(&doc, root, key).as_ref(),
            Some(value),
            "found by /Limits: {key:?}"
        );
    }
    assert_eq!(
        name_tree_lookup(&doc, root, b"\xFF\xFF\xFF\xFF-absent"),
        None
    );
    assert!(doc.warnings().is_empty(), "{:?}", doc.warnings());
}

fn check_numbers(keys: &[i64]) {
    let mut editor = DocumentEditor::new(blank());
    let entries = keys
        .iter()
        .map(|&key| (key, Object::Int(key.wrapping_mul(3))))
        .collect();
    let root = editor
        .add_number_tree(entries)
        .expect("the tree is written");
    let doc = reopen(&editor);
    let mut expected: Vec<(i64, Object)> = keys
        .iter()
        .map(|&key| (key, Object::Int(key.wrapping_mul(3))))
        .collect();
    expected.sort_by_key(|entry| entry.0);
    assert_well_formed(&doc, root, b"Nums", true);
    assert_eq!(number_tree(&doc, root), expected);
    assert!(doc.warnings().is_empty(), "{:?}", doc.warnings());
}

fn numbered_keys(count: usize) -> Vec<Vec<u8>> {
    // Written out of order on purpose: the writer sorts.
    (0..count)
        .rev()
        .map(|i| format!("dest-{i:06}").into_bytes())
        .collect()
}

/// The sizes at which the shape changes: one leaf, one leaf too many, a full
/// second level, and one past it.
#[test]
fn every_shape_reads_back() {
    for (count, levels) in [(0, 1), (1, 1), (64, 1), (65, 2), (4096, 2), (4097, 3)] {
        let keys = numbered_keys(count);
        check_names(&keys);
        let (doc, root) = names_round_trip(&keys);
        assert_eq!(depth(&doc, root), levels, "{count} entries");

        let numbers: Vec<i64> = (0..count as i64).map(|n| n * 7 - 1000).collect();
        check_numbers(&numbers);
    }
}

/// Table 36: the root has no `/Limits`, every other node has them, and they
/// bound what is beneath.
#[test]
fn limits_are_on_every_node_but_the_root() {
    let (doc, root) = names_round_trip(&numbered_keys(200));
    let limits = doc.intern(b"Limits");
    let object = doc.get(root).expect("the root");
    let dict = object.as_dict().expect("a dictionary");
    assert!(dict.get(limits).is_none(), "the root carries no /Limits");
    assert!(dict.get(doc.intern(b"Names")).is_none(), "and not both");
    let kids = dict.get_array(Name::KIDS).expect("kids");
    assert_eq!(kids.len(), 4, "200 entries are four leaves of up to 64");
    let first = doc
        .get(kids[0].as_objref().expect("indirect"))
        .expect("a leaf");
    let bounds = first
        .as_dict()
        .and_then(|d| d.get_array(limits))
        .expect("a leaf has /Limits");
    let bound = |i: usize| bounds[i].as_string().map(|s| s.bytes.clone());
    assert_eq!(bound(0), Some(b"dest-000000".to_vec()));
    assert_eq!(bound(1), Some(b"dest-000063".to_vec()));
}

/// Duplicate keys are refused, naming the key, and nothing is written.
///
/// 7.9.6 maps a key to one value; a reader handed two finds whichever its
/// search reaches first. Which one the caller meant is theirs to decide.
#[test]
fn a_duplicate_key_is_refused_and_writes_nothing() {
    let mut editor = DocumentEditor::new(blank());
    let entries = vec![
        (b"b".to_vec(), Object::Int(1)),
        (b"a".to_vec(), Object::Int(2)),
        (b"b".to_vec(), Object::Int(3)),
    ];
    assert_eq!(
        editor.add_name_tree(entries),
        Err(TreeWriteError::DuplicateName(b"b".to_vec()))
    );
    assert_eq!(
        editor.add_number_tree(vec![(5, Object::Null), (5, Object::Null)]),
        Err(TreeWriteError::DuplicateNumber(5))
    );
    assert!(!editor.is_dirty(), "a refused tree adds no node");
}

/// Past the reader's cap the tree would read back truncated, so it is not
/// written; at the cap it reads back whole.
#[test]
fn the_readers_cap_is_the_writers() {
    let mut editor = DocumentEditor::new(blank());
    let over: Vec<(i64, Object)> = (0..=MAX_TREE_ENTRIES as i64)
        .map(|n| (n, Object::Null))
        .collect();
    assert_eq!(
        editor.add_number_tree(over),
        Err(TreeWriteError::TooManyEntries(MAX_TREE_ENTRIES + 1))
    );
    assert!(!editor.is_dirty());

    let at: Vec<i64> = (0..MAX_TREE_ENTRIES as i64).collect();
    check_numbers(&at);
}

fn key() -> impl Strategy<Value = Vec<u8>> {
    // The full byte range, and the four bytes a literal string must escape.
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..8),
        proptest::collection::vec(
            prop_oneof![
                Just(b'('),
                Just(b')'),
                Just(b'\\'),
                Just(b'\r'),
                any::<u8>()
            ],
            0..5
        ),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Write then read is the identity, over sizes that force `/Kids`, with
    /// the keys handed over in no particular order.
    #[test]
    fn a_name_tree_reads_back_as_written(
        keys in proptest::collection::btree_set(key(), 0..700),
        turn in any::<usize>(),
    ) {
        let mut keys: Vec<Vec<u8>> = keys.into_iter().collect();
        if !keys.is_empty() {
            let turn = turn % keys.len();
            keys.rotate_left(turn);
        }
        check_names(&keys);
    }

    #[test]
    fn a_number_tree_reads_back_as_written(
        keys in proptest::collection::btree_set(any::<i64>(), 0..5000)
    ) {
        let keys: Vec<i64> = keys.into_iter().rev().collect();
        check_numbers(&keys);
    }

    /// Any key given twice is refused, and the key named is one that was.
    #[test]
    fn a_repeated_key_is_always_refused(
        keys in proptest::collection::vec(key(), 1..200),
        again in any::<usize>(),
    ) {
        let mut keys = keys;
        let repeated = keys[again % keys.len()].clone();
        keys.push(repeated);
        let mut editor = DocumentEditor::new(blank());
        let entries = keys.iter().map(|k| (k.clone(), Object::Null)).collect();
        match editor.add_name_tree(entries) {
            Err(TreeWriteError::DuplicateName(named)) => {
                prop_assert!(keys.iter().filter(|k| **k == named).count() >= 2);
            }
            other => prop_assert!(false, "not refused: {:?}", other),
        }
        prop_assert!(!editor.is_dirty());
    }
}
