# Structure-aware and metamorphic fuzzing

A byte-mutating target hands its input to a parser, and most mutations of a
well-formed input are refused at the first field they break. A session spends
its time at the door, and the code behind the refusals — an object stream
inside a cross-reference stream, a charstring two subroutine calls deep, a
composite glyph three levels down — is reached only as far as a mutation
happens to leave everything before it well formed. A **structure-aware**
target reads its input as a sequence of choices and builds a well-formed input
out of them, so a mutation changes a decision rather than a byte and lands past
the refusals; and because it knows what it built, it can assert more than that
nothing panicked: that the reader read back what was written. A
**metamorphic** target asserts that two computations which must agree do: a
page against its rewrite, a tile against its page, a native build against a
wasm one.

## Scope

- One choice reader, `tinker_pdf_fuzz::Choices` in `fuzz/src/lib.rs`, that
  every generator below reads its input through.
- Eight generators and the targets they drive, each with the property it
  asserts beyond not panicking: COS syntax (`cos_syntax`), content streams
  (`content_stream`), Type 2 charstrings (`cff_charstrings`), TrueType outlines
  (`glyf_outlines`), JPX codestreams (`jpx_codestream`), XML and HTML
  (`xml_structured`, `html_structured`) and CSS (`css_structured`).
- Three metamorphic relations, with what each compares and which inputs it is
  asked of: render against rewrite (`render_rewrite`), tile against page
  (`render_tile`), and native against wasm (`wasm_parity.rs`, a replay test).
- Each new target's seeds, its rows in the fuzz audit and in
  `fuzz/README.md`, and where its coverage figure comes from and is kept.

## Non-goals

- **Retiring a byte-mutating target.** `cos_document`, `cos_object`,
  `content_tokenizer`, `cff`, `cff_subset`, `truetype`, `sfnt`, `jpx`, `xml`,
  `html` and `css` keep their corpora and their job, which is the refusals:
  what a damaged input does to a parser. A generator writes what a format
  allows, so the refusal paths stay theirs.
- **Moving the existing generators onto the shared reader.** `layout`,
  `crypt`, `form_script` and `signatures` carve their bytes with readers of
  their own, and every committed seed of theirs is a sequence of choices in
  that reader's order. Moving them would read each seed as another input.
- **The object writer's round trip.** `tinker_pdf_cos::write_object` writes a
  real to six decimal places and an integral real as an integer, which 7.3.3
  lets a reader treat alike, so a structural equality over its output fails on
  conforming bytes. The writer is held where its output is used:
  `render_rewrite` renders what a rewrite wrote.
- **Damage in `cos_syntax` beyond a dishonest `/Length`.** Offsets that lie and
  broken sections or chains are what the leniency ladder repairs, and
  `cos_document`'s mutation reaches them; a repaired file has no model to be
  compared with.
- **A picture of what is right.** Every relation compares the engine with
  itself, and no target compares a render with anything an outside program
  drew (ruling 13). What that leaves unproved is the first risk below.
- **Running the sessions.** K-04.12 runs them, in the end phase.
- **A dependency.** The reader is written here, and the fuzz crate's one
  dependency outside the workspace stays `libfuzzer-sys`.

## Design

### The choice reader

`Choices` wraps a target's input and reads it front to back, one decision at a
time:

| Call | Reads | Gives |
| --- | --- | --- |
| `byte()` | one byte | the byte |
| `flag()` | one byte | its low bit |
| `below(n)` | one byte for `n` up to 256, two up to 65 536, four above | a value in `0..n`, and 0 for `n` of 0 or 1 |
| `range(lo, hi)` | as `below` over the span | a value in `lo..=hi` |
| `pick(items)` | as `below(items.len())` | one of the items |
| `bytes(max)` | a length as `below(max + 1)`, then that many bytes | a run of the input itself |

Three properties make it a reader every generator can share:

- **An input that has run out reads as zeros**, and zero is the smallest choice
  wherever a generator asks one: no further item, the first form, the shortest
  length. So every generator ends on every input, the empty one included, and
  a short input is a small structure.
- **A decision owns its bytes.** A choice of up to 256 options is one byte, so
  a mutation of one byte changes one decision, and the decisions after it read
  the bytes they read before, except where the decision changed is a count or
  a form, which changes what follows it. `bytes` hands a run of the input
  through untouched, so stream data, string contents and code-block bytes are
  mutated as bytes.
- **Nothing it builds is unbounded.** Each generator caps every count and
  depth it asks for, so an iteration's work is bounded whatever the input
  says; the caps are the generator's, and its target's header names them.

`fuzz/src/lib.rs` is the fuzz crate's library: `Choices`, and every
generator, a module each — the COS model and its file writer in
`fuzz/src/cos.rs`, which `content_stream`, `render_rewrite` and `render_tile`
write their documents with, the page generator in `fuzz/src/content.rs`, and
one module for each format's generator, `fuzz/src/cff.rs` the first. A
generator lives there even when one target uses it, because its seed writer
is a unit test beside it and a target is a binary with no test harness of
its own; the target holds the assertions. The library's unit tests run with
`cargo test --manifest-path fuzz/Cargo.toml --lib` on stable, and
`cargo xtask fuzz` compiles them with the targets
(`cargo check --all-targets`).

### Seeds, and the audit

A structure-aware target's seeds are choice sequences, which nobody can read,
so each is written rather than kept by hand. An `#[ignore]`d test beside the
generator, `write_the_fuzz_seeds`, names a feature per seed — an object stream,
an update that frees an object, a stream whose `/Length` lies, a subroutine
INDEX of 1 240 entries — and writes, for each, the first input of a fixed
pseudo-random sequence whose structure has that feature, asserting it does. A
change to what a generator reads first reads every committed seed as another
structure, so such a change runs the writer again in its own commit, and the
seeds keep their features by name.

Each new target is `unsignatured` in the fuzz audit, with its reason in
`xtask/src/fuzzaudit.rs`'s `UNSIGNED`: every input is a structure, so there is
no signature to look for. Each has a corpus under `fuzz/corpus/<target>/`, a
`[[bin]]` in `fuzz/Cargo.toml`, a header with its `# What this target ...`
section, and rows in both of `fuzz/README.md`'s tables.

### COS syntax: `cos_syntax`

The generator builds a model file and writes it:

- **Sections.** One to four (7.5.6): the original and up to three incremental
  updates. Each is a cross-reference table (7.5.4) or a cross-reference stream
  (7.5.8), and a stream section may hold objects in an object stream (7.5.7).
  Each section defines new objects, redefines earlier ones under the same
  number with the same or the next generation, and frees others. Each trailer
  carries `/Size`, `/Root` — a catalog over an empty page tree — and `/Prev`
  back to the section before it.
- **Objects.** Every type of 7.3: null, booleans, integers, reals, literal and
  hexadecimal strings, names, arrays, dictionaries, streams and indirect
  references, to an object the file defines, to a freed one and to one it
  never defines (7.3.10's null).
- **Spellings.** Each object is written in a spelling 7.3 allows, chosen token
  by token: each kind of white space 7.2.3 names, comments, and none at all
  where a delimiter separates; integers and reals with a sign, leading zeros,
  and a leading or trailing point (7.3.3); every escape of 7.3.4.2, octal of
  one to three digits, a line continuation, an end-of-line written as CR, LF
  or CRLF, and balanced parentheses unescaped; hexadecimal strings in either
  case, with white space and an odd final digit (7.3.4.3); and names with
  `#xx` for any byte but NUL (7.3.5).
- **Streams.** Data from `bytes`, a `/Length` that is direct or indirect
  (7.3.8.2) and honest or dishonest, and any of 7.3.8.1's endings before
  `endstream`: none, LF, CRLF or CR.

A real is a mantissa of at most fifteen digits and a count of decimal places,
so the value the model holds is the mantissa divided by an exact power of ten:
one rounding, which is the value the lexer's exact path gives
(`crates/tinker-pdf-cos/src/lexer.rs`,
`real_values_are_exact_in_the_range_pdf_uses`).

It asserts:

1. **Every object reads back as written.** Each object, as spelled, read by
   `parse_object_at`, is the model's — every value, every key in its order, a
   string's bytes and its form — raises no warning and stops at the spelling's
   end; and read in its indirect form by `parse_indirect_at`, it names the
   model's reference.
2. **A file written clean opens with no repair.** With every `/Length` honest
   the file opens on `LadderLevel::Trust`, and once every object the model
   defines has been read it has raised no warning: the corpus relations'
   *read cleanly* (`tools/tpdf`'s `cleanly_read`; the generator writes no
   encryption). Every live object reads as the model's and a stream's raw data
   is its data; every freed object reads as null; the document has one
   revision per section, each at the offset its section was written at; and
   the strict validator finds no structure-tier defect.
3. **A dishonest `/Length` is recovered.** The offsets are still honest, so
   the file still opens on `Trust`, and each stream whose `/Length` lies reads
   its data whole, with one `StreamLengthRecovered` naming the declared length
   and the data's, wherever the extent is recoverable at all: its data holds no
   `endstream`, and the ending before the keyword cannot be read as part of the
   data — the one ambiguity of 7.3.8.2, data that ends in CR before a lone LF,
   or ends in an end-of-line with none after it. Every other object reads as
   in 2.

### Content streams: `content_stream`

The generator writes a one-page document with the COS writer, every `/Length`
honest, and generates the page's content:

- **Operators.** Every operator of Annex A, each with operands of the shape it
  takes or of a wrong one — too few, too many, of the wrong type — chosen per
  operator.
- **Nesting.** `q` and `Q`, balanced and not; `BT` and `ET`; marked content —
  `BMC`, `BDC` with an inline or a named property list, `EMC`, `MP` and `DP`;
  and `BX` and `EX`.
- **What the operators name.** Resources generated with the page: standard 14
  fonts, and a Type 3 font whose glyph procedures are generated content opening
  with `d0` or `d1`; form XObjects of generated content with a matrix and a
  box, nested to a depth; image XObjects and inline images of small generated
  samples in the device spaces, an indexed space and as stencil masks;
  graphics states with line, dash, alpha, blend and soft-mask entries; axial
  and radial shadings; tiling and shading patterns; and property lists.
- **Options.** The render's scale, region, pixel format, anti-aliasing and
  annotations, chosen with the page.

Every generated page's content opens with a filled rectangle, so the page
draws something unless its generated content paints over it. It asserts that
the display list replays the page: for the generated options, `DisplayList::render` returns the bitmap
`Page::render` returns — width, height, stride, format, every byte, and the
warnings — which is the display list's contract
(`crates/tinker-pdf/src/display.rs`).

### Type 2 charstrings: `cff_charstrings`

The generator builds a bare CFF (Adobe Technical Note 5176) from generated
INDEXes and DICTs: the header; the Name, Top DICT, String and Global Subr
INDEXes; a charset; the CharStrings INDEX; and a Private DICT with its local
Subrs — or a CID-keyed font, with `ROS`, an FDArray and an FDSelect, and a
Private DICT and local Subrs per Font DICT. Each subroutine INDEX holds a count
either side of a bias edge of Technical Note 5177 — 1 239 or 1 240, 33 899 or
33 900 — or a small one, so a call is biased by 107, 1 131 or 32 768.

Each charstring is drawn from every Type 2 operator: the moves, lines and
curves; the four flex operators; the stem hints, with `hintmask` and
`cntrmask` followed by the mask bytes the stems declared so far call for;
`callsubr` and `callgsubr` whose biased operand names a subroutine the INDEX
holds, and `return`; `endchar`, alone or with the four operands of `seac`
naming StandardEncoding codes the charset holds; a width; and operands in each
of 5177's encodings. The generator keeps every program inside the
interpreter's caps — ten levels of subroutine, its operation budget, and the
outline's segment cap — so an outline is the whole program's.

It asserts that every glyph's outline extracts inside those caps, with every
coordinate finite; that its advance is the width it was written with — the
Private DICT's nominal width and the glyph's width operand, or the default
width where it has none, a glyph that is `endchar` alone included; and that a
subset keeping the glyph (`tinker_pdf_font::subset`, read back by
`Cff::parse`) draws the same outline, segment for segment, and the same
advance. A subset may decline (ruling 2); one that is written must hold.

### TrueType outlines: `glyf_outlines`

The generator builds a minimal `sfnt` — `head` with either
`indexToLocFormat`, `maxp`, `hhea`, `hmtx`, `loca` and `glyf`. A simple glyph
is contours of on- and off-curve points whose flags take every form the `glyf`
table defines: a repeated flag, a short coordinate with its sign, a coordinate
the same as the last, and a long one, with or without instructions. A
composite glyph takes every component form: arguments as words or bytes, as
offsets or as points to match, a scale, an x and y scale or a two-by-two, its
offset scaled or not, instructions, `USE_MY_METRICS`, and components that are
composites themselves, nested inside `glyf::outline`'s depth and component
caps.

It asserts what `cff_charstrings` asserts, through `glyf::outline` and the
`sfnt` subsetter, glyph id for glyph id — the subset keeps every id
(`crates/tinker-pdf-font/src/subset.rs`) — with the advance the one `hmtx`
states, and one more: every glyph draws one closed contour for each contour
it holds, its components' included, so a flag run read one point short or a
coordinate read in the wrong width shows as a contour that ends or starts in
the wrong place.

### JPX codestreams: `jpx_codestream`

The generator, `fuzz/src/jpx.rs`, writes a codestream (T.800 Annex A) whose
markers agree: SIZ's image and tile geometry and components; COD and COC's
progression, layers, component transform, decomposition levels, code-block size
and style, and wavelet, in the main header and in a tile's first tile-part
header; QCD and QCC, each fitting the wavelet and the levels of every
tile-component it serves by A.6's precedence; POC volumes, in the main header
and a tile's, the last covering what the others left; and tile-parts cut at
packet boundaries and interleaved between tiles, `TNsot` stated or not, whose
`Psot` is honest, or zero for the last. Packet headers follow the declared
progression (Annex B) — the inclusion and zero-bit-plane tag trees, pass counts,
`Lblock` and one length per codeword segment, empty packets — in the bit stream,
in PPT or in PPM. Code-block data is the input's own bytes, kept to what an
arithmetic coder can emit, or Annex D's coding of generated coefficients through
an MQ encoder (C.2), D.6's raw bits and D.3's context model of the module's own,
which is how segmentation symbols are written. One field may be broken, on a
choice the generator records: set to a value the standard rules out.

It asserts that a codestream with nothing broken decodes to the generated
width, height and component count, and that one with a field broken decodes
to them too or is refused with a reason naming the field — its marker, T.800's
name for it, or the words for what it is. The MQ encoder's decisions are held
apart, by a unit test that reads them back through the engine's MQ decoder; a
context the encoder forms otherwise than tier-1 then shows as a segmentation
symbol that is not `1010`.

### XML and HTML: `xml_structured` and `html_structured`

`fuzz/src/markup.rs` generates one element tree — names with and without a
prefix, attributes in no namespace, a bound one and `xml`'s, prefix and
default namespace declarations, text with the five predefined entities,
character references and CR LF line ends, CDATA sections, comments and
processing instructions — and writes it two ways, as two targets. Its element
names are HTML's, chosen for the insertion modes they open, so one tree is an
ordinary XML document and a soup the tree builder has work to do on.

- `xml_structured` writes it well formed, or near well formed with one defect
  the generator names — an undeclared prefix, an end tag that closes the wrong
  element, a duplicate attribute, a bare `&` — and may put a document type
  declaration before the root: bare, with an external identifier, or with an
  internal subset. In both `Doctype` modes a well-formed document without one
  reads back as written: every element by its qualified name, its attributes in
  order with their values as §3.3.3 normalises them, the character data with
  references resolved and line ends normalised, every comment and instruction,
  and nothing refused, so the reader's count of start events is the tree's
  element count. `Refuse` refuses a declaration before the root opens;
  `SkipExternalId` refuses an internal subset so and reads past the others.
  A defect is refused as the error that names it.
- `html_structured` writes it as tag soup aimed at the tree builder's
  insertion modes: end tags left out or stray, formatting elements mis-nested
  (the adoption agency), text and elements straight inside a table (foster
  parenting), `template`, `select`, foreign content with CDATA, raw-text
  elements with markup inside, attributes quoted three ways and repeated, and
  references with and without their semicolons, parsed under four bounds the
  input chooses. Every input builds a tree whose parent and child links agree —
  each child names its parent, and each node is reached once — within
  `Limits::max_tokens`, which counts every node the builder makes, and parsing
  is deterministic. `MAX_DOM_NODES` is the cascade's cap, and binds only a tree
  a stylesheet is cascaded over.

### CSS: `css_structured`

The generator writes a stylesheet — rule sets whose selectors use every
combinator and the pseudo-classes of `selectors-4` the matcher implements;
`@media`, `@import` with cycles, `@font-face` and `@layer`; declarations of
known and unknown properties, and `!important` — over a generated tree, and
cascades it as `css` does. It asserts `css`'s strongest assertion, with
well-formed stylesheets: the rule index and brute force match the same rules
for every element, and give the same cascade.

### Render against rewrite: `render_rewrite`

The input is a document the COS and content generators write. The relation is
asked only of one read cleanly — `LadderLevel::Trust`, no warning, not
encrypted — as the corpus relations ask theirs, since a rewrite of a document
the reader repaired compares two repairs rather than two renders. Such a
document is rewritten as those relations rewrite one,
`Document::editor().save(&WriteOptions::default())` (`WriteMode::Rewrite`),
and reopened, and every page is rendered before and after with the same
options. A rewrite that does not reopen fails the run, and so does a page
count, a size, a byte or a warning that differs.

### Tile against page: `render_tile`

The generator's page is rendered whole at a generated scale, and then at
generated regions at that scale (ruling 5). Each region's bitmap must be the
whole render's rectangle there, trimmed by the page, byte for byte.

One shape is not compared: ruling 5's named exception, RC-21's image run,
which H-15's default keeps — two images that overlap outside a tile and abut
inside it are one run in the tile and two on the page. The target leaves out
every page on which the device bounding boxes of two drawn images meet, at any
depth of forms, groups, patterns and annotations: a superset of the class,
decided from the generator's model before anything is rendered, which errs
toward comparing less and never toward passing a mismatch outside the class.
RC-21's fix deletes the predicate in its own commit.

### Native against wasm: `wasm_parity.rs`

libFuzzer instruments one native build and does not run inside
`wasm32-wasip1`, so the relation takes H-136's replay form. K-04.12's sessions
leave corpora for `render_page`, `content_stream` and `render_rewrite`. Each
is minimised with `cargo fuzz cmin` into a copy, and the two structured ones
are written out as the documents their entries generate, since an entry of
theirs is a choice sequence and not a PDF: an `#[ignore]`d test in the fuzz
library runs each entry through the generator its target writes with,
natively.

`crates/tinker-pdf/tests/wasm_parity.rs`'s
`print_the_render_hash_of_every_input` reads the directory an environment
variable names, renders every page of each file at one fixed scale, and prints
a `wasm-parity: RAN` banner with its count, then one line per file in name
order — the name, and the SHA-256 `determinism.rs` fingerprints with
(`tinker_pdf_crypto::sha2::sha256`) over every page's size and bytes. With no
directory it prints `SKIPPED`. It runs natively and on `wasm32-wasip1` under
wasmtime, which hosts the test binary, judges nothing (ruling 13) and reads
only the directory it is granted. The two lists must be equal line for line,
and a line that differs is a failure. Each of the three targets carries a
coverage figure (H-136), so `render_page` runs in K-04.12's sessions beside
the new targets.

### Where a coverage figure comes from

K-04.12 runs each new target, and `render_page`, for 600 seconds on Linux. A
session writes every unit it finds into the first directory it is given, so it
is given a scratch directory first and the committed corpus second:
`cargo fuzz run <target> <scratch> fuzz/corpus/<target> -- -max_total_time=600 -timeout=20`.
A target's figure is the `cov:` and the `ft:` of its session's last status
line, the one libFuzzer marks `DONE`: the edges and the features the session
reached. `-print_final_stats` prints `stat::` lines, which carry neither.

The figures are kept in `fuzz/coverage.tsv`, one row a target — its name,
`cov` and `ft` — with the session's revision and day in the commit that
records them (ruling 16); `docs/verification.md` names the file and quotes no
figure. A figure is the record of one session and never a floor: it is not
comparable across targets, nor across two builds of one.

### What a finding becomes

A crash or a broken property is minimised with `cargo fuzz tmin` and kept
twice: as a seed of its target, and as an ordinary test in the crate that owns
the code, holding the bytes the engine was handed — the document, the font
program, the codestream — because the fuzz crate is outside the workspace and
its generators do not run under `cargo test` (`fuzz/README.md`, *Crashes*).

## Milestones

1. **K-04.2, the reader and COS syntax.** `fuzz/src/lib.rs`, `fuzz/src/cos.rs`
   and `fuzz/fuzz_targets/cos_syntax.rs`, with seeds and rows. Exit:
   `cos_syntax` builds under
   `cargo check --manifest-path fuzz/Cargo.toml --all-targets`,
   `cargo xtask fuzz` lists it with an `UNSIGNED` reason and its seeds, its
   seeds replay clean in a local twenty-second `cargo fuzz run`, and on every
   input an object it writes parses back equal and a file it writes clean opens
   with no repair.
2. **K-04.3, content streams.** `fuzz/src/content.rs` and `content_stream`.
   Exit: it builds, is in `cargo xtask fuzz`'s roster, replays its seeds clean
   in a local twenty-second `cargo fuzz run`, and asserts on every input that
   `DisplayList::render` equals `Page::render` byte for byte.
3. **K-04.4, Type 2 charstrings.** `cff_charstrings`. Exit: it builds, is in
   the roster, and replays its seeds clean in a local twenty-second
   `cargo fuzz run`; on every input every glyph's outline extracts inside the
   outline caps and is the same outline after a subset that keeps it.
4. **K-04.5, TrueType outlines.** `glyf_outlines`. Exit: as K-04.4's.
5. **K-04.6, JPX codestreams.** `jpx_codestream`. Exit: it builds, is in the
   roster, and replays its seeds clean in a local twenty-second
   `cargo fuzz run`; on every input a decoded image has the generated
   dimensions and component count, or a refusal names a field the generator was
   told to break.
6. **K-04.7, XML and HTML.** `xml_structured` and `html_structured`. Exit: both
   build, are in the roster, and replay their seeds clean in a local
   twenty-second `cargo fuzz run`; for every well-formed XML document generated
   the reader's element count equals the generator's, and every HTML input
   builds a tree whose parent and child links agree, inside
   `Limits::max_tokens`.
7. **K-04.8, CSS.** `css_structured`. Exit: it builds, is in the roster, and
   replays its seeds clean in a local twenty-second `cargo fuzz run`, asserting
   on every input that indexed and brute-force matching give the same cascade.
8. **K-04.9, render against rewrite.** `render_rewrite`. Exit: it builds, is in
   the roster, and replays its seeds clean in a local twenty-second
   `cargo fuzz run`; every page of every clean input renders identically before
   and after `save`.
9. **K-04.10, tile against page.** `render_tile`. Exit: it builds, is in the
   roster, and replays its seeds clean in a local twenty-second
   `cargo fuzz run`; every region it asks for equals the crop of the whole
   render.
10. **K-04.11a, native against wasm.** `crates/tinker-pdf/tests/wasm_parity.rs`
    and the fuzz library's document writer. Exit:
    `print_the_render_hash_of_every_input` prints `wasm-parity: RAN` with a
    non-zero count natively and on `wasm32-wasip1`, or `SKIPPED` with no
    directory.
11. **K-04.11b, the corpus half.** Exit: a local run over the three minimised
    corpora K-04.12's sessions leave prints the same two lists natively and on
    `wasm32-wasip1`.
12. **K-04.12, the sessions.** Exit: every new target and `render_page` run
    600 seconds on Linux and find nothing, K-04.11b's two lists agree, and
    `fuzz/coverage.tsv` carries each target's `cov` and `ft`.

## Risks

- **A shared misreading passes.** A generator written here and a reader written
  here can share a wrong reading of a clause, and a round trip through two
  pieces of this repository cannot say which reading is the standard's. Each
  generator spells from the clause it cites rather than from the reader's
  code; where it agrees with the reader by construction, its header says so;
  and the corpora and the conformance suites stay the evidence about the
  standard (`docs/verification.md`).
- **A generator reaches only what it writes.** A well-formed input exercises
  the paths a well-formed input takes. The byte-mutating targets keep the
  refusals, and each generator's wrong shapes — a dishonest `/Length`, wrong
  operand lists, a broken JPX field, near-well-formed XML — are chosen so that
  the paths just past a refusal are reached too.
- **The read order is the seeds' meaning.** A generator that reads its
  choices in another order reads every committed seed as another structure,
  silently. The seed writer names each seed's feature and asserts it, so
  running it again after such a change restores the features rather than the
  bytes.
- **Throughput.** A structured iteration costs more than a byte-mutated one:
  `content_stream` renders a page twice, and `render_rewrite` renders, saves,
  reopens and renders again. The caps keep an iteration small, and a session's
  executions go in the commit that records its figure, beside it.
- **A relation that holds trivially.** A page that draws nothing tiles,
  rewrites and replays perfectly. Each generated page's content opens with a
  filled rectangle, so only a page whose own content paints it over is blank.
- **A predicate wider than its class.** `render_tile`'s exclusion leaves out
  pages on which two images' boxes meet without the run deciding anything, so
  it compares less than it could, until RC-21 deletes it.
