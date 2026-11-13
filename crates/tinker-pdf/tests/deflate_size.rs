//! The deflate ratio level held to tier P's planning number (ruling 17): on
//! the benchmark set's page-like and photo-like inputs, at most 1.02 times
//! the bytes level 6 gives each (`docs/design/performance.md`, P-04, "The
//! level-6 size bound").
//!
//! The counts are committed beside the generators in
//! `benches/support/samples.rs`, each with the length and SHA-256 of the bytes
//! it was taken on, which the generator asserts of its own output. No level-6
//! encoder runs here or anywhere in the tree: a count states a target and
//! judges no output, and the bound is held in integers.

/// The benchmark set's raw samples and their counts: two generators are read
/// here, and no document is written.
#[allow(dead_code, reason = "two of the benchmark set's samples are read here")]
#[path = "../benches/support/samples.rs"]
mod samples;

use samples::{LevelSix, PAGE_LIKE_LEVEL_SIX, PHOTO_LIKE_LEVEL_SIX};
use tinker_pdf_filters::{deflate_at, inflate_raw, DeflateLevel, Limits};

/// `input` at the ratio level, at most 1.02 times `count`'s bytes and
/// inflating to itself.
fn holds(name: &str, input: &[u8], count: &LevelSix) {
    assert!(
        count.describes(input),
        "{name}: the bytes the count was taken on"
    );
    let coded = deflate_at(input, DeflateLevel::Ratio);
    assert!(
        100 * coded.len() <= 102 * count.count,
        "{name}: the ratio level wrote {} bytes, more than 1.02 times level 6's {}",
        coded.len(),
        count.count
    );
    let back = inflate_raw(&coded, &Limits::new(input.len() + 1));
    assert!(back.complete, "{name}: it inflates whole");
    assert!(back.warnings.is_empty(), "{name}: {:?}", back.warnings);
    assert!(back.data == input, "{name}: it inflates to its input");
}

#[test]
fn the_ratio_level_holds_level_six_size() {
    holds("page-like", &samples::page_like(), &PAGE_LIKE_LEVEL_SIX);
    holds("photo-like", &samples::photo_like(), &PHOTO_LIKE_LEVEL_SIX);
}
