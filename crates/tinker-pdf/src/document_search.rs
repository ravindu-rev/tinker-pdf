//! Searching a whole document (`docs/design/document-search.md`): a cursor
//! over the page text joined line to line, block to block and page to page,
//! which finds the next or the previous hit from a position and reads a page
//! only when it reaches it.
//!
//! Feature documentation: `docs/features/content-and-text.md`, "Document
//! search".
//!
//! # The joined text
//!
//! Each page's text is its `Page::text`, every line in logical order (ruling
//! 14), taken in `TextPage::lines` order, page after page. A line's units are
//! its glyphs, each read as its piece of the page's kept form in the options'
//! fold (`TextPage::folded_lines`), so case and diacritics behave as on one
//! page; without `keep_hyphens` every soft hyphen is removed from the pieces.
//! Between two lines stands a boundary no hit crosses, a hyphen join by
//! `tinker_pdf_content::plain::hyphen_join` — the rule plain text rejoins by
//! — or one space; white space at a line's end is set aside wherever the
//! options join lines there. A **run** is a stretch no boundary cuts, and a
//! **chunk** the lines a hyphen joins, with a space between each chunk and the
//! next.
//!
//! # Hits
//!
//! A run's hits are the page search's: every occurrence of the folded needle,
//! found by the page search's own matcher ([`Needle`]), its state carried
//! from chunk to chunk, and its two overlap rules — an occurrence that begins
//! inside a hit is passed over, and a whole-word candidate that is not one
//! holds nothing back. A chunk's UAX #29 boundaries are computed with a space
//! beside each end that meets a space and nothing beside one that meets a
//! run's edge, which are the run's boundaries, since no `Word_Break` rule
//! looks past a space.
//!
//! # The cursor
//!
//! The hits are fixed by the needle and the options, and the cursor moves
//! through them: [`Iterator::next`] returns the first that starts at or after
//! its position, [`DocumentSearch::previous`] the last that ends at or before
//! it. A scan starts only at a clean chunk start — one no qualifying
//! occurrence crosses, read off the needle's length less one on either side,
//! or a run's start — from which the hits after it are the run's whatever
//! came before. `next` keeps its scan between calls.
//!
//! # The sources
//!
//! A document's text is read as stretches in a fixed order: its pages, its
//! annotations, its form fields, its outline and its attachments, each but
//! the first a list of strings read through what the facade lists. A
//! stretch's reader reads each string as a page of one block, its lines split
//! at line ends and each character a glyph of its own, so the same joins,
//! folding and matching read it; a boundary stands between two strings, each
//! its own run.

use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tinker_pdf_content::{
    hyphen_join, word_boundaries, Needle, Quad, SearchOptions, TextBlock, TextChar, TextLine,
    TextPage, WritingMode,
};
use tinker_pdf_cos::{FieldKind, FieldValue, ObjRef, OutlineItem};

use crate::annotations::RichText;
use crate::{Document, DocumentError};

/// How [`Document::search`] reads a document: the page search's options, and
/// how the lines it reads are joined.
///
/// `Default` joins every line to the next — across blocks and pages — with a
/// space, or at a hyphen by plain text's rule, and searches as
/// [`SearchOptions::default`] does: literal and case-insensitive. No field is
/// one a page's own search reads, so `TextPage::search` and `search_with`
/// keep their answers; with [`DocumentSearchOptions::keep_lines`] and
/// [`DocumentSearchOptions::keep_hyphens`] both on, the hits a document
/// search reports on a page are `search_with`'s there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentSearchOptions {
    /// Case, whole word and diacritics, as a page's search reads them.
    pub search: SearchOptions,
    /// Keep every hit inside one line: a line's end is a boundary no hit
    /// crosses, and each line is read as drawn.
    pub keep_lines: bool,
    /// Keep every hit inside one block: a block's end — and so a page's — is
    /// a boundary no hit crosses.
    pub keep_paragraphs: bool,
    /// Keep a hyphen where the page draws it: no line is rejoined at a
    /// hyphen, and a soft hyphen is a character like any other.
    pub keep_hyphens: bool,
    /// Which of the document's sources are searched; every one by default.
    pub sources: SearchSources,
}

/// Which of a document's sources a [`DocumentSearch`] reads, in the order it
/// reads them: the page text, then each other source's strings
/// (`docs/design/document-search.md`, "The sources").
///
/// `Default` is [`SearchSources::ALL`]; [`SearchSources::PAGES`] is the page
/// text alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchSources {
    /// The page text.
    pub pages: bool,
    /// Each page's annotations' text: `/Contents`, the text of `/RC`, `/Subj`
    /// and a markup annotation's author, `/T`.
    pub annotations: bool,
    /// The form fields' values: a field's text value, or each value a list box
    /// holds.
    pub form_fields: bool,
    /// The outline items' titles.
    pub outline: bool,
    /// The attachments' names, file names and descriptions.
    pub attachments: bool,
    /// The PDFs attached to the document, each searched with the same
    /// options to [`MAX_ATTACHMENT_DEPTH`] levels, [`MAX_ATTACHMENT_BYTES`]
    /// each and [`MAX_SEARCHED_ATTACHMENTS`] in one search.
    pub attached_documents: bool,
}

impl SearchSources {
    /// Every source.
    pub const ALL: SearchSources = SearchSources {
        pages: true,
        annotations: true,
        form_fields: true,
        outline: true,
        attachments: true,
        attached_documents: true,
    };

    /// The page text alone.
    pub const PAGES: SearchSources = SearchSources {
        pages: true,
        annotations: false,
        form_fields: false,
        outline: false,
        attachments: false,
        attached_documents: false,
    };
}

/// How many levels of attached document a search reads into: the document
/// searched holds level 1's attachments, a PDF attached inside one of them
/// is level 2, and an attached PDF past the last level is not opened and is
/// named in [`DocumentSearch::unsearched`] with
/// [`UnsearchedReason::TooDeep`].
///
/// A bound on recursion: each level opens a document inside the one above
/// it, held while the search reads it.
///
/// | | Levels |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 4 |
/// | Any other fixture here: `tests/document_search.rs`'s nested attachments | 2 |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **4** |
///
/// The three zeros are facts about what those paths write: none of them
/// attaches a file.
pub const MAX_ATTACHMENT_DEPTH: usize = 4;

/// How many decoded bytes an attachment may hold for a search to open it: an
/// attachment that decodes to more is not opened and is named in
/// [`DocumentSearch::unsearched`] with [`UnsearchedReason::TooLarge`].
///
/// A bound on what a search holds: an attached document is held while the
/// search reads it, one a level. The stream decoder's own ceiling,
/// `MAX_DECODED_STREAM`, stands in front of it.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 32 MiB |
/// | Any other fixture here: `tests/document_search.rs`'s attached PDFs | under 64 KiB (estimate, not summed) |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **32 MiB** |
pub const MAX_ATTACHMENT_BYTES: usize = 32 << 20;

/// How many attachments one search reads, at every level together: past it
/// no further attachment's bytes are decoded, and each one the search
/// reaches is named in [`DocumentSearch::unsearched`] with
/// [`UnsearchedReason::TooMany`].
///
/// A bound on work: each attachment read is decoded, and an attached PDF
/// opened and searched.
///
/// | | Attachments |
/// | --- | --- |
/// | The most any fixture in this repository spends: the one built to spend it | 256 |
/// | Any other fixture here: `tests/document_search.rs`'s | under 8 |
/// | A 200-page comic archive | 0 |
/// | A 200-page fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **256** |
pub const MAX_SEARCHED_ATTACHMENTS: usize = 256;

impl Default for SearchSources {
    fn default() -> SearchSources {
        SearchSources::ALL
    }
}

/// Which of an annotation's strings a hit was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationText {
    /// `/Contents` (Table 164).
    Contents,
    /// The character data of `/RC`'s rich text (Table 170).
    RichText,
    /// `/Subj` (Table 170).
    Subject,
    /// A markup annotation's `/T`, its author (Table 170).
    Author,
}

/// Which of an attachment's strings a hit was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentText {
    /// The name it is filed under in `/Names /EmbeddedFiles`.
    Name,
    /// `/UF` or `/F`, where it is not the name.
    FileName,
    /// `/Desc`.
    Description,
}

/// How many characters of a hit's first line before it, and of its last line
/// after it, [`SearchHit::before`] and [`SearchHit::after`] hold at most.
pub const HIT_CONTEXT: usize = 40;

/// One page a hit covers, with a quad for each of its lines the hit covers.
#[derive(Clone, Debug, PartialEq)]
pub struct PageQuads {
    /// The page, zero-based.
    pub page: u32,
    /// One quad a line, in the order the hit reads them, each the quad the
    /// page's own search reports over the same glyphs
    /// (`TextLine::span_quad`).
    pub quads: Vec<Quad>,
}

/// Where a hit was found.
///
/// Each source beside the page text names the string it was found in and
/// `range`, the bytes of that string the hit covers as the string states it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum HitPlace {
    /// In the page text: each page the hit covers, in order — two or more
    /// for a hit that crosses a page break.
    Pages(Vec<PageQuads>),
    /// In an annotation's text: the page's annotation `index`, in `/Annots`
    /// order as [`crate::Page::annotations`] lists it, with its `/Rect`.
    Annotation {
        /// The page, zero-based.
        page: u32,
        /// The annotation's index in the page's listing.
        index: usize,
        /// Its `/Rect`, as the listing reads it.
        rect: (f64, f64, f64, f64),
        /// Which of its strings.
        text: AnnotationText,
        /// The bytes of that string the hit covers.
        range: Range<usize>,
    },
    /// In a form field's value.
    FormField {
        /// The field's fully qualified name.
        name: String,
        /// The widget annotations that draw it.
        widgets: Vec<ObjRef>,
        /// Which of its values: 0 for a single value, each of a list box's in
        /// turn.
        value: usize,
        /// The bytes of that value the hit covers.
        range: Range<usize>,
    },
    /// In an outline item's title.
    Outline {
        /// The item's index among its siblings at each depth, from the top.
        path: Vec<usize>,
        /// The titles from the top to the item, its own last.
        titles: Vec<String>,
        /// The bytes of its title the hit covers.
        range: Range<usize>,
    },
    /// In an attachment's name, file name or description.
    Attachment {
        /// The name it is filed under.
        name: String,
        /// Which of its strings.
        text: AttachmentText,
        /// The bytes of that string the hit covers.
        range: Range<usize>,
    },
}

/// One hit of a [`DocumentSearch`].
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// Where it was found.
    pub place: HitPlace,
    /// The text it covers as the source states it: the text of every glyph
    /// or character it covers, the lines it crosses joined as the search
    /// joined them.
    pub text: String,
    /// Up to [`HIT_CONTEXT`] characters of its first line before it, as
    /// stated.
    pub before: String,
    /// Up to [`HIT_CONTEXT`] characters of its last line after it, as
    /// stated.
    pub after: String,
    /// The attachments the document it was found in is filed under,
    /// outermost first; empty in the document searched.
    pub attachment_path: Vec<String>,
    /// Where it starts.
    start: SearchPosition,
    /// Where it ends.
    end: SearchPosition,
}

impl SearchHit {
    /// Where the hit starts: [`DocumentSearch::seek`] there, and the next hit
    /// is this one.
    #[must_use]
    pub fn start(&self) -> &SearchPosition {
        &self.start
    }

    /// Where the hit ends: [`DocumentSearch::seek`] there, and the previous
    /// hit is this one.
    #[must_use]
    pub fn end(&self) -> &SearchPosition {
        &self.end
    }
}

/// A place in a document's text, ordered as the document reads.
///
/// Opaque: a caller names the document's start and end, a page's start, and
/// the positions its hits start and end at. A position inside the page text
/// stands before a byte of one glyph's folded piece, or at the end of a line,
/// so a hit that ends inside a ligature is followed by the one that starts
/// there.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchPosition(Spot);

impl SearchPosition {
    /// The start of the document's text.
    #[must_use]
    pub fn start() -> SearchPosition {
        SearchPosition(Spot::START)
    }

    /// The end of the document's text.
    #[must_use]
    pub fn end() -> SearchPosition {
        SearchPosition(Spot::End)
    }

    /// The start of page `index`'s text, zero-based; past the last page, the
    /// end of the document's text.
    #[must_use]
    pub fn page(index: u32) -> SearchPosition {
        SearchPosition(Spot::Text {
            page: index,
            line: 0,
            unit: 0,
            within: 0,
        })
    }
}

/// Something a [`DocumentSearch`] reached and could not read (ruling 10): a
/// document not searched is named, never counted as having no hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsearched {
    /// The attachments the unread document is filed under, outermost first;
    /// empty for the document searched.
    pub attachment_path: Vec<String>,
    /// Why it was not read.
    pub reason: UnsearchedReason,
}

/// Why a document was not searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsearchedReason {
    /// It is encrypted and no password has opened it: for an attached PDF,
    /// the empty password, which the search tries.
    PasswordRequired,
    /// It is encrypted with a handler this build does not implement.
    UnsupportedEncryption,
    /// An attached PDF past [`MAX_ATTACHMENT_DEPTH`] levels.
    TooDeep,
    /// An attachment that decodes to more than [`MAX_ATTACHMENT_BYTES`].
    TooLarge,
    /// An attachment past [`MAX_SEARCHED_ATTACHMENTS`] in one search.
    TooMany,
    /// An attachment with no stream, a stream that does not decode, or bytes
    /// that begin as a PDF and that `Document::open` refuses.
    Unreadable,
}

/// A search of a whole document: a cursor through its hits, in the order the
/// document reads.
///
/// [`Iterator::next`] returns the first hit that starts at or after the
/// cursor's position and moves the position to the hit's end;
/// [`DocumentSearch::previous`] returns the last hit that ends at or before
/// it and moves the position to the hit's start — so `next` after `previous`
/// returns the hit again, as a list iterator does, and the hits `previous`
/// finds from the end are the hits `next` finds from the start, in reverse.
/// A page is extracted only when the search reaches it, and the cursor keeps
/// the few pages it stands on.
pub struct DocumentSearch {
    doc: Document,
    /// The needle as the caller gave it, for the searches of attached
    /// documents.
    needle_text: String,
    options: DocumentSearchOptions,
    /// The folded needle; `None` finds nothing.
    needle: Option<Needle>,
    /// How many levels of attached document this one is inside: 0 for the
    /// document searched.
    depth: usize,
    /// The attachments this document is filed under, outermost first.
    path: Vec<String>,
    /// How many attachments the whole search has read, at every level.
    read: Arc<AtomicUsize>,
    /// Each stretch's reader — the page text's, then each other source's —
    /// built when the cursor first reaches it; `None` for a stretch left out.
    readers: [Option<Reader>; STRETCHES],
    /// Whether each stretch's reader has been built.
    built: [bool; STRETCHES],
    /// The attached documents, listed when the cursor first reaches them.
    attached: Option<Attached>,
    /// The cursor's position.
    position: Spot,
    /// The scan `next` continues, and its stretch, while the position is
    /// where it left it.
    stream: Option<(usize, Stream)>,
    /// Hits the scan decided that `next` has not returned.
    pending: VecDeque<SearchHit>,
    /// What the search could not read.
    unsearched: Vec<Unsearched>,
}

/// How many stretches one document's text is read as before its attached
/// documents: its pages, its annotations, its form fields, its outline and
/// its attachments.
const STRETCHES: usize = 5;

impl Document {
    /// A search of the document's text for `needle`, as `options` read it,
    /// its cursor at the document's start (`docs/design/document-search.md`).
    ///
    /// A needle that is empty, or that folding empties, finds nothing. A
    /// document that is encrypted and that no password has opened is not
    /// read: the search finds nothing in it and names it in
    /// [`DocumentSearch::unsearched`].
    #[must_use]
    pub fn search(&self, needle: &str, options: &DocumentSearchOptions) -> DocumentSearch {
        DocumentSearch::new(
            self,
            needle,
            options,
            0,
            Vec::new(),
            Arc::new(AtomicUsize::new(0)),
        )
    }
}

/// Why `doc` cannot be read, as a search names it; `None` when it can.
fn unreadable(doc: &Document) -> Option<UnsearchedReason> {
    match doc.readable() {
        Ok(()) => None,
        Err(DocumentError::PasswordRequired) => Some(UnsearchedReason::PasswordRequired),
        Err(DocumentError::UnsupportedEncryption) => Some(UnsearchedReason::UnsupportedEncryption),
    }
}

impl DocumentSearch {
    /// A search of `doc`, `depth` levels of attached document down, filed
    /// under `path`, counting the attachments it reads into `read`.
    fn new(
        doc: &Document,
        needle: &str,
        options: &DocumentSearchOptions,
        depth: usize,
        path: Vec<String>,
        read: Arc<AtomicUsize>,
    ) -> DocumentSearch {
        let mut unsearched = Vec::new();
        let reason = unreadable(doc);
        if let Some(reason) = reason {
            unsearched.push(Unsearched {
                attachment_path: path.clone(),
                reason,
            });
        }
        DocumentSearch {
            doc: doc.clone(),
            needle_text: needle.to_string(),
            options: options.clone(),
            needle: Needle::new(needle, &options.search).filter(|_| reason.is_none()),
            depth,
            path,
            read,
            readers: Default::default(),
            built: [false; STRETCHES],
            attached: None,
            position: Spot::START,
            stream: None,
            pending: VecDeque::new(),
            unsearched,
        }
    }

    /// The last hit that ends at or before the cursor's position, the
    /// position moved to its start; `None`, the position moved to the
    /// document's start, where there is none.
    pub fn previous(&mut self) -> Option<SearchHit> {
        self.stream = None;
        self.pending.clear();
        // The attached documents first, where the position is in or past
        // them.
        let attached = match &self.position {
            Spot::Attached { index, inner } => Some(Some((*index as usize, (**inner).clone()))),
            Spot::End => Some(None),
            Spot::Text { .. } | Spot::Item { .. } => None,
        };
        if let Some(to) = attached {
            if let Some(hit) = self.previous_attached(to) {
                self.position = hit.start.0.clone();
                return Some(hit);
            }
        }
        let (from, local) = split(&self.position).unwrap_or((STRETCHES - 1, Spot::End));
        for stretch in (0..=from).rev() {
            let to = if stretch == from {
                local.clone()
            } else {
                Spot::End
            };
            let path = self.path.clone();
            let Some(reader) = self.reader(stretch) else {
                continue;
            };
            if let Some(raw) = reader.previous_before(&to) {
                let hit = reader.hit(stretch, raw, path);
                self.position = hit.start.0.clone();
                return Some(hit);
            }
        }
        self.position = Spot::START;
        None
    }

    /// The cursor's position.
    #[must_use]
    pub fn position(&self) -> SearchPosition {
        SearchPosition(self.position.clone())
    }

    /// Moves the cursor to `position`.
    pub fn seek(&mut self, position: SearchPosition) {
        self.position = position.0;
        self.stream = None;
        self.pending.clear();
    }

    /// What the search reached and could not read, each once however often
    /// the cursor passed it: the document searched where no password has
    /// opened it, and each attachment it reached and did not search, with
    /// the attachment path it sits under.
    #[must_use]
    pub fn unsearched(&self) -> &[Unsearched] {
        &self.unsearched
    }

    /// Stretch `stretch`'s reader, built now where the cursor has not reached
    /// it; `None` for a stretch the options leave out, or of a document that
    /// is not read.
    fn reader(&mut self, stretch: usize) -> Option<&mut Reader> {
        let needle = self.needle.clone()?;
        if !self.built.get(stretch).copied().unwrap_or(true) {
            if let Some(built) = self.built.get_mut(stretch) {
                *built = true;
            }
            let sources = &self.options.sources;
            let reader = match stretch {
                0 if sources.pages => Some(Reader::pages(&self.doc, &self.options, needle)),
                1 if sources.annotations => Some(Reader::items(
                    &self.doc,
                    &self.options,
                    needle,
                    annotation_items(&self.doc),
                )),
                2 if sources.form_fields => Some(Reader::items(
                    &self.doc,
                    &self.options,
                    needle,
                    field_items(&self.doc),
                )),
                3 if sources.outline => Some(Reader::items(
                    &self.doc,
                    &self.options,
                    needle,
                    outline_items(&self.doc),
                )),
                4 if sources.attachments => Some(Reader::items(
                    &self.doc,
                    &self.options,
                    needle,
                    attachment_items(&self.doc),
                )),
                _ => None,
            };
            if let Some(slot) = self.readers.get_mut(stretch) {
                *slot = reader;
            }
        }
        self.readers.get_mut(stretch)?.as_mut()
    }

    /// The scan `next` reads from `local` in stretch `from`, or from the start
    /// of the first stretch after it that holds anything after it.
    fn stream_from(&mut self, from: usize, local: &Spot) -> Option<(usize, Stream)> {
        for stretch in from..STRETCHES {
            let start = if stretch == from {
                local.clone()
            } else {
                Spot::START
            };
            let Some(reader) = self.reader(stretch) else {
                continue;
            };
            if let Some(stream) = reader.stream_from(&start) {
                return Some((stretch, stream));
            }
        }
        None
    }

    /// The next hit of the stretches before the attached documents, from
    /// `from`, the scan kept for the next call.
    fn next_in_stretches(&mut self, from: &Spot) -> Option<SearchHit> {
        if self.stream.is_none() {
            self.pending.clear();
            self.stream = match split(from) {
                Some((stretch, local)) => self.stream_from(stretch, &local),
                None => None,
            };
        }
        loop {
            while let Some(hit) = self.pending.pop_front() {
                if hit.start.0 >= *from {
                    return Some(hit);
                }
            }
            let (stretch, mut stream) = self.stream.take()?;
            let path = self.path.clone();
            let reader = self.readers.get_mut(stretch).and_then(Option::as_mut)?;
            let mut decided = VecDeque::new();
            let more = stream.pump(reader, &mut decided);
            for raw in decided {
                self.pending
                    .push_back(reader.hit(stretch, raw, path.clone()));
            }
            self.stream = if more {
                Some((stretch, stream))
            } else {
                // The stretch is read: the next one's scan, from its start.
                self.stream_from(stretch + 1, &Spot::START)
            };
        }
    }

    /// The attached documents, listed now where the cursor has not reached
    /// them; `None` where the options leave them out or the document is not
    /// read.
    fn attached(&mut self) -> Option<&mut Attached> {
        if !self.options.sources.attached_documents || self.needle.is_none() {
            return None;
        }
        if self.attached.is_none() {
            let attachments = self.doc.attachments();
            self.attached = Some(Attached {
                standing: vec![None; attachments.len()],
                attachments,
                open: None,
            });
        }
        self.attached.as_mut()
    }

    /// Names `reason` for the attachment `name` of this document, once.
    fn report(&mut self, name: &str, reason: UnsearchedReason) {
        let mut attachment_path = self.path.clone();
        attachment_path.push(name.to_string());
        merge(
            &mut self.unsearched,
            [Unsearched {
                attachment_path,
                reason,
            }],
        );
    }

    /// Closes the attached document's search that is open, keeping what it
    /// could not read.
    fn close_attached(&mut self) {
        if let Some((_, search)) = self.attached.as_mut().and_then(|a| a.open.take()) {
            merge(&mut self.unsearched, search.unsearched);
        }
    }

    /// The search of attachment `index`, opened now where another is open or
    /// none is; `None` for an attachment that is no document, or that this
    /// search could not read, which is named.
    fn attached_search(&mut self, index: usize) -> Option<&mut DocumentSearch> {
        let open_now = self
            .attached
            .as_ref()
            .and_then(|attached| attached.open.as_ref())
            .is_some_and(|(open, _)| *open == index);
        if !open_now {
            self.close_attached();
            let attached = self.attached.as_ref()?;
            let attachment = attached.attachments.get(index)?.clone();
            let standing = attached.standing.get(index).copied().flatten();
            let outcome = match standing {
                Some(Standing::NotADocument | Standing::Unsearched) => return None,
                Some(Standing::Document) => self.open_attachment(&attachment, false),
                None => self.open_attachment(&attachment, true),
            };
            let (standing, search) = match outcome {
                Ok(doc) => {
                    let mut path = self.path.clone();
                    path.push(attachment.name.clone());
                    let search = DocumentSearch::new(
                        &doc,
                        &self.needle_text,
                        &self.options,
                        self.depth + 1,
                        path,
                        Arc::clone(&self.read),
                    );
                    (Standing::Document, Some(search))
                }
                Err(Some(reason)) => {
                    self.report(&attachment.name, reason);
                    (Standing::Unsearched, None)
                }
                Err(None) => (Standing::NotADocument, None),
            };
            let attached = self.attached.as_mut()?;
            if let Some(slot) = attached.standing.get_mut(index) {
                *slot = Some(standing);
            }
            attached.open = search.map(|search| (index, Box::new(search)));
        }
        self.attached
            .as_mut()?
            .open
            .as_mut()
            .map(|(_, search)| search.as_mut())
    }

    /// Attachment `attachment` opened as a document, read under the caps:
    /// counted against [`MAX_SEARCHED_ATTACHMENTS`] where `first` is its first
    /// read; `Err(None)` for bytes that are no PDF, and the reason a PDF, or
    /// what may be one, is not searched otherwise.
    fn open_attachment(
        &self,
        attachment: &tinker_pdf_cos::Attachment,
        first: bool,
    ) -> Result<Document, Option<UnsearchedReason>> {
        if first {
            let read = self.read.fetch_add(1, Ordering::Relaxed);
            if read >= MAX_SEARCHED_ATTACHMENTS {
                self.read.fetch_sub(1, Ordering::Relaxed);
                return Err(Some(UnsearchedReason::TooMany));
            }
        }
        let stream = attachment
            .stream
            .ok_or(Some(UnsearchedReason::Unreadable))?;
        let bytes = self
            .doc
            .cos()
            .stream_decoded(stream)
            .map_err(|_| Some(UnsearchedReason::Unreadable))?;
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(Some(UnsearchedReason::TooLarge));
        }
        let head = bytes.get(..PDF_HEADER_WINDOW).unwrap_or(&bytes);
        if !head.windows(5).any(|window| window == b"%PDF-") {
            return Err(None);
        }
        if self.depth + 1 > MAX_ATTACHMENT_DEPTH {
            return Err(Some(UnsearchedReason::TooDeep));
        }
        let doc = Document::open(bytes).map_err(|_| Some(UnsearchedReason::Unreadable))?;
        if doc.is_encrypted() && doc.auth_level() == crate::AuthLevel::None {
            // The search has no password for an attached document but the
            // empty one, which a document encrypted only to restrict
            // permissions opens with.
            let _ = doc.authenticate("");
        }
        match unreadable(&doc) {
            Some(reason) => Err(Some(reason)),
            None => Ok(doc),
        }
    }

    /// The first hit of the attached documents from attachment `index`'s
    /// position `inner`, or from the first one's start.
    fn next_attached(&mut self, from: Option<(usize, Spot)>) -> Option<SearchHit> {
        let count = self.attached()?.attachments.len();
        let (mut index, mut inner) = from.unwrap_or((0, Spot::START));
        while index < count {
            if let Some(search) = self.attached_search(index) {
                if search.position != inner {
                    search.seek(SearchPosition(inner.clone()));
                }
                let hit = search.next();
                let found = search.unsearched.clone();
                merge(&mut self.unsearched, found);
                if let Some(hit) = hit {
                    return Some(within_attachment(index, hit));
                }
            }
            index += 1;
            inner = Spot::START;
        }
        None
    }

    /// The last hit of the attached documents to attachment `index`'s
    /// position `inner`, or to the last one's end.
    fn previous_attached(&mut self, to: Option<(usize, Spot)>) -> Option<SearchHit> {
        let count = self.attached()?.attachments.len();
        let last = count.checked_sub(1)?;
        let (mut index, mut inner) = match to {
            Some((index, inner)) if index <= last => (index, inner),
            _ => (last, Spot::End),
        };
        loop {
            if let Some(search) = self.attached_search(index) {
                search.seek(SearchPosition(inner.clone()));
                let hit = search.previous();
                let found = search.unsearched.clone();
                merge(&mut self.unsearched, found);
                if let Some(hit) = hit {
                    return Some(within_attachment(index, hit));
                }
            }
            index = index.checked_sub(1)?;
            inner = Spot::End;
        }
    }
}

/// How far into an attachment's bytes the search looks for `%PDF-`, as
/// `Document::open` looks for a header.
const PDF_HEADER_WINDOW: usize = 1024;

/// The attached documents a search reads, after the document's own stretches.
struct Attached {
    /// The document's attachments, in name order, as its listing gives them.
    attachments: Vec<tinker_pdf_cos::Attachment>,
    /// What each attachment turned out to be, once read.
    standing: Vec<Option<Standing>>,
    /// The attachment whose search is open, and its search: one at a time.
    open: Option<(usize, Box<DocumentSearch>)>,
}

/// What an attachment turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    /// A document the search reads.
    Document,
    /// Bytes that are no PDF, whose contents are no source.
    NotADocument,
    /// What the search could not read, which it named.
    Unsearched,
}

/// `entries` added to `list`, each once.
fn merge(list: &mut Vec<Unsearched>, entries: impl IntoIterator<Item = Unsearched>) {
    for entry in entries {
        if !list.contains(&entry) {
            list.push(entry);
        }
    }
}

/// A hit an attached document's search found, its positions inside
/// attachment `index` of the document.
fn within_attachment(index: usize, mut hit: SearchHit) -> SearchHit {
    let index = index_u32(index);
    hit.start = SearchPosition(Spot::Attached {
        index,
        inner: Box::new(hit.start.0),
    });
    hit.end = SearchPosition(Spot::Attached {
        index,
        inner: Box::new(hit.end.0),
    });
    hit
}

impl Iterator for DocumentSearch {
    type Item = SearchHit;

    /// The first hit that starts at or after the cursor's position, the
    /// position moved to its end; `None`, the position moved to the
    /// document's end, past the last.
    fn next(&mut self) -> Option<SearchHit> {
        let from = self.position.clone();
        let hit = match &from {
            Spot::Attached { index, inner } => {
                self.next_attached(Some((*index as usize, (**inner).clone())))
            }
            Spot::End => None,
            Spot::Text { .. } | Spot::Item { .. } => match self.next_in_stretches(&from) {
                Some(hit) => Some(hit),
                None => {
                    self.stream = None;
                    self.next_attached(None)
                }
            },
        };
        self.position = hit.as_ref().map_or(Spot::End, |hit| hit.end.0.clone());
        hit
    }
}

/// Which stretch a position is in, and where in it as the stretch's reader
/// reads it: the page text's positions as they are, another stretch's with its
/// item for the page; `None` at the document's end.
fn split(spot: &Spot) -> Option<(usize, Spot)> {
    match *spot {
        Spot::Text { .. } => Some((0, spot.clone())),
        Spot::Item {
            stretch,
            item,
            line,
            unit,
            within,
        } => Some((
            stretch.index(),
            Spot::Text {
                page: item,
                line,
                unit,
                within,
            },
        )),
        Spot::Attached { .. } | Spot::End => None,
    }
}

/// A position a stretch's reader read, as the document's.
fn joined(stretch: usize, local: Spot) -> Spot {
    match (Stretch::of(stretch), local) {
        (
            Some(kind),
            Spot::Text {
                page,
                line,
                unit,
                within,
            },
        ) => Spot::Item {
            stretch: kind,
            item: page,
            line,
            unit,
            within,
        },
        (_, local) => local,
    }
}

/// A position, as [`SearchPosition`] wraps it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Spot {
    /// In the page text: before byte `within` of glyph `unit`'s folded piece
    /// in line `line` of page `page`. A `unit` past the line's last glyph is
    /// the line's end, a `line` past the page's last its end, and a `page`
    /// past the document's last the end of the page text.
    Text {
        page: u32,
        line: u32,
        unit: u32,
        within: u32,
    },
    /// In another stretch: before byte `within` of character `unit`'s
    /// folded piece in line `line` of its string `item`.
    Item {
        stretch: Stretch,
        item: u32,
        line: u32,
        unit: u32,
        within: u32,
    },
    /// In the attached documents: position `inner` of the search of
    /// attachment `index`, in the document's listing order.
    Attached { index: u32, inner: Box<Spot> },
    /// The end of the document's text.
    End,
}

impl Spot {
    const START: Spot = Spot::Text {
        page: 0,
        line: 0,
        unit: 0,
        within: 0,
    };
}

/// The stretches after the page text, in the order a search reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Stretch {
    Annotations,
    Fields,
    Outline,
    Attachments,
}

impl Stretch {
    /// The stretch's index among a document's, the page text's 0.
    const fn index(self) -> usize {
        match self {
            Stretch::Annotations => 1,
            Stretch::Fields => 2,
            Stretch::Outline => 3,
            Stretch::Attachments => 4,
        }
    }

    /// The stretch of index `index`; `None` for the page text's and past the
    /// last.
    const fn of(index: usize) -> Option<Stretch> {
        match index {
            1 => Some(Stretch::Annotations),
            2 => Some(Stretch::Fields),
            3 => Some(Stretch::Outline),
            4 => Some(Stretch::Attachments),
            _ => None,
        }
    }
}

/// One string a source beside the page text holds, and where it is.
struct Item {
    text: String,
    place: ItemPlace,
}

/// Where an [`Item`] is: a [`HitPlace`] but for the range a hit covers.
#[derive(Clone)]
enum ItemPlace {
    Annotation {
        page: u32,
        index: usize,
        rect: (f64, f64, f64, f64),
        text: AnnotationText,
    },
    FormField {
        name: String,
        widgets: Vec<ObjRef>,
        value: usize,
    },
    Outline {
        path: Vec<usize>,
        titles: Vec<String>,
    },
    Attachment {
        name: String,
        text: AttachmentText,
    },
}

impl ItemPlace {
    /// The place of a hit covering `range` of the item's string.
    fn with(self, range: Range<usize>) -> HitPlace {
        match self {
            ItemPlace::Annotation {
                page,
                index,
                rect,
                text,
            } => HitPlace::Annotation {
                page,
                index,
                rect,
                text,
                range,
            },
            ItemPlace::FormField {
                name,
                widgets,
                value,
            } => HitPlace::FormField {
                name,
                widgets,
                value,
                range,
            },
            ItemPlace::Outline { path, titles } => HitPlace::Outline {
                path,
                titles,
                range,
            },
            ItemPlace::Attachment { name, text } => HitPlace::Attachment { name, text, range },
        }
    }
}

/// Each page's annotations' strings, in page and `/Annots` order: each
/// annotation's `/Contents`, the text of its `/RC`, its `/Subj` and, for a
/// markup annotation, its `/T`. A pop-up's text is its parent's, read there,
/// so a pop-up is passed over; a widget's `/T` is its field's partial name,
/// which no source searches.
fn annotation_items(doc: &Document) -> Vec<Item> {
    let mut items = Vec::new();
    for index in 0..doc.page_count() {
        let Some(page) = doc.page(index) else {
            continue;
        };
        for (at, annotation) in page.annotations().into_iter().enumerate() {
            if annotation.kind == crate::AnnotationKind::Popup {
                continue;
            }
            let rich = annotation.rich_text.as_ref().map(|rich| match rich {
                RichText::Text(markup) => rich_text(markup),
                RichText::Stream(stream) => doc
                    .cos()
                    .stream_decoded(*stream)
                    .map(|bytes| rich_text(&String::from_utf8_lossy(&bytes)))
                    .unwrap_or_default(),
            });
            let author = annotation
                .kind
                .is_markup()
                .then(|| annotation.title.clone())
                .flatten();
            for (text, string) in [
                (AnnotationText::Contents, annotation.contents.clone()),
                (AnnotationText::RichText, rich),
                (AnnotationText::Subject, annotation.subject.clone()),
                (AnnotationText::Author, author),
            ] {
                if let Some(string) = string {
                    items.push(Item {
                        text: string,
                        place: ItemPlace::Annotation {
                            page: index,
                            index: at,
                            rect: annotation.rect,
                            text,
                        },
                    });
                }
            }
        }
    }
    items
}

/// The elements whose end ends a line of rich text: XHTML's blocks and its
/// line break.
const RICH_TEXT_BLOCKS: [&str; 13] = [
    "p", "div", "li", "br", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "dt", "dd",
];

/// `/RC`'s text: the character data of its XHTML (Table 170), each end of a
/// block element or a line break a line's end; text that does not parse as
/// XML, as it stands.
fn rich_text(markup: &str) -> String {
    let Ok(source) = tinker_pdf_xml::Source::new(markup.as_bytes()) else {
        return markup.to_string();
    };
    let mut out = String::new();
    for event in source.reader(&tinker_pdf_xml::Limits::DEFAULT) {
        match event {
            Ok(tinker_pdf_xml::Event::Text(text) | tinker_pdf_xml::Event::Cdata(text)) => {
                out.push_str(&text);
            }
            Ok(tinker_pdf_xml::Event::End(name)) if RICH_TEXT_BLOCKS.contains(&name.local()) => {
                out.push('\n');
            }
            Ok(_) => {}
            Err(_) => return markup.to_string(),
        }
    }
    out
}

/// The form fields' values, in `Document::form_fields` order: a field's text
/// value, or each value a list box holds, one item each. A check box's, a
/// radio button's or a push button's state and a signature are no text.
fn field_items(doc: &Document) -> Vec<Item> {
    let mut items = Vec::new();
    for field in doc.form_fields() {
        if matches!(
            field.kind,
            FieldKind::Checkbox | FieldKind::Radio | FieldKind::PushButton | FieldKind::Signature
        ) {
            continue;
        }
        let values = match &field.value {
            FieldValue::Text(text) => vec![text.clone()],
            FieldValue::Many(values) => values.clone(),
            FieldValue::State(_) | FieldValue::None => Vec::new(),
        };
        for (value, text) in values.into_iter().enumerate() {
            items.push(Item {
                text,
                place: ItemPlace::FormField {
                    name: field.name.clone(),
                    widgets: field.widgets.clone(),
                    value,
                },
            });
        }
    }
    items
}

/// Every outline item's title, depth first.
fn outline_items(doc: &Document) -> Vec<Item> {
    fn walk(
        items: &[OutlineItem],
        path: &mut Vec<usize>,
        titles: &mut Vec<String>,
        out: &mut Vec<Item>,
    ) {
        for (index, item) in items.iter().enumerate() {
            path.push(index);
            titles.push(item.title.clone());
            out.push(Item {
                text: item.title.clone(),
                place: ItemPlace::Outline {
                    path: path.clone(),
                    titles: titles.clone(),
                },
            });
            walk(&item.children, path, titles, out);
            path.pop();
            titles.pop();
        }
    }
    let mut out = Vec::new();
    walk(&doc.outline(), &mut Vec::new(), &mut Vec::new(), &mut out);
    out
}

/// Each attachment's strings, in name order: its name, its file name where
/// it is not the name, and its description.
fn attachment_items(doc: &Document) -> Vec<Item> {
    let mut items = Vec::new();
    for attachment in doc.attachments() {
        let file_name =
            (attachment.filename != attachment.name).then(|| attachment.filename.clone());
        for (text, string) in [
            (AttachmentText::Name, Some(attachment.name.clone())),
            (AttachmentText::FileName, file_name),
            (AttachmentText::Description, attachment.description.clone()),
        ] {
            if let Some(string) = string {
                items.push(Item {
                    text: string,
                    place: ItemPlace::Attachment {
                        name: attachment.name.clone(),
                        text,
                    },
                });
            }
        }
    }
    items
}

/// A line of the page text, by its page and its index in `TextPage::lines`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LineAt {
    page: u32,
    line: u32,
}

/// One page's text as the search reads it.
struct PageText {
    text: TextPage,
    /// Each line of `text.lines()`, in that order.
    lines: Vec<LineInfo>,
}

/// What the search reads off a line before it reads its glyphs.
struct LineInfo {
    /// The block it is in, and its index among the block's lines.
    block: usize,
    row: usize,
    /// How many glyphs it has.
    units: u32,
    /// Its first glyph that is not white space, or `units` where none is.
    first: u32,
    /// One past its last glyph that is not white space, or 0 where none is.
    last: u32,
    /// Its glyphs' texts, joined.
    stated: String,
    /// For a line of an item's string, where each character starts in the
    /// string and where the line ends: one more entry than it has
    /// characters. Empty for a page's line.
    offsets: Vec<usize>,
}

impl LineInfo {
    /// Whether the line holds no glyph that is not white space: no text a
    /// search reads, which it passes over.
    fn blank(&self) -> bool {
        self.first >= self.last
    }
}

/// Whether a glyph's text is white space, or empty: one a join sets aside.
fn white(c: &TextChar) -> bool {
    c.text.chars().all(char::is_whitespace)
}

/// A quad that places nothing: an item's characters are no glyphs on a page.
const ZERO_QUAD: Quad = Quad {
    ul: (0.0, 0.0),
    ur: (0.0, 0.0),
    ll: (0.0, 0.0),
    lr: (0.0, 0.0),
};

/// One character of an item's string, as a glyph the search reads.
fn char_unit(c: char) -> TextChar {
    TextChar {
        text: c.to_string(),
        quad: ZERO_QUAD,
        size: 0.0,
        origin: (0.0, 0.0),
        mcid: None,
        stream: 0,
        font: None,
        stated: None,
        artifact: false,
        glyph: None,
    }
}

/// A count of glyphs or lines as a position holds it.
fn index_u32(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

impl PageText {
    fn read(text: TextPage) -> PageText {
        let mut lines = Vec::new();
        for (block, lines_of) in text.blocks().iter().enumerate() {
            for (row, line) in lines_of.lines.iter().enumerate() {
                let units = line.chars.len();
                let first = line.chars.iter().position(|c| !white(c)).unwrap_or(units);
                let last = line
                    .chars
                    .iter()
                    .rposition(|c| !white(c))
                    .map_or(0, |at| at + 1);
                lines.push(LineInfo {
                    block,
                    row,
                    units: index_u32(units),
                    first: index_u32(first),
                    last: index_u32(last),
                    stated: line.chars.iter().map(|c| c.text.as_str()).collect(),
                    offsets: Vec::new(),
                });
            }
        }
        PageText { text, lines }
    }

    /// An item's string read as a page of one block: its lines split at
    /// `\r\n`, `\r` and `\n`, each character a glyph of its own, so the
    /// search reads it as it reads a page, its characters folded one at a
    /// time.
    fn of_item(item: &str) -> PageText {
        let mut lines = Vec::new();
        let mut offsets = Vec::new();
        let mut start = 0usize;
        let bytes = item.as_bytes();
        let mut at = 0usize;
        while at <= bytes.len() {
            let end_here = at == bytes.len() || matches!(bytes.get(at), Some(b'\r' | b'\n'));
            if end_here {
                let line = item.get(start..at).unwrap_or("");
                let chars: Vec<TextChar> = line.chars().map(char_unit).collect();
                let mut starts: Vec<usize> = line.char_indices().map(|(i, _)| start + i).collect();
                starts.push(at);
                offsets.push(starts);
                lines.push(TextLine {
                    text: line.to_string(),
                    chars,
                    quad: ZERO_QUAD,
                    wmode: WritingMode::Horizontal,
                    rtl: false,
                    size: 0.0,
                });
                // `\r\n` is one line end.
                at += if bytes.get(at) == Some(&b'\r') && bytes.get(at + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = at;
            } else {
                at += 1;
            }
        }
        let text = TextPage::new(
            vec![TextBlock {
                lines,
                quad: ZERO_QUAD,
            }],
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
        );
        let mut page = PageText::read(text);
        for (info, starts) in page.lines.iter_mut().zip(offsets) {
            info.offsets = starts;
        }
        page
    }

    fn info(&self, line: u32) -> Option<&LineInfo> {
        self.lines.get(line as usize)
    }

    fn text_line(&self, line: u32) -> Option<&TextLine> {
        let info = self.info(line)?;
        self.text.blocks().get(info.block)?.lines.get(info.row)
    }
}

/// Which join stands between two lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    /// No hit crosses it.
    Boundary,
    /// The lines meet at a hyphen, removed, with nothing between them.
    Hyphen,
    /// The lines meet across one space.
    Space,
}

/// What meets a chunk's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// One space, from the line before.
    Space,
    /// The run's start.
    Edge,
}

/// What meets a chunk's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// The run's end, by the options or the document's.
    Edge,
    /// One space, where another line follows, decided when a scan goes on.
    Open,
}

/// How one line of a chunk ends: joined to the next at a hyphen, or ending
/// the chunk.
#[derive(Clone, Copy, Debug)]
enum LineEnd {
    Joined(LineAt),
    Ends(End),
}

/// One line of a chunk: where it is, the page it is on, the glyphs it reads,
/// and whether a hyphen joins it to the next.
struct ChunkLine {
    at: LineAt,
    page: Arc<PageText>,
    first: u32,
    last: u32,
    hyphen: bool,
}

/// One glyph's piece of a chunk's text.
#[derive(Clone, Copy, Debug)]
struct Piece {
    /// Its line, by its index in the chunk's lines.
    line: usize,
    unit: u32,
    start: usize,
    end: usize,
}

/// The lines a hyphen joins, read as one text.
struct Chunk {
    lines: Vec<ChunkLine>,
    /// The chunk's folded text.
    text: String,
    /// Each glyph's piece, in order.
    pieces: Vec<Piece>,
    /// The chunk's UAX #29 boundaries as its run reads them, for a whole-word
    /// search; none otherwise.
    boundaries: Vec<usize>,
    end: End,
}

impl Chunk {
    fn start(&self) -> Option<LineAt> {
        self.lines.first().map(|line| line.at)
    }

    fn last_line(&self) -> Option<LineAt> {
        self.lines.last().map(|line| line.at)
    }

    /// Whether `offset`, in the chunk's text, is a word boundary there.
    fn boundary(&self, offset: usize) -> bool {
        self.boundaries.binary_search(&offset).is_ok()
    }
}

/// What a search reads one stretch through: the document's pages as it
/// reaches them, or a source's strings, each read as a page of its own; the
/// needle and the options.
struct Reader {
    doc: Document,
    options: DocumentSearchOptions,
    /// The folded needle; `None` finds nothing.
    needle: Option<Needle>,
    /// How many pages the stretch has: the document's, or the source's
    /// strings.
    page_count: u32,
    /// A source's strings, for a stretch beside the page text; `None` for
    /// the page text, whose pages are the document's.
    items: Option<Vec<Item>>,
    /// The pages the search keeps, by index.
    pages: BTreeMap<u32, Arc<PageText>>,
    /// How many pages the search has extracted.
    pages_read: u64,
}

/// How many pages a search keeps beside the ones its scan stands on.
const KEPT_PAGES: usize = 8;

impl Reader {
    /// The page text's reader.
    fn pages(doc: &Document, options: &DocumentSearchOptions, needle: Needle) -> Reader {
        Reader {
            doc: doc.clone(),
            options: options.clone(),
            needle: Some(needle),
            page_count: doc.page_count(),
            items: None,
            pages: BTreeMap::new(),
            pages_read: 0,
        }
    }

    /// The reader of a source's strings, each its own page and its own run.
    fn items(
        doc: &Document,
        options: &DocumentSearchOptions,
        needle: Needle,
        items: Vec<Item>,
    ) -> Reader {
        Reader {
            doc: doc.clone(),
            options: options.clone(),
            needle: Some(needle),
            page_count: index_u32(items.len()),
            items: Some(items),
            pages: BTreeMap::new(),
            pages_read: 0,
        }
    }

    /// `raw`, a hit this reader's stream decided in stretch `stretch`, as the
    /// document's: its positions the document's, and its place a source's
    /// string and the bytes of it the hit covers where it is not the page
    /// text's.
    fn hit(&self, stretch: usize, raw: RawHit, attachment_path: Vec<String>) -> SearchHit {
        let item = self
            .items
            .as_ref()
            .and_then(|items| items.get(raw.item as usize));
        let place = match (item, raw.range) {
            (Some(item), Some(range)) => item.place.clone().with(range),
            _ => HitPlace::Pages(raw.pages),
        };
        SearchHit {
            place,
            text: raw.text,
            before: raw.before,
            after: raw.after,
            attachment_path,
            start: SearchPosition(joined(stretch, raw.start)),
            end: SearchPosition(joined(stretch, raw.end)),
        }
    }

    /// Page `index`'s text, extracted now where the search keeps none; a
    /// source's string read as a page.
    fn page(&mut self, index: u32) -> Arc<PageText> {
        if let Some(page) = self.pages.get(&index) {
            return Arc::clone(page);
        }
        let page = match self.items.as_ref() {
            Some(items) => Arc::new(PageText::of_item(
                items
                    .get(index as usize)
                    .map_or("", |item| item.text.as_str()),
            )),
            None => {
                let text = self
                    .doc
                    .page(index)
                    .map(|page| page.text())
                    .unwrap_or_default();
                self.pages_read += 1;
                Arc::new(PageText::read(text))
            }
        };
        if self.pages.len() >= KEPT_PAGES {
            // The kept page farthest from this one goes; a scan still
            // standing on it holds it.
            let far = self
                .pages
                .keys()
                .copied()
                .max_by_key(|&kept| kept.abs_diff(index));
            if let Some(far) = far {
                self.pages.remove(&far);
            }
        }
        self.pages.insert(index, Arc::clone(&page));
        page
    }

    /// The first line with text at or after line `line` of page `page`.
    fn first_line_from(&mut self, mut page: u32, line: u32) -> Option<LineAt> {
        let mut line = line as usize;
        while page < self.page_count {
            let text = self.page(page);
            while let Some(info) = text.lines.get(line) {
                if !info.blank() {
                    return Some(LineAt {
                        page,
                        line: index_u32(line),
                    });
                }
                line += 1;
            }
            page += 1;
            line = 0;
        }
        None
    }

    /// The last line with text before line `line` of page `page`, or on a
    /// page before it; a `line` past the page's last reads from its last.
    fn last_line_to(&mut self, mut page: u32, line: u32) -> Option<LineAt> {
        let mut line = line as usize;
        loop {
            if page < self.page_count {
                let text = self.page(page);
                let mut at = line.min(text.lines.len());
                while at > 0 {
                    at -= 1;
                    if text.lines.get(at).is_some_and(|info| !info.blank()) {
                        return Some(LineAt {
                            page,
                            line: index_u32(at),
                        });
                    }
                }
            }
            if page == 0 {
                return None;
            }
            page = page.min(self.page_count) - 1;
            line = usize::MAX;
        }
    }

    fn next_line(&mut self, at: LineAt) -> Option<LineAt> {
        self.first_line_from(at.page, at.line.saturating_add(1))
    }

    fn prev_line(&mut self, at: LineAt) -> Option<LineAt> {
        // `last_line_to` reads lines before the one it is given.
        self.last_line_to(at.page, at.line)
    }

    /// The line `spot` stands in, or the first line with text after it. A
    /// reader reads its stretch's positions as the page text's.
    fn line_at_or_after(&mut self, spot: &Spot) -> Option<LineAt> {
        match *spot {
            Spot::Text { page, line, .. } => self.first_line_from(page, line),
            Spot::Item { .. } | Spot::Attached { .. } | Spot::End => None,
        }
    }

    /// The line `spot` stands in, or the last line with text before it: the
    /// last line with text before the line after the spot's.
    fn line_at_or_before(&mut self, spot: &Spot) -> Option<LineAt> {
        match *spot {
            Spot::Text { page, line, .. } if page < self.page_count => {
                self.last_line_to(page, line.saturating_add(1))
            }
            _ => self.last_line_to(self.page_count, 0),
        }
    }

    /// Whether the line's leading white space is set aside: wherever the
    /// options join lines before it.
    fn trims_left(&self, page: &PageText, at: LineAt) -> bool {
        if self.options.keep_lines {
            return false;
        }
        !(self.options.keep_paragraphs && starts_block(page, at.line))
    }

    /// Whether the line's trailing white space is set aside: wherever the
    /// options join lines after it.
    fn trims_right(&self, page: &PageText, at: LineAt) -> bool {
        if self.options.keep_lines {
            return false;
        }
        !(self.options.keep_paragraphs && ends_block(page, at.line))
    }

    /// The join between `a` and the line with text after it, `b`: a
    /// boundary between two of a source's strings, each its own run.
    fn join(&mut self, a: LineAt, b: LineAt) -> Join {
        if self.options.keep_lines || (self.items.is_some() && a.page != b.page) {
            return Join::Boundary;
        }
        let (first, second) = (self.page(a.page), self.page(b.page));
        let (Some(one), Some(two)) = (first.info(a.line), second.info(b.line)) else {
            return Join::Boundary;
        };
        if self.options.keep_paragraphs && (a.page != b.page || one.block != two.block) {
            return Join::Boundary;
        }
        if !self.options.keep_hyphens && hyphen_join(&one.stated, Some(&two.stated)).is_some() {
            return Join::Hyphen;
        }
        Join::Space
    }

    /// What meets the chunk that starts at `at`.
    fn side_before(&mut self, at: LineAt) -> Side {
        match self.prev_line(at) {
            Some(previous) if self.join(previous, at) != Join::Boundary => Side::Space,
            _ => Side::Edge,
        }
    }

    /// The start of the chunk `line` is in.
    fn chunk_start_of(&mut self, line: LineAt) -> LineAt {
        let mut at = line;
        while let Some(previous) = self.prev_line(at) {
            if self.join(previous, at) != Join::Hyphen {
                break;
            }
            at = previous;
        }
        at
    }

    /// The start of the chunk before the one starting at `start`, in its run.
    fn chunk_before(&mut self, start: LineAt) -> Option<LineAt> {
        let previous = self.prev_line(start)?;
        if self.join(previous, start) == Join::Boundary {
            return None;
        }
        Some(self.chunk_start_of(previous))
    }

    /// The start of the run before the one whose first chunk starts at
    /// `start`: the start of its last chunk.
    fn run_before(&mut self, start: LineAt) -> Option<LineAt> {
        let previous = self.prev_line(start)?;
        Some(self.chunk_start_of(previous))
    }

    /// The line that follows a chunk ending as `end` after `last` across a
    /// space, in the same run; `None` at the run's end.
    fn after_space(&mut self, end: End, last: Option<LineAt>) -> Option<LineAt> {
        if end != End::Open {
            return None;
        }
        let last = last?;
        let next = self.next_line(last)?;
        (self.join(last, next) != Join::Boundary).then_some(next)
    }

    /// The chunk that starts at `start`, `before` meeting its start.
    fn chunk(&mut self, start: LineAt, before: Side) -> Chunk {
        let keep_hyphens = self.options.keep_hyphens;
        let search = self.options.search;
        let mut lines = Vec::new();
        let mut text = String::new();
        let mut pieces = Vec::new();
        let mut at = start;
        let end = loop {
            let page = self.page(at.page);
            let Some(info) = page.info(at.line) else {
                break End::Edge;
            };
            let first = if self.trims_left(&page, at) {
                info.first
            } else {
                0
            };
            let last = if self.trims_right(&page, at) {
                info.last
            } else {
                info.units
            };
            // How the line ends: at the run's edge, where the options keep it
            // apart from the next; joined to the next at a hyphen, which only
            // a line ending in one can be and the next line decides; or across
            // a space, which a scan decides when it goes on.
            let ends = if self.options.keep_lines
                || (self.options.keep_paragraphs && ends_block(&page, at.line))
            {
                LineEnd::Ends(End::Edge)
            } else if !keep_hyphens && hyphen_join(&info.stated, Some("a")).is_some() {
                match self.next_line(at) {
                    Some(next) => match self.join(at, next) {
                        Join::Hyphen => LineEnd::Joined(next),
                        Join::Space => LineEnd::Ends(End::Open),
                        Join::Boundary => LineEnd::Ends(End::Edge),
                    },
                    None => LineEnd::Ends(End::Edge),
                }
            } else {
                LineEnd::Ends(End::Open)
            };
            let hyphen = matches!(ends, LineEnd::Joined(_));
            let folded = page.text.folded_lines(&search);
            let line = folded.get(at.line as usize);
            let index = lines.len();
            for unit in first..last {
                let mut piece: String = line
                    .map(|line| line.piece(unit as usize))
                    .unwrap_or("")
                    .to_string();
                if !keep_hyphens {
                    piece.retain(|c| c != '\u{AD}');
                }
                if hyphen && unit + 1 == last {
                    let trimmed = piece.trim_end();
                    let cut = trimmed
                        .strip_suffix(['-', '\u{2010}'])
                        .map(str::len)
                        .unwrap_or(trimmed.len());
                    piece.truncate(cut);
                }
                let start = text.len();
                text.push_str(&piece);
                pieces.push(Piece {
                    line: index,
                    unit,
                    start,
                    end: text.len(),
                });
            }
            lines.push(ChunkLine {
                at,
                page: Arc::clone(&page),
                first,
                last,
                hyphen,
            });
            match ends {
                LineEnd::Joined(next) => at = next,
                LineEnd::Ends(end) => break end,
            }
        };
        let boundaries = if search.whole_word {
            let left = if before == Side::Space { " " } else { "" };
            let right = if end == End::Open { " " } else { "" };
            let padded = format!("{left}{text}{right}");
            word_boundaries(&padded)
                .into_iter()
                .filter_map(|at| at.checked_sub(left.len()))
                .filter(|&at| at <= text.len())
                .collect()
        } else {
            Vec::new()
        };
        Chunk {
            lines,
            text,
            pieces,
            boundaries,
            end,
        }
    }

    /// The latest clean chunk start at or before `start`, a chunk start: one
    /// no qualifying occurrence crosses, or its run's start.
    fn clean_start(&mut self, start: LineAt) -> LineAt {
        let length = self.needle.as_ref().map_or(0, Needle::len);
        let mut at = start;
        loop {
            if length <= 1 || self.side_before(at) == Side::Edge || self.is_clean(at, length) {
                return at;
            }
            match self.chunk_before(at) {
                Some(before) => at = before,
                None => return at,
            }
        }
    }

    /// Whether no qualifying occurrence of a needle `length` bytes long
    /// starts before the chunk that starts at `start` and ends after its
    /// start, read off `length` less one byte on either side of it.
    fn is_clean(&mut self, start: LineAt, length: usize) -> bool {
        let reach = length.saturating_sub(1);
        // The chunks before it, to `reach` bytes or the run's start.
        let mut window: VecDeque<Chunk> = VecDeque::new();
        let mut behind = 0usize;
        let mut at = start;
        while behind < reach {
            let Some(before) = self.chunk_before(at) else {
                break;
            };
            let side = self.side_before(before);
            let chunk = self.chunk(before, side);
            behind += chunk.text.len() + 1;
            window.push_front(chunk);
            at = before;
        }
        if window.is_empty() {
            return true;
        }
        // The chunk itself and those after it, to `reach` bytes or the run's
        // end.
        let mut after = 0usize;
        let mut chunk = self.chunk(start, Side::Space);
        loop {
            after += chunk.text.len();
            let (end, last) = (chunk.end, chunk.last_line());
            window.push_back(chunk);
            if after >= reach {
                break;
            }
            let Some(next) = self.after_space(end, last) else {
                break;
            };
            after += 1;
            chunk = self.chunk(next, Side::Space);
        }
        let scan = Stream::over(window);
        let Some(origin) = scan.base_of(start) else {
            return true;
        };
        let whole_word = self.options.search.whole_word;
        let mut crossing = false;
        scan.occurrences(self, |stream, s, e| {
            if s < origin && e > origin && stream.qualifies_with(whole_word, s, e) {
                crossing = true;
            }
        });
        !crossing
    }

    /// The scan `next` reads from `from`, started at the latest clean chunk
    /// start at or before it; `None` where nothing is after it.
    fn stream_from(&mut self, from: &Spot) -> Option<Stream> {
        self.needle.as_ref()?;
        let line = self.line_at_or_after(from)?;
        let chunk_start = self.chunk_start_of(line);
        let start = self.clean_start(chunk_start);
        let side = self.side_before(start);
        let chunk = self.chunk(start, side);
        Some(Stream::over(VecDeque::from([chunk])))
    }

    /// The last hit that ends at or before `to`.
    fn previous_before(&mut self, to: &Spot) -> Option<RawHit> {
        self.needle.as_ref()?;
        let line = self.line_at_or_before(to)?;
        let mut last_chunk = self.chunk_start_of(line);
        loop {
            let start = self.clean_start(last_chunk);
            let side = self.side_before(start);
            let first = self.chunk(start, side);
            let mut stream = Stream::over(VecDeque::from([first]));
            let mut found: Option<RawHit> = None;
            let mut decided = VecDeque::new();
            loop {
                let reached = stream.feeding() == Some(last_chunk) && stream.fed_whole();
                if reached {
                    break;
                }
                let more = stream.pump(self, &mut decided);
                for hit in decided.drain(..) {
                    if hit.end <= *to {
                        found = Some(hit);
                    }
                }
                if !more {
                    break;
                }
            }
            if found.is_some() {
                return found;
            }
            // Every hit before the clean start ends there, at or before `to`.
            last_chunk = match self.chunk_before(start) {
                Some(before) => before,
                None => self.run_before(start)?,
            };
        }
    }
}

/// Whether line `line` of `page` opens its block.
fn starts_block(page: &PageText, line: u32) -> bool {
    let Some(index) = (line as usize).checked_sub(1) else {
        return true;
    };
    match (page.lines.get(index), page.info(line)) {
        (Some(before), Some(this)) => before.block != this.block,
        _ => true,
    }
}

/// Whether line `line` of `page` closes its block.
fn ends_block(page: &PageText, line: u32) -> bool {
    match (page.info(line), page.lines.get(line as usize + 1)) {
        (Some(this), Some(after)) => after.block != this.block,
        _ => true,
    }
}

/// A scan of a run: the chunks it reads, each at its offset in the scan, the
/// matcher's state, and the first offset the next hit may start at.
struct Stream {
    /// The chunks fed or being fed, oldest first, each at its offset.
    chunks: VecDeque<(u64, Chunk)>,
    /// How many bytes of the newest chunk are fed.
    fed: usize,
    /// How much of the needle the bytes fed match.
    matched: usize,
    /// The first offset the next hit may start at.
    allowed: u64,
}

impl Stream {
    /// A scan over `chunks`, joined by spaces, from the first one's start.
    fn over(chunks: VecDeque<Chunk>) -> Stream {
        let mut at = 0u64;
        let mut placed = VecDeque::with_capacity(chunks.len());
        for chunk in chunks {
            let length = chunk.text.len() as u64;
            placed.push_back((at, chunk));
            at += length + 1;
        }
        // A scan built over several chunks is read whole by `occurrences`;
        // one built over one is fed by `pump`, which needs to know it has
        // fed none of it yet.
        Stream {
            chunks: placed,
            fed: 0,
            matched: 0,
            allowed: 0,
        }
    }

    /// The offset of the chunk that starts at `start`.
    fn base_of(&self, start: LineAt) -> Option<u64> {
        self.chunks
            .iter()
            .find(|(_, chunk)| chunk.start() == Some(start))
            .map(|(base, _)| *base)
    }

    /// The chunk being fed, by its start.
    fn feeding(&self) -> Option<LineAt> {
        self.chunks.back().and_then(|(_, chunk)| chunk.start())
    }

    /// Whether every byte of the chunk being fed is fed.
    fn fed_whole(&self) -> bool {
        self.chunks
            .back()
            .is_some_and(|(_, chunk)| self.fed >= chunk.text.len())
    }

    /// The chunk holding `offset` in its range — its text, and the space after
    /// it at its end — and the offset within it.
    fn locate(&self, offset: u64) -> Option<(&Chunk, usize)> {
        self.chunks.iter().find_map(|(base, chunk)| {
            let within = offset.checked_sub(*base)?;
            let within = usize::try_from(within).ok()?;
            (within <= chunk.text.len()).then_some((chunk, within))
        })
    }

    /// Whether `offset` is a word boundary of the run.
    fn boundary(&self, offset: u64) -> bool {
        self.locate(offset)
            .is_some_and(|(chunk, within)| chunk.boundary(within))
    }

    /// Whether the occurrence `s..e` qualifies: for a whole-word search, its
    /// two ends are boundaries.
    fn qualifies_with(&self, whole_word: bool, s: u64, e: u64) -> bool {
        !whole_word || (self.boundary(s) && self.boundary(e))
    }

    /// Every occurrence of the needle in the chunks the scan holds, joined by
    /// spaces, each handed to `each` with the scan, by its start and end.
    fn occurrences(&self, reader: &Reader, mut each: impl FnMut(&Stream, u64, u64)) {
        let Some(needle) = reader.needle.as_ref() else {
            return;
        };
        let length = needle.len() as u64;
        let mut matched = 0usize;
        let mut ends = Vec::new();
        for (index, (base, chunk)) in self.chunks.iter().enumerate() {
            for (at, &byte) in chunk.text.as_bytes().iter().enumerate() {
                if needle.step(&mut matched, byte) {
                    ends.push(*base + at as u64 + 1);
                }
            }
            if index + 1 < self.chunks.len() && needle.step(&mut matched, b' ') {
                ends.push(*base + chunk.text.len() as u64 + 1);
            }
        }
        for end in ends {
            if let Some(start) = end.checked_sub(length) {
                each(self, start, end);
            }
        }
    }

    /// Feeds the scan on: the newest chunk's bytes, or the space after it and
    /// the next chunk, or the next run's first chunk; every hit decided on the
    /// way pushed onto `out`. `false` once the text is read.
    fn pump(&mut self, reader: &mut Reader, out: &mut VecDeque<RawHit>) -> bool {
        let Some(length) = reader.needle.as_ref().map(Needle::len) else {
            return false;
        };
        let Some((base, end, last, fed_all)) = self.chunks.back().map(|(base, chunk)| {
            (
                *base,
                chunk.end,
                chunk.last_line(),
                self.fed >= chunk.text.len(),
            )
        }) else {
            return false;
        };
        if !fed_all {
            // The chunk's own bytes: every occurrence ending in it is decided
            // by its boundaries alone.
            let bytes: Vec<u8> = self
                .chunks
                .back()
                .and_then(|(_, chunk)| chunk.text.as_bytes().get(self.fed..))
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            let mut at = base + self.fed as u64;
            for byte in bytes {
                at += 1;
                self.fed += 1;
                self.step(reader, out, byte, at, length);
            }
            return true;
        }
        let length_of_last = self
            .chunks
            .back()
            .map_or(0, |(_, chunk)| chunk.text.len() as u64);
        let next_base = base + length_of_last + 1;
        if let Some(next) = reader.after_space(end, last) {
            // The chunk after the space first, so an occurrence ending with
            // the space is decided at its far side.
            let chunk = reader.chunk(next, Side::Space);
            self.chunks.push_back((next_base, chunk));
            self.fed = 0;
            self.step(reader, out, b' ', next_base, length);
        } else {
            // The run's end: the next line, where there is one, opens a new
            // run, and nothing carries across the edge.
            let Some(next) = last.and_then(|last| reader.next_line(last)) else {
                return false;
            };
            let chunk = reader.chunk(next, Side::Edge);
            self.chunks.push_back((next_base, chunk));
            self.fed = 0;
            self.matched = 0;
            self.allowed = next_base;
        }
        self.prune(next_base, length);
        true
    }

    /// One byte fed, ending at `at`: an occurrence ending with it decided.
    fn step(
        &mut self,
        reader: &Reader,
        out: &mut VecDeque<RawHit>,
        byte: u8,
        at: u64,
        length: usize,
    ) {
        let Some(needle) = reader.needle.as_ref() else {
            return;
        };
        if !needle.step(&mut self.matched, byte) {
            return;
        }
        let Some(start) = at.checked_sub(length as u64) else {
            return;
        };
        if start < self.allowed {
            return;
        }
        // A candidate that is not a whole word is no hit and holds nothing
        // back.
        if !self.qualifies_with(reader.options.search.whole_word, start, at) {
            return;
        }
        self.allowed = at;
        if let Some(hit) = self.hit(reader, start, at) {
            out.push_back(hit);
        }
    }

    /// Drops the chunks no occurrence still to come can reach into.
    fn prune(&mut self, at: u64, length: usize) {
        while self.chunks.len() > 1 {
            let Some((base, chunk)) = self.chunks.front() else {
                break;
            };
            if base + chunk.text.len() as u64 + 1 + length as u64 <= at {
                self.chunks.pop_front();
            } else {
                break;
            }
        }
    }

    /// The pieces the bytes `s..e` cover, each with its chunk, in order: the
    /// first whose piece ends past `s` to the last whose piece starts before
    /// `e`, as the page search maps a hit to glyphs.
    fn covered(&self, s: u64, e: u64) -> Vec<(usize, Piece)> {
        let mut out = Vec::new();
        for (index, (base, chunk)) in self.chunks.iter().enumerate() {
            for piece in &chunk.pieces {
                let (start, end) = (base + piece.start as u64, base + piece.end as u64);
                if end > s && start < e {
                    out.push((index, *piece));
                }
            }
        }
        out
    }

    /// The position before byte `offset`.
    fn spot_at(&self, offset: u64) -> Option<Spot> {
        for (base, chunk) in &self.chunks {
            let Some(within) = offset.checked_sub(*base) else {
                continue;
            };
            let within = usize::try_from(within).ok()?;
            if within > chunk.text.len() {
                continue;
            }
            for piece in &chunk.pieces {
                if piece.start <= within && within < piece.end {
                    let line = chunk.lines.get(piece.line)?;
                    return Some(Spot::Text {
                        page: line.at.page,
                        line: line.at.line,
                        unit: piece.unit,
                        within: index_u32(within - piece.start),
                    });
                }
            }
            // The space after the chunk: the end of its last line.
            let line = chunk.lines.last()?;
            let units = line.page.info(line.at.line).map_or(0, |info| info.units);
            return Some(Spot::Text {
                page: line.at.page,
                line: line.at.line,
                unit: units,
                within: 0,
            });
        }
        None
    }

    /// The position after byte `offset - 1`, the last a hit ending at
    /// `offset` covers.
    fn spot_after(&self, offset: u64) -> Option<Spot> {
        let last = offset.checked_sub(1)?;
        for (index, (base, chunk)) in self.chunks.iter().enumerate() {
            let Some(within) = last.checked_sub(*base) else {
                continue;
            };
            let within = usize::try_from(within).ok()?;
            if within > chunk.text.len() {
                continue;
            }
            for piece in &chunk.pieces {
                if piece.start <= within && within < piece.end {
                    let line = chunk.lines.get(piece.line)?;
                    let (unit, inside) = if within + 1 < piece.end {
                        (piece.unit, index_u32(within + 1 - piece.start))
                    } else {
                        (piece.unit.saturating_add(1), 0)
                    };
                    return Some(Spot::Text {
                        page: line.at.page,
                        line: line.at.line,
                        unit,
                        within: inside,
                    });
                }
            }
            // The space after the chunk: the start of the next chunk's line.
            let (_, next) = self.chunks.get(index + 1)?;
            let line = next.lines.first()?;
            return Some(Spot::Text {
                page: line.at.page,
                line: line.at.line,
                unit: 0,
                within: 0,
            });
        }
        None
    }

    /// The hit over the bytes `s..e`, or `None` where they cover no glyph — a
    /// needle of one space, matching a join alone.
    fn hit(&self, reader: &Reader, s: u64, e: u64) -> Option<RawHit> {
        let covered = self.covered(s, e);
        let (first_chunk, first) = *covered.first()?;
        let (last_chunk, last) = *covered.last()?;
        let keep_hyphens = reader.options.keep_hyphens;

        // A quad for each line covered, grouped by page.
        let mut pages: Vec<PageQuads> = Vec::new();
        let mut text = String::new();
        let mut span: Option<(usize, usize, u32, u32)> = None;
        let flush = |span: Option<(usize, usize, u32, u32)>, pages: &mut Vec<PageQuads>| {
            let Some((chunk, line, a, b)) = span else {
                return;
            };
            let Some(line) = self.chunks.get(chunk).and_then(|(_, c)| c.lines.get(line)) else {
                return;
            };
            let Some(quad) = line
                .page
                .text_line(line.at.line)
                .and_then(|text_line| text_line.span_quad(a as usize, b as usize))
            else {
                return;
            };
            match pages.last_mut() {
                Some(page) if page.page == line.at.page => page.quads.push(quad),
                _ => pages.push(PageQuads {
                    page: line.at.page,
                    quads: vec![quad],
                }),
            }
        };
        let mut previous: Option<(usize, usize)> = None;
        for &(chunk, piece) in &covered {
            match span {
                Some((c, l, a, _)) if c == chunk && l == piece.line => {
                    span = Some((c, l, a, piece.unit));
                }
                _ => {
                    flush(span, &mut pages);
                    span = Some((chunk, piece.line, piece.unit, piece.unit));
                }
            }
            // The stated text, a space where a scan crossed one.
            if let Some((c, _)) = previous {
                if c != chunk {
                    text.push(' ');
                }
            }
            if let Some(line) = self
                .chunks
                .get(chunk)
                .and_then(|(_, c)| c.lines.get(piece.line))
            {
                text.push_str(&stated(line, piece.unit, keep_hyphens));
            }
            previous = Some((chunk, piece.line));
        }
        flush(span, &mut pages);
        if pages.is_empty() {
            return None;
        }

        let line_of =
            |chunk: usize, line: usize| self.chunks.get(chunk).and_then(|(_, c)| c.lines.get(line));
        let before = line_of(first_chunk, first.line).map_or_else(String::new, |line| {
            let text: String = (line.first..first.unit)
                .map(|unit| stated(line, unit, keep_hyphens))
                .collect();
            let skip = text.chars().count().saturating_sub(HIT_CONTEXT);
            text.chars().skip(skip).collect()
        });
        let after = line_of(last_chunk, last.line).map_or_else(String::new, |line| {
            (last.unit.saturating_add(1)..line.last)
                .map(|unit| stated(line, unit, keep_hyphens))
                .collect::<String>()
                .chars()
                .take(HIT_CONTEXT)
                .collect()
        });
        // For a source's string, the bytes of it the hit covers: from where
        // its first character starts to where its last ends.
        let range = match (
            line_of(first_chunk, first.line),
            line_of(last_chunk, last.line),
        ) {
            (Some(one), Some(two)) => {
                let start = one
                    .page
                    .info(one.at.line)
                    .and_then(|info| info.offsets.get(first.unit as usize).copied());
                let end = two
                    .page
                    .info(two.at.line)
                    .and_then(|info| info.offsets.get(last.unit as usize + 1).copied());
                start.zip(end).map(|(start, end)| start..end.max(start))
            }
            _ => None,
        };
        let item = line_of(first_chunk, first.line).map_or(0, |line| line.at.page);
        Some(RawHit {
            pages,
            text,
            before,
            after,
            start: self.spot_at(s)?,
            end: self.spot_after(e)?,
            item,
            range,
        })
    }
}

/// A hit as a stretch's scan decides it, in the stretch's own positions,
/// before [`Reader::hit`] makes it the document's.
struct RawHit {
    pages: Vec<PageQuads>,
    text: String,
    before: String,
    after: String,
    start: Spot,
    end: Spot,
    /// The page it starts on — for a source, its string.
    item: u32,
    /// The bytes of a source's string it covers; `None` in the page text.
    range: Option<Range<usize>>,
}

/// Glyph `unit`'s text on `line` as a hit states it: soft hyphens removed
/// where the search removed them, and the hyphen a join removed.
fn stated(line: &ChunkLine, unit: u32, keep_hyphens: bool) -> String {
    let Some(glyph) = line
        .page
        .text_line(line.at.line)
        .and_then(|text_line| text_line.chars.get(unit as usize))
    else {
        return String::new();
    };
    let mut text = glyph.text.clone();
    if !keep_hyphens {
        text.retain(|c| c != '\u{AD}');
    }
    if line.hyphen && unit + 1 == line.last {
        let trimmed = text.trim_end();
        let cut = trimmed
            .strip_suffix(['-', '\u{2010}'])
            .map(str::len)
            .unwrap_or(trimmed.len());
        text.truncate(cut);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_cos::DocumentBuilder;

    /// A document of one page a string of `pages`, each string's lines one a
    /// line of Helvetica at 12 points, 14 apart, held to the validate-on-save
    /// helper as a document this engine wrote.
    fn document(pages: &[&str]) -> Document {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for page in pages {
            let mut content = String::from("BT /F0 12 Tf 14 TL 40 760 Td ");
            for line in page.lines() {
                let escaped = line
                    .replace('\\', "\\\\")
                    .replace('(', "\\(")
                    .replace(')', "\\)");
                content.push_str(&format!("({escaped}) Tj T* "));
            }
            content.push_str("ET");
            builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
        }
        let bytes = builder.finish();
        crate::subset::tests_support::written_support::authored("a document search", &bytes)
    }

    /// How many pages `search`'s page text reader has extracted.
    fn pages_read(search: &DocumentSearch) -> u64 {
        search.readers[0]
            .as_ref()
            .map_or(0, |reader| reader.pages_read)
    }

    /// **A forward search extracts the pages up to its hit and no further**:
    /// a hit in the middle of the fourth of eight pages extracts four; the
    /// next, on the sixth, two more; a needle on no page extracts every page
    /// once; and a page's last line ending in a hyphen reads the next page to
    /// decide its join.
    #[test]
    fn the_next_hit_extracts_only_the_pages_up_to_it() {
        let pages = [
            "alpha one\nfiller line",
            "beta two\nfiller line",
            "gamma three\nfiller line",
            "first needle here\nfiller line",
            "delta five\nfiller line",
            "second needle here\nfiller line",
            "epsilon seven\nfiller line",
            "zeta eight\nfiller line",
        ];
        let doc = document(&pages);
        let mut search = doc.search("needle", &DocumentSearchOptions::default());
        let hit = search.next().expect("the first needle");
        assert!(
            matches!(&hit.place, HitPlace::Pages(pages) if pages.len() == 1 && pages[0].page == 3),
            "{:?}",
            hit.place
        );
        assert_eq!(hit.text, "needle");
        assert_eq!(hit.before, "first ");
        assert_eq!(hit.after, " here");
        assert_eq!(pages_read(&search), 4, "pages 1 to 4, and not 5");
        let second = search.next().expect("the second needle");
        assert!(
            matches!(&second.place, HitPlace::Pages(pages) if pages.len() == 1 && pages[0].page == 5)
        );
        assert_eq!(pages_read(&search), 6, "two more pages");
        assert!(search.next().is_none());
        assert_eq!(pages_read(&search), 8, "every page read once");

        let mut nowhere = doc.search("absent", &DocumentSearchOptions::default());
        assert!(nowhere.next().is_none());
        assert_eq!(pages_read(&nowhere), 8, "each page once");

        // The join at a page's end ending in a hyphen is the next page's to
        // decide, so a hit on that line reads one page more.
        let hyphenated = document(&["the hyphen-", "ation of it"]);
        let mut search = hyphenated.search("the", &DocumentSearchOptions::default());
        assert!(search.next().is_some());
        assert_eq!(pages_read(&search), 2);
        let plain = document(&["the line", "after it"]);
        let mut search = plain.search("the", &DocumentSearchOptions::default());
        assert!(search.next().is_some());
        assert_eq!(pages_read(&search), 1, "no hyphen, no next page");
    }

    /// The pages a search keeps are bounded, whatever it reads.
    #[test]
    fn a_search_keeps_a_bounded_number_of_pages() {
        let pages: Vec<String> = (0..20).map(|i| format!("page {i} text")).collect();
        let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
        let doc = document(&pages);
        let mut search = doc.search("text", &DocumentSearchOptions::default());
        assert_eq!(search.by_ref().count(), 20);
        assert!(
            search.readers[0]
                .as_ref()
                .map_or(0, |reader| reader.pages.len())
                <= KEPT_PAGES
        );
        assert_eq!(pages_read(&search), 20);
    }

    /// `/RC`'s text is its character data, a block or a line break a line's
    /// end, references resolved; text that is not XML is read as it stands.
    #[test]
    fn rich_text_reads_its_character_data_a_block_a_line() {
        assert_eq!(
            rich_text("<body><p>One <b>two</b></p><p>three</p></body>"),
            "One two\nthree\n"
        );
        assert_eq!(rich_text("<body>a<br/>b</body>"), "a\nb");
        assert_eq!(rich_text("<p>fish &amp; chips</p>"), "fish & chips\n");
        assert_eq!(rich_text("not <xml"), "not <xml");
        assert_eq!(rich_text("<p>unclosed"), "<p>unclosed");
    }

    /// An item's string is read as lines split at `\r\n`, `\r` and `\n`, each
    /// character a glyph, each knowing where it starts in the string.
    #[test]
    fn an_items_lines_and_offsets_are_its_strings() {
        let page = PageText::of_item("one\r\ntwo\rthree\nf\u{e9}");
        let stated: Vec<&str> = page.lines.iter().map(|info| info.stated.as_str()).collect();
        assert_eq!(stated, ["one", "two", "three", "f\u{e9}"]);
        assert_eq!(page.lines[0].offsets, [0, 1, 2, 3]);
        assert_eq!(page.lines[1].offsets, [5, 6, 7, 8]);
        assert_eq!(page.lines[2].offsets, [9, 10, 11, 12, 13, 14]);
        assert_eq!(
            page.lines[3].offsets,
            [15, 16, 18],
            "a character of two bytes"
        );
        assert!(page.lines.iter().all(|info| info.block == 0), "one block");
        let trailing = PageText::of_item("end\n");
        assert_eq!(trailing.lines.len(), 2);
        assert!(
            trailing.lines[1].blank(),
            "the empty last line holds no text"
        );
        assert!(PageText::of_item("").lines[0].blank());
    }

    /// Positions order as the document reads: page, line, glyph, byte.
    #[test]
    fn positions_order_as_the_document_reads() {
        let at = |page, line, unit, within| {
            SearchPosition(Spot::Text {
                page,
                line,
                unit,
                within,
            })
        };
        assert!(SearchPosition::start() < at(0, 0, 0, 1));
        assert!(at(0, 0, 3, 1) < at(0, 0, 4, 0));
        assert!(at(0, 1, 0, 0) < at(1, 0, 0, 0));
        assert!(at(9, 9, 9, 9) < SearchPosition::end());
        assert_eq!(SearchPosition::page(0), SearchPosition::start());
        assert!(SearchPosition::page(2) > at(1, 50, 0, 0));
    }
}
