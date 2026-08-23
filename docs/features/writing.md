# Writing

Two shapes of output, and the difference is the point. A **full rewrite**
emits every object afresh — the path that compacts, garbage-collects,
re-compresses, encrypts or linearizes a document. An **incremental update**
appends: the original bytes survive byte-for-byte as a prefix and only the
changed objects follow, with a new cross-reference section chained to the
old one — the only way to modify a signed document without breaking the
signature over it. Everything that produces a PDF goes through this one
serializer: `DocumentEditor::save`, `DocumentBuilder::finish`, and the
container conversions built on the builder.

## What it does

**Serializing.** Every object form of 7.3 is written: names with `#xx`
escapes where a byte demands one (7.3.5), literal strings escaping only
what they must (7.3.4.2), reals without exponents — which PDF output does
not allow — and dictionaries in insertion order. A stream's `/Length` is
computed from the bytes actually written (7.3.8.2), never trusted from the
dictionary. A rewrite opens with the declared version header
(`WriteOptions::version`, default 1.7) and the four-byte binary marker of
7.5.2, and nesting past the depth limit degrades to `null` rather than
overflowing the stack.

**Full rewrite.** Every reachable revision of the document is flattened
and re-emitted. Garbage collection (`WriteOptions::garbage_collect`) is
opt-in mark-and-sweep from the trailer, walking every key of every
reachable object with no schema — and it is *off* by default, because
"rewrite" has meant "serialize everything" since the writer existed:
redaction overwrites content streams in place precisely so that an
unreferenced object cannot survive with the original text in it, and that
guarantee is not inherited from a flag. The cross-reference is a classic
table (7.5.4) with contiguous subsections and exactly-twenty-byte entries;
with `object_streams` on, eligible non-stream objects pack into an
`/ObjStm` container (7.5.7) and the file switches to a cross-reference
stream (7.5.8), whose dictionary then carries the trailer entries itself —
such a file has no `trailer` keyword at all (7.5.8.2). The trailer `/Size`
is one more than the highest object number *in the file*, containers and
the `/Encrypt` dictionary included (7.5.5 Table 15).

**Incremental update.** The output starts with the original, byte for
byte — asserted by tests, not assumed — with the appended section opening
on its own newline (7.5.6). Only the changed objects are written, followed
by a classic cross-reference section covering exactly them, so the table
stays small. `/Prev` names the previous cross-reference *section's* offset
(7.5.5 Table 15); a base document with no section to chain to — one the
repair scanner rebuilt — gets no `/Prev` at all, rather than a `/Prev 0`
sending every reader to byte zero. `/Size` can only go up: the changed
set's own maximum never clobbers the file's. An incremental update to a
linearized file de-linearizes it by nature — the parameter dictionary goes
stale — which is documented, not errored.

**Compression.** `WriteOptions::compress` deflates streams through the
project's own encoder and records `/FlateDecode`. Two hard rules: a stream
that already declares a `/Filter` is handed through untouched — that is a
contract, not an optimisation, since pass-through image data depends on its
declared filter still describing its bytes — and a result no smaller than
the input is discarded, so an already-compressed image never grows. The
object-stream container is compressed whenever compression is on at all;
a cross-reference stream never is, because a reader finds it by offset
before it knows anything about filters.

**Encrypt-on-save.** `WriteOptions::encryption` encrypts a rewrite at R6
(AES-256): every string and every stream (7.6.2), each under an
initialisation vector derived from the file key and object number so
identical plaintexts never encrypt identically; ciphertext strings are
written in hex form. The caller supplies 48 bytes of entropy — the 32-byte
file key and two 8-byte salts — because the engine links no random number
generator ([encryption](encryption.md)). The `/Encrypt` dictionary and the
cross-reference stream stay in the clear (7.6.1). A rewrite of an opened
encrypted document *without* the option decrypts on the way through and
drops `/Encrypt`, because carrying it forward over plaintext would make
every reader decrypt clear bytes into garbage.

**Linearization.** `WriteOptions::linearize` lays the file out per
Annex F: parameter dictionary first, a cross-reference table for page
one's objects ahead of them, the document-level objects, the primary hint
stream, the first page's objects as one consecutive run led by its page
object, the remaining pages, shared objects, and the main table at the
end (F.3.1). The hint stream carries the two required tables — page-offset
(F.4.1) and shared-object (F.4.2) — packed by column with per-run byte
padding; the optional generic tables are omitted, which F.4 permits. There
is no patching pass: the parameter dictionary's integers are written to a
fixed width, a classic table's length follows from its entry count, and
the hint tables' bit widths derive from counts and lengths, so every
offset is known before a byte is emitted. `/E` is the end of the
first-page section and `/T` the offset of the main table's first entry.
Hints are advisory and most viewers ignore them; the practical win is HTTP
range serving. And it **combines with encryption**: each object is
encrypted as it is serialised and the layout measured from the ciphertext,
so the padding AES-CBC adds is inside every offset the file declares. The
`/Encrypt` dictionary takes the reserved object number 3 — numbering it
above the ordinary objects would put it in the front table's freeing
range — and the parameter dictionary stays clear, sound because it
contains no strings, which a test asserts rather than assumes.

## API

Everything goes through the facade (ruling 11, [rulings](../rulings.md)):
`Document::editor()` hands back a `DocumentEditor`, and
`DocumentEditor::save(&WriteOptions) -> Vec<u8>` produces bytes — no I/O
traits in the core, because `wasm32-unknown-unknown` has no files.
`WriteOptions` carries `mode` (`WriteMode::Rewrite` or
`WriteMode::Incremental`), `linearize`, `version`, `object_streams`,
`compress`, `encryption` (`Option<Encryption>`) and `garbage_collect`;
the default is a plain uncompressed rewrite.

```rust
let doc = Document::open(bytes.clone())?;
let editor = doc.editor();
// ... edits ...
let saved = editor.save(&WriteOptions {
    mode: WriteMode::Incremental,
    ..WriteOptions::default()
});
assert!(saved.starts_with(&bytes)); // the prefix invariant, always
```

Encrypting or linearizing takes the same call with `encryption:
Some(Encryption { .. })` or `linearize: true` on a `Rewrite`
([encryption](encryption.md) shows the encrypted form).

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| `/ID` on encrypted saves | qpdf's `invalid /ID in trailer dictionary`, allowed through the oracle *by name* in `qpdf_oracle.rs` so nothing else hides behind it | 7.5.5 Table 15 requires `/ID` beside `/Encrypt`; all 48 entropy bytes are consumed and no derivation is decided | [ROADMAP](../ROADMAP.md) Tier 1 |
| Encrypting an incremental update | no typed variant: the incremental writer takes no cipher, so the combination cannot be requested | an update inherits the base file's encryption, which needs the original file key plumbed through | [ROADMAP](../ROADMAP.md) Tier 2 |
| Linearizing an incremental update | none — `linearize` is quietly dropped, documented on the field | an update appends to whatever layout the original had; claiming `/Linearized` over it would be a lie a reader believes | 7.5.6, Annex F |
| Linearizing a document with no catalog or no pages | none — `linearize` returns no layout and the ordinary rewrite is emitted | there is no first page to put first, and a file claiming `/Linearized` falsely is worse than an ordinary one | Annex F |
| `object_streams` under `linearize` | none — ignored when linearization succeeds | packing page one's objects into a container with everything else is the opposite of the layout's point | 7.5.7 |
| Re-compressing a stream that declares a `/Filter` | none — handed through untouched, asserted in both directions | the dictionary is the only signal the bytes are already encoded; wrapping them again yields a stream no reader can undo | [filters](filters.md) |
| Signing | `SignaturePlaceholder` exists as a record of where `/Contents` and `/ByteRange` would be patched; nothing produces or fills one | CMS production and everything cryptographic is unbuilt; the byte-identical prefix is the foundation it will stand on (12.8.1) | [ROADMAP](../ROADMAP.md) Tier 3 |

## Verified

In-module tests in `crates/tinker-pdf-cos/src/write.rs` pin the load-bearing
invariants by name: `an_incremental_update_preserves_the_original_bytes_exactly`,
`an_update_over_an_unchainable_document_writes_no_prev`,
`the_cross_reference_table_always_has_its_free_head` (every entry exactly
twenty bytes), `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
(both directions, so it cannot pass on a writer that stopped compressing),
and object-stream round-trips through the engine's own reader.

`crates/tinker-pdf-cos/tests/linearized.rs` checks byte offsets against the
bytes — a file can open perfectly with page one scattered through the
middle, which a round-trip cannot see. `crates/tinker-pdf-cos/tests/qpdf_oracle.rs`
then asks somebody else: `qpdf --check` and `qpdf --show-linearization`
run as subprocesses over one-, two- and six-page fixtures plus
a shared-resource fixture with a real part 8, encrypted and not, with
every hint-table value recomputed from the raw bytes; the writer's whole
object surface — graphics states, groups, gradients, patterns, a composite
font, links and outlines — is read back through `--show-object` and
`--json`, in plain, rewritten, linearized and compressed-object-stream
form. The `qpdf-linearization` CI job (`.github/workflows/ci.yml`)
installs qpdf, greps its own log for the `RAN` banner and goes **red when
qpdf is missing** — a skip may not impersonate a pass.

`crates/tinker-pdf-cos/tests/encrypt_on_save.rs` round-trips encrypted
output ([encryption](encryption.md)); `tests/page_operations.rs` and the
forms suites save through both modes. The three document byte-hashes in
`crates/tinker-pdf/tests/determinism.rs` pin writer byte-determinism —
object numbering, dictionary order and stream framing frozen as bytes
(ruling 4, [determinism](determinism.md)). All of it rides in the
workspace suite: 2 790 passed, 0 failed, 8 ignored (Windows x86_64,
August 2026) — [verification](../verification.md). What is *not* yet
verified is scale: the linearization check runs on fixtures, never over the
corpus. And under ruling 13 the arbiter itself changes: the strict validator
that replaces qpdf reads what qpdf read *and* the structures the tolerant
reader never consults — but it is this project's own reader, so the one
thing it cannot establish is that anybody else accepts these files
([ROADMAP](../ROADMAP.md) Tier 1, [verification](../verification.md)).
