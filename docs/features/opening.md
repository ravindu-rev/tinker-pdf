# Opening documents

`Document::open` takes bytes and gives back a document, or a typed reason why
not — and "why not" is deliberately rare. A damaged file opens anyway wherever
anything can be recovered; what the recovery cost is recorded on the document
itself, as a ladder level and a list of typed, object-addressed warnings, so
"it opened" and "it opened cleanly" stay distinguishable facts (ruling 10,
[rulings](../rulings.md)). Below the facade sits the COS layer: one immutable
buffer, a merged cross-reference table in every flavour the spec defines, and
a lazy `Send + Sync` object store over it.

## What it does

**Format routing.** Bytes that begin with a container signature are not read
as a PDF. A ZIP (`PK\x03\x04` at offset zero) is one signature over several
formats, so it is opened once and asked what it is: an XPS package is
recognised by ECMA-388 E.3's three steps, an EPUB by OCF's
`META-INF/container.xml`, and anything else falls through to the comic-archive
path — each synthesised into a real document with a real catalog and page
tree. RAR, 7z and tar are refused by name. The signatures are tested at a
fixed position only, so a PDF carrying `PK\x03\x04` inside a stream is
unaffected. See [cbz](cbz.md), [xps](xps.md) and [epub](epub.md); a
reflowable book additionally takes `OpenOptions`, because its page count is a
function of the page box the caller passes, not a property of the file.

**COS parsing.** A hand-written lexer covers the full token grammar of 7.2
and every object form of 7.3: literal strings with all escapes and octal
(7.3.4.2), hex strings (7.3.4.3), names with `#xx` escapes (7.3.5), numbers
with the wild's malformations tolerated and warned (7.3.3 — clamped
overflows, doubled signs, exponent notation), arrays and dictionaries (7.3.6,
7.3.7), streams (7.3.8) and indirect references (7.3.10). The lexer never
fails; every leniency emits a `WarningKind` variant instead of a log line, so
a file that parses with an empty sink is a file that obeyed 7.2 and 7.3.

**Cross-references, every flavour.** Classic tables (7.5.4) with 19- and
21-byte entries resynchronised on the entry grammar; cross-reference streams
(7.5.8) with `/W`, `/Index` defaulting to `[0 /Size]`, and unknown entry
types read as references to null (7.5.8.3); hybrid files via `/XRefStm`
(7.5.8.4); `/Prev` chains with a visited-offset set and a depth cap; and
incremental updates (7.5.6) recorded per revision — each `Revision` carries
the byte range a signature covers and the offset its own `startxref` names
(7.5.5 Table 15). The merged table is built newest-revision-first with
first-writer-wins, dense to a documented cap and spilling to a map above it,
because `/Size` is a claim, not a fact. Objects inside object streams (7.5.7)
are decompressed once and cached; 7.5.7's two rules — generation 0, never
itself a stream — are enforced rather than assumed.

**The leniency ladder.** No offset is trusted: every table entry is validated
against its `N G obj` header when the document opens, eagerly, so the ladder
is decided by the bytes alone rather than by a caller's access order.
Level 1 (`LadderLevel::Trust`) uses the tables as written. Level 2
(`Patch`) repairs a lying entry from the repair scanner — one forward pass
matching `digits ws digits ws obj` at token boundaries, skipping stream
bodies so `N G obj` sequences inside stream data never poison the index —
and keeps the rest of the table. Level 3 (`Rescan`) discards the tables
entirely: no usable `startxref`, an unlocatable `/Root`, or four-plus
validation failures that are also a majority. The scan index becomes truth
and a trailer is synthesised if the file's own is gone. The ladder is
deterministic — same bytes, same path, same warnings ([determinism](determinism.md)).

**Three stream tiers.** `stream_raw_encrypted` is forensic — the exact bytes
the file holds, for signature checking and byte-identical revision copies.
`stream_raw` is extraction — decrypted, undecoded, so a JPEG extracted is the
JPEG embedded. `stream_decoded` is what an interpreter eats, and it is the
only tier with a decode ceiling. `/Length` policy (7.3.8.2): trust the number
when `endstream` sits where it points; otherwise recover from the keyword and
carry both lengths in the warning; with no `endstream` at all, truncate at
the next object header.

**The lazy store.** Objects load on first read and are shared as
`Arc<Object>` — one parse, any number of concurrent readers, no copy on
read. Publication is a compare-and-swap, not a lock around the parse, so
mutually referencing objects loaded from two threads cannot deadlock because
nobody waits. Cycles — a `/Length` pointing into its own stream — read as
null with a warning rather than hanging. The same code runs single-threaded
on `wasm32-unknown-unknown`, which is why the input is bytes rather than a
path: that target has no filesystem ([architecture](../architecture.md)).

**Encryption at open.** An encrypted document opens perfectly well; the
`/Encrypt` scalars are extracted and everything else waits. `readable()` is
what separates "not a PDF" from "wants a password", and
`authenticate` reports which password matched. 7.6.2's exemptions —
cross-reference streams, the metadata stream under `/EncryptMetadata false`,
`/Crypt /Identity`, strings inside object streams — are each honoured. See
[encryption](encryption.md).

## API

The facade entry points, all on `tinker_pdf::Document`: `open(bytes)`,
`open_with(bytes, &OpenOptions)` (the seam where a reflowable book's page box
and a `FontProvider` arrive early enough to decide pagination), `sniff(bytes)`
(a cheap header probe — `%PDF-` in the first 1024 bytes, no parsing),
`readable()`, `authenticate(password)`, `ladder_level()`, `warnings()`, and
`cos()` — the escape hatch to `CosDocument`, whose types (`Object`, `Dict`,
`Name`, `PdfString`, `ObjRef`, `StreamObj`, `XrefTable`, `XrefEntry`,
`Revision`) are re-exported on the facade so the hatch can be named without a
second dependency. On `CosDocument`: `get`, `resolve`, `trailer`,
`revisions`, `xref`, and the tiers `stream_raw_encrypted` / `stream_raw` /
`stream_decoded` (plus `stream_image_input`, decoded up to but not through an
image codec).

```rust
let bytes = std::fs::read("report.pdf")?;
let doc = tinker_pdf::Document::open(bytes)?;
doc.readable()?; // Err(PasswordRequired) — opened fine, wants a password
if doc.ladder_level() != tinker_pdf::LadderLevel::Trust {
    for w in doc.warnings() {
        eprintln!("tolerated: {w}");
    }
}
```

## Refused by name

Hard refusals are `OpenError` and `DocumentError`; everything else is a cap
that degrades with a named warning rather than failing the open. The caps are
hardening limits, not conformance limits — Annex C is advisory and real files
exceed it routinely — declared in one place,
[`limits.rs`](../../crates/tinker-pdf-cos/src/limits.rs).

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| Zero bytes | `OpenError::Empty` | Almost always a caller's bug — a path that did not exist — and telling that apart from a bad file matters | `crates/tinker-pdf/src/lib.rs` |
| Nothing PDF-shaped | `OpenError::NotAPdf` | Not one indirect object found, even after a full rescan | `CosDocument::open` → `OpenError::NoObjects` |
| RAR, 7z, tar | `OpenError::UnsupportedArchive(ArchiveRefusal::NotAZip)` | Recognised containers, refused by name: more decompressors, two of them encumbered, none of them a page | [cbz](cbz.md), [ROADMAP](../ROADMAP.md) |
| Encrypted, nothing authenticated | `DocumentError::PasswordRequired` | The document opened; reading it is the thing that waits | [encryption](encryption.md) |
| Encryption handler not implemented | `DocumentError::UnsupportedEncryption` | A handler outside R2–R6 cannot be pretended at | [encryption](encryption.md) |
| Decompression bomb | `WarningKind::Filter(Warning::OutputCapHit)` | `stream_decoded` output capped at `MAX_DECODED_STREAM` (128 MiB), so a 1 KB stream cannot buy unbounded memory | `limits.rs` |
| Unknown `/Filter` name | `WarningKind::FilterUnknown` | The chain stops there; bytes decoded so far come back still encoded | [filters](filters.md) |
| Container nesting past 256 | `WarningKind::DepthCapHit` | The container is skipped and reads as null; parser recursion stays bounded | `limits.rs` |
| `/Prev` chains past 64 links | `WarningKind::XrefChainCapHit` | Cycles are caught by the visited set; this bounds the acyclic-but-absurd chain | `limits.rs` |
| `Ref → Ref` chains past 32 hops | `WarningKind::ResolveDepthCapHit` | Legal but never deep; the chain reads as null | `limits.rs` |
| Self-referential loads | `WarningKind::ObjectCycle` | A `/Length` into its own stream reads as null rather than hanging | `crates/tinker-pdf-cos/src/store.rs` |
| Warning floods past 10 000 | `WarningKind::WarningCapReached` | A pathological file cannot flood memory with its own diagnostics; the cap warns exactly once | `limits.rs` |

## Verified

As of August 2026, `cargo test --workspace` runs 2 924 tests (0 failed,
8 ignored, Windows x86_64), and the parts that cover opening are named
([verification](../verification.md)):

- **`crates/tinker-pdf-cos/tests/corrupt.rs`** — the ladder on damage built
  byte by byte: truncated tails, junk before `%PDF-`, lying offsets, dead
  `startxref`, each asserting the exact rung and the exact warning.
- **`crates/tinker-pdf-cos/tests/strict_validator.rs`** — the ladder read
  from the other side. `validate` opens a file with every leniency counted as
  a defect and walks the cross-reference sections out of the bytes, which is
  how two repairs nobody could see were found: an entry whose offset points a
  few bytes *before* its object, and one whose generation the table invented.
  Both open at `Trust` with no warning, because the reader lexes forward for
  the first and normalises the second.
- **`crates/tinker-pdf-cos/tests/document.rs`, `object_grammar.rs`,
  `proptest_lexer.rs`, `proptest_document.rs`** — the grammar, the store and
  property-tested round-trips for string and name escaping.
- **`crates/tinker-pdf/tests/hostile_input.rs`** — real fixtures put through
  seeded deterministic damage on every `cargo test`, asserting only that
  nothing panics and nothing hangs (ruling 1, on stable, so it cannot be
  skipped by accident).
- **`crates/tinker-pdf/tests/tinker_parity.rs`** — compares `OpenError`
  values by ruling 12, which is why the enum stays `Copy + PartialEq + Eq`.
- **Fuzzing** — `cos_document` (the whole file parser, every ladder rung
  reachable from arbitrary bytes, plus a bounded page-tree walk) and
  `cos_object` are two of the 24 fuzz targets, run briefly in CI on every
  commit over committed seed corpora.
- **Corpus** — 4 525 files, 4 484 of them rendered every page, 0 crashes
  (August 2026), in the ratcheted corpus run
  ([verification](../verification.md)); the canonical fixtures in
  `crates/tinker-pdf-cos/tests/document.rs` are mutool-written and must open
  at `Trust` with an empty warning list.
- **Determinism** — the 15 render fingerprints and 3 document byte-hashes all
  pass through `Document::open` first, so a change to opening moves them
  ([determinism](determinism.md)).
