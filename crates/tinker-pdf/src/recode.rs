//! Image recompression and downsampling on save: [`crate::write::SaveOptions::images`].
//!
//! Off by default, and off means **off**: with [`ImagePolicy::Keep`] this
//! module is never entered and every image stream is written through as the
//! file stored it, byte for byte. A caller who asks names a coding per image
//! kind and, optionally, a resolution, and gets back a report naming every
//! image by object reference — what it carries now, or why it was left as it
//! was (ruling 10).
//!
//! # Why the switch is on `SaveOptions` and not `WriteOptions::images`
//!
//! The row this closes asked for a `WriteOptions::images`. It is on the
//! facade's save door for the reason font subsetting is, and one more.
//!
//! - **The resolution needs the interpreter.** "No more than N pixels per
//!   inch" is a property of where an image is *drawn*, not of the image: the
//!   same object placed as a thumbnail and as a full page has two resolutions.
//!   Placements come from interpreting every content stream that draws it —
//!   pages, form XObjects at any depth, annotation appearances — and the
//!   interpreter is `tinker-pdf-content`, which `tinker-pdf-cos` does not
//!   depend on (`cargo xtask dag`). A `WriteOptions::images` carrying a
//!   resolution would be a field the crate holding it cannot act on, the
//!   silent-flag shape `write.rs`'s module header rejects for fonts.
//! - **`WriteOptions` describes bytes on disk**, and it promises — in
//!   `crate::ImageData::Compressed`'s contract, on the cos side — that the
//!   writer never re-encodes image bytes. That is a promise worth keeping on
//!   the door that has always kept it; this pass runs *before* that writer,
//!   on the editor, and the writer still re-encodes nothing.
//!
//! # What is recoded, and how
//!
//! Every image XObject the document holds — swept from the cross-reference
//! table of [`tinker_pdf_cos::DocumentEditor::view`], so an image this editor
//! added is found too — is one of two kinds:
//!
//! - **Bilevel**: one component at one bit (an `/ImageMask`, a one-bit
//!   `DeviceGray`, a one-bit `/Indexed`), coded as [`BilevelCodec`] says:
//!   deflate, ITU-T T.6 (`/CCITTFaxDecode` with `/K -1`), or a T.88 generic
//!   region (`/JBIG2Decode`, an embedded stream of three segments). All three
//!   are lossless, so the samples come back bit for bit, and none is ever
//!   downsampled — a box filter makes grey of black and white, which a
//!   one-bit image cannot hold.
//! - **Continuous**: everything else, coded as [`ContinuousCodec`] says:
//!   deflate, lossless at any depth, or baseline JPEG (T.81) with **the
//!   caller's quantisation tables**, which needs eight bits and one or three
//!   components. There is no quality number, for the reason
//!   `tinker_pdf_filters::jpeg_encode`'s header gives at length.
//!
//! An image that is another image's `/SMask` or `/Mask` is coverage rather
//! than a picture, and is coded losslessly whatever the continuous codec: a
//! quantiser's ringing on an edge of alpha is a halo around the edge.
//!
//! **Downsampling** ([`ImageRecoding::max_ppi`]) is an integer box filter,
//! one whole factor per axis, chosen so that **no placement of the image is
//! finer than the caller's number on either axis**: a placement whose
//! transform maps the unit square's `x` axis to a vector of length `L`
//! points shows `Width` samples over `L / 72` inches, so the factor is
//! `ceil(finest ppi / max_ppi)`. Each output sample is the mean of its block,
//! rounded half up, and a partial block at the right or bottom edge is the
//! mean of the samples it has. Only an eight-bit, non-indexed image is
//! resampled.
//!
//! **The result is kept only if it is smaller** than the stream the file
//! stored, as the font pass keeps a face whose subset came out no smaller.
//!
//! # What is left whole, by name
//!
//! Every image the pass does not rewrite is in [`ImageReport::untouched`]
//! with an [`UntouchedImageReason`], and an image rewritten without the
//! downsampling asked for says why in [`Recoded::resolution_kept`]:
//!
//! - **Stored with an image codec** — `DCTDecode`, `JPXDecode`,
//!   `CCITTFaxDecode`, `JBIG2Decode` — or a filter this pass does not decode
//!   (`Crypt`, a name it does not know). This pass reads samples through the
//!   general filters only (7.4.2 to 7.4.5 and the predictors); decoding a
//!   codec's samples *in their own colour space* is the read side's image row,
//!   and a second decoder here would be a second answer to what those bytes
//!   mean.
//! - **A colour-key `/Mask`** (8.9.6.4) under a lossy coding or a resample:
//!   which pixels are masked is decided by exact sample equality, and neither
//!   preserves it. A lossless coding does, and is allowed.
//! - **A soft mask with `/Matte`** (11.6.5.3), and its parent, are never
//!   resampled: the matte relation requires the mask and the image to keep
//!   the same dimensions, and the two need not share placements.
//! - **`/Indexed`**: samples are palette indices, which neither a quantiser
//!   nor a mean means anything for.
//! - **Depth or components** a coding cannot carry.
//! - **No placement** a walked content stream draws — an image reached only
//!   through a tiling pattern's cell, a Type 3 glyph or a soft mask's group,
//!   which this pass does not interpret — is recoded but never resampled,
//!   because there is no resolution to measure.
//!
//! **A `/Decode` array is preserved by every operation this pass performs**,
//! so no image is left whole for one: a lossless coding keeps the samples it
//! applies to, and 8.9.5.2's map is affine per component, so a block mean
//! of samples is the block mean of what they decode to and a quantiser's error
//! is scaled by `(Dmax - Dmin) / 255`, never amplified. The row anticipated a
//! `/Decode` that could not be kept; none of the codings here produces one.
//!
//! Inline images (8.9.7) are part of a content stream and are not touched.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use tinker_pdf_content::{interpret, Device, FontSource, Form, GraphicsState, ImageRef, Matrix};
use tinker_pdf_cos::{
    pages as cos_pages, CosDocument, Dict, DocumentEditor, Name, ObjRef, Object, StreamData,
    XrefEntry,
};

/// What a save does to the document's images. See [`crate::write::SaveOptions::images`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ImagePolicy {
    /// Write every image stream through exactly as the file stored it. **The
    /// default**: a save that was not asked to change pictures does not.
    #[default]
    Keep,
    /// Recode, and optionally downsample, as the recoding says.
    Recode(ImageRecoding),
}

/// How each kind of image is coded, and at what resolution. See this module's
/// documentation.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ImageRecoding {
    /// Images of more than one bit, or more than one component.
    pub continuous: ContinuousCodec,
    /// One-bit, one-component images, `/ImageMask` included.
    pub bilevel: BilevelCodec,
    /// The finest resolution, in pixels per inch on either axis, any
    /// placement of a continuous image is left at; `None` resamples nothing.
    /// A value that is not a finite positive number resamples nothing either.
    pub max_ppi: Option<f64>,
}

impl ImageRecoding {
    /// Codes each kind as given, at its own resolution.
    #[must_use]
    pub fn new(continuous: ContinuousCodec, bilevel: BilevelCodec) -> ImageRecoding {
        ImageRecoding {
            continuous,
            bilevel,
            max_ppi: None,
        }
    }

    /// The same, with no placement finer than `ppi` on either axis.
    #[must_use]
    pub fn with_max_ppi(mut self, ppi: f64) -> ImageRecoding {
        self.max_ppi = Some(ppi);
        self
    }
}

/// How a continuous-tone image is coded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContinuousCodec {
    /// As stored, unless it is resampled — then deflated, losslessly, at its
    /// new size.
    Keep,
    /// `/FlateDecode`, lossless.
    Flate,
    /// Baseline JPEG (`/DCTDecode`) with these tables.
    Jpeg(JpegTables),
}

/// The caller's quantisation tables for [`ContinuousCodec::Jpeg`].
///
/// Natural row-major order, **not** zig-zag (the encoder applies B.2.4.1's
/// zig-zag in the DQT segment); every element must be 1 to 255. A grey image
/// uses `luminance` alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegTables {
    /// The table for `Y`, or for the one component of a grey image.
    pub luminance: [u8; 64],
    /// The table for `Cb` and `Cr`.
    pub chrominance: [u8; 64],
    /// 4:2:0 chrominance (each chrominance sample covers two by two pixels)
    /// rather than 4:4:4. Off is the one whose error the tables alone bound.
    pub subsampled: bool,
}

/// How a bilevel image is coded. All three are lossless.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BilevelCodec {
    /// As stored.
    Keep,
    /// `/FlateDecode`.
    Flate,
    /// ITU-T T.6, `/CCITTFaxDecode` with `/K -1`.
    CcittG4,
    /// An ITU-T T.88 generic region, `/JBIG2Decode`.
    Jbig2Generic,
}

/// The coding an image carries after the pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageCoding {
    /// `/FlateDecode`.
    Flate,
    /// `/DCTDecode`, baseline.
    Jpeg,
    /// `/CCITTFaxDecode`, G4.
    CcittG4,
    /// `/JBIG2Decode`, one generic region.
    Jbig2Generic,
}

/// Why an image was left as the file stored it, or kept its resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UntouchedImageReason {
    /// The policy asks nothing of this kind of image.
    NotAsked,
    /// Stored through a filter this pass does not decode to samples — an
    /// image codec (`DCTDecode`, `JPXDecode`, `CCITTFaxDecode`,
    /// `JBIG2Decode`), `Crypt`, or a name it does not know.
    Filter {
        /// The filter's name, as the file writes it.
        name: String,
    },
    /// The general filters failed, or produced fewer bytes than `/Width`,
    /// `/Height`, the components and `/BitsPerComponent` promise — or the
    /// dictionary states no positive 32-bit `/Width`, `/Height` or a depth
    /// 8.9.5.1 allows. A size is never refused for being large: what it costs
    /// is samples, and those are bounded where every stream is decoded.
    Undecodable,
    /// A `/ColorSpace` whose component count this pass cannot read.
    UnknownColourSpace,
    /// A colour-key `/Mask` (8.9.6.4), which a lossy coding or a resample
    /// would change the masked pixels of.
    ColourKeyMask,
    /// A soft mask carrying `/Matte`, or the image it masks (11.6.5.3): the
    /// two must keep the same dimensions.
    Matte,
    /// `/Indexed`: palette indices, which no quantiser or mean applies to.
    Indexed,
    /// A depth the coding or the resample cannot carry.
    Depth {
        /// `/BitsPerComponent`.
        bits: u8,
    },
    /// A component count baseline JPEG cannot carry (it codes one or three).
    Components {
        /// How many.
        count: u8,
    },
    /// One bit: a box filter would make grey.
    Bilevel,
    /// No walked content stream places it, so there is no resolution to
    /// measure.
    Unplaced,
    /// The new coding was no smaller than the stored one.
    NotSmaller,
    /// The encoder refused, in its own words.
    Encoder(String),
}

impl core::fmt::Display for UntouchedImageReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UntouchedImageReason::NotAsked => f.write_str("the policy asks nothing of it"),
            UntouchedImageReason::Filter { name } => {
                write!(f, "stored through /{name}, which this pass does not decode")
            }
            UntouchedImageReason::Undecodable => f.write_str("its samples would not decode"),
            UntouchedImageReason::UnknownColourSpace => {
                f.write_str("its colour space's components cannot be counted")
            }
            UntouchedImageReason::ColourKeyMask => {
                f.write_str("a colour-key /Mask decides by exact sample (8.9.6.4)")
            }
            UntouchedImageReason::Matte => {
                f.write_str("a /Matte soft mask keeps its image's size (11.6.5.3)")
            }
            UntouchedImageReason::Indexed => f.write_str("its samples are palette indices"),
            UntouchedImageReason::Depth { bits } => write!(f, "{bits} bits per component"),
            UntouchedImageReason::Components { count } => {
                write!(
                    f,
                    "{count} components, and baseline JPEG codes one or three"
                )
            }
            UntouchedImageReason::Bilevel => f.write_str("one bit: a box filter makes grey"),
            UntouchedImageReason::Unplaced => f.write_str("no walked content stream draws it"),
            UntouchedImageReason::NotSmaller => f.write_str("the new coding was no smaller"),
            UntouchedImageReason::Encoder(why) => write!(f, "the encoder refused: {why}"),
        }
    }
}

/// One image the pass rewrote.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Recoded {
    /// The image XObject.
    pub image: ObjRef,
    /// What it is coded as now.
    pub coding: ImageCoding,
    /// `/Width` and `/Height` before.
    pub size: (u32, u32),
    /// `/Width` and `/Height` after, which differ when it was resampled.
    pub resized: (u32, u32),
    /// The stream's stored bytes before.
    pub before: usize,
    /// And after.
    pub after: usize,
    /// Downsampling was asked for and not done to this image, and why.
    pub resolution_kept: Option<UntouchedImageReason>,
}

/// One image written through as the file stored it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UntouchedImage {
    /// The image XObject.
    pub image: ObjRef,
    /// Its stored bytes.
    pub bytes: usize,
    /// Why.
    pub reason: UntouchedImageReason,
}

impl core::fmt::Display for UntouchedImage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "image {} {} R left as stored ({} bytes): {}",
            self.image.num, self.image.gen, self.bytes, self.reason
        )
    }
}

/// What an image pass did, every image in object order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageReport {
    /// Every image rewritten.
    pub recoded: Vec<Recoded>,
    /// Every image written through as stored, and why (ruling 10).
    pub untouched: Vec<UntouchedImage>,
}

impl ImageReport {
    /// The image streams' stored bytes before the pass.
    #[must_use]
    pub fn bytes_before(&self) -> usize {
        self.recoded.iter().map(|r| r.before).sum::<usize>()
            + self.untouched.iter().map(|u| u.bytes).sum::<usize>()
    }

    /// And after.
    #[must_use]
    pub fn bytes_after(&self) -> usize {
        self.recoded.iter().map(|r| r.after).sum::<usize>()
            + self.untouched.iter().map(|u| u.bytes).sum::<usize>()
    }
}

/// Runs the pass over every image the editor's document holds.
pub(crate) fn apply(editor: &mut DocumentEditor, recoding: &ImageRecoding) -> ImageReport {
    // The editor's own objects resolve in the view, so an image or a page it
    // added is swept and walked like one the file had. A view that will not
    // open is a defect in the writer or the reader rather than in the input;
    // the file as opened is the fallback, and its images are still swept.
    let view = editor.view().unwrap_or_else(|_| editor.shared_document());
    let max_ppi = recoding.max_ppi.filter(|ppi| ppi.is_finite() && *ppi > 0.0);
    let placements = if max_ppi.is_some() {
        walk(&view)
    } else {
        BTreeMap::new()
    };

    let images = sweep(editor, &view);
    let masks = mask_targets(editor, &images);
    let mut report = ImageReport::default();
    for image in &images {
        let mut placed = placements.get(&image.reference.num).copied();
        // A mask is drawn wherever an image it masks is drawn.
        for parent in masks
            .parents
            .get(&image.reference.num)
            .into_iter()
            .flatten()
        {
            if let Some(theirs) = placements.get(parent) {
                placed = Some(placed.map_or(*theirs, |ours| ours.finer(theirs)));
            }
        }
        let job = Job {
            image,
            placed,
            is_mask: masks.parents.contains_key(&image.reference.num),
            matte: masks.matte.contains(&image.reference.num),
            max_ppi,
        };
        match recode_one(editor, &view, recoding, &job) {
            Outcome::Recoded(recoded) => report.recoded.push(recoded),
            Outcome::Untouched(reason) => report.untouched.push(UntouchedImage {
                image: image.reference,
                bytes: stored_len(&view, image.reference),
                reason,
            }),
        }
    }
    report
}

// ---------------------------------------------------------------------------
// The sweep.
// ---------------------------------------------------------------------------

/// One image XObject the sweep found.
struct Found {
    reference: ObjRef,
    dict: Dict,
}

/// Every image XObject in the view's cross-reference table, read through the
/// editor so its names are the editor's.
fn sweep(editor: &DocumentEditor, view: &CosDocument) -> Vec<Found> {
    let numbers: Vec<(u32, u16)> = view
        .xref()
        .iter()
        .filter_map(|(number, entry)| match entry {
            XrefEntry::Free { .. } => None,
            XrefEntry::Offset { gen, .. } => Some((number, gen)),
            XrefEntry::InStream { .. } => Some((number, 0)),
        })
        .take(crate::subset::MAX_SWEPT_OBJECTS)
        .collect();
    let doc = editor.document();
    let subtype = doc.intern(b"Subtype");
    let mut found = Vec::new();
    for (number, gen) in numbers {
        let reference = ObjRef::new(number, gen);
        let Some(object) = editor.get(reference) else {
            continue;
        };
        // A stream in the file reads as `Stream`, one the editor wrote as
        // `Dict` (its data lives in the overlay); a plain dictionary that says
        // `/Subtype /Image` is not a stream and fails to decode below, which
        // is reported rather than skipped.
        let Some(dict) = object.as_dict().cloned() else {
            continue;
        };
        let is_image = dict
            .get_name(subtype)
            .and_then(|n| doc.name_bytes(n))
            .is_some_and(|n| n.as_ref() == b"Image");
        if is_image {
            found.push(Found { reference, dict });
        }
    }
    found
}

/// Which images are another image's `/SMask` or `/Mask`, and which soft masks
/// carry `/Matte` (with their parents).
#[derive(Default)]
struct Masks {
    /// Mask object number to the numbers of the images that name it.
    parents: HashMap<u32, Vec<u32>>,
    /// Images on either side of a `/Matte` relation.
    matte: BTreeSet<u32>,
}

fn mask_targets(editor: &DocumentEditor, images: &[Found]) -> Masks {
    let doc = editor.document();
    let (smask, mask, matte) = (
        doc.intern(b"SMask"),
        doc.intern(b"Mask"),
        doc.intern(b"Matte"),
    );
    let mut masks = Masks::default();
    for image in images {
        for key in [smask, mask] {
            let Some(target) = image.dict.get_ref(key) else {
                continue;
            };
            masks
                .parents
                .entry(target.num)
                .or_default()
                .push(image.reference.num);
            if key == smask {
                let has_matte = editor
                    .get(target)
                    .and_then(|o| o.as_dict().map(|d| d.contains_key(matte)))
                    .unwrap_or(false);
                if has_matte {
                    masks.matte.insert(target.num);
                    masks.matte.insert(image.reference.num);
                }
            }
        }
    }
    masks
}

/// The stream's stored length, as the view reads it.
fn stored_len(view: &CosDocument, reference: ObjRef) -> usize {
    view.stream_raw(reference).map(|raw| raw.len()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The walk: where every image is drawn.
// ---------------------------------------------------------------------------

/// The shortest each axis of an image's unit square is drawn, in points,
/// over every placement the walk found: the finest placement on each axis is
/// all a resolution needs, so that is all that is kept.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shortest {
    x: f64,
    y: f64,
}

impl Shortest {
    /// A placement's two axes, where both are finite and not degenerate.
    fn of(m: &Matrix) -> Option<Shortest> {
        let (x, y) = (m.a.hypot(m.b), m.c.hypot(m.d));
        (x > 0.0 && y > 0.0 && x.is_finite() && y.is_finite()).then_some(Shortest { x, y })
    }

    fn finer(self, other: &Shortest) -> Shortest {
        Shortest {
            x: self.x.min(other.x),
            y: self.y.min(other.y),
        }
    }
}

/// Image object number to its finest placement.
type Placements = BTreeMap<u32, Shortest>;

/// Interprets every page and every annotation appearance, recording each
/// image's placements.
fn walk(view: &Arc<CosDocument>) -> Placements {
    let found: Arc<Mutex<Option<ObjRef>>> = Arc::new(Mutex::new(None));
    let forms: Arc<Mutex<HashMap<u32, Arc<Vec<u8>>>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut device = Placer {
        found: Arc::clone(&found),
        placements: BTreeMap::new(),
    };
    for page in cos_pages::collect(view) {
        let scope = Scope {
            doc: Arc::clone(view),
            resources: page.resources.clone(),
            ancestry: Vec::new(),
            found: Arc::clone(&found),
            forms: Arc::clone(&forms),
        };
        let content = cos_pages::content_bytes(view, &page);
        interpret(&content, Matrix::IDENTITY, &mut device, &scope);

        for (content, resources, matrix) in appearances(view, page.reference) {
            let scope = Scope {
                doc: Arc::clone(view),
                resources,
                ancestry: Vec::new(),
                found: Arc::clone(&found),
                forms: Arc::clone(&forms),
            };
            interpret(&content, matrix, &mut device, &scope);
        }
    }
    device.placements
}

/// The device: an image's placement is the CTM at its `Do`.
struct Placer {
    /// The image the scope resolved for the `Do` being drawn.
    found: Arc<Mutex<Option<ObjRef>>>,
    placements: Placements,
}

impl Device for Placer {
    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if image.inline {
            return;
        }
        let Some(reference) = self.found.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        // A placement that draws nothing measures nothing.
        let Some(axes) = Shortest::of(&state.ctm) else {
            return;
        };
        self.placements
            .entry(reference.num)
            .and_modify(|seen| *seen = seen.finer(&axes))
            .or_insert(axes);
    }
}

/// A resource scope of the walk.
///
/// Its own `FontSource` rather than [`crate::resources::PageResources`],
/// because the one question the walk asks is "which object did this `Do`
/// name", and the interpreter asks the scope [`FontSource::form`] for every
/// `Do` before it draws an image — so the scope that resolves the name is the
/// one that knows the answer, in the scope the name means something in. It
/// interprets nothing else: no fonts (a glyph draws no image), no Type 3
/// procedures and no soft-mask groups (an image only they reach is
/// [`UntouchedImageReason::Unplaced`]).
struct Scope {
    doc: Arc<CosDocument>,
    resources: Option<Dict>,
    /// The forms this scope is inside, so a form that draws itself is not
    /// entered again (the interpreter's depth cap would stop it, after
    /// sixteen copies).
    ancestry: Vec<u32>,
    found: Arc<Mutex<Option<ObjRef>>>,
    /// Decoded form content, once per form however often it is drawn.
    forms: Arc<Mutex<HashMap<u32, Arc<Vec<u8>>>>>,
}

impl Scope {
    fn xobject(&self, name: &[u8]) -> Option<(ObjRef, Dict)> {
        let resources = self.resources.as_ref()?;
        let table = self.doc.resolve_key(resources, self.doc.intern(b"XObject"));
        let reference = table.as_dict()?.get_ref(self.doc.intern(name))?;
        let object = self.doc.get(reference).ok()?;
        Some((reference, object.as_dict()?.clone()))
    }

    fn subtype(&self, dict: &Dict) -> Option<Arc<[u8]>> {
        self.doc
            .resolve_key(dict, self.doc.intern(b"Subtype"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))
    }
}

impl FontSource for Scope {
    fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
        Vec::new()
    }

    fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
        (0.0, 0.0, 0.0)
    }

    fn form(&self, name: &[u8]) -> Option<Form> {
        if let Ok(mut slot) = self.found.lock() {
            *slot = None;
        }
        let (reference, dict) = self.xobject(name)?;
        match self.subtype(&dict).as_deref() {
            Some(b"Image") => {
                if let Ok(mut slot) = self.found.lock() {
                    *slot = Some(reference);
                }
                None
            }
            Some(b"Form") if !self.ancestry.contains(&reference.num) => {
                let content = {
                    let cached = self
                        .forms
                        .lock()
                        .ok()
                        .and_then(|cache| cache.get(&reference.num).cloned());
                    match cached {
                        Some(content) => content,
                        None => {
                            let content = Arc::new(self.doc.stream_decoded(reference).ok()?);
                            if let Ok(mut cache) = self.forms.lock() {
                                cache.insert(reference.num, Arc::clone(&content));
                            }
                            content
                        }
                    }
                };
                Some(Form {
                    content: content.as_ref().clone(),
                    matrix: matrix_of(&self.doc, &dict, b"Matrix").unwrap_or(Matrix::IDENTITY),
                    bbox: None,
                    group: None,
                    stream: (u64::from(reference.num) << 16) | u64::from(reference.gen),
                })
            }
            _ => None,
        }
    }

    fn form_scope(&self, name: &[u8]) -> Option<Arc<Self>> {
        let (reference, dict) = self.xobject(name)?;
        // Always a scope of its own, even for a form with no `/Resources`
        // (which keeps the invoking one, 8.10.1), so the ancestry grows with
        // every form entered and a cycle is seen whatever the form brought.
        let own = self
            .doc
            .resolve_key(&dict, Name::RESOURCES)
            .as_dict()
            .cloned();
        let mut ancestry = self.ancestry.clone();
        ancestry.push(reference.num);
        Some(Arc::new(Scope {
            doc: Arc::clone(&self.doc),
            resources: own.or_else(|| self.resources.clone()),
            ancestry,
            found: Arc::clone(&self.found),
            forms: Arc::clone(&self.forms),
        }))
    }
}

/// A six-number matrix under `key`.
fn matrix_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Matrix> {
    let value = doc.resolve_key(dict, doc.intern(key));
    let values = value.as_array()?;
    let n = |i: usize| values.get(i).and_then(|v| doc.resolve(v).as_number());
    let m = Matrix {
        a: n(0)?,
        b: n(1)?,
        c: n(2)?,
        d: n(3)?,
        e: n(4)?,
        f: n(5)?,
    };
    m.is_finite().then_some(m)
}

/// A four-number rectangle under `key`, normalised.
fn rect_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<[f64; 4]> {
    let value = doc.resolve_key(dict, doc.intern(key));
    let values = value.as_array()?;
    let n = |i: usize| values.get(i).and_then(|v| doc.resolve(v).as_number());
    let (x0, y0, x1, y1) = (n(0)?, n(1)?, n(2)?, n(3)?);
    let rect = [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
    rect.iter().all(|v| v.is_finite()).then_some(rect)
}

/// Every appearance stream of every annotation on a page — each state of
/// `/AP /N` — with its resources and the transform 12.5.5 draws it through.
fn appearances(view: &CosDocument, page: ObjRef) -> Vec<(Vec<u8>, Option<Dict>, Matrix)> {
    let mut out = Vec::new();
    let Ok(page) = view.get(page) else {
        return out;
    };
    let Some(page) = page.as_dict() else {
        return out;
    };
    let annots = view.resolve_key(page, view.intern(b"Annots"));
    let Some(annots) = annots.as_array() else {
        return out;
    };
    for annot in annots.iter().take(crate::annotations::MAX_ANNOTS) {
        let annot = view.resolve(annot);
        let Some(annot) = annot.as_dict() else {
            continue;
        };
        let Some(rect) = rect_of(view, annot, b"Rect") else {
            continue;
        };
        let ap = view.resolve_key(annot, view.intern(b"AP"));
        let Some(ap) = ap.as_dict() else {
            continue;
        };
        let normal = ap.get(view.intern(b"N")).cloned();
        let mut streams = Vec::new();
        match normal {
            Some(Object::Ref(r)) => match view.get(r).ok().as_deref() {
                // A stream is one appearance; a dictionary is one per state.
                Some(Object::Stream(_)) => streams.push(r),
                Some(Object::Dict(states)) => {
                    streams.extend(states.iter().filter_map(|(_, v)| v.as_objref()));
                }
                _ => {}
            },
            Some(Object::Dict(states)) => {
                streams.extend(states.iter().filter_map(|(_, v)| v.as_objref()));
            }
            _ => {}
        }
        for stream in streams {
            let Ok(object) = view.get(stream) else {
                continue;
            };
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let Ok(content) = view.stream_decoded(stream) else {
                continue;
            };
            let matrix = matrix_of(view, dict, b"Matrix").unwrap_or(Matrix::IDENTITY);
            let Some(bbox) = rect_of(view, dict, b"BBox") else {
                continue;
            };
            let resources = view.resolve_key(dict, Name::RESOURCES).as_dict().cloned();
            if let Some(placed) = appearance_matrix(rect, bbox, &matrix) {
                out.push((content, resources, matrix.then(&placed)));
            }
        }
    }
    out
}

/// 12.5.5's algorithm: the box transformed by `/Matrix`, its bounding
/// rectangle, and the scale-and-translate `A` that maps that onto `/Rect`.
fn appearance_matrix(rect: [f64; 4], bbox: [f64; 4], matrix: &Matrix) -> Option<Matrix> {
    let corners = [
        matrix.apply(bbox[0], bbox[1]),
        matrix.apply(bbox[2], bbox[1]),
        matrix.apply(bbox[2], bbox[3]),
        matrix.apply(bbox[0], bbox[3]),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return None;
    }
    let sx = (rect[2] - rect[0]) / w;
    let sy = (rect[3] - rect[1]) / h;
    let a = Matrix {
        a: sx,
        b: 0.0,
        c: 0.0,
        d: sy,
        e: rect[0] - x0 * sx,
        f: rect[1] - y0 * sy,
    };
    a.is_finite().then_some(a)
}

// ---------------------------------------------------------------------------
// One image.
// ---------------------------------------------------------------------------

struct Job<'a> {
    image: &'a Found,
    placed: Option<Shortest>,
    is_mask: bool,
    matte: bool,
    max_ppi: Option<f64>,
}

enum Outcome {
    Recoded(Recoded),
    Untouched(UntouchedImageReason),
}

/// What the image dictionary says its samples are.
struct Layout {
    width: u32,
    height: u32,
    bits: u8,
    components: u8,
    indexed: bool,
    colour_key: bool,
}

impl Layout {
    /// Bytes in one row, 8.9.3: rows start on byte boundaries.
    fn stride(&self) -> usize {
        (self.width as usize)
            .saturating_mul(usize::from(self.components))
            .saturating_mul(usize::from(self.bits))
            .div_ceil(8)
    }

    fn bilevel(&self) -> bool {
        self.bits == 1 && self.components == 1
    }
}

fn recode_one(
    editor: &mut DocumentEditor,
    view: &CosDocument,
    recoding: &ImageRecoding,
    job: &Job<'_>,
) -> Outcome {
    let reference = job.image.reference;
    let dict = &job.image.dict;
    if let Some(name) = unreadable_filter(editor, dict) {
        return Outcome::Untouched(UntouchedImageReason::Filter { name });
    }
    let layout = match layout(editor, dict) {
        Ok(layout) => layout,
        Err(reason) => return Outcome::Untouched(reason),
    };
    let Some(samples) = editor.stream_bytes(reference) else {
        return Outcome::Untouched(UntouchedImageReason::Undecodable);
    };
    let expected = layout.stride().saturating_mul(layout.height as usize);
    let Some(samples) = samples.get(..expected) else {
        return Outcome::Untouched(UntouchedImageReason::Undecodable);
    };

    // The resolution: a factor per axis, or why there is none.
    let mut resolution_kept = None;
    let mut factors = (1u32, 1u32);
    if let Some(max_ppi) = job.max_ppi {
        let refusal = if layout.bilevel() {
            Some(UntouchedImageReason::Bilevel)
        } else if layout.indexed {
            Some(UntouchedImageReason::Indexed)
        } else if layout.bits != 8 {
            Some(UntouchedImageReason::Depth { bits: layout.bits })
        } else if layout.colour_key {
            Some(UntouchedImageReason::ColourKeyMask)
        } else if job.matte {
            Some(UntouchedImageReason::Matte)
        } else {
            None
        };
        match (refusal, job.placed) {
            (Some(reason), _) => resolution_kept = Some(reason),
            (None, None) => resolution_kept = Some(UntouchedImageReason::Unplaced),
            (None, Some(placed)) => factors = reduction(&layout, placed, max_ppi),
        }
    }
    let resampled = factors != (1, 1);

    // The coding.
    let coding = if layout.bilevel() {
        match recoding.bilevel {
            BilevelCodec::Keep => None,
            BilevelCodec::Flate => Some(Target::Flate),
            BilevelCodec::CcittG4 => Some(Target::CcittG4),
            BilevelCodec::Jbig2Generic => Some(Target::Jbig2),
        }
    } else {
        match recoding.continuous {
            ContinuousCodec::Keep => resampled.then_some(Target::Flate),
            ContinuousCodec::Flate => Some(Target::Flate),
            // Coverage is coded losslessly whatever the picture codec is.
            ContinuousCodec::Jpeg(_) if job.is_mask => Some(Target::Flate),
            ContinuousCodec::Jpeg(tables) => {
                if layout.indexed {
                    return Outcome::Untouched(UntouchedImageReason::Indexed);
                }
                if layout.bits != 8 {
                    return Outcome::Untouched(UntouchedImageReason::Depth { bits: layout.bits });
                }
                if !matches!(layout.components, 1 | 3) {
                    return Outcome::Untouched(UntouchedImageReason::Components {
                        count: layout.components,
                    });
                }
                if layout.colour_key {
                    return Outcome::Untouched(UntouchedImageReason::ColourKeyMask);
                }
                Some(Target::Jpeg(tables))
            }
        }
    };
    let Some(target) = coding else {
        // Nothing to code it as: a resample that was asked for and refused
        // is the answer, and `NotAsked` only when nothing was.
        return Outcome::Untouched(resolution_kept.unwrap_or(UntouchedImageReason::NotAsked));
    };

    let (width, height, samples) = if resampled {
        box_filter(&layout, samples, factors)
    } else {
        (layout.width, layout.height, samples.to_vec())
    };
    let resized = Layout {
        width,
        height,
        ..layout
    };
    let coded = match encode(editor, &resized, &samples, target) {
        Ok(coded) => coded,
        Err(reason) => return Outcome::Untouched(reason),
    };
    let before = stored_len(view, reference);
    let after = coded.data.len();
    if after >= before {
        return Outcome::Untouched(UntouchedImageReason::NotSmaller);
    }
    write(editor, reference, dict, &resized, coded);
    Outcome::Recoded(Recoded {
        image: reference,
        coding: target.coding(),
        size: (layout.width, layout.height),
        resized: (width, height),
        before,
        after,
        resolution_kept,
    })
}

#[derive(Clone, Copy)]
enum Target {
    Flate,
    Jpeg(JpegTables),
    CcittG4,
    Jbig2,
}

impl Target {
    fn coding(self) -> ImageCoding {
        match self {
            Target::Flate => ImageCoding::Flate,
            Target::Jpeg(_) => ImageCoding::Jpeg,
            Target::CcittG4 => ImageCoding::CcittG4,
            Target::Jbig2 => ImageCoding::Jbig2Generic,
        }
    }
}

/// The first filter in the chain this pass will not decode to samples.
fn unreadable_filter(editor: &DocumentEditor, dict: &Dict) -> Option<String> {
    let doc = editor.document();
    let names: Vec<Name> = match dict.get(Name::FILTER).map(|f| resolve(editor, f)) {
        None | Some(Object::Null) => Vec::new(),
        Some(Object::Name(name)) => vec![name],
        Some(Object::Array(items)) => {
            let mut names = Vec::new();
            for item in items {
                match resolve(editor, &item) {
                    Object::Name(name) => names.push(name),
                    _ => return Some("?".to_string()),
                }
            }
            names
        }
        Some(_) => return Some("?".to_string()),
    };
    // A stream whose data lives in another file (7.3.8.2's `/F`) has no bytes
    // here to recode.
    if dict.contains_key(doc.intern(b"F")) {
        return Some("F".to_string());
    }
    for name in names {
        let bytes = doc.name_bytes(name).unwrap_or_else(|| Arc::from(&b"?"[..]));
        let general = matches!(
            bytes.as_ref(),
            b"FlateDecode"
                | b"Fl"
                | b"LZWDecode"
                | b"LZW"
                | b"RunLengthDecode"
                | b"RL"
                | b"ASCIIHexDecode"
                | b"AHx"
                | b"ASCII85Decode"
                | b"A85"
        );
        if !general {
            return Some(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    None
}

/// Follows references through the editor, a few deep.
fn resolve(editor: &DocumentEditor, object: &Object) -> Object {
    let mut current = object.clone();
    for _ in 0..8 {
        match current {
            Object::Ref(r) => current = editor.get(r).unwrap_or(Object::Null),
            other => return other,
        }
    }
    Object::Null
}

/// What the dictionary says about the samples.
fn layout(editor: &DocumentEditor, dict: &Dict) -> Result<Layout, UntouchedImageReason> {
    let doc = editor.document();
    let int = |key: &[u8]| match resolve(editor, dict.get(doc.intern(key)).unwrap_or(&Object::Null))
    {
        Object::Int(value) => Some(value),
        _ => None,
    };
    // Any positive 32-bit size, and no cap of this pass's own in front of it.
    // The samples are what the size costs, and they are bounded already: they
    // decode under the ceiling every stream decodes under, a size they do not
    // fill is `Undecodable` when they are read, and every encoder this pass
    // calls checks the size against the samples and refuses in its own words —
    // a JPEG frame, which states each side in sixteen bits, among them. A
    // `1 << 16` that stood here reported a valid 70 000-sample-wide image as
    // samples that would not decode.
    let side = |value: Option<i64>| {
        value
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v >= 1)
            .ok_or(UntouchedImageReason::Undecodable)
    };
    let width = side(int(b"Width"))?;
    let height = side(int(b"Height"))?;
    let image_mask = matches!(
        resolve(
            editor,
            dict.get(doc.intern(b"ImageMask")).unwrap_or(&Object::Null)
        ),
        Object::Bool(true)
    );
    let colour_key = matches!(
        resolve(
            editor,
            dict.get(doc.intern(b"Mask")).unwrap_or(&Object::Null)
        ),
        Object::Array(_)
    );
    if image_mask {
        // 8.9.6.2: a stencil is one bit, and has no colour space.
        return Ok(Layout {
            width,
            height,
            bits: 1,
            components: 1,
            indexed: false,
            colour_key: false,
        });
    }
    let bits = match int(b"BitsPerComponent") {
        Some(b @ (1 | 2 | 4 | 8 | 16)) => b as u8,
        _ => return Err(UntouchedImageReason::Undecodable),
    };
    let space = resolve(
        editor,
        dict.get(doc.intern(b"ColorSpace")).unwrap_or(&Object::Null),
    );
    let (components, indexed) =
        components(editor, &space).ok_or(UntouchedImageReason::UnknownColourSpace)?;
    Ok(Layout {
        width,
        height,
        bits,
        components,
        indexed,
        colour_key,
    })
}

/// A colour space's component count, and whether it is `/Indexed` (8.6).
fn components(editor: &DocumentEditor, space: &Object) -> Option<(u8, bool)> {
    let doc = editor.document();
    let name = |object: &Object| match resolve(editor, object) {
        Object::Name(n) => doc.name_bytes(n),
        _ => None,
    };
    match space {
        Object::Name(_) => match name(space)?.as_ref() {
            b"DeviceGray" | b"CalGray" => Some((1, false)),
            b"DeviceRGB" | b"CalRGB" | b"Lab" => Some((3, false)),
            b"DeviceCMYK" => Some((4, false)),
            _ => None,
        },
        Object::Array(items) => {
            let family = name(items.first()?)?;
            match family.as_ref() {
                b"DeviceGray" | b"CalGray" | b"Separation" => Some((1, false)),
                b"DeviceRGB" | b"CalRGB" | b"Lab" => Some((3, false)),
                b"DeviceCMYK" => Some((4, false)),
                b"Indexed" | b"I" => Some((1, true)),
                b"DeviceN" => match resolve(editor, items.get(1)?) {
                    Object::Array(names) => u8::try_from(names.len())
                        .ok()
                        .filter(|n| *n > 0)
                        .map(|n| (n, false)),
                    _ => None,
                },
                b"ICCBased" => {
                    let stream = resolve(editor, items.get(1)?);
                    let n = match resolve(editor, stream.as_dict()?.get(Name::N)?) {
                        Object::Int(n @ (1 | 3 | 4)) => n as u8,
                        _ => return None,
                    };
                    Some((n, false))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The whole factor per axis that leaves no placement finer than `max_ppi`.
fn reduction(layout: &Layout, placed: Shortest, max_ppi: f64) -> (u32, u32) {
    // Default user space is 1/72 inch (8.3.2.3), so an axis drawn `length`
    // points long shows its samples over `length / 72` inches.
    let finest_x = 72.0 * f64::from(layout.width) / placed.x;
    let finest_y = 72.0 * f64::from(layout.height) / placed.y;
    let factor = |ppi: f64, samples: u32| -> u32 {
        // A ratio within a part in 10^9 of a whole number is that number:
        // 12 ppi asked down to 4 is three, not four because of a last ulp.
        let ratio = ppi / max_ppi;
        if !(ratio.is_finite() && ratio > 1.0) {
            return 1;
        }
        let whole = (ratio - ratio * 1e-9).ceil();
        if whole >= f64::from(samples) {
            samples.max(1)
        } else {
            whole.max(1.0) as u32
        }
    };
    (
        factor(finest_x, layout.width),
        factor(finest_y, layout.height),
    )
}

/// An integer box filter over eight-bit interleaved samples: each output
/// sample is its block's mean, rounded half up, and a partial block at the
/// right or bottom edge is the mean of what it holds.
fn box_filter(layout: &Layout, samples: &[u8], (fx, fy): (u32, u32)) -> (u32, u32, Vec<u8>) {
    let n = usize::from(layout.components);
    let (w, h) = (layout.width as usize, layout.height as usize);
    let (fx, fy) = (fx.max(1) as usize, fy.max(1) as usize);
    let (ow, oh) = (w.div_ceil(fx), h.div_ceil(fy));
    let mut out = Vec::with_capacity(ow * oh * n);
    for oy in 0..oh {
        let rows = oy * fy..((oy + 1) * fy).min(h);
        for ox in 0..ow {
            let cols = ox * fx..((ox + 1) * fx).min(w);
            let count = (rows.len() * cols.len()) as u64;
            for c in 0..n {
                let mut sum = 0u64;
                for y in rows.clone() {
                    for x in cols.clone() {
                        sum += u64::from(samples.get((y * w + x) * n + c).copied().unwrap_or(0));
                    }
                }
                // `count` is at least one: every block holds its first sample.
                out.push(((sum + count / 2) / count.max(1)) as u8);
            }
        }
    }
    (ow as u32, oh as u32, out)
}

/// An image's new stream: the bytes, the `/Filter` and the `/DecodeParms`.
struct Coded {
    data: Vec<u8>,
    filter: &'static [u8],
    parms: Option<Dict>,
}

/// Codes the samples as `target` says.
fn encode(
    editor: &DocumentEditor,
    layout: &Layout,
    samples: &[u8],
    target: Target,
) -> Result<Coded, UntouchedImageReason> {
    let encoder = |e: &dyn core::fmt::Display| UntouchedImageReason::Encoder(e.to_string());
    match target {
        Target::Flate => Ok(Coded {
            data: tinker_pdf_filters::zlib_compress(samples),
            filter: b"FlateDecode",
            parms: None,
        }),
        Target::Jpeg(tables) => {
            use tinker_pdf_filters::{
                jpeg_encode, JpegOptions, JpegQuantisation, JpegSampling, JpegSource,
                JpegSourceColour,
            };
            let colour = if layout.components == 3 {
                JpegSourceColour::Rgb
            } else {
                JpegSourceColour::Gray
            };
            let sampling = if tables.subsampled && layout.components == 3 {
                JpegSampling::FourTwoZero
            } else {
                JpegSampling::FourFourFour
            };
            let coded = jpeg_encode(
                &JpegSource {
                    width: layout.width,
                    height: layout.height,
                    colour,
                    stride: layout.stride(),
                    data: samples,
                },
                &JpegOptions {
                    quantisation: JpegQuantisation::Tables {
                        luminance: tables.luminance,
                        chrominance: tables.chrominance,
                    },
                    sampling,
                    restart_interval: 0,
                },
            )
            .map_err(|e| encoder(&e))?;
            Ok(Coded {
                data: coded,
                filter: b"DCTDecode",
                parms: None,
            })
        }
        Target::CcittG4 => {
            let coded = tinker_pdf_filters::ccitt_g4_encode(&tinker_pdf_filters::CcittSource {
                columns: layout.width,
                rows: layout.height,
                // A one-bit sample of 0 is black in DeviceGray and the
                // painted value of a stencil; Table 11's default reads 0 as
                // black, so the bits go through as they are.
                black_is_1: false,
                stride: layout.stride(),
                end_of_block: true,
                data: samples,
            })
            .map_err(|e| encoder(&e))?;
            let mut parms = Dict::new();
            parms.insert(editor.intern(b"K"), Object::Int(-1));
            parms.insert(
                editor.intern(b"Columns"),
                Object::Int(i64::from(layout.width)),
            );
            parms.insert(
                editor.intern(b"Rows"),
                Object::Int(i64::from(layout.height)),
            );
            Ok(Coded {
                data: coded,
                filter: b"CCITTFaxDecode",
                parms: Some(parms),
            })
        }
        Target::Jbig2 => {
            // T.88 6.2.2 codes 1 for black, and a one-bit sample is 0 for
            // black: the inversion the read side undoes in `jbig2_samples`.
            let inverted: Vec<u8> = samples.iter().map(|b| !b).collect();
            let region = tinker_pdf_filters::jbig2_generic_region_segment(
                &tinker_pdf_filters::Jbig2GenericSource {
                    width: layout.width,
                    height: layout.height,
                    template: 0,
                    tpgdon: true,
                    // 6.2.5.3's nominal positions for template 0.
                    at: [(3, -1), (-3, -1), (2, -2), (-2, -2)],
                    stride: layout.stride(),
                    data: &inverted,
                },
                0,
                0,
                0,
            )
            .map_err(|e| encoder(&e))?;
            Ok(Coded {
                data: embedded_jbig2(layout.width, layout.height, &region),
                filter: b"JBIG2Decode",
                parms: None,
            })
        }
    }
}

/// T.88 Annex D.3's embedded organisation, which is what `/JBIG2Decode` reads
/// (7.4.7): a page information segment, one immediate lossless generic region
/// and an end of page, all on page 1, with no file header.
fn embedded_jbig2(width: u32, height: u32, region: &[u8]) -> Vec<u8> {
    // 7.2's header, short form: number, flags (the type in the low six bits),
    // one byte of referred-to count and retention, a one-byte page
    // association, and the data length.
    let segment = |number: u32, kind: u8, data: &[u8]| -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + 11);
        out.extend_from_slice(&number.to_be_bytes());
        out.push(kind & 0x3F);
        out.push(0);
        out.push(1);
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        out.extend_from_slice(data);
        out
    };
    // 7.4.8: width, height, two unknown resolutions, flags (bit 0: the page
    // is eventually lossless; default pixel 0, white) and no striping.
    let mut page = Vec::with_capacity(19);
    page.extend_from_slice(&width.to_be_bytes());
    page.extend_from_slice(&height.to_be_bytes());
    page.extend_from_slice(&0u32.to_be_bytes());
    page.extend_from_slice(&0u32.to_be_bytes());
    page.push(0x01);
    page.extend_from_slice(&0u16.to_be_bytes());
    let mut out = segment(0, 48, &page);
    // 7.3: type 39, the immediate *lossless* generic region — it is.
    out.extend_from_slice(&segment(1, 39, region));
    out.extend_from_slice(&segment(2, 49, &[]));
    out
}

/// Replaces the image's stream, keeping every key that still describes it.
fn write(
    editor: &mut DocumentEditor,
    reference: ObjRef,
    old: &Dict,
    layout: &Layout,
    coded: Coded,
) {
    let doc = editor.document();
    // What the new bytes make stale: the coding, its parameters, the stored
    // length (written for the stream) and the decoded length hint, and the
    // dimensions, which are written again below.
    let stale = [
        Name::FILTER,
        Name::DECODE_PARMS,
        Name::LENGTH,
        doc.intern(b"DL"),
        doc.intern(b"Width"),
        doc.intern(b"Height"),
    ];
    let mut dict: Dict = old
        .iter()
        .filter(|(key, _)| !stale.contains(key))
        .cloned()
        .collect();
    dict.insert(
        editor.intern(b"Width"),
        Object::Int(i64::from(layout.width)),
    );
    dict.insert(
        editor.intern(b"Height"),
        Object::Int(i64::from(layout.height)),
    );
    dict.insert(Name::FILTER, Object::Name(editor.intern(coded.filter)));
    if let Some(parms) = coded.parms {
        dict.insert(Name::DECODE_PARMS, Object::Dict(parms));
    }
    editor.put_stream(
        reference,
        StreamData {
            dict,
            data: coded.data,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(width: u32, height: u32) -> Layout {
        Layout {
            width,
            height,
            bits: 8,
            components: 1,
            indexed: false,
            colour_key: false,
        }
    }

    /// The box filter, by hand: a 5 x 3 grey image at 2 x 2 is 3 x 2, its
    /// right column and bottom row the means of the partial blocks.
    #[test]
    fn a_box_filter_is_its_blocks_means_rounded_half_up() {
        #[rustfmt::skip]
        let samples = [
            0, 1, 2, 3, 250,
            4, 5, 6, 8, 251,
            9, 10, 11, 12, 13,
        ];
        let (w, h, out) = box_filter(&gray(5, 3), &samples, (2, 2));
        assert_eq!((w, h), (3, 2));
        // (0+1+4+5)/4 = 2.5 -> 3; (2+3+6+8)/4 = 4.75 -> 5;
        // (250+251)/2 = 250.5 -> 251; (9+10)/2 = 9.5 -> 10;
        // (11+12)/2 = 11.5 -> 12; 13.
        assert_eq!(out, vec![3, 5, 251, 10, 12, 13]);
    }

    /// The factor is the ceiling of the finest placement's ratio, per axis,
    /// with a whole ratio staying whole and the factor never past the image.
    #[test]
    fn the_reduction_is_the_ceiling_at_the_finest_placement() {
        let layout = gray(100, 60);
        let at = |w: f64, h: f64| {
            Shortest::of(&Matrix {
                a: w,
                b: 0.0,
                c: 0.0,
                d: h,
                e: 0.0,
                f: 0.0,
            })
        };
        let once = at(72.0, 36.0).expect("a placement");
        // 100 samples over one inch is 100 ppi; 60 over half an inch, 120.
        assert_eq!(reduction(&layout, once, 50.0), (2, 3));
        assert_eq!(reduction(&layout, once, 40.0), (3, 3));
        // Twice as large elsewhere does not matter: the finest placement does.
        let twice = at(144.0, 72.0).expect("a placement").finer(&once);
        assert_eq!(reduction(&layout, twice, 40.0), (3, 3));
        assert_eq!(reduction(&layout, once, 1000.0), (1, 1));
        assert_eq!(reduction(&layout, once, 0.001), (100, 60));
        // A degenerate placement measures nothing; a turned one measures its
        // own axes.
        assert_eq!(at(0.0, 0.0), None);
        let turned = Shortest::of(&Matrix {
            a: 0.0,
            b: 72.0,
            c: -36.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        })
        .expect("a placement");
        assert_eq!(reduction(&layout, turned, 50.0), (2, 3));
    }

    /// 12.5.5: a box mapped onto a rectangle twice its size and elsewhere.
    #[test]
    fn an_appearance_is_fitted_to_its_rectangle() {
        let a = appearance_matrix(
            [100.0, 200.0, 140.0, 220.0],
            [0.0, 0.0, 20.0, 10.0],
            &Matrix::IDENTITY,
        )
        .expect("a matrix");
        assert_eq!(a.apply(0.0, 0.0), (100.0, 200.0));
        assert_eq!(a.apply(20.0, 10.0), (140.0, 220.0));
    }
}
