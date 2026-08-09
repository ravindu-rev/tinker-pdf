# Fuzzing

One cargo-fuzz target per leaf input format (inflate, lzw, jpeg, ccitt, cff,
truetype, type1, cmap, cos-parser, content-tokenizer). Skeletons land with each
crate's first parser. Policy: `docs/plans/14-testing-and-corpora.md`.
