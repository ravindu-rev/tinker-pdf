//! A retained page: [`Page::display_list`] interprets a page once, and
//! [`DisplayList::render`] draws it at any scale without interpreting it
//! again.
//!
//! # What is kept, and why that is enough
//!
//! The calls the interpreter made — every fill, stroke, clip, glyph, image,
//! shading, `q`, `Q`, form, group, soft mask and marked-content scope, each
//! with the graphics state it saw — recorded by `tinker-pdf-content`'s
//! recording device, in the space the interpreter runs in. The renderer holds
//! the transform to pixels, not the events, which is what makes one recording
//! drawable at any scale: nothing in it is a pixel.
//!
//! And the page's **resources**, kept alive beside it. A replay draws through
//! the same `Renderer` a direct render does, and that renderer resolves
//! glyph outlines, images, shadings, patterns and form scopes by name as it
//! meets them; the resource object it asks is the display list's own, so
//! what it decodes once — an image, an outline, a nested scope — is decoded
//! once for every render the list is asked for.
//!
//! # Why a replay is the direct render, byte for byte
//!
//! Three of the calls are questions the interpreter acts on (`begin_form`,
//! `begin_group`, `begin_soft_mask`), so a recording is only the renderer's
//! picture if it was made with the renderer's answers. It is:
//! `tinker_pdf_render::DisplayRecorder` asks the renderer's own `Admission`,
//! and the renderer's answers depend on nothing but the content stream — no
//! pixel, no scale and no rectangle — since ruling 5's September 2026 change.
//! Everything else is the same code: a replay goes through
//! [`Page::render_layer_with`], which is [`Page::render`]'s pipeline with the
//! interpretation swapped for [`tinker_pdf_content::replay`].
//! `determinism.rs`'s `a_display_list_replays_every_fingerprinted_page_byte_for_byte`
//! holds it over every fingerprinted page at the fingerprints' scale and three
//! others.
//!
//! Annotations are recorded too, each with the resource scope its appearance
//! resolves in, and replayed only when [`RenderOptions::annotations`] asks —
//! after the content, as `Page::render` draws them.

use std::sync::Arc;

use tinker_pdf_content::{interpret, replay, Event, Matrix};
use tinker_pdf_cos::pages as cos_pages;
use tinker_pdf_render::DisplayRecorder;

use crate::annots::{self, Recorded};
use crate::resources::PageResources;
use crate::{Bitmap, Page, RenderOptions};

/// A page interpreted once, to be drawn as often and at as many scales as a
/// caller likes. See [`Page::display_list`].
///
/// Holds the page (which shares the document), the calls its content made,
/// its annotations' appearances, and the resources a replay resolves names
/// in — so it is as large as the page's drawing, not as large as a bitmap of
/// it, and it does not borrow the [`crate::Document`].
pub struct DisplayList {
    page: Page,
    resources: PageResources,
    content: Vec<Event>,
    annotations: Vec<Recorded>,
}

impl Page {
    /// Interprets the page once and keeps what it drew.
    ///
    /// The returned [`DisplayList`] renders with [`DisplayList::render`] to
    /// the same bitmap [`Page::render`] returns for the same options — the
    /// same pixels, the same size and the same warnings — at any scale,
    /// without tokenizing the content stream again. Worth it for a caller that
    /// draws one page more than once: a viewer zooming, a tiler asking for
    /// many regions, a thumbnail and a full view of the same page.
    ///
    /// Annotations are recorded whatever a later render asks, and drawn only
    /// by a render that asks for them.
    #[must_use]
    pub fn display_list(&self) -> DisplayList {
        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        let resources = PageResources::new(&self.doc, &self.inner, self.fonts.as_ref());
        let mut recorder = DisplayRecorder::new();
        interpret(&content, Matrix::IDENTITY, &mut recorder, &resources);
        let content = recorder.take();
        let annotations =
            annots::record(&self.doc, &self.inner, self.fonts.as_ref(), &mut recorder);
        DisplayList {
            page: self.clone(),
            resources,
            content,
            annotations,
        }
    }
}

impl DisplayList {
    /// Draws the page as [`Page::render`] would with the same `options`.
    ///
    /// Every option applies exactly as it does there — the scale and its
    /// clamp, the region, the format, the anti-aliasing, a transparent or
    /// premultiplied page, cancellation, and whether annotations are drawn —
    /// because this is the same pipeline with the interpretation replaced by
    /// a replay of what the interpretation produced.
    #[must_use]
    pub fn render(&self, options: &RenderOptions) -> Bitmap {
        self.page
            .render_layer_with(options, None, &self.resources, |renderer, _| {
                replay(&self.content, renderer);
                if options.annotations {
                    for annotation in &self.annotations {
                        renderer.push_resources(Arc::clone(&annotation.scope));
                        replay(&annotation.events, renderer);
                        renderer.pop_resources();
                    }
                }
            })
    }

    /// The page the list was recorded from.
    pub(crate) fn page(&self) -> &Page {
        &self.page
    }

    /// The resources a replay resolves names in.
    pub(crate) fn resources(&self) -> &PageResources {
        &self.resources
    }

    /// The page's own calls.
    pub(crate) fn content(&self) -> &[Event] {
        &self.content
    }

    /// Each annotation's resource scope and calls, in `/Annots` order.
    pub(crate) fn annotation_layers(
        &self,
    ) -> impl Iterator<Item = (&Arc<PageResources>, &[Event])> {
        self.annotations
            .iter()
            .map(|annotation| (&annotation.scope, annotation.events.as_slice()))
    }

    /// Which page of its document this is.
    #[must_use]
    pub fn page_index(&self) -> u32 {
        self.page.index()
    }

    /// How many calls the page's content made, not counting its annotations.
    ///
    /// A measure of what the list holds rather than of what it draws: a glyph
    /// is one call and so is a full-page image.
    #[must_use]
    pub fn len(&self) -> usize {
        self.content.len()
    }

    /// Whether the page's content made no calls at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

impl core::fmt::Debug for DisplayList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DisplayList")
            .field("page", &self.page.index())
            .field("calls", &self.content.len())
            .field("annotations", &self.annotations.len())
            .finish()
    }
}
