//! The level-6 byte counts tier P holds the deflate ratio level to (ruling
//! 17) are counts of the bytes the benchmark set's generators write.
//!
//! Each count sits beside its generator in `benches/support/inputs.rs`, with
//! the length and SHA-256 of the input it was taken on, and the generator
//! asserts both of its own output. A test reaches the counts by path, as this
//! one does, so a generator changed under its count fails `cargo test` and
//! not only a benchmark run.

/// The benchmark set's generators: two of them are read here.
#[allow(
    dead_code,
    reason = "two of the benchmark set's generators are read here"
)]
#[path = "../benches/support/inputs.rs"]
mod inputs;

use inputs::{PAGE_BYTES, PAGE_LIKE_LEVEL_SIX, PHOTO_LIKE_LEVEL_SIX};

#[test]
fn each_level_six_count_was_taken_on_its_generators_bytes() {
    // Each generator asserts its count's length and digest of what it returns.
    assert_eq!(inputs::page_like().len(), PAGE_LIKE_LEVEL_SIX.length);
    assert_eq!(inputs::photo_like().len(), PHOTO_LIKE_LEVEL_SIX.length);
}

#[test]
fn a_count_describes_no_bytes_but_its_own() {
    let mut bytes = inputs::page_like();
    assert!(
        !PHOTO_LIKE_LEVEL_SIX.describes(&bytes),
        "the photo-like count is not the page-like bytes'"
    );
    bytes[PAGE_BYTES / 2] ^= 1;
    assert!(
        !PAGE_LIKE_LEVEL_SIX.describes(&bytes),
        "one byte changed is other bytes"
    );
    bytes[PAGE_BYTES / 2] ^= 1;
    bytes.pop();
    assert!(
        !PAGE_LIKE_LEVEL_SIX.describes(&bytes),
        "one byte short is other bytes"
    );
}
