//! 7z: the signature header, the property-id header grammar, packed streams,
//! folders and coders.
//!
//! A `.cb7` is a 7z of page images. This module is the container; [`crate::lzma`]
//! is the compression it almost always carries.
//!
//! # The shape of the format, and the one thing about it that surprises
//!
//! A 7z has no per-file data. It has **folders** — a folder is a chain of
//! coders that turns one or more packed byte ranges into one output stream —
//! and then a list of **substreams** that says where each file lives inside a
//! folder's output. A "solid" archive is one folder holding every file, which
//! is why 7z compresses so well and why this module's `read` cannot hand back a
//! borrow: there is no range of the *input* that is any one file.
//!
//! That is the whole argument for `tinker-pdf-archive` being its own crate and
//! for there being no trait over its three modules; it is written out in the
//! crate header and in `xtask`'s eleventh DAG amendment, and this is the module
//! it is a fact about. [`Archive::read`] returns owned bytes, takes `&mut self`,
//! and caches the last folder it decoded — so reading five pages out of one
//! solid block decompresses once rather than five times.
//!
//! The second surprise is that **the header itself is usually compressed**.
//! `kEncodedHeader` is a complete `StreamsInfo` describing one packed stream
//! whose contents are the real header, so a reader must be able to decompress
//! before it can list. 7-Zip writes the header with plain LZMA and the file
//! data with LZMA2, which is why [`crate::lzma`] implements both.
//!
//! # What adjudicates this decoder
//!
//! **The archive's own CRC-32**, recorded per substream in `kCRC`, checked in
//! [`Archive::read`] before any bytes are handed over. A wrong window, a
//! mis-set probability array or an ignored LZMA2 dictionary reset fails the
//! *format's* check rather than needing a second implementation to disagree
//! with, which is what ruling 13 asks for. `docs/design/comic-archives.md`
//! carries the argument in full.
//!
//! # Coders read
//!
//! Copy (`00`), LZMA (`030101`), LZMA2 (`21`), Deflate (`040108`), bzip2
//! (`040202`, in [`crate::bzip2`]), PPMd (`030401`, in [`crate::ppmd`]), BCJ
//! (`03030103`, the x86 branch filter `-mf=BCJ` puts in front of LZMA2) and
//! BCJ2 (`0303011B`, the four-stream x86 converter), the last two in the
//! crate's private `bcj` and `bcj2` modules.
//!
//! **A folder is a tree of coders, walked from its output down its bind
//! pairs**, and never by the order its coders are listed in. Each coder's
//! inputs are pack streams or other coders' outputs; a chain is the case
//! where each has one, and BCJ2 is the case where one coder has four — its
//! main, call and jump streams each out of an LZMA coder, its range-coded
//! decisions straight from a pack stream. The listing order is the writers'
//! choice and they disagree: 7-Zip lists BCJ before the LZMA2 feeding it and
//! py7zr after, and 7-Zip 26 lists BCJ2 *last* of four though its command
//! line numbers it first. The graph is checked at open to be a tree this
//! build can walk.
//!
//! # Refused by name
//!
//! **Encrypted archives** (coder `06F10701`, AES-256 + SHA-256):
//! [`Error::Encrypted`], a named non-goal shared with `tinker-pdf-zip`.
//! **Coders this build does not implement** are [`Error::UnsupportedCoder`],
//! by method id. **Folders whose graph has no answer** — a coder with two
//! outputs, a stream fed twice or by nothing, a cycle, a known coder declaring
//! a stream count that is not its own — are [`Error::NotAChain`], the name
//! the variant has always had.

use tinker_pdf_filters::{crc32, inflate_raw, Limits as InflateLimits};

use crate::{bcj, bcj2, bzip2, lzma, ppmd};

pub mod limits;

use limits::{MAX_7Z_CODERS, MAX_7Z_ENTRIES, MAX_7Z_FOLDERS, MAX_7Z_NAME_LEN, MAX_7Z_UNPACKED};

/// `7z\xBC\xAF\x27\x1C`, at offset zero and nowhere else.
const SIGNATURE: &[u8] = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];
/// Signature, version, start-header CRC and the start header itself.
const SIGNATURE_HEADER: usize = 32;

// The property ids, from the format's own `7zFormat.txt` grammar. Named rather
// than inlined because the header parser is a `match` over them and a bare
// `0x0E` in that position is unreadable.
const K_END: u8 = 0x00;
const K_HEADER: u8 = 0x01;
const K_ARCHIVE_PROPERTIES: u8 = 0x02;
const K_ADDITIONAL_STREAMS: u8 = 0x03;
const K_MAIN_STREAMS: u8 = 0x04;
const K_FILES_INFO: u8 = 0x05;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SUBSTREAMS_INFO: u8 = 0x08;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0A;
const K_FOLDER: u8 = 0x0B;
const K_CODERS_UNPACK_SIZE: u8 = 0x0C;
const K_NUM_UNPACK_STREAM: u8 = 0x0D;
const K_EMPTY_STREAM: u8 = 0x0E;
const K_EMPTY_FILE: u8 = 0x0F;
const K_ANTI: u8 = 0x10;
const K_NAME: u8 = 0x11;
const K_ENCODED_HEADER: u8 = 0x17;
const K_DUMMY: u8 = 0x19;

/// 7z method `03030103`, BCJ: the x86 branch converter (`DOC/Methods.txt`:
/// `03` branch, `03` x86, `01` version, `03` BCJ).
const BCJ_X86: &[u8] = &[0x03, 0x03, 0x01, 0x03];
/// 7z method `040202`, bzip2 (`DOC/Methods.txt`: `04` misc, `02` BZip2,
/// `02` BZip2).
const BZIP2: &[u8] = &[0x04, 0x02, 0x02];
/// 7z method `030401`, PPMd (`DOC/Methods.txt`: `03` 7z, `04` PPMD, `01`
/// PPMd var.H with 7z's range coder).
const PPMD: &[u8] = &[0x03, 0x04, 0x01];
/// 7z method `0303011B`, BCJ2 (`DOC/Methods.txt`: `03` branch, `03` x86,
/// `01` version, `1B` BCJ2 — "4 packed streams").
const BCJ2: &[u8] = &[0x03, 0x03, 0x01, 0x1B];

/// Why an archive could not be opened at all.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// No `7z\xBC\xAF\x27\x1C` at offset zero, or a file too short to hold a
    /// signature header.
    NotA7z,
    /// The start header's own CRC-32 does not match, so the offset and length
    /// of the real header cannot be trusted to point anywhere.
    ///
    /// The one place this format checks itself before reading anything, and it
    /// is checked: those two numbers are a `u64` each and are the only
    /// file-derived values here that become a slice bound on the whole file.
    StartHeaderCorrupt,
    /// The header is past [`Limits::max_unpacked`], or points outside the file.
    HeaderOutOfRange,
    /// The header decompressed but is not the grammar — a property id out of
    /// place, a count that does not match, a `NUMBER` that runs off the end.
    BadHeader,
    /// The header's own CRC-32 does not match.
    HeaderCrcMismatch,
    /// Past [`Limits::max_entries`], [`Limits::max_folders`] or
    /// [`Limits::max_coders`].
    TooManyEntries,
    /// A coder this build does not implement, by its 7z method id.
    ///
    /// Carries the id so the refusal names the method rather than the file:
    /// `03` is the delta filter and `0A` ARM64's branch filter, and a host
    /// that says which is a host whose user can re-pack.
    UnsupportedCoder { id: Vec<u8> },
    /// AES-256 (`06F10701`). A named non-goal, shared with `tinker-pdf-zip`.
    Encrypted,
    /// A folder whose coder graph is not a tree this build can walk: a coder
    /// with more than one output, an in-stream fed twice or by nothing, a
    /// bind pair naming a stream that does not exist, a cycle, or a coder this
    /// build reads declaring stream counts that are not its own (BCJ2 has four
    /// inputs, every other coder one).
    ///
    /// The name is older than the rule. It once refused every folder that was
    /// not a chain, which was every BCJ2 folder; since BCJ2 is read, it names
    /// the graphs that have no answer rather than the ones that have four.
    NotAChain,
    /// The header decompressed to something, and it was not a header.
    HeaderNotDecodable,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotA7z => f.write_str("not a 7z archive"),
            Error::StartHeaderCorrupt => f.write_str("a start header that does not checksum"),
            Error::HeaderOutOfRange => f.write_str("a header outside the file or past the cap"),
            Error::BadHeader => f.write_str("a header that is not the 7z grammar"),
            Error::HeaderCrcMismatch => f.write_str("a header that does not checksum"),
            Error::TooManyEntries => f.write_str("more entries than this build reads"),
            Error::UnsupportedCoder { id } => {
                write!(f, "a 7z coder this build does not read: {}", hex(id))
            }
            Error::Encrypted => f.write_str("an encrypted 7z, which is a named non-goal"),
            Error::NotAChain => f.write_str("a folder whose coders do not form a tree"),
            Error::HeaderNotDecodable => f.write_str("a compressed header that would not decode"),
        }
    }
}

impl std::error::Error for Error {}

/// Why one entry's bytes were not handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntryError {
    /// No entry with that index. A caller's bug rather than a file's.
    NoSuchEntry,
    /// A directory, or an "anti-file" (a deletion marker in an incremental
    /// archive). Neither holds bytes.
    NotAFile,
    /// The folder this entry lives in would not decompress.
    ///
    /// **Every file in a solid block shares one of these.** That is the format
    /// rather than this reader: one corrupt block is every file in it, and
    /// saying so per entry is what turns it into one placeholder page each
    /// (ruling 2) rather than a refused archive.
    FolderFailed(lzma::Error),
    /// The folder's bzip2 stream would not decode — the same sentence as
    /// [`EntryError::FolderFailed`] for coder `040202`, carrying bzip2's own
    /// reason, which includes its block and stream CRCs failing.
    Bzip2Failed(bzip2::Error),
    /// The folder's PPMd stream would not decode, carrying the model's
    /// reason — properties outside 7-Zip's ranges, an arena past the cap, or
    /// a stream the model cannot follow.
    PpmdFailed(ppmd::Error),
    /// The folder's BCJ2 decision stream does not start as a range-coded
    /// stream: its first byte is not zero, or its initial code is `FFFFFFFF`.
    /// (One of BCJ2's streams running out before its output is
    /// [`EntryError::Truncated`].)
    Bcj2Failed,
    /// The folder decompressed and this entry's CRC-32 does not match what the
    /// archive recorded.
    ///
    /// **This is the check that adjudicates the decompressor.** It is not
    /// tolerated: bytes that fail it are a picture with the wrong pixels in it,
    /// where a refusal is a placeholder page that says so.
    CrcMismatch,
    /// The folder decompressed to fewer bytes than the substream table says
    /// this entry occupies.
    Truncated,
    /// The archive's coder is one this build does not implement, discovered at
    /// read rather than at open because another folder was readable.
    UnsupportedCoder,
    /// The folder's output is past [`Limits::max_unpacked`].
    TooLarge,
}

impl core::fmt::Display for EntryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EntryError::NoSuchEntry => f.write_str("no entry with that index"),
            EntryError::NotAFile => f.write_str("an entry that holds no file data"),
            EntryError::FolderFailed(e) => write!(f, "a block that would not decompress: {e}"),
            EntryError::Bzip2Failed(e) => write!(f, "a bzip2 block that would not decompress: {e}"),
            EntryError::PpmdFailed(e) => write!(f, "a PPMd block that would not decompress: {e}"),
            EntryError::Bcj2Failed => {
                f.write_str("a BCJ2 block whose range-coded stream is damaged")
            }
            EntryError::CrcMismatch => f.write_str("an entry whose recorded CRC-32 does not match"),
            EntryError::Truncated => f.write_str("a block shorter than its own substream table"),
            EntryError::UnsupportedCoder => f.write_str("a coder this build does not read"),
            EntryError::TooLarge => f.write_str("a block past the size this build decompresses"),
        }
    }
}

impl std::error::Error for EntryError {}

/// What reading an archive tolerated (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// A name that is not valid UTF-16, decoded with replacement characters.
    ///
    /// 7z stores names as UTF-16LE, so an unpaired surrogate is damage rather
    /// than an encoding this reader does not know — but it is one entry's name
    /// and not the archive, so it is a warning and the page still opens.
    NameNotUtf16 { index: usize },
    /// A name past [`Limits::max_name_len`], truncated to it.
    NameTruncated { index: usize },
    /// An entry the archive records no CRC-32 for.
    ///
    /// **Worth a warning because of what it costs**: the format's own check is
    /// the whole verification argument for the decompressor, and an entry
    /// without one is handed over unadjudicated.
    NoCrcRecorded { index: usize },
    /// A header property this build does not act on — timestamps, attributes,
    /// start positions. Counted once per archive.
    HeaderPropertyIgnored,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A regular file with bytes in a folder.
    File,
    /// A regular file of zero length, which 7z stores with no stream at all.
    EmptyFile,
    /// A directory: an empty stream that is not an empty file.
    Directory,
    /// A deletion marker in an incremental archive. Listed, never read.
    Anti,
}

/// One entry, as the header describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The stored path, `\` already turned into `/`.
    pub name: String,
    /// The unpacked size in bytes.
    pub size: u64,
    /// What kind of entry it is.
    pub kind: Kind,
    /// The CRC-32 the archive recorded, when it recorded one.
    pub crc: Option<u32>,
    /// Position in the archive's own file list.
    pub index: usize,
    /// Which folder holds it, and where inside that folder's output.
    at: Option<(usize, usize)>,
}

impl Entry {
    /// Whether this entry holds no file data.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        matches!(self.kind, Kind::Directory | Kind::Anti)
    }
}

/// Resource ceilings. Mandatory rather than defaulted, for
/// `tinker_pdf_zip::Limits`' reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Entries listed. See [`limits::MAX_7Z_ENTRIES`].
    pub max_entries: usize,
    /// Folders — decompression units — in one archive. See
    /// [`limits::MAX_7Z_FOLDERS`].
    pub max_folders: usize,
    /// Coders in one folder. See [`limits::MAX_7Z_CODERS`].
    pub max_coders: usize,
    /// Bytes one folder may decompress to, **and** the cap on a compressed
    /// header. See [`limits::MAX_7Z_UNPACKED`].
    pub max_unpacked: usize,
    /// Bytes of one stored path. See [`limits::MAX_7Z_NAME_LEN`].
    pub max_name_len: usize,
}

impl Limits {
    /// The constants in [`limits`], which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_entries: MAX_7Z_ENTRIES,
        max_folders: MAX_7Z_FOLDERS,
        max_coders: MAX_7Z_CODERS,
        max_unpacked: MAX_7Z_UNPACKED,
        max_name_len: MAX_7Z_NAME_LEN,
    };

    fn lzma(&self) -> lzma::Limits {
        lzma::Limits {
            max_unpacked: self.max_unpacked,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One coder in a folder's graph.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Coder {
    id: Vec<u8>,
    props: Vec<u8>,
    in_streams: usize,
    out_streams: usize,
}

/// A folder: the unit of decompression.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Folder {
    coders: Vec<Coder>,
    /// `(in stream index, out stream index)`, the format's own spelling.
    bind_pairs: Vec<(usize, usize)>,
    /// Which of the folder's in-streams are fed from the pack stream list.
    packed: Vec<usize>,
    /// One per out-stream, in order.
    unpack_sizes: Vec<u64>,
    crc: Option<u32>,
    /// Where this folder's packed bytes begin, and how many bytes they are,
    /// all of its pack streams together.
    packed_at: usize,
    packed_len: usize,
    /// Each pack stream on its own, `(offset, length)`, in the order of
    /// `packed`: the `k`th feeds in-stream `packed[k]`.
    pack_ranges: Vec<(usize, usize)>,
    /// Substreams: `(size, crc)`, in order.
    substreams: Vec<(u64, Option<u32>)>,
}

impl Folder {
    /// The out-stream nothing consumes, which is the folder's output. Every
    /// coder this build reads has one out-stream, so it is also a coder index.
    fn final_out(&self) -> Option<usize> {
        let total: usize = self.coders.iter().map(|c| c.out_streams).sum();
        (0..total).find(|out| !self.bind_pairs.iter().any(|(_, o)| o == out))
    }

    /// The index of `coder`'s first in-stream: in-streams are numbered across
    /// the folder, coder by coder.
    fn first_in(&self, coder: usize) -> usize {
        self.coders.iter().take(coder).map(|c| c.in_streams).sum()
    }

    /// Whether the coders make a tree this build can walk: every coder has one
    /// out-stream, every bind pair names streams that exist, no in-stream or
    /// out-stream is bound twice or both bound and packed, and a walk from
    /// the folder's output down its bind pairs meets every coder exactly once.
    /// A cycle, a coder no walk reaches, or a stream fed twice is a graph with
    /// no answer, and refused at open rather than discovered at read.
    fn is_walkable(&self) -> bool {
        let coders = self.coders.len();
        let total_in: usize = self.coders.iter().map(|c| c.in_streams).sum();
        if self.coders.iter().any(|c| c.out_streams != 1) {
            return false;
        }
        let mut in_used = vec![false; total_in];
        let mut out_used = vec![false; coders];
        for &(i, o) in &self.bind_pairs {
            let (Some(in_slot), Some(out_slot)) = (in_used.get_mut(i), out_used.get_mut(o)) else {
                return false;
            };
            if *in_slot || *out_slot {
                return false;
            }
            *in_slot = true;
            *out_slot = true;
        }
        for &p in &self.packed {
            match in_used.get_mut(p) {
                Some(slot) if !*slot => *slot = true,
                _ => return false,
            }
        }
        if in_used.iter().any(|used| !used) || self.packed.is_empty() {
            return false;
        }
        let Some(root) = self.final_out() else {
            return false;
        };
        // Down from the output, marking each coder as it is reached.
        let mut seen = vec![false; coders];
        let mut stack = vec![root];
        while let Some(coder) = stack.pop() {
            match seen.get_mut(coder) {
                Some(slot) if !*slot => *slot = true,
                _ => return false,
            }
            let Some(c) = self.coders.get(coder) else {
                return false;
            };
            let first = self.first_in(coder);
            for stream in first..first + c.in_streams {
                if let Some(&(_, feeder)) = self.bind_pairs.iter().find(|(i, _)| *i == stream) {
                    stack.push(feeder);
                }
            }
        }
        seen.iter().all(|s| *s)
    }

    fn unpack_size(&self) -> u64 {
        self.final_out()
            .and_then(|out| self.unpack_sizes.get(out).copied())
            .unwrap_or(0)
    }
}

/// A 7z archive, opened.
///
/// Holds the input, the entry list and one decompressed folder — see this
/// module's header for why the last of those is not optional.
#[derive(Clone, Debug)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    warnings: Vec<Warning>,
    folders: Vec<Folder>,
    limits: Limits,
    /// The last folder decompressed, kept so that reading five pages out of
    /// one solid block decompresses once.
    cached: Option<(usize, Vec<u8>)>,
}

impl<'a> Archive<'a> {
    /// Reads the signature header, decompresses the header if it is
    /// compressed, and walks it.
    ///
    /// # Errors
    /// [`Error`], one variant per way a file is not a readable 7z.
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Archive<'a>, Error> {
        if bytes.get(..6) != Some(SIGNATURE) || bytes.len() < SIGNATURE_HEADER {
            return Err(Error::NotA7z);
        }
        let start = bytes.get(12..32).ok_or(Error::NotA7z)?;
        // The start header's own CRC, checked before its two `u64`s are
        // believed. See `Error::StartHeaderCorrupt`.
        let stored = le32(bytes, 8).ok_or(Error::NotA7z)?;
        if crc32(start) != stored {
            return Err(Error::StartHeaderCorrupt);
        }
        let offset = le64(bytes, 12).ok_or(Error::NotA7z)?;
        let size = le64(bytes, 20).ok_or(Error::NotA7z)?;
        let header_crc = le32(bytes, 28).ok_or(Error::NotA7z)?;

        if size == 0 {
            // A legal empty archive: no header at all.
            return Ok(Archive {
                bytes,
                entries: Vec::new(),
                warnings: Vec::new(),
                folders: Vec::new(),
                limits: *limits,
                cached: None,
            });
        }
        let size = usize::try_from(size).map_err(|_| Error::HeaderOutOfRange)?;
        if size > limits.max_unpacked {
            return Err(Error::HeaderOutOfRange);
        }
        let at = usize::try_from(offset)
            .ok()
            .and_then(|o| SIGNATURE_HEADER.checked_add(o))
            .ok_or(Error::HeaderOutOfRange)?;
        let header = bytes
            .get(at..at.checked_add(size).ok_or(Error::HeaderOutOfRange)?)
            .ok_or(Error::HeaderOutOfRange)?;
        if crc32(header) != header_crc {
            return Err(Error::HeaderCrcMismatch);
        }

        let mut warnings = Vec::new();
        // `kEncodedHeader` is a `StreamsInfo` whose one folder decompresses to
        // the real header. One level only: an encoded header that itself
        // encodes a header is not a shape any writer emits and is refused
        // rather than recursed into.
        let decoded;
        let header = match header.first() {
            Some(&K_ENCODED_HEADER) => {
                let mut at = 1usize;
                let (folders, _) = streams_info(header, &mut at, bytes, limits, &mut warnings)?;
                let folder = folders.first().ok_or(Error::BadHeader)?;
                decoded =
                    decode_folder(bytes, folder, limits).map_err(|_| Error::HeaderNotDecodable)?;
                if let Some(want) = folder.crc {
                    if crc32(&decoded) != want {
                        return Err(Error::HeaderCrcMismatch);
                    }
                }
                decoded.as_slice()
            }
            _ => header,
        };

        let mut at = 0usize;
        if header.first() != Some(&K_HEADER) {
            return Err(Error::BadHeader);
        }
        at += 1;
        let (entries, folders, mut more) = read_header(header, &mut at, bytes, limits)?;
        warnings.append(&mut more);
        Ok(Archive {
            bytes,
            entries,
            warnings,
            folders,
            limits: *limits,
            cached: None,
        })
    }

    /// Every entry, in the order the archive lists them.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// What was tolerated, in the order it happened (ruling 10).
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// One entry's bytes, **owned**, with its recorded CRC-32 checked.
    ///
    /// `&mut self` and an owned `Vec` rather than a borrow, because a solid
    /// block is many files in one stream and there is no range of the input
    /// that is any one of them. The folder is cached, so reading a whole comic
    /// out of one block decompresses once.
    ///
    /// # Errors
    /// [`EntryError`], one variant per reason there are no bytes to hand over.
    /// [`EntryError::CrcMismatch`] is the format adjudicating this crate's
    /// decompressor and is never tolerated.
    pub fn read(&mut self, index: usize) -> Result<Vec<u8>, EntryError> {
        let entry = self.entries.get(index).ok_or(EntryError::NoSuchEntry)?;
        match entry.kind {
            Kind::File => {}
            Kind::EmptyFile => return Ok(Vec::new()),
            Kind::Directory | Kind::Anti => return Err(EntryError::NotAFile),
        }
        let (folder_index, offset) = entry.at.ok_or(EntryError::NotAFile)?;
        let size = usize::try_from(entry.size).map_err(|_| EntryError::TooLarge)?;
        let want = entry.crc;

        if self.cached.as_ref().map(|(i, _)| *i) != Some(folder_index) {
            let folder = self
                .folders
                .get(folder_index)
                .ok_or(EntryError::NoSuchEntry)?;
            let bytes = decode_folder(self.bytes, folder, &self.limits).map_err(|e| match e {
                FolderError::Unsupported => EntryError::UnsupportedCoder,
                FolderError::TooLarge => EntryError::TooLarge,
                FolderError::Lzma(e) => EntryError::FolderFailed(e),
                FolderError::Bzip2(e) => EntryError::Bzip2Failed(e),
                FolderError::Ppmd(e) => EntryError::PpmdFailed(e),
                FolderError::Bcj2 => EntryError::Bcj2Failed,
                FolderError::SizeMismatch => EntryError::Truncated,
            })?;
            self.cached = Some((folder_index, bytes));
        }
        let (_, block) = self.cached.as_ref().ok_or(EntryError::Truncated)?;
        let end = offset.checked_add(size).ok_or(EntryError::Truncated)?;
        let data = block.get(offset..end).ok_or(EntryError::Truncated)?;
        if let Some(want) = want {
            if crc32(data) != want {
                return Err(EntryError::CrcMismatch);
            }
        }
        Ok(data.to_vec())
    }
}

/// Why a folder did not decompress.
enum FolderError {
    Unsupported,
    TooLarge,
    Lzma(lzma::Error),
    Bzip2(bzip2::Error),
    Ppmd(ppmd::Error),
    /// BCJ2's range-coded stream does not open as one.
    Bcj2,
    /// A coder produced a length other than the one the header declared for
    /// its output — for a filter, whose output is its input, the header and
    /// the stream disagreeing about one number.
    SizeMismatch,
}

/// Runs a folder's coders over its packed bytes.
///
/// From the output down: a coder's inputs are each either a pack stream or
/// another coder's output, named by the folder's bind pairs, and each is
/// decoded before the coder that reads it. A chain — the common shape, one
/// coder feeding the next — is the case where every coder has one input; a
/// BCJ2 folder is the case where one coder has four. The graph was checked at
/// [`Archive::open`] to be a tree, and the depth bound here is the second
/// line of that, so a walk cannot recurse further than there are coders.
fn decode_folder(bytes: &[u8], folder: &Folder, limits: &Limits) -> Result<Vec<u8>, FolderError> {
    let root = folder.final_out().ok_or(FolderError::Unsupported)?;
    decode_coder(bytes, folder, root, limits, 0)
}

fn decode_coder(
    bytes: &[u8],
    folder: &Folder,
    coder: usize,
    limits: &Limits,
    depth: usize,
) -> Result<Vec<u8>, FolderError> {
    if depth > folder.coders.len() {
        return Err(FolderError::Unsupported);
    }
    let c = folder.coders.get(coder).ok_or(FolderError::Unsupported)?;
    let first = folder.first_in(coder);
    let mut owned: Vec<Vec<u8>> = Vec::new();
    let mut sources: Vec<Result<usize, &[u8]>> = Vec::with_capacity(c.in_streams);
    for stream in first..first + c.in_streams {
        if let Some(&(_, feeder)) = folder.bind_pairs.iter().find(|(i, _)| *i == stream) {
            owned.push(decode_coder(bytes, folder, feeder, limits, depth + 1)?);
            sources.push(Ok(owned.len() - 1));
        } else {
            let slot = folder
                .packed
                .iter()
                .position(|p| *p == stream)
                .ok_or(FolderError::Unsupported)?;
            let &(at, len) = folder.pack_ranges.get(slot).ok_or(FolderError::TooLarge)?;
            let packed = bytes
                .get(at..at.saturating_add(len))
                .ok_or(FolderError::TooLarge)?;
            sources.push(Err(packed));
        }
    }
    let inputs: Vec<&[u8]> = sources
        .iter()
        .map(|source| match source {
            Ok(i) => owned.get(*i).map_or(&[][..], Vec::as_slice),
            Err(packed) => packed,
        })
        .collect();
    let out_size = folder
        .unpack_sizes
        .get(coder)
        .copied()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(FolderError::TooLarge)?;
    if out_size > limits.max_unpacked {
        return Err(FolderError::TooLarge);
    }
    run_coder(c, &inputs, out_size, limits)
}

/// One coder, by its 7z method id, over its inputs in in-stream order.
fn run_coder(
    coder: &Coder,
    inputs: &[&[u8]],
    out_size: usize,
    limits: &Limits,
) -> Result<Vec<u8>, FolderError> {
    // Every coder but BCJ2 reads one stream; the arity was checked at open.
    let input = inputs.first().copied().unwrap_or_default();
    match coder.id.as_slice() {
        // Copy.
        [0x00] => Ok(input.to_vec()),
        // LZMA2. One property byte, the dictionary size, which this decoder
        // does not need: the window is the output. See `lzma`'s header.
        [0x21] => lzma::decode_lzma2(input, out_size, &limits.lzma()).map_err(FolderError::Lzma),
        // LZMA.
        [0x03, 0x01, 0x01] => {
            let props = coder
                .props
                .first()
                .copied()
                .ok_or(FolderError::Unsupported)?;
            lzma::decode(input, props, out_size, &limits.lzma()).map_err(FolderError::Lzma)
        }
        // BCJ, the x86 branch converter `-mf=BCJ` puts in front of LZMA2. A
        // filter rather than a compressor: the output is the input with its
        // call and jump operands rewritten, so the two lengths are one length
        // and a header that says otherwise is not describing this coder.
        BCJ_X86 => {
            if input.len() != out_size {
                return Err(FolderError::SizeMismatch);
            }
            let mut data = input.to_vec();
            bcj::x86_decode(&mut data);
            Ok(data)
        }
        // bzip2: a whole stream, `BZh` and all, exactly as ZIP method 12
        // stores one. Its own block and stream CRCs are checked inside the
        // decoder, and the folder's CRC-32 one layer up.
        BZIP2 => {
            let limits = bzip2::Limits {
                max_unpacked: out_size,
            };
            let data = bzip2::decode(input, &limits).map_err(FolderError::Bzip2)?;
            if data.len() != out_size {
                return Err(FolderError::SizeMismatch);
            }
            Ok(data)
        }
        // PPMd var.H. The model's arena is the one allocation the properties
        // size, and the folder cap bounds it as it bounds the output.
        PPMD => {
            let limits = ppmd::Limits {
                max_unpacked: out_size,
                max_memory: limits.max_unpacked,
            };
            ppmd::decode(input, &coder.props, out_size, &limits).map_err(FolderError::Ppmd)
        }
        // Deflate: 7z method `040108` is RFC 1951 with no wrapper, exactly as
        // ZIP method 8 is, which is the second half of this crate's edge into
        // `tinker-pdf-filters`.
        [0x04, 0x01, 0x08] => {
            let r = inflate_raw(input, &InflateLimits::new(out_size));
            if r.capped {
                return Err(FolderError::TooLarge);
            }
            // A stream that ended early is still an answer here: the folder's
            // own CRC-32 is checked one layer up, so short bytes fail the
            // format's check rather than being handed over as a page.
            Ok(r.data)
        }
        // BCJ2: the main stream, the CALL targets, the JMP targets and the
        // range-coded decisions, in that in-stream order.
        BCJ2 => {
            let [main, call, jump, rc] = inputs else {
                return Err(FolderError::Unsupported);
            };
            bcj2::decode(main, call, jump, rc, out_size).map_err(|e| match e {
                bcj2::Error::Truncated => FolderError::SizeMismatch,
                bcj2::Error::BadRangeStart => FolderError::Bcj2,
            })
        }
        _ => Err(FolderError::Unsupported),
    }
}

// ---- The header grammar -----------------------------------------------------

/// `Header`: the archive's file list and the streams behind it.
type HeaderParts = (Vec<Entry>, Vec<Folder>, Vec<Warning>);

fn read_header(
    h: &[u8],
    at: &mut usize,
    file: &[u8],
    limits: &Limits,
) -> Result<HeaderParts, Error> {
    let mut warnings = Vec::new();
    let mut folders: Vec<Folder> = Vec::new();
    let mut ignored = false;

    loop {
        let id = *h.get(*at).ok_or(Error::BadHeader)?;
        *at += 1;
        match id {
            K_END => break,
            K_ARCHIVE_PROPERTIES => {
                // Type/size pairs until a zero type.
                loop {
                    let kind = number(h, at).ok_or(Error::BadHeader)?;
                    if kind == 0 {
                        break;
                    }
                    let size = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                        .map_err(|_| Error::BadHeader)?;
                    *at = at.checked_add(size).ok_or(Error::BadHeader)?;
                }
                ignored = true;
            }
            K_ADDITIONAL_STREAMS => {
                // Only used by encrypted-name archives, which are refused
                // elsewhere; skipped as a whole rather than parsed.
                let (_, _) = streams_info(h, at, file, limits, &mut warnings)?;
                ignored = true;
            }
            K_MAIN_STREAMS => {
                let (f, _) = streams_info(h, at, file, limits, &mut warnings)?;
                folders = f;
            }
            K_FILES_INFO => {
                let entries = files_info(h, at, &folders, limits, &mut warnings)?;
                if ignored {
                    warnings.push(Warning::HeaderPropertyIgnored);
                }
                // `kFilesInfo` is the last thing in a header; the `kEnd` after
                // it is read by the loop above on the next turn.
                let id = *h.get(*at).ok_or(Error::BadHeader)?;
                if id != K_END {
                    return Err(Error::BadHeader);
                }
                return Ok((entries, folders, warnings));
            }
            _ => return Err(Error::BadHeader),
        }
    }

    // A header with streams and no `kFilesInfo` is legal and is what an
    // encoded header looks like; it has no entries.
    if ignored {
        warnings.push(Warning::HeaderPropertyIgnored);
    }
    Ok((Vec::new(), folders, warnings))
}

/// `StreamsInfo`: pack streams, folders, and the substreams inside them.
///
/// Returns the folders with their pack offsets, sizes, CRCs and substream
/// tables already resolved, because every one of those lives in a different
/// section of the grammar and joining them at the end is where an off-by-one
/// would hide.
fn streams_info(
    h: &[u8],
    at: &mut usize,
    file: &[u8],
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> Result<(Vec<Folder>, usize), Error> {
    let mut pack_pos = 0u64;
    let mut pack_sizes: Vec<u64> = Vec::new();
    let mut folders: Vec<Folder> = Vec::new();
    // Whether a `kSubStreamsInfo` said how the folders divide. Without one,
    // every folder is one substream; **with** one, a folder of zero substreams
    // is a thing a header may legally say, and defaulting it to one would
    // invent an entry that the file list has no name for.
    let mut divided = false;

    loop {
        let id = *h.get(*at).ok_or(Error::BadHeader)?;
        *at += 1;
        match id {
            K_END => break,
            K_PACK_INFO => {
                pack_pos = number(h, at).ok_or(Error::BadHeader)?;
                let count = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                    .map_err(|_| Error::BadHeader)?;
                if count > limits.max_entries {
                    return Err(Error::TooManyEntries);
                }
                loop {
                    let id = *h.get(*at).ok_or(Error::BadHeader)?;
                    *at += 1;
                    match id {
                        K_END => break,
                        K_SIZE => {
                            pack_sizes = (0..count)
                                .map(|_| number(h, at).ok_or(Error::BadHeader))
                                .collect::<Result<_, _>>()?;
                        }
                        K_CRC => {
                            let _ = digests(h, at, count).ok_or(Error::BadHeader)?;
                        }
                        _ => return Err(Error::BadHeader),
                    }
                }
            }
            K_UNPACK_INFO => {
                folders = unpack_info(h, at, limits)?;
            }
            K_SUBSTREAMS_INFO => {
                substreams_info(h, at, &mut folders, limits)?;
                divided = true;
            }
            _ => return Err(Error::BadHeader),
        }
    }

    // Fold the pack stream list into the folders: each folder consumes as many
    // pack streams as it has packed in-streams, in order.
    let base = usize::try_from(pack_pos)
        .ok()
        .and_then(|p| SIGNATURE_HEADER.checked_add(p))
        .ok_or(Error::HeaderOutOfRange)?;
    let mut at_pack = base;
    let mut taken = 0usize;
    for folder in &mut folders {
        let count = folder.packed.len().max(1);
        let mut len = 0u64;
        let mut stream_at = at_pack;
        for _ in 0..count {
            let size = pack_sizes.get(taken).copied().ok_or(Error::BadHeader)?;
            taken += 1;
            len = len.saturating_add(size);
            let size = usize::try_from(size).map_err(|_| Error::HeaderOutOfRange)?;
            folder.pack_ranges.push((stream_at, size));
            stream_at = stream_at.saturating_add(size);
        }
        folder.packed_at = at_pack;
        folder.packed_len = usize::try_from(len).map_err(|_| Error::HeaderOutOfRange)?;
        if folder
            .packed_at
            .checked_add(folder.packed_len)
            .is_none_or(|end| end > file.len())
        {
            return Err(Error::HeaderOutOfRange);
        }
        at_pack = at_pack.saturating_add(folder.packed_len);
        // A folder with no substream table is one substream: itself.
        if !divided && folder.substreams.is_empty() {
            folder.substreams = vec![(folder.unpack_size(), folder.crc)];
        }
    }
    let _ = warnings;
    Ok((folders, taken))
}

/// `UnpackInfo`: the folders, their coders and their output sizes.
fn unpack_info(h: &[u8], at: &mut usize, limits: &Limits) -> Result<Vec<Folder>, Error> {
    if *h.get(*at).ok_or(Error::BadHeader)? != K_FOLDER {
        return Err(Error::BadHeader);
    }
    *at += 1;
    let count =
        usize::try_from(number(h, at).ok_or(Error::BadHeader)?).map_err(|_| Error::BadHeader)?;
    if count > limits.max_folders {
        return Err(Error::TooManyEntries);
    }
    let external = *h.get(*at).ok_or(Error::BadHeader)?;
    *at += 1;
    if external != 0 {
        // Folders stored in an additional stream. No writer emits this for a
        // plain archive, and reading it would mean decompressing before the
        // folder list is known.
        return Err(Error::BadHeader);
    }
    let mut folders: Vec<Folder> = (0..count)
        .map(|_| folder(h, at, limits))
        .collect::<Result<_, _>>()?;

    if *h.get(*at).ok_or(Error::BadHeader)? != K_CODERS_UNPACK_SIZE {
        return Err(Error::BadHeader);
    }
    *at += 1;
    for folder in &mut folders {
        let outs: usize = folder.coders.iter().map(|c| c.out_streams).sum();
        folder.unpack_sizes = (0..outs)
            .map(|_| number(h, at).ok_or(Error::BadHeader))
            .collect::<Result<_, _>>()?;
    }

    loop {
        let id = *h.get(*at).ok_or(Error::BadHeader)?;
        *at += 1;
        match id {
            K_END => break,
            K_CRC => {
                let crcs = digests(h, at, folders.len()).ok_or(Error::BadHeader)?;
                for (folder, crc) in folders.iter_mut().zip(crcs) {
                    folder.crc = crc;
                }
            }
            _ => return Err(Error::BadHeader),
        }
    }
    Ok(folders)
}

/// One folder's coder list and bind pairs.
fn folder(h: &[u8], at: &mut usize, limits: &Limits) -> Result<Folder, Error> {
    let count =
        usize::try_from(number(h, at).ok_or(Error::BadHeader)?).map_err(|_| Error::BadHeader)?;
    if count == 0 || count > limits.max_coders {
        return Err(Error::TooManyEntries);
    }
    let mut coders = Vec::with_capacity(count);
    let mut total_in = 0usize;
    let mut total_out = 0usize;
    for _ in 0..count {
        let flags = *h.get(*at).ok_or(Error::BadHeader)?;
        *at += 1;
        let id_len = usize::from(flags & 0x0F);
        let id = h
            .get(*at..at.checked_add(id_len).ok_or(Error::BadHeader)?)
            .ok_or(Error::BadHeader)?
            .to_vec();
        *at += id_len;
        let (in_streams, out_streams) = if flags & 0x10 != 0 {
            let i = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                .map_err(|_| Error::BadHeader)?;
            let o = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                .map_err(|_| Error::BadHeader)?;
            (i, o)
        } else {
            (1, 1)
        };
        if in_streams > limits.max_coders || out_streams > limits.max_coders {
            return Err(Error::TooManyEntries);
        }
        let props = if flags & 0x20 != 0 {
            let size = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                .map_err(|_| Error::BadHeader)?;
            let props = h
                .get(*at..at.checked_add(size).ok_or(Error::BadHeader)?)
                .ok_or(Error::BadHeader)?
                .to_vec();
            *at += size;
            props
        } else {
            Vec::new()
        };
        // AES-256 + SHA-256, refused where it is named rather than as an
        // unreadable stream three layers down.
        if id.as_slice() == [0x06, 0xF1, 0x07, 0x01] {
            return Err(Error::Encrypted);
        }
        total_in += in_streams;
        total_out += out_streams;
        coders.push(Coder {
            id,
            props,
            in_streams,
            out_streams,
        });
    }

    let bind_count = total_out.checked_sub(1).ok_or(Error::BadHeader)?;
    let mut bind_pairs = Vec::with_capacity(bind_count);
    for _ in 0..bind_count {
        let i = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
            .map_err(|_| Error::BadHeader)?;
        let o = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
            .map_err(|_| Error::BadHeader)?;
        bind_pairs.push((i, o));
    }

    let packed_count = total_in.checked_sub(bind_count).ok_or(Error::BadHeader)?;
    let packed = if packed_count == 1 {
        // The one in-stream no bind pair feeds.
        vec![(0..total_in)
            .find(|i| !bind_pairs.iter().any(|(bi, _)| bi == i))
            .ok_or(Error::BadHeader)?]
    } else {
        (0..packed_count)
            .map(|_| {
                usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                    .map_err(|_| Error::BadHeader)
            })
            .collect::<Result<_, _>>()?
    };

    // Refused here rather than at read, so an unreadable method is one
    // sentence about the archive rather than five identical page defects.
    for coder in &coders {
        let id = coder.id.as_slice();
        let known = matches!(
            id,
            [0x00]
                | [0x21]
                | [0x03, 0x01, 0x01]
                | [0x04, 0x01, 0x08]
                | BCJ_X86
                | BCJ2
                | BZIP2
                | PPMD
        );
        // BCJ and BCJ2 have no properties, and 7-Zip since 23 refuses a coder
        // handed properties it has no use for rather than ignoring them: a
        // header that carries some is describing a different filter.
        let bare = !(id == BCJ_X86 || id == BCJ2) || coder.props.is_empty();
        if !known || !bare {
            return Err(Error::UnsupportedCoder {
                id: coder.id.clone(),
            });
        }
    }
    // Every coder this build reads has one output, and one input but BCJ2,
    // which has four. A known coder declaring other counts is not that coder.
    for coder in &coders {
        let inputs = if coder.id.as_slice() == BCJ2 { 4 } else { 1 };
        if coder.in_streams != inputs || coder.out_streams != 1 {
            return Err(Error::NotAChain);
        }
    }

    let folder = Folder {
        coders,
        bind_pairs,
        packed,
        unpack_sizes: Vec::new(),
        crc: None,
        packed_at: 0,
        packed_len: 0,
        pack_ranges: Vec::new(),
        substreams: Vec::new(),
    };
    if !folder.is_walkable() {
        return Err(Error::NotAChain);
    }
    Ok(folder)
}

/// `SubStreamsInfo`: how a folder's one output stream is divided into files.
fn substreams_info(
    h: &[u8],
    at: &mut usize,
    folders: &mut [Folder],
    limits: &Limits,
) -> Result<(), Error> {
    let mut counts: Vec<usize> = folders.iter().map(|_| 1usize).collect();
    let mut sizes: Option<Vec<Vec<u64>>> = None;

    loop {
        let id = *h.get(*at).ok_or(Error::BadHeader)?;
        *at += 1;
        match id {
            K_END => break,
            K_NUM_UNPACK_STREAM => {
                counts = folders
                    .iter()
                    .map(|_| {
                        usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
                            .map_err(|_| Error::BadHeader)
                    })
                    .collect::<Result<_, _>>()?;
                if counts.iter().sum::<usize>() > limits.max_entries {
                    return Err(Error::TooManyEntries);
                }
            }
            K_SIZE => {
                // The last substream of each folder is not listed: it is
                // whatever the folder's output has left. A reader that
                // listed it would disagree with every writer.
                let mut all = Vec::with_capacity(folders.len());
                for (folder, &count) in folders.iter().zip(counts.iter()) {
                    if count == 0 {
                        all.push(Vec::new());
                        continue;
                    }
                    let mut here = Vec::with_capacity(count);
                    let mut sum = 0u64;
                    for _ in 0..count.saturating_sub(1) {
                        let size = number(h, at).ok_or(Error::BadHeader)?;
                        sum = sum.saturating_add(size);
                        here.push(size);
                    }
                    here.push(folder.unpack_size().saturating_sub(sum));
                    all.push(here);
                }
                sizes = Some(all);
            }
            K_CRC => {
                // Only substreams whose CRC is not already known from the
                // folder's own are listed here.
                let unknown: usize = folders
                    .iter()
                    .zip(counts.iter())
                    .map(|(folder, &count)| {
                        if count == 1 && folder.crc.is_some() {
                            0
                        } else {
                            count
                        }
                    })
                    .sum();
                let mut crcs = digests(h, at, unknown).ok_or(Error::BadHeader)?.into_iter();
                let mut per_folder: Vec<Vec<Option<u32>>> = Vec::with_capacity(folders.len());
                for (folder, &count) in folders.iter().zip(counts.iter()) {
                    if count == 1 && folder.crc.is_some() {
                        per_folder.push(vec![folder.crc]);
                    } else {
                        per_folder.push((0..count).map(|_| crcs.next().flatten()).collect());
                    }
                }
                for (folder, crcs) in folders.iter_mut().zip(per_folder) {
                    folder.substreams = crcs.into_iter().map(|c| (0, c)).collect();
                }
            }
            _ => return Err(Error::BadHeader),
        }
    }

    for (index, folder) in folders.iter_mut().enumerate() {
        let count = counts.get(index).copied().unwrap_or(1);
        let listed = match &sizes {
            Some(all) => all.get(index).cloned().unwrap_or_default(),
            None if count == 1 => vec![folder.unpack_size()],
            None => return Err(Error::BadHeader),
        };
        let crcs: Vec<Option<u32>> = if folder.substreams.len() == count {
            folder.substreams.iter().map(|(_, c)| *c).collect()
        } else {
            vec![None; count]
        };
        folder.substreams = listed
            .into_iter()
            .zip(crcs.into_iter().chain(core::iter::repeat(None)))
            .collect();
    }
    Ok(())
}

/// `FilesInfo`: the names, and which files have streams.
fn files_info(
    h: &[u8],
    at: &mut usize,
    folders: &[Folder],
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<Entry>, Error> {
    let count =
        usize::try_from(number(h, at).ok_or(Error::BadHeader)?).map_err(|_| Error::BadHeader)?;
    if count > limits.max_entries {
        return Err(Error::TooManyEntries);
    }
    let mut empty_stream = vec![false; count];
    let mut empty_file: Vec<bool> = Vec::new();
    let mut anti: Vec<bool> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut ignored = false;

    loop {
        let kind = number(h, at).ok_or(Error::BadHeader)?;
        if kind == 0 {
            break;
        }
        let size = usize::try_from(number(h, at).ok_or(Error::BadHeader)?)
            .map_err(|_| Error::BadHeader)?;
        let end = at.checked_add(size).ok_or(Error::BadHeader)?;
        let body = h.get(*at..end).ok_or(Error::BadHeader)?;
        let mut inner = 0usize;
        match u8::try_from(kind).unwrap_or(0xFF) {
            K_EMPTY_STREAM => {
                empty_stream = bits(body, &mut inner, count).ok_or(Error::BadHeader)?
            }
            K_EMPTY_FILE => {
                let n = empty_stream.iter().filter(|b| **b).count();
                empty_file = bits(body, &mut inner, n).ok_or(Error::BadHeader)?;
            }
            K_ANTI => {
                let n = empty_stream.iter().filter(|b| **b).count();
                anti = bits(body, &mut inner, n).ok_or(Error::BadHeader)?;
            }
            K_NAME => {
                let external = *body.first().ok_or(Error::BadHeader)?;
                if external != 0 {
                    return Err(Error::BadHeader);
                }
                names = utf16_names(body.get(1..).unwrap_or_default(), count, limits, warnings);
            }
            K_DUMMY => {}
            _ => ignored = true,
        }
        *at = end;
    }
    if ignored {
        warnings.push(Warning::HeaderPropertyIgnored);
    }

    // Files with a stream take substreams in order, folder by folder.
    let mut stream_slots: Vec<(usize, usize, u64, Option<u32>)> = Vec::new();
    for (folder_index, folder) in folders.iter().enumerate() {
        let mut offset = 0usize;
        for (size, crc) in &folder.substreams {
            stream_slots.push((folder_index, offset, *size, *crc));
            offset = offset.saturating_add(usize::try_from(*size).unwrap_or(usize::MAX));
        }
    }

    let mut slots = stream_slots.into_iter();
    let mut empty_seen = 0usize;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let name = names
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("entry{index}"));
        let is_empty = empty_stream.get(index).copied().unwrap_or(false);
        if is_empty {
            let is_anti = anti.get(empty_seen).copied().unwrap_or(false);
            let is_file = empty_file.get(empty_seen).copied().unwrap_or(false);
            empty_seen += 1;
            entries.push(Entry {
                name,
                size: 0,
                kind: if is_anti {
                    Kind::Anti
                } else if is_file {
                    Kind::EmptyFile
                } else {
                    Kind::Directory
                },
                crc: None,
                index,
                at: None,
            });
            continue;
        }
        let (folder_index, offset, size, crc) = slots.next().ok_or(Error::BadHeader)?;
        if crc.is_none() {
            warnings.push(Warning::NoCrcRecorded { index });
        }
        entries.push(Entry {
            name,
            size,
            kind: Kind::File,
            crc,
            index,
            at: Some((folder_index, offset)),
        });
    }
    Ok(entries)
}

// ---- Primitives -------------------------------------------------------------

/// 7z's own variable-length integer.
///
/// The first byte's high bits say how many more follow, and its remaining low
/// bits are the *most* significant part of the value — which is the detail
/// that makes a hand-written decoder wrong on its first try, and is why
/// `a_number_decodes_the_way_the_format_spells_it` walks all eight widths.
fn number(input: &[u8], at: &mut usize) -> Option<u64> {
    let first = *input.get(*at)?;
    *at += 1;
    let mut mask = 0x80u8;
    let mut value = 0u64;
    for i in 0..8 {
        if first & mask == 0 {
            let high = u64::from(first & mask.wrapping_sub(1));
            return Some(value | high.checked_shl(8 * i).unwrap_or(0));
        }
        let byte = *input.get(*at)?;
        *at += 1;
        value |= u64::from(byte) << (8 * i);
        mask >>= 1;
    }
    Some(value)
}

/// A packed bit vector, most significant bit of each byte first.
fn bits(input: &[u8], at: &mut usize, count: usize) -> Option<Vec<bool>> {
    let bytes = count.div_ceil(8);
    let slice = input.get(*at..at.checked_add(bytes)?)?;
    *at += bytes;
    Some(
        (0..count)
            .map(|i| {
                slice
                    .get(i / 8)
                    .is_some_and(|b| (b >> (7 - i % 8)) & 1 == 1)
            })
            .collect(),
    )
}

/// `allAreDefined`, then an optional bit vector, then a `u32` per defined.
fn digests(input: &[u8], at: &mut usize, count: usize) -> Option<Vec<Option<u32>>> {
    let all = *input.get(*at)?;
    *at += 1;
    let defined = if all != 0 {
        vec![true; count]
    } else {
        bits(input, at, count)?
    };
    let mut out = Vec::with_capacity(count);
    for is_defined in defined {
        if is_defined {
            let value = le32(input, *at)?;
            *at += 4;
            out.push(Some(value));
        } else {
            out.push(None);
        }
    }
    Some(out)
}

/// NUL-terminated UTF-16LE names, one after another.
///
/// Replacement characters rather than a refusal for an unpaired surrogate: a
/// name is what decides page order, and a comic whose ninth page has a broken
/// character in its name still has nine pages. The warning is what says so.
fn utf16_names(
    body: &[u8],
    count: usize,
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> Vec<String> {
    let mut names = Vec::with_capacity(count);
    let mut units: Vec<u16> = Vec::new();
    let mut index = 0usize;
    for pair in body.chunks_exact(2) {
        let unit = u16::from(pair[0]) | (u16::from(pair[1]) << 8);
        if unit == 0 {
            let mut lossy = false;
            let mut name: String = char::decode_utf16(units.iter().copied())
                .map(|c| {
                    c.unwrap_or_else(|_| {
                        lossy = true;
                        char::REPLACEMENT_CHARACTER
                    })
                })
                // 7z stores a path separator as `\` on Windows and `/`
                // elsewhere; a comic's page order must not depend on which.
                .map(|c| if c == '\\' { '/' } else { c })
                .collect();
            if lossy {
                warnings.push(Warning::NameNotUtf16 { index });
            }
            if name.len() > limits.max_name_len {
                warnings.push(Warning::NameTruncated { index });
                // On a character boundary, so a truncated name is still a
                // `String`.
                let mut cut = limits.max_name_len;
                while cut > 0 && !name.is_char_boundary(cut) {
                    cut -= 1;
                }
                name.truncate(cut);
            }
            names.push(name);
            units.clear();
            index += 1;
            if index >= count {
                break;
            }
            continue;
        }
        units.push(unit);
    }
    names
}

fn le32(input: &[u8], at: usize) -> Option<u32> {
    let slice = input.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn le64(input: &[u8], at: usize) -> Option<u64> {
    let slice = input.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

/// A method id, for a refusal that names it.
fn hex(id: &[u8]) -> String {
    id.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests;
