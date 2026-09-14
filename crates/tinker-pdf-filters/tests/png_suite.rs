//! PngSuite as an external arbiter for the PNG decoder (gap 29 M3).
//!
//! PngSuite is Willem van Schaik's test set, and it is **the only PNG material
//! in reach that this repository did not write**. Everything in
//! `src/png/tests.rs` is either built by a fixture helper here or produced by a
//! Python script for this milestone; both prove the decoder self-consistent,
//! and gap 18 milestone 6 is the record of what that is worth on its own — a
//! JPEG 2000 plane count that was correct only for the code-blocks its own
//! in-tree encoder happened to emit.
//!
//! # Fetched, never committed (ruling 13's input carve-out)
//!
//! PngSuite is a set of *files*, not a program: it says what a decoder should
//! produce by being deliberately broken in fourteen documented ways and
//! deliberately correct in the rest. Ruling 13 removes programs from the
//! verification surface and keeps inputs, and this is squarely an input.
//!
//! Nothing from PngSuite is committed. Fetch it and point `TINKER_PNGSUITE` at
//! the extracted directory:
//!
//! ```text
//! curl -LO http://www.schaik.com/pngsuite/PngSuite-2017jul19.zip
//! unzip -d pngsuite PngSuite-2017jul19.zip
//! TINKER_PNGSUITE=$PWD/pngsuite cargo test -p tinker-pdf-filters --test png_suite
//! ```
//!
//! Gap 17's SerenityOS JBIG2 streams are the precedent for both halves of that:
//! third-party material is a comparison reference, and a check that quietly
//! succeeds when its corpus is missing is a check that will one day be missing
//! everywhere. Without the directory these tests print [`SKIPPED`] and do
//! nothing; with it they print [`RAN`]. Gap 20 found that a skipped oracle exits
//! 0 and reads exactly like a pass, which is why the two strings exist.
//!
//! # What PngSuite publishes, and what is asserted from it
//!
//! It ships no reference rasters, so the references are the ones its author
//! published in its naming convention and its file groupings — and they are
//! sharper than a pixel dump would be, because each one is a *claim about a
//! feature* rather than about an image:
//!
//! - **The filename says the header.** `basn0g01` is non-interlaced, colour
//!   type 0, bit depth 1; `basi3p04` is interlaced, colour type 3, depth 4.
//!   The fifteen `bas*` files are exactly Table 11.1's fifteen legal pairs.
//! - **Fourteen files are broken by design** and every one must be refused. They
//!   cover four signature corruptions, a CR and an LF injected into it, a bad
//!   IHDR CRC, a bad IDAT CRC, a missing IDAT, two illegal colour types and
//!   three illegal bit depths — which is most of this milestone's refusal list,
//!   written by somebody else.
//! - **`basn` and `basi` are the same image**, interlaced and not. Fifteen
//!   independent confirmations of Adam7, on files produced by a different
//!   encoder, against a decoder that has never seen them.
//! - **`oi1`, `oi2`, `oi4` and `oi9` are the same image** in one IDAT chunk, in
//!   two, in four unequal ones and in a run of length-one ones.
//! - **`z00`, `z03`, `z06` and `z09`** are the same image at four deflate
//!   levels; **`bg*`, `ps*`, `pp*` and `ch*`** are base images carrying `bKGD`,
//!   `sPLT`, `pHYs` and `hIST`, which a decoder must ignore.
//! - **`s01` to `s40`** are square images of the size in their names, which is
//!   where the interlace passes that fall away entirely live.
//! - **`t*`** are the transparency files, and their published descriptions name
//!   which of them carry `tRNS` and which are "not transparent, for reference".
//!
//! Two groups are deliberately **not** asserted equal, and it is worth saying
//! why rather than leaving a reader to wonder: the `f*` filter files and the
//! `g*` gamma files carry different pixel data from each other, so a test
//! pairing them would be asserting something PngSuite never claimed.
//!
//! # And the encoder, which needed a second leg of its own
//!
//! `png_encode` arrived after the decoder and is held here too, because the
//! same argument applies twice over: a raster this repository decoded, encoded
//! and decoded again would agree with itself no matter what either half did.
//! PngSuite breaks half of that — **the raster going in was produced by an
//! encoder nobody here wrote**, across all fifteen legal colour-type/depth
//! pairs, interlaced and not — and
//! [`every_file_in_the_suite_survives_a_pass_through_this_encoder`] is that leg.
//!
//! It is not enough on its own, and the reason is specific rather than
//! rhetorical. `png/encode.rs` filters with `predictors.rs`'s Paeth predictor
//! and `png.rs` unfilters with the same function; a defect in it moves both
//! directions together and a decode-encode-decode comparison cannot see one.
//! So [`the_encoders_output_unfilters_by_clause_9_2s_own_formulas`] transcribes
//! 9.2's five **reconstruction** formulas into this file — `Recon(a)`,
//! `Recon(b)`, `Recon(c)` and `PaethPredictor` written out from the clause —
//! and rebuilds the raster from the encoder's own IDAT without calling
//! [`png_decode`] at all. First-party, which ruling 13 requires; a second
//! *implementation* rather than a second *program*, which is the line that
//! ruling draws.
//!
//! The two catch different things, and the injection matrix in the commit that
//! added them is the measurement rather than the claim — including where it
//! contradicted the first draft of this paragraph. A filter emitted under the
//! wrong type byte is caught by both. A **reversed Paeth tie-break is invisible
//! to leg one**: all 162 round trips stay green, because the encoder filters
//! and the decoder unfilters through the same reversed function. Leg two
//! catches it. So do two of the decoder tests already in this file —
//! [`every_interlaced_twin_decodes_to_its_non_interlaced_original`] and
//! [`the_published_equivalence_classes_decode_to_one_image_each`] — and they
//! catch it for the same structural reason, which is worth naming: they compare
//! two *third-party* files against each other rather than against anything this
//! repository wrote, so the shared defect has nothing to cancel against. Three
//! of 2 014 in all, and not one of them a round trip.
//!
//! One thing neither leg reaches: **16-bit precision**. `png_encode` writes
//! eight bits a component because `tinker_pdf::Bitmap` has no 16-bit format,
//! so the four colour types Table 11.1 permits at 16 are narrowed to their
//! high bytes before the comparison, and the round trip below is at that
//! precision. Stated at the assertion as well as here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tinker_pdf_filters::{
    crc32, flate_decode, png_decode, png_encode, png_scan, zlib_compress, Limits, PngColour,
    PngError, PngImage, PngSource, PNG_SIGNATURE,
};

/// Printed once per test that actually read the corpus. CI greps for it.
const RAN: &str = "pngsuite-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "pngsuite-oracle: SKIPPED";

/// Comfortably above the 256 x 256 montage, which is the largest file present.
const CAP: Limits = Limits::new(1 << 24);

/// The fourteen files PngSuite publishes as corrupted, with the corruption its
/// author documented for each.
///
/// Every one of them must be refused. They are listed with their published
/// descriptions rather than only their names, because the point of the test is
/// that an independent party decided what "broken" means here.
const BROKEN: [(&str, &str); 14] = [
    ("xs1n0g01", "signature byte 1 MSBit reset to zero"),
    ("xs2n0g01", "signature byte 2 is a 'Q'"),
    ("xs4n0g01", "signature byte 4 lowercase"),
    ("xs7n0g01", "7th byte a space instead of control-Z"),
    ("xcrn0g04", "added cr bytes"),
    ("xlfn0g04", "added lf bytes"),
    ("xhdn0g08", "incorrect IHDR checksum"),
    ("xc1n0g08", "colour type 1"),
    ("xc9n2c08", "colour type 9"),
    ("xd0n2c08", "bit-depth 0"),
    ("xd3n2c08", "bit-depth 3"),
    ("xd9n2c08", "bit-depth 99"),
    ("xdtn0g01", "missing IDAT chunk"),
    ("xcsn0g01", "incorrect IDAT checksum"),
];

fn suite() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TINKER_PNGSUITE")?);
    // A directory that exists but holds no PNGs is a mis-set variable, not a
    // corpus, and treating it as one would make every test below vacuous.
    dir.join("basn0g01.png").is_file().then_some(dir)
}

/// Runs `body` with the corpus, or prints [`SKIPPED`] and returns.
macro_rules! with_suite {
    ($what:expr, |$dir:ident| $body:block) => {
        match suite() {
            Some($dir) => {
                $body
                println!("{} {} ({})", RAN, $what, $dir.display());
            }
            None => {
                println!(
                    "{} {} (set TINKER_PNGSUITE to the extracted PngSuite directory)",
                    SKIPPED, $what
                );
            }
        }
    };
}

fn read(dir: &std::path::Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(format!("{name}.png"))).unwrap_or_else(|e| panic!("{name}.png: {e}"))
}

fn decode(dir: &std::path::Path, name: &str) -> PngImage {
    png_decode(&read(dir, name), &CAP).unwrap_or_else(|e| panic!("{name}.png: {e}"))
}

/// Every `.png` in the directory, by stem.
fn all(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("PngSuite directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// The whole corpus, split the way its author split it — and the count, which
/// gap 29 milestone 3 records.
///
/// The two numbers are asserted rather than merely printed, because "162 files
/// decoded" is only evidence if the run that produced it read all 176.
#[test]
fn the_whole_suite_splits_into_the_files_its_author_says_are_broken_and_the_rest() {
    with_suite!("whole-suite", |dir| {
        let names = all(&dir);
        let mut decoded = 0usize;
        let mut refused: Vec<(String, PngError)> = Vec::new();
        for name in &names {
            match png_decode(&read(&dir, name), &CAP) {
                Ok(img) => {
                    decoded += 1;
                    // Ruling 10, on 162 files nobody here wrote: a well-formed
                    // PNG must take **no** leniency at all. A decoder that
                    // warned routinely would make "it opened cleanly" a
                    // sentence with no content for this format.
                    assert!(
                        img.warnings.is_empty(),
                        "{name} is well formed and produced {:?}",
                        img.warnings
                    );
                    assert!(img.complete, "{name} decoded short");
                    assert_eq!(
                        img.data.len() as u64,
                        u64::from(img.width)
                            * u64::from(img.height)
                            * u64::from(img.colour.components())
                            * u64::from(img.bits_per_component / 8),
                        "{name}: raster is not its own declared size"
                    );
                }
                Err(e) => refused.push((name.clone(), e)),
            }
        }

        let broken: Vec<String> = refused.iter().map(|(n, _)| n.clone()).collect();
        let mut published: Vec<String> = BROKEN.iter().map(|(n, _)| (*n).to_owned()).collect();
        published.sort();
        assert_eq!(
            broken, published,
            "the refused set is not the published one"
        );
        assert_eq!(names.len(), 176, "the 2017jul19 release holds 176 images");
        assert_eq!(decoded, 162);
        assert_eq!(refused.len(), 14);
    });
}

/// Each broken file, refused **by a name that matches the published reason**.
///
/// Refusing all fourteen for the same reason would pass a test that only
/// counted them, and gap 17's finding was that naming the refusal is the
/// feature — so the four signature corruptions have to be signature failures,
/// the two checksum ones have to be checksum failures, and so on.
#[test]
fn every_file_published_as_broken_is_refused_for_the_published_reason() {
    with_suite!("broken-files", |dir| {
        for (name, why) in BROKEN {
            let err = png_decode(&read(&dir, name), &CAP)
                .err()
                .unwrap_or_else(|| panic!("{name} ({why}) decoded"));
            let matched = match name {
                // 5.2, and the two mangling cases the signature exists to catch.
                "xs1n0g01" | "xs2n0g01" | "xs4n0g01" | "xs7n0g01" | "xcrn0g04" | "xlfn0g04" => {
                    err == PngError::NotPng
                }
                "xhdn0g08" => matches!(err, PngError::ChunkCrc(t) if t.0 == *b"IHDR"),
                "xcsn0g01" => matches!(err, PngError::ChunkCrc(t) if t.0 == *b"IDAT"),
                "xdtn0g01" => err == PngError::MissingImageData,
                "xc1n0g08" => {
                    err == PngError::BadColourTypeDepth {
                        colour_type: 1,
                        bit_depth: 8,
                    }
                }
                "xc9n2c08" => {
                    err == PngError::BadColourTypeDepth {
                        colour_type: 9,
                        bit_depth: 8,
                    }
                }
                "xd0n2c08" => {
                    err == PngError::BadColourTypeDepth {
                        colour_type: 2,
                        bit_depth: 0,
                    }
                }
                "xd3n2c08" => {
                    err == PngError::BadColourTypeDepth {
                        colour_type: 2,
                        bit_depth: 3,
                    }
                }
                "xd9n2c08" => {
                    err == PngError::BadColourTypeDepth {
                        colour_type: 2,
                        bit_depth: 99,
                    }
                }
                _ => unreachable!(),
            };
            assert!(matched, "{name} ({why}) was refused as {err}");
        }
    });
}

/// PngSuite's filename convention is its header reference: characters 4 to 7
/// are the colour type, a letter for its family, and the bit depth; character 3
/// is `n` or `i` for the interlace method.
///
/// So every file in the corpus carries an independent statement of what this
/// decoder's IHDR parse should produce, and there are 162 of them.
#[test]
fn the_filename_convention_agrees_with_the_header_this_decoder_read() {
    with_suite!("filename-convention", |dir| {
        let mut checked = 0usize;
        for name in all(&dir) {
            if BROKEN.iter().any(|(b, _)| *b == name) {
                continue;
            }
            let b = name.as_bytes();
            let Some(&[ct, family, d0, d1]) = b.get(4..8) else {
                continue;
            };
            if !ct.is_ascii_digit() || !d0.is_ascii_digit() || !d1.is_ascii_digit() {
                continue; // `PngSuite.png`, the montage, is not in the scheme.
            }
            let colour_type = ct - b'0';
            let bit_depth = (d0 - b'0') * 10 + (d1 - b'0');
            let expect_family = match colour_type {
                0 => b'g',
                2 => b'c',
                3 => b'p',
                _ => b'a',
            };
            assert_eq!(family, expect_family, "{name}: family letter");

            let header = png_scan(&read(&dir, &name)).expect("valid").header;
            assert_eq!(header.colour_type, colour_type, "{name}: colour type");
            assert_eq!(header.bit_depth, bit_depth, "{name}: bit depth");
            match b[3] {
                b'n' => assert!(!header.interlaced, "{name} is named non-interlaced"),
                b'i' => assert!(header.interlaced, "{name} is named interlaced"),
                // `exif2c08` and `PngSuite` do not carry the interlace letter.
                _ => {}
            }
            checked += 1;
        }
        assert_eq!(checked, 161, "one file, the montage, is outside the scheme");
    });
}

/// The fifteen `bas*` pairs are Table 11.1's fifteen legal pairings, and this
/// asserts the *coverage* rather than any one file: a decoder missing a pair
/// would still pass every test above by refusing nothing.
#[test]
fn the_base_images_cover_every_one_of_table_11_1s_fifteen_pairs() {
    with_suite!("table-11-1-coverage", |dir| {
        let mut seen: Vec<(u8, u8)> = Vec::new();
        for name in all(&dir) {
            if !name.starts_with("basn") {
                continue;
            }
            let h = png_scan(&read(&dir, &name)).expect("valid").header;
            seen.push((h.colour_type, h.bit_depth));
        }
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![
                (0, 1),
                (0, 2),
                (0, 4),
                (0, 8),
                (0, 16),
                (2, 8),
                (2, 16),
                (3, 1),
                (3, 2),
                (3, 4),
                (3, 8),
                (4, 8),
                (4, 16),
                (6, 8),
                (6, 16),
            ]
        );
    });
}

/// **Adam7, checked against files this repository did not produce.**
///
/// `basn*` and `basi*` are the same image, non-interlaced and interlaced, at
/// every one of the fifteen legal pairings — so fifteen independent encoders'
/// worth of interlacing has to land on the same raster as fifteen ordinary
/// ones. The committed 8 x 8 fixture in `src/png/tests.rs` catches a transposed
/// pass; this catches everything a 32 x 32 image can express that an 8 x 8
/// cannot, including passes whose reduced width is not a whole number of bytes
/// at depths 1, 2 and 4.
#[test]
fn every_interlaced_twin_decodes_to_its_non_interlaced_original() {
    with_suite!("adam7-twins", |dir| {
        let mut pairs = 0usize;
        for name in all(&dir) {
            let Some(rest) = name.strip_prefix("basi") else {
                continue;
            };
            let plain = decode(&dir, &format!("basn{rest}"));
            let woven = decode(&dir, &name);
            assert_eq!(plain.width, woven.width, "{name}");
            assert_eq!(plain.colour, woven.colour, "{name}");
            assert_eq!(plain.bits_per_component, woven.bits_per_component, "{name}");
            assert_eq!(plain.data, woven.data, "{name}: interlacing changed pixels");
            pairs += 1;
        }
        assert_eq!(pairs, 15, "one twin per legal colour-type/depth pair");

        // The size series, where the small images are the ones on which whole
        // passes fall away: at 1 x 1 only the first of the seven holds a pixel.
        let mut sizes = 0usize;
        for name in all(&dir) {
            if !name.starts_with('s') || name.as_bytes().get(3) != Some(&b'i') {
                continue;
            }
            let mut twin = name.clone().into_bytes();
            twin[3] = b'n';
            let plain = decode(&dir, &String::from_utf8(twin).expect("ascii"));
            let woven = decode(&dir, &name);
            assert_eq!(plain.data, woven.data, "{name}: interlacing changed pixels");
            // And the number in the name is the published size.
            let n: u32 = name[1..3].parse().expect("two digits");
            assert_eq!((woven.width, woven.height), (n, n), "{name}");
            sizes += 1;
        }
        assert_eq!(sizes, 18);
    });
}

/// The published equivalence classes: same image, different container detail.
///
/// The `oi*` group is the one this milestone's exit criteria name — one zlib
/// stream spanning several IDAT chunks — and it is here rather than only in the
/// hand-built test because `oi9n0g16` cuts the stream into chunks of **one
/// byte**, which no fixture author would think to write.
#[test]
fn the_published_equivalence_classes_decode_to_one_image_each() {
    with_suite!("equivalence-classes", |dir| {
        const CLASSES: [(&str, &[&str]); 10] = [
            // 10.3: one zlib stream, cut into 1, 2, 4 unequal, and n one-byte
            // chunks.
            (
                "IDAT chunking, greyscale 16-bit",
                &["oi1n0g16", "oi2n0g16", "oi4n0g16", "oi9n0g16"],
            ),
            (
                "IDAT chunking, truecolour 16-bit",
                &["oi1n2c16", "oi2n2c16", "oi4n2c16", "oi9n2c16"],
            ),
            // Deflate levels 0, 3, 6 and 9 of the same raster.
            (
                "compression level",
                &["z00n2c08", "z03n2c08", "z06n2c08", "z09n2c08"],
            ),
            // Ancillary chunks a decoder must ignore: bKGD, sPLT, pHYs, hIST,
            // tEXt, zTXt and tIME.
            (
                "bKGD on grey+alpha 8",
                &["basn4a08", "bgbn4a08", "bgai4a08", "basi4a08"],
            ),
            (
                "bKGD on grey+alpha 16",
                &["basn4a16", "bggn4a16", "bgai4a16", "basi4a16"],
            ),
            ("bKGD on RGBA 8", &["basn6a08", "bgwn6a08", "bgan6a08"]),
            ("bKGD on RGBA 16", &["basn6a16", "bgyn6a16", "bgan6a16"]),
            ("sPLT", &["basn0g08", "ps1n0g08", "ps2n0g08"]),
            (
                "sPLT and pHYs on 16-bit colour",
                &["basn2c16", "ps1n2c16", "ps2n2c16", "pp0n2c16"],
            ),
            // hIST beside the palette it describes, and text/time chunks.
            ("hIST, tEXt, zTXt and tIME", &["basn3p04", "ch1n3p04"]),
        ];
        let mut classes = 0usize;
        for (what, members) in CLASSES {
            let first = decode(&dir, members[0]);
            for m in &members[1..] {
                let other = decode(&dir, m);
                assert_eq!(
                    first.data, other.data,
                    "{what}: {} and {m} are the same image",
                    members[0]
                );
                assert_eq!(first.colour, other.colour, "{what}: {m}");
            }
            classes += 1;
        }
        assert_eq!(classes, 10);

        // The text and timestamp files, which are one class among themselves
        // rather than equal to a `bas*` image.
        let text = [
            "ct0n0g04", "ct1n0g04", "ctzn0g04", "cm0n0g04", "cm7n0g04", "cm9n0g04",
        ];
        let first = decode(&dir, text[0]);
        for m in &text[1..] {
            assert_eq!(first.data, decode(&dir, m).data, "tEXt/zTXt/tIME: {m}");
        }
    });
}

/// `tRNS` in all three of its forms, on files somebody else wrote — and, just
/// as importantly, its **absence** on the three files PngSuite ships "for
/// reference" precisely so a decoder that invents an alpha channel is caught.
#[test]
fn the_transparency_files_produce_alpha_exactly_where_they_say_they_do() {
    with_suite!("transparency", |dir| {
        // 11.3.2.1 form 1: a grey key. Form 2: an RGB key. Form 3: per-entry
        // palette alpha, which is the one gap 29's table sends here rather than
        // through the pass-through, since PDF's /Mask cannot express it.
        const KEYED: [(&str, PngColour); 10] = [
            ("tbbn0g04", PngColour::GreyAlpha),
            ("tbwn0g16", PngColour::GreyAlpha),
            ("tbrn2c08", PngColour::Rgba),
            ("tbbn2c16", PngColour::Rgba),
            ("tbgn2c16", PngColour::Rgba),
            ("tbbn3p08", PngColour::Rgba),
            ("tbgn3p08", PngColour::Rgba),
            ("tbwn3p08", PngColour::Rgba),
            ("tbyn3p08", PngColour::Rgba),
            ("tm3n3p02", PngColour::Rgba),
        ];
        for (name, want) in KEYED {
            let img = decode(&dir, name);
            assert_eq!(img.colour, want, "{name}");
            let n = img.colour.components() as usize;
            let step = n * usize::from(img.bits_per_component / 8);
            let alpha_at = step - usize::from(img.bits_per_component / 8);
            let transparent = img
                .data
                .chunks_exact(step)
                .filter(|p| p[alpha_at] == 0)
                .count();
            assert!(
                transparent > 0,
                "{name} carries tRNS and produced no transparent pixel"
            );
            assert!(
                transparent < img.data.len() / step,
                "{name} produced nothing but transparent pixels"
            );
        }

        // PngSuite's own "not transparent, for reference" files. A decoder that
        // manufactured an alpha channel would pass every assertion above.
        for name in ["tp0n0g08", "tp0n2c08", "tp0n3p08"] {
            let img = decode(&dir, name);
            assert!(
                matches!(img.colour, PngColour::Grey | PngColour::Rgb),
                "{name} has no tRNS and came back as {:?}",
                img.colour
            );
            assert!(png_scan(&read(&dir, name))
                .expect("valid")
                .transparency
                .is_none());
        }
        // "transparent, but no background chunk" — a tRNS with no bKGD beside
        // it is still a tRNS.
        assert_eq!(decode(&dir, "tp1n3p08").colour, PngColour::Rgba);
    });
}

/// Two images that are *not* the same in any group above, which is what makes
/// every equality assertion here mean something.
///
/// A decoder returning one constant raster for every input would satisfy all
/// forty-odd equalities above and nothing would say so — the failure mode gap
/// 17 named for a comparative test.
///
/// The measured figure is **91 distinct rasters from 162 files**, and the
/// duplication is PngSuite's own: the equivalence classes above account for
/// most of it, and the `s01`..`s40` series repeats one pattern at nineteen
/// sizes. The assertion is the floor rather than the measurement, because a
/// later PngSuite release may add files; a decoder that collapsed would fall
/// through it by an order of magnitude.
#[test]
fn the_suite_does_not_collapse_into_one_picture() {
    with_suite!("distinctness", |dir| {
        let mut seen: BTreeMap<Vec<u8>, String> = BTreeMap::new();
        let mut valid = 0usize;
        for name in all(&dir) {
            if let Ok(img) = png_decode(&read(&dir, &name), &CAP) {
                valid += 1;
                seen.entry(img.data).or_insert(name);
            }
        }
        assert_eq!(valid, 162);
        assert!(
            seen.len() >= 80,
            "only {} distinct rasters from {valid} files",
            seen.len()
        );
    });
}

// --- the encoder --------------------------------------------------------

/// One decoded PngSuite image as `png_encode` takes it: eight bits a component,
/// tightly packed.
///
/// **The narrowing is the 16-bit statement.** Four of Table 11.1's colour types
/// permit 16-bit components and PngSuite ships files that use them; this writer
/// emits eight, because `tinker_pdf::Bitmap` — the thing it exists to serialise
/// — stores a byte a channel. So a 16-bit sample is reduced to its high byte,
/// and every comparison below is against the narrowed raster rather than the
/// original. The round trip is at `Bitmap` precision and says so.
fn narrow(image: &PngImage) -> Vec<u8> {
    if image.bits_per_component == 8 {
        return image.data.clone();
    }
    image.data.chunks_exact(2).map(|p| p[0]).collect()
}

fn source<'a>(image: &PngImage, eight: &'a [u8]) -> PngSource<'a> {
    PngSource {
        width: image.width,
        height: image.height,
        colour: image.colour,
        stride: image.width as usize * image.colour.components() as usize,
        data: eight,
    }
}

/// 5.3's chunk structure, walked here rather than through `png_scan`.
///
/// Every CRC is recomputed from the **type and the data**, which is what 5.3
/// covers and not the length, and the walk asserts the file ends exactly where
/// its last chunk does. Asking `png_scan` instead would be asking one of the
/// two halves under test what it made of the other.
fn chunks(file: &[u8], name: &str) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&file[..8], &PNG_SIGNATURE, "{name}: 5.2's signature");
    let mut out = Vec::new();
    let mut at = 8usize;
    while at + 12 <= file.len() {
        let len = u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let kind = [file[at + 4], file[at + 5], file[at + 6], file[at + 7]];
        let data = file[at + 8..at + 8 + len].to_vec();
        let declared = u32::from_be_bytes([
            file[at + 8 + len],
            file[at + 9 + len],
            file[at + 10 + len],
            file[at + 11 + len],
        ]);
        let mut covered = Vec::from(kind);
        covered.extend_from_slice(&data);
        assert_eq!(
            declared,
            crc32(&covered),
            "{name}: chunk {} carries the wrong CRC",
            String::from_utf8_lossy(&kind)
        );
        out.push((kind, data));
        at += 12 + len;
    }
    assert_eq!(at, file.len(), "{name}: a chunk ran off the end");
    assert_eq!(out.first().map(|(k, _)| *k), Some(*b"IHDR"), "{name}: 5.6");
    assert_eq!(out.last().map(|(k, _)| *k), Some(*b"IEND"), "{name}: 5.6");
    assert!(
        out.last().is_some_and(|(_, d)| d.is_empty()),
        "{name}: IEND carries data"
    );
    out
}

/// The IDAT payload — every chunk's data concatenated, as 10.3 allows one zlib
/// stream to be split — and the filtered scanlines inside it.
fn idat_and_stream(file: &[u8], name: &str) -> (Vec<u8>, Vec<u8>) {
    let mut idat = Vec::new();
    for (kind, data) in chunks(file, name) {
        if &kind == b"IDAT" {
            idat.extend_from_slice(&data);
        }
    }
    let decoded = flate_decode(&idat, &CAP, None).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(decoded.complete, "{name}: the IDAT did not inflate whole");
    assert!(
        decoded.warnings.is_empty(),
        "{name}: this writer's own zlib stream took leniency: {:?}",
        decoded.warnings
    );
    (idat, decoded.data)
}

/// **ISO/IEC 15948 9.2, Table 9.1 — the reconstruction formulas, transcribed.**
///
/// This is the second implementation, and it is the whole reason the round trip
/// below means anything. `png.rs` unfilters through `predictors.rs`, which is
/// also where `png/encode.rs` gets its Paeth predictor; a defect shared by the
/// two directions cancels exactly and leaves a decode-encode-decode comparison
/// perfectly green. Nothing here calls into that code.
///
/// `Recon(a)` is the byte `bpp` to the left of the current one **in the row
/// being reconstructed**, `Recon(b)` the one directly above it in the previous
/// reconstructed row, and `Recon(c)` the one above and to the left; each is
/// zero where it falls outside the image. All arithmetic is modulo 256, which
/// 9.2 states.
fn unfilter_by_clause_9_2(stream: &[u8], width: usize, height: usize, bpp: usize) -> Vec<u8> {
    let row_bytes = width * bpp;
    let mut out = vec![0u8; row_bytes * height];
    let mut prior = vec![0u8; row_bytes];
    let mut at = 0usize;
    for y in 0..height {
        let kind = stream[at];
        let filt = &stream[at + 1..at + 1 + row_bytes];
        at += 1 + row_bytes;
        let mut recon = vec![0u8; row_bytes];
        for i in 0..row_bytes {
            let a = if i >= bpp { recon[i - bpp] } else { 0 };
            let b = prior[i];
            let c = if i >= bpp { prior[i - bpp] } else { 0 };
            recon[i] = match kind {
                // Recon(x) = Filt(x)
                0 => filt[i],
                // Recon(x) = Filt(x) + Recon(a)
                1 => filt[i].wrapping_add(a),
                // Recon(x) = Filt(x) + Recon(b)
                2 => filt[i].wrapping_add(b),
                // Recon(x) = Filt(x) + floor((Recon(a) + Recon(b)) / 2)
                3 => filt[i].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                // Recon(x) = Filt(x) + PaethPredictor(Recon(a), Recon(b),
                // Recon(c)), with the predictor written out from the clause:
                // p = a + b - c, then whichever of a, b and c is nearest p,
                // ties going to a and then to b — the order 9.2 gives them in,
                // and the half of the rule a reimplementation gets wrong.
                4 => {
                    let (ai, bi, ci) = (i32::from(a), i32::from(b), i32::from(c));
                    let p = ai + bi - ci;
                    let (pa, pb, pc) = ((p - ai).abs(), (p - bi).abs(), (p - ci).abs());
                    let pred = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    filt[i].wrapping_add(pred)
                }
                other => panic!("filter type {other}, which 9.2 does not define"),
            };
        }
        out[y * row_bytes..(y + 1) * row_bytes].copy_from_slice(&recon);
        prior = recon;
    }
    assert_eq!(at, stream.len(), "the filtered stream has bytes left over");
    out
}

/// **Leg one: every file in the suite makes the round trip.**
///
/// Decode a file somebody else produced, encode the raster, decode that, and
/// require the pixels to be the ones the first decode gave. 162 files, all
/// fifteen legal colour-type/bit-depth pairs, interlaced and not — and the
/// fourteen broken by design stay refused, counted here as well so that a
/// change making the decoder tolerant cannot quietly widen the set.
///
/// The container is asserted against the specification rather than against the
/// reader: the signature, IHDR first with 11.2.2's thirteen bytes, IEND last
/// and empty, every chunk's CRC recomputed over its type and its data, and the
/// IDAT payload equal to what `zlib_compress` makes of the filtered stream.
#[test]
fn every_file_in_the_suite_survives_a_pass_through_this_encoder() {
    with_suite!("encoder-round-trip", |dir| {
        let mut round_tripped = 0usize;
        let mut refused = 0usize;
        let mut sixteen_bit = 0usize;
        for name in all(&dir) {
            let Ok(first) = png_decode(&read(&dir, &name), &CAP) else {
                refused += 1;
                continue;
            };
            if first.bits_per_component == 16 {
                sixteen_bit += 1;
            }
            let eight = narrow(&first);
            let file = png_encode(&source(&first, &eight))
                .unwrap_or_else(|e| panic!("{name}: this raster would not encode: {e}"));

            let (idat, stream) = idat_and_stream(&file, &name);
            assert_eq!(
                idat,
                zlib_compress(&stream),
                "{name}: the IDAT is not zlib_compress of its own filtered stream"
            );
            let walked = chunks(&file, &name);
            let ihdr = &walked[0].1;
            assert_eq!(ihdr.len(), 13, "{name}: 11.2.2's thirteen bytes");
            assert_eq!(&ihdr[0..4], &first.width.to_be_bytes(), "{name}: width");
            assert_eq!(&ihdr[4..8], &first.height.to_be_bytes(), "{name}: height");
            assert_eq!(ihdr[8], 8, "{name}: bit depth");
            // Table 11.1's own numbering, against the layout the first decode
            // reported rather than against anything the encoder said.
            let want = match first.colour {
                PngColour::Grey => 0u8,
                PngColour::Rgb => 2,
                PngColour::GreyAlpha => 4,
                PngColour::Rgba => 6,
            };
            assert_eq!(ihdr[9], want, "{name}: colour type");
            assert_eq!(&ihdr[10..13], &[0, 0, 0], "{name}: the three methods");

            let second = png_decode(&file, &CAP)
                .unwrap_or_else(|e| panic!("{name}: our own output did not read back: {e}"));
            assert_eq!(second.width, first.width, "{name}");
            assert_eq!(second.height, first.height, "{name}");
            assert_eq!(second.colour, first.colour, "{name}");
            assert_eq!(second.bits_per_component, 8, "{name}");
            assert!(second.complete, "{name}");
            // Ruling 10: a file this engine wrote must take no leniency from
            // this engine's reader.
            assert!(
                second.warnings.is_empty(),
                "{name}: re-read with {:?}",
                second.warnings
            );
            assert_eq!(
                second.data, eight,
                "{name}: the round trip changed the picture \
                 (at eight bits a component, which is all this writer emits)"
            );
            round_tripped += 1;
        }
        assert_eq!(round_tripped, 162, "every decodable file round-trips");
        assert_eq!(refused, 14, "the broken fourteen are still refused");
        assert!(
            sixteen_bit > 0,
            "no file was narrowed from 16 bits, so that half of the claim is vacuous"
        );
        println!("  {round_tripped} round-tripped, {sixteen_bit} narrowed from 16 bits to 8");
    });
}

/// **Leg two: the encoder's own bytes, unfiltered by a transcription of 9.2
/// that shares no code with this crate.**
///
/// This is the leg that breaks the symmetry. Leg one puts the output through
/// `png_decode`, which unfilters with the same `predictors.rs` the encoder
/// filters with — so a defect in the predictor cancels and the comparison stays
/// green. Here the filtered stream is rebuilt by [`unfilter_by_clause_9_2`] and
/// compared against the raster that went in.
///
/// It also asserts the **coverage of the filter types**, which is the other
/// thing a round trip cannot see: an encoder emitting type 0 for every row
/// would round-trip perfectly and would leave 9.2's other four formulas
/// untouched on both sides. All five have to be chosen somewhere in the suite,
/// and the tally is printed so a change that collapses the heuristic shows up
/// as a number rather than as a pass.
#[test]
fn the_encoders_output_unfilters_by_clause_9_2s_own_formulas() {
    with_suite!("independent-unfilter", |dir| {
        let mut used = [0usize; 5];
        let mut checked = 0usize;
        for name in all(&dir) {
            let Ok(first) = png_decode(&read(&dir, &name), &CAP) else {
                continue;
            };
            let eight = narrow(&first);
            let file = png_encode(&source(&first, &eight)).expect("a decoded raster encodes");
            let (_, stream) = idat_and_stream(&file, &name);

            let width = first.width as usize;
            let height = first.height as usize;
            let bpp = first.colour.components() as usize;
            assert_eq!(
                stream.len(),
                (width * bpp + 1) * height,
                "{name}: one tag and one row per scanline"
            );
            for y in 0..height {
                let tag = stream[y * (width * bpp + 1)] as usize;
                assert!(tag < 5, "{name}: row {y} is tagged {tag}");
                used[tag] += 1;
            }

            let rebuilt = unfilter_by_clause_9_2(&stream, width, height, bpp);
            assert_eq!(
                rebuilt, eight,
                "{name}: 9.2's reconstruction formulas do not give back the raster"
            );
            checked += 1;
        }
        assert_eq!(checked, 162);
        println!("  rows by 9.2 filter type, None first: {used:?}");
        for (kind, count) in used.iter().enumerate() {
            assert!(
                *count > 0,
                "9.2 filter type {kind} was never chosen, so the round trip \
                 never exercised it: {used:?}"
            );
        }
    });
}
