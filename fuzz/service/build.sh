#!/bin/bash -eu
# The continuous-fuzzing service's build of every fuzz target
# (docs/design/continuous-fuzzing.md). The service runs it in its Rust
# builder image through a one-line build.sh of its own; a local build runs it
# in fuzz/service/Dockerfile's image. Either way it builds the checkout it sits
# in and writes, for each target in fuzz/fuzz_targets/:
#
#   $OUT/<target>                   the binary
#   $OUT/<target>_seed_corpus.zip   every file of fuzz/corpus/<target>/
#   $OUT/<target>.options           the local sessions' -timeout=20
#
# `cargo xtask fuzz --service-out DIR` checks all three.
set -eu

# The checkout is two directories up, wherever this is called from.
cd "$(dirname "$(readlink -f "$0")")/../.."

# The local sessions' mode, which is `cargo fuzz build`'s own: release, with
# debug assertions and overflow checks, under AddressSanitizer. It is called
# as exactly `cargo fuzz build` from the checkout's root because the builder
# image's coverage build recognises that call and no other. RUSTFLAGS is the
# image's, and nothing here adds `-D warnings`.
cargo fuzz build

release=fuzz/target/x86_64-unknown-linux-gnu/release
# The roster is the directory: no list of names, so a new target is built
# with no edit here.
for source in fuzz/fuzz_targets/*.rs; do
    target=$(basename "$source" .rs)
    cp "$release/$target" "$OUT/$target"

    # The seeds, packed from the commit being built and never committed:
    # every file directly in the corpus, dotted names too, with no directory
    # entries and no paths. An empty or missing corpus stops the build.
    corpus="fuzz/corpus/$target"
    archive="$OUT/${target}_seed_corpus.zip"
    if [ ! -d "$corpus" ]; then
        echo "build.sh: $corpus is missing" >&2
        exit 1
    fi
    rm -f "$archive"
    find "$corpus" -maxdepth 1 -type f -print0 | sort -z | xargs -0 zip -q -X -j "$archive"

    printf '[libfuzzer]\ntimeout = 20\n' > "$OUT/$target.options"
done
