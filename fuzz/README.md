# Fuzzing

Twenty-one `cargo-fuzz` targets, one per leaf format plus two whole-pipeline ones.
Policy: [`docs/plans/14-testing-and-corpora.md`](../docs/plans/14-testing-and-corpora.md).

| Target | What it drives |
| --- | --- |
| `cos_document` | The whole file parser: every rung of the leniency ladder, then the page tree and every stream |
| `cos_object` | The object grammar of 7.3 alone, with no file around it |
| `inflate` | Own inflate, including truncated and corrupt streams |
| `lzw` | LZW with and without early change |
| `jpeg` | Baseline JPEG: Huffman tables, restarts, sampling factors |
| `ccitt` | G3 and G4; the first two input bytes choose the parameters |
| `jbig2` | T.88 segment headers, the MQ coder and generic regions; the first byte splits the body between `/JBIG2Globals` and the image's own stream |
| `jpx` | T.800: the JP2 box walk and the Annex A codestream, then tier-2 and tier-1; the first byte chooses the ceiling and whether the body is wrapped in JP2 boxes |
| `zip_archive` | A ZIP by both of its routes — the central directory and the local-header scan — with the first byte choosing the four bounds, deliberately away from the shipped defaults so the caps stay reachable inside an iteration |
| `png` | The chunk walk, both interlace methods, and the two independent length systems a PNG carries over one file: the declared chunk lengths and IHDR's geometry. The first byte chooses the output ceiling, and one of the four values it can pick is one byte |
| `xml` | XML 1.0 with namespaces: the seams between constructs rather than a record layout — a `<` inside an attribute value, a `]]` that is not a terminator, a `&` with no `;`, a prefix declared on the element that uses it. The first byte chooses the four bounds, deliberately away from the shipped defaults so a million-event cap is crossable inside one iteration; the target asserts that no document type declaration is ever *parsed*, which is the refusal gap 30 exists for |
| `ascii_filters` | ASCIIHex, ASCII85, RunLength, and the predictors |
| `sfnt` | The table directory and everything around the outlines, including the subsetter that rebuilds one |
| `truetype` | `cmap`, and `glyf` including composites |
| `cff` | The tables that decide which charstring runs — charset, encoding, string INDEX, FDSelect — and then Type 2 charstrings: subrs, `seac`, `flex`, `hintmask` |
| `type1` | eexec and charstring decryption, `/CharStrings`, othersubrs |
| `cmap` | CMap syntax as an embedded stream, then splitting a string by the codespaces it declared |
| `crypt` | The standard security handler and the ciphers under it; the input is carved into `/Encrypt`'s fields |
| `content_tokenizer` | The content-stream tokenizer |
| `form_script` | The form calculation and format scripts, as an interpreter over attacker-controlled source |
| `render_page` | Open, extract, and render arbitrary bytes end to end |

## Running

*Running* these needs a nightly toolchain and `cargo-fuzz`:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run cos_document
cargo +nightly fuzz run cos_document -- -max_total_time=3600   # a long soak
```

*Compiling* them needs neither, and CI does it on every push:

```sh
cargo check --manifest-path fuzz/Cargo.toml --all-targets
```

That distinction is not academic. This crate sits outside the workspace, so
`cargo check --workspace` never looks at it, and four of the eleven targets
called functions with the wrong arity from the day they were written — a
mistake nothing caught, because the only thing that would have caught it was
a toolchain nobody had installed. Anything unrunnable in the normal loop needs
*something* in the normal loop that would notice it rotting.

The crate is deliberately outside the workspace. `cargo-fuzz` builds it with
its own profile and sanitizer flags, and `libfuzzer-sys` pulls in a C++
runtime the engine itself refuses — tooling is exempt from the hand-rolled
rule, and none of it ships inside the library.

## What runs without nightly

Because these targets need a toolchain nobody has by accident, they cannot be
the only thing enforcing ruling 1. The same entry points are swept on every
`cargo test`, on stable, by
[`crates/tinker-pdf/tests/hostile_input.rs`](../crates/tinker-pdf/tests/hostile_input.rs):
the real fixtures put through deterministic damage, arbitrary bytes, and the
structural shapes that have historically broken PDF parsers. It is far
shallower than a fuzzer and it runs every time, which is the trade.

That test found the page-box allocation ceiling now enforced by
`MAX_PAGE_PIXELS`: `/MediaBox [0 0 1e9 1e9]` is four tokens, and a failed
allocation aborts the process rather than unwinding, so it could not have been
caught and reported after the fact.

## Corpora

Every target has a committed seed corpus under `corpus/<target>/`. They are
the difference between a run that explores and one that spends its hour
rediscovering the file header, and plan 14 asks for them minimised and small
enough to review: the whole set is 27 KB across 72 files, and the largest
single seed is 2.8 KB.

They are built from material this repository already had, so each one is
something a parser here is known to accept rather than a blob nobody can
account for:

| Target | Seeds come from |
| --- | --- |
| `ascii_filters` | Hand-written hex, ASCII85 and RunLength streams, plus rows shaped for a PNG-Up predictor |
| `ccitt` | `ccitt.rs`'s own `pack` fixtures, behind the two control bytes the target reads first |
| `cff` | `cff.rs`'s own fixtures — the same bytes, built by the tests so the two cannot drift, and rewritten by `cargo test -p tinker-pdf-font write_the_fuzz_seeds -- --ignored`: a three-glyph program, a format 0 charset with an encoding supplement and a string INDEX name, the same font with a format 2 charset, and a CID-keyed font with `ROS`, an FDArray and a format 3 FDSelect |
| `content_tokenizer` | Hand-written operator streams: text, paths, an inline image, escapes, comments |
| `cos_document` | The four `testdata/` PDFs and the four pages `determinism.rs` builds |
| `cos_object` | Hand-written 7.3 objects: dictionary, stream, nested array, every number form, two unterminated |
| `inflate` | Our own `zlib_compress` output, including an empty stream and an incompressible one |
| `jpeg` | `jpeg.rs`'s `tiny_gray`, `sequential_block` and both progressive fixtures |
| `jpx` | Twenty-five. Codestreams `opj_compress` made from *our own* 32 x 32 images — LRCP through CPRL, a boxed JP2, RGB with the RCT and with the ICT, a lossy 9/7 at a rate and truncated, two layers, explicit precincts, tiles with SOP and EPH, a subsampled component and segmentation symbols — plus hand-built ones the writer in `jpx/tests/writer.rs` emits, including 16-bit signed samples, a `pclr` palette and a `cdef` opacity channel that `opj_compress` cannot write at all, and an RGN a conformant encoder will not produce. A tool's output on our input is ours to commit; ISO/IEC 15444-4's conformance codestreams stay out, and no part of openjpeg is vendored (ruling 9). Each seed carries the target's control byte in front. `narrow-precision-crash` is the twenty-fifth and the only one a *run* produced: gap 18a milestone 8's campaign minimised it out of a six-bit codestream that `JpxImage` handed back with `precision: 6`, against a contract of 8 or 16, so the image drew at a quarter of its brightness |
| `lzw` | 7.4.4.2's Table 8 example, truncated, and a clear-code stream |
| `render_page` | The subset of the `cos_document` PDFs that has a page worth rendering, plus the composite font `composite_fonts.rs` builds — rewritten by `cargo test -p tinker-pdf --test composite_fonts write_the_fuzz_seed -- --ignored`. Nothing else in any corpus carries a Type 0 font, so `/CIDToGIDMap` — a document-controlled table the renderer indexes once per glyph — was reachable from no target at all. Gap 06 added `optional-content.pdf`, rewritten by `cargo test -p tinker-pdf --test optional_content write_the_fuzz_seed -- --ignored`: nothing in any corpus had an `/OCProperties`, so the group table, the four `/P` policies, `/VE` evaluation and the `/OC` lookups on `/Properties` and on an XObject were reachable from no target — and every one of them walks attacker-controlled indirect references. Reachable by construction, not measured: the writer asserts the seed still renders with the layer hidden, and no campaign has been run against it. Gap 30's milestone 3 added two **XPS packages**, `fixed-document.xps` and `fixed-document-openxps.oxps`, copied verbatim from `crates/tinker-pdf/tests/xps/` under the same "our input through their tool" precedent the `jpx` row records: `Document::open` now routes a `PK\x03\x04` by ECMA-388 E.3's three steps, so the OPC layer — part-name arithmetic, the content-types algorithm, relationship resolution — is reachable from **this** target and from no leaf one, and the two are the smallest package of each dialect so a mutator has both spellings to work from. Neither is a raster document, so what they exercise is the spine rather than the comic path they used to be mistaken for |
| `truetype` | `determinism.rs`'s `curvy_font`, plus a truncated copy and its directory alone |
| `sfnt` | The same face, plus the `OTTO` program `glyf.rs` builds and a directory pointing past the end |
| `type1` | `type1.rs`'s `font_with_square`, and a copy cut off inside the eexec section |
| `cmap` | A `/ToUnicode`, a `cidrange` CMap with two codespaces, a `usecmap`, a predefined name, and one malformed throughout. Gap 04 added three: a differential CMap that inherits from a predefined parent, one that names itself, and one whose sections close with the wrong `end*` operator or with none. The target drives `parse_embedded` with three resolvers built out of the input, since a `usecmap` chain past the predefined set needs a caller and there is no document here to be one. Gap 03 added four more, because until it landed `CMap::predefined` was a fourteen-entry prefix list and there was no table to reach: each names a real registry CMap on its first line, which is where the target looks for one, and follows it with bytes chosen for the new code. `predefined-registry` is Shift-JIS with an undefined two-byte code and a lead byte no range claims; `predefined-vertical` drives the registry's own `usecmap`, so the CMap is built recursively and merged; `predefined-gb18030` is the two-byte/four-byte overlap that per-byte codespace matching exists for; `predefined-utf8` has four codespace widths and sequences that are not UTF-8 |
| `crypt` | `build_r6` output laid out in the target's carve order, so one seed authenticates and decrypts; plus R4, R2, and three bytes |
| `zip_archive` | Five, from `tinker-pdf-zip`'s own hand-built archives — the same bytes, written by the tests so the two cannot drift, and rewritten by `cargo test -p tinker-pdf-zip write_the_fuzz_seeds -- --ignored`: both compression methods with a CP437 name beside two UTF-8 ones, an archive with no central directory so the local-header scan is the route, and an entry whose sizes are in a data descriptor. **Two of the five repeat an archive behind a control byte of zero**, which sets every cap to the lowest value the target offers — one entry, sixteen bytes an entry, no inflation at all, one byte of name. A corpus in which every seed is roomy explores the happy path and reaches no refusal, which is gap 18a milestone 8's failure arriving through the corpus instead of through the constant |
| `png` | Five, from `png.rs`'s own fixtures and rewritten by `cargo test -p tinker-pdf-filters write_the_fuzz_seeds -- --ignored`: the two committed 8 x 8 files whose sixty-four pixels are all distinct, one plain and one Adam7, plus an indexed file with a `PLTE` and a `tRNS` — the pass-through's shape, and the one branch of `png_scan` with a table to bounds-check — and a 16-bit RGBA raster, which is the widest layout Table 11.1 allows and therefore the most `MAX_PNG_SAMPLES` is ever charged. The fifth repeats the plain file behind a control byte of zero, a ceiling of **one byte**, which is the only value from which `ExceedsOutputLimit` fires on an otherwise perfectly good file |
| `xml` | Six. Two are the fixed page of `wpf-image-and-text.xps` — markup Microsoft's own serialiser wrote, and the only XML in this repository the repository did not write — one roomy and one behind a control byte of zero. Then `billion-laughs`, so the corpus holds the input the crate exists to refuse and a mutator has the shape to work from; `namespaces-tight`, which shadows a prefix, undeclares one and uses `xml:lang`, also at a control byte of zero; `utf16`, little-endian with a mark, which nothing else in any corpus here is; and `asides`, which carries a comment, two processing instructions, a CDATA section whose body holds `]]`, and all five predefined entities beside both radixes of character reference. **Two of the six set every knob to the tightest value the target offers** — one element deep, no attributes, one byte of name, no events at all — because a corpus in which every seed is roomy explores the happy path and reaches no refusal |

Replaying a corpus without mutating it is the cheapest check that a seed
still reaches what it was chosen for:

```sh
cargo +nightly fuzz run cff corpus/cff -- -runs=0
```

Two things a run does to `corpus/` that are easy to be surprised by: it
writes every new unit it finds straight into the directory it was given, and
`cargo fuzz cmin` rewrites that directory in place, renaming every file to
its hash. Neither is reversible and both discard the names above, so minimise
a *copy* unless you mean to lose them. `git status` after a run is worth a
look before `git add`.

Large third-party corpora are fetched by pinned checksum and never committed —
see the plan for the per-corpus licence table.

## Crashes

`cargo-fuzz` writes a failing input to `artifacts/<target>/`. It does not stay
there: minimise it with `cargo +nightly fuzz tmin <target> <artifact>`, then
land the minimised bytes as an ordinary test in the crate that owns the
parser, in the style `hostile_input.rs` uses, with a comment naming what it
triggered. The value is not the fuzzer finding it once; it is the reproducer
running on every `cargo test` afterwards, on stable, where it cannot be
ignored.
