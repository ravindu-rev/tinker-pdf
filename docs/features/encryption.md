# Encryption

The standard security handler, both directions: documents encrypted at any
revision from R2 to R6 open, authenticate and decrypt, and documents are
encrypted on save at R6 (AES-256). Every primitive — MD5, SHA-1, SHA-2,
RC4, AES-CBC — is the project's own, living in the `tinker-pdf-crypto` leaf
crate (bytes and plain scalars in, bytes and values out, no PDF types on
its surface — ruling 8, [rulings](../rulings.md)), which is what lets it be
fuzzed on its own and gated on published vectors. PDF permissions are
advisory and nothing here pretends otherwise: a document that says printing
is denied is asking, not enforcing.

## What it does

**Reading.** `/Encrypt` and the trailer's `/ID` are flattened to plain
values and handed to the handler. Revisions 2–4 derive the file key by
7.6.4.3.2 Algorithm 2 and check the password against `/U` (7.6.4.4.4
Algorithm 4, 7.6.4.4.5 Algorithm 5), recovering the user password from
`/O` first (7.6.4.4.8 Algorithm 7) so the owner password is always tried
before the user one — a document whose two passwords are equal
authenticates at the higher level. R6 runs the hardened hash of 7.6.4.3.4
Algorithm 2.B and unwraps the file key from `/UE` or `/OE` (7.6.4.3.3
Algorithm 2.A); R5, the withdrawn draft, reads with a warning and a single
SHA-256 in place of the rounds ([pdf20-deltas](../pdf20-deltas.md)). Every
password and hash comparison is constant-time (`constant_time_eq`).

Ciphers are resolved through `/CF`, `/StmF` and `/StrF` (7.6.5 Tables 24
and 25): `V2` is RC4, `AESV2` is AES-128-CBC with an IV prefix, `AESV3` is
AES-256-CBC keyed by the file key itself. Before `/V 4` everything is RC4.
Pre-R5 revisions salt a per-object key with the containing object's number
and generation (7.6.2 Algorithm 1); strings decrypt when their containing
indirect object loads, streams in `stream_raw`/`stream_decoded`, and three
things are never handed to a decryptor at all (7.6.2): the `/Encrypt`
dictionary itself, cross-reference streams, and strings inside object
streams — the container was decrypted whole, so its contents already are.

Two opt-outs the file can declare are honoured. `/EncryptMetadata false`
leaves the metadata stream in the clear (and mixes `0xFFFFFFFF` into the
legacy key derivation, 7.6.4.3.2 step f); a stream whose `/Crypt` filter
names `/Identity` (7.4.10) — an appearance stream a signature covers, most
often — passes through undecrypted.

`/P` is kept exactly as the file stores it and read bit by bit (7.6.4.2
Table 22). This is deliberate: the specification requires the reserved
bits to be 1, so a strict bitflags-style parse rejects every genuine value
and falls back to reporting everything as permitted. At R6, `/Perms` is
decrypted and checked against `/P` and the `adb` sentinel (7.6.4.3.3 step
f); a mismatch means the permissions may have been edited after the fact
and is reported as a warning.

Leniency is typed, never silent (ruling 10): a missing `/R` is inferred
from `/V`, a short `/U` or `/O` is noted, an over-long R6 password is
truncated at 127 bytes (7.6.4.3.3) — each becomes a
`WarningKind::SecurityHandler(HandlerNote)` on the document's warning
list: `RevisionInferred`, `ShortPasswordEntry`, `PasswordTruncated`,
`PermsMismatch`, `DeprecatedRevision5`. A `/Length` written in bytes rather
than bits is the one tolerance without a note: the key-length rule reads
either spelling into the legal 5–16 byte range and both land on the same
key, so there is nothing to report.

**Writing.** `WriteOptions::encryption` encrypts a rewrite at R6:
`build_r6` derives `/U`, `/UE`, `/O`, `/OE` and `/Perms` (7.6.4.3.3
Algorithms 8, 9 and 10) from the two passwords and 48 caller-supplied
bytes of entropy — the 32-byte file key and two 8-byte salts. The engine
links no random number generator: `wasm32-unknown-unknown` has no
operating system to ask, so the caller supplies randomness (the
`EntropySource` trait states the contract). Every string and stream is
encrypted (7.6.2, 7.6.3.2) with a per-object initialisation vector derived
from the file key and object number, so identical plaintexts never
encrypt identically; ciphertext strings are written in hex form.
Encryption composes with object streams and with linearized output
(Annex F) — the linearized writer encrypts each object as it serialises it
and measures the layout from the ciphertext. A rewrite of an opened
encrypted document without the option decrypts on the way through and
drops `/Encrypt`, because carrying it forward over plaintext would make
every reader decrypt clear bytes into garbage.

## API

On `Document`: `is_encrypted()`, `authenticate(&self, password)`
returning which password matched, `auth_level()`, `permissions()`, and
`readable()`, which distinguishes "this is not a PDF" from "this is a PDF
and it wants a password" (`DocumentError::PasswordRequired`,
`DocumentError::UnsupportedEncryption`). `authenticate` takes `&self` and
installs the decryptor through interior mutability, so it works on a
document that has already been shared — cloned, or had a page taken from
it. `AuthLevel` is ordered (`None < User < Owner`) so `>=` expresses "at
least this much authority"; the owner password lifts every restriction,
and `Permissions` reads its named bits (`print`, `modify`, `copy`,
`annotate`, `fill_forms`, `accessibility`, `assemble`, `print_high_res`)
from the raw `/P`, which `raw()` round-trips exactly.

```rust
let doc = Document::open(bytes)?;
if doc.is_encrypted() {
    let level = doc.authenticate("open-sesame")?; // AuthLevel::User or ::Owner
    if level == AuthLevel::User && !doc.permissions().print() {
        // the document asks that printing be denied
    }
}
```

Encrypting on save goes through `DocumentEditor::save` (see
[writing](writing.md)) with `WriteOptions::encryption`:

```rust
let bytes = doc.editor().save(&WriteOptions {
    encryption: Some(Encryption {
        user_password: "open-sesame".into(),
        owner_password: "owner-secret".into(),
        permissions: -1,
        entropy, // 48 random bytes from the host
    }),
    ..WriteOptions::default()
});
```

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| Public-key and vendor handlers (`Adobe.PubSec` and kin) | `AuthError::UnsupportedHandler` | only the `Standard` handler is implemented; a foreign `/Filter` is refused rather than guessed | [ROADMAP](../ROADMAP.md) Tier 3 |
| Writing R5 | none offered — the writer emits R6 and nothing else | R5 is the withdrawn draft; reading it works and carries `HandlerNote::DeprecatedRevision5` | [pdf20-deltas](../pdf20-deltas.md) |
| Encrypting an incremental update | no typed variant: the incremental writer takes no cipher, so the combination cannot be requested | an update inherits the original file's encryption, which needs the original file key plumbed through | [ROADMAP](../ROADMAP.md) Tier 2 |
| Wrong password / unencrypted document | `AuthError::WrongPassword`, `AuthError::NotEncrypted` | the ordinary API errors, typed so a prompt loop can tell them apart | — |

## Verified

Published vectors are merge gates, in-module in `tinker-pdf-crypto`:
FIPS 197 appendix C known-answer blocks and NIST SP 800-38A F.2 CBC
vectors (`aes.rs`), FIPS 180-4 examples for SHA-256/384/512 (`sha2.rs`)
and for SHA-1 including the million-`a` vector (`sha1.rs`), RFC 6229
keystream vectors (`rc4.rs`), the complete RFC 1321 appendix A.5 suite
(`md5.rs`). `handler.rs` pins constant-time comparison, Algorithm 2.B
termination and the `/P -2056` fixture that reads "printing denied,
copying permitted" correctly.

`crates/tinker-pdf-cos/tests/encryption.rs` decrypts real encrypted
fixtures: user and owner passwords told apart, a wrong password refused,
`/P` surviving its reserved bits, the owner password lifting restrictions,
metadata left clear under `/EncryptMetadata false`, an `/Identity` `/Crypt`
filter left alone, and a cross-reference stream never decrypted.
`crates/tinker-pdf-cos/tests/encrypt_on_save.rs` round-trips
encrypt-on-save through the engine's own reader: password required,
content and `/Info` strings ciphertext on disk and intact after
authentication, owner and user passwords distinguished, `/P` surviving
the trip, identical plaintexts encrypting differently, and encryption
composed with object streams. `crates/tinker-pdf-cos/tests/strict_validator.rs`
holds the encrypted output — plain and linearized — to ISO 32000 read
strictly, including 7.5.5 Table 15's `/ID`, which no file this engine wrote
carried until the validator refused one.

That identifier is a hash of the document rather than of the moment (ruling 4
bans a clock), and on an encrypted write the caller's entropy goes into the
hash. Without it the identifier would be a confirmation oracle: anybody
holding a candidate document could hash it and check the `/ID`, which is in
the clear, with no password. For the same reason an encrypted write derives
*both* halves instead of inheriting 14.4's permanent one from its source.

Two of the 24 fuzz targets are this feature's: `crypt` drives
authentication with input-chosen field widths and both decrypt paths over
a handler the crate built itself, and `crypt_ciphers` drives the raw
primitives; their committed seeds are written by a test inside
`handler.rs` so seeds and carve order cannot drift, including the one
pre-R6 seed that genuinely authenticates. All of it rides in the
workspace suite: 2 939 passed, 0 failed, 8 ignored (Windows x86_64,
August 2026) — [verification](../verification.md).
