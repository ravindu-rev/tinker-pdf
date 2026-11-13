# Rendering for a usage

*For [ROADMAP](../ROADMAP.md) row RC-13.* A page is drawn for a use: on a
screen, on paper, or saved as a picture. Two things a page shows depend on
which, and neither is its content's to decide alone: which optional content
groups are on, which a document can make depend on the use through its usage
application dictionaries (ISO 32000-2 8.11.4.4), and which annotations draw,
which their flags decide per use (12.5.3). This document fixes how a render
names its use, the optional-content states each use gives and how one render
binds them once, the annotations each use draws, the magnification `/NoZoom`
and `/NoRotate` are placed against, the switches that draw a page's content,
its annotations and its widgets apart, and what a display list records so that
a render naming a use replays it or draws the direct way.

Clause and table numbers are ISO 32000-2's.

## Scope

- **The usage a render names** (RC-13.2): view, print or export, or none,
  which is the render this engine makes today.
- **The optional-content states a usage gives** (RC-13.2): the default
  configuration's `/AS` entries for the usage's event, applied over its
  initial states, and bound once a render for every scope it opens.
- **The annotations each usage draws** (RC-13.3): by `/F`, `/Invisible`
  included, and by an annotation's `/OC`.
- **A magnification**, and the corner `/NoZoom` and `/NoRotate` keep fixed
  (RC-13.4).
- **The parts of a page a render draws** (RC-13.5): its content, its widgets
  and its other annotations, each on its own switch.
- **What a display list records**, for each of the above.

## Non-goals

- **`to_svg`** names no usage. It keeps what it draws: the default
  configuration's initial states and the annotations a view draws by `/F`,
  gaining only the corrections RC-13.3 makes to every render that draws
  annotations — `/Invisible` and an annotation's `/OC` (*The annotations each
  usage draws*).
- **The field highlight** an interactive viewer draws over a form field. None
  exists to suppress: AN-15 owes the highlight, drawn in view usage only, and
  RC-13.5's print test is the guard for it once it lands.
- **Alternate configurations, the zoom, language and user categories,
  per-render overrides, `/RBGroups`, `/Locked` and `/Order`** are RC-14's,
  built on the usage and the binding here
  ([design/optional-content-config.md](optional-content-config.md)).
- **Text and image extraction** read the default configuration's initial
  states, as they do today: they read the content a document has, not a use's
  view of it, and name no usage.
- **Interaction**: `/ReadOnly`, `/Locked`, `/ToggleNoView` and
  `/LockedContents` (Table 167) govern what a user may do with an annotation,
  and a render does nothing with one.
- **Printing itself**: no spooling, device, page fitting or print scaling. A
  print render is a picture of the page as it prints, at the scale it is asked
  for.

## Design

### The usage a render names

`RenderOptions::usage: Option<Usage>`, with `Usage` one of `View`, `Print` and
`Export`, the three events of a usage application dictionary: its `/Event`
"Shall be one of View, Print, or Export" (Table 101). `None` is the default.

A render naming none draws the default configuration's initial states, which
8.11.4.5 makes everyone's starting point — "When a document is opened, its
optional content groups shall be assigned a state based on the D (default)
configuration dictionary … This state shall be the initial state used by all
PDF processors" — and which is exactly what every render draws today. So no
page changes unless a caller names a usage. A render naming one starts from
the same initial states and applies the default configuration's `/AS` entries
for that event over them.

### The states a usage gives

8.11.4.4 is the rule, and this is it as read:

- **Which entries.** "The Event entry specifies whether the usage settings
  shall be applied during viewing, printing, or exporting the document. The
  OCGs entry specifies the set of optional content groups to which usage
  settings shall be applied." The render applies the default configuration's
  (`/D`'s) `/AS` entries whose `/Event` is its usage, and no other: 8.11.4.5
  applies each event's entries for its own operation, "applied over the
  current states", and a render has no session whose current states are
  anything but the initial ones. A print render is a printing application's
  render of the document as opened, not a viewer's print of a page it has
  been adjusting for viewing, so view's entries are not applied before
  print's.
- **What each category recommends.** "For each of the groups in OCGs, the
  entries in its usage dictionary … specified by Category shall be examined
  to yield a recommended state for the group." The three this row reads are
  the states of the three events: "View: The state shall be the value of the
  ViewState entry", "Print: The state shall be the value of the PrintState
  entry. If PrintState is not present, the state of the optional content
  group shall be left unchanged", and "Export: The state shall be the value of
  the ExportState entry". An entry of any event may name any category — "the
  corresponding usage application dictionaries are permitted to specify that
  other categories can be applied" (8.11.4.5, note 3) — so a print entry
  listing `/View` reads `/ViewState`.
- **A category with no entry recommends nothing.** The clause's example
  settles what a group without the category's entry gets: "Object 4 has none;
  therefore, it is not affected by zoom level changes." So a group whose
  `/Usage` lacks the category's dictionary, or whose dictionary lacks the
  state, takes no recommendation from that category.
- **How recommendations combine.** "If all the entries yield a recommended
  state of ON, the group's state shall be set to ON; otherwise, its state
  shall be set to OFF", and across entries of the event, "If a given optional
  content group appears in more than one OCGs array, its state shall be ON
  only if all categories in all the usage application dictionaries it
  appears in have a state of ON." So a group with at least one recommendation
  is OFF if any is OFF and ON otherwise, and a group with none keeps its
  initial state.

**This row reads three categories: `/View`, `/Print` and `/Export`.**
`/Zoom`, `/Language` and `/User`, whose inputs are a magnification, the
caller's language and the caller's identity, are read as
[optional-content-config.md](optional-content-config.md) states: `/Zoom` at
the render's magnification, 1 unless the caller states one, and `/Language`
and `/User` recommending nothing where the render states no language, or no
user of the kind compared. `/CreatorInfo` and
`/PageElement` carry no state and recommend nothing. An alternate
configuration's own `/AS` is applied as that document states; here it is
`/D`'s.

**What is malformed leaves its group as `/D` set it.** An `/AS` that is not an
array, an entry that is not a dictionary, an `/Event` that is none of the
three names, a missing or non-array `/Category`, a `/Usage` or a category
dictionary that is not a dictionary, and a state that is neither `/ON` nor
`/OFF` each yield no recommendation; an entry's `/OCGs` defaults to the empty
array, which "no groups shall be affected" (Table 101). A group an entry
names that `/OCProperties /OCGs` does not list takes its recommendation as a
listed one does — `/D`'s own `/ON` and `/OFF` already reach such a group — and
everything else the module reads keeps its visible-first fallbacks. Every
array is read a bounded way in, as `/ON` and `/OFF` are (`MAX_OPERANDS`), and
every value where it lies.

**One binding a render.** The render's operation (`resources::Operation`)
carries its usage, and binds the states at the first scope that asks:
`OptionalContent::bind_for(doc, usage)`. Every scope the render opens reads
them through the operation — the page's, each form's, each tiling cell's and
each appearance's two — where each scope binds `/D` again today
(`PageResources::over`). A form's scope the caches keep is served bound to
the asker's operation (`PageResources::bind`), and with it to the asker's
states, so a scope built under one usage never answers for another. An
operation that names no usage — a text or image extraction, an SVG writer, a
save's pass, a validator — binds the initial states once, the first time it
is asked, as every render naming none does.

### The annotations each usage draws

12.5.3's flags, as read (Table 167):

- **`Hidden`**: "If set, do not render the annotation or allow it to interact
  with the user, regardless of its annotation type or whether an annotation
  handler is available." Never drawn.
- **`Print`**: "If set, print the annotation when the page is printed unless
  the Hidden flag is also set. If clear, never print the annotation,
  regardless of whether it is rendered on the screen." A print render draws an
  annotation only where it is set. "If the annotation does not contain any
  appearance streams this flag shall be ignored" — and an annotation with no
  appearance draws nothing in any usage here.
- **`NoView`**: "do not render the annotation on the screen … The annotation
  may be printed (depending on the setting of the Print flag)". A view hides
  it; a print render draws it where `Print` is set.
- **`Invisible`**: "Applies only to annotations which do not belong to one of
  the standard annotation types and for which no annotation handler is
  available. If set, do not render the unknown annotation and do not print it
  even if the Print flag is set." This engine draws every annotation by its
  appearance and has a handler for none, so the flag applies to an annotation
  whose `/Subtype` is not one of Table 171's — `Text`, `Link`, `FreeText`,
  `Line`, `Square`, `Circle`, `Polygon`, `PolyLine`, `Highlight`, `Underline`,
  `Squiggly`, `StrikeOut`, `Caret`, `Stamp`, `Ink`, `Popup`,
  `FileAttachment`, `Sound`, `Movie`, `Screen`, `Widget`, `PrinterMark`,
  `TrapNet`, `Watermark`, `3D`, `Redact`, `Projection` and `RichMedia` — or
  that has none, and hides it in every usage.
- **`/OC`**: "Before the annotation is drawn, its visibility shall be
  determined based on this entry as well as the annotation flags … If it is
  determined to be invisible, the annotation shall not be drawn" (Table 166),
  and "it shall be visible for screen or print only if the flags have the
  appropriate settings and the group or membership dictionary indicates it
  shall be visible" (8.11.3.3). Read under the render's states, by the module
  that reads a content stream's `/OC` (`OptionalContent::layer_of`), its
  fallbacks visible.

| The annotation | No usage | View | Print | Export |
| --- | --- | --- | --- | --- |
| `Hidden` set | hidden | hidden | hidden | hidden |
| `NoView` set | hidden | hidden | as `Print` says | hidden |
| `Print` clear | drawn | drawn | hidden | drawn |
| `Invisible` set, a subtype Table 171 does not name | hidden | hidden | hidden | hidden |
| `/OC` off under the render's states | hidden | hidden | hidden | hidden |

**Export draws what a view draws.** The flags speak of the screen and of
printing, and an export — "saved by a PDF processor to a format that does not
support optional content (for example, a raster image format)" (Table 100) —
is neither. It is a saved picture of the page, which a reader expects to show
what the screen shows: the comments drawn without the `Print` flag included,
and nothing printing alone adds. A caller exporting what the page prints names
`Print`. Its optional-content states are still export's.

**No usage draws what a view draws**, which is today's `/F` rule, and with it
the two corrections every usage takes: `Invisible` on an unknown subtype and
an annotation's `/OC`, which no render reads today. The risks name what that
moves.

One `annots::prepare` decides for a direct render, a display list and
`Page::render_annotation`, from the operation's usage and states, with a
`NotDrawn` reason for each refusal: `Hidden` for the `Hidden` flag, and new
`NoView`, `NotPrinted`, `Invisible` and `OptionalContentOff` for the others.

### A magnification, and the corner `/NoZoom` and `/NoRotate` keep

`RenderOptions::scale` is pixels per point: a resolution. A viewer's zoom is a
different number — a render at 300 dpi of a page at 100 % and one at 72 dpi
of a page at 417 % have one scale — and 12.5.3's two placement flags, and
RC-14.4's `/Zoom` category, read the zoom. So `RenderOptions::magnification`
stands beside `scale`, 1 unless the caller states one, and changes nothing
else: an annotation with neither flag draws as it does today at every
magnification.

12.5.3, as read: `/NoZoom` — "If set, do not scale the annotation's
appearance to match the magnification of the page. The location of the
annotation on the page (defined by the upper-left corner of its annotation
rectangle) shall remain fixed, regardless of the page magnification";
`/NoRotate` — "If set, do not rotate the annotation's appearance to match the
rotation of the page. The upper-left corner of the annotation rectangle shall
remain in a fixed location on the page, regardless of the page rotation." And
the corner: "the annotation's position is defined by the coordinates of the
upper-left corner of its annotation rectangle, as defined by the Rect entry in
the annotation dictionary and interpreted in the default user space of the
page … it shall not actually change the annotation's Rect entry"; "The
upper-left corner of the annotation remains at the same point in default user
space; the annotation pivots around that point."

So the fixed point `P` is `/Rect`'s upper-left corner in default user space,
`(min x, max y)`, and the placement composes onto 12.5.5's fit, in default user
space, before the page's view transform:

- **`/NoZoom`** scales the fitted appearance by `1 / magnification` about `P`,
  so it keeps the size it has at magnification 1 however the page is zoomed.
- **`/NoRotate`** turns it about `P` counter-clockwise by the page's `/Rotate`,
  which the view's clockwise turn of the page undoes, so it stands as on an
  unturned page.

Both are computed in the page's frame from the magnification and `/Rotate`
alone, never from a region, so a tile draws the page's pixels (ruling 5).
`Page::render_annotation`'s viewport is the placed box.

### The parts a render draws

`RenderOptions::annotations` keeps its meaning: off, no annotation of any kind
is drawn. Beside it, three switches, each on by default so that no render
changes:

- **`content`**, the page's content stream;
- **`widgets`**, widget annotations;
- **`other_annotations`**, every annotation that is not a widget — 12.5.6.2's
  markup annotations, links, stamps and watermarks alike.

Three, where `annotations` and a widget switch alone would not do: a page's
widgets with neither its content nor its other annotations, and its content
with its widgets alone, are two of the seven non-empty combinations of the
three parts RC-13.5 holds, and neither can be said without a switch of their
own for the annotations that are not widgets. A widget draws the normal
appearance its `/AS` state names in every usage, as it does today; there is no
highlight to leave out in print usage (*Non-goals*).

### What a display list records

- **Its usage.** `Page::display_list` records under no usage, as today, and
  `Page::display_list_for(usage)` under the one it names: the recorded calls
  carry the visibility the interpreter decided under that usage's states. A
  render naming the list's usage replays it; one naming another draws the
  direct way (`Page::render`), so its pixels are the direct render's.
- **The annotations its usage draws** (RC-13.3): what the usage hides is not
  recorded.
- **An annotation with `/NoZoom` or `/NoRotate`** (RC-13.4) is kept prepared
  rather than recorded, and interpreted at each replay under the placement the
  render's magnification and `/Rotate` give.
- **The parts** (RC-13.5): the content's calls and each annotation's, beside
  whether it is a widget, are kept apart, so a replay draws only the parts the
  render asks for. The one dependence between them is the group budget
  (`Admission`), which the annotations were recorded after the content had
  spent: a render that leaves the content out draws the direct way when the
  recording's annotations met a budget the content had spent.

## Milestones

1. **RC-13.1: this document.**
2. **RC-13.2: a usage's states, bound once a render.** Exit:
   `optional_content.rs` `a_print_usage_render_of_an_ocmd_with_print_usage_is_its_d_twin`
   (new) — an OCMD over groups whose `/Usage /Print /PrintState` is `/OFF`,
   under an `/AS` print entry, renders in print usage byte-equal to its twin
   whose `/D` is written as printing's states, and in view usage and with no
   usage as before; `each_usage_and_state_category_renders_as_its_d_twin`
   (new), a fixture per usage and state category, malformed entries among
   them, each with its `/D` twin; `display_list.rs`
   `a_list_draws_another_usage_the_direct_way` (new), a print render of a list
   recorded under no usage equal to the direct print render, and one of a list
   recorded for print equal to it too; `optional.rs`
   `one_render_binds_one_configuration` (new), the binds counted under
   `cfg(test)` — one for a render through forms, a tiling cell and an
   appearance; and every fingerprint and golden unchanged, no usage named.
3. **RC-13.3: the annotations each usage draws.** Exit:
   `annotation_appearances.rs` `each_flag_draws_as_its_usage_says` (new) — a
   table over `/Hidden`, `/Print`, `/NoView`, `/Print` with `/NoView`,
   `/Invisible` on a known and an unknown subtype and an `/OC` on and off,
   each in view and print usage, equal to its twin, the page with that
   annotation dropped or its flags cleared, byte for byte, directly and from a
   display list; `render_parts.rs` `a_part_the_usage_hides_is_refused_by_name`
   (new), each `NotDrawn` reason; `svg_output.rs` holding `to_svg`'s two
   corrections.
4. **RC-13.4: `/NoZoom` and `/NoRotate` placed.** Exit:
   `annotation_appearances.rs` `no_zoom_and_no_rotate_keep_their_fixed_corner`
   (new) — at magnifications 0.5, 1 and 2 and `/Rotate` 0, 90, 180 and 270,
   each flagged annotation equal to its twin, an unflagged one whose `/Rect`
   and `/Matrix` are written as the placement the flag gives there, directly
   and from a display list; `render_regions.rs` tiling a `/NoZoom` page at four
   tile sizes byte-equal to the page; `render_options.rs` pinning one such
   page's fingerprint through K-07's roster.
5. **RC-13.5: the parts drawn apart.** Exit: `render_parts.rs`
   `content_annotations_and_widgets_render_separately` (new) — a page with
   content, a markup annotation and two widgets, under each of the seven
   non-empty combinations of content, other annotations and widgets, equal to
   its twin with the parts left out deleted from its content stream or
   `/Annots`, directly and from a display list; each widget's print render
   equal to its view render; `render_options.rs`'s `the_default_is_unchanged`
   unmodified.

## Risks

| Risk | Mitigation |
| --- | --- |
| **Pixels move once `/NoRotate`, `/Invisible` and an annotation's `/OC` are honoured**, in every render that draws annotations, the default included: an annotation flagged `/NoRotate` on a page with a `/Rotate` of 90, 180 or 270 — a note's icon, a stamp — draws upright where it draws turned today; an annotation of a subtype Table 171 does not name, or of none, with `/Invisible` set draws nothing where it draws today; and an annotation whose `/OC` the render's states turn off draws nothing where it draws today, the `/OC` read by no render now. | Each is a correction the clause states, made by RC-13.3 and RC-13.4, each step naming in its commit what moves; no fingerprint or golden fixture carries the flags or an annotation's `/OC`; `/NoZoom` moves nothing at magnification 1, the default |
| A scope built under one usage answers for another — a kept form scope, a retained page | The states ride the operation, and a kept scope is served bound to the asker's; `one_render_binds_one_configuration`; a display list replays only its own usage |
| A malformed `/AS` or `/Usage` hides content | Each such entry recommends nothing and leaves its group as `/D` set it; the module's other fallbacks stay visible-first; `each_usage_and_state_category_renders_as_its_d_twin`'s malformed fixtures |
| Print applies view's entries first, as an interactive viewer printing would | The reading is stated: a render is a printing application's, over the initial states; a caller who wants a view's adjustments in print states them as overrides once RC-14.3 lands |
| Export's annotations, which the flags do not speak to | Decided and stated: what a view draws; the twin tests hold it |
| The magnification read where the scale is meant | It defaults to 1, changes nothing without the two flags, and is computed in the page frame, so tiles stay the page's (`render_regions.rs`) |
| A display list's annotation replay diverges from a direct render that leaves the content out | The group-budget dependence is the one shared state; such a render draws the direct way; RC-13.5's display-list twins |
