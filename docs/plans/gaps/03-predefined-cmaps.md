# Predefined CMaps are identity stubs

A document using `90ms-RKSJ-H` — Shift-JIS, and one of the commonest Japanese
encodings in circulation — has its string split at the wrong byte boundaries
and then maps each fragment to itself as a CID. Both the glyphs and the
*advances* come out wrong, and nothing is reported. When this is done, the
Adobe registry CMaps map codes to CIDs the way the registry says. (L)

## What is wrong

`CMap::predefined` (`crates/tinker-pdf-font/src/cmap.rs`) returns a real CMap
for exactly two names, `Identity-H` and `Identity-V`. Everything else is
matched against a fourteen-entry prefix list:

```rust
const CJK_PREFIXES: [&[u8]; 14] = [
    b"UniJIS", b"UniGB", b"UniCNS", b"UniKS", b"GBK-", b"GB-", b"ETen", b"90ms", b"90pv",
    b"B5pc", b"KSC", b"Add-", b"Ext-", b"RKSJ",
];
```

and a match produces a stub: one blanket two-byte codespace `0x0000..=0xFFFF`,
and `identity: true`, which makes `CMap::cid` return the code unchanged.

Three things follow.

**The codespace is wrong for every mixed-width encoding.** RKSJ, GBK, Big5 and
the EUC families all have one-byte *and* two-byte ranges. A blanket two-byte
codespace mis-splits the string before any mapping happens.

**Mis-splitting makes the widths wrong too.** The CID feeds `/W` lookup in
`crates/tinker-pdf-cos/src/font.rs`, so a wrong CID is a wrong advance — not
merely a wrong glyph. The doc comment in `cmap.rs` claims the opposite ("only
the CID mapping is approximate"); it does not hold.

**The degradation is silent.** `is_approximate` exists, is public, and has no
callers anywhere. Nothing stores it on `Font`, and `read_encoding` has no
warning sink at all. Ruling 10 says warnings carry provenance; this one
carries nothing.

Matching is `starts_with`, so `b"RKSJ"` is dead weight — no registry name
*begins* with it — while `83pv-RKSJ-H`, `78-RKSJ-H`, `Hankaku`, `Roman`,
`HKscs-B5-H` and the `UniHojo-*` family match nothing and return `None`, after
which a Type 0 string is split one byte per code.

## Scope

- Vendor Adobe's `cmap-resources` (BSD-3-Clause) and compile it at build time
  into static tables, exactly as `docs/plans/05-fonts.md` already specifies.
- A `build.rs` for `tinker-pdf-font` — the crate has none, and neither does
  any crate in the workspace.
- The `cmap-predefined` feature gate named in plan 05, default on, with
  `Identity-H`/`-V` always compiled in regardless.
- Correct codespace ranges per CMap, so strings split correctly.
- Surface approximation: store it on `Font`, and emit a warning naming the
  CMap when a document uses one this build does not have.
- `THIRDPARTY.md` at the repo root with the licence texts. It does not exist.

## Non-goals

- **Every CMap in the registry, if size forbids.** Plan 05 estimates ~10 MB of
  source compiling to low single-digit megabytes. If the wasm budget cannot
  take it, ship the common subset and warn precisely for the rest — a named
  gap beats a silent guess.
- **Embedded CMap streams.** Already parsed; their inheritance is
  [04](04-usecmap-and-codespaces.md).

## Design

**Encoding.** Delta-encoded ranges, deflated with the engine's own filter code
so no third-party crate enters the dependency graph (ruling 3). Plan 05
specifies this; it has not been built.

**Feature gating.** `cmap-predefined` on by default. With it off, the two
identity CMaps still work and every other name reports a named gap rather than
guessing. The wasm size delta gets measured and written back into plan 05,
which reserves a place for the number.

**The warning matters as much as the tables.** Whatever subset ships, a
document using a CMap outside it must say so. A wrong CID is invisible; a
warning naming `90ms-RKSJ-H` is actionable.

## Where a half-implementation is worse than none

Keeping the stub as a fallback once real tables exist. The stub's failure mode
is silent and plausible — text that lays out and reads as gibberish looks like
a missing font, so nobody files it against the CMap. Once there is a real
path, an unknown CMap must warn and refuse to guess, not fall back to
identity.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Vendored data, `THIRDPARTY.md`, licence check taught the code/data distinction | `cargo deny check licenses` green with the data present | S |
| 2 | `build.rs` compiling ranges into static tables, deflated with our own encoder | A fixture maps `90ms-RKSJ-H` codes to the CIDs the registry lists | M |
| 3 | Correct codespaces; mixed-width splitting | A one-byte and a two-byte code in the same string split correctly, verified against the registry's own codespace declarations | M |
| 4 | `cmap-predefined` feature; measured wasm delta recorded in plan 05 | Build with the feature off shrinks by a recorded number; identity CMaps still work | S |
| 5 | Approximation surfaced | A document using an unshipped CMap produces a warning naming it; `is_approximate` has a caller | S |
| 6 | `cmap` fuzz target — plan 05 names it and it does not exist | `cargo fuzz run cmap` survives a session | S |

## Dependencies

**Needs first:** [02](02-cid-to-gid.md). Correct CIDs are wasted while the CID
never reaches the glyph.

**Unblocks:** CJK rendering at all, and the corpus pass rate on any collection
with Japanese or Chinese documents.

## Risks

| Risk | Mitigation |
| --- | --- |
| ~10 MB of source data blows the wasm budget | The feature gate, own-encoder deflation, and a measured delta recorded rather than assumed. Plan 05 already carries this risk row; this is where the number gets filled in |
| Data licensing mishandled in an MIT/Apache repository | Vendored with licence texts in `THIRDPARTY.md`; build-time compilation keeps raw data out of the published crate; the CI licence check is taught that data and code differ |
| A generated table is wrong in a way no test notices | Exit criteria are against the registry's own published mappings, not against our own output |
