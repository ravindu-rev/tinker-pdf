//! AES, RC4, MD5 and SHA-2, driven directly rather than through the handler
//! that uses them.
//!
//! **The twenty-second target, and it exists because of a rate rather than a
//! gap in coverage.** These four ciphers were fuzzed by `crypt`, at the bottom
//! of a target whose every input first pays 7.6.4.3.3's hardened hash —
//! measured at 184 to 300 ms an input, against 1 to 5 ms for a legacy
//! revision. Gap 24 milestone 5's first campaign therefore bought them **2 635
//! executions in 301 seconds**; here they get about a million in the same time.
//! The hash is slow *by design* and cannot be made cheap without ceasing to
//! authenticate; the ciphers underneath it have no such excuse, and there is no
//! reason for them to be charged for it. Two independent things behind one
//! entry point are one thing being tested.
//!
//! Nothing here derives a key. The input is carved into a key, an IV and a
//! body, and every entry point is driven at whatever rate it costs.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! Every one of these is a **round trip or a cross-check between two entry
//! points over one algorithm**, because a cipher has no other oracle: a second
//! implementation to compare against is exactly what ruling 9 keeps out of
//! this repository, and the published vectors are already unit tests. What a
//! vector cannot do is cover the lengths, and length handling is where the
//! bugs are.
//!
//! - **CBC with PKCS#7 survives its own round trip at every length**, and the
//!   ciphertext is the IV plus the plaintext rounded up to a block, plus a
//!   whole block when the plaintext already ended on one. That last case is
//!   the one an off-by-one padding writer gets wrong and no test fixture of a
//!   convenient length ever reaches.
//! - **CBC without padding round trips on whole blocks** and drops a ragged
//!   tail rather than reading past it.
//! - **RC4 is its own inverse**, and the streaming and one-shot doors agree.
//! - **MD5, SHA-256, SHA-384 and SHA-512 agree with themselves fed in two
//!   pieces**, at a split the input chooses — which is the only thing that
//!   exercises `update`'s partial-block buffer, and it is a buffer per
//!   algorithm.
//! - **`Aes::new` accepts exactly two key widths, 16 and 32.** It is the gate
//!   every other entry point here is behind, so a widened one would hand a
//!   short key to the schedule silently. **Twenty-four is in the menu the
//!   input picks from and is a refusal**, which is worth stating rather than
//!   leaving to a reader's assumption: AES-192 is a real FIPS 197 width that
//!   PDF has no use for, so it is the length most likely to be accepted by
//!   accident. This assertion's first draft said three widths and the target
//!   crashed on its second input — the crate was right and the invariant was
//!   wrong, which is the shape gap 30's `xml` session found and is recorded
//!   here for the same reason: a target's invariants are as capable of being
//!   wrong as the code, and a build that satisfied this one would have been
//!   worse.
//! - **`constant_time_eq` agrees with `==`, and does not stop early.** It is
//!   the comparison that decides whether a password matched, and a version
//!   that always returned true would pass every functional test in the crate.
//!   The second half needs a pair that agrees on its first byte and differs at
//!   its last, which the obvious pair — a prefix and a suffix of one buffer —
//!   is not; the injection matrix is what said so, by surviving.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_crypto::aes::{
    cbc_decrypt_no_padding, cbc_decrypt_with_iv_prefix, cbc_encrypt_no_padding,
    cbc_encrypt_with_iv_prefix, Aes, AesNote,
};
use tinker_pdf_crypto::handler::constant_time_eq;
use tinker_pdf_crypto::md5::{md5, Md5};
use tinker_pdf_crypto::rc4::{rc4, Rc4};
use tinker_pdf_crypto::sha2::{sha256, sha384, sha512, Sha256, Sha512};

fuzz_target!(|data: &[u8]| {
    let (head, rest) = data.split_at(data.len().min(2));
    let control = head.first().copied().unwrap_or(0);
    let split = usize::from(head.get(1).copied().unwrap_or(0));

    // The two widths this crate expands, the FIPS 197 width it deliberately
    // does not, and a length that is no cipher's — so both sides of the gate
    // are reachable from the corpus rather than only from a hand-written test.
    let key_len = match control & 3 {
        0 => 16,
        1 => 24,
        2 => 32,
        _ => 5,
    };
    let (key, rest) = rest.split_at(rest.len().min(key_len));
    let (iv_bytes, body) = rest.split_at(rest.len().min(16));
    let mut iv = [0u8; 16];
    if let Some(slot) = iv.get_mut(..iv_bytes.len()) {
        slot.copy_from_slice(iv_bytes);
    }

    // ---- the gate ------------------------------------------------------
    let usable = Aes::new(key).is_some();
    assert_eq!(
        usable,
        matches!(key.len(), 16 | 32),
        "this crate expands 16- and 32-byte keys and nothing else, and \
         `Aes::new` is the only thing that says so; a {}-byte key was {}",
        key.len(),
        if usable { "accepted" } else { "refused" },
    );

    // ---- AES-CBC, padded ------------------------------------------------
    match cbc_encrypt_with_iv_prefix(key, &iv, body) {
        Some(sealed) => {
            assert!(usable, "a key AES refused still encrypted");
            // 7.6.2's layout: sixteen bytes of IV, then whole blocks. PKCS#7
            // adds a **whole block** when the plaintext already ends on one,
            // which is the case a fixture of convenient length never reaches.
            assert_eq!(
                sealed.len(),
                16 + body.len() + (16 - body.len() % 16),
                "the sealed length is not the IV plus a padded body"
            );
            let (back, notes) = cbc_decrypt_with_iv_prefix(key, &sealed);
            assert_eq!(back, body, "AES-CBC did not survive its own round trip");
            assert!(
                notes.is_empty(),
                "reading back what this crate wrote reported {notes:?}"
            );
        }
        None => assert!(!usable, "a key AES accepted would not encrypt"),
    }

    // Damage, from the same key: never a panic, and never more plaintext than
    // there was ciphertext to make it from.
    let (loose, _) = cbc_decrypt_with_iv_prefix(key, body);
    assert!(
        loose.len() <= body.len().saturating_sub(16),
        "decryption produced more bytes than the ciphertext after its IV"
    );
    // Under sixteen bytes there is no IV, let alone a block.
    if body.len() < 32 {
        let (short, notes) = cbc_decrypt_with_iv_prefix(key, body);
        assert!(short.is_empty() && notes.contains(&AesNote::TooShort));
    }

    // ---- AES-CBC, unpadded ---------------------------------------------
    // Algorithm 2.B's inner loop, where the input is whole blocks by
    // construction. A ragged tail is dropped rather than read past.
    let whole = body.get(..body.len() - body.len() % 16).unwrap_or_default();
    if let Some(sealed) = cbc_encrypt_no_padding(key, &iv, whole) {
        assert_eq!(sealed.len(), whole.len());
        assert_eq!(
            cbc_decrypt_no_padding(key, &iv, &sealed).as_deref(),
            Some(whole),
            "unpadded AES-CBC did not survive its own round trip"
        );
    }
    if let (Some(ragged), Some(trimmed)) = (
        cbc_encrypt_no_padding(key, &iv, body),
        cbc_encrypt_no_padding(key, &iv, whole),
    ) {
        assert_eq!(ragged, trimmed, "a ragged tail changed the ciphertext");
    }

    // ---- RC4 ------------------------------------------------------------
    // Its own inverse, which is the only oracle a keystream cipher has that
    // is not a second implementation of it.
    let once = rc4(key, body);
    assert_eq!(rc4(key, &once), body, "RC4 applied twice was not the input");
    // One-shot and streaming are two doors, and the engine uses both.
    let mut streamed = body.to_vec();
    let mut state = Rc4::new(key);
    let at = split.min(streamed.len());
    if let Some(front) = streamed.get_mut(..at) {
        state.apply(front);
    }
    if let Some(back) = streamed.get_mut(at..) {
        state.apply(back);
    }
    assert_eq!(streamed, once, "RC4 in two calls is RC4 in one");

    // ---- the hashes ------------------------------------------------------
    // Fed in two pieces at a split the input chooses. Each algorithm has its
    // own partial-block buffer, and nothing else here reaches one.
    let at = split.min(body.len());
    let (a, b) = body.split_at(at);

    let mut h = Md5::new();
    h.update(a);
    h.update(b);
    assert_eq!(h.finish(), md5(body), "MD5 in two updates is MD5 in one");

    let mut h = Sha256::new();
    h.update(a);
    h.update(b);
    assert_eq!(
        h.finish(),
        sha256(body),
        "SHA-256 in two updates is SHA-256 in one"
    );

    let mut h = Sha512::new();
    h.update(a);
    h.update(b);
    assert_eq!(
        h.finish(),
        sha512(body),
        "SHA-512 in two updates is SHA-512 in one"
    );

    let mut h = Sha512::new_384();
    h.update(a);
    h.update(b);
    let wide = h.finish();
    assert_eq!(
        wide.get(..48),
        Some(&sha384(body)[..]),
        "SHA-384 is SHA-512's state truncated, and the two doors disagreed"
    );

    // ---- the comparison that decides whether a password matched ----------
    assert!(constant_time_eq(a, a));
    assert_eq!(
        constant_time_eq(a, b),
        a == b,
        "the constant-time comparison disagreed with =="
    );
    // Two strings that **agree on their first byte and differ at the last**.
    // The pair above does not have that shape, and the injection matrix said
    // so: a `constant_time_eq` rewritten to compare only the first byte
    // survived it, because a prefix and a suffix of one buffer usually differ
    // at byte zero and then both answers are `false` for different reasons.
    // This is the pair that separates a comparison which reads every byte from
    // one that returns as soon as it can — which is the whole property, since
    // the value of a constant-time compare *is* that it does not stop early.
    if body.len() >= 2 {
        let mut twin = body.to_vec();
        if let Some(last) = twin.last_mut() {
            *last ^= 0xFF;
        }
        assert!(
            !constant_time_eq(body, &twin),
            "two strings differing only in their last byte compared equal"
        );
        assert!(constant_time_eq(body, body));
    }
});
