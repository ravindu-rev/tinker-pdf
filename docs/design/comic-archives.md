# Comic archives: the containers that are not ZIP

A `.cbz` is a ZIP. A `.cbt` is a tar, a `.cb7` is a 7z and a `.cbr` is a RAR,
each recognised at a fixed offset ([features/cbz.md](../features/cbz.md)).
This is the design that reads the three that are not ZIP as pages.

## Scope

**One leaf crate, `tinker-pdf-archive`, holding three container readers and
the compression they need.** Bytes in, names and byte ranges out, no COS
types, no idea what an entry is *for* (ruling 8). The facade's `cbz.rs` keeps
every decision about what a page is.

- **tar** — POSIX 1003.1 `ustar`, GNU's `L` long-name pseudo-entry, PAX's `x`
  and `g` extended headers. No compression at all; the entry walk is the whole
  format.
- **7z** — the signature header, the property-id header grammar, packed
  streams, folders and coders, including `kEncodedHeader`. Coders: Copy, LZMA,
  LZMA2, Deflate, bzip2, PPMd, BCJ and BCJ2.
- **LZMA and LZMA2** — the range decoder, the probability model and the LZ77
  window, hand-rolled. Both front ends, because 7-Zip writes a `.7z`'s own
  header with plain LZMA and its file data with LZMA2.
- **RAR 5.0** — the signature, the `vint`, the header chain, file records and
  their extra areas. Compression method 0 (store).

Everything is hand-rolled. `CONTRIBUTING.md` rule 1, and `deny.toml` names
`lzma-rs`, `xz2`, `liblzma`, `sevenz-rust`, `sevenz-rust2`, `unrar`,
`compress-tools` and `libarchive` so that reaching for one is a build failure
rather than a judgement call. The RAR names carry a second reason on top of
rule 1: there is one RAR implementation in the world, RARLAB's `unrar`, every
binding wraps it, and its licence forbids using the source to write a
compatible compressor — a restriction this repository's dual MIT/Apache-2.0
grant cannot pass on, and one the licence allowlist would not have caught,
because a `-sys` crate's manifest says whatever its author typed.

## Non-goals

- **Encryption**, in any of the three. AES-256 is offered by both 7z and RAR 5
  and is refused as `Encrypted`, as in `tinker-pdf-zip`, rather than as an
  unreadable stream three layers down. A password seam is
  [ROADMAP](../ROADMAP.md) row SE-14.
- **Writing** any of these formats. Every reader here is a reader; writers are
  [ROADMAP](../ROADMAP.md) rows FO-11 and FO-12c.
- **Multi-volume sets.** The fragment that happens to be present is not the
  archive, in RAR and in tar's type `M` alike. A whole set is
  [ROADMAP](../ROADMAP.md) row SE-14; a lone fragment is not the archive.
- **Sparse files.** Refused rather than returned with their holes closed up,
  which is what a reader that ignored the flag would hand back: bytes in the
  wrong places are a picture that decodes to the wrong thing. Reading them with
  their holes is [ROADMAP](../ROADMAP.md) row FI-25.
- **7z coders beyond the ones listed**, and **folders whose coder graph is not
  a tree**. BCJ2 takes four input streams, so the folder walk is a tree walk
  rather than a chain, and what is refused is a graph with no answer — a
  cycle, a stream fed twice or by nothing. The coders refused by name are
  [ROADMAP](../ROADMAP.md) row FI-25.
- **RAR 5 compression methods 1–5**, and **RAR 4 entirely.** Both have a
  section of their own below, because both are decisions rather than
  omissions, and the first of them is a *permanent* non-goal rather than a
  staged one.
- **A trait over the three readers.** The next section is why.

## Design

### One crate, three container modules, and no trait over them

What tar, 7z and RAR have in common is a *negative* — they are the archive
containers that are not ZIP — which is weaker than `tinker-pdf-filters`'
"decoders" and is honestly weaker. It is still real, and it buys one node in
the crate graph and one edge instead of three of each.

The part that is not negotiable is the second half: **no trait unifies them**,
and the reason is a measurement rather than a preference.

`tinker_pdf_zip::Archive::read` returns a `Cow` and hands a stored entry back
**borrowed**, copied nowhere, and that crate's own test suite pins it with the
reason attached: *the moment this copies, a 3.6 GB peak comes back.* The comic
path places image bytes into a PDF stream verbatim, so a copy per entry is a
copy of the whole archive, and a 200-page scan is the size at which that stops
being a detail.

The three readers land in three different places on that question, and the
signatures say so:

| Reader | `read` returns | Why |
| --- | --- | --- |
| `tar::Archive` | `&'a [u8]` | every entry is a contiguous byte range of the input, always |
| `rar::Archive` | `Cow<'a, [u8]>` | a **stored** entry is a range; the format allows one that is not |
| `sevenz::Archive` | `Vec<u8>`, `&mut self` | a solid block is many files in one stream — **no range of the input is any one file** |

A trait over all three would have to return the weakest of the three, which
deletes the exact property that ZIP test exists to hold, in the crates that
have it, in order to give three unrelated readers one name. So the facade
matches on an enum and is one `match` longer for it.

The failure this avoids is visible one layer up and is the same shape:
`tinker_pdf::ArchiveRefusal` is a union of several formats' vocabularies of
which only some are reachable from a comic archive, because several formats
were given one enum to fail through. Three error enums here is that not
repeated.

### The exit criterion: the identical-payload property

`crates/tinker-pdf/tests/cbz_real.rs` commits archives written by independent
real archivers — 7-Zip, WinRAR, .NET's `System.IO.Compression` through
PowerShell, and CPython's `zipfile` — and **every one of them holds the same
five pages**: `page1.png` 60×80, `page2.png` 64×88, `page3.jpg` 48×96,
`page10.png` 56×72, `page11.png` 72×64.

So a container decoder is right exactly when the pictures it produces are
byte-identical to the ones the already-trusted ZIP path produces from the same
five pages. Nothing outside this repository renders anything: both sides of the
comparison are this engine reading two files, and a relation between two reads
needs no ground truth (ruling 13, [verification.md](../verification.md)).

It is a strong criterion and it is worth saying why. 7-Zip packs the pages in
the order `page1, page10, page11, page2, page3` — deliberately not reading
order — so a decoder that trusted the archive's own order, or sorted
lexicographically, pages the comic 1, 10, 11, 2, 3 with every page present and
nothing anywhere saying so. And a decompressor that is subtly wrong cannot pass
at all: the pages are PNG and JPEG, so a single wrong byte is a raster that
fails to decode or renders differently.

A container with a reader is in `READ_CONTAINERS` and is held to that same
sentence rather than given a check of its own.

**`winrar-rar5.cbr` is the one that cannot join it**, and rather than weaken
the criterion for everyone it sits in a third list, `PARTLY_READ`, with its own
check: five pages in reading order, four byte-identical to the ZIP's, and the
fifth a placeholder naming the method that made it one. Filing it under either
of the other two lists would be a false claim — "refused" is false, and "read"
is what the criterion means.

### What adjudicates each decoder, which is not the same in the three

This is the axis on which the three modules differ most, and it decides how
many unit tests each needs.

| Container | Checks itself over | Consequence |
| --- | --- | --- |
| tar | **one checksum per header**, none over file data | nothing adjudicates a name or a size but an assertion that names it |
| 7z | **a CRC-32 per file**, in the header | a wrong LZMA window fails the *format's* check |
| RAR 5 | **a CRC-32 per header and per file** | a walk cannot go wrong silently; an extraction cannot either |

That table is the whole reason a hand-rolled LZMA decoder stands here with no
oracle to check it against. `7z-lzma2.cb7` was written by 7-Zip, not by this
engine, and it records a CRC-32 per file; a wrong window, a mis-set
probability array or an ignored LZMA2 dictionary reset fails that check inside
`sevenz::Archive::read` and becomes a placeholder page rather than a picture
with the wrong pixels in it. The format adjudicating the decompression is
first-party in the only sense ruling 13 cares about.

What a CRC does **not** cover is metadata: the `NUMBER` and `vint` encodings,
filename decoding, the empty-stream bit vectors. A mis-decoded filename changes
page order and fails nothing, which is why that half is asserted by name
against hand-built fixtures instead.

### The LZ77 window is the output

A general LZMA decoder keeps a circular dictionary because it streams. This one
does not stream: every caller knows the unpacked size before it starts, because
7z records it in `kCodersUnpackSize` and LZMA2 records it per chunk. So the
"dictionary" is the output written so far, a match is a copy from earlier in
the same `Vec`, and a distance reaching past the start is an error rather than
a wrap-around to whatever the buffer happened to hold.

That removes the class of bug this decoder would otherwise be most likely to
have — a wrong modulus on the window, which produces plausible bytes rather
than an error — at the cost of the peak memory `MAX_7Z_UNPACKED` bounds.

### Degrading, and where the line between page and archive is

Ruling 2. One bad entry is a placeholder page, not a lost archive, and the
line is drawn at *what a host can act on*:

- **Archive-level** (`ArchiveRefusal`): no signature, an encrypted archive, a
  coder this build does not read, a folder that is not a tree, a header that
  does not checksum. All of these are true of the whole file.
- **Page-level** (`PageDefect`): a sparse tar entry, a 7z entry whose CRC does
  not match, a RAR entry compressed with a method this build does not
  decompress. A comic of a hundred pages with one of these keeps ninety-nine,
  **and the missing one keeps its page number**.

A coder is refused at `open` rather than at `read` on purpose: an unreadable
method is one sentence about the archive rather than the same page defect
repeated two hundred times.

### RAR 4, and RAR 5's compression: two decisions, argued

**RAR 4 is refused by name.** `Error::Rar4` is its own variant, separate from
`NotARar`, because the two are different sentences: "this is not a RAR" and
"this is a RAR of a version I do not read" lead a user to different actions,
and the second is true.

The reason is recorded in `crates/tinker-pdf/tests/cbz/README.md`: **WinRAR
7.20 cannot create RAR 4.** There is no `-ma` switch at all — `-ma4` answers
`ERROR: Unknown option: ma4` and the help lists no format-version switch. So a
RAR 4 decoder would have **no first-party fixture here to be held to**, and
under ruling 13 that means decoded-but-unadjudicated. Between shipping a
decoder nothing in this repository can check and refusing a format by name, the
refusal is the honest one.

**RAR 5's compression methods 1–5 are a page-level refusal, by method
number.** `winrar-rar5.cbr` holds six records:

| Record | Method | |
| --- | --- | --- |
| `page1.png`, `page10.png`, `page11.png`, `page2.png` | 0 (store) | read |
| `page3.jpg` | **3 (normal)** | refused, by method number |
| `QO` | — | a quick-open index service record, listed and never a page |

`page3.jpg` is 169 bytes with its own recorded CRC-32, written by WinRAR, and
the ZIP corpus holds the same picture — so a decompressor could be adjudicated
exactly, by the format's own checksum and by the cross-container identity. The
fixture is not what stops one.

The identical-payload property does **not** hold for `.cbr`. Four of the five
pages come back as the ZIP's own pictures; the fifth is a placeholder that
names its method. So `winrar-rar5.cbr` is not in `READ_CONTAINERS`, and what
reads is the container, the store path and a page-level refusal — which is
ruling 2 working as intended (four readable pages beat a refused archive) and
is not the same thing as the criterion being met.

**There is nothing to hand-roll RAR 5's compression from.** CONTRIBUTING rule 1
says every decoder here is written from the format, and every other decoder in
this workspace names the document it was written from: DEFLATE from RFC 1951,
CFF from Adobe TN 5176, tar from POSIX 1003.1, the 7z container from
`7zFormat.txt`. **RAR has no such document for its compression.** RARLAB
publishes a RAR 5.0 *archive format* note, and that note is exactly what the
`rar` module was written from — the signature, the `vint`, the header chain,
the file records, the flag words. It stops at the data area. The compression
algorithm has never been specified publicly.

What exists instead is one implementation, RARLAB's `unrar`, which every RAR
reader in the world is a copy or a wrapper of. `deny.toml` records this
repository's position on it, in the entry that denies the `unrar` crates: its
licence forbids using the source to write a compatible compressor, *"a
restriction this repository's dual MIT/Apache-2.0 grant cannot pass on"*, and
the RAR reader here is *"written from the published format note and from the
committed fixture, and nothing of unrar is vendored, linked or read."* Writing
the decompressor would mean transcribing that source from memory or from a
copy, which is the one thing that entry says will not happen.

So RAR 5 compression is a non-goal: named, permanent, and refused with the
reason attached rather than left as a debt. The row in
[features/cbz.md](../features/cbz.md) says which method it refused, so a user
can re-pack with `-m0` and open the result — which is a real answer, and the
best one available under rule 1.

**What would change this:** a published specification of RAR's compression, or
a clean-room description not derived from `unrar`. If one appears, the fixture
is committed and the criterion is written: `winrar-rar5.cbr` joins
`READ_CONTAINERS` and `five_zip_writers_produce_the_same_five_pictures` passes
over it with no placeholder.

## Milestones

1. **tar.** `7z-tar.cbt` is in `READ_CONTAINERS` in `cbz_real.rs` and
   `five_zip_writers_produce_the_same_five_pictures` passes over it — the same
   five pictures at the same sizes, no placeholder, no warning — with a fuzz
   target, committed seeds and a replay test that runs them on stable.
2. **7z, LZMA and LZMA2.** The same sentence for `7z-lzma2.cb7`, **and** its
   five recorded CRC-32s matching — which is the decompressor's own criterion
   and is checked inside `read` rather than asserted beside it. The archive's
   header is LZMA-compressed, so listing it at all exercises the second front
   end.
3. **RAR 5, container and store.** `winrar-rar5.cbr` opens, lists its six
   records in the order it holds them, and hands back the four stored pages
   with their recorded CRC-32s matching and their rasters identical to the
   ZIP's. Every header's own CRC-32 is checked before its fields are believed.
   RAR 4 is refused by its own signature. `page3.jpg` is method 3, so the
   archive is not five pictures and is held instead to
   `the_rar_a_real_archiver_wrote_pages_what_it_stored_and_names_what_it_did_not`:
   five pages in reading order, four of them byte-identical to the ZIP's, and
   the fifth a placeholder naming its method.

## Risks

**A decompressor that is subtly wrong and passes.** The mitigation is the
per-file CRC-32, and it is a real one — but it is only as good as the coverage
of the *paths* the fixtures take. `-m0=LZMA2` is what a desktop archiver writes
by default and the default is the *simplest* thing the format allows: one
folder holding one LZMA2 chunk, which enters `decode_folder`'s walk and
`decode_lzma2`'s chunk loop exactly once each. So the same producer is asked
for two more shapes:

- `7z-nonsolid.cb7` (`-ms=off`) is **five folders**, one per page: the walk runs
  past folder 0 over five different pack offsets and the folder cache is asked
  for a folder it does not hold.
- `7z-dictreset.cb7` (`-m0=LZMA2:d8k:c8k`) is one folder of **three LZMA2
  chunks**, each opening with a dictionary reset — two of them mid-stream, at
  output offsets 8 192 and 16 384, which fall inside a page rather than between
  two.

Both structures are asserted, not assumed:
`the_two_cb7s_added_for_coverage_have_the_structure_they_are_named_for` opens
the committed bytes with this crate's own reader and holds each file to its
folder count and its chunk count. The chunks come from the LZMA2 block size
(`c`): a dictionary size does not split a solid block, so `-m0=LZMA2:d64k` is
one chunk over these five pages. A fixture named for a shape it does not have
is worse than no fixture, since the name would be doing the arguing.

**A misreading of the format shared between writer and reader** is what a
second real archiver guards against; five folders and three chunks buy
coverage of this decoder's loops and nothing against it. py7zr 1.1.3 — its own
container writer, over liblzma — wrote `py7zr-bcj.cb7`, which is in
`READ_CONTAINERS`, so `five_zip_writers_produce_the_same_five_pictures` passes
over it. It lists a folder's coders in the opposite order from 7-Zip, which a
walk by list position rather than by bind pair would read backwards.

**A coder's fixture proves something only once a reader runs the coder**: an
archive refused at open proves nothing about the coder it would have run.
`-mf=BCJ` writes coder id `03030103`, which `sevenz.rs` reads;
`tinker-pdf-archive/tests/coders/py7zr-bcj.7z` is a real writer's BCJ over a
file shaped for the filter, and BCJ2's fixture is 7-Zip 26.02's own, from the
Linux build (`7zz-bcj2.7z`).

The residual risk: the unit tests cover the container's grammar with the Copy
coder and the arithmetic coder with a first-party range *encoder*, and the gap
between those two is bridged by the committed archives alone.

**A shared misunderstanding between the encoder and decoder in
`lzma/tests.rs`.** The round trip closes only over literals, and both halves
are this repository's. They were transcribed separately — the carry chain in
`shift_low` has no counterpart in the decoder at all — but a format read wrong
in the same way twice would pass. Any property both halves share is invisible
to the round trip *by construction* — `MOVE_BITS`, the probability adaptation
rate, is one constant both read — which is why the round trip is described as
covering the arithmetic coder's mechanics rather than LZMA, and why the
fixture is the thing that rules. For the decompressor the committed archives
are the only instrument: no unit test here asserts anything about an LZMA
distance.

**Hostile input, in three parsers.** Every one of these formats is a
length-prefixed structure walk over bytes a stranger wrote, and a comic archive
is the file type in this repository a user is most likely to have been handed.
Ruling 1: no `unwrap` on a file-derived value, checked or saturating arithmetic
throughout, `#![forbid(unsafe_code)]`, and an explicit budget in front of every
allocation. Each module has a fuzz target with a committed seed corpus **and a
replay test**, because a seed corpus nothing reads on every commit stops
describing the parser without anybody noticing.

**A cap that cannot fire** explores no refusal. Every constant in the three
`limits` modules carries three numbers — the most any fixture here spends, the
most a plausible real archive spends, and the cap — and each is proved to fire
by its own test. The fuzz targets take their bounds from a control byte rather
than the shipped defaults, and two seeds per target carry a control byte of
zero so the corpus reaches a refusal.

**Memory, on the 7z path specifically.** A solid block decompresses whole to
read any one file in it, so peak memory is the block and not the page.
`MAX_7Z_UNPACKED` is 1 GiB and is deliberately larger than any per-entry cap
elsewhere in this workspace; the folder cache means a whole comic costs that
once rather than once per page, and it is the one place this crate spends more
than `tinker-pdf-zip` would.

**Doc drift about what is verified.** `.cbr` support is a container with a
store path, and the compression will not be written; that sentence can decay
into "RAR is done" as the rows move. The mitigation is structural rather than
editorial. `winrar-rar5.cbr` is in `PARTLY_READ` and not in `READ_CONTAINERS`,
so it is held to
`the_rar_a_real_archiver_wrote_pages_what_it_stored_and_names_what_it_did_not`
— which asserts the placeholder **by name and by method number**, and fails if
`page3.jpg` ever silently starts or stops being one. Changing that means
deleting that test, which is a visible act in a diff, where letting a claim
drift is not.
