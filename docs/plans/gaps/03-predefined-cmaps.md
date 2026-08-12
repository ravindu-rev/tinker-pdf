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

## As built

*August 2026.* All six milestones are done, in six commits. Everything in
Scope is implemented; both Non-goals held, and the first of them — "every CMap
in the registry, if size forbids" — turned out not to bind, because the
measurement said so. Nine things the plan did not say.

**1. The whole registry ships. The subset question was answered by a
number.** Adobe's `cmap-resources` is 7.5 MB of PostScript across 202 CMaps,
which the plan estimated at "~10 MB ... compiling to low single-digit
megabytes". Compiled it is **1.19 MB**, and the wasm delta measured on the
`bindings/js` cdylib is 1 251 641 bytes raw and 1 149 630 gzipped. At that
price a common subset would have had to drop the Unicode-keyed CMaps —
`UniGB-UCS2-H` and its family are three quarters of the bytes and what modern
producers actually emit — to save a megabyte from a build that can turn the
whole thing off with a feature flag. All of it ships. The figures are recorded
in [plan 05](../05-fonts.md), which reserved a place for them.

Two things fell out of the measurement. The raw wasm delta is within 139 bytes
of the compiled blob, so nothing else grew. And the *gzipped* delta is 92 % of
the raw one: deflating at build time is not redundant with transport
compression, it is the reason the transport has nothing left to do.

**2. `decode_codes` had two defects the plan does not describe, and they are
the half that made the advances wrong.** The plan says the blanket codespace
"mis-splits the string before any mapping happens", which is true, and stops
there. Underneath it:

- **9.7.6.2's bounds are per byte and were compared as one integer interval.**
  `<8140> <9FFC>` is not the range 0x8140..0x9FFC; it is "first byte in
  0x81..0x9F *and* second in 0x40..0xFC". Where this stops being pedantry is
  `GBK2K-H`, whose two-byte and four-byte codespaces have completely
  overlapping lead bytes and are separated by nothing but the second byte —
  0x40..0xFE against 0x30..0x39. Compared as integers, `82 35 87 39` reads as
  two two-byte codes; it is one four-byte GB18030 code, and the registry says
  which: `<82358739> 30366`.
- **9.7.6.3 gives an undefined code a length, from its first byte.** The old
  comment claimed the opposite — "consumed one byte at a time, which is what
  9.7.6.3 prescribes". Consuming one byte re-reads a *trail* byte as a lead
  byte, so `81 30 41` came out as three codes, the middle one a perfectly good
  one-byte code for CID 247. One bad byte pair grew a glyph the file never
  contained.

**3. `notdefrange` is read by nothing, deliberately.** `90ms-RKSJ-H` opens
`1 beginnotdefrange <00> <1f> 231`, and 231 is the same CID `<20>` maps to —
the collection's space. Honouring it would replace "this code means nothing"
with an answer indistinguishable from a real one, which is the failure this
gap exists to stop. `a_notdefrange_maps_nothing` pins the decision.

**4. The registry's own `usecmap` is what makes a megabyte affordable.** 80 of
the 202 CMaps inherit, at most two links deep, and every parent resolves —
`build.rs` fails the build otherwise. `90ms-RKSJ-V` is 78 `cidrange`s over
`90ms-RKSJ-H` rather than a second copy of it. This is gap 04's machinery
being used for the first time on a real parent, and the registry supplies its
own decoy for testing it: both files map `<8141>`, to 7887 and to 634.

**5. Sorted tables needed a second field on `CMap`, not more of the first
one.** A CMap's own `cidrange`s are in file order and the first match wins; a
compiled table is sorted and disjoint and can be binary-searched.
`UniCNS-UTF8-H` is 18 568 ranges after `cidchar` entries are folded in, and
scanning that per character is tens of milliseconds a page. `cid_sorted` is a
*list* of sorted blocks so a chain keeps first-match-wins across the merge.
`build.rs` asserts disjointness over all 202 CMaps rather than assuming it,
because a `cidchar` inside its own CMap's `cidrange` would make the sort
silently change which one answers. There are none; the assertion is what says
so on every build.

**6. `tinker-pdf-font` now depends on `tinker-pdf-filters`, and the plan's
"the leaf crate itself depends on nothing" is amended.** Plan 05's own
dependency section always said the phase "needs ... the filters phase
(FlateDecode for FontFile2/3 **and for the bundled-asset pipeline**)"; this is
that edge arriving. Ruling 3 is untouched — a sibling workspace crate
implementing the engine's own deflate is not a third-party dependency — and
there is no cycle to create, because `filters` depends on nothing, which was
checked before committing to it since a build-dependency cycle is a hard cargo
error. The runtime edge is optional and disappears with `cmap-predefined`; the
build-time edge is unconditional, because a build script never reaches a
binary and gating it would stop `build.rs` compiling with the feature off.
`cargo xtask dag` counts a `[build-dependencies]` entry as an edge and had to
be taught this one by name.

**7. `cargo deny check licenses` was green before this gap and after it, with
the data present, because it never had an opinion about the data.** Milestone
1's exit criterion is satisfiable by a check that cannot see the thing it is
checking: cargo-deny reads the *crate* graph, and 7.5 MB of BSD-3-Clause text
inside a crate declaring `MIT OR Apache-2.0` is not in that graph. So the
milestone grew a second half. `cargo xtask vendor` requires every
`crates/*/data/*` tree to be declared in `THIRDPARTY.md`, to carry its own
licence file, and to declare an SPDX identifier `deny.toml` already allows —
which is what "the licence check taught the code/data distinction" has to mean
if it is to mean anything.

**8. `deny.toml` allowed `OFL-1.1` for bundled fonts that do not exist.** The
comment read "bundled substitute fonts (Liberation family), an asset not
code". There are none, and there never have been:
`crates/tinker-pdf/tests/substitute_fonts.rs` synthesises its face on the spot
so that "the repository carries no font anyone has to licence". An allowlist
entry describing an asset nobody ships reads as evidence that faces are
shipped, which is the exact false impression the engine's largest remaining
font gap turns on. Removed, with the reasoning kept as a comment so it is not
re-added speculatively — and with a test pinning that a *commented* entry does
not allow, since `cargo xtask vendor` reads the same list.

**9. Two cargo behaviours make a feature look off while it is on, and both
are silent.** `default-features = false` written by a workspace *member* is
ignored unless the `[workspace.dependencies]` entry says so too — all four
manifests had it, the feature stayed on, and `cargo tree -e features` was the
only thing that said so. And `cargo test --workspace --no-default-features`
cannot turn this feature off at all, because `tools/` and `tinker-pdf-ffi`
depend on the facade with its defaults and cargo unifies features across
everything in one build: the command passes and proves nothing. The gate leg
is per package, in CI and in CONTRIBUTING.

### What a build without `cmap-predefined` does

Not nothing, and this is a deliberate line rather than an accident. Names,
writing modes, `usecmap` parents and codespace ranges are a separate **4.6 KB**
table that is always compiled in. So a build with the feature off still splits
`41 81 40 B1 E0 40` into four codes at the right widths — which is what keeps
the advances right — and answers `None` for every CID rather than substituting
identity. `is_approximate` is true and the document says which CMap.

Returning `None` outright was rejected: it re-creates this gap's own defect
inside the feature-off build, because a font with no CMap falls back to one
byte per code. Four kilobytes buys the half of a CMap that is a published fact
about byte boundaries; the half that costs a megabyte is the lookup table, and
that is the half the feature removes.

### Evidence, and why the obvious test would not have been any

The risk table's third row is "a generated table is wrong in a way no test
notices", mitigated by testing "against the registry's own published mappings,
not against our own output". A round trip through `build.rs`'s encoder and
back proves the encoder agrees with itself; an off-by-one in the delta scheme
survives it untouched. So the two load-bearing tests re-read Adobe's text with
a **second, deliberately different parser** — line-oriented, no tokenizer, no
sections, no deflate — and compare:

- `every_declared_mapping_survives_the_build`: both bounds of all **402 816**
  `cidrange` and `cidchar` entries the 202 vendored files declare. 805 632
  assertions.
- `every_declared_codespace_splits_at_its_own_width`: both bounds of all 237
  declared codespace ranges, fed back through `decode_codes` and required to
  return as one code of the width Adobe wrote it at. 474 assertions.

Everything else quotes the registry into the test source with the arithmetic
done by hand, so a reviewer can check a claim without running anything.

The pixel tests exist because gap 02 made them possible, and they are built so
that a wrong answer is *blank* rather than different: `/CIDToGIDMap` is a
stream naming four CIDs and nothing else, so any mis-split draws nothing.
`the_blanket_two_byte_codespace_draws_nothing` renders the defect and asserts
the blank page, so the tests above it measure a change rather than a constant.

That fixture also shows why milestone 5 was still owed after milestone 3. The
blank page *does* warn — `UnreadableFont`, from gap 02, because a stream
`/CIDToGIDMap` can tell a CID ran off its end. It says *this font has no glyph
for that CID*. It does not say *that CID came from a CMap nobody wrote*, and
on a real CJK document, where the font has a glyph for almost every CID, it
would not fire at all.

### Milestone 6: the first real `cmap` campaign

The target existed — [PRE-C](00-execution-order.md) created it and gap 04
extended it — but neither ran a session, so its seeds were reachable by
construction rather than by measurement. Four seeds were added first, because
until this gap landed there was no table for a seed to reach: each names a
real registry CMap on the first line, where the target looks for one.

Run under WSL2 / Ubuntu-24.04, nightly + cargo-fuzz 0.13.2, since libFuzzer
does not support `x86_64-pc-windows-msvc`:

```
cargo fuzz run cmap fuzz/corpus/cmap -- \
  -max_total_time=900 -timeout=25 -rss_limit_mb=4096 -jobs=4 -workers=4
```

**623 895 executions** over 906 seconds on four workers — a little over an
hour of CPU. **No crash, no hang, no leak**: slowest unit 0 s, peak RSS 505 MB,
and `fuzz/artifacts/cmap` empty. 11 337 new units across the four workers, and
the corpus was reset afterwards, since plan 14 commits minimised seeds and
merging a grown corpus is a human's decision.

Coverage over the grown corpus, `llvm-cov` against the instrumented build:

| File | Lines | Functions |
| --- | --- | --- |
| `tinker-pdf-font/src/predefined.rs` | 97.12 % | 100 % |
| `tinker-pdf-font/src/cmap.rs` | 96.96 % | 96.49 % |
| `tinker-pdf-filters/src/inflate.rs` | 62.19 % | 88.89 % |

Every function this gap added is at 100 % line coverage —
`CodespaceRange::contains`, `covers_lead` and `byte`, `CMap::predefined`,
`from_registry`, `from_entry`, `next_code`, `inherit_from` — and
`predefined::ranges`, which is the inflate-and-decode path, is at 93.10 %.
`inflate.rs` appearing at all is the direct evidence: the only way this target
reaches it is by decompressing a compiled CMap table.

The most persuasive line is not in the table. libFuzzer's recommended
dictionary at the end of the run contains `78-V`, `78ms-RKSJ-V` and
`Identity-V` — registry names it *derived* from comparison feedback against
the name table. Nothing in the corpus contained `78ms-RKSJ-V`.

**Ruling 4:** no determinism fingerprint moved, on native Windows or on
`wasm32-wasip1` under wasmtime 47.0.3, at any of the six commits. Expected —
the `text` fixture embeds a simple TrueType face and names no CMap — and
checked each time, because every commit here changes a path `Font::read`
reaches.

**Test count:** 1060 → 1093, plus four that exist only with `cmap-predefined`
off.
