//! The committed JPEG 2000 fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/jpx/` is twenty-five inputs and the target that consumes them
//! needs nightly, so the seeds were only ever exercised when somebody ran
//! `cargo fuzz`. A seed corpus nothing reads is a corpus that stops describing
//! the decoder without anybody noticing — the argument `jbig2_seeds.rs` makes
//! first and `jxr_seeds.rs` makes again.
//!
//! This replays each one through the same knob byte and the same assertions
//! `fuzz/fuzz_targets/jpx.rs` makes, minus the mutation. It is not fuzzing and
//! does not pretend to be: it is a regression test over inputs that were once
//! interesting.
//!
//! # What it found
//!
//! **Six of the twenty-five seeds never reached the decoder.** The target
//! reads `data[0]` as knobs and decodes `data[1..]`; nineteen seeds were
//! written with that byte and six — every one of them a whole JP2 file, added
//! later than the rest — were written raw. Those six had their first byte
//! swallowed, which left `00 00 0c 6a 50 20 20 …` where Annex I's signature
//! box should be, so the box walk refused them on the length field before any
//! codestream was read.
//!
//! Nothing said so. The corpus listing looked right, the seed count was
//! right, and `cargo fuzz` ran clean — because a refusal is a perfectly good
//! outcome for a fuzz target and twenty-four per cent of the corpus producing
//! one is invisible unless something counts.
//!
//! This is the same defect `fuzz/corpus/jxr` had, found the same way, which is
//! why `cargo run -p xtask -- fuzz` now checks the whole `fuzz/` tree for it
//! mechanically rather than leaving it to whoever writes the next corpus.

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{jpx_decode, Capability, FilterError, Limits};

/// The seed directory, from this crate rather than from the working directory.
fn seeds() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jpx");
    dir.is_dir().then_some(dir)
}

/// Annex I's JP2 signature box, as the target spells it.
const SIGNATURE: [u8; 12] = [0, 0, 0, 12, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A];

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// The target's own reading of its first byte, restated so a seed means here
/// exactly what it means there.
fn inputs_of(data: &[u8]) -> (usize, Vec<Vec<u8>>) {
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
        let mut jp2 = SIGNATURE.to_vec();
        jp2.extend_from_slice(&boxed(b"ftyp", b"jp2 \x00\x00\x00\x00jp2 "));
        jp2.extend_from_slice(&boxed(
            b"jp2h",
            &boxed(b"ihdr", &[0, 0, 0, 4, 0, 0, 0, 4, 0, 1, 7, 7, 0, 0]),
        ));
        jp2.extend_from_slice(&boxed(b"jp2c", body));
        inputs.push(jp2);
    }
    (ceiling, inputs)
}

/// Every seed either decodes into an image its own geometry describes, or
/// refuses with the one named capability this codec has.
///
/// The count of *decodes* is the assertion that matters. "It did not panic" is
/// ruling 1 and is the least a decoder owes; a corpus in which nothing decodes
/// is one the tier-1 coder, the packet walk and the wavelet never run on, and
/// it passes the panic check exactly as well as a healthy one.
#[test]
fn every_committed_seed_decodes_or_refuses_by_name() {
    let Some(dir) = seeds() else {
        println!("jpx-seeds: SKIPPED (no fuzz/corpus/jpx)");
        return;
    };
    let mut files = 0;
    let mut decoded = 0;
    let mut refused = 0;
    for entry in std::fs::read_dir(&dir)
        .expect("the seed directory")
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).expect("a committed seed");
        files += 1;
        let (ceiling, inputs) = inputs_of(&data);
        let limits = Limits::new(ceiling);
        for input in inputs {
            let mut warnings = Vec::new();
            match jpx_decode(&input, &limits, &mut warnings) {
                Ok(image) => {
                    let per = usize::from(image.precision > 8) + 1;
                    let want = (image.width as usize)
                        .checked_mul(image.height as usize)
                        .and_then(|n| n.checked_mul(image.components as usize))
                        .and_then(|n| n.checked_mul(per));
                    assert_eq!(
                        Some(image.samples.len()),
                        want,
                        "{}: samples that are not the size the geometry implies",
                        path.display()
                    );
                    assert!(
                        image.samples.len() <= ceiling,
                        "{}: the output ceiling was exceeded",
                        path.display()
                    );
                    assert!(
                        matches!(image.precision, 8 | 16),
                        "{}: a precision that is neither 8 nor 16",
                        path.display()
                    );
                    decoded += 1;
                }
                Err(error) => {
                    assert_eq!(
                        error,
                        FilterError::Unsupported(Capability::Jpx),
                        "{}: failed with something other than the refusal",
                        path.display()
                    );
                    assert!(
                        !warnings.is_empty(),
                        "{}: a refusal left no warning (ruling 10)",
                        path.display()
                    );
                    refused += 1;
                }
            }
            let mut seen: Vec<&str> = warnings.iter().map(|w| w.as_str()).collect();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(
                seen.len(),
                warnings.len(),
                "{}: a warning was recorded more than once",
                path.display()
            );
        }
    }
    assert!(files > 0, "the committed seed corpus is empty");
    // **The count that matters.** See the module header: this is the check
    // that was missing when six seeds were reaching nothing.
    assert!(
        decoded > 0,
        "not one seed decoded: the corpus exercises only the refusal paths"
    );
    println!("RAN jpx-seeds: {files} seeds, {decoded} decoded, {refused} refused");
}

/// Every seed carries the knob byte the target eats.
///
/// The narrower, sharper form of the check above: a seed whose body starts
/// with Annex I's signature box has had its knob byte forgotten, and the
/// decode count would only fall by one — which a threshold of "more than zero"
/// would never notice.
#[test]
fn every_seed_carries_the_knob_byte_the_target_eats() {
    let Some(dir) = seeds() else {
        println!("jpx-seeds: SKIPPED (no fuzz/corpus/jpx)");
        return;
    };
    let mut raw = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .expect("the seed directory")
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).expect("a committed seed");
        // Either signature at offset 0 means the file *is* the body, so the
        // knob byte was never written.
        let bare_codestream = data.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]);
        let jp2_file = data.get(..4) == Some(&[0, 0, 0, 12]) && data.get(4..8) == Some(b"jP  ");
        if bare_codestream || jp2_file {
            raw.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    assert!(
        raw.is_empty(),
        "these seeds were written without the knob byte the target eats, so \
         their first byte is swallowed and their signature destroyed: {raw:?}"
    );
}
