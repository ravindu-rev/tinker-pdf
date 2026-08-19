# Execution order, and the gaps that were not in the gap list

[README.md](README.md) says what each of the twenty-eight gap plans is for.
This file says what order they are being closed in, why that order is not the
order they were written in, and what five pieces of work turned out to be
missing from the set entirely.

It exists because the twenty-eight plans were each written self-contained, and
self-contained documents cannot see each other. Read as a batch they contain
dependency inversions no single one of them is wrong about: a plan whose exit
criterion needs a toolchain another plan stands up later, two plans that both
claim to create the same module, a plan whose milestones belong to an option
its author did not choose. Those are recorded here rather than in any one plan
because they are properties of the set.

Nothing in this file overrides a gap plan. Where it disagrees with one, the
plan has been amended in place and this file cites the amendment.

## The five items that were in no plan

Each of these blocks work that *is* planned. None of them appears in the
twenty-eight, in [16-build-sequence.md](../16-build-sequence.md), or in
[PLAN.md](../../PLAN.md).

### PRE-A — the determinism suite was hashing a blank page

**Status: done** — `9ba9237 fix(testing): make the determinism text fixture
actually draw glyphs`.

`crates/tinker-pdf/tests/determinism.rs` is the only pixel baseline in the
repository. Its `text` fixture named Helvetica and embedded nothing, and
`fingerprint` rendered with no `FontProvider` — so on an engine that bundles no
faces it rasterised nothing at all. Measured before the fix: **0 of 20 000
pixels non-background, one distinct pixel value, `warnings = [UnreadableFont]`,
and a hash byte-identical to a page that draws nothing.** That hash was the
committed fingerprint. Text extraction worked perfectly, which is why the
fixture read as healthy.

The fixture's own doc comment called it "the densest source of coverage
arithmetic in the engine".

Why it had to come first: gaps 01, 02, 03, 05 and 13 all change glyph selection
or glyph geometry, and "the fingerprints did not move" was not evidence about
any of them. Gap 14's milestone 1 exit criterion *is* "fingerprints do not
move", and gap 14's headline defect is one full-page mask per glyph — the glyph
path is precisely what the fixture failed to cover.

The durable half is the guard, not the fixture: `GOLDEN` now carries a
minimum-ink floor per fixture, and `fingerprint` refuses to hash a bitmap that
painted too little or that carries `UnreadableFont`.

### PRE-B — the wasm determinism leg, moved to the front

**Status: done** — `f864b8b`, `1714abf`. This is gap
[25](25-wasm-determinism-leg.md) M1–M3, hoisted.

[16-build-sequence.md](../16-build-sequence.md) ranks the cross-target
determinism test as item 6 of the whole programme and says to land it *before*
the render items start moving pixels. The hash table half landed months ago;
the wasm leg never ran. Eight gaps queued behind it (13, 14, 12, 15, 16, 11,
09, 10) all change pixel arithmetic, and gap 25 names exactly that class of
bug: an `as` cast whose overflow behaviour differs, a `usize` width assumption,
a flattening tolerance. Finding a divergence after all eight costs a twelve-
commit bisection with no cheap way to attribute it.

Outcome: `wasm32-wasip1` reproduces all four fingerprints byte for byte against
native Windows — 64-bit against 32-bit, which is the `usize` case. Nothing was
re-baselined.

The CI job needed one fix that had nothing to do with wasm: it could not tell a
run from a non-run. `cargo test` with a filter matching nothing prints
`0 passed; 2 filtered out` and exits 0, so an `#[ignore]` would have produced a
green tick and no rendering.

### PRE-C — the fuzz toolchain, moved to the front

**Status: done** — `04fb5ae` through `17be6b7`. This is gap
[24](24-fuzz-execution.md) M1–M4, hoisted; M5 stays at its numbered position.

Three separate plans carry a `cargo fuzz run` exit criterion — gap 03 M6
(`cmap`), gap 16 M5 (`ccitt`), gap 17 M7 (`jbig2`) — and all three sat *ahead*
of the plan that stands the toolchain up. Gap 03 M6 and gap 24 M4 also both
claimed to create the `cmap` target.

Hoisting M1–M4 cost nothing (the milestones have no dependencies) and unblocked
all three. It also found two live defects before any gap work began; see
[Findings that reshaped a plan](#findings-that-reshaped-a-plan).

### PRE-D — the JPX option-A plan does not exist

**Status: not started.** Runs immediately before gap
[18](18-jpx-decision.md).

Gap 18 is a decision document offering two options, and its milestone table is
explicitly option B's: *"Only for option B, since A needs its own plan written
after the decision."* Option A is a 3 500–4 500 line, 5–7 week build with no
milestones, no exit criteria, and two unanswered questions its own text says
must be settled **before any code**:

- the fixed-point fraction width for the 9/7 wavelet, and the perceptual budget
  against a float reference — written down before `dwt.rs` exists, because the
  plan's risk table names the failure as "made implicitly by whoever writes the
  wavelet";
- what the decoder is tested against. The repository holds zero JPX bytes,
  ISO/IEC 15444-4 conformance codestreams are not freely redistributable, and
  gap 18 pre-argues away the obvious oracle by noting a fixed-point wavelet will
  differ from every float-based reference decoder, OpenJPEG included.

PRE-D produces `18a-jpx-decoder.md` with those answers and a real milestone
table. Without it, a subagent handed "gap 18, option A" gets a several-thousand
line brief whose milestones describe different work.

### PRE-E — `DocumentEditor` has no transaction primitive

**Status: not started.** Runs immediately before gap
[27](27-form-calculations-decision.md).

Gap 27 option A requires that a calculation either applies wholly or not at
all. The forms API cannot express that. `DocumentEditor::set_field_value`
(`crates/tinker-pdf-cos/src/edit.rs`) takes one field, returns `bool`, and
applies everything immediately — object writes, per-widget appearance streams,
`clear_need_appearances`. There is no begin/commit/rollback anywhere on the
type.

It is not even atomic within a single field: the widget loop skips a widget
with no `/Rect` and still returns `true`, so a two-widget field can end with one
regenerated appearance and one stale, reported as success. That is
[16-build-sequence.md](../16-build-sequence.md)'s "a file that looks filled and
is wrong", already present one level below where gap 27 looks for it.

The fix is not structurally hard — `DocumentEditor` is a clean overlay over an
immutable `Arc<CosDocument>`, so a transaction is a snapshot of four fields —
but it is unscoped work, and it needs a decision about object-number allocation
under rollback or a rolled-back calculation leaks object numbers into every
subsequent save.

## Findings that reshaped a plan

These came out of a cross-plan review and out of the work itself. Each is
recorded in the plan it affects; they are collected here because several are
the reason the order changed.

**The CFF INDEX was off by one, so no CFF font had ever parsed.** Found while
building fuzz seed corpora (`263fcf0`), not by fuzzing. INDEX offsets are
1-based from the data's first byte; `get` subtracted the one and `parse`
subtracted it again. Because `Index::parse` also returns where it stopped, the
Top DICT INDEX began parsing inside the Name INDEX, so `Cff::parse` returned
`None` for essentially every real program.

This invalidated gap 01's premise. Its symptom description — "wrong, at the
right widths, so it looks like a font substitution rather than a bug" — was
unreachable, because the branch behind it never executed. Ground truth measured
before gap 01's fix: a **subset** font, which is what producers embed, drew *no
pixels* and warned `UnreadableFont`; only a full font drew the wrong letter.
The plan is amended.

**Gap 08 must land after gap 16, not before.** Gap 08 routes inline CCITT to
the shared decoder. Gap 16 flips that decoder's output shape and states that
the flip and the caller rewire must be one commit, because there is no
end-to-end CCITT test in the repository. Landing 08 first creates a second call
site the compiler cannot flag — `ccitt::decode`'s signature does not change,
only the meaning of the bytes it returns.

**Gap 07 lands all four of its milestones, not "the shading half".** The hazard
its text names is shading-painting-on-strokes *together with tiling patterns
silently black*. The stroke-path `UnsupportedPattern` warning is not the
deferred part — "the warning is the part that must not be skipped". Only
"tiling actually paints on a stroke" defers to gap 09, and gap 09 must then
re-run gap 07's CTM-anchoring assertion on the stroke path, which nothing
currently asks it to do.

**Gap 17 must put the MQ arithmetic decoder in its own module from its first
commit.** Gap 17 says "put it in its own module"; gap 18 says it "moves to a
shared module". If gap 17 reads that as a module inside `jbig2.rs`, gap 18
begins with a refactor inside an already-oversized commit. Same conclusion for
gap 16: gap 17 M5 needs a resumable row-level T.6 decoder at an arbitrary bit
offset, and gap 16's scope only exposes the whole-stream `ccitt::decode`. If gap
16 does not expose that seam, gap 17 duplicates T.6.

**The first corpus run will record a font-parity number, not an engine number.**
[16-build-sequence.md](../16-build-sequence.md) says a `FontProvider` "belongs
before anything else in the render list"; gap 28 owns it and cannot move before
gap 23, which needs it. Gap 23's headline pass rate is safe — it defines
`rendered` as "returned a bitmap without crashing" and excludes oracle
comparison — but its second axis, the share of files rendering *with warnings*,
will be dominated by `missing_fonts` on the widest class of real files, and
that number becomes the committed ratchet. Gap 23's brief therefore gains a
`--fonts` flag on the corpus runner and records two numbers, with and without
faces, saying which is which.

**There are no golden images in this repository.** Both
[16-build-sequence.md](../16-build-sequence.md)'s "re-baseline once, after item
14" and [README.md](README.md)'s "12–15 before any golden re-baseline" refer to
Tinker's MuPDF goldens in a different repository, live only in phase 15, and are
not a gate inside tinker-pdf. The only pixel baseline here is the four
fingerprints in `determinism.rs`, whose rule is the opposite: update in the same
commit that caused the change. Every pixel-moving brief states this explicitly,
because the instinct on a red determinism build is to update the expected value,
and here that would destroy the only evidence that the two targets disagree.

**Three plans' fuzz criteria can only run under WSL.** `cargo-fuzz` needs
libFuzzer, which is not supported on `x86_64-pc-windows-msvc`; the repository's
own fuzz job is `runs-on: ubuntu-latest`. Gaps 03, 16, 17 and 24 all inherit
this. WSL2/Ubuntu-24.04 with nightly and `cargo-fuzz 0.13.2` is the local route
and it works.

**One ZIP signature covers five formats, so gap 29's sniff mis-opens gap 30's
input.** *Added while planning [30](30-xps.md), August 2026.* `Document::open`
sniffs `PK\x03\x04` at offset zero and hands the bytes to `cbz::synthesise`,
which is correct for a comic archive and wrong for every other ZIP-shaped
document format. Measured on a real one-page XPS written by Windows' own XPS
serialiser: it **opens**, reports one page, and renders a 4 × 4-point page whose
picture is one of the document's raster resources, with the 816 × 1056 page
dimensions, the text and the fonts discarded and **no warning at all**. A
document with no raster resource is refused as `NoImages` instead — "a valid
archive and not one entry produced a page" — said about a document that has a
page.

This is a property of the set rather than a defect in either plan: gap 29 was
right to sniff the container it was built for, and gap 30 is where the second
container arrives. It is recorded here because it changes an ordering. Gap 30's
milestone 3, which routes by ECMA-388 E.3's three-step test, is the **only** one
of that plan's nine milestones that improves matters on its own, and it is the
part that must land even if the rest of XPS is descoped — so it is early in that
plan's table and its two failing tests are committed by milestone 1.

**Gap 30's milestone 5 must land before its milestones 6 and 7.** The same shape
as PRE-D and as gap 29's `inflate_raw` milestone: the writer cannot emit an
`/ExtGState`, a `/Shading`, a `/Pattern` or a Type0 font, and a build that
reaches XPS's brushes and glyphs without them will approximate opacity and
address glyphs through `/WinAnsiEncoding`. Both approximations render correctly
on every fixture anybody would write by hand and are wrong on real documents,
which is this programme's recurring failure and the reason the ordering is
written down rather than left to whoever picks the plan up.

**The same ZIP signature still mis-opens gap 31's input, and gap 30 did not fix
it.** *Added while planning [31](31-epub.md), August 2026.* Gap 30 closed the
XPS half and named EPUB in the same sentence; nobody went back for it. Measured
on six Project Gutenberg books before a line of gap 31's code: Frankenstein —
thirty-one XHTML content documents and three stylesheets — **opens**, reports
**one page**, and renders a 1824 × 2726-point page that is the auto-generated
cover, with `ArchiveReport::warnings()` **empty**; a book with no image at all is
refused as `NoImages`. An EPUB fails ECMA-388 E.3 at step 2's *first* check —
it carries neither `[Content_Types].xml` nor `_rels/.rels`, measured across nine
files — so gap 30's `UnreadablePackage` is **unreachable** for one and the comic
fallthrough is exactly what E.3's own text asks for. **Gap 30 is not wrong; EPUB
is a different question it did not ask.** Gap 31's milestone 3 is the only one
of that plan's thirteen milestones that improves matters on its own, so it is
early there for the same reason gap 30's was, and it is the part that must land
even if the rest of EPUB is descoped.

**Gap 31 changes a leaf gap 30 froze, and the ordering is one-way.** Ruling 8's
gap 30 amendment says EPUB *"reuses this crate"*, which understates it:
`tinker-pdf-xml` refuses `<!DOCTYPE` outright, and **100 % of Project
Gutenberg's EPUB 2 content documents carry one**, as does the cover wrapper of
every EPUB 3 book. So gap 31's milestone 2 gives that crate a two-valued doctype
mode before its milestone 3 can read a book, and re-asserts all four of gap 30's
committed entity bombs *under the new mode* — because the internal DTD subset is
where all four live, EPUB 3.3 §3.9 forbids exactly that half, and a relaxation
whose defence is not re-proved is a relaxation nobody checked.

**Gap 31's milestone 5 must land before its milestone 8**, which is the third
time this programme has scheduled the writer ahead of its consumers, after
PRE-D and gap 30's milestone 5. `DocumentBuilder` emits **no annotations at
all** — gap 30 named that as a non-goal — and no outline, and an EPUB's
cross-references and navigation document are both. A build that reaches the
first readable book without them renders cross-references as ordinary blue text
and a book with no table of contents, both of which look like the book rather
than like a missing feature.

**The commit-boundary rule is per-plan, not global.** CONTRIBUTING says to treat
each plan's milestone table as the commit boundary set. One commit per gap is
right for gap 21 and gap 22; it is wrong for gap 01 (five milestones, landed as
five commits), gap 03, gap 11, gap 17 and gap 18-A. Gap 16 requires the
opposite — two milestones in one commit, deliberately.

## Order

Prerequisites are marked. Sizes are from [README.md](README.md).

| # | Item | Why here | State |
| --- | --- | --- | --- |
| 1 | [21](21-metadata-absent-vs-empty.md) metadata absent vs empty (S) | Smallest real defect; warm-up | **done** `243d755` |
| 2 | **PRE-A** determinism fixture | Blocks all glyph and pixel work | **done** `9ba9237` |
| 3 | **PRE-C** = [24](24-fuzz-execution.md) M1–M4 | Three later plans' exit criteria need it | **done** `04fb5ae`..`17be6b7` |
| 4 | **PRE-B** = [25](25-wasm-determinism-leg.md) M1–M3 | Build sequence ranks it before all pixel work | **done** `f864b8b`, `1714abf` |
| 5 | [22](22-pdf-version-and-trapped.md) version and `/Trapped` (S) | Same file as 21, still cold | **done** `12959a6` |
| 6 | [01](01-cff-glyph-selection.md) CFF glyph selection (L) | Unblocks 02; the deepest font defect | **done** `81a9da7`..`51f36d3` |
| 7 | [02](02-cid-to-gid.md) CID to GID (M) | Needs 01's `gid_for_cid` | **done** `93a01db`, `125f5e0` |
| 8 | [04](04-usecmap-and-codespaces.md) usecmap (S) | Parser-level; parents become real at 9 | in progress |
| 9 | [03](03-predefined-cmaps.md) predefined CMaps (L) | Makes 04's parents real; needs the `cmap` fuzz target from PRE-C | |
| 10 | [05](05-vertical-metrics.md) vertical metrics (M) | Completes the font lane | |
| 11 | [07](07-stroked-patterns.md) stroked patterns (S) | **All four milestones**, per the finding above | |
| 12 | [06](06-optional-content.md) optional content (M) | Silent wrong output, no dependencies | |
| 13 | [13](13-quadratic-path-verb.md) quadratic verb (S) | Rasteriser lane opens; smallest first | |
| 14 | [14](14-bounded-painting.md) bounded painting (M) | Fingerprints must **not** move | |
| 15 | [12](12-image-sampling.md) image sampling (M) | + a determinism fixture (25 M4) | |
| 16 | [15](15-cancellation.md) cancellation (S) | Closes the rasteriser lane | |
| 17 | [16](16-ccitt-completion.md) CCITT (M) | Decoder flip and rewire in one commit; must expose a row-level T.6 seam for 17 | |
| 18 | [08](08-inline-image-filters.md) inline image filters (S) | **After 16**, per the finding above | |
| 19 | [11](11-transparency-groups.md) transparency groups (L) | Hard constraint: 11 before 09 | |
| 20 | [09](09-tiling-patterns.md) tiling patterns (M) | + 07's tiling half + the stroke-path anchoring test | |
| 21 | [19](19-encrypt-and-linearize.md) encrypt and linearize (M) | Writing lane | |
| 22 | [20](20-linearization-validation.md) linearization validation (S) | Needs qpdf, installed | |
| 23 | [23](23-corpus-runner.md) corpus runner (M) | Hard constraint: 23 before 10 and 17. Needs a `--fonts` flag | |
| 24 | [10](10-mesh-shadings.md) mesh shadings (M) | Scheduled by corpus evidence | |
| 25 | [17](17-jbig2-generic-region.md) JBIG2 (L) | Scheduled by corpus evidence. MQ coder in its own module from commit one | |
| 26 | **PRE-D** JPX option-A plan | 18 has no milestones for the chosen option | |
| 27 | [18](18-jpx-decision.md) JPX decoder, option A (XL) | | |
| 28 | [24](24-fuzz-execution.md) M5 first real campaign | Parsers have finished changing by here | |
| 29 | [25](25-wasm-determinism-leg.md) M4 fixture growth | Owed by 09, 10, 11, 12 | |
| 30 | [26](26-binding-packaging.md) binding packaging (M) | Dry run only; the matrix legs are CI-only | |
| 31 | **PRE-E** `DocumentEditor` transactions | 27 option A cannot be built without it | |
| 32 | [27](27-form-calculations-decision.md) form calculations, option A | Two commits: option B's reader, then the interpreter | |
| 33 | [28](28-tinker-integration-decisions.md) Tinker integration | M4 is code in a different repository | **done** |
| 34 | [29](29-cbz.md) CBZ (S) | The first of the three container plans gap 28's decision spawned. Nothing blocks it; it builds `tinker-pdf-zip`, which 30 needs | **done** `5f46fe3`..`b764917` |
| 35 | [30](30-xps.md) XPS (L) | The second. Needs 29 for `tinker-pdf-zip`, `ImageData::Compressed` and `bounds_ledger.rs`, and needs 09, 10 and 11 because its milestone 5 writes patterns, shadings and groups and the only check on what it writes is that this engine already reads them. **Its own milestone 1 is a corpus of real documents, before any reader** — gap 29 closed owing exactly that, and the ordering is how this one does not | **done** `83d49f3`..`1574767` |
| 36 | [31](31-epub.md) EPUB (XL+) | The third and last. Needs 29 and 30 for everything both built, and **changes** one of them: `tinker-pdf-xml` grows a doctype mode in this plan's milestone 2, because XHTML in the wild carries doctypes and the crate refuses them. The owner chose the **full reflowable engine** on 19 August 2026 over fixed-layout-only and a bounded subset. Its milestone 3 is the only one of thirteen that improves matters alone, for gap 30 milestone 3's reason; its milestone 5 is the writer's missing half again — link annotations and an outline — and lands before its consumers | |

The three container plans sit after 33 because gap 28 is the decision that
spawned them, and they are numbered here rather than left out of the ledger
because the file's own opening — "what order they are being closed in" — stops
being true the moment work happens outside it.

## What has landed

Seven items, 971 tests to 1027. Every commit passed the full seven-command
gate; every claim below was re-verified outside the agent that made it.

| Item | Commits | Tests | What its plan did not say |
| --- | --- | --- | --- |
| 21 | `243d755` | 971 → 978 | The collapsing closure served **eight** fields, not the six named. `/ModDate ()` made a document claim it had never been modified |
| PRE-A | `9ba9237` | 978 | — |
| PRE-C | 8 commits | 978 → 981 | Two live defects: the CFF INDEX off-by-one, and a format 12 `cmap` overflowing a glyph id into a panic. Format 12 is the subtable used past the BMP, so any CJK or emoji face reaches it |
| PRE-B | `f864b8b`, `1714abf` | 981 | The CI job could not distinguish a run from a non-run |
| 22 | `12959a6` | 981 → 988 | `version_string` can no longer return `None`, so it returns `String` |
| 01 | 5 commits | 988 → 1011 | The third code-as-glyph-index site was **correct as written** — a composite font with an identity map needs that fallback. What was missing sat before it: 9.6.6.4 sends a symbolic font's code through the `cmap` as a code |
| 02 | `93a01db`, `125f5e0` | 1011 → 1027 | Gap 01's CIDFontType0 evidence used `Identity-H`, where code == CID, so it could not distinguish a reader that resolves the CID from one that does not. Re-proved with three distinct numbers; it was already correct |

Two techniques from this run are worth reusing. Gap 01's integration tests
assert the **ink bounding box**, which is the only way to tell a right glyph
from a wrong one of the same width. Gap 02 planted a **decoy** `cmap` giving a
different answer, so a test could not pass through a fallback path by accident —
and caught its own first decoy having unsorted format-4 segments, which had made
one test pass for the wrong reason.
