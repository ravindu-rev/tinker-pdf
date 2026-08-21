//! OCF's two font obfuscations, undone (gap 31, milestone 9).
//!
//! Milestone 3 parsed `META-INF/encryption.xml` far enough to know that a book
//! using either of these is a book this engine will read, and then read
//! nothing: the entries were recorded and no resource was ever de-obfuscated,
//! because there was no font path to hand a de-obfuscated face to. This is that
//! path's other end.
//!
//! # Neither of these is encryption
//!
//! Both are a XOR of a fixed key over the **first kilobyte** of the file and
//! nothing at all over the rest. They exist so that a font's licence terms are
//! not violated by dragging the file out of the archive with an unzipper, and
//! OCF 3.3 §4.4 says as much. So the key is derived from something the book
//! publishes in its own package document, the algorithm is named in a file
//! beside it, and undoing it is arithmetic. Nothing here protects anything and
//! nothing here should be described as though it does.
//!
//! # Why the tests assert bytes and not a page
//!
//! Gap 30's milestone 7 spent itself on exactly this: **a wrong key still
//! produces a font a reader will parse**, because the header a parser looks at
//! is often outside the obfuscated kilobyte or happens to survive the XOR, and
//! the page then draws *something*. A page that drew proves the chain ran. It
//! does not prove the key is right. So the assertions in `epub::tests` read
//! the de-obfuscated bytes out to a table tag and compare them with the
//! original font's, which is a claim only the right key can satisfy.
//!
//! # The two lengths are different, and neither is a round number by accident
//!
//! IDPF's covers **1 040** bytes, which is fifty-two repetitions of a 20-byte
//! SHA-1 digest. Adobe's covers **1 024**, which is sixty-four repetitions of a
//! 16-byte UUID. A build that used one length for both would leave sixteen
//! bytes of an Adobe-obfuscated font XORed with the wrong key — sixteen bytes
//! well inside the table directory of any real face — and a build that used the
//! wrong key length would corrupt everything past the first repetition.

use tinker_pdf_crypto::sha1::sha1;

use super::ocf::Obfuscation;

/// How many bytes OCF §4.4.4's obfuscation covers: fifty-two SHA-1 digests.
pub const IDPF_LENGTH: usize = 1040;

/// How many bytes Adobe's obfuscation covers: sixty-four UUIDs.
pub const ADOBE_LENGTH: usize = 1024;

/// Why a resource named in `encryption.xml` could not be de-obfuscated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyDefect {
    /// The package document has no unique identifier, so §4.4.3's key has no
    /// input at all.
    NoIdentifier,
    /// The unique identifier is not a UUID, which Adobe's algorithm requires
    /// and OCF's does not.
    ///
    /// A real distinction and not a nicety: a book obfuscated the IDPF way
    /// works with any identifier a publisher chose, and the same book
    /// obfuscated the Adobe way does not — so a reading system that reported
    /// "the key failed" for both would send a publisher looking in the wrong
    /// place.
    IdentifierIsNotAUuid,
}

/// §4.4.3's key: SHA-1 of the whitespace-stripped unique identifier.
///
/// **The stripping is the whole subtlety.** §4.4.3 says to remove all
/// whitespace — space, tab, carriage return and line feed — before hashing,
/// and an XML parser hands back a `dc:identifier` with whatever indentation
/// the producer's pretty-printer put around it. A build that hashed the
/// element's text as it came out of the parser would compute a different key
/// for the same book depending on how the package document was formatted,
/// which is a bug that only shows on books nobody wrote by hand.
///
/// Whitespace is stripped from **inside** as well as from the ends: an
/// identifier written across two lines in a wrapped package document has a
/// newline in the middle of it, and trimming only the ends would leave it.
///
/// # Errors
/// [`KeyDefect::NoIdentifier`] when the stripped identifier is empty. Hashing
/// the empty string is a perfectly good SHA-1 and a perfectly wrong key, and
/// the digest of nothing is exactly the kind of plausible answer that reaches
/// a page as a font full of noise.
pub fn idpf_key(identifier: &str) -> Result<[u8; 20], KeyDefect> {
    let stripped: String = identifier
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
        .collect();
    if stripped.is_empty() {
        return Err(KeyDefect::NoIdentifier);
    }
    Ok(sha1(stripped.as_bytes()))
}

/// Adobe's key: the sixteen bytes of the identifier's UUID.
///
/// The identifier is expected as `urn:uuid:<UUID>`, which is what a book using
/// this obfuscation writes, and the prefix is optional here because a package
/// that wrote the bare UUID means the same thing. What is **not** optional is
/// that there are exactly thirty-two hexadecimal digits: an identifier that is
/// an ISBN, a DOI or a publisher's own string has no UUID in it and Adobe's
/// algorithm has no key for it.
///
/// # Errors
/// [`KeyDefect::NoIdentifier`] for an empty identifier, and
/// [`KeyDefect::IdentifierIsNotAUuid`] for one that is not a UUID.
pub fn adobe_key(identifier: &str) -> Result<[u8; 16], KeyDefect> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return Err(KeyDefect::NoIdentifier);
    }
    let body = trimmed
        .strip_prefix("urn:uuid:")
        .or_else(|| trimmed.strip_prefix("URN:UUID:"))
        .unwrap_or(trimmed);
    let digits: Vec<u8> = body
        .chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<Vec<u8>>>()
        .ok_or(KeyDefect::IdentifierIsNotAUuid)?;
    if digits.len() != 32 {
        return Err(KeyDefect::IdentifierIsNotAUuid);
    }
    let mut key = [0u8; 16];
    for (slot, pair) in key.iter_mut().zip(digits.chunks_exact(2)) {
        *slot = (pair[0] << 4) | pair[1];
    }
    Ok(key)
}

/// Undoes one of the two obfuscations, in place.
///
/// A file **shorter** than the obfuscated length is XORed as far as it goes,
/// which is what §4.4.4 says: the algorithm covers *"the first 1040 bytes, or
/// the whole file if it is shorter"*. A build that refused a short file would
/// refuse every small font, and a build that read past the end would panic on
/// one.
///
/// # Errors
/// Whatever the key derivation could not do.
pub fn deobfuscate(
    algorithm: Obfuscation,
    identifier: &str,
    bytes: &mut [u8],
) -> Result<(), KeyDefect> {
    match algorithm {
        Obfuscation::Idpf => {
            let key = idpf_key(identifier)?;
            xor(bytes, &key, IDPF_LENGTH);
        }
        Obfuscation::Adobe => {
            let key = adobe_key(identifier)?;
            xor(bytes, &key, ADOBE_LENGTH);
        }
    }
    Ok(())
}

/// The XOR itself: `key` repeated over the first `length` bytes.
///
/// Its own function so that the two algorithms differ **only** in their key and
/// their length. A build with the loop written out twice is a build where one
/// copy can acquire an off-by-one the other does not have, and the two would
/// then disagree about a font neither test covers.
fn xor(bytes: &mut [u8], key: &[u8], length: usize) {
    if key.is_empty() {
        return;
    }
    let end = length.min(bytes.len());
    for (index, byte) in bytes.iter_mut().take(end).enumerate() {
        *byte ^= key[index % key.len()];
    }
}
