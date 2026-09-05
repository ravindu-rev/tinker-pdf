# The public-key security handler

ISO 32000-1 7.6.5's `/Adobe.PubSec` seals a document's file key to
certificates instead of to a password. A recipient unseals a CMS
`EnvelopedData` in `/Recipients` with their private key, recovers a twenty-byte
seed, and the file key is a digest over that seed and every envelope in the
array. Everything after the key — Algorithm 1's per-object salting, the four
crypt methods, the decryptor a document installs — is what the standard
handler already does.

This design doc exists for a reason the roadmap's size band does not capture.
The item is **M**, and by the letter of this repository's process an M-sized
item "lives in the roadmap alone". It gets a doc anyway, because what needs
writing down here is not the design but the **evidence**, and the evidence is
unlike anything else in the tree.

## Scope

- Read `/Filter /Adobe.PubSec` with `/SubFilter /adbe.pkcs7.s3`, `s4` or `s5`,
  taking `/Recipients` from the `/Encrypt` dictionary or, from `/V 4`, from a
  crypt filter's own.
- Parse the envelopes with `tinker-pdf-pki` (RFC 5652 §6), read only as far as
  finding a recipient: `KeyTransRecipientInfo`, both `RecipientIdentifier`
  shapes, and the content-encryption algorithm.
- Derive the file key per 7.6.5 and install the same decryptor the standard
  handler installs.
- A `Recipient` seam so the host does the private-key operation.

## Non-goals

- **Writing.** Nothing produces a public-key-encrypted document. Encrypting on
  save would need to choose recipients, generate a seed and seal it — and the
  sealing is the private-key side's public half, which needs a certificate
  the engine has no business choosing.
- **Key material of any kind.** No PKCS#8, no PKCS#12, no passphrase handling,
  no RSA private-key arithmetic. The same rule signing follows.
- **`KeyAgreeRecipientInfo`, `KEKRecipientInfo`, `PasswordRecipientInfo`** and
  the `other` shape (§6.2.2–§6.2.5). Recognised by tag and refused by name.
- **Triple DES** as a content-encryption algorithm. This tree has AES and RC4
  and no DES, and OpenSSL still emits `des-ede3-cbc` by default for older
  recipients — so it is the one real gap, and it is named where it is met.
- **Applying the enveloped permissions.** The unsealed content carries four
  bytes of `/P` and this build ignores them. PDF permissions are advisory
  (see [encryption](../features/encryption.md)), so honouring a second source
  of them would add a code path whose only effect is to disagree with the
  first.

## Design

`crates/tinker-pdf-cos/src/pubsec.rs`, beside `security.rs`, because a security
handler is that crate's charter. The envelope is DER, which is
`tinker-pdf-pki`'s, so this adds the edge `cos → pki` — the ninth amendment in
`xtask/src/main.rs` argues it, and the argument is the same one `cos → font`
already makes: reading a `/Recipients` envelope is object-model work that needs
a leaf's parser.

The alternative was putting the handler in the facade, which already depends on
both crates. It was rejected on what it would cost: installing a decryptor is
`CosDocument`'s own operation, so a facade-level handler needs
`set_decryptor_with_key` to become public — and a public "install this
decryptor on an opened document" is a hole with no floor under it, offered so
that a dependency edge could be avoided.

**`FileKey::from_derived`** is the other door into `tinker-pdf-crypto`'s key
type. Everything after the file key is identical between the two handlers, and
two implementations of Algorithm 1 would be two chances to get the per-object
salt wrong in a way that produces plausible garbage.

**The order of `/Recipients` is load-bearing** and the parsed form keeps the
stored bytes because of it. 7.6.5 digests each envelope in full, in file order;
a reader that re-serialised one, or sorted the array, would derive a key that
decrypts nothing, with no error anywhere to say why.

## The evidence, and what it is not

**No corpus file uses this handler.** Zero of 5 594, re-measured on 6 September 2026 after a thousand real-world documents were pinned. Ruling 3 schedules
capabilities by corpus hit-rate, so this one was built because it was asked
for and not because the evidence called for it — and the consequence is that
its verification cannot be what the rest of this crate's is.

The layers are not equally weak and the difference matters:

| Layer | Evidence | Strength |
| --- | --- | --- |
| `EnvelopedData` parsing | Three envelopes **OpenSSL 3.5.5 produced**, in three content ciphers, with the recipient identifier checked against the certificate's own issuer and serial | Real interop: the parser reads what another implementation writes |
| RSA unsealing | Not implemented here at all — it is the host's | N/A |
| Content decryption | AES-CBC and RC4, already gated on FIPS 197 and RFC 6229 | Published vectors |
| **7.6.5 key derivation** | A second implementation in Python, written from the same clause by the same author, generating the end-to-end fixture | **Catches a transcription slip; cannot catch a misreading** |
| The document decrypting | `pubsec.rs`'s tests read "Public key" back out of the page and `PubSec fixture` out of `/Info` | Ties every layer together, on one file |

There was no way to do better. No tool on the machine this was written on
produces a public-key-encrypted PDF — **qpdf 12.3.2, which is installed, has no
public-key support at all** — and the corpus has none to borrow. This is the
same class of gap `verification.md` names for the whole suite, in its sharpest
form: reader, writer, fixture and reviewer share one author's reading of one
clause, and if that reading is wrong everything agrees and everything is wrong.

**Not closed.** It closes the day a real public-key-encrypted document arrives,
and until then `docs/features/encryption.md` says so where a caller will see
it rather than only here.

## Risks

| Risk | Mitigation | Closed? |
| --- | --- | --- |
| The key derivation is misread and nothing catches it | Stated in the module doc, the feature doc and here; the derivation is small and each step is asserted separately in unit tests | **No** |
| A wrong key reads as a decrypted document | 7.6.5 has no verifier the way `/U` is one, so this cannot be caught at authentication; a test pins that the failure stays visible as garbage rather than becoming plausible text | Partly |
| An attacker-chosen envelope drives the parser | The parser is `tinker-pdf-pki`'s, under ruling 1 with its own fuzz targets; the tests sweep every single-byte mutation of a real envelope | Yes |
| A password offered to a public-key document reads as a wrong password | `AuthError::UnsupportedHandler`, asserted — `WrongPassword` would send a caller looking for a better password when none exists | Yes |
