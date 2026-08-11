# Fuzzing

Fifteen `cargo-fuzz` targets, one per leaf format plus two whole-pipeline ones.
Policy: [`docs/plans/14-testing-and-corpora.md`](../docs/plans/14-testing-and-corpora.md).

| Target | What it drives |
| --- | --- |
| `cos_document` | The whole file parser: every rung of the leniency ladder, then the page tree and every stream |
| `cos_object` | The object grammar of 7.3 alone, with no file around it |
| `inflate` | Own inflate, including truncated and corrupt streams |
| `lzw` | LZW with and without early change |
| `jpeg` | Baseline JPEG: Huffman tables, restarts, sampling factors |
| `ccitt` | G3 and G4; the first two input bytes choose the parameters |
| `ascii_filters` | ASCIIHex, ASCII85, RunLength, and the predictors |
| `sfnt` | The table directory and everything around the outlines, including the subsetter that rebuilds one |
| `truetype` | `cmap`, and `glyf` including composites |
| `cff` | Type 2 charstrings: subrs, `seac`, `flex`, `hintmask` |
| `type1` | eexec and charstring decryption, `/CharStrings`, othersubrs |
| `cmap` | CMap syntax as an embedded stream, then splitting a string by the codespaces it declared |
| `crypt` | The standard security handler and the ciphers under it; the input is carved into `/Encrypt`'s fields |
| `content_tokenizer` | The content-stream tokenizer |
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
enough to review: the whole set is 25 KB across 66 files, and the largest
single seed is 2.8 KB.

They are built from material this repository already had, so each one is
something a parser here is known to accept rather than a blob nobody can
account for:

| Target | Seeds come from |
| --- | --- |
| `ascii_filters` | Hand-written hex, ASCII85 and RunLength streams, plus rows shaped for a PNG-Up predictor |
| `ccitt` | `ccitt.rs`'s own `pack` fixtures, behind the two control bytes the target reads first |
| `cff` | `cff.rs`'s `three_glyph_program` test — the same bytes, built by the test so the two cannot drift |
| `content_tokenizer` | Hand-written operator streams: text, paths, an inline image, escapes, comments |
| `cos_document` | The four `testdata/` PDFs and the four pages `determinism.rs` builds |
| `cos_object` | Hand-written 7.3 objects: dictionary, stream, nested array, every number form, two unterminated |
| `inflate` | Our own `zlib_compress` output, including an empty stream and an incompressible one |
| `jpeg` | `jpeg.rs`'s `tiny_gray`, `sequential_block` and both progressive fixtures |
| `lzw` | 7.4.4.2's Table 8 example, truncated, and a clear-code stream |
| `render_page` | The subset of the `cos_document` PDFs that has a page worth rendering |
| `truetype` | `determinism.rs`'s `curvy_font`, plus a truncated copy and its directory alone |
| `sfnt` | The same face, plus the `OTTO` program `glyf.rs` builds and a directory pointing past the end |
| `type1` | `type1.rs`'s `font_with_square`, and a copy cut off inside the eexec section |
| `cmap` | A `/ToUnicode`, a `cidrange` CMap with two codespaces, a `usecmap`, a predefined name, and one malformed throughout |
| `crypt` | `build_r6` output laid out in the target's carve order, so one seed authenticates and decrypts; plus R4, R2, and three bytes |

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
