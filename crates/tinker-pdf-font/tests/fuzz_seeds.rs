//! Six committed font corpora, replayed on stable, measured for *reach*.
//!
//! `woff_seeds.rs` does the same for `fuzz/corpus/woff`. These six share a
//! file because they share a shape: five of the six targets open with
//! `let Some(x) = X::parse(data) else { return; }`, so a seed that does not
//! parse makes the whole target a no-op and the fuzzer spends its budget
//! mutating something that was never a font.
//!
//! # Why "it did not panic" is not the question here
//!
//! A fuzz target over arbitrary bytes is supposed to be refused most of the
//! time, so a corpus in which *every* seed is refused runs exactly as clean as
//! one in which every seed parses. `fuzz/corpus/jxr` held forty-two seeds and
//! decoded none of them; `fuzz/corpus/jpx` held six of the same. Neither was
//! visible from a seed count, a directory listing, or a green `cargo fuzz`.
//!
//! So these count, and each asserts that **at least one** seed reaches the
//! parser, and names the ones that do not.
//!
//! The first draft of this file asserted that *every* seed parses, on the
//! reasoning that a small hand-curated font corpus has no reason to hold a
//! non-font. Two seeds failed it and both are deliberate:
//! `type1/truncated-eexec.pfb` is a **negative** seed that exists to drive the
//! eexec decryptor's refusal, and `truetype/cmap12-glyph-id-overflow.ttf` is a
//! `cmap`-only face with no outline table at all, which is exactly what a
//! format-12 overflow test wants. A corpus needs both kinds, so the rule is
//! the weaker true one rather than the stronger false one — with the split
//! printed, so a corpus drifting towards all-refusals is visible in the
//! output rather than only in a fuzz run nobody reads.
//!
//! # What they are not
//!
//! Not a correctness check. Nothing below compares an outline, an advance or a
//! character mapping against an expected value — that is what this crate's own
//! tests and `crates/tinker-pdf/tests/cff_fonts.rs` are for. These answer one
//! question: does the corpus reach the code it was written for?

use std::path::{Path, PathBuf};

use tinker_pdf_font::{cmap, Cff, Sfnt, Type1};

/// One corpus directory, or `None` when the fuzz tree is not in this checkout.
fn seeds(name: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fuzz/corpus/{name}"));
    if !dir.is_dir() {
        return None;
    }
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path: PathBuf = entry.path();
        if path.is_file() {
            let label = path.file_name()?.to_string_lossy().into_owned();
            out.push((label, std::fs::read(&path).ok()?));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Some(out)
}

/// Reports the count and holds the floor.
///
/// The floor is "more than none". A corpus in which nothing parses is the
/// jxr defect; a corpus in which one seed deliberately does not is a corpus
/// with a negative case in it, which is a different and healthy thing.
fn report(name: &str, seeds: usize, reached: usize, unreached: &[String]) {
    assert!(seeds > 0, "fuzz/corpus/{name} is empty");
    assert!(
        reached > 0,
        "fuzz/corpus/{name}: not one of {seeds} seeds parses, so every run of this target returns at once"
    );
    if unreached.is_empty() {
        println!("RAN {name}-seeds: {seeds} seeds, {reached} reach the parser");
    } else {
        println!(
            "RAN {name}-seeds: {seeds} seeds, {reached} reach the parser, {} refused on purpose: {unreached:?}",
            unreached.len()
        );
    }
}

#[test]
fn the_cff_seeds_reach_the_parser() {
    let Some(files) = seeds("cff") else {
        println!("cff-seeds: SKIPPED (no fuzz/corpus/cff)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Cff::parse(data) {
            Some(cff) => {
                reached += 1;
                // Touching what the target touches, so a seed that parses and
                // then has nothing in it is visible as well.
                let count = cff.glyph_count().min(64) as u16;
                for glyph in 0..count {
                    let _ = cff.outline(glyph);
                    let _ = cff.advance(glyph);
                }
            }
            None => missed.push(label.clone()),
        }
    }
    report("cff", files.len(), reached, &missed);
}

#[test]
fn the_cff_subset_seeds_reach_the_subsetter() {
    let Some(files) = seeds("cff_subset") else {
        println!("cff_subset-seeds: SKIPPED (no fuzz/corpus/cff_subset)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // The target returns on a failed parse *and* on a zero glyph count, so
        // both are "not reached" — a font with no glyphs subsets nothing.
        match Cff::parse(data) {
            Some(cff) if cff.glyph_count() > 0 => reached += 1,
            _ => missed.push(label.clone()),
        }
    }
    report("cff_subset", files.len(), reached, &missed);
}

#[test]
fn the_sfnt_seeds_reach_the_directory_walk() {
    let Some(files) = seeds("sfnt") else {
        println!("sfnt-seeds: SKIPPED (no fuzz/corpus/sfnt)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Sfnt::parse(data) {
            Some(font) => {
                reached += 1;
                for tag in [b"head", b"hhea", b"maxp", b"cmap"] {
                    let _ = font.table(u32::from_be_bytes(*tag));
                }
            }
            None => missed.push(label.clone()),
        }
    }
    report("sfnt", files.len(), reached, &missed);
}

/// Whether a face's `glyf` holds a **composite** glyph, which is a
/// `numberOfContours` below zero at some glyph's start.
///
/// Read off the tables here rather than asked of the crate, because the crate
/// has nothing to ask. `glyf::outline` returns an `Outline`, and a composite
/// and the simple glyph it assembles are the same `Outline` by the time it
/// does — that is the whole point of the function. So "the corpus reaches the
/// recursion" is not a question the public API can answer, and a test that
/// wants it answered has to open `loca` itself.
fn carries_a_composite(font: &Sfnt<'_>) -> bool {
    let (Some(head), Some(loca), Some(glyf)) = (
        font.table(u32::from_be_bytes(*b"head")),
        font.table(u32::from_be_bytes(*b"loca")),
        font.table(u32::from_be_bytes(*b"glyf")),
    ) else {
        return false;
    };
    let Some(format) = head.get(50..52) else {
        return false;
    };
    // head.indexToLocFormat: 0 means 16-bit offsets stored halved.
    let long = i16::from_be_bytes([format[0], format[1]]) != 0;
    let width = if long { 4 } else { 2 };

    // A seed corpus, not a hostile input — but the ceiling costs nothing and
    // this test may not be the thing that hangs on a bad `loca`.
    for glyph in 0..4096usize {
        let Some(pair) = loca.get(glyph * width..glyph * width + width * 2) else {
            return false; // Past the last glyph.
        };
        let (start, end) = if long {
            (
                u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]) as usize,
                u32::from_be_bytes([pair[4], pair[5], pair[6], pair[7]]) as usize,
            )
        } else {
            (
                usize::from(u16::from_be_bytes([pair[0], pair[1]])) * 2,
                usize::from(u16::from_be_bytes([pair[2], pair[3]])) * 2,
            )
        };
        if end <= start {
            continue; // An empty range: a space.
        }
        if let Some(header) = glyf.get(start..start + 2) {
            if i16::from_be_bytes([header[0], header[1]]) < 0 {
                return true;
            }
        }
    }
    false
}

/// The floor on `truetype` seeds carrying an outline table.
///
/// Four is what the corpus holds, so this is a ratchet rather than a wish:
/// the number was measured when the three written seeds landed, and a seed
/// deleted or quietly stopped parsing takes it below the floor. It sat at
/// **one** before that, which is the coverage gap this constant replaced —
/// `docs/verification.md` carried the sentence and nothing enforced it.
const TRUETYPE_OUTLINED: usize = 4;

/// The floor on `truetype` seeds carrying a **composite** glyph.
///
/// The target's own header calls composite recursion "the interesting part",
/// and before `composite-transforms.ttf` and `composite-nested.ttf` were
/// written the corpus reached it from a single starting point. A floor of one
/// would let either of them go without the count moving, so it is two.
const TRUETYPE_COMPOSITE: usize = 2;

#[test]
fn the_truetype_seeds_reach_the_outlines() {
    let Some(files) = seeds("truetype") else {
        println!("truetype-seeds: SKIPPED (no fuzz/corpus/truetype)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    let mut outlineless: Vec<String> = Vec::new();
    let mut outlined: Vec<String> = Vec::new();
    let mut composite: Vec<String> = Vec::new();
    for (label, data) in &files {
        match Sfnt::parse(data) {
            Some(font) => {
                reached += 1;
                if font.table(u32::from_be_bytes(*b"glyf")).is_none()
                    && font.table(u32::from_be_bytes(*b"CFF ")).is_none()
                {
                    // Reported, not refused. `cmap12-glyph-id-overflow.ttf` is
                    // a `cmap`-only face on purpose, and the target's
                    // `glyph_for_char` half is real code that it reaches.
                    outlineless.push(label.clone());
                } else {
                    outlined.push(label.clone());
                    if carries_a_composite(&font) {
                        composite.push(label.clone());
                    }
                }
            }
            None => missed.push(label.clone()),
        }
    }
    if !outlineless.is_empty() {
        println!("  truetype: {outlineless:?} carry no outline table, by design");
    }
    println!("  truetype: {outlined:?} carry one, of which {composite:?} are composite");

    // The count, not the reach. `report` below asks whether the corpus parses
    // at all; these ask whether it reaches the half of the target that the
    // target's own header says is the half worth reaching.
    assert!(
        outlined.len() >= TRUETYPE_OUTLINED,
        "fuzz/corpus/truetype: {} of {} seeds carry an outline table, floor is \
         {TRUETYPE_OUTLINED} — {outlined:?}. A seed was deleted, or one stopped \
         parsing and is now reaching the directory walk and nothing else.",
        outlined.len(),
        files.len()
    );
    assert!(
        composite.len() >= TRUETYPE_COMPOSITE,
        "fuzz/corpus/truetype: {} seeds carry a composite glyph, floor is \
         {TRUETYPE_COMPOSITE} — {composite:?}. Composite recursion is what this \
         target's header calls the interesting part, and the fuzzer is now \
         starting from fewer places in it. `glyf.rs`'s `write_the_fuzz_seeds` \
         writes them.",
        composite.len()
    );

    report("truetype", files.len(), reached, &missed);
}

#[test]
fn the_type1_seeds_reach_the_charstrings() {
    let Some(files) = seeds("type1") else {
        println!("type1-seeds: SKIPPED (no fuzz/corpus/type1)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        match Type1::parse(data) {
            Some(font) => {
                reached += 1;
                // Two layers of stream cipher stand between the bytes and the
                // interpreter, so a program that decrypts to nothing still
                // "parses". The glyph count is what says it did not.
                assert!(
                    font.glyph_count() > 0,
                    "{label}: a program that parsed with no glyphs in it"
                );
            }
            None => missed.push(label.clone()),
        }
    }
    report("type1", files.len(), reached, &missed);
}

#[test]
fn the_cmap_seeds_reach_the_parser() {
    let Some(files) = seeds("cmap") else {
        println!("cmap-seeds: SKIPPED (no fuzz/corpus/cmap)");
        return;
    };
    let (mut reached, mut missed) = (0, Vec::new());
    for (label, data) in &files {
        // `cmap::parse` is infallible, so "reached" cannot be "it returned".
        // A CMap that found no codespace range decodes nothing, which is the
        // observable form of the same failure.
        let map = cmap::parse(data);
        if map.decode_codes(data).is_empty() {
            missed.push(label.clone());
        } else {
            reached += 1;
        }
    }
    report("cmap", files.len(), reached, &missed);
}
