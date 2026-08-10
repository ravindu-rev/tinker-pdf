//! Serializing objects and documents (7.5).
//!
//! Two shapes of output, and the difference matters more than it looks.
//!
//! A **full rewrite** renumbers and emits everything, which is how a document
//! is compacted or garbage-collected. An **incremental update** appends: the
//! original bytes stay byte-for-byte identical as a prefix and only changed
//! objects are written after them, with a new cross-reference section chained
//! to the old one. That prefix invariance is not a nicety — a digital
//! signature covers a byte range of the original file, so a signed document
//! can only be updated this way, and phase 10's signing support is built on
//! exactly this.

use std::collections::BTreeMap;

use crate::name::{Name, NameTable};
use crate::object::{Dict, Object, PdfString};

/// How a document should be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Emit every object afresh, renumbering from one.
    Rewrite,
    /// Append changed objects to the original bytes.
    Incremental,
}

/// Options for writing.
#[derive(Clone, Debug)]
pub struct WriteOptions {
    /// Which shape of output.
    pub mode: WriteMode,
    /// The PDF version to declare in the header, on a rewrite.
    pub version: (u8, u8),
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            mode: WriteMode::Rewrite,
            version: (1, 7),
        }
    }
}

/// Escapes and writes a literal string (7.3.4.2).
fn write_string(out: &mut Vec<u8>, s: &PdfString) {
    if s.hex {
        out.push(b'<');
        for byte in &s.bytes {
            out.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
        out.push(b'>');
        return;
    }

    out.push(b'(');
    for &byte in &s.bytes {
        match byte {
            // Only these three need escaping in a literal string; escaping
            // more is legal but makes files bigger and diffs noisier.
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => out.push(byte),
        }
    }
    out.push(b')');
}

/// Writes a name, escaping what 7.3.5 requires.
fn write_name(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'/');
    for &byte in bytes {
        // Delimiters, whitespace, '#' itself and anything outside the
        // printable range must be written as #xx.
        let needs_escape = byte <= b' '
            || byte >= 0x7F
            || matches!(
                byte,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            );
        if needs_escape {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
}

/// Formats a real number without an exponent, which PDF does not allow.
fn write_real(out: &mut Vec<u8>, value: f64) {
    if !value.is_finite() {
        out.extend_from_slice(b"0");
        return;
    }
    if value == value.trunc() && value.abs() < 1e15 {
        out.extend_from_slice(format!("{}", value as i64).as_bytes());
        return;
    }
    // Six decimals is more precision than any coordinate needs, and trailing
    // zeros are trimmed so the output stays compact.
    let text = format!("{value:.6}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(if trimmed.is_empty() { "0" } else { trimmed }.as_bytes());
}

/// Writes one object.
pub fn write_object(out: &mut Vec<u8>, object: &Object, names: &NameTable) {
    write_object_at(out, object, names, 0);
}

fn write_object_at(out: &mut Vec<u8>, object: &Object, names: &NameTable, depth: u32) {
    if depth > crate::limits::MAX_NEST_DEPTH {
        out.extend_from_slice(b"null");
        return;
    }

    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Bool(true) => out.extend_from_slice(b"true"),
        Object::Bool(false) => out.extend_from_slice(b"false"),
        Object::Int(v) => out.extend_from_slice(v.to_string().as_bytes()),
        Object::Real(v) => write_real(out, *v),
        Object::String(s) => write_string(out, s),
        Object::Name(n) => {
            let bytes = names.bytes(*n).unwrap_or_default();
            write_name(out, &bytes);
        }
        Object::Ref(r) => {
            out.extend_from_slice(format!("{} {} R", r.num, r.gen).as_bytes());
        }
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object_at(out, item, names, depth + 1);
            }
            out.push(b']');
        }
        Object::Dict(dict) => write_dict(out, dict, names, depth),
        // A parsed stream refers to bytes in the document it came from, so it
        // cannot be written from the object alone. Emitting one needs its
        // data, which is what `Written::Stream` carries; here only the
        // dictionary is available and only that is written.
        Object::Stream(stream) => write_dict(out, &stream.dict, names, depth),
    }
}

fn write_dict(out: &mut Vec<u8>, dict: &Dict, names: &NameTable, depth: u32) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        let bytes = names.bytes(*key).unwrap_or_default();
        write_name(out, &bytes);
        out.push(b' ');
        write_object_at(out, value, names, depth + 1);
    }
    out.extend_from_slice(b">>");
}

/// A stream ready to be written: a dictionary and its already-encoded bytes.
#[derive(Clone, Debug, Default)]
pub struct StreamData {
    /// The stream dictionary, without `/Length`, which is written for it.
    pub dict: Dict,
    /// The encoded bytes.
    pub data: Vec<u8>,
}

/// One thing a writer emits.
#[derive(Clone, Debug)]
pub enum Written {
    /// A plain object.
    Object(Object),
    /// A stream, whose data the writer owns and whose `/Length` it computes.
    Stream(StreamData),
}

/// What a writer emits: object numbers to objects.
#[derive(Clone, Debug, Default)]
pub struct ObjectSet {
    entries: BTreeMap<u32, Written>,
}

impl ObjectSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> ObjectSet {
        ObjectSet::default()
    }

    /// Adds or replaces an object.
    pub fn insert(&mut self, num: u32, object: Object) {
        self.entries.insert(num, Written::Object(object));
    }

    /// Adds or replaces a stream.
    pub fn insert_stream(&mut self, num: u32, stream: StreamData) {
        self.entries.insert(num, Written::Stream(stream));
    }

    /// How many objects the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The highest object number, or zero.
    #[must_use]
    pub fn max_number(&self) -> u32 {
        self.entries.keys().next_back().copied().unwrap_or(0)
    }
}

/// Writes one numbered object, whichever kind it is.
fn write_entry(out: &mut Vec<u8>, num: u32, entry: &Written, names: &NameTable) {
    out.extend_from_slice(
        format!(
            "{num} 0 obj
"
        )
        .as_bytes(),
    );
    match entry {
        Written::Object(object) => write_object(out, object, names),
        Written::Stream(stream) => {
            // 7.3.8.2: /Length must agree with the bytes that follow, so the
            // writer computes it rather than trusting whatever was there.
            let mut dict = stream.dict.clone();
            dict.insert(Name::LENGTH, Object::Int(stream.data.len() as i64));
            write_dict(out, &dict, names, 0);
            out.extend_from_slice(
                b"
stream
",
            );
            out.extend_from_slice(&stream.data);
            out.extend_from_slice(
                b"
endstream",
            );
        }
    }
    out.extend_from_slice(
        b"
endobj
",
    );
}

/// Appends an incremental update to `original`.
///
/// The result **starts with `original`, byte for byte**. That invariant is
/// what lets a signed document be updated without breaking its signature, and
/// it is asserted by tests rather than assumed.
///
/// `trailer` supplies `/Root`, `/Info` and `/ID`; `/Prev` and `/Size` are
/// written for it.
#[must_use]
pub fn incremental_update(
    original: &[u8],
    changed: &ObjectSet,
    trailer: &Dict,
    previous_startxref: u64,
    names: &NameTable,
) -> Vec<u8> {
    let mut out = original.to_vec();

    // 7.5.6: an update begins on a new line so the appended section cannot be
    // mistaken for a continuation of whatever the original ended with.
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }

    let mut offsets: Vec<(u32, u64)> = Vec::with_capacity(changed.entries.len());
    for (num, object) in &changed.entries {
        offsets.push((*num, out.len() as u64));
        write_entry(&mut out, *num, object, names);
    }

    let xref_at = out.len() as u64;
    write_classic_xref(&mut out, &offsets);

    let mut trailer = trailer.clone();
    let size = changed.max_number().saturating_add(1);
    trailer.insert(Name::SIZE, Object::Int(i64::from(size)));
    trailer.insert(
        Name::PREV,
        Object::Int(i64::try_from(previous_startxref).unwrap_or(0)),
    );

    out.extend_from_slice(b"trailer\n");
    write_dict(&mut out, &trailer, names, 0);
    out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    out
}

/// Writes a whole document afresh.
#[must_use]
pub fn rewrite(
    objects: &ObjectSet,
    trailer: &Dict,
    options: &WriteOptions,
    names: &NameTable,
) -> Vec<u8> {
    let (major, minor) = options.version;
    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(format!("%PDF-{major}.{minor}\n").as_bytes());
    // 7.5.2: four bytes above 127 tell transfer software the file is binary.
    out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

    let mut offsets: Vec<(u32, u64)> = Vec::with_capacity(objects.entries.len());
    for (num, object) in &objects.entries {
        offsets.push((*num, out.len() as u64));
        write_entry(&mut out, *num, object, names);
    }

    let xref_at = out.len() as u64;
    write_classic_xref(&mut out, &offsets);

    // A rewrite has no earlier revision, so /Prev is simply not written; the
    // caller's trailer is not expected to carry one.
    let mut trailer = trailer.clone();
    trailer.insert(
        Name::SIZE,
        Object::Int(i64::from(objects.max_number().saturating_add(1))),
    );

    out.extend_from_slice(b"trailer\n");
    write_dict(&mut out, &trailer, names, 0);
    out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());

    out
}

/// Writes a classic cross-reference table (7.5.4).
///
/// Entries are grouped into contiguous subsections, which is what makes an
/// incremental update's table small: only the objects that changed appear.
fn write_classic_xref(out: &mut Vec<u8>, offsets: &[(u32, u64)]) {
    out.extend_from_slice(b"xref\n");

    if offsets.is_empty() {
        // A table with only the free head; the specification requires object
        // zero to exist and be free.
        out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
        return;
    }

    let mut index = 0usize;
    while index < offsets.len() {
        let start = match offsets.get(index) {
            Some((num, _)) => *num,
            None => break,
        };

        // Find how far the run of consecutive numbers extends.
        let mut end = index;
        while let (Some((a, _)), Some((b, _))) = (offsets.get(end), offsets.get(end + 1)) {
            if b.saturating_sub(*a) == 1 {
                end += 1;
            } else {
                break;
            }
        }

        let count = end - index + 1;
        // Object zero heads the free list, so a run starting at 1 must
        // include it or readers reject the table.
        if start == 1 {
            out.extend_from_slice(format!("0 {}\n", count + 1).as_bytes());
            out.extend_from_slice(b"0000000000 65535 f \n");
        } else {
            out.extend_from_slice(format!("{start} {count}\n").as_bytes());
        }

        for (_, offset) in offsets.get(index..=end).unwrap_or_default() {
            // 7.5.4: exactly twenty bytes per entry, including the trailing
            // two-character end-of-line.
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        index = end + 1;
    }
}

/// Where a signature's byte range would go, reserved so it can be patched.
///
/// 12.8.1 needs a signature's `/Contents` to be a hex string large enough for
/// the eventual signature, and its `/ByteRange` to name the spans of the file
/// the signature covers — which cannot be known until the file is laid out.
/// Reserving both and patching afterwards is the only way round the circle,
/// and this records where to patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignaturePlaceholder {
    /// Byte offset of the `/Contents` hex string, at its opening angle.
    pub contents_at: usize,
    /// How many bytes the placeholder occupies, brackets included.
    pub contents_len: usize,
    /// Byte offset of the `/ByteRange` array.
    pub byte_range_at: usize,
    /// How many bytes that array occupies.
    pub byte_range_len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjRef;

    fn names() -> NameTable {
        NameTable::new()
    }

    fn rendered(object: &Object) -> String {
        let mut out = Vec::new();
        write_object(&mut out, object, &names());
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn scalars_write_as_the_syntax_says() {
        assert_eq!(rendered(&Object::Null), "null");
        assert_eq!(rendered(&Object::Bool(true)), "true");
        assert_eq!(rendered(&Object::Int(-42)), "-42");
        assert_eq!(rendered(&Object::Ref(ObjRef::new(3, 0))), "3 0 R");
    }

    #[test]
    fn reals_never_use_an_exponent() {
        // PDF has no exponent notation, so 1e-7 must not be written as such.
        assert_eq!(rendered(&Object::Real(0.5)), "0.5");
        assert_eq!(rendered(&Object::Real(2.0)), "2");
        assert_eq!(rendered(&Object::Real(-0.25)), "-0.25");
        assert!(!rendered(&Object::Real(1e-7)).contains('e'));
        assert!(!rendered(&Object::Real(1e20)).contains('e'));
        // Non-finite values cannot be written at all, so they become zero.
        assert_eq!(rendered(&Object::Real(f64::NAN)), "0");
        assert_eq!(rendered(&Object::Real(f64::INFINITY)), "0");
    }

    #[test]
    fn strings_escape_only_what_they_must() {
        assert_eq!(
            rendered(&Object::String(PdfString::literal(b"plain".to_vec()))),
            "(plain)"
        );
        assert_eq!(
            rendered(&Object::String(PdfString::literal(b"a(b)c\\d".to_vec()))),
            r"(a\(b\)c\\d)"
        );
    }

    #[test]
    fn names_escape_delimiters_and_whitespace() {
        let table = names();
        let mut out = Vec::new();
        write_object(&mut out, &Object::Name(table.intern(b"A B")), &table);
        assert_eq!(String::from_utf8_lossy(&out), "/A#20B");

        let mut out = Vec::new();
        write_object(&mut out, &Object::Name(table.intern(b"Plain")), &table);
        assert_eq!(String::from_utf8_lossy(&out), "/Plain");
    }

    #[test]
    fn a_streams_length_is_written_for_it() {
        let table = names();
        let mut out = Vec::new();
        write_entry(
            &mut out,
            4,
            &Written::Stream(StreamData {
                dict: Dict::new(),
                data: b"hello".to_vec(),
            }),
            &table,
        );
        let text = String::from_utf8_lossy(&out);

        assert!(text.contains("/Length 5"), "got {text}");
        assert!(text.contains("stream\nhello\nendstream"), "got {text}");
    }

    #[test]
    fn a_rewrite_produces_a_readable_document() {
        let table = names();
        let mut objects = ObjectSet::new();

        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        objects.insert(1, Object::Dict(catalog));

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let bytes = rewrite(&objects, &trailer, &WriteOptions::default(), &table);
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.starts_with("%PDF-1.7"), "a header");
        assert!(text.contains("1 0 obj"), "the object");
        assert!(text.contains("xref"), "a table");
        assert!(text.contains("/Root 1 0 R"), "a trailer");
        assert!(text.ends_with("%%EOF\n"), "and an end marker");

        // And it reopens.
        let doc = crate::CosDocument::open(bytes.clone()).expect("the output reopens");
        assert!(doc.catalog().is_some(), "with a catalog");
    }

    /// The invariant phase 10's signing depends on.
    #[test]
    fn an_incremental_update_preserves_the_original_bytes_exactly() {
        let table = names();
        let mut objects = ObjectSet::new();
        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(table.intern(b"Catalog")));
        objects.insert(1, Object::Dict(catalog));
        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));

        let original = rewrite(&objects, &trailer, &WriteOptions::default(), &table);

        let mut changed = ObjectSet::new();
        changed.insert(2, Object::Int(42));
        let updated = incremental_update(&original, &changed, &trailer, 0, &table);

        assert!(
            updated.starts_with(&original),
            "the original must survive byte for byte"
        );
        assert!(updated.len() > original.len(), "and something was appended");

        let text = String::from_utf8_lossy(&updated);
        assert!(text.contains("/Prev"), "the update chains to the old table");
        assert!(text.matches("%%EOF").count() >= 2, "two revisions");

        // The updated file still opens, and sees the new object.
        let doc = crate::CosDocument::open(updated).expect("the update reopens");
        assert_eq!(
            doc.get(ObjRef::new(2, 0)).ok().as_deref(),
            Some(&Object::Int(42))
        );
    }

    #[test]
    fn the_cross_reference_table_always_has_its_free_head() {
        let mut out = Vec::new();
        write_classic_xref(&mut out, &[(1, 100), (2, 200)]);
        let text = String::from_utf8_lossy(&out);

        assert!(text.starts_with("xref\n0 3\n"), "got {text}");
        assert!(text.contains("0000000000 65535 f \n"), "the free head");
        assert!(text.contains("0000000100 00000 n \n"), "object one");

        // Every entry is exactly twenty bytes.
        for line in text.lines().skip(2) {
            if line.ends_with(" n ") || line.ends_with(" f ") {
                assert_eq!(line.len() + 1, 20, "entry {line:?} is not 20 bytes");
            }
        }
    }

    #[test]
    fn non_consecutive_objects_become_separate_subsections() {
        let mut out = Vec::new();
        write_classic_xref(&mut out, &[(1, 10), (2, 20), (7, 70)]);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("0 3\n"), "the first run, with the free head");
        assert!(text.contains("7 1\n"), "and a second subsection: {text}");
    }

    #[test]
    fn deeply_nested_objects_do_not_overflow_the_stack() {
        let mut object = Object::Int(0);
        for _ in 0..(crate::limits::MAX_NEST_DEPTH + 100) {
            object = Object::Array(vec![object]);
        }
        let mut out = Vec::new();
        write_object(&mut out, &object, &names());
        assert!(!out.is_empty());
    }
}
