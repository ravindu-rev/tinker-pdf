//! The page tree (7.7.3).
//!
//! One walk builds the index: page references in document order, each with its
//! inherited attributes already resolved. Doing it once means the geometry of
//! a thousand-page document costs one traversal, and a page's `/Resources`
//! never has to be chased up the tree again at render time.

use crate::doc::CosDocument;
use crate::limits;
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::store::MutexExt;
use crate::warn::WarningKind;
use crate::work::{Counter, Work};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A rectangle in PDF units, normalized so the corners are ordered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x0: f64,
    /// Bottom edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Top edge.
    pub y1: f64,
}

impl Rect {
    /// US Letter, the fallback when `/MediaBox` is absent or unusable.
    ///
    /// 7.7.3.3 makes `/MediaBox` required, so reaching this means the document
    /// is malformed; every surviving reader guesses rather than refuses.
    pub const US_LETTER: Rect = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: 612.0,
        y1: 792.0,
    };

    /// Builds a rectangle from a `/MediaBox`-style array, ordering the corners.
    #[must_use]
    pub fn from_array(values: &[Object]) -> Option<Rect> {
        let n = |i: usize| values.get(i).and_then(Object::as_number);
        let (a, b, c, d) = (n(0)?, n(1)?, n(2)?, n(3)?);
        if ![a, b, c, d].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Rect {
            x0: a.min(c),
            y0: b.min(d),
            x1: a.max(c),
            y1: b.max(d),
        })
    }

    /// Width in PDF units.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    /// Height in PDF units.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Whether the rectangle encloses any area.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    /// Whether every edge is a real number.
    ///
    /// A rectangle read from a file can hold an infinity or a NaN, and any of
    /// those written back into a content stream produces bytes no reader can
    /// lex — `NaN` is not a number to a PDF parser, it is a syntax error that
    /// throws away the rest of the stream.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.x0.is_finite() && self.y0.is_finite() && self.x1.is_finite() && self.y1.is_finite()
    }

    /// The overlap of two rectangles, or `None` when they do not meet.
    #[must_use]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let r = Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        (!r.is_empty()).then_some(r)
    }
}

/// One of the five page boundaries of 14.11.2, by the key a page dictionary
/// stores it under (7.7.3.3 Table 30).
///
/// The same five names are what a viewer preference's `/ViewArea`,
/// `/ViewClip`, `/PrintArea` and `/PrintClip` hold (12.2 Table 147), which is
/// why one type serves both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageBoundary {
    /// `/MediaBox`: the physical medium.
    MediaBox,
    /// `/CropBox`: what a viewer shows and prints.
    CropBox,
    /// `/BleedBox`: where content is clipped in a production environment.
    BleedBox,
    /// `/TrimBox`: the finished page after trimming.
    TrimBox,
    /// `/ArtBox`: the page's meaningful content, as its creator intended.
    ArtBox,
}

impl PageBoundary {
    /// Every boundary, outermost first.
    pub const ALL: [PageBoundary; 5] = [
        PageBoundary::MediaBox,
        PageBoundary::CropBox,
        PageBoundary::BleedBox,
        PageBoundary::TrimBox,
        PageBoundary::ArtBox,
    ];

    /// The key the boundary is stored under, and the name a viewer preference
    /// spells it with.
    #[must_use]
    pub fn key(self) -> &'static [u8] {
        match self {
            PageBoundary::MediaBox => b"MediaBox",
            PageBoundary::CropBox => b"CropBox",
            PageBoundary::BleedBox => b"BleedBox",
            PageBoundary::TrimBox => b"TrimBox",
            PageBoundary::ArtBox => b"ArtBox",
        }
    }

    /// The boundary a key names, or `None` for any other name.
    #[must_use]
    pub fn from_key(bytes: &[u8]) -> Option<PageBoundary> {
        PageBoundary::ALL
            .into_iter()
            .find(|boundary| boundary.key() == bytes)
    }
}

/// One page, with its inherited attributes resolved.
#[derive(Clone, Debug)]
pub struct Page {
    /// The page's own indirect reference.
    pub reference: ObjRef,
    /// Zero-based position in the document.
    pub index: u32,
    /// `/MediaBox`, inherited if the page does not carry one.
    pub media_box: Rect,
    /// `/CropBox` clipped to `/MediaBox`; equal to it when absent (7.7.3.3).
    pub crop_box: Rect,
    /// `/BleedBox` (14.11.2): the page's own, clipped to the media box, and
    /// the crop box when it has none.
    ///
    /// **Not inherited.** 7.7.3.3 Table 30 marks four page attributes
    /// inheritable — `/Resources`, `/MediaBox`, `/CropBox`, `/Rotate` — and
    /// the bleed, trim and art boxes are not among them, so a value on a
    /// `/Pages` node describes no page. Table 30 defaults each to the crop
    /// box, and 14.11.2.1 reduces each to its intersection with the media
    /// box; one that misses the media box altogether reads as the crop box,
    /// the way a crop box that misses it reads as the media box.
    pub bleed_box: Rect,
    /// `/TrimBox` (14.11.2), under the same rules as [`Page::bleed_box`].
    pub trim_box: Rect,
    /// `/ArtBox` (14.11.2), under the same rules as [`Page::bleed_box`].
    pub art_box: Rect,
    /// `/Rotate` normalized to 0, 90, 180 or 270.
    pub rotation: u16,
    /// `/Resources`, inherited and already resolved.
    ///
    /// The dictionary itself rather than a reference to it, because 7.7.3.3
    /// allows it to be written inline and plenty of producers do — including
    /// this crate's own builder. Holding a reference here silently lost every
    /// direct one, which cost a page all of its fonts.
    pub resources: Option<Dict>,
}

impl Page {
    /// One of the five boundaries, as this page resolves it.
    #[must_use]
    pub fn boundary(&self, boundary: PageBoundary) -> Rect {
        match boundary {
            PageBoundary::MediaBox => self.media_box,
            PageBoundary::CropBox => self.crop_box,
            PageBoundary::BleedBox => self.bleed_box,
            PageBoundary::TrimBox => self.trim_box,
            PageBoundary::ArtBox => self.art_box,
        }
    }

    /// The page's size after rotation, which is what a viewer lays out.
    #[must_use]
    pub fn display_size(&self) -> (f64, f64) {
        let (w, h) = (self.crop_box.width(), self.crop_box.height());
        if self.rotation == 90 || self.rotation == 270 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

/// Attributes that descend the tree (7.7.3.4), and three that do not.
#[derive(Clone, Debug, Default)]
struct Inherited {
    media_box: Option<Rect>,
    crop_box: Option<Rect>,
    rotation: Option<i64>,
    resources: Option<Dict>,
    /// `/BleedBox`, `/TrimBox` and `/ArtBox` **of the last node extended
    /// over, alone**: `extend` replaces them rather than falling back to the
    /// parent's, so what reaches a leaf is the leaf's own (Table 30 does not
    /// make them inheritable).
    own_boxes: [Option<Rect>; 3],
}

impl Inherited {
    /// Overlays whatever this node defines onto what it inherited.
    fn extend(self, dict: &Dict, doc: &CosDocument) -> Inherited {
        let rect = |key: Name| {
            let value = doc.resolve_key(dict, key);
            value.as_array().and_then(Rect::from_array)
        };
        // Resolved through the reference when there is one, taken as it
        // stands when the dictionary is written inline.
        let resources = doc
            .resolve_key(dict, Name::RESOURCES)
            .as_dict()
            .cloned()
            .or(self.resources);

        Inherited {
            media_box: rect(Name::MEDIA_BOX).or(self.media_box),
            crop_box: rect(doc.crop_box_name()).or(self.crop_box),
            rotation: dict.get_int(doc.rotate_name()).or(self.rotation),
            resources,
            own_boxes: [
                PageBoundary::BleedBox,
                PageBoundary::TrimBox,
                PageBoundary::ArtBox,
            ]
            .map(|boundary| rect(doc.intern(boundary.key()))),
        }
    }
}

/// Every page one walk found, kept on the document (P-01's index): the pages
/// in document order as [`collect`] returns them, and each page's position by
/// its reference.
#[derive(Debug)]
pub(crate) struct PageIndex {
    pages: Vec<Page>,
    positions: HashMap<ObjRef, u32>,
}

impl PageIndex {
    /// The index of `pages`, a walk's list in document order.
    fn new(pages: Vec<Page>) -> PageIndex {
        let mut positions = HashMap::with_capacity(pages.len());
        for page in &pages {
            positions.entry(page.reference).or_insert(page.index);
        }
        PageIndex { pages, positions }
    }

    /// The pages, in document order.
    pub(crate) fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The position of the page whose object is `reference`: its first, for a
    /// page a shared subtree holds twice, as a search of [`PageIndex::pages`]
    /// from the front finds.
    pub(crate) fn position(&self, reference: ObjRef) -> Option<u32> {
        self.positions.get(&reference).copied()
    }
}

/// The index's place on a document: the index once [`count`] has built it.
#[derive(Debug, Default)]
pub(crate) struct IndexSlot {
    index: Option<Arc<PageIndex>>,
}

/// The document's page index, when it has been built.
fn indexed(doc: &CosDocument) -> Option<Arc<PageIndex>> {
    doc.page_index().lock_safe().index.clone()
}

/// Builds the document's page index with one walk, counted, which raises the
/// walk's warnings this once — and `/Count`'s, where the root's disagrees with
/// what the walk found.
fn build(doc: &CosDocument, work: Option<&Work>) -> Arc<PageIndex> {
    let pages = collect_walked(doc, limits::MAX_PAGES, work);
    let claimed = doc
        .catalog()
        .and_then(|root| root.get_ref(Name::PAGES))
        .and_then(|tree| doc.get(tree).ok())
        .and_then(|node| node.as_dict().and_then(|d| d.get_int(Name::COUNT)));
    if claimed.is_some_and(|claimed| u64::try_from(claimed).ok() != Some(pages.len() as u64)) {
        doc.warn(WarningKind::PageCountMismatch);
    }
    let index = Arc::new(PageIndex::new(pages));
    doc.page_index().lock_safe().index = Some(Arc::clone(&index));
    index
}

/// The document's page index, built by this call when nothing has, as
/// [`count`] builds it: what an editor reads its pages out of.
pub(crate) fn index_counted(doc: &CosDocument, work: Option<&Work>) -> Arc<PageIndex> {
    match indexed(doc) {
        Some(index) => index,
        None => build(doc, work),
    }
}

/// The document's page index when a count has built it, and otherwise an
/// index of one walk made for this caller and not kept: what a reader that
/// needs every page's position reads, without building the index itself.
pub(crate) fn index_or_walked(doc: &CosDocument) -> Arc<PageIndex> {
    indexed(doc)
        .unwrap_or_else(|| Arc::new(PageIndex::new(collect_walked(doc, limits::MAX_PAGES, None))))
}

/// The reference of page `at` out of the document's page index, which this
/// builds when nothing has, counted as one entry copied; `None` past the last
/// page.
pub(crate) fn reference_counted(doc: &CosDocument, at: u32, work: Option<&Work>) -> Option<ObjRef> {
    let reference = index_counted(doc, work).pages.get(at as usize)?.reference;
    tally(work, Counter::PageEntriesCopied);
    Some(reference)
}

/// Every page's reference out of the document's page index, which this
/// builds when nothing has, each counted as an entry copied.
pub(crate) fn references_counted(doc: &CosDocument, work: Option<&Work>) -> Vec<ObjRef> {
    let index = index_counted(doc, work);
    tally_entries(work, index.pages.len());
    index.pages.iter().map(|page| page.reference).collect()
}

/// Page `index` out of the document's index, counted as one entry copied;
/// `None` past the last page.
fn copied(index: &PageIndex, at: u32, work: Option<&Work>) -> Option<Page> {
    let page = index.pages.get(at as usize).cloned()?;
    tally(work, Counter::PageEntriesCopied);
    Some(page)
}

/// Every page in document order.
///
/// A cyclic or absurdly deep `/Kids` graph is truncated with a warning rather
/// than followed: the visited set makes a cycle terminate, and the depth cap
/// makes a very deep legitimate tree terminate too.
#[must_use]
pub fn collect(doc: &CosDocument) -> Vec<Page> {
    collect_upto(doc, limits::MAX_PAGES)
}

/// [`collect`], counting into `work` when the operation is counting.
#[must_use]
pub fn collect_counted(doc: &CosDocument, work: Option<&Work>) -> Vec<Page> {
    collect_upto_counted(doc, limits::MAX_PAGES, work)
}

/// Adds one step of `counter` to `work`, when the operation is counting.
fn tally(work: Option<&Work>, counter: Counter) {
    if let Some(work) = work {
        work.add(counter, 1);
    }
}

/// The first `limit` pages in document order.
///
/// The same walk, stopped early: same order, same cycle guard, same inherited
/// attributes, and the same `index` on every page it yields. A caller that
/// wants page one does not need the rest of the tree -- and on a streamed
/// document opened through Annex F's head-only path, the rest of the tree is
/// the rest of the file, so walking it would spend the whole file to answer a
/// question about the first page.
///
/// Stopping early is not truncation and does not warn as if it were: a
/// document whose tree is cyclic or deeper than the cap still says so, and a
/// document that simply has more pages than the caller asked for does not.
#[must_use]
pub fn collect_upto(doc: &CosDocument, limit: usize) -> Vec<Page> {
    collect_upto_counted(doc, limit, None)
}

/// [`collect_upto`], counting into `work` when the operation is counting: one
/// walk of the page tree ([`Counter::PageWalks`]), and each node of it the
/// walk reads ([`Counter::PageNodesVisited`]) — a node it stops before, having
/// gathered `limit` pages, is not read and not counted.
///
/// Read out of the document's page index once it is built, each page counted
/// as an entry copied ([`Counter::PageEntriesCopied`]); a whole-tree read does
/// not build the index itself.
#[must_use]
pub fn collect_upto_counted(doc: &CosDocument, limit: usize, work: Option<&Work>) -> Vec<Page> {
    if let Some(index) = indexed(doc) {
        let pages: Vec<Page> = index.pages.iter().take(limit).cloned().collect();
        tally_entries(work, pages.len());
        return pages;
    }
    collect_walked(doc, limit, work)
}

/// Counts `n` entries copied out of the index.
fn tally_entries(work: Option<&Work>, n: usize) {
    if let Some(work) = work {
        work.add(Counter::PageEntriesCopied, n as u64);
    }
}

/// [`collect_upto_counted`]'s walk, which the index is built by.
fn collect_walked(doc: &CosDocument, limit: usize, work: Option<&Work>) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut visited = HashSet::new();
    if limit == 0 {
        return pages;
    }
    tally(work, Counter::PageWalks);

    let Some(root) = doc.catalog() else {
        return pages;
    };
    let Some(tree) = root.get_ref(Name::PAGES) else {
        return pages;
    };

    walk(
        doc,
        tree,
        Inherited::default(),
        0,
        limit,
        &mut visited,
        &mut pages,
        work,
    );
    pages
}

#[allow(clippy::too_many_arguments)]
fn walk(
    doc: &CosDocument,
    node: ObjRef,
    inherited: Inherited,
    depth: u32,
    limit: usize,
    visited: &mut HashSet<u32>,
    out: &mut Vec<Page>,
    work: Option<&Work>,
) {
    if out.len() >= limit {
        // The caller has what it asked for. Not truncation, so not a warning.
        return;
    }
    if depth > limits::MAX_NEST_DEPTH || out.len() >= limits::MAX_PAGES {
        doc.warn(WarningKind::PageTreeTruncated);
        return;
    }
    if !visited.insert(node.num) {
        // 7.7.3.2: the tree is a tree. A repeated node is a cycle or a
        // shared subtree, and following either duplicates pages forever.
        doc.warn(WarningKind::PageTreeCycle);
        return;
    }

    // Read: one node, whatever it turns out to hold.
    tally(work, Counter::PageNodesVisited);
    let Ok(object) = doc.get(node) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };

    let inherited = inherited.extend(dict, doc);

    match dict.get_array(Name::KIDS) {
        Some(kids) => {
            let kids: Vec<ObjRef> = kids.iter().filter_map(Object::as_objref).collect();
            for kid in kids {
                walk(
                    doc,
                    kid,
                    inherited.clone(),
                    depth + 1,
                    limit,
                    visited,
                    out,
                    work,
                );
            }
        }
        // 7.7.3.3: a node without /Kids is a leaf, whatever its /Type says —
        // and plenty of files say nothing at all.
        None => out.push(leaf(node, out.len() as u32, inherited, doc)),
    }

    visited.remove(&node.num);
}

fn leaf(reference: ObjRef, index: u32, inherited: Inherited, doc: &CosDocument) -> Page {
    let media_box = match inherited.media_box {
        Some(r) if !r.is_empty() => r,
        _ => {
            doc.warn(WarningKind::MediaBoxMissing);
            Rect::US_LETTER
        }
    };

    // 7.7.3.3: /CropBox defaults to /MediaBox and is clipped to it. A crop box
    // that misses the page entirely is meaningless, so fall back.
    let crop_box = inherited
        .crop_box
        .and_then(|c| c.intersect(&media_box))
        .unwrap_or(media_box);

    // 14.11.2.1 and Table 30: each defaults to the crop box and is reduced to
    // its intersection with the media box.
    let [bleed_box, trim_box, art_box] = inherited.own_boxes.map(|own| {
        own.and_then(|r| r.intersect(&media_box))
            .unwrap_or(crop_box)
    });

    Page {
        reference,
        index,
        media_box,
        crop_box,
        bleed_box,
        trim_box,
        art_box,
        rotation: normalize_rotation(inherited.rotation.unwrap_or(0)),
        resources: inherited.resources,
    }
}

/// 7.7.3.3: `/Rotate` is a multiple of 90, which files routinely violate.
///
/// Values are taken modulo 360 in the positive direction, so -90 is 270 and
/// 450 is 90; anything not a multiple of 90 rounds to the nearest one.
#[must_use]
pub fn normalize_rotation(degrees: i64) -> u16 {
    let wrapped = degrees.rem_euclid(360);
    let quarter = ((wrapped as f64) / 90.0).round() as i64 % 4;
    (quarter * 90) as u16
}

/// The number of pages: the length of the document's page index, which this
/// builds when nothing has yet.
///
/// `/Count` is a claim like any other; a document that lies about it gets
/// counted by the walk rather than believed, and says so once, when the
/// walk builds the index ([`WarningKind::PageCountMismatch`]). A count walks
/// the whole tree, so the index it builds costs nothing more, and every page
/// loop that starts by counting reads its pages out of it.
#[must_use]
pub fn count(doc: &CosDocument) -> u32 {
    count_counted(doc, None)
}

/// [`count`], counting into `work` when the operation is counting: the walk
/// that builds the index, or, once it is built, nothing.
#[must_use]
pub fn count_counted(doc: &CosDocument, work: Option<&Work>) -> u32 {
    index_counted(doc, work).pages.len() as u32
}

/// One page built from one object and the ancestors the head already holds,
/// or nothing.
///
/// The page is built by [`leaf`], the same function the tree walk uses, which
/// is only sound when the object is a leaf carrying its own `/MediaBox` and
/// `/Resources`. When it is not, this declines and the walk runs, because a
/// page laid out at US Letter because its ancestor was not fetched is the
/// wrong page rather than a cheaper one. Its `/Rotate` and `/CropBox` are
/// inherited as the walk inherits them (7.7.3.4), from every ancestor along
/// `/Parent` that is placed without the tail ([`ancestors_in_the_head`]).
fn page_alone(
    doc: &CosDocument,
    reference: ObjRef,
    index: u32,
    work: Option<&Work>,
) -> Option<Page> {
    tally(work, Counter::PageNodesVisited);
    let object = doc.get(reference).ok()?;
    // Not a dictionary at all is the common way for this to be the wrong
    // object: a run's leading object read a few bytes late is its content
    // stream, and a stream is not a page.
    let dict = object.as_dict()?;
    let inherited = ancestors_in_the_head(doc, dict, work)
        .iter()
        .rev()
        .filter_map(|ancestor| ancestor.as_dict())
        .fold(Inherited::default(), |inherited, ancestor| {
            inherited.extend(ancestor, doc)
        })
        .extend(dict, doc);
    // One test rather than two, because two were each other's shadow: a page
    // with no `/MediaBox` key resolves to no media box, so whichever ran first
    // declined and removing either alone changed nothing a test could see.
    //
    // 7.7.3.4 lets both attributes come from an ancestor, and an ancestor is
    // exactly what neither route here fetched. `leaf` would default a missing
    // media box to US Letter with a warning, which would be this function
    // guessing rather than declining.
    if inherited.media_box.is_none_or(|r| r.is_empty()) || inherited.resources.is_none() {
        return None;
    }
    Some(leaf(reference, index, inherited, doc))
}

/// The ancestors of the page `dict`, nearest first, along `/Parent`, as far
/// as each is placed without the tail
/// ([`CosDocument::placed_without_the_tail`]).
///
/// Annex F lays a file out so that page one needs only the head, and a
/// layout is free to leave the page tree in the tail: an ancestor the head
/// does not place stops the climb rather than costing the tail, so a page
/// built here inherits what a reader holding the head can know, and a file
/// whose ancestors are all in the head, as this engine's own linearizer
/// writes them, reads its page as the tree walk does. A cycle or a chain
/// deeper than the walk's own limit stops it too.
fn ancestors_in_the_head(
    doc: &CosDocument,
    dict: &Dict,
    work: Option<&Work>,
) -> Vec<std::sync::Arc<Object>> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut parent = dict.get_ref(Name::PARENT);
    while let Some(reference) = parent {
        if chain.len() > limits::MAX_NEST_DEPTH as usize
            || !seen.insert(reference.num)
            || !doc.placed_without_the_tail(reference.num)
        {
            break;
        }
        tally(work, Counter::PageNodesVisited);
        let Ok(object) = doc.get(reference) else {
            break;
        };
        parent = object.as_dict().and_then(|node| node.get_ref(Name::PARENT));
        chain.push(object);
    }
    chain
}

/// Page one of a linearized document, from `/O` (F.3.3, Table F.1).
///
/// Annex F names the first page's object number so a reader holding only the
/// head can reach it **without the page tree**. That is not a shortcut: a
/// linearized file is free to leave the page tree root in the tail and qpdf's
/// linearizer does, so a page-one render that insisted on walking the tree
/// would fetch the end of every such file to draw the front of it.
fn linearized_first_page(doc: &CosDocument, work: Option<&Work>) -> Option<Page> {
    page_alone(doc, ObjRef::new(doc.first_page_object()?, 0), 0, work)
}

/// Page `index` of a linearized document, from Annex F's page offset hint
/// table.
///
/// `/O` names the first page and nothing else, so every page after it needed
/// the main cross-reference table until the hint tables reached the open path.
/// Table F.4 item 2 is what replaces it: a page's location is the accumulated
/// lengths of the pages before it, and item 1 makes the page's own page object
/// the first object of its run.
///
/// Only the *range* comes from the tables. The page is built here only when
/// what the file's own object header names at the front of that range is a
/// leaf with its own `/MediaBox` and `/Resources`; anything else declines with
/// a typed warning naming what was found, and the tree walk runs. Hints
/// accelerate and never decide (`docs/design/streaming-open.md`).
fn hinted_page(doc: &CosDocument, index: u32, work: Option<&Work>) -> Option<Page> {
    let reference = doc.hinted_page_object(index)?;
    let page = page_alone(doc, reference, index, work);
    if page.is_none() {
        doc.warn_object(reference, WarningKind::LinearizedPageHintRejected);
    }
    page
}

/// Convenience: the page at `index`, if it exists.
#[must_use]
pub fn at(doc: &CosDocument, index: u32) -> Option<Page> {
    at_counted(doc, index, None)
}

/// [`at`], counting into `work` when the operation is counting: the objects a
/// linearized file's first-page entry or its hint tables read; or the `/Count`
/// descent, one walk of the nodes on the page's path and the kids before
/// them; or, where a count does not hold up, the walk that gathers
/// `index + 1` pages — which a page past the last one walks to its end.
#[must_use]
pub fn at_counted(doc: &CosDocument, index: u32, work: Option<&Work>) -> Option<Page> {
    if index == 0 {
        if let Some(page) = linearized_first_page(doc, work) {
            return Some(page);
        }
    } else if let Some(page) = hinted_page(doc, index, work) {
        return Some(page);
    }
    // The index, once a count has built it; a lookup never builds it, so one
    // lookup on its own stays within its root and depth x fan-out.
    if let Some(built) = indexed(doc) {
        return copied(&built, index, work);
    }
    match descend(doc, index, work) {
        Descent::Found(page) => return Some(page),
        Descent::Absent => return None,
        Descent::Lied(node) => doc.warn_object(node, WarningKind::PageCountMismatch),
        Descent::Past => {}
    }
    let wanted = (index as usize).checked_add(1)?;
    collect_upto_counted(doc, wanted, work)
        .into_iter()
        .nth(index as usize)
}

/// What the `/Count` descent found.
enum Descent {
    /// The page.
    Found(Page),
    /// No page: the index is at or past `limits::MAX_PAGES`, which the walk
    /// never reaches either.
    Absent,
    /// The page lies past the root's own `/Count`, or the tree has no root to
    /// count: the walk decides, and nothing lied.
    Past,
    /// A count on the path did not hold up at this node: the walk decides, and
    /// the node is named.
    Lied(ObjRef),
}

/// One node of the tree, read and counted as the walk reads and counts one:
/// `None` for a node that does not read as a dictionary, which holds no page.
fn read_node(doc: &CosDocument, node: ObjRef, work: Option<&Work>) -> Option<Arc<Object>> {
    tally(work, Counter::PageNodesVisited);
    let object = doc.get(node).ok()?;
    object.as_dict().is_some().then_some(object)
}

/// Page `index` by 7.7.3.2's `/Count`, without the walk.
///
/// From the root, a node's kids are read in order, each once: a kid without
/// `/Kids` is a leaf and counts one, as the walk takes it whatever its `/Type`
/// says; a kid with `/Kids` counts its own `/Count`; a kid that does not read
/// counts none, as the walk finds no page in it. Kids are passed while their
/// counts sum to no more than the index, and the descent enters the one that
/// holds the page, carrying the inherited attributes down the path exactly as
/// the walk does. So one lookup reads its root and at most fan-out kids at
/// each level under it — and the kids after the one it enters are never read,
/// which is what keeps a page near the front of a flat tree as cheap as the
/// walk makes it.
///
/// What it checks, at each node on the path: a `/Count` that is a
/// non-negative integer and more than what is left of the index; kids whose
/// counts, up to and including the one it enters, stay within it; a leaf
/// reached with nothing left of the index; no node twice on the path; and a
/// path no deeper than `limits::MAX_NEST_DEPTH`. Any that fails is
/// [`Descent::Lied`], and the walk answers. What it cannot see is a lie in a
/// kid it passes: that kid's `/Count` is believed, as a reader that never
/// enters it must.
///
/// Counted as one walk of the tree, whose nodes are the ones it reads.
fn descend(doc: &CosDocument, index: u32, work: Option<&Work>) -> Descent {
    if index as usize >= limits::MAX_PAGES {
        return Descent::Absent;
    }
    let Some(tree) = doc.catalog().and_then(|root| root.get_ref(Name::PAGES)) else {
        return Descent::Past;
    };
    tally(work, Counter::PageWalks);
    let Some(mut object) = read_node(doc, tree, work) else {
        return Descent::Past;
    };
    let mut node = tree;
    let mut left = u64::from(index);
    let mut inherited = Inherited::default();
    let mut path = HashSet::new();
    let mut depth = 0u32;
    loop {
        if depth > limits::MAX_NEST_DEPTH || !path.insert(node.num) {
            return Descent::Lied(node);
        }
        let Some(dict) = object.as_dict() else {
            return Descent::Lied(node);
        };
        inherited = inherited.extend(dict, doc);
        let Some(kids) = dict.get_array(Name::KIDS) else {
            // A leaf, which is the page when the index has run down to it.
            return match left {
                0 => Descent::Found(leaf(node, index, inherited, doc)),
                _ => Descent::Lied(node),
            };
        };
        let Some(count) = dict
            .get_int(Name::COUNT)
            .and_then(|count| u64::try_from(count).ok())
        else {
            return Descent::Lied(node);
        };
        if left >= count {
            // Past the root's count, the walk decides what is there; past an
            // inner node's, its parent counted it wrong.
            return match depth {
                0 => Descent::Past,
                _ => Descent::Lied(node),
            };
        }
        let mut passed = 0u64;
        let mut entered = None;
        for kid in kids.iter().filter_map(Object::as_objref) {
            let read = read_node(doc, kid, work);
            let holds = match read.as_deref().and_then(Object::as_dict) {
                None => 0,
                Some(kid_dict) => match kid_dict.get_array(Name::KIDS) {
                    None => 1,
                    Some(_) => match kid_dict
                        .get_int(Name::COUNT)
                        .and_then(|count| u64::try_from(count).ok())
                    {
                        Some(count) => count,
                        None => return Descent::Lied(kid),
                    },
                },
            };
            let through = passed.saturating_add(holds);
            if through > count {
                return Descent::Lied(node);
            }
            if through > left {
                entered = read.map(|read| (kid, read));
                break;
            }
            passed = through;
        }
        // The kids ran out before the page: their counts fall short of the
        // node's own.
        let Some((kid, read)) = entered else {
            return Descent::Lied(node);
        };
        left -= passed;
        node = kid;
        object = read;
        depth += 1;
    }
}

/// The `/Contents` streams of a page, in order.
///
/// `/Contents` is one stream or an array of them, and the array's parts are
/// concatenated with whitespace between (7.7.3.3) — so a caller wanting the
/// content must join them, not pick one.
#[must_use]
pub fn contents(doc: &CosDocument, page: &Page) -> Vec<ObjRef> {
    let Ok(object) = doc.get(page.reference) else {
        return Vec::new();
    };
    let Some(dict) = object.as_dict() else {
        return Vec::new();
    };

    if let Some(single) = dict.get_ref(Name::CONTENTS) {
        return vec![single];
    }
    let value = doc.resolve_key(dict, Name::CONTENTS);
    value
        .as_array()
        .map(|a| a.iter().filter_map(Object::as_objref).collect())
        .unwrap_or_default()
}

/// Every page's content streams decoded and joined, ready for the interpreter.
#[must_use]
pub fn content_bytes(doc: &CosDocument, page: &Page) -> Vec<u8> {
    content_bytes_counted(doc, page, None)
}

/// [`content_bytes`], counting each part's decode into `work` when the
/// operation is counting.
#[must_use]
pub fn content_bytes_counted(doc: &CosDocument, page: &Page, work: Option<&Work>) -> Vec<u8> {
    let mut out = Vec::new();
    for part in contents(doc, page) {
        if let Ok(bytes) = doc.stream_decoded_counted(part, work) {
            out.extend_from_slice(&bytes);
            // 7.7.3.3: the parts divide at lexical boundaries only if
            // separated, and a producer may end one mid-token.
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF of `objects` (numbered from 1) with a classic table, whose
    /// catalog is object 1.
    fn pdf(objects: &[String]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (at, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", at + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// What `/Pages` node `level` of a generated tree writes besides its kids:
    /// each attribute 7.7.3.4 lets a node pass down, at some levels and not
    /// others, so a page inherits each from a different ancestor.
    fn attributes(level: usize) -> String {
        let mut out = String::new();
        if level % 2 == 0 {
            out += &format!(" /MediaBox [0 0 {} {}]", 300 + level, 400 + 2 * level);
        }
        if level % 3 == 1 {
            out += &format!(" /CropBox [{level} {level} 250 350]");
        }
        if level % 2 == 1 {
            out += &format!(" /Rotate {}", 90 * level);
        }
        if level % 3 == 2 {
            out += &format!(" /Resources << /ProcSet [/PDF] /Level {level} >>");
        }
        out
    }

    /// A tree `depth` levels of `fan_out` kids under its root, every level
    /// full, its nodes passing attributes down at every level, three leaves in
    /// four carrying a box or a rotation of their own, and — where the tree is
    /// two levels deep and the root has two kids or more — the root's first
    /// kid shared by its last kid, in place of that kid's own first kid, so
    /// two parents hold it. Every `/Count` is honest: it counts what the walk
    /// finds, a shared subtree twice.
    fn tree(depth: usize, fan_out: usize) -> Vec<u8> {
        // Numbered breadth first from 2; the catalog is 1.
        let mut levels: Vec<Vec<usize>> = vec![vec![2]];
        let mut next = 3;
        for _ in 0..depth {
            let width = levels[levels.len() - 1].len() * fan_out;
            levels.push((next..next + width).collect());
            next += width;
        }
        let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string()];
        let mut counts = vec![0usize; next];
        // Counts bottom up, the shared subtree counted under both parents.
        for level in (0..=depth).rev() {
            for (at, node) in levels[level].iter().enumerate() {
                counts[*node] = if level == depth {
                    1
                } else {
                    let mut kids: Vec<usize> =
                        levels[level + 1][at * fan_out..(at + 1) * fan_out].to_vec();
                    if level == 1 && fan_out > 1 && at == fan_out - 1 {
                        kids[0] = levels[1][0];
                    }
                    kids.iter().map(|kid| counts[*kid]).sum()
                };
            }
        }
        for (level, nodes) in levels.iter().enumerate() {
            for (at, node) in nodes.iter().enumerate() {
                if level == depth {
                    let own = match node % 4 {
                        0 => " /Rotate 270",
                        1 => " /MediaBox [0 0 123 456]",
                        2 => " /TrimBox [5 5 100 100]",
                        _ => "",
                    };
                    objects.push(format!("<< /Type /Page{own} >>"));
                    continue;
                }
                let mut kids: Vec<usize> =
                    levels[level + 1][at * fan_out..(at + 1) * fan_out].to_vec();
                if level == 1 && fan_out > 1 && at == fan_out - 1 {
                    kids[0] = levels[1][0];
                }
                let kids: Vec<String> = kids.iter().map(|kid| format!("{kid} 0 R")).collect();
                objects.push(format!(
                    "<< /Type /Pages{} /Kids [{}] /Count {} >>",
                    attributes(level),
                    kids.join(" "),
                    counts[*node]
                ));
            }
        }
        pdf(&objects)
    }

    /// Whether two pages are the same page: reference, index, every box, the
    /// rotation and the resources.
    fn same(a: &Page, b: &Page) -> bool {
        a.reference == b.reference
            && a.index == b.index
            && a.media_box == b.media_box
            && a.crop_box == b.crop_box
            && a.bleed_box == b.bleed_box
            && a.trim_box == b.trim_box
            && a.art_box == b.art_box
            && a.rotation == b.rotation
            && format!("{:?}", a.resources) == format!("{:?}", b.resources)
    }

    /// **The descent hands back the walk's page, for every page of every
    /// honest tree**: fan-out 1 to 16, depth 1 to 6, at most 4 096 pages,
    /// attributes inherited at every level, and a subtree two parents share.
    /// Each lookup is the descent, not the walk or the index: it reads its root
    /// and at most fan-out kids a level, where the walk to a late page reads
    /// every node before it.
    #[test]
    fn descent_and_walk_agree_on_every_page_of_honest_trees() {
        let mut trees = 0;
        for fan_out in 1..=16usize {
            for depth in 1..=6u32 {
                let Some(leaves) = fan_out.checked_pow(depth).filter(|n| *n <= 4_096) else {
                    continue;
                };
                let doc = CosDocument::open(tree(depth as usize, fan_out)).expect("the tree opens");
                let walked = collect(&doc);
                // The shared subtree, `fan_out^(depth - 1)` pages, stands in
                // for one of `fan_out^(depth - 2)`.
                let shared = if fan_out > 1 && depth >= 2 {
                    fan_out.pow(depth - 1) - fan_out.pow(depth - 2)
                } else {
                    0
                };
                assert_eq!(
                    walked.len(),
                    leaves + shared,
                    "the walk of {fan_out}^{depth}"
                );
                for page in &walked {
                    // The descent itself: a lookup through `at` builds the
                    // index at its second miss and reads pages out of it.
                    let work = Work::new();
                    let Descent::Found(found) = descend(&doc, page.index, Some(&work)) else {
                        panic!(
                            "{fan_out}^{depth}: the descent gave up on page {}",
                            page.index
                        );
                    };
                    assert!(
                        same(&found, page),
                        "{fan_out}^{depth}: page {} is {found:?} by the descent and {page:?} by \
                         the walk",
                        page.index
                    );
                    let stats = work.stats();
                    // A page under the shared subtree sits a level deeper.
                    let deepest = u64::from(depth) + u64::from(shared > 0);
                    let bound = 1 + deepest * fan_out as u64;
                    assert_eq!(stats.page_walks, 1, "one walk of the tree");
                    assert!(
                        stats.page_nodes_visited <= bound,
                        "{fan_out}^{depth}: page {} read {} nodes, over {bound}",
                        page.index,
                        stats.page_nodes_visited
                    );
                }
                assert!(
                    at(&doc, walked.len() as u32).is_none(),
                    "past the last page"
                );
                assert!(
                    !doc.warnings()
                        .iter()
                        .any(|w| w.kind == WarningKind::PageCountMismatch),
                    "{fan_out}^{depth}: an honest tree is said to lie"
                );
                trees += 1;
            }
        }
        assert!(trees >= 40, "{trees} trees");
    }

    /// **A count that lies falls back to the walk and says so**: a node whose
    /// `/Count` claims a page its kids do not hold sends the lookup to the
    /// walk, which answers as it always did, and the lie is a
    /// `PageCountMismatch` naming the node. A count that is not a non-negative
    /// integer, and a node met twice on the path, do the same.
    #[test]
    fn a_lying_count_falls_back_to_the_walk_and_says_so() {
        // 2 is the root, holding 3 (claiming 3 pages and holding 2) and the
        // leaf 6; 4 and 5 are 3's leaves.
        let lying = |count: &str| {
            pdf(&[
                "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
                "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 4 /MediaBox [0 0 100 100] >>"
                    .to_string(),
                format!("<< /Type /Pages /Kids [4 0 R 5 0 R] /Count {count} >>"),
                "<< /Type /Page /Rotate 90 >>".to_string(),
                "<< /Type /Page /Rotate 180 >>".to_string(),
                "<< /Type /Page /Rotate 270 >>".to_string(),
            ])
        };
        for count in ["3", "-1", "two"] {
            let doc = CosDocument::open(lying(count)).expect("it opens");
            let walked = collect(&doc);
            assert_eq!(walked.len(), 3, "the walk finds three pages");
            // Page 2 is the leaf 6, which the lie in 3 would place wrong.
            let found = at(&doc, 2).expect("the walk finds page 2");
            assert!(same(&found, &walked[2]), "/Count {count}: {found:?}");
            assert_eq!(found.reference, ObjRef::new(6, 0));
            let named = doc.warnings().into_iter().any(|w| {
                w.kind == WarningKind::PageCountMismatch && w.object == Some(ObjRef::new(3, 0))
            });
            assert!(named, "/Count {count}: no PageCountMismatch names node 3");
        }

        // A node met twice on the path: 3's kids hold 3 itself.
        let cyclic = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 2 /MediaBox [0 0 100 100] >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page >>".to_string(),
        ]);
        let doc = CosDocument::open(cyclic).expect("it opens");
        let walked = collect(&doc);
        let found = at(&doc, 0);
        assert_eq!(
            found.map(|p| p.reference),
            walked.first().map(|p| p.reference)
        );
        assert!(doc
            .warnings()
            .iter()
            .any(|w| w.kind == WarningKind::PageCountMismatch));
    }

    #[test]
    fn rotation_normalizes_every_way_a_file_can_write_it() {
        assert_eq!(normalize_rotation(0), 0);
        assert_eq!(normalize_rotation(90), 90);
        assert_eq!(normalize_rotation(450), 90, "more than one turn");
        assert_eq!(normalize_rotation(-90), 270, "negative angles wrap");
        assert_eq!(normalize_rotation(-450), 270);
        assert_eq!(normalize_rotation(360), 0);
        assert_eq!(normalize_rotation(100), 90, "off-quarter angles round");
        assert_eq!(normalize_rotation(i64::MIN), 0, "no overflow");
        assert_eq!(normalize_rotation(i64::MAX), 0);
    }

    #[test]
    fn rectangles_order_their_corners() {
        let values = [
            Object::Int(100),
            Object::Int(200),
            Object::Int(0),
            Object::Int(0),
        ];
        let r = Rect::from_array(&values).expect("a rectangle");
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0.0, 0.0, 100.0, 200.0));
        assert_eq!((r.width(), r.height()), (100.0, 200.0));
    }

    #[test]
    fn a_rectangle_needs_four_finite_numbers() {
        assert!(Rect::from_array(&[Object::Int(1), Object::Int(2)]).is_none());
        assert!(Rect::from_array(&[
            Object::Real(f64::NAN),
            Object::Int(0),
            Object::Int(1),
            Object::Int(1)
        ])
        .is_none());
    }

    #[test]
    fn intersection_of_disjoint_rectangles_is_nothing() {
        let a = Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        let b = Rect {
            x0: 20.0,
            y0: 20.0,
            x1: 30.0,
            y1: 30.0,
        };
        assert!(a.intersect(&b).is_none());
        assert_eq!(a.intersect(&a), Some(a));
    }

    #[test]
    fn display_size_swaps_on_quarter_turns() {
        let page = Page {
            reference: ObjRef::new(1, 0),
            index: 0,
            media_box: Rect::US_LETTER,
            crop_box: Rect::US_LETTER,
            bleed_box: Rect::US_LETTER,
            trim_box: Rect::US_LETTER,
            art_box: Rect::US_LETTER,
            rotation: 0,
            resources: None,
        };
        assert_eq!(page.display_size(), (612.0, 792.0));
        let rotated = Page {
            rotation: 90,
            ..page.clone()
        };
        assert_eq!(rotated.display_size(), (792.0, 612.0));
        let upside_down = Page {
            rotation: 180,
            ..page
        };
        assert_eq!(upside_down.display_size(), (612.0, 792.0));
    }

    /// 7.7.3.3 allows `/Resources` to be an inline dictionary, and producers
    /// use it — this crate's own builder among them.
    ///
    /// This was once typed as a reference, so a direct dictionary resolved to
    /// nothing and the page lost every font it had. Nothing failed loudly:
    /// text extraction simply returned an empty string.
    #[test]
    fn a_direct_resource_dictionary_is_read() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
   /Resources << /Font << /F0 4 0 R >> >> >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let pages = collect(&doc);
        assert_eq!(pages.len(), 1);

        let resources = pages[0]
            .resources
            .as_ref()
            .expect("the inline dictionary is read");
        assert!(
            resources.get(doc.intern(b"Font")).is_some(),
            "and its /Font survives"
        );
    }

    /// The indirect form still works, and still inherits down the tree.
    #[test]
    fn an_inherited_indirect_resource_dictionary_is_read() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] /Resources 4 0 R >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>\nendobj\n\
4 0 obj\n<< /Font << /F0 5 0 R >> >>\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let pages = collect(&doc);
        let resources = pages[0]
            .resources
            .as_ref()
            .expect("inherited from the parent node");
        assert!(resources.get(doc.intern(b"Font")).is_some());
    }

    fn corners(r: Rect) -> (f64, f64, f64, f64) {
        (r.x0, r.y0, r.x1, r.y1)
    }

    /// Table 30: the bleed, trim and art boxes are the page's own. A value on
    /// the `/Pages` node above it describes no page, and a page without one
    /// reads its crop box.
    #[test]
    fn the_production_boxes_are_not_inherited() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 200 100]\n\
   /CropBox [10 10 190 90] /TrimBox [20 20 180 80] /ArtBox [30 30 170 70] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /TrimBox [25 25 175 75] >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let pages = collect(&doc);
        assert_eq!(pages.len(), 2);
        let crop = (10.0, 10.0, 190.0, 90.0);
        assert_eq!(
            corners(pages[0].crop_box),
            crop,
            "the crop box does inherit"
        );
        for boundary in [
            PageBoundary::BleedBox,
            PageBoundary::TrimBox,
            PageBoundary::ArtBox,
        ] {
            assert_eq!(
                corners(pages[0].boundary(boundary)),
                crop,
                "{boundary:?} comes from the page or defaults to the crop box"
            );
        }
        assert_eq!(corners(pages[1].trim_box), (25.0, 25.0, 175.0, 75.0));
        assert_eq!(corners(pages[1].art_box), crop, "the parent's is not taken");
    }

    /// 14.11.2.1: a box reaching past the media box is its intersection with
    /// it; one that misses the media box entirely is the crop box, and the
    /// corners are ordered however the file wrote them.
    #[test]
    fn a_production_box_is_clipped_to_the_media_box() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
   /BleedBox [-5 -5 205 105] /TrimBox [180 90 20 10] /ArtBox [300 300 400 400] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n";

        let doc = CosDocument::open(&bytes[..]).expect("it opens");
        let page = &collect(&doc)[0];
        assert_eq!(corners(page.bleed_box), (0.0, 0.0, 200.0, 100.0));
        assert_eq!(corners(page.trim_box), (20.0, 10.0, 180.0, 90.0));
        assert_eq!(corners(page.art_box), corners(page.crop_box));
    }

    #[test]
    fn a_boundary_names_itself_both_ways() {
        for boundary in PageBoundary::ALL {
            assert_eq!(PageBoundary::from_key(boundary.key()), Some(boundary));
        }
        assert_eq!(PageBoundary::from_key(b"Trimbox"), None, "names are bytes");
    }
}
