# Comic archives: the containers that are not ZIP

A `.cbz` is a ZIP and has been readable since tier 1. A `.cbt` is a tar, a
`.cb7` is a 7z and a `.cbr` is a RAR, and all three were recognised at a fixed
offset and refused by name — `ArchiveRefusal::NotAZip` — with the refusal row
in [features/cbz.md](../features/cbz.md) reading *"three more decompressors,
two of them encumbered, and none of them a page"*.

This is the design for making them pages. It is owed by
[ROADMAP.md](../ROADMAP.md)'s tier-4 CBZ item, which says a design doc arrives
with the decoders.

## Scope

**One new leaf crate, `tinker-pdf-archive`, holding three container readers and
the compression one of them needs.** Bytes in, names and byte ranges out, no
COS types, no idea what an entry is *for* (ruling 8). The facade's `cbz.rs`
keeps every decision about what a page is.

- **tar** — POSIX 1003.1 `ustar`, GNU's `L` long-name pseudo-entry, PAX's `x`
  and `g` extended headers. No compression at all; the entry walk is the whole
  format.
- **7z** — the signature header, the property-id header grammar, packed
  streams, folders and coders, including `kEncodedHeader`. Coders: Copy, LZMA,
  LZMA2, Deflate.
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
  and is a named non-goal shared with `tinker-pdf-zip`, refused as
  `Encrypted` rather than as an unreadable stream three layers down.
- **Writing** any of these formats. Every reader here is a reader.
- **Multi-volume sets.** The fragment that happens to be present is not the
  archive, in RAR and in tar's type `M` alike.
- **Sparse files.** Refused rather than returned with their holes closed up,
  which is what a reader that ignored the flag would hand back: bytes in the
  wrong places are a picture that decodes to the wrong thing.
- **7z coders beyond the four listed**, and **folders whose coder graph is not
  a chain**. BCJ2 is the one that exists in the wild and it takes four input
  streams; a reader that walked it as a chain would hand back a quarter of a
  file.
- **RAR 5 compression methods 1–5**, and **RAR 4 entirely.** Both have a
  section of their own below, because both are decisions rather than omissions.
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
`tinker_pdf::ArchiveRefusal` is a twenty-odd-variant union of several formats'
vocabularies of which only a third are reachable from a comic archive, because
several formats were given one enum to fail through. Three error enums here is
that not repeated.

### The exit criterion: the identical-payload property

`crates/tinker-pdf/tests/cbz_real.rs` commits archives written by four
independent real archivers — 7-Zip, WinRAR, .NET's `System.IO.Compression`
through PowerShell, and CPython's `zipfile` — and **every one of them holds the
same five pages**: `page1.png` 60×80, `page2.png` 64×88, `page3.jpg` 48×96,
`page10.png` 56×72, `page11.png` 72×64.

So a container decoder is right exactly when the pictures it produces are
byte-identical to the ones the already-trusted ZIP path produces from the same
five pages. Nothing outside this repository renders anything: both sides of the
comparison are this engine reading two files, which is
[verification.md](../verification.md)'s fourth corpus axis — a relation between
two reads needs no ground truth (ruling 13).

It is a strong criterion and it is worth saying why. 7-Zip packs the pages in
the order `page1, page10, page11, page2, page3` — deliberately not reading
order — so a decoder that trusted the archive's own order, or sorted
lexicographically, pages the comic 1, 10, 11, 2, 3 with every page present and
nothing anywhere saying so. And a decompressor that is subtly wrong cannot pass
at all: the pages are PNG and JPEG, so a single wrong byte is a raster that
fails to decode or renders differently.

A new container joins `READ_CONTAINERS` and is held to that same sentence
rather than getting a check of its own. A container leaves `NOT_READ` only in
the commit that gives it a reader.

### What adjudicates each decoder, which is not the same in the three

This is the axis on which the three modules differ most, and it decides how
many unit tests each needs.

| Container | Checks itself over | Consequence |
| --- | --- | --- |
| tar | **one checksum per header**, none over file data | nothing adjudicates a name or a size but an assertion that names it |
| 7z | **a CRC-32 per file**, in the header | a wrong LZMA window fails the *format's* check |
| RAR 5 | **a CRC-32 per header and per file** | a walk cannot go wrong silently; an extraction cannot either |

That table is the whole reason a hand-rolled LZMA decoder could be written here
with no oracle to check it against. `7z-lzma2.cb7` was written by 7-Zip before
this engine had a decoder, and it records a CRC-32 per file; a wrong window, a
mis-set probability array or an ignored LZMA2 dictionary reset fails that check
inside `sevenz::Archive::read` and becomes a placeholder page rather than a
picture with the wrong pixels in it. The format adjudicating the
decompression is first-party in the only sense ruling 13 cares about.

What a CRC does **not** cover is metadata: the `NUMBER` and `vint` encodings,
filename decoding, the empty-stream bit vectors. A mis-decoded filename changes
page order and fails nothing, which is why that half is asserted by name
against hand-built fixtures instead.

The counted-injection tables in `tar/tests.rs` and `sevenz/tests.rs` are the
measurement of that difference, and they came out the way the table predicts:
six of the eight tar defects are caught once or twice, because there is nothing
in the format to catch them but an assertion.

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
  coder this build does not read, a folder that is not a chain, a header that
  does not checksum. All of these are true of the whole file.
- **Page-level** (`PageDefect`): a sparse tar entry, a 7z entry whose CRC does
  not match, a RAR entry compressed with a method this build does not
  decompress. A comic of a hundred pages with one of these keeps ninety-nine,
  **and the missing one keeps its page number**.

A coder is refused at `open` rather than at `read` on purpose: an unreadable
method is one sentence about the archive rather than the same page defect
repeated two hundred times.

### RAR 4, and RAR 5's compression: two decisions, argued

**RAR 4 stays refused by name.** `Error::Rar4` is its own variant, separate
from `NotARar`, because the two are different sentences: "this is not a RAR"
and "this is a RAR of a version I do not read" lead a user to different
actions, and the second is true.

The reason is a constraint discovered by trying, and it is recorded in
`crates/tinker-pdf/tests/cbz/README.md`: **WinRAR 7.20 on this machine cannot
create RAR 4.** There is no `-ma` switch at all — `-ma4` answers `ERROR:
Unknown option: ma4` and the help lists no format-version switch. So a RAR 4
decoder would have **no first-party fixture here to be held to**, and under
ruling 13 that means decoded-but-unadjudicated. Between shipping a decoder
nothing in this repository can check and refusing a format by name, the refusal
is the honest one, and it is cheap: `.cbr` files in the wild have been RAR 5
since 2013.

**RAR 5's compression methods 1–5 are a page-level refusal, by method number**,
and the honest version of this is not the one this document was first written
with. The first draft said the committed `winrar-rar5.cbr` stores every entry,
reasoning that WinRAR compresses only what gets smaller and an image never
does. **That was wrong, and reading the fixture rather than reasoning about it
is what found it.** What `winrar-rar5.cbr` actually holds is six records:

| Record | Method | |
| --- | --- | --- |
| `page1.png`, `page10.png`, `page11.png`, `page2.png` | 0 (store) | read |
| `page3.jpg` | **3 (normal)** | refused, by method number |
| `QO` | — | a quick-open index service record, listed and never a page |

So the reasoning was right about the four PNGs and wrong about the JPEG, and
the consequence runs both ways.

**The good half:** RAR 5's compression *does* have a first-party fixture here
after all. `page3.jpg` is 169 bytes with its own recorded CRC-32, written by
WinRAR before any decoder existed, and the ZIP corpus holds the same picture —
so a decompressor can be adjudicated exactly, by the format's own checksum and
by the cross-container identity. Milestone 5 is therefore a real, checkable
piece of work rather than one blocked on a fixture nobody can make.

**The costly half:** the identical-payload property is **not** green for
`.cbr`, and this lane does not claim it is. Four of the five pages come back as
the ZIP's own pictures; the fifth is a placeholder that names its method. So
`winrar-rar5.cbr` does not join `READ_CONTAINERS`, the row does not close, and
what lands is the container, the store path and a page-level refusal — which is
ruling 2 working as intended (four readable pages beat a refused archive) and
is not the same thing as the criterion being met.

## Milestones

Each is one commit, and the commit that makes a criterion green is the same
commit that edits the refusal row in [features/cbz.md](../features/cbz.md) and
narrows the tier-4 CBZ entry in [ROADMAP.md](../ROADMAP.md).

**1. tar. Done.**
Exit criterion: `7z-tar.cbt` joins `READ_CONTAINERS` in `cbz_real.rs` and
`five_zip_writers_produce_the_same_five_pictures` passes over it — the same
five pictures at the same sizes, no placeholder, no warning. Plus a fuzz target
with seven committed seeds and a replay test that runs them on stable.
*Row: loses CBT.*

**2. 7z, LZMA and LZMA2. Done.**
Exit criterion: the same sentence for `7z-lzma2.cb7`, **and** its five
recorded CRC-32s matching — which is the decompressor's own criterion and is
checked inside `read` rather than asserted beside it. The archive's header is
LZMA-compressed, so listing it at all exercises the second front end.
*Row: loses CB7.*

**3. RAR 5, container and store. Done, and it does not close the row.**
Exit criterion, met: `winrar-rar5.cbr` opens, lists its six records in the
order it holds them, and hands back the four stored pages with their recorded
CRC-32s matching and their rasters identical to the ZIP's. Every header's own
CRC-32 is checked before its fields are believed. RAR 4 is refused by its own
signature.
Exit criterion, **not** met and not claimed: `page3.jpg` is method 3, so the
archive is not five pictures and does not join `READ_CONTAINERS`.
*Row: CBR leaves the archive-level table; a page-level row naming the
compression methods and a RAR 4 row take its place.*

**4. This document.** Owed by the roadmap; lands with the decoders.

**Not scheduled, and listed so it is a decision rather than a gap:**

**5. RAR 5's compression algorithm — LZSS, its Huffman tables and its filter
chain.** This is what closes the CBR row, and it is the one piece of the lane
that is scoped and unwritten.

The fixture already exists and is committed: `page3.jpg` inside
`winrar-rar5.cbr`, method 3, 169 bytes unpacked, with its own recorded CRC-32
— and the same picture sits in five ZIPs beside it, so the decoder is
adjudicated twice over, by the format's checksum and by the cross-container
identity. No new corpus work is needed and no producer has to be run.

Exit criterion: `winrar-rar5.cbr` joins `READ_CONTAINERS` and
`five_zip_writers_produce_the_same_five_pictures` passes over it with no
placeholder — at which point the compression row leaves
[features/cbz.md](../features/cbz.md) and only the RAR 4 row remains.

## Risks

**A decompressor that is subtly wrong and passes.** The mitigation is the
per-file CRC-32, and it is a real one — but it is only as good as the coverage
of the *paths* the fixture takes. `7z-lzma2.cb7` is one solid LZMA2 block of
five images; it never exercises a multi-folder archive, a BCJ filter chain, or
an LZMA2 stream with more than one dictionary reset. The unit tests cover the
container's grammar with the Copy coder and the arithmetic coder with a
first-party range *encoder*, and the gap between those two is the residual
risk: **matches and distances are exercised only by the one fixture.** Named
here rather than left implicit.

**A shared misunderstanding between the encoder and decoder in
`lzma/tests.rs`.** The round trip closes only over literals, and both halves
are this repository's. They were transcribed separately — the carry chain in
`shift_low` has no counterpart in the decoder at all — but a format read wrong
in the same way twice would pass.

**The injection campaign demonstrated this rather than leaving it as a worry,
and the demonstration is worth keeping.** One of the eleven defects was
`MOVE_BITS`, the probability adaptation rate, set to 1/16 instead of the
format's 1/32. Both the decoder and the test encoder read that one constant, so
the round trip closed perfectly on every input — and the defect was caught only
by the two `.cb7` tests, through the archive's own CRC-32. Any property both
halves share is invisible to the round trip *by construction*, which is why the
round trip is described as covering the arithmetic coder's mechanics rather
than LZMA, and why the fixture is the thing that rules.

All five decoder defects landed the same way: `five_zip_writers_produce_the_same_five_pictures`
and `the_7z_a_real_archiver_wrote_pages_in_natural_order`, twice each, and no
unit test anywhere. The corpus is not a nice-to-have beside the unit tests here;
for the decompressor it is the only instrument.

**Hostile input, in three new parsers at once.** Every one of these formats is
a length-prefixed structure walk over bytes a stranger wrote, and a comic
archive is the file type in this repository a user is most likely to have been
handed. Ruling 1: no `unwrap` on a file-derived value, checked or saturating
arithmetic throughout, `#![forbid(unsafe_code)]`, and an explicit budget in
front of every allocation. Each module has a fuzz target with a committed seed
corpus **and a replay test**, because a seed corpus nothing reads on every
commit stops describing the parser without anybody noticing.

**A cap that cannot fire.** Gap 18a's milestone 8 found a work cap set above
the most its own inputs could ask for, so nothing ever explored a refusal.
Every constant in the three `limits` modules carries three numbers — the most
any fixture here spends, the most a plausible real archive spends, and the cap
— and each is proved to fire by its own test. The fuzz targets take their
bounds from a control byte rather than the shipped defaults, and two seeds per
target carry a control byte of zero so the corpus reaches a refusal.

**Memory, on the 7z path specifically.** A solid block decompresses whole to
read any one file in it, so peak memory is the block and not the page.
`MAX_7Z_UNPACKED` is 1 GiB and is deliberately larger than any per-entry cap
elsewhere in this workspace; the folder cache means a whole comic costs that
once rather than once per page, and it is the one place this lane spends more
than `tinker-pdf-zip` would.

**Doc drift about what is actually verified.** The uncomfortable sentence in
this document is that `.cbr` support is a container with a store path and the
compression is unwritten. It is easy for that to decay into "RAR is done" as
the rows move. The mitigation is structural: the refusal row does not vanish
when RAR lands, it changes shape, and `NOT_READ` in `cbz_real.rs` asserts it is
non-empty so that a sweep with nothing to sweep fails rather than passing
green.
