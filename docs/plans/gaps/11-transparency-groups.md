# Transparency groups and soft masks do not exist

A drop shadow, a knocked-out logo, a group faded as a unit — all of clause 11
beyond constant alpha and blend modes. `/Group` appears nowhere in the tree
and ExtGState `/SMask` is never parsed, so a masked group paints unmasked and
a group with its own alpha paints each element at full strength. When this is
done, a transparency group composites as a unit and a soft mask shapes what it
covers. (L)

## What is wrong

Two things are already right and should not be re-planned. Constant alpha
(`/CA`, `/ca`) works end to end. Blend modes — all sixteen — work, and the
`Canvas` alpha convention was corrected to straight alpha with the
source-over formula from 11.3.6, so compositing onto a transparent buffer is
already correct.

What is absent:

- `/Group` on a form XObject: never read. No `/S /Transparency`, no `/I`
  (isolated), no `/K` (knockout), no group colour space.
- ExtGState `/SMask`: never parsed. Neither `/Luminosity` nor `/Alpha`,
  neither `/BC` nor `/TR`.
- `Canvas::composite` does not exist. A `Canvas` can be rendered into but not
  blitted onto another.
- The `Device` trait has no group or soft-mask events.

## Scope

- `Canvas::composite(&mut self, src, at, alpha, mode, mask)` — the missing
  primitive.
- `Canvas::to_mask(kind, transfer)` producing a `Mask` from a rendered group,
  for `/Luminosity` and `/Alpha`.
- `Device` gains defaulted `begin_group`/`end_group` and
  `begin_soft_mask`/`end_soft_mask`/`clear_soft_mask`, so the text and
  recording devices are unaffected.
- The interpreter drives them: `/Group` on a form around its content, and
  `gs` with an `/SMask` rendering the mask group before the content it masks.
- Isolation and knockout.
- `/TR` as a pre-sampled 256-entry lookup table.

## Non-goals

- **Blend modes.** Done.
- **Spot colours through a group colour space.** The group's `/CS` matters for
  correctness of blending in a non-RGB space; the engine composites in RGB
  throughout and that is a separate decision.
- **Full ICC.** Same reason, and plan 08 already defers it.

## Design

**The alpha convention is settled**, which is what makes this tractable: a
group buffer starts fully transparent, and compositing onto it already
produces straight-alpha results rather than premultiplied ones.

**The CTM for a mask group is the one in force at the `gs` operator**, not at
paint time (11.6.5.2). Getting this wrong is subtle and looks like a slightly
misplaced shadow rather than a bug.

**The soft mask lives on the device, not the graphics state.** It is pixels,
and the graphics state is PDF-level (ruling 8). Follow the clip-stack
precedent: `save_state`/`restore_state` push and pop it, so an `/SMask` set
inside `q … Q` does not survive the `Q`.

**Knockout** composites each element against the group's *initial* backdrop
rather than against the accumulated result, so the frame needs to keep that
initial copy.

**Backdrop removal** for a non-isolated group: the backdrop is composited in,
then removed again after the group is blended, or the backdrop is counted
twice.

## Where a half-implementation is worse than none

Two traps, both of which render plausibly:

**A `/Luminosity` mask with no `/BC` defaults to black**, meaning fully
masked. Defaulting to white or to transparent inverts every drop shadow in
the corpus and still looks reasonable on a light page — the shadow appears
where the light should be, which reads as a design choice.

**Groups without backdrop removal or knockout** render *darker*, not broken.
A double-counted backdrop is a plausible image. Claiming group support while
getting this wrong is worse than the honest current behaviour, because the
error is distributed across every group in the document rather than
concentrated in a missing feature.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `Canvas::composite`, bounded to the source rectangle | A small buffer blitted onto a page matches the same content drawn directly; compositing onto a transparent buffer keeps straight-alpha colour | S |
| 2 | `Device` group events; `/Group` read; non-isolated, non-knockout groups | A group with `/ca 0.5` fades as a unit — two overlapping opaque shapes inside it do not show the seam that per-element alpha would produce | M |
| 3 | Isolation | An isolated group ignores its backdrop; a non-isolated one does not, with backdrop removal so it is not counted twice | M |
| 4 | Knockout | Each element composites against the initial backdrop; overlapping elements in a knockout group do not accumulate | M |
| 5 | ExtGState `/SMask`, `/Luminosity` and `/Alpha`, `/BC`, `/TR` | A luminosity mask with no `/BC` masks fully where the group is absent; a transfer function shifts the mask measurably | L |
| 6 | Interaction with the clip and the existing blend modes | A soft mask set inside `q … Q` does not survive the `Q`; a masked group under `Multiply` blends and masks | S |

## Dependencies

**Needs first:** nothing — the alpha convention and blend modes landed.

**Unblocks:** [09](09-tiling-patterns.md), which needs `Canvas::composite` to
rasterise a tile once and blit it rather than replaying it per lattice
position.

## Risks

| Risk | Mitigation |
| --- | --- |
| Group buffers are page-sized, and nesting them multiplies memory | Bound each buffer to the group's `/BBox` in device space rather than to the page; cap nesting with the existing form-recursion depth |
| The `/BC` default is easy to get wrong and looks plausible either way | A fixture with a luminosity mask and no `/BC`, asserting the masked region is *hidden*; it is the first test to write |
| Every existing golden moves once groups composite | Land it before any golden re-baseline, per the ordering rule in [16-build-sequence.md](../16-build-sequence.md) |
