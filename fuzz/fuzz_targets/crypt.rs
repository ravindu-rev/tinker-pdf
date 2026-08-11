//! The standard security handler and the ciphers under it.
//!
//! `/Encrypt` is read before anything is decrypted, which makes it the one
//! dictionary a document can use against a reader who has not authenticated
//! yet. Its entries are fixed-width byte strings whose widths the file
//! declares, so the interesting inputs are the ones that are the wrong
//! length, and the interesting revisions are the ones nobody writes any more.
//!
//! The input is carved into `/Encrypt`'s fields in a fixed order, shorter
//! inputs being zero-filled, so every length of input produces a
//! well-shaped-but-hostile handler rather than being thrown away. Whatever is
//! left over is the ciphertext.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_crypto::{aes, authenticate, md5, rc4, sha2, CryptMethod, HandlerParams};

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

fuzz_target!(|data: &[u8]| {
    let mut carve = Carve { data, at: 0 };
    let control = carve.byte();
    let filters = carve.byte();

    let params = HandlerParams {
        v: i64::from(control & 0x07),
        r: i64::from((control >> 3) & 0x07),
        // 40 to 264 bits, so both the legal multiples of eight and the
        // lengths no key can be are reachable.
        length_bits: 40 + 8 * i64::from(carve.byte() % 29),
        // Declared as 32 bytes before revision 6 and 48 at revision 6; the
        // handler has to survive being handed either at either revision.
        o: carve.take(48),
        u: carve.take(48),
        oe: carve.take(32),
        ue: carve.take(32),
        perms: carve.take(16),
        p: i32::from_be_bytes([carve.byte(), carve.byte(), carve.byte(), carve.byte()]),
        id_first: carve.take(16),
        encrypt_metadata: control & 0x40 == 0,
        stream_method: method(filters),
        string_method: method(filters >> 2),
    };

    let password_len = usize::from(carve.byte()) % 40;
    let password = carve.take(password_len);
    let body = carve.rest();

    // The empty password is what every reader tries first, and the path a
    // document that "is not encrypted as far as the user is concerned" takes.
    for attempt in [b"".as_slice(), password.as_slice()] {
        let Some(key) = authenticate(&params, attempt) else {
            continue;
        };
        let _ = key.outcome();
        let _ = key.notes();
        let _ = key.key();

        // Object number and generation salt the per-object key, and both come
        // from the file.
        for (num, gen) in [(1u32, 0u16), (0, 0), (u32::MAX, u16::MAX)] {
            let _ = key.decrypt_string(num, gen, body);
            let _ = key.decrypt_stream(num, gen, body);
        }
    }

    // The ciphers under the handler, driven directly: a stream's first
    // sixteen bytes are its IV, its length need not be a multiple of the
    // block size, and its padding byte is whatever the file says.
    let (head, tail) = body.split_at(body.len().min(32));
    let _ = aes::cbc_decrypt_with_iv_prefix(head, tail);
    let _ = aes::cbc_decrypt_with_iv_prefix(&params.perms, body);
    if let Ok(iv) = <[u8; 16]>::try_from(params.id_first.as_slice()) {
        let _ = aes::cbc_decrypt_no_padding(head, &iv, tail);
    }
    let _ = rc4::rc4(head, tail);
    let _ = md5::md5(body);
    let _ = sha2::sha256(body);
    let _ = sha2::sha384(body);
    let _ = sha2::sha512(body);
});
