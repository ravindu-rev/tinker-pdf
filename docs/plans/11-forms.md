# Phase 11 — Forms

When this phase is done, the engine reads any AcroForm into a typed field model —
tree, types, flags, values, defaults, appearance instructions — fills it with
correct value normalization and regenerated appearance streams, resets it, and
enumerates it precisely enough that Tinker's FDF/XFDF/JSON interchange needs
nothing from the engine but this model. Signature fields are readable down to
`/ByteRange`; calculation is a capability-flagged open item stated plainly below;
XFA is permanently out. The phase is shaped by one asymmetry: reading and filling
AcroForms is well-specified, mechanical work over the COS graph this engine
already has, while everything adjacent — JavaScript, XFA, interchange formats,
signature cryptography — is either someone else's phase, Tinker's code, or an
explicitly open decision. The fences are the design.

## Scope

- `/AcroForm` dictionary (12.7.2, Table 218): `/Fields`, `/NeedAppearances`,
  `/SigFlags`, `/CO` calculation order, `/DR` default resources, `/DA`, `/Q`.
- Field tree (12.7.3.1): `/Parent`/`/Kids` walk with cycle guards, inheritance of
  `/FT`, `/Ff`, `/V`, `/DV` down the parent chain, fully qualified names per
  12.7.3.2 (`parent.child.field`), and merged field+widget dictionaries — the
  single-widget field whose annotation entries live in the field dict itself,
  which is the common case in the wild, versus `/Kids` that are widgets (no `/T`)
  versus `/Kids` that are child fields (`/T` present).
- Field types (12.7.4): `Btn` push/checkbox/radio with flags from Table 226
  (NoToggleToOff bit 15, Radio 16, Pushbutton 17, RadiosInUnison 26) and `/Opt`
  export values (Table 227); `Tx` with flags from Table 228 (Multiline 13,
  Password 14, FileSelect 21, DoNotScroll 24, Comb 25) and `/MaxLen`; `Ch`
  combo/list with flags from Table 230 (Combo 18, Edit 19, Sort 20, MultiSelect
  22, CommitOnSelChange 27), `/Opt`, `/TI`, `/I`; `Sig` as read-only presence.
- Common flags (Table 221): ReadOnly, Required, NoExport — reported and, for
  ReadOnly, enforced by the fill API.
- Values and defaults: `/V`/`/DV` with per-type decoding, `/AS` appearance states
  on widgets (12.5.5), same-name field semantics (fields sharing a fully
  qualified name share a value).
- `/DA` parsing (12.7.3.3, Table 222): the default-appearance fragment tokenized
  with the phase-06 tokenizer — `Tf` font and size, color operators — with font
  resolution against `/DR` then the widget page's resources.
- Fill: `set_value` with format normalization per type, checkbox/radio toggle by
  `/AS` export state, choice selection with `/Opt` membership and `/I` sync,
  reset-form semantics (12.7.5.3: `/DV` → `/V`, clear when absent).
- Appearance regeneration per field type through the appearance synthesizer of
  [10-editing](10-editing.md): variable-text layout (`/Q` quadding, multiline
  wrap, comb cells, password bullets, auto-size `Tf 0`), plus NeedAppearances
  repair mode — a document arriving with the flag set gets every field's
  appearance regenerated so it displays in viewers that ignore the flag.
- `/CO` read and reported; `/AA` calculate/format/keystroke/validate scripts
  surfaced as raw data so the application can warn (never executed here).
- Signature fields: structure read — `/V` signature dictionary (12.8.1,
  Table 252: `/ByteRange`, `/Contents`, `/Filter`, `/SubFilter`, `/M`, `/Name`,
  `/Reason`, `/Location`) — and `/ByteRange` span access for external
  verification.
- XFA detection: `/XFA` presence classified as Hybrid (non-empty `/Fields`) or
  Dynamic, reported as data. Detection only; see Non-goals.
- Field enumeration API sufficient for Tinker's FDF/XFDF/JSON interchange:
  document-order iteration, fully qualified names, export values, options,
  values with type.

## Non-goals

- **JavaScript execution.** The AF\* helper family (AFSimple_Calculate,
  AFNumber/AFDate/AFPercent format) needs an ECMAScript subset, and that is the
  open item below — until it is decided, no calculation, no keystroke/format
  events, `formCalc = false`. Keystroke and format events are out of scope for
  this phase regardless of how the calc decision lands.
- **XFA, permanently.** Tinker's ruling (its `plans/09`, reaffirmed in
  `mupdf-limitations.md`) stands unchanged: no XFA processor, ever. The engine
  reports `/XFA` presence and classification so Tinker can show its banner and
  offer its hybrid-strip workaround; the packet bytes are never interpreted.
- **Interchange formats.** FDF, XFDF and JSON codecs stay in Tinker on
  lopdf/quick-xml as its `plans/09` already builds them — the engine exposes the
  field model only. Duplicating format code here would create two owners for one
  behavior.
- **Signature verification and signing** — Tinker's pure-Rust sign modules (its
  `plans/10`). This phase hands over structure and `/ByteRange` spans; the
  cryptography, trust chains, and incremental signing flow live there.
- **Field creation, editing, deletion** — Tinker's form designer performs
  object-level construction through the engine's COS mutation surface
  ([01-cos-and-object-model](01-cos-and-object-model.md)); a first-class designer
  API, if it ever exists, is an extension of [12-creation](12-creation.md).
- **Flattening** — baking widget appearances into page content is an editing
  operation over the same appearance streams and belongs to
  [10-editing](10-editing.md)'s bake pipeline.
- **Form UI concerns** — tab order, date pickers, field overlays, permission
  enforcement policy. The engine exposes the fill-forms permission bit via
  [03-encryption](03-encryption.md); whether to honor it is application policy,
  and Tinker enforces it.

## Design

### Where the code lives

In the `tinker-pdf` facade crate as the `form` module, beside `doc` — the same
reasoning as [04-document-semantics](04-document-semantics.md): `Form`, `Field`
and `FieldValue` *are* the public surface (ruling 11), their only consumers are
Tinker and the bindings, and a `tinker-pdf-form` crate would be a boundary with
cost and no benefit. Fill mutates COS objects in place; appearance regeneration
calls into the synthesizer [10-editing](10-editing.md) builds for annotations;
saving — incremental or full — is [09-writing](09-writing.md)'s job and none of
this module's business.

### The field model

The tree walk mirrors phase 04's discipline: iterative, visited-set guarded on
both `/Kids` and `/Parent` (a `/Parent` cycle is as cheap to author as a `/Kids`
one), lenient about `/Type`. Classification of a `/Kids` entry is by `/T`
presence — a kid with `/T` is a child field, without it a widget of its parent —
because that is the spec's own rule (12.7.3.2) and the only one real files
follow. The merged single-widget dictionary is detected by `/Subtype /Widget` on
the field dict itself and modeled as a field with one widget, not as two
objects, so the API never shows the artifact of the file layout.

```rust
impl Document {
    /// None when the catalog has no /AcroForm or its /Fields is empty —
    /// an ordinary answer, not an error.
    pub fn form(&self) -> Option<Form<'_>>;
}

pub struct Field {
    pub name: String,             // fully qualified, 12.7.3.2
    pub partial: String,          // this node's /T
    pub alt_name: Option<String>, // /TU, for the UI
    pub kind: FieldKind,
    pub flags: FieldFlags,        // ReadOnly | Required | NoExport
    pub value: Option<FieldValue>,
    pub default: Option<FieldValue>,
    pub widgets: Vec<Widget>,     // page index, rect, /AS, /MK presence
    pub scripts: FieldScripts,    // raw /AA calculate/format/… strings; data only
}

pub enum FieldKind {
    PushButton,
    CheckBox { on_state: Name, export: Option<String> },
    RadioGroup { no_toggle_to_off: bool, in_unison: bool,
                 states: Vec<RadioState> },       // per-kid on-state + export
    Text { multiline: bool, password: bool, file_select: bool,
           comb: bool, max_len: Option<u32> },
    Choice { combo: bool, editable: bool, multi_select: bool, sorted: bool,
             options: Vec<ChoiceOption> },        // export + display per /Opt
    Signature(Option<SignatureInfo>),
}

pub enum FieldValue {
    Text(String),
    State(Name),           // checkbox/radio: an on-state name, or "Off"
    Choices(Vec<String>),  // export values; len 1 unless MultiSelect
}
```

Checkbox and radio on-states are discovered, not assumed: the on-state is
whatever key the widget's `/AP` `/N` dictionary carries besides the reserved
`Off` (12.7.4.2.3). Files that use `/1`, `/Yes`, `/Ja` or an export value from
`/Opt` all work because the model records what is there. `RadioState` pairs each
kid widget's on-state name with its `/Opt` export value when present — the two
diverge in real files, and interchange needs the export value while `/AS`
toggling needs the state name, so both are kept.

Fully-qualified-name collisions across separate tree entries — a spec violation
generators commit constantly — are resolved the way the spec resolves legitimate
same-name fields: they are one logical field. Enumeration yields one `Field`
with all the widgets; setting its value updates every underlying dict, with a
provenance warning (ruling 10) recording that the file was structurally dubious.

Orphan widgets — `/Subtype /Widget` annotations reachable from a page `/Annots`
but from no `/AcroForm` `/Fields` path — are adopted into the model with a
warning, because the wild is full of them and a fill tool that cannot see a
visible field is broken in the way that matters. Fields reachable from
`/Fields` with no widget anywhere still enumerate: invisible fields are legal
and interchange must round-trip them.

### `/DA` and font resolution

`/DA` is a degenerate content fragment — operators with no `BT`/`ET` — so it is
tokenized by the phase-06 tokenizer rather than a second parser. Extracted:
`Tf` (font resource name + size) and the color operators (`g`, `rg`, `k`).
Resolution order for the named font: the field's own `/DA`, else the inherited
one, else `/AcroForm` `/DA`; the name resolves against `/DR` `/Font` first,
then the widget page's resources — `/DR` is the spec's home for it, the page is
where broken generators put it. A `/DA` that is missing, empty, or names an
unresolvable font falls back to Helvetica from the phase-05 base-14 set at size
0 (auto) with a warning — matching what Acrobat synthesizes, because a field
with a broken `/DA` still has to display its value. Glyph coverage is checked
via [05-fonts](05-fonts.md): a value containing glyphs the DA font lacks
triggers fallback substitution in the synthesized appearance plus a warning,
never a tofu-only render presented as success.

### Fill semantics per type

```rust
impl Form<'_> {
    pub fn set_value(&mut self, field: &FieldRef, value: FieldValue)
        -> Result<SetOutcome, FormError>;
    pub fn toggle(&mut self, field: &FieldRef, state: &Name)
        -> Result<SetOutcome, FormError>;              // Btn convenience
    pub fn reset(&mut self, fields: Option<&[FieldRef]>)
        -> Result<(), FormError>;                      // 12.7.5.3 semantics
    pub fn regenerate_appearances(&mut self) -> Result<u32, FormError>;
}
```

- **Text.** `/V` is written as a PDF text string — PDFDocEncoding when the value
  fits it, UTF-16BE otherwise, encrypted on the way out by the writer per the
  document's crypto. Multiline line endings normalize to CR, which is what
  Acrobat writes and reads. A value exceeding `/MaxLen` is a typed
  `FormError::TooLong`, not a silent truncation — truncation loses user data,
  and the caller (which is a UI with the length in hand) is the right place to
  shorten. Password fields store plaintext in `/V` like every other conforming
  implementation; the bullets are an appearance concern.
- **Checkbox.** `set_value(State)` requires the name to be the widget's on-state
  or `Off`; `/V` and `/AS` are set together, because a file where they disagree
  displays one thing and exports another.
- **Radio.** `/V` on the group is set to the chosen state; each kid's `/AS`
  becomes that name if its `/AP` has the state, `Off` otherwise. RadiosInUnison
  means several kids may light at once — the per-kid check handles it with no
  special case. `NoToggleToOff` is enforced: turning the group off when the
  flag is set is `FormError::NotToggleable`.
- **Choice.** Values must be `/Opt` export values unless the Edit flag allows
  free text (editable combo). MultiSelect writes `/V` as an array and rebuilds
  `/I` in `/Opt` order — `/I` is what disambiguates duplicate export values
  (12.7.4.4, note on `/I`), so it is never left stale. Single-select writes a
  plain string and removes `/I`.
- **Push buttons** hold no value; setting one is `FormError::NoValue`.
- **Signature fields are read-only** in this phase: `FormError::ReadOnlyKind`.
- **Reset** copies `/DV` to `/V` (removing `/V` where `/DV` is absent), clears
  `/I`, re-derives `/AS` from the restored value, and regenerates appearances
  for the affected fields — the engine-side semantics of the ResetForm action
  (12.7.5.3) without modeling the action itself.

ReadOnly-flagged fields refuse `set_value` with `FormError::ReadOnly`; the
fill-forms permission bit is *reported*, not enforced — enforcement is Tinker's
policy layer, consistent with how phase 03 treats every permission.

### Appearances and NeedAppearances

Every successful `set_value` regenerates the affected widgets' `/N` appearance
streams through the synthesizer from [10-editing](10-editing.md): variable-text
layout per 12.7.3.3 — `/Q` quadding, multiline wrapping against the widget
rect, comb cell placement when Comb is set (`/MaxLen` cells, one glyph each),
bullets for password fields, and the auto-size loop for `Tf 0` (largest size
that fits the rect, floored at 4pt, matching observed Acrobat behavior rather
than any published algorithm — the perceptual gate below is the arbiter, not
pixel identity). Checkbox and radio toggling regenerates nothing when the `/AP`
states already exist — flipping `/AS` is the whole job — and synthesizes the
standard check/dot from `/MK` only when a state is missing.

`/NeedAppearances` gets both directions. After the engine generates appearances
it clears the flag, so viewers that ignore it (most of them) show the values.
`regenerate_appearances()` is the repair mode for documents that arrive with
the flag set: every field's appearance is rebuilt from `/V` and `/DA`, the flag
is cleared, and the count of regenerated widgets is returned so the caller can
report what happened.

### Calculation — the open item, stated plainly

Real-world order forms depend on the AF\* helper family, and AF\* is JavaScript:
supporting it requires an ECMAScript subset interpreter. The options are known
and neither is chosen here. Building our own minimal interpreter fits the
hand-rolled rule and is honestly large — an ES subset with the `util`/`AFNumber`
surface is months, not weeks. Adopting `boa` conflicts with the rule that no
third-party crate implements PDF logic or primitives. **The owner decides when
this phase starts**; the decision is recorded as an ADR either way. Until then:
`formCalc = false` as an engine capability flag, `set_value` updates exactly the
field it names, there is no recalculation — manual or automatic — and
keystroke/format events are out of scope regardless. What the engine does do is
read `/CO` and surface the `/AA` script sources, so Tinker can tell the user
"this form computes totals and we did not", which is the honest floor.

### Signature fields

`SignatureInfo` exposes the `/V` dictionary read-only: `/ByteRange` as
`Vec<(u64, u64)>` offset/length spans validated against the file length,
`/Contents` raw bytes, `/Filter`, `/SubFilter`, `/M`, `/Name`, `/Reason`,
`/Location`. That is exactly the surface Tinker's verification code needs and
nothing more. One rule crosses phases and is stated here because forms is where
it bites: **filling a document that has a populated signature field must be
saved incrementally** — a full rewrite moves bytes and voids every signature.
The fill API flags the condition (`SetOutcome::requires_incremental`), and
[09-writing](09-writing.md)'s incremental writer is the only legal save path
for it.

### Error and leniency policy

The phase-04 ladder applies unchanged: absent form → `None`, ordinary; broken
structure → best-effort model plus typed provenance warnings (ruling 10) —
orphan widgets adopted, name collisions unified, `/V` that mismatches `/FT`
coerced where a reading exists (a string in a checkbox becomes a state name
attempt) and reported as unreadable where none does; hostile structure (cycles,
bomb `/Kids`) → bounded truncation plus a warning. Fill errors are the one
place this phase is *stricter* than read: writing a bad value into a file is
corruption we would author ourselves, so `set_value` validates and refuses
(`TooLong`, `NotAnOption`, `ReadOnly`) rather than degrading. Nothing panics
(ruling 1); `fuzz_form_tree` and `fuzz_form_fill` enforce it.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Field model and enumeration: tree walk, inheritance, merged widgets, all field kinds with flags, values/defaults, `/DA` parse, `/CO` and `/AA` surfacing, XFA classification | Field-tree JSON snapshots (name, kind, flags, value, options, export values) match Acrobat-reported structure on the forms fixture set — basic-all-types, radio-kids, hierarchy, comb-maxlen, listbox-multiselect, readonly-required, xfa-hybrid, xfa-dynamic. Every AcroForm file in the corpus enumerates without panic; `fuzz_form_tree` runs cyclic `/Parent`/`/Kids` and orphan-widget inputs clean. `/DA` parse yields warnings, never errors, corpus-wide. | S |
| 2 | Fill and reset: `set_value`/`toggle`/`reset` for all types, normalization, `/AS` state logic, `/I` sync, same-name propagation, validation errors | Fill → incremental save → reopen reads back every written value on the fixture set; `qpdf --check` green on every filled output. Radio in-unison and no-toggle-to-off fixtures pin behavior; MaxLen violation and non-`/Opt` value are typed errors; same-name fixture shows one value in all widgets. `fuzz_form_fill` (arbitrary values into arbitrary fields) panic-free. | S |
| 3 | Appearance regeneration and NeedAppearances repair via [10-editing](10-editing.md) synthesis | Filled `forms-needappearances.pdf` displays values with the flag cleared. `pdfcmp` perceptual match against oracle renders (`pdftoppm`, `pdfium_test` as subprocesses, ruling 9) of the same filled files across the fill corpus, within the phase-08 perceptual budget. Unit vectors for quadding ×3, multiline wrap, comb cells, password bullets, auto-size floor, and CJK values through `/DR` fallback substitution. | M |
| 4 | Signature field read and signed-fill discipline | `/ByteRange`/`/Contents`/metadata exposed on signed fixtures; spans validated to cover the file minus the `/Contents` hole. Filling a signed fixture reports `requires_incremental`; after incremental save, the bytes inside every pre-existing `/ByteRange` span compare equal pre/post. | S |
| 5 | Validation gate and capability wiring | PDF.js driven as a CI subprocess reads back filled values on the corpus; Acrobat open-and-display is a recorded manual checklist per release (it does not run in CI, and pretending otherwise would be theater). `formCalc = false` flag exposed with `/CO` presence and script surfacing; the calc ADR exists — a decision or a dated deferral, not silence. `tpdf form` subcommand dumps the field model for debugging. | S |

## Dependencies

Needs [01-cos-and-object-model](01-cos-and-object-model.md) (object graph and
COS mutation), [04-document-semantics](04-document-semantics.md) (page tree,
`/Annots`, text-string decoding), [05-fonts](05-fonts.md) (base-14 metrics,
`/DR` font loading, glyph coverage), [06-content-and-text](06-content-and-text.md)
(the tokenizer that reads `/DA`), [09-writing](09-writing.md) (incremental
writer — non-negotiable for signed documents), and
[10-editing](10-editing.md) (appearance synthesis; this phase drives it, does
not duplicate it).

Per [PLAN.md](../PLAN.md) this phase sits after Checkpoint B and is
post-integration OK: nothing inside the engine waits on it. It unblocks the
rewrite of Tinker's `plans/09` forms features off MuPDF — fill, reset,
NeedAppearances repair, and the field model under Tinker's FDF/XFDF/JSON
codecs — and the form surface of [13-bindings](13-bindings.md). Note the parity
caveat honestly: Tinker's frozen test suite in `crates/tinker-core/tests/`
contains no forms tests (forms were a later Tinker phase), so this phase's bar
is the exit criteria above plus Tinker's `plans/09` requirements, not a port of
existing assertions.

## Risks

| Risk | Mitigation |
| --- | --- |
| Appearance fidelity vs Acrobat — auto-size, wrap points, comb metrics — differs enough that filled forms look wrong in other viewers | Perceptual budget via `pdfcmp` against two independent oracles, not pixel identity vs one; unit vectors pin each layout feature; where Acrobat's behavior is unpublished (auto-size), observed behavior is documented in the code with fixtures that would catch drift |
| The calc open item stalls adoption — forms with totals fill "wrong" because dependent fields never update | `formCalc = false` is loud: the flag, `/CO` presence, and script surfacing let Tinker banner the limitation per file; the ADR deadline at phase start prevents the decision from being deferred by default |
| Structural chaos in the wild: orphan widgets, name collisions, `/V`/`/AS` disagreement, `/Opt`-vs-on-state mismatches in radio kids | Adoption and unification with provenance warnings; the fixture set is built from Acrobat-authored and known-broken generator output, not just our own files; corpus enumeration ratchets like every other read path |
| Filling a signed document silently invalidates signatures | `requires_incremental` on the outcome, the incremental writer as the only legal save path, and a byte-compare test over every pre-existing `/ByteRange` span in CI |
| Hybrid XFA staleness: filling the AcroForm half leaves `/XFA` datasets stale, so Adobe shows old values | Engine reports Hybrid classification; Tinker owns the user-facing warning and its opt-in `/XFA`-strip workaround — the engine never edits what it will not parse |
| Value encoding bugs (UTF-16BE, PDFDocEncoding edge cases) corrupt every interchange path at once | One text-string encoder shared with [09-writing](09-writing.md), tested against Annex D and UTF-16 vectors; round-trip assertions run on Unicode-value fixtures including CJK |
| Scope creep toward the designer, flatten, or interchange — all adjacent, all tempting | Each is named in Non-goals with its owning home; ruling precedence in [99-consistency](99-consistency.md) outranks enthusiasm, same as every phase |

---

Rulings 1, 9, 10 and 11 in [99-consistency](99-consistency.md) bind this phase;
the master map is [PLAN.md](../PLAN.md).
