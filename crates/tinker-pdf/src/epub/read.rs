//! The cascade over a content document, and the box tree it produces (gap 31,
//! milestone 8).
//!
//! Milestone 6 built a CSS engine that matches against a trait, milestone 7 a
//! layout engine that takes plain structs; this is the file that owns the join,
//! and the two decisions it makes are the ones neither leaf could.
//!
//! # The user-agent stylesheet is a file, and it is committed
//!
//! [`UA_STYLESHEET`] is `src/epub/ua.css`, included at compile time and parsed
//! by milestone 6's parser exactly as a book's own sheet is. It is not a table
//! of Rust constants and not a `match` on element names, and that is the whole
//! point: **a UA sheet written in Rust is a second style system**, with its own
//! specificity rules, its own cascade order and no way for an author to beat
//! it. Written as CSS at `Origin::UserAgent` it loses to a book's own rules by
//! `css-cascade-5` §6.1's ordinary machinery, which is what a reading system
//! is required to do.
//!
//! Its absence is **visible and not merely worse**, and
//! `tests/epub_reading.rs` is what says so: with no UA sheet every
//! element computes `display: inline`, so a book has no block boxes at all —
//! every chapter is one paragraph, every heading is body text, and `<head>` is
//! set into the flow. Those are three independent consequences and the test
//! asserts all three, because a test for one of them is not a test.
//!
//! # Text nodes get the anonymous inline box's style, not the parent's
//!
//! CSS 2.2 §9.2.2.1 wraps a text node in an **anonymous inline box** that
//! inherits from its parent and has no margin, no padding, no border and no
//! background. Giving the text the parent's own computed style instead is the
//! shortcut that looks identical on a `<span>` and doubles every margin on a
//! `<p>`: the parent's `margin: 1em 0` would be applied by the `<p>`'s block
//! box and again by a text child that `tinker-pdf-layout` would then treat as
//! block-level. [`inline_box`] is that rule, and
//! `a_paragraph_does_not_pay_its_own_margin_twice` is what holds it.

use std::cell::RefCell;
use std::collections::HashSet;

use tinker_pdf_cos::{gif_image, png_image, webp_image};
use tinker_pdf_css::cascade::{cascade_from, ComputedStyle, Origin, PseudoBox, StyleTree};
use tinker_pdf_css::font_face::FontFace;
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::Stylesheet;
use tinker_pdf_css::property::{Display, Float, Overflow, Position};
use tinker_pdf_css::selector::PseudoElement;
use tinker_pdf_css::{
    Budget as CssBudget, ImportResolver, Limits as CssLimits, Refusal as CssRefusal,
};
use tinker_pdf_filters::Limits as FilterLimits;
use tinker_pdf_layout::{BoxNode, CellSpan, Content, Intrinsic};
use tinker_pdf_zip::limits as zip_limits;

use super::ocf::{resolve_reference, Ocf};
use super::xhtml::{Child, Dom, Node};
use super::Limits;
use crate::cbz::{image_format, ImageDefect, ImageFormat};

/// The user-agent stylesheet, as CSS, committed at `src/epub/ua.css`.
pub const UA_STYLESHEET: &str = include_str!("ua.css");

/// CSS 2.2 §4.3.2's reference pixel against a PDF point: 96 to 72.
///
/// Everything `tinker-pdf-css` computes is in CSS pixels — `absolute_px` turns
/// a `pt` into one and not the other way round — and everything a PDF page is
/// measured in is points. The factor lives here, once, at the boundary between
/// the two, because a build that converted in two places would eventually
/// convert in one and a half.
pub const PX_TO_PT: f64 = 72.0 / 96.0;

/// What one content document's stylesheets cost and could not do.
#[derive(Clone, Debug, Default)]
pub struct Census {
    /// Properties this build knows the name of and did not honour, per
    /// property, counted **by element reached** — `tinker-pdf-css`'s own
    /// counting, which is what makes the number a property of the book rather
    /// than of the stylesheet.
    pub unsupported: Vec<(&'static str, usize)>,
    /// Names no specification this build cites defines: a vendor extension, a
    /// custom property, or a typo.
    pub unknown: Vec<(String, usize)>,
    /// Declarations discarded by `css-syntax-3` §5.4.4.
    pub discarded_declarations: usize,
    /// Rules discarded by §5.4.2.
    pub discarded_rules: usize,
}

impl Census {
    /// Folds another census into this one, keeping the counts additive.
    pub fn absorb(&mut self, other: &Census) {
        for (property, count) in &other.unsupported {
            match self.unsupported.iter_mut().find(|(p, _)| p == property) {
                Some(slot) => slot.1 += count,
                None => self.unsupported.push((property, *count)),
            }
        }
        for (property, count) in &other.unknown {
            match self.unknown.iter_mut().find(|(p, _)| p == property) {
                Some(slot) => slot.1 += count,
                None => self.unknown.push((property.clone(), *count)),
            }
        }
        self.discarded_declarations += other.discarded_declarations;
        self.discarded_rules += other.discarded_rules;
    }

    /// The census, sorted by count and then by name, which is what a report
    /// prints and what a test compares.
    #[must_use]
    pub fn ranked(&self) -> Vec<(&'static str, usize)> {
        let mut out = self.unsupported.clone();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        out
    }

    /// How many elements were affected by an unimplemented property, in total.
    #[must_use]
    pub fn affected(&self) -> usize {
        self.unsupported.iter().map(|(_, count)| count).sum()
    }
}

/// Why a reference a content document made did not produce bytes.
///
/// Two answers and not one, because they blame different parties and
/// [`super::typeface::FaceDefect`] already tells them apart: a reference that
/// names nothing is the document's mistake, and an entry that is there and
/// will not inflate is the container's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unavailable {
    /// The reference is not one this provider can resolve, or it resolves to
    /// nothing the provider holds.
    Missing,
    /// It resolves to something the provider holds, and the bytes would not
    /// come out.
    Unreadable,
}

/// Where a content document's references are read from: a stylesheet's
/// `<link href>` and `@import`, an `<img src>`, an `@font-face` `url()`.
///
/// # Why this is a trait and not an OCF container
///
/// Until tier 5's formats row every one of those references was resolved
/// against [`Ocf`], because a content document only ever arrived inside one.
/// A loose XHTML file, a creation call handed markup and a stylesheet, and an
/// FB2 whose pictures are `<binary>` elements in the same file are documents
/// read by **the same cascade and the same layout** with different answers to
/// *"what does `cover.jpg` mean here"* — and that answer is the only thing
/// that differs. So the reader asks this, and each caller supplies its own;
/// the EPUB path is the [`Ocf`] implementation below, which does exactly what
/// the four call sites it replaced did.
///
/// The provider resolves as well as reads, and that is deliberate: a `data:`
/// URL (RFC 2397) is a reference with no path at all, and §4.2.3's grammar,
/// which [`resolve_reference`] enforces, refuses it by its scheme. A provider
/// that was only handed paths could never answer one.
pub trait Resources {
    /// The path `reference` names when written in the document at
    /// `referring`, and the bytes there.
    ///
    /// The path comes back because it is the base for anything the fetched
    /// resource itself refers to — an `@import` inside an imported sheet — and
    /// because two references spelled differently that name one file must be
    /// recognisably one file ([`super::typeface::load`] deduplicates on it).
    ///
    /// # Errors
    /// [`Unavailable`], naming which half failed.
    fn fetch(
        &mut self,
        referring: &str,
        reference: &str,
        limits: &Limits,
    ) -> Result<(String, Vec<u8>), Unavailable>;
}

/// A provider borrowed is a provider, so a caller can lend one to a reader
/// that wraps it — [`crate::standalone::DataUrls`] in front of a creation
/// call's — and still hold it afterwards.
impl<R: Resources + ?Sized> Resources for &mut R {
    fn fetch(
        &mut self,
        referring: &str,
        reference: &str,
        limits: &Limits,
    ) -> Result<(String, Vec<u8>), Unavailable> {
        (**self).fetch(referring, reference, limits)
    }
}

/// An OCF container resolves a reference as §4.2.5 says: against the referring
/// document, by [`resolve_reference`], to an entry compared case-sensitively.
impl Resources for Ocf<'_> {
    fn fetch(
        &mut self,
        referring: &str,
        reference: &str,
        limits: &Limits,
    ) -> Result<(String, Vec<u8>), Unavailable> {
        let path =
            resolve_reference(referring, reference, limits).map_err(|_| Unavailable::Missing)?;
        let index = self.index_of(&path).ok_or(Unavailable::Missing)?;
        let bytes = self
            .read(index)
            .map_err(|_| Unavailable::Unreadable)?
            .to_vec();
        Ok((path, bytes))
    }
}

/// A document with nothing beside it: every reference is missing.
///
/// A reference such a document makes is named as unresolved by whatever made
/// it — [`crate::ArchiveWarning::ImageNotDrawn`] for a picture,
/// [`super::typeface::FaceDefect::ResourceMissing`] for a face — rather than
/// guessed at.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoResources;

impl Resources for NoResources {
    fn fetch(&mut self, _: &str, _: &str, _: &Limits) -> Result<(String, Vec<u8>), Unavailable> {
        Err(Unavailable::Missing)
    }
}

/// Where an `@import` in a book's stylesheet is resolved from.
///
/// Milestone 6 built [`ImportResolver`] and shipped `NoImports` beside it,
/// saying in as many words that *"a caller that has an OCF container
/// implements this"*. This is that caller, over any [`Resources`]. The
/// `RefCell` is not a shortcut: [`Ocf::read`] takes `&mut self` because
/// inflating an entry spends the archive's budget, and the trait takes `&self`
/// because a resolver is shared by a whole parse.
pub(crate) struct Imports<'b, R: ?Sized> {
    resources: RefCell<&'b mut R>,
    limits: Limits,
}

impl<'b, R: Resources + ?Sized> Imports<'b, R> {
    /// A resolver over `resources`, for one parse.
    pub(crate) fn new(resources: &'b mut R, limits: Limits) -> Self {
        Imports {
            resources: RefCell::new(resources),
            limits,
        }
    }
}

impl<R: Resources + ?Sized> ImportResolver for Imports<'_, R> {
    fn resolve(&self, href: &str, base: Option<&str>) -> Option<(String, Vec<u8>)> {
        // A sheet with no address of its own is a `<style>` element, and its
        // base is the document that holds it — which the caller put in `base`
        // for exactly this. With neither there is nothing to resolve against
        // and the import is dropped rather than guessed at.
        let base = base?;
        self.resources
            .borrow_mut()
            .fetch(base, href, &self.limits)
            .ok()
    }
}

/// Everything a content document is read *against*, which is the same for
/// every spine item in one book.
///
/// A struct rather than five parameters, and not only because
/// `clippy::too_many_arguments` says so: **these five are the book's, and the
/// two that are not — the path and the bytes — are the chapter's.** A caller
/// that built a fresh one per spine item would re-tokenize the user-agent
/// sheet thirteen times and could give two chapters of one book different
/// initial values.
pub struct Context<'a> {
    /// The parsed user-agent stylesheet, parsed once per book.
    pub ua: &'a [Stylesheet],
    /// Author sheets the caller supplies rather than the document links, in
    /// the order they apply, ahead of every sheet the document names.
    ///
    /// Empty for a book, whose sheets are all its own. A creation call handed
    /// markup and a stylesheet separately puts the stylesheet here, which is
    /// what a `<link>` at the top of the document's `<head>` would have done.
    pub author: &'a [Stylesheet],
    /// The container's own ceilings, for resolving a `<link href>`.
    pub limits: &'a Limits,
    /// What the cascade may spend.
    pub css_limits: &'a CssLimits,
    /// What `@media` is evaluated against.
    ///
    /// The book's page box for a reflowable document. A **pre-paginated** one
    /// is cascaded against its own §8.2.2.6 viewport instead, which
    /// [`read_document`] substitutes once it has the tree: the dimensions are
    /// in the content document, so they are not knowable until it has been
    /// read, and a `@media (max-width: 600px)` block in a fixed-layout book is
    /// about that document's viewport and not about the reading system's page.
    pub media: &'a MediaContext,
    /// Whether this spine item is EPUB 3.3 §8.2's `pre-paginated`.
    pub pre_paginated: bool,
    /// The root element's initial values, carrying the caller's base font
    /// size. See [`tinker_pdf_css::cascade::cascade_from`].
    pub initial: &'a ComputedStyle,
}

/// One content document, cascaded and turned into a box tree.
pub struct Reading {
    /// The element tree, kept because the anchors on the box tree index into
    /// it: a link's target, a destination's `id` and the outline all need to
    /// walk back from a positioned run to the element it came from.
    pub dom: Dom,
    /// The box tree, rooted at `<body>`.
    pub tree: BoxNode,
    /// One computed style per element of [`Reading::dom`].
    pub styles: StyleTree,
    /// What the cascade could not honour.
    pub census: Census,
    /// Every `@font-face` this document's author sheets declared, in source
    /// order, each carrying the address it must be resolved against (gap 31,
    /// milestone 9).
    ///
    /// Per **document** and not per book, because a spine item's sheets are
    /// its own — and folded into one set by the caller, because a PDF's font
    /// resources belong to the document rather than to a page.
    pub font_faces: Vec<FontFace>,
    /// §8.2.2.6's viewport, where the document states one.
    ///
    /// Read for **every** document rather than only for a pre-paginated one,
    /// because a reflowable book that carries the element is not thereby
    /// fixed-layout — §8.2.1's `rendition:layout` is what decides that — and
    /// reading it here keeps the two questions apart.
    pub viewport: Option<super::xhtml::Viewport>,
    /// Every `<img>` in the document, resolved against the container: the ones
    /// that became replaced boxes, with their bytes, and the ones that did not,
    /// with the reason.
    pub pictures: Pictures,
    /// `<link rel="stylesheet">` elements whose `href` produced no sheet.
    ///
    /// Counted because the document is then set without rules its author
    /// wrote, and nothing on the page says so: a loose XHTML file opened from
    /// its bytes alone has nothing beside it, so every sheet it links lands
    /// here, and a book that names an entry its container does not hold is the
    /// same sentence about a smaller mistake.
    pub unresolved_sheets: usize,
}

/// Reads one content document: markup, stylesheets, cascade, box tree.
///
/// `ua` is the parsed user-agent sheet, parsed once per book rather than once
/// per spine item — a thirteen-chapter book would otherwise tokenize the same
/// four kilobytes thirteen times, and the sheet cannot differ between them.
///
/// `initial` carries the caller's base font size as the root element's initial
/// value; see [`tinker_pdf_css::cascade::cascade_from`] for why that is not a
/// stylesheet rule.
///
/// # Errors
/// A [`CssRefusal`] from the cascade: a cap, or a tree the cascade will not
/// walk. A markup failure is **not** an error — it is a
/// [`super::xhtml::MarkupDefect`] on a partial tree, because a chapter that
/// stops half way has still said most of itself.
pub fn read_document<R: Resources + ?Sized>(
    book: &mut R,
    path: &str,
    bytes: &[u8],
    context: &Context<'_>,
    budget: &mut CssBudget,
) -> Result<Reading, CssRefusal> {
    let dom = markup(bytes, &context.limits.xml);
    read_dom(book, path, dom, context, budget)
}

/// A content document's markup, read as XML into a tree.
///
/// Never fails: a document the reader stops part way through is the tree it
/// got to with [`super::xhtml::MarkupDefect::Truncated`] on it, and one it
/// cannot begin — an encoding this build does not decode, or a character XML
/// §2.2 forbids — is no tree at all with the same defect, so the page that
/// results says so through its own defect rather than through this one.
#[must_use]
pub fn markup(bytes: &[u8], limits: &tinker_pdf_xml::Limits) -> Dom {
    match super::xhtml::read(bytes, limits) {
        Ok(dom) => dom,
        Err(_) => Dom {
            defects: vec![super::xhtml::MarkupDefect::Truncated],
            ..Dom::default()
        },
    }
}

/// [`read_document`] for a tree something else already built.
///
/// The markup reader is the one step a content document's language decides:
/// an EPUB's is XML, a loose `.html` file's is HTML's own tree builder, and an
/// FB2's or a Markdown file's is a translation into this tree. Everything from
/// the stylesheets on is the same reading of the same tree, which is why this
/// is where [`read_document`] hands over rather than a second copy of it.
///
/// # Errors
/// [`read_document`]'s.
pub fn read_dom<R: Resources + ?Sized>(
    book: &mut R,
    path: &str,
    dom: Dom,
    context: &Context<'_>,
    budget: &mut CssBudget,
) -> Result<Reading, CssRefusal> {
    let Context {
        ua,
        author: given,
        limits,
        css_limits,
        media,
        pre_paginated,
        initial,
    } = context;

    // §8.2.2.6's viewport, and the media context that follows from it. The
    // substitution happens **here** rather than at the caller because the
    // dimensions are inside the document: a caller could only supply them by
    // parsing the markup a second time.
    let viewport = dom.viewport();
    let own_media;
    let media: &MediaContext = match (pre_paginated, viewport) {
        (true, Some(view)) => {
            own_media = MediaContext::screen(view.width, view.height);
            &own_media
        }
        _ => media,
    };

    let mut unresolved_sheets = 0;
    let own = author_sheets(
        book,
        path,
        &dom,
        limits,
        css_limits,
        budget,
        media,
        &mut unresolved_sheets,
    );
    // A caller's sheets come first, as a `<link>` at the top of `<head>` would:
    // §6.1's order of appearance then lets the document's own rules win a tie,
    // which is what an author who wrote a `<style>` element meant by it.
    let author: Vec<&Stylesheet> = given.iter().chain(own.iter()).collect();
    let mut sheets: Vec<(Origin, &Stylesheet)> =
        ua.iter().map(|sheet| (Origin::UserAgent, sheet)).collect();
    for sheet in &author {
        sheets.push((Origin::Author, *sheet));
    }

    let styles = cascade_from(&sheets, &dom.nodes, css_limits, budget, initial)?;

    let mut census = Census {
        unsupported: styles.report.unsupported.clone(),
        unknown: styles.report.unknown.clone(),
        discarded_declarations: 0,
        discarded_rules: 0,
    };
    // A sheet's own parse report is counted **per sheet**, not per element: a
    // declaration §5.4.4 threw away never reached the cascade at all, so there
    // is no element for it to have affected. The two numbers are kept apart
    // for that reason rather than summed into one that means neither.
    for sheet in &author {
        census.discarded_declarations += sheet.report.discarded_declarations;
        census.discarded_rules += sheet.report.discarded_rules;
    }

    // A `<style>` element's sheet has no address of its own, so `parse` left
    // the base `None` even though the element is inside a document that does
    // have one. Filling it in here rather than in `tinker-pdf-css` is ruling 8:
    // the CSS crate knows what the sheet said and this one knows where the
    // sheet was.
    let mut font_faces: Vec<FontFace> = Vec::new();
    for sheet in &author {
        for face in &sheet.font_faces {
            let mut face = face.clone();
            if face.base.is_none() {
                face.base = Some(path.to_owned());
            }
            font_faces.push(face);
        }
    }

    // After the cascade and before the tree, which is the only place it can be:
    // the box tree needs each picture's dimensions and the cascade needs the
    // container's reader, which `author_sheets` is still holding until here.
    let pictures = pictures(book, path, &dom, limits);
    let tree = box_tree(&dom, &styles, &pictures);
    Ok(Reading {
        dom,
        tree,
        styles,
        census,
        viewport,
        font_faces,
        pictures,
        unresolved_sheets,
    })
}

/// Every stylesheet a content document pulls in, in document order.
///
/// `<link rel="stylesheet">` and `<style>` in the order they appear, which is
/// `css-cascade-5` §6.1's sixth criterion — two sheets that set the same
/// property at the same specificity are decided by which came later, and a
/// build that read every `<link>` before every `<style>` would get that
/// backwards for calibre's books, which write both.
///
/// `unresolved` counts the `<link rel="stylesheet" href>` elements whose
/// reference produced no bytes — the one sheet a document names that this build
/// can tell it did not apply, where a sheet that would not parse is the CSS
/// crate's to report.
#[allow(clippy::too_many_arguments)]
fn author_sheets<R: Resources + ?Sized>(
    book: &mut R,
    path: &str,
    dom: &Dom,
    limits: &Limits,
    css_limits: &CssLimits,
    budget: &mut CssBudget,
    media: &MediaContext,
    unresolved: &mut usize,
) -> Vec<Stylesheet> {
    let mut out = Vec::new();
    for node in &dom.nodes {
        if !node.is_html() {
            continue;
        }
        match node.name.as_str() {
            "link" => {
                if !applies_as_stylesheet(node.attr("rel").unwrap_or_default()) {
                    continue;
                }
                let Some(href) = node.attr("href") else {
                    continue;
                };
                let Ok((target, bytes)) = book.fetch(path, href, limits) else {
                    *unresolved += 1;
                    continue;
                };
                let resolver = Imports::new(&mut *book, *limits);
                if let Ok(sheet) = tinker_pdf_css::parser::parse(
                    &bytes,
                    Some(&target),
                    &resolver,
                    media,
                    css_limits,
                    budget,
                ) {
                    out.push(sheet);
                }
            }
            "style" => {
                let mut source = String::new();
                for child in &node.children {
                    if let Child::Text(text) = child {
                        source.push_str(text);
                    }
                }
                if source.trim().is_empty() {
                    continue;
                }
                let resolver = Imports::new(&mut *book, *limits);
                // The **document's** path is the base, not `None`: a `<style>`
                // has no address of its own and HTML resolves a relative URL in
                // it against the document. Passing `None` would drop every
                // `@import` in an inline sheet.
                if let Ok(sheet) = tinker_pdf_css::parser::parse(
                    source.as_bytes(),
                    Some(path),
                    &resolver,
                    media,
                    css_limits,
                    budget,
                ) {
                    out.push(sheet);
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether a `<link rel>` names a stylesheet this build applies.
///
/// HTML §4.2 makes `rel` a **token list**, and two of its tokens matter here.
/// `stylesheet` is what makes the link one at all; `alternate` is what makes it
/// one a reading system offers rather than applies, and applying it would set
/// a book in a theme its author marked as *not the default*. A build that
/// compared the whole attribute against `"stylesheet"` would drop
/// `rel="stylesheet next"`, and one that searched for the substring would apply
/// `rel="alternate stylesheet"`.
///
/// A function of its own because neither corpus contains an alternate sheet, so
/// the rule is unreachable from a real book and a defect injected into it
/// survived every test in the suite until this existed.
#[must_use]
pub fn applies_as_stylesheet(rel: &str) -> bool {
    let mut names = false;
    let mut alternate = false;
    for token in rel.split_whitespace() {
        names |= token.eq_ignore_ascii_case("stylesheet");
        alternate |= token.eq_ignore_ascii_case("alternate");
    }
    names && !alternate
}

/// CSS 2.2 §9.2.2.1's anonymous inline box: the parent's inherited values and
/// nothing else.
///
/// `text-decoration` is copied across although it is not an inherited
/// property, which is §16.3.1's rule rather than an exception invented here:
/// the decoration is drawn by the element that declared it **across its
/// in-flow descendants**, so an `<a>`'s underline reaches its text and a
/// `<div>`'s reaches every line in it. A build that dropped it would underline
/// nothing anywhere, since a decoration is only ever declared on an ancestor
/// of the text it marks.
#[must_use]
pub fn inline_box(parent: &ComputedStyle) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent);
    style.text_decoration = parent.text_decoration;
    style
}

/// One `<img>` this build can put on a page, with the bytes it will put there.
///
/// **The bytes are held rather than the entry index**, and that is the write
/// pass's ordering rather than a cache: `DocumentBuilder::begin_page` snapshots
/// the document's resource set, so every picture in the book has to be
/// registered before the first page begins — by which time the container's
/// reader is no longer being walked in spine order. It is the same lesson
/// [`super::svg::Registry`] is named after, one element along.
#[derive(Debug)]
pub struct Picture {
    /// The element this picture is the content of, indexing `Dom::nodes`.
    pub element: usize,
    /// The picture's own pixel dimensions, which are CSS pixels: `css-images-3`
    /// §4.1 makes a raster's intrinsic size *"its density-corrected intrinsic
    /// size"*, and nothing in an EPUB sets a density.
    pub intrinsic: (f64, f64),
    /// The bytes, in whichever shape the writer takes them.
    pub data: PictureData,
}

/// A picture's bytes, ready for `DocumentBuilder::add_image`.
///
/// The same routes `cbz.rs` takes and for its reasons: a JPEG is placed
/// verbatim because re-encoding is generational loss the caller cannot undo,
/// a PNG goes through the reader that decides between passing its `IDAT`
/// through and decoding it, a GIF — which no `/Filter` reads — is decoded
/// and kept `/Indexed`, and a WebP is decoded to RGB or RGBA.
///
/// `#[non_exhaustive]` like every other facade enum here: it grew `Raster`
/// when GIF and WebP gained decoders, which broke any match outside this
/// crate, and the next route a picture can take would grow it again.
#[non_exhaustive]
pub enum PictureData {
    /// A JPEG, placed as its own bytes.
    Jpeg(Vec<u8>),
    /// A PNG, read into whatever `tinker-pdf-cos` decided to write.
    Png(Box<tinker_pdf_cos::PngImageData>),
    /// A GIF's first image or a WebP's picture, decoded and arranged by
    /// `tinker-pdf-cos`.
    Raster(Box<tinker_pdf_cos::RasterImageData>),
}

impl std::fmt::Debug for PictureData {
    /// Hand-written because `PngImageData` is not `Debug`: it owns a decoded
    /// raster, and a derived one would print a book's worth of samples.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PictureData::Jpeg(bytes) => write!(f, "Jpeg({} bytes)", bytes.len()),
            PictureData::Png(png) => write!(f, "Png({} by {})", png.width(), png.height()),
            PictureData::Raster(raster) => {
                write!(f, "Raster({} by {})", raster.width(), raster.height())
            }
        }
    }
}

/// Every `<img>` in one content document, resolved against the container.
///
/// Two lists and not one map, because the two halves go to different places:
/// the resolved ones become replaced boxes and then `/XObject`s, and the
/// refused ones become [`crate::ArchiveWarning::ImageNotDrawn`] and nothing
/// else. A document with no `<img>` in it produces an empty one of these and
/// pays nothing.
#[derive(Debug, Default)]
pub struct Pictures {
    /// The ones that will be drawn, in document order.
    pub drawn: Vec<Picture>,
    /// The ones that will not, each with the element it was and why.
    pub refused: Vec<(usize, ImageDefect)>,
}

impl Pictures {
    /// The intrinsic size of the picture an element is, if it is one.
    #[must_use]
    fn intrinsic_of(&self, element: usize) -> Option<(f64, f64)> {
        self.drawn
            .iter()
            .find(|picture| picture.element == element)
            .map(|picture| picture.intrinsic)
    }
}

/// Resolves every `<img>` in a content document against the container it came
/// from.
///
/// # Why the bytes are read here and not at the painter
///
/// A replaced box's size is the picture's size, CSS 2.2 §10.3.2, and the box
/// tree is built before anything is laid out — so the dimensions have to exist
/// before the flow does. Reading the entry is the only way to have them:
/// `<img width>` and `<img height>` are HTML §15.4.2 presentational hints this
/// build does not map (see `docs/features/epub.md`), and the manifest's
/// `media-type` is a claim where the first bytes of an entry are a fact.
///
/// # What is **not** here
///
/// `alt`. HTML §4.8.4.4 makes an unavailable `<img>` represent its alternative
/// text, and laying that out would put characters on the page that the spine's
/// markup does not contain — one page character per refused image with no
/// source character to answer it, which is exactly the quantity
/// `epub_conservation.rs` compares. So a refused `<img>` generates **no box**,
/// which is the other half of §4.8.4.4's own sentence: an element is *"expected
/// to be treated as a replaced element"* only when the image is available.
fn pictures<R: Resources + ?Sized>(
    book: &mut R,
    path: &str,
    dom: &Dom,
    limits: &Limits,
) -> Pictures {
    let mut out = Pictures::default();
    for element in 0..dom.nodes.len() {
        let node = &dom.nodes[element];
        if !node.is_html() || node.name != "img" {
            continue;
        }
        match picture(book, path, element, dom, limits) {
            Ok((intrinsic, data)) => out.drawn.push(Picture {
                element,
                intrinsic,
                data,
            }),
            Err(defect) => out.refused.push((element, defect)),
        }
    }
    out
}

/// One `<img>`, resolved and read, or the reason it was not.
fn picture<R: Resources + ?Sized>(
    book: &mut R,
    path: &str,
    element: usize,
    dom: &Dom,
    limits: &Limits,
) -> Result<((f64, f64), PictureData), ImageDefect> {
    // §5.7's reference, resolved against the document it was written in —
    // `epub::svg`'s own sentence, and the same function.
    let href = dom.nodes[element]
        .attr("src")
        .ok_or(ImageDefect::Unresolved)?
        .to_owned();
    let (_, bytes) = book
        .fetch(path, &href, limits)
        .map_err(|_| ImageDefect::Unresolved)?;
    picture_data(bytes)
}

/// A picture's bytes, read into its intrinsic size and the shape the writer
/// takes — for an `<img>`, and for a `background-image`, which is the same
/// raster reached through a stylesheet.
///
/// # Errors
/// The [`ImageDefect`] that says why the bytes are no picture this build draws.
pub(crate) fn picture_data(bytes: Vec<u8>) -> Result<((f64, f64), PictureData), ImageDefect> {
    // Classification by magic and never by extension, `cbz::image_format`'s
    // own rule: a `.jpg` that is a PNG is routine, and an extension is a claim
    // where the first bytes of a file are a fact.
    match image_format(&bytes).ok_or(ImageDefect::Unknown)? {
        ImageFormat::Jpeg => {
            // The same reader `add_image` uses, so the box and the `/Width`
            // cannot disagree.
            let (width, height, _) =
                tinker_pdf_cos::jpeg_shape(&bytes).ok_or(ImageDefect::Undecodable)?;
            if width == 0 || height == 0 {
                return Err(ImageDefect::Undecodable);
            }
            Ok((
                (f64::from(width), f64::from(height)),
                PictureData::Jpeg(bytes),
            ))
        }
        ImageFormat::Png => {
            // The caller's own entry ceiling, which is `cbz.rs`'s posture
            // exactly: a raster bigger than the biggest file the container may
            // hold is not a picture, and the number is the *host's* rather than
            // the decoder's so a host that lowered it can tell its decision
            // from `MAX_PNG_SAMPLES`.
            let png = png_image(&bytes, &FilterLimits::new(zip_limits::MAX_ZIP_ENTRY_BYTES))
                .map_err(|_| ImageDefect::Undecodable)?;
            if png.width() == 0 || png.height() == 0 {
                return Err(ImageDefect::Undecodable);
            }
            Ok((
                (f64::from(png.width()), f64::from(png.height())),
                PictureData::Png(Box::new(png)),
            ))
        }
        // A core media type (§3.2) with no pass-through: decoded under the
        // same ceiling, and its first image is the picture.
        ImageFormat::Gif => {
            let gif = gif_image(&bytes, &FilterLimits::new(zip_limits::MAX_ZIP_ENTRY_BYTES))
                .map_err(|_| ImageDefect::Undecodable)?;
            Ok((
                (f64::from(gif.width()), f64::from(gif.height())),
                PictureData::Raster(Box::new(gif)),
            ))
        }
        // The fourth core media type, the same way, lossless or lossy.
        ImageFormat::WebP => {
            let webp = webp_image(&bytes, &FilterLimits::new(zip_limits::MAX_ZIP_ENTRY_BYTES))
                .map_err(|_| ImageDefect::Undecodable)?;
            Ok((
                (f64::from(webp.width()), f64::from(webp.height())),
                PictureData::Raster(Box::new(webp)),
            ))
        }
        other => Err(ImageDefect::UnsupportedFormat(other)),
    }
}

/// Turns the element tree and its computed styles into a box tree.
///
/// Public so that a test can build the same tree from a cascade it controls:
/// there is no way to open a book **without** the user-agent stylesheet, which
/// is the point of committing it, so the only way to assert what its absence
/// costs is to cascade twice and lay both out through this.
///
/// # Rooted at the document element, and not at `<body>`
///
/// Starting at `<body>` is the obvious choice and it is **wrong in a way no
/// output shows**: it removes `<head>` from the tree by position rather than by
/// `display`, so `head { display: none }` in the user-agent sheet becomes a
/// rule that changes nothing and every test of the sheet's absence gets the
/// right answer for the wrong reason. Milestone 8 wrote it that way first and
/// the test that found it is
/// `without_the_ua_stylesheet_a_book_has_no_block_structure_at_all`, which
/// asserts that the `<title>` and the `<style>` **do** reach the page once the
/// sheet is removed — an assertion that cannot fail if the subtree was never
/// in the tree.
///
/// A document with no element at all lays out as an empty block, which is a
/// page rather than a refusal.
#[must_use]
pub fn box_tree(dom: &Dom, styles: &StyleTree, pictures: &Pictures) -> BoxNode {
    let Some(root) = dom.root else {
        return BoxNode::element(ComputedStyle::initial(), Vec::new());
    };
    let mut tree = build(dom, styles, pictures, root);
    propagate_overflow(dom, root, &mut tree);
    tree
}

/// `css-overflow-3` §3.3: the root element's `overflow` — or, where that is
/// `visible` and the root is HTML's `<html>`, its `<body>`'s — **belongs to
/// the viewport**, and *"the element from which the value is propagated must
/// then have a used overflow value of `visible`"*.
///
/// Here the viewport is the page, which clips already, so the value lands
/// nowhere; what matters is the second sentence. Without it a book's `body {
/// overflow-x: hidden }` — a web habit, written against horizontal scrolling —
/// would make `<body>` a scroll container: its margin would stop collapsing
/// with its first child's, and every chapter would start lower by the
/// smaller of the two.
fn propagate_overflow(dom: &Dom, root: usize, tree: &mut BoxNode) {
    let open = |style: &ComputedStyle| {
        style.overflow_x == Overflow::Visible && style.overflow_y == Overflow::Visible
    };
    if tree.style.display == Display::None {
        return;
    }
    let make_visible = |style: &mut ComputedStyle| {
        style.overflow_x = Overflow::Visible;
        style.overflow_y = Overflow::Visible;
    };
    if !open(&tree.style) {
        make_visible(&mut tree.style);
        return;
    }
    let node = &dom.nodes[root];
    if !(node.is_html() && node.name == "html") {
        return;
    }
    let Content::Children(children) = &mut tree.content else {
        return;
    };
    // *"The first such child element"*: a `<body>` whose `display` is not
    // `none`.
    let body = children.iter_mut().find(|child| {
        child.style.display != Display::None
            && child.anchor.is_some_and(|at| {
                dom.nodes
                    .get(at as usize)
                    .is_some_and(|node| node.is_html() && node.name == "body")
            })
    });
    if let Some(body) = body {
        make_visible(&mut body.style);
    }
}

fn build(dom: &Dom, styles: &StyleTree, pictures: &Pictures, at: usize) -> BoxNode {
    let mut out = Vec::with_capacity(1);
    let mut build = Build {
        dom,
        styles,
        pictures,
        lettered: HashSet::new(),
    };
    build_into(&mut out, &mut build, at);
    out.pop()
        .unwrap_or_else(|| BoxNode::element(ComputedStyle::initial(), Vec::new()))
}

/// What [`build_into`] reads, and the one record it keeps besides the tree.
struct Build<'a> {
    dom: &'a Dom,
    styles: &'a StyleTree,
    pictures: &'a Pictures,
    /// The elements whose own `::first-letter` search has ended — it found the
    /// letter, or found that the first line has none — so that an ancestor's
    /// search stops at their box rather than wrapping the same letter again.
    /// See [`first_letter`].
    lettered: HashSet<u32>,
}

/// One element's box, pushed onto `out`.
///
/// **The recursion of the box tree, and its frame is kept small on purpose.**
/// It runs once per level of the document, and an unoptimised build gives
/// every temporary of the function its own stack slot: a computed style is a
/// kilobyte and a box node more, and the version of this that built its text
/// boxes, generated boxes and its own node inline held several of each per
/// level. `hostile_input.rs`'s two hundred nested `<em>` — Markdown's own
/// nesting cap — then overflowed a two-megabyte thread once `border-radius`,
/// the shadows and the transform had grown the computed style. So every box
/// is made in a helper of its own ([`push_text`], [`push_pseudo`],
/// [`push_element`], [`push_replaced`]), whose frame is gone before the next
/// level begins, and this one and [`build_with`] hold references and the
/// child list.
fn build_into(out: &mut Vec<BoxNode>, build: &mut Build<'_>, at: usize) {
    let styles = build.styles;
    match styles.styles.get(at) {
        Some(style) => build_with(out, build, at, style),
        None => build_unstyled(out, build, at),
    }
}

/// [`build_into`] for an element the cascade gave no style, which a tree the
/// cascade built does not have: the initial style, in a frame of its own so
/// the recursion's carries no second computed style.
#[inline(never)]
fn build_unstyled(out: &mut Vec<BoxNode>, build: &mut Build<'_>, at: usize) {
    let initial = ComputedStyle::initial();
    build_with(out, build, at, &initial);
}

/// [`build_into`] with the element's style in hand.
fn build_with(out: &mut Vec<BoxNode>, build: &mut Build<'_>, at: usize, style: &ComputedStyle) {
    let (dom, styles) = (build.dom, build.styles);
    let Some(node) = dom.nodes.get(at) else {
        return;
    };
    let anchor = u32::try_from(at).unwrap_or(u32::MAX);
    // CSS 2.2 §3.1's replaced element, and the only one this build has. It is
    // decided here rather than by `display`, because *being replaced* is a
    // property of the element and not of a property: `img { display: block }`
    // is a block-level picture and `img { display: inline }` is an inline one,
    // and a build that keyed on `display` would have to say which of them is
    // the picture.
    //
    // An `<img>` whose picture did not resolve is **not** one: HTML §4.8.4.4
    // makes an element *"expected to be treated as a replaced element"* only
    // when the image is available, so it falls through to the branch below and
    // becomes what it is — an empty inline element, generating an empty box and
    // no ink. See [`pictures`].
    if let Some(size) = build.pictures.intrinsic_of(at) {
        push_replaced(out, style, size, anchor);
        return;
    }
    let mut children = Vec::with_capacity(node.children.len());
    // CSS 2.1 §12.1: `::before` is the first child of its originating element
    // and `::after` is the last. **Inside**, not beside — a `::before` on a
    // `<p>` is inside the paragraph's borders and shares its line box, and a
    // build that put the box next to the element would give it the parent's
    // width and its own line.
    if let Some(generated) = styles.pseudo(at, PseudoElement::Before) {
        push_pseudo(&mut children, generated, anchor);
    }
    for child in &node.children {
        match child {
            Child::Element(index) => build_into(&mut children, build, *index),
            Child::Text(text) => push_text(&mut children, style, text, anchor),
        }
    }
    if let Some(generated) = styles.pseudo(at, PseudoElement::After) {
        push_pseudo(&mut children, generated, anchor);
    }
    // `css-pseudo-4` §2.2: a block container's `::first-letter` is the first
    // typographic letter unit of its first formatted line — which, in a box
    // tree built before line breaking, is the first letter of its first
    // in-flow text, through inline boxes and into a first child block.
    if let Some(letter) = styles.pseudo(at, PseudoElement::FirstLetter) {
        if matches!(
            style.display,
            Display::Block
                | Display::ListItem
                | Display::InlineBlock
                | Display::TableCell
                | Display::TableCaption
        ) && first_letter(&mut children, &letter.style, &build.lettered, 0)
            != LetterSearch::Continue
        {
            build.lettered.insert(anchor);
        }
    }
    push_element(out, style, children, node, anchor, styles.marker(at));
}

/// A replaced box, for [`build_into`].
#[inline(never)]
fn push_replaced(
    out: &mut Vec<BoxNode>,
    style: &ComputedStyle,
    (width, height): (f64, f64),
    anchor: u32,
) {
    out.push(
        BoxNode::replaced(style.clone(), Intrinsic::raster(width, height)).with_anchor(anchor),
    );
}

/// A text box in its element's inline style, for [`build_into`].
#[inline(never)]
fn push_text(out: &mut Vec<BoxNode>, style: &ComputedStyle, text: &str, anchor: u32) {
    out.push(BoxNode::text(inline_box(style), text.to_owned()).with_anchor(anchor));
}

/// A generated box, for [`build_into`].
#[inline(never)]
fn push_pseudo(out: &mut Vec<BoxNode>, generated: &PseudoBox, anchor: u32) {
    out.push(pseudo_box(generated, anchor));
}

/// An element's own box round its children, for [`build_into`].
#[inline(never)]
fn push_element(
    out: &mut Vec<BoxNode>,
    style: &ComputedStyle,
    children: Vec<BoxNode>,
    node: &Node,
    anchor: u32,
    marker: Option<&str>,
) {
    // An element with no children at all still has to be a `Children(vec![])`
    // rather than a `Text("")`: an empty `<p>` generates a block box with its
    // own margins, and one carrying an empty string would be an inline box
    // with none.
    out.push(BoxNode {
        style: style.clone(),
        content: Content::Children(children),
        anchor: Some(anchor),
        span: cell_span(node, &style.display),
        // `css-lists-3` §4's `list-item` counter, which the cascade walked over
        // the whole document: an `<ol start>`, an `<li value>` and every item
        // between are in this number, and the layout crate sees one box.
        marker: marker.map(str::to_owned),
    });
}

/// Where the search for a `::first-letter` stands after a list of boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LetterSearch {
    /// The letter was found and wrapped — by this search, or already by a
    /// nearer block container's own `::first-letter`.
    Found,
    /// The first formatted line begins with something that has no first
    /// letter — a picture, an inline-block, a table, punctuation and then
    /// nothing — so there is none, and the search ends.
    Stop,
    /// Nothing in these boxes yet: the line has not begun.
    Continue,
}

/// Wraps the first typographic letter unit in `children` in a box carrying
/// `letter`, `css-pseudo-4` §2.2: the text's own leading white space stays
/// outside it, the punctuation before and after the letter goes inside it
/// with the letter's combining marks, and the box anchors to the text's
/// element, so extraction and the structure tree read the same characters in
/// the same order. Inline boxes are searched through, a first in-flow block
/// child is searched into (its first line is the container's), and floats and
/// absolutely positioned boxes are passed over as out of the line.
///
/// **A block that searched for its own first letter is not searched again.**
/// The box tree is built innermost first, so where a container and the block
/// inside it both have a `::first-letter`, the inner one's search has already
/// run; `lettered` holds every element whose search ended, and this one stops
/// at such a block's box. The letter keeps the inner box only, which is the
/// one CSS 2.1 §5.12.2's fictional tag sequence puts innermost and whose
/// declarations the letter therefore shows. Without it every level wrapped the
/// same letter again — a box tree twice the document's depth, refused past
/// `MAX_BOX_DEPTH`, and a search two frames a level deep.
///
/// **What is approximated, and stated.** The box inherits from the
/// originating block and not from the inline box the letter is inside, so an
/// `<em>` round the first word does not italicise the drop cap; a letter that
/// a leading quotation mark and an element boundary separate (`“<em>T`) is
/// not found; `display` other than `inline` is read as `inline` unless the
/// box floats, as §2.2 says; and of nested containers' `::first-letter`s only
/// the innermost makes a box, so an outer one's border or background round
/// the inner one's is not drawn.
///
/// **The frame is small on purpose**, as [`build_into`]'s is: this recurses
/// once per level it searches through, and the box it makes is made in
/// [`wrap_letter`], whose computed styles are gone before it returns.
fn first_letter(
    children: &mut Vec<BoxNode>,
    letter: &ComputedStyle,
    lettered: &HashSet<u32>,
    depth: usize,
) -> LetterSearch {
    if depth > tinker_pdf_layout::limits::MAX_BOX_DEPTH {
        return LetterSearch::Stop;
    }
    let mut at = 0;
    while at < children.len() {
        let child = &mut children[at];
        let style = &child.style;
        if style.display == Display::None
            || style.float != Float::None
            || matches!(style.position, Position::Absolute | Position::Fixed)
        {
            at += 1;
            continue;
        }
        let display = style.display;
        match &mut child.content {
            Content::Replaced(_) => return LetterSearch::Stop,
            Content::Text(text) => {
                let Some(unit) = letter_unit(text) else {
                    if text.chars().all(char::is_whitespace) {
                        at += 1;
                        continue;
                    }
                    return LetterSearch::Stop;
                };
                wrap_letter(children, at, unit, letter);
                return LetterSearch::Found;
            }
            Content::Children(inner) => match display {
                Display::Inline => match first_letter(inner, letter, lettered, depth + 1) {
                    LetterSearch::Continue => at += 1,
                    done => return done,
                },
                Display::Block | Display::ListItem => {
                    // An element's own box carries its anchor, and nothing
                    // outside that box does — its text and generated boxes are
                    // inside it — so the anchor names the block whose search
                    // ended.
                    if child
                        .anchor
                        .is_some_and(|anchor| lettered.contains(&anchor))
                    {
                        return LetterSearch::Found;
                    }
                    match first_letter(inner, letter, lettered, depth + 1) {
                        // An empty block has no line; the first line is the next
                        // box's.
                        LetterSearch::Continue => at += 1,
                        done => return done,
                    }
                }
                _ => return LetterSearch::Stop,
            },
        }
    }
    LetterSearch::Continue
}

/// The text box at `children[at]` cut round `start..end`, its first letter
/// unit, which goes into a box of its own in `letter`'s style, for
/// [`first_letter`].
#[inline(never)]
fn wrap_letter(
    children: &mut Vec<BoxNode>,
    at: usize,
    (start, end): (usize, usize),
    letter: &ComputedStyle,
) {
    let Some(child) = children.get_mut(at) else {
        return;
    };
    let Content::Text(text) = &mut child.content else {
        return;
    };
    // `start..end` is `letter_unit`'s answer for this very text, so all three
    // are character boundaries inside it and none of these is `None`.
    let (Some(before), Some(unit), Some(after)) =
        (text.get(..start), text.get(start..end), text.get(end..))
    else {
        return;
    };
    let (before, unit, after) = (before.to_owned(), unit.to_owned(), after.to_owned());
    let anchor = child.anchor;
    let text_style = child.style.clone();
    let mut style = letter.clone();
    if style.float == Float::None {
        style.display = Display::Inline;
    }
    let mut replacement = Vec::with_capacity(3);
    if !before.is_empty() {
        replacement.push(text_node(text_style.clone(), &before, anchor));
    }
    let inner = text_node(inline_box(&style), &unit, anchor);
    replacement.push(BoxNode {
        style,
        content: Content::Children(vec![inner]),
        anchor,
        span: CellSpan::ONE,
        marker: None,
    });
    if !after.is_empty() {
        replacement.push(text_node(text_style, &after, anchor));
    }
    children.splice(at..=at, replacement);
}

/// A text box anchored where the text it was cut from was.
fn text_node(style: ComputedStyle, text: &str, anchor: Option<u32>) -> BoxNode {
    let node = BoxNode::text(style, text);
    match anchor {
        Some(anchor) => node.with_anchor(anchor),
        None => node,
    }
}

/// The byte range of a text's first typographic letter unit, `css-pseudo-4`
/// §2.2: any punctuation before the first letter or number, the letter, its
/// combining marks, and any punctuation after it — `“A”` whole, `A.` with its
/// full stop. Leading white space is before the range. `None` where the text
/// holds no letter or number before a space or its end.
fn letter_unit(text: &str) -> Option<(usize, usize)> {
    use tinker_pdf_layout::unicode::{is_combining, is_letter_or_number, is_punctuation_or_symbol};
    let punctuation = |c: char| is_punctuation_or_symbol(c) && !is_letter_or_number(c);
    let mut chars = text
        .char_indices()
        .skip_while(|(_, c)| c.is_whitespace())
        .peekable();
    let start = chars.peek()?.0;
    while chars.peek().is_some_and(|(_, c)| punctuation(*c)) {
        chars.next();
    }
    let (_, first) = chars.next()?;
    if !is_letter_or_number(first) {
        return None;
    }
    while chars.peek().is_some_and(|(_, c)| is_combining(*c)) {
        chars.next();
    }
    while chars.peek().is_some_and(|(_, c)| punctuation(*c)) {
        chars.next();
    }
    let end = chars.peek().map_or(text.len(), |(at, _)| *at);
    Some((start, end))
}

/// One `::before` or `::after` box, as `epub::read` builds every other box.
///
/// # It anchors to the originating element, and that is a decision
///
/// The anchor is what text extraction and the conservation harness use to say
/// *where on the page this text came from*, and a generated box has no source
/// node to point at — it is text the document does not contain. Anchoring it to
/// the element that generated it is the only honest answer available: it is
/// where a reader would say the text is, and it keeps every downstream
/// consumer's "which element is this" question answerable.
///
/// It does **not** make the text conserved. `epub_conservation.rs` compares the
/// page against the source markup and counts anything on the page that is not
/// in the source as `extra`; generated content is exactly that, by definition,
/// and the harness is right to say so. No committed book generates any, so no
/// recorded figure moves — `epub_pseudo.rs` is where that interaction is pinned
/// rather than left to be discovered by the first book that uses one.
///
/// # Two boxes and not one
///
/// The generated box wraps a text box, which is the same shape `build` gives an
/// element with one text child. That is what lets `display` decide: the outer
/// box carries the pseudo-element's own computed style, so `content: "x";
/// display: block` is a block box and the default `display: inline` is not, and
/// neither case is special-cased here.
fn pseudo_box(generated: &PseudoBox, anchor: u32) -> BoxNode {
    let inner =
        BoxNode::text(inline_box(&generated.style), generated.text.clone()).with_anchor(anchor);
    BoxNode {
        style: generated.style.clone(),
        content: Content::Children(vec![inner]),
        anchor: Some(anchor),
        // A generated box is never a table cell: `cell_span` reads `colspan`
        // and `rowspan` off a source element, and this box has none.
        span: CellSpan::ONE,
        marker: None,
    }
}

/// HTML's `colspan`, `rowspan` and `span`, CSS 2.2 §17.5.
///
/// **This is the one thing a table needs that no stylesheet can say.** There is
/// no CSS property behind any of the three, so the cascade cannot carry them
/// and `tinker_pdf_layout::style::consume`'s compile-time device — which is
/// about computed styles — has nothing to say about them. They arrive on
/// [`BoxNode::span`] instead, from here, which is the file that already knows
/// what an XHTML attribute is.
///
/// The clamps are HTML's own and each is a different number for a different
/// reason. `colspan` is 1 to 1 000 and a `colspan="0"` is *one* column, because
/// HTML 4's *"spans every column"* reading was dropped. `rowspan` is 0 to
/// 65 534 and **zero survives**: it is HTML's *"to the end of the row group"*,
/// which is the only one of the three where zero is a value rather than a
/// mistake, and clamping it here would turn a real book's `rowspan="0"` into a
/// one-row cell with the rest of the column shifted up.
fn cell_span(node: &Node, display: &Display) -> CellSpan {
    match display {
        Display::TableCell => CellSpan {
            columns: attribute_count(node.attr("colspan"), 1, 1_000, 1),
            rows: attribute_count(node.attr("rowspan"), 0, 65_534, 1),
        },
        // `<col span>` and `<colgroup span>`, which say how many columns the
        // box describes rather than how many a cell occupies.
        Display::TableColumn | Display::TableColumnGroup => CellSpan {
            columns: attribute_count(node.attr("span"), 1, 1_000, 1),
            rows: 1,
        },
        _ => CellSpan::ONE,
    }
}

/// HTML's *"rules for parsing non-negative integers"*, clamped.
///
/// A value with trailing rubbish — `"3 "`, `"2x"` — is HTML's leading-digits
/// rule and yields the digits; a value with no leading digit at all is the
/// default. A build that used `str::parse` alone would give `colspan="2 "` one
/// column, which is a table with a hole in it.
fn attribute_count(value: Option<&str>, low: u32, high: u32, default: u32) -> u32 {
    let Some(raw) = value else {
        return default;
    };
    let digits: String = raw
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    match digits.parse::<u32>() {
        Ok(count) => count.clamp(low, high),
        Err(_) => default,
    }
}
