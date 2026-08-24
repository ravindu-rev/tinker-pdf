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
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hints {
    /// Table F.3 item 2: where the first page's page object begins.
    pub first_page_offset: u32,
    /// One row per page, in page order.
    pub pages: Vec<PageHint>,
    /// Table F.5 item 1: part 8's first object number, or zero.
    pub first_shared_object: u32,
    /// Table F.5 item 2: where that object begins, or zero.
    pub first_shared_offset: u32,
    /// Table F.5 item 3: how many entries describe the first page's objects.
    pub shared_first_page: u32,
    /// Each entry's group length (Table F.6 item 1 plus the header's least).
    pub shared_lengths: Vec<u32>,
}

/// Unpacks both tables, or nothing when the stream runs out mid-field.
///
/// `shared_at` is the hint stream's own `/S`: the byte offset, inside the
/// decoded stream, where the shared object hint table begins.
pub(crate) fn decode(data: &[u8], shared_at: usize, page_count: usize) -> Option<Hints> {
    let mut bits = BitReader::new(data);

    // ---- Page offset hint table (F.3, Table F.3) ----
    let least_objects = bits.read(32)?;
    let first_page_offset = bits.read(32)?;
    let object_bits = bits.read(16)? as u16;
    let least_length = bits.read(32)?;
    let length_bits = bits.read(16)? as u16;
    let _least_content_offset = bits.read(32)?;
    let content_offset_bits = bits.read(16)? as u16;
    let _least_content_length = bits.read(32)?;
    let content_length_bits = bits.read(16)? as u16;
    let count_bits = bits.read(16)? as u16;
    let identifier_bits = bits.read(16)? as u16;
    let position_bits = bits.read(16)? as u16;
    let _position_denominator = bits.read(16)?;

    let mut objects = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        objects.push(least_objects.saturating_add(bits.read(object_bits)?));
    }
    bits.align();

    let mut lengths = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        lengths.push(least_length.saturating_add(bits.read(length_bits)?));
    }
    bits.align();

    let mut counts = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        counts.push(bits.read(count_bits)?);
    }
    bits.align();

    let mut shared = Vec::with_capacity(page_count);
    for count in &counts {
        let mut ids = Vec::with_capacity(*count as usize);
        for _ in 0..*count {
            ids.push(bits.read(identifier_bits)?);
        }
        shared.push(ids);
    }
    bits.align();

    // Items 5, 6 and 7: the fractional position, and the content stream's
    // offset and length. Each is a run at a width the header states, and this
    // writer states zero for all three — but a file from anywhere else may
    // not, and reading past a run that is really there would put every later
    // field one column out.
    for width in [position_bits, content_offset_bits, content_length_bits] {
        for _ in 0..page_count {
            bits.read(width)?;
        }
        bits.align();
    }

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
    let mut shared_lengths = Vec::with_capacity(total);
    for _ in 0..total {
        shared_lengths.push(least_group.saturating_add(bits.read(group_bits)?));
    }
    bits.align();

    // Item 2: a one-bit flag per entry saying whether a 128-bit signature
    // follows. Never optional, and writing it at zero width is what once made
    // a reader run off the end of the stream before it had read one entry.
    let mut signed = 0usize;
    for _ in 0..total {
        if bits.read(1)? == 1 {
            signed += 1;
        }
    }
    bits.align();
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

    Some(Hints {
        first_page_offset,
        pages: (0..page_count)
            .map(|index| PageHint {
                objects: objects.get(index).copied().unwrap_or(0),
                length: lengths.get(index).copied().unwrap_or(0),
                shared: shared.get(index).cloned().unwrap_or_default(),
            })
            .collect(),
        first_shared_object,
        first_shared_offset,
        shared_first_page,
        shared_lengths,
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
}
