# Fuzzing

`cargo-fuzz` targets, one per format plus whole-pipeline ones. Each file in
`fuzz_targets/` opens with a header saying what it drives and asserts.
Policy: [`docs/verification.md`](../docs/verification.md). Among them:

| Target | What it drives |
| --- | --- |
| `cos_document` | The whole file parser: every rung of the leniency ladder, then the page tree and every stream |
| `cos_object` | The object grammar of 7.3 alone, with no file around it |
| `cos_syntax` | Whole files a generator writes from a model (`src/cos.rs`): every object type of 7.3, each token spelled one of the ways the clause allows, in one to four sections — tables, cross-reference streams and object streams, updates that redefine, free and reuse objects, `/Length` direct or indirect, honest or lying. **Structure-aware**: the input is read as choices, so a mutation changes a decision rather than breaking the syntax, and because the target knows what it wrote it asserts the reader read it back — every object as written, a clean file on `Trust` with no warning and no structure-tier defect, and a lying length recovered wherever 7.3.8.2 leaves it recoverable |
| `annotation_appearance` | Appearance synthesis (12.5.5, 12.5.6) for one annotation dictionary parsed from the bytes, against a fixed form whose `/DR` holds four fonts so `/DA` reaches every free text branch: what is written must stay bounded by the input and lex with no leniency |
| `inflate` | Own inflate, including truncated and corrupt streams |
| `lzw` | LZW with and without early change |
| `jpeg` | Baseline JPEG: Huffman tables, restarts, sampling factors |
| `ccitt` | G3 and G4; the first two input bytes choose the parameters |
| `jbig2` | T.88 segment headers, the MQ coder and generic regions; the first byte splits the body between `/JBIG2Globals` and the image's own stream |
| `jpx` | T.800: the JP2 box walk and the Annex A codestream, then tier-2 and tier-1; the first byte chooses the ceiling and whether the body is wrapped in JP2 boxes |
| `jpx_codestream` | Codestreams a generator writes (`src/jpx.rs`) from T.800's Annexes A to D: SIZ, COD, COC, QCD, QCC and POC that agree with one another, in the main header and in tile-part headers; up to nine tiles cut into tile-parts and interleaved; packets in all five progression orders and over POC volumes, their headers in the bit stream, in PPT or in PPM, with B.10's tag trees, pass counts, `Lblock` and lengths; code-block data that is the input's own bytes or Annex D's coding of generated coefficients through every Table A.19 style. One field may be broken to a value the standard rules out. **Structure-aware**: a codestream written whole must decode to the width, height and component count it was written with, and one with a field broken must decode to them too or be refused by a name of that field |
| `zip_archive` | A ZIP by both of its routes — the central directory and the local-header scan — with the first byte choosing the four bounds, deliberately away from the shipped defaults so the caps stay reachable inside an iteration |
| `png` | The chunk walk, both interlace methods, and the two independent length systems a PNG carries over one file: the declared chunk lengths and IHDR's geometry. The first byte chooses the output ceiling, and one of the four values it can pick is one byte |
| `xml` | XML 1.0 with namespaces: the seams between constructs rather than a record layout — a `<` inside an attribute value, a `]]` that is not a terminator, a `&` with no `;`, a prefix declared on the element that uses it. The first byte chooses the four bounds, deliberately away from the shipped defaults so a million-event cap is crossable inside one iteration; the target asserts that no document type declaration is ever *parsed* |
| `xml_structured` | Documents a generator writes (`src/markup.rs`) from one element tree: names with and without a prefix, attributes in no namespace, a bound one and `xml`'s, prefix and default namespace declarations among them, the five predefined entities and character references across §2.2's ranges, CR LF line ends, CDATA, comments and processing instructions, and an XML declaration or none; one defect it names — an undeclared prefix, a mismatched end tag, a duplicate attribute, a bare `&` — and a document type declaration, bare, external or with an internal subset, where the input asks. **Structure-aware**: in both `Doctype` modes a well-formed document reads back as written, element for element with its attributes and its character data whole; a declaration is refused or read past as the mode says, and a defect is refused as itself |
| `html_structured` | The same tree written as HTML tag soup aimed at the tree builder's insertion modes: text and elements straight inside tables, formatting elements closed out of order, `template`, `select`, `svg` and `math` with CDATA, raw-text elements with markup in them, end tags left out, stray end tags, attributes quoted three ways and repeated, references with and without semicolons, and a standards or quirks declaration, under four bounds the input chooses. **Structure-aware**: the tree built is a tree inside `Limits::max_tokens` and the attribute cap, and parsing is deterministic |
| `css` | `css-syntax-3`'s tokenizer and grammar with its normative error recovery, `selectors-4` matching and specificity, and `css-cascade-5`'s whole sorting order. Text rather than a record layout, like `xml`: what a mutator finds is the seams — a `;` inside a function, a `}` inside a string, an `@media` whose block never closes, an `@import` that names itself. The stylesheet is fuzzed **and so is the tree it cascades over**, out of the same bytes, or the matcher is never reached. The first byte chooses the bounds and its last pair carries three related knobs, because all eight bits are spent and a second control byte would shift every corpus entry ever written. The strongest assertion is that the rule **index and brute force agree**: a bucketing bug produces a book styled slightly less than it should be, which reads as a plain stylesheet rather than as a defect |
| `css_structured` | Stylesheets a generator writes (`src/css.rs`): selectors of all four combinators, attribute selectors of every matcher and case flag, the pseudo-classes the matcher implements — `:nth-*` with `of S`, `:not()`, `:is()`, `:where()`, `:has()`, `:lang()`, `:dir()`, the user-interface states and the seven of a reading session — and four pseudo-elements; declarations implemented, unsupported and unknown, defaulted and `!important`; `@import`s naming one another, `@media`, `@supports`, `@layer`, `@font-face` and an unknown at-rule, over a generated tree. **Structure-aware**, and its assertion is a relation: the sheet written again with every selector in `:is()`, which the rule index cannot bucket, parses to the same rules and cascades to the same styles, so the indexed cascade is held to brute force's |
| `layout` | The box model with `box-sizing`, CSS 2.2 section 8.3.1's collapsing margins, section 9.4's block and inline formatting contexts, `css-text-3` section 4.1's white-space processing, UAX #14 line breaking with section 5's tailorings, and section 13.3's fragmentation into pages. **The only target here whose input is not bytes**, because `css` is a parser and `layout` is an algorithm whose inputs are already-validated structures and whose failure mode is unbounded work. It reads two control bytes, and the rest drive a **structured generator** -- a tree of boxes, their styles and their text out of a small alphabet chosen for the line breaker's own rules -- since a target that handed these bytes to a parser would spend its session being refused at the door. The strongest assertion is **text conservation**: every non-whitespace character of the tree appears on some page exactly once in order, which is the invariant that survives every level of CSS partiality and the one a rendered comparison cannot see |
| `ascii_filters` | ASCIIHex, ASCII85, RunLength, and the predictors |
| `sfnt` | The table directory and everything around the outlines, including the subsetter that rebuilds one |
| `truetype` | `cmap`, and `glyf` including composites |
| `glyf_outlines` | A minimal `sfnt` a generator writes (`src/glyf.rs`): the directory, `head`, `hhea`, `hmtx`, `maxp`, a short or long `loca` and `glyf`, simple glyphs in every flag and coordinate form and composites in every component form, nested four deep. **Structure-aware**: every glyph must draw one closed contour per contour it holds, inside the caps, with the advance `hmtx` states, and a subset that keeps a glyph must draw it the same |
| `cff` | The tables that decide which charstring runs — charset, encoding, string INDEX, FDSelect — and then Type 2 charstrings: subrs, `seac`, `flex`, `hintmask` |
| `type1` | eexec and charstring decryption, `/CharStrings`, othersubrs |
| `cff_charstrings` | A bare CFF a generator writes (`src/cff.rs`): INDEXes with chosen offset widths, the charset in all three formats, a CID-keyed font's FDArray and FDSelect, subroutine INDEXes either side of the 107 / 1 131 / 32 768 bias edges, and Type 2 charstrings of every operator — widths, stem hints with their masks, the flex family, biased calls without a cycle, `seac`. **Structure-aware**: every glyph's outline must stay inside the caps with its advance the width it was written with, and a subset that keeps a glyph must draw it the same and read the same advance, under the subset's own bias |
| `cmap` | CMap syntax as an embedded stream, then splitting a string by the codespaces it declared |
| `crypt` | The standard security handler: the input is carved into `/Encrypt`'s fields **at widths it also chooses**, since the widths are the file's to declare and a wrong one is what reaches the early returns. A valid revision-6 handler is built once per process, so `FileKey`'s whole surface — the per-object key, both decryption doors — is driven on every input rather than on the two seeds that happen to authenticate |
| `crypt_ciphers` | AES, RC4, MD5 and SHA-2 driven directly, with no key derivation anywhere: round trips and two-door cross-checks, because a cipher has no other oracle. Kept apart from `crypt`, where the handler's hardened hash dominates every input |
| `content_stream` | A page a generator writes (`src/content.rs`): content of every operator of Annex A, its operands of the shape each takes or of a wrong one, blocks balanced and not, inline images, and forms, a Type 3 font's glyph procedures, patterns and appearances of generated content, over generated fonts, images, graphics states, colour spaces, shadings and property lists, rendered with generated options. **Structure-aware**, and its assertion is a relation: the page's display list replays to the bitmap `Page::render` draws — shape, every byte, and the warnings |
| `content_tokenizer` | The content-stream tokenizer |
| `form_script` | The form calculation and format scripts, as an interpreter over attacker-controlled source |
| `render_page` | Open, extract, and render arbitrary bytes end to end |
| `render_rewrite` | A document a generator writes (`src/document.rs`): `content_stream`'s page, laid out in one to four sections of tables and cross-reference streams, objects packed into object streams or not, updates that write the page's objects again and add and free others, every token spelled one of the ways 7.3 allows. **Structure-aware**, and its assertion is a relation: the document opens without repair, and once read cleanly it is rewritten, reopened and rendered again, every page the same shape, every byte and every warning |

## Running

*Running* these needs a nightly toolchain and `cargo-fuzz`, and every
session runs under its target's row of [`limits.tsv`](limits.tsv): the
`-rss_limit_mb` and `-malloc_limit_mb` `crates/tinker-pdf/tests/bounds_ledger.rs`
derives from the bounds the target's path reaches, on a stack of `stack_kib`.
[`docs/verification.md`](../docs/verification.md) gives the loop that runs
every target that way, and one target is the same thing alone:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz build
export ASAN_OPTIONS=quarantine_size_mb=64:allocator_release_to_os_interval_ms=500
IFS=$'\t' read -r _ rss malloc stack < <(grep -P '^cos_document\t' fuzz/limits.tsv)
( ulimit -s "$stack" &&
  fuzz/target/x86_64-unknown-linux-gnu/release/cos_document fuzz/corpus/cos_document \
    -rss_limit_mb="$rss" -malloc_limit_mb="$malloc" -max_total_time=3600 )   # a long soak
```

The table is written by `cargo test -p tinker-pdf --test bounds_ledger
write_the_fuzz_limits -- --ignored` and never by hand: a session that fails
under its limits has found an allocation its bounds do not admit, and the fix
is a bound, not a larger number.

*Compiling* them needs neither, and `cargo xtask fuzz` — inside `cargo
xtask check` — does it:

```sh
cargo check --manifest-path fuzz/Cargo.toml --all-targets
```

The crate's library has unit tests of its own, and they run on stable too:

```sh
cargo test --manifest-path fuzz/Cargo.toml --lib
```

That distinction is not academic. This crate sits outside the workspace, so
`cargo check --workspace` never looks at it, and anything unrunnable in the
normal loop needs *something* in the normal loop that would notice it
rotting.

The crate is deliberately outside the workspace. `cargo-fuzz` builds it with
its own profile and sanitizer flags, and `libfuzzer-sys` pulls in a C++
runtime the engine itself refuses — tooling is exempt from the hand-rolled
rule, and none of it ships inside the library.

## The continuous-fuzzing service's build

`service/build.sh` is the build a public continuous-fuzzing service runs
([`docs/design/continuous-fuzzing.md`](../docs/design/continuous-fuzzing.md)):
every target in `fuzz_targets/`, built in the local sessions' mode, written
to `$OUT` beside `<target>_seed_corpus.zip`, every file of
`corpus/<target>/` packed when the build runs, and `<target>.options`,
which holds the sessions' `timeout = 20`. `service/Dockerfile` is the
service's recipe with its clone of this repository replaced by the commit
it is built from. To build it locally, from the checkout's root, on a
machine with Docker running Linux containers:

```sh
git archive HEAD | docker build -f fuzz/service/Dockerfile -t tinker-pdf-fuzz-service -
docker run --rm --privileged -e FUZZING_LANGUAGE=rust -e SANITIZER=address \n  -e FUZZING_ENGINE=libfuzzer -e ARCHITECTURE=x86_64 \n  -v "$PWD/target/fuzz-service:/out" tinker-pdf-fuzz-service compile
cargo xtask fuzz --service-out target/fuzz-service
```

The first builds the image over the commit, the second is the service's own
build in it — `compile` runs `build.sh` in the configuration the service's
address build sets, and is privileged because `compile` sets a kernel
parameter first — and the third checks what it wrote: each target's binary
an ELF file, its seed archive exactly its corpus, file for file, and its
options the sessions' timeout. Nothing in the three runs a target.

## Structure-aware targets

`src/lib.rs` is the crate's library: `Choices`, the reader a structure-aware
target reads its input through, and every generator, a module each, beside
the seed writer its target's corpus needs; the target holds the assertions.
An input that has run out reads as zeros, and zero is the plainest
choice wherever a generator asks one, so every input — the empty one
included — is a well-formed structure, and a mutation changes one decision
rather than breaking the syntax. Such a target's seeds are choice sequences,
each written by its generator's `write_the_fuzz_seeds` and named for the
feature it reaches; a change to what a generator reads first runs the writer
again. [`docs/design/structured-fuzzing.md`](../docs/design/structured-fuzzing.md)
is the design.

## What runs without nightly

Because these targets need a toolchain nobody has by accident, they cannot be
the only thing enforcing ruling 1. The same entry points are swept on every
`cargo test`, on stable, by
[`crates/tinker-pdf/tests/hostile_input.rs`](../crates/tinker-pdf/tests/hostile_input.rs):
the real fixtures put through deterministic damage, arbitrary bytes, and the
structural shapes known to break PDF parsers. It is far shallower than a
fuzzer and it runs every time, which is the trade.

## Corpora

Every target has a committed seed corpus under `corpus/<target>/`. Seeds are
the difference between a run that explores and one that spends its hour
rediscovering the file header, and they are kept minimised and small enough
to review.

They are built from material this repository already had, so each one is
something a parser here is known to accept rather than a blob nobody can
account for:

| Target | Seeds come from |
| --- | --- |
| `ascii_filters` | Hand-written hex, ASCII85 and RunLength streams, plus rows shaped for a PNG-Up predictor |
| `ccitt` | `ccitt.rs`'s own `pack` fixtures, behind the two control bytes the target reads first |
| `cff` | `cff.rs`'s own fixtures — the same bytes, built by the tests so the two cannot drift, and rewritten by `cargo test -p tinker-pdf-font write_the_fuzz_seeds -- --ignored`: a three-glyph program, a format 0 charset with an encoding supplement and a string INDEX name, the same font with a format 2 charset, and a CID-keyed font with `ROS`, an FDArray and a format 3 FDSelect |
| `content_stream` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its page holds — text in the Type 3 font, forms in transparency groups, inline images and a stencil, image XObjects, patterns and shadings, a soft mask, annotations, marked content, a page of two content streams, a Separation, a DeviceN and a Lab space, and a region rendered in ink — and `plain`, the empty input, whose page is its opening rectangle alone |
| `content_tokenizer` | Hand-written operator streams: text, paths, an inline image, escapes, comments |
| `cos_document` | `testdata/` PDFs and the pages `determinism.rs` builds |
| `cos_object` | Hand-written 7.3 objects: dictionary, stream, nested array, every number form, two unterminated |
| `cos_syntax` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored`: each is the first input of a fixed pseudo-random sequence whose file has the feature it is named for, cut to the bytes the generator read — one table, one cross-reference stream, an object stream plain and compressed, rows under the Up predictor, four sections, a number freed and used again, a lying `/Length` direct and indirect, every kind of value, nesting to the cap — and `plain`, the empty input, which reads as zeros throughout and so is the plainest file the generator writes |
| `inflate` | Our own `zlib_compress` output, including an empty stream and an incompressible one |
| `jpeg` | `jpeg.rs`'s `tiny_gray`, `sequential_block` and both progressive fixtures |
| `jpx` | Codestreams `opj_compress` made from *our own* 32 x 32 images — LRCP through CPRL, a boxed JP2, RGB with the RCT and with the ICT, a lossy 9/7 at a rate and truncated, two layers, explicit precincts, tiles with SOP and EPH, a subsampled component and segmentation symbols — plus hand-built ones the writer in `jpx/tests/writer.rs` emits, including 16-bit signed samples, a `pclr` palette and a `cdef` opacity channel that `opj_compress` cannot write at all, and an RGN a conformant encoder will not produce. A tool's output on our input is ours to commit; ISO/IEC 15444-4's conformance codestreams stay out, and no part of openjpeg is vendored, linked or invoked (ruling 13). Each seed carries the target's control byte in front. `narrow-precision-crash` is the one a *run* produced: a six-bit codestream, which holds `JpxImage` to its contract of a `precision` of 8 or 16 |
| `jpx_codestream` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its codestream holds — each of the five progression orders, POC volumes in the main header and a tile's, interleaved tile-parts with `TNsot` unstated, tile-level COD, COC, QCD and QCC, subsampled components, the RCT and the ICT, SOP and EPH, the bypass with each pass terminated, raw bypass passes, `RESET`, `VERTICALLY_CAUSAL` and `PREDICTABLE`, segmentation symbols, packet headers in PPM and in PPT, a last `Psot` of zero, 16-bit signed samples, and a field broken in SIZ, in COD and in an SOT — and `plain`, the empty input |
| `lzw` | 7.4.4.2's Table 8 example, truncated, and a clear-code stream |
| `render_page` | The subset of the `cos_document` PDFs that has a page worth rendering, plus the composite font `composite_fonts.rs` builds — rewritten by `cargo test -p tinker-pdf --test composite_fonts write_the_fuzz_seed -- --ignored` — which is the corpus's Type 0 font, so `/CIDToGIDMap`, a document-controlled table the renderer indexes once per glyph, is reachable. `optional-content.pdf`, rewritten by `cargo test -p tinker-pdf --test optional_content write_the_fuzz_seed -- --ignored`, carries the corpus's `/OCProperties`, so the group table, the four `/P` policies, `/VE` evaluation and the `/OC` lookups on `/Properties` and on an XObject are reachable — and every one of them walks attacker-controlled indirect references; the writer asserts the seed still renders with the layer hidden. Two **XPS packages**, `fixed-document.xps` and `fixed-document-openxps.oxps`, are copied verbatim from `crates/tinker-pdf/tests/xps/` under the same "our input through their tool" precedent the `jpx` row records: `Document::open` routes a `PK\x03\x04` by ECMA-388 E.3's three steps, so the OPC layer — part-name arithmetic, the content-types algorithm, relationship resolution — is reachable from **this** target and from no leaf one, and the two are the smallest package of each dialect so a mutator has both spellings to work from. `fixed-document-interleaved.xps` is `fixed-document.xps` with its page cut into three OPC 7.2.4 pieces and `[Content_Types].xml` into two, written round-robin by CPython's `zipfile` with the one-liner `tests/xps_interleaved/make-interleaved.py` generalises, so the piece grouping and joining in `xps::opc` — which no leaf target reaches either — has a seed that assembles. Three **inputs the fuzzer found** are kept as seeds, so every run starts from them: `soft-mask-names-its-own-page.pdf`, whose ExtGState `/SMask` names the page's own content stream as its group, so every mask group opens three more; `soft-mask-mutated-past-its-seed.pdf`, that page with 252 bytes mutated, whose knockout group reads on into a copy of the page and draws itself twice, leaving a `q` open each time; and `form-reads-into-the-page-that-draws-it.pdf`, that page mutated another way, whose knockout form lost its `/Group` and reads on into two copies of the page that each draw it — a stream already running in its own ancestry is not run again there, so each draws once on any path in every reading, `transparency_groups.rs` holds each of this target's calls on all three, and `stats.rs` counts the third's work |
| `render_rewrite` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its document holds — four sections, cross-reference streams, object streams, a freed object, text in the Type 3 font, image XObjects beside inline images, patterns and shadings, transparency, annotations, and a region rendered over updates — and `plain`, the empty input, which is `content_stream`'s plainest page |
| `truetype` | `determinism.rs`'s `curvy_font`, plus a truncated copy and its directory alone |
| `glyf_outlines` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its face holds — a short and a long `loca`, compressed flag runs, every coordinate form, nested composites, point-matched components, the three scale forms, the offset and rounding flags, word arguments with `USE_MY_METRICS`, `OVERLAP_COMPOUND` and instructions, empty glyphs beside a composite, and sixteen glyphs — and `plain`, the empty input |
| `sfnt` | The same face, plus the `OTTO` program `glyf.rs` builds and a directory pointing past the end |
| `type1` | `type1.rs`'s `font_with_square`, and a copy cut off inside the eexec section |
| `cff_charstrings` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its font holds — a local subroutine INDEX at each of 1 239, 1 240, 33 899 and 33 900, a global one at 1 240 and at 33 900, a CID-keyed font of several Font DICTs, glyphs ending in `seac`, glyphs that are `endchar` alone, masks inside subroutines, and twelve glyphs — and `plain`, the empty input |
| `cmap` | A `/ToUnicode`, a `cidrange` CMap with two codespaces, a `usecmap`, a predefined name, and one malformed throughout; a differential CMap that inherits from a predefined parent, one that names itself, and one whose sections close with the wrong `end*` operator or with none. The target drives `parse_embedded` with three resolvers built out of the input, since a `usecmap` chain past the predefined set needs a caller and there is no document here to be one. Four name a real registry CMap on their first line, which is where the target looks for one, and follow it with bytes chosen for that CMap's code: `predefined-registry` is Shift-JIS with an undefined two-byte code and a lead byte no range claims; `predefined-vertical` drives the registry's own `usecmap`, so the CMap is built recursively and merged; `predefined-gb18030` is the two-byte/four-byte overlap that per-byte codespace matching exists for; `predefined-utf8` has four codespace widths and sequences that are not UTF-8 |
| `crypt` | Written by `cargo test -p tinker-pdf-crypto write_the_fuzz_seeds -- --ignored` rather than by hand, so the seeds follow the target's carve order: bytes laid by hand with nothing tying them to that order would turn the seed that authenticates into noise when the order changed, and a corpus that no longer reaches what it was chosen for looks exactly like one that does. `build_r6` output laid out to authenticate and decrypt, the same with one byte of `/Perms` flipped, R4, R2, three bytes, and two more. `r6-boundary-widths` carries `/O` and `/U` at **47** bytes, one short of the width revision 6 checks, so `authenticate_r6`'s early returns are reachable rather than every revision-6 execution paying the hardened hash for one control-flow shape. `r4-authenticates` is a **pre-revision-6 document that matches its own password**, built with the crate's own `compute_key_legacy` and `expected_u` from inside the module rather than with a restatement of Algorithms 2, 4 and 5 in the target: 7.6.2's per-object key sits behind a `FileKey`, a `FileKey` exists only after a password matched, and matching by mutation means colliding a 128-bit digest, so without this seed `object_key` is unreachable |
| `crypt_ciphers` | From the same writer: a body that is already a whole number of blocks, which is where PKCS#7 adds an entire block of its own; a ragged one at AES-256; an empty body behind a **24-byte** key, which is the zero-length pad and the zero-length digest and also the FIPS 197 width this crate deliberately does not expand; and a five-byte key, which is no cipher's |
| `zip_archive` | `tinker-pdf-zip`'s own hand-built archives — the same bytes, written by the tests so the two cannot drift, and rewritten by `cargo test -p tinker-pdf-zip write_the_fuzz_seeds -- --ignored`: both compression methods with a CP437 name beside two UTF-8 ones, an archive with no central directory so the local-header scan is the route, and an entry whose sizes are in a data descriptor. The `-tight` seeds repeat an archive behind a control byte of zero, which sets every cap to the lowest value the target offers — one entry, sixteen bytes an entry, no inflation at all, one byte of name — because a corpus in which every seed is roomy explores the happy path and reaches no refusal |
| `png` | `png.rs`'s own fixtures, rewritten by `cargo test -p tinker-pdf-filters write_the_fuzz_seeds -- --ignored`: the two committed 8 x 8 files whose sixty-four pixels are all distinct, one plain and one Adam7, plus an indexed file with a `PLTE` and a `tRNS` — the pass-through's shape, and the one branch of `png_scan` with a table to bounds-check — and a 16-bit RGBA raster, which is the widest layout Table 11.1 allows and therefore the most `MAX_PNG_SAMPLES` is ever charged. `plain-8x8-tight` repeats the plain file behind a control byte of zero, a ceiling of **one byte**, which is the only value from which `ExceedsOutputLimit` fires on an otherwise perfectly good file |
| `xml` | The fixed page of `wpf-image-and-text.xps` — markup Microsoft's own serialiser wrote — once roomy and once behind a control byte of zero. Then `billion-laughs`, so the corpus holds the input the crate exists to refuse and a mutator has the shape to work from; `namespaces-tight`, which shadows a prefix, undeclares one and uses `xml:lang`, also at a control byte of zero; `utf16`, little-endian with a mark; and `asides`, which carries a comment, two processing instructions, a CDATA section whose body holds `]]`, and all five predefined entities beside both radixes of character reference. The seeds behind a control byte of zero set every knob to the tightest value the target offers — one element deep, no attributes, one byte of name, no events at all — because a corpus in which every seed is roomy explores the happy path and reaches no refusal |
| `xml_structured` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its document holds — prefixes and a default namespace, references, CDATA, comments and instructions, CR LF line ends, nesting eight deep, forty elements, each form of declaration, and each defect — and `plain`, the empty input, which writes `<div></div>` |
| `html_structured` | Choice sequences from the same writer, each named for what its soup holds — fostered content, mis-nested formatting elements, tables, templates, selects, foreign content, raw text, omitted and stray end tags, a quirks declaration, and a token cap the parse reaches — and `plain`, the empty input |
| `css` | `ordinary` is a stylesheet the shape of a real book's — the property names the committed corpus uses, `@media screen` and `@media print` side by side, and the `page-break-*` pair both producers write. `selectors` is the specificity table's hard rows as a stylesheet: `:not()` inside `:is()`, `:where()`, all seven attribute matchers with both case flags, all four pseudo-elements, and two selectors that are *invalid* so the recovery path is in the corpus too. `recovery` is nothing but §5.4.4 and §5.4.2 — a bad declaration, a bad rule, an unterminated string, an unterminated comment, `!IMPORTANT` in the wrong place. `imports` drives both `@import` guards from the input's own text: an href beginning `cycle` comes back at a constant address and must hit the cycle guard, anything else comes back one segment deeper and must hit the depth cap — and a build missing either does not fail an assertion, it hangs, which is what the nightly's `-timeout` is for. The `tight` seeds sit behind a control byte of zero, sixty-four bytes and four tokens and one compound and no matching at all, because a corpus in which every seed is roomy explores the happy path and reaches no refusal. `repeated-class` is the one a *run* produced: a `.note` rule and an element carrying the class `note` twice, on which the rule index and brute force must agree. Applying one declaration twice changes no page, so a defect there is invisible in pixels; what it multiplies is the **match budget**, out of one attribute that `MAX_DOM_NODES` does not count |
| `css_structured` | Choice sequences, written by `cargo test --release --manifest-path fuzz/Cargo.toml --lib write_the_fuzz_seeds -- --ignored` as `cos_syntax`'s are, each named for what its sheet holds — every combinator, functional pseudo-classes, pseudo-elements, attribute selectors, imports with cycles, `@media`, `@supports` and `@layer` blocks, `@layer` statements and `@font-face`, `!important` and the defaulting keywords, unknown properties, inline styles, and a tree of 24 elements — and `plain`, the empty input |
| `layout` | Each behind **two** control bytes. `ordinary` is a paragraph in a page box the size of a book's. `cjk` is the alphabet's ideographs, small kana and full-width bracket, which is what the vendored UCD is for and what a space-scanner would set as one line. `breaks` puts `page-break-before: always` and `page-break-after: avoid` at one margin with `orphans` and `widows` set, which is where CSS 2.2 section 13.3.3's rules A to D are decided. `nested` is deep enough to reach the box-tree depth cap and nested enough to reach the parent-and-first-child margin collapse. `tight` and `tight-pages` sit behind a control byte of zero: `tight` puts every cap at its tightest -- two boxes deep, four boxes, **no float work at all**, eight break evaluations, one page, and a page one point square -- and `tight-pages` leaves the caps roomy and keeps the one-point page, because with the box cap unreachable the *page* cap and `overflow-wrap`'s last-resort character break are what the input reaches. The seeds runs produced each read a clipped paragraph in order, and each has its reproducer on stable: `nested-clip-order` builds nested `overflow: hidden` boxes over a paragraph of one-character lines, whose inner box's hidden tail is drawn after the outer box's (`a_clip_inside_a_clip_hides_its_text_in_document_order`); `float-clip-order` floats an `overflow: hidden` box of a stated height inside another on twenty-point pages, whose hidden tail is broken over pages with the float it is cut from (`a_hidden_tail_is_broken_over_pages_with_the_float_it_is_in`); and `negative-margin-clip-order` puts a `-14.25px` first-child margin inside an `overflow: hidden` box, taking the column's `y` back up past a page break, and its tail is drawn on the page of the line it follows (`a_hidden_tail_is_drawn_on_the_page_of_the_line_it_follows`) |

Replaying a corpus without mutating it is the cheapest check that a seed
still reaches what it was chosen for:

```sh
IFS=$'\t' read -r _ rss malloc stack < <(grep -P '^cff\t' fuzz/limits.tsv)
( ulimit -s "$stack" &&
  fuzz/target/x86_64-unknown-linux-gnu/release/cff fuzz/corpus/cff \
    -rss_limit_mb="$rss" -malloc_limit_mb="$malloc" -runs=0 )
```

under the same limits as a session.

Two things a run does to `corpus/` that are easy to be surprised by: it
writes every new unit it finds straight into the directory it was given, and
`cargo fuzz cmin` rewrites that directory in place, renaming every file to
its hash. Neither is reversible and both discard the names above, so minimise
a *copy* unless you mean to lose them. `git status` after a run is worth a
look before `git add`.

Large third-party corpora are fetched by pinned checksum and never committed —
see [`corpus/README.md`](../corpus/README.md) for the per-corpus licence table.

## Crashes

`cargo-fuzz` writes a failing input to `artifacts/<target>/`. It does not stay
there: minimise it with `cargo +nightly fuzz tmin <target> <artifact>`, then
land the minimised bytes as an ordinary test in the crate that owns the
parser, in the style `hostile_input.rs` uses, with a comment naming what it
triggered. The value is not the fuzzer finding it once; it is the reproducer
running on every `cargo test` afterwards, on stable, where it cannot be
ignored.
