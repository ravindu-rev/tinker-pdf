#!/usr/bin/env bash
# One book, or a few, looked at closely.
#
# usage: diag.sh WORKTREE BUILD CASES OUT [lines] [cs]
#   BUILD  default | bundled
#   CASES  a TSV file, one book a line: name<TAB>lang<TAB>style<TAB>body
#          (lines starting with # are skipped)
#   OUT    the output file
#   lines  also print each drawn line's glyphs with their boxes (DBG_LINES)
#   cs     also print the page's text operators (DBG_CS)
#
# Copies probes/diag/zz_fx2dbg.rs into WORKTREE's crates/tinker-pdf/tests/
# (refusing if one is there), runs it, removes exactly that file. Each book
# prints `V <name> c<cons>l<logical>w<whole>s<search> ov=<adjacent>/<visible>
# miss=[words search did not find] plain=<text read> log=<logical text>`.
set -u
case ${1:-} in -h|--help) sed -n '2,/^set -u$/{/^set -u$/!p}' "$0"; exit 0 ;; esac
if [ $# -lt 4 ]; then
  sed -n '2,/^set -u$/{/^set -u$/!p}' "$0" >&2
  exit 2
fi
KIT=$(cd "$(dirname "$0")/.." && pwd)
W=$(cd "$1" && pwd) || exit 2
FEAT=""
[ "$2" = bundled ] && FEAT="--features bundled-fonts"
CASES=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
OUT=$4
shift 4
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-3}
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
unset DBG_LINES DBG_CS
for opt in "$@"; do
  case $opt in
    lines) export DBG_LINES=1 ;;
    cs) export DBG_CS=1 ;;
    *) echo "unknown option $opt" >&2; exit 2 ;;
  esac
done
DST="$W/crates/tinker-pdf/tests/zz_fx2dbg.rs"
if [ -e "$DST" ]; then
  echo "refusing: $DST is already there" >&2
  exit 2
fi
cp "$KIT/probes/diag/zz_fx2dbg.rs" "$DST" || exit 2
trap 'rm -f "$DST"' EXIT INT TERM
# shellcheck disable=SC2086
( cd "$W" && DBG_FILE="$CASES" cargo test -q -p tinker-pdf $FEAT --test zz_fx2dbg -- --nocapture ) > "$OUT" 2>&1
echo "exit $?" >> "$OUT"
grep -E '^(V|LINE|CS)\b|panicked|^error' "$OUT" | cut -c1-400
