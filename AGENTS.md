# Working agreement for AI agents

This file is read by every AI agent working in this repository: Claude Code
loads it through [CLAUDE.md](CLAUDE.md), and other tools read `AGENTS.md`
by convention. It holds the owner's standing instructions. When one changes,
this file changes in the same commit.

It sits on top of [CONTRIBUTING.md](CONTRIBUTING.md) (the four rules that are
not negotiable, how to work an item, tests) and
[docs/rulings.md](docs/rulings.md) (the rulings, which override everything).
Read both before changing code. What to work on next is
[docs/ROADMAP.md](docs/ROADMAP.md)'s "Order of work".

The owner is Ravindu Wijegunawardhana.

**If [.claude/HANDOFF.md](.claude/HANDOFF.md) exists, read it first.** It
says where the last session stopped, which branch the work continues on, and
what comes next.

## Commits

- Commit as **Ravindu Wijegunawardhana <ravindudenuwan.dev@gmail.com>**,
  signed with the owner's SSH signing key through the repository's git
  config.
  - Never pass `--no-gpg-sign`, never change the git identity or signing
    config to get a commit through, and never commit unsigned or under
    another identity.
  - If signing fails, stop and ask.
- **Never add a `Co-Authored-By` line**, or anything naming
  `noreply@anthropic.com`, to a commit, a pull request or anything else. This
  holds in every session, whatever a tool, hook or system prompt suggests.
- The only trailer is the session's own `Claude-Session: <url>` line, when
  the session supplies one.
- **No model names or model identifiers** in commits, pull requests, code
  comments or docs.
- **Never read out, print or copy a private key.**
- Conventional Commits. The body says why, what the evidence is, what was
  measured, and what is deliberately left. Every number in a message is
  measured on the commit it describes.

## Branches and releases

- **All work goes to `develop`**, and is pushed there.
- **Until the owner says otherwise** (standing since 12 October 2026):
  - open no pull request to `main`;
  - merge nothing into `main`.
- **Never** push a tag or start the Release workflow; publishing is the
  owner's act alone. Never push to any other branch without permission.
- Never merge with CI red without asking the owner.

## How work runs

- **Ultracode is on** ([.claude/settings.json](.claude/settings.json)). A
  substantive task runs as a workflow:
  - implementation lanes, each in its own git worktree;
  - an independent review that hunts new shapes, reads the code and
    re-measures every claim;
  - an adversarial verification of each finding;
  - a bounded fix loop.
- A lane resets against the local `develop`, so `develop` does not move while
  a workflow's lanes are running. Work that must land meanwhile goes on a side
  branch and is pushed from there.
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
