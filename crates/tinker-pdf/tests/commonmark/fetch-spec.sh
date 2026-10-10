#!/bin/sh
# Fetches CommonMark 0.31.2's spec.txt, which cannot be committed (tier 5's
# Markdown row).
#
# `README.md` beside this file has the provenance. The short of it: the
# specification is CC-BY-SA 4.0, and `deny.toml`'s "deliberately NO copyleft
# ... not even weak copyleft" rule bars share-alike material from this
# repository, as it bars `epub3-samples` (`tests/epub/fetch-corpus.sh`). So the
# examples the Markdown reader is held to live here instead: fetched on demand,
# at a pinned tag, checked against a pinned SHA-256, into a directory this
# script refuses to place inside the tree.
#
#   sh crates/tinker-pdf/tests/commonmark/fetch-spec.sh [destination]
#
# The destination defaults to `target/commonmark-spec`, which `.gitignore`
# already covers. `crates/tinker-pdf/tests/commonmark_spec.rs` reads
# `TINKER_COMMONMARK_SPEC` -- the ABSOLUTE path of the fetched `spec.txt` --
# and prints `commonmark-spec: RAN` or `commonmark-spec: SKIPPED`.

set -eu

repo=$(cd "$(dirname "$0")/../../../.." && pwd)
dest=${1:-$repo/target/commonmark-spec}

case $dest in
    "$repo"/target/*) ;;
    "$repo" | "$repo"/*)
        echo "refusing to fetch into $dest: inside the repository and not under target/" >&2
        exit 2
        ;;
    *) ;;
esac

tag=0.31.2
want=257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80

mkdir -p "$dest"
cd "$dest"
if [ ! -s spec.txt ]; then
    curl -fsSL --retry 6 --retry-delay 5 --retry-all-errors \
        --retry-max-time 180 --max-time 300 -o spec.txt.part \
        "https://raw.githubusercontent.com/commonmark/commonmark-spec/$tag/spec.txt"
    mv spec.txt.part spec.txt
fi
got=$(sha256sum spec.txt | cut -d' ' -f1)
if [ "$got" != "$want" ]; then
    echo "commonmark-spec: spec.txt is $got, and the $tag tag's is $want" >&2
    rm -f spec.txt
    exit 1
fi

abs=$(pwd)/spec.txt
echo "commonmark-spec: fetched CommonMark $tag's spec.txt to $abs"
echo "commonmark-spec: set TINKER_COMMONMARK_SPEC=$abs to run the examples"
echo "commonmark-spec: set TINKER_COMMONMARK_SPEC_REQUIRED=1 to make a skip a failure"
