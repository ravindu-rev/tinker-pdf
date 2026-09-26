//! What the corpus's JBIG2 actually uses (roadmap Tier 2, milestone 1).
//!
//! The symbol-dictionary-plus-text-region lineage is the engine's
//! highest-reachability refusal — 103 files in the pdf.js corpus — and it has
//! two coding variants and an optional refinement stage. Which of them to build
//! first is a question about real files rather than about the standard, so
//! ruling 3 says measure before staging: this walks every JBIG2 stream in the
//! fetched corpora and tallies the flags that decide it.
//!
//! **The parser here is written from ITU-T T.88 clause 7.2 and shares nothing
//! with `tinker-pdf-filters`.** It has to: a census taken with the decoder's own
//! header reader would agree with that reader by construction, including
//! wherever it is wrong, and the whole point is to find out what is out there.
//! It also has to survive segments the decoder refuses, which is most of them
//! today.
//!
//! # The half that has to use the decoder, and why it is not a retreat
//!
//! *Added 26 September 2026, for `docs/verification.md`'s `jbig2` fuzz row.*
//! One figure this census is asked for cannot be reached from a header at all:
//! **how large a dictionary's symbols are.** 6.5.5 accumulates a symbol's
//! height from `IADH` deltas and its width from `IADW` deltas *inside the
//! arithmetic coder*, and on the Huffman road from Annex B deltas inside the bit
//! stream, so neither dimension is a field anywhere in clause 7.4.3. A walk that
//! shares nothing with the decoder therefore cannot see one, and [`Tally`]'s
//! `max_symbol_pixels` field sat declared, merged by [`Tally::add`] and assigned
//! by nothing from the day this file was written — reading a structural zero
//! over every file, which is why it was printed as `not measured` instead.
//!
//! [`measure`] takes that figure by *decoding*, through
//! [`tinker_pdf_filters::jbig2_decode_measured`]. The independence above is not
//! given up, because it was never independence about this: the argument for it is
//! that a census of *what the format says* must not agree with the decoder by
//! construction, and a symbol's size is not something the format says — it is
//! something the coded data means, and there is exactly one thing in this
//! repository that knows what coded data means. What the census keeps instead is
//! its other property: a stream it could not walk is counted as unwalkable
//! rather than guessed at, so an image whose decode refused is counted in
//! `images_refused` and its figures read as a floor.
//!
//! Run it, and record what it says in `docs/design/jbig2-symbol-text.md`:
//!
//! ```sh
//! cargo test -p tinker-pdf --test jbig2_census -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::Document;
use tinker_pdf_cos::{ObjRef, Object, XrefEntry};
use tinker_pdf_filters::{jbig2_decode_measured, Jbig2Params, MAX_JBIG2_SYMBOL_PAGE_MULTIPLE};

/// The ceiling the measurement below decodes under, matching
/// `jbig2_attribution.rs` so the two censuses are commensurable.
const CEILING: usize = 1 << 22;

/// How tightly one file's largest symbol fits the page it is drawn onto.
struct Fit<'a> {
    /// Thousandths of the page, so that `1.000` — a symbol exactly its page's
    /// size — is distinguishable from any glyph.
    permille: u32,
    /// The same, rounded up to the whole multiple the bound is charged in.
    multiple: u32,
    name: &'a str,
    /// The widest and the tallest symbol, and the page both were measured
    /// against.
    symbol: (u32, u32),
    page: (u32, u32),
}

/// A thousandth, printed as a fraction of one rather than as a permille.
///
/// `1.000` is the interesting reading — a symbol exactly its page's size — and
/// `1000` beside a column of whole multiples reads like a whole multiple.
fn permille_of(permille: u32) -> String {
    format!("{}.{:03}", permille / 1_000, permille % 1_000)
}

/// Segment types this census names (T.88 Table 34).
mod kind {
    pub const SYMBOL_DICTIONARY: u8 = 0;
    pub const INTERMEDIATE_TEXT_REGION: u8 = 4;
    pub const IMMEDIATE_TEXT_REGION: u8 = 6;
    pub const IMMEDIATE_LOSSLESS_TEXT_REGION: u8 = 7;
    pub const PATTERN_DICTIONARY: u8 = 16;
    pub const INTERMEDIATE_HALFTONE_REGION: u8 = 20;
    pub const IMMEDIATE_HALFTONE_REGION: u8 = 22;
    pub const IMMEDIATE_LOSSLESS_HALFTONE_REGION: u8 = 23;
    pub const INTERMEDIATE_REFINEMENT_REGION: u8 = 40;
    pub const IMMEDIATE_REFINEMENT_REGION: u8 = 42;
    pub const IMMEDIATE_LOSSLESS_REFINEMENT_REGION: u8 = 43;
    pub const TABLES: u8 = 53;
}

/// A big-endian reader that answers `None` rather than panicking, because every
/// byte here is attacker-controlled and half of them are truncated.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    fn u16(&mut self) -> Option<u16> {
        let slice = self.bytes.get(self.at..self.at + 2)?;
        self.at += 2;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let slice = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n)?;
        (self.at <= self.bytes.len()).then_some(())
    }
}

/// What one file's JBIG2 uses.
#[derive(Default, Clone)]
struct Tally {
    segments: BTreeMap<u8, u32>,
    /// Symbol dictionaries coded with Huffman tables (7.4.3.1.1 bit 0).
    sdhuff: u32,
    /// Symbol dictionaries using refinement/aggregate coding (bit 1).
    sdrefagg: u32,
    /// Symbol dictionaries that use a retained context (bit 8).
    context_used: u32,
    /// Symbol dictionaries that retain their context (bit 9).
    context_retained: u32,
    /// Text regions coded with Huffman tables (7.4.4.1.1 bit 0).
    sbhuff: u32,
    /// Text regions that refine their symbols (bit 1).
    sbrefine: u32,
    /// Refining symbol dictionaries at refinement template 1 (bit 12), whose
    /// pixel set Annex H does not pin — see `refinement_context`.
    sdrtemplate1: u32,
    /// Refining text regions at refinement template 1 (bit 15).
    sbrtemplate1: u32,
    /// Segments that want refinement over the Huffman road: `SDHUFF` with
    /// `SDREFAGG`, or `SBHUFF` with `SBREFINE`. Refused by name, so the count
    /// is what decides whether that refusal is worth closing.
    huffman_refinement: u32,
    /// 7.4.4.1.2's refinement table selectors on a Huffman refining text
    /// region: `SBHUFFRDW`, `RDH`, `RDX`, `RDY` (0 = B.14, 1 = B.15,
    /// 3 = custom) and `SBHUFFRSIZE` (0 = B.1, 1 = custom). Which of these
    /// appear is what decides which of Annex B has to exist.
    refine_selectors: [[u32; 4]; 4],
    rsize_selector: [u32; 2],
    /// Generic refinement region segments (types 40, 42, 43) by template
    /// (7.4.7.2 bit 0), and how many of them set TPGRON (bit 1).
    grtemplate: [u32; 2],
    tpgron: u32,
    /// Text regions by REFCORNER (bits 4-5): which corner of a symbol its
    /// coordinate names, and therefore where the symbol is put.
    corners: [u32; 4],
    /// Text regions with TRANSPOSED set (bit 6): S runs down the page rather
    /// than across it.
    transposed: u32,
    /// Text regions with a non-zero SBDSOFFSET (bits 10-14, signed).
    dsoffset: u32,
    /// Text regions with more than one strip (bits 2-3, LOGSBSTRIPS).
    striped: u32,
    /// Headers that would not parse — a truncated or damaged stream.
    unparsable: u32,
    /// The largest `SDNUMNEWSYMS` any one dictionary declares.
    max_new_symbols: u32,
    /// The largest `SDNUMEXSYMS` any one dictionary declares.
    max_exported_symbols: u32,
    /// The largest total `width x height` over one dictionary's symbols, which
    /// is what a pixel budget is a budget of. Zero where the dictionary could
    /// not be walked far enough to know.
    ///
    /// **Assigned since 26 September 2026, and it is not the header walk that
    /// assigns it.** See [`measure`] for why it cannot be: neither dimension of
    /// a symbol is in any segment header.
    max_symbol_pixels: u64,
    /// The largest `SBNUMINSTANCES` any one text region declares.
    max_instances: u32,

    // --- the measurement, from the decoder rather than from a header ---
    /// The most pixels any *one* symbol occupies, and its dimensions.
    max_one_symbol_pixels: u64,
    max_one_symbol: (u32, u32),
    /// The widest and the tallest single symbol, which are three different
    /// symbols from the one above as often as not.
    widest_symbol: u32,
    tallest_symbol: u32,
    /// **The figure a per-symbol bound has to clear**: the smallest whole
    /// multiple of the page's own width and height that admits every symbol in
    /// this file, over every image in it. One means no symbol is larger than
    /// the page it is drawn onto.
    worst_multiple: u32,
    /// The same thing in thousandths rather than rounded up to a whole
    /// multiple, because the whole multiple cannot tell a glyph a fiftieth of
    /// its page wide from a symbol that fills it exactly — and which of those
    /// the corpus's tightest fit is decides whether a bound of one is a bound
    /// or the measurement itself.
    tightest_permille: u32,
    /// The symbol and the page that produced *that* figure, so the number and
    /// the evidence for it always name the same image. Selecting them on the
    /// whole multiple instead printed a pair from one image beside a ratio from
    /// another, which is a report that cannot be checked.
    tightest_symbol: (u32, u32),
    tightest_page: (u32, u32),
    /// Symbols decoded, and dictionaries decoded to the end.
    symbols_decoded: u64,
    dictionaries_decoded: u32,
    /// Images whose decode refused, so their figures above are a floor rather
    /// than a measurement — the census's own property, kept: a stream it could
    /// not walk is counted as unwalkable rather than guessed at.
    images_refused: u32,
    images_measured: u32,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        for (kind, count) in &other.segments {
            *self.segments.entry(*kind).or_default() += count;
        }
        self.sdhuff += other.sdhuff;
        self.sdrefagg += other.sdrefagg;
        self.context_used += other.context_used;
        self.context_retained += other.context_retained;
        self.sbhuff += other.sbhuff;
        self.sbrefine += other.sbrefine;
        self.sdrtemplate1 += other.sdrtemplate1;
        self.sbrtemplate1 += other.sbrtemplate1;
        for (slot, count) in self.grtemplate.iter_mut().zip(other.grtemplate) {
            *slot += count;
        }
        self.tpgron += other.tpgron;
        self.huffman_refinement += other.huffman_refinement;
        for (row, other_row) in self.refine_selectors.iter_mut().zip(other.refine_selectors) {
            for (slot, count) in row.iter_mut().zip(other_row) {
                *slot += count;
            }
        }
        for (slot, count) in self.rsize_selector.iter_mut().zip(other.rsize_selector) {
            *slot += count;
        }
        for (slot, count) in self.corners.iter_mut().zip(other.corners) {
            *slot += count;
        }
        self.transposed += other.transposed;
        self.dsoffset += other.dsoffset;
        self.striped += other.striped;
        self.unparsable += other.unparsable;
        self.max_new_symbols = self.max_new_symbols.max(other.max_new_symbols);
        self.max_exported_symbols = self.max_exported_symbols.max(other.max_exported_symbols);
        self.max_symbol_pixels = self.max_symbol_pixels.max(other.max_symbol_pixels);
        self.max_instances = self.max_instances.max(other.max_instances);
        if other.max_one_symbol_pixels > self.max_one_symbol_pixels {
            self.max_one_symbol_pixels = other.max_one_symbol_pixels;
            self.max_one_symbol = other.max_one_symbol;
        }
        self.widest_symbol = self.widest_symbol.max(other.widest_symbol);
        self.tallest_symbol = self.tallest_symbol.max(other.tallest_symbol);
        self.worst_multiple = self.worst_multiple.max(other.worst_multiple);
        // `>` rather than `>=`, so the first file in sort order wins a tie and
        // the figure this prints is the same on every target (ruling 4).
        if other.tightest_permille > self.tightest_permille {
            self.tightest_permille = other.tightest_permille;
            self.tightest_symbol = other.tightest_symbol;
            self.tightest_page = other.tightest_page;
        }
        self.symbols_decoded += other.symbols_decoded;
        self.dictionaries_decoded += other.dictionaries_decoded;
        self.images_refused += other.images_refused;
        self.images_measured += other.images_measured;
    }

    fn count(&self, kind: u8) -> u32 {
        self.segments.get(&kind).copied().unwrap_or(0)
    }

    fn symbol_dictionaries(&self) -> u32 {
        self.count(kind::SYMBOL_DICTIONARY)
    }

    fn text_regions(&self) -> u32 {
        self.count(kind::INTERMEDIATE_TEXT_REGION)
            + self.count(kind::IMMEDIATE_TEXT_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_TEXT_REGION)
    }

    fn refinement_regions(&self) -> u32 {
        self.count(kind::INTERMEDIATE_REFINEMENT_REGION)
            + self.count(kind::IMMEDIATE_REFINEMENT_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION)
    }

    fn halftone(&self) -> u32 {
        self.count(kind::PATTERN_DICTIONARY)
            + self.count(kind::INTERMEDIATE_HALFTONE_REGION)
            + self.count(kind::IMMEDIATE_HALFTONE_REGION)
            + self.count(kind::IMMEDIATE_LOSSLESS_HALFTONE_REGION)
    }
}

/// Walks one embedded JBIG2 stream's segment headers (T.88 clause 7.2, in the
/// PDF organisation of ISO 32000-1 7.4.7: no file header, headers and data
/// together, in order).
fn census_stream(bytes: &[u8], tally: &mut Tally) {
    let mut reader = Reader::new(bytes);
    loop {
        let Some(segment) = read_segment(&mut reader) else {
            // The end of the stream, or a header that stopped making sense.
            // Both end the walk; only the second is worth counting, and the
            // difference is whether anything was left.
            if reader.at < bytes.len() {
                tally.unparsable += 1;
            }
            return;
        };
        *tally.segments.entry(segment.kind).or_default() += 1;

        let mut data = Reader::new(segment.data);
        match segment.kind {
            kind::SYMBOL_DICTIONARY => {
                // 7.4.3.1.1: two bytes of flags open the data part.
                if let Some(flags) = data.u16() {
                    if flags & 0x0001 != 0 {
                        tally.sdhuff += 1;
                    }
                    if flags & 0x0002 != 0 {
                        tally.sdrefagg += 1;
                    }
                    if flags & 0x0100 != 0 {
                        tally.context_used += 1;
                    }
                    if flags & 0x0200 != 0 {
                        tally.context_retained += 1;
                    }

                    // 7.4.3.1.2 onwards: the AT pixels sit between the flags
                    // and the two counts, and how many there are depends on
                    // the flags just read. Getting this wrong reads the counts
                    // from the wrong offset, so it is worth being explicit.
                    let huff = flags & 0x0001 != 0;
                    let refagg = flags & 0x0002 != 0;
                    let template = (flags >> 10) & 0x0003;
                    let rtemplate = (flags >> 12) & 0x0001;
                    if refagg && rtemplate == 1 {
                        tally.sdrtemplate1 += 1;
                    }
                    if refagg && huff {
                        tally.huffman_refinement += 1;
                    }
                    let mut ok = Some(());
                    if !huff {
                        ok = data.skip(if template == 0 { 8 } else { 2 });
                    }
                    if ok.is_some() && refagg && rtemplate == 0 {
                        ok = data.skip(4);
                    }
                    if ok.is_some() {
                        if let (Some(exported), Some(new)) = (data.u32(), data.u32()) {
                            tally.max_exported_symbols = tally.max_exported_symbols.max(exported);
                            tally.max_new_symbols = tally.max_new_symbols.max(new);
                        }
                    }
                }
            }
            kind::INTERMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_LOSSLESS_TEXT_REGION => {
                // 7.4.1: the region segment information field is seventeen
                // bytes — width, height, x, y, then one byte of flags — and
                // 7.4.4.1.1's own flags follow it.
                if let Some(flags) = data.skip(17).and_then(|()| data.u16()) {
                    if flags & 0x0001 != 0 {
                        tally.sbhuff += 1;
                    }
                    if flags & 0x0002 != 0 {
                        tally.sbrefine += 1;
                        if (flags >> 15) & 1 == 1 {
                            tally.sbrtemplate1 += 1;
                        }
                        if flags & 0x0001 != 0 {
                            tally.huffman_refinement += 1;
                        }
                    }
                    tally.corners[((flags >> 4) & 3) as usize] += 1;
                    if flags & 0x0040 != 0 {
                        tally.transposed += 1;
                    }
                    if (flags >> 10) & 0x1F != 0 {
                        tally.dsoffset += 1;
                    }
                    // 7.4.4: the Huffman flags, then the refinement AT
                    // pixels, then the instance count.
                    let mut ok = Some(());
                    if flags & 0x0001 != 0 {
                        // 7.4.4.1.2's selectors, and for a refining region the
                        // four refinement tables plus the size table are the
                        // part that decides what Annex B must carry.
                        match data.u16() {
                            Some(selectors) => {
                                if flags & 0x0002 != 0 {
                                    for (index, shift) in [6u32, 8, 10, 12].iter().enumerate() {
                                        let value = ((selectors >> shift) & 0x0003) as usize;
                                        tally.refine_selectors[index][value] += 1;
                                    }
                                    let rsize = ((selectors >> 14) & 0x0001) as usize;
                                    tally.rsize_selector[rsize] += 1;
                                }
                            }
                            None => ok = None,
                        }
                    }
                    if ok.is_some() && flags & 0x0002 != 0 && (flags >> 15) & 1 == 0 {
                        ok = data.skip(4);
                    }
                    if ok.is_some() {
                        if let Some(instances) = data.u32() {
                            tally.max_instances = tally.max_instances.max(instances);
                        }
                    }
                    if (flags >> 2) & 3 != 0 {
                        tally.striped += 1;
                    }
                }
            }
            kind::INTERMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION => {
                // 7.4.7.2's one flags byte, after the seventeen of region info.
                if let Some(flags) = data.skip(17).and_then(|()| data.u8()) {
                    tally.grtemplate[usize::from(flags & 0x01)] += 1;
                    if flags & 0x02 != 0 {
                        tally.tpgron += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

struct Segment<'a> {
    kind: u8,
    data: &'a [u8],
}

/// One segment header and its data, per T.88 7.2.
fn read_segment<'a>(reader: &mut Reader<'a>) -> Option<Segment<'a>> {
    let number = reader.u32()?;
    let flags = reader.u8()?;
    let kind = flags & 0x3F;
    // 7.2.3 bit 6: the page association is four bytes rather than one.
    let long_page = flags & 0x40 != 0;

    // 7.2.4: the top three bits of the next byte hold the referred-to count,
    // unless all three are set — then the whole four bytes are the count and a
    // run of retain flags follows it.
    let first = reader.u8()?;
    let count = if first >> 5 == 7 {
        reader.at -= 1;
        let long = reader.u32()? & 0x1FFF_FFFF;
        reader.skip((long as usize).checked_add(1)?.div_ceil(8))?;
        long
    } else {
        u32::from(first >> 5)
    };

    // 7.2.5: the width of each referred-to number is decided by *this*
    // segment's number, not by the values being referred to.
    let width = if number <= 256 {
        1
    } else if number <= 65536 {
        2
    } else {
        4
    };
    reader.skip((count as usize).checked_mul(width)?)?;

    if long_page {
        reader.u32()?;
    } else {
        reader.u8()?;
    }

    let length = reader.u32()?;
    if length == u32::MAX {
        // 7.2.7: an unknown length is legal only for an immediate generic
        // region and nothing after it can be located. The census stops here
        // rather than guessing, and says so by leaving bytes unread.
        return None;
    }
    let start = reader.at;
    reader.skip(length as usize)?;
    let data = reader.bytes.get(start..start + length as usize)?;
    Some(Segment { kind, data })
}

/// One JBIG2 image, as the decoder takes it.
///
/// Separate from the header walk's own list of streams, and not a refinement of
/// it: the walk reads `stream_raw` because it is parsing segment headers out of
/// whatever the file literally holds, and a decode needs `stream_decoded` —
/// which stops *at* the image filter, so a `[FlateDecode, JBIG2Decode]` chain
/// arrives inflated and a bare `JBIG2Decode` arrives unchanged.
struct Image {
    data: Vec<u8>,
    globals: Vec<u8>,
    /// ISO 32000-1 7.4.7 makes the image dictionary's `/Width` and `/Height`
    /// the authority for an embedded stream, so this is the page geometry the
    /// decoder is given and the one a per-symbol bound is charged against.
    width: u32,
    height: u32,
}

/// Every JBIG2 stream in one document: its bytes, and its globals — and, beside
/// them, the same images in the shape a decode takes.
fn jbig2_streams(bytes: Vec<u8>) -> (Vec<Vec<u8>>, Vec<Image>) {
    let Ok(doc) = Document::open(bytes) else {
        return (Vec::new(), Vec::new());
    };
    let cos = doc.cos();
    let filter = cos.intern(b"Filter");
    let parms = cos.intern(b"DecodeParms");
    let globals = cos.intern(b"JBIG2Globals");

    let names = |object: &Object| -> bool {
        let named = |o: &Object| {
            o.as_name()
                .and_then(|n| cos.name_bytes(n))
                .is_some_and(|b| b.as_ref() == b"JBIG2Decode")
        };
        match object {
            Object::Array(items) => items.iter().any(named),
            other => named(other),
        }
    };

    let width_key = cos.intern(b"Width");
    let height_key = cos.intern(b"Height");

    let mut out = Vec::new();
    let mut images = Vec::new();
    for (number, entry) in cos.xref().iter() {
        if number == 0 || matches!(entry, XrefEntry::Free { .. }) {
            continue;
        }
        let generation = match entry {
            XrefEntry::Offset { gen, .. } => gen,
            _ => 0,
        };
        let reference = ObjRef::new(number, generation);
        let Ok(object) = cos.get(reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        if !names(&cos.resolve_key(dict, filter)) {
            continue;
        }
        // The globals stream is a separate object the image points at, and it
        // carries the shared symbol dictionaries — which is exactly what this
        // census is counting, so it has to be walked too.
        let mut shared = Vec::new();
        let parms_value = cos.resolve_key(dict, parms);
        if let Some(parms_dict) = parms_value.as_dict() {
            if let Some(reference) = parms_dict.get_ref(globals) {
                if let Ok(data) = cos.stream_decoded(reference) {
                    shared = data.clone();
                    out.push(data);
                }
            }
        }
        if let Ok(data) = cos.stream_raw(reference) {
            out.push(data);
        }
        // And the same image again, in the shape a decode takes. A missing or
        // unreadable `/Width` or `/Height` leaves it out of the measurement
        // rather than guessing a page: the ratio is *against* that geometry, so
        // an invented one would invent the answer.
        let (Some(width), Some(height)) = (
            cos.resolve_key(dict, width_key)
                .as_int()
                .and_then(|v| u32::try_from(v).ok()),
            cos.resolve_key(dict, height_key)
                .as_int()
                .and_then(|v| u32::try_from(v).ok()),
        ) else {
            continue;
        };
        if let Ok(data) = cos.stream_decoded(reference) {
            images.push(Image {
                data,
                globals: shared,
                width,
                height,
            });
        }
    }
    (out, images)
}

/// **The measurement the header walk cannot take**, over one file's images.
///
/// Neither of a symbol's dimensions is in a segment header. 6.5.5 accumulates
/// both from `IADH` and `IADW` deltas *inside* the arithmetic coder, and on the
/// Huffman road from Annex B deltas inside the bit stream — so the only way to
/// know how large a dictionary's symbols are is to decode it. That is why
/// `Tally::max_symbol_pixels` sat declared, merged and unassigned from the day
/// this file was written: the walk above shares nothing with the decoder on
/// purpose, and this one figure is not reachable from where it stands.
///
/// So this half of the census uses [`jbig2_decode_measured`] — the same decode
/// `jbig2_attribution.rs` runs, with 6.5.5's own tally handed back. The
/// census's property is kept rather than dropped: an image whose decode refused
/// is counted in `images_refused` and its figures are a **floor**, because a
/// dictionary cut short asked for at least what it got.
fn measure(images: &[Image], tally: &mut Tally) {
    for image in images {
        let params = Jbig2Params {
            globals: &image.globals,
            width: image.width,
            height: image.height,
        };
        let mut refusals = Vec::new();
        let (out, extent) = jbig2_decode_measured(&image.data, &params, CEILING, &mut refusals);
        tally.images_measured += 1;
        if out.is_err() {
            tally.images_refused += 1;
        }
        tally.max_symbol_pixels = tally.max_symbol_pixels.max(extent.total_pixels);
        if extent.largest_pixels > tally.max_one_symbol_pixels {
            tally.max_one_symbol_pixels = extent.largest_pixels;
            tally.max_one_symbol = extent.largest;
        }
        tally.widest_symbol = tally.widest_symbol.max(extent.widest);
        tally.tallest_symbol = tally.tallest_symbol.max(extent.tallest);
        tally.symbols_decoded += extent.symbols;
        tally.dictionaries_decoded += extent.dictionaries;

        // The page's own width and height, floored at one so a degenerate
        // `/Width 0` divides rather than panics — and a page of one pixel is a
        // real thing the fuzz target asks for.
        let (page_w, page_h) = (image.width.max(1), image.height.max(1));
        let multiple = extent
            .widest
            .div_ceil(page_w)
            .max(extent.tallest.div_ceil(page_h));
        tally.worst_multiple = tally.worst_multiple.max(multiple);
        let permille = |symbol: u32, page: u32| {
            u32::try_from(u64::from(symbol) * 1_000 / u64::from(page)).unwrap_or(u32::MAX)
        };
        let permille = permille(extent.widest, page_w).max(permille(extent.tallest, page_h));
        if permille > tally.tightest_permille {
            tally.tightest_permille = permille;
            tally.tightest_symbol = (extent.widest, extent.tallest);
            tally.tightest_page = (page_w, page_h);
        }
    }
}

/// The corpora, if they have been fetched.
fn corpus_root() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_CORPUS") {
        let path = PathBuf::from(named);
        return path.is_dir().then_some(path);
    }
    // `cargo test` runs with the crate root as the working directory.
    let guess = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()?;
    guess.is_dir().then_some(guess)
}

fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

/// **What the corpus's JBIG2 actually uses.** Ignored by default: it reads
/// thousands of fetched files and answers a scheduling question rather than
/// asserting a property.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_jbig2() {
    let Some(root) = corpus_root() else {
        println!("jbig2-census: SKIPPED (no corpus; set TINKER_CORPUS)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!(
        "jbig2-census: RAN over {} files under {}",
        files.len(),
        root.display()
    );

    let mut total = Tally::default();
    let mut carriers = 0u32;
    let mut per_file: Vec<(String, Tally)> = Vec::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let (streams, images) = jbig2_streams(bytes);
        if streams.is_empty() {
            continue;
        }
        let mut tally = Tally::default();
        for stream in &streams {
            census_stream(stream, &mut tally);
        }
        if tally.segments.is_empty() {
            continue;
        }
        measure(&images, &mut tally);
        carriers += 1;
        total.add(&tally);
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        per_file.push((name, tally));
    }

    println!("\n{carriers} files carry JBIG2\n");
    println!(
        "{:<58} {:>4} {:>4} {:>5} {:>5} {:>5} {:>5} {:>4}",
        "file", "sdic", "text", "sdhuf", "sbhuf", "refag", "refin", "ctx"
    );
    for (name, t) in &per_file {
        println!(
            "{:<58} {:>4} {:>4} {:>5} {:>5} {:>5} {:>5} {:>4}",
            if name.len() > 58 {
                &name[name.len() - 58..]
            } else {
                name
            },
            t.symbol_dictionaries(),
            t.text_regions(),
            t.sdhuff,
            t.sbhuff,
            t.sdrefagg,
            t.sbrefine,
            t.context_used + t.context_retained
        );
    }

    let files_with = |f: fn(&Tally) -> u32| per_file.iter().filter(|(_, t)| f(t) > 0).count();

    println!("\n--- totals over {carriers} files ---");
    println!(
        "symbol dictionaries      {:>6}   in {:>3} files",
        total.symbol_dictionaries(),
        files_with(Tally::symbol_dictionaries)
    );
    println!(
        "text regions             {:>6}   in {:>3} files",
        total.text_regions(),
        files_with(Tally::text_regions)
    );
    println!(
        "refinement regions       {:>6}   in {:>3} files",
        total.refinement_regions(),
        files_with(Tally::refinement_regions)
    );
    println!(
        "halftone / pattern       {:>6}   in {:>3} files",
        total.halftone(),
        files_with(Tally::halftone)
    );
    println!(
        "custom tables (type 53)  {:>6}   in {:>3} files",
        total.count(kind::TABLES),
        files_with(|t| t.count(kind::TABLES))
    );
    println!();
    println!(
        "SDHUFF   (Huffman symbol dict) {:>6}   in {:>3} files",
        total.sdhuff,
        files_with(|t| t.sdhuff)
    );
    println!(
        "SBHUFF   (Huffman text region) {:>6}   in {:>3} files",
        total.sbhuff,
        files_with(|t| t.sbhuff)
    );
    println!(
        "  at refinement template 1     {:>6}   in {:>3} files",
        total.sdrtemplate1 + total.sbrtemplate1,
        files_with(|t| t.sdrtemplate1 + t.sbrtemplate1)
    );
    println!(
        "refinement over Huffman        {:>6}   in {:>3} files",
        total.huffman_refinement,
        files_with(|t| t.huffman_refinement)
    );
    for (name, row) in ["SBHUFFRDW", "SBHUFFRDH", "SBHUFFRDX", "SBHUFFRDY"]
        .iter()
        .zip(total.refine_selectors)
    {
        println!(
            "  {name:<10} B.14 {:>3}  B.15 {:>3}  reserved {:>3}  custom {:>3}",
            row[0], row[1], row[2], row[3]
        );
    }
    println!(
        "  SBHUFFRSIZE  B.1 {:>3}  custom {:>3}",
        total.rsize_selector[0], total.rsize_selector[1]
    );
    println!(
        "refinement regions, template 0 {:>6}   template 1 {:>6}",
        total.grtemplate[0], total.grtemplate[1]
    );
    println!(
        "  of those, TPGRON set         {:>6}   in {:>3} files",
        total.tpgron,
        files_with(|t| t.tpgron)
    );
    println!(
        "SDREFAGG (refine/aggregate)    {:>6}   in {:>3} files",
        total.sdrefagg,
        files_with(|t| t.sdrefagg)
    );
    println!(
        "SBREFINE (text refinement)     {:>6}   in {:>3} files",
        total.sbrefine,
        files_with(|t| t.sbrefine)
    );
    println!();
    println!("REFCORNER bottom-left  {:>6}", total.corners[0]);
    println!("REFCORNER top-left     {:>6}", total.corners[1]);
    println!("REFCORNER bottom-right {:>6}", total.corners[2]);
    println!("REFCORNER top-right    {:>6}", total.corners[3]);
    println!(
        "TRANSPOSED             {:>6}   in {:>3} files",
        total.transposed,
        files_with(|t| t.transposed)
    );
    println!(
        "SBDSOFFSET non-zero    {:>6}   in {:>3} files",
        total.dsoffset,
        files_with(|t| t.dsoffset)
    );
    println!(
        "more than one strip    {:>6}   in {:>3} files",
        total.striped,
        files_with(|t| t.striped)
    );
    println!(
        "context used                   {:>6}   in {:>3} files",
        total.context_used,
        files_with(|t| t.context_used)
    );
    println!(
        "context retained               {:>6}   in {:>3} files",
        total.context_retained,
        files_with(|t| t.context_retained)
    );
    println!(
        "unparsable headers             {:>6}   in {:>3} files",
        total.unparsable,
        files_with(|t| t.unparsable)
    );

    println!();
    println!("--- what a bound has to clear (ruling 1's yardsticks) ---");
    println!("largest SDNUMNEWSYMS      {:>10}", total.max_new_symbols);
    println!(
        "largest SDNUMEXSYMS       {:>10}",
        total.max_exported_symbols
    );
    println!("largest SBNUMINSTANCES    {:>10}", total.max_instances);
    // **The fourth yardstick, taken at last — and the three above are not it.**
    //
    // `MAX_JBIG2_SYMBOL_PIXELS` is a *pixel* budget and those three figures are
    // counts, of symbols and of instances, so none of them bounds the pixels.
    // `Tally::max_symbol_pixels` sat declared, merged by `add` and **assigned by
    // nothing** from the day this file was written until 26 September 2026: the
    // walk above shares no code with the decoder on purpose, and neither of a
    // symbol's dimensions is in any segment header — 6.5.5 accumulates both
    // inside the coder. Printed for the first time on 23 September it read 0
    // over 117 files, which reads as "no real document comes close" and meant
    // "nobody looked", so it was printed as `not measured` and asserted still
    // unassigned, to tell whoever populated it to take the measurement too.
    //
    // [`measure`] is that measurement. It is the decoder's own tally rather than
    // a header walk, because nothing else can reach a symbol's size, and the
    // figures below are what `docs/verification.md`'s jbig2 row was waiting on.
    println!(
        "largest dictionary pixels {:>10}   <- MAX_JBIG2_SYMBOL_PIXELS's own yardstick",
        total.max_symbol_pixels
    );
    println!(
        "largest single symbol     {:>10}   {} x {}",
        total.max_one_symbol_pixels, total.max_one_symbol.0, total.max_one_symbol.1
    );
    println!(
        "widest symbol             {:>10}   tallest {}",
        total.widest_symbol, total.tallest_symbol
    );
    println!(
        "symbols decoded           {:>10}   over {} dictionaries in {} images",
        total.symbols_decoded, total.dictionaries_decoded, total.images_measured
    );
    println!(
        "images whose decode refused{:>9}   (their figures above are a floor)",
        total.images_refused
    );
    println!();
    println!("--- the largest symbol relative to its page ---");
    println!(
        "smallest whole multiple of the page that admits every symbol: {}",
        total.worst_multiple
    );
    println!(
        "the tightest fit, unrounded: {} of the page it is drawn onto",
        permille_of(total.tightest_permille)
    );
    // **The measurement is load-bearing, so it is asserted and not only
    // printed.** Two directions, and the census is worthless without both.
    //
    // A census that measured nothing reads exactly like a census that found
    // nothing — the `RAN` / `SKIPPED` discipline one level down — so the first
    // assertion is that symbols were decoded at all. The second is the figure
    // `MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` was chosen from: every symbol in the
    // corpus fits inside the bound, with the margin its ledger publishes. If
    // this fires, either the corpus grew a document the bound refuses — in
    // which case the bound is wrong and `jbig2_attribution.rs`'s pinned count
    // will have moved too — or the bound was narrowed without re-measuring.
    assert!(
        total.symbols_decoded > 0 && total.images_measured > 0,
        "the measurement decoded no symbols at all, so every figure above is a \
         zero that means `nobody looked` — which is the exact failure this half \
         of the census was added to end"
    );
    assert!(
        total.worst_multiple <= MAX_JBIG2_SYMBOL_PAGE_MULTIPLE,
        "a corpus symbol spans its page {}x and the cap admits {}: narrow the \
         bound rather than re-pinning this, and expect \
         `jbig2_attribution.rs`'s refused count to have moved with it",
        total.worst_multiple,
        MAX_JBIG2_SYMBOL_PAGE_MULTIPLE,
    );
    let mut ratios: Vec<Fit<'_>> = per_file
        .iter()
        .filter(|(_, t)| t.worst_multiple > 0)
        .map(|(name, t)| Fit {
            permille: t.tightest_permille,
            multiple: t.worst_multiple,
            name: name.as_str(),
            symbol: t.tightest_symbol,
            page: t.tightest_page,
        })
        .collect();
    // By the unrounded fit rather than by the whole multiple, because every
    // file in the corpus shares the same whole multiple and the ordering would
    // otherwise be the file names.
    ratios.sort_by(|a, b| b.permille.cmp(&a.permille).then(a.name.cmp(b.name)));
    println!("the five tightest files:");
    for fit in ratios.iter().take(5) {
        println!(
            "  {:>7} ({}x)  widest {:>6} tallest {:>6} against a page of {} x {}   {}",
            permille_of(fit.permille),
            fit.multiple,
            fit.symbol.0,
            fit.symbol.1,
            fit.page.0,
            fit.page.1,
            fit.name,
        );
    }
    let mut worst: Vec<(u32, &str)> = per_file
        .iter()
        .map(|(name, t)| (t.max_instances, name.as_str()))
        .collect();
    worst.sort_unstable();
    worst.reverse();
    println!("the five heaviest text regions, by instance count:");
    for (count, name) in worst.iter().take(5) {
        println!("  {count:>9}  {name}");
    }
    let mut sym: Vec<(u32, &str)> = per_file
        .iter()
        .map(|(name, t)| (t.max_new_symbols, name.as_str()))
        .collect();
    sym.sort_unstable();
    sym.reverse();
    println!("the five largest dictionaries, by new-symbol count:");
    for (count, name) in sym.iter().take(5) {
        println!("  {count:>9}  {name}");
    }

    // The scheduling question milestone 1 exists to answer is not how many
    // segments use a feature but how many *files* a stage would unlock, and
    // that is a question about overlap: a file needing both Huffman and
    // refinement is unlocked by neither alone.
    let needs_huff = |t: &Tally| t.sdhuff > 0 || t.sbhuff > 0;
    let needs_refine = |t: &Tally| t.sdrefagg > 0 || t.sbrefine > 0;
    let lineage = |t: &Tally| t.symbol_dictionaries() > 0 || t.text_regions() > 0;

    let carrying: Vec<&Tally> = per_file
        .iter()
        .map(|(_, t)| t)
        .filter(|t| lineage(t))
        .collect();
    let plain = carrying
        .iter()
        .filter(|t| !needs_huff(t) && !needs_refine(t))
        .count();
    let huff_only = carrying
        .iter()
        .filter(|t| needs_huff(t) && !needs_refine(t))
        .count();
    let refine_only = carrying
        .iter()
        .filter(|t| !needs_huff(t) && needs_refine(t))
        .count();
    let both = carrying
        .iter()
        .filter(|t| needs_huff(t) && needs_refine(t))
        .count();
    let halftone_too = carrying.iter().filter(|t| t.halftone() > 0).count();
    let halftone_only = per_file
        .iter()
        .filter(|(_, t)| !lineage(t) && t.halftone() > 0)
        .count();

    println!();
    println!(
        "--- what each stage unlocks, of the {} files in this lineage ---",
        carrying.len()
    );
    println!("arithmetic alone (milestones 3-4)   {plain:>3}");
    println!(
        "+ Huffman     (milestone 5)         {huff_only:>3}   cumulative {:>3}",
        plain + huff_only
    );
    println!(
        "+ refinement  (milestone 6)         {refine_only:>3}   cumulative {:>3}",
        plain + refine_only
    );
    println!("needs both                          {both:>3}");
    println!("also carry halftone, still degrade  {halftone_too:>3}");
    println!("halftone or pattern only            {halftone_only:>3}");
    println!();

    println!("\nsegment types seen, by T.88 Table 34 number:");
    for (kind, count) in &total.segments {
        println!("  type {kind:>3}  {count:>6}");
    }
}
