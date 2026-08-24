# Examples

Six programs, one per thing this engine is for. Each runs with **no
arguments** — it resolves a committed fixture through `CARGO_MANIFEST_DIR`, so
it works from any directory — and each takes an optional path to run against
something of your own:

```sh
cargo run -p tinker-pdf --example open
cargo run -p tinker-pdf --example open -- some-file.pdf
```

| Example | What it shows |
| --- | --- |
| `open` | opening a document and reading what the reader had to repair |
| `render` | a page to pixels, and the warnings that came with them |
| `extract` | text with its positions, not just its characters |
| `edit` | rotating and cropping a page, saved incrementally |
| `create` | a document built from nothing |
| `convert` | an EPUB, XPS or comic archive read as a document |

They are ordinary cargo examples, so `cargo clippy --workspace --all-targets`
compiles them and CI runs every one of them and checks its output. An example
that compiles and does nothing is the same green tick as one that works, which
is the hazard [verification.md](../../../docs/verification.md) names.
