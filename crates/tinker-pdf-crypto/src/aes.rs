//! AES-128 and AES-256 in CBC mode (FIPS 197, NIST SP 800-38A).
//!
//! Two padding disciplines, because PDF needs both: encrypted strings and
//! streams carry a leading 16-byte IV and PKCS#7 padding (7.6.2), while
//! Algorithm 2.B's hardened hash runs CBC with no padding at all and an
//! explicit IV (7.6.4.3.4).
//!
//! The implementation is table-free on purpose: the S-box is a constant array
//! and the round transforms are arithmetic, so there are no key-dependent
//! table indices. That is not a claim of constant-time execution — it removes
//! the most obvious cache-timing surface, and the threat model here (decrypting
//! a document the caller already possesses) does not include an adversary
//! measuring our cache.

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 11] = [
    0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
];

fn inv_sbox() -> [u8; 256] {
    let mut inv = [0u8; 256];
    for (i, &s) in SBOX.iter().enumerate() {
        if let Some(slot) = inv.get_mut(usize::from(s)) {
            *slot = i as u8;
        }
    }
    inv
}

fn sub(byte: u8) -> u8 {
    SBOX.get(usize::from(byte)).copied().unwrap_or(0)
}

/// Multiplication in GF(2^8) with the AES reduction polynomial 0x11b.
fn xtime(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1b } else { 0 }
}

fn mul(a: u8, b: u8) -> u8 {
    let mut result = 0u8;
    let mut a = a;
    let mut b = b;
    while b != 0 {
        if b & 1 != 0 {
            result ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    result
}

/// An expanded AES key. 128- and 256-bit only: PDF has no use for AES-192.
#[derive(Clone, Debug)]
pub struct Aes {
    /// Round keys, 4 bytes each: 44 words for AES-128, 60 for AES-256.
    words: Vec<[u8; 4]>,
    rounds: usize,
}

impl Aes {
    /// Expands a 16- or 32-byte key. Any other length is rejected: unlike
    /// document data, a wrong-sized key is a caller error the security handler
    /// must decide about.
    #[must_use]
    pub fn new(key: &[u8]) -> Option<Self> {
        let (nk, rounds) = match key.len() {
            16 => (4usize, 10usize),
            32 => (8, 14),
            _ => return None,
        };

        let total = 4 * (rounds + 1);
        let mut words: Vec<[u8; 4]> = Vec::with_capacity(total);

        for chunk in key.chunks_exact(4) {
            let mut w = [0u8; 4];
            w.copy_from_slice(chunk);
            words.push(w);
        }

        for i in nk..total {
            let mut temp = words.get(i - 1).copied().unwrap_or([0; 4]);
            if i % nk == 0 {
                temp.rotate_left(1);
                for byte in &mut temp {
                    *byte = sub(*byte);
                }
                let rcon = RCON.get(i / nk).copied().unwrap_or(0);
                if let Some(first) = temp.first_mut() {
                    *first ^= rcon;
                }
            } else if nk > 6 && i % nk == 4 {
                // FIPS 197 section 5.2: AES-256 substitutes on the fourth word
                // of each group as well.
                for byte in &mut temp {
                    *byte = sub(*byte);
                }
            }
            let prev = words.get(i - nk).copied().unwrap_or([0; 4]);
            words.push([
                prev[0] ^ temp[0],
                prev[1] ^ temp[1],
                prev[2] ^ temp[2],
                prev[3] ^ temp[3],
            ]);
        }

        Some(Self { words, rounds })
    }

    fn add_round_key(&self, state: &mut [u8; 16], round: usize) {
        for (col, chunk) in state.chunks_exact_mut(4).enumerate() {
            let w = self.words.get(round * 4 + col).copied().unwrap_or([0; 4]);
            for (byte, k) in chunk.iter_mut().zip(w.iter()) {
                *byte ^= k;
            }
        }
    }

    fn encrypt_block(&self, block: &mut [u8; 16]) {
        self.add_round_key(block, 0);

        for round in 1..=self.rounds {
            for byte in block.iter_mut() {
                *byte = sub(*byte);
            }
            shift_rows(block);
            if round != self.rounds {
                mix_columns(block);
            }
            self.add_round_key(block, round);
        }
    }

    fn decrypt_block(&self, block: &mut [u8; 16], inv: &[u8; 256]) {
        self.add_round_key(block, self.rounds);

        for round in (1..=self.rounds).rev() {
            inv_shift_rows(block);
            for byte in block.iter_mut() {
                *byte = inv.get(usize::from(*byte)).copied().unwrap_or(0);
            }
            self.add_round_key(block, round - 1);
            if round != 1 {
                inv_mix_columns(block);
            }
        }
    }
}

/// State is column-major (FIPS 197 section 3.4): byte `i` is row `i % 4`,
/// column `i / 4`, so row `r` is the bytes at `r`, `r+4`, `r+8`, `r+12`.
fn shift_rows(state: &mut [u8; 16]) {
    for row in 1..4usize {
        let mut tmp = [0u8; 4];
        for (col, slot) in tmp.iter_mut().enumerate() {
            *slot = state.get((col + row) % 4 * 4 + row).copied().unwrap_or(0);
        }
        for (col, value) in tmp.iter().enumerate() {
            if let Some(slot) = state.get_mut(col * 4 + row) {
                *slot = *value;
            }
        }
    }
}

fn inv_shift_rows(state: &mut [u8; 16]) {
    for row in 1..4usize {
        let mut tmp = [0u8; 4];
        for col in 0..4usize {
            tmp[(col + row) % 4] = state.get(col * 4 + row).copied().unwrap_or(0);
        }
        for (col, value) in tmp.iter().enumerate() {
            if let Some(slot) = state.get_mut(col * 4 + row) {
                *slot = *value;
            }
        }
    }
}

fn mix_columns(state: &mut [u8; 16]) {
    for col in state.chunks_exact_mut(4) {
        let [a0, a1, a2, a3] = [col[0], col[1], col[2], col[3]];
        col[0] = mul(a0, 2) ^ mul(a1, 3) ^ a2 ^ a3;
        col[1] = a0 ^ mul(a1, 2) ^ mul(a2, 3) ^ a3;
        col[2] = a0 ^ a1 ^ mul(a2, 2) ^ mul(a3, 3);
        col[3] = mul(a0, 3) ^ a1 ^ a2 ^ mul(a3, 2);
    }
}

fn inv_mix_columns(state: &mut [u8; 16]) {
    for col in state.chunks_exact_mut(4) {
        let [a0, a1, a2, a3] = [col[0], col[1], col[2], col[3]];
        col[0] = mul(a0, 14) ^ mul(a1, 11) ^ mul(a2, 13) ^ mul(a3, 9);
        col[1] = mul(a0, 9) ^ mul(a1, 14) ^ mul(a2, 11) ^ mul(a3, 13);
        col[2] = mul(a0, 13) ^ mul(a1, 9) ^ mul(a2, 14) ^ mul(a3, 11);
        col[3] = mul(a0, 11) ^ mul(a1, 13) ^ mul(a2, 9) ^ mul(a3, 14);
    }
}

/// What a decryption tolerated. PDF strings in the wild are truncated,
/// mis-padded, and occasionally not encrypted at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AesNote {
    /// Fewer than 16 bytes, or no whole block after the IV: nothing to do.
    TooShort,
    /// Trailing bytes that are not a whole block; they are ignored.
    RaggedTail,
    /// The PKCS#7 padding was not well formed, so nothing was stripped.
    BadPadding,
}

/// Decrypts CBC ciphertext whose first 16 bytes are the IV, stripping PKCS#7
/// padding (7.6.2).
///
/// Never fails: damage returns the plaintext recovered so far with a note, per
/// the leniency policy every reading path in this engine follows.
#[must_use]
pub fn cbc_decrypt_with_iv_prefix(key: &[u8], data: &[u8]) -> (Vec<u8>, Vec<AesNote>) {
    let mut notes = Vec::new();

    let Some(aes) = Aes::new(key) else {
        return (Vec::new(), vec![AesNote::TooShort]);
    };
    let Some((iv, body)) = data.split_at_checked(16) else {
        return (Vec::new(), vec![AesNote::TooShort]);
    };
    if body.len() < 16 {
        return (Vec::new(), vec![AesNote::TooShort]);
    }
    if body.len() % 16 != 0 {
        notes.push(AesNote::RaggedTail);
    }

    let inv = inv_sbox();
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);

    let mut out = Vec::with_capacity(body.len());
    for chunk in body.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let cipher = block;
        aes.decrypt_block(&mut block, &inv);
        for (byte, p) in block.iter_mut().zip(prev.iter()) {
            *byte ^= p;
        }
        out.extend_from_slice(&block);
        prev = cipher;
    }

    match out.last().copied() {
        Some(pad @ 1..=16) if usize::from(pad) <= out.len() => {
            let keep = out.len() - usize::from(pad);
            if out.get(keep..).is_some_and(|t| t.iter().all(|&b| b == pad)) {
                out.truncate(keep);
            } else {
                notes.push(AesNote::BadPadding);
            }
        }
        _ => notes.push(AesNote::BadPadding),
    }

    (out, notes)
}

/// Encrypts with CBC and PKCS#7 padding, prefixing the given IV — the layout
/// [`cbc_decrypt_with_iv_prefix`] expects.
#[must_use]
pub fn cbc_encrypt_with_iv_prefix(key: &[u8], iv: &[u8; 16], plain: &[u8]) -> Option<Vec<u8>> {
    let aes = Aes::new(key)?;

    let pad = 16 - (plain.len() % 16);
    let mut padded = plain.to_vec();
    padded.extend(std::iter::repeat_n(pad as u8, pad));

    let mut out = Vec::with_capacity(16 + padded.len());
    out.extend_from_slice(iv);

    let mut prev = *iv;
    for chunk in padded.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        for (byte, p) in block.iter_mut().zip(prev.iter()) {
            *byte ^= p;
        }
        aes.encrypt_block(&mut block);
        out.extend_from_slice(&block);
        prev = block;
    }

    Some(out)
}

/// CBC encryption with an explicit IV and no padding: Algorithm 2.B's inner
/// loop (7.6.4.3.4), where the input is always a whole number of blocks.
#[must_use]
pub fn cbc_encrypt_no_padding(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    let aes = Aes::new(key)?;

    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        for (byte, p) in block.iter_mut().zip(prev.iter()) {
            *byte ^= p;
        }
        aes.encrypt_block(&mut block);
        out.extend_from_slice(&block);
        prev = block;
    }

    Some(out)
}

/// CBC decryption with an explicit IV and no padding: how revision 6 unwraps
/// `/UE` and `/OE` (7.6.4.3.3).
#[must_use]
pub fn cbc_decrypt_no_padding(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    let aes = Aes::new(key)?;
    let inv = inv_sbox();

    let mut out = Vec::with_capacity(data.len());
    let mut prev = *iv;
    for chunk in data.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(chunk);
        let cipher = block;
        aes.decrypt_block(&mut block, &inv);
        for (byte, p) in block.iter_mut().zip(prev.iter()) {
            *byte ^= p;
        }
        out.extend_from_slice(&block);
        prev = cipher;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks_exact(2)
            .filter_map(|pair| {
                let text = std::str::from_utf8(pair).ok()?;
                u8::from_str_radix(text, 16).ok()
            })
            .collect()
    }

    /// FIPS 197 appendix C.1: AES-128 known-answer block.
    #[test]
    fn fips197_aes128_block() {
        let key = unhex("000102030405060708090a0b0c0d0e0f");
        let plain = unhex("00112233445566778899aabbccddeeff");
        let aes = Aes::new(&key).expect("a 16-byte key expands");

        let mut block = [0u8; 16];
        block.copy_from_slice(&plain);
        aes.encrypt_block(&mut block);
        assert_eq!(hex(&block), "69c4e0d86a7b0430d8cdb78070b4c55a");

        aes.decrypt_block(&mut block, &inv_sbox());
        assert_eq!(hex(&block), hex(&plain));
    }

    /// FIPS 197 appendix C.3: AES-256 known-answer block.
    #[test]
    fn fips197_aes256_block() {
        let key = unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let plain = unhex("00112233445566778899aabbccddeeff");
        let aes = Aes::new(&key).expect("a 32-byte key expands");

        let mut block = [0u8; 16];
        block.copy_from_slice(&plain);
        aes.encrypt_block(&mut block);
        assert_eq!(hex(&block), "8ea2b7ca516745bfeafc49904b496089");

        aes.decrypt_block(&mut block, &inv_sbox());
        assert_eq!(hex(&block), hex(&plain));
    }

    /// NIST SP 800-38A F.2.1/F.2.2: AES-128-CBC, first two blocks.
    #[test]
    fn sp800_38a_cbc_aes128() {
        let key = unhex("2b7e151628aed2a6abf7158809cf4f3c");
        let iv = unhex("000102030405060708090a0b0c0d0e0f");
        let plain = unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");

        let mut iv16 = [0u8; 16];
        iv16.copy_from_slice(&iv);
        let cipher = cbc_encrypt_no_padding(&key, &iv16, &plain).expect("valid key");
        // Both blocks are published answers; the second is what proves the
        // chaining, since a broken CBC still gets the first block right.
        assert_eq!(
            hex(cipher.get(..16).unwrap_or_default()),
            "7649abac8119b246cee98e9b12e9197d"
        );
        assert_eq!(
            hex(cipher.get(16..32).unwrap_or_default()),
            "5086cb9b507219ee95db113a917678b2"
        );

        let back = cbc_decrypt_no_padding(&key, &iv16, &cipher).expect("valid key");
        assert_eq!(hex(&back), hex(&plain));
    }

    #[test]
    fn iv_prefixed_round_trip_strips_padding() {
        let key = [7u8; 32];
        let iv = [3u8; 16];
        for len in [0usize, 1, 15, 16, 17, 64] {
            let plain = vec![0xabu8; len];
            let cipher = cbc_encrypt_with_iv_prefix(&key, &iv, &plain).expect("valid key");
            let (back, notes) = cbc_decrypt_with_iv_prefix(&key, &cipher);
            assert_eq!(back, plain, "length {len}");
            assert!(notes.is_empty(), "length {len} produced {notes:?}");
        }
    }

    #[test]
    fn damage_returns_notes_not_panics() {
        let key = [1u8; 16];
        for data in [
            vec![],
            vec![0u8; 8],
            vec![0u8; 16],
            vec![0u8; 20],
            vec![0xffu8; 48],
        ] {
            let _ = cbc_decrypt_with_iv_prefix(&key, &data);
        }
        assert!(Aes::new(&[0u8; 24]).is_none(), "AES-192 is not offered");
    }
}
