# Resource exhaustion

A hostile input can fail a reader without making it panic: it can make it
allocate more than a host has, recurse past the stack of the thread it runs
on, or make it do work that grows faster than the input does. This design
makes each of those a failure something reports. Every fuzz session runs
under memory limits derived from the caps
`crates/tinker-pdf/tests/bounds_ledger.rs` holds and under the stack of the
smallest thread a supported host gives, and every parser family has a
size-scaling test in `cargo test` that counts its work over one input shape
at two sizes and holds the ratio to a law. Nothing is timed: the per-input
time limit of a session stays the fuzz command's `-timeout`.

## Scope

- **A fuzz session's memory limits**: each target's `-rss_limit_mb` and
  `-malloc_limit_mb`, derived from the bounds its path reaches — the ledger's
  caps at their shipped values, the caps it reaches that the ledger has no row
  for, read where they are declared, the ceilings the target passes in a cap's
  place, and the input.
- **What a session's resident set is measured against**, and which growth is
  a failure.
- **The stack budget** a session runs under.
- **`fuzz/limits.tsv`**, the one table the local fuzz command and
  `cargo xtask fuzz` both read, and the test that holds it to the derivation.
- **The size-scaling law** each parser family is held to: COS, the content
  interpreter, the fonts, each codec of `tinker-pdf-filters`, XML, HTML and
  CSS, and the archives — the shapes and sizes each scales over, its ratio
  bound, the counter it reads and the file its test is in.

## Non-goals

- **A clock.** A size-scaling test compares counts, never times, as MS-01's
  guards do (`docs/design/performance.md`, "Non-goals"); `bounds_ledger.rs`
  bans `Instant::now` in every test its ledger names, and each complexity test
  reads no clock either.
- **A limit raised to pass.** A limit moves only when a bound it is derived
  from moves. `fuzz/limits.tsv` is written from the derivation and never by
  hand, and a session that fails under its limits is a finding (below).
- **Peak memory in `cargo test`.** An operation's peak resident set is
  MS-05's, read from `/proc` by its own probe. These limits are a fuzz
  session's.
- **A counter that refuses.** A count only adds; refusing is what the caps and
  the budgets do. A counted door returns what its uncounted sibling returns.
- **A count in a global, a thread-local or a document.** Every count is
  returned by the door that was asked, as MS-01's tally is
  (`docs/design/performance.md`, "The tally, and the value a door hands
  out").
- **The input length a session generates.** The command passes no
  `-max_len`; libFuzzer chooses it from the corpus and never goes past 1 MiB,
  which is what the derivation takes for the input.
- **Coverage.** A session's `cov` and `ft` figures are K-04.12's.

## Design

### What a session is measured against

libFuzzer has two memory checks, and a session uses both.

**`-malloc_limit_mb` is per allocation.** The sanitiser's allocation hook sees
every allocation's size, and an allocation of the limit or more fails the input
that asked for it, with the stack that asked and that input saved. It is the
per-input check, and what it finds reproduces from the saved input alone.

**`-rss_limit_mb` is per session.** A thread reads the process's peak resident
set once a second, and the peak never falls. The resident set holds the live
heap and more besides: the quarantine of freed blocks the sanitiser keeps, the
shadow it maps beside every byte it touches, free pages the allocator has not
returned yet, libFuzzer's own corpus and tables, and the binary. When the check
fails, libFuzzer saves the input that happened to be running, which need not be
one that held anything: a session can climb on retained pages and a full
quarantine while its live heap stays small, and then save an input, the empty
one included, that reproduces nothing.

So:

- **One input's peak is the failure, and growth across a session is not.** An
  input is answerable for what it held. The held total is the live heap one
  input may reach under the bounds its path has, and the resident limit is that
  total plus the session's own share.
- **The session's share is pinned, so that the resident set tracks the
  inputs.** The command sets
  `ASAN_OPTIONS=quarantine_size_mb=64:allocator_release_to_os_interval_ms=500`:
  a quarantine of a known size in place of the sanitiser's default, and freed
  pages returned to the system within half a second, inside the
  once-a-second check, so the pages one large input freed are not still
  resident under the next. The shadow is an eighth of the heap it covers, and
  the base is the process before its first input.
- **A resident failure is reproduced before it is fixed.** The saved input is
  run alone under the same limits. One that fails alone is that input's
  finding. One that does not is replayed after the inputs the session added
  before it, in order, until one fails; a failure no input reproduces is the
  session's share outgrowing what this design pins, which is a finding against
  this design, settled here and never in the table.

### The derivation

A target's path holds **buffers**, and each buffer has a **bound**:

- **a ledger row**, at its shipped value: `MAX_DECODED_STREAM`,
  `MAX_PAGE_PIXELS`, `MAX_ZIP_ENTRY_BYTES`;
- **a cap the ledger has no row for**, read from the constant that declares
  it, as `tinker_pdf_archive::sevenz::MAX_7Z_UNPACKED` is;
- **a ceiling the target passes** in a cap's place, `Limits::new(1 << 20)` in
  `inflate`, written with the text that passes it, which the derivation finds
  in the target's source;
- **a generator's caps**, for a structured target that writes its file from
  its input's choices: the size the caps keep the generated file under, which
  the row says is an estimate, standing on the texts of the caps it is
  worked from, which the derivation finds in the generator's source;
- **the input**, 1 MiB: libFuzzer makes no input past that while the command
  names no `-max_len`.

A buffer is its bound, times the bytes one unit of the bound takes in it — a
decoded stream one byte a byte, a page canvas four bytes a pixel, a JPEG's
coefficients two bytes for every sample of its output ceiling — times the
copies of it the path holds at once. With `MiB` for 2²⁰ bytes:

| Figure | Is | Because |
| --- | --- | --- |
| largest | the largest buffer, the input included | — |
| held | every buffer at its bound, times its copies, summed | one input may fill every bound on its path at once |
| `malloc_limit_mb` | ⌊2 × largest ÷ MiB⌋ + 1 | a buffer grown by doubling asks for up to twice what it holds in one allocation, and libFuzzer fails an allocation of the limit or more |
| `rss_limit_mb` | 256 + 64 + ⌈9 × 2 × held ÷ (8 × MiB)⌉ | the base, the pinned quarantine, and every buffer at the capacity its last doubling gave it, with the sanitiser's eighth of shadow beside it |
| `stack_kib` | 512 | the stack budget, below |

The base, 256 MiB, is the fuzzer process before its first input: the binary,
libFuzzer's corpus and tables and the sanitiser's own bookkeeping. libFuzzer's
`INITED` line reports it for every session, so a session's log shows the
figure this one stands for.

`bounds_ledger.rs` holds the derivation: one row a fuzz target, naming its
buffers, each with its bound, the bytes a unit of it takes and its copies, and
why. Every bound is read from the code at test time, so a cap that moves moves
the limits of the targets that reach it, and a target ceiling whose text has
left the target's source fails the test that reads it.

**A buffer no row names** is found by a session that fails. When a bound the
path already has admits it, the row is short: it names the buffer and that
bound. When none does, the parser is unbounded, and is bounded. Either way the
table follows the code.

### The stack budget

**512 KiB: the smallest stack a supported host gives a thread.** Ruling 4's
hosts are Linux, Windows, macOS and the wasm targets, and the engine runs on
whatever thread its host calls it from. Windows gives a thread the 1 MiB its
image reserves unless asked otherwise; the `wasm32` targets link a 1 MiB
stack; Linux sizes a thread by the process's stack limit, 8 MiB unless lowered;
and macOS gives a thread created without a size 512 KiB, the stack a worker of
a dispatch queue has, which is the least of the four.

A session runs its inputs on the fuzzer's main thread, so the budget is that
thread's: `ulimit -s 512` in the shell that starts the target's binary on
Linux, and `-C link-arg=/STACK:524288` in `RUSTFLAGS` when the targets are
built on Windows with MSVC, where the explicit target triple keeps the flag
off the build scripts. A sanitised frame is larger than a release one, so a
session is stricter than any host: an input that fits the budget under the
sanitiser fits a host's 512 KiB. A recursion that does not fit is a finding,
fixed by an explicit stack on the heap or a depth bound, never by a larger
budget.

### The table and its readers

`fuzz/limits.tsv` is a header line and then one row a target, sorted by name,
its fields separated by tabs: `target`, `rss_limit_mb`, `malloc_limit_mb`,
`stack_kib`.

- **The local fuzz command** (`docs/verification.md`) builds the targets, then
  starts each target's binary under its row's stack budget with its row's two
  limits, for the seed replay (`-runs=0`) and the mutating session alike.
- **`cargo xtask fuzz`** requires exactly one well-formed row for every target
  in the roster and none for a name that is not a target, and prints each
  target's limits beside its seeds; it runs inside `cargo xtask check`, so the
  targets, the corpora, `fuzz/Cargo.toml` and the table cannot drift apart
  with the gates green.
- **`bounds_ledger.rs`** holds the table to the derivation:
  `the_fuzz_limits_are_derived_from_this_ledger` reads the committed table
  with `include_str!`, derives every row from the live bounds, fails on any
  difference and prints the table it expects, and the ignored
  `write_the_fuzz_limits` writes it.

### The size-scaling law

A **shape** is an input a test generates at a size n: n objects, n operators,
n rows. A test builds its shape at n and at k × n, runs the parser over both
through a counted door, and holds

    work(k × n) ≤ 1.1 × k × L × work(n)

with **k = 4**. For a shape whose parser reads each element a bounded number of
times, L = 1, so the bound is 4.4 — MS-01's A2 bound for four times the pages
(`docs/design/performance.md`, "The guard suite"). For a shape whose parser
bisects or orders its elements, L = log₂(k × n) ÷ log₂ n. A quadratic shape
costs 16 times at k = 4 and an n¹·⁵ one 8, so either fails at every n the
tests use; a constant cost only lowers the ratio, and the families' table says
which L each shape has.

n is chosen so that k × n is inside every cap on the shape's path, far enough
below each that nothing is stopped: a shape that reaches a cap measures the
cap, not the parser.

**`work` is the sum of a family's counters.** A counter counts steps of one
named kind where they are taken — a token lexed, an operator run, a symbol
decoded, a sample written, a record or an entry read, a table entry built, an
element a search examines — and is placed where an input can make the work
repeat, not at the door: a count at the door counts the input, which is
linear by construction. A count sees only what it counts, as MS-01's A2 does,
so each family's counted sites are listed with it; a loop that counts nothing
passes, and that is a defect of the counter, fixed by counting it.

### The counters

Two homes, as the crate graph decides (`docs/design/performance.md`, "Home"):

- **COS and the content interpreter count into MS-01's tally.** cos's `Stats`
  gains one counter for each kind of parse step, counted by the parse path's
  own context — the `WarningSink` every parse is handed, which already carries
  what a parse tolerated — and handed out by two counted doors,
  `CosDocument::open_counted(bytes, &Work)` and
  `CosDocument::get_counted(r, &Work)`. The interpreter depends on cos and
  counts its tokens and operators into the same `Stats` through
  `interpret_counted`, keeping them as plain integers and adding them to the
  `Work` once a run, so no lock is taken an operator.
- **A leaf returns its count as a value**, in its own vocabulary, as
  `Jbig2SymbolExtent::work` already does: a counted sibling of each door —
  `flate_decode_counted`, `Sfnt::parse_counted` — returns its uncounted
  sibling's answer and the steps it took, kept in the decoder's own state as a
  plain integer. A leaf gains no `Work` and no edge (ruling 8).

### The families

| Family | Parsers | Shapes, at n and 4 n | L | Counter, one unit is | Test |
| --- | --- | --- | --- | --- | --- |
| COS | the lexer, the object parser, cross-reference tables and streams, the `/Prev` walk, object streams, the repair scan | n objects under one classic table; an array nested n deep; n incremental sections chained by `/Prev`; n objects in one object stream; n objects in a file whose `startxref` names nothing, which the repair scan finds | 1 | `Stats::tokens_lexed`, a token the lexer produced; `xref_entries_read`, an entry read from a table or a stream's rows; `objects_parsed`, an indirect object parsed; `bytes_scanned`, a byte the repair scan's searches examined | `crates/tinker-pdf-cos/tests/complexity.rs` |
| Content | the content tokenizer and the interpreter | n operators; one operator after a run of n operands, inside the operand stack's 512; n nested `q` and as many `Q`; n nested `BDC` and as many `EMC`; n inline images; a form drawn n times, and forms nested n deep inside the form depth | 1 | `Stats::content_tokens_read`, a token the interpreter took from the tokenizer, nested content included; `operators_run`, an operator it ran | `crates/tinker-pdf-content/tests/complexity.rs` |
| Fonts | `Sfnt::parse`, the `cmap` subtables, `glyf` outlines, `Cff::parse` and its charstrings, `Type1::parse` and its charstrings, `woff::decode`, `cmap::parse_embedded` | a directory of n tables; a format 4 subtable of n segments and a format 12 of n groups, each segment's first code mapped; a simple glyph of n points; a composite of n components; a CFF of n glyphs, each outlined; a charstring of n operators; n subroutine calls; an `eexec` section of n bytes; a Type 1 font of n charstrings, each outlined; WOFF and WOFF2 files of n tables; an embedded CMap of n `bfchar` mappings and one of n `cidrange` ranges, each code split and mapped | 1, and the log for a `cmap` subtable's bisection | the steps a parse reads: a table record, a segment or group examined, a point or a component decoded, an INDEX entry, a charstring operator run, a byte decrypted or decompressed, a CMap entry read and a code split | `crates/tinker-pdf-font/tests/complexity.rs`, in both feature builds |
| Codecs | inflate, LZW, ASCII85, ASCIIHex, RunLength, the PNG and TIFF predictors, brotli, CCITT, JPEG baseline, progressive and arithmetic, JPX, JBIG2, PNG, TIFF, GIF, BMP, WebP and JPEG XR | for each codec, n units of its own: blocks or bytes for the byte filters and rows for the predictors; rows of a G3 and a G4 page; MCUs of a baseline and a progressive frame, and restart intervals of an arithmetic one, each the same MCU; tiles of a JPX codestream; rows of a JBIG2 generic region; rows and chunks of a PNG; strips of a TIFF; rows of a GIF and of a BMP; pixels of a lossless WebP and macroblocks of a lossy one; macroblocks of a JPEG XR picture | 1 | the steps a decode takes: a symbol or a code decoded, a byte or a sample written, a row unfiltered, a table entry built; JBIG2 reads its decode's own `work` | `crates/tinker-pdf-filters/tests/complexity.rs` |
| Markup | the XML reader; the HTML tree builder; CSS tokenising, parsing, selector matching and the cascade | XML nested n deep, an element of n attributes, n namespace declarations, n character references; HTML of n formatting elements reopened, n cells fostered out of a table, n nested tables; a stylesheet of n rules, a selector of n compounds, n elements matched against n rules' worth of selectors, a cascade over n declarations | 1 | the XML reader's events and the bytes it scanned; the tree builder's own step count, made an always-on count a counted entry point returns and the one count `assert_linear` reads too; the CSS `Budget`'s tokens, rules, declarations and matches | `crates/tinker-pdf-xml/tests/complexity.rs`, `crates/tinker-pdf-css/tests/complexity.rs` |
| Archives | ZIP by its central directory and by its local-header scan; tar; 7z and its coders, LZMA, BCJ, BCJ2, PPMd, bzip2 and zstd; RAR | n entries listed by each route; n tar headers; n 7z entries and n bytes through each coder; n RAR headers | 1 | the steps a listing or a decode takes: a header or a record read, a byte the scan examined, a symbol decoded, a byte written | `crates/tinker-pdf-zip/tests/complexity.rs`, `crates/tinker-pdf-archive/tests/complexity.rs` |

A shape is generated by the test, from the crate's own encoders where it has
one — `deflate`, `jpeg_encode`, `png_encode`, `ccitt_g4_encode`,
`jbig2_generic_encode` — and by hand from the format's own clauses where it
has none. Two shapes need a coder only the crate's tests hold: the arithmetic
JPEG's restart interval is coded once by `qm.rs`'s test-only QM encoder, a
flat block's two decisions in fresh contexts, committed beside the test and
held to that encoder's output by a unit test so it cannot drift, and repeated
n and 4 n times between restart markers; and JPEG XR, which this repository
cannot encode, is read from the committed lossless pictures whose areas are
four times one another, counting its macroblocks and samples, which a
picture's content does not change.

## Milestones

1. **K-05.2, the limits.** `bounds_ledger.rs`
   `the_fuzz_limits_are_derived_from_this_ledger` fails when a row of
   `fuzz/limits.tsv` differs from the derivation or a bound moves without it;
   `fuzzaudit.rs`'s `a_target_without_a_limit_row_fails_the_audit` passes;
   `cargo xtask check` is green.
2. **K-05.3, the sessions.** The local fuzz command passes each target's two
   limits from `fuzz/limits.tsv` and runs it under the stack budget, the seed
   replay under the same limits; a local session of every target finds
   nothing, each session's log showing its limits.
3. **K-05.4, COS and content.** `complexity.rs` in `tinker-pdf-cos` and in
   `tinker-pdf-content`, in `cargo test`: for every shape, work at 4 n is at
   most the law's bound times work at n.
4. **K-05.5, the fonts.** `complexity.rs` in `tinker-pdf-font`, in
   `cargo test` with and without default features, holds every shape to the
   law.
5. **K-05.6, the codecs.** `complexity.rs` in `tinker-pdf-filters`, in
   `cargo test`: one test a codec, holding each of its shapes to the law.
6. **K-05.7, markup.** `complexity.rs` in `tinker-pdf-xml` and in
   `tinker-pdf-css`, in `cargo test`, hold every shape to the law, and the
   HTML tree builder's `assert_linear` tests pass unchanged.
7. **K-05.8, archives.** `complexity.rs` in `tinker-pdf-zip` and in
   `tinker-pdf-archive`, in `cargo test`, hold every shape to the law.

## Risks

| Risk | Mitigation |
| --- | --- |
| A row names too little, and a session fails on a buffer a bound already admits | The row names the buffer and that bound, and the table is written again from the derivation; the limit follows the code and is never edited |
| A row names too much, and a limit admits an allocation no input can make | A buffer is named only with a bound on the path, and a bound's unit is read off the code the buffer is in; `held` sums every buffer at once, which overstates a path that frees one before it fills the next, the safe direction for a limit that must not fail a clean input |
| A derived resident limit exceeds the session machine's memory | The host refuses the session before libFuzzer does; the caps that admit it are the ledger's to narrow, and the session runs on a machine that holds the limit |
| The session's share grows past what is pinned: retained pages, the sanitiser's bookkeeping | The quarantine is pinned and freed pages are returned within the check's interval; a resident failure no input reproduces is a finding against this design |
| The sanitised build's frames overflow 512 KiB where a host's would not | The direction is safe; a recursion is moved onto the heap or bounded, so the fix holds on every host |
| A complexity test passes because its counter misses the loop that repeats | Each family's counted sites are listed in its counter's documentation, and a counter is placed where an input makes work repeat rather than at the door |
| A shape reaches a cap and measures the cap | n is chosen so that 4 n is far inside every cap on the path, and each test asserts its shape decoded whole at both sizes |
| A counted door drifts from the uncounted one | The uncounted door is the counted one with its count dropped, and each complexity test compares the two answers at both sizes |
| A count differs by target | Counts are integers added in program order, read on the caller's thread, with no hash order and no clock |
