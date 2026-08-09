//! RC4, the stream cipher of PDF encryption revisions 2 to 4.
//!
//! Kept because the format requires it for files that already exist. It offers
//! no meaningful confidentiality at 40 bits and little at 128; the engine reads
//! such documents, and never writes them by choice.

/// RC4 keystream generator.
#[derive(Clone, Debug)]
pub struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    /// Key-scheduling algorithm. An empty key is accepted and produces the
    /// identity permutation's keystream rather than panicking — callers reach
    /// here from document bytes.
    #[must_use]
    pub fn new(key: &[u8]) -> Self {
        let mut s = [0u8; 256];
        for (i, slot) in s.iter_mut().enumerate() {
            *slot = i as u8;
        }

        if !key.is_empty() {
            let mut j = 0u8;
            for i in 0..256usize {
                let k = key.get(i % key.len()).copied().unwrap_or(0);
                let si = s.get(i).copied().unwrap_or(0);
                j = j.wrapping_add(si).wrapping_add(k);
                s.swap(i, usize::from(j));
            }
        }

        Self { s, i: 0, j: 0 }
    }

    /// XORs the keystream into `data` in place. RC4 is symmetric, so this both
    /// encrypts and decrypts.
    pub fn apply(&mut self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            let si = self.s.get(usize::from(self.i)).copied().unwrap_or(0);
            self.j = self.j.wrapping_add(si);
            self.s.swap(usize::from(self.i), usize::from(self.j));
            let a = self.s.get(usize::from(self.i)).copied().unwrap_or(0);
            let b = self.s.get(usize::from(self.j)).copied().unwrap_or(0);
            let k = self
                .s
                .get(usize::from(a.wrapping_add(b)))
                .copied()
                .unwrap_or(0);
            *byte ^= k;
        }
    }
}

/// One-shot RC4 over a copy of `data`.
#[must_use]
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    Rc4::new(key).apply(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// RFC 6229 test vectors: the first 16 keystream bytes for each key, taken
    /// by encrypting zeros.
    #[test]
    fn rfc6229_keystream_prefixes() {
        let cases: [(&[u8], &str); 5] = [
            (
                &[0x01, 0x02, 0x03, 0x04, 0x05],
                "b2396305f03dc027ccc3524a0a1118a8",
            ),
            (
                &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07],
                "293f02d47f37c9b633f2af5285feb46b",
            ),
            (
                &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
                "97ab8a1bf0afb96132f2f67258da15a8",
            ),
            (
                &[
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                    0x0e, 0x0f, 0x10,
                ],
                "9ac7cc9a609d1ef7b2932899cde41b97",
            ),
            (
                &[
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                    0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a,
                    0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
                ],
                "eaa6bd25880bf93d3f5d1e4ca2611d91",
            ),
        ];
        for (key, want) in cases {
            assert_eq!(hex(&rc4(key, &[0u8; 16])), want, "key {key:?}");
        }
    }

    #[test]
    fn encryption_is_its_own_inverse() {
        let key = b"an arbitrary key";
        let plain = b"streams and strings alike".to_vec();
        assert_eq!(rc4(key, &rc4(key, &plain)), plain);
    }

    #[test]
    fn an_empty_key_does_not_panic() {
        let _ = rc4(&[], b"data");
    }
}
