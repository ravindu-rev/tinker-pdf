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

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use tinker_pdf_content::{
    hyphen_join, word_boundaries, Needle, Quad, SearchOptions, TextChar, TextLine, TextPage,
};

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
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum HitPlace {
    /// In the page text: each page the hit covers, in order — two or more
    /// for a hit that crosses a page break.
    Pages(Vec<PageQuads>),
}

/// One hit of a [`DocumentSearch`].
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// Where it was found.
    pub place: HitPlace,
    /// The text it covers as the page states it: the text of every glyph it
    /// covers, the lines it crosses joined as the search joined them.
    pub text: String,
    /// Up to [`HIT_CONTEXT`] characters of its first line before it, as
    /// stated.
    pub before: String,
    /// Up to [`HIT_CONTEXT`] characters of its last line after it, as
    /// stated.
    pub after: String,
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
    /// It is encrypted and no password has opened it.
    PasswordRequired,
    /// It is encrypted with a handler this build does not implement.
    UnsupportedEncryption,
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
    reader: Reader,
    /// The cursor's position.
    position: Spot,
    /// The scan `next` continues, while the position is where it left it.
    stream: Option<Stream>,
    /// Hits the scan decided that `next` has not returned.
    pending: VecDeque<SearchHit>,
    /// What the search could not read.
    unsearched: Vec<Unsearched>,
}

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
        let mut unsearched = Vec::new();
        let readable = match self.readable() {
            Ok(()) => true,
            Err(error) => {
                unsearched.push(Unsearched {
                    attachment_path: Vec::new(),
                    reason: match error {
                        DocumentError::PasswordRequired => UnsearchedReason::PasswordRequired,
                        DocumentError::UnsupportedEncryption => {
                            UnsearchedReason::UnsupportedEncryption
                        }
                    },
                });
                false
            }
        };
        DocumentSearch {
            reader: Reader {
                needle: Needle::new(needle, &options.search).filter(|_| readable),
                page_count: if readable { self.page_count() } else { 0 },
                doc: self.clone(),
                options: options.clone(),
                pages: BTreeMap::new(),
                pages_read: 0,
            },
            position: Spot::START,
            stream: None,
            pending: VecDeque::new(),
            unsearched,
        }
    }
}

impl DocumentSearch {
    /// The last hit that ends at or before the cursor's position, the
    /// position moved to its start; `None`, the position moved to the
    /// document's start, where there is none.
    pub fn previous(&mut self) -> Option<SearchHit> {
        self.stream = None;
        self.pending.clear();
        let hit = self.reader.previous_before(&self.position);
        self.position = hit.as_ref().map_or(Spot::START, |hit| hit.start.0.clone());
        hit
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

    /// What the search reached and could not read.
    #[must_use]
    pub fn unsearched(&self) -> &[Unsearched] {
        &self.unsearched
    }
}

impl Iterator for DocumentSearch {
    type Item = SearchHit;

    /// The first hit that starts at or after the cursor's position, the
    /// position moved to its end; `None`, the position moved to the
    /// document's end, past the last.
    fn next(&mut self) -> Option<SearchHit> {
        let from = self.position.clone();
        if self.stream.is_none() {
            self.pending.clear();
            self.stream = self.reader.stream_from(&from);
        }
        loop {
            while let Some(hit) = self.pending.pop_front() {
                if hit.start.0 >= from {
                    self.position = hit.end.0.clone();
                    return Some(hit);
                }
            }
            let more = match self.stream.as_mut() {
                Some(stream) => stream.pump(&mut self.reader, &mut self.pending),
                None => false,
            };
            if !more && self.pending.is_empty() {
                self.stream = None;
                self.position = Spot::End;
                return None;
            }
        }
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
                });
            }
        }
        PageText { text, lines }
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

/// What a search reads through: the document, its pages as it reaches them,
/// the needle and the options.
struct Reader {
    doc: Document,
    options: DocumentSearchOptions,
    /// The folded needle; `None` finds nothing.
    needle: Option<Needle>,
    page_count: u32,
    /// The pages the search keeps, by index.
    pages: BTreeMap<u32, Arc<PageText>>,
    /// How many pages the search has extracted.
    pages_read: u64,
}

/// How many pages a search keeps beside the ones its scan stands on.
const KEPT_PAGES: usize = 8;

impl Reader {
    /// Page `index`'s text, extracted now where the search keeps none.
    fn page(&mut self, index: u32) -> Arc<PageText> {
        if let Some(page) = self.pages.get(&index) {
            return Arc::clone(page);
        }
        let text = self
            .doc
            .page(index)
            .map(|page| page.text())
            .unwrap_or_default();
        self.pages_read += 1;
        let page = Arc::new(PageText::read(text));
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

    /// The line `spot` stands in, or the first line with text after it.
    fn line_at_or_after(&mut self, spot: &Spot) -> Option<LineAt> {
        match *spot {
            Spot::Text { page, line, .. } => self.first_line_from(page, line),
            Spot::End => None,
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

    /// The join between `a` and the line with text after it, `b`.
    fn join(&mut self, a: LineAt, b: LineAt) -> Join {
        if self.options.keep_lines {
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
    fn previous_before(&mut self, to: &Spot) -> Option<SearchHit> {
        self.needle.as_ref()?;
        let line = self.line_at_or_before(to)?;
        let mut last_chunk = self.chunk_start_of(line);
        loop {
            let start = self.clean_start(last_chunk);
            let side = self.side_before(start);
            let first = self.chunk(start, side);
            let mut stream = Stream::over(VecDeque::from([first]));
            let mut found: Option<SearchHit> = None;
            let mut decided = VecDeque::new();
            loop {
                let reached = stream.feeding() == Some(last_chunk) && stream.fed_whole();
                if reached {
                    break;
                }
                let more = stream.pump(self, &mut decided);
                for hit in decided.drain(..) {
                    if hit.end.0 <= *to {
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
    fn pump(&mut self, reader: &mut Reader, out: &mut VecDeque<SearchHit>) -> bool {
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
        out: &mut VecDeque<SearchHit>,
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
    fn hit(&self, reader: &Reader, s: u64, e: u64) -> Option<SearchHit> {
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
        Some(SearchHit {
            place: HitPlace::Pages(pages),
            text,
            before,
            after,
            start: SearchPosition(self.spot_at(s)?),
            end: SearchPosition(self.spot_after(e)?),
        })
    }
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
        assert_eq!(search.reader.pages_read, 4, "pages 1 to 4, and not 5");
        let second = search.next().expect("the second needle");
        assert!(
            matches!(&second.place, HitPlace::Pages(pages) if pages.len() == 1 && pages[0].page == 5)
        );
        assert_eq!(search.reader.pages_read, 6, "two more pages");
        assert!(search.next().is_none());
        assert_eq!(search.reader.pages_read, 8, "every page read once");

        let mut nowhere = doc.search("absent", &DocumentSearchOptions::default());
        assert!(nowhere.next().is_none());
        assert_eq!(nowhere.reader.pages_read, 8, "each page once");

        // The join at a page's end ending in a hyphen is the next page's to
        // decide, so a hit on that line reads one page more.
        let hyphenated = document(&["the hyphen-", "ation of it"]);
        let mut search = hyphenated.search("the", &DocumentSearchOptions::default());
        assert!(search.next().is_some());
        assert_eq!(search.reader.pages_read, 2);
        let plain = document(&["the line", "after it"]);
        let mut search = plain.search("the", &DocumentSearchOptions::default());
        assert!(search.next().is_some());
        assert_eq!(search.reader.pages_read, 1, "no hyphen, no next page");
    }

    /// The pages a search keeps are bounded, whatever it reads.
    #[test]
    fn a_search_keeps_a_bounded_number_of_pages() {
        let pages: Vec<String> = (0..20).map(|i| format!("page {i} text")).collect();
        let pages: Vec<&str> = pages.iter().map(String::as_str).collect();
        let doc = document(&pages);
        let mut search = doc.search("text", &DocumentSearchOptions::default());
        assert_eq!(search.by_ref().count(), 20);
        assert!(search.reader.pages.len() <= KEPT_PAGES);
        assert_eq!(search.reader.pages_read, 20);
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
