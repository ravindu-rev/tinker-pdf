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
