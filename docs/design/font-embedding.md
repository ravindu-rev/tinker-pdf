# Embedding missing fonts on write

*For [ROADMAP](../ROADMAP.md) row FT-12.* A document that names a font and
embeds no program leaves its reader to supply the glyphs, and every reader
supplies different ones. This engine draws such a font through a host's
`FontProvider` (`docs/design/fallback-faces.md`, `docs/design/symbol-faces.md`),
and a file drawn that way draws differently in the next reader. This document
fixes the pass that writes the face a provider drew into the file, so that the
rewritten document draws as the provider face drew it wherever it is opened:
which fonts it embeds and which it leaves, where it sits on the save door,
how a font's dictionary, descriptor and program change, how `/Widths` is kept
and a face whose advances disagree is measured, and what ISO 19005's font
clauses ask of what it writes at each part. SD-02 and SD-12 call this pass as
their font fix-up.

Clause and table numbers are ISO 32000-2's, read from it with this
repository's own `tpdf text`. The tables of 9.8 and 9.9 are numbered two
lower there than in ISO 32000-1, which the source and the roadmap otherwise
cite: Table 120 (*Entries common to all font descriptors*) is ISO 32000-1's
Table 122, Table 121 (*Font flags*) its 123, Table 122 (*Additional font
descriptor entries for CIDFonts*, `/Style` among them) its 124, and Table 124
(*Embedded font organisation for various font types*) its 126. ISO 19005's
clauses are cited by the numbers `crates/tinker-pdf/src/pdfa.rs`'s clause
tables give them, and their requirements as veraPDF's published rule
statements quote them, the source `pdfa/fonts.rs` already reads them from.

## Scope

- **The request** (FT-12.2): `FontRequest` carries the PANOSE classification
  and family class a descriptor's `/Style` states (9.8.3.2), beside the name
  and flags it carries today.
- **The pass** (FT-12.3): on the facade's save door, opt in, for each font the
  pages draw whose dictionary embeds no program — a simple Type 1 font, the
  standard 14 included, or a TrueType font — the provider's face embedded,
  cut to the glyphs drawn, under the key and subtype Table 124 gives it, with
  its descriptor written or completed and `/Widths` kept; and a report naming
  every such font, embedded or left, with its reason.
- **Widths** (FT-12.4): each drawn code's advance in the face measured against
  the width the dictionary lays it out with, a typed warning for each that
  differs, and the embedded program's advance set to the dictionary's.
- **PDF/A's font clauses** (FT-12.5): what the pass writes, written to pass
  every font clause `validate_pdfa` runs at every part and level, and a font
  whose face cannot be made to is left and named.
- **Composite fonts** (FT-12.6): a CIDFont embedded where the provider's face
  states each drawn CID's glyph itself, and every other composite font left
  and named.
- **Its speed row** (FT-12.7), in MS-04's write group.

## Non-goals

- **A font that embeds a program.** The document's own program is the
  document's statement, however damaged, and replacing it is not this row: it
  is drawn as it is, and the pass passes over it, as it passes over a Type 3
  font, whose glyphs are the document's own procedures (9.6.5).
- **Choosing a face.** Matching is the provider's: `SimpleFontProvider` keeps
  choosing by weight and slope, and FT-13's policy matches on name, flags and
  PANOSE. The pass embeds whatever answers.
- **Fitting a face to the widths.** The face is embedded as the default
  substitute path draws it, the pen moving by the dictionary's widths and each
  glyph drawn at its own size. Fitting a glyph to its width is metric-matched
  substitution (FT-14, opt-in by H-147), and FT-14.1's design says how an
  embedding of a fitted face draws, for whichever of the two rows lands
  later.
- **Converting a face.** A TrueType answer is embedded as TrueType and a CFF
  answer as CFF; neither becomes the other, and a Type 1 answer, which the
  `FontProvider` contract does not offer, is left.
- **Recoding a document's text.** No content stream changes, so no font
  changes how its codes are read into glyphs except as this document states:
  a simple font stays simple, its codes one byte each, and a composite font
  keeps its encoding CMap.
- **A face embedded whole.** The row embeds a subset; a face whose licence
  forbids one is left, named, for the caller to supply another.
- **`/CharSet` and `/CIDSet`**, which the pass does not write (*What PDF/A
  asks of it*).
- **The builder's archival refusal.** `DocumentBuilder::finish_archival`
  refuses an unembedded font, and keeps refusing it: a builder's caller
  embeds directly, with `add_embedded_font` or `add_cid_font`.
- **The append door.** `write::append` runs no pass, and this is one.

## Design

### The switch, and where it sits on the door

`SaveOptions::embed` is a `FontEmbedding`, `Leave` by default: the pass is
not entered, and a save that does not ask writes exactly the bytes it wrote
before the switch existed. `FontEmbedding::Embed(Arc<dyn FontProvider>)`
names the provider it asks. Off by default for `ImagePolicy::Keep`'s reason:
a default that embedded host faces would put them in every saved file
unasked, and a face is the host's to hand over.

The pass runs **between the image pass and the font pass**. It reads content
streams, as the font pass does, and writes none, so its place after the
images is free; it has to come before the font pass because that pass walks
the document as the editor now has it (`subset::apply`'s view), and a
program written first is one the walk sees.

**The new programs are cut by the pass itself**, whatever `SaveOptions::fonts`
says of the document's own programs, because the row embeds a subset: under
`FontPolicy::Keep` nothing else would cut them. So the font pass is handed the
programs this pass wrote and passes over them — it would otherwise cut them a
second time, find the second cut no smaller and name each one
`SubsetNotSmaller`, which makes `SubsetOutcome::removed` false for a file whose
every program was cut. `SubsetReport` stays a report on the document's own
programs; the new ones are on the embedding report.

`Saved::embedded` is `None` when nothing was asked and an `EmbedReport`
otherwise, an empty one when no drawn font was missing its program. An
incremental save appends the new programs and the font dictionaries that name
them; the original file embedded none, so nothing of the pass's is left in its
prefix and the report is the rewrite's.

### Which provider it asks, and what

The pass asks the caller's provider **chained as a document opened with that
provider draws with it**: in a `bundled-fonts` build the bundled faces answer
what the host declines (`fonts::effective`), so the face a page was drawn with
and the face embedded are one. It asks once a font, with the request the
renderer asks with — `FontRequest::read` over the font's dictionary — so a
provider that answers a request the same way every time is asked the question
whose answer drew the page.

**The request's PANOSE** (FT-12.2). `/Style` is Table 122's, a descriptor
entry for a CIDFont, and holds one key, `/Panose`, whose value 9.8.3.2 makes a
twelve-byte string: the two bytes of the `OS/2` table's `sFamilyClass`, class
then subclass, and the ten of the PANOSE classification. `FontRequest` gains
`family_class: Option<u16>`, class in the high byte, and `panose:
Option<[u8; 10]>`, read from the descriptor `FontRequest::read` already reads
— a composite font's descendant's, a simple font's own, where a producer
writes one there too. A `/Style` that is no dictionary, a `/Panose` that is no
string, or one of any other length gives **neither**: a classification read
from part of a string is one the font did not state.

### Which fonts it embeds

Which fonts are drawn, and with which codes, is learned from the walk the font
pass makes (`subset::collect`): pages, form XObjects at any depth, Type 3
procedures, tiling cells, soft masks' groups and every state of every
annotation's `/AP`, with the codes shown through each font object recorded
beside the glyphs. A code shown at rendering mode 3 counts, as it does there:
the walk copies no graphics state, and a font embedded for text nothing paints
costs its subset and moves no pixel. A font object the walk shows a code
through is a candidate when its dictionary — a composite font's descendant's —
has a descriptor naming no `/FontFile`, `/FontFile2` or `/FontFile3` stream, or
no descriptor at all, and is not a Type 3 font.

Left, and named, before the provider is asked:

| Font | Why it is left |
| --- | --- |
| One written directly into a resource dictionary | there is no object for a rewrite to address (`NotAnObject`) |
| One the AcroForm `/DR` names | a field's next value may draw any character, and a cut face cannot (`FieldResource`, 12.7.3.3) |
| One a Type 3 font's own `/Resources` names | the interpreter runs a procedure in the enclosing scope, so the codes are credited to another font (`Type3Resource`, 9.6.5) |
| One shown more than 65 536 distinct codes | the pass holds every code it checks (`TooManyCodes`) |

### What the provider answers

| Answer | Embedded as |
| --- | --- |
| An sfnt with `glyf` and `loca` — `0x00010000` or `true`, a collection by its first member | a TrueType program |
| A bare CFF program | a CFF program |
| An sfnt with a `CFF ` table and no `glyf` (`OTTO`) | the `CFF ` table, as a CFF program |
| Anything else: a Type 1 program, a `CFF2` face, a WOFF file, bytes neither parser reads | left (`Unreadable`) |
| `None` | left (`Declined`) |

**The face's licence is honoured** (9.9.1). An sfnt's `OS/2` `fsType` that
asks for restricted-licence embedding — its low four bits exactly `0x0002` —
or for bitmap embedding only (`0x0200`) leaves the font
(`EmbeddingForbidden`); one that forbids subsetting (`0x0100`) leaves it too
(`SubsettingForbidden`), since the pass embeds a subset. A face with no `OS/2`
table, a bare CFF among them, states no restriction. ISO 19005 asks for
programs legally embeddable for universal rendering, which these are.

### What the face drew

For each drawn code the pass asks which glyph the face drew for it: the
renderer's own choice of glyph in a substituted face
(`resources::substitute_choice`, which the renderer's `substituted_glyph`
calls; `docs/design/fallback-faces.md` and `docs/design/symbol-faces.md`), as
a function the renderer and this pass both call, never a second resolver. A
code whose glyph is `.notdef` — no step of the substitute's reading reached
one — leaves the font (`NoGlyph { code }`): embedding the face would make a
missing glyph permanent in every reader, where a reader with a better face
draws it today, and ISO 19005's `.notdef` clauses (6.2.11.8, 6.2.10.9) refuse
such a reference whatever the face. So does a code a face with nothing to be
read by — no `cmap`, no glyph names, no `ROS` — reaches only by the glyph
number the code gives: that is 9.6.6.4's closing guess, which the face does
not state.

### How the dictionary, descriptor and program change

Table 124 pairs each key with the dictionaries it may appear under:

| Answer | Under | `/Subtype` | Descriptor key | Stream |
| --- | --- | --- | --- | --- |
| TrueType | a simple font | `/TrueType` | `/FontFile2` | `/Length1`, the program's length |
| CFF | a simple font | `/Type1` | `/FontFile3` | `/Subtype /Type1C`, no `/Length1` to `/Length3` |
| CID-keyed CFF of the font's collection | a composite font's descendant | `/CIDFontType0` | `/FontFile3` | `/Subtype /CIDFontType0C` |

`/FontFile3 /Subtype /OpenType` is never written: it is PDF 1.6, past ISO
19005-1's PDF 1.4, and every face it could carry has one of the three forms
above. A Type 1 or multiple-master font answered by a TrueType face becomes a
TrueType font, and a TrueType font answered by a CFF face a Type 1 font: the
key follows the outlines, and the `/Subtype` follows the key.

- **The descriptor is a new object**, so a descriptor two font dictionaries
  share does not embed one font's face for the other: the document's own
  entries copied where it had one, then `/FontName`; `/Flags`; every metric
  key Table 120 requires that the copy lacks — `/FontBBox`, `/ItalicAngle`,
  `/Ascent`, `/Descent`, `/CapHeight`, `/StemV` — and `/XHeight`, each from
  the face's own tables; the program's key; and no `/FontFile*`, `/CharSet`
  or `/CIDSet` the copy carried, each of which describes another program.
  From an sfnt: `head`'s box, `hhea`'s ascender and descender (else
  `OS/2`'s typographic pair), `OS/2`'s cap and x heights where its version
  states them, else the tops of the glyphs `H` and `x` reach, and `post`'s
  italic angle, each in thousandths of an em; from a CFF, its Top DICT's
  `FontBBox` and `ItalicAngle`, its Private DICT's `StdVW`, the tops of the
  glyphs named `H` and `x`, and — where the answer has no `hhea` of its own,
  a bare CFF — the box's top and bottom for the ascent and descent. A cap
  height no table or glyph states is the ascent, and a stem the face does not
  state is written `0`, which Table 120 reads as unknown. The flags are the
  document's where it had a descriptor; otherwise fixed pitch, serif, script
  and italic as the face's `post`, `sFamilyClass` and italic angle say, else
  as the request says. The symbolic flag is kept as the font had it — Symbol
  and ZapfDingbats, whose built-in encodings are their own (Annex D.5, D.6),
  symbolic whatever their dictionary states — and the nonsymbolic flag set as
  its inverse, as 9.8.2 asks of a writer, except in the TrueType form *What
  PDF/A asks of it* settles, which is symbolic.
- **The name** is the face's: 9.9.2 names a subset by its tag, a plus sign and
  the PostScript name of the font the subset was cut from, and that font is the
  face. The face's name ID 6 — or a CFF's Name INDEX entry — where it is one to
  sixty-three printable ASCII characters outside `[](){}<>/%`, else the
  document's `/BaseFont` with any tag taken off, else `Embedded`, so a tag
  always stands before a name. The tag is
  `tinker_pdf_cos::subset_tag` of the cut program, the function every subset
  this engine writes is named by. `/BaseFont`, the descriptor's `/FontName`
  and a composite font's descendant's `/BaseFont` take the one name, as the
  font pass renames a subset (9.8.1, 9.7.6).
- **`/Widths` is kept**: the pen moves by the dictionary, and the dictionary
  does not change. A font that has none — the standard 14, which 9.6.2.2 lets
  omit it — gains `/FirstChar` and `/LastChar` spanning the drawn codes and a
  `/Widths` stating, for each code between, the width the font laid that code
  out with: the standard metrics, by name for Symbol and ZapfDingbats.
- **The program** is the face cut to the glyphs the drawn codes chose
  (`tinker_pdf_font::subset_with`): composite components and `.notdef` kept,
  no glyph renumbered, `post` dropped as every cut drops it. A face the cut
  refuses leaves the font (`NotSubsettable`).

**What a code reads as is checked, not assumed.** Each font's objects are
written in one editor transaction, and before it commits the pass reads the
font back through the editor and its program back from what it wrote, and asks
of every drawn code what a reader of the new file asks: the glyph the embedded
program's mapping chooses — the renderer's own functions for an embedded
program, `simple_glyph_by_cmap` for a TrueType program and `cff_glyph` for a
CFF one — the width it is laid out with, and the text it reads as. A code
whose glyph, width or text differs from what the document drew and read
before rolls the transaction back and leaves the font
(`DrawsOtherwise { code }`). A TrueType program is written in the symbolic
form (*What PDF/A asks of it*), its `cmap` mapping each drawn code to the
glyph that code drew, so a font drawn by name — Symbol and ZapfDingbats,
drawn through `substitute_glyph` — is read by the glyph it drew as a text face
is; a CFF program is read by the names its encoding gives, written so that
each reaches the glyph drawn.

### Widths, and a face whose advances disagree

The dictionary's widths and the face's advances are two statements of one
number, and a face that is not metric-compatible with the font it stands in
for makes them two numbers. The pen moves by the dictionary either way — a
glyph is drawn at the face's own size, at the place the dictionary's widths
put it — so the page draws as the provider face drew it whichever is kept.
ISO 19005's metrics clause (6.3.6, 6.2.11.5, 6.2.10.5) asks the two to agree,
so **the embedded program's advance is made the dictionary's**.

For each drawn code, the width the font lays it out with (`Font::width_of`)
is compared with its glyph's advance in the face, in thousandths of an em as
`pdfa/fonts.rs`'s width rule reads it: a `glyf` face's `hmtx` over its units
per em, a CFF's charstring width through its `FontMatrix`. **The tolerance is
the clause's**, one thousandth of an em — the same constant the validator's
rule holds — so a face the warning passes is a face the clause passes. Each
code past it is an `EmbedWarning::WidthMismatch` on the report, naming the
font, the code and both widths.

Each drawn glyph's advance is then set to the width of the lowest drawn code
that chose it:

- **TrueType**: its `hmtx` entry, in font units, the width times units per em
  over a thousand, rounded to the nearest unit; `hmtx` is widened to an entry
  for every glyph where a glyph past `numberOfHMetrics` changes, and `hhea`'s
  `numberOfHMetrics` and `advanceWidthMax` follow.
- **CFF**: the charstring's width operand, the first of its first
  stack-clearing operator's operands, replaced where the charstring states one
  and inserted where it takes `defaultWidthX`, as the width less its Private
  DICT's `nominalWidthX`, through its `FontMatrix` — an integer where it is
  one and a 16.16 fraction where it is not. A charstring whose first operator
  is a subroutine call, or any operator other than a stack-clearing one,
  states its width nowhere a static rewrite can reach, and where that glyph's
  width has to be set the cut declines, which leaves the font
  (`NotSubsettable`).

The cut and the advances are one call in `tinker-pdf-font` (ruling 8: bytes,
glyph sets and advances in, bytes out), and the CFF path checks its own
output — every kept glyph's outline equal to the face's, and every set advance
read back — as the cut already does.

### What PDF/A asks of it

ISO 19005 constrains fonts used for rendering, and these are the clauses
`validate_pdfa` runs today and what each asks of what the pass writes:

| Clause (parts 1, 2 and 3, 4) | Asks | The pass writes |
| --- | --- | --- |
| Embedding (6.3.4, 6.2.11.4.1, 6.2.10.4.1) | a program under the key its font's subtype admits, in the format the key names; every glyph drawn defined | the three forms of the table above; every drawn glyph kept, and `.notdef` drawn by none |
| Subsets (6.3.5, 6.2.11.4.2, 6.2.10.4.1) | 9.9.2's tag; part 1 a `/CharSet` on a Type 1 subset and a `/CIDSet` on a CIDFont subset, parts 2 and 3 each only true where present | the tag; neither `/CharSet` nor `/CIDSet` |
| Metrics (6.3.6, 6.2.11.5, 6.2.10.5) | each drawn code's dictionary width the program's advance, within a thousandth of an em | the advances set, and checked after the cut |
| Encodings (6.3.7, 6.2.11.6, 6.2.10.6) | a symbolic TrueType font: no `/Encoding`, and its `cmap` one subtable (part 1) or one subtable or a (3,0) one (parts 2 to 4); a non-symbolic one `/WinAnsiEncoding` or `/MacRomanEncoding`, with `/Differences` held as `pdfa/fonts.rs` reads each part | every TrueType font in the symbolic form below |
| Unicode (6.3.8, 6.2.11.7, 6.2.10.7) | levels A and U and part 4: a `/ToUnicode`, or a predefined encoding, or a published collection | a `/ToUnicode` on every simple font it embeds |
| CIDFonts (6.3.3, 6.2.11.3, 6.2.10.3) | a complete `/CIDSystemInfo`; a `/CIDToGIDMap` on a CIDFontType2; the CMap's rules | a CIDFontType0, which needs no map; the collection and the CMap are the document's and are judged as they were |

**Every TrueType font the pass embeds takes the symbolic form**, which is the
one form that meets the encoding clause of every part whatever the font's
encoding was, and the one 9.6.5.4 recommends for a font whose glyphs are not
reached by `MacRomanEncoding` or `WinAnsiEncoding` — every standard-14 font
read by `StandardEncoding` among them. The descriptor's symbolic flag is set
and the nonsymbolic one clear; the dictionary's `/Encoding` is removed; and
the program's `cmap` is one (3,0) subtable, format 4, mapping `0xF000` plus
each drawn code to the glyph that code drew — 9.6.5.4's reading of a symbolic
font with no `/Encoding`, which every reader of the file takes code by code.
So a font drawn by name (Symbol, ZapfDingbats) is read by the glyph it drew,
and the reading depends on no table of glyph names.

**A CFF font keeps its encoding where it reaches the glyphs it drew**: `cff_glyph`
reads a code by the name its encoding gives, and for every font but Symbol and
ZapfDingbats that is the reading the face was drawn by. Where it is not, each
such code is named in a `/Differences` array by the name the face's charset
gives its glyph, over the dictionary's own base encoding and differences.

**The text is stated.** A symbolic TrueType font with no `/Encoding`, and a
CFF font with new `/Differences`, read their codes as text otherwise than the
document did, so every simple font the pass embeds carries a `/ToUnicode`
stating each drawn code's text as the document read it — its own `/ToUnicode`
where it had one, else its encoding — in one-byte `bfchar` entries; a code
whose text was empty stays unmapped. The read-back holds the result. A
TrueType font with a drawn code the document read as no text is left before
anything is written (`NoText { code }`): with its `/Encoding` gone, a reader
reads such a code through `StandardEncoding` as a character, and only a
`/ToUnicode` entry could say otherwise, which an empty text is not. Under a
CFF font that keeps its encoding the code reads as it did, and the read-back
judges one whose `/Differences` changed.

**A face that cannot meet a clause is left and named, never embedded
half-conforming.** After the cut, the pass checks the metrics clause on what
it wrote: a drawn code whose program advance is still more than a thousandth
of an em from its width — two codes that chose one glyph at two widths, or a
width the face's units per em cannot state that closely — leaves the font
(`WidthsDisagree { code }`). Every other clause is met by construction and
held by the read-back.

**`/CharSet` and `/CIDSet` are not written.** Parts 2 and 3 judge them only
where present, part 4 runs no rule on them, and part 1's requirement on a
subset is the staged half of 6.3.5 (`pdfa_ledger.tsv`): a list this build's
validator does not judge is one nothing here could hold true. The pass gains
each when SD-01 un-stages it. Neither is the `/ToUnicode` value rule
(6.2.11.7.2, 6.2.10.7) held: the pass restates the document's own text.

### Composite fonts

A composite font's codes select CIDs through its encoding CMap, and a CID is a
number in the font's character collection (9.7.3). A face keyed by the same
collection — a CID-keyed CFF whose `ROS` is the font's `/CIDSystemInfo`
registry and ordering — states each CID's glyph itself, through its charset,
and a substituted one is drawn by that statement first
(`docs/design/fallback-faces.md`'s first route). Any other face is drawn
through a character: the collection's CMaps inverted, or `/ToUnicode`, to the
face's `cmap`. That reading is the collection's approximation of what a CID
means — a CID with no character, two CIDs with one, a vertical variant drawn
by the face's `vert` — and embedding it would need a `/CIDToGIDMap` writing
each approximation into the file as the font's own, for every reader, where a
reader with a face of the collection draws the CID itself today.

So **a non-embedded CIDFont is embedded only where the provider's face states
each drawn CID's glyph itself**: the answer, bare or an `OTTO` face's `CFF `
table, is CID-keyed, its `ROS` names the font's registry and ordering, and
every drawn code's glyph came from the first route. Its descendant is written
anew as a `/CIDFontType0` with `/FontFile3 /Subtype /CIDFontType0C`, its
`/CIDToGIDMap` dropped, and the Type 0 dictionary pointed at it, so a
descendant two Type 0 fonts share does not take one's cut for the other.
`/W` and `/DW` are kept and measured as `/Widths` is, and the advances set as
a CFF's are, in each glyph's own Private DICT. The encoding CMap and any
`/ToUnicode` are the document's, so the text reads as it did and none is
written. Every other composite font is left (`NotOfItsCollection`), the
`Adobe-Identity` collection among them: an `Identity` ordering numbers no
glyph two faces share, so a face keyed by it states no CID of another
document's.

### The report

```rust
pub enum FontEmbedding { Leave, Embed(Arc<dyn FontProvider>) }

pub struct EmbedReport {
    pub embedded: Vec<EmbeddedFont>,  // font-object order
    pub left: Vec<LeftFont>,          // font-object order, direct fonts last
    pub warnings: Vec<EmbedWarning>,  // font, then code
}

pub struct EmbeddedFont {
    pub font: ObjRef,
    pub base_font: String,            // as now written, tag included
    pub program: ObjRef,
    pub format: EmbeddedFormat,       // TrueType, Type1C, CidFontType0C
    pub face: usize,                  // the provider's bytes
    pub bytes: usize,                 // the cut's
    pub glyphs: usize,                // glyphs asked for
}

pub struct LeftFont { pub font: Option<ObjRef>, pub base_font: String, pub reason: LeftReason }

pub enum EmbedWarning {
    WidthMismatch { font: ObjRef, code: u32, dictionary: f64, program: f64 },
}
```

`LeftReason` is closed, as `UntouchedReason` is, so a caller matches every
reason and a new one is a compile error where it is handled: `Declined`,
`Unreadable`, `EmbeddingForbidden`, `SubsettingForbidden`, `NotAnObject`,
`FieldResource`, `Type3Resource`, `TooManyCodes`, `NoGlyph { code }`,
`DrawsOtherwise { code }`, `NotSubsettable`, `NoText { code }`,
`WidthsDisagree { code }` and `NotOfItsCollection`. Until FT-12.6 lands, a composite font is left as
`Composite`, which that step replaces.

**The same bytes on every target** (ruling 4). The faces are the provider's
bytes; the fonts are taken in object order, each object allocated in turn; the
walk is the font pass's, folded in page order on any number of threads; every
number the pass computes is integer arithmetic or a correctly rounded one —
units per em scaled with IEEE 754's four operations and `round` — and the tag
is a hash of the cut.

## Milestones

1. **FT-12.2, the request.** `FontRequest::family_class` and
   `FontRequest::panose`, read in `FontRequest::read`. Exit: `fonts.rs`'s
   `the_descriptor_panose_is_read` (a simple font's descriptor and a composite
   font's descendant's) and `a_malformed_panose_is_absent_not_guessed` (a
   string of eleven and of thirteen bytes, a name, an absent key, a `/Style`
   that is not a dictionary); `substitute_fonts.rs`'s
   `the_provider_is_told_the_panose`; every existing request test unchanged;
   default and `bundled-fonts` builds.
2. **FT-12.3, the pass.** `crates/tinker-pdf/src/embed.rs`,
   `SaveOptions::embed`, `Saved::embedded`, the font pass told which programs
   to pass over, the walk's codes, the substitute's choice of glyph as a
   function of its own, and `tinker_pdf_font::face_facts` for the descriptor.
   Exit: `embed_missing.rs`'s
   `a_rewritten_document_draws_as_its_provider_face_drew_it` (a standard-14
   page and an unembedded TrueType page rendered with the provider, rewritten,
   then rendered without one: fingerprints equal, and the text read the same),
   `the_embedded_face_is_cut_to_the_drawn_glyphs`,
   `an_embedded_or_type3_font_is_left_as_it_is`,
   `a_declined_request_is_left_and_reported`,
   `a_face_whose_licence_forbids_it_is_left`,
   `a_code_the_face_has_no_glyph_for_leaves_the_font`,
   `the_new_faces_are_cut_whatever_the_font_policy`,
   `a_cff_answer_is_embedded_as_type1c` and
   `a_save_that_asks_for_nothing_embeds_nothing`; the rewritten documents pass
   the strict validator; `determinism.rs`'s
   `the_rewritten_document_with_embedded_faces_is_the_same_bytes_on_every_target`,
   its value a run's on Linux, Windows and `wasm32-wasip1`; default and
   `bundled-fonts` builds.
3. **FT-12.4, widths.** Exit: `embed_missing.rs`'s
   `each_width_mismatch_is_a_typed_warning` (a face whose advances differ from
   `/Widths` on two drawn codes gives exactly two warnings carrying both
   widths, and a metric-compatible face none; the rewritten page still draws
   as the provider face drew it, and the program's advances are the
   dictionary's) and `a_cff_faces_widths_are_set_too`; `tinker-pdf-font`'s
   `subset.rs` `advances_are_set_where_asked_and_nowhere_else` and
   `cff_subset/tests.rs` `a_width_is_set_where_the_charstring_states_one_and_where_it_does_not`
   and `a_width_behind_a_subroutine_call_is_declined`; default and
   `bundled-fonts` builds.
4. **FT-12.5, PDF/A's font clauses.** Exit: `embed_missing.rs`'s
   `the_rewritten_document_passes_the_pdfa_font_rules` (for every fixture,
   rewritten and claimed as each part and level, `validate_pdfa` reports no
   finding under a font clause),
   `a_face_that_cannot_meet_a_clause_is_left_and_named`,
   `a_symbol_page_draws_as_its_provider_face_drew_it` and
   `every_truetype_font_takes_the_symbolic_form`; `tinker-pdf-font`'s
   `subset.rs` `a_symbol_cmap_maps_each_code_at_f000`; `pdfa_fonts.rs`
   unchanged; default and `bundled-fonts` builds.
5. **FT-12.6, composite fonts.** Exit: `embed_missing.rs`'s
   `a_composite_font_with_a_face_of_its_collection_draws_as_before` (and
   passes the font clauses as FT-12.5 holds them) and
   `a_composite_font_whose_face_is_not_of_its_collection_is_left_and_named`;
   default and `bundled-fonts` builds.
6. **FT-12.7, its speed row.** Exit: `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` holds `benches/write.rs` and
   `baseline.json` to one list with the operation in it, and `cargo bench -p
   tinker-pdf` reports its time.

## Risks

| Risk | Mitigation |
| --- | --- |
| A provider answers the same request differently at the save than at the render | The pass embeds what its own answer draws and checks what it wrote against that answer; a provider is asked for one face a request, and the trait documents it |
| A face drawn for a font is not the font, and its glyphs sit oddly at the dictionary's widths | That is how the page was drawn; the warning names every code where the two disagree, and a metric-compatible face gives none |
| Another reader reads the new font another way | The TrueType form maps every drawn code to its glyph directly, with no table of names between; a CFF font is read by the names its encoding gives, the reading Table 124's Type1C names for it |
| A hostile face (ruling 1) | The face is parsed by the leaves that draw it, the cut and the advances check their own output, and a refusal anywhere leaves the font |
| A licence the face states wrongly, or not at all | The pass reads what the face states and no more; a bare CFF states nothing, and 9.9.1 leaves the conditions to the face's own licence |
| A pass that changes what a save with no embedding writes | `Leave` never enters the pass and changes no argument the font pass is handed: an empty set of programs to pass over |
| A clause the validator stages is not met | Named above: `/CharSet` and `/CIDSet`, and the `/ToUnicode` values; the pass gains each with SD-01 |
