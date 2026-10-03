//! What the geometric inferences read off a page: the text device's page,
//! read once.
//!
//! [`crate::reading_order`] wants the [`TextPage`] that [`crate::Page::text`]
//! builds — one assembler, or an inferred view drifts from search and
//! selection — and, with the structure tree hidden, the page as an untagged
//! reader would see it.
//!
//! What runs is **one interpretation into a tee**: [`Observer`] is a
//! [`Device`] that hands the four calls [`TextDevice`] implements —
//! `begin_marked_content`, `end_marked_content`, `show_glyph` and `end_text` —
//! to a `TextDevice` unchanged. It answers the interpreter's three questions
//! (`begin_form`, `begin_group`, `begin_soft_mask`) with the trait's defaults,
//! which are `TextDevice`'s own, so the interpreter does exactly what it does
//! for [`crate::Page::text`] and the page it builds is that page, character
//! for character. `crates/tinker-pdf/tests/reading_order.rs` holds the
//! equality over every committed fixture rather than leaving it to this
//! argument.
//!
//! # The tree hidden
//!
//! [`Observed::read`] with `keep_artifacts` set is the page **as an untagged
//! reader would see it**: an `/Artifact` scope (14.8.2.2) is handed to the text
//! device under another tag, so its text is extracted rather than dropped.
//! An artifact is part of a file's tagging — the producer saying "this running
//! head is not content" — and an inference measured with the tree hidden has
//! to find the running head without being told, which it cannot do if the
//! head was never extracted.

use tinker_pdf_content::{
    interpret, Device, Glyph, GraphicsState, MarkedProps, Matrix, TextDevice, TextPage, TextWarning,
};
use tinker_pdf_cos::pages as cos_pages;

use crate::{resources, text_order, Page};

/// What one interpretation of a page left behind.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observed {
    /// The page's text, in logical order (ruling 14) — [`Page::text`]'s,
    /// unless artifacts were kept.
    pub text: TextPage,
}

impl Observed {
    /// Interprets `page` once.
    ///
    /// `keep_artifacts` reads `/Artifact` scopes as content; see the module
    /// documentation.
    pub(crate) fn read(page: &Page, keep_artifacts: bool) -> Observed {
        let content = cos_pages::content_bytes(&page.doc, &page.inner);
        // `Page::text`'s own resources: no glyph outlines are needed, so no
        // provider is consulted.
        let resources = resources::PageResources::new(&page.doc, &page.inner, None);
        let mut observer = Observer::new(keep_artifacts);
        interpret(&content, Matrix::IDENTITY, &mut observer, &resources);
        for name in resources.missing_fonts() {
            observer.text.warn(TextWarning::UnknownFont { name });
        }
        let mut text = observer.text.finish();
        text_order::into_logical_order(&mut text);
        Observed { text }
    }
}

/// The tee: a [`TextDevice`], and what it is told.
struct Observer {
    text: TextDevice,
    keep_artifacts: bool,
}

impl Observer {
    fn new(keep_artifacts: bool) -> Observer {
        Observer {
            text: TextDevice::new(),
            keep_artifacts,
        }
    }
}

impl Device for Observer {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.text.show_glyph(glyph, state);
    }

    fn end_text(&mut self) {
        self.text.end_text();
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        // The text device drops what an `/Artifact` scope draws (14.8.2.2);
        // any other tag it reads as content. See the module documentation.
        let tag: &[u8] = if self.keep_artifacts && tag == b"Artifact" {
            b"Artifact (read as content)"
        } else {
            tag
        };
        self.text
            .begin_marked_content(tag, visible, hidden_layer, props);
    }

    fn end_marked_content(&mut self) {
        self.text.end_marked_content();
    }
}
