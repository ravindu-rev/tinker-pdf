# The catalog overrides the header unconditionally, and /Trapped is absent

Two small things phase 04 puts in scope, both absent while STATUS calls the
phase complete. A file whose header says 1.7 and whose catalog says 1.4
reports 1.4. A repaired file whose header is unreadable reports no version at
all rather than the baseline. And `/Trapped` — which tells a print workflow
whether the file has been trapped — is not read. When this is done, the
version is the one 7.7.2 describes and `/Trapped` is available. (S)

## What is wrong

**The version.** `crates/tinker-pdf-cos/src/outline.rs`:

```rust
let version = from_catalog.or_else(|| doc.header_version())?;
```

The catalog wins whenever it is present. `docs/plans/04-document-semantics.md`
says it wins **when greater**: the catalog `/Version` exists so an incremental
update can raise the version, not lower it. A file whose header is 1.7 and
whose catalog says `/Version /1.4` is a 1.7 file with a stale or mistaken
catalog entry, and reporting 1.4 misdescribes it.

The function's own doc comment states the unconditional rule, so the
divergence is baked into the documented contract as well as the code.

**The baseline.** The `?` means a repaired file with an unreadable header
reports `None`. Plan 04:

> If repair recovered a document whose header is unreadable, the version
> reports the 1.7 baseline with a provenance warning: guessing low misleads
> more than stating the baseline, and the warning keeps "we guessed" on the
> record.

`WarningKind::HeaderMissing` already exists and nothing in the version path
emits or consults it.

**`/Trapped`.** Absent from `Metadata`'s eight fields, and absent from the
tree entirely outside plan 04 and the audit. It is a *name* — `/True`,
`/False` or `/Unknown` — and the `/Info` field closure only accepts string
objects, so it could not pass through even if it were listed.

## Scope

- Compare the catalog `/Version` against the header and take the greater.
- Fall back to the 1.7 baseline with a provenance warning when the header is
  unreadable.
- A `Trapped` enum — `True`, `False`, `Unknown` — and a `trapped` field on
  `Metadata`, read as a name.
- Correct the version function's doc comment, which currently documents the
  wrong rule.

## Non-goals

- **Version enforcement.** Reporting a version is not the same as refusing
  features above it; the engine reads what is there regardless, which is what
  leniency requires.
- **XMP's `pdf:Trapped`.** XMP is passed through as bytes by design; this is
  the `/Info` key.

## Design

Version comparison is on the numeric pair, not on the string — `1.10` sorts
above `1.9` numerically and below it lexically, and while no such version
exists, comparing strings is the kind of thing that is wrong the moment it
matters.

An unparseable version on either side is treated as absent rather than as
zero, so a malformed catalog entry cannot suppress a good header.

`Trapped` needs its own reader rather than the string closure. `/Unknown` is
also the correct answer for a key present with an unrecognised name, which is
what the enum's third variant is for — not a `None`, because "the file says
something we did not understand" and "the file says nothing" differ in the
same way [21](21-metadata-absent-vs-empty.md) is about.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Greater-of comparison; doc comment corrected | Header 1.7 with catalog 1.4 reports 1.7; header 1.4 with catalog 1.7 reports 1.7; a malformed catalog entry does not suppress the header | S |
| 2 | 1.7 baseline with provenance | A file with no readable header reports 1.7 and emits `HeaderMissing` | S |
| 3 | `Trapped` enum and field | `/Trapped /True` reads as `True`; `/Trapped /Nonsense` reads as `Unknown`; absent reads as `None` | S |

## Dependencies

**Needs first:** nothing. Lands naturally beside
[21](21-metadata-absent-vs-empty.md) — same file, same function, same kind of
contract error.

**Unblocks:** phase 04 being honestly describable as complete, which STATUS
currently claims.

## Risks

| Risk | Mitigation |
| --- | --- |
| Reporting a higher version than before could change a caller's behaviour | It is the correct version; the change is small and the tests state each case explicitly |
| The baseline fallback could mask a genuinely broken file | The warning is the point — the fallback without `HeaderMissing` would be the guess plan 04 warns against |

## As built

*August 2026.* All three milestones are done. Every defect the plan named was
still live at `1714abf`, on the one line it quoted. Seven things the plan did
not say:

1. **The baseline makes the `Option` a lie, so it is gone.** With an
   unreadable header falling back to 1.7, `version_string` can no longer
   return `None` for anything — so it returns `String`, and
   `Document::pdf_version` with it. The plan asked only for the fallback and
   an `Option` that is always `Some` is the shape of a defect this repository
   keeps finding, so the signature followed the rule rather than outliving it.
   Two consumers changed: `tpdf info` dropped its `unwrap_or_else(|| "unknown")`
   — a branch that could never run — and `tinker_parity.rs` asserts the same
   `"PDF 1.7"` against a `String`. Plan 04 carries the sentence.

2. **The baseline is a last resort, not an override.** Header unreadable *and*
   a catalog `/Version /1.4` reports 1.4, not 1.7. The alternative — substitute
   the baseline for the missing header, then take the greater — would discard
   the only version statement the file actually makes in favour of a guess, and
   would warn `HeaderMissing` on a document whose version we did not have to
   guess at. The warning fires only when nothing readable was found on either
   side, which is what makes it mean "we guessed" rather than "there was no
   header" (ruling 10). Both cases are tests.

3. **An unrecognised `/Trapped` name is `Unknown`; a `/Trapped` of the wrong
   *type* is absent.** The plan settles the first. The second follows from
   [21](21-metadata-absent-vs-empty.md), one function above: `/Title 42` is
   `None` because a key of the wrong type states nothing, and `/Trapped (True)`
   is `None` for the same reason. `Unknown` is reserved for the case where the
   file answered in the right shape and the answer was not one of the three.

4. **`Trapped` is not `Option<bool>`.** Table 349's `/Unknown` is a real
   answer as well as the default, so `Option<Trapped>` carries two distinct
   facts — silence and a declared "we do not know" — where `Option<bool>` would
   collapse them into the same `None` this pair of gaps exists to separate.

5. **Nothing else had to follow.** `tinker-pdf` re-exports `Trapped` beside
   `Metadata` (ruling 11); the C ABI, Python, JS and .NET bindings expose only
   the *engine's* version and no document metadata at all, so there was nothing
   to project. The one live consumer is `tpdf info`, which now prints a
   `trapped` line when — and only when — the document holds the key.

6. **No new warning kind.** Ignoring a malformed catalog `/Version` is
   arguably a leniency ruling 10 would want named, and there is no
   `WarningKind` for it. It is deliberately not added: `WarningKind` is
   documented as a closed set whose growth is "a deliberate change to
   documented behaviour", the plan named `HeaderMissing` and only
   `HeaderMissing`, and treating an unreadable version as absent is a reading
   rule rather than a repair. If a corpus run ([23](23-corpus-runner.md)) shows
   files that lose a version this way, that is the evidence for adding one.

7. **A naive test would have passed against the broken code.** The obvious
   fixture is a header and a catalog that agree, or a catalog with no
   `/Version` at all — both of which the old line answered correctly, and both
   of which are what every file in `testdata/` looks like. The existing
   `the_version_string_matches_the_header` is exactly that test and was green
   throughout. Only a *disagreeing* pair separates "the catalog wins" from "the
   later wins", and only a pair disagreeing in the tenths place — 1.10 against
   1.9 — separates numeric comparison from string comparison. Both are among
   the seven new tests, which are hand-built byte by byte beside the code in
   `outline.rs`: header over catalog, catalog over header, equal, catalog
   absent, `1.10` beating `1.9` in both orders, `2.0` over `1.7`,
   `/Version /banana` and `/Version 42` failing to suppress a good header,
   `%PDF-banana` deferring to a good catalog, `%PDF-1.4.2` as not-a-version,
   the baseline with its warning counted before and after, a readable version
   warning about nothing, the three `/Trapped` names, `/Trapped /Nonsense` as
   `Unknown`, and `/Trapped` as a string, an integer and a null as absent.

Seven tests added, 981 → 988. Version *enforcement* remains a non-goal and
nothing gates on the reported version; XMP's `pdf:Trapped` is still passed
through as bytes, unread, by design.
