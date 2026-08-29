//! Annex F's hint tables, unpacked.
//!
//! The writer packs these; nothing read them back until the deleted qpdf
//! oracle did, and they were wrong in five separate ways while every test in
//! this repository passed. That is the shape of defect this module exists for:
//! a bit-packed structure with no consumer is a structure with no check.
//!
//! # What makes this evidence rather than a mirror
//!
//! Reading the tables with the writer's own `Plan` would prove only that the
//! writer agrees with itself. So this decoder produces plain numbers — how
//! many objects a page claims, how long it claims to be, which shared entries
//! it names — and [`super`] compares every one of them against the *file's*
//! own object extents, recovered from the cross-reference sections. The two
//! routes meet at the same numbers or the file is wrong.
//!
//! The field order is Annex F's own, and the column packing is a measurement
//! rather than a reading: Tables F.4 and F.6 list the items of one entry,
//! which reads as though entries were written one after another. They are not.
//! Every entry's item 1 is written for all entries, then every entry's item 2,
//! and each run is padded to a byte boundary — which is what accounts for the
//! sixteen bytes between a thirty-six byte header and a `/S` of 52 in a
//! six-page file, and row packing cannot.
//!
//! # One reader, not two
//!
//! *August 2026.* `linearize.rs`'s test module carried a second reader for the
//! same two tables, written against the same Annex F clauses, and the two
//! disagreed about Table F.4 item 5 — see [`decode`]. A format with two
//! readers has no reader: whichever is wrong is wrong in private. The tests
//! that drove the second one now drive this one, which is why this module
//! reports every header item and both end positions rather than only the
//! fields [`super`] happens to compare. A field nothing reads is a field
//! nothing checks, and that is the defect this module was created for.

use crate::limits;

/// One page's row of the page offset hint table (F.3, Table F.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PageHint {
    /// How many objects the page claims, item 1 plus the header's least.
    pub objects: u32,
    /// How many bytes it claims, item 2 plus the header's least.
    pub length: u32,
    /// Which shared-table entries it names (items 3 and 4).
    pub shared: Vec<u32>,
}

/// What the primary hint stream declares (F.3 and F.4).
///
/// Every header item of Tables F.3 and F.5 is reported, in their order and
/// under their numbers, rather than only the ones a caller happens to compare
/// today: an item this struct dropped would be an item that could be written
/// at the wrong width forever, which is exactly how the tables came to be
/// wrong in five ways at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hints {
    /// Table F.3 item 1: the least objects any page has.
    pub least_objects: u32,
    /// Table F.3 item 2: where the first page's page object begins.
    pub first_page_offset: u32,
    /// Item 3: the width of Table F.4 item 1.
    pub object_bits: u16,
    /// Item 4: the least bytes any page occupies.
    pub least_length: u32,
    /// Item 5: the width of Table F.4 item 2.
    pub length_bits: u16,
    /// Item 6: the least content-stream offset.
    pub least_content_offset: u32,
    /// Item 7: the width of Table F.4 item 6.
    pub content_offset_bits: u16,
    /// Item 8: the least content-stream length.
    pub least_content_length: u32,
    /// Item 9: the width of Table F.4 item 7.
    pub content_length_bits: u16,
    /// Item 10: the width of Table F.4 item 3.
    pub count_bits: u16,
    /// Item 11: the width of Table F.4 item 4.
    pub identifier_bits: u16,
    /// Item 12: the width of Table F.4 item 5.
    pub position_bits: u16,
    /// Item 13: what item 5's numerators are over.
    pub position_denominator: u32,
    /// One row per page, in page order.
    pub pages: Vec<PageHint>,
    /// How many bytes the page offset hint table occupies.
    ///
    /// The stream's `/S` names the same number from the other side, so the
    /// two disagreeing is a file that packs one table and addresses another.
    pub page_table_end: usize,
    /// Table F.5 item 1: part 8's first object number, or zero.
    pub first_shared_object: u32,
    /// Table F.5 item 2: where that object begins, or zero.
    pub first_shared_offset: u32,
    /// Table F.5 item 3: how many entries describe the first page's objects.
    pub shared_first_page: u32,
    /// Table F.5 item 5: the width of Table F.6 item 4.
    pub group_count_bits: u16,
    /// Table F.5 item 6: the least group length.
    pub least_group: u32,
    /// Table F.5 item 7: the width of Table F.6 item 1.
    pub group_bits: u16,
    /// Each entry's group length (Table F.6 item 1 plus the header's least).
    ///
    /// Its length is Table F.5 item 4, the entry count.
    pub shared_lengths: Vec<u32>,
    /// Table F.6 item 2, one flag per entry: whether a 128-bit signature
    /// follows it.
    pub signature_flags: Vec<u32>,
    /// How many bytes of the stream both tables together occupy, measured
    /// from byte zero.
    pub end: usize,
}

/// Unpacks both tables, or nothing when the stream runs out mid-field.
///
/// `shared_at` is the hint stream's own `/S`: the byte offset, inside the
/// decoded stream, where the shared object hint table begins.
///
/// # Table F.4 item 5 is per *reference*, not per page
///
/// The fractional-position numerator follows item 4, the shared object
/// identifiers, and Table F.4 repeats both of them for each shared object a
/// page references — so a page naming three shared objects contributes three
/// numerators and a page naming none contributes nothing. Reading one per page
/// instead, which this module did until August 2026, puts every later run of
/// the table out of step by the difference between the reference count and the
/// page count. It never showed, because this writer states item 12 as zero
/// bits and a zero-width run consumes nothing either way; a producer that
/// states a width would have been misread in silence.
pub(crate) fn decode(data: &[u8], shared_at: usize, page_count: usize) -> Option<Hints> {
    // `page_count` reaches here from a file's own `/N` on the streaming path,
    // so it is attacker-chosen. Capped before anything is sized by it.
    if page_count > limits::MAX_PAGES {
        return None;
    }
    let mut bits = BitReader::new(data);

    // ---- Page offset hint table (F.3, Table F.3) ----
    let least_objects = bits.read(32)?;
    let first_page_offset = bits.read(32)?;
    let object_bits = bits.read(16)? as u16;
    let least_length = bits.read(32)?;
    let length_bits = bits.read(16)? as u16;
    let least_content_offset = bits.read(32)?;
    let content_offset_bits = bits.read(16)? as u16;
    let least_content_length = bits.read(32)?;
    let content_length_bits = bits.read(16)? as u16;
    let count_bits = bits.read(16)? as u16;
    let identifier_bits = bits.read(16)? as u16;
    let position_bits = bits.read(16)? as u16;
    let position_denominator = bits.read(16)?;

    let mut objects = Vec::new();
    for _ in 0..page_count {
        objects.push(least_objects.saturating_add(bits.read(object_bits)?));
    }
    bits.align();

    let mut lengths = Vec::new();
    for _ in 0..page_count {
        lengths.push(least_length.saturating_add(bits.read(length_bits)?));
    }
    bits.align();

    let mut counts = Vec::new();
    for _ in 0..page_count {
        counts.push(bits.read(count_bits)?);
    }
    bits.align();

    // Item 4, one identifier per reference. The count is the file's own, so
    // it is bounded against the bits that could hold the identifiers before
    // any vector is grown by it: a zero-width identifier consumes nothing, so
    // the reads below would never fail however large the claim.
    let references: u64 = counts.iter().map(|c| u64::from(*c)).sum();
    if references > data.len().saturating_mul(8) as u64 {
        return None;
    }
    let mut shared = Vec::new();
    for count in &counts {
        let mut ids = Vec::new();
        for _ in 0..*count {
            ids.push(bits.read(identifier_bits)?);
        }
        shared.push(ids);
    }
    bits.align();

    // Item 5: one fractional-position numerator per shared object reference —
    // see the note above, which is what this module got wrong.
    for count in &counts {
        for _ in 0..*count {
            bits.read(position_bits)?;
        }
    }
    bits.align();

    // Items 6 and 7: the content stream's offset and length, one per page.
    for width in [content_offset_bits, content_length_bits] {
        for _ in 0..page_count {
            bits.read(width)?;
        }
        bits.align();
    }
    let page_table_end = bits.byte();

    // ---- Shared object hint table (F.4, Table F.5) ----
    //
    // Addressed by `/S` rather than by where the page table happened to end:
    // the stream's own dictionary says where this begins, and a reader that
    // trusted its own arithmetic instead would never notice the two
    // disagreeing.
    let mut bits = BitReader::new(data.get(shared_at..)?);
    let first_shared_object = bits.read(32)?;
    let first_shared_offset = bits.read(32)?;
    let shared_first_page = bits.read(32)?;
    let total = bits.read(32)?;
    let group_count_bits = bits.read(16)? as u16;
    let least_group = bits.read(32)?;
    let group_bits = bits.read(16)? as u16;

    let total = usize::try_from(total).ok()?;
    // A hostile file can claim four billion entries in a stream of forty
    // bytes; the reads below would fail anyway, but not before the allocation.
    if total > data.len().saturating_mul(8) {
        return None;
    }
    let mut shared_lengths = Vec::new();
    for _ in 0..total {
        shared_lengths.push(least_group.saturating_add(bits.read(group_bits)?));
    }
    bits.align();

    // Item 2: a one-bit flag per entry saying whether a 128-bit signature
    // follows. Never optional, and writing it at zero width is what once made
    // a reader run off the end of the stream before it had read one entry.
    let mut signature_flags = Vec::new();
    for _ in 0..total {
        signature_flags.push(bits.read(1)?);
    }
    bits.align();
    let signed = signature_flags.iter().filter(|flag| **flag == 1).count();
    for _ in 0..signed {
        for _ in 0..4 {
            bits.read(32)?;
        }
    }
    if signed > 0 {
        bits.align();
    }
    for _ in 0..total {
        bits.read(group_count_bits)?;
    }
    bits.align();
    let end = shared_at.checked_add(bits.byte())?;

    Some(Hints {
        least_objects,
        first_page_offset,
        object_bits,
        least_length,
        length_bits,
        least_content_offset,
        content_offset_bits,
        least_content_length,
        content_length_bits,
        count_bits,
        identifier_bits,
        position_bits,
        position_denominator,
        pages: (0..page_count)
            .map(|index| PageHint {
                objects: objects.get(index).copied().unwrap_or(0),
                length: lengths.get(index).copied().unwrap_or(0),
                shared: shared.get(index).cloned().unwrap_or_default(),
            })
            .collect(),
        page_table_end,
        first_shared_object,
        first_shared_offset,
        shared_first_page,
        group_count_bits,
        least_group,
        group_bits,
        shared_lengths,
        signature_flags,
        end,
    })
}

/// Reads fields of arbitrary bit width, most significant bit first (F.4).
struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
    used: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            at: 0,
            used: 0,
        }
    }

    /// The next `width` bits, or `None` past the end. A zero width reads as
    /// zero and consumes nothing, which is what the header's zero-width
    /// columns mean.
    fn read(&mut self, width: u16) -> Option<u32> {
        if width == 0 {
            return Some(0);
        }
        if width > 32 {
            return None;
        }
        // Checked before anything is consumed, so a field that does not fit
        // leaves the cursor where it was. The decode abandons the whole table
        // either way, and a reader that half-consumed its last field is a
        // reader whose position means nothing to whoever debugs it.
        let available = self
            .data
            .len()
            .saturating_mul(8)
            .saturating_sub(self.at.saturating_mul(8) + self.used as usize);
        if usize::from(width) > available {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..width {
            let byte = *self.data.get(self.at)?;
            let bit = (byte >> (7 - self.used)) & 1;
            value = (value << 1) | u32::from(bit);
            self.used += 1;
            if self.used == 8 {
                self.used = 0;
                self.at += 1;
            }
        }
        Some(value)
    }

    /// Moves to the next byte boundary, as each run's end does.
    fn align(&mut self) {
        if self.used > 0 {
            self.used = 0;
            self.at += 1;
        }
    }

    /// How many whole bytes have been consumed. Only meaningful when aligned.
    fn byte(&self) -> usize {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reader is the writer's inverse on a hand-packed field run.
    #[test]
    fn fields_read_back_most_significant_bit_first() {
        // 0b1010_0110, 0b1100_0000
        let data = [0xA6, 0xC0];
        let mut bits = BitReader::new(&data);
        assert_eq!(bits.read(4), Some(0b1010));
        assert_eq!(bits.read(4), Some(0b0110));
        assert_eq!(bits.read(2), Some(0b11));
        assert_eq!(bits.read(0), Some(0), "a zero-width field consumes nothing");
        bits.align();
        assert_eq!(bits.read(1), None, "and the stream ends where it ends");
    }

    #[test]
    fn a_field_wider_than_the_stream_reads_as_nothing() {
        let mut bits = BitReader::new(&[0xFF]);
        assert_eq!(bits.read(33), None);
        assert_eq!(bits.read(9), None);
        assert_eq!(bits.read(8), Some(0xFF));
    }

    #[test]
    fn a_truncated_table_decodes_to_nothing_rather_than_to_zeroes() {
        assert_eq!(decode(&[0u8; 8], 0, 1), None);
    }

    /// The two headers are the sizes Annex F states, measured rather than
    /// assumed.
    ///
    /// Table F.3 is thirteen items and Table F.5 is seven, and every field of
    /// both is fixed-width, so their sizes are arithmetic a reader can be held
    /// to: 36 bytes and 24. An item dropped or added shifts every run below it
    /// and this is the assertion that says so, in the decoder's own file,
    /// without a document in front of it.
    #[test]
    fn the_two_table_headers_are_thirty_six_and_twenty_four_bytes() {
        let data = [0u8; 60];
        let read = decode(&data, 36, 0).expect("an empty pair of tables decodes");
        assert_eq!(read.page_table_end, 36, "Table F.3 is thirteen items");
        assert_eq!(read.end, 60, "Table F.5 is seven more");
        assert!(read.pages.is_empty());
        assert!(read.shared_lengths.is_empty());
    }

    /// One byte short of either header is nothing, not a header of zeroes.
    #[test]
    fn a_header_one_byte_short_decodes_to_nothing() {
        assert_eq!(decode(&[0u8; 35], 0, 0), None, "the page offset header");
        assert_eq!(decode(&[0u8; 59], 36, 0), None, "the shared object header");
    }

    /// Table F.4 item 5 is one numerator per shared object *reference*.
    ///
    /// Nothing in the fetched qpdf corpus discriminates this: every linearized
    /// file in it states item 12 as zero bits, so a zero-width run consumes
    /// nothing whichever way it is counted, and this decoder read one per page
    /// for as long as it existed without a single file noticing. The table is
    /// therefore packed here by hand, with a width and two pages whose
    /// reference counts differ from the page count, so the two readings land
    /// on different bytes: per reference the page table is 44 bytes, per page
    /// it is 43, and `/S` names the first.
    #[test]
    fn one_fractional_position_is_read_for_each_shared_reference() {
        let mut data = vec![0u8; 36];
        // Item 10, the width of a page's reference count.
        data[29] = 8;
        // Item 11, the width of one identifier.
        data[31] = 8;
        // Item 12, the width of one fractional-position numerator.
        data[33] = 8;
        // Item 13, what those numerators are over.
        data[35] = 1;
        // Two pages: the first names three shared objects, the second none.
        data.extend_from_slice(&[3, 0]);
        // Item 4, three identifiers.
        data.extend_from_slice(&[10, 11, 12]);
        // Item 5, three numerators -- one for each reference, not one for
        // each page. A reader that takes two here ends a byte short.
        data.extend_from_slice(&[1, 2, 3]);
        let shared_at = data.len();
        assert_eq!(shared_at, 44, "the hand-packed page table is 44 bytes");
        // An empty shared object hint table: seven header items, no entries.
        data.extend_from_slice(&[0u8; 24]);

        let read = decode(&data, shared_at, 2).expect("the tables decode");
        assert_eq!(read.count_bits, 8, "item 10");
        assert_eq!(read.identifier_bits, 8, "item 11");
        assert_eq!(read.position_bits, 8, "item 12");
        assert_eq!(read.position_denominator, 1, "item 13");
        assert_eq!(read.pages[0].shared, vec![10, 11, 12], "item 4, page one");
        assert_eq!(read.pages[1].shared, Vec::<u32>::new(), "item 4, page two");
        assert_eq!(
            read.page_table_end, shared_at,
            "item 5 is per reference: one numerator per page ends at 43"
        );
        assert_eq!(read.end, 68, "and the shared header is the last 24 bytes");
    }

    /// A page count no file could have is refused before it is allocated for.
    #[test]
    fn an_absurd_page_count_is_refused_rather_than_sized_for() {
        assert_eq!(decode(&[0u8; 64], 0, limits::MAX_PAGES + 1), None);
    }

    /// A hostile shared-reference count over a zero-width identifier column
    /// would otherwise grow a vector without consuming a bit.
    #[test]
    fn shared_references_are_bounded_by_the_bits_that_could_hold_them() {
        // Thirteen header items in thirty-six bytes. Item 10 -- the width of
        // a page's reference count -- is the fifth 16-bit field, so it lands
        // at bytes 28 and 29; item 11, the identifier width, is left at zero
        // so that the references themselves consume nothing however many are
        // claimed. Then one page whose count field is four bytes of ones.
        let mut data = vec![0u8; 36];
        data[29] = 32;
        data.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        data.extend_from_slice(&[0u8; 64]);
        assert_eq!(decode(&data, 0, 1), None);
    }
}

/// The reader against linearized files this project did not write.
///
/// The round-trip tests above prove the decoder is the writer's inverse. That
/// is the agreement that proves the least: a reader misunderstanding Annex F
/// exactly as the writer does agrees with it perfectly, which is how the
/// tables came to be wrong in five ways with every test green. These files
/// were linearized by somebody else, so the two understandings are
/// independent — and the count is asserted, because a sweep reporting whatever
/// it happens to find reads as a pass when the set shrinks to nothing.
#[cfg(test)]
mod corpus {
    use std::path::PathBuf;

    use super::decode;
    use crate::doc::CosDocument;
    use crate::object::Object;
    use crate::parse::parse_indirect_at;
    use crate::repair::next_object_header;
    use crate::warn::WarningSink;
    use crate::xref;

    /// How many files in the pinned qpdf corpus are already linearized.
    ///
    /// Committed numbers rather than "however many were found", so a corpus
    /// that half-extracted, a filter that stopped matching, or a decoder that
    /// started calling linearized files ordinary all fail here instead of
    /// passing quietly with a smaller set. Measured against the `qpdf` entry
    /// of `corpus/corpora.lock`, commit e8adee32, whose pinned subdirectory
    /// holds 626 PDFs.
    const LINEARIZED_FILES: usize = 45;

    /// How many of those unpack both hint tables.
    const DECODED_FILES: usize = 32;

    /// How many are encrypted under a password this test does not have.
    ///
    /// Their hint streams are ciphertext, so refusing to read them is the
    /// right answer rather than a defect: counted as linearized, excluded from
    /// what must decode, and pinned so that a decoder which started returning
    /// numbers for ciphertext would fail here.
    const SEALED_FILES: usize = 10;

    /// The linearized files whose hint tables this reader refuses, by name and
    /// with the reason it refuses them.
    ///
    /// Named rather than counted, and asserted as a set: qpdf carries these
    /// three as deliberately malformed linearization fixtures — two bounds
    /// cases and one allocation regression — so a refusal is the answer
    /// rulings 1 and 2 ask for. A file that stopped being refused, or a
    /// fourth that started, both fail here.
    const REFUSED_FILES: &[&str] = &[
        "linearization-bounds-1.pdf: the tables ran out mid-field",
        "linearization-bounds-2.pdf: the tables ran out mid-field",
        "linearization-large-vector-alloc.pdf: the tables ran out mid-field",
    ];

    /// Where `cargo xtask corpus-fetch` puts the qpdf corpus, and the override
    /// for a checkout that shares one fetch between worktrees.
    fn corpus_dir() -> Option<PathBuf> {
        let named = std::env::var_os("TINKER_QPDF_CORPUS").map(PathBuf::from);
        let default = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/files/qpdf/qpdf/qtest/qpdf");
        named
            .into_iter()
            .chain(std::iter::once(default))
            .find(|dir| dir.is_dir())
    }

    /// Every `.pdf` in the corpus, by name, so the order is the same on every
    /// machine and a failure names the same file twice running.
    fn pdfs(dir: &PathBuf) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "pdf"))
            .collect();
        out.sort();
        out
    }

    /// What one file turned out to be.
    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        /// Its first object is not a linearization parameter dictionary.
        NotLinearized,
        /// It is linearized and both hint tables unpacked.
        Decoded,
        /// It is linearized and something stopped the tables being read.
        Refused(&'static str),
        /// It is linearized and encrypted under a password this test does not
        /// have, so its hint stream is ciphertext. Counted as linearized and
        /// excluded from what must decode, because refusing to read bytes
        /// nobody supplied the key for is the correct answer rather than a
        /// defect in the decoder.
        NeedsPassword,
    }

    /// F.2.2: the parameter dictionary is the first object in the file, so a
    /// file whose first object is anything else is not linearized. That is the
    /// same test [`crate::validate`] applies, deliberately: two different
    /// answers to "is this file linearized" would make the count meaningless.
    fn read_hints(bytes: Vec<u8>) -> Outcome {
        let Ok(doc) = CosDocument::open(bytes) else {
            return Outcome::NotLinearized;
        };
        let mut sink = WarningSink::new();
        // 7.5.2: bytes before `%PDF-` shift every offset the file stores, and
        // Annex F's `/H` is one of them. A reader that forgets the shift finds
        // no hint stream in a file that has a perfectly good one.
        let shift = xref::header_shift(doc.bytes(), &mut sink);
        let Some(at) = next_object_header(doc.bytes(), 0) else {
            return Outcome::NotLinearized;
        };
        let Some(first) = parse_indirect_at(doc.bytes(), at, doc.names_table(), &mut sink) else {
            return Outcome::NotLinearized;
        };
        let linearized = doc.intern(b"Linearized");
        let Some(dict) = first
            .object
            .as_dict()
            .filter(|d| d.contains_key(linearized))
        else {
            return Outcome::NotLinearized;
        };

        let Some(page_count) = dict
            .get_int(doc.intern(b"N"))
            .and_then(|v| usize::try_from(v).ok())
        else {
            return Outcome::Refused("/N is not a page count");
        };
        let hint_offset = dict
            .get_array(doc.intern(b"H"))
            .and_then(|a| a.first().cloned())
            .and_then(|o| o.as_int())
            .and_then(|v| u64::try_from(v).ok());
        let Some(hint_offset) = hint_offset else {
            return Outcome::Refused("/H does not name an offset");
        };

        if doc.is_encrypted() && doc.authenticate("").is_err() {
            return Outcome::NeedsPassword;
        }

        let mut sink = WarningSink::new();
        let len = doc.bytes().len() as u64;
        let stream = xref::offset_candidates(len, hint_offset, shift)
            .into_iter()
            .find_map(|at| parse_indirect_at(doc.bytes(), at, doc.names_table(), &mut sink));
        let Some(stream) = stream else {
            return Outcome::Refused("/H names no object");
        };
        let Object::Stream(hint) = &stream.object else {
            return Outcome::Refused("/H names something that is not a stream");
        };
        let shared_at = hint
            .dict
            .get_int(doc.intern(b"S"))
            .and_then(|v| usize::try_from(v).ok());
        let Some(shared_at) = shared_at else {
            return Outcome::Refused("the hint stream states no /S");
        };
        let Ok(data) = doc.stream_decoded(stream.reference) else {
            return Outcome::Refused("the hint stream does not decode");
        };
        match decode(&data, shared_at, page_count) {
            Some(_) => Outcome::Decoded,
            None => Outcome::Refused("the tables ran out mid-field"),
        }
    }

    /// Ruling 13's `RAN`/`SKIPPED` discipline: a check that can be absent says
    /// which it was, so a corpus nobody fetched cannot read as a corpus that
    /// passed.
    #[test]
    fn every_linearized_file_in_the_qpdf_corpus_decodes_its_hint_tables() {
        let Some(dir) = corpus_dir() else {
            println!("SKIPPED hint tables over the qpdf corpus: it is not fetched");
            return;
        };

        let files = pdfs(&dir);
        assert!(
            files.len() > 500,
            "{} holds {} PDFs, which is not the qpdf corpus",
            dir.display(),
            files.len()
        );

        let mut linearized = 0usize;
        let mut decoded = 0usize;
        let mut sealed = 0usize;
        let mut refused: Vec<String> = Vec::new();
        for path in &files {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match read_hints(bytes) {
                Outcome::NotLinearized => {}
                Outcome::NeedsPassword => {
                    linearized += 1;
                    sealed += 1;
                }
                Outcome::Decoded => {
                    linearized += 1;
                    decoded += 1;
                }
                Outcome::Refused(why) => {
                    linearized += 1;
                    refused.push(format!("{name}: {why}"));
                }
            }
        }

        println!(
            "RAN hint tables over qpdf: {decoded} decoded, {sealed} sealed, {linearized} linearized, {} PDFs",
            files.len()
        );
        assert_eq!(
            refused, REFUSED_FILES,
            "these are the linearized files this reader refuses, and no others"
        );
        assert_eq!(
            linearized, LINEARIZED_FILES,
            "the corpus holds this many already-linearized files"
        );
        assert_eq!(
            decoded, DECODED_FILES,
            "this many of them unpack both hint tables, and a set that shrank would
             otherwise read as a pass"
        );
        assert_eq!(
            sealed, SEALED_FILES,
            "this many are sealed under a password"
        );
    }
}
