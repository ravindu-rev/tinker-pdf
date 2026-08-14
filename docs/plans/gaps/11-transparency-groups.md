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
| ~~Every existing golden moves once groups composite~~ **Corrected, August 2026, on building it.** There are no golden images in this repository, and the mitigation named a rule that does not apply here. [16-build-sequence.md](../16-build-sequence.md)'s "re-baseline once" refers to Tinker's MuPDF goldens in a *different* repository, live only in phase 15. The only pixel baseline here is `determinism.rs`, whose rule is the opposite: **update in the same commit that caused the change, naming which fixture moved and why.** Measured rather than assumed: none of the seven existing fingerprints moved at all, because not one of those fixtures had a `/Group` or an ExtGState `/SMask` — which is why this gap owed an eighth | Nothing to re-baseline. The eighth fingerprint is the mitigation, and it is a *new* one rather than a moved one |

---

## As built — August 2026

All six milestones, six commits. 1243 tests to 1276; the full gate green on
every one, including the `wasm32-wasip1` determinism leg under wasmtime
47.0.3.

| # | Commit | What landed |
| --- | --- | --- |
| 1 | `94e74ef` | `Canvas::composite`, `extract`, `adopt_backdrop`, `remove_backdrop`, `snapshot`, `to_mask`; `Mask::uniform`, `Mask::overwrite` |
| 2 | `d269777` | `Device::begin_group`/`end_group`, `/Group` read, 11.6.6's state reset, the renderer's group buffers |
| 3 | `dbd348b` | `/I`, and 11.4.7.2's backdrop removal |
| 4 | `8abba98` | `/K`, and the restore at every element |
| 5 | `48d1e6a` | ExtGState `/SMask`: `/Luminosity`, `/Alpha`, `/BC`, `/TR` |
| 6 | `dc67f25` | The clip stack, the blend modes, and the eighth determinism fingerprint |

### `Canvas::composite`'s final signature — for gap 09

```rust
pub fn composite(
    &mut self,
    src: &Canvas,
    at: (i32, i32),
    alpha: f64,
    mode: BlendMode,
    mask: Option<&Mask>,
    stop: Option<&dyn Fn() -> bool>,
)
```

Exactly the shape the Scope section asked for, plus the cancellation hook that
gap 15 made the convention for anything that walks pixels.

What a tile-blitting caller needs to know:

- **The walk is the source's rectangle**, clipped to the destination. A 40x40
  tile costs 1600 pixels on A4 rather than the page, so blitting per lattice
  position is linear in the lattice and not in the paper.
- `at` is the source's top-left in the destination's device pixels, and may be
  negative or past the far edge; a tile entirely off the canvas costs nothing
  and cannot overflow — the placement arithmetic is `i64` throughout.
- `mask` is in the **destination's** coordinates, which is what a tiling
  pattern wants: the filled path's coverage is the mask, and it does not move
  as the tile does.
- The source's own alpha decides its shape. A tile rendered into an
  `Rgba8`/`GrayA8` buffer that started at `Color::TRANSPARENT` composites its
  own coverage; a source without an alpha channel is opaque everywhere.
- `stop` is asked once every sixteen rows and the walk returns immediately;
  the predicate cannot change a pixel that is computed.

Two more primitives gap 09 may find useful: `Canvas::extract(at, w, h, format)`
copies a rectangle out, transparent where it hangs off the edge, and
`Canvas::snapshot()` clones pixels without the stored backdrop.

### The design decision that was not in the plan: two alphas

A non-isolated group's buffer has to carry two different alphas at once, and
this is the thing that shaped the whole implementation.

The colour channels hold the group **over** its backdrop, because that is what
a blend mode inside the group must see (11.4.4). But 11.4.7.2's removal is
`C = Cn + (Cn - C0)·(a0/agn - a0)`, and `agn` is the group's **own**
accumulated alpha. With an opaque backdrop — which is every page — `agn`
divides out of the stored union entirely and cannot be recovered afterwards.

So `blend` takes the initial backdrop's alpha as a separate input: colour
renormalises against the union `ag + a0·(1 - ag)`, and the alpha channel keeps
accumulating `ag`. With no backdrop the two numbers are equal and every line is
what it was, which is why no existing fingerprint moved.

The alternative — a shadow coverage plane updated at every paint site — reaches
the same `agn` and has to be threaded through `fill_mask`, `blend_pixel` and
the image sampler's inner loop, three places that can drift apart. Putting it
in `blend` puts it in the one place all three already go through.

### Where the buffer's bounds come from

Not from `/BBox` directly: from **the clip in force** when the group opens.
8.10.2 clips a form to its `/BBox` and the interpreter installs that clip
before `begin_group`, so the clip is the box intersected with whatever encloses
it — tighter than the box, never larger, and free. Everything inside the group
then draws in the buffer's coordinates, with the translation folded into `base`
rather than into a second origin that four call sites would have to remember.

A soft mask's buffer is bounded by its group's `/BBox` and deliberately *not*
by the clip: the mask applies to whatever is painted after the `gs`, which may
be anywhere.

### Two corrections to the plan's own text

**`/BC` outside the group's bounding box.** The plan says the `/BC` default is
black and stops there. It is also true that `/BC` applies *outside* `/G`'s
bounding box, because there is no group out there either — and a `Mask` reads
zero outside its own rectangle, so a bounding-box-sized soft mask silently
means "`/BC` is black" whatever the file said. Correct for the default and
wrong for every file that gives one. The page-sized field is built only when
`/BC` has any luminosity at all, so the common mask still costs its box.

**The removal step is invisible on an opaque group.** `a0/agn - a0` is exactly
zero when `agn` is 1. A fixture whose group paints opaquely cannot see backdrop
removal at all, and the first draft of the determinism fixture did exactly
that: deleting the removal step left its hash unchanged. Both the integration
fixture and the fingerprint now put a `ca 0.5` *inside* the group.

### Not done, and deliberately

- **Group colour spaces (`/CS`).** A non-goal in the plan and still one: the
  engine composites in RGB throughout. `/CS` is read only to resolve `/BC`.
- **A page-level `/Group`.** Plan 08 mentions rendering the page as an isolated
  group so blend modes meet the page backdrop correctly. Out of this plan's
  Scope, which names `/Group` on a *form* XObject, and not implemented.
- **Shape separate from alpha (`/AIS`).** 11.4.5's knockout and 11.6.5's
  luminosity both distinguish an object's shape from its alpha; these buffers,
  like every buffer this engine has, carry one number. Knockout's restore is
  therefore exact at full coverage and at none, and weights between the two on
  an anti-aliased edge.
- **A form's own `/Resources`.** Not consulted anywhere in this engine — a
  pre-existing gap this work ran into while writing a nested-group fixture, not
  one it created. Named here because the fixture had to work around it.

### Injections

Each defect was inserted deliberately and the whole workspace re-run with
`--no-fail-fast`.

| Defect | Caught by |
| --- | --- |
| `/BC` defaulted to white | 8 integration tests **and** the `transparency` fingerprint |
| The mask group rendered at the wrong CTM | 1 test — the two-document one — and **not** the fingerprint, whose mask group has an identity matrix |
| Backdrop removal skipped | 2 tests and the fingerprint, but only after the fixture gained a partial alpha inside the group; before that, nothing at all |
| Knockout composited against the accumulated result | 2 tests and the fingerprint |
| The soft mask surviving a `Q` | Exactly 1 test, and nothing else in the workspace |
| Group alpha applied element by element | 3 tests, and **not** the fingerprint |

The two "and not the fingerprint" rows are the useful ones: a hash over one
page cannot cover a construct that page does not contain, and both of those
constructs are cheap to add later if a corpus run ever wants them.

### The determinism fixture

`transparency`, 120x80, 8066 pixels of 9600, floor 4000. Five things in no
other fixture: a non-isolated group at `ca 0.5` under `Multiply` over a
coloured bar with partial alpha inside it; two overlapping shapes in that
group; a knockout group whose two half-opaque shapes overlap; a `/Luminosity`
mask whose group is an axial shading, so the mask value differs in almost every
column; and a square-law `/TR` applied to the backdrop as well as to the group.
`wasm32-wasip1` and `x86_64-pc-windows-msvc` agree on the hash. **The seven
existing fingerprints did not move**, on either target.
