//! What milestone 1 has to measure about a book, and nothing that reads one.
//!
//! Gap 31's milestone 1 is scheduled before any EPUB code exists, for gap 30's
//! reason: gap 29 closed having never opened a `.cbz` a real archiver wrote,
//! and gap 30 fixed that by buying eight genuine packages *first*. So the
//! censuses this milestone owes — the doctype shapes, the named character
//! references, the CSS properties, the `mimetype` rule — are computed here, out
//! of bytes, by code that exists only to count.
//!
//! **None of this is a parser and none of it becomes one.** Milestone 2 writes
//! the doctype mode inside `tinker-pdf-xml` and milestone 6 writes the CSS
//! tokenizer; both have specifications to satisfy and bounds to enforce, and
//! neither will reuse a line of this. What these functions are for is answering
//! the three questions the plan says milestone 1 must answer before milestone 2
//! chooses between three options for named character references. Each is
//! deliberately the *crudest* thing that can answer its question, and each says
//! so where it is wrong.
//!
//! # Why the allow
//!
//! Three test binaries include this module — `epub.rs` over the committed
//! corpus, `epub_fetched.rs` over the one that cannot be committed, and
//! milestone 3's `epub_ocf.rs` over containers no producer would write — and
//! each compiles its own copy. The committed corpus has no encrypted font and
//! the fetched one does, so the three use different subsets and each reports
//! the others' as dead.

#![allow(
    dead_code,
    reason = "shared by three test binaries; each uses a different subset"
)]

use tinker_pdf_zip::{Archive, Entry, Limits, Method};

// ---- reading a book ---------------------------------------------------------

/// Every entry of a book, through **this repository's own** ZIP reader.
///
/// Gap 30's milestone 1 made the same choice and recorded why: an inventory
/// produced by the tool that wrote the file cannot disagree with it. Reading
/// the corpus through `tinker-pdf-zip` is the only way a cross-check between
/// two independent readers is possible at all.
pub fn entries(bytes: &[u8]) -> Vec<Entry> {
    Archive::open(bytes, &Limits::DEFAULT)
        .expect("a book in the corpus is not readable as a ZIP")
        .entries()
        .to_vec()
}

/// One entry's bytes by name, inflated, or `None` when the book has no such
/// entry.
pub fn read(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut archive = Archive::open(bytes, &Limits::DEFAULT).ok()?;
    let index = archive.entries().iter().position(|e| e.name == name)?;
    archive.read(index).ok().map(|c| c.into_owned())
}

/// One entry's bytes by directory index.
pub fn read_at(bytes: &[u8], index: usize) -> Option<Vec<u8>> {
    let mut archive = Archive::open(bytes, &Limits::DEFAULT).ok()?;
    archive.read(index).ok().map(|c| c.into_owned())
}

/// A method as the inventory spells it.
pub fn method_name(method: Method) -> String {
    match method {
        Method::Stored => "stored".to_owned(),
        Method::Deflated => "deflate".to_owned(),
        Method::Other(code) => format!("method-{code}"),
    }
}

// ---- OCF 3.3 section 4.3.2 --------------------------------------------------

/// What §4.3.2 asks of the `mimetype` entry, as four separate answers.
///
/// Four fields and not one boolean, because the four are independent and a
/// single `is_valid` cannot say which of them a producer got wrong. A book
/// whose `mimetype` is deflated and a book whose `mimetype` is second are two
/// different bugs in two different producers, and milestone 3 warns about them
/// separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MimetypeVerdict {
    /// There is an entry named exactly `mimetype`.
    pub present: bool,
    /// It is **first by `header_offset`**, which is what "the first file in the
    /// ZIP archive" means.
    ///
    /// Not `index == 0`. The central directory's order is the writer's to
    /// choose and physical order is the archive's; they agree in every book
    /// measured so far, which is exactly why an implementation that checks the
    /// wrong one passes. Milestone 3's exit criterion names this explicitly and
    /// owes it a fixture where the two disagree.
    pub first_by_offset: bool,
    /// It would also pass a check written against `index == 0`.
    ///
    /// Carried so a test can assert the two questions are **currently
    /// indistinguishable** across the corpus — which is the evidence that the
    /// distinction has to come from a fixture rather than from a real book.
    pub first_by_index: bool,
    /// Method 0.
    pub stored: bool,
    /// The contents are exactly `application/epub+zip`, 20 bytes.
    pub exact_bytes: bool,
}

/// Measures §4.3.2 over one book.
pub fn mimetype_verdict(bytes: &[u8]) -> MimetypeVerdict {
    let entries = entries(bytes);
    let Some(entry) = entries.iter().find(|e| e.name == "mimetype") else {
        return MimetypeVerdict {
            present: false,
            first_by_offset: false,
            first_by_index: false,
            stored: false,
            exact_bytes: false,
        };
    };
    let lowest = entries.iter().map(|e| e.header_offset).min();
    let payload = read(bytes, "mimetype");
    MimetypeVerdict {
        present: true,
        first_by_offset: lowest == Some(entry.header_offset),
        first_by_index: entry.index == 0,
        stored: entry.method == Method::Stored,
        exact_bytes: payload.as_deref() == Some(b"application/epub+zip".as_slice()),
    }
}

/// OCF 3.3 §4.2.6.3's reserved names inside `META-INF`.
///
/// Six, and the set is closed. A book that puts a seventh file there is not
/// using a reserved name, and the census below counts how often that happens —
/// which turns out to be more often than the specification's tone suggests.
pub const RESERVED_META_INF: &[&str] = &[
    "META-INF/container.xml",
    "META-INF/encryption.xml",
    "META-INF/manifest.xml",
    "META-INF/metadata.xml",
    "META-INF/rights.xml",
    "META-INF/signatures.xml",
];

/// Whether an entry name is a content document by its **extension**.
///
/// An extension is a claim and the manifest's `media-type` is the fact, which
/// is the same distinction `cbz::ImageFormat`'s header makes about images. The
/// manifest cannot be read until milestone 4, so the census uses the claim and
/// records that it did. It matters: one of the two producers in the committed
/// corpus writes `.html` and the other writes `.xhtml`, so a census that keyed
/// on `.xhtml` alone would report one producer's books as having no content
/// documents at all.
pub fn is_content_document(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

/// Whether an entry name is a stylesheet by its extension. Same caveat.
pub fn is_stylesheet(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".css")
}

// ---- the doctype census -----------------------------------------------------

/// Which of the shapes a document type declaration is in.
///
/// The five values are the rows of the plan's own settlement table in
/// [The doctype collision], plus the split between the two quote characters —
/// because *"the single-quoted XHTML 1.1 form"* is named in milestone 2's exit
/// criteria and a census that folded the quotes together could not tell
/// milestone 2 whether it had ever been seen.
///
/// [The doctype collision]: ../../../../docs/plans/gaps/31-epub.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doctype {
    /// No declaration at all.
    None,
    /// `<!DOCTYPE html>` — a name and nothing else.
    Bare,
    /// `<!DOCTYPE html PUBLIC "…" "…">`, with the quote character used and the
    /// public identifier read out.
    Public { quote: char, id: String },
    /// `<!DOCTYPE html SYSTEM "…">`.
    System { quote: char },
    /// An internal subset: a `[` before the terminating `>`. This is where all
    /// four of gap 30's committed XML bombs live, and milestone 2 refuses it by
    /// its own name.
    InternalSubset,
    /// A `<!DOCTYPE` that never terminates.
    Unterminated,
}

/// Classifies the **first** document type declaration in a document.
///
/// The scan is the one milestone 2 has to write and no more of it: find
/// `<!DOCTYPE` case-insensitively, then walk to the matching `>` while tracking
/// string literals so that a `>` inside one does not end the declaration, and
/// stop at a `[`. Nothing is expanded, no entity table exists, and the
/// literals are read only far enough to say which quote character opened them.
///
/// What it does **not** do is check that the declaration is in the prolog. A
/// document with the text `<!DOCTYPE` inside a `<code>` element would be
/// misread, and the corpus is small enough that the census prints its per-file
/// answers for a human to disbelieve.
pub fn classify_doctype(text: &str) -> Doctype {
    let bytes = text.as_bytes();
    let Some(start) = find_ascii_case_insensitive(bytes, b"<!DOCTYPE") else {
        return Doctype::None;
    };
    let mut i = start + b"<!DOCTYPE".len();
    let mut quote: Option<u8> = None;
    // What the declaration said, minus the literals' delimiters, so the public
    // identifier can be pulled back out of it.
    let mut literals: Vec<(u8, String)> = Vec::new();
    let mut current = String::new();
    let mut keyword_public = false;
    let mut keyword_system = false;
    let mut word = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    literals.push((q, core::mem::take(&mut current)));
                    quote = None;
                } else {
                    current.push(b as char);
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'[' => return Doctype::InternalSubset,
                b'>' => {
                    let quote_char = literals.first().map_or('"', |(q, _)| *q as char);
                    return if keyword_public {
                        Doctype::Public {
                            quote: quote_char,
                            id: literals.first().map(|(_, s)| s.clone()).unwrap_or_default(),
                        }
                    } else if keyword_system {
                        Doctype::System { quote: quote_char }
                    } else {
                        Doctype::Bare
                    };
                }
                _ => {
                    if b.is_ascii_alphabetic() {
                        word.push(b.to_ascii_uppercase() as char);
                    } else {
                        if word == "PUBLIC" {
                            keyword_public = true;
                        } else if word == "SYSTEM" {
                            keyword_system = true;
                        }
                        word.clear();
                    }
                }
            },
        }
        i += 1;
    }
    Doctype::Unterminated
}

/// A short name for a shape, for the census table.
pub fn doctype_shape(doctype: &Doctype) -> &'static str {
    match doctype {
        Doctype::None => "none",
        Doctype::Bare => "bare",
        Doctype::Public { quote: '\'', .. } => "public-single-quoted",
        Doctype::Public { .. } => "public-double-quoted",
        Doctype::System { .. } => "system",
        Doctype::InternalSubset => "internal-subset",
        Doctype::Unterminated => "unterminated",
    }
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| {
        haystack[i..i + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

// ---- the named-character-reference census -----------------------------------

/// XML 1.0 §4.6's five predefined entities, and the whole of what a
/// non-validating parser is required to know without a DTD.
pub const XML_PREDEFINED: &[&str] = &["amp", "lt", "gt", "quot", "apos"];

/// Every `&name;` in a document that is neither a numeric reference nor one of
/// XML's five, with a count each.
///
/// This is the measurement that settles the plan's open question: XHTML's named
/// references are declared by the DTD the doctype mode discards, so a document
/// that uses `&nbsp;` without a declaration is not well-formed XML, and the
/// choice between refusing, vendoring ~250 names and vendoring ~2 200 depends
/// on how many real books do it.
///
/// The count is the whole answer only beside [`numeric_references`]: "no book
/// uses `&nbsp;`" and "no book needs a non-ASCII character" are different
/// claims, and only the first of them is about the entity table.
pub fn named_references(text: &str) -> Vec<(String, usize)> {
    let bytes = text.as_bytes();
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        // A reference name is at most this long in any published table; the cap
        // is what stops a bare `&` in prose from swallowing a paragraph.
        let end = (i + 34).min(bytes.len());
        while j < end && bytes[j].is_ascii_alphanumeric() {
            j += 1;
        }
        if j < end && bytes[j] == b';' && j > i + 1 && bytes[i + 1].is_ascii_alphabetic() {
            let name = &text[i + 1..j];
            if !XML_PREDEFINED.contains(&name) {
                match counts.iter_mut().find(|(n, _)| n == name) {
                    Some((_, count)) => *count += 1,
                    None => counts.push((name.to_owned(), 1)),
                }
            }
        }
        i = j.max(i + 1);
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    counts
}

/// How many `&#…;` and `&#x…;` references a document uses.
///
/// The corroboration the plan's sentence *"producers overwhelmingly write
/// `&#160;`"* needs. Without it, a zero from [`named_references`] could mean
/// either "nobody needs the table" or "nobody in this corpus writes a character
/// above ASCII at all", and those recommend opposite things to milestone 2.
pub fn numeric_references(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'&' && bytes[i + 1] == b'#' {
            let mut j = i + 2;
            // `&#x…;` is hexadecimal and `&#…;` is decimal. Accepting any
            // alphanumeric in both is the shortcut that counted `&#xZZ;`, which
            // is not a character reference and is what the census's own test
            // caught on its first run.
            let hex = bytes.get(j).is_some_and(|b| b.eq_ignore_ascii_case(&b'x'));
            if hex {
                j += 1;
            }
            let digits = j;
            while j < bytes.len()
                && (if hex {
                    bytes[j].is_ascii_hexdigit()
                } else {
                    bytes[j].is_ascii_digit()
                })
            {
                j += 1;
            }
            if j > digits && bytes.get(j) == Some(&b';') {
                count += 1;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    count
}

// ---- the CSS property census ------------------------------------------------

/// Every property name that appears on the left of a `:` inside a declaration
/// block, deduplicated.
///
/// A tokenizer this is not, and the plan says what replaces it: milestone 6
/// writes `css-syntax-3`'s real one. What this has to be right about is the
/// **set of names**, because that set is what replaces the plan's
/// forty-one-name list with evidence, and what decides which properties
/// milestone 6's `Known` enum has to hold.
///
/// Comments are stripped first. Everything at brace depth zero is a selector or
/// an at-rule prelude and yields nothing; a `@media` block nests, so depth is
/// counted rather than assumed to be one. A custom property (`--x`) is kept,
/// because "custom properties are a non-goal" is a claim worth being able to
/// check against a number.
///
/// **A name found before a `:` is only committed when the construct ends in a
/// `;` or a `}`.** That is not a nicety: inside `@media screen { a:hover { … } }`
/// the selector `a:hover` sits at depth 1 and reads exactly like a declaration
/// until the `{` arrives. The first run of this census reported `a` as a CSS
/// property across the fetched corpus, which is how the rule got written.
pub fn css_properties(text: &str) -> Vec<String> {
    let stripped = strip_css_comments(text);
    let mut names: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut pending: Option<String> = None;
    for ch in stripped.chars() {
        match ch {
            '{' => {
                // Whatever preceded this was a selector, pseudo-class and all.
                depth += 1;
                pending = None;
                current.clear();
            }
            '}' => {
                depth = depth.saturating_sub(1);
                commit(&mut names, pending.take());
                current.clear();
            }
            ';' => {
                commit(&mut names, pending.take());
                current.clear();
            }
            ':' if depth > 0 && pending.is_none() => {
                pending = Some(current.trim().to_ascii_lowercase());
                current.clear();
            }
            _ => {
                if pending.is_none() {
                    current.push(ch);
                }
            }
        }
    }
    names.sort();
    names
}

fn commit(names: &mut Vec<String>, candidate: Option<String>) {
    if let Some(name) = candidate {
        if is_property_name(&name) && !names.contains(&name) {
            names.push(name);
        }
    }
}

fn is_property_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '-')
}

fn strip_css_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            match find_ascii_case_insensitive(&bytes[i + 2..], b"*/") {
                Some(end) => i += 2 + end + 2,
                None => break,
            }
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

// ---- a container this file built, for the shapes no real book has -----------

/// One entry of a hand-built OCF container.
#[derive(Clone)]
pub struct OcfEntry {
    pub name: String,
    pub data: Vec<u8>,
    /// Method 8 rather than method 0.
    pub deflated: bool,
    /// Bytes for the **local** header's extra area, and only the local one.
    ///
    /// OCF 3.3 §4.3.2's third clause is that the `mimetype` entry carries no
    /// extra field, and it is the local header's field that clause is about:
    /// together with "first" and "uncompressed" it is what puts
    /// `application/epub+zip` at offset 38 of a conforming container. APPNOTE
    /// 4.4.11 lets the two areas differ, so writing one here and not in the
    /// directory is a legal archive rather than a damaged one — and it is the
    /// shape that tells a check written against the local header from one
    /// written against the central directory's copy.
    pub extra: Vec<u8>,
}

impl OcfEntry {
    pub fn stored(name: &str, data: &[u8]) -> OcfEntry {
        OcfEntry {
            name: name.to_owned(),
            data: data.to_vec(),
            deflated: false,
            extra: Vec::new(),
        }
    }

    pub fn deflated(name: &str, data: &[u8]) -> OcfEntry {
        OcfEntry {
            deflated: true,
            ..OcfEntry::stored(name, data)
        }
    }

    pub fn with_extra(mut self, extra: &[u8]) -> OcfEntry {
        self.extra = extra.to_vec();
        self
    }
}

/// Builds an archive whose **physical order is `entries`** and whose **central
/// directory order is `directory`**, as indices into `entries`.
///
/// The two orders are separate parameters because the only interesting fixture
/// in this file is the one where they disagree, and no real book supplies it:
/// all twenty-six measured here put `mimetype` first in both, so a `mimetype`
/// check written against `index == 0` passes every one of them and is still
/// wrong. §4.3.2 is a statement about the file's physical layout and the
/// central directory is a table the writer orders as it pleases.
///
/// Milestone 3 owes the same fixture as an exit criterion. What it is used for
/// here is smaller and prior: proving that [`MimetypeVerdict`]'s two fields can
/// differ at all, so a corpus-wide assertion that they never do is a
/// measurement rather than a tautology.
pub fn ocf_zip(entries: &[OcfEntry], directory: &[usize]) -> Vec<u8> {
    let flags: u16 = 1 << 11; // bit 11: the name is UTF-8.
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    let mut payloads: Vec<Vec<u8>> = Vec::new();

    for entry in entries {
        offsets.push(u32::try_from(out.len()).expect("fixture fits in 32 bits"));
        let payload = if entry.deflated {
            tinker_pdf_filters::deflate(&entry.data)
        } else {
            entry.data.clone()
        };
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&if entry.deflated { 8u16 } else { 0u16 }.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&tinker_pdf_filters::crc32(&entry.data).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entry.extra.len() as u16).to_le_bytes());
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&entry.extra);
        out.extend_from_slice(&payload);
        payloads.push(payload);
    }

    let cd_start = out.len();
    for &i in directory {
        let entry = &entries[i];
        out.extend_from_slice(b"PK\x01\x02");
        out.extend_from_slice(&20u16.to_le_bytes()); // version made by
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&if entry.deflated { 8u16 } else { 0u16 }.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&tinker_pdf_filters::crc32(&entry.data).to_le_bytes());
        out.extend_from_slice(&(payloads[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(&0u16.to_le_bytes()); // comment
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        out.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        out.extend_from_slice(&offsets[i].to_le_bytes());
        out.extend_from_slice(entry.name.as_bytes());
    }
    let cd_size = out.len() - cd_start;

    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // disk with the directory
    out.extend_from_slice(&(directory.len() as u16).to_le_bytes());
    out.extend_from_slice(&(directory.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd_size as u32).to_le_bytes());
    out.extend_from_slice(&(cd_start as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out
}

/// The bytes §4.3.2 requires the `mimetype` entry to hold.
pub const OCF_MEDIA_TYPE: &[u8] = b"application/epub+zip";

// ---- what today does with a book --------------------------------------------

/// What `Document::open` makes of a book right now, as data.
///
/// One shape for both outcomes, so a sweep can tabulate a corpus without
/// branching at every call site — and so a test that expects a refusal cannot
/// accidentally be satisfied by a document that opened.
#[derive(Clone, Debug, PartialEq)]
pub enum TodaysAnswer {
    /// It opened, as a comic archive.
    Opens {
        pages: u32,
        /// Every page's size in points, in page order.
        sizes: Vec<(f64, f64)>,
        /// The stored path each page came from.
        origins: Vec<String>,
        warnings: usize,
        parsed_parts: usize,
        /// How many pages carry a `PageOrigin.defect`.
        defects: usize,
    },
    /// It was refused, with the name.
    Refused(tinker_pdf::ArchiveRefusal),
    /// It failed to open for a reason that is not an archive refusal.
    Other(String),
}

/// Opens a book and records what came back.
pub fn todays_answer(bytes: &[u8]) -> TodaysAnswer {
    match tinker_pdf::Document::open(bytes.to_vec()) {
        Ok(doc) => {
            let report = doc
                .archive()
                .expect("a book that opens today opens through the container path");
            let sizes = (0..doc.page_count())
                .map(|i| doc.page(i).expect("page in range").size())
                .collect();
            TodaysAnswer::Opens {
                pages: doc.page_count(),
                sizes,
                origins: report.pages().iter().map(|o| o.name.clone()).collect(),
                warnings: report.warnings().len(),
                parsed_parts: report.parsed_parts(),
                defects: report.pages().iter().filter(|o| o.defect.is_some()).count(),
            }
        }
        Err(tinker_pdf::OpenError::UnsupportedArchive(why)) => TodaysAnswer::Refused(why),
        Err(other) => TodaysAnswer::Other(format!("{other:?}")),
    }
}
