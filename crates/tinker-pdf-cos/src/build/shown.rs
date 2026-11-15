//! What the text-showing operators of content a caller wrote show (9.3.1,
//! 9.4.3): the codes each font resource is asked to draw, so that an embedded
//! program keeps every glyph that content draws.
//!
//! The builder records what its own text calls place as it writes them. The
//! operators a caller writes itself — [`super::PageBuilder::raw`], a form's,
//! a tiling pattern's and a colour glyph's content — are read here the way a
//! reader reads a content stream: `Tf` selects a font resource; `q` and `Q`
//! save and restore it with the rest of the graphics state (8.4.2), an
//! unmatched `Q` restoring nothing; and `Tj`, `'`, `"` and `TJ` show their
//! strings' codes in the font selected. An inline image's data is passed over
//! to its `EI` (8.9.7), by the rule this repository's interpreter reads it
//! with, so bytes inside it are never taken for operators.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::lexer::{is_delimiter, is_whitespace, Lexer, TokenKind};
use crate::warn::WarningSink;

/// Which of a content stream's text-showing operators are read, and the font
/// the stream starts in.
pub(super) enum Read<'a> {
    /// A page's content, which the builder's own calls and
    /// [`super::PageBuilder::raw`] wrote together. The builder recorded what
    /// its calls drew, so only the operators starting inside these byte
    /// ranges, the ones `raw` appended, are read; every operator still moves
    /// the font. A page starts with no font selected, and text shown before
    /// one is drawn in none.
    Page(&'a [Range<usize>]),
    /// Content a caller wrote whole: a form's, a tiling pattern's or a colour
    /// glyph's. It starts in the font it is drawn in, which is not known
    /// where it is written.
    Whole,
}

/// The font text is shown in at a point of a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Current {
    /// None selected.
    Unset,
    /// The font the stream is drawn in.
    Inherited,
    /// The font resource a `Tf` named, by its place in the names read.
    Font(usize),
}

/// What a stream's text-showing operators showed.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Shown {
    /// Each font resource's codes: a string's bytes one at a time, or two at
    /// a time, high byte first, in a font whose codes are two bytes.
    pub(super) codes: BTreeMap<Vec<u8>, BTreeSet<u16>>,
    /// Whether text was shown in the font the stream is drawn in.
    pub(super) inherited: bool,
}

/// Reads `content` as [`Read`] says. `two_bytes` answers which font
/// resources have two-byte codes: a composite font under `/Identity-H`,
/// whose codespace is two bytes throughout (9.7.5.2).
pub(super) fn shown(content: &[u8], read: Read<'_>, two_bytes: impl Fn(&[u8]) -> bool) -> Shown {
    let (spans, mut current) = match read {
        Read::Page(spans) => (Some(spans), Current::Unset),
        Read::Whole => (None, Current::Inherited),
    };
    // Each font named once, with whether its codes are two bytes and the
    // codes shown in it, so a saved state is a number rather than a name;
    // and each name's number.
    let mut fonts: Vec<(Vec<u8>, bool, BTreeSet<u16>)> = Vec::new();
    let mut numbers: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut inherited = false;
    let mut saved: Vec<Current> = Vec::new();
    // The operands since the last operator that an operator here reads: the
    // last name, `Tf`'s font, and the strings, `TJ`'s array's or the last.
    let mut name: Option<Vec<u8>> = None;
    let mut strings: Vec<Vec<u8>> = Vec::new();
    // The first of `spans` not ending at or before the operator read.
    let mut span = 0usize;
    let mut lexer = Lexer::new(content);
    let mut sink = WarningSink::new();
    loop {
        let token = lexer.next_token(&mut sink);
        let start = usize::try_from(token.start).unwrap_or(usize::MAX);
        let end = usize::try_from(token.end).unwrap_or(usize::MAX);
        match token.kind {
            TokenKind::Eof => break,
            TokenKind::Name(read) => name = Some(read),
            TokenKind::String(read) => strings.push(read.bytes),
            TokenKind::Unknown => {
                let operator = content.get(start..end).unwrap_or_default();
                match operator {
                    b"Tf" => {
                        if let Some(font) = name.take() {
                            let at = match numbers.get(&font) {
                                Some(&at) => at,
                                None => {
                                    let wide = two_bytes(&font);
                                    numbers.insert(font.clone(), fonts.len());
                                    fonts.push((font, wide, BTreeSet::new()));
                                    fonts.len() - 1
                                }
                            };
                            current = Current::Font(at);
                        }
                    }
                    b"q" => saved.push(current),
                    b"Q" => {
                        if let Some(restored) = saved.pop() {
                            current = restored;
                        }
                    }
                    b"Tj" | b"'" | b"\"" | b"TJ" => {
                        let read = match spans {
                            None => true,
                            Some(spans) => {
                                while spans.get(span).is_some_and(|range| range.end <= start) {
                                    span += 1;
                                }
                                spans.get(span).is_some_and(|range| range.start <= start)
                            }
                        };
                        let shows = if operator == b"TJ" {
                            &strings[..]
                        } else {
                            strings.last().map(std::slice::from_ref).unwrap_or_default()
                        };
                        match current {
                            _ if !read || shows.is_empty() => {}
                            Current::Unset => {}
                            Current::Inherited => inherited = true,
                            Current::Font(at) => {
                                if let Some((_, wide, codes)) = fonts.get_mut(at) {
                                    for string in shows {
                                        if *wide {
                                            codes.extend(string.chunks_exact(2).map(|pair| {
                                                u16::from_be_bytes([pair[0], pair[1]])
                                            }));
                                        } else {
                                            codes
                                                .extend(string.iter().map(|&code| u16::from(code)));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    b"BI" => {
                        let rest = content.get(end..).unwrap_or_default();
                        lexer.seek(u64::try_from(end + inline_image_end(rest)).unwrap_or(u64::MAX));
                    }
                    _ => {}
                }
                name = None;
                strings.clear();
            }
            _ => {}
        }
    }
    Shown {
        codes: crate::sort::map(
            fonts
                .into_iter()
                .filter(|(.., codes)| !codes.is_empty())
                .map(|(font, _, codes)| (font, codes)),
        ),
        inherited,
    }
}

/// How far past `BI` an inline image runs: to the end of the first `EI` with
/// white space before it and white space, a delimiter or the end after it
/// (8.9.7), the rule this repository's interpreter skips the data by; the
/// whole of `rest` where there is none.
fn inline_image_end(rest: &[u8]) -> usize {
    let mut at = 0usize;
    while at + 1 < rest.len() {
        if rest[at] == b'E' && rest[at + 1] == b'I' {
            let before = at == 0 || is_whitespace(rest[at - 1]);
            let after = rest
                .get(at + 2)
                .is_none_or(|&b| b.is_ascii_whitespace() || is_delimiter(b));
            if before && after {
                return at + 2;
            }
        }
        at += 1;
    }
    rest.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `content` read whole, with `C0` the one font of two-byte codes.
    fn whole(content: &[u8]) -> Shown {
        shown(content, Read::Whole, |font| font == b"C0")
    }

    fn codes(pairs: &[(&[u8], &[u16])]) -> BTreeMap<Vec<u8>, BTreeSet<u16>> {
        pairs
            .iter()
            .map(|(font, codes)| (font.to_vec(), codes.iter().copied().collect()))
            .collect()
    }

    /// **Each text-showing operator shows its strings in the font `Tf` left
    /// selected**: `Tj` and `'` their string, `"` the string after its two
    /// spacings, `TJ` every string of its array and none of its numbers; a
    /// two-byte font's strings two bytes a code, high byte first.
    #[test]
    fn each_operator_shows_its_strings_in_the_selected_font() {
        let found = whole(
            b"BT /F0 12 Tf (AB) Tj (C) ' 1 2 (D) \" [(E) -120 (F)] TJ \
              /C0 9 Tf <00410102> Tj ET",
        );
        assert_eq!(
            found.codes,
            codes(&[(b"F0", &[65, 66, 67, 68, 69, 70]), (b"C0", &[0x41, 0x0102])])
        );
        assert!(!found.inherited);
    }

    /// **`q` and `Q` save and restore the font**, and a `Q` with nothing
    /// saved restores nothing.
    #[test]
    fn the_font_is_saved_and_restored_with_the_graphics_state() {
        let found = whole(
            b"/F0 12 Tf q /F1 12 Tf (a) Tj Q (b) Tj Q (c) Tj \
              q q /F2 1 Tf Q (d) Tj Q",
        );
        assert_eq!(
            found.codes,
            codes(&[(b"F0", &[98, 99, 100]), (b"F1", &[97])])
        );
    }

    /// **Text shown before any `Tf` is in the font the content is drawn
    /// in** for content read whole, and in no font on a page.
    #[test]
    fn text_before_a_font_is_the_inherited_font_or_none() {
        let found = whole(b"BT (a) Tj ET");
        assert!(found.inherited);
        assert!(found.codes.is_empty());
        let content = b"BT (a) Tj ET";
        let all = 0..content.len();
        let page = shown(content, Read::Page(std::slice::from_ref(&all)), |_| false);
        assert_eq!(page, Shown::default(), "a page's first font is none");
        // A `Q` past everything saved leaves the font as it is.
        assert!(whole(b"q /F0 1 Tf Q Q (x) Tj").inherited);
    }

    /// **What is not an operator is not read as one**: a comment, a string
    /// spelling an operator, a name spelling one, and an inline image's data,
    /// which runs to its `EI`.
    #[test]
    fn strings_comments_names_and_inline_images_are_not_operators() {
        let found = whole(
            b"/F0 12 Tf % (no) Tj\n\
              (Tf) /Tj (yes) Tj \
              BI /W 6 /H 1 /CS /G /BPC 8 ID (G) Tj EI (after) Tj",
        );
        let shown: Vec<u16> = b"yesafter".iter().map(|&b| u16::from(b)).collect();
        assert_eq!(found.codes, codes(&[(b"F0", &shown)]));
        // The data runs to the end where no `EI` closes it.
        assert!(whole(b"/F0 1 Tf BI /W 1 ID (x) Tj").codes.is_empty());
    }

    /// **A page reads only the operators `raw` wrote**, while every operator
    /// moves the font: the builder's own `Tf` selects the font a raw `Tj`
    /// after it shows in, and the builder's own `Tj` is not read again.
    #[test]
    fn a_page_reads_only_its_raw_spans() {
        let builder = b"BT /F0 12 Tf (A) Tj ET\n".as_slice();
        let raw = b"BT (B) Tj ET".as_slice();
        let mut content = builder.to_vec();
        content.extend_from_slice(raw);
        let raw_span = builder.len()..content.len();
        let found = shown(
            &content,
            Read::Page(std::slice::from_ref(&raw_span)),
            |_| false,
        );
        assert_eq!(found.codes, codes(&[(b"F0", &[66])]));
        assert!(!found.inherited);
    }

    /// **A name operand is `Tf`'s only when it is the font's operand**: the
    /// last name before the operator, read afresh after every operator.
    #[test]
    fn a_font_is_the_name_before_tf() {
        let found = whole(b"/F0 12 Tf /Span <</MCID 0>> BDC (x) Tj EMC /F1 Tf (y) Tj");
        assert_eq!(found.codes, codes(&[(b"F0", &[120]), (b"F1", &[121])]));
        // `Tf` with no name operand selects nothing new.
        let found = whole(b"/F0 12 Tf 12 Tf (z) Tj");
        assert_eq!(found.codes, codes(&[(b"F0", &[122])]));
    }
}
