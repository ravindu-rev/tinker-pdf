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
//!   lengths — never from an absolute position — and the two *offsets* the
//!   tables carry occupy fixed 32-bit fields, so the hint stream's length is
//!   known once the objects are serialised, before its contents are.
//!
//! So the writer serialises every object, computes every length, derives
//! every offset, and only then emits. A two-pass writer with a patch-up phase
//! is the usual approach and this deliberately avoids it: a patch that misses
//! one field produces a file that opens fine everywhere except in the reader
//! the whole feature exists for. The parameter dictionary and the hint stream
//! are each *built twice* — once to measure, once with the values — which is
//! not a patch: no byte of the emitted buffer is ever revisited, and both
//! builds are asserted to be the same length.
//!
//! # How a reader finds a page, and what the layout owes it
//!
//! The page-offset hint table (F.4.1) does not name the objects a page is
//! made of. It gives a *count*, and the reader takes that many **consecutive
//! object numbers beginning with the page's own page object**. So the
//! numbering is not free: each page's objects are emitted as one run led by
//! the page object, and the first page's run begins at `/O`. Numbering them
//! any other way produces a file whose hint tables point a reader at objects
//! that belong to some other page, or at numbers no table carries at all.
//!
//! For the same reason the primary hint stream sits in part 5, *ahead* of the
//! first page's objects rather than after them: `/E` is the end of part 6,
//! the first-page section, and the hint stream is the one thing a reader
//! needs before it can use any of it.
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

/// Which part of the file an object belongs to (F.3.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    /// Part 4: the catalog, the page tree, and the rest of the document-level
    /// objects a reader needs before it can do anything at all.
    Document,
    /// Part 6: the first page's section — its page object, everything only it
    /// uses, and everything it shares with a later page. F.3.8 keeps a shared
    /// object here when the first page needs it, because a reader holding the
    /// front of the file must not have to reach past `/E` to draw page one.
    FirstPage,
    /// Part 7: the remaining pages, one consecutive run each.
    Rest,
    /// Part 8: objects more than one page needs and the first page does not.
    Shared,
    /// Part 9: everything no page reaches.
    Other,
}

struct Plan {
    /// Objects in output order, already renumbered, with their bytes.
    ordered: Vec<Placed>,
    /// New number of the first page's page object, which is `/O`.
    first_page_object: u32,
    /// How many pages the document has, which is `/N`.
    page_count: u32,
    /// Serialised lengths of each page's run of objects, for the hint tables.
    page_lengths: Vec<u32>,
    /// How many objects are in each page's run.
    page_object_counts: Vec<u32>,
    /// Which shared-table entries each page references (F.4.1 items 3 and 4).
    /// Page one's list is empty: F.4.2 makes its objects the first entries of
    /// the shared table, so a reader already has them.
    page_shared: Vec<Vec<u32>>,
    /// The shared-object table's entries, in order: part 6's objects first
    /// (Table F.5 item 3 counts them), then part 8's.
    shared_entries: Vec<u32>,
    /// Each entry's serialised length, which is its group length (Table F.6).
    shared_lengths: Vec<u32>,
    /// How many of `shared_entries` belong to part 6.
    shared_first_page: u32,
    /// Part 8's first object number, or zero when part 8 is empty.
    first_shared_object: u32,
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
            section.insert(num, Section::Other);
        }
        for num in &document {
            section.insert(*num, Section::Document);
        }
        for (num, count) in &users {
            if document.contains(num) {
                continue;
            }
            // F.3.8: part 8 is for objects *several* pages need and the first
            // page does not. An object the first page needs travels with it
            // whether or not later pages need it too, because a reader that
            // has only the front of the file has to be able to draw page one.
            let first = per_page.first().is_some_and(|set| set.contains(num));
            section.insert(
                *num,
                match (first, *count > 1) {
                    (true, _) => Section::FirstPage,
                    (false, true) => Section::Shared,
                    (false, false) => Section::Rest,
                },
            );
        }

        // Output order, and with it the new numbering. Object 1 is the
        // parameter dictionary and object 2 the hint stream; both are
        // reserved here and written by `emit`.
        //
        // The order is not a preference. F.4.1's per-page entry gives a
        // reader a count and nothing else, and the reader takes that many
        // consecutive object numbers starting at the page's own page object —
        // so each page's objects go out as one run, page object first, or the
        // hint table describes objects that belong to another page.
        let mut order: Vec<(u32, Section)> = Vec::new();
        let mut placed: BTreeSet<u32> = BTreeSet::new();
        let mut push = |num: u32, want: Section, order: &mut Vec<(u32, Section)>| {
            if placed.insert(num) {
                order.push((num, want));
                return true;
            }
            false
        };

        // Part 4. F.3.4 puts the catalog first; a reader that has the front of
        // the file reaches it without a search.
        push(root.num, Section::Document, &mut order);
        for num in objects.numbers() {
            if section.get(&num) == Some(&Section::Document) {
                push(num, Section::Document, &mut order);
            }
        }

        // Part 6: the first page's run, its page object leading.
        let mut runs: Vec<Vec<u32>> = Vec::with_capacity(pages.len());
        let mut first_run = Vec::new();
        if let Some(first) = pages.first() {
            if push(*first, Section::FirstPage, &mut order) {
                first_run.push(*first);
            }
        }
        for num in objects.numbers() {
            if section.get(&num) == Some(&Section::FirstPage)
                && push(num, Section::FirstPage, &mut order)
            {
                first_run.push(num);
            }
        }
        runs.push(first_run);

        // Part 7: the remaining pages, a run each.
        for (index, page) in pages.iter().enumerate().skip(1) {
            let mut run = Vec::new();
            if push(*page, Section::Rest, &mut order) {
                run.push(*page);
            }
            let reached = per_page.get(index);
            for num in objects.numbers() {
                if section.get(&num) == Some(&Section::Rest)
                    && reached.is_some_and(|set| set.contains(&num))
                    && push(num, Section::Rest, &mut order)
                {
                    run.push(num);
                }
            }
            runs.push(run);
        }

        // Part 8, then part 9 — which is everything no page reached, plus any
        // object the walk above could not attribute to a page.
        let mut shared_run: Vec<u32> = Vec::new();
        for num in objects.numbers() {
            if section.get(&num) == Some(&Section::Shared) && push(num, Section::Shared, &mut order)
            {
                shared_run.push(num);
            }
        }
        for num in objects.numbers() {
            push(num, Section::Other, &mut order);
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

        for (old, section) in &order {
            let entry = objects.get(*old)?;
            let renumbered = renumber_entry(entry, &mapping);
            let number = *mapping.get(old)?;
            let mut bytes = Vec::new();
            write_indirect(&mut bytes, number, &renumbered, names, compress, crypt);

            ordered.push(Placed {
                number,
                section: *section,
                bytes,
            });
        }

        // Per-page sizes, for the page-offset hint table. A page's "length"
        // is the bytes of its run, which is what a reader must have received
        // before it can draw the page.
        let sizes: BTreeMap<u32, u32> = ordered
            .iter()
            .map(|placed| (placed.number, placed.bytes.len() as u32))
            .collect();
        let length_of = |run: &[u32]| -> u32 {
            run.iter()
                .filter_map(|old| mapping.get(old))
                .filter_map(|new| sizes.get(new))
                .sum()
        };
        let page_object_counts: Vec<u32> = runs.iter().map(|run| run.len() as u32).collect();
        let page_lengths: Vec<u32> = runs.iter().map(|run| length_of(run)).collect();

        // F.4.2: the shared-object table describes part 6's objects first —
        // that is what Table F.5's "entries for the first page" counts — and
        // then part 8's. A reader walks it positionally, from the first page's
        // page object and then from `first_shared_obj`, so the order is the
        // structure and not a convention.
        let shared_first_page = runs.first().map_or(0, |run| run.len() as u32);
        let shared_old: Vec<u32> = runs
            .first()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(shared_run.iter().copied())
            .collect();
        let shared_entries: Vec<u32> = shared_old
            .iter()
            .filter_map(|old| mapping.get(old).copied())
            .collect();
        let shared_lengths: Vec<u32> = shared_entries
            .iter()
            .map(|num| sizes.get(num).copied().unwrap_or(0))
            .collect();
        let first_shared_object = shared_run
            .first()
            .and_then(|old| mapping.get(old).copied())
            .unwrap_or(0);

        // Which of those entries each page needs. Page one's list stays empty:
        // its objects *are* the first entries, so a reader that has page one
        // has them already.
        let mut page_shared: Vec<Vec<u32>> = vec![Vec::new(); pages.len()];
        for (index, reached) in per_page.iter().enumerate().skip(1) {
            page_shared[index] = shared_old
                .iter()
                .enumerate()
                .filter(|(_, old)| reached.contains(old))
                .map(|(at, _)| at as u32)
                .collect();
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
            page_lengths,
            page_object_counts,
            page_shared,
            shared_entries,
            shared_lengths,
            shared_first_page,
            first_shared_object,
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

        // The hint stream carries two file offsets, and neither can be known
        // until its own length is — so it is built once with zeros to measure
        // it, and once more with the values. Both offsets sit in fixed 32-bit
        // fields and every bit width below is derived from counts and object
        // lengths, so the second build is the same size as the first. That is
        // asserted rather than assumed.
        let hint_object = |first_page_offset: u32, first_shared_offset: u32| -> Vec<u8> {
            let (data, shared_at) = self.hint_tables(first_page_offset, first_shared_offset);
            let mut dict = Dict::new();
            dict.insert(names.intern(b"S"), Object::Int(shared_at as i64));
            let mut bytes = Vec::new();
            write_indirect(
                &mut bytes,
                2,
                &Written::Stream(StreamData { dict, data }),
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
        let hint_len = hint_object(0, 0).len();

        // Part 2 is a fixed size because every integer in it is written to a
        // fixed width, which is the whole reason this needs no second pass.
        let parameters = self.parameter_dictionary(0, 0, 0, 0, 0, names);
        let part2_len = parameters.len();

        // Part 3 covers everything ahead of part 7: the document-level
        // objects, the hint stream, and the first page.
        let document: Vec<&Placed> = self
            .ordered
            .iter()
            .filter(|p| p.section == Section::Document)
            .collect();
        let first_page: Vec<&Placed> = self
            .ordered
            .iter()
            .filter(|p| p.section == Section::FirstPage)
            .collect();
        let back: Vec<&Placed> = self
            .ordered
            .iter()
            .filter(|p| !matches!(p.section, Section::Document | Section::FirstPage))
            .collect();

        // Every entry is twenty bytes, so the table's size follows from the
        // count alone (7.5.4).
        let front_count = document.len() + first_page.len();
        let first_xref_len =
            subsection_len(front_count + 2) + self.trailer_bytes(Some(0), names).len();

        let part1 = header.len();
        let part2_at = part1;
        let part3_at = part2_at + part2_len;
        let part4_at = part3_at + first_xref_len;

        let mut at = part4_at;
        let mut offsets: BTreeMap<u32, u64> = BTreeMap::new();
        for placed in &document {
            offsets.insert(placed.number, at as u64);
            at += placed.bytes.len();
        }

        // Part 5. It comes before the first page's objects and not after, so
        // that a reader receiving the file in order has the tables that say
        // what to ask for next before it has the thing they describe.
        let hint_at = at;
        offsets.insert(2, hint_at as u64);
        at += hint_len;

        let first_page_at = at;
        for placed in &first_page {
            offsets.insert(placed.number, at as u64);
            at += placed.bytes.len();
        }

        // `/E` is the end of part 6, the first-page section (Table F.1).
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

        // F.4: an offset inside a hint table is measured as though the primary
        // hint stream were not in the file at all, because the tables are
        // built before their own length is known. Everything after the hint
        // stream is therefore short by that length. `first_page_at` sits
        // immediately behind it, so this comes out equal to `hint_at`.
        let without_hint = |offset: u64| -> u32 {
            u32::try_from(offset.saturating_sub(hint_len as u64)).unwrap_or(u32::MAX)
        };
        let hint_bytes = hint_object(
            without_hint(first_page_at as u64),
            match self.first_shared_object {
                0 => 0,
                num => offsets.get(&num).copied().map_or(0, without_hint),
            },
        );
        debug_assert_eq!(hint_bytes.len(), hint_len, "the hint stream is fixed width");

        // Table F.1: `/T` is the offset of the *first entry* of the main
        // cross-reference table — the free entry for object zero — not of the
        // `xref` keyword above it.
        let xref_zero_at = main_xref_at + first_entry_offset(&main_xref);

        let parameters = self.parameter_dictionary(
            total as u64,
            hint_at as u64,
            hint_len as u64,
            end_of_first_page as u64,
            xref_zero_at as u64,
            names,
        );
        debug_assert_eq!(parameters.len(), part2_len, "the dictionary is fixed width");

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&header);
        out.extend_from_slice(&parameters);

        let mut front_offsets: Vec<(u32, u64)> = document
            .iter()
            .chain(first_page.iter())
            .filter_map(|p| offsets.get(&p.number).map(|at| (p.number, *at)))
            .collect();
        front_offsets.push((1, part1 as u64));
        front_offsets.push((2, hint_at as u64));
        front_offsets.sort_unstable();
        self.write_first_xref(&mut out, &front_offsets, main_xref_at as u64, names);
        debug_assert_eq!(out.len(), part4_at, "part 4 begins where it was placed");

        for placed in &document {
            out.extend_from_slice(&placed.bytes);
        }
        out.extend_from_slice(&hint_bytes);
        for placed in &first_page {
            out.extend_from_slice(&placed.bytes);
        }
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
        xref_zero_at: u64,
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
        pad(&mut out, xref_zero_at);
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
    /// starts within it, which is `/S` (F.4).
    ///
    /// Both offsets are passed in already measured as though this stream were
    /// not in the file; see `emit`.
    ///
    /// # The packing is by column, not by row
    ///
    /// Tables F.4 and F.6 list the items of *one* entry, which reads as though
    /// the entries were written one after another. They are not. Every
    /// entry's item 1 is written first, for all entries, then every entry's
    /// item 2, and so on — and each of those runs is padded to a byte
    /// boundary before the next begins.
    ///
    /// That is not a reading of the table layout; it is a measurement.
    /// qpdf's own linearized output declares `/S 52` with a thirty-six byte
    /// page-offset header and six pages, and only the column packing accounts
    /// for the sixteen bytes between: 6x1 bit padded to 1, 6x7 padded to 6,
    /// 6x1 padded to 1, 5x2 padded to 2, then two empty runs and 6x7 padded
    /// to 6. Row packing of the same values needs thirteen.
    fn hint_tables(&self, first_page_offset: u32, first_shared_offset: u32) -> (Vec<u8>, usize) {
        let mut bits = BitWriter::default();
        let pages = self.page_count as usize;

        // ---- Page offset hint table (F.4.1, Table F.3) ----
        let least_objects = self.page_object_counts.iter().copied().min().unwrap_or(0);
        let most_objects = self.page_object_counts.iter().copied().max().unwrap_or(0);
        let object_bits = bits_for(most_objects.saturating_sub(least_objects));

        let least_length = self.page_lengths.iter().copied().min().unwrap_or(0);
        let most_length = self.page_lengths.iter().copied().max().unwrap_or(0);
        let length_bits = bits_for(most_length.saturating_sub(least_length));

        let most_shared = self
            .page_shared
            .iter()
            .map(|ids| ids.len() as u32)
            .max()
            .unwrap_or(0);
        let count_bits = bits_for(most_shared);
        let identifier_bits = bits_for(
            self.page_shared
                .iter()
                .flatten()
                .copied()
                .max()
                .unwrap_or(0),
        );

        bits.write(least_objects, 32); // 1: least objects in a page
        bits.write(first_page_offset, 32); // 2: where page one's page object is
        bits.write(u32::from(object_bits), 16); // 3
        bits.write(least_length, 32); // 4: least page length
        bits.write(u32::from(length_bits), 16); // 5
        bits.write(0, 32); // 6: least content-stream offset
        bits.write(0, 16); // 7
        bits.write(0, 32); // 8: least content-stream length
        bits.write(0, 16); // 9
        bits.write(u32::from(count_bits), 16); // 10: shared-reference count
        bits.write(u32::from(identifier_bits), 16); // 11: shared identifier
        bits.write(0, 16); // 12: fractional position numerator
        bits.write(1, 16); // 13: its denominator

        // Table F.4, item by item.
        for index in 0..pages {
            let objects = self.page_object_counts.get(index).copied().unwrap_or(0);
            bits.write(objects.saturating_sub(least_objects), object_bits);
        }
        bits.align();
        for index in 0..pages {
            let length = self.page_lengths.get(index).copied().unwrap_or(0);
            bits.write(length.saturating_sub(least_length), length_bits);
        }
        bits.align();
        for index in 0..pages {
            let count = self
                .page_shared
                .get(index)
                .map_or(0, |ids| ids.len() as u32);
            bits.write(count, count_bits);
        }
        bits.align();
        for ids in &self.page_shared {
            for id in ids {
                bits.write(*id, identifier_bits);
            }
        }
        bits.align();
        // Item 5, the fractional-position numerators: item 12 above makes them
        // zero bits wide, so this run is empty and only its padding remains.
        bits.align();
        // Items 6 and 7, the content-stream offset and length deltas: items 7
        // and 9 make both zero bits wide. F.4.1 permits it and Acrobat writes
        // them so.
        bits.align();
        let shared_at = bits.len();

        // ---- Shared object hint table (F.4.2, Table F.5) ----
        let least_group = self.shared_lengths.iter().copied().min().unwrap_or(0);
        let most_group = self.shared_lengths.iter().copied().max().unwrap_or(0);
        let group_bits = bits_for(most_group.saturating_sub(least_group));

        bits.write(self.first_shared_object, 32); // 1: part 8's first object
        bits.write(first_shared_offset, 32); // 2: and where it is
        bits.write(self.shared_first_page, 32); // 3: entries for the first page
        bits.write(self.shared_entries.len() as u32, 32); // 4: entries in all
        bits.write(0, 16); // 5: bits for the group-object count
        bits.write(least_group, 32); // 6: least group length
        bits.write(u32::from(group_bits), 16); // 7: bits for the delta

        // Table F.6, item by item. Item 2 is a one-bit flag saying whether a
        // 128-bit MD5 signature follows; it is never optional, and writing it
        // at zero width is what made a reader run off the end of the stream
        // before it had read a single entry. None of ours carry a signature,
        // so no digest follows, and item 4's width is zero because every group
        // here is exactly one object.
        for length in &self.shared_lengths {
            bits.write(length.saturating_sub(least_group), group_bits);
        }
        bits.align();
        for _ in &self.shared_lengths {
            bits.write(0, 1);
        }
        bits.align();
        // Item 4, the object count in each group, at item 5's zero width.
        bits.align();

        (bits.finish(), shared_at)
    }
}

/// Where the first entry of a classic cross-reference table sits inside it:
/// past `xref\n` and past the subsection header line.
fn first_entry_offset(table: &[u8]) -> usize {
    table
        .iter()
        .skip(5)
        .position(|byte| *byte == b'\n')
        .map_or(0, |at| 5 + at + 1)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::Encryption;

    fn a_plan(crypt: Option<StreamCipher>) -> Plan {
        Plan {
            ordered: Vec::new(),
            first_page_object: 7,
            page_count: 6,
            page_lengths: Vec::new(),
            page_object_counts: Vec::new(),
            page_shared: Vec::new(),
            shared_entries: Vec::new(),
            shared_lengths: Vec::new(),
            shared_first_page: 0,
            first_shared_object: 0,
            trailer: Dict::new(),
            size: 20,
            crypt,
        }
    }

    fn a_cipher() -> StreamCipher {
        let request = Encryption {
            user_password: "open-me".to_string(),
            owner_password: "owner-me".to_string(),
            permissions: -1,
            entropy: [7u8; 48],
        };
        crate::write::build_encryption(&request, &NameTable::new())
            .expect("R6 builds from any entropy")
            .1
    }

    /// The one genuinely awkward point in 7.6.1, settled rather than assumed.
    ///
    /// 7.6.1 exempts the `/Encrypt` dictionary and the cross-reference
    /// streams from encryption. It does *not* exempt the linearization
    /// parameter dictionary, and this writer leaves it in the clear anyway,
    /// because a reader consults it before it has authenticated — before it
    /// has even found `/Encrypt`.
    ///
    /// What makes that sound is that there is nothing in it for encryption to
    /// touch. 7.6.2 encrypts strings and streams; every value here is an
    /// integer or an array of integers, so the question is moot. Asserted
    /// here so that a field added later which *does* carry a string fails
    /// this test rather than being written in plain sight inside a file that
    /// claims to be encrypted.
    #[test]
    fn the_parameter_dictionary_carries_no_strings() {
        let bytes =
            a_plan(None).parameter_dictionary(3288, 1416, 133, 1549, 2801, &NameTable::new());
        let text = String::from_utf8(bytes).expect("the parameter dictionary is ASCII");

        // 7.3.4: a string is written `(...)` or `<...>`. The dictionary's own
        // `<<` and `>>` are the only angle brackets allowed to appear.
        let stripped = text.replace("<<", "").replace(">>", "");
        for delimiter in ['(', ')', '<', '>'] {
            assert!(
                !stripped.contains(delimiter),
                "a {delimiter} means a string is in there: {text}"
            );
        }

        // And positively, so that something neither a number nor a string
        // cannot slip through the scan above: every token is a name, a
        // bracket, an integer, or the indirect-object syntax.
        for token in text.split_ascii_whitespace() {
            let known = token.starts_with('/')
                || matches!(token, "<<" | ">>" | "[" | "]" | "obj" | "endobj")
                || token.bytes().all(|b| b.is_ascii_digit());
            assert!(known, "unexpected token {token:?} in: {text}");
        }
    }

    /// The other half of the same claim, from the writer's side: the cipher
    /// does not reach the parameter dictionary at all.
    ///
    /// The test above says there is nothing in it to encrypt. This says
    /// nothing encrypts it, which is a different statement and the one that
    /// would fail if a future change routed part 2 through `write_indirect`
    /// like every other object.
    #[test]
    fn the_parameter_dictionary_is_the_same_bytes_encrypted_or_not() {
        let names = NameTable::new();
        let clear = a_plan(None).parameter_dictionary(3288, 1416, 133, 1549, 2801, &names);
        let sealed =
            a_plan(Some(a_cipher())).parameter_dictionary(3288, 1416, 133, 1549, 2801, &names);
        assert_eq!(
            String::from_utf8_lossy(&clear),
            String::from_utf8_lossy(&sealed),
            "the cipher never touches part 2"
        );
    }
}
