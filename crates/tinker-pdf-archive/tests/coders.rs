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
    let Some(original) = seed("sevenz", "bcj-lzma2") else {
        println!("SKIPPED: fuzz/corpus/sevenz/bcj-lzma2 is not in this tree");
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
    println!("RAN: {tried} damaged BCJ archives, none panicked");
}
