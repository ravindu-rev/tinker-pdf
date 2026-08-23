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
can have because it looks like it worked. A rotated or skewed run is
therefore left uncut rather than mis-cut — under-redaction is visible to
whoever checks. Form XObjects are rewritten recursively, each
resolving names against its own `/Resources` (8.10.1), because forms are how
most producers place repeated content and a redaction driven straight
through one would leave the secret in the form. An image a redaction touches
is scrubbed whole to a blank sample: cutting a hole would mean decoding,
editing and re-encoding through a codec this build may have no encoder for,
and leaving the rest is not a redaction. `mark` paints the area black
afterwards — cosmetic, because the content is already gone; it tells a
reader something was removed rather than leaving a gap that reads as if
nothing was there. The acceptance test is not "does it look right" but
"decompress every stream in the output and assert the needle bytes are
absent".

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
// report.operations cut, report.glyphs removed, report.images scrubbed

let bytes = editor.save(&tinker_pdf::WriteOptions::default());
```

`DocumentEditor` (facade re-export of `tinker_pdf_cos::DocumentEditor`):
`document()`, `is_dirty()`, `allocate()`, `get()`, `put()`, `put_stream()`,
`delete()`, `intern()`, `transaction()`, `page_refs()`, `delete_page()`,
`move_page()`, `rotate_page()`, `set_crop_box()`, `insert_page()`,
`import_page()`,
`keep_pages()`, `append_content()`, `page_box()`, `flatten_annotations()`,
`add_annotation()`, the [forms](forms.md) methods, and `save(&WriteOptions)
-> Vec<u8>`. `redact::{Redaction, RedactionReport, apply}` live in the facade
(`apply` returns `Option<RedactionReport>`, `None` for a page that does not
exist; the report counts `operations`, `glyphs` and `images`)
because glyph coverage needs both the content tokenizer and font metrics
(ruling 8, [rulings.md](../rulings.md)).

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Redacting a rotated or skewed text run | that run is left uncut, so its glyphs never reach `RedactionReport::glyphs` — an under-redaction a caller can see (`a_rotated_run_is_left_alone_rather_than_cut_wrongly`) | cutting a rotated run by a rectangle mis-cuts glyphs and looks correct — the worst redaction failure | — |
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
  terminates, scaled and rotated runs, and the needle-bytes-absent assertion
  over every decompressed stream.
- Every edited document is written through the [writer](writing.md), whose
  output is held to the strict validator (`strict_validator.rs`); the
  `render_page` and
  `cos_document` fuzz targets cover the reader side of what the editor
  produces.
