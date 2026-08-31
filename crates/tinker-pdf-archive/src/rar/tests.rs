//! What the RAR 5 reader is held to, from the format's own header layout.
//!
//! Every fixture is built by [`archive`] below — a transcription of the header
//! chain, not a call into the parser — and every entry in one is **stored**,
//! because that is what the format's own compression decision produces for
//! page images and therefore all this build reads. `rar.rs`'s header argues
//! that line; what lives here is the container, which is fully checkable.
//!
//! The `.cbr` a real archiver wrote is opened in the facade
//! (`crates/tinker-pdf/tests/cbz_real.rs`), where its five pages are compared
//! against the same five pages a ZIP produced.

use super::*;

/// RAR 5's `vint`, written the way the format spells it.
fn vint_of(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// One header, with its own CRC-32 over exactly the bytes the format says.
fn header(kind: u64, flags: u64, body: &[u8], extra: &[u8], data: usize) -> Vec<u8> {
    let mut inner = Vec::new();
    inner.extend(vint_of(kind));
    inner.extend(vint_of(flags));
    if flags & HAS_EXTRA != 0 {
        inner.extend(vint_of(extra.len() as u64));
    }
    if flags & HAS_DATA != 0 {
        inner.extend(vint_of(data as u64));
    }
    inner.extend_from_slice(body);
    inner.extend_from_slice(extra);

    let mut covered = vint_of(inner.len() as u64);
    covered.extend_from_slice(&inner);
    let mut out = Vec::from(crc32(&covered).to_le_bytes());
    out.extend_from_slice(&covered);
    out
}

/// How a builder line is stored.
#[derive(Clone, Copy, PartialEq, Eq)]
enum As {
    Stored,
    /// A method 1-5 entry, whose data area is whatever the caller gave.
    Compressed(u8),
    Solid,
    Directory,
    Service,
    /// A stored entry whose recorded CRC-32 is deliberately wrong.
    BadCrc,
    /// A stored entry with no recorded CRC-32 at all.
    NoCrc,
    /// A stored entry with an encryption record in its extra area.
    Encrypted,
}

type Line<'a> = (&'a str, &'a [u8], As);

fn file_header(name: &str, data: &[u8], how: As) -> Vec<u8> {
    let directory = how == As::Directory;
    let mut file_flags = FILE_HAS_MTIME;
    if directory {
        file_flags |= FILE_DIRECTORY;
    }
    if !matches!(how, As::NoCrc) {
        file_flags |= FILE_HAS_CRC;
    }
    let method: u64 = match how {
        As::Compressed(m) => u64::from(m),
        _ => 0,
    };
    let solid: u64 = u64::from(how == As::Solid);
    // Version 0, solid at bit 6, method at bits 7-9, a 1 MB dictionary at
    // bits 10-13: the six fields RAR 5 packs into one integer.
    let compression = (solid << 6) | (method << 7);

    let mut body = Vec::new();
    body.extend(vint_of(file_flags));
    body.extend(vint_of(data.len() as u64));
    body.extend(vint_of(0x20)); // attributes
    body.extend_from_slice(&0x6512_3456u32.to_le_bytes()); // mtime
    if file_flags & FILE_HAS_CRC != 0 {
        let crc = if how == As::BadCrc {
            crc32(data) ^ 0xFFFF
        } else {
            crc32(data)
        };
        body.extend_from_slice(&crc.to_le_bytes());
    }
    body.extend(vint_of(compression));
    body.extend(vint_of(0)); // host OS
    body.extend(vint_of(name.len() as u64));
    body.extend_from_slice(name.as_bytes());

    let extra = if how == As::Encrypted {
        // One record: `size, type, data`, where the type is the encryption
        // record and the body is whatever a real one would carry.
        let mut record = vint_of(EXTRA_ENCRYPTION);
        record.extend_from_slice(&[0, 0x0F, 1]);
        let mut area = vint_of(record.len() as u64);
        area.extend_from_slice(&record);
        area
    } else {
        Vec::new()
    };

    let kind = if how == As::Service {
        TYPE_SERVICE
    } else {
        TYPE_FILE
    };
    let mut flags = HAS_DATA;
    if !extra.is_empty() {
        flags |= HAS_EXTRA;
    }
    let mut out = header(kind, flags, &body, &extra, data.len());
    out.extend_from_slice(data);
    out
}

/// A RAR 5 holding `files`, with a main header and an end-of-archive record.
fn archive(files: &[Line<'_>]) -> Vec<u8> {
    archive_with_flags(files, 0)
}

fn archive_with_flags(files: &[Line<'_>], archive_flags: u64) -> Vec<u8> {
    let mut out = Vec::from(SIGNATURE_5);
    out.extend(header(TYPE_MAIN, 0, &vint_of(archive_flags), &[], 0));
    for (name, data, how) in files {
        let data: &[u8] = if *how == As::Directory { b"" } else { data };
        out.extend(file_header(name, data, *how));
    }
    out.extend(header(TYPE_END, 0, &vint_of(0), &[], 0));
    out
}

fn open(bytes: &[u8]) -> Archive<'_> {
    Archive::open(bytes, &Limits::DEFAULT).expect("the archive opens")
}

// ---- The container ----------------------------------------------------------

/// **A stored RAR lists its files and hands their bytes back borrowed.**
///
/// The borrow is asserted by address, not by type: `read` returns a `Cow`, so
/// the compiler is happy either way and only the pointer says which arm it
/// took. A stored RAR entry is a contiguous range of the input, and the comic
/// path places image bytes into a PDF stream verbatim — so a copy per entry
/// would be a copy of the whole archive.
#[test]
fn a_stored_rar_lists_its_files_and_hands_their_bytes_back_borrowed() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"the first page", As::Stored),
        ("page10.png", b"the tenth", As::Stored),
        (
            "page2.png",
            b"the second page, longer than the others",
            As::Stored,
        ),
    ];
    let bytes = archive(files);
    let a = open(&bytes);

    let names: Vec<&str> = a.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["page1.png", "page10.png", "page2.png"]);
    assert_eq!(
        a.entries().iter().map(|e| e.size).collect::<Vec<_>>(),
        [14, 9, 39]
    );
    assert!(a.entries().iter().all(|e| e.crc.is_some()));
    assert!(a.entries().iter().all(|e| e.method == 0));

    let base = bytes.as_ptr() as usize;
    for (index, (_, want, _)) in files.iter().enumerate() {
        let got = a
            .read(index)
            .unwrap_or_else(|e| panic!("entry {index}: {e}"));
        assert_eq!(got.as_ref(), *want, "entry {index}");
        assert!(
            matches!(got, Cow::Borrowed(_)),
            "entry {index} was copied rather than borrowed"
        );
        let at = got.as_ptr() as usize;
        assert!(
            at >= base && at + got.len() <= base + bytes.len(),
            "entry {index}'s bytes are not inside the archive"
        );
    }
    assert_eq!(
        a.warnings(),
        &[],
        "a well-formed archive warns about nothing"
    );
}

/// **A RAR 4 archive is recognised and refused by its own name.**
///
/// The two signatures differ in one byte, and the difference is the whole of
/// the decision: `Rar!\x1A\x07\x00` is RAR 4 and `Rar!\x1A\x07\x01\x00` is
/// RAR 5. Refusing the first as "not a RAR" would be false, and refusing it as
/// [`Error::NotARar`] is what a reader that only compared the first four bytes
/// would do.
#[test]
fn a_rar_4_archive_is_recognised_and_refused_by_its_own_name() {
    let mut rar4 = Vec::from(SIGNATURE_4);
    rar4.extend_from_slice(&[0u8; 64]);
    assert_eq!(
        Archive::open(&rar4, &Limits::DEFAULT).err(),
        Some(Error::Rar4)
    );
    assert!(Error::Rar4.to_string().contains("RAR 4"));

    assert_eq!(
        Archive::open(b"not a RAR at all", &Limits::DEFAULT).err(),
        Some(Error::NotARar)
    );
    assert_eq!(
        Archive::open(SIGNATURE_5, &Limits::DEFAULT)
            .expect("a signature and nothing else still opens")
            .entries(),
        &[],
        "an archive with no headers is empty rather than damaged"
    );
}

/// A header that does not checksum ends the walk and keeps what came before.
///
/// A RAR is a chain in which every header says where the next one is, so a
/// header that does not check is the point past which nothing can be located.
/// The **first** one is an [`Error`] because there is then no chain at all,
/// and any later one is a [`Warning`] because everything before it is still an
/// answer (ruling 2).
#[test]
fn a_header_that_does_not_checksum_ends_the_walk_and_keeps_what_came_before() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"the first page", As::Stored),
        ("page2.png", b"the second page", As::Stored),
    ];
    let good = archive(files);
    // Find the second file header by its name and damage the byte after its
    // CRC, which is inside the CRC's coverage and outside the data.
    let at = good
        .windows(9)
        .position(|w| w == b"page2.png")
        .expect("the second name");
    let mut bytes = good.clone();
    bytes[at] ^= 0xFF;
    let a = open(&bytes);
    assert_eq!(a.entries().len(), 1, "the entry before the damage survives");
    assert_eq!(a.entries()[0].name, "page1.png");
    assert!(
        a.warnings()
            .contains(&Warning::HeaderChecksumFailed { index: 1 }),
        "and the walk says where it stopped: {:?}",
        a.warnings()
    );

    // The first header, damaged: there is no chain to follow at all.
    let mut bytes = good.clone();
    bytes[SIGNATURE_5.len() + 5] ^= 0xFF;
    assert_eq!(
        Archive::open(&bytes, &Limits::DEFAULT).err(),
        Some(Error::FirstHeaderCorrupt)
    );
}

/// **An entry whose recorded CRC-32 does not match is refused, not returned.**
///
/// The check that adjudicates the extraction, and the one that makes a
/// decompressor addable later without a second implementation to disagree
/// with.
#[test]
fn an_entry_whose_recorded_crc_does_not_match_is_refused_rather_than_returned() {
    let bytes = archive(&[
        ("page1.png", b"the first page", As::BadCrc),
        ("page2.png", b"the second page", As::Stored),
    ]);
    let a = open(&bytes);
    assert_eq!(a.read(0), Err(EntryError::CrcMismatch));
    assert_eq!(
        a.read(1).as_deref(),
        Ok(&b"the second page"[..]),
        "one bad page is not a lost archive"
    );
}

/// An entry with no recorded CRC-32 is **warned about**, because of what the
/// missing check costs: it is handed over unadjudicated.
#[test]
fn an_entry_with_no_recorded_crc_is_warned_about() {
    let bytes = archive(&[("page1.png", b"the first page", As::NoCrc)]);
    let a = open(&bytes);
    assert_eq!(a.entries()[0].crc, None);
    assert!(a.warnings().contains(&Warning::NoCrcRecorded { index: 0 }));
    assert_eq!(
        a.read(0).as_deref(),
        Ok(&b"the first page"[..]),
        "and it still reads: a missing checksum is not a refusal"
    );
}

/// A compressed entry is refused **by its method number**, one page at a time.
///
/// The refusal is at the entry rather than the archive, so a `.cbr` that mixes
/// stored and compressed entries pages the stored ones and puts a placeholder
/// where the others are (ruling 2). The method number is carried because it is
/// what a host would put in front of a user: methods 1 to 5 are one algorithm
/// at five efforts, and knowing which changes nothing a user can do — but
/// knowing it is *compression* rather than *encryption* changes everything.
#[test]
fn a_compressed_entry_is_refused_by_its_method_number() {
    for method in 1..=5u8 {
        let bytes = archive(&[
            (
                "page1.png",
                b"pretend this is compressed",
                As::Compressed(method),
            ),
            ("page2.png", b"the second page", As::Stored),
        ]);
        let a = open(&bytes);
        assert_eq!(
            a.entries()[0].method,
            method,
            "method {method} is read back"
        );
        assert_eq!(a.read(0), Err(EntryError::Compressed { method }));
        assert_eq!(
            a.read(1).as_deref(),
            Ok(&b"the second page"[..]),
            "the stored entry beside it still pages"
        );
    }
    // Solid is a different refusal from compressed, and is checked first: a
    // solid *stored* entry is a thing the format allows and this build cannot
    // read either, and calling it "compressed with method 0" would be false.
    let bytes = archive(&[("page1.png", b"a page", As::Solid)]);
    let a = open(&bytes);
    assert!(a.entries()[0].solid);
    assert_eq!(a.read(0), Err(EntryError::Solid));
}

/// Multi-volume and encrypted archives are refused by name at open.
#[test]
fn multi_volume_and_encrypted_archives_are_refused_by_name() {
    let files: &[Line<'_>] = &[("page1.png", b"a page", As::Stored)];
    assert_eq!(
        Archive::open(&archive_with_flags(files, ARCHIVE_VOLUME), &Limits::DEFAULT).err(),
        Some(Error::MultiVolume)
    );
    assert_eq!(
        Archive::open(
            &archive_with_flags(files, ARCHIVE_HAS_VOLUME_NUMBER),
            &Limits::DEFAULT
        )
        .err(),
        Some(Error::MultiVolume),
        "a volume number is as good as the volume flag"
    );

    let mut encrypted = Vec::from(SIGNATURE_5);
    encrypted.extend(header(TYPE_ARCHIVE_ENCRYPTION, 0, &vint_of(0), &[], 0));
    assert_eq!(
        Archive::open(&encrypted, &Limits::DEFAULT).err(),
        Some(Error::Encrypted)
    );

    // A per-entry encryption record: the archive opens and the entry refuses,
    // because the rest of the archive is still readable.
    let bytes = archive(&[
        ("page1.png", b"ciphertext", As::Encrypted),
        ("page2.png", b"the second page", As::Stored),
    ]);
    let a = open(&bytes);
    assert!(a.entries()[0].encrypted);
    assert_eq!(a.read(0), Err(EntryError::Encrypted));
    assert_eq!(a.read(1).as_deref(), Ok(&b"the second page"[..]));
}

/// Directories and service records are listed, so the count stays honest, and
/// neither is read as a page.
#[test]
fn directories_and_service_records_are_listed_and_not_read() {
    let bytes = archive(&[
        ("pages", b"", As::Directory),
        ("pages/page1.png", b"a page", As::Stored),
        ("CMT", b"a comment", As::Service),
    ]);
    let a = open(&bytes);
    assert_eq!(
        a.entries().iter().map(|e| e.kind).collect::<Vec<_>>(),
        [Kind::Directory, Kind::File, Kind::Service]
    );
    assert!(a.entries()[0].is_directory());
    assert!(
        a.entries()[2].is_directory(),
        "a service record is not a page"
    );
    assert_eq!(a.read(0), Err(EntryError::NotAFile));
    assert_eq!(a.read(2), Err(EntryError::NotAFile));
    assert_eq!(a.read(1).as_deref(), Ok(&b"a page"[..]));
    assert_eq!(a.read(3), Err(EntryError::NoSuchEntry));
}

/// A `\` in a stored path becomes a `/`, because page order is decided by the
/// name and RAR records whichever separator the packing machine used.
#[test]
fn a_windows_separator_in_a_name_becomes_a_forward_slash() {
    let bytes = archive(&[("chapter1\\page1.png", b"a page", As::Stored)]);
    let a = open(&bytes);
    assert_eq!(a.entries()[0].name, "chapter1/page1.png");
}

/// RAR 5's `vint` decodes the way the format spells it, at every width, and a
/// `vint` that never terminates is refused rather than walked forever.
#[test]
fn the_vint_decodes_the_way_the_format_spells_it() {
    for value in [
        0u64,
        1,
        0x7F,
        0x80,
        0x3FFF,
        0x4000,
        0x1F_FFFF,
        0x0FFF_FFFF,
        0x07_FFFF_FFFF,
        u64::MAX / 2,
        u64::MAX,
    ] {
        let encoded = vint_of(value);
        let mut at = 0usize;
        assert_eq!(vint(&encoded, &mut at), Some(value), "{value:#x}");
        assert_eq!(at, encoded.len(), "{value:#x} consumed its whole encoding");
    }
    // Ten bytes is `ceil(64 / 7)`; an eleventh means the file is spending the
    // walk's time rather than describing anything.
    let mut at = 0usize;
    assert_eq!(vint(&[0x80u8; 16], &mut at), None, "a vint that never ends");
    let mut at = 0usize;
    assert_eq!(vint(&[0x80, 0x80], &mut at), None, "a vint cut short");
}

/// A name past the cap is truncated and says so; a name that is not UTF-8
/// decodes lossily and says so.
///
/// Both are warnings rather than refusals: the name lives inside a header
/// whose length is already known, so a damaged name costs the name and not the
/// page — and a comic whose ninth page has a broken character in its name
/// still has nine pages.
#[test]
fn a_damaged_name_costs_the_name_and_not_the_page() {
    let long = "a".repeat(80);
    let bytes = archive(&[(long.as_str(), b"a page", As::Stored)]);
    let limits = Limits {
        max_name_len: 16,
        ..Limits::DEFAULT
    };
    let a = Archive::open(&bytes, &limits).expect("it opens");
    assert!(a.warnings().contains(&Warning::NameTruncated { index: 0 }));
    assert_eq!(a.entries()[0].name, "a".repeat(16));
    assert_eq!(
        a.read(0).as_deref(),
        Ok(&b"a page"[..]),
        "and it still reads"
    );

    // A name that is not UTF-8. The builder takes a `&str`, so the bytes are
    // damaged in the finished archive and the header CRC rebuilt over them.
    let mut bytes = archive(&[("page1.png", b"a page", As::Stored)]);
    let at = bytes
        .windows(9)
        .position(|w| w == b"page1.png")
        .expect("the name");
    bytes[at + 4] = 0xFF;
    // The header this name is in runs from its CRC to the start of the data.
    let header_at = bytes
        .windows(4)
        .position(|w| w == b"\x00\x00\x00\x00")
        .unwrap_or(0);
    let _ = header_at;
    // Rebuild the whole archive around the damaged name instead, which is
    // simpler than re-checksumming in place and is what a producer would do.
    let mut rebuilt = Vec::from(SIGNATURE_5);
    rebuilt.extend(header(TYPE_MAIN, 0, &vint_of(0), &[], 0));
    let mut body = Vec::new();
    body.extend(vint_of(FILE_HAS_CRC));
    body.extend(vint_of(6));
    body.extend(vint_of(0x20));
    body.extend_from_slice(&crc32(b"a page").to_le_bytes());
    body.extend(vint_of(0));
    body.extend(vint_of(0));
    body.extend(vint_of(9));
    body.extend_from_slice(b"page\xFF.png");
    let mut file = header(TYPE_FILE, HAS_DATA, &body, &[], 6);
    file.extend_from_slice(b"a page");
    rebuilt.extend_from_slice(&file);
    rebuilt.extend(header(TYPE_END, 0, &vint_of(0), &[], 0));

    let a = open(&rebuilt);
    assert!(
        a.warnings().contains(&Warning::NameNotUtf8 { index: 0 }),
        "{:?}",
        a.warnings()
    );
    assert_eq!(a.read(0).as_deref(), Ok(&b"a page"[..]));
}

/// Each cap is refused by name, and each can actually fire.
#[test]
fn every_cap_is_refused_by_name_and_can_actually_fire() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"one", As::Stored),
        ("page2.png", b"two", As::Stored),
        ("page3.png", b"three", As::Stored),
    ];
    let bytes = archive(files);
    let tight = Limits {
        max_entries: 2,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &tight).err(),
        Some(Error::TooManyEntries)
    );

    // A header larger than the cap is refused **before its CRC is computed over
    // it**, which is the difference between a cap and a check. Which refusal it
    // is depends on which header is too big, and both halves are asserted
    // because the first draft of this test asserted the wrong one: a main
    // archive header is three bytes and a file header is thirty-odd, so a cap
    // between them lets the walk start and stops it at the first file.
    let stops_at_the_first_file = Limits {
        max_header_bytes: 4,
        ..Limits::DEFAULT
    };
    let a = Archive::open(&bytes, &stops_at_the_first_file).expect("the main header fits");
    assert_eq!(
        a.entries(),
        &[],
        "no file header fits, so there are no entries"
    );
    assert!(
        a.warnings()
            .contains(&Warning::HeaderChecksumFailed { index: 0 }),
        "and the walk says where it stopped: {:?}",
        a.warnings()
    );

    let small = Limits {
        max_header_bytes: 2,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &small).err(),
        Some(Error::FirstHeaderCorrupt),
        "a cap below even the main header is the same answer as a first header \
         that will not checksum: neither can be walked past"
    );
}

/// An entry whose declared size and packed size disagree is refused.
///
/// A stored entry's two sizes are the same number by definition, so a header
/// that says otherwise is describing something this build would otherwise hand
/// back short — and the caller's `/Length` came from the declared one.
#[test]
fn a_stored_entry_whose_two_sizes_disagree_is_refused() {
    let mut rebuilt = Vec::from(SIGNATURE_5);
    rebuilt.extend(header(TYPE_MAIN, 0, &vint_of(0), &[], 0));
    let mut body = Vec::new();
    body.extend(vint_of(FILE_HAS_CRC));
    body.extend(vint_of(999)); // an unpacked size the data area cannot hold
    body.extend(vint_of(0x20));
    body.extend_from_slice(&crc32(b"a page").to_le_bytes());
    body.extend(vint_of(0));
    body.extend(vint_of(0));
    body.extend(vint_of(9));
    body.extend_from_slice(b"page1.png");
    let mut file = header(TYPE_FILE, HAS_DATA, &body, &[], 6);
    file.extend_from_slice(b"a page");
    rebuilt.extend_from_slice(&file);
    rebuilt.extend(header(TYPE_END, 0, &vint_of(0), &[], 0));

    let a = open(&rebuilt);
    assert_eq!(a.entries()[0].size, 999);
    assert_eq!(a.read(0), Err(EntryError::Truncated));
}

/// **Hostile headers produce answers rather than panics** (ruling 1).
///
/// One byte of a good archive corrupted at a time, across the whole file,
/// which reaches every `vint`, every flag and every length — the bounds a
/// fuzzer needs a corpus to find.
#[test]
fn hostile_headers_produce_answers_rather_than_panics() {
    let good = archive(&[
        ("pages", b"", As::Directory),
        ("page1.png", b"the first page", As::Stored),
        ("page2.png", b"the second", As::Compressed(3)),
        ("CMT", b"a comment", As::Service),
    ]);
    let mut answered = 0usize;
    let mut opened = 0usize;
    for at in 0..good.len() {
        for mask in [0x01u8, 0x80, 0xFF] {
            let mut bytes = good.clone();
            bytes[at] ^= mask;
            if let Ok(a) = Archive::open(&bytes, &Limits::DEFAULT) {
                opened += 1;
                for index in 0..a.entries().len().min(16) {
                    let _ = a.read(index);
                }
            }
            answered += 1;
        }
    }
    assert_eq!(answered, good.len() * 3, "every corruption returned");
    assert!(
        opened > 0,
        "no corruption of any byte left a readable archive, so this test only \
         ever exercised the refusal path"
    );

    for bytes in [
        vec![],
        Vec::from(SIGNATURE_5),
        Vec::from(SIGNATURE_4),
        [SIGNATURE_5, &[0xFFu8; 32][..]].concat(),
        [SIGNATURE_5, &[0x80u8; 64][..]].concat(),
    ] {
        let _ = Archive::open(&bytes, &Limits::DEFAULT);
    }
}

/// Writes the seeds `fuzz/corpus/rar/` carries, so the seeds and the fixtures
/// here cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
#[test]
#[ignore = "writes into fuzz/corpus/rar, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/rar");
    std::fs::create_dir_all(&base).expect("the corpus directory");

    let stored = archive(&[
        ("page1.png", b"the first page", As::Stored),
        ("page10.png", b"the tenth", As::Stored),
        ("page2.png", b"the second page", As::Stored),
    ]);
    let kinds = archive(&[
        ("pages", b"", As::Directory),
        ("pages/page1.png", b"a page", As::Stored),
        ("CMT", b"a comment", As::Service),
    ]);
    let methods = archive(&[
        (
            "page1.png",
            b"pretend this is compressed",
            As::Compressed(3),
        ),
        ("page2.png", b"a page", As::Solid),
        ("page3.png", b"ciphertext", As::Encrypted),
    ]);
    let bad_crc = archive(&[("page1.png", b"the first page", As::BadCrc)]);
    let mut rar4 = Vec::from(SIGNATURE_4);
    rar4.extend_from_slice(&[0u8; 48]);

    for (name, bytes) in [
        ("stored", [&[0xFFu8][..], &stored].concat()),
        ("kinds", [&[0xFF][..], &kinds].concat()),
        ("methods", [&[0xFF][..], &methods].concat()),
        ("crc-mismatch", [&[0xFF][..], &bad_crc].concat()),
        ("rar4-signature", [&[0xFF][..], &rar4].concat()),
        ("stored-tight", [&[0x00][..], &stored].concat()),
        ("kinds-tight", [&[0x00][..], &kinds].concat()),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }
}
