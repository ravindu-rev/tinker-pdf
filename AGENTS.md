# Working agreement for AI agents

Claude Code loads this file through [CLAUDE.md](CLAUDE.md); other tools
read `AGENTS.md` by convention. It holds the owner's standing instructions.
[CONTRIBUTING.md](CONTRIBUTING.md) has the build and the four rules that are
not negotiable, and [docs/rulings.md](docs/rulings.md) the engineering
rulings, which override everything. The owner is Ravindu Wijegunawardhana.

## What to work on

- [docs/ROADMAP.md](docs/ROADMAP.md) and its step files in
  [docs/roadmap/](docs/roadmap/) are the only plan. Keep no plan, handoff or
  progress file anywhere else.
- Take what the owner asks for, or the next step in the roadmap's order of
  work. Find a step with `grep -n '| CD-19.3 |' docs/roadmap/CD.md` and read
  only the lines and the code it points to.

## How a change is made

1. Implement the feature or the fix. Fix the cause, for every case it
   produces, not only the case that was reported.
2. Write its tests: unit tests beside the code, integration tests in
   `tests/`.
3. When everything asked for is written, run the gates once:
   `python3 -I tools/gates.py`. Compiling while writing is fine; tests and
   gates are not run step by step.
4. Fix what fails and re-run only the gates that failed.
5. Delete each finished step from its step file, and a row once it has no
   steps left, together with its feature doc's refusal row. Update the
   feature doc so it describes what now exists. Commit and push.

- No reviews, no verifying agents, and no probes or measurements beyond the
  change's own tests.
- Several agents only when the work splits into independent parts, each in
  its own git worktree. Never a workflow.
- Never skip, ignore or weaken a test, or change a golden or a determinism
  fingerprint, to get green.
- If a change would make something that works today worse, stop and ask the
  owner. Their decision goes into [docs/rulings.md](docs/rulings.md) as a
  rule.
- Test corpora are never downloaded; they run only in CI.

## Docs

- Docs describe how the code is built and how it behaves now. No history:
  no dates, commit hashes, rounds, reviews, quotes, measured tallies or
  "used to". Git history is the record of change.
- Only the roadmap and its step files describe what is not built.
- A change that alters behaviour updates its feature doc in the same commit.

## Commits

- Commit as **Ravindu Wijegunawardhana <ravindudenuwan.dev@gmail.com>**,
  signed with the owner's ed25519 key in `~/.ssh` through the repository's
  git config (`gpg.format ssh`, `user.signingkey`, `commit.gpgsign true`).
  Never commit unsigned or under another identity, never pass
  `--no-gpg-sign`, and never change the identity or the signing config to
  get a commit through. If signing fails, stop and ask.
- No trailer lines. Never a `Co-Authored-By` line or anything naming
  `noreply@anthropic.com`, in a commit or anywhere else.
- No model names or model identifiers in commits, docs or code.
- Never read out, print or copy a private key.
- Conventional Commits; the body says briefly what changed and why.

## Branches and releases

- All work goes to `develop`, with no pull request, and is pushed there. The
  owner alone merges `develop` into `main`, for releases: open no pull
  request to `main` and merge nothing into it.
- Side branches and worktrees are fine while working. When the work is done
  it is merged into `develop` fast-forward and pushed, and every other
  branch, local or on `origin`, is deleted, except `develop` and `main`.
- Never push a tag or start the Release workflow. Never push to another
  branch without the owner's permission. Never merge with CI red without
  asking the owner.

## Reporting

Say plainly what is done and what is not.
