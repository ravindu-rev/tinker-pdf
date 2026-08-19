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

*Amended, 19 August 2026.* **It is not owed any more** — every target has had
a session, and the section at the bottom of this file records what each one
bought. Two things below are corrected by it in place: point 6's reading of
`crypt`, and the target count, which is twenty-two.

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

   *Corrected, 19 August 2026, by milestone 5.* **"The target is not doing
   anything wasteful" was wrong.** The hardened hash is genuinely slow and
   genuinely has to be, and that accounted for about a third of it. The rest
   was the harness: every `/Encrypt` field was carved at exactly its legal
   width, so a *three-byte* input still built a 48-byte `/O` and `/U` and cost
   184 ms, and every input authenticated twice. Worse, the cost bought nothing
   — with the widths fixed, every revision-6 execution took the identical path
   — and the whole of `FileKey` past the password check had never executed at
   all. Both are fixed; the rate roughly doubled, the ciphers moved into a
   target of their own where they run five hundred times faster, and the
   conclusion of this point survives in a weaker form: `crypt` is still slow,
   still for a reason, and a green `crypt` nightly is still to be read as ten
   thousand executions rather than ten million. The section at the bottom of
   this file has the measurements.

Fifteen targets, 66 seed files, 25 KB. Every target was proved to build and
to run: thirty seconds each, from the committed seeds, all fifteen exiting
zero after the format 12 fix. Three other gap plans have `cargo fuzz run` in
their exit criteria — [03](03-predefined-cmaps.md) milestone 6 (`cmap`),
[16](16-ccitt-completion.md) milestone 5 (`ccitt`) and
[17](17-jbig2-generic-region.md) milestone 7 (`jbig2`) — and none of them is
blocked on toolchain work any more. `jbig2` has no target yet because it has
no decoder yet; it arrives with 17, as plan 02 asks.

---

## Progress — 19 August 2026, milestone 5

**Milestone 5 is done: every one of the twenty-one targets has had a session**,
which is the thing the `As built` above says is owed and which nothing before
now had done. **186 159 981 recorded executions.** Two crashes, one of them a
real defect in the engine that is fixed — and one target's *result* is a
finding in its own right, which is the part of this milestone that took the
work and which produced a twenty-second target.

The route is the one four other plans record, because libFuzzer does not exist
on `x86_64-pc-windows-msvc`: WSL2, Ubuntu 24.04, `rustc 1.100.0-nightly
(34baba539 2026-08-16)`, `cargo-fuzz 0.13.2`, the tree copied onto ext4 because
building across `/mnt/c` is glacial, and `wsl -u root` because `sudo` wants a
password in a non-interactive shell.

```
cargo fuzz run <target> fuzz/corpus/<target> -- \
  -max_total_time=300 -timeout=25 -rss_limit_mb=2048 -print_final_stats=1
```

### The sessions

Seventeen targets ran for 300 seconds each; four had already run for longer,
inside the plans that built them. Nothing here is rounded, and the two sessions
that ended early say why they did.

| Target | Executions | Session | Peak RSS | Findings |
| --- | --- | --- | --- | --- |
| `png` | **122 293 604** | 1 200 s, gap 29 M6 | 461 MB | none |
| `ascii_filters` | 18 600 384 | 301 s | 577 MB | none |
| `truetype` | 8 261 891 | 301 s | 534 MB | none |
| `lzw` | 8 211 527 | 301 s | 406 MB | none |
| `type1` | 5 994 689 | 301 s | — | none |
| `form_script` | 4 682 094 | 301 s | — | none |
| `content_tokenizer` | 4 334 087 | 301 s | — | none |
| `zip_archive` | 4 249 823 | 1 200 s, gap 29 M6 | 446 MB | none |
| `cff` | 2 962 131 | 301 s | — | none |
| `cos_object` | 1 569 219 | **to first crash** | 667 MB | **one, real, fixed at `a6a4fef`** |
| `ccitt` | 1 502 867 | 301 s | 467 MB | none |
| `inflate` | 1 468 872 | 301 s | 454 MB | none |
| `cos_document` | 636 121 | 301 s | 592 MB | none |
| `sfnt` | 622 593 | 301 s | 483 MB | none |
| `jpx` | 405 918 | 301 s | 586 MB | none |
| `cmap` | 140 666 | 301 s | — | none |
| `jbig2` | 124 552 | 301 s | 386 MB | none |
| `render_page` | 65 940 | 936 s, gap 30 M9 | **1 911 MB** | none; slowest unit **16 s** |
| `jpeg` | 30 368 | 301 s | 289 MB | none |
| `crypt` | **2 635** | 301 s | 405 MB | **the rate itself; see below** |
| `xml` | to first crash | ~11 min, gap 30 M9 | — | **one — and the *target* was wrong** |
| `crypt_ciphers` | 984 348 | 301 s | 514 MB | none — and it did not exist before this milestone |

`render_page`'s peak sat at 1 911 MB against a 2 048 MB ceiling and its slowest
unit at 16 seconds against a 25-second timeout. Neither crossed, both are
closer to the edge than is comfortable, and they are recorded rather than
rounded off.

**`sfnt`'s first attempt is not in that table and should not be.** The sweep's
log recorded `Done 500 runs in -1 second(s)`, which is not a session; it was
re-run for a full 300 seconds and the number above is that run. A result that
reports a negative duration is exactly the sort of thing a campaign write-up
smooths over, and smoothing it over would have put a target in the "has had a
session" column that had not had one. `truetype`'s number was lost the same way
— to a `tail` in the sweep script — and was re-run rather than reconstructed.

Four orders of magnitude separate the top of that table from the bottom, and
the `As built` above already says that is not one defect to fix. What it is, is
a reason to read each row against its own scale rather than against the word
"clean". Two rows earn a section each.

### `cos_object`, and why the arithmetic is worth the space

The crash is written up in `a6a4fef` and in the regression test, so only its
shape belongs here: `0.0002E-7700000000000000`, found in 1.5 million runs.
The exponent's own accumulation *saturates*, so a huge negative exponent
arrives as `i32::MIN + 1` and negating that is fine; `build_real` then adds the
fraction's scale with `saturating_add`, which is also fine, and lands on
exactly `i32::MIN`, for which there is no `i32` negation. **Two saturating
operations, each correct, composing into the one value the third could not
take.** It panicked wherever overflow checks are on and *wrapped* in release,
where `POW10.get` then misses and the slow path runs on a number nothing had
clamped — a panic in debug and a wrong number in release, reachable from any
content stream. Fixed by moving two `return`s that were already there above the
fast path that negates. Both libFuzzer inputs are committed to
`fuzz/corpus/cos_object/`.

This is the plan's own thesis met: a hand-written test finds that by luck.

### `crypt`: a clean result at eight executions a second is not a clean result

`crypt` came back clean, and filing it as a pass would have been the single
most misleading line in this document. **2 635 executions in 301 seconds is
8 a second, against `ascii_filters`' 61 795** — so a `crypt` session buys
roughly what four *seconds* buys the filters. Whatever "twenty-one targets ran
clean" is worth, it is not worth the same for this one.

The `As built` above predicted the rate and gave a reason: revision 6 password
hashing is designed to be slow. That reason is true and was **about a third of
the story**; the other two thirds were defects in the harness rather than facts
about the format, and point 6 above is corrected in place accordingly.

**Measured first, on the reference host, one execution per process:**

| Seed | Cost |
| --- | --- |
| `r4-aes128` (revision 4) | **1 ms** |
| `r2-rc4-40` (revision 2) | **5 ms** |
| `short-input` — **three bytes** | **184 ms** |
| `r6-aes256` | **241 ms** |
| `r6-perms-tampered` | **300 ms** |

The three-byte seed is the whole diagnosis. It costs 184 ms because `Carve`
zero-fills and every field was taken at exactly the width the specification
names — so three bytes of input still produced a 48-byte `/O` and a 48-byte
`/U`, and 7.6.4.3.3's hardened hash ran over them twice.

Three things were wrong, and only the first is the format's fault.

1. **The hardened hash is slow by design and stays.** Algorithm 2.B runs at
   least sixty-four rounds of AES-CBC over a buffer repeated sixty-four times
   *because* it is meant to cost an attacker something. Making it cheap means
   not authenticating, which is the thing being fuzzed.
2. **The widths were not fuzzed, so the hash was paid for nothing.** With every
   field at its legal width, `params.u.len() < 48` was false on every input
   this target has ever run, and `params.o.len() >= 48` true on every one.
   Every revision-6 execution took the *identical* path — `hash_2b`'s own
   control flow depends on three lengths, all of which the harness fixed — so
   the hash was coverage-saturated after the first handful of inputs and bought
   nothing for the millions after. The target's doc comment says "the
   interesting inputs are the ones that are the wrong length", and the target
   then made every length right.
3. **Every input authenticated twice**, over the empty password and the carved
   one, which doubles the cost of the most expensive operation in the crate to
   explore a dimension the input can carry itself in one bit.

And a fourth, which is not about the rate at all and is the more serious of the
two problems:

4. **Nothing past authentication had ever been fuzzed.** `authenticate` returns
   `Some` only after a 128- or 256-bit equality against bytes the file
   supplies, so a *mutated* input reaches it with probability 2^-128. Coverage
   over the committed corpus put **`FileKey::object_key` at 0.00 %** — 7.6.2
   Algorithm 1, the per-object key that every PDF encrypted before 2008 uses on
   every string and every stream, **never executed, not once, by the target
   whose job it is** — and `FileKey::decrypt` at 26.32 %. `handler.rs` overall
   sat at 68.73 % of regions with four of its twenty-four functions never
   entered.

#### What was done

**A valid handler is built once per process.** `build_r6` and two
`authenticate` calls sit behind a `OnceLock`, so the cost is paid once for a
whole session instead of on every input and never succeeding. Every input then
drives the resulting keys — both outcomes, all four of Table 25's methods
across the two — over its own body. That is the "expensive setup happens once"
this milestone asked for, and it is what makes the second half of the crate
reachable at all.

**The widths are the input's to choose**, from a menu of four per field that
includes zero, the legal values, and — for `/O` and `/U` — **47**, one byte
short of the boundary revision 6 checks. Most inputs now refuse before they
hash, which is both cheaper and a better test, because those early returns
were previously unreachable.

**One authentication attempt per input**, the empty password or the carved one,
chosen by a control bit. And the carved one can now exceed 127 bytes, so
`PasswordTruncated` is a note some input can produce; the old menu stopped at
39.

**The ciphers moved out.** AES, RC4, MD5 and SHA-2 were driven at the bottom of
`crypt`, which means they were charged 184 to 300 ms an input for a key
derivation they have nothing to do with. `crypt_ciphers` is the twenty-second
target: no key derivation anywhere, and every assertion in it is a round trip
or a cross-check between two doors over one algorithm, because a cipher has no
other oracle — a second implementation is what ruling 9 keeps out of this
repository, and the published vectors are already unit tests. What a vector
cannot do is cover the lengths, and length handling is where the bugs are.

**The seeds are written by the crate now.** They had been hand-laid in the
target's carve order with nothing tying the two together, so rearranging that
order would have turned the one seed that authenticates into 240 bytes of
noise — and a corpus that no longer reaches what it was chosen for looks
exactly like one that does.
`cargo test -p tinker-pdf-crypto write_the_fuzz_seeds -- --ignored` writes both
corpora, in the shape `cff`, `zip_archive` and `png` already use. It is the
sixth such writer; the count of ignored tests goes 9 to 10 and the number that
*run* is unchanged at 2 243.

Two seeds exist because of what this campaign found. `r6-boundary-widths`
carries `/O` and `/U` at 47 bytes. **`r4-authenticates` is a pre-revision-6
document that matches its own password** — the only one in this repository, and
the thing that makes Algorithm 1 reachable at all. It is built inside
`handler.rs`'s own test module by `compute_key_legacy` and `expected_u`, the
private functions that own the algorithm, rather than by a restatement of
Algorithms 2, 4 and 5 in the fuzz target. There is no `build_legacy` to call
because this engine writes revision 6 and nothing else, and `testdata/` holds
exactly one encrypted document: `encrypted-aes256.pdf`, which is revision 6.

#### Measured after, same host, same flags, same 300 seconds

| | Before | After |
| --- | --- | --- |
| `crypt` executions in 300 s | 2 635 | **3 945**, and 4 303 and 4 947 on two other runs |
| `crypt` rate | 8/s | 13 to 16/s |
| `crypt` libFuzzer coverage | cov 798 | cov 853 |
| `handler.rs` regions, over the committed seeds | **68.73 %** | **91.64 %** |
| `handler.rs` functions never entered | 4 of 24 | 1 of 24 |
| `FileKey::object_key` | **0.00 %** | **100.00 %** |
| `FileKey::decrypt` | 26.32 % | 100.00 % |
| `build_r6` | 0.00 % | 95.60 % |
| `authenticate_r6` | 62.38 % | 80.20 % |
| the four ciphers, executions in 300 s | 2 635, inside `crypt` | **984 348**, and 1 281 602 on the other run |

Three sessions of the restructured `crypt` rather than one, because 300 seconds
of libFuzzer is noisy at this rate and a single number would have implied a
precision that is not there. The rate roughly doubled and the coverage of the
crate it drives went up by a fifth of the file.

The last row is the one worth reading twice. The four ciphers went from a
session's 2 635 executions to nearly a million — **about 370 times** — purely
by being taken out from under a key derivation they were never related to.
Neither target crashed, neither exceeded its ceiling, and both `artifacts/`
directories are empty.

The one function still never entered over the seeds is `infer_revision`, which
needs `/R` absent. The control byte can produce that and no seed does; it is
recorded rather than fixed, because a seed exists to reach what a mutator finds
hard, and three bits is not hard.

#### What remains slow, and what a `crypt` session is worth

Revision 6 still costs what Algorithm 2.B costs, and the arithmetic belongs
here rather than being rediscovered:

- At 13 executions a second, **a 300-second nightly buys `crypt` about four
  thousand executions.** `cos_object` needed 1.5 million to find its crash.
- Buying `crypt` those 1.5 million would take **32 hours**. Buying it
  `ascii_filters`' 18.6 million would take **sixteen days**.
- Removing AddressSanitizer roughly triples the rate — measured, 22/s against
  8/s on the old target over 60 seconds — and is **not** recommended: that is a
  detector traded for a rate, and detecting is the nightly's job.

So the honest reading of a green `crypt` nightly is *four thousand executions,
most of them over the cheap revisions*, and it should be read that way. What
makes that acceptable rather than merely admitted is that the expensive
revision's own arithmetic is not where the hostility lives. The field widths,
the revisions nobody writes any more, and everything downstream of a key are —
and all three are now either cheap or in a target that runs at three thousand a
second.

#### The injection matrix

Ten defects, one at a time, each reverted before the next, both corpora
replayed with `-runs=0 -timeout=25` — which is what the per-PR job does, so a
defect caught here fails a pull request rather than waiting for a nightly.
**Eight caught on the first pass, two survived; one of the two was closed and
the whole matrix re-run against the closure, so the standing figure is nine of
ten.**

| Defect | Caught by |
| --- | --- |
| `Aes::new` widened to accept a 24-byte key | `crypt_ciphers` |
| PKCS#7 adds nothing when the plaintext already ends on a block | `crypt_ciphers` |
| CBC decryption ignores the IV it was given | `crypt` **and** `crypt_ciphers` |
| **`Md5::update` drops a partial block across two calls** | **both — as a *hang*, not a wrong digest** |
| `sha384` truncates SHA-512's state at the wrong offset | `crypt_ciphers` |
| `build_r6` wraps a different key into `/OE` | `crypt` |
| `/Identity` returns nothing instead of the bytes | `crypt` |
| 7.6.2 Algorithm 1 stops mixing in the object number | `crypt` |
| `constant_time_eq` compares only the first byte | **survived**, then `crypt_ciphers` once the assertion gained the right pair |
| Algorithm 1 stops mixing in the `sAlT` constant | **survived**, and stays survived — below |

**The strongest result did not come back as a wrong answer.** `Md5::update`
carries a `return` under a three-line comment saying that falling through would
drop what was just buffered. Deleting it does not produce a wrong digest: it
produces a **hang**, because `Md5::finish` pads with
`while self.buffered != 56` and an `update` that resets `buffered` to zero on
every one-byte call never gets there. The first run of this matrix used
`-runs=0` with no `-timeout` and sat on that one input for ten minutes before it
was killed. Two things follow. The comment was defending *termination* as well
as correctness and neither it nor any test said so; and a matrix that replays a
corpus has to pass the nightly's `-timeout` or it cannot tell a defect from a
stuck job. It does now.

**The survivor that is closed.** `constant_time_eq` rewritten to compare only
the first byte survived, and the reason is a real weakness in the assertion
rather than in the idea: the pair it was given is a prefix and a suffix of one
buffer, which normally differ at byte zero, so the injected version and the
honest one both answer `false` and agree for different reasons. The property
that matters — a comparison that does not stop early — needs two strings that
**agree at the front and differ at the back**, and the target now builds one.
This is the milestone's own rule biting the milestone: a test for one
consequence of a property is not a test for the property.

**The survivor that stays.** Dropping 7.6.2's `sAlT` constant from Algorithm
1's AES branch changes the derived key and nothing notices, because neither
target has any way to know what the key should *be*. RC4 is its own inverse
under any key; two objects still differ from each other; the ciphertext is the
fuzzer's own bytes and there is no plaintext to compare against. `object_key`
went from 0.00 % to 100.00 % of regions in this milestone and bought exactly one
of the two things that number suggests: **reaching a line is not testing it.**
What would close it is a known-answer vector for Algorithm 1 in the crate's own
tests, or an encrypted-at-revision-4 document from an outside producer —
`qpdf` is already invoked by CI for linearization and can write one — and both
are unit-test work rather than fuzzing work. It is written down here rather
than done, and `testdata/` holding exactly one encrypted document, at revision
6, is the reason it is worth writing down.

### What milestone 5 proves, and what it does not

**Proves:** twenty-one targets have been run rather than smoke-tested; ruling 1
held across 186 million executions; and the two crashes that did happen were
found in minutes, one of them a real, reachable, silently-wrong-in-release
defect in the number lexer.

**Does not prove that 300 seconds is a campaign.** It is a first session, and
the gap between it and the twenty minutes gap 29 gave `zip_archive` and `png`
is a factor of four in time and a hundred million executions in yield. What
this milestone establishes is the *baseline*: every target has a number now,
the numbers differ by four orders of magnitude, and the difference is
understood in every case rather than assumed to be noise.

**And it proves nothing about `crypt` beyond the shallow.** That is stated here
rather than buried in the table, because the hazard this milestone exists to
avoid is a column of green ticks whose entries mean different things.
