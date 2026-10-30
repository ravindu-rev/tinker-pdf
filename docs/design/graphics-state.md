# The external graphics state

*For [ROADMAP](../ROADMAP.md) row K-01.* `gs` (ISO 32000-1 8.4.5) sets the
parameters a graphics state parameter dictionary — an `/ExtGState` resource —
writes: the keys of Table 58 (Table 57 in ISO 32000-2) and the two that
ISO 32000-2 adds, `/UseBlackPtComp` and `/HTO`. The interpreter asks the
resource seam three questions at each `gs` — the alphas, the blend mode and
the soft mask (`crates/tinker-pdf-content/src/interpret.rs:403-429`) — and
each answer reads the dictionary again; every other key is read past, and
`ri` and `i` set nothing (`interpret.rs:1078`). This design reads the
dictionary once, into one typed value; applies each key whose mechanism the
engine has through the code its operator runs; keeps the rest in
`GraphicsState` for the rows that apply them; gives a font selected by
reference a name the name-keyed font seam resolves; and states what the kept
state costs a `q` and a recorded event, and how two recordings are held
equal.

## Scope

- **One seam call per `gs`**, `FontSource::ext_g_state`, answering every key
  of Table 58 and the two 2.0 additions in one typed value, each parameter an
  `Option` whose `None` leaves it as it was. It replaces `ext_g_state_alpha`,
  `ext_g_state_blend` and `ext_g_state_soft_mask` in every source that
  implements them: `PageResources` (`crates/tinker-pdf/src/resources.rs:1987-2118`),
  `subset.rs`'s `Recorded` (`crates/tinker-pdf/src/subset.rs:967-990`), and
  the test sources of `interpret.rs` (`:2183`) and `record.rs` (`:886`).
- **Applied:** `/LW` `/LC` `/LJ` `/ML` `/D` through the code `w` `J` `j` `M`
  `d` run, `/Font` through the code `Tf` runs, and `/BM` `/SMask` `/CA` `/ca`
  as the three seam methods apply them now.
- **Stored** in `GraphicsState` for the row that applies each: `/RI` (RC-01),
  `/UseBlackPtComp` (RC-02), `/OP` `/op` `/OPM` (RC-03), and `/FL` `/SM`
  `/SA` `/AIS` `/TK` `/TR` `/TR2` `/BG` `/BG2` `/UCR` `/UCR2` `/HT` `/HTO`
  (RC-08). `ri` and `i` set the fields `/RI` and `/FL` set.
- **A font selected by reference** (`/Font [ref size]`) becomes the text font
  for every reader of the text font: rendering, extraction, the display list,
  SVG output, subsetting on save, redaction's scan and pen, PDF/A's content
  walk, and the document's font listing.
- What the kept state costs a `q`, a form, a mask group, a Type 3 glyph and a
  recorded event, and how the display list's budget counts it.
- How two recordings are compared, and what a twin is.
- The exit tests of K-01.2 to K-01.5.

## Non-goals

- **Applying a stored key.** Rendering intents are RC-01's, black-point
  compensation RC-02's, overprint RC-03's, and black generation, undercolour
  removal, transfer, halftones and their origin, flatness, smoothness, stroke
  adjustment, alpha source and text knockout RC-08's, whose design
  (RC-08.1, `docs/design/device-dependent-state.md`) says where each enters
  the pipeline. Until its row lands, a stored key moves no pixel.
- **Keys outside Table 58 and its 2.0 additions.** `/HTP`, a PDF 1.2 key
  PDF 1.3 dropped (the PDF Association's Arlington model records it so), and a
  producer's prefixed keys are read past.
- **ISO 32000-2's deprecations.** The Arlington model records `/TR`, `/TR2`
  and `/BM`'s array form as deprecated in 2.0. They are read as ISO 32000-1
  reads them; a warning for them is SD-18.6's.
- **Writing a graphics state.** The builder's `ExtGState`
  (`crates/tinker-pdf-cos/src/build.rs:670`) is unchanged; SD-18.2 writes
  `/UseBlackPtComp` and `/HTO`.
- **Keeping a reading per name.** A `gs` reads its dictionary once; keeping
  that reading for the next `gs` of the same name is P-02.5's, which the seam's
  shape is made for (below).
- **The text font across a form's own resources.** The interpreter holds the
  text font by name, and a form whose `/Resources` gives the name to another
  font draws in that one (the form runs under the inherited state in its own
  scope, `interpret.rs:1606-1620`). A `gs` whose font the scope already names
  takes that name and shares `Tf`'s behaviour there; changing it is not this
  row's.
- **PDF/A's and PDF/X's rules on these keys.** `crates/tinker-pdf/src/pdfa/colour.rs`
  reads `/TR`, `/TR2` and the rest for its own clauses, and is unchanged.

## Design

### The keys, applied and stored

The values are Table 58's. ISO 32000-2's text is not in hand
([pdf20-deltas.md](../pdf20-deltas.md)), so the two 2.0 keys are the
Arlington model's (`tsv/latest/GraphicsStateParameter.tsv`, at the revision
`pdf20-deltas.md` cites): `/UseBlackPtComp` is one of the names `/ON`, `/OFF`
and `/Default`, `/Default` when absent, and `/HTO` an array of two numbers.

| Key | Value | Applied or stored | Sets | Operator twin | Row |
| --- | --- | --- | --- | --- | --- |
| `/Type` | name | read past | — | — | — |
| `/LW` | number | applied | `line_width` | `w` | K-01.2 |
| `/LC` | integer | applied | `line_cap` | `J` | K-01.2 |
| `/LJ` | integer | applied | `line_join` | `j` | K-01.2 |
| `/ML` | number | applied | `miter_limit` | `M` | K-01.2 |
| `/D` | `[dashArray dashPhase]` | applied | `dashes`, `dash_phase` | `d` | K-01.2 |
| `/Font` | `[font size]`, `font` an indirect reference | applied | `text.font`, `text.size` | `Tf` | K-01.5 |
| `/BM` | name or array | applied | `blend` | — | as now |
| `/SMask` | dictionary or `/None` | applied | the device's mask, through `begin_soft_mask` or `clear_soft_mask` | — | as now |
| `/CA`, `/ca` | number | applied | `stroke_alpha`, `fill_alpha` | — | as now |
| `/RI` | name | stored | `rendering_intent` | `ri` | RC-01 |
| `/UseBlackPtComp` | `/ON`, `/OFF`, `/Default` | stored | `black_point_compensation` | — | RC-02 |
| `/OP` | boolean | stored | `stroke_overprint`, and `fill_overprint` where `/op` is absent | — | RC-03 |
| `/op` | boolean | stored | `fill_overprint` | — | RC-03 |
| `/OPM` | integer | stored | `overprint_mode` | — | RC-03 |
| `/FL` | number | stored | `flatness` | `i` | RC-08 |
| `/SM` | number | stored | `smoothness` | — | RC-08 |
| `/SA` | boolean | stored | `stroke_adjustment` | — | RC-08 |
| `/AIS` | boolean | stored | `alpha_is_shape` | — | RC-08 |
| `/TK` | boolean | stored | `text_knockout` | — | RC-08 |
| `/BG`, `/BG2` | function; `/BG2` also `/Default` | stored | `black_generation` | — | RC-08 |
| `/UCR`, `/UCR2` | function; `/UCR2` also `/Default` | stored | `undercolor_removal` | — | RC-08 |
| `/TR`, `/TR2` | function, four functions, `/Identity`; `/TR2` also `/Default` | stored | `transfer` | — | RC-08 |
| `/HT` | halftone dictionary or stream, or `/Default` | stored | `halftone` | — | RC-08 |
| `/HTO` | two numbers | stored | `halftone_origin` | — | RC-08 |

Applied means the engine already has the mechanism: the line parameters, the
text font, the alphas, the blend mode and the soft mask each have a field or
a device call that every consumer reads. Stored means no consumer reads the
parameter yet, so the state keeps what the dictionary set until the row that
applies it reads it there — which is also what lets RC-17.3 report overprint
from what a page draws, and SD-18.2 read back what it writes.

### One seam call, one reading

```rust
/// What one graphics state parameter dictionary sets (8.4.5): one field per
/// parameter, `None` where the dictionary leaves it as it was.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateParameters { /* one Option per row of the table above */ }

pub trait FontSource {
    /// A named `/ExtGState`, read once.
    fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
        let _ = name;
        None
    }
}
```

`StateParameters` lives in `interpret.rs` beside `SoftMask` and `MaskGroup`,
the other values the seam hands over, and is named for what 8.4.5 calls the
dictionary rather than `ExtGState`, which the facade already re-exports for
the builder's type. Its fields are **one per parameter, not one per key**, so
Table 58's rules between keys are applied once, where the dictionary is read,
and the interpreter applies `Option`s:

- **`/op` absent takes `/OP`'s value.** `stroke_overprint` is `/OP`;
  `fill_overprint` is `/op`, or `/OP` where `/op` is absent. A dictionary
  that writes only `/OP true` and one that writes `/OP true /op true` are the
  same value, which is the trap K-01.2's note names.
- **A second spelling wins.** `/BG2` over `/BG`, `/UCR2` over `/UCR`, `/TR2`
  over `/TR`, when one dictionary writes both.
- **`/Default` is a value, not an absence.** It resets the parameter to the
  device's own (below); an absent key leaves the parameter alone.

`PageResources::ext_g_state` reads the scope's `/ExtGState` and the named
entry **where they lie**, through `read_resolved` and `read_key`
(`resources.rs:635-665`), and walks the entry's keys once, matching each by
its bytes and reading its value where it lies; a key it does not know is read
past. The three methods it replaces each resolve both the table and the
entry with `resolve_key`, which hands back a copy of a direct dictionary
(`crates/tinker-pdf-cos/src/doc.rs:1233`), so a page whose `/ExtGState` is
written inline in its resources copies it three times at every `gs`. The one
reading copies nothing it does not keep, and costs the keys the dictionary
writes rather than a lookup for each of the twenty-eight it could.

The answer is an `Arc` so that P-02.5, which keeps each name's reading in the
scope's caches, hands every `gs` of a name the same value without copying a
soft mask's group with it. `subset.rs`'s `Recorded` forwards the call by
hand, as it forwards every method, and queues a soft mask's group for its walk
from the answer's `soft_mask`, as `ext_g_state_soft_mask` does now.

### Applied keys run their operators' code

Each applied parameter has one setter in the interpreter, called by its
operator and by `gs` alike, so a twin cannot drift: line width (finite and
not negative, else unchanged), cap and join (`LineCap::from_operand`,
`LineJoin::from_operand`, so `/LC 7` is a butt cap as `7 J` is), miter limit
(finite and at least 1, else unchanged), the dash (the check `set_dashes`
makes, `interpret.rs:745`, with the operand-stack walk split from it: a
negative or non-finite element or phase leaves the dash as it was, all zeros
is a solid line), and the text font (a name and a finite size, as `Tf`'s arm
at `interpret.rs:819` takes them). The facade hands the setters the operands
as the dictionary writes them — `/LW`'s number, `/D`'s numbers and phase —
and a value the operator could not have been handed (a string where a number
belongs) makes the key absent, which leaves the parameter as the operator
leaves it on an operand it cannot read. `/D` holds the bound `d` meets: an
array longer than `d` could be handed on the operand stack, which keeps 512
tokens (`interpret.rs:695`), is absent, as such a `d` is ignored, so a
dictionary cannot put an array into every state copy that no content stream
could.

**Order inside one `gs`.** Every parameter the dictionary sets is applied,
then the soft mask runs, at the CTM in force (`run_soft_mask`,
`interpret.rs:1738`). The alphas and the blend mode precede the mask in the
arm that runs now (`interpret.rs:1233-1254`), and every other parameter joins
them, so the mask group runs under the state the `gs` made, with the alphas
and the blend mode reset inside it as they are now. `/CA`, `/ca`, `/BM` and
`/SMask` are read and applied exactly as the three methods read and apply
them, and K-01.2 pins that before it replaces them.

### Stored parameters, one shared value in the state

```rust
pub struct GraphicsState {
    // ... every field it has now ...
    /// The parameters kept for the rows that apply them; `None` while every
    /// one has its initial value.
    pub stored: Option<Arc<StoredParameters>>,
}
```

`StoredParameters` lives in `state.rs`, and `GraphicsState::stored_parameters()`
answers a reference to it, or to a `static` of the initial values when the
field is `None`, so a reader never matches on the option. Its fields:

| Field | Type | Initial | Set by |
| --- | --- | --- | --- |
| `rendering_intent` | `RenderingIntent`: the four of 8.6.5.8, and `Unrecognised` | 8.4.1's | `/RI`, `ri` |
| `black_point_compensation` | `On`, `Off`, `Default` | `Default` | `/UseBlackPtComp` |
| `stroke_overprint`, `fill_overprint` | `bool` | 8.4.1's | `/OP`, `/op` |
| `overprint_mode` | the integer written | 8.4.1's | `/OPM` |
| `alpha_is_shape` | `bool` | 8.4.1's | `/AIS` |
| `text_knockout` | `bool` | 9.3.8's | `/TK` |
| `flatness` | `Option<f64>`, the number written | `None` | `/FL`, `i` |
| `smoothness` | `Option<f64>`, the number written | `None` | `/SM` |
| `stroke_adjustment` | `Option<bool>` | `None` | `/SA` |
| `black_generation` | `Option<Arc<Function>>` | `None` | `/BG2` over `/BG` |
| `undercolor_removal` | `Option<Arc<Function>>` | `None` | `/UCR2` over `/UCR` |
| `transfer` | `Option<Transfer>` | `None` | `/TR2` over `/TR` |
| `halftone` | `Option<Arc<Halftone>>` | `None` | `/HT` |
| `halftone_origin` | `Option<[f64; 2]>` | `None` | `/HTO` |

A plain field starts at the value 8.4.1's tables (Tables 52 and 53) or 9.3.8
give, which K-01.2 transcribes from the text and `state.rs`'s tests pin. An
`Option` field's `None` is **the device's own**: what the renderer does with
no key — its fixed flattening tolerance (`Renderer::tolerance`,
`crates/tinker-pdf-render/src/lib.rs:813-814`), maximum undercolour removal
(`rgb_to_cmyk`, `crates/tinker-pdf-raster/src/canvas.rs:1042`), no transfer
function, and the screen RC-06 gives its 1-bit output — and what `/Default`
restores where a key admits it. That is what lets each of RC-08's steps say
that a page without its key moves no byte: a plain field holding the
standard's initial value could not tell a page that never set the parameter
from one that set it to that value.

`ri` stores the name it is handed as one of the four intents, and any other
name as `Unrecognised`, so RC-01 applies 8.6.5.8's reading of a name it does
not know, and can say so, rather than finding the name already replaced. A
stored number is kept as written when it is finite; its range is the
applying row's to read from the text. `ri` and `i` reach the same setters as
`/RI` and `/FL`.

**A set** copies the stored parameters, changes the fields the dictionary or
the operator writes, and keeps the old value when nothing changed; a result
equal to the initial values is stored as `None`. So two states that hold the
same parameters compare equal however they were reached, and a `gs` repeated
before every text object allocates nothing after the first.

### Functions and halftones: typed values, shared

Of the three representations the step names — sampled tables, typed values,
or a handle the seam resolves later — a function is a **typed value**, and a
halftone is a typed value whose threshold array is **a handle**. No stored
parameter is a sampled table.

**A function is the parsed `tinker_pdf_color::Function`, behind an `Arc`.**
`parse_function` (`resources.rs:2671`) reads it, handed the entry **as
written**, as `PageResources::function` hands it (`:2643-2669`), so a type 0
or type 4 function — a stream — reaches the reader through its reference.
`Transfer` is `Identity` (the name), `One(Arc<Function>)`, or
`Four([Arc<Function>; 4])` for the four process colorants; `/Default`
stores `None`. The scope keeps each function it reads by its object, so one
function many states share, or one state set at every `gs`, is decoded and
parsed once; a function written directly into the dictionary has no object,
is a few numbers, and is read where it lies at each `gs`. P-02.5 extends that
cache to the shading and tint readers rather than doubling it. The content
crate already depends on `tinker-pdf-color` (`docs/architecture.md`), so
`GraphicsState` names `Function` with no new edge. A sampled table was not
chosen because:

- it fixes the precision at the `gs`, where the applying row is the one that
  knows the stage and the depth (an ink device's components; RC-07's 16-bit
  samples) — RC-08.2 samples with `sample_transfer` (`resources.rs:4880`)
  where it applies a function;
- `/UCR` maps into −1 to 1, which a `[u8; 256]` cannot hold without an
  encoding of its own;
- sampling runs every function 256 times at every `gs` of every page that
  renders now, for a key no render applies.

A handle was not chosen for a function because a function is small and read
once per object, and reading it at the `gs` is what lets a malformed one be
absent there, by the rule every key follows (below), and lets K-01.3's test
evaluate the recorded value itself.

**A halftone is a `Halftone`, behind an `Arc`, with one variant per type.**
Type 1 carries its frequency, angle, accurate-screens flag, spot function (a
function read as above, or one of the twenty-one predefined names the
Arlington model lists for it, kept as its name; the array form the model also
admits is K-01.4's to read from the text) and transfer function; type 5
its entries by colorant, `/Default` among them; types 6, 10 and 16 their
dimensions as written (`/Width` and `/Height`; `/Xsquare` and `/Ysquare`;
type 16's second rectangle) and their transfer functions. **A threshold
array is kept by its stream**: the stream's reference packed as
`Form::stream` packs one (`num << 16 | gen`, a plain integer for the reason
`interpret.rs:43-60` gives), which RC-08.9 decodes where it screens. The
array is the one value whose size is the file's — width by height samples, up
to `MAX_DECODED_STREAM` (`crates/tinker-pdf-cos/src/limits.rs:103`) — and
nothing reads it until a 1-bit render screens with it, so decoding it at the
`gs` would make every light render of a halftoned page pay for it. A
dictionary whose shape is wrong — a missing required key,
a type 5 with no `/Default`, an entry of a type a type 5 may not hold — is
absent at the `gs`; a threshold stream that decodes short is found where it is
decoded, which RC-08.9 screens as its design says.

**The soft mask's own `/TR` is read the same way.** `ext_g_state_soft_mask`
resolves `/TR` before it hands it to `parse_function`
(`resources.rs:2100-2108`), so a sampled or calculator transfer function, a
stream, finds no reference and is read as `/Identity`, silently; the shading
reader reads its entry as written for exactly that reason
(`resources.rs:2661-2668`). K-01.3 reads every function entry through the one
reader, the soft mask's `/TR` included, and keeps sampling it to 256 entries,
which the mask applies per pixel. The committed suites' soft masks carry
type 2 functions only (`determinism.rs`, `render_regions.rs`,
`transparency_groups.rs`), which both readings read alike, so no fingerprint
moves.

### A font set by reference reaches the name-keyed seam

Every font question the interpreter asks is keyed by the text font's name —
`decode`, `is_vertical`, `vertical_metrics`, `font_id`, `font_name`,
`type3_glyph` — and everything downstream of the facade keys on that name or
its id: the font program, the outline cache, `selection`, `font_object`
(`resources.rs:2814-3010`). So `/Font [ref size]` is answered with **a name**,
and `gs` sets `text.font` and `text.size` exactly as `Tf` does. The seam's
font methods, the `Device` trait and every consumer of a glyph event are
unchanged.

**The name.** `fonts::state_font` (`crates/tinker-pdf/src/fonts.rs`, over any
`Resolve`, so the document and the editor read it alike) answers it:

1. **The scope's own name for the object**, when the scope's `/Font`
   subdictionary binds one to that reference — the first such entry in file
   order (`Dict` keeps it, `crates/tinker-pdf-cos/src/object.rs:236-244`).
   `PageResources` builds its scope's reverse map, reference to name, once,
   on the first `gs` that sets a font; redaction and PDF/A, which walk a
   stream once, scan the subdictionary at such a `gs`. Then `/F1 12 Tf` and
   a `gs` whose `/Font` is `[5 0 R 12]`, in a scope binding `/F1 5 0 R`, set
   the same name and size, so every later question, every glyph's `font_id`
   (the interned name's id, `resources.rs:832`) and `font_name`, and every
   recorded state are the same: the twin records what `Tf` records by
   construction.
2. **Otherwise a reserved name**: a NUL byte followed by the reference's
   object and generation numbers in decimal (`\0` `5 0`). 7.3.5 allows any
   byte in a name except NUL, so no conforming resource dictionary binds the
   name and no conforming `Tf` writes it. The reference is absolute, so the
   name means the same object in every scope, which is the K-01.5 note's
   requirement: a font a `gs` sets inside a form is the object the reference
   names, and one set on the page and inherited into a form with resources
   of its own still resolves there.

**The resolution.** `fonts::text_font` answers what a text-font name stands
for in a scope, as a font dictionary and its reference: the scope's `/Font`
entry for the name, as now; else, when the name is the reserved spelling, the
object it spells, which each reader reads as it reads a font the scope names
(`PageResources` through `cos::font::at`,
`crates/tinker-pdf-cos/src/font.rs:1131`; redaction through the editor);
else nothing, which is the missing font it is now. The scope's own binding
is asked first, so no name that resolves now resolves differently.
`PageResources` keeps a
reserved name's font beside the fonts its dictionary names, read on the first
question about it, and every path that reads a font's dictionary by its name
in the scope — `program_for` and `embedded_program` (`resources.rs:1734-1781`),
`type3_glyph` through `fonts::font_dict` (`fonts.rs:220`), `font_object`,
`resource_name` and `selection` — goes through `text_font`, so a reserved
name's font has a program, outlines, a selection and an object. P-02.3, which
makes every name's font lazy in one table per scope, takes this as its first
lazily read entry.

**The id.** A reserved name's `font_id` is its reference with the top bit set
(`1 << 63 | num << 16 | gen`), which no interned name's id (a `u32`) can be,
and which is the same in every scope, so subsetting's `font_object` reads the
object from the id itself wherever the glyph was shown. A scope's own name
keeps the interned id `Tf`'s glyphs carry.

**Every reader agrees because every reader asks the same two functions.**
Redaction's `Scan::stream` reads a Type 3 face's procedures at a `gs` that
sets one, as at a `Tf` (`crates/tinker-pdf/src/redact.rs:2908-2928`), and its
pen's `gs` arm sets the pen's font and size from `fonts::state_font` as its
`Tf` arm does from the operands (`redact.rs:4572-4625`), each looking a name
its scope's map (`fonts_in`, `redact.rs:1489`) lacks up through
`fonts::text_font`. PDF/A's walk sets its tracked font at a `gs` as at a `Tf`
(`crates/tinker-pdf/src/pdfa/content.rs:250`, `Op::font` at `:65`), and
`pdfa/fonts.rs`'s `usage` and `widths` resolve it through `text_font` rather
than `content::lookup(…, b"Font", …)` (`pdfa/fonts.rs:193-197`, `:855-858`).
`Document::fonts` lists a font a scope's graphics state names, as its contract
("what an interpreter could bind", `crates/tinker-pdf/src/lib.rs:1907`) then
requires; its `resource_names` are the `/Font` names that reach it, none for a
font only a state names. A content stream that spells the reserved name in a
`Tf` through `#00`, which the tokenizer decodes
(`crates/tinker-pdf-content/src/tokenizer.rs:157-180`), selects the object it
spells in every one of these readers alike, so no reader draws a font another
cannot see.

**What a warning calls it.** A reserved name is never shown: the facade's
missing-font list, and with it `TextWarning::UnknownFont`, and redaction's
warnings that name a font spell it as the reference, `5 0 R` (ruling 10: the
warning names the object).

### What a `q` and an event pay

`GraphicsState` gains one field, `stored`, and nothing else: the applied keys
write fields it has, and the text font is the name `Tf` writes. So:

- **A `q`** (`self.saved.push(self.gs.clone())`, `interpret.rs:781`), a form
  (`:1616`), a mask group (`:1745`) and a Type 3 glyph (`:1920`) copy one
  word more than now — `Option<Arc<_>>` is one pointer wide — and, on a page
  that sets a stored parameter, raise one reference count. No copy allocates
  anything it did not allocate before. The save stack's 64 states
  (`interpret.rs:780`) share one value.
- **A recorded event** (`RecordingDevice::snapshot`, `record.rs:642-644`)
  boxes the same state, one word wider; `Event` itself is unchanged, since the
  state is boxed, and `event_stays_small` (`record.rs:1178`) holds as it is.
  An event recorded without the state (`Capture::state` off, as subsetting's
  walk records) pays nothing.
- **A set** pays one allocation of `StoredParameters` when it changes a value,
  and none when it does not.
- **The display list's budget** (`MAX_DISPLAY_LIST_BYTES`,
  `crates/tinker-pdf-render/src/display.rs:97`) counts each state at
  `size_of::<GraphicsState>()`, one word more, and counts the stored
  parameters — with their functions and halftones — **once per change**: the
  recorder remembers the value it last counted (`DisplayRecorder::count`,
  `display.rs:408`) and counts again when an event holds another. That is at
  least once per value the recording keeps alive, so the count stays an upper
  bound of what it holds. The table of yardsticks in the cap's documentation
  takes the new state size.

### Comparing two recordings

`Event` and every type it carries derive `PartialEq`: `Glyph` and `ImageRef`
(`device.rs`), `Form`, `MaskGroup`, `SoftMask` and `StateParameters`
(`interpret.rs`), and `GraphicsState`, `TextState`, `StoredParameters` and the
types it holds (`state.rs`); the rest derive it already. Two recordings are
equal when their event lists are, compared with `==`, through a test helper in
`crates/tinker-pdf/src/display.rs` that names the first index where they part
and prints both events. Equality is by value throughout: two `Arc`s holding
equal functions are equal, and `/Default` and the initial values compare
equal because both store `None`. `f64` compares by IEEE equality, so a NaN
would make a recording unequal to itself; a twin's operands are finite, and
the interpreter refuses a non-finite operand and a non-finite matrix.

**A twin is two pages of one document sharing one indirect `/Resources`
dictionary**, differing only in their content streams. An event carries ids
that are the document's own — a glyph's `font_id` is an interned name's id, so
two documents that intern their names in different orders give one font two
ids — and one document's pages, read through one resource dictionary, give
both recordings the same ids. A recording is the facade's
(`DisplayList::content`, crate-internal), so the comparisons are unit tests in
`display.rs`, and `crates/tinker-pdf/tests/ext_gstate.rs` compares pixels,
text and subset output through the public surface. Every twin of an applied
key also renders unequal to the page without the key, so a pair that agrees
because neither applies anything fails.

The one twin that differs in its resources is a page whose `/Font` does not
name the object its `gs` selects: its glyphs carry the reserved name's id and
its states the reserved name, and its recording equals its twin's with those
two fields the only difference.

### A value that cannot be read

One rule for every key: **a value that is not of the type Table 58 gives
(the Arlington model's types for the 2.0 keys) is absent**, and the parameter
keeps its value. For an applied key this is the operator's own behaviour on
an operand it cannot read; for `/SMask` it is the behaviour now (a mask
dictionary with no `/G` changes nothing); for `/Font` it is an array whose
first element is not an indirect reference, or whose second is not a finite
number; for a function, one `parse_function` refuses; for `/HT`, a
dictionary or stream of the wrong shape. A malformed key is never a refusal of
the page (ruling 2) and never a panic (ruling 1). The interpreter skips an
operator whose operands it cannot read without a warning, and a key is read
past the same way; a stored key's warning, where reading past one changes
what a render shows, is its applying row's.

## Milestones

1. **K-01.2: the seam call, the scalar keys, `ri` and `i`.** `ext_g_state` and
   `StateParameters` replace the three methods in all four sources; `/LW`
   `/LC` `/LJ` `/ML` `/D` applied through the shared setters; `/RI` `/FL`
   `/SM` `/SA` `/OP` `/op` `/OPM` `/AIS` `/TK` `/UseBlackPtComp` stored, with
   `ri` and `i`; `PartialEq` on events; the stored value counted once per
   change. Exit: `ext_gstate.rs` `every_scalar_key_set_by_gs_draws_as_its_twin`
   (pixels, degenerate values and `Q` included, applied keys unequal to the
   page without them, `/CA` `/ca` `/BM` `/SMask` pinned first); `display.rs`'s
   `gs_twins_record_the_same_events` (the twins' recordings, the four keys'
   recordings against their pins, and each stored key's recorded value, kept
   through a later `gs` that omits it and restored by `Q`); `state.rs`'s
   `stored_parameters_cost_a_state_one_word`; `tinker-pdf-render`'s
   `display.rs` `stored_parameters_are_counted_once_per_change`;
   `soft_mask_clips.rs` and `blend_modes.rs` unchanged.
2. **K-01.3: the function keys.** `/TR` `/TR2` `/BG` `/BG2` `/UCR` `/UCR2`
   stored as typed functions, read as written and kept per scope by
   reference, and the soft mask's `/TR` through the same reader. Exit:
   `display.rs`'s `every_function_key_is_stored`; `ext_gstate.rs`
   `every_function_key_draws_as_the_page_without_it`, which also draws a soft
   mask whose `/TR` is a type 0 or type 4 function as its twin whose `/TR` is
   the type 2 function of the same curve.
3. **K-01.4: the halftone keys.** `/HT` of types 1, 5, 6, 10 and 16, and
   `/Default`, stored as typed halftones with each threshold array by its
   stream, and `/HTO`. Exit: `display.rs`'s `every_halftone_key_is_stored`;
   `ext_gstate.rs` `every_halftone_key_draws_as_the_page_without_it`.
4. **K-01.5: the font by reference, for every reader at once.**
   `fonts::state_font` and `fonts::text_font`, the reserved name and its id,
   and every reader above in one change. Exit: `ext_gstate.rs`
   `a_font_set_by_gs_draws_and_reads_as_tf` (renders, extracts, subsets and is
   listed as its `Tf` twin, and so does the page whose `/Font` does not name
   the object); `display.rs`'s `gs_twins_record_the_same_events` with the
   font's twins; `redact.rs`'s `text_whose_font_gs_set_is_redacted_as_tf_text`;
   `pdfa_fonts.rs`'s unembedded font reached only by `gs`; the `gs`-driven
   page in `determinism.rs`'s table; in the default and `bundled-fonts`
   builds.
5. **The row's corpus half, which no step holds.** The exit criterion's
   `rotate` and `crop` relations are re-run over the corpus by `corpus.yml`'s
   three `corpus-run --check` steps on a run that carries K-01.5, each at or
   above its committed bar; corpora run only in CI.

## Risks

| Risk | Mitigation |
| --- | --- |
| A twin passes because neither page applies anything | Every applied key's twins are also compared with the page without the key, which they must not equal; the stored keys' recorded values are read, not only compared |
| A `gs` twin and its operator drift apart on a degenerate value | One setter per parameter, called by both; `every_scalar_key_set_by_gs_draws_as_its_twin` runs the degenerate operands of each |
| `/op`'s fallback to `/OP` is applied in one reader and not another | It is applied once, where the dictionary is read, and the value carries parameters, not keys; a dictionary writing only `/OP` and one writing both are pinned equal |
| Recordings of two documents compare unequal through ids alone | A twin is two pages of one document on one resource dictionary; the gs-only font page states its one difference |
| `PartialEq` on floats: NaN unequal to itself, `-0.0` equal to `0.0` | No state holds a NaN the interpreter's checks let through; pixels are compared beside recordings, so a sign of zero that drew differently is still caught |
| A font one reader draws and another cannot see, which a redaction leaves behind | One function chooses the name and one resolves it, used by the facade, redaction and PDF/A; K-01.5 lands every reader in one commit with redaction's test |
| A forged reserved name in a `Tf` | 7.3.5 rules NUL out of names; a stream that writes one anyway selects the same object in every reader, and a name the scope binds is always asked first |
| A `gs` font mapped to a scope's name and inherited into a form that rebinds the name draws there in the form's font | It is `Tf`'s behaviour, shared on purpose so the twin holds; a reserved name, used where the scope names no such font, is absolute |
| The state's growth tips a page near the display list's cap into a direct render | One word per state, and the stored value counted once per change; a page past the cap draws the same bitmap, at a direct render's cost |
| A `gs` before every text object costs more than the three methods did | One reading of the keys present, where they lie, with no copy of a direct table; a set that changes nothing allocates nothing; a function is parsed once per object; a threshold array is not decoded; P-02.5 keeps each reading per name |
| A hostile `/D` puts a long array into every state copy | `/D` meets `d`'s bound: an array no operand stack could hand `d` is absent |
| A stored function's precision or sign is lost before its row applies it | Functions are kept parsed, not sampled; sampling is the applying row's, at its stage and depth |
| A large threshold array, or a type 5 halftone with many colorants, read at every `gs` | A threshold array is a reference until RC-08.9 decodes it; a halftone dictionary is read in time linear in its size, once per `gs`, as every key is |
| A malformed key read in a way the applying row cannot see | Absent is the one rule, stated here and pinned per key; a malformed key changes no output until its row applies it, and that row's design says what it warns |
| The soft mask's `/TR` fix moves a pinned render | The committed soft masks carry type 2 functions, read alike by both readings; the new twin pins type 0 and type 4 |
