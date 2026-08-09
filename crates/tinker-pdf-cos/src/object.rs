//! The COS object model (7.3).
//!
//! Objects are immutable once parsed: the store shares them as `Arc<Object>`
//! and the editing layer will overlay rather than mutate, so nothing here
//! carries interior mutability.

use core::ops::Range;

use crate::name::Name;

/// An indirect reference (7.3.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjRef {
    /// Object number.
    pub num: u32,
    /// Generation number.
    pub gen: u16,
}

impl ObjRef {
    /// The reference `num gen R`.
    pub const fn new(num: u32, gen: u16) -> ObjRef {
        ObjRef { num, gen }
    }
}

impl core::fmt::Display for ObjRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} {} R", self.num, self.gen)
    }
}

/// A string object (7.3.4), decoded to its bytes.
///
/// PDF strings are byte strings; interpreting them as text needs the
/// surrounding context (7.9.2), which this layer does not have. The origin
/// flag is kept because 7.9.2's PDFDocEncoded/UTF-16BE sniffing is unaffected
/// by it but round-tripping is not: a writer that re-emits a hex string as a
/// literal string changes bytes that signatures cover.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct PdfString {
    /// The decoded bytes, after escape processing and before decryption.
    pub bytes: Vec<u8>,
    /// True if the source spelling was `<...>` rather than `(...)`.
    pub hex: bool,
}

impl PdfString {
    /// A string written as `(...)` (7.3.4.2).
    pub fn literal(bytes: Vec<u8>) -> PdfString {
        PdfString { bytes, hex: false }
    }

    /// A string written as `<...>` (7.3.4.3).
    pub fn hex(bytes: Vec<u8>) -> PdfString {
        PdfString { bytes, hex: true }
    }
}

/// A stream object (7.3.8): its dictionary plus where its data sits.
///
/// The range is raw and pre-recovery — exactly what the file claims, with no
/// `endstream` validation and no `/Length` resolution. Both are the document
/// layer's job because both can require resolving an indirect object, which
/// this layer has no store to do.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamObj {
    /// The stream's dictionary.
    pub dict: Dict,
    /// Offset of the first data byte: just past the EOL after `stream`.
    pub data_start: u64,
    /// The declared length, when `/Length` was a direct non-negative integer.
    ///
    /// `None` means the length is not yet known — `/Length` was indirect
    /// (legal and common) or absent (warned about at parse time). The document
    /// layer resolves it and validates it against `endstream`.
    pub len_hint: Option<u64>,
}

impl StreamObj {
    /// The claimed data range. Empty when the length is still unresolved, so
    /// no caller can mistake "unknown" for "reaches to end of buffer".
    pub fn data_range(&self) -> Range<u64> {
        let end = match self.len_hint {
            Some(len) => self.data_start.saturating_add(len),
            None => self.data_start,
        };
        self.data_start..end
    }
}

/// A COS object (7.3).
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    /// `null` (7.3.9), and the value every unrecoverable read degrades to.
    Null,
    /// `true` or `false` (7.3.2).
    Bool(bool),
    /// An integer (7.3.3), clamped to `i64` at lex time.
    Int(i64),
    /// A real (7.3.3).
    Real(f64),
    /// A literal or hexadecimal string (7.3.4).
    String(PdfString),
    /// A name (7.3.5), interned.
    Name(Name),
    /// An array (7.3.6).
    Array(Vec<Object>),
    /// A dictionary (7.3.7).
    Dict(Dict),
    /// A stream (7.3.8).
    Stream(StreamObj),
    /// An indirect reference (7.3.10).
    Ref(ObjRef),
}

impl Object {
    /// Whether this is [`Object::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Object::Null)
    }

    /// The boolean value, or `None` for any other type.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Object::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer value, or `None` for any other type — including a real that
    /// happens to be integral.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Object::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The real value, or `None` for any other type including integers.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Object::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// The numeric value of an integer or a real. 7.3.3 allows an integer
    /// wherever a real is expected, so most numeric readers want this.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Object::Int(i) => Some(*i as f64),
            Object::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// The string, or `None` for any other type.
    pub fn as_string(&self) -> Option<&PdfString> {
        match self {
            Object::String(s) => Some(s),
            _ => None,
        }
    }

    /// The name, or `None` for any other type.
    pub fn as_name(&self) -> Option<Name> {
        match self {
            Object::Name(n) => Some(*n),
            _ => None,
        }
    }

    /// The array's elements, or `None` for any other type.
    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Object::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The dictionary, for a dictionary or a stream — 7.3.8 makes a stream a
    /// dictionary plus data, and readers of `/Type` should not care which.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// The stream, or `None` for any other type.
    pub fn as_stream(&self) -> Option<&StreamObj> {
        match self {
            Object::Stream(s) => Some(s),
            _ => None,
        }
    }

    /// The indirect reference, or `None` for any other type.
    pub fn as_objref(&self) -> Option<ObjRef> {
        match self {
            Object::Ref(r) => Some(*r),
            _ => None,
        }
    }
}

/// A dictionary (7.3.7): insertion-ordered key/value pairs with linear lookup.
///
/// Real dictionaries hold well under sixteen entries, where a flat vector beats
/// a hash map, and file order is something the incremental-update writer wants
/// to preserve. Keys are unique: a repeated key replaces the earlier value in
/// place, which is what every surviving reader does with the broken files that
/// contain them.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dict {
    entries: Vec<(Name, Object)>,
}

impl Dict {
    /// An empty dictionary.
    pub fn new() -> Dict {
        Dict {
            entries: Vec::new(),
        }
    }

    /// An empty dictionary with room for `capacity` entries.
    pub fn with_capacity(capacity: usize) -> Dict {
        Dict {
            entries: Vec::with_capacity(capacity),
        }
    }

    /// The entries in file order.
    pub fn entries(&self) -> &[(Name, Object)] {
        &self.entries
    }

    /// Iterates the entries in file order.
    pub fn iter(&self) -> core::slice::Iter<'_, (Name, Object)> {
        self.entries.iter()
    }

    /// Iterates the entries in file order, allowing values to be rewritten.
    ///
    /// The one writer is the decryption pass (7.6.2), which replaces string
    /// bytes in place after the containing object has been parsed. Keys are
    /// not exposed mutably: rewriting one would break the uniqueness the
    /// lookup relies on.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Object> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the dictionary has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stores `value` under `key`, returning true if it replaced an existing
    /// entry. A replacement keeps the original key's position.
    pub fn insert(&mut self, key: Name, value: Object) -> bool {
        for (k, v) in &mut self.entries {
            if *k == key {
                *v = value;
                return true;
            }
        }
        self.entries.push((key, value));
        false
    }

    /// The value stored under `key`.
    pub fn get(&self, key: Name) -> Option<&Object> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Whether `key` is present. A present `null` counts as present; 7.3.9
    /// makes it equivalent to absent for most readers, but the distinction
    /// belongs to the caller.
    pub fn contains_key(&self, key: Name) -> bool {
        self.get(key).is_some()
    }

    /// The value under `key` if it is a boolean.
    ///
    /// Wrong types read as `None`. The absent-with-warning contract the higher
    /// layers use lives at the call site, which knows what was expected and
    /// which object it was expected in; here a type mismatch is only absence.
    pub fn get_bool(&self, key: Name) -> Option<bool> {
        self.get(key).and_then(Object::as_bool)
    }

    /// The value under `key` if it is an integer.
    pub fn get_int(&self, key: Name) -> Option<i64> {
        self.get(key).and_then(Object::as_int)
    }

    /// The value under `key` if it is a real.
    pub fn get_real(&self, key: Name) -> Option<f64> {
        self.get(key).and_then(Object::as_real)
    }

    /// The value under `key` if it is an integer or a real (7.3.3).
    pub fn get_number(&self, key: Name) -> Option<f64> {
        self.get(key).and_then(Object::as_number)
    }

    /// The value under `key` if it is a string.
    pub fn get_string(&self, key: Name) -> Option<&PdfString> {
        self.get(key).and_then(Object::as_string)
    }

    /// The value under `key` if it is a name.
    pub fn get_name(&self, key: Name) -> Option<Name> {
        self.get(key).and_then(Object::as_name)
    }

    /// The value under `key` if it is an array.
    pub fn get_array(&self, key: Name) -> Option<&[Object]> {
        self.get(key).and_then(Object::as_array)
    }

    /// The value under `key` if it is a dictionary or a stream.
    pub fn get_dict(&self, key: Name) -> Option<&Dict> {
        self.get(key).and_then(Object::as_dict)
    }

    /// The value under `key` if it is a stream.
    pub fn get_stream(&self, key: Name) -> Option<&StreamObj> {
        self.get(key).and_then(Object::as_stream)
    }

    /// The value under `key` if it is an indirect reference. Note that this is
    /// the unresolved reference: resolution needs the store.
    pub fn get_ref(&self, key: Name) -> Option<ObjRef> {
        self.get(key).and_then(Object::as_objref)
    }
}

impl FromIterator<(Name, Object)> for Dict {
    fn from_iter<I: IntoIterator<Item = (Name, Object)>>(iter: I) -> Dict {
        let mut dict = Dict::new();
        for (k, v) in iter {
            dict.insert(k, v);
        }
        dict
    }
}

impl<'a> IntoIterator for &'a Dict {
    type Item = &'a (Name, Object);
    type IntoIter = core::slice::Iter<'a, (Name, Object)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::NameTable;

    #[test]
    fn dict_preserves_insertion_order() {
        let mut d = Dict::new();
        d.insert(Name::SIZE, Object::Int(3));
        d.insert(Name::TYPE, Object::Name(Name::PAGES));
        d.insert(Name::COUNT, Object::Int(1));
        let keys: Vec<Name> = d.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, [Name::SIZE, Name::TYPE, Name::COUNT]);
    }

    #[test]
    fn duplicate_key_replaces_in_place() {
        let mut d = Dict::new();
        d.insert(Name::SIZE, Object::Int(3));
        d.insert(Name::TYPE, Object::Null);
        assert!(d.insert(Name::SIZE, Object::Int(9)));
        assert_eq!(d.len(), 2);
        assert_eq!(d.get_int(Name::SIZE), Some(9));
        let keys: Vec<Name> = d.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, [Name::SIZE, Name::TYPE]);
    }

    #[test]
    fn typed_getters_treat_wrong_types_as_absent() {
        let mut d = Dict::new();
        d.insert(Name::LENGTH, Object::Name(Name::TYPE));
        d.insert(Name::SIZE, Object::Int(4));
        assert_eq!(d.get_int(Name::LENGTH), None);
        assert_eq!(d.get_name(Name::SIZE), None);
        assert_eq!(d.get_array(Name::SIZE), None);
        assert_eq!(d.get_dict(Name::SIZE), None);
        assert_eq!(d.get_ref(Name::SIZE), None);
        assert_eq!(d.get_string(Name::SIZE), None);
        assert_eq!(d.get_bool(Name::SIZE), None);
        assert_eq!(d.get_real(Name::SIZE), None);
        assert_eq!(d.get_int(Name::KIDS), None);
        assert!(d.contains_key(Name::LENGTH));
        assert!(!d.contains_key(Name::KIDS));
    }

    #[test]
    fn get_number_accepts_both_numeric_types() {
        let mut d = Dict::new();
        d.insert(Name::SIZE, Object::Int(4));
        d.insert(Name::COUNT, Object::Real(4.5));
        assert_eq!(d.get_number(Name::SIZE), Some(4.0));
        assert_eq!(d.get_number(Name::COUNT), Some(4.5));
        assert_eq!(d.get_real(Name::SIZE), None);
    }

    #[test]
    fn stream_dict_reads_as_a_dict() {
        let mut dict = Dict::new();
        dict.insert(Name::LENGTH, Object::Int(10));
        let stream = Object::Stream(StreamObj {
            dict,
            data_start: 40,
            len_hint: Some(10),
        });
        assert_eq!(
            stream.as_dict().and_then(|d| d.get_int(Name::LENGTH)),
            Some(10)
        );
        assert_eq!(stream.as_stream().map(StreamObj::data_range), Some(40..50));
    }

    #[test]
    fn unresolved_stream_length_is_an_empty_range() {
        let s = StreamObj {
            dict: Dict::new(),
            data_start: 40,
            len_hint: None,
        };
        assert_eq!(s.data_range(), 40..40);
        assert!(s.data_range().is_empty());
    }

    #[test]
    fn stream_range_saturates_on_absurd_length() {
        let s = StreamObj {
            dict: Dict::new(),
            data_start: 10,
            len_hint: Some(u64::MAX),
        };
        assert_eq!(s.data_range(), 10..u64::MAX);
    }

    #[test]
    fn dict_collects_from_pairs() {
        let table = NameTable::new();
        let a = table.intern(b"A");
        let d: Dict = [(a, Object::Int(1)), (Name::TYPE, Object::Null)]
            .into_iter()
            .collect();
        assert_eq!(d.len(), 2);
        assert_eq!(d.get_int(a), Some(1));
    }

    #[test]
    fn objref_displays_as_a_reference() {
        assert_eq!(ObjRef::new(12, 0).to_string(), "12 0 R");
    }
}
