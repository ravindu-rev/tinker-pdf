//! Raw ASN.1 DER, and the X.509 profile drawn over it.
//!
//! This is the target `docs/design/signatures.md` names as the mitigation for
//! its own largest risk — *"ASN.1 parser panics or overreads on malformed DER
//! (largest new untrusted surface)"* — and the design requires it to land in
//! the same commit as the parser rather than after it. Every byte a signature
//! is checked against arrives through here: a certificate embedded in a CMS
//! blob inside a PDF's `/Contents` is attacker-supplied twice over, once by
//! whoever wrote the document and once by whoever edited it afterwards.
//!
//! ## What the input is
//!
//! The bytes, unmodified, three ways. There is no carving and no control byte,
//! because unlike a fixed-field structure such as `/Encrypt`, DER *is* the
//! whole input: a mutation anywhere in it is a different length, a different
//! tag or a different depth, which is exactly the dimension worth exploring.
//!
//! 1. **The walker, exhaustively.** Every node reachable from the front of the
//!    buffer, descending into every constructed one, with every accessor tried
//!    on every node — a tag this crate does not read is what `as_string` and
//!    `as_time` refuse on, and the refusal paths are as much of the surface as
//!    the acceptance paths.
//! 2. **The X.509 profile**, which is the walker plus the extension decoders,
//!    the distinguished-name reader and the time arithmetic.
//! 3. **The walker again under a one-level depth cap**, so `DepthExceeded` is
//!    reached on ordinary inputs rather than only on adversarially deep ones.
//!    Without this the cap is a branch nothing takes.
//! 4. **The walker with X.690 §8.1.3.6's indefinite length allowed**, which is
//!    the mode `Limits::CMS` runs in and which no other pass here reaches.
//!    It is a whole second reading of the same bytes — an end-of-contents
//!    scan, a terminator that may be missing or forged, and a `raw` that is
//!    two octets wider than its header plus its value — so the passes above
//!    would leave all of it unfuzzed.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! A never-panic target that asserts nothing checks one property. These cost
//! nothing per node and check the invariants the *callers* of this crate are
//! entitled to rely on:
//!
//! - **A node's `raw` is its header plus its value** — plus, for an
//!   indefinite-length one, the two-octet terminator — and `range()` is
//!   exactly as wide either way. `Certificate::tbs_range` is what milestone 4
//!   will digest, so a range that is one byte wrong is a signature that never
//!   verifies and a defect that looks like bad crypto.
//! - **An indefinite-length node really ends in `00 00`**, is constructed, and
//!   leaves room for a header. The scan that found the terminator is the one
//!   piece of arithmetic here that can be off by two, and this is where being
//!   off by two shows up.
//! - **What `require_definite_lengths` accepts really is definite
//!   throughout.** Where it says yes, no node in the subtree may report
//!   `is_indefinite`, checked by walking the subtree a second way. That method
//!   is what stands between a BER `signedAttrs` and a digest nobody can
//!   explain, so "it returned `Ok`" is not enough to know about it.
//! - **A node's range lies inside its parent's**, and after the header. A
//!   child that claimed to start before its parent would let a caller quote a
//!   byte range covering bytes the structure does not contain.
//! - **The walker consumes exactly `raw.len()`**: the cursor's offset after a
//!   read is the node's `end()`.
//! - **Depth never exceeds the cap**, whatever the input says.
//! - **A parsed certificate's `tbs` is the slice its own range names**, which
//!   is the one property the whole verify path is built on.
//! - **Name matching is reflexive.** `a.matches(&a)` must hold for every name
//!   any input produces, because a comparison that can disagree with itself
//!   would make chain building depend on which copy of a name it happened to
//!   hold.

#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_pki::der::{Budget, Cursor, Limits, Tlv};
use tinker_pdf_pki::x509::Certificate;

/// Deep enough for anything real, shallow enough that the cap is reachable.
const WALK: Limits = Limits::new(12, 4_096);

/// Tries every reader on one node, whatever its tag turns out to be.
fn read_every_way(node: &Tlv<'_>) {
    let _ = node.as_bool();
    let _ = node
        .as_integer()
        .map(|i| (i.as_u64(), i.as_i64(), i.to_hex()));
    let _ = node.as_octet_string();
    let _ = node.as_null();
    let _ = node.as_bit_string().map(|b| {
        let _ = b.whole_bytes();
        // Past the end must be `false` rather than a panic, and the last bit
        // in range is where an off-by-one in the unused-bit arithmetic lands.
        let _ = b.bit(usize::MAX);
        let _ = b.bit(b.bit_len());
        b.bit_len()
    });
    let _ = node.as_oid().map(|o| (o.to_dotted(), o.arcs().count()));
    let _ = node.as_string();
    let _ = node.as_time();
    let _ = node.is_string();
    let _ = node.universal_tag();
}

/// Walks everything reachable, asserting the geometry as it goes.
///
/// Iterative rather than recursive, because the thing being fuzzed is a
/// structure whose nesting the input chooses — a recursive harness would
/// overflow its own stack and report the crate for it.
fn walk(data: &[u8], limits: Limits) {
    let budget = Budget::new(limits);
    // Each entry is a cursor still to be read from, beside the node whose
    // content it walks — `None` for the outermost one, which has no parent.
    let mut stack: Vec<(Cursor<'_, '_>, Option<Tlv<'_>>)> =
        vec![(Cursor::new(data, &budget), None)];

    while let Some((mut cursor, parent)) = stack.pop() {
        let before = cursor.offset();
        let Ok(node) = cursor.read() else {
            continue;
        };

        assert_eq!(
            node.start(),
            before,
            "a node did not start where the cursor was"
        );
        assert_eq!(node.range(), node.start()..node.end());
        assert_eq!(node.end() - node.start(), node.raw().len());
        assert!(
            node.raw().len() >= node.value().len(),
            "a value larger than the encoding that holds it"
        );
        assert_eq!(cursor.offset(), node.end(), "the cursor overran the node");
        assert!(node.depth() <= limits.max_depth, "past the depth cap");
        if node.is_indefinite() {
            assert!(
                limits.allow_indefinite_lengths,
                "an indefinite length read by a parse that did not allow one"
            );
            assert!(
                node.is_constructed(),
                "X.690 §8.1.3.2 admits the form only for constructed encodings"
            );
            // Two octets of header at least, then the value, then the pair.
            assert!(
                node.raw().len() >= node.value().len() + 4,
                "no room for both a header and a terminator"
            );
            assert_eq!(
                node.raw().get(node.raw().len() - 2..),
                Some(&[0x00, 0x00][..]),
                "an indefinite-length node that does not end in its terminator"
            );
        }
        // Where the sweep says a subtree is definite, no node in it may say
        // otherwise — the property `cms.rs` refuses a BER `signedAttrs` on.
        if node.require_definite_lengths(&budget).is_ok() {
            assert!(!node.is_indefinite());
            if let Ok(mut inner) = node.children(&budget) {
                while let Ok(child) = inner.read() {
                    assert!(
                        !child.is_indefinite(),
                        "a subtree called definite holds an indefinite node"
                    );
                }
            }
        }
        if let Some(parent) = parent {
            assert!(
                node.start() >= parent.start() && node.end() <= parent.end(),
                "a child outside the parent that holds it"
            );
        }

        read_every_way(&node);

        // The cursor goes back on so its next sibling is read later; the
        // child goes on above it. Both are bounded by the node budget, so
        // the stack cannot grow past it however the input nests.
        stack.push((cursor, parent));
        if node.is_constructed() {
            if let Ok(inner) = node.children(&budget) {
                stack.push((inner, Some(node)));
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    walk(data, WALK);
    // One level of nesting allowed, so `DepthExceeded` is on the path an
    // ordinary certificate takes rather than only an adversarial one.
    walk(data, Limits::new(1, 4_096));
    // The BER reading, which is a different walker: an end-of-contents scan
    // in front of every constructed node, and a `raw` two octets wider than
    // the header and value that make it up.
    walk(data, WALK.allowing_indefinite_lengths());
    // And shallow, so the scan's own depth ceiling is on an ordinary path
    // rather than only a deep one — it refuses a node the definite-length
    // reader would have handed back and refused only on descent.
    walk(data, Limits::new(2, 4_096).allowing_indefinite_lengths());

    if let Ok(certificate) = Certificate::parse(data) {
        let range = certificate.tbs_range();
        assert!(range.end <= data.len(), "a TBS range past the buffer");
        assert_eq!(
            &data[range],
            certificate.tbs(),
            "the TBSCertificate range does not name the TBSCertificate bytes"
        );
        assert_eq!(certificate.der().len(), data.len());

        for name in [certificate.issuer(), certificate.subject()] {
            assert!(name.matches(name), "a name that does not match itself");
            let _ = name.to_rfc4514();
            let _ = name.common_name();
        }
        // Self-issued is a name comparison across two independently parsed
        // names, which is the comparison chain building will make millions of.
        let _ = certificate.is_self_issued();
        let _ = certificate.key_identifier_sha1();
        let _ = certificate.signature_algorithms_agree();
        let _ = certificate.subject_public_key_info().public_key();
        let _ = certificate.validity().contains(0);
        let _ = certificate.serial().to_hex();
        for extension in certificate.extensions().all() {
            let _ = extension.oid().to_dotted();
            let _ = extension.value();
        }
        let _ = certificate.extensions().unrecognised_critical().count();
    }
});
