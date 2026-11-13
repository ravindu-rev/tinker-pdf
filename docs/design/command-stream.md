# The command stream

A host that wants a page drawn by something other than this engine — a
browser's canvas, a platform's 2D graphics, a printer driver, a device of its
own written in a binding — wants what the renderer is handed: every path,
glyph, image, shading, clip, group and soft mask, in order, each with the
state it is drawn in, and the answers to the questions a render asks about
them, without the document. A display list is that, kept in memory for this
engine's own renders; this document makes it bytes a host can keep, send and
read, for roadmap row RC-18, and names the test that holds each part.

## Scope

- **A versioned binary form** of one page's drawing, the **command stream**:
  a header with a major and a minor version, then length-prefixed records a
  reader can skip.
- **The commands**: every call a display list records — paths, glyphs
  grouped into runs by font and state, images by reference, `sh`, clips,
  `q`/`Q`, marked content, forms, groups, soft masks and the budget notices —
  and the graphics state each carries, every field of it to the bit.
- **The tables a byte-identical replay needs**: the outline of every glyph
  drawn, every image decoded at every size a draw can ask for, every shading
  with its functions and colour spaces, each pattern's definition and each
  tiling cell as a nested stream of its own, each form's and each
  appearance's resource scope, and what each answer reported (ruling 10).
- **Writing** a retained page from its display list, and a page that is not
  retained by interpreting it into the writer.
- **Reading** with budgets, total over hostile bytes, and **replaying**
  through the engine's own rasteriser.
- **The C ABI cursor** and an encoder, and **each binding's surface**.

## Non-goals

- **SVG output.** `Page::to_svg` is the vector format a browser draws, and it
  is its own device and its own row (`docs/design/svg.md`); a command stream
  is the renderer's input, not a picture format, and nothing here writes SVG.
- **Callbacks across the C ABI.** H-28's question is answered by ruling 20,
  which gives the C ABI one callback, for a key the engine does not hold. A
  device written in a binding over the C ABI reads the decoded stream through
  the cursor and calls itself; no device callback crosses the boundary, and
  nothing here adds one.
- **Editing a page through its stream.** A stream is read and replayed; it is
  never written back into a document.
- **A smaller picture.** Images are held decoded, so a stream of a scanned
  page is larger than the page's file (*Risks*).
- **The document around the page**: links, annotations as objects, text,
  structure. An annotation's appearance is drawn, as a render draws it, and
  nothing else of it is in the stream.

## Design

### The form

```text
stream  := magic major minor record* end
magic   := 89 54 50 43 4D 44 0D 0A          ("\x89TPCMD\r\n")
major   := u16, little-endian                 (1)
minor   := u16, little-endian                 (1)
record  := kind:varint length:varint field*   (length counts the fields' bytes)
end     := the record of kind 0, with no fields, and nothing after it
field   := tag:u8 value
```

| Tag | Field | Value |
| ---: | --- | --- |
| 0 | `Nothing` | none: an absent value |
| 1 | `Unsigned` | a varint |
| 2 | `Signed` | a zigzag varint |
| 3 | `Real` | an IEEE 754 binary64, eight bytes little-endian, its bits as written |
| 4 | `Bytes` | a varint count, then the bytes |
| 5 | `Reals` | a varint count, then each real's eight bytes |
| 6 | `Naturals` | a varint count, then each varint |
| 7 | `Strings` | a varint count, then each one's varint length and bytes |

A varint is LEB128, seven bits a byte, low first, at most ten bytes, and the
shortest that holds the value; a zigzag varint maps `n` to `2n` and `-n - 1`
to `2n + 1`. So a list of records has exactly one encoding, and a stream read
and written back is the same bytes, which is what lets a binding round-trip a
stream through its own types.

Each record's fields are positional: its kind fixes what each place holds.
**A newer minor may append fields to a kind and add kinds**, nothing else: a
reader skips a record of a kind it does not know in a stream whose minor is
newer than its own, ignores fields past the ones it reads, and refuses an
unknown kind in a stream of its own minor or older (`UnknownRecord`). **A
newer major is refused by name** (`NewerMajor`), and so is anything that is
not a stream (`NotACommandStream`). Minor 1 appends a field to `Colorants`,
and a reader takes a `Colorants` record of minor 0, which ends before it, as
listing none.

A reference is a field `Unsigned(i)`, the `i`th record of its table counted
from zero in stream order, or `Nothing` for none. Every definition precedes
the first record that refers to it, so a reference points backwards and a
cycle cannot be written.

### The records

Every definition and every table precedes the first command, so a host can
draw as it reads: the `Page` record, then the definitions and the tables,
then the page's commands, then each annotation's after its `Annotation`
record, then `End`.

`tinker-pdf-content`'s codec (`crates/tinker-pdf-content/src/commands.rs`)
reads and writes kinds 0 to 37, the commands and the state they carry; the
facade (`crates/tinker-pdf/src/command_stream.rs`) reads and writes kinds 64
to 79, the page and its tables, and hands the codec every other kind it
knows. One list of kinds, `CommandKind`, names every kind and each kind's
fields, so a binding names a field without knowing the format.

**Definitions** (codec):

| Kind | Record | Fields |
| ---: | --- | --- |
| 0 | `End` | — |
| 1 | `Function` | type (0 sampled, 2 exponential, 3 stitching, 4 calculator, 5 array, 6 identity); sampled: domain, range, size, bits, encode, decode, samples; exponential: domain, c0, c1, n; stitching: domain, functions, bounds, encode; calculator: domain, range, code, numbers, operators; array: functions |
| 2 | `Halftone` | type (1, 5, 6, 10, 16); 1: frequency, angle, spot function or nothing, spot names, accurate, transfer; 5: names, halftones; 6: width, height, thresholds, transfer; 10: xsquare, ysquare, thresholds, transfer; 16: width, height, second width, second height, thresholds, transfer |
| 3 | `Stored` | rendering intent, black-point compensation, stroke overprint, fill overprint, overprint mode, alpha is shape, text knockout, flatness, smoothness, stroke adjustment, black generation, undercolour removal, transfer, halftone, halftone origin |
| 4 | `Colorants` | kind, names, tints, which have their own ink, those inks, alternate, process; and, from minor 1, which tints are no ink and not zero — a tint of 0 a file gave as more than 0, which 8.6.7's non-zero overprint mode reads as not zero (`ColorantTint::zero`); every other byte 0 is zero, and a tint of ink listed there is malformed |
| 5 | `Source` | the device space the components were read in, by name, or nothing for the slot's own; the components |
| 6 | `State` | ctm; the text numbers — size, character spacing, word spacing, horizontal scale, leading, rise; render mode; font; fill alpha; stroke alpha; fill colour; stroke colour; both at sixteen bits, fill then stroke; fill ink; stroke ink; fill family; stroke family; fill colorants; stroke colorants; line width; line cap; line join; miter limit; dashes; dash phase; fill space; stroke space; fill pattern; blend; stroke pattern; stored; fill source; stroke source |

A calculator's program is a flat code — 0 a number, the next of `numbers`; 1
an operator, the next of `operators` by its name in 7.10.5; 2 a block opening
and 3 its end — and a `Transfer` is a `Naturals` of function references: none
for `/Identity`, one for a single function, four for four, `Nothing` for no
transfer function at all. A flag is `Unsigned` 0 or 1, a colour three bytes
and an ink four, and the enumerations are numbers: a render mode its `Tr`
operand; a line cap and join their `J` and `j` operands; a blend mode 11.3.5's
sixteen in the order of Tables 136 and 137, Normal 0; a colour family 0 grey,
1 RGB, 2 CMYK, 3 any other; a rendering intent Table 69's four in its order,
then 4 for a name it does not list; black-point compensation 0 `/ON`, 1
`/OFF`, 2 `/Default`; a colorant kind 0 DeviceCMYK, 1 colorants, 2
`NChannel`, 3 `/All`, 4 `/None`; and a group's space 0 grey, 1 RGB, 2 CMYK, 3
Lab.

**Commands** (codec), each with its state as a reference to a `State`:

| Kind | Record | Fields |
| ---: | --- | --- |
| 16 | `BeginText` | — |
| 17 | `GlyphRun` | state, font id, font name, size, vertical, codes, text lengths, text, transforms (six reals a glyph), advances, which glyphs carry a baseline, those baselines |
| 18 | `EndText` | — |
| 19 | `FillPath` | state, even-odd, path operators, path numbers |
| 20 | `StrokePath` | state, path operators, path numbers |
| 21 | `ClipPath` | state, even-odd, path operators, path numbers |
| 22 | `SaveState` | — |
| 23 | `RestoreState` | — |
| 24 | `DrawImage` | state, name, inline, inline dictionary, inline data |
| 25 | `DrawShading` | state, name |
| 26 | `BeginMarkedContent` | tag, visible, hidden layer, props present, mcid, actual text, alt, lang, expansion, stream, associated files |
| 27 | `EndMarkedContent` | — |
| 28 | `BeginForm` | id, name, entered |
| 29 | `EndForm` | id |
| 30 | `BeginGroup` | state, isolated, knockout, space, entered |
| 31 | `EndGroup` | — |
| 32 | `BeginSoftMask` | state, content, matrix, bbox, group present, isolated, knockout, space, stream, luminosity, backdrop, transfer, bbox operators, bbox numbers, entered |
| 33 | `EndSoftMask` | — |
| 34 | `ClearSoftMask` | — |
| 35 | `GroupBudgetSpent` | — |
| 36 | `RecursionRefused` | — |
| 37 | `NestedBudgetSpent` | — |

A path is its operators, one byte a segment — 0 move, 1 line, 2 curve, 3
close — and its numbers, two for a move or a line and six for a curve. **A
glyph run** is consecutive `ShowGlyph` events with the same state, font id,
font name, size and writing mode, at most 65 536 of them; a run is the
events it stands for, in their order.

**The page and its tables** (facade):

| Kind | Record | Fields |
| ---: | --- | --- |
| 64 | `Page` | crop box, rotation, the page group's space, usage, page index |
| 65 | `Annotation` | scope: the commands after it, up to the next `Annotation` or `End`, are one annotation's appearance |
| 66 | `MissingFont` | name: a font the page's interpretation could not resolve, said where the replay reaches it |
| 67 | `RepairedSpace` | name, repair: a colour space the interpretation read with a repair, likewise |
| 68 | `Scope` | id, parent scope, form name: a form's resources; with no parent and no name, an annotation appearance's |
| 69 | `Outline` | scope, font id, code, found, outline operators, outline numbers, notes |
| 70 | `FontIdentity` | scope, font id, identity (0 none, 1 an object, 2 the scope's own), object |
| 71 | `Profile` | an ICC profile's bytes |
| 72 | `ColorSpace` | type, then its parameters, its base, alternate, tint function, profile and colorants as references |
| 73 | `Image` | scope, name, intent, frame (0 none, 1 JPEG, 2 JPEG 2000), width, height, origin, levels, opaque, interpolate |
| 74 | `InlineImage` | scope, which inline image of the stream's commands, intent, frame and its fields as `Image` |
| 75 | `ImageDecode` | image, reduction, answer (0 none, 1 an image, 2 refused), codec, width, height, rgb, alpha, two-colour samples, two colours, stencil, interpolate, reduced grid, family, soft-masked, window, notes |
| 76 | `ShadingDefinition` | type, space, function, coordinates or domain, extend; a mesh: its kind, components, positions, values, triangles, patch points, patch corners |
| 77 | `Shading` | scope, name, answer (0 none, 1 a type not painted, 2 a shading), type, definition, notes |
| 78 | `Pattern` | scope, name, answer (0 none, 1 unsupported, 2 shading, 3 tiling), definition, matrix, bbox, x step, y step, uncoloured, notes |
| 79 | `Cell` | scope, pattern name, own resources, the cell's stream |

Scopes are numbered from 1 in `Scope` order; scope 0 is the page's. **Notes**
end every record that answers a question the renderer asks — `Outline`,
`ImageDecode`, `Shading`, `Pattern` — as five `Strings`: the fonts, the
damaged images and what was tolerated, the repaired spaces and what was
repaired, that answering it noted in the scope it was asked in. A replay notes
them again in its scope at every answer, as a retained page's resources say
again what a cached decode reported.

### What a replay asks, and what answers it

A replay draws through the same `Renderer` a direct render does, and the
renderer asks its `GlyphSource` eight questions. The stream answers each with
the value the page's resources gave, in the scope the renderer is in:

- **`outline(font id, code)`**: the `Outline` record of the scope, for every
  glyph shown in a mode that paints or clips.
- **`font_identity(font id)`**: `FontIdentity`, so the glyph-mask cache keys
  a replay's glyphs as it keys the page's.
- **`image` and `inline_image`**: an image's decode depends on the draw's
  size — a JPEG decoded at a half, a quarter or an eighth, a JPEG 2000
  codestream with levels dropped (`docs/design/jpeg-speed.md`,
  `docs/design/jpx-speed.md`) — so the stream holds its frame and a decode at
  **every reduction a draw can ask for**, keyed by the intent in force at the
  draw, and a replay asks the frame what a request asks, as the page's
  resources do (`Frame::asked`). A decode is held whole: the window a JPEG
  2000 draw holds only bounds memory, and draws what the whole decode draws.
- **`shading` and `pattern`**: the definition, with its colour spaces and
  functions; an ICC space by the bytes of its profile, compiled again on read
  (`icc::Transforms::compile_bytes`), so its transforms are the page's.
- **`tile`**: a tiling cell is a content stream, which a replay cannot
  interpret, so the writer interprets it once into a nested stream of its own
  events and tables, recorded with the renderer's answers, in the cell's own
  resources or the scope that names the pattern, to the depth the renderer
  reaches (`MAX_PATTERN_DEPTH`); a replay draws the cell by replaying it at
  the request's geometry exactly as `PageResources::tile` draws it, and folds
  what it noted into the asking scope where the cell has resources of its
  own.
- **`form_scope`**: the `Scope` record whose parent is the asking scope and
  whose name is the form's, a fresh scope each time, as the page's resources
  bind one; none where the form brings no resources.
- **`kept_mask`** and **`keep_mask`**: a stream keeps no masks beyond a
  render, so the render keeps its own.

The answers are written by asking the page's resources the same questions,
each through resources of its own for one answer
(`PageResources::for_one_render`) so that what it notes is that answer's, and
a request built for each reduction. The interpretation's own notes — a font
name the page's resources do not define, a colour space read with a repair —
are kept where a replay reaches them: after the content for a retained page,
as `DisplayList::render` notes them, and after the call they were met at for
a page interpreted into the writer and for a cell, as a direct render meets
them.

### A page that is not retained

A page whose recording passes `MAX_DISPLAY_LIST_BYTES` keeps no calls, and a
list draws it by interpreting it again. The writer does the same: it is a
`Device`, answering the interpreter's questions through the renderer's own
`Admission` as `DisplayRecorder` does, so the page is interpreted into it and
its calls are written as they come, its annotations drawn into it through
`annots::draw` — as `DisplayList::to_svg` writes such a page. The stream is
the same whichever way it was written.

### The reader's budgets

`CommandBudgets` bounds what a stream can make a reader hold, and a stream
past one is refused by name (`OverBudget`), never read in part:

| Budget | Default | What it bounds |
| --- | ---: | --- |
| `records` | 2^24 | records in a stream, nested streams' included |
| `bytes` | 2^30 | bytes the read holds: every field's values, every record, and every command's state and path counted as a display list counts them (`Event::kept_bytes`) |
| `depth` | 8 | nested cell streams |
| `functions` | 32 | functions nested in functions, and calculator blocks |
| `spaces` | 8 | colour spaces nested in colour spaces, and halftones in halftones |

Every count is checked against the bytes left before anything is allocated,
so a hostile length costs a comparison, and every copy a reference makes — a
state into each command, a function into the function that holds it — is
checked against the byte budget before it is made. A list of commands must
nest as a recording's does: each form, group and soft mask entered ends
inside the ones open around it, and none is left open at the list's end. A
glyph run carries as many of each field as it has glyphs, and a reference
points at a record that precedes it.

### The facade

```rust
impl Page { pub fn command_stream(&self) -> Vec<u8> }        // its display list's
impl DisplayList { pub fn command_stream(&self) -> Vec<u8> }
impl CommandStream {
    pub fn parse(bytes: &[u8]) -> Result<CommandStream, CommandStreamError>;
    pub fn parse_with(bytes: &[u8], budgets: &CommandBudgets) -> Result<CommandStream, CommandStreamError>;
    pub fn from_records(records: Vec<CommandRecord>) -> Result<CommandStream, CommandStreamError>;
    pub fn records(&self) -> &[CommandRecord];
    pub fn to_bytes(&self) -> Vec<u8>;
    pub fn version(&self) -> (u16, u16);
    pub fn usage(&self) -> Option<Usage>;
    pub fn render(&self, options: &RenderOptions) -> Bitmap;
}
```

`render` draws as `DisplayList::render` draws, through the same pipeline: the
scale and its clamp, the region, the format, anti-aliasing and its levels,
forced colours, a transparent or premultiplied page, cancellation, the
content and the appearances by their kind (each `Annotation` record says
whether it is a widget's), and statistics — drawn whole on the caller, the
bitmap a render in bands draws. The options it does not read are `usage`,
`layers` and `magnification`: a stream is the drawing of the usage and the
optional-content configuration it was written for, which its `Page` record
names and `usage()` returns, an appearance `/NoZoom` or `/NoRotate` places at
render time written placed as at magnification 1, since there is no document
to interpret again. An image XObject's entry holds the room the writing
document's budget left its decode, so a draw takes the reduction the page's
resources would and notes it as they do.

### The C ABI

Handle-based and engine-owned, as `TpdfBitmap` is (`docs/design/bindings-write.md`):

- `tpdf_page_command_stream(page, out)` hands back the bytes as a
  `TpdfBuffer`.
- `tpdf_command_stream_parse(bytes, len, out)` reads them into a
  `TpdfCommandStream`; its **cursor**, `tpdf_command_stream_next(stream,
  TpdfCommand *out)`, fills one `TpdfCommand` per call — its kind an `int`
  checked against `TpdfCommandKind`, its record's index and its field count —
  and `tpdf_command_stream_field(stream, index, field, TpdfCommandField *out)`
  reads one field as plain values: its tag, an unsigned, a signed, a real, or
  a pointer and length into the handle's storage, borrowed until the handle
  is freed. A reference is an unsigned like any other.
- **Table accessors**: `tpdf_command_stream_outline` hands back an outline
  record's operators and numbers, and `tpdf_command_stream_image` an image
  decode's size and samples, so a host draws glyphs and images without
  reading their fields one at a time.
- **An encoder**, `TpdfCommandEncoder`, takes commands back: each pushed as
  its kind and its fields as `TpdfCommandField`s, and finished into the bytes,
  checked as a parse checks them. A wrapper round-trips a stream without
  parsing the format itself.
- `tpdf_command_stream_render(stream, scale, format, out)` replays it.

`TpdfCommandKind` numbers the kinds from zero, as every enum on the ABI is
numbered for the bindings that pass an enum by its ordinal, so a command
carries its kind's number on the wire beside it, and -1 for a kind a newer
minor version added. `tpdf_command_stream_count`, `_rewind` and `_record`
count, rewind and read by place beside the cursor; `_to_bytes`, `_version`,
`_page_index` and `_size` project the facade's own. Bytes the reader refuses,
and commands whose stream it would refuse, are `BadArgument` with the
reader's sentence.

A command is read through a pointer the caller passes, so Ruby's Fiddle,
which passes no struct by value, can call every function. No callback crosses
the boundary (H-28 untouched).

### Each binding's surface

Ruling 11 keeps a binding a projection: each projects the facade's decoder —
the records, their kinds and fields, the encoder and the render — and a
device interface fed from it, one method a command kind, which the binding
calls in the stream's order. A replay to a platform's 2D canvas is an example
over that projection, written in the binding's README — or, for Java, which
has none, in its smoke program, where it runs — and never binding API.

| Binding | Route | Its platform's 2D canvas | The example |
| --- | --- | --- | --- |
| JavaScript | the facade, through wasm-bindgen | `CanvasRenderingContext2D` | a replay to a canvas in the README |
| Python | the facade, through PyO3 | none in the standard library that clips, blends or draws cubic Béziers (`tkinter.Canvas` does neither of the first two) | the device protocol alone |
| .NET | the C ABI | `System.Drawing.Graphics` (GDI+), supported on Windows only | a replay to `Graphics` on Windows |
| Java | the C ABI | `java.awt.Graphics2D` | a replay to `Graphics2D` in `test/Smoke.java` |
| Go | the C ABI | none: `image/draw` composites rasters and draws no paths | the device interface alone |
| Ruby | the C ABI | none | the device interface alone |
| Swift | source only (SU-13) | Core Graphics on Apple platforms | none until SU-13 widens its coverage |

## Milestones

1. **RC-18.1**, this document.
2. **RC-18.2, the codec.** `crates/tinker-pdf-content/src/commands.rs`
   `every_event_round_trips_to_the_bit`: a recording holding every `Event`
   variant and every `GraphicsState` field at edge values — negative zero,
   infinities, a NaN with a payload, the largest and smallest integers —
   reads back equal and writes back to the same bytes, `f64` bits included;
   `a_newer_minor_s_unknown_record_is_skipped` and
   `a_newer_major_is_refused_by_name`;
   `truncations_and_mutations_never_panic`, a seeded campaign over every
   prefix of a stream and single-byte changes at every offset; and the
   `command_stream` fuzz target in `cargo xtask fuzz`'s roster.
3. **RC-18.3, the writer.** `crates/tinker-pdf/src/command_stream.rs`
   `the_stream_answers_every_question_a_render_asks`: a page holding text in
   an embedded face and a Type 3 font, three image kinds, an axial and a mesh
   shading, a tiling cell holding an image and a form with its own resources,
   rendered through a logging `GlyphSource` at 0.5 and 2.25 times, asks only
   questions the stream's tables answer, each with the value the page's
   resources gave — outlines to the bit, images byte for byte, shadings field
   for field — and the tables read back from the bytes equal those written.
4. **RC-18.4, the replay.** `crates/tinker-pdf/tests/determinism.rs`
   `a_command_stream_replays_every_fingerprinted_page_byte_for_byte`: every
   fingerprint fixture's stream, parsed and rendered at 1, 0.5, 1.5 and 2.25
   times with and without annotations, equals the direct render in pixels,
   size, format and warnings; `the_command_stream_is_the_same_bytes_on_every_target`
   pins each fixture's stream hash; both in K-07's roster. The `command_stream`
   fuzz target parses and renders what it reads.
5. **RC-18.5, the C ABI.** `crates/tinker-pdf-ffi/src/commands.rs`
   `the_command_stream_round_trips_through_the_c_abi`: for a page holding
   every command kind, the commands the cursor reads, encoded back, give the
   facade's bytes; `raw_enum_tests.rs` pins `TpdfCommandKind`'s range and its
   refusal of an undeclared number.
6. **RC-18.6, landed: Python and JavaScript.**
   `bindings/python/tests/command_stream.py`
   and `bindings/js/tests/command_stream.mjs` decode and re-encode the stream
   of a page holding every command kind to the facade's bytes, and drive a
   recording device that receives every kind in the order the facade's
   decoder gives them.
7. **RC-18.7 and RC-18.8, landed: .NET, Java, Go and Ruby**: each
   wrapper's smoke program decodes and re-encodes the same page's stream
   through its own types to the facade's bytes, dispatches every kind to a
   recording device, and exits non-zero on any difference —
   `bindings/dotnet/tests/Smoke/Program.cs`, `bindings/java/test/Smoke.java`,
   `bindings/go/cmd/smoke/main.go` and `bindings/ruby/test/smoke.rb` — and
   the FFI's `the_command_stream_enums_have_the_numbers_the_bindings_transcribe`
   pins the three enums .NET and Ruby transcribe and Java passes by position,
   while Go names the header's constants.
8. **RC-18.9a, landed: parity.** The *command-stream* script in the
   facade's `crates/tinker-pdf/examples/write_parity.rs` and in every
   surface's parity program — `bindings/python/tests/write_parity.py`,
   `bindings/js/tests/write_parity.mjs`, `bindings/dotnet/tests/Smoke`,
   `bindings/go/cmd/parity`, `bindings/ruby/test/write_parity.rb` and
   `bindings/java/test/WriteParity.java` — decodes each page's stream of the
   graphics artefact into that surface's own records and fields, rebuilds
   each field from its tag and each record from its kind, writes the records
   back and prints `READ sha256=` of the bytes, and `cargo xtask
   bindings-parity --require-all` holds every surface to the one hash
   `xtask/src/parity.rs` records. **RC-18.9b** projects what no surface
   carries yet, so `cargo xtask bindings-coverage` passes with no refusal
   line left for the stream.
9. **RC-18.10a, landed: the speed rows**, unmeasured, in MS-04's render
   group (`crates/tinker-pdf/benches/render.rs`):
   `render/write the 150-dpi text page's command stream`, two writes an
   iteration, and `render/the text page at 150 dpi from its parsed command
   stream`, two renders an iteration of the stream written and parsed once,
   each held before it is timed to a stream that reads back to its own
   bytes and draws the page's render pixel for pixel, and each paired with
   the planning baseline's direct render of the same page, which that
   build draws in its place; `xtask/src/bench.rs`
   `the_committed_baseline_agrees_with_the_tree` holds the bench file and
   `baseline.json` to one list. **RC-18.10b** takes their figures in the
   timing phase, committed in `baseline.json` and held by `cargo xtask
   baseline`.

## Risks

| Risk | Mitigation |
| --- | --- |
| A stream holds every image decoded, at every reduction a draw can ask for | Stated rather than avoided: a replay that decoded would need the document's colour machinery for each image. A JPEG's four scales are about four thirds of its whole decode, a codestream's levels likewise |
| A stream past the reader's budgets cannot be read | Refused by name with the budget it passed; `parse_with` reads it under a caller's larger budgets |
| The tables miss a question a render asks | The writer asks the questions the renderer asks, per scope, for every call that can ask one, and `the_stream_answers_every_question_a_render_asks` logs every question a render of a page holding each kind asks and finds each answered |
| A replay's warnings differ from the page's | Every answer carries what it noted, and the interpretation's notes stand where a replay reaches them; the determinism test holds the warnings with the pixels |
| A decode held whole draws otherwise than the window a draw asked for | A window holds what a draw reaches widened by the sampler's reach, so the canvas's pixels are the whole decode's (`docs/design/jpx-speed.md`); the determinism test covers the minified codestream fixture |
| An ICC space whose transforms were built from a parsed profile, not from bytes, has nothing to write | Every space the engine reads from a document is compiled from its profile's bytes; one a caller builds by hand is written as 8.6.5.5's approximation by its component count |
| A binding's platform has no 2D canvas: Python's standard library, Go, Ruby | Each ships the device interface alone, as *Each binding's surface* records; JavaScript (`CanvasRenderingContext2D`), Java (`Graphics2D`) and .NET on Windows (`System.Drawing.Graphics`) carry a replay example |
| The format drifts from its readers | One kind list, `CommandKind`, read by the codec, the facade and the C ABI; a newer minor only appends, and `the_command_stream_is_the_same_bytes_on_every_target` pins every fixture's bytes |
