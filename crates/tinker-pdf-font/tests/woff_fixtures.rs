//! The WOFF decoders against seven committed files, five of which are one of
//! the other two wearing a container.
//!
//! # Where the files came from, and why they are admissible
//!
//! `crates/tinker-pdf-font/tests/woff/` holds two source faces and five
//! packings of them, all written on **2026-08-31** by
//! `tests/woff/make-fixtures.py`, from `cargo run -p xtask -- synth-face` —
//! this repository's own TrueType face, the one `corpus-run --fonts synthetic`
//! measures with. The script's header carries the long version; the short one
//! is that the row this closes was open partly because **no producer emits a
//! web font into the EPUB corpus and repacking a vendored face is barred**,
//! OFL-1.1 reserving the name of the only faces available. Neither objection
//! survives a face the project wrote itself.
//!
//! | file | producer | version |
//! | --- | --- | --- |
//! | `synthetic-1.ttf` | `xtask synth-face` + `make-fixtures.py` | `synthetic-1` |
//! | `synthetic-1.woff` | fontTools `flavor="woff"` | 4.63.0 |
//! | `synthetic-1-ttf2woff.woff` | `ttf2woff` (JavaScript, pako) | 3.0.0 |
//! | `synthetic-1.woff2` | fontTools `flavor="woff2"` | 4.63.0 |
//! | `synthetic-1-wawoff2.woff2` | `wawoff2`, Google's reference C++ encoder built to wasm | 2.0.1 |
//! | `synthetic-1-aligned.ttf` | the same face, every `lsb` equal to its `xMin` | `synthetic-1` |
//! | `synthetic-1-aligned-hmtx.woff2` | fontTools, `hmtx` transform 1 asked for | 4.63.0 |
//!
//! **fontTools generated these files and does not adjudicate them.** Ruling 13
//! bars a third party from deciding whether a document was read correctly; it
//! has never barred one from supplying a document, which is what
//! `tests/brotli/`'s forty Node streams and `tests/epub/`'s nine producer
//! books already are. Nothing in this file spawns a program, and no reference
//! decoder is consulted. Every assertion here is this build against
//! **`synthetic-1.ttf`, committed beside the containers** — if fontTools and
//! this build disagreed about what a container held, the source face is what
//! would settle it.
//!
//! Two producers per format because one encoder's output tests one encoder's
//! reading of the specification, and these two disagree in ways the decoder
//! has to survive: fontTools sorts its WOFF2 table directory alphabetically,
//! so `loca` lands four entries after `glyf` rather than immediately after it,
//! which §5.5 permits for a single font and forbids only in a collection.
//! A decoder that assumed adjacency read the first fixture and refused the
//! second. This one did, until this file existed.
//!
//! # The property, and why it is not byte identity
//!
//! **WOFF 1.0** is a repackaging: each table deflated on its own, the
//! directory recording the original length and checksum. Reconstruction is
//! lossless by construction, so the assertion for it is byte identity against
//! `synthetic-1.ttf` — but only against a producer that kept the input's
//! table order, because that order is the only record of where the tables sat
//! and the WOFF directory itself is sorted by tag. fontTools keeps it and
//! `ttf2woff` sorts, so the byte-identity claim is made for one of the two and
//! the outline claim below for both. That is the honest shape of the property,
//! and a version of this file that asserted the weaker claim for both would
//! have let a decoder rebuilding in *directory* order pass. One did.
//!
//! **WOFF 2.0** is a re-encoding. §5 says in as many words that it "may
//! produce binary results that are different from the original data", and it
//! does: the table directory is rebuilt in tag order, `glyf` comes back out of
//! seven substreams, and `loca` is not stored at all. So the assertion is
//! **identical glyph outlines through `Sfnt`**, over every one of the 263
//! glyphs, plus identical `cmap` answers and identical advances.
//!
//! That triangle — source face, WOFF, WOFF2, all agreeing glyph for glyph — is
//! the strongest evidence available without an oracle, and it is entirely
//! first-party.
//!
//! # Counted injections
//!
//! Nine defects are reintroduced by [`injections`], one per rule this file
//! exists to hold, and **each is asserted to be caught**: nine assertions
//! fire, none of them zero. They are listed at that function.

use tinker_pdf_font::glyf::outline;
use tinker_pdf_font::woff::{decode, packaging, Packaging, WoffError};
use tinker_pdf_font::Sfnt;

/// Far above the 7 280 bytes any of these unpacks to, so these tests measure
/// decoding rather than the ceiling. The ceiling has its own tests.
const ROOMY: usize = 1 << 22;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/woff/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn source() -> Vec<u8> {
    fixture("synthetic-1.ttf")
}

/// One committed container.
struct Container {
    file: &'static str,
    producer: &'static str,
    packing: Packaging,
    /// Whether unpacking it reproduces `synthetic-1.ttf` **byte for byte**.
    ///
    /// A property of the producer and not only of the format. WOFF 1.0's
    /// reconstruction is lossless, but "the original font" it reconstructs is
    /// the one the container recorded: the directory is sorted by tag and the
    /// compressed blocks carry the physical order, so a producer that kept the
    /// input's table order round-trips to the same bytes and a producer that
    /// sorted them round-trips to an equivalent font at different offsets.
    /// fontTools keeps the order; `ttf2woff` writes its tables alphabetically.
    /// Neither is wrong — §5 asks for a font, not for a file — and the
    /// distinction is worth a column rather than a weaker assertion for both.
    byte_identical: bool,
}

/// The four containers.
const CONTAINERS: [Container; 4] = [
    Container {
        file: "synthetic-1.woff",
        producer: "fontTools 4.63.0",
        packing: Packaging::Woff,
        byte_identical: true,
    },
    Container {
        file: "synthetic-1-ttf2woff.woff",
        producer: "ttf2woff 3.0.0",
        packing: Packaging::Woff,
        byte_identical: false,
    },
    Container {
        file: "synthetic-1.woff2",
        producer: "fontTools 4.63.0",
        packing: Packaging::Woff2,
        // §5: a WOFF2 "may produce binary results that are different from the
        // original data". It always does here: the directory is rebuilt.
        byte_identical: false,
    },
    Container {
        file: "synthetic-1-wawoff2.woff2",
        producer: "wawoff2 2.0.1",
        packing: Packaging::Woff2,
        byte_identical: false,
    },
];

/// How many glyphs the source face has, asserted rather than read, so a
/// regenerated fixture that quietly lost the seven appended glyphs is a
/// failure here and not a weaker test everywhere.
const GLYPHS: u16 = 263;

/// `hmtx`, as a big-endian table tag.
const TAG_HMTX: u32 = 0x686D_7478;

// ---- the fixtures are what they claim to be ---------------------------------

/// The signature decides, and it agrees with the file name.
#[test]
fn each_container_announces_itself() {
    for c in CONTAINERS {
        let (name, producer, expected) = (c.file, c.producer, c.packing);
        assert_eq!(
            packaging(&fixture(name)),
            Some(expected),
            "{name} from {producer}"
        );
    }
    // The source face is not a container, and saying so is the other half of
    // a sniffer: one that answered `Some` for an sfnt would refuse every
    // ordinary font in the EPUB corpus.
    assert_eq!(packaging(&source()), None, "the source face is a bare sfnt");
}

/// The face has the 256 glyphs `synth-face` writes and the seven the fixture
/// script appends.
#[test]
fn the_source_face_is_the_synthetic_one_plus_seven() {
    let bytes = source();
    let sfnt = Sfnt::parse(&bytes).expect("the source face parses");
    let maxp = sfnt.table(0x6D61_7870).expect("maxp");
    let count = u16::from_be_bytes([maxp[4], maxp[5]]);
    assert_eq!(count, GLYPHS, "256 from synth-face and seven appended");
    assert_eq!(sfnt.units_per_em, 1000, "synth-face's units per em");

    // `synth-face` writes one filled square from code 32 up. Glyph 65 is the
    // one an `A` reaches, and it is still four lines round a 700-unit box.
    let square = outline(&sfnt, 65).expect("glyph 65 has an outline");
    assert_eq!(
        square.segments.len(),
        6,
        "a move, four lines and a close: {:?}",
        square.segments
    );
    let (x0, y0, x1, y1) = square.bounds().expect("the square has bounds");
    assert_eq!(
        (x0, y0, x1, y1),
        (0.0, 0.0, 700.0, 700.0),
        "synth-face's SIDE is 700"
    );
}

/// **`PROVENANCE.tsv` names every committed file, and every row is true.**
///
/// The record exists because a reader who opens `tests/woff/` has to be able
/// to tell where the bytes came from without reading this file or the
/// generator — which command produced each one, from which face, on what day,
/// and that the producers **generated** and did not adjudicate (ruling 13).
///
/// It is asserted rather than trusted for the reason every count in this
/// repository is: a record nothing checks is a record that stops being true
/// the first time a fixture is regenerated. Both directions are checked — a
/// file with no row and a row with no file are different mistakes, and the
/// second is the one that reads as documentation.
#[test]
fn the_provenance_record_names_every_file_and_every_row_is_true() {
    let dir = format!("{}/tests/woff", env!("CARGO_MANIFEST_DIR"));
    let record = std::fs::read_to_string(format!("{dir}/PROVENANCE.tsv"))
        .expect("tests/woff/PROVENANCE.tsv");

    // Ruling 13 is stated in the record itself, not only in this file's
    // header, because the record is what a reader of the directory finds.
    assert!(
        record.contains("GENERATED") && record.contains("ADJUDICATES"),
        "the record does not say which half of ruling 13 its producers are"
    );

    let mut rows: Vec<(String, usize, String)> = Vec::new();
    let mut seen_header = false;
    for line in record.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !seen_header {
            assert!(
                line.starts_with("file\tbytes\tproducer\tversion\tfrom\twritten\trole"),
                "unexpected header: {line}"
            );
            seen_header = true;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 7, "a row with the wrong shape: {line}");
        let bytes: usize = fields[1].parse().expect("a byte count");
        assert!(!fields[2].is_empty(), "a row with no producer: {line}");
        assert!(!fields[3].is_empty(), "a row with no version: {line}");
        assert!(!fields[4].is_empty(), "a row with no source: {line}");
        assert_eq!(fields[5], "2026-08-31", "a row with a different date");
        rows.push((fields[0].to_owned(), bytes, fields[2].to_owned()));
    }
    assert_eq!(rows.len(), 7, "seven committed files, seven rows");

    // Every row names a file of exactly that size.
    for (name, bytes, _) in &rows {
        let on_disk = std::fs::metadata(format!("{dir}/{name}"))
            .unwrap_or_else(|e| panic!("{name} is named in the record and not there: {e}"))
            .len() as usize;
        assert_eq!(on_disk, *bytes, "{name}: the record's byte count is stale");
    }

    // And every file has a row. The generator is the only thing in the
    // directory that is not a fixture.
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("the fixture directory")
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "make-fixtures.py" && name != "PROVENANCE.tsv")
        .collect();
    files.sort();
    assert_eq!(files.len(), 7, "seven committed files: {files:?}");
    for file in &files {
        assert!(
            rows.iter().any(|(name, _, _)| name == file),
            "{file} is committed and the record does not name it"
        );
    }

    // The three producers, asserted by name and by number: the whole case for
    // two encoders per format is that they have no code in common, and a
    // regenerated corpus that quietly came from one of them would still pass
    // every other test in this file.
    let mut producers: Vec<String> = rows.iter().map(|(_, _, p)| p.clone()).collect();
    producers.sort();
    producers.dedup();
    assert_eq!(
        producers,
        vec![
            "fontTools".to_owned(),
            "ttf2woff".to_owned(),
            "wawoff2".to_owned()
        ],
        "three encoders with no code in common"
    );
}

// ---- the property ------------------------------------------------------------

/// Every container unpacks to a font of exactly the source face's length.
///
/// A weak claim on its own — the outline comparison below is the real one —
/// and worth making first because it fails with a number a reader can act on
/// rather than with a glyph index.
#[test]
fn every_container_unpacks_to_the_length_of_the_source() {
    let src = source();
    for c in CONTAINERS {
        let (name, producer) = (c.file, c.producer);
        let out =
            decode(&fixture(name), ROOMY).unwrap_or_else(|e| panic!("{name} from {producer}: {e}"));
        assert_eq!(out.len(), src.len(), "{name} from {producer}");
    }
}

/// WOFF 1.0 is a repackaging, so the bytes come back.
///
/// §5 requires the directory be rebuilt in ascending tag order and the tables
/// keep the physical order the container recorded, which together reproduce
/// the original file rather than an equivalent one. Both producers, because a
/// round trip that held for only one would be a round trip through one
/// encoder's habits.
#[test]
fn woff_1_round_trips_byte_for_byte() {
    let src = source();
    let mut checked = 0usize;
    for c in CONTAINERS {
        if !c.byte_identical {
            continue;
        }
        let (name, producer) = (c.file, c.producer);
        checked += 1;
        let out = decode(&fixture(name), ROOMY).expect("a WOFF 1.0 decodes");
        assert_eq!(
            out, src,
            "{name} from {producer} is not the source face byte for byte"
        );
    }
    assert_eq!(checked, 1, "one producer here preserves the physical order");
}

/// Every glyph of every container draws what the source face draws.
///
/// This is the claim the row closed on. It is made over all 263 glyphs and not
/// a sample: the seven the fixture script appends are the ones that reach
/// WOFF2's composite, empty-glyph, off-curve and instruction paths, and a
/// sample that missed them would pass on a decoder that could not read a
/// composite at all.
#[test]
fn every_container_draws_the_source_face_glyph_for_glyph() {
    let src = source();
    let expected = Sfnt::parse(&src).expect("the source face parses");

    for c in CONTAINERS {
        let (name, producer) = (c.file, c.producer);
        let out =
            decode(&fixture(name), ROOMY).unwrap_or_else(|e| panic!("{name} from {producer}: {e}"));
        let got = Sfnt::parse(&out).unwrap_or_else(|| panic!("{name} unpacks to an sfnt"));

        let mut drawn = 0usize;
        let mut empty = 0usize;
        for glyph in 0..GLYPHS {
            let want = outline(&expected, glyph).expect("the source face has a glyf");
            let have = outline(&got, glyph)
                .unwrap_or_else(|| panic!("{name}: glyph {glyph} has no outline at all"));
            assert_eq!(
                want.segments, have.segments,
                "{name} from {producer}: glyph {glyph}"
            );
            if want.is_empty() {
                empty += 1;
            } else {
                drawn += 1;
            }
        }
        // Asserted by number so that a decoder returning an empty outline for
        // everything cannot pass by agreeing with a source face it also failed
        // to read. `synth-face` gives codes 0..31 no glyph and the fixture
        // script appends `tp.empty`, so 33 draw nothing and 230 draw.
        assert_eq!(empty, 33, "{name}: glyphs that draw nothing");
        assert_eq!(drawn, 230, "{name}: glyphs that draw something");
    }
}

/// The `cmap` survives, so a character still finds its glyph.
///
/// Separate from the outlines because a container could carry every outline
/// and lose the table that names them, and a book set in such a face renders
/// 230 correct shapes in the wrong order.
#[test]
fn every_container_keeps_the_character_map() {
    let src = source();
    let expected = Sfnt::parse(&src).expect("the source face parses");
    // Two ASCII letters, the top of the byte range, and the four private
    // codes the fixture script gives the appended glyphs.
    let probes = ['A', 'z', '\u{00FF}', '\u{2400}', '\u{2401}', '\u{2405}'];

    for c in CONTAINERS {
        let (name, producer) = (c.file, c.producer);
        let out = decode(&fixture(name), ROOMY).expect("a container decodes");
        let got = Sfnt::parse(&out).expect("it unpacks to an sfnt");
        for probe in probes {
            assert_eq!(
                expected.glyph_for_char(probe),
                got.glyph_for_char(probe),
                "{name} from {producer}: U+{:04X}",
                u32::from(probe)
            );
            assert!(
                expected.glyph_for_char(probe).is_some(),
                "the fixture maps U+{:04X} at all",
                u32::from(probe)
            );
        }
    }
}

/// Advances survive, including through WOFF2's `hmtx` transform.
///
/// The fixture is written so one glyph in seven has a left side bearing that
/// is not its `xMin`, which is what decides whether an encoder may delete the
/// bearings. Both WOFF2 producers were handed that face and both declined the
/// transform — so this test asserts the advances and the module's own tests
/// assert the transform's reverse against a stream built for it.
#[test]
fn every_container_keeps_the_advances() {
    let src = source();
    let expected = Sfnt::parse(&src).expect("the source face parses");
    for c in CONTAINERS {
        let (name, producer) = (c.file, c.producer);
        let out = decode(&fixture(name), ROOMY).expect("a container decodes");
        let got = Sfnt::parse(&out).expect("it unpacks to an sfnt");
        for glyph in [0u16, 1, 65, 200, GLYPHS - 1] {
            assert_eq!(
                expected.advance(glyph),
                got.advance(glyph),
                "{name} from {producer}: glyph {glyph}"
            );
        }
        assert_eq!(
            expected.advance(65),
            Some(900),
            "the fixture's advance is 900"
        );
    }
}

/// The reconstructed font's own checksums are right.
///
/// Not a restatement of the round trip: WOFF 2.0 rebuilds the directory, so
/// its checksums are this build's arithmetic rather than a copy, and a `head`
/// whose `checkSumAdjustment` is stale is a font some consumers reject and
/// this test's other assertions would never notice.
#[test]
fn the_reconstructed_font_checks_out_against_itself() {
    for c in CONTAINERS {
        let (name, producer) = (c.file, c.producer);
        let out = decode(&fixture(name), ROOMY).expect("a container decodes");
        let sfnt = Sfnt::parse(&out).expect("it unpacks to an sfnt");
        let head = sfnt.table(0x6865_6164).expect("head");
        let adjustment = u32::from_be_bytes([head[8], head[9], head[10], head[11]]);

        let mut zeroed = out.clone();
        let at = out
            .windows(4)
            .position(|w| w == b"head")
            .expect("a head entry in the directory");
        let offset =
            u32::from_be_bytes([out[at + 8], out[at + 9], out[at + 10], out[at + 11]]) as usize;
        zeroed[offset + 8..offset + 12].fill(0);

        let mut sum = 0u32;
        for chunk in zeroed.chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            sum = sum.wrapping_add(u32::from_be_bytes(word));
        }
        assert_eq!(
            adjustment,
            0xB1B0_AFBAu32.wrapping_sub(sum),
            "{name} from {producer}: checkSumAdjustment"
        );
    }
}

// ---- counted injections ------------------------------------------------------

/// Nine defects, reintroduced one at a time, each caught by name.
///
/// **Nine assertions fire and none of them is zero.** Each is a rule the
/// decoder enforces that nothing else in this file would notice the loss of,
/// because a well-formed fixture never reaches the branch:
///
/// 1. a signature that is neither container
/// 2. WOFF 1.0's reserved header field set (§3 rejects)
/// 3. `compLength` greater than `origLength` (§5 rejects)
/// 4. a `totalSfntSize` that disagrees with the directory (§4 rejects)
/// 5. a table whose `origChecksum` no longer matches (§5)
/// 6. a compressed table that will not inflate
/// 7. a WOFF2 `glyf` claiming a transform version this build has no reverse for
/// 8. a WOFF2 truncated inside its compressed block
/// 9. an output ceiling below what the font needs
///
/// The eighth is the one worth a sentence. `Truncated` and `Malformed` are
/// different conversations with whoever produced the file, and a decoder that
/// collapsed them would tell a host "your file is corrupt" when the answer is
/// "your download stopped".
#[test]
fn injections() {
    let mut caught = 0usize;

    // 1. Not a container at all.
    assert_eq!(decode(&source(), ROOMY), Err(WoffError::NotAWoff));
    caught += 1;

    // 2. §3: "This MUST be set to zero. If this field is non-zero, a
    //    conforming user agent MUST reject the file."
    let mut woff = fixture("synthetic-1.woff");
    woff[14] = 0x01;
    assert!(
        matches!(decode(&woff, ROOMY), Err(WoffError::Malformed(_))),
        "a non-zero reserved field is refused"
    );
    caught += 1;

    // 3. §5: compLength greater than origLength is invalid outright.
    let mut woff = fixture("synthetic-1.woff");
    let entry = 44; // the first table directory entry
    let orig = u32::from_be_bytes([
        woff[entry + 12],
        woff[entry + 13],
        woff[entry + 14],
        woff[entry + 15],
    ]);
    woff[entry + 8..entry + 12].copy_from_slice(&(orig + 1).to_be_bytes());
    assert!(
        matches!(decode(&woff, ROOMY), Err(WoffError::Malformed(_))),
        "a table larger compressed than uncompressed is refused"
    );
    caught += 1;

    // 4. §4: "If this value is incorrect, a conforming user agent MUST reject
    //    the file as invalid."
    let mut woff = fixture("synthetic-1.woff");
    let total = u32::from_be_bytes([woff[16], woff[17], woff[18], woff[19]]);
    woff[16..20].copy_from_slice(&(total + 4).to_be_bytes());
    assert!(
        matches!(decode(&woff, ROOMY), Err(WoffError::Malformed(_))),
        "a totalSfntSize that disagrees with the directory is refused"
    );
    caught += 1;

    // 5. A table that no longer sums to what the directory recorded. The
    //    checksum is the one end-to-end integrity check either container
    //    carries, so its own test is not optional.
    let mut woff = fixture("synthetic-1.woff");
    let sum = u32::from_be_bytes([
        woff[entry + 16],
        woff[entry + 17],
        woff[entry + 18],
        woff[entry + 19],
    ]);
    woff[entry + 16..entry + 20].copy_from_slice(&(sum ^ 1).to_be_bytes());
    assert!(
        matches!(decode(&woff, ROOMY), Err(WoffError::TableChecksum { .. })),
        "a table whose checksum disagrees is named as such"
    );
    caught += 1;

    // 6. A compressed table whose bytes are not a zlib stream.
    let mut woff = fixture("synthetic-1.woff");
    let offset = u32::from_be_bytes([
        woff[entry + 4],
        woff[entry + 5],
        woff[entry + 6],
        woff[entry + 7],
    ]) as usize;
    woff[offset] ^= 0xFF;
    assert!(
        matches!(
            decode(&woff, ROOMY),
            Err(WoffError::Unpackable { .. } | WoffError::TableChecksum { .. })
        ),
        "a table that will not inflate is named"
    );
    caught += 1;

    // 7. §4.1: "If a decoder encounters a table entry that specifies an
    //    unknown transformation version number the entire font MUST be
    //    rejected." The first directory entry of the WOFF2 fixture is `OS/2`,
    //    whose only legal version is 0.
    let mut woff2 = fixture("synthetic-1.woff2");
    woff2[48] |= 0b0100_0000;
    assert!(
        matches!(
            decode(&woff2, ROOMY),
            Err(WoffError::UnknownTransform { .. })
        ),
        "an unknown transform version is named, not guessed at"
    );
    caught += 1;

    // 8. A download that stopped. Truncated, not Malformed.
    let woff2 = fixture("synthetic-1.woff2");
    let short = &woff2[..woff2.len() - 200];
    assert!(
        matches!(
            decode(short, ROOMY),
            Err(WoffError::Truncated | WoffError::Malformed(_))
        ),
        "a truncated container is refused rather than half-read"
    );
    assert_eq!(
        decode(&woff2[..40], ROOMY),
        Err(WoffError::Truncated),
        "a container shorter than its own header is Truncated and nothing else"
    );
    caught += 1;

    // 9. The ceiling. A WOFF2 directory states its lengths in UIntBase128,
    //    which reaches 2^32 - 1 in five bytes, so this is not advisory.
    for c in CONTAINERS {
        let name = c.file;
        assert!(
            matches!(
                decode(&fixture(name), 64),
                Err(WoffError::ExceedsOutputLimit { limit: 64 })
            ),
            "{name}: a ceiling below the font is refused by name"
        );
    }
    caught += 1;

    assert_eq!(caught, 9, "nine injections, nine catches");
}

/// §5.4's `hmtx` transform, against a producer's file rather than a hand-built
/// one.
///
/// The transform deletes the left side bearings on the grounds that every one
/// of them equals its glyph's `xMin`, so reversing it means reading the bounds
/// back out of the `glyf` that was itself just reconstructed. It is the one
/// place where two reverses are chained, and nothing else here reaches it:
/// `synthetic-1.ttf` has one glyph in seven whose bearing disagrees, so no
/// encoder may apply the transform to it, and neither does.
///
/// `synthetic-1-aligned.ttf` is that face with the bearings aligned and
/// `synthetic-1-aligned-hmtx.woff2` is fontTools packing it with the transform
/// asked for by name. Same outlines, same advances, and the bearings come back
/// from the bounding boxes.
#[test]
fn the_hmtx_transform_reverses() {
    let src = fixture("synthetic-1-aligned.ttf");
    let packed = fixture("synthetic-1-aligned-hmtx.woff2");
    let out = decode(&packed, ROOMY).expect("the aligned WOFF2 decodes");
    assert_eq!(out.len(), src.len(), "the aligned face's length");

    let expected = Sfnt::parse(&src).expect("the aligned face parses");
    let got = Sfnt::parse(&out).expect("it unpacks to an sfnt");

    // The outlines first: a wrong `glyf` would give wrong bounding boxes and
    // therefore plausible-looking wrong bearings.
    for glyph in 0..GLYPHS {
        let want = outline(&expected, glyph).expect("the aligned face has a glyf");
        let have = outline(&got, glyph).expect("the decoded face has a glyf");
        assert_eq!(want.segments, have.segments, "aligned: glyph {glyph}");
    }

    // Then `hmtx` itself, byte for byte. The advances were stored and the
    // bearings were not, so this is the half of the table that had to be
    // rebuilt sitting beside the half that did not.
    let want = expected.table(TAG_HMTX).expect("the aligned face's hmtx");
    let have = got.table(TAG_HMTX).expect("the decoded face's hmtx");
    assert_eq!(have, want, "hmtx did not come back");

    // And the bearings are not all zero, which is what a reverse that gave up
    // and wrote padding would produce.
    let nonzero = want
        .chunks_exact(4)
        .filter(|m| m[2] != 0 || m[3] != 0)
        .count();
    assert!(nonzero > 0, "the fixture has bearings worth reconstructing");
}

/// Every prefix of every fixture either decodes or is refused, and none of
/// them panics (ruling 1).
///
/// A cheap sweep beside the fuzz target rather than instead of it: this runs
/// on every `cargo test`, and it covers the one axis — length — that a
/// truncated download actually varies.
#[test]
fn no_prefix_of_any_fixture_panics() {
    for c in CONTAINERS {
        let name = c.file;
        let bytes = fixture(name);
        for take in 0..bytes.len() {
            let _ = decode(&bytes[..take], ROOMY);
        }
    }
}
