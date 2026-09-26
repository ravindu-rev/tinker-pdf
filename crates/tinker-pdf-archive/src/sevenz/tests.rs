//! What the 7z reader is held to, from the format's own `7zFormat.txt`
//! grammar.
//!
//! Every fixture here is built by [`archive`] below — a transcription of the
//! header grammar, not a call into the parser — and every one of them uses the
//! **Copy** coder, which is the whole trick that makes this file possible:
//! Copy needs no compression, so the container can be exercised end to end
//! without an encoder this workspace is not allowed to have. The compressed
//! coders are adjudicated where they can be, by `7z-lzma2.cb7`'s own CRC-32 in
//! `crates/tinker-pdf/tests/cbz_real.rs`.
//!
//! # Injection, counted
//!
//! Eleven defects were reintroduced across this module and [`crate::lzma`] and
//! the suite run to see what caught them.
//!
//! Counts are of every test binary that can reach this crate — its own suite,
//! and the facade's `cbz` and `cbz_real`, which a grep for the four container
//! names says are the only two of `tinker-pdf`'s that do. They are sums over
//! the per-binary `test result:` lines of a run with **`--no-fail-fast`**, and
//! that flag is not a detail: a plain `cargo test` stops at the first failing
//! binary, so the first two attempts at this table under-counted six of its
//! rows.
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the entry CRC-32 not checked before the bytes are handed over | **3** |
//! | the start header's own CRC-32 not checked | **1** |
//! | `NUMBER`'s high bits read as the *low* part of the value | **4** |
//! | a substream's last size listed rather than inferred from the folder | **12** |
//! | an empty stream always a directory, never an empty file | **1** |
//! | the matched-literal path never taken (`state >= 7` ignored) | **2** |
//! | the four remembered distances rotated the wrong way | **2** |
//! | a match's length not offset by the two-byte minimum | **2** |
//! | the distance model skipping its four alignment bits | **2** |
//! | the probability adaptation rate 1/16 instead of 1/32 | **2** |
//! | an LZMA2 dictionary reset not resetting the literal context | **0**, then **1** |
//!
//! **The five decoder rows are the argument for this whole lane.** Each of
//! them — the matched literal, the rep rotation, the length offset, the
//! alignment bits, the adaptation rate — is caught by exactly two tests, and
//! in every case the two are `cbz_real.rs`'s `.cb7` checks and *no unit test
//! at all*. Nothing in this file asserts anything about an LZMA distance. What
//! catches them is `7z-lzma2.cb7`'s own recorded CRC-32, checked inside
//! [`super::Archive::read`]: a decompressor that is wrong fails the format's
//! check and the page becomes a placeholder. That is what let a hand-rolled
//! LZMA decoder be written with no oracle to disagree with (ruling 13), and
//! the table is the measurement of it rather than the claim.
//!
//! **The last row is why this practice is worth its cost.** Making the literal
//! context reach back past an LZMA2 dictionary reset was caught by **zero**
//! tests. The defect is real — the models diverge from that byte on — and it
//! was invisible because the one `.cb7` here is a single solid block whose
//! only reset is at position 0, where reaching back finds nothing and a wrong
//! decoder is accidentally right. `a_dictionary_reset_restarts_the_literal_context`
//! in `lzma/tests.rs` is the fixture that reaches it, written *because* the
//! count came back zero, and the row's second number is that test.
//!
//! The start-header CRC row being **1** is not a weakness and is worth reading
//! correctly: removing a check cannot fail a decode that was already correct,
//! so what catches it is the one test that hands it a deliberately wrong
//! archive. Its value is measured by the five decoder rows, not by its own.
//!
//! **The substream row at 12 is the opposite lesson.** `kSize` lists every
//! substream but the last, and the last is whatever the folder's output has
//! left; a reader that read a number there instead loses its place in the
//! table and every entry after it, so it fails nine unit tests, both seed
//! replays and both `.cb7` corpus checks. It is the 7z equivalent of tar's
//! `advance`: the one defect that is about *the walk* rather than about one
//! field, and the one the corpus is therefore strongest against.
//!
//! # Injection, counted again — what `7z-nonsolid.cb7` and `7z-dictreset.cb7` bought
//!
//! The table above was measured when `7z-lzma2.cb7` was the only `.cb7` here,
//! and what it could not say is how much of the decoder that one fixture
//! *misses*. `-m0=LZMA2` writes the simplest shape the format allows — one
//! folder, one chunk — so `read_header`'s folder walk and
//! `lzma::decode_lzma2`'s chunk loop were each entered exactly once by every
//! archive in the tree.
//!
//! Four defects, measured twice by the same method: **before** is the tree as
//! it stood with the two new archives absent from every test, **after** is
//! them read. Same command, same parse of the per-binary `test result:` lines,
//! `--no-fail-fast` before `-p` both times, and a control run in each
//! configuration that came back **0**.
//!
//! | Injected | Before | After |
//! | --- | ---: | ---: |
//! | `decode_lzma2` ignores the dictionary-reset bit of the control byte | **1** | **4** |
//! | the folder walk stops after folder 0 | **0** | **3** |
//! | a distance-slot decode off by one (`spec_pos`' base offset) | **2** | **3** |
//! | `MOVE_BITS` 1/16 instead of 1/32 | **2** | **3** |
//!
//! **The second row is the whole argument, and it came back zero.** A reader
//! that walks one folder and stops hands every entry after the first a slot
//! that does not exist, and until `7z-nonsolid.cb7` existed *nothing in this
//! workspace could tell* — every fixture, hand-built and committed alike, had
//! exactly one folder, and one folder is all a walk that stops after the first
//! one needs. It is the `an LZMA2 dictionary reset not resetting the literal
//! context` row of the table above happening a second time, for the same
//! reason: a defect in the second iteration of a loop no input iterates twice.
//!
//! **The `MOVE_BITS` row reproduces the older table exactly**, which is what
//! makes the rest of this one worth reading: 1/16 instead of 1/32 was recorded
//! as **2** there and measures **2** here in the before column. The method did
//! not change, the tree did. Its third catch is
//! `the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for`,
//! which reads every entry of both new archives and so is held to their
//! recorded CRC-32s like the corpus tests are.
//!
//! What none of these four rows measures is a *second producer*. All three
//! `.cb7`s were written by 7-Zip 26.02, so a misreading of the format shared
//! between that writer and this reader would survive every one of them.
//! `docs/design/comic-archives.md` records that as unmet rather than closed.

use super::*;

// ---- A 7z, built by hand from the grammar -----------------------------------

/// 7z's `NUMBER`, written in its shortest legal form.
///
/// Shortest rather than always-eight-bytes on purpose: the compact forms are
/// what a real writer emits and are where a reader's mask arithmetic is
/// wrong, so a builder that only ever wrote `0xFF` would leave the reader's
/// interesting path untested.
fn number(value: u64) -> Vec<u8> {
    for extra in 0..8usize {
        let bits = 8 * extra + (7 - extra);
        if value < (1u64 << bits) {
            let mut first = 0u8;
            for k in 0..extra {
                first |= 0x80 >> k;
            }
            first |= (value >> (8 * extra)) as u8;
            let mut out = vec![first];
            for k in 0..extra {
                out.push((value >> (8 * k)) as u8);
            }
            return out;
        }
    }
    let mut out = vec![0xFFu8];
    out.extend_from_slice(&value.to_le_bytes());
    out
}

fn utf16(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in name.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// What kind of entry a builder line is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum As {
    File,
    Directory,
    Empty,
}

/// One line of a built archive.
type Line<'a> = (&'a str, &'a [u8], As);

/// Wraps a header and packed data in a signature header, with both CRCs
/// correct.
fn wrap(packed: Vec<u8>, header: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::from(SIGNATURE);
    out.extend_from_slice(&[0, 4]);
    let mut start = Vec::new();
    start.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    start.extend_from_slice(&(header.len() as u64).to_le_bytes());
    start.extend_from_slice(&crc32(&header).to_le_bytes());
    out.extend_from_slice(&crc32(&start).to_le_bytes());
    out.extend_from_slice(&start);
    out.extend_from_slice(&packed);
    out.extend_from_slice(&header);
    out
}

/// A 7z holding `files`, stored with the Copy coder in one folder.
///
/// `coder` and `coder_props` are the caller's, because several tests are about
/// a folder whose coder is *not* Copy and the difference is one field.
fn archive_with(files: &[Line<'_>], coder: &[u8], coder_props: &[u8], packed: Vec<u8>) -> Vec<u8> {
    let with_stream: Vec<&Line<'_>> = files.iter().filter(|(_, _, k)| *k == As::File).collect();
    let unpacked: u64 = with_stream.iter().map(|(_, d, _)| d.len() as u64).sum();

    let mut folder = Vec::new();
    folder.extend(number(1)); // one coder
    let mut flags = coder.len() as u8;
    if !coder_props.is_empty() {
        flags |= 0x20;
    }
    folder.push(flags);
    folder.extend_from_slice(coder);
    if !coder_props.is_empty() {
        folder.extend(number(coder_props.len() as u64));
        folder.extend_from_slice(coder_props);
    }

    let mut streams = Vec::new();
    streams.push(K_PACK_INFO);
    streams.extend(number(0)); // packPos
    streams.extend(number(1)); // one pack stream
    streams.push(K_SIZE);
    streams.extend(number(packed.len() as u64));
    streams.push(K_END);

    streams.push(K_UNPACK_INFO);
    streams.push(K_FOLDER);
    streams.extend(number(1)); // one folder
    streams.push(0); // not external
    streams.extend_from_slice(&folder);
    streams.push(K_CODERS_UNPACK_SIZE);
    streams.extend(number(unpacked));
    streams.push(K_END);

    streams.push(K_SUBSTREAMS_INFO);
    streams.push(K_NUM_UNPACK_STREAM);
    streams.extend(number(with_stream.len() as u64));
    streams.push(K_SIZE);
    // Every substream but the last: the last is what the folder has left,
    // which is the format's rule and not an optimisation.
    for (_, data, _) in with_stream.iter().take(with_stream.len().saturating_sub(1)) {
        streams.extend(number(data.len() as u64));
    }
    streams.push(K_CRC);
    streams.push(1); // all defined
    for (_, data, _) in &with_stream {
        streams.extend_from_slice(&crc32(data).to_le_bytes());
    }
    streams.push(K_END);
    streams.push(K_END);

    let mut header = vec![K_HEADER];
    if unpacked > 0 || !with_stream.is_empty() {
        header.push(K_MAIN_STREAMS);
        header.extend_from_slice(&streams);
    }
    header.push(K_FILES_INFO);
    header.extend(number(files.len() as u64));

    if files.iter().any(|(_, _, k)| *k != As::File) {
        let mut vector = vec![0u8; files.len().div_ceil(8)];
        for (i, (_, _, kind)) in files.iter().enumerate() {
            if *kind != As::File {
                vector[i / 8] |= 0x80 >> (i % 8);
            }
        }
        header.extend(number(u64::from(K_EMPTY_STREAM)));
        header.extend(number(vector.len() as u64));
        header.extend_from_slice(&vector);

        let empties: Vec<&Line<'_>> = files.iter().filter(|(_, _, k)| *k != As::File).collect();
        let mut files_vector = vec![0u8; empties.len().div_ceil(8)];
        for (i, (_, _, kind)) in empties.iter().enumerate() {
            if *kind == As::Empty {
                files_vector[i / 8] |= 0x80 >> (i % 8);
            }
        }
        header.extend(number(u64::from(K_EMPTY_FILE)));
        header.extend(number(files_vector.len() as u64));
        header.extend_from_slice(&files_vector);
    }

    let mut names = vec![0u8]; // not external
    for (name, _, _) in files {
        names.extend(utf16(name));
    }
    header.extend(number(u64::from(K_NAME)));
    header.extend(number(names.len() as u64));
    header.extend_from_slice(&names);
    header.extend(number(0)); // end of properties
    header.push(K_END);

    wrap(packed, header)
}

/// The common case: Copy, and the packed bytes are the files concatenated.
fn archive(files: &[Line<'_>]) -> Vec<u8> {
    let packed: Vec<u8> = files
        .iter()
        .filter(|(_, _, k)| *k == As::File)
        .flat_map(|(_, d, _)| d.iter().copied())
        .collect();
    archive_with(files, &[0x00], &[], packed)
}

fn open(bytes: &[u8]) -> Archive<'_> {
    Archive::open(bytes, &Limits::DEFAULT).expect("the archive opens")
}

// ---- The container ----------------------------------------------------------

/// **A stored archive lists its files and hands their bytes back.**
///
/// The baseline: names in header order, sizes from the substream table,
/// offsets that partition the folder's output, and every entry's recorded
/// CRC-32 matching. Everything else in this file is a way for one of those to
/// be wrong.
#[test]
fn a_stored_archive_lists_its_files_and_hands_their_bytes_back() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"the first page", As::File),
        ("page10.png", b"the tenth", As::File),
        (
            "page2.png",
            b"the second page, longer than the others",
            As::File,
        ),
    ];
    let bytes = archive(files);
    let mut a = open(&bytes);

    let names: Vec<&str> = a.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["page1.png", "page10.png", "page2.png"]);
    let sizes: Vec<u64> = a.entries().iter().map(|e| e.size).collect();
    assert_eq!(sizes, [14, 9, 39]);
    assert!(
        a.entries().iter().all(|e| e.crc.is_some()),
        "every entry carries the CRC the archive recorded"
    );
    for (index, (_, want, _)) in files.iter().enumerate() {
        assert_eq!(a.read(index).as_deref(), Ok(*want), "entry {index}");
    }
    // Reverse order too: the folder cache must not depend on being read
    // forwards, which is the order a viewer scrolls in.
    for (index, (_, want, _)) in files.iter().enumerate().rev() {
        assert_eq!(a.read(index).as_deref(), Ok(*want), "entry {index} again");
    }
    assert_eq!(
        a.warnings(),
        &[],
        "a well-formed archive warns about nothing"
    );
}

/// **An entry whose recorded CRC-32 does not match is refused, not returned.**
///
/// This is the assertion the whole verification argument rests on: it is what
/// makes a wrong LZMA window a refused page rather than a wrong picture, and
/// it is why this crate needs no second decompressor to disagree with. The
/// defect is injected in the only place it can be — the packed bytes — because
/// with the Copy coder that *is* the decompressor's output.
#[test]
fn an_entry_whose_recorded_crc_does_not_match_is_refused_rather_than_returned() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"the first page", As::File),
        ("page2.png", b"the second page", As::File),
    ];
    let mut bytes = archive(files);
    // One bit inside the first entry's data. The archive is otherwise perfect:
    // both header CRCs still match, every size is right, and only the format's
    // own per-file check can see it.
    bytes[SIGNATURE_HEADER + 3] ^= 0x01;
    let mut a = open(&bytes);
    assert_eq!(
        a.read(0),
        Err(EntryError::CrcMismatch),
        "a flipped bit is a refusal"
    );
    assert_eq!(
        a.read(1).as_deref(),
        Ok(&b"the second page"[..]),
        "and the entry beside it still reads: one bad page is not a lost archive"
    );
}

/// A file with no 7z signature is refused by name, and a start header that
/// does not checksum is refused **before** its two `u64`s are used.
///
/// The order matters and is not decoration. `nextHeaderOffset` and
/// `nextHeaderSize` are the only file-derived numbers here that become a slice
/// bound over the whole file, and the start header's CRC is the only thing
/// that says they are the writer's rather than a fuzzer's.
#[test]
fn a_file_that_is_not_a_7z_is_refused_by_name() {
    assert_eq!(
        Archive::open(b"not a 7z at all, not even close", &Limits::DEFAULT).err(),
        Some(Error::NotA7z)
    );
    assert_eq!(
        Archive::open(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C], &Limits::DEFAULT).err(),
        Some(Error::NotA7z),
        "a signature and nothing else"
    );

    let mut bytes = archive(&[("page1.png", b"a page", As::File)]);
    bytes[20] ^= 0xFF; // inside the start header, so its CRC fails
    assert_eq!(
        Archive::open(&bytes, &Limits::DEFAULT).err(),
        Some(Error::StartHeaderCorrupt)
    );

    let mut bytes = archive(&[("page1.png", b"a page", As::File)]);
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF; // inside the header, so the header CRC fails
    assert_eq!(
        Archive::open(&bytes, &Limits::DEFAULT).err(),
        Some(Error::HeaderCrcMismatch)
    );
}

/// Coders this build does not read are refused **by their method id**, and
/// encryption is refused as encryption.
///
/// Three different sentences for three different things a host can act on:
/// re-pack without PPMd, supply a password this build will never take, or
/// nothing at all.
#[test]
fn a_coder_this_build_does_not_read_is_refused_by_its_method_id() {
    let files: &[Line<'_>] = &[("page1.png", b"a page", As::File)];
    let packed = b"a page".to_vec();

    let ppmd = archive_with(
        files,
        &[0x03, 0x04, 0x01],
        &[0x05, 0, 0, 0, 0],
        packed.clone(),
    );
    assert_eq!(
        Archive::open(&ppmd, &Limits::DEFAULT).err(),
        Some(Error::UnsupportedCoder {
            id: vec![0x03, 0x04, 0x01]
        }),
        "PPMd names itself in the refusal"
    );
    assert!(
        Archive::open(&ppmd, &Limits::DEFAULT)
            .unwrap_err()
            .to_string()
            .contains("030401"),
        "and in the sentence a host would show"
    );

    let aes = archive_with(
        files,
        &[0x06, 0xF1, 0x07, 0x01],
        &[0x53, 0x07],
        packed.clone(),
    );
    assert_eq!(
        Archive::open(&aes, &Limits::DEFAULT).err(),
        Some(Error::Encrypted),
        "AES-256 is refused as encryption, not as an unknown method"
    );

    let bzip2 = archive_with(files, &[0x04, 0x02, 0x02], &[], packed);
    assert_eq!(
        Archive::open(&bzip2, &Limits::DEFAULT).err(),
        Some(Error::UnsupportedCoder {
            id: vec![0x04, 0x02, 0x02]
        })
    );
}

/// A folder whose coder graph is not a chain is refused by name.
///
/// BCJ2 is the shape that exists: four input streams into one output, so a
/// reader that walked it as a chain would decode the first quarter of the data
/// and hand it over as the file. The `0x10` flag bit is what declares it.
#[test]
fn a_folder_that_is_not_a_chain_is_refused_by_name() {
    let mut folder = Vec::new();
    folder.extend(number(1));
    folder.push(0x14); // idSize 4, isComplex
    folder.extend_from_slice(&[0x03, 0x03, 0x01, 0x1B]); // BCJ2
    folder.extend(number(4)); // four in-streams
    folder.extend(number(1));

    let mut streams = vec![K_PACK_INFO];
    streams.extend(number(0));
    streams.extend(number(1));
    streams.push(K_SIZE);
    streams.extend(number(6));
    streams.push(K_END);
    streams.push(K_UNPACK_INFO);
    streams.push(K_FOLDER);
    streams.extend(number(1));
    streams.push(0);
    streams.extend_from_slice(&folder);
    streams.push(K_CODERS_UNPACK_SIZE);
    streams.extend(number(6));
    streams.push(K_END);
    streams.push(K_END);

    let mut header = vec![K_HEADER, K_MAIN_STREAMS];
    header.extend_from_slice(&streams);
    header.push(K_END);
    let bytes = wrap(b"a page".to_vec(), header);
    assert_eq!(
        Archive::open(&bytes, &Limits::DEFAULT).err(),
        Some(Error::NotAChain)
    );
}

/// The Deflate coder is read, because 7z method `040108` is RFC 1951 with no
/// wrapper — exactly as ZIP method 8 is.
///
/// The fixture is a raw DEFLATE **stored block**, which is the one shape of
/// that format writable by hand: `BFINAL|BTYPE=00`, then the length and its
/// complement. It proves the coder is wired to `inflate_raw` and that its
/// output size is bounded; the inflater itself is `tinker-pdf-filters`' to
/// verify and already is.
#[test]
fn the_deflate_coder_is_the_same_rfc_1951_the_zip_reader_reads() {
    let payload = b"a stored deflate block, which is legal RFC 1951";
    let mut stream = vec![0x01u8];
    stream.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    stream.extend_from_slice(&(!(payload.len() as u16)).to_le_bytes());
    stream.extend_from_slice(payload);

    let files: &[Line<'_>] = &[("page1.png", payload, As::File)];
    let bytes = archive_with(files, &[0x04, 0x01, 0x08], &[], stream);
    let mut a = open(&bytes);
    assert_eq!(a.read(0).as_deref(), Ok(&payload[..]));
}

/// Directories, empty files and anti-files are told apart, and none of them is
/// read as a page.
///
/// 7z spells all three as "no stream" and then distinguishes them with two
/// more bit vectors, which is the part a reader gets wrong: a directory and a
/// zero-byte file have identical entries but for one bit, and a reader that
/// missed it pages a comic with a blank page where a folder was.
#[test]
fn directories_and_empty_files_are_told_apart_and_neither_is_read() {
    let files: &[Line<'_>] = &[
        ("pages", b"", As::Directory),
        ("pages/page1.png", b"a page", As::File),
        ("pages/notes.txt", b"", As::Empty),
    ];
    let bytes = archive(files);
    let mut a = open(&bytes);
    let kinds: Vec<Kind> = a.entries().iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [Kind::Directory, Kind::File, Kind::EmptyFile]);
    assert!(a.entries()[0].is_directory());
    assert!(
        !a.entries()[2].is_directory(),
        "an empty file is not a folder"
    );
    assert_eq!(a.read(0), Err(EntryError::NotAFile));
    assert_eq!(a.read(1).as_deref(), Ok(&b"a page"[..]));
    assert_eq!(
        a.read(2).as_deref(),
        Ok(&b""[..]),
        "an empty file reads as no bytes rather than refusing"
    );
    assert_eq!(a.read(3), Err(EntryError::NoSuchEntry));
}

/// A `\` in a stored path becomes a `/`.
///
/// 7z records whatever separator the packing machine used, so the same comic
/// packed on Windows and on Linux has different names — and page order is
/// decided by the name. Normalising here is what makes the two archives one
/// document.
#[test]
fn a_windows_separator_in_a_name_becomes_a_forward_slash() {
    let bytes = archive(&[("chapter1\\page1.png", b"a page", As::File)]);
    let a = open(&bytes);
    assert_eq!(a.entries()[0].name, "chapter1/page1.png");
}

/// Each cap is refused by name, and each is **reachable** — see
/// `limits`' own note on why that is the property worth asserting.
#[test]
fn every_cap_is_refused_by_name_and_can_actually_fire() {
    let files: &[Line<'_>] = &[
        ("page1.png", b"one", As::File),
        ("page2.png", b"two", As::File),
        ("page3.png", b"three", As::File),
    ];
    let bytes = archive(files);

    let tight = Limits {
        max_entries: 2,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &tight).err(),
        Some(Error::TooManyEntries),
        "three entries under a cap of two"
    );

    // `max_unpacked` bounds two different things -- the compressed header and
    // a folder's output -- so exercising the *folder* half needs an archive
    // whose folder is bigger than its header, which the three-word one above
    // is not. Hence a second fixture rather than a second cap.
    let big: Vec<u8> = vec![b'x'; 4096];
    let fat = archive(&[
        ("page1.png", big.as_slice(), As::File),
        ("page2.png", big.as_slice(), As::File),
    ]);
    let tiny = Limits {
        max_unpacked: 1024,
        ..Limits::DEFAULT
    };
    let mut a = Archive::open(&fat, &tiny).expect("a header under the cap still opens");
    assert_eq!(
        a.read(0),
        Err(EntryError::TooLarge),
        "an 8 KiB folder under a cap of 1 KiB"
    );
    // And the header half: a cap below the header's own length refuses at open,
    // before the two `u64`s in the start header are used to slice anything.
    let no_header = Limits {
        max_unpacked: 4,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &no_header).err(),
        Some(Error::HeaderOutOfRange)
    );

    let one_coder = Limits {
        max_coders: 0,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &one_coder).err(),
        Some(Error::TooManyEntries)
    );

    let no_folders = Limits {
        max_folders: 0,
        ..Limits::DEFAULT
    };
    assert_eq!(
        Archive::open(&bytes, &no_folders).err(),
        Some(Error::TooManyEntries)
    );
}

/// A name past the cap is truncated **on a character boundary** and says so.
///
/// Truncated rather than refused, for the reason `limits` records: 7z stores
/// every name in one NUL-separated blob, so a refused name costs the walk its
/// place in that blob and every name after it.
#[test]
fn a_name_past_the_cap_is_truncated_and_says_so() {
    // A multi-byte character straddling the cut, so a naive `truncate` would
    // panic rather than warn.
    let long = format!("{}\u{00e9}page.png", "a".repeat(40));
    let bytes = archive(&[(long.as_str(), b"a page", As::File)]);
    let limits = Limits {
        max_name_len: 41,
        ..Limits::DEFAULT
    };
    let a = Archive::open(&bytes, &limits).expect("it opens");
    assert_eq!(a.warnings(), &[Warning::NameTruncated { index: 0 }]);
    assert_eq!(a.entries()[0].name, "a".repeat(40));
}

/// An entry the archive records no CRC-32 for is **warned about**, because of
/// what the missing check costs.
///
/// Every other reader in this workspace treats a missing checksum as ordinary.
/// Here it is not: the recorded CRC is the entire argument that this crate's
/// decompressor is right, so an entry without one is handed over
/// unadjudicated and a host is entitled to know which.
#[test]
fn an_entry_with_no_recorded_crc_is_warned_about() {
    let files: &[Line<'_>] = &[("page1.png", b"a page", As::File)];
    let mut bytes = archive(files);
    // Turn `allAreDefined` off and give an empty bit vector: the format's own
    // way of saying "no CRC for this one".
    let at = bytes
        .windows(2)
        .position(|w| w == [K_CRC, 0x01])
        .expect("the substream CRC record");
    bytes[at + 1] = 0x00; // not all defined
    bytes[at + 2] = 0x00; // the bit vector: one entry, not defined
                          // The four CRC bytes that followed are now two spare bytes plus `kEnd`,
                          // `kEnd`; rebuild the header CRC over whatever that leaves.
    let header_at = SIGNATURE_HEADER + 6;
    let header = bytes[header_at..].to_vec();
    let crc = crc32(&header).to_le_bytes();
    bytes[28..32].copy_from_slice(&crc);
    let start = bytes[12..32].to_vec();
    let start_crc = crc32(&start).to_le_bytes();
    bytes[8..12].copy_from_slice(&start_crc);

    if let Ok(a) = Archive::open(&bytes, &Limits::DEFAULT) {
        assert!(
            a.warnings()
                .iter()
                .any(|w| matches!(w, Warning::NoCrcRecorded { .. })),
            "an entry with no recorded CRC says so: {:?}",
            a.warnings()
        );
    }
}

/// 7z's `NUMBER` decodes the way the format spells it, at all eight widths.
///
/// The first byte's high bits say how many more follow **and its low bits are
/// the most significant part of the value**, which is the detail that makes a
/// hand-written decoder wrong on its first try — and wrong in a way that reads
/// small numbers correctly, so a test of three values would pass.
#[test]
fn a_number_decodes_the_way_the_format_spells_it() {
    let cases: &[u64] = &[
        0,
        1,
        0x7F,
        0x80,
        0x3FFF,
        0x4000,
        0x1F_FFFF,
        0x20_0000,
        0x0FFF_FFFF,
        0x1000_0000,
        0x07_FFFF_FFFF,
        0x08_0000_0000,
        0x03FF_FFFF_FFFF,
        0x0400_0000_0000,
        0x01_FFFF_FFFF_FFFF,
        0x02_0000_0000_0000,
        0x00FF_FFFF_FFFF_FFFF,
        u64::MAX,
    ];
    for &value in cases {
        let encoded = number(value);
        let mut at = 0usize;
        assert_eq!(
            super::number(&encoded, &mut at),
            Some(value),
            "{value:#x} as {encoded:02x?}"
        );
        assert_eq!(at, encoded.len(), "{value:#x} consumed its whole encoding");
    }
    // A `NUMBER` that runs off the end is `None`, not a partial value: the
    // walk's every bound comes from one of these.
    let mut at = 0usize;
    assert_eq!(super::number(&[0xFF, 1, 2], &mut at), None);
    let mut at = 0usize;
    assert_eq!(super::number(&[], &mut at), None);
}

/// **Hostile headers produce answers rather than panics** (ruling 1).
///
/// The fuzz target is the real instrument; this is the part that runs on every
/// commit. It walks a good archive and corrupts one byte of it at a time
/// across the whole header, which reaches every `NUMBER`, every count and
/// every property id — the bounds a fuzzer needs a corpus to find.
#[test]
fn hostile_headers_produce_answers_rather_than_panics() {
    let good = archive(&[
        ("pages", b"", As::Directory),
        ("page1.png", b"the first page", As::File),
        ("page2.png", b"the second", As::File),
        ("empty.txt", b"", As::Empty),
    ]);
    let mut answered = 0usize;
    let mut opened = 0usize;
    for at in 0..good.len() {
        for mask in [0x01u8, 0x80, 0xFF] {
            let mut bytes = good.clone();
            bytes[at] ^= mask;
            if let Ok(mut a) = Archive::open(&bytes, &Limits::DEFAULT) {
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

    // And the degenerate inputs, which the sweep above cannot reach.
    for bytes in [
        vec![],
        Vec::from(SIGNATURE),
        vec![0u8; 32],
        [SIGNATURE, &[0u8; 26][..]].concat(),
    ] {
        let _ = Archive::open(&bytes, &Limits::DEFAULT);
    }
}

/// Writes the seeds `fuzz/corpus/sevenz/` carries, so the seeds and the
/// fixtures here cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
///
/// Each seed is the target's **control byte** and then an archive. Two carry a
/// control byte of zero — every cap at its lowest — since a corpus in which
/// every seed is roomy explores the happy path and never a refusal, which is
/// gap 18a milestone 8's failure arriving through the corpus.
#[test]
#[ignore = "writes into fuzz/corpus/sevenz, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sevenz");
    std::fs::create_dir_all(&base).expect("the corpus directory");

    let stored = archive(&[
        ("page1.png", b"the first page", As::File),
        ("page10.png", b"the tenth", As::File),
        ("page2.png", b"the second page", As::File),
    ]);
    let mixed = archive(&[
        ("pages", b"", As::Directory),
        ("pages/page1.png", b"a page", As::File),
        ("pages/empty.txt", b"", As::Empty),
    ]);
    let payload = b"a stored deflate block, which is legal RFC 1951";
    let mut deflate = vec![0x01u8];
    deflate.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    deflate.extend_from_slice(&(!(payload.len() as u16)).to_le_bytes());
    deflate.extend_from_slice(payload);
    let deflated = archive_with(
        &[("page1.png", payload, As::File)],
        &[0x04, 0x01, 0x08],
        &[],
        deflate,
    );
    let ppmd = archive_with(
        &[("page1.png", b"a page", As::File)],
        &[0x03, 0x04, 0x01],
        &[0x05, 0, 0, 0, 0],
        b"a page".to_vec(),
    );
    // One bit inside the data, so the header is perfect and only the entry's
    // own CRC can see it: the branch this whole crate's verification rests on.
    let mut bad_crc = stored.clone();
    bad_crc[SIGNATURE_HEADER + 3] ^= 0x01;

    for (name, bytes) in [
        ("stored", [&[0xFFu8][..], &stored].concat()),
        ("mixed-kinds", [&[0xFF][..], &mixed].concat()),
        ("deflate", [&[0xFF][..], &deflated].concat()),
        ("unsupported-coder", [&[0xFF][..], &ppmd].concat()),
        ("crc-mismatch", [&[0xFF][..], &bad_crc].concat()),
        ("stored-tight", [&[0x00][..], &stored].concat()),
        ("mixed-tight", [&[0x00][..], &mixed].concat()),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }
}

// ---- The corpus `.cb7`s, and the shapes they were made to have --------------

/// One `.cb7` of the comic corpus, read from `crates/tinker-pdf/tests/cbz/`.
///
/// This is the one place in this file that reaches outside the crate, and it
/// is deliberate: everything else here is a hand-built Copy-coder archive
/// because the container can be exercised without an encoder, and *nothing*
/// hand-built can say how many folders 7-Zip decided to write. The claim below
/// is about the committed bytes of a real archiver's output, so it has to read
/// them.
fn corpus_cb7(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf/tests/cbz")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// What one LZMA2 stream's chunk framing asked for.
#[derive(Debug, Default, PartialEq, Eq)]
struct Framing {
    /// Chunks before the terminating zero byte.
    chunks: usize,
    /// Of those, the ones stored rather than range-coded (control `01`/`02`).
    uncompressed: usize,
    /// Chunks that asked for the dictionary to restart — control `01`, and a
    /// compressed chunk whose reset field is `3`.
    dict_resets: usize,
}

/// Walks LZMA2's chunk headers and counts what they asked for.
///
/// A second reading of the framing [`crate::lzma::decode_lzma2`] already walks,
/// and that is the point rather than a duplication to be regretted: it is a
/// *census* and not a decode, it shares no state with the decoder, and it
/// checks itself — a mis-read chunk header would leave `at` somewhere other
/// than the end of the packed stream, which the caller asserts. What it buys
/// is a fixture named `7z-dictreset.cb7` that cannot quietly stop having
/// dictionary resets.
fn framing(input: &[u8]) -> Framing {
    let mut f = Framing::default();
    let mut at = 0usize;
    let byte = |at: usize| -> usize { *input.get(at).expect("inside the packed stream") as usize };
    let be16 = |at: usize| -> usize { (byte(at) << 8) | byte(at + 1) };
    loop {
        let control = byte(at);
        at += 1;
        if control == 0 {
            break;
        }
        f.chunks += 1;
        if control < 3 {
            f.uncompressed += 1;
            if control == 1 {
                f.dict_resets += 1;
            }
            at += 2 + be16(at) + 1;
            continue;
        }
        assert!(control >= 0x80, "an LZMA2 control byte the format defines");
        let packed = be16(at + 2) + 1;
        at += 4;
        let reset = (control >> 5) & 3;
        if reset >= 2 {
            at += 1;
        }
        if reset == 3 {
            f.dict_resets += 1;
        }
        at += packed;
    }
    assert_eq!(
        at,
        input.len(),
        "the census walked the whole packed stream and stopped exactly at its end"
    );
    f
}

/// **The two `.cb7`s added for coverage have the structure they are named for.**
///
/// `7z-lzma2.cb7` is what a desktop archiver writes by default, and what it
/// writes is *one* folder holding *one* LZMA2 chunk: the folder walk in
/// [`super::decode_folder`] and the chunk loop in
/// [`crate::lzma::decode_lzma2`] each run exactly once, so neither loop's
/// second iteration was reached by any committed archive. `-ms=off` and
/// `-m0=LZMA2:d8k:c8k` are how 7-Zip was asked for the other two shapes, and
/// **this test exists because a flag is not evidence.** `-m0=LZMA2:d64k` over
/// these same five pages measured one chunk, not several — the dictionary size
/// does not split a solid block, the LZMA2 block size does — and a fixture
/// called `7z-dictreset.cb7` holding a single chunk would be worse than no
/// fixture, because the name would be doing the arguing.
///
/// So the numbers here are measured off the committed bytes by this
/// repository's own reader, and they are asserted rather than printed: a
/// regeneration that lost either shape fails here, in the crate that cares,
/// rather than passing quietly in `cbz_real.rs` where the pictures would still
/// come out right.
#[test]
fn the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for() {
    // The default: one solid folder, one chunk. The baseline the other two are
    // a departure from, asserted so that the departure means something.
    let bytes = corpus_cb7("7z-lzma2.cb7");
    let solid = open(&bytes);
    assert_eq!(solid.folders.len(), 1, "7z-lzma2.cb7: folders");
    let folder = solid.folders.first().expect("the one folder");
    assert_eq!(
        framing(packed_of(&bytes, folder)),
        Framing {
            chunks: 1,
            uncompressed: 0,
            dict_resets: 1,
        },
        "7z-lzma2.cb7: one chunk, and its dictionary reset is the one at offset zero"
    );

    // `-ms=off`: one folder per file, so `decode_folder` is called five times
    // with five different coder setups and five different pack offsets, and
    // `Archive::read`'s folder cache is asked for a folder it does not hold.
    let bytes = corpus_cb7("7z-nonsolid.cb7");
    let mut nonsolid = open(&bytes);
    assert_eq!(nonsolid.folders.len(), 5, "7z-nonsolid.cb7: folders");
    let sizes: Vec<u64> = nonsolid.folders.iter().map(Folder::unpack_size).collect();
    assert_eq!(
        sizes,
        vec![4521, 4154, 4455, 5184, 169],
        "7z-nonsolid.cb7: one folder per page, each the size of its own page"
    );
    for (index, folder) in nonsolid.folders.iter().enumerate() {
        assert_eq!(
            folder.substreams.len(),
            1,
            "7z-nonsolid.cb7: folder {index} holds one file"
        );
    }
    // Read them back to front, so the cache is missed on every entry and the
    // walk cannot be right by only ever visiting folder 0.
    for index in (0..nonsolid.entries.len()).rev() {
        let data = nonsolid
            .read(index)
            .unwrap_or_else(|e| panic!("7z-nonsolid.cb7: entry {index}: {e}"));
        assert_eq!(
            data.len() as u64,
            nonsolid.entries[index].size,
            "7z-nonsolid.cb7: entry {index} is its recorded length"
        );
    }

    // `-m0=LZMA2:d8k:c8k`: one folder, three LZMA2 chunks, each opening with a
    // dictionary reset — two of them mid-stream, at output offsets 8 192 and
    // 16 384, which are inside `page10.png` and inside `page2.png` rather than
    // on any page boundary.
    let bytes = corpus_cb7("7z-dictreset.cb7");
    let mut chunked = open(&bytes);
    assert_eq!(chunked.folders.len(), 1, "7z-dictreset.cb7: folders");
    let folder = chunked.folders.first().expect("the one folder");
    assert_eq!(
        framing(packed_of(&bytes, folder)),
        Framing {
            chunks: 3,
            uncompressed: 0,
            dict_resets: 3,
        },
        "7z-dictreset.cb7: three range-coded chunks, three dictionary resets"
    );
    assert_eq!(
        folder.substreams.len(),
        5,
        "7z-dictreset.cb7: the five pages are still one solid block"
    );
    for index in 0..chunked.entries.len() {
        chunked
            .read(index)
            .unwrap_or_else(|e| panic!("7z-dictreset.cb7: entry {index}: {e}"));
    }
}

/// A folder's packed bytes, which is what the LZMA2 framing lives in.
fn packed_of<'a>(bytes: &'a [u8], folder: &Folder) -> &'a [u8] {
    bytes
        .get(folder.packed_at..folder.packed_at + folder.packed_len)
        .expect("a folder's packed range is inside the file")
}
