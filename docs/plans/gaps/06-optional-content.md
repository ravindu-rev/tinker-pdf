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

> **Amended, August 2026 — there is nothing to reassemble.** The scan was
> written and then deleted, because injecting a defect into it could not change
> a single answer. 8.11.3.2 makes an `/OC` entry a *reference* to an optional
> content group or a membership dictionary, and 7.3.10 puts indirect references
> in the file structure, where a content stream cannot write one. So an inline
> `<< … >>` property list names no group in any document, well formed or not,
> and both the reassembly and its absence produce a visible scope. What
> remains is the rule the paragraph above is really about, which is kept and
> tested: an `/OC` property list that is not a name is visible.

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

## As built — August 2026

**Done**, five milestones in four commits. All three absences were still real
when the work started, verified against the code rather than against this
document: `Interpreter::operator` had no arm for `BMC`, `BDC`, `EMC`, `MP`,
`DP`, `BX` or `EX`; operands were cleared after every operator; and `/OC`
appeared nowhere in `crates/` at all. The `Device` trait had no marked-content
methods and `interpret()` returned `()`.

**Milestone 5 landed first, with milestone 1.** The plan's own ordering puts
the OCMD policies last, and a tree where `/OC` suppresses and `/P` does not
exist hides content that should show — which is the failure this document's
"worse than none" section is about. The whole decision procedure therefore
lands before the first thing that can act on it.

**Where it went.** `crates/tinker-pdf/src/optional.rs` reads `/OCProperties`
`/D` once per page bind; `PageResources` answers two new `FontSource`
questions with a `Layer { visible, label }`; the interpreter tracks 14.6.2's
nesting and reports each scope through two new `Device` methods; the renderer
keeps a stack of open scopes and returns early from `fill_path`,
`stroke_path`, `show_glyph`, `draw_image` and `draw_shading`.

**Three things this document does not describe.**

- *The inline property list cannot hide anything.* See the amendment in
  Design. The reassembly it asks for is unreachable, was written, was proved
  dead by injection, and was removed.
- *`/OC` on an XObject is best expressed as a marked-content scope.* Bracketing
  the `Do` rather than adding a second suppression mechanism is what makes a
  hidden image skip its *decode* — so a JPX in an off layer is not reported as
  an unsupported codec and gets no grey placeholder — while a hidden *form* is
  still run, so its text still extracts.
- *Three guards had to be placed, not merely present.* `show_glyph` returns
  before 9.3.6's clipping mode is recorded, or a text object in modes 4–7
  inside a hidden layer clips the whole page away; it also returns before the
  outline lookup, so a hidden glyph is not counted as an unreadable font.
  `draw_image` returns before the decode. And a clip set inside a hidden
  layer still applies, because 8.5.4 makes it graphics state rather than a
  paint.

**Measured.** 1131 tests to 1174. The determinism table grows a sixth
fingerprint, `optional`, because the other five would render identically on a
build that had never heard of 8.11; the other five did not move, and
`wasm32-wasip1` reproduces all six byte for byte. `fuzz/corpus/render_page`
gains `optional-content.pdf` — reachable by construction, not measured; no
campaign has been run.

**Not built, and still not:** the layer-toggle API and `/AS`
usage-application dictionaries, both non-goals above. Annotation `/OC`
(12.5.3) is also absent: an annotation's appearance stream is run directly
rather than through `Do`, so it does not reach this path, and the annotation
model is plan 08's milestone 9 rather than this one's.
