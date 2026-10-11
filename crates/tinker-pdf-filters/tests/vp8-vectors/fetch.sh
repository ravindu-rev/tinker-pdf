#!/bin/sh
# Fetches the WebM project's VP8 test vectors, which cannot be committed.
#
# `webmproject/vp8-test-vectors` is 61 IVF files and, beside each, the MD5 of
# every frame the reference decoder makes of it -- the published answers a VP8
# decoder is held to. The repository carries **no licence file and no licence
# statement**, and material with no stated terms is not redistributable, so
# nothing of it lives in this tree. It is fetched on demand, into a directory
# this script refuses to place anywhere in the working tree but `target/`, read
# by `src/webp/vp8/tests.rs`, and deleted with `target/`.
#
#   sh crates/tinker-pdf-filters/tests/vp8-vectors/fetch.sh [destination]
#
# The destination defaults to `target/vp8-test-vectors`, which `.gitignore`
# covers.
#
# Pinned twice, so that two runs a year apart read the same answers:
#
# - to commit 8afcf0579a9d5221ff892dd197af21ac3d56962d (6 June 2013), the
#   repository's last, fetched by its hash rather than by a branch name; and
# - to the SHA-256 of every file the test reads, in `SHA256SUMS` beside this
#   script, recorded from that commit on 2 October 2026. A file that differs is
#   a failure and not a warning: the MD5s are the expected answers, and an
#   expected answer that moved is not one.
#
# The test prints `vp8-test-vectors: RAN` or `vp8-test-vectors: SKIPPED`, and
# CI's `vp8-vectors` job greps for both, because a skip exits 0 and reads
# exactly like a pass (CONTRIBUTING's RAN / SKIPPED rule).

set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../../.." && pwd)
dest=${1:-${TINKER_VP8_VECTORS:-$repo/target/vp8-test-vectors}}
commit=8afcf0579a9d5221ff892dd197af21ac3d56962d
url=https://github.com/webmproject/vp8-test-vectors

# A destination inside the working tree but outside `target/` would put files
# with no licence under version control the next time anybody typed
# `git add -A`. `target/` is the only writable place because `.gitignore`'s
# first line is `/target`.
case $dest in
    "$repo"/target/*) ;;
    "$repo" | "$repo"/*)
        echo "refusing to fetch into $dest: inside the repository and not under target/" >&2
        exit 2
        ;;
    *) ;;
esac

have=$(git -C "$dest" rev-parse HEAD 2>/dev/null || true)
if [ "$have" = "$commit" ]; then
    echo "vp8-test-vectors: have $commit in $dest"
else
    # Never deletes anything: a directory that holds something else is the
    # caller's to clear.
    if [ -e "$dest" ] && [ -n "$(ls -A "$dest" 2>/dev/null)" ]; then
        echo "refusing to fetch into $dest: it exists, is not empty and is not the pinned checkout" >&2
        exit 2
    fi
    mkdir -p "$dest"
    git -C "$dest" init -q
    echo "vp8-test-vectors: fetching $commit into $dest"
    # Three attempts a minute apart: a red build whose cause was somebody
    # else's outage is noise, and a vector set that is genuinely unreachable
    # still fails the fetch, which is the outcome the job wants.
    tries=0
    until git -C "$dest" fetch -q --depth 1 "$url" "$commit"; do
        tries=$((tries + 1))
        if [ "$tries" -ge 3 ]; then
            echo "vp8-test-vectors: could not fetch $commit from $url" >&2
            exit 1
        fi
        sleep 60
    done
    git -C "$dest" -c advice.detachedHead=false checkout -q FETCH_HEAD
fi

# The second pin. `--strict` fails on a malformed line as well as a mismatch,
# so a damaged manifest cannot pass by checking nothing.
(cd "$dest" && sha256sum --check --quiet --strict "$here/SHA256SUMS")
files=$(wc -l <"$here/SHA256SUMS" | tr -d ' ')
echo "vp8-test-vectors: $files files match SHA256SUMS at $commit"

abs=$(cd "$dest" && pwd)
echo "vp8-test-vectors: set TINKER_VP8_VECTORS=$abs to run the test over them"
echo "vp8-test-vectors: the path must be ABSOLUTE -- a test binary runs from its"
echo "vp8-test-vectors: crate directory, so a relative one resolves under"
echo "vp8-test-vectors: crates/tinker-pdf-filters/."
echo "vp8-test-vectors: set TINKER_VP8_VECTORS_REQUIRED=1 to make a skip a failure."
