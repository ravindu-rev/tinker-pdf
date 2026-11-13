//! **The CJK fallback faces are the files `THIRDPARTY.md` records, and every
//! table of each is the published face's** (FT-01.8; ruling 23's two FT-01.8
//! rows; `docs/design/fallback-faces.md`, "The storage form").
//!
//! Each face is release `2.005R` of Source Han Sans, a region's Regular, made
//! WOFF2 by the reference encoder. No outside program vouches for the bytes:
//! [`each_cjk_face_holds_every_table_of_the_published_face`] unpacks each
//! file with the engine's own `woff.rs` and holds every table it gives back
//! to the published face's, through `cjk_faces.tsv` beside this file — each
//! published table's tag, length and SHA-256, and each published face's
//! version, table count and the two `head` fields WOFF2 §5 has a coder
//! rewrite. Three things differ, each by §5, and nothing else: the `DSIG`
//! table, which an encoder MUST remove, is gone; bit 11 of `head`'s flags,
//! which an encoder MUST set, is set; and `head`'s `checkSumAdjustment` is
//! the one a decoder MUST recalculate for the font it assembles. The font
//! data are the published face's apart from the compression, which is what
//! lets each face keep its reserved name under the OFL (`THIRDPARTY.md`).
//!
//! The record is the published faces', and
//! [`the_record_is_the_published_faces`] holds it to them wherever
//! `TINKER_CJK_FACES` names a directory holding them — the census's
//! directory (`docs/design/fallback-faces.md`, "The census") — each published
//! file first held to the SHA-256 `THIRDPARTY.md` records for it. It prints
//! [`RAN`], or [`SKIPPED`] where the variable names no directory.
//! [`cjk_face_tables`], ignored, prints the record's rows from the published
//! faces:
//!
//! ```text
//! TINKER_CJK_FACES=<directory> cargo test -p tinker-pdf --features cjk-fonts --test cjk_faces -- --ignored cjk_face_tables --exact --nocapture
//! ```
//!
//! The file compiles to nothing without `cjk-fonts`: a build without the
//! faces has nothing here to hold.

#![cfg(feature = "cjk-fonts")]

use std::path::{Path, PathBuf};

use tinker_pdf_crypto::sha2::sha256;
use tinker_pdf_font::bundled::Region;
use tinker_pdf_font::woff;

/// Printed when the record was held to the published faces: a skipped
/// check exits 0 and reads like a pass, so a run without this line proves
/// nothing about them.
const RAN: &str = "cjk-faces: RAN";

/// Printed when there were no published faces to hold the record to.
const SKIPPED: &str = "cjk-faces: SKIPPED";

/// The published faces' record, beside this file.
const RECORD: &str = include_str!("cjk_faces.tsv");

/// The head of the record's `face` rows.
const FACE_HEAD: &str = "face\tfile\tsfnt_version\ttables\tchecksum_adjustment\tflags";

/// The head of the record's `table` rows.
const TABLE_HEAD: &str = "table\tfile\ttag\tlength\tsha256";

/// `head`.
const HEAD: [u8; 4] = *b"head";

/// `DSIG`, which WOFF2 §5 has an encoder remove: "the compliant WOFF2
/// encoder MUST remove the DSIG table from an input font data".
const DSIG: [u8; 4] = *b"DSIG";

/// Bit 11 of `head`'s flags, which WOFF2 §5 has an encoder set "to indicate
/// that a recreated font file was subjected to lossless modifying
/// transform".
const LOSSLESS_TRANSFORM: u16 = 1 << 11;

/// The sfnt whole-font checksum every font sums to, `checkSumAdjustment`
/// included.
const WHOLE_FONT_SUM: u32 = 0xB1B0_AFBA;

/// A bound on a face unpacked, above every face the build carries.
const MAX_FACE: usize = 64 << 20;

/// One published face, as the record states it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Face {
    /// The published file, as its release names it.
    file: String,
    /// The sfnt version: `OTTO` for these CFF faces.
    version: u32,
    /// How many tables its directory lists.
    tables: usize,
    /// `head`'s `checkSumAdjustment`.
    adjustment: u32,
    /// `head`'s flags.
    flags: u16,
}

/// One published table, as the record states it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Table {
    /// The published face's file.
    file: String,
    /// The table's tag.
    tag: [u8; 4],
    /// Its length, as the face's directory states it.
    length: usize,
    /// The SHA-256 of its bytes, in lower-case hex.
    sha256: String,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A tag as the record spells it: its four characters, a trailing space
/// kept.
fn tag_text(tag: [u8; 4]) -> String {
    tag.iter().map(|&byte| char::from(byte)).collect()
}

/// An sfnt's version and its tables, each a tag and its bytes, in its
/// directory's order. `None` where the directory reaches past the font.
fn tables(font: &[u8]) -> Option<(u32, Vec<([u8; 4], &[u8])>)> {
    let version = be32(font, 0)?;
    let count = usize::from(be16(font, 4)?);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 12 + i * 16;
        let tag: [u8; 4] = font.get(at..at + 4)?.try_into().ok()?;
        let offset = usize::try_from(be32(font, at + 8)?).ok()?;
        let length = usize::try_from(be32(font, at + 12)?).ok()?;
        out.push((tag, font.get(offset..offset.checked_add(length)?)?));
    }
    Some((version, out))
}

/// `head`'s `checkSumAdjustment` and flags.
fn head_fields(head: &[u8]) -> Option<(u32, u16)> {
    Some((be32(head, 8)?, be16(head, 16)?))
}

/// The sum of a font's big-endian 32-bit words, its last partial word
/// zero-padded: the sfnt checksum of the whole file.
fn whole_sum(font: &[u8]) -> u32 {
    font.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// The record's rows for a published face, `file` its name and `font` its
/// bytes: the `face` row and a `table` row a table, in its directory's order.
fn published_rows(file: &str, font: &[u8]) -> (Face, Vec<Table>) {
    let (version, found) = tables(font).unwrap_or_else(|| panic!("{file} is an sfnt"));
    let head = found
        .iter()
        .find(|(tag, _)| *tag == HEAD)
        .and_then(|(_, bytes)| head_fields(bytes))
        .unwrap_or_else(|| panic!("{file} has a head"));
    let face = Face {
        file: file.to_string(),
        version,
        tables: found.len(),
        adjustment: head.0,
        flags: head.1,
    };
    let rows = found
        .iter()
        .map(|(tag, bytes)| Table {
            file: file.to_string(),
            tag: *tag,
            length: bytes.len(),
            sha256: hex(&sha256(bytes)),
        })
        .collect();
    (face, rows)
}

fn face_line(face: &Face) -> String {
    format!(
        "face\t{}\t0x{:08x}\t{}\t0x{:08x}\t0x{:04x}",
        face.file, face.version, face.tables, face.adjustment, face.flags
    )
}

fn table_line(table: &Table) -> String {
    format!(
        "table\t{}\t{}\t{}\t{}",
        table.file,
        tag_text(table.tag),
        table.length,
        table.sha256
    )
}

fn number(cell: &str, what: &str) -> u32 {
    let parsed = match cell.strip_prefix("0x") {
        Some(digits) => u32::from_str_radix(digits, 16).ok(),
        None => cell.parse().ok(),
    };
    parsed.unwrap_or_else(|| panic!("{what}: `{cell}` is no number"))
}

/// The record, read: its `face` rows and its `table` rows, in its order,
/// each kind under the head the record states for it.
fn record() -> (Vec<Face>, Vec<Table>) {
    let mut faces = Vec::new();
    let mut rows = Vec::new();
    let mut heads = Vec::new();
    for line in RECORD.lines().filter(|line| !line.starts_with('#')) {
        let cells: Vec<&str> = line.split('\t').collect();
        match cells.as_slice() {
            ["face", "file", ..] | ["table", "file", ..] => heads.push(line),
            ["face", file, version, count, adjustment, flags] => faces.push(Face {
                file: (*file).to_string(),
                version: number(version, "a face's version"),
                tables: number(count, "a face's table count") as usize,
                adjustment: number(adjustment, "a face's checkSumAdjustment"),
                flags: u16::try_from(number(flags, "a face's flags")).expect("sixteen bits"),
            }),
            ["table", file, tag, length, digest] => rows.push(Table {
                file: (*file).to_string(),
                tag: tag
                    .as_bytes()
                    .try_into()
                    .unwrap_or_else(|_| panic!("`{tag}` is a tag of four characters")),
                length: number(length, "a table's length") as usize,
                sha256: (*digest).to_string(),
            }),
            [] | [""] => {}
            _ => panic!("cjk_faces.tsv: a row the record does not hold: {line:?}"),
        }
    }
    assert_eq!(heads, [FACE_HEAD, TABLE_HEAD], "the record's heads");
    (faces, rows)
}

/// The SHA-256 `THIRDPARTY.md` records for the file `name`: the line of its
/// CJK faces' section that names it beside a digest.
fn recorded_sha256(name: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../THIRDPARTY.md");
    let text = std::fs::read_to_string(&path).expect("THIRDPARTY.md");
    text.lines().find_map(|line| {
        let (digest, file) = line.trim().split_once("  ")?;
        (file.trim() == name
            && digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| digest.to_string())
    })
}

/// The WOFF2 file a region's face is stored as, by name: the published
/// file's stem with `.woff2`.
fn stored_name(region: Region) -> String {
    let file = region.published_file();
    let stem = file
        .strip_suffix(".otf")
        .expect("a published face is an .otf");
    format!("{stem}.woff2")
}

/// **Each face is the file `THIRDPARTY.md` records** (FT-01.8): the bytes
/// each region's crate compiles in hash to the SHA-256 its section records
/// beside the WOFF2 file's name, so the provenance written there — the
/// release, the encoder and its version — is the provenance of what a build
/// carries.
#[test]
fn each_cjk_face_hashes_to_the_sha256_thirdparty_records() {
    for region in Region::ALL {
        let name = stored_name(region);
        let recorded =
            recorded_sha256(&name).unwrap_or_else(|| panic!("THIRDPARTY.md records {name}"));
        assert_eq!(
            hex(&sha256(region.stored())),
            recorded,
            "{name}, as {region:?}'s crate compiles it in"
        );
    }
}

/// **Every table of each face is the published face's** (FT-01.8, ruling
/// 23): each region's file, unpacked by `woff.rs`, is a font of the published
/// face's version whose every table is one of the published face's, of its
/// length and its SHA-256, with the published face's every table but `DSIG`
/// among them. `head` is held whole too, with the two fields WOFF2 §5 has a
/// coder rewrite put back as the record states them: its flags are the
/// published face's with bit 11 set, as §5 has an encoder set it, and its
/// `checkSumAdjustment` is the decoder's, which makes the whole font sum to
/// the sfnt's constant.
#[test]
fn each_cjk_face_holds_every_table_of_the_published_face() {
    let (faces, rows) = record();
    assert_eq!(faces.len(), Region::ALL.len(), "a face a region");
    for region in Region::ALL {
        let file = region.published_file();
        let face = faces
            .iter()
            .find(|face| face.file == file)
            .unwrap_or_else(|| panic!("the record holds {file}"));
        let published: Vec<&Table> = rows.iter().filter(|row| row.file == file).collect();
        let decoded = woff::decode(region.stored(), MAX_FACE)
            .unwrap_or_else(|error| panic!("{region:?} unpacks: {error}"));
        let (version, found) =
            tables(&decoded).unwrap_or_else(|| panic!("{region:?} unpacks to an sfnt"));
        assert_eq!(
            version, face.version,
            "{region:?}: the published face's version"
        );
        assert_eq!(
            whole_sum(&decoded),
            WHOLE_FONT_SUM,
            "{region:?}: the decoder's checkSumAdjustment sums the font it assembled"
        );

        for table in &published {
            let shown = tag_text(table.tag);
            let ours = found.iter().find(|(tag, _)| *tag == table.tag);
            if table.tag == DSIG {
                assert!(ours.is_none(), "{region:?}: WOFF2 §5 has DSIG removed");
                continue;
            }
            let (_, bytes) = ours.unwrap_or_else(|| panic!("{region:?} holds {shown}"));
            assert_eq!(bytes.len(), table.length, "{region:?}: {shown}'s length");
            let mut held = bytes.to_vec();
            if table.tag == HEAD {
                let (_, flags) =
                    head_fields(bytes).unwrap_or_else(|| panic!("{region:?}: head's fields"));
                assert_eq!(
                    flags,
                    face.flags | LOSSLESS_TRANSFORM,
                    "{region:?}: head's flags are the published face's, bit 11 set"
                );
                held[8..12].copy_from_slice(&face.adjustment.to_be_bytes());
                held[16..18].copy_from_slice(&face.flags.to_be_bytes());
            }
            assert_eq!(
                hex(&sha256(&held)),
                table.sha256,
                "{region:?}: {shown} is the published face's"
            );
        }
        for (tag, _) in &found {
            assert!(
                published.iter().any(|table| table.tag == *tag),
                "{region:?}: {} is no table of the published face",
                tag_text(*tag)
            );
        }
        let removed = usize::from(published.iter().any(|table| table.tag == DSIG));
        assert_eq!(
            found.len() + removed,
            face.tables,
            "{region:?}: the published face's tables, DSIG aside"
        );
    }
}

/// **The record is whole**: a `face` row for each region's published face,
/// in the regions' order, each with as many `table` rows as it lists tables,
/// in the order its directory gives them, one of them `head`, each digest
/// sixty-four hex digits; and `THIRDPARTY.md` records the SHA-256 of each
/// published face beside its stored file's.
#[test]
fn the_record_is_whole() {
    let (faces, rows) = record();
    let files: Vec<&str> = faces.iter().map(|face| face.file.as_str()).collect();
    let expected: Vec<&str> = Region::ALL
        .iter()
        .map(|region| region.published_file())
        .collect();
    assert_eq!(files, expected, "a face row a region, in order");
    for face in &faces {
        let own: Vec<&Table> = rows.iter().filter(|row| row.file == face.file).collect();
        assert_eq!(own.len(), face.tables, "{}: a row a table", face.file);
        assert!(
            own.iter().any(|row| row.tag == HEAD),
            "{} has a head",
            face.file
        );
        for pair in own.windows(2) {
            assert!(
                pair[0].tag < pair[1].tag,
                "{}: the rows are in its directory's order, ascending by tag",
                face.file
            );
        }
        for row in own {
            assert!(
                row.sha256.len() == 64
                    && row
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{}: {}'s digest",
                face.file,
                tag_text(row.tag)
            );
        }
        assert!(
            recorded_sha256(&face.file).is_some(),
            "THIRDPARTY.md records {}",
            face.file
        );
    }
    let mut seen: Vec<(&str, [u8; 4])> = rows
        .iter()
        .map(|row| (row.file.as_str(), row.tag))
        .collect();
    seen.sort();
    let before = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), before, "no table is recorded twice");
    assert!(
        rows.iter().all(|row| files.contains(&row.file.as_str())),
        "every table row is a recorded face's"
    );
}

/// The directory `TINKER_CJK_FACES` names, when it names one.
fn faces_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TINKER_CJK_FACES")?);
    dir.is_dir().then_some(dir)
}

/// **The record is the published faces'**: where `TINKER_CJK_FACES` names a
/// directory, each region's published face in it hashes to the SHA-256
/// `THIRDPARTY.md` records for it — the release's own file — and its rows,
/// read from it, are the record's. A directory missing one fails, naming it,
/// since a record held to part of the faces is not held.
#[test]
fn the_record_is_the_published_faces() {
    let Some(dir) = faces_dir() else {
        println!(
            "{SKIPPED} the_record_is_the_published_faces (set TINKER_CJK_FACES to a \
             directory holding the published faces docs/design/fallback-faces.md names)"
        );
        return;
    };
    let (faces, rows) = record();
    for region in Region::ALL {
        let file = region.published_file();
        let font = std::fs::read(dir.join(file))
            .unwrap_or_else(|error| panic!("{} holds {file}: {error}", dir.display()));
        assert_eq!(
            Some(hex(&sha256(&font))),
            recorded_sha256(file),
            "{file} is the release's file THIRDPARTY.md records"
        );
        let (face, own) = published_rows(file, &font);
        assert_eq!(
            faces.iter().find(|recorded| recorded.file == file),
            Some(&face),
            "{file}'s face row"
        );
        let recorded: Vec<&Table> = rows.iter().filter(|row| row.file == file).collect();
        assert_eq!(
            recorded,
            own.iter().collect::<Vec<_>>(),
            "{file}'s table rows"
        );
    }
    println!(
        "{RAN} the_record_is_the_published_faces ({})",
        dir.display()
    );
}

/// **The record's rows, printed from the published faces** in
/// `TINKER_CJK_FACES`, in the record's order — the face rows, then the table
/// rows — and, beside each table, how the region's stored file unpacks it:
/// `same`, `absent`, or for `head` its flags and `checkSumAdjustment`. Holds
/// nothing.
#[test]
#[ignore = "prints the record from the published faces, and holds nothing"]
fn cjk_face_tables() {
    let Some(dir) = faces_dir() else {
        println!(
            "{SKIPPED} cjk_face_tables (set TINKER_CJK_FACES to a directory holding the \
             published faces)"
        );
        return;
    };
    println!("{RAN} cjk_face_tables ({})", dir.display());
    let mut faces = Vec::new();
    let mut rows = Vec::new();
    let mut unpacked = Vec::new();
    for region in Region::ALL {
        let file = region.published_file();
        let Ok(font) = std::fs::read(dir.join(file)) else {
            println!("absent\t{file}");
            continue;
        };
        println!("sha256\t{file}\t{}", hex(&sha256(&font)));
        let (face, own) = published_rows(file, &font);
        faces.push(face);
        let decoded = woff::decode(region.stored(), MAX_FACE);
        for table in &own {
            let note = match decoded
                .as_ref()
                .ok()
                .and_then(|decoded| tables(decoded))
                .and_then(|(_, found)| {
                    found
                        .into_iter()
                        .find(|(tag, _)| *tag == table.tag)
                        .map(|(_, bytes)| bytes.to_vec())
                }) {
                None => "absent".to_string(),
                Some(bytes) if hex(&sha256(&bytes)) == table.sha256 => "same".to_string(),
                Some(bytes) => match head_fields(&bytes) {
                    Some((adjustment, flags)) if table.tag == HEAD => {
                        format!("checkSumAdjustment 0x{adjustment:08x} flags 0x{flags:04x}")
                    }
                    _ => format!("differs: {} bytes", bytes.len()),
                },
            };
            unpacked.push(format!("unpacked\t{file}\t{}\t{note}", tag_text(table.tag)));
        }
        rows.extend(own);
    }
    println!("{FACE_HEAD}");
    for face in &faces {
        println!("{}", face_line(face));
    }
    println!("{TABLE_HEAD}");
    for row in &rows {
        println!("{}", table_line(row));
    }
    for line in unpacked {
        println!("{line}");
    }
}
