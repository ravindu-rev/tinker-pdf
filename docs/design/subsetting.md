# Subsetting breadth

*For [ROADMAP](../ROADMAP.md) row FT-11.* The rewrite path cuts a TrueType
program and a CFF whose subroutine calls it can renumber
([editing](../features/editing.md), *Font subsetting on rewrite*), and writes
everything else through whole as `UntouchedReason::ProgramNotRebuildable`: a
Type 1 program always, and a CFF whose subroutine numbers it would have to
guess. This document fixes four mechanisms that lift those refusals — a Type
1 subsetter, a CFF subset written with its subroutines inlined, a Type 1
program converted to a CFF, and a subset extended with glyphs it lacks — and
the edge cases each meets, once. One rule holds all four: **a glyph a page
needs never leaves the program.** The sources are the Adobe Type 1 Font
Format (version 1.1, the chapters cited by number), Adobe's Technical Note
5177, *The Type 2 Charstring Format* (16 March 2000, its sections cited as
5177 §n), Technical Note 5176, *The Compact Font Format Specification*, and
ISO 32000-2.

## Scope

- **Which Type 1 glyphs a code selects** (FT-11.3): one selection for the
  renderer and `PageResources::selection`, so the usage walk counts Type 1
  glyphs, and which glyphs a Type 1 subset keeps.
- **The Type 1 subsetter** (FT-11.2): the charstrings and subroutines it
  keeps, how both cipher layers are written, the three segments, and what it
  refuses.
- **Type 1 on save** (FT-11.4): `/Length1` to `/Length3`, `/CharSet`, the
  subset tag, and the report.
- **The CFF reader's arithmetic** (FT-11.5): the arithmetic, storage and
  conditional operators of 5177 §4.4 to §4.6, what each does where 5177 leaves
  the result undefined, and `random`'s sequence.
- **Flattening** (FT-11.6): which refused CFF shapes are written with their
  subroutines inlined, and which stay refused.
- **Type 1 to Type 2** (FT-11.7, FT-11.8): the charstring mapping, and when a
  save converts.
- **Extension** (FT-11.9): the call ED-02 makes, and how it proves a face is a
  subset's source.
- **What is measured** (FT-11.10, FT-11.11).

## Non-goals

- **Renumbering a Type 1 program's subroutines.** A subroutine no kept glyph
  reaches becomes a bare `return` at its own number, so no `callsubr` operand
  is rewritten and none can be rewritten wrongly.
- **A CFF whose CharStrings INDEX holds Type 1 charstrings** (`CharstringType
  1`, 5176 Table 9): neither renumbering nor flattening reads that language,
  and the converter takes a Type 1 *program*, not a CFF. It stays refused.
- **The forms a PDF does not embed** (ISO 32000-2 Table 125, note 2): a PFB
  container, a hexadecimal encrypted portion, and the synthetic and hybrid
  programs of chapter 9, each refused by name and written whole.
- **New hints.** A flattened charstring carries the hints its glyph executed;
  a converted one carries its Type 1 stems as Type 2 stems and masks. Nothing
  computes a hint.
- **A `/CharSet` where the face had none.** PDF 2.0 deprecates the entry
  (Table 120), and the face, which was not a subset, carried none.
- **A Type 42 program and a multiple-master instance.** FT-10 draws them; a
  save writes each through whole, as `ProgramNotRebuildable`, as today.

## Design

### Which glyphs a Type 1 code selects

A Type 1 program names its charstrings, and a code reaches one through an
encoding (ISO 32000-2 9.6.5.2): the document's `/Encoding`, where it names
the code, and otherwise the program's own built-in encoding. `cos::Font::
glyph_name` is the first: `/Differences`, then a base encoding the dictionary
names, `None` where the document named nothing. The second is the program's
`/Encoding`: its `dup <code> /<name> put` entries, or — chapter 10.3 — the
token `StandardEncoding`, which assigns the Adobe Standard Encoding, so
`Type1::glyph_for_code` reads a program that says `/Encoding
StandardEncoding def` through the Standard Encoding's names.

One function, `type1_selection` in `resources.rs`, chooses the charstring
for the renderer, `to_svg` and the usage walk alike, first match wins:

1. **the document's name** for the code, where the program has a charstring
   of that name;
2. **today's route through the code's text**: the first character
   `Font::text_of` gives, its Adobe Glyph List name, and the charstring of
   that name;
3. **the built-in encoding's** glyph for the code.

So a page changes only where the document's encoding and the code's text
disagree: a `/Differences` name with no Unicode meaning, and a `/ToUnicode`
that maps a code to another letter, draw the glyph the encoding names. A code
none of the three reaches draws nothing, as today.

`Selection` answers a Type 1 program with the glyph drawn, whether the font
stated it, and a second glyph to keep (`Selection::also`):

| The document names | The text reaches | The built-in encoding gives | Drawn | `stated` | Also kept |
| --- | --- | --- | --- | --- | --- |
| a glyph the program has, G | — | — | G | yes | — |
| nothing | B or nothing | B | B | yes | — |
| nothing | T | B, not T | T | yes | B |
| nothing | T | nothing | T | no | — |
| a glyph the program lacks | T | — | T | no | — |
| a glyph the program lacks | nothing | B | B | yes, as `cff_glyph`'s fall-through is | — |
| anything | nothing | nothing | nothing | no, the glyph `.notdef` | — |

The third row is where this renderer and 9.6.5.2 part: a document that names
nothing for the code is drawn here through its text and by a conforming
reader through the built-in encoding, so the subset keeps both. A row marked
no is a code whose glyph rests on this renderer's own guess, and a program
any shown code of which lands on one is left whole as `CodeNotMapped`, as for
the other formats. `.notdef`, which 9.6.5.2 substitutes for a name the
program lacks, is kept in every subset. PDF/A's width rule asks `selection`
too and skips a Type 1 program before it does, since `ProgramMetrics::read`
reads none.

The parsed program is kept beside the scope's font (`ScopeFont`), so a page's
glyphs decrypt it once rather than once a glyph.

### The Type 1 subsetter

`tinker_pdf_font::subset_type1(program, glyphs)` takes a program and glyph
indices — positions in the order `Type1::parse` reads the charstrings, which
nothing in a PDF addresses, so they never have to stay put — and answers a
`Type1Subset` (the program, its three segment lengths, and the kept glyphs'
names in program order) or a `Type1Refusal` naming why it declined.

**It reads the program as the reader reads it.** `type1.rs`'s scanner — the
one `Type1::parse` reads `/Subrs` and `/CharStrings` with — hands both the
spans of each entry: where its text begins (`dup` or the name's `/`), its
length operand, its `RD` token as the program spells it and its binary bytes.
So the subsetter edits exactly the entries the renderer decrypts, and an
entry the renderer does not read is copied as text.

**Which glyphs stay**: the asked-for glyphs the program has, `.notdef`, and
each `seac` component of a kept glyph — the Standard Encoding names its
`bchar` and `achar` codes give (chapter 6, *seac*), closed over, since a
component may itself be a `seac`. The reader composes one level: a component
is drawn as its own charstring and never composes again, so a `seac` that
names itself is drawn and not followed forever.

**Which subroutines stay**: every subroutine a kept glyph's charstring runs
on the reader's machine, followed through every call it makes; subroutines 0
to 3, the flex and hint-replacement entries chapter 8.4 fixes; and the
subroutine each hint replacement names — the `subr#` handed to OtherSubrs
entry 3 (chapter 8.1) — together with everything it calls, since an
interpreter that replaces hints calls it where this reader, which does not,
calls subroutine 3. An
OtherSubrs entry past 3 is answered as chapter 8 says an interpreter that
does not know it answers, its arguments back through `pop`, so a subroutine
number computed from one is the reader's.

**What is written.** The clear-text portion — up to `eexec` and the white
space after it — is copied byte for byte, so `/FontName`, `/Encoding`,
`/FontMatrix`, `/FontBBox` and `/UniqueID` say what they said; every kept
glyph draws as it did, so a device's cached bitmaps under the `UniqueID` are
still the glyphs'. The private section is the decrypted one up to its
`closefile`, with:

- each charstring nobody keeps deleted, from its `/` to the end of its `ND`
  token and the line end after it; `/CharStrings n dict` keeps its `n`, which
  chapter 10 lets exceed the entries;
- each subroutine nobody reaches rewritten in place as a one-byte charstring
  `return` (11), behind its own `dup <index>` and before its own `NP`, under
  the program's own `RD` spelling, so `/Subrs n array` and every subroutine
  number stay as they were;
- every kept charstring and subroutine copied as the ciphertext it was, lead
  bytes and all;

and one line feed after `closefile`, inside the encryption, as the token
needs a delimiter.

**Both ciphers, with fixed lead bytes** (chapter 7). A stub is encrypted with
the charstring key, 4330, behind `lenIV` zero bytes. The private section is
encrypted with the eexec key, 55 665, behind four zero bytes, and written in
binary, the one form ISO 32000-2 Table 125's note 2 admits. Four zero bytes
satisfy chapter 7.2's restrictions: the first ciphertext byte is `0xD9`,
which is neither white space nor a hexadecimal digit. Nothing random enters,
so the same program and glyphs give the same bytes on every target (ruling
4).

**The three segments** (Table 125). The input's encrypted portion runs at
least one byte past `closefile`, since eexec decrypts the delimiter that ends
the token before it stops. The fixed-content portion is the input's own: from
the first `cleartomark` after that byte back through the run of zeros and
white space before it, and everything after it — chapter
2.3's 512 zeros, `cleartomark`, and whatever the program appends, such as a
`{restore}if` that closes a `save` its clear text opened. A program with no
`cleartomark` there has none (Table 125: a `Length3` of 0, the 512 zeros and
`cleartomark` to be added by the reader). `Length1` is the clear text,
`Length2` the encrypted portion written, `Length3` the fixed content, and the
three add up to the program.

**It outlines every kept glyph in both programs before answering**: each
kept glyph is found in the subset by its name, and its outline and advance
must equal the original's, the glyph count must be the kept count, and the
subset must parse. A subset that disagrees is declined, never written.

**What it refuses**, by name (`Type1Refusal`), the program then written whole:

| Refusal | Why |
| --- | --- |
| `Unreadable` | `Type1::parse` declines it, or it carries no charstring |
| `Pfb` | a PFB container: PDF embeds the bare program (Table 124) |
| `HexEncrypted` | the encrypted portion in hexadecimal, which Table 125's note 2 says PDF does not support |
| `LenIv` | a `/lenIV` outside 0 to 16, which the reader does not decrypt as written |
| `SyntheticOrHybrid` | `FontDirectory` or `hires` after `/Private` (chapter 10.3): two sets of entries, of which the reader reads one |
| `NoCloseFile` | no `closefile` after the charstrings, so where the encrypted portion ends is not stated |
| `Entries` | two scanned entries overlap, or one lies past `closefile` |
| `DuplicateName` | two charstrings under one name: a PostScript interpreter keeps the last and this reader the first |
| `DuplicateSubroutine` | one `Subrs` index defined twice |
| `TooManyGlyphs` | more than the 20 000 charstrings the reader reads |
| `MissingSubroutine` | a kept glyph calls a subroutine the program does not define |
| `SubroutineDepth`, `Budget`, `StackOverflow` | a kept glyph's run met one of the reader's bounds — a call past ten deep (chapter 6, *callsubr*), the 100 000-operator budget, more than 48 operands — and what it draws there is this reader's, not the program's |
| `Mismatch` | the subset does not outline or advance a kept glyph as the program does |

### Type 1 on save

The usage walk records each shown code's glyph through the selection above,
the glyph kept beside it included. `subset.rs` hands a `/FontFile` program to
`subset_type1`; a refusal leaves it whole as `ProgramNotRebuildable`, and a
`/FontFile` whose bytes are not a Type 1 program at all goes to
`tinker_pdf_font::subset` as today. A written subset replaces the stream with
`/Length1`, `/Length2` and `/Length3` from its segments, takes the 9.9.2
subset tag on `/BaseFont` and `/FontName` as every subset does, and is
reported in `SubsetReport::subsetted` with its bytes before and after. A
program two font dictionaries share is one job, its glyph sets unioned.

**`/CharSet`** (Table 120: the names of a subset's glyphs, each as a PDF
name, `.notdef` omitted). A descriptor of the job that carries one has it
rewritten to the subset's names, in program order. It is dropped instead
where a kept name cannot be written as a name — it holds a NUL byte (7.3.5)
— since a `/CharSet` that lists less than the program holds is worse than
none. One is never added.

### The CFF reader's arithmetic

The Type 2 machine runs 5177 §4.4 to §4.6 in CFF, through one function,
`cff::compute`, so a charstring that computes an operand draws with it. CFF2
has none of these operators (CFF2 chapter, *CharString operators*) and clears
its stack at them.

| Operator | Code | Result | Where 5177 leaves it undefined |
| --- | --- | --- | --- |
| `abs`, `neg` | 12 9, 12 14 | the magnitude, the negation | — |
| `add`, `sub`, `mul` | 12 10, 12 11, 12 24 | sum, `num1 − num2`, product | `mul`'s overflow |
| `div` | 12 12 | `num1 / num2` | overflow; a zero divisor gives 0 |
| `sqrt` | 12 26 | the square root | a negative operand gives 0 |
| `drop`, `exch`, `dup` | 12 18, 12 28, 12 27 | as named | — |
| `index` | 12 29 | a copy of element `i` from the top | a negative `i` copies the top (5177); `i` truncated toward zero; an `i` past the bottom copies the bottom |
| `roll` | 12 30 | the top `N` elements turned by `J` (`rotate_right(J mod N)`, 5177's picture) | `N` and `J` truncated toward zero; `N` past the stack is the stack; `N` of 0 or less turns nothing |
| `random` | 12 23 | the next value of the sequence below | — |
| `and`, `or`, `not`, `eq` | 12 3, 12 4, 12 5, 12 15 | 1 or 0 | — |
| `ifelse` | 12 22 | `s1` if `v1 ≤ v2`, else `s2` | — |
| `put`, `get` | 12 20, 12 21 | the transient array, 32 elements (appendix B) | an index truncated toward zero and a negative one taken as element 0; one past the array ignored by `put` and read as 0 by `get`; an element no `put` set reads 0 |

An operator that finds fewer operands than it takes leaves the stack as it
is; `put` and `get` pop what there is. Every result is the IEEE 754
operation on doubles — exact for `add`, `sub`, `neg` and `abs` over the
numbers a charstring writes, correctly rounded for `mul`, `div` and `sqrt` —
and is then settled: one that is not finite, or smaller in magnitude than the
least normal double, is 0, which is 5177's *zero for underflow* and this
reader's answer to the overflow it leaves undefined. So an outline is the
same bits on every target (ruling 4), and nothing rounds to 16.16, which 5177
asks of an operand's encoding and of no result.

**`random`'s sequence.** Each glyph's run starts a 32-bit state at
2 463 534 242; each `random` steps it as xorshift with shifts 13, 17 and 5 —
`s ^= s << 13; s ^= s >> 17; s ^= s << 5`, modulo 2³² — and pushes
`((s >> 16) + 1) / 65536`, a value in (0, 1] as 5177 §4.4 asks, which a
16.16 number holds exactly. It reads no clock and no operating system, it
restarts at every glyph, so the order glyphs are drawn in changes no outline,
and a flattened charstring, which keeps every `random` in its order, draws
the values the original drew.

**Bounds.** The stack holds 48 operands (appendix B) and a push past them is
dropped; `roll` and `index` work within it; the transient array is 32
elements, and an index outside 0 to 31 is out of it; calls nest ten deep; and
the glyph's machine reads at most 64 000 operands and operators. The machine
records the first of these it meets — `Bound::StackOverflow`,
`TransientIndex`, `SubroutineDepth` and `Budget`, and `MissingSubroutine` for
a call to a subroutine the INDEX does not hold — in both flavours; CFF draws
on past it, CFF2 refuses the glyph at the three it shares with `Stop`, and
flattening refuses every one by name.

**The renumbering walk reads the same depths.** `cff_subset.rs`'s token walk
counts stem hints from the stack's depth, so it runs the same operators
through the same function as the reader and agrees with it at every
`hintmask`; the transient array and `random`'s sequence it keeps per glyph,
outside its memo, since no depth depends on a value they hold.

### Flattening

The renumbering walk declines two shapes no static rewrite can fix: a
`callsubr` whose operand is not the number written immediately before it — a
computed one, or one a caller left — and a subroutine read two ways, its
`hintmask` lengths or its calls differing between two states it is entered
in. For those two `subset_cff` writes the subset flattened: each kept glyph's
charstring is what the reader runs, with every call inlined —

- each token the machine reads is written as it was, `hintmask` and
  `cntrmask` with the mask bytes the reader skipped;
- a call whose operand is the number written immediately before it in the
  same body, which the stack took, is written as nothing, and the number goes
  with it; any other call is written as `drop` (12 18), which pops the number
  as the call popped it; a call that finds the stack empty pops nothing and
  ends its body, as the reader's does, and is written as nothing;
- `return` is written as nothing; `endchar` is written and ends the glyph;

and every subroutine INDEX empty, `Subrs` gone from every Private DICT, and
the charset, encoding, FDSelect and the DICTs' other entries as renumbering
writes them. Stack depths, stem counts, widths and `random`'s draws are the
original's at every token, so the flattened charstring carries the hints its
glyph executed. A glyph nobody keeps is `endchar`, as today. The writer
outlines and advances every kept glyph in both programs before answering.

**Flattening refuses** (`Unflattened`), by name, the program then written
whole: a kept glyph whose run meets a bound (`StackOverflow`,
`TransientIndex`, `SubroutineDepth`, `Budget`) or calls a missing subroutine;
one that does not end in `endchar`, or reads a token after it; a token cut by
the end of its body; a flattened charstring past appendix B's 65 535 bytes;
and flattened charstrings that together pass the program's own length, since
a subset no smaller than its face is never used (`SubsetNotSmaller`) — which
also bounds what flattening holds by its input (ruling 1).

### Every `ProgramNotRebuildable` cause, before and after

| Today | After the row |
| --- | --- |
| A Type 1 program (`/FontFile`) | subset (FT-11.4); converted when the save asks (FT-11.8); still refused by `Type1Refusal`'s names |
| Bytes that are no sfnt, CFF or Type 1 program | still refused: nothing to rebuild |
| An sfnt missing `glyf`, `loca`, `head` or `maxp`, or carrying neither `glyf` nor `CFF ` | still refused: no outlines to cut |
| A CFF the reader declines, or whose header, Name, Top DICT, String or global subroutine INDEX does not re-read | still refused: unreadable |
| `CharstringType 1` | still refused: another charstring language (*Non-goals*) |
| No CharStrings offset, an empty CharStrings INDEX, an FDArray that does not read | still refused: no glyphs to keep |
| A `callsubr` operand that is not the number written before the call | flattened |
| A subroutine read two ways (two hint states) | flattened |
| A call to a subroutine the INDEX does not hold, or an operand naming none | still refused, `MissingSubroutine`: the new bias would make the same operand name a subroutine that exists, and the reader skips the call while another may not |
| A subroutine that reaches itself in the same state (self-calling) | still refused: the reader stops it at its depth cap and draws what it had, which the program does not say |
| The walk's memo past 2²⁰ states or its 40 000 000-token budget | still refused: resource bounds (ruling 1) |
| A charset, encoding or FDSelect whose length cannot be measured | still refused: malformed table |
| An INDEX past 65 535 items or 32-bit offsets; a Top DICT or layout that disagrees with its own measure | still refused: nothing a font can be written as |
| A kept glyph that outlines differently in the subset | still refused: the writer's own check |
| A flattened glyph that meets a bound, calls a missing subroutine, does not end in `endchar`, or overruns | refused by name (*Flattening*) |

### Type 1 to Type 2

`tinker_pdf_font::type1_to_cff(program)` (FT-11.7) answers a bare CFF — one
font, Type 2 charstrings, `CharstringType` unstated — or a refusal naming the
construct it cannot map:

- **Width and side bearing.** `hsbw sbx wx` gives the glyph's width `wx`,
  written as the first operand of its first stack-clearing operator over a
  `nominalWidthX` and `defaultWidthX` of 0 (5177 §3.1); Type 2 has no side
  bearing (5177 appendix C), so the current point starts at `(sbx, 0)` and
  the first move carries it. `sbw` with a vertical width or a vertical side
  bearing other than 0 is refused: Type 2 has no vertical width.
- **Paths.** `rmoveto`, `hmoveto`, `vmoveto`, `rlineto`, `hlineto`,
  `vlineto`, `rrcurveto`, `hvcurveto` and `vhcurveto` keep their operator
  codes, Type 2's forms of one operand set; `closepath` is dropped, since a
  Type 2 path closes at its next move and at `endchar`.
- **Hints.** Every stem the glyph declares — its own and every hint
  replacement's — is gathered, moved by the side bearing (`vstem`'s `x` is
  relative to `sbx`), sorted and deduplicated, and declared at the start as
  `hstemhm`/`vstemhm` with a `hintmask` for each set where the glyph replaces
  hints (chapter 8.1), or as `hstem`/`vstem` where it does not; `hstem3` and
  `vstem3` are three stems; `dotsection` is dropped; past appendix B's 96
  stems is refused.
- **Flex.** The seven points between OtherSubrs 1 and 0 (chapter 8.3) are
  Type 2's `flex` (12 35), its depth the first operand subroutine 0 is
  handed.
- **Accents.** `seac` is written as the base's path and the accent's moved by
  `(adx, ady)` in one charstring, which every Type 2 reader draws, rather than
  as `endchar`'s deprecated four operands (5177 appendix C), which this
  engine's CFF reader does not compose.
- **Subroutines** are inlined, so the CFF carries none and the CFF subsetter
  cuts it.
- **`div`** is Type 2's `div` (12 12) where both operands are numbers Type 2
  writes — an integer within ±32 767 or a 16.16 value; a dividend past that
  (chapter 6.2 lets a Type 1 integer reach 2³¹ before a `div`) is written as
  its quotient where a 16.16 value holds it exactly, and refused otherwise.
- **Refused**: an OtherSubrs entry past 3, a `pop` or `setcurrentpoint`
  outside the flex protocol, and a glyph whose conversion does not outline
  and advance as the program does.

The charset names every charstring, `.notdef` first as 5176 requires, each
name a standard string or one in the String INDEX; the encoding is the
predefined Standard one where the program says `StandardEncoding`, and
otherwise a format 0 or 1 table with a supplement for a glyph two codes name;
the Top DICT carries the program's `FontMatrix` and `FontBBox`. The INDEX and
DICT writers are `cff_subset.rs`'s. Every glyph is outlined and advanced in
both programs before the converter answers.

### When a save converts

Only when asked (FT-11.8): `SaveOptions::type1` takes `Type1Programs::Keep`,
the default — a Type 1 program stays one, cut as `SaveOptions::fonts` says —
or `Type1Programs::Convert`, under which the font pass rewrites a `/FontFile`
program as `/FontFile3 /Subtype /Type1C` through `type1_to_cff`, cuts it with
the CFF subsetter and drops `/Length1` to `/Length3`. A program the
converter refuses is cut as a Type 1 program instead, and `FontPolicy::Keep`
converts nothing, since it writes every program through as it arrived. Opt-in
because a default that rewrote every Type 1 program's format would change
what other readers are handed without being asked.

### Extension

ED-02 adds text to a document whose font is already a subset, and needs the
glyphs the new text draws. `tinker_pdf_font::extend_subset(subset, face,
glyphs)` (FT-11.9) answers the subset with exactly the asked-for glyphs — and
the components a composite among them names — added at their own
identifiers, or a refusal; the facade's call writes it over the program
under a new subset tag. The dictionary entries for new codes stay the
editor's (ED-02).

**The source test**, which a producer's subset that renumbered fails, so that
ED-02 falls back:

- **TrueType**: both carry `glyf`; the same `maxp` glyph count; the
  subset's `cmap`, `hmtx`, `hhea`, `OS/2`, `cvt `, `fpgm` and `prep` byte for
  byte the face's, as this engine's subsetter copies them; and every glyph
  the subset's `glyf` holds byte for byte the face's at the same index.
- **CFF**: the same glyph count, the same charset and String INDEX bytes, the
  same `ROS` or none, the same font matrix per glyph; and every glyph that
  draws in the subset outlining and advancing as the face's at the same
  identifier.
- **Type 1**: the same clear-text portion; the subset's charstring names a
  subset of the face's; and every charstring of the subset outlining and
  advancing as the face's of the same name.

The answer is the face cut to the subset's glyphs and the new ones, which
keeps every old glyph as it was by the test and adds nothing else.

### What is measured

`cff_subset_census.rs` (FT-11.10) also cuts every Type 1 program, every CFF
renumbering refused and flattening takes, and every Type 1 program converted,
counting programs, refusals by reason and bytes before and after, its figures
pinned as floors in the census (ruling 16); MS-04's write group gains a Type 1
subset on save and a conversion with its cut (FT-11.11).

## Milestones

1. **FT-11.2, the Type 1 subsetter.** `type1_subset.rs` and the shared
   scanner and cipher in `type1.rs`; a writer fuzz target. Exit:
   `type1_subset.rs`'s `every_kept_glyph_outlines_as_before`,
   `a_seac_glyph_keeps_its_components`,
   `an_unreached_subroutine_keeps_its_number`,
   `the_same_input_gives_the_same_bytes` and
   `garbage_is_declined_not_panicked_on`; the `type1_subset` fuzz target
   re-parses every output and finds each glyph it asked for.
2. **FT-11.3, one Type 1 selection.** Exit: `substitute_fonts.rs`'s
   `a_type1_glyph_is_chosen_by_its_encoding_name`; `an_embedded_type1_font_draws`
   and every fingerprint unchanged.
3. **FT-11.4, Type 1 on save.** Exit: `subset_breadth.rs`'s
   `a_type1_subset_draws_the_used_glyphs_identically`,
   `the_type1_subset_passes_the_strict_validator` and
   `the_bytes_saved_are_reported`; `determinism.rs` gains the saved
   document's hash.
4. **FT-11.5, the arithmetic.** Exit: `cff.rs`'s
   `each_arithmetic_operator_computes_what_the_specification_says` and
   `a_computed_subroutine_number_calls_that_subroutine`; every CFF test,
   `cff_fonts.rs` and every fingerprint unchanged.
5. **FT-11.6, flattening.** Exit: `cff_subset/tests.rs`'s
   `a_computed_subroutine_number_is_flattened` and
   `a_subroutine_reached_with_two_hint_states_is_flattened`, replacing
   `a_computed_subroutine_number_is_refused`, the refusals above still held;
   `subset_breadth.rs`'s `a_flattened_cff_subset_draws_the_used_glyphs_identically`.
6. **FT-11.7, the converter.** Exit: `type1_to_cff.rs`'s
   `every_glyph_converts_to_the_same_outline_and_advance` over programs using
   `seac`, flex, hint replacement and `div`;
   `the_charset_names_every_glyph_and_the_encoding_maps_every_code`;
   `the_output_parses_and_subsets`; the `type1` fuzz target converts and
   re-parses.
7. **FT-11.8, converting on save.** Exit: `subset_breadth.rs`'s
   `a_converted_type1_font_draws_the_used_glyphs_identically` and
   `the_converted_font_passes_the_strict_validator_and_the_pdfa_pairing`; a
   default save leaves the program Type 1; `determinism.rs` gains the
   converted document's hash.
8. **FT-11.9, extension.** Exit: `subset_extension.rs`'s
   `an_extended_subset_gains_exactly_the_new_glyphs`,
   `a_face_that_is_not_the_source_is_refused` and
   `the_extended_program_is_renamed`; the saved document passes the strict
   validator.
9. **FT-11.10, the census.** Exit: a local run of `cff_subset_census.rs` over
   the fetched corpora prints the new counts, pinned as floors the next run
   holds.
10. **FT-11.11, the speed rows.** Exit: `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree` holds both operations.

## Risks

| Risk | Mitigation |
| --- | --- |
| A reader that does hint replacement calls a subroutine this one never runs | Each `subr#` handed to OtherSubrs 3 is kept with what it calls, and subroutines 0 to 3 always |
| A program's trailer is not 512 zeros and `cleartomark` | It is copied from the first `cleartomark` back over its zeros and forward to the end, whatever it holds; with none, `Length3` is 0, which Table 125 defines |
| The renderer and a conforming reader choose different Type 1 glyphs for a code | The subset keeps both (the selection's third row), and a glyph only this renderer's guess reaches leaves the font whole |
| Running the arithmetic moves a page that draws a CFF computing its operands | It draws what the charstring computes, where today it drew the operand missing; no committed fixture computes, and the corpus renders are the end phase's to read |
| A flattened subset is larger than the face | It is refused before it is written, and `SubsetNotSmaller` would refuse it after |
| `random` here is not another reader's | 5177 fixes no sequence, so a program whose outline depends on it draws differently in every reader; this one is fixed and restarts each glyph, and flattening keeps each draw in its order |
