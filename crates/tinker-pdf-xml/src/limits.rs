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
//! by a clock**. All four fire at the shipped default rather than at a lowered
//! one, because an input that reaches any of them is a few kilobytes of markup.
//!
//! The yardstick for the second number is gap 30's, named in its bounds
//! section: **a 200-page fixed document at roughly 2 000 drawable elements and
//! 40 000 path segments a page**, which is a dense report or a technical
//! drawing rather than a letter. These four bound **one part**, and a part is
//! one page of that document.
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
//! part they are in, by the paragraph above, and none of them is copied more
//! than once. A constant over any of them could never fire before the input
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
