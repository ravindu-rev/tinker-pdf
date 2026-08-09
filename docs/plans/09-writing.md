# Phase 09 — Writing

When this phase is done, `tinker-pdf-cos` serializes as well as it parses: a full rewrite
path (garbage collection, renumbering, either cross-reference flavor, recompression,
linearization) and an incremental-update path whose output is the original file byte-for-byte
plus an appended section — the foundation Tinker's signing plan
(`docs/plans/10-security-and-signatures.md` in the Tinker repo) calls "the hardest piece" and
builds everything on. Encrypt-on-save lands here too, closing the loop
[03-encryption](03-encryption.md) left open by design. The serializers live in the same crate
as the object model because [01-cos-and-object-model](01-cos-and-object-model.md) shaped its
structures for this moment: insertion-ordered dictionaries, literal-vs-hex string provenance,
and per-revision bookkeeping all exist so that writing can round-trip what reading saw.
Linearization is the phase's superiority item — MuPDF 1.26 removed linearized output
entirely, which forced Tinker to plan a qpdf post-pass; tinker-pdf writes it natively.

## Scope

- Token and object serializers in `tinker-pdf-cos` for every object form of 7.3: strings
  re-emitted in their recorded literal/hex form (7.3.4), names with `#xx` escapes where
  bytes demand them (7.3.5), reals in shortest round-trip decimal (exponent forms are
  illegal on output even though the lexer tolerates them on input), streams with `/Length`
  always a direct integer, binary-marker header comment per 7.5.2.
- Full rewrite: garbage collection by generic mark-from-trailer, compact renumbering,
  classic cross-reference tables (7.5.4) or cross-reference streams (7.5.8) with object
  streams (7.5.7), stream recompression profiles over the
  [02-filters](02-filters.md) deflate encoder, `/ID` handling per 14.4.
- Incremental-update writer (7.5.6): append-only, original bytes preserved as a
  byte-identical prefix (a test invariant, asserted on every output), changed-object
  tracking via an explicit `ChangeSet`, appended xref section or stream matching the base
  file's flavor with a correct `/Prev` chain, and the signature seams — reserved
  `/Contents` hex gap plus padded `/ByteRange` slots (12.8.1), offsets patched after
  layout. The engine provides the mechanics; CMS production and everything cryptographic
  about signing stays in Tinker.
- Encrypt-on-save: R6 write path (Algorithm 2.A/2.B synthesis of `/O`, `/U`, `/OE`,
  `/UE`, `/Perms`), R4 with `/AESV2` crypt filters for interop, key and salt generation
  through the `EntropySource` seam from [03-encryption](03-encryption.md), `/P` written
  with reserved bits set to 1 as 7.6.3 requires — the exact detail MuPDF's read side
  tripped over (limitation #2 in Tinker's `mupdf-limitations.md`).
- Save-with-permissions: R6 with an empty user password, owner password, and permission
  bits — the "open freely, restrict actions" configuration, as a first-class option.
- Linearization per Annex F: part 1–11 layout, primary hint stream with page-offset and
  shared-object hint tables, first-page objects front-loaded, parameter dictionary
  (`/Linearized`, `/L`, `/H`, `/O`, `/E`, `/N`, `/T`); validated by `qpdf --check` and
  `qpdf --show-linearization` as subprocess oracles per ruling 9 in
  [99-consistency](99-consistency.md).
- Facade surface: `doc.save(&WriteOptions) -> Vec<u8>`, bytes out — no I/O traits in the
  core, same reasoning as reading, because wasm32-unknown-unknown is first-class and has
  no files. Path and writer conveniences are facade sugar on native.

## Non-goals

- **Signing.** CMS SignedData, certificate chains, timestamps, appearance streams — all of
  it is Tinker's `plans/10`. This phase ends at "here are the byte spans, digest and patch
  them"; nothing in the engine ever sees a private key.
- **Content-stream generation and editing.** Producing or rewriting page content is
  [10-editing](10-editing.md) and [12-creation](12-creation.md); this phase serializes
  whatever object graph it is handed.
- **Semantic optimization.** Image downsampling, font subsetting, structure pruning —
  those are editing-side passes ([10-editing](10-editing.md)) or Tinker features. Here,
  "optimize" means only garbage collection and recompression.
- **Encoders other than deflate.** A [02-filters](02-filters.md) non-goal restated: the
  writer emits `FlateDecode` for everything it compresses and never transcodes an existing
  codec.
- **Writing R2, R3, or R5 encryption.** Read-only revisions. R5 in particular is the
  withdrawn draft [03-encryption](03-encryption.md) accepts with a warning; producing it
  would be malpractice. Public-key handlers are out on both sides.
- **PDF 2.0 write features** (`/KDF`, unencrypted wrappers) — tracked in `pdf20-deltas.md`
  per the locked 1.7 baseline.

## Design

### Deterministic serialization

The serializer is a pure function of the object graph and options: same input, same bytes,
on every platform. Reals use shortest round-trip decimal with an integer fast path and no
platform libm — the same discipline ruling 4 imposes on the rasterizer, applied here because
byte-stable output is what makes golden tests, caching, and the incremental prefix invariant
cheap to enforce. Unencrypted saves are fully deterministic; `/ID[1]` is the MD5 of the
serialized body rather than a random value, so even the file identifier costs no entropy
(`/ID[0]` is preserved from the source per 14.4). Encrypted saves are necessarily
randomized — salts and IVs come from `EntropySource` — and that is the only exception.

`PdfString` re-emits in its recorded literal or hex form; `Dict` re-emits in insertion
order. Destinations round-trip as the `Explicit | Named | Uri` enum without rewriting, per
ruling 6 — the writer must never repeat MuPDF limitation #6, where a URI silently became a
percent-encoded named destination.

The header version is the maximum of the source version and what the emitted features
demand: 1.5 for cross-reference and object streams, 1.6 for `/AESV2`, 2.0 for R6. Readers
key off the actual structures, not the header, so this is honesty rather than
compatibility — and on incremental saves, where the header cannot change, a needed upgrade
is written as `/Version` in the updated catalog (7.5.2).

### Full rewrite

Garbage collection marks from the complete trailer dictionary of the newest revision and
walks every key of every reachable object generically — no schema. A schema-driven walk
drops whatever it does not know about (proprietary keys, `/PieceInfo`, extension
dictionaries), and losing an object we merely failed to recognize is the classic GC bug.
Unreachable objects are dropped, survivors renumbered compactly `1..=n` preserving original
ascending order (stable, diff-friendly), except under linearization, which numbers in
output order because the hint tables' delta encoding assumes it. The free list collapses to
the single mandatory entry 0.

```rust
pub enum XrefFlavor { Classic, Streams }

pub enum StreamProfile { Keep, Store, Flate }

pub struct RewriteOptions {
    pub garbage_collect: bool,   // default true
    pub xref: XrefFlavor,        // default Streams
    pub object_streams: bool,    // requires Streams (7.5.7 type-2 entries); forced off under Classic
    pub compress: StreamProfile, // default Flate
    pub linearize: bool,         // default false
}
```

Recompression policy has fixed carve-outs the profile cannot override: streams carrying a
lossy or exotic codec (`DCTDecode`, `CCITTFaxDecode`, `JBIG2Decode`, `JPXDecode`) pass
through as raw bytes untouched — recompressing lossy data is lossy, and re-encoding a codec
we can decode but not encode is impossible anyway. `Flate` re-encodes only streams that are
currently unfiltered or Flate-only chains, using the [02-filters](02-filters.md) encoder
(one fixed strategy; its 1.2×-of-zlib-6 size gate is that phase's exit criterion, not
re-litigated here). `Keep` passes every stream's encoded bytes through verbatim — the
damage-proof profile, and the fallback whenever a decode fails.

Object-stream packing takes every eligible object — not a stream, generation 0, not the
encryption dictionary (7.5.7's exclusions) — in chunks of ~100. Every stream is serialized
to a scratch buffer before its dictionary is emitted, so `/Length` is always a known direct
integer and offsets fall out of a single layout pass. Indirect `/Length` is a read-side
leniency we never produce.

Rewriting a document that contains signature fields necessarily invalidates the signatures
(every byte moves). That is correct behavior, but silent correctness is how users lose
signatures: the writer emits a structured `WillInvalidateSignatures` warning (provenance
per ruling 10) when `/SigFlags` or populated signature fields are present, and the caller
decides.

### Incremental updates

```rust
/// Everything an incremental save appends. Produced by the editing overlay
/// (10-editing) or by engine-internal operations; consumed only here.
pub struct ChangeSet { /* replaced/new objects, freed refs, trailer deltas */ }

impl ChangeSet {
    pub fn put(&mut self, r: ObjRef, obj: Object);
    pub fn free(&mut self, r: ObjRef);
    pub fn allocate(&mut self) -> ObjRef; // next free number in the merged table
}

pub struct SignatureSeam {
    /// Span of the hex placeholder inside /Contents, exclusive of < >.
    pub contents: Range<usize>,
    /// Span of the /ByteRange array value, space-padded after patching.
    pub byte_range: Range<usize>,
}

pub struct IncrementalOutput {
    pub bytes: Vec<u8>,          // starts_with(original) — always
    pub seams: Vec<SignatureSeam>,
}
```

The output is `original ‖ newline ‖ new bodies ‖ new xref ‖ trailer ‖ startxref ‖ %%EOF`.
The leading newline makes the append safe whether or not the original ended with an EOL —
the prefix is never touched to find out. `starts_with(original)` is asserted in every test
that produces an incremental output, without exception; it is the invariant that keeps
existing signatures valid, and a writer that violates it once is a writer nobody can trust
for signing.

The appended cross-reference section matches the base file's newest-revision flavor: a
classic table gets a classic section, an xref-stream file gets an xref stream — appending a
classic table to an xref-stream file is illegal (7.5.8.4 covers only the hybrid files that
were born that way). `/Prev` points at the previous `startxref` target; the new trailer
carries forward `/Root`, `/Info`, `/Encrypt`, and `/ID` with `/ID[1]` refreshed.

Signature seams: when a `ChangeSet` object carries a signature-dictionary placeholder, the
writer reserves a caller-sized run of `0` hex digits between `<` and `>` for `/Contents`
and fixed-width, space-padded slots for the four `/ByteRange` integers. After layout the
absolute offsets are known; the writer patches `/ByteRange` itself (the two ranges bracket
the entire `/Contents` value including delimiters) and returns both spans. The caller —
Tinker's signer — digests `bytes[..contents.start-1]` and `bytes[contents.end+1..]`, builds
its CMS, and hex-patches it into the gap. The engine never interprets what goes in the gap.

Repair interplay, stated as policy: the [01-cos-and-object-model](01-cos-and-object-model.md)
leniency ladder has three levels. A base document that opened at level 1 (trusted) or
level 2 (patched offsets) may be saved incrementally — and for level 2 the appended xref
includes corrected entries for the objects whose offsets were patched, which is repair by
append: the broken prior sections stay byte-identical, the new section shadows them, and
the file is healthier than it arrived. A document that needed level 3 (full rescan) is
refused with `WriteError::BaseNeedsRewrite`: the in-memory view no longer corresponds to
any xref in the file, and appending an update that pretends otherwise produces a file only
we can read. Full rewrite is the honest path there.

### Encrypt-on-save

[03-encryption](03-encryption.md) built its primitives bidirectional and left the
`EntropySource` seam precisely so this section needs nothing new from crypto. R6 is the
default and the only path offered without a reason: generate the 32-byte file key and the
four 8-byte salts from `EntropySource`, compute `/U` and `/O` via Algorithm 2.B, wrap the
file key into `/UE` and `/OE` with AES-256-CBC (zero IV, no padding), and produce `/Perms`
as one raw AES-256 block — `/P` little-endian in bytes 0–3, `T`/`F` at byte 8, `adb` at
9–11, random tail. `/P` is written with all reserved bits set to 1. R4 with `/AESV2` (and
`/V2` RC4-128 beneath the same crypt-filter plumbing) exists behind an explicit interop
option for consumers stuck with pre-R6 readers; nothing older is ever produced.

Write-side rules mirror the read side exactly, inverted: compress, then encrypt; the
never-encrypted set (the `/Encrypt` dictionary's strings, trailer `/ID`, cross-reference
streams) stays clear; strings inside object streams are not individually encrypted —
the container is (7.5.7); metadata follows `/EncryptMetadata`.

On incremental saves, `encryption: None` (keep) encrypts new objects with the held file
key and the base file's `/StmF`/`/StrF` methods — which requires the document to have been
authenticated. Changing or removing encryption incrementally is structurally impossible
(every string in the file would need re-encryption, which means touching the prefix) and
returns `WriteError::EncryptionChangeNeedsRewrite` rather than pretending.

### Linearization

Annex F, parts 1–11: header; linearization parameter dictionary; first-page xref and
trailer at the front of the file; catalog and required document-level objects; the primary
hint stream; the first page's objects with their shared resources; remaining pages in
order; shared and other objects; the main xref and trailer at the end. The primary hint
stream carries the two required tables — page-offset hints and shared-object hints,
bit-packed per F.4; the optional generic hint tables (outlines, threads) are omitted,
which the spec permits and qpdf accepts.

Layout is two-pass with patch-after-layout, deliberately the same machinery as the
signature seams: the hint stream's own length changes every subsequent offset, so the
writer reserves a padded gap for it (padding is legal; qpdf does the same), lays out the
file, then fills the hint stream and patches `/L`, `/H`, `/E`, `/T`, and the first-page
xref in place. Objects are numbered in output order, which keeps the hint tables' deltas
compact and is what real linearizers do.

Honesty about value: hints are advisory and most viewers ignore them; the practical win is
HTTP range serving plus the workflows that validate "fast web view" as a checkbox. It is
off by default, it is the superiority item over MuPDF 1.26+ (which removed the capability;
Tinker's `plans/03` had to shell out to qpdf for it), and it is also the phase's named
descope lever in [PLAN.md](../PLAN.md) if the band is pressed. Incremental updates to a
linearized file de-linearize it — by nature, not by bug; the linearization dictionary
becomes stale and validators report it. Documented, not errored. Linearize combines with
encryption but not with `Incremental` mode.

### Facade surface

```rust
pub enum SaveMode {
    Rewrite(RewriteOptions),
    Incremental,
}

pub enum EncryptionSpec {
    Remove,                                       // requires an authenticated document; policy is the caller's
    R6 { user_password: Vec<u8>, owner_password: Vec<u8>,
         permissions: Permissions, encrypt_metadata: bool },
    R4Aes { /* interop only; same fields */ },
}

impl EncryptionSpec {
    /// Save-with-permissions: empty user password, owner password, /P bits.
    pub fn permissions_only(owner_password: Vec<u8>, permissions: Permissions) -> Self;
}

pub struct WriteOptions {
    pub mode: SaveMode,
    pub encryption: Option<EncryptionSpec>, // None = keep the document's current state
}

impl Document {
    pub fn save(&self, opts: &WriteOptions) -> Result<Vec<u8>, WriteError>;
}
```

Bytes out, mirroring bytes in: the core has no I/O traits, wasm needs none, and the native
facade adds `save_to_path` sugar. Invalid combinations are typed errors, not silent
downgrades: `Incremental` + `linearize` has no meaning, `Incremental` + `Some(spec)` is
the encryption-change refusal above, `Classic` + `object_streams` forces the latter off
with a warning.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Token/object serializer: all 7.3 forms, provenance-preserving strings and dict order, direct `/Length`, deterministic reals | proptest `parse(serialize(graph)) == graph` on arbitrary graphs including delimiter-heavy strings, `#`-escaped names, and streams; serializing twice is byte-identical; builds and tests on wasm32-unknown-unknown | S |
| 2 | Full rewrite: generic mark-from-trailer GC, renumbering, classic xref + xref streams + object streams, recompression profiles with codec carve-outs, `/ID` per 14.4, `WillInvalidateSignatures` warning | Corpus rewrite reopens in our own reader with equal page count (structured-text equality joins the gate when [06-content-and-text](06-content-and-text.md) lands); `qpdf --check` green on every output; GC fixture drops known-dead objects and keeps an object reachable only through an unknown key; both xref flavors exercised corpus-wide; unencrypted saves byte-deterministic across platforms | M |
| 3 | Incremental writer: `ChangeSet`, flavor matching, `/Prev` chain, signature seams, level-2 repair-by-append, level-3 refusal | Every output asserts `starts_with(original)`; appended flavor matches base on classic, stream, and hybrid fixtures; `/ByteRange` values verified by independently digesting the returned spans; hex gap size honored exactly; rescan-repaired base returns `BaseNeedsRewrite`; `qpdf --check` green on updated files | M |
| 4 | Encrypt-on-save: R6 write path, R4/AESV2 interop, `permissions_only`, `EntropySource` wiring, incremental-keep | R6 output reopens with user password → `AuthLevel::User`, owner → `Owner`, permissions round-trip with reserved bits 1 and `/Perms` validating; mutool and qpdf (subprocess oracles, ruling 9) both open the outputs; ObjStm-strings-encrypted-once fixture round-trips; incremental save on an encrypted base encrypts new objects with the held key | S |
| 5 | Linearization: Annex F layout, page-offset + shared-object hint tables, padded hint gap, parameter-dict patching | `qpdf --check` and `qpdf --show-linearization` report no errors across the linearized corpus subset; first-page objects verifiably precede part 7 (offset assertion); linearize+encrypt output validates; de-linearization by later incremental update documented and fixture-pinned | M |

Total sits at the top of the L band. Linearization is the named descope lever
([PLAN.md](../PLAN.md)) if reality presses past it; nothing downstream depends on it.

## Dependencies

- **Needs:** [01-cos-and-object-model](01-cos-and-object-model.md) complete — the object
  model, revision bookkeeping, merged xref, and repair-level reporting are this phase's
  raw material. [02-filters](02-filters.md) wave 1 through its deflate-encoder milestone —
  the write lane in [PLAN.md](../PLAN.md) starts exactly there, and milestone 2 is the
  first consumer. [03-encryption](03-encryption.md) for milestone 4 only: its primitives
  are bidirectional by design and `EntropySource` is the entire additional surface.
  [04-document-semantics](04-document-semantics.md) for linearization's first-page object
  identification and the page-count exit predicates.
  [06-content-and-text](06-content-and-text.md) is not a blocker: it upgrades milestone
  2's gate with text equality when it lands.
- **Unblocks:** the write-parity leg of Checkpoint B (`qpdf --check` green is part of that
  bar in [PLAN.md](../PLAN.md)); Tinker's signing plan, whose incremental writer + seam
  requirements this phase is the standalone answer to; [10-editing](10-editing.md),
  [11-forms](11-forms.md), and [12-creation](12-creation.md), which all save through this
  phase; and Tinker's sanitize/optimize rewrite paths at integration
  ([15-tinker-integration](15-tinker-integration.md)).

## Risks

| Risk | Mitigation |
| --- | --- |
| A subtle prefix mutation (EOL normalization, BOM handling, off-by-one at append) silently invalidates every signature built on the incremental writer | `starts_with(original)` asserted in every incremental test without exception; the append begins with its own newline so the prefix is never inspected or repaired; fixtures include a no-trailing-EOL base; Tinker's signing CI (pyHanko cross-validation) becomes a second, external guard once integration lands |
| Own deflate compresses worse than the source, making every rewrite grow the file | The 1.2×-of-zlib-6 gate is [02-filters](02-filters.md)'s exit criterion; the `Keep` profile passes original encoded bytes untouched as the guaranteed non-regression path; milestone 2 tracks corpus size delta so growth is measured, not assumed |
| Hint tables are the least-exercised structure in the format — readers ignore them, so bugs survive | qpdf is the arbiter: `--check` plus `--show-linearization` on every linearized output; only the two required tables are emitted; the whole feature is a descope lever, so it can never hold the phase hostage |
| Generic GC still over-collects — an object reachable only via a mechanism that is not a key walk (e.g., a reference embedded in content-stream text) | Mark-from-trailer walks every key of every dict and array with no schema; content-stream references go through `/Resources` names, which the walk covers; a fixture with `/PieceInfo`, extension dicts, and a deliberately odd reachable object pins the behavior; `garbage_collect: false` is the escape hatch |
| Rewrite of a signed document destroys signatures without the user understanding why | Structured `WillInvalidateSignatures` warning with provenance (ruling 10); Tinker's UI decides whether to force incremental mode; the warning is fixture-pinned so it cannot rot |
| A host wires a weak `EntropySource` and every "encrypted" file shares predictable keys | The trait contract documents the CSPRNG requirement; facade native hosts wire OS RNGs, wasm bridges `crypto.getRandomValues`; the deterministic test source is `cfg(test)`-gated so it cannot ship; `SECURITY.md` names the responsibility split |
| Encryption × object-stream interactions (double-encrypted ObjStm strings, encrypted xref streams) corrupt output that our own lenient reader still opens, hiding the bug | Oracle round-trips, not self-round-trips, are the gate: mutool and qpdf must open every encrypted output (ruling 9); the write path is a mirror of read rules already fixture-pinned in [03-encryption](03-encryption.md) |
| Float formatting drifts across platforms and breaks byte-determinism | Shortest-round-trip decimal with integer fast path, no platform libm — the ruling-4 discipline; CI compares serialized goldens across linux/windows/macos/wasm exactly as the rasterizer does its bitmaps |

---

Sibling context: [01-cos-and-object-model](01-cos-and-object-model.md) owns the structures
this phase serializes; [02-filters](02-filters.md) owns the deflate encoder and its size
gate; [03-encryption](03-encryption.md) owns every algorithm encrypt-on-save invokes.
Master plan and checkpoint definitions: [PLAN.md](../PLAN.md).
