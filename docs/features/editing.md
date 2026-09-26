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
through the document's name table. `stream_bytes` reads a stream as the
editor has it, decoded — including a stream copied in by `import_page`,
which keeps the file's `/Filter` and was handed back still compressed until
September 2026, so `append_content` on an imported page spliced deflate
output into it as operators.

**Every read of the editor's own state goes through the overlay.** The
editor implements `Resolve` — the trait `CosDocument` implements too, with
`CosDocument`'s own method names — and the walks that serve its edits take
that view: the field tree (`fields()`), the widget, form and quadding reads
behind an appearance, and `trees::name_tree_in` / `number_tree_in` /
`name_tree_lookup_in`. So a field the editor has put and listed in
`/Fields` is a field before anything is saved, and `fill_field` fills and
draws it. `catalog()` is the catalog as the editor has it and
`update_catalog(|catalog| ...)` is the one door every catalog edit goes
through, so two compose; before it, `/NeedAppearances`' clean-up read the
*file's* catalog and wrote it back over every earlier change to it, and a
certifying save's `/Perms /DocMDP` was made after the update's object set
had been taken and never reached the file. `add_name_tree` and
`add_number_tree` write a 7.9.6 / 7.9.7 tree as new objects and return its
root ([document model](document-model.md)).

**Trailer entries.** `set_trailer_entry(key, value)` lays an entry over the
document's trailer (7.5.5), and every save writes the merged trailer —
incremental, rewrite and signed — so it is editor state like the overlay: a
checkpoint takes it and a rollback restores it. The writer's own keys are
refused (`/Size`, `/Prev`, `/XRefStm`, `/ID`, `/Encrypt` and the
cross-reference stream's), since a value for them would be overwritten or
believed. `set_info(key, value)` is the use it exists for: an existing
`/Info` is updated where it is, and a document with none gets one, which
until September 2026 no save could name — `save` wrote the document's own
trailer and nothing else. The value is a text string, encoded as the
builder's `set_info` encodes it.

**A view as a document.** `view()` returns an `Arc<CosDocument>` in which
everything the editor has done resolves — objects put or allocated, streams
written, deletions as null, the page order, the trailer entries — for a
caller that has to hand something built over a `CosDocument` (a page's
`PageResources`, an interpreter) a reference the editor has only just
allocated. It is the incremental update `save` would write, built in memory
and reopened, so object numbers are the editor's own and every read goes
through the ordinary reader, filters included. An encrypted document is
viewed decrypted: the update is sealed with the file's key as a save seals
it, and the view inherits the security handler the document was
authenticated with; a document never authenticated gives a view that was not
either. A view is a snapshot costing a copy of the file and a parse of its
tables; an untouched editor answers with its own document and copies
nothing. `editor_view.rs` resolves an allocated form XObject through the page
that names it, at the same number, plain and encrypted, and the facade's
`resources_over_an_editors_view_resolve_what_it_allocated` does the same
through `PageResources`.

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

**Transactions.** `transaction(|tx| ...)` snapshots all five mutable fields
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
`RedactionReport::warnings` — four of that type's five classes, in the
refusal table below; the fifth is the form placement one further down —
because a redaction that silently fails to redact is worse than one that
refuses: the caller believes the content is gone and distributes the file.
A warning says the run was not measured, not that it was covered, so
warnings are raised only when there is at least one rectangle to fall under.
Form XObjects are rewritten recursively, each
resolving names against its own `/Resources` (8.10.1), because forms are how
most producers place repeated content and a redaction driven straight
through one would leave the secret in the form. A form is rewritten **once
per distinct placement**, each pass reading the bytes the pass before it
left, so a rectangle over a form's second placement is measured against that
placement rather than against nothing. The guard that stops a
self-referential form recursing is keyed by the transform as well as by the
object, which is what separates "the same form again" from "the same form
somewhere else"; bitwise and not by tolerance, because two transforms an ulp
apart are two placements and calling them one is a decision not to cut, and
what bounds the walk is a cap on placements per form (`MAX_PLACEMENTS`)
rather than any comparison of floats. Until September 2026 the guard was
keyed by the object alone and only the first placement was ever measured —
a silent under-redaction that reported `glyphs: 0` with no warning,
indistinguishable from a rectangle that covered nothing. **What that costs
is the other direction, and it is named.** A form is one stream however
often it is drawn, so a glyph cut because a rectangle covered it at one
placement is gone at all of them, including placements no rectangle touched;
`RedactionWarning::RepeatedForm` names the form and how many placements it
had, and is raised only when a cut was actually made, since a repeated form
nothing was cut from is exact. Making it exact means a copy of the form per
placement, which is a [roadmap](../ROADMAP.md) row of its own. An image a
redaction touches is scrubbed whole to a blank sample: cutting a hole would mean decoding,
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

**Font subsetting on rewrite** (9.6.4, 9.9). A rewrite used to copy every
embedded font program through untouched however little of it the document
still drew. `subset::apply(&mut editor)` cuts each one down to the glyphs the
document draws now. Two costs go with it, and the second is why the redaction
section above ends here:

- **Size.** Measured on the vendored Liberation Serif Regular, 393 576 bytes:
  a page of ten characters carries **29 376** after the pass, and a page
  drawing none of it 26 428 — not zero, and it should not be, since `cmap`,
  `hmtx`, `OS/2` and the three hinting tables are copied through for the
  readers that interpret them.
- **Disclosure.** Redaction removes the *text*. It does not remove the
  **glyphs**, which sit in the program exactly as they were, and a face whose
  `glyf` entries are `J`, `o`, `h`, `n`, `S`, `m`, `i`, `t` and `h` names what
  the redaction was for. So a redaction that must not disclose runs this
  afterwards. Measured on a two-line fixture: 395 534 bytes redacted, **30 301**
  redacted and subsetted, and the six removed letters' outlines absent from the
  program that remains — asserted against its own `loca` and `glyf`, not
  against a picture.

**It is whole-document and it runs last.** A font used on page two must keep
page two's glyphs however thoroughly page one was redacted, so there is no
per-page form of the operation that is not wrong; and it reads the content the
*editor* now has rather than the file, so an edit applied first is an edit this
sees. Run the other way round it would keep exactly what the redaction removed.
Save with `WriteMode::Rewrite` if the removal has to be real — an incremental
save appends and leaves the original program's bytes in the file, the same
caveat redaction carries.

**Which glyphs count as used.** Err toward inclusion: a glyph wrongly excluded
is a blank or a wrong glyph on the page, with nothing in the file saying so.
Counted are a page's own content stream; a form XObject it draws, at any depth
(8.10); a Type 3 glyph procedure entered because its own glyph was shown
(9.6.5); and **every** appearance stream under an annotation's `/AP` — `/N`,
`/D` and `/R`, and every state of each, whatever `/AS` currently selects, since
12.5.5 lets a viewer switch states with no edit to the file and a subset cut to
today's state loses tomorrow's tick. A hidden annotation's appearance counts
too: the flag is a viewer's instruction, and clearing it is one bit.

**How the encoding survives.** `tinker_pdf_font::subset` does not renumber — a
dropped glyph becomes a zero-length `loca` entry rather than a gap the later
glyphs shuffle into — so `/FirstChar`, `/LastChar`, `/Widths`, `/W`, `/DW`,
`/Encoding`, `/Differences`, `/CIDToGIDMap` and `/ToUnicode` are all still
correct **because they are unchanged**, and this pass changes none of them
(`font_dictionaries_are_untouched_except_for_the_subset_tag`). Three names do
move, and all three are the same name: `/BaseFont` on the font dictionary gains
9.6.4's six-letter tag from the same `subset_tag` the *builder* names its
subsets with, `/BaseFont` on a composite font's descendant (9.7.6.2) and
`/FontName` in the descriptor (9.8.1) follow it. A tag already there is
replaced, never stacked. One thing moves on the stream: `/Length1`, which
Table 126 defines as the decoded length of a `/FontFile2`; a `/FontFile3` has
none and a stale one is dropped rather than carried.

**The unit of work is the program, not the font dictionary.** Two font
dictionaries may point at one `/FontFile2`, and subsetting it for one of them
would take the other's glyphs away, so every font that names a given stream is
found first, their glyph sets unioned, and the stream left whole if any one of
those fonts is one this pass will not bound.

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
`delete()`, `intern()`, `stream_bytes()`, `catalog()`, `update_catalog()`,
`add_name_tree()`, `add_number_tree()`, `set_trailer_entry()`, `set_info()`,
`view()`,
`transaction()`, `checkpoint()`, `restore()`, `page_refs()`,
`delete_page()`,
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

`subset::{SubsetReport, Subsetted, Untouched, UntouchedReason, apply}` are on
the facade too. `apply(&mut editor) -> SubsetReport` takes no page: it is
whole-document by construction. `SubsetReport::subsetted` carries each
program's object, its new `/BaseFont`, the bytes before and after, and how many
glyphs were asked for; `untouched` carries every program written through whole
with its `UntouchedReason`, and `bytes_before()`/`bytes_after()` total both.
An empty `untouched` is the answer a caller wants; a non-empty one is not an
error list, since a document whose every face is already a tight subset reports
all of them and is right to. Over the fetched corpora it is usually non-empty:
4 950 of 10 832 programs went through whole, 3 406 of them because a rebuild
came out no smaller than the producer's own subset.

**A caller no longer has to remember.** `tinker_pdf::write::save` is the
facade's save door and runs this pass by default, in the one order that is
right — after every other edit, before the serializer
([writing](writing.md)). Calling `apply` by hand is still correct and is what
a caller wants when the report has to be read before the bytes are written.

```rust
// The arranged form: a rewrite that subsets, with the report on the way out.
use tinker_pdf::write::{save, SaveOptions};

let saved = save(&mut editor, &SaveOptions::default());
if !saved.fonts.removed() {
    // Not a failure. `removed()` is false whenever *any* program went through
    // whole, because such a program still carries every outline it had.
    for whole in saved.fonts.report().into_iter().flat_map(|r| &r.untouched) {
        // "/LiberationSerif left whole (393 576 bytes): a form field's /DA may
        //  draw it at any character (12.7.3.3)" — ruling 10, so a caller
        //  checking for disclosure can see what is still in the file
        println!("{whole}");
    }
}
std::fs::write("out.pdf", &saved.bytes)?;
```

```rust
// The asked-for form, unchanged: the report in hand before anything is
// serialized, for a caller who decides whether to write at all from it.
let report = tinker_pdf::subset::apply(&mut editor); // after every other edit
if report.untouched.is_empty() {
    let bytes = editor.save(&tinker_pdf::WriteOptions {
        mode: tinker_pdf::WriteMode::Rewrite, // an incremental save keeps the old bytes
        ..Default::default()
    });
}
```

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Redacting a **vertical** text run (9.4.4) | the run is left whole and `RedactionReport::warnings` carries a `VerticalRun` naming the font and how many operand bytes stayed (`a_vertical_run_is_left_uncut_and_reported`) | the vertical branch advances by `/W2`'s `w1` down the page and a `TJ` number displaces along that axis too — a different formula, not a different matrix. This class used to hide behind the rotation refusal, whose matrix test a vertical run passes, so it was being cut horizontally | — |
| Redacting a run in a **Type 3 font whose `/FontMatrix` is not the 1/1000 default** (9.6.5) | left whole, `RescaledType3Font` (`a_rescaled_type3_font_is_left_uncut_and_reported`); the same font with the conventional matrix is cut (`a_type3_font_with_the_conventional_matrix_is_cut`) | `/Widths` are in the font's own glyph space, and `width / 1000` is right for the conventional matrix and wrong by exactly that matrix for any other, so every position after the first glyph drifts | — |
| Redacting a run whose `Tf` named a font the resources in scope do not have | left whole, `UnknownFont` (`a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported`) | no metrics at all, so no glyph can be placed. Permanent, and it was silent before: the run was kept, nothing was counted, and the report looked like a rectangle that covered nothing | — |
| Redacting a run whose text rendering matrix is not finite | left whole, `UnmeasurableFrame` (`a_non_finite_text_matrix_is_left_uncut_and_reported`) | a position that is not a number cannot be compared with a rectangle. Permanent. The whole showing operand is left, never half of it | — |
| Partial image redaction | the whole image is scrubbed (`RedactionReport::images`) | a hole needs a re-encode through a codec this build may not write | [filters](filters.md) |
| Cutting a form XObject at one placement only, when it is drawn at several | the cut is the union over every placement and `RedactionWarning::RepeatedForm` names the form and its placement count (`a_form_drawn_twice_is_cut_at_the_placement_the_rectangle_covers`); a repeated form nothing was cut from is exact and says nothing (`a_form_drawn_twice_that_nothing_is_cut_from_raises_no_warning`) | a form is one stream however often it is drawn, so a cut made for one placement shows at all of them. Over-removal is the direction this module errs in everywhere; the alternative here is the leak. Exactness needs a copy of the form per placement, which is a [roadmap](../ROADMAP.md) row | 8.10 |
| Measuring more than `MAX_PLACEMENTS` distinct placements of one form | the count in `RepeatedForm` saturates at the cap, which is how a caller tells "too much went" from "something may have survived" (`a_form_placed_more_times_than_the_cap_saturates_its_count`) | a form that invokes itself under a matrix that moves each round makes a fresh placement every time; a count bounds it, where a tolerance on matrices would have to be loose enough to call two real placements one | ruling 1 |
| Appearance synthesis for other subtypes | `add_annotation` inserts the dictionary; no `/AP` is generated | seven subtypes cover the common producer gap; others render only if they carry their own `/AP` | — |
| Redaction of text inside a Type 3 glyph procedure or an annotation appearance | not rewritten | content streams reachable from a page are rewritten; glyph procedures and `/AP` streams are separate objects | — |
| Subsetting a program `tinker_pdf_font::subset` will not rebuild — a Type 1 program, a CFF whose charstrings cannot be renumbered without guessing, bytes that are neither | the program is written through exactly as it arrived, `UntouchedReason::ProgramNotRebuildable` | ruling 2: a document that renders is worth more than one that is small | [fonts](fonts.md) |
| A subset that comes out no smaller than the face | whole face, `SubsetNotSmaller` (`a_subset_that_is_no_smaller_is_refused_and_named`) | the face is both smaller and the one the producer tested — the same reason the builder refuses it | [fonts](fonts.md) |
| Subsetting a font any of whose shown codes resolved only by 9.6.6.4's **closing guess** — read the code as the glyph index | whole face, `CodeNotMapped` (`a_font_whose_codes_resolve_only_by_guess_is_left_whole_and_reported`) | the guess is not a statement the font made, and two readers are free to guess differently; a glyph dropped on one guess is a glyph the other reader draws and no longer has | 9.6.6.4 |
| Subsetting a font the AcroForm `/DR` names for a field's `/DA` | whole face, `FieldResource` (`a_font_the_acroform_default_resources_name_is_left_whole_and_reported`) | a `/DA` is a promise about *future* uses: the field's value can be retyped and the generated appearance may draw any character the font has, so there is no set to bound | 12.7.3.3 |
| Subsetting a font no walked resource dictionary names — one reached only from a form XObject nothing draws | whole face, `ScopeNotWalked` (`a_font_no_walked_scope_names_is_left_whole_and_reported`) | "no glyphs were shown through it" is then ignorance rather than a measurement | — |
| Subsetting a font a **Type 3 font's own `/Resources`** names | whole face, `Type3Resource` (`a_font_a_type3_fonts_own_resources_name_is_left_whole_and_reported`) | this engine runs a glyph procedure in the *enclosing* scope, so a `/F0` inside a procedure is credited to the enclosing `/F0`; the enclosing font merely gains glyphs it does not need, the Type 3 font's own would lose every glyph it does | 9.6.5 |
| Subsetting a font written **directly** into a resource dictionary rather than by reference | whole face, `NotAnObject` (`a_font_written_directly_into_the_resources_is_left_whole_and_reported`) | there is no object to key glyph usage by; the refusal also protects a program such a font *shares* with one that does have an object, which would otherwise be cut to the other font's glyphs | — |
| Subsetting a program a `/FontDescriptor` embeds that **no font dictionary names** | whole face, `NoFontNamesIt` (`a_program_no_font_dictionary_names_is_left_whole_and_reported`) | there is no font, so no encoding and no glyph usage — nothing to subset it against. It is *reported* because a `Rewrite` keeps unreferenced objects unless `garbage_collect` asks otherwise, so every outline is still in the output; it was silently invisible until the corpus census counted 73 of them across eight of 5 605 documents | 9.8.1 |
| Running the subsetter automatically from `DocumentEditor::save` | it cannot: `save` takes `&self` and the pass rewrites the editor, and `WriteOptions` is a crate below the interpreter that drives the walk. That door writes every program through as it arrived and does not offer to do otherwise | a flag the crate carrying it cannot act on would read as done and do nothing, on the one path where that is a disclosure. The switch is `tinker_pdf::write::SaveOptions::fonts`, which **defaults to subsetting** | [writing](writing.md) |
| Running the subsetter from `tpdf` | nowhere to put it: all nine subcommands are read-only, so the CLI has no write path for a flag to attach to | tier 5's "A user-facing CLI" [roadmap](../ROADMAP.md) row owns the write half and now carries the font policy in its exit criterion, so the flag arrives with the door rather than before it (ruling 11: a subcommand is a wrapper over the facade with no logic of its own) | — |
| Paying for the walk on a save that changed one annotation | `FontPolicy::Keep` on `write::save`, or `DocumentEditor::save`, which is unchanged | the pass is whole-document and order-dependent and costs a full interpretation of every page. The default is still `Subset`, because forgetting costs a disclosure and paying costs time | [writing](writing.md) |

## Verified

- `crates/tinker-pdf-cos/tests/page_operations.rs` — delete, move, rotate,
  insert, import, keep, append; each saved in both modes, because a page
  operation once reached only the incremental set and nothing caught it.
- `crates/tinker-pdf-cos/tests/form_transactions.rs` — `transaction`
  restores the first four fields (the fifth, the trailer entries, is
  `trailer_overlay.rs`'s); injection puts each restore under exactly one
  test.
- `crates/tinker-pdf/tests/annotation_appearances.rs` — synthesised
  appearances render and flatten.
- Redaction tests live beside `crates/tinker-pdf/src/redact.rs`: multi-page
  fixtures (a two-page file once redacted page 0's image and left page 1's
  secret), text inside form XObjects, a self-referential form that
  terminates and one that does so under a transform that moves each round,
  a form drawn twice cut at the placement the rectangle covers and at both
  when both are covered, a nested form measured at every placement of its
  parent, an image drawn twice scrubbed from its second placement, scaled
  runs, and the needle-bytes-absent assertion over every
  decompressed stream. Three further modules carry the rotated cut: a
  quarter turn, an oblique rotation, a skew, a rotation that lives in the
  `cm` rather than the `Tm`, and the matrix and the `TJ` gaps re-emitted in
  the run's own units (`rotated_runs`); `'`, `"` and an existing `TJ`
  adjustment surviving a cut (`showing_operators`); and one test per
  unmeasurable-run `RedactionWarning` variant (`refusals`) — the fifth
  variant, `RepeatedForm`, is about a form rather than a run and its tests
  sit with the other form ones. Their fixtures are a Type 3 font
  whose every glyph fills its em square, so the geometry a test computes by
  hand from 9.4.2 to 9.4.4 and the ink the renderer draws are the same
  rectangle — which is what lets each of them assert the safety property at
  both levels, the covered glyph's code absent from every stream **and** no
  ink inside the rectangle.
- Subsetting-on-rewrite tests live beside `crates/tinker-pdf/src/subset.rs` —
  26 of them over the **vendored Liberation faces**, which are third-party
  bytes: which of their glyphs are composite, what those are built from and
  where `loca` puts them are facts about the face and not about a fixture
  written to pass. The document around them is this engine's own writer, which
  is a container and not an adjudicator, and that is said in the module's own
  documentation rather than implied. What is asserted: the rewrite renders
  **identically** to the original and the program shrank; every glyph the page
  shows is still in the program and the `/BaseFont` carries a well-formed
  9.6.4 tag on all three names that must agree; the font dictionary is
  untouched except for that tag, `/Widths` and `/FirstChar` included; a glyph
  shown only in an annotation appearance survives, and so does one in a state
  `/AS` does not select, whether the state dictionary is written inline or
  reached by reference; a shown composite glyph's components survive; two
  fonts in one scope each keep their own glyphs and neither the other's; an
  existing tag is replaced rather than stacked; and one test per
  `UntouchedReason`, since a refusal nothing can reach is not a refusal.
  The disclosure property has its own: after redacting one of two lines, the
  removed letters' outlines are **absent** from the embedded program, read back
  through its own `loca` and `glyf`, while the kept line's are there and render
  unchanged. `crates/tinker-pdf/src/write.rs` repeats that one through the
  **arranged** door, where nobody asked for a subset at all, and carries the
  counterfactual beside it: the same redaction under `FontPolicy::Keep` still
  has every removed letter's outline in the file.
- **The same pass, over the corpus**:
  `crates/tinker-pdf/tests/cff_subset_census.rs` now runs
  `subset::apply` over all 5 605 fetched documents, not just
  `tinker_pdf_font::subset` over a program pulled out of one. 2 180 of them
  carry an embedded program; 5 882 programs are cut and 4 950 written through
  whole, and seven properties are asserted over documents nobody here wrote —
  every font dictionary identical but for `/BaseFont`, every descriptor but
  for `/FontName`, every cut program still declaring the same glyph count and
  still parsing, none larger than it was, `/Length1` describing the bytes it
  is on (Table 126), and every embedded program in one list or the other.
  Three of the seven failed the first time it ran; the constants at the foot
  of that file say what each was and where it was fixed.
- **Counted injection over the subsetter's rewrite path**
  (`docs/verification.md`'s house practice). Nineteen defects put back one at a
  time, `cargo test --no-fail-fast -p tinker-pdf` run for each, and the
  assertions that fire counted:

  | defect reintroduced | tests that caught it |
  | --- | --- |
  | the used-glyph set is collected but not passed to the subsetter | 8 |
  | a glyph used only in an annotation appearance is omitted | 2 |
  | the pass moves an encoding key it must not touch (`/FirstChar`) | 3 |
  | the pass truncates `/Widths` rather than leaving it at the original range | 3 |
  | the 9.6.4 subset tag is omitted | 2 |
  | the subset tag is written malformed — `abc+` for `ABCDEF+` | 1 |
  | an existing tag is stacked rather than replaced | 2 |
  | the composite-glyph closure is not taken | 1 here, 1 in `tinker-pdf-font` |
  | the fallback-to-whole-program path is taken silently | 5 |
  | the walk reads the file rather than the editor's rewritten content | 1 |
  | `/Length1` is left describing the face | 1 |
  | a glyph chosen by 9.6.6.4's closing guess is counted as used | 1 |
  | `NotAnObject` dropped — a directly written font's program is cut | 1 |
  | `ScopeNotWalked` dropped | 1 |
  | `Type3Resource` dropped | 1 |
  | `FieldResource` dropped | 2 |
  | a `/AP` state dictionary written **inline** is dropped, only a stream counting | 1 |
  | a `/AP` state dictionary reached by **reference** is dropped | **0**, then 1 |
  | glyph usage keyed by the scope's first font rather than the named one | 1 |

  **One row scored zero, and it is the row worth reading.** `appearance_streams`
  has a branch for an `/AP` entry that is an indirect reference, which must then
  be resolved and asked whether what came back is a stream (one appearance) or a
  dictionary (a state per key). Deleting the dictionary half of that branch
  broke nothing: every fixture wrote its states inline, so the branch real
  producers actually take — the states are shared between widgets, so they get
  an object — was reached by no test at all. It is now
  `a_glyph_in_an_appearance_state_reached_by_reference_is_kept`, and the same
  deletion scores 1.

- Every edited document is written through the [writer](writing.md), whose
  output is held to the strict validator (`strict_validator.rs`); the
  `render_page` and
  `cos_document` fuzz targets cover the reader side of what the editor
  produces.
