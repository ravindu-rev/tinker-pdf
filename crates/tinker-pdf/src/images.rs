//! The images a page draws, with their samples as decoded and the colour
//! space they are in, behind [`Page::images`].
//!
//! Feature documentation: `docs/features/content-and-text.md`.
//!
//! # Why neither existing type is this one
//!
//! The renderer's `DecodedImage` is RGB by the time it exists: the colour
//! conversion is the last step of the decode, so a CMYK scan, an indexed
//! palette and a spot-colour separation all arrive as three bytes a pixel and
//! the space they were in is gone. The writer's `ImageData` is the other
//! direction — what a caller hands the builder — and carries no placement and
//! no reference. [`PageImage`] is the read side's own: the samples **before**
//! any conversion, the space they were written in, and where the page put
//! them.
//!
//! # One decode, not two
//!
//! The samples come from the same functions the renderer's decode calls —
//! `stream_samples`, `jpeg_samples`, `jpx_samples` and `inline_samples` in
//! `resources.rs`, each split out of the renderer's path for this — so a
//! filter chain, a fax's polarity or a JBIG2 inversion is one rule read in
//! one place. What the renderer does *after* them (`/Decode`, the colour
//! conversion, masks) is not done here: those are reported, not applied.
//!
//! # What is walked
//!
//! The page's content and every form XObject it draws, through the same
//! interpreter the renderer and the text extractor use, with a device that
//! follows form scopes the way the renderer's does. An image drawn by a
//! tiling pattern's cell, a soft-mask group or an annotation's appearance is
//! not a drawing of *this page's content* and is not listed.

use std::collections::BTreeMap;
use std::sync::Arc;

use tinker_pdf_content::{interpret, Device, FontSource, GraphicsState, ImageRef, Matrix};
use tinker_pdf_cos::{pages as cos_pages, ObjRef};

use crate::resources::PageResources;
use crate::Page;

/// One image a page draws (8.9), with its samples and the space they are in.
///
/// **The samples are not converted.** They are the image's samples as its
/// filters leave them: rows from the top, each row padded to a whole byte,
/// [`Self::components`] values a pixel interleaved, [`Self::bits_per_component`]
/// bits a value, sixteen-bit values big-endian — 8.9.3's layout, which is the
/// layout the stream itself has once decoded. `/Decode` is carried in
/// [`Self::decode`] and not applied, and no colour space has been evaluated.
///
/// Three codecs describe themselves rather than the dictionary, and their
/// samples are what the decoder produces, as [`Self::codec`] says:
///
/// - **`/DCTDecode`**: eight bits a component, the frame's own components —
///   YCbCr already turned into RGB and an Adobe-inverted CMYK already turned
///   back into ink values, which are the decoder's to undo;
/// - **`/JPXDecode`**: the codestream's own components at its own precision,
///   8 or 16 bits (8.9.5.4 makes `/BitsPerComponent` meaningless here), with
///   an opacity channel the file carried *not* included;
/// - **`/CCITTFaxDecode` and `/JBIG2Decode`**: one bit a pixel whatever the
///   dictionary said, in PDF's polarity (0 is black under `/DeviceGray`),
///   which for JBIG2 is the inverse of T.88's own.
///
/// A stream shorter than its geometry needs is reported as it is — the
/// samples are then fewer than `width × height` asks for — and one longer is
/// cut to the geometry. An image that could not be decoded at all is still
/// listed, with no samples and [`Self::refused`] saying why (ruling 2); one
/// that decoded only with a leniency says which in [`Self::warnings`]
/// (ruling 10).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PageImage {
    /// The image XObject's reference, or `None` for an inline image (8.9.7).
    pub reference: Option<ObjRef>,
    /// The resource name the page first drew it under, or empty for an
    /// inline image and for a mask.
    pub name: Vec<u8>,
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// Bits a value in [`Self::samples`]: 1, 2, 4, 8 or 16.
    pub bits_per_component: u8,
    /// Values a pixel in [`Self::samples`].
    pub components: u8,
    /// The colour space, as the image states it — `None` for a stencil mask
    /// (8.9.6.2), which has none, and for a JPEG 2000 image whose dictionary
    /// leaves the space to the codestream (8.9.5.4).
    pub color_space: Option<ImageSpace>,
    /// `/Decode` as written, one `(min, max)` a component, or empty when the
    /// dictionary has none (8.9.5.2). Not applied to the samples.
    pub decode: Vec<(f64, f64)>,
    /// Whether this is a stencil mask, `/ImageMask true` (8.9.6.2): one bit a
    /// pixel, painted in the fill colour where the sample is 0.
    pub stencil: bool,
    /// The samples, laid out as the type's documentation says.
    pub samples: Vec<u8>,
    /// Which decoder produced the samples.
    pub codec: SampleCodec,
    /// `/Mask`: a colour-key range per component (8.9.6.4) or a stencil mask
    /// image (8.9.6.3).
    pub mask: Option<ImageMask>,
    /// `/SMask`: the soft mask image whose samples are this one's opacity
    /// (11.6.5.3).
    pub soft_mask: Option<Box<PageImage>>,
    /// Where the page draws it: the current transformation matrix at each
    /// `Do` or `BI`, as `[a b c d e f]`, mapping the image's unit square into
    /// the page's default user space (8.9.5.2). One entry per drawing, in
    /// content order, so an image drawn twice is listed once with two.
    pub placements: Vec<[f64; 6]>,
    /// Why the samples could not be decoded, when they could not.
    pub refused: Option<String>,
    /// What the decoder tolerated to produce [`Self::samples`] — a fax row
    /// that did not decode and was replicated from the one above, a JBIG2
    /// segment skipped, a JPEG 2000 codestream cut short — each named as
    /// [`crate::RenderWarning::DamagedImage`]'s `reason` names it when the
    /// page is rendered, and each once. Empty for a clean decode (ruling 10):
    /// an image decoded with a leniency is listed with its samples, and this
    /// is what tells it from one that needed none. A mask's own warnings are
    /// on the mask.
    pub warnings: Vec<String>,
}

/// Which decoder produced a [`PageImage`]'s samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SampleCodec {
    /// The stream's filters, whatever they were, produced the samples
    /// directly — flate, LZW, run-length, ASCII, or none.
    Stream,
    /// `/DCTDecode` (7.4.8).
    Dct,
    /// `/JPXDecode` (7.4.9).
    Jpx,
    /// `/CCITTFaxDecode` (7.4.6).
    CcittFax,
    /// `/JBIG2Decode` (7.4.7).
    Jbig2,
}

/// A colour space as an image states it (8.6), with what an extractor needs
/// to interpret its samples — the names, the profile, the palette — and
/// nothing evaluated.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ImageSpace {
    /// `/DeviceGray`.
    DeviceGray,
    /// `/DeviceRGB`.
    DeviceRgb,
    /// `/DeviceCMYK`.
    DeviceCmyk,
    /// `[/CalGray << ... >>]` (8.6.5.2).
    CalGray {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Gamma`.
        gamma: f64,
    },
    /// `[/CalRGB << ... >>]` (8.6.5.3).
    CalRgb {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Gamma`, one per component.
        gamma: [f64; 3],
        /// `/Matrix`, as Table 65 writes it, column by column.
        matrix: [f64; 9],
    },
    /// `[/Lab << ... >>]` (8.6.5.4).
    Lab {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Range`: `[amin amax bmin bmax]`.
        range: [f64; 4],
    },
    /// `[/ICCBased stream]` (8.6.5.5): the profile's bytes, as embedded.
    Icc {
        /// `/N`.
        components: u8,
        /// The profile, decoded from its stream; empty when the stream would
        /// not decode.
        profile: Vec<u8>,
        /// `/Alternate`, when the stream names one.
        alternate: Option<Box<ImageSpace>>,
    },
    /// `[/Indexed base hival lookup]` (8.6.6.3): each sample an index into
    /// `lookup`, which holds `base`'s components for each of `high + 1`
    /// entries.
    Indexed {
        /// The space the table's entries are in.
        base: Box<ImageSpace>,
        /// `hival`, the largest index.
        high: u8,
        /// The table, as stored.
        lookup: Vec<u8>,
    },
    /// `[/Separation name alternate tint]` (8.6.6.4): each sample one tint of
    /// the named colorant.
    Separation {
        /// The colorant's name.
        colorant: Vec<u8>,
        /// The space the tint transform maps into.
        alternate: Box<ImageSpace>,
    },
    /// `[/DeviceN [names] alternate tint ...]` (8.6.6.5): each sample one tint
    /// per colorant, in this order.
    DeviceN {
        /// The colorants' names.
        colorants: Vec<Vec<u8>>,
        /// The space the tint transform maps into.
        alternate: Box<ImageSpace>,
    },
    /// A space this reader could not describe: the family name as written,
    /// or empty when there was not even a name.
    Unreadable {
        /// The name the space was given.
        family: Vec<u8>,
    },
}

impl ImageSpace {
    /// Colour components a sample of this space has, where the space says.
    ///
    /// One for `/Indexed`, whose sample is an index whatever its base.
    #[must_use]
    pub fn components(&self) -> Option<u8> {
        match self {
            ImageSpace::DeviceGray
            | ImageSpace::CalGray { .. }
            | ImageSpace::Indexed { .. }
            | ImageSpace::Separation { .. } => Some(1),
            ImageSpace::DeviceRgb | ImageSpace::CalRgb { .. } | ImageSpace::Lab { .. } => Some(3),
            ImageSpace::DeviceCmyk => Some(4),
            ImageSpace::Icc { components, .. } => Some(*components),
            ImageSpace::DeviceN { colorants, .. } => u8::try_from(colorants.len()).ok(),
            ImageSpace::Unreadable { .. } => None,
        }
    }
}

/// An image's `/Mask` (8.9.6).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ImageMask {
    /// 8.9.6.4 colour-key masking: an inclusive `(min, max)` range of raw
    /// sample values per component. A pixel inside every range is not
    /// painted.
    ColorKey(Vec<(u32, u32)>),
    /// 8.9.6.3: a stencil mask image, painted where its samples are 0.
    Stencil(Box<PageImage>),
}

/// The device that collects images as the interpreter reaches them.
///
/// It keeps its own resource scope and swaps it at a form's `begin_form` and
/// `end_form`, as the renderer's device does, because an image named inside a
/// form is named in the form's `/Resources` and not the page's.
struct Collector {
    scope: Arc<PageResources>,
    outer: Vec<Option<Arc<PageResources>>>,
    images: Vec<PageImage>,
    /// Where each XObject already listed sits in `images`, so one drawn twice
    /// gains a placement rather than a second decode.
    listed: BTreeMap<ObjRef, usize>,
}

impl Device for Collector {
    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        let m = state.ctm;
        let placement = [m.a, m.b, m.c, m.d, m.e, m.f];
        if image.inline {
            if let Some(mut found) = self
                .scope
                .extract_inline(&image.inline_dict, &image.inline_data)
            {
                found.placements.push(placement);
                self.images.push(found);
            }
            return;
        }
        let Some((dict, reference)) = self.scope.xobject(&image.name) else {
            return;
        };
        if let Some(found) = self
            .listed
            .get(&reference)
            .and_then(|at| self.images.get_mut(*at))
        {
            found.placements.push(placement);
            return;
        }
        let Some(mut found) = self.scope.extract_image(&dict, reference, &image.name) else {
            return;
        };
        found.placements.push(placement);
        self.listed.insert(reference, self.images.len());
        self.images.push(found);
    }

    fn begin_form(&mut self, _id: u64, name: &[u8]) -> bool {
        let nested = FontSource::form_scope(&*self.scope, name);
        self.outer
            .push(nested.map(|scope| std::mem::replace(&mut self.scope, scope)));
        true
    }

    fn end_form(&mut self, _id: u64) {
        if let Some(Some(outer)) = self.outer.pop() {
            self.scope = outer;
        }
    }
}

impl Page {
    /// The images this page draws, in the order it first draws them, each
    /// with its samples as decoded and the colour space they are in.
    ///
    /// An image XObject drawn several times is listed once with every
    /// placement; each inline image is its own entry. Images inside form
    /// XObjects are included, found through the form's own resources. See
    /// [`PageImage`] for what the samples are and are not.
    ///
    /// An image the page's configuration hides — an `/OC` it is drawn under,
    /// or its own — is listed like any other: the list is of what the page's
    /// content draws, not of what a viewer shows.
    #[must_use]
    pub fn images(&self) -> Vec<PageImage> {
        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        let resources = Arc::new(PageResources::new(&self.doc, &self.inner, None));
        let mut collector = Collector {
            scope: Arc::clone(&resources),
            outer: Vec::new(),
            images: Vec::new(),
            listed: BTreeMap::new(),
        };
        interpret(&content, Matrix::IDENTITY, &mut collector, &*resources);
        collector.images
    }
}
