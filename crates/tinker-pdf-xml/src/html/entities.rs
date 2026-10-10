//! HTML §13.5's named character references: the standard's 2 231 names,
//! compiled by `build.rs` from the vendored `data/html-entities/entities.json`.
//!
//! **This table is not the XML reader's.** That one is XHTML 1.0's 253 names
//! and maps each to one code point, which is what keeps decoded XML no longer
//! than its source; this one is HTML's and is consulted only by the HTML
//! tokenizer. Ninety-three of its names expand to two code points (`&nGt;` is
//! U+226B U+20D2), and every one of them is at least five bytes of source for
//! at most six of UTF-8 — so a reference can lengthen text by a fifth and no
//! more, a ratio and never a multiplier. `build.rs` checks that bound for
//! every row.

// `HTML_ENTITIES`: `(name, first, second)` sorted by the name's bytes, the
// name without its `&` and with its `;` where the standard gives one, and
// `second` 0 for a reference that is one code point.
include!(concat!(env!("OUT_DIR"), "/html_entities.rs"));

/// The characters `name` stands for, if the standard names it. `name` is
/// what follows the `&`, with its `;` if it has one: `amp;` and `amp` are
/// both names, `ampx` is neither.
pub(crate) fn lookup(name: &str) -> Option<(char, Option<char>)> {
    let at = HTML_ENTITIES
        .binary_search_by(|(entry, _, _)| entry.as_bytes().cmp(name.as_bytes()))
        .ok()?;
    let &(_, first, second) = HTML_ENTITIES.get(at)?;
    let first = char::from_u32(first)?;
    let second = if second == 0 {
        None
    } else {
        char::from_u32(second)
    };
    Some((first, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_standards_and_is_sorted() {
        assert_eq!(HTML_ENTITIES.len(), 2231);
        assert!(HTML_ENTITIES
            .windows(2)
            .all(|pair| pair[0].0.as_bytes() < pair[1].0.as_bytes()));
        assert_eq!(lookup("amp;"), Some(('&', None)));
        assert_eq!(lookup("amp"), Some(('&', None)), "a legacy name");
        assert_eq!(lookup("nGt;"), Some(('\u{226B}', Some('\u{20D2}'))));
        assert_eq!(lookup("nGt"), None, "not every name has a legacy form");
        assert_eq!(
            lookup("CounterClockwiseContourIntegral;"),
            Some(('\u{2233}', None))
        );
        // HTML's lang is U+27E8 where XHTML 1.0's is U+2329: the two tables
        // answer two documents' questions.
        assert_eq!(lookup("lang;"), Some(('\u{27E8}', None)));
    }
}
