//! The crate's sorts, compiled once.
//!
//! `<[T]>::sort_by` is generic in its comparison as well as in its items, and
//! every closure is a type of its own, so each call compiles the standard
//! library's whole sort again for the items and the comparison it is given,
//! and a crate that sorts in many places carries the sort as many times. In
//! the wasm module those copies are bytes every page that loads the engine
//! downloads (ruling 19). These functions put the items in the order the
//! comparison gives by sorting their indices — one sort, compiled once for
//! the crate, its comparison reached through a pointer — and then moving each
//! item to its place. The order is the one `<[T]>::sort_by` gives: the sort is
//! stable, so items that compare equal keep the order they had.
//!
//! Collecting into a `BTreeMap` or a `BTreeSet` sorts too: the standard
//! library sorts the items to build the tree in one pass, and compiles that
//! sort again for every item type collected. [`map`] and [`set`] insert the
//! items one by one instead, which needs no sort, into the tree collecting
//! makes.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Sorts `items` by `compare`, stably: what `<[T]>::sort_by` does.
pub(crate) fn by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    if items.len() < 2 {
        return;
    }
    let mut order = order(items.len(), &mut |a, b| compare(&items[a], &items[b]));
    permute(items, &mut order);
}

/// Sorts `items` by the key `key` gives each, stably: what
/// `<[T]>::sort_by_key` does, the key taken for each comparison as it is
/// there.
pub(crate) fn by_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
    by(items, |a, b| key(a).cmp(&key(b)));
}

/// Sorts `items` in their own order, stably: what `<[T]>::sort` does.
pub(crate) fn ascending<T: Ord>(items: &mut [T]) {
    by(items, T::cmp);
}

/// The pairs as a map, each inserted in turn: the map collecting them makes,
/// a key met again holding the value it was met with last. Collecting would
/// keep the last of equal keys too, where inserting keeps the first; every
/// key this crate builds a map of is equal only to itself, so the two are
/// the same map.
pub(crate) fn map<K: Ord, V>(pairs: impl IntoIterator<Item = (K, V)>) -> BTreeMap<K, V> {
    let mut map = BTreeMap::new();
    map.extend(pairs);
    map
}

/// The items as a set, each inserted in turn: the set collecting them makes.
/// Collecting keeps the last of equal items, inserting the first; every item
/// this crate builds a set of is equal only to itself, so the two are the
/// same set.
pub(crate) fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    let mut set = BTreeSet::new();
    set.extend(items);
    set
}

/// The indices `0..len` in the order `compare` puts the items they stand for,
/// stably: the one sort the crate compiles. Every index it hands `compare` is
/// below `len`, so [`by`]'s indexing cannot miss.
fn order(len: usize, compare: &mut dyn FnMut(usize, usize) -> Ordering) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    order.sort_by(|&a, &b| compare(a, b));
    order
}

/// Moves the item at `order[i]` to `i`, for every `i`: each cycle of the
/// permutation followed once, an index set to itself as its item lands.
/// `order` is a permutation of `0..items.len()`, so no index misses.
fn permute<T>(items: &mut [T], order: &mut [usize]) {
    for start in 0..order.len() {
        let mut at = start;
        loop {
            let from = order[at];
            order[at] = at;
            if from == start {
                break;
            }
            items.swap(at, from);
            at = from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The order is the standard library's**, stability included: items
    /// that compare equal stay in the order they had, for every length up to
    /// sixty-four and keys that collide often, against `<[T]>::sort_by`.
    #[test]
    fn the_order_is_the_standard_sorts() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for len in 0..64usize {
            let items: Vec<(u8, usize)> = (0..len)
                .map(|at| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    ((seed % 5) as u8, at)
                })
                .collect();
            let mut want = items.clone();
            want.sort_by_key(|item| item.0);
            let mut got = items.clone();
            by(&mut got, |a, b| a.0.cmp(&b.0));
            assert_eq!(got, want, "by, {len} items");
            let mut got = items.clone();
            by_key(&mut got, |item| std::cmp::Reverse(item.0));
            let mut want = items.clone();
            want.sort_by_key(|item| std::cmp::Reverse(item.0));
            assert_eq!(got, want, "by_key, {len} items");
            let mut got = items.clone();
            ascending(&mut got);
            let mut want = items;
            want.sort();
            assert_eq!(got, want, "ascending, {len} items");
        }
    }

    /// **A map or a set built item by item is the one collecting builds**: a
    /// key met twice holds its last value, and the order is the keys'.
    #[test]
    fn maps_and_sets_are_what_collecting_makes() {
        let pairs = [(3u32, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
        let want: BTreeMap<u32, char> = pairs.into_iter().collect();
        assert_eq!(map(pairs), want);
        let items = [5u16, 1, 5, 3, 1, 9];
        let want: BTreeSet<u16> = items.into_iter().collect();
        assert_eq!(set(items), want);
        assert_eq!(map(Vec::<(u8, u8)>::new()), BTreeMap::new());
    }

    /// Items that own what they hold are moved, never copied or dropped:
    /// every string comes out once, in its place.
    #[test]
    fn owned_items_are_moved_whole() {
        let mut words: Vec<String> = ["pear", "fig", "apple", "fig", "kiwi"]
            .into_iter()
            .map(String::from)
            .collect();
        by_key(&mut words, |word| word.len());
        assert_eq!(words, ["fig", "fig", "pear", "kiwi", "apple"]);
    }
}
