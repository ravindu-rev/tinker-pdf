//! Document-level structures the catalog carries: page labels (12.4.2),
//! embedded files (7.11.4), the outline (12.3.3) and viewer preferences
//! (12.2).
//!
//! Each is written by a typed setter and read back by the reader this crate
//! already had — `outline::page_labels`, `outline::attachments`,
//! `outline::outline` and `viewer::viewer_preferences` — so a write followed by
//! a read is an equality rather than a translation. A setter that refuses
//! writes nothing: every check runs before the first object is put.

use std::collections::HashSet;

use super::{without, DocumentEditor};
use crate::build::{outline_is_writable, OutlineEntry};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::outline::LabelStyle;
use crate::text_string::{decode_text_string, encode_text_string, Date};
use crate::trees::{self, TreeWriteError};
use crate::viewer::{self, ViewerPreferences};
use crate::write::StreamData;

/// One run of page labels (12.4.2, Table 159): the pages from `first_page` up
/// to the next range's first page are labelled `prefix` followed by a number
/// in `style`, counting from `start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageLabelRange {
    /// The zero-based index of the range's first page — the key it is filed
    /// under in the number tree.
    pub first_page: u32,
    /// How the number is written; [`LabelStyle::None`] writes no number, so
    /// every page of the range is labelled with the prefix alone.
    pub style: LabelStyle,
    /// `/P`: text before the number. `None` writes no `/P`, which reads the
    /// same as an empty one and is not the same file.
    pub prefix: Option<String>,
    /// `/St`: the number of the range's first page. At least 1 (Table 159).
    pub start: u32,
}

/// Why [`DocumentEditor::set_page_labels`] wrote nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageLabelError {
    /// The trailer has no `/Root` naming a catalog to hang the tree on.
    NoCatalog,
    /// No range starts at page 0. 12.4.2: "the tree shall include a value for
    /// page index 0", because a page before the first range has no label and
    /// a reader shows it as nothing at all.
    NoFirstPage,
    /// A range starts at a page the document does not have.
    PastLastPage {
        /// The range's first page.
        first_page: u32,
        /// How many pages the document has.
        pages: u32,
    },
    /// A range numbers from 0 (Table 159: `/St` shall be at least 1).
    StartBelowOne {
        /// The range's first page.
        first_page: u32,
    },
    /// The tree writer refused the ranges: two starting at one page, or more
    /// than a reader walks.
    Tree(TreeWriteError),
}

impl core::fmt::Display for PageLabelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PageLabelError::NoCatalog => f.write_str("the document has no catalog"),
            PageLabelError::NoFirstPage => {
                f.write_str("no range starts at page 0 (12.4.2 requires one)")
            }
            PageLabelError::PastLastPage { first_page, pages } => write!(
                f,
                "a range starts at page {first_page} of a document of {pages}"
            ),
            PageLabelError::StartBelowOne { first_page } => write!(
                f,
                "the range at page {first_page} numbers from 0 (/St is at least 1)"
            ),
            PageLabelError::Tree(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PageLabelError {}

/// A file to embed in the document (7.11.4), filed under `name` in the
/// catalog's `/Names /EmbeddedFiles` tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmbeddedFile {
    /// The key it is filed under, which is what a viewer's attachment panel
    /// lists it by.
    pub name: String,
    /// The file name to offer when it is saved out: `/UF` as a text string,
    /// and `/F` beside it for the readers that predate `/UF` (7.11.3).
    pub filename: String,
    /// `/Desc`.
    pub description: Option<String>,
    /// The stream's `/Subtype`: a MIME type such as `text/csv` (7.11.4
    /// Table 44). Printable ASCII with no spaces, since it is written as a
    /// name.
    pub mime_type: Option<String>,
    /// `/Params /CreationDate`.
    pub created: Option<Date>,
    /// `/Params /ModDate`.
    pub modified: Option<Date>,
    /// The file's bytes, unencoded. `/Params /Size` and the MD5 `/CheckSum`
    /// Table 45 asks for are computed from them.
    pub data: Vec<u8>,
}

/// Why [`DocumentEditor::attach_file`] wrote nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachError {
    /// The trailer has no `/Root` naming a catalog.
    NoCatalog,
    /// A file is already filed under this name. Two entries under one key is
    /// a tree a reader resolves by whichever it reaches first, so which file
    /// the caller meant is theirs to decide.
    NameTaken(String),
    /// The MIME type is empty or carries a byte a name should not: a space,
    /// a control character or anything outside ASCII.
    MimeType(String),
    /// A date with a field out of range (7.9.4): a year outside 0 to 9999, a
    /// month outside 1 to 12, and so on.
    Date(Date),
    /// The tree writer refused the entries — more than a reader walks.
    Tree(TreeWriteError),
}

impl core::fmt::Display for AttachError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AttachError::NoCatalog => f.write_str("the document has no catalog"),
            AttachError::NameTaken(name) => write!(f, "a file is already attached as {name:?}"),
            AttachError::MimeType(mime) => write!(f, "{mime:?} cannot be written as a name"),
            AttachError::Date(date) => write!(f, "{date:?} is not a date 7.9.4 can spell"),
            AttachError::Tree(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AttachError {}

/// A date as 7.9.4 spells it, or `None` when a field is out of the range the
/// syntax has digits for.
///
/// The zone is `Z` for UT, `+HH'mm` or `-HH'mm` for an offset, and absent for
/// a date whose zone is unknown. A document declaring less than 2.0 gets the
/// closing apostrophe ISO 32000-1 wrote (`+HH'mm'`); 2.0 dropped it, and the
/// reader here accepts both.
pub(crate) fn pdf_date(date: &Date, version: (u8, u8)) -> Option<String> {
    let in_range = (0..=9999).contains(&date.year)
        && (1..=12).contains(&date.month)
        && (1..=31).contains(&date.day)
        && date.hour <= 23
        && date.minute <= 59
        && date.second <= 59
        && date.utc_offset_minutes.is_none_or(|m| m.abs() < 24 * 60);
    if !in_range {
        return None;
    }
    let mut out = format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}",
        date.year, date.month, date.day, date.hour, date.minute, date.second
    );
    match date.utc_offset_minutes {
        None => {}
        Some(0) => out.push('Z'),
        Some(minutes) => {
            let sign = if minutes < 0 { '-' } else { '+' };
            let minutes = minutes.unsigned_abs();
            out.push_str(&format!("{sign}{:02}'{:02}", minutes / 60, minutes % 60));
            if version < (2, 0) {
                out.push('\'');
            }
        }
    }
    Some(out)
}

/// `/F` beside `/UF` (7.11.3): the file name in bytes a pre-1.7 reader takes,
/// which is the name itself when it is printable ASCII and the name with every
/// other character replaced by `_` when it is not. `/UF` carries the real one.
fn byte_file_name(name: &str) -> Vec<u8> {
    name.chars()
        .map(|c| match u8::try_from(c) {
            Ok(b) if (0x20..0x7f).contains(&b) => b,
            _ => b'_',
        })
        .collect()
}

impl DocumentEditor {
    /// The catalog's `/Names` dictionary (7.7.4) as this editor has it, and
    /// the object it lives in when it is indirect.
    fn names_dictionary(&self) -> (Dict, Option<ObjRef>) {
        let key = self.intern(b"Names");
        match self.catalog().and_then(|c| c.get(key).cloned()) {
            Some(Object::Ref(r)) => match self.get(r) {
                Some(Object::Dict(dict)) => (dict, Some(r)),
                _ => (Dict::new(), Some(r)),
            },
            Some(Object::Dict(dict)) => (dict, None),
            _ => (Dict::new(), None),
        }
    }

    /// Sets (`Some`) or removes (`None`) one entry of the catalog's `/Names`
    /// dictionary, wherever it lives; creates the dictionary, in the catalog,
    /// when there is none. False when there is no catalog.
    pub(super) fn set_names_entry(&mut self, key: Name, value: Option<Object>) -> bool {
        if self.catalog().is_none() {
            return false;
        }
        let (mut dict, holder) = self.names_dictionary();
        match value {
            Some(value) => {
                dict.insert(key, value);
            }
            None => dict = without(&dict, key),
        }
        match holder {
            Some(r) => {
                self.put(r, Object::Dict(dict));
                true
            }
            None => {
                let names = self.intern(b"Names");
                self.update_catalog(|catalog| {
                    catalog.insert(names, Object::Dict(dict));
                })
            }
        }
    }

    /// Deletes the nodes of a structure this editor is replacing: `root` and
    /// every node reached from it through the `links` keys, cycle-guarded and
    /// capped at [`limits::MAX_TREE_ENTRIES`], the bound every reader of these
    /// structures stops at.
    ///
    /// Nodes only — the dictionaries of a tree's `/Kids` chain or an outline's
    /// `/First` and `/Next` chain. What a node's entries *point at* (a file
    /// specification, a page label dictionary, an action) is left, because the
    /// replacement may point at it too. Deleted rather than orphaned because an
    /// orphan is still in a rewrite unless it is garbage-collected, and an old
    /// outline's titles are content.
    fn retire(&mut self, root: ObjRef, links: &[Name]) {
        let mut visited = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if visited.len() >= limits::MAX_TREE_ENTRIES || !visited.insert(node.num) {
                continue;
            }
            let Some(Object::Dict(dict)) = self.get(node) else {
                continue;
            };
            for link in links {
                match dict.get(*link) {
                    Some(Object::Ref(r)) => stack.push(*r),
                    Some(Object::Array(items)) => {
                        stack.extend(items.iter().filter_map(Object::as_objref));
                    }
                    _ => {}
                }
            }
            self.delete(node);
        }
    }

    /// Replaces the document's page labels (12.4.2) with `ranges`.
    ///
    /// Written as the catalog's `/PageLabels` number tree, one
    /// `/Type /PageLabel` dictionary per range, keyed by its first page; the
    /// tree the document had is deleted. An empty slice removes the labels
    /// altogether, which reads back as no labels rather than as empty ones.
    /// The prefix is a text string, encoded for the version the document
    /// declares.
    ///
    /// # Errors
    ///
    /// [`PageLabelError`], and then **nothing was written**: no range at page
    /// 0, a range past the last page, a range numbering from 0, two ranges at
    /// one page, or no catalog.
    pub fn set_page_labels(&mut self, ranges: &[PageLabelRange]) -> Result<(), PageLabelError> {
        let key = self.intern(b"PageLabels");
        let Some(catalog) = self.catalog() else {
            return Err(PageLabelError::NoCatalog);
        };
        let old = catalog.get_ref(key);

        let root = if ranges.is_empty() {
            None
        } else {
            let pages = u32::try_from(self.page_refs().len()).unwrap_or(u32::MAX);
            if !ranges.iter().any(|range| range.first_page == 0) {
                return Err(PageLabelError::NoFirstPage);
            }
            for range in ranges {
                if range.first_page >= pages {
                    return Err(PageLabelError::PastLastPage {
                        first_page: range.first_page,
                        pages,
                    });
                }
                if range.start < 1 {
                    return Err(PageLabelError::StartBelowOne {
                        first_page: range.first_page,
                    });
                }
            }

            let version = self.text_version();
            let entries = ranges
                .iter()
                .map(|range| {
                    let mut dict = Dict::new();
                    dict.insert(crate::name::Name::TYPE, self.name_object(b"PageLabel"));
                    let style: Option<&[u8]> = match range.style {
                        LabelStyle::Decimal => Some(b"D"),
                        LabelStyle::RomanUpper => Some(b"R"),
                        LabelStyle::RomanLower => Some(b"r"),
                        LabelStyle::LettersUpper => Some(b"A"),
                        LabelStyle::LettersLower => Some(b"a"),
                        LabelStyle::None => None,
                    };
                    if let Some(style) = style {
                        dict.insert(self.intern(b"S"), self.name_object(style));
                    }
                    if let Some(prefix) = &range.prefix {
                        dict.insert(
                            self.intern(b"P"),
                            Object::String(encode_text_string(prefix, version)),
                        );
                    }
                    if range.start != 1 {
                        dict.insert(self.intern(b"St"), Object::Int(i64::from(range.start)));
                    }
                    (i64::from(range.first_page), Object::Dict(dict))
                })
                .collect();
            Some(
                self.add_number_tree(entries)
                    .map_err(PageLabelError::Tree)?,
            )
        };

        self.update_catalog(|catalog| match root {
            Some(root) => {
                catalog.insert(key, Object::Ref(root));
            }
            None => *catalog = without(catalog, key),
        });
        if let Some(old) = old {
            self.retire(old, &[Name::KIDS]);
        }
        Ok(())
    }

    /// Embeds a file (7.11.4) and files it under `file.name` in the catalog's
    /// `/Names /EmbeddedFiles` tree, beside whatever is filed there already.
    ///
    /// The file specification carries `/F`, `/UF` and `/Desc`, and an `/EF`
    /// naming one embedded file stream under both `/F` and `/UF`. The stream
    /// is `/Type /EmbeddedFile` with the `/Subtype` given, and `/Params`
    /// holding `/Size`, the MD5 `/CheckSum` of the bytes (Table 45) and the
    /// dates given. It is written unencoded; a save with `compress` deflates
    /// it like any other stream.
    ///
    /// Returns the embedded file stream — what
    /// [`crate::outline::Attachment::stream`] names when the tree is read
    /// back.
    ///
    /// # Errors
    ///
    /// [`AttachError`], and then **nothing was written**: the name is taken,
    /// the MIME type cannot be a name, a date cannot be spelled, the tree
    /// would be longer than a reader walks, or there is no catalog.
    pub fn attach_file(&mut self, file: &EmbeddedFile) -> Result<ObjRef, AttachError> {
        if self.catalog().is_none() {
            return Err(AttachError::NoCatalog);
        }
        if let Some(mime) = &file.mime_type {
            if mime.is_empty() || !mime.bytes().all(|b| b.is_ascii_graphic()) {
                return Err(AttachError::MimeType(mime.clone()));
            }
        }
        let version = self.text_version();
        let dates = [
            (b"CreationDate".as_slice(), file.created),
            (b"ModDate".as_slice(), file.modified),
        ];
        let mut params = Dict::new();
        params.insert(self.intern(b"Size"), Object::Int(file.data.len() as i64));
        params.insert(
            self.intern(b"CheckSum"),
            Object::String(PdfString::hex(
                tinker_pdf_crypto::md5::md5(&file.data).to_vec(),
            )),
        );
        for (key, date) in dates {
            if let Some(date) = date {
                let text = pdf_date(&date, version).ok_or(AttachError::Date(date))?;
                params.insert(
                    self.intern(key),
                    Object::String(PdfString::literal(text.into_bytes())),
                );
            }
        }

        let tree_key = self.intern(b"EmbeddedFiles");
        let old = self.names_dictionary().0.get_ref(tree_key);
        let mut entries = match old {
            Some(root) => trees::name_tree_in(self, root),
            None => Vec::new(),
        };
        if entries
            .iter()
            .any(|(key, _)| decode_text_string(key) == file.name)
        {
            return Err(AttachError::NameTaken(file.name.clone()));
        }

        // Everything that can refuse has refused, bar the tree writer; a
        // transaction puts back the two objects written before it if it does.
        self.transaction(|editor| {
            let mut stream_dict = Dict::new();
            stream_dict.insert(Name::TYPE, editor.name_object(b"EmbeddedFile"));
            if let Some(mime) = &file.mime_type {
                stream_dict.insert(
                    editor.intern(b"Subtype"),
                    editor.name_object(mime.as_bytes()),
                );
            }
            stream_dict.insert(editor.intern(b"Params"), Object::Dict(params));
            let stream = editor.allocate();
            editor.put_stream(
                stream,
                StreamData {
                    dict: stream_dict,
                    data: file.data.clone(),
                },
            );

            let mut ef = Dict::new();
            ef.insert(editor.intern(b"F"), Object::Ref(stream));
            ef.insert(editor.intern(b"UF"), Object::Ref(stream));
            let mut spec = Dict::new();
            spec.insert(Name::TYPE, editor.name_object(b"Filespec"));
            spec.insert(
                editor.intern(b"F"),
                Object::String(PdfString::literal(byte_file_name(&file.filename))),
            );
            spec.insert(
                editor.intern(b"UF"),
                Object::String(encode_text_string(&file.filename, version)),
            );
            if let Some(description) = &file.description {
                spec.insert(
                    editor.intern(b"Desc"),
                    Object::String(encode_text_string(description, version)),
                );
            }
            spec.insert(editor.intern(b"EF"), Object::Dict(ef));
            let spec_ref = editor.allocate();
            editor.put(spec_ref, Object::Dict(spec));

            entries.push((
                encode_text_string(&file.name, version).bytes,
                Object::Ref(spec_ref),
            ));
            let root = editor.add_name_tree(entries).map_err(AttachError::Tree)?;
            editor.set_names_entry(tree_key, Some(Object::Ref(root)));
            if let Some(old) = old {
                editor.retire(old, &[Name::KIDS]);
            }
            Ok(stream)
        })
    }

    /// Replaces the document's outline (12.3.3) with `entries`, or gives a
    /// document without one an outline.
    ///
    /// The builder's own [`OutlineEntry`] and [`crate::build::Target`], so one
    /// vocabulary serves both writers: a [`crate::build::Target::Page`] is an
    /// **explicit** destination (12.3.2.2) naming the page by reference —
    /// never collapsed into a name, and following the page if it is moved
    /// later (ruling 6) — and a URI is a `/URI` action. `index` counts pages
    /// in this editor's current order; one past the end writes no
    /// destination, as the builder does. Titles are text strings encoded for
    /// the document's version; `/Count` carries 12.3.3's sign for each
    /// entry's `open`.
    ///
    /// The outline the document had is deleted, item by item. An empty slice
    /// removes the outline: no `/Outlines` at all, which is not the same file
    /// as an empty outline dictionary.
    ///
    /// Returns false, writing nothing, for a tree this repository's reader
    /// could not read back — deeper than [`limits::MAX_NEST_DEPTH`], a level
    /// longer than [`limits::MAX_TREE_ENTRIES`], or a target that cannot be
    /// written — or a document with no catalog.
    pub fn set_outline(&mut self, entries: &[OutlineEntry]) -> bool {
        if !outline_is_writable(entries) {
            return false;
        }
        let key = self.intern(b"Outlines");
        let Some(catalog) = self.catalog() else {
            return false;
        };
        let old = catalog.get_ref(key);

        let root = if entries.is_empty() {
            None
        } else {
            let pages = self.page_refs();
            let version = self.text_version();
            let root = self.allocate();
            let children = self.write_outline_level(entries, &pages, root, version);
            let mut dict = Dict::new();
            dict.insert(Name::TYPE, self.name_object(b"Outlines"));
            if let Some((first, last, visible)) = children {
                dict.insert(self.intern(b"First"), Object::Ref(first));
                dict.insert(self.intern(b"Last"), Object::Ref(last));
                // Table 152: the visible items at every level; the root is
                // the one node that cannot be closed, so never negative.
                dict.insert(Name::COUNT, Object::Int(visible));
            }
            self.put(root, Object::Dict(dict));
            Some(root)
        };

        self.update_catalog(|catalog| match root {
            Some(root) => {
                catalog.insert(key, Object::Ref(root));
            }
            None => *catalog = without(catalog, key),
        });
        if let Some(old) = old {
            let (first, next) = (self.intern(b"First"), self.intern(b"Next"));
            self.retire(old, &[first, next]);
        }
        true
    }

    /// One level of outline items, returning `(first, last, visible)` — the
    /// builder's `write_outline`, with the editor's allocator.
    ///
    /// Recursion is bounded: [`outline_is_writable`] has already refused
    /// anything deeper than [`limits::MAX_NEST_DEPTH`].
    fn write_outline_level(
        &mut self,
        entries: &[OutlineEntry],
        pages: &[ObjRef],
        parent: ObjRef,
        version: (u8, u8),
    ) -> Option<(ObjRef, ObjRef, i64)> {
        if entries.is_empty() {
            return None;
        }
        let refs: Vec<ObjRef> = entries.iter().map(|_| self.allocate()).collect();
        let mut visible: i64 = 0;
        let names = std::sync::Arc::clone(&self.doc);

        for (index, entry) in entries.iter().enumerate() {
            let Some(&reference) = refs.get(index) else {
                continue;
            };
            let children = self.write_outline_level(&entry.children, pages, reference, version);

            let mut dict = Dict::new();
            dict.insert(
                self.intern(b"Title"),
                Object::String(encode_text_string(&entry.title, version)),
            );
            dict.insert(Name::PARENT, Object::Ref(parent));
            if let Some(target) = &entry.target {
                target.write(names.names_table(), pages, &mut dict);
            }
            if let Some(&previous) = index.checked_sub(1).and_then(|i| refs.get(i)) {
                dict.insert(self.intern(b"Prev"), Object::Ref(previous));
            }
            if let Some(&next) = refs.get(index + 1) {
                dict.insert(self.intern(b"Next"), Object::Ref(next));
            }
            visible += 1;
            if let Some((first, last, below)) = children {
                dict.insert(self.intern(b"First"), Object::Ref(first));
                dict.insert(self.intern(b"Last"), Object::Ref(last));
                // Table 153: positive for an open item, negative for a closed
                // one, the magnitude the same either way.
                dict.insert(
                    Name::COUNT,
                    Object::Int(if entry.open { below } else { -below }),
                );
                if entry.open {
                    visible += below;
                }
            }
            self.put(reference, Object::Dict(dict));
        }

        match (refs.first(), refs.last()) {
            (Some(&first), Some(&last)) => Some((first, last, visible)),
            _ => None,
        }
    }

    /// Sets the document's viewer preferences (12.2, Table 147).
    ///
    /// Every entry [`ViewerPreferences`] models is written as `prefs` states
    /// it — a `Some` field written, a `None` field removed — so reading the
    /// preferences back with [`viewer::viewer_preferences`] returns `prefs`.
    /// A key the type does not model (a private one, or one a later edition
    /// adds) is kept as it was. The dictionary is updated where it lives: an
    /// indirect one in its own object, a direct one in the catalog.
    ///
    /// Returns false, writing nothing, when `prefs` is not
    /// [`ViewerPreferences::is_writable`] or there is no catalog.
    pub fn set_viewer_preferences(&mut self, prefs: &ViewerPreferences) -> bool {
        if !prefs.is_writable() {
            return false;
        }
        let key = self.intern(b"ViewerPreferences");
        let Some(catalog) = self.catalog() else {
            return false;
        };
        let (existing, holder) = match catalog.get(key) {
            Some(Object::Ref(r)) => match self.get(*r) {
                Some(Object::Dict(dict)) => (dict, Some(*r)),
                _ => (Dict::new(), Some(*r)),
            },
            Some(Object::Dict(dict)) => (dict.clone(), None),
            _ => (Dict::new(), None),
        };
        let modelled: Vec<Name> = viewer::KEYS.iter().map(|k| self.intern(k)).collect();
        let mut dict: Dict = existing
            .iter()
            .filter(|(k, _)| !modelled.contains(k))
            .cloned()
            .collect();
        for (k, v) in prefs.entries(self) {
            dict.insert(k, v);
        }
        match holder {
            Some(r) => self.put(r, Object::Dict(dict)),
            None => {
                self.update_catalog(|catalog| {
                    if dict.is_empty() {
                        *catalog = without(catalog, key);
                    } else {
                        catalog.insert(key, Object::Dict(dict));
                    }
                });
            }
        }
        true
    }

    /// A name object, interned in the document's table.
    pub(super) fn name_object(&self, bytes: &[u8]) -> Object {
        Object::Name(self.intern(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, offset: Option<i32>) -> Date {
        Date {
            year,
            month: 9,
            day: 26,
            hour: 14,
            minute: 5,
            second: 9,
            utc_offset_minutes: offset,
        }
    }

    /// Every form 7.9.4 spells reads back through this crate's own parser as
    /// the date it was written from.
    #[test]
    fn a_written_date_parses_back_to_itself() {
        for offset in [None, Some(0), Some(90), Some(-330)] {
            for version in [(1, 7), (2, 0)] {
                let text = pdf_date(&date(2026, offset), version).expect("writable");
                assert_eq!(
                    crate::text_string::parse_date(&text),
                    Some(date(2026, offset)),
                    "{text}"
                );
            }
        }
        assert_eq!(
            pdf_date(&date(2026, Some(90)), (1, 7)).as_deref(),
            Some("D:20260926140509+01'30'")
        );
        assert_eq!(
            pdf_date(&date(2026, Some(-330)), (2, 0)).as_deref(),
            Some("D:20260926140509-05'30")
        );
    }

    #[test]
    fn a_date_the_syntax_has_no_digits_for_is_refused() {
        assert_eq!(pdf_date(&date(10_000, None), (1, 7)), None);
        assert_eq!(pdf_date(&date(-1, None), (1, 7)), None);
        assert_eq!(pdf_date(&date(2026, Some(24 * 60)), (1, 7)), None);
        let mut bad = date(2026, None);
        bad.month = 13;
        assert_eq!(pdf_date(&bad, (1, 7)), None);
    }

    #[test]
    fn a_byte_file_name_keeps_printable_ascii_only() {
        assert_eq!(byte_file_name("data.csv"), b"data.csv");
        assert_eq!(byte_file_name("Übersicht 1.csv"), b"_bersicht 1.csv");
    }
}
