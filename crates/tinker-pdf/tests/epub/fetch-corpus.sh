#!/bin/sh
# Fetches the EPUB corpus that cannot be committed (gap 31, milestone 1).
#
# `README.md` beside this file has the licence table. The short of it: Project
# Gutenberg's books carry a trademark licence whose clause 1.E.1 requires the
# boilerplate to appear wherever a copy is "accessed, displayed, performed,
# viewed, copied or distributed" and whose 1.E.4 forbids detaching the terms;
# and `epub3-samples` is CC-BY-SA 3.0, which `deny.toml`'s "deliberately NO
# copyleft ... not even weak copyleft" rule bars. Both are excellent corpora and
# neither may live in this repository, so they live here instead: fetched on
# demand, into a directory this script refuses to place inside the tree.
#
#   sh crates/tinker-pdf/tests/epub/fetch-corpus.sh [destination]
#
# The destination defaults to `target/epub-corpus`, which `.gitignore` already
# covers. `crates/tinker-pdf/tests/epub_fetched.rs` reads `TINKER_EPUB_CORPUS`
# and prints `epub-corpus: RAN` or `epub-corpus: SKIPPED`; CI greps for the
# second and goes red, because gap 20 found that a skipped oracle exits 0 and
# reads exactly like a pass. That grep matters more here than for any earlier
# oracle: these files are not in the repository, so the test over them can fail
# to run for a second reason as well as the first.
#
# Nothing here is redistributed. The files are read and deleted with `target/`.

set -eu

repo=$(cd "$(dirname "$0")/../../../.." && pwd)
dest=${1:-${TINKER_EPUB_CORPUS:-$repo/target/epub-corpus}}

# The one rule this script enforces rather than documents. A destination inside
# the working tree but outside `target/` would put unredistributable books under
# version control the next time anybody typed `git add -A`, which is a licence
# violation committed by a convenience. `target/` is the only writable place
# because `.gitignore`'s first line is `/target`.
case $dest in
    "$repo"/target/*) ;;
    "$repo" | "$repo"/*)
        echo "refusing to fetch into $dest: inside the repository and not under target/" >&2
        exit 2
        ;;
    *) ;;
esac

mkdir -p "$dest"
cd "$dest"

get() {
    # $1 destination name, $2 URL. Skips what is already here, so a re-run is
    # cheap and an interrupted run resumes.
    if [ -s "$1" ]; then
        echo "  have  $1"
        return 0
    fi
    echo "  get   $1"
    curl -fsSL --retry 3 --max-time 300 -o "$1.part" "$2"
    mv "$1.part" "$1"
}

echo "epub-corpus: fetching into $dest"

# ---- Project Gutenberg ------------------------------------------------------
#
# The six books this plan's "What is wrong" table was measured on, by the same
# URLs. Gutenberg regenerates its EPUBs when a text is corrected, so a hash is
# not pinned here: `MANIFEST.tsv` records what this run got, and a difference
# between two runs is information rather than a failure.
#
# One `ebookmaker` wrote all six, which is exactly why they are not enough on
# their own -- see README.md's note on why the committed corpus names two
# producers minimum.

echo "Project Gutenberg (public-domain text, trademark licence, never committed):"
get pg84-images.epub        'https://www.gutenberg.org/ebooks/84.epub3.images'
get pg1342-noimages.epub    'https://www.gutenberg.org/ebooks/1342.epub.noimages'
get pg2701-images.epub      'https://www.gutenberg.org/ebooks/2701.epub3.images'
get pg11-alice-images.epub  'https://www.gutenberg.org/ebooks/11.epub3.images'
get pg11-epub2-images.epub  'https://www.gutenberg.org/ebooks/11.epub.images'
get pg16328-beowulf.epub    'https://www.gutenberg.org/ebooks/16328.epub3.images'

# ---- IDPF/W3C epub3-samples -------------------------------------------------
#
# Pinned to the 20230704 release rather than to `master`, so two runs a year
# apart fetch the same bytes. The subset is chosen by what each file is the only
# example of, not by size: the two obfuscated fonts are milestone 9's only real
# input, the vertical and RTL books are the two refusals this plan promises by
# name, and the SVG spine item is a non-goal that has to be recognised before it
# can be refused.

samples='https://github.com/IDPF/epub3-samples/releases/download/20230704'
echo "epub3-samples (CC-BY-SA 3.0 -- barred from this repository by deny.toml):"
for s in \
    hefty-water.epub \
    wasteland.epub \
    wasteland-otf-obf.epub \
    wasteland-woff-obf.epub \
    childrens-literature.epub \
    childrens-media-query.epub \
    trees.epub \
    epub30-spec.epub \
    quiz-bindings.epub \
    regime-anticancer-arabic.epub \
    svg-in-spine.epub \
    internallinks.epub \
    georgia-cfi.epub \
    linear-algebra.epub
do
    get "sample-$s" "$samples/$s"
done

# ---- the manifest -----------------------------------------------------------
#
# Written last, over whatever is actually on disk, so it describes the run
# rather than the script's intentions.

{
    echo "file	bytes	sha256"
    for f in *.epub; do
        [ -e "$f" ] || continue
        printf '%s\t%s\t%s\n' "$f" "$(wc -c <"$f" | tr -d ' ')" \
            "$(sha256sum "$f" | cut -d' ' -f1)"
    done
} >MANIFEST.tsv

echo "epub-corpus: fetched $(ls -1 ./*.epub | wc -l | tr -d ' ') books into $dest"
echo "epub-corpus: set TINKER_EPUB_CORPUS=$dest to run the tests over them"
