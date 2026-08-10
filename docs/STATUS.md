# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**733 tests**, `cargo fmt --check` and `clippy -D warnings` clean, and
`wasm32-unknown-unknown` builds — on every commit.

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + JPEG + CCITT | Own inflate/deflate, LZW, ASCIIHex/85, RunLength, predictors; **baseline JPEG** with subsampling, restarts, YCbCr/YCCK and Adobe's inverted CMYK; **CCITT G3 and G4** |
| [03 Encryption](plans/03-encryption.md) | complete for reading | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; **owner vs user distinguished**; **`/P` read correctly through its reserved bits** |
| [04 Document semantics](plans/04-document-semantics.md) | complete | Metadata, page tree with inheritance, geometry, outlines, name/number trees, **destination enum**, page labels, actions |
| [05 Fonts](plans/05-fonts.md) | both waves + host seam | Encodings, CMaps, standard-14 metrics, TrueType tables; TrueType `glyf` and CFF Type 2 outlines; **`FontProvider` for faces a document does not embed** |
| [06 Content & text](plans/06-content-and-text.md) | complete | Tokenizer, full text state machine, `Device` seam, text device with quads and search |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | substantively complete | Colour operators and spaces, all four function types, clipping, images, axial and radial shadings, ExtGState alpha, glyphs, outward pixel rounding, **annotation appearances drawn**, **page-area ceiling** |
| [09 Writing](plans/09-writing.md) | rewrite + incremental + object streams | Full rewrite, **incremental update with byte-identical prefix**, classic xref, **object streams with cross-reference streams**, signature placeholder record |
| [10 Editing](plans/10-editing.md) | editor, pages, annotations, redaction | Copy-on-write `DocumentEditor`; delete/move/rotate pages; annotations **with synthesized appearance streams**; **redaction that removes content rather than covering it** |
| [11 Forms](plans/11-forms.md) | read, fill, appearance regeneration | AcroForm field tree with inheritance and qualified names; fill text, choice, checkbox and radio; **appearances rebuilt on every fill**; reset |
| [12 Creation](plans/12-creation.md) | pages, text, images | `DocumentBuilder`: pages, base-14 text, rectangles, colour, JPEG/RGB/grey images, metadata, outlines — output reopens at ladder level Trust |
| [13 Bindings](plans/13-bindings.md) | all three compile | C ABI; **Python** (PyO3, releases the GIL), **JavaScript/wasm** (wasm-bindgen), **.NET** (C# over the C ABI, SafeHandle lifetimes) |
| [14 Testing](plans/14-testing-and-corpora.md) | tools real, fuzzing written | **`tpdf`**, **`pdfcmp`** and **`oracle-diff`** are working programs; **11 fuzz targets**; **a hostile-input sweep that runs on stable, every commit** |

**Checkpoint A is reached and exceeded.** All 22 ported assertions from
Tinker's own suite pass — `open_documents`, `text_and_search`, `outline` and
`render_pages` — including the two that were MuPDF defects and the A4-at-150-dpi
rounding its engine left as folklore.

## Not built

Stated plainly, because a plan that reads as if everything is done is worse
than no plan.

| Gap | Consequence | Where it belongs |
| --- | --- | --- |
| **No corpus of any size has been run** | Eight real-world files have now been through `tpdf` (see below) — enough to be encouraging and far too few to be evidence. The pinned public corpora have still never been fetched. **This remains the largest gap between "tests pass" and "handles what exists".** | [14](plans/14-testing-and-corpora.md) |
| **Fuzzing written but never run** | Eleven targets exist and compile against the real APIs; none has been executed, because that needs a nightly toolchain and `cargo-fuzz`. The stable sweep covers the same entry points far more shallowly. | [14](plans/14-testing-and-corpora.md) |
| **`oracle-diff` never met an oracle** | None of mutool, poppler or pdfium is installed on the machine it was written on. Its similarity metric is unit-tested; its subprocess plumbing is not. | [14](plans/14-testing-and-corpora.md) |
| **No bundled substitute fonts** | A host can now supply faces through `FontProvider`, so this is no longer a blocker — but a caller that supplies nothing still gets no text for documents that embed no fonts. Bundling Liberation would fix it out of the box, and needs a licensing answer for Symbol and ZapfDingbats. | [05](plans/05-fonts.md) |
| **The font seam is not in the bindings** | `FontProvider` is reachable from Rust only. The C ABI, Python, JS and .NET cannot supply one, so they cannot draw text for non-embedding documents at all. | [13](plans/13-bindings.md) |
| **Editing: no merge, split, or flatten** | Cross-document merge with resource grafting, page insertion, and annotation flattening are unbuilt. Page delete, move and rotate work. | [10](plans/10-editing.md) |
| **Forms: no comb fields, no calculations** | A comb field's fixed cells are not laid out, and `/AA` calculation scripts are not run — the open JavaScript question in the plan is still open. | [11](plans/11-forms.md) |
| **Progressive JPEG, JBIG2, JPX; mesh shadings** | Reported with a warning rather than half-decoded. | [02](plans/02-filters.md), [08](plans/08-rendering-device.md) |
| **Transparency groups, soft masks, tiling patterns** | Skipped; constant alpha works. | [08](plans/08-rendering-device.md) |
| **Font embedding and subsetting in the builder** | Built documents can use the standard 14 but cannot embed a font. | [12](plans/12-creation.md) |
| **Encrypt-on-save, linearization** | The options exist and reading handles both; writing produces neither. Encryption also needs the host entropy source wired. | [09](plans/09-writing.md) |
| **Binding packaging** | All three compile; none is published, and there is no CI job building wheels or per-RID natives. | [13](plans/13-bindings.md) |
| **Tinker integration** | Tinker still runs on MuPDF. Nothing has been swapped. See below. | [15](plans/15-tinker-integration.md) |

## The first real files

Eight documents from outside this repository — produced by Microsoft Print to
PDF and by web-to-PDF tooling, none of them written by the engine — were run
through `tpdf check`, `tpdf render` and `tpdf text`. Too small to be a corpus,
large enough to be the first evidence that was not self-generated.

| | Result |
| --- | --- |
| Opened | 8 of 8, all at ladder level **Trust**, zero warnings |
| Rendered | 20 of 20 pages, no failures, every page with real ink except one |
| Progressive JPEG | **4 of 8 files** — reported and degraded, not decoded |
| No embedded fonts | 1 of 8 — rendered its rules and none of its text |
| No text at all | 1 of 8, a scan with zero font resources; extracting nothing is correct |

The finding worth acting on is the first one. **Progressive JPEG turned up in
half of them.** Plan [02](plans/02-filters.md) defers it behind a capability
flag pending corpus hit-rates, under ruling 3 — schedule capabilities on
evidence rather than on guesswork. This is that evidence arriving earlier and
louder than expected, on a sample where JBIG2 and JPX did not appear at all.
It should be reweighted against them before any further filter work.

Nothing here says the engine is correct on real files: no output was compared
against another renderer, only checked for not failing and not being blank.
It says the engine does not fall over on them, which is a different and much
weaker claim.

## Where Tinker integration actually stands

Tinker's engine layer is narrow — `open`, `open_bytes`, `page_count`,
`metadata`, `permissions`, `page_geometry`, `encryption_info`, `render`,
`page_text`, `search_page`, `outline` — and **every one of those is now
covered**, several better than MuPDF covered them:

- `permissions` returns real flags rather than reporting every document
  unrestricted, and `auth_level` distinguishes owner from user.
- `render` pins A4 at 150 dpi to 1240×1755 as an API guarantee.
- Destinations are an enum, so a named destination is never mistaken for an
  explicit one.
- Reads are `Send + Sync`, which dissolves the constraint behind the
  render-clone-pool plan.

Two things still block the swap, and neither is a missing feature.

**The corpus has never been run.** Swapping engines on the strength of four
fixtures and a mutation sweep would be trading a battle-tested renderer for an
untested one. Running `tpdf check` over a real corpus is the cheapest
remaining work with the highest information return, and it should happen
before any Tinker file changes.

**One decision is the owner's.** MuPDF also opens EPUB, XPS and CBZ, which is
what `Doc::Other` exists for. This engine reads PDF and will not read those.
Dropping them, shelling out to another tool, or keeping MuPDF alongside for
those formats alone is a product decision, not a technical one, and the plan
has always recorded it as such.

Beyond those: Tinker will need to hand the engine a `FontProvider` — a system
face on desktop — or its rendered pages will lose the text of any document
that does not embed fonts.

## The honest summary

The engine reads PDFs properly: it opens damaged ones, decrypts them with
correct security semantics, extracts text with geometry, and answers every
structural question Tinker asks. It renders vector graphics, images, gradients
and annotations in colour, and writes valid files including object streams and
signature-safe incremental updates. It edits: page operations, annotations
with real appearance streams, form filling that regenerates appearances, and
redaction that actually removes the bytes. Three language bindings compile
against it, and three working tools drive it from a command line.

What it has never done is meet a real corpus. Everything above is true of the
inputs it has been shown, and those inputs were mostly written by this
repository. That is the next thing worth doing, ahead of any new feature.
