//! The coders behind 7z's and ZIP's method ids, held to archives real writers
//! made over this repository's own bytes.
//!
//! Every archive in `tests/coders/` is a third-party writer asked for one
//! coder over the files in `tests/coders/input/`, which `make-inputs.py`
//! makes by arithmetic (see `tests/coders/README.md` for the tool, version and
//! command behind each). Every coder here is lossless, so the expected answer
//! for an entry is **the file that went in** — not another decoder's output
//! (ruling 13) — and each test holds every decoded entry to it byte for byte.
//! The archive's own CRC-32 over the original bytes stands behind that inside
//! the reader, which is what adjudicates a hand-rolled decoder in this crate;
//! the byte comparison here is the stronger claim made from outside.

use std::path::{Path, PathBuf};

use tinker_pdf_archive::sevenz::{Archive, Kind, Limits};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/coders")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = dir().join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn input(name: &str) -> Vec<u8> {
    fixture(&format!("input/{name}"))
}

/// Opens a 7z and holds every entry to the input file of the same name,
/// returning the names read so a caller can assert which files it covered.
fn every_entry_is_its_input(name: &str) -> Vec<String> {
    let bytes = fixture(name);
    let mut archive = Archive::open(&bytes, &Limits::DEFAULT)
        .unwrap_or_else(|e| panic!("{name}: the archive opens: {e}"));
    assert!(
        archive
            .warnings()
            .iter()
            .all(|w| !matches!(w, tinker_pdf_archive::sevenz::Warning::NoCrcRecorded { .. })),
        "{name}: every entry carries the CRC-32 that adjudicates its coder: {:?}",
        archive.warnings()
    );
    let entries = archive.entries().to_vec();
    // Back to front, so a folder cache that only ever served entry 0 would
    // have to decode again rather than be right by accident.
    for entry in entries.iter().rev() {
        assert_eq!(entry.kind, Kind::File, "{name}: {}", entry.name);
        let got = archive
            .read(entry.index)
            .unwrap_or_else(|e| panic!("{name}: {}: {e}", entry.name));
        let want = input(&entry.name);
        assert!(
            got == want,
            "{name}: {} decodes to the file that went into it ({} bytes against {})",
            entry.name,
            got.len(),
            want.len()
        );
    }
    entries.into_iter().map(|e| e.name).collect()
}

/// **BCJ, 7z coder `03030103`, from a real writer.**
///
/// py7zr's `FILTER_X86` in front of `FILTER_LZMA2`: one folder of two coders,
/// the LZMA2 stream feeding the filter through a bind pair. `x86.bin` is the
/// file BCJ exists for — calls and jumps whose targets are inside it, runs of
/// `E8 E8 E8` for the previous-byte mask, operands that must not be converted,
/// and an `E8` in its last four bytes — and `prose.txt` shares the folder, so
/// the filter's output is divided into two entries after it runs. That the
/// filter really rewrote operands in this file, rather than being handed bytes
/// it had nothing to do to, is asserted in `sevenz/tests.rs`, which can see
/// the folder's coders.
#[test]
fn py7zr_s_bcj_entries_are_the_files_that_went_in() {
    let names = every_entry_is_its_input("py7zr-bcj.7z");
    assert_eq!(names, ["x86.bin", "prose.txt"]);
}

/// **bzip2 as 7z coder `040202`, from a real writer.**
///
/// py7zr's `FILTER_BZIP2`, which is libbzip2 1.0.8 at level 9 through
/// CPython's `bz2`: one solid folder of `prose.txt`, `runs.bin` and
/// `x86.bin`, 148 186 bytes in one bzip2 block of six Huffman groups.
#[test]
fn py7zr_s_bzip2_entries_are_the_files_that_went_in() {
    let names = every_entry_is_its_input("py7zr-bzip2.7z");
    assert_eq!(names, ["prose.txt", "runs.bin", "x86.bin"]);
}

/// **PPMd, 7z coder `030401`, from a real writer — twice.**
///
/// py7zr's `FILTER_PPMD`, which is pyppmd 1.3.1: 7-Zip's own `Ppmd7Enc.c`
/// compiled for CPython. `py7zr-ppmd.7z` is order 6 in a 16 MiB arena, room
/// the model never runs out of; `py7zr-ppmd-tight.7z` is order 32 in 64 KiB,
/// which fills and restarts the model again and again across the same
/// 148 186 bytes (counted in `sevenz/tests.rs`). The second is the one that
/// holds the allocator: its free-list gluing, its borrowing from the text
/// area, and the restart itself, each of which a decoder must do at exactly
/// the byte the encoder did or every byte after it is wrong.
#[test]
fn py7zr_s_ppmd_entries_are_the_files_that_went_in() {
    for name in ["py7zr-ppmd.7z", "py7zr-ppmd-tight.7z"] {
        let names = every_entry_is_its_input(name);
        assert_eq!(names, ["prose.txt", "runs.bin", "x86.bin"], "{name}");
    }
}

/// **BCJ2, 7z coder `0303011B`, from the only writer of it there is.**
///
/// 7-Zip 26.02's own Linux build, `-m0=BCJ2 -m1=LZMA:d20 -m2=LZMA:d20
/// -m3=LZMA:d20 -mb0:1 -mb0s1:2 -mb0s2:3` (`make-bcj2.sh`): BCJ2's main, call
/// and jump streams each LZMA-compressed, its decision stream packed as it
/// is — four coders and four pack streams meeting in one output, the folder
/// shape the chain-only reader refused as `NotAChain`. `x86.bin` gives BCJ2
/// calls, jumps and conditional jumps to convert; that it converted them is
/// asserted in `sevenz/tests.rs`, which can see the call and jump streams.
#[test]
fn seven_zip_s_bcj2_entries_are_the_files_that_went_in() {
    let names = every_entry_is_its_input("7zz-bcj2.7z");
    // 7-Zip orders a solid block by extension, so the text comes first.
    assert_eq!(names, ["prose.txt", "x86.bin"]);
}

/// One local file header of a ZIP, walked by hand: the method, the CRC-32,
/// the name and the compressed bytes. This crate has no ZIP reader and should
/// not grow one for a test; APPNOTE 4.3.7's thirty bytes are enough to find a
/// stream, and `tinker-pdf`'s `cbz_real.rs` reads the same archive through the
/// real one.
struct Local<'a> {
    method: u16,
    crc: u32,
    name: String,
    data: &'a [u8],
}

fn locals(zip: &[u8]) -> Vec<Local<'_>> {
    let u16le = |at: usize| u16::from_le_bytes([zip[at], zip[at + 1]]);
    let u32le = |at: usize| u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]]);
    let mut out = Vec::new();
    let mut at = 0usize;
    while zip.get(at..at + 4) == Some(b"PK\x03\x04") {
        let method = u16le(at + 8);
        let crc = u32le(at + 14);
        let size = u32le(at + 18) as usize;
        let name_len = u16le(at + 26) as usize;
        let extra_len = u16le(at + 28) as usize;
        let name = String::from_utf8_lossy(&zip[at + 30..at + 30 + name_len]).into_owned();
        let start = at + 30 + name_len + extra_len;
        out.push(Local {
            method,
            crc,
            name,
            data: &zip[start..start + size],
        });
        at = start + size;
    }
    out
}

/// Where the 48-bit block magic `0x314159265359` sits in a bzip2 stream, by
/// bit. A census rather than a decode: it shares nothing with the decoder, so
/// the fixture's claim to hold two blocks rests on the bytes and not on the
/// code it is testing.
fn block_magics(stream: &[u8]) -> usize {
    let bits = stream.len() * 8;
    let bit = |i: usize| (stream[i / 8] >> (7 - i % 8)) & 1;
    (0..bits.saturating_sub(47))
        .filter(|&start| {
            (0..48).all(|k| {
                let want = (0x3141_5926_5359u64 >> (47 - k)) & 1;
                u64::from(bit(start + k)) == want
            })
        })
        .count()
}

/// **bzip2 as ZIP method 12, from a real writer**, decoded here by the same
/// decoder the facade hands `tinker-pdf-zip` for it.
///
/// CPython's `zipfile` at `compresslevel=1`, whose 100 000-byte blocks cut
/// `prose.txt` in two — asserted by a census of the block magics, so the
/// fixture cannot quietly lose its second block — and `empty.txt`, which a
/// ZIP writer still codes into a stream: `BZh1` and the end-of-stream marker
/// with nothing between. Each stream is held to the file that went in and to
/// the CRC-32 the ZIP recorded.
#[test]
fn cpython_s_method_12_streams_are_the_files_that_went_in() {
    use tinker_pdf_archive::bzip2;
    let zip = fixture("python-bzip2.zip");
    let entries = locals(&zip);
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["prose.txt", "runs.bin", "x86.bin", "empty.txt"]);
    for entry in &entries {
        assert_eq!(entry.method, 12, "{}: APPNOTE method 12", entry.name);
        assert_eq!(&entry.data[..4], b"BZh1", "{}: level 1", entry.name);
        let want = input(&entry.name);
        let got = bzip2::decode(
            entry.data,
            &bzip2::Limits {
                max_unpacked: want.len(),
            },
        )
        .unwrap_or_else(|e| panic!("{}: {e}", entry.name));
        assert!(got == want, "{}: the file that went in", entry.name);
        assert_eq!(tinker_pdf_filters::crc32(&got), entry.crc, "{}", entry.name);
    }
    let blocks: Vec<usize> = entries.iter().map(|e| block_magics(e.data)).collect();
    assert_eq!(
        blocks,
        [2, 1, 1, 0],
        "prose.txt is two blocks at level 1; empty.txt is none"
    );
}

/// Reads every entry of a possibly damaged 7z and asserts only what
/// `hostile_input.rs` asserts of every parser: nothing panics, and a read that
/// succeeds is the length it declared with the CRC-32 it recorded.
fn exercise(bytes: &[u8]) {
    if let Ok(mut archive) = Archive::open(bytes, &Limits::DEFAULT) {
        for index in 0..archive.entries().len() {
            let (size, crc) = (archive.entries()[index].size, archive.entries()[index].crc);
            if let Ok(data) = archive.read(index) {
                assert_eq!(data.len() as u64, size, "a read is its declared length");
                if let Some(crc) = crc {
                    assert_eq!(tinker_pdf_filters::crc32(&data), crc, "and its CRC-32");
                }
            }
        }
    }
}

/// The real-writer fuzz seed a coder's sweep starts from, without its
/// control byte, or `None` in a tree without `fuzz/`.
fn seed(target: &str, name: &str) -> Option<Vec<u8>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target)
        .join(name);
    let bytes = std::fs::read(path).ok()?;
    Some(bytes.get(1..)?.to_vec())
}

/// Flips one bit at both ends of every byte of a small real BCJ archive — the
/// `bcj-lzma2` fuzz seed, py7zr's BCJ over `x86.bin`'s first 768 bytes — and
/// cuts it at every length. Most flips land in the LZMA2 stream, so the filter
/// runs over whatever a damaged stream decodes to; the rest reach the header's
/// coder list and bind pairs.
#[test]
fn hostile_bytes_through_a_bcj_folder_never_panic() {
    sweep("bcj-lzma2", "BCJ");
}

/// The same sweep over the `bzip2` seed, py7zr's `040202` over 600 bytes of
/// `runs.bin`: most flips land in a bzip2 block, and reach its tables, its
/// selectors and its origin pointer before the block CRC can say no.
#[test]
fn hostile_bytes_through_a_bzip2_folder_never_panic() {
    sweep("bzip2", "bzip2");
}

/// And over the `ppmd` seed, py7zr's `030401` over 700 bytes of `prose.txt`:
/// a flip in the range-coded stream steers the model somewhere its encoder
/// never went, and one in the header can hand it any order and any arena the
/// property check allows.
#[test]
fn hostile_bytes_through_a_ppmd_folder_never_panic() {
    sweep("ppmd", "PPMd");
}

/// And over the `bcj2` seed, 7-Zip's BCJ2 and three LZMA coders over 768
/// bytes of branches whose targets are inside them (`make-bcj2.sh`), so BCJ2
/// converted them: flips in the folder's bind pairs and pack-stream list
/// reach the graph check, and flips in the four streams reach BCJ2 with
/// streams of the wrong lengths.
#[test]
fn hostile_bytes_through_a_bcj2_folder_never_panic() {
    sweep("bcj2", "BCJ2");
}

/// Flips two bits of every byte of a real-writer 7z seed and cuts it at every
/// length, asserting only what `exercise` does.
fn sweep(name: &str, what: &str) {
    let Some(original) = seed("sevenz", name) else {
        println!("SKIPPED: fuzz/corpus/sevenz/{name} is not in this tree");
        return;
    };
    exercise(&original);
    let mut tried = 0usize;
    for at in 0..original.len() {
        for bit in [0x01u8, 0x80] {
            let mut bytes = original.clone();
            bytes[at] ^= bit;
            exercise(&bytes);
            tried += 1;
        }
    }
    for cut in 0..original.len() {
        exercise(&original[..cut]);
        tried += 1;
    }
    println!("RAN: {tried} damaged {what} archives, none panicked");
}
