# A composite font's CID never reaches its outline

The CID is computed, and then used only to look up a width. The raw character
code goes to the glyph lookup instead. So a CJK document lays out at exactly
the right spacing and draws the wrong characters — which reads as a font
problem rather than an engine one. When this is done, the CID that decides the
advance also decides the glyph. (M)

## What is wrong

`CMap::cid` (`crates/tinker-pdf-font/src/cmap.rs`) has one non-test caller in
the entire workspace: `width_of` in `crates/tinker-pdf-cos/src/font.rs`. Every
other consumer sees only `DecodedCode`, which carries the raw `code` and no
CID at all.

`extract_outline` in `crates/tinker-pdf/src/resources.rs` therefore hands the
code to the glyph lookup for every font type. For a simple font that is
correct — the code *is* what the encoding maps. For a Type 0 font it is not:
the code is a byte sequence that a CMap turns into a CID, and the CID is what
selects the glyph.

`/CIDToGIDMap` is read nowhere. For a CIDFontType2 (TrueType-backed) font it
is required to be either `/Identity` or a stream of two-byte GIDs, and the
stream form is common in subsetted fonts.

## Scope

- Carry the CID on `DecodedCode` alongside the code, so a caller that has one
  has the other.
- Read `/CIDToGIDMap` from the descendant font: `/Identity` or a stream, with
  the stream indexed by CID and yielding a GID.
- In `extract_outline`, use CID → GID for a composite font and leave the
  simple-font path alone.
- CIDFontType0 (CFF-backed): the CID goes through the CFF charset, which
  [01](01-cff-glyph-selection.md) builds.
- CIDFontType2 (TrueType-backed): the CID goes through `/CIDToGIDMap`, or is
  the GID when that is `/Identity`.

## Non-goals

- **The CMap tables themselves.** Whether the code → CID mapping is *correct*
  is [03](03-predefined-cmaps.md); this is about the CID reaching the glyph
  once it has been computed.
- **Vertical advances.** [05](05-vertical-metrics.md).

## Design

`DecodedCode` gains a `cid: u32` field, set by `Font::decode` from the
encoding CMap — which is already consulted there to split the string into
codes, so the CID costs one more call on a lookup that has already located the
range.

For an Identity CMap the CID equals the code, so nothing changes for the
overwhelmingly common `Identity-H` case. That is worth stating because it
means this fix cannot regress the files that work today: it can only change
behaviour where a non-identity CMap is in play, which is exactly where the
behaviour is currently wrong.

`/CIDToGIDMap` as a stream is `2 × cid` into the decoded bytes. Out of range
is GID 0 — `.notdef` — not a panic and not a wrap.

## Where a half-implementation is worse than none

Carrying the CID but leaving `/CIDToGIDMap` unread. A subsetted CIDFontType2
almost always has a non-identity map, so a CID would then index a GID table
that is not the identity and draw a *different* wrong glyph. Both halves land
together or neither does.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `DecodedCode.cid`, populated from the encoding CMap | An `Identity-H` fixture has `cid == code` for every code, so nothing moves | S |
| 2 | `/CIDToGIDMap` read, both forms | A fixture with a non-identity map draws the glyph the map names; an out-of-range CID draws `.notdef` | S |
| 3 | `extract_outline` uses the CID for composite fonts | A CIDFontType2 fixture with a non-identity CMap and a non-identity GID map renders the right glyphs at the right widths | M |

## Dependencies

**Needs first:** [01](01-cff-glyph-selection.md) for the CIDFontType0 half —
the CFF charset is what turns a CID into a GID there. The CIDFontType2 half
has no dependency and can land first.

**Unblocks:** any real CJK rendering. Also makes [03](03-predefined-cmaps.md)
worth doing: correct CMap tables are pointless while the CID they produce is
discarded.

## Risks

| Risk | Mitigation |
| --- | --- |
| No CJK fixture exists anywhere in the tree | Hand-build a CIDFontType2 with a two-entry `/CIDToGIDMap`; the point is that CID ≠ code ≠ GID, which needs three distinct small numbers rather than a real font |
| Widths and glyphs could silently diverge again if a later change reads one and not the other | Assert both in the same test from the same fixture, so a divergence fails rather than drifts |
