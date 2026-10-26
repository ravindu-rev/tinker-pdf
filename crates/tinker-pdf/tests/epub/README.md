# The EPUB corpus

Books written by real producers over text authored in this repository,
committed here; and more that **cannot** be committed, fetched by
`fetch-corpus.sh` into a directory outside the tree. This file says which is
which, where each came from, what it demonstrates, and what licence it is
under.

## The licence gate, which is the reason there are two corpora

| Source | Licence | Committable? |
| --- | --- | --- |
| **Project Gutenberg** | Public-domain *text* under a **trademark** licence | **No.** Clause 1.E.1 requires the boilerplate to appear *"whenever any copy … is accessed, displayed, performed, viewed, copied or distributed"*, and 1.E.4 forbids detaching the terms. Clause 1.C permits redistribution *"as long as all references to Project Gutenberg are removed"* — legal, fiddly, and it leaves a fixture nobody can trace |
| **IDPF/W3C `epub3-samples`** | **CC-BY-SA 3.0**, per the repository's own README | **No**: `deny.toml` says *"There is deliberately NO copyleft in this list — not even weak copyleft"*, and share-alike is weak copyleft. **The obvious source of committable EPUBs is barred by this repository's own gate** |
| **W3C `epub-tests`** | **W3C Software and Document License**, at `w3c/epub-tests/LICENSE.md` | **Not as things stand.** The licence itself is permissive, but `W3C-20150513` is not on `deny.toml`'s allowlist, and that list is what `cargo xtask vendor` checks committed data trees against. Committing this corpus needs an allowlist entry first, in the same commit as the files — the shape `deny.toml`'s own OFL-1.1 comment prescribes |
| **A real producer's output on our own text** | Ours | **Yes**, and it is what is here |

The committable route is the one `fuzz/README.md` records for the JPEG 2000
seeds — *"codestreams `opj_compress` made from **our own** 32 × 32 images"* —
and that `tests/xps/README.md` uses for Windows' XPS serialisers. **Author the
text here, run it through a real EPUB producer, commit the output.** What
comes back is a genuine producer's idea of an EPUB, with that producer's OPF
conventions, its container layout, its stylesheet and its doctype habits, over
content nobody else owns.

**Two producers minimum, and that number is not decoration.** A corpus from
one producer is one producer's idea of the format, and Project Gutenberg's
books are all one `ebookmaker`'s. Almost every point at the bottom of this
file is a place where the producers disagree, and none of them would show in
a corpus of one.

## What is committed

Every `.epub` here is produced by `make-corpus.ps1` beside this file, on
Windows 11, from `source/book.md`, `source/figures.md`, `source/typeface.md`,
`source/embedded.css` and the four PNGs under `source/figures/`. The PNGs are
written byte by byte from the PNG specification by that script, so they are
ours without qualification, and they are reproducible.

| File | Bytes | Producer | EPUB | What it demonstrates | SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `pandoc-book-cover.epub` | 9 661 | pandoc 3.10.2 | 3.0 | A navigation document **and** an NCX in one EPUB 3 book, and one picture that is the cover | `c5491d64…3cd172bd` |
| `pandoc-book-nocover.epub` | 8 458 | pandoc 3.10.2 | 3.0 | The same book with **no image entry anywhere** | `9a9bb6a2…d585ab12` |
| `pandoc-book-epub2.epub` | 9 548 | pandoc 3.10.2 | 2.0 | OPF 2.0, read as a compatibility surface, and the **double-quoted** XHTML 1.1 doctype | `644cd5a8…939e88e9` |
| `pandoc-plates.epub` | 8 329 | pandoc 3.10.2 | 3.0 | Three pictures of three different sizes, two **stored** and one deflated, written in **reverse** of the order the book names them | `34eaa4ff…3ece9d17` |
| `calibre-book-cover.epub` | 7 358 | calibre 9.13.0 | 3.0 | A second producer: content documents named `.html`, the package document at the archive root, a `META-INF/` **directory entry**, and no doctype anywhere | `d5785321…ce81347d` |
| `calibre-book-nocover.epub` | 6 243 | calibre 9.13.0 | 2.0 | The second producer with no image entry, and an NCX with no navigation document | `d9830ec0…cecff4a17` |
| `kcc-fixed-layout.epub` | 16 571 | kcc 11.0.1 | 3.0 | **A third producer, and the only pre-paginated book here**: `rendition:layout` on the package with no `prefix` declaration, a per-item viewport on every one of six spine items, `page-spread-left`/`-right` in the itemref properties, and every entry **stored** | `0d78b4ec…f756fcd3` |
| `calibre-embedded-font.epub` | 424 611 | calibre 9.14.0 | 3.0 | **Two `@font-face` rules for one family**, told apart by their weight descriptor, over a face declared `application/vnd.ms-opentype` | `5f8be869…a2b9395d` |
| `pandoc-embedded-font.epub` | 217 380 | pandoc 3.11 | 3.0 | The **same face through a second producer**, declared `font/ttf`, reached through a **two-entry `src` list** whose first entry is a `local()` this reading system cannot satisfy | `3d369d45…240da2ac` |

The full hashes are in `make-corpus.ps1`'s own output, and **the corpus is not
regenerable byte for byte**: all three producers mint a fresh UUID for the
package document's `dc:identifier` on every run, and calibre and KCC stamp a
`dcterms:modified` timestamp, so a second run is a different file.
`.gitattributes` marks `*.epub` binary, because a normalised line ending inside
a **stored** entry would break its CRC-32, and there would be no way back;
`kcc-fixed-layout.epub` stores *every* entry, package document and content
documents included.

### Licences of the producers, and of what they put in

pandoc is GPL-2.0-or-later, calibre is GPL-3.0-only, and **Kindle Comic
Converter is ISC** — read out of `LICENSE.txt` in its own repository rather than
assumed, where it is `Copyright (c) 2012-2025 Ciro Mattia Gonano, 2013-2019
Paweł Jastrzębski, 2021-2023 Darodi and 2023-2025 Alex Xu` under *"Permission to
use, copy, modify, and/or distribute this software for any purpose with or
without fee"*. The third is the most permissive of the three and it makes no
difference, because **none of the three licences touches the output**: a
converter's copyright does not reach the document it converts, which is the same
reading `fuzz/README.md` applies to `opj_compress`, `tests/xps/README.md` to WPF
and Ghostscript, and `tests/cbz/README.md` to the archivers. Nothing of any of
them is vendored, linked or redistributed, and none is a dependency of anything
in the workspace — `make-corpus.ps1` is how these files are made, not something
CI runs.

### The face, which is the one part that is not ours

The face is **Liberation Serif**, regular and bold, from
`crates/tinker-pdf-font/data/liberation` — `OFL-1.1`, release 2.1.5, vendored,
on `deny.toml`'s allowlist and declared in `THIRDPARTY.md`. **Nothing enters
the tree that row does not already cover**, and that is not a coincidence:
calibre ships the identical release in `app/resources/fonts/liberation`, the
machine has no Liberation in `C:\Windows\Fonts`, and `make-corpus.ps1` hands
pandoc this repository's own copy by path. `epub_fonts.rs`'s
`the_face_in_both_books_is_the_vendored_file` asserts the bytes are equal
rather than trusting the coincidence.

**Both books embed the face unmodified, and that is a licence constraint rather
than a preference.** calibre's `--subset-embedded-fonts` would make
`calibre-embedded-font.epub` a small fraction of its size, and the file it
produces is one this repository may not redistribute, in two independent ways:

- **OFL-1.1 clause 3.** `AUTHORS` beside the vendored faces reads *"with
  Reserved Font Name Liberation"*, and clause 3 forbids a Modified Version from
  using a Reserved Font Name. A subset deletes components, which the licence's
  own definition of Modified Version names, and calibre keeps `name` ID 1 as
  `Liberation Serif`.
- **OFL-1.1 clause 2**, which requires every redistributed copy to carry the
  copyright notice and licence, *"in the appropriate machine-readable metadata
  fields"*. The full face carries them in `name` IDs 13 and 14 — *"Licensed
  under the SIL Open Font License, Version 1.1"* and
  `http://scripts.sil.org/OFL`. **calibre's subsetter drops name IDs 7 through
  14**, so the licence grant is not in the file it writes.

So the size of the two font books is the cost of a redistributable face, and
not carelessness to "fix". `the_face_in_both_books_is_the_vendored_file`
asserts IDs 13 and 14 are still there, which is the clause-2 half of this made
mechanical.

**No book here carries a WOFF or WOFF2.** No producer on the machine emits one
— calibre, pandoc and KCC all write plain sfnt — and the manual route is
barred by the same clause 3, because OFL-1.1's definition of a Modified Version
includes *"changing formats"*, so a WOFF of Liberation is a Modified Version
under a Reserved Font Name exactly as a subset is. A WOFF fixture therefore
needs either a producer that ships one or a face with no reserved name — the
same shape of constraint `tests/cbz/README.md` records for RAR 4.
`pandoc-embedded-font.epub`'s `src` list has an entry this reading system
cannot use followed by one it can, so `typeface::load_one`'s walk to the end of
a preference order is exercised by a producer's file rather than only by a
fixture.

## The fixed-layout producer, and the routes that cannot be used

**calibre cannot produce a fixed-layout EPUB.** `ebook-convert comic.cbz
out.epub` writes EPUB 2 with **no `rendition:` metadata at all**;
`--epub-version 3` writes `version="3.0"` and still no `rendition:layout`; and
`ebook-convert --help` lists no fixed-layout, viewport or pre-paginated option
in any form. calibre 9.14.0 reads fixed-layout books and does not write them.

Sigil 2.8.1 can set `rendition:layout`, but only through its GUI — there is no
batch mode that writes one — so a book from it cannot be produced by a script
anybody can re-run, and a corpus whose provenance is "somebody clicked" is not
provenance.

**What works is KCC**, `ciromattia/kcc` 11.0.1, whose `kcc-c2e` comic-to-ebook
converter writes `<meta property="rendition:layout">pre-paginated</meta>` for
every comic it converts. It takes a directory of pictures, so it is fed the same
four PNGs `make-corpus.ps1` writes byte by byte from the PNG specification — no
new content, and the book is our input through their tool in exactly the sense
the others are.

That it is a *comic* converter rather than a general one is worth naming rather
than apologising for. Fixed-layout EPUB in the world is overwhelmingly comics,
manga and children's books; a pre-paginated novel is the rare case. So the book
here is the ordinary shape of the feature rather than a contrived one — and
what it does differently from the hand-built fixtures (below) it does because
that is what the format's actual users produce.

## What is fetched, and never committed

`fetch-corpus.sh` pulls books into `target/epub-corpus` from Project Gutenberg
and from `epub3-samples`, pinned to its `20230704` release. It **refuses** a
destination inside the working tree that is not under `target/`, because a
convenience that put unredistributable books where `git add -A` can reach them
is a licence violation committed by accident.

`crates/tinker-pdf/tests/epub_fetched.rs` reads `TINKER_EPUB_CORPUS` and prints
**`epub-corpus: RAN`** or **`epub-corpus: SKIPPED`**; the CI job greps for the
second and goes red. A skipped test exits 0 and reads exactly like a pass, and
the corpus is not in the repository, so its books can fail to arrive as well as
fail to read, and both look like a green tick.

`TINKER_EPUB_CORPUS` **must be an absolute path.** A test binary's working
directory is its *crate* root, not the workspace root, so
`TINKER_EPUB_CORPUS=target/epub-corpus` resolves to
`crates/tinker-pdf/target/epub-corpus`, finds nothing, prints `SKIPPED` and
exits 0. The fetch script prints the absolute path and says so; `corpus()`
refuses a relative one outright under the flag below.

`TINKER_EPUB_CORPUS_REQUIRED=1` turns a skip into a **failure**, because a
banner is not a gate — one line in a wall of build output reads exactly like a
test that ran. It is opt-in so that a contributor with no network can still run
`cargo test`, and CI sets it, so a job that means to measure the books cannot
go green over none.

The samples are chosen by what each is the only example of, not by size:
`wasteland-otf-obf` and `wasteland-woff-obf` are the only real input for the
two font obfuscations, `regime-anticancer-arabic` is right to left,
`svg-in-spine` puts an SVG document in the spine, and `linear-algebra` is
MathML throughout.

## epubcheck

`EPUBCHECK.tsv` beside this file records epubcheck **5.3.0**'s verdict, run
once under Temurin 21.0.12. It turns the corpus from "files" into "files with
a verdict": **when this engine and a book disagree, epubcheck's recorded
verdict says whose fault it is.** A book it rejects is one this engine is
entitled to refuse; a book it accepts and this engine mis-reads is this
engine's bug. It is read by a person and gates nothing (ruling 13).

**Three books have no verdict at all, and the file says so with a `-`**:
`calibre-embedded-font.epub`, `kcc-fixed-layout.epub` and
`pandoc-embedded-font.epub`. Ruling 13 lets no outside program be run to
adjudicate a document, so none is run to supply one. A row of zeroes for them
would read exactly like a clean verdict, so a count in `EPUBCHECK.tsv` is a
number **or** `-`, `Verdict` holds `Option<u32>` so no arithmetic can confuse
the two, and `epub.rs`'s `no_book_is_quietly_unmeasured` holds the unmeasured
set to exactly those three and refuses a row that is measured in one column and
not another. **For these three books, when this engine and the file disagree,
there is no arbiter** — and for the fixed-layout book that matters most,
because it is the only book here exercising a part of the format nothing
outside this repository has checked this engine's reading of.

Of the six that do have a verdict, five are clean. The sixth is **calibre's own
EPUB 3 cover output**: `WARNING:NAV-011`, *"toc nav must be in reading order"*,
because the generated title page precedes the first entry the navigation
document links to. A default invocation of a mainstream producer does not
produce a clean book.

The fetched corpus cannot carry its own file, so its verdicts are recorded
here: `sample-georgia-cfi.epub` has `ERROR:RSC-020` — a malformed URL — so it
is a book this engine may refuse without apology. `sample-hefty-water.epub` and
`sample-quiz-bindings.epub` carry `WARNING:RSC-017`, and both obfuscated-font
samples carry `INFO:RSC-004`, which is epubcheck saying it could not read the
font.

## The inventory

`INVENTORY.tsv` names every entry of the committed books, with the media type
the package document declares, the ZIP compression method, the local header
offset and both sizes. It is written by `inventory.ps1` through .NET's own
central-directory walk, and `tests/epub.rs`'s `inventory_matches_the_books`
recomputes name, method, header offset and both sizes through
**`tinker-pdf-zip`** on every `cargo test`. So the inventory cannot drift from
the books, and two independent ZIP readers have to agree about every row.

The media-type column is deliberately **not** checked by that test: resolving
one means following `container.xml` to the package document and reading its
manifest, which no ZIP reader does. The column is committed as a table of real
answers — including the fact that one producer's `application/xhtml+xml` items
are named `.html`.

## The conservation record

`CONSERVATION.tsv` beside this file records, per book, the number of
`<itemref>`s its spine holds, the number of **conservable characters** its
content documents hold, how many of those the paginated document actually
carries, and how many pages it has. `tests/epub_conservation.rs` recomputes
every row on every `cargo test`, so the figure is a ratchet rather than a claim:
a change that moves it updates this table in the same commit.

A conservable character is a non-whitespace one. Layout reflows white space by
construction — `css-text-3` §4.1.1 collapses runs of it and a line break replaces
a space — so the stream of non-whitespace characters is the largest stream that
can survive a layout engine, and it is the one the invariant is stated over:
*every character of text in every content document in the spine appears exactly
once in the paginated output, in document order.*

`kcc-fixed-layout.epub` is the row whose two numbers differ: it conserves
**0 of 6**. The six characters are six full stops: KCC writes
`<div style="display:none;">.</div>` into every one of its six content
documents, a workaround for readers that dislike a page with no text on it, and
a full stop that is `display: none` is a character this build is **right** not
to paint. So the invariant holds over text a producer means to show.
`epub_fixed_layout.rs`'s `the_real_books_six_lost_characters_are_six_hidden_full_stops`
is where that is established rather than asserted in a comment. The two claims
the test makes stand: nothing extra reaches a page, and every character is
conserved or missing and nothing else.

## The censuses

### Doctypes

- **The producers disagree about whether to write one at all.** pandoc writes a
  declaration on every content document it produces; calibre writes none, in
  either EPUB version; KCC writes `<!DOCTYPE html>` on all seven of its
  documents.
- **Both quote characters are real, and neither corpus shows both.**
  Gutenberg's EPUB 2 books write the public identifier single-quoted; pandoc
  writes the same identifier **double-quoted**, which the fetched content
  documents never do.
- One public identifier appears anywhere: `-//W3C//DTD XHTML 1.1//EN`, which is
  **not** in EPUB 3.3 Appendix B's closed set — it was banned from EPUB 3
  deliberately.
- **No real document in either corpus carries a `SYSTEM`-only declaration or an
  internal subset.** Fixtures for those two have to be written, not found — and
  the internal subset being absent from every real book is what makes refusing
  it by name cost nothing.

### Named character references

**None**, across every content document of both corpora, and the committed
producers write no numeric reference either: the em dash, the ellipsis, the
non-breaking hyphen and the Japanese line in `source/book.md` all reach the
content documents as literal UTF-8.

### CSS properties

Real books use multi-column — `column-count`, `column-gap`, `column-rule`,
`column-fill` and their `-webkit-` and `-moz-` spellings — and also
`box-shadow`, `text-shadow`, `border-radius`, `content`, `visibility`,
`table-layout`, `border-collapse`, `word-wrap`, `-epub-text-emphasis-style`,
and Antenna House's `-ah-margin-start` / `-ah-margin-end`.

Both producers write `page-break-before` and `page-break-after`.

## What the real books show

1. **pandoc puts a seventh file in `META-INF`.** Every pandoc book carries
   `META-INF/com.apple.ibooks.display-options.xml`, which is not one of
   §4.2.6.3's six reserved names. **None of the fetched books carries anything
   unreserved there at all.** A reader that refused an unrecognised `META-INF`
   entry would refuse every book pandoc writes and pass the entire downloaded
   corpus.
2. **calibre writes a `META-INF/` directory entry** — a name ending in `/`,
   zero bytes, **deflated**. pandoc writes none. A `META-INF` walk that treated
   every entry there as a file meets an empty one first.
3. **A content document is not named `.xhtml`.** calibre writes
   `index_split_000.html` and declares it `application/xhtml+xml` in the
   manifest. The extension is a claim; the manifest is the fact.
4. **The package document is not always under a directory.** calibre puts
   `content.opf` at the archive root; pandoc puts it under `EPUB/`, and the
   fetched corpus uses `OEBPS/` and `OPS/` too. §4.2.6.3.1 defines `full-path`
   as a path from the **container root**, so it is the one reference in this
   format whose base is *not* the document it is written in; resolving it
   against `META-INF/container.xml` yields `META-INF/EPUB/content.opf`, which
   no book here holds. The general §4.2.5 rule is what the manifest's `href`s
   use.
5. **One producer's EPUB 3 has no NCX and its EPUB 2 has no navigation
   document; the other's EPUB 3 has both.** A reader that expects one of the two
   to be present sees a different book from each producer.
6. **Both ZIP methods appear inside one archive, from one producer, in one
   run.** pandoc stores two of `pandoc-plates.epub`'s three PNGs and deflates
   the third; it is what a producer that measures does.
7. **The pictures are written in reverse of the order the book names them.**
   `file2.png`, `file1.png`, `file0.png`, by header offset. Physical order,
   directory order and spine order are three different orders, and this is the
   file that says so.
8. **A default invocation of a mainstream producer does not produce a clean
   book** — calibre's EPUB 3 cover output warns `NAV-011` under epubcheck 5.3.0.
9. **No real book distinguishes `header_offset == 0` from `index == 0`.** Every
   one puts `mimetype` first in both orders, so the wrong check passes every one
   of them. `tests/epub.rs` builds the container that does distinguish them, so
   the corpus-wide assertion is a measurement rather than a tautology.

## What the fixed-layout and font books show

### The fixed-layout book against the fixtures

The fixtures in `epub_fixed_layout.rs` are hand-built OCF, each written from
§8.2's own sentences. The real book differs from them like this:

1. **A real producer declares `rendition:layout` with no `prefix` attribute at
   all.** Every fixture in `epub_fixed_layout.rs` writes
   `prefix="rendition: http://www.idpf.org/vocab/rendition/#"` on `<package>`,
   because that is what the specification's own example shows. EPUB 3.3 §5.4.3
   makes `rendition:` a **reserved** prefix that need not be declared, and KCC
   does not declare it. A build that required the declaration would pass every
   fixed-layout fixture and **reflow every fixed-layout book in circulation** —
   a complete-looking book of the wrong shape.
   `the_real_book_declares_the_layout_with_no_prefix_declaration` asserts both
   halves: the declaration really is absent, and the book is read as
   pre-paginated anyway.
2. **The layout is on the package and on no itemref, and what the itemrefs carry
   instead is a *different* rendition property in the same attribute.** Every
   fixture exercising §8.2.2's override puts `rendition:layout-pre-paginated` in
   `itemref/@properties`. The real book puts `page-spread-left` and
   `page-spread-right` there — six of them, alternating — and nothing about
   layout, so a build that matched by prefix, or treated an unrecognised token
   there as a defect, meets all six at once. `rendition:spread` is written too,
   in the metadata, which this build has no notion of and correctly ignores.
3. **Four pictures make six pages.** KCC's double-page splitter cuts the two
   landscape plates in half. The spine is not the picture count, the page count
   is not the input count — the fixed-layout analogue of
   `tests/cbz/README.md`'s point about natural order, one format over.
4. **Six viewports, six different sizes, and not one is the size of the picture
   that produced it.** 117 × 177, 30 × 40, 29 × 36, 40 × 50, 39 × 45, 40 × 88 —
   KCC resizes the cover from 120 × 180 to fit its device profile's aspect ratio
   and halves the two it splits. `two_fixed_chapters_may_be_two_different_page_sizes`
   makes the per-item-viewport claim on three documents this repository wrote;
   this makes it on six a producer chose the numbers for, which is the stronger
   version of the same sentence. Six *distinct* sizes is what makes it visible:
   a build that read one viewport and used it for the book would give six equal
   pages, and a page count would not notice.
5. **The legacy spelling is written beside the modern one.** The package carries
   `<meta name="fixed-layout" content="true"/>` — Kindle's KF8 spelling — as well
   as `rendition:layout`, plus seven more `name`/`content` metas
   (`original-resolution`, `book-type`, `primary-writing-mode`, `zero-gutter`,
   `zero-margin`, `orientation-lock`, `region-mag`) that belong to no EPUB
   vocabulary. Two spellings of one fact in one file is what a producer targeting
   two ecosystems writes, and a build that read the legacy one would agree with
   this book and disagree with every book that only writes the modern one.
6. **A real producer's fixed-layout book trips neither of this build's two
   fixed-layout warnings — nor any warning at all.** Every fixture that produces
   `FixedLayoutWithoutViewport` or `FixedLayoutContentClipped` is built to
   produce it, and no fixture can answer whether an *ordinary* book trips them
   anyway. A viewport grammar read a shade too strictly, or an initial
   containing block computed a fraction of a point small, would warn on every
   page of every real book and every fixture would still pass.
7. **Every page is a picture.** It is the only committed book whose content is
   pictures; the others are text.
   `every_page_of_the_real_book_is_its_picture_in_more_than_one_colour` holds
   §8.1.2's clip on each page, requires one `/XObject` placement per page at the
   viewport's own size, and then *renders* each page and counts its distinct
   colours — because a content stream naming a resource proves nothing about
   whether a reader can resolve the name, and only pixels tell a drawn page from
   a blank one. The sixth page's black `<body>` background is asserted by name,
   because a black rectangle under a picture is exactly what a page would look
   like if the picture were missing and the background were the second colour.

### The two font books

8. **calibre embeds only the faces the document reaches, and writes a rule per
   face with three descriptors.** `--embed-all-fonts` over a document with an
   `<h1>` and paragraphs embeds regular and bold and nothing else, and writes
   `font-weight`, `font-style` **and** `font-stretch` on each rule. The other
   route, `--embed-font-family`, embeds all four faces of the family whether the
   book uses them or not, and writes only the descriptors that differ. Half the
   bytes and more descriptors, which is why the committed book is the first.
9. **A real `src` list has more than one entry.**
   `pandoc-embedded-font.epub`'s rule is
   `local("Liberation Serif"), url(…) format("truetype")` — a name the reading
   system might have installed, then the file the book brought, which is what a
   rule out in the world looks like. `typeface::load_one`'s documentation says
   the list is walked to the end because §4.3 is a preference order. The book
   reports one `LocalUnavailable` and **not** `NoUsableSource`, and the face is
   on the page: the entry defect says what failed, the absence of the rule-level
   defect says the rule succeeded. `FONTS.tsv`'s only non-zero `face-defects`
   cell is that one, and it is there on purpose.
10. **The two producers declare byte-identical files under two different media
    types.** calibre writes `application/vnd.ms-opentype`, which OCF 3.0 named
    and EPUB 3.3 dropped; pandoc writes `font/ttf`, which is RFC 8081's. A build
    that decided what a manifest item is from its declared media type would read
    one of these books and not the other; a build that decided from the
    extension would read both and be right by accident. It is the `.html`
    content-document point above one item type further out, and it takes a
    second producer to see it.
11. **pandoc does not rewrite the author's `url()`, and puts the stylesheet and
    the face in different directories.** `--epub-embed-font` writes the file to
    `EPUB/fonts/` and copies the `--css` sheet verbatim to
    `EPUB/styles/stylesheet1.css`, so a rule written as though the two sat
    together resolves to nothing. `source/embedded.css` says `../fonts/…` for
    that reason and records it. §4.2.5's "resolve against the referring
    document" is the referring *stylesheet* here, not the content document and
    not the package.

## Regenerating

```text
pwsh -NoProfile -ExecutionPolicy Bypass \
    -File crates/tinker-pdf/tests/epub/make-corpus.ps1 \
    -Pandoc <path to pandoc> \
    -EbookConvert <path to ebook-convert> \
    -KccC2e <path to kcc-c2e>
pwsh -NoProfile -ExecutionPolicy Bypass \
    -File crates/tinker-pdf/tests/epub/inventory.ps1
sh crates/tinker-pdf/tests/epub/fetch-corpus.sh
```

The first is not byte-reproducible and the committed files are the record. The
second must be re-run whenever the first is, or `inventory_matches_the_books`
goes red — which is the point of it.

**`pwsh` 7, not Windows PowerShell 5.1**, and `-ExecutionPolicy Bypass` when
running either by path, because scripts are disabled by default on the machine
these were produced on. `inventory.ps1` writes LF with no byte-order mark
through `[System.IO.File]::WriteAllText`: `Set-Content` writes CRLF,
`.gitattributes` normalises it back on commit, and every regeneration then shows
a whole-file diff that changes nothing.

**To add a book without disturbing the ones already here**, pass `-OutDir` a
directory outside the tree and copy in only what is new. It is the only safe
way while the corpus is not reproducible.
