# Eleven fuzz targets compile and none has ever run

The targets exist, they build, and CI checks that they still build — which was
itself a fix, after four of them had been calling functions with the wrong
arity since the day they were written. What has never happened is running one.
Ruling 1 says the engine never panics on untrusted input; the stable
hostile-input sweep covers the same entry points far more shallowly. When this
is done, the rule is exercised rather than asserted. (S)

## What is wrong

`fuzz/fuzz_targets/` holds eleven targets: `ascii_filters`, `ccitt`, `cff`,
`content_tokenizer`, `cos_document`, `cos_object`, `inflate`, `jpeg`, `lzw`,
`render_page`, `truetype`.

None has been executed. There is no nightly job, no committed corpus, no
minimised crash directory.

Four targets plan 05 names are missing: `sfnt`, `type1` and `cmap` have no
target at all, and `cff` exists but does not reach the tables
[01](01-cff-glyph-selection.md) adds. Plan 02 wants one per leaf format from
day one.

For what it is worth, the sweep is not nothing — this session found a real
`extend` panic in the JPEG decoder, reachable from any file with a corrupt
Huffman table, via a hand-written progressive fixture rather than a fuzzer.
That is the class of bug a fuzzer finds in minutes and a hand-written test
finds by luck.

## Scope

- A nightly job running each target for a bounded time.
- Seed corpora, committed and minimised — small, and they make each run start
  from coverage rather than from noise.
- A short per-PR run over the seed corpus, which catches a regression that
  reintroduces a known crash without waiting for the nightly.
- The missing targets: `sfnt`, `type1`, `cmap`, and a `crypt` target for the
  decryption path.
- A place for minimised reproducers, so a crash becomes a committed test
  rather than a log line.

## Non-goals

- **OSS-Fuzz.** Plan 14 wants it once the repository is public. Not yet.
- **Fuzzing the facade.** `render_page` already covers the deep path; the leaf
  crates are where the byte-level parsers are and where fuzzing pays.

## Design

**A crash becomes a test.** The value is not the fuzzer finding something
once; it is the reproducer landing in the ordinary suite so it can never come
back. Minimised inputs go into the crate's own tests, in the style the
hostile-input sweep already uses, with a comment saying what it triggered.

**Seed corpora are committed and minimised.** Plan 14 says so. They are the
difference between a nightly that explores and one that spends its hour
rediscovering the file header.

**Bounded nightly time**, per target, so the job has a predictable length and
a new target does not starve the others.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Seed corpora committed and minimised for the eleven existing targets | Each target starts from seeds; corpus size is small enough to review | S |
| 2 | Nightly job, bounded per target | A scheduled run completes; a crash fails the job with the input attached | S |
| 3 | Per-PR run over the seed corpus | Adds a bounded time to CI; a reintroduced known crash fails the PR | S |
| 4 | The four missing targets | `sfnt`, `type1`, `cmap`, `crypt` exist and reach their formats' parsers | S |
| 5 | First real session; findings landed as tests | Every crash found becomes a committed reproducer test, or is documented as accepted with a reason | S |

## Dependencies

**Needs first:** nothing. Milestone 4's `cmap` target is most useful after
[03](03-predefined-cmaps.md) and [04](04-usecmap-and-codespaces.md) give it
something to chew on.

**Unblocks:** confidence in ruling 1 that is measured rather than claimed —
which matters most for the leniency ladder, whose entire job is surviving
hostile input.

## Risks

| Risk | Mitigation |
| --- | --- |
| A nightly that fails often gets ignored | A crash lands as a test in the ordinary suite, so the signal moves to where it cannot be ignored |
| `cargo-fuzz` needs nightly Rust, which the rest of CI does not | A separate job on a separate toolchain; the build check on stable stays as it is |
| Fuzzing finds a stream of shallow crashes and the work stalls | Bounded time per target, and the findings are landed as tests incrementally rather than as a batch |
