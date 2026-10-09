# Encryption

The standard security handler, both directions: documents encrypted at any
revision from R2 to R6 open, authenticate and decrypt, and documents are
encrypted on save at R6 (AES-256). An *incremental* save is the one place the
writer does not choose: it appends into a file whose `/Encrypt` still stands,
so it re-encrypts with that file's own key and methods, whichever of the four
they are ([writing](writing.md)). The public-key handler goes both ways
too: a document sealed to certificates opens with the holder's key, and a
document can be sealed on save to certificates the caller supplies. Every
primitive — MD5, SHA-1, SHA-2, RC4, AES-CBC, and RSA's public-key half — is
the project's own, living in the `tinker-pdf-crypto` leaf
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

**Writing to recipients.** `PublicKeyEncryption::seal(certificates,
permissions, entropy)` seals a document to DER X.509 certificates instead of a
password (7.6.5), and `DocumentEditor::save_sealed(options, &sealed)` writes
it: `/Filter /Adobe.PubSec`, `/SubFilter /adbe.pkcs7.s5`, `/V 5`, and one
`/AESV3` crypt filter whose `/Recipients` is a single CMS `EnvelopedData`
that every recipient can open. Sealing draws a twenty-byte seed, seals it with
the permissions to each certificate's RSA key (RSAES-PKCS1-v1_5 key
transport, AES-256-CBC content, the shape `openssl cms -encrypt -aes256`
writes), and derives the file key from the seed and the envelope; from there
the file is encrypted exactly as an R6 password write is, object streams and
linearized layout included. The randomness is the caller's `EntropySource`,
as for a password. Every certificate is checked before any entropy is drawn,
so a bad one costs nothing, and each refusal names the recipient by its index. The
sealing happens in `seal`, which can fail; `save_sealed` fails only when it is
asked for an incremental save or for a password as well. It sits beside
`save` rather than in `WriteOptions`, because `Encryption` is a struct its
callers build by field and `save` has no error to return
([design/pubsec.md](../design/pubsec.md)).

**An empty owner password is the user's.** Algorithm 2.A tries the owner
password before the user's, and a reader tries the empty password before
asking for one, so an `/O` derived from the empty string opened the file
with the owner's authority — and the file key — for anybody, whatever the
user password was. That was the plainest call on every surface: the C ABI
documents a null owner password as "none" and the Python binding defaults
to one. The writer now takes Algorithm 3 step (a)'s rule for R2 to R4 — "if
there is no owner password, use the user password instead" — for R6 too, so
a document with only a user password opens with it and with nothing else
(`an_empty_owner_password_is_the_user_password`, and through the C ABI
`an_owner_password_left_null_is_the_users`). What it costs is stated rather
than hidden: whoever has the user password then has the owner's authority,
so `permissions` restrict nobody who can open the file unless an owner
password is given. With neither password the empty one is both, as before.
Found by `tpdf encrypt`, which refused the case on its own until the facade
made the decision for every surface (ruling 11).

**A save that would undo the encryption is the editor's question.** A
rewrite of an encrypted document asking for no encryption drops `/Encrypt`
and writes the plaintext — the objects were decrypted to be read — and so
does any save, of any document, of a page or a stamp copied in from an
encrypted one (`import_page`, `import_page_as_form`), with no encryption of
its own. Replacing or removing the encryption of a document opened with the
user's password lifts whatever the owner withheld from that user (7.6.4.2,
Table 22). `DocumentEditor::check_save(&WriteOptions)` answers both before
anything is written: `SaveRefusal::WouldDecrypt` for the first, unless the
save is a rewrite that brings its own encryption, or an incremental update
sealed with the key the document was opened with, where that key encrypts
both streams and strings; and
`SaveRefusal::OwnerAuthorityNeeded` for the second, whose `withheld()`
names the Table 22 permissions in the table's order. An incremental update
is sealed with that key and nothing else (7.6.2): it does not read
`WriteOptions::encryption`, so encryption asked of one seals nothing and
replaces nothing, and a document that is not encrypted, or was opened
without its password, has no key — an update of it writes anything copied
in from an encrypted source in the clear, and is refused. So is an update
under a key whose `/StmF` or `/StrF` is `/Identity` (an absent one is, and so
is a crypt filter whose `/CFM` is `/None`, 7.6.5): the update reproduces the
file's own encryption, which passes that half through unchanged, so what was
copied in is written as the file stores its own, in the clear. So too an
update under an AES method whose key AES does not take: Algorithm 1 (7.6.2)
keys `/AESV2` with n + 5 bytes of hash, so a `/V 4` document with a 40-bit
key — a `/Length` of 40, or none, which reads as 40 — authenticates with a
key that gets a ten-byte AES key, and encryption under it hands the bytes
back unchanged. `FileKey::seals` asks the encryption itself rather than
the method names.
`DocumentEditor::check_decrypt()` asks whether decrypting on purpose is
allowed: the document is encrypted (`SaveRefusal::NotEncrypted` otherwise),
and whoever opened it holds the owner's authority or was withheld nothing.
A rollback forgets an import it undoes
(`tinker-pdf-cos/tests/save_refusals.rs`). Permissions stay advisory and
the save doors do not ask — `save` returns bytes, so whether it refuses is
an API change the owner decides in the ROADMAP's CLI row — but `tpdf` asks
before every write, which until October 2026 it decided on its own.

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

Sealing to recipients instead, with certificates the host holds and the
host's randomness:

```rust
let sealed = PublicKeyEncryption::seal(&[certificate_der], permissions, &mut entropy)?;
let bytes = doc.editor().save_sealed(&WriteOptions::default(), &sealed)?;
// A holder opens it with Document::authenticate_with_recipient(&their_key).
```

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| A vendor's own `/Filter` | `AuthError::UnsupportedHandler` | only `Standard` and `Adobe.PubSec` are implemented; a foreign handler is refused rather than guessed | this page |
| A password offered to a public-key document | `AuthError::UnsupportedHandler`, not `WrongPassword` | no password was ever going to work, and saying "wrong password" sends a caller looking for a better one | 7.6.5 |
| Sealing to a key that is not RSA | `SealError::NotRsa { index }` | key transport here is RSAES-PKCS1-v1_5, every PDF public-key handler's; key agreement is not read either | [design/pubsec.md](../design/pubsec.md) |
| Sealing to an RSA key its certificate restricts to signing | `SealError::KeyRestricted { index }` | RFC 4055 §1.2: a key published under `id-RSASSA-PSS` is for RSASSA-PSS signatures only, and only `rsaEncryption` leaves it free for key transport; `openssl cms -encrypt` refuses the same certificate | [design/pubsec.md](../design/pubsec.md) |
| A sealed incremental save | `SealError::NotRewrite` | an update appends under an `/Encrypt` that still stands, and cannot change who the file is sealed to | this page |
| A password and recipients both | `SealError::PasswordAlsoRequested` | a document has one security handler | 7.6 |
| Recipients sealed with different permissions | none offered — one envelope, one `/P` for all | 7.6.5 allows an envelope per group; not yet asked for | [design/pubsec.md](../design/pubsec.md) |
| Sealing below `/V 5` — `s3`, `s4`, RC4, AES-128 | none offered — the writer emits `AESV3` | as R6 is the password writer's only revision; reading all of them is unchanged | — |
| An envelope's content cipher this build does not implement — AES-192-CBC and RC2 are the reachable ones | `PubSecError::UnsupportedContentCipher` | AES-128/256-CBC, RC4 and `des-ede3-cbc` are implemented, the last being what OpenSSL still picks by default for older recipients; anything else is named rather than silently unopenable | [design/pubsec.md](../design/pubsec.md) |
| Recipient shapes other than key transport | `EnvelopedError::UnsupportedRecipientKind` | key agreement, KEK and password recipients are recognised by tag and refused; every PDF public-key handler in the wild uses key transport | RFC 5652 §6.2 |
| Writing R5 | none offered — the writer emits R6 and nothing else | R5 is the withdrawn draft; reading it works and carries `HandlerNote::DeprecatedRevision5` | [pdf20-deltas](../pdf20-deltas.md) |
| Wrong password / unencrypted document | `AuthError::WrongPassword`, `AuthError::NotEncrypted` | the ordinary API errors, typed so a prompt loop can tell them apart | — |

## The public-key handler, and what it is not verified against

`/Adobe.PubSec` (7.6.5) reads: `Document::authenticate_with_recipient` takes a
[`Recipient`] the caller implements, hands it the sealed key and the identifier
saying whose it is, and the caller does the one private-key operation this
engine holds no key material to do. The envelope is CMS `EnvelopedData` and is
parsed by `tinker-pdf-pki`; the file key is the digest 7.6.5 describes over the
unsealed seed and every `/Recipients` string in file order.

**It has no corpus behind it at all — zero of 5 605 files use this handler**
(re-measured 15 September 2026; `/Adobe.PubSec` and `/Recipients` are each
zero), and no tool available here produces one, qpdf included. So the evidence
splits three ways, and only the middle layer is weak:

- the envelope parsing is checked against structures **OpenSSL produced**,
  which is real interop;
- the **content ciphers are each held to published vectors** — FIPS 197 and
  RFC 6229 as before, and since Triple DES landed, 500 NIST CAVP known answers
  for `des-ede3-cbc`. That layer owes the corpus nothing;
- the key derivation between them is checked against a second implementation
  written from the same clause by the same author, which catches a
  transcription slip and **cannot catch a misreading**.

So a document this code opens is, at the derivation step, a document this code
agrees with itself about. [design/pubsec.md](../design/pubsec.md) records that
as an open risk rather than a footnote.

Writing narrows the gap from the other side without closing it. The envelope
the writer seals is RFC 5652's byte for byte outside its random fields, as is
the one OpenSSL sealed to the same certificate, both held to an encoding
written from the RFCs; and
**OpenSSL 3.0.13 opened one** (`cms -decrypt`, 2 October 2026) to the seed and
the permissions, after which `openssl enc -aes-256-cbc` decrypted every
content stream of the sealed file under the derived key. RSAES-PKCS1-v1_5 is
held to all 300 of RSA Laboratories' encryption known answers. The key
OpenSSL was handed, though, came from the same reading of 7.6.5 the reader
uses, so the derivation is still one author agreeing with themselves.

One reader gap the writer found and works around: ISO 32000-1 Table 27 puts the
public-key handler's `/EncryptMetadata` in the crypt filter, and the reader
looks for it on `/Encrypt` only. A third-party `/V 4` or `/V 5` file that
says `false` in its crypt filter derives the wrong key here. The writer writes
the flag nowhere, and both places default to true.

## Verified

Published vectors are merge gates, in-module in `tinker-pdf-crypto`:
FIPS 197 appendix C known-answer blocks and NIST SP 800-38A F.2 CBC
vectors (`aes.rs`), FIPS 180-4 examples for SHA-256/384/512 (`sha2.rs`)
and for SHA-1 including the million-`a` vector (`sha1.rs`), RFC 6229
keystream vectors (`rc4.rs`), the complete RFC 1321 appendix A.5 suite
(`md5.rs`), and for Triple DES **500 NIST CAVP known answers** across seven
committed files (`des.rs`) — the variable-plaintext and variable-key KATs,
which between them walk every plaintext and every key bit position, the
substitution-table, permutation-operation and inverse-permutation KATs, and
the two multi-block message tests. The five KATs are all keyed
`K1 = K2 = K3` and all single-block, so between them they cannot see EDE3's
*order*, the CBC chain, or the 16-byte key bundle at all; the three-key file
sees the first two and the two-key file the third, and a counted-injection
campaign is how that was established rather than assumed. `handler.rs` pins
constant-time comparison, Algorithm 2.B
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
composed with object streams. `crates/tinker-pdf/tests/pubsec_write.rs`
does the same for a document sealed to a certificate, its key holder a test
`Recipient` over the committed `visible-signer-key.der`.
`crates/tinker-pdf-cos/tests/strict_validator.rs`
holds the encrypted output — plain and linearized — to ISO 32000 read
strictly, including 7.5.5 Table 15's `/ID`, which no file this engine wrote
carried until the validator refused one.

That identifier is a hash of the document rather than of the moment (ruling 4
bans a clock), and on an encrypted write the caller's entropy goes into the
hash. Without it the identifier would be a confirmation oracle: anybody
holding a candidate document could hash it and check the `/ID`, which is in
the clear, with no password. For the same reason an encrypted write derives
*both* halves instead of inheriting 14.4's permanent one from its source.

Two of the 50 fuzz targets are this feature's: `crypt` drives
authentication with input-chosen field widths and both decrypt paths over
a handler the crate built itself, and `crypt_ciphers` drives the raw
primitives; their committed seeds are written by a test inside
`handler.rs` so seeds and carve order cannot drift, including the one
pre-R6 seed that genuinely authenticates. All of it rides in the
workspace suite: 4 879 passed, 0 failed, 58 ignored (Windows x86_64,
14 September 2026) — [verification](../verification.md).
