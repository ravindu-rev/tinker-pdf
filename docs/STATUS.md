# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**971 tests**, `cargo fmt --check` and `clippy -D warnings` clean,
`wasm32-unknown-unknown` builds, the crate graph is enforced, and the fuzz
targets and language bindings type-check — on every commit.

> **It was wrong again.** A second audit in August 2026 checked this file
> against the code in *both* directions and found the gap table accurate but
> incomplete: **22 things listed as built are absent or materially thinner
> than claimed**, seven of them producing silently wrong output or unopenable
> files. They are written down in [audit-2026-08.md](audit-2026-08.md), with
> the five fixed so far struck through there. A known gap is manageable; a
> false claim is not, because nobody goes looking.

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
| **An embedded font carried its whole face** — tens of megabytes to set a line of Latin text with a CJK font, which is the reason nobody embeds | Subset to the glyphs the document draws, with composite components followed and the 9.6.4 name tag written |
| **Nothing could write a linearized file**, so a viewer could not show page one before the last byte arrived — the capability MuPDF 1.26 removed, and the reason Tinker's plans had to shell out to qpdf | Annex F layout: parameter dictionary first, a cross-reference table for page one ahead of it, page one's objects before everything else, shared objects last, and the final `startxref` pointing back to the front |
| **Form XObjects were not clipped to their `/BBox`** (8.10.2), and forms are how most producers place repeated content — so a form drawing outside its box painted over the rest of the page, on every page it appeared | Clipped, with the corners transformed into device space; the annotation case, which is the same defect one layer up, is fixed too |
| **`pdfcmp` gated on the mean channel difference** while promising its budgets transfer from Tinker's, which counts changed pixels — a glyph moving one pixel barely moves the mean, so the tool would report "within budget" for exactly the regression it exists to catch | Gates on the fraction of pixels that moved by more than a threshold, with Tinker's own constants as the defaults |
| **`cargo xtask dag` never checked xtask's own dependencies** — it looked for `tools/xtask/Cargo.toml`, which does not exist, and a failed read was a silent `continue` | Paths rather than names, and an unreadable manifest is a reported problem |
| **Progressive JPEG was refused, not decoded** — in four of the first eight real files, so their photographs rendered as grey placeholders | Spectral selection, successive approximation for DC and AC, and end-of-band runs. Baseline moved onto the same coefficient buffer rather than keeping a second path |
| **`extend` shifted by an unclamped magnitude category**, so any JPEG with a corrupt Huffman table panicked — reachable from baseline since the decoder was written (ruling 1) | Clamped to the largest category T.81 defines |
| **A single-component JPEG scan was decoded over the MCU-padded block grid** instead of the component's own, desynchronising every later block where the sampling factors are not 1×1 | Non-interleaved scans iterate their own dimensions (T.81 A.2.2) |
| **Platform `libm` was called on three pixel paths** — round joins, the PostScript calculator, the sRGB transfer function — so bit-identical cross-target output was impossible whatever CI measured (ruling 4) | `tinker-pdf-math`, built only from operations IEEE 754 pins exactly, plus `cargo xtask libm` to keep it that way |
| **`Document::cos` returned a type the facade did not export**, and `DocumentBuilder` was not exported at all, so a caller depending on the facade alone could neither use the escape hatch nor write a file | Both exported |
| **Redaction stopped at the page stream.** Forms are how most producers place repeated content, so a redaction could be driven straight through one; images under a rectangle were covered, not removed | Forms are rewritten recursively; images are scrubbed to a blank sample |
| **Page operations were silently dropped by a rewrite** — the reordered `/Kids` was written into the incremental set only | Applied to whichever set the mode builds. Every existing page-operation test saved incrementally, which is why nothing caught it |
| **`WriteOptions::encryption` had no reader**, so asking for encryption produced a plaintext file with no error | R6 encrypt-on-save, strings and streams, per-object IVs |
| **`/EncryptMetadata` and `/Crypt /Identity` were ignored**, so exempt streams were decrypted into noise | Both honoured |
| **Comb fields laid out as ordinary text**, drifting out of their printed cells | Laid out in cells |
| **Attachments, XMP and `/Limits` descent were unreachable** | All three implemented |
| **The font seam was Rust-only**, so no binding could draw text for a document embedding no fonts | `set_fonts` across the C ABI, Python, JS and .NET |

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + deflate + JPEG + CCITT | Own inflate and deflate, LZW, ASCIIHex/85, RunLength, predictors; JPEG **baseline, extended sequential and progressive**; CCITT G3/G4 |
| [03 Encryption](plans/03-encryption.md) | reading and **writing** | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; owner vs user distinguished; `/P` read through its reserved bits; `/EncryptMetadata` and `/Crypt /Identity` honoured |
| [04 Document semantics](plans/04-document-semantics.md) | complete | Metadata, page tree with inheritance, geometry, outlines, name/number trees with **`/Limits` descent**, destination enum, page labels, actions, links, **attachments**, **XMP** |
| [05 Fonts](plans/05-fonts.md) | TrueType + CFF + **Type 1** + host seam | Encodings, CMaps, standard-14 metrics; TrueType `glyf`, CFF Type 2 **and Type 1** outlines; `FontProvider` for faces a document does not embed |
| [06 Content & text](plans/06-content-and-text.md) | substantially | Tokenizer, text state machine, `Device` seam, text device with quads and search, **inline images**, **all stroke parameters** |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | broad, see gaps | Colour spaces incl. **Lab, Separation, DeviceN**; all four function types **and function arrays**; clipping incl. **text clip modes**; images with **`/SMask` and `/Decode`**; axial and radial shadings; **shading patterns**; **`/Rotate` and `/CropBox`**; alpha; outward pixel rounding; page-area ceiling |
| [09 Writing](plans/09-writing.md) | rewrite + incremental + object streams + encryption + **linearization** | Full rewrite with optional GC, incremental update with byte-identical prefix, classic xref, working object streams, compression, R6 encrypt-on-save, **Annex F layout** |
| [10 Editing](plans/10-editing.md) | substantially complete | Copy-on-write editor; delete/move/rotate/**insert/import/keep**; annotations with synthesized appearances **and flattening**; redaction through **forms and images** |
| [11 Forms](plans/11-forms.md) | read, fill, appearance regeneration | AcroForm field tree, fill text/choice/checkbox/radio, **comb fields**, appearances rebuilt, reset |
| [12 Creation](plans/12-creation.md) | pages, text, images, embedded fonts **with subsetting** | `DocumentBuilder` |
| [13 Bindings](plans/13-bindings.md) | all three build and are checked | C ABI, Python, JS/wasm, .NET, each able to **supply fonts** |
| [14 Testing](plans/14-testing-and-corpora.md) | tools real, fuzzing written | `tpdf`, `pdfcmp`, `oracle-diff`; 11 fuzz targets; a hostile-input sweep on stable |

## Not built

Every row below has a plan of its own in [plans/gaps/](plans/gaps/README.md),
linked in the last column — what to build, what to reuse, and how you know
when it is done. [16 Build sequence](plans/16-build-sequence.md) orders them
by value over risk.

| Gap | Consequence | Where |
| --- | --- | --- |
| **No corpus has been run** | Eight real files have been through `tpdf`. The pinned public corpora never have. **Still the largest gap between "tests pass" and "handles what exists".** | [23](plans/gaps/23-corpus-runner.md) |
| **Fuzzers compile but have never been executed** | Needs nightly and `cargo-fuzz`. The stable sweep covers the same entry points far more shallowly. | [24](plans/gaps/24-fuzz-execution.md) |
| **Linearization: external validation, and encrypt+linearize** | The layout is written and every offset it declares is checked against the bytes — but `qpdf --check` and `--show-linearization` are the arbiters the plan names and neither has been run, so the hint *tables* are unproven. `linearize` is also silently dropped when encryption is on, rather than combining with it. An *incremental* update still cannot encrypt, since it would need the original file's key. | [19](plans/gaps/19-encrypt-and-linearize.md), [20](plans/gaps/20-linearization-validation.md) |
| **CCITT `/EndOfLine`, `/EndOfBlock` parameters** | The codes are now recognised wherever they appear, but the two parameters are not consulted, `/K > 0` is not true T.4 mixed mode, and the output is one byte per pixel rather than packed 1-bpp. | [16](plans/gaps/16-ccitt-completion.md) |
| **JBIG2, JPX; mesh shadings; tiling patterns** | Reported with a warning rather than half-decoded. | [17](plans/gaps/17-jbig2-generic-region.md), [18](plans/gaps/18-jpx-decision.md), [10](plans/gaps/10-mesh-shadings.md), [09](plans/gaps/09-tiling-patterns.md) |
| **Transparency groups, soft-mask groups, blend modes** | Constant alpha works; `/SMask` on *images* works; group transparency does not. | [11](plans/gaps/11-transparency-groups.md) |
| **Determinism: the wasm leg** | `tests/determinism.rs` hashes rendered pages against committed fingerprints and runs on linux, windows and macos in the existing matrix — so ruling 4 is demonstrated on three of the four targets, not merely achievable. The fourth runs under wasmtime on `wasm32-wasip1` in a job that has never executed here, because neither the target nor wasmtime is installed locally. Written, unverified. | [25](plans/gaps/25-wasm-determinism-leg.md) |
| **Binding packaging** | Nothing published; no wheel or per-RID CI. | [26](plans/gaps/26-binding-packaging.md) |
| **Forms: calculations** | `/AA` scripts are not run — the open JavaScript question, which is a decision before it is code. Comb fields now lay out in their cells. | [27](plans/gaps/27-form-calculations-decision.md) |
| **Tinker integration** | Tinker still runs on MuPDF and does not depend on this engine at all. | [28](plans/gaps/28-tinker-integration-decisions.md) |

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
