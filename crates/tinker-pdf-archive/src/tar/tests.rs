//! What the tar reader is held to, from POSIX 1003.1's field layout.
//!
//! Every expected value here is written out from the standard rather than read
//! back from what the code emitted, and the fixtures are built by
//! [`ustar_header`] below — a transcription of the header layout, not a call
//! into the parser. The `.cbt` a real archiver wrote is opened in the facade
//! (`crates/tinker-pdf/tests/cbz_real.rs`), where its five pages can be
//! compared against the same five pages a ZIP produced; what lives here is the
//! half that comparison cannot see.
//!
//! # Injection, counted
//!
//! Eight defects were reintroduced and the suite run to see what caught them,
//! which is this repository's standing practice for a guard: a guard that
//! catches nothing when its defect is injected is not one. Counts are of the
//! 1 2xx tests in `tinker-pdf-archive` and `tinker-pdf` together, which is
//! every test in the workspace that can reach this module.
//!
//! | Injected | Caught by |
//! | --- | ---: |
//! | the `ustar` magic check dropped, so any bytes are a tar | **1** |
//! | `checksum_matches` always true, so any 512 bytes are a header | **1** |
//! | a GNU `L` pseudo-entry's name not carried to the entry after it | **2** |
//! | ustar's 155-byte `prefix` never joined to the name | **1** |
//! | a sparse file handed back with its holes closed up | **2** |
//! | `advance` rounding the payload **down** to a whole block | **9** |
//! | GNU's base-256 numeric escape read as octal | **1** |
//! | a PAX `size` record not overriding the header's size | **1** |
//!
//! Two things in that table are worth saying out loud.
//!
//! **`advance` is the only one the real-archive corpus catches.** Seven of its
//! nine are here and the other two are `cbz_real.rs`'s
//! `the_tar_a_real_archiver_wrote_pages_in_natural_order` and
//! `five_zip_writers_produce_the_same_five_pictures` — and no other defect in
//! the table reaches them at all. That is the shape of the whole lane: the
//! cross-container identity is a strong check of *the walk*, because a walk
//! that loses its place loses every entry after it, and it is a weak check of
//! anything a single header says. The unit tests are not redundant with the
//! corpus; they cover what it cannot see.
//!
//! **Six of the eight are caught once or twice.** That is low, and it is a
//! fact about tar rather than about this file: the format has no checksum over
//! file data at all — only one over each header — so there is nothing here
//! that adjudicates a *name* or a *size* except an assertion that names it.
//!
//! `sevenz`'s table is the comparison worth making, and the difference is not
//! that its numbers are bigger — they are also mostly ones and twos. It is
//! *which* defects the corpus catches. Five separate defects injected into the
//! LZMA decoder are each caught by the two `.cb7` tests and by no unit test at
//! all, because the archive's own CRC-32 rules on the decompressed bytes. tar
//! has no equivalent: only the walk is corpus-visible, and everything a single
//! header says has to be asserted by hand. `docs/design/comic-archives.md`
//! sets the three containers side by side.

use super::*;

/// One header block, from the field layout at POSIX 1003.1 and nothing else.
///
/// `magic` is the caller's, because the three dialects differ in exactly that
/// field and the difference is what several of these tests are about.
fn header(name: &[u8], size: u64, flag: u8, magic: &[u8; 8], prefix: &[u8]) -> Vec<u8> {
    let mut block = vec![0u8; BLOCK];
    let put = |block: &mut Vec<u8>, at: usize, bytes: &[u8]| {
        for (i, &b) in bytes.iter().enumerate() {
            if let Some(slot) = block.get_mut(at + i) {
                *slot = b;
            }
        }
    };
    put(&mut block, 0, &name[..name.len().min(100)]);
    put(&mut block, 100, b"0000644\0");
    put(&mut block, 108, b"0000000\0");
    put(&mut block, 116, b"0000000\0");
    put(&mut block, 124, format!("{size:011o}\0").as_bytes());
    put(&mut block, 136, b"00000000000\0");
    block[156] = flag;
    put(&mut block, 257, magic);
    put(&mut block, 345, &prefix[..prefix.len().min(155)]);

    // 148..156 reads as eight spaces while the sum is taken, then the sum goes
    // there as six octal digits, a NUL and a space.
    put(&mut block, 148, b"        ");
    let sum: u32 = block.iter().map(|&b| u32::from(b)).sum();
    put(&mut block, 148, format!("{sum:06o}\0 ").as_bytes());
    block
}

const POSIX: &[u8; 8] = b"ustar\x0000";
const GNU: &[u8; 8] = b"ustar  \0";

/// One file as the fixture builder takes it: name, data, type flag, prefix.
type File<'a> = (&'a [u8], &'a [u8], u8, &'a [u8]);

/// Rewrites a header's checksum field after the block has been edited.
///
/// POSIX 1003.1: the unsigned sum of every byte with 148..156 read as eight
/// spaces, stored as six octal digits, a NUL and a space.
fn recheck(block: &mut [u8]) {
    for slot in block.iter_mut().take(156).skip(148) {
        *slot = b' ';
    }
    let sum: u32 = block.iter().map(|&b| u32::from(b)).sum();
    for (slot, &b) in block
        .iter_mut()
        .skip(148)
        .zip(format!("{sum:06o}\0 ").as_bytes())
    {
        *slot = b;
    }
}

/// An archive of files, with the two zero blocks POSIX 1003.1 ends one with.
fn archive(files: &[File<'_>], magic: &[u8; 8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data, flag, prefix) in files {
        out.extend_from_slice(&header(name, data.len() as u64, *flag, magic, prefix));
        out.extend_from_slice(data);
        let pad = (BLOCK - data.len() % BLOCK) % BLOCK;
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    out.extend(std::iter::repeat_n(0u8, BLOCK * 2));
    out
}

fn open(bytes: &[u8]) -> Archive<'_> {
    Archive::open(bytes, &Limits::DEFAULT).expect("a tar")
}

/// The plain case, in both dialects: three files, three names, three ranges.
#[test]
fn a_ustar_archive_lists_its_files_and_hands_their_bytes_back_borrowed() {
    for magic in [POSIX, GNU] {
        let bytes = archive(
            &[
                (b"page1.png", b"one", b'0', b""),
                (b"page2.png", b"two", b'0', b""),
                (b"page3.jpg", b"three", b'0', b""),
            ],
            magic,
        );
        let tar = open(&bytes);
        let names: Vec<&str> = tar.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["page1.png", "page2.png", "page3.jpg"]);
        assert_eq!(tar.read(0), Ok(b"one".as_slice()));
        assert_eq!(tar.read(1), Ok(b"two".as_slice()));
        assert_eq!(tar.read(2), Ok(b"three".as_slice()));
        assert_eq!(tar.read(3), Err(EntryError::NoSuchEntry));
        assert!(tar.warnings().is_empty(), "a healthy archive opens cleanly");

        // The borrow is the property this module exists to keep: the bytes
        // handed back are *inside* the input, not a copy of part of it.
        let read = tar.read(0).expect("an entry");
        let start = read.as_ptr() as usize - bytes.as_ptr() as usize;
        assert_eq!(
            &bytes[start..start + read.len()],
            b"one",
            "the entry is a range of the archive rather than a copy"
        );
    }
}

/// The magic is the only signature tar has, so a file without it is not one.
#[test]
fn a_file_with_no_ustar_magic_is_refused_by_name() {
    assert_eq!(Archive::open(b"", &Limits::DEFAULT), Err(Error::NotATar));
    assert_eq!(
        Archive::open(&[0u8; 512], &Limits::DEFAULT),
        Err(Error::NotATar),
        "an all-zero block checksums correctly and is still not a tar"
    );
    let mut bytes = archive(&[(b"a.png", b"x", b'0', b"")], POSIX);
    bytes[257] = b'x';
    assert_eq!(Archive::open(&bytes, &Limits::DEFAULT), Err(Error::NotATar));
}

/// ustar's 155-byte prefix, joined with a `/`. **Only** under POSIX's magic:
/// GNU puts a sparse map in the same bytes, and a reader that joined it would
/// name the entry after a binary blob.
#[test]
fn a_posix_prefix_joins_the_name_and_a_gnu_one_does_not() {
    let bytes = archive(&[(b"p1.png", b"x", b'0', b"chapter1/pages")], POSIX);
    assert_eq!(open(&bytes).entries()[0].name, "chapter1/pages/p1.png");

    let bytes = archive(&[(b"p1.png", b"x", b'0', b"chapter1/pages")], GNU);
    assert_eq!(
        open(&bytes).entries()[0].name,
        "p1.png",
        "GNU's magic means those bytes are not a prefix"
    );
}

/// A GNU long name is a whole pseudo-entry whose data is the next entry's
/// name — and the pseudo-entry is not an entry.
#[test]
fn a_gnu_long_name_renames_the_entry_after_it_and_is_not_one_itself() {
    let long: Vec<u8> = b"chapter-one/"
        .iter()
        .copied()
        .cycle()
        .take(180)
        .chain(b"page1.png".iter().copied())
        .collect();
    let mut with_nul = long.clone();
    with_nul.push(0);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header(
        b"././@LongLink",
        with_nul.len() as u64,
        b'L',
        GNU,
        b"",
    ));
    bytes.extend_from_slice(&with_nul);
    bytes.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - with_nul.len() % BLOCK) % BLOCK,
    ));
    bytes.extend_from_slice(&header(b"truncated-in-the-header", 1, b'0', GNU, b""));
    bytes.extend_from_slice(b"x");
    bytes.extend(std::iter::repeat_n(0u8, BLOCK - 1));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries().len(), 1, "the pseudo-entry is not an entry");
    assert_eq!(
        tar.entries()[0].name,
        String::from_utf8(long).expect("ASCII")
    );
    assert_eq!(tar.read(0), Ok(b"x".as_slice()));
}

/// PAX's `path` and `size` records override the header's own fields.
///
/// `size` is the one that matters for the walk: a header can only declare
/// 8 GiB in eleven octal digits, so a larger file's real size is only in the
/// extended header, and a reader that ignored it would resume the walk in the
/// middle of the data.
#[test]
fn a_pax_header_overrides_the_name_and_the_size() {
    let records = {
        let mut out = Vec::new();
        for (keyword, value) in [
            ("path", "chapter/one/page1.png"),
            ("size", "3"),
            ("mtime", "1756000000.0"),
        ] {
            let body = format!("{keyword}={value}\n");
            // The length counts itself, which is why it is computed by
            // widening until it stops changing rather than guessed.
            let mut length = body.len() + 2;
            while format!("{length}").len() + 1 + body.len() != length {
                length += 1;
            }
            out.extend_from_slice(format!("{length} {body}").as_bytes());
        }
        out
    };

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header(
        b"PaxHeader",
        records.len() as u64,
        b'x',
        POSIX,
        b"",
    ));
    bytes.extend_from_slice(&records);
    bytes.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - records.len() % BLOCK) % BLOCK,
    ));
    // The header's own size field says 0 and PAX says 3.
    bytes.extend_from_slice(&header(b"short", 0, b'0', POSIX, b""));
    bytes.extend_from_slice(b"abc");
    bytes.extend(std::iter::repeat_n(0u8, BLOCK - 3));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries().len(), 1);
    assert_eq!(tar.entries()[0].name, "chapter/one/page1.png");
    assert_eq!(tar.entries()[0].size, 3);
    assert_eq!(tar.read(0), Ok(b"abc".as_slice()));
    assert!(
        tar.warnings().contains(&Warning::PaxRecordIgnored),
        "`mtime` is a record this build does not act on, and says so once"
    );
}

/// **Sparse and multi-volume are listed and refused by name.**
///
/// A reader that ignored the flag hands back the sparse *map* as if it were
/// the file, which is a picture with the wrong bytes in it — worse than a
/// picture that failed to decode, because nothing anywhere says so.
#[test]
fn sparse_and_multi_volume_entries_are_listed_and_refused_by_name() {
    let bytes = archive(
        &[
            (b"sparse.png", b"map", b'S', b""),
            (b"split.png", b"half", b'M', b""),
            (b"pages/", b"", b'5', b""),
            (b"link.png", b"", b'2', b""),
        ],
        GNU,
    );
    let tar = open(&bytes);
    assert_eq!(tar.entries().len(), 4, "every one is still counted");
    assert_eq!(tar.entries()[0].kind, Kind::Sparse);
    assert_eq!(tar.read(0), Err(EntryError::Sparse));
    assert_eq!(tar.entries()[1].kind, Kind::MultiVolume);
    assert_eq!(tar.read(1), Err(EntryError::MultiVolume));
    assert_eq!(tar.entries()[2].kind, Kind::Directory);
    assert_eq!(tar.read(2), Err(EntryError::NotAFile));
    assert_eq!(tar.entries()[3].kind, Kind::Link);
    assert_eq!(tar.read(3), Err(EntryError::NotAFile));
}

/// A PAX `GNU.sparse.*` record says sparse just as loudly as the type flag,
/// and the three revisions of that extension differ in which record they
/// write — so any of them is the answer.
#[test]
fn a_pax_sparse_record_is_as_good_as_the_type_flag() {
    let body = "GNU.sparse.major=1\n";
    let mut length = body.len() + 2;
    while format!("{length}").len() + 1 + body.len() != length {
        length += 1;
    }
    let records = format!("{length} {body}").into_bytes();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header(
        b"PaxHeader",
        records.len() as u64,
        b'x',
        POSIX,
        b"",
    ));
    bytes.extend_from_slice(&records);
    bytes.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - records.len() % BLOCK) % BLOCK,
    ));
    bytes.extend_from_slice(&header(b"holes.png", 1, b'0', POSIX, b""));
    bytes.extend_from_slice(b"x");
    bytes.extend(std::iter::repeat_n(0u8, BLOCK - 1));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries()[0].kind, Kind::Sparse);
    assert_eq!(tar.read(0), Err(EntryError::Sparse));
}

/// A header whose checksum does not match ends the walk, and everything before
/// it is still an answer (ruling 2).
#[test]
fn a_bad_header_checksum_ends_the_walk_and_keeps_what_came_before() {
    let mut bytes = archive(
        &[(b"a.png", b"one", b'0', b""), (b"b.png", b"two", b'0', b"")],
        GNU,
    );
    // Corrupt the second header's name without fixing its checksum.
    bytes[BLOCK * 2] = b'z';
    let tar = open(&bytes);
    assert_eq!(tar.entries().len(), 1);
    assert_eq!(tar.read(0), Ok(b"one".as_slice()));
    assert!(tar
        .warnings()
        .contains(&Warning::HeaderChecksumFailed { index: 1 }));
}

/// Historical writers on platforms with a signed `char` summed the header the
/// other way, and a reader that checked only POSIX's summation refuses their
/// archives.
#[test]
fn a_signed_header_checksum_is_accepted_too() {
    let mut block = header("é-page.png".as_bytes(), 1, b'0', GNU, b"");
    let signed: i32 = block
        .iter()
        .enumerate()
        .map(|(at, &b)| i32::from(if (148..156).contains(&at) { b' ' } else { b } as i8))
        .sum();
    for (slot, &b) in block
        .iter_mut()
        .skip(148)
        .zip(format!("{signed:06o}\0 ").as_bytes())
    {
        *slot = b;
    }
    let mut bytes = block;
    bytes.extend_from_slice(b"x");
    bytes.extend(std::iter::repeat_n(0u8, BLOCK - 1));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries().len(), 1, "the signed summation is accepted");
    assert_eq!(tar.entries()[0].name, "é-page.png");
}

/// A name that is not UTF-8 is ordinary rather than damaged: tar declares no
/// encoding for the field. The fallback is total and deterministic, and it is
/// warned about because the name is what decides page order.
#[test]
fn a_name_that_is_not_utf8_decodes_and_says_so() {
    let bytes = archive(&[(b"p\xE9ge1.png", b"x", b'0', b"")], GNU);
    let tar = open(&bytes);
    assert_eq!(tar.entries()[0].name, "p\u{E9}ge1.png");
    assert!(tar.warnings().contains(&Warning::NameNotUtf8 { index: 0 }));
}

/// **The entry cap fires, by its own refusal.**
///
/// Built rather than asserted against a lowered constant: a cap proved only
/// against a copy of itself has not been proved to fire.
#[test]
fn an_archive_with_more_entries_than_the_cap_is_refused_by_name() {
    let mut bytes = Vec::with_capacity((MAX_TAR_ENTRIES + 2) * BLOCK);
    for n in 0..=MAX_TAR_ENTRIES {
        bytes.extend_from_slice(&header(format!("p{n}.png").as_bytes(), 0, b'0', GNU, b""));
    }
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));
    assert_eq!(
        Archive::open(&bytes, &Limits::DEFAULT),
        Err(Error::TooManyEntries)
    );
    // And one fewer opens, so the cap is where it says it is.
    let inside = bytes.len() - BLOCK * 3;
    let mut smaller = bytes[..inside].to_vec();
    smaller.extend(std::iter::repeat_n(0u8, BLOCK * 2));
    assert_eq!(
        Archive::open(&smaller, &Limits::DEFAULT)
            .expect("one under the cap")
            .entries()
            .len(),
        MAX_TAR_ENTRIES
    );
}

/// **The name cap fires**, by truncation and a warning rather than a refusal —
/// a tar's name and its file are in different blocks, so refusing the name
/// would leave the walk with a file it could not name.
#[test]
fn a_name_past_the_cap_is_truncated_and_says_so() {
    let long = vec![b'a'; MAX_TAR_NAME_LEN + 64];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header(b"././@LongLink", long.len() as u64, b'L', GNU, b""));
    bytes.extend_from_slice(&long);
    bytes.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - long.len() % BLOCK) % BLOCK,
    ));
    bytes.extend_from_slice(&header(b"short.png", 0, b'0', GNU, b""));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries()[0].name.len(), MAX_TAR_NAME_LEN);
    assert!(tar
        .warnings()
        .contains(&Warning::NameTruncated { index: 0 }));
}

/// An archive that stops mid-file keeps what it has and refuses the entry that
/// was cut (ruling 2), rather than handing back a short read as if it were the
/// file.
#[test]
fn a_truncated_archive_keeps_its_whole_entries_and_refuses_the_cut_one() {
    let full = archive(
        &[
            (b"a.png", b"one", b'0', b""),
            (b"b.png", &[b'x'; 600], b'0', b""),
        ],
        GNU,
    );
    let cut = &full[..BLOCK * 3 + 100];
    let tar = open(cut);
    assert_eq!(tar.entries().len(), 2);
    assert_eq!(tar.read(0), Ok(b"one".as_slice()));
    assert_eq!(tar.read(1), Err(EntryError::Truncated));
    assert!(tar.warnings().contains(&Warning::NoEndOfArchive));
}

/// GNU's base-256 size extension, which exists because eleven octal digits top
/// out at 8 GiB.
#[test]
fn a_base_256_size_field_is_read_as_one() {
    let mut block = header(b"big.png", 0, b'0', GNU, b"");
    for slot in block.iter_mut().take(136).skip(124) {
        *slot = 0;
    }
    // The high bit of the first byte is the escape; the rest is big-endian.
    block[124] = 0x80;
    block[134] = 0x02;
    recheck(&mut block);
    let mut bytes = block;
    bytes.extend(std::iter::repeat_n(b'x', 512));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    let tar = open(&bytes);
    assert_eq!(tar.entries()[0].size, 512);
    assert_eq!(tar.read(0).map(<[u8]>::len), Ok(512));
}

/// Arbitrary bytes behind a `ustar` magic never panic (ruling 1).
///
/// The fuzz target is the real guard; this is the cheap version that runs on
/// every `cargo test`, over the shapes a fuzzer takes longest to reach — a
/// size field of every ones, a length record that does not advance, and a
/// header that claims to be its own successor.
#[test]
fn hostile_headers_produce_answers_rather_than_panics() {
    let mut base = header(b"a.png", 0, b'0', GNU, b"");
    for slot in base.iter_mut().take(136).skip(124) {
        *slot = b'7';
    }
    recheck(&mut base);
    let tar = open(&base);
    // A declared size of 8 GiB in a 512-byte file: listed, and refused when
    // read, rather than indexing past the end.
    assert_eq!(tar.entries().len(), 1);
    assert_eq!(tar.read(0), Err(EntryError::Truncated));

    // A PAX record whose length does not advance.
    let records = b"1 x\n".to_vec();
    let mut bytes = header(b"PaxHeader", records.len() as u64, b'x', POSIX, b"");
    bytes.extend_from_slice(&records);
    bytes.extend(std::iter::repeat_n(0u8, BLOCK - records.len()));
    bytes.extend(std::iter::repeat_n(0u8, BLOCK * 2));
    assert!(open(&bytes).entries().is_empty());

    // Every byte pattern behind a valid magic, over a short window.
    for seed in 0u16..=255 {
        let mut noise = vec![seed as u8; BLOCK * 3];
        noise[257..262].copy_from_slice(b"ustar");
        let _ = Archive::open(&noise, &Limits::DEFAULT).map(|tar| {
            for index in 0..tar.entries().len() {
                let _ = tar.read(index);
            }
        });
    }
}

/// Writes `fuzz/corpus/tar`, which is committed.
///
/// It lives here, beside the fixtures, for the reason `docs/verification.md`
/// gives for the other seed corpora written this way: **the seeds and the code
/// that makes them cannot drift when they are the same code.** A header layout
/// transcribed a second time into the fuzz crate would be a second
/// implementation of [`header`], and the two would disagree eventually.
///
/// A target with no seeds spends its whole budget rediscovering that a tar
/// begins with a name and a checksum, so what is committed is one file per
/// shape the walk has a *branch* for — and one of them carries a control byte
/// that puts a bound one under what the file needs, because a corpus that
/// never reaches a refusal is the corpus half of gap 18a milestone 8's
/// failure.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
#[test]
#[ignore = "writes into fuzz/corpus/tar, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/tar");
    std::fs::create_dir_all(&base).expect("the corpus directory");

    let plain = archive(
        &[
            (b"page1.png", b"one", b'0', b""),
            (b"page10.png", b"ten", b'0', b""),
            (b"page2.png", b"two", b'0', b""),
        ],
        GNU,
    );
    let posix_prefix = archive(&[(b"p1.png", b"x", b'0', b"chapter1/pages")], POSIX);
    let structural = archive(
        &[
            (b"sparse.png", b"map", b'S', b""),
            (b"split.png", b"half", b'M', b""),
            (b"pages/", b"", b'5', b""),
            (b"link.png", b"", b'2', b""),
        ],
        GNU,
    );

    // A GNU long name, which is the branch where the file chooses an
    // allocation length.
    let long: Vec<u8> = b"chapter-one/"
        .iter()
        .copied()
        .cycle()
        .take(180)
        .chain(b"page1.png".iter().copied())
        .collect();
    let mut gnu_long = Vec::new();
    gnu_long.extend_from_slice(&header(b"././@LongLink", long.len() as u64, b'L', GNU, b""));
    gnu_long.extend_from_slice(&long);
    gnu_long.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - long.len() % BLOCK) % BLOCK,
    ));
    gnu_long.extend_from_slice(&header(b"short", 1, b'0', GNU, b""));
    gnu_long.extend_from_slice(b"x");
    gnu_long.extend(std::iter::repeat_n(0u8, BLOCK - 1));
    gnu_long.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    // A PAX header, which is the branch with a length field that counts
    // itself — the one place in this format a wrong number is a loop rather
    // than a wrong answer.
    let mut records = Vec::new();
    for (keyword, value) in [("path", "chapter/one/page1.png"), ("size", "3")] {
        let body = format!("{keyword}={value}\n");
        let mut length = body.len() + 2;
        while format!("{length}").len() + 1 + body.len() != length {
            length += 1;
        }
        records.extend_from_slice(format!("{length} {body}").as_bytes());
    }
    let mut pax = Vec::new();
    pax.extend_from_slice(&header(
        b"PaxHeader",
        records.len() as u64,
        b'x',
        POSIX,
        b"",
    ));
    pax.extend_from_slice(&records);
    pax.extend(std::iter::repeat_n(
        0u8,
        (BLOCK - records.len() % BLOCK) % BLOCK,
    ));
    pax.extend_from_slice(&header(b"short", 0, b'0', POSIX, b""));
    pax.extend_from_slice(b"abc");
    pax.extend(std::iter::repeat_n(0u8, BLOCK - 3));
    pax.extend(std::iter::repeat_n(0u8, BLOCK * 2));

    // A size field of eleven sevens: 8 GiB declared inside 512 bytes, which is
    // the arithmetic every `saturating_` in the walk exists for.
    let mut absurd = header(b"huge.png", 0, b'0', GNU, b"");
    for slot in absurd.iter_mut().take(135).skip(124) {
        *slot = b'7';
    }
    absurd[135] = 0;
    recheck(&mut absurd);

    // `0xFF` sets every knob to its widest, which is the shipped shape.
    // `0x00` sets `max_entries` to 1 and `max_name_len` to 1, which is the
    // only value from which `TooManyEntries` and `NameTruncated` both fire on
    // an otherwise perfectly good archive.
    for (name, bytes) in [
        ("plain-gnu", [&[0xFFu8][..], &plain].concat()),
        ("posix-prefix", [&[0xFF][..], &posix_prefix].concat()),
        ("gnu-long-name", [&[0xFF][..], &gnu_long].concat()),
        ("pax-header", [&[0xFF][..], &pax].concat()),
        ("sparse-and-links", [&[0xFF][..], &structural].concat()),
        ("size-past-the-file", [&[0xFF][..], &absurd].concat()),
        ("plain-gnu-tight", [&[0x00][..], &plain].concat()),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }
}
