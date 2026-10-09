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
multiple of 90, stored as `/Rotate`; any other turn is refused and changes
nothing, where it used to be rounded to the nearest quarter —
`a_turn_that_is_not_a_quarter_is_refused_and_changes_nothing`), `set_crop_box` (14.11.2, written as the
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
`appearance.rs` synthesises an appearance stream for thirteen subtypes, so
a viewer that draws only `/AP` (most of them) shows the annotation — eleven
because the dictionary determines the appearance, and `Text` and `Caret`
because this module invents one — and declines the fourteen
`appearance::UNDETERMINED_SUBTYPES` names, with its reasons in the refusal
table below. That list changes nothing a caller sees — a subtype on it is
declined exactly as an unknown one is — and declining them is not a
ruling: `Text`'s `/Name` icon is the reader's (12.5.6.4) exactly as a
stamp's, a file attachment's and a sound's are (12.5.6.12, .15, .16), and
`Text` is drawn, as an invented sticky note, so whether those three get
invented icons too is owed to the owner (the ROADMAP's Editing row (d)).
The first seven — `Highlight`, `Underline`, `StrikeOut`, `Square`,
`Circle`, `Text` and `Link` (which draws none: 12.5.6.5's convention is no
border) — are drawn byte for byte as they were before any other was added
**for a dictionary that carries none of the entries they did not read
before** — `/CA`, `/ca`, `/RD`, a `/BS /S /D` or a `/Border` dash — and
none that is malformed: `the_seven_first_subtypes_are_drawn_exactly_as_they_were`
pins the dictionaries the editor's own constructors make, which carry
none. With one of those entries they draw what it says — under `/CA` an
underline is preceded by `/GS0 gs` and gains an `ExtGState`, and a square
with `/RD [5 4 3 2]` is drawn at `16 23 90 32` rather than `11 21 98 38`
(`the_first_seven_draw_what_an_entry_they_did_not_read_says`) — and with
a malformed entry, one they read before included, they are declined, as
every subtype is (below). Since October
2026 a **`Line`** (12.5.6.7) is drawn too: `/L` in `/C` at the `/BS` width
and dash, Table 176's ten endings filled with `/IC`, and Figure 60's leader
lines from `/LL`, `/LLE` and `/LLO`; the endings go at the ends of the line
proper, which the leader lines lift off `/L`, and with no leader lines `/LLO`
— the gap before they begin — offsets nothing. Table 176 names each ending's shape and
not its size; each is drawn three line widths (at least three points) from
its point, so a square ending is six widths across and an arrowhead six
widths long. An absent `/C` strokes a line-like annotation black, as
`Underline` always has; an empty one is 12.5.2's "transparent". A
**`Square`** or **`Circle`** (12.5.6.8) is drawn inside Table 177's `/RD`
when it has one — the left, top, right and bottom differences between
`/Rect` and the shape, each at least zero and together leaving the shape a
width and a height — and its border is dashed as a line's is. A
**`Polygon`** (12.5.6.9) is its `/Vertices` joined, closed and filled with
`/IC`; a **`PolyLine`** is the same path left open, with a line's `/LE`
endings at its first and last vertex, facing along its end segments (past a
vertex written twice), and its `/IC` filling only those. Both stroke black
when `/C` is absent, as a line does, and need two distinct vertices to draw
anything. A **`Squiggly`** (12.5.6.10), the fourth text markup, is a zigzag
under each quad's text in the quad's own frame — along its baseline, so a
quad on turned text gets a turned zigzag — in a band from 3% to 3% + ⅙ of
the quad's height, its strokes at forty-five degrees and as thick as an
underline's, and black when `/C` is absent or empty, as an underline is. It
is drawn without a vertex per tooth: the band is clipped and each set of
parallel strokes is the dashes of one wide diagonal line, so a quad costs the
same fourteen operators however long it is, where a loop over teeth would let
eight numbers — a quad a kilometre long on a point of text — ask for a
million. The lines are diagonal in the quad's frame rather than straight
under a shear because a renderer that strokes in device space with one
scale for the width squares a sheared dash back into a vertical bar — which
this engine's did until October 2026, a defect [rendering](rendering.md)
records as fixed; the squiggly keeps the construction that stays clear of
it, which is right under either reading and under other renderers too. A **`Caret`** (12.5.6.11) is the typographic caret, filled in `/C`
(black when absent) inside `/Rect` less Table 180's `/RD`: a spike from the
middle of the bottom edge to the middle of the top, its sides cubics bowed
in from the bottom corners. 12.5.6.11 names the symbol and not its outline,
so the outline is this module's choice and is written down here — an
invention, as `Text`'s note is. An
**`Ink`** (12.5.6.13) strokes each path of `/InkList` in `/C` (black when
absent) at the `/BS` width and dash, through every point with straight
lines — Table 182 leaves "straight lines or curves" to the implementation —
and round caps and joins, the shape a pen leaves; a path of one point is a
dot, and the paths are never joined to each other. A path written as a
reference is drawn once however often the list names it, so a small file
cannot ask for one large array many times over. A **`FreeText`** (12.5.6.6)
is drawn when its `/DA` names a font the interactive form's `/DR` holds as a
simple font — Type 1 or TrueType, a byte a glyph, and not symbolic — and
every character of `/Contents` is one that font's encoding (9.6.6) has a
byte for: each is written as the lowest byte the encoding — `/BaseEncoding`
and `/Differences`, StandardEncoding when the font names none — gives that
character, and that `/ToUnicode`, where it maps the byte, agrees with, and
measured by that byte's width. So `é` is 0xE9 in a WinAnsi font and refuses
the annotation in a StandardEncoding one, which has no `é`, and a straight
quote is 0xA9 there, where 0x27 is a right quote. Its box
is `/Rect` less `/RD`, filled with `/C` (12.5.2's "background of the
annotation's icon", which a free text annotation's box is) and bordered at
the `/BS` width and dash in the text's colour, since ISO 32000-1 names no
other; the text is laid out as a multiline field's is — two units in from
the border, wrapped at the last space that fits (or between characters, for
a word wider than the box) and at each line break, the spaces it breaks
at belonging to neither line, the first baseline 0.85
of the size below the top and the lines 1.15 apart, aligned by `/Q` and
clipped to the box — in the `/DA` font, size and colour, a size of zero being
the largest whole size from twelve down to four at which every line fits. A
line wholly below the box is not written, nor any after it, and each line
after the first costs its text and at most one number (`TL` once, then `T*`),
so that the stream grows with what the box shows rather than with a large
number repeated once a line; the lines are laid out one at a time and the
layout stops where the writing does, so a long `/Contents` in a box
narrower than a glyph costs the lines the box shows, and one with no room
inside its border is not laid out at all.
Only the font, the size and a `g`, `rg` or `k` colour are read from `/DA`;
the rest of the string is not copied into the appearance, where a `Q` or an
`ET` of the producer's would unbalance it. `/DA` is read with the object
lexer, as the content stream it is (7.2): the font's name is `#`-decoded to
find it in `/DR` and written back escaped as the resource key is, a
delimiter ends a name, and a string that needs any leniency to lex — an
exponent, an unterminated string, a stray delimiter — is not read, and the
annotation is given no appearance. Under `/IT /FreeTextCallout`,
Table 174's `/CL` callout is stroked as the border is, from the point it
calls out to the box, with `/LE`'s ending at that point. Every
synthesised appearance carries 12.5.6.2's `/CA` (and ISO 32000-2's `/ca` for
what is filled) as the `ExtGState` it selects, since a renderer reads
opacity from the content and not from the annotation.
An entry is read only when it is what its table says it is: one that is
present and is not — `/L`, `/Vertices`, an `/InkList` path, `/CL`, `/RD`,
`/QuadPoints`, a colour or `/Rect` holding anything but finite numbers
within Annex C's real (±3.403 × 10³⁸), or the wrong count of them; an `/RD` Table 177 forbids; a dash 8.4.3.6 refuses (a
negative or non-finite element, or every one zero) or a `/BS /S` Table 166
does not name; an `/LE` that is not names Table 176 lists; a negative
`/LLE` or `/LLO`; a `/CA`, `/ca`, `/LL` that is not a number; a `/Q` other
than 0, 1 or 2; a negative `/DA` size — declines the appearance rather than
being read past (`a_malformed_entry_declines_the_appearance`). Until the
October 2026 review a bad element was dropped and the rest re-paired, so
`/L [10 (x) 50 90 50]` drew a line from (10, 50) to (90, 50) that is
nowhere in the data, and an unlisted ending, a refused dash and a forbidden
`/RD` were each drawn as something the producer did not write. Declining
repairs nothing, so it is not a leniency with a warning to emit; the
annotation is left as it came, as one of an unknown subtype is. Constructors for the common
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
in it), the glyph box is the horizontal one stood on end where the
position vector puts the glyph — `-v_x` to `w0 - v_x` across, and along the
column the glyph's cell joined with the default cell measured from its
horizontal origin `v_y` below the pen, where the interpreter draws it (until
October 2026 `v_y` was not read, and a glyph whose `/W2` moved it off the
cell below the pen was measured where it was not drawn) — and a `TJ`
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
were inherited (`redact.rs`'s `editor_reads` module, one test each). Its
**fonts** are read through the editor too, since October 2026: every object a
font reaches — dictionary, descriptor, widths, encoding, CMaps, a Type 3
face's procedures and resources — through `cos::font::from_resources_in`, the
font loader made generic over `Resolve`, so a run in a font only the editor
holds (a page `import_page` copied in, a font object a caller wrote) is
measured and cut rather than left whole as `UnknownFont`
(`a_run_in_a_font_only_the_editor_holds_is_cut`). What
redaction cannot **measure** it still leaves whole and names in
`RedactionReport::warnings` — two of that type's six classes, in the
refusal table below; the third is the form placement one further down, the
fourth a stream of more XObjects than one stream's walk follows, the
fifth a tiling pattern that shows text or draws an image, and
the sixth a glyph procedure whose measurement ran out of budget —
because a redaction that silently fails to redact is worse than one that
refuses: the caller believes the content is gone and distributes the file.
A warning says the run was not measured, not that it was covered, so
warnings are raised only when there is at least one rectangle to fall under.
Form XObjects are rewritten recursively, each
resolving names against its own `/Resources` (8.10.1), because forms are how
most producers place repeated content and a redaction driven straight
through one would leave the secret in the form — to at least the sixteen
levels of nesting the interpreter draws (until October 2026 the walk
stopped at thirteen, and text fourteen to sixteen forms down was drawn and
never measured: `text_as_deep_in_forms_as_the_renderer_draws_is_redacted`,
and `the_renderer_draws_no_deeper_than_the_walk_measures`, which pins the
two limits together). A rewritten form is written
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
it). The read follows a form again when it meets it shallower than before,
since one met past the depth the read stops at was read for nothing — until
October 2026 the first meeting was the only one, and a form page two drew
both deep and shallow was not followed into (`a_form_met_deep_and_then_shallow_on_another_page_is_read`).
A form cut the old way (below) is cut in place for everything that draws
it, so when something else does, the widened cut is named `RepeatedForm`
even at one placement here (`a_form_cut_in_place_that_another_page_draws_is_named`;
until October 2026 it was named only for a form placed twice here). The
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
`an_inline_image_under_a_redaction_is_scrubbed`). One a **form** or an
appearance draws is scrubbed the same way: until October 2026 a form's cut
was written back only when it removed a glyph, so an inline image a form drew
once, or drew at placements every one of which a rectangle covered, kept its
samples and its ink, and the report said `images: 0` (`an_inline_image_a_form_draws_under_a_redaction_is_scrubbed`).
An image already blank is not counted again. An **annotation's
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
a form it draws and bounds one that shows its own glyph). A procedure is
measured in the scope that showed the glyph, where this engine runs it, and
in the font's own `/Resources`, where 9.6.5 puts what it names — each way
round, so a name only one has resolves and one they bind differently is
measured as each binds it; until October 2026 a `Do` only the font's
resources named was passed over in silence (`a_procedure_is_measured_in_the_fonts_own_resources_too`). `mark` paints the area black
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
(9.6.5); **every** appearance stream under an annotation's `/AP` — `/N`,
`/D` and `/R`, and every state of each, whatever `/AS` currently selects, since
12.5.5 lets a viewer switch states with no edit to the file and a subset cut to
today's state loses tomorrow's tick; and a tiling pattern's cell or a soft
mask's group anything of those paints with (8.7.3.2, 11.6.5.2), each in its
own `/Resources` or the scope that painted, which the interpreter runs
neither of — until October 2026 neither was walked, and a glyph shown only in
a cell was dropped from its program and drew a blank. A hidden annotation's
appearance counts too: the flag is a viewer's instruction, and clearing it is
one bit.

**Type 3 fonts** have no program to cut: a glyph is a procedure in
`/CharProcs`, and the procedures are the face's outlines — and can show text
besides, which is what a redaction leaves in one when it removes a glyph's
use rather than rewriting the procedure every use shares. So the same pass
empties every procedure nothing the document shows still runs, writing
`0 0 d0` over the stream in place and leaving the font dictionary as it was.
Which procedures run is learned where the interpreter asks for one, since a
Type 3 glyph is run rather than shown and no device hears of it — and,
for a code whose procedure this engine could not run (no `/FontMatrix`, a
procedure that does not decode), where it shows the glyph instead, because
a reader that defaults the matrix runs it. Kept: a
procedure under **any** name `/Differences` gives a shown code, not only the
first, which is the one this engine draws; and a stream another font keeps
or leaves whole. A shown code `/Differences` gives no name leaves the font
whole, as a code mapped only by guess leaves a program whole. A Type 3 font is left whole, and listed, for the reasons a
program is — no walked scope names it, the AcroForm `/DR` does, a Type 3
font's own `/Resources` does, or it has no object — and one left whole makes
`SubsetOutcome::removed` false, as a program does. Until October 2026 Type 3
fonts went through whole and unmentioned, and `removed` was true over them
(`subset.rs`'s `type3_fonts`).

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
and the two caps `redact::{MAX_PLACEMENTS, MAX_FORM_COPY_BYTES}` live in the facade
(`apply` returns `Option<RedactionReport>`, `None` for a page that does not
exist; the report counts `operations`, `glyphs` and `images`, and carries
`warnings: Vec<RedactionWarning>` — empty is the answer a caller wants,
since a non-empty list means some text was never tested against the
rectangles at all)
because glyph coverage needs both the content tokenizer and font metrics
(ruling 8, [rulings.md](../rulings.md)).

`subset::{SubsetReport, Subsetted, Type3Subsetted, Untouched, UntouchedReason, apply}` are on
the facade too. `apply(&mut editor) -> SubsetReport` takes no page: it is
whole-document by construction. `SubsetReport::subsetted` carries each
program's object, its new `/BaseFont`, the bytes before and after, and how many
glyphs were asked for; `untouched` carries every program written through whole
with its `UntouchedReason`; `type3` carries each Type 3 font measured — its
object, its name, its procedures' bytes before and after, and how many it
kept and emptied — and `type3_untouched` each one left whole, with the font
dictionary as `Untouched::program`; `bytes_before()`/`bytes_after()` total
all four.
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

**From the command line.** `tpdf`'s write half is these calls (ruling 11,
`tools/tpdf/src/writing.rs`): `merge` is `import_page` for every page of each
later file, `split` is `keep_pages` per piece, `rotate` is `rotate_page`,
`attach` is `attach_file`, `stamp` is `import_page_as_form` and then `stamp`
per page, `sanitise` is `sanitise` with each flag one `Sanitise` field and
`--all` `Sanitise::ALL` (no flag at all is refused, where it once meant all
four — a default the facade's `Sanitise::default()`, which is nothing, does
not have), and `encrypt` and `decrypt` change only `WriteOptions::encryption`.
Every one saves through `write::save` as a rewrite and takes `--font-policy
subset|keep`, defaulting to the facade's subset and printing the report
above, a program left whole named with its reason; the image policy is there
too, off unless asked ([writing](writing.md)). `split` sets `garbage_collect`
and `merge` `deduplicate_streams`, the options `keep_pages`' and
`import_page`'s own documentation pair them with. Every writer asks the
editor before it writes whether the save would undo an encryption, and maps
the answer into its own words: every writer but `encrypt` and `decrypt`
refuses an encrypted input, or a page or stamp copied in from one, which a
rewrite asking for no encryption writes decrypted
(`DocumentEditor::check_save`, `SaveRefusal::WouldDecrypt`), and those two
refuse to lift an owner's restrictions with only the user's authority
(`SaveRefusal::OwnerAuthorityNeeded`, `check_decrypt`; [encryption](encryption.md)).
Until October 2026 both were the CLI's own refusals. The save doors do not
ask, so the C ABI and a binding, which save without asking, still rewrite an
encrypted input decrypted; whether `save` itself refuses is owed in the
[roadmap](../ROADMAP.md)'s CLI row. What the CLI used to decide on its own
and the facade now decides for every surface: a turn that is not a quarter
(`rotate_page` refuses it), an empty owner password (the user's,
[encryption](encryption.md)), and these two.

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Redacting a run whose `Tf` named a font the resources in scope do not have | left whole, `UnknownFont` (`a_run_whose_font_is_not_in_scope_is_left_uncut_and_reported`) | no metrics at all, so no glyph can be placed. Permanent, and it was silent before: the run was kept, nothing was counted, and the report looked like a rectangle that covered nothing | — |
| Redacting a run whose text rendering matrix is not finite | left whole, `UnmeasurableFrame` (`a_non_finite_text_matrix_is_left_uncut_and_reported`) | a position that is not a number cannot be compared with a rectangle. Permanent. The whole showing operand is left, never half of it | — |
| Partial image redaction | the whole image is scrubbed (`RedactionReport::images`) | a hole needs a re-encode through a codec this build may not write | [filters](filters.md) |
| Cutting exactly, a copy per placement, a form that **draws itself** (directly or through another form), one **placed past `MAX_PLACEMENTS`**, or one whose distinct cuts would take a redaction past **`MAX_FORM_COPY_BYTES`** (32 MiB of held cuts, `a_walk_past_its_copy_budget_cuts_the_form_in_place`; until October 2026 there was no budget, and a form could be held and copied 64 times over at 128 MiB a copy) — and every form any of them draws | the cut is the union over every placement, in the one stream, and `RedactionWarning::RepeatedForm` names the form and its placement count when a cut was made or a placement went unmeasured (`a_self_referential_form_under_a_moving_transform_terminates`, `a_form_drawn_by_one_placed_past_the_cap_is_cut_the_old_way_too`) | a copy per placement of a form that draws itself would be a copy per round of a recursion, and a placement past the cap was never measured, so no copy could say what it should hold; what such a form draws goes the same way because its one stream names its children by their own objects. Over-removal is the direction this module errs in everywhere; the alternative here is the leak | 8.10 |
| Cutting a **tiling pattern's cell** at the tiles a rectangle covers alone | the cell is measured at every tile of its lattice whose `/BBox` meets a rectangle — the pattern's `/Matrix` anchored both where 8.7.2 puts it and where this engine's renderer does, the page's space (`a_cell_a_form_paints_with_is_measured_in_the_forms_space_too`) — and cut in its one stream, so at every tile wherever the pattern paints; `RedactionWarning::RepeatedForm` names the pattern and the tiles measured (`a_tiling_cell_whose_text_is_under_a_rectangle_is_cut_at_its_tiles`). A tile the fill does not reach is measured too | every tile runs the cell's one stream, and a copy per tile would be a pattern per tile; this module does not follow paths, so it cannot tell a tile the fill reaches from one it does not, and measuring both removes more, never less | 8.7.3 |
| Measuring a tiling cell with more than `MAX_PLACEMENTS` tiles under the rectangles, one whose `/Matrix`, `/BBox` or steps place no lattice, what a cell invokes beyond its text and inline images (an XObject, a graphics state, another pattern), and a cell a Type 3 glyph's procedure paints with | **named**, `RedactionWarning::PatternOrMask` with the resource name that selected it (`a_tiling_pattern_whose_cell_shows_text_is_named`); a cell that only paints paths is not, since nothing in it is anything a redaction removes. A cell's own text is still cut when what it invokes is not followed | the tile count bounds the work a lattice of tiny cells under a page-sized rectangle would ask (ruling 1); a cell's XObjects would be placements with no node of the walk's to hold them; a procedure is measured rather than cut | ruling 1, 8.7.3 |
| Cutting a **soft mask's group** through a copy | the group is measured at its `gs`, as a form placement (`a_soft_mask_group_is_measured_where_its_gs_places_it`), and cut in its own stream, the old way; `RepeatedForm` names it when that is wider than asked — a second placement, or another page that sets the same state (`a_mask_group_another_page_draws_is_cut_in_place_and_named`). A group a **Type 3 glyph's procedure** sets is measured at each use, and a use whose group draws under a rectangle is removed, but the group is **not cut**: what it showed there stays in the file, and `PatternOrMask` names the state (`a_glyph_whose_procedure_masks_text_under_a_rectangle_is_removed`) | a copy would need a copied graphics state under a fresh name and the `gs` pointed at it, for a state a stream nearly always sets once; over-removal is this module's direction, and it is named. A procedure's group is the procedure's: every use of the glyph draws it, and the procedure is not rewritten (the Type 3 row below) | 11.6.5 |
| Measuring more than `MAX_PLACEMENTS` distinct placements of one form | the count in `RepeatedForm` saturates at the cap, which is how a caller tells "too much went" from "something may have survived" (`a_form_placed_more_times_than_the_cap_saturates_its_count`) | a form that invokes itself under a matrix that moves each round makes a fresh placement every time; a count bounds it, where a tolerance on matrices would have to be loose enough to call two real placements one | ruling 1 |
| Following more than 4 096 `Do`s of one content stream | the ones past the bound are written back as they were and never resolved — an image they draw is tested against no rectangle, a form not entered — and `RedactionWarning::TooManyXObjects` counts them (`a_stream_of_more_xobjects_than_the_walk_follows_is_reported`); until October 2026 the bound was there and the warning was not | the walk holds a use per `Do`, and a content stream may be 128 MiB of six-byte `/a Do`s; a page of more than four thousand XObject placements — a map, a tiled scan — has to be told it was not measured whole | ruling 1 |
| A free text annotation whose `/DA` names no font the form's `/DR` holds, or one it holds as a composite, Type 3 or symbolic font, or whose `/Contents` has a character that font's encoding gives no byte, or whose `/DA` does not lex cleanly | no appearance is synthesised, and the annotation renders only if it carries its own (`a_free_text_without_a_font_to_write_it_in_is_refused`, `a_free_text_is_written_in_the_bytes_its_fonts_encoding_gives`, `a_default_appearance_is_lexed_as_a_content_stream`) | the text is written a byte a glyph, each the byte the font's encoding draws that character with, and a font that is not there, a font addressed by multi-byte codes, a symbolic font's own glyphs or a character the encoding has no byte for would make a box of wrong text or question marks: this module draws none rather than a wrong one. Until October 2026 a character was written as its Latin-1 byte whatever the encoding, so a StandardEncoding font drew `café` as `cafØ`. A composite `/DA` font is what `fill.rs`'s shaped path writes for fields, and a free text annotation does not take it yet. **Not permanent** for a composite font: owed in the ROADMAP's Editing row (d); a missing font, a symbolic one and a character the encoding has no byte for are not ruled either — a substitute font is the alternative — and are the owner's with the rest of (d) | 12.5.6.6, 12.7.3.3 |
| A free text annotation's rich text (`/RC`), default style (`/DS`), and the operators of `/DA` other than its `Tf` and its `g`, `rg` or `k` | `/Contents` is drawn as plain text in the `/DA` font, size and colour; the rest is not read | `/RC` is XHTML and `/DS` CSS (12.7.3.4), which this module does not lay out — **not permanent**, both owed in the ROADMAP's Editing row (d); `/DA`'s other operators are the producer's text, and copying them into the appearance could unbalance its `q`/`Q` or `BT`/`ET` | 12.5.6.6 |
| Appearance synthesis for the subtypes whose appearance no dictionary determines — `Stamp`, `FileAttachment`, `Sound`, `Movie`, `Screen`, `3D`, `RichMedia`, `Popup`, `Widget`, `PrinterMark`, `TrapNet`, `Watermark`, `Redact` and `Projection` (`appearance::UNDETERMINED_SUBTYPES`) | `add_annotation` inserts the dictionary and synthesises no `/AP`, so the annotation renders only if it carries its own (`the_subtypes_no_dictionary_determines_are_declined`; `every_subtype_of_12_5_6_is_drawn_or_declined` fails if a subtype is in neither list). The list changes nothing a caller sees: a subtype on it is declined exactly as an unknown one is | what each would show is not in its dictionary: a stamp's, a file attachment's and a sound's `/Name` names an icon — `Approved`, `PushPin`, `Speaker` — that 12.5.6 gives no outline; a movie, a screen, a 3D annotation and rich media show their medium; a pop-up is the viewer's window for its parent's text; a widget's appearance is its field's, which the form filler builds from the value (`fill.rs`); a printer's mark, a trap network and a watermark exist only as the `/AP` their producer wrote; a redaction's entries say what replaces the content once it is applied and not what the mark looks like before; and a projection adds no entry at all. Inventing a picture is worse than drawing none — but `Text` is drawn with an invented note though 12.5.6.4 leaves its icon to the reader just as 12.5.6.12, .15 and .16 leave the first three's, and `Caret` with an invented outline, so the reason is not applied evenly. **Not ruled permanent**: this module's position, the owner's to accept or refuse in the ROADMAP's Editing row (d): draw invented icons for `Stamp`, `FileAttachment` and `Sound` as `Text` has one, or rule that an invented icon is refused and decide `Text`'s | 12.5.6 |
| A line's caption (`/Cap`, `/CP`, `/CO`) | the line is drawn, its caption is not | a caption is text, and a line annotation names no font to draw it in. **Not ruled permanent**: this module's position, the owner's to accept or refuse in the ROADMAP's Editing row (d) | 12.5.6.7 |
| A caret's paragraph symbol (`/Sy /P`) | the caret is drawn and the symbol is not, exactly as for `/Sy /None` | 12.5.6.11 says a ¶ "shall be associated with the caret" and says nothing of where it goes or how large it is; a symbol put somewhere is an invention, and a producer that wants one writes its own `/AP`. **Not ruled permanent**: this module's position, the owner's to accept or refuse in the ROADMAP's Editing row (d) | 12.5.6.11 |
| An annotation without `/AP` whose dictionary has an entry that is present and not what its table says — coordinates or colours that are not finite numbers within Annex C's real limit or not the count Table 175, 177, 178, 180 or 182 asks for, an `/RD` Table 177 forbids, a dash 8.4.3.6 refuses, a `/BS /S` Table 166 does not name, an `/LE` name Table 176 does not list, a negative `/LLE` or `/LLO`, a `/CA`, `/ca` or `/LL` that is not a number, a `/Q` other than 0, 1 or 2, a negative `/DA` size | no appearance is synthesised (`a_malformed_entry_declines_the_appearance`) | what would be drawn from what is left is a shape the producer did not write: one bad element of `/L` re-paired moves every later coordinate, and a forbidden `/RD` read past puts the shape somewhere else. Declining repairs nothing, so no warning is emitted; the annotation is left as it came | 12.5.6 |
| A synthesised border's **effect** (`/BE /S /C`, cloudy, on a square, circle, polygon or free text) and the **beveled, inset and underline** border styles (`/BS /S /B`, `/I`, `/U`) | the border is drawn solid, at its width and in its colour, as though `/BE` were absent and `/S` were `/S` | 12.5.4 says a cloudy border "shall appear cloudy" at an intensity from 0 to 2 and gives no geometry for a cloud; Table 166's beveled and inset styles are "simulated" embossing in shades nothing names, and the underline style draws a widget's bottom edge, which is not what a shape is. A producer that wants one of them writes its own `/AP`, and one it wrote is kept. **Not ruled permanent**: this module's position, the owner's to accept or refuse in the ROADMAP's Editing row (d). The corner radii of a legacy `/Border` are not drawn either, as they never were | 12.5.4 |
| Rewriting a Type 3 glyph's procedure when it draws under a rectangle | the **use** is removed whole and the procedure is left byte for byte (`a_glyph_whose_procedure_shows_text_under_a_rectangle_is_removed_at_that_use`), so a procedure that shows the covered words still says them in `/CharProcs` while any use of it is left; once none is, `subset::apply` — the default save — empties it (`a_procedure_whose_last_use_was_redacted_is_emptied_by_the_default_save`). What the procedure draws is left with it: a form it invokes is not cut, nor a soft mask's group it sets, and emptying the procedure removes neither from the file; a group is named `PatternOrMask` when it is what removed a use, and a form is not named | the procedure is the font's: every use of the glyph on every page runs it, so cutting it would cut every use, and there is no copy to give the uncovered ones short of a new glyph in the font. This is a substitute for the ROADMAP Editing row's "glyph-procedure streams rewritten", chosen here and not yet ruled on: the row stays open until the owner accepts it or it is replaced | 9.6.5 |
| Measuring a glyph procedure that shows glyphs whose procedures show glyphs, past `MAX_PLACEMENTS` streams for one use | the use is removed as though covered, and `RedactionWarning::UnboundedProcedure` names the font and counts the uses (`a_glyph_procedure_that_shows_its_own_glyph_ends_and_errs_toward_removal`); until October 2026 nothing reported it | a procedure can show its own glyph, and a face that branches makes the measurement exponential; the budget is per use (`every_use_of_a_glyph_has_a_budget_of_its_own`), so only such a face reaches it | ruling 1 |
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
| Dropping every reference to a page `keep_pages` or `delete_page` removed | the page leaves the page tree and nothing else: an outline item, a named destination, a link, the structure tree or `/OpenAction` that names it still does, so a rewrite keeps the page and its content outside the tree, and `garbage_collect` keeps them too, because they are reached — page 6 of `outline-3level.pdf` kept alone still carries the four pages its outline names (`tools/tpdf/src/writing.rs`'s `a_page_the_outline_names_stays_in_a_piece_that_dropped_it`, found through `tpdf split`) | which references to null and which entries to drop — an outline item, a link annotation, a structure element — is a sweep over every object the editor has with a decision per kind, and none is made yet. **Not permanent**: owed in the ROADMAP's CLI row. Until it lands, a document cut down with `keep_pages` is not a redaction | 7.7.3, 12.3 |
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
- `crates/tinker-pdf/tests/appearance_synthesis.rs` — one fixture per
  subtype, its annotation written as PDF text with no `/AP`: added through
  the editor, saved, reopened through the facade and rendered, and the page
  read at points 12.5.6 puts inside the shape and at points it puts outside.
  A line along `/L` at its width and no further; its arrowhead and square
  endings filled with `/IC` and bordered with `/C`; a dash from its first
  point; leader lines that lift the line proper off its points by `/LLO`
  and `/LL` and run `/LLE` past it, its square endings filled where the
  line proper ends and nothing at the points in `/L`; and half opacity reaching the page as
  half the colour. A square bordered inside its `/Rect` and filled, moved
  inside it by `/RD`, and dashed from its lower-left corner; a circle
  inscribed in its `/Rect`, its corners left white, and its fill at half
  opacity. A polygon closed back to its first vertex and filled; a polyline
  left open and unfilled, a square ending at its first vertex and a closed
  arrow at its last, pointing on along the last segment. A highlight filling
  its quad and nothing past it; an underline under the quad's text and a
  strike-out through it; a squiggly underline's crests drawn at the band's
  top and not its bottom, its troughs the other way round, white between
  its strokes; and the same zigzag along a quad turned a quarter. A caret
  flaring from the bottom of the rectangle its `/RD` leaves, white beside
  its spike and between `/RD` and `/Rect`. An ink annotation's three paths
  — a peak, a bar and a dot — each stroked with round caps and joins and
  none joined to the next, and a path named twice by reference drawn once.
  A free text box filled with `/C` and bordered in its text's colour; its
  contents in the `/DR` font the `/DA` names — `render_support`'s synthetic
  face, embedded — wrapped onto three lines where the layout puts them, and
  set against the right side by `/Q 2`; and a callout from the box to the
  point it calls out, drawn only under `/IT /FreeTextCallout`.
  `appearance.rs`'s own tests pin the seven first subtypes byte for byte and
  each ending's path, decline every subtype whose appearance no dictionary
  determines and every dictionary with a malformed entry, fail if a subtype
  of 12.5.6 is in neither list, and put 768 dictionaries of hostile numbers
  — none, too few, enormous, infinite, not a number — and `/DA` strings of
  arbitrary bytes through every subtype without a panic, every stream they
  write read back by the crate's own lexer with no leniency
  (`synthesis_never_panics_whatever_the_numbers`). A whole number past a
  32-bit integer is written as a real, which a reader takes whole. The fuzz
  target `annotation_appearance` drives the same synthesis from arbitrary
  bytes parsed as one dictionary, against a form whose `/DR` holds four
  fonts, and asserts the same two properties: output bounded by the input,
  and lexing clean.
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
  its axis, the box centred on the pen and placed by `v_y`, and a column
  turned a quarter turn (`vertical_runs`); a Type 3 glyph space in hundredths, skewed, rotated
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
  it; a form page two meets deep and then shallow followed into; and a form
  cut the old way that page two draws named. `appearance_streams`:
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
- Beside them, in Helvetica: `patterns_and_masks` (a tiling pattern or a
  soft mask that shows text named, one of paths not, in the scope of the
  stream that painted, and what a glyph procedure paints with), `xobject_cap`
  (4 096 `Do`s followed, the 4 097th named), the depth pair in `tests`
  (sixteen levels of forms redacted, seventeen not drawn), and `hostile`:
  one two-page file over every kind of content above — appearances and their
  states, procedures that show text and draw forms, a form drawn twice and
  on the second page, a pattern, a mask, an inline image — put through 300
  deterministic injuries and then redacted twice, subsetted and saved, with
  nothing asserted but that nothing panics. `tests/hostile_input.rs` does the
  same for the reading surface and does not edit.
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
  has every removed letter's outline in the file. Six more, `type3_fonts`,
  over Type 3 faces written here whose procedures are boxes of different
  heights: a procedure nothing shows emptied and one shown kept, the page
  rendering exactly as it did; a stream two fonts share kept for the one that
  shows it; every name `/Differences` gives a shown code kept; a glyph a form
  shows keeping its procedure; a font no walked scope names, and one written
  into the resources, left whole and listed; and `removed()` false over the
  first and true once it is walked. `redact.rs`'s `glyph_procedures` carries
  the disclosure half: a procedure whose last use was redacted is the empty
  one after the default save.
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

  The Type 3 half was counted the same way on 2 October 2026, over
  `cargo test --no-fail-fast -p tinker-pdf --lib` (352 tests), none zero:

  | defect reintroduced | tests that caught it |
  | --- | --- |
  | the Type 3 glyphs the interpreter runs never written down, so every procedure is emptied | 4 |
  | only the first name `/Differences` gives a shown code kept | 1 |
  | a procedure stream kept only for the font that shows it, not for one that shares it | 4 |
  | a Type 3 font no walked scope names emptied anyway | 2 |
  | a directly written Type 3 font's procedures not protected | 1 |
  | a directly written Type 3 font not recognised as one | 1 |
  | a form's own scope not written down in, so a glyph it shows is lost | 1 |
  | `removed()` not asking about a Type 3 font left whole | 1 |

- Every edited document is written through the [writer](writing.md), whose
  output is held to the strict validator (`strict_validator.rs`); the
  `render_page` and
  `cos_document` fuzz targets cover the reader side of what the editor
  produces.
