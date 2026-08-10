# Fuzzing

Eleven `cargo-fuzz` targets, one per leaf format plus two whole-pipeline ones.
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
| `truetype` | Table directory, `cmap`, and `glyf` including composites |
| `cff` | Type 2 charstrings: subrs, `seac`, `flex`, `hintmask` |
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

Minimized corpora are committed under `corpus/<target>/` as they accumulate.
Large third-party corpora are fetched by pinned checksum and never committed —
see the plan for the per-corpus licence table.
