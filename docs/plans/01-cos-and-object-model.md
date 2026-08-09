# Phase 01 — COS and the object model

When this phase is done, `tinker-pdf-cos` opens any byte sequence that any real-world
reader would call a PDF — well-formed, incrementally updated, hybrid, encrypted, or
actively lying about itself — and exposes a lazy, thread-safe, never-panicking object
store over it. This is the root of the semantics lane in [PLAN.md](../PLAN.md): every
other reading phase consumes what this one produces, and MuPDF-grade leniency on broken
files is the single hardest thing to replicate about MuPDF, so it is designed in from
the first milestone rather than bolted on. The phase is read-only by design: the same
crate will later carry the serializers, but writing is [09-writing](09-writing.md)'s
problem and none of the structures here are allowed to assume mutation.

## Scope

- Lexer for the full token grammar of 7.2: whitespace and delimiter classes (7.2.2),
  comments (7.2.3), keywords (`obj`, `endobj`, `stream`, `endstream`, `R`, `xref`,
  `trailer`, `startxref`, `true`, `false`, `null`).
- All object forms of 7.3: null, boolean, integer, real, literal string (7.3.4.2 —
  nested parens, all escapes, octal, line continuations), hex string (7.3.4.3), name
  with `#xx` escapes (7.3.5), array, dictionary, stream (7.3.8), indirect reference
  (7.3.10). Numeric leniency including broken exponent forms.
- Cross-reference machinery: classic tables (7.5.4) including broken-offset tolerance,
  cross-reference streams (7.5.8), hybrid files via `/XRefStm` (7.5.8.4), `/Prev`
  chains with cycle guards, `startxref` chase, and incremental-update reading (7.5.6)
  with per-revision bookkeeping.
- Object streams (7.5.7), decompressed once and cached.
- Lazy object loading into an `Arc`'d store, safe under concurrent access, with
  reference-cycle guards.
- The repair scanner and the three-level leniency ladder (trust → patch → rescan).
- Stream data access: indirect `/Length` resolution, missing-`endstream` recovery, and
  the three-tier raw/decrypted/decoded API.
- The encryption hookup point — the `Decryptor` seam that
  [03-encryption](03-encryption.md) fills in.
- `tpdf objects`: the debug dump that makes every one of the above inspectable.

## Non-goals

- **Serialization and incremental-update writing.** The serializers live in this crate
  eventually, but they are specified and built in [09-writing](09-writing.md).
- **Document semantics.** Page tree APIs, inheritance, outlines, destinations, name
  trees — [04-document-semantics](04-document-semantics.md). This phase walks the page
  tree only as a test predicate (resolve `/Root` → `/Pages` → `/Kids` with a visited
  set), not as an API.
- **Filter implementations.** `stream_decoded` calls into
  [02-filters](02-filters.md); this phase defines only the call and the decode limits
  it passes.
- **Key derivation and ciphers.** This phase parses `/Encrypt` and owns the decrypt
  call sites; the algorithms are [03-encryption](03-encryption.md)'s.
- **PDF 2.0 deltas** (UTF-8 string type, 2.0 xref clarifications) are recorded in
  `pdf20-deltas.md` as encountered, per the locked baseline of PDF 1.7 (ISO 32000-1).

## Design

### Input model

The document owns one immutable byte buffer:

```rust
pub struct CosDocument { /* buffer: Arc<[u8]>, xref, slots, decryptor, warnings */ }

impl CosDocument {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<CosDocument, OpenError>;
    pub fn trailer(&self) -> &Dict;
    pub fn get(&self, r: ObjRef) -> Result<Arc<Object>, CosError>;
    pub fn warnings(&self) -> &[Warning];
    pub fn revisions(&self) -> &[Revision];   // newest first; byte range + trailer per revision
}
```

Bytes in, values out — no file handles, no I/O traits in the core. wasm32-unknown-
unknown has neither mmap nor files, and it is a first-class target, so the caller does
the reading. Memory-mapping is a facade decision on native platforms; everything below
works on a slice. `OpenError` is reserved for total failure — not one object could be
located even after a full rescan. Anything less is a warning plus degraded content,
per rulings 1 and 10 in [99-consistency](99-consistency.md).

### Lexer

A hand-written state machine over `&[u8]` with a cursor — no regex, no lookahead
buffer, positions preserved for warnings. Token forms and the leniency decisions,
which are policy, not accident:

- **Literal strings** (7.3.4.2): balanced nested parens; escapes `\n \r \t \b \f \( \)
  \\`; octal `\ddd` with one to three digits, overflow taken mod 256; backslash before
  end-of-line is a line continuation (both dropped); a backslash before any other
  character drops the backslash; a raw CRLF or CR inside the string normalizes to LF.
  Unterminated string at EOF: close it, warn.
- **Hex strings** (7.3.4.3): whitespace ignored; odd digit count padded with a
  trailing zero; a non-hex byte is skipped with a warning rather than aborting the
  string — real files contain them.
- **Names** (7.3.5): `#xx` decodes to the raw byte; a `#` not followed by two hex
  digits is taken literally with a warning (the pdf.js behavior — rejecting the name
  loses the whole dict key). Names are byte strings, never assumed UTF-8.
- **Numbers** (7.3.3): integers clamp to `i64` on overflow with a warning; leading `+`
  accepted; the doubled-sign form `--5` that broken producers emit parses as `-5` with
  a warning. Exponent notation (`6.02e23`) is illegal in PDF but exists in the wild:
  parse and evaluate it, warn. Any other malformed numeric lexes as its longest valid
  prefix, or `0` if there is none, always with a warning — MuPDF's effective behavior,
  and the one the corpus rewards.
- **`stream` keyword** (7.3.8.1): must be followed by CRLF or LF; a lone CR, or stray
  bytes before the EOL, are tolerated with a warning.

The lexer never fails; it produces a token or a warning-carrying fallback at every
position. Failure is a parser-level concept.

### Objects and the store

```rust
pub enum Object {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    String(PdfString),   // bytes + literal/hex origin, post-decryption
    Name(Name),          // interned symbol
    Array(Vec<Object>),
    Dict(Dict),
    Stream(StreamObj),   // Dict + data byte-range into the buffer
    Ref(ObjRef),         // { num: u32, gen: u16 }
}
```

Loaded objects are immutable and shared as `Arc<Object>` — one parse, any number of
concurrent readers, no copy-on-read. Editing ([10-editing](10-editing.md)) will layer
an overlay above the store rather than mutate it; that decision is made now so nothing
here grows interior mutability later.

`Name` is a `u32` symbol interned per document, with the common spec names (`Type`,
`Pages`, `Kids`, `Length`, `Filter`, …) pre-interned at fixed values so the content
interpreter's hot loop compares integers. `Dict` is an insertion-ordered flat
`Vec<(Name, Object)>` with linear lookup — real dictionaries are small (typically
under 16 entries), a flat vec beats a hash map there, and preserving file order is
something [09-writing](09-writing.md) will want. `PdfString` records whether it was
literal or hex for the same reason.

### Cross-reference machinery

```rust
pub enum XrefEntry {
    Free { next: u32, gen: u16 },
    Offset { offset: u64, gen: u16 },      // classic type-1
    InStream { stream_num: u32, idx: u32 },// xref-stream type-2
}
```

The merged table is a dense `Vec<Option<XrefEntry>>` indexed by object number, built
newest-revision-first with first-writer-wins (newest revision shadows older). `/Size`
is a claim, not a fact: the dense table is capped at a documented constant (1 M
slots); numbers beyond the cap spill to a fallback map, so a hostile `/Size` cannot
allocate gigabytes.

- **`startxref` chase:** scan the final 1 KiB for `startxref` (7.5.5), extending to
  64 KiB before giving up — trailing junk after `%%EOF` is routine. A missing or
  out-of-range offset drops straight to ladder level 3.
- **Classic tables** (7.5.4): subsection headers, 20-byte entries; 19- and 21-byte
  entries (wrong EOL discipline) tolerated by resynchronizing on the entry grammar
  rather than fixed stride.
- **Xref streams** (7.5.8): `/W` field widths including the `W[0] = 0` default-to-
  type-1 rule; `/Index` defaulting to `[0 /Size]`; entry types 0/1/2; unknown types
  read as references to null per 7.5.8.3. Decoding needs FlateDecode with PNG
  predictors from [02-filters](02-filters.md) — xref streams are never encrypted, so
  crypto is not in this loop.
- **Hybrids** (7.5.8.4): within one revision, classic entries are consulted first,
  then the `/XRefStm` stream, and only then `/Prev`.
- **`/Prev` chains:** followed with a visited-offset set and a depth cap; a cycle or
  a bad link warns and stops the chain rather than failing the open.
- **Revisions:** each trailer step records `(byte_range, trailer_dict)` in
  `revisions()`. Signature byte-range checking and "save original revision" are later
  phases' features, but only this phase sees the boundaries cheaply, so it keeps them.
- **Broken offsets:** a leading-junk file (bytes before `%PDF-`, 7.5.2 violated) gets
  one uniform shift computed from the header position and applied to every offset —
  the single most common corruption in the wild. Per-object failures after that are
  ladder level 2.

Every offset the table hands out is verified before use: the bytes at the target must
parse as `N G obj` with matching numbers. An offset that lies is a per-object repair,
not a trusted read — this check is what makes level 1 of the ladder safe.

### Object streams

7.5.7: `/N` pairs of (object number, relative offset) in the first `/First` bytes,
then the objects. The decompressed stream and its parsed offset table are cached
keyed by the stream's object number, so fifty objects in one `ObjStm` cost one
inflate. Contained objects always have generation 0 and are never themselves streams;
violations warn and skip the entry. Mismatched `/N`/`/First` fall back to lexing
pairs until the numbers stop making sense, keeping what parsed. Strings inside object
streams are **not** decrypted individually (7.6.2 — the containing stream was already
decrypted); the loader threads an `in_objstm` flag to the string path for exactly
this reason.

### Lazy loading, cycle guards, concurrency

Objects load on first `get`. The store is a slot table; publication is a
compare-and-swap of the `Arc`:

- No lock is held while parsing. Two threads racing on the same object both parse
  (parsing is pure — same bytes, same result), one CAS wins, the loser drops its
  copy. Cheaper and deadlock-proof compared to blocking a slot, and mutually
  referencing objects loaded from two threads cannot deadlock because nobody waits.
- Cycles go through a `ResolveCtx` carried down the load path holding the in-progress
  object numbers. Re-entering an object already on the stack — a `/Length` pointing
  into its own stream, a self-referential dict — yields `Object::Null` plus a
  provenance warning instead of recursing forever. `std::sync::OnceLock` was rejected
  precisely because same-thread re-entry deadlocks it.
- Reference-following helpers (`resolve`) are depth-capped independently, so long
  `Ref → Ref → Ref` chains terminate.
- On wasm32-unknown-unknown the same code runs single-threaded; the atomics compile
  and never contend. No separate code path.

### Streams: `/Length` and the three-tier access API

```rust
impl CosDocument {
    /// Exact bytes from the file: encrypted if the file is, never decoded.
    pub fn stream_raw_encrypted(&self, r: ObjRef) -> Result<&[u8], CosError>;
    /// Decrypted, filters NOT applied. DCT data comes out as JPEG bytes.
    pub fn stream_raw(&self, r: ObjRef) -> Result<Vec<u8>, CosError>;
    /// Decrypted and run through the /Filter chain, subject to decode limits.
    pub fn stream_decoded(&self, r: ObjRef) -> Result<Vec<u8>, CosError>;
}
```

Three tiers because collapsing them is exactly how MuPDF's API got vague: Tinker's
`docs/mupdf-limitations.md` carries `read_raw_stream` as "semantics unconfirmed" and
Tinker's `plans/07-extraction-and-data-mining.md` had to plan a lopdf fallback for
byte-identical image extraction because of it. Here the semantics are fixed by
construction: `stream_raw_encrypted` is the forensic tier (signature verification,
byte-identical revision copy in [09-writing](09-writing.md)); `stream_raw` is the
extraction tier (a JPEG extracted is the JPEG embedded, byte for byte); and
`stream_decoded` is what the interpreter eats. `stream_decoded` passes an output-size
cap into [02-filters](02-filters.md) so a decompression bomb costs bounded memory.

`/Length` policy: resolve it (indirect `/Length` is common and goes through the
store, cycle-guarded); trust it if the bytes at `offset + length` are `endstream`
optionally preceded by an EOL; otherwise scan forward from the stream start for the
`endstream` keyword and take that extent, warning with both lengths. A missing
`endstream` entirely truncates at the next `N G obj` header or EOF, warning again.
Declared-but-wrong lengths are among the most common real-world damage; trusting the
keyword over the number is what every surviving reader does.

### Encryption hookup

```rust
pub trait Decryptor: Send + Sync {
    fn decrypt_string(&self, containing: ObjRef, data: &[u8]) -> Vec<u8>;
    fn decrypt_stream(&self, r: ObjRef, data: &[u8]) -> Vec<u8>;
}
```

This phase owns the call sites and their exemptions — strings decrypt at object load
with the containing indirect object's ref (the RC4/AES key is salted per object);
streams decrypt in `stream_raw`/`stream_decoded`; the `/Encrypt` dictionary itself,
xref streams, and strings inside object streams are never passed to the decryptor
(7.6.2). [03-encryption](03-encryption.md) supplies the real implementation; this
phase ships `IdentityDecryptor` and the wiring: after the trailer parses, cos
extracts the `/Encrypt` scalars and byte strings plus the first `/ID` element and
hands them across as plain values — `tinker-pdf-crypto` is a leaf crate with zero
PDF types, per ruling 8. Password entry and user/owner distinction are 03's scope;
the seam here is deliberately dumb.

### The leniency ladder

The moat, stated as policy so it can be tested as policy:

1. **Trust** — xref offsets used as-is, every read validated against its `N G obj`
   header. Zero warnings means the file was honest.
2. **Patch** — a validation failure repairs that entry from the scan index (built
   lazily on first failure) and warns, keeping the rest of the table. Bounded damage
   costs bounded work.
3. **Rescan** — no usable `startxref`, unparseable tables, or per-object failures
   past a threshold: discard the tables, take the scan index as truth, synthesize a
   trailer if the file's own is gone (`/Root` = the `/Type /Catalog` dict at the
   highest offset; `/Info` likewise if present).

The repair scanner is one forward pass over the whole buffer matching
`digits ws digits ws "obj"` at token boundaries. On each hit it parses the object; if
the object is a stream, the scanner skips its body using the `endstream` recovery
logic above — which is what keeps `N G obj` sequences *inside* stream data from
poisoning the index. Duplicate object numbers keep the higher generation, then the
higher offset (later means newer in an incrementally updated file). Trailer
candidates (`trailer` keyword and `/Type /XRef` streams) are collected in the same
pass.

The ladder is deterministic: the same bytes always take the same path and produce the
same warnings — required for reproducible fuzz triage and for `pdfcmp` runs to mean
anything. Every downgrade emits a typed, object-addressed warning (ruling 10), so
"it opened" and "it opened after a full rescan" are different observable facts.

### Errors and warnings

Never panic on untrusted input (ruling 1) is the contract the fuzzers enforce.
`CosError` is per-object and non-fatal to the document; typed accessors on `Dict` for
the higher layers treat wrong-typed values as absent-with-warning, because that is
what the corpus demands. The warning sink is capped (documented constant, order 10 k)
so a pathological file cannot flood memory with its own diagnostics; the cap itself
warns once. Recursion depth, `/Prev` chain length, objstm nesting and decode output
are all capped by named constants in one module — the fuzzer's job is to prove the
caps are the only limits that exist.

### tpdf

`tpdf objects <file>` dumps the merged xref (with per-entry ladder provenance), the
trailer, revision boundaries and any object by number; `--raw` and `--decoded` dump
stream tiers. It is the first consumer of every API above and the debugging tool for
every corpus failure after; it costs little because it is nothing but the public API
plus formatting.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Lexer + object grammar: all token forms, strings, names, numbers, arrays, dicts, refs | Unit suite covers every 7.3 example plus every leniency case named in this plan; proptest round-trips for string/name escaping; `fuzz_lexer` and `fuzz_object` targets run 1 h clean in CI; suite passes on native and wasm32-unknown-unknown | S |
| 2 | Xref machinery + lazy store: classic tables, xref streams, hybrids, `/Prev`, revisions, object streams, cycle guards under threads | Every well-formed seed fixture opens and resolves all objects; objstm cache-hit counter proves one inflate per stream; 8-thread stress over mutually referencing fixtures completes under CI timeout; cyclic-`/Length` fixture yields Null + warning, not a hang | M |
| 3 | Stream access tiers + crypto hookup: `/Length` resolution, `endstream` recovery, three-tier API, `Decryptor` + `IdentityDecryptor` | `stream_raw_encrypted` byte-equal to independently extracted ranges on fixtures; wrong-`/Length` and missing-`endstream` fixtures decode with object-addressed warnings; encrypted fixture parses to the hookup with 03 stubbed | S |
| 4 | Repair scanner + leniency ladder end to end | Corrupt-fixture suite (truncated tail, junk before `%PDF`, lying offsets, dead `startxref`, damaged xref stream, `obj` sequences inside stream data) opens at the documented ladder level, asserted per fixture; 100 repeat runs bit-identical warnings | M |
| 5 | Corpus gate + fuzz campaign + `tpdf objects` | ≥ 99 % of the qpdf + pdf.js test corpora walk the page tree with zero panics, page counts cross-checked against mutool as subprocess oracle where it opens the file (ruling 9); 24 h fuzz campaign across all cos targets crash-free; `tpdf objects` dumps every fixture including repaired ones | S |

Encrypted corpus files count toward the milestone-5 gate only once
[03-encryption](03-encryption.md) lands; until then the runner tags and excludes
them, and the exclusion list is committed so the debt is visible.

## Dependencies

- **Needs first:** [00-architecture](00-architecture.md) (workspace, hand-rolled
  policy boundary — both in place). [02-filters](02-filters.md) wave 1 must deliver
  FlateDecode with PNG predictors before milestone 2's xref-stream work; the
  `tinker-pdf-cos` → `tinker-pdf-filters`/`tinker-pdf-crypto` dependency edges are
  already scaffolded. [03-encryption](03-encryption.md) is *not* a prerequisite —
  the hook ships against `IdentityDecryptor`.
- **Unblocks:** everything on the semantics lane —
  [04-document-semantics](04-document-semantics.md) is the first consumer,
  [06-content-and-text](06-content-and-text.md) and Checkpoint A transitively.
  [09-writing](09-writing.md) builds its serializers into this crate against these
  structures, and its incremental-update writer consumes `revisions()` and the
  forensic stream tier. [14-testing-and-corpora](14-testing-and-corpora.md) takes
  over the corpus runner and ratchet built for milestone 5.

## Risks

| Risk | Mitigation |
| --- | --- |
| Leniency behavior diverges from MuPDF/pdf.js on real-world garbage — the moat is judged by files nobody has seen yet | oracle-diff (mutool page counts, subprocess only, ruling 9) runs corpus-wide in CI; every divergence is a triage item with the fixture kept; corpus pass-rate ratchets per [14](14-testing-and-corpora.md) so the gap trends visible, not surprising |
| Repair scanner false positives: `N G obj` byte patterns inside stream bodies or strings poison the scan index | Scanner parses each hit and skips stream extents using the same `endstream` recovery as normal reads; header re-validation on use; fuzzer-found offenders become permanent fixtures |
| Hostile structure: huge `/Size`, deep nesting, objstm/decode bombs, warning floods | Named cap constants in one module (slot cap + spill map, depth caps, decode output cap, warning cap); fuzz targets run under an allocation limit so a blowup is a crash in CI, not in production |
| Store concurrency bugs — deadlock or torn publication under the parse-race design | No locks held across parsing, CAS-only publication, re-entry handled by `ResolveCtx` not blocking; milestone-2 thread-stress tests with CI timeouts as the deadlock detector; wasm shares the identical code path |
| Number fidelity: `f64` load loses the source lexeme a writer might need to round-trip | Accepted deliberately — [09-writing](09-writing.md)'s incremental updater copies untouched objects as verbatim byte ranges, so lexeme preservation is only needed for objects being rewritten anyway; noted there |
| Corpus redistribution: qpdf and pdf.js test files carry mixed licenses | Fetched in CI at pinned upstream commits, never vendored or redistributed — same posture as the oracle rule (ruling 9); a local mirror script exists for offline work |
| Xref pathology diversity exceeds the enumerated cases | That is what level 3 is for: the rescan path assumes nothing about the tables, so an unforeseen pathology degrades to a full scan instead of a failure; each new one becomes a fixture and, if systematic, a level-2 patch rule |

## As built

Milestones 1–4 are implemented (336 tests across `tinker-pdf-cos` and
`tinker-pdf-filters`; fmt, clippy `-D warnings`, and wasm32 green). Milestone 5 —
the corpus gate and the 24-hour fuzz campaign — is outstanding and moves to
[14-testing-and-corpora](14-testing-and-corpora.md)'s runner. Six places where
the implementation diverged from the design above, each because the design was
wrong rather than inconvenient:

1. **`warnings()` returns an owned `Vec`, not `&[Warning]`.** Objects load lazily
   behind `&self`, so warnings keep arriving after `open`; no borrow could stay
   valid across the next read, and a `Deref` guard would deadlock if held across
   a `get`.
2. **The leading-junk offset shift is a two-candidate probe, not an
   unconditional rewrite.** Shifted is tried first at every offset, unshifted
   second — a producer that prepends junk *and* corrects its own offsets exists,
   and one extra probe is far cheaper than pushing that file to level 2.
3. **Unknown xref-stream entry types map to `Free`.** The enum keeps the three
   specified variants; a free entry reads as null (7.5.8.3's requirement) *and*
   still occupies its slot, so an older revision stays correctly shadowed.
4. **The ladder is decided eagerly at open**, by validating every type-1 entry's
   `N G obj` header. "Per-object failures past a threshold" is not
   deterministically evaluable lazily: `ladder_level()` would depend on the
   caller's access order, contradicting this plan's own requirement that the same
   bytes always take the same path. Level 3 triggers on no usable `startxref`, no
   section parsed, an empty table, an unlocatable `/Root`, or four-plus failures
   that are also a majority of type-1 entries.
5. **`get` ignores the generation number.** The object number is the identity and
   the file's own `N G obj` header wins over a disagreeing table — real tables
   disagree routinely and every surviving reader does this.
6. **`in_objstm` is a separate load path, not a flag.** `load_from_objstm` simply
   never calls the decryptor, which is stronger than threading a boolean that a
   later edit could forget to check (7.6.2).

---

Sibling context: [02-filters](02-filters.md) and [03-encryption](03-encryption.md)
supply the two capabilities this phase calls across crate boundaries;
[99-consistency](99-consistency.md) rulings 1, 8, 9 and 10 bind the design above.
See [PLAN.md](../PLAN.md) for lanes and checkpoints.
