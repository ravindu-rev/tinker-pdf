# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**620 tests**, `cargo fmt --check` and `clippy -D warnings` clean, and
`wasm32-unknown-unknown` builds — on every commit.

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + JPEG + CCITT | Own inflate/deflate, LZW, ASCIIHex/85, RunLength, predictors; **baseline JPEG** with subsampling, restarts, YCbCr/YCCK and Adobe's inverted CMYK; **CCITT G3 and G4** |
| [03 Encryption](plans/03-encryption.md) | complete for reading | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; **owner vs user distinguished**; **`/P` read correctly through its reserved bits** |
| [04 Document semantics](plans/04-document-semantics.md) | complete | Metadata, page tree with inheritance, geometry, outlines, name/number trees, **destination enum**, page labels, actions |
| [05 Fonts](plans/05-fonts.md) | both waves | Encodings, CMaps, standard-14 metrics, TrueType tables; TrueType `glyf` and CFF Type 2 outlines |
| [06 Content & text](plans/06-content-and-text.md) | complete | Tokenizer, full text state machine, `Device` seam, text device with quads and search |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | substantively complete | Colour operators and spaces, all four function types, clipping, **images decoded and drawn**, **axial and radial shadings**, ExtGState alpha, glyphs from embedded fonts, outward pixel rounding |
| [09 Writing](plans/09-writing.md) | rewrite + incremental + object streams | Full rewrite, **incremental update with byte-identical prefix**, classic xref, **object streams with cross-reference streams**, signature placeholder record |
| [12 Creation](plans/12-creation.md) | pages, text, images | `DocumentBuilder`: pages, base-14 text, rectangles, colour, **JPEG/RGB/grey images**, metadata, outlines — output reopens at ladder level Trust |
| [13 Bindings](plans/13-bindings.md) | all three compile | C ABI; **Python** (PyO3, releases the GIL), **JavaScript/wasm** (wasm-bindgen), **.NET** (C# over the C ABI, SafeHandle lifetimes) |

**Checkpoint A is reached and exceeded.** All 22 ported assertions from
Tinker's own suite pass — `open_documents`, `text_and_search`, `outline` and
`render_pages` — including the two that were MuPDF defects and the A4-at-150-dpi
rounding its engine left as folklore.

## Not built

Stated plainly, because a plan that reads as if everything is done is worse
than no plan.

| Gap | Consequence | Where it belongs |
| --- | --- | --- |
| **No bundled substitute fonts** | Text renders only where a document embeds its font programs. The fixtures name base-14 Helvetica and embed nothing, so their text extracts perfectly and draws nothing. Fixing it needs a bundled face — Liberation is metric-compatible and OFL — plus a licensing answer for Symbol and ZapfDingbats. | [05](plans/05-fonts.md) |
| **Corpus gate and fuzzing** | Correctness rests on unit and fixture tests, not on the real-world corpus that would prove the leniency. `pdfcmp`, `oracle-diff` and `tpdf` are still stubs and `fuzz/` holds no targets, so "never panics" is enforced by review and hostile-input tests rather than by fuzzing. **This is the largest gap between "tests pass" and "handles what exists".** | [14](plans/14-testing-and-corpora.md) |
| **Editing and forms** | No mutation API beyond creation: no annotations, page operations, redaction, flatten, or form filling. | [10](plans/10-editing.md), [11](plans/11-forms.md) |
| **Progressive JPEG, JBIG2, JPX; mesh shadings** | Reported with a warning rather than half-decoded. | [02](plans/02-filters.md), [08](plans/08-rendering-device.md) |
| **Transparency groups, soft masks, tiling patterns** | Skipped; constant alpha works. | [08](plans/08-rendering-device.md) |
| **Font embedding and subsetting in the builder** | Built documents can use the standard 14 but cannot embed a font. | [12](plans/12-creation.md) |
| **Encrypt-on-save, linearization** | The options exist and reading handles both; writing produces neither. Encryption also needs the host entropy source wired. | [09](plans/09-writing.md) |
| **Binding packaging** | All three bindings compile; none is published to PyPI, npm or NuGet, and there is no CI job building wheels or the per-RID natives. | [13](plans/13-bindings.md) |
| **Tinker integration** | Tinker still runs on MuPDF. Nothing has been swapped. | [15](plans/15-tinker-integration.md) |

## The honest summary

The engine reads PDFs properly: it opens damaged ones, decrypts them with
correct security semantics, extracts text with geometry, and answers every
structural question Tinker asks. It renders vector graphics, images and
gradients in colour, and writes valid files including object streams and
signature-safe incremental updates. Three language bindings compile against it.

Two things stand between this and replacing MuPDF in Tinker. It **cannot draw
text for documents that do not embed their fonts**, which is most simple
documents, and that needs a bundled face rather than more code. And it has
**never met a real-world corpus**, which is what would turn its leniency from
designed to demonstrated. The corpus runner is the next thing worth building,
ahead of any new feature.
