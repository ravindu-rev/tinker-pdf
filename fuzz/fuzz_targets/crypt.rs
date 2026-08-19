//! The standard security handler (7.6.4): `/Encrypt` as the one dictionary a
//! document can use against a reader who has not authenticated yet.
//!
//! Its entries are fixed-width byte strings **whose widths the file declares**,
//! so the interesting inputs are the ones that are the wrong length, and the
//! interesting revisions are the ones nobody writes any more. The input is
//! carved into those fields in a fixed order, shorter inputs being zero-filled,
//! so every length of input produces a well-shaped-but-hostile handler rather
//! than being thrown away. Whatever is left over is the ciphertext.
//!
//! ## What gap 24 milestone 5's campaign changed here, and why
//!
//! The first real session gave this target **2 635 executions in 301 seconds**
//! — 8 a second, against `ascii_filters`' 61 795 — and a clean result at that
//! rate means almost nothing. Three things were wrong, and only one of them
//! was the slow hash everybody expects.
//!
//! **The widths were not fuzzed.** Every field was taken at exactly the width
//! the specification names, and [`Carve`] zero-fills, so a *three-byte* input
//! still produced a 48-byte `/O` and a 48-byte `/U`. Measured: that three-byte
//! seed cost 184 ms. Every revision-6 execution therefore took the identical
//! path — `params.u.len() < 48` was false on every input this target has ever
//! run, and `params.o.len() >= 48` true on every one — so the length handling
//! the doc comment above promises to attack was unreachable, and the hardened
//! hash was paid for a control-flow shape that never varied. The widths are
//! now the input's to choose, from a menu that includes zero, the two legal
//! values and the value one short of the revision-6 boundary.
//!
//! **Every input authenticated twice.** The loop tried the empty password and
//! then the carved one, which doubles the cost of the most expensive thing in
//! the crate to explore a dimension the input can carry itself. One attempt
//! per input now, chosen by a control bit.
//!
//! **Nothing past authentication had ever been fuzzed.** `authenticate`
//! returns `Some` only after a 128- or 256-bit equality against bytes the file
//! supplies, so mutated input reaches it with probability 2^-128: `FileKey`'s
//! whole surface was reachable from the one pristine seed that matched and
//! from nothing else, and the first mutation of it lost that. Coverage over
//! the old corpus put `FileKey::object_key` — 7.6.2 Algorithm 1, the
//! per-object key every encrypted PDF written before 2008 uses on every string
//! and every stream — at **0.00 %**, and `FileKey::decrypt` at 26 %. A valid
//! handler is now built **once** per process by [`build_r6`] and authenticated
//! once with each password, and every input drives the resulting keys over its
//! own body. That is the expensive setup happening once instead of never
//! succeeding at all. Algorithm 1 itself needs a *pre*-revision-6 key, which
//! `build_r6` cannot make; the `r4-authenticates` seed does, built from inside
//! the crate by the functions that own the algorithm.
//!
//! What remains slow is 7.6.4.3.3's hardened hash itself, and that is not a
//! defect to fix: Algorithm 2.B runs sixty-plus rounds of AES over a 64-times
//! repeated buffer *because* it is meant to cost an attacker something. Making
//! it cheap would mean not authenticating, which is the thing being fuzzed.
//! The plan records what session length that implies.
//!
//! ## What is asserted beyond "it did not panic"
//!
//! - **Both passwords recover the same file key.** Algorithms 8 and 9 wrap one
//!   key twice; if they did not, a document opened with the owner password
//!   would decrypt to noise, and nothing else here would notice.
//! - **A stream this crate sealed comes back byte for byte** through the
//!   handler that reads one, at every length, which is where CBC's padding
//!   lives.
//! - **RC4 is its own inverse**, so a `/StmF /V2` stream decrypted twice is
//!   the input again.
//! - **`/Identity` passes bytes through unchanged** — the one method with
//!   nothing to get wrong, and therefore the one whose failure would be
//!   silent.
//! - **A key that authenticated is a legal width**: 5 to 16 bytes before
//!   revision 5, 32 at and after it. `key_length_bytes` clamps `/Length`, and
//!   a clamp that stopped clamping would hand a short key to AES.
//! - **The object's identity reaches Algorithm 1 before revision 5 and not
//!   after it.** Two objects decrypt differently under `/V2` or `/AESV2` at a
//!   legacy revision and identically at revision 6, which is 7.6.2 against
//!   7.6.4.3.3 stated as bytes. What this does *not* check is whether the
//!   derived key is the **right** one — that is a known-answer question, and
//!   no fuzzer has an answer to compare against.
#![no_main]
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;

use tinker_pdf_crypto::handler::{build_r6, encrypt_aes256};
use tinker_pdf_crypto::{authenticate, AuthOutcome, CryptMethod, FileKey, HandlerParams};

/// Hands out fixed-width fields, zero-filling once the input runs out.
struct Carve<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Carve<'a> {
    fn take(&mut self, n: usize) -> Vec<u8> {
        let end = self.at.saturating_add(n).min(self.data.len());
        let mut out = self
            .data
            .get(self.at.min(end)..end)
            .unwrap_or_default()
            .to_vec();
        self.at = self.at.saturating_add(n);
        out.resize(n, 0);
        out
    }

    fn byte(&mut self) -> u8 {
        self.take(1).first().copied().unwrap_or(0)
    }

    fn rest(&self) -> &'a [u8] {
        self.data
            .get(self.at.min(self.data.len())..)
            .unwrap_or_default()
    }
}

fn method(bits: u8) -> CryptMethod {
    match bits & 3 {
        0 => CryptMethod::Identity,
        1 => CryptMethod::Rc4,
        2 => CryptMethod::AesV2,
        _ => CryptMethod::AesV3,
    }
}

/// A field's declared width, from a menu rather than from the specification.
///
/// Two bits per field, and the menu for each is the same shape: absent, short,
/// legal, and — where it matters — the value **one byte short of the boundary
/// the code checks**, which is the case a harness that always writes the legal
/// width can never produce.
fn width(bits: u8, menu: [usize; 4]) -> usize {
    menu[usize::from(bits & 3)]
}

/// The passwords the once-built handler is opened with. Neither is empty, so
/// the owner and user branches are told apart by which one matched rather than
/// by one of them being the default.
const USER_PASSWORD: &[u8] = b"a user password";
const OWNER_PASSWORD: &[u8] = b"an owner password";

/// A handler that authenticates, and the two keys it authenticates to.
struct Opened {
    file_key: [u8; 32],
    /// Opened with the user password. `/StmF /AESV3`, `/StrF /Identity`.
    user: FileKey,
    /// Opened with the owner password. `/StmF /V2`, `/StrF /AESV2`.
    owner: FileKey,
}

static OPENED: OnceLock<Opened> = OnceLock::new();

/// Builds and opens a real revision-6 handler. Called once per process.
///
/// Every `hash_2b` in this function is paid exactly once for the whole
/// session; before this existed the same work was paid on **every input** and
/// bought nothing, because a mutated `/U` cannot match its own hash.
fn open_once() -> Opened {
    // Fixed rather than random: a fuzz target has to behave identically for
    // the same input on every run, and that includes the run `cargo fuzz tmin`
    // makes of a crash.
    let mut entropy = [0u8; 48];
    for (i, slot) in entropy.iter_mut().enumerate() {
        *slot = (i as u8).wrapping_mul(37).wrapping_add(11);
    }

    let built = build_r6(USER_PASSWORD, OWNER_PASSWORD, -3904, true, &entropy)
        .expect("the writer's own handler builds");

    let params = |stream, string| HandlerParams {
        v: 5,
        r: 6,
        length_bits: 256,
        o: built.o.clone(),
        u: built.u.clone(),
        oe: built.oe.clone(),
        ue: built.ue.clone(),
        perms: built.perms.clone(),
        p: -3904,
        id_first: Vec::new(),
        encrypt_metadata: true,
        stream_method: stream,
        string_method: string,
    };

    let user = authenticate(
        &params(CryptMethod::AesV3, CryptMethod::Identity),
        USER_PASSWORD,
    )
    .expect("the user password opens the handler built from it");
    let owner = authenticate(
        &params(CryptMethod::Rc4, CryptMethod::AesV2),
        OWNER_PASSWORD,
    )
    .expect("the owner password opens the handler built from it");

    assert_eq!(user.outcome(), AuthOutcome::User);
    assert_eq!(owner.outcome(), AuthOutcome::Owner);
    // Algorithms 8 and 9 wrap **one** file key twice. A build where they did
    // not would authenticate either password and decrypt only one of them.
    assert_eq!(user.key(), &built.file_key[..]);
    assert_eq!(owner.key(), &built.file_key[..]);

    Opened {
        file_key: built.file_key,
        user,
        owner,
    }
}

fuzz_target!(|data: &[u8]| {
    let opened = OPENED.get_or_init(open_once);

    let mut carve = Carve { data, at: 0 };
    let control = carve.byte();
    let filters = carve.byte();
    let lengths = carve.byte();
    let widths = carve.byte();
    let more_widths = carve.byte();

    let params = HandlerParams {
        v: i64::from(control & 0x07),
        r: i64::from((control >> 3) & 0x07),
        // 40 to 264 bits, so both the legal multiples of eight and the
        // lengths no key can be are reachable.
        length_bits: 40 + 8 * i64::from(lengths % 29),
        // Declared as 32 bytes before revision 6 and 48 at revision 6. 47 is
        // in the menu because it is the width that passes the pre-revision-6
        // check and fails revision 6's by one byte, which is the only way to
        // reach `authenticate_r6`'s early returns at all.
        o: carve.take(width(widths, [0, 32, 47, 48])),
        u: carve.take(width(widths >> 2, [0, 32, 47, 48])),
        oe: carve.take(width(widths >> 4, [0, 16, 32, 40])),
        ue: carve.take(width(widths >> 6, [0, 16, 32, 40])),
        perms: carve.take(width(more_widths, [0, 8, 16, 24])),
        p: i32::from_be_bytes([carve.byte(), carve.byte(), carve.byte(), carve.byte()]),
        id_first: carve.take(width(more_widths >> 2, [0, 8, 16, 32])),
        encrypt_metadata: control & 0x40 == 0,
        stream_method: method(filters),
        string_method: method(filters >> 2),
    };

    // Past 127 bytes, 7.6.4.3.3 step (a) truncates and says so. The menu here
    // used to stop at 39, so `PasswordTruncated` was a note no input could
    // produce.
    let password_len = usize::from(carve.byte()) % 160;
    let password = carve.take(password_len);
    let body = carve.rest();

    // **One** attempt per input. The empty password is what every reader tries
    // first, and the path a document that "is not encrypted as far as the user
    // is concerned" takes; the carved one is everything else. Trying both on
    // every input doubles the cost of the most expensive operation in the
    // crate to explore a dimension the input itself can carry.
    let attempt: &[u8] = if control & 0x80 == 0 {
        b""
    } else {
        password.as_slice()
    };
    if let Some(key) = authenticate(&params, attempt) {
        let _ = key.notes();
        assert_ne!(
            key.outcome(),
            AuthOutcome::Failed,
            "a key that came back is a key that matched"
        );
        assert!(
            matches!(key.key().len(), 5..=16 | 32),
            "a {}-byte file key: /Length is clamped to 5..=16 before revision 5 \
             and fixed at 32 after it, so no other width can reach a cipher",
            key.key().len(),
        );

        // Object number and generation salt the per-object key, and both come
        // from the file.
        for (num, gen) in [(1u32, 0u16), (0, 0), (u32::MAX, u16::MAX)] {
            let _ = key.decrypt_string(num, gen, body);
            let _ = key.decrypt_stream(num, gen, body);
        }

        // 7.6.2 Algorithm 1 salts the file key with the object's identity —
        // *before* revision 5, and not at or after it, where 7.6.4.3.3 uses
        // the file key directly. Both halves of that sentence are asserted,
        // because a build that dropped the salting and a build that started
        // applying it are different defects and each looks like a decode.
        //
        // `/AESV3` is excluded on purpose: it never derives an object key at
        // any revision, which is the one thing in Table 25 that is not a
        // function of `/R`. Thirty-two bytes is the least an AES stream can
        // be and still be an IV and a block.
        let salted = match params.string_method {
            CryptMethod::Rc4 => true,
            // Algorithm 1 derives `(len + 5).min(16)` bytes and AES expands
            // only 16 or 32, so a 40-bit file key gives `/AESV2` a **ten-byte**
            // key that `Aes::new` refuses — and then every object decrypts to
            // nothing, which is equal to nothing. That combination is a
            // malformed document rather than a defect, and asserting on it
            // would be asserting that two empty vectors differ.
            CryptMethod::AesV2 => key.key().len() >= 11,
            CryptMethod::Identity | CryptMethod::AesV3 => false,
        };
        if salted && body.len() >= 32 {
            let first = key.decrypt_string(1, 0, body);
            let second = key.decrypt_string(2, 0, body);
            let revision_5_or_later =
                params.r >= 5 || (params.r == 0 && !matches!(params.v, 0 | 1 | 2 | 4));
            if revision_5_or_later {
                assert_eq!(
                    first, second,
                    "revision 6 has no per-object key, so two objects decrypt \
                     the same bytes the same way"
                );
            } else {
                assert_ne!(
                    first, second,
                    "two objects decrypted identically before revision 5, so \
                     the object number is not reaching Algorithm 1"
                );
            }
        }
    }

    // The half authenticating by luck cannot reach. These keys are real, they
    // cost nothing after the first input, and the body is the fuzzer's.
    //
    // 7.6.3.2: a stream this crate sealed comes back through the handler that
    // reads one, at every length, which is where CBC's padding lives.
    let sealed = encrypt_aes256(&opened.file_key, &[0x5A; 16], body);
    for (num, gen) in [(1u32, 0u16), (u32::MAX, u16::MAX)] {
        // `/Identity` is the method with nothing to get wrong, and therefore
        // the one whose failure would be silent.
        assert_eq!(
            opened.user.decrypt_string(num, gen, body),
            body,
            "/Identity is a pass-through"
        );
        assert_eq!(
            opened.user.decrypt_stream(num, gen, &sealed),
            body,
            "an AES-256 stream did not survive its own round trip"
        );
        // RC4 is its own inverse, so a `/StmF /V2` stream decrypted twice is
        // the input again — the only oracle available for a keystream cipher
        // without a second implementation of it.
        let once = opened.owner.decrypt_stream(num, gen, body);
        assert_eq!(
            opened.owner.decrypt_stream(num, gen, &once),
            body,
            "RC4 applied twice did not return the plaintext"
        );
        let _ = opened.owner.decrypt_string(num, gen, body);
        let _ = opened.owner.notes();
    }
});
