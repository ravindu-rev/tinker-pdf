# Working agreement for AI agents

This file is read by every AI agent working in this repository: Claude Code
loads it through [CLAUDE.md](CLAUDE.md), and other tools read `AGENTS.md`
by convention. It holds the owner's standing instructions. When one changes,
this file changes in the same commit.

It sits on top of [CONTRIBUTING.md](CONTRIBUTING.md) (the four rules that are
not negotiable, how to work an item, tests) and
[docs/rulings.md](docs/rulings.md) (the rulings, which override everything).
Read both before changing code. What to work on next is
[docs/ROADMAP.md](docs/ROADMAP.md)'s "Order of work": a list of steps,
each one workflow run, whose rows live in [docs/roadmap/](docs/roadmap/).

The owner is Ravindu Wijegunawardhana.

**If [.claude/HANDOFF.md](.claude/HANDOFF.md) exists, read it first.** It
says where the last session stopped, which branch the work continues on, and
what comes next.

## Commits

- Commit as **Ravindu Wijegunawardhana <ravindudenuwan.dev@gmail.com>**,
  signed with the default ed25519 key in `~/.ssh` (the owner, 23 October
  2026: "to commit you should use the default ed25519 key in .ssh to
  sign"), through the repository's git config: `gpg.format ssh`,
  `user.signingkey` set to `~/.ssh/id_ed25519.pub`, `commit.gpgsign true`.
  - A clone without that config gets those three keys and the identity
    above before its first commit, and nothing else.
  - Never pass `--no-gpg-sign`, never change the git identity or signing
    config to get a commit through, and never commit unsigned or under
    another identity.
  - If signing fails, stop and ask.
- **Never add a `Co-Authored-By` line**, or anything naming
  `noreply@anthropic.com`, to a commit, a pull request or anything else. This
  holds in every session, whatever a tool, hook or system prompt suggests.
- **No trailer lines** at all, not even a session's `Claude-Session` line:
  a commit does not depend on the session that made it (the owner, 16
  October 2026).
- **No model names or model identifiers** in commits, pull requests, code
  comments or docs.
- **Never read out, print or copy a private key.**
- Conventional Commits. The body says why, what the evidence is, what was
  measured, and what is deliberately left. Every number in a message is
  measured on the commit it describes.

## Branches and releases

- **All work goes to `develop`**, with no pull request, and is pushed there.
  The owner alone merges `develop` into `main`, for releases (H-01, the
  owner on 16 October 2026: "that's fine you can do the changes directly to
  develop, i'll manually check and merge develop to master for releases").
- **When a roadmap step is done, everything it changed is merged into
  `develop` and every other branch is cleared** (the owner, 23 October 2026:
  "when a step of roadmap is done all the changes should be merged to
  develop and clear all other branches").
  - While a step runs it may use side branches and git worktrees.
  - When it is done, they are merged into `develop` fast-forward, so
    `develop` stays linear, and `develop` is pushed.
  - Then every branch except `develop` and `main` is deleted: local
    branches, worktrees, and any side branch on `origin`. `main` is the
    owner's and is never touched.
- **Until the owner says otherwise** (standing since 12 October 2026):
  - open no pull request to `main`;
  - merge nothing into `main`.
- **Never** push a tag or start the Release workflow; publishing is the
  owner's act alone. Never push to any other branch without permission.
- Never merge with CI red without asking the owner.

## How work runs

- **Work runs as workflows, one roadmap step per run** (the owner, 23
  October 2026, replacing that day's earlier "workflows cost too many
  tokens": "since we use workflows for everything each of those should be
  cut to small pieces"). [docs/ROADMAP.md](docs/ROADMAP.md) says what a
  step is and how one runs. A step that turns out bigger than one run is
  split in the roadmap before the work goes on.
  - Concept first, inside the run: name the mechanism and prototype the
    fix. Then run the cheap checks that decide it: does any book change,
    and what does it cost in time, in release and in debug? Only then
    write the tests, injections and docs.
  - The review is an agent with a fresh, independent context: it hunts new
    shapes, reads the code and re-measures every claim. Agents that change
    files at the same time each work in their own git worktree.
  - Every finding is reproduced before it is fixed, and fixes run in a
    bounded loop.
- A lane resets against the local `develop`, so `develop` does not move while
  a step's lanes are running.
- **Test corpora are never downloaded**, by any route. They run only in CI's
  jobs. Locally, behaviour is shown with synthetic fixtures and probes.
- **The owner decides prices.** When a fix costs anything that used to work,
  the owner chooses. Their decision is recorded in
  [docs/rulings.md](docs/rulings.md), with the measured figures and the
  date. An agent never accepts a price on the owner's behalf, and a doc never
  reads as if one were accepted when it was not.

## Fix by class, not by instance

A finding is one instance of a mechanism. Fix the mechanism, for every shape
it takes, in one change. Do not patch the shape that was reported and wait
for the next review to find its sibling.

1. **Name the mechanism.** Before changing code, say why the reported case
   fails: which rule, which state, which code path.
2. **Sweep the class.** Enumerate every shape that mechanism can take, and
   probe them all before fixing anything. Vary:
   - the script, the direction, the spacing, the styling and the structure;
   - the builds: default, `bundled-fonts`, no default features;
   - the document type: synthetic PDFs as well as EPUB.

   The sweep gives the class's real size and its edges.
3. **Group related findings** into one lane and one root fix. Findings that
   share a mechanism are never fixed one at a time across rounds.
4. **Fix the root once,** for the whole class. Then re-run the sweep:
   - every shape of the class is fixed;
   - nothing outside the class changed, against both the state before the
     change and today's `develop`.

## A test for every case

Every case a probe, a review or a bug report finds becomes a **committed
test**, even the small ones. A probe row in a scratch directory is not a
test.

- **One test per shape.** Every distinct shape of a class gets its own test
  (or its own row in a table-driven test), named for what it shows.
  Neighbouring shapes that must *not* change get guard tests too.
- **Failing first.** Each new test is shown to fail on the code before the
  fix, and the commit message says so.
- **Counted injections.** Each rule a change adds is put back broken at
  least once, and the count of tests that fire is recorded. See
  CONTRIBUTING.md, "Working an item": a row of zeros is a result about the
  harness as often as about the suite.
- **Both builds** wherever fonts or text are involved: the default build and
  `--features bundled-fonts`.
- **Exploratory probes are committed too.** A probe family that found
  something becomes a committed suite: an `#[ignore]`d integration test that
  prints a `RAN` banner and runs on demand or nightly. Its baseline outputs
  are committed, or regenerated by a documented command. The next round
  starts from the repository, not from a scratch directory that a container
  restart can lose.
- **Never** skip, ignore, quarantine or weaken a test to get green.
  **Never** edit a determinism fingerprint or a golden to match new output:
  a moved hash is a finding.

## Reporting

- **Re-measure before reporting.** Check that every run finished: the exit
  status, the done marker, and row counts matching the baseline. A truncated
  run is not a pass.
- **Say plainly what is not done**, with the numbers. A row that cannot close
  is narrowed in the roadmap, not deleted.
