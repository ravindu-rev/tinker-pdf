# Optional-content configuration at render time

*For [ROADMAP](../ROADMAP.md) row RC-14.* A document's optional content groups
start in the states its default configuration gives them, and a render today
can show nothing else. ISO 32000-2's 8.11.4.3 offers more: alternate
configurations a reader may choose, radio-button collections of groups, groups
locked against a user's changes, an `/Order` tree for a layer panel; and
8.11.4.4's auto-state adjusts groups by zoom, language and user as well as by
the view, print and export events. This document fixes the order a render's
states are built in, how a render names a configuration and changes groups of
its own, what radio-button collections and locks do to those changes, the
inputs the zoom, language and user categories read, the `/Order` tree a
caller is handed, and what a display list does for a render naming another
configuration.

It builds on [design/render-usage.md](render-usage.md): the usage a render
names, the `/View`, `/Print` and `/Export` state categories, and the one
binding of the states a render's operation makes for every scope it opens are
RC-13's, and everything here is computed into that binding. Clause and table
numbers are ISO 32000-2's.

## Scope

- **The order a render's states are built in** (RC-14.2): the default
  configuration's initial states, a chosen configuration over them, its
  auto-state, then the render's overrides.
- **Choosing a configuration** (RC-14.2): `/D` or an entry of `/Configs`, its
  `/BaseState` with `/Unchanged` read against `/D`, its `/Intent`, and the
  configurations' list.
- **Per-render overrides** (RC-14.3), with `/RBGroups` and `/Locked`.
- **The zoom, language and user categories** (RC-14.4) and their inputs.
- **The `/Order` tree** (RC-14.5).
- **What a display list does** for a render naming another configuration.

## Non-goals

- **`to_svg` and text and image extraction** keep `/D`'s initial states. SVG
  draws no usage's states (RC-13), and an extraction reads the content a
  document has; neither chooses a configuration or takes overrides.
- **Layer authoring** — writing `/OCProperties`, `/Configs`, `/RBGroups`,
  `/Locked` or `/Order` — is ED-40's. Everything here is read.
- **`/ListMode`** (Table 99): which of `/Order`'s groups a user interface
  displays — `/AllPages` or `/VisiblePages`, "only those groups in the Order
  array that are referenced by one or more visible pages". A filter over a
  layer panel, which changes no state and draws nothing; the tree is handed
  back whole, and a caller showing only some pages filters it by them.
- **The interactive session**: 8.11.4.5's manual changes that "remain
  overridden", the re-application of view entries "whenever there is a change
  to a factor", and set-OCG-state actions (12.6.4.13). A render is computed
  from its options alone, so its states are the same whoever asks and
  whatever was asked before.
- **Script access to layers** is FJ's.

## Design

### The order a render's states are built in

1. **`/D`'s initial states**, as every render builds them today: `/BaseState`
   over every group `/OCGs` lists, then `/D`'s `/ON`, then its `/OFF` (8.11.4.5
   a and b). These are needed whatever configuration a render chooses, for
   `/Unchanged`.
2. **The chosen configuration** over them, if it is not `/D`: "Used to
   initialise the states of all the optional content groups in a document
   when this configuration is applied … ON: The states of all groups shall be
   turned ON. OFF: The states of all groups shall be turned OFF. Unchanged:
   The states of all groups shall be left unchanged. After this
   initialization, the contents of the ON and OFF arrays shall be processed,
   overriding the state of the groups included in the arrays" (Table 99). A
   render applies a configuration to a document as opened, so `/Unchanged`
   leaves each group in the state step 1 gave it; a missing `/BaseState` is
   `/ON`, its default; `/OFF` is applied after `/ON`, as `/D`'s is.
3. **The configuration's auto-state** for the render's usage (RC-13): the
   chosen configuration's own `/AS` entries whose `/Event` is the render's
   usage. A configuration without `/AS` has none — Table 99 gives `/Order`
   and `/RBGroups` a default "from the default configuration dictionary" and
   `/AS` none, and "If no AS entry is present, states shall not be
   automatically adjusted based on usage information" (8.11.4.4). Its
   entries read the zoom, language and user categories too (*The zoom,
   language and user categories*, below).
4. **The render's overrides**, last (*Overrides*, below).

The configuration's **`/Intent`** then decides which groups count at all
(8.11.2.3): "If one or more of a group's intents is contained in the current
configuration's set of intents, the group shall be used in determining
visibility. If there is no match, the group shall have no effect on
visibility. If the configuration's Intent is an empty array, no groups shall
be used in determining visibility; therefore, all content shall be considered
visible." A group's `/Intent` and a configuration's both default to `/View`,
`/D`'s "shall be View", and `/All` in a configuration's set matches every
intent. A group with no effect reads as one the configuration never mentioned
does: visible, in a group's own `/OC` and as a member of a membership
dictionary alike. A name or an array of names is read; anything else is the
default.

### Choosing a configuration

`RenderOptions::layers: LayerOptions` holds what a render says about its
layers, beside RC-13's `usage` and `magnification`, its default changing
nothing: `configuration: Option<usize>` — `None` for `/D`, `Some(i)` for the
`i`th entry of `/Configs` — the overrides, the language and the user. An index
past `/Configs`, or an entry that is not a dictionary, renders as `/D` and is
reported as `RenderWarning::UnreadableLayerConfiguration { index }`.
`Document::layer_configurations` lists each entry of `/Configs` in order, its
`/Name` and `/Creator` decoded as text strings (Table 99), and a slot for an
entry that is not a dictionary, so an index a caller reads off the list is the
index a render takes.

### Overrides

`LayerOptions::overrides` is a list of `(ObjRef, bool)`: a group, by the
reference `Document::layers` hands out, turned on or off, applied in order
after step 3, so a later override wins over an earlier one.

**`/RBGroups`** (Table 99), as read: "each of which represents a collection of
optional content groups whose states shall be intended to follow a radio
button paradigm. That is, the state of at most one optional content group in
each array shall be ON at a time. If one group is turned ON, all others shall
be turned OFF. However, turning a group from ON to OFF does not force any
other group to be turned ON." An override turning a group on turns off every
other member of every collection the group is in; one turning a group off
changes nothing else. The clause describes a change of state — a group *turned*
on — so the collections constrain the render's overrides and not the
configuration's own `/ON` and `/OFF` arrays or its auto-state, which are the
document's statement of its states and are applied as written: "the contents
of the ON and OFF arrays shall be processed, overriding the state of the
groups included in the arrays". A configuration that turns two members of one
collection on shows both, the module's visible-first reading of a document
that says so. The collections are the chosen configuration's own, and "in
other configuration dictionaries, the default is the RBGroups value from the
default configuration dictionary".

**`/Locked`** (Table 99): "An array of optional content groups that shall be
locked when this configuration is applied. The state of a locked group cannot
be changed through the user interface of an interactive PDF processor … An
interactive PDF processor may allow the states of optional content groups to be
changed by means other than the user interface, such as ECMAScript or items in
the AS entry of a configuration dictionary." An override is a caller's change,
the user interface's part here, so an override of a locked group is refused —
the group keeps its state — and reported once a group by its `/Name`, as
`RenderWarning::LockedLayer { layer }`, the resource-name fallback of
`HiddenOptionalContent` replaced by the reference's text where there is no
`/Name`. A radio-button consequence that would change a locked group is not
made either, and reported the same way. The auto-state changes a locked group
as it changes any. The locks are the chosen configuration's own; Table 99
gives `/Locked` a default of "an empty array", not `/D`'s.

**An override of a group `/OCGs` does not list changes nothing**: 8.11.4.2's
`/OCGs` lists "all the optional content groups in the document", and a
reference outside it names no group of this document's.

### The zoom, language and user categories

8.11.4.4, as read, for the three categories RC-13 leaves recommending nothing:

- **`/Zoom`**: "If the current magnification level of the document is greater
  than or equal to min and less than max, the ON state shall be used;
  otherwise, OFF shall be used", `min` defaulting to 0 and `max` to infinity
  (Table 100). The magnification is RC-13's `RenderOptions::magnification`,
  1 unless the caller states one; one that is not finite or not positive is
  read as 1, as a scale is.
- **`/Language`**: "If an exact match to the language and locale is found among
  the Lang entries of the optional content groups in the usage application
  dictionary's OCGs list, all groups that have exact matches shall receive an
  ON recommendation. If no exact match is found, but a partial match is found
  (that is, the language matches but not the locale), all partially matching
  groups that have Preferred entries with a value of ON shall receive an ON
  recommendation. All other groups shall receive an OFF recommendation." The
  input is `LayerOptions::language: Option<String>`, one language tag, the
  application's; nothing reads the host's locale. `/Lang` and the input are
  14.9.2's language identifiers, compared without regard to ASCII case, as
  their tags are: an exact match is the whole tag, a partial match the
  primary language subtag — what precedes the first hyphen — with the rest
  differing. The match is decided within each usage application dictionary
  over the groups its `/OCGs` lists, as the clause says; "all other groups"
  are the other groups there that have a `/Language` entry, since a group
  without one has nothing for the category to examine (RC-13's reading of
  the example). With no language stated, the category recommends nothing.
- **`/User`**: "The Name entry shall specify a name or names to match with the
  user's identification. The Type entry determines how the Name entry shall be
  interpreted (name, title, or organisation). If there is an exact match, the
  ON state shall be used; otherwise OFF shall be used." The input is
  `LayerOptions::user`, the caller's identification as an individual's name, a
  title and an organisation, each optional; a group's `/Type` (`/Ind`, `/Ttl`
  or `/Org`) chooses which is compared, its `/Name` — a text string or an array
  of them — matching when any of them equals it exactly once decoded. With no
  identification of the group's type stated, or a `/Type` none of the three,
  the category recommends nothing.

Each applies only through an entry of the render's event, combined with the
other categories as RC-13 combines them. Nothing here reads the host's locale,
user or zoom; only what `RenderOptions` names.

### The `/Order` tree

`Document::layer_tree(configuration)` returns a configuration's `/Order`
(Table 99) as a tree. Its default for `/D` is the empty array, and for another
configuration `/D`'s `/Order` — "In the default configuration dictionary, the
default value shall be an empty array; in other configuration dictionaries,
the default shall be the Order value from the default configuration
dictionary." Each node is:

- **a group**: its reference, its `/Name` and its state under that
  configuration — steps 1 and 2 above, with no usage and no override, as
  `Document::layers` reports `/D`'s;
- **a label**: "Each nested array may optionally have as its first element a
  text string to be used as a non-selectable label", decoded as a text string,
  with the rest of its array as its children.

As read, with the clause's two examples: a nested array **with** a label is a
collection under that label, "and not to communicate actual nesting of content
inside multiple layers of groups"; a nested array **without** one that follows
a group holds that group's sublayers, its children — "To reflect actual
nesting of groups in the content, such as for layers with sublayers, nested
arrays of groups without a text label shall be used" — and one that follows
no group is a node with no label of its own, its entries its children; an
entry passed over between a group and the array does not part them. A text
string anywhere but first in a nested array is a label with no children, kept
so that nothing the file shows is lost. A group listed twice appears twice, as
written; a group `/OCGs` does not list is a group node all the same, its state
on as every unlisted group's is. An index naming no configuration reads
`/D`'s tree, as a render naming it draws `/D`'s states.

**Bounds, and what is not a node.** The tree is read 16 arrays deep, the
top level the first, as a visibility expression is (`MAX_EXPRESSION_DEPTH`),
and to 4 096 nodes in all;
an array past either bound is not read further and the tree says it was cut.
An array reached again through a reference while it is being read is a cycle,
and is not descended into a second time. An entry that is neither a group — a
dictionary whose `/Type` is `/OCG`, reached by reference — a nested array nor a
text string, a number or a name or a membership dictionary, is no node and is
passed over. Nothing in a malformed tree panics or loops.

### What a display list does

A list's calls carry the visibility its recording's states gave. A list keeps
those states beside its usage (RC-13), and a render binds its own: a render
naming the list's usage whose states equal the list's replays it — a render
whose override changes nothing, or whose magnification no `/Zoom` entry reads,
among them — and any other draws the direct way, through `Page::render`. So a
render naming another configuration is drawn as a direct render draws it,
pixels and warnings alike, and a list that is not retained is unaffected.
`Page::display_list_for` records under a usage and `Page::display_list_with`
under layer options too, so a viewer that keeps one configuration keeps
replaying. A list keeps the magnification its states were bound at, 1 for the
lists a caller asks for, and a render at another binds its own states to
compare only where the list's read a `/Zoom` entry; a form's kept drawing is
keyed by the magnification likewise, so documents that read none keep sharing
drawings across zoom levels.

## Milestones

1. **RC-14.1: this document.**
2. **RC-14.2: a chosen configuration.** Exit: `optional_content.rs`
   `each_configuration_renders_as_its_d_twin` (new) — a document with `/D` and
   `/Configs` entries of base state `/ON` with `/OFF`, `/OFF` with `/ON`, and
   `/Unchanged` with both, rendering under each byte-equal to its twin whose
   `/D` is written as that configuration's states, directly and from a display
   list; an index past `/Configs` and an entry that is no dictionary each
   rendering as `/D` and reported by name; `an_intent_the_configuration_does_not_hold_has_no_effect`
   (new), a `/Design` group off under `/D` drawn, and under a configuration
   whose `/Intent` holds `/Design` hidden; `layers.rs`
   `the_configurations_are_listed_in_configs_order` (new); `optional.rs`'s
   `one_render_binds_one_configuration` holding a chosen configuration to one
   binding.
3. **RC-14.3: overrides.** Exit: `optional_content.rs`
   `overrides_radio_buttons_and_locks_render_as_their_d_twins` (new) — one
   override on and one off, two overrides into one radio-button group in either
   order, a group in two radio-button groups, a configuration whose `/ON` turns
   two members of one group on, a locked group overridden and a locked member
   of a radio-button group, and an unlisted group, each byte-equal to its twin
   whose `/D` is written as the resulting states, each refused override's
   warning naming its group; `render_options.rs` pinning an overridden render's
   fingerprint through K-07's roster.
4. **RC-14.4: the zoom, language and user categories.** Exit:
   `optional_content.rs` `each_auto_state_category_renders_as_its_d_twin`
   (new) — per category and event, groups either side of each boundary: a
   magnification below, at and above `/min` and `/max`; an exact, a partial
   with `/Preferred /ON` and without, and an unmatched language, and a group
   with no `/Language` beside them; a matching and an unmatched user of each
   type, and a name array — each byte-equal to its twin with `/D` written as
   the resulting states; a malformed usage dictionary leaving its group as
   configured.
5. **RC-14.5: the `/Order` tree.** Exit: `layers.rs`
   `the_order_tree_is_the_configurations_own` (new) — nested labels and
   groups, sublayers after a group, a group listed twice, an array nested past
   the bound, an `/Order` containing itself, a configuration without `/Order`
   (inheriting `/D`'s) and one whose `/Order` names an unlisted group, each
   giving its known tree; `facade_read.rs` reading a builder-written
   document's tree back in the order the builder wrote it.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A fallback stops being visible-first.** Each stays as it is: an unreadable configuration renders `/D` and is reported; a missing or unknown `/BaseState` is `/ON`; an `/ON`, `/OFF`, `/RBGroups` or `/Locked` entry that is not a group is passed over; a group no configuration mentions, or one whose intents the configuration's set does not hold, is visible; a malformed usage dictionary recommends nothing; an override of an unlisted group changes nothing; a radio-button collection never hides what a configuration's own lists show. | Each fallback named here and held by a twin test in its step; `optional.rs`'s existing malformed-input tests unchanged |
| **`/Intent` moves pixels**: a group whose `/Intent` does not hold `/View` — a `/Design`-only layer — that `/D` turns off hides its content today, and draws it once RC-14.2 applies 8.11.2.3 | A correction the clause states; RC-14.2's commit names it; no fingerprint or golden fixture writes an `/Intent` |
| An override, a radio-button collection and a lock disagree about one group | One order (configuration, auto-state, overrides in order), one rule for each, and every combination a row of RC-14.3's twin table |
| A display list replays states another configuration's render does not have | It replays only where the render's bound states and usage are its own, and draws the direct way otherwise |
| A language tag compared in a way the clause does not say | The rule is stated — case-insensitive, whole tag or primary subtag — and RC-14.4's twins hold each side of it |
| An `/Order` tree that is deep, wide or cyclic costs without bound | 16 deep, 4 096 nodes, cycles not re-entered; the `render_page` fuzz target walks every configuration's tree |
