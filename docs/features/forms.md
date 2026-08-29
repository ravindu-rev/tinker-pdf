# Forms

Interactive forms, three layers deep: the AcroForm field tree read into a
typed model (12.7), fills that rewrite the widget's appearance rather than
hoping a viewer will, and the form's own `/AA` calculate actions run through
a bounded ECMAScript subset the engine wrote for the purpose. One contract
holds the layers together: **a partial application never reaches the
document**. A multi-field fill lands whole or not at all, a recalculation
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
`Signature` (recognised, not verified), and `Unknown` for an `/FT` this
build does not know. A button's on state is discovered from its `/AP` `/N`
dictionary, never assumed to be `/Yes`. Values decode per type
(`Text`/`State`/`Many`/`None` — a button with no `/V` is `Off`, which is a
state rather than an absence), and choice options come from `/Opt` in both
its string and `[export, display]` forms (12.7.4.4).

**Filling.** Setting `/V` is the easy half and the useless half: a value
with no matching appearance shows only in viewers that regenerate, which is
why filled forms so often print blank. So every fill rebuilds the widget's
`/AP` — a form XObject sized to the widget's `/Rect` at the origin, which
is what 12.5.5's mapping expects — and clears `/NeedAppearances` (12.7.2)
rather than setting it. Layout honours the `/DA` font, size and colour
(replayed verbatim, so an operator this build does not interpret still
comes out right), `/Q` quadding, multiline wrap, auto-size for `Tf 0`, and
comb fields (12.7.4.3): `/MaxLen` equal cells, one character centred in
each, overflow dropped rather than drawn outside the last box. Non-ASCII
values are written as UTF-16BE with a byte-order mark (7.9.2.2).

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

Anything else keeps the single-byte path, still draws a `?`, and emits
`WarningKind::FieldCharacterUnrepresentable { character }` against the
field's own object for every character it could not write (rulings 2
and 10) — a simple font; a **vertical** CMap, because 9.7.4.3 advances the
pen downward and this module places glyphs along a baseline, so drawing
the right glyphs in a row a viewer will stack is worse than a mark that
announces itself; a program that is not an sfnt, which is every bare CFF
(`/FontFile3 /Subtype /Type1C` or `/CIDFontType0C`), because a CFF carries
no `GSUB`/`GPOS` to execute.

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
widget's `/AS` together, all widgets or none. `reset_form` restores `/DV`
into `/V` and removes `/V` where there is no `/DV` (12.7.5.3) — "never
filled" and "filled with nothing" are different states.

**Transactions.** `DocumentEditor::transaction` snapshots the editor's
whole mutable state — overlay, deletions, page order and the object-number
counter — runs a closure, and restores everything on `Err`. A closure
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
object, `getField`, and the Acrobat helpers `AFSimple_Calculate`,
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
(64 KiB per script, 4 MiB of source per document, 16 384 tokens, 8 192-byte
strings, 1 024-entry arrays, 256 variables, 4 096 calculating fields).

**Limits, stated plainly.** Keystroke (`/K`) and validate (`/V`) actions
are surfaced as source and never run. Document-level scripts
(`/Names /JavaScript`, 7.7.4) and the catalog's `/AA` (12.6.3 Table 200)
are surfaced and never run. Nothing recalculates automatically — when a
calculation runs is host policy, so `recalculate()` is an explicit call.
Format actions (`/AA` `/F`) produce a display string through
`formatted_value` that deliberately never becomes `/V`: 12.7.3.3 keeps a
field's value and its appearance apart, and a `/V` of "GBP 1,234.00" is a
form whose export is unusable.

## API

On `Document`: `form_fields()` returns the typed `Field` model —
`name`, `kind`, `value`, `default`, `flags` (with `is_read_only()` /
`is_required()`), `widgets`, `options`, `max_len`, `default_appearance`,
and `scripts` (a `FieldScripts` of the four `/AA` sources, each a
`Script::Source` or `Script::Oversize`). `calculation_order()`,
`document_scripts()` and `catalog_scripts()` surface `/CO` and the
document's own scripts; `script_summary()` counts everything for a caller
that has to warn before filling — reading a script runs nothing.

Mutation goes through `Document::editor()`, a `DocumentEditor`:
`fill_field`, `set_field_values`, `set_field_value`, `set_checkbox`,
`select_radio`, `reset_form`, `transaction`, `recalculate`, and
`set_calculated_values` for a host that computes values itself. The format
event is `tinker_pdf_cos::calc::formatted_value`. The facade re-exports
`FillError`, `FillRejection`, `SkippedWidget`, `WidgetDefect`, `CalcError`,
`Recalculation` and `ScriptError` (ruling 11).

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
| `eval`, `function`, `try`, `switch`, `for...in`, `typeof`, `delete`, `with`, `class`, `let`/`const`, `import`/`export`, regular expressions, object literals, prototypes | `ScriptError::Syntax` — each word reserved and refused outright | a construct silently approximated is one rename away from running it | — |
| `app.*` and `console.*` | inert stubs; assigning a stub's result to a field is `ScriptError::NotStorable` | a stub's result must never become a field value | — |
| A name or member outside the subset | `ScriptError::UnknownName` / `ScriptError::UnknownMember` | a calculation that guesses is a form that lies | — |
| A script that does not terminate cheaply, or outgrows the size caps | `ScriptError::OutOfSteps` / `TooDeep` / `TooManyTokens` / `StringTooLong` / `ArrayTooLong` / `TooManyVars` | three independent bounds — depth, work, size — because none substitutes for another | — |
| Script source past 64 KiB, or past the document's 4 MiB total | `Script::Oversize(len)`; running it is `ScriptError::TooLong` | truncated source means something different from what the file says (ruling 10) | — |
| More than 4 096 calculating fields in one pass | `CalcError::TooManyFields` | refused rather than truncated, for the same reason a failing script refuses the pass | — |
| A value the field will not take — over `/MaxLen`, not an option, ReadOnly against a user write | `FillError::ValueRefused` (in a multi-field apply, `FillRejection` names the field) | refusing beats truncating, which hides a data error in a file that looks filled | — |
| A widget missing 12.5.2 Table 164's `/Rect` | `SkippedWidget` with `WidgetDefect::RectMissing` | the value is written and drawable widgets drawn; the damage is named, never silent (rulings 2, 10) | [rulings](../rulings.md) |
| Shaping a value against a simple `/DA` font, a vertical CMap, or a `/FontFile3` that is a bare CFF | `WarningKind::FieldCharacterUnrepresentable { character }` per character; the single-byte path draws `?` | a byte cannot name a glyph past 255; a vertical run drawn along a baseline is stacked by the viewer; a CFF carries no `GSUB` | [design/shaping.md](../design/shaping.md) |
| Shaping a value under a **registry CMap** in a build without `cmap-predefined` | `WarningKind::PredefinedCMapApproximate(name)` against the field, then the per-character warnings | the code-to-CID tables that would be inverted were never compiled in — a capability that depends on a feature has to say so | [fonts.md](fonts.md) |
| A CID no code means any more — a `cidchar` took the code its `cidrange` would have given | `WarningKind::FieldCharacterUnrepresentable { character }`; nothing is written for that glyph | the inverse of a CMap is not a function, and an unverified inverse draws a *different* wrong glyph | [rulings](../rulings.md) ruling 10 |
| Keystroke and validate actions; document-level and catalog scripts | surfaced (`FieldScripts`, `DocumentScript`) and never run — nothing is attempted, so nothing errors | events need an interactive host; a document-level script is arbitrary program text with no field to write | [ROADMAP](../ROADMAP.md) Tier 4 |
| A format action's display string reaching `/V` | none offered — `formatted_value` returns the string and writes nothing | 12.7.3.3 keeps value and appearance apart | [ROADMAP](../ROADMAP.md) Tier 4 |
| Automatic recalculation | none offered — `recalculate()` is explicit | when a calculation runs is a host's policy, not the engine's | — |
| Signature verification and signing | `Document::signatures()` reads the dictionary, classifies what `/ByteRange` covers and digests it; nothing **verifies** the CMS blob or the certificate chain yet | verify-only cryptography is its own capability, designed separately — the inventory is milestone 1 of it | [ROADMAP](../ROADMAP.md) Tier 3, [design](../design/signatures.md) |
| XFA | not read anywhere | removed in ISO 32000-2; a stated permanent non-goal | [ROADMAP](../ROADMAP.md) |

## Verified

Unit tests sit beside the code: 17 in `form.rs` (tree walk, inheritance,
merged widgets, qualified names, on-state discovery, cyclic trees, script
surfacing including the stream form and the oversize refusal), 18 in
`fill.rs` (the `/DA` split, quadding, multiline, auto-size shrink, comb
cells and their overflow, escaping, UTF-16BE values, `/MaxLen` and
ReadOnly refusals), and 40 in `script.rs` (arithmetic through the AF
helpers, each excluded construct refused by name, the inert `app.*`
stubs).

`crates/tinker-pdf-cos/tests/form_transactions.rs` (15 tests) holds up the
atomicity claims against hand-written fixtures: a successful fill produces
the bytes it always did, rollback restores objects, deletions, page order
and the object-number counter, abandoned edits do not grow the next saved
file, a refused field rolls back the ones before it, and a widget without
a `/Rect` is reported by object number rather than skipped in silence.
`crates/tinker-pdf-cos/tests/form_calculations.rs` (26 tests) runs an
invoice fixture end to end — `/CO` order honoured, a failing script
leaving the editor byte-identical, ReadOnly totals written by the
calculation and refused to the user, cascades cut and reported, and the
format string never landing in `/V`.

`crates/tinker-pdf/tests/shaped_forms.rs` (12 tests, and the same 12 in a
`--no-default-features` build — the registry pair swap places) holds up the
shaped half against a face the file synthesises, so the expected glyph
indices are ones the test names rather than reads back out of the engine.
It asserts joined Arabic in visual order under `/Identity-H`, under an
embedded CMap stream, under a one-byte codespace, under a registry CMap —
where the codes are checked *forwards* through `CMap::cid`, which is what
makes it a round trip rather than a restatement — and over a non-identity
`/CIDToGIDMap`; and it asserts each refusal by name: the vertical CMap, the
bare CFF, the CID whose code a `cidchar` took, and the registry CMap whose
table a `cmap-predefined`-off build left out. Its module header carries the
nine reintroduced defects and how many assertions each one fired.

`form_script` is one of the 24 fuzz targets, with a committed seed corpus:
it drives the lexer, parser and evaluator with arbitrary text against a
live `Host` and asserts two properties — no panic (ruling 1), and that a
run never spends more budget than it was given, which is how a bounded
interpreter is kept bounded rather than believed bounded. All of it rides
in the workspace suite: 2 963 passed, 0 failed, 8 ignored (Windows x86_64,
August 2026) — [verification](../verification.md).
