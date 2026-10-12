#!/usr/bin/env bash
# The right-to-left probe kit's runner.
#
# usage: run.sh WORKTREE BUILD OUTDIR [FILTER]
#   WORKTREE  a tinker-pdf checkout at the commit to probe (this one, or a
#             `git worktree add` of another)
#   BUILD     default | bundled | both
#   OUTDIR    where the outputs go; created if missing
#   FILTER    an extended regular expression matched against "group/family"
#             (families.tsv), e.g. 'hunt/', 'zz_rr3m_h1min$', '^lines/'.
#             Default: every family.
#
# For each family, one at a time: copy its .rs into WORKTREE's
# crates/<crate>/tests/ (refusing if a file of that name is there), run
# `cargo test -p <crate> [--features bundled-fonts] --test <family> --
# --nocapture`, remove exactly the file it copied, normalise the output.
# Line probes (tinker-pdf-shape) have no bundled-fonts build and run once,
# in the default pass.
#
# Writes:
#   OUTDIR/tree.txt                    WORKTREE's HEAD and status, before and after
#   OUTDIR/progress                    one line per family as it finishes
#   OUTDIR/<build>/raw/<family>.txt    cargo's output, then "exit N"
#   OUTDIR/<build>/<family>.txt        the normalised output (norm.py)
#   OUTDIR/lines/<name>.tsv            the line probes' TSVs, as they write them
#   OUTDIR/<build>/<family>.tsv        the td probe's TSV ($ZZ_OUT), as it writes it
#   OUTDIR/<build>/<family>.exit       cargo's exit status
#   OUTDIR/<build>/done                written last: families run, and those whose exit was not 0
#   OUTDIR/done                        written when every requested build is done
# where <build> is default, bundled or lines. A missing done marker means the
# run did not finish, and its outputs are not a measurement.
#
# Environment: CARGO_TARGET_DIR (default WORKTREE/target); CARGO_BUILD_JOBS
# (default 3). KEEP_BINARIES=1 keeps each probe's test binary (by default it
# is deleted after its run, so that two builds fit on a small disk).
# RUN_GIT=0 skips recording HEAD and status, for a tree that is not a git
# checkout (an exported archive).
set -u

if [ $# -lt 3 ]; then
  sed -n '2,/^set -u$/{/^set -u$/!p}' "$0" >&2
  exit 2
fi
KIT=$(cd "$(dirname "$0")/.." && pwd)
W=$(cd "$1" && pwd) || exit 2
BUILD=$2
mkdir -p "$3" || exit 2
O=$(cd "$3" && pwd)
FILTER=${4:-}
case $BUILD in
  default) BUILDS="default" ;;
  bundled) BUILDS="bundled" ;;
  both) BUILDS="default bundled" ;;
  *) echo "BUILD must be default, bundled or both" >&2; exit 2 ;;
esac
if [ ! -d "$W/crates/tinker-pdf/tests/epub_support" ]; then
  echo "$W does not look like a tinker-pdf checkout (no crates/tinker-pdf/tests/epub_support)" >&2
  exit 2
fi

export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-3}
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
TGT=${CARGO_TARGET_DIR:-$W/target}
export CARGO_TARGET_DIR=$TGT
# The probes' own knobs narrow or change what they print; a kit run never has them.
unset RV_FILTER RV_FULL RV3_FILTER FX2_TRACE DBG_FILE DBG_LINES DBG_CS DIAG_LANG DIAG_STYLE DIAG_BODY \
  DIAG_STREAM DIAG_FILE NAMED_FILE

COPIED=()
cleanup() {
  for f in "${COPIED[@]:-}"; do
    [ -n "$f" ] && rm -f "$f"
  done
  COPIED=()
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

tree() {
  {
    echo "## $1 $(date -u +%FT%TZ)"
    if [ "${RUN_GIT:-1}" = 1 ]; then
      git -C "$W" rev-parse HEAD 2>&1
      git -C "$W" status --short 2>&1
    else
      echo "not recorded (RUN_GIT=0)"
    fi
  } >> "$O/tree.txt"
}

rm -f "$O/done"
tree before

# One family: $1 crate, $2 family, $3 path in the kit, $4 threads, $5 build dir name, $6 cargo feature flags.
run_one() {
  local crate=$1 fam=$2 path=$3 threads=$4 bdir=$5 feats=$6
  local tests="$W/crates/$crate/tests"
  local dst="$tests/$fam.rs"
  local raw="$O/$bdir/raw/$fam.txt"
  mkdir -p "$O/$bdir/raw" "$O/lines"
  if [ -e "$dst" ]; then
    echo "refusing: $dst is already there" | tee -a "$O/progress" >&2
    echo 125 > "$O/$bdir/$fam.exit"
    return
  fi
  cp "$KIT/$path" "$dst" || { echo 126 > "$O/$bdir/$fam.exit"; return; }
  COPIED+=("$dst")
  local t0=$SECONDS
  # shellcheck disable=SC2086
  ( cd "$W" && K11RV_OUT="$O/lines" ZZ_OUT="$O/$bdir/$fam.tsv" \
      cargo test -p "$crate" $feats --test "$fam" -- --nocapture --test-threads="$threads" ) > "$raw" 2>&1
  local code=$?
  echo "exit $code" >> "$raw"
  echo "$code" > "$O/$bdir/$fam.exit"
  rm -f "$dst"
  COPIED=()
  if [ "${KEEP_BINARIES:-0}" != 1 ]; then
    rm -f "$TGT"/debug/deps/"$fam"-*
  fi
  local rows
  rows=$(python3 -I "$KIT/scripts/norm.py" "$raw" "$O/$bdir/$fam.txt" | cut -f2)
  echo "$bdir $fam exit $code $rows $((SECONDS - t0))s" >> "$O/progress"
}

# families.tsv: family crate path group threads kind origin note
FAMS=()
while IFS=$'\t' read -r fam crate path group threads kind _; do
  case $fam in ''|'#'*|family) continue ;; esac
  if [ -n "$FILTER" ] && ! [[ "$group/$fam" =~ $FILTER ]]; then continue; fi
  FAMS+=("$fam|$crate|$path|$group|$threads|$kind")
done < "$KIT/families.tsv"
if [ ${#FAMS[@]} -eq 0 ]; then
  echo "no family matches '$FILTER'" >&2
  exit 2
fi

for b in $BUILDS; do
  feats=""
  [ "$b" = bundled ] && feats="--features bundled-fonts"
  ran=0
  bad=""
  lines_ran=0
  lines_bad=""
  for entry in "${FAMS[@]}"; do
    IFS='|' read -r fam crate path group threads kind <<< "$entry"
    if [ "$crate" = tinker-pdf-shape ]; then
      [ "$b" = default ] || continue
      run_one "$crate" "$fam" "$path" "$threads" lines ""
      lines_ran=$((lines_ran + 1))
      [ "$(cat "$O/lines/$fam.exit")" = 0 ] || lines_bad="$lines_bad $fam"
    else
      run_one "$crate" "$fam" "$path" "$threads" "$b" "$feats"
      ran=$((ran + 1))
      [ "$(cat "$O/$b/$fam.exit")" = 0 ] || bad="$bad $fam"
    fi
  done
  if [ $ran -gt 0 ]; then
    printf 'families %d\nnot exit 0:%s\n' "$ran" "$bad" > "$O/$b/done"
  fi
  if [ $lines_ran -gt 0 ]; then
    printf 'families %d\nnot exit 0:%s\n' "$lines_ran" "$lines_bad" > "$O/lines/done"
  fi
  echo "$b: $ran families, not exit 0:${bad:- none}; lines: $lines_ran, not exit 0:${lines_bad:- none}"
done

tree after
echo done > "$O/done"
