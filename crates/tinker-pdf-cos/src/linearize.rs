//! Linearized output (Annex F): the first page at the front of the file.
//!
//! A linearized file is laid out so a reader that has received only its first
//! few kilobytes can already draw page one — the objects that page needs come
//! first, and a cross-reference table for exactly those objects sits ahead of
//! them. Over HTTP range requests that is the difference between a document
//! that appears immediately and one that appears when the last byte lands.
//!
//! This is the phase's superiority item: MuPDF 1.26 removed linearized
//! output, and Tinker's own plans had to shell out to qpdf for it.
//!
//! # Why this needs no patching pass
//!
//! Every offset in the file is known before a byte is written, because
//! nothing whose *size* matters depends on an offset:
//!
//! - The parameter dictionary's integers are written to a fixed width, so
//!   their values cannot change its length.
//! - A classic cross-reference table has fixed-width entries, so its length
//!   follows from the entry count alone.
//! - The hint tables' bit widths are derived from object counts and object
//!   lengths — never from an absolute position — so the hint stream's length
//!   is known once the objects are serialised.
//!
//! So the writer serialises every object, computes every length, derives
//! every offset, and only then emits. A two-pass writer with a patch-up phase
//! is the usual approach and this deliberately avoids it: a patch that misses
//! one field produces a file that opens fine everywhere except in the reader
//! the whole feature exists for.
//!
//! # Encryption: encrypt first, then measure
//!
//! Encryption looks like it breaks that premise. AES-256-CBC prefixes a
//! 16-byte initialisation vector and pads to the block size, so an encrypted
//! stream is 17 to 32 bytes longer than its plaintext by an amount that
//! depends on the plaintext's length — a size that is not known from the
//! object alone.
//!
//! It is only a breakage if the measuring happens first. Every object is
//! serialised *and encrypted* in one pass, in [`Plan::build`], and the layout
//! is computed from the lengths of the encrypted bytes. Nothing else changes
//! and the no-patching property survives. The opposite order is the failure
//! this arrangement exists to prevent: every offset after the first stream
//! would be short by the accumulated padding, and the file would open in this
//! engine — whose reader walks the subsection headers — while failing in any
//! reader that trusts `/L`, `/H` or `/T`, which is exactly the population
//! linearization is for.
//!
//! Three things stay in the clear, because a reader reaches them before it can
//! decrypt anything:
//!
//! - The `/Encrypt` dictionary (7.6.1), which is object 3 here and the first
//!   object in part 4.
//! - Both cross-reference tables and their trailers (7.6.1): a reader finds
//!   them before it knows there is an `/Encrypt` dictionary to look for.
//! - The linearization parameter dictionary. 7.6.1 does not exempt it, but a
//!   reader consults it *before* authenticating. Strings inside it would have
//!   to be encrypted, and it contains none — every value is an integer or an
//!   array of integers — so the question is moot. `parameter_dictionary`'s
//!   own test asserts that rather than assuming it.
//!
//! The hint stream is *not* in that list. It is an ordinary stream object and
//! it is encrypted; `/H` gives its offset and length in the file, which is the
//! encrypted length.
//!
//! # What is not here
//!
//! The optional generic hint tables — outlines, threads, named destinations —
//! are omitted, which F.4 permits. Only the page-offset and shared-object
//! tables are required and only those are written.

use std::collections::{BTreeMap, BTreeSet};

use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object};
use crate::write::{write_object, ObjectSet, StreamCipher, StreamData, WriteOptions, Written};

/// How many digits every patchable integer in the parameter dictionary gets.
///
/// Ten covers any file a `u32` offset can describe, and writing them to a
/// fixed width is what removes the need for a patching pass. Leading zeros in
/// an integer are legal (7.3.3).
const FIELD_WIDTH: usize = 10;

/// The object number the `/Encrypt` dictionary takes, when there is one.
///
/// Objects 1 and 2 are the parameter dictionary and the hint stream. The
/// third reserved number matters more than it looks: the first-page
/// cross-reference table declares a single subsection running from zero to
/// its highest entry, and marks every number in that range it does not carry
/// as *free*. A free entry in the newer table overrides the main table
/// reached through `/Prev`, so numbering `/Encrypt` above the ordinary
/// objects — which is what the unlinearized writer does — would put it in the
/// front table's range and free everything between. Reserving a low number
/// keeps the front table's range exactly the front of the file.
const ENCRYPT_OBJECT: u32 = 3;

/// Writes a linearized file, or returns `None` when the document has no shape
/// to linearize around.
///
/// A document with no catalog or no pages cannot be linearized — there is no
/// "first page" to put first — and the caller falls back to an ordinary
/// rewrite rather than emitting a file that claims `/Linearized` and is not.
#[must_use]
pub fn linearize(
    objects: &ObjectSet,
    trailer: &Dict,
    options: &WriteOptions,
    names: &NameTable,
) -> Option<Vec<u8>> {
    // The cipher is built before a single object is serialised, because every
    // length this layout depends on is a length of encrypted bytes.
    let encryption = match options.encryption.as_ref() {
        Some(request) => Some(crate::write::build_encryption(request, names)?),
        None => None,
    };
    let plan = Plan::build(objects, trailer, names, options.compress, encryption)?;
    Some(plan.emit(options, names))
}

/// Which section of the file an object belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Part 4: the catalog, the page tree, and the rest of the document-level
    /// objects a reader needs before it can do anything at all.
    Document,
    /// Part 5: the first page and everything only it uses.
    FirstPage,
    /// Part 8: objects more than one page needs.
    Shared,
    /// Parts 7 and 9: later pages and everything else.
    Rest,
}

struct Plan {
    /// Objects in output order, already renumbered, with their bytes.
    ordered: Vec<Placed>,
    /// New number of the first page's page object, which is `/O`.
    first_page_object: u32,
    /// How many pages the document has, which is `/N`.
    page_count: u32,
    /// New numbers of the objects in part 8.
    shared_objects: Vec<u32>,
    /// Serialised lengths of each page's own objects, for the hint tables.
    page_lengths: Vec<u32>,
    /// How many objects each page owns.
    page_object_counts: Vec<u32>,
    trailer: Dict,
    /// The highest new object number, plus the two reserved ones.
    size: u32,
    /// The cipher, when the file is encrypted. Held so the hint stream — an
    /// ordinary stream object, and encrypted like one — can be built in
    /// `emit`, where its dictionary is known.
    crypt: Option<StreamCipher>,
}

/// One object, renumbered and serialised.
struct Placed {
    number: u32,
    section: Section,
    bytes: Vec<u8>,
}

impl Plan {
    fn build(
        objects: &ObjectSet,
        trailer: &Dict,
        names: &NameTable,
        compress: bool,
        encryption: Option<(Dict, StreamCipher)>,
    ) -> Option<Plan> {
        let root = trailer.get_ref(Name::ROOT)?;
        let catalog = dict_of(objects, root.num)?;
        let pages_ref = catalog.get_ref(Name::PAGES)?;

        let pages = collect_pages(objects, pages_ref.num);
        if pages.is_empty() {
            return None;
        }

        // Which objects each page reaches, so an object two pages use can be
        // told from one only the first page uses.
        let per_page: Vec<BTreeSet<u32>> = pages
            .iter()
            .map(|page| reachable_from(objects, *page))
            .collect();

        // The document-level set is everything the trailer reaches without
        // going through a page: the catalog, the page tree, `/Info`, the
        // name trees. A reader needs all of it before the first page means
        // anything.
        let tree: BTreeSet<u32> = page_tree_nodes(objects, pages_ref.num);
        let mut document: BTreeSet<u32> = BTreeSet::new();
        for (_, value) in trailer.iter() {
            for r in refs_of(value) {
                document.extend(reachable_avoiding(objects, r.num, &tree, &pages));
            }
        }
        document.extend(tree.iter().copied());

        // An object more than one page uses is shared; one only the first
        // page uses travels with it.
        let mut users: BTreeMap<u32, u32> = BTreeMap::new();
        for set in &per_page {
            for num in set {
                *users.entry(*num).or_insert(0) += 1;
            }
        }

        let mut section: BTreeMap<u32, Section> = BTreeMap::new();
        for num in objects.numbers() {
            section.insert(num, Section::Rest);
        }
        for num in &document {
            section.insert(*num, Section::Document);
        }
        for (num, count) in &users {
            if document.contains(num) {
                continue;
            }
            let shared = *count > 1;
            let first = per_page.first().is_some_and(|set| set.contains(num));
            section.insert(
                *num,
                match (shared, first) {
                    (true, _) => Section::Shared,
                    (false, true) => Section::FirstPage,
                    (false, false) => Section::Rest,
                },
            );
        }

        // Output order, and with it the new numbering. Object 1 is the
        // parameter dictionary and object 2 the hint stream; both are
        // reserved here and written by `emit`.
        let mut order: Vec<(u32, Section)> = Vec::new();
        for want in [
            Section::Document,
            Section::FirstPage,
            Section::Rest,
            Section::Shared,
        ] {
            for num in objects.numbers() {
                if section.get(&num) == Some(&want) {
                    order.push((num, want));
                }
            }
        }

        let mut mapping: BTreeMap<u32, u32> = BTreeMap::new();
        let mut next = if encryption.is_some() {
            ENCRYPT_OBJECT.saturating_add(1)
        } else {
            ENCRYPT_OBJECT
        };
        for (old, _) in &order {
            mapping.insert(*old, next);
            next = next.saturating_add(1);
        }

        // F.3.1: `/O` names the first page's page object.
        let first_page_object = *mapping.get(pages.first()?)?;

        let crypt = encryption.as_ref().map(|(_, cipher)| cipher);

        let mut ordered = Vec::with_capacity(order.len() + 1);
        // 7.6.1: the `/Encrypt` dictionary is the one object never encrypted,
        // and it leads part 4 so a reader that has the front of the file can
        // authenticate before it needs anything else.
        if let Some((dict, _)) = &encryption {
            let mut bytes = Vec::new();
            write_indirect(
                &mut bytes,
                ENCRYPT_OBJECT,
                &Written::Object(Object::Dict(dict.clone())),
                names,
                false,
                None,
            );
            ordered.push(Placed {
                number: ENCRYPT_OBJECT,
                section: Section::Document,
                bytes,
            });
        }

        let mut shared_objects = Vec::new();
        for (old, section) in &order {
            let entry = objects.get(*old)?;
            let renumbered = renumber_entry(entry, &mapping);
            let number = *mapping.get(old)?;
            let mut bytes = Vec::new();
            write_indirect(&mut bytes, number, &renumbered, names, compress, crypt);

            if section == &Section::Shared {
                shared_objects.push(number);
            }
            ordered.push(Placed {
                number,
                section: *section,
                bytes,
            });
        }

        // Per-page sizes, for the page-offset hint table. A page's "length"
        // is the bytes of the objects it owns, which is what a reader needs
        // to have received before it can draw the page.
        let mut page_lengths = Vec::with_capacity(pages.len());
        let mut page_object_counts = Vec::with_capacity(pages.len());
        let sizes: BTreeMap<u32, u32> = ordered
            .iter()
            .map(|placed| (placed.number, placed.bytes.len() as u32))
            .collect();
        for set in &per_page {
            let owned: Vec<u32> = set
                .iter()
                .filter(|num| !document.contains(num))
                .filter(|num| section.get(num) != Some(&Section::Shared))
                .filter_map(|num| mapping.get(num).copied())
                .collect();
            page_object_counts.push(owned.len() as u32);
            page_lengths.push(owned.iter().filter_map(|n| sizes.get(n)).sum());
        }

        let mut trailer = renumber_dict(trailer, &mapping);
        trailer.insert(Name::SIZE, Object::Int(i64::from(next)));
        if encryption.is_some() {
            trailer.insert(Name::ENCRYPT, Object::Ref(ObjRef::new(ENCRYPT_OBJECT, 0)));
        }

        Some(Plan {
            ordered,
            first_page_object,
            page_count: pages.len() as u32,
            shared_objects,
            page_lengths,
            page_object_counts,
            trailer,
            size: next,
            crypt: encryption.map(|(_, cipher)| cipher),
        })
    }

    /// Lays the parts out and writes them.
    fn emit(&self, options: &WriteOptions, names: &NameTable) -> Vec<u8> {
        let (major, minor) = options.version;
        let mut header = format!("%PDF-{major}.{minor}\n").into_bytes();
        header.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

        // The hint stream's bytes depend only on counts and lengths, so they
        // can be built now and their size relied on below.
        let (hint_data, shared_at) = self.hint_tables();
        let mut hint_dict = Dict::new();
        hint_dict.insert(names.intern(b"S"), Object::Int(shared_at as i64));
        let hint_bytes = {
            let mut bytes = Vec::new();
            write_indirect(
                &mut bytes,
                2,
                &Written::Stream(StreamData {
                    dict: hint_dict,
                    data: hint_data,
                }),
                names,
                // Never compressed: `/H` measures this object's length, and
                // shrinking it after the layout was computed from it would
                // point every later offset somewhere else.
                false,
                // Encrypted, though. The hint stream is an ordinary stream
                // object and 7.6.1 exempts only the three things a reader
                // must read before it can decrypt; this is not one of them.
                // `/H` is measured from these bytes, so it carries the
                // encrypted length.
                self.crypt.as_ref(),
            );
            bytes
        };

        // Part 2 is a fixed size because every integer in it is written to a
        // fixed width, which is the whole reason this needs no second pass.
        let parameters = self.parameter_dictionary(0, 0, 0, 0, 0, names);
        let part2_len = parameters.len();

        // Part 3 covers the objects in parts 4 to 6: the document-level
        // objects, the first page, and the hint stream itself.
        let front: Vec<&Placed> = self
            .ordered
            .iter()
            .filter(|p| matches!(p.section, Section::Document | Section::FirstPage))
            .collect();
        let back: Vec<&Placed> = self
            .ordered
            .iter()
            .filter(|p| !matches!(p.section, Section::Document | Section::FirstPage))
            .collect();

        // Every entry is twenty bytes, so the table's size follows from the
        // count alone (7.5.4).
        let first_xref_len =
            subsection_len(front.len() + 2) + self.trailer_bytes(Some(0), names).len();

        let part1 = header.len();
        let part2_at = part1;
        let part3_at = part2_at + part2_len;
        let part4_at = part3_at + first_xref_len;

        let mut at = part4_at;
        let mut offsets: BTreeMap<u32, u64> = BTreeMap::new();
        for placed in &front {
            offsets.insert(placed.number, at as u64);
            at += placed.bytes.len();
        }
        let hint_at = at;
        offsets.insert(2, hint_at as u64);
        at += hint_bytes.len();

        // `/E` is where the first page's material ends, which is the end of
        // the hint stream: everything before it is what page one needs.
        let end_of_first_page = at;

        for placed in &back {
            offsets.insert(placed.number, at as u64);
            at += placed.bytes.len();
        }

        let main_xref_at = at;
        let main_xref = self.main_xref(&back, &offsets, names);
        at += main_xref.len();

        // The last line points back at the *first-page* table near the front,
        // which is what lets a reader with the file's tail resolve page one
        // without the middle (F.3.4).
        let tail = format!("startxref\n{part3_at}\n%%EOF\n");
        let total = at + tail.len();

        let parameters = self.parameter_dictionary(
            total as u64,
            hint_at as u64,
            hint_bytes.len() as u64,
            end_of_first_page as u64,
            main_xref_at as u64,
            names,
        );
        debug_assert_eq!(parameters.len(), part2_len, "the dictionary is fixed width");

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&header);
        out.extend_from_slice(&parameters);

        let mut front_offsets: Vec<(u32, u64)> = front
            .iter()
            .filter_map(|p| offsets.get(&p.number).map(|at| (p.number, *at)))
            .collect();
        front_offsets.push((1, part1 as u64));
        front_offsets.push((2, hint_at as u64));
        front_offsets.sort_unstable();
        self.write_first_xref(&mut out, &front_offsets, main_xref_at as u64, names);
        debug_assert_eq!(out.len(), part4_at, "part 4 begins where it was placed");

        for placed in &front {
            out.extend_from_slice(&placed.bytes);
        }
        out.extend_from_slice(&hint_bytes);
        for placed in &back {
            out.extend_from_slice(&placed.bytes);
        }
        out.extend_from_slice(&main_xref);
        out.extend_from_slice(tail.as_bytes());
        out
    }

    /// Part 2: the linearization parameter dictionary (F.2.2).
    fn parameter_dictionary(
        &self,
        length: u64,
        hint_at: u64,
        hint_len: u64,
        end_of_first_page: u64,
        main_xref_at: u64,
        names: &NameTable,
    ) -> Vec<u8> {
        let _ = names;
        let mut out = Vec::new();
        out.extend_from_slice(b"1 0 obj\n<< /Linearized 1 /L ");
        pad(&mut out, length);
        out.extend_from_slice(b" /H [ ");
        pad(&mut out, hint_at);
        out.push(b' ');
        pad(&mut out, hint_len);
        out.extend_from_slice(b" ] /O ");
        pad(&mut out, u64::from(self.first_page_object));
        out.extend_from_slice(b" /E ");
        pad(&mut out, end_of_first_page);
        out.extend_from_slice(b" /N ");
        pad(&mut out, u64::from(self.page_count));
        out.extend_from_slice(b" /T ");
        pad(&mut out, main_xref_at);
        out.extend_from_slice(b" >>\nendobj\n");
        out
    }

    /// Part 3: a cross-reference table for the objects ahead of it, whose
    /// trailer points onward to the main table at the end.
    fn write_first_xref(
        &self,
        out: &mut Vec<u8>,
        entries: &[(u32, u64)],
        main_xref_at: u64,
        names: &NameTable,
    ) {
        out.extend_from_slice(b"xref\n");
        write_subsection(out, entries);

        out.extend_from_slice(&self.trailer_bytes(Some(main_xref_at), names));
    }

    /// The trailer, with `/Prev` written to a fixed width.
    ///
    /// `/Prev` names the main table at the end of the file, whose offset is
    /// not known until the layout is complete — but the trailer's *length*
    /// has to be known before it, because it sits ahead of everything the
    /// offset depends on. Writing the value to a fixed width breaks that
    /// circle. Letting the number size itself is what this did first: the
    /// prediction was six bytes long, every offset after it was wrong, and
    /// only the assertion in `emit` said so.
    fn trailer_bytes(&self, prev: Option<u64>, names: &NameTable) -> Vec<u8> {
        let mut trailer = self.trailer.clone();
        trailer.insert(Name::SIZE, Object::Int(i64::from(self.size)));

        let mut out = Vec::new();
        out.extend_from_slice(b"trailer\n");
        write_object(&mut out, &Object::Dict(trailer), names);

        if let Some(prev) = prev {
            // The dictionary ends in `>>`; the padded entry goes inside it.
            debug_assert!(out.ends_with(b">>"), "a dictionary was written");
            out.truncate(out.len() - 2);
            out.extend_from_slice(b" /Prev ");
            pad(&mut out, prev);
            out.extend_from_slice(b" >>");
        }
        out.push(b'\n');
        out
    }

    /// Part 10: the main table, covering everything the first one did not.
    fn main_xref(
        &self,
        back: &[&Placed],
        offsets: &BTreeMap<u32, u64>,
        names: &NameTable,
    ) -> Vec<u8> {
        let mut entries: Vec<(u32, u64)> = back
            .iter()
            .filter_map(|p| offsets.get(&p.number).map(|at| (p.number, *at)))
            .collect();
        entries.sort_unstable();

        let mut out = Vec::new();
        out.extend_from_slice(b"xref\n");
        write_subsection(&mut out, &entries);
        out.extend_from_slice(&self.trailer_bytes(None, names));
        out
    }

    /// The primary hint stream's data, and where the shared-object table
    /// starts within it (F.4).
    fn hint_tables(&self) -> (Vec<u8>, usize) {
        let mut bits = BitWriter::default();

        // ---- Page offset hint table (F.4.1) ----
        let least_objects = self.page_object_counts.iter().copied().min().unwrap_or(0);
        let most_objects = self.page_object_counts.iter().copied().max().unwrap_or(0);
        let object_bits = bits_for(most_objects.saturating_sub(least_objects));

        let least_length = self.page_lengths.iter().copied().min().unwrap_or(0);
        let most_length = self.page_lengths.iter().copied().max().unwrap_or(0);
        let length_bits = bits_for(most_length.saturating_sub(least_length));

        let shared_bits = bits_for(self.shared_objects.len() as u32);

        bits.write(least_objects, 32); // 1
        bits.write(self.first_page_object, 32); // 2
        bits.write(u32::from(object_bits), 16); // 3
        bits.write(least_length, 32); // 4
        bits.write(u32::from(length_bits), 16); // 5
        bits.write(0, 32); // 6: least content-stream offset
        bits.write(0, 16); // 7
        bits.write(0, 32); // 8: least content-stream length
        bits.write(0, 16); // 9
        bits.write(u32::from(shared_bits), 16); // 10
        bits.write(u32::from(shared_bits), 16); // 11
        bits.write(0, 16); // 12: fractional position numerator
        bits.write(1, 16); // 13: its denominator

        for index in 0..self.page_count as usize {
            let objects = self.page_object_counts.get(index).copied().unwrap_or(0);
            let length = self.page_lengths.get(index).copied().unwrap_or(0);
            bits.write(objects.saturating_sub(least_objects), object_bits);
            bits.write(length.saturating_sub(least_length), length_bits);
            // No per-page shared references are recorded: the shared section
            // is described by its own table below, and F.4.1 allows a page to
            // declare none. A reader uses this to prefetch, never to resolve.
            bits.write(0, shared_bits);
            bits.write(0, 0);
            bits.write(0, 0);
        }
        bits.align();
        let shared_at = bits.len();

        // ---- Shared object hint table (F.4.2) ----
        let first_shared = self.shared_objects.first().copied().unwrap_or(0);
        bits.write(first_shared, 32); // 1: first shared object number
        bits.write(0, 32); // 2: its location, relative and unused here
        bits.write(0, 32); // 3: entries for the first page
        bits.write(self.shared_objects.len() as u32, 32); // 4: entries in all
        bits.write(0, 16); // 5: bits for the group-object count
        bits.write(0, 32); // 6: least group length
        bits.write(0, 16); // 7: bits for the group-length delta
        for _ in &self.shared_objects {
            bits.write(0, 0);
        }
        bits.align();

        (bits.finish(), shared_at)
    }
}

/// Writes an integer right-aligned in a fixed field, so its value cannot
/// change the length of what surrounds it.
fn pad(out: &mut Vec<u8>, value: u64) {
    let text = value.to_string();
    for _ in text.len()..FIELD_WIDTH {
        out.push(b'0');
    }
    out.extend_from_slice(text.as_bytes());
}

/// How many bytes one cross-reference subsection occupies, given its entries.
fn subsection_len(entries: usize) -> usize {
    // "xref\n", then "0 N\n", then twenty bytes an entry including the free
    // head. The header line's width depends on the count's digits.
    let count = entries + 1;
    5 + format!("0 {count}\n").len() + count * 20
}

fn write_subsection(out: &mut Vec<u8>, entries: &[(u32, u64)]) {
    let highest = entries.last().map_or(0, |(num, _)| *num);
    let count = usize::try_from(highest).unwrap_or(0) + 1;
    out.extend_from_slice(format!("0 {count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");

    let map: BTreeMap<u32, u64> = entries.iter().copied().collect();
    for number in 1..count as u32 {
        match map.get(&number) {
            Some(at) => {
                out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
            }
            // A number this table does not cover is written free; the other
            // table carries it, and a reader merges the two through /Prev.
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
}

/// Bits needed to represent `value`, at least one.
fn bits_for(value: u32) -> u16 {
    if value == 0 {
        return 1;
    }
    (32 - value.leading_zeros()) as u16
}

/// Packs fields of arbitrary bit width, most significant bit first (F.4).
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    partial: u8,
    used: u32,
}

impl BitWriter {
    fn write(&mut self, value: u32, width: u16) {
        for i in (0..width).rev() {
            let bit = if i < 32 { (value >> i) & 1 } else { 0 };
            self.partial = (self.partial << 1) | bit as u8;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.partial);
                self.partial = 0;
                self.used = 0;
            }
        }
    }

    /// Pads to a byte boundary, which every table must start on.
    fn align(&mut self) {
        while self.used != 0 {
            self.write(0, 1);
        }
    }

    fn len(&self) -> usize {
        self.out.len()
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

// ---- Walking and renumbering ----------------------------------------------

fn dict_of(objects: &ObjectSet, num: u32) -> Option<Dict> {
    match objects.get(num)? {
        Written::Object(Object::Dict(dict)) => Some(dict.clone()),
        Written::Stream(stream) => Some(stream.dict.clone()),
        _ => None,
    }
}

fn refs_of(object: &Object) -> Vec<ObjRef> {
    let mut out = Vec::new();
    collect_refs(object, &mut out, 0);
    out
}

fn collect_refs(object: &Object, out: &mut Vec<ObjRef>, depth: u32) {
    if depth > crate::limits::MAX_NEST_DEPTH {
        return;
    }
    match object {
        Object::Ref(r) => out.push(*r),
        Object::Array(items) => {
            for item in items {
                collect_refs(item, out, depth + 1);
            }
        }
        Object::Dict(dict) => {
            for (_, value) in dict.iter() {
                collect_refs(value, out, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                collect_refs(value, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn entry_refs(entry: &Written) -> Vec<ObjRef> {
    match entry {
        Written::Object(object) => refs_of(object),
        Written::Stream(stream) => refs_of(&Object::Dict(stream.dict.clone())),
    }
}

/// The page objects, in order, from a page tree.
fn collect_pages(objects: &ObjectSet, tree: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    walk_pages(objects, tree, &mut out, &mut seen, 0);
    out
}

fn walk_pages(
    objects: &ObjectSet,
    node: u32,
    out: &mut Vec<u32>,
    seen: &mut BTreeSet<u32>,
    depth: u32,
) {
    if depth > crate::limits::MAX_NEST_DEPTH || !seen.insert(node) {
        return;
    }
    let Some(dict) = dict_of(objects, node) else {
        return;
    };
    match dict.get(Name::KIDS) {
        Some(Object::Array(kids)) => {
            for kid in kids.clone() {
                if let Object::Ref(r) = kid {
                    walk_pages(objects, r.num, out, seen, depth + 1);
                }
            }
        }
        _ => out.push(node),
    }
}

/// Every object number in the page tree itself, which is document-level
/// however deeply nested.
fn page_tree_nodes(objects: &ObjectSet, tree: u32) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let mut queue = vec![tree];
    let mut depth = 0u32;
    while let Some(node) = queue.pop() {
        depth += 1;
        if depth > 1 << 16 || !out.insert(node) {
            continue;
        }
        let Some(dict) = dict_of(objects, node) else {
            continue;
        };
        if let Some(Object::Array(kids)) = dict.get(Name::KIDS) {
            for kid in kids.clone() {
                if let Object::Ref(r) = kid {
                    if dict_of(objects, r.num).is_some_and(|d| d.get(Name::KIDS).is_some()) {
                        queue.push(r.num);
                    }
                }
            }
        }
    }
    out
}

/// Everything a page needs, not counting the page tree above it.
///
/// `/Parent` points back up the tree, and following it reaches the tree node,
/// then its other kids, then their contents — so every page's set becomes the
/// whole document, every object looks shared, and the first-page section
/// comes out empty. The file still opens; it is simply not linearized, which
/// is the one thing it claims to be. Removing the parent afterwards is not
/// enough, because by then the walk has already gone through it.
fn reachable_from(objects: &ObjectSet, page: u32) -> BTreeSet<u32> {
    let mut live = BTreeSet::new();
    let mut queue = vec![page];
    while let Some(num) = queue.pop() {
        if !live.insert(num) {
            continue;
        }
        let Some(entry) = objects.get(num) else {
            continue;
        };
        for r in entry_refs_downward(entry) {
            if !live.contains(&r.num) {
                queue.push(r.num);
            }
        }
    }
    live
}

/// An object's references, skipping the ones that point back up the tree.
fn entry_refs_downward(entry: &Written) -> Vec<ObjRef> {
    let dict = match entry {
        Written::Object(Object::Dict(dict)) => dict.clone(),
        Written::Stream(stream) => stream.dict.clone(),
        Written::Object(object) => return refs_of(object),
    };

    let mut out = Vec::new();
    for (key, value) in dict.iter() {
        if *key == Name::PARENT {
            continue;
        }
        collect_refs(value, &mut out, 0);
    }
    out
}

/// Reachability that stops at the page tree and at page objects, so the
/// document-level walk does not swallow the pages.
fn reachable_avoiding(
    objects: &ObjectSet,
    start: u32,
    tree: &BTreeSet<u32>,
    pages: &[u32],
) -> BTreeSet<u32> {
    let mut live = BTreeSet::new();
    let mut queue = vec![start];
    while let Some(num) = queue.pop() {
        if pages.contains(&num) || !live.insert(num) {
            continue;
        }
        if tree.contains(&num) {
            continue;
        }
        let Some(entry) = objects.get(num) else {
            continue;
        };
        for r in entry_refs(entry) {
            if !live.contains(&r.num) {
                queue.push(r.num);
            }
        }
    }
    live
}

fn renumber_entry(entry: &Written, mapping: &BTreeMap<u32, u32>) -> Written {
    match entry {
        Written::Object(object) => Written::Object(renumber(object, mapping, 0)),
        Written::Stream(stream) => Written::Stream(StreamData {
            dict: renumber_dict(&stream.dict, mapping),
            data: stream.data.clone(),
        }),
    }
}

fn renumber_dict(dict: &Dict, mapping: &BTreeMap<u32, u32>) -> Dict {
    let mut out = Dict::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        out.insert(*key, renumber(value, mapping, 0));
    }
    out
}

fn renumber(object: &Object, mapping: &BTreeMap<u32, u32>, depth: u32) -> Object {
    if depth > crate::limits::MAX_NEST_DEPTH {
        return Object::Null;
    }
    match object {
        // A reference to an object that did not survive becomes null, which
        // is what a reader would resolve a dangling reference to anyway.
        Object::Ref(r) => match mapping.get(&r.num) {
            Some(new) => Object::Ref(ObjRef::new(*new, 0)),
            None => Object::Null,
        },
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|item| renumber(item, mapping, depth + 1))
                .collect(),
        ),
        Object::Dict(dict) => {
            let mut out = Dict::with_capacity(dict.len());
            for (key, value) in dict.iter() {
                out.insert(*key, renumber(value, mapping, depth + 1));
            }
            Object::Dict(out)
        }
        other => other.clone(),
    }
}

/// One indirect object, header to `endobj`.
///
/// `crypt` is applied here rather than to the finished file, which is what
/// makes the layout measurable: the caller takes `out.len()` afterwards and
/// gets the length of what will actually be in the file.
fn write_indirect(
    out: &mut Vec<u8>,
    number: u32,
    entry: &Written,
    names: &NameTable,
    compress: bool,
    crypt: Option<&StreamCipher>,
) {
    out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
    match entry {
        // 7.6.2: every string in the file is encrypted too, not only the
        // streams. Leaving them clear puts titles, form values and annotation
        // contents in plain sight inside a file that claims to be encrypted —
        // and the ciphertext is longer than the plaintext, so this changes the
        // object's length as well as its contents.
        Written::Object(object) => match crypt {
            Some(cipher) => write_object(out, &cipher.encrypt_strings(object, number), names),
            None => write_object(out, object, names),
        },
        Written::Stream(stream) => {
            // `compress` had no effect at all on this path: the ordinary
            // writer compresses inside `write_entry`, which the linearized
            // layout does not use. Asking for both gave an uncompressed file
            // and no error — four times the size, measured.
            let mut dict = stream.dict.clone();
            let data = crate::write::maybe_compress(&stream.data, &mut dict, names, compress);
            // Encryption is the last thing applied and the first thing undone,
            // so it wraps the compressed bytes rather than the other way
            // round. /Length is then taken from the encrypted data, which is
            // both what 7.3.8.2 requires and what keeps every offset derived
            // below describing the bytes that are written.
            let data = match crypt {
                Some(cipher) => cipher.encrypt_stream(&data, number),
                None => data,
            };
            dict.insert(Name::LENGTH, Object::Int(data.len() as i64));
            write_object(out, &Object::Dict(dict), names);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&data);
            out.extend_from_slice(b"\nendstream");
        }
    }
    out.extend_from_slice(b"\nendobj\n");
}
