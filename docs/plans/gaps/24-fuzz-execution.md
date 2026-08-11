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

## As built

*August 2026.* Milestones 1 to 4 are done. **Milestone 5 is not**, and the
distinction matters more here than on most plans: the targets now build, run,
start from seeds and are wired into CI, but the longest session anyone has run
is thirty seconds a target. That is enough to prove the machinery works and
nowhere near enough to say ruling 1 holds. The campaign is still owed.

Two bugs, both real, and the more interesting one was found before a fuzzer
started.

**Building the `cff` seed found the CFF INDEX reader off by one.** The seed
is a valid font program by construction and `Cff::parse` refused it. Offsets
in an INDEX are 1-based from the first byte of the data; `get` subtracted
that one correctly and `parse` subtracted it again working out where the data
began, so every item in every INDEX was read one byte early and one byte
short. Because `Index::parse` also returns where it stopped, and that was
short too, the Top DICT INDEX was parsed starting inside the Name INDEX's
last byte — so `Cff::parse` returned `None` for essentially every real
program, and `resources.rs` read that as a face it could not handle. **No
embedded CFF or OpenType/CFF font had ever resolved a glyph.** Nothing caught
it because nothing had ever parsed a whole program: the crate's tests covered
DICT operand encodings, real-number nibbles, subroutine bias and refusal of
garbage, every one of which passes with the reader off by a byte. This is the
argument for seed corpora put better than the Design puts it — the seeds paid
for themselves before they were used for anything.

**The fuzzers' first find was a `cmap` format 12 overflow.** `sfnt` and
`truetype` crashed within thirty seconds of each other on the same line:
`startGlyphID + (code - startCharCode)` in `u32`, all three from the file, so
a `startGlyphID` near the top of `u32` overflows the addition. `attempt to
add with overflow` under the fuzzer's overflow checks, and a wrapped glyph id
without them. Format 12 is the subtable used for anything past the BMP, so it
is reachable from any document embedding a CJK or emoji face. Fixed with
`checked_add` before the `u16::try_from` that was already there and could
never run. `cargo fuzz tmin` got it from 673 bytes to 358 and no further,
because the input has to stay a parseable table directory all the way down;
the committed reproducer is a hand-built 68-byte font, which is the actual
minimum and is reviewable.

Five things the plan did not say.

1. **libFuzzer does not build on `x86_64-pc-windows-msvc`,** which is this
   repository's development host. The runs behind this section happened in
   WSL2 under Ubuntu 24.04 against the same working tree at
   `/mnt/c/...`. Nothing about that is in CI's way — the existing
   `fuzz-targets` job was already `ubuntu-latest` for the same reason — but
   it is why "run the fuzzers" is not a thing a Windows contributor can do
   without setting up a Linux environment first, and the README now says so.

2. **The nightly is its own workflow file; the per-PR run is a job in
   `ci.yml`.** The trigger decides, because in GitHub Actions the trigger
   belongs to the workflow rather than the job: `schedule:` added to `ci.yml`
   would schedule the three-OS test matrix, wasm, the bindings and the licence
   check every night unless all six jobs grew a guard. The per-PR run has
   exactly `ci.yml`'s trigger, so it lives there. Both bound time *per target*
   rather than in total, but differently — the nightly gives each target its
   own runner, the per-PR job runs them in sequence on one, because holding
   fifteen machines for twenty seconds each is worse for the queue than
   holding one for five minutes.

3. **The nightly matrix is read off `fuzz_targets/`, not written down.** A
   hardcoded list is a list someone forgets to add a target to, which is the
   defect this plan exists to end. `ci.yml`'s loop reads `cargo fuzz list` for
   the same reason.

4. **A run rewrites the corpus directory it is given,** and `cargo fuzz cmin`
   renames every file in it to its hash. Both are irreversible and both
   destroy the descriptive seed names the whole corpus commit is built on.
   That is fine on a CI runner, whose checkout is discarded, and a trap
   locally; the README says to minimise a copy.

5. **The `cff` target's seed lives in two places on purpose.** It is built by
   a test in `cff.rs` and committed under `fuzz/corpus/cff`, so the assertion
   and the seed cannot drift. The format 12 reproducer does the same thing for
   the opposite reason: a crash lands as a test *and* as a seed, so the per-PR
   replay fails on a regression rather than waiting for the nightly.

6. **Execution rates differ by four orders of magnitude,** and one of them is
   not a defect to fix. Over thirty seconds each on the reference host:
   `ascii_filters` 76k executions a second, `content_tokenizer` 61k,
   `cff` 50k, `type1` 46k, `truetype` 42k, `lzw` 36k, `cos_object` 31k,
   `cmap` 17k, `inflate` 16k, `ccitt` 13k, `sfnt` 4.5k, `cos_document` 2.8k,
   `render_page` 1.3k, `jpeg` 732 — and `crypt` **16**. `crypt` is slow
   because revision 6 password hashing is *designed* to be slow: 7.6.4.3.3's
   hardened hash runs sixty-plus rounds of AES over a repeated buffer before
   it can say no. The target is not doing anything wasteful and making it
   faster would mean not authenticating, which is the thing being fuzzed.
   Read a `crypt` nightly as ten thousand executions rather than ten million,
   and judge its coverage on that. The number is recorded here so that a
   future reader does not mistake the format for a broken target.

Fifteen targets, 66 seed files, 25 KB. Every target was proved to build and
to run: thirty seconds each, from the committed seeds, all fifteen exiting
zero after the format 12 fix. Three other gap plans have `cargo fuzz run` in
their exit criteria — [03](03-predefined-cmaps.md) milestone 6 (`cmap`),
[16](16-ccitt-completion.md) milestone 5 (`ccitt`) and
[17](17-jbig2-generic-region.md) milestone 7 (`jbig2`) — and none of them is
blocked on toolchain work any more. `jbig2` has no target yet because it has
no decoder yet; it arrives with 17, as plan 02 asks.
