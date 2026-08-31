//! The committed `tar` fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/tar/` is seven inputs written by this crate's own
//! `write_the_fuzz_seeds`, and the target that consumes them needs nightly and
//! a sanitizer runtime. So the seeds were only ever exercised when somebody
//! ran `cargo fuzz`, which is not on every commit — and a seed corpus nothing
//! reads is a corpus that stops describing the parser without anybody
//! noticing. The same argument
//! `crates/tinker-pdf-pki/tests/fuzz_seeds.rs` and
//! `crates/tinker-pdf-filters/tests/jbig2_seeds.rs` make, and the same
//! arrangement.
//!
//! This replays each seed through the same control byte, the same bounds and
//! the same assertions `fuzz/fuzz_targets/tar.rs` makes, minus the mutation.
//! It is not fuzzing and does not pretend to be: it is a regression test over
//! inputs that were once interesting, which is what a seed corpus is. It
//! prints `RAN` or `SKIPPED` for the reason every check that can be absent does
//! ([verification](../../../docs/verification.md)).
//!
//! # Why the assertions are restated here rather than shared
//!
//! `fuzz/` is a separate workspace that this crate cannot depend on, so a
//! shared helper would have to live here and be called from there — which
//! inverts the dependency and makes the target's meaning a thing you read in
//! two files. They are restated instead, so a seed means here exactly what it
//! means there, and [`the_target_and_this_file_pick_the_same_bounds`] is what
//! stops the two drifting: it decodes the control byte with this file's table
//! and asserts the answer against the four values the target's `match` arms
//! spell out.

use std::path::{Path, PathBuf};

use tinker_pdf_archive::tar::{Archive, EntryError, Kind, Limits};

/// Every seed, by name, sorted so a failure names the same file on every
/// machine (ruling 4).
///
/// `None` when the directory is absent, which is what a source distribution
/// without `fuzz/` looks like — the difference between "the corpus is empty"
/// and "there is no corpus" is the one this returns.
fn seeds() -> Option<Vec<(String, Vec<u8>)>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/tar");
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path: PathBuf = entry.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, std::fs::read(&path).ok()?))
        })
        .collect();
    out.sort();
    (!out.is_empty()).then_some(out)
}

/// The target's own control-byte table, restated. See this file's header.
fn bounds(knobs: u8) -> Limits {
    Limits {
        max_entries: match knobs & 3 {
            0 => 1,
            1 => 4,
            2 => 64,
            _ => 4096,
        },
        max_name_len: match (knobs >> 2) & 3 {
            0 => 1,
            1 => 16,
            2 => 256,
            _ => 1024,
        },
    }
}

/// Whether `slice` is a subslice of `data`, **by address**.
///
/// The property the whole crate is organised around — `tar::Archive::read`
/// borrows and never copies — is not visible in a type once the lifetime is
/// erased, so it is checked the only way it can be checked from outside.
fn inside(data: &[u8], slice: &[u8]) -> bool {
    let base = data.as_ptr().addr();
    let at = slice.as_ptr().addr();
    at >= base && at.saturating_add(slice.len()) <= base.saturating_add(data.len())
}

/// Everything `fuzz_targets/tar.rs` asserts, over one input. Returns how many
/// entries the walk listed, so the caller can say the corpus reached something.
fn replay(name: &str, data: &[u8]) -> usize {
    let (control, body) = data.split_at(data.len().min(1));
    let limits = bounds(control.first().copied().unwrap_or(0));

    let Ok(archive) = Archive::open(body, &limits) else {
        return 0;
    };

    let listed: Vec<(String, u64, Kind)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind))
        .collect();

    assert!(
        listed.len() <= limits.max_entries,
        "{name}: the entry cap was exceeded rather than refused"
    );
    for (entry, _, _) in &listed {
        assert!(
            entry.len() <= limits.max_name_len,
            "{name}: a name past the cap was kept at full length"
        );
    }

    // Reverse order on purpose: a caller reads pages in whatever order the
    // viewer scrolls.
    for index in (0..listed.len()).rev() {
        match archive.read(index) {
            Ok(bytes) => {
                assert_eq!(
                    bytes.len() as u64,
                    listed[index].1,
                    "{name}: a successful read returned a length other than the \
                     one the entry declared"
                );
                assert_eq!(
                    listed[index].2,
                    Kind::File,
                    "{name}: an entry that is not a file was read rather than refused"
                );
                assert!(
                    bytes.is_empty() || inside(body, bytes),
                    "{name}: a read returned bytes that are not inside the archive"
                );
            }
            Err(EntryError::NoSuchEntry) => {
                panic!("{name}: an index taken from the entry list was not an entry")
            }
            Err(_) => {}
        }
    }

    let after: Vec<(String, u64, Kind)> = archive
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.size, e.kind))
        .collect();
    assert_eq!(
        listed, after,
        "{name}: the entry list changed under reading"
    );

    // The same bytes under the shipped bounds, which is the configuration that
    // ships and the one no widened cap may turn into a panic.
    if let Ok(shipped) = Archive::open(body, &Limits::DEFAULT) {
        for index in 0..shipped.entries().len().min(64) {
            let _ = shipped.read(index);
        }
    }
    listed.len()
}

/// Every committed seed replays without a panic and with the target's
/// invariants intact.
#[test]
fn the_committed_tar_seeds_replay() {
    let Some(seeds) = seeds() else {
        println!("SKIPPED: fuzz/corpus/tar is not in this tree");
        return;
    };
    let mut entries = 0usize;
    for (name, data) in &seeds {
        entries += replay(name, data);
    }
    println!(
        "RAN: {} tar seeds, {entries} entries walked in total",
        seeds.len()
    );
    assert_eq!(
        seeds.len(),
        7,
        "the seed count changed; `write_the_fuzz_seeds` is what should have \
         changed it, and the new file needs a reason in that test's comment"
    );
    assert!(
        entries > 0,
        "every seed opened to nothing, so the corpus exercises the signature \
         check and no further"
    );
}

/// **The corpus reaches both sides of both bounds.**
///
/// A seed corpus in which every input is roomy explores the happy path and
/// never a refusal, which is gap 18a milestone 8's failure arriving through the
/// corpus rather than through the constant. `plain-gnu-tight` is the seed that
/// exists to stop that, and this is the assertion that notices if its control
/// byte is ever edited to a roomier one.
#[test]
fn the_corpus_reaches_a_refusal_and_not_only_the_happy_path() {
    let Some(seeds) = seeds() else {
        println!("SKIPPED: fuzz/corpus/tar is not in this tree");
        return;
    };
    let mut refused = 0usize;
    let mut opened = 0usize;
    for (_, data) in &seeds {
        let (control, body) = data.split_at(data.len().min(1));
        let limits = bounds(control.first().copied().unwrap_or(0));
        match Archive::open(body, &limits) {
            Ok(_) => opened += 1,
            Err(_) => refused += 1,
        }
    }
    println!("RAN: {opened} seeds open, {refused} are refused outright");
    assert!(
        refused > 0,
        "no seed reaches a refusal, so the corpus never exercises the bounds"
    );
    assert!(
        opened > 0,
        "no seed opens, so the corpus never exercises the walk"
    );
}

/// The control-byte table here and the one in `fuzz/fuzz_targets/tar.rs` agree.
///
/// The two cannot share code — `fuzz/` is its own workspace and depends on this
/// crate rather than the reverse — so what stops them drifting is this: the
/// eight values below are transcribed from the target's two `match` arms, and a
/// seed's meaning is the pair they produce. A target edited to a different
/// ladder without editing this test replays its corpus under the old bounds and
/// says nothing.
#[test]
fn the_target_and_this_file_pick_the_same_bounds() {
    for (knobs, max_entries, max_name_len) in [
        (0x00u8, 1usize, 1usize),
        (0x01, 4, 1),
        (0x02, 64, 1),
        (0x03, 4096, 1),
        (0x04, 1, 16),
        (0x08, 1, 256),
        (0x0C, 1, 1024),
        (0xFF, 4096, 1024),
    ] {
        assert_eq!(
            bounds(knobs),
            Limits {
                max_entries,
                max_name_len
            },
            "control byte {knobs:#04x}"
        );
    }
    // The widest pair is the shipped one only by coincidence of the numbers,
    // and saying so is what stops a reader assuming `0xFF` means "defaults".
    assert_ne!(
        bounds(0xFF),
        Limits::DEFAULT,
        "if these ever coincide, the target stops exercising anything but the \
         shipped build under its own widest control byte"
    );
}
