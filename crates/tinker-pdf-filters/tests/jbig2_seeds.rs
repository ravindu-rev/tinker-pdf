//! The committed JBIG2 fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/jbig2/` is twenty-two inputs a fuzzer found or a person wrote,
//! and the target that consumes them needs nightly. So the seeds were only ever
//! exercised when somebody ran `cargo fuzz`, which is not on every commit — and
//! a seed corpus nothing reads is a corpus that stops describing the decoder
//! without anybody noticing.
//!
//! This replays each one through the same split and the same assertions the
//! target makes, minus the mutation. It is not fuzzing and does not pretend to
//! be: it is a regression test over inputs that were once interesting, which is
//! what a seed corpus is. It prints `RAN` or `SKIPPED` for the reason every
//! check that can be absent does ([verification](../../../docs/verification.md)).

use std::path::{Path, PathBuf};

use tinker_pdf_filters::{jbig2_decode, Capability, FilterError, Jbig2Params};

/// The seed directory, from this crate rather than from the working directory.
fn seeds() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/jbig2");
    dir.is_dir().then_some(dir)
}

/// The target's own reading of its first byte (`fuzz/fuzz_targets/jbig2.rs`),
/// restated so a seed means here exactly what it means there.
fn knobs(data: &[u8]) -> (u32, u32, &[u8], &[u8]) {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let width = match knobs & 3 {
        0 => 1,
        1 => 8,
        2 => 37,
        _ => 64,
    };
    let height = match (knobs >> 2) & 3 {
        0 => 1,
        1 => 8,
        2 => 56,
        _ => 61,
    };
    let split = match (knobs >> 4) & 3 {
        0 => 0,
        1 => body.len() / 4,
        2 => body.len() / 2,
        _ => body.len(),
    };
    let (globals, own) = body.split_at(split.min(body.len()));
    (width, height, globals, own)
}

/// **Every committed seed still decodes to a page or to the refusal**, and to
/// nothing else.
///
/// The two assertions are the contract the render path rests on: a decode that
/// succeeds returns exactly the packed page its caller sized, because the
/// caller indexes it by row without measuring it again; and a decode that fails
/// fails with the named capability, because that refusal is the whole of this
/// codec's degradation contract (rulings 2 and 3).
#[test]
fn every_committed_seed_decodes_or_refuses_by_name() {
    let Some(dir) = seeds() else {
        println!("jbig2-seeds: SKIPPED (no fuzz/corpus/jbig2)");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the seed directory reads")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "the seed directory is empty");

    for path in &paths {
        let data = std::fs::read(path).expect("a seed reads");
        let (width, height, globals, own) = knobs(&data);
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals,
            width,
            height,
        };
        let ceiling = 1 << 16;
        match jbig2_decode(own, &params, ceiling, &mut warnings) {
            Ok(page) => {
                let stride = (width as usize).div_ceil(8);
                assert_eq!(
                    page.len(),
                    stride * height as usize,
                    "{name}: a successful decode returned a page that is not \
                     the size its caller asked for"
                );
                assert!(
                    page.len() <= ceiling,
                    "{name}: the output ceiling was exceeded"
                );
            }
            Err(error) => assert_eq!(
                error,
                FilterError::Unsupported(Capability::Jbig2),
                "{name}: failed with something other than the refusal"
            ),
        }

        // The warning set is closed and each variant is recorded at most once
        // per decode, so a stream of a million bad segments cannot turn
        // leniency into an allocation attack.
        let mut seen = warnings.clone();
        seen.sort_by_key(|w| format!("{w:?}"));
        seen.dedup_by_key(|w| format!("{w:?}"));
        assert_eq!(
            seen.len(),
            warnings.len(),
            "{name}: a warning was recorded more than once"
        );
    }

    println!("jbig2-seeds: RAN over {} committed seeds", paths.len());
}
