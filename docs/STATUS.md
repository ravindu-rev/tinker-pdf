# Status

What is built, what is not, and what the difference means. Updated as phases
land; the plan files say what *should* exist, this says what *does*.

**585 tests**, `cargo fmt --check` and `clippy -D warnings` clean, and
`wasm32-unknown-unknown` builds — on every commit.

## Built

| Phase | State | What works |
| --- | --- | --- |
| [01 COS](plans/01-cos-and-object-model.md) | milestones 1–4 | Lexer, object model, xref in every flavour, object streams, lazy `Send + Sync` store, repair scanner, leniency ladder, three stream tiers |
| [02 Filters](plans/02-filters.md) | wave 1 + baseline JPEG | Own inflate/deflate, LZW, ASCIIHex/85, RunLength, predictors; **baseline JPEG** with subsampling, restarts, YCbCr/YCCK and Adobe's inverted CMYK |
| [03 Encryption](plans/03-encryption.md) | complete for reading | Own MD5, RC4, SHA-2, AES-CBC; handlers R2–R6; **owner vs user distinguished**; **`/P` read correctly through its reserved bits** |
| [04 Document semantics](plans/04-document-semantics.md) | complete | Metadata, page tree with inheritance, geometry, outlines, name/number trees, **destination enum**, page labels, actions |
| [05 Fonts](plans/05-fonts.md) | both waves | Encodings, CMaps, standard-14 metrics, TrueType tables; TrueType `glyf` and CFF Type 2 outlines |
| [06 Content & text](plans/06-content-and-text.md) | complete | Tokenizer, full text state machine, `Device` seam, text device with quads and search |
| [07 Rasterizer](plans/07-rasterizer.md) | complete | Paths, deterministic anti-aliased fill, stroking with caps/joins/dashes, clipping, compositing |
| [08 Rendering device](plans/08-rendering-device.md) | first milestone | Colour spaces, all four function types, paths and glyphs drawn, page transform, outward pixel rounding |
| [09 Writing](plans/09-writing.md) | rewrite + incremental | Full rewrite, **incremental update with byte-identical prefix**, classic xref, signature placeholder record |
| [12 Creation](plans/12-creation.md) | first milestone | `DocumentBuilder`: pages, text, rectangles, metadata, outlines — output reopens at ladder level Trust |
| [13 Bindings](plans/13-bindings.md) | C ABI | Handle-based, thread-safe, per-thread errors, documented ownership |

**Checkpoint A is reached and exceeded.** All 21 ported assertions from
Tinker's own suite pass — `open_documents`, `text_and_search`, `outline` and
`render_pages` — including the two that were MuPDF defects and the A4-at-150-dpi
rounding its engine left as folklore.

## Not built

Stated plainly, because a plan that reads as if everything is done is worse
than no plan.

| Gap | Consequence | Where it belongs |
| --- | --- | --- |
| **Images not wired to the renderer** | The baseline JPEG decoder exists and is tested, but the rendering device does not yet fetch and draw image XObjects — so a page with one still reports `UnsupportedImage`. This is now plumbing, not a missing decoder. | [08](plans/08-rendering-device.md) |
| **CCITT, progressive JPEG, JBIG2, JPX** | Reported rather than half-decoded. | [02 wave 2](plans/02-filters.md) |
| **Colour operators in the interpreter** | Everything paints black. The colour machinery exists and is tested; the interpreter does not yet track `cs`/`sc`/`rg`/`k` and hand it over. | [08](plans/08-rendering-device.md) |
| **Glyph source wiring** | The renderer draws outlines it is given, and the font crate produces them, but the facade does not yet connect embedded font programs to it — so text renders only where a document embeds nothing. | [08](plans/08-rendering-device.md) |
| **Shadings, patterns, transparency groups, soft masks** | Skipped with a warning. | [08](plans/08-rendering-device.md) |
| **Editing, forms** | No mutation API beyond creation. | [10](plans/10-editing.md), [11](plans/11-forms.md) |
| **Python, JS/wasm and .NET packages** | The C ABI and the facade are ready; the packaging is not. | [13](plans/13-bindings.md) |
| **Corpus gate and fuzzing campaign** | Correctness is asserted by unit and fixture tests, not yet by the real-world corpus that would prove leniency. This is the largest gap between "tests pass" and "handles what exists". | [14](plans/14-testing-and-corpora.md) |
| **Linearization, encrypt-on-save, object streams on write** | Reading handles all three; writing does not yet produce them. | [09](plans/09-writing.md) |

## The honest summary

The engine reads PDFs properly: it opens damaged ones, decrypts them with
correct security semantics, extracts text with geometry, and answers every
structural question Tinker asks. It renders geometry and writes valid files.

It is **not yet a viewer-grade renderer** — no images, no colour, no embedded
glyphs wired through — and it has **not met a real-world corpus**, which is
what would turn its leniency from designed to proven. Those two gaps are the
distance between what exists and what could replace MuPDF in Tinker.
