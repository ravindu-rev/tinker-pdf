# Hyphenation

*For [ROADMAP](../ROADMAP.md) row FT-15.* `hyphens: auto` asks the layout to
break a word where a hyphenation resource for its language says it may, and
this build reads `none` and `manual` only: a soft hyphen (U+00AD) is a break
that draws a hyphen, and `auto` is refused by value
([features/epub.md](../features/epub.md)). This design adds the resource —
Liang's patterns and exception lists, one compiled table a language — the
content language that keys it, and the seam where its points join the line
breaker, for EPUB, HTML and the layout API alike.

## Scope

- **`hyphens: auto`**, `css-text-3` §5.3, read from the clause:
  - *"Words may be broken at hyphenation opportunities determined
    automatically by a language-appropriate hyphenation resource in addition
    to those indicated explicitly by a conditional hyphen."* A word set in
    `auto` gains its dictionary's points as break opportunities; its soft
    hyphens stay what `manual` makes them.
  - *"Automatic hyphenation opportunities elsewhere within a word must be
    ignored if the word contains a conditional hyphen ... in favor of the
    conditional hyphen(s)."* A word that holds a soft hyphen takes no
    automatic point. The clause's next sentence lets an automatic point be
    used where a part of such a word is still too long for its line; that is
    a *may*, and this build does not take it: such a word breaks at its soft
    hyphens, and past them as `overflow-wrap` says.
  - *"The UA must therefore only automatically hyphenate text for which the
    content language is known and for which it has an appropriate
    hyphenation resource."* The language is the element's content language,
    the nearest `xml:lang` or `lang` (`selectors-4` §6.5.1); text with none,
    or with one this build has no table for, is set as `manual` and counted.
  - *"Inline element boundaries and out-of-flow elements must be ignored
    when determining word boundaries."* Words are found over the inline
    formatting context's text, which `flow.rs` already holds as one string
    across its pieces.
  - *"Hyphenation opportunities are considered when calculating min-content
    intrinsic sizes."* The points join the one opportunity list every line
    of a context is broken over, the min-content measurement's included.
  - *"Hyphenation is a rendering effect only."* The text stays the book's:
    an automatic point adds no character to a run, and the hyphen is drawn
    after the run whose line breaks there, as a soft hyphen's is.
- **The dictionaries**: every language whose published dictionary carries a
  licence `deny.toml` admits (ruling 18) and publishes hyphenated words its
  patterns can be held to (the table in *The dictionaries* below), vendored
  and compiled into static tables behind a cargo feature.
- **The content language** as a computed value: `ComputedStyle` carries
  each element's, `style::consume` reads it into every `Piece`, and CD-14's
  language-conditional casings and FT-02.5's language-keyed faces read the
  same field.
- **Counting**: an element set in `auto` whose language this build has no
  table for is counted against `hyphens`, by element, in every build.

## Non-goals

- **Spelling changes at a break.** §5.3's own examples — Dutch `cafeetje`
  becoming `café‐tje`, Hungarian `Összeg` becoming `Ösz‐szeg` — rewrite the
  word, and Liang's patterns say only where a word may break. The hyphen is
  inserted before the break and nothing after it.
- **`css-text-4`'s hyphenation properties** — `hyphenate-character`,
  `hyphenate-limit-chars`, `-zone`, `-lines`, `-last` — are not parsed. The
  hyphen is U+002D, the hyphen every face has and the one `manual` draws; a
  word's shortest fragments are its dictionary's minimums.
- **Choosing among points by quality.** The line breaker takes the last
  opportunity that fits, as it does for every opportunity; a paragraph-wide
  optimisation that would trade a hyphen for a looser line is not this row.
- **Heuristics that spare proper nouns.** §5.3 allows a user agent to exclude
  words by their capitalisation; this build excludes none, so a capitalised
  word is hyphenated as its lowercase is.
- **A host's own dictionary.** `hyphenate::Dictionary::parse` reads a pattern
  file at run time, and nothing in `Options` hands one to a layout: the
  tables the build carries are the ones a layout asks.
- **Another hyphenator as an oracle.** No other hyphenator's output is an
  expected answer (ruling 13). The points a test expects are the dictionary
  author's, written in the vendored file, or worked by hand in the test from
  Liang's rule.
- **A language whose licence the allowlist does not admit**, or **whose
  dictionary publishes no hyphenated words**: it is not vendored, its text
  is set as `manual`, and its elements are counted. Each is named in the
  table below with the reason.

## Design

### The content language's path

`tinker_pdf_css::Element::language` already answers which language an
element **declares**: the facade's XHTML adapter returns `xml:lang` before
`lang`, since an EPUB content document is XML, and an empty declaration as
itself, since `lang=""` says the language is unknown. The cascade already
walks the tree for the nearest declaring element (`language_of`, for
`casing_needs_language`). That walk becomes a computed value:

- `ComputedStyle::language: Option<Language>` — the nearest declaration's tag,
  or `None` where no element up to the root declares one or the nearest
  declares the empty string. It is not a CSS property: no declaration sets
  it, and `inherit`, `initial`, `unset` and the `all` shorthand do not touch
  it.
- It is assigned **after** the memo has given an element its style: the
  memo's key is the parent's style number, the winners and the root's font
  size, and two elements that share all three may still speak two
  languages. The generated boxes `::before`, `::after` and `::first-letter`
  take their originating element's, for the same reason and §12.1's.
- `ComputedStyle::inherit_from` copies it, so an anonymous box — a text
  node's inline box, a table's wrapper — speaks its parent's language.
- `Language` is an immutable, shared tag as the document wrote it, behind
  one reference-counted pointer: `Option<Language>` is eight bytes, the tag
  is allocated once for each element that declares one, and every element
  that inherits it shares that allocation. A copy is a reference-count
  increment. `Consumed` carries the same field, which is eight bytes in the
  frame of each `flow::Builder::block` recursion the depth cap is measured
  against.
- Tags are compared ASCII-case-insensitively wherever they are matched, as
  RFC 5646 §2.1.1 makes them, and kept as written.

`style::consume` reads the field into `Consumed::language`, so every `Piece`
of an inline formatting context carries its element's language.

### Liang's algorithm

From F. M. Liang, *Word Hy-phen-a-tion by Com-put-er*, Stanford University
report STAN-CS-83-977 (1983), the publication that defines it:

- A **pattern** is a string of letters with a digit, zero where none is
  written, before, between and after them; `.` stands for the beginning or
  the end of a word. A digit is a level: odd levels allow a break there and
  even ones inhibit it, each level an exception to the one below.
- A word is hyphenated by padding it as `.word.` and matching every pattern
  at every position. *"If more than one hyphenation value is specified for a
  given intercharacter position, then the higher value takes priority ...
  If the final value is odd, the position is an allowable hyphen point."*
  The thesis's own example is the one FT-15.3's test works by hand:
  `hy3ph he2n hena4 hen5at 1na n2at 1tio 2io` over `hyphenation` give
  `hy3phe2n5a4t2ion`, so `hy-phen-ation`.
- An **exception** is a whole word with its points marked by hyphens; a word
  in the exception list takes its points from there and not from the
  patterns.
- A **minimum** keeps a point away from a word's edges: a point after `i` of
  a word's `n` letters needs `i ≥ left` and `n − i ≥ right`. Each dictionary
  states its typesetting minimums in its header (`hyphenmins`, `typesetting`,
  `left` and `right`), and they bind every point, an exception's as well, since
  a minimum is a rule about every word and an exception is a word.

### Words

The hyphenator is asked about words, found over the context's text:

- A word starts at a character of a piece set in `auto` whose language has a
  table, and that table — the **word's dictionary** — decides what its
  letters are: a character the dictionary knows, after folding, or one of
  General_Category L or N, or a combining mark (Mn or Mc), or a soft hyphen.
  The first character that is none of these ends it. Inline boxes do not end
  a word.
- **Folding**: a character is a letter of the dictionary as itself; failing
  that, as its lowercase where the vendored UCD maps it to one character;
  failing that, U+2019 is U+0027 where the dictionary has U+0027 and not
  U+2019, which is how the dictionaries that carry an apostrophe write it.
- A word gets **no points** when it holds a soft hyphen (§5.3), when any of
  its characters is not a letter of its dictionary after folding (a word half
  in another alphabet is not this dictionary's to break), when it is longer
  than [`MAX_HYPHENATED_WORD`](../../crates/tinker-pdf-layout/src/limits.rs),
  64 characters, or when it is shorter than its dictionary's two minimums
  together.
- A point is kept only where the character before it is in a piece set in
  `auto` whose language has the same table — a word that runs from an `auto`
  piece into a `manual` one breaks only in the first — and where the
  character after it is not a combining mark (General_Category Mn or Mc), so
  no point parts a letter from its mark.

### The table format

One trie a dictionary, its patterns and its exceptions in it together, laid
out in four arrays of bytes, multi-byte values little-endian, so every
target reads the same bytes:

- **Symbols.** `0` is the word boundary `.`, `1` starts an exception, and `2`
  onwards are the dictionary's letters in code-point order, one byte each: a
  dictionary of more than 254 letters is refused when it is compiled.
- **`alphabet`**: each letter's code point, four bytes, sorted, so a letter's
  symbol is found by a binary search.
- **`nodes`**: seven bytes a node — the symbol on the edge into it, the index
  of its first child as four bytes, and its digit vector as two (zero for
  none, otherwise the vector's index plus one) — in breadth-first order with
  each node's children sorted by symbol, and one sentinel record after the
  last. A node's children are therefore the nodes from its first child up to
  the next node's, and are found by a binary search on their symbols.
- **`offsets`** and **`values`**: each digit vector once, however many nodes
  share it — a node of depth `k` holds `k + 1` digits — its start in `values`
  as four bytes in `offsets`, which ends with the length of `values`.
- A **pattern** is the path of its symbols from the root, its digit vector on
  its last node. An **exception** is the path from the root through symbol
  `1` and then its letters, its vector on its last node holding a digit for
  each gap of the word, `1` at each of its points. No pattern starts with
  symbol `1`, so a pattern's walk never enters an exception's path.
- **Looking a word up** walks symbol `1` and the word once, for an exception,
  and otherwise walks from each of the `n + 2` positions of `.word.`, raising
  each gap to the maximum it is given. A walk from one position follows at
  most the longest pattern's symbols, so a word costs at most
  `(n + 2) × longest + n + 1` steps.
- **What a table adds** is the four arrays: four bytes a letter, seven a node
  and seven for the sentinel, four a vector and four more, and a byte a digit.
  Each vendored table's figure, and the counts it is made of, are committed in
  `crates/tinker-pdf-layout/tests/hyphenation_tables.tsv` (ruling 16), which
  FT-15.4's test holds to the tables the build compiles.

### The reader and the build-time compiler

`crates/tinker-pdf-layout/src/hyphenate/compile.rs` reads a pattern file and
builds the four arrays, and `build.rs` includes the same file by path, so the
reader a test parses with at run time is the one the build compiles with:

- `%` begins a comment that runs to the end of its line. One or more
  `\patterns{…}` blocks hold whitespace-separated patterns and any
  `\hyphenation{…}` blocks the exceptions; anything else outside a comment is
  refused.
- A pattern is refused when it has two digits in a row, a `.` anywhere but
  first or last, no letter, or the letters of a pattern already read; an
  exception when a hyphen starts it, ends it or follows another, when it
  holds a digit or a `.`, or when its word was listed already.
- The header's `hyphenmins` block gives the minimums from its `typesetting`
  entry; a vendored file without one fails the build.
- Each vendored file is a tree of its own,
  `crates/tinker-pdf-layout/data/hyphenation-<tag>/hyph-<tag>.tex` with its
  `LICENSE`, since each language's dictionary is its own authors' under its
  own licence. Under the cargo feature, `build.rs` compiles each, writes its
  four arrays to `OUT_DIR` and a registry that includes them with
  `include_bytes!`. Nothing under `data/` is opened at run time, and without
  the feature the dictionaries are neither compiled nor carried.
- A table is found for a language by RFC 4647 §3.4's lookup: the tag is
  compared with each table's, ASCII-case-insensitively, and truncated from
  the end, a single-letter subtag going with the subtag after it, until one
  matches or nothing is left. `en-GB-oxendict` finds `en-GB`'s table and
  `pt-BR` finds `pt`'s; `en` and `en-US` find none.

### The cargo feature

`hyphenation`, declared on `tinker-pdf-layout` and forwarded by the facade's
own `hyphenation`, **off by default**: ruling 19 puts locale data a capability
needs beyond the default wasm profile in an opt-in profile, and the default
build's footprint does not move. A build without it has no table, sets
`auto` as `manual`, and counts every element set in `auto`.

### Joining the line breaker

`flow::Builder::lines` holds a context's text as one string and its break
opportunities as one sorted list, and the points join it there, once per
context:

1. UAX #14's opportunities are found and `hyphens: none` takes its soft
   hyphens' away, as now.
2. The words of the pieces set in `auto` with a table are found in one pass,
   their letters are charged to the layout budget's break work beside the
   characters `spend_breaks` already charges, and each word is looked up
   once.
3. Each point that survives *Words* and is not already an opportunity is
   added to the list, and kept in a sorted list of automatic points.
4. **One test replaces `hyphen_shown` and the `ends_with(SOFT_HYPHEN)` in
   `line`**: a line that ends at a break ends in a hyphen when the character
   before the break is a soft hyphen in a piece not set in `none`, or the
   break is an automatic point. `fit` reserves the hyphen's width where the
   test holds, `line` measures the run that ends there with the hyphen after
   it and flags `TextRun::hyphenated`, and `paint::hyphenate` draws it — so a
   soft hyphen's break and an automatic one are one path from here on. The
   hyphen is measured in the style of the piece the character before the
   break is in, which for a soft hyphen is the soft hyphen's.

`white-space` that does not wrap keeps its text whole: `fit` passes over every
opportunity in such a piece, an automatic one included, and its words are not
looked up. A min-content measurement lays the context out at zero width
through the same path, so its words break at their points there too.

### Counted where no table covers the language

The facade counts `hyphens` as `UnimplementedProperty`, by element, where it
counts `font-kerning` against a face it does not shape: every element whose
text a page draws — a painted, non-generated run's anchor — whose computed
`hyphens` is `auto` and whose language finds no table in this build. That is
an element with no language, one whose language has no dictionary here, and,
in a build without the feature, every one. Text set in `none` or `manual`
asks for no table and is not counted.

### The dictionaries

Every dictionary is a file of the published pattern collection
[THIRDPARTY.md](../../THIRDPARTY.md) records, one file a language,
`hyph-<tag>.tex`, at the commit it pins. *Licence* is the file's own header
as an SPDX expression (`+` for *or later*, and an LPPL the file gives as 1.3
as that version's first release, `LPPL-1.3a`); *admitted* is whether
`deny.toml`'s allowlist admits it today, and under which identifier it would
be taken; *published words* is whether the file publishes hyphenated words —
its exception list is the only list of them any of these files carries — and
a language with none is one this row cannot close. A language is vendored
where it is admitted and publishes words; each other language's text is set
as `manual` and counted, for the reason its row gives.

| Tag | Language | Licence | Admitted | Published words | Vendored |
| --- | --- | --- | --- | --- | --- |
| `af` | Afrikaans | `MIT` | yes, `MIT` | its exception list | **yes**, for `af` |
| `ar` | Arabic | `MIT` | yes, `MIT` | none, and no patterns: a script the collection does not hyphenate | no |
| `as` | Assamese | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `be` | Belarusian | `MIT` | yes, `MIT` | none | no: no words |
| `bg` | Bulgarian | a permissive text of its own, no SPDX identifier | no | none | no: licence |
| `bn` | Bengali | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `ca` | Catalan | `LPPL-1.0+` | no | its exception list | no: licence |
| `cop` | Coptic | `MIT` | yes, `MIT` | none | no: no words |
| `cs` | Czech | `GPL-2.0-or-later` | no | its exception list | no: licence |
| `cu` | Church Slavonic | `MIT` | yes, `MIT` | its exception list | **yes**, for `cu` |
| `cy` | Welsh | `LPPL-1.0+ OR MIT` | yes, `MIT` | none | no: no words |
| `da` | Danish | `LPPL-1.3a+ OR MIT` | yes, `MIT` | none | no: no words |
| `de-1901` | German, traditional spelling | `MIT` | yes, `MIT` | none | no: no words |
| `de-1996` | German, reformed spelling | `MIT` | yes, `MIT` | none | no: no words |
| `de-ch-1901` | German, traditional Swiss spelling | `MIT` | yes, `MIT` | none | no: no words |
| `el-monoton` | Modern Greek, monotonic | `LPPL OR MIT`, no LPPL version | yes, `MIT` | none | no: no words |
| `el-polyton` | Modern Greek, polytonic | `LPPL OR MIT`, no LPPL version | yes, `MIT` | none | no: no words |
| `en-gb` | English, British spelling | `MIT` | yes, `MIT` | its exception list | **yes**, for `en-GB` |
| `en-us` | English, American spelling | `FSFAP` | no | its exception list | no: licence |
| `eo` | Esperanto | `LPPL-1.0+` | no | none | no: licence |
| `es` | Spanish | `MIT` | yes, `MIT` | none | no: no words |
| `et` | Estonian | `MIT OR LPPL-1.3a+` | yes, `MIT` | none | no: no words |
| `eu` | Basque | a data-files text of its own, no SPDX identifier | no | none | no: licence |
| `fa` | Persian | `MIT` | yes, `MIT` | none, and no patterns | no |
| `fi` | Finnish | *"Patterns may be freely distributed"*, no SPDX identifier | no | none | no: licence |
| `fi-x-school` | Finnish, school hyphenation | `MIT` | yes, `MIT` | none | no: no words |
| `fr` | French | `MIT` | yes, `MIT` | none | no: no words |
| `fur` | Friulan | `MIT OR LPPL-1.3a+` | yes, `MIT` | none | no: no words |
| `ga` | Irish | `GPL-2.0-or-later OR MIT` | yes, `MIT` | its exception list | **yes**, for `ga` |
| `gl` | Galician | `Unlicense` | yes, `Unlicense` | none | no: no words |
| `grc` | Ancient Greek | `LPPL OR MIT`, no LPPL version | yes, `MIT` | none | no: no words |
| `grc-x-ibycus` | Ancient Greek, a legacy encoding | `LPPL`, no version | no | none, and no patterns | no |
| `gu` | Gujarati | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `he` | Hebrew | `MIT` | yes, `MIT` | none, and no patterns | no |
| `hi` | Hindi | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `hr` | Croatian | `LPPL-1.0+` | no | none | no: licence |
| `hsb` | Upper Sorbian | `LPPL-1.3a+` | no | its exception list | no: licence |
| `hu` | Hungarian | `MPL-1.1 OR GPL-2.0-only OR LGPL-2.1-only` | no | none | no: licence |
| `hy` | Armenian | `LGPL-3.0-only` | no | none | no: licence |
| `ia` | Interlingua | `LPPL-1.3a` | no | its exception list | no: licence |
| `id` | Indonesian | `GPL-2.0-only` | no | its exception list | no: licence |
| `is` | Icelandic | `LPPL-1.2+` | no | none | no: licence |
| `it` | Italian | `LPPL-1.3a+ OR MIT` | yes, `MIT` | none | no: no words |
| `ka` | Georgian | `MIT` | yes, `MIT` | none | no: no words |
| `kk` | Kazakh | *"Public domain"*, no SPDX identifier | no | none | no: licence |
| `kmr` | Northern Kurdish | `LPPL-1.3a` | no | none | no: licence |
| `kn` | Kannada | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `la` | Latin | `MIT OR LPPL-1.0+` | yes, `MIT` | none | no: no words |
| `la-x-classic` | Classical Latin | `MIT` | yes, `MIT` | none | no: no words |
| `la-x-liturgic` | Liturgical Latin | `MIT` | yes, `MIT` | none | no: no words |
| `lt` | Lithuanian | `MIT` | yes, `MIT` | none | no: no words |
| `lv` | Latvian | `LGPL-2.1-only OR GPL-2.0-or-later` | no | none | no: licence |
| `mk` | Macedonian | `GPL`, no version | no | none | no: licence |
| `ml` | Malayalam | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `mn-cyrl` | Mongolian, Cyrillic script | `LPPL-1.3a+ OR MIT` | yes, `MIT` | none | no: no words |
| `mn-cyrl-x-lmc` | Mongolian, an alternative set | none stated | no | none | no: licence |
| `mr` | Marathi | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `mul-ethi` | Languages in the Ethiopic script | `MIT` | yes, `MIT` | none | no: no words |
| `nb` | Norwegian Bokmål | `FSFAP`, its patterns `no`'s | no | its exception list | no: licence |
| `nl` | Dutch | `MIT` | yes, `MIT` | its exception list | **yes**, for `nl` |
| `nn` | Norwegian Nynorsk | `FSFAP`, its patterns `no`'s | no | its exception list | no: licence |
| `no` | Norwegian | `FSFAP` | no | its exception list | no: licence |
| `oc` | Occitan | `MIT OR LPPL-1.0+` | yes, `MIT` | none | no: no words |
| `or` | Odia | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `pa` | Punjabi | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `pi` | Pāli | `MIT` | yes, `MIT` | none | no: no words |
| `pl` | Polish | `MIT`, or a public-domain notice | yes, `MIT` | its exception list | **yes**, for `pl` |
| `pms` | Piedmontese | `MIT` and `LPPL-1.3a+` listed, no choice stated | yes, `MIT` | none | no: no words |
| `pt` | Portuguese | `BSD-3-Clause` | yes, `BSD-3-Clause` | its exception list | **yes**, for `pt` |
| `rm` | Romansh | `MIT OR LPPL-1.3a+` | yes, `MIT` | none | no: no words |
| `ro` | Romanian | none stated | no | none | no: licence |
| `ru` | Russian | `LPPL-1.2+` | no | its exception list | no: licence |
| `sa` | Sanskrit | *"You may freely use, copy, modify and/or distribute this file"*, no SPDX identifier | no | none | no: licence |
| `sh-cyrl` | Serbo-Croatian, Cyrillic script | `LPPL-1.0+` | no | its exception list | no: licence |
| `sh-latn` | Serbo-Croatian, Latin script | `LPPL-1.0+` | no | its exception list | no: licence |
| `sk` | Slovak | `MIT` | yes, `MIT` | its exception list | **yes**, for `sk` |
| `sl` | Slovenian | `LPPL-1.0+ OR MIT` | yes, `MIT` | none | no: no words |
| `sq` | Albanian | `MIT` | yes, `MIT` | none | no: no words |
| `sr-cyrl` | Serbian, Cyrillic script | `GPL`, no version | no | its exception list | no: licence |
| `sv` | Swedish | `LPPL-1.2+` | no | none | no: licence |
| `ta` | Tamil | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `te` | Telugu | `MIT OR LGPL-3.0-or-later OR GPL-3.0-or-later` | yes, `MIT` | none | no: no words |
| `th` | Thai | `LPPL-1.3a+` | no | none | no: licence |
| `tk` | Turkmen | `MIT` | yes, `MIT` | none | no: no words |
| `tr` | Turkish | `LPPL-1.0+` | no | none | no: licence |
| `uk` | Ukrainian | `MIT OR LPPL`, no LPPL version | yes, `MIT` | none | no: no words |
| `vi` | Vietnamese | `MIT` | yes, `MIT` | none, and no patterns | no |
| `zh-latn-pinyin` | Mandarin, pinyin | `MIT` | yes, `MIT` | none | no: no words |

So eight are vendored: `af`, `cu`, `en-gb`, `ga`, `nl`, `pl`, `pt` and `sk`,
each found by the tag in its row. American English is left out for its
licence, so `en` and `en-US` find no table, and German, French, Spanish and
Italian, admitted, publish no words this row could hold them to. Each table's
bytes, patterns, published words, letters, nodes, vectors and longest pattern
are the row of `hyphenation_tables.tsv` with its tag.

## Milestones

1. **FT-15.1, this design**, and `hyphenation_tables.tsv`.
2. **FT-15.2, the content language reaches layout.**
   `crates/tinker-pdf-css/src/tests/cascade.rs`'s
   `the_content_language_is_computed_and_inherited` (`xml:lang` beats `lang`,
   a child without either inherits its parent's, an empty declaration and a
   root with neither give none) and `a_shared_style_keeps_each_elements_own_language`
   (two parents the memo gives one style, in two languages);
   `text_transform_inherits_and_is_counted_where_its_language_conditions_it`
   and `memoised_and_fresh_cascades_agree` still hold, and
   `crates/tinker-pdf-layout/tests/uncascaded_field_does_not_build.rs` still
   refuses a field `consume` does not read.
3. **FT-15.3, the hyphenator.** `crates/tinker-pdf-layout/src/hyphenate.rs`'s
   `points_are_the_odd_maxima_of_every_matching_pattern` (Liang's
   `hyphenation`, worked in the test), `an_exception_replaces_the_patterns_for_its_word`,
   `no_point_falls_inside_the_minimums`, `a_word_past_the_cap_has_no_points`,
   the proptest `any_pattern_file_and_any_word_are_read_without_a_panic`, and
   `a_walk_is_held_to_the_words_length_times_the_longest_pattern` over the
   `cfg(test)` step counter; the reader's own tests beside them.
4. **FT-15.4, the dictionaries.** `crates/tinker-pdf-layout/tests/hyphenation_words.rs`'s
   `each_language_hyphenates_its_dictionarys_published_test_words`, with the
   feature, reading each vendored file's exception list in the test and
   printing a `RAN` line for each language with its word count, and
   `the_compiled_tables_are_the_sizes_the_design_records` against
   `hyphenation_tables.tsv`; without the feature
   `a_build_without_the_feature_has_no_table`; `cargo xtask vendor` and
   `cargo xtask check` with the eight trees declared.
5. **FT-15.5, `auto` in layout.** `crates/tinker-pdf-layout/src/tests.rs`'s
   `hyphens_auto_breaks_a_word_at_its_dictionarys_points`,
   `hyphens_auto_in_a_language_without_a_table_breaks_as_manual`,
   `manual_and_none_take_no_automatic_point`,
   `a_word_holding_a_soft_hyphen_takes_no_automatic_point` and
   `each_word_is_looked_up_once_a_context`, with the feature, and every older
   layout test.
6. **FT-15.6, EPUB and HTML.** `crates/tinker-pdf/tests/epub_reftest.rs`, a
   pair for each vendored language: `auto` in that `lang` draws the lines,
   places and widths of its dictionary's own words that `manual` draws with a
   soft hyphen at each of their points, and the mismatch is `manual` without
   them; `crates/tinker-pdf/tests/html_creation.rs`'s
   `hyphens_auto_is_honoured_in_html`; `crates/tinker-pdf/tests/epub_paint.rs`'s
   `hyphens_auto_without_a_table_is_counted_by_element`, in every build;
   `crates/tinker-pdf-css/src/tests/parser.rs`'s `hyphens_reads_its_three_keywords`.
7. **FT-15.7, the fingerprint.** `crates/tinker-pdf/tests/determinism.rs`'s
   `rendering_is_stable_across_targets` with a page of a generated book set in
   `hyphens: auto` in a vendored language, under the feature, natively and
   under `--target wasm32-wasip1`, the fixture asserting a line ends at an
   automatic point.
8. **FT-15.8, the speed row.** An operation in MS-04's layout group that lays
   out and paginates a generated book set in `hyphens: auto`, with its
   `baseline.json` entry, held by `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree`.

## Risks

| Risk | Mitigation |
| --- | --- |
| The only words these dictionaries publish are their exception lists, so FT-15.4 holds the reader, the trie and the exception path to the authors' words and not the patterns' points | The pattern path is held by FT-15.3's hand-worked examples from Liang's publication, whose arithmetic is in the test; a language whose authors publish words its patterns produce can be held to them where it is vendored |
| Most English books find no table: the American patterns' licence is `FSFAP`, which the allowlist does not admit, and `en` alone is not `en-GB` | Stated here and counted on every such element; taking British patterns for an unqualified `en` would set an American book in another spelling's breaks |
| German, French, Spanish and Italian are admitted and publish no words, so they stay counted | Named in the table; a later row that brings words a reader of the language writes, as H-64's person-written expectations do for shaping, can close them |
| The tables' bytes reach a wasm build | The feature is off by default (ruling 19) and each table's bytes are committed in `hyphenation_tables.tsv`; an opt-in profile's gzipped size is MS-06's to hold |
| `Consumed` grows by the language and `flow::Builder::block`'s frame with it, which the depth cap is measured against | The field is one thin pointer, eight bytes; `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name` is in FT-15.2's exit unchanged |
| A word's text and its letters disagree on case or form: a capitalised word, a typographic apostrophe, a letter written decomposed | Folding maps a letter to its lowercase and U+2019 to U+0027 where the dictionary writes that; a letter the dictionary does not know, a decomposed one included, takes the word's points away rather than guessing them |
| An automatic hyphen is a character the book does not contain | It is drawn and read back as U+002D at a line end, as a soft hyphen's is; extraction's `rejoin_hyphens` joins it by its rule for a hard hyphen before a lower-case letter, and a page's text conservation counts it as one extra character where a soft hyphen's has its source character |
| A hostile book asks for a hyphenation of every word | Each letter looked up is charged to the break budget beside the characters UAX #14 is charged for, the walk is bounded by the word's length times the longest pattern, and a word past `MAX_HYPHENATED_WORD` is not looked up |
