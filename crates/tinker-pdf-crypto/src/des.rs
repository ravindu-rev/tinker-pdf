//! DES and Triple DES (EDE3) in CBC mode, **decryption only** (FIPS 46-3).
//!
//! The one reason this exists: a CMS `EnvelopedData` in a PDF's `/Recipients`
//! may name `des-ede3-cbc` (`1.2.840.113549.3.7`) as its content cipher, and
//! OpenSSL still emits that by default for older recipients. Without it the
//! public-key handler meets such a document and can only name the gap. See
//! `docs/design/pubsec.md`.
//!
//! # Decryption only, and what that does *not* mean
//!
//! Nothing here encrypts a document. `Des::encrypt_block` exists because EDE3
//! decryption **is** `D(K1, E(K2, D(K3, c)))` — the middle step is a DES
//! encryption and there is no way to decrypt without it. It is private to this
//! module, not merely to the crate, and reached only through [`TripleDes`]: no
//! other file can call it even by mistake.
//!
//! # This is a weak cipher and it is here to read old documents
//!
//! Triple DES has a 64-bit block, which is Sweet32's whole premise, and
//! two-key EDE is below 112 bits of security. `SECURITY.md`'s framing covers
//! it: the scope is reading documents that already exist. Nothing in this
//! engine will ever *write* a `des-ede3-cbc` envelope.
//!
//! # The tables
//!
//! Every table below — `IP`, `IP_INV`, `E`, the eight S-boxes, `P`, `PC1`,
//! `PC2` and `SHIFTS` — is transcribed from **FIPS 46-3** (reaffirmed 25
//! October 1999, withdrawn 19 May 2005), NIST's archived PDF at
//! <https://csrc.nist.gov/files/pubs/fips/46-3/final/docs/fips46-3.pdf>,
//! SHA-256 `38dc009ca59d391814328fbbf3df0dfe30c69e75dc22b280efd807621e0244b1`.
//! `IP` and `IP^-1` are its page 10, `E` page 13, and Appendix 1's pages 17 to
//! 21 carry S1 through S8, `P`, `PC-1`, the shift schedule and `PC-2`.
//!
//! They were read **twice**, by two routes that fail differently, because a
//! single wrong entry in an S-box is wrong for only some inputs and a spot
//! check passes it. Both routes go through this engine — ruling 13: no
//! external program adjudicates a document here, and `tpdf` reads this one.
//!
//! - **The text layer** (`tpdf text`) preserves the *digits* in order but
//!   loses the column spacing wherever the source set a number flush against
//!   its neighbour: `IP^-1`, `P` in Appendix 1, `PC-1`, `PC-2` and S6, S7, S8
//!   all come out with numbers run together, so `12 1 10 15` reads as
//!   `12 11015`. Order survives; grouping does not.
//! - **The page as a picture** (`tpdf render --dpi 200`) recovers the
//!   grouping. Two things are worth knowing before repeating it: this build
//!   carries no bundled faces, so `--fonts <a path to a face>` is required or
//!   every page comes out blank with `UnreadableFont`; and page 13, which
//!   carries the `E` table beside Figure 2, stays blank regardless. `E` needs
//!   no second route — it is the one table whose text layer is spaced cleanly
//!   throughout, every entry separated.
//!
//! The test named `every_table_matches_the_published_digit_stream` below keeps
//! the first route runnable: it holds each table's digits, concatenated,
//! against the string the specification's text layer yields — so the two
//! readings stay checked against each other after the fact, and not only at
//! the moment of transcription. A wrong *digit* is caught there; a wrong
//! *grouping* is what the pictures settled.
//!
//! Two tables are printed twice in FIPS 46-3 itself — `S1` in the body and
//! again in Appendix 1, and `P` likewise — and the two printings agree, which
//! is the document checking itself rather than this module checking it.
//!
//! And none of that is the real adjudicator. That is NIST CAVP's known-answer
//! files, in the tests at the bottom. The table readings can only catch a
//! transcription slip; whether the *algorithm* is right is settled by
//! published answers, and the counted-injection campaign behind this commit
//! measured exactly which of them catch what.

/// Initial permutation (FIPS 46-3, page 10). One-based bit positions, most
/// significant first.
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, //
    60, 52, 44, 36, 28, 20, 12, 4, //
    62, 54, 46, 38, 30, 22, 14, 6, //
    64, 56, 48, 40, 32, 24, 16, 8, //
    57, 49, 41, 33, 25, 17, 9, 1, //
    59, 51, 43, 35, 27, 19, 11, 3, //
    61, 53, 45, 37, 29, 21, 13, 5, //
    63, 55, 47, 39, 31, 23, 15, 7,
];

/// The inverse of [`IP`], applied to the preoutput block (page 10).
const IP_INV: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, //
    39, 7, 47, 15, 55, 23, 63, 31, //
    38, 6, 46, 14, 54, 22, 62, 30, //
    37, 5, 45, 13, 53, 21, 61, 29, //
    36, 4, 44, 12, 52, 20, 60, 28, //
    35, 3, 43, 11, 51, 19, 59, 27, //
    34, 2, 42, 10, 50, 18, 58, 26, //
    33, 1, 41, 9, 49, 17, 57, 25,
];

/// The E bit-selection table: 32 bits of R to 48 (page 13).
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, //
    4, 5, 6, 7, 8, 9, //
    8, 9, 10, 11, 12, 13, //
    12, 13, 14, 15, 16, 17, //
    16, 17, 18, 19, 20, 21, //
    20, 21, 22, 23, 24, 25, //
    24, 25, 26, 27, 28, 29, //
    28, 29, 30, 31, 32, 1,
];

/// The permutation function P, applied to the S-box output (Appendix 1).
const P: [u8; 32] = [
    16, 7, 20, 21, //
    29, 12, 28, 17, //
    1, 15, 23, 26, //
    5, 18, 31, 10, //
    2, 8, 24, 14, //
    32, 27, 3, 9, //
    19, 13, 30, 6, //
    22, 11, 4, 25,
];

/// The eight selection functions S1..S8 (Appendix 1), each four rows of
/// sixteen. The row is the outer and inner bits of the six-bit input, the
/// column the middle four.
const S: [[u8; 64]; 8] = [
    // S1
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, //
        0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, //
        4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, //
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    // S2
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, //
        3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5, //
        0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, //
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    // S3
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, //
        13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, //
        13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, //
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    // S4
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, //
        13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, //
        10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, //
        3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    // S5
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, //
        14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, //
        4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, //
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    // S6
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, //
        10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, //
        9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, //
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    // S7
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, //
        13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, //
        1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, //
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    // S8
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, //
        1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, //
        7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, //
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// Permuted choice 1: 64 key bits to 56, dropping the parity bits (Appendix 1).
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, //
    1, 58, 50, 42, 34, 26, 18, //
    10, 2, 59, 51, 43, 35, 27, //
    19, 11, 3, 60, 52, 44, 36, //
    63, 55, 47, 39, 31, 23, 15, //
    7, 62, 54, 46, 38, 30, 22, //
    14, 6, 61, 53, 45, 37, 29, //
    21, 13, 5, 28, 20, 12, 4,
];

/// Permuted choice 2: 56 bits of `CnDn` to the 48-bit round key (Appendix 1).
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, //
    3, 28, 15, 6, 21, 10, //
    23, 19, 12, 4, 26, 8, //
    16, 7, 27, 20, 13, 2, //
    41, 52, 31, 37, 47, 55, //
    30, 40, 51, 45, 33, 48, //
    44, 49, 39, 56, 34, 53, //
    46, 42, 50, 36, 29, 32,
];

/// The schedule of left shifts, one per iteration (Appendix 1). Rounds 1, 2,
/// 9 and 16 rotate by one and the other twelve by two; they sum to 28, which
/// is why `C16` and `D16` are back where `C0` and `D0` started.
const SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

/// Applies a FIPS permutation table to `input`, whose `in_bits` significant
/// bits are numbered from 1 at the most significant end.
///
/// The result's first table entry becomes its most significant bit, which is
/// the convention every table in the standard is written in.
fn permute(input: u64, table: &[u8], in_bits: u32) -> u64 {
    let mut out = 0u64;
    for &position in table {
        out <<= 1;
        // `position` is 1-based from the most significant bit.
        let shift = in_bits - u32::from(position);
        out |= (input >> shift) & 1;
    }
    out
}

/// One DES key, expanded into its sixteen 48-bit round keys.
#[derive(Clone)]
pub struct Des {
    /// `K1`..`K16`, each in the low 48 bits.
    subkeys: [u64; 16],
}

impl core::fmt::Debug for Des {
    /// Prints no key material. A `Debug` that spelled the schedule out would
    /// put a content-encryption key in any log that formats an error.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Des { .. }")
    }
}

impl Des {
    /// Expands one 8-byte key. The parity bits (8, 16, ..., 64) are ignored,
    /// exactly as `PC-1` ignores them; a key of even parity is not refused,
    /// because FIPS 46-3 makes parity an error-detection aid and not a
    /// validity condition, and a document is not going to be re-keyed over it.
    #[must_use]
    pub fn new(key: [u8; 8]) -> Self {
        let key = u64::from_be_bytes(key);
        let permuted = permute(key, &PC1, 64);
        // C is the high 28 bits of the 56, D the low 28.
        let mut c = (permuted >> 28) & 0x0FFF_FFFF;
        let mut d = permuted & 0x0FFF_FFFF;

        let mut subkeys = [0u64; 16];
        for (subkey, &shift) in subkeys.iter_mut().zip(SHIFTS.iter()) {
            c = rotate_left_28(c, shift);
            d = rotate_left_28(d, shift);
            *subkey = permute((c << 28) | d, &PC2, 56);
        }
        Self { subkeys }
    }

    /// The sixteen-round Feistel network, with the round keys in the given
    /// order. Encryption uses `K1..K16` and decryption the reverse — that is
    /// the whole of the difference (FIPS 46-3, "Deciphering").
    fn feistel(&self, block: u64, reversed: bool) -> u64 {
        let permuted = permute(block, &IP, 64);
        let mut l = (permuted >> 32) & 0xFFFF_FFFF;
        let mut r = permuted & 0xFFFF_FFFF;

        for round in 0..16 {
            let index = if reversed { 15 - round } else { round };
            let subkey = self.subkeys.get(index).copied().unwrap_or(0);
            let next = l ^ f(r, subkey);
            l = r;
            r = next;
        }

        // The preoutput is R16L16, not L16R16: the last iteration's halves are
        // not swapped. Writing `(r << 32) | l` here *is* that final
        // interchange.
        permute((r << 32) | l, &IP_INV, 64)
    }

    /// Enciphers one block. Crate-private: EDE3 decryption needs it for the
    /// middle `E(K2, ...)` and nothing else in this engine encrypts with DES.
    fn encrypt_block(&self, block: u64) -> u64 {
        self.feistel(block, false)
    }

    /// Deciphers one block.
    fn decrypt_block(&self, block: u64) -> u64 {
        self.feistel(block, true)
    }
}

/// A 28-bit left rotation, the key schedule's "single left shift".
fn rotate_left_28(value: u64, by: u32) -> u64 {
    ((value << by) | (value >> (28 - by))) & 0x0FFF_FFFF
}

/// The cipher function f(R, K) (FIPS 46-3, "The Cipher Function f").
fn f(r: u64, subkey: u64) -> u64 {
    let expanded = permute(r, &E, 32) ^ subkey;

    let mut substituted = 0u64;
    for (box_index, sbox) in S.iter().enumerate() {
        // The six bits feeding S(i+1), most significant group first.
        let shift = 42 - 6 * box_index;
        let six = ((expanded >> shift) & 0x3F) as usize;
        substituted = (substituted << 4) | u64::from(select(sbox, six));
    }

    permute(substituted, &P, 32)
}

/// One S-box lookup: the row is the six-bit input's first and last bits, the
/// column its middle four (FIPS 46-3, "The Cipher Function f").
///
/// Extracted from [`f`] so the split lives in exactly one place. The
/// standard's own worked example tests *this*, which it could not do if the
/// arithmetic were inline above — a test that recomputed the split would agree
/// with itself about a transposition rather than catching one.
fn select(sbox: &[u8; 64], six: usize) -> u8 {
    let row = ((six & 0x20) >> 4) | (six & 1);
    let column = (six >> 1) & 0x0F;
    sbox.get(row * 16 + column).copied().unwrap_or(0)
}

/// A Triple DES key bundle, expanded.
///
/// Holding three [`Des`] schedules rather than three keys is what makes the
/// ordering explicit at the point of use: [`decrypt_block`](Self::decrypt_block)
/// reads `k3`, `k2`, `k1` in that order and no other place in this module
/// decides it.
#[derive(Clone, Debug)]
pub struct TripleDes {
    k1: Des,
    k2: Des,
    k3: Des,
}

impl TripleDes {
    /// Expands a key bundle, in any of ANSI X9.52's three keying options,
    /// which FIPS 46-3 lists by name:
    ///
    /// - **24 bytes** — option 1, `K1`, `K2`, `K3` independent. This is what a
    ///   `des-ede3-cbc` envelope always carries.
    /// - **16 bytes** — option 2, `K3 = K1`. Adjudicated by `TCBCMMT2`, which
    ///   prints all three keys and so can be run both ways; before those
    ///   vectors were committed this branch was caught by no test at all.
    /// - **8 bytes** — option 3, `K1 = K2 = K3`, which reduces EDE3 to single
    ///   DES and is how every one of CAVP's five KAT files is keyed.
    ///
    /// Any other length returns `None`: unlike document data a wrong-sized key
    /// is not damage to read through, it is a caller error, and the same rule
    /// [`crate::aes::Aes::new`] follows.
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        let part = |start: usize| -> Option<Des> {
            let bytes = key.get(start..start + 8)?;
            Some(Des::new(<[u8; 8]>::try_from(bytes).ok()?))
        };
        match key.len() {
            8 => {
                let k = part(0)?;
                Some(Self {
                    k1: k.clone(),
                    k2: k.clone(),
                    k3: k,
                })
            }
            16 => {
                let k1 = part(0)?;
                Some(Self {
                    k3: k1.clone(),
                    k1,
                    k2: part(8)?,
                })
            }
            24 => Some(Self {
                k1: part(0)?,
                k2: part(8)?,
                k3: part(16)?,
            }),
            _ => None,
        }
    }

    /// Deciphers one block: `D(K1, E(K2, D(K3, c)))`.
    ///
    /// FIPS 46-3's Appendix 2 block diagram gives the TDEA decryption
    /// operation as `I → D(K3) → E(K2) → D(K1) → O`, so the ciphertext meets
    /// `K3` first and `K1` last. Getting that backwards is invisible under
    /// keying option 3, where all three keys are equal, and equally invisible
    /// under option 2, where `K3 = K1` makes the swap a no-op — so the only
    /// thing that catches it is the three-key `TCBCMMT3` set, and the campaign
    /// measured exactly that: transposing `K1` and `K3` here fails one test.
    #[must_use]
    pub fn decrypt_block(&self, block: u64) -> u64 {
        self.k1
            .decrypt_block(self.k2.encrypt_block(self.k3.decrypt_block(block)))
    }
}

/// What a decryption tolerated, mirroring [`crate::aes::AesNote`] because the
/// callers are the same callers and the leniency policy is the same policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesNote {
    /// The key was not 8, 16 or 24 bytes, or there was no whole block.
    TooShort,
    /// Trailing bytes that are not a whole 8-byte block; they are ignored.
    RaggedTail,
    /// The PKCS#7 padding was not well formed, so nothing was stripped.
    BadPadding,
}

/// Decrypts `des-ede3-cbc` ciphertext with an explicit IV, stripping PKCS#7
/// padding (RFC 5652 §6.3).
///
/// Never fails: damage returns what was recovered plus a note, per the
/// leniency policy every reading path in this engine follows. The IV is
/// separate rather than prefixed because a CMS envelope carries it in the
/// algorithm identifier's parameters, not in the content.
#[must_use]
pub fn cbc_decrypt(key: &[u8], iv: &[u8; 8], data: &[u8]) -> (Vec<u8>, Vec<DesNote>) {
    let mut notes = Vec::new();

    let Some(tdes) = TripleDes::new(key) else {
        return (Vec::new(), vec![DesNote::TooShort]);
    };
    if data.len() < 8 {
        return (Vec::new(), vec![DesNote::TooShort]);
    }
    if data.len() % 8 != 0 {
        notes.push(DesNote::RaggedTail);
    }

    let mut out = chain(&tdes, u64::from_be_bytes(*iv), data);

    match out.last().copied() {
        Some(pad @ 1..=8) if usize::from(pad) <= out.len() => {
            let keep = out.len() - usize::from(pad);
            if out.get(keep..).is_some_and(|t| t.iter().all(|&b| b == pad)) {
                out.truncate(keep);
            } else {
                notes.push(DesNote::BadPadding);
            }
        }
        _ => notes.push(DesNote::BadPadding),
    }

    (out, notes)
}

/// Decrypts whole blocks with no padding discipline at all, returning `None`
/// for a bad key or a ragged length.
///
/// This is the shape the known-answer vectors need — their messages are exact
/// multiples of the block and carry no padding — and it is the same split
/// [`crate::aes`] makes for Algorithm 2.B.
#[must_use]
pub fn cbc_decrypt_no_padding(key: &[u8], iv: &[u8; 8], data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data.len() % 8 != 0 {
        return None;
    }
    let tdes = TripleDes::new(key)?;
    Some(chain(&tdes, u64::from_be_bytes(*iv), data))
}

/// CBC's chain, decrypting: each block is deciphered and then XORed with the
/// **previous ciphertext** block, the IV standing in for block zero.
fn chain(tdes: &TripleDes, iv: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut previous = iv;
    for block in data.chunks_exact(8) {
        let Ok(bytes) = <[u8; 8]>::try_from(block) else {
            break;
        };
        let cipher = u64::from_be_bytes(bytes);
        let plain = tdes.decrypt_block(cipher) ^ previous;
        out.extend_from_slice(&plain.to_be_bytes());
        // The *ciphertext* carries forward. Using `plain` here would still
        // decrypt the first block correctly and every later one wrongly.
        previous = cipher;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                let text = core::str::from_utf8(pair).ok()?;
                u8::from_str_radix(text, 16).ok()
            })
            .collect()
    }

    /// Every permutation table is a permutation, and every S-box is four rows
    /// of sixteen values below 16.
    ///
    /// This catches a duplicated or dropped entry — the transcription slip
    /// that a permutation table makes possible — and catches nothing about
    /// *order*, which is what the CAVP vectors below are for.
    #[test]
    fn the_tables_are_well_formed() {
        for (name, table, bits) in [
            ("IP", &IP[..], 64u8),
            ("IP_INV", &IP_INV[..], 64),
            ("P", &P[..], 32),
            ("PC1", &PC1[..], 64),
        ] {
            let mut seen = table.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), table.len(), "{name} repeats an entry");
            assert!(
                table.iter().all(|&b| b >= 1 && b <= bits),
                "{name} names a bit outside 1..={bits}"
            );
        }

        // PC-1 selects 56 of 64: the eight it drops must be exactly the parity
        // bits, which is the claim "the parity bits are ignored" rests on.
        let mut dropped: Vec<u8> = (1..=64u8).filter(|b| !PC1.contains(b)).collect();
        dropped.sort_unstable();
        assert_eq!(dropped, vec![8, 16, 24, 32, 40, 48, 56, 64]);

        // E and PC-2 select with repetition and omission respectively.
        assert!(E.iter().all(|&b| (1..=32).contains(&b)));
        assert!(PC2.iter().all(|&b| (1..=56).contains(&b)));

        for (index, sbox) in S.iter().enumerate() {
            assert!(
                sbox.iter().all(|&v| v < 16),
                "S{} holds a value above 15",
                index + 1
            );
            for row in 0..4usize {
                let mut values: Vec<u8> = sbox[row * 16..row * 16 + 16].to_vec();
                values.sort_unstable();
                values.dedup();
                assert_eq!(
                    values.len(),
                    16,
                    "S{} row {row} is not a permutation of 0..16",
                    index + 1
                );
            }
        }

        assert_eq!(SHIFTS.iter().sum::<u32>(), 28, "C16 returns to C0");
    }

    /// The second reading of every table, kept runnable.
    ///
    /// FIPS 46-3's text layer preserves each table's digits in order even
    /// where it loses the column spacing, so concatenating a table's entries
    /// must reproduce the specification's own digit stream. The strings below
    /// are that stream, taken from the pages named in the module doc: they are
    /// what the document says, and the arrays above are what this module
    /// believes. A transcription slip that the rendered image and the reader's
    /// eye both let through still has to survive this.
    ///
    /// A wrong *digit* is caught here. A wrong *grouping* — reading `1 2` as
    /// `12` — is not, and that is what the rendered pages settled and what the
    /// well-formedness test above independently constrains.
    #[test]
    fn every_table_matches_the_published_digit_stream() {
        fn stream(table: &[u8]) -> String {
            table.iter().map(|v| v.to_string()).collect()
        }

        // Page 10, IP.
        assert_eq!(
            stream(&IP),
            "58504234261810260524436282012462544638302214664\
             56484032241685749413325179159514335271911361534\
             5372921135635547393123157"
        );
        // Page 10, IP^-1.
        assert_eq!(
            stream(&IP_INV),
            "40848165624643239747155523633138646145422623037\
             54513532161293644412522060283534311511959273424\
             2105018582633141949175725"
        );
        // Page 13, the E bit-selection table.
        assert_eq!(
            stream(&E),
            "32123454567898910111213121314151617161718192021\
             20212223242524252627282928293031321"
        );
        // Appendix 1, P.
        assert_eq!(
            stream(&P),
            "1672021291228171152326518311028241432273919133062211425"
        );
        // Appendix 1, PC-1 and PC-2.
        assert_eq!(
            stream(&PC1),
            "57494133251791585042342618102595143352719113605\
             24436635547393123157625446383022146615345372921\
             1352820124"
        );
        assert_eq!(
            stream(&PC2),
            "14171124153281562110231912426816727201324152313\
             74755304051453348444939563453464250362932"
        );

        // Appendix 1, S1..S8, one string per box.
        let boxes = [
            "14413121511831061259070157414213110612119538411\
             48136211151297310501512824917511314100613",
            "15181461134972131205103134715281412011069115014\
             71110413158126932151381013154211671205149",
            "10091463155113127114281370934610285141211151136\
             49815301112125101471101306987415143115212",
            "71314306910128511124151381156150347212110149106\
             90121171315131452843150610113894511127214",
            "21241710116853151301491411212471315015103986421\
             11101378159125630141181271142136150910453",
            "12110159268013341475111015427129561131401138914\
             15528123704101131164321295151011141760813",
            "41121415081331297510611301174911014351221586141\
             11312371410156805926111381410795015142312",
            "13284615111109314501271151381037412561101492711\
             41912142061013153582114741081315129035611",
        ];
        for (index, published) in boxes.iter().enumerate() {
            let sbox = S.get(index).expect("eight boxes");
            assert_eq!(
                stream(sbox),
                *published,
                "S{} disagrees with the published digits",
                index + 1
            );
        }

        assert_eq!(
            SHIFTS.iter().map(|v| v.to_string()).collect::<String>(),
            "1122222212222221"
        );
    }

    /// The single worked example FIPS 46-3 spells out in prose (page 12): S1
    /// applied to `011011` selects row 1, column 13, which the table says is
    /// 5, so the output is `0101`.
    ///
    /// It goes through [`select`] rather than recomputing the split, which is
    /// the whole point of extracting that function: a test that did the
    /// arithmetic itself would agree with itself about a transposed row and
    /// column and catch nothing. The counted-injection campaign confirmed
    /// that — this check fired on nothing until `select` existed.
    #[test]
    fn the_standards_own_worked_example_for_s1() {
        assert_eq!(select(&S[0], 0b011011), 5);
        // And the neighbours, so that a shift of the whole table by one — which
        // the single value above would survive if 5 happened to land next to
        // itself — does not pass.
        assert_eq!(select(&S[0], 0b000000), 14, "row 0, column 0");
        assert_eq!(select(&S[0], 0b111111), 13, "row 3, column 15");
        assert_eq!(select(&S[0], 0b000001), 0, "row 1, column 0");
        assert_eq!(select(&S[0], 0b100000), 4, "row 2, column 0");
    }

    /// A key of any accepted length expands, and a key of any other does not.
    #[test]
    fn only_the_three_keying_options_are_accepted() {
        for length in [8usize, 16, 24] {
            assert!(
                TripleDes::new(&vec![0x01; length]).is_some(),
                "{length} bytes is a keying option"
            );
        }
        for length in [0usize, 7, 9, 15, 17, 23, 25, 32] {
            assert!(
                TripleDes::new(&vec![0x01; length]).is_none(),
                "{length} bytes is not"
            );
        }
    }

    /// Keying option 3 must reduce EDE3 to single DES — that is what makes
    /// TDEA backward compatible with DES, and what makes every CAVP
    /// known-answer file below a test of the block cipher itself.
    #[test]
    fn keying_option_three_is_single_des() {
        let key = [0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1];
        let single = Des::new(key);
        let triple = TripleDes::new(&key).expect("eight bytes");
        for sample in [0u64, 1, 0x0123_4567_89ab_cdef, u64::MAX] {
            assert_eq!(triple.decrypt_block(sample), single.decrypt_block(sample));
        }
    }

    /// Enciphering and deciphering are inverse, which pins that the round keys
    /// are consumed in opposite orders and nothing else differs.
    #[test]
    fn the_feistel_network_is_its_own_inverse() {
        let des = Des::new([0x13, 0x34, 0x57, 0x79, 0x9b, 0xbc, 0xdf, 0xf1]);
        for sample in [0u64, 1, 0x0123_4567_89ab_cdef, u64::MAX] {
            assert_eq!(des.decrypt_block(des.encrypt_block(sample)), sample);
        }
    }

    /// One vector as a CAVP `.rsp` prints it.
    struct Vector {
        /// The key bundle: `KEYs` where the file gives one, or `KEY1`, `KEY2`
        /// and `KEY3` concatenated where it gives three.
        key: Vec<u8>,
        /// Those three parts kept apart, when the file printed them apart.
        /// `TCBCMMT2` is why: it prints `K3 = K1` in full, so the same vector
        /// can also be run as the 16-byte bundle, and that is the only
        /// published answer keying option 2 has here.
        parts: Option<[Vec<u8>; 3]>,
        iv: [u8; 8],
        plaintext: Vec<u8>,
        ciphertext: Vec<u8>,
    }

    /// Reads every vector out of one CAVP `.rsp`.
    ///
    /// Both the `[ENCRYPT]` and `[DECRYPT]` sections are read, and both are run
    /// *through the decryption path*, because that is the only path this crate
    /// ships: an encrypt vector says `P` enciphers to `C`, so deciphering `C`
    /// must give back `P`. That doubles the vectors a two-section file
    /// contributes; `TCBCMMT2` has only the one section and contributes ten.
    fn vectors(source: &str) -> Vec<Vector> {
        let mut key = Vec::new();
        let mut parts: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let mut spelled_out = false;
        let mut iv = [0u8; 8];
        let mut plaintext = Vec::new();
        let mut ciphertext = Vec::new();
        let mut out = Vec::new();

        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((name, value)) = line.split_once(" = ") else {
                continue;
            };
            match name {
                "COUNT" => {
                    plaintext.clear();
                    ciphertext.clear();
                }
                "KEYs" => {
                    key = unhex(value);
                    spelled_out = false;
                }
                "KEY1" => parts[0] = unhex(value),
                "KEY2" => parts[1] = unhex(value),
                "KEY3" => {
                    parts[2] = unhex(value);
                    key = parts.concat();
                    spelled_out = true;
                }
                "IV" => iv = <[u8; 8]>::try_from(&unhex(value)[..]).expect("an 8-byte IV"),
                "PLAINTEXT" => plaintext = unhex(value),
                "CIPHERTEXT" => ciphertext = unhex(value),
                _ => {}
            }
            // Whichever of the pair arrives second completes the vector.
            if plaintext.is_empty() || ciphertext.is_empty() {
                continue;
            }
            out.push(Vector {
                key: key.clone(),
                parts: spelled_out.then(|| parts.clone()),
                iv,
                plaintext: core::mem::take(&mut plaintext),
                ciphertext: core::mem::take(&mut ciphertext),
            });
        }
        out
    }

    /// Decrypts every vector in one file with the key bundle exactly as that
    /// file prints it, and returns how many there were.
    fn run_kat(source: &str, single_key: bool) -> usize {
        let vectors = vectors(source);
        for (index, vector) in vectors.iter().enumerate() {
            let Vector {
                key,
                iv,
                ciphertext,
                ..
            } = vector;
            assert_eq!(
                key.len() == 8,
                single_key,
                "this file's keying option is not the one claimed"
            );
            let got = cbc_decrypt_no_padding(key, iv, ciphertext)
                .expect("a whole number of blocks and a valid key length");
            assert_eq!(
                got, vector.plaintext,
                "vector {index}: key {key:02x?} iv {iv:02x?} ciphertext {ciphertext:02x?}"
            );
        }
        vectors.len()
    }

    /// **The variable-plaintext KAT.** 64 vectors whose plaintext is a single
    /// 1 bit walking from position 1 to position 64, under a fixed key.
    ///
    /// This is one of the two files that make the eight S-boxes falsifiable.
    /// Every input bit position is exercised, so every path through `IP`, `E`,
    /// the S-boxes, `P` and `IP^-1` is driven from a different direction, and
    /// a single wrong S-box entry changes some of these 128 answers. A handful
    /// of random vectors would not: an S-box entry is one of 512 and a random
    /// block reaches a small subset of them.
    #[test]
    fn cavp_cbc_variable_plaintext_kat() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_vartext.rsp");
        assert_eq!(run_kat(VECTORS, true), 128, "64 encrypt and 64 decrypt");
    }

    /// **The variable-key KAT.** 56 vectors whose key is a single 1 bit
    /// walking through the 56 non-parity positions, under a fixed plaintext.
    ///
    /// The other half of the pair, and the one that adjudicates the key
    /// schedule: `PC-1`, the rotation schedule and `PC-2` between them decide
    /// where each key bit lands in each of sixteen round keys, and moving any
    /// one of them moves some of these answers.
    #[test]
    fn cavp_cbc_variable_key_kat() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_varkey.rsp");
        assert_eq!(run_kat(VECTORS, true), 112, "56 encrypt and 56 decrypt");
    }

    /// **The substitution-table KAT.** 19 key/plaintext pairs chosen by NIST
    /// to exercise the S-boxes specifically (SP 800-20's Table 5).
    #[test]
    fn cavp_cbc_substitution_table_kat() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_subtab.rsp");
        assert_eq!(run_kat(VECTORS, true), 38, "19 encrypt and 19 decrypt");
    }

    /// **The permutation-operation KAT.** 32 keys chosen to exercise `P`.
    #[test]
    fn cavp_cbc_permutation_operation_kat() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_permop.rsp");
        assert_eq!(run_kat(VECTORS, true), 64, "32 encrypt and 32 decrypt");
    }

    /// **The inverse-permutation KAT.** 64 vectors that walk the output bit
    /// positions, which is what pins `IP^-1` specifically: omitting the final
    /// permutation leaves a cipher that is still a bijection and still
    /// self-inverse, so only a published answer catches it.
    #[test]
    fn cavp_cbc_inverse_permutation_kat() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_invperm.rsp");
        assert_eq!(run_kat(VECTORS, true), 128, "64 encrypt and 64 decrypt");
    }

    /// **The multi-block message test, three independent keys.** 10 messages
    /// of 1 to 10 blocks under `K1 != K2 != K3` with a non-zero IV.
    ///
    /// Every KAT above is keyed `K1 = K2 = K3` — that is how CAVP writes them
    /// — and under that option the three sub-keys are interchangeable and
    /// EDE3's *order* is unobservable. So is it under the two-key file below,
    /// where `K3 = K1` makes transposing them a no-op. **This file is the only
    /// one that sees the order at all**, and the campaign measured it: taking
    /// the sub-keys as `D(K3, E(K2, D(K1, c)))` fails this test and no other.
    ///
    /// It is one of the two files with more than one block per message, so it
    /// also sees the CBC chain — a single block under a zero IV is decrypted
    /// identically by a chain that carries the ciphertext forward and one that
    /// carries the plaintext, which is why none of the five KATs can tell them
    /// apart.
    #[test]
    fn cavp_cbc_multiblock_three_key() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_mmt3.rsp");
        assert_eq!(run_kat(VECTORS, false), 20, "10 encrypt and 10 decrypt");
    }

    /// **The multi-block message test, two keys.** 10 messages of 1 to 10
    /// blocks under `K1`, `K2`, `K3 = K1`, decrypt direction only, which is
    /// all CAVS wrote for this file.
    ///
    /// This is the only published answer the 16-byte key bundle has, and it is
    /// here because the counted-injection campaign found the branch was
    /// guarded by nothing: making [`TripleDes::new`] take `K3 = K2` for a
    /// 16-byte key — a plausible slip, one character from the real line —
    /// broke no test at all. `only_the_three_keying_options_are_accepted`
    /// checks that 16 bytes are *accepted* and never what they expand to.
    ///
    /// Every vector is run twice: once as the 24 bytes the file prints, and
    /// once as `K1 || K2`. The second run is the one that matters, and it is
    /// adjudicated by NIST's plaintext rather than by the first run — an
    /// engine agreeing with itself about an abbreviation it invented would
    /// prove nothing.
    #[test]
    fn cavp_cbc_multiblock_two_key() {
        const VECTORS: &str = include_str!("../tests/data/cavp/tdes_cbc_mmt2.rsp");
        let vectors = vectors(VECTORS);
        assert_eq!(vectors.len(), 10, "ten decrypt messages");

        for (index, vector) in vectors.iter().enumerate() {
            let parts = vector
                .parts
                .as_ref()
                .expect("this file spells K1, K2, K3 out");
            assert_eq!(parts[2], parts[0], "vector {index}: option 2 means K3 = K1");

            let long = cbc_decrypt_no_padding(&vector.key, &vector.iv, &vector.ciphertext)
                .expect("24 bytes and whole blocks");
            assert_eq!(long, vector.plaintext, "vector {index}, as 24 bytes");

            let short = [parts[0].as_slice(), parts[1].as_slice()].concat();
            assert_eq!(short.len(), 16);
            let got = cbc_decrypt_no_padding(&short, &vector.iv, &vector.ciphertext)
                .expect("16 bytes and whole blocks");
            assert_eq!(got, vector.plaintext, "vector {index}, as 16 bytes");
        }
    }

    /// PKCS#7 padding is stripped, and a corrupt pad is reported rather than
    /// guessed at — the discipline RFC 5652 §6.3 asks of an envelope's
    /// content, and the same one [`crate::aes`] follows.
    #[test]
    fn padding_is_stripped_and_damage_is_named() {
        // One block of ciphertext under a known key, produced by this module's
        // own inverse; what is asserted is the *padding* behaviour around it.
        let key = [0x01u8; 24];
        let iv = [0u8; 8];
        let tdes = TripleDes::new(&key).expect("24 bytes");

        // "hi" plus six bytes of 0x06 is a whole padded block.
        let mut plain = b"hi".to_vec();
        plain.extend(std::iter::repeat_n(6u8, 6));
        let cipher = encrypt_for_test(&tdes, &iv, &plain);

        let (out, notes) = cbc_decrypt(&key, &iv, &cipher);
        assert_eq!(out, b"hi");
        assert!(notes.is_empty(), "well-formed padding is silent");

        // A block whose last byte claims a pad the rest does not agree with.
        let mut wrong = b"hi".to_vec();
        wrong.extend_from_slice(&[6, 6, 6, 1, 6, 6]);
        let cipher = encrypt_for_test(&tdes, &iv, &wrong);
        let (out, notes) = cbc_decrypt(&key, &iv, &cipher);
        assert_eq!(out, wrong, "nothing is stripped");
        assert!(notes.contains(&DesNote::BadPadding));

        // A ragged tail is read as far as it goes.
        let (_, notes) = cbc_decrypt(&key, &iv, &[0u8; 12]);
        assert!(notes.contains(&DesNote::RaggedTail));

        // A key of the wrong size is refused rather than truncated.
        let (out, notes) = cbc_decrypt(&[0u8; 20], &iv, &[0u8; 8]);
        assert!(out.is_empty());
        assert_eq!(notes, vec![DesNote::TooShort]);
        assert!(cbc_decrypt_no_padding(&[0u8; 20], &iv, &[0u8; 8]).is_none());
        assert!(cbc_decrypt_no_padding(&key, &iv, &[0u8; 9]).is_none());
        assert!(cbc_decrypt_no_padding(&key, &iv, &[]).is_none());
    }

    /// CBC encryption, for the padding test alone. Not public and not
    /// reachable from the engine: nothing here writes a DES envelope.
    fn encrypt_for_test(tdes: &TripleDes, iv: &[u8; 8], plain: &[u8]) -> Vec<u8> {
        let mut previous = u64::from_be_bytes(*iv);
        let mut out = Vec::new();
        for chunk in plain.chunks_exact(8) {
            let bytes = <[u8; 8]>::try_from(chunk).expect("a whole block");
            let block = u64::from_be_bytes(bytes) ^ previous;
            // EDE3 encryption is the mirror of `decrypt_block`.
            let cipher = tdes
                .k3
                .encrypt_block(tdes.k2.decrypt_block(tdes.k1.encrypt_block(block)));
            out.extend_from_slice(&cipher.to_be_bytes());
            previous = cipher;
        }
        out
    }
}
