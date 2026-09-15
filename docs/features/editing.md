# Editing

`DocumentEditor` is an exclusive copy-on-write overlay over an open
document: edits accumulate in the overlay while every reader of the original
keeps its consistent snapshot, and `save` serialises the result through the
[writer](writing.md). Page surgery, annotations, flattening and redaction all
go through it — there is one mutation path, and it is this one.

## What it does

**Copy-on-write over `Arc<CosDocument>`.** The editor holds the original
document by shared pointer and records changes as an overlay of new or
replaced objects, a set of deletions, a page order and an object-number
counter. Nothing writes to the source bytes, ever; a `Page` or `Document`
handed out before the editor existed stays valid and unchanged. Readers
never observe a half-applied edit and never dangle.

**Object-level access.** `allocate` mints a fresh object number; `get`
reads through the overlay so a later edit sees an earlier one (a field
filled and then read back returns the filled value, not the saved one);
`put`, `put_stream` and `delete` write; `intern` turns bytes into a `Name`
through the document's name table.

**Page surgery** (7.7.3). `delete_page`, `move_page`, `rotate_page` (any
multiple of 90, stored as `/Rotate`), `set_crop_box` (14.11.2, written as the
caller states it and never clipped here — 14.11.2 lets a crop box and a media
box disagree and the *reader* reconciles them; a rectangle of no area is
refused), `insert_page` (a blank page of the
given size), `import_page` (a page and its resource closure copied from
another `CosDocument`), `keep_pages` (the complement of delete, in one
call), `append_content` (operators appended to a page's content array) and
`page_box`. Page operations apply to whichever object set the save mode
builds — rewrite or incremental — so a reordered `/Kids` reaches the file
either way.

**Transactions.** `transaction(|tx| ...)` snapshots all four mutable fields
and restores them if the closure returns `Err`. It is a closure rather than
a begin/commit/rollback triple because the failure it prevents is silent —
a closure has no scope exit that neither commits nor rolls back. The
object-number counter is restored deliberately: leaving it advanced grows
the file on every *failed* edit. [Forms](forms.md) build their all-or-nothing
multi-field fill on this primitive.

**Annotations** (12.5). Reading and rendering of existing `/AP` appearance
streams is the [renderer](rendering.md)'s job; the editor adds and removes.
`add_annotation` inserts a dictionary into a page's `/Annots`, and
`appearance.rs` synthesises an appearance stream for seven subtypes that
producers commonly leave without one — `Highlight`, `Underline`,
`StrikeOut`, `Square`, `Circle`, `Text` and `Link` — so a viewer that draws
only `/AP` (most of them) shows the annotation. Constructors for the common
shapes live in `tinker_pdf_cos::annot`: `highlight(doc, quads, color)`, `square(doc, rect,
color, width)`, `text_note(doc, rect, contents, open)` and `link(doc, rect,
page)`. `flatten_annotations(page)` paints every annotation's normal
appearance into the page content and removes the annotation, returning how
many it flattened.

**Redaction.** The whole point is that the content is *gone*, not covered —
a black rectangle over text leaves the text in the stream for any extractor
to find. `redact::apply(&mut editor, page, &[Redaction { area, mark }])`
rewrites the content stream itself: every text-showing operator whose
glyphs fall inside a redaction rectangle has those glyphs removed and is
re-emitted with a displacement in their place, so surviving text keeps its
position. Deciding which glyphs a rectangle covers needs the whole `Tm` and
`cm` matrices, not a translation — a scaled or rotated run measured as a
translation cuts the wrong glyphs, which is the worst failure a redaction
can have because it looks like it worked. Both matrices are carried whole,
so a rotated or skewed run is **cut along its own baseline**: the glyph box
is a parallelogram in page space and whether it meets the rectangle is a
separating-axis test rather than a comparison of two intervals, while the
replacement displacement needs no frame of its own, because 9.4.3 already
measures a `TJ` number in unscaled text space — the run's own frame — and
the gap a removed glyph leaves therefore rotates with the run for free. The
surviving sub-runs keep the original text matrix untouched; giving each a
fresh `Tm` at its own origin is the answer that looks plausible on screen
and is wrong four ways, which `emit_array`'s doc comment sets out. A glyph
the rectangle covers only *partly* is removed, because a content stream can
show a glyph or not show it and only one of those two can leak. What
redaction cannot **measure** it still leaves whole and names in
`RedactionReport::warnings` — the four classes in the refusal table below —
because a redaction that silently fails to redact is worse than one that
refuses: the caller believes the content is gone and distributes the file.
A warning says the run was not measured, not that it was covered, so
warnings are raised only when there is at least one rectangle to fall under.
Form XObjects are rewritten recursively, each
resolving names against its own `/Resources` (8.10.1), because forms are how
most producers place repeated content and a redaction driven straight
through one would leave the secret in the form. Each form is rewritten
**once**, which is right when a form is drawn once or twice under the same
transform and wrong when the same form is drawn in two places: only the first
placement is measured, and that under-redaction is pinned by
`a_form_drawn_twice_is_cut_only_at_its_first_placement` and carries a
[roadmap](../ROADMAP.md) row of its own. An image a redaction touches
is scrubbed whole to a blank sample: cutting a hole would mean decoding,
editing and re-encoding through a codec this build may have no encoder for,
and leaving the rest is not a redaction. `mark` paints the area black
afterwards — cosmetic, because the content is already gone; it tells a
reader something was removed rather than leaving a gap that reads as if
nothing was there. The acceptance test is not "does it look right" but two
assertions at once: decompress every stream in the output and assert the
needle bytes are absent, **and** render the redacted page and assert there
is no ink inside the rectangle. A stream check alone would pass a build that
left the glyph in an untouched duplicate stream; an ink check alone would
pass the black-rectangle non-redaction this module exists to refuse. Neither
half is the property.

## API

```rust
let doc = tinker_pdf::Document::open(bytes)?;
let mut editor = doc.editor();

editor.rotate_page(0, 90);
editor.move_page(3, 0);
editor.keep_pages(&[0, 1, 2]);

let report = tinker_pdf::redact::apply(
    &mut editor,
    0,
    &[tinker_pdf::redact::Redaction { area: rect, mark: true }],
)
.expect("page 0 exists"); // None only when the page does not
// report.operations cut, report.glyphs removed, report.images scrubbed,
// report.warnings: runs left whole because they could not be measured

let bytes = editor.save(&tinker_pdf::WriteOptions::default());
```

`DocumentEditor` (facade re-export of `tinker_pdf_cos::DocumentEditor`):
`document()`, `is_dirty()`, `allocate()`, `get()`, `put()`, `put_stream()`,
`delete()`, `intern()`, `transaction()`, `page_refs()`, `delete_page()`,
`move_page()`, `rotate_page()`, `set_crop_box()`, `insert_page()`,
`import_page()`,
`keep_pages()`, `append_content()`, `page_box()`, `flatten_annotations()`,
`add_annotation()`, the [forms](forms.md) methods, and `save(&WriteOptions)
-> Vec<u8>`. `redact::{Redaction, RedactionReport, RedactionWarning, apply}`
live in the facade
(`apply` returns `Option<RedactionReport>`, `None` for a page that does not
exist; the report counts `operations`, `glyphs` and `images`, and carries
`warnings: Vec<RedactionWarning>` — empty is the answer a caller wants,
since a non-empty list means some text was never tested against the
rectangles at all)
because glyph coverage needs both the content tokenizer and font metrics
(ruling 8, [rulings.md](../rulings.md)).

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Redacting a **vertical** text run (9.4.4) | the run is left whole and `RedactionReport::warnings` carries a `VerticalRun` naming the font and how many operand bytes stayed (`a_vertical_run_is_left_uncut_and_reported`) | the vertical branch advances by `/W2`'s `w1` down the page and a `TJ` number displaces along that axis too — a different formula, not a different matrix. This class used to hide behind the rotation refusal, whose matrix test a vertical run passes, so it was being cut horizontally | — |
| Redacting a run in a **Type 3 font whose `/FontMatrix` is not the 1/1000 default** (9.6.5) | left whole, `RescaledType3Font` (`a_rescaled_type3_font_is_left_uncut_and_reported`); the same font with the conventional matrix is cut (`a_type3_font_with_the_conventional_matrix_is_cut`) | `/Widths` are in the font's own glyph space, and `width / 1000` is right for the conventional matrix and wrong by exactly that matrix for any other, so every position after the first glyph drifts | — |
| Redacting a run whose `Tf` named a font the resources in scope do not have | left whole, `UnknownFont` (`a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported`) | no metrics at all, so no glyph can be placed. Permanent, and it was silent before: the run was kept, nothing was counted, and the report looked like a rectangle that covered nothing | — |
| Redacting a run whose text rendering matrix is not finite | left whole, `UnmeasurableFrame` (`a_non_finite_text_matrix_is_left_uncut_and_reported`) | a position that is not a number cannot be compared with a rectangle. Permanent. The whole showing operand is left, never half of it | — |
| Partial image redaction | the whole image is scrubbed (`RedactionReport::images`) | a hole needs a re-encode through a codec this build may not write | [filters](filters.md) |
| Appearance synthesis for other subtypes | `add_annotation` inserts the dictionary; no `/AP` is generated | seven subtypes cover the common producer gap; others render only if they carry their own `/AP` | — |
| Redaction of text inside a Type 3 glyph procedure or an annotation appearance | not rewritten | content streams reachable from a page are rewritten; glyph procedures and `/AP` streams are separate objects | — |

## Verified

- `crates/tinker-pdf-cos/tests/page_operations.rs` — delete, move, rotate,
  insert, import, keep, append; each saved in both modes, because a page
  operation once reached only the incremental set and nothing caught it.
- `crates/tinker-pdf-cos/tests/form_transactions.rs` — `transaction`
  restores all four fields; injection puts each restore under exactly one
  test.
- `crates/tinker-pdf/tests/annotation_appearances.rs` — synthesised
  appearances render and flatten.
- Redaction tests live beside `crates/tinker-pdf/src/redact.rs`: multi-page
  fixtures (a two-page file once redacted page 0's image and left page 1's
  secret), text inside form XObjects and a self-referential form that
  terminates, scaled runs, and the needle-bytes-absent assertion over every
  decompressed stream. Three further modules carry the rotated cut: a
  quarter turn, an oblique rotation, a skew, a rotation that lives in the
  `cm` rather than the `Tm`, and the matrix and the `TJ` gaps re-emitted in
  the run's own units (`rotated_runs`); `'`, `"` and an existing `TJ`
  adjustment surviving a cut (`showing_operators`); and one test per
  `RedactionWarning` variant (`refusals`). Their fixtures are a Type 3 font
  whose every glyph fills its em square, so the geometry a test computes by
  hand from 9.4.2 to 9.4.4 and the ink the renderer draws are the same
  rectangle — which is what lets each of them assert the safety property at
  both levels, the covered glyph's code absent from every stream **and** no
  ink inside the rectangle.
- Every edited document is written through the [writer](writing.md), whose
  output is held to the strict validator (`strict_validator.rs`); the
  `render_page` and
  `cos_document` fuzz targets cover the reader side of what the editor
  produces.
