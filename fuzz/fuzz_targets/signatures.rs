//! The signature reader (12.8), over arbitrary bytes.
//!
//! Narrower and far faster than `render_page`, which is the point: the
//! signature path indexes the raw file buffer with four offsets the document
//! itself supplies, and a target that also rasterizes would spend almost all
//! its executions somewhere else. Digesting each signature is included because
//! the span arithmetic — `checked_add`, `try_from`, the slice that must not be
//! taken when a span runs past the end — is the part ruling 1 is about.
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
