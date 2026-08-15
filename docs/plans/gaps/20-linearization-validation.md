# Nothing has ever checked the hint tables

The linearized writer emits a complete Annex F file and every offset it
declares is checked against the actual bytes — by tests this engine wrote,
reading output this engine produced. `qpdf --check` and
`qpdf --show-linearization` are the arbiters plan 09 names, and neither has
run. The layout is well tested; the **hint tables are unproven**. When this is
done, an external validator agrees. (S)

## What is wrong

The tests assert the things a reader of our own output can assert: the
parameter dictionary comes first, `/L` is the file length, `/H` points at a
stream, `/E` precedes `/T`, the final `startxref` points into the front of the
file, page one's content is inside `/E` and the last page's is not.

None of that can catch a wrong hint table. The page-offset and shared-object
tables are bit-packed structures whose fields are read by nobody in this
repository — the writer emits them and nothing consumes them. They could be
entirely wrong and every test would still pass.

They are also the part most likely to be wrong. Hint tables are the
least-exercised structure in the format; most viewers ignore them, so bugs
survive for years in real implementations.

## Scope

- Run `qpdf --check` over linearized output.
- Run `qpdf --show-linearization` and compare its reading of the hint tables
  against what the writer intended.
- Wire both into CI as a subprocess oracle (ruling 9), skipped with a clear
  message when qpdf is absent rather than silently passing.
- A round-trip reader for our own hint tables, so the structure is verifiable
  offline too.

## Non-goals

- **Redistributing qpdf.** Ruling 9: oracles are subprocesses, invoked, never
  vendored, outputs used for comparison and never shipped.
- **Matching qpdf's layout.** Two linearizers may lay a file out differently
  and both be conformant. The bar is that qpdf reads ours without complaint,
  not that it would have produced it.

## Design

**The offline half matters as much as the qpdf half**, and can be built now.
A reader for our own bit-packed tables, in the test module, that parses them
back and checks the values against the layout the writer computed: least
objects per page, per-page deltas, the shared-object counts. That is a genuine
check — it catches a field written at the wrong bit width, a header item in
the wrong order, a delta computed against the wrong base — and it needs no
external tool.

What it cannot catch is a misreading of the specification: if the writer and
the round-trip reader share a wrong idea of the field order, they agree. That
is exactly what qpdf is for, and why both halves are wanted.

**Skipped, not silently passed.** A CI job that quietly succeeds when qpdf is
missing is a job that will one day be missing on every runner. It reports
skipped, visibly.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Round-trip reader for the hint tables | Every header field and per-page entry parses back to what the writer computed, for a one-page, a six-page and a shared-resource fixture | S |
| 2 | `qpdf --check` in CI over linearized output | Green on the fixtures; the job reports skipped, not passed, when qpdf is absent | S |
| 3 | `qpdf --show-linearization` compared | qpdf's page count, first-page object and hint-table summary agree with the writer's intent | S |
| 4 | Encrypted linearized output validated too | The same two checks pass on the output of [19](19-encrypt-and-linearize.md) | S |

## Dependencies

**Needs first:** nothing for milestones 1–3. Milestone 4 needs
[19](19-encrypt-and-linearize.md).

**Unblocks:** the claim that linearization works. Until this lands, STATUS
should keep saying the hint tables are unproven — which it does.

## Risks

| Risk | Mitigation |
| --- | --- |
| ~~qpdf is not installed in this environment, so milestones 2–4 cannot be verified locally~~ **Amended, 15 August 2026: qpdf 12.3.2 is installed and on `PATH`, and milestones 2–4 were run locally.** Nothing is marked unverified | Milestone 1 is the offline half and is verifiable now; the CI job is written and marked unverified until a runner has qpdf, exactly as the wasm determinism job is |
| A skipped job looks like a passing job | The job reports its skip explicitly, and STATUS carries the state until it has actually run |
| The round-trip reader and the writer share a misreading | Stated above as the known limit of the offline half; it is why the qpdf half is not optional |

*Amended, 15 August 2026.* Milestone 4's dependency on
[19](19-encrypt-and-linearize.md) is satisfied — encrypted linearized output
exists, and gap 19's `As built` names the fixture.

## As built — 15 August 2026

Six commits: one for the defect, one per milestone, and one closing what the
injection matrix found. 1 316 tests to 1 328; the full gate green on each,
plus the wasm determinism leg, and none of the nine rendering fingerprints
moved on either target — this is the writer.

The plan's premise held exactly. The hint tables "could be entirely wrong and
every test would still pass", and they were: **five separate faults**, none of
them caught by anything, in the one structure this repository writes and does
not read.

### What `overflow reading bit stream` actually was

Table F.6 item 2 is a one-bit flag saying whether a 128-bit MD5 signature
follows a shared-object entry. It was written at **zero** width, so an entry
occupied no bits at all — after the twenty-four byte header the stream simply
ended, and a reader asking for the first flag wanted one bit and had none.
That is the whole message: `wanted = 1; available = 0`, and the `1` is that
flag.

It could not have been reached anyway, because the entries were also packed in
the wrong order. Tables F.4 and F.6 list the items of *one* entry, which reads
as though entries were written one after another; they are not. Every entry's
item 1 is written first, for all entries, then every entry's item 2, and each
run is padded to a byte boundary. That is a measurement rather than a reading:
qpdf's own linearized output declares `/S 52` over a thirty-six byte
page-offset header and six pages, and only column packing accounts for the
sixteen bytes between them — 6×1 padded to 1, 6×7 to 6, 6×1 to 1, 5×2 to 2,
two empty runs, 6×7 to 6. Row packing of the same values needs thirteen.

Three more, all found by pointing `--check` at the one-page fixture, which is
the only one whose tables parsed far enough to be checked:

- **Table F.3 item 2 is a byte offset** — where the first page's page object
  is — and it held the object *number*. qpdf: `first page object offset
  mismatch`.
- **A page is a run of consecutive object numbers led by its page object.**
  F.4.1 hands a reader a count and nothing else. The writer numbered by
  section in old-object order, which put page one's font ahead of page one's
  page object, so three consecutive numbers from `/O` ran off the end of the
  file: `no xref table entry for 9 0`, and `page length mismatch for page 0:
  hint table = 293; computed length = 201`.
- **`/T` and `/E` named the wrong bytes.** Table F.1 makes `/T` the offset of
  the main table's *first entry* and `/E` the end of part 6; they were the
  offset of the `xref` keyword and the end of the hint stream, which sat after
  the first page's objects instead of in part 5 (F.3.1) where it belongs.

And one the fix needed that no error message named: **an offset inside a hint
table is measured as though the primary hint stream were not in the file.**
The tables are built before their own length is known, so everything behind
the hint stream is short by it. qpdf adds it back before printing, which is
how the rule was recovered: its own output stores `567` for a page object that
sits at `704`, with `H_offset 567` and `H_length 137`.

The no-patching property survives. The hint stream carries two offsets it
cannot know before its own length, so it is built once with zeros to measure
and once with the values; both sit in fixed 32-bit fields and every bit width
comes from counts and object lengths, so the two builds are the same size and
that is asserted. Nothing in the emitted buffer is revisited — the same trick
the parameter dictionary has always used.

### qpdf's verdict, after

`--check`: `File is linearized`, `No syntax or stream encoding errors found`,
no warnings, exit 0, on one-page, two-page, six-page and shared-resource
fixtures. `--show-linearization`: parses, exit 0, no warnings, and every field
agrees with a second computation made from the emitted bytes rather than from
the writer's internals.

**The encrypted file is affected by the missing `/ID`, and here is how.**
7.5.5 Table 15 requires one whenever `/Encrypt` is present; gap 19 left it as
a live audit row. qpdf reads, authenticates and fully parses the encrypted
linearized file — `R = 6`, `AESv3`, `Supplied password is user password`,
`File is linearized`, hint tables clean — and warns `invalid /ID in trailer
dictionary`. It does not touch linearization and it does not stop any
milestone. What it costs is one line: qpdf suppresses its `No syntax or stream
encoding errors found` summary once anything has warned, so that sentence
cannot be asserted on the encrypted file directly. It is asserted on a
decrypted copy instead — `qpdf --decrypt` then `--check`, clean with exit 0 —
which is the stronger statement anyway: every object offset resolved and every
stream decrypted. The allowance is by name, so a warning that does not mention
`/ID` fails, and `error encountered`, `mismatch` and `overflow reading` are
each forbidden outright.

### The offline half, and what it cannot do

The round-trip reader lives in `linearize.rs`'s test module and is written
from Tables F.3 to F.6 rather than from `hint_tables`. It checks every header
field and every entry against what `Plan::build` computed, on a one-page, a
six-page and a shared-resource fixture; deltas are checked by their sum with
the header's minimum, which is what a reader does, and widths separately,
because a field one bit narrow truncates silently. Two of its assertions are
about the stream rather than any field — `/S` must name the byte the shared
table starts on, and the reader must finish on the last byte of the data — and
the second is what fails on zero-width entries, which satisfy every value
assertion and leave the reader twenty-four bytes short. A second test measures
the counts and lengths against the serialised bytes, since both halves of a
round trip read the same `Plan`.

Its limit is not theoretical. Dropping the hint-stream length adjustment is
caught by **nothing offline at all** — not by the round-trip reader, which is
handed the offsets rather than deriving them, and not by any byte-level
assertion, because the file is internally consistent either way. Only qpdf
says so, on four tests. That is the plan's stated limit turned into a number.

### The injection matrix

Each injected alone, `cargo test --workspace --no-fail-fast`, offline
assertions and qpdf oracle counted separately.

| Injected | Offline | qpdf |
| --- | --- | --- |
| A field at the wrong bit width (F.3 item 3 at 32) | 1 | 5 |
| A per-page delta against the wrong base | 1 | 4 |
| The shared-object count off by one (F.5 item 4) | 1 | 6 |
| The signature flag at zero width (F.6 item 2) | 1 | 6 |
| F.3 item 2 back to an object number | 1 | 4 |
| `/T` back at the `xref` keyword | 2 | 4 |
| **Hint offsets without the hint-stream adjustment** | **0** | 4 |
| `/E` measured past the hint stream | 1 | 4 |
| The page object no longer leading part 6 | 2 | 4 |
| The per-page entries packed row by row | 1 | 5 |
| qpdf removed from `PATH` | — | job red, see below |

Two of those rows were zero offline before this gap finished, and the
assertions that make them one and two were added because the matrix said so:
the page object leading part 6 was never asserted (the run test checks the
gaps *between* page objects, and an object placed in front of `/O` is outside
every one of them), and `/E` was checked only by inequalities that an `/E`
measured a little long still satisfies.

### The skip is visible, and that was measured

With qpdf removed from `PATH`, `cargo test` exits 0 and reports `2 passed`
with the oracle never having run — indistinguishable from a real pass, which
is the hazard `f864b8b` fixed for `wasm-determinism`. The CI job copies that
fix: `tee` under `pipefail`, a grep for a non-zero pass count, a grep for
`qpdf-oracle: RAN`, and an explicit `::error::` and non-zero exit if
`qpdf-oracle: SKIPPED` appears instead. Run both ways: red with the error
message when qpdf is absent, green when it is present.

### Deliberately not done

- **Matching qpdf's layout.** It numbers its parts differently, pads part 2 so
  it can patch it afterwards, and places its remaining pages ahead of its
  shared section. The bar is that it reads ours.
- **Vendoring qpdf** (ruling 9). It is a subprocess; its output is read for
  comparison and dropped, and the fixtures live in `CARGO_TARGET_TMPDIR`.
- **Writing an `/ID`.** It is a defect of encrypt-on-save rather than of
  linearization, it has a live row in `audit-2026-08.md`, and choosing how to
  derive an identifier is a design decision rather than a patch.
- **A corpus.** Plan 09's milestone 5 asks for the oracle "across the
  linearized corpus subset"; there is no corpus in this repository until
  [23](23-corpus-runner.md). Four fixtures, encrypted and not, is what exists.
