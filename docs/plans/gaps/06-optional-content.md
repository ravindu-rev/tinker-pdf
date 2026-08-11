# A hidden layer paints at full strength

Optional content is how a PDF carries a CAD drawing's construction lines, a
map's alternate languages, a proof's annotations layer. A layer the file marks
`/OFF` is not supposed to be drawn. It is drawn, at full opacity, over
whatever is underneath, with an empty warnings list. When this is done, a
document's default configuration decides what is visible. (M)

## What is wrong

8.11 is absent from `crates/` entirely. `OCProperties`, `OCGs`, `OCMD`, `/OC`
and the `/Properties` resource sub-dictionary appear only in
`docs/plans/08-rendering-device.md` and `docs/audit-2026-08.md`.

Three specific absences:

- `BDC` and `EMC` fall into the interpreter's catch-all arm at the end of
  `Interpreter::operator` (`crates/tinker-pdf-content/src/interpret.rs`).
  There is no arm for `BMC`, `BDC`, `EMC`, `MP`, `DP`, `BX` or `EX`.
- Operands are cleared after every operator, so a `BDC`'s property list is
  discarded before anything could look at it. The tokenizer does produce
  `DictOpen`/`DictClose`, so an inline `<< /OC … >>` reaches the stack as flat
  tokens — nothing reassembles them.
- `/OC` on an XObject is never read. `PageResources::form` reads `/Subtype`,
  `/Matrix` and `/BBox`; `decode_image_at` reads seven keys and not that one.

The `Device` trait has no marked-content methods, and the interpreter has no
warning channel at all — `interpret()` returns `()`.

## Scope

- Catalog `/OCProperties`: the `/D` default configuration, `/BaseState`,
  `/ON`, `/OFF`, and the `/OCGs` list.
- OCMD: `/OCGs` plus a `/P` policy of `AnyOn`, `AllOn`, `AnyOff` or `AllOff`,
  and `/VE` visibility expressions.
- Compute the visibility set once per document, at bind time.
- `BDC`/`EMC` with an `/OC` tag: track nesting, suppress paints inside a
  hidden scope.
- `/OC` on form and image XObjects: skip a hidden one.
- `BMC`, `MP`, `DP`, `BX`, `EX` recognised as no-ops rather than falling into
  the catch-all, so the operand stack is not left holding their operands.

## Non-goals

- **A layer toggle API.** Plan 08 defers it, and it needs a decision about
  whether visibility is a render option or a document mutation. Default
  visibility is what makes documents render correctly; the toggle is a
  feature.
- **`/AS` usage-application dictionaries**, which switch layers by zoom or by
  print-versus-view. Rare, and the default configuration is what a first
  render needs.

## Design

Plan 08 already specifies the seam, and this follows it:

> At bind time, 04-document-semantics hands over the catalog's
> `/OCProperties` `/D` configuration; the device computes the visibility set
> once per document. The interpreter reports `/OC` marked-content scopes and
> `/OC` entries on XObjects; the device suppresses paints inside hidden
> scopes.

**Where it binds.** `Page::render` builds `PageResources` and then
`Renderer`. `PageResources` already holds an `Arc<CosDocument>`, so the
catalog is in reach from the resource seam without a new parameter.

**Reassembling the property list.** `BDC`'s second operand is either a name
into the page's `/Properties` resource dictionary or an inline dictionary. The
name form is the common one and needs only a resource lookup — a new method on
the resource seam beside `ext_g_state_blend`. The inline form needs the
tokenizer's `DictOpen`/`DictClose` reassembled; treat a malformed one as
visible, because hiding content on a parse failure loses it.

**Suppression, not skipping.** Hidden content still advances the text pen and
still runs its operators — a `q`/`Q` inside a hidden scope must still balance,
and text extraction should arguably still see it. Suppress at the *paint*,
which is the `Device`, rather than by not interpreting.

## Where a half-implementation is worse than none

Suppressing on `/OC` without implementing `/VE` and the OCMD policies. An OCMD
with `/P /AnyOff` is visible when *any* of its groups is off — the opposite of
the naive reading — so a partial implementation hides content that should show.
Content wrongly hidden is worse than content wrongly shown: the reader cannot
tell it is missing.

Default to **visible** wherever the expression is not understood.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `/OCProperties` `/D` parsed; visibility set computed | A fixture with two groups, one `/OFF`, reports the right set; a document with no `/OCProperties` reports everything visible | S |
| 2 | `BDC`/`EMC` nesting; `BMC`, `MP`, `DP`, `BX`, `EX` as no-ops | A nested `BDC` inside a hidden scope stays hidden; an unbalanced `EMC` does not underflow | S |
| 3 | Paints suppressed inside a hidden scope | A fixture drawing a red rectangle inside an `/OFF` layer renders white; the same fixture with the layer `/ON` renders red | M |
| 4 | `/OC` on form and image XObjects | A hidden form draws nothing; a hidden image draws nothing and is not reported as an unsupported codec | S |
| 5 | OCMD policies and `/VE` | Each of the four `/P` policies decided correctly; an unparseable `/VE` renders visible | M |

## Dependencies

**Needs first:** nothing.

**Unblocks:** correct rendering of any CAD, mapping or proofing document.
These are common in exactly the professional corpora the engine is aimed at.

## Risks

| Risk | Mitigation |
| --- | --- |
| Hiding content on a misparse is unrecoverable from the reader's side | Default to visible everywhere; the tests assert that a malformed `/VE` and an unknown `/P` both render |
| The interpreter has no warning channel, so a skipped layer cannot report itself | Route it through the `Device`, which owns `RenderWarning`; a new variant naming the layer |
| Text extraction and rendering could disagree about what is present | Suppress at the paint rather than at interpretation, so both see the same operators and only drawing differs |
