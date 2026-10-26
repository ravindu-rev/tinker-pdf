# The public-key security handler

ISO 32000-1 7.6.5's `/Adobe.PubSec` seals a document's file key to
certificates instead of to a password. A recipient unseals a CMS
`EnvelopedData` in `/Recipients` with their private key, recovers a twenty-byte
seed, and the file key is a digest over that seed and every envelope in the
array. Everything after the key — Algorithm 1's per-object salting, the four
crypt methods, the decryptor a document installs — is what the standard
handler already does. It is written as well as read: a document can be sealed
on save to certificates the caller supplies.

What needs writing down here is the **evidence** as much as the design,
because the evidence is unlike anything else in the tree.

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
- Write `/Filter /Adobe.PubSec /SubFilter /adbe.pkcs7.s5` at `/V 5`, one
  `/AESV3` crypt filter carrying the envelope: `PublicKeyEncryption::seal`
  seals a fresh seed and `/P` to the caller's certificates, and
  `DocumentEditor::save_sealed` writes a rewrite under the key 7.6.5 derives
  from them.

## Non-goals

- **Recipients with different permissions.** 7.6.5 allows one envelope per
  group of recipients, each sealing its own `/P`. The writer seals every
  recipient into one envelope under one `/P`. Envelopes per group are
  [ROADMAP](../ROADMAP.md) row SE-07.
- **Writing below `/V 5`** — `s3`, `s4`, RC4, AES-128. The writer emits
  `AESV3` and nothing else, as the standard handler's emits R6 and nothing
  else; all of them are read. Writing them, behind an explicit legacy option
  with a typed weakness warning, is [ROADMAP](../ROADMAP.md) row SE-01.
- **Sealing to a key that is not RSA.** Key agreement is not read, so it is
  not written: an EC certificate is `SealError::NotRsa`. [ROADMAP](../ROADMAP.md)
  row SE-07.
- **Sealing to an RSA key restricted to signing.** RFC 4055 §1.2 makes a key
  published under `id-RSASSA-PSS` an RSASSA-PSS key and nothing else, so it
  is `SealError::KeyRestricted`; only `rsaEncryption` is sealed to.
- **Key material of any kind.** No PKCS#8, no PKCS#12, no passphrase handling,
  no RSA private-key arithmetic. The same rule signing follows. Key import is
  [ROADMAP](../ROADMAP.md) row SG-05 and RSA signing SG-01.
- **`KeyAgreeRecipientInfo`, `KEKRecipientInfo`, `PasswordRecipientInfo`** and
  the `other` shape (§6.2.2–§6.2.5). Recognised by tag and refused by name.
  [ROADMAP](../ROADMAP.md) row SE-07.
- **Content-encryption algorithms beyond AES-128/256-CBC, RC4 and
  `des-ede3-cbc`.** `des-ede3-cbc` (`tinker_pdf_crypto::des`, FIPS 46-3
  tables, NIST CAVP known answers) is what OpenSSL's `cms -encrypt` picks by
  default for older recipients. **AES-192-CBC and RC2 are not implemented** —
  `cms -encrypt` can be asked for either, this crate's AES takes 16- and
  32-byte keys only, and neither has been a PDF default. They are named where
  they are met rather than guessed at. AES-192 and RC2 are
  [ROADMAP](../ROADMAP.md) row SE-07.
- **Writing a `des-ede3-cbc` envelope**, or any DES encryption at all.
  `tinker_pdf_crypto::des` decrypts and nothing more; its single-block encrypt
  exists only because EDE3 decryption is `D(K1, E(K2, D(K3, c)))` and the
  middle step is an encryption. A 64-bit block cipher is not something this
  engine should offer a writer.
- **Applying the enveloped permissions.** The unsealed content carries four
  bytes of `/P` and this build ignores them. PDF permissions are advisory
  (see [encryption](../features/encryption.md)), so honouring a second source
  of them would add a code path whose only effect is to disagree with the
  first. Applying an envelope's `/P` is [ROADMAP](../ROADMAP.md) row SE-07.

## Design

`crates/tinker-pdf-cos/src/pubsec.rs`, beside `security.rs`, because a security
handler is that crate's charter. The envelope is DER, which is
`tinker-pdf-pki`'s, so the handler needs the edge `cos → pki` — the ninth
amendment in `xtask/src/main.rs` argues it, and the argument is the same one
`cos → font` makes: reading a `/Recipients` envelope is object-model work that
needs a leaf's parser.

**`FileKey::from_derived`** is the other door into `tinker-pdf-crypto`'s key
type. Everything after the file key is identical between the two handlers, and
two implementations of Algorithm 1 would be two chances to get the per-object
salt wrong in a way that produces plausible garbage.

**The order of `/Recipients` is load-bearing** and the parsed form keeps the
stored bytes because of it. 7.6.5 digests each envelope in full, in file order;
a reader that re-serialised one, or sorted the array, would derive a key that
decrypts nothing, with no error anywhere to say why.

## Writing

**The API is beside `save`, not inside `WriteOptions`.** `Encryption` is a
struct with public fields that callers build by name — the FFI and the Python
binding among them — so an `Encryption::PublicKey { recipients }` variant
would break every one of them and every caller's. And `DocumentEditor::save`
returns bytes and nothing else, while sealing can fail in ways that are the
caller's to hear about: a certificate that does not parse, a key that is not
RSA, a source that will not fill. So sealing is its own step,
`PublicKeyEncryption::seal(certificates, permissions, entropy)`, and writing is
`DocumentEditor::save_sealed(options, &sealed)`. The writer is shared through a
crate-private `Sealing` that the rewriting and linearizing writers take in
place of `options.encryption`, so neither public type carries it. A sealed
save is refused when it is incremental (`SealError::NotRewrite`), because an
update appends under an `/Encrypt` that still stands, and when a password is
asked for as well (`SealError::PasswordAlsoRequested`).

**Three crates, one job each.** `tinker_pdf_crypto::rsa::encrypt_pkcs1_v15`
is RSAES-PKCS1-v1_5 (RFC 8017 §7.2.1), taking its padding from the caller,
because this tree has no randomness and because a caller-supplied `PS` is
what makes RSA Laboratories' vectors exact known answers.
`tinker_pdf_pki::seal` is the one encoder in that crate: a `ContentInfo`
around a version-0 `EnvelopedData` with a version-0 `KeyTransRecipientInfo`
for each certificate (issuer and serial, `rsaEncryption` with `NULL`), DER
`SET OF` order, and AES-256-CBC content under a 16-octet IV — field for field
what `openssl cms -encrypt -aes256` writes. `PublicKeyEncryption` in
`pubsec.rs` draws a twenty-octet seed, seals the seed followed by `/P`'s four
octets most significant first, and derives the file key with the same
`derive` the reader calls.

**Randomness is the caller's.** The seed, 48 octets the file identifier is
mixed with (as a password-encrypted write mixes its own), the content key, the
IV and each recipient's padding all come from the `EntropySource`. Every
certificate is checked first, in a dry run against a source that refuses, so a
bad certificate costs no entropy. A zero padding octet is redrawn, not
replaced, and a source that returns 64 zeros running is refused as broken.

**The crypt filter's `/Length` is 256, in bits.** ISO 32000-1 Table 25 has
the standard handler give that entry in bytes and public-key handlers give it
"as is", so the password writer's AESV3 filter says 32 and this one says 256.
The reader takes the key length from `/V` and ignores the filter's
(`decrypt.rs` calls the entry ambiguous).

**`/EncryptMetadata` is not written.** ISO 32000-1 Table 27 puts the public-key handler's
flag in the crypt filter, and this tree's reader looks for it on `/Encrypt`
itself (see Risks). Both default to true, and the writer always encrypts the
metadata, so writing it in neither place is the one choice every reader
agrees on.

### What the writing is held to

- **The envelope is RFC 5652's, and so is OpenSSL's.**
  `a_sealed_envelope_is_the_rfcs_and_so_is_openssls_outside_their_random_fields`
  (`tinker-pdf-pki/tests/enveloped.rs`) writes the expected encoding from RFC
  5652 §6 and RFC 3565, seals the content OpenSSL 3.5.5 sealed in
  `aes-256-cbc.der` to the same certificate, and holds both envelopes to it:
  the same octets outside the encrypted key, the IV and the ciphertext.
  OpenSSL's bytes are an input checked against the clause, not the expected
  answer (ruling 13), and the writer is held to the RFCs' encoding rather
  than to this tree's own reader.
- **RSAES-PKCS1-v1_5 is RSA Laboratories'.** All the encryption known answers
  in `pkcs1v15crypt-vectors.txt`, across keys from 1024 to 2048 bits,
  including the 1025- to 1031-bit moduli whose leading octet is not full.
- **The file opens.** `crates/tinker-pdf/tests/pubsec_write.rs`: a sealed
  `simple-text.pdf` reopened with a `Recipient` that holds the committed
  `visible-signer-key.der` and does RFC 8017 §7.2.2 itself, text and `/P`
  intact; the dictionary 7.6.5 asks for; the envelope opened by hand to
  twenty octets and then the permissions, big-endian; a stranger told
  `NoMatchingRecipient` and a password `UnsupportedHandler`; two recipients
  in one envelope; object streams and the linearized layout; the same
  entropy giving the same bytes (ruling 4); the `/ID` mixed with the
  entropy; each refusal by name; a `Debug` that prints no key.

## The evidence, and what it is not

No fixture is a public-key-encrypted document another PDF implementation
made, so this handler's verification cannot be what the rest of this crate's
is. The layers are not equally weak and the difference matters:

| Layer | Evidence | Strength |
| --- | --- | --- |
| `EnvelopedData` parsing | Three envelopes **OpenSSL 3.5.5 produced**, in three content ciphers, with the recipient identifier checked against the certificate's own issuer and serial | Real interop: the parser reads what another implementation writes |
| RSA unsealing | Not implemented here at all — it is the host's | N/A |
| Content decryption | AES-CBC and RC4, gated on FIPS 197 and RFC 6229; `des-ede3-cbc`, gated on NIST CAVP known answers, its tables held to FIPS 46-3's published digit stream | Published vectors |
| **7.6.5 key derivation** | A second implementation in Python, written from the same clause by the same author, generating the end-to-end fixture | **Catches a transcription slip; cannot catch a misreading** |
| The document decrypting | `pubsec.rs`'s tests read "Public key" back out of the page and `PubSec fixture` out of `/Info` | Ties every layer together, on one file |
| `EnvelopedData` writing | Byte-equal to OpenSSL 3.5.5's outside the random fields, both held to the RFCs' encoding | Real interop |
| RSAES-PKCS1-v1_5 encryption | RSA Laboratories' known answers | Published vectors |

**What `des-ede3-cbc.der` proves, and what it does not.** That fixture is one
of the three envelopes OpenSSL 3.5.5 wrote, 475 bytes. Walking it:
`id-envelopedData` wrapping version 0, one `KeyTransRecipientInfo` naming the
recipient by issuer and a 20-byte serial, `rsaEncryption` over a 256-byte
encrypted key, and an `EncryptedContentInfo` over `id-data` whose algorithm is
`1.2.840.113549.3.7` with an 8-byte `OCTET STRING` parameter and 32 bytes —
four whole blocks — of content. So it proves three things and they are all
about the *envelope*: the OID this module matches on is the one OpenSSL emits
byte for byte rather than one transcribed from a table; the IV really is eight
bytes in a real envelope, where the AES branch beside it reads sixteen; and
the branch runs a cipher rather than returning `UnsupportedContentCipher`.

It proves **nothing whatever about decryption**. The throwaway private key is
not in the tree, so the 256-byte encrypted key cannot be unsealed, there is no
recoverable content-encryption key, and no plaintext exists to compare against
— `the_openssl_triple_des_envelope_reaches_the_cipher` hands the path an
arbitrary 24-byte key and gets 32 bytes of garbage, which is all it claims. The
32 bytes are not even known to *be* Triple DES output; any 32 bytes would parse
the same. Parsing an algorithm identifier is not decrypting with it, and every
question about whether the S-boxes, the key schedule, the sub-key order, the
chain or the padding are right is settled by the CAVP known answers in
`tinker_pdf_crypto::des` and by nothing in `pubsec.rs`.

This is the same class of gap `verification.md` names for the whole suite, in
its sharpest form: reader, writer, fixture and reviewer share one author's
reading of one clause, and if that reading is wrong everything agrees and
everything is wrong. **Not closed**, and
[features/encryption.md](../features/encryption.md) says so where a caller
will see it rather than only here.

### What the Triple DES content cipher is held to

`tinker_pdf_crypto::des` is held to NIST CAVP's known answers
(`tinker-pdf-crypto/tests/data/cavp/tdes_cbc_*.rsp`) and to FIPS 46-3 itself.
The variable-plaintext file walks a 1 bit through all 64 plaintext positions
and the variable-key file through all 56 non-parity key positions, so every
S-box is driven from every direction — a handful of random vectors would not
do it, because an S-box entry is one of 512 and a random block reaches few of
them. FIPS 46-3 works one S1 lookup out in prose, and
`the_standards_own_worked_example_for_s1` pins it;
`every_table_matches_the_published_digit_stream` keeps the tables' reading of
the standard runnable.

All five KAT files are keyed `K1 = K2 = K3` and are single-block under a zero
IV, so between them they cannot see the order of EDE3's sub-keys *or* the
chain. `TCBCMMT3` is the only committed file whose three keys differ, so one
published multi-key vector set is the whole of the evidence for sub-key order.
`TripleDes::new` takes all three of X9.52's keying options, and option 2 — 16
bytes, `K3 = K1` — is adjudicated by `TCBCMMT2`: it prints `K1`, `K2` and
`K3 = K1` in full, so every vector runs twice — once as the 24 bytes it spells
out and once as `K1 || K2` — and the short form is held to NIST's own
plaintext rather than to the long form, which would only be this engine
agreeing with itself about an abbreviation.

## Risks

| Risk | Mitigation | Closed? |
| --- | --- | --- |
| An envelope OpenSSL wrote by default cannot be opened at all, because its content cipher is `des-ede3-cbc` | `tinker_pdf_crypto::des` decrypts EDE3-CBC, held to NIST CAVP known answers and to FIPS 46-3's digit stream (`every_table_matches_the_published_digit_stream`). The two multi-block files carry the weight the five KATs cannot: under the KATs' `K1 = K2 = K3` the sub-key order and the CBC chain are both invisible | **Yes** |
| The key derivation is misread and nothing catches it | Stated in the module doc, the feature doc and here; the derivation is small and each step is asserted separately in unit tests | **No** |
| A wrong key reads as a decrypted document | 7.6.5 has no verifier the way `/U` is one, so this cannot be caught at authentication; a test pins that the failure stays visible as garbage rather than becoming plausible text | Partly |
| An attacker-chosen envelope drives the parser | The parser is `tinker-pdf-pki`'s, under ruling 1 with its own fuzz targets; `every_single_byte_mutation_is_an_error_or_a_parse_and_never_a_panic` sweeps every single-byte mutation of a real envelope | Yes |
| A password offered to a public-key document reads as a wrong password | `AuthError::UnsupportedHandler`, asserted — `WrongPassword` would send a caller looking for a better password when none exists | Yes |
| A `/V 4` or `/V 5` document whose crypt filter says `/EncryptMetadata false` opens with the wrong key | Table 27 puts the public-key handler's flag in the crypt filter, and `decrypt.rs` reads it from `/Encrypt` only, so the key is derived as if the metadata were encrypted. The writer avoids it by writing the flag nowhere; a third-party file that writes it there fails to open here | **No** |
| A sealed document is only as unpredictable as the host's entropy | The source is the caller's by design, because the engine links no random number generator; a source that cannot fill, or returns zeros, is refused; a predictable one is not detectable and is documented as the caller's | Partly |
