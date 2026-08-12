# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**1060 tests**, `cargo fmt --check` and `clippy -D warnings` clean,
`wasm32-unknown-unknown` builds, the crate graph is enforced, and the fuzz
targets and language bindings type-check — on every commit. The fuzz targets
also *run* on every commit now, briefly, over committed seed corpora. The four
determinism fingerprints reproduce byte-for-byte on `wasm32-wasip1` under
wasmtime — run locally against native Windows, not yet observed in CI.

> **It was wrong again.** A second audit in August 2026 checked this file
> against the code in *both* directions and found the gap table accurate but
> incomplete: **22 things listed as built are absent or materially thinner
> than claimed**, seven of them producing silently wrong output or unopenable
> files. They are written down in [audit-2026-08.md](audit-2026-08.md), with
> the thirteen fixed so far struck through there. A known gap is manageable; a
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
| **A `cmap` format 12 group could overflow a glyph id into a panic.** `startGlyphID + (code - startCharCode)` in `u32`, all three from the file — the first thing the fuzzers found, by two targets within thirty seconds of each other. Format 12 is the subtable used past the BMP, so any CJK or emoji face reaches it (ruling 1) | `checked_add` before the `u16::try_from` that was already there and could never run. The 68-byte reproducer is a test in `sfnt.rs` and a seed in two corpora |
| **A CFF font drew the wrong glyphs, or none.** The character code was used as the glyph index at three sites — the bare `/FontFile3`, the `CFF ` table of an `OTTO` face, and the TrueType fallback. Which symptom a file got depended on its glyph count: a *subset*, which is what a producer embeds, had no glyph at that index and the page came out blank; a full font drew the wrong letter at the right width, silently. Nothing read the charset, the encoding, the string INDEX or `FDSelect`, so no better answer was available | The charset in all three formats and the three predefined ones, the string INDEX and the 391 standard strings, the built-in encoding with supplements, `ROS`/FDArray/FDSelect with per-FD private dicts and matrices, and 9.6.6's fallback order ending in `.notdef` *and a warning*. Twenty-four tests; the fixtures are boxes of different sizes, so the assertion is which glyph drew, not that one did |
| **A composite font drew the wrong characters at exactly the right spacing.** 9.7.4 makes a Type 0 font's code a byte sequence and the CID it maps to the thing that selects both the advance and the glyph. Only the advance was told: `width_of` looked its number up by CID and the glyph lookup was handed the code, which went through `/ToUnicode` to a character and the character through the font program's own `cmap` — a table that answers "which glyph draws this character", which is not a question a composite font asks. `/CIDToGIDMap` (9.7.4.2) was read nowhere at all, and a subsetter writes it because subsetting renumbers glyphs and the CIDs must not move with them. A CJK page therefore laid out perfectly and read as a font substitution rather than a bug | `DecodedCode` carries the CID beside the code, from the same call the width goes through; `/CIDToGIDMap` is read in both forms, with a CID past the end of the table drawing `.notdef` *and reporting* rather than wrapping onto entry zero; and a composite font selects its glyph by CID. Sixteen tests over a hand-built CIDFontType2 whose code, CID and glyph are three distinct numbers — 0x41, 7 and 3 — and whose face carries a decoy `cmap`, so what proves the CID drove the choice is that the decoy's glyph is *not* what drew |
| **Every CFF INDEX was read one byte early**, so no embedded CFF or OpenType/CFF face ever resolved a glyph — `Cff::parse` refused almost every real program, and `resources.rs` read that as a font it could not read. The tests covered DICT operands and subroutine bias, none of which builds an INDEX, so nothing had ever parsed a whole program | The 1-based offset is subtracted once rather than twice. Two tests: the eight-byte reproducer, and a whole three-glyph program that also serves as the `cff` fuzz seed |
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
| **`/Info` collapsed blank onto absent.** A field present and empty — or all whitespace — read as `None`, identically to a key the document never wrote, so no caller could tell "the producer left the title blank" from "there is no title" | Present and a string is `Some`, whatever it holds, untrimmed. Plan 04 always called this a contract; the code and the comment on the line both said the opposite |
| **A stale catalog `/Version` demoted the file it sat in.** 7.7.2 lets the catalog raise the header's version, because that is how an incremental update declares a later one without rewriting header bytes a signature covers; the code let it *set* the version, so a 1.7 file carrying `/Version /1.4` reported 1.4. A repaired file whose header was unreadable reported no version at all rather than the baseline plan 04 specifies, and `/Trapped` — the ninth `/Info` field, and the one a print workflow needs — was not read | The later of the two wins, compared as the `M.N` number pair so `1.10` outranks `1.9`, with an unparseable version on either side treated as absent rather than as zero. No readable version on either side reports the 1.7 baseline and emits `HeaderMissing`, so the guess is on the record. `/Trapped` reads as its three names, with an unrecognised one as `Unknown` and only an absent key as `None` |
| **An embedded CMap could not inherit.** `usecmap` (9.7.5.3) is how a CMap says "everything the parent defines, plus these changes", and it appeared nowhere in the engine; the CMap stream dictionary's `/UseCMap` was not read either, because the dictionary's keys were never looked at. A differential CMap — the normal shape in Japanese production workflows — therefore got only the changes, and since the parent is what supplies the codespace ranges, the child had none: `decode_codes` fell back to one byte per code, so every two-byte code became two fragments and each fragment selected a CID the file never named. Alongside it, the `end*` operators were not matched by name. Each section loop stopped at the first token of an unexpected *shape*, so a truncated section ended as quietly as a complete one **and consumed the keyword that ended it**, losing whatever the next `begin*` opened | Both spellings resolve and merge: the child's ranges override the parent's where they overlap, the parent's survive where they do not, and the child's `/WMode` wins where it declares one. The chain is capped at four links and refuses a source already on it, so a CMap that uses itself terminates. Ruling 8 kept the fetch out of the leaf crate — the chain is walked in `tinker-pdf-font` and the parent is fetched through a resolver closure from `tinker-pdf-cos`, the shape `/DecodeParms` already uses. Sections now close on their own operator, with a typed warning naming which section and which stream (ruling 10). Thirty-three tests over hand-built parents that answer the same codes with *different* CIDs, asserted in parsed ranges, in a document, and in pixels — with the merge stubbed out, sixteen of them fail |
| **The `text` determinism fixture rendered nothing.** It named Helvetica and embedded no program; the engine bundles no faces, so every glyph resolved to nothing and its committed fingerprint was — bit for bit — the hash of a blank 200×100 page. The only pixel baseline in the repository covered no glyph at all, and five gap plans were queued behind "the fingerprints did not move" | The fixture embeds a synthetic TrueType face of curves, diagonals and a hole, built in the test file. Every fixture now asserts a minimum ink count and the absence of `UnreadableFont` before it is hashed, so a fixture that draws nothing fails instead of becoming the baseline |

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + deflate + JPEG + CCITT | Own inflate and deflate, LZW, ASCIIHex/85, RunLength, predictors; JPEG **baseline, extended sequential and progressive**; CCITT G3/G4 |
| [03 Encryption](plans/03-encryption.md) | reading and **writing** | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; owner vs user distinguished; `/P` read through its reserved bits; `/EncryptMetadata` and `/Crypt /Identity` honoured |
| [04 Document semantics](plans/04-document-semantics.md) | complete | Metadata incl. **`/Trapped`**, the **later** of header and catalog version, page tree with inheritance, geometry, outlines, name/number trees with **`/Limits` descent**, destination enum, page labels, actions, links, **attachments**, **XMP** |
| [05 Fonts](plans/05-fonts.md) | TrueType + CFF + **Type 1** + host seam | Encodings, CMaps, standard-14 metrics; TrueType `glyf`, CFF Type 2 **and Type 1** outlines; **CFF glyph selection: charset, string INDEX, standard strings, built-in encoding, and CID-keyed `ROS`/FDArray/FDSelect**; **composite fonts select by CID, through `/CIDToGIDMap` for a CIDFontType2 and the charset for a CIDFontType0**; **CMap inheritance through `usecmap` and `/UseCMap`, merged so the child wins where it overlaps**; `FontProvider` for faces a document does not embed. The predefined CMaps that produce those CIDs are still stubs past `Identity-H`/`-V` — gap [03](plans/gaps/03-predefined-cmaps.md), which is also what a `usecmap` chain inherits *from* today — and vertical advances still come from `/W` — gap [05](plans/gaps/05-vertical-metrics.md) |
| [06 Content & text](plans/06-content-and-text.md) | substantially | Tokenizer, text state machine, `Device` seam, text device with quads and search, **inline images**, **all stroke parameters** |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | broad, see gaps | Colour spaces incl. **Lab, Separation, DeviceN**; all four function types **and function arrays**; clipping incl. **text clip modes**; images with **`/SMask` and `/Decode`**; axial and radial shadings; **shading patterns**; **`/Rotate` and `/CropBox`**; alpha; outward pixel rounding; page-area ceiling |
| [09 Writing](plans/09-writing.md) | rewrite + incremental + object streams + encryption + **linearization** | Full rewrite with optional GC, incremental update with byte-identical prefix, classic xref, working object streams, compression, R6 encrypt-on-save, **Annex F layout** |
| [10 Editing](plans/10-editing.md) | substantially complete | Copy-on-write editor; delete/move/rotate/**insert/import/keep**; annotations with synthesized appearances **and flattening**; redaction through **forms and images** |
| [11 Forms](plans/11-forms.md) | read, fill, appearance regeneration | AcroForm field tree, fill text/choice/checkbox/radio, **comb fields**, appearances rebuilt, reset |
| [12 Creation](plans/12-creation.md) | pages, text, images, embedded fonts **with subsetting** | `DocumentBuilder` |
| [13 Bindings](plans/13-bindings.md) | all three build and are checked | C ABI, Python, JS/wasm, .NET, each able to **supply fonts** |
| [14 Testing](plans/14-testing-and-corpora.md) | tools real, fuzzing **running** | `tpdf`, `pdfcmp`, `oracle-diff`; **15** fuzz targets with committed seed corpora, a nightly job and a short per-PR run; a hostile-input sweep on stable |

## Not built

Every row below has a plan of its own in [plans/gaps/](plans/gaps/README.md),
linked in the last column — what to build, what to reuse, and how you know
when it is done. [16 Build sequence](plans/16-build-sequence.md) orders them
by value over risk.

| Gap | Consequence | Where |
| --- | --- | --- |
| **No corpus has been run** | Eight real files have been through `tpdf`. The pinned public corpora never have. **Still the largest gap between "tests pass" and "handles what exists".** | [23](plans/gaps/23-corpus-runner.md) |
| **Fuzzers run, but no long campaign has happened** | They *have* now been executed: fifteen targets, each with a committed seed corpus, each proved to build and run under `cargo-fuzz` on a nightly toolchain, and wired into a bounded nightly job and a short per-PR one. What has not happened is a real session — the longest run so far is thirty seconds a target, which found nothing and is not evidence that there is nothing. Milestone 5 of the gap plan is the campaign; until it runs, ruling 1 is better measured than it was and still not measured deeply. Building the seed corpora alone found a CFF bug (see above), which is the argument for doing the rest. | [24](plans/gaps/24-fuzz-execution.md) |
| **Linearization: external validation, and encrypt+linearize** | The layout is written and every offset it declares is checked against the bytes — but `qpdf --check` and `--show-linearization` are the arbiters the plan names and neither has been run, so the hint *tables* are unproven. `linearize` is also silently dropped when encryption is on, rather than combining with it. An *incremental* update still cannot encrypt, since it would need the original file's key. | [19](plans/gaps/19-encrypt-and-linearize.md), [20](plans/gaps/20-linearization-validation.md) |
| **CCITT `/EndOfLine`, `/EndOfBlock` parameters** | The codes are now recognised wherever they appear, but the two parameters are not consulted, `/K > 0` is not true T.4 mixed mode, and the output is one byte per pixel rather than packed 1-bpp. | [16](plans/gaps/16-ccitt-completion.md) |
| **JBIG2, JPX; mesh shadings; tiling patterns** | Reported with a warning rather than half-decoded. | [17](plans/gaps/17-jbig2-generic-region.md), [18](plans/gaps/18-jpx-decision.md), [10](plans/gaps/10-mesh-shadings.md), [09](plans/gaps/09-tiling-patterns.md) |
| **Transparency groups, soft-mask groups, blend modes** | Constant alpha works; `/SMask` on *images* works; group transparency does not. | [11](plans/gaps/11-transparency-groups.md) |
| **Determinism: the wasm leg has run, but not in CI** | ~~Written, unverified.~~ It has now been executed. `wasm32-wasip1` under wasmtime 47.0.3 reproduces all four fingerprints byte-for-byte against native Windows, along with the page dimensions and the ink counts they are computed from — 1486, 2363, 9600 and 3600 pixels — so the interesting half of ruling 4's pairing holds: a 64-bit target and a 32-bit one render the same bytes, which is where a `usize` width assumption would have shown. That is **2 of ruling 4's 4 targets**, on one machine. Linux and macOS come only from the CI matrix and no run of the `wasm-determinism` job has been observed, so four-target agreement is not yet a thing anyone has seen. The job is also now guarded against reporting success without running anything, which `cargo test` will otherwise do. Milestone 4, fixture growth, belongs to gaps 09, 10, 11 and 12. | [25](plans/gaps/25-wasm-determinism-leg.md) |
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
