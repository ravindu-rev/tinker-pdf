//! ZIP containers: bytes in, entries out, with no PDF vocabulary anywhere.
//!
//! Scope, design and exit criteria: `docs/plans/gaps/29-cbz.md`, milestone 2.
//!
//! A CBZ is a ZIP of page images and an XPS document is an OPC package, which
//! is also a ZIP, so this is a leaf crate rather than a module in whatever
//! opens the first of them. It knows about APPNOTE 6.3.10 and nothing else: no
//! catalog, no page tree, no image format, no idea what any entry is *for*.
//! Deciding which entries are pages is the facade's job (ruling 8), and this
//! crate would be wrong to have an opinion.
//!
//! # The three things it is worth knowing before reading further
//!
//! **The central directory wins, and the local header is consulted for one
//! thing.** [`Archive::read`] computes where an entry's data starts from the
//! local header — it must, because the local extra field may be a different
//! length from the central one — and takes the CRC, the sizes, the method and
//! the flags from the central directory. A local header that disagrees is
//! recorded as [`Warning::LocalHeaderDisagrees`] and not followed. The reason
//! is not a preference for one record over another: an entry written with
//! general-purpose bit 3 set has **zeros** in its local header by design
//! (APPNOTE 4.3.9), because its producer did not know the sizes when it wrote
//! them, so "the local header disagrees" is the *normal* state of a streamed
//! entry and a reader that followed it would refuse every one of them.
//!
//! **Deflated entries go through [`tinker_pdf_filters::inflate_raw`] and never
//! through `flate_decode`.** Method 8 is RFC 1951 with no wrapper by
//! definition (APPNOTE 4.4.5), so a caller here knows a priori what it holds;
//! the sniffing door treats raw DEFLATE as a *fallback* and would raise a
//! [`Warning::Inflate`] carrying `RawDeflateFallback` on every deflated entry
//! of every archive, which is why gap 29 made the raw door public in its own
//! milestone before this one existed. `no_deflated_entry_is_read_through_the
//! _sniffing_door` is the tripwire: the wrong door is visible from outside
//! this crate, so a later refactor cannot quietly take it.
//!
//! **Every entry is checksummed, and an entry that cannot be is refused.**
//! Gap 29's design copies image bytes verbatim into a PDF stream, so nothing
//! in this engine ever looks at them again — the archive's CRC is the only
//! integrity evidence that will ever exist for them, and a CRC read and not
//! compared is gap 16's `let (gray, _) = ccitt_decode(...)` with a checksum.
//! An entry whose checksum the archive never recorded (the recovery route,
//! with an unsigned data descriptor) is listed so the count stays honest and
//! refused at [`Archive::read`], because handing back unchecked bytes is the
//! one answer this crate may not give.
//!
//! # The ladder
//!
//! The central directory is the route. When there is no end-of-central-
//! directory record, or its offsets do not land on `PK\x01\x02`, the archive
//! is recovered by scanning for local file headers — the posture
//! `tinker-pdf-cos` already takes for a damaged cross-reference table, and for
//! the same reason: a truncated archive still holds the entries it has.
//! [`Archive::route`] says which was used and [`Warning`] records why.

#![forbid(unsafe_code)]

mod cp437;
mod dir;
mod le;
pub mod limits;
mod local;
mod scan;

#[cfg(test)]
mod build;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt;

use tinker_pdf_filters::{crc32, inflate_raw, Limits as InflateLimits};

pub use tinker_pdf_filters::Warning as InflateWarning;

/// Resource ceilings, defaulting to the constants in [`limits`].
///
/// Separate fields rather than one number because they bound different things
/// and one of them is a **total**: [`Limits::max_inflated_total`] is spent
/// across every entry of an archive and never refunded, and
/// [`Limits::max_entry_bytes`] bounds one entry and is not that total. Reading
/// the second as the first is the mistake `MAX_SCRIPT_TOTAL` and
/// `MAX_TILE_WORK` each exist to name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Entries enumerated from either route. See [`limits::MAX_ZIP_ENTRIES`].
    pub max_entries: usize,
    /// Bytes one entry may inflate to. **Not the total.** See
    /// [`limits::MAX_ZIP_ENTRY_BYTES`].
    pub max_entry_bytes: usize,
    /// Bytes one archive may inflate, across every entry. See
    /// [`limits::MAX_ZIP_INFLATED`].
    pub max_inflated_total: usize,
    /// Decoded bytes of one entry name; longer names truncate and warn. See
    /// [`limits::MAX_ZIP_NAME_LEN`].
    pub max_name_len: usize,
}

impl Limits {
    /// The constants in [`limits`], which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_entries: limits::MAX_ZIP_ENTRIES,
        max_entry_bytes: limits::MAX_ZIP_ENTRY_BYTES,
        max_inflated_total: limits::MAX_ZIP_INFLATED,
        max_name_len: limits::MAX_ZIP_NAME_LEN,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Where an archive's entry list came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    /// The end-of-central-directory record and the directory it points at.
    CentralDirectory,
    /// Local file headers, found by scanning. The recovery rung.
    LocalHeaderScan,
}

/// How an entry's data is stored (APPNOTE 4.4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    /// Method 0: the data is the entry, uncompressed.
    Stored,
    /// Method 8: raw DEFLATE, RFC 1951, no wrapper.
    Deflated,
    /// Anything else — shrink, implode, bzip2, LZMA, Zstandard. Named rather
    /// than collapsed, so a refusal can say which.
    Other(u16),
}

impl Method {
    /// APPNOTE 4.4.5's compression method, as a value rather than a number.
    ///
    /// Every method but the two this build reads keeps its code, so a refusal
    /// can name it: "method 14" tells a reader the archive used LZMA, where a
    /// bare "unsupported" tells them nothing they can act on.
    pub(crate) const fn from_code(code: u16) -> Method {
        match code {
            0 => Method::Stored,
            8 => Method::Deflated,
            other => Method::Other(other),
        }
    }
}

/// One entry of the archive's directory.
///
/// **Every number here is a claim the archive made**, and none of them is
/// trusted to be true: [`Archive::read`] bounds the declared size before it
/// inflates anything, and checks both the produced length and the CRC against
/// what was declared before it returns a byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The stored path, decoded as UTF-8 under general-purpose bit 11 and as
    /// CP437 otherwise (APPNOTE D.1), truncated at
    /// [`Limits::max_name_len`]. Separators are `/` per 4.4.17.1; this crate
    /// neither normalises the path nor resolves `..`, because it never touches
    /// a filesystem and inventing a canonical form would be a claim about one.
    pub name: String,
    pub method: Method,
    /// The CRC-32 of the uncompressed data, or `None` when the archive never
    /// recorded one this crate could find — the recovery route with an
    /// unsigned data descriptor. Such an entry is refused rather than read.
    pub crc: Option<u32>,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    /// General-purpose bit 0. The entry is refused at [`Archive::read`]:
    /// neither ZipCrypto nor the AES extensions are in scope, and both are
    /// named non-goals in gap 29.
    pub encrypted: bool,
    /// General-purpose bit 3: the sizes and CRC follow the data rather than
    /// preceding it.
    pub streamed: bool,
    /// Where the local file header sits. Not the data — the data starts after
    /// the header's own name and extra fields.
    pub header_offset: u64,
    /// Position in the directory, which is the archive's own order. Ties in
    /// any sort a caller applies break on this, so the order is total.
    pub index: usize,
}

impl Entry {
    /// A directory entry rather than a file: 4.4.17.1 says a stored path that
    /// ends in `/` is one.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }
}

/// Everything this crate tolerated, as data rather than a log line (ruling
/// 10).
///
/// The set is closed and each variant is recorded **at most once per
/// archive**: an archive of two hundred entries with two hundred disagreeing
/// local headers yields one [`Warning::LocalHeaderDisagrees`], not two
/// hundred. That is the same contract `tinker_pdf_filters::Warning` carries,
/// and it is what keeps "it opened" and "it opened cleanly" distinguishable
/// for a format whose entry count is chosen by the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    /// No end-of-central-directory record was found anywhere in the last
    /// 64 KiB plus the record's own length, which is as far as a comment can
    /// legally push it.
    NoEndOfCentralDirectory,
    /// The record was found and the directory it named was not there: an
    /// offset past the archive, or one that does not land on `PK\x01\x02`.
    /// A self-extracting archive with a stub in front of it lands here, since
    /// every offset in it is short by the length of the stub.
    CentralDirectoryUnreadable,
    /// The entry list was recovered by scanning for local file headers. The
    /// route, named, rather than left to be inferred from the two above.
    RecoveredFromLocalHeaders,
    /// The end-of-central-directory record's entry count is not the number of
    /// records the directory actually holds. The records win; the count is a
    /// claim a 22-byte record can make about four billion entries.
    EntryCountDisagrees,
    /// A local file header's method, flags, CRC or sizes differ from the
    /// central directory's. The central directory was followed.
    LocalHeaderDisagrees,
    /// A data descriptor was absent from where the entry's own length says it
    /// is, or disagreed with the central directory. The central directory was
    /// followed.
    DataDescriptorDisagrees,
    /// A deflate stream ended somewhere other than the compressed size the
    /// directory declared. The stream's own end was used, because a DEFLATE
    /// stream says where it stops and a directory record only claims to.
    CompressedSizeDisagrees,
    /// General-purpose bit 11 said the name is UTF-8 and it was not valid
    /// UTF-8; it was read as CP437 instead.
    NameNotUtf8,
    /// A name longer than [`Limits::max_name_len`] was truncated. The entry is
    /// kept: dropping it would drop a page and renumber every page after it.
    NameTruncated,
    /// What the inflater reported, carried verbatim.
    ///
    /// On a healthy archive this never appears, which is what makes it a
    /// tripwire as well as a record: `RawDeflateFallback` here would mean this
    /// crate had been rewired to `flate_decode`, and `OutputCapHit` would mean
    /// an entry produced more than it declared.
    Inflate(InflateWarning),
}

/// Why a whole archive was refused.
///
/// Each of these is a refusal **by name**, which is gap 29's feature rather
/// than its absence: "this is a comic archive and here is why I will not open
/// it" is a different sentence from "this is not a document", and a host shows
/// different things for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArchiveError {
    /// Not a ZIP at all: no end-of-central-directory record and no local file
    /// header anywhere in the bytes.
    NotAZip,
    /// ZIP structure is present and nothing could be recovered from it.
    Damaged,
    /// Spanned or multi-disk. A disk number other than zero refuses the
    /// archive rather than reading the fragment that happens to be here.
    MultiDisk,
    /// A Zip64 locator or record declares a size or offset the archive cannot
    /// contain, or one this platform's `usize` cannot hold.
    Zip64OutOfBounds,
    /// More entries than [`Limits::max_entries`]. The archive is refused
    /// rather than truncated to the cap: a truncated entry list is a document
    /// with pages missing from the end and nothing saying so.
    TooManyEntries,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            Self::NotAZip => "not a ZIP archive",
            Self::Damaged => "ZIP structure found, no entries recoverable",
            Self::MultiDisk => "spanned or multi-disk archive",
            Self::Zip64OutOfBounds => "a Zip64 value the archive cannot contain",
            Self::TooManyEntries => "more entries than the cap allows",
        };
        f.write_str(what)
    }
}

impl std::error::Error for ArchiveError {}

/// Why one entry was refused.
///
/// A refusal here is per-entry and leaves the rest of the archive alone: gap
/// 29 refuses an archive of a hundred JPEGs and one GIF at the *page* level,
/// because refusing the archive would throw away a hundred readable pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryError {
    /// No entry with that index. A caller's bug rather than a file's.
    NoSuchEntry,
    /// General-purpose bit 0. ZipCrypto and the AES extensions are non-goals.
    Encrypted,
    /// A compression method other than stored or deflated, named.
    UnsupportedMethod(u16),
    /// The archive recorded no CRC for this entry that could be found, so
    /// there is nothing to check the bytes against and they are not handed
    /// back unchecked.
    NoChecksum,
    /// The local file header the directory pointed at is not there.
    LocalHeaderLost,
    /// The entry's data runs past the end of the archive, or its deflate
    /// stream ended in the middle of a block.
    Truncated,
    /// The deflate stream was structurally invalid.
    Corrupt,
    /// The stream had more to give than the entry's declared uncompressed
    /// size. The declaration is what bounded the decode, so the excess is
    /// evidence the declaration was a lie rather than a size to allocate for.
    OversizedStream { declared: u64 },
    /// What came out is not the length that was declared.
    SizeMismatch { declared: u64, produced: u64 },
    /// The CRC-32 of the data is not the one the archive recorded. **The
    /// entry is refused, not warned about**: this is the only check that will
    /// ever be made on bytes gap 29's design passes through untouched.
    ChecksumMismatch { declared: u32, computed: u32 },
    /// The entry's declared size is at or past [`Limits::max_entry_bytes`], so
    /// it was refused before anything was inflated. Per-entry, and not the
    /// archive's total.
    EntryTooLarge,
    /// [`Limits::max_inflated_total`] is spent. The archive's total, which a
    /// per-entry cap is not.
    ArchiveBudgetSpent,
}

impl fmt::Display for EntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchEntry => f.write_str("no entry with that index"),
            Self::Encrypted => f.write_str("encrypted entry"),
            Self::UnsupportedMethod(m) => write!(f, "compression method {m} is not read here"),
            Self::NoChecksum => f.write_str("no CRC-32 was recorded for this entry"),
            Self::LocalHeaderLost => f.write_str("no local file header where the directory said"),
            Self::Truncated => f.write_str("the entry's data ends before the entry does"),
            Self::Corrupt => f.write_str("the deflate stream is structurally invalid"),
            Self::OversizedStream { declared } => {
                write!(
                    f,
                    "the stream produces more than the {declared} bytes declared"
                )
            }
            Self::SizeMismatch { declared, produced } => {
                write!(f, "declared {declared} bytes, produced {produced}")
            }
            Self::ChecksumMismatch { declared, computed } => {
                write!(
                    f,
                    "CRC-32 {computed:#010x}, the archive says {declared:#010x}"
                )
            }
            Self::EntryTooLarge => f.write_str("the entry is past the per-entry ceiling"),
            Self::ArchiveBudgetSpent => f.write_str("the archive's inflation total is spent"),
        }
    }
}

impl std::error::Error for EntryError {}

/// The total, spent and never refunded.
///
/// A per-entry ceiling times an entry count the file chose is not a bound,
/// which is `5adf502`'s finding restated for a format that needs no nesting to
/// be dangerous. What is charged is what an entry was **permitted** to
/// produce, decided before it is read: a permit is the number of bytes this
/// crate has undertaken to hand back, and an entry that was allowed a
/// gigabyte has cost the archive a gigabyte whether or not it delivered one.
/// Charging what happened to arrive instead would let a file ask for the
/// budget over and over by delivering nothing.
///
/// Nothing is refunded, including by an entry that is then refused: a bomb
/// that fails its checksum has still been read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    spent: usize,
    cap: usize,
}

impl Budget {
    /// What is left, which is what a decode may be offered rather than what it
    /// would like. Offering a ceiling the total cannot cover would inflate
    /// bytes that are then refused, which is the work the cap exists to
    /// prevent being done anyway.
    pub(crate) fn remaining(&self) -> usize {
        self.cap.saturating_sub(self.spent)
    }

    pub(crate) fn spend(&mut self, permit: usize) -> Result<(), EntryError> {
        let after = self.spent.checked_add(permit);
        match after {
            Some(after) if after <= self.cap => {
                self.spent = after;
                Ok(())
            }
            _ => Err(EntryError::ArchiveBudgetSpent),
        }
    }
}

/// Warnings, deduplicated, in first-occurrence order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Warnings(Vec<Warning>);

impl Warnings {
    pub(crate) fn push(&mut self, w: Warning) {
        if !self.0.contains(&w) {
            self.0.push(w);
        }
    }
}

/// A ZIP archive's directory, read; its entries, not yet.
///
/// Opening walks the directory and decodes the names. It inflates nothing,
/// except in the recovery route where a streamed entry's extent cannot be
/// known any other way, so opening a 700 MB archive costs the directory and
/// not the archive.
#[derive(Clone, Debug)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    limits: Limits,
    entries: Vec<Entry>,
    route: Route,
    warnings: Warnings,
    budget: Budget,
}

impl<'a> Archive<'a> {
    /// Reads the directory.
    ///
    /// # Errors
    ///
    /// [`ArchiveError`], one variant per refusal, each of them by name.
    pub fn open(bytes: &'a [u8], limits: &Limits) -> Result<Self, ArchiveError> {
        let mut warnings = Warnings::default();
        let mut budget = Budget {
            spent: 0,
            cap: limits.max_inflated_total,
        };

        let (entries, route) = match dir::central_directory(bytes, limits, &mut warnings)? {
            Some(entries) => (entries, Route::CentralDirectory),
            None => {
                warnings.push(Warning::RecoveredFromLocalHeaders);
                let entries = scan::local_headers(bytes, limits, &mut warnings, &mut budget)?;
                if entries.is_empty() {
                    // Two different answers, and the difference is the whole
                    // point of refusing by name: a JPEG is not a damaged
                    // archive, and a truncated CBZ is not "not a PDF".
                    return Err(
                        if le::find(bytes, 0, &local::LOCAL_SIG).is_none()
                            && le::rfind(bytes, &local::EOCD_SIG).is_none()
                        {
                            ArchiveError::NotAZip
                        } else {
                            ArchiveError::Damaged
                        },
                    );
                }
                (entries, Route::LocalHeaderScan)
            }
        };

        Ok(Self {
            bytes,
            limits: *limits,
            entries,
            route,
            warnings,
            budget,
        })
    }

    /// The entries, in the archive's own order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Which route produced [`Archive::entries`].
    #[must_use]
    pub fn route(&self) -> Route {
        self.route
    }

    /// Everything tolerated so far, including by reads already made.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings.0
    }

    /// Bytes of [`Limits::max_inflated_total`] spent so far.
    ///
    /// Public because a bound nobody can measure is a bound nobody can check:
    /// this is what the ledger's "the most any fixture spends" column is read
    /// from, rather than asserted about in prose.
    #[must_use]
    pub fn inflated(&self) -> usize {
        self.budget.spent
    }

    /// One entry's **local** extra area (4.3.7), as bytes.
    ///
    /// `None` when there is no such entry or its local header is not where the
    /// directory said. An entry with no extra field answers `Some(&[])`, and
    /// the two are different sentences: "there is no header to ask" and "the
    /// header says none".
    ///
    /// # Why this is a method and not a field on [`Entry`]
    ///
    /// 4.4.11 lets the local extra area differ from the central directory's,
    /// and routinely it does — so a field on `Entry`, which is the directory's
    /// view, would answer a different question from the one asked. Filling one
    /// in from the local header instead would mean parsing every local header
    /// at [`Archive::open`], and this reader's whole posture is that opening a
    /// 700 MB archive costs the directory and not the archive.
    ///
    /// So it is computed on demand, for the entries a caller asks about. The
    /// caller that asks is EPUB 3.3 OCF §4.3.2, which requires the `mimetype`
    /// entry to be first, uncompressed and to carry **no extra field** — the
    /// three clauses together are what put the twenty bytes of
    /// `application/epub+zip` at a fixed offset in every conforming container.
    /// Nothing else in this workspace has ever needed to know.
    ///
    /// The area is handed back rather than its length, because a caller that
    /// wants to know it is empty asks `is_empty()` and one that wants to know
    /// *what* is in it — a Zip64 record, a timestamp, a Unix uid — has it.
    #[must_use]
    pub fn local_extra(&self, index: usize) -> Option<&'a [u8]> {
        let entry = self.entries.get(index)?;
        let at = le::offset(entry.header_offset, self.bytes.len())?;
        let header = local::parse_local(self.bytes, at)?;
        let start = header.data_offset.checked_sub(header.extra_len)?;
        self.bytes.get(start..header.data_offset)
    }

    /// Reads one entry, checked.
    ///
    /// A stored entry is handed back **borrowed** — it is a subslice of the
    /// archive, copied nowhere, which is what keeps gap 29's pass-through a
    /// pass-through. A deflated entry is inflated once, into an owned buffer.
    ///
    /// Both are checked the same way before they are returned: the produced
    /// length must be the declared length, and the CRC-32 must be the one the
    /// archive recorded. A mismatch **refuses the entry**; it does not warn
    /// and hand the bytes over.
    ///
    /// # Errors
    ///
    /// [`EntryError`], one variant per refusal.
    pub fn read(&mut self, index: usize) -> Result<Cow<'a, [u8]>, EntryError> {
        let bytes = self.bytes;
        let entry = self
            .entries
            .get(index)
            .ok_or(EntryError::NoSuchEntry)?
            .clone();

        if entry.encrypted {
            return Err(EntryError::Encrypted);
        }
        let declared_crc = entry.crc.ok_or(EntryError::NoChecksum)?;
        if let Method::Other(m) = entry.method {
            return Err(EntryError::UnsupportedMethod(m));
        }

        let at = le::offset(entry.header_offset, bytes.len()).ok_or(EntryError::LocalHeaderLost)?;
        let header = local::parse_local(bytes, at).ok_or(EntryError::LocalHeaderLost)?;
        if self.route == Route::CentralDirectory && header_disagrees(&header, &entry) {
            self.warnings.push(Warning::LocalHeaderDisagrees);
        }

        let data = match entry.method {
            Method::Stored => self.read_stored(&header, &entry)?,
            Method::Deflated => self.read_deflated(&header, &entry)?,
            Method::Other(m) => return Err(EntryError::UnsupportedMethod(m)),
        };

        // The two checks that make a pass-through honest, in the order that
        // reports the more specific failure: a length that is wrong says so,
        // rather than arriving as a checksum that does not match.
        let produced = data.len() as u64;
        if produced != entry.uncompressed_size {
            return Err(EntryError::SizeMismatch {
                declared: entry.uncompressed_size,
                produced,
            });
        }
        // Milestone 1 built `Crc32` resumable so an entry could be checksummed
        // *as* it inflated, and that is not what happens here, so the reason
        // belongs in the code rather than in a plan nobody reads next to it:
        // `inflate_raw` returns a finished `Vec`, not a stream of chunks, so
        // there is no interleaving to do without giving that function a
        // callback shape it does not have and no other caller wants. A stored
        // entry has no inflation to interleave with at all.
        //
        // So this is one pass over the finished bytes, and it is the only pass
        // anything in this engine makes over them — gap 29 hands the same
        // buffer to a PDF stream without re-reading it. The resumable form
        // stays worth having for the PNG decoder, whose chunks arrive
        // separately by construction.
        let computed = crc32(&data);
        if computed != declared_crc {
            return Err(EntryError::ChecksumMismatch {
                declared: declared_crc,
                computed,
            });
        }

        Ok(data)
    }

    /// Method 0: the data is a subslice, so there is nothing to inflate and
    /// nothing to charge. Stored bytes cannot expand and are already bounded
    /// by the archive's own length, which is the reason the total counts
    /// inflation rather than output.
    fn read_stored(
        &mut self,
        header: &local::LocalHeader,
        entry: &Entry,
    ) -> Result<Cow<'a, [u8]>, EntryError> {
        let n = usize::try_from(entry.compressed_size).map_err(|_| EntryError::Truncated)?;
        let end = header
            .data_offset
            .checked_add(n)
            .ok_or(EntryError::Truncated)?;
        let data = self
            .bytes
            .get(header.data_offset..end)
            .ok_or(EntryError::Truncated)?;
        if entry.streamed {
            self.check_descriptor(header.data_offset, n, entry, header.zip64);
        }
        Ok(Cow::Borrowed(data))
    }

    /// Method 8: raw DEFLATE, through the raw door.
    fn read_deflated(
        &mut self,
        header: &local::LocalHeader,
        entry: &Entry,
    ) -> Result<Cow<'a, [u8]>, EntryError> {
        // The declared size is **bounded before it is believed**. It is used
        // as the decode's ceiling and as the charge against the total, and
        // never as a length to allocate: a 4 GB declaration costs a refusal
        // here rather than 4 GB of `Vec`.
        let declared = usize::try_from(entry.uncompressed_size).unwrap_or(usize::MAX);
        if declared >= self.limits.max_entry_bytes {
            return Err(EntryError::EntryTooLarge);
        }
        self.budget.spend(declared)?;

        let tail = self
            .bytes
            .get(header.data_offset..)
            .ok_or(EntryError::Truncated)?;
        let r = inflate_raw(tail, &InflateLimits::new(declared));
        for w in &r.warnings {
            self.warnings.push(Warning::Inflate(*w));
        }
        if r.capped {
            return Err(EntryError::OversizedStream {
                declared: entry.uncompressed_size,
            });
        }
        if !r.complete {
            return Err(
                if r.warnings.contains(&InflateWarning::CorruptDeflateStream) {
                    EntryError::Corrupt
                } else {
                    EntryError::Truncated
                },
            );
        }

        // `end` is read **here and nowhere else**, because here is the only
        // place `complete` is known to be true. A capped or truncated decode
        // records where this decoder gave up rather than where the stream
        // ends, and a consumer that took it for a stream end would look for
        // the data descriptor inside the entry's own bytes.
        let consumed = r.end;
        if consumed as u64 != entry.compressed_size {
            self.warnings.push(Warning::CompressedSizeDisagrees);
        }
        if entry.streamed {
            self.check_descriptor(header.data_offset, consumed, entry, header.zip64);
        }
        Ok(Cow::Owned(r.data))
    }

    /// The descriptor an entry with general-purpose bit 3 carries after its
    /// data (APPNOTE 4.3.9), checked against the directory rather than read
    /// in place of it.
    fn check_descriptor(
        &mut self,
        data_offset: usize,
        consumed: usize,
        entry: &Entry,
        zip64: bool,
    ) {
        let Some(at) = data_offset.checked_add(consumed) else {
            return;
        };
        match local::parse_descriptor(self.bytes, at, consumed as u64, zip64) {
            Some(d)
                if Some(d.crc) == entry.crc
                    && d.uncompressed_size == entry.uncompressed_size
                    && d.compressed_size == entry.compressed_size => {}
            _ => self.warnings.push(Warning::DataDescriptorDisagrees),
        }
    }
}

/// Whether the local header contradicts the directory on anything that is not
/// allowed to differ.
///
/// The sizes and the CRC are compared **only when bit 3 is clear**, because
/// with it set the producer wrote zeros there on purpose and a comparison
/// would fire on every correctly written streamed entry in the world.
fn header_disagrees(header: &local::LocalHeader, entry: &Entry) -> bool {
    let method = match entry.method {
        Method::Stored => 0,
        Method::Deflated => 8,
        Method::Other(m) => m,
    };
    if header.method != method {
        return true;
    }
    if (header.flags & local::FLAG_STREAMED != 0) != entry.streamed {
        return true;
    }
    if (header.flags & local::FLAG_ENCRYPTED != 0) != entry.encrypted {
        return true;
    }
    if entry.streamed {
        return false;
    }
    Some(header.crc) != entry.crc
        || header.compressed_size != entry.compressed_size
        || header.uncompressed_size != entry.uncompressed_size
}

/// Decodes one entry name, under the rules general-purpose bit 11 sets.
pub(crate) fn decode_name(raw: &[u8], flags: u16, limits: &Limits, w: &mut Warnings) -> String {
    let mut name = if flags & local::FLAG_UTF8 != 0 {
        match std::str::from_utf8(raw) {
            Ok(s) => s.to_string(),
            Err(_) => {
                // The bit is a claim like every other field here. CP437 is
                // total over every byte, so there is always an answer, and it
                // is the answer the archive would have had without the claim.
                w.push(Warning::NameNotUtf8);
                cp437::decode(raw)
            }
        }
    } else {
        cp437::decode(raw)
    };

    if name.len() > limits.max_name_len {
        let mut cut = limits.max_name_len;
        while cut > 0 && !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name.truncate(cut);
        w.push(Warning::NameTruncated);
    }
    name
}
