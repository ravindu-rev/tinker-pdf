//! The signature reader (12.8), over arbitrary bytes.
//!
//! Narrower and far faster than `render_page`, which is the point: the
//! signature path indexes the raw file buffer with four offsets the document
//! itself supplies, and a target that also rasterizes would spend almost all
//! its executions somewhere else. Digesting each signature is included because
//! the span arithmetic — `checked_add`, `try_from`, the slice that must not be
//! taken when a span runs past the end — is the part ruling 1 is about.
//!
//! # What this target checks, and what it does not
//!
//! **Only that the code did not panic, hang, or exhaust memory.** Seven
//! calls, every one discarded. A signature reported as covering the wrong
//! byte ranges, or as valid when it is not, passes here. So a run that
//! returned the *wrong* answer passes this target exactly as a correct one
//! does, and a green `cargo fuzz` here is evidence about ruling 1 and about
//! nothing else.
//!
//! That is worth writing down rather than leaving implied. Correctness lives
//! in `crates/tinker-pdf/tests/signatures.rs` and `certification.rs`. The
//! distinction matters more than usual for this one: a validity answer nobody
//! checks is worse than none.
//!
//! Recorded because the same shape has already cost this repository once: the
//! `brotli` target asserts only self-consistency and could not have found the
//! ring-buffer defect that a decoded-bytes comparison found immediately.
//!
//! # The first session
//!
//! **4 September 2026: 1 924 289 executions in 601 seconds — 3 201 a second —
//! and nothing found.** The corpus went from 5 seeds to 3 572, which is the
//! largest proportional growth of the four targets run that day and says the
//! five it started from were nowhere near covering the field arithmetic.
//! `rustlang/rust:nightly`, rustc 1.100.0-nightly (a69a63265 2026-09-03),
//! cargo-fuzz 0.13.2, one core, on `x86_64-unknown-linux-gnu` in Docker
//! because libFuzzer does not build for `x86_64-pc-windows-msvc`.
//!
//! 3 201 a second is the second-slowest of the four and two orders of
//! magnitude under `pki_cms`'s 97 260, so this result is weaker evidence than
//! the raw execution count suggests — which is exactly the distinction
//! `docs/verification.md` draws and the reason the rate is recorded beside
//! the count rather than left to be inferred.
//!
//! Nothing here was minimised or committed, because nothing fired. That is
//! the result, and a stated zero beats a line nobody wrote down.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf::{DigestAlgorithm, Document};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::open(data.to_vec()) else {
        return;
    };
    // An encrypted document hides its object streams until a key exists, so
    // without this the interesting half of the corpus stops at the trailer.
    let _ = doc.authenticate("");

    for signature in doc.signatures() {
        let _ = signature.digest(&doc, DigestAlgorithm::Sha1);
        let _ = signature.digest(&doc, DigestAlgorithm::Sha256);
        let _ = signature.digest(&doc, DigestAlgorithm::Sha384);
        let _ = signature.digest(&doc, DigestAlgorithm::Sha512);
        let _ = signature.covers_whole_file();
        let _ = signature.is_usage_rights();
    }
});
