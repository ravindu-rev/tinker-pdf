# Phase 03 — Encryption

When this phase is done, tinker-pdf opens every standard-handler encrypted PDF in the wild —
RC4-40 through AES-256 — reports *which* password authenticated (user or owner), and exposes
the permission bits exactly as stored, raw `i32` included. All cryptographic primitives are
hand-rolled per the project's no-third-party-primitives rule ([PLAN.md](../PLAN.md)), live in
the `tinker-pdf-crypto` leaf crate (bytes in, values out, zero PDF types), and merge only
behind published test vectors. The shape follows from two MuPDF failures Tinker carries
workarounds for today: permissions parsed through a bitflags type that rejects every real
`/P` value, and an authentication API that collapses the user/owner distinction to a `bool`.
This phase is built so neither bug is expressible.

## Scope

- Primitives in `tinker-pdf-crypto`, each gated on published vectors before merge:
  - MD5 (RFC 1321, its reference vectors).
  - RC4 (RFC 6229 key-stream vectors).
  - SHA-256/384/512 (NIST CAVP short/long-message and Monte Carlo suites).
  - AES-128 and AES-256, CBC mode plus the raw single-block operation (CAVP KAT/MMT/MCT
    suites). Both directions from day one — Algorithm 2.B *encrypts* even on the read path.
  - Constant-time byte comparison for every password/hash check.
- Standard security handler (7.6.3), all wild revisions:
  - R2 (V1, RC4-40) and R3 (V2, RC4 40–128): Algorithm 2 key derivation, Algorithms 4/5
    `/U` computation, Algorithms 6/7 user/owner authentication.
  - R4 (V4): crypt filters (7.6.5) — `/CF` map, `/StmF`, `/StrF`, `/EFF`, `/Identity`,
    `/V2` (RC4) and `/AESV2` methods, `/EncryptMetadata false` (the extra `0xFFFFFFFF` in
    the Algorithm 2 hash, and metadata streams left in the clear), per-stream `/Crypt`
    filter with `/DecodeParms /Name` (7.4.10).
  - R6 (V5, AES-256): Algorithm 2.A password verification and file-key retrieval via
    `/UE`/`/OE`, Algorithm 2.B hardened hash including its SHA-256/384/512 modulo-3
    selection and AES-128-CBC-no-padding rounds, `/Perms` decryption and validation.
  - R5: accepted read-only with a warning (deprecated Adobe draft; see Design).
- Object-key salting for R≤4 (Algorithm 1): low 3 bytes of object number + low 2 bytes of
  generation, plus `sAlT` (`0x73416C54`) for AESV2; MD5; first `min(n+5, 16)` bytes.
- String vs stream key routing (`/StrF` and `/StmF` may name different filters), and the
  never-encrypted set: the `/Encrypt` dictionary's own strings, the trailer `/ID`,
  cross-reference streams (7.5.8.2), and strings inside object streams (7.5.7 — the
  container was encrypted, its contents must not be decrypted twice).

  *Amended, August 2026 (gap 19).* The write side has two more members, both only
  reachable on the linearized path and one of them outside 7.6.1. **Classic**
  cross-reference tables, alongside the streams: a linearized file has two tables and a
  reader finds both before it knows there is an `/Encrypt` dictionary. And the
  **linearization parameter dictionary**, which 7.6.1 does not exempt — a reader consults
  it before authenticating, and it is sound only because part 2 carries no strings, which
  `linearize.rs` asserts rather than assumes. Nothing on the read side changes: a
  decryptor is never handed any of these, and never was.
- Authentication level surfaced as `AuthLevel::{None, User, Owner}` from the Algorithm
  2.A / Algorithm 7 checks — kills MuPDF limitation #3 (owner/user collapsed to `bool`).
- Permissions as raw `i32` plus typed accessors that read only spec-defined bits — kills
  MuPDF limitation #2 (reserved `/P` bits are 1 by spec, so `from_bits`-style parsing fails
  on every real document).
- `EntropySource` trait seam for the future writer. Decryption needs no randomness; the
  engine never links an RNG.

R6 formally belongs to ISO 32000-2, but it shipped years earlier as Adobe Extension Level 8
and dominates encrypted files in the wild, so it is in scope here despite the 1.7 baseline.
The remaining 2.0 encryption deltas (unencrypted wrapper documents, `/KDF`) stay on the 2.0
delta list in [PLAN.md](../PLAN.md).

## Non-goals

- **Public-key (PKCS#7) security handler** (7.6.4, `/Adobe.PubSec`). Rare outside
  enterprise DRM and it drags in certificate parsing. Tracked as a capability flag; opening
  such a file reports "unsupported security handler: Adobe.PubSec" cleanly, never a parse
  error. Same treatment for third-party handlers (FileOpen et al.), which are permanently
  out of scope.
- **Writing encrypted output** — key generation, `/O`/`/U`/`/OE`/`/UE`/`/Perms` synthesis,
  `/Encrypt` emission. That is writer work and lands with the serializers in
  [01-cos](01-cos-and-object-model.md). This phase only guarantees the primitives are bidirectional and the
  `EntropySource` seam exists so the writer needs nothing new from crypto.
- **Full SASLprep normalization of R6 passwords.** See Design; documented leniency, not an
  accident.
- **Side-channel hardening beyond constant-time comparisons.** The threat model is a
  document being decrypted on its owner's machine, not a network decryption oracle. Stated
  in `SECURITY.md`, revisited only if the FFI consumer changes the model.

## Design

### Crate boundary

`tinker-pdf-crypto` never sees a PDF object. It exposes primitives plus the standard-handler
math over a plain parameter struct; `tinker-pdf-cos` extracts that struct from the
`/Encrypt` dictionary and trailer, and owns all wiring. This keeps the crate independently
fuzzable and keeps the vector suites honest — nothing in the test path can accidentally
depend on parsing.

```rust
// tinker-pdf-crypto — bytes in, values out.

pub struct StdSecurityParams<'a> {
    pub revision: u8,            // 2..=6; 5 accepted read-only
    pub key_bits: u16,           // /Length; defaulted to 40 when absent
    pub o: &'a [u8],             // 32 bytes (R<=4) or 48 (R6); padded/truncated leniently
    pub u: &'a [u8],
    pub oe: Option<&'a [u8]>,    // R5/R6 only
    pub ue: Option<&'a [u8]>,
    pub perms: Option<&'a [u8]>, // R6 /Perms, 16 bytes
    pub p: i32,                  // raw, sign-extended; never interpreted here
    pub encrypt_metadata: bool,
    pub file_id0: &'a [u8],      // first element of trailer /ID
}

pub enum AuthResult {
    Owner { key: FileKey },
    User { key: FileKey },
    WrongPassword,
}

/// Tries `password` as owner first, then as user (Algorithm 2.A for R6;
/// Algorithms 7 then 6 for R<=4). Callers try `b""` first — an empty user
/// password that authenticates means "open silently, still encrypted".
pub fn authenticate(params: &StdSecurityParams<'_>, password: &[u8]) -> AuthResult;

pub enum CryptMethod {
    Identity,
    Rc4 { key_bits: u16 }, // object-key salting applied inside
    AesV2,                 // salted key + "sAlT", CBC, leading IV
    AesV3,                 // file key used directly, CBC, leading IV
}

pub fn decrypt_in_place(
    key: &FileKey,
    method: CryptMethod,
    obj: u32,
    gen: u16,
    data: &mut Vec<u8>,
) -> Result<(), CryptoError>;
```

Owner-first matters: for R≤4 an owner password that happens to equal the user password must
still report `Owner`, because owner authentication is the stronger claim and Algorithm 7
subsumes Algorithm 6. All hash and `/U`/`/O` comparisons go through the constant-time
compare. That is nearly free and removes a class of bug reports, even though the local-file
threat model makes the timing channel mostly academic.

### Wiring in cos

`tinker-pdf-cos` builds a `Decryptor` at open time: locate `/Encrypt` from the trailer,
extract `StdSecurityParams`, run the empty-password attempt, and hold the file key plus the
resolved `/StmF`/`/StrF`/`/EFF` methods. Strings are decrypted at object-load time — the
loader is the only code that knows the owning object number — except for objects loaded out
of object streams, which skip string decryption entirely (7.5.7). Streams decrypt lazily
when bytes are first requested, and the pipeline order is fixed: **decrypt, then defilter**
([02-filters](02-filters.md) never sees ciphertext). A per-stream `/Crypt` entry in
`/Filter` overrides the default stream method at its position in the filter chain, which in
practice is always first.

Error taxonomy in cos: `NeedsPassword` (the empty-password attempt failed and no password
was supplied) vs `WrongPassword` (a supplied password was rejected). The facade phase maps
these to the codes Tinker's `open_documents.rs` asserts (`DOC_PASSWORD_REQUIRED`,
`DOC_PASSWORD_WRONG`); the distinction has to exist here or it cannot exist there.

### Key derivation, per revision

**R≤4 (Algorithm 2).** Pad/truncate the password to 32 bytes with the spec pad string
(7.6.3.3); MD5 over pad ‖ `/O` ‖ `/P` as 4 little-endian bytes ‖ `ID[0]`, appending
`FF FF FF FF` when R4 and `/EncryptMetadata false`; for R≥3, 50 further MD5 rounds over the
first `n` bytes; key is the first `n = key_bits/8` bytes (5 for R2). Per-object keys via
Algorithm 1 salting as scoped above. Passwords here are byte strings in PDFDocEncoding; we
take the caller's bytes as given and additionally retry a Latin-1 transcoding on failure
(see leniency).

**R6 (Algorithms 2.A/2.B).** `/U` and `/O` are 48 bytes: 32-byte hash, 8-byte validation
salt, 8-byte key salt. Password is UTF-8, truncated to 127 bytes. Owner check: hash of
password ‖ O-validation-salt ‖ `U[0..48]` equals `O[0..32]`; user check: password ‖
U-validation-salt against `U[0..32]`. The matching key salt hashes to an intermediate key
that AES-256-CBC-decrypts `/OE` or `/UE` (zero IV, no padding) into the 32-byte file key.
The hash is Algorithm 2.B: start with SHA-256, then rounds of — concatenate 64 repetitions
of password ‖ K (‖ `U` for owner checks); AES-128-CBC-encrypt with key `K[0..16]`, IV
`K[16..32]`, no padding; read the first 16 bytes of the ciphertext as a big-endian integer
mod 3 to pick SHA-256/384/512; K becomes that hash of the ciphertext. At least 64 rounds,
terminating once the last ciphertext byte is at most `round − 32`. This is the one
algorithm in the phase with genuinely poor public test coverage, so it gets fixtures from
multiple producers plus oracle cross-checks (see Risks).

`/Perms` is decrypted with the file key as one raw AES-256 block (ECB, no IV): bytes 0–3
must echo `/P` (little-endian), byte 8 is `T`/`F` for `/EncryptMetadata`, bytes 9–11 are
`adb`. A mismatch is surfaced as a tamper warning, not a hard failure — we are a reader,
and refusing to open a file we can decrypt helps nobody; the warning is the honest output.

**R5** is the deprecated Adobe draft that R6 hardened: plain SHA-256 without the 2.B
rounds, which is why it was withdrawn (trivially brute-forceable at GPU speeds). We read it
— files exist — with a document-level warning, and the writer will never produce it.

**AES payloads** (V2 and V3): first 16 bytes are the IV, remainder is PKCS#5-padded
ciphertext. Empty ciphertext decrypts to empty. Invalid padding is repaired by the leniency
policy below rather than rejected.

### Permissions and authentication level

```rust
/// Wraps /P exactly as stored. Reserved bits are 1 by spec, so any parser
/// that validates the full bit pattern rejects every real document — the
/// accessors therefore read only their own defined bit and nothing else.
pub struct Permissions(i32);

impl Permissions {
    pub fn raw(&self) -> i32;
    pub fn print(&self) -> bool;              // bit 3
    pub fn modify(&self) -> bool;             // bit 4
    pub fn copy(&self) -> bool;               // bit 5
    pub fn annotate(&self) -> bool;           // bit 6
    pub fn fill_forms(&self) -> bool;         // bit 9
    pub fn extract_accessible(&self) -> bool; // bit 10
    pub fn assemble(&self) -> bool;           // bit 11
    pub fn print_hires(&self) -> bool;        // bit 12
}
```

(Bit numbers are the spec's 1-based positions, Table 22.) `Permissions` reports the bits
and only the bits; it does not encode policy. `AuthLevel::Owner` meaning "restrictions do
not apply to you" is the caller's decision, made where Tinker makes it today. This is the
direct fix for the two workarounds Tinker documents as MuPDF limitations #2 and #3:
`permissions-noprint.pdf` (`/P -2056`) must report `print() == false`,
`copy() == true`, and the same file opened with the owner password must say so.

### Leniency policy

Real `/Encrypt` dictionaries are sloppy. The reader's rules, in priority order:

- Trust `/R`; sanity-check `/V` and warn on mismatched pairs rather than failing.
- `/Length` absent → 40. `/Length` in bytes instead of bits (a known producer bug: values
  ≤ 40 that are multiples of 8 are ambiguous — treat 16/24/32 as bytes) → normalize, warn.
- `/O`/`/U` shorter than expected → zero-pad; longer → truncate. Warn either way.
- `/P` written as an unsigned integer → read as `i64`, wrap to `i32`.
- Wrong password with a non-ASCII R≤4 password → retry the bytes transcoded Latin-1 →
  PDFDocEncoding before reporting `WrongPassword` (matches Acrobat's observed behavior).
- R6 SASLprep (RFC 4013) is skipped: passwords are UTF-8-encoded and truncated to 127
  bytes, full stop. No major reader implements SASLprep in practice, so implementing it
  would *reject* passwords that every other viewer accepts. Documented; revisited only on
  corpus evidence of a file that needs it.
- Bad PKCS#5 padding → strip by last-byte count when plausible, else keep all bytes; warn.
  A hard failure here would lose an otherwise readable page to one corrupt stream.

Every leniency emits a structured warning through cos's diagnostics channel — silent repair
is how oracles drift apart.

### Entropy seam

```rust
/// The engine never links an RNG. Hosts inject one; only the writer consumes it.
pub trait EntropySource {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), EntropyError>;
}
```

OS shims live with the hosts, not the engine: `BCryptGenRandom` in the Windows FFI host,
the `getrandom` syscall on Linux, `crypto.getRandomValues` bridged in from JS on
wasm32-unknown-unknown. This is what keeps the wasm target first-class — there is no
ambient entropy on `wasm32-unknown-unknown`, so an engine that reached for an OS RNG would
either fail to build or grow a wasm special case. Decryption needs no randomness, so
nothing in this phase blocks on a host providing one.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Primitive suite in `tinker-pdf-crypto`: MD5, RC4, SHA-256/384/512, AES-128/256 (CBC + raw block, both directions), constant-time compare | RFC 1321, RFC 6229, and NIST CAVP vector suites green and wired as CI merge gates; encrypt/decrypt roundtrip property tests (proptest) pass; crate builds and tests on wasm32-unknown-unknown | S |
| 2 | R2–R4 handler complete: Algorithms 1–7, crypt filters (`/CF`, `/StmF`, `/StrF`, `/EFF`, `/Identity`, `/V2`, `/AESV2`), `/EncryptMetadata false`, per-stream `/Crypt`; cos wiring with decrypt-before-defilter and the never-encrypted set | Fixtures `rc4-40`, `rc4-128`, `aesv2`, `aesv2-cleartext-metadata`, `stringcrypt-differs` decrypt to plaintext byte-identical against oracle-diff (mutool); user vs owner password reports the correct `AuthLevel` on each; strings inside object streams round-trip undamaged | S |
| 3 | R6 (Algorithms 2.A/2.B, `/UE`/`/OE`, `/Perms`) plus R5 read-only with warning | `encrypted-aes256.pdf` opens: `open-sesame` → `AuthLevel::User`, `owner-secret` → `AuthLevel::Owner`, wrong password → `WrongPassword`, no password → `NeedsPassword`; `/Perms` validates and a deliberately tampered copy raises the tamper warning; R5 fixture opens with its deprecation warning | S |
| 4 | `Permissions` + `AuthLevel` surfaced through cos; error taxonomy; leniency corpus; fuzz targets (`authenticate`, `decrypt_in_place`, `/Encrypt`-dict extraction); `SECURITY.md` | `permissions-noprint.pdf` (`/P -2056`) reports `print() == false`, `copy() == true` under user auth; leniency corpus (bad `/Length`, short `/U`, unsigned `/P`, mismatched V/R) opens with warnings; fuzzers run clean in CI; `SECURITY.md` states scope, threat model, and the review invitation | S |

Total ≈ M, matching the phase band.

## Dependencies

- **Needs:** the [01-cos](01-cos-and-object-model.md) object model, trailer, and xref to exist for wiring
  (milestones 2+); [02-filters](02-filters.md) only for pipeline ordering. Milestone 1 has
  no dependencies at all — the primitive suite can start the day the workspace compiles,
  and should, because everything else in the phase stacks on it.
- **Unblocks:** the encryption and permissions halves of Checkpoint A — Tinker's
  `encrypted_documents_ask_for_a_password_before_anything_else` and
  `permission_flags_are_reported_under_user_authentication` in
  `crates/tinker-core/tests/open_documents.rs` are the parity bar, and neither needs a
  rasterizer. Also unblocks the writer's encrypted-output work in [01-cos](01-cos-and-object-model.md)
  (primitives + `EntropySource` are the full crypto surface it needs) and any later
  embedded-file extraction from encrypted documents.

## Risks

| Risk | Mitigation |
| --- | --- |
| Hand-rolled crypto is wrong in a way vectors miss | Scope is document decryption only — no network surface, no key exchange. Still: published vectors as merge gates, roundtrip property tests, fuzzed decrypt paths in CI, and `SECURITY.md` explicitly invites external review of `tinker-pdf-crypto` as the one crate where it pays most |
| Algorithm 2.B has thin public test coverage; an off-by-one in round termination or the mod-3 read authenticates nothing | Fixtures generated by multiple independent producers (Acrobat, qpdf, mutool); oracle-diff cross-checks every R6 fixture; once the writer exists, write-then-read roundtrips become a standing property test |
| Real-world `/Encrypt` garbage (bytes-vs-bits `/Length`, short `/U`, unsigned `/P`, V/R mismatch) breaks files other readers open | The leniency policy above, each rule carried by a corpus fixture, with behavior pinned against mutool/pdfium via oracle-diff so repairs never drift silently |
| Skipping SASLprep rejects some non-ASCII R6 password | Deliberate, documented, and shared with every major reader; a corpus counterexample reopens the decision — the fix is additive |
| Timing side channels beyond password comparison (table-based AES) | Constant-time comparison everywhere a secret is checked; the residual channel is documented honestly in `SECURITY.md` under the local-file threat model rather than half-fixed |
| Tamper-warning policy on `/Perms` mismatch lets a modified file open quietly if hosts ignore warnings | Warnings are structured diagnostics, not log lines; the facade surfaces them on `DocumentInfo`, and the Checkpoint A test list includes the tampered-`/Perms` fixture so the path stays exercised |

---

Sibling context: [01-cos](01-cos-and-object-model.md) owns the `/Encrypt` extraction and the future encrypted
writer; [02-filters](02-filters.md) receives only plaintext. Master plan and checkpoint
definitions: [PLAN.md](../PLAN.md).
