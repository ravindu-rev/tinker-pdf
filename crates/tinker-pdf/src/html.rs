//! HTML and CSS to PDF as a creation API (tier 5's formats row):
//! [`FromHtml::from_html`], callable as `DocumentBuilder::from_html`.
//!
//! The cascade, the layout engine and the painter were all in the tree and
//! wired only into the EPUB pipeline, so the one way to set a paragraph of HTML
//! onto a page was to pack it into a book. This is the same three passes with
//! the book taken away: the markup is one content document, the stylesheet is
//! an author sheet applied ahead of every sheet the document links, and the
//! page box is the box a reflowable chapter is laid into — so a document made
//! here and the same document as the one chapter of an EPUB are **the same
//! pages**, which `tests/html_creation.rs` holds pixel for pixel.
//!
//! # Why a trait, and why here
//!
//! [`DocumentBuilder`] is `tinker-pdf-cos`'s, and that crate cannot see the
//! layout engine: ruling 8 keeps `tinker-pdf-layout` a leaf with no PDF in it,
//! and the COS writer has no business knowing what CSS is. The join of the two
//! is this facade — the EPUB path already lives here for the same reason — so
//! the constructor is an extension trait the facade implements for the
//! builder. Bring [`FromHtml`] into scope and `DocumentBuilder::from_html`
//! reads as the roadmap row asked for it.
//!
//! # What comes back
//!
//! A builder holding the pages, with every face the layout used registered
//! and the `<title>` as the document's `/Title` — so a caller can set more
//! information, add pages of its own and call `finish` — and an
//! [`HtmlReport`] carrying the same typed warnings a book's report carries
//! ([`ArchiveWarning`]): properties the cascade does not implement, counted by
//! element, pictures that did not reach the page, sheets that did not resolve,
//! characters no face covers. A document the cascade or the layout refuses
//! outright, by one of their caps, is [`HtmlError`] rather than a placeholder
//! page, because a creation call has no page count to keep.
//!
//! References resolve as a loose file's do: RFC 2397's `data:` URLs carry
//! their own bytes, and everything else is asked of the [`Resources`] a caller
//! hands [`FromHtml::from_html_with`] — or is missing, and named, with
//! [`FromHtml::from_html`].

use tinker_pdf_cos::DocumentBuilder;

use crate::cbz::ArchiveWarning;
use crate::epub::read::{NoResources, Resources};
use crate::epub::{
    self, BookCost, BookLayout, BookOptionDefect, Loose, SpineDefect, DEFAULT_FONT_SIZE,
    PAGE_MARGIN,
};
use crate::standalone::DataUrls;

/// The page a document made by [`FromHtml::from_html`] is laid into.
///
/// A size and a margin inside it, in points. The margin is the reading
/// system's half of the box and is **not** the `<body>`'s: HTML's own
/// `body { margin: 8px }` still applies inside it, and a stylesheet that sets
/// `body { margin: 0 }` puts the text at the margin rather than at the edge.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct PageBox {
    /// The page's width and height, in points.
    pub size: (f64, f64),
    /// The margin between the page's edge and the content area, in points.
    pub margin: f64,
    /// The size `1rem` and an unstyled paragraph resolve to, in points.
    pub font_size: f64,
}

impl PageBox {
    /// A page of `width` × `height` points, with
    /// [`crate::epub::PAGE_MARGIN`]'s half inch inside it and a twelve-point
    /// base size — what a book is laid out at.
    #[must_use]
    pub fn new(width: f64, height: f64) -> PageBox {
        PageBox {
            size: (width, height),
            margin: PAGE_MARGIN,
            font_size: DEFAULT_FONT_SIZE,
        }
    }

    /// The same page with a different margin, in points.
    #[must_use]
    pub fn with_margin(mut self, margin: f64) -> PageBox {
        self.margin = margin;
        self
    }

    /// The same page with a different base font size, in points.
    #[must_use]
    pub fn with_font_size(mut self, font_size: f64) -> PageBox {
        self.font_size = font_size;
        self
    }
}

/// What making a document from HTML tolerated, and what it cost.
#[derive(Clone, Debug)]
pub struct HtmlReport {
    warnings: Vec<ArchiveWarning>,
    pages: usize,
    layout: BookLayout,
    margin: f64,
    cost: BookCost,
}

impl HtmlReport {
    /// Everything tolerated, in the order it happened — the vocabulary a
    /// book's [`crate::ArchiveReport`] speaks, because it is the same reader.
    #[must_use]
    pub fn warnings(&self) -> &[ArchiveWarning] {
        &self.warnings
    }

    /// How many pages the markup took.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// The page box and base font size the document was laid out at, after
    /// any number that could not be used was replaced.
    #[must_use]
    pub fn layout(&self) -> BookLayout {
        self.layout
    }

    /// The margin the document was laid out with, after the same check.
    #[must_use]
    pub fn margin(&self) -> f64 {
        self.margin
    }

    /// What the document spent against the cascade's and the layout's caps.
    #[must_use]
    pub fn cost(&self) -> BookCost {
        self.cost
    }
}

/// Why markup did not become a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HtmlError {
    /// The cascade refused the document: one of `tinker-pdf-css`'s caps —
    /// elements, rules, declarations, selector matches — was spent.
    StyleRefused,
    /// Layout or fragmentation refused it: one of `tinker-pdf-layout`'s caps
    /// was spent.
    LayoutRefused,
}

impl core::fmt::Display for HtmlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            HtmlError::StyleRefused => "the cascade refused the document at one of its caps",
            HtmlError::LayoutRefused => "layout refused the document at one of its caps",
        })
    }
}

impl std::error::Error for HtmlError {}

/// Markup and a stylesheet, laid out into pages (tier 5's formats row).
///
/// Implemented for [`DocumentBuilder`], so that with this trait in scope
/// `DocumentBuilder::from_html(markup, stylesheet, page)` is the call.
///
/// ```
/// use tinker_pdf::{DocumentBuilder, FromHtml, PageBox};
///
/// let markup = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Note</title></head>
///   <body><h1>A heading</h1><p>A paragraph.</p></body></html>"#;
/// let (builder, report) =
///     DocumentBuilder::from_html(markup, "h1 { font-size: 20pt }", PageBox::new(612.0, 792.0))?;
/// assert_eq!(report.pages(), 1);
/// let pdf = builder.finish();
/// # let _ = pdf;
/// # Ok::<(), tinker_pdf::HtmlError>(())
/// ```
pub trait FromHtml: Sized {
    /// A builder holding `markup` laid out into pages of `page`, with
    /// `stylesheet` applied as an author sheet ahead of every sheet the
    /// markup links.
    ///
    /// `markup` is read as XML — XHTML, or HTML that is well-formed XML — and
    /// a document that stops being well-formed is laid out as far as it read,
    /// with [`ArchiveWarning::Markup`] saying so. A reference to anything but a
    /// `data:` URL is missing and named; [`FromHtml::from_html_with`] is where
    /// a caller says what one means.
    ///
    /// # Errors
    /// [`HtmlError`]: a cap the cascade or the layout enforces.
    fn from_html(
        markup: impl AsRef<[u8]>,
        stylesheet: &str,
        page: PageBox,
    ) -> Result<(Self, HtmlReport), HtmlError> {
        Self::from_html_with(markup, stylesheet, page, &mut NoResources)
    }

    /// [`FromHtml::from_html`], with `resources` answering every reference the
    /// markup and its sheets make — a `<link href>`, an `@import`, an
    /// `<img src>`, an `@font-face` `url()` — that is not a `data:` URL.
    ///
    /// The markup's own address is the empty string, so a reference it makes
    /// reaches `resources` as it was written, and one made inside a sheet
    /// `resources` handed back is asked against the path that sheet came
    /// back with.
    ///
    /// # Errors
    /// The same as [`FromHtml::from_html`].
    fn from_html_with<R: Resources>(
        markup: impl AsRef<[u8]>,
        stylesheet: &str,
        page: PageBox,
        resources: &mut R,
    ) -> Result<(Self, HtmlReport), HtmlError>;
}

impl FromHtml for DocumentBuilder {
    fn from_html_with<R: Resources>(
        markup: impl AsRef<[u8]>,
        stylesheet: &str,
        page: PageBox,
        resources: &mut R,
    ) -> Result<(DocumentBuilder, HtmlReport), HtmlError> {
        let (layout, mut unusable) = BookLayout::sanitised(page.size, page.font_size);
        // A margin that leaves no content area is the caller's number and not a
        // claim about the markup, so it is replaced and named rather than
        // refused — the reasoning `BookLayout::sanitised` gives for the box.
        let shortest = layout.page.0.min(layout.page.1);
        let margin =
            if page.margin.is_finite() && page.margin >= 0.0 && page.margin * 2.0 < shortest {
                page.margin
            } else {
                unusable.push(BookOptionDefect::Margin);
                PAGE_MARGIN.min(shortest / 4.0)
            };

        let limits = epub::Limits::DEFAULT;
        let dom = epub::read::markup(markup.as_ref(), &limits.xml);
        let mut builder = DocumentBuilder::new();
        if let Some(title) = dom.title() {
            builder.set_info(b"Title", &title);
        }
        let laid = epub::lay_out_one(
            &mut DataUrls(resources),
            &mut builder,
            "",
            Loose::Markup(dom),
            stylesheet,
            margin,
            &limits,
            &layout,
        );
        match laid.defect {
            Some(SpineDefect::NotStyled) => return Err(HtmlError::StyleRefused),
            Some(_) => return Err(HtmlError::LayoutRefused),
            None => {}
        }
        let mut warnings: Vec<ArchiveWarning> = unusable
            .into_iter()
            .map(ArchiveWarning::UnusableOption)
            .collect();
        warnings.extend(laid.warnings);
        let pages = laid.pages.len();
        Ok((
            builder,
            HtmlReport {
                warnings,
                pages,
                layout,
                margin,
                cost: laid.cost,
            },
        ))
    }
}
