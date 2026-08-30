# The EPUB corpus (gap 31, milestone 1; gap 31, tier 4)

Nine books, written by three real producers over text authored in this
repository, committed here; and twenty more that **cannot** be committed,
fetched by `fetch-corpus.sh` into a directory outside the tree. This file says
which is which, where each came from, what it demonstrates, and what licence it
is under.

**Six landed at milestone 1 and three at tier 4**, and the split is the point
rather than an accident of scheduling. Milestone 1 closed owing two rows it
could not fill from the producers it had: a **fixed-layout** book from a real
producer, and a book carrying **a real producer's font** through the
`@font-face` path. `docs/features/epub.md` carried both as a stated caveat and
`docs/ROADMAP.md` carried them into tier 4. The last three rows of the table
below are that debt paid, and between them those books found eleven things the
first six could not — including this build's largest gap in the format, which is
written down here as a test somebody has to delete rather than as a note.

This milestone is scheduled **before any EPUB code exists**, and that is the
point of it. Gap 29 closed having never
opened a `.cbz` a real archiver produced — three milestones owed it, and the
gap's closing statement had to record it as a limitation of the whole gap.
Gap 30 answered that structurally, by
obtaining eight genuine packages in milestone 1 *before a reader existed*, and
its Progress sections record seven things real files did that ECMA-388 did not
predict — two of which would have produced a reader that refuses every OpenXPS
file Windows writes. This is that device a third time.

## The licence gate, which is the reason there are two corpora

Gap 31's plan measured this before a
line of it was written, and milestone 1 confirmed the one row it left open.

| Source | Licence | Committable? |
| --- | --- | --- |
| **Project Gutenberg** | Public-domain *text* under a **trademark** licence | **No.** Clause 1.E.1 requires the boilerplate to appear *"whenever any copy … is accessed, displayed, performed, viewed, copied or distributed"*, and 1.E.4 forbids detaching the terms. Clause 1.C permits redistribution *"as long as all references to Project Gutenberg are removed"* — legal, fiddly, and it leaves a fixture nobody can trace |
| **IDPF/W3C `epub3-samples`** | **CC-BY-SA 3.0**, per the repository's own README | **No**, and this is the finding worth recording: `deny.toml` says *"There is deliberately NO copyleft in this list — not even weak copyleft"*, and share-alike is weak copyleft. **The obvious source of committable EPUBs is barred by this repository's own gate** |
| **W3C `epub-tests`** | **W3C Software and Document License** — verified 19 August 2026 at `w3c/epub-tests/LICENSE.md`, where the plan had "unverified, probably" | **Not as things stand.** The licence itself is permissive. `deny.toml`'s allowlist holds eleven identifiers and **none of them is `W3C` or `W3C-20150513`**, and that list is what `cargo xtask vendor` checks committed data trees against. Committing this corpus needs an allowlist entry first, in the same commit as the files — the shape `deny.toml`'s own OFL-1.1 comment prescribes |
| **A real producer's output on our own text** | Ours | **Yes**, and it is what is here |

The committable route is the one `fuzz/README.md` already records for the JPEG
2000 seeds — *"codestreams `opj_compress` made from **our own** 32 × 32
images"* — and that gap 30 used for Windows' XPS serialisers. **Author the text
here, run it through a real EPUB producer, commit the output.** What comes back
is a genuine producer's idea of an EPUB, with that producer's OPF conventions,
its container layout, its stylesheet and its doctype habits, over content nobody
else owns.

**Two producers minimum, and that number is not decoration.** Gap 30 closed
owing one package produced by something that is not Windows, so its corpus is
one vendor's idea of the format. The equivalent trap here is that Project
Gutenberg's six books are one `ebookmaker`'s. Almost every finding at the bottom
of this file is a place where the two producers disagree, and none of them would
exist in a corpus of one.

## What is committed

708 159 bytes. Every `.epub` here was produced by `make-corpus.ps1` beside this
file — the first six on **19 August 2026** and the last three on **29 August
2026**, both on Windows 11 — from `source/book.md`, `source/figures.md`,
`source/typeface.md`, `source/embedded.css` and the four PNGs under
`source/figures/`. The PNGs are written byte by byte from the PNG specification
by that script, so they are ours without qualification.

**The three tier-4 books were made without regenerating the six.** The script
was run with `-OutDir` pointing outside the tree and only the new files copied
in. The corpus is not byte-reproducible — see below — so a plain run would have
replaced six committed books in order to add three, and those six would have
changed for a reason nobody could later reconstruct. The four PNGs *are*
reproducible, and an empty `git status` for them after that run is what says so.

| File | Bytes | Producer | EPUB | What it demonstrates | SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `pandoc-book-cover.epub` | 9 661 | pandoc 3.10.2 | 3.0 | A navigation document **and** an NCX in one EPUB 3 book, and one picture that is the cover | `c5491d64…3cd172bd` |
| `pandoc-book-nocover.epub` | 8 458 | pandoc 3.10.2 | 3.0 | The same book with **no image entry anywhere** — the `NoImages` half of the defect | `9a9bb6a2…d585ab12` |
| `pandoc-book-epub2.epub` | 9 548 | pandoc 3.10.2 | 2.0 | OPF 2.0, which this plan reads as a compatibility surface, and the **double-quoted** XHTML 1.1 doctype | `644cd5a8…939e88e9` |
| `pandoc-plates.epub` | 8 329 | pandoc 3.10.2 | 3.0 | Three pictures of three different sizes, two **stored** and one deflated, written in **reverse** of the order the book names them | `34eaa4ff…3ece9d17` |
| `calibre-book-cover.epub` | 7 358 | calibre 9.13.0 | 3.0 | A second producer: content documents named `.html`, the package document at the archive root, a `META-INF/` **directory entry**, and no doctype anywhere | `d5785321…ce81347d` |
| `calibre-book-nocover.epub` | 6 243 | calibre 9.13.0 | 2.0 | The second producer with no image entry, and an NCX with no navigation document | `d9830ec0…cecff4a17` |
| `kcc-fixed-layout.epub` | 16 571 | kcc 11.0.1 | 3.0 | **A third producer, and the only pre-paginated book here**: `rendition:layout` on the package with no `prefix` declaration, a per-item viewport on every one of six spine items, `page-spread-left`/`-right` in the itemref properties, and every entry **stored** | `0d78b4ec…f756fcd3` |
| `calibre-embedded-font.epub` | 424 611 | calibre 9.14.0 | 3.0 | **Two `@font-face` rules for one family**, told apart by their weight descriptor, over a face declared `application/vnd.ms-opentype` | `5f8be869…a2b9395d` |
| `pandoc-embedded-font.epub` | 217 380 | pandoc 3.11 | 3.0 | The **same face through a second producer**, declared `font/ttf`, reached through a **two-entry `src` list** whose first entry is a `local()` this reading system cannot satisfy | `3d369d45…240da2ac` |

The full hashes are in `make-corpus.ps1`'s own output and are reproducible only
in the sense a hash of a *committed* file always is: **the corpus is not
regenerable byte for byte.** All three producers mint a fresh UUID for the
package document's `dc:identifier` on every run, and calibre and KCC stamp a
`dcterms:modified` timestamp, so a second run is a different file. That is gap
30's situation exactly, and `.gitattributes` gains `*.epub binary` for gap 30's
reason: a normalised line ending inside a **stored** entry would break its
CRC-32, and there would be no way back. `kcc-fixed-layout.epub` is the file that
makes that clause bite rather than merely apply — it is the first book here
whose *every* entry is stored, package document and content documents included.

### Licences of the producers, and of what they put in

pandoc is GPL-2.0-or-later, calibre is GPL-3.0-only, and **Kindle Comic
Converter is ISC** — read out of `LICENSE.txt` in its own repository rather than
assumed, where it is `Copyright (c) 2012-2025 Ciro Mattia Gonano, 2013-2019
Paweł Jastrzębski, 2021-2023 Darodi and 2023-2025 Alex Xu` under *"Permission to
use, copy, modify, and/or distribute this software for any purpose with or
without fee"*. The third is the most permissive of the three and it makes no
difference, because **none of the three licences touches the output**: a
converter's copyright does not reach the document it converts, which is the same
reading `fuzz/README.md` applies to `opj_compress`, gap 30 applied to WPF and
Ghostscript, and `tests/cbz/README.md` applies to four archivers. Nothing of any
of them is vendored, linked or redistributed, and none is a dependency of
anything in the workspace — `make-corpus.ps1` is how these nine files were
obtained, not something CI runs.

### The face, which is the one part that is not ours

Milestone 1 could write *"no font is involved … there is nothing here anybody
has to licence"*. Tier 4's whole point is a real producer's font, so that
sentence is gone and this is what replaces it.

The face is **Liberation Serif**, regular and bold, from
`crates/tinker-pdf-font/data/liberation` — `OFL-1.1`, release 2.1.5, already
vendored, already on `deny.toml`'s allowlist and already declared in
`THIRDPARTY.md`. **Nothing new enters the tree that row does not already
cover**, and that is not a coincidence: calibre ships the identical release in
`app/resources/fonts/liberation`, the machine has no Liberation in
`C:\Windows\Fonts`, and `make-corpus.ps1` hands pandoc this repository's own copy
by path. `epub_fonts.rs`'s `the_face_in_both_books_is_the_vendored_file` asserts
the bytes are equal rather than trusting the coincidence.

**Both books embed the face unmodified, and that is a licence constraint rather
than a preference.** calibre's `--subset-embedded-fonts` takes
`calibre-embedded-font.epub` from 424 611 bytes to **11 335** — a thirty-seven
fold saving, and the file it produces is one this repository may not
redistribute, in two independent ways:

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

So the 660 KB the two font books cost is the price of a redistributable face,
and it is recorded here so that nobody later reads the size as carelessness and
"fixes" it. `the_face_in_both_books_is_the_vendored_file` asserts IDs 13 and 14
are still there, which is the clause-2 half of this made mechanical.

**WOFF and WOFF2 are still owed and could not be supplied here.** The roadmap
row asks for a real producer's font through `@font-face` *"(WOFF/WOFF2 are
refused by name)"*, and the first half is now here while the second is not: no
producer on this machine emits a WOFF — calibre, pandoc and KCC all write plain
sfnt — and the obvious manual route is barred by the same clause 3, because
OFL-1.1's definition of a Modified Version includes *"changing formats"*, so a
WOFF of Liberation is a Modified Version under a Reserved Font Name exactly as a
subset is. A WOFF fixture therefore needs either a producer that ships one or a
face with no reserved name; `tests/cbz/README.md` records RAR 4's equivalent, and
this is the same shape of constraint discovered the same way. What *is* here is
the refusal path's neighbour: `pandoc-embedded-font.epub`'s `src` list has an
entry this reading system cannot use followed by one it can, so
`typeface::load_one`'s walk to the end of a preference order is exercised by a
committed file rather than only by a fixture.

## The fixed-layout producer, and the route that could not be used

**calibre cannot produce a fixed-layout EPUB, and it was tried first.** It is
the producer already in this corpus, so a fixed-layout book from it would have
cost nothing and compared cleanly against the five reflowable books beside it.
`ebook-convert comic.cbz out.epub` writes EPUB 2 with **no `rendition:`
metadata at all**; `--epub-version 3` writes `version="3.0"` and still no
`rendition:layout`; and `ebook-convert --help` lists no fixed-layout, viewport
or pre-paginated option in any form. calibre 9.14.0 reads fixed-layout books and
does not write them. This is recorded rather than left to be rediscovered, the
way `tests/xps/README.md` records the XPS printer route that needed elevation.

Sigil 2.8.1 is installed on the same machine and *can* set `rendition:layout`,
but only through its GUI — there is no batch mode that writes one — so a book
from it could not be produced by a script anybody could re-run, and a corpus
whose provenance is "somebody clicked" is not provenance. It was not used.

**What works is KCC**, `ciromattia/kcc` 11.0.1, whose `kcc-c2e` comic-to-ebook
converter writes `<meta property="rendition:layout">pre-paginated</meta>` for
every comic it converts. It takes a directory of pictures, so it is fed the same
four PNGs `make-corpus.ps1` already writes byte by byte from the PNG
specification — no new content, and the book is our input through their tool in
exactly the sense the other eight are.

That it is a *comic* converter rather than a general one is worth naming rather
than apologising for. Fixed-layout EPUB in the world is overwhelmingly comics,
manga and children's books; a pre-paginated novel is the rare case. So the book
here is the ordinary shape of the feature rather than a contrived one — and the
four things at the bottom of this file that it does differently from the
hand-built fixtures are things it does because that is what the format's actual
users produce.

## What is fetched, and never committed

`fetch-corpus.sh` pulls twenty books into `target/epub-corpus` — six from
Project Gutenberg and fourteen from `epub3-samples`, pinned to the `20230704`
release. It **refuses** a destination inside the working tree that is not under
`target/`, because a convenience that put unredistributable books where
`git add -A` can reach them is a licence violation committed by accident.

`crates/tinker-pdf/tests/epub_fetched.rs` reads `TINKER_EPUB_CORPUS` and prints
**`epub-corpus: RAN`** or **`epub-corpus: SKIPPED`**; the CI job greps for the
second and goes red. Gap 20 found that a skipped oracle exits 0 and reads
exactly like a pass, and that matters more here than for any oracle before it:
the corpus is not in the repository, so a test over it can fail to run for a
second reason as well as the first, and both look like a green tick.

The fourteen samples are chosen by what each is the only example of, not by
size: `wasteland-otf-obf` and `wasteland-woff-obf` are milestone 9's only real
input for the two font obfuscations, `regime-anticancer-arabic` is the RTL
refusal, `svg-in-spine` is a non-goal that has to be recognised before it can be
refused, and `linear-algebra` is 94 content documents of MathML.

## epubcheck

`EPUBCHECK.tsv` beside this file records epubcheck **5.3.0**'s verdict for every
committed book, run on 19 August 2026 under Temurin 21.0.12. This is what turns
the corpus from "files" into "files with a verdict": **when this engine and a
book disagree, epubcheck says whose fault it is.** A book it rejects is one this
engine is entitled to refuse; a book it accepts and this engine mis-reads is
this engine's bug.

**Three of the nine have no verdict at all, and the file says so with a `-`.**
The tier-4 books arrived on 29 August 2026, by which time epubcheck was no
longer installed and ruling 13 forbade installing it to find out: no external
program may adjudicate a document here. A row of zeroes for them would have read
exactly like the five clean verdicts above it, and the entire value of a dated
measurement is knowing which rows are dated — so a count in `EPUBCHECK.tsv` is a
number **or** `-`, `Verdict` holds `Option<u32>` so no arithmetic can confuse the
two, and `epub.rs`'s `no_book_is_quietly_unmeasured` holds the unmeasured set to
exactly those three and refuses a row that is measured in one column and not
another. What this costs is real and is stated rather than hidden: **for these
three books, when this engine and the file disagree, there is no arbiter** — and
for the fixed-layout book in particular that is a live risk, because it is the
only book here exercising a part of the format nothing outside this repository
has ever checked our reading of.

Of the six that do have a verdict, five are clean. The sixth is **calibre's own
EPUB 3 cover output**:
`WARNING:NAV-011`, *"toc nav must be in reading order"*, because the generated
title page precedes the first entry the navigation document links to. A default
invocation of a mainstream producer does not produce a clean book, which is
worth knowing before this engine's first disagreement with one.

The fetched corpus, recorded here because it cannot be committed and therefore
cannot carry its own file: eighteen of twenty clean, and the two exceptions are
the useful ones. `sample-georgia-cfi.epub` has **seven `ERROR:RSC-020`** — a
malformed URL — so it is a book this engine may refuse without apology.
`sample-hefty-water.epub` and `sample-quiz-bindings.epub` carry
`WARNING:RSC-017`, and both obfuscated-font samples carry `INFO:RSC-004`, which
is epubcheck saying it could not read the font — the same fact milestone 9 will
have to de-obfuscate its way past.

## The inventory

`INVENTORY.tsv` names all **118** entries of the nine books, with the media type
the package document declares, the ZIP compression method, the local header
offset and both sizes. It is written by `inventory.ps1` through .NET's own
central-directory walk, and `tests/epub.rs`'s `inventory_matches_the_books`
recomputes name, method, header offset and both sizes through
**`tinker-pdf-zip`** on every `cargo test`. So the inventory cannot drift from
the books, and two independent ZIP readers have to agree about all one hundred
and eighteen rows.

The media-type column is deliberately **not** checked by that test, for gap 30's
reason in a different format: resolving one means following `container.xml` to
the package document and reading its manifest, which is milestone 3's and
milestone 4's work. The column is committed so that when they land they have a
table of real answers to check against — including the fact that one producer's
`application/xhtml+xml` items are named `.html`.

## The conservation record

`CONSERVATION.tsv` beside this file records, per book, the number of `<itemref>`s
its spine holds, the number of **conservable characters** its content documents
hold, how many of those the paginated document actually carries, and how many
pages it has. Gap 31 milestone 4 built the harness that measures it, and
`tests/epub_conservation.rs` recomputes every row on every `cargo test`.

A conservable character is a non-whitespace one. Layout reflows white space by
construction — `css-text-3` §4.1.1 collapses runs of it and a line break replaces
a space — so the stream of non-whitespace characters is the largest stream that
can survive a layout engine, and it is the one gap 31's invariant is stated over:
*every character of text in every content document in the spine appears exactly
once in the paginated output, in document order.*

**The conserved column is `0` for every book today**, because milestone 4's pages
are placeholders. That is the point of committing the file rather than asserting
a boolean: a milestone that lays text out has to re-measure and update this table
in the same commit, so the figure is a ratchet rather than a claim.

*Amended by milestone 8 and again by tier 4.* Milestone 8 made the figure total
— every character of every book — and tier 4 broke that with one row, honestly.
`kcc-fixed-layout.epub` conserves **0 of 6**, and it is the only row here whose
two numbers differ. The six characters are six full stops: KCC writes
`<div style="display:none;">.</div>` into every one of its six content
documents, a workaround for readers that dislike a page with no text on it, and
a full stop that is `display: none` is a character this build is **right** not
to paint. So milestone 8's invariant was never quite what it said — it was
"every character reaches a page" *plus* an unexamined assumption that no
producer writes text it does not mean to show, and the first real fixed-layout
book in this corpus writes six of them.
`epub_fixed_layout.rs`'s `the_real_books_six_lost_characters_are_six_hidden_full_stops`
is where that is established rather than asserted in a comment. The two claims
the test actually makes are untouched: nothing extra reaches a page, and every
character is conserved or missing and nothing else.

## The censuses

### Doctypes

`tinker-pdf-xml` refuses `<!DOCTYPE` before one byte after it is read, so this
census decides whether milestone 2 is a nicety or a blocker.

| Corpus | Documents | none | `<!DOCTYPE html>` | PUBLIC, single-quoted | PUBLIC, double-quoted | SYSTEM | internal subset |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Committed (9 books) | 43 | 13 | 25 | 0 | **5** | 0 | 0 |
| Fetched (20 books) | 241 | 200 | 11 | **30** | 0 | 0 | 0 |

One public identifier appears anywhere: `-//W3C//DTD XHTML 1.1//EN`, which is
**not** in EPUB 3.3 Appendix B's closed set — it was banned from EPUB 3
deliberately — so it is exactly the case milestone 2 names in a warning rather
than refusing or silently accepting.

Three things this table says that the plan did not:

- **The producers disagree about whether to write one at all.** pandoc writes a
  declaration on every content document it produces; calibre writes none, in
  either EPUB version. A reader on the parser as it stands reads every calibre
  book and refuses every pandoc one.
  *Amended by tier 4:* the third producer broke the tie two-to-one. KCC writes
  `<!DOCTYPE html>` on all seven of its documents, so **calibre is now the only
  producer here whose books a doctype-refusing parser could read**, and
  milestone 2 stops being one program's problem. Bare declarations went from 14
  of 29 committed documents to 25 of 43 on the strength of one book.
- **Both quote characters are real, and neither corpus shows both.** The plan
  measured the single-quoted form on Gutenberg's EPUB 2 books and milestone 2's
  exit criteria name it. pandoc writes the same identifier **double-quoted**,
  which 241 fetched content documents supply zero of.
- **No real document in 284 carries a `SYSTEM`-only declaration or an internal
  subset.** Milestone 2's fixtures for those two rows have to be written, not
  found — and the internal subset being absent from every real book is what
  makes refusing it by name cost nothing. Fourteen more real documents from a
  third producer did not change it.

### Named character references

**Zero**, across all 284 content documents of both corpora. The plan's working
assumption was a vendored table of the ~250 XHTML 1.0 names with a per-use
warning; the measurement refutes it.

And the corroboration is weaker than the plan expected, in the more useful
direction. Its sentence was *"producers overwhelmingly write `&#160;`"*. The
fetched corpus writes the numeric form **65 times** against **83 240 literal
non-ASCII characters**; the two committed producers write it **not once**, and
the em dash, the ellipsis, the non-breaking hyphen and the Japanese line in
`source/book.md` all reach the content documents as literal UTF-8. The escaping
habit the plan expected to find is a rounding error.

So the recommendation to milestone 2 is option 1 — refuse by name, per XML 1.0 —
with the brittleness now measured rather than feared: not one of 284 real
content documents from three independent producer families would be lost by it.
Both censuses count non-ASCII characters alongside the references, because
without that column a corpus of 83 240 non-ASCII characters and a corpus of none
report the same zero.

### CSS properties

| Corpus | Stylesheets | Distinct properties |
| --- | --- | --- |
| Committed | 12 | 44 |
| Fetched | 53 | 84 |

The plan's list was 41 names. The union across both corpora is considerably
larger, and the interesting part is not the count but which of this plan's
**non-goals** turn up in real books: `column-count`, `column-gap`,
`column-rule`, `column-fill` and their `-webkit-` and `-moz-` spellings —
multi-column, which the non-goals name as one of the two *"worth flagging rather
than filing under rare, because a book that uses either will lay out as a single
column and look entirely reasonable"*. Also present: `box-shadow`,
`text-shadow`, `border-radius`, `content`, `visibility`, `table-layout`,
`border-collapse`, `word-wrap`, `-epub-text-emphasis-style`, and Antenna House's
`-ah-margin-start` / `-ah-margin-end`.

Both producers write `page-break-before` and `page-break-after`, which is the
pair milestone 7's fragmentation criterion says appears in every measured book.

## What the real books showed that the plan did not predict

Recorded here as well as in the plan's Progress section, because this is the
file somebody reads when they open the directory.

1. **pandoc puts a seventh file in `META-INF`.** Every pandoc book carries
   `META-INF/com.apple.ibooks.display-options.xml`, which is not one of
   §4.2.6.3's six reserved names. **None of the twenty fetched books carries
   anything unreserved there at all.** A milestone 3 that refused an
   unrecognised `META-INF` entry would refuse every book pandoc writes and pass
   the entire downloaded corpus.
2. **calibre writes a `META-INF/` directory entry** — a name ending in `/`,
   zero bytes, **deflated**. pandoc writes none. A `META-INF` walk that treated
   every entry there as a file meets an empty one first.
3. **A content document is not named `.xhtml`.** calibre writes
   `index_split_000.html` and declares it `application/xhtml+xml` in the
   manifest. The extension is a claim; the manifest is the fact.
4. **The package document is not always under a directory.** calibre puts
   `content.opf` at the archive root; pandoc puts it under `EPUB/`. §4.2.5's
   resolution against the *referring document* is load-bearing from the first
   real file rather than from an exotic one.
   *Amended by milestone 3:* the direction was wrong, and the fetched corpus
   makes it four places rather than two — `EPUB/`, `OEBPS/`, `OPS/` and the
   archive root. §4.2.6.3.1 defines `full-path` as a path from the **container
   root**, so it is the one reference in this format whose base is *not* the
   document it is written in; resolving it against `META-INF/container.xml`
   yields `META-INF/EPUB/content.opf`, which no book here holds. The general
   §4.2.5 rule stands and is what milestone 4's manifest `href`s use.
5. **One producer's EPUB 3 has no NCX and its EPUB 2 has no navigation
   document; the other's EPUB 3 has both.** A reader that expects one of the two
   to be present sees a different book from each producer.
6. **Both ZIP methods appear inside one archive, from one producer, in one
   run.** pandoc stores two of `pandoc-plates.epub`'s three PNGs and deflates
   the third. Gap 30 recorded the same habit in Microsoft's two serialisers and
   read it as inconsistency; it is what a producer that measures does.
7. **The pictures are written in reverse of the order the book names them.**
   `file2.png`, `file1.png`, `file0.png`, by header offset. Physical order,
   directory order and spine order are three different orders, and this is the
   file that says so.
8. **A default invocation of a mainstream producer does not produce a clean
   book** — calibre's EPUB 3 cover output warns `NAV-011` under epubcheck 5.3.0.
9. **`tinker_pdf_zip::Entry` has no extra-field accessor**, so §4.3.2's *"no
   extra field"* clause cannot be checked through it. Measured by hand across
   all twenty-six books: zero everywhere. Milestone 3 needs either an accessor
   on `Entry` or a byte check in the facade.
10. **No real book distinguishes `header_offset == 0` from `index == 0`.** All
    twenty-six put `mimetype` first in both orders, so the wrong check passes
    every one of them. `tests/epub.rs` builds the container that does
    distinguish them, so the corpus-wide assertion is a measurement rather than
    a tautology.

## What the three tier-4 books showed, that the six could not

Eleven, and they divide cleanly. The first six are the fixed-layout book against
`epub_fixed_layout.rs`'s 615 lines of hand-built OCF fixtures — every one of
which was written from §8.2's own sentences, and four of which turn out to
describe something no producer writes. The next four are the two font books. The
last is not a finding about the format at all.

### The fixed-layout book against the fixtures

1. **A real producer declares `rendition:layout` with no `prefix` attribute at
   all, and this is the one that would have cost a reader everything.** Every
   fixture in `epub_fixed_layout.rs` writes
   `prefix="rendition: http://www.idpf.org/vocab/rendition/#"` on `<package>`,
   because that is what the specification's own example shows. EPUB 3.3 §5.4.3
   makes `rendition:` a **reserved** prefix that need not be declared, and KCC
   does not declare it. A build that required the declaration would have passed
   all thirteen fixed-layout tests and **reflowed every fixed-layout book in
   circulation** — a complete-looking book of the wrong shape, which is the
   failure this whole gap is organised around.
   `the_real_book_declares_the_layout_with_no_prefix_declaration` asserts both
   halves: the declaration really is absent, and the book is read as
   pre-paginated anyway.
2. **The layout is on the package and on no itemref, and what the itemrefs carry
   instead is a *different* rendition property in the same attribute.** Every
   fixture exercising §8.2.2's override puts `rendition:layout-pre-paginated` in
   `itemref/@properties`. The real book puts `page-spread-left` and
   `page-spread-right` there — six of them, alternating — and nothing about
   layout. So the first real book this meets supplies six tokens in that
   attribute that are not layouts, and a build that matched by prefix, or
   treated an unrecognised token there as a defect, meets all six at once.
   `rendition:spread` is written too, in the metadata, which this build has no
   notion of and correctly ignores.
3. **Four pictures became six pages.** KCC's double-page splitter cut the two
   landscape plates in half. The spine is not the picture count, the page count
   is not the input count, and a corpus built by assuming either would have
   missed it — this is the fixed-layout analogue of `tests/cbz/README.md`'s
   point about natural order, one format over.
4. **Six viewports, six different sizes, and not one is the size of the picture
   that produced it.** 117 × 177, 30 × 40, 29 × 36, 40 × 50, 39 × 45, 40 × 88 —
   KCC resized the cover from 120 × 180 to fit its device profile's aspect ratio
   and halved the two it split. `two_fixed_chapters_may_be_two_different_page_sizes`
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
   `FixedLayoutWithoutViewport` or `FixedLayoutContentClipped` was built to
   produce it, and no fixture can answer whether an *ordinary* book trips them
   anyway. A viewport grammar read a shade too strictly, or an initial
   containing block computed a fraction of a point small, would warn on every
   page of every real book and every fixture would still pass.

### The two font books

7. **calibre embeds only the faces the document reaches, and writes a rule per
   face with three descriptors.** `--embed-all-fonts` over a document with an
   `<h1>` and paragraphs embedded regular and bold and nothing else, and wrote
   `font-weight`, `font-style` **and** `font-stretch` on each rule. The other
   route, `--embed-font-family`, embeds all four faces of the family whether the
   book uses them or not — 863 KB against 424 KB — and writes only the
   descriptors that differ. Half the bytes and more descriptors, which is why the
   committed book is the first; `make-corpus.ps1` records the measurement so it
   need not be rediscovered.
8. **A real `src` list has more than one entry.**
   `pandoc-embedded-font.epub`'s rule is
   `local("Liberation Serif"), url(…) format("truetype")` — a name the reading
   system might have installed, then the file the book brought, which is what a
   rule out in the world looks like. `typeface::load_one`'s documentation says
   the list is walked to the end because §4.3 is a preference order, and until
   now every test of that was a fixture this repository wrote. The book reports
   one `LocalUnavailable` and **not** `NoUsableSource`, and the face is on the
   page: the entry defect says what failed, the absence of the rule-level defect
   says the rule succeeded. `FONTS.tsv`'s only non-zero `face-defects` cell is
   that one, and it is there on purpose.
9. **The two producers declare byte-identical files under two different media
   types.** calibre writes `application/vnd.ms-opentype`, which OCF 3.0 named and
   EPUB 3.3 dropped; pandoc writes `font/ttf`, which is RFC 8081's. A build that
   decided what a manifest item is from its declared media type would read one of
   these books and not the other; a build that decided from the extension would
   read both and be right by accident. That is milestone 1's `.html`
   content-document finding one item type further out, and it took a second
   producer to see it — which is the argument at the top of this file, paid off
   for the third time.
10. **pandoc does not rewrite the author's `url()`, and puts the stylesheet and
    the face in different directories.** `--epub-embed-font` writes the file to
    `EPUB/fonts/` and copies the `--css` sheet verbatim to
    `EPUB/styles/stylesheet1.css`, so a rule written as though the two sat
    together resolves to nothing. `source/embedded.css` says `../fonts/…` for
    that reason and records it. §4.2.5's "resolve against the referring
    document" is the referring *stylesheet* here, not the content document and
    not the package.

### And one that is not about the format

11. **A fixed-layout comic reaches this build as six correctly-sized,
    correctly-clipped, entirely blank pages.** The book paginates at the right
    count, at the six right sizes, with §8.1.2's clip on each — and **draws none
    of the six pictures**, because nothing in `src/epub/` turns an `<img>` into
    a box. No committed book draws an image on any page; the other eight are
    text, and text arrives, so the gap was invisible until a book arrived whose
    entire content is pictures. This is the corpus doing the job it exists for,
    and it is exactly gap 29's closing sentence coming true a format later: *"the
    first real archive this meets may find something, and nothing here would
    have."*

    It is recorded as an assertion rather than a note.
    `today_every_page_of_the_real_book_is_empty_inside_its_clip` pins every
    page's content stream to `q 0 0 W H re W n Q` and nothing between, and also
    checks the JPEGs really are in the container — so the emptiness is the
    painter's and not the archive's. **The milestone that paints a replaced
    element has to come here and delete that test**, which is the only kind of
    caveat that cannot rot.

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
directory outside the tree and copy in only what is new. That is how the three
tier-4 books arrived, and it is the only safe way while the corpus stays
non-reproducible.
