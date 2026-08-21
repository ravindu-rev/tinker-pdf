//! Hand-rolled cryptography for PDF's standard security handler.
//!
//! A leaf crate: bytes and plain scalars in, bytes and values out, no PDF or
//! COS types anywhere in its surface (ruling 8). That is what lets it be
//! fuzzed, tested against published vectors, and read on its own.
//!
//! Scope, design and exit criteria: `docs/plans/03-encryption.md`.
//!
//! # On trusting this
//!
//! Implementing MD5, RC4, SHA-2 and AES by hand is unusual and deserves the
//! scrutiny it invites. The honest framing, also recorded in `SECURITY.md`:
//! the scope is document decryption and encryption-on-save, not a protocol;
//! correctness is gated on published vectors (FIPS 197, FIPS 180-4, RFC 6229,
//! RFC 1321) as merge requirements rather than aspirations; password
//! comparison is constant-time. PDF's own older revisions are weak by design —
//! RC4 at 40 bits protects nothing — and its permission flags are advisory:
//! a document saying "printing denied" is asking, not enforcing. Nothing here
//! changes that, and no caller should treat PDF permissions as a security
//! boundary.

pub mod aes;
pub mod handler;
pub mod md5;
pub mod rc4;
pub mod sha1;
pub mod sha2;

pub use handler::{authenticate, AuthOutcome, CryptMethod, FileKey, HandlerNote, HandlerParams};

/// A document's permission flags (7.6.4.2, Table 22).
///
/// The raw `/P` integer is kept exactly as the file stores it and each flag is
/// read as a bit. This is deliberate, and it is the fix for a real bug: the
/// specification requires `/P`'s reserved bits to be 1, so a strict
/// bitflags-style parse rejects every genuine value — which is how the engine
/// this replaces came to report every document, including ones that forbid
/// printing, as entirely unrestricted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    raw: i32,
}

impl Permissions {
    /// Wraps a `/P` value.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        Self { raw }
    }

    /// Everything permitted: an unencrypted document, or one opened with the
    /// owner password.
    #[must_use]
    pub const fn all() -> Self {
        Self { raw: -1 }
    }

    /// `/P` exactly as stored, for callers that must round-trip it.
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.raw
    }

    /// Bit `n`, counting from 1 as the specification's tables do.
    const fn bit(self, n: u32) -> bool {
        self.raw & (1 << (n - 1)) != 0
    }

    /// Bit 3: print, at whatever quality bit 12 allows.
    #[must_use]
    pub const fn print(self) -> bool {
        self.bit(3)
    }

    /// Bit 4: modify the document's contents.
    #[must_use]
    pub const fn modify(self) -> bool {
        self.bit(4)
    }

    /// Bit 5: copy or otherwise extract text and graphics.
    #[must_use]
    pub const fn copy(self) -> bool {
        self.bit(5)
    }

    /// Bit 6: add or modify annotations.
    #[must_use]
    pub const fn annotate(self) -> bool {
        self.bit(6)
    }

    /// Bit 9, or bit 6, which also grants it (Table 22).
    #[must_use]
    pub const fn fill_forms(self) -> bool {
        self.bit(9) || self.bit(6)
    }

    /// Bit 10: extract for accessibility.
    ///
    /// PDF 2.0 deprecated this restriction — accessibility extraction is
    /// always permitted there — but 1.7 files still carry it, so it is
    /// reported rather than assumed.
    #[must_use]
    pub const fn accessibility(self) -> bool {
        self.bit(10)
    }

    /// Bit 11: insert, rotate or delete pages.
    #[must_use]
    pub const fn assemble(self) -> bool {
        self.bit(11)
    }

    /// Bit 12: print at full resolution rather than a degraded image.
    #[must_use]
    pub const fn print_high_res(self) -> bool {
        self.bit(12)
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Self::all()
    }
}

/// Where the writer gets randomness.
///
/// The engine links no random number generator: `wasm32-unknown-unknown` has
/// no operating system to ask, and a library that reaches for entropy behind
/// its caller's back is a library that cannot be embedded. Hosts supply this
/// when they encrypt (phase 09); reading a document never needs it.
pub trait EntropySource {
    /// Fills `out` with unpredictable bytes, or returns false if it cannot —
    /// in which case the caller must refuse to encrypt rather than proceed
    /// with a guessable key.
    fn fill(&mut self, out: &mut [u8]) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture that proved the previous engine wrong: `/P -2056` forbids
    /// printing and high-resolution printing, and permits copying.
    #[test]
    fn permissions_noprint_fixture_reads_correctly() {
        let p = Permissions::from_raw(-2056);
        assert!(!p.print(), "printing is denied");
        assert!(!p.print_high_res(), "high-resolution printing is denied");
        assert!(p.copy(), "copying is permitted");
        assert!(p.modify());
        assert!(p.annotate());
        assert!(p.assemble());
        assert_eq!(p.raw(), -2056, "the raw value round-trips");
    }

    #[test]
    fn reserved_bits_do_not_defeat_parsing() {
        // Every real /P has bits 1, 2, 7 and 8 set to 1 because the
        // specification reserves them. A parse that rejects unknown bits sees
        // this as invalid and falls back to permitting everything.
        let p = Permissions::from_raw(-1);
        assert!(p.print() && p.copy() && p.modify() && p.assemble());
    }

    #[test]
    fn filling_forms_follows_either_bit() {
        // Bit 6 alone grants form filling even with bit 9 clear.
        let annotate_only = Permissions::from_raw(0b10_0000);
        assert!(annotate_only.fill_forms());
        let forms_only = Permissions::from_raw(0b1_0000_0000);
        assert!(forms_only.fill_forms());
        assert!(!Permissions::from_raw(0).fill_forms());
    }

    #[test]
    fn nothing_permitted_is_representable() {
        let p = Permissions::from_raw(0);
        assert!(!p.print() && !p.copy() && !p.modify() && !p.annotate());
        assert!(!p.accessibility() && !p.assemble() && !p.print_high_res());
    }
}
