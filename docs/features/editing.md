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
root ([document model](document-model.md)). `add_named_destination(name,
page, view)` is the first caller: it rewrites the catalog's `/Names /Dests`
tree with every entry the old one held plus the new one — a `/Names` that is
an indirect object is replaced at its own number, a direct one through
`update_catalog` — and refuses a name the tree already holds.

**Layers.** `set_layer_visible(group, visible)` changes an optional content
group's state in the default configuration `/OCProperties /D` (8.11.4.3):
the group leaves both `/ON` and `/OFF` and joins whichever disagrees with
`/BaseState`, an emptied list is removed, and an indirect `/D` or
`/OCProperties` is replaced at its own number. A group `/OCGs` does not list
is refused. `group` is the reference `Document::layers()` reports.

**Trailer entries.** `set_trailer_entry(key, value)` lays an entry over the
document's trailer (7.5.5), and every save writes the merged trailer —
incremental, rewrite and signed — so it is editor state like the overlay: a
checkpoint takes it and a rollback restores it. The writer's own keys are
refused (`/Size`, `/Prev`, `/XRefStm`, `/ID`, `/Encrypt` and the
cross-reference stream's), since a value for them would be overwritten or
believed. A null value removes the entry (7.3.9): a rewrite leaves the key
out, and an incremental update writes it as null, because a reader that
merges each revision's trailer with the ones before it — this crate's does —
would otherwise find the earlier revision's value and bring it back.
`set_info(key, value)` is the use it exists for: an existing
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
either way. `set_trim_box`, `set_art_box` and `set_bleed_box` — and
`set_page_boundary(index, PageBoundary, ..)` for any of the five — follow
`set_crop_box`'s rules and write on the page itself, because 7.7.3.3 Table 30
does not make the three production boxes inheritable and a value on a
`/Pages` node describes no page.

**Document operations.** Typed setters over the catalog and the trailer, each
read back by the reader this crate already had, so a write followed by a read
is an equality rather than a translation. Every one of them checks
everything before it puts anything, so a refusal leaves the editor exactly as
it was.

- `set_page_labels(&[PageLabelRange])` writes `/PageLabels` as a 12.4.2 number
  tree through `add_number_tree`, one `/Type /PageLabel` dictionary per range
  with Table 159's `/S`, `/P` and `/St`, and `page_labels()` reads it. A list
  with no range at page 0 (12.4.2 requires one), a range past the last page,
  one numbering from 0 and two at one page are each a `PageLabelError`. An
  empty list removes the labels.
- `attach_file(&EmbeddedFile)` embeds a file (7.11.4): a `/Type
  /EmbeddedFile` stream whose `/Subtype` is the MIME type and whose `/Params`
  carry `/Size`, the MD5 `/CheckSum` Table 45 asks for and the dates given,
  under a `/Filespec` with `/F`, `/UF`, `/Desc` and an `/EF` naming the stream,
  filed in `/Names /EmbeddedFiles` beside whatever is filed there already —
  a tree written directly into `/Names` included, whose entries are carried
  into the new tree rather than dropped — and `attachments()` lists it. A
  name already filed is `AttachError::NameTaken`
  rather than a second entry under one key, which a reader resolves by
  whichever it reaches first.
- `set_outline(&[OutlineEntry])` adds an outline or replaces one, in the
  builder's own vocabulary: a `Target::Page` is an **explicit** destination
  naming the page by reference (ruling 6), so it is never collapsed into a name
  and a page moved afterwards takes its destination with it. The builder's
  writability rule is the editor's — a tree the reader would truncate is
  refused whole.
- Replacing labels, attachments or an outline **deletes the old structure's
  nodes, and only those**. The old structure's links are the producer's, so
  they are not trusted to stay inside it: below the root, a node is a
  dictionary with no `/Type` (neither 7.9.6 Table 36's tree nodes nor 12.3.3
  Table 153's items have one), and it is deleted only when nothing reaches it
  once the replacement is in place — read as a save reads it, the pending
  page order included. A last outline item whose `/Next` names an object the
  file does not have names, once the editor allocates that number, the new
  outline's own root, a page inserted earlier or an object a caller has put
  and not yet linked; a `/Kids` into the page tree names pages. None of them
  is deleted. A root written directly into the catalog has its nodes deleted
  like an indirect one's.
- `set_title`, `set_author`, `set_subject`, `set_keywords`, `set_creator`,
  `set_producer`, `set_creation_date(Date)`, `set_modification_date(Date)` and
  `set_trapped(Trapped)` are `/Info` (14.3.3) typed, over `set_info`. A date is
  spelled as 7.9.4 spells one — the closing apostrophe for a 1.x document,
  without it for 2.0 — and one with a field the syntax has no digits for is
  refused (`None`), a UTC offset of a day or more among them, whatever the
  caller's `i32` holds (`i32::MIN` included, which has no `i32` absolute
  value).
- `set_xmp_metadata(packet)` makes a caller's packet the catalog's
  `/Metadata`, `/Type /Metadata /Subtype /XML`, verbatim. **Never
  compressed**: the writer leaves every `/Type /Metadata` stream unfiltered
  whatever `compress` says ([writing](writing.md)), because ISO 19005 forbids a
  filter there and a packet scanner reads the raw bytes.
- `set_viewer_preferences(&ViewerPreferences)` writes 12.2 Table 147 typed:
  every entry the type models is written as stated, a `None` removed, and a key
  it does not model kept where it was.

**`/Info` and XMP are not kept in step, and that is decided rather than
drifted into.** Keeping them in step means parsing a packet and rewriting part
of it, and `tinker-pdf-cos` neither parses nor rewrites XML — it has no edge to
`tinker-pdf-xml`, for the reason `xmp_metadata`'s own documentation gives —
while deriving `/Info` from a caller's packet would be a second, smaller XMP
reader. So neither half is derived from the other, and every setter says what
it left: a `#[must_use]` `MetadataSync`, `Alone` when the other half does not
exist and `OtherHalfUnchanged` when it does and may now say something else. A
caller who gets the second is the one who can make the two agree, by supplying
a packet that says what `/Info` says — which is what the builder's archival
profile does for the documents it creates.

**Replacing deletes what it replaces.** A second `set_outline`,
`set_page_labels` or `attach_file` deletes the nodes of the structure it
supersedes — outline items, tree nodes, walked cycle-guarded and capped at the
readers' `MAX_TREE_ENTRIES` — rather than leaving them unreferenced, because a
rewrite writes an unreferenced object unless it is garbage-collected and an old
outline's titles are content. What a node *points at* (a file specification, a
label dictionary) is left, since the replacement may point at it too.

**Sanitising.** `sanitise(&Sanitise) -> SanitiseReport` takes out what the
four switches name: `javascript` (every JavaScript action — `/S
/JavaScript`, a `javascript:` URI, a `/Rendition` carrying `/JS` — plus
`/Names /JavaScript` and `/AcroForm`'s `/CO` and `/XFA`), `actions` (the
ones that leave the document, send its data or play media: `/Launch`,
`/URI`, `/SubmitForm`, `/ImportData`, `/GoToR`, `/GoToE`, `/Sound`,
`/Movie`, `/Rendition`, `/RichMediaExecute`; navigation inside it stays),
`embedded_files` (`/Names /EmbeddedFiles` and every file specification's
`/EF` and `/RF`) and `metadata` (`/Info` and every `/Metadata` stream).
`Sanitise::ALL` is the four.

It is **a sweep over every object the editor has**, not the form's script
walkers: `script_summary` and its walkers read the field tree's `/AA`, the
catalog's and `/Names /JavaScript`, and never a page's `/AA`, an
annotation's `/AA` or `/A`, an outline item's `/A` or an action's `/Next` —
each of which a viewer runs. The walkers are how the tests prove the sweep
left nothing *they* can see; the fixture carries the places they cannot see
as well and asserts those by hand.

**An action is what sits where a viewer runs one.** A viewer looks at the
place a dictionary sits and at its `/S`, resolving a reference, and at
nothing more, so in an *action slot* — `/A`, `/PA`, `/NA`, `/OpenAction`,
every entry of an `/AA`, and an action's `/Next`, one action or an array of
them — a dictionary whose `/S` names a Table 198 type is an action whatever
else it carries: an extra `/K` or `/P`, a `/Type /Whatever` or an `/S 9 0 R`
does not hide it. An object written on its own sits in every slot a reference
to it does, found by following the slots out from every object before
anything is cleaned, so an indirect `/AA` dictionary or `/Next` array is
swept as what it is. Anywhere else a stricter test finds actions in places
that list does not name — the `/S`, a `/Type` of `/Action` or none, and
neither `/P` nor `/K`, the keys every structure element has — so a structure
element role-mapped to `URI` in the structure tree, where nothing runs, is
not mistaken for one. A value that is an action by the test its place calls
for, in place or by reference, is removed wherever it sits. Arrays lose
elements in two kinds of place only, an array in an action slot and a page's
`/Annots`: a name tree's `/Names` is positional, and taking an element out
elsewhere could shift every key onto the wrong value. A `/Link` whose `/A` goes and which has no `/Dest`
goes with it, out of `/Annots` — a link to nowhere is a hot spot the strict
validator refuses (12.5.6.5) — while a widget whose `/A` goes stays, because
it is a field first.

**What is deleted is decided on the document as it will be.** An entry
removed is a reference removed; an object only removed entries reached is
deleted, and one anything else still reaches is not — reached through the
pending page order too, since a page `import_page` or `insert_page` added is
written into `/Kids` only by a save, and a walk that missed it once deleted an
imported page's content and fonts when its link's `/P` led there. So a script and its
`/JS` stream go, the page a removed action's `/Next` pointed at stays, and no
reference is left dangling. An orphan the file carries — a script object
nothing names, an embedded file stream, a metadata stream — is found by what
it is and goes too. The report accounts for every change:
`SanitiseReport::removed` names each entry taken out of an object that stays
(its `EntryHolder` — an object or the trailer — and the `PathStep`s to it)
and `deleted` each object deleted, both with a `Removal` saying why, and an
object in neither is exactly as it was. As with redaction, **save with
`WriteMode::Rewrite` for the removal to be real**: an incremental update
appends and the original objects are still in the prefix.

**Stamps and watermarks.** `add_resource(page, category, prefix, object)`
registers an object in a page's `/Resources /<category>` under `prefix` and
the smallest number the page's sub-dictionary does not already use, and
returns the name. The page's resource dictionary may be inherited from the
page tree (7.7.3.4) or shared by reference with other pages, so the
*effective* dictionary is copied — with the category's sub-dictionary — and
the copy written onto this page; the shared or inherited original is never
changed. `stamp(page, form, StampPlacement::Over | Under)` registers a form
XObject that way and adds a stream invoking it to the page's `/Contents`
array — before the page's streams for `Under`, after them for `Over` — and
the page's own streams are not rewritten or copied, so an incremental save
carries the page dictionary and the new streams and nothing else. An `Over`
stamp runs in whatever graphics state the page's content left behind, and
inside whatever it left open, so the content is tokenized first (an inline
image's data skipped to its `EI` by the interpreter's rule). If every `q` has
its `Q`, every `BT` its `ET`, every `BMC`/`BDC` its `EMC`, and nothing outside
a `q` changes the state, the stamp is simply appended. Otherwise the stamp's
stream first closes what the content left open, innermost first — so an
`/OC … BDC` a producer forgot to close does not hide the stamp with its layer
— and when the content changed the state outside any `q` of its own, a `q`
stream goes before the page's streams and one more `Q` after them, so a `cm`
before an unclosed `q` is undone too. "Changes the state" includes `TD` and
`"`, which look like positioning and set the leading and the spacings (Table
108). Content that cannot be followed — a stream that will not decode, a `Q`
with nothing to restore — gets one `q` before and one `Q` after, which
isolates the stamp only when that content's own operators balance. An `Under`
stamp needs no bracket: it runs in the initial state and `Do` restores
whatever the form changes (8.10.1). `add_form(&FormXObject, resources)` writes a form in the
editor, and `import_page_as_form(source, page, matrix)` turns another
document's page into one — its content joined, its crop box the `/BBox`, its
resources deep-copied by the copy `import_page` uses.

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
and is wrong four ways, which `emit_array`'s doc comment sets out. A
**vertical** run (9.7.4.3) is cut down its own column by the same test: the
pen walks text-space y by `/W2`'s `w1` (signed, and with no horizontal scale
in it), the glyph box is the horizontal one stood on end about the pen —
`-v_x` to `w0 - v_x` across, where the interpreter draws it — and a `TJ`
number or a removed glyph's gap displaces along y in thousandths of `Tfs`
alone. Until September 2026 a vertical run was left whole as `VerticalRun`,
and before that it was cut as if it were horizontal (`redact.rs`'s
`vertical_runs` module, against an `Identity-V` font whose `/W2` gives one
glyph a different advance, so a gap of the wrong length moves every glyph
after it). A **Type 3** run is measured in the font's own glyph space
(9.6.5): the advance is the horizontal component of the width carried
through `/FontMatrix` (`w0 · a`, what the interpreter advances by), and the
glyph box is a glyph-space rectangle — `0` to `w0` along, the `/FontBBox`
height joined with one em across — carried through all six numbers of the
matrix, so a scaled, skewed, rotated or translated glyph space is cut where
its procedures draw. Until September 2026 any matrix but the 1/1000 default
was left whole as `RescaledType3Font` (`type3_glyph_space`, where the skewed
and rotated fixtures each name the neighbour an upright box would have cut).
A glyph
the rectangle covers only *partly* is removed, because a content stream can
show a glyph or not show it and only one of those two can leak. The page is
read **as the editor has it** — its place in the editor's page order, its
content as the editor now holds it, and `/Resources` inherited through the
page tree when the page has none (7.7.3.4). Until September 2026 it was read
out of the file, which made a second redaction of a page put back what the
first had removed, redacted the file's page *n* when the editor had moved
pages, and silently skipped every form and image on a page whose resources
were inherited (`redact.rs`'s `editor_reads` module, one test each). What
redaction cannot **measure** it still leaves whole and names in
`RedactionReport::warnings` — two of that type's three classes, in the
refusal table below; the third is the form placement one further down —
because a redaction that silently fails to redact is worse than one that
refuses: the caller believes the content is gone and distributes the file.
A warning says the run was not measured, not that it was covered, so
warnings are raised only when there is at least one rectangle to fall under.
Form XObjects are rewritten recursively, each
resolving names against its own `/Resources` (8.10.1), because forms are how
most producers place repeated content and a redaction driven straight
through one would leave the secret in the form. A rewritten form is written
back as plain operators with the encoding keys of its dictionary dropped
(`/Filter`, `/DecodeParms`, `/DL` and the external-file keys): until
September 2026 a compressed form kept `/Filter /FlateDecode` over bytes that
were never deflated, and the saved form drew nothing at all
(`a_compressed_form_is_written_back_as_a_stream_that_decodes`, which holds
the saved file to the strict validator). A form drawn at several placements
is **cut exactly at each**: every placement is measured against the form as
it was, and each distinct outcome gets a stream of its own — a copy of the
form cut in that placement's frame, which the placement's `Do` is pointed at
through a fresh resource name (`Rd` and the copy's object number) in a
resources dictionary of the page's, or of the enclosing copy's, own. A form
drawn inside a copied form is decided first, because a form whose `Do` names
a copy is itself a different outcome. Placements that cut the same share one
stream, and a form nothing was cut from is not written at all. The form's
own object keeps an uncut outcome when there is one, so another page that
draws it draws it as it was. When every placement cut something, the object
is left as it was if anything else **draws** the form — another page, a form
or an annotation appearance there, a Type 3 glyph's procedure anywhere — and
every placement on the redacted page draws a copy; only when nothing else
draws it does it take the first placement's outcome, so it is never left in
the file holding covered text with nothing drawing it. Drawn rather than
named, because one `/Resources` shared by every page names every form, and
counting that would leave exactly such a stream behind. That read is made
once per redaction, and only when such a form arises; until October 2026 it
was not made at all, and a page sharing the form lost what the redacted
page's rectangles covered (`redact.rs`'s `forms_elsewhere`, a page two that
draws the form directly, through a form of its own, as an annotation's
appearance and from a glyph procedure, and one whose resources only name
it). The
guard that stops a self-referential form recursing is keyed by the transform
as well as by the object — bitwise and not by tolerance, because two
transforms an ulp apart are two placements and calling them one is a
decision not to cut — and what bounds the walk is a cap on placements per
form (`MAX_PLACEMENTS`) rather than any comparison of floats. This is the
third form of this answer in September 2026: first the guard was keyed by
the object alone and only the first placement was measured (a silent
under-redaction reporting `glyphs: 0`); then every placement was cut in the
one stream they share and `RedactionWarning::RepeatedForm` named the widened
cut; now `RepeatedForm` is raised only for the two kinds of form that still
go that way, with everything they draw — one that draws itself, directly or
through another, and one placed past the cap (`a_form_whose_placements_are_cut_differently_is_cut_exactly_at_each`,
and the tests beside it in `redact.rs`'s `tests`). The subsetter walks the
editor's view, where the copies resolve, so a glyph drawn only in a copy
stays in the program (`subset.rs`'s `redacted_copies`). An image a
redaction touches is scrubbed whole to a blank sample: cutting a hole would mean decoding,
editing and re-encoding through a codec this build may have no encoder for,
and leaving the rest is not a redaction — an inline image (8.9.7) as much as
an XObject, and a stencil mask to a stencil sample that paints nothing. An
inline image the rectangles do not touch is written back byte for byte:
until September 2026 the rewrite tokenized its samples like the rest of the
stream and wrote back whatever tokens they spelled, corrupting every inline
image on a redacted page and scrubbing none (`an_inline_image_is_carried_through_a_rewrite_byte_for_byte`,
`an_inline_image_under_a_redaction_is_scrubbed`). An **annotation's
appearance** (12.5.5) is a form the page draws over itself, and is cut as
one: every appearance an annotation on the page can show — each of `/N`,
`/R` and `/D`, every state of each whatever `/AS` selects, and a hidden
annotation's too — is a placement of its form at the transform the renderer
draws it with, the `/BBox` carried through the form's `/Matrix` and fitted
onto `/Rect`. One annotation's appearance is cut in place; one that
annotations share and that is covered under one of them gives that one a
copy, through an `/AP` of its own, since a shared `/AP` dictionary draws for
the others too. The annotation is rewritten rather than removed. Until
October 2026 appearances were not read at all, and a FreeText note under a
rectangle stayed on the page (`redact.rs`'s `appearance_streams`, every test
read back flattened, by extraction, and by ink). A **Type 3 glyph whose
procedure draws text or an image** (9.6.5) is measured through the
procedure, under the transform the interpreter runs it with, because the
procedure can draw far outside the glyph's own box; a use whose procedure
draws under a rectangle is removed whole, as a partly covered glyph is, and
the procedure is left as it was — it is the font's, and every other use of
that glyph runs it (`glyph_procedures`, which also follows a procedure into
a form it draws and bounds one that shows its own glyph). `mark` paints the area black
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
sees — through `DocumentEditor::view`, the editor's state as a document of its
own, so the page order it has, a page it inserted and a copy of a form a
redaction made all resolve (until September 2026 the walk resolved names
through the file with the editor's bytes substituted, and could not see a
copy). Run the other way round it would keep exactly what the redaction removed.
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
`add_name_tree()`, `add_number_tree()`, `add_named_destination()`,
`set_layer_visible()`,
`set_trailer_entry()`, `set_info()`,
`view()`,
`transaction()`, `checkpoint()`, `restore()`, `page_refs()`,
`delete_page()`,
`move_page()`, `rotate_page()`, `set_crop_box()`, `insert_page()`,
`import_page()`,
`keep_pages()`, `append_content()`, `add_resource()`, `add_form()`,
`import_page_as_form()`, `stamp()`, `page_box()`, `flatten_annotations()`,
`add_annotation()`, `set_page_boundary()`, `set_trim_box()`, `set_art_box()`,
`set_bleed_box()`, `set_page_labels()`, `attach_file()`, `set_outline()`,
`set_title()`, `set_author()`, `set_subject()`, `set_keywords()`,
`set_creator()`, `set_producer()`, `set_creation_date()`,
`set_modification_date()`, `set_trapped()`, `set_xmp_metadata()`,
`set_viewer_preferences()`, `sanitise()`, the [forms](forms.md) methods, and
`save(&WriteOptions) -> Vec<u8>`. The document operations' types are on the
facade beside it: `PageLabelRange`, `PageLabelError`, `LabelStyle`,
`EmbeddedFile`, `AttachError`, `MetadataSync`, `ViewerPreferences` and its
enums, `PageBoundary`, `TreeWriteError`, and for sanitising `Sanitise`,
`SanitiseReport`, `RemovedEntry`, `DeletedObject`, `EntryHolder`, `PathStep`
and `Removal`. `redact::{Redaction, RedactionReport, RedactionWarning, apply}`
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
| Redacting a run whose `Tf` named a font the resources in scope do not have | left whole, `UnknownFont` (`a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported`) | no metrics at all, so no glyph can be placed. Permanent, and it was silent before: the run was kept, nothing was counted, and the report looked like a rectangle that covered nothing | — |
| Redacting a run whose text rendering matrix is not finite | left whole, `UnmeasurableFrame` (`a_non_finite_text_matrix_is_left_uncut_and_reported`) | a position that is not a number cannot be compared with a rectangle. Permanent. The whole showing operand is left, never half of it | — |
| Partial image redaction | the whole image is scrubbed (`RedactionReport::images`) | a hole needs a re-encode through a codec this build may not write | [filters](filters.md) |
| Cutting exactly, a copy per placement, a form that **draws itself** (directly or through another form) or one **placed past `MAX_PLACEMENTS`** — and every form either draws | the cut is the union over every placement, in the one stream, and `RedactionWarning::RepeatedForm` names the form and its placement count when a cut was made or a placement went unmeasured (`a_self_referential_form_under_a_moving_transform_terminates`, `a_form_drawn_by_one_placed_past_the_cap_is_cut_the_old_way_too`) | a copy per placement of a form that draws itself would be a copy per round of a recursion, and a placement past the cap was never measured, so no copy could say what it should hold; what such a form draws goes the same way because its one stream names its children by their own objects. Over-removal is the direction this module errs in everywhere; the alternative here is the leak | 8.10 |
| Redacting what a **tiling pattern's cell** or a **soft mask's group** draws | not read, and the report does not say so: the walk follows `Do`, annotation appearances and Type 3 procedures, and a cell (8.7.3.1) or a mask's group (11.6.5.2) is reached through `scn` or `gs` instead; the read of what else draws a form does not follow them either | a cell is painted at every tile of whatever it fills, so cutting one is a form drawn at as many placements as the fill has tiles, which is a design rather than a fix. A [roadmap](../ROADMAP.md) row (Editing) | 8.7.3, 11.6.5 |
| Measuring more than `MAX_PLACEMENTS` distinct placements of one form | the count in `RepeatedForm` saturates at the cap, which is how a caller tells "too much went" from "something may have survived" (`a_form_placed_more_times_than_the_cap_saturates_its_count`) | a form that invokes itself under a matrix that moves each round makes a fresh placement every time; a count bounds it, where a tolerance on matrices would have to be loose enough to call two real placements one | ruling 1 |
| Appearance synthesis for other subtypes | `add_annotation` inserts the dictionary; no `/AP` is generated | seven subtypes cover the common producer gap; others render only if they carry their own `/AP` | — |
| Rewriting a Type 3 glyph's procedure when it draws under a rectangle | the **use** is removed whole and the procedure is left byte for byte (`a_glyph_whose_procedure_shows_text_under_a_rectangle_is_removed_at_that_use`), so a procedure that shows the covered words still says them in `/CharProcs` — after the default save too, because `subset::apply` cuts embedded programs and not Type 3 fonts | the procedure is the font's: every use of the glyph on every page runs it, so cutting it would cut every use, and there is no copy to give the uncovered ones short of a new glyph in the font. Dropping a procedure no use is left drawing is the [roadmap](../ROADMAP.md) Editing row's | 9.6.5 |
| Measuring a glyph procedure that shows glyphs whose procedures show glyphs, past `MAX_PLACEMENTS` streams for one use | the use is removed as though covered, and nothing reports it (`a_glyph_procedure_that_shows_its_own_glyph_ends_and_errs_toward_removal`) | a procedure can show its own glyph, and a face that branches makes the measurement exponential; the budget is per use (`every_use_of_a_glyph_has_a_budget_of_its_own`), so only such a face reaches it | ruling 1 |
| Removing an annotation's own text — `/Contents`, a rich-text `/RC`, a field's `/V` — when its appearance is cut | left as it was; only what the annotation draws is cut | what a redaction measures is what a page draws, and these have no position to compare with a rectangle. Deleting an annotation outright is the caller's decision, through the editor | 12.5 |
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
| Keeping `/Info` and the XMP packet in step | each `/Info` setter and `set_xmp_metadata` returns `MetadataSync::OtherHalfUnchanged` when the other half exists and was left as it was (`a_caller_supplied_packet_is_written_verbatim_and_uncompressed`) | `tinker-pdf-cos` neither parses nor rewrites XML, and deriving `/Info` from a caller's packet would be a second XMP reader; the caller who is told is the one who can make them agree | [document model](document-model.md) |
| Page labels with no range at page 0, past the last page, numbering from 0, or two at one page | `PageLabelError`, nothing written (`page_label_refusals_write_nothing`) | 12.4.2 requires page 0's entry and Table 159 a `/St` of at least 1; which of two ranges at one page the caller meant is theirs to say | 12.4.2 |
| A second attachment under a name already filed | `AttachError::NameTaken`, nothing written | two entries under one key is a tree a reader resolves by whichever it reaches first | 7.9.6 |
| A MIME type that cannot be a name, or a date 7.9.4 cannot spell | `AttachError::MimeType`, `AttachError::Date`; `set_creation_date` returns `None` | a `/Subtype` with a space in it is not a MIME type, and a year of five digits is not a PDF date | 7.9.4 |
| A viewer preference page range from page 0 or backwards, or zero copies | `set_viewer_preferences` returns false (`ViewerPreferences::is_writable`) | Table 147 numbers pages from 1; which of two numbers the caller meant is theirs to say | 12.2 |
| Sanitising the bytes an incremental save keeps | nothing is refused; the original objects stay in the prefix, as redaction's do | 7.5.6: an update appends. The report says what left the *document*; only a rewrite makes that what left the *file* | [writing](writing.md) |
| A link left with neither `/A` nor `/Dest` after sanitising | the link leaves its page's `/Annots` with its action, for its action's reason (`actions_alone_leave_the_scripts`) | 12.5.6.5: a link exists to be followed, and the strict validator refuses one that goes nowhere | 12.5.6.5 |
| Removing an element from an array other than one in an action slot or `/Annots` | never done; an action found there stays | a name tree's `/Names` is positional pairs, and one element out would shift every key onto the wrong value | 7.9.6 |
| Removing an action's `/Next` successors that are not themselves removed | they go with the action when nothing else reaches them | the chain is part of the action (12.6.2); what else still reaches stays | 12.6.2 |
| Reading an XFA form's scripts to decide what to keep | `/XFA` is removed whole under `javascript` (`Removal::XfaForm`) | XFA is a named non-goal and its packets carry `<script>` elements this engine does not parse | [forms](forms.md) |
| Paying for the walk on a save that changed one annotation | `FontPolicy::Keep` on `write::save`, or `DocumentEditor::save`, which is unchanged | the pass is whole-document and order-dependent and costs a full interpretation of every page. The default is still `Subset`, because forgetting costs a disclosure and paying costs time | [writing](writing.md) |

## Verified

- `crates/tinker-pdf/tests/editor_stamp.rs` — a stamp over paints on top and
  one under beneath; the page's own content streams are the same objects,
  unredefined by the update, with the same bytes; two pages sharing one
  `/Resources` are stamped independently, neither touching the shared object
  and the new name stepping past an existing `Stamp0`; a page ending with a
  stray `cm` is bracketed and the stripe lands at the page's scale, and so
  does one whose `cm` precedes a `q` it never closes; a hidden layer the page
  leaves open is closed before the stamp, which shows; a page whose text used
  `TD` is bracketed, so a stamp positioned with `T*` lands where it would on a
  blank page, while one that used `Td` is not; inherited
  resources are copied onto the page and the tree node is not in the update;
  another document's page stamps as a form with its image; every saved file
  is clean under the strict validator.
- `crates/tinker-pdf-cos/tests/page_operations.rs` — delete, move, rotate,
  insert, import, keep, append; each saved in both modes, because a page
  operation once reached only the incremental set and nothing caught it.
- `crates/tinker-pdf-cos/tests/form_transactions.rs` — `transaction`
  restores the first four fields (the fifth, the trailer entries, is
  `trailer_overlay.rs`'s); injection puts each restore under exactly one
  test.
- `crates/tinker-pdf/tests/annotation_appearances.rs` — synthesised
  appearances render and flatten.
- `crates/tinker-pdf/tests/editor_docops.rs` — the document operations, from
  outside the crate: each setter's output saved **incrementally and as a
  rewrite**, reopened through `Document::open`, read back through the public
  reader (`page_labels`, `attachments` and the stream's own bytes and MD5,
  `outline` with its destination kinds, `metadata`, `xmp_metadata`,
  `viewer_preferences`, `Page::trim_box` and its siblings) and handed to the
  strict validator, which must find nothing. Replacement deletes the old
  structure; every refusal leaves the editor as it was; every setter on one
  editor composes through the catalog. Sixteen defects put back one at a time
  (`cargo test --no-fail-fast` over this file and the crate's unit tests):
  production boxes inherited from `/Pages` 1, not clipped to the media box 2,
  a date's zone sign flipped 3, the 1.x closing apostrophe on the wrong
  version 1, the metadata stream compressed 1, a `None` preference left
  standing 1, `/Direction`'s two names swapped in reader and writer alike 2
  (the reader's own tests; a round trip cannot see a consistent swap), the
  old outline or label tree orphaned rather than deleted 1 each, the checksum
  taken over the wrong bytes 1, a label style's case 2, a closed entry's
  `/Count` sign 3, the page-0 rule 1, an `/Info` write that never reports the
  packet 1, `/Trapped` 1, a taken attachment name 1.
- `crates/tinker-pdf-cos/tests/editor_structures.rs` — replacing a structure
  on a file whose old one is damaged or written in place, each saved both
  ways and held to the strict validator: an old outline's dangling `/Next` at
  the number that becomes the new outline's root, a page inserted earlier, or
  an object a caller has put and not linked, deletes none of them; a `/Next`
  into a page and a label tree's `/Kids` into the page tree delete no page;
  an outline and a label tree written directly into the catalog have their
  nodes deleted; an attachment tree written directly into `/Names`, with its
  leaves in place or as objects of their own, keeps its file beside the new
  one; and a UTC offset no zone has, `i32::MIN` included, is refused by
  `set_creation_date`, `set_modification_date` and `attach_file` rather than
  panicking, with the widest offsets the syntax spells still written. Each of
  the ten failed against the code before the fix. No injection campaign is
  counted for this file yet: running the suites against a put-back defect
  was refused by the permission policy of the session that wrote it, so the
  evidence is the before-and-after run alone, and a campaign is owed.
- `crates/tinker-pdf-cos/tests/sanitise.rs` — one hand-written fixture with
  every kind in every place a viewer looks (listed in the file's header) and
  what must survive beside them. After `Sanitise::ALL`, saved both ways:
  `script_summary` is empty, no attachment, packet or `/Info` is left, the
  page and annotation `/AA` the walkers miss are asserted by hand, the named
  destinations, the `/GoTo` in the same `/AA` and the viewer preferences
  stay, the strict validator passes, and a rewrite's bytes hold none of the
  scripts, file contents, metadata or URLs. The report is checked against
  the object sets themselves: every object that differs is a holder or
  deleted, every reported path was there before and is gone after, and an
  object named nowhere is unchanged. Each switch is run alone against the
  same fixture, and a second pass finds nothing. Seventeen defects put back:
  `/S /JavaScript` not recognised 3, a rendition's `/JS` 1, a padded
  `javascript:` URI 1, the `/P`/`/K` exclusion 1, `/Next` not swept 2, a
  targetless link kept 4, `/CO` kept 3, `/Names /JavaScript` kept 3,
  `/Metadata` kept 3, `/EF` kept 2, nothing deleted 5, deletion ignoring what
  still reaches 3, a cleaned stream losing its data 1, the trailer's `/Info`
  kept 3, a null trailer entry written rather than dropped 2, orphans not
  found 2, one removal per object left unreported 5. Added after review:
  `an_action_dressed_as_something_else_is_still_taken_out` — an `/AA` entry
  with an extra `/K`, one with `/Type /Whatever`, an `/OpenAction` whose `/S`
  is a reference, a navigation action with `/P` whose `/Next` is script, and
  an indirect action with an odd `/Type` whose `/Next` is an indirect array,
  all taken out by `Sanitise::ALL` with `script_summary` empty after, beside
  a structure element role-mapped to `URI` that stays;
  `an_imported_page_keeps_its_content_when_its_link_goes` — the content and
  font of a page `import_page` added survive sanitising its link, and both
  saves draw what the source drew; and
  `info_taken_out_does_not_come_back_from_an_earlier_trailer` — an
  incremental update writes `/Info null`, so an `/Info` something else still
  names does not come back through the earlier trailer. Injections: the
  strict test used in action slots fires 2, an `/S` reference not resolved 2,
  an indirect object's slot not found 1, the page order left out of what is
  reached 1, the null entry dropped from an update's trailer 1.
- Redaction tests live beside `crates/tinker-pdf/src/redact.rs`: multi-page
  fixtures (a two-page file once redacted page 0's image and left page 1's
  secret), text inside form XObjects, a self-referential form that
  terminates and one that does so under a transform that moves each round,
  a form drawn twice cut at the placement the rectangle covers and at both
  when both are covered, two placements cut differently each cut exactly at
  its own rectangle (read back by extraction and by ink), placements that cut
  the same sharing one copy, a nested form measured and copied at every
  placement of its parent, a second redaction cutting the copy the first
  made and keeping a form's in-place cut, a form another page draws left
  whole, a form placed past the cap cut the old way with everything it
  draws, an image drawn twice scrubbed from its second placement, scaled
  runs, and the needle-bytes-absent assertion over every
  decompressed stream. Three further modules carry the rotated cut: a
  quarter turn, an oblique rotation, a skew, a rotation that lives in the
  `cm` rather than the `Tm`, and the matrix and the `TJ` gaps re-emitted in
  the run's own units (`rotated_runs`); a vertical column cut exactly at the
  covered glyphs with every kept glyph extracted where it was, the gap
  emitted down the column in the vertical thousandth, a `TJ` number keeping
  its axis, the box centred on the pen, and a column turned a quarter turn
  (`vertical_runs`); a Type 3 glyph space in hundredths, skewed, rotated
  and translated, each cut at the glyph the arithmetic says the rectangle
  covers with every other pixel of the page rendered as before, a malformed
  `/FontMatrix` read as the renderer reads it, and a `/FontBBox` that
  descends or understates (`type3_glyph_space`); `'`, `"` and an existing `TJ`
  adjustment surviving a cut (`showing_operators`); and one test per
  unmeasurable-run `RedactionWarning` variant (`refusals`) — the third
  variant, `RepeatedForm`, is about a form rather than a run and its tests
  sit with the other form ones. Their fixtures are a Type 3 font
  whose every glyph fills its em square, so the geometry a test computes by
  hand from 9.4.2 to 9.4.4 and the ink the renderer draws are the same
  rectangle — which is what lets each of them assert the safety property at
  both levels, the covered glyph's code absent from every stream **and** no
  ink inside the rectangle.
- Three more redaction modules draw in the vendored Liberation Serif, so what
  they cut is read back by the extractor as well as by ink. `forms_elsewhere`:
  a form cut at every placement on page one, left whole for a page two that
  draws it directly, through a form of its own, as an annotation's
  appearance or from a glyph procedure — page two extracting and rendering
  exactly as before — and cut in place when page two's resources only name
  it. `appearance_streams`:
  an annotation's appearance cut where 12.5.5 fits it onto `/Rect`, a
  quarter-turned one where its `/Matrix` turns it, both states of `/N` and of
  `/D`, the `/R` and a hidden annotation's appearance all cut, one appearance
  two annotations share cut only under the one covered (whether that one is
  an object or written into `/Annots`), one no rectangle touches left as it
  was, and one with no `/Resources` measured in the page's.
  `glyph_procedures`: a Type 3 glyph whose procedure shows text, draws an
  inline image or draws a form, removed at the use that draws under a
  rectangle and kept at the others with the procedure byte for byte; one
  that shows its own glyph ending; every use with a budget of its own; a
  procedure's text in a font no scope has reported; and a glyph in a form
  drawn twice measured at each placement.
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
