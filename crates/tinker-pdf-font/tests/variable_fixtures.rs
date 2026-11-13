//! The committed variable faces: `tests/variable/`'s two files held to the
//! statements `variable_support` builds them from, the provenance record
//! held to the files, and each face held to the tables it claims.
//!
//! # Where the files come from
//!
//! [`write_the_committed_faces`], ignored, writes them: the `glyf` face and
//! the CFF2 face that `variable_support::glyf_fixture` and
//! `variable_support::cff2_fixture` state table by table, and
//! `PROVENANCE.tsv` describing both. No outside program made them and none
//! reads them here, so there is nothing to vendor or license; other crates'
//! tests read them with `include_bytes!`, as the facade's book tests read the
//! WOFF fixtures.
//!
//! The record is held to the files rather than to the builder: a file
//! rewritten without its row, or a row edited by hand, fails
//! [`the_provenance_record_names_every_file_and_every_row_is_true`], and a
//! statement changed without the writer run fails
//! [`the_committed_faces_are_the_builders_bytes`].

mod variable_support;

use std::path::{Path, PathBuf};

use tinker_pdf_font::cff2::Cff2;
use tinker_pdf_font::variation::cvar::cvt_at;
use tinker_pdf_font::variation::gvar::Gvar;
use tinker_pdf_font::variation::metrics::{Hvar, Mvar, Vvar};
use tinker_pdf_font::variation::{tag_value, DesignSpace, Fixed, Instance};
use tinker_pdf_font::{glyf, Sfnt};
use variable_support::{cff2_fixture, checksum, glyf_fixture};

/// The `glyf` face's file name.
const GLYF_FILE: &str = "variable-glyf.ttf";
/// The CFF2 face's.
const CFF2_FILE: &str = "variable-cff2.otf";
/// The record's.
const RECORD_FILE: &str = "PROVENANCE.tsv";

/// The record's columns, in order.
const COLUMNS: &str =
    "file\tbytes\tcrc32\toutlines\ttables\taxes\tnamed_instances\tglyphs\tbuilt_by\trole";

/// The directory the faces are committed in.
fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("variable")
}

/// One committed face: its file, the builder that states it, what the
/// builder makes, and the sentence the record says about it.
struct Committed {
    file: &'static str,
    built_by: &'static str,
    bytes: Vec<u8>,
    role: &'static str,
}

fn committed() -> Vec<Committed> {
    vec![
        Committed {
            file: GLYF_FILE,
            built_by: "variable_support::glyf_fixture",
            bytes: glyf_fixture().build(),
            role: "TrueType outlines over four axes: gvar over simple, composite and empty \
                   glyphs, an avar, cvar over cvt, HVAR, VVAR and MVAR, with fpgm, prep and gasp",
        },
        Committed {
            file: CFF2_FILE,
            built_by: "variable_support::cff2_fixture",
            bytes: cff2_fixture().build(),
            role: "CFF2 outlines over two axes: two Font DICTs and two blend data sets, \
                   blends in charstrings, subroutines and Private DICTs, hints, HVAR and MVAR",
        },
    ]
}

/// A face's table directory: each record's tag, checksum, offset and
/// length, in the order the directory lists them.
fn directory_records(face: &[u8]) -> Vec<([u8; 4], u32, usize, usize)> {
    let be16 = |at: usize| usize::from(u16::from_be_bytes([face[at], face[at + 1]]));
    let be32 = |at: usize| u32::from_be_bytes(face[at..at + 4].try_into().expect("four bytes"));
    (0..be16(4))
        .map(|i| {
            let at = 12 + i * 16;
            (
                face[at..at + 4].try_into().expect("a tag"),
                be32(at + 4),
                be32(at + 8) as usize,
                be32(at + 12) as usize,
            )
        })
        .collect()
}

/// A tag as the record writes it.
fn tag_text(tag: &[u8; 4]) -> String {
    String::from_utf8(tag.to_vec()).expect("an ASCII tag")
}

/// A user value as the record writes it: the shortest decimal that is it.
fn user_text(value: Fixed) -> String {
    format!("{}", value.to_f64())
}

/// What the record says of one file, every column computed from its bytes.
fn row(file: &str, bytes: &[u8], built_by: &str, role: &str) -> String {
    let sfnt = Sfnt::parse(bytes).expect("a face");
    let outlines = if sfnt.table(tag_value(*b"glyf")).is_some() {
        "glyf"
    } else if sfnt.table(tag_value(*b"CFF2")).is_some() {
        "CFF2"
    } else {
        "none"
    };
    let tables: Vec<String> = directory_records(bytes)
        .iter()
        .map(|(tag, ..)| tag_text(tag))
        .collect();
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let axes: Vec<String> = space
        .axes()
        .iter()
        .map(|axis| {
            format!(
                "{}:{}:{}:{}",
                tag_text(&axis.tag),
                user_text(axis.min),
                user_text(axis.default),
                user_text(axis.max)
            )
        })
        .collect();
    format!(
        "{file}\t{}\t{:08x}\t{outlines}\t{}\t{}\t{}\t{}\t{built_by}\t{role}",
        bytes.len(),
        tinker_pdf_filters::crc32(bytes),
        tables.join(","),
        axes.join(" "),
        space.instances().len(),
        sfnt.glyph_count().expect("maxp counts the glyphs"),
    )
}

/// The record as the writer writes it.
fn record() -> String {
    let mut out = String::new();
    out.push_str(
        "# Written by tests/variable_fixtures.rs's write_the_committed_faces from the\n\
         # statements in tests/variable_support/mod.rs. First-party bytes: no outside\n\
         # program made these faces, and the tests beside them hold each to its\n\
         # builder byte for byte and this record to the files.\n",
    );
    out.push_str(COLUMNS);
    out.push('\n');
    for face in committed() {
        out.push_str(&row(face.file, &face.bytes, face.built_by, face.role));
        out.push('\n');
    }
    out
}

/// Writes the two faces and the record into `tests/variable/`.
#[test]
#[ignore = "writes tests/variable/; run it when a statement in variable_support changes"]
fn write_the_committed_faces() {
    let dir = directory();
    std::fs::create_dir_all(&dir).expect("the directory");
    for face in committed() {
        std::fs::write(dir.join(face.file), &face.bytes).expect("a face written");
    }
    std::fs::write(dir.join(RECORD_FILE), record()).expect("the record written");
}

/// A committed file's bytes.
fn read(file: &str) -> Vec<u8> {
    let path = directory().join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn the_committed_faces_are_the_builders_bytes() {
    for face in committed() {
        let on_disk = read(face.file);
        assert!(
            on_disk == face.bytes,
            "{} is not what {} builds ({} bytes on disk, {} built); \
             `cargo test -p tinker-pdf-font --test variable_fixtures -- --ignored \
             write_the_committed_faces --exact` writes it",
            face.file,
            face.built_by,
            on_disk.len(),
            face.bytes.len()
        );
    }
}

#[test]
fn the_provenance_record_names_every_file_and_every_row_is_true() {
    let text = String::from_utf8(read(RECORD_FILE)).expect("UTF-8");
    let mut lines = text.lines().filter(|line| !line.starts_with('#'));
    assert_eq!(lines.next(), Some(COLUMNS), "the record's columns");
    let rows: Vec<&str> = lines.collect();
    let named: Vec<&str> = rows
        .iter()
        .map(|row| row.split('\t').next().expect("a file column"))
        .collect();

    // Every file in the directory, the record aside, has exactly one row.
    let mut files: Vec<String> = std::fs::read_dir(directory())
        .expect("the directory")
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != RECORD_FILE)
        .collect();
    files.sort();
    let mut sorted = named.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, files, "one row a committed file, and no other row");

    // And each row says what is true of its file now.
    let builders = committed();
    for line in &rows {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            columns.len(),
            COLUMNS.split('\t').count(),
            "{line}: its columns"
        );
        let (file, built_by, role) = (columns[0], columns[8], columns[9]);
        let bytes = read(file);
        assert_eq!(*line, row(file, &bytes, built_by, role), "{file}: its row");
        let builder = builders
            .iter()
            .find(|face| face.built_by == built_by)
            .unwrap_or_else(|| panic!("{file}: {built_by} is no builder here"));
        assert_eq!(builder.file, file, "{file}: its builder writes it");
        assert!(builder.bytes == bytes, "{file}: its builder's bytes");
    }
}

/// The checks every committed face passes: each directory checksum is its
/// table's, the whole face sums to 0xB1B0AFBA through `head`'s adjustment,
/// and it carries exactly the tables given.
fn holds_its_directory(file: &str, face: &[u8], tables: &[&[u8; 4]]) {
    let records = directory_records(face);
    let mut tags: Vec<[u8; 4]> = records.iter().map(|(tag, ..)| *tag).collect();
    let mut sorted = tags.clone();
    sorted.sort_unstable();
    assert_eq!(tags, sorted, "{file}: the directory is sorted by tag");
    tags.sort_unstable();
    let mut claimed: Vec<[u8; 4]> = tables.iter().map(|tag| **tag).collect();
    claimed.sort_unstable();
    assert_eq!(tags, claimed, "{file}: the tables it claims, and no others");
    for (tag, sum, offset, length) in &records {
        let mut table = face[*offset..*offset + *length].to_vec();
        if tag == b"head" {
            table[8..12].copy_from_slice(&[0; 4]);
        }
        assert_eq!(
            checksum(&table),
            *sum,
            "{file}: {}'s checksum",
            tag_text(tag)
        );
        assert_eq!(
            offset % 4,
            0,
            "{file}: {} is four-byte aligned",
            tag_text(tag)
        );
    }
    assert_eq!(
        checksum(face),
        0xB1B0_AFBA,
        "{file}: the whole-font checksum"
    );
}

#[test]
fn each_face_carries_the_tables_it_claims() {
    // The glyf face: every variation table a TrueType face can carry, the
    // hinting tables beside them, and what a face needs to be installed.
    let face = read(GLYF_FILE);
    holds_its_directory(
        GLYF_FILE,
        &face,
        &[
            b"HVAR", b"MVAR", b"OS/2", b"STAT", b"VVAR", b"avar", b"cmap", b"cvar", b"cvt ",
            b"fpgm", b"fvar", b"gasp", b"glyf", b"gvar", b"head", b"hhea", b"hmtx", b"loca",
            b"maxp", b"name", b"post", b"prep", b"vhea", b"vmtx",
        ],
    );
    let statement = glyf_fixture();
    let sfnt = Sfnt::parse(&face).expect("the glyf face parses");
    let space = DesignSpace::read(&sfnt).expect("fvar and avar are read");
    assert_eq!(space.axis_count(), 4);
    assert_eq!(space.instances().len(), statement.instances.len());
    assert!(space.has_avar(), "the glyf face bends an axis");
    let gvar = Gvar::read(&sfnt, &space)
        .expect("gvar is read")
        .expect("the face carries gvar");
    assert_eq!(gvar.glyph_count(), 8);
    let hvar = Hvar::read(&sfnt, &space)
        .expect("HVAR is read")
        .expect("the face carries HVAR");
    let vvar = Vvar::read(&sfnt, &space)
        .expect("VVAR is read")
        .expect("the face carries VVAR");
    let mvar = Mvar::read(&sfnt, &space)
        .expect("MVAR is read")
        .expect("the face carries MVAR");
    assert_eq!(mvar.tags().len(), statement.mvar_records.len());
    for (code, glyph) in [(' ', 1), ('.', 2), (':', 3), ('H', 4), ('I', 5), ('O', 6)] {
        assert_eq!(sfnt.glyph_for_char(code), Some(glyph), "cmap maps {code:?}");
    }
    for (name, glyph) in [
        (".notdef", 0),
        ("space", 1),
        ("colon", 3),
        ("O", 6),
        ("hbar", 7),
    ] {
        assert_eq!(sfnt.glyph_for_name(name), Some(glyph), "post names {name}");
    }
    let mut instances: Vec<Instance> = (0..statement.instances.len())
        .map(|i| Instance::Named(u16::try_from(i).expect("few")))
        .collect();
    instances.push(Instance::default());
    for instance in &instances {
        let location = space.location(instance).expect("a named instance");
        assert_eq!(
            cvt_at(&sfnt, &space, &location)
                .expect("cvar applies")
                .len(),
            statement.cvt.len(),
            "the cvt at {instance:?}"
        );
        for tag in mvar.tags() {
            assert!(
                mvar.value_at(&sfnt, tag, &location)
                    .expect("an MVAR value")
                    .is_some(),
                "{} has a default to vary",
                tag_text(&tag)
            );
        }
        for glyph in 0..8u16 {
            glyf::outline_at(&sfnt, glyph, Some(&gvar), &location)
                .unwrap_or_else(|e| panic!("glyph {glyph} at {instance:?}: {e}"));
            // The two sources of an advance agree: HVAR and the phantom
            // points, and VVAR and the phantom points, as the statement
            // derives the first of each from the second.
            let phantom =
                glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), &location).expect("phantoms");
            let width = phantom[1][0].saturating_sub(phantom[0][0]);
            let height = phantom[2][1].saturating_sub(phantom[3][1]);
            assert_eq!(
                hvar.advance_at(&sfnt, glyph, &location)
                    .expect("an advance"),
                Some(width),
                "glyph {glyph}'s advance at {instance:?}"
            );
            assert_eq!(
                vvar.advance_at(&sfnt, glyph, &location).expect("a height"),
                Some(height),
                "glyph {glyph}'s advance height at {instance:?}"
            );
        }
    }

    // The CFF2 face: CFF2 outlines and no other, HVAR and MVAR, and what a
    // face needs to be installed.
    let face = read(CFF2_FILE);
    holds_its_directory(
        CFF2_FILE,
        &face,
        &[
            b"CFF2", b"HVAR", b"MVAR", b"OS/2", b"STAT", b"cmap", b"fvar", b"head", b"hhea",
            b"hmtx", b"maxp", b"name", b"post",
        ],
    );
    let statement = cff2_fixture();
    let sfnt = Sfnt::parse(&face).expect("the CFF2 face parses");
    assert_eq!(sfnt.version(), u32::from_be_bytes(*b"OTTO"));
    let space = DesignSpace::read(&sfnt).expect("fvar is read");
    assert_eq!(space.axis_count(), 2);
    assert_eq!(space.instances().len(), statement.instances.len());
    let cff2 = Cff2::parse(sfnt.table(tag_value(*b"CFF2")).expect("CFF2")).expect("CFF2 parses");
    assert_eq!(cff2.glyph_count(), statement.glyphs.len());
    assert_eq!(cff2.font_dict_count(), 2);
    assert_eq!(
        cff2.variation_store()
            .expect("a variation store")
            .data_count(),
        2,
        "two blend data sets"
    );
    let hvar = Hvar::read(&sfnt, &space)
        .expect("HVAR is read")
        .expect("the face carries HVAR");
    let mvar = Mvar::read(&sfnt, &space)
        .expect("MVAR is read")
        .expect("the face carries MVAR");
    assert_eq!(mvar.tags().len(), statement.mvar_rows.len());
    for (code, glyph) in [(' ', 1), ('.', 2), (':', 3), ('H', 4), ('I', 5), ('O', 6)] {
        assert_eq!(sfnt.glyph_for_char(code), Some(glyph), "cmap maps {code:?}");
    }
    let mut instances: Vec<Instance> = (0..statement.instances.len())
        .map(|i| Instance::Named(u16::try_from(i).expect("few")))
        .collect();
    instances.push(Instance::default());
    for instance in &instances {
        let location = space.location(instance).expect("a named instance");
        let blends = cff2
            .blends_in(&space, &location)
            .expect("the store's axes are fvar's");
        for glyph in 0..7u16 {
            let outline = cff2
                .outline_at(glyph, &location)
                .unwrap_or_else(|e| panic!("glyph {glyph} at {instance:?}: {e}"));
            assert_eq!(
                glyph == 1,
                outline.segments.is_empty(),
                "glyph {glyph} draws at {instance:?}, the space alone drawing nothing"
            );
            assert!(
                hvar.advance_at(&sfnt, glyph, &location)
                    .expect("an advance")
                    .is_some(),
                "glyph {glyph}'s advance at {instance:?}"
            );
        }
        for font_dict in 0..2u16 {
            cff2.private_dict_at(font_dict, &blends)
                .unwrap_or_else(|e| panic!("Font DICT {font_dict} at {instance:?}: {e}"));
        }
    }
}
