#!/usr/bin/env bash
# The right-to-left probe kit's runner.
#
# usage: run.sh WORKTREE BUILD OUTDIR [FILTER]
#        run.sh -h | --help
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
#   OUTDIR/progress                    one line per family as it finishes, and
#                                      one if the run is interrupted
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
# An interrupt stops the whole run and writes no done marker: Ctrl-C, or
# SIGINT, SIGTERM or SIGHUP sent to this script, and also a family whose cargo
# or test binary was killed by one of those signals or by SIGKILL (a family
# stopped from outside is not a measurement, whoever stopped it). Each cargo
# runs as a job in its own process group, so a signal reaches this script
# first and the script passes it on to the whole job. A shell that starts
# this script in the background with SIGINT ignored cannot give it back:
# Ctrl-C then reaches nothing, and the script says so when it starts; stop
# such a run with `kill -TERM <pid>`.
#
# Environment: CARGO_TARGET_DIR (default WORKTREE/target); CARGO_BUILD_JOBS
# (default 3). KEEP_BINARIES=1 keeps each probe's test binary (by default it
# is deleted after its run, so that two builds fit on a small disk).
# RUN_GIT=0 skips recording HEAD and status, for a tree that is not a git
# checkout (an exported archive). Every variable a probe reads with
# `env::var` (RV_FILTER, RV3_FILTER, RV4C_FILTER, RV_FULL, the DBG_ knobs, ...)
# is unset, found by reading the probes rather than from a list, except the
# two this script sets itself: K11RV_OUT and ZZ_OUT.
set -u

usage() { sed -n '2,/^set -u$/{/^set -u$/!p}' "$0"; }
case ${1:-} in -h|--help) usage; exit 0 ;; esac
if [ $# -lt 3 ]; then
  usage >&2
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

# The probes' own knobs narrow or change what they print; a kit run never has
# them. Read off the probes, so that a knob a new probe adds is unset too.
KNOBS=$(grep -rhoE 'env::var(_os)?\("[A-Za-z0-9_]+"\)' "$KIT/probes" | sed -E 's/.*\("([A-Za-z0-9_]+)"\)/\1/' | sort -u)
for knob in $KNOBS; do
  case $knob in
    K11RV_OUT|ZZ_OUT) ;;
    *) unset "$knob" ;;
  esac
done

COPIED=()
cleanup() {
  for f in "${COPIED[@]:-}"; do
    [ -n "$f" ] && rm -f "$f"
  done
  COPIED=()
}
trap cleanup EXIT

# Job control puts each cargo run in a process group of its own. A terminal's
# Ctrl-C then reaches this shell, which is waiting in `wait` and so runs its
# trap at once, and the trap sends the signal on to the whole job, cargo and
# the test binary alike. Without it a cargo that handles the signal and exits
# with a status of its own reads, to this shell, as a family that failed.
set -m
JOB=
INTERRUPTED=
on_signal() {
  INTERRUPTED=$1
  if [ -n "$JOB" ]; then
    kill -"$1" -- "-$JOB" 2>/dev/null
  fi
}
trap 'on_signal INT' INT
trap 'on_signal TERM' TERM
trap 'on_signal HUP' HUP
# A signal ignored when a non-interactive shell starts cannot be trapped: the
# trap above was refused without a word, and `trap -p` shows it ignored.
if ! trap -p INT | grep -q on_signal; then
  echo "note: SIGINT is ignored in this shell (started in the background?), so Ctrl-C will not stop this run; stop it with kill -TERM $$" >&2
fi

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

# Stop the whole run: no done marker, of the run or of any build.
stop() {
  echo "interrupted ($1) in $2 after ${SECONDS}s: the run stops here and writes no done marker" | tee -a "$O/progress" >&2
  cleanup
  tree interrupted
  exit 130
}

# Whether a family's cargo run was stopped from outside: cargo itself killed by
# a signal (an exit of 128 + N), or cargo reporting its test binary or a
# compiler killed by one.
stopped_from_outside() {
  local code=$1 raw=$2
  case $code in 129|130|131|137|143) return 0 ;; esac
  grep -qE '\(signal: (1|2|3|9|15), SIG' "$raw"
}

# One family: $1 crate, $2 family, $3 path in the kit, $4 threads, $5 build dir name, $6 cargo feature flags.
run_one() {
  local crate=$1 fam=$2 path=$3 threads=$4 bdir=$5 feats=$6
  local tests="$W/crates/$crate/tests"
  local dst="$tests/$fam.rs"
  local raw="$O/$bdir/raw/$fam.txt"
  [ -z "$INTERRUPTED" ] || stop "$INTERRUPTED" "$bdir/$fam"
  mkdir -p "$O/$bdir/raw"
  [ "$bdir" = lines ] && mkdir -p "$O/lines"
  if [ -e "$dst" ]; then
    echo "refusing: $dst is already there" | tee -a "$O/progress" >&2
    echo 125 > "$O/$bdir/$fam.exit"
    return
  fi
  cp "$KIT/$path" "$dst" || { echo 126 > "$O/$bdir/$fam.exit"; return; }
  COPIED+=("$dst")
  local t0=$SECONDS
  # shellcheck disable=SC2086
  ( cd "$W" && export K11RV_OUT="$O/lines" ZZ_OUT="$O/$bdir/$fam.tsv" &&
      exec cargo test -p "$crate" $feats --test "$fam" -- --nocapture --test-threads="$threads" ) > "$raw" 2>&1 &
  JOB=$!
  wait "$JOB"
  local code=$?
  # `wait` returns early when a trapped signal arrives; the trap has passed it
  # on, so wait again for the job itself.
  while kill -0 "$JOB" 2>/dev/null; do
    wait "$JOB"
    code=$?
  done
  JOB=
  rm -f "$dst"
  COPIED=()
  if [ -n "$INTERRUPTED" ]; then
    echo "interrupted ($INTERRUPTED)" >> "$raw"
    stop "$INTERRUPTED" "$bdir/$fam"
  fi
  if stopped_from_outside "$code" "$raw"; then
    echo "stopped from outside (exit $code)" >> "$raw"
    stop "a signal from outside, exit $code" "$bdir/$fam"
  fi
  echo "exit $code" >> "$raw"
  echo "$code" > "$O/$bdir/$fam.exit"
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

# A done marker left by an earlier run into the same directory would vouch for
# this one; remove every one this run could write before it starts.
rm -f "$O/done" "$O/lines/done"
for b in $BUILDS; do rm -f "$O/$b/done"; done
tree before

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
  [ -z "$INTERRUPTED" ] || stop "$INTERRUPTED" "$b, between families"
  if [ $ran -gt 0 ]; then
    printf 'families %d\nnot exit 0:%s\n' "$ran" "$bad" > "$O/$b/done"
  fi
  if [ $lines_ran -gt 0 ]; then
    printf 'families %d\nnot exit 0:%s\n' "$lines_ran" "$lines_bad" > "$O/lines/done"
  fi
  echo "$b: $ran families, not exit 0:${bad:- none}; lines: $lines_ran, not exit 0:${lines_bad:- none}"
done

[ -z "$INTERRUPTED" ] || stop "$INTERRUPTED" "the end of the run"
tree after
echo done > "$O/done"
