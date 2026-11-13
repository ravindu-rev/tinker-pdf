# Document search

*For [ROADMAP](../ROADMAP.md) row FT-21.* A page can be searched today:
`TextPage::search` and `search_with` find a needle within each line of one
page's text, over the forms the page keeps and the linear matcher that reads
them (`docs/design/performance.md`, P-24; `docs/features/content-and-text.md`,
"Search options"). This document fixes what searches a whole document: a
cursor that finds the next or previous hit from a position, reading pages
only as it reaches them; the page text joined line to line, block to block
and page to page, which three keep- options govern; a hit that crosses a page
break, reported as quads on each page it covers; the sources beside the page
text — annotations, form values, outline titles and attachments — and the
PDFs attached to a document, searched to a bounded depth and size; and
`tpdf grep`, which runs it over files.

## Scope

- **The options** (FT-21.2): `DocumentSearchOptions`, the page search's
  `SearchOptions` beside `keep_lines`, `keep_paragraphs` and `keep_hyphens`,
  every default chosen so that nothing a page's own search answers changes.
- **The joined text** (FT-21.2): what text a document search reads — each
  page's `Page::text`, every line in logical order (ruling 14), joined to the
  next with a space or rejoined at a hyphen by `plain.rs`'s rule, or kept
  apart where a keep- option says so.
- **The cursor** (FT-21.2): `Document::search`, a `DocumentSearch` that
  finds the next hit (`Iterator::next`) or the previous one from its
  position, extracts a page only when it reaches it, and whose hits backwards
  are its hits forwards in reverse.
- **A hit** (FT-21.2): its place — quads grouped by page for page text — its
  text, the text around it, and the positions it starts and ends at.
- **The sources** (FT-21.3): annotation text, form-field values, outline
  titles, and attachment names and descriptions, each searched with the same
  options, each hit naming where it was found.
- **Attached documents** (FT-21.4): a PDF attached to the document opened
  through the facade and searched with the same cursor, to
  `MAX_ATTACHMENT_DEPTH` levels, `MAX_ATTACHMENT_BYTES` each and
  `MAX_SEARCHED_ATTACHMENTS` in one search; an attachment those caps or a
  password stop is reported by name.
- **`tpdf grep`** (FT-21.5): its arguments, its output and its exit status.
- **The speed row** (FT-21.6): MS-04's text group's two document searches.

## Non-goals

- **Another answer from a page's search.** `TextPage::search`,
  `search_with` and `search_counted` keep their quads, and `SearchOptions`
  keeps its fields: the keep- options are the document search's own, so a
  page searched alone answers as it always has.
- **A pattern across pages.** The cursor finds a literal needle. A regular
  expression is FT-20's, and FT-20's matcher is a line's
  (`docs/design/regex-search.md`).
- **An index.** The cursor reads pages each time it is run; a document-wide
  or corpus-wide index is FT-22's.
- **An annotation's appearance stream.** The text an appearance draws is
  FT-27's; the sources here are the strings an annotation dictionary states.
- **Another reading order.** The page text is `Page::text`'s. A document
  searched in an inferred or stated order would need the reader to choose
  it, which `Page::text_in` already offers a page.
- **The bytes of an attachment that is not a PDF.** Its name, file name and
  description are a source; its contents are not opened.
- **Unlocking the caller's document.** An encrypted document is searched as
  its caller authenticated it; a document no password has opened yields no
  hit and is reported, never searched as garbage.
- **Threads.** A search is one thread's.

## Design

### The options

```rust
pub struct DocumentSearchOptions {
    /// Case, whole word and diacritics, as a page's search reads them.
    pub search: SearchOptions,
    /// Keep every hit inside one line: a line's end is a boundary no hit
    /// crosses.
    pub keep_lines: bool,
    /// Keep every hit inside one block: a block's end — and so a page's — is a
    /// boundary no hit crosses.
    pub keep_paragraphs: bool,
    /// Keep a hyphen where the page draws it: no line is rejoined, and a soft
    /// hyphen is a character like any other.
    pub keep_hyphens: bool,
    /// Which of the document's sources are searched (FT-21.3).
    pub sources: SearchSources,
}
```

Every `bool` is off by default, and `search` is `SearchOptions::default()`,
`search`'s own behaviour: so the default joins every line to the next across
blocks and pages and rejoins hyphenated words, which is the reading a person
searching a document expects. `SearchSources::default()` searches every
source; `SearchSources::PAGES` is the page text alone. The options carry no
switch a page's search reads, so `search_with` and its default are untouched.

With `keep_lines` and `keep_hyphens` both on, each line is read alone and as
drawn, so the hits a document search reports on a page are `search_with`'s
on that page, quad for quad, for the same `SearchOptions`.

### The joined text

A page's text is its `Page::text()`, every line in logical order (ruling 14),
so a right-to-left phrase is found as it is read, a line continued on the next
page included. Its lines are taken in `TextPage::lines` order, page after
page. A line's **units** are its `TextChar`s: what a hit maps back to, each
with its text and its quad.

**What a unit reads as** is its piece of the page's kept form in the
options' fold (P-24): the text the page's own search folds a glyph at a time,
so case and diacritics behave exactly as on one page. Without `keep_hyphens`,
every U+00AD SOFT HYPHEN is removed from the pieces, as
`PlainTextOptions::rejoin_hyphens` removes every one: it is invisible except
where a line breaks at it.

**Between two consecutive lines** stands one of three joins:

- **A boundary** no hit crosses: always with `keep_lines`; with
  `keep_paragraphs`, where the two lines are in different blocks, a page's
  last line and the next page's first among them.
- **A hyphen join**, without `keep_hyphens`, where `plain.rs`'s rule joins
  the two lines: the first line, its trailing white space set aside, ends in
  a soft hyphen; or it ends in U+002D or U+2010 after a letter and the second
  line, its leading white space set aside, starts with a lower-case letter.
  The rule is `tinker_pdf_content::plain::hyphen_join`, the one function
  `PlainTextOptions::rejoin_hyphens` and the document search both call, so
  the two cannot come to disagree about what a hyphen is. The hyphen is
  removed and the lines meet with nothing between them.
- **A space**, otherwise: the two lines meet across one U+0020 that belongs
  to no unit.

At a hyphen join or a space, the first line's trailing white-space units and
the second line's leading ones are set aside (a unit whose text is all white
space, or empty), so a phrase broken across a line reads with one space in
it whatever the producer drew at the line's ends. At a boundary nothing is set
aside: each side is read as drawn. The hyphen a hyphen join removes is the
last character of the first line's last unit not set aside, its own glyph as
a producer draws one, and it is removed from that unit's piece alone, so the
unit still maps a hit back to its glyph.

So the document's text is a sequence of **runs**, each a stretch no boundary
cuts — a line with `keep_lines`, a block with `keep_paragraphs`, otherwise
every line of the document — and each run is a sequence of **chunks**, the
lines a hyphen joins, with a space between each chunk and the next.

### Hits

A run's hits are chosen exactly as a page's search chooses a line's: every
occurrence of the folded needle in the run's joined text, by
`tinker_pdf_content::search::Needle` — the page search's Knuth–Morris–Pratt
automaton, the one matcher, its state carried from chunk to chunk — and the
same two overlap rules: an occurrence that begins inside a hit is passed
over, and a whole-word candidate that is not one holds nothing back. Whole
word reads UAX #29 boundaries of the joined text (`word_boundaries`), each
chunk's computed with a space beside each end that meets a space join and
nothing beside one that meets a boundary. A space is a character no
`Word_Break` rule looks past, so a chunk's boundaries computed that way are
the run's; a hyphen join keeps its two lines in one chunk, where the word it
mends is read whole. An occurrence that maps to no unit — a needle of one
space, matching only a join — is passed over as a hit and still holds back
what it covers, as the page search's does.

The hits of a document are therefore fixed by the needle and the options,
whatever the cursor does, and the cursor moves through them.

### The cursor

```rust
impl Document {
    pub fn search(&self, needle: &str, options: &DocumentSearchOptions) -> DocumentSearch;
}
impl Iterator for DocumentSearch { type Item = SearchHit; /* the next hit */ }
impl DocumentSearch {
    pub fn previous(&mut self) -> Option<SearchHit>;
    pub fn position(&self) -> SearchPosition;
    pub fn seek(&mut self, position: SearchPosition);
    pub fn unsearched(&self) -> &[Unsearched];
}
```

A **position** stands before a unit, or at the end of a line: a page, a line
of it, a unit and a byte inside the unit's folded piece, so a hit that ends
inside a ligature is followed by the one that starts there. `SearchPosition`
is opaque and ordered as the document reads; `SearchPosition::start()`,
`end()` and `page(n)`, the start of page `n`'s text, are the ones a caller
names, and every hit carries the two it starts and ends at.

`next` returns the first hit that starts at or after the position and moves
the position to its end; `previous` returns the last hit that ends at or
before it and moves the position to its start. So `next` after `previous`
returns the hit again, as a list iterator does, and the hits `previous` finds
from the end are the hits `next` finds from the start, in reverse.

**Where a scan starts.** A run's hits depend on where the scan of it began —
the overlap rule carries from one occurrence to the next — so the cursor
never starts mid-run at a point an occurrence crosses. A chunk's start is
**clean** when no qualifying occurrence starts before it and ends after it,
which the cursor reads off the needle's length less one byte on either side;
a run's start always is. From a clean point the hits after it are the run's,
whatever came before it, since every hit before it ended there. `next` scans
forward from the latest clean chunk start at or before its position, and keeps
its scan between calls, so a cursor walked forwards scans each page once.
`previous` scans from the latest clean chunk start before its position to the
position, takes the last hit there, and steps back a clean point at a time
while it finds none.

**Which pages it reads.** A page is extracted when the scan reaches its
first line, or when a clean check or a join reads its first or last line;
the cursor keeps the few pages its scan stands on and drops the rest. A
forward search from a page's start reads the pages up to the one its hit ends
on, and the page after one only where it reaches that page's last line ending
in a hyphen, whose join the next page's first line decides. A page whose text
is empty joins nothing, and a clean check before a page's first line reads
back over empty pages to the last line before it.

**Which sides are set aside.** Whether a line's leading or trailing white
space is set aside depends on the options and the blocks alone, never on
whether another line follows: it is, wherever the options would join lines
there. So the document's first and last lines are read trimmed by default,
and a block's first and last as drawn with `keep_paragraphs`.

### What is not searched

Ruling 10: a document not searched is named, never counted as having no hit.
`DocumentSearch::unsearched` lists what the cursor reached and could not read,
each an `Unsearched` — the attachment path it sits under, empty for the
document searched, and its reason, an `UnsearchedReason` — once however often
the cursor passes it. The document searched is listed with
`PasswordRequired` where it is encrypted and no password has opened it, or
`UnsupportedEncryption` where its handler is one this build does not
implement (`Document::readable`), and then nothing of it is read; FT-21.4
adds the reasons an attachment is not searched.

### A hit

```rust
pub struct SearchHit {
    pub place: HitPlace,
    /// The text it covers as the source states it: its units' texts, joined
    /// as the search joined them.
    pub text: String,
    /// Up to `HIT_CONTEXT` characters of its first line before it, and of
    /// its last line after it, as stated.
    pub before: String,
    pub after: String,
    /// The attachments the document holding it is filed under, outermost
    /// first; empty in the document searched (FT-21.4).
    pub attachment_path: Vec<String>,
    /* where it starts and ends: SearchHit::start, SearchHit::end */
}

#[non_exhaustive]
pub enum HitPlace {
    /// Page text: each page the hit covers, in order, with a quad for each
    /// line of it the hit covers, mapped to glyphs as `search_with` maps its
    /// own and taken as its `span_quad` takes them.
    Pages(Vec<PageQuads>),
    /* FT-21.3's four, below */
}
```

A unit a hit covers is chosen as the page search chooses a glyph: the first
whose piece ends past the hit's start and the last whose piece starts before
its end. The quad of each line is `TextLine::span_quad` over the line's first
and last covered unit, the page search's own quad, right to left the right
way round.

### The sources

Each source is read through what the facade already lists, so nothing
re-parses the file, and each **item** — one string — is its own run: a hit
never crosses from one item to the next. An item's lines are its own,
split at `\r\n`, `\r` and `\n`, one block; its units are its characters,
each folded alone as a page's glyphs are; the keep- options join its lines
as they join a page's. A hit names its item and the byte range it covers in
the item's string as stated — for `/RC`, in its character data.

| Source | `SearchSources` | Items, in order | `HitPlace` |
| --- | --- | --- | --- |
| Annotations | `annotations` | each page's `Page::annotations`, in `/Annots` order: `/Contents`, then the text of `/RC`, then `/Subj`, then a markup annotation's `/T` (its author); a pop-up's text is its parent's, so a pop-up is passed over | `Annotation { page, index, rect, text, range }`, `text` an `AnnotationText` naming which string |
| Form fields | `form_fields` | each of `Document::form_fields`, in its order: a field's text value, or each value a list box holds, one item each; a check box's, a radio button's or a push button's state and a signature are no text | `FormField { name, widgets, value, range }`, `value` the item's index among the field's values |
| Outline | `outline` | every outline item's title, depth first | `Outline { path, titles, range }`, `path` the item's index among its siblings at each depth and `titles` the titles from the top to it |
| Attachments | `attachments` | each of `Document::attachments`, in name order: its name, its file name where it is not the name, then its description | `Attachment { name, text, range }`, `text` an `AttachmentText` naming which string |

`/RC` is XHTML (Table 170). Its text is its character data, read by
`tinker-pdf-xml`, with each end of a `p`, `div`, `li`, `br` or heading
element a line's end, so a paragraph of rich text reads as the person typed
it and no tag name is ever found. A rich-text stream, which the listing
names by reference rather than decoding, is decoded by
`CosDocument::stream_decoded` for the search, so a defect in it is the
document's warning as any decode's is; text that does not parse as XML is
searched as it stands. A widget's `/T` is its field's partial name, which the
form-field source does not search; field names are not a source.

**The order of the sources.** One document's text is read as its stretches
in a fixed order — its pages, its annotations, its form fields, its outline,
its attachments' names, then its attached documents — so the hits have one
order, `SearchPosition` is ordered by it, and `previous` crosses from a
stretch to the one before. A stretch whose source is left out of
`SearchSources` is passed over.

### Attached documents

With `SearchSources::attached_documents`, each attachment is read through
the stream reference `Document::attachments` hands back
(`crates/tinker-pdf-cos/src/outline.rs`, which deliberately reads no bytes)
and decoded by `CosDocument::stream_decoded`, under its 128 MiB ceiling.
Bytes that begin `%PDF-` within their first kilobyte are opened with
`Document::open`, its empty password tried where it is encrypted, and searched
with the same options by a cursor of their own, after the document's own
stretches, in name order, depth first. Every hit found there carries the
attachment path it was found under. Any other bytes are no document, and their
contents are not searched.

Three caps bound it, each a `pub const` on the facade with its ledger row in
`bounds_ledger.rs`:

| Cap | Value | Past it |
| --- | --- | --- |
| `MAX_ATTACHMENT_DEPTH` | 4 levels of attached document | the attachment is not opened |
| `MAX_ATTACHMENT_BYTES` | 32 MiB of decoded bytes | the attachment is not opened |
| `MAX_SEARCHED_ATTACHMENTS` | 256 attachments read in one search, at every level | no further attachment is read |

Each attachment the cursor reached and did not search joins
`DocumentSearch::unsearched` with its attachment path and the reason:
`TooDeep`, `TooLarge` or `TooMany` for a cap, `PasswordRequired` or
`UnsupportedEncryption` for an attached PDF the empty password does not open,
and `Unreadable` for a stream that does not decode or bytes `Document::open`
refuses.

### `tpdf grep`

```text
tpdf grep <needle> <file.pdf>... [--case-sensitive] [--whole-word]
          [--ignore-diacritics] [--keep-lines] [--keep-paragraphs]
          [--keep-hyphens] [--sources LIST] [--password P]
```

The first argument is the needle, every other one a file. `--sources` takes a
comma-separated list of `pages`, `annotations`, `fields`, `outline`,
`attachments` and `attached`, or `all`, the default. `--password` is tried on
each file that is encrypted, the empty password otherwise, as the reading
verbs do.

Each hit prints one line to standard output:

```text
<file>:<place>: <before>[<text>]<after>
```

where the place is `page N`, or `pages N-M` for a hit that crosses a break,
1-based; `annotation I on page N (contents)` with `rich text`, `subject` or
`author` in place of `contents`; `field NAME`; `outline T1 > T2`, the titles
from the outline's top; or `attachment NAME (name)` with `file name` or
`description`; and a hit inside an attached document has its place prefixed
by `attached NAME >` and a space for each attachment on its path,
`attached a.pdf > page 2`. A file that does not open, or that its password
does not open, is reported by name on standard error, and so is each entry
`unsearched` lists.

The exit status tells the three outcomes apart, as `grep` does: **0** when a
hit was found and every file and attachment was read, **1** when none was
found and every one was read, and **2** when any file or attachment could not
be read, whatever was found elsewhere. An argument the verb cannot parse exits
2 as every verb's does.

### What the content crate adds

Each a door the cursor reads and none an answer of the page's search:

- `plain::hyphen_join(line, next) -> Option<HyphenJoin>`, the hyphen rule,
  which `assemble` calls in place of its own copy.
- `search::Needle`, the page search's matcher owned, its automaton stepped a
  byte at a time, which `TextPage::search_with` scans with.
- `SearchOptions::fold`, a string folded as the options fold a page.
- `TextPage::folded_lines`, each line's kept form in the options' fold: its
  text and where each glyph's piece starts.
- `TextLine::span_quad`, the quad the page's search reports for a glyph span.

### Determinism and speed

The cursor reads `Page::text`, the kept forms, the matcher and UAX #29, each
the same on every target, and computes no number but the quads `span_quad`
takes from the glyphs; `determinism.rs`'s `document_search_is_stable_across_targets`
hashes the hits of a built document searched under every option. MS-04's text
group times a search from the first of 100 dense pages to a hit on the last,
and the same search backwards, each its own operation.

## Milestones

1. **FT-21.1, this document.**
2. **FT-21.2, the cursor.** `crates/tinker-pdf/tests/document_search.rs`:
   `a_match_across_a_page_break_is_found_forwards_and_backwards`,
   `backwards_finds_the_forward_hits_in_reverse`,
   `keep_lines_keeps_every_hit_inside_one_line`,
   `keep_paragraphs_stops_a_hit_at_a_block`,
   `keep_hyphens_leaves_a_broken_word_broken`, in the default and
   `bundled-fonts` builds; the module's unit test
   `the_next_hit_extracts_only_the_pages_up_to_it`; `determinism.rs`'s
   `document_search_is_stable_across_targets`, on Linux, Windows and
   `wasm32-wasip1`.
3. **FT-21.3, the sources.** `document_search.rs`'s
   `each_source_is_searched_with_a_known_hit`: a built annotation's
   `/Contents`, a value set through `DocumentEditor::set_field_value`,
   `testdata/outline-3level.pdf`'s outline title `Chapter 1`, and an
   attachment's name and description added through
   `DocumentEditor::attach_file`, each found once with its place and not
   found when its source is left out; both builds.
4. **FT-21.4, attached documents.** `document_search.rs`'s
   `a_hit_inside_an_attached_pdf_names_its_attachment` (one level, and a PDF
   attached inside an attached PDF),
   `an_attachment_past_the_caps_is_reported_not_searched` and
   `an_encrypted_attachment_is_reported_by_name`, both builds;
   `bounds_ledger.rs` holding the three caps to their rows.
5. **FT-21.5, `tpdf grep`.** `tools/tpdf/src/grep.rs`'s
   `grep_finds_the_known_hits_across_the_testdata_fixtures`: over
   `testdata/`, `page 3 of 3` once, on the third page of `simple-text.pdf`,
   and `Chapter 1` once, in `outline-3level.pdf`'s outline and on none of its
   pages, with `encrypted-aes256.pdf` and `permissions-noprint.pdf` reported
   by name when no `--password` is given, and the status 2; `tpdf --help`
   lists the verb and its flags.
6. **FT-21.6, the speed row.** `cargo xtask baseline` holds the two
   operations in MS-04's text group, `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` with the bench file and
   `baseline.json` agreeing, and `cargo bench -p tinker-pdf` reports their
   times.

## Risks

| Risk | Mitigation |
| --- | --- |
| A scan started mid-run picks other hits than one started at the run's start, so backwards and forwards disagree | Scans start only at clean points, from which the run's hits are the same; `backwards_finds_the_forward_hits_in_reverse` holds it over overlapping needles across a page break |
| A word boundary near a join reads otherwise than over the whole run | A space join is a space no `Word_Break` rule looks past, and a hyphen join keeps its lines in one chunk; whole-word fixtures cross both kinds |
| The hyphen rule drifts between plain text and search | One function, `plain::hyphen_join`, called by both; `plain.rs`'s unit tests and the cursor's fixtures read it |
| A page's own search moves | No `SearchOptions` field is added; `search.rs`'s tests, `text_options.rs` and `tinker_parity.rs` hold its quads, and with `keep_lines` and `keep_hyphens` the cursor's hits on a page are `search_with`'s |
| A cursor walked back and forth extracts every page again | The cursor keeps the pages its scan stands on; `the_next_hit_extracts_only_the_pages_up_to_it` counts extractions |
| A hostile attachment spends without bound: a deep nest, a decompression bomb, thousands of attachments | `MAX_ATTACHMENT_DEPTH`, `MAX_ATTACHMENT_BYTES` under the decoder's own ceiling, and `MAX_SEARCHED_ATTACHMENTS`, each reported by name when it stops a read |
| An attachment not searched reads as one with no hit | Ruling 10: `unsearched` names it, and `tpdf grep` exits 2 |
