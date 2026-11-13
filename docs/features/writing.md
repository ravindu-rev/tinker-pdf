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
reachable object with no schema — and it is *off* by default, because a
rewrite serializes everything: redaction overwrites content streams in
place precisely so that an unreferenced object cannot survive with the
original text in it, and that guarantee is not inherited from a flag. The
cross-reference is a classic
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

*The section alone.* `write::update_section` writes what an incremental
update appends without the original: it is handed the original's length and
whether its last byte is a newline (`write::Prefix`, which
`CosDocument::prefix` reads from the last byte, so a streamed document asks
its source for one byte), and every offset it writes is that length plus its
own place. `incremental_update` is the original followed by that section,
joined once; `DocumentEditor::save_update` hands back the section of the save
`DocumentEditor::save` writes incrementally, so the document's bytes followed
by it are that save, byte for byte. A signature's placeholder is reserved in
the section with its offsets in the whole file, which is joined once because
the digest covers it.

An update to an **encrypted** document is sealed with that document's own key
(7.6.2). It has to be: the appended objects sit under a trailer whose
`/Encrypt` still stands, so they must speak whatever the original file speaks —
RC4, AES-128 or AES-256, with the per-object key derivation of Algorithm 1
where the revision calls for it — and not a scheme this writer would rather
use. The key is the one authentication produced, carried on the document for
this single purpose; a document that was never authenticated has none and
writes in the clear, which is correct because it was never encrypted.
Initialisation vectors are derived from the file key and the object's identity
rather than drawn from a random source, so the same edit writes the same bytes
on every target (ruling 4) and `wasm32-unknown-unknown` needs no RNG. `/ID[0]`
survives the revision, as 14.4 requires.

**Compression.** `WriteOptions::compress` deflates streams through the
project's own encoder and records `/FlateDecode`. Two hard rules: a stream
that already declares a `/Filter` is handed through untouched — that is a
contract, not an optimisation, since pass-through image data depends on its
declared filter still describing its bytes — and a result no smaller than
the input is discarded, so an already-compressed image never grows. A
`/Type /Metadata` stream is never compressed at all: a packet scanner reads
XMP from the raw bytes without decoding PDF filters, and ISO 19005 forbids a
filter on one, so `compress` does not reach it — the editor's
`set_xmp_metadata` relies on that ([editing](editing.md)). The
object-stream container is compressed whenever compression is on at all;
a cross-reference stream never is, because a reader finds it by offset
before it knows anything about filters.

**Compression on the pool, when asked.** `WriteOptions::parallelism` is
`Parallelism::Serial` by default, and each object is compressed and written in
turn. With `Threads(n)` and `compress` on, the writer compresses the streams
an output carries on the engine's own pool (`tinker-pdf-pool`) a window at a
time — eight streams a worker, each deflated alone into a pre-sized slot of
its own — and then serialises the window's objects in their order as the
serial writer does, so a rewrite holds one window's compressed streams beside
the file and no more. A rewrite, an incremental update and the object-stream
container, built before the loose objects are written and compressed beside
them, take that path; the linearized layout, which holds every serialised
object to measure its offsets, compresses all of its streams before it lays
the file out. Encryption stays after compression and in object order, so
every IV and every ciphertext, like every other byte, is the serial writer's
at any thread count; a write that compresses nothing is the serial one.
`crates/tinker-pdf-cos/tests/parallel_save.rs`'s
`saves_on_2_3_4_and_7_threads_are_the_serial_bytes` holds a rewrite, one
packing objects, a linearized one — each encrypted and not — an incremental
update to a plain file and to one sealed under its own key, and a rewrite that
collects garbage and merges streams to that, and `write.rs`'s
`compression_on_four_threads_runs_on_no_caller_thread` counts every stream of
a four-thread write made ready on a worker.

**Cost is the object count, never the object numbering.** Both tables are
written in subsections — the classic one by its `first count` runs (7.5.4),
the stream by `/Index` (7.5.8.2 Table 17) — so a file holding four objects
writes four entries whatever it numbered them. That is not a size
optimisation: a file of four objects may number the last of them
`2147483647`, and a rewrite that walked the range rather than the objects
would make two thousand million lookups. The rewrite path enumerates the
cross-reference table's own entries for the same reason, and the reader
bounds this shape (`limits::MAX_XREF_SLOTS`).

**Stream deduplication.** `WriteOptions::deduplicate_streams` merges
identical streams into one object on a rewrite, after garbage collection, and
**identical means identical**: equal dictionaries — every key and value in
order, `/Length` aside since the writer computes it — and equal content,
compared byte for byte. A hash only puts streams in buckets worth comparing;
it never decides a merge, because a wrong merge is not a bigger file but one
font drawn with another's program and nothing in the output saying so.
**Equal encodings merge undecoded**: streams are bucketed by their
dictionary's bytes, which carry the filter chain, and by the length and the
XXH64 of their stored bytes, and two whose stored bytes are equal are one
stream without a decode — a face imported with every page of a merge is
merged without running its chain once. **Differing encodings under one
dictionary are compared decoded**, so two encodings of one content merge:
ruling 23's answer to H-137 keeps those merges, at the price of a decode of
each stream whose dictionary another shares under a different encoding,
every page's content stream of a merge among them. Each such stream is
decoded at most once a save, counted on the save's `Stats`, and its decoded
bytes kept until the pass ends; a byte-identical copy merges without a
decode, and no other stream is decoded. A decode that is not exact — a filter
this build cannot run, an image codec the chain stops at, a decode that
warned or hit the output cap — compares the stored bytes under a
different tag instead, so a damaged stream that decodes to the same prefix as
another is never taken for it. Every reference to a duplicate, the trailer's
included, is pointed at the survivor (the lowest-numbered), and the pass
repeats to a fixed point, since merging two images can make the two forms that
drew them equal. Only streams are candidates, so nothing whose object number
is its identity — a page, an annotation, an optional content group, a
structure element, a signature dictionary — can be merged; among streams a
cross-reference or object stream and one an `/OBJR` names (14.7.5.3) are held
apart. Off by default, because it decodes each stream whose dictionary
another shares under a different encoding to compare it, and a file whose
streams all differ pays that for nothing; ignored on an incremental update,
which appends and must not rewrite what a signature's revision covers.
`DocumentEditor::import_page` copies a shared resource per import, and this
pass merges the copies **only where
every object the copies' dictionaries name is a stream or is direct**. A copy
whose dictionary names an indirect object that is not a stream — an indirect
`/ColorSpace`, `/DecodeParms` or `[/ICCBased …]` array — has that object
copied afresh with it per import, the two dictionaries name two different
objects, and no pass merges a non-stream to make them equal: two imports of
such an image stay two images, where the same image with its `/ColorSpace`
written directly becomes one.

**Encrypt-on-save.** `WriteOptions::encryption` encrypts a rewrite at R6
(AES-256): every string and every stream (7.6.2), each under an
initialisation vector derived from the file key and object number so
identical plaintexts never encrypt identically; ciphertext strings are
written in hex form. The caller supplies 48 bytes of entropy — the 32-byte
file key and two 8-byte salts — because the engine links no random number
generator ([encryption](encryption.md)). The key is expanded once for the
save, and the streams are sealed a window at a time — eight streams written
serially, the pool's window on threads — side by side through the AES core, a
stream a lane; each stream's bytes are what sealing it alone writes, so the
file is the same bytes whichever way it is written. An update to a file
sealed under `/AESV3` seals its streams the same way under that file's key,
and one under RC4 or `/AESV2` an object at a time. The `/Encrypt` dictionary
and the cross-reference stream stay in the clear (7.6.1). A rewrite of an opened
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
range serving.

**The numbering is F.3.1's and not this writer's convenience.** Parts 7, 8
and 9 — the remaining pages, the shared objects, everything no page
reaches — are numbered sequentially starting at 1, which is what makes
Table F.4 item 1's *the first object of the second page shall have an
object number of 1* true by construction. Everything the head of the file
must resolve without the tail is numbered after them, as one contiguous
range: part 2, the `/Encrypt` dictionary, part 4, part 6, and last of all
the primary hint stream, which F.3.6 gives *the last object number in the
file* whatever its physical position. Contiguous because the first-page
cross-reference section covers all of it with a single subsection, and
7.5.4 makes a subsection a range — so that section starts at the parameter
dictionary's own number, carries no object-0 entry and marks nothing free.
The file's free head lives in the main section, which is also where Table
F.1 item 6's `/T` points.

It **combines with encryption**: each object is encrypted as it is
serialised and the layout measured from the ciphertext, so the padding
AES-CBC adds is inside every offset the file declares. The `/Encrypt`
dictionary takes the number immediately after the parameter dictionary's,
which keeps it inside the head group — a number outside it would leave the
front trailer's `/Encrypt N 0 R` resolvable only through `/Prev`, and that
is a tail read before a streaming reader can decrypt anything at all. The
parameter dictionary stays clear, sound because it contains no strings,
which a test asserts rather than assumes.

## API

Everything goes through the facade (ruling 11, [rulings](../rulings.md)):
`Document::editor()` hands back a `DocumentEditor`, and
`DocumentEditor::save(&WriteOptions) -> Vec<u8>` produces bytes — no I/O
traits in the core, because `wasm32-unknown-unknown` has no files.
`WriteOptions` carries `mode` (`WriteMode::Rewrite` or
`WriteMode::Incremental`), `linearize`, `version`, `object_streams`,
`compress`, `encryption` (`Option<Encryption>`), `garbage_collect`,
`deduplicate_streams` and `parallelism` (`Parallelism::Serial`);
the default is a plain uncompressed rewrite on the calling thread.

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

**Two doors, and the second one subsets.** `DocumentEditor::save` above is
`tinker-pdf-cos`'s, and it writes every embedded font program through exactly
as it arrived. `tinker_pdf::write::save(&mut editor, &SaveOptions) -> Saved` is
the facade's, and it runs `subset::apply` first **unless asked not to** —
cutting every embedded program down to the glyphs the document still draws
([fonts](fonts.md), [editing](editing.md)). `SaveOptions` is `WriteOptions`
verbatim plus `fonts: FontPolicy`, whose default is `FontPolicy::Subset` —
`FontPolicy::Keep` is the other value and there is no third — `images`, below,
`stats`, which counts the save's work, and `parallelism`, the threads the
font pass's walk may spend, handed to the writer in place of
`WriteOptions::parallelism` where it asks for any.

```rust
use tinker_pdf::write::{save, SaveOptions};

let mut editor = doc.editor();
// ... edits, a redaction, whatever ...
let saved = save(&mut editor, &SaveOptions::default());
if !saved.fonts.removed() {
    // Some program went through whole; the report names each and why.
    for whole in saved.fonts.report().into_iter().flat_map(|r| &r.untouched) {
        eprintln!("{whole}");
    }
}
std::fs::write("out.pdf", &saved.bytes)?;
```

**Why the switch is not on `WriteOptions`,** which is the obvious place.
`WriteOptions` lives in `tinker-pdf-cos`; the subsetting pass lives in the
facade, because it is driven by the interpreter's own glyph walk — pages, form
XObjects at any depth, Type 3 glyph procedures, every state of every `/AP`.
`tinker-pdf-cos` depends on neither the interpreter nor the facade and
`cargo xtask dag` holds that direction, so a `WriteOptions::subset_fonts: bool`
would be a flag the crate carrying it cannot act on. Every caller who writes
through `DocumentEditor::save` — the door this page documents — would set it,
get no subsetting, and be told nothing; and the caller who reaches for that
flag is by definition the caller redacting something. A silent flag on the
disclosure path is worse than no flag, so there is none: `WriteOptions` has
nine fields, eight about bytes on disk and one about the threads the writer
spends making them, every one acted on where it is carried, and its own
documentation says why there is no font switch among them. Moving the
subsetting down into `tinker-pdf-cos`
instead would mean moving the interpreter down or writing a second glyph
resolver there, and a second answer to "what does this code decode to" is a
second engine.

**`Saved` states what happened**, because the caller has no other way to find
out. It is `#[must_use]`, and `Saved::fonts` is a `SubsetOutcome` with three
values rather than a `bool`: `Kept` (nothing was asked for), `Cut(report)` (the
pass ran and this was a rewrite) and `CutButTheOriginalsRemain(report)` — a
subset taken before an **incremental** save, which appends, so every original
program is still in the prefix at its original offset. That combination is not
refused, because a caller may want a viewer to use the smaller program while a
signature over the prefix survives; it is named, and
`SubsetOutcome::removed()` is false for it.

**`removed()` is a question about the file, not about whether the pass ran.**
It is true only for a `Cut` whose report leaves *nothing* whole, because a
program written through entire carries every outline it had — including the
ones a redaction has just removed the text of. Every `UntouchedReason` is a
good reason to keep a program and none of them is a reason to tell a
caller the disclosure is gone, so a non-empty `untouched` is `false` here
whatever is in it — and so is a non-empty `type3_untouched`, a Type 3 font
whose glyph procedures went through whole: the pass empties the procedures
nothing shown runs ([editing](editing.md)'s "Type 3 fonts"), and one it
leaves whole is in the file. `false` is the ordinary answer rather than a
failure: it is an instruction to read the report, which names each one
(ruling 10).
`DocumentEditor::save_signed` has no facade door of its own for the same
reason: signing is incremental by definition, so subsetting into it could never
remove anything.

**The font pass's walk on the pool, when asked.** `SaveOptions::parallelism`
is `Parallelism::Serial` by default, and the walk reads each page and its
annotation appearances in turn on the calling thread. With `Threads(n)` each
page is walked on a thread of the engine's own pool (`tinker-pdf-pool`) into
a usage of its own — the glyphs each font shows, the fonts each scope names,
what cannot be bounded and why — and the usages are folded in page order,
never in the order the pages finished: the walk keeps the first reason a font
was refused for and names a program a directly written font embeds after the
first such font in page order, so a fold in completion order would change the
report. The pages read their streams through the pass's one set of decoded
streams, which is single-flight (`tinker_pdf_cos::DecodedStreams` over
`tinker_pdf_cos::Flights`): the first page to read a stream decodes it and a
page asking for it meanwhile waits for that decode, so each stream is still
decoded once. The programs cut, the report and every byte saved are the
serial save's at any thread count, and a counted save asked for threads counts
where each page was walked (`Stats::pool_items_on_workers` and
`pool_items_on_caller`, [rendering](rendering.md)); a save asked for none
counts no part. `parallelism.rs`'s
`a_subset_walk_on_2_3_4_and_7_threads_cuts_what_the_serial_walk_cuts` holds a
document of shared fonts, a redacted page, a tiling cell, an appearance and
two fonts written directly into resource dictionaries to that, and
`a_subset_walk_on_four_threads_walks_no_page_on_the_caller` reads where the
pages ran from the save's stats.

**Images, on the same door, off by default.** `SaveOptions::images` is an
`ImagePolicy`, `Keep` by default — the pass is not entered and every image
stream is written through as the file stored it. `Recode(ImageRecoding)`
names a coding per image kind and, optionally, a resolution
(`ImageRecoding::new(continuous, bilevel).with_max_ppi(ppi)`):

- a **continuous** image (more than one bit or one component) as
  `ContinuousCodec::Keep` (as stored, unless resampled — then deflated),
  `Flate`, or `Jpeg(JpegTables)` — baseline T.81 with **the caller's**
  luminance and chrominance tables in natural order and 4:4:4 unless
  `subsampled`, eight bits and one or three components only;
- a **bilevel** image (one bit, one component, `/ImageMask` included) as
  `BilevelCodec::Keep`, `Flate`, `CcittG4` (`/K -1`, `BlackIs1` false, so a
  sample's bits go through as they are) or `Jbig2Generic` (D.3's embedded
  organisation: a page information segment, one immediate lossless generic
  region at template 0 with TPGDON and the nominal AT pixels, an end of
  page — the assembly the filter crate deliberately leaves to its caller —
  with the samples inverted, because T.88 codes 1 for black);
- `max_ppi`: an **integer box filter**, one whole factor per axis, chosen so
  that no placement of the image is finer than that on either axis. Placements
  come from interpreting every page, every form XObject it draws at any depth
  (a form that draws itself is not re-entered) and every state of every
  annotation's `/AP /N`, placed by 12.5.5's algorithm: an axis drawn `L`
  points long shows its samples over `L / 72` inches, and the factor is
  `ceil(finest ppi / max_ppi)`. Each output sample is its block's mean,
  rounded half up, a partial block at the right or bottom edge the mean of
  what it holds. An image's `/SMask` or `/Mask` is placed wherever the image
  is.

An image that is another's soft or stencil mask is coded losslessly whatever
the continuous codec, since a quantiser's ringing on an edge of alpha is a
halo. A recoding is kept only if it is **smaller** than the stream stored.
`Saved::images` is an `ImageOutcome` with `SubsetOutcome`'s three shapes —
`Kept`, `Recoded(ImageReport)` and `RecodedButTheOriginalsRemain(report)` for an
appended save, whose prefix keeps every stored image — and the report names
every image by object reference: `recoded` (coding, size before and after,
bytes before and after, and `resolution_kept` when a resample was asked and
not done) and `untouched` with an `UntouchedImageReason`.

**Why the switch is not `WriteOptions::images`:** font subsetting's reason,
and one of its own. A
resolution is a property of where an image is *drawn* — one object placed as
a thumbnail and as a page has two — and only the interpreter's walk can say
where, which `tinker-pdf-cos` is below. And `WriteOptions` describes bytes on
disk under a contract its writer keeps, that it never re-encodes image bytes;
this pass runs on the editor before that writer and the writer still
re-encodes nothing.

**A `/Decode` array never blocks it.** Every operation here preserves one: a
lossless coding keeps the samples it applies to, and 8.9.5.2's map is affine
per component, so a block mean of samples is the block mean of what they
decode to and a quantiser's error is scaled by `(Dmax - Dmin) / 255`, never
amplified. Inline images (8.9.7) live in content streams and are not
touched.

From the command line, every `tpdf` command that writes takes the policy:
`--images keep|flate|jpeg` for the continuous kind, `--bilevel
keep|flate|g4|jbig2` for the one-bit kind, and `--max-ppi N`, none of them
meaning `ImagePolicy::Keep`. `--images jpeg` needs `--jpeg-tables FILE`, the
two tables as 128 bytes in natural order, because this door takes the
caller's tables and the CLI chooses none (ruling 11); the report is printed an
image a line.

**Counted work, on the same door.** `SaveOptions::stats` (off) asks the save
to count what it does, and `Saved::stats` hands it back: an `Option<Stats>`
that is `None` unless asked ([rendering](rendering.md) names the counters). A
save counts its passes: the font pass's walk of the page tree, the fonts each
page's scope reads, each glyph's font-dictionary lookup, each glyph it selects
in the program of a font that is an object and each form's scope; the image
pass's walk, which it makes only to find placements for `max_ppi`; and every
stream they decode — the content each walk reads, and once the font pass each
stream its scopes read, a font's program and CMaps at the first page that
reads them and a form, a procedure or a cell however many pages draw it, the
cut reading each program from the walk's decode ([editing](editing.md)), and
each image's samples the image pass weighs — and each stream the serializer
decodes to compare when a rewrite deduplicates streams; and a save that
encrypts counts the passes of the AES core its streams were sealed side by
side in (`stream_seal_passes`), a rewrite's and an update's under an
inherited `/AESV3` key. A stream the editor
holds unfiltered, an import's copy, is handed back as it is and runs no chain.
`DocumentEditor::count_work()` makes an editor hold one tally from then until
it is dropped, which both passes count into whether or not the save was asked,
and a save asked on that editor reports all of it — so a merge, which is pages
imported and then saved, reads its stats from the save. An import counts its
walk of its source's page tree, the editor its walks of its own until an edit
first sets its page order, and a rolled-back transaction keeps what was
counted. A counted save writes the same bytes and reports the same passes as
an uncounted one.

**Appended and read back, on a third door.**
`tinker_pdf::write::append(&document, &editor, &AppendOptions) ->
Result<Appended, AppendError>` writes an editor's edits as the update section
an incremental save appends (`DocumentEditor::save_update`) and reopens
`document`'s object layer over it (`CosDocument::reopen`,
[opening](opening.md)), so a caller holds the bytes to append to its file and
the document they make without opening the file again. `Appended` carries the
section (`update`, an `Arc<[u8]>` the reopened document reads where it is:
`document`'s bytes followed by it are `DocumentEditor::save` under
`WriteMode::Incremental`, byte for byte), the reopened `Document`, which keeps
the font provider, the archive report and the security `document` had, and
its cache budget (`Document::cache_limits`) in a cache of its own, since an
entry `document` kept may be of an object the update replaces, the
`FullOpen` reason when the reopen read the whole file again, the numbers of
the objects the update writes (`DocumentEditor::changed_numbers`), and the
reopen's `Stats` when `AppendOptions::stats` asked — on an editor
`count_work` made count, the editor's whole tally, as a save hands it back.
`AppendOptions` holds `compress` and `parallelism`, the two `WriteOptions`
fields an update reads, and `stats`. An editor whose object layer is not
`document`'s own — the same bytes opened again included — is
`AppendError::NotThisDocument`, and a reopen that must read the whole file and
cannot fetch it is `AppendError::Reopen`, naming the range. The door runs
neither pass `save` runs, since an update leaves every original program and
image in the file (7.5.6). A reopen that reads the section alone reads its
rows — one for each changed object and one for object zero's free head — and
parses no object, where a full open reads every row the file has.
`Appended::patch` re-renders a page of the reopened document over the bitmap
of the page before the edit, by the rectangle the edit changed
([rendering](rendering.md)).

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| Linearizing an incremental update | none — `linearize` is quietly dropped, documented on the field | an update appends to whatever layout the original had; claiming `/Linearized` over it would be a lie a reader believes | 7.5.6, Annex F |
| Linearizing a document with no catalog or no pages | none — `linearize` returns no layout and the ordinary rewrite is emitted | there is no first page to put first, and a file claiming `/Linearized` falsely is worse than an ordinary one | Annex F |
| `object_streams` under `linearize` | none — ignored when linearization succeeds | packing page one's objects into a container with everything else is the opposite of the layout's point | 7.5.7 |
| Re-compressing a stream that declares a `/Filter` | none — handed through untouched, asserted in both directions | the dictionary is the only signal the bytes are already encoded; wrapping them again yields a stream no reader can undo | [filters](filters.md) |
| Deduplicating on an incremental update | none — `deduplicate_streams` is ignored there, documented on the field (`an_incremental_update_merges_nothing`) | an update appends; merging would rewrite objects an earlier revision, and a signature over it, covers | 7.5.6 |
| Merging two streams whose digests agree and whose bytes or dictionaries do not | never; the digest only chooses what to compare (`a_colliding_digest_never_merges_different_bytes`, `equal_bytes_under_different_decode_parameters_stay_apart`) | a wrong merge silently swaps one font's program for another's | — |
| Comparing a stream by decoded content when the decode warned | its stored bytes are compared instead (`two_damaged_streams_that_decode_alike_stay_apart`) | a partial decode of two different streams can be equal | [filters](filters.md) |
| Merging a cross-reference or object stream, or a stream an `/OBJR` names | held apart (`a_stream_with_identity_is_never_merged`) | the first two describe the file they came from; the third is a structure element's claim on that object, and two claims must stay two | 14.7.5.3 |
| Compressing a `/Type /Metadata` stream | none — written unfiltered whatever `compress` says (`a_caller_supplied_packet_is_written_verbatim_and_uncompressed`) | an XMP packet is read from the raw bytes by tools that do not decode PDF, and ISO 19005 forbids a filter on one | 14.3.2 |
| A font-subsetting switch on `WriteOptions` | none, and deliberately — the field does not exist and `WriteOptions`' own doc comment says why | the pass is driven by the interpreter, which `tinker-pdf-cos` is below; a flag the crate carrying it cannot act on would read as done and do nothing, on the one path where that is a disclosure. The switch is `tinker_pdf::write::SaveOptions::fonts` | [fonts](fonts.md) |
| Subsetting that *removes* anything on an incremental save | `SubsetOutcome::CutButTheOriginalsRemain`, and `removed()` is false | 7.5.6: the output starts with the original bytes, the original font programs among them. The pass still runs, because the smaller programs are what a reader resolves — but nothing has left the file | 7.5.6 |
| An image switch on `WriteOptions` | none, and deliberately; the switch is `SaveOptions::images` | a resolution is where an image is drawn, which only the interpreter's walk knows, and `WriteOptions`' writer promises never to re-encode image bytes | [ROADMAP](../ROADMAP.md) |
| Recoding an image stored through an image codec (`DCTDecode`, `JPXDecode`, `CCITTFaxDecode`, `JBIG2Decode`), `Crypt`, an unknown filter or an external `/F` | `UntouchedImageReason::Filter { name }` | the pass reads samples through the general filters only; a codec's samples *in their own colour space* are the read side's images row, and a second decoder here would be a second answer to what those bytes mean | [ROADMAP](../ROADMAP.md) |
| A lossy coding or a resample of an image with a colour-key `/Mask`; a resample of a `/Matte` soft mask or its image, an `/Indexed` image, one not at eight bits, a bilevel one, or one no walked stream draws | `UntouchedImageReason::ColourKeyMask`, `Matte`, `Indexed`, `Depth { bits }`, `Bilevel`, `Unplaced` — on `untouched`, or on `Recoded::resolution_kept` when a lossless coding still ran | each would change what the page shows: masked pixels decided by exact equality, a matte relation that needs equal dimensions, palette indices, grey from black and white, or a resolution with nothing to measure it by (a tiling pattern's cell, a Type 3 glyph and a soft mask's group are not walked) | 8.9.6.4, 11.6.5.3 |
| JPEG for four components or an image that is not eight bits | `UntouchedImageReason::Components { count }`, `Depth { bits }` | baseline T.81 as `jpeg_encode` writes it: one or three components, eight bits | [filters](filters.md) |
| A recoding no smaller than what the file stored | `UntouchedImageReason::NotSmaller` | the stored stream is both smaller and the producer's own | — |
| Recoding into an incremental save | `ImageOutcome::RecodedButTheOriginalsRemain` | 7.5.6: every stored image stays in the prefix, so the file grows; the pass still runs, because the recoded streams are what a reader draws | 7.5.6 |

## Verified

In-module tests in `crates/tinker-pdf-cos/src/write.rs` pin the load-bearing
invariants by name: `an_incremental_update_preserves_the_original_bytes_exactly`,
`an_update_over_an_unchainable_document_writes_no_prev`,
`edit/tests.rs`'s `an_update_section_is_written_without_copying_the_original`
(the section copies none of the original, the whole save copies it once, and
the two are one file whose update reopens at `Trust`),
`the_cross_reference_table_always_has_its_free_head` (every entry exactly
twenty bytes), `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
(both directions, so it cannot pass on a writer that stopped compressing),
and object-stream round-trips through the engine's own reader.

`crates/tinker-pdf/tests/incremental_reopen.rs` holds `write::append` on the
public surface: `an_annotation_edit_saved_and_reopened_parses_only_what_changed`
counts a one-annotation edit of a generated 1 000-page file — its rows the
changed object's and the free head's, no object parsed, every count the one
the same edit of a 10-page file gives — and renders and extracts every page of
the reopened document as a fresh open of the joined bytes does; the rest hold
the update to the incremental save's section, compressed and on threads, the
font provider, archive report and security carried, the cache budget kept in
a cache of the reopened document's own, an update to an update, an
untouched editor, an editor of another document refused, every `FullOpen`
reason the facade can meet, and a source that cannot supply the file refused
with the range it missed.

The encrypted incremental update has two tests of its own in
`crates/tinker-pdf-cos/tests/strict_validator.rs`, and they are a pair on
purpose: one asserts the appended string comes back through the reader, the
other asserts it is **not** sitting in the file's bytes in the clear. The
round trip of every method against its own decryption is one level down, in
`crates/tinker-pdf-crypto/src/handler.rs`.

`crates/tinker-pdf-cos/tests/linearized.rs` checks byte offsets against the
bytes — a file can open perfectly with page one scattered through the
middle, which a round-trip cannot see. `tests/linearized_numbering.rs` checks
the *numbers* the same way: it derives every page's first object by Annex F's
own arithmetic — `/O`, then object 1, then each page's declared count added to
the one before — and asks the document whether the number it produced is the
page it claims to be. Over files this project did not write, that sweep is
`validate/hints.rs`'s `annex_f_numbering`, where the hint decoder lives.
`crates/tinker-pdf-cos/tests/strict_validator.rs` then reads the file again
with the leniency ladder **off**: the frame, the
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
establish is that anybody else accepts these files.

The facade's door has tests of its own in
`crates/tinker-pdf/src/write.rs`. `the_default_save_cuts_the_programs_down`
and `keeping_is_asked_for_by_name_and_reported_as_kept` are the two policies;
`the_two_doors_differ_and_the_difference_is_pinned` holds them apart — the
same editor written through `DocumentEditor::save` carries the whole vendored
face and written through `write::save` does not, so the difference between the
two doors is a tested fact rather than a sentence on this page — and
`an_incremental_save_says_the_originals_are_still_in_the_file` asserts the
prefix invariant and the outcome variant together.

Three are about disclosure, which is what the door is for.
`a_redaction_saved_the_ordinary_way_does_not_carry_the_removed_glyphs` redacts
a line and then saves with `SaveOptions::default()` — no policy named, nothing
asked for — and reads the removed letters' absence out of the output program's
own `loca` and `glyf`;
`keeping_the_programs_leaves_the_redacted_letters_outlines_in_the_file` is the
counterfactual under `FontPolicy::Keep`, so "the default is what changes this"
is measured rather than asserted; and
`a_program_left_whole_means_the_disclosure_is_not_out_of_the_file` pins
`removed()` against a report with one `FieldResource` entry in it, which is
the case where "the pass ran" and "the file is clean" come apart.

Stream deduplication is held at two levels. `crates/tinker-pdf/tests/stream_dedup.rs`
embeds the vendored Liberation Serif twice under two font dictionaries and
shows a deduplicating rewrite carries it once — both descriptors name one
`/FontFile2`, the file shrinks by the face — and **renders the page byte for
byte as before**; the output passes the strict validator. The two fonts alone
could not make that render fail on a wrong merge, since they embed one program
and draw the same glyphs whichever each names, so the same page draws two
images under equal dictionaries whose samples differ: a merge that did not
compare their bytes would draw one image twice, and the test asserts they stay
two objects besides. The render guards that class of swap — a merge that
looked at too little — and a swap behind a colliding hash rests on
`dedup.rs`'s injected-hash test below, since no colliding XXH64 is built
here. Beside it: equal bytes under different
`/DecodeParms` stay two, a zlib stream of stored blocks and the encoder's
output of the same content merge (the comparison is on decoded bytes), two
damaged streams that decode to the same prefix stay two, an incremental
update merges nothing, and an image `import_page` copied twice merges to one
when its dictionary names nothing indirect and stays two when its
`/ColorSpace` is a reference (`an_imported_image_merges_unless_its_dictionary_names_a_copied_non_stream`,
the limit the paragraph on `import_page` above states).
`crates/tinker-pdf-cos/src/dedup.rs`'s own tests cover
what a real hash is not made to do here: an injected hash that collides on
everything, under which only true duplicates merge; four copies of streams
whose dictionaries differ, which merge with nothing decoded
(`byte_identical_encodings_merge_undecoded`); the fixed point through
two forms naming two copies of one image; the trailer redirected; and the
`/OBJR` and cross-reference holds. What a merge's deduplication decodes is
the counted guard A6 in `crates/tinker-pdf/tests/guards.rs`
(`a6_merge_deduplication_decodes_no_copy_and_each_differing_encoding_once`):
four documents of a hundred pages merged and saved decode none of the copies
of the face their imports make, and each encoding that differs under the
dictionary the pages' contents share with the face once, by the save's own
count; the face's copies leave one program, and every page keeps a content
stream of its own.

The image pass is `crates/tinker-pdf/tests/image_recode.rs`, and every
expected answer there is the generator's input or arithmetic done in the file.
`the_default_save_leaves_every_image_as_stored` holds a default save equal,
byte for byte, to the font pass and the serializer alone, with every image
stream as stored. `lossless_codings_give_back_exactly_the_samples_written`
saves eight-bit grey and RGB, four-bit grey and an indexed image deflated, and
a one-bit grey image and a stencil mask under each of deflate, G4 and JBIG2,
and reads back exactly the samples the test wrote and a page drawn to the same
pixels. `downsampling_is_an_exact_box_filter_at_the_finest_placement` places
one image twice and another only inside a form whose `/Matrix` halves it, and
holds the factors, the dimensions and every block mean against a box filter
written in the test. `jpeg_decodes_within_the_bound_the_callers_tables_give`
codes a textured grey and RGB image with the caller's tables, finds exactly
those tables in the DQT segments, and holds every sample within a bound
computed from them: T.81 A.3.3's basis applied to half of each quantiser,
widened by what a 1/16384 basis and a flooring integer IDCT can add, and for
RGB carried through T.871's inverse with the encoder's rounding and the
decoder's truncation. `what_cannot_be_kept_is_left_whole_by_name` reaches
every `UntouchedImageReason` a page can, and
`a_hostile_document_never_panics_the_pass` saves mutated documents under three
policies.

`crates/tinker-pdf-cos/tests/encrypt_on_save.rs` round-trips encrypted
output ([encryption](encryption.md)); `tests/page_operations.rs` and the
forms suites save through both modes. The document byte-hashes in
`crates/tinker-pdf/tests/determinism.rs` pin writer byte-determinism —
object numbering, dictionary order and stream framing frozen as bytes
(ruling 4, [determinism](determinism.md)).

**And it runs at scale.** Every corpus file this engine reads cleanly is
rewritten in memory and the rewrite is validated, and `corpus/ratchet.json`
holds the rate.
