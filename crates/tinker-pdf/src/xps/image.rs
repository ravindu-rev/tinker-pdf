//! Image parts: 9.1.5's four formats, two of which this engine refuses by name
//! (gap 30, milestone 8).
//!
//! # XPS has no image element
//!
//! Milestone 1 measured every package Windows writes and found the same shape
//! in all of them: a raster arrives as a `<Path>` whose `Fill` is an
//! `<ImageBrush>`, usually behind a `{StaticResource}`. There is no `<Image>`
//! element in ECMA-388 at all. So this module is not "the image reader" — it is
//! the half of [`super::brush`] that needs the *package*, and it exists
//! separately for the reason [`super::font::Fonts`] does, quoted here because
//! the two are the same trade: [`Package::read_part`] hands back a borrow of
//! the package and the painter is already holding one for the markup it walks,
//! so an image resolved *while* drawing would need the page's bytes copied out
//! first, and a fixed page part is the one part of this format whose size the
//! file chooses. A pass over the markup before the walk costs time proportional
//! to the part and **no** memory.
//!
//! # Nothing here decodes a picture
//!
//! Gap 29 built the pass-through this consumes, and its whole argument carries
//! over unchanged: a JPEG's bytes are placed verbatim by `ImageData::Jpeg`, and
//! a non-interlaced PNG's IDAT is zlib-compressed predictor-filtered scanlines,
//! which is what `/FlateDecode` with `/Predictor 15` expects. So a page's peak
//! cost is a multiple of the *part* rather than *w × h × 3*, and a ten-page
//! report with a photograph on every page costs what the package costs.
//!
//! `png_image` is the one door, and it decides for itself which of the two
//! routes a given PNG takes — Adam7 and interleaved alpha cannot pass through.
//! Gap 29 milestone 6 found that "an indexed file decoded instead of passed
//! through" moves a byte hash and three route assertions **while every rendered
//! comparison still passes**, so the tests here assert the *route*, not only the
//! picture.
//!
//! # 13.4.1's resolution, and why it is not decoration
//!
//! An `ImageBrush` states its `Viewbox` in the image's **own** units, and the
//! real package in the design section says
//! `Viewbox="0,0,4.00055837631226,4.00055837631226"` for a 384-pixel PNG. Four
//! units is 384 pixels only if the image is 96 dots to the inch, so the
//! resolution is what makes an `Absolute` viewbox mean anything. 13.4.1 makes 96
//! the default and lets the image state otherwise; a reader that assumed 96 for
//! a 300 dpi scan would draw it at three times its intended size.

use std::collections::{HashMap, HashSet};

use tinker_pdf_cos::{jpeg_shape, png_image, DocumentBuilder, ImageData, PngRoute};
use tinker_pdf_filters::Limits as FilterLimits;
use tinker_pdf_xml::{Event, Source};

use super::markup::Trouble;
use super::opc::{Package, PartName};
use super::{dialect_of, Limits, XpsElementDefect};

/// 13.4.1's default, in dots per inch. One XPS unit is 1/96 inch, so an image
/// at 96 dpi is one pixel to the unit.
const DEFAULT_DPI: f64 = 96.0;

/// What 9.1.5 admits, and what it does not.
///
/// Recognised from the **content type** and from the **magic bytes**, and those
/// are two rules rather than one. 7.2.3.5's content type is what OPC says
/// decides a part's format, and a package may state one that is wrong or state
/// none at all; the magic bytes are what the part actually is. A TIFF that this
/// build cannot draw has to be refused whichever of the two says so, because
/// refusing on one and drawing on the other is how a `.tiff` renamed `.png`
/// reaches a JPEG decoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// ISO/IEC 15948, through gap 29's pass-through.
    Png,
    /// ISO/IEC 10918, placed verbatim.
    Jpeg,
    /// 9.1.5's TIFF. No decoder in this engine, refused by name.
    Tiff,
    /// 9.1.5.1's JPEG XR — the format the specification recommends and nothing
    /// outside Microsoft's stack implements. No decoder here either.
    JpegXr,
}

impl Kind {
    /// From the magic bytes alone.
    fn from_magic(bytes: &[u8]) -> Option<Kind> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(Kind::Png);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Kind::Jpeg);
        }
        // TIFF is either endianness, and the answer to "42" decides which.
        if bytes.starts_with(b"II\x2A\x00") || bytes.starts_with(b"MM\x00\x2A") {
            return Some(Kind::Tiff);
        }
        // JPEG XR: the same two byte orders, then 0xBC and a version.
        if bytes.starts_with(&[0x49, 0x49, 0xBC]) || bytes.starts_with(&[0x4D, 0x4D, 0xBC]) {
            return Some(Kind::JpegXr);
        }
        None
    }

    /// From a content type, matched case-insensitively per 7.2.3.5.
    fn from_media(media: &str) -> Option<Kind> {
        // The parameter after a semicolon is not part of the comparison.
        let media = media.split(';').next().unwrap_or(media).trim();
        for (name, kind) in [
            ("image/png", Kind::Png),
            ("image/jpeg", Kind::Jpeg),
            ("image/tiff", Kind::Tiff),
            ("image/vnd.ms-photo", Kind::JpegXr),
            ("image/jxr", Kind::JpegXr),
        ] {
            if media.eq_ignore_ascii_case(name) {
                return Some(kind);
            }
        }
        None
    }
}

/// One image part, placed on the document and measured.
pub struct Image {
    /// The `/XObject` resource name the cell of a tiling pattern shows.
    pub resource: Vec<u8>,
    /// Pixels across and down, from the image's own header — never from the
    /// markup, which states a viewbox and not a size.
    pub px: (f64, f64),
    /// Dots per inch, horizontally and vertically, defaulting to 13.4.1's 96.
    pub dpi: (f64, f64),
}

impl Image {
    /// The image's size in XPS units, which is what an `Absolute` `Viewbox` is
    /// stated in.
    ///
    /// One XPS unit is 1/96 inch, so this is the pixel count scaled by 96 over
    /// the image's own resolution: a 384-pixel image at 96 dpi is four units,
    /// and the same image at 192 dpi is two.
    #[must_use]
    pub fn units(&self) -> (f64, f64) {
        (
            self.px.0 * DEFAULT_DPI / self.dpi.0,
            self.px.1 * DEFAULT_DPI / self.dpi.1,
        )
    }
}

/// Every image part a synthesised document has placed, keyed by the part an
/// `ImageSource` resolved to.
///
/// One per document, for [`super::font::Fonts`]'s reason: `add_page` copies the
/// document's whole resource table into every page, so two pages naming one
/// `/XI0` for two different pictures would be one picture.
#[derive(Default)]
pub struct Images {
    placed: HashMap<PartName, Result<Image, XpsElementDefect>>,
    /// Which route each PNG took, for the tests. See [`Images::route`].
    routes: HashMap<PartName, PngRoute>,
    next: usize,
}

impl Images {
    /// Places every image the `ImageBrush`es of one fixed page part name.
    ///
    /// # Errors
    /// [`Trouble::Exhausted`] when one page names more distinct images than the
    /// package could hold parts, which refuses the package for
    /// [`super::font::Fonts::load`]'s reason: a page naming eight thousand
    /// pictures is not a page whose pictures eight thousand lookups would find.
    pub fn load(
        &mut self,
        package: &mut Package<'_>,
        page: &PartName,
        builder: &mut DocumentBuilder,
        limits: &Limits,
    ) -> Result<(), Trouble> {
        let wanted = match package.read_part(page) {
            Ok(bytes) => image_references(bytes, page, limits)?,
            Err(_) => return Ok(()),
        };
        if wanted.len() > limits.max_parts {
            return Err(Trouble::Exhausted);
        }
        for name in wanted {
            if self.placed.contains_key(&name) {
                continue;
            }
            let placed = self.place_one(package, &name, builder, limits);
            self.placed.insert(name, placed);
        }
        Ok(())
    }

    /// The image an `ImageSource` on `page` names.
    ///
    /// Pure: resolution is arithmetic over two strings and the lookup is the
    /// table [`Images::load`] filled, so the drawing walk never needs the
    /// package.
    ///
    /// # Errors
    /// One [`XpsElementDefect`] per way an `ImageSource` can fail to be a
    /// picture, each by its own name.
    pub fn get(&self, page: &PartName, uri: &str) -> Result<&Image, XpsElementDefect> {
        // 9.1.5's `ImageSource` may carry a `{ColorConvertedBitmap …}` wrapper
        // naming an ICC profile. The profile is a non-goal of this whole plan,
        // and the wrapper's syntax has nowhere to put an sRGB fallback, so it
        // is refused rather than unwrapped and drawn in the wrong colours.
        if uri.trim_start().starts_with('{') {
            return Err(XpsElementDefect::ImageProfileUnsupported);
        }
        let name = page
            .resolve(source_reference(uri))
            .ok_or(XpsElementDefect::ImageUnresolved)?;
        match self.placed.get(&name) {
            Some(Ok(image)) => Ok(image),
            Some(Err(defect)) => Err(*defect),
            None => Err(XpsElementDefect::ImageUnresolved),
        }
    }

    fn place_one(
        &mut self,
        package: &mut Package<'_>,
        name: &PartName,
        builder: &mut DocumentBuilder,
        limits: &Limits,
    ) -> Result<Image, XpsElementDefect> {
        if !package.has(name) {
            return Err(XpsElementDefect::ImageUnresolved);
        }
        // Read and copied before the bytes, because both borrow the package.
        let declared = match package.media_type(name) {
            Ok(Some(media)) => Kind::from_media(media),
            // A part with no content type is not one 7.2.3.5 resolved, and
            // nothing may be assumed about it from its name.
            _ => None,
        };
        let bytes = package
            .read_part(name)
            .map_err(|_| XpsElementDefect::ImageUnresolved)?;
        let actual = Kind::from_magic(bytes);

        // The two rules, and either one refusing is a refusal. A part whose
        // content type says PNG and whose bytes say TIFF is not a PNG, and a
        // part whose bytes say PNG and whose content type says TIFF is a
        // package this build declines to guess about.
        for kind in [declared, actual].into_iter().flatten() {
            if matches!(kind, Kind::Tiff | Kind::JpegXr) {
                return Err(XpsElementDefect::ImageFormatUnsupported);
            }
        }
        let kind = match (declared, actual) {
            // They agree, or only one of them spoke.
            (Some(a), Some(b)) if a == b => a,
            (Some(a), None) => a,
            (None, Some(b)) => b,
            // They disagree about two formats this build *can* draw. The bytes
            // win, because a decoder reads bytes — but it is a leniency and it
            // is named, so "it opened" and "it opened cleanly" stay apart.
            (Some(_), Some(b)) => b,
            (None, None) => return Err(XpsElementDefect::ImageFormatUnsupported),
        };

        let resource = format!("XI{}", self.next).into_bytes();
        let image = match kind {
            Kind::Jpeg => {
                let (w, h, _) = jpeg_shape(bytes).ok_or(XpsElementDefect::ImageUnreadable)?;
                if !builder.add_image(&resource, &ImageData::Jpeg(bytes)) {
                    return Err(XpsElementDefect::ImageUnreadable);
                }
                Image {
                    resource,
                    px: (f64::from(w), f64::from(h)),
                    dpi: jfif_dpi(bytes).unwrap_or((DEFAULT_DPI, DEFAULT_DPI)),
                }
            }
            Kind::Png => {
                let png = png_image(bytes, &FilterLimits::new(limits.max_synthesised))
                    .map_err(|_| XpsElementDefect::ImageUnreadable)?;
                self.routes.insert(name.clone(), png.route());
                let data = png.image();
                let (w, h) = shape_of(&data).ok_or(XpsElementDefect::ImageUnreadable)?;
                if !builder.add_image(&resource, &data) {
                    return Err(XpsElementDefect::ImageUnreadable);
                }
                Image {
                    resource,
                    px: (f64::from(w), f64::from(h)),
                    dpi: phys_dpi(bytes).unwrap_or((DEFAULT_DPI, DEFAULT_DPI)),
                }
            }
            // Refused above, before either decoder was reached — so this arm
            // is unreachable, and it is here because the alternative is a
            // `match` that does not cover its own type.
            //
            // The redundancy is measured rather than assumed: the injection
            // matrix's "a TIFF the magic bytes say is drawn" removes the loop's
            // `actual` and **survives**, because this arm refuses the same file
            // a line later. That is the one injection of twenty-eight that
            // changes no answer, and it says the rule is enforced twice rather
            // than that a test is missing.
            Kind::Tiff | Kind::JpegXr => return Err(XpsElementDefect::ImageFormatUnsupported),
        };
        self.next += 1;
        Ok(image)
    }

    /// Whether an image part took the pass-through route or the decode.
    ///
    /// For the tests, and only for them: gap 29 milestone 6 found the route is
    /// invisible to every rendered comparison, so a test that cannot read it
    /// cannot assert it.
    #[must_use]
    pub fn route(&self, page: &PartName, uri: &str) -> Option<PngRoute> {
        let name = page.resolve(source_reference(uri))?;
        self.routes.get(&name).copied()
    }
}

/// The `/Width` and `/Height` an [`ImageData`] carries.
fn shape_of(data: &ImageData<'_>) -> Option<(u32, u32)> {
    match data {
        ImageData::Compressed(image) => Some((image.width, image.height)),
        ImageData::Rgb8 { width, height, .. } | ImageData::Gray8 { width, height, .. } => {
            Some((*width, *height))
        }
        ImageData::Jpeg(bytes) => jpeg_shape(bytes).map(|(w, h, _)| (w, h)),
        _ => None,
    }
}

/// PNG's `pHYs` (11.3.5.3), which states pixels per metre.
///
/// `None` when the chunk is absent or says the unit is unknown, which 13.4.1
/// turns into the 96 dpi default rather than into a refusal: an image with no
/// stated resolution is ordinary, and every PNG this repository writes is one.
fn phys_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    let mut at = 8usize;
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        let kind = bytes.get(at + 4..at + 8)?;
        if kind == b"pHYs" {
            let data = bytes.get(at + 8..at + 8 + len)?;
            if data.len() < 9 || data[8] != 1 {
                // Unit 0 is "unknown", which states an aspect ratio and not a
                // resolution, so it says nothing about how large the image is.
                return None;
            }
            let x = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?);
            let y = u32::from_be_bytes(data.get(4..8)?.try_into().ok()?);
            if x == 0 || y == 0 {
                return None;
            }
            // Per metre to per inch.
            return Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254));
        }
        if kind == b"IDAT" || kind == b"IEND" {
            // `pHYs` must precede the image data, so there is none.
            return None;
        }
        at = at.checked_add(12)?.checked_add(len)?;
    }
    None
}

/// JFIF's APP0 density (JFIF 1.02, clause 5), which states either dots per inch
/// or dots per centimetre.
///
/// `None` for a JPEG with no JFIF segment or one whose units field says the
/// numbers are an aspect ratio, for `phys_dpi`'s reason.
fn jfif_dpi(bytes: &[u8]) -> Option<(f64, f64)> {
    // The APP0 segment, when there is one, follows SOI immediately.
    if bytes.get(2..4)? != [0xFF, 0xE0] {
        return None;
    }
    let segment = bytes.get(4..)?;
    if segment.get(2..7)? != b"JFIF\0" {
        return None;
    }
    let units = *segment.get(9)?;
    let x = u16::from_be_bytes(segment.get(10..12)?.try_into().ok()?);
    let y = u16::from_be_bytes(segment.get(12..14)?.try_into().ok()?);
    if x == 0 || y == 0 {
        return None;
    }
    match units {
        1 => Some((f64::from(x), f64::from(y))),
        // Per centimetre.
        2 => Some((f64::from(x) * 2.54, f64::from(y) * 2.54)),
        // 0 is an aspect ratio and not a resolution.
        _ => None,
    }
}

/// An `ImageSource`'s part reference, with any fragment dropped.
fn source_reference(uri: &str) -> &str {
    uri.split('#').next().unwrap_or(uri)
}

/// Every part an `ImageBrush` of one fixed page part names, deduplicated and in
/// markup order.
///
/// The order is load-bearing for [`super::font::Fonts`]'s reason: it decides
/// which image gets `/XI0`, and a resource name that depended on a hash's
/// iteration order would make the synthesised document differ between two runs
/// over one package — which the determinism fingerprint hashes.
fn image_references(
    bytes: &[u8],
    page: &PartName,
    limits: &Limits,
) -> Result<Vec<PartName>, Trouble> {
    let Ok(source) = Source::new(bytes) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<PartName> = Vec::new();
    let mut seen: HashSet<PartName> = HashSet::new();
    for event in source.reader(&limits.xml) {
        let element = match event {
            Ok(Event::Start(element)) => element,
            Ok(_) => continue,
            // Markup that will not read is the painter's to report, and it
            // will: this pass answers with what it found and the drawing walk
            // meets the same failure a moment later.
            Err(_) => break,
        };
        if element.local() != "ImageBrush" || dialect_of(element.namespace()).is_none() {
            continue;
        }
        let Some(uri) = element.attribute(None, "ImageSource") else {
            continue;
        };
        if uri.trim_start().starts_with('{') {
            continue;
        }
        let Some(name) = page.resolve(source_reference(uri)) else {
            continue;
        };
        if seen.insert(name.clone()) {
            out.push(name);
            if out.len() > limits.max_parts {
                return Err(Trouble::Exhausted);
            }
        }
    }
    Ok(out)
}
