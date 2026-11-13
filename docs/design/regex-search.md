# Regular-expression search: one pass, linear in the text

A page's text searched by pattern. The syntax is the documented linear-time
subset H-17 settled (ruling 23), named exactly below; a match is the
leftmost-first one a backtracking reading would choose; and the matcher is a
Thompson automaton simulated in lockstep over each line's characters, so a
search enters at most the pattern's compiled size in states for each position
of each line it reads, whatever the pattern and the text — `(a*)*b` over a line
of `a`s included. The search runs inside the search options
`TextPage::search_with` takes (`crates/tinker-pdf-content/src/search.rs`): the
same three switches, the same folded forms of the page, the same word
boundaries and the same mapping of a match back to its glyphs. The engine is
in `tinker-pdf-content` beside `search.rs`, which already compiles the UCD
tables its classes need (ROADMAP row FT-20).

## Scope

- **The syntax** (FT-20.2): literals and escapes, `.`, bracket classes with
  ranges and negation, `\d` `\w` `\s` and their negations, `^` `$`, `\b` `\B`,
  alternation, groups, and greedy and lazy quantifiers with braces, read on an
  explicit stack into a public syntax tree; every malformed pattern a typed
  error at its byte offset; caps on a pattern's length, its nesting and its
  repetition counts.
- **The matcher** (FT-20.3): the tree compiled into a Thompson automaton under
  a cap on its size, and simulated in lockstep — every hit of a line found in
  one pass over it, each state entered at most once at each position, and
  every entry counted in MS-01's tally.
- **The search** (FT-20.4): `TextPage::search_pattern` and its counted
  sibling, the pattern folded as the page's text is, whole word as boundary
  assertions around it, and every hit mapped to the glyphs it covers;
  re-exported on the facade, and the syntax in the feature doc.
- **Hostile input** (FT-20.5): a fuzz target over a pattern and a text that
  holds the work bound on every input.
- **Speed** (FT-20.6): an operation in MS-04's text group.

## Non-goals

- **Backreferences and look-around.** No automaton matches either in time
  linear in the text. `\1` to `\9`, `\k` and the four look-around groups are
  refused by name, never read as something else.
- **ECMAScript's `RegExp`.** FJ-01.4's pattern grammar and FJ-02's matcher
  are a separate backtracking engine under step budgets, with ECMAScript's
  syntax and semantics, in `tinker-pdf-script`. No code is shared in either
  direction, and nothing here promises that a pattern means the same in both.
- **A crate of its own.** A leaf crate for the engine would need a fourth
  vendored UCD tree (`crates/tinker-pdf-shape/tests/ucd_version.rs`), and
  `tinker-pdf-content` compiles its tables from the tree it has. No file is
  vendored for this design.
- **Captures and replacement.** A group groups; nothing is captured, and a
  search answers quads, as `search_with` does.
- **Flags inside a pattern.** `(?i)` and its kin are refused. Case, diacritics
  and whole word are `SearchOptions`' switches, and a line holds no line break
  for a multi-line or dot-all flag to change.
- **Unicode case folding, property escapes, POSIX classes and class set
  operations.** Case-insensitive stays `search_with`'s lower-casing, so `ß`
  does not match `SS`. `\p{…}`, `[[:alpha:]]`, `&&` and `--` are not in the
  syntax, and an unescaped `[` inside a class is an error rather than any of
  them.
- **A match across lines.** Each line is searched on its own, as
  `search_with` searches.
- **A faster matcher.** No DFA, no cache of states and no literal prefilter:
  the lockstep simulation is the one matcher, and speed past it is a row of
  its own, measured first.
- **Changing `search` or `search_with`.** Both stay as they are, and the
  parity suite keeps pinning `search`.
- **The command line, the C ABI and the bindings.** None of them searches;
  the pattern search is the Rust API's, beside `search_with`.

## Design

### Where it lives

`crates/tinker-pdf-content/src/regex/`: `mod.rs` holds the public types and
the caps, `parse.rs` the parser (FT-20.2), `compile.rs` the compiler and
`vm.rs` the simulation (FT-20.3). `search.rs` gains the page search (FT-20.4),
and `build.rs` compiles the class tables from the files `data/ucd` already
holds and lists what lower-casing changes (*Folding a pattern*). The facade re-exports `SearchPattern`, `PatternError`,
`PatternErrorKind` and the four caps (ruling 11).

### The syntax

A pattern is a `&str`, so it is Unicode by construction, and every offset in
an error is a byte offset into it.

| Construct | Written | Matches |
| --- | --- | --- |
| A literal | any character but `\ . [ ] ( ) { } * + ? \| ^ $` | itself |
| An escaped character | `\` and any ASCII punctuation character | that character |
| A control escape | `\t` `\n` `\v` `\f` `\r` | U+0009 to U+000D, in that order |
| A code point | `\xHH`, two hex digits; `\x{H…}`, one to six | the Unicode scalar value it names |
| Any character | `.` | any one character |
| A class | `[…]`; `[^…]` | one character among its members; one among none of them |
| A shorthand | `\d` `\w` `\s`; `\D` `\W` `\S` | one character of its table's; one of none of them |
| A line anchor | `^`; `$` | the line's start; its end |
| A word boundary | `\b`; `\B` | a UAX #29 word boundary; any other position |
| Alternation | `x\|y` | `x`, else `y` |
| A group | `(x)`, `(?:x)` | `x`, as one unit |
| Greedy repetition | `x*` `x+` `x?` `x{n}` `x{n,}` `x{n,m}` | `x` any number of times, at least once, at most once, n times, at least n, n to m — as many as can be first |
| Lazy repetition | each of those followed by `?` | the same counts, as few as can be first |

The rules the table leaves:

- **Metacharacters.** The fourteen in the first row are each syntax where they
  stand or an error: a `)`, `]` or `}` with nothing open is an error, and so is
  a `{` that begins no repetition. Each is written as a literal escaped.
- **Escapes.** A backslash before ASCII punctuation is that character, so any
  punctuation may be escaped whether it needs to be or not. Before anything
  else it is one of the table's escapes or an error: `\1` to `\9` and `\k` are
  refused as backreferences by name, and every other escape the table does not
  name (`\p`, `\A`, `\0`, `\é`) is refused as unknown.
- **Classes.** A member is a character — a literal, an escaped character, a
  control escape or a code point — a range of two characters joined by `-`, or
  a shorthand. Inside a class only `\`, `]`, `[`, `-` and a leading `^` are
  special. A `]` first, after the `^` of a negated class if there is one, is a
  member, so `[]]` is a class and `[]` is an unclosed one. A `-` first, last or
  escaped is a member; between two members it joins them into a range, and is
  an error when either is a shorthand or a range. An unescaped `[`, and `\b`
  or `\B`, are errors inside a class, and so is a range whose end is below its
  start.
- **Repetition.** A quantifier follows an atom: a literal, `.`, a class or a
  group. At the start of the pattern, of a group or of an alternative, after
  an assertion, or after another quantifier — the `?` that makes one lazy
  aside — it has nothing to repeat and is an error. A count is decimal, with
  no space, at most `MAX_PATTERN_REPETITION`, and `n` at most `m`; `x{,m}` is
  an error.
- **Groups.** `(` and `(?:` open a group, which only groups. `(?=`, `(?!`,
  `(?<=` and `(?<!` are refused as look-around by name, and every other `(?`
  form, inline flags and named groups among them, is refused.

The parser reads the pattern once, left to right, keeping its open groups on
an explicit stack, and builds a tree of nine kinds of node:

```text
Syntax
├ Empty                               an empty pattern, alternative or group
├ Char(char)                          a literal, written or escaped
├ Any                                 .
├ Class { negated, members }          [...], [^...] and the six shorthands, as written
├ Assert(Assertion)                   ^ $ \b \B
├ Concat(Vec<Syntax>)
├ Alternate(Vec<Syntax>)              in priority order
├ Repeat { inner, min, max, greedy }  * + ? and braces; max None when unbounded
└ Group(Box<Syntax>)                  ( ) and (?: )
```

The nesting cap bounds the tree's depth, so a walk that recurses over it —
compiling it, dropping it — stays as shallow as the cap.

### What a match is

- **Leftmost-first.** A search takes the match that starts leftmost in the
  line, and of the matches starting there the one a backtracking reading
  reaches first: alternatives left to right, a greedy quantifier's most
  repetitions first and a lazy one's fewest. One rule a backtracking reading
  lacks: an iteration of a repetition that consumes nothing fails rather than
  repeating, so `(a*)*` and `(a|)*` end.
- **Hits.** A line's hits are its leftmost-first match, then the one from where
  that ends, and so on: never overlapping, in order. A match that covers no
  character is no hit — a quad covers glyphs, and it covers none — and the
  next search starts one character past it. So a pattern that matches only the
  empty string (`""`, `a*?`, `\b`) finds nothing, as an empty needle finds
  nothing, and one whose preferred match at a position is empty (`|a`) finds
  nothing there.
- **A line at a time.** `^` holds at the start of a line's text and `$` at its
  end. Nothing spans two lines.
- **Literal twins.** A pattern whose every character is a literal — a needle
  with each metacharacter escaped — finds `search_with`'s quads for that
  needle under the same options, with the one difference *Folding a pattern*
  names.

### The classes, from the vendored UCD

A class's membership is Unicode 17.0.0's, compiled by `build.rs` into sorted
ranges from the tree vendored in `crates/tinker-pdf-content/data/ucd`, and
never the toolchain's `char` methods, whose Unicode version is the compiler's
rather than the tree's.

| Class | Members | From |
| --- | --- | --- |
| `\d` | General_Category `Nd` | `UnicodeData.txt`, field 2 |
| `\s` | `White_Space` | `PropList.txt` |
| `\w` | UTS #18 Annex C's word class: `Alphabetic`, General_Category `Mn`, `Mc` and `Me`, `Nd`, `Pc`, and `Join_Control` | `UnicodeData.txt`, field 2, and `PropList.txt` |

`Alphabetic` is derived as UAX #44 derives it, because the crate's tree holds
no `DerivedCoreProperties.txt`: General_Category `Lu`, `Ll`, `Lt`, `Lm`, `Lo`
or `Nl`, or `Other_Uppercase`, `Other_Lowercase` or `Other_Alphabetic`.
`UnicodeData.txt`'s `First>` and `Last>` pairs are ranges, as `build.rs`'s
right-to-left table already reads them. `\D`, `\W` and `\S` are complements,
taken after folding (below); a bracket class's members are what it writes, and
`.` is every character.

### Folding a pattern

A pattern search reads the page's form in the options' fold: each glyph's text
lower-cased by `str::to_lowercase` unless case is significant, then its
diacritics removed by `fold_diacritics` unless they are — the forms
`search_with` builds at its first search in a fold and keeps
(`docs/design/performance.md`, P-24), read and not rebuilt. The pattern is
folded to meet them, once for each of the four folds, when it is parsed:

- **A literal** becomes its image: the character lower-cased on its own unless
  case is significant (`char::to_lowercase`, which is what `str::to_lowercase`
  gives a glyph of one character), then its diacritics removed unless they are
  significant. An image is one character, several in a row (`İ` lower-cased is
  `i` and U+0307), or none (U+0301 without diacritics), and each character is
  matched in turn.
- **A positive class** matches one character of the folded text that is a
  member's one-character image, or the several characters a member's longer
  image is, the longer image first. A range stands for each character in it
  and a shorthand for each in its table.
- **A negated class**, and `\D`, `\W` and `\S` inside brackets or out, match
  one character of the folded text that is no member's one-character image.
  The complement is taken after folding, so `[^a]` searched without case does
  not match `A`, which the folded text holds as `a`; a member whose image is
  several characters takes no part in it.
- A member whose image is empty stands for nothing, and `.` is any one
  character of the folded text.

An image is computed from the short lists of what each fold changes, never by
walking a range's characters: the diacritic tables `fold_diacritics` reads, and
a list of every character `char::to_lowercase` changes with what it gives,
which `build.rs` writes from the build's own `char::to_lowercase`. One
toolchain compiles the build script and the crate, so the list is the
lower-casing the text gets. A range's image is its characters outside the
lists and the images of those inside.

The one difference from `search_with`: `search_with` lower-cases its needle
whole, and `str::to_lowercase` reads context for one character, so a capital
sigma ending a word of the needle becomes `ς`, where the page, folded a glyph
at a time, holds `σ`. A pattern folds a character at a time, as the page is
folded, and finds the page's word where the needle does not.

### Whole word and `\b`

`\b` holds at a position of a line's folded text that is one of its UAX #29
word boundaries (`crate::words::word_boundaries`), and `\B` at every other
position. They are the boundaries `whole_word` reads, kept in the form at the
first search in its fold that needs them, so a boundary assertion and the
whole-word switch never disagree. `\b` is therefore segmentation and not a
transition between `\w` and `\W`: it holds at both ends of a non-empty line and
between two punctuation marks, and not inside `can't` or `3.14`.

`whole_word` is `\b(?:P)\b` around the pattern: a thread starts only at a
boundary and a match is accepted only at one. That adds no state, so neither a
program nor its cap depends on the switch. A rejected whole-word candidate
holds nothing back, as in `search_with`, because the next start is already
running. `SearchPattern::matches` reads a bare text's boundaries from
`word_boundaries` itself.

### The program

The tree compiles into a Thompson automaton: a state for each literal
character, class, `.` and assertion; a split for each alternation and each
quantifier's choice, its first branch the greedy or the lazy preference; jumps;
and one match state. A counted repetition is written out: `x{n,m}` as n copies
of `x` and m − n nested optional ones, `x{n,}` as n − 1 copies and `x+`, and
`x{0,}` as `x*`. A class's state holds its ranges, and under a fold one more
state for each further character of a member's longer image.

A program's size is its number of states. `SearchPattern::parse` compiles four
programs, one for each fold a page keeps a form in, and `MAX_PATTERN_STATES`
holds the largest, so whether a pattern parses never depends on the options it
is later searched with. The compiler counts as it emits and stops at the cap,
so a refusal costs at most the cap.

### The simulation

A line's positions are the boundaries of its folded characters, from 0 to its
length. At each position the simulation keeps a list of threads, each a state
and the position its match began, in priority order. Stepping over the next
character moves every thread whose state is a character, a class or `.` that
accepts it into its next state, and follows splits, jumps and assertions there
at once, a split's first branch before its second. A state already entered at
a position is not entered again — a sparse set the size of the program
remembers — so each state is entered at most once at each position. A class's
ranges are searched by bisection; `^` and `$` compare the position with the
line's ends, and `\b` and `\B` read a cursor that moves forward along the
line's boundaries.

A thread that reaches the match state — at a boundary, under whole word —
records its match and ends the step there: no thread below it is stepped and no
branch below it followed, since leftmost-first would discard them, and nothing
below it enters a state at that position. Until a search records a match, a
new start thread joins it at each position, below all its others.

### Every hit of a line in one pass

Taking a line's hits one search at a time, each starting where the last ended,
would read again whatever a search had read past its hit while a preferred
thread was still alive: `x*y|x` over a line of n `x`s has n hits, and each
search reads on to the line's end hoping for a `y`, about n²/2 characters in
all. So the search that would follow a hit runs in the same pass, behind it.

When a search records a match, a search starting where the match ends — a
character further, for an empty one — joins the list below every thread of the
search before it. Threads stay in that order: every thread of an earlier search
above every thread of a later one, and within a search in priority order. A
state is entered once at a position, by whichever thread reaches it first, so a
later search's thread that meets an earlier search's thread in the same state
is dropped. That loses nothing. The two would go on identically, so the states
the dropped thread would have kept from its search's other threads, the earlier
thread keeps from them too; and it could only have mattered otherwise by
reaching a match, which the earlier search would reach first, by the same
thread or a preferred one. Then the earlier search's match is replaced, and
every search behind it, started where the replaced match ended, is discarded
for one started anew.

A search whose match is final — none of its threads left — hands its hit out
once every search before it has, and the next behind it becomes the first.
So a line is read once, a position enters each state at most once across every
search on it, and the hits are those that searches run one after another would
find.

### The API

```rust
// tinker_pdf_content::regex, re-exported by the facade.
pub const MAX_PATTERN_BYTES: usize = 4_096;
pub const MAX_PATTERN_NESTING: usize = 64;
pub const MAX_PATTERN_REPETITION: u32 = 1_000;
pub const MAX_PATTERN_STATES: usize = 8_192;

pub struct SearchPattern { /* its tree and its four programs */ }

impl SearchPattern {
    pub fn parse(pattern: &str) -> Result<SearchPattern, PatternError>;
    pub fn syntax(&self) -> &Syntax;
    pub fn states(&self) -> usize;
    pub fn matches(&self, line: &str) -> Vec<Range<usize>>;
    pub fn matches_counted(&self, line: &str) -> (Vec<Range<usize>>, Stats);
}

pub struct PatternError {
    pub offset: usize,
    pub kind: PatternErrorKind,
}

impl TextPage {
    pub fn search_pattern(&self, pattern: &SearchPattern, options: &SearchOptions) -> Vec<Quad>;
    pub fn search_pattern_counted(
        &self,
        pattern: &SearchPattern,
        options: &SearchOptions,
    ) -> (Vec<Quad>, Stats);
}
```

- `parse` reads, checks and compiles a pattern, or says at which byte it is
  malformed; `syntax` is its tree and `states` the size of its largest
  program.
- `matches` is the matcher on its own, over one line of text as written, case
  and accents significant: the byte ranges of its hits.
- `search_pattern` is the page search: the form in `options`' fold, the
  program compiled for it, `whole_word` as boundary assertions, and each hit
  mapped back to the glyphs it covers by `search_with`'s cursor, so the answer
  is a quad per hit in the order the lines are read.
- `PatternError` implements `Display` and `std::error::Error`, naming its kind
  and its offset. A malformed pattern is always this error and never an empty
  answer (ruling 10).
- A `SearchPattern` is immutable, `Send` and `Sync`: parsed once and searched
  on every page, from as many threads as search them. A search allocates its
  own thread lists, sized by the program.

**Beside the options, not a field of them.** The row puts the pattern inside
the existing search options, and it is inside them in what they mean: the three
switches `SearchOptions` already has are the pattern search's switches, meaning
for a pattern what they mean for a needle, over the same forms. The pattern is
an argument beside them rather than a field, for three reasons. `SearchOptions`
is `Copy`, `Eq` and `Default`, and a pattern owns its compiled programs, so a
field would take `Copy` from every caller's options or put a lifetime on the
type. A needle parsed inside `search_with` would have nowhere to say it is
malformed but an empty answer, so the error is the parse's, before any page is
read. And a pattern is parsed once and searched on many pages, as a page is
folded once and searched many times.

### The work, counted

Every state the simulation enters is counted into MS-01's tally as
`Stats::pattern_states_entered`, a `Counter::PatternStatesEntered` added to
`crates/tinker-pdf-cos/src/work.rs`. A state is entered at most once at each
of a line's `n + 1` positions, so a line costs at most `states() × (n + 1)`
entries, and a page the sum over its lines. `matches_counted` hands the count
back for one line, and `search_pattern_counted` with what `search_counted`
counts beside it: `text_forms_built`, `word_boundaries_computed`, and
`search_steps` for the boundary cursor and the mapping back to glyphs. The
bound is a counted guard in MS-01's sense — a count against a bound, never a
clock (`docs/design/performance.md`, "The guard suite").

### The caps

| Cap | Value | What it bounds |
| --- | --- | --- |
| `MAX_PATTERN_BYTES` | 4 096 | the pattern, checked before a byte of it is parsed: a few hundred words in an alternation fit |
| `MAX_PATTERN_NESTING` | 64 | the groups open at once, and with them the depth of every walk of the tree |
| `MAX_PATTERN_REPETITION` | 1 000 | a count a brace states |
| `MAX_PATTERN_STATES` | 8 192 | the largest of a pattern's four programs, and with it a line's work: at most 8 192 states entered at each position |

Each is a public constant with a row in `crates/tinker-pdf/tests/bounds_ledger.rs`,
and each refusal is a `PatternError`, never a panic and never a truncated
pattern.

### Errors

| Kind | Raised for | Offset |
| --- | --- | --- |
| `TooLong` | a pattern of more than `MAX_PATTERN_BYTES` bytes | `MAX_PATTERN_BYTES` |
| `TooDeep` | a group opened inside `MAX_PATTERN_NESTING` open groups | its `(` |
| `UnclosedGroup` | a `(` with no `)` | the `(` |
| `UnclosedClass` | a `[` with no `]` | the `[` |
| `Unescaped` | a `)`, `]` or `}` with nothing open, or a `{` that begins no repetition | the character |
| `ClassSyntax` | an unescaped `[`, a `\b` or `\B`, or a range with a shorthand or a range at an end, inside a class | the character |
| `ReversedRange` | a range whose end is below its start | the range's first character |
| `NothingToRepeat` | a quantifier with no atom before it | the quantifier |
| `ReversedRepetition` | `x{n,m}` with `m` below `n` | the `{` |
| `RepetitionTooLarge` | a count above `MAX_PATTERN_REPETITION` | the `{` |
| `UnknownEscape` | an escape the syntax does not name | the `\` |
| `CodePoint` | a `\x` escape that names no Unicode scalar value | the `\` |
| `Backreference` | `\1` to `\9`, or `\k` | the `\` |
| `LookAround` | `(?=`, `(?!`, `(?<=` or `(?<!` | the `(` |
| `GroupSyntax` | any other `(?` form but `(?:` | the `(` |
| `ProgramTooLarge` | a pattern whose largest program would pass `MAX_PATTERN_STATES` | 0, the pattern as a whole |

### Determinism

The parser, the compiler and the simulation use integers and fixed orders
alone: no floating point, no hashing and no clock. The hits, and the quads
`span_quad` makes from them as it makes `search_with`'s, are the same on every
target (ruling 4).

## Milestones

1. **The parser** (FT-20.2). The syntax above read into its tree on an
   explicit stack, every malformed pattern a `PatternError` at its offset, and
   the length, nesting and repetition caps enforced before anything is
   compiled, each a public constant the facade re-exports. Exit:
   `crates/tinker-pdf-content/tests/regex_syntax.rs`'s
   `every_construct_parses_to_its_tree`, a table over each construct the
   syntax admits; `malformed_patterns_are_errors_at_their_offset`, over an
   unclosed group and an unclosed class, a reversed range, a quantifier with
   nothing to repeat, an unknown escape, and a backreference and a
   look-around refused by name; `nesting_past_the_cap_is_refused_on_a_64_kib_stack`;
   a property test that arbitrary text never panics; and `bounds_ledger.rs`
   holding each of the three caps to its row.
2. **The matcher** (FT-20.3). The program, the simulation, every hit of a line
   in one pass, `MAX_PATTERN_STATES` and `pattern_states_entered`. Exit:
   `crates/tinker-pdf-content/tests/regex_match.rs`'s
   `every_construct_matches_its_fixtures`, per construct a pattern, a text and
   the hits worked out by hand; `leftmost_first_picks_the_documented_alternative`;
   and the counted guard `a_hostile_pattern_costs_work_linear_in_the_text`:
   for `(a*)*b`, `(a|aa)*c`, `(x+x+)+y` and `x*y|x` over texts of `n` and
   `2n` characters, `pattern_states_entered` never exceeds `states()` times
   one more than the text's length, and `x*y|x` finds its `n` hits; entered in
   `xtask/src/determinism.rs`'s roster beside the other counted guards and run
   on the guard suite's targets. `bounds_ledger.rs` holds `MAX_PATTERN_STATES`
   to its row.
3. **The search** (FT-20.4). Folding, whole word and `search_pattern`, and
   the syntax table in `docs/features/content-and-text.md` and in `search.rs`'s
   module documentation, in place of their paragraphs saying regular
   expressions are not an option. Exit:
   `crates/tinker-pdf/tests/regex_search.rs`'s
   `each_construct_finds_its_known_hits_on_a_page`, quads computed from the
   glyph boxes, and `the_options_compose_with_a_pattern` — case, diacritics
   and whole word, each against its literal twin — in the default and
   `bundled-fonts` builds; `search.rs`'s `the_default_is_search_quad_for_quad`
   and `tinker_parity.rs` passing unchanged; and `determinism.rs`'s
   `regex_search_is_stable_across_targets`, entered in the roster, passing
   under `cargo test` on Linux and Windows and under `--target wasm32-wasip1`.
4. **The fuzz target** (FT-20.5). `fuzz/fuzz_targets/regex.rs` carves a
   pattern and a text from its input, parses, compiles and matches, and
   asserts no panic, every refusal a typed error, and `pattern_states_entered`
   within `states()` times one more than the text's length; its seeds in
   `fuzz/corpus/regex` cover every construct and the hostile families. Exit:
   `cargo xtask check` passes with the target in `fuzz/Cargo.toml` and
   `xtask/src/fuzzaudit.rs` giving its reason for carrying no signature, and
   `crates/tinker-pdf-content/tests/regex_seeds.rs`'s
   `every_seed_reaches_the_matcher` restates the target's carving and shows
   each committed seed parsed and run rather than stopped at the carving.
5. **Speed** (FT-20.6). A pattern search, one pattern per construct family,
   over the prebuilt text MS-04's search rows use, sized by repetition into
   MS-04's 20 to 300 ms window, each machine entry of `baseline.json` naming
   it unmeasured until its figure is recorded. Exit: `cargo xtask baseline`
   holds the operation in MS-04's text group with the bench file and
   `baseline.json` agreeing (`xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree`), and
   `cargo bench -p tinker-pdf` reports its time.

## Risks

| Risk | Mitigation |
| --- | --- |
| `\b` surprises a reader of other syntaxes: UAX #29 puts a boundary at both ends of a line and between two punctuation marks, and none inside `can't` or `3.14` | It is the whole-word switch's boundary, so `\b` and whole word agree on every line, and the syntax table says which definition it is |
| Lower-casing and the classes come from two Unicode versions, the toolchain's and the tree's 17.0.0 | Membership is the tree's alone, and a class is matched through the same lower-casing its text gets, so a class and the text it is matched against agree whatever the toolchain's version |
| A capital sigma ends a needle's word as `ς` and a page's glyph as `σ` | The pattern folds a character at a time, as the page is folded, and finds the page's word; the difference is stated where the literal twins are |
| Counted repetition multiplies a program, `(a{1000}){1000}` | Counts are capped, the program is capped as it is emitted, and the refusal arrives at parse, costing at most the cap |
| One fold's program passes the cap where another's does not | The cap holds the largest of the four, so a pattern's parse never depends on the options; the growth is a state for each further character of a member's longer image, which most classes never meet |
| A line whose first search stays alive to its end holds every later hit until then | They are the answer's hits, held as positions; the memory is linear in the line, and nothing is read twice |
| Empty matches find nothing, so `a*?`, `\|a` and `\b` alone return no quads | The rule is stated beside the syntax, as the empty needle's is: a quad covers glyphs, and an empty match covers none |
| A pattern is untrusted input wherever a caller passes an end user's query through | The caps are checked before anything is compiled, the work is linear in the text by construction and counted, and FT-20.5's target holds the bound on every input |
| The lockstep simulation is slower than `search_with`'s Knuth–Morris–Pratt scan for a plain needle | `search_with` stays the literal search; FT-20.6 measures the pattern search, and a faster matcher is a row of its own |
