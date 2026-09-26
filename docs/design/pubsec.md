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
- **Content-encryption algorithms beyond AES-128/256-CBC, RC4 and
  `des-ede3-cbc`.** Triple DES used to be listed here as the one real gap; it
  is implemented now (`tinker_pdf_crypto::des`, FIPS 46-3 tables, 500 NIST
  CAVP known answers), which closes the case that mattered, because
  `des-ede3-cbc` is what OpenSSL's `cms -encrypt` still picks by default for
  older recipients. **AES-192-CBC and RC2 are not implemented** — `cms
  -encrypt` can be asked for either, this crate's AES takes 16- and 32-byte
  keys only, and neither has ever been a PDF default. They are named where
  they are met rather than guessed at, which is the same answer Triple DES
  used to get.
- **Writing a `des-ede3-cbc` envelope**, or any DES encryption at all. The new
  module decrypts and nothing more; its single-block encrypt exists only
  because EDE3 decryption is `D(K1, E(K2, D(K3, c)))` and the middle step is
  an encryption. A 64-bit block cipher is not something this engine should
  offer a writer.
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

**No corpus file uses this handler.** Zero of 5 605, re-measured on 15
September 2026 at the commit that landed Triple DES: `/Adobe.PubSec` and
`/Recipients` are each zero across every PDF under `corpus/files`. Ten files
match `pubsec` case-insensitively; every one of the ten carries `/Prop_Build`
and none carries `/Adobe.PubSec`, because the hit is `/PubSec` inside a
signature's build-properties dictionary — a different key entirely. That is
worth writing down because a case-insensitive grep is the obvious way to
re-take this measurement and it reads as ten hits. Ruling 3 schedules
capabilities by corpus hit-rate, so this one was built because it was asked
for and not because the evidence called for it — and the consequence is that
its verification cannot be what the rest of this crate's is.

The layers are not equally weak and the difference matters:

| Layer | Evidence | Strength |
| --- | --- | --- |
| `EnvelopedData` parsing | Three envelopes **OpenSSL 3.5.5 produced**, in three content ciphers, with the recipient identifier checked against the certificate's own issuer and serial | Real interop: the parser reads what another implementation writes |
| RSA unsealing | Not implemented here at all — it is the host's | N/A |
| Content decryption | AES-CBC and RC4, gated on FIPS 197 and RFC 6229; `des-ede3-cbc`, gated on 500 NIST CAVP known answers, its tables read twice from FIPS 46-3 | Published vectors |
| **7.6.5 key derivation** | A second implementation in Python, written from the same clause by the same author, generating the end-to-end fixture | **Catches a transcription slip; cannot catch a misreading** |
| The document decrypting | `pubsec.rs`'s tests read "Public key" back out of the page and `PubSec fixture` out of `/Info` | Ties every layer together, on one file |

**What `des-ede3-cbc.der` proves, and what it does not.** That fixture is one
of the three OpenSSL 3.5.5 wrote, 475 bytes, and it was committed with the
parser long before there was a cipher to use it. Walking it: `id-envelopedData`
wrapping version 0, one `KeyTransRecipientInfo` naming the recipient by issuer
and a 20-byte serial, `rsaEncryption` over a 256-byte encrypted key, and an
`EncryptedContentInfo` over `id-data` whose algorithm is `1.2.840.113549.3.7`
with an 8-byte `OCTET STRING` parameter and 32 bytes — four whole blocks — of
content. So it proves three things and they are all about the *envelope*: the
OID this module matches on is the one OpenSSL emits byte for byte rather than
one transcribed from a table; the IV really is eight bytes in a real envelope,
where the AES branch beside it reads sixteen and a copied line would have kept
sixteen; and the branch now runs a cipher instead of returning
`UnsupportedContentCipher`.

It proves **nothing whatever about decryption**. The throwaway private key was
not kept, so the 256-byte encrypted key cannot be unsealed, there is no
recoverable content-encryption key, and no plaintext exists to compare against
— `the_openssl_triple_des_envelope_reaches_the_cipher` hands the path an
arbitrary 24-byte key and gets 32 bytes of garbage, which is all it claims. The
32 bytes are not even known to *be* Triple DES output; any 32 bytes would parse
the same. Parsing an algorithm identifier is not decrypting with it, and every
question about whether the S-boxes, the key schedule, the sub-key order, the
chain or the padding are right is settled by the 500 CAVP known answers in
`tinker_pdf_crypto::des` and by nothing in `pubsec.rs`.

There was no way to do better. No tool on the machine this was written on
produces a public-key-encrypted PDF — **qpdf 12.3.2, which is installed, has no
public-key support at all** — and the corpus has none to borrow. This is the
same class of gap `verification.md` names for the whole suite, in its sharpest
form: reader, writer, fixture and reviewer share one author's reading of one
clause, and if that reading is wrong everything agrees and everything is wrong.

**Not closed.** It closes the day a real public-key-encrypted document arrives,
and until then `docs/features/encryption.md` says so where a caller will see
it rather than only here.

### Counted injections: the Triple DES content cipher

Each defect re-introduced in turn, `cargo test --no-fail-fast -p
tinker-pdf-crypto` run on each, and the count recorded — zeros included,
because a guard that catches nothing when its defect is injected is not a
guard. Measured 15 September 2026 on the branch that landed the cipher.

The eight S-boxes are injected identically so the counts can be compared:
row 0's first two entries transposed, which leaves the row a permutation of
0..16 and so is invisible to `the_tables_are_well_formed`.

| Injected | Tests that failed |
| --- | ---: |
| `IP^-1`'s first entry, 40 → 41 | 12 |
| `IP^-1` omitted entirely | 10 |
| `IP` omitted entirely | 10 |
| the final interchange omitted — preoutput read as `L16R16` | 10 |
| **S1** row 0, columns 0 and 1 transposed | 9 |
| **S2** the same | 8 |
| **S3** the same | 8 |
| **S4** the same | 8 |
| **S5** the same | 8 |
| **S6** the same | 8 |
| **S7** the same | 8 |
| **S8** the same | 8 |
| `select()` reading the row as the top two bits and the column as the bottom four | 8 |
| `E`'s last entry, 1 → 2 | 8 |
| `PC-1`'s first entry, 57 → 58 | 7 |
| the shift schedule: round 9 rotating by 2 rather than 1 | 7 |
| the round keys consumed in the same order both ways | 7 |
| `PC-2`'s first entry, 14 → 15 | 6 |
| EDE3 performed as `D(K1, D(K2, D(K3, c)))` | 6 |
| the CBC chain carrying the plaintext forward rather than the ciphertext | 2 |
| the three sub-keys taken in the wrong order, `D(K3, E(K2, D(K1, c)))` | 1 |
| PKCS#7 padding stripped without checking the pad bytes agree | 1 |
| **the 16-byte key bundle expanding `K3 = K2` rather than `K3 = K1`** | **0, then 1** |

**The zero is the row worth reading.** `TripleDes::new` takes all three of
X9.52's keying options, and option 2 — 16 bytes, `K3 = K1` — was checked by
nothing. `only_the_three_keying_options_are_accepted` asserts that 16 bytes
are *accepted* and never asks what they expand to, so taking `K3 = K2` instead,
one character from the real line, broke no test in the crate. The fix is the
seventh committed vector file, CAVP's `TCBCMMT2`: it prints `K1`, `K2` and
`K3 = K1` in full, so every vector can be run twice — once as the 24 bytes it
spells out and once as `K1 || K2` — and the short form is then adjudicated by
NIST's own plaintext rather than by the long form, which would only be this
engine agreeing with itself about an abbreviation it invented. It also raised
the CBC-chain row from 1 to 2, being the second multi-block file.

**The two rows at 1 are structural, not an oversight.** All five KAT files are
keyed `K1 = K2 = K3` and are single-block under a zero IV, so between them they
cannot see the order of EDE3's sub-keys *or* the chain; `TCBCMMT3` is the only
committed file whose three keys differ, and under `TCBCMMT2`'s `K3 = K1`
transposing `K1` and `K3` is a no-op. One published multi-key vector set is the
whole of the evidence for sub-key order, and the honest thing is to say so
rather than to add a fixture this engine generated itself, which would prove
nothing about the standard.

**The eight S-boxes are covered evenly**, which is the question the separate
rows were there to answer: seven tests catch each, plus the digit-stream
reading, and S1 draws a ninth only because FIPS 46-3 works one S1 lookup out in
prose and `the_standards_own_worked_example_for_s1` pins it. No box is
under-covered, and that is a property of the KATs rather than luck — the
variable-plaintext file walks a 1 bit through all 64 plaintext positions and
the variable-key file through all 56 non-parity key positions, so every S-box
is driven from every direction. A handful of random vectors would not do it: an
S-box entry is one of 512, and a random block reaches few of them.

**This is not a hypothetical.** The campaign was re-run on a working tree that
still held a live defect from an interrupted session — `S4` row 0 ending
`4, 14`, so 14 twice and no 15 — and nine tests caught it, including both table
readings. It is exactly the shape the whole apparatus exists for: one wrong
entry, wrong for only some inputs, which a spot check passes.

## Risks

| Risk | Mitigation | Closed? |
| --- | --- | --- |
| An envelope OpenSSL wrote by default cannot be opened at all, because its content cipher is `des-ede3-cbc` | Implemented, 15 September 2026. `tinker_pdf_crypto::des` decrypts EDE3-CBC; the FIPS 46-3 tables were read twice by two routes that fail differently, and `every_table_matches_the_published_digit_stream` keeps the first reading runnable. 500 NIST CAVP known answers adjudicate. The two multi-block files carry the weight the five KATs cannot: under the KATs' `K1 = K2 = K3` the sub-key order and the CBC chain are both invisible, and a counted-injection campaign measured that exactly — transposing `K1` and `K3` fails one test, `TCBCMMT3`, and nothing else | **Yes** |
| The key derivation is misread and nothing catches it | Stated in the module doc, the feature doc and here; the derivation is small and each step is asserted separately in unit tests | **No** |
| A wrong key reads as a decrypted document | 7.6.5 has no verifier the way `/U` is one, so this cannot be caught at authentication; a test pins that the failure stays visible as garbage rather than becoming plausible text | Partly |
| An attacker-chosen envelope drives the parser | The parser is `tinker-pdf-pki`'s, under ruling 1 with its own fuzz targets; the tests sweep every single-byte mutation of a real envelope | Yes |
| A password offered to a public-key document reads as a wrong password | `AuthError::UnsupportedHandler`, asserted — `WrongPassword` would send a caller looking for a better password when none exists | Yes |
