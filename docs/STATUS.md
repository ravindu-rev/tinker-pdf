# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**826 tests**, `cargo fmt --check` and `clippy -D warnings` clean,
`wasm32-unknown-unknown` builds, the crate graph is enforced, and the fuzz
targets and language bindings type-check — on every commit.

> **This file was wrong for a long time.** A 47-agent audit against every plan
> file in August 2026 found ~35 of ~211 milestones genuinely complete, ~123
> partial and ~53 missing, and found this file claiming several of the missing
> ones as done. What follows is written to be checkable rather than
> encouraging. Where something is partial, the gap is named.

## What the audit changed

Fixed since, each with tests that would have caught it:

| Was | Now |
| --- | --- |
| **Four of eleven fuzz targets never compiled.** Wrong arity since written; `fuzz/` sits outside the workspace so nothing checked it | Fixed, and `cargo check` on the fuzz crate is a CI job. The bindings, excluded for the same reason, are too |
| **`authenticate` failed on any shared document.** `Arc::get_mut` returns None once a `Page` exists, so "look at a page, then supply the password" returned `NotEncrypted` for an encrypted file | Interior mutability; `authenticate(&self)`, as the plan always specified |
| **`/Rotate` was never applied**, though the canvas *was* sized for it — rotated pages drew upright and clipped | `page_view_transform`, with the crop-box origin as well |
| **`J j M d` were discarded**, so every stroke was solid, butt-capped, miter-joined, while the rasterizer's tested implementation sat unreachable | Wired through the graphics state |
| **Pattern fills painted black.** Every gradient from a design tool became an opaque rectangle | Shading patterns paint; tiling patterns are reported, not blacked out |
| **`/Function` arrays truncated to the first element**; **Separation tint transforms were the identity**; **`/Lab` was aliased to RGB** | All three read properly; Lab converts through XYZ |
| **`/SMask` was never read** — every soft-masked image painted an opaque rectangle. `/Decode` likewise. Stencils were hardcoded black | All three honoured |
| **Text render modes**: mode 1 filled instead of stroking, modes 4–7 never clipped | Fill, stroke and clip decided independently; clip accumulates to `ET` |
| **Inline images were skipped entirely** | Decoded, with Table 93's abbreviated keys |
| **~2330 lines of `src/semantics/` never compiled** — no `mod` declaration anywhere. Anyone grepping concluded phase 04 was done | Deleted. `link.rs` was the only part describing something the live tree lacked, so links are reimplemented and tested |
| **No deflate encoder existed**, so `compress` had no reader and every written byte was uncompressed | Implemented; round-tripped against our own inflate |
| **Object streams produced files that opened with zero pages** — the source trailer's stale `/Size` overwrote the correct one | Fixed. This had been true since the feature landed |
| **Rewriting an authenticated encrypted document emitted `/Encrypt` over plaintext** | `/Encrypt` dropped on rewrite |
| **"Full rewrite" had no garbage collection** | Opt-in mark-and-sweep |
| **Redaction read `Tm` as translation only**, so scaled text cut the wrong glyphs — the worst failure mode for a redaction, because it looks like it worked | The whole matrix is read; rotated runs are refused rather than cut wrongly |
| **`/FontFile` (a Type 1 program) went to the sfnt and CFF parsers**, which declined it correctly, so embedded Type 1 fonts drew nothing and said only that some font was unreadable | Type 1 is read: eexec, charstrings, `seac`, flex |
| **`xtask` was a stub that exited 2**, so nothing checked the crate graph — a design rule the compiler cannot enforce | `cargo xtask dag`, in CI, with the one undeclared edge written down and justified |
| **Type 3 glyphs never drew.** A Type 3 glyph is a content stream, not an outline, and there was no path for one — no warning either, because nothing was missing | The interpreter recurses into the procedure, with the `/FontMatrix` inside the placing transform and the graphics state restored after |
| **An EOL aborted a whole CCITT image.** T.4 separates rows with one and `/K > 0` streams always carry them | EOL and EOFB recognised; a damaged row repeats the row above rather than ending the page |
| **`Document::open` collapsed every failure into `NotAPdf`**, so "needs a password" and "not a PDF" were the same answer | `readable()` reports `PasswordRequired`; `OpenError::Empty` separates zero bytes from bad bytes |
| **`TextPage` had no warnings**, so "no text" and "text this build could not decode" were indistinguishable | `TextPage::warnings`, deduplicated |
| **`deny.toml` could not express the hand-rolled rule** — it lists licences, which say nothing about what a crate *does* | The crates that would violate it are denied by name |
| **Redaction ignored `cm`**, so content under a transform was measured in the wrong space | The matrix is composed, saved and restored with the pen |

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + **deflate** + JPEG + CCITT | Own inflate **and deflate**, LZW, ASCIIHex/85, RunLength, predictors; baseline JPEG; CCITT G3/G4 |
| [03 Encryption](plans/03-encryption.md) | reading, R6 exercised | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; owner vs user distinguished; `/P` read through its reserved bits |
| [04 Document semantics](plans/04-document-semantics.md) | most of it | Metadata, page tree with inheritance, geometry, outlines, name/number trees, destination enum, page labels, actions, **links** |
| [05 Fonts](plans/05-fonts.md) | TrueType + CFF + **Type 1** + host seam | Encodings, CMaps, standard-14 metrics; TrueType `glyf`, CFF Type 2 **and Type 1** outlines; `FontProvider` for faces a document does not embed |
| [06 Content & text](plans/06-content-and-text.md) | substantially | Tokenizer, text state machine, `Device` seam, text device with quads and search, **inline images**, **all stroke parameters** |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | broad, see gaps | Colour spaces incl. **Lab, Separation, DeviceN**; all four function types **and function arrays**; clipping incl. **text clip modes**; images with **`/SMask` and `/Decode`**; axial and radial shadings; **shading patterns**; **`/Rotate` and `/CropBox`**; alpha; outward pixel rounding; page-area ceiling |
| [09 Writing](plans/09-writing.md) | rewrite + incremental + object streams | Full rewrite with **optional GC**, incremental update with byte-identical prefix, classic xref, **working object streams**, **compression** |
| [10 Editing](plans/10-editing.md) | editor, pages, annotations, redaction | Copy-on-write editor; delete/move/rotate pages; annotations with synthesized appearances; redaction that removes content |
| [11 Forms](plans/11-forms.md) | read, fill, appearance regeneration | AcroForm field tree, fill text/choice/checkbox/radio, appearances rebuilt, reset |
| [12 Creation](plans/12-creation.md) | pages, text, images | `DocumentBuilder` |
| [13 Bindings](plans/13-bindings.md) | all three build, now checked | C ABI, Python, JS/wasm, .NET |
| [14 Testing](plans/14-testing-and-corpora.md) | tools real, fuzzing written | `tpdf`, `pdfcmp`, `oracle-diff`; 11 fuzz targets; a hostile-input sweep on stable |

## Not built

| Gap | Consequence | Where |
| --- | --- | --- |
| **No corpus has been run** | Eight real files have been through `tpdf`. The pinned public corpora never have. **Still the largest gap between "tests pass" and "handles what exists".** | [14](plans/14-testing-and-corpora.md) |
| **Fuzzers compile but have never been executed** | Needs nightly and `cargo-fuzz`. The stable sweep covers the same entry points far more shallowly. | [14](plans/14-testing-and-corpora.md) |
| **Font embedding and subsetting in the builder** | Built documents can use the standard 14 but cannot embed a font. Reading is complete: TrueType, CFF, **Type 1** and **Type 3** all draw. | [12](plans/12-creation.md) |
| **Redaction: form XObjects, images** | The text *and* transformation matrices are handled. Form XObjects are not recursed into and images are not scrubbed, so a redaction over content inside a form XObject still does nothing. | [10](plans/10-editing.md) |
| **Editing: merge, split, insert, flatten** | Only delete, move and rotate exist. | [10](plans/10-editing.md) |
| **Encrypt-on-save, linearization** | `WriteOptions::encryption` still has no reader; set it and you get a plaintext file with no error. Rewriting now drops `/Encrypt` rather than lying about it. | [09](plans/09-writing.md) |
| **CCITT `/EndOfLine`, `/EndOfBlock` parameters** | The codes are now recognised wherever they appear, but the two parameters are not consulted, `/K > 0` is not true T.4 mixed mode, and the output is one byte per pixel rather than packed 1-bpp. | [02](plans/02-filters.md) |
| **Progressive JPEG** | Refused, not decoded — and it appeared in 4 of the first 8 real files. | [02](plans/02-filters.md) |
| **JBIG2, JPX; mesh shadings; tiling patterns** | Reported with a warning rather than half-decoded. | [02](plans/02-filters.md), [08](plans/08-rendering-device.md) |
| **Transparency groups, soft-mask groups, blend modes** | Constant alpha works; `/SMask` on *images* works; group transparency does not. | [08](plans/08-rendering-device.md) |
| **Annotation `/BBox` clipping** | An appearance stream larger than its box is not clipped to it. | [08](plans/08-rendering-device.md) |
| **Determinism** | The DAG check and the hand-rolled allowlist now exist and run in CI. What does not: a determinism job, and platform `libm` is still called on paths that reach pixels (`powf`, `sin`, `cos`, `atan2`, `ln`), so bit-identical cross-target output is not achievable today whatever a job would measure. | [00](plans/00-architecture.md), [99](plans/99-consistency.md) |
| **The font seam is not in the bindings** | `FontProvider` is Rust-only, so no binding can draw text for a non-embedding document. | [13](plans/13-bindings.md) |
| **Binding packaging** | Nothing published; no wheel or per-RID CI. | [13](plans/13-bindings.md) |
| **Forms: comb fields, calculations** | A comb field's fixed cells are not laid out; `/AA` scripts are not run (the open JavaScript question). | [11](plans/11-forms.md) |
| **Tinker integration** | Tinker still runs on MuPDF and does not depend on this engine at all. | [15](plans/15-tinker-integration.md) |

## Where Tinker integration stands

Tinker's engine layer is eleven functions wide — open, page count, metadata,
permissions, geometry, encryption info, render, text, search, outline — and
every one is covered, several better than MuPDF covered them: real permission
flags, owner-versus-user, a pinned A4-at-150-dpi size, destinations as an enum,
`Send + Sync` reads.

Two things still block the swap, and neither is a missing feature.

**The corpus has never been run.** Swapping a battle-tested renderer for one
validated on four self-authored fixtures and eight files from one laptop would
be a bad trade. `tpdf check` over the pinned corpora is the cheapest remaining
work with the highest information return.

**One decision is the owner's.** MuPDF also opens EPUB, XPS and CBZ, which is
what `Doc::Other` exists for. This engine reads PDF and will not read those.
Dropping them, shelling out, or keeping MuPDF for those formats alone is a
product decision.

Beyond those, Tinker will need to hand the engine a `FontProvider`, or rendered
pages lose the text of every document that does not embed its fonts.

## The honest summary

The engine reads PDFs properly and now renders them substantially correctly:
the silent-wrongness class of defect — rotation, patterns, soft masks, tint
transforms, stroke parameters, text render modes — has been found and fixed,
each with a test that would have caught it. It writes valid files, compresses
them, and no longer claims encryption it did not apply.

What it still has not done is meet a real corpus. Everything above is true of
the inputs it has been shown, and almost all of those were written by this
repository. That remains the next thing worth doing, ahead of any new feature.
