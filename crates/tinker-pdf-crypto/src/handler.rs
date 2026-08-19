//! The standard security handler (7.6.4).
//!
//! Inputs are plain scalars and byte strings; nothing here knows what a PDF
//! dictionary is. The caller extracts `/Encrypt` and hands the values over.
//!
//! Two things this module does that the engine it replaces could not:
//!
//! * It reports **which** password matched. `fz_authenticate_password` returns
//!   a bitmask, but the binding Tinker used collapsed it to a bool, so owner
//!   and user authentication were indistinguishable and "the owner password
//!   lifts every restriction" was unimplementable. [`AuthOutcome`] keeps the
//!   distinction.
//! * It never parses `/P` into a bitflags type. Reserved bits are 1 by
//!   specification, so a strict parse fails on every real document — which is
//!   exactly how the previous engine came to report every file as
//!   unrestricted. The raw integer is preserved and read bit by bit.

use crate::aes;
use crate::md5::{md5, Md5};
use crate::rc4::{rc4, Rc4};
use crate::sha2::{sha256, sha384, sha512};

/// 7.6.4.3 Algorithm 2, step (a): the padding string every pre-revision-6
/// password is extended with.
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// Which password matched.
///
/// `Owner` is a strictly stronger result than `User`: the owner password
/// authenticates a document whose restrictions no longer apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuthOutcome {
    /// Neither password matched.
    Failed,
    /// The user password matched; the document's permissions apply.
    User,
    /// The owner password matched; restrictions are lifted.
    Owner,
}

/// How a crypt filter transforms bytes (7.6.5 Table 25).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CryptMethod {
    /// `/None`, or `/Identity` selected as the filter: bytes pass through.
    #[default]
    Identity,
    /// `/V2`: RC4 with the per-object key.
    Rc4,
    /// `/AESV2`: AES-128-CBC with a per-object key and an IV prefix.
    AesV2,
    /// `/AESV3`: AES-256-CBC with the file key itself and an IV prefix.
    AesV3,
}

/// What the handler tolerated or noticed. Reported rather than logged, so the
/// caller can attach it to the document's warning list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandlerNote {
    /// `/R 5` is the withdrawn Adobe draft. Reading is supported; writing it
    /// is not, and nothing should be created at this revision.
    DeprecatedRevision5,
    /// `/R` or `/V` was absent and had to be inferred.
    RevisionInferred,
    /// `/U` or `/O` was shorter than the specification requires.
    ShortPasswordEntry,
    /// A revision-6 password exceeded 127 bytes and was truncated (7.6.4.3.3).
    PasswordTruncated,
    /// `/Perms` did not decrypt to the expected sentinel: the permissions in
    /// the document may have been tampered with (7.6.4.3.3 step f).
    PermsMismatch,
}

/// `/Encrypt` and `/ID`, flattened.
#[derive(Clone, Debug, Default)]
pub struct HandlerParams {
    /// `/V`, the algorithm selector (Table 20).
    pub v: i64,
    /// `/R`, the handler revision (Table 21).
    pub r: i64,
    /// `/Length` in bits; the specification's default is 40.
    pub length_bits: i64,
    /// `/O`, 32 bytes before revision 6, 48 at revision 6.
    pub o: Vec<u8>,
    /// `/U`, likewise.
    pub u: Vec<u8>,
    /// `/OE`, revision 6 only.
    pub oe: Vec<u8>,
    /// `/UE`, revision 6 only.
    pub ue: Vec<u8>,
    /// `/Perms`, revision 6 only.
    pub perms: Vec<u8>,
    /// `/P`, exactly as the file stores it, reserved bits and all.
    pub p: i32,
    /// The first element of the trailer's `/ID`.
    pub id_first: Vec<u8>,
    /// `/EncryptMetadata`; the default is true.
    pub encrypt_metadata: bool,
    /// The method `/StmF` resolves to.
    pub stream_method: CryptMethod,
    /// The method `/StrF` resolves to.
    pub string_method: CryptMethod,
}

/// An authenticated document's key material and the decisions made reaching it.
#[derive(Clone, Debug)]
pub struct FileKey {
    /// The file encryption key.
    key: Vec<u8>,
    outcome: AuthOutcome,
    revision: i64,
    stream_method: CryptMethod,
    string_method: CryptMethod,
    notes: Vec<HandlerNote>,
}

impl FileKey {
    #[must_use]
    pub fn outcome(&self) -> AuthOutcome {
        self.outcome
    }

    #[must_use]
    pub fn notes(&self) -> &[HandlerNote] {
        &self.notes
    }

    /// The file encryption key. Exposed because phase 09's writer needs it to
    /// re-encrypt an incrementally updated document.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Decrypts one string belonging to the given indirect object.
    #[must_use]
    pub fn decrypt_string(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt(self.string_method, num, gen, data)
    }

    /// Decrypts one stream belonging to the given indirect object.
    #[must_use]
    pub fn decrypt_stream(&self, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt(self.stream_method, num, gen, data)
    }

    fn decrypt(&self, method: CryptMethod, num: u32, gen: u16, data: &[u8]) -> Vec<u8> {
        match method {
            CryptMethod::Identity => data.to_vec(),
            CryptMethod::Rc4 => rc4(&self.object_key(num, gen, false), data),
            CryptMethod::AesV2 => {
                let key = self.object_key(num, gen, true);
                aes::cbc_decrypt_with_iv_prefix(&key, data).0
            }
            // 7.6.4.3.3: revision 6 uses the file key directly; there is no
            // per-object salting.
            CryptMethod::AesV3 => aes::cbc_decrypt_with_iv_prefix(&self.key, data).0,
        }
    }

    /// 7.6.2 Algorithm 1: the file key salted with the object's identity.
    fn object_key(&self, num: u32, gen: u16, aes: bool) -> Vec<u8> {
        if self.revision >= 5 {
            return self.key.clone();
        }

        let mut h = Md5::new();
        h.update(&self.key);
        h.update(&num.to_le_bytes()[..3]);
        h.update(&gen.to_le_bytes()[..2]);
        if aes {
            // 7.6.2: AES-128 mixes in this constant as well.
            h.update(&[0x73, 0x41, 0x6C, 0x54]);
        }
        let digest = h.finish();

        let n = (self.key.len() + 5).min(16);
        digest.get(..n).unwrap_or(&digest).to_vec()
    }
}

/// Attempts `password` against a document's `/Encrypt` values.
///
/// The owner password is tried first, so a document whose two passwords are
/// equal authenticates as the owner — the more capable of the two, and what a
/// user setting one password expects.
#[must_use]
pub fn authenticate(params: &HandlerParams, password: &[u8]) -> Option<FileKey> {
    let mut notes = Vec::new();

    let revision = if params.r == 0 {
        notes.push(HandlerNote::RevisionInferred);
        infer_revision(params)
    } else {
        params.r
    };
    if revision == 5 {
        notes.push(HandlerNote::DeprecatedRevision5);
    }

    let key = if revision >= 5 {
        authenticate_r6(params, password, revision, &mut notes)
    } else {
        authenticate_legacy(params, password, revision, &mut notes)
    };

    let (key, outcome) = key?;

    Some(FileKey {
        key,
        outcome,
        revision,
        stream_method: params.stream_method,
        string_method: params.string_method,
        notes,
    })
}

/// A missing `/R` is rare but occurs; `/V` implies it closely enough to try.
fn infer_revision(params: &HandlerParams) -> i64 {
    match params.v {
        0 | 1 => 2,
        2 => 3,
        4 => 4,
        _ => 6,
    }
}

fn key_length_bytes(params: &HandlerParams, revision: i64) -> usize {
    if revision == 2 {
        return 5;
    }
    // /Length is in bits; a few producers write bytes. Both readings are
    // clamped into the legal 5..=16 range rather than rejected.
    let bits = if params.length_bits == 0 {
        40
    } else {
        params.length_bits
    };
    let bytes = if (5..=16).contains(&bits) {
        bits
    } else {
        bits / 8
    };
    bytes.clamp(5, 16) as usize
}

fn pad_password(password: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let take = password.len().min(32);
    if let (Some(dst), Some(src)) = (out.get_mut(..take), password.get(..take)) {
        dst.copy_from_slice(src);
    }
    if let (Some(dst), Some(src)) = (out.get_mut(take..), PAD.get(..32 - take)) {
        dst.copy_from_slice(src);
    }
    out
}

/// 7.6.4.3.2 Algorithm 2: the file key from a padded user password.
fn compute_key_legacy(params: &HandlerParams, padded: &[u8; 32], revision: i64) -> Vec<u8> {
    let n = key_length_bytes(params, revision);

    let mut h = Md5::new();
    h.update(padded);
    h.update(params.o.get(..32).unwrap_or(&params.o));
    h.update(&params.p.to_le_bytes());
    h.update(&params.id_first);
    if revision >= 4 && !params.encrypt_metadata {
        // 7.6.4.3.2 step (f).
        h.update(&[0xff, 0xff, 0xff, 0xff]);
    }
    let mut digest = h.finish();

    if revision >= 3 {
        // Step (h): 50 further hashes over the first n bytes.
        for _ in 0..50 {
            digest = md5(digest.get(..n).unwrap_or(&digest));
        }
    }

    digest.get(..n).unwrap_or(&digest).to_vec()
}

/// 7.6.4.4.4 Algorithm 4 and 7.6.4.4.5 Algorithm 5: the expected `/U`.
fn expected_u(params: &HandlerParams, key: &[u8], revision: i64) -> Vec<u8> {
    if revision == 2 {
        return rc4(key, &PAD);
    }

    let mut h = Md5::new();
    h.update(&PAD);
    h.update(&params.id_first);
    let mut block = h.finish().to_vec();

    // Step (d): nineteen further RC4 passes with the key bytes XORed by the
    // iteration number.
    Rc4::new(key).apply(&mut block);
    for i in 1..=19u8 {
        let stepped: Vec<u8> = key.iter().map(|b| b ^ i).collect();
        Rc4::new(&stepped).apply(&mut block);
    }
    block
}

fn user_password_matches(params: &HandlerParams, key: &[u8], revision: i64) -> bool {
    let want = expected_u(params, key, revision);
    // Revision 3 and later leave the last 16 bytes of /U arbitrary.
    let compare = if revision == 2 { 32 } else { 16 };
    let (Some(a), Some(b)) = (want.get(..compare), params.u.get(..compare)) else {
        return false;
    };
    constant_time_eq(a, b)
}

/// 7.6.4.4.8 Algorithm 7: recover the user password from `/O`, then check it.
fn authenticate_legacy(
    params: &HandlerParams,
    password: &[u8],
    revision: i64,
    notes: &mut Vec<HandlerNote>,
) -> Option<(Vec<u8>, AuthOutcome)> {
    if params.u.len() < 32 || params.o.len() < 32 {
        notes.push(HandlerNote::ShortPasswordEntry);
    }

    let n = key_length_bytes(params, revision);
    let padded = pad_password(password);

    // Owner first: a document whose two passwords are equal is the owner's.
    let mut digest = md5(&padded);
    if revision >= 3 {
        for _ in 0..50 {
            digest = md5(&digest);
        }
    }
    let owner_key = digest.get(..n).unwrap_or(&digest).to_vec();

    let mut recovered = params.o.get(..32).unwrap_or(&params.o).to_vec();
    if revision == 2 {
        Rc4::new(&owner_key).apply(&mut recovered);
    } else {
        for i in (0..=19u8).rev() {
            let stepped: Vec<u8> = owner_key.iter().map(|b| b ^ i).collect();
            Rc4::new(&stepped).apply(&mut recovered);
        }
    }

    let mut recovered_padded = [0u8; 32];
    if let Some(src) = recovered.get(..32) {
        recovered_padded.copy_from_slice(src);
    }
    let owner_file_key = compute_key_legacy(params, &recovered_padded, revision);
    if user_password_matches(params, &owner_file_key, revision) {
        return Some((owner_file_key, AuthOutcome::Owner));
    }

    let user_file_key = compute_key_legacy(params, &padded, revision);
    if user_password_matches(params, &user_file_key, revision) {
        return Some((user_file_key, AuthOutcome::User));
    }

    None
}

/// 7.6.4.3.4 Algorithm 2.B: the hardened hash of revision 6.
fn hash_2b(password: &[u8], salt: &[u8], udata: &[u8], revision: i64) -> Vec<u8> {
    let mut input = Vec::with_capacity(password.len() + salt.len() + udata.len());
    input.extend_from_slice(password);
    input.extend_from_slice(salt);
    input.extend_from_slice(udata);
    let mut k = sha256(&input).to_vec();

    // Revision 5 is the withdrawn draft: a single SHA-256 and no rounds.
    if revision == 5 {
        return k;
    }

    let mut round = 0u32;
    loop {
        round += 1;

        let unit_len = password.len() + k.len() + udata.len();
        let mut k1 = Vec::with_capacity(unit_len * 64);
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(udata);
        }

        let (Some(key), Some(iv_slice)) = (k.get(..16), k.get(16..32)) else {
            return k;
        };
        let mut iv = [0u8; 16];
        iv.copy_from_slice(iv_slice);
        let Some(e) = aes::cbc_encrypt_no_padding(key, &iv, &k1) else {
            return k;
        };

        // Step (c): the first 16 bytes as a big-endian integer, modulo 3.
        // Base 256 is congruent to 1 modulo 3, so the digits may be summed.
        let sum: u32 = e
            .get(..16)
            .unwrap_or(&[])
            .iter()
            .map(|&b| u32::from(b))
            .sum();
        k = match sum % 3 {
            0 => sha256(&e).to_vec(),
            1 => sha384(&e).to_vec(),
            _ => sha512(&e).to_vec(),
        };

        // The loop runs at least 64 times, then until the last byte of E is no
        // greater than the round number less 32.
        if round >= 64 {
            let last = e.last().copied().unwrap_or(0);
            if u32::from(last) <= round.saturating_sub(32) {
                break;
            }
        }
        // A malformed file must not spin forever; 2.B terminates well inside
        // this bound for every valid input.
        if round > 1000 {
            break;
        }
    }

    k.truncate(32);
    k
}

/// 7.6.4.3.3 Algorithm 2.A: revision 6 authentication and key retrieval.
fn authenticate_r6(
    params: &HandlerParams,
    password: &[u8],
    revision: i64,
    notes: &mut Vec<HandlerNote>,
) -> Option<(Vec<u8>, AuthOutcome)> {
    // Step (a): the password is UTF-8, truncated to 127 bytes.
    let password = if password.len() > 127 {
        notes.push(HandlerNote::PasswordTruncated);
        password.get(..127).unwrap_or(password)
    } else {
        password
    };

    if params.u.len() < 48 {
        notes.push(HandlerNote::ShortPasswordEntry);
        return None;
    }
    let u48 = params.u.get(..48)?.to_vec();

    // Owner first, for the same reason as the legacy path.
    if params.o.len() >= 48 {
        let o_validation = params.o.get(32..40)?;
        let o_key_salt = params.o.get(40..48)?;
        let hash = hash_2b(password, o_validation, &u48, revision);
        if constant_time_eq(&hash, params.o.get(..32)?) {
            let intermediate = hash_2b(password, o_key_salt, &u48, revision);
            let key = aes::cbc_decrypt_no_padding(&intermediate, &[0u8; 16], &params.oe)?;
            check_perms(params, &key, notes);
            return Some((key, AuthOutcome::Owner));
        }
    }

    let u_validation = params.u.get(32..40)?;
    let u_key_salt = params.u.get(40..48)?;
    let hash = hash_2b(password, u_validation, &[], revision);
    if constant_time_eq(&hash, params.u.get(..32)?) {
        let intermediate = hash_2b(password, u_key_salt, &[], revision);
        let key = aes::cbc_decrypt_no_padding(&intermediate, &[0u8; 16], &params.ue)?;
        check_perms(params, &key, notes);
        return Some((key, AuthOutcome::User));
    }

    None
}

/// 7.6.4.3.3 step (f): `/Perms` decrypts to the permissions plus the sentinel
/// `adb`, which detects a `/P` edited after the fact.
fn check_perms(params: &HandlerParams, key: &[u8], notes: &mut Vec<HandlerNote>) {
    if params.perms.len() < 16 {
        return;
    }
    let Some(aes_key) = crate::aes::Aes::new(key) else {
        return;
    };
    // ECB over the single block, which is what CBC with a zero IV amounts to
    // for one block.
    let _ = aes_key;
    let Some(plain) =
        aes::cbc_decrypt_no_padding(key, &[0u8; 16], params.perms.get(..16).unwrap_or(&[]))
    else {
        return;
    };

    let sentinel_ok = plain.get(9..12) == Some(b"adb");
    let p_ok = match plain.get(..4) {
        Some(bytes) => {
            let mut b = [0u8; 4];
            b.copy_from_slice(bytes);
            i32::from_le_bytes(b) == params.p
        }
        None => false,
    };
    if !sentinel_ok || !p_ok {
        notes.push(HandlerNote::PermsMismatch);
    }
}

/// Comparison whose duration does not depend on where two byte strings first
/// Everything the `/Encrypt` dictionary of a new R6 document needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltHandler {
    /// The 32-byte file key, which encrypts every string and stream.
    pub file_key: [u8; 32],
    /// `/U`: 48 bytes, the user password's hash and its two salts.
    pub u: Vec<u8>,
    /// `/UE`: the file key wrapped with the user password.
    pub ue: Vec<u8>,
    /// `/O`: 48 bytes, the owner password's hash over `/U`.
    pub o: Vec<u8>,
    /// `/OE`: the file key wrapped with the owner password.
    pub oe: Vec<u8>,
    /// `/Perms`: the permissions, encrypted with the file key, so a reader can
    /// tell a tampered `/P` from an honest one.
    pub perms: Vec<u8>,
}

/// Builds an R6 (AES-256) handler for a new document.
///
/// 7.6.4.3.3 and Algorithms 8, 9 and 10. `entropy` supplies the 32-byte file
/// key and the two 8-byte user salts — 48 bytes in all, which is what `/U`'s
/// layout of hash-then-validation-salt-then-key-salt requires. This crate has
/// no opinion about where randomness comes from, and
/// `wasm32-unknown-unknown` has no source of it at all. A caller passing
/// predictable bytes gets a predictably weak document, which is their decision
/// to make and more honest than pretending to have entropy.
///
/// Returns `None` only when the AES layer refuses, which it does not for these
/// fixed sizes.
#[must_use]
pub fn build_r6(
    user_password: &[u8],
    owner_password: &[u8],
    permissions: i32,
    encrypt_metadata: bool,
    entropy: &[u8; 48],
) -> Option<BuiltHandler> {
    let mut file_key = [0u8; 32];
    file_key.copy_from_slice(&entropy[..32]);

    // 7.6.4.3.3: both salts are eight bytes. `/U` is the 32-byte hash, then
    // the validation salt, then the key salt — 48 bytes, and a reader slices
    // it at exactly those offsets.
    let user_validation = &entropy[32..40];
    let user_key_salt = &entropy[40..48];

    // The owner salts are derived rather than asked for: they must differ from
    // the user's, and hashing the same entropy achieves that without inventing
    // a second contract for the caller to get wrong.
    let owner_salts = crate::sha2::sha256(entropy);
    let owner_validation = &owner_salts[..8];
    let owner_key_salt = &owner_salts[8..16];

    // Algorithm 8: /U is hash || validation salt || key salt, and /UE is the
    // file key encrypted with a key derived from the password and key salt.
    let mut u = hash_2b(user_password, user_validation, &[], 6);
    u.extend_from_slice(user_validation);
    u.extend_from_slice(user_key_salt);

    let intermediate = hash_2b(user_password, user_key_salt, &[], 6);
    let ue = crate::aes::cbc_encrypt_no_padding(&intermediate, &[0u8; 16], &file_key)?;

    // Algorithm 9: the owner hashes are taken over the password *and* /U, so
    // the two passwords cannot be swapped and the owner one cannot be checked
    // without the file already carrying /U.
    let mut o = hash_2b(owner_password, owner_validation, &u, 6);
    o.extend_from_slice(owner_validation);
    o.extend_from_slice(owner_key_salt);

    let intermediate = hash_2b(owner_password, owner_key_salt, &u, 6);
    let oe = crate::aes::cbc_encrypt_no_padding(&intermediate, &[0u8; 16], &file_key)?;

    // Algorithm 10: /Perms is the permissions, a marker, and the metadata
    // flag, encrypted with the file key in ECB — which for one block is CBC
    // with a zero IV. A reader that decrypts it and finds /P disagreeing knows
    // the permissions were edited.
    let mut perms = [0u8; 16];
    perms[..4].copy_from_slice(&permissions.to_le_bytes());
    perms[4..8].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    perms[8] = if encrypt_metadata { b'T' } else { b'F' };
    perms[9] = b'a';
    perms[10] = b'd';
    perms[11] = b'b';
    // The last four bytes are unspecified filler.
    perms[12..].copy_from_slice(&entropy[32..36]);
    let perms = crate::aes::cbc_encrypt_no_padding(&file_key, &[0u8; 16], &perms)?;

    Some(BuiltHandler {
        file_key,
        u,
        ue,
        o,
        oe,
        perms,
    })
}

/// Encrypts one string or stream with an R6 file key (7.6.3.2).
///
/// AES-256-CBC with the initialisation vector prefixed, and — unlike every
/// earlier revision — no per-object key derivation: R6 uses the file key
/// directly, which is why the object number is not a parameter.
#[must_use]
pub fn encrypt_aes256(file_key: &[u8; 32], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    crate::aes::cbc_encrypt_with_iv_prefix(file_key, iv, data).unwrap_or_default()
}

/// differ.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_a_password_fills_from_the_pad_string() {
        assert_eq!(pad_password(b""), PAD);
        let padded = pad_password(b"ab");
        assert_eq!(padded.get(..2), Some(&b"ab"[..]));
        assert_eq!(padded.get(2..), PAD.get(..30));
        // Over-long passwords truncate rather than overflow.
        let long = pad_password(&[0x41u8; 64]);
        assert_eq!(long, [0x41u8; 32]);
    }

    #[test]
    fn key_length_tolerates_bits_and_bytes() {
        let mut p = HandlerParams {
            length_bits: 128,
            ..HandlerParams::default()
        };
        assert_eq!(key_length_bytes(&p, 4), 16);
        p.length_bits = 16; // a producer writing bytes
        assert_eq!(key_length_bytes(&p, 4), 16);
        p.length_bits = 0; // absent: the specification's default of 40 bits
        assert_eq!(key_length_bytes(&p, 3), 5);
        assert_eq!(key_length_bytes(&p, 2), 5, "revision 2 is always 40 bits");
    }

    #[test]
    fn constant_time_eq_is_still_correct() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn hash_2b_terminates_and_is_deterministic() {
        let a = hash_2b(b"password", &[1u8; 8], &[], 6);
        let b = hash_2b(b"password", &[1u8; 8], &[], 6);
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        assert_ne!(a, hash_2b(b"password", &[2u8; 8], &[], 6));
    }

    #[test]
    fn revision_5_is_a_single_sha256() {
        let got = hash_2b(b"pw", &[9u8; 8], &[], 5);
        let mut input = b"pw".to_vec();
        input.extend_from_slice(&[9u8; 8]);
        assert_eq!(got, sha256(&input).to_vec());
    }

    #[test]
    fn authentication_of_garbage_fails_without_panicking() {
        for r in [0i64, 2, 3, 4, 5, 6, 99] {
            let params = HandlerParams {
                v: 5,
                r,
                length_bits: 256,
                o: vec![0u8; 48],
                u: vec![0u8; 48],
                oe: vec![0u8; 32],
                ue: vec![0u8; 32],
                perms: vec![0u8; 16],
                p: -1,
                id_first: vec![0u8; 16],
                encrypt_metadata: true,
                stream_method: CryptMethod::AesV3,
                string_method: CryptMethod::AesV3,
            };
            let _ = authenticate(&params, b"whatever");
        }
    }

    #[test]
    fn short_entries_do_not_index_out_of_bounds() {
        let params = HandlerParams {
            v: 1,
            r: 2,
            o: vec![1, 2, 3],
            u: vec![4, 5],
            ..HandlerParams::default()
        };
        assert!(authenticate(&params, b"x").is_none());
    }

    /// Writes the seeds `fuzz/corpus/crypt/` and `fuzz/corpus/crypt_ciphers/`
    /// carry, so the seeds and the targets' carve orders cannot drift apart.
    ///
    /// Run with `--ignored` when a target's layout changes; the corpora are
    /// committed, and a run that rewrites them is a diff to look at rather than
    /// to apply blindly.
    ///
    /// **This is why the seeds landed here rather than staying hand-laid.** The
    /// `crypt` corpus was written by hand in the target's carve order and
    /// nothing tied the two together, so gap 24 milestone 5's rearrangement of
    /// that order would have silently turned the one seed that authenticates
    /// into 232 bytes of noise — and a corpus that no longer reaches what it
    /// was chosen for looks exactly like one that does.
    ///
    /// `crypt`'s layout is five control bytes and then the fields, at widths
    /// the fourth and fifth bytes choose:
    ///
    /// | Byte | Chooses |
    /// | --- | --- |
    /// | 0 | `/V` (bits 0-2), `/R` (3-5), `/EncryptMetadata` (6, inverted), and whether the carved password or the empty one is tried (7) |
    /// | 1 | `/StmF` (bits 0-1) and `/StrF` (2-3) |
    /// | 2 | `/Length`, as `40 + 8 * (byte % 29)` bits |
    /// | 3 | the widths of `/O`, `/U`, `/OE` and `/UE`, two bits each |
    /// | 4 | the widths of `/Perms` and `/ID`, two bits each |
    ///
    /// then `/O`, `/U`, `/OE`, `/UE`, `/Perms`, four big-endian bytes of `/P`,
    /// `/ID`, a password length, the password, and the rest as ciphertext.
    #[test]
    #[ignore = "writes into fuzz/corpus/, which is committed"]
    fn write_the_fuzz_seeds() {
        #[allow(clippy::too_many_arguments)]
        fn crypt_seed(
            control: u8,
            filters: u8,
            lengths: u8,
            widths: u8,
            more: u8,
            fields: [&[u8]; 6],
            p: i32,
            password: &[u8],
            body: &[u8],
        ) -> Vec<u8> {
            let [o, u, oe, ue, perms, id] = fields;
            let mut out = vec![control, filters, lengths, widths, more];
            out.extend_from_slice(o);
            out.extend_from_slice(u);
            out.extend_from_slice(oe);
            out.extend_from_slice(ue);
            out.extend_from_slice(perms);
            out.extend_from_slice(&p.to_be_bytes());
            out.extend_from_slice(id);
            out.push(u8::try_from(password.len()).expect("under 160 bytes"));
            out.extend_from_slice(password);
            out.extend_from_slice(body);
            out
        }

        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
        let filler = |n: usize, seed: u8| -> Vec<u8> {
            (0..n)
                .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
                .collect()
        };
        let body = filler(48, 0x40);

        // A handler the crate itself built, so the seed that authenticates
        // authenticates for the same reason a real document does.
        let mut entropy = [0u8; 48];
        for (i, slot) in entropy.iter_mut().enumerate() {
            *slot = (i as u8).wrapping_mul(53).wrapping_add(7);
        }
        let permissions = -3904;
        let built = build_r6(b"sesame", b"open sesame", permissions, true, &entropy)
            .expect("the writer's own handler builds");

        // Byte 0: /V 5, /R 6, /EncryptMetadata true, the carved password.
        // Byte 1: /StmF and /StrF both AESV3. Byte 2: 40 + 8 * 27 = 256 bits.
        // Byte 3: /O and /U at 48, /OE and /UE at 32. Byte 4: /Perms at 16,
        // no /ID, which revision 6 does not read.
        let r6 = |perms: &[u8]| {
            crypt_seed(
                0xB5,
                0x0F,
                27,
                0xAF,
                0x02,
                [&built.o, &built.u, &built.oe, &built.ue, perms, &[]],
                permissions,
                b"sesame",
                &body,
            )
        };
        let mut tampered = built.perms.clone();
        tampered[0] ^= 0x01;

        // The seed the campaign is the reason for: revision 6 with `/O` and
        // `/U` at **47** bytes, one short of the width `authenticate_r6`
        // checks. Every earlier version of this corpus wrote the legal width
        // into every field, so the early return was unreachable and every
        // revision-6 execution paid the hardened hash for a control-flow shape
        // that never varied.
        let boundary = crypt_seed(
            0xB5,
            0x0F,
            27,
            0x5A,
            0x05,
            [
                &built.o[..47],
                &built.u[..47],
                &built.oe[..16],
                &built.ue[..16],
                &built.perms[..8],
                &filler(8, 0x90),
            ],
            permissions,
            b"sesame",
            &body,
        );

        let legacy = |control: u8, filters: u8, lengths: u8| {
            crypt_seed(
                control,
                filters,
                lengths,
                // /O and /U at 32, /OE and /UE absent: the widths a
                // pre-revision-6 document actually carries.
                0x05,
                0x08,
                [
                    &filler(32, 0x11),
                    &filler(32, 0x22),
                    &[],
                    &[],
                    &[],
                    &filler(16, 0x33),
                ],
                -44,
                b"",
                &body,
            )
        };

        // A pre-revision-6 document that **actually authenticates**, and the
        // only seed in this repository that does.
        //
        // It matters because 7.6.2 Algorithm 1 — the per-object key, which
        // every encrypted PDF written before 2008 uses on every string and
        // every stream — is behind a `FileKey`, and a `FileKey` only exists
        // once a password has matched. Coverage over the old corpus put
        // `object_key` at **0.00 %**: the target had never executed it, and no
        // mutation ever could, because matching means colliding a 128-bit
        // digest.
        //
        // `/U` is computed here with the crate's own `compute_key_legacy` and
        // `expected_u` rather than with a restatement of Algorithms 2, 4 and 5
        // in the fuzz target. That is the same arrangement the `cff` and
        // `zip_archive` seeds use: the fixture is built by the code that owns
        // the format, so the two cannot drift, and this test is inside the
        // module so the private functions are in reach. There is no
        // `build_legacy` to call because this engine writes revision 6 and
        // nothing else, which is why the seed has to be built rather than
        // produced.
        let authenticating_legacy = {
            let password = b"open";
            let mut params = HandlerParams {
                v: 4,
                r: 4,
                length_bits: 128,
                o: filler(32, 0x11),
                p: -44,
                id_first: filler(16, 0x33),
                encrypt_metadata: true,
                stream_method: CryptMethod::Rc4,
                string_method: CryptMethod::AesV2,
                ..HandlerParams::default()
            };
            let key = compute_key_legacy(&params, &pad_password(password), 4);
            // Revision 3 and later leave `/U`'s last sixteen bytes arbitrary,
            // and a real file fills them; only the first sixteen are compared.
            let mut u = expected_u(&params, &key, 4);
            u.resize(32, 0xEE);
            params.u = u.clone();
            assert_eq!(
                authenticate(&params, password).map(|k| k.outcome()),
                Some(AuthOutcome::User),
                "the seed that is supposed to authenticate does not"
            );

            crypt_seed(
                // /V 4, /R 4, /EncryptMetadata true, the carved password.
                0x80 | 4 | (4 << 3),
                // /StmF /V2 and /StrF /AESV2, so both of Algorithm 1's
                // branches run — the AES one mixes in `sAlT` and the RC4 one
                // does not.
                0x01 | (0x02 << 2),
                11,
                0x05,
                0x08,
                [&params.o, &u, &[], &[], &[], &params.id_first],
                params.p,
                password,
                &body,
            )
        };

        for (name, bytes) in [
            ("r6-aes256", r6(&built.perms)),
            ("r6-perms-tampered", r6(&tampered)),
            ("r6-boundary-widths", boundary),
            // /V 4, /R 4, AESV2 both ways, 40 + 8 * 11 = 128 bits.
            ("r4-aes128", legacy(0x24, 0x0A, 11)),
            // /V 1, /R 2, RC4 both ways, the 40-bit default.
            ("r2-rc4-40", legacy(0x11, 0x05, 0)),
            ("r4-authenticates", authenticating_legacy),
            // Three bytes: revision 6 selected, and every width zero-filled to
            // "absent", so the handler refuses on `/U`'s length before it
            // hashes anything. This input cost 184 ms under the old layout,
            // where the zero fill produced a full-width `/U` instead.
            ("short-input", vec![0xB5, 0x0F, 27]),
        ] {
            std::fs::write(base.join("crypt").join(name), bytes)
                .expect("the corpus directory is there");
        }

        // `crypt_ciphers` is a control byte choosing the key width, a byte
        // choosing where a two-part update splits, then the key, sixteen bytes
        // of IV, and the body.
        let cipher_seed = |control: u8, split: u8, key: &[u8], body: &[u8]| -> Vec<u8> {
            let mut out = vec![control, split];
            out.extend_from_slice(key);
            out.extend_from_slice(&filler(16, 0x77));
            out.extend_from_slice(body);
            out
        };
        for (name, bytes) in [
            // A body that is already a whole number of blocks, which is where
            // PKCS#7 adds an entire block of its own.
            (
                "aes128-two-blocks",
                cipher_seed(0x00, 7, &filler(16, 0x01), &filler(32, 0x50)),
            ),
            (
                "aes256-ragged",
                cipher_seed(0x02, 19, &filler(32, 0x02), &filler(37, 0x60)),
            ),
            // Nothing to hash and nothing to encrypt: the zero-length pad. The
            // key is 24 bytes, which is a FIPS 197 width this crate
            // deliberately does not expand, so this seed is also the one that
            // holds the refusal in place.
            (
                "aes192-refused-empty-body",
                cipher_seed(0x01, 0, &filler(24, 0x03), &[]),
            ),
            // Five bytes is not any cipher's key. RC4 and the hashes still run.
            (
                "key-aes-refuses",
                cipher_seed(0x03, 3, &filler(5, 0x04), &filler(24, 0x70)),
            ),
        ] {
            std::fs::write(base.join("crypt_ciphers").join(name), bytes)
                .expect("the corpus directory is there");
        }
    }
}
