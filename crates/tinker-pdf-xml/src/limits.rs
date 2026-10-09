//! Every bound this crate enforces, in one place, with the numbers that set
//! it.
//!
//! The form is `tinker-pdf-zip`'s `limits.rs`, and it is that shape for the two
//! scars that shape records. `5adf502` found an 1 851-byte page that took 19.3
//! seconds to render with `MAX_GROUP_DEPTH` in place the entire time — *depth
//! is not work once the recursion branches*. The XML form of that sentence is
//! that **a per-element cap is not a total once the element count is chosen by
//! the file**, so [`MAX_XML_TOKENS`] is a total, spent across one part and
//! never refunded, and the three beside it say in as many words that they are
//! not that total.
//!
//! Gap 18a's milestone 8 found the opposite failure in a constant written to
//! avoid the first: `MAX_JPX_WORK` sat *above* the most its own inputs could
//! ask for, so it could never fire. **A cap that cannot fire is not a cap.**
//! Every constant here carries three numbers — the most any fixture in this
//! repository spends, the most a plausible real document spends, and the
//! constant — and each is proved to fire in a test **by its own refusal, never
//! by a clock**. All six fire at the shipped default rather than at a lowered
//! one, because an input that reaches any of them is a few kilobytes of markup
//! — or, for [`MAX_HTML_CLONE_BYTES`], a hundred.
//!
//! The yardstick for the second number is gap 30's, named in its bounds
//! section: **a 200-page fixed document at roughly 2 000 drawable elements and
//! 40 000 path segments a page**, which is a dense report or a technical
//! drawing rather than a letter. The first four bound **one part**, and a part
//! is one page of that document. The last two bound only [`crate::html`], whose
//! tree builder keeps a list the XML reader does not and makes elements no
//! token asked for; no fixed document is HTML, so their yardstick is tag soup.
//!
//! # The bomb these caps do not defend against
//!
//! None of them is the defence against entity expansion, and reading them as
//! though they were is the mistake this paragraph exists to prevent. Billion
//! laughs is refused by [`crate::Error::DoctypeUnsupported`] before one byte
//! past `<!DOCTYPE` is read — ECMA-388 9.3.2 [M2.71] makes that the conformant
//! answer rather than a hardening choice — so the grammar that expands is never
//! entered. A depth cap that caught billion laughs would be evidence the
//! declaration had been *parsed*, which is the thing being refused.
//!
//! What is left after that cannot expand at all: the five predefined entities
//! and both radixes of numeric character reference each produce **exactly one
//! character** from at least four bytes of source, so decoded text is never
//! longer than the text it was decoded from. That is a property rather than a
//! budget, and it is why there is no cap on the length of an attribute value or
//! a text run.
//!
//! # There is deliberately no cap on value length, and none on namespace count
//!
//! An attribute value, a text run and a namespace URI are each bounded by the
//! part they are in, by the paragraph above, and in this reader none of them
//! is copied more than once. **HTML's tree builder is the exception**, and it
//! has a cap of its own: a clone of a formatting element carries a copy of
//! every attribute its start tag had, made as often as the input reopens it,
//! so [`MAX_HTML_CLONE_BYTES`] bounds the bytes those copies hold. Everything
//! else in an HTML tree was read from the input once. A constant over any of
//! the three could never fire before the input
//! ran out — gap 18a milestone 8's failure reached from the other direction,
//! which is the same argument `tinker-pdf-zip` makes for not bounding path
//! depth. The count of namespace declarations in scope is bounded by
//! [`MAX_XML_DEPTH`] times [`MAX_XML_ATTRIBUTES`], which is a product of two
//! caps rather than a number a file chooses.

/// The deepest element nesting one part may reach.
///
/// **This is not the work cap.** It bounds the parser's own stack — one entry
/// per open element, holding a name and a count of namespace bindings — and a
/// file with no nesting at all can still produce as many events as it likes.
/// [`MAX_XML_TOKENS`] is the total; reading this as the total is the mistake
/// `MAX_SCRIPT_STEPS` and `MAX_TILE_WORK` each carry the same warning about.
///
/// It also does **not** bound visual nesting across parts, which is what a
/// remote resource dictionary recurses through; gap 30's `MAX_XPS_VISUAL_DEPTH`
/// is that one and is deliberately separate.
///
/// | | Elements |
/// | --- | --- |
/// | The most any fixture here spends | 256 |
/// | A dense fixed page: ECMA-388 18.2's recommended 16 canvases, over a path geometry's own four | 24 |
/// | **This cap** | **256** |
///
/// The first row is the cap because the fixture that proves it fires is 257
/// nested elements, which is 771 bytes; the most any *real* markup in this
/// repository reaches is 6, measured across the eight XPS packages by
/// `crates/tinker-pdf/tests/xml_real_packages.rs`.
///
/// Reachable: an element costs three bytes (`<a>`), so a 128 MiB part —
/// `tinker_pdf_zip::limits::MAX_ZIP_ENTRY_BYTES`, which is what stands in front
/// of this in the only caller there will ever be — nests forty-four million
/// deep, and `nesting_past_the_depth_cap_is_refused_by_name` builds 257 of it.
pub const MAX_XML_DEPTH: usize = 256;

/// The most attributes one element may carry.
///
/// **This is not the work cap** either: it is per element, and the element
/// count is chosen by the file.
///
/// | | Attributes |
/// | --- | --- |
/// | The most any fixture here spends | 256 |
/// | A `Glyphs` with every optional attribute ECMA-388 12.1 gives it | 24 |
/// | **This cap** | **256** |
///
/// The most any real markup here carries is 8, on the `ImageBrush` of
/// `wpf-image-and-text.xps`. Namespace declarations are counted against this
/// too, because they arrive in the same list and cost the same parse.
///
/// Reachable: the shortest attribute is ` a=""`, five bytes, so a 128 MiB part
/// offers twenty-six million of them on one element;
/// `more_attributes_than_the_cap_is_refused_by_name` writes 257.
pub const MAX_XML_ATTRIBUTES: usize = 256;

/// The longest qualified element or attribute name, in bytes.
///
/// A name past this **refuses the part**; it is not truncated. That is the
/// opposite of `tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN`, and deliberately:
/// a truncated ZIP entry name still names an entry a reader can decide about,
/// where a truncated element name silently *becomes a different element* —
/// `FixedPage` and `FixedPag` are not the same tag, and nothing downstream
/// would ever find out. Gap 30's package layer has to make a truncated ZIP name
/// unresolvable for exactly this reason; a markup reader can simply refuse.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 1 024 |
/// | `LinearGradientBrush.GradientStops`, and room for a prefix | 48 |
/// | **This cap** | **1 024** |
///
/// The longest real name in the eight packages is 33 bytes, which is that
/// element. Reachable: a name may be as long as the part it is in, so 128 MiB;
/// `a_name_past_the_cap_is_refused_rather_than_truncated` writes 1 025 bytes.
pub const MAX_XML_NAME_LEN: usize = 1024;

/// **The work cap.** Events one part may produce, spent and never refunded.
///
/// **For [`crate::html`] it is tokens and nodes together.** HTML's tree
/// builder makes elements no token asked for — it reopens the formatting
/// elements a block closed, before every run of text after it, and clones them
/// in the adoption agency — so eight bytes of `<p>x</p>` after two hundred
/// open `<b>`s make two hundred elements. Every node created spends one beside
/// every token, and a clone spends one more for each attribute it copies, so
/// the count of nodes and of copied attributes is inside this one number;
/// `reopened_formatting_elements_are_spent_against_the_token_cap` crosses it
/// at the shipped value with fifty kilobytes, and
/// `attributes_copied_onto_clones_are_spent_against_the_token_cap` with
/// thirty-four. How long the copies are is [`MAX_HTML_CLONE_BYTES`]'s. The
/// other three caps bound the HTML parser as they bound this reader: its
/// stack of open elements, an element's attributes — a tag's, and the
/// `<html>` or `<body>` a later tag's attributes are merged into — and a tag,
/// attribute or DOCTYPE name.
///
/// A per-element cap times an element count the file chose is not a bound, and
/// this is the number that is. Every event costs one — a start tag, an end tag,
/// a text run, a CDATA section, a comment, a processing instruction — so an
/// empty-element tag costs two, because it produces two.
///
/// | | Events |
/// | --- | --- |
/// | The most any fixture here spends | 1 048 576 |
/// | A dense fixed page: 2 000 drawable elements and 40 000 path segments as `PolyLineSegment` children | ~92 000 |
/// | **This cap** | **1 048 576** |
///
/// The most any real markup here produces is 41 events, for
/// `wpf-gradients.xps`'s fixed page — twenty-eight of them element events and
/// thirteen the whitespace WPF writes between them.
///
/// Reachable: `<a/>` is four bytes and produces two events, so a 128 MiB part
/// asks for sixty-seven million — sixty-four times this cap.
/// `more_events_than_the_token_cap_is_refused_by_name` builds the two-megabyte
/// input that crosses it, at the shipped constant rather than a lowered one,
/// because a work cap proved only against a lowered limit is a work cap nobody
/// has checked the shipped number of.
pub const MAX_XML_TOKENS: usize = 1 << 20;

/// The most entries HTML's list of active formatting elements may hold,
/// markers included. **[`crate::html`] only**: the XML reader keeps no such
/// list.
///
/// §13.2.4.3's list is where a formatting element waits to be reopened after a
/// block closed it, and the tree builder walks it — Noah's Ark on every
/// formatting start tag, the adoption agency on every misnested end tag.
/// **[`MAX_XML_DEPTH`] does not bound it.** A table cell is a marker, and
/// behind each marker the cell may leave a stack's worth of formatting
/// elements that a `</p>` closed and nothing has reopened yet; nested cells
/// cost the stack four entries each (`table`, `tbody`, `tr`, `td`), so a list
/// can hold many times the stack that holds its cells. Past the cap the parse
/// stops with [`crate::Error::FormattingCap`], and the tree built so far is
/// kept.
///
/// | | Entries |
/// | --- | --- |
/// | The most any fixture here spends | 1 024 |
/// | Tag soup at its worst plausible: a `<font>`, a `<b>` and an `<i>` left open in every cell of tables nested eight deep, and a marker for each cell | 32 |
/// | **This cap** | **1 024** |
///
/// Reachable: thirty-two cells nested one inside another, each leaving a
/// hundred `<b>`s behind its marker, hold 3 232 entries and are never more
/// than 231 elements deep; `the_list_of_active_formatting_elements_stops_at_its_cap`
/// builds six cells of two hundred, eleven kilobytes.
pub const MAX_HTML_ACTIVE_FORMATTING: usize = 1024;

/// The most bytes of attribute names and values HTML's tree builder may copy
/// onto clones of formatting elements, across one parse. **[`crate::html`]
/// only.**
///
/// A clone is the one place an HTML tree is bigger than its input. §13.2.4.3
/// reopens every formatting element a block closed before the next text after
/// it, and the adoption agency clones them too, each with every attribute its
/// start tag had — so eight bytes of `<p>x</p>` after a `<b>` whose `title`
/// is a hundred kilobytes long is a hundred kilobytes more. The review of the
/// tier-5 formats lane measured 116 kB of markup holding 200 MB of attribute
/// values, and about 100 GB inside the token cap. [`MAX_XML_TOKENS`] bounds how
/// many copies there are, one unit for each attribute a clone copies; this
/// bounds how long they are. Past it the clone is not made, the parse stops
/// with [`crate::Error::CloneCap`], and the tree built so far is kept.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture here spends | 64 MiB |
/// | Tag soup at its worst plausible: three `<font face=… color=… size=…>`s of 100 bytes each reopened in every one of 20 000 paragraphs | 6 000 000 |
/// | **This cap** | **64 MiB** |
///
/// Reachable: a value half a one-megabyte input long, reopened by the
/// `<p>x</p>`s in its other half, asks for 2^35 bytes, 512 times
/// this cap; `attribute_bytes_copied_onto_clones_stop_at_their_cap` crosses
/// it with a hundred kilobytes.
pub const MAX_HTML_CLONE_BYTES: usize = 64 << 20;
