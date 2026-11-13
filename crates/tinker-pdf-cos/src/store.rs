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
//!
//! **Under a budget** (`docs/design/performance.md`, P-16, "Parsed objects
//! under pressure") the store keeps its objects in an [`Accountant`]'s
//! ledger instead, charged an estimate of their bytes beside whatever else
//! charges it: every read that reaches a kept object takes a number from the
//! ledger's counter, and past the budget the objects least recently
//! requested that no caller holds are dropped. A later read parses again from
//! the same immutable bytes and gets an equal value, which is what makes
//! dropping one safe. With no budget nothing is dropped, and the store is the
//! table below, as it always was.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::accountant::{Accountant, Entries, ObjectLedger};
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
/// Under a budget the objects live in the ledger it charges instead, and the
/// table stays empty.
pub(crate) struct SlotStore {
    /// This store's name in a ledger, which several stores may share.
    id: u64,
    slots: RwLock<Slots>,
    /// The ledger this store's objects are charged to, once one is handed it.
    budget: OnceLock<Arc<dyn ObjectLedger>>,
}

/// The names stores are given in a ledger: one each, never reused.
static STORES: AtomicU64 = AtomicU64::new(0);

impl Default for SlotStore {
    fn default() -> SlotStore {
        SlotStore {
            id: STORES.fetch_add(1, Ordering::Relaxed),
            slots: RwLock::new(Slots::default()),
            budget: OnceLock::new(),
        }
    }
}

impl SlotStore {
    pub(crate) fn new() -> SlotStore {
        SlotStore::default()
    }

    pub(crate) fn get(&self, num: u32) -> Option<Arc<Object>> {
        if let Some(ledger) = self.budget.get() {
            return ledger.get(self.id, num);
        }
        let slots = self.slots.read_lock();
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => slots.dense.get(i).cloned().flatten(),
            _ => slots.spill.get(&num).cloned(),
        }
    }

    /// Publishes `object` under `num` unless something is already there,
    /// returning whichever value won. The loser's copy is dropped by the
    /// caller when the returned `Arc` replaces it.
    ///
    /// `charge` is what a ledger is charged for it, where this store has one:
    /// past that ledger's budget the object is handed back and kept nowhere,
    /// so that the next read parses it again.
    pub(crate) fn publish(&self, num: u32, object: Arc<Object>, charge: u64) -> Arc<Object> {
        if let Some(ledger) = self.budget.get() {
            return ledger.keep(self.id, num, object, charge);
        }
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
        {
            let mut slots = self.slots.write_lock();
            slots.dense.clear();
            slots.spill.clear();
        }
        if let Some(ledger) = self.budget.get() {
            ledger.forget_objects(self.id);
        }
    }

    /// Charges this store's objects to `accountant` from now on, and says
    /// whether it took: a store is charged to the first accountant handed it
    /// and to that one only. The objects loaded before are forgotten, as
    /// [`SlotStore::clear`] forgets them, so that none is held outside the
    /// budget; a later read parses it again.
    pub(crate) fn charge_to<E: Entries>(&self, accountant: Arc<Accountant<E>>) -> bool {
        let ledger: Arc<dyn ObjectLedger> = accountant;
        if self.budget.set(ledger).is_err() {
            return false;
        }
        let mut slots = self.slots.write_lock();
        slots.dense.clear();
        slots.spill.clear();
        true
    }

    /// Whether this store's objects are charged to a budget.
    pub(crate) fn is_budgeted(&self) -> bool {
        self.budget.get().is_some()
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
    /// Whether a range the load needed was not available.
    ///
    /// **A miss never publishes.** An object whose bytes could not be fetched
    /// reads as null, because 7.3.10 says a missing object is null and ruling
    /// 2 says degrade rather than fail -- but that null must not enter the
    /// store, or every later read of the slot would get the miss's answer
    /// forever, including the read that happens after the host has supplied
    /// the bytes. The flag is set where the window was refused and checked
    /// where the value would be published.
    missed: bool,
}

impl ResolveCtx {
    pub(crate) fn new() -> ResolveCtx {
        ResolveCtx::default()
    }

    pub(crate) fn contains(&self, num: u32) -> bool {
        self.stack.contains(&num)
    }

    /// Records that a range this load needed was not available.
    pub(crate) fn note_miss(&mut self) {
        self.missed = true;
    }

    /// Whether anything in this resolve has missed.
    pub(crate) fn missed(&self) -> bool {
        self.missed
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
    use crate::doc::count;
    use crate::{CosDocument, ObjRef, PdfString, SliceSource};

    #[test]
    fn first_publisher_wins_and_later_ones_see_it() {
        let store = SlotStore::new();
        assert!(store.get(3).is_none());
        let first = store.publish(3, Arc::new(Object::Int(1)), 0);
        assert_eq!(*first, Object::Int(1));
        let second = store.publish(3, Arc::new(Object::Int(2)), 0);
        assert_eq!(*second, Object::Int(1));
        assert_eq!(*store.get(3).expect("published"), Object::Int(1));
    }

    #[test]
    fn numbers_past_the_dense_cap_spill() {
        let store = SlotStore::new();
        let num = u32::MAX;
        store.publish(num, Arc::new(Object::Bool(true)), 0);
        assert_eq!(*store.get(num).expect("spilled"), Object::Bool(true));
        assert!(store.get(num - 1).is_none());
    }

    #[test]
    fn clear_forgets_but_does_not_invalidate() {
        let store = SlotStore::new();
        let held = store.publish(1, Arc::new(Object::Int(7)), 0);
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
        for budget in [None, Some(1 << 20)] {
            let store = Arc::new(SlotStore::new());
            if let Some(budget) = budget {
                assert!(store.charge_to(Arc::new(Accountant::for_objects(budget))));
            }
            let mut handles = Vec::new();
            for t in 0..8u32 {
                let store = Arc::clone(&store);
                handles.push(std::thread::spawn(move || {
                    let mut seen = Vec::new();
                    for num in 0..200u32 {
                        seen.push(store.publish(num, Arc::new(Object::Int(i64::from(t))), 100));
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
                            assert_eq!(a, b, "budget {budget:?}");
                        }
                    }
                }
            }
        }
    }

    /// **A store under a budget keeps its objects in the ledger**, charged
    /// what it is told: it hands back what it keeps, forgets what a clear
    /// forgets, and is charged to the first accountant handed it only.
    #[test]
    fn a_store_under_a_budget_keeps_its_objects_in_the_ledger() {
        let store = SlotStore::new();
        assert!(!store.is_budgeted());
        drop(store.publish(1, Arc::new(Object::Int(1)), 100));
        let accountant = Arc::new(Accountant::for_objects(250));
        assert!(store.charge_to(Arc::clone(&accountant)));
        assert!(store.is_budgeted());
        assert!(
            store.get(1).is_none(),
            "what was loaded before the budget is forgotten"
        );
        assert!(
            !store.charge_to(Arc::new(Accountant::for_objects(1 << 20))),
            "charged to the first accountant only"
        );

        drop(store.publish(1, Arc::new(Object::Int(1)), 100));
        drop(store.publish(2, Arc::new(Object::Int(2)), 100));
        assert_eq!(accountant.held(), 200);
        // A third evicts object 1, the least recently requested.
        drop(store.publish(3, Arc::new(Object::Int(3)), 100));
        assert!(store.get(1).is_none());
        assert_eq!(store.get(3).as_deref(), Some(&Object::Int(3)));
        assert_eq!(accountant.held(), 200);
        // Past the whole budget: handed back, kept nowhere.
        let large = store.publish(4, Arc::new(Object::Int(4)), 251);
        assert_eq!(*large, Object::Int(4));
        assert!(store.get(4).is_none());

        store.clear();
        assert!(store.get(2).is_none() && store.get(3).is_none());
        assert_eq!(accountant.held(), 0, "a clear forgets the ledger's objects");
    }

    /// A row of a cross-reference stream at `/W [1 4 2]`.
    fn row(rows: &mut Vec<u8>, kind: u8, field: u32, index: u16) {
        rows.push(kind);
        rows.extend_from_slice(&field.to_be_bytes());
        rows.extend_from_slice(&index.to_be_bytes());
    }

    /// A generated document of everything a read can be charged for, and the
    /// number of every object in it: the catalog and an empty page tree;
    /// `direct` objects of growing size from 3, every fifth a stream with its
    /// `/Length` written in it; one stream whose `/Length` is an indirect
    /// object of its own; an object stream holding `members` more; and the
    /// cross-reference stream that names them all, at `/W [1 4 2]`,
    /// uncompressed.
    fn generated(direct: u32, members: u32) -> (Vec<u8>, Vec<u32>) {
        let mut file = b"%PDF-1.7\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        let mut object = |file: &mut Vec<u8>, num: u32, body: &[u8]| {
            offsets.push((num, file.len()));
            file.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            file.extend_from_slice(body);
            file.extend_from_slice(b"\nendobj\n");
        };
        object(&mut file, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut file, 2, b"<< /Type /Pages /Count 0 /Kids [] >>");
        for num in 3..3 + direct {
            let pad = "p".repeat((num as usize * 7) % 90);
            let body = if num % 5 == 0 {
                let data = format!("data of {num} ").repeat(num as usize % 13 + 1);
                format!(
                    "<< /N {num} /Pad ({pad}) /Length {} >>\nstream\n{data}\nendstream",
                    data.len()
                )
            } else {
                format!(
                    "<< /N {num} /Pad ({pad}) /Kids [{} 0 R] >>",
                    3 + (num % direct)
                )
            };
            object(&mut file, num, body.as_bytes());
        }
        let indirect = 3 + direct;
        let data = "a stream whose length is an object of its own";
        object(
            &mut file,
            indirect,
            format!(
                "<< /Length {} 0 R >>\nstream\n{data}\nendstream",
                indirect + 1
            )
            .as_bytes(),
        );
        object(&mut file, indirect + 1, data.len().to_string().as_bytes());

        let container = indirect + 2;
        let first_member = container + 1;
        let mut prologue = String::new();
        let mut bodies = String::new();
        for num in first_member..first_member + members {
            prologue.push_str(&format!("{num} {} ", bodies.len()));
            bodies.push_str(&format!(
                "<< /Member {num} /Pad ({}) >> ",
                "m".repeat(num as usize % 40)
            ));
        }
        let packed = format!("{prologue}{bodies}");
        object(
            &mut file,
            container,
            format!(
                "<< /Type /ObjStm /N {members} /First {} /Length {} >>\nstream\n{packed}\nendstream",
                prologue.len(),
                packed.len()
            )
            .as_bytes(),
        );

        let xref_num = first_member + members;
        let xref_at = file.len();
        let mut rows = Vec::new();
        row(&mut rows, 0, 0, 0xFFFF);
        for num in 1..xref_num {
            match offsets.iter().find(|(at, _)| *at == num) {
                Some((_, offset)) => row(&mut rows, 1, *offset as u32, 0),
                None => row(&mut rows, 2, container, (num - first_member) as u16),
            }
        }
        row(&mut rows, 1, xref_at as u32, 0);
        file.extend_from_slice(
            format!(
                "{xref_num} 0 obj\n<< /Type /XRef /Size {} /W [1 4 2] /Root 1 0 R /Length {} >>\nstream\n",
                xref_num + 1,
                rows.len()
            )
            .as_bytes(),
        );
        file.extend_from_slice(&rows);
        file.extend_from_slice(
            format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        (file, (1..=xref_num).collect())
    }

    /// Every object of `numbers` read from `doc`, in order.
    fn read_all(doc: &CosDocument, numbers: &[u32]) -> Vec<Arc<Object>> {
        numbers
            .iter()
            .map(|&num| doc.get(ObjRef::new(num, 0)).expect("an object reads"))
            .collect()
    }

    /// **A store over its budget drops objects and parses them again equal**
    /// (`docs/design/performance.md`, P-16.8). Over a generated document of
    /// direct objects, streams, a stream whose `/Length` is an object of its
    /// own and an object stream's members, every object read twice:
    ///
    /// - with no budget, each is parsed once — today's count — and a budget
    ///   that holds everything parses each once too;
    /// - under a small budget, every read is equal to the unbudgeted one, the
    ///   bytes held never pass the budget, objects dropped are parsed again —
    ///   counted — and the three objects a caller holds throughout are never
    ///   dropped: a read of one is the very value held, and parses nothing;
    /// - the object stream is decoded once whatever is dropped, since the
    ///   decoded containers and their indexes are never charged;
    /// - under a budget of zero nothing is kept, and every read parses.
    #[test]
    fn a_store_over_its_budget_drops_objects_and_parses_them_again_equal() {
        let (bytes, numbers) = generated(60, 24);
        let objects = numbers.len() as u64;

        let plain = CosDocument::open(bytes.clone()).expect("the document opens");
        let _ = count::parses();
        let want = read_all(&plain, &numbers);
        assert_eq!(read_all(&plain, &numbers), want);
        assert_eq!(
            count::parses(),
            objects,
            "with no budget, each object is parsed once"
        );
        assert!(!plain.objects_charged());
        assert!(
            want.iter()
                .any(|object| matches!(**object, Object::Stream(_))),
            "the document holds streams"
        );

        let roomy = CosDocument::open(bytes.clone()).expect("the document opens");
        let everything = Arc::new(Accountant::for_objects(1 << 30));
        assert!(roomy.charge_objects_to(Arc::clone(&everything)));
        let _ = count::parses();
        assert_eq!(read_all(&roomy, &numbers), want);
        assert_eq!(read_all(&roomy, &numbers), want);
        assert_eq!(
            count::parses(),
            objects,
            "a budget that holds everything parses each object once"
        );
        let all_held = everything.held();
        assert!(all_held >= objects * crate::accountant::ENTRY_BYTES);

        // A budget of a fifth of what holding everything is charged.
        let budget = all_held / 5;
        let doc = CosDocument::open(bytes.clone()).expect("the document opens");
        let accountant = Arc::new(Accountant::for_objects(budget));
        assert!(doc.charge_objects_to(Arc::clone(&accountant)));
        assert!(doc.objects_charged());
        let _ = count::parses();
        // One direct object, the stream whose length is an object of its own,
        // and one member of the object stream, held by this caller throughout.
        let held_numbers = [7u32, 63, 70];
        let held: Vec<Arc<Object>> = held_numbers
            .iter()
            .map(|&num| doc.get(ObjRef::new(num, 0)).expect("reads"))
            .collect();
        for round in 0..2 {
            for (&num, want) in numbers.iter().zip(&want) {
                let got = doc.get(ObjRef::new(num, 0)).expect("an object reads");
                assert_eq!(
                    &got, want,
                    "object {num}, round {round}: what an unbudgeted read reads"
                );
                drop(got);
                assert!(
                    accountant.held() <= budget,
                    "object {num}, round {round}: {} held of {budget}",
                    accountant.held()
                );
            }
        }
        let parses = count::parses();
        assert!(
            parses > objects + held_numbers.len() as u64,
            "objects dropped are parsed again: {parses} parses of {objects} objects"
        );
        for (num, held) in held_numbers.iter().zip(&held) {
            let again = doc.get(ObjRef::new(*num, 0)).expect("reads");
            assert!(
                Arc::ptr_eq(&again, held),
                "object {num} is held, so never dropped"
            );
        }
        assert_eq!(count::parses(), 0, "a held object is never parsed again");
        assert_eq!(
            doc.object_stream_decodes(),
            1,
            "the object stream is decoded once, whatever its members' fate"
        );
        assert!(accountant.objects_held() == accountant.held() && accountant.held() <= budget);
        drop(held);

        let starved = CosDocument::open(bytes).expect("the document opens");
        let nothing = Arc::new(Accountant::for_objects(0));
        assert!(starved.charge_objects_to(Arc::clone(&nothing)));
        let _ = count::parses();
        assert_eq!(read_all(&starved, &numbers), want);
        assert_eq!(read_all(&starved, &numbers), want);
        assert!(
            count::parses() >= 2 * objects,
            "under a budget of zero every read parses"
        );
        assert_eq!(nothing.held(), 0);
    }

    /// **A streamed document under a budget reads what a buffered one reads**:
    /// an object dropped and read again is read from the source's bytes, to
    /// the same value.
    #[test]
    fn a_streamed_document_under_a_budget_reads_its_objects_again_equal() {
        let (bytes, numbers) = generated(30, 10);
        let want = read_all(&CosDocument::open(bytes.clone()).expect("opens"), &numbers);
        let streamed = CosDocument::open_source(Arc::new(SliceSource::new(bytes)))
            .expect("the document opens from its source");
        let accountant = Arc::new(Accountant::for_objects(600));
        assert!(streamed.charge_objects_to(Arc::clone(&accountant)));
        for _ in 0..2 {
            assert_eq!(read_all(&streamed, &numbers), want);
            assert!(accountant.held() <= 600);
        }
    }

    /// **Installing a decryptor under a budget gives the charges back**: the
    /// store forgets every object loaded, as it does with no budget, the
    /// ledger with it, and reads parse again to the same values.
    #[test]
    fn a_clear_under_a_budget_gives_the_charges_back() {
        let (bytes, numbers) = generated(10, 4);
        let doc = CosDocument::open(bytes).expect("opens");
        let accountant = Arc::new(Accountant::for_objects(1 << 20));
        assert!(doc.charge_objects_to(Arc::clone(&accountant)));
        let want = read_all(&doc, &numbers);
        assert!(accountant.held() > 0);
        doc.set_decryptor(Arc::new(crate::IdentityDecryptor));
        assert_eq!(accountant.held(), 0);
        let _ = count::parses();
        assert_eq!(read_all(&doc, &numbers), want);
        assert_eq!(
            count::parses(),
            numbers.len() as u64,
            "every object parsed again"
        );
    }

    /// **A reopened document under a budget leaves its base's objects to the
    /// base**: an object the update leaves alone is read through the base's
    /// store and charged nowhere here, and one the update defines is parsed
    /// from the update and charged.
    #[test]
    fn a_reopened_document_under_a_budget_leaves_its_bases_objects_to_the_base() {
        let bodies = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Count 0 /Kids [] >>",
            "(object 3)",
            "(object 4)",
        ];
        let mut file = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            offsets.push(file.len());
            file.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = file.len();
        file.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in &offsets {
            file.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        file.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        // The update redefines object 3, in a section of its own chained to
        // the base's by `/Prev`.
        let three = file.len();
        let mut update = b"3 0 obj\n(object 3, updated)\nendobj\n".to_vec();
        let section = file.len() + update.len();
        update.extend_from_slice(
            format!(
                "xref\n0 1\n0000000000 65535 f \n3 1\n{three:010} 00000 n \n\
                 trailer\n<< /Size 5 /Root 1 0 R /Prev {xref} >>\n\
                 startxref\n{section}\n%%EOF\n"
            )
            .as_bytes(),
        );
        let base = Arc::new(CosDocument::open(file).expect("the base opens"));
        let reopened = CosDocument::reopen(&base, update).expect("it reopens");
        assert_eq!(reopened.full_open, None, "only the update was read");
        let doc = reopened.document;
        let accountant = Arc::new(Accountant::for_objects(1 << 20));
        assert!(doc.charge_objects_to(Arc::clone(&accountant)));
        for _ in 0..2 {
            let four = doc.get(ObjRef::new(4, 0)).expect("reads");
            assert_eq!(
                *four,
                Object::String(PdfString::literal(b"object 4".to_vec()))
            );
        }
        assert_eq!(accountant.held(), 0, "object 4 is the base's, held there");
        let three = doc.get(ObjRef::new(3, 0)).expect("reads");
        assert_eq!(
            *three,
            Object::String(PdfString::literal(b"object 3, updated".to_vec()))
        );
        assert!(
            accountant.held() > 0,
            "object 3 is the update's own, charged here"
        );
    }
}
