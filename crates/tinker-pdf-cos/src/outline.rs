//! The document outline (12.3.3), metadata (14.3.3) and page labels (12.4.2).

use crate::dest::{Destination, Resolver};
use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{ObjRef, Object};
use crate::text_string::{decode_text_string, parse_date, Date};
use crate::trees;
use crate::warn::WarningKind;
use std::collections::HashSet;

/// One entry in the outline tree.
#[derive(Clone, Debug)]
pub struct OutlineItem {
    /// The entry's visible text, already decoded.
    pub title: String,
    /// Where it goes, if anywhere. An entry that is only a heading has none.
    pub destination: Option<Destination>,
    /// Whether the entry was saved expanded (`/Count` positive).
    pub open: bool,
    /// Nested entries.
    pub children: Vec<OutlineItem>,
}

impl OutlineItem {
    /// Flattens the tree to `(depth, item)` pairs in reading order.
    #[must_use]
    pub fn flatten(items: &[OutlineItem]) -> Vec<(u32, &OutlineItem)> {
        let mut out = Vec::new();
        fn go<'a>(items: &'a [OutlineItem], depth: u32, out: &mut Vec<(u32, &'a OutlineItem)>) {
            for item in items {
                out.push((depth, item));
                go(&item.children, depth + 1, out);
            }
        }
        go(items, 0, &mut out);
        out
    }
}

/// Reads the outline tree. A document without one has an empty outline, which
/// is not an error.
#[must_use]
pub fn outline(doc: &CosDocument) -> Vec<OutlineItem> {
    let resolver = Resolver::new(doc);
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let outlines = doc.intern(b"Outlines");
    let Some(root) = catalog.get_ref(outlines) else {
        return Vec::new();
    };

    let first = doc.intern(b"First");
    let mut visited = HashSet::new();
    match doc.get(root) {
        Ok(node) => match node.as_dict().and_then(|d| d.get_ref(first)) {
            Some(child) => siblings(doc, &resolver, child, 0, &mut visited),
            None => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// Walks one chain of `/Next` siblings, recursing into each one's `/First`.
fn siblings(
    doc: &CosDocument,
    resolver: &Resolver,
    start: ObjRef,
    depth: u32,
    visited: &mut HashSet<u32>,
) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    if depth > limits::MAX_NEST_DEPTH {
        doc.warn(WarningKind::OutlineTruncated);
        return out;
    }

    let title_key = doc.intern(b"Title");
    let first = doc.intern(b"First");
    let next = doc.intern(b"Next");
    let count = Name::COUNT;

    let mut current = Some(start);
    while let Some(node) = current {
        if out.len() >= limits::MAX_TREE_ENTRIES {
            doc.warn(WarningKind::OutlineTruncated);
            break;
        }
        // A /Next chain that loops back is a real corruption, not a rarity.
        if !visited.insert(node.num) {
            doc.warn(WarningKind::OutlineCycle);
            break;
        }

        let Ok(object) = doc.get(node) else { break };
        let Some(dict) = object.as_dict() else { break };

        let title = doc
            .resolve_key(dict, title_key)
            .as_string()
            .map(|s| decode_text_string(&s.bytes))
            .unwrap_or_default();

        let children = match dict.get_ref(first) {
            Some(child) => siblings(doc, resolver, child, depth + 1, visited),
            None => Vec::new(),
        };

        out.push(OutlineItem {
            title,
            destination: resolver.entry_target(dict),
            // 12.3.3: a positive /Count means the entry was open when saved.
            open: dict.get_int(count).is_some_and(|c| c > 0),
            children,
        });

        current = dict.get_ref(next);
    }

    out
}

/// The `/Info` dictionary (14.3.3), decoded.
///
/// Every field is absent rather than empty when the document does not define
/// it — a blank title and no title are different facts, and only one of them
/// should reach a user interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// `/Title`.
    pub title: Option<String>,
    /// `/Author`.
    pub author: Option<String>,
    /// `/Subject`.
    pub subject: Option<String>,
    /// `/Keywords`.
    pub keywords: Option<String>,
    /// `/Creator`: the application that authored the original document.
    pub creator: Option<String>,
    /// `/Producer`: the application that wrote the PDF.
    pub producer: Option<String>,
    /// `/CreationDate`, as written.
    pub creation_date: Option<String>,
    /// `/ModDate`, as written.
    pub modification_date: Option<String>,
}

impl Metadata {
    /// `/CreationDate` parsed, when it parses.
    #[must_use]
    pub fn created(&self) -> Option<Date> {
        self.creation_date.as_deref().and_then(parse_date)
    }

    /// `/ModDate` parsed, when it parses.
    #[must_use]
    pub fn modified(&self) -> Option<Date> {
        self.modification_date.as_deref().and_then(parse_date)
    }
}

/// Reads `/Info`.
///
/// 7.5.5 puts the information dictionary in the trailer, and that is looked at
/// first. Some producers — MuPDF among them — write it into the catalog
/// instead, so a document that has metadata but no trailer entry still reports
/// it rather than appearing blank.
#[must_use]
pub fn metadata(doc: &CosDocument) -> Metadata {
    let from_trailer = doc
        .trailer()
        .get_ref(Name::INFO)
        .and_then(|r| doc.get(r).ok());

    let catalog = doc.catalog();
    let from_catalog = catalog.as_ref().and_then(|c| {
        let value = doc.resolve_key(c, Name::INFO);
        value.as_dict().is_some().then_some(value)
    });

    let Some(object) = from_trailer.or(from_catalog) else {
        return Metadata::default();
    };
    let Some(dict) = object.as_dict() else {
        return Metadata::default();
    };

    let field = |key: &[u8]| -> Option<String> {
        let name = doc.intern(key);
        let value = doc.resolve_key(dict, name);
        let text = value.as_string().map(|s| decode_text_string(&s.bytes))?;
        // Absent beats empty once this reaches a caller.
        (!text.trim().is_empty()).then_some(text)
    };

    Metadata {
        title: field(b"Title"),
        author: field(b"Author"),
        subject: field(b"Subject"),
        keywords: field(b"Keywords"),
        creator: field(b"Creator"),
        producer: field(b"Producer"),
        creation_date: field(b"CreationDate"),
        modification_date: field(b"ModDate"),
    }
}

/// The document's version, as "PDF 1.7".
///
/// 7.5.2 puts it in the header; 7.7.2 lets the catalog's `/Version` override
/// it, which is how an incremental update raises the version of a file whose
/// header it must not touch.
#[must_use]
pub fn version_string(doc: &CosDocument) -> Option<String> {
    let from_catalog = doc
        .catalog()
        .and_then(|c| match c.get(doc.intern(b"Version")) {
            Some(Object::Name(n)) => doc.name_bytes(*n).map(|b| b.to_vec()),
            _ => None,
        })
        .and_then(|b| String::from_utf8(b).ok());

    let version = from_catalog.or_else(|| doc.header_version())?;
    Some(format!("PDF {version}"))
}

/// How a run of pages is numbered (12.4.2, Table 159).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelStyle {
    /// `/D`: 1, 2, 3.
    Decimal,
    /// `/R`: I, II, III.
    RomanUpper,
    /// `/r`: i, ii, iii.
    RomanLower,
    /// `/A`: A, B, ... Z, AA, BB.
    LettersUpper,
    /// `/a`: a, b, ... z, aa, bb.
    LettersLower,
    /// No style: the prefix alone, repeated.
    None,
}

/// Page labels for every page, in order.
///
/// Empty when the document defines none, which is the common case.
#[must_use]
pub fn page_labels(doc: &CosDocument, page_count: u32) -> Vec<String> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Some(tree) = catalog.get_ref(doc.intern(b"PageLabels")) else {
        return Vec::new();
    };

    let entries = trees::number_tree(doc, tree);
    if entries.is_empty() {
        return Vec::new();
    }

    let s_key = doc.intern(b"S");
    let p_key = doc.intern(b"P");
    let st_key = doc.intern(b"St");

    let mut out = Vec::with_capacity(page_count as usize);
    for index in 0..page_count {
        let Some(entry) = trees::lookup_number_range(&entries, i64::from(index)) else {
            // Pages before the first labelled range have no label at all.
            out.push(String::new());
            continue;
        };
        let resolved = doc.resolve(entry);
        let Some(dict) = resolved.as_dict() else {
            out.push(String::new());
            continue;
        };

        let style = match dict.get(s_key) {
            Some(Object::Name(n)) => match doc.name_bytes(*n).as_deref() {
                Some(b"D") => LabelStyle::Decimal,
                Some(b"R") => LabelStyle::RomanUpper,
                Some(b"r") => LabelStyle::RomanLower,
                Some(b"A") => LabelStyle::LettersUpper,
                Some(b"a") => LabelStyle::LettersLower,
                _ => LabelStyle::None,
            },
            _ => LabelStyle::None,
        };

        let prefix = doc
            .resolve_key(dict, p_key)
            .as_string()
            .map(|s| decode_text_string(&s.bytes))
            .unwrap_or_default();

        // The range's own start, plus how far into the range this page is.
        let start = dict.get_int(st_key).unwrap_or(1).max(1);
        let range_start = entries
            .iter()
            .rev()
            .find(|(k, _)| *k <= i64::from(index))
            .map_or(0, |(k, _)| *k);
        let number = start + (i64::from(index) - range_start);

        out.push(format!("{prefix}{}", format_number(style, number)));
    }

    out
}

fn format_number(style: LabelStyle, n: i64) -> String {
    match style {
        LabelStyle::None => String::new(),
        LabelStyle::Decimal => n.to_string(),
        LabelStyle::RomanUpper => roman(n),
        LabelStyle::RomanLower => roman(n).to_lowercase(),
        LabelStyle::LettersUpper => letters(n).to_uppercase(),
        LabelStyle::LettersLower => letters(n),
    }
}

/// Roman numerals, additive-subtractive, for the range labels actually use.
fn roman(n: i64) -> String {
    if n <= 0 {
        return String::new();
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    // Beyond a few thousand the notation stops being useful; cap rather than
    // emit a page of M's for a hostile /St.
    let mut remaining = n.min(100_000);
    let mut out = String::new();
    for (value, glyph) in TABLE {
        while remaining >= value {
            out.push_str(glyph);
            remaining -= value;
        }
    }
    out
}

/// 12.4.2 Table 159: A to Z, then AA to ZZ, then AAA — the letter repeats
/// rather than carrying like a base-26 digit.
fn letters(n: i64) -> String {
    if n <= 0 {
        return String::new();
    }
    let zero_based = (n - 1) as u64;
    let letter = char::from(b'a' + (zero_based % 26) as u8);
    let repeats = (zero_based / 26 + 1).min(64) as usize;
    core::iter::repeat_n(letter, repeats).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_numerals_match_the_usual_forms() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(9), "IX");
        assert_eq!(roman(14), "XIV");
        assert_eq!(roman(40), "XL");
        assert_eq!(roman(1990), "MCMXC");
        assert_eq!(roman(0), "", "there is no zero");
        assert_eq!(roman(-5), "");
    }

    #[test]
    fn letters_repeat_rather_than_carry() {
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa", "not 'ba'");
        assert_eq!(letters(52), "zz");
        assert_eq!(letters(53), "aaa");
        assert_eq!(letters(0), "");
    }

    #[test]
    fn styles_format_as_the_table_says() {
        assert_eq!(format_number(LabelStyle::Decimal, 7), "7");
        assert_eq!(format_number(LabelStyle::RomanUpper, 7), "VII");
        assert_eq!(format_number(LabelStyle::RomanLower, 7), "vii");
        assert_eq!(format_number(LabelStyle::LettersUpper, 27), "AA");
        assert_eq!(format_number(LabelStyle::LettersLower, 27), "aa");
        assert_eq!(
            format_number(LabelStyle::None, 7),
            "",
            "with no style the prefix stands alone"
        );
    }

    #[test]
    fn a_hostile_start_value_does_not_hang() {
        let _ = format_number(LabelStyle::RomanUpper, i64::MAX);
        let _ = format_number(LabelStyle::LettersLower, i64::MAX);
    }
}
