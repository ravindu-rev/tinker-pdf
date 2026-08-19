# The EPUB corpus (gap 31, milestone 1)

Six books, written by two real producers over text authored in this repository,
committed here; and twenty more that **cannot** be committed, fetched by
`fetch-corpus.sh` into a directory outside the tree. This file says which is
which, where each came from, what it demonstrates, and what licence it is under.

This milestone is scheduled **before any EPUB code exists**, and that is the
point of it. [Gap 29](../../../../docs/plans/gaps/29-cbz.md) closed having never
opened a `.cbz` a real archiver produced — three milestones owed it, and the
gap's closing statement had to record it as a limitation of the whole gap.
[Gap 30](../../../../docs/plans/gaps/30-xps.md) answered that structurally, by
obtaining eight genuine packages in milestone 1 *before a reader existed*, and
its Progress sections record seven things real files did that ECMA-388 did not
predict — two of which would have produced a reader that refuses every OpenXPS
file Windows writes. This is that device a third time.

## The licence gate, which is the reason there are two corpora

[Gap 31's plan](../../../../docs/plans/gaps/31-epub.md) measured this before a
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

49 597 bytes. Every `.epub` here was produced on **19 August 2026** on Windows
11, by `make-corpus.ps1` beside this file, from `source/book.md`,
`source/figures.md` and the four PNGs under `source/figures/`. The PNGs are
written byte by byte from the PNG specification by that script, so they are
ours without qualification.

| File | Bytes | Producer | EPUB | What it demonstrates | SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `pandoc-book-cover.epub` | 9 661 | pandoc 3.10.2 | 3.0 | A navigation document **and** an NCX in one EPUB 3 book, and one picture that is the cover | `c5491d64…3cd172bd` |
| `pandoc-book-nocover.epub` | 8 458 | pandoc 3.10.2 | 3.0 | The same book with **no image entry anywhere** — the `NoImages` half of the defect | `9a9bb6a2…d585ab12` |
| `pandoc-book-epub2.epub` | 9 548 | pandoc 3.10.2 | 2.0 | OPF 2.0, which this plan reads as a compatibility surface, and the **double-quoted** XHTML 1.1 doctype | `644cd5a8…939e88e9` |
| `pandoc-plates.epub` | 8 329 | pandoc 3.10.2 | 3.0 | Three pictures of three different sizes, two **stored** and one deflated, written in **reverse** of the order the book names them | `34eaa4ff…3ece9d17` |
| `calibre-book-cover.epub` | 7 358 | calibre 9.13.0 | 3.0 | A second producer: content documents named `.html`, the package document at the archive root, a `META-INF/` **directory entry**, and no doctype anywhere | `d5785321…ce81347d` |
| `calibre-book-nocover.epub` | 6 243 | calibre 9.13.0 | 2.0 | The second producer with no image entry, and an NCX with no navigation document | `d9830ec0…cecff4a17` |

The full hashes are in `make-corpus.ps1`'s own output and are reproducible only
in the sense a hash of a *committed* file always is: **the corpus is not
regenerable byte for byte.** Both producers mint a fresh UUID for the package
document's `dc:identifier` on every run and calibre stamps a `dcterms:modified`
timestamp, so a second run is a different file. That is gap 30's situation
exactly, and `.gitattributes` gains `*.epub binary` for gap 30's reason: a
normalised line ending inside a **stored** entry would break its CRC-32, and
there would be no way back.

### Licences of the producers, and of what they put in

pandoc is GPL-2.0-or-later and calibre is GPL-3.0-only. **Neither licence
touches the output**: a converter's copyright does not reach the document it
converts, which is the same reading `fuzz/README.md` applies to `opj_compress`
and gap 30 applied to WPF. Nothing of either program is vendored, linked or
redistributed, and neither is a dependency of anything in the workspace —
`make-corpus.ps1` is how these six files were obtained, not something CI runs.

Unlike gap 30's corpus, **no font is involved**. That corpus had to justify
Cascadia Mono; this one embeds no face at all, so there is nothing here anybody
has to licence.

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

Five of six are clean. The sixth is **calibre's own EPUB 3 cover output**:
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

`INVENTORY.tsv` names all **72** entries of the six books, with the media type
the package document declares, the ZIP compression method, the local header
offset and both sizes. It is written by `inventory.ps1` through .NET's own
central-directory walk, and `tests/epub.rs`'s `inventory_matches_the_books`
recomputes name, method, header offset and both sizes through
**`tinker-pdf-zip`** on every `cargo test`. So the inventory cannot drift from
the books, and two independent ZIP readers have to agree about all seventy-two
rows.

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

## The censuses

### Doctypes

`tinker-pdf-xml` refuses `<!DOCTYPE` before one byte after it is read, so this
census decides whether milestone 2 is a nicety or a blocker.

| Corpus | Documents | none | `<!DOCTYPE html>` | PUBLIC, single-quoted | PUBLIC, double-quoted | SYSTEM | internal subset |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Committed (6 books) | 29 | 10 | 14 | 0 | **5** | 0 | 0 |
| Fetched (20 books) | 241 | 200 | 11 | **30** | 0 | 0 | 0 |

One public identifier appears anywhere: `-//W3C//DTD XHTML 1.1//EN`, which is
**not** in EPUB 3.3 Appendix B's closed set — it was banned from EPUB 3
deliberately — so it is exactly the case milestone 2 names in a warning rather
than refusing or silently accepting.

Three things this table says that the plan did not:

- **The two producers disagree about whether to write one at all.** pandoc
  writes a declaration on every content document it produces; calibre writes
  none, in either EPUB version. A reader on the parser as it stands reads every
  calibre book and refuses every pandoc one.
- **Both quote characters are real, and neither corpus shows both.** The plan
  measured the single-quoted form on Gutenberg's EPUB 2 books and milestone 2's
  exit criteria name it. pandoc writes the same identifier **double-quoted**,
  which 241 fetched content documents supply zero of.
- **No real document in 270 carries a `SYSTEM`-only declaration or an internal
  subset.** Milestone 2's fixtures for those two rows have to be written, not
  found — and the internal subset being absent from every real book is what
  makes refusing it by name cost nothing.

### Named character references

**Zero**, across all 270 content documents of both corpora. The plan's working
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
with the brittleness now measured rather than feared: not one of 270 real
content documents from two independent producer families would be lost by it.
Both censuses count non-ASCII characters alongside the references, because
without that column a corpus of 83 240 non-ASCII characters and a corpus of none
report the same zero.

### CSS properties

| Corpus | Stylesheets | Distinct properties |
| --- | --- | --- |
| Committed | 8 | 42 |
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

## Regenerating

```
pwsh -NoProfile -File crates/tinker-pdf/tests/epub/make-corpus.ps1 \
    -Pandoc <path to pandoc> -EbookConvert <path to ebook-convert>
pwsh -NoProfile -File crates/tinker-pdf/tests/epub/inventory.ps1
sh crates/tinker-pdf/tests/epub/fetch-corpus.sh
```

The first is not byte-reproducible and the committed files are the record. The
second must be re-run whenever the first is, or `inventory_matches_the_books`
goes red — which is the point of it.
