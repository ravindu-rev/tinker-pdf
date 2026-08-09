//! Interned names (7.3.5).
//!
//! A [`Name`] is a `u32` symbol, so the content interpreter's hot loop compares
//! integers instead of byte strings. The names every PDF uses are pre-interned
//! at fixed ids by [`NameTable::new`], which is what makes the [`Name`]
//! constants below usable as compile-time values.
//!
//! Names are byte strings. 7.3.5 says nothing about their encoding and real
//! files carry names that are not valid UTF-8, so nothing here ever assumes it.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// An interned name, valid only against the [`NameTable`] that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(u32);

impl Name {
    /// The raw symbol id. Only meaningful to the table that issued it.
    pub fn id(self) -> u32 {
        self.0
    }
}

/// The names pre-interned by every [`NameTable`], in id order.
///
/// The [`Name`] constants below are the indices into this list; the two are
/// kept in step by `preinterned_ids_match_constants`.
pub const NAMES: &[&[u8]] = &[
    b"Type",
    b"Pages",
    b"Kids",
    b"Count",
    b"Parent",
    b"Length",
    b"Filter",
    b"DecodeParms",
    b"Root",
    b"Info",
    b"Size",
    b"Prev",
    b"XRefStm",
    b"W",
    b"Index",
    b"First",
    b"N",
    b"Encrypt",
    b"ID",
    b"MediaBox",
    b"Contents",
    b"Resources",
];

impl Name {
    /// `/Type`
    pub const TYPE: Name = Name(0);
    /// `/Pages`
    pub const PAGES: Name = Name(1);
    /// `/Kids`
    pub const KIDS: Name = Name(2);
    /// `/Count`
    pub const COUNT: Name = Name(3);
    /// `/Parent`
    pub const PARENT: Name = Name(4);
    /// `/Length`
    pub const LENGTH: Name = Name(5);
    /// `/Filter`
    pub const FILTER: Name = Name(6);
    /// `/DecodeParms`
    pub const DECODE_PARMS: Name = Name(7);
    /// `/Root`
    pub const ROOT: Name = Name(8);
    /// `/Info`
    pub const INFO: Name = Name(9);
    /// `/Size`
    pub const SIZE: Name = Name(10);
    /// `/Prev`
    pub const PREV: Name = Name(11);
    /// `/XRefStm`
    pub const XREF_STM: Name = Name(12);
    /// `/W`
    pub const W: Name = Name(13);
    /// `/Index`
    pub const INDEX: Name = Name(14);
    /// `/First`
    pub const FIRST: Name = Name(15);
    /// `/N`
    pub const N: Name = Name(16);
    /// `/Encrypt`
    pub const ENCRYPT: Name = Name(17);
    /// `/ID`
    pub const ID: Name = Name(18);
    /// `/MediaBox`
    pub const MEDIA_BOX: Name = Name(19);
    /// `/Contents`
    pub const CONTENTS: Name = Name(20);
    /// `/Resources`
    pub const RESOURCES: Name = Name(21);
}

#[derive(Default)]
struct Interner {
    bytes: Vec<Arc<[u8]>>,
    ids: HashMap<Arc<[u8]>, u32>,
}

/// The per-document symbol table.
///
/// Interior-mutable and `Sync`: object loading interns from many threads at
/// once, and a `&self` API keeps the store's parse path lock-free above this
/// point. Lookups take a read lock and only a first sighting takes the write
/// lock.
pub struct NameTable {
    inner: RwLock<Interner>,
}

impl Default for NameTable {
    fn default() -> NameTable {
        NameTable::new()
    }
}

impl NameTable {
    /// A table with [`NAMES`] already interned at their constant ids.
    pub fn new() -> NameTable {
        let mut interner = Interner {
            bytes: Vec::with_capacity(NAMES.len()),
            ids: HashMap::with_capacity(NAMES.len()),
        };
        for bytes in NAMES {
            let key: Arc<[u8]> = Arc::from(*bytes);
            // Length is bounded by NAMES, which is far below u32::MAX.
            let id = interner.bytes.len() as u32;
            interner.bytes.push(Arc::clone(&key));
            interner.ids.insert(key, id);
        }
        NameTable {
            inner: RwLock::new(interner),
        }
    }

    /// The symbol for `bytes`, interning it if this is its first sighting.
    ///
    /// Names are compared as raw bytes: `/Lime#20Green` and `/Lime Green`
    /// cannot both occur, but a name and its `#`-escaped spelling decode to the
    /// same bytes and therefore to the same symbol.
    pub fn intern(&self, bytes: &[u8]) -> Name {
        if let Some(name) = self.lookup(bytes) {
            return name;
        }
        let mut inner = self.write();
        // Re-check: another thread may have interned it between the two locks.
        if let Some(id) = inner.ids.get(bytes) {
            return Name(*id);
        }
        // A table needing more than u32::MAX ids needs 4 G distinct names in
        // one document; no buffer holds them. Saturate rather than wrap.
        let Ok(id) = u32::try_from(inner.bytes.len()) else {
            return Name(u32::MAX);
        };
        let key: Arc<[u8]> = Arc::from(bytes);
        inner.bytes.push(Arc::clone(&key));
        inner.ids.insert(key, id);
        Name(id)
    }

    /// The symbol for `bytes` if it has been interned, without interning it.
    pub fn lookup(&self, bytes: &[u8]) -> Option<Name> {
        self.read().ids.get(bytes).copied().map(Name)
    }

    /// The bytes behind a symbol, or `None` if it came from another table.
    pub fn bytes(&self, name: Name) -> Option<Arc<[u8]>> {
        self.read().bytes.get(name.0 as usize).cloned()
    }

    /// How many distinct names are interned.
    pub fn len(&self) -> usize {
        self.read().bytes.len()
    }

    /// Always false: [`NAMES`] is interned at construction.
    pub fn is_empty(&self) -> bool {
        self.read().bytes.is_empty()
    }

    // Nothing in this module can panic while holding either lock, so poisoning
    // is unreachable; recovering the guard rather than unwrapping keeps that
    // true even if it ever stops being.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Interner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Interner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }
}

impl std::fmt::Debug for NameTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NameTable")
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preinterned_ids_match_constants() {
        let table = NameTable::new();
        let consts = [
            (Name::TYPE, &b"Type"[..]),
            (Name::PAGES, b"Pages"),
            (Name::KIDS, b"Kids"),
            (Name::COUNT, b"Count"),
            (Name::PARENT, b"Parent"),
            (Name::LENGTH, b"Length"),
            (Name::FILTER, b"Filter"),
            (Name::DECODE_PARMS, b"DecodeParms"),
            (Name::ROOT, b"Root"),
            (Name::INFO, b"Info"),
            (Name::SIZE, b"Size"),
            (Name::PREV, b"Prev"),
            (Name::XREF_STM, b"XRefStm"),
            (Name::W, b"W"),
            (Name::INDEX, b"Index"),
            (Name::FIRST, b"First"),
            (Name::N, b"N"),
            (Name::ENCRYPT, b"Encrypt"),
            (Name::ID, b"ID"),
            (Name::MEDIA_BOX, b"MediaBox"),
            (Name::CONTENTS, b"Contents"),
            (Name::RESOURCES, b"Resources"),
        ];
        assert_eq!(consts.len(), NAMES.len());
        assert_eq!(table.len(), NAMES.len());
        for (name, bytes) in consts {
            assert_eq!(
                table.intern(bytes),
                name,
                "{:?}",
                core::str::from_utf8(bytes)
            );
            assert_eq!(table.bytes(name).as_deref(), Some(bytes));
        }
        assert_eq!(table.len(), NAMES.len());
    }

    #[test]
    fn interning_is_stable_and_reversible() {
        let table = NameTable::new();
        let a = table.intern(b"CustomThing");
        let b = table.intern(b"CustomThing");
        let c = table.intern(b"OtherThing");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(table.bytes(a).as_deref(), Some(&b"CustomThing"[..]));
        assert_eq!(table.len(), NAMES.len() + 2);
    }

    #[test]
    fn names_are_byte_strings_not_utf8() {
        let table = NameTable::new();
        let raw = table.intern(&[0xff, 0xfe, 0x00, 0x41]);
        assert_eq!(
            table.bytes(raw).as_deref(),
            Some(&[0xff, 0xfe, 0x00, 0x41][..])
        );
    }

    #[test]
    fn empty_name_is_a_name() {
        let table = NameTable::new();
        let empty = table.intern(b"");
        assert_eq!(table.bytes(empty).as_deref(), Some(&b""[..]));
        assert_ne!(empty, Name::TYPE);
    }

    #[test]
    fn lookup_does_not_intern() {
        let table = NameTable::new();
        assert_eq!(table.lookup(b"NeverSeen"), None);
        assert_eq!(table.len(), NAMES.len());
        assert_eq!(table.lookup(b"Type"), Some(Name::TYPE));
    }

    #[test]
    fn unknown_symbol_has_no_bytes() {
        let table = NameTable::new();
        let other = NameTable::new();
        let only_in_other = other.intern(b"Elsewhere");
        assert_eq!(table.bytes(only_in_other), None);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn interning_is_thread_safe() {
        let table = Arc::new(NameTable::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let table = Arc::clone(&table);
            handles.push(std::thread::spawn(move || {
                let mut ids = Vec::new();
                for i in 0..200u32 {
                    ids.push(table.intern(format!("Name{i}").as_bytes()));
                }
                ids
            }));
        }
        let mut first: Option<Vec<Name>> = None;
        for h in handles {
            let ids = h.join().expect("interner thread must not panic");
            match &first {
                None => first = Some(ids),
                Some(expected) => assert_eq!(&ids, expected),
            }
        }
        assert_eq!(table.len(), NAMES.len() + 200);
    }
}
