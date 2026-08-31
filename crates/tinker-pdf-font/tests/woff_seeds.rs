//! The committed `woff` fuzz seeds, replayed on stable.
//!
//! `fuzz/corpus/woff/` is thirteen inputs — the five committed containers and
//! eight shapes written to break a specific rule — and the target that
//! consumes them needs nightly and a sanitizer runtime. So they would only
//! ever run when somebody ran `cargo fuzz`, which is not on every commit, and
//! a seed corpus nothing reads is a corpus that stops describing the parser
//! without anybody noticing. The same argument
//! `crates/tinker-pdf-pki/tests/fuzz_seeds.rs` makes, and the same
//! arrangement.
//!
//! This replays each seed through the same ceiling and the same assertions
//! `fuzz/fuzz_targets/woff.rs` makes, minus the mutation. It is not fuzzing
//! and does not pretend to be: it is a regression test over inputs that were
//! once interesting, which is what a seed corpus is. It prints `RAN` or
//! `SKIPPED` for the reason, as every check that can be absent does.
//!
//! # Counted injections
//!
//! **Two assertions fire, and neither is zero.** The eight crafted seeds are
//! each expected to be *refused* and the five container seeds to be
//! *accepted*, and both counts are asserted by number — a build where every
//! seed decoded would pass a test that only checked for absence of panics, and
//! that is precisely the build this corpus exists to catch.

use tinker_pdf_font::glyf::outline;
use tinker_pdf_font::woff::{decode, packaging};
use tinker_pdf_font::Sfnt;

/// `fuzz/fuzz_targets/woff.rs`'s own ceiling.
const LIMIT: usize = 1 << 20;

/// The five seeds that are a real encoder's output and must decode.
const CONTAINERS: [&str; 5] = [
    "synthetic-2.woff",
    "synthetic-2-ttf2woff.woff",
    "synthetic-2.woff2",
    "synthetic-2-wawoff2.woff2",
    "synthetic-2-aligned-hmtx.woff2",
];

fn seeds() -> Option<Vec<(String, Vec<u8>)>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/woff");
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, std::fs::read(&path).ok()?))
        })
        .collect();
    out.sort();
    (!out.is_empty()).then_some(out)
}

/// Every seed reaches an answer, and the answer is the one the seed was
/// written to produce.
#[test]
fn the_seed_corpus_still_describes_the_decoder() {
    let Some(seeds) = seeds() else {
        println!("SKIPPED: fuzz/corpus/woff is not present in this checkout");
        return;
    };

    let mut decoded = 0usize;
    let mut refused = 0usize;
    for (name, bytes) in &seeds {
        let announced = packaging(bytes);
        match decode(bytes, LIMIT) {
            Ok(font) => {
                assert!(
                    announced.is_some(),
                    "{name}: decoded a container the sniffer did not recognise"
                );
                assert!(font.len() <= LIMIT, "{name}: the ceiling held");
                // The target's own follow-through: bytes a decoder called a
                // font have to survive being read as one.
                if let Some(sfnt) = Sfnt::parse(&font) {
                    for glyph in [0u16, 1, 65, 255, 256, u16::MAX] {
                        let _ = outline(&sfnt, glyph);
                        let _ = sfnt.advance(glyph);
                    }
                }
                assert_eq!(
                    decode(bytes, LIMIT),
                    Ok(font),
                    "{name}: two decodes, one answer"
                );
                assert!(
                    CONTAINERS.contains(&name.as_str()),
                    "{name} was written to be refused and decoded instead"
                );
                decoded += 1;
            }
            Err(_) => {
                assert!(
                    !CONTAINERS.contains(&name.as_str()),
                    "{name} is a real encoder's output and must decode"
                );
                refused += 1;
            }
        }
    }

    println!("RAN: {} seeds", seeds.len());
    assert_eq!(decoded, 5, "the five containers decode");
    assert_eq!(refused, 8, "the eight crafted seeds are refused by name");
}

/// Every prefix of every seed reaches an answer rather than a panic
/// (ruling 1).
///
/// The axis a truncated download actually varies, swept exhaustively. Cheap:
/// the whole corpus is under six kilobytes.
#[test]
fn no_prefix_of_any_seed_panics() {
    let Some(seeds) = seeds() else {
        println!("SKIPPED: fuzz/corpus/woff is not present in this checkout");
        return;
    };
    let mut tried = 0usize;
    for (_, bytes) in &seeds {
        for take in 0..=bytes.len() {
            let _ = decode(&bytes[..take], LIMIT);
            tried += 1;
        }
    }
    println!("RAN: {tried} prefixes");
    assert!(tried > 5_000, "the sweep is not empty: {tried}");
}

/// A single flipped byte anywhere in a container reaches an answer rather than
/// a panic.
///
/// The other axis, and the one that reaches the transform: a flip inside the
/// Brotli stream changes what `reverse_glyf` is handed without changing any of
/// the lengths the directory declared, which is the shape no truncation
/// produces.
#[test]
fn no_single_byte_flip_in_a_container_panics() {
    let Some(seeds) = seeds() else {
        println!("SKIPPED: fuzz/corpus/woff is not present in this checkout");
        return;
    };
    let mut tried = 0usize;
    for (name, bytes) in &seeds {
        if !CONTAINERS.contains(&name.as_str()) {
            continue;
        }
        for at in 0..bytes.len() {
            for mask in [0x01u8, 0x80, 0xFF] {
                let mut flipped = bytes.clone();
                flipped[at] ^= mask;
                if let Ok(font) = decode(&flipped, LIMIT) {
                    if let Some(sfnt) = Sfnt::parse(&font) {
                        for glyph in [0u16, 1, 65, u16::MAX] {
                            let _ = outline(&sfnt, glyph);
                        }
                    }
                }
                tried += 1;
            }
        }
    }
    println!("RAN: {tried} single-byte flips");
    assert!(tried > 10_000, "the sweep is not empty: {tried}");
}
