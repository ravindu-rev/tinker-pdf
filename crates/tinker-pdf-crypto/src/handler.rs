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
}
