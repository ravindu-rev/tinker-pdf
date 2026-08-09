//! The lazy object store and its cycle guard.
//!
//! Objects load on first read and are shared as `Arc<Object>`: one parse, any
//! number of concurrent readers, no copy on read.
//!
//! Publication is a compare-and-swap of the `Arc`, not a lock around the
//! parse. Two threads racing on the same object both parse it — parsing is
//! pure, so both get the same value — one wins the swap and the loser drops
//! its copy. That is cheaper than blocking a slot and it is deadlock-proof:
//! mutually referencing objects loaded from two threads cannot wait on each
//! other because nobody waits at all. `OnceLock` was rejected for the same
//! reason, since same-thread re-entry deadlocks it, and re-entry is exactly
//! what a `/Length` pointing into its own stream produces.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::limits;
use crate::object::Object;

/// Poison-safe locking.
///
/// Nothing in this crate panics while holding a lock, so poisoning is
/// unreachable; recovering the guard rather than unwrapping keeps a future
/// panic from turning one broken document into a permanently broken store.
pub(crate) trait LockExt<T> {
    fn read_lock(&self) -> RwLockReadGuard<'_, T>;
    fn write_lock(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> LockExt<T> for RwLock<T> {
    fn read_lock(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_lock(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(|e| e.into_inner())
    }
}

pub(crate) trait MutexExt<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Default)]
struct Slots {
    dense: Vec<Option<Arc<Object>>>,
    spill: BTreeMap<u32, Arc<Object>>,
}

/// The slot table: object number to loaded object.
///
/// Dense up to [`limits::MAX_XREF_SLOTS`] and a map above it, for the same
/// reason the cross-reference table is: object numbers are attacker-chosen.
#[derive(Default)]
pub(crate) struct SlotStore {
    slots: RwLock<Slots>,
}

impl SlotStore {
    pub(crate) fn new() -> SlotStore {
        SlotStore::default()
    }

    pub(crate) fn get(&self, num: u32) -> Option<Arc<Object>> {
        let slots = self.slots.read_lock();
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => slots.dense.get(i).cloned().flatten(),
            _ => slots.spill.get(&num).cloned(),
        }
    }

    /// Publishes `object` under `num` unless something is already there,
    /// returning whichever value won. The loser's copy is dropped by the
    /// caller when the returned `Arc` replaces it.
    pub(crate) fn publish(&self, num: u32, object: Arc<Object>) -> Arc<Object> {
        let mut slots = self.slots.write_lock();
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => {
                if slots.dense.len() <= i {
                    slots.dense.resize(i + 1, None);
                }
                match slots.dense.get_mut(i) {
                    Some(Some(existing)) => Arc::clone(existing),
                    Some(slot) => {
                        *slot = Some(Arc::clone(&object));
                        object
                    }
                    // Unreachable: the resize above guarantees the index.
                    None => object,
                }
            }
            _ => Arc::clone(
                slots
                    .spill
                    .entry(num)
                    .or_insert_with(|| Arc::clone(&object)),
            ),
        }
    }

    /// Forgets every loaded object. Outstanding `Arc`s keep their values; a
    /// later read reloads from the buffer, which is what installing a
    /// decryptor after the fact needs.
    pub(crate) fn clear(&self) {
        let mut slots = self.slots.write_lock();
        slots.dense.clear();
        slots.spill.clear();
    }
}

/// The objects currently being loaded, innermost last.
///
/// Carried down the load path rather than kept per-thread so that the guard
/// works the same on wasm32-unknown-unknown, where there is one thread and
/// re-entry is still possible.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolveCtx {
    stack: Vec<u32>,
}

impl ResolveCtx {
    pub(crate) fn new() -> ResolveCtx {
        ResolveCtx::default()
    }

    pub(crate) fn contains(&self, num: u32) -> bool {
        self.stack.contains(&num)
    }

    pub(crate) fn depth(&self) -> usize {
        self.stack.len()
    }

    pub(crate) fn push(&mut self, num: u32) {
        self.stack.push(num);
    }

    pub(crate) fn pop(&mut self) {
        self.stack.pop();
    }

    /// Runs `body` with `num` on the stack, popping it however `body` exits.
    pub(crate) fn enter<T>(&mut self, num: u32, body: impl FnOnce(&mut ResolveCtx) -> T) -> T {
        self.push(num);
        let value = body(self);
        self.pop();
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_publisher_wins_and_later_ones_see_it() {
        let store = SlotStore::new();
        assert!(store.get(3).is_none());
        let first = store.publish(3, Arc::new(Object::Int(1)));
        assert_eq!(*first, Object::Int(1));
        let second = store.publish(3, Arc::new(Object::Int(2)));
        assert_eq!(*second, Object::Int(1));
        assert_eq!(*store.get(3).expect("published"), Object::Int(1));
    }

    #[test]
    fn numbers_past_the_dense_cap_spill() {
        let store = SlotStore::new();
        let num = u32::MAX;
        store.publish(num, Arc::new(Object::Bool(true)));
        assert_eq!(*store.get(num).expect("spilled"), Object::Bool(true));
        assert!(store.get(num - 1).is_none());
    }

    #[test]
    fn clear_forgets_but_does_not_invalidate() {
        let store = SlotStore::new();
        let held = store.publish(1, Arc::new(Object::Int(7)));
        store.clear();
        assert!(store.get(1).is_none());
        assert_eq!(*held, Object::Int(7));
    }

    #[test]
    fn the_context_is_a_stack() {
        let mut ctx = ResolveCtx::new();
        assert!(!ctx.contains(1));
        ctx.enter(1, |ctx| {
            assert!(ctx.contains(1));
            ctx.enter(2, |ctx| {
                assert!(ctx.contains(1));
                assert!(ctx.contains(2));
                assert_eq!(ctx.depth(), 2);
            });
            assert!(!ctx.contains(2));
        });
        assert_eq!(ctx.depth(), 0);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn racing_publishers_agree() {
        let store = Arc::new(SlotStore::new());
        let mut handles = Vec::new();
        for t in 0..8u32 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                let mut seen = Vec::new();
                for num in 0..200u32 {
                    seen.push(store.publish(num, Arc::new(Object::Int(i64::from(t)))));
                }
                seen
            }));
        }
        let mut winners: Option<Vec<Arc<Object>>> = None;
        for h in handles {
            let seen = h.join().expect("store thread must not panic");
            match &winners {
                None => winners = Some(seen),
                Some(expected) => {
                    for (a, b) in seen.iter().zip(expected) {
                        assert_eq!(a, b);
                    }
                }
            }
        }
    }
}
