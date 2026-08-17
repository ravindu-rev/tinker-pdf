# A comic archive opens as `NotAPdf`, and after MuPDF nothing else will open one

A CBZ is a ZIP of page images, one file per page, and it is the smallest of
the three formats [28](28-tinker-integration-decisions.md) decided are built
here. `Document::open` on one returns `OpenError::NotAPdf` today, and Tinker's
route to MuPDF's `Doc::Other` disappears with the swap
[plan 15](../15-tinker-integration.md) describes — so on the day the AGPL
dependency leaves, a format that used to open stops opening. When this is
done, a CBZ opens as a `Document` whose pages are its images, in the order a
reader expects, and an archive this build cannot honestly page is **refused by
name** rather than opened with pages quietly missing from the middle of it.
(S)

**This is the first of the three plans gap 28 promises.** That document says,
at the end of its option D section: *"Three new gap plans will be written for
them — CBZ, XPS and EPUB — after this gap closes."* [18a](18a-jpx-decoder.md)
closed at `d9945a0`, which was the last of the twenty-eight, so the three are
now owed. XPS (L) and EPUB (XL+) follow this one and neither is planned here.

## Which rule governs this, since it is not ruling 3

Ruling 3 schedules deferred capabilities by corpus hit-rate, and both
[10](10-mesh-shadings.md) and [18a](18a-jpx-decoder.md) had to argue over it
in writing before they could be built. This plan does not, and the reason is
worth stating rather than leaving a reader to wonder why the argument is
missing.

Ruling 3 binds [02](../02-filters.md), [08](../08-rendering-device.md) and the
master plan's descope levers, and what it schedules is a **`Capability`** — a
codec inside a PDF that the engine defers behind a flag, degrades with a
placeholder under ruling 2, and builds when the nightly hit-rate report says
real documents need it. CBZ is none of those things. It is a container format,
not a codec; it produces no `Capability` variant, no placeholder and no
`Warning::Degraded`; and no PDF in any corpus will ever contain one, so a
corpus hit-rate for it is not a number that can exist. [23](23-corpus-runner.md)
measured 4 525 PDFs and could not have measured this if it had run for a year.

What governs it is an **owner decision, dated 16 August 2026, recorded in plan
15 where the options used to be** and summarised in gap 28's `As built`. That
is the same authority [27](27-form-calculations-decision.md) was built under,
and it is a stronger one than a hit rate rather than a weaker one — the corpus
is evidence, and a decision is a decision.

One correction while this is being written down. Plan 15's own answer says
"Ruling 3 and CONTRIBUTING rule 1 hold throughout — the ZIP reader, the XML
parser and every line of the CSS are ours", and its form-calculations
paragraph says "`boa` was never a candidate, because ruling 3 rules out a
JavaScript crate". Ruling 3 does not rule out a crate; it schedules
capabilities. The rule that makes the ZIP reader ours is **CONTRIBUTING rule
1**, backed by `deny.toml`. The conclusion is right in both places and the
citation is wrong in both, and this plan cites rule 1.

## What is wrong

Nothing is subtly wrong, which is this plan's version of what
[17](17-jbig2-generic-region.md) and [18a](18a-jpx-decoder.md) each had to say
about their own subjects. `Document::open` reads the bytes, `CosDocument::open`
finds no indirect object, and `OpenError::NotAPdf` comes back with the message
*"not a PDF: no indirect objects found"*, which is exactly true. Nothing
crashes, nothing lies, and nothing is degraded.

So this is a missing feature, and the only defect in today's behaviour is that
it is not the feature. What makes it urgent rather than merely absent is the
sequencing: Tinker exposes CBZ through MuPDF today, and plan 15's deletion
checklist removes MuPDF. Until this lands, that checklist trades a working
format for a licensing outcome, and gap 28 chose option D precisely so that it
would not have to.

## The two decisions taken during planning

Both were put to the owner and both are recorded here because they are the
kind that get made implicitly by whoever writes the first file — which is the
failure gap 18's risk table named for the fixed-point width and the reason
[18a](18a-jpx-decoder.md) exists as a separate document.

**1. A CBZ becomes a `Document` by synthesising a PDF at open.** Not an enum
inside `Document`, not a trait behind it, and not a second document type
beside it. The argument is in [Design](#why-synthesis-and-not-an-enum) and it
turns on one method.

**2. JPEG and PNG are both supported, with a real PNG decoder** — not a
pass-through path that works for the easy PNGs and refuses the rest. But the
decoder is the **fallback**, not the default, for reasons that are about
memory rather than about elegance; see
[Pass-through is the default](#pass-through-is-the-default-and-the-decoder-is-the-fallback).

## Scope

- **A ZIP reader**, in a new leaf crate: the end-of-central-directory record,
  the central directory, local file headers as a fallback, Zip64's locator and
  record, stored and deflated entries, data descriptors, and CRC-32 checked on
  every entry.
- **CRC-32**, which does not exist anywhere in this repository, shared by the
  ZIP reader and the PNG decoder.
- **`inflate_raw` made public** in `tinker-pdf-filters`, so ZIP data — which
  is raw DEFLATE by definition — does not go through `flate_decode`'s zlib
  sniff.
- **A PNG decoder** in `tinker-pdf-filters`: the chunk structure with CRCs,
  IHDR, PLTE, tRNS, IDAT and IEND; colour types 0, 2, 3, 4 and 6; bit depths
  1, 2, 4, 8 and 16; and Adam7 interlace.
- **An `ImageData` variant for pre-compressed images**, so a PNG's IDAT
  reaches a page as `/FlateDecode` with `/Predictor 15` without being decoded.
- **CBZ page semantics** in the facade: which entries are pages, in what
  order, at what size, and what a page that cannot be built looks like.
- **`Document::open` sniffing `PK\x03\x04`**, and `OpenError` growing the
  variant an unrecognisable container needs.
- **A refusal, by name, everywhere the answer would otherwise be a plausible
  wrong document** — the feature, not the absence of one.

## Non-goals

Each of these is refused rather than approximated, and named so a reader does
not infer it from "CBZ works now".

- **CBR, CB7, CBT.** RAR, 7z and tar are three more decompressors and two of
  them are patent- and licence-encumbered in ways this project has no reason
  to touch. A `Rar!` or `7z\xBC\xAF\x27\x1C` signature is recognised and
  refused **by name**, because "this is a CBR and I do not read CBR" is a
  different sentence from "this is not a PDF".
- **Encrypted ZIP entries.** Both ZipCrypto and the AES extensions.
  General-purpose bit 0 refuses the entry.
- **Multi-disk and spanned archives.** A disk number other than zero refuses
  the archive.
- **Image formats other than JPEG and PNG.** GIF, WebP, AVIF, BMP and TIFF are
  each recognised by magic bytes and refused by name at the page level; see
  [What is refused, and at which level](#what-is-refused-and-at-which-level).
- **`ComicInfo.xml` and the rest of the metadata conventions.** Reading it
  needs an XML parser this engine does not have — gap 30, XPS, is where one
  arrives — and a comic's title is not what a reader is blocked on.
- **Writing a CBZ.** Nothing in the shipped surface produces one. A
  synthesised document saves as a PDF, which is what it is.
- **Progressive or partial open.** The document is synthesised whole at
  `open`, like every other document this engine produces.

## Design

### Why synthesis, and not an enum

`Document::cos(&self) -> &CosDocument` (`crates/tinker-pdf/src/lib.rs:465`)
returns a **borrow, and not an `Option`**. That single signature is the whole
argument.

A CBZ has no `CosDocument` to lend. If `Document` became an enum over PDF and
archive, `cos()` would have to become fallible — and it is the one method on
the facade that cannot cheaply become fallible, because it is a documented
escape hatch that eleven COS types are re-exported to support. The re-export's
own doc comment says why they are there: *"The escape hatch is only an escape
hatch if the types it hands back can be named without depending on the crate
underneath."* Plan 00 records the reason the hatch exists at all — Tinker's
page-surgery plans repeatedly needed raw object access MuPDF hid, and the
answer here was a supported read-only view rather than an `unsafe` shim
culture. Making it fallible is the only genuinely breaking change available in
this whole plan, and it would be taken to support a format that has nothing to
put behind it.

A trait behind `Document` is the same problem with an extra layer: every
method that returns a concrete type — `cos()`, `page()`, `metadata()`,
`form_fields()` — has to be expressible for a format that has no answer, and
the honest implementations are all `unimplemented` in one direction or
another.

**Synthesis answers the method truthfully.** The synthesised document *is*
what the engine renders. `cos()` hands back a real `CosDocument` with a real
catalog, a real page tree and real image XObjects, because that is exactly what
the renderer was given; there is no second path through the interpreter, no
second `Device`, and no CBZ-shaped branch anywhere below the facade. Every
capability the engine already has — cancellation, optional content (there is
none), warnings, the sample path, `at_dpi`, the bounded painting of
[14](14-bounded-painting.md) — arrives for nothing rather than being wired
twice.

It is also testable in a way the alternatives are not. A synthesised document
can be saved and handed to qpdf, which ruling 9 already puts in CI for
[20](20-linearization-validation.md), so "the CBZ produced a valid document"
becomes a claim a third-party tool checks rather than a claim this repository
makes about itself.

The mechanism is the one the authoring layer already has:
`DocumentBuilder` builds objects, `finish()` serialises to `Vec<u8>`, and
`CosDocument::open` parses those bytes. Synthesis therefore goes out through
the writer and back in through the reader, which means the synthesised
document is parsed by the same parser as every real PDF and cannot take a
shortcut around it.

### Pass-through is the default, and the decoder is the fallback

This is the part of the plan most likely to be got wrong by an implementation
that reads only the scope list, because the obvious build decodes every image
and only fails at the size that matters.

**CBZ images map onto PDF image XObjects with no pixel work at all.**
`ImageData::Jpeg` (`crates/tinker-pdf-cos/src/build.rs:20`) already places
JPEG bytes verbatim, and its own comment says why: *"never re-encoded:
recompression is generational quality loss the caller cannot undo"*. The PNG
case is less obvious and just as exact. A non-interlaced PNG's IDAT is a zlib
stream of predictor-filtered scanlines, each row prefixed by a filter tag of
0 to 4 — which is precisely what PDF's `/FlateDecode` with
`/DecodeParms << /Predictor 15 /Colors n /BitsPerComponent d /Columns w >>`
expects, byte for byte, including the per-row tag and including the
left-neighbour offset being `ceil(colors × bpc / 8)` with a floor of one.

That is not a coincidence and the repository already contains the evidence.
`crates/tinker-pdf-filters/src/predictors.rs` implements PNG's filter set with
`paeth` at line 207 under the doc comment *"PNG 9.4 Paeth predictor"*, in a
module whose own header cites *"the PNG row filters"* — because PDF's
`/Predictor` **is** PNG's specification, adopted wholesale. The pass-through
is therefore not a trick; it is the two formats agreeing.

**And it is load-bearing rather than merely elegant.** Synthesis at open
builds every page upfront, so whatever a page holds is held for the whole
document. Decoding each PNG to `Rgb8` would hold *w × h × 3* per page: a
200-page archive at 2000 × 3000 is **about 3.6 GB**, for a document a reader
will show one page of. Passing the compressed bytes through keeps the peak at
a small multiple of the archive's own size — the archive bytes, plus the
synthesised PDF that mostly consists of copies of them — and that multiple is
a constant rather than a function of the pixel count.

Two further properties fall out of it, and both are worth having:

- **Most CBZ entries are `stored`, not deflated**, because JPEG and PNG data
  does not compress again. For a stored entry the image file is a subslice of
  the archive, copied once into the synthesised document, and nothing is
  inflated at all — which removes most of the decompression-bomb surface
  before any cap has to fire.
- **The image the user sees is the image the archive holds.** No resample, no
  requantisation, no colour conversion, and — for JPEG — no second generation
  of DCT loss.

**So the PNG decoder is what handles the cases that cannot pass through, and
nothing else calls it.** Those cases are exactly:

| PNG feature | Why pass-through fails | Answer |
| --- | --- | --- |
| Adam7 interlace | Seven reduced images with their own filtered rows; `/Predictor` describes one raster and cannot express it | Decode |
| Colour type 4 (grey + alpha) | The alpha is interleaved into the same scanlines; PDF has no interleaved alpha | Decode, split into samples and an `/SMask` |
| Colour type 6 (RGBA) | As above | Decode, split into samples and an `/SMask` |
| `tRNS` giving partial alpha to palette entries | PDF's `/Mask` colour key is a range test, not a lookup | Decode, build the `/SMask` |
| Colour types 0, 2, 3, non-interlaced | — | **Pass through** |
| `tRNS` naming one fully transparent colour or index | PDF's `/Mask` array expresses exactly this (8.9.6.4) | **Pass through**, with `/Mask` |

The plan's working assumption is that colour types 4 and 6 are at least as
common in real comic archives as Adam7 is, and probably more so, since an
export pipeline that keeps an alpha channel is more likely than one that turns
interlacing on. The decoder is therefore not an edge case to be built last.

Decoding is bounded to **one entry at a time**, because synthesis is
sequential: decode, split, re-deflate into the two streams, drop the buffer.
Peak memory gains one page's pixels, not two hundred. Re-deflating is lossless
— DEFLATE is — so `ImageData::Jpeg`'s generational-loss argument does not
transfer to this path and must not be cited as though it did.

### Where the code goes

**A new leaf crate, `tinker-pdf-zip`.** Plan 00 says of the leaves that they
are *"bytes-in/values-out with zero PDF types … the property that makes each
one independently fuzzable"*, and a ZIP reader is a textbook one: bytes in,
entries out, no PDF vocabulary anywhere near it. It does not go in
`tinker-pdf-filters`, whose module documentation and whose name both say PDF
*stream filters*, and which would then hold a container format that no PDF
stream contains.

It depends on `tinker-pdf-filters` for inflate and CRC-32. That is a leaf-to-
leaf edge, and the graph already has one with its argument written out:
`font -> filters`, which the `ALLOWED` doc comment justifies at length and
closes with *"the graph cannot cycle, because `filters` depends on nothing"*.
The same sentence covers this edge.

**The PNG decoder goes in `tinker-pdf-filters`**, as `png.rs` beside `jpeg.rs`,
`ccitt.rs`, `jbig2.rs` and `jpx/`. It is a codec, it needs `predictors.rs` and
`inflate.rs`, and putting it anywhere else means either duplicating the filter
set or an edge that points the wrong way. **CRC-32 goes there too**, as its
own small module, because both the ZIP reader and the PNG decoder need it and
`tinker-pdf-zip` already depends on that crate.

**CBZ page semantics go in the facade**, as `crates/tinker-pdf/src/cbz.rs`.
They cannot live in a leaf, because deciding what a page *is* needs document
types and no leaf may know one (ruling 8); and they cannot live in `cos`
without teaching the object model about ZIP, which is a container it will
never otherwise meet. The facade is where `resources.rs` already puts the
other boundary decisions — `ccitt_samples`, `jbig2_samples`, `jpx_samples` —
for the same reason.

The DAG amendment, then, is two lines in `xtask/src/main.rs`'s `ALLOWED`
(declared at line 160, checked by `check_dag` at 529 and by
`this_repository_obeys_its_own_graph` at 617): `("tinker-pdf-zip",
&["tinker-pdf-filters"])`, and `"tinker-pdf-zip"` added to the facade's row.
The doc comment above `ALLOWED` spells out each existing amendment with its
argument, and this one is written in the same register rather than added
silently — the file's own commentary records what happened last time a crate
was listed without care: `xtask` was named as `"xtask"`, looked for at
`tools/xtask/Cargo.toml`, not found, and skipped, *"by a check whose entire
purpose is that the compiler cannot do this"*.

### The zlib sniff is the wrong door for ZIP data

`flate_bytes` (`crates/tinker-pdf-filters/src/inflate.rs:704`) sniffs for a
zlib header and treats raw DEFLATE as a fallback. ZIP entry data is raw
DEFLATE **by definition** — APPNOTE 4.4.5 method 8 is RFC 1951 with no
wrapper — so a ZIP reader knows a priori what it has and must not go through
a function that guesses. Three costs, in increasing rarity and decreasing
obviousness:

**Every deflated entry manufactures a `RawDeflateFallback` warning.** The
fallback path pushes it unconditionally. Ruling 10 says warnings carry
provenance and exist so that *"it opened" and "it opened cleanly" stay
distinguishable*; a warning emitted once per entry for something that is not
leniency at all makes the distinction worthless for every CBZ. This one is
certain rather than probabilistic, and it is the strongest of the three.

**Every entry with a data descriptor manufactures a `TrailingGarbage`
warning.** After a raw stream completes, `flate_bytes` compares `r.end`
against `input.len()` and reports anything left over. For an entry whose sizes
were not known at write time (general-purpose bit 3), what follows the stream
is the descriptor and then the next local header — structure, not garbage.

**And a crafted entry can be mis-decoded and never retried.** `zlib_header`
requires the low nibble of the first byte to be 8 and the two bytes to pass
the mod-31 check. In raw DEFLATE the low three bits of the first byte are
BFINAL and BTYPE, so a low nibble of 8 means BFINAL 0 and BTYPE 00 — a
**non-final stored block whose discarded pad bit happens to be set**. That is
narrow, and it is entirely constructible, because ZIP entry data is
attacker-chosen: `08 1D …` passes both tests (`0x081D` is 31 × 67). The zlib
branch then decodes from offset 2, and `flate_bytes` retries as raw *only if
the wrapped read produced nothing at all* — so an input engineered to yield
one byte keeps the wrong answer with no warning saying so.

**The fix is to expose what already exists.** `inflate_raw`
(`inflate.rs:653`) is private, decodes RFC 1951 from bit zero, and already
returns `end`, *"bytes consumed, counted to the end of the final block"* —
which is precisely what an entry with a streamed size needs in order to find
its data descriptor. Making it public, with a public result type, is the whole
change the ZIP reader asks of the filters crate. `flate_decode` keeps its
sniff, keeps its fallback and keeps every one of its tests, because for a PDF
stream named `/FlateDecode` the sniff is the right behaviour and real files
depend on the retry.

### CRC-32 is real work here, not box-ticking

**No CRC-32 exists anywhere in this repository.** `adler32` appears twice —
in `inflate.rs` and `deflate.rs` — and it is a different polynomial answering
a different question. This is new code, and both consumers want it: ZIP stores
one per entry (in the local header, the central directory and, when bit 3 is
set, the data descriptor) and PNG stores one per chunk.

The reason to check them rather than store them is the pass-through. When a
JPEG's bytes are copied verbatim into a PDF stream, **no decoder in this
engine will ever look at them again until a page is rendered**, and by then
there is nothing to compare against. The archive's own CRC is the only
integrity evidence that exists, it covers exactly the bytes about to be
trusted, and discarding it means a corrupt archive produces a page that half
draws with nothing anywhere saying the file was damaged. The same argument
holds one level down for a PNG whose IDAT is passed through and whose chunk
CRCs are the only check on it.

Gap 16 found the shape of the opposite mistake in this very repository:
`ccitt_decode`'s warnings were discarded at the call site — `let (gray, _) =
ccitt_decode(...)` — *"so every leniency it took was invisible"*. A CRC that
is read and not compared is that, with a checksum.

### Entry ordering: natural sort

**Entries are pages in natural order, so `page2` precedes `page10`.**

This is stated because lexicographic is the obvious wrong answer and it
produces a failure that is invisible in test material and catastrophic in the
wild. An archive whose pages are `1.jpg` through `12.jpg` reads
lexicographically as 1, 10, 11, 12, 2, 3, … — every page present, every page
rendered correctly, and the comic unreadable. An archive whose names are
zero-padded (`page001.jpg`) sorts identically under both orders, and padding
is what a carelessly written fixture will have, so the bug ships green.
The ZIP format does not help: APPNOTE puts no ordering requirement on the
central directory, and stored order is whatever the producing tool happened to
walk.

The comparison is defined here so it is not invented twice, and it is defined
in byte arithmetic so that ruling 4 holds — a locale-aware or Unicode-collating
comparison is a per-platform answer and this engine may not have one:

- Compare the full stored path, not the basename, so directories group.
- Split each name into maximal runs of ASCII digits and maximal runs of
  everything else, and compare run by run.
- Non-digit runs compare byte by byte with ASCII letters case-folded, then by
  the unfolded bytes to break ties.
- Digit runs compare by **length after leading zeros are trimmed, then by
  bytes** — never by parsing into an integer, because a forty-digit run is a
  legal filename and `u64::from_str` is not a total function over one.
- Equal names are possible in a ZIP, so ties break on central-directory
  position, which makes the order total.

### Page geometry: one image pixel is one PDF point

`RenderOptions::scale` is documented as *"pixels per PDF point. 1.0 renders at
72 dpi"* and `at_dpi(dpi)` is `dpi / 72.0`. A CBZ image has pixels and no
physical size at all, so a point size has to be chosen, and the choice decides
what every one of those numbers means for this format.

**A CBZ page's `/MediaBox` is `[0 0 width height]` in the image's own pixel
dimensions, and the image fills it.** Consequences, all of them intended:

- `RenderOptions::default()` — `scale: 1.0` — renders at **native
  resolution**, one output pixel per source pixel, with no resampling on any
  path. That is the identity case, it is what a comic reader wants, and it is
  the only scale at which the bytes on screen are the bytes in the archive.
- `Page::size()` reports the image's pixel dimensions, which is a number a
  host can size a viewport with directly.
- `at_dpi(144)` doubles. It remains a correct answer to "144 dpi" given the
  72-dpi premise `RenderOptions` already carries for every document.

Two alternatives were considered and rejected.

**Normalising each page to a paper size** (fitting to A4 or Letter) makes
`size()` a claim about paper that the archive never made, and makes
`scale: 1.0` resample every image in the document — so the default render is
the one path that cannot be byte-exact.

**Reading a physical size out of the image** — PNG's `pHYs`, JPEG's JFIF
density — is worse, and it is worse in an interesting way. Both are present in
a minority of files and wrong in many (the 72-versus-96 confusion is endemic),
and if two pages in one archive disagree, a reader's page-fit jumps between
them. A consistent convention beats an inconsistent measurement here: the
archive genuinely has no physical size, and inventing a per-page one produces
a document that is unstable for no gain.

### `OpenError` grows a named refusal

`OpenError` has two variants and neither reads right for a container this
build recognises and cannot page. `NotAPdf` means *"not one indirect object
could be found"*, which stays exactly true for random bytes; `Empty` is a
caller's bug. Neither can say "this is a comic archive, and here is why I will
not open it" — and gap 17's finding was that saying which is the feature.

The proposed shape keeps `Copy` and `PartialEq`, which the facade's tests and
Tinker's ported parity test both rely on:

```rust
pub enum OpenError {
    NotAPdf,
    Empty,
    /// The bytes are a container this build recognises and cannot open as a
    /// document. The reason is named rather than collapsed into `NotAPdf`,
    /// because "I do not read CBR" and "this is not a document" are different
    /// answers and a host shows different things for them.
    UnsupportedArchive(ArchiveRefusal),
}

pub enum ArchiveRefusal {
    /// A container this build does not read: RAR, 7z, tar.
    NotAZip,
    /// The central directory could not be located and no local header was
    /// found either.
    Damaged,
    /// Every entry is encrypted, so there is nothing to page.
    Encrypted,
    /// Spanned or multi-disk.
    MultiDisk,
    /// A Zip64 field declares a size or offset the archive cannot contain.
    Zip64OutOfBounds,
    /// It is a valid archive and not one entry produced a page.
    NoImages,
    /// A bound in the table below was spent.
    TooLarge,
}
```

`NotAPdf` is deliberately **not** renamed. Its meaning is unchanged, ruling 12
makes `tinker_parity.rs` name it verbatim, and the only thing that changes is
that failing to be a PDF is no longer the same as failing to be a document —
which its doc comment gains a sentence about.

Both `OpenError` and `ImageData` become `#[non_exhaustive]` in the same commit
that grows them. Neither is today, so both grow breakingly; doing it once now
costs one break instead of one more each for gaps 30 and 31.

### What is refused, and at which level

Ruling 2 degrades rather than fails, and gap 17's finding was that *the
refusal is the feature*: a JBIG2 file that decoded its page-information
segment, found no generic region and returned a blank white page as success
would have been strictly worse than the grey placeholder it replaced. The same
rule applies here and it applies at **two different levels**, which is the
distinction that decides whether this plan produces something honest.

**Archive level — refuse at `open`.** Not a ZIP; damaged past recovery; every
entry encrypted; multi-disk; a Zip64 value the archive cannot contain; a bound
spent; and **an archive that is a valid ZIP with no image entries at all**.
That last one is the important one. Opening it as a zero-page `Document` is
`NotAPdf`'s failure wearing a success costume — a host asks `page_count()`,
gets 0, and shows an empty reader with no error to display. It is refused.

**Page level — a placeholder page, and the page count is unchanged.** An entry
that is recognisably an image and cannot be turned into a real page becomes a
page anyway: a page of the right size carrying the neutral placeholder and a
named warning. That covers a GIF or a WebP, an entry whose CRC failed, an
encrypted entry among unencrypted ones, a PNG whose IHDR is unreadable, and an
image past `MAX_PNG_SAMPLES`.

This is gap 17's ruling one level up, and it is taken for gap 17's own stated
reason. That plan refuses a file with *no* decodable region and **draws** a
file that has a generic region beside an undecodable symbol dictionary,
because refusing the second *"would throw away a picture that decoded
perfectly"*. An archive of 100 JPEGs and one GIF is that file. Refusing it
loses 100 readable pages; dropping the GIF silently is worse still, for the
reason in the next section.

**Entries that are not images at all are not pages and not warnings.**
`ComicInfo.xml`, `Thumbs.db`, `.DS_Store`, `__MACOSX/`, `.nfo`, and directory
entries are metadata and noise; skipping them is correct rather than lenient,
and warning about them would bury the warnings that matter. Classification is
by **magic bytes at the head of the entry**, never by extension, because
`.jpg` files that are PNGs are routine and an extension is a claim rather than
a fact. Extensions are used only for ordering.

## Where a half-implementation is worse than none

Five, and the first is the one this format is uniquely exposed to.

**A missing page that renumbers the rest.** The obvious handling of an entry
that cannot become a page is to skip it. That produces a document that looks
complete: 99 pages where the archive holds 100, every page correct, the page
after the gap sitting where the missing one was, and nothing anywhere saying
so. A reader sees a comic with a story that jumps and blames the scan. This is
this plan's version of gap 17's blank-page-reported-as-success and gap 18a's
plausible photograph — a wrong answer indistinguishable from a right one — and
the defence is that **an unusable image entry still becomes a page**, so the
count is right, the order is right, and the hole is visible.

**Lexicographic ordering.** Covered above and repeated here because it belongs
in this list: every page present, every page correct, the document unreadable,
and no fixture with zero-padded names can see it. A test whose archive is
`page001..page012` proves nothing about ordering and must say so in its own
doc comment, the way gap 16's
`mixed_mode_rows_are_coded_the_way_their_tag_bit_says` does.

**Decoding every PNG.** It works on a twelve-page fixture, passes review, and
allocates 3.6 GB on a real 200-page archive. The failure arrives only at the
size that matters, only on a user's machine, and the profile that would have
caught it looks fine on everything in `testdata/`. The pass-through is not an
optimisation to add later; it is the design, and the decoder exists for the
cases the table above names.

**An unchecked pass-through.** Copying bytes into a PDF stream without
verifying the ZIP entry CRC and the PNG chunk CRCs removes the only decoder
that would ever have noticed the archive is damaged. The page half-draws, the
rest of the document is fine, and no warning exists. Pass-through is what
makes the checksum mandatory rather than optional — the two decisions are the
same decision.

**Shipping the ZIP reader on `flate_decode`.** It works, which is the problem.
Every CBZ acquires a spurious `RawDeflateFallback` per deflated entry and a
spurious `TrailingGarbage` per streamed one, so the warning surface for the
format is noise from the first release, and ruling 10's distinction between
"it opened" and "it opened cleanly" is gone for CBZ before anyone has looked
at it. That is why exposing `inflate_raw` is milestone 1 and not a cleanup.

## Bounds, per ruling 1

Every number below is attacker-controlled, and two scars in this repository
say how to set them.

`5adf502 fix(render): bound the group buffers a page may open, not just their
depth` found an 1 851-byte page that took **19.3 seconds to render 9 600
pixels**, with `MAX_GROUP_DEPTH` in place the entire time: *"depth is not work
once the recursion branches"*. The ZIP version of that sentence is that **a
per-entry output cap is not a total once the entry count is chosen by the
file** — which is what `42.zip` is, and it does not need nesting to be
dangerous.

[18a](18a-jpx-decoder.md)'s milestone 8 found the other failure, in a constant
written specifically to avoid the first: `MAX_JPX_WORK` was set *above* the
most its own inputs could ask for, so it **could never fire**. A cap that
cannot fire is not a cap, and no test that only checks the happy path can tell
the two apart.

So there is a total, spent and never refunded:

| Name | Bounds | Why it cannot be a per-item cap |
| --- | --- | --- |
| `MAX_ZIP_ENTRIES` | Entries enumerated, from the central directory or the local-header scan | The EOCD's entry count is a claim: a 22-byte record can declare four billion |
| `MAX_ZIP_INFLATED` | **The work cap.** Bytes inflated across every entry in one archive | A per-entry `Limits::max_output` times a file-chosen entry count is not a bound |
| `MAX_CBZ_PAGES` | Pages synthesised | A page costs an object graph as well as an entry, and `limits::MAX_PAGES` is 2²¹ — far above anything a comic is |
| `MAX_PNG_SAMPLES` | width × height × components, `checked_mul`'d before any buffer exists | The pattern `ccitt.rs` established and `jbig2.rs` reused as `packed_size` |
| `MAX_SYNTHESISED_PDF` | Bytes of the document the synthesiser hands to `CosDocument::open` | The archive bounds the image data and not the object graph: thousands of one-byte entries are a page tree far larger than their input |

Per-item caps sit beside them — the largest single entry, the largest PNG
dimension, the longest entry name, the deepest stored path — and the comment
on each says in as many words that it is *not* the work cap, in the register
`MAX_SCRIPT_STEPS` and `MAX_MESH_TRIANGLES` already use.

**How each is measured, so it is a number rather than a preference.** For
every constant, three figures go in the `As built`: the most any archive in
this repository legitimately spends, the most a plausible real archive spends
(a 200-page comic at 2000 × 3000), and the constant. The gap between the
second and the third is the safety margin, and writing it down is what would
have caught `MAX_JPX_WORK`. And each cap is proved to fire **by its own
refusal or warning, never by a clock** — `5adf502`'s method, taken for its
stated reason: *"a timing assertion would fail on a slow machine and pass on a
fast one with the budget removed, where the warning says the budget was what
stopped the page."*

Two fuzz targets land with the code that needs them rather than at the end,
per plan 02's rule that every decoder gets one the day it exists: `zip` as the
repository's **nineteenth** and `png` as its **twentieth**.

## `deny.toml` has a hole exactly here

`deny.toml` denies `flate2`, `miniz_oxide`, `png`, `image`, `jpeg-decoder`
and thirty more by name, under a comment saying the hand-rolled rule *"lived
only in prose, so a new dependency that happened to be MIT-licensed would have
passed every check in this file unnoticed"*. It does **not** deny `zip`, and
it does not deny any CRC crate — and `crc32fast` is precisely the *"just a
small helper"* that plan 00's exemption paragraph names as the thing that
slips in during a busy phase. This plan adds a runtime need for both, so both
are denied in it: `zip`, `zip-extract`, `async_zip`, `rc-zip`, `rawzip`,
`crc32fast`, `crc`, `crc32c`, `adler`, `adler2`, `simd-adler32`.

The XML crates gap 30 will need denied are that plan's to add, not this one's.

## Milestones

The commit-boundary rule is per-plan
([00-execution-order.md](00-execution-order.md)); this one is six commits, one
per milestone, each independently green under the full gate.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | The filters-crate seam, before anything consumes it | `inflate_raw` is public with a public result carrying the bytes, whether it finished, whether the ceiling stopped it, its warnings, and **the input bytes consumed to the end of the final block**; `crc32` and a resumable `Crc32` pinned against three published values — `crc32("")` = 0, `crc32("123456789")` = `0xCBF4_3926`, and `crc32("IEND")` = `0xAE42_6082`, which is the constant every PNG in the world ends with; the three costs `flate_bytes` would impose on ZIP data each demonstrated by a test, including a **committed byte string whose first two bytes pass `zlib_header`** so that the two entry points provably disagree about one input; `flate_decode`'s behaviour and every existing filters test unchanged | S |
| 2 | `tinker-pdf-zip` | Central directory located from the EOCD with Zip64's locator and record read, **and local headers scanned as a fallback** when there is no EOCD or its offsets do not land on `PK\x01\x02` — the ladder posture `cos` already takes, with a warning naming which route was used; stored and deflated entries, the latter through milestone 1's entry point and never through `flate_decode`; data descriptors located from the consumed length; CRC-32 compared on **every** entry and a mismatch refusing that entry rather than warning and continuing; a Zip64 value past the archive's own length refused; names decoded as UTF-8 under general-purpose bit 11 and CP437 otherwise; ruling 8 holds — no PDF type in the public API; `cargo run -p xtask -- dag` green with the new node and the `ALLOWED` doc comment carrying this edge's argument; the nineteenth fuzz target | S |
| 3 | The PNG decoder | Signature and chunk walk with the CRC checked on every chunk — a failure on IHDR, PLTE, IDAT or IEND refuses the image, one on an ancillary chunk warns and drops the chunk; every legal colour-type/bit-depth pair decoded and **every illegal pair refused**; IDAT concatenated across chunks before inflating, since one zlib stream spans them; `PLTE` bounds-checked against the bit depth; `tRNS` in all three forms; Adam7 asserted against a hand-built 8 × 8 image whose sixty-four pixels are all distinct, so a transposed pass is not a permutation the test cannot see — gap 17's context-bit lesson, applied to interlace; the twentieth fuzz target; **PngSuite decoded against its published references**, invoked and not committed, with the count recorded — the only PNG material in reach this repository did not write, and gap 17's SerenityOS precedent for how to use it | M |
| 4 | `ImageData` gains a pre-compressed variant | The variant carries width, height, bits per component, a colour space the writer can already emit, the `/Filter` and `/DecodeParms` **already applied to the bytes**, an optional `/Mask` colour-key array and an optional soft-mask sub-image; `maybe_compress` declining a dict that already has a `/Filter` becomes an asserted contract rather than an implementation detail, since the variant depends on it; a non-interlaced PNG's concatenated IDAT reaches a page as `/FlateDecode` with `/Predictor 15`, `/Colors` and `/BitsPerComponent` from IHDR and `/Columns` from its width — **and the page renders pixel-identically to the same PNG decoded by milestone 3 and embedded as `Rgb8`**, which is the test that says the two paths are one picture; `ImageData` and `OpenError` become `#[non_exhaustive]` here, once | S |
| 5 | CBZ → `CosDocument`, and the facade seam | `Document::open` sniffs `PK\x03\x04` at offset zero **and nowhere else**, so a PDF that happens to contain the signature is unaffected; a CBZ opens, `page_count()` equals its image entries, and `page(i).render(&RenderOptions::default())` is byte-identical to the same image embedded by hand; an archive stored in the order `p10, p1, p2` pages as 1, 2, 10; `Page::size()` is the image's pixel dimensions; `Document::cos()` returns the synthesised document, and **saving it produces a file qpdf reads clean**, through the CI job [20](20-linearization-validation.md) already built; every `ArchiveRefusal` variant returned by a fixture built for it; an unusable image entry produces a placeholder page with page count and order unchanged, and an archive of nothing but `ComicInfo.xml` is refused rather than opened as zero pages | M |
| 6 | Bounds, determinism, ledgers, campaign | Every bound in the table fires in a test **by its own warning or refusal, not by a clock**, and each carries three recorded numbers — the most any in-tree fixture spends, the most a 200-page 2000 × 3000 archive spends, and the constant — so `MAX_JPX_WORK`'s failure cannot repeat; the **thirteenth** determinism fingerprint, which is the first whose document is *synthesised* rather than parsed and therefore pins the synthesis as well as the render, reproduced byte-for-byte on `wasm32-wasip1` with none of the other twelve moving; `deny.toml` gains the eleven names above; the ledger sweep below; `cargo fuzz run zip` and `cargo fuzz run png` each survive a session with no crash, no OOM and no timeout | S |

Milestone 1 comes before milestone 2 deliberately, and the ordering is the
point rather than a preference. [18a](18a-jpx-decoder.md)'s M0 did exactly
this — `mq.rs` gained `set_state` in its own commit, before tier-1 existed —
because gap 17 had put the module in place *"precisely so this plan would not
open with a refactor"*. Building the ZIP reader on `flate_decode` and
correcting it afterwards means one commit that ships a known-wrong path, and
gap 16's whole design section exists to say why that is not done here.

Milestone 3 comes before milestone 4 for the reason milestone 4's exit
criterion states: without a decoder there is nothing to compare the
pass-through against, and a pass-through that is never checked against a
decode is a claim nobody has tested.

### The ledger sweep milestone 6 owes

**The leaf count changes here, and it is written in four places** — three
documents and one `const` — none of which the compiler can reach. It has
already drifted once without anybody noticing: `tinker-pdf-math` arrived as a
second-order leaf and not one of the three prose statements moved. Gap 28 made
exactly this kind of sweep for the "PDF engine and always will be" line and
named it as CONTRIBUTING rule 4 work; the same discipline applies here.

The first four entries below are that count. The rest is the ordinary ledger a
new capability owes.

- **`docs/plans/00-architecture.md`**, the DAG diagram and line 41's *"Five
  leaf crates — `filters`, `crypto`, `font`, `color`, `raster`"*, plus
  milestone 3's *"the five leaves"* — a dated in-place amendment, the way that
  document's `cos -> font` amendment is written.
- **`docs/plans/99-consistency.md`** ruling 8, which names the same five.
- **`CONTRIBUTING.md`** rule 3, which names them again.
- **`xtask/src/main.rs`**'s `ALLOWED` and its doc comment.
- **`docs/plans/02-filters.md`**'s non-goals, which gain PNG the way the JBIG2
  and JPX entries were amended — and the amendment must say that PNG arrives
  as a *container* decoder for CBZ rather than as a PDF stream filter, since
  no PDF stream is a PNG file.
- **`docs/STATUS.md`**, where CBZ moves from decided to built, and
  **`README.md`**, whose opening line gap 28 amended to say what this engine
  is becoming.
- **`fuzz/README.md`**, whose seed table gains two rows — and the seeds are
  curated, not a campaign's working state, which is what `d9945a0` is about.

## Dependencies

**Needs first — all landed:**

- [28](28-tinker-integration-decisions.md), for the decision this plan
  implements and the size it was agreed at.
- [16](16-ccitt-completion.md), for the `ccitt_samples` shape the CBZ image
  boundary follows and for the packed-sample conventions.
- [20](20-linearization-validation.md), for the qpdf CI job milestone 5's
  round-trip criterion uses — including its finding that a **skipped** oracle
  test exits 0 and reads exactly like a pass.
- [24](24-fuzz-execution.md) M1–M4 for the fuzz toolchain. `cargo-fuzz` needs
  libFuzzer, which `x86_64-pc-windows-msvc` does not support; WSL2 with
  nightly is the local route, as four other plans record.
- [25](25-wasm-determinism-leg.md) M1–M3 for the leg the thirteenth
  fingerprint is checked on.

**Needs, and is not in the repository:** nothing linked. PngSuite is fetched
for milestone 3's external check and not committed, under the same handling
gap 17 gave the SerenityOS JBIG2 streams.

**Unblocks:** gap 30, XPS, which neither exists nor is written yet. An XPS
document is an OPC package, and an OPC package is a ZIP, so `tinker-pdf-zip`
is built here and used there — that is the second reason it is its own crate
rather than a module in the facade. It does **not** unblock EPUB in any
meaningful sense: EPUB is also a ZIP, and the ZIP is the smallest part of it.

**Amends, in the same commits:** the ledger sweep above.

## Risks

| Risk | Mitigation |
| --- | --- |
| The implementation decodes every PNG, passes on fixtures, and allocates gigabytes on a real archive | Pass-through is the design and the decoder is the named fallback for a table of five cases; milestone 4's exit criterion is the pass-through, and milestone 6 records peak memory for a 200-page archive |
| An unusable entry is skipped, and the document reads as complete with a page missing from the middle | The page-level refusal produces a **page**, not a gap; milestone 5 asserts page count and order are unchanged by it |
| Ordering is lexicographic and every fixture has zero-padded names, so nothing catches it | The comparison is specified here; milestone 5's fixture is stored `p10, p1, p2` and the padded case is called out as proving nothing, the way gap 16's mixed-mode test documents its own blind spot |
| `flate_decode`'s sniff is used because it is there, and every CBZ acquires two spurious warnings | Milestone 1, before the consumer exists — 18a's M0 pattern, and gap 16's reason for refusing to sequence a known-wrong path |
| A cap is set above what its own inputs can reach and never fires | Gap 18a M8's exact failure. Milestone 6 records three numbers per constant and proves each fires by its refusal rather than by a clock |
| A per-entry output cap is treated as the total, and a zip bomb runs | `MAX_ZIP_INFLATED` is a total, spent and never refunded — `5adf502`'s lesson, and `MAX_TILE_WORK`'s, and `MAX_SCRIPT_TOTAL`'s |
| A CRC is read and not compared, so a damaged archive draws half a page silently | The pass-through removes every other check; milestone 2 makes a mismatch refuse the entry and milestone 3 makes it refuse the image |
| `Document` is made an enum by whoever writes `cbz.rs`, and `cos()` becomes fallible | Decided here with the argument written out, before any file exists — the shape gap 18's risk table warned about for the fixed-point width |
| `zip` or `crc32fast` is added as a dependency because the rule lived only in prose | Both denied by name in milestone 6, in the file whose own comment says that is exactly how the rule became enforceable |
| An `As built` that reads as "comic archives work now" | The claim this plan can support is JPEG and PNG in a ZIP; every other container and every other image format is refused **by name**, and the `As built` says which of gap 23's corpora contained one (none of them can) |

## Progress — 17 August 2026

**Milestone 1 has landed.** One commit, in the filters crate, and nothing
consumes any of it yet — which is the point rather than an oversight, and the
reason is the last row but three of the risk table above.

- **`inflate_raw` is public**, as
  `inflate_raw(input: &[u8], limits: &Limits) -> RawInflated`, with the result
  carrying `data`, `complete`, `capped`, `end` and `warnings`. The private
  byte layer it wraps is now called `raw_bytes`, which is the name every other
  filter in that crate gives its own, and `flate_bytes` calls it under the new
  name and is otherwise untouched: every removed line in `inflate.rs` is one
  of the five occurrences of the old identifier.
- **`end` is documented as a ceiling and as conditional.** DEFLATE finishes on
  a bit boundary and every container that carries it resumes on a byte one, so
  a final block ending mid-byte counts that whole byte; and `end` means what
  the doc comment says **only when `complete`**, because a truncated, corrupt
  or capped decode stopped where this decoder gave up rather than where the
  stream ends.
- **`crc32` and `Crc32`**, in `crc32.rs`, table-driven from a `const fn` that
  derives the 256 entries from `0xEDB8_8320` at compile time. The three
  published values are pinned, and a second implementation written the other
  way round — most-significant-bit-first over the unreflected `0x04C1_1DB7` —
  is asserted to agree over every single byte and thirteen lengths, because a
  polynomial reflected the wrong way produces self-consistent wrong answers
  that no amount of testing `crc32` against itself can see.
- **`flate_decode` is unchanged**, which is a claim the diff supports rather
  than a promise: the sniff, the fallback, the Adler check and all 1 666
  existing tests are as they were. This milestone adds a door; it does not
  move the one that was there.

Twenty tests, in `crates/tinker-pdf-filters/tests/containers.rs` and beside
the code, and the workspace stands at **1 686**. The three costs the design
section predicts are each demonstrated: a `RawDeflateFallback` on a plain
deflated entry, a `TrailingGarbage` on one followed by a data descriptor, and
the mis-decode — `08 1D 00 E2 FF ...` is committed as `ZLIB_SNIFF_WITNESS`
and the two doors return two different twenty-nine-byte answers from it. The
sharpest part of that last one was not predicted here and is worth recording:
`flate_decode` does not warn that it took the zlib branch, because
`RawDeflateFallback` is pushed on the *other* path, so its only warning on
the witness is `TruncatedInput` — about a stream that is not truncated.

`end` is the field with no consumer, so it is tested as though it had one.
Every assertion is `input[end..]`, against the bytes that follow the stream,
rather than against a number: a data descriptor is what milestone 2 will find
with it. Injecting an off-by-one **in the public wrapper alone**, in both
directions, fails six tests in that file and **nothing else in the
workspace** — which is the measurement this milestone existed to take, and
gap 18's milestone 6 is why it was taken.

### What milestone 2 needs

- **`tinker-pdf-zip` calls `inflate_raw` and never `flate_decode`.** That is
  the whole reason this milestone is separate, and it is a claim worth a test
  of its own rather than a convention: an entry decoded through the sniffing
  door is detectable from outside, because it arrives carrying a
  `RawDeflateFallback` that nothing did.
- **Read `end` only when `complete` is true.** A capped decode reports where
  the ceiling stopped it, and a consumer that treated that as a stream end
  would look for the data descriptor inside the entry's own bytes.
  `a_capped_decode_says_so_rather_than_reporting_a_stream_end` asserts the
  distinction from this side; the entry reader owes the other half.
- **`Limits::max_output` here is a per-entry cap and is not
  `MAX_ZIP_INFLATED`.** The bounds table says why in as many words. The total
  is the ZIP reader's to keep, spent across every entry and never refunded,
  and passing a per-entry ceiling into `inflate_raw` does not create one.
- **The CRC is compared, not stored.** `Crc32` is resumable so that an entry
  can be checksummed as it is inflated rather than after; `crc32` over a
  reassembled buffer is the same answer and costs a second pass over data the
  pass-through design says will be hundreds of megabytes.
- **The DAG amendment and `deny.toml` are still owed.** Neither moved here:
  `xtask -- dag` is green because no node was added, and the eleven denied
  names are milestone 6's. A `tinker-pdf-zip` that appears without the
  `ALLOWED` edge and its written argument is the failure that file's own
  commentary records.

## Progress — 17 August 2026, milestone 2

**`tinker-pdf-zip` has landed**, as the seventh leaf: `cp437.rs`, `le.rs`,
`limits.rs`, `local.rs`, `dir.rs`, `scan.rs`, `lib.rs`, plus a test-only
archive writer. Forty-seven tests, and the workspace stands at **1 733**.

### The two things the design got wrong, found by writing the tests

**A data descriptor's width cannot be recovered from its bytes.** The first
version of `parse_descriptor` tried the 32-bit shape and then the 64-bit one
and kept whichever agreed with the length the caller already knew — and the
field comment on `LocalHeader::zip64` said, in as many words, that this was
*stronger* than trusting APPNOTE 4.3.9.2's flag, because a writer that sets
the flag wrongly would still be read correctly.

It is not stronger; it is wrong, and wrong in the ordinary case rather than the
exotic one. A 64-bit little-endian 7 begins with the same four bytes as a
32-bit 7. So for **every** entry under 4 GB, reading a 64-bit descriptor as
32-bit succeeds, agrees with the expected length, and silently takes the high
half of the compressed size — a zero — as the uncompressed size. Trying both
and keeping whichever matches therefore always keeps the 32-bit answer, and is
wrong exactly when the width mattered.

The flag now chooses the width and validation confirms it, with the declared
width tried first and the other kept as a fallback for a writer that flagged
it wrongly. A field the compiler had reported as never read was not dead code
to be allowed away; it was the one input the function needed.

**`NoEndOfCentralDirectory` was a variant nothing ever pushed.** Recovery
reported only that it had happened, not which of the two ways the directory
was missing — a writer that stopped before appending it, or a record that is
there and points nowhere. Both fall back to the scan, and a caller reporting
to a human wants to say which. `dir.rs` now pushes it, and
`the_two_ways_a_directory_can_be_missing_are_reported_apart` holds the two
apart in both directions.

### The bounds, and that each one fires

`limits.rs` carries the three numbers per constant that gap 18 milestone 8
made mandatory — what the fixtures here actually spend, what a plausible real
archive spends, and the cap — and the first of the three is **measured** by
`the_fixtures_in_this_crate_spend_what_the_ledger_says` reading
`Archive::inflated()` back, so the ledger cannot drift away from the code in
silence.

| Constant | Fixtures | A 200-page comic | Cap | Proved to fire by |
| --- | --- | --- | --- | --- |
| `MAX_ZIP_ENTRIES` | 6 | 202 | 16 384 | building the real 16 385-entry archive |
| `MAX_ZIP_ENTRY_BYTES` | 1 024 | ~48 MB | 128 MiB | an entry declaring one byte past it |
| `MAX_ZIP_INFLATED` | 1 024 | 0 stored, ~300 MB deflated | 1 GiB | a 64-entry bomb crossing a lowered total |
| `MAX_ZIP_NAME_LEN` | 24 | ~42 | 1 024 | a 1 524-byte name truncating and warning |

There is deliberately **no** cap on path depth, and `limits.rs` says why:
nothing here touches a filesystem, so depth bounds no allocation, no recursion
and no work. A constant for it could never fire, which is gap 18 milestone 8's
failure reached from the other direction.

### What the tests actually catch

Twelve defects injected one at a time, each reverted before the next, the
suite re-run with `--no-fail-fast`. **All twelve are caught; none survived.**

| Defect | Caught by |
| --- | --- |
| Descriptor width guessed rather than declared | `a_descriptor_is_read_in_whichever_of_its_four_shapes_it_has` |
| CRC-32 check dropped | `a_corrupt_entry_is_refused_rather_than_handed_over` |
| Stored entry copied instead of borrowed | `a_stored_entry_is_borrowed_and_a_deflated_one_is_owned` |
| Declared uncompressed size trusted, not bounded | `an_entry_declaring_more_than_the_per_entry_cap_is_refused_before_it_allocates` |
| Archive inflation total never spent | `a_zip_bomb_is_refused_by_name...`, and the ledger test |
| Entry cap warns instead of refusing | `an_archive_with_more_entries_than_the_cap_is_refused_by_name` |
| Over-long name drops the entry | `a_name_past_the_cap_is_truncated_and_says_so` |
| Scan resumes inside an entry's data | `image_bytes_containing_a_local_signature_do_not_become_an_entry` |
| Length check dropped, only the checksum left | `a_short_entry_is_refused_for_its_length_rather_than_its_checksum` |
| Streamed stored entry sized by a guess | `a_streamed_stored_entry_is_listed_and_then_refused` |
| `end` trusted when the decode did not complete | `a_failed_decode_does_not_get_to_name_its_own_extent` |
| Multi-disk archive read in part | `a_multi_disk_archive_is_refused_rather_than_read_in_part` |

Three of those needed a test written for them, and two of the three are worth
recording because the *first* attempt did not work.

**The length check looked redundant and is not.** A short entry fails the CRC
too, so both versions refuse and a test asserting only "refused" cannot tell
them apart — which is precisely what the matrix reported. The two say
different things to whoever reads the error: "this archive promised 5 000
bytes and produced 4 000" points at a truncated file and "checksum wrong"
points at a corrupted one. The more specific one has to come first, and now a
test holds it there.

**The `complete` guard is not observable from an ordinary truncation.** On a
truncated stream the decoder consumes the whole remaining file, so a
descriptor read from `end` runs off the end and fails whether or not the guard
is there — the first version of that test proved nothing, and the matrix said
so. The guard earns its place against an archive *built* for it, because
`expected_compressed` is derived from the same `end` it is meant to check, so
the validation is self-consistent and confirms a number it took on trust. The
fixture is a complete non-final stored block, one byte the decoder must reject
as a block header (BTYPE=11 is reserved), and then a well-formed unsigned
descriptor at exactly the offset the failed decode reports, declaring exactly
that offset as its compressed size. `end` counts the byte the decoder choked
on rather than the one before it, which the fixture had to be corrected for.

### The seams milestone 1 named, and where each stands

- **`inflate_raw`, never `flate_decode`.** Held, and tested rather than
  assumed: `a_deflate_stream_beginning_with_a_stored_block_decodes` builds a
  raw stream whose first two bytes are `08 1D` — a valid CM, a multiple of 31,
  FDICT clear — so it passes every test the sniff applies, and asserts the
  entry still decodes to its twenty-nine bytes.
- **`end` only when `complete`.** Held, and now the one thing in this
  milestone that needed a purpose-built adversarial fixture to prove.
- **The per-entry ceiling is not the total.** Held. `read_deflated` passes
  the declared size as `inflate_raw`'s ceiling and spends it against a
  separate archive budget; `stored_entries_do_not_spend_the_inflation_total`
  pins that stored data charges nothing, because it allocates nothing.
- **The CRC is compared, not stored — but not resumably.** Milestone 1
  expected an entry checksummed *as* it inflated. `inflate_raw` returns a
  finished `Vec` rather than a stream of chunks, so there is no interleaving
  available without giving it a callback shape no other caller wants, and a
  stored entry has no inflation to interleave with at all. It is one pass over
  the finished bytes and the only pass anything makes over them. `Crc32`'s
  resumable form stays worth having for the PNG decoder, whose chunks arrive
  separately by construction. The reason now lives beside the code.
- **The DAG amendment and `deny.toml`.** Both landed here rather than being
  left to milestone 6. `ALLOWED` gains `tinker-pdf-zip -> tinker-pdf-filters`
  with its argument written out in the house style: it is the third amendment
  and the second leaf-to-leaf edge, it cannot cycle because `filters` depends
  on nothing, and a sibling workspace crate is not a third-party dependency.
  `deny.toml` gains eleven names — `zip` and its neighbours, `tar`, and the
  CRC-32 crates — with the note that `zip` is the one CONTRIBUTING rule 1
  would have been most quietly broken by.
- **The nineteenth fuzz target**, `zip_archive`, whose control byte picks the
  *bounds* rather than the input, deliberately away from the shipped defaults:
  a 1 GiB total cannot fire inside a fuzz iteration, so a target using it
  would leave the crate's only work cap unexplored. It asserts four things
  past "it did not panic" — a successful read produced exactly the declared
  length, it spent no more than the total, an entry with no checksum was
  refused rather than read, and the entry list did not change under reading.

Two cheap versions of the fuzzer run on every `cargo test`, because a panic
introduced today should not wait on a fuzz session:
`truncating_a_good_archive_anywhere_never_panics` and
`flipping_any_single_byte_never_panics`.

### Still owed

Every archive here is hand-built from APPNOTE's field layouts. **There is no
`.cbz` in this repository to round-trip**, and gap 17 is the precedent for
saying so plainly rather than implying coverage that does not exist. A real
archive from a real archiver remains worth acquiring before milestone 6
claims the format works.

## Progress — 17 August 2026, milestone 3

**The PNG decoder has landed**, as `crates/tinker-pdf-filters/src/png.rs` with
its tests beside it in `src/png/tests.rs`, plus `tests/png_suite.rs` and the
twentieth fuzz target. Forty-nine tests, and the workspace stands at **1 782**.

### The one design decision this milestone took on its own

The milestone table says "the PNG decoder" and the scope list says the same.
What milestone 4 needs is **two entry points**, and building only one would
have forced the pass-through to inflate a raster in order to read a header —
which is the *w x h x 3* per page this whole plan exists to avoid.

So `png_scan` walks the signature and the chunk structure, checks every CRC and
hands back IHDR, PLTE, `tRNS` and **the concatenated IDAT with nothing
inflated**; `PngScan::decode` is the second half, and `png_decode` is the two
together. The seam costs about ten lines and it is the one milestone 4 will
build `/FlateDecode` with `/Predictor 15` on top of — the header it needs for
`/Colors`, `/BitsPerComponent` and `/Columns` comes out of the same walk that
verified the checksums.

Two smaller shapes were decided here and are recorded so they are not
re-litigated in milestone 4. The decoder **applies** the palette rather than
handing one back, because 11.2.3's bounds check is what makes a palette safe
and it belongs beside the palette rather than in every caller; and it
**applies** `tRNS` in all three forms, producing an alpha channel, because a
caller building an `/SMask` wants an alpha channel and re-deriving one from a
colour key in the facade would put PNG semantics in a place ruling 8 keeps
PDF-free from the other direction.

### The three things found the hard way

**A `TrailingGarbage` check that could never fire, found by the injection
matrix.** The first version compared the inflated length against the raster's
declared length and warned if it was larger. It cannot be larger: the inflate
ceiling *is* the raster's declared length, so the inflater is not permitted to
produce the condition the branch tested for. Disabling the branch failed
nothing, which is how it was caught. It is gone, and what reports an over-long
IDAT is `Warning::OutputCapHit` from the inflater, which
`an_idat_that_inflates_past_the_raster_stops_at_the_raster` asserts. This is
gap 18a milestone 8's failure — a check set above what its own inputs can reach
— arriving as a *warning* rather than as a cap, and the shape transfers.

**The short-row pixel count divided by the bits in a sample, not the bits in a
pixel.** Thirty tests missed it, and the reason is exact: the two expressions
agree on every complete row and on every one-channel image, and almost
everything in this file is one or the other. The wrong count draws a pixel out
of samples that were never in the file — the red of a truncated RGB triple
beside two zeroes, which is a saturated red where the truth is that nothing
arrived. `a_row_that_stops_mid_pixel_places_no_partial_pixel` builds a stream
that is *complete* and a raster that is short, which is a different fixture
from a truncated IDAT and the only one that can see this.

**`tRNS` on greyscale is compared before 13.12's scaling, and only a sub-byte
depth can tell.** The chunk stores its key in two bytes at the image's own bit
depth; a 4-bit sample of 2 is stored as `0x0002` and scales to 34. A decoder
comparing the scaled value against the stored one finds no match anywhere and
produces a fully opaque image, which looks entirely reasonable until somebody
notices the transparency is missing. At depth 8 the two readings coincide, so
an 8-bit fixture proves nothing about it — the same shape as gap 16's
zero-padded-filename problem, one format down.

### Nothing new was written that already existed

- **`inflate.rs`** does the IDAT. PNG is zlib-*wrapped* (10.3) where a ZIP entry
  is raw DEFLATE by definition, so this goes through the sniffing door and gets
  the Adler-32 check milestone 1 was careful to keep on it;
  `the_idat_adler_is_verified_because_png_is_zlib_wrapped` corrupts the checksum
  and nothing else. A `RawDeflateFallback` here **means** something, which is
  exactly the opposite of what it would have meant in the ZIP reader.
- **`predictors.rs`** does the row filters, through the public
  `predictor_decode` with `/Predictor 15`. For an interlaced image it is called
  **once per pass** with that pass's own width as `/Columns`, which is what 7.2
  asks for and needed no new code. Not one line of PNG 9.2 was written here.
- **`crc32.rs`**'s resumable `Crc32` does the chunk checksums, and milestone 1's
  guess about who wanted the resumable form was right for the wrong reason: the
  ZIP reader turned out not to need it, and PNG does — 5.3's CRC covers a
  chunk's type and its data, and those are never adjacent in one buffer.

### The bounds

One cap, and `png.rs`'s module note says in as many words which candidates were
considered and refused — the `tinker-pdf-zip` `limits.rs` form, taken for the
same reason: a constant that can never fire is gap 18a milestone 8's failure
reached from the other direction.

| Constant | This crate's fixtures | PngSuite's largest | A 2000 x 3000 comic page | Cap | Proved to fire by |
| --- | --- | --- | --- | --- | --- |
| `MAX_PNG_SAMPLES` | 4 096 | 6 400 | 24 000 000 | 67 108 864 | a **thirteen-byte IHDR** declaring 2^31-1 square |

The margin over a real page is 2.8x, and the constant is `1 << 26` — the same
as `MAX_JPX_SAMPLES`, and for the same arithmetic: at sixteen bits a component
it is 134 217 728 bytes, `MAX_DECODED_STREAM` to the byte. It is charged at the
*widest* layout the declared colour type can produce, because `tRNS` decides
between three components and four and is not known until later in the same
file. The caller's own `Limits::max_output` sits beside it and refuses under its
own name, `ExceedsOutputLimit`, carrying the caller's number rather than this
crate's — so a host that lowered its ceiling can tell its decision from ours.

**Four caps are deliberately absent**, each named in the module note with its
reason: no chunk-count cap, since a chunk is twelve bytes and the walk allocates
nothing per chunk, so the input length already bounds both the loop and the
work; no IDAT-total cap, since the bytes are copied out of the input; no palette
cap beyond 11.2.3's own `2^bit_depth`; and no per-dimension cap, since a
1 x 2^31 image is refused by the product and a dimension cap would refuse
nothing the sample cap does not.

### PngSuite, and the count

**Obtained.** `PngSuite-2017jul19.zip` from `schaik.com`, extracted to a scratch
directory and **not committed** — ruling 9, and gap 17's SerenityOS handling.
`tests/png_suite.rs` reads `TINKER_PNGSUITE` and prints `pngsuite-oracle: RAN`
or `pngsuite-oracle: SKIPPED`, because gap 20 found that a skipped oracle exits
0 and reads exactly like a pass.

**176 images. 162 decode and 14 are refused, which is precisely the set its
author publishes as corrupted** — and each is refused for the published reason
rather than merely refused: four signature bytes and the CR/LF pair as
`NotPng`, `xhdn0g08` and `xcsn0g01` as `ChunkCrc(IHDR)` and `ChunkCrc(IDAT)`,
`xdtn0g01` as `MissingImageData`, and `xc1`, `xc9`, `xd0`, `xd3` and `xd9` as
`BadColourTypeDepth` carrying both numbers. That is most of this milestone's
refusal list, decided by somebody else.

PngSuite ships no reference rasters, so the references are the ones its author
published in the naming convention and the file groupings — and they are
sharper than a pixel dump would be, because each is a claim about a *feature*:

- **The filename is the header.** 161 files state their colour type, bit depth
  and interlace method in characters 3 to 7, and all 161 agree with what this
  decoder read. The fifteen `basn*` files are exactly Table 11.1's fifteen
  pairs, asserted as a set so a missing pair cannot pass by refusing nothing.
- **`basn` and `basi` are the same image.** Fifteen independent confirmations of
  Adam7 on files from another encoder, plus eighteen more from the
  `s01`..`s40` size series — which is where whole passes fall away, since at
  1 x 1 only the first of the seven holds a pixel. All thirty-three pairs agree.
- **`oi1`, `oi2`, `oi4`, `oi9`** — one zlib stream in one IDAT chunk, in two, in
  four unequal ones, and in a run of **length-one** ones. That last is this
  milestone's concatenation criterion, written by somebody who thought of a
  fixture nobody here would have.
- **Ten equivalence classes** in all, adding the four deflate levels of `z*` and
  the `bKGD`, `sPLT`, `pHYs`, `hIST`, `tEXt`, `zTXt` and `tIME` files a decoder
  must ignore.
- **No leniency at all on 162 well-formed files.** Every one decodes complete
  with an empty warning list, which is ruling 10's distinction being worth
  something for this format rather than noise from the first release.
- **And the suite does not collapse**: 91 distinct rasters from those 162, so
  the forty-odd equalities above are not a decoder returning one picture.

Two groups are deliberately *not* asserted equal, and the file says so rather
than leaving a reader to wonder: the `f*` filter files and the `g*` gamma files
carry different pixel data from each other, so pairing them would assert
something PngSuite never claimed.

**Pillow was also used, once, at fixture-authoring time.** The two committed
8 x 8 fixtures were produced by a Python script — `zlib` for the stream,
`binascii` for the CRCs, the interlacing written from Table 7.1 — and then
decoded by Pillow, which agreed both are 0..63 in row-major order, before either
was committed. That is tooling under CONTRIBUTING rule 1's exemption and nothing
links it; it is recorded because the provenance of the Adam7 fixture is the
whole point of that fixture.

The script is here rather than in the tree, so the 245 committed bytes can be
regenerated and checked against rather than taken on trust. It reproduces both
files byte for byte:

```python
import binascii, zlib
W = H = 8
PIX = [[y * 8 + x for x in range(W)] for y in range(H)]
# Table 7.1: starting row, starting column, row increment, column increment.
P = [(0,0,8,8), (0,4,8,8), (4,0,8,4), (0,2,4,4), (2,0,4,2), (0,1,2,2), (1,0,2,1)]
def ck(k, d):
    return len(d).to_bytes(4,"big") + k + d + (binascii.crc32(k+d) & 0xFFFFFFFF).to_bytes(4,"big")
def pae(a, b, c):
    p = a + b - c
    d = [abs(p-a), abs(p-b), abs(p-c)]
    return a if d[0] <= d[1] and d[0] <= d[2] else (b if d[1] <= d[2] else c)
def flt(rows, pick):
    out, prev = bytearray(), bytes(len(rows[0]))
    for n, r in enumerate(rows):
        t = pick(n)
        f = [0]*len(r)
        for i in range(len(r)):
            a, b, c = (r[i-1] if i else 0), prev[i], (prev[i-1] if i else 0)
            f[i] = (r[i] - [0, a, b, (a+b)>>1, pae(a,b,c)][t]) & 0xFF
        out.append(t); out += bytes(f); prev = r
    return bytes(out)
def png(il, raw):
    ih = W.to_bytes(4,"big") + H.to_bytes(4,"big") + bytes([8,0,0,0,il])
    return (bytes([0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A]) + ck(b"IHDR", ih)
            + ck(b"IDAT", zlib.compress(raw, 9)) + ck(b"IEND", b""))
plain = png(0, flt([bytes(r) for r in PIX], lambda n: n % 5))
raw = bytearray()
for (ys, xs, yi, xi) in P:
    cols, rws = range(xs, W, xi), range(ys, H, yi)
    if len(cols) and len(rws):
        raw += flt([bytes(PIX[y][x] for x in cols) for y in rws], lambda _n: 0)
adam7 = png(1, bytes(raw))
open("plain8x8.png","wb").write(plain); open("adam7_8x8.png","wb").write(adam7)
```

### The injection matrix

Twenty-six defects, one at a time, each reverted before the next, the suite
re-run with `--no-fail-fast` and `TINKER_PNGSUITE` set. **Twenty-four were
caught on the first attempt; two survived, both were real gaps, and both are now
closed** — the two recorded above.

| Defect | Caught by |
| --- | --- |
| Adam7 pass table transposed, row and column roles swapped | `adam7_places_all_sixty_four_distinct_pixels`, and three more |
| Adam7 increments swapped, starting positions left alone | the same four |
| A pass with no pixels still emits a filter tag | `interlaced_images_too_small_for_a_pass_skip_it_entirely` |
| Interlaced files unfiltered as one raster at the full width | `every_interlaced_twin_decodes_to_its_non_interlaced_original`, and four more |
| Chunk CRC computed over the length field as well | thirty-nine tests, including every PngSuite one |
| 5.4's ancillary bit read the wrong way round | `a_crc_failure_refuses_a_critical_chunk_and_drops_an_ancillary_one`, and ten more |
| Only the first IDAT chunk kept | `idat_is_concatenated_across_chunks_before_anything_is_inflated` |
| Table 11.1 given colour type 3 at depth 16 | `table_11_1_admits_fifteen_pairs_and_refuses_every_other` |
| 13.12 scaling by shifting instead of multiplying | `sub_byte_depths_are_scaled_to_the_full_range_not_shifted` |
| 16-bit samples read little-endian | `sixteen_bit_samples_keep_the_networks_byte_order` |
| `tRNS` grey key compared after scaling instead of before | `trns_on_greyscale_keys_the_raw_sample_and_not_the_scaled_one` |
| `tRNS` palette entries past the list made transparent, not opaque | `trns_on_a_palette_gives_each_entry_its_own_alpha` |
| A 16-bit opaque alpha written as `0xFF` rather than `0xFFFF` | the same, and the PngSuite transparency test |
| Colour type 4's alpha channel dropped on the way out | `every_legal_pair_decodes_to_the_samples_the_table_describes` |
| PLTE bounded at a flat 256 rather than against the bit depth | `a_palette_larger_than_the_bit_depth_can_index_is_refused` |
| A palette index past the end silently black | `a_palette_index_past_the_end_is_black_and_says_so` |
| `MAX_PNG_SAMPLES` never charged | `an_image_past_the_sample_cap_is_refused_before_it_allocates` |
| The inflate ceiling taken from the caller rather than the raster | `an_idat_that_inflates_past_the_raster_stops_at_the_raster` |
| The caller's ceiling compared with `>=` instead of `>` | `the_callers_output_ceiling_refuses_with_the_callers_number` |
| 7.2's row width floored instead of rounded up | `the_channel_count_is_table_11_1s_own_column`, and four more |
| A second IHDR replaces the first | `a_repeated_header_or_palette_is_dropped` |
| A missing IEND no longer warns | `a_missing_iend_warns_and_bytes_after_one_warn` |
| IEND does not stop the walk | the same |
| The inflater's warnings discarded at the call site — gap 16's defect | `the_idat_adler_is_verified_because_png_is_zlib_wrapped`, and three more |
| `predictors.rs`'s warnings discarded at the call site | `a_row_that_stops_mid_pixel_places_no_partial_pixel` |
| **Short-row pixel count divided by sample bits, not pixel bits** | **survived** — now `a_row_that_stops_mid_pixel_places_no_partial_pixel` |
| **An over-long inflate absorbed rather than reported** | **survived, and the branch was dead** — removed, and `an_idat_that_inflates_past_the_raster_stops_at_the_raster` holds the condition that is real |

### The twentieth fuzz target

`png`, in the shape `zip_archive` landed: the control byte picks the **bounds**
rather than the input, and one of the four values it can pick is a ceiling of
**one byte**, which is the only value from which `ExceedsOutputLimit` is
reachable at all. A PNG is the right subject for a fuzzer because it carries two
independent length systems over one file — the chunk walk's declared lengths,
which decide where the IDAT is, and IHDR's geometry, which decides how long the
inflated result should be — and a hand-built fixture makes them agree by
construction.

Past "it did not panic" it asserts four things: a decoded raster is exactly its
own declared size, since milestone 4 hands these bytes to a `/Width` and
`/Height` taken from the same header; the scan and the decode agree about
whether a file decodes at all, which is milestone 4's whole premise; a scan that
succeeds produced a usable IDAT and, for colour type 3, a palette inside its bit
depth; and **a roomier ceiling never changes the picture and never turns a
decode into a refusal**. Two cheap versions run on every `cargo test`, as in
milestone 2: `truncating_a_good_file_anywhere_never_panics` and
`flipping_any_single_byte_never_panics`.

### Still owed

- **`cargo fuzz run png` has not been run.** libFuzzer is unavailable on
  `x86_64-pc-windows-msvc`; the WSL2 nightly route is milestone 6's, as it is
  for `zip`.
- **`fuzz/README.md`'s seed table still has no rows for `zip` or `png`**, and
  `docs/plans/02-filters.md`'s non-goals still do not mention PNG. Both belong to
  the ledger sweep milestone 6 owes, and neither moved here — the posture
  milestone 2 took, for the same reason.
- **No PNG in this repository comes from a real comic archive.** PngSuite is 162
  files of deliberate edge cases and this crate's own fixtures are two pixels
  wide; neither is a 2000 x 3000 scan. The pass-through means most real pages
  will never reach this decoder at all, which is why the gap matters less here
  than it would elsewhere — but milestone 6's peak-memory measurement needs one,
  and so does the claim that the format works.
