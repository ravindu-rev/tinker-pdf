# Forms

Interactive forms, three layers deep: the AcroForm field tree read into a
typed model (12.7), fills that rewrite the widget's appearance rather than
hoping a viewer will, and the form's own `/AA` scripts run through a bounded
ECMAScript subset the engine wrote for the purpose — calculate and format
by default, keystroke, validate and the document's own helpers under a
`ScriptPolicy` a host sets. One contract holds the layers together: **a
partial application never reaches the document**. A multi-field fill lands whole or not at all, a recalculation
that cannot run one script writes nothing, and the one degradation the
damage model allows — a widget with no `/Rect` to draw into — is named
rather than silent (rulings 2 and 10, [rulings](../rulings.md)).

## What it does

**Reading.** The field tree (12.7.3.1) is walked with a visited set and a
depth cap, because a `/Parent` cycle is as cheap to author as a `/Kids`
one. `/FT`, `/Ff`, `/V` and `/DV` inherit down the tree, with `/DA` and
`/Q` seeded from the form's own defaults; fully qualified names join each
ancestor's `/T` with dots (12.7.3.2). A kid carrying `/T` or `/FT` is a
child field, anything else is a widget of its parent, and a field merged
with its single widget (12.7.3.3) is modelled as a field that lists itself.
`FieldKind` classifies by `/FT` and the flags of 12.7.4.2 Table 228 —
`Text`, `Checkbox`, `Radio`, `PushButton`, `ComboBox`, `ListBox`,
`Signature` — which this module recognises and neither fills nor signs;
reading one is [signatures](signatures.md) — and `Unknown` for an `/FT` this
build does not know. A button's on state is discovered from its `/AP` `/N`
dictionary, never assumed to be `/Yes`. Values decode per type
(`Text`/`State`/`Many`/`None` — a button with no `/V` is `Off`, which is a
state rather than an absence), and choice options come from `/Opt` in both
its string and `[export, display]` forms (12.7.4.4).

**Creating.** `DocumentEditor::add_field(&NewField)` makes a text field, a
check box, a radio group or a choice field (a combo box or a list box), and
the field is a field at once: `fields()` in the same editor finds it and
`fill_field` fills it, because both walk the tree through the editor's own
overlay. A text, check box or choice field is merged with its one widget
(12.7.3.3); a radio group is one field whose `/Kids` are a widget per
button, each on whatever page the caller put it. Every widget is added to
its page's `/Annots`, carries `/P` and `/F 4` (Print), and has an appearance
for every state it can be in: a text or choice field is drawn by **the fill
layer's own path** from the field as the tree walk reads it back, so creating
a field with a value and filling it with that value produce the same
appearance; a check box and each radio button get an `/Off` appearance and
one for their on state keyed by the export value, drawn as paths rather than
ZapfDingbats glyphs so no second font has to be in `/DR`, and `/AS` selects
between them (12.7.4.2). A `/DA` names `/Helv`, and a form whose `/DR` has no
`/Helv` is given Helvetica — once, however many fields are made; a form that
has one keeps it. An initial value is written as `/V` and `/DV` both, so a
reset (12.7.5.3) returns to it. A dotted name is a hierarchy (12.7.3.2):
`a.b.c` joins or creates the non-terminal fields `a` and `a.b`. `/Ff` is
written on every created field, zero included, because it is inheritable and
a check box created under a node carrying the Radio bit would otherwise read
back as a radio group. Everything that can refuse is checked before anything
is written, inside a transaction, and each refusal is an `AddFieldError`
naming what was wrong.

**The `/DA` font is found through the editor.** `text_appearance` used to
look the `/DA` font up in the *file*, so a field created and filled in one
editor was laid out against a `/Helv` the file did not have yet — measured
at half an em a character, which auto-sizes forty `i`s to 4.8 points where
Helvetica's own widths give 10.8. The font dictionary is now read through the
view. What is still read from the file is a font's subsidiary objects — its
descriptor, `/Widths` array and program — because `font::read` takes a
`CosDocument`: a standard-14 font, which is what a created field names, has
none, and a composite font an editor adds together with its program is the
case that remains.

**Filling.** Setting `/V` is the easy half and the useless half: a value
with no matching appearance shows only in viewers that regenerate, which is
why filled forms so often print blank. So every fill rebuilds the widget's
`/AP` — a form XObject sized to the widget's `/Rect` at the origin, which
is what 12.5.5's mapping expects — and clears `/NeedAppearances` (12.7.2)
rather than setting it. Layout honours the `/DA` font, size and colour
(replayed verbatim, so an operator this build does not interpret still
comes out right), `/Q` quadding, multiline wrap, auto-size for `Tf 0`, and
comb fields (12.7.4.3): `/MaxLen` equal cells, one character centred in
each, overflow dropped rather than drawn outside the last box. The value is
a text string written by the shared encoder (7.9.2.2,
[document-model](document-model.md)): PDFDocEncoding where it carries the
value, otherwise UTF-16BE behind `FE FF`, or UTF-8 behind `EF BB BF` in a
document declaring 2.0 or later.

**Non-Latin values are shaped** (milestone 8 of
[design/shaping.md](../design/shaping.md)). Until it landed, this module
mapped every character above the single-byte range onto `?` and said
nothing, so a form whose Arabic field had become a row of question marks
was indistinguishable from one that had been filled correctly. What
unblocked it was `tinker_pdf_cos::Font::program`, which walks
`/DescendantFonts` → `/FontDescriptor` → `/FontFile2` (or `/FontFile3`,
or `/FontFile`) and returns the stream's *address* — not its bytes, which
would put every embedded program in a document into memory the moment its
resources were read. Where the `/DA` font is composite, horizontal, and
embeds a program `tinker_pdf_font::Sfnt` reads, the value is shaped
through `tinker-pdf-shape` and written as a `TJ` run of codes: joining
forms, marks positioned by `GPOS`, and UAX #9's rule L2 applied before
anything is written, so a right-to-left value is drawn in the order a
reader of it expects. The advances the run is measured at are the shaper's
own, and the `TJ` numbers absorb the difference between those and the `/W`
a viewer will advance by, glyph by glyph — one measurement path per run,
which is the rule `tinker-pdf-layout`'s `metrics.rs` states.

**Which encodings.** Milestone 8 shipped `/Identity-H` and nothing else,
because going from a glyph back to a *code* means reading an encoding CMap
backwards and 9.7.5's are written to be read forwards. "Written to be read
forwards" is not "not invertible": `tinker_pdf_font::CMap::code_for_cid`
gathers every code a CMap's own tables could have meant by a CID, walks
them in ascending order and returns the first that maps **back**, so the
round trip is checked rather than assumed — a `cidchar` that overrode a
`cidrange` cannot be inverted into a code that now means something else,
and a CID nothing means any more is refused rather than guessed. The code
comes back with its byte width, because 9.7.6.2's codespaces are what
decide where one code ends and the next begins and `90ms-RKSJ-H` has both
widths in one CMap. So four cases now fill: `/Identity-H`; an **embedded
CMap stream**, whose tables are the document's own; a **predefined
registry CMap** of 9.7.5.2, where this build compiled its table in; and
any of those over a non-identity `/CIDToGIDMap`, which is what every
subset font in the wild has. `Font::cid_for_gid` inverts that last step
through an index built once per font rather than by scanning the table per
glyph.

**Three more fonts shape (October 2026).** The ROADMAP named them as the
refusals left, and each now has a fixture whose operators the test computes:

- **A vertical CMap** (9.7.4.3) is written as a **column**: the glyphs are
  found as above, `GSUB` runs `vert` and `vrt2` in place of the horizontal
  features and no `GPOS` runs, and the pen goes down the box's centre line —
  a glyph is drawn displaced by its position vector, whose horizontal half
  is half its width, so the column is centred — advancing by each CID's own
  `/W2` displacement, so the run carries no `TJ` numbers. `/Q` reads down
  the column (top, centred, bottom); a multiline field's lines are columns
  from the right; auto-sizing fits the longest column to the height.
- **A bare CFF** (`/FontFile3 /Subtype /CIDFontType0C` or `/Type1C` under a
  `CIDFontType0`) has no `cmap`, no `hmtx` and no `GSUB`, and the shaper
  takes an sfnt, so the program is **wrapped** per line in the smallest
  sfnt that answers the shaper's questions: a `cmap` from each character to
  the code the font's own `/ToUnicode` gives it read backwards
  (`Font::code_for_char`, the lowest such code), that code to a CID through
  the encoding, kept only where the program's charset carries the CID, and an
  `hmtx` from `/W`. The value is drawn at the advances a reader will use, in
  UAX #9's visual order; nothing joins, because a CFF carries nothing to
  join with.
- **A simple TrueType font** is shaped against its embedded sfnt, and each
  glyph written as the lowest byte that reaches it — the code whose
  `/Encoding` character (9.6.6) the program's `cmap` maps to that glyph — so
  `GPOS` kerning and placement reach the field as `TJ` numbers and `Ts`. A
  line that needs a glyph no byte reaches — a ligature, a joined form —
  keeps the single-byte path whole, which is what it drew before.

Anything else keeps the single-byte path, still draws a `?`, and emits
`WarningKind::FieldCharacterUnrepresentable { character }` against the
field's own object for every character it could not write (rulings 2
and 10): a symbolic simple font, or one that is not TrueType or embeds no
sfnt; a vertical CMap over a CFF; a CFF whose font has no `/ToUnicode`, so
nothing in the document says which code means which character; a vertical
**comb** field, whose cells 12.7.4.3 lays across the box; and a program that
is neither an sfnt nor a CFF.

**The feature gate is declared, not silent.** The registry's code-to-CID
tables are 1.19 MB behind the `cmap-predefined` cargo feature
([fonts.md](fonts.md)), so a `--no-default-features` build reads a
`UniJIS-UCS2-H` field's codespaces and widths and has nothing to invert.
That build refuses the shaped path and emits
`WarningKind::PredefinedCMapApproximate` **against the field**, naming the
CMap whose table is missing, before the per-character warnings. The
alternative — filling with `?` and saying only that the characters were
undrawable — would make a capability's absence look like a document's
defect, which is the failure PDF/A named once already: a verdict that
depends on a feature is not a verdict, and neither is a fill. The two
paths that need no table at all — `/Identity-H` and an embedded CMap
stream — work in either build, and `shaped_forms.rs` asserts both legs.

A value
the field refuses — over `/MaxLen`, not among a non-editable list's
options, or written by a user into a ReadOnly field (12.7.4.1 Table 227) —
is refused whole, because truncating hides a data error inside a file that
then looks correctly filled. Checkboxes and radio groups set `/V` and every
widget's `/AS` together, all widgets or none — through `set_checkbox` and
`select_radio`, or through `fill_field` and `set_field_values` given the
*name* of the state to show (`On`, `blue`, `Off`), which is what an FDF or
XFDF file carries for a button. A state no widget's `/AP /N` offers is
refused rather than written, because a `/V` naming a state nothing can draw
is a box that reads as ticked and displays as empty. That door is the
user's: `set_calculated_values` refuses a check box or radio group, as it
always has, since no calculation in this build computes a button state and
a ReadOnly box is not one to tick on a script's word. `reset_form` restores `/DV`
into `/V` and removes `/V` where there is no `/DV` (12.7.5.3) — "never
filled" and "filled with nothing" are different states.

**Exchanging form data: FDF and XFDF** (`tinker_pdf::form_data`). Both
directions, through one model: a `FormData` is a list of `FieldData` —
a fully qualified name and the `FieldValue` the field-tree reader already
uses — plus the source document's name and the warnings. `FormData::from_fields`
exports from `Document::form_fields()` or `DocumentEditor::fields()`, the
walk that joins `/T` with periods and inherits `/V`; `apply(editor, &data)`
imports through `set_field_values`, so a name resolves against the same
walk, a value a field would refuse from a user is refused, and the first
refusal rolls back every field before it. A check box or radio group takes
the name of its state, as the filling paragraph above says. FDF (12.7.8) is
read by the same object reader every PDF is: the `/FDF` dictionary's
`/Fields` tree, `/T` partial names joined as 12.7.3.2 joins them, `/V` as a
text string, a name or an array of either, and `/F`; `/Kids` is walked with
a visited set holding every field, every `/Kids` array and every link of a
reference chain to either — so a `/Kids` array two fields share, or one
whose entries name it as their own `/Kids`, is walked once and the second
parent named as a `TreeCut` — and the field tree's own depth bound. What
both readers hand back is held to one budget, `MAX_FORM_DATA_BYTES` (64 MiB
of names, values and warnings), charged before each copy is made, because a
copy is where a small file became a large allocation: a field's name was
copied into every warning met inside it, and one indirect `/T` or `/V` into
every field beneath or beside it, so 67 KiB of FDF asked for 184 MB and
22 KiB for a gigabyte. A file that asks for more is refused whole,
`FormDataError::TooLarge`, since part of a form's data imports as a
different form. An entry whose qualified name is empty — no `/T` or `name`,
or an empty one, and no named ancestor — is not read but named
(`FormDataWarning::Unnamed`), and `apply` refuses the empty name in data
built by hand: the field-tree walk gives `""` to every field with no `/T`
up its tree, so it would land in whichever of those came first. Written, the qualified
names go back into a tree — `/T` is a *partial* name — a state is a name,
and a cross-reference table is included although 12.7.8 makes it optional.
XFDF is the XML form, read by `tinker-pdf-xml` under its default bounds,
which refuse a document type declaration outright. **What XFDF is read as is
the commonly documented core** — `<xfdf>`, `<f href>`, `<fields>`, nested
`<field name>`, repeated `<value>` — because the specification that defines
it, Adobe's *XML Forms Data Format Specification*, standardised as ISO
19444-1, was not available to this build; a value is text, and the field it
lands in decides whether `On` is a state. Everything either reader meets and
does not read — FDF's `/Annots`, `/Pages`, `/JavaScript`, a field's `/AP`,
`/Ff`, `/SetFf`, `/Opt`, `/RV`; XFDF's `<annots>`, `<ids>`,
`<value-richtext>` — is named in `FormData::warnings` rather than skipped.
A value XML 1.0 cannot carry (a C0 control other than tab, line feed and
carriage return) is refused by `to_xfdf` rather than written some other way,
and a carriage return is written `&#13;` so it does not come back a line
feed. A name with an empty partial name (`.x`, `a..b`) is written whole, and
a name deeper than a quarter of the object parser's nesting bound keeps its
tail in the deepest `/T`, so every name reads back as the name it was.

**Transactions.** `DocumentEditor::transaction` snapshots the editor's
whole mutable state — overlay, deletions, page order, trailer entries and the
object-number counter — runs a closure, and restores everything on `Err`. A closure
rather than a begin/commit/rollback triple because the failure it prevents
is silent: there is no way to leave the scope without either committing or
rolling back. It nests, the snapshot copies only what has been edited (the
document underneath is immutable behind an `Arc`), and the counter is
restored so a failed edit does not grow the next saved file — at the stated
cost that an `ObjRef` allocated inside a rolled-back transaction is void.
`set_field_values` is the all-or-nothing multi-field apply built on it: the
first refusal rolls back every field before it and returns a
`FillRejection` naming which field and why.

**Damage.** 12.5.2 Table 164 makes `/Rect` required for every annotation,
so a widget without one is a damaged file. Ruling 2 degrades rather than
fails — the value is written and every drawable widget is drawn — and
ruling 10 forbids the degradation to be silent: each undrawn widget comes
back as a typed `SkippedWidget` naming the object. A caller that wants
all-or-nothing over that too checks `skipped.is_empty()` or uses
`set_field_value`, whose `bool` is true only when the field applied whole.

**Calculations.** `recalculate()` runs the `/AA` `/C` actions in `/CO`
order (12.7.2 Table 218), then any remaining calculating field in document
order, one pass, each action at most once. Scripts run through a
hand-rolled ECMAScript subset (`crates/tinker-pdf-cos/src/script.rs`) that is PDF-free behind
a two-method `Host`: numbers, strings, arithmetic, comparison, logical and
conditional operators, `if`/`else`, `var`, `while` and C-style `for`,
blocks, `return`, arrays, member access, indexing, calls, the `event`
object, `getField`, calls to the document's own helpers (7.7.4, under policy), and
the Acrobat helpers `AFSimple_Calculate`,
`AFNumber_Format`, `AFPercent_Format`, `AFDate_Format` and
`AFSpecial_Format` (12.6.4.16 Table 217; `/JS` is read in both its string
and stream forms, 7.3.8). Every write a script makes lands in a staging map
— visible to later scripts, which is what makes `/CO` order mean something,
invisible to the document — and the pass applies through
`set_calculated_values`, the same all-or-nothing door, where ReadOnly binds
the user and not the document's own action. Any script that cannot run
refuses the whole pass: running nine scripts and skipping the tenth is a
document whose totals disagree with its inputs. A cascade cycle terminates
by construction — one pass, no re-entry — and the one place that rule and a
full fixed-point disagree is reported in `Recalculation::cascades_cut`.

A hostile script terminates under three independent bounds, because none
substitutes for another: **depth** (parse nesting capped at 32, which also
bounds the evaluator's stack), **work** (every statement and expression
node charges a step — 20 000 per script, 200 000 per pass, so a document
cannot multiply per-script caps by carrying more scripts), and **size**
(64 KiB per script, 4 MiB of source per read of the document, 16 384 tokens,
8 192-byte strings, 1 024-entry arrays, 256 variables, 4 096 calculating
fields).

**The 4 MiB is one total, not three.** A document keeps its scripts in three
places — the field tree's `/AA`, `/Names /JavaScript` and the catalog's
`/AA` — and each walk used to start from the full total, so a file that
filled all three surfaced twelve mebibytes. `ScriptBudget` is that total as a
value the caller carries: `fields_within`, `document_scripts_within` and
`catalog_scripts_within` spend one between them, in that order (the order is
fixed because which scripts come back as source and which as
`Script::Oversize` depends on it, and determinism is a contract — ruling 4).
`script_summary` — the one call that reads all three surfaces —
threads one. The bare `fields`, `document_scripts` and `catalog_scripts` are
each **one read of one surface** and each start from the full total; a caller
that reads more than one and wants the document's answer threads a budget.
The alternative, a budget living inside `CosDocument`, was rejected because
it would make reading a document mutate it and the same document read twice
answer differently. One thing the shape of the defect says out loud: the
catalog could never have tripled the total on its own, because 12.6.3
Table 200 defines five triggers and 64 KiB each is a 320 KiB ceiling.

**A form's own helpers now resolve** (7.7.4). `/Names /JavaScript` is where
a generated form keeps the functions its `/AA /C` scripts call, and until
`ScriptScope` existed the interpreter met the first call to one, raised
`ScriptError::UnknownName` and refused the whole pass — a correctly authored
file this build could not compute at all. Under a policy that allows
`Trigger::Document`, every document-level source is read into a **name table
of function definitions and nothing else**, and the table is consulted after
every builtin, so a document cannot redefine `getField` by declaring a
function of that name. Anything at document scope that is not a definition is
`ScriptError::NotADefinition`, named and refusing rather than skipped: a
skipped statement builds a table silently missing whatever it would have
defined.

Definition is document scope's production alone. `function` stays reserved
everywhere a *field* script is parsed, so a calculate action gained the
ability to **call** a helper and never to declare one — one place declares,
and it is the one the policy gates. A helper's body is read by the same
statement productions a calculate action's is, so nothing the subset excludes
can arrive through one. Recursion is refused by name
(`ScriptError::Recursion`) rather than bounded by a counter, direct or
mutual; a chain is therefore at most as long as the table has distinct
functions, which `MAX_SCRIPT_FUNCTIONS` caps at 256. Work is the pass's own
budget: a helper that does not terminate cheaply stops the pass. Each call
gets a fresh frame holding its parameters, so a helper can neither read nor
overwrite its caller's locals; missing arguments are `undefined` and extra
ones are dropped.

Denying `Trigger::Document` gives an **empty table**, not a refusal, and the
distinction is deliberate: a document-level script is not something the pass
is asked to run but a resource it may consult, and refusing would make every
form carrying a `/Names /JavaScript` block uncomputable under the default.

**Keystroke and validate need an event, so they are entry points rather
than something a pass fires.** A calculate action runs against the document as
it stands; a keystroke action runs against what is being typed, where the
caret is, and whether this is the commit at the end — three facts a host has
and a reader does not — and a validate action runs against a value somebody
committed. That, and not any limit of the interpreter, is why both sat
surfaced-and-never-run. `DocumentEditor::keystroke(field, event, policy)` and
`DocumentEditor::validate(field, value, policy)` take an event a host built
and answer an `EventVerdict`: `Accepted(text)` — which for a keystroke is
`event.change` as the script left it, because a keystroke action rewriting
what is being typed is how every digits-only field in the wild works — or
`Refused`. A refusal is the action **working**, never an error: a form that
rejects a date in the wrong century is doing its job, and `CalcError` is
reserved for scripts that could not run at all. A field carrying no action of
that class accepts, under every policy, because there is nothing to consult.
Both run against the read-only host a format action gets, so an event script
that tries to write a field is `ScriptError::FieldRefused`.

**A computed value goes through its own validate action** (12.7.2), at the
last moment in the pass when nothing has been written. A refusal aborts the
**whole** pass — `CalcError::Invalid`, naming the field and the value —
because a form whose validation rejects one total and whose other nine were
written anyway is a document that disagrees with itself. Under a policy that
denies validation the pass still runs and `Recalculation::refused` names every
field it wrote **without** checking: ruling 10 applied to a check rather than
a repair, since a pass that silently skipped a form's own validation reads
exactly like a pass over a form that has none.

**Two strings compare as strings** (11.8.5). This engine compared every
relational operator by number until the keystroke entry point arrived, and
what found it is the commonest keystroke script there is —
`event.change >= '0' && event.change <= '9'`, under which `'!'` is `NaN`,
every comparison against it is false, and a digits-only field silently accepts
everything. Both operands strings compares by code point; anything else is
still arithmetic, which is what keeps `getField('a').value > 500` meaning what
it says.

**The format string cannot reach `/V`, and a type says so.** 12.7.3.3 keeps
a field's value and its appearance apart, and until this milestone that was
held up by `formatted_value` simply not calling the editor — a property of
how the code was arranged rather than a rule anything enforced. The first
caller to write `editor.set_field_value(name, formatted)` would have produced
a `/V` of "GBP 1,234.00" and nothing would have objected. So
`formatted_value` returns a `DisplayString`: a newtype with no `Deref`, no
`Into<String>` and no constructor outside `calc.rs`, and every door into the
document — `set_field_value`, `fill_field`, `set_field_values`,
`set_calculated_values` — takes `&str`, which one of these cannot be spelled
as. `DisplayString::text` is the way out and is deliberately a spelling
rather than a coercion: a caller who wants the characters writes `.text()`,
and one who writes `.text()` into `/V` has made a decision a reviewer can
see. What the type removes is the mistake nobody makes on purpose.

**Which scripts run is a policy, and it is a type**
([design/form-script-policy.md](../design/form-script-policy.md) carries the
reasoning and the alternatives that were rejected). `ScriptPolicy` names
the six trigger classes a document's scripts arrive under — `Calculate`
(`/AA` `/C`), `Format` (`/F`), `Keystroke` (`/K`), `Validate` (`/V`),
`Document` (`/Names /JavaScript`, 7.7.4) and `Catalog` (12.6.3 Table 200's
`WC WS DS WP DP`) — and says which of them this host allows to run. Deny by
default, with two exceptions that are the shipped behaviour written down
rather than a judgement about safety: `Calculate` and `Format` are allowed
because `recalculate()` and `formatted_value()` have run them since the
interpreter landed, and a type that silently turned an existing capability
off would be a breaking change wearing a safety argument. The other four are
denied because nothing ran them before the type existed, and a default that
starts running document program text on the strength of a new struct is
exactly the change nobody reviews.

A denied trigger is a **named refusal**, never a silent skip:
`CalcError::Refused` carries the trigger and the field or script that
carried it (ruling 10), because a pass that quietly ran nothing is
indistinguishable from a form with no scripts in it. It is raised only
against a script that is *there* — a form with no calculate action
recalculates to the same empty answer under every policy, since refusing an
absent script would collapse "this host does not run calculations" into
"this document has none".

`recalculate()` and `formatted_value()` keep their signatures and run under
`ScriptPolicy::default()`; `recalculate_under()` and
`formatted_value_under()` take one. Nothing recalculates automatically —
when a calculation runs is host policy, so `recalculate()` is an explicit
call.

**The policy, in full.** What each trigger gates, what it is by default, and
what a host gets by changing it.

| Trigger | Where the script lives | Default | Allowed | Denied |
| --- | --- | --- | --- | --- |
| `Calculate` | a field's `/AA /C` (12.6.3 Table 198) | **allowed** | `recalculate()` runs the `/CO` order and applies the result, all or nothing | `CalcError::Refused`, naming the first calculating field — nothing is written |
| `Format` | a field's `/AA /F` | **allowed** | `formatted_value()` answers a `DisplayString` no write door will take | `CalcError::Refused`, naming the field |
| `Keystroke` | a field's `/AA /K` | denied | `DocumentEditor::keystroke(field, event)` answers an `EventVerdict` for an event the host built | `CalcError::Refused` — there is no implicit path to reach it, so nothing else changes |
| `Validate` | a field's `/AA /V` | denied | `DocumentEditor::validate(field, value)`; and a computed value is checked before it is committed (12.7.2), a refusal aborting the whole pass as `CalcError::Invalid` | the pass still runs, and `Recalculation::refused` names every field it wrote unchecked |
| `Document` | `/Names /JavaScript` (7.7.4) | denied | the sources are read into a `ScriptScope` of **function definitions only**, and a field script calling one resolves | the table is empty, so a call to a helper is the `ScriptError::UnknownName` it always was — a refusal here would make every form carrying such a block uncomputable |
| `Catalog` | the catalog's `/AA` (12.6.3 Table 200) | denied | **nothing**. No entry point in this build runs a catalog action | nothing. Declarable and inert — see below |

`Trigger::Catalog` is the one row with no consumer, and it is written down
rather than left to be discovered. `WC`, `WS`, `DS`, `WP` and `DP` are
will-close, will-save, did-save, will-print and did-print; every one needs an
event a document reader has no notion of, and none of them is document-open,
which is the one `/Names /JavaScript` covers. The bit exists because it names
a real trigger class a host has to be able to deny and because the projection
onto the C ABI is 1:1 (ruling 11). `form_document_scripts.rs`'s
`allowing_the_catalog_trigger_changes_no_answer` is the assertion that says
so, so that a guard holding nothing is a measured fact rather than an
assumption.

## API

On `Document`: `form_fields()` returns the typed `Field` model —
`name`, `kind`, `value`, `default`, `flags` (with `is_read_only()` /
`is_required()`), `widgets`, `options`, `max_len`, `default_appearance`,
and `scripts` (a `FieldScripts` of the four `/AA` sources, each a
`Script::Source` or `Script::Oversize`). `calculation_order()`,
`document_scripts()` and `catalog_scripts()` surface `/CO` and the
document's own scripts; `script_summary()` counts what those walkers see —
the fields' `/AA` scripts, `/CO`, `/Names /JavaScript` and the catalog's
`/AA` — for a caller that has to warn before filling; reading a script runs
nothing. It is a form's count, **not every script a document carries**: it
does not look at `/OpenAction`, a page's `/AA`, an annotation's `/AA` or
`/A`, an outline item's `/A` or an action's `/Next` chain, all of which a
viewer runs. `DocumentEditor::sanitise` sweeps every object for that reason
([editing](editing.md)). Each of the
three has a `_within` sibling taking a `ScriptBudget`, for a caller reading
more than one surface under one total.

Mutation goes through `Document::editor()`, a `DocumentEditor`:
`add_field`, `fill_field`, `set_field_values`, `set_field_value`,
`set_checkbox`, `select_radio`, `reset_form`, `transaction`, `recalculate`,
and `set_calculated_values` for a host that computes values itself.
`add_field` takes a `NewField` — a fully qualified name, a `NewFieldKind`
(`Text`, `Checkbox`, `Radio` with its `RadioButton`s, `Choice`) carrying the
page, the `Rect` and the initial value, the caller's `/Ff` bits and a `/DA`
font size — and answers the terminal field's `ObjRef` or an
`AddFieldError`; the facade re-exports all five, and `Rect` with them.

Form data lives in the facade module `tinker_pdf::form_data`:
`read_fdf(bytes)`, `read_xfdf(bytes)`, `FormData::{from_fields, to_fdf,
to_xfdf}`, `apply(editor, &data)`, the types `FormData`, `FieldData`,
`FormDataWarning` and `FormDataError`, and the readers' budget
`MAX_FORM_DATA_BYTES`. It is not yet projected through the C
ABI or the bindings.

```rust
let data = form_data::FormData::from_fields(&document.form_fields());
let xfdf = data.to_xfdf()?;                      // or data.to_fdf()
let mut editor = other.editor();
form_data::apply(&mut editor, &form_data::read_xfdf(xfdf.as_bytes())?)?;
```
`recalculate_under` takes a `ScriptPolicy`; `formatted_value`, `keystroke`
and `validate` take one too, and are the format event and the two event entry
points. The free functions behind them are
`tinker_pdf_cos::calc::{formatted_value, formatted_value_under, keystroke,
validate}` — the last two are reached module-qualified rather than from the
crate root, because that root already has a `validate` and it is the strict
structural validator. The facade re-exports `FillError`, `FillRejection`,
`SkippedWidget`, `WidgetDefect`, `CalcError`, `Recalculation`, `ScriptError`,
`ScriptPolicy`, `Trigger`, `ScriptScope`, `ScriptBudget`, `Keystroke`,
`EventVerdict` and `DisplayString` (ruling 11).

**Across the C ABI** ([bindings](bindings.md)), 1:1 and with no logic of its
own: `tpdf_editor_recalculate`, `tpdf_editor_formatted_value`,
`tpdf_editor_keystroke` and `tpdf_editor_validate`, with the pass's four
lists behind `tpdf_recalculation_*` and freed by
`tpdf_recalculation_free`. The policy crosses as a bitmask of the
`TPDF_SCRIPT_*` constants — a bitmask rather than a struct of six `int`s,
because a struct is ABI a later trigger class could not be appended to.
Bits this build does not define are ignored rather than refused, so a host
compiled against a later header gets this build's answer. A script that
would not run, and a policy that would not let it, are one status:
`TpdfStatus::ScriptRefused` (15), because from a caller's side both mean
nothing was written; which script and why crosses through
`tpdf_last_error_message`, since a status code cannot carry a field name and
inventing seven codes for seven refusals would be a vocabulary the facade
does not have. **A refusal from a keystroke or validate action is not one of
them**: that is `Ok` with `*out_accepted` zero, because a form saying no is
the form working.

```rust
let doc = Document::open(bytes)?;
let mut editor = doc.editor();

// All of these fields land, or none of them do.
editor.set_field_values(&[("net", "100"), ("rate", "0.2")])?;

// /CO order; Err means nothing was written, and names the field.
let pass = editor.recalculate()?;
for (name, value) in &pass.changed { /* regenerated appearances */ }
for skip in &pass.skipped { /* a widget with no /Rect, by object */ }

let bytes = editor.save(&WriteOptions::default());
```

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| `eval`, `try`, `switch`, `for...in`, `typeof`, `delete`, `with`, `class`, `let`/`const`, `import`/`export`, regular expressions, object literals, prototypes — and `function` anywhere but document scope | `ScriptError::Syntax` — each word reserved and refused outright | a construct silently approximated is one rename away from running it | — |
| Anything at document scope that is not a function definition | `ScriptError::NotADefinition`, naming the `/Names /JavaScript` key through `CalcError::DocumentScript` | a skipped statement builds a name table silently missing what it would have defined | — |
| A document-level helper that calls itself, directly or through another | `ScriptError::Recursion` | a depth cap makes the answer depend on a number nobody can predict from the file — the cascade rule's argument | — |
| More than 256 document-level functions | `ScriptError::TooManyFunctions` | a name table is a lookup scanned per unknown name, so a document-controlled count of them is document-controlled work | — |
| `app.*` and `console.*` | inert stubs; assigning a stub's result to a field is `ScriptError::NotStorable` | a stub's result must never become a field value | — |
| A name or member outside the subset | `ScriptError::UnknownName` / `ScriptError::UnknownMember` | a calculation that guesses is a form that lies | — |
| A script that does not terminate cheaply, or outgrows the size caps | `ScriptError::OutOfSteps` / `TooDeep` / `TooManyTokens` / `StringTooLong` / `ArrayTooLong` / `TooManyVars` | three independent bounds — depth, work, size — because none substitutes for another | — |
| Script source past 64 KiB, or past what one read of the document has left of its 4 MiB `ScriptBudget` | `Script::Oversize(len)`; running it is `ScriptError::TooLong` | truncated source means something different from what the file says (ruling 10) | — |
| More than 4 096 calculating fields in one pass | `CalcError::TooManyFields` | refused rather than truncated, for the same reason a failing script refuses the pass | — |
| A value the field will not take — over `/MaxLen`, not an option, ReadOnly against a user write, a button state no widget offers | `FillError::ValueRefused` (in a multi-field apply, `FillRejection` names the field) | refusing beats truncating, which hides a data error in a file that looks filled | — |
| Creating a field under a name that is taken, or beneath a terminal field | `AddFieldError::NameTaken` / `AncestorIsTerminal` | two fields answering to one name are one field a filler cannot address; a terminal field's kids are widgets | — |
| Creating a field whose flags decide a different kind, or a list box marked editable | `AddFieldError::FlagsContradictKind` | Radio, Pushbutton, Combo and Edit are what `NewFieldKind` says, and a second answer to that question is a field that reads back as something else | — |
| A button export value that is empty, `Off`, repeated, or not a name | `AddFieldError::ExportUnusable` | 12.7.4.2.3 reserves `Off`, and two buttons answering to one state are one button | — |
| XFDF beyond the commonly documented core, and FDF beyond `/Fields` and `/F` — annotations, page templates, JavaScript, appearances, flags, rich text | `FormDataWarning::NotRead`, naming the key or element and the field | ISO 19444-1 was not available to this build, and a skipped construct would read as one that was not there | — |
| An encrypted FDF | `FormDataError::Encrypted` | reading one needs a key, and this reader takes none | — |
| An FDF or XFDF that asks for more than 64 MiB of names, values and warnings | `FormDataError::TooLarge` (`MAX_FORM_DATA_BYTES`) | each copy repeats something the file says once, so a small file can ask for terabytes; part of a form's data imports as a different form | [ruling 1](../rulings.md) |
| An entry with no name, read or imported — no `/T` or `name` anywhere up its tree | `FormDataWarning::Unnamed` when read; `FillError::NoSuchField` from `apply` | `""` addresses whichever of the document's nameless fields comes first, not the one the data meant | — |
| A multiple selection imported into a field | `FillError::ValueRefused` through `FillRejection` | this build fills one value per field; half a selection is a different answer | — |
| A value XML 1.0 cannot carry, written as XFDF | `FormDataError::NotRepresentable`, naming the field | written any other way it would come back different; FDF carries it | — |
| A widget missing 12.5.2 Table 164's `/Rect` | `SkippedWidget` with `WidgetDefect::RectMissing` | the value is written and drawable widgets drawn; the damage is named, never silent (rulings 2, 10) | [rulings](../rulings.md) |
| Shaping a value against a symbolic or non-TrueType simple `/DA` font, a vertical CMap over a CFF, a CFF with no `/ToUnicode`, or in a vertical comb field | `WarningKind::FieldCharacterUnrepresentable { character }` per character; the single-byte path draws `?` | a symbolic font's codes name glyphs rather than characters; the wrapper reads codes from `/ToUnicode` and a column from an sfnt; 12.7.4.3 lays comb cells across the box | [design/shaping.md](../design/shaping.md) |
| Shaping a value under a **registry CMap** in a build without `cmap-predefined` | `WarningKind::PredefinedCMapApproximate(name)` against the field, then the per-character warnings | the code-to-CID tables that would be inverted were never compiled in — a capability that depends on a feature has to say so | [fonts.md](fonts.md) |
| A CID no code means any more — a `cidchar` took the code its `cidrange` would have given | `WarningKind::FieldCharacterUnrepresentable { character }`; nothing is written for that glyph | the inverse of a CMap is not a function, and an unverified inverse draws a *different* wrong glyph | [rulings](../rulings.md) ruling 10 |
| A trigger class the policy denies — keystroke, validate and document-level by default | `CalcError::Refused { trigger, subject }` | a pass that quietly ran nothing reads exactly like a form with no scripts (ruling 10) | — |
| A computed value a field's own `/AA /V` refuses | `CalcError::Invalid { field, value }`, and the whole pass writes nothing | one total rejected and nine written anyway is a document that disagrees with itself | — |
| A validate action the policy would not run, over a value the pass wrote anyway | `Recalculation::refused` names the field | a skipped check reads exactly like a form that has none, and the difference is whether the numbers were looked at (ruling 10) | — |
| A catalog action (`WC`, `WS`, `DS`, `WP`, `DP`) | `Trigger::Catalog` exists and **nothing in this build runs one**; allowing it changes no answer | every one of the five names an event a reader has no notion of, and none of them is document-open | — |
| A format action's display string reaching `/V` | `DisplayString`, which no write door will take — `error[E0308]`, proved by compiling the mistake | 12.7.3.3 keeps value and appearance apart, and a type is a guarantee where an arrangement of code was a convention | — |
| Automatic recalculation | none offered — `recalculate()` is explicit | when a calculation runs is a host's policy, not the engine's | — |
| Filling or signing a **signature field** | `FieldKind::Signature` recognises it and this module does neither | a signature field's value is a CMS blob over a `/ByteRange`, not text a fill layer could lay out; producing one is `DocumentEditor::save_signed` and reading one is `Document::verify_signatures` | [signatures](signatures.md) |
| XFA | not read anywhere | removed in ISO 32000-2; a stated permanent non-goal | [ROADMAP](../ROADMAP.md) |

## Verified

Unit tests sit beside the code: 17 in `form.rs` (tree walk, inheritance,
merged widgets, qualified names, on-state discovery, cyclic trees, script
surfacing including the stream form and the oversize refusal), 18 in
`fill.rs` (the `/DA` split, quadding, multiline, auto-size shrink, comb
cells and their overflow, escaping, UTF-16BE values, `/MaxLen` and
ReadOnly refusals), and 53 in `script.rs` (arithmetic through the AF
helpers, each excluded construct refused by name, the inert `app.*` stubs,
the policy defaults, and the document-level name table — arity, frames,
recursion, the function cap and a hostile document scope that never panics).

`crates/tinker-pdf-cos/tests/form_transactions.rs` (20 tests) holds up the
atomicity claims against hand-written fixtures: a successful fill produces
the bytes it always did, rollback restores objects, deletions, page order
and the object-number counter, abandoned edits do not grow the next saved
file, a refused field rolls back the ones before it, and a widget without
a `/Rect` is reported by object number rather than skipped in silence.
`crates/tinker-pdf-cos/tests/form_calculations.rs` (29 tests) runs an
invoice fixture end to end — `/CO` order honoured, a failing script
leaving the editor byte-identical, ReadOnly totals written by the
calculation and refused to the user, cascades cut and reported, and the
format string never landing in `/V`. Its module header carries the six
policy defaults flipped one at a time and how many assertions each flip
fired, four of which are zero and say so.

`crates/tinker-pdf/tests/form_creation.rs` (9 tests) is the creation row's
exit criterion, on a file with no form and on `testdata/form-fields.pdf`:
each of the four kinds created, found by `fields()` in the same editor,
filled through `fill_field`, saved incrementally and as a rewrite, reopened,
read back with its value and held clean by the strict validator; rendered
blank where it holds nothing and inked where it holds something, the tick
and the selected radio button counted against the frames beside them;
initial values restored by a reset; a dotted name creating its ancestors
once; the `/DR` font joined or added exactly once; the auto-size that proves
the `/DA` font is read through the editor; and every `AddFieldError` leaving
the editor clean. Its header carries six reintroduced defects, each firing.

`crates/tinker-pdf/tests/form_data.rs` (17 tests) is the exchange row's
exit criterion: export then import reproduces every terminal field's value,
in both formats, on `testdata/form-fields.pdf` before and after a fill, and
on a form `add_field` builds with names three deep, a list box, a combo box,
a check box and a radio group, and values that need every escape both
formats have. Four hand-written fixtures in `tests/form_data/` — not written
by this writer, and their README says what was and was not available to
write them from — are read value by value, and imported into the form they
were written for. The refusals, the unread keys and a name ten thousand
partial names deep are asserted; `hostile_input.rs`'s
`mutated_form_data_never_panics` sweeps the fixtures on every commit, and
`form_data` is a fuzz target whose body is a round-trip property rather than
a crash hunt. That property, run once as a stable-toolchain sweep of 360 000
mutations before the target was committed, found two writer defects — a
valueless field beneath which other fields sat was lost, and `.x` came back
as `x` — and both are pinned. Seven reintroduced defects each fire. The
review of the row found what the sweep could not reach: the copy budget is
fired on every shape it named and the ones beside them (one field's name in
four thousand warnings, one indirect `/T` down 127 inline and 256 indirect
levels, one `/V` in five thousand fields, four thousand kids under one long
name, the XFDF equivalents) and swept from small to past it by
`hostile_input.rs`'s `form_data_hands_back_no_more_than_its_budget`; a
self-naming `/Kids` array returns; a nameless entry is neither read nor
imported; and a unit test beside the writer counts that writing names back
into a tree compares each partial name with one sibling at most, where a
scan of every sibling took seven seconds over forty thousand flat names.

`crates/tinker-pdf/tests/shaped_forms.rs` (19 tests, and the same 19 in a
`--no-default-features` build — the registry pair swap places) holds up the
shaped half against a face the file synthesises, so the expected glyph
indices are ones the test names rather than reads back out of the engine.
It asserts joined Arabic in visual order under `/Identity-H`, under an
embedded CMap stream, under a one-byte codespace, under a registry CMap —
where the codes are checked *forwards* through `CMap::cid`, which is what
makes it a round trip rather than a restatement — and over a non-identity
`/CIDToGIDMap`; a vertical CMap written as a column at the box's centre,
with `/Q` read down it; a bare CFF drawn through its wrapper at `/W`'s
advances, naming a character its `/ToUnicode` never mentions and one whose
CID the program lacks, and refused with no `/ToUnicode`; a simple TrueType
font carrying a `GPOS` placement as `TJ` numbers and `Ts`, and keeping the
byte path for a line whose ligature no byte reaches; and it asserts each
remaining refusal by name: a program that is no font, the CID whose code a
`cidchar` took, and the registry CMap whose table a `cmap-predefined`-off
build left out. Its module header carries milestone 8's nine reintroduced
defects and how many assertions each one fired.

`crates/tinker-pdf-cos/tests/form_script_budget.rs` (4 tests) builds a
document that crowds all three surfaces and asserts the four mebibytes are
spent once between them, that the eight name-tree entries and five catalog
triggers past the line come back named, and that each bare entry point is
still one read of its own. Its header records what the catalog cannot hold
and why.

`crates/tinker-pdf-cos/tests/form_document_scripts.rs` (13 tests) is the
milestone that made real calculating forms computable: one test asserts both
that a form computes through its own helpers under the trigger **and** that
it fails with `UnknownName` without it, because "it works now" and "it did
not work before" are two claims and only the pair is evidence. Recursion
direct and mutual, a helper that outruns the step budget, a statement at
document scope, a field script that tries to declare a function, a helper
that tries to take a builtin's name, and the catalog trigger changing no
answer anywhere are each asserted by name. Its header carries the same six
flips: `document` fires 3 of 13 here, where `form_calculations.rs` reports a
zero.

`crates/tinker-pdf-cos/tests/form_events.rs` (13 tests) covers the two
actions that need an event: a keystroke accepted, refused and rewritten; the
selection and commit flag a host states and a script may not move; the
default policy refusing both by name; a field with no action accepting under
every policy; an event script refused a write; a validation that aborts a
whole pass and one that is named as never having run. Its header explains why
the keystroke and validate flips are 1 and 2 of 13 rather than more — most of
the file passes an explicit policy, which a change to the default cannot
reach.

`crates/tinker-pdf-cos/tests/display_string_does_not_reach_v.rs` (2 tests)
compiles the mistake rather than describing it: `rustc` builds a caller
against the real rlib this test run produced, once with `.text()` — which
must compile, or nothing below it means anything — and once per write door
with the `DisplayString` handed straight over, each asserted to fail with
`error[E0308]` naming the type. It is one of the rows in `xtask`'s
`SPAWNERS`, marked `PERMANENT`: it asks this repository's own compiler
whether this repository's own type refuses a state, and adjudicates nothing
about a document (ruling 13).

`form_script` is one of the 50 fuzz targets, with a committed seed corpus.
It drives every entry point the policy gates from one input, split at the
first NUL byte: the prefix is offered to `ScriptScope::define` as document
scope, and the suffix is run as a field script with whatever that produced
in scope, then twice more as each event — because a keystroke action reads
members no calculate action ever touches and a bound that holds for `run`
and not for a helper's body is not a bound. A seed with no NUL defines
nothing and runs whole, which is what every committed seed already did. Two
properties are asserted on each: no panic (ruling 1), and that a run never
spends more budget than it was given, which is how a bounded interpreter is
kept bounded rather than believed bounded.

All of it rides in the workspace suite: 4 879 passed, 0 failed, 58 ignored across 218 suites
(`x86_64-pc-windows-msvc`, 14 September 2026) —
[verification](../verification.md).
