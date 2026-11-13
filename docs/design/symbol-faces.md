# Symbol and ZapfDingbats when nothing embeds them

*For [ROADMAP](../ROADMAP.md) row FT-03.* Two of the standard 14 fonts
(9.6.2.2) are not text faces: Symbol and ZapfDingbats. A document that names
one of them and embeds no program leaves the reader to supply the glyphs, and
each has a built-in encoding of its own (Annex D.5 and D.6) that maps its
codes to glyph names no text encoding knows. Today a `bundled-fonts` build
declines both by name, and a host face handed over with
`SimpleFontProvider::substituting_symbolic()` is read by its `cmap` alone, the
code at `0xF000` through a (3,0) subtable, so a face built on Unicode draws
nothing for them. This document fixes the two encodings, the route from a
code to a substitute face's glyph by the code's name, the widths a document
that gives none is laid out with, the census the owner rules H-94 with, and
what follows from each of H-94's two answers.

## Scope

- **The two built-in encodings** (FT-03.2): Table D.5's codes and Table D.6's,
  each with its glyph name and the Unicode value the published glyph lists
  give the name, public in `tinker-pdf-font`.
- **Which name a code has**: `/Differences`, a named base encoding and the
  built-in encoding, in that order.
- **The route** (FT-03.3, FT-03.4): from a code's name to a substitute face's
  glyph, through the face's own glyph names, then the name's Unicode value
  through its `cmap`, then today's (3,0) reading, in that order.
- **Precedence**: an embedded program keeps its own encoding and is never
  asked by name, and a document's names win over the built-in ones.
- **Widths** when `/Widths` is absent (FT-03.4).
- **The census** (FT-03.3): the candidate faces, what it measures of each, and
  where its figures go.
- **H-94's two answers**: a face vendored and answered by the bundled set
  (FT-03.5), or a row of first-party outlines (FT-03.6).

## Non-goals

- **Text extraction of these fonts.** The Unicode values could serve it; what
  a Symbol or ZapfDingbats code extracts as stays what it is.
- **An embedded Symbol or ZapfDingbats program.** It draws by its own
  encoding, as every embedded program does (9.6.6).
- **Any other symbolic font.** Wingdings, a symbolic TrueType that embeds
  nothing, a font whose descriptor sets the symbolic flag: `family_for` keeps
  declining each, and the route is asked only for a font
  `Standard14::from_base_font` names Symbol or ZapfDingbats.
- **Glyphs the tables do not encode.** A program may carry glyphs Annex D's
  tables give no code; one is reached only by a name a `/Differences` array
  gives it.
- **Outlines drawn by this engine.** That is the row H-94's second answer
  opens (FT-03.6), not this one.
- **Choosing a face.** The census measures; admissibility is H-94's.

## Design

### The two built-in encodings

Annex D.5 and D.6 give the two fonts' character sets and built-in encodings,
one row a character: its picture, its glyph name and its code in octal. Both
tables are the same in ISO 32000-1 and ISO 32000-2, and they are read from the
second with this repository's own `tpdf text`, twice and independently — once
in reading order, once by the cells' positions on the page, each name taken
with the code beside it on its row — and the two readings agree on every
pair.

| Table | Font | Codes | Where they are | Ordered by |
| --- | --- | ---: | --- | --- |
| D.5 | Symbol | 189 | 32 to 126 and 160 to 254, but 240 | name |
| D.6 | ZapfDingbats | 188 | 32 to 126, 161 to 239 and 241 to 254 | code |

No code is listed twice and no name is listed twice in either table.

**The Unicode value of a name** is the published glyph list's, never the
table's picture. Symbol's names and ZapfDingbats' `space` take the Adobe Glyph
List's value (`glyphlist.txt`, table version 2.0); ZapfDingbats' `a`-names
take the ITC Zapf Dingbats Glyph List's (`zapfdingbats.txt`, table version
2.0). Each list gives each of these names exactly one value. The Dingbats list
also names fourteen glyphs Table D.6 gives no code (`a85` to `a96`, `a205`,
`a206`); they keep their values, since a `/Differences` array can name them.
Some of the values are in the private-use area (`radicalex`, `arrowvertex`,
`arrowhorizex`, the sans and serif `copyright`, `registered` and `trademark`,
and the pieces of the large parentheses, brackets, braces and integral), and
the table's pictures read back as characters elsewhere for those and for the
two phis (*Risks*).

```rust
pub enum BuiltInEncoding { Symbol, ZapfDingbats }

pub struct BuiltInCode { pub code: u8, pub name: &'static str, pub unicode: char }

impl BuiltInEncoding {
    pub fn codes(self) -> &'static [BuiltInCode];            // ascending by code
    pub fn glyph_name(self, code: u8) -> Option<&'static str>;
    pub fn unicode_of(self, name: &str) -> Option<char>;     // this font's list
}
```

in `crates/tinker-pdf-font/src/encoding.rs`, beside the base encodings, and
exported from the crate root.

### Which name a code has

A simple font whose `/BaseFont` `Standard14::from_base_font` reads as Symbol
or ZapfDingbats gives a code its glyph name, first match wins:

1. the name `/Differences` gives the code;
2. the name the base encoding the `/Encoding` entry names gives it, because
   the encoding dictionary's `/BaseEncoding` (9.6.6.1) makes a symbolic
   font's built-in encoding its base only where the dictionary names none;
3. the name the font's built-in encoding gives it, Table D.5 or D.6;
4. none: the code is undefined.

Steps 1 and 2 are `cos::Font::glyph_name` as it stands, whose `None` already
means the document named nothing; step 3 is `BuiltInEncoding::glyph_name`.

### The route: a name to a substitute face's glyph

A substitute face is the TrueType, OpenType or bare CFF program a
`FontProvider`, or the bundled set, hands over for the font. Its glyph for a
code is the first of these that reaches a glyph other than glyph 0:

1. **The face's own glyph names.** A `glyf` face's `post` table: version 1.0
   is the 258 standard Macintosh names in their order; version 2.0's index is
   one of those 258 below 258 and one of the table's own strings from 258 on;
   version 2.5 is each glyph's offset into the standard order; version 3.0
   names nothing. An OpenType CFF face's or a bare CFF program's names are its
   charset's. Asked for the code's name.
2. **The name's Unicode value through the face's Unicode subtable**: (3,10),
   else (3,1), else a platform 0 subtable of encoding 0 to 4 or 6 (encoding 5
   maps variation sequences, not characters). The value is
   `BuiltInEncoding::unicode_of`, else `glyph_name_to_char`'s, which reads the
   Glyph List's algorithmic `uniXXXX` and `uXXXX` forms and the common names;
   a name neither knows has none.
3. **The code through a (3,0) symbol subtable, at `0xF000` plus the code**:
   today's reading, kept last, so a face that reaches a code only that way —
   the face that draws a Symbol page today — still reaches it.
4. **Nothing**: the code draws nothing and is reported, as an unresolved glyph
   is today.

A code with no name skips the first two steps. Glyph 0 is `.notdef` in every
face, so it is not an answer at any step, and a name that reaches it falls to
the next step.

The font half lives in `tinker-pdf-font`, beside the census (FT-03.3) that
counts with it, so the census's figures are the drawing's:

```rust
impl Sfnt<'_> {
    pub fn glyph_for_name(&self, name: &str) -> Option<u16>;   // post 1.0, 2.0, 2.5
    pub fn glyph_for_unicode(&self, c: char) -> Option<u16>;   // (3,10), (3,1), (0, x)
    pub fn glyph_for_symbol_code(&self, code: u8) -> Option<u16>; // (3,0) at 0xF000 + code
}

pub enum SymbolRoute { Name, Unicode, SymbolCode }

pub fn substitute_glyph(
    program: &[u8],
    encoding: BuiltInEncoding,
    name: Option<&str>,
    code: u8,
) -> Option<(u16, SymbolRoute)>;
```

`Sfnt::glyph_for_name` reads every version of `post` over the standard
order, Apple's, from the TrueType Reference Manual's `post` chapter, so a
name a face stores by its index — `space`, `Delta`, `summation` — reaches
its glyph as one the table spells out does. FT-03.4 calls `substitute_glyph`
from the facade's substitute addressing in `resources.rs`.

### Precedence: the document's program and the document's names

- **An embedded program keeps its own encoding.** A font whose descriptor
  carries `/FontFile`, `/FontFile2` or `/FontFile3` is drawn as today —
  `sfnt_selection`, `cff_selection` and the Type 1 branch — and the route is
  never asked: `program_for` returns the embedded program before it asks a
  provider, and only a program a provider or the bundled set supplied takes
  the route.
- **`/Differences` and a named base encoding win** over the built-in
  encoding, at the name (*Which name a code has*), so a document that renames
  a code renames what it draws and what it is laid out with.

### Widths when `/Widths` is absent

A Symbol or ZapfDingbats font may omit `/Widths` (9.6.2.2), and a document
that does was laid out against the standard metrics. So a code's width is the
advance Adobe's published Core 14 metrics (`Symbol.afm`, `ZapfDingbats.afm`)
give **the code's name**, in 1/1000 em, taken as `base14.rs` takes the other
twelve faces': the same name the route draws, so a `/Differences` name moves
its width with it. A name the metrics do not list keeps today's 600, reported
as inexact. The substitute face's own advances are not used: they are
another face's. FT-03.4 transcribes the two tables into `base14.rs`, read by
name, and `cos::Font::width_of` asks them through the code's name.

### The census

`crates/tinker-pdf-font/tests/symbol_census.rs`, `#[ignore]`d,
`symbol_face_census`: the price H-94 rules with. It reads each candidate below
from the directory `TINKER_SYMBOL_FACES` names, prints `RAN` and a
tab-separated row per candidate, or `SKIPPED` when the variable is unset or
names no directory. Candidates are fetched once to be measured and never
committed (ruling 18).

For each candidate file it states:

- its bytes;
- for each of the two encodings, the codes whose built-in name reaches a glyph
  by the route, of 189 and of 188, split by the step that reached each;
- the licence its `name` table states (name ID 13, and the URL of ID 14);
- the licence its upstream declares, with its SPDX expression, and whether
  every identifier the expression needs is on `deny.toml`'s `[licenses]`
  allowlist, which the census reads;

and a last row counting the codes any admitted candidate reaches, since a
bundled answer could pair two faces.

| File | Face | Upstream | Licence the upstream declares | SPDX |
| --- | --- | --- | --- | --- |
| `StandardSymbolsPS.ttf`, `StandardSymbolsPS.otf` | URW++ Standard Symbols PS | <https://github.com/ArtifexSoftware/urw-base35-fonts>, tag `20200910`, `fonts/` | GNU Affero GPL version 3, with an exception for including the fonts in a PostScript or PDF document | `AGPL-3.0-only`; the exception has no identifier |
| `D050000L.ttf`, `D050000L.otf` | URW++ D050000L | <https://github.com/ArtifexSoftware/urw-base35-fonts>, tag `20200910`, `fonts/` | GNU Affero GPL version 3, with an exception for including the fonts in a PostScript or PDF document | `AGPL-3.0-only`; the exception has no identifier |
| `NotoSansSymbols-Regular.ttf` | Noto Sans Symbols | <https://github.com/notofonts/symbols>, release `NotoSansSymbols-v2.003`, its unhinted TrueType | SIL Open Font License 1.1 | `OFL-1.1` |
| `NotoSansSymbols2-Regular.ttf` | Noto Sans Symbols 2 | the same repository, release `NotoSansSymbols2-v2.008`, its unhinted TrueType | SIL Open Font License 1.1 | `OFL-1.1` |
| `NotoSansMath-Regular.ttf` | Noto Sans Math | <https://github.com/notofonts/math>, release `NotoSansMath-v3.000`, its unhinted TrueType | SIL Open Font License 1.1 | `OFL-1.1` |
| `STIXTwoMath-Regular.otf` | STIX Two Math | <https://github.com/stipub/stixfonts>, release `v2.13`, `fonts/static_otf/` | SIL Open Font License 1.1 | `OFL-1.1` |
| `DejaVuSans.ttf` | DejaVu Sans | <https://github.com/dejavu-fonts/dejavu-fonts>, release `version_2_37`, `dejavu-fonts-ttf-2.37` | the Bitstream Vera Fonts licence, with DejaVu's changes in the public domain and the Arev Fonts licence for the glyphs taken from them | `Bitstream-Vera`; the Arev licence has no identifier |
| `FreeSerif.otf` | GNU FreeFont FreeSerif | <https://ftp.gnu.org/gnu/freefont/>, `freefont-otf-20120503` | GNU GPL version 3 or later, with the font exception | `GPL-3.0-or-later WITH Font-exception-2.0` |

**Where the figures go** (ruling 16): the rows it prints are committed, when
it runs, as `crates/tinker-pdf-font/tests/symbol_census.tsv` beside it, and
the run's revision and day go in that commit's message:

```text
TINKER_SYMBOL_FACES=<directory> cargo test -p tinker-pdf-font --test symbol_census -- --ignored --nocapture
```

### H-94's two answers

- **A face is admitted** (FT-03.5): it is vendored under
  `crates/tinker-pdf-font/data/symbol` with its licence file and its
  `THIRDPARTY.md` row and section; `BundledFonts` answers a request
  `Standard14::from_base_font` reads as Symbol or ZapfDingbats with it, drawn
  by the route, while `family_for` keeps declining every other symbolic name;
  the refusal row in `features/fonts.md` goes. The bytes it adds are recorded
  for H-04, and a figure MS-06's ratchet must raise past its tolerance is
  H-78's to approve before it lands.
- **No face is admitted** (FT-03.6): first-party outlines for the 189 Symbol
  and 188 ZapfDingbats glyphs the tables encode become a row of the fonts tier,
  with this document as its design doc. The row starts from the census, which
  says what no admissible face reaches, and from the route: a face this engine
  draws names its glyphs as the tables do, so the first step reaches every
  code.

## Milestones

1. **FT-03.2, the encodings.** `BuiltInEncoding` and `BuiltInCode` in
   `encoding.rs`, exported from the crate root, and the glyph lists'
   provenance in `THIRDPARTY.md`. Exit: `encoding.rs`'s
   `the_symbol_built_in_encoding_names_189_codes` and
   `the_dingbats_built_in_encoding_names_188_codes` — the counts, each code
   once, and every pair and value equal to a second transcription in the test
   keyed by name, where the source is keyed by code — in the default and
   `bundled-fonts` builds.
2. **FT-03.3, the route's font half and the census.** `Sfnt::glyph_for_name`
   over the standard order, `glyph_for_unicode`, `glyph_for_symbol_code` and
   `substitute_glyph`. Exit: `sfnt.rs`'s
   `a_post_name_in_the_standard_order_reaches_its_glyph`,
   `each_post_version_names_as_it_states` and
   `the_unicode_and_symbol_subtables_are_asked_apart`; `symbolic.rs`'s
   `the_route_takes_its_steps_in_order`,
   `a_name_reaching_notdef_falls_to_the_next_step` and
   `a_bare_cff_is_asked_by_its_charset`; `symbol_census.rs`'s
   `symbol_face_census`, which prints `RAN` with a row per candidate or
   `SKIPPED`, its figures taken when the owner rules H-94.
3. **FT-03.4, the facade.** `resources.rs` draws a substituted Symbol or
   ZapfDingbats font through `substitute_glyph`, and `base14.rs` lays out a
   code with no `/Widths` by its name's advance. Exit: `substitute_fonts.rs`'s
   `every_code_of_both_built_in_encodings_draws_its_named_glyph` (a host face
   naming all 377 glyphs the way the route reads them, each a box of its own
   size, inks each code's named box),
   `a_differences_name_wins_over_the_built_in_encoding`,
   `an_embedded_symbol_program_keeps_its_own_encoding`,
   `a_code_with_no_widths_takes_its_names_advance`; `base14.rs`'s
   `the_symbolic_faces_advances_are_read_by_name`; `determinism.rs` pins the
   page, reproduced under `wasm32-wasip1` before it is committed.
4. **FT-03.5, on H-94's first answer.** Exit: `substitute_fonts.rs`'s
   `a_bundled_build_draws_every_code_of_both_built_in_encodings` (every code
   draws the glyph the face's own tables give its name, read in the test, with
   no `UnreadableFont`); `bundled.rs`'s
   `the_two_symbolic_standard_faces_are_declined_by_name` becomes the test that
   they are answered and Wingdings still declined; `cargo xtask vendor`
   passes.
5. **FT-03.6, on H-94's second answer.** Exit: the new row stands in the fonts
   tier with its item, exit criterion, doc and steps; `features/fonts.md`'s
   refusal row names it; this document records that the row starts from the
   census and the route.

## Risks

| Risk | Mitigation |
| --- | --- |
| A Unicode face has no glyph at the private-use values the Glyph List gives Symbol's bracket pieces, sans and serif marks and extenders | The face's own names come first, so a face that names `parenlefttp` reaches it; the census counts what each candidate misses, split by step, before H-94 rules |
| Table D.5's pictures read back as U+03D5 for `phi` and U+03C6 for `phi1`, the Glyph List's values the other way round | The route follows the published list, as the step that transcribes it states; a face draws Symbol's two phis as its own U+03C6 and U+03D5, and FT-03.4's test reads the list, not a picture |
| A host face that draws today through its (3,0) subtable is read another way | The (3,0) reading stays, last; a name or a value reaches a glyph first only where the face states one for it |
| A ZapfDingbats document uses codes 128 to 141, which Table D.6 does not encode | Such a code has no name, skips the first two steps and keeps today's (3,0) reading; a `/Differences` name for it takes the route |
| The Core 14 metrics' licence is not on `deny.toml`'s allowlist | The advances are transcribed as `base14.rs`'s twelve tables are, the files are not vendored, and `THIRDPARTY.md` records where they were read; if the owner reads ruling 18 as covering such a table, the two fonts keep today's 600 |
| The census and the drawing disagree | The census counts through `substitute_glyph`, the function FT-03.4 draws through |
