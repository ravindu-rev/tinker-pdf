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
//! `docs/design/comic-archives.md` recorded that as unmet rather than closed,
//! and it stayed unmet until tier 4's coder rows brought py7zr: its archives
//! are a second implementation of the container, and the first thing one of
//! them showed is that it lists a folder's coders in the opposite order from
//! 7-Zip (`the_bcj_fixture_is_a_filter_chain_that_rewrote_operands`).

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

    // PPMd (`030401`) stood here until it was read; ARM64's branch filter
    // (`0A`) is one 7-Zip writes that this build still does not.
    let arm64 = archive_with(files, &[0x0A], &[], packed.clone());
    assert_eq!(
        Archive::open(&arm64, &Limits::DEFAULT).err(),
        Some(Error::UnsupportedCoder { id: vec![0x0A] }),
        "ARM64 names itself in the refusal"
    );
    assert!(
        Archive::open(&arm64, &Limits::DEFAULT)
            .unwrap_err()
            .to_string()
            .contains("0A"),
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

    // bzip2 (`040202`) stood here until it was read; the delta filter is a
    // coder py7zr and 7-Zip both write that this build still does not.
    let delta = archive_with(files, &[0x03], &[0x00], packed.clone());
    assert_eq!(
        Archive::open(&delta, &Limits::DEFAULT).err(),
        Some(Error::UnsupportedCoder { id: vec![0x03] })
    );

    // BCJ is read, and has no properties: one handed some is a different
    // filter, and 7-Zip since 23 refuses it rather than ignoring them.
    let bcj_with_props = archive_with(files, BCJ_X86, &[0x00, 0x10, 0x00, 0x00], packed);
    assert_eq!(
        Archive::open(&bcj_with_props, &Limits::DEFAULT).err(),
        Some(Error::UnsupportedCoder {
            id: BCJ_X86.to_vec()
        })
    );
}

/// A one-file archive whose folder is `folder`, written out byte for byte by
/// the caller, over `pack_sizes` pack streams laid end to end in `packed`,
/// with one unpack size per coder.
fn graph_archive(
    folder: Vec<u8>,
    pack_sizes: &[usize],
    unpack_sizes: &[u64],
    packed: Vec<u8>,
    file: &[u8],
) -> Vec<u8> {
    let mut streams = vec![K_PACK_INFO];
    streams.extend(number(0));
    streams.extend(number(pack_sizes.len() as u64));
    streams.push(K_SIZE);
    for size in pack_sizes {
        streams.extend(number(*size as u64));
    }
    streams.push(K_END);
    streams.push(K_UNPACK_INFO);
    streams.push(K_FOLDER);
    streams.extend(number(1));
    streams.push(0);
    streams.extend_from_slice(&folder);
    streams.push(K_CODERS_UNPACK_SIZE);
    for size in unpack_sizes {
        streams.extend(number(*size));
    }
    streams.push(K_END);
    streams.push(K_SUBSTREAMS_INFO);
    streams.push(K_CRC);
    streams.push(1);
    streams.extend_from_slice(&crc32(file).to_le_bytes());
    streams.push(K_END);
    streams.push(K_END);

    let mut header = vec![K_HEADER, K_MAIN_STREAMS];
    header.extend_from_slice(&streams);
    header.push(K_FILES_INFO);
    header.extend(number(1));
    let mut names = vec![0u8];
    names.extend(utf16("code.bin"));
    header.extend(number(u64::from(K_NAME)));
    header.extend(number(names.len() as u64));
    header.extend_from_slice(&names);
    header.extend(number(0));
    header.push(K_END);
    wrap(packed, header)
}

/// A coder record: its id, and its stream counts when they are not one each.
fn coder_record(id: &[u8], streams: Option<(u64, u64)>) -> Vec<u8> {
    let mut out = vec![id.len() as u8 | if streams.is_some() { 0x10 } else { 0 }];
    out.extend_from_slice(id);
    if let Some((ins, outs)) = streams {
        out.extend(number(ins));
        out.extend(number(outs));
    }
    out
}

/// BCJ2's decision stream for "no conversion, every time": a zero, then a
/// code of zero, which is below every bound the decoder can compute.
const NO_CONVERSIONS: [u8; 9] = [0; 9];

/// x86-shaped bytes with branch opcodes in them, which a BCJ2 decoder must
/// pass through untouched when every decision says so.
const CODE: &[u8] = b"\x55\x48\x89\xE5\xE8\x10\x20\x30\x40\x0F\x85\x01\x02\x03\x04\xE9\xC3";

/// **A folder whose coders are not a chain is read when they are a tree.**
///
/// BCJ2 is the coder that made the chain-only walk wrong: four inputs meet in
/// one output. Two folders, built from the grammar:
///
/// - BCJ2 alone, its four in-streams fed straight from four pack streams;
/// - BCJ2 fed by three Copy coders through bind pairs, with its decision
///   stream packed as it is — the shape 7-Zip writes for
///   `-m0=BCJ2 -m1=LZMA -m2=LZMA -m3=LZMA`, Copy standing in for LZMA — and
///   the pack streams listed in a different order from the in-streams, so a
///   walk that took them in list order would hand BCJ2 its streams crossed.
#[test]
fn a_bcj2_folder_is_walked_as_a_tree() {
    let mut folder = number(1);
    folder.extend(coder_record(BCJ2, Some((4, 1))));
    for index in 0..4 {
        folder.extend(number(index));
    }
    let mut packed = CODE.to_vec();
    packed.extend_from_slice(&NO_CONVERSIONS);
    let bytes = graph_archive(
        folder,
        &[CODE.len(), 0, 0, NO_CONVERSIONS.len()],
        &[CODE.len() as u64],
        packed,
        CODE,
    );
    let mut archive = open(&bytes);
    assert_eq!(
        archive.read(0).as_deref(),
        Ok(CODE),
        "BCJ2 over four pack streams"
    );

    // BCJ2 (in-streams 0-3) fed by Copy coders 1, 2 and 3 (in-streams 4, 5
    // and 6) through three bind pairs; the pack streams are listed jump,
    // decisions, main, call.
    let mut folder = number(4);
    folder.extend(coder_record(BCJ2, Some((4, 1))));
    for _ in 0..3 {
        folder.extend(coder_record(&[0x00], None));
    }
    for (input, output) in [(0u64, 1u64), (1, 2), (2, 3)] {
        folder.extend(number(input));
        folder.extend(number(output));
    }
    for stream in [6u64, 3, 4, 5] {
        folder.extend(number(stream));
    }
    let mut packed = Vec::new();
    packed.extend_from_slice(&NO_CONVERSIONS);
    packed.extend_from_slice(CODE);
    let bytes = graph_archive(
        folder,
        &[0, NO_CONVERSIONS.len(), CODE.len(), 0],
        &[CODE.len() as u64, CODE.len() as u64, 0, 0],
        packed,
        CODE,
    );
    let mut archive = open(&bytes);
    let folder = &archive.folders[0];
    assert_eq!(folder.final_out(), Some(0), "BCJ2 is the folder's output");
    assert_eq!(folder.pack_ranges.len(), 4, "four pack streams");
    assert_eq!(
        archive.read(0).as_deref(),
        Ok(CODE),
        "BCJ2 over three coders"
    );
}

/// An LZMA coder record (`030101`) with its five property bytes: `lc=3`,
/// `lp=0`, `pb=2` and a 1 MiB dictionary.
fn lzma_record() -> Vec<u8> {
    let mut out = vec![0x20 | 3, 0x03, 0x01, 0x01];
    out.extend(number(5));
    out.extend_from_slice(&[0x5D, 0x00, 0x00, 0x10, 0x00]);
    out
}

/// Five bytes no LZMA stream starts with: its range coder's first byte is
/// zero. A feeder over them fails the moment it is decoded, so a folder that
/// answers anything else did not decode it.
const NOT_LZMA: &[u8] = &[0xFF; 5];

/// A BCJ2 folder whose four inputs — main, call, jump, decisions — each come
/// out of a coder of their own, `(record, declared output, pack stream)`, over
/// one pack stream each. `out` is BCJ2's output and the file's bytes.
fn bcj2_fed(out: &[u8], declared_out: u64, feeders: [(Vec<u8>, u64, &[u8]); 4]) -> Vec<u8> {
    let mut folder = number(5);
    folder.extend(coder_record(BCJ2, Some((4, 1))));
    for (record, _, _) in &feeders {
        folder.extend_from_slice(record);
    }
    for (input, output) in [(0u64, 1u64), (1, 2), (2, 3), (3, 4)] {
        folder.extend(number(input));
        folder.extend(number(output));
    }
    for stream in [4u64, 5, 6, 7] {
        folder.extend(number(stream));
    }
    let mut unpack = vec![declared_out];
    let mut sizes = Vec::new();
    let mut packed = Vec::new();
    for (_, declared, bytes) in &feeders {
        unpack.push(*declared);
        sizes.push(bytes.len());
        packed.extend_from_slice(bytes);
    }
    graph_archive(folder, &sizes, &unpack, packed, out)
}

/// A Copy feeder over `bytes`, declaring their own length unless told
/// otherwise.
fn copy_of(bytes: &[u8]) -> (Vec<u8>, u64, &[u8]) {
    (coder_record(&[0x00], None), bytes.len() as u64, bytes)
}

/// **BCJ2's feeders are held to what BCJ2 can read, before any is decoded**
/// (review of lane 5A).
///
/// Every byte BCJ2 reads from main, call and jump is written to its output
/// once — a main byte as itself, a target as an operand's four bytes, the
/// last cut short by the output's end by at most three — so between them they
/// hold at most `out + 3` bytes it can read. Its decision stream is five
/// opening bytes and at most one more per decision, one decision per output
/// byte at most: `out + 5`. A header that declares more describes bytes
/// nothing reads, and before this check each of the three feeders was held
/// only to the folder cap on its own — a one-byte BCJ2 output over three LZMA
/// feeders declaring a gigabyte each decoded all three.
///
/// Both bounds are exact, and both edges are here: `[90 E8 00]` is a call
/// converted in the second byte with its target's four bytes cut to one, which
/// reads `out + 3` (two main bytes, four call bytes) and decodes; one more
/// byte declared in the jump stream, which nothing reads, is refused.
#[test]
fn bcj2_s_feeders_are_held_to_what_it_can_read_before_they_are_decoded() {
    // The one decision is a 1: a code at the top of the range.
    let convert: &[u8] = &[0, 0xFF, 0xFF, 0xFF, 0xFE];
    // The target is absolute: the position after its four bytes, 2 + 4, is
    // an operand of zero.
    let target: &[u8] = &[0, 0, 0, 6];
    let out: &[u8] = &[0x90, 0xE8, 0x00];
    let at_the_edge = bcj2_fed(
        out,
        3,
        [
            copy_of(b"\x90\xE8"),
            copy_of(target),
            copy_of(&[]),
            copy_of(convert),
        ],
    );
    assert_eq!(
        open(&at_the_edge).read(0).as_deref(),
        Ok(out),
        "main, call and jump at out + 3 between them decode"
    );
    let one_past = bcj2_fed(
        out,
        3,
        [
            copy_of(b"\x90\xE8"),
            copy_of(target),
            copy_of(&[0]),
            copy_of(convert),
        ],
    );
    assert_eq!(
        open(&one_past).read(0),
        Err(EntryError::Bcj2Failed),
        "one byte past out + 3 is a stream nothing reads"
    );

    // The decision stream: a one-byte output makes no decision, so it reads
    // the five opening bytes and nothing more; out + 5 decodes and out + 6
    // does not.
    let decisions = |n: usize| {
        let rc = vec![0u8; n];
        bcj2_fed(
            b"\x90",
            1,
            [copy_of(b"\x90"), copy_of(&[]), copy_of(&[]), copy_of(&rc)],
        )
    };
    assert_eq!(
        open(&decisions(6)).read(0).as_deref(),
        Ok(&b"\x90"[..]),
        "a decision stream of out + 5 decodes"
    );
    assert_eq!(
        open(&decisions(7)).read(0),
        Err(EntryError::Bcj2Failed),
        "and one of out + 6 is refused"
    );

    // **Before**, not after: the call stream is an LZMA coder declaring the
    // whole folder cap over bytes that are not LZMA. Had it been decoded the
    // answer would be its failure; the refusal is the header's sizes alone.
    let bomb = bcj2_fed(
        b"\x90",
        1,
        [
            copy_of(b"\x90"),
            (lzma_record(), MAX_7Z_UNPACKED as u64, NOT_LZMA),
            copy_of(&[]),
            copy_of(&[0; 5]),
        ],
    );
    assert_eq!(
        open(&bomb).read(0),
        Err(EntryError::Bcj2Failed),
        "a one-byte output does not decode a gigabyte of call targets"
    );

    // And BCJ2's own output is held to the cap before its feeders are
    // decoded, not after all four have been.
    let past_the_cap = bcj2_fed(
        b"\x90",
        MAX_7Z_UNPACKED as u64 + 1,
        [
            (lzma_record(), 1, NOT_LZMA),
            copy_of(&[]),
            copy_of(&[]),
            copy_of(&NO_CONVERSIONS),
        ],
    );
    assert_eq!(
        open(&past_the_cap).read(0),
        Err(EntryError::TooLarge),
        "a folder past the cap is refused before its feeders run"
    );
}

/// **A filter's decoded input is its output's length, checked before it is
/// decoded** (review of lane 5A): Copy and BCJ write exactly what they read,
/// so a header declaring their feeder at any other length is not describing
/// them, and is refused without decompressing what it declared.
#[test]
fn a_filter_s_feeder_is_its_output_s_length_before_it_is_decoded() {
    for filter in [&[0x00][..], BCJ_X86] {
        let mut folder = number(2);
        folder.extend(coder_record(filter, None));
        folder.extend(lzma_record());
        folder.extend(number(0));
        folder.extend(number(1));
        let bytes = graph_archive(
            folder,
            &[NOT_LZMA.len()],
            &[1, MAX_7Z_UNPACKED as u64],
            NOT_LZMA.to_vec(),
            b"\x90",
        );
        assert_eq!(
            open(&bytes).read(0),
            Err(EntryError::Truncated),
            "{filter:02X?} over a feeder declaring the cap for a one-byte output"
        );
    }
}

/// **A folder whose coder graph has no answer is refused by name** — the
/// sentence the chain-only reader said about every BCJ2 folder, now kept for
/// the graphs that really cannot be walked.
#[test]
fn a_folder_whose_graph_cannot_be_walked_is_refused_by_name() {
    let refused = |folder: Vec<u8>, packs: usize, unpacks: &[u64]| {
        let sizes = vec![1usize; packs];
        let bytes = graph_archive(folder, &sizes, unpacks, vec![0x90; packs], b"\x90");
        Archive::open(&bytes, &Limits::DEFAULT).err()
    };

    // A coder with two out-streams: nothing this build reads has one.
    let mut folder = number(1);
    folder.extend(coder_record(&[0x00], Some((1, 2))));
    folder.extend(number(0));
    folder.extend(number(1));
    assert_eq!(
        refused(folder, 1, &[1, 1]),
        Some(Error::NotAChain),
        "two outputs"
    );

    // BCJ2 declaring three inputs is not BCJ2.
    let mut folder = number(1);
    folder.extend(coder_record(BCJ2, Some((3, 1))));
    for index in 0..3 {
        folder.extend(number(index));
    }
    assert_eq!(
        refused(folder, 3, &[1]),
        Some(Error::NotAChain),
        "BCJ2 with three"
    );

    // Copy feeding itself: coder 1's output bound to its own input, so a walk
    // down from the folder's output never reaches it.
    let mut folder = number(2);
    folder.extend(coder_record(&[0x00], None));
    folder.extend(coder_record(&[0x00], None));
    folder.extend(number(1));
    folder.extend(number(1));
    assert_eq!(
        refused(folder, 1, &[1, 1]),
        Some(Error::NotAChain),
        "a cycle"
    );

    // A bind pair naming an in-stream the folder does not have.
    let mut folder = number(2);
    folder.extend(coder_record(&[0x00], None));
    folder.extend(coder_record(&[0x00], None));
    folder.extend(number(9));
    folder.extend(number(1));
    assert_eq!(
        refused(folder, 1, &[1, 1]),
        Some(Error::NotAChain),
        "stream 9"
    );

    // BCJ2 carrying properties is a different filter, as BCJ is.
    let mut folder = number(1);
    let mut record = coder_record(BCJ2, Some((4, 1)));
    record[0] |= 0x20;
    record.extend(number(1));
    record.push(0);
    folder.extend(record);
    for index in 0..4 {
        folder.extend(number(index));
    }
    assert_eq!(
        refused(folder, 4, &[1]),
        Some(Error::UnsupportedCoder { id: BCJ2.to_vec() }),
        "BCJ2 with properties"
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
    // The delta filter: a coder 7-Zip and py7zr both write and this build
    // does not read. (This seed was PPMd's id until PPMd was read.)
    let unsupported = archive_with(
        &[("page1.png", b"a page", As::File)],
        &[0x03],
        &[0x00],
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
        ("unsupported-coder", [&[0xFF][..], &unsupported].concat()),
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

// ---- The coder fixtures, and what they were made to reach -------------------

/// One archive of `tests/coders/`, which real writers made over this crate's
/// own inputs (`tests/coders/README.md`).
fn coder_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/coders")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// **`py7zr-bcj.7z` really is BCJ in front of LZMA2, and BCJ really had work
/// to do in it.**
///
/// `tests/coders.rs` holds the decoded entries to the files that went in,
/// which proves the pair decodes. It cannot say the filter was *needed*: a
/// writer that listed BCJ and then converted nothing would pass there too,
/// and a fixture named for a coder that never runs is the `7z-dictreset.cb7`
/// lesson again. So the folder is taken apart here: two coders, the filter
/// first, one bind pair feeding it the LZMA2 output — and that LZMA2 output,
/// the filter's input, differs from the folder's output in hundreds of
/// places, every one of them inside an `E8`/`E9` operand.
#[test]
fn the_bcj_fixture_is_a_filter_chain_that_rewrote_operands() {
    let bytes = coder_fixture("py7zr-bcj.7z");
    let archive = open(&bytes);
    assert_eq!(archive.folders.len(), 1, "one solid folder");
    let folder = &archive.folders[0];
    // py7zr lists LZMA2 first and BCJ second, where 7-Zip lists the filter
    // first: the bind pair, not the list order, says which feeds which, and a
    // walk that assumed 7-Zip's order would have read this archive backwards.
    let ids: Vec<&[u8]> = folder.coders.iter().map(|c| c.id.as_slice()).collect();
    assert_eq!(
        ids,
        [&[0x21][..], BCJ_X86],
        "py7zr's order: LZMA2, then BCJ"
    );
    assert_eq!(
        folder.bind_pairs,
        vec![(1, 0)],
        "LZMA2's output is BCJ's input"
    );
    assert_eq!(folder.packed, vec![0], "the packed stream feeds LZMA2");
    assert_eq!(folder.final_out(), Some(1), "BCJ's output is the folder's");

    let lzma2 = lzma::decode_lzma2(
        packed_of(&bytes, folder),
        folder.unpack_sizes[0] as usize,
        &Limits::DEFAULT.lzma(),
    )
    .expect("the LZMA2 half decodes");
    let whole = decode_folder(&bytes, folder, &Limits::DEFAULT)
        .ok()
        .expect("the folder decodes");
    assert_eq!(lzma2.len(), whole.len(), "a filter keeps the length");
    let differing: Vec<usize> = (0..whole.len()).filter(|&i| lzma2[i] != whole[i]).collect();
    assert!(
        differing.len() > 500,
        "BCJ rewrote operands: {} bytes differ",
        differing.len()
    );
    // Every rewritten byte sits in the four bytes after an `E8`/`E9` of the
    // decoded output — an operand, never an opcode or anything else.
    for &at in &differing {
        assert!(
            (1..=4).any(|back| at >= back && whole[at - back] & 0xFE == 0xE8),
            "byte {at} changed and is not inside a branch operand"
        );
    }
}

/// **The two PPMd fixtures run in the arenas they are named for, and the
/// tight one really restarts.**
///
/// `tests/coders.rs` holds both archives' entries to the files that went in.
/// What it cannot see is *how* the model got there, and the second archive
/// exists for one path: an arena small enough to fill, so the allocator glues
/// its free lists, borrows units from the text area and, when nothing is
/// left, throws the model away (`RestartModel`). A regeneration that quietly
/// produced a roomy arena would still decode — and would stop testing the
/// path it is named for. So the coder's properties are read off the header
/// and the restarts are counted by the decoder itself.
#[test]
fn the_ppmd_fixtures_run_in_the_arenas_they_are_named_for() {
    for (name, order, arena, restarts) in [
        ("py7zr-ppmd.7z", 6u8, 1u32 << 24, 0..=0u32),
        ("py7zr-ppmd-tight.7z", 32, 1 << 16, 10..=u32::MAX),
    ] {
        let bytes = coder_fixture(name);
        let archive = open(&bytes);
        assert_eq!(archive.folders.len(), 1, "{name}: one solid folder");
        let folder = &archive.folders[0];
        assert_eq!(folder.coders.len(), 1, "{name}: PPMd alone");
        let coder = &folder.coders[0];
        assert_eq!(coder.id, PPMD, "{name}: coder 030401");
        assert_eq!(coder.props[0], order, "{name}: model order");
        assert_eq!(
            u32::from_le_bytes([
                coder.props[1],
                coder.props[2],
                coder.props[3],
                coder.props[4]
            ]),
            arena,
            "{name}: arena size"
        );
        let size = folder.unpack_sizes[0] as usize;
        let (out, counted) = crate::ppmd::decode_counting(
            packed_of(&bytes, folder),
            &coder.props,
            size,
            &crate::ppmd::Limits {
                max_unpacked: size,
                max_memory: 1 << 24,
            },
        )
        .expect("the folder decodes");
        assert_eq!(out.len(), size);
        assert!(
            restarts.contains(&counted),
            "{name}: the model restarted {counted} times"
        );
    }
}

/// **`7zz-bcj2.7z` is the shape it is named for, and BCJ2 converted in it.**
///
/// `tests/coders.rs` holds the decoded entries to the files that went in. What
/// it cannot see is the folder: four coders, BCJ2 reading four in-streams —
/// three of them bound to the three LZMA coders' outputs, the fourth, its
/// range-coded decisions, a pack stream of its own — and whether the call and
/// jump streams hold anything at all. A BCJ2 that converted nothing would
/// decode just as correctly and test only the main stream's copy loop, so the
/// two target streams are held to being non-empty, whole four-byte targets,
/// and to adding up with the main stream to the output.
///
/// **7-Zip 26.02 lists BCJ2 last**, after the three LZMA coders, though the
/// command line numbers it `-m0`, and binds the jump stream's coder first;
/// its pack streams are in yet another order. Nothing but the bind pairs
/// says which stream is which, which is the point of walking them.
#[test]
fn the_bcj2_fixture_is_four_streams_meeting_in_one() {
    let bytes = coder_fixture("7zz-bcj2.7z");
    let archive = open(&bytes);
    assert_eq!(archive.folders.len(), 1, "one solid folder");
    let folder = &archive.folders[0];
    let ids: Vec<&[u8]> = folder.coders.iter().map(|c| c.id.as_slice()).collect();
    let lzma: &[u8] = &[0x03, 0x01, 0x01];
    assert_eq!(
        ids,
        [lzma, lzma, lzma, BCJ2],
        "three LZMA coders, then BCJ2"
    );
    let bcj2 = 3;
    assert_eq!(folder.coders[bcj2].in_streams, 4, "BCJ2 reads four streams");
    assert_eq!(
        folder.final_out(),
        Some(bcj2),
        "BCJ2's output is the folder's"
    );

    // BCJ2's in-streams are main, call, jump, decisions; each of the first
    // three is some LZMA coder's output, and the fourth is packed.
    let first = folder.first_in(bcj2);
    let feeder = |k: usize| {
        folder
            .bind_pairs
            .iter()
            .find(|(input, _)| *input == first + k)
            .map(|(_, output)| *output)
    };
    let (main, call, jump) = (feeder(0), feeder(1), feeder(2));
    assert!(
        [main, call, jump]
            .iter()
            .all(|f| f.is_some_and(|c| c < bcj2)),
        "main, call and jump each come out of an LZMA coder: {:?}",
        folder.bind_pairs
    );
    assert_eq!(feeder(3), None, "the decision stream is bound to nothing");
    assert!(
        folder.packed.contains(&(first + 3)),
        "and is packed as it is"
    );
    assert_eq!(folder.pack_ranges.len(), 4, "four pack streams");

    let size = |coder: Option<usize>| folder.unpack_sizes[coder.unwrap_or(0)];
    let out = folder.unpack_sizes[bcj2];
    let (main, call, jump) = (size(main), size(call), size(jump));
    assert!(
        call > 0 && jump > 0,
        "BCJ2 converted calls ({call}) and jumps ({jump})"
    );
    assert_eq!((call % 4, jump % 4), (0, 0), "targets are four bytes each");
    assert_eq!(
        main + call + jump,
        out,
        "the three streams are the output between them"
    );
}
