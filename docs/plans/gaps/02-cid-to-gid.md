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

> *Amended August 2026, before the work started.* That count was true when it
> was written and had been overtaken by [01](01-cff-glyph-selection.md) by the
> time anyone read it. `Font::cid_of` now wraps `CMap::cid`, `width_of` calls
> it, and so does `cff_glyph` in `resources.rs` — so the CFF half of the
> sentence below was already fixed, and only the TrueType half was still live.
> The count is left standing because it is what the gap was commissioned
> against; see `As built`.

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
| A fixture whose face carries no `cmap` cannot tell "the CID was used" from "the character lookup found nothing and fell through to the code" | *Added August 2026.* The face carries a **decoy** `cmap` that answers a different glyph, and every composite assertion is that the decoy's answer is *not* what drew. Without it the four composite tests still pass with the CID discarded |

## As built

*August 2026.* All three milestones are done, in two commits plus this one.
Everything in Scope is implemented and both Non-goals held. Seven things the
plan did not say.

1. **The CIDFontType0 half was already done, and this checked rather than
   assumed it.** [01](01-cff-glyph-selection.md)'s `As built` says so; the
   claim survives inspection — `cff_glyph` resolves `font.cid_of(code)`
   through the charset for a CID-keyed program and uses the CID as the glyph
   index for a plain CFF (9.7.4.2) — but the test behind it used
   `Identity-H`, where the code and the CID are the same number, so it could
   not distinguish a reader that resolves the CID from one that hands the
   code straight to the charset. `a_cid_keyed_program_follows_a_non_identity_cmap`
   is that distinction: code 0x41, CID 11, glyph 2, three numbers. It passes
   without any change to the CFF path. **Nothing was owed there.** This gap's
   code is entirely the CIDFontType2 half.

2. **The TrueType site was not "the code goes to the glyph lookup".** The
   plan describes a raw code being used as a glyph index. What was actually
   there — after 01 rewrote the function — sent the code through
   `/ToUnicode` to a *character* and the character through the font program's
   own `cmap`, and only fell back to the code when that found nothing. So the
   defect had a second shape the plan does not describe: a composite font
   whose embedded face still carries a `cmap` drew whatever glyph that table
   named for the character, which for a subsetted face describes the
   *original* glyph numbering and not the one in the file. A `cmap` answers
   "which glyph draws this character", and a composite font's code is not a
   character; 9.7.4.2 does not consult it at all, and neither does this
   engine now.

   This is the one case where a file that opened before renders differently,
   and it renders correctly.
   `an_identity_h_face_with_a_cmap_now_follows_the_cid_not_the_character`
   pins it rather than leaving it to be discovered.

3. **The last resort still fires, and now only where it should.** 01 left
   `.or_else(|| u16::try_from(code).ok())` in place with a comment saying it
   is correct for a composite font with an identity `/CIDToGIDMap`. That
   reading was right only while every composite font's CMap was also the
   identity — which is all this engine could produce, since the CID was
   discarded. The composite branch now sits ahead of it and indexes by CID:
   the same number wherever the old comment was right, the right one wherever
   it was not. The line's remaining job is the one 01 also named — a subset
   font whose `cmap` a producer dropped, where the code is the only glyph
   number on offer — and `a_simple_font_with_no_cmap_still_falls_back_to_the_code`
   holds it there.

4. **`.notdef` from a CID is reported, which the plan's exit criterion does
   not ask for.** Milestone 2 asks that an out-of-range CID draw `.notdef`.
   Drawing it is not enough: 01's third finding was that `.notdef` is an
   *outline*, usually empty, which the renderer reads as a legitimate space —
   so a page full of unresolvable CIDs is indistinguishable from a page of
   spaces. A CID that resolves to glyph 0 when the CID is not itself 0 now
   goes through the same `report_unresolved_glyph` the CFF path uses, and
   surfaces as `UnreadableFont` (ruling 10).

5. **An unreadable `/CIDToGIDMap` degrades to the identity, not to nothing.**
   The plan says out of range is `.notdef` and says nothing about a map that
   will not decode at all. A font drawing its glyphs in order is recoverable
   by a reader; one drawing them by an unknown permutation is not, and
   `/Identity` is what the entry means when absent. So a name, a number, an
   array, a dangling reference and an undecodable stream all mean the
   identity — and only a stream that decodes becomes a table.

6. **The fixture needed a decoy, which the risk table now records.** A
   hand-built face with no `cmap` proves nothing about which table was
   consulted, because the character lookup would find nothing and fall
   through to the code anyway. The face here maps `A` to glyph 2, the
   900-unit box, and every composite assertion is that a *different* box
   drew. Measured: with the fix reverted, five of the nine tests in
   `composite_fonts.rs` fail and the three that assert nothing-moved pass on
   both sides, which is what those three are for.

7. **The corpus had no composite font at all.** `/CIDToGIDMap` is a
   document-controlled table indexed once per glyph, and no fuzz target could
   reach the code that indexes it — `render_page`'s seeds are the
   `cos_document` PDFs, none of which has a Type 0 font. One seed is added,
   written by an `#[ignore]`d test from the fixture itself so the two cannot
   drift, in the style 01 established for `cff`.

**Ruling 4:** no determinism fingerprint moved, on native Windows or on
`wasm32-wasip1` under wasmtime 47.0.3. Expected — that fixture's `text` page
embeds a *simple* TrueType font and reaches none of this — and checked rather
than assumed, because the TrueType branch changed.

**For [03](03-predefined-cmaps.md):** unblocked. Its premise was that correct
CMap tables are pointless while the CID they produce is discarded; the CID now
decides the glyph as well as the advance, so a better `90ms-RKSJ-H` table
would change what draws and not only how far apart it draws. What 03 still
owns is unchanged: the fourteen registry prefixes are still a blanket two-byte
codespace with code-as-CID, which mis-splits and mis-maps, and `is_approximate`
still has no callers.

**Test count:** 1011 -> 1027, plus one `#[ignore]`d seed writer.
