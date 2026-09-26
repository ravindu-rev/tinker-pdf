//! WOFF 1.0 and WOFF 2.0: the two web font containers, unpacked to sfnt.
//!
//! Feature documentation: `docs/features/fonts.md`.
//!
//! Both are a wrapper around a font this crate already reads, so both belong
//! here: undoing them is table-directory surgery, which is what [`crate::subset`]
//! already does in the other direction. Neither carries its own decompressor —
//! WOFF 1.0 deflates each table and WOFF 2.0 Brotli-compresses the lot, and
//! both reach down the `font → filters` edge that the CMap tables already use.
//!
//! # The two are not variations on each other
//!
//! **WOFF 1.0** ([W3C REC, 2012]) is a repackaging and nothing more. Each
//! table is zlib-compressed on its own, the directory records the original
//! length and the original checksum, and reconstruction is: inflate each
//! table, rebuild the sfnt directory, pad to four bytes. It is **lossless by
//! construction** for any font whose tables were in tag order to begin with,
//! which lets the test assert byte identity rather than equivalence.
//!
//! **WOFF 2.0** ([W3C REC, 2018]) is a re-encoding. One Brotli stream carries
//! every table concatenated; `glyf` is taken apart into seven substreams and
//! its contours re-encoded as triplets; `loca` is not stored at all but fallen
//! out of the `glyf` reconstruction; and `hmtx` may have its left side
//! bearings deleted on the grounds that they equal the glyph bounding boxes.
//! §5 says in as many words that the result "may produce binary results that
//! are different from the original data", so byte identity is **not** the
//! property to test for and the test does not claim it. See
//! [`docs/features/fonts.md`] for what is claimed instead.
//!
//! # Ruling 8, and what this module refuses to know
//!
//! Bytes in, bytes out. [`decode`] takes a slice and a byte budget and returns
//! an sfnt or a typed reason it could not; it has no idea what an EPUB is, and
//! the facade's `@font-face` handling is what turns a [`WoffError`] into a
//! warning naming a family.
//!
//! [W3C REC, 2012]: https://www.w3.org/TR/WOFF/
//! [W3C REC, 2018]: https://www.w3.org/TR/WOFF2/
//! [`docs/features/fonts.md`]: https://github.com/ravindu-rev/tinker-pdf/blob/main/docs/features/fonts.md

use tinker_pdf_filters::{brotli_decode, flate_decode, Limits};

/// `wOFF`, WOFF 1.0's signature (§3).
const WOFF1_SIGNATURE: u32 = 0x774F_4646;
/// `wOF2`, WOFF 2.0's signature (§3.2).
const WOFF2_SIGNATURE: u32 = 0x774F_4632;
/// `ttcf`, which as a WOFF2 `flavor` means a collection follows (§4.2).
const COLLECTION_FLAVOR: u32 = 0x7474_6366;

const TAG_GLYF: u32 = 0x676C_7966;
const TAG_LOCA: u32 = 0x6C6F_6361;
const TAG_HMTX: u32 = 0x686D_7478;
const TAG_HEAD: u32 = 0x6865_6164;
const TAG_HHEA: u32 = 0x6868_6561;
const TAG_MAXP: u32 = 0x6D61_7870;

/// A ceiling on the table count, so a directory cannot ask for gigabytes of
/// `Vec` before a single byte has been checked.
///
/// The sfnt directory's own `numTables` is a `u16`, so 65 535 is the format's
/// ceiling; this is a working one. No real face is near it — the largest CJK
/// faces in the corpus carry about thirty tables.
const MAX_TABLES: usize = 4096;

/// Which container a byte slice announces itself to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Packaging {
    /// WOFF 1.0: per-table zlib around an otherwise ordinary sfnt.
    Woff,
    /// WOFF 2.0: one Brotli stream, with `glyf`, `loca` and `hmtx` transformed.
    Woff2,
}

impl Packaging {
    /// The keyword `css-fonts-4` §4.3's `format()` uses for this container,
    /// which is also what a warning prints.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Packaging::Woff => "woff",
            Packaging::Woff2 => "woff2",
        }
    }
}

/// Whether these bytes are a packed web font, by signature.
///
/// The bytes decide, not a `format()` hint: §4.3 of `css-fonts-4` makes the
/// hint advisory, and a sheet that omits it is commoner than one that lies.
#[must_use]
pub fn packaging(bytes: &[u8]) -> Option<Packaging> {
    match be32(bytes, 0)? {
        WOFF1_SIGNATURE => Some(Packaging::Woff),
        WOFF2_SIGNATURE => Some(Packaging::Woff2),
        _ => None,
    }
}

/// Why a WOFF did not become a font.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WoffError {
    /// The signature is neither `wOFF` nor `wOF2`.
    NotAWoff,
    /// A length or offset reached past the end of the file.
    Truncated,
    /// A rule one of the two specifications states was broken. The string
    /// names the rule.
    Malformed(&'static str),
    /// A table's `origChecksum` disagrees with the table that came out
    /// (WOFF 1.0 §5).
    ///
    /// Separate from [`WoffError::Malformed`] because it is the one failure
    /// that means *the container is intact and the font inside it is not*,
    /// which is a different conversation with whoever produced the file.
    TableChecksum { tag: u32 },
    /// A table would not decompress, or decompressed to the wrong length.
    Unpackable { tag: u32 },
    /// The transform on a table is one this build has no reverse for
    /// (WOFF 2.0 §4.1: "If a decoder encounters a table entry that specifies
    /// an unknown transformation version number the entire font MUST be
    /// rejected").
    UnknownTransform { tag: u32, version: u8 },
    /// The reconstructed font would be larger than the caller allowed.
    ExceedsOutputLimit { limit: usize },
}

impl core::fmt::Display for WoffError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WoffError::NotAWoff => write!(f, "not a WOFF container"),
            WoffError::Truncated => write!(f, "the WOFF ended early"),
            WoffError::Malformed(why) => write!(f, "malformed WOFF: {why}"),
            WoffError::TableChecksum { tag } => {
                write!(f, "the {} table's checksum disagrees", tag_name(*tag))
            }
            WoffError::Unpackable { tag } => {
                write!(f, "the {} table would not unpack", tag_name(*tag))
            }
            WoffError::UnknownTransform { tag, version } => write!(
                f,
                "the {} table uses transform version {version}, which is not read here",
                tag_name(*tag)
            ),
            WoffError::ExceedsOutputLimit { limit } => {
                write!(f, "the font unpacks to more than {limit} bytes")
            }
        }
    }
}

/// A table tag as the four characters it is, for a message.
fn tag_name(tag: u32) -> String {
    tag.to_be_bytes()
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}

/// Unpacks a WOFF 1.0 or WOFF 2.0 container into the sfnt inside it.
///
/// `max_output` bounds the reconstructed font. It is not advisory: a WOFF2
/// table directory states lengths in `UIntBase128`, which reaches 2^32 - 1 in
/// five bytes, so a forty-byte file can ask for four gigabytes.
///
/// # Errors
///
/// [`WoffError`] — the bytes are not a WOFF, the container is damaged, a table
/// will not unpack, a transform is unknown, or the result would exceed
/// `max_output`.
pub fn decode(bytes: &[u8], max_output: usize) -> Result<Vec<u8>, WoffError> {
    match packaging(bytes) {
        Some(Packaging::Woff) => decode_woff1(bytes, max_output),
        Some(Packaging::Woff2) => decode_woff2(bytes, max_output),
        None => Err(WoffError::NotAWoff),
    }
}

// ---- shared sfnt assembly ---------------------------------------------------

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// An sfnt table checksum: the sum of its big-endian 32-bit words, wrapping,
/// with the final partial word zero-padded.
///
/// The padding is the part worth stating: the sum is over the table *as
/// stored*, which the sfnt specification requires to be a multiple of four
/// bytes long, so a table whose real length is not is summed as though the
/// missing bytes were zero.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(4);
    for chunk in &mut chunks {
        sum = sum.wrapping_add(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    let tail = chunks.remainder();
    if !tail.is_empty() {
        let mut word = [0u8; 4];
        word[..tail.len()].copy_from_slice(tail);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// A table's checksum **as an sfnt table directory states it**, which is not
/// always [`checksum`] of the bytes.
///
/// `head` is the exception, and it is the only one. Its `checkSumAdjustment`
/// field is a function of the whole font including this very sum, so the sfnt
/// specification breaks the circle by defining `head`'s directory entry to be
/// computed with those four bytes taken as zero. Both containers inherit that
/// definition without restating it: WOFF 1.0 §5's `origChecksum` is "the
/// checksum for this table in the original font", and the original font's
/// directory held the zeroed value.
///
/// Getting this wrong is not subtle in its effect and is very subtle in its
/// cause — every table validates, `head` does not, and the file is perfectly
/// good. Both WOFF 1.0 producers used to build this crate's fixtures write the
/// zeroed value, because both copied it out of a real sfnt directory.
fn directory_checksum(tag: u32, data: &[u8]) -> u32 {
    if tag != TAG_HEAD || data.len() < 12 {
        return checksum(data);
    }
    let mut zeroed = data.to_vec();
    zeroed[8..12].fill(0);
    checksum(&zeroed)
}

/// One reconstructed table, before it is placed.
struct Table {
    tag: u32,
    data: Vec<u8>,
    /// Where this table sat in the container's own physical ordering, which
    /// is the only record of where it sat in the font the container was made
    /// from.
    order: usize,
}

/// Builds an sfnt from `flavor` and a set of tables.
///
/// Two orderings, and they are deliberately different. The **directory** is
/// written in ascending tag order, which WOFF 1.0 §5 requires of a decoder in
/// as many words ("User agents MUST likewise assure that the sfnt table
/// directory is recreated in ascending tag value order"). The **table data**
/// is written in the container's own physical order, because that is what the
/// original font's physical order was and reproducing it is what makes the
/// WOFF 1.0 round trip byte-identical rather than merely equivalent.
fn assemble(flavor: u32, mut tables: Vec<Table>, max_output: usize) -> Result<Vec<u8>, WoffError> {
    let count = tables.len();
    let count16 = u16::try_from(count)
        .map_err(|_| WoffError::Malformed("more tables than an sfnt directory can hold"))?;

    // 12-byte offset table, 16 bytes per record, each table padded to four.
    let mut total = 12usize + count * 16;
    for table in &tables {
        let padded = table
            .data
            .len()
            .checked_add(3)
            .map(|n| n & !3)
            .ok_or(WoffError::Malformed("a table length that cannot be padded"))?;
        total = total
            .checked_add(padded)
            .ok_or(WoffError::Malformed("a font larger than the address space"))?;
        if total > max_output {
            return Err(WoffError::ExceedsOutputLimit { limit: max_output });
        }
    }

    let mut out = vec![0u8; 12 + count * 16];
    out[0..4].copy_from_slice(&flavor.to_be_bytes());
    out[4..6].copy_from_slice(&count16.to_be_bytes());
    // searchRange, entrySelector and rangeShift are not stored in either
    // container and MUST be recomputed (WOFF 1.0 §4). They are a function of
    // the table count alone.
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= count as u32 {
        entry_selector += 1;
    }
    let search_range = 16u16.wrapping_mul(1 << entry_selector);
    out[6..8].copy_from_slice(&search_range.to_be_bytes());
    out[8..10].copy_from_slice(&entry_selector.to_be_bytes());
    out[10..12].copy_from_slice(
        &count16
            .wrapping_mul(16)
            .wrapping_sub(search_range)
            .to_be_bytes(),
    );

    // Place the data in the container's physical order, recording where each
    // table landed so the directory can point at it.
    tables.sort_by_key(|t| t.order);
    let mut placed: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(count);
    for table in &tables {
        let offset = u32::try_from(out.len())
            .map_err(|_| WoffError::Malformed("a table offset past four gigabytes"))?;
        let length = u32::try_from(table.data.len())
            .map_err(|_| WoffError::Malformed("a table longer than four gigabytes"))?;
        placed.push((
            table.tag,
            directory_checksum(table.tag, &table.data),
            offset,
            length,
        ));
        out.extend_from_slice(&table.data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    // The directory goes in ascending tag order, whatever order the data is in.
    placed.sort_by_key(|&(tag, _, _, _)| tag);
    for (i, (tag, sum, offset, length)) in placed.iter().enumerate() {
        let at = 12 + i * 16;
        out[at..at + 4].copy_from_slice(&tag.to_be_bytes());
        out[at + 4..at + 8].copy_from_slice(&sum.to_be_bytes());
        out[at + 8..at + 12].copy_from_slice(&offset.to_be_bytes());
        out[at + 12..at + 16].copy_from_slice(&length.to_be_bytes());
    }
    Ok(out)
}

// ---- WOFF 1.0 ---------------------------------------------------------------

/// WOFF 1.0's fixed header length (§3): signature through `privLength`.
const WOFF1_HEADER: usize = 44;
/// One WOFF 1.0 table directory entry (§5).
const WOFF1_ENTRY: usize = 20;

fn decode_woff1(bytes: &[u8], max_output: usize) -> Result<Vec<u8>, WoffError> {
    let flavor = be32(bytes, 4).ok_or(WoffError::Truncated)?;
    let count = usize::from(be16(bytes, 12).ok_or(WoffError::Truncated)?);
    // §3: "The header includes a reserved field; this MUST be set to zero. If
    // this field is non-zero, a conforming user agent MUST reject the file."
    if be16(bytes, 14).ok_or(WoffError::Truncated)? != 0 {
        return Err(WoffError::Malformed(
            "the reserved header field is not zero",
        ));
    }
    let total_sfnt_size = be32(bytes, 16).ok_or(WoffError::Truncated)?;
    if count == 0 {
        return Err(WoffError::Malformed("a WOFF with no tables"));
    }
    if count > MAX_TABLES {
        return Err(WoffError::Malformed(
            "more tables than a font plausibly has",
        ));
    }

    let mut tables = Vec::with_capacity(count);
    let mut expected_size = 12usize + count * 16;
    for i in 0..count {
        let at = WOFF1_HEADER + i * WOFF1_ENTRY;
        let tag = be32(bytes, at).ok_or(WoffError::Truncated)?;
        let offset = be32(bytes, at + 4).ok_or(WoffError::Truncated)? as usize;
        let comp_length = be32(bytes, at + 8).ok_or(WoffError::Truncated)? as usize;
        let orig_length = be32(bytes, at + 12).ok_or(WoffError::Truncated)? as usize;
        let orig_checksum = be32(bytes, at + 16).ok_or(WoffError::Truncated)?;

        // §5: "WOFF files containing table directory entries for which
        // compLength is greater than origLength are considered invalid and
        // MUST NOT be loaded by user agents."
        if comp_length > orig_length {
            return Err(WoffError::Malformed(
                "a compressed table larger than the table it came from",
            ));
        }
        let end = offset
            .checked_add(comp_length)
            .ok_or(WoffError::Truncated)?;
        let packed = bytes.get(offset..end).ok_or(WoffError::Truncated)?;

        expected_size = expected_size
            .checked_add((orig_length + 3) & !3)
            .ok_or(WoffError::Malformed("a font larger than the address space"))?;
        if expected_size > max_output {
            return Err(WoffError::ExceedsOutputLimit { limit: max_output });
        }

        // §5: equal lengths mean the table was stored rather than compressed,
        // which producers do for tables compression would not help.
        let data = if comp_length == orig_length {
            packed.to_vec()
        } else {
            // §4: "If compressed, it MUST have been compressed by the
            // compress2() function of zlib" — a zlib wrapper, not raw
            // DEFLATE.
            let limits = Limits::new(orig_length);
            let decoded =
                flate_decode(packed, &limits, None).map_err(|_| WoffError::Unpackable { tag })?;
            // §5: "Files containing compressed font tables that decompress to
            // a size other than origLength are also considered invalid."
            if !decoded.complete || decoded.data.len() != orig_length {
                return Err(WoffError::Unpackable { tag });
            }
            decoded.data
        };

        // §5 puts checksum validation on the producer ("Tools producing WOFF
        // files MUST validate these checksums"), which leaves a decoder free
        // to skip it. This build does not skip it: `origChecksum` is the only
        // end-to-end integrity check either container carries, the cost is one
        // pass over bytes already in cache, and a face that fails it is one
        // this engine would rather decline by name than embed.
        if directory_checksum(tag, &data) != orig_checksum {
            return Err(WoffError::TableChecksum { tag });
        }

        tables.push(Table {
            tag,
            data,
            // The container's **physical** order, which is `offset` and not
            // `i`. §5 requires the WOFF directory be sorted by tag, so `i` is
            // alphabetical and says nothing about the font this was made
            // from; the compressed blocks, on the other hand, are laid down in
            // the order the original font had them. Reproducing that is the
            // difference between a font that is byte-for-byte the one the
            // producer packed and one that merely draws the same.
            order: offset,
        });
    }

    // §4: "If this value is incorrect, a conforming user agent MUST reject the
    // file as invalid." It is also the one field that cross-checks the whole
    // directory at once, which is why it is worth honouring rather than
    // treating as a hint.
    if expected_size != total_sfnt_size as usize {
        return Err(WoffError::Malformed(
            "totalSfntSize disagrees with the table directory",
        ));
    }

    // Metadata and private data are skipped outright. §6 and §7 make both
    // advisory, neither is part of the sfnt, and the metadata block is
    // XML — reading it would give this crate an opinion about markup that
    // ruling 8 says it may not have.
    assemble(flavor, tables, max_output)
}

// ---- WOFF 2.0 ---------------------------------------------------------------

/// WOFF 2.0's fixed header length (§3.2): signature through `privLength`.
const WOFF2_HEADER: usize = 48;

/// §4.1's known table tags, indexed by the low six bits of the flags byte.
///
/// Index 63 is not a tag: it means "a four-byte tag follows". Note `cvt `,
/// `OS/2` and `SVG ` — §4.1 ends with the reminder that a tag shorter than
/// four characters is padded with trailing spaces, and a table that dropped
/// the space would be a tag no reader looks for.
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// A cursor over a decompressed WOFF2 stream.
///
/// §4.1 says the format's "best strategy for decoding is to process the file
/// as a stream, rather than trying to access it randomly", and the variable
/// widths of `255UInt16` and `UIntBase128` are why: nothing in the directory
/// can be found without reading everything before it.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, at: 0 }
    }

    fn u8(&mut self) -> Result<u8, WoffError> {
        let byte = *self.data.get(self.at).ok_or(WoffError::Truncated)?;
        self.at += 1;
        Ok(byte)
    }

    fn u16(&mut self) -> Result<u16, WoffError> {
        let value = be16(self.data, self.at).ok_or(WoffError::Truncated)?;
        self.at += 2;
        Ok(value)
    }

    fn i16(&mut self) -> Result<i16, WoffError> {
        self.u16().map(|v| v as i16)
    }

    fn u32(&mut self) -> Result<u32, WoffError> {
        let value = be32(self.data, self.at).ok_or(WoffError::Truncated)?;
        self.at += 4;
        Ok(value)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], WoffError> {
        let end = self.at.checked_add(n).ok_or(WoffError::Truncated)?;
        let out = self.data.get(self.at..end).ok_or(WoffError::Truncated)?;
        self.at = end;
        Ok(out)
    }

    fn is_empty(&self) -> bool {
        self.at >= self.data.len()
    }

    /// §3.1's `255UInt16`: one to three bytes. The encoding is deliberately
    /// not unique — "the value 506 can be encoded as [255, 253], [254, 0], and
    /// [253, 1, 250] ... a decoder MUST accept them all."
    fn u255(&mut self) -> Result<u16, WoffError> {
        const WORD: u8 = 253;
        const ONE_MORE_A: u8 = 255;
        const ONE_MORE_B: u8 = 254;
        match self.u8()? {
            WORD => self.u16(),
            ONE_MORE_A => Ok(u16::from(self.u8()?) + u16::from(WORD)),
            ONE_MORE_B => Ok(u16::from(self.u8()?) + u16::from(WORD) * 2),
            other => Ok(u16::from(other)),
        }
    }

    /// §3.1's `UIntBase128`, with both refusals the specification demands: a
    /// value padded with a leading zero, and a sequence past five bytes.
    ///
    /// Both matter for a reason beyond tidiness. This is the field that states
    /// a table's length, so an encoding that admits two spellings of the same
    /// number admits a file two readers disagree about the size of.
    fn base128(&mut self) -> Result<u32, WoffError> {
        let mut accumulator = 0u32;
        for i in 0..5 {
            let byte = self.u8()?;
            if i == 0 && byte == 0x80 {
                return Err(WoffError::Malformed(
                    "a UIntBase128 value padded with a leading zero",
                ));
            }
            if accumulator & 0xFE00_0000 != 0 {
                return Err(WoffError::Malformed("a UIntBase128 value past 2^32 - 1"));
            }
            accumulator = (accumulator << 7) | u32::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Ok(accumulator);
            }
        }
        Err(WoffError::Malformed(
            "a UIntBase128 sequence past five bytes",
        ))
    }
}

/// One entry of §4.1's table directory, after the variable-width fields have
/// been read out of it.
struct Woff2Entry {
    tag: u32,
    orig_length: u32,
    /// How many bytes this entry occupies in the decompressed block. Equal to
    /// `orig_length` for an untransformed table, and to the entry's
    /// `transformLength` otherwise — which is **zero** for a transformed
    /// `loca`, because §5.3 makes it a placeholder that consumes no stream.
    span: usize,
    transformed: bool,
}

/// Whether this `(tag, version)` pair names a transform, a null transform, or
/// something this build has no reverse for (§4.1).
fn transform_kind(tag: u32, version: u8) -> Result<bool, WoffError> {
    let transformed = match (tag, version) {
        // §4.1: "For 'glyf' and 'loca' tables, transformation version 3
        // indicates the null transform"; version 0 is §5.1's transform.
        (TAG_GLYF | TAG_LOCA, 0) => true,
        (TAG_GLYF | TAG_LOCA, 3) => false,
        // §5.4: hmtx version 0 is the null transform and version 1 is the one
        // that deletes the side bearings. The sense is inverted against glyf,
        // which is exactly the kind of detail worth a citation.
        (TAG_HMTX, 0) => false,
        (TAG_HMTX, 1) => true,
        // Every other table: version 0 and nothing else.
        (_, 0) => false,
        _ => return Err(WoffError::UnknownTransform { tag, version }),
    };
    Ok(transformed)
}

fn decode_woff2(bytes: &[u8], max_output: usize) -> Result<Vec<u8>, WoffError> {
    let flavor = be32(bytes, 4).ok_or(WoffError::Truncated)?;
    let count = usize::from(be16(bytes, 12).ok_or(WoffError::Truncated)?);
    // §3.2 is explicit that the reserved field is *not* grounds for refusal
    // here, unlike WOFF 1.0's: "a decoder MUST NOT reject a downloaded font
    // file if the reserved header value is not zero." So it is read and
    // ignored on purpose.
    let _reserved = be16(bytes, 14).ok_or(WoffError::Truncated)?;
    let compressed_size = be32(bytes, 20).ok_or(WoffError::Truncated)? as usize;
    if count == 0 {
        return Err(WoffError::Malformed("a WOFF2 with no tables"));
    }
    if count > MAX_TABLES {
        return Err(WoffError::Malformed(
            "more tables than a font plausibly has",
        ));
    }

    let mut directory = Reader::new(bytes.get(WOFF2_HEADER..).ok_or(WoffError::Truncated)?);
    let mut entries = Vec::with_capacity(count);
    let mut block_size = 0usize;
    for _ in 0..count {
        let flags = directory.u8()?;
        let known = usize::from(flags & 0x3f);
        let tag = if known == 63 {
            directory.u32()?
        } else {
            let name = KNOWN_TAGS
                .get(known)
                .ok_or(WoffError::Malformed("a known-tag index past the table"))?;
            u32::from_be_bytes(**name)
        };
        let version = flags >> 6;
        let transformed = transform_kind(tag, version)?;
        let orig_length = directory.base128()?;
        // §4.1: "The transformLength field is present in the table directory
        // entry if, and only if, the table has been processed by a non-null
        // transform."
        let span = if transformed {
            directory.base128()? as usize
        } else {
            orig_length as usize
        };
        // §5.3: "The transformLength of the transformed loca table MUST always
        // be zero." It is a placeholder; the real table falls out of glyf.
        if transformed && tag == TAG_LOCA && span != 0 {
            return Err(WoffError::Malformed(
                "a transformed loca that claims a length of its own",
            ));
        }
        block_size = block_size
            .checked_add(span)
            .ok_or(WoffError::Malformed("a table directory larger than memory"))?;
        entries.push(Woff2Entry {
            tag,
            orig_length,
            span,
            transformed,
        });
    }

    // §4.2: the collection directory follows the table directory, and only
    // when the flavor says the input was a collection.
    let collection = if flavor == COLLECTION_FLAVOR {
        Some(read_collection_directory(&mut directory, count)?)
    } else {
        None
    };

    let data_at = WOFF2_HEADER
        .checked_add(directory.at)
        .ok_or(WoffError::Truncated)?;
    let data_end = data_at
        .checked_add(compressed_size)
        .ok_or(WoffError::Truncated)?;
    let compressed = bytes.get(data_at..data_end).ok_or(WoffError::Truncated)?;

    // §5: "If the decompression of the data block fails for any reason, the
    // WOFF2 file is invalid and MUST NOT be loaded." The ceiling is the
    // caller's, because `block_size` is attacker-stated.
    if block_size > max_output {
        return Err(WoffError::ExceedsOutputLimit { limit: max_output });
    }
    let block = brotli_decode(compressed, &Limits::new(block_size.max(1)))
        .map_err(|_| WoffError::Malformed("the compressed font data would not decompress"))?;
    // §5: "The sum of the origLength ... and transformLength ... MUST equal
    // the size of the font data block after it has been decompressed."
    if block.len() != block_size {
        return Err(WoffError::Malformed(
            "the decompressed block is not the size the directory declared",
        ));
    }

    reconstruct(flavor, &entries, &block, collection.as_ref(), max_output)
}

/// §4.2's collection directory: which table directory entries each nested font
/// is built from.
struct Collection {
    /// The TTC header version of the input, which the output may downgrade.
    version: u32,
    /// One `(flavor, indices into the table directory)` per nested font.
    fonts: Vec<(u32, Vec<usize>)>,
}

fn read_collection_directory(r: &mut Reader<'_>, tables: usize) -> Result<Collection, WoffError> {
    let version = r.u32()?;
    let num_fonts = usize::from(r.u255()?);
    if num_fonts == 0 {
        return Err(WoffError::Malformed("a collection with no fonts"));
    }
    let mut fonts = Vec::with_capacity(num_fonts.min(MAX_TABLES));
    for _ in 0..num_fonts {
        let num_tables = usize::from(r.u255()?);
        if num_tables == 0 || num_tables > tables {
            return Err(WoffError::Malformed(
                "a nested font naming more tables than the directory has",
            ));
        }
        let flavor = r.u32()?;
        let mut indices = Vec::with_capacity(num_tables);
        for _ in 0..num_tables {
            let index = usize::from(r.u255()?);
            if index >= tables {
                return Err(WoffError::Malformed(
                    "a nested font naming a table directory entry that is not there",
                ));
            }
            indices.push(index);
        }
        fonts.push((flavor, indices));
    }
    Ok(Collection { version, fonts })
}

/// Slices the decompressed block into tables, reversing the three transforms.
fn reconstruct(
    flavor: u32,
    entries: &[Woff2Entry],
    block: &[u8],
    collection: Option<&Collection>,
    max_output: usize,
) -> Result<Vec<u8>, WoffError> {
    // Pass one: everything except the transformed hmtx, which needs the glyf
    // that pass one produces. §5.5 pins loca immediately after glyf in the
    // directory, so the pair is always in hand together.
    let mut tables: Vec<Option<Table>> = Vec::with_capacity(entries.len());
    let mut at = 0usize;
    let mut deferred_hmtx: Vec<usize> = Vec::new();
    let mut pending_loca: Option<(usize, Vec<u8>)> = None;

    for (index, entry) in entries.iter().enumerate() {
        let end = at
            .checked_add(entry.span)
            .ok_or(WoffError::Malformed("a table span past the block"))?;
        let slice = block
            .get(at..end)
            .ok_or(WoffError::Malformed("a table span past the block"))?;
        at = end;

        let data = match (entry.tag, entry.transformed) {
            (TAG_GLYF, true) => {
                let (glyf, loca) = reverse_glyf(slice, max_output)?;
                pending_loca = Some((index, loca));
                Some(glyf)
            }
            (TAG_LOCA, true) => {
                // The placeholder. Its bytes came out of the glyf reverse, and
                // §5.3 gives the length it must have.
                let (_, loca) = pending_loca.take().ok_or(WoffError::Malformed(
                    "a transformed loca with no glyf before it",
                ))?;
                if loca.len() != entry.orig_length as usize {
                    return Err(WoffError::Malformed(
                        "a reconstructed loca is not the length the directory declared",
                    ));
                }
                Some(loca)
            }
            (TAG_HMTX, true) => {
                deferred_hmtx.push(index);
                None
            }
            _ => Some(slice.to_vec()),
        };
        tables.push(data.map(|data| Table {
            tag: entry.tag,
            data,
            order: index,
        }));
    }

    for index in deferred_hmtx {
        let entry = entries.get(index).ok_or(WoffError::Malformed(
            "a deferred hmtx that is not in the directory",
        ))?;
        let start: usize = entries.iter().take(index).map(|e| e.span).sum();
        let slice = block
            .get(start..start + entry.span)
            .ok_or(WoffError::Malformed("an hmtx span past the block"))?;
        let hmtx = reverse_hmtx(slice, &tables)?;
        if let Some(slot) = tables.get_mut(index) {
            *slot = Some(Table {
                tag: TAG_HMTX,
                data: hmtx,
                order: index,
            });
        }
    }

    let mut built: Vec<Table> = Vec::with_capacity(tables.len());
    for table in tables {
        built.push(table.ok_or(WoffError::Malformed("a table that never got reconstructed"))?);
    }

    // §5: "the decoder MUST recalculate the checkSumAdjustment value of the
    // entire font". The field is zeroed *before* assembly, because the head
    // table's own directory checksum is defined to be computed with it zero,
    // and then written back once the whole-font sum is known.
    for table in &mut built {
        if table.tag == TAG_HEAD && table.data.len() >= 12 {
            table.data[8..12].fill(0);
        }
    }

    let mut font = match collection {
        None => assemble(flavor, built, max_output)?,
        Some(collection) => assemble_collection(collection, built, max_output)?,
    };
    set_checksum_adjustment(&mut font);
    Ok(font)
}

/// Writes the whole-font checksum into `head`, for every `head` in the font.
///
/// The value is `0xB1B0AFBA` minus the sum of the entire file with the field
/// itself zeroed, which is the sfnt definition. A collection has one `head`
/// per nested font, and they share table data, so this walks the directories
/// it just wrote rather than assuming there is one.
fn set_checksum_adjustment(font: &mut [u8]) {
    let sum = checksum(font);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(sum);
    for at in head_offsets(font) {
        if let Some(slot) = font.get_mut(at + 8..at + 12) {
            slot.copy_from_slice(&adjustment.to_be_bytes());
        }
    }
}

/// Every `head` table's offset in an assembled font or collection.
fn head_offsets(font: &[u8]) -> Vec<usize> {
    let mut directories = Vec::new();
    if be32(font, 0) == Some(COLLECTION_FLAVOR) {
        let count = be32(font, 8).unwrap_or(0).min(MAX_TABLES as u32);
        for i in 0..count as usize {
            if let Some(offset) = be32(font, 12 + i * 4) {
                directories.push(offset as usize);
            }
        }
    } else {
        directories.push(0);
    }

    let mut out = Vec::new();
    for base in directories {
        let Some(count) = be16(font, base + 4) else {
            continue;
        };
        for i in 0..usize::from(count).min(MAX_TABLES) {
            let at = base + 12 + i * 16;
            if be32(font, at) == Some(TAG_HEAD) {
                if let Some(offset) = be32(font, at + 8) {
                    out.push(offset as usize);
                }
            }
        }
    }
    out
}

/// Builds a TrueType Collection from the shared tables and §4.2's index lists.
fn assemble_collection(
    collection: &Collection,
    mut tables: Vec<Table>,
    max_output: usize,
) -> Result<Vec<u8>, WoffError> {
    tables.sort_by_key(|t| t.order);
    let fonts = &collection.fonts;

    // §4.2 permits converting a version 2.0 TTC header to version 1, which is
    // what this does rather than writing null DSIG fields — the signature the
    // fields point at cannot survive a re-encoding in any case, and §5 says as
    // much when it requires an encoder to drop DSIG outright.
    let header = 12 + fonts.len() * 4;
    let mut offset_tables = Vec::with_capacity(fonts.len());
    let mut at = header;
    for (_, indices) in fonts {
        offset_tables.push(at);
        at = at
            .checked_add(12 + indices.len() * 16)
            .ok_or(WoffError::Malformed("a collection larger than memory"))?;
        if at > max_output {
            return Err(WoffError::ExceedsOutputLimit { limit: max_output });
        }
    }

    // Place the shared table data once, in the container's own order.
    let mut body = Vec::new();
    let mut placed: Vec<(u32, u32, u32)> = Vec::with_capacity(tables.len());
    for table in &tables {
        while (at + body.len()) % 4 != 0 {
            body.push(0);
        }
        let offset = u32::try_from(at + body.len())
            .map_err(|_| WoffError::Malformed("a table offset past four gigabytes"))?;
        let length = u32::try_from(table.data.len())
            .map_err(|_| WoffError::Malformed("a table longer than four gigabytes"))?;
        if at + body.len() + table.data.len() > max_output {
            return Err(WoffError::ExceedsOutputLimit { limit: max_output });
        }
        placed.push((checksum(&table.data), offset, length));
        body.extend_from_slice(&table.data);
    }

    let mut out = Vec::with_capacity(at + body.len());
    out.extend_from_slice(&COLLECTION_FLAVOR.to_be_bytes());
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(
        &u32::try_from(fonts.len())
            .map_err(|_| WoffError::Malformed("more nested fonts than a TTC can hold"))?
            .to_be_bytes(),
    );
    for offset in &offset_tables {
        out.extend_from_slice(
            &u32::try_from(*offset)
                .map_err(|_| WoffError::Malformed("a nested font past four gigabytes"))?
                .to_be_bytes(),
        );
    }

    for (flavor, indices) in fonts {
        let count = u16::try_from(indices.len())
            .map_err(|_| WoffError::Malformed("more tables than a directory can hold"))?;
        let mut entry_selector = 0u16;
        while (1u32 << (entry_selector + 1)) <= indices.len() as u32 {
            entry_selector += 1;
        }
        let search_range = 16u16.wrapping_mul(1 << entry_selector);
        out.extend_from_slice(&flavor.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(
            &count
                .wrapping_mul(16)
                .wrapping_sub(search_range)
                .to_be_bytes(),
        );

        // The nested directory goes in ascending tag order, like any sfnt's.
        let mut rows: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(indices.len());
        for &index in indices {
            let table = tables.get(index).ok_or(WoffError::Malformed(
                "a nested font naming a table that is not there",
            ))?;
            let (sum, offset, length) = *placed
                .get(index)
                .ok_or(WoffError::Malformed("a table that was never placed"))?;
            rows.push((table.tag, sum, offset, length));
        }
        rows.sort_by_key(|&(tag, _, _, _)| tag);
        for (tag, sum, offset, length) in rows {
            out.extend_from_slice(&tag.to_be_bytes());
            out.extend_from_slice(&sum.to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&length.to_be_bytes());
        }
    }
    let _ = collection.version;
    out.extend_from_slice(&body);
    Ok(out)
}

// ---- §5.1, the glyf transform -----------------------------------------------

/// §5.2's triplet encoding, as the five arrays a decoder indexes by flag.
///
/// Transcribed from the specification's own 128-row table rather than derived
/// from its pattern, because the pattern is not stated anywhere and a
/// re-derivation would be a second guess at what the rows mean. The
/// consistency the table *does* state — that the byte count is one flag byte
/// plus the coordinate bits — is asserted in a test.
const TRIPLET_X_BITS: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 16, 16,
    16, 16,
];

const TRIPLET_Y_BITS: [u8; 128] = [
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 16, 16,
    16, 16,
];

const TRIPLET_DELTA_X: [u16; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 256, 256, 512, 512, 768, 768, 1024, 1024, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17,
    33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 49, 49, 49, 49, 49, 49, 49, 49,
    49, 49, 49, 49, 49, 49, 49, 49, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 257, 257, 257, 257, 257,
    257, 257, 257, 257, 257, 257, 257, 513, 513, 513, 513, 513, 513, 513, 513, 513, 513, 513, 513,
    0, 0, 0, 0, 0, 0, 0, 0,
];

const TRIPLET_DELTA_Y: [u16; 128] = [
    0, 0, 256, 256, 512, 512, 768, 768, 1024, 1024, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 17,
    17, 17, 17, 33, 33, 33, 33, 49, 49, 49, 49, 1, 1, 1, 1, 17, 17, 17, 17, 33, 33, 33, 33, 49, 49,
    49, 49, 1, 1, 1, 1, 17, 17, 17, 17, 33, 33, 33, 33, 49, 49, 49, 49, 1, 1, 1, 1, 17, 17, 17, 17,
    33, 33, 33, 33, 49, 49, 49, 49, 1, 1, 1, 1, 257, 257, 257, 257, 513, 513, 513, 513, 1, 1, 1, 1,
    257, 257, 257, 257, 513, 513, 513, 513, 1, 1, 1, 1, 257, 257, 257, 257, 513, 513, 513, 513, 0,
    0, 0, 0, 0, 0, 0, 0,
];

/// `false` is negative, `true` positive. §5.2's "X sign" and "Y sign" columns,
/// with the `N/A` rows (where the coordinate has no bits) left as `false`.
const TRIPLET_X_POSITIVE: [bool; 128] = [
    false, false, false, false, false, false, false, false, false, false, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true, false, true, false, true, false, true, false, true, false, true, false, true,
    false, true,
];

const TRIPLET_Y_POSITIVE: [bool; 128] = [
    false, true, false, true, false, true, false, true, false, true, false, false, false, false,
    false, false, false, false, false, false, false, false, true, true, false, false, true, true,
    false, false, true, true, false, false, true, true, false, false, true, true, false, false,
    true, true, false, false, true, true, false, false, true, true, false, false, true, true,
    false, false, true, true, false, false, true, true, false, false, true, true, false, false,
    true, true, false, false, true, true, false, false, true, true, false, false, true, true,
    false, false, true, true, false, false, true, true, false, false, true, true, false, false,
    true, true, false, false, true, true, false, false, true, true, false, false, true, true,
    false, false, true, true, false, false, true, true, false, false, true, true, false, false,
    true, true,
];

/// The most points one glyph may carry, from the sfnt's own `endPtsOfContours`
/// being 16 bits wide. A cap rather than a guess: a transformed glyph states
/// its point counts as `255UInt16` values that a decoder would otherwise
/// accumulate into an allocation before reading a single coordinate.
const MAX_POINTS: usize = 0x1_0000;

/// Reverses §5.1's `glyf` transform, returning the reconstructed `glyf` and
/// the `loca` that falls out of it.
#[allow(clippy::too_many_lines)]
fn reverse_glyf(data: &[u8], max_output: usize) -> Result<(Vec<u8>, Vec<u8>), WoffError> {
    let mut head = Reader::new(data);
    let _reserved = head.u16()?;
    let option_flags = head.u16()?;
    let num_glyphs = usize::from(head.u16()?);
    let index_format = head.u16()?;
    let n_contour_size = head.u32()? as usize;
    let n_points_size = head.u32()? as usize;
    let flag_size = head.u32()? as usize;
    let glyph_size = head.u32()? as usize;
    let composite_size = head.u32()? as usize;
    let bbox_size = head.u32()? as usize;
    let instruction_size = head.u32()? as usize;

    let mut n_contours = Reader::new(head.bytes(n_contour_size)?);
    let mut n_points = Reader::new(head.bytes(n_points_size)?);
    let mut flags = Reader::new(head.bytes(flag_size)?);
    let mut glyphs = Reader::new(head.bytes(glyph_size)?);
    let mut composites = Reader::new(head.bytes(composite_size)?);
    let bbox_block = head.bytes(bbox_size)?;
    let mut instructions = Reader::new(head.bytes(instruction_size)?);

    // §5.1: "The total number of bytes in bboxBitmap is equal to
    // 4 * floor((numGlyphs + 31) / 32)", which is the numGlyphs-long bit array
    // rounded up to a whole number of 32-bit words.
    let bitmap_len = 4 * num_glyphs.div_ceil(32);
    let bbox_bitmap = bbox_block
        .get(..bitmap_len)
        .ok_or(WoffError::Malformed("a bbox bitmap shorter than numGlyphs"))?;
    let mut bboxes = Reader::new(
        bbox_block
            .get(bitmap_len..)
            .ok_or(WoffError::Malformed("a bbox stream past its block"))?,
    );

    // §5.1: bit 0 of optionFlags adds a numGlyphs-long bit array carrying the
    // OVERLAP_SIMPLE flag of each simple glyph. Without it "the decoder MUST
    // set all OVERLAP_SIMPLE flag values to zero" — so an absent array is a
    // statement, not a gap.
    let overlap = if option_flags & 1 == 1 {
        Some(head.bytes(num_glyphs.div_ceil(8))?)
    } else {
        None
    };

    let has_bbox = |glyph: usize| -> bool {
        bbox_bitmap
            .get(glyph / 8)
            .is_some_and(|byte| byte & (0x80 >> (glyph % 8)) != 0)
    };
    let overlaps = |glyph: usize| -> bool {
        overlap.is_some_and(|bits| {
            bits.get(glyph / 8)
                .is_some_and(|byte| byte & (0x80 >> (glyph % 8)) != 0)
        })
    };

    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(num_glyphs + 1);
    offsets.push(0);

    for glyph in 0..num_glyphs {
        let contours = n_contours.i16()?;
        let start = glyf.len();

        if contours == 0 {
            // §5.1: "Reconstruction of an empty glyph ... loca[n] = loca[n-1]."
            if has_bbox(glyph) {
                return Err(WoffError::Malformed(
                    "an empty glyph with an explicit bounding box",
                ));
            }
        } else if contours > 0 {
            let contour_count = usize::from(contours as u16);
            let mut ends: Vec<u16> = Vec::with_capacity(contour_count.min(MAX_POINTS));
            let mut total = 0usize;
            for _ in 0..contour_count {
                total = total
                    .checked_add(usize::from(n_points.u255()?))
                    .ok_or(WoffError::Malformed("a contour point count that overflows"))?;
                if total == 0 || total > MAX_POINTS {
                    return Err(WoffError::Malformed(
                        "a glyph with no points, or more than a glyph may have",
                    ));
                }
                // §5.1: "Convert this into the endPtsOfContours[] array by
                // computing the cumulative sum, then subtracting one."
                ends.push(u16::try_from(total - 1).unwrap_or(u16::MAX));
            }

            let mut xs: Vec<i32> = Vec::with_capacity(total);
            let mut ys: Vec<i32> = Vec::with_capacity(total);
            let mut on_curve: Vec<bool> = Vec::with_capacity(total);
            let (mut x, mut y) = (0i32, 0i32);
            for _ in 0..total {
                let flag = flags.u8()?;
                // §5.2: "if the most significant bit is 0, then the point is
                // on-curve"; the low seven bits index the triplet table.
                on_curve.push(flag & 0x80 == 0);
                let index = usize::from(flag & 0x7f);
                let (dx, dy) = read_triplet(&mut glyphs, index)?;
                x = x.saturating_add(dx);
                y = y.saturating_add(dy);
                xs.push(x);
                ys.push(y);
            }

            let instruction_length = usize::from(glyphs.u255()?);
            let code = instructions.bytes(instruction_length)?;

            let box_of = if has_bbox(glyph) {
                [bboxes.i16()?, bboxes.i16()?, bboxes.i16()?, bboxes.i16()?]
            } else {
                // §5.1: "if the corresponding bit in the bounding box bit
                // vector is not set, then derive the bounding box by computing
                // the minimum and maximum x and y coordinates in the outline"
                // — over *all* points, on- and off-curve alike.
                bounding_box(&xs, &ys)
            };

            write_simple_glyph(
                &mut glyf,
                contours,
                box_of,
                &ends,
                &xs,
                &ys,
                &on_curve,
                code,
                overlaps(glyph),
            );
        } else {
            if contours != -1 {
                return Err(WoffError::Malformed(
                    "a glyph whose contour count is neither simple, empty nor composite",
                ));
            }
            // §5.1: "A composite glyph MUST have an explicitly supplied
            // bounding box", because computing one would mean resolving
            // component references and would defeat a streaming decoder.
            if !has_bbox(glyph) {
                return Err(WoffError::Malformed(
                    "a composite glyph with no bounding box",
                ));
            }
            let box_of = [bboxes.i16()?, bboxes.i16()?, bboxes.i16()?, bboxes.i16()?];
            let (components, wants_instructions) = read_composite(&mut composites)?;
            glyf.extend_from_slice(&(-1i16).to_be_bytes());
            for value in box_of {
                glyf.extend_from_slice(&value.to_be_bytes());
            }
            glyf.extend_from_slice(components);
            if wants_instructions {
                let instruction_length = usize::from(glyphs.u255()?);
                let code = instructions.bytes(instruction_length)?;
                glyf.extend_from_slice(
                    &u16::try_from(code.len()).unwrap_or(u16::MAX).to_be_bytes(),
                );
                glyf.extend_from_slice(code);
            }
        }

        // Glyph records are padded so that a short `loca` — which stores every
        // offset halved — can address them at all.
        if glyf.len() > start {
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        if glyf.len() > max_output {
            return Err(WoffError::ExceedsOutputLimit { limit: max_output });
        }
        offsets.push(
            u32::try_from(glyf.len())
                .map_err(|_| WoffError::Malformed("a glyf table past four gigabytes"))?,
        );
    }

    // §5.3: the offsets are stored "using a format that is indicated by the
    // indexFormat field of the Transformed glyf Table".
    let mut loca = Vec::with_capacity((num_glyphs + 1) * 4);
    for offset in &offsets {
        if index_format == 0 {
            let halved = u16::try_from(offset / 2).map_err(|_| {
                WoffError::Malformed("a glyf too large for the short loca the font declared")
            })?;
            loca.extend_from_slice(&halved.to_be_bytes());
        } else {
            loca.extend_from_slice(&offset.to_be_bytes());
        }
    }
    Ok((glyf, loca))
}

/// §5.2: reads one point's coordinate bytes and returns its delta.
fn read_triplet(r: &mut Reader<'_>, index: usize) -> Result<(i32, i32), WoffError> {
    let x_bits = u32::from(
        *TRIPLET_X_BITS
            .get(index)
            .ok_or(WoffError::Malformed("a triplet index past the table"))?,
    );
    let y_bits = u32::from(TRIPLET_Y_BITS[index]);
    let bytes = r.bytes(((x_bits + y_bits) / 8) as usize)?;

    // "When X and Y coordinate values are recorded using nibbles ... the bits
    // are packed in the byte stream with most significant bit of X coordinate
    // first, followed by the value for Y coordinate."
    let mut packed = 0u64;
    for &byte in bytes {
        packed = (packed << 8) | u64::from(byte);
    }
    let x_raw = (packed >> y_bits) as i64;
    // `y_bits` is 0, 4, 8, 12 or 16 straight out of the table, so the shift is
    // always in range and the mask is never negative.
    let y_raw = (packed & ((1u64 << y_bits) - 1)) as i64;
    let x = i32::try_from(i64::from(TRIPLET_DELTA_X[index]) + x_raw).unwrap_or(i32::MAX);
    let y = i32::try_from(i64::from(TRIPLET_DELTA_Y[index]) + y_raw).unwrap_or(i32::MAX);
    Ok((
        if TRIPLET_X_POSITIVE[index] { x } else { -x },
        if TRIPLET_Y_POSITIVE[index] { y } else { -y },
    ))
}

/// The bounding box of every point of an outline, on- and off-curve alike.
fn bounding_box(xs: &[i32], ys: &[i32]) -> [i16; 4] {
    let clamp = |v: i32| i16::try_from(v).unwrap_or(if v < 0 { i16::MIN } else { i16::MAX });
    let x_min = xs.iter().copied().min().unwrap_or(0);
    let y_min = ys.iter().copied().min().unwrap_or(0);
    let x_max = xs.iter().copied().max().unwrap_or(0);
    let y_max = ys.iter().copied().max().unwrap_or(0);
    [clamp(x_min), clamp(y_min), clamp(x_max), clamp(y_max)]
}

/// Writes one simple glyph in the ordinary sfnt encoding (OFF §5.3.3).
///
/// The coordinates are written in whichever of the three widths fits, but the
/// **repeat** flag is never used. That is a size choice and not a fidelity
/// one: §5.1 says the reconstruction need not be a binary match, only an
/// equivalent outline, and a decoder that also had to decide when repeating
/// pays would be a compressor.
#[allow(clippy::too_many_arguments)]
fn write_simple_glyph(
    out: &mut Vec<u8>,
    contours: i16,
    bbox: [i16; 4],
    ends: &[u16],
    xs: &[i32],
    ys: &[i32],
    on_curve: &[bool],
    instructions: &[u8],
    overlap: bool,
) {
    out.extend_from_slice(&contours.to_be_bytes());
    for value in bbox {
        out.extend_from_slice(&value.to_be_bytes());
    }
    for end in ends {
        out.extend_from_slice(&end.to_be_bytes());
    }
    out.extend_from_slice(
        &u16::try_from(instructions.len())
            .unwrap_or(u16::MAX)
            .to_be_bytes(),
    );
    out.extend_from_slice(instructions);

    let mut flags = Vec::with_capacity(xs.len());
    let mut x_bytes = Vec::new();
    let mut y_bytes = Vec::new();
    let (mut last_x, mut last_y) = (0i32, 0i32);
    for i in 0..xs.len() {
        let mut flag = 0u8;
        if on_curve.get(i).copied().unwrap_or(false) {
            flag |= 0x01; // ON_CURVE_POINT
        }
        // §5.1: OVERLAP_SIMPLE goes on "the first flag byte for each simple
        // glyph of the output font", and nowhere else.
        if overlap && i == 0 {
            flag |= 0x40;
        }
        let dx = xs[i] - last_x;
        let dy = ys[i] - last_y;
        last_x = xs[i];
        last_y = ys[i];

        if dx == 0 {
            flag |= 0x10; // X_IS_SAME: no bytes at all
        } else if (-255..=255).contains(&dx) {
            flag |= 0x02; // X_SHORT_VECTOR
            if dx > 0 {
                flag |= 0x10; // ...and positive
            }
            x_bytes.push(dx.unsigned_abs() as u8);
        } else {
            x_bytes.extend_from_slice(
                &(dx.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16).to_be_bytes(),
            );
        }

        if dy == 0 {
            flag |= 0x20; // Y_IS_SAME
        } else if (-255..=255).contains(&dy) {
            flag |= 0x04; // Y_SHORT_VECTOR
            if dy > 0 {
                flag |= 0x20;
            }
            y_bytes.push(dy.unsigned_abs() as u8);
        } else {
            y_bytes.extend_from_slice(
                &(dy.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16).to_be_bytes(),
            );
        }
        flags.push(flag);
    }
    out.extend_from_slice(&flags);
    out.extend_from_slice(&x_bytes);
    out.extend_from_slice(&y_bytes);
}

/// §5.1's composite stream: component records, passed through untouched.
///
/// The components are not re-encoded, only measured — a composite glyph's
/// bytes in the transformed stream are already exactly the bytes the sfnt
/// wants, which is why §5.1 gives the composite case its own three steps
/// rather than a coordinate encoding.
fn read_composite<'a>(r: &mut Reader<'a>) -> Result<(&'a [u8], bool), WoffError> {
    const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
    const WE_HAVE_A_SCALE: u16 = 0x0008;
    const MORE_COMPONENTS: u16 = 0x0020;
    const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
    const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
    const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;

    let start = r.at;
    let mut wants_instructions = false;
    let mut components = 0u32;
    loop {
        let flags = r.u16()?;
        let _glyph_index = r.u16()?;
        wants_instructions |= flags & WE_HAVE_INSTRUCTIONS != 0;

        let argument_bytes = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
        let transform_bytes = if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            8
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            4
        } else if flags & WE_HAVE_A_SCALE != 0 {
            2
        } else {
            0
        };
        let _ = r.bytes(argument_bytes + transform_bytes)?;

        components += 1;
        // The same bound `glyf.rs` puts on drawing a composite, applied to
        // reading one: a component list is attacker-controlled and its only
        // natural terminator is a flag bit.
        if components > 256 {
            return Err(WoffError::Malformed(
                "a composite glyph with more components than one may have",
            ));
        }
        if flags & MORE_COMPONENTS == 0 {
            break;
        }
    }
    let bytes = r
        .data
        .get(start..r.at)
        .ok_or(WoffError::Malformed("a composite record past its stream"))?;
    Ok((bytes, wants_instructions))
}

// ---- §5.4, the hmtx transform -----------------------------------------------

/// Reverses §5.4's `hmtx` transform: puts back the left side bearings the
/// encoder deleted because they equalled the glyph bounding boxes' `xMin`.
fn reverse_hmtx(data: &[u8], tables: &[Option<Table>]) -> Result<Vec<u8>, WoffError> {
    let find = |tag: u32| {
        tables
            .iter()
            .flatten()
            .find(|t| t.tag == tag)
            .map(|t| t.data.as_slice())
    };
    let hhea = find(TAG_HHEA).ok_or(WoffError::Malformed("a transformed hmtx with no hhea"))?;
    let maxp = find(TAG_MAXP).ok_or(WoffError::Malformed("a transformed hmtx with no maxp"))?;
    let glyf = find(TAG_GLYF).ok_or(WoffError::Malformed("a transformed hmtx with no glyf"))?;
    let loca = find(TAG_LOCA).ok_or(WoffError::Malformed("a transformed hmtx with no loca"))?;
    let head = find(TAG_HEAD).ok_or(WoffError::Malformed("a transformed hmtx with no head"))?;

    let num_h_metrics = usize::from(be16(hhea, 34).ok_or(WoffError::Truncated)?);
    let num_glyphs = usize::from(be16(maxp, 4).ok_or(WoffError::Truncated)?);
    let long_loca = be16(head, 50).ok_or(WoffError::Truncated)? != 0;
    if num_h_metrics == 0 || num_h_metrics > num_glyphs {
        return Err(WoffError::Malformed(
            "numberOfHMetrics disagrees with numGlyphs",
        ));
    }

    let mut r = Reader::new(data);
    let flags = r.u8()?;
    // §5.4: "When hmtx transform is indicated by the table directory, the
    // Flags (bits 0 or 1 or both) MUST be set. Bits 2-7 are reserved and MUST
    // be zero."
    if flags & 0x03 == 0 || flags & 0xfc != 0 {
        return Err(WoffError::Malformed("an hmtx transform with invalid flags"));
    }

    let mut advances = Vec::with_capacity(num_h_metrics);
    for _ in 0..num_h_metrics {
        advances.push(r.u16()?);
    }

    // The bearing this glyph's outline implies: xMin of its bounding box, and
    // zero for a glyph with no outline. §5.4 states the empty case separately
    // because an empty glyph has no xMin to take.
    let x_min_of = |glyph: usize| -> Result<i16, WoffError> {
        let (start, end) = if long_loca {
            (
                be32(loca, glyph * 4).ok_or(WoffError::Truncated)? as usize,
                be32(loca, glyph * 4 + 4).ok_or(WoffError::Truncated)? as usize,
            )
        } else {
            (
                usize::from(be16(loca, glyph * 2).ok_or(WoffError::Truncated)?) * 2,
                usize::from(be16(loca, glyph * 2 + 2).ok_or(WoffError::Truncated)?) * 2,
            )
        };
        if end <= start {
            return Ok(0);
        }
        Ok(be16(glyf, start + 2).ok_or(WoffError::Truncated)? as i16)
    };

    let mut lsbs = Vec::with_capacity(num_h_metrics);
    for glyph in 0..num_h_metrics {
        lsbs.push(if flags & 0x01 == 0 {
            r.i16()?
        } else {
            x_min_of(glyph)?
        });
    }
    let mut bearings = Vec::with_capacity(num_glyphs - num_h_metrics);
    for glyph in num_h_metrics..num_glyphs {
        bearings.push(if flags & 0x02 == 0 {
            r.i16()?
        } else {
            x_min_of(glyph)?
        });
    }
    // §5: "There MUST NOT be any extraneous data between the table entries in
    // the decompressed data stream."
    if !r.is_empty() {
        return Err(WoffError::Malformed(
            "a transformed hmtx with bytes left over",
        ));
    }

    let mut out = Vec::with_capacity(num_h_metrics * 4 + bearings.len() * 2);
    for (advance, lsb) in advances.iter().zip(&lsbs) {
        out.extend_from_slice(&advance.to_be_bytes());
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    for bearing in &bearings {
        out.extend_from_slice(&bearing.to_be_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
