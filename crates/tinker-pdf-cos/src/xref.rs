//! Cross-reference machinery: classic tables (7.5.4), cross-reference streams
//! (7.5.8), hybrid files (7.5.8.4), `/Prev` chains and incremental updates
//! (7.5.6).
//!
//! The merged table is built newest-revision-first with first-writer-wins, so
//! a newer revision shadows an older one without either being copied. `/Size`
//! is a claim, not a fact: the table is dense to a documented cap and spills
//! to a map above it, so a hostile `/Size` cannot allocate gigabytes.
//!
//! Nothing here trusts an offset. Every offset this module records is
//! validated against its `N G obj` header before a read uses it, which is the
//! check that makes level 1 of the leniency ladder safe.

use core::ops::Range;
use std::collections::{BTreeMap, BTreeSet};

use tinker_pdf_filters as filters;

use crate::doc::DocNames;
use crate::lexer::{Keyword, Lexer, TokenKind};
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::parse::{parse_indirect_at, parse_object_at};
use crate::repair::{find_from, rfind_from};
use crate::source::{Bytes, Window};
use crate::streams::{build_chain, resolve_extent, slice_range};
use crate::warn::{WarningKind, WarningSink};

/// One cross-reference entry (7.5.4 Table 18, 7.5.8.3 Table 18).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XrefEntry {
    /// A free object: `next` is the next free object number, `gen` the
    /// generation a reuse of this slot would take. Reads as null.
    Free {
        /// Object number of the next free entry.
        next: u32,
        /// Generation to be used if this slot is reused.
        gen: u16,
    },
    /// A classic type-1 entry: the object's header starts at `offset`.
    Offset {
        /// Byte offset of the `N G obj` header.
        offset: u64,
        /// The generation the table claims.
        gen: u16,
    },
    /// A type-2 entry: the object lives at index `idx` of an object stream.
    InStream {
        /// Object number of the containing `/Type /ObjStm` stream.
        stream_num: u32,
        /// Index of this object within that stream.
        idx: u32,
    },
}

/// The merged cross-reference table.
///
/// Dense up to [`limits::MAX_XREF_SLOTS`], a `BTreeMap` above it. Iteration is
/// in ascending object-number order across both halves.
#[derive(Clone, Debug, Default)]
pub struct XrefTable {
    dense: Vec<Option<XrefEntry>>,
    spill: BTreeMap<u32, XrefEntry>,
    count: usize,
}

impl XrefTable {
    /// An empty table.
    pub fn new() -> XrefTable {
        XrefTable::default()
    }

    /// The entry for `num`, if any revision defined one.
    pub fn get(&self, num: u32) -> Option<XrefEntry> {
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => self.dense.get(i).copied().flatten(),
            _ => self.spill.get(&num).copied(),
        }
    }

    /// How many object numbers have an entry.
    pub fn len(&self) -> usize {
        self.count
    }

    /// The highest object number with an entry, or zero.
    ///
    /// An editor allocates above this, so a new object cannot collide with
    /// one the document already uses — including one only an older revision
    /// still mentions.
    pub fn max_number(&self) -> u32 {
        self.iter().map(|(num, _)| num).max().unwrap_or(0)
    }

    /// Whether no object number has an entry.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Every entry, in ascending object-number order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        let dense = self
            .dense
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.map(|e| (i as u32, e)));
        let spill = self.spill.iter().map(|(n, e)| (*n, *e));
        dense.chain(spill)
    }

    /// Records `entry` for `num` unless a newer revision already claimed it.
    ///
    /// 7.5.6: sections are walked newest first, so the first writer is the
    /// newest and later writers are older revisions being shadowed.
    pub(crate) fn insert_new(&mut self, num: u32, entry: XrefEntry) -> bool {
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => {
                if self.dense.len() <= i {
                    self.dense.resize(i + 1, None);
                }
                match self.dense.get_mut(i) {
                    Some(slot) if slot.is_none() => {
                        *slot = Some(entry);
                        self.count += 1;
                        true
                    }
                    _ => false,
                }
            }
            _ => {
                if self.spill.contains_key(&num) {
                    return false;
                }
                self.spill.insert(num, entry);
                self.count += 1;
                true
            }
        }
    }

    /// Overwrites the entry for `num`, for a level-2 repair replacing an
    /// offset the file lied about.
    pub(crate) fn replace(&mut self, num: u32, entry: XrefEntry) {
        match usize::try_from(num) {
            Ok(i) if i < limits::MAX_XREF_SLOTS => {
                if self.dense.len() <= i {
                    self.dense.resize(i + 1, None);
                }
                if let Some(slot) = self.dense.get_mut(i) {
                    if slot.is_none() {
                        self.count += 1;
                    }
                    *slot = Some(entry);
                }
            }
            _ => {
                if self.spill.insert(num, entry).is_none() {
                    self.count += 1;
                }
            }
        }
    }
}

/// One incremental-update revision (7.5.6).
#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    /// The document as of this revision: bytes `0..end`, where `end` is just
    /// past this revision's `%%EOF`. That is exactly the byte range a
    /// signature covers and exactly what "save the original revision" writes.
    pub byte_range: Range<u64>,
    /// Where this revision's cross-reference section starts (7.5.4, 7.5.8),
    /// after any header shift has been corrected for — the offset its own
    /// `startxref` names, and the one the *next* revision's `/Prev` must name.
    ///
    /// Distinct from `byte_range.end` on purpose: they differ by the size of
    /// the section itself plus the trailer, and an update that chains to the
    /// end of the file instead of to the table leaves every object of every
    /// earlier revision unreachable. Zero when the revision was synthesized by
    /// the repair scanner, which means there is no section to chain to.
    pub xref_at: u64,
    /// The trailer dictionary this revision published.
    pub trailer: Dict,
}

/// What one walk of the cross-reference chain produced.
#[derive(Debug, Default)]
pub(crate) struct XrefBuild {
    pub table: XrefTable,
    /// Newest first.
    pub revisions: Vec<Revision>,
    /// Every revision's trailer merged newest-first, so a key dropped by a
    /// later incremental update is still found in the one that set it.
    pub trailer: Dict,
    pub sections: usize,
}

/// The `N G obj` header at `offset`, if there is one.
///
/// Deliberately cheap: three tokens, no object body, because this runs once
/// per cross-reference entry when the document opens.
pub(crate) fn object_header_at(buf: &[u8], offset: u64) -> Option<ObjRef> {
    let mut scratch = WarningSink::new();
    let mut lexer = Lexer::at(buf, offset);
    let TokenKind::Int(num) = lexer.next_token(&mut scratch).kind else {
        return None;
    };
    let TokenKind::Int(gen) = lexer.next_token(&mut scratch).kind else {
        return None;
    };
    if lexer.next_token(&mut scratch).kind != TokenKind::Keyword(Keyword::Obj) {
        return None;
    }
    Some(ObjRef::new(
        u32::try_from(num).ok()?,
        u16::try_from(gen).ok()?,
    ))
}

/// The offset of `%PDF-`, and hence the uniform shift every stored offset
/// needs (7.5.2).
///
/// Bytes before the header are the most common corruption in the wild: the
/// file was concatenated onto something, so every offset in it is short by
/// exactly the header's position.
pub(crate) fn header_shift(buf: &[u8], sink: &mut WarningSink) -> u64 {
    let window = buf.len().min(limits::MAX_HEADER_SCAN);
    match buf.get(..window).and_then(|w| find_from(w, b"%PDF-", 0)) {
        Some(0) => 0,
        Some(at) => {
            sink.warn(at as u64, WarningKind::HeaderNotAtStart);
            at as u64
        }
        None => {
            sink.warn(0, WarningKind::HeaderMissing);
            0
        }
    }
}

/// The offset `startxref` names (7.5.5).
///
/// The last kilobyte first, then the last 64 KB: trailing junk after `%%EOF`
/// is routine, and a `startxref` further back than that is not worth trusting
/// over a full rescan. On a streamed source those are two tail windows and
/// nothing else -- the ladder is the same ladder, read from the end.
pub(crate) fn startxref(bytes: &Bytes<'_>, sink: &mut WarningSink) -> Option<u64> {
    let len = bytes.len();
    for scan in [limits::STARTXREF_SCAN, limits::STARTXREF_SCAN_MAX] {
        let tail_at = len.saturating_sub(scan as u64);
        let Some(window) = bytes.window(tail_at, len.saturating_sub(tail_at)) else {
            continue;
        };
        let from = usize::try_from(window.local(tail_at)?)
            .unwrap_or(usize::MAX)
            .min(window.bytes().len());
        let Some(at) = rfind_from(window.bytes(), b"startxref", from) else {
            continue;
        };
        let mut scratch = WarningSink::new();
        let mut lexer = Lexer::at(window.bytes(), (at + b"startxref".len()) as u64);
        if let TokenKind::Int(value) = lexer.next_token(&mut scratch).kind {
            if let Ok(offset) = u64::try_from(value) {
                return Some(offset);
            }
        }
        sink.warn(window.abs(at as u64), WarningKind::StartxrefUnusable);
        return None;
    }
    sink.warn(len, WarningKind::StartxrefMissing);
    None
}

/// Walks the cross-reference chain from `start`, newest section first.
pub(crate) fn build(
    bytes: Bytes<'_>,
    start: u64,
    shift: u64,
    names: &DocNames,
    sink: &mut WarningSink,
) -> XrefBuild {
    build_limited(bytes, start, shift, names, sink, limits::MAX_XREF_CHAIN)
}

/// [`build`], stopping after `max_sections` of the chain.
///
/// Annex F's fast path walks exactly one: a linearized file's first-page
/// cross-reference section carries a `/Prev` to the main table at the end of
/// the file, and following it is the one thing that would put a tail read in a
/// head-only open.
pub(crate) fn build_limited(
    bytes: Bytes<'_>,
    start: u64,
    shift: u64,
    names: &DocNames,
    sink: &mut WarningSink,
    max_sections: u32,
) -> XrefBuild {
    build_with(bytes, start, shift, names, sink, max_sections, false)
}

/// [`build_limited`], told that this section's revision is the whole document.
///
/// Annex F's first-page cross-reference section is at the front of the file
/// and the only `%%EOF` is at the very back (part 11), so searching forward
/// for it would read everything -- to learn what the layout already says. The
/// head-only open passes `true` and the walker takes the document's end.
pub(crate) fn build_head(
    bytes: Bytes<'_>,
    start: u64,
    names: &DocNames,
    sink: &mut WarningSink,
) -> XrefBuild {
    build_with(bytes, start, 0, names, sink, 1, true)
}

fn build_with(
    bytes: Bytes<'_>,
    start: u64,
    shift: u64,
    names: &DocNames,
    sink: &mut WarningSink,
    max_sections: u32,
    revision_is_the_document: bool,
) -> XrefBuild {
    let len = bytes.len();
    let mut walker = Walker {
        bytes,
        len,
        shift,
        names,
        revision_is_the_document,
        first_window: if revision_is_the_document {
            limits::HEAD_SECTION_WINDOW
        } else {
            limits::XREF_SECTION_WINDOW
        },
        table: XrefTable::new(),
        revisions: Vec::new(),
    };
    let sections = walker.walk(start, max_sections, sink);
    let mut trailer = Dict::new();
    for revision in &walker.revisions {
        for (key, value) in revision.trailer.iter() {
            if !trailer.contains_key(*key) {
                trailer.insert(*key, value.clone());
            }
        }
    }
    XrefBuild {
        table: walker.table,
        revisions: walker.revisions,
        trailer,
        sections,
    }
}

struct Walker<'a> {
    bytes: Bytes<'a>,
    len: u64,
    shift: u64,
    names: &'a DocNames,
    /// Annex F: this section's revision is the whole file, so there is nothing
    /// to search forward for.
    revision_is_the_document: bool,
    /// How much to fetch for a section before the window doubles. The head
    /// path asks for less: a linearized file's first-page table is small, its
    /// `/E` can be under a single chunk, and an over-eager first window would
    /// read the tail of a small file to parse the front of it.
    first_window: u64,
    table: XrefTable,
    revisions: Vec<Revision>,
}

/// One subsection header: the first object number, and how many entries
/// follow it (7.5.4).
#[derive(Clone, Copy)]
struct Subsection {
    first: u64,
    count: u64,
}

struct Section {
    trailer: Dict,
    /// Just past this revision's `%%EOF`, or end of buffer.
    end: u64,
}

/// What one attempt at reading a section out of one window found.
enum Attempt {
    /// The window ran out before the section did. Grow and try again; on a
    /// whole buffer this cannot happen, because the window is the document.
    Short,
    /// A table, with the entries it named and the trailer it published.
    ///
    /// Entries are carried rather than inserted as they are read: an attempt
    /// that turns out to be short is abandoned whole, so a torn window can
    /// never leave half a subsection in the merged table.
    Read {
        entries: Vec<(u32, XrefEntry)>,
        section: Section,
    },
}

impl Walker<'_> {
    fn candidates(&self, raw: u64) -> Vec<u64> {
        offset_candidates(self.len, raw, self.shift)
    }

    fn walk(&mut self, start: u64, max_sections: u32, sink: &mut WarningSink) -> usize {
        let mut visited: BTreeSet<u64> = BTreeSet::new();
        let mut next = Some(start);
        let mut sections = 0usize;
        let mut depth = 0u32;

        while let Some(raw) = next.take() {
            if depth >= max_sections.min(limits::MAX_XREF_CHAIN) {
                if max_sections >= limits::MAX_XREF_CHAIN {
                    sink.warn(raw, WarningKind::XrefChainCapHit);
                }
                break;
            }
            depth += 1;
            if !visited.insert(raw) {
                sink.warn(raw, WarningKind::XrefPrevCycle);
                break;
            }
            let Some((found_at, section)) = self
                .candidates(raw)
                .into_iter()
                .find_map(|at| self.section(at, sink).map(|section| (at, section)))
            else {
                sink.warn(
                    raw,
                    if sections == 0 {
                        WarningKind::StartxrefUnusable
                    } else {
                        WarningKind::XrefUnreadable
                    },
                );
                break;
            };
            sections += 1;

            // 7.5.8.4: within one revision the classic entries are consulted
            // first, then /XRefStm, and only then /Prev.
            if let Some(xref_stm) = nonnegative(section.trailer.get_int(Name::XREF_STM)) {
                let hit = self
                    .candidates(xref_stm)
                    .into_iter()
                    .find_map(|at| self.xref_stream_section(at, sink));
                if hit.is_none() {
                    sink.warn(xref_stm, WarningKind::HybridXrefStmUnreadable);
                }
            }

            match section.trailer.get(Name::PREV) {
                Some(Object::Int(prev)) => match u64::try_from(*prev) {
                    Ok(prev) => next = Some(prev),
                    Err(_) => sink.warn(raw, WarningKind::XrefPrevBad),
                },
                Some(_) => sink.warn(raw, WarningKind::XrefPrevBad),
                None => {}
            }

            self.revisions.push(Revision {
                byte_range: 0..section.end,
                xref_at: found_at,
                trailer: section.trailer,
            });
        }
        sections
    }

    fn section(&mut self, at: u64, sink: &mut WarningSink) -> Option<Section> {
        self.classic_section(at, sink)
            .or_else(|| self.xref_stream_section(at, sink))
    }

    /// A classic table (7.5.4).
    ///
    /// The window doubles until the trailer parses, which is what a structure
    /// whose size is not known until it is read costs on a streamed source.
    /// Re-reading from the section start is free in *fetched* bytes: the chunk
    /// cache already holds every chunk the shorter attempt pulled, so the
    /// doubling costs arithmetic and no transport.
    fn classic_section(&mut self, at: u64, sink: &mut WarningSink) -> Option<Section> {
        let mut want = self.first_window;
        loop {
            let window = self.bytes.window(at, want)?;
            let reaches_end = window.end() >= self.len;
            let mut scratch = WarningSink::new();
            match self.try_classic(&window, at, reaches_end, &mut scratch)? {
                Attempt::Read { entries, section } => {
                    for (num, entry) in entries {
                        self.table.insert_new(num, entry);
                    }
                    sink.extend(scratch.take());
                    return Some(section);
                }
                // The attempt is abandoned whole, diagnostics and all: they
                // describe a window rather than a document.
                Attempt::Short => want = want.saturating_mul(2),
            }
        }
    }

    /// One attempt at a classic table inside `window`.
    ///
    /// Entries are read as tokens rather than at a fixed stride, so the 19-
    /// and 21-byte entries that wrong end-of-line discipline produces
    /// resynchronize on the grammar instead of shearing the table.
    fn try_classic(
        &self,
        window: &Window<'_>,
        at: u64,
        reaches_end: bool,
        sink: &mut WarningSink,
    ) -> Option<Attempt> {
        let mut scratch = WarningSink::new();
        let start = window.local(at)?;
        let mut lexer = Lexer::at(window.bytes(), start);
        if lexer.next_token(&mut scratch).kind != TokenKind::Keyword(Keyword::Xref) {
            return None;
        }

        let mut entries = Vec::new();
        let mut trailer = Dict::new();
        let mut end = self.len;
        loop {
            let save = lexer.offset();
            let token = lexer.next_token(&mut scratch);
            match token.kind {
                TokenKind::Keyword(Keyword::Trailer) => {
                    let parsed =
                        parse_object_at(window.bytes(), token.end, &self.names.table, sink);
                    if let Object::Dict(dict) = parsed.object {
                        trailer = dict;
                    }
                    end = self.revision_end_from(window.abs(parsed.end_offset));
                    break;
                }
                TokenKind::Eof => {
                    // On a streamed source this is far more often the window
                    // ending than the file ending, and the two are different
                    // facts: one is fetched, the other is warned about.
                    if !reaches_end {
                        return Some(Attempt::Short);
                    }
                    sink.warn(window.abs(save), WarningKind::XrefTrailerMissing);
                    break;
                }
                TokenKind::Int(first) => {
                    let TokenKind::Int(count) = lexer.next_token(&mut scratch).kind else {
                        sink.warn(window.abs(save), WarningKind::XrefSubsectionMalformed);
                        break;
                    };
                    let (Ok(first), Ok(count)) = (u64::try_from(first), u64::try_from(count))
                    else {
                        sink.warn(window.abs(save), WarningKind::XrefSubsectionMalformed);
                        break;
                    };
                    match self.subsection(
                        window,
                        &mut lexer,
                        Subsection { first, count },
                        reaches_end,
                        &mut entries,
                        sink,
                    ) {
                        Some(true) => {}
                        Some(false) => break,
                        None => return Some(Attempt::Short),
                    }
                }
                _ => {
                    sink.warn(window.abs(save), WarningKind::XrefSubsectionMalformed);
                    break;
                }
            }
        }
        Some(Attempt::Read {
            entries,
            section: Section { trailer, end },
        })
    }

    /// Reads `count` entries into `entries`.
    ///
    /// `Some(false)` when the table sheared badly enough that the remaining
    /// subsections cannot be trusted either, and `None` when the *window* ran
    /// out rather than the table -- two different facts, and conflating them
    /// is how a streamed read would silently drop the entries it had not
    /// fetched yet.
    fn subsection(
        &self,
        window: &Window<'_>,
        lexer: &mut Lexer<'_>,
        run: Subsection,
        reaches_end: bool,
        entries: &mut Vec<(u32, XrefEntry)>,
        sink: &mut WarningSink,
    ) -> Option<bool> {
        let Subsection { first, count } = run;
        let mut scratch = WarningSink::new();
        for i in 0..count {
            let save = lexer.offset();
            let field1 = lexer.next_token(&mut scratch);
            let TokenKind::Int(field1) = field1.kind else {
                if field1.kind == TokenKind::Eof && !reaches_end {
                    return None;
                }
                lexer.seek(save);
                sink.warn(window.abs(save), WarningKind::XrefEntryMalformed);
                return Some(false);
            };
            let field2 = lexer.next_token(&mut scratch);
            let TokenKind::Int(field2) = field2.kind else {
                if field2.kind == TokenKind::Eof && !reaches_end {
                    return None;
                }
                lexer.seek(save);
                sink.warn(window.abs(save), WarningKind::XrefEntryMalformed);
                return Some(false);
            };
            let marker = lexer.next_token(&mut scratch);
            if marker.kind == TokenKind::Eof && !reaches_end {
                return None;
            }
            let kind = window
                .bytes()
                .get(usize::try_from(marker.start).unwrap_or(usize::MAX));
            let entry = match kind {
                Some(b'n') if marker.end == marker.start + 1 => XrefEntry::Offset {
                    offset: u64::try_from(field1).unwrap_or(0),
                    gen: u16::try_from(field2).unwrap_or(0),
                },
                Some(b'f') if marker.end == marker.start + 1 => XrefEntry::Free {
                    next: u32::try_from(field1).unwrap_or(0),
                    gen: u16::try_from(field2).unwrap_or(0),
                },
                _ => {
                    // No type marker: this is not an entry. Give the token
                    // back so a `trailer` keyword is still seen.
                    lexer.seek(marker.start);
                    sink.warn(window.abs(marker.start), WarningKind::XrefEntryMalformed);
                    return Some(true);
                }
            };
            if let Ok(num) = u32::try_from(first.saturating_add(i)) {
                entries.push((num, entry));
            }
        }
        Some(true)
    }

    /// A cross-reference stream (7.5.8). Never decrypted (7.6.2).
    ///
    /// Two windows rather than one, which is the shape 7.5.8 asks for: the
    /// dictionary is parsed out of a window at the section, and then its own
    /// `/Length` says how much to fetch for the data. A reader that guessed
    /// one window for both would either fetch far too much or fail on a table
    /// bigger than its guess.
    fn xref_stream_section(&mut self, at: u64, sink: &mut WarningSink) -> Option<Section> {
        let mut want = self.first_window;
        let (window, parsed) = loop {
            let window = self.bytes.window(at, want)?;
            let reaches_end = window.end() >= self.len;
            let local = window.local(at)?;
            let mut scratch = WarningSink::new();
            match parse_indirect_at(window.bytes(), local, &self.names.table, &mut scratch) {
                Some(parsed) => break (window, parsed),
                None if reaches_end => return None,
                None => want = want.saturating_mul(2),
            }
        };
        let mut scratch = WarningSink::new();
        let Object::Stream(stream) = parsed.object else {
            return None;
        };
        if !stream.dict.contains_key(Name::W) {
            return None;
        }
        sink.extend(scratch.take());

        // 7.3.8.2: the extent is the declared length when `endstream` follows
        // it, so the window has to hold the keyword as well as the data.
        let slack = limits::MAX_STREAM_EOL_SKIP as u64 + b"endstream".len() as u64;
        let data_start = window.abs(stream.data_start);
        let want_data = match stream.len_hint {
            Some(declared) => declared.saturating_add(slack),
            None => self.first_window,
        };
        let data = self.bytes.window(data_start, want_data)?;
        let local_start = data.local(data_start)?;
        let local_range = resolve_extent(
            data.bytes(),
            local_start,
            stream.len_hint,
            Some(parsed.reference),
            sink,
        );
        let raw = slice_range(data.bytes(), &local_range);
        // An indirect value inside a cross-reference stream's own dictionary
        // cannot be resolved: the table that would find it is this one.
        let mut resolve = |o: &Object| match o {
            Object::Ref(_) => Object::Null,
            other => other.clone(),
        };
        let chain = build_chain(&stream.dict, self.names, &mut resolve, at, sink);
        let decoded = match filters::apply_chain(
            raw,
            &chain,
            &filters::Limits::new(limits::MAX_DECODED_STREAM),
        ) {
            Ok(filters::ChainOutput::Bytes(decoded)) => {
                for w in decoded.warnings {
                    sink.warn_at(at, Some(parsed.reference), WarningKind::Filter(w));
                }
                decoded.data
            }
            Ok(filters::ChainOutput::EncodedImage { .. }) | Err(_) => {
                sink.warn_at(at, Some(parsed.reference), WarningKind::FilterParamsBad);
                return None;
            }
        };

        // The data window is released before the table is applied: it borrows
        // the walker, and applying entries needs it back.
        let data_end = data.abs(local_range.end);
        drop(data);
        self.apply_xref_stream(&stream.dict, &decoded, at, sink);
        Some(Section {
            trailer: stream.dict,
            end: self.revision_end_from(data_end),
        })
    }

    fn apply_xref_stream(&mut self, dict: &Dict, data: &[u8], at: u64, sink: &mut WarningSink) {
        let Some(widths) = widths(dict) else {
            sink.warn(at, WarningKind::XrefStreamWidthsBad);
            return;
        };
        let entry_size: usize = widths.iter().sum();
        if entry_size == 0 || entry_size > data.len() {
            sink.warn(at, WarningKind::XrefStreamWidthsBad);
            return;
        }
        // 7.5.8.2: a /W first element of 0 means every entry is type 1.
        let type_width = widths.first().copied().unwrap_or(0);
        let f2_width = widths.get(1).copied().unwrap_or(0);
        let f3_width = widths.get(2).copied().unwrap_or(0);

        let rows = (data.len() / entry_size) as u64;
        let index = index_ranges(dict, rows, at, sink);
        let mut unknown_type = false;
        let mut truncated = false;
        let mut pos = 0usize;

        'outer: for (first, count) in index {
            for i in 0..count {
                let Some(row) = data.get(pos..pos + entry_size) else {
                    truncated = true;
                    break 'outer;
                };
                pos += entry_size;
                let kind = if type_width == 0 {
                    1
                } else {
                    be(row, 0, type_width)
                };
                let f2 = be(row, type_width, f2_width);
                let f3 = be(row, type_width + f2_width, f3_width);
                let entry = match kind {
                    0 => XrefEntry::Free {
                        next: u32::try_from(f2).unwrap_or(0),
                        gen: u16::try_from(f3).unwrap_or(0),
                    },
                    1 => XrefEntry::Offset {
                        offset: f2,
                        gen: u16::try_from(f3).unwrap_or(0),
                    },
                    2 => XrefEntry::InStream {
                        stream_num: u32::try_from(f2).unwrap_or(0),
                        idx: u32::try_from(f3).unwrap_or(0),
                    },
                    // 7.5.8.3: any other type is a reference to the null
                    // object. It still occupies the slot, so an older
                    // revision's entry stays shadowed.
                    _ => {
                        unknown_type = true;
                        XrefEntry::Free { next: 0, gen: 0 }
                    }
                };
                if let Ok(num) = u32::try_from(first.saturating_add(i)) {
                    self.table.insert_new(num, entry);
                }
            }
        }
        if unknown_type {
            sink.warn(at, WarningKind::XrefStreamUnknownType);
        }
        if truncated {
            sink.warn(at, WarningKind::XrefStreamTruncated);
        }
    }

    /// Where this revision ends: just past the `%%EOF` that follows `from`.
    ///
    /// On a streamed source the search window doubles until the marker is
    /// found or the document runs out, which costs no transport it has not
    /// already paid for -- the chunk cache holds what the shorter search read.
    fn revision_end_from(&self, from: u64) -> u64 {
        if self.revision_is_the_document {
            return self.len;
        }
        let mut want = limits::XREF_SECTION_WINDOW;
        loop {
            let Some(window) = self.bytes.window(from, want) else {
                return self.len;
            };
            // The cap is what keeps a linearized file's first-page section
            // from reading the whole document to find the one `%%EOF` at the
            // end of it. A whole buffer never reaches it: its window is the
            // document, so the first pass either finds the marker or there is
            // none to find.
            let reaches_end = window.end() >= self.len || want >= limits::REVISION_END_SCAN;
            if let Some(found) = self.revision_end(&window, from, reaches_end) {
                return found;
            }
            want = want.saturating_mul(2);
        }
    }

    /// The `%%EOF` after `from` inside `window`, or `None` when the window
    /// ended first and there are more bytes to fetch.
    fn revision_end(&self, window: &Window<'_>, from: u64, reaches_end: bool) -> Option<u64> {
        let local = usize::try_from(window.local(from)?)
            .unwrap_or(usize::MAX)
            .min(window.bytes().len());
        match find_from(window.bytes(), b"%%EOF", local) {
            Some(at) => Some(window.abs((at + b"%%EOF".len()) as u64)),
            None if reaches_end => Some(self.len),
            None => None,
        }
    }
}

/// `/W` as byte widths, or `None` when it cannot describe an entry.
fn widths(dict: &Dict) -> Option<Vec<usize>> {
    let array = dict.get_array(Name::W)?;
    if array.len() < 2 {
        return None;
    }
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let value = item.as_int()?;
        // Wider than a u64 cannot be read, and no real file exceeds 8.
        let width = usize::try_from(value).ok().filter(|w| *w <= 8)?;
        out.push(width);
    }
    Some(out)
}

/// `/Index` as (first, count) pairs, defaulting to `[0 /Size]` (7.5.8.2).
fn index_ranges(dict: &Dict, rows: u64, at: u64, sink: &mut WarningSink) -> Vec<(u64, u64)> {
    let default = || {
        let size = nonnegative(dict.get_int(Name::SIZE)).unwrap_or(rows);
        vec![(0, size.min(rows))]
    };
    let Some(array) = dict.get_array(Name::INDEX) else {
        return default();
    };
    if array.is_empty() || array.len() % 2 != 0 {
        sink.warn(at, WarningKind::XrefStreamIndexBad);
        return default();
    }
    let mut out = Vec::with_capacity(array.len() / 2);
    for pair in array.chunks_exact(2) {
        let (Some(first), Some(count)) = (
            pair.first().and_then(Object::as_int),
            pair.get(1).and_then(Object::as_int),
        ) else {
            sink.warn(at, WarningKind::XrefStreamIndexBad);
            return default();
        };
        let (Ok(first), Ok(count)) = (u64::try_from(first), u64::try_from(count)) else {
            sink.warn(at, WarningKind::XrefStreamIndexBad);
            return default();
        };
        out.push((first, count));
    }
    out
}

/// A big-endian field of `width` bytes at `offset`. A zero width is zero,
/// which is what 7.5.8.2's defaults amount to.
fn be(row: &[u8], offset: usize, width: usize) -> u64 {
    let mut value: u64 = 0;
    for i in 0..width {
        let byte = row.get(offset + i).copied().unwrap_or(0);
        value = (value << 8) | u64::from(byte);
    }
    value
}

fn nonnegative(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| u64::try_from(v).ok())
}

/// Where an offset stored in the file might really be.
///
/// The header shift comes first because a leading-junk file has every offset
/// short by exactly that much (7.5.2); the unshifted value is tried second
/// because a producer that prepended junk *and* corrected its offsets exists
/// too, and one extra probe is far cheaper than a rescan.
pub(crate) fn offset_candidates(buf_len: u64, raw: u64, shift: u64) -> Vec<u64> {
    let mut out = Vec::with_capacity(2);
    if shift > 0 {
        let shifted = raw.saturating_add(shift);
        if shifted < buf_len {
            out.push(shifted);
        }
    }
    if raw < buf_len && !out.contains(&raw) {
        out.push(raw);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::DocNames;

    fn build_at(buf: &[u8], start: u64) -> (XrefBuild, Vec<WarningKind>) {
        let names = DocNames::new();
        let mut sink = WarningSink::new();
        let built = build(Bytes::Whole(buf), start, 0, &names, &mut sink);
        let kinds = sink.warnings().iter().map(|w| w.kind).collect();
        (built, kinds)
    }

    #[test]
    fn the_table_is_dense_then_spills() {
        let mut table = XrefTable::new();
        assert!(table.is_empty());
        assert!(table.insert_new(2, XrefEntry::Offset { offset: 9, gen: 0 }));
        assert!(!table.insert_new(2, XrefEntry::Offset { offset: 5, gen: 0 }));
        assert!(table.insert_new(u32::MAX, XrefEntry::Free { next: 0, gen: 1 }));
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(2), Some(XrefEntry::Offset { offset: 9, gen: 0 }));
        assert_eq!(
            table.get(u32::MAX),
            Some(XrefEntry::Free { next: 0, gen: 1 })
        );
        let nums: Vec<u32> = table.iter().map(|(n, _)| n).collect();
        assert_eq!(nums, [2, u32::MAX]);
        table.replace(2, XrefEntry::Offset { offset: 1, gen: 0 });
        assert_eq!(table.get(2), Some(XrefEntry::Offset { offset: 1, gen: 0 }));
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_hostile_size_does_not_allocate() {
        // The dense half only ever grows to the numbers actually present.
        let mut table = XrefTable::new();
        table.insert_new(1_000_000_000, XrefEntry::Free { next: 0, gen: 0 });
        assert_eq!(table.len(), 1);
        assert!(table.get(5).is_none());
    }

    #[test]
    fn a_classic_table_parses() {
        let buf = b"xref\n0 3\n0000000000 65535 f \n0000000017 00000 n \n0000000081 00000 n \ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n";
        let (built, warnings) = build_at(buf, 0);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(built.sections, 1);
        assert_eq!(
            built.table.get(0),
            Some(XrefEntry::Free {
                next: 0,
                gen: 65535
            })
        );
        assert_eq!(
            built.table.get(1),
            Some(XrefEntry::Offset { offset: 17, gen: 0 })
        );
        assert_eq!(built.trailer.get_ref(Name::ROOT), Some(ObjRef::new(1, 0)));
        assert_eq!(built.revisions.len(), 1);
        assert_eq!(built.revisions[0].byte_range, 0..buf.len() as u64 - 1);
    }

    #[test]
    fn nineteen_and_twenty_one_byte_entries_resynchronize() {
        // No trailing space (19 bytes) and a doubled EOL (21 bytes).
        let buf = b"xref\n0 3\n0000000000 65535 f\n0000000017 00000 n\r\n\n0000000081 00000 n \ntrailer\n<< /Size 3 >>\n%%EOF";
        let (built, warnings) = build_at(buf, 0);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            built.table.get(1),
            Some(XrefEntry::Offset { offset: 17, gen: 0 })
        );
        assert_eq!(
            built.table.get(2),
            Some(XrefEntry::Offset { offset: 81, gen: 0 })
        );
    }

    #[test]
    fn multiple_subsections_parse() {
        let buf = b"xref\n0 1\n0000000000 65535 f \n4 2\n0000000017 00000 n \n0000000081 00000 n \ntrailer\n<< /Size 6 >>\n%%EOF";
        let (built, warnings) = build_at(buf, 0);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(built.table.get(1).is_none());
        assert_eq!(
            built.table.get(4),
            Some(XrefEntry::Offset { offset: 17, gen: 0 })
        );
        assert_eq!(
            built.table.get(5),
            Some(XrefEntry::Offset { offset: 81, gen: 0 })
        );
    }

    #[test]
    fn a_missing_trailer_keeps_the_entries() {
        let buf = b"xref\n0 2\n0000000000 65535 f \n0000000017 00000 n \n";
        let (built, warnings) = build_at(buf, 0);
        assert_eq!(warnings, [WarningKind::XrefTrailerMissing]);
        assert_eq!(built.table.len(), 2);
    }

    /// A buffer holding one cross-reference stream, positioned at offset 9.
    fn xref_stream_buf(dict_extra: &str, rows: &[u8]) -> Vec<u8> {
        let mut buf = format!(
            "%PDF-1.7\n1 0 obj\n<< /Type /XRef {dict_extra} /Length {} >>\nstream\n",
            rows.len()
        )
        .into_bytes();
        buf.extend_from_slice(rows);
        buf.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        buf
    }

    #[test]
    fn a_zero_width_type_field_defaults_to_type_one() {
        // 7.5.8.2: "If the first element is zero, the type field shall not be
        // present, and shall default to type 1."
        let rows = [0x00, 0x0a, 0x00, 0x00, 0x14, 0x00];
        let buf = xref_stream_buf("/Size 2 /W [0 2 1]", &rows);
        let (built, warnings) = build_at(&buf, 9);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            built.table.get(0),
            Some(XrefEntry::Offset { offset: 10, gen: 0 })
        );
        assert_eq!(
            built.table.get(1),
            Some(XrefEntry::Offset { offset: 20, gen: 0 })
        );
    }

    #[test]
    fn an_index_moves_the_first_object_number() {
        let rows = [0x01, 0x00, 0x0a, 0x00, 0x01, 0x00, 0x14, 0x07];
        let buf = xref_stream_buf("/Size 7 /W [1 2 1] /Index [5 2]", &rows);
        let (built, warnings) = build_at(&buf, 9);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(built.table.len(), 2);
        assert_eq!(
            built.table.get(5),
            Some(XrefEntry::Offset { offset: 10, gen: 0 })
        );
        assert_eq!(
            built.table.get(6),
            Some(XrefEntry::Offset { offset: 20, gen: 7 })
        );
    }

    #[test]
    fn an_odd_index_falls_back_to_the_default_range() {
        let rows = [0x01, 0x00, 0x0a, 0x00];
        let buf = xref_stream_buf("/Size 1 /W [1 2 1] /Index [5]", &rows);
        let (built, warnings) = build_at(&buf, 9);
        assert_eq!(warnings, [WarningKind::XrefStreamIndexBad]);
        assert_eq!(
            built.table.get(0),
            Some(XrefEntry::Offset { offset: 10, gen: 0 })
        );
    }

    #[test]
    fn a_stream_ending_mid_entry_keeps_what_it_read() {
        // Two entries claimed by /Index, one and a half present.
        let rows = [0x01, 0x00, 0x0a, 0x00, 0x01, 0x00];
        let buf = xref_stream_buf("/Size 2 /W [1 2 1] /Index [0 2]", &rows);
        let (built, warnings) = build_at(&buf, 9);
        assert_eq!(warnings, [WarningKind::XrefStreamTruncated]);
        assert_eq!(
            built.table.get(0),
            Some(XrefEntry::Offset { offset: 10, gen: 0 })
        );
        assert!(built.table.get(1).is_none());
    }

    #[test]
    fn a_missing_w_is_not_a_cross_reference_stream() {
        let buf = xref_stream_buf("/Size 2", &[0x01]);
        let (built, warnings) = build_at(&buf, 9);
        assert_eq!(warnings, [WarningKind::StartxrefUnusable]);
        assert!(built.table.is_empty());
    }

    #[test]
    fn widths_reject_impossible_entries() {
        let mut dict = Dict::new();
        dict.insert(Name::W, Object::Array(vec![Object::Int(1)]));
        assert!(widths(&dict).is_none());
        dict.insert(
            Name::W,
            Object::Array(vec![Object::Int(1), Object::Int(99)]),
        );
        assert!(widths(&dict).is_none());
        dict.insert(
            Name::W,
            Object::Array(vec![Object::Int(1), Object::Int(-1)]),
        );
        assert!(widths(&dict).is_none());
        dict.insert(
            Name::W,
            Object::Array(vec![Object::Int(0), Object::Int(2), Object::Int(1)]),
        );
        assert_eq!(widths(&dict), Some(vec![0, 2, 1]));
    }

    #[test]
    fn big_endian_fields_read_as_specified() {
        assert_eq!(be(&[0x01, 0x02, 0x03], 0, 2), 0x0102);
        assert_eq!(be(&[0x01, 0x02, 0x03], 1, 2), 0x0203);
        assert_eq!(be(&[0x01], 0, 0), 0);
        // Past the row reads as zero rather than panicking.
        assert_eq!(be(&[0x01], 0, 4), 0x01000000);
    }

    #[test]
    fn header_shift_finds_leading_junk() {
        let mut sink = WarningSink::new();
        assert_eq!(header_shift(b"%PDF-1.7\n", &mut sink), 0);
        assert!(sink.is_empty());
        assert_eq!(header_shift(b"JUNK%PDF-1.7\n", &mut sink), 4);
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::HeaderNotAtStart]
        );
        let mut sink = WarningSink::new();
        assert_eq!(header_shift(b"nothing here", &mut sink), 0);
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::HeaderMissing]
        );
    }

    #[test]
    fn startxref_reads_the_last_one() {
        let mut sink = WarningSink::new();
        assert_eq!(
            startxref(&Bytes::Whole(b"startxref\n12\n%%EOF\n"), &mut sink),
            Some(12)
        );
        assert_eq!(
            startxref(
                &Bytes::Whole(b"startxref\n12\n%%EOF\nstartxref\n40\n%%EOF\n"),
                &mut sink
            ),
            Some(40)
        );
        assert!(sink.is_empty());
        assert_eq!(
            startxref(&Bytes::Whole(b"no marker at all"), &mut sink),
            None
        );
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StartxrefMissing]
        );
    }

    #[test]
    fn startxref_tolerates_trailing_junk() {
        let mut buf = b"startxref\n7\n%%EOF\n".to_vec();
        buf.extend(std::iter::repeat_n(b'z', 2000));
        let mut sink = WarningSink::new();
        assert_eq!(startxref(&Bytes::Whole(&buf), &mut sink), Some(7));
    }

    #[test]
    fn a_negative_startxref_is_unusable() {
        let mut sink = WarningSink::new();
        assert_eq!(
            startxref(&Bytes::Whole(b"startxref\n-4\n%%EOF\n"), &mut sink),
            None
        );
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StartxrefUnusable]
        );
    }

    #[test]
    fn object_headers_are_matched_exactly() {
        assert_eq!(object_header_at(b"12 0 obj", 0), Some(ObjRef::new(12, 0)));
        assert_eq!(object_header_at(b"12 0 objx", 0), None);
        assert_eq!(object_header_at(b"12 obj", 0), None);
        assert_eq!(object_header_at(b"-1 0 obj", 0), None);
        assert_eq!(object_header_at(b"", 0), None);
    }
}
