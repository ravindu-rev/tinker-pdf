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

An update to an **encrypted** document is sealed with that document's own key
(7.6.2). It has to be: the appended objects sit under a trailer whose
`/Encrypt` still stands, so they must speak whatever the original file speaks —
RC4, AES-128 or AES-256, with the per-object key derivation of Algorithm 1
where the revision calls for it — and not a scheme this writer would rather
use. The key is the one authentication produced, carried on the document for
this single purpose; a document that was never authenticated has none and
writes in the clear as before, which is correct because it was never encrypted.
Initialisation vectors are derived from the file key and the object's identity
rather than drawn from a random source, so the same edit writes the same bytes
on every target (ruling 4) and `wasm32-unknown-unknown` needs no RNG. `/ID[0]`
survives the revision, as 14.4 requires.

**Compression.** `WriteOptions::compress` deflates streams through the
project's own encoder and records `/FlateDecode`. Two hard rules: a stream
that already declares a `/Filter` is handed through untouched — that is a
contract, not an optimisation, since pass-through image data depends on its
declared filter still describing its bytes — and a result no smaller than
the input is discarded, so an already-compressed image never grows. The
object-stream container is compressed whenever compression is on at all;
a cross-reference stream never is, because a reader finds it by offset
before it knows anything about filters.

**Cost is the object count, never the object numbering.** Both tables are
written in subsections — the classic one by its `first count` runs (7.5.4),
the stream by `/Index` (7.5.8.2 Table 17) — so a file holding four objects
writes four entries whatever it numbered them. That is not a size
optimisation. `pdfjs/test/pdfs/bug1980958.pdf` is 219 bytes, holds four
objects and numbers the last of them `2147483647`; a rewrite that walked
the range rather than the objects made two thousand million lookups and had
not returned after three minutes. The rewrite path enumerates the
cross-reference table's own entries for the same reason, and the reader has
been hardened against this shape since it was written
(`limits::MAX_XREF_SLOTS`).

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
| Linearizing an incremental update | none — `linearize` is quietly dropped, documented on the field | an update appends to whatever layout the original had; claiming `/Linearized` over it would be a lie a reader believes | 7.5.6, Annex F |
| Linearizing a document with no catalog or no pages | none — `linearize` returns no layout and the ordinary rewrite is emitted | there is no first page to put first, and a file claiming `/Linearized` falsely is worse than an ordinary one | Annex F |
| `object_streams` under `linearize` | none — ignored when linearization succeeds | packing page one's objects into a container with everything else is the opposite of the layout's point | 7.5.7 |
| Re-compressing a stream that declares a `/Filter` | none — handed through untouched, asserted in both directions | the dictionary is the only signal the bytes are already encoded; wrapping them again yields a stream no reader can undo | [filters](filters.md) |

## Verified

In-module tests in `crates/tinker-pdf-cos/src/write.rs` pin the load-bearing
invariants by name: `an_incremental_update_preserves_the_original_bytes_exactly`,
`an_update_over_an_unchainable_document_writes_no_prev`,
`the_cross_reference_table_always_has_its_free_head` (every entry exactly
twenty bytes), `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
(both directions, so it cannot pass on a writer that stopped compressing),
and object-stream round-trips through the engine's own reader.

The encrypted incremental update carries two of its own in
`crates/tinker-pdf-cos/tests/strict_validator.rs`, and they are a pair on
purpose: one asserts the appended string comes back through the reader, the
other asserts it is **not** sitting in the file's bytes in the clear. Only the
second would have caught what was there before — the writer passed no cipher at
all, so it appended plaintext under a standing `/Encrypt`, and the engine's own
reader decrypted it back either way. Taking the cipher away again fails exactly
those two of the file's eighty-four. The round trip of every method against its
own decryption is one level down, in
`crates/tinker-pdf-crypto/src/handler.rs`.

`crates/tinker-pdf-cos/tests/linearized.rs` checks byte offsets against the
bytes — a file can open perfectly with page one scattered through the
middle, which a round-trip cannot see.
`crates/tinker-pdf-cos/tests/strict_validator.rs` then reads the file again
with the leniency ladder **off**: eighty injections over the frame, the
cross-reference sections as the bytes spell them, the trailer's Table 15
entries, stream extents against `endstream`, and Annex F's hint tables
decoded by a reader that shares nothing with the writer and is held to the
file's own object extents. `tests/validated_output.rs` reads the writer's
whole object surface back out of the dictionaries — graphics states, groups,
gradients, patterns, a composite font down to its `/W` and `/ToUnicode`,
links and outlines with their counts and back-links — because every one of
those entries is one the *typed* readers supply a default for.

What that costs is stated where it is spent: this is this project's own
parser reading this project's own writer, so the one thing it cannot
establish is that anybody else accepts these files. The qpdf oracle that
used to make that claim is gone under ruling 13
([verification](../verification.md)).

`crates/tinker-pdf-cos/tests/encrypt_on_save.rs` round-trips encrypted
output ([encryption](encryption.md)); `tests/page_operations.rs` and the
forms suites save through both modes. The three document byte-hashes in
`crates/tinker-pdf/tests/determinism.rs` pin writer byte-determinism —
object numbering, dictionary order and stream framing frozen as bytes
(ruling 4, [determinism](determinism.md)). All of it rides in the
workspace suite: 4 403 passed, 0 failed, 43 ignored (Windows x86_64,
August 2026) — [verification](../verification.md).

**And it runs at scale.** Every corpus file this engine reads cleanly is
rewritten in memory and the rewrite is validated, which is 4 224
documents nobody here authored: all 4 224 of those rewrites carry no
structural defect at all, and `corpus/ratchet.json` holds the rate. That pass
is what found the three defects this writer had and no other check could see,
because this engine's own reader repairs all three in silence: no trailer
`/ID` on any path, a `/Prev` inherited from a source that no longer exists, a
cross-reference table with no free head whenever the lowest object number was
two or more, and a linearization parameter dictionary carried into a rewrite
that is not linearized.
