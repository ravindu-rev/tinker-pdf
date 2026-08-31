//! The committed JPEG XR fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/jxr/` is forty-two inputs — every fixture as a whole Annex A
//! file and again as the bare `CODED_IMAGE( )` inside it — and the target that
//! consumes them needs nightly. So the seeds were only ever exercised when
//! somebody ran `cargo fuzz`, which is not on every commit, and a seed corpus
//! nothing reads is a corpus that stops describing the decoder without anybody
//! noticing.
//!
//! This replays each one through the same knobs and the same assertions
//! `fuzz/fuzz_targets/jxr.rs` makes, minus the mutation. It is not fuzzing and
//! does not pretend to be: it is a regression test over inputs that were once
//! interesting, which is what a seed corpus is. It prints `RAN` or `SKIPPED`
//! for the reason every check that can be absent does
//! ([verification](../../../docs/verification.md)).
//!
//! # Why the seeds are written from the fixtures rather than by hand
//!
//! `jxr_fixtures.rs`'s `write_fuzz_seeds` copies them, so the two cannot
//! disagree about what a JPEG XR looks like. The seeds were rewritten when the
//! fixtures were: the encoder finding recorded in `tests/jxr/README.md` — that
//! `Lossless` does nothing without `QualityLevel` — changed every lossless
//! file, and a corpus still holding the old ones would have been exercising a
//! quantizer the fixtures no longer use.
//!
//! # What this test found on its first run
//!
//! **Forty-two seeds, eighty-four decode attempts, zero successes.** The
//! target reads `data[0]` as knobs and decodes `data[1..]`, and the seeds were
//! written as raw files — so every one of them had its first byte swallowed
//! and its magic broken before the decoder saw it. The corpus exercised the
//! refusal paths and nothing else, which is the half a fuzzer is least short
//! of on its own.
//!
//! `write_fuzz_seeds` now prefixes the knob byte. That is the whole value of
//! replaying a corpus on stable: the seeds looked right in the directory
//! listing, the target ran without complaint, and only counting the successes
//! showed that none of them reached a parser.

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{jxr_decode, JxrError, Limits};

/// The seed directory, from this crate rather than from the working directory.
fn seeds() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jxr");
    dir.is_dir().then_some(dir)
}

/// The target's own reading of its first byte, restated so that a seed means
/// here exactly what it means there.
fn knobs(data: &[u8]) -> (usize, Vec<Vec<u8>>) {
    /// A.5's four-byte file header, as the target spells it.
    const FILE_HEADER: [u8; 8] = [0x49, 0x49, 0xBC, 0x01, 0x08, 0x00, 0x00, 0x00];
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let ceiling = match knobs & 3 {
        0 => 1 << 8,
        1 => 1 << 12,
        2 => 1 << 16,
        _ => 1 << 20,
    };
    let mut inputs: Vec<Vec<u8>> = vec![body.to_vec()];
    if knobs & 4 != 0 {
        let mut wrapped = FILE_HEADER.to_vec();
        wrapped.extend_from_slice(body);
        inputs.push(wrapped);
    }
    if knobs & 8 != 0 {
        let mut bare = b"WMPHOTO\0".to_vec();
        bare.extend_from_slice(body);
        inputs.push(bare);
    }
    (ceiling, inputs)
}

/// Every seed either decodes into a raster its own geometry describes, or
/// refuses with one of [`JxrError`]'s named decisions.
///
/// The two halves are the point. "It did not panic" is ruling 1 and is the
/// least of what a decoder owes; what this adds is that a *success* is
/// self-consistent — the right number of bytes, a depth the output contract
/// admits, a non-empty picture, and no warning recorded twice, which is the
/// dedup ruling 10 asks for so that a file with a million damaged tiles cannot
/// turn leniency into an allocation attack.
#[test]
fn every_committed_seed_decodes_or_refuses_by_name() {
    let Some(dir) = seeds() else {
        println!("jxr-seeds: SKIPPED (no fuzz/corpus/jxr)");
        return;
    };
    let mut files = 0;
    let mut decoded = 0;
    let mut refused = 0;
    let entries = std::fs::read_dir(&dir).expect("the seed directory is committed");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).expect("a committed seed");
        files += 1;
        let (ceiling, inputs) = knobs(&data);
        let limits = Limits::new(ceiling);
        for input in inputs {
            match jxr_decode(&input, &limits) {
                Ok(image) => {
                    let per = usize::from(image.bits_per_component() / 8);
                    let want = (image.width as usize)
                        .checked_mul(image.height as usize)
                        .and_then(|n| n.checked_mul(usize::from(image.channels())))
                        .and_then(|n| n.checked_mul(per));
                    assert_eq!(
                        Some(image.data.len()),
                        want,
                        "{}: a raster that is not the size its geometry implies",
                        path.display()
                    );
                    assert!(
                        image.data.len() <= ceiling,
                        "{}: the output ceiling was exceeded",
                        path.display()
                    );
                    assert!(
                        matches!(image.bits_per_component(), 8 | 16),
                        "{}: an output depth that is neither 8 nor 16",
                        path.display()
                    );
                    assert!(
                        image.width > 0 && image.height > 0,
                        "{}: a zero-sized success",
                        path.display()
                    );
                    let mut seen: Vec<&str> = image.warnings.iter().map(|w| w.as_str()).collect();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(
                        seen.len(),
                        image.warnings.len(),
                        "{}: a warning was recorded more than once",
                        path.display()
                    );
                    decoded += 1;
                }
                Err(error) => {
                    // A closed enum of decisions. The match is exhaustive on
                    // purpose: adding a variant without deciding whether a
                    // fuzzer may reach it should fail to compile here as well
                    // as in the target.
                    match error {
                        JxrError::NotJxr
                        | JxrError::UnsupportedFileVersion(_)
                        | JxrError::UnsupportedCodestreamVersion(_)
                        | JxrError::Truncated
                        | JxrError::MissingRequiredTag(_)
                        | JxrError::ReservedValue(_)
                        | JxrError::BadDimensions
                        | JxrError::BadTiling
                        | JxrError::BadIndexTable
                        | JxrError::BadProfileLevel
                        | JxrError::BadAlphaPlane
                        | JxrError::BadTileStartCode(_)
                        | JxrError::TooManySamples { .. }
                        | JxrError::TooManyTiles { .. }
                        | JxrError::TooManyComponents { .. }
                        | JxrError::TooManyMacroblocks { .. }
                        | JxrError::ExceedsOutputLimit { .. }
                        | JxrError::Unsupported(_) => {}
                    }
                    refused += 1;
                }
            }
        }
    }
    // A corpus that quietly emptied would otherwise pass every assertion
    // above, which is the failure mode `docs/verification.md` keeps the RAN
    // discipline for.
    assert!(files > 0, "the committed seed corpus is empty");
    // **The count that matters.** See the module header: a corpus whose every
    // seed refuses is one the decoder never runs on, and it looks identical
    // to a healthy one from the outside.
    assert!(
        decoded > 0,
        "not one seed decoded: the corpus exercises only the refusal paths"
    );
    println!("RAN jxr-seeds: {files} seeds, {decoded} decoded, {refused} refused");
}

/// The corpus still holds both entry points.
///
/// `jxr_decode` takes a whole Annex A file *and* a bare `CODED_IMAGE( )`, and
/// the two take different paths through the front of the decoder — one walks a
/// directory of file offsets, the other goes straight to clause 8. A corpus
/// that lost one half would keep passing the replay above while covering half
/// as much, which is exactly the kind of silent narrowing a seed count does
/// not catch.
#[test]
fn the_corpus_holds_both_of_the_decoders_entry_points() {
    let Some(dir) = seeds() else {
        println!("jxr-seeds: SKIPPED (no fuzz/corpus/jxr)");
        return;
    };
    let mut files = 0;
    let mut codestreams = 0;
    for entry in std::fs::read_dir(&dir)
        .expect("the seed directory")
        .flatten()
    {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".jxr") {
            files += 1;
        } else if name.ends_with(".codestream") {
            codestreams += 1;
        }
    }
    assert!(files > 0, "no Annex A files in the corpus");
    assert!(codestreams > 0, "no bare codestreams in the corpus");
    assert_eq!(
        files, codestreams,
        "every fixture should contribute one of each"
    );
}
