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
///
/// So `None` means the key is missing, or `/Info` is; `Some("")` means the
/// producer wrote an empty string and meant to. A viewer showing "(untitled)"
/// for the first and an empty field for the second needs both answers, and
/// nothing downstream can recover one from the other.
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
    /// `/Trapped`. `None` when the key is absent, which is not the same
    /// answer as [`Trapped::Unknown`] — see that type.
    pub trapped: Option<Trapped>,
}

/// Whether the document has been trapped (7.7.2, Table 349).
///
/// Trapping is the prepress compensation for press misregistration, and a
/// print workflow needs to know whether it has already been applied before it
/// decides to apply it again.
///
/// The three variants are the three names the table defines, and the type is
/// deliberately not an `Option<bool>`: `/Unknown` is a real answer — the
/// document saying it does not know — as well as the default the table gives
/// for a document that says nothing. `Option<Trapped>` therefore carries two
/// different facts. `None` is "the key is absent"; `Some(Unknown)` is "the
/// document was asked and answered `/Unknown`", which is also what an
/// unrecognised name reads as, because the file did say something and what it
/// said was not one of the three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trapped {
    /// `/True`: the document has been trapped.
    True,
    /// `/False`: it has not.
    False,
    /// `/Unknown`: partly trapped, or the producer would not say. Table 349's
    /// default, and what a name outside the three reads as.
    Unknown,
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
        // Absent is not empty. A key the document does not define is `None`; a
        // key holding `()` is `Some("")`, and whitespace is kept as written
        // because a producer that indented its title meant to. Deciding here
        // that a blank field is no field puts the difference beyond every
        // caller's reach, and only the caller knows what to show for either.
        value.as_string().map(|s| decode_text_string(&s.bytes))
    };

    // /Trapped is a name, not a text string, so it cannot go through the
    // closure above and needs its own reader. Table 349 defines /True, /False
    // and /Unknown; anything else is /Unknown, which is also the default.
    // A value that is not a name at all — a string, a number, a null — is
    // absent, the same answer the closure gives for a /Title that is not a
    // string: a key of the wrong type states nothing.
    let trapped_value = doc.resolve_key(dict, doc.intern(b"Trapped"));
    let trapped = match &*trapped_value {
        Object::Name(name) => Some(match doc.name_bytes(*name).as_deref() {
            Some(b"True") => Trapped::True,
            Some(b"False") => Trapped::False,
            _ => Trapped::Unknown,
        }),
        _ => None,
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
        trapped,
    }
}

/// The version reported by a document that states none this crate can read.
///
/// 1.7 rather than anything lower, because guessing low is the guess that
/// misleads: a caller told "1.0" concludes the file cannot hold what it
/// plainly holds, while a caller told the baseline reads it as it finds it.
const BASELINE_VERSION: &str = "1.7";

/// The document's version, as "PDF 1.7".
///
/// 7.5.2 puts the version in the header. 7.7.2 lets the catalog's `/Version`
/// **raise** it, and only raise it: that entry exists so an incremental update
/// can declare a later version without rewriting header bytes an existing
/// signature covers. So the later of the two is the document's version, and a
/// stale or mistaken `/Version /1.4` cannot demote a file whose header says
/// 1.7 — reporting 1.4 for it would misdescribe the file.
///
/// The two are compared as the `M.N` number pair 7.5.2 spells, never as text:
/// `1.10` is later than `1.9` and sorts before it. A version that does not
/// parse is treated as **absent** rather than as zero on either side, so a
/// catalog holding `/Version /banana` cannot suppress a header that reads
/// perfectly.
///
/// When neither side states a readable version — a repaired file whose header
/// was lost with the bytes in front of it — this reports [`BASELINE_VERSION`]
/// and emits [`WarningKind::HeaderMissing`], because a guess that leaves no
/// trace is indistinguishable from knowledge (ruling 10).
///
/// Reporting a version is not enforcing one. Nothing in this engine refuses a
/// feature for being newer than the version a file claims; leniency requires
/// reading what is there.
#[must_use]
pub fn version_string(doc: &CosDocument) -> String {
    let header = doc.header_version();
    let catalog = catalog_version(doc);
    let header_number = header.as_deref().and_then(version_number);
    let catalog_number = catalog.as_deref().and_then(version_number);

    let stated = match (header_number, catalog_number) {
        // The only case where the catalog wins: it names a later version.
        (Some(h), Some(c)) if c > h => catalog,
        // A readable header stands otherwise, whether the catalog is older,
        // equal, malformed or absent.
        (Some(_), _) => header,
        (None, Some(_)) => catalog,
        (None, None) => None,
    };

    match stated {
        Some(version) => format!("PDF {version}"),
        None => {
            doc.warn(WarningKind::HeaderMissing);
            format!("PDF {BASELINE_VERSION}")
        }
    }
}

/// The catalog's `/Version` (7.7.2), as the name was written.
fn catalog_version(doc: &CosDocument) -> Option<String> {
    let catalog = doc.catalog()?;
    let Some(Object::Name(name)) = catalog.get(doc.intern(b"Version")) else {
        return None;
    };
    String::from_utf8(doc.name_bytes(*name)?.to_vec()).ok()
}

/// A version as the `M.N` pair 7.5.2 defines it, for comparison.
///
/// `None` for anything that is not that pair — and `None` rather than `(0, 0)`
/// deliberately, so an unreadable version loses no comparison it should never
/// have entered.
fn version_number(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.split_once('.')?;
    Some((decimal(major)?, decimal(minor)?))
}

/// A run of ASCII digits as a number.
///
/// Stricter than [`str::parse`] alone, which accepts a leading `+`. A run too
/// long for `u32` is unreadable rather than enormous: saturating it would
/// invent a version no file states, and win a comparison with it.
fn decimal(field: &str) -> Option<u32> {
    if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    field.parse().ok()
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

/// One file attached to the document (7.11.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// The name it was filed under in `/Names /EmbeddedFiles`.
    pub name: String,
    /// `/F` or `/UF`: the filename to offer when saving it out.
    pub filename: String,
    /// `/Desc`, when the producer wrote one.
    pub description: Option<String>,
    /// The embedded file stream, so a caller can read the bytes itself.
    ///
    /// The bytes are deliberately not read here: an attachment may be
    /// enormous, and listing what a document carries should not cost what it
    /// costs to extract it.
    pub stream: Option<ObjRef>,
    /// `/Params /Size`, when declared. Advisory — the stream is the truth.
    pub size: Option<i64>,
}

/// Every file attached to the document, in name order (7.11.4).
///
/// Attachments are how a PDF carries a spreadsheet next to the report made
/// from it, and how some invoicing standards carry their machine-readable
/// half. A reader that cannot list them silently hides content the document
/// plainly contains.
#[must_use]
pub fn attachments(doc: &CosDocument) -> Vec<Attachment> {
    let Some(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let names = doc.resolve_key(&catalog, doc.intern(b"Names"));
    let Some(names) = names.as_dict() else {
        return Vec::new();
    };
    let Some(root) = names.get_ref(doc.intern(b"EmbeddedFiles")) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (name, value) in crate::trees::name_tree(doc, root) {
        let resolved = doc.resolve(&value);
        let Some(spec) = resolved.as_dict() else {
            continue;
        };

        // 7.11.3: /UF is the Unicode filename and /F the byte one. The
        // Unicode form wins where both are present, because the other is in
        // whatever encoding the producer's filesystem used.
        let filename = ["UF", "F"]
            .iter()
            .find_map(|key| {
                doc.resolve_key(spec, doc.intern(key.as_bytes()))
                    .as_string()
                    .map(|s| crate::text_string::decode_text_string(&s.bytes))
            })
            .unwrap_or_default();

        let description = doc
            .resolve_key(spec, doc.intern(b"Desc"))
            .as_string()
            .map(|s| crate::text_string::decode_text_string(&s.bytes));

        // /EF holds the streams, one per naming convention; the same rule
        // applies to which is preferred.
        let ef = doc.resolve_key(spec, doc.intern(b"EF"));
        let stream = ef.as_dict().and_then(|ef| {
            ef.get_ref(doc.intern(b"UF"))
                .or_else(|| ef.get_ref(doc.intern(b"F")))
        });

        let size = stream.and_then(|r| {
            let object = doc.get(r).ok()?;
            let dict = object.as_dict()?;
            let params = doc.resolve_key(dict, doc.intern(b"Params"));
            params.as_dict()?.get_int(doc.intern(b"Size"))
        });

        out.push(Attachment {
            name: crate::text_string::decode_text_string(&name),
            filename,
            description,
            stream,
            size,
        });
    }
    out
}

/// The document's XMP metadata stream, as bytes (14.3.2).
///
/// Returned unparsed. XMP is RDF/XML, parsing it needs an XML reader this
/// engine does not have and should not grow, and a caller that wants it
/// already has one. What matters here is that the bytes are *reachable* —
/// they carry the document identity and rights information that `/Info` does
/// not, and several archival profiles require them.
#[must_use]
pub fn xmp_metadata(doc: &CosDocument) -> Option<Vec<u8>> {
    let catalog = doc.catalog()?;
    let reference = catalog.get_ref(doc.intern(b"Metadata"))?;
    doc.stream_decoded(reference).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page document whose trailer carries `info` verbatim as `/Info`.
    ///
    /// Written out by hand so the `/Info` dictionary is exactly what the test
    /// says it is: the fixtures in `testdata/` are MuPDF's, and no producer
    /// emits the degenerate entries this rule is about.
    fn with_info(info: &str) -> CosDocument {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
4 0 obj\n<< {info} >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R /Info 4 0 R >>\n%%EOF\n"
        );
        CosDocument::open(bytes.into_bytes()).expect("it opens")
    }

    /// The distinction plan 04 calls a contract: a key that is present and
    /// empty is not a key that is missing. A user interface shows "(untitled)"
    /// for one and a blank field for the other, and collapsing them on read
    /// makes that unrecoverable.
    #[test]
    fn a_blank_title_is_not_the_same_as_no_title() {
        assert_eq!(
            metadata(&with_info("/Title ()")).title.as_deref(),
            Some(""),
            "a producer that wrote an empty title wrote one"
        );
        assert_eq!(
            metadata(&with_info("/Author (Ada)")).title,
            None,
            "a key the /Info dictionary does not hold is absent"
        );
    }

    /// Whitespace is the producer's choice, not ours: a title indented to line
    /// up in a table is still that title. Trimming is a caller's decision.
    #[test]
    fn whitespace_survives_untrimmed() {
        assert_eq!(
            metadata(&with_info("/Title (   )")).title.as_deref(),
            Some("   ")
        );
        assert_eq!(
            metadata(&with_info("/Title (  spaced  )")).title.as_deref(),
            Some("  spaced  ")
        );
    }

    /// The rule holds for every field the closure serves, not just `/Title` —
    /// one of them being special would be the same bug with a smaller blast
    /// radius. `/Trapped` is a name rather than a string, so the closure does
    /// not serve it; its own reader is exercised below.
    #[test]
    fn every_info_string_field_tells_empty_from_absent() {
        let all = "/Title () /Author () /Subject () /Keywords () \
                   /Creator () /Producer () /CreationDate () /ModDate ()";
        let present = metadata(&with_info(all));
        for (label, value) in [
            ("title", &present.title),
            ("author", &present.author),
            ("subject", &present.subject),
            ("keywords", &present.keywords),
            ("creator", &present.creator),
            ("producer", &present.producer),
            ("creation date", &present.creation_date),
            ("modification date", &present.modification_date),
        ] {
            assert_eq!(value.as_deref(), Some(""), "{label} was present");
        }

        assert_eq!(metadata(&with_info("/Type /Info")), Metadata::default());
    }

    /// Empty is a string; a name, a number or a null is not, and a caller
    /// asking for text should not be handed the empty one for those.
    #[test]
    fn a_value_that_is_not_a_string_is_absent() {
        assert_eq!(metadata(&with_info("/Title /NotAString")).title, None);
        assert_eq!(metadata(&with_info("/Title 42")).title, None);
        assert_eq!(metadata(&with_info("/Title null")).title, None);
        assert_eq!(metadata(&with_info("/Title [(a) (b)]")).title, None);
    }

    /// No `/Info` at all is the ordinary case, and every field is `None` —
    /// which is the answer the empty string would otherwise have hidden.
    #[test]
    fn a_document_without_an_info_dictionary_reports_nothing() {
        let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes).expect("it opens");
        assert_eq!(metadata(&doc), Metadata::default());
        assert_eq!(metadata(&doc).title, None);
    }

    /// 7.9.2.2: the value is decoded before it is handed over, and a UTF-16BE
    /// string that is nothing but a BOM decodes to the empty string — present,
    /// empty, and not the same as absent.
    #[test]
    fn a_bom_only_string_decodes_to_present_and_empty() {
        assert_eq!(
            metadata(&with_info("/Title <FEFF>")).title.as_deref(),
            Some(""),
        );
    }

    /// A one-page document with `header` as its first line and `catalog`
    /// spliced into the catalog dictionary, so a test can state a header
    /// version and a `/Version` entry independently of each other.
    ///
    /// An empty `header` produces a file with no `%PDF-` marker at all, which
    /// is the repaired-file case: the scanner finds the objects and nothing
    /// states a version.
    fn with_version(header: &str, catalog: &str) -> CosDocument {
        let bytes = format!(
            "{header}\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R {catalog} >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n"
        );
        CosDocument::open(bytes.into_bytes()).expect("it opens")
    }

    /// How many times the document has reported a missing header, which is the
    /// only provenance the version path emits.
    fn header_missing_count(doc: &CosDocument) -> usize {
        doc.warnings()
            .iter()
            .filter(|w| w.kind == WarningKind::HeaderMissing)
            .count()
    }

    /// 7.7.2: the catalog's `/Version` raises the version, it does not set it.
    /// A 1.7 file carrying a stale `/Version /1.4` is a 1.7 file, and saying
    /// 1.4 misdescribes every feature it is allowed to contain.
    #[test]
    fn the_later_of_the_header_and_the_catalog_wins() {
        assert_eq!(
            version_string(&with_version("%PDF-1.7", "/Version /1.4")),
            "PDF 1.7",
            "a stale catalog entry cannot demote the header"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.4", "/Version /1.7")),
            "PDF 1.7",
            "an incremental update raising the version is what /Version is for"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.4", "/Version /1.4")),
            "PDF 1.4",
            "agreement is not a special case"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.4", "")),
            "PDF 1.4",
            "no catalog entry leaves the header alone"
        );
    }

    /// The comparison is on the number pair, not the text. No such version
    /// exists, and that is exactly why a string comparison survives: it is
    /// wrong only in the release where it first matters.
    #[test]
    fn versions_compare_as_numbers_not_as_strings() {
        assert_eq!(
            version_string(&with_version("%PDF-1.9", "/Version /1.10")),
            "PDF 1.10",
            "1.10 is later than 1.9, though it sorts before it"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.10", "/Version /1.9")),
            "PDF 1.10",
            "and the same holds with the two sides swapped"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.7", "/Version /2.0")),
            "PDF 2.0",
            "a major-version bump is later than any 1.x"
        );
    }

    /// An unreadable version states nothing, and stating nothing is not the
    /// same as stating zero: a `/Version /banana` read as 0.0 would lose every
    /// comparison, which is right, but a *header* read as 0.0 would let that
    /// nonsense win. Neither side may be read as a number it does not hold.
    #[test]
    fn an_unparseable_version_is_absent_rather_than_zero() {
        assert_eq!(
            version_string(&with_version("%PDF-1.4", "/Version /banana")),
            "PDF 1.4",
            "a malformed catalog entry does not suppress a good header"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.4", "/Version 42")),
            "PDF 1.4",
            "nor does one that is not a name at all"
        );
        assert_eq!(
            version_string(&with_version("%PDF-banana", "/Version /1.4")),
            "PDF 1.4",
            "and a good catalog entry survives a header that says nothing"
        );
        assert_eq!(
            version_string(&with_version("%PDF-1.4.2", "/Version /1.4")),
            "PDF 1.4",
            "7.5.2 spells one major and one minor; a third field is not a version"
        );
    }

    /// Plan 04: a repaired file whose header is unreadable reports the
    /// baseline rather than nothing, and the warning is what keeps "we
    /// guessed" on the record (ruling 10).
    #[test]
    fn a_file_stating_no_version_reports_the_baseline_and_says_so() {
        // A header that is present but unreadable: the opener finds `%PDF-`
        // and warns about nothing, so the count below is the version path's.
        let doc = with_version("%PDF-banana", "");
        assert_eq!(header_missing_count(&doc), 0, "the opener found a header");
        assert_eq!(version_string(&doc), "PDF 1.7");
        assert_eq!(
            header_missing_count(&doc),
            1,
            "the baseline is a guess and the guess is reported"
        );

        // And with no header at all, where the opener has already warned:
        // what matters is that the version path adds its own.
        let doc = with_version("", "");
        let before = header_missing_count(&doc);
        assert_eq!(version_string(&doc), "PDF 1.7");
        assert_eq!(header_missing_count(&doc), before + 1);
    }

    /// The baseline is a last resort, not an override: a file that states a
    /// version in the one place it survives is describing itself, and a guess
    /// must not outrank it.
    #[test]
    fn a_readable_version_is_never_a_guess() {
        let doc = with_version("%PDF-1.4", "");
        assert_eq!(version_string(&doc), "PDF 1.4");
        assert_eq!(header_missing_count(&doc), 0, "nothing was guessed");
    }

    /// 7.7.2 Table 349's three names, and the absence that is not one of them.
    #[test]
    fn trapped_reads_the_three_names_it_defines() {
        for (written, expected) in [
            ("/Trapped /True", Trapped::True),
            ("/Trapped /False", Trapped::False),
            ("/Trapped /Unknown", Trapped::Unknown),
        ] {
            assert_eq!(
                metadata(&with_info(written)).trapped,
                Some(expected),
                "{written}"
            );
        }
        assert_eq!(
            metadata(&with_info("/Title (t)")).trapped,
            None,
            "a key the document does not hold is absent"
        );
    }

    /// The distinction gap 21 drew for strings, drawn again for this name:
    /// `Unknown` is the document answering, `None` is the document silent. A
    /// name outside the three is an answer — an unrecognised one — and Table
    /// 349 already has the variant for it.
    #[test]
    fn an_unrecognised_trapped_name_is_unknown_not_absent() {
        assert_eq!(
            metadata(&with_info("/Trapped /Nonsense")).trapped,
            Some(Trapped::Unknown),
            "the file said something; what it said was not one of the three"
        );
        // A value of the wrong type states nothing at all, which is what the
        // string fields do with a name and this does with a string.
        assert_eq!(metadata(&with_info("/Trapped (True)")).trapped, None);
        assert_eq!(metadata(&with_info("/Trapped 1")).trapped, None);
        assert_eq!(metadata(&with_info("/Trapped null")).trapped, None);
    }

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
