//! JBIG2 (ITU-T T.88): the generic-region lineage.
//!
//! Scope and the reasoning behind it: `docs/plans/gaps/17-jbig2-generic-region.md`.
//!
//! JBIG2 is two largely separate codecs sharing one container. The **generic
//! region** lineage codes a bilevel bitmap pixel by pixel, either with the MQ
//! arithmetic coder against a template of already-decoded neighbours (6.2.5)
//! or with the same MMR coding a G4 fax uses (6.2.6). That is what a scanner
//! emits. The **symbol dictionary plus text region** lineage builds a
//! dictionary of glyph bitmaps and then places them, and that is what
//! `jbig2enc` and OCRmyPDF emit — most PDFs that have been through an OCR
//! pipeline. This module decodes the first and **refuses the second by name**.
//!
//! # The refusal is the feature
//!
//! The tempting shape is to skip segments this build does not understand and
//! return whatever page came out. A file that is symbol dictionary plus text
//! region would then decode its page information segment, find no generic
//! region, and hand back a **blank white page reported as success** —
//! indistinguishable from a correct decode of a blank scan, and strictly
//! worse than the grey placeholder it replaced, which at least says something
//! is missing.
//!
//! So [`decode`] returns [`FilterError::Unsupported`] when no region was
//! composited onto the page, and the caller draws the placeholder. Ruling 2
//! degrades; it does not invent content. The corpus makes this the *common*
//! path rather than a corner: of the 103 JBIG2 files gap 23 measured, the OCR
//! lineage is the bulk.
//!
//! # Polarity
//!
//! JBIG2 is 1 = black (6.2.2). A 1-bit DeviceGray image is 0 = black. This
//! module returns **JBIG2's own sense**, unconverted, exactly as
//! [`crate::T6Rows`] does — the inversion belongs at the PDF boundary where
//! `/ImageMask` and `/Decode` are also read, not buried in a decoder that
//! would then be guessing which convention its caller wanted.
//!
//! # Allocation
//!
//! Region width and height are attacker-controlled 32-bit values, and a
//! 300 dpi A4 page is 8.7 megabytes at one byte per pixel. Every allocation
//! here goes through [`packed_size`], a checked multiply against the output
//! ceiling, **before** the allocation happens — the pattern `ccitt.rs`
//! already uses. A region declaring 2^32 pixels is refused rather than
//! attempted (ruling 1).

use crate::{Capability, FilterError, Warning};

/// Segment types (T.88 7.3, Table 34) this decoder distinguishes by name.
mod kind {
    pub const SYMBOL_DICTIONARY: u8 = 0;
    pub const INTERMEDIATE_TEXT_REGION: u8 = 4;
    pub const IMMEDIATE_TEXT_REGION: u8 = 6;
    pub const IMMEDIATE_LOSSLESS_TEXT_REGION: u8 = 7;
    pub const PATTERN_DICTIONARY: u8 = 16;
    pub const INTERMEDIATE_HALFTONE_REGION: u8 = 20;
    pub const IMMEDIATE_HALFTONE_REGION: u8 = 22;
    pub const IMMEDIATE_LOSSLESS_HALFTONE_REGION: u8 = 23;
    pub const INTERMEDIATE_GENERIC_REGION: u8 = 36;
    pub const IMMEDIATE_GENERIC_REGION: u8 = 38;
    pub const IMMEDIATE_LOSSLESS_GENERIC_REGION: u8 = 39;
    pub const INTERMEDIATE_REFINEMENT_REGION: u8 = 40;
    pub const IMMEDIATE_REFINEMENT_REGION: u8 = 42;
    pub const IMMEDIATE_LOSSLESS_REFINEMENT_REGION: u8 = 43;
    pub const PAGE_INFORMATION: u8 = 48;
    pub const END_OF_PAGE: u8 = 49;
    pub const END_OF_STRIPE: u8 = 50;
    pub const END_OF_FILE: u8 = 51;
    pub const PROFILES: u8 = 52;
    pub const TABLES: u8 = 53;
    pub const COLOUR_PALETTE: u8 = 54;
    pub const EXTENSION: u8 = 62;
}

/// The eight bytes a standalone JBIG2 file opens with (T.88 D.4.1).
///
/// A PDF stream is the *embedded* organisation (D.3) and carries none of
/// this — but producers that pasted a whole file into a stream exist, and
/// skipping a header that is there costs four lines.
const FILE_HEADER: [u8; 8] = [0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];

/// What the stream tier knows about a JBIG2 image that the coded bytes do
/// not carry (ISO 32000-1 7.4.7).
#[derive(Clone, Copy, Debug)]
pub struct Jbig2Params<'a> {
    /// The `/JBIG2Globals` stream's bytes, already run through its own filter
    /// chain, or empty. Its segments — a shared symbol dictionary, usually —
    /// are visible to every image that names it.
    pub globals: &'a [u8],
    /// The image's declared width in pixels.
    pub width: u32,
    /// The image's declared height in pixels.
    pub height: u32,
}

/// A parsed segment header (T.88 7.2) and the data block that follows it.
///
/// The segment *number* is not kept: 7.2.5 uses it only to decide how wide
/// the referred-to numbers in this same header are, and nothing this build
/// decodes follows a reference. Keeping a field nothing reads is how a
/// half-parsed header comes to look complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment<'a> {
    /// 7.2.3, the low six bits of the header flags.
    kind: u8,
    /// 7.2.6.
    page: u32,
    /// 7.2.7 through 7.2.8: the segment's own data.
    data: &'a [u8],
}

/// A big-endian cursor that runs out rather than panicking.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.data.len()
    }

    fn u8(&mut self) -> Option<u8> {
        let b = self.data.get(self.at).copied()?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) << 8 | u16::from(self.u8()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n).filter(|a| *a <= self.data.len())?;
        Some(())
    }

    /// The next `n` bytes, or everything left when the segment claims more
    /// than the stream holds — which is the truncation case, reported by the
    /// caller rather than swallowed here.
    fn take(&mut self, n: usize) -> (&'a [u8], bool) {
        let end = self.at.saturating_add(n).min(self.data.len());
        let whole = end == self.at.saturating_add(n);
        let slice = self.data.get(self.at..end).unwrap_or(&[]);
        self.at = end;
        (slice, whole)
    }
}

/// Records a leniency once. Same contract as the rest of the crate: one entry
/// per condition per decode, not one per occurrence.
fn note(warnings: &mut Vec<Warning>, warning: Warning) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Splits a stream into segments (T.88 7.2), in the order they appear.
///
/// This is the *embedded* organisation of Annex D.3, which is the only one a
/// PDF may use: every segment header is immediately followed by its own data.
/// The sequential file organisation of D.2 is the same layout with a file
/// header in front, so a whole file pasted into a stream parses too.
///
/// The random-access organisation — all headers, then all data — cannot
/// appear in a PDF and is not guessed at: it is recorded and the stream ends
/// there, because parsing it as sequential would read data blocks as headers
/// and invent segments that are not in the file.
fn segments<'a>(data: &'a [u8], warnings: &mut Vec<Warning>) -> Vec<Segment<'a>> {
    let mut reader = Reader::new(data);
    if data.starts_with(&FILE_HEADER) {
        let _ = reader.skip(FILE_HEADER.len());
        let flags = reader.u8().unwrap_or(0);
        // D.4.2 bit 0: 1 is sequential, 0 is random access. Bit 1: 0 means
        // the number of pages is known and follows as four bytes.
        if flags & 1 == 0 {
            note(warnings, Warning::Jbig2SegmentSkipped);
            return Vec::new();
        }
        if flags & 2 == 0 {
            let _ = reader.skip(4);
        }
    }

    let mut out = Vec::new();
    // A header is eleven bytes at the very least, so this cannot spin: every
    // turn either consumes bytes or breaks.
    while !reader.done() {
        let before = reader.at;
        let Some(segment) = read_segment(&mut reader, warnings) else {
            break;
        };
        if reader.at <= before {
            break;
        }
        out.push(segment);
    }
    out
}

/// One segment header and its data (T.88 7.2).
fn read_segment<'a>(reader: &mut Reader<'a>, warnings: &mut Vec<Warning>) -> Option<Segment<'a>> {
    let number = reader.u32()?;
    let flags = reader.u8()?;
    let kind = flags & 0x3F;
    // 7.2.3 bit 6: the page association field is four bytes rather than one.
    let long_page = flags & 0x40 != 0;

    // 7.2.4: the top three bits of the next byte are the count of referred-to
    // segments — unless they are all set, in which case the whole four bytes
    // are the count and a run of retain flags follows.
    let first = reader.u8()?;
    let count = if first >> 5 == 7 {
        reader.at -= 1;
        let long = reader.u32()? & 0x1FFF_FFFF;
        // ceil((count + 1) / 8) bytes of retain flags. `count` is
        // attacker-controlled, so the skip is checked against the data.
        let retain = (long as usize).checked_add(1)?.div_ceil(8);
        reader.skip(retain)?;
        long
    } else {
        u32::from(first >> 5)
    };

    // 7.2.5: how wide each referred-to segment number is, decided by *this*
    // segment's number rather than by the values being referred to.
    let width = if number <= 256 {
        1
    } else if number <= 65536 {
        2
    } else {
        4
    };
    // A count is up to 2^29, and the referred-to numbers are the only thing
    // between here and the data, so the whole run has to fit in what is left
    // or the header is not a header.
    let referred_bytes = (count as usize).checked_mul(width)?;
    reader.skip(referred_bytes)?;

    let page = if long_page {
        reader.u32()?
    } else {
        u32::from(reader.u8()?)
    };

    let length = reader.u32()?;
    if length == u32::MAX {
        // 7.2.7: an unknown data length is legal only for an immediate
        // generic region, and finding its end means scanning for a row
        // terminator that depends on the region's own coding. Nothing after
        // this segment can be located, so the stream ends here rather than
        // being guessed at.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return None;
    }
    let (data, whole) = reader.take(length as usize);
    if !whole {
        note(warnings, Warning::TruncatedInput);
    }

    Some(Segment { kind, page, data })
}

/// Whether a segment type is one this build decodes.
///
/// Everything else is skipped **and recorded**, which is what keeps the
/// refusal honest: the skip is not silent, and it is not sufficient on its
/// own — a page that ends with no region on it refuses regardless.
fn understood(kind: u8) -> bool {
    matches!(
        kind,
        kind::INTERMEDIATE_GENERIC_REGION
            | kind::IMMEDIATE_GENERIC_REGION
            | kind::IMMEDIATE_LOSSLESS_GENERIC_REGION
            | kind::PAGE_INFORMATION
            | kind::END_OF_PAGE
            | kind::END_OF_STRIPE
            | kind::END_OF_FILE
            | kind::PROFILES
            | kind::EXTENSION
    )
}

/// Whether a segment carries content this build cannot reproduce.
///
/// Distinguished from [`understood`] because the two answer different
/// questions. An extension segment is skippable by design (7.4.14 makes them
/// optional unless a necessity bit says otherwise); a text region is a
/// picture that will be missing from the page. Only the second kind is worth
/// a warning naming a lineage.
fn carries_content(kind: u8) -> bool {
    matches!(
        kind,
        kind::SYMBOL_DICTIONARY
            | kind::INTERMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_LOSSLESS_TEXT_REGION
            | kind::PATTERN_DICTIONARY
            | kind::INTERMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_LOSSLESS_HALFTONE_REGION
            | kind::INTERMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION
            | kind::TABLES
            | kind::COLOUR_PALETTE
    )
}

/// Bytes a packed 1-bpp bitmap of these dimensions occupies, or `None` if it
/// would exceed `ceiling`.
///
/// **Checked before the allocation, not after it.** `width` and `height` come
/// straight off the wire, so `width.div_ceil(8) * height` overflows a 32-bit
/// `usize` for perfectly ordinary-looking garbage and allocates a hundred
/// gigabytes on a 64-bit one. Ruling 1 wants the arithmetic bounded, and the
/// only way to bound it is to do it in `usize` with `checked_mul` before
/// anything reserves memory.
fn packed_size(width: u32, height: u32, ceiling: usize) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = (width as usize).div_ceil(8);
    let bytes = stride.checked_mul(height as usize)?;
    (bytes <= ceiling).then_some(bytes)
}

/// Decodes an embedded JBIG2 stream into packed one-bit-per-pixel rows.
///
/// Rows are `params.width.div_ceil(8)` bytes each, most significant bit
/// first, **1 for black** — JBIG2's own sense (6.2.2), which the caller
/// inverts for PDF's.
///
/// `warnings` is a sink rather than a return value because the refusal below
/// is an `Err`, and ruling 10 wants what was skipped to survive it: "no
/// region, because the file is a symbol dictionary" and "no region, because
/// the stream was truncated" are different failures and the caller cannot
/// tell them apart from the error alone.
///
/// # Errors
/// [`FilterError::Unsupported`] when no region was composited onto the page.
/// That is the whole degradation contract for this codec: the caller draws
/// the neutral placeholder (ruling 2) rather than being handed a blank page
/// that reads as a successful decode of a blank scan.
pub fn decode(
    data: &[u8],
    params: &Jbig2Params<'_>,
    max_output: usize,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<u8>, FilterError> {
    let Some(size) = packed_size(params.width, params.height, max_output) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return Err(FilterError::Unsupported(Capability::Jbig2));
    };
    let mut page = Page {
        bits: vec![0u8; size],
        width: params.width,
        height: params.height,
        number: None,
        regions: 0,
    };

    // D.3: the globals stream's segments are read first and are visible to
    // the page's own, which is how a shared symbol dictionary reaches every
    // image that names it. They are ordinary segments in every other way.
    let globals = segments(params.globals, warnings);
    let own = segments(data, warnings);
    for segment in globals.iter().chain(own.iter()) {
        if !understood(segment.kind) {
            if carries_content(segment.kind) {
                note(warnings, Warning::Jbig2SegmentSkipped);
            }
            continue;
        }
        if !page.owns(segment) {
            continue;
        }
        if segment.kind == kind::PAGE_INFORMATION {
            page.begin(segment, warnings);
        }
    }

    if page.regions == 0 {
        // The refusal. Not polish, and not a fallback: see the module note.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return Err(FilterError::Unsupported(Capability::Jbig2));
    }
    Ok(page.bits)
}

/// The page bitmap regions are composited onto, and what the page
/// information segment said about it.
struct Page {
    /// Packed 1-bpp rows, most significant bit first, 1 = black.
    bits: Vec<u8>,
    width: u32,
    height: u32,
    /// The page association of the page information segment, once one has
    /// been seen. A multi-page JBIG2 file pasted into a PDF stream carries
    /// segments for pages this image is not, and compositing those would
    /// draw another page's content onto this one.
    number: Option<u32>,
    /// How many regions were composited. Zero is the refusal.
    regions: usize,
}

impl Page {
    /// A page information segment (T.88 7.4.8).
    ///
    /// The page's own declared dimensions are read and *not* believed over
    /// the caller's: ISO 32000-1 7.4.7 makes the image dictionary's `/Width`
    /// and `/Height` the authority for an embedded stream, and a striped page
    /// writes `0xFFFFFFFF` for its height precisely because it does not yet
    /// know. What is taken from here is the default pixel value, which
    /// decides whether the page starts black.
    fn begin(&mut self, segment: &Segment<'_>, warnings: &mut Vec<Warning>) {
        let mut reader = Reader::new(segment.data);
        let (Some(width), Some(height)) = (reader.u32(), reader.u32()) else {
            note(warnings, Warning::TruncatedInput);
            return;
        };
        let _ = (reader.u32(), reader.u32()); // x and y resolution
        let Some(flags) = reader.u8() else {
            note(warnings, Warning::TruncatedInput);
            return;
        };
        self.number = Some(segment.page);
        if width != self.width || (height != u32::MAX && height != self.height) {
            // Not fatal and not repaired: the region segments carry their own
            // placement, so a page that disagrees with the dictionary still
            // composites at the coordinates it names. Worth recording,
            // because it is also what a stream pasted from another file looks
            // like.
            note(warnings, Warning::Jbig2SegmentSkipped);
        }
        // 7.4.8.5 bit 2: the value every pixel starts at. A scan of a mostly
        // black page is coded as black-by-default with white regions on it,
        // and ignoring this bit renders it as its own negative.
        if flags & 0x04 != 0 {
            self.bits.fill(0xFF);
        }
    }

    /// Whether a segment's page association names this page.
    ///
    /// Page 0 is the association D.3 gives a segment that belongs to no
    /// particular page — what a `/JBIG2Globals` stream carries — so it always
    /// matches; and until a page information segment has been seen there is
    /// nothing for a segment to disagree with.
    fn owns(&self, segment: &Segment<'_>) -> bool {
        segment.page == 0 || self.number.is_none_or(|n| n == segment.page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A segment header (T.88 7.2) in its short form: no referred-to
    /// segments, one-byte page association.
    fn header(number: u32, kind: u8, page: u8, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&number.to_be_bytes());
        out.push(kind & 0x3F);
        out.push(0); // no referred-to segments, no retain flags
        out.push(page);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    /// A page information segment's nineteen bytes (T.88 7.4.8).
    fn page_info(width: u32, height: u32, flags: u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // x resolution
        out.extend_from_slice(&0u32.to_be_bytes()); // y resolution
        out.push(flags);
        out.extend_from_slice(&0u16.to_be_bytes()); // striping
        out
    }

    #[test]
    fn segments_enumerate_in_order() {
        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &[1, 2, 3]);
        stream.extend(header(1, kind::IMMEDIATE_GENERIC_REGION, 1, &[4, 5]));
        stream.extend(header(2, kind::END_OF_PAGE, 1, &[]));

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].kind, kind::PAGE_INFORMATION);
        assert_eq!(parsed[0].page, 1);
        assert_eq!(parsed[0].data, &[1, 2, 3]);
        assert_eq!(parsed[1].kind, kind::IMMEDIATE_GENERIC_REGION);
        assert_eq!(parsed[1].data, &[4, 5]);
        assert_eq!(parsed[2].data, &[] as &[u8]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_whole_file_pasted_into_a_stream_still_parses() {
        // D.4: file header, sequential organisation, number of pages known.
        let mut stream = FILE_HEADER.to_vec();
        stream.push(0b01); // sequential, page count present
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend(header(0, kind::PAGE_INFORMATION, 1, &[9]));

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data, &[9]);
    }

    #[test]
    fn random_access_organisation_is_recorded_rather_than_guessed() {
        let mut stream = FILE_HEADER.to_vec();
        stream.push(0b10); // random access, page count absent
        stream.extend(header(0, kind::PAGE_INFORMATION, 1, &[9]));

        let mut warnings = Vec::new();
        assert!(segments(&stream, &mut warnings).is_empty());
        assert_eq!(warnings, vec![Warning::Jbig2SegmentSkipped]);
    }

    #[test]
    fn long_form_referred_to_count_skips_its_retain_flags() {
        // 7.2.4 long form: 0b111 in the top three bits, count in the low 29,
        // then ceil((count + 1) / 8) retain bytes, then `count` one-byte
        // referred-to numbers.
        let mut stream = Vec::new();
        stream.extend_from_slice(&7u32.to_be_bytes()); // segment number 7
        stream.push(kind::IMMEDIATE_GENERIC_REGION);
        stream.extend_from_slice(&(0xE000_0000u32 | 9).to_be_bytes());
        stream.extend_from_slice(&[0u8; 2]); // ceil(10 / 8) retain bytes
        stream.extend_from_slice(&[0u8; 9]); // nine one-byte referred numbers
        stream.push(1); // page
        stream.extend_from_slice(&2u32.to_be_bytes());
        stream.extend_from_slice(&[0xAB, 0xCD]);

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1, "the header's own length arithmetic is off");
        assert_eq!(parsed[0].data, &[0xAB, 0xCD]);
    }

    #[test]
    fn referred_to_numbers_widen_with_the_segment_number() {
        // 7.2.5: a segment numbered above 65536 refers with four-byte
        // numbers. Reading them as one byte each would put the page
        // association nine bytes early and invent a segment.
        let mut stream = Vec::new();
        stream.extend_from_slice(&70_000u32.to_be_bytes());
        stream.push(0x40 | kind::IMMEDIATE_GENERIC_REGION); // four-byte page
        stream.push(2 << 5); // two referred-to segments
        stream.extend_from_slice(&[0u8; 8]);
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.push(0x5A);

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].page, 1);
        assert_eq!(
            parsed[0].data,
            &[0x5A],
            "the referred-to run was read at the wrong width, so the page \
             association and the length came from the middle of it"
        );
    }

    #[test]
    fn a_segment_claiming_more_than_the_stream_holds_is_truncated_and_named() {
        let mut stream = header(0, kind::IMMEDIATE_GENERIC_REGION, 1, &[1, 2, 3]);
        // Rewrite the length to claim far more than follows.
        let at = stream.len() - 4 - 3;
        stream[at..at + 4].copy_from_slice(&999u32.to_be_bytes());

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data, &[1, 2, 3]);
        assert_eq!(warnings, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn an_unknown_data_length_ends_the_stream_rather_than_guessing() {
        let mut stream = header(0, kind::IMMEDIATE_GENERIC_REGION, 1, &[1]);
        let at = stream.len() - 5;
        stream[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());

        let mut warnings = Vec::new();
        assert!(segments(&stream, &mut warnings).is_empty());
        assert_eq!(warnings, vec![Warning::Jbig2SegmentSkipped]);
    }

    /// The lineage this plan does not build, named rather than absorbed.
    #[test]
    fn a_symbol_dictionary_file_refuses_and_says_so() {
        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &page_info(64, 56, 0));
        stream.extend(header(1, kind::SYMBOL_DICTIONARY, 1, &[0u8; 4]));
        stream.extend(header(2, kind::IMMEDIATE_TEXT_REGION, 1, &[0u8; 4]));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let out = decode(&stream, &params, 1 << 20, &mut warnings);
        assert_eq!(out, Err(FilterError::Unsupported(Capability::Jbig2)));
        assert!(warnings.contains(&Warning::Jbig2SegmentSkipped));
    }

    /// A globals stream's segments are read, and read *first*.
    #[test]
    fn globals_segments_are_enumerated_before_the_stream_s_own() {
        let globals = header(0, kind::SYMBOL_DICTIONARY, 0, &[0u8; 4]);
        let stream = header(1, kind::PAGE_INFORMATION, 1, &page_info(8, 8, 0));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &globals,
            width: 8,
            height: 8,
        };
        assert_eq!(
            decode(&stream, &params, 1 << 20, &mut warnings),
            Err(FilterError::Unsupported(Capability::Jbig2))
        );
        assert_eq!(
            warnings,
            vec![Warning::Jbig2SegmentSkipped],
            "the globals' symbol dictionary has to be seen, or a file whose \
             whole payload is shared would refuse without saying why"
        );
    }

    /// 7.4.8.5 bit 2: a page that starts black.
    ///
    /// The refusal hides the page from a caller, so the bitmap is checked
    /// through the one thing that survives it — a region composited onto it
    /// arrives in the next milestone, so for now the fill is checked where it
    /// happens.
    #[test]
    fn the_page_default_pixel_value_starts_the_page_black() {
        let mut page = Page {
            bits: vec![0u8; 8],
            width: 8,
            height: 8,
            number: None,
            regions: 0,
        };
        let data = page_info(8, 8, 0x04);
        let segment = Segment {
            kind: kind::PAGE_INFORMATION,
            page: 1,
            data: &data,
        };
        let mut warnings = Vec::new();
        page.begin(&segment, &mut warnings);
        assert_eq!(page.bits, vec![0xFF; 8], "1 is black (6.2.2)");
        assert_eq!(page.number, Some(1));
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_segment_for_another_page_is_not_composited_onto_this_one() {
        let data = page_info(8, 8, 0);
        let mut page = Page {
            bits: vec![0u8; 8],
            width: 8,
            height: 8,
            number: None,
            regions: 0,
        };
        let mut warnings = Vec::new();
        page.begin(
            &Segment {
                kind: kind::PAGE_INFORMATION,
                page: 1,
                data: &data,
            },
            &mut warnings,
        );
        let elsewhere = Segment {
            kind: kind::IMMEDIATE_GENERIC_REGION,
            page: 2,
            data: &[],
        };
        let globalish = Segment {
            kind: kind::IMMEDIATE_GENERIC_REGION,
            page: 0,
            data: &[],
        };
        assert!(!page.owns(&elsewhere));
        assert!(page.owns(&globalish));
    }

    #[test]
    fn a_region_declaring_four_billion_pixels_is_refused_before_allocating() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: u32::MAX,
            height: u32::MAX,
        };
        let out = decode(&[], &params, 1 << 20, &mut warnings);
        assert_eq!(out, Err(FilterError::Unsupported(Capability::Jbig2)));
        assert_eq!(warnings, vec![Warning::Jbig2RegionTooLarge]);
    }

    /// Both halves of the bound, because they catch on different targets.
    ///
    /// `checked_mul` is what saves a 32-bit `usize` — wasm32 is a first-class
    /// target here — where `2^32 / 8 * 2^32` does not fit at all. On a 64-bit
    /// one it fits comfortably, as two exabytes, and only the ceiling refuses
    /// it. A test that asserted the multiply alone would pass on wasm and
    /// prove nothing on the machine most of this is built on.
    #[test]
    fn packed_size_refuses_what_it_cannot_multiply_or_cannot_afford() {
        assert_eq!(packed_size(8, 2, 1024), Some(2));
        assert_eq!(packed_size(9, 2, 1024), Some(4));
        assert_eq!(packed_size(0, 2, 1024), None);
        assert_eq!(packed_size(8, 0, 1024), None);
        assert_eq!(packed_size(64, 64, 100), None, "past the ceiling");
        assert_eq!(
            packed_size(u32::MAX, u32::MAX, 1 << 28),
            None,
            "2^32 by 2^32 must not reach an allocator on any target"
        );
        #[cfg(target_pointer_width = "32")]
        assert_eq!(packed_size(u32::MAX, u32::MAX, usize::MAX), None);
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x1234_5678u32;
        for _ in 0..2048 {
            let len = (seed % 96) as usize;
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                bytes.push((seed >> 16) as u8);
            }
            let mut warnings = Vec::new();
            let params = Jbig2Params {
                globals: &bytes,
                width: 32,
                height: 32,
            };
            let _ = decode(&bytes, &params, 1 << 16, &mut warnings);
        }
    }
}
