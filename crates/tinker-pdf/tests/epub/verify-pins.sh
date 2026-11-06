#!/bin/sh
# Holds a fetched EPUB corpus to the books `PINNED.sha256` names, by SHA-256.
#
#   sh crates/tinker-pdf/tests/epub/verify-pins.sh DIR [PINS]
#
# Every book in DIR must be pinned, every pinned book must be in DIR, and
# each must be the bytes its pin names; the first that is not fails the run
# by name. `fetch-corpus.sh` runs it last, as `cargo xtask corpus-fetch`
# holds the PDF corpora to `corpus/corpora.lock`.
#
# The public-domain library regenerates a book when its text is corrected,
# and `epub_fetched.rs`'s figures are read from these bytes, so every book
# is pinned, and re-pinned in a reviewed commit whenever one changes: a
# re-pin re-reads the figures, and the job is red until then.

set -eu

dir=$1
pins=${2:-$(cd "$(dirname "$0")" && pwd)/PINNED.sha256}

[ -f "$pins" ] || { echo "epub-corpus: no pins at $pins" >&2; exit 2; }
[ -d "$dir" ] || { echo "epub-corpus: no corpus at $dir" >&2; exit 2; }

failed=0
checked=0
while read -r pinned name; do
    [ -n "$name" ] || continue
    if [ ! -f "$dir/$name" ]; then
        echo "epub-corpus: $name is pinned and was not fetched" >&2
        failed=1
        continue
    fi
    actual=$(sha256sum "$dir/$name" | cut -d' ' -f1)
    if [ "$actual" != "$pinned" ]; then
        echo "epub-corpus: $name is not the pinned book: sha256 $actual, pinned $pinned;" \
             "re-pin it in a reviewed commit, with the figures it changes" >&2
        failed=1
        continue
    fi
    checked=$((checked + 1))
done <"$pins"

for book in "$dir"/*.epub; do
    [ -e "$book" ] || continue
    name=$(basename "$book")
    if ! awk -v name="$name" '$2 == name { found = 1 } END { exit !found }' "$pins"; then
        echo "epub-corpus: $name was fetched and is not pinned" >&2
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    exit 1
fi
echo "epub-corpus: every book is the pinned one ($checked books)"
