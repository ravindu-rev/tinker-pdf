//! Resolving a page's resources for the interpreter and the renderer.
//!
//! The two seams — `FontSource` for the interpreter, `GlyphSource` for the
//! renderer — exist so those crates never see a PDF dictionary. This is where
//! the dictionaries actually get read.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use tinker_pdf_color::{icc, ColorSpace, Colorant, Colorants, Function, Process};
use tinker_pdf_content::{
    ColorantKind, ColorantTints, Device, DeviceDefaults, FontSource, Layer, MarkedProps, Matrix,
    PathSegment, RenderingIntent, Rgb, Rgb16,
};
use tinker_pdf_cos::{
    decode_text_string, font as cos_font, limits, pages as cos_pages, Cached, CosDocument,
    CosError, Counting, DecodedStreams, Dict, Filled, Flights, MultipleMasterInstance, Name,
    ObjRef, Object, WarningKind,
};
use tinker_pdf_filters::{
    ccitt_decode, jbig2_decode, jpeg_decode, jpeg_decode_strips_sized, CcittParams, Jbig2Params,
    JpegColor, JpegLayout, JpegScale, JpegSink, Warning as FilterWarning, MAX_JBIG2_SYMBOL_PIXELS,
    MAX_JBIG2_WORK_MULTIPLE,
};
use tinker_pdf_font::cmap::collection_character;
use tinker_pdf_font::{
    base_glyph_name, cff::Cff, glyf, substitute_glyph, BaseEncoding, Outline, Sfnt, Type1,
};
use tinker_pdf_pool::{Executor, Parallelism};
use tinker_pdf_raster::canvas::{Canvas, Color};
use tinker_pdf_raster::fill::Mask;
use tinker_pdf_raster::image::{sampling_for, ImageSource, SourceWindow, Transform};
use tinker_pdf_render::{
    colorant_tints, tint_byte, DecodedImage, FontIdentity, GlyphSource, ImagePlates, ImageRequest,
    Mesh, MeshParams, PatternPaint, PlatePlan, Reduced, Shading, TilingPattern, TwoColourSamples,
};

use crate::cache::{
    self, DocumentCache, Fill, FormKey, FrameKey, IccKey, ImageKey, KeptForm, ProfileKey, Written,
};
use crate::fonts::{self, FontProvider, FontRequest};
use crate::optional::OptionalContent;
use crate::stats::{self, Counter, Work};
use crate::{LayerOptions, Usage};

/// Extracted glyph outlines, keyed by font identity and character code.
///
/// `None` records that a glyph was looked for and is not there, so a missing
/// one costs the extraction attempt once rather than on every occurrence.
/// Beside it, the font the extraction reported as resolving no glyph for the
/// code, if it did — so a later render that meets the code from the cache
/// says what the first one said ([`PageResources::for_one_render`]).
type OutlineCache = Flights<(u64, u32), (Option<Arc<Outline>>, Option<String>)>;

/// The most image names [`Caches::images`] keeps. A page names what it names,
/// so an honest one never reaches it; past it each draw of a new name decodes
/// it again, as before the cache.
const MAX_CACHED_IMAGES: usize = 256;

/// The most glyph outlines [`Caches::outlines`] keeps: a hostile document
/// could ask for millions of codes.
const MAX_CACHED_OUTLINES: usize = 1 << 16;

/// A decoded image, or the codec that could not decode it, and the damage the
/// decode tolerated as `(name, what was tolerated)`.
///
/// The failure is kept in the words the decode used and the damage beside
/// the picture, because both are reported **per render**: a render that meets
/// the image from the cache reports what the decode reported, rather than
/// nothing (the damage) or the resource name standing in for a codec (the
/// failure, which is what a cached `None` used to come back as).
///
/// A name of `None` is the image's own, said again under the name the render
/// that meets it gives the image: one image XObject named two ways in two
/// scopes is one entry of the document's cache (`crate::cache`), and each
/// render reports it by its own name. `Some` is a name the decode gave
/// something else, a soft mask's `SMask`, said again as it was.
#[derive(Clone)]
pub(crate) struct CachedImage {
    image: Result<Arc<DecodedImage>, String>,
    damage: Vec<(Option<String>, String)>,
    /// What [`PageResources::repaired_spaces`] gained while the image
    /// decoded, said again by every render that meets the image here, as
    /// `damage` is.
    repairs: Vec<(Option<String>, String)>,
}

impl CachedImage {
    /// What `decoded` and the scope it decoded through, `decoding`, reported
    /// for the image `label` names: every damage and repair `decoding` noted,
    /// each under `None` where it was noted under `label`.
    fn of(
        decoded: &Result<DecodedImage, String>,
        decoding: &PageResources,
        label: &str,
    ) -> CachedImage {
        let named = |(name, what): (String, String)| ((name != label).then_some(name), what);
        CachedImage {
            image: match decoded {
                Ok(image) => Ok(Arc::new(image.clone())),
                Err(codec) => Err(codec.clone()),
            },
            damage: decoding.damaged_images().into_iter().map(named).collect(),
            repairs: decoding.repaired_spaces().into_iter().map(named).collect(),
        }
    }

    /// What the entry is charged beside the cache's own bookkeeping: the
    /// samples, alpha and two-colour bytes at the decoded size, and the
    /// words it keeps.
    fn bytes(&self) -> u64 {
        let pixels = match &self.image {
            Ok(image) => {
                image.rgb.len()
                    + image.alpha.len()
                    + image.two_colour.as_ref().map_or(0, |two| two.samples.len())
            }
            Err(codec) => codec.len(),
        };
        let words = self
            .damage
            .iter()
            .chain(&self.repairs)
            .map(|(name, what)| name.as_ref().map_or(0, String::len) + what.len())
            .sum::<usize>();
        u64::try_from(pixels.saturating_add(words)).unwrap_or(u64::MAX)
    }

    /// Says again, to `resources`, what the decode reported, the image's own
    /// name being `label`.
    fn say_again(&self, resources: &PageResources, label: &str) {
        let named = |name: &Option<String>| name.clone().unwrap_or_else(|| label.to_string());
        for (name, what) in &self.damage {
            resources.note_damaged_image((named(name), what.clone()));
        }
        for (name, what) in &self.repairs {
            resources.note_repaired_space((named(name), what.clone()));
        }
    }

    /// The image, or the codec that refused it, as a render is handed it.
    fn handed(&self) -> Result<Option<DecodedImage>, String> {
        match &self.image {
            Ok(image) => Ok(Some((**image).clone())),
            Err(codec) => Err(codec.clone()),
        }
    }

    /// What the decode was asked for ([`Asked`]): its grid's factor and the
    /// window it holds, and every sample at full size for a refusal.
    fn asked(&self) -> Asked {
        match &self.image {
            Ok(image) => Asked::of(image),
            Err(_) => Asked::WHOLE,
        }
    }
}

/// One image name's decodes in [`Caches::images`].
///
/// The first decode of a name reads what its frame is ([`Frame`]), and every
/// later draw of the name reads what it asks of the decode from that; an
/// image with no frame is decoded whole whatever a draw asks. It reads too
/// how the image's colours take a rendering intent ([`ImageColour`]), so a
/// later draw knows the conversion it asks for before anything is decoded.
/// Each decode is kept under what it was asked for ([`Asked`]) — the
/// reduction it was made at, and the window it holds — and the conversion its
/// colours took, where they take one: one image drawn under two intents is two
/// conversions (`docs/design/icc.md`, "What compiling per intent costs"). The
/// name's first decode is its first ask's in its first intent, so the render
/// that makes it decodes the image once; each other ask and intent fills once
/// too, up to [`MAX_DECODES_OF_ONE_IMAGE`] of them.
struct ImageEntry {
    /// The frame a request can reduce, or `None` where none can.
    frame: Option<Frame>,
    /// How the image's colours take an intent.
    colour: ImageColour,
    /// The decodes made, by what each was asked for and [`ImageColour::key`].
    decoded: Flights<(Asked, Option<icc::Conversion>), CachedImage>,
}

/// The most decodes [`ImageEntry`] keeps of one image: a name has four sizes
/// as a JPEG and thirty-two as a JPEG 2000 codestream, and a codestream a
/// window for every tile a list's renders draw, each in every conversion its
/// colours take. Past it each new ask decodes
/// for its draw alone, as before the cache.
const MAX_DECODES_OF_ONE_IMAGE: usize = 64;

/// What a draw asked an image's decode for, as [`ImageEntry`] keeps it: how
/// many of the image's own samples each way one decoded sample stands for —
/// 1 at full size, 2, 4 and 8 for a JPEG at a half, a quarter and an eighth,
/// `2^d` for a codestream with `d` levels dropped, which is
/// [`Reduced::factor`] — and the rectangle of the decoded image it holds,
/// where it holds less than all of it ([`DecodedImage::window`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Asked {
    reduction: u32,
    window: Option<SourceWindow>,
}

impl Asked {
    /// Every sample at full size.
    const WHOLE: Asked = Asked {
        reduction: 1,
        window: None,
    };

    /// What `image` was decoded as.
    fn of(image: &DecodedImage) -> Asked {
        Asked {
            reduction: image.reduced.map_or(1, |grid| grid.factor),
            window: image.window,
        }
    }
}

/// What an image's decode found it to be, which every later draw of it reads
/// what it asks of the decode against ([`ImageEntry::frame`]).
///
/// Only an image whose codec can answer a draw with fewer samples has one,
/// and only one nothing else samples against its whole grid: a mask
/// (11.6.5.3, 8.9.6.3, 8.9.6.4) is drawn for the grid it was made for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// A JPEG of its frame's own `width` by `height` samples, decoded at the
    /// smallest of an eighth, a quarter and a half that covers the draw
    /// ([`jpeg_scale`]); none under an `/Indexed` space, whose samples are
    /// indices (8.6.6.3) a smaller scale would average.
    Jpeg { width: u32, height: u32 },
    /// A JPEG 2000 codestream, decoded with the levels and the window a draw
    /// asks ([`JpxFrame::ask`]).
    Jpx(JpxFrame),
}

impl Frame {
    /// How many of the image's own samples each way one decoded sample
    /// stands for in the decode a draw that asks as `request` does is handed,
    /// its decode's samples to take at most `room` bytes where a budget is
    /// stated: [`Frame::asked`]'s reduction, which a command stream's replay
    /// picks its decode by (`docs/design/command-stream.md`).
    pub(crate) fn reduction(self, request: &ImageRequest, room: Option<Room>) -> u32 {
        self.asked(request, room).reduction
    }

    /// Every reduction a draw of an image of this frame can be handed a
    /// decode at, each with a request that asks for it and holds the whole
    /// image: what a command stream holds a decode at. A JPEG's four scales,
    /// each asked by a drawn size of exactly its samples (`jpeg_scale`), and
    /// a codestream's `d` levels dropped for every `d` its header allows that
    /// leaves a sample a side (`jpx_discard`), each asked by `W >> d` by
    /// `H >> d`; the full size is the infinite request's. A budget only ever
    /// takes a draw to one of these.
    pub(crate) fn reductions(self) -> Vec<(u32, ImageRequest)> {
        let asking = |across: f64, down: f64| ImageRequest {
            drawn: (across, down),
            reach: None,
            visible: None,
        };
        let candidates: Vec<ImageRequest> = match self {
            Frame::Jpeg { width, height } => {
                [JpegScale::Half, JpegScale::Quarter, JpegScale::Eighth]
                    .into_iter()
                    .map(|scale| {
                        let samples =
                            |side: u32| f64::from(side) * f64::from(scale.block_samples()) / 8.0;
                        asking(samples(width), samples(height))
                    })
                    .collect()
            }
            Frame::Jpx(frame) => (1..=frame.levels.min(31))
                .map(|d| asking(f64::from(frame.width >> d), f64::from(frame.height >> d)))
                .collect(),
        };
        let mut out = vec![(1, ImageRequest::FULL)];
        for request in candidates {
            let reduction = self.reduction(&request, None);
            if !out.iter().any(|(held, _)| *held == reduction) {
                out.push((reduction, request));
            }
        }
        out
    }

    /// What a draw that asks as `request` does asks the decode for, its
    /// decode's samples to take at most `room` bytes where a budget is stated
    /// ([`Room`]): the draw's own ask where they fit or no budget is stated,
    /// and otherwise the least reduction past it whose decode fits
    /// ([`jpeg_scale_within`], [`JpxFrame::ask_within`]).
    fn asked(self, request: &ImageRequest, room: Option<Room>) -> Asked {
        match self {
            Frame::Jpeg { width, height } => Asked {
                reduction: reduction(jpeg_scale_within((width, height), request, room)),
                window: None,
            },
            Frame::Jpx(frame) => {
                let (discard, window) = frame.ask_within(request, room);
                Asked {
                    reduction: 1 << discard.min(31),
                    window,
                }
            }
        }
    }
}

/// The bytes a decoded image's samples may take and the decode still be kept
/// under the document's budget (`docs/design/performance.md`, P-16,
/// "Reduced-resolution decode"): the budget less what its entry is charged
/// beside them — the cache's 64 bytes and its key's defaults
/// (`cache::ImageKey::painted`). A function of the budget and the key alone,
/// never of what the cache holds, so the reduction a draw takes is the same
/// whatever was drawn before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Room(pub(crate) u64);

impl Room {
    /// The room under `budget` for an image whose key's defaults are
    /// `key_bytes` long.
    fn under(budget: u64, key_bytes: u64) -> Room {
        Room(budget.saturating_sub(key_bytes.saturating_add(cache::ENTRY_BYTES)))
    }

    /// Whether `pixels` samples of `per` bytes each fit.
    fn holds(self, pixels: u64, per: u64) -> bool {
        pixels.saturating_mul(per) <= self.0
    }
}

/// What a draw asks an image's decode for beside the image itself: the size
/// it is drawn at and the parts of it the draw reaches and shows, the
/// conversion its colours take (8.6.5.8, Table 89), and the bytes the
/// document's budget leaves its samples where the document keeps the decode
/// ([`Room`]) — `None` for a decode its draw alone holds: an inline image, a
/// soft mask, a render with no budget.
#[derive(Clone, Copy)]
struct DecodeAsk<'a> {
    request: &'a ImageRequest,
    conversion: icc::Conversion,
    room: Option<Room>,
    /// The plate plan of a separations render the image is decoded for,
    /// whose plates its samples' tints are kept on beside their light
    /// (RC-04.12, [`ImagePlating`]); `None` for every other decode.
    plates: Option<&'a PlatePlan>,
}

/// A JPEG 2000 codestream as its header and its dictionary give it, which
/// every draw of it reads the levels it drops and the window it holds from
/// (`docs/design/jpx-speed.md`, "The page's request").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JpxFrame {
    /// Its image's own width and height, `Xsiz − XOsiz` by `Ysiz − YOsiz`.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `XOsiz` and `YOsiz`, which the reduced grids are cut from (T.800
    /// B.5).
    pub(crate) origin: (u32, u32),
    /// The most levels a decode of it can drop
    /// ([`tinker_pdf_filters::JpxHeader::levels`]), and none under an
    /// `/Indexed` space, whose samples are indices (8.6.6.3) a lower
    /// resolution would average.
    pub(crate) levels: u8,
    /// Whether every sample is opaque — no opacity channel that
    /// `/SMaskInData` puts to use — so that a window may hold the part a draw
    /// shows rather than all it reaches ([`ImageRequest::visible`]).
    pub(crate) opaque: bool,
    /// The dictionary's `/Interpolate`, which the sampling policy reads and
    /// the window's margin with it.
    pub(crate) interpolate: bool,
}

impl JpxFrame {
    /// The levels a draw that asks as `request` does is decoded with dropped
    /// ([`jpx_discard`]), and the window of the reduced image it holds
    /// ([`JpxFrame::window`]).
    fn ask(&self, request: &ImageRequest) -> (u8, Option<SourceWindow>) {
        let discard = jpx_discard((self.width, self.height), self.levels, request);
        (discard, self.window(discard, request))
    }

    /// [`JpxFrame::ask`], its decode's samples to take at most `room` bytes
    /// where a budget is stated (`docs/design/performance.md`, P-16,
    /// "Reduced-resolution decode"): the draw's own levels where the image
    /// reduced by them fits or no budget is stated; otherwise the fewest
    /// levels dropped past them, up to all the codestream can drop, whose
    /// reduced image fits; and the draw's own where none does, which is
    /// decoded as it always was and not kept. Each with the window that many
    /// levels down the draw holds ([`JpxFrame::window`]).
    ///
    /// What fits is read off the whole reduced image, never the window: the
    /// window is the canvas's — a tile's, a band's — and the levels a page
    /// draws an image at are one for every part of it (ruling 5), so a tile
    /// is drawn from the samples the page is. An image whose samples are
    /// indices drops none (`levels` is 0), so it is always the draw's own.
    fn ask_within(&self, request: &ImageRequest, room: Option<Room>) -> (u8, Option<SourceWindow>) {
        let own = self.ask(request);
        let Some(room) = room else {
            return own;
        };
        let fits = |d: u8| {
            let (columns, rows) = self.reduced(d);
            room.holds(u64::from(columns) * u64::from(rows), self.sample_bytes())
        };
        if fits(own.0) {
            return own;
        }
        (own.0.saturating_add(1)..=self.levels.min(31))
            .find(|d| fits(*d))
            .map_or(own, |d| (d, self.window(d, request)))
    }

    /// The bytes a decoded sample takes: three of RGB, and a fourth of
    /// opacity where `/SMaskInData` puts the codestream's own to use.
    fn sample_bytes(&self) -> u64 {
        if self.opaque {
            3
        } else {
            4
        }
    }

    /// The reduced image's own width and height once `d` levels are dropped:
    /// its area on the reference grid divided by `2^d`, rounded up at both
    /// ends (T.800 B.5).
    fn reduced(&self, d: u8) -> (u32, u32) {
        let reduce = |v: u64| v.div_ceil(1u64 << d.min(31));
        let side = |origin: u32, size: u32| {
            let origin = u64::from(origin);
            let end = origin + u64::from(size);
            u32::try_from(reduce(end) - reduce(origin)).unwrap_or(u32::MAX)
        };
        (
            side(self.origin.0, self.width),
            side(self.origin.1, self.height),
        )
    }

    /// The rectangle of the image reduced by `2^d` a draw that asks as
    /// `request` does holds (`docs/design/jpx-speed.md`, "The visible
    /// window"), or `None` for all of it.
    ///
    /// The part of the unit square the draw shows — what it reaches, for an
    /// image with opacity of its own ([`ImageRequest::visible`]) — in the
    /// reduced image's samples, as [`Reduced`] places them: `skip + 2^d · X`
    /// of the image's columns for sample `X`. It is widened each way by the
    /// sampler's reach, `(⌈footprint⌉ + 2) · 2^(h + 1)` samples for the area
    /// filter's footprint after `h` halvings of the pyramid — a sample more
    /// than a bilinear tap or an area tap reaches, and twice the level a
    /// rounding of the drawn size could move the pyramid by — and its edges
    /// are moved out to multiples of `2^(h + 1)`, so every pair the pyramid
    /// averages inside it is the whole image's pair; the sampling policy is
    /// worked out over the whole reduced image, as the draw works it out
    /// (`ImageSource::window`). A draw that shows nothing holds one sample,
    /// so a window's nearest-sample stand-in still covers every pixel the
    /// image's square reaches; a window that is all of the image is none.
    fn window(&self, d: u8, request: &ImageRequest) -> Option<SourceWindow> {
        let area = if self.opaque {
            request.visible
        } else {
            request.reach
        }?;
        let (columns, rows) = self.reduced(d);
        if columns == 0 || rows == 0 || self.width == 0 || self.height == 0 {
            return None;
        }
        let factor = f64::from(1u32 << d.min(31));
        let skip = (jpx_skip(self.origin.0, d), jpx_skip(self.origin.1, d));
        // The device lengths of the samples' own square, the unit square's
        // times the part of it the samples span (`reduced_square`), which
        // the sampler reads its policy off.
        let span = |samples: u32, full: u32| f64::from(samples) * factor / f64::from(full);
        let placed = Transform {
            a: request.drawn.0 * span(columns, self.width),
            b: 0.0,
            c: 0.0,
            d: request.drawn.1 * span(rows, self.height),
            e: 0.0,
            f: 0.0,
        };
        let whole = ImageSource {
            width: columns,
            height: rows,
            rgb: &[],
            alpha: &[],
            two_colour: None,
            window: None,
        };
        let sampling = sampling_for(&whole, &placed, self.interpolate);
        // One axis: `lo` to `hi` of the unit square's side, `full` of the
        // image's samples from `skip` in, onto `samples` of the reduced
        // image's.
        let axis = |lo: f64, hi: f64, full: u32, skip: u32, samples: u32, footprint, halvings| {
            let at = |t: f64| (t * f64::from(full) - f64::from(skip)) / factor;
            // Within `i64` whatever the inputs: `t` is in [0, 1].
            let (first, last) = (at(lo).floor() as i64, at(hi).ceil() as i64);
            if hi <= lo {
                let one = first.clamp(0, i64::from(samples) - 1);
                return (one, one + 1);
            }
            let level = 1i64 << (u32::min(halvings, 30) + 1);
            let footprint: u64 = footprint;
            let texels = i64::try_from(footprint.div_ceil(1 << 16)).unwrap_or(i64::MAX / 4);
            let margin = texels.saturating_add(2).saturating_mul(level);
            let start = first.saturating_sub(margin);
            let start = start.saturating_sub(start.rem_euclid(level));
            let end = last.saturating_add(margin);
            let end = end.saturating_add((level - end.rem_euclid(level)) % level);
            (
                start.clamp(0, i64::from(samples)),
                end.clamp(0, i64::from(samples)),
            )
        };
        let (x0, x1) = axis(
            area.u0,
            area.u1,
            self.width,
            skip.0,
            columns,
            sampling.footprint.0,
            sampling.halvings.0,
        );
        // Rows run down the image and `v` up its square.
        let (y0, y1) = axis(
            1.0 - area.v1,
            1.0 - area.v0,
            self.height,
            skip.1,
            rows,
            sampling.footprint.1,
            sampling.halvings.1,
        );
        let whole_image = x0 == 0 && y0 == 0 && x1 == i64::from(columns) && y1 == i64::from(rows);
        if whole_image {
            return None;
        }
        // Each lies in `0..=samples` of a `u32` side, so each fits.
        let fit = |v: i64| u32::try_from(v).unwrap_or(0);
        Some(SourceWindow {
            x: fit(x0),
            y: fit(y0),
            width: fit(x1 - x0),
            height: fit(y1 - y0),
        })
    }
}

/// How an image XObject's colours take a rendering intent (8.6.5.8, Table
/// 89): the intent its own `/Intent` names, which decides for the image where
/// it names one, and whether any intent changes its colours at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ImageColour {
    /// The image's `/Intent`, where that is a name, one 8.6.5.8 does not list
    /// kept as unrecognised; `None` where the entry is absent or not a name,
    /// which leaves the intent in force at the draw deciding, and for an image
    /// mask, whose `/Intent` is never read.
    own: Option<RenderingIntent>,
    /// Whether converting the image's samples reads an intent: it is no image
    /// mask, and its space as painted holds an ICC profile
    /// ([`reads_an_intent`]). An image whose colours no intent changes is one
    /// decode under every intent.
    by_intent: bool,
}

impl ImageColour {
    /// The conversion the image's colours take in a draw where `state` is
    /// the intent in force: its own intent's where it names one.
    fn conversion(self, state: RenderingIntent) -> icc::Conversion {
        self.own.unwrap_or(state).conversion()
    }

    /// What a decode for a draw in `state` is kept under beside its
    /// reduction: the conversion its colours take where an intent changes
    /// them, and nothing where none does, so one decode serves every draw.
    fn key(self, state: RenderingIntent) -> Option<icc::Conversion> {
        self.by_intent.then(|| self.conversion(state))
    }
}

/// Whether converting a colour of `space` reads a rendering intent: an ICC
/// space's does, and so does a space's that holds one — an `/Indexed` base, a
/// `/Separation` or `/DeviceN` alternate, an uncoloured pattern's underlying
/// space. `ColorSpace::to_rgb_in` converts every other space as `to_rgb`
/// converts it (`docs/design/icc.md`, "Where each conversion takes its
/// intent").
fn reads_an_intent(space: &ColorSpace) -> bool {
    match space {
        ColorSpace::Icc { .. } => true,
        ColorSpace::Indexed { base, .. } => reads_an_intent(base),
        ColorSpace::Separation { alternate, .. } => reads_an_intent(alternate),
        ColorSpace::Pattern { base } => base.as_deref().is_some_and(reads_an_intent),
        ColorSpace::DeviceGray
        | ColorSpace::DeviceRgb
        | ColorSpace::DeviceCmyk
        | ColorSpace::Approximated { .. }
        | ColorSpace::CalGray { .. }
        | ColorSpace::CalRgb { .. }
        | ColorSpace::Lab { .. } => false,
    }
}

/// An image XObject decoded for one draw ([`PageResources::decode_image`]),
/// the frame the draws after it read their size against
/// ([`ImageEntry::frame`]), and how its colours take an intent
/// ([`ImageEntry::colour`]).
struct Decoded {
    image: Result<DecodedImage, String>,
    frame: Option<Frame>,
    colour: ImageColour,
}

/// The scale a JPEG of `frame` samples, its own width and height, is decoded
/// at for a draw that asks as `request` does (`docs/design/jpeg-speed.md`,
/// "The scale rule"): the smallest of an eighth, a quarter and a half whose
/// image still has at least as many samples as the draw has pixels along both
/// axes, so the reduced grid is never drawn magnified, and full size where
/// none has or where the drawn size is not finite.
///
/// `W·N/8` is compared as it stands rather than rounded up: it is exact in
/// `f64` — a sixteen-bit width times at most four, over a power of two — so
/// the comparison is the same on every target.
fn jpeg_scale(frame: (u32, u32), request: &ImageRequest) -> JpegScale {
    let (across, down) = request.drawn;
    if !(across.is_finite() && down.is_finite()) {
        return JpegScale::Full;
    }
    let holds = |samples: u32, scale: JpegScale, drawn: f64| {
        f64::from(samples) * f64::from(scale.block_samples()) / 8.0 >= drawn
    };
    [JpegScale::Eighth, JpegScale::Quarter, JpegScale::Half]
        .into_iter()
        .find(|&scale| holds(frame.0, scale, across) && holds(frame.1, scale, down))
        .unwrap_or(JpegScale::Full)
}

/// [`jpeg_scale`], the decode's samples to take at most `room` bytes where a
/// budget is stated (`docs/design/performance.md`, P-16, "Reduced-resolution
/// decode"): the draw's own scale where its decode fits or no budget is
/// stated; otherwise the largest of the smaller scales whose decode — three
/// bytes a sample of `⌈W·N/8⌉ × ⌈H·N/8⌉` ([`JpegScale::length`]) — fits;
/// and the draw's own where none does, which is decoded as it always was and
/// not kept.
fn jpeg_scale_within(frame: (u32, u32), request: &ImageRequest, room: Option<Room>) -> JpegScale {
    let own = jpeg_scale(frame, request);
    let Some(room) = room else {
        return own;
    };
    let fits = |scale: JpegScale| {
        let pixels = u64::from(scale.length(frame.0)) * u64::from(scale.length(frame.1));
        room.holds(pixels, 3)
    };
    if fits(own) {
        return own;
    }
    JpegScale::ALL
        .into_iter()
        .filter(|scale| scale.block_samples() < own.block_samples())
        .find(|scale| fits(*scale))
        .unwrap_or(own)
}

/// How many of an image's own samples each way one decoded sample stands for
/// at `scale`: 1 at full size, 2, 4 and 8 at a half, a quarter and an eighth
/// — what [`ImageEntry`] keys a decode by, and [`Reduced::factor`].
fn reduction(scale: JpegScale) -> u32 {
    8 / scale.block_samples()
}

/// The resolution levels a JPEG 2000 codestream of `size` samples, which can
/// drop `levels` of them, is decoded with dropped for a draw that asks as
/// `request` does (`docs/design/jpx-speed.md`, "The page's request"): the most
/// levels `d` for which `W >> d` by `H >> d` — the fewest samples the image
/// reduced by `2^d` has, whatever its origin on the reference grid (T.800
/// B.5) — is still at least as many samples as the draw has pixels along both
/// axes, and at least one, so the reduced grid is never drawn magnified; none
/// where the drawn size is not finite; and never more than `levels`, so the
/// figure is the discard the decoder applies. A shift past 31 would leave no
/// sample of a `u32` side, so none is asked for.
///
/// `W >> d` is compared as it stands: an integer under `2^32`, which `f64`
/// holds exactly, so the comparison is the same on every target.
fn jpx_discard((width, height): (u32, u32), levels: u8, request: &ImageRequest) -> u8 {
    let (across, down) = request.drawn;
    if !(across.is_finite() && down.is_finite()) {
        return 0;
    }
    let holds = |samples: u32, d: u8, drawn: f64| f64::from(samples >> d) >= drawn.max(1.0);
    let mut d = 0u8;
    while d < levels.min(31) && holds(width, d + 1, across) && holds(height, d + 1, down) {
        d += 1;
    }
    d
}

/// How many of a JPEG 2000 image's own columns, or rows, stand before its
/// first decoded sample's cell once `d` levels are dropped
/// ([`Reduced::skip`]): the grid reduced by `2^d` puts that sample at
/// `ceil(origin / 2^d)`, whose cell covers the full grid from `2^d` times it
/// (T.800 B.5), and the image starts at `origin`. Less than a cell, so it
/// fits.
fn jpx_skip(origin: u32, d: u8) -> u32 {
    let cell = 1u64 << d.min(31);
    let start = u64::from(origin).div_ceil(cell) * cell;
    u32::try_from(start - u64::from(origin)).unwrap_or(0)
}

/// The ceiling every JPEG 2000 decode and header read here is held to: a
/// decoded stream's.
fn jpx_limits() -> tinker_pdf_filters::Limits {
    tinker_pdf_filters::Limits::new(limits::MAX_DECODED_STREAM)
}

/// What a page's resources work out once and every render of the page can
/// reuse: the fonts its names select, font programs, decoded images, compiled
/// ICC transforms, glyph outlines and the nested scopes of forms.
///
/// Behind one `Arc` so a retained page can give each render resources of its
/// own — fresh lists of what that render had to tolerate — over the same
/// caches ([`PageResources::for_one_render`]).
///
/// **The five dear ones fill once however many renders ask at once**: font
/// programs, images, ICC transforms, outlines and form scopes are
/// [`Flights`] (`docs/design/parallelism.md`, "Single-flight fills"), so the
/// first render of a display list to miss a key computes it while the renders
/// beside it on other threads wait for that value, and a render with a fill of
/// its own open computes for itself rather than waiting, so no fill waits on
/// another and none can deadlock. A render that met a key another filled
/// counts a hit and says again what the fill reported, so a hit is exactly
/// what a miss computes, warnings included.
#[derive(Default)]
struct Caches {
    /// The embedded font program of each font, by the id the interpreter uses.
    ///
    /// **Decoded on first use, not at construction.** A font program is an
    /// inflate of a stream that is routinely a megabyte, and since a form
    /// XObject's own `/Resources` became a scope of their own there is one of
    /// these per form rather than one per page — a document of nine hundred
    /// objects went from 537 ms to 24.7 s doing it eagerly, and one corpus
    /// file stopped making progress at all. Most scopes are opened to resolve
    /// an image or a pattern and never ask for a glyph.
    ///
    /// `None` is cached as firmly as a hit: a font with no embedded program
    /// must not be looked up again on every glyph.
    ///
    /// Keyed by the id, which is a resource name and means nothing outside
    /// this scope, so no other scope shares it; the program is read through
    /// the operation's decoded streams ([`Operation`]), keyed by the stream,
    /// so a program the scopes of one operation draw — a page's and its
    /// forms', or every page's of a save — is decoded once between them, and
    /// the entry here is the operation's bytes, not a copy of them.
    programs: Flights<u64, Option<FontProgram>>,
    /// Decoded images, kept because a page may draw one many times: by the
    /// resource name, each name's decodes by the reduction they were made at
    /// and the conversion their colours took (`ImageEntry`), so a hit hands
    /// back exactly what that size's miss in that intent computes
    /// (`docs/design/jpeg-speed.md`, "The scale rule";
    /// `docs/design/jpx-speed.md`, "The page's request").
    images: Flights<Vec<u8>, Arc<ImageEntry>>,
    /// Compiled ICC transforms, by the profile stream's object number.
    ///
    /// A transform is three 4 096-entry tables, and a page may name the same
    /// `ICCBased` space at every one of a thousand `cs` operators. The `None`
    /// is cached too: a profile that would not parse must be refused once
    /// rather than re-read and re-refused a thousand times.
    icc: IccCache,
    /// Outlines already extracted, keyed by font and code.
    outlines: OutlineCache,
    /// Nested scopes already built, by the form's own object number.
    ///
    /// A page may invoke one form a thousand times — a stamp, a rule, a
    /// letterhead — and building its resources is not cheap: every font in the
    /// dictionary is parsed and the optional-content configuration is bound
    /// again. Two pdf.js corpus files stopped making progress at all when this
    /// was rebuilt per invocation, which is what put the cache here.
    ///
    /// Keyed by the XObject's reference rather than by the resource name,
    /// because two names can reach one form and a name means nothing outside
    /// the dictionary it was looked up in. `None` is cached too: "this form
    /// brought no resources" is an answer worth not recomputing.
    form_scopes: Flights<u64, Option<Arc<PageResources>>>,
    /// The scope's default colour spaces (8.6.5.6), read on the first
    /// question it is asked about a device space.
    ///
    /// One `/ColorSpace` lookup and three keyed gets, once a scope, so that a
    /// page naming no default pays a load and a branch per device selection
    /// and nothing per operator — the interpreter asks once a scope too
    /// ([`FontSource::device_defaults`]).
    defaults: OnceLock<Defaults>,
    /// The colour space each resource name selects, read out of
    /// `/ColorSpace` on the first question about the name and kept with both
    /// of K-02.1's readings of it, as painted and as selected, and the
    /// repairs reading it noted, said again at each later question
    /// (`docs/design/performance.md`, P-02, "Colour spaces, functions and
    /// graphics states"). A device space named directly is matched by its
    /// name and kept nowhere. A name `/ColorSpace` does not hold is kept
    /// too, so it is looked for once.
    spaces: RwLock<HashMap<Vec<u8>, Arc<KeptSpace>>>,
    /// Every function this scope reads from an object of its own — a
    /// shading's `/Function` and its members, a colour space's tint
    /// transform, a stitching function's pieces, and the functions a
    /// graphics state names (`/TR`, `/BG`, `/UCR`, their second spellings,
    /// a halftone's and a soft mask's) — by its object: one function many
    /// shadings, spaces or states share, or one shading painted at every
    /// `sh`, is decoded and parsed once. A stitching function is kept by its
    /// object and the depth it was read at, `Some` here, because pieces
    /// nested past [`MAX_FUNCTION_DEPTH`] read as nothing, so its reading
    /// depends on where it stands; every other function reads alike at every
    /// depth, `None`. `None` is cached too. A function written directly into
    /// a dictionary has no object, is a few numbers, and is read where it
    /// lies.
    functions: Mutex<FunctionCache>,
    /// The fonts this scope's names select, each read on the first question
    /// about its name and kept by the name and by the id its glyphs carry
    /// (`docs/design/performance.md`, P-02, "The font table"). Shared by
    /// every render over these caches, so a retained page's renders read each
    /// font once between them.
    fonts: RwLock<FontTable>,
    /// Each graphics state a `gs` names in this scope, read on the first
    /// `gs` of the name: its keys, its functions and its soft mask's group,
    /// whose content is decoded then and not again. A page that sets one
    /// masked state before each of a hundred thousand paths decoded the
    /// mask's group at every one of them and stopped making progress
    /// (MS-20). What reading it repaired is kept beside it and said again at
    /// each `gs`, as [`CachedImage`] says an image's. `None` is cached too.
    states: Mutex<HashMap<Vec<u8>, CachedState>>,
    /// This scope's resource dictionary written exactly
    /// ([`cache::write_exact`]), on the first question that needs it: what
    /// the drawing of a form that brings no resources of its own is kept
    /// under beside its invocation (`cache::FormKey`).
    written: OnceLock<Written>,
}

/// A graphics state as [`Caches::states`] keeps it.
struct CachedState {
    state: Option<Arc<tinker_pdf_content::StateParameters>>,
    /// The colour spaces reading it repaired, said again at each `gs`.
    repaired_spaces: Vec<(String, String)>,
}

/// A resource name's colour space as [`Caches::spaces`] keeps it: K-02.1's
/// two answers to the one selection, read together on the first question
/// about the name.
struct KeptSpace {
    /// As painted, each device space in it put in as the scope's default
    /// (8.6.5.6): what every conversion reads.
    painted: Option<ColorSpace>,
    /// As the file selected it: what `cs` resets the colour to, its
    /// component count and whether a colour is DeviceCMYK ink.
    selected: Option<ColorSpace>,
    /// The repairs reading it noted, said again at each later question, so a
    /// render that meets it here says what the render that read it said.
    repaired_spaces: Vec<(String, String)>,
}

/// Which of K-02.1's two answers to a selection a question reads.
#[derive(Clone, Copy)]
enum Answer {
    /// The space as painted.
    Painted,
    /// The space as selected.
    Selected,
}

/// The most entries [`Caches::states`], [`Caches::spaces`] and
/// [`Caches::functions`] each keep. A resource table holds what it holds, so
/// an honest scope never reaches it; past it each question reads its entry
/// again, as before the caches.
const MAX_CACHED_ENTRIES: usize = 1 << 12;

/// The deepest a function is read: a stitching function's pieces (7.10.4),
/// and a colour space's tint transform, are one deeper than what holds them,
/// and one past this reads as nothing, so a function that holds itself ends.
const MAX_FUNCTION_DEPTH: u32 = 8;

/// One font a scope's content selects, read on the first question about the
/// name it selects it by (`docs/design/performance.md`, P-02, "The font
/// table").
pub(crate) struct ScopeFont {
    /// The name the content selects it by in this scope.
    name: Vec<u8>,
    /// The id the interpreter's glyphs carry: the name's interned id, or a
    /// reserved name's (`fonts::reserved_id`).
    id: u64,
    /// The `/Font` entry's reference, or the object a reserved name spells;
    /// `None` for a font written into the resource dictionary.
    object: Option<ObjRef>,
    /// The font's dictionary as resolved — for an indirect font the
    /// document's own object, so nothing is copied — which the program and a
    /// substitute's request are read from.
    dict: Arc<Object>,
    /// The font, as `cos_font::read_in` reads it.
    font: Arc<cos_font::Font>,
    /// Whether [`FontSource::type3_glyph`] reads it as a Type 3 font: its own
    /// `/Subtype` the name `/Type3`, as written.
    type3: bool,
    /// A Type 3 font's procedures, read on its first glyph and kept for every
    /// render over the scope.
    procedures: OnceLock<Type3Procedures>,
    /// The font's program parsed as Type 1 and drawn from, read on its first
    /// glyph drawn and kept for every render over the scope, so a page's
    /// glyphs decrypt it once rather than once a glyph; `None` inside for a
    /// program that is not one. An embedded multiple-master program is held
    /// at the instance its font names (`PageResources::type1_program`).
    type1: OnceLock<Option<Type1<'static>>>,
    /// The same program as read, at its default instance, for the glyph
    /// selection the usage walk asks for, which no instance moves and which
    /// sets none.
    type1_read: OnceLock<Option<Type1<'static>>>,
    /// The font's ascent and descent, read on the first question a box
    /// option asks of it ([`FontSource::font_extent`]) and kept.
    extent: OnceLock<Option<(f64, f64)>>,
}

/// What a Type 3 font's glyphs are drawn from (9.6.5), read once a scope on
/// its first glyph (`docs/design/performance.md`, P-02, "The Type 3 record").
struct Type3Procedures {
    /// Each code's glyph name, as [`type3_codes`] reads `/Differences`.
    names: HashMap<u32, Name>,
    /// `/CharProcs`, resolved; a glyph's procedure is the stream its name
    /// names there.
    procs: Arc<Object>,
    /// `/FontMatrix`'s six numbers, each a number where it lies; `None` when
    /// they are not six, which draws no glyph.
    matrix: Option<Matrix>,
}

/// A Type 3 font's `/Differences`, read as the interpreter reads it — item by
/// item from the first, a counter starting at 0: an integer sets it, a
/// negative one or one past `u32` to 0; a real sets it as `as u32` converts
/// it; a name is given to the counter's code, **the first name a code is
/// given the one it keeps**, and the counter steps on, held at `u32::MAX`;
/// every item is resolved before it is read, and an item of any other type is
/// passed over. Names are kept as the array holds them, never as text.
///
/// Not the cos `Font`'s table, which lets a later entry win, passes over an
/// indirect item and keeps lossy strings; `subset.rs`'s `names_of` and
/// `redact.rs`'s `carrying_procedures` read the array this way too.
fn type3_codes(doc: &CosDocument, differences: &[Object]) -> HashMap<u32, Name> {
    let mut names = HashMap::new();
    let mut code = 0u32;
    for item in differences {
        #[cfg(test)]
        DIFFERENCES_READ.with(|read| read.set(read.get().saturating_add(1)));
        match doc.resolve(item).as_ref() {
            Object::Int(v) => code = u32::try_from(*v).unwrap_or(0),
            Object::Real(v) => code = *v as u32,
            Object::Name(name) => {
                names.entry(code).or_insert(*name);
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }
    names
}

#[cfg(test)]
thread_local! {
    /// The `/Differences` items [`type3_codes`] has read on this thread, so a
    /// test holds a scope to reading its array once.
    static DIFFERENCES_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A scope's fonts, by the name its content selects each by and by the id
/// its glyphs carry. A name or an id that selects nothing is kept as `None`
/// while the table holds fewer than [`MAX_UNREAD_FONT_NAMES`] of each.
#[derive(Default)]
struct FontTable {
    by_name: HashMap<Vec<u8>, Option<Arc<ScopeFont>>>,
    by_id: HashMap<u64, Option<Arc<ScopeFont>>>,
}

/// The most names, and ids, that select no font a scope's table keeps. Every
/// font its `/Font` binds is kept, which the dictionary bounds; past this a
/// name that selects nothing is looked for again at each question, as before
/// the table.
const MAX_UNREAD_FONT_NAMES: usize = 1 << 12;

/// What a scope's `/ColorSpace` subdictionary puts in for each device space
/// (8.6.5.6): its `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK`, read as
/// written — a default's own spaces are never remapped again, which is the
/// reading that terminates: `/DefaultRGB /DeviceRGB` is a legal default.
///
/// `None` for a family the scope names no default for, names a refused one
/// for, or names its own device space for: three ways of painting the device
/// space as 8.6.4 converts it, the last without asking anything per operator.
#[derive(Default)]
struct Defaults {
    gray: Option<ColorSpace>,
    rgb: Option<ColorSpace>,
    cmyk: Option<ColorSpace>,
    /// Why each family's default was refused, gray, RGB and CMYK in that
    /// order, as `RepairedColorSpace`'s `reason` names it: what an image
    /// drawn in that family says it fell back from (`Page::images`).
    refused: [Option<&'static str>; 3],
    /// The defaults this scope puts in, each family's `/ColorSpace` entry as
    /// written ([`cache::write_exact`]) after the family's own name: what a
    /// decoded image the document's cache keeps is a function of beside its
    /// object (`ImageKey::painted`). Nothing for a scope that puts none in;
    /// a refused default paints its device space as itself and writes none.
    written: Written,
}

impl Defaults {
    /// What stands in for `device` in this scope, if anything does.
    fn for_device(&self, device: &ColorSpace) -> Option<&ColorSpace> {
        match device {
            ColorSpace::DeviceGray => self.gray.as_ref(),
            ColorSpace::DeviceRgb => self.rgb.as_ref(),
            ColorSpace::DeviceCmyk => self.cmyk.as_ref(),
            _ => None,
        }
    }

    /// Why this scope refused `device`'s default, if it named one it
    /// refused.
    fn refusal(&self, device: &ColorSpace) -> Option<&'static str> {
        match device {
            ColorSpace::DeviceGray => self.refused[0],
            ColorSpace::DeviceRgb => self.refused[1],
            ColorSpace::DeviceCmyk => self.refused[2],
            _ => None,
        }
    }

    /// Whether this scope puts nothing in and refuses nothing: the scope of
    /// almost every page, which an extraction then asks nothing more.
    fn is_inert(&self) -> bool {
        self.puts_nothing_in() && self.refused.iter().all(Option::is_none)
    }

    /// Whether this scope puts no default in for any device space, so that a
    /// space as painted is the space as selected, read alike.
    fn puts_nothing_in(&self) -> bool {
        self.gray.is_none() && self.rgb.is_none() && self.cmyk.is_none()
    }
}

/// What describing an image's space as painted met (8.6.5.6): whether a
/// device space in it was put in as the scope's default, and why any default
/// a device space in it fell back from was refused.
#[derive(Default)]
struct Painting {
    replaced: bool,
    refused: Vec<String>,
}

/// How [`PageResources::read_space`] reads a colour space.
#[derive(Clone, Copy)]
struct Reading<'a> {
    /// The defaults each device space in the space is read as — the space as
    /// painted (8.6.5.6) — and `None` for the space as selected: what `cs`
    /// resets to, whether a colour is DeviceCMYK ink, and a default's own
    /// value, which is never remapped again.
    ///
    /// Borrowed rather than asked of the scope reading, because the two can
    /// differ: a form's group `/CS` is remapped by the form's own resources,
    /// where its contents are painted, and read where `Do` runs, where what
    /// it tolerates is reported.
    defaults: Option<&'a Defaults>,
    /// A name that is no family resolves in this scope's `/ColorSpace`, as an
    /// inline image's may (8.9.7); everywhere else a name is a family.
    inline: bool,
    /// Whether a `/DeviceN`'s attributes are read (Table 70): where a space is
    /// selected, painted, or the base of an `/Indexed` or `/Pattern` space
    /// read so, and not in a separation's alternate or in a space an
    /// attributes dictionary holds — so a `/Colorants` entry naming its own
    /// space ends rather than fanning out.
    colorants: bool,
}

impl Reading<'_> {
    /// The space as the file selected it.
    const SELECTED: Reading<'static> = Reading {
        defaults: None,
        inline: false,
        colorants: true,
    };

    /// `device`, or the default this reading paints it in (8.6.5.6).
    ///
    /// A clone of the default, read once a scope. The device operators do
    /// not come here: [`FontSource::resolve_color`] borrows the default
    /// instead, so `k` under a `/DefaultCMYK` copies nothing.
    fn device(self, device: ColorSpace) -> ColorSpace {
        self.defaults
            .and_then(|defaults| defaults.for_device(&device))
            .cloned()
            .unwrap_or(device)
    }
}

/// Functions read from objects of their own ([`Caches::functions`]): by the
/// object, and for a stitching function the depth it was read at too.
type FunctionCache = HashMap<(ObjRef, Option<u32>), Option<Arc<Function>>>;

/// Compiled ICC transforms, by the profile stream's object number and
/// generation.
///
/// The `None` is cached as deliberately as the `Some`: a profile that will
/// not parse must be refused once rather than re-read at every `cs`.
type IccCache = Flights<(u32, u16), Option<Arc<tinker_pdf_color::icc::Transforms>>>;

/// Which glyph of an embedded program a code selects, and on whose authority.
///
/// The second half is what [`crate::subset`] rests on, and it is the whole
/// reason this is a struct rather than a `u16`. 9.6.6.4 ends its selection
/// order with a guess — *read the code as the glyph index* — which is what
/// every reader does with a subset font whose producer dropped the `cmap`, and
/// which is **not** a statement the font made. Two readers guessing are free
/// to guess differently: this one reads the code, another reads the
/// `/Differences` name through a `post` table the subsetter drops.
///
/// A glyph kept on the strength of a guess is therefore a glyph some other
/// reader may not draw, and — much worse — a glyph *dropped* because the guess
/// went elsewhere is a glyph that reader draws and no longer has. So
/// subsetting refuses a font any of whose shown codes landed here, and leaves
/// its program whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    /// The glyph index in the embedded program — for a Type 1 program, its
    /// charstring's position in the order `Type1::parse` reads them.
    pub glyph: u16,
    /// Whether a mapping the font itself states chose it — its `cmap` through
    /// a character, its `/CIDToGIDMap` through a CID, its CFF charset or its
    /// Type 1 encoding through a name — rather than 9.6.6.4's closing guess.
    pub stated: bool,
    /// A second glyph a subset keeps for the same code: the one a Type 1
    /// program's built-in encoding names where this renderer draws the
    /// code's text instead (`docs/design/subsetting.md`), which a reader that
    /// follows ISO 32000-2 9.6.5.2 draws.
    pub also: Option<u16>,
}

impl Selection {
    fn stated(glyph: u16) -> Selection {
        Selection {
            glyph,
            stated: true,
            also: None,
        }
    }

    fn guessed(glyph: u16) -> Selection {
        Selection {
            glyph,
            stated: false,
            also: None,
        }
    }
}

/// What one operation — a render, a text or an image extraction, a save's
/// font pass — carries into every resource scope it reads
/// (`docs/design/performance.md`, P-03, "The cache: who holds it, and its
/// key"): the streams it has decoded, so that it decodes each once however
/// many of its scopes read it, its tally, when it is counting, and the
/// optional-content states its scopes read, bound once for the usage it
/// names (`docs/design/render-usage.md`), and the threads its decodes may
/// spend.
///
/// Made by the door the operation goes through and cloned into every scope it
/// builds; a scope a scope makes inherits its maker's. A scope a cache keeps
/// outlives the operation that built it and holds none
/// ([`Operation::none`]); each asker is handed it bound to the asker's
/// ([`PageResources::bind`]), so no operation's streams outlive it.
#[derive(Clone)]
pub(crate) struct Operation {
    /// Every stream the operation's scopes read — a form's content, a Type 3
    /// glyph's procedure, a tiling pattern's cell, an appearance, a font's
    /// program and CMaps, a function's, a mesh's, an ICC profile, a lookup
    /// table, a JBIG2 image's globals — by its reference. `None` holds
    /// nothing: every read decodes. An image's samples are never here: the
    /// scope's image cache keeps the image.
    streams: Option<Arc<DecodedStreams>>,
    /// The tally, when the operation is counting.
    work: Option<Arc<Work>>,
    /// What the operation renders for (`crate::RenderOptions::usage`):
    /// `None` for a render naming none and for every operation that is not a
    /// render.
    usage: Option<Usage>,
    /// What the render says about the document's layers
    /// (`crate::RenderOptions::layers`): the configuration whose states it
    /// draws, and the groups it turns on or off. The default, `/D`'s states
    /// as they are, for every operation that is not a render.
    layers: LayerOptions,
    /// The render's magnification (`crate::RenderOptions::magnification`),
    /// read as 1 where it is not a finite number above zero, which the
    /// auto-state's `/Zoom` category reads: 1 for every operation that is not
    /// a render.
    magnification: f64,
    /// The optional-content states every scope the operation opens reads,
    /// bound by the first that asks (ISO 32000-2 8.11.4.4): the chosen
    /// configuration's states over `/D`'s initial ones, its `/AS` entries
    /// for `usage` over them, and the render's overrides last
    /// (`docs/design/optional-content-config.md`).
    /// Shared by every clone, so the page's scope, each form's, each tiling
    /// cell's and each appearance's read one binding.
    optional: Arc<OnceLock<OptionalContent>>,
    /// The document's cache, where its caller stated a budget for it
    /// (`crate::cache`): what every scope of the operation keeps across
    /// renders in place of keeping it for the render alone. `None` keeps
    /// everything the render's own, as it always was.
    cache: Option<Arc<DocumentCache>>,
    /// The threads a decode the operation makes may hand its independent
    /// parts to — a JPEG 2000 codestream's code-blocks, tiles, components and
    /// wavelet levels (`docs/design/parallelism.md`, "JPEG 2000: P-10's
    /// seam") — which is
    /// the caller's `RenderOptions::parallelism` for a render that asks, and
    /// `Serial` for every other operation. A decode made on one of the
    /// pool's workers runs its parts there, whatever this says.
    parallelism: Parallelism,
    /// Whether a glyph the operation selects that its font does not carry is
    /// reported nowhere: true for a text extraction, whose glyph lookups
    /// (`FontSource::glyph_id`) are asked for what a code stands for and not
    /// to draw it, so that they add no unresolved font to what the page's
    /// text says (`docs/design/extraction-options.md`). False for every
    /// other operation, a render reporting what it cannot draw.
    quiet_glyphs: bool,
}

impl Operation {
    /// A new operation, which has decoded nothing yet, counting into `work`
    /// when it is counting, naming no usage, its decodes serial.
    pub(crate) fn new(work: Option<Arc<Work>>) -> Operation {
        Operation::rendering(work, None)
    }

    /// A render's operation: [`Operation::new`], for a render naming
    /// `usage`, whose scopes read the states that usage gives.
    pub(crate) fn rendering(work: Option<Arc<Work>>, usage: Option<Usage>) -> Operation {
        Operation {
            streams: Some(Arc::new(DecodedStreams::new())),
            work,
            usage,
            layers: LayerOptions::default(),
            magnification: 1.0,
            optional: Arc::default(),
            cache: None,
            parallelism: Parallelism::Serial,
            quiet_glyphs: false,
        }
    }

    /// This operation, reporting no glyph its font does not carry: a text
    /// extraction's, whose glyph lookups say what a code stands for and draw
    /// nothing.
    pub(crate) fn reporting_no_glyphs(self) -> Operation {
        Operation {
            quiet_glyphs: true,
            ..self
        }
    }

    /// This operation, for a render at `magnification`, which its `/Zoom`
    /// recommendations read.
    pub(crate) fn at_magnification(self, magnification: f64) -> Operation {
        Operation {
            magnification: crate::optional::magnification(magnification),
            ..self
        }
    }

    /// This operation, binding the states `layers` gives — its
    /// configuration's and its overrides — rather than `/D`'s.
    pub(crate) fn with_layers(self, layers: LayerOptions) -> Operation {
        Operation { layers, ..self }
    }

    /// This operation, its decodes handing their independent parts to
    /// `parallelism`'s threads.
    pub(crate) fn on(self, parallelism: Parallelism) -> Operation {
        Operation {
            parallelism,
            ..self
        }
    }

    /// No operation: what a scope a cache keeps holds between the operations
    /// that ask for it. Nothing counts, every stream is decoded as it is
    /// read, serially, and nothing is kept for the document; asked about
    /// optional content itself, it binds the initial states for itself.
    pub(crate) fn none() -> Operation {
        Operation {
            streams: None,
            work: None,
            usage: None,
            layers: LayerOptions::default(),
            magnification: 1.0,
            optional: Arc::default(),
            cache: None,
            parallelism: Parallelism::Serial,
            quiet_glyphs: false,
        }
    }

    /// The optional-content states the operation's scopes read, bound from
    /// `doc` the first time any of them asks, and the same binding every time
    /// after.
    pub(crate) fn optional(&self, doc: &CosDocument) -> &OptionalContent {
        self.optional.get_or_init(|| {
            OptionalContent::bind_with(doc, self.usage, &self.layers, self.magnification)
        })
    }

    /// The magnification a form's kept drawing is keyed by beside the usage
    /// and the layer options, as its bits: the operation's where its states
    /// read a `/Zoom` entry, which a render at another magnification may
    /// answer otherwise, and `None` where they read none, so drawings kept at
    /// one magnification serve every other.
    pub(crate) fn magnification_key(&self, doc: &CosDocument) -> Option<u64> {
        self.optional(doc)
            .reads_magnification()
            .then_some(self.magnification.to_bits())
    }

    /// The layer options the operation binds the states of: what a form's
    /// kept drawing is keyed by beside the usage.
    pub(crate) fn layers(&self) -> &LayerOptions {
        &self.layers
    }

    /// The operation's binding, bound now if no scope has asked yet: what a
    /// display list keeps of its recording's states, and hands a render of
    /// it whose states those are.
    pub(crate) fn states(&self, doc: &CosDocument) -> Arc<OnceLock<OptionalContent>> {
        self.optional(doc);
        Arc::clone(&self.optional)
    }

    /// This operation, reading `states` — a binding another operation of the
    /// same render made — rather than binding its own, so that a render
    /// whose states were bound to compare them binds them once.
    pub(crate) fn sharing(self, states: Arc<OnceLock<OptionalContent>>) -> Operation {
        Operation {
            optional: states,
            ..self
        }
    }

    /// This operation, keeping what it computes in the document's cache
    /// where the document has one.
    pub(crate) fn with_cache(mut self, cache: Option<&Arc<DocumentCache>>) -> Operation {
        self.cache = cache.cloned();
        self
    }

    /// The tally, when the operation is counting.
    pub(crate) fn work(&self) -> Option<&Arc<Work>> {
        self.work.as_ref()
    }

    /// The document's cache, when the document has one.
    pub(crate) fn cache(&self) -> Option<&DocumentCache> {
        self.cache.as_deref()
    }

    /// What the operation renders for, which binds its optional-content
    /// states: the usage a form's kept drawing is keyed by.
    pub(crate) fn usage(&self) -> Option<Usage> {
        self.usage
    }

    /// Whether this is [`Operation::none`].
    fn is_none(&self) -> bool {
        self.streams.is_none() && self.work.is_none() && self.cache.is_none()
    }

    /// `r`'s bytes through its filter chain, once an operation: the
    /// operation's first read of `r` runs the chain, counted into the tally
    /// when there is one, and every later read is handed what it decoded,
    /// with nothing run, counted or warned again.
    pub(crate) fn decoded(&self, doc: &CosDocument, r: ObjRef) -> Result<Arc<Vec<u8>>, CosError> {
        let decode = || doc.stream_decoded_counted(r, self.work.as_deref());
        match &self.streams {
            Some(streams) => streams.read(r, decode),
            None => decode().map(Arc::new),
        }
    }

    /// The font `dict` describes, read as `cos_font::read_in` reads it,
    /// through `doc`: its CMaps and `/CIDToGIDMap` decoded once an
    /// operation, and counted, with every key the font spells out, when the
    /// operation counts.
    fn read_font(&self, doc: &CosDocument, dict: &Dict) -> cos_font::Font {
        // An operation of none still reads the font through the view, over
        // streams of the read's own, so that one reader serves both.
        let own;
        let streams = match &self.streams {
            Some(streams) => &**streams,
            None => {
                own = DecodedStreams::new();
                &own
            }
        };
        match self.work.as_deref() {
            Some(work) => cos_font::read_in(&Cached::new(&Counting::new(doc, work), streams), dict),
            None => cos_font::read_in(&Cached::new(doc, streams), dict),
        }
    }
}

/// How long each list of what a scope's resources tolerated is
/// ([`PageResources::tolerated`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tolerated {
    /// Images decoded with damage tolerated.
    pub(crate) damaged: usize,
    /// Colour spaces read with a repair.
    pub(crate) repaired: usize,
    /// Images drawn from a decode the budget reduced.
    pub(crate) reduced: usize,
}

/// Everything one page's rendering needs from its resources.
pub struct PageResources {
    doc: Arc<CosDocument>,
    /// The resource dictionary every name here is read in. Its fonts are read
    /// on the first question about each (`Caches::fonts`), not when the scope
    /// is built. Shared, so that the scope a cache keeps is handed to an
    /// asker over the same dictionary rather than a copy of it
    /// ([`PageResources::bind`]).
    resources: Option<Arc<Dict>>,
    /// Everything worked out once and reusable by every render: fonts,
    /// programs, images, ICC transforms, outlines, form scopes. Shared, so
    /// [`PageResources::for_one_render`] can hand a render its own lists of
    /// what it tolerated over the same caches.
    caches: Arc<Caches>,
    /// Resource names that named no font this build could resolve, and — when
    /// glyphs were being drawn — names whose font resolved no glyph for a
    /// code the page used.
    missing_fonts: Mutex<Vec<String>>,
    /// Images that decoded with something tolerated, as `(resource name,
    /// what the decoder tolerated)`. Ruling 10: the leaf crate says what it
    /// forgave, and this is where the object it happened in gets attached.
    damaged_images: Mutex<Vec<(String, String)>>,
    /// Images drawn from a decode the document's budget took below what
    /// their drawn size asks, as `(resource name, reduction)` — reported as
    /// [`tinker_pdf_render::RenderWarning::ReducedImage`] (ruling 10).
    reduced_images: Mutex<Vec<(String, u32)>>,
    /// Colour spaces whose parameters were read the nearest way that means
    /// something, as `(resource name, what was repaired)` — reported as
    /// [`tinker_pdf_render::RenderWarning::RepairedColorSpace`] (ruling 10).
    repaired_spaces: Mutex<Vec<(String, String)>>,
    /// While [`crate::Page::images`] decodes one image, what the decoders
    /// tolerated for **that** image, so the leniency lands on the
    /// [`crate::PageImage`] it happened to rather than only on the page's
    /// list above — which dedups by resource name, and which `images` has no
    /// bitmap to carry out on (ruling 10). `None` when nothing is capturing.
    image_capture: Mutex<Option<Vec<String>>>,
    /// The font the glyph lookup under way reported as resolving no glyph,
    /// taken by [`GlyphSource::outline`] into the cache entry beside the
    /// outline so a render that meets it there can say it again.
    unresolved: Mutex<Option<String>>,
    /// The forms being recorded in this scope for the document's cache,
    /// innermost last, each with what the scope has been told since its
    /// interpretation began (`FontSource::kept_form`).
    recording: Mutex<Vec<FormCapture>>,
    /// The host's substitute faces, kept so a resource dictionary *inside*
    /// this one can be read with the same configuration.
    ///
    /// A tiling pattern's `/Resources` (8.7.3.2) is read into a
    /// `PageResources` of its own, and without this the cell would be the one
    /// place in a document where a `FontProvider` the caller installed does
    /// not apply — text inside a pattern would silently lose its substitute
    /// face while the same text beside it kept one.
    provider: Option<Arc<dyn FontProvider>>,
    /// The operation these resources serve: the streams it has decoded,
    /// which every stream this scope reads is read through, its tally
    /// when it is counting (`docs/design/performance.md`) — the fonts this
    /// scope resolves, the image decoders it runs and every lookup in its
    /// caches count into it — and the optional-content states every `BDC`
    /// and every XObject's `/OC` this scope is asked about reads, bound once
    /// for the operation rather than walked per `BDC`
    /// ([`Operation::optional`]).
    ///
    /// Its tally is `None` for every operation nobody asked about, and then
    /// every counting site below tests it and does nothing else. A scope
    /// this one makes inherits it; a scope a cache keeps holds none, because
    /// the cache outlives the operation, and is handed to each asker bound to
    /// the asker's ([`PageResources::bind`]).
    op: Operation,
}

/// Applies `/Decode [1 0]` to already-decoded samples.
///
/// The general `/Decode` machinery works on raw samples before colour
/// conversion, which the JPEG and CCITT paths have already done by the time
/// they return. Reversal is the only form either of them meets in practice —
/// `/Decode [1 0]` on a fax, `[1 0 1 0 1 0]` on a JPEG — and inverting the
/// output is exactly equivalent for those. Any other range is left alone
/// rather than approximated.
fn invert_if_decode_reverses(decode: &[(f64, f64)], rgb: &mut [u8]) {
    if decode.is_empty() || !decode.iter().all(|(a, b)| *a > *b) {
        return;
    }
    for value in rgb.iter_mut() {
        *value = 255 - *value;
    }
}

/// What an inline image is called in a warning. It has no resource name to be
/// called anything else by, which is also why it is not cached.
pub(crate) const INLINE_NAME: &str = "inline";

/// What an inline image's filter chain produced: samples, or the still-coded
/// bytes of a codec that returns its own pixels.
pub(crate) enum InlineSamples {
    /// Samples for the sample loop; `fax` when they are a fax's packed
    /// one-bit samples, whatever `/BitsPerComponent` said.
    Samples {
        /// The samples.
        bytes: Vec<u8>,
        /// Whether a CCITT decode produced them.
        fax: bool,
    },
    /// A JPEG, for [`jpeg_samples`].
    Jpeg(Vec<u8>),
    /// A JPEG 2000 codestream, for [`PageResources::jpx_samples`].
    Jpx(Vec<u8>),
}

/// A JPEG's samples, decoded and **not** converted: one byte a component, in
/// the components the frame carries — YCbCr already turned into RGB and an
/// Adobe-inverted CMYK already turned back into ink values, which are the
/// decoder's to undo, and nothing else (7.4.8).
///
/// The decode [`crate::Page::images`] makes, under the ceiling
/// [`PageResources::jpeg_image`]'s strips are decoded under.
fn jpeg_samples(raw: &[u8]) -> Result<tinker_pdf_filters::JpegImage, String> {
    jpeg_decode(raw, JPEG_CEILING).map_err(|e| format!("{e:?}"))
}

/// The output ceiling every JPEG decode is handed.
const JPEG_CEILING: usize = 1 << 28;

/// How many components a decoded JPEG's samples have a pixel.
fn jpeg_components(color: JpegColor) -> u8 {
    match color {
        JpegColor::Gray => 1,
        JpegColor::Rgb => 3,
        JpegColor::Cmyk | JpegColor::CmykInverted => 4,
    }
}

/// A codec's samples drawn in the space its image's dictionary names (8.9.5).
///
/// `pixels` pixels of `space.components()` values of `bpc` bits, eight or
/// sixteen, interleaved: the layout a JPEG's samples and a JPEG 2000
/// codestream's already have, and a stream's once its rows are whole bytes
/// (8.9.3). Each value is read through `decode`, or Table 90's default, and
/// each pixel through [`ColorSpace::to_rgb_in`] as `conversion` says, exactly
/// as the sample loop reads a stream's — so a codec's samples and the same
/// samples written as a stream in the same space draw the same bytes — by the
/// converter the image's space, depth and `/Decode` choose ([`Converter`]),
/// what it asks of the conversion counted into `work`.
fn samples_in_space(
    space: &ColorSpace,
    samples: &[u8],
    pixels: usize,
    bpc: u32,
    decode: &[(f64, f64)],
    conversion: icc::Conversion,
    work: Option<&Work>,
) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(pixels.saturating_mul(3));
    let mut converter = SpaceSamples::new(space, bpc, decode, conversion);
    converter.extend(space, samples, pixels, &mut rgb);
    converter.count_into(work);
    rgb
}

/// How the sample loop reads one image's samples (8.9.3, 8.9.5): everything
/// it needs besides the bytes, the same for an image XObject and an inline
/// image.
#[derive(Clone, Copy)]
struct SampleLayout<'a> {
    /// Samples across.
    width: u32,
    /// Samples down.
    height: u32,
    /// Bits a component, 1 to 16.
    bpc: u32,
    /// Components a sample.
    n: usize,
    /// The space the components are in.
    space: &'a ColorSpace,
    /// `/Decode`, a pair a component; a component past the array takes Table
    /// 90's default.
    decode: &'a [(f64, f64)],
    /// An `/ImageMask`: a sample is paint or not, and has no colour (8.9.6.2).
    mask: bool,
    /// 8.9.6.4's colour key: one inclusive range of raw values a component.
    color_key: Option<&'a [(u32, u32)]>,
    /// How a sample's colour converts: in the rendering intent the image's
    /// colours take (8.6.5.8, Table 89), which only an ICC space reads.
    conversion: icc::Conversion,
    /// The tally the colours the loop converts are counted into
    /// ([`crate::Stats::image_colours_converted`]), when the operation counts.
    work: Option<&'a Work>,
}

/// What an image's samples put on the plates of a separations render by
/// `plan` (RC-04.12): each sample's tints, read from the raw values the light
/// loop reads ([`read_bits`], a byte at a time where a component is one) and
/// worked out by [`ImageTints`] beside the sample's light in `rgb`, three
/// bytes a sample; nothing at all for a space naming only `/None`; `None`
/// where the space names no colorant and the samples are their light.
fn plate_samples(
    data: &[u8],
    layout: &SampleLayout<'_>,
    rgb: &[u8],
    plan: &PlatePlan,
) -> Option<ImagePlates> {
    let SampleLayout {
        width,
        height,
        bpc,
        n,
        space,
        decode,
        ..
    } = *layout;
    let mut tints = match ImagePlating::of(space, plan)? {
        ImagePlating::Nothing => return Some(ImagePlates::Nothing),
        ImagePlating::Tints(tints) => tints,
    };
    let (width, height) = (width as usize, height as usize);
    let row_bytes = (width * n * bpc as usize).div_ceil(8);
    let channels = tints.channels();
    let mut out = Vec::with_capacity(width.saturating_mul(height).saturating_mul(channels));
    let mut values = Vec::with_capacity(n);
    for y in 0..height {
        for x in 0..width {
            values.clear();
            for c in 0..n {
                let value = if bpc == 8 {
                    let at = y * row_bytes + x * n + c;
                    u32::from(data.get(at).copied().unwrap_or(0))
                } else {
                    read_bits(data, y * row_bytes * 8 + (x * n + c) * bpc as usize, bpc)
                };
                values.push(value);
            }
            let at = (y * width + x) * 3;
            let light = match rgb.get(at..at + 3) {
                Some(&[r, g, b]) => (r, g, b),
                _ => (255, 255, 255),
            };
            tints.push(space, decode, &values, bpc, light, &mut out);
        }
    }
    Some(ImagePlates::Tints {
        tints: out,
        channels,
        marks: tints.marks(),
    })
}

/// An image's samples as RGB, three bytes a sample, with an alpha byte a
/// sample where a mask or a colour key gives one: the sample loop both image
/// paths share.
///
/// An 8-bit device image is its own colour and is copied rather than read
/// ([`identity_pixels`]); a one-bit image of one component is expanded a byte
/// at a time from its two outcomes ([`one_bit_pixels`]); everything else is
/// read sample by sample ([`general_pixels`]).
fn image_pixels(data: &[u8], layout: &SampleLayout<'_>) -> (Vec<u8>, Vec<u8>) {
    if let Some(rgb) = identity_pixels(data, layout) {
        return (rgb, Vec::new());
    }
    if let Some(pixels) = one_bit_pixels(data, layout) {
        return pixels;
    }
    general_pixels(data, layout)
}

/// An 8-bit DeviceGray or DeviceRGB image whose samples are its colour,
/// copied — or `None` where the loop has to read them.
///
/// Every `/Decode` pair `[0 1]` or absent, no colour key and no
/// `/ImageMask`. The loop would read a sample `v` as `v / 255.0`, a `[0 1]`
/// pair would map it to `0 + raw × 1`, which is `raw` exactly, and
/// `ColorSpace::to_rgb` would answer `round(raw × 255)`, which is within
/// 1e-13 of `v` before it is rounded and so is `v`: the sample is the byte. A
/// grey is replicated to three. Rows of 8-bit samples are whole bytes with
/// nothing between them, so the image is one run of bytes, and a sample the
/// stream stops short of is zero, as `read_bits` reads it.
fn identity_pixels(data: &[u8], layout: &SampleLayout<'_>) -> Option<Vec<u8>> {
    let grey = match layout.space {
        ColorSpace::DeviceGray => true,
        ColorSpace::DeviceRgb => false,
        _ => return None,
    };
    if layout.mask
        || layout.color_key.is_some()
        || layout.bpc != 8
        || layout.n != layout.space.components()
        || layout
            .decode
            .iter()
            .take(layout.n)
            .any(|&(low, high)| low != 0.0 || high != 1.0)
    {
        return None;
    }
    let pixels = (layout.width as usize).checked_mul(layout.height as usize)?;
    let length = pixels.checked_mul(3)?;
    let samples = data.get(..pixels.checked_mul(layout.n)?).unwrap_or(data);
    let mut rgb = Vec::with_capacity(length);
    if grey {
        for &value in samples {
            rgb.extend_from_slice(&[value, value, value]);
        }
    } else {
        rgb.extend_from_slice(samples);
    }
    rgb.resize(length, 0);
    Some(rgb)
}

/// What the sample loop makes of the two samples of a one-component, one-bit
/// image — each one's colour, and its alpha, 255 where the image carries none
/// — or `None` for any other image.
///
/// Such an image has two samples, and everything the loop makes of one is a
/// function of its bit: the component through `/Decode` by [`component`], the
/// colour through `ColorSpace::to_rgb`, a mask's paint as that component
/// below one half (8.9.6.2), and a colour key's test, which is on the raw bit
/// (8.9.6.4). So the two outcomes are worked out once, by those same
/// expressions, for [`one_bit_pixels`] and [`two_colour_pixels`] alike: the
/// two colours an image's table of `2^bpc` holds (`docs/design/icc.md`,
/// P-22, "The per-image converter"), counted into the layout's tally.
fn one_bit_outcomes(layout: &SampleLayout<'_>) -> Option<[([u8; 3], u8); 2]> {
    let SampleLayout {
        bpc,
        n,
        space,
        decode,
        mask,
        color_key,
        conversion,
        work,
        ..
    } = *layout;
    if bpc != 1 || n != 1 {
        return None;
    }
    if !mask {
        stats::colours_converted(work, 2);
    }
    let outcome = |bit: u32| -> ([u8; 3], u8) {
        let value = component(space, decode, 0, bit, 1.0);
        if mask {
            return ([0, 0, 0], if value < 0.5 { 255 } else { 0 });
        }
        let (r, g, b) = converted(space, &[value], conversion);
        let keyed = color_key.is_some_and(|ranges| {
            // An absent range can never match, as in the general loop.
            let (lo, hi) = ranges.first().copied().unwrap_or((1, 0));
            bit >= lo && bit <= hi
        });
        ([r, g, b], if keyed { 0 } else { 255 })
    };
    Some([outcome(0), outcome(1)])
}

/// A one-component, one-bit image — a fax, a JBIG2 region, a one-bit Flate or
/// uncompressed stream, an `/ImageMask` — expanded a byte at a time through a
/// table, or `None` where the loop has to read the samples.
///
/// The two outcomes are [`one_bit_outcomes`]'s, and a 256-entry table holds
/// each byte's eight samples. A row begins on a byte (8.9.3), so a row reads
/// `ceil(w / 8)` bytes and keeps its first `w` samples, and a byte past the
/// stream's end reads as zero, as [`read_bits`] reads it.
fn one_bit_pixels(data: &[u8], layout: &SampleLayout<'_>) -> Option<(Vec<u8>, Vec<u8>)> {
    let outcomes = one_bit_outcomes(layout)?;
    let SampleLayout {
        width,
        height,
        mask,
        color_key,
        ..
    } = *layout;
    let with_alpha = mask || color_key.is_some();

    // Each byte's eight samples, the most significant bit first.
    let mut colours = vec![[0u8; 24]; 256];
    let mut alphas = vec![[0u8; 8]; 256];
    for (byte, (colour, alpha)) in colours.iter_mut().zip(alphas.iter_mut()).enumerate() {
        for (sample, (pixel, opacity)) in
            colour.chunks_exact_mut(3).zip(alpha.iter_mut()).enumerate()
        {
            let (rgb, a) = outcomes[(byte >> (7 - sample)) & 1];
            pixel.copy_from_slice(&rgb);
            *opacity = a;
        }
    }

    let (width, height) = (width as usize, height as usize);
    let row_bytes = width.div_ceil(8);
    let mut rgb = Vec::with_capacity(width.saturating_mul(height).saturating_mul(3));
    let mut alpha = Vec::with_capacity(if with_alpha {
        width.saturating_mul(height)
    } else {
        0
    });
    for row in 0..height {
        let mut left = width;
        for at in 0..row_bytes {
            let byte = row
                .checked_mul(row_bytes)
                .and_then(|start| start.checked_add(at))
                .and_then(|index| data.get(index))
                .copied()
                .unwrap_or(0);
            let take = left.min(8);
            rgb.extend_from_slice(&colours[usize::from(byte)][..take * 3]);
            if with_alpha {
                alpha.extend_from_slice(&alphas[usize::from(byte)][..take]);
            }
            left -= take;
        }
    }
    Some((rgb, alpha))
}

/// Each byte's eight one-bit samples, the most significant bit first, as
/// bytes of 0 and 1.
static BIT_SAMPLES: [[u8; 8]; 256] = {
    let mut table = [[0u8; 8]; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut sample = 0;
        while sample < 8 {
            table[byte][sample] = ((byte >> (7 - sample)) & 1) as u8;
            sample += 1;
        }
        byte += 1;
    }
    table
};

/// A one-component, one-bit image held as its two colours and one byte a
/// sample, or `None` where it is held as RGB.
///
/// The colours are [`one_bit_outcomes`]'s — the two [`one_bit_pixels`]
/// expands, each with its alpha — and a byte of a row becomes its eight
/// samples' bits through [`BIT_SAMPLES`]: a row reads `ceil(w / 8)` bytes and
/// keeps its first `w` samples, and a byte past the stream's end reads as
/// zero, as [`read_bits`] reads it. So the image expanded to RGB is the image
/// [`one_bit_pixels`] gives, sample for sample, and the rasterizer draws the
/// one exactly as the other (`tinker_pdf_raster::TwoColour`) at a third or a
/// quarter of the bytes. A colour key that selects one raw value is that
/// colour's alpha of zero; one that selects both keeps the RGB form, the
/// form's rule in `docs/design/image-edges.md` (P-08).
fn two_colour_pixels(data: &[u8], layout: &SampleLayout<'_>) -> Option<TwoColourSamples> {
    // A mask reads no colour key, so only an image's key can select both.
    // Asked before the outcomes are worked out, so an image held as RGB
    // converts its two colours once, in the expansion that holds it.
    let keys_both = !layout.mask
        && layout.color_key.is_some_and(|ranges| {
            let (lo, hi) = ranges.first().copied().unwrap_or((1, 0));
            lo == 0 && hi >= 1
        });
    if keys_both {
        return None;
    }
    let [(zero, zero_alpha), (one, one_alpha)] = one_bit_outcomes(layout)?;

    let (width, height) = (layout.width as usize, layout.height as usize);
    let row_bytes = width.div_ceil(8);
    let mut samples = Vec::with_capacity(width.saturating_mul(height));
    for row in 0..height {
        let mut left = width;
        for at in 0..row_bytes {
            let byte = row
                .checked_mul(row_bytes)
                .and_then(|start| start.checked_add(at))
                .and_then(|index| data.get(index))
                .copied()
                .unwrap_or(0);
            let take = left.min(8);
            samples.extend_from_slice(&BIT_SAMPLES[usize::from(byte)][..take]);
            left -= take;
        }
    }
    Some(TwoColourSamples {
        samples,
        colours: [
            [zero[0], zero[1], zero[2], zero_alpha],
            [one[0], one[1], one[2], one_alpha],
        ],
    })
}

/// How a decode holds a one-component, one-bit image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleForm {
    /// As its two colours and one byte a sample, where the image allows it
    /// ([`two_colour_pixels`]).
    TwoColour,
    /// As RGB whatever the image: an image a soft mask applies to, and the
    /// soft mask itself, which [`apply_soft_mask`] reads and writes as RGB.
    Rgb,
}

/// An image's samples as a decode holds them, `(rgb, alpha, two_colour)`: as
/// two colours and one byte a sample where `form` and the image allow it,
/// and otherwise as [`image_pixels`] gives them.
fn decoded_pixels(
    data: &[u8],
    layout: &SampleLayout<'_>,
    form: SampleForm,
) -> (Vec<u8>, Vec<u8>, Option<TwoColourSamples>) {
    if form == SampleForm::TwoColour {
        if let Some(two) = two_colour_pixels(data, layout) {
            return (Vec::new(), Vec::new(), Some(two));
        }
    }
    let (rgb, alpha) = image_pixels(data, layout);
    (rgb, alpha, None)
}

/// The sample loop, a sample at a time: each component read through
/// [`read_bits`] and tested against the colour key on its raw value, and the
/// sample's colour asked of the image's converter ([`Converter`]), chosen
/// once for the image by its space, depth and `/Decode` — or, for a mask,
/// painted where its component is below one half. An image of 8 or 16 bits
/// that no mask or colour key reads is converted as one run of samples
/// ([`Converter::extend`]), its rows being whole bytes with nothing between
/// them (8.9.3). What the converter asks of the space's conversion is
/// counted into the layout's tally.
fn general_pixels(data: &[u8], layout: &SampleLayout<'_>) -> (Vec<u8>, Vec<u8>) {
    let SampleLayout {
        width,
        height,
        bpc,
        n,
        space,
        decode,
        mask,
        color_key,
        conversion,
        work,
    } = *layout;
    let pixels = (width as usize).saturating_mul(height as usize);
    let mut rgb = Vec::with_capacity(pixels.saturating_mul(3));
    let mut alpha = Vec::new();
    let row_bits = (width as usize) * n * (bpc as usize);
    let row_bytes = row_bits.div_ceil(8);
    let max = ((1u32 << bpc.min(16)) - 1) as f64;
    let mut converter = Converter::new(space, n, bpc, decode, conversion, color_key.is_some());
    // Samples of whole bytes run on from row to row with nothing between
    // them (8.9.3), so an image no mask or colour key reads a sample at a
    // time converts as one run.
    if !mask && color_key.is_none() && bpc % 8 == 0 {
        converter.extend(space, decode, data, pixels, &mut rgb);
        converter.count_into(work);
        return (rgb, alpha);
    }
    let mut values = Vec::with_capacity(n);

    for y in 0..height as usize {
        for x in 0..width as usize {
            values.clear();
            // "Shall be masked ... if min_i <= sample_i <= max_i for *all*
            // i" — one component outside its range paints the pixel, which
            // is why this starts true and is narrowed rather than widened.
            let mut keyed = color_key.is_some();
            for c in 0..n {
                let bit = y * row_bytes * 8 + (x * n + c) * bpc as usize;
                let value = read_bits(data, bit, bpc);
                if let Some(ranges) = color_key {
                    // An absent range can never match, so a short array
                    // masks nothing rather than everything.
                    let (lo, hi) = ranges.get(c).copied().unwrap_or((1, 0));
                    keyed &= value >= lo && value <= hi;
                }
                values.push(value);
            }

            if mask {
                // A zero sample paints, a one does not (8.9.6.2).
                let first = values.first().copied().unwrap_or(0);
                let paints = component(space, decode, 0, first, max) < 0.5;
                rgb.extend_from_slice(&[0, 0, 0]);
                alpha.push(if paints { 255 } else { 0 });
            } else {
                let (r, g, b) = converter.colour(space, decode, &values);
                rgb.extend_from_slice(&[r, g, b]);
                if color_key.is_some() {
                    alpha.push(if keyed { 0 } else { 255 });
                }
            }
        }
    }
    converter.count_into(work);
    (rgb, alpha)
}

/// Component `c` of a sample whose raw value is `value`, as the sample loop
/// reads it: scaled into 0..1 by `max`, the depth's largest value — an indexed
/// space's component is the index itself — then through `/Decode`, or Table
/// 90's default where the array stops short of it.
fn component(space: &ColorSpace, decode: &[(f64, f64)], c: usize, value: u32, max: f64) -> f64 {
    let indexed = matches!(space, ColorSpace::Indexed { .. });
    let raw = if indexed {
        f64::from(value)
    } else {
        f64::from(value) / max
    };
    match decode.get(c).copied().or_else(|| default_decode(space, c)) {
        // The interpolation is over the *sample* range, so an indexed image
        // maps its index rather than a fraction.
        Some((dmin, dmax)) if indexed => dmin + raw * (dmax - dmin) / max.max(1.0),
        Some((dmin, dmax)) => dmin + raw * (dmax - dmin),
        None => raw,
    }
}

#[cfg(test)]
thread_local! {
    /// Colours the sample loops have converted through `ColorSpace::to_rgb`
    /// on this thread: what `an_eight_bit_device_image_converts_by_identity`
    /// and the converter's tests count.
    static SAMPLE_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `space.to_rgb_in(components, conversion)` for the sample loops, counted
/// under `cfg(test)`.
fn converted(space: &ColorSpace, components: &[f64], conversion: icc::Conversion) -> (u8, u8, u8) {
    #[cfg(test)]
    SAMPLE_CONVERSIONS.with(|count| count.set(count.get() + 1));
    space.to_rgb_in(components, conversion)
}

/// [`converted`] through `scratch`, which the caller keeps: a `/Separation`
/// or `/DeviceN` space evaluates its tint transform there rather than into a
/// buffer of its own (`ColorSpace::to_rgb_with_in`), the same colour.
fn converted_with(
    space: &ColorSpace,
    components: &[f64],
    scratch: &mut Vec<f64>,
    conversion: icc::Conversion,
) -> (u8, u8, u8) {
    #[cfg(test)]
    SAMPLE_CONVERSIONS.with(|count| count.set(count.get() + 1));
    space.to_rgb_with_in(components, scratch, conversion)
}

/// An entry of a converter's table that holds no colour yet: no colour packs
/// to it, since a colour fills only the low 24 bits.
const UNFILLED: u32 = u32::MAX;

/// An entry of the DeviceCMYK table that holds no byte yet.
const UNFILLED_BYTE: u16 = u16::MAX;

/// The DeviceCMYK table's entries: one for each byte of an ink against each
/// byte of black.
const CMYK_PAIRS: usize = 1 << 16;

/// How one image's samples become RGB (`docs/design/icc.md`, P-22, "The
/// per-image converter"): chosen once an image, by its space, its depth, its
/// `/Decode` and whether a colour key reads its samples, where the sample
/// loops once converted every pixel through [`ColorSpace::to_rgb_in`].
///
/// Every arm gives a pixel the colour `to_rgb_in` gives the components the
/// loop reads from its samples ([`component`]), so the converter moves no
/// pixel; only what the conversion costs does. A table's entry is filled
/// through `to_rgb_in` the first time a sample asks for it, so an image of
/// few pixels asks no more of the conversion than its pixels did, and none
/// asks more than its table holds. What it asks is counted
/// ([`Converter::count_into`]).
struct Converter {
    /// Values a pixel.
    n: usize,
    /// Bits a value.
    bpc: u32,
    /// The largest value a sample holds.
    max: f64,
    /// How a pixel's colour converts: in the intent the image's colours take.
    conversion: icc::Conversion,
    arm: Arm,
    /// What the converter has asked of the space's conversion.
    converted: u64,
    /// One pixel's raw values, a buffer [`Converter::extend`] keeps.
    values: Vec<u32>,
}

/// A [`Converter`]'s way with the image's samples.
enum Arm {
    /// The samples are the colour: 8-bit DeviceGray, replicated to three, or
    /// DeviceRGB — or a refused profile approximated as one of them — with
    /// every `/Decode` pair `[0 1]` or absent and no colour key, so the loop
    /// would read a sample `v` as `v / 255` and `to_rgb` round it back to `v`
    /// (P-08's identity path).
    Copy {
        /// One component, replicated.
        grey: bool,
    },
    /// An 8-bit ICC space with `/Decode` absent or `[0 1]` and no colour key,
    /// whose transform takes the space's components: the transform's row path
    /// (`icc::Transform::convert_row`), which converts bytes as
    /// `icc::Transform::apply` converts them, through tables a transform builds
    /// once, and asks no conversion of the converter at all. A pixel asked
    /// alone is read through two buffers the image keeps: its bytes and its
    /// colour.
    Row { pixel: Vec<u8>, out: Vec<u8> },
    /// One component of at most 8 bits, whatever its space and `/Decode`: the
    /// colour of each of its `2^bpc` values, packed `0x00RRGGBB`, filled the
    /// first time a sample holds the value.
    Values(Vec<u32>),
    /// 8-bit DeviceCMYK, or a profile approximated as it, with `/Decode`
    /// absent or `[0 1]`: 8.6.4.4's `(1 − x)(1 − k)` as a byte for each ink
    /// byte `x` against each black byte `k`, at `x · 256 + k`, read for each of
    /// C, M and Y against K. `to_rgb` gives each channel by that one relation
    /// of its ink and black alone, so a pixel's conversion fills its three
    /// entries at once, and an entry is that relation's byte whichever ink
    /// filled it.
    Cmyk(Vec<u16>),
    /// Everything else: the components read through `/Decode` into one
    /// buffer the image keeps, converted through a second it keeps for a tint
    /// transform's outputs, with what was converted remembered where a
    /// conversion is dear ([`Remembered`]).
    General {
        remembered: Option<Remembered>,
        components: Vec<f64>,
        scratch: Vec<f64>,
    },
}

impl Converter {
    /// The converter for an image of `n` values of `bpc` bits a pixel in
    /// `space` under `decode`, its colours converted as `conversion` says,
    /// `keyed` where a colour key reads its samples (8.9.6.4).
    fn new(
        space: &ColorSpace,
        n: usize,
        bpc: u32,
        decode: &[(f64, f64)],
        conversion: icc::Conversion,
        keyed: bool,
    ) -> Converter {
        Converter {
            n,
            bpc,
            max: ((1u32 << bpc.min(16)) - 1) as f64,
            conversion,
            arm: Arm::choose(space, n, bpc, decode, keyed),
            converted: 0,
            values: Vec::with_capacity(n),
        }
    }

    /// Adds what the converter has asked of the space's conversion to `work`,
    /// once: the count is taken as it is added.
    fn count_into(&mut self, work: Option<&Work>) {
        stats::colours_converted(work, std::mem::take(&mut self.converted));
    }

    /// The colour of one pixel whose raw values are `values`, `n` of them,
    /// in `space`, the space the converter was chosen for, under `decode`.
    fn colour(
        &mut self,
        space: &ColorSpace,
        decode: &[(f64, f64)],
        values: &[u32],
    ) -> (u8, u8, u8) {
        let value = |c: usize| values.get(c).copied().unwrap_or(0);
        let (max, conversion) = (self.max, self.conversion);
        match &mut self.arm {
            Arm::Copy { grey: true } => {
                let v = byte_of(value(0));
                (v, v, v)
            }
            Arm::Copy { grey: false } => (byte_of(value(0)), byte_of(value(1)), byte_of(value(2))),
            Arm::Row { pixel, out } => {
                // The row path over the one pixel: its bytes, its colour.
                pixel.clear();
                pixel.extend((0..self.n).map(|c| byte_of(value(c))));
                out.clear();
                if let ColorSpace::Icc { transforms, .. } = space {
                    transforms.get(conversion.intent).convert_row(pixel, out);
                }
                match out[..] {
                    [r, g, b, ..] => (r, g, b),
                    _ => (0, 0, 0),
                }
            }
            Arm::Values(colours) => {
                let at = usize::try_from(value(0)).unwrap_or(usize::MAX);
                match colours.get(at).copied() {
                    Some(packed) if packed != UNFILLED => unpack(packed),
                    _ => {
                        let colour = converted(
                            space,
                            &[component(space, decode, 0, value(0), max)],
                            conversion,
                        );
                        self.converted += 1;
                        if let Some(slot) = colours.get_mut(at) {
                            *slot = pack(colour);
                        }
                        colour
                    }
                }
            }
            Arm::Cmyk(light) => {
                let k = value(3) & 0xFF;
                let at = |x: u32| (((x & 0xFF) << 8) | k) as usize;
                let held = [0, 1, 2].map(|c| light.get(at(value(c))).copied());
                if let [Some(r), Some(g), Some(b)] = held {
                    if r != UNFILLED_BYTE && g != UNFILLED_BYTE && b != UNFILLED_BYTE {
                        return (r as u8, g as u8, b as u8);
                    }
                }
                let components = [0, 1, 2, 3].map(|c| f64::from(value(c) & 0xFF) / 255.0);
                let (r, g, b) = converted(space, &components, conversion);
                self.converted += 1;
                for (c, channel) in [r, g, b].into_iter().enumerate() {
                    if let Some(slot) = light.get_mut(at(value(c))) {
                        *slot = u16::from(channel);
                    }
                }
                (r, g, b)
            }
            Arm::General {
                remembered,
                components,
                scratch,
            } => {
                components.clear();
                let mut key = 0u128;
                for c in 0..self.n {
                    key = Remembered::key(key, c, value(c));
                    components.push(component(space, decode, c, value(c), max));
                }
                match remembered.as_mut() {
                    Some(table) => {
                        let mut fresh = false;
                        let colour = table.convert(key, || {
                            fresh = true;
                            converted_with(space, components, scratch, conversion)
                        });
                        self.converted += u64::from(fresh);
                        colour
                    }
                    None => {
                        self.converted += 1;
                        converted_with(space, components, scratch, conversion)
                    }
                }
            }
        }
    }

    /// Converts the first `pixels` pixels of `samples` — `n` values of `bpc`
    /// bits a pixel, interleaved with nothing between them, a value the run
    /// stops short of zero, as [`read_bits`] reads it — in `space` under
    /// `decode`, onto the end of `rgb`.
    fn extend(
        &mut self,
        space: &ColorSpace,
        decode: &[(f64, f64)],
        samples: &[u8],
        pixels: usize,
        rgb: &mut Vec<u8>,
    ) {
        let n = self.n.max(1);
        rgb.reserve(pixels.saturating_mul(3));
        let wanted = pixels.saturating_mul(n);
        let intent = self.conversion.intent;
        match self.arm {
            Arm::Copy { grey } if self.bpc == 8 => {
                let held = samples.get(..wanted).unwrap_or(samples);
                if grey {
                    for &v in held {
                        rgb.extend_from_slice(&[v, v, v]);
                    }
                    rgb.resize(rgb.len() + (wanted - held.len()) * 3, 0);
                } else {
                    let start = rgb.len();
                    rgb.extend_from_slice(held);
                    rgb.resize(start + wanted, 0);
                }
                return;
            }
            Arm::Row { ref mut pixel, .. } if self.bpc == 8 => {
                if let ColorSpace::Icc { transforms, .. } = space {
                    let transform = transforms.get(intent);
                    let held = samples.get(..wanted).unwrap_or(samples);
                    let whole = held.len() / n * n;
                    transform.convert_row(&held[..whole], rgb);
                    let mut left = pixels - whole / n;
                    // The pixel the run stops inside, its missing values
                    // zero, and every pixel after it, which is all zeros and
                    // so one colour, converted once.
                    let partial = &held[whole..];
                    if left > 0 && !partial.is_empty() {
                        pixel.clear();
                        pixel.extend_from_slice(partial);
                        pixel.resize(n, 0);
                        transform.convert_row(pixel, rgb);
                        left -= 1;
                    }
                    if left > 0 {
                        pixel.clear();
                        pixel.resize(n, 0);
                        let start = rgb.len();
                        transform.convert_row(pixel, rgb);
                        if let Some(&[r, g, b]) = rgb.get(start..start + 3) {
                            for _ in 1..left {
                                rgb.extend_from_slice(&[r, g, b]);
                            }
                        }
                    }
                    return;
                }
            }
            _ => {}
        }
        // The pixel's values, in a buffer the converter keeps across its
        // runs, so a strip's conversion allocates nothing.
        let mut values = std::mem::take(&mut self.values);
        for pixel in 0..pixels {
            values.clear();
            for c in 0..n {
                let at = pixel * n + c;
                values.push(match self.bpc {
                    8 => u32::from(samples.get(at).copied().unwrap_or(0)),
                    bpc => read_bits(samples, at * bpc as usize, bpc),
                });
            }
            let (r, g, b) = self.colour(space, decode, &values);
            rgb.extend_from_slice(&[r, g, b]);
        }
        self.values = values;
    }
}

impl Arm {
    /// The arm for an image of `n` values of `bpc` bits a pixel in `space`
    /// under `decode`, `keyed` where a colour key reads its samples.
    fn choose(space: &ColorSpace, n: usize, bpc: u32, decode: &[(f64, f64)], keyed: bool) -> Arm {
        // Every `/Decode` pair the components read `[0 1]` or absent, so a
        // sample `v` of eight bits reads as `v / 255` exactly.
        let plain = decode
            .iter()
            .take(n)
            .all(|&(low, high)| low == 0.0 && high == 1.0);
        let eight = bpc == 8;
        // The device space a space converts as: a refused profile is
        // approximated by its component count as that device space
        // (`ColorSpace::to_rgb_in`).
        let device = match space {
            ColorSpace::DeviceGray | ColorSpace::Approximated { components: 1 } => Some(1),
            ColorSpace::DeviceRgb | ColorSpace::Approximated { components: 3 } => Some(3),
            ColorSpace::DeviceCmyk | ColorSpace::Approximated { components: 4 } => Some(4),
            _ => None,
        };
        let ours = device == Some(n);
        if ours && eight && plain && !keyed && n != 4 {
            return Arm::Copy { grey: n == 1 };
        }
        if let ColorSpace::Icc { transforms, .. } = space {
            if eight && plain && !keyed && transforms.inputs() == n {
                return Arm::Row {
                    pixel: Vec::with_capacity(n),
                    out: Vec::with_capacity(3),
                };
            }
        }
        if n == 1 && bpc <= 8 {
            return Arm::Values(vec![UNFILLED; 1usize << bpc.max(1)]);
        }
        if ours && eight && plain && n == 4 {
            return Arm::Cmyk(vec![UNFILLED_BYTE; CMYK_PAIRS]);
        }
        Arm::General {
            remembered: Remembered::for_image(space, n, bpc),
            components: Vec::with_capacity(n),
            scratch: Vec::new(),
        }
    }
}

/// A value of at most eight bits as the byte it is: the copy's and the row
/// path's sample.
fn byte_of(value: u32) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// A colour packed into a table's entry, `0x00RRGGBB`.
fn pack((r, g, b): (u8, u8, u8)) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// The colour a table's entry holds.
fn unpack(packed: u32) -> (u8, u8, u8) {
    let [_, r, g, b] = packed.to_be_bytes();
    (r, g, b)
}

/// [`samples_in_space`] a run of pixels at a time, for a codec that hands its
/// samples out in strips: the converter lasts across the runs, which come in
/// the image's order, so every pixel's colour is the one a single call over
/// the whole image gives.
struct SpaceSamples<'d> {
    decode: &'d [(f64, f64)],
    converter: Converter,
}

impl<'d> SpaceSamples<'d> {
    fn new(
        space: &ColorSpace,
        bpc: u32,
        decode: &'d [(f64, f64)],
        conversion: icc::Conversion,
    ) -> SpaceSamples<'d> {
        SpaceSamples {
            decode,
            converter: Converter::new(space, space.components(), bpc, decode, conversion, false),
        }
    }

    /// Converts the first `pixels` pixels of `samples` in `space`, the space
    /// [`SpaceSamples::new`] was made for, onto the end of `rgb`.
    fn extend(&mut self, space: &ColorSpace, samples: &[u8], pixels: usize, rgb: &mut Vec<u8>) {
        self.converter
            .extend(space, self.decode, samples, pixels, rgb);
    }

    /// [`Converter::count_into`].
    fn count_into(&mut self, work: Option<&Work>) {
        self.converter.count_into(work);
    }
}

/// Where a JPEG's strips are written: straight into the image's RGB, so no
/// whole decoded image and no second copy of its colour exist beside it
/// (`docs/design/jpeg-speed.md`, "Decoding into the image").
struct JpegInto<'r, 'd> {
    resources: &'r PageResources,
    /// The space the dictionary's `/ColorSpace` was read as, until the layout
    /// says how many components the samples have.
    named: Option<ColorSpace>,
    name: &'d [u8],
    decode: &'d [(f64, f64)],
    /// How the samples' colours convert where they are drawn in the
    /// dictionary's space: in the intent the image's colours take.
    conversion: icc::Conversion,
    /// The space the samples are drawn in, when it is not the codec's own
    /// colour, and its converter.
    space: Option<(ColorSpace, SpaceSamples<'d>)>,
    /// A CMYK JPEG's converter where it is drawn in its own colour, which is
    /// DeviceCMYK's (8.6.4.4) with no `/Decode`, that applying to the
    /// converted strip instead.
    own: Option<Converter>,
    color: JpegColor,
    width: usize,
    rgb: Vec<u8>,
    /// The plan of a separations render the image is decoded for (RC-04.12).
    plates: Option<&'d PlatePlan>,
    /// Each sample's tints on the plan's plates, worked out by the space
    /// the samples are drawn in — the dictionary's, or DeviceCMYK for a CMYK
    /// JPEG drawn in its own colour — where that space names colorants.
    tints: Option<(ImageTints<'d>, ColorSpace)>,
    /// Whether that space names only `/None`, so the image paints nothing.
    nothing: bool,
    /// The tints, `tints`' channels a sample.
    plate_tints: Vec<u8>,
}

impl JpegInto<'_, '_> {
    /// Adds what the strips' converters asked of a conversion to `work`.
    fn count_into(&mut self, work: Option<&Work>) {
        if let Some((_, converter)) = self.space.as_mut() {
            converter.count_into(work);
        }
        if let Some(own) = self.own.as_mut() {
            own.count_into(work);
        }
    }
}

impl JpegSink for JpegInto<'_, '_> {
    fn begin(&mut self, layout: JpegLayout) {
        self.color = layout.color;
        self.width = layout.width as usize;
        let pixels = self.width.saturating_mul(layout.height as usize);
        self.rgb = Vec::with_capacity(pixels.saturating_mul(3));
        let components = usize::from(jpeg_components(layout.color));
        self.space = self
            .resources
            .codec_space(self.named.take(), components, self.name)
            .map(|space| {
                let converter = SpaceSamples::new(&space, 8, self.decode, self.conversion);
                (space, converter)
            });
        // The space the samples' tints are read in: the one they are drawn
        // in, or a CMYK JPEG's own ink.
        let tinted = match (&self.space, layout.color) {
            (Some((space, _)), _) => Some(space.clone()),
            (None, JpegColor::Cmyk | JpegColor::CmykInverted) => Some(ColorSpace::DeviceCmyk),
            (None, JpegColor::Gray | JpegColor::Rgb) => None,
        };
        if let (Some(plan), Some(space)) = (self.plates, tinted) {
            match ImagePlating::of(&space, plan) {
                Some(ImagePlating::Tints(tints)) => {
                    self.plate_tints = Vec::with_capacity(pixels.saturating_mul(tints.channels()));
                    self.tints = Some((tints, space));
                }
                Some(ImagePlating::Nothing) => self.nothing = true,
                None => {}
            }
        }
        self.own = (self.space.is_none()
            && matches!(layout.color, JpegColor::Cmyk | JpegColor::CmykInverted))
        .then(|| {
            Converter::new(
                &ColorSpace::DeviceCmyk,
                4,
                8,
                &[],
                icc::Conversion::default(),
                false,
            )
        });
    }

    fn strip(&mut self, _: u32, rows: u32, data: &[u8]) {
        let pixels = (rows as usize).saturating_mul(self.width);
        // The strip's light starts where the image's so far ends.
        let first = self.rgb.len() / 3;
        self.convert_strip(data, pixels);
        if let Some((tints, space)) = self.tints.as_mut() {
            let components = usize::from(jpeg_components(self.color));
            let mut values = Vec::with_capacity(components);
            for i in 0..pixels {
                values.clear();
                values.extend(
                    (0..components)
                        .map(|c| u32::from(data.get(i * components + c).copied().unwrap_or(0))),
                );
                let at = (first + i) * 3;
                let light = match self.rgb.get(at..at + 3) {
                    Some(&[r, g, b]) => (r, g, b),
                    _ => (255, 255, 255),
                };
                tints.push(space, self.decode, &values, 8, light, &mut self.plate_tints);
            }
        }
    }
}

impl JpegInto<'_, '_> {
    /// The light of a strip of `pixels` samples, `data`, appended.
    fn convert_strip(&mut self, data: &[u8], pixels: usize) {
        match self.space.as_mut() {
            // Drawn in its dictionary's space, a sample takes `/Decode`
            // before the space, as the sample loop's does.
            Some((space, converter)) => converter.extend(space, data, pixels, &mut self.rgb),
            // Drawn in its own colour, `/Decode` is applied to the converted
            // strip instead.
            None => {
                let start = self.rgb.len();
                match self.own.as_mut() {
                    Some(own) => {
                        own.extend(&ColorSpace::DeviceCmyk, &[], data, pixels, &mut self.rgb);
                    }
                    None => jpeg_rgb(self.color, data, pixels, &mut self.rgb),
                }
                invert_if_decode_reverses(
                    self.decode,
                    self.rgb.get_mut(start..).unwrap_or_default(),
                );
            }
        }
    }
}

/// What a JBIG2 image's decode is allowed to cost, as the pixels
/// [`Jbig2Params::work_ceiling`] measures it against: the image's own
/// `/Width` x `/Height`, plus a floor that holds one shared dictionary.
///
/// **Not the output ceiling.** The decoder is handed `MAX_DECODED_STREAM` for
/// every image, which is a bound on any stream rather than a statement about
/// this one, and [`MAX_JBIG2_WORK_MULTIPLE`] times the 2^30 pixels it holds was
/// a budget of 17 179 869 184 units: sixteen copies of a 117-byte dictionary
/// deciding symbols past its marker, 2 078 bytes in all, decoded for 10.2 s in
/// release and were not refused. The image is what the page asked for, so its
/// area is the scale.
///
/// **The floor is for `/JBIG2Globals`.** A global dictionary is decoded whole
/// for every image that names it (7.4.7), and an image may be a small mask on a
/// page whose glyphs a dictionary shared by two hundred pages holds, so the
/// image's area alone would refuse a document for the shape its producer
/// chose. One dictionary may decode `MAX_JBIG2_SYMBOL_PIXELS` pixels, each of
/// them an arithmetic decision; the integers that size and export its symbols
/// and the copy a text region takes of them are less than that again. So the
/// floor is that cap twice over, in units — 134 217 728, which at sixteen units
/// a pixel is 8 388 608 pixels, about one 300 dpi A4 page — and a dictionary the
/// cap admits is never refused by the budget, whatever image names it.
fn jbig2_work_ceiling(width: u32, height: u32) -> u64 {
    let globals = 2 * MAX_JBIG2_SYMBOL_PIXELS / MAX_JBIG2_WORK_MULTIPLE;
    (u64::from(width) * u64::from(height)).saturating_add(globals)
}

/// Rewrites an inline image's dictionary into the long spellings the shared
/// image path reads (8.9.7, Table 93).
///
/// Name by name rather than by text substitution. A search for `"/Fl "` finds
/// neither `/F/Fl` nor `[/AHx/Fl]`, and both are ordinary PDF — 7.2.2 makes
/// the `/` its own delimiter, so no producer is obliged to write the space
/// that search depends on. The consequence was silent and total: the whole
/// `/Filter` entry stayed a two-letter name nothing matched, so a compressed
/// inline image was sampled straight out of its compressed bytes, and `/DP`
/// written tight against its dictionary took the predictor down with it.
///
/// **A name is read where it stands.** `/I` is the one abbreviation both of
/// 8.9.7's tables use: Table 93's key `/Interpolate` and Table 94's family
/// `/Indexed`, which only begins an array — `/CS [/I /RGB 1 <…>]`. Every `/I`
/// was the key until October 2026, so that space became `[/Interpolate …]`,
/// painted its indices as grey and was listed as a space nobody could read.
/// And a string is not names: a palette written as a literal string carries
/// whatever bytes its colours are, `(/I…)` among them, and is copied as it
/// stands rather than rewritten into a different palette.
fn expand_inline_abbreviations(dict: &[u8]) -> Vec<u8> {
    // Every abbreviation of Table 93 and of Table 94, keys and values both:
    // `/CS /G` is a colour space named the short way, and `/F /AHx` a filter.
    // `/I` is read by where it stands, below, and so is not here.
    const ABBREVIATIONS: &[(&[u8], &[u8])] = &[
        (b"BPC", b"BitsPerComponent"),
        (b"CS", b"ColorSpace"),
        (b"D", b"Decode"),
        (b"DP", b"DecodeParms"),
        (b"F", b"Filter"),
        (b"H", b"Height"),
        (b"IM", b"ImageMask"),
        (b"W", b"Width"),
        (b"G", b"DeviceGray"),
        (b"RGB", b"DeviceRGB"),
        (b"CMYK", b"DeviceCMYK"),
        (b"AHx", b"ASCIIHexDecode"),
        (b"A85", b"ASCII85Decode"),
        (b"LZW", b"LZWDecode"),
        (b"Fl", b"FlateDecode"),
        (b"RL", b"RunLengthDecode"),
        (b"CCF", b"CCITTFaxDecode"),
        (b"DCT", b"DCTDecode"),
    ];
    // 7.2.2: everything that ends a name. A regular character is anything
    // else — `#` included, so an escaped name is read whole, matches no
    // abbreviation and comes back out exactly as it went in.
    const DELIMITERS: &[u8] = b"()<>[]{}/%";
    let is_regular = |b: u8| !b.is_ascii_whitespace() && b != 0 && !DELIMITERS.contains(&b);

    let mut out: Vec<u8> = Vec::with_capacity(dict.len() + 32);
    out.extend_from_slice(b"<< ");
    let mut i = 0usize;
    // Whether the next object is an array's first element, where `/I` is the
    // family `/Indexed` rather than the key `/Interpolate`.
    let mut array_start = false;
    while let Some(&byte) = dict.get(i) {
        let copied = match byte {
            // 7.3.4: a string, copied whole — a literal one to its balancing
            // parenthesis, a hexadecimal one to its `>`.
            b'(' => literal_string_end(dict, i),
            b'<' if dict.get(i + 1) == Some(&b'<') => i + 2,
            b'<' => dict
                .iter()
                .skip(i)
                .position(|&b| b == b'>')
                .map_or(dict.len(), |at| i + at + 1),
            // 7.2.4: a comment runs to the end of its line, and is no object.
            b'%' => {
                let end = dict
                    .iter()
                    .skip(i)
                    .position(|&b| b == b'\r' || b == b'\n')
                    .map_or(dict.len(), |at| i + at);
                out.extend_from_slice(dict.get(i..end).unwrap_or_default());
                i = end;
                continue;
            }
            b'/' => {
                let start = i + 1;
                let mut end = start;
                while dict.get(end).copied().is_some_and(is_regular) {
                    end += 1;
                }
                let name = dict.get(start..end).unwrap_or_default();
                let long = if name == b"I" {
                    if array_start {
                        b"Indexed".as_slice()
                    } else {
                        b"Interpolate".as_slice()
                    }
                } else {
                    ABBREVIATIONS
                        .iter()
                        .find(|(short, _)| *short == name)
                        .map_or(name, |(_, long)| *long)
                };
                out.push(b'/');
                out.extend_from_slice(long);
                // No separator is added: whatever ended the short name —
                // white space, a delimiter or the end of the dictionary —
                // ends the long one too.
                i = end;
                array_start = false;
                continue;
            }
            _ => i + 1,
        };
        out.extend_from_slice(dict.get(i..copied).unwrap_or_default());
        if !byte.is_ascii_whitespace() && byte != 0 {
            array_start = byte == b'[';
        }
        i = copied;
    }
    out.extend_from_slice(b" >>");
    out
}

/// Where the literal string opening at `start` ends (7.3.4.2): one past the
/// parenthesis that balances it, a backslash taking the byte after it with
/// it, or the end of `bytes` for a string that never closes.
fn literal_string_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// 11.6.6's `/CS`, reduced to the blending space a compositor needs.
///
/// By component count, plus `/Lab` named separately because its components are
/// not in `0..1` and no count would say so. An `ICCBased` space reaches here as
/// `ColorSpace::Approximated`, which is 8.6.5.5's alternate-space reading and
/// carries exactly the count this needs; what the profile would have said about
/// the *meaning* of those components is `docs/design/icc.md`'s.
fn group_space(space: tinker_pdf_color::ColorSpace) -> tinker_pdf_content::GroupSpace {
    use tinker_pdf_content::GroupSpace;
    if matches!(space, tinker_pdf_color::ColorSpace::Lab { .. }) {
        return GroupSpace::Lab;
    }
    match space.components() {
        1 => GroupSpace::Gray,
        4 => GroupSpace::Cmyk,
        // Three, or anything a group has no business declaring, reads as RGB —
        // which is what this build composites in, so an unexpected count is
        // the case that needs no warning rather than the one that does.
        _ => GroupSpace::Rgb,
    }
}

/// Table 90's default `/Decode` pair for component `c` of an image in
/// `space`, where it is not `[0 1]` and so not what a sample's fraction
/// already is: a `/Lab` sample spans `L*` over `0..100` and `a*`, `b*` over
/// the space's `/Range` (8.6.5.4, Table 90).
///
/// Until October 2026 an image in `/Lab` with no `/Decode` read every
/// sample as a fraction of one, so `L*` ran 0 to 1 and the image was all but
/// black. Every other space's default is `[0 1]` — an `/Indexed` one's is
/// its index range, which the sample loop reads as the index itself.
fn default_decode(space: &ColorSpace, c: usize) -> Option<(f64, f64)> {
    match space {
        ColorSpace::Lab { range } => match c {
            0 => Some((0.0, 100.0)),
            1 => Some((range[0], range[1])),
            2 => Some((range[2], range[3])),
            _ => None,
        },
        _ => None,
    }
}

/// The D50 white point, and the default when a CIE-based space names none.
///
/// 8.6.5.2 and 8.6.5.3 make `/WhitePoint` required, so this is the answer for a
/// dictionary that omits it or writes fewer than three numbers — a file that
/// has not described a white point rather than one that described a different
/// one.
const WHITE_D50: [f64; 3] = [0.964_212, 1.0, 0.825_188];

/// `read` of `object` resolved, **where it lies**: an indirect object is the
/// document's cached one and a direct one is `object` itself.
///
/// Not [`CosDocument::resolve`], which hands a direct object back as a fresh
/// `Arc` of a deep copy. That is nothing for a number and everything for a
/// long array, and the readers this serves run once per operator: a review
/// measured a page of 2 000 `/X /P0 BDC EMC` naming one list beside a
/// 1 048 576-entry array at 150 s to extract text from while each `BDC` was
/// handed a copy, against 0.38 s with the list borrowed.
pub(crate) fn read_resolved<R>(
    doc: &CosDocument,
    object: &Object,
    read: impl FnOnce(&Object) -> R,
) -> R {
    if object.as_objref().is_some() {
        read(&doc.resolve(object))
    } else {
        read(object)
    }
}

/// `read` of the value of `key` in `dict`, resolved where it lies
/// ([`read_resolved`]): what [`CosDocument::resolve_key`] answers, `Null` for
/// an absent key, without its copy of a direct value.
///
/// For the readers that take values **out of** a dictionary read where it
/// lies: the dictionary is not copied, and a value under one of the keys
/// they read must not be either, or a long array under `/Alt` costs what it
/// cost beside the keys before.
pub(crate) fn read_key<R>(
    doc: &CosDocument,
    dict: &Dict,
    key: &[u8],
    read: impl FnOnce(&Object) -> R,
) -> R {
    match dict.get(doc.intern(key)) {
        Some(value) => read_resolved(doc, value, read),
        None => read(&Object::Null),
    }
}

impl PageResources {
    /// `read` of the property list this scope's `/Properties` names `name`
    /// (14.6.2), resolved: what `/Tag /name BDC` refers to. `None` when there
    /// is no such list or it is not a dictionary.
    ///
    /// Lent rather than returned, because neither the table nor the list is
    /// copied to answer: each is the document's cached object when it is
    /// indirect and the resource dictionary's own when it is direct (see
    /// [`read_resolved`]). Every named `BDC` on every reading of a page asks,
    /// so a copy here is the list's size times the page's sequences —
    /// `tests/property_list_work.rs` counts it. The same holds for every value
    /// `read` takes out of the list, which is why the readers lent it read
    /// each through [`read_key`] rather than `resolve_key`.
    pub(crate) fn with_property_list<R>(
        &self,
        name: &[u8],
        read: impl FnOnce(&Dict) -> R,
    ) -> Option<R> {
        let resources = self.resources.as_deref()?;
        let table = resources.get(Name::PROPERTIES)?;
        read_resolved(&self.doc, table, |table| {
            let entry = table.as_dict()?.get(self.name(name))?;
            read_resolved(&self.doc, entry, |list| list.as_dict().map(read))
        })
    }

    /// The font resource names that could not be resolved.
    #[must_use]
    pub fn missing_fonts(&self) -> Vec<String> {
        self.missing_fonts
            .lock()
            .map(|names| names.clone())
            .unwrap_or_default()
    }

    /// Images that decoded with a leniency, and which leniency (ruling 10).
    #[must_use]
    pub fn damaged_images(&self) -> Vec<(String, String)> {
        self.damaged_images
            .lock()
            .map(|images| images.clone())
            .unwrap_or_default()
    }

    /// Colour spaces read with a repair, and which repair (ruling 10).
    #[must_use]
    pub(crate) fn repaired_spaces(&self) -> Vec<(String, String)> {
        self.repaired_spaces
            .lock()
            .map(|spaces| spaces.clone())
            .unwrap_or_default()
    }

    /// Images drawn from a decode the document's budget reduced past what
    /// their drawn size asks, and the reduction each was drawn at (ruling 10,
    /// `docs/design/performance.md`, P-16, "Reduced-resolution decode").
    #[must_use]
    pub(crate) fn reduced_images(&self) -> Vec<(String, u32)> {
        self.reduced_images
            .lock()
            .map(|images| images.clone())
            .unwrap_or_default()
    }

    /// Adds an image drawn from a decode the budget reduced, and its
    /// reduction, to what this render met, once.
    fn note_reduced_image(&self, entry: (String, u32)) {
        if let Ok(mut reduced) = self.reduced_images.lock() {
            if reduced.len() < 64 && !reduced.contains(&entry) {
                reduced.push(entry);
            }
        }
    }

    /// Adds repairs some other pass over the same content met — the
    /// interpretation a retained page recorded — to what this render reports,
    /// as [`PageResources::note_missing_fonts`] does for fonts.
    pub(crate) fn note_repaired_spaces(&self, entries: &[(String, String)]) {
        for entry in entries {
            self.note_repaired_space(entry.clone());
        }
    }

    /// These resources again, for **one render** of a page whose resources
    /// are kept across renders — a retained page's: every cache shared, so
    /// nothing decoded or extracted is paid for twice, and nothing tolerated
    /// yet, so what the render reports is what it met.
    ///
    /// A direct render builds its resources fresh and reports what its own
    /// interpretation and drawing ran into. A retained page's resources
    /// outlive its renders, and their lists used to as well: a cancelled
    /// replay reported a font the recording had met, and a region reported
    /// what a pattern cell drawn by an earlier render had met. What a cache
    /// hit stands for — an image's damage, a glyph its font could not resolve —
    /// is kept in the cache entry and said again by every render that meets
    /// it, as that render's own decode would have.
    #[must_use]
    pub(crate) fn for_one_render(&self) -> PageResources {
        self.for_one_render_counted(None, None)
    }

    /// [`PageResources::for_one_render`], counting into `work`, for a render
    /// naming `usage`: a counted render's resources over a retained page's
    /// caches. A render is an operation of its own, so it reads what it
    /// decodes into streams of its own, dropped with it, and binds its own
    /// optional-content states. Building it reads nothing — its fonts are the
    /// kept scope's table, shared — so it counts nothing itself.
    #[must_use]
    pub(crate) fn for_one_render_counted(
        &self,
        work: Option<Arc<Work>>,
        usage: Option<Usage>,
    ) -> PageResources {
        self.serving(Operation::rendering(work, usage))
    }

    /// [`PageResources::for_one_render`], serving `op`: a render that
    /// counts as `op` counts and keeps what it computes where `op` keeps it —
    /// the document's cache, for a document with a budget.
    #[must_use]
    pub(crate) fn for_operation(&self, op: Operation) -> PageResources {
        self.serving(op)
    }

    /// These resources over the same dictionary and caches, serving `op`:
    /// nothing tolerated yet, and every stream read through `op`'s and every
    /// step counted into its tally.
    fn serving(&self, op: Operation) -> PageResources {
        PageResources {
            doc: Arc::clone(&self.doc),
            resources: self.resources.clone(),
            caches: Arc::clone(&self.caches),
            missing_fonts: Mutex::new(Vec::new()),
            damaged_images: Mutex::new(Vec::new()),
            reduced_images: Mutex::new(Vec::new()),
            repaired_spaces: Mutex::new(Vec::new()),
            image_capture: Mutex::new(None),
            unresolved: Mutex::new(None),
            recording: Mutex::new(Vec::new()),
            provider: self.provider.clone(),
            op,
        }
    }

    /// A scope a cache keeps, for an asker in this scope's operation: the
    /// kept scope over its own caches, serving the asker's operation —
    /// reading every stream through the operation's decoded streams and
    /// counting into its tally — so a hit stays a hit, a stream the
    /// operation's other scopes decoded is not decoded again, and two renders
    /// holding one kept scope at once read and count apart. The kept scope
    /// itself for an asker that serves no operation.
    pub(crate) fn bind(&self, kept: &Arc<PageResources>) -> Arc<PageResources> {
        if self.op.is_none() {
            return Arc::clone(kept);
        }
        Arc::new(kept.serving(self.op.clone()))
    }

    /// These resources, kept past the operation they served — a retained
    /// page's: they serve none from here, so the streams that operation
    /// decoded go with it, and each later operation is handed them bound to
    /// its own ([`PageResources::bind`], [`PageResources::for_one_render`]).
    #[must_use]
    pub(crate) fn kept(mut self) -> PageResources {
        self.op = Operation::none();
        self
    }

    /// The tally these resources count into, when their operation is
    /// counting.
    pub(crate) fn work(&self) -> Option<&Arc<Work>> {
        self.op.work()
    }

    /// The operation these resources serve, for a scope built beside them in
    /// the same operation: an annotation's appearance, drawn by the page's
    /// render.
    pub(crate) fn operation(&self) -> &Operation {
        &self.op
    }

    /// What tells this scope from another: the address of its caches, which
    /// a kept scope shares with every binding of it ([`PageResources::bind`])
    /// and a retained page with every render of it, whatever operation each
    /// serves. Two scopes alive at once are one scope exactly when this is
    /// equal.
    pub(crate) fn scope_id(&self) -> usize {
        Arc::as_ptr(&self.caches) as usize
    }

    /// Counts one step of `counter` into the tally, when there is one.
    fn count(&self, counter: Counter) {
        stats::count(self.op.work.as_deref(), counter);
    }

    /// The symbol for `bytes` in the document's name table: every name this
    /// scope looks up by its bytes — a resource name its content gives, and a
    /// key that is not one of the pre-interned [`Name`] constants — comes
    /// here, so that a lookup a constant would have answered is counted as a
    /// key looked up (`Stats::keys_looked_up`) when the operation counts
    /// (`docs/design/performance.md`, P-02).
    fn name(&self, bytes: &[u8]) -> Name {
        let name = self.doc.intern(bytes);
        if name.is_preinterned() {
            self.count(Counter::KeysLookedUp);
        }
        name
    }

    /// `r`'s bytes through its filter chain, once the operation: the first
    /// read runs the chain, counted into the tally when there is one, and a
    /// later read in any scope of the operation is handed what it decoded
    /// ([`Operation::decoded`]).
    pub(crate) fn decoded(&self, r: ObjRef) -> Result<Arc<Vec<u8>>, CosError> {
        self.op.decoded(&self.doc, r)
    }

    /// An image's samples through its filter chain, read at each decode of
    /// the image and never kept beside it: the scope's image cache keeps the
    /// decoded image, and holding its samples too would hold the image twice.
    /// The run counted as [`PageResources::decoded`] counts one.
    fn samples(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        self.doc.stream_decoded_counted(r, self.op.work.as_deref())
    }

    /// `r`'s bytes as an image codec takes them, an image's samples too, and
    /// counted as [`PageResources::samples`] counts.
    fn image_input(&self, r: ObjRef) -> Result<Vec<u8>, CosError> {
        self.doc
            .stream_image_input_counted(r, self.op.work.as_deref())
    }

    /// How many damaged images, repaired spaces and reduced images these
    /// resources have met so far: what a caller delivering calls one at a
    /// time reads after each, to say which call each was first met at
    /// (`display.rs`, a page's bands).
    pub(crate) fn tolerated(&self) -> Tolerated {
        Tolerated {
            damaged: self.damaged_images.lock().map_or(0, |list| list.len()),
            repaired: self.repaired_spaces.lock().map_or(0, |list| list.len()),
            reduced: self.reduced_images.lock().map_or(0, |list| list.len()),
        }
    }

    /// Adds fonts some other pass over the same content could not resolve —
    /// the interpretation a retained page recorded — to what this render
    /// reports, as though its own interpretation had met them.
    pub(crate) fn note_missing_fonts(&self, names: &[String]) {
        for name in names {
            self.note_missing_font(name.clone());
        }
    }

    /// Reads a page's resource dictionary, for an operation of its own.
    #[must_use]
    pub fn new(
        doc: &Arc<CosDocument>,
        page: &cos_pages::Page,
        provider: Option<&Arc<dyn FontProvider>>,
    ) -> PageResources {
        PageResources::new_in(doc, page, provider, Operation::new(None))
    }

    /// [`PageResources::new`] for `op`: every stream the scope reads is read
    /// through the operation's decoded streams, each font the scope reads
    /// counts when the operation counts, and the scope carries both.
    #[must_use]
    pub(crate) fn new_in(
        doc: &Arc<CosDocument>,
        page: &cos_pages::Page,
        provider: Option<&Arc<dyn FontProvider>>,
        op: Operation,
    ) -> PageResources {
        PageResources::over(doc, page.resources.clone().map(Arc::new), provider, op)
    }

    /// Resources over `resources`, with nothing read and nothing kept yet.
    fn over(
        doc: &Arc<CosDocument>,
        resources: Option<Arc<Dict>>,
        provider: Option<&Arc<dyn FontProvider>>,
        op: Operation,
    ) -> PageResources {
        PageResources {
            doc: doc.clone(),
            resources,
            caches: Arc::default(),
            missing_fonts: Mutex::new(Vec::new()),
            damaged_images: Mutex::new(Vec::new()),
            reduced_images: Mutex::new(Vec::new()),
            repaired_spaces: Mutex::new(Vec::new()),
            image_capture: Mutex::new(None),
            unresolved: Mutex::new(None),
            recording: Mutex::new(Vec::new()),
            provider: provider.cloned(),
            op,
        }
    }

    /// The compiled transforms for an `ICCBased` profile stream, one for each
    /// rendering intent, or `None` where the profile cannot be made into one.
    ///
    /// `None` covers two different things, and deliberately does not
    /// distinguish them here: a stream that will not decode, and a profile
    /// `Profile::parse` refuses by name. Both land on 8.6.5.5's
    /// alternate-space reading, which is what the caller was doing before
    /// profiles were read at all, so neither costs a page anything it had.
    /// Relative colorimetric's transform is compiled here; another intent's
    /// is compiled the first time a conversion asks for it, and kept with the
    /// rest (`docs/design/icc.md`, "What compiling per intent costs").
    ///
    /// A document with a budget keeps the transforms in its cache under the
    /// profile's bytes (`cache::IccKey`, `docs/design/icc.md`, P-22, "The
    /// document cache: its key and its charge"), so a profile named on many
    /// pages, forms and renders, or embedded as many objects, compiles once
    /// while it fits, charged the most bytes the transforms can come to hold
    /// whichever intents are compiled later (`Transforms::bytes_bound`) and
    /// the profile's bytes its key holds. The bytes each stream holds are
    /// kept beside them under the stream's object (`cache::ProfileKey`), so a
    /// stream is decoded and its bytes digested once a document while that is
    /// kept. A refusal is kept as firmly — a profile that will not parse
    /// under its bytes, a stream that will not decode under its object — so
    /// each is refused once a document. Past the budget each use compiles,
    /// as without one; and without one each scope keeps its own, as it always
    /// did.
    fn icc_transform(
        &self,
        reference: tinker_pdf_cos::ObjRef,
    ) -> Option<Arc<tinker_pdf_color::icc::Transforms>> {
        if let Some(cache) = self.op.cache() {
            let work = self.op.work.as_deref();
            let (profile, read) = cache.profile(ProfileKey(reference), work, || {
                match self.decoded(reference) {
                    Ok(bytes) => {
                        let key = IccKey::srgb(bytes);
                        let held = key.held();
                        Fill::Keep(Some(key), held)
                    }
                    Err(_) => Fill::Keep(None, 0),
                }
            });
            let Some(profile) = profile else {
                // A stream that will not decode has no profile to compile: it
                // is refused where it was read, as a profile that will not
                // parse is refused where its bytes were.
                self.count(if read {
                    Counter::IccTransformHits
                } else {
                    Counter::IccTransformMisses
                });
                return None;
            };
            let (compiled, hit) = cache.icc(profile.clone(), work, || {
                // Compiled keeping the key's bytes as the transforms' source,
                // shared rather than copied, so a command stream writes the
                // space by them (`Transforms::source`).
                let compiled = tinker_pdf_color::icc::Transforms::compile_shared(profile.shared())
                    .map(Arc::new);
                let bytes = compiled
                    .as_ref()
                    .map_or(0, |transforms| transforms.bytes_bound());
                let bytes = u64::try_from(bytes)
                    .unwrap_or(u64::MAX)
                    .saturating_add(profile.held());
                Fill::Keep(compiled, bytes)
            });
            self.count(if hit {
                Counter::IccTransformHits
            } else {
                Counter::IccTransformMisses
            });
            return compiled;
        }
        let key = (reference.num, reference.gen);
        let made = Cell::new(false);
        let compiled = self.caches.icc.read(key, || {
            made.set(true);
            self.count(Counter::IccTransformMisses);
            Filled::Keep(self.compile_icc(reference))
        });
        if !made.get() {
            self.count(Counter::IccTransformHits);
        }
        compiled
    }

    /// The transforms of the ICC profile stream `reference`, compiled, or
    /// `None` where the stream will not decode or the profile not parse.
    fn compile_icc(
        &self,
        reference: tinker_pdf_cos::ObjRef,
    ) -> Option<Arc<tinker_pdf_color::icc::Transforms>> {
        self.decoded(reference)
            .ok()
            .and_then(tinker_pdf_color::icc::Transforms::compile_shared)
            .map(Arc::new)
    }

    /// The page's own transparency group space (11.4.7), if it declares one.
    ///
    /// A page-level `/Group` is not a form's: nothing invokes it, so there is
    /// no `Do` for the interpreter to notice and no `Group` value to carry.
    /// 11.4.7 makes it the space the *page* composites in, which is why it is
    /// read here and asked for by `Page::render` rather than arriving through
    /// the content stream like every other group.
    #[must_use]
    pub fn page_group_space(
        &self,
        page: &cos_pages::Page,
    ) -> Option<tinker_pdf_content::GroupSpace> {
        let object = self.doc.get(page.reference).ok()?;
        let dict = object.as_dict()?;
        let group = self.doc.resolve_key(dict, self.name(b"Group"));
        let group = group.as_dict()?;
        // 11.6.6: the subtype is checked rather than the key's presence, for
        // the reason `form_from` checks it — 8.10.3's `/S /Reference` group is
        // a different thing wearing the same key.
        let subtype = self
            .doc
            .resolve_key(group, self.name(b"S"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Transparency" {
            return None;
        }
        let cs = group.get(self.name(b"CS")).cloned()?;
        self.parse_space(&cs, 0, b"Group").map(group_space)
    }

    /// Reads a resource dictionary that is not a page's, for `op`, as
    /// [`PageResources::new_in`] reads a page's.
    ///
    /// An annotation's appearance stream carries its own resources, and
    /// resolving its fonts against the page's would find the wrong ones or
    /// none at all.
    #[must_use]
    pub(crate) fn from_dict_in(
        doc: &Arc<CosDocument>,
        dict: Dict,
        provider: Option<&Arc<dyn FontProvider>>,
        op: Operation,
    ) -> PageResources {
        PageResources::over(doc, Some(Arc::new(dict)), provider, op)
    }

    /// A pattern resource: its dictionary, and the object it lives in when it
    /// is an indirect one (8.7.3).
    ///
    /// The reference is what a *tiling* pattern needs and a shading pattern
    /// does not — type 1 is a stream and its cell is the stream's bytes, type 2
    /// is a dictionary that may perfectly well be written inline. So both come
    /// back, and both routes read the dictionary through the same lookup:
    /// `/PatternType` deciding the kind is the one thing they must agree on.
    fn pattern_entry(&self, name: &[u8]) -> Option<(Dict, Option<ObjRef>)> {
        let resources = self.resources.as_deref()?;
        let table = self.doc.resolve_key(resources, Name::PATTERN);
        let table = table.as_dict()?;
        let key = self.name(name);
        let reference = table.get_ref(key);
        let entry = self.doc.resolve_key(table, key);
        Some((entry.as_dict()?.clone(), reference))
    }

    /// The stream of the tiling pattern `name` selects in this scope, whose
    /// bytes are its cell (8.7.3.2) — the stream [`GlyphSource::tile`]
    /// paints. `None` for a shading pattern, which has no cell, and for a
    /// name this scope does not have.
    ///
    /// For [`crate::subset`], whose walk has to enter a cell the interpreter
    /// never runs: the renderer paints it itself.
    pub(crate) fn tiling_cell(&self, name: &[u8]) -> Option<ObjRef> {
        let (dict, reference) = self.pattern_entry(name)?;
        let kind = self
            .doc
            .resolve_key(&dict, self.name(b"PatternType"))
            .as_int();
        (kind == Some(1)).then_some(reference).flatten()
    }

    /// The scope a stream's own `/Resources` makes: a tiling pattern's, a
    /// mask group's or a form's, built as [`PageResources::from_dict_in`]
    /// builds any and serving this scope's operation. `None` when the stream
    /// has none, and its names resolve where it was painted (8.7.3.2,
    /// 8.10.1).
    pub(crate) fn own_scope(&self, stream: ObjRef) -> Option<PageResources> {
        let object = self.doc.get(stream).ok()?;
        let own = self
            .doc
            .resolve_key(object.as_dict()?, Name::RESOURCES)
            .as_dict()?
            .clone();
        Some(PageResources::from_dict_in(
            &self.doc,
            own,
            self.provider.as_ref(),
            self.op.clone(),
        ))
    }

    /// A pattern's `/Matrix`, which maps pattern space to the parent content
    /// stream's default space (8.7.3.1).
    fn pattern_matrix(&self, dict: &Dict) -> Matrix {
        self.doc
            .resolve_key(dict, self.name(b"Matrix"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .and_then(|v| {
                (v.len() >= 6 && v.iter().all(|x| x.is_finite())).then(|| Matrix {
                    a: v[0],
                    b: v[1],
                    c: v[2],
                    d: v[3],
                    e: v[4],
                    f: v[5],
                })
            })
            .unwrap_or(Matrix::IDENTITY)
    }

    /// A tiling pattern's geometry (8.7.3.2, Table 75).
    ///
    /// The cell's pixels are not read here: they are a content stream, and the
    /// renderer decides how large a buffer to draw it into and whether the
    /// lattice is affordable before anything is rasterized.
    fn tiling_pattern(&self, dict: &Dict, matrix: Matrix) -> PatternPaint {
        let number = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.name(key))
                .as_number()
                .filter(|v| v.is_finite())
        };

        // Required, and the cell has no extent without it. The corners may be
        // given in either order (7.9.5), as they may on a form's `/BBox`.
        let bbox = self
            .doc
            .resolve_key(dict, self.name(b"BBox"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| {
                    values
                        .get(i)
                        .and_then(|v| self.doc.resolve(v).as_number())
                        .filter(|v| v.is_finite())
                };
                let (x0, y0, x1, y1) = (n(0)?, n(1)?, n(2)?, n(3)?);
                Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
            });
        let Some(bbox) = bbox.filter(|b| b[2] > b[0] && b[3] > b[1]) else {
            return PatternPaint::Unsupported;
        };

        // 8.7.3.2: `/PaintType` 2 is the uncoloured pattern, which takes the
        // colour its `SCN` operands supplied. Anything that is not 2 is
        // coloured -- including a value the file got wrong, because a coloured
        // cell painted in its own colours is the reading that loses nothing.
        let uncolored = self
            .doc
            .resolve_key(dict, self.name(b"PaintType"))
            .as_int()
            .unwrap_or(1)
            == 2;

        PatternPaint::Tiling(Box::new(TilingPattern {
            matrix,
            bbox,
            // Absent or zero is invalid; the renderer falls back to the cell's
            // own size, which is the only reading that tiles.
            xstep: number(b"XStep").unwrap_or(0.0),
            ystep: number(b"YStep").unwrap_or(0.0),
            uncolored,
        }))
    }

    /// The `/Resources` a form XObject brings with it (8.10.1).
    ///
    /// A form written beside one document and pasted into another carries the
    /// only dictionary its names resolve in — which is why a stamp annotation
    /// whose appearance invokes a second form can name an image the page has
    /// never heard of. A form that omits the key resolves against the invoking
    /// scope, which is what its producer is relying on.
    ///
    /// The `provider` is carried into the nested scope for the reason the
    /// tiling path carries it: without it a form would be the one place in a
    /// document where a `FontProvider` the caller installed does not apply.
    fn form_resources(&self, name: &[u8]) -> Option<Arc<PageResources>> {
        let (dict, reference) = self.xobject(name)?;
        let key = (u64::from(reference.num) << 16) | u64::from(reference.gen);
        let made = Cell::new(false);
        let built = self.caches.form_scopes.read(key, || {
            made.set(true);
            self.count(Counter::FormScopeMisses);
            Filled::Keep(self.build_form_resources(&dict))
        });
        if !made.get() {
            self.count(Counter::FormScopeHits);
        }
        built.as_ref().map(|kept| self.bind(kept))
    }

    fn build_form_resources(&self, dict: &Dict) -> Option<Arc<PageResources>> {
        let subtype = self
            .doc
            .resolve_key(dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Form" {
            return None;
        }
        let own = self
            .doc
            .resolve_key(dict, Name::RESOURCES)
            .as_dict()
            .cloned()?;
        // The scope is kept serving no operation: the cache outlives this
        // one, and each asker reads and counts through its own.
        Some(Arc::new(PageResources::from_dict_in(
            &self.doc,
            own,
            self.provider.as_ref(),
            Operation::none(),
        )))
    }

    /// Whether this scope's `/XObject` dictionary names `name` and, if it
    /// does, the XObject's `/Subtype` — `Some(None)` for one with none.
    ///
    /// The one question `Page::render_form` needs answered that `form` cannot:
    /// `form` answers `None` alike for a name that is not there, an image and a
    /// stream that will not decode, and a caller who asked for one form is owed
    /// the difference.
    pub(crate) fn xobject_subtype(&self, name: &[u8]) -> Option<Option<Vec<u8>>> {
        let (dict, _) = self.xobject(name)?;
        Some(
            self.doc
                .resolve_key(&dict, Name::SUBTYPE)
                .as_name()
                .and_then(|n| self.doc.name_bytes(n))
                .map(|bytes| bytes.to_vec()),
        )
    }

    /// Whether the XObject `name` selects in this scope carries an `/SMask`
    /// key at all — `pdfa/colour.rs`'s reading of a mask on an XObject, the
    /// key and not its value, since an XObject's has no `/None` spelling.
    /// For page analysis (`crate::analysis`), which reads it at the `Do`.
    pub(crate) fn xobject_has_soft_mask(&self, name: &[u8]) -> bool {
        self.xobject(name)
            .is_some_and(|(dict, _)| dict.contains_key(self.name(b"SMask")))
    }

    pub(crate) fn xobject(&self, name: &[u8]) -> Option<(Dict, tinker_pdf_cos::ObjRef)> {
        let resources = self.resources.as_deref()?;
        let value = self.doc.resolve_key(resources, Name::XOBJECT);
        let dict = value.as_dict()?;
        let reference = dict.get_ref(self.name(name))?;
        let object = self.doc.get(reference).ok()?;
        let stream = object.as_dict()?.clone();
        Some((stream, reference))
    }

    /// A colour of `space` converted by `convert` through the space as
    /// painted, in `intent`: what [`FontSource::resolve_color`] answers as
    /// bytes and [`FontSource::resolve_color_wide`] at sixteen bits, from one
    /// reading of the space.
    ///
    /// 8.6.5.6: the light is the space as painted. A device space named
    /// directly — `g`, `rg` and `k` in a scope that remaps them, and
    /// `/DeviceRGB cs … sc` — converts through the default where it lies, so
    /// the most frequent colour operators copy nothing. An ICC space, or one
    /// holding one, converts in the intent in force, its transform for that
    /// intent compiled once a profile and kept with the rest.
    fn painted_color<R>(
        &self,
        space: &[u8],
        components: &[f64],
        intent: tinker_pdf_content::RenderingIntent,
        convert: impl Fn(&ColorSpace, &[f64], tinker_pdf_color::icc::Conversion) -> R,
    ) -> Option<R> {
        let conversion = intent.conversion();
        let device = match space {
            b"DeviceGray" => Some(ColorSpace::DeviceGray),
            b"DeviceRGB" => Some(ColorSpace::DeviceRgb),
            b"DeviceCMYK" => Some(ColorSpace::DeviceCmyk),
            _ => None,
        };
        match device {
            Some(device) => Some(match self.defaults().for_device(&device) {
                Some(default) => convert(default, components, conversion),
                // A device space converts alike in every intent.
                None => convert(&device, components, conversion),
            }),
            None => self.named_space(space, Answer::Painted, |space| {
                convert(space, components, conversion)
            }),
        }
    }

    /// `read` of the colour space a resource name selects (8.6.6), as
    /// `answer` reads it, or `None` for a name that selects none. As painted,
    /// a device space this scope remaps is its default (8.6.5.6); as
    /// selected, the space is the file's, whatever default the scope paints
    /// it in — what `cs` resets the colour to (8.6.8) and whether a colour is
    /// DeviceCMYK ink are asked of it (`docs/design/icc.md`, "One selection,
    /// two answers").
    ///
    /// Lent rather than returned: a name that is no device space's is read
    /// out of `/ColorSpace` once a scope, both readings at once, and kept
    /// ([`PageResources::kept_space`]), so the `cs`, `sc` and `scn` that name
    /// it copy nothing.
    fn named_space<R>(
        &self,
        name: &[u8],
        answer: Answer,
        read: impl FnOnce(&ColorSpace) -> R,
    ) -> Option<R> {
        // The device spaces may be named directly without appearing in
        // /ColorSpace at all — 8.6.8: they "never refer to resources", so the
        // default is what a resource dictionary is asked for. Matched by the
        // name and kept nowhere.
        let device = match name {
            // A bare `/CalGray` or `/CalRGB` name carries no parameter
            // dictionary at all, so there is no white point or gamma to read
            // and the device space *is* the whole of what the file said.
            b"DeviceGray" | b"CalGray" => ColorSpace::DeviceGray,
            b"DeviceRGB" | b"CalRGB" => ColorSpace::DeviceRgb,
            b"DeviceCMYK" => ColorSpace::DeviceCmyk,
            b"Pattern" => return Some(read(&ColorSpace::Pattern { base: None })),
            _ => {
                let kept = self.kept_space(name);
                let space = match answer {
                    Answer::Painted => kept.painted.as_ref(),
                    Answer::Selected => kept.selected.as_ref(),
                };
                return space.map(read);
            }
        };
        let space = match answer {
            Answer::Painted => self.defaults().for_device(&device).unwrap_or(&device),
            Answer::Selected => &device,
        };
        Some(read(space))
    }

    /// The colour space `name` selects through this scope's `/ColorSpace`,
    /// read on the first question about the name and kept
    /// ([`Caches::spaces`]); a later question notes again the repairs the
    /// reading noted, as [`FontSource::ext_g_state`] notes a state's, so a
    /// render's lists do not depend on whether it read the space or found it.
    fn kept_space(&self, name: &[u8]) -> Arc<KeptSpace> {
        if let Ok(cache) = self.caches.spaces.read() {
            if let Some(hit) = cache.get(name) {
                let hit = Arc::clone(hit);
                drop(cache);
                self.note_repaired_spaces(&hit.repaired_spaces);
                return hit;
            }
        }
        let read = Arc::new(self.read_named_space(name));
        if let Ok(mut cache) = self.caches.spaces.write() {
            if cache.len() < MAX_CACHED_ENTRIES {
                cache
                    .entry(name.to_vec())
                    .or_insert_with(|| Arc::clone(&read));
            }
        }
        read
    }

    /// Reads the colour space `name` selects, as
    /// [`PageResources::kept_space`] keeps it: the entry `/ColorSpace` holds
    /// for the name, as painted and as selected, counted as a colour space
    /// parsed; or, for a name it does not hold, the device space Table 93
    /// abbreviates by it, if it is one of those.
    fn read_named_space(&self, name: &[u8]) -> KeptSpace {
        // The scope's defaults first, so what they refused is said as the
        // scope's, once, as it always was, and not kept beside a space.
        let as_painted = self.painted();
        let before = self.repaired_spaces().len();
        let entry = self.resources.as_deref().and_then(|resources| {
            let table = resources.get(Name::COLOR_SPACE)?;
            read_resolved(&self.doc, table, |table| {
                let entry = table.as_dict()?.get(self.name(name))?;
                self.count(Counter::ColorSpacesParsed);
                let selected = self.read_space(entry, 0, name, Reading::SELECTED);
                // A scope that puts no default in paints a space as it was
                // selected, and the two readings are one.
                let painted = if self.defaults().puts_nothing_in() {
                    selected.clone()
                } else {
                    self.read_space(entry, 0, name, as_painted)
                };
                Some((painted, selected))
            })
        });
        // `/G`, `/RGB` and `/CMYK` are Table 93's abbreviations, which an
        // inline image may use and a content stream's `cs` may not — 8.6.8
        // gives `cs` a device space's own name or a `/ColorSpace` resource,
        // and `/G` is a perfectly good resource name. So the resources are
        // asked first and the abbreviation is the fallback. Until October
        // 2026 the abbreviation won, and a page that registered its
        // `/CalGray` as `/G` was drawn in DeviceGray, gamma and white point
        // unread.
        let (painted, selected) = entry.unwrap_or_else(|| {
            let abbreviated = match name {
                b"G" => Some(ColorSpace::DeviceGray),
                b"RGB" => Some(ColorSpace::DeviceRgb),
                b"CMYK" => Some(ColorSpace::DeviceCmyk),
                _ => None,
            };
            (
                abbreviated.clone().map(|device| as_painted.device(device)),
                abbreviated,
            )
        });
        KeptSpace {
            painted,
            selected,
            repaired_spaces: self.repaired_spaces().into_iter().skip(before).collect(),
        }
    }

    /// This scope's default colour spaces, read on the first question asked
    /// of it and shared by every render of it ([`Caches::defaults`]).
    fn defaults(&self) -> &Defaults {
        self.caches.defaults.get_or_init(|| self.read_defaults())
    }

    /// Every painting path's reading in this scope: images, shadings, a
    /// group's `/CS`, a mask's backdrop and the colour `sc` sets, each device
    /// space put in as this scope's default for it (8.6.5.6).
    fn painted(&self) -> Reading<'_> {
        Reading {
            defaults: Some(self.defaults()),
            inline: false,
            colorants: true,
        }
    }

    /// Reads `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` from this
    /// scope's `/ColorSpace` subdictionary (8.6.5.6), each as written.
    ///
    /// 8.6.5.6 allows any space but `/Lab`, `/Indexed` and `/Pattern`, with
    /// the device space's number of components. One that is not allowed, or
    /// that is no colour space at all, is refused: the device space is
    /// painted as it would be without it, and the refusal is named —
    /// `RepairedColorSpace` under the default's own name, once a render of
    /// the scope (ruling 10). The clause says what may be a default and not
    /// what a reader does with one that may not; drawing what the content
    /// asked for, and saying so, is ruling 2's answer.
    fn read_defaults(&self) -> Defaults {
        let mut defaults = Defaults::default();
        let Some(resources) = self.resources.as_deref() else {
            return defaults;
        };
        let table = self.doc.resolve_key(resources, Name::COLOR_SPACE);
        let Some(table) = table.as_dict() else {
            return defaults;
        };
        let families: [(&[u8], ColorSpace); 3] = [
            (b"DefaultGray", ColorSpace::DeviceGray),
            (b"DefaultRGB", ColorSpace::DeviceRgb),
            (b"DefaultCMYK", ColorSpace::DeviceCmyk),
        ];
        let mut written = Vec::new();
        for (family, (key, device)) in families.into_iter().enumerate() {
            let Some(entry) = table.get(self.name(key)) else {
                continue;
            };
            let refusal = match self.read_space(entry, 0, key, Reading::SELECTED) {
                None => Err("DefaultSpaceUnreadable"),
                Some(
                    ColorSpace::Lab { .. }
                    | ColorSpace::Indexed { .. }
                    | ColorSpace::Pattern { .. },
                ) => Err("DefaultSpaceFamily"),
                Some(space) if space.components() != device.components() => {
                    Err("DefaultSpaceComponents")
                }
                // The device space itself, `/DefaultRGB /DeviceRGB` or a
                // bare `/CalRGB`, paints what no default paints, and costs a
                // page's device operators nothing by being no default.
                Some(space) if space == device => Ok(None),
                Some(space) => Ok(Some(space)),
            };
            match refusal {
                Ok(default) => {
                    if default.is_some() {
                        written.push(u8::try_from(family).unwrap_or(u8::MAX));
                        cache::write_exact(entry, &mut written);
                    }
                    match device {
                        ColorSpace::DeviceGray => defaults.gray = default,
                        ColorSpace::DeviceRgb => defaults.rgb = default,
                        _ => defaults.cmyk = default,
                    }
                }
                Err(reason) => {
                    let at = match device {
                        ColorSpace::DeviceGray => 0,
                        ColorSpace::DeviceRgb => 1,
                        _ => 2,
                    };
                    defaults.refused[at] = Some(reason);
                    self.note_repaired_space((
                        String::from_utf8_lossy(key).into_owned(),
                        reason.to_string(),
                    ));
                }
            }
        }
        defaults.written = Written::of(written);
        defaults
    }

    /// A colour space value, read as every painting path reads one: each
    /// device space in it this scope remaps put in as its default.
    fn parse_space(&self, object: &Object, depth: u32, name: &[u8]) -> Option<ColorSpace> {
        self.read_space(object, depth, name, self.painted())
    }

    /// The colour alone of the spot `name`, which `definition` defines
    /// (RC-03.3; `docs/design/overprint-separations.md`, "The composite"):
    /// its space read as every painting path reads one, and the spot's
    /// component of it evaluated at every tint
    /// ([`tinker_pdf_render::spot_colour`]); white at every tint where the
    /// space cannot be read.
    pub(crate) fn spot_colour(
        &self,
        name: &[u8],
        definition: &crate::separations::SpotDefinition,
    ) -> tinker_pdf_raster::SpotColour {
        let (object, component) = definition.space();
        self.parse_space(object, 0, name)
            .map_or_else(tinker_pdf_raster::SpotColour::white, |space| {
                tinker_pdf_render::spot_colour(&space, component)
            })
    }

    fn read_space(
        &self,
        object: &Object,
        depth: u32,
        name: &[u8],
        how: Reading<'_>,
    ) -> Option<ColorSpace> {
        if depth > 8 {
            return None;
        }
        let resolved = self.doc.resolve(object);

        if let Some(family) = resolved.as_name() {
            let bytes = self.doc.name_bytes(family)?;
            return match bytes.as_ref() {
                b"DeviceGray" | b"G" | b"CalGray" => Some(how.device(ColorSpace::DeviceGray)),
                b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(how.device(ColorSpace::DeviceRgb)),
                b"DeviceCMYK" | b"CMYK" => Some(how.device(ColorSpace::DeviceCmyk)),
                b"Pattern" => Some(ColorSpace::Pattern { base: None }),
                // 8.9.7: an inline image's space may be a `/ColorSpace`
                // resource, read as an image XObject's would be — so the
                // entry, which is the space itself, cannot name another.
                // Until October 2026 the name fell through to DeviceGray
                // while `Page::images` listed the space it names.
                resource if how.inline => {
                    let entry = self.resources.as_deref().and_then(|resources| {
                        let table = self.doc.resolve_key(resources, Name::COLOR_SPACE);
                        table.as_dict()?.get(self.name(resource)).cloned()
                    })?;
                    let how = Reading {
                        inline: false,
                        ..how
                    };
                    self.read_space(&entry, depth + 1, name, how)
                }
                _ => None,
            };
        }

        let items = resolved.as_array()?;
        let family = items.first().and_then(Object::as_name)?;
        let family = self.doc.name_bytes(family)?;

        match family.as_ref() {
            b"ICCBased" => {
                let reference = items.get(1).and_then(Object::as_objref);
                let stream = items.get(1).map(|o| self.doc.resolve(o))?;
                let n = stream
                    .as_dict()
                    .and_then(|d| d.get_int(Name::N))
                    .unwrap_or(3);
                let components = n.clamp(1, 4) as usize;

                // The profile itself, if it can be read. Cached by object
                // number: a page naming the same space at a thousand `cs`
                // operators compiles it once.
                if let Some(reference) = reference {
                    if let Some(transforms) = self.icc_transform(reference) {
                        return Some(ColorSpace::Icc {
                            transforms,
                            components,
                        });
                    }
                }

                // 8.6.5.5: a reader may use the alternate space, and the
                // component count is what the data's shape actually is. This
                // is where a profile that would not parse lands, which is
                // exactly where it landed before profiles were read.
                Some(ColorSpace::Approximated { components })
            }
            b"Indexed" | b"I" => {
                // 8.6.5.6: an `/Indexed` base is remapped like any selection.
                let base = self.read_space(items.get(1)?, depth + 1, name, how)?;
                let high = items.get(2).and_then(|o| self.doc.resolve(o).as_int())?;
                let lookup = match items.get(3).map(|o| self.doc.resolve(o)) {
                    Some(value) => match value.as_string() {
                        Some(s) => s.bytes.clone(),
                        None => items
                            .get(3)
                            .and_then(Object::as_objref)
                            .and_then(|r| self.decoded(r).ok())
                            .map(Arc::unwrap_or_clone)
                            .unwrap_or_default(),
                    },
                    None => Vec::new(),
                };
                Some(ColorSpace::Indexed {
                    base: Box::new(base),
                    lookup,
                    high: high.clamp(0, 255) as u32,
                })
            }
            b"Separation" | b"DeviceN" => {
                let separation = family.as_ref() == b"Separation";
                let components = if separation {
                    1
                } else {
                    self.doc
                        .resolve(items.get(1)?)
                        .as_array()
                        .map_or(1, <[Object]>::len)
                };
                // The names, as `Page::images` describes them.
                let names = colorant_names(&self.doc, separation, items);
                // 8.6.5.6 remaps an alternate "only if the alternate colour
                // space is actually selected", which this engine does for
                // every named colorant, since no render paints one on a plate
                // of its own. 8.6.6.4's `/All` and `/None` are read here as
                // never selecting it, so their alternate is read as written.
                let selects_alternate =
                    !(separation && matches!(names.as_slice(), [Colorant::All | Colorant::None]));
                let alternate_how = Reading {
                    defaults: how.defaults.filter(|_| selects_alternate),
                    colorants: false,
                    ..how
                };
                let alternate = self.read_space(items.get(2)?, depth + 1, name, alternate_how)?;

                // 8.6.6.4: the fourth element converts tint values into the
                // alternate space. Left as the identity, a one-ink Separation
                // feeds its tint straight into a CMYK alternate as cyan, so
                // full-tint black prints cyan — and nothing about the result
                // looks like a bug. Spot colours are everywhere in
                // print-origin files.
                let tint = items
                    .get(3)
                    .and_then(|o| self.parse_function(o, depth + 1))
                    .unwrap_or(Function::Identity);

                let colorants = if separation || !how.colorants {
                    Colorants {
                        names,
                        ..Colorants::default()
                    }
                } else {
                    self.device_n_colorants(names, items, depth, name, how)
                };
                Some(ColorSpace::Separation {
                    components,
                    alternate: Box::new(alternate),
                    tint: Box::new(tint),
                    colorants,
                })
            }
            // 8.7.3.2: `[/Pattern base]` is the *uncoloured* pattern space.
            // The second element is where the operands of an `scn` that names
            // a `PaintType 2` pattern are measured; a bare `/Pattern` has no
            // second element and no colour. Without this branch the array form
            // resolved to nothing at all, so `cs` left the slot in whatever
            // space preceded it and the operands were read against that.
            // 8.6.5.6 remaps the underlying space where `cs` selected it,
            // since that is where the colour is given.
            b"Pattern" => Some(ColorSpace::Pattern {
                base: items
                    .get(1)
                    .and_then(|o| self.read_space(o, depth + 1, name, how))
                    .map(Box::new),
            }),
            // 8.6.5.2 and 8.6.5.3. These were aliased to the device spaces,
            // which read neither the white point nor the gamma and left
            // *nothing* recording that an approximation had happened — unlike
            // an ICC profile this build refuses, where `Approximated` says so
            // on the type.
            b"CalGray" => {
                let params = items.get(1).map(|o| self.doc.resolve(o));
                let dict = params.as_ref().and_then(|p| p.as_dict());
                let white = dict
                    .and_then(|d| self.numbers(d, b"WhitePoint", 3))
                    .map_or(WHITE_D50, |v| [v[0], v[1], v[2]]);
                let gamma = dict
                    .map(|d| self.doc.resolve_key(d, self.name(b"Gamma")))
                    .and_then(|g| g.as_number())
                    .unwrap_or(1.0);
                Some(ColorSpace::CalGray { white, gamma })
            }
            b"CalRGB" => {
                let params = items.get(1).map(|o| self.doc.resolve(o));
                let dict = params.as_ref().and_then(|p| p.as_dict());
                let white = dict
                    .and_then(|d| self.numbers(d, b"WhitePoint", 3))
                    .map_or(WHITE_D50, |v| [v[0], v[1], v[2]]);
                let gamma = dict
                    .and_then(|d| self.numbers(d, b"Gamma", 3))
                    .map_or([1.0; 3], |v| [v[0], v[1], v[2]]);
                let matrix = dict.and_then(|d| self.numbers(d, b"Matrix", 9)).map_or(
                    // Table 64's default is the identity, which makes the
                    // components XYZ directly.
                    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    |v| {
                        let mut m = [0.0; 9];
                        m.copy_from_slice(&v[..9]);
                        m
                    },
                );
                Some(ColorSpace::CalRgb {
                    white,
                    gamma,
                    matrix,
                })
            }
            // 8.6.5.4: L runs 0..100 and a/b roughly -128..127. Aliasing Lab
            // to RGB clamps every component into 0..1, which renders almost
            // the whole space black.
            b"Lab" => {
                let params = items.get(1).map(|o| self.doc.resolve(o));
                let range = params
                    .as_ref()
                    .and_then(|p| p.as_dict())
                    .map(|d| self.doc.resolve_key(d, self.name(b"Range")))
                    .and_then(|r| {
                        let values: Vec<f64> =
                            r.as_array()?.iter().filter_map(Object::as_number).collect();
                        (values.len() >= 4).then(|| [values[0], values[1], values[2], values[3]])
                    })
                    .unwrap_or([-100.0, 100.0, -100.0, 100.0]);
                // Table 65: `[amin amax bmin bmax]`. A pair written backwards
                // is read as the span it covers — `tinker_pdf_color`'s
                // `within`, which is what stopped it panicking — and that is a
                // leniency, so it is named (ruling 10).
                if range[0] > range[1] || range[2] > range[3] {
                    self.note_repaired_space((
                        String::from_utf8_lossy(name).into_owned(),
                        "LabRangeUnordered".to_string(),
                    ));
                }
                Some(ColorSpace::Lab { range })
            }
            _ => None,
        }
    }

    /// `count` numbers from a dictionary's array-valued key, or `None`.
    ///
    /// Shorter than `count` is `None` rather than a padded array: a
    /// `/WhitePoint` of two numbers is a dictionary that does not describe a
    /// white point, and the clause's default is a better answer than two of
    /// its three axes.
    fn numbers(&self, dict: &tinker_pdf_cos::Dict, key: &[u8], count: usize) -> Option<Vec<f64>> {
        let value = self.doc.resolve_key(dict, self.name(key));
        let values: Vec<f64> = value
            .as_array()?
            .iter()
            .filter_map(|o| self.doc.resolve(o).as_number())
            .collect();
        (values.len() >= count).then_some(values)
    }

    /// A `/DeviceN` space's colorants with its attributes (8.6.6.5, Tables 70
    /// and 71): `names` as [`colorant_names`] read them, `/Subtype`, each
    /// `/Colorants` entry that is a `/Separation` space, and `/Process`. The
    /// spaces the attributes hold are read one deeper than the space, as
    /// painted where it is, and without attributes of their own.
    fn device_n_colorants(
        &self,
        names: Vec<Colorant>,
        items: &[Object],
        depth: u32,
        name: &[u8],
        how: Reading<'_>,
    ) -> Colorants {
        let Some(attributes) = device_n_attributes(&self.doc, items) else {
            return Colorants {
                names,
                ..Colorants::default()
            };
        };
        let nested = Reading {
            colorants: false,
            ..how
        };
        let separations = attributes
            .colorants
            .iter()
            .filter_map(|(colorant, space)| {
                let space = self.read_space(space, depth + 1, name, nested)?;
                matches!(space, ColorSpace::Separation { .. }).then(|| (colorant.clone(), space))
            })
            .collect();
        let process = attributes.process.as_ref().and_then(|(space, components)| {
            let space = self.read_space(space, depth + 1, name, nested)?;
            Some(Process {
                space: Box::new(space),
                components: components.clone(),
            })
        });
        Colorants {
            names,
            n_channel: attributes.n_channel,
            separations,
            process,
        }
    }

    /// Reads a shading dictionary, wherever it was found.
    ///
    /// Split out of [`GlyphSource::shading`] so a shading *pattern* can reuse
    /// it on the dictionary it resolved itself, rather than duplicating the
    /// nine entries a shading is made of.
    ///
    /// `reference` is what the mesh types need and the gradient types do not:
    /// a type 4 to 7 shading is a **stream** (8.7.4.5.5), and its vertices are
    /// the stream's bytes. A mesh written as a direct dictionary has no
    /// vertices at all, which is reported rather than drawn as an empty area.
    fn read_shading(
        &self,
        dict: &Dict,
        reference: Option<ObjRef>,
        name: &[u8],
    ) -> Result<Option<Shading>, i64> {
        let kind = self
            .doc
            .resolve_key(dict, self.name(b"ShadingType"))
            .as_int()
            .unwrap_or(0);

        let space = self.doc.resolve_key(dict, Name::COLOR_SPACE);
        let space = self
            .parse_space(&space, 0, name)
            .unwrap_or(ColorSpace::DeviceRgb);
        let function = self.function(dict).unwrap_or(Function::Identity);

        let coords = self.doc.resolve_key(dict, self.name(b"Coords"));
        let coords: Vec<f64> = coords
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect()
            })
            .unwrap_or_default();

        let extend = self.doc.resolve_key(dict, self.name(b"Extend"));
        let extend = extend
            .as_array()
            .map(|a| {
                (
                    a.first().and_then(Object::as_bool).unwrap_or(false),
                    a.get(1).and_then(Object::as_bool).unwrap_or(false),
                )
            })
            .unwrap_or((false, false));

        match kind {
            1 => {
                let domain = self.doc.resolve_key(dict, self.name(b"Domain"));
                let domain: Vec<f64> = domain
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| self.doc.resolve(o).as_number())
                            .collect()
                    })
                    .unwrap_or_default();
                let d = |i: usize, fallback: f64| domain.get(i).copied().unwrap_or(fallback);
                Ok(Some(Shading::FunctionBased {
                    space,
                    function,
                    domain: [d(0, 0.0), d(1, 1.0), d(2, 0.0), d(3, 1.0)],
                }))
            }
            2 if coords.len() >= 4 => Ok(Some(Shading::Axial {
                space,
                function,
                coords: [coords[0], coords[1], coords[2], coords[3]],
                extend,
            })),
            3 if coords.len() >= 6 => Ok(Some(Shading::Radial {
                space,
                function,
                coords: [
                    coords[0], coords[1], coords[2], coords[3], coords[4], coords[5],
                ],
                extend,
            })),
            4..=7 => match self.read_mesh(dict, reference, kind, space, function) {
                Some(mesh) => Ok(Some(Shading::Mesh(Box::new(mesh)))),
                // Ruling 2 and ruling 10: nothing is painted and the type is
                // named, which is the degradation this path has always had.
                None => Err(kind),
            },
            _ => Ok(None),
        }
    }

    /// A mesh shading's vertex stream and the six keys that say how it is
    /// packed (8.7.4.5.5 to 8.7.4.5.8, Tables 79 to 82).
    fn read_mesh(
        &self,
        dict: &Dict,
        reference: Option<ObjRef>,
        kind: i64,
        space: ColorSpace,
        function: Function,
    ) -> Option<Mesh> {
        let data = self.decoded(reference?).ok()?;

        let integer = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.name(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = |key: &[u8]| u32::try_from(integer(key)).unwrap_or(0);

        // 8.7.4.5.5: with a `/Function` each vertex carries one parametric
        // value and the function turns it into a colour, so the component
        // count is 1 rather than the space's. Reading the space's count
        // instead cuts the stream up wrongly from the first vertex on.
        let has_function = self
            .doc
            .resolve_key(dict, self.name(b"Function"))
            .as_dict()
            .is_some()
            || self
                .doc
                .resolve_key(dict, self.name(b"Function"))
                .as_array()
                .is_some();
        let components = if has_function { 1 } else { space.components() };

        let decode = self.doc.resolve_key(dict, self.name(b"Decode"));
        let decode: Vec<(f64, f64)> = decode
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .filter(|v| v.is_finite())
                    .collect::<Vec<f64>>()
            })
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .collect();

        let params = MeshParams {
            kind,
            bits_per_coordinate: width(b"BitsPerCoordinate"),
            bits_per_component: width(b"BitsPerComponent"),
            bits_per_flag: width(b"BitsPerFlag"),
            decode,
            vertices_per_row: width(b"VerticesPerRow"),
            components,
        };
        let mesh = tinker_pdf_render::mesh::read_mesh(
            &params,
            &data,
            space,
            has_function.then_some(function),
        )?;
        (!mesh.is_empty()).then_some(mesh)
    }
}

/// A font's program, and whose it is.
#[derive(Clone)]
struct FontProgram {
    /// The program's bytes.
    bytes: Arc<Vec<u8>>,
    /// Whether a provider substituted it for a program the document does not
    /// embed. A substituted face numbers its glyphs in an order of its own,
    /// so it is read by character rather than by the document's glyph
    /// numbers (`docs/design/fallback-faces.md`).
    substituted: bool,
}

/// The font program to draw a font's glyphs with: the embedded one, or a
/// substitute the host supplied.
///
/// The document's own program always wins. A substitute is a different face
/// with different outlines, and preferring it over what the file carries
/// would change how a correctly embedded document looks.
///
/// Both read the dictionary the scope's table holds for the font, so asking
/// for a substitute reads no font dictionary a second time; the embedded one
/// is read through the operation's decoded streams, so every scope of the
/// operation that draws it is handed the one decode.
fn program_for(
    doc: &CosDocument,
    font: &ScopeFont,
    provider: Option<&dyn FontProvider>,
    op: &Operation,
) -> Option<FontProgram> {
    let dict = font.dict.as_dict()?;
    if let Some(embedded) = embedded_program(doc, dict, op) {
        return Some(FontProgram {
            bytes: embedded,
            substituted: false,
        });
    }

    let provider = provider?;
    if !fonts::is_substitutable(&font.font) {
        return None;
    }
    provider
        .substitute(&FontRequest::read(doc, dict))
        .map(|bytes| FontProgram {
            bytes,
            substituted: true,
        })
}

/// The design coordinates a multiple-master instance's `/BaseFont` carries,
/// by the convention 9.6.2.3 cites from Adobe's TN #5088, section 5: past a
/// subset tag (9.9.2), the family and style up to the first underscore, then
/// each axis's coordinate as a run of decimal digits, the runs separated by
/// labels of anything else — `MinionMM_366_465_11_`, `MyriadMM_367wd_450wt`.
///
/// `None` where the name has no underscore, no digits after it, or a run of
/// more than nine digits, which is no design coordinate of any axis the
/// supplement describes; whether there are as many as the program's axes is
/// the caller's question.
pub(crate) fn design_coordinates(base_font: &str) -> Option<Vec<f64>> {
    let bytes = base_font.as_bytes();
    let tagged =
        bytes.len() > 7 && bytes[..6].iter().all(u8::is_ascii_uppercase) && bytes[6] == b'+';
    let name = if tagged { &bytes[7..] } else { bytes };
    let underscore = name.iter().position(|b| *b == b'_')?;
    let mut out = Vec::new();
    for run in name[underscore + 1..]
        .split(|b| !b.is_ascii_digit())
        .filter(|run| !run.is_empty())
    {
        if run.len() > 9 {
            return None;
        }
        let value = run
            .iter()
            .fold(0u32, |value, digit| value * 10 + u32::from(digit - b'0'));
        out.push(f64::from(value));
    }
    (!out.is_empty()).then_some(out)
}

fn embedded_program(doc: &CosDocument, font: &Dict, op: &Operation) -> Option<Arc<Vec<u8>>> {
    // A composite font keeps its program on the descendant.
    let descendants = doc.resolve_key(font, Name::DESCENDANT_FONTS);
    let descendant = descendants
        .as_array()
        .and_then(<[Object]>::first)
        .map(|first| doc.resolve(first));
    let holder = descendant
        .as_deref()
        .and_then(Object::as_dict)
        .unwrap_or(font);

    let descriptor = doc.resolve_key(holder, Name::FONT_DESCRIPTOR);
    let descriptor = descriptor.as_dict()?;

    for key in [Name::FONT_FILE2, Name::FONT_FILE3, Name::FONT_FILE] {
        if let Some(reference) = descriptor.get_ref(key) {
            if let Ok(bytes) = op.decoded(doc, reference) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

impl FontSource for PageResources {
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
        let Some(found) = self.font(font) else {
            // A name the resource dictionary does not define. Recorded so a
            // caller can tell "this page has no text" from "this page has text
            // whose font this build could not resolve".
            self.note_missing_font(String::from_utf8_lossy(&fonts::shown(font)).into_owned());
            return Vec::new();
        };
        let font = found;
        font.decode(bytes)
            .into_iter()
            .map(|d| (d.code, d.text, d.width))
            .collect()
    }

    fn is_vertical(&self, font: &[u8]) -> bool {
        self.font(font).is_some_and(|f| f.is_vertical())
    }

    fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
        let Some(found) = self.font(font) else {
            // Unreachable in practice: an unresolved font decodes to no codes
            // at all, so nothing is ever shown to ask about. 9.7.4.3's default
            // rather than a zero displacement, so that a future caller getting
            // here still advances.
            return (0.0, 880.0, -1000.0);
        };
        // 9.7.4.3 keys /W2 by CID, and this is the same `cid_of` the advance
        // goes through — so the displacement and the position vector cannot
        // end up describing a different CID from the glyph that is drawn.
        found.vertical_metrics(found.cid_of(code))
    }

    fn font_id(&self, font: &[u8]) -> u64 {
        // A reserved name's id is its reference with the top bit set, the
        // same in every scope (`fonts::reserved_id`), whether or not the
        // object it spells reads as a font.
        self.scope_font(font)
            .map(|found| found.id)
            .or_else(|| fonts::reserved_reference(font).map(fonts::reserved_id))
            .unwrap_or(0)
    }

    /// `/BaseFont` as the dictionary writes it; `None` for a name this scope
    /// does not define and for a font that states none (a Type 3 font may
    /// omit it, 9.6.5 Table 110).
    fn font_name(&self, font: &[u8]) -> Option<Arc<str>> {
        let found = self.font(font)?;
        let name = found.base_font();
        (!name.is_empty()).then(|| Arc::from(name))
    }

    /// The glyph a render draws for `code` in the font `font` names here:
    /// [`PageResources::selection`]'s, the embedded program's `cmap` through
    /// the code's character or the code, a `/CIDToGIDMap` through the CID, a
    /// CFF charset through a name, 9.6.6.4's closing guess, or a Type 1
    /// program's charstring by its place in the program. `None` where that
    /// selects no glyph index: no embedded program, a program no parser here
    /// reads, a Type 3 font.
    fn glyph_id(&self, font: &[u8], code: u32) -> Option<u16> {
        let id = FontSource::font_id(self, font);
        self.selection(id, code).map(|selected| selected.glyph)
    }

    /// The font's ascent and descent in an em's units
    /// (`docs/design/extraction-options.md`, options 6 and 7), the first of:
    /// an embedded sfnt program's `hhea` ascender and descender over its
    /// `unitsPerEm`, where they are not both zero; the font descriptor's
    /// `/Ascent` and `/Descent` over 1000 (Table 120), where they are not
    /// both zero; its `/FontBBox`'s lower and upper `y` over 1000, where the
    /// lower is below the upper. A composite font's descriptor is its
    /// descendant's; a Type 3 font, whose glyph space is its own, has none.
    /// Read once a scope and kept.
    fn font_extent(&self, font: &[u8]) -> Option<(f64, f64)> {
        let found = self.scope_font(font)?;
        if found.type3 {
            return None;
        }
        *found.extent.get_or_init(|| self.read_extent(&found))
    }

    /// The bounds of the outline `code` selects in the font `font` names
    /// here, in an em's units: [`Outline::bounds`] — its points' box,
    /// control points included, which for a `glyf` glyph is its header's —
    /// of the outline a render draws ([`GlyphSource::outline`], at the
    /// instance a render reads). `None` for a glyph that draws nothing, a
    /// font with no program and a Type 3 font.
    fn glyph_ink(&self, font: &[u8], code: u32) -> Option<(f64, f64, f64, f64)> {
        if self.scope_font(font)?.type3 {
            return None;
        }
        let id = FontSource::font_id(self, font);
        GlyphSource::outline(self, id, code)?.bounds()
    }

    fn type3_glyph(&self, font: &[u8], code: u32) -> Option<tinker_pdf_content::Type3Glyph> {
        // 9.6.5: only a Type 3 font has glyph procedures. Every other kind
        // returns None here, which leaves the ordinary outline path untouched,
        // and is answered from the font the scope read, with nothing read or
        // copied for the glyph.
        let found = self.scope_font(font)?;
        if !found.type3 {
            return None;
        }
        let dict = found.dict.as_dict()?;

        // A Type 3 font addresses its procedures by name, and the only way to
        // get one is the /Encoding /Differences array — there is no built-in
        // encoding for a font whose glyphs the document invented. Read once
        // a scope, on the font's first glyph; a glyph is then one probe of
        // the codes and one lookup in /CharProcs, and its procedure the
        // operation's one decode of it.
        let procedures = found.procedures.get_or_init(|| self.read_type3(dict));
        let name = *procedures.names.get(&code)?;
        let reference = procedures.procs.as_dict()?.get_ref(name)?;
        let content = self.decoded(reference).ok()?;
        let matrix = procedures.matrix?;

        Some(tinker_pdf_content::Type3Glyph {
            content: Arc::unwrap_or_clone(content),
            matrix,
            // The procedure's identity, packed as a form's is, which is what
            // tells a procedure that shows its own glyph from one that shows
            // another.
            stream: crate::structure::stream_id(reference),
        })
    }

    fn form_scope(&self, name: &[u8]) -> Option<Arc<PageResources>> {
        self.form_resources(name)
    }

    /// As much as the document's budget, and at most what a retained page
    /// may hold, where the document keeps forms' drawings; nothing without
    /// a budget, or with one of zero, which keeps none.
    fn form_recording_bytes(&self) -> usize {
        self.op.cache().map_or(0, |cache| {
            usize::try_from(cache.limits().bytes)
                .unwrap_or(usize::MAX)
                .min(tinker_pdf_render::MAX_DISPLAY_LIST_BYTES)
        })
    }

    /// The drawing the document's cache keeps under `invocation` and, for a
    /// form that brings no resources of its own, this scope's resources:
    /// handed back with the fonts its interpretation found none for said
    /// again. Where none is kept, the interpretation about to run is watched
    /// for what it tells this scope, until `form_drawn` says how it went.
    fn kept_form(
        &self,
        name: &[u8],
        invocation: &tinker_pdf_content::FormInvocation,
    ) -> Option<Arc<tinker_pdf_content::FormRecording>> {
        // Every miss is answered by a `form_drawn` that takes its capture, so
        // a miss that has no key still leaves one, which keeps nothing.
        let key = self
            .op
            .cache()
            .and_then(|_| self.form_key(name, invocation));
        let found = match (self.op.cache(), &key) {
            (Some(cache), Some(key)) => cache.form(key, self.op.work.as_deref()),
            _ => Err(0),
        };
        match found {
            Ok(kept) => {
                for name in &kept.missing_fonts {
                    self.note_missing_font(name.clone());
                }
                Some(Arc::clone(&kept.recording))
            }
            Err(asked) => {
                if let Ok(mut recording) = self.recording.lock() {
                    recording.push(FormCapture {
                        key,
                        asked,
                        missing_fonts: Vec::new(),
                        repaired: false,
                    });
                }
                None
            }
        }
    }

    /// Counts how the form was drawn, and keeps the drawing recorded after
    /// `kept_form` found none, where it is one to keep and its
    /// interpretation reported no repaired colour space.
    fn form_drawn(&self, _name: &[u8], drawn: tinker_pdf_content::FormDrawn) {
        use tinker_pdf_content::FormDrawn;
        let recorded = match drawn {
            FormDrawn::Replayed => {
                self.count(Counter::FormsReplayed);
                return;
            }
            FormDrawn::Interpreted => {
                self.count(Counter::FormsInterpreted);
                return;
            }
            FormDrawn::Recorded { recording, .. } => recording,
        };
        self.count(Counter::FormsInterpreted);
        let capture = self
            .recording
            .lock()
            .ok()
            .and_then(|mut recording| recording.pop());
        let (Some(cache), Some(capture), Some(recording)) = (self.op.cache(), capture, recorded)
        else {
            return;
        };
        let Some(key) = capture.key else {
            return;
        };
        if capture.repaired {
            return;
        }
        let words = capture
            .missing_fonts
            .iter()
            .map(String::len)
            .fold(0usize, usize::saturating_add);
        let bytes = recording
            .kept_bytes()
            .saturating_add(key.invocation.written_len())
            .saturating_add(key.scope.as_ref().map_or(0, Written::len))
            .saturating_add(words);
        cache.offer_form(
            key,
            Arc::new(KeptForm {
                recording: Arc::new(recording),
                missing_fonts: capture.missing_fonts,
            }),
            u64::try_from(bytes).unwrap_or(u64::MAX),
            capture.asked,
            self.op.work.as_deref(),
        );
    }

    fn form(&self, name: &[u8]) -> Option<tinker_pdf_content::Form> {
        let (dict, reference) = self.xobject(name)?;
        let subtype = self
            .doc
            .resolve_key(&dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Form" {
            return None;
        }
        self.form_from(&dict, reference, name, Contents::OwnScope)
    }

    fn resolve_color(
        &self,
        space: &[u8],
        components: &[f64],
        intent: tinker_pdf_content::RenderingIntent,
    ) -> Option<Rgb> {
        let (r, g, b) = self.painted_color(space, components, intent, ColorSpace::to_rgb_in)?;
        Some(Rgb { r, g, b })
    }

    fn resolve_color_wide(
        &self,
        space: &[u8],
        components: &[f64],
        intent: tinker_pdf_content::RenderingIntent,
    ) -> Option<(Rgb, Rgb16)> {
        // One evaluation: the colour crate's 16-bit answer tops out at the
        // bytes `to_rgb_in` gives, so the bytes are its top bytes
        // (`docs/design/sixteen-bit.md`, "Resolved colour").
        let wide =
            Rgb16::from(self.painted_color(space, components, intent, ColorSpace::to_rgb16_in)?);
        Some((wide.top(), wide))
    }

    fn color_components(&self, space: &[u8]) -> Option<usize> {
        // A default has its device space's count, so the space as selected
        // answers.
        self.named_space(space, Answer::Selected, ColorSpace::components)
    }

    fn resolve_ink(&self, space: &[u8], components: &[f64]) -> Option<[u8; 4]> {
        // 8.6.4.4: DeviceCMYK's own components, clamped as `to_rgb` clamps
        // them. Every other space — an ICC CMYK profile included, whose
        // components are a profile's to interpret — stays light.
        //
        // Asked of the space as selected, so a DeviceCMYK colour keeps the
        // document's own ink under a `/DefaultCMYK` while its light is the
        // default's: H-135's default until the owner decides, and choosing
        // conversion instead is this one check.
        if !self.named_space(space, Answer::Selected, |space| {
            *space == ColorSpace::DeviceCmyk
        })? {
            return None;
        }
        let byte = |i: usize| {
            let v = components.get(i).copied().unwrap_or(0.0);
            if v.is_finite() {
                (v.clamp(0.0, 1.0) * 255.0).round() as u8
            } else {
                0
            }
        };
        Some([byte(0), byte(1), byte(2), byte(3)])
    }

    fn color_family(&self, space: &[u8]) -> tinker_pdf_content::ColorFamily {
        // The space as painted: a device space a default replaces is its
        // substitute's family (8.6.5.6), which a default that is a CIE-based
        // or ICC space makes `Other`, so black generation leaves it alone.
        let device = match space {
            b"DeviceGray" => Some(ColorSpace::DeviceGray),
            b"DeviceRGB" => Some(ColorSpace::DeviceRgb),
            b"DeviceCMYK" => Some(ColorSpace::DeviceCmyk),
            _ => None,
        };
        match device {
            Some(device) => match self.defaults().for_device(&device) {
                Some(default) => tinker_pdf_render::color_family(default),
                None => tinker_pdf_render::color_family(&device),
            },
            None => self
                .named_space(space, Answer::Painted, tinker_pdf_render::color_family)
                .unwrap_or(tinker_pdf_content::ColorFamily::Other),
        }
    }

    fn colorants(
        &self,
        space: &[u8],
        components: &[f64],
        intent: tinker_pdf_content::RenderingIntent,
    ) -> Option<Arc<ColorantTints>> {
        // The colour's light as painted, in the intent in force: what a
        // space reverting as a whole through an alternate other than
        // DeviceCMYK paints, turned into ink.
        let light = self.resolve_color(space, components, intent)?;
        // The space as selected, as `resolve_ink` asks: a DeviceCMYK colour
        // under a `/DefaultCMYK` keeps its own components on the process
        // colorants, H-135's default, which the process plates inherit.
        self.named_space(space, Answer::Selected, |selected| {
            colorant_tints(selected, components, (light.r, light.g, light.b))
        })?
        .map(Arc::new)
    }

    fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
        // 8.6.8's initial value of the space as selected, carried into the
        // default as every other value is: `/DeviceCMYK cs` and `0 0 0 1 k`
        // paint one colour under one default, where an `ICCBased` default's
        // own initial value, all zeros, would be no ink at all.
        self.named_space(space, Answer::Selected, ColorSpace::initial)
    }

    fn device_defaults(&self) -> DeviceDefaults {
        let defaults = self.defaults();
        DeviceDefaults {
            gray: defaults.gray.is_some(),
            rgb: defaults.rgb.is_some(),
            cmyk: defaults.cmyk.is_some(),
        }
    }

    fn ext_g_state(&self, name: &[u8]) -> Option<Arc<tinker_pdf_content::StateParameters>> {
        // Read once a name in this scope ([`Caches::states`]), and every `gs`
        // of the name handed the same value, as the trait asks.
        if let Ok(cache) = self.caches.states.lock() {
            if let Some(hit) = cache.get(name) {
                self.note_repaired_spaces(&hit.repaired_spaces);
                return hit.state.clone();
            }
        }
        let before = self.repaired_spaces().len();
        let state = self.read_ext_g_state(name);
        let repaired_spaces = self.repaired_spaces().into_iter().skip(before).collect();
        if let Ok(mut cache) = self.caches.states.lock() {
            if cache.len() < MAX_CACHED_ENTRIES {
                cache.insert(
                    name.to_vec(),
                    CachedState {
                        state: state.clone(),
                        repaired_spaces,
                    },
                );
            }
        }
        state
    }

    fn optional_content(&self, name: &[u8]) -> Option<Layer> {
        // 8.11.3.2: `/OC /MC0 BDC` names an entry in the /Properties
        // sub-dictionary of the current resource dictionary. The entry is an
        // optional content group or a membership dictionary; anything else is
        // an ordinary property list and hides nothing.
        // The table and the entry are read where they lie, as
        // `with_property_list` reads them: every `/OC` `BDC` asks.
        let resources = self.resources.as_deref()?;
        let table = resources.get(Name::PROPERTIES)?;
        read_resolved(&self.doc, table, |table| {
            let entry = table.as_dict()?.get(self.name(name))?;
            let label = String::from_utf8_lossy(name).into_owned();
            Some(
                self.op
                    .optional(&self.doc)
                    .layer_of(&self.doc, entry, &label),
            )
        })
    }

    fn xobject_optional_content(&self, name: &[u8]) -> Option<Layer> {
        // 8.11.4.4: an /OC entry on a form or image XObject hides the whole
        // XObject, and is read from the XObject's own dictionary rather than
        // from /Properties.
        let (dict, _) = self.xobject(name)?;
        let entry = dict.get(self.name(b"OC"))?.clone();
        let label = String::from_utf8_lossy(name).into_owned();
        Some(
            self.op
                .optional(&self.doc)
                .layer_of(&self.doc, &entry, &label),
        )
    }

    fn marked_content_properties(&self, name: &[u8]) -> Option<MarkedProps> {
        // 14.6.2: `/P /MC0 BDC` names an entry in the same /Properties
        // sub-dictionary `optional_content` reads. This is the COS half of
        // the seam — the interpreter never sees a dictionary — and it is the
        // reason the inline and named forms arrive at a device
        // indistinguishable from each other.
        let props = self.with_property_list(name, |dict| self.marked_props(name, dict))?;
        // An `/OC` group, a `/Type /Pagination` artifact list, a producer's
        // private dictionary: every one of them reaches here and says nothing
        // 14.6.2 or 14.9 defines. `None` rather than an empty struct, so the
        // interpreter's own filter and this one cannot disagree.
        (!props.is_empty()).then_some(props)
    }
}

/// The most elements a `/D` dash array may hold: `d` reads its operands off a
/// stack of 512 tokens, which holds the brackets, the phase and 509 numbers,
/// and a longer array never reaches it. A dictionary is held to the same
/// bound, so it cannot put an array into every state copy that no content
/// stream could.
const MAX_DASH_ELEMENTS: usize = 509;

impl PageResources {
    /// A graphics state parameter dictionary (8.4.5, Table 58, and ISO
    /// 32000-2's `/UseBlackPtComp`), read once: each key's value where it
    /// lies, and a value of a type the table does not give read as absent.
    /// `name` is the dictionary's resource name, for a soft mask's warnings.
    fn state_parameters(&self, dict: &Dict, name: &[u8]) -> tinker_pdf_content::StateParameters {
        use tinker_pdf_content::{BlackPointCompensation, RenderingIntent};
        let mut set = tinker_pdf_content::StateParameters::default();
        let mut fill_overprint = None;
        // Table 58's second spellings win when a dictionary writes both.
        let (mut generation, mut generation2) = (None, None);
        let (mut removal, mut removal2) = (None, None);
        let (mut transfer, mut transfer2) = (None, None);
        let name_of = |value: &Object| value.as_name().and_then(|n| self.doc.name_bytes(n));
        for (key, raw) in dict.iter() {
            let Some(key) = self.doc.name_bytes(*key) else {
                continue;
            };
            // A function is handed to its reader as written: a sampled or a
            // calculator function is a stream, reached through its reference.
            read_resolved(&self.doc, raw, |value| match key.as_ref() {
                b"LW" => set.line_width = value.as_number(),
                b"LC" => set.line_cap = value.as_number(),
                b"LJ" => set.line_join = value.as_number(),
                b"ML" => set.miter_limit = value.as_number(),
                b"D" => set.dash = self.dash(value),
                b"Font" => set.font = self.state_text_font(value),
                b"RI" => {
                    set.rendering_intent =
                        name_of(value).map(|intent| RenderingIntent::from_name(&intent));
                }
                b"UseBlackPtComp" => {
                    set.black_point_compensation = match name_of(value).as_deref() {
                        Some(b"ON") => Some(BlackPointCompensation::On),
                        Some(b"OFF") => Some(BlackPointCompensation::Off),
                        Some(b"Default") => Some(BlackPointCompensation::Default),
                        _ => None,
                    };
                }
                b"OP" => set.stroke_overprint = value.as_bool(),
                b"op" => fill_overprint = value.as_bool(),
                b"OPM" => set.overprint_mode = value.as_int(),
                b"FL" => set.flatness = value.as_number(),
                b"SM" => set.smoothness = value.as_number(),
                b"SA" => set.stroke_adjustment = value.as_bool(),
                b"AIS" => set.alpha_is_shape = value.as_bool(),
                b"TK" => set.text_knockout = value.as_bool(),
                // 8.4.5 Table 58: `ca` is the non-stroking alpha, `CA` the
                // stroking.
                b"CA" => set.stroke_alpha = value.as_number(),
                b"ca" => set.fill_alpha = value.as_number(),
                b"BM" => set.blend = self.blend_mode(value),
                b"SMask" => set.soft_mask = self.state_soft_mask(value, name),
                b"BG" => generation = self.state_curve(raw, value, false),
                b"BG2" => generation2 = self.state_curve(raw, value, true),
                b"UCR" => removal = self.state_curve(raw, value, false),
                b"UCR2" => removal2 = self.state_curve(raw, value, true),
                b"TR" => transfer = self.state_transfer(raw, value, false),
                b"TR2" => transfer2 = self.state_transfer(raw, value, true),
                b"HT" => set.halftone = self.state_halftone(raw, value),
                b"HTO" => set.halftone_origin = self.halftone_origin(value),
                _ => {}
            });
        }
        set.black_generation = generation2.or(generation);
        set.undercolor_removal = removal2.or(removal);
        set.transfer = transfer2.or(transfer);
        // Table 58: an absent `/op` takes `/OP`'s value, so a dictionary that
        // writes only `/OP true` is one that writes both.
        set.fill_overprint = fill_overprint.or(set.stroke_overprint);
        set
    }

    /// `/Font` (Table 58), `[font size]`: the font an indirect reference as
    /// written and the size a finite number, as `Tf`'s operands. The text font
    /// is the name `fonts::state_font` gives the object in this scope — the
    /// scope's own name for it where `/Font` binds one, so a twin's `Tf`
    /// selects the same name, and otherwise the reserved name that spells it.
    fn state_text_font(&self, value: &Object) -> Option<(Vec<u8>, f64)> {
        let [font, size] = value.as_array()? else {
            return None;
        };
        let reference = font.as_objref()?;
        let size = read_resolved(&self.doc, size, Object::as_number).filter(|v| v.is_finite())?;
        let name = match self.resources.as_deref() {
            Some(resources) => fonts::state_font(&*self.doc, resources, reference),
            None => fonts::reserved_name(reference),
        };
        Some((name, size))
    }

    /// `/D`, `[dashArray dashPhase]`: the numbers as written, for the check
    /// `d` makes; anything `d` could not have been handed is absent.
    fn dash(&self, value: &Object) -> Option<(Vec<f64>, f64)> {
        let [array, phase] = value.as_array()? else {
            return None;
        };
        let phase = read_resolved(&self.doc, phase, Object::as_number)?;
        let dashes = read_resolved(&self.doc, array, |array| {
            let array = array.as_array()?;
            if array.len() > MAX_DASH_ELEMENTS {
                return None;
            }
            array
                .iter()
                .map(|item| read_resolved(&self.doc, item, Object::as_number))
                .collect::<Option<Vec<f64>>>()
        })?;
        Some((dashes, phase))
    }

    /// A function a graphics state names, read as written, through the
    /// scope's cache when it is an object of its own
    /// ([`PageResources::kept_function`]).
    fn state_function(&self, raw: &Object) -> Option<Arc<Function>> {
        self.kept_function(raw, 0)
    }

    /// `/BG` or `/UCR` (10.3.4, 10.3.5): a function; the second spellings
    /// also `/Default`, which is `Some(None)`. Anything else is absent.
    fn state_curve(
        &self,
        raw: &Object,
        value: &Object,
        default: bool,
    ) -> Option<Option<Arc<Function>>> {
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return (default && name.as_ref() == b"Default").then_some(None);
        }
        self.state_function(raw).map(Some)
    }

    /// `/TR` or `/TR2` (10.5): a function, an array of four, or `/Identity`;
    /// `/TR2` also `/Default`, which is `Some(None)`. Anything else is absent.
    fn state_transfer(
        &self,
        raw: &Object,
        value: &Object,
        default: bool,
    ) -> Option<Option<tinker_pdf_content::Transfer>> {
        use tinker_pdf_content::Transfer;
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return match name.as_ref() {
                b"Identity" => Some(Some(Transfer::Identity)),
                b"Default" if default => Some(None),
                _ => None,
            };
        }
        if let Some(items) = value.as_array() {
            let [a, b, c, d] = items else {
                return None;
            };
            let four = [
                self.state_function(a)?,
                self.state_function(b)?,
                self.state_function(c)?,
                self.state_function(d)?,
            ];
            return Some(Some(Transfer::Four(four)));
        }
        self.state_function(raw).map(|f| Some(Transfer::One(f)))
    }

    /// `/HT` (10.6.5): a halftone dictionary or stream, or `/Default`, which
    /// is `Some(None)`. A halftone of the wrong shape is absent, never a
    /// refusal of the page (ruling 2).
    fn state_halftone(
        &self,
        raw: &Object,
        value: &Object,
    ) -> Option<Option<Arc<tinker_pdf_content::Halftone>>> {
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return (name.as_ref() == b"Default").then_some(None);
        }
        self.halftone(raw, false)
            .map(|halftone| Some(Arc::new(halftone)))
    }

    /// One halftone dictionary or stream, as written (10.6.5, Tables 130 to
    /// 134). A threshold array is kept by its stream's reference; a type 5's
    /// entries are halftones of the other types, and it holds `/Default`.
    fn halftone(&self, raw: &Object, in_type5: bool) -> Option<tinker_pdf_content::Halftone> {
        use tinker_pdf_content::Halftone;
        let reference = raw
            .as_objref()
            .map(|r| (u64::from(r.num) << 16) | u64::from(r.gen));
        read_resolved(&self.doc, raw, |value| {
            let dict = value.as_dict()?;
            let int = |key: &[u8]| read_key(&self.doc, dict, key, Object::as_int);
            let number = |key: &[u8]| {
                read_key(&self.doc, dict, key, Object::as_number).filter(|v| v.is_finite())
            };
            // A threshold array is a stream's data, reached by reference.
            let thresholds = || value.as_stream().and(reference);
            let transfer = match dict.get(self.name(b"TransferFunction")) {
                None => None,
                Some(raw) => read_resolved(&self.doc, raw, |value| {
                    self.state_transfer(raw, value, false)
                })?,
            };
            match int(b"HalftoneType")? {
                1 => {
                    let spot = dict.get(self.name(b"SpotFunction"))?;
                    let spot =
                        read_resolved(&self.doc, spot, |value| self.spot_function(spot, value))?;
                    Some(Halftone::Screen {
                        frequency: number(b"Frequency")?,
                        angle: number(b"Angle")?,
                        spot,
                        accurate: read_key(&self.doc, dict, b"AccurateScreens", Object::as_bool)
                            .unwrap_or(false),
                        transfer,
                    })
                }
                5 if !in_type5 => {
                    let mut entries = Vec::new();
                    for (key, raw) in dict.iter() {
                        let key = self.doc.name_bytes(*key)?;
                        if matches!(key.as_ref(), b"Type" | b"HalftoneType" | b"HalftoneName") {
                            continue;
                        }
                        entries.push((key.to_vec(), self.halftone(raw, true)?));
                    }
                    entries
                        .iter()
                        .any(|(colorant, _)| colorant == b"Default")
                        .then_some(Halftone::Colorants(entries))
                }
                6 => Some(Halftone::Threshold {
                    width: int(b"Width")?,
                    height: int(b"Height")?,
                    thresholds: thresholds()?,
                    transfer,
                }),
                10 => Some(Halftone::Squares {
                    xsquare: int(b"Xsquare")?,
                    ysquare: int(b"Ysquare")?,
                    thresholds: thresholds()?,
                    transfer,
                }),
                16 => {
                    let second = match (int(b"Width2"), int(b"Height2")) {
                        (Some(width), Some(height)) => Some((width, height)),
                        (None, None) => None,
                        _ => return None,
                    };
                    Some(Halftone::Threshold16 {
                        width: int(b"Width")?,
                        height: int(b"Height")?,
                        second,
                        thresholds: thresholds()?,
                        transfer,
                    })
                }
                _ => None,
            }
        })
    }

    /// A type 1 halftone's `/SpotFunction` (10.6.5.2): a function, read as
    /// written, or a predefined name, or an array of names in order of
    /// preference, kept as written.
    fn spot_function(
        &self,
        raw: &Object,
        value: &Object,
    ) -> Option<tinker_pdf_content::SpotFunction> {
        use tinker_pdf_content::SpotFunction;
        let name = |object: &Object| {
            object
                .as_name()
                .and_then(|n| self.doc.name_bytes(n))
                .map(|n| n.to_vec())
        };
        if let Some(name) = name(value) {
            return Some(SpotFunction::Names(vec![name]));
        }
        if let Some(items) = value.as_array() {
            let names: Option<Vec<Vec<u8>>> = items
                .iter()
                .map(|item| read_resolved(&self.doc, item, name))
                .collect();
            return names
                .filter(|names| !names.is_empty())
                .map(SpotFunction::Names);
        }
        self.state_function(raw).map(SpotFunction::Function)
    }

    /// `/HTO` (ISO 32000-2): an origin of two numbers.
    fn halftone_origin(&self, value: &Object) -> Option<[f64; 2]> {
        let [x, y] = value.as_array()? else {
            return None;
        };
        Some([
            read_resolved(&self.doc, x, Object::as_number)?,
            read_resolved(&self.doc, y, Object::as_number)?,
        ])
    }

    /// `/BM` (11.3.5): a name, or an array of names offering fallbacks.
    fn blend_mode(&self, value: &Object) -> Option<tinker_pdf_content::BlendMode> {
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return Some(tinker_pdf_content::BlendMode::from_name(&name));
        }
        let names: Vec<Vec<u8>> = value
            .as_array()?
            .iter()
            .filter_map(|item| self.doc.resolve(item).as_name())
            .filter_map(|n| self.doc.name_bytes(n))
            .map(|n| n.to_vec())
            .collect();
        (!names.is_empty())
            .then(|| tinker_pdf_content::BlendMode::from_names(names.iter().map(Vec::as_slice)))
    }

    /// The graphics state `name` selects, read from where it lies: the table
    /// and the entry as `with_property_list` reads them, and the entry's keys
    /// once each, with no copy of a table written inline. Counted as a
    /// graphics state read when the entry is a dictionary.
    fn read_ext_g_state(&self, name: &[u8]) -> Option<Arc<tinker_pdf_content::StateParameters>> {
        let resources = self.resources.as_deref()?;
        let table = resources.get(Name::EXT_G_STATE)?;
        read_resolved(&self.doc, table, |table| {
            let entry = table.as_dict()?.get(self.name(name))?;
            read_resolved(&self.doc, entry, |entry| {
                let dict = entry.as_dict()?;
                self.count(Counter::GraphicsStatesRead);
                Some(Arc::new(self.state_parameters(dict, name)))
            })
        })
    }

    /// `/SMask` (11.6.5.1), as the dictionary `gs` names `name` writes it.
    fn state_soft_mask(&self, value: &Object, name: &[u8]) -> Option<tinker_pdf_content::SoftMask> {
        // 11.6.5.1: `/SMask` is either the name `/None` or a mask dictionary.
        // The key being absent is *not* the same as `/None` — the first
        // leaves whatever mask is in force alone, the second removes it —
        // which is why the whole method returns an `Option` of an enum rather
        // than an `Option` of a mask.
        if let Some(name) = value.as_name().and_then(|n| self.doc.name_bytes(n)) {
            return (name.as_ref() == b"None").then_some(tinker_pdf_content::SoftMask::None);
        }
        let mask = value.as_dict()?;

        // `/G` is required, and is a form XObject with a transparency group.
        let reference = mask.get_ref(self.name(b"G"))?;
        let group_dict = self.doc.get(reference).ok()?.as_dict()?.clone();
        let form = self.form_from(&group_dict, reference, name, Contents::ThisScope)?;

        // 11.6.5.2: `/S` selects what the rendered group is read as. A name
        // this build does not know is `/Alpha`'s opposite rather than an
        // error, because `/Luminosity` is overwhelmingly the one in the wild
        // and a mask that fails to parse is a mask that hides nothing.
        let luminosity = self
            .doc
            .resolve_key(mask, self.name(b"S"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))
            .is_none_or(|s| s.as_ref() != b"Alpha");

        // `/BC` is in the group's own colour space, which is `/Group /CS` on
        // `/G`. Without one the components are read by count, which is what
        // every other unnamed space in this engine does.
        let backdrop = self
            .doc
            .resolve_key(mask, self.name(b"BC"))
            .as_array()
            .map(|values| {
                let components: Vec<f64> = values
                    .iter()
                    .filter_map(|v| self.doc.resolve(v).as_number())
                    .collect();
                let space = self
                    .doc
                    .resolve_key(&group_dict, self.name(b"Group"))
                    .as_dict()
                    .and_then(|g| g.get(self.name(b"CS")).cloned())
                    .and_then(|cs| self.parse_space(&cs, 0, name));
                let (r, g, b) = match space {
                    Some(space) => space.to_rgb(&components),
                    None => by_component_count(&components),
                };
                Rgb { r, g, b }
            });

        // 11.6.5.2: `/TR` is a function, or the name `/Identity`. A name is
        // the identity whatever it says, because the only alternative a
        // reader has for an unknown one is to mask nothing. The entry is read
        // as written, by the reader every graphics state function takes, so a
        // sampled or a calculator function — a stream — is reached through its
        // reference; sampled to 256 entries, since the mask applies it per
        // pixel.
        let transfer = mask
            .get(self.name(b"TR"))
            .and_then(|raw| self.state_function(raw))
            .map(|function| sample_transfer(&function));

        Some(tinker_pdf_content::SoftMask::Group(Box::new(
            tinker_pdf_content::MaskGroup {
                form,
                luminosity,
                backdrop,
                transfer,
            },
        )))
    }
}

impl PageResources {
    /// What the property list `dict`, named `name`, says to a device: the
    /// body of [`FontSource::marked_content_properties`], run on the list
    /// where it lies.
    fn marked_props(&self, name: &[u8], dict: &Dict) -> MarkedProps {
        // 14.7.4.2: a non-negative integer. Resolved because 7.3.10 lets any
        // value in a *file* dictionary be indirect — which is exactly the
        // difference between this form and the inline one, where it cannot
        // be. Every value here is read where it lies (`read_key`), as the
        // list is: a copy of one is its size at every `BDC`.
        let mcid =
            read_key(&self.doc, dict, b"MCID", Object::as_int).and_then(|n| u32::try_from(n).ok());

        let text = |key: &[u8]| {
            read_key(&self.doc, dict, key, |value| {
                value.as_string().map(|s| decode_text_string(&s.bytes))
            })
        };

        MarkedProps {
            mcid,
            actual_text: text(b"ActualText"),
            alt: text(b"Alt"),
            lang: text(b"Lang"),
            expansion: text(b"E"),
            // 14.7.4.2's stream half of the identifier is not in the property
            // list and cannot be: it is which stream the `BDC` was written
            // in, which only the interpreter knows. It stamps this on the way
            // past.
            stream: 0,
            // ISO 32000-2 14.13.5, Table 409a as the approved errata add it:
            // a named list with an `/MCAF` array associates files with an
            // `/AF` sequence. The name is handed on, not the files, which
            // `Page::marked_content_associated_files` reads in this scope.
            // Asked whether it is an array where it lies, since a direct
            // `/MCAF` is the list's own and as long as the file makes it.
            associated_files: read_key(&self.doc, dict, b"MCAF", |mcaf| mcaf.as_array().is_some())
                .then(|| name.to_vec()),
        }
    }
}

impl GlyphSource for PageResources {
    fn form_scope(&self, name: &[u8]) -> Option<Arc<PageResources>> {
        self.form_resources(name)
    }

    /// The font `font_id` selects here, from the scope's font table: the
    /// object its `/Font` entry refers to, or the one a reserved name spells,
    /// is one font in every scope; a font written into the dictionary is one
    /// only within this scope; an id that selects no font has no identity,
    /// and its glyphs' masks are not kept.
    fn font_identity(&self, font_id: u64) -> Option<FontIdentity> {
        let object = |reference: ObjRef| {
            FontIdentity::Object((u64::from(reference.num) << 16) | u64::from(reference.gen))
        };
        if let Some(reference) = fonts::reserved_id_reference(font_id) {
            return Some(object(reference));
        }
        let found = self.font_by_id(font_id)?;
        Some(match found.object {
            Some(reference) => object(reference),
            None => FontIdentity::Scoped,
        })
    }

    /// A document with a budget keeps its glyph masks with everything else
    /// it keeps, under P-06's key with the font as the document names it
    /// ([`cache::MaskKey`]); without one the render keeps its own.
    fn kept_mask(&self, key: &tinker_pdf_render::MaskKey) -> Option<Result<Arc<Mask>, u64>> {
        let cache = self.op.cache()?;
        let kept = cache::MaskKey::of(key, self.mask_font(key.font_id)?);
        Some(cache.mask(&kept, self.op.work.as_deref()))
    }

    /// Charged the mask's pixels and its key's resources beside the cache's
    /// 64 bytes, as P-06 charges a mask its pixels and 64.
    fn keep_mask(&self, key: &tinker_pdf_render::MaskKey, mask: &Arc<Mask>, asked: u64) {
        let (Some(cache), Some(font)) = (self.op.cache(), self.mask_font(key.font_id)) else {
            return;
        };
        let kept = cache::MaskKey::of(key, font);
        let bytes = (u64::from(mask.width) * u64::from(mask.height)).saturating_add(kept.held());
        cache.offer_mask(
            kept,
            Arc::clone(mask),
            bytes,
            asked,
            self.op.work.as_deref(),
        );
    }

    fn outline(&self, font_id: u64, code: u32) -> Option<Outline> {
        let made = Cell::new(false);
        let (outline, unresolved) = self.caches.outlines.read((font_id, code), || {
            made.set(true);
            self.count(Counter::GlyphOutlineMisses);
            if let Ok(mut marker) = self.unresolved.lock() {
                *marker = None;
            }
            let outline = self.extract_outline(font_id, code).map(Arc::new);
            let unresolved = self
                .unresolved
                .lock()
                .ok()
                .and_then(|mut marker| marker.take());
            let found = (outline, unresolved);
            if self.caches.outlines.len() <= MAX_CACHED_OUTLINES {
                Filled::Keep(found)
            } else {
                Filled::Once(found)
            }
        });
        if !made.get() {
            self.count(Counter::GlyphOutlineHits);
            // What the extraction reported, reported again: this render met
            // the glyph too, and a render of its own would have.
            if let Some(name) = unresolved {
                self.note_missing_font(name);
            }
        }
        outline.map(|o| (*o).clone())
    }

    fn shading(&self, name: &[u8]) -> Result<Option<Shading>, i64> {
        let Some(resources) = self.resources.as_deref() else {
            return Ok(None);
        };
        let table = self.doc.resolve_key(resources, Name::SHADING);
        let Some(table) = table.as_dict() else {
            return Ok(None);
        };
        let key = self.name(name);
        let reference = table.get_ref(key);
        let entry = self.doc.resolve_key(table, key);
        let Some(dict) = entry.as_dict() else {
            return Ok(None);
        };

        self.read_shading(dict, reference, name)
    }

    fn pattern(&self, name: &[u8]) -> Option<PatternPaint> {
        let (dict, _) = self.pattern_entry(name)?;

        // 8.7.3.3: type 1 is a tiling pattern, type 2 a shading pattern.
        let kind = self
            .doc
            .resolve_key(&dict, self.name(b"PatternType"))
            .as_int()
            .unwrap_or(0);
        // 8.7.3.1: the pattern matrix maps pattern space to the *default*
        // space of the parent content stream, so the CTM in force at fill time
        // is not part of it. Shared by both kinds, because two readers of one
        // key is how the anchoring comes to be right on one route and wrong on
        // the other.
        let matrix = self.pattern_matrix(&dict);

        match kind {
            1 => Some(self.tiling_pattern(&dict, matrix)),
            2 => {
                let key = Name::SHADING;
                let reference = dict.get_ref(key);
                let shading = self.doc.resolve_key(&dict, key);
                let shading = shading.as_dict()?;
                // A mesh that cannot be read at all is as unpaintable inside a
                // pattern as anywhere else, and reports as an unpainted
                // pattern rather than as a missing one.
                let Ok(Some(shading)) = self.read_shading(shading, reference, name) else {
                    return Some(PatternPaint::Unsupported);
                };
                Some(PatternPaint::Shading(Box::new(shading), matrix))
            }
            _ => Some(PatternPaint::Unsupported),
        }
    }

    fn tile(
        &self,
        name: &[u8],
        request: &tinker_pdf_render::TileRequest,
    ) -> Option<tinker_pdf_render::Tile> {
        let (dict, reference) = self.pattern_entry(name)?;
        // A tiling pattern is a *stream* (8.7.3.2, Table 75): its cell is a
        // content stream. A direct dictionary carrying the keys but no stream
        // has no cell to draw, and declining here is what turns it back into
        // the reported gap it was.
        let reference = reference?;
        let Some(cache) = self.op.cache() else {
            let drawn = self.drawn_cell(&dict, reference, request, None)?;
            drawn.say_again(self);
            return Some(drawn.tile);
        };
        // A document with a budget keeps the cell (P-23.2): under its pattern
        // and everything of the request and the scope its pixels are a
        // function of (`cache::CellKey`), so a hit is the cell a miss draws,
        // and what its drawing told its scope is kept with it and said again
        // by every fill that meets it.
        let key = self.cell_key(&dict, reference, request);
        let key_bytes = key.held();
        let (kept, _) = cache.cell(key, self.op.work.as_deref(), || {
            // Drawn through resources of its own over this scope's caches,
            // so what it reports is the cell's alone, whatever this render
            // had reported before it.
            let serving = self.serving(self.op.clone());
            let Some(drawn) = self.drawn_cell(&dict, reference, request, Some(&serving)) else {
                return Fill::Keep(None, 0);
            };
            // A cancelled render's cell holds what it drew before it stopped,
            // which is not the cell its key names: it serves this fill alone.
            let cancelled = request.cancel.is_cancelled()
                || drawn
                    .tile
                    .warnings
                    .contains(&tinker_pdf_render::RenderWarning::Cancelled);
            let cell = Arc::new(drawn);
            if cancelled {
                return Fill::Once(Some(cell));
            }
            let bytes = cell.bytes().saturating_add(key_bytes);
            Fill::Keep(Some(cell), bytes)
        });
        let cell = kept?;
        cell.say_again(self);
        Some(cell.tile.clone())
    }

    fn inline_image(
        &self,
        dict: &[u8],
        data: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        let text = expand_inline_abbreviations(dict);

        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed = tinker_pdf_cos::parse_object_at(&text, 0, self.doc.names_table(), &mut sink);
        let Some(dict) = parsed.object.as_dict() else {
            return Ok(None);
        };

        // Table 91: an inline image names its intent under the full key, and
        // where it names one its colours take it rather than the intent in
        // force at `EI`. Never cached, so every inline image is a decoder run
        // in the conversion it asks for.
        let conversion = self.own_intent(dict).unwrap_or(intent).conversion();
        self.count(Counter::ImagesDecoded);
        self.decode_inline(dict, data, request, conversion, None)
            .map(|(image, _)| Some(image))
    }

    fn inline_image_on_plates(
        &self,
        dict: &[u8],
        data: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
        plan: &Arc<PlatePlan>,
    ) -> Result<Option<DecodedImage>, String> {
        let text = expand_inline_abbreviations(dict);
        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed = tinker_pdf_cos::parse_object_at(&text, 0, self.doc.names_table(), &mut sink);
        let Some(dict) = parsed.object.as_dict() else {
            return Ok(None);
        };
        let conversion = self.own_intent(dict).unwrap_or(intent).conversion();
        self.count(Counter::ImagesDecoded);
        self.decode_inline(dict, data, request, conversion, Some(plan))
            .map(|(image, _)| Some(image))
    }

    fn image_on_plates(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
        plan: &Arc<PlatePlan>,
    ) -> Result<Option<DecodedImage>, String> {
        // A decode for plates is the draw's own: the scope's and the
        // document's caches keep light, and a decode's tints are those of one
        // render's plan. It decodes at the size the draw asks, as a render
        // with no budget does.
        self.count(Counter::ImagesDecoded);
        let Decoded { image, .. } = self.decode_image(name, request, intent, None, Some(plan));
        image.map(Some)
    }

    fn image(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Result<Option<DecodedImage>, String> {
        // A document with a budget keeps an image XObject's decodes across its
        // renders; an inline image has no object to key it by, and an
        // XObject that is no image is answered as it always was, below.
        if let Some(cache) = self.op.cache() {
            if let Some(kept) = self.kept_image(cache, name, request, intent) {
                return kept;
            }
        }

        // Whether this read decoded the image itself, rather than meeting a
        // decode another render made or is making.
        let made = Cell::new(false);
        // The name's first decode is made for this draw in this draw's
        // intent, by the first render to ask; its frame says what every later
        // draw of the name asks, and its colour which conversion.
        let entry = self.caches.images.read(name.to_vec(), || {
            made.set(true);
            let (decoded, frame, colour) = self.decode_cached(name, request, intent);
            let entry = Arc::new(ImageEntry {
                frame,
                colour,
                decoded: Flights::new(),
            });
            let key = (decoded.at, colour.key(intent));
            entry.decoded.read(key, || Filled::Keep(decoded.cached));
            // Bounded by names, as a hostile page could name thousands; a
            // name by its decodes (`MAX_DECODES_OF_ONE_IMAGE`).
            if self.caches.images.len() <= MAX_CACHED_IMAGES {
                Filled::Keep(entry)
            } else {
                Filled::Once(entry)
            }
        });
        // A render's own cache keeps nothing past it, so no budget reaches
        // what it decodes.
        let at = entry
            .frame
            .map_or(Asked::WHOLE, |frame| frame.asked(request, None));
        let key = (at, entry.colour.key(intent));
        let cached = entry.decoded.read(key, || {
            made.set(true);
            let cached = self.decode_cached(name, request, intent).0.cached;
            if entry.decoded.len() <= MAX_DECODES_OF_ONE_IMAGE {
                Filled::Keep(cached)
            } else {
                Filled::Once(cached)
            }
        });
        if !made.get() {
            self.count(Counter::ImageHits);
            // What the decode said, said again: a render that meets the image
            // here met it as surely as the one that decoded it.
            cached.say_again(self, &String::from_utf8_lossy(name));
        }
        cached.handed()
    }
}

/// Draws a tiling cell for `request` (8.7.3.2) on a renderer over `glyphs`,
/// `paint` making its calls in pattern space: the buffer the request
/// describes, transparent, clipped to the cell's `/BBox`, at the request's
/// depth, levels, rule and thinnest line. Hands back the cell's pixels, what
/// it met, what it drew and whether every paint of it was fully opaque
/// (11.7.5.2). One function for a cell interpreted from its content stream
/// and one replayed from a command stream (`docs/design/command-stream.md`),
/// so the two draw one cell.
pub(crate) fn paint_cell<G: GlyphSource>(
    request: &tinker_pdf_render::TileRequest,
    glyphs: &G,
    paint: impl FnOnce(&mut tinker_pdf_render::Renderer<'_, G>),
) -> (
    Canvas,
    Vec<tinker_pdf_render::RenderWarning>,
    tinker_pdf_render::RenderCounts,
    bool,
) {
    let canvas = Canvas::new(
        request.width,
        request.height,
        request.format,
        Color::TRANSPARENT,
    );
    let mut renderer = tinker_pdf_render::Renderer::new(canvas, request.to_pixels, glyphs)
        .with_cancel(request.cancel.clone())
        .with_pattern_depth(request.depth)
        .with_antialias(request.antialias)
        .with_levels(request.graphics_bits, request.text_bits)
        .with_hard_edge(request.hard_edge)
        .with_thinnest_line(request.thinnest_line)
        .with_held_plates(request.plate_bytes);
    // A coloured cell drawn on plates paints its colorants by the page's plan,
    // so a spot inside it reaches its own plate, and overprints as the page's
    // paints do.
    if let Some(plan) = &request.plates {
        renderer = renderer
            .with_plates(Arc::clone(plan))
            .with_overprint(request.overprint);
    }

    // 8.7.3.2: the cell is clipped to its `/BBox`. The buffer is already that
    // box rounded outward, so this only takes back the part of a pixel the
    // rounding gave away -- but a hatch is normally drawn with a line width
    // that deliberately runs past the box so neighbouring cells join, and
    // without the clip that overshoot lands in the next cell's gap.
    let [x0, y0, x1, y1] = request.bbox;
    let box_path = [
        PathSegment::MoveTo { x: x0, y: y0 },
        PathSegment::LineTo { x: x1, y: y0 },
        PathSegment::LineTo { x: x1, y: y1 },
        PathSegment::LineTo { x: x0, y: y1 },
        PathSegment::Close,
    ];
    // Pattern space, because `to_pixels` is the renderer's `base` and the
    // cell's CTM starts at the identity.
    let state = tinker_pdf_content::GraphicsState::new(Matrix::IDENTITY);
    renderer.clip_path(&box_path, &state, false);

    paint(&mut renderer);
    // 8.7.3.3: a `PaintType 2` cell is drawn as its content stream draws it
    // and is only a shape: the renderer composites its coverage in the `SCN`
    // colour (`Canvas::composite_tinted`), so nothing about colour is decided
    // here and the cell is the same whatever it is filled in.
    //
    // 11.7.5.2 asks whether every object of the cell was fully opaque.
    let opaque = renderer.painted_opaquely();
    let (canvas, warnings, counts) = renderer.finish_counted();
    (canvas, warnings, counts, opaque)
}

/// One decode [`Caches::images`] keeps: the image or the codec's refusal, with
/// what the decode tolerated, and what it was asked for.
struct Kept {
    at: Asked,
    cached: CachedImage,
}

impl PageResources {
    /// Decodes image XObject `name` for a draw that asks as `request` does,
    /// `intent` the rendering intent in force at it — the decoder run of a
    /// miss, counted as one — and hands it back as the cache keeps it, with
    /// the frame later draws read their size against and how its colours take
    /// an intent.
    fn decode_cached(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> (Kept, Option<Frame>, ImageColour) {
        self.count(Counter::ImageMisses);
        self.count(Counter::ImagesDecoded);
        let label = String::from_utf8_lossy(name).into_owned();
        let Decoded {
            image,
            frame,
            colour,
        } = self.decode_image(name, request, intent, None, None);
        // The damage this decode reported: every entry under this name, which
        // only this image's decode writes.
        let damage: Vec<(Option<String>, String)> = self
            .damaged_images
            .lock()
            .map(|damaged| {
                damaged
                    .iter()
                    .filter(|(named, _)| *named == label)
                    .map(|(_, what)| (None, what.clone()))
                    .collect()
            })
            .unwrap_or_default();
        // And the repairs, by the same rule: the decode names its colour
        // space by the image's name.
        let repairs: Vec<(Option<String>, String)> = self
            .repaired_spaces()
            .into_iter()
            .filter(|(named, _)| *named == label)
            .map(|(_, what)| (None, what))
            .collect();
        let cached = CachedImage {
            image: image.map(Arc::new),
            damage,
            repairs,
        };
        (
            Kept {
                at: cached.asked(),
                cached,
            },
            frame,
            colour,
        )
    }

    /// The frame image XObject `name` is decoded against
    /// ([`ImageEntry::frame`]), in `intent`: the scope's cache entry's, which
    /// a decode of the name has already filled, or a decode's own where none
    /// has, kept no longer than the call. `None` for an image no draw
    /// reduces. What a command stream reads its reductions from
    /// ([`Frame::reductions`]).
    pub(crate) fn image_frame(&self, name: &[u8], intent: RenderingIntent) -> Option<Frame> {
        let entry = self.caches.images.read(name.to_vec(), || {
            let (decoded, frame, colour) = self.decode_cached(name, &ImageRequest::FULL, intent);
            let entry = Arc::new(ImageEntry {
                frame,
                colour,
                decoded: Flights::new(),
            });
            let key = (decoded.at, colour.key(intent));
            entry.decoded.read(key, || Filled::Keep(decoded.cached));
            Filled::Once(entry)
        });
        entry.frame
    }

    /// The bytes the document's budget leaves image XObject `name`'s decode
    /// in this scope ([`Room`]): what every draw of it reads the reduction it
    /// takes against beside its size ([`Frame::asked`]), and so what a command
    /// stream holds beside the image's frame. `None` for a render with no
    /// budget, and for a name selecting no image whose decode a draw's size
    /// reduces ([`PageResources::reducible`]), which no budget reduces either.
    pub(crate) fn image_room(&self, name: &[u8]) -> Option<Room> {
        let cache = self.op.cache()?;
        let (dict, _) = self.xobject(name)?;
        let subtype = self
            .doc
            .resolve_key(&dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Image" || !self.reducible(&dict) {
            return None;
        }
        let key_bytes = u64::try_from(self.defaults().written.len()).unwrap_or(u64::MAX);
        Some(Room::under(cache.limits().bytes, key_bytes))
    }

    /// The frame an inline image's decode is read against, as
    /// [`GlyphSource::inline_image`] decodes it in `intent`: a JPEG's or a
    /// codestream's own, `None` for any other. An inline image carries no
    /// mask, so every frame its codec reports reduces.
    pub(crate) fn inline_frame(
        &self,
        dict: &[u8],
        data: &[u8],
        intent: RenderingIntent,
    ) -> Option<Frame> {
        let text = expand_inline_abbreviations(dict);
        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed = tinker_pdf_cos::parse_object_at(&text, 0, self.doc.names_table(), &mut sink);
        let dict = parsed.object.as_dict()?;
        let conversion = self.own_intent(dict).unwrap_or(intent).conversion();
        self.decode_inline(dict, data, &ImageRequest::FULL, conversion, None)
            .ok()
            .and_then(|(_, frame)| frame)
    }

    /// How image XObject `dict`'s colours take a rendering intent
    /// ([`ImageColour`]), `name` the resource name its space is read under.
    fn image_colour(&self, dict: &Dict, name: &[u8]) -> ImageColour {
        // 8.9.6.2: an image mask has no colour of its own. Its paint is the
        // fill colour, which converts in the intent in force like any flat
        // colour, so Table 89's `/Intent` is not read for it.
        let mask = self
            .doc
            .resolve_key(dict, self.name(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);
        if mask {
            return ImageColour::default();
        }
        ImageColour {
            own: self.own_intent(dict),
            by_intent: self
                .image_space(dict, name)
                .is_some_and(|space| reads_an_intent(&space)),
        }
    }

    /// An image dictionary's own rendering intent (Table 89; an inline
    /// image's under the same full key, Table 91), where it is a name: one
    /// 8.6.5.8 does not list is kept as unrecognised, which converts as
    /// relative colorimetric. `None` where the entry is absent or is not a
    /// name, which leaves the intent in force at the draw deciding.
    fn own_intent(&self, dict: &Dict) -> Option<RenderingIntent> {
        let value = self.doc.resolve_key(dict, self.name(b"Intent"));
        let name = value.as_name().and_then(|n| self.doc.name_bytes(n))?;
        Some(RenderingIntent::from_name(&name))
    }
}

impl PageResources {
    /// The image XObject `name` selects, through the document's cache, for a
    /// draw that asks as `request` does in `intent`, the rendering intent in
    /// force at it: its decode kept under the object, what it is asked for,
    /// the conversion its colours take where an intent changes them
    /// ([`ImageColour::key`]) and the defaults this scope paints device
    /// spaces in (`cache::ImageKey`), filled single-flight, and what the
    /// decode reported said again under `name`. `None` for a name that
    /// selects no image XObject, which the scope's own cache answers.
    ///
    /// What the decode is asked for is what the scope's own cache reads
    /// ([`Asked`], [`ImageEntry`]): every sample at full size for an image no
    /// draw reduces, and for a JPEG or a JPEG 2000 codestream with no mask
    /// what its frame gives this draw ([`Frame::asked`]) — a JPEG's scale, a
    /// codestream's levels dropped and the window it holds. The frame is kept
    /// beside the decodes under the image and the defaults
    /// (`cache::FrameKey`), learnt by a decode for this draw, which the
    /// lookup of what it asks then keeps rather than decoding again — so a
    /// hit is the decode a miss for that ask makes, and an image drawn at two
    /// sizes keeps two decodes and one frame.
    ///
    /// The decode runs through resources of its own over this scope's caches
    /// and operation (`serving`), so what it reports is exactly its own,
    /// whatever this render had already reported, and is kept with it.
    fn kept_image(
        &self,
        cache: &DocumentCache,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
    ) -> Option<Result<Option<DecodedImage>, String>> {
        let (dict, reference) = self.xobject(name)?;
        let subtype = self
            .doc
            .resolve_key(&dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))?;
        if subtype.as_ref() != b"Image" {
            return None;
        }
        // Read before the decode, so that a default the scope refuses is the
        // render's to report, as it always was, and not the image's.
        let painted = self.defaults().written.clone();
        let key_bytes = u64::try_from(painted.len()).unwrap_or(u64::MAX);
        // How the image's colours take an intent, read through resources of
        // their own as the decode is, so what the read reports is the
        // decode's to say, once.
        let colour = self.serving(self.op.clone()).image_colour(&dict, name);
        let label = String::from_utf8_lossy(name).into_owned();
        let work = self.op.work.as_deref();
        // The bytes the decode's samples may take and its entry still be kept
        // under the budget, which what a draw asks is read against beside its
        // size: a function of the budget and the key, so the reduction a draw
        // takes is the same whatever the cache holds.
        let room = Room::under(cache.limits().bytes, key_bytes);
        // Whether this draw decoded the image, and the decode it made to
        // learn the frame, for the lookup of what it asks to keep.
        let decoded = Cell::new(false);
        let learnt: Cell<Option<Arc<CachedImage>>> = Cell::new(None);
        let decode = || {
            decoded.set(true);
            self.count(Counter::ImagesDecoded);
            let decoding = self.serving(self.op.clone());
            let Decoded { image, frame, .. } =
                decoding.decode_image(name, request, intent, Some(room), None);
            (Arc::new(CachedImage::of(&image, &decoding, &label)), frame)
        };
        // What the draw asks, and the reduction the budget took it to past
        // what its size asks, where it took one.
        let (at, reduced) = if self.reducible(&dict) {
            let key = FrameKey {
                object: reference,
                painted: painted.clone(),
            };
            let (frame, _) = cache.frame(key, work, || {
                let (entry, frame) = decode();
                learnt.set(Some(entry));
                Fill::Keep(frame, key_bytes)
            });
            match frame {
                Some(frame) => {
                    let at = frame.asked(request, Some(room));
                    let own = frame.asked(request, None);
                    (at, (at.reduction > own.reduction).then_some(at.reduction))
                }
                None => (Asked::WHOLE, None),
            }
        } else {
            (Asked::WHOLE, None)
        };
        let key = ImageKey {
            object: reference,
            asked: at,
            conversion: colour.key(intent),
            painted,
        };
        let (kept, _) = cache.image(key, work, || {
            let entry = match learnt.take() {
                Some(entry) if entry.asked() == at => entry,
                _ => decode().0,
            };
            let bytes = entry.bytes().saturating_add(key_bytes);
            Fill::Keep(entry, bytes)
        });
        self.count(if decoded.get() {
            Counter::ImageMisses
        } else {
            Counter::ImageHits
        });
        kept.say_again(self, &label);
        // Ruling 10: an image the budget took below what its drawn size asks
        // is drawn *and* named, by every render that draws it so, whether the
        // cache served it or it decoded. A refusal draws no picture to name.
        if let (Some(factor), Ok(_)) = (reduced, &kept.image) {
            self.note_reduced_image((label, factor));
        }
        Some(kept.handed())
    }

    /// Whether a draw's size can reduce the decode of image `dict`: a JPEG or
    /// a JPEG 2000 codestream with no soft mask (11.6.5.3) and no `/Mask` of
    /// either form (8.9.6.3, 8.9.6.4), which are sampled against the grid they
    /// were drawn for ([`PageResources::decode_image`]). Each codec's own path
    /// decodes an image of palette indices whole too ([`Frame`]).
    fn reducible(&self, dict: &Dict) -> bool {
        let masked =
            dict.get_ref(self.name(b"SMask")).is_some() || dict.get(self.name(b"Mask")).is_some();
        let codec = matches!(
            self.last_filter(dict).as_deref(),
            Some(b"DCTDecode") | Some(b"DCT") | Some(b"JPXDecode")
        );
        codec && !masked
    }
}

impl PageResources {
    /// The key the cell of the tiling pattern `dict`, object `reference`,
    /// drawn as `request` asks, is kept under in this scope
    /// (`cache::CellKey`): the pattern, the request's every field the cell's
    /// pixels read, this scope's resources where the pattern brings none of
    /// its own, so that its names resolve in them, and the usage, the layer
    /// options and the magnification the operation binds its optional-content
    /// states for — the last only where those states read a `/Zoom` entry.
    fn cell_key(
        &self,
        dict: &Dict,
        reference: ObjRef,
        request: &tinker_pdf_render::TileRequest,
    ) -> cache::CellKey {
        // The rule `drawn_cell` reads a pattern's own scope by.
        let owns = self
            .doc
            .resolve_key(dict, Name::RESOURCES)
            .as_dict()
            .is_some();
        let to = request.to_pixels;
        cache::CellKey {
            pattern: reference,
            to_pixels: [to.a, to.b, to.c, to.d, to.e, to.f].map(f64::to_bits),
            bbox: request.bbox.map(f64::to_bits),
            size: (request.width, request.height),
            format: request.format,
            antialias: request.antialias,
            levels: (request.graphics_bits, request.text_bits),
            hard_edge: request.hard_edge,
            thinnest_line: request.thinnest_line.map(f64::to_bits),
            depth: request.depth,
            scope: (!owns).then(|| self.written().clone()),
            usage: self.op.usage(),
            layers: self.op.layers().clone(),
            magnification: self.op.magnification_key(&self.doc),
            plates: request.plates.clone(),
            plate_bytes: request.plate_bytes,
            overprint: request.overprint,
        }
    }

    /// The cell of the tiling pattern `dict`, object `reference`, drawn into
    /// the buffer `request` describes, and what its drawing told the scope
    /// its content ran in: the pattern's own `/Resources` where it brings
    /// them, and otherwise `scope` — or, where none is handed, this one,
    /// whose lists the drawing then writes to directly, so nothing is kept
    /// apart. Counted as one cell rasterised. `None` where the pattern's
    /// stream will not decode.
    fn drawn_cell(
        &self,
        dict: &Dict,
        reference: ObjRef,
        request: &tinker_pdf_render::TileRequest,
        scope: Option<&PageResources>,
    ) -> Option<cache::KeptCell> {
        let content = self.decoded(reference).ok()?;
        self.count(Counter::TilingCellsRasterised);

        // 8.7.3.2: the cell's own `/Resources`. Gap 11 recorded that a *form*
        // XObject's `/Resources` are consulted nowhere in this engine; a
        // pattern's are, because a cell that names `/F0` almost always means a
        // font the page never heard of — a pattern is a self-contained
        // drawing, where a form is usually written beside the page that
        // invokes it.
        let own = self
            .doc
            .resolve_key(dict, Name::RESOURCES)
            .as_dict()
            .cloned();
        let nested;
        let resources: &PageResources = match own {
            Some(dict) => {
                nested = PageResources::from_dict_in(
                    &self.doc,
                    dict,
                    self.provider.as_ref(),
                    self.op.clone(),
                );
                &nested
            }
            // Falling back to the invoking scope is what a producer that omits
            // the key is relying on, and it is what every reader does.
            None => scope.unwrap_or(self),
        };

        let (canvas, warnings, counts, opaque) = paint_cell(request, resources, |renderer| {
            tinker_pdf_content::interpret(&content, Matrix::IDENTITY, renderer, resources);
        });
        // The cell's renderer is one of this render's own, so the work it did
        // is; the calls it drew are a paint of the fill that names the
        // pattern, not the page's.
        stats::add_render(self.op.work.as_deref(), counts, stats::Draws::Elsewhere);

        // Ruling 10: a font the cell could not resolve, or an image it had to
        // tolerate, is a fact about the page and the page is the only place a
        // caller looks. A cell drawn in this scope has told it already.
        let apart = !std::ptr::eq(resources, self);
        let told = |list: Vec<(String, String)>| if apart { list } else { Vec::new() };
        Some(cache::KeptCell {
            tile: tinker_pdf_render::Tile {
                canvas: Arc::new(canvas),
                warnings,
                opaque,
            },
            missing_fonts: if apart {
                resources.missing_fonts()
            } else {
                Vec::new()
            },
            damaged: told(resources.damaged_images()),
            repaired: told(resources.repaired_spaces()),
            reduced: if apart {
                resources.reduced_images()
            } else {
                Vec::new()
            },
        })
    }
}

impl cache::KeptCell {
    /// Says again, to `resources`, what the cell's drawing told the scope its
    /// content ran in: each font it found none for, each image it decoded
    /// with damage or drew reduced and each colour space it read with a
    /// repair, once.
    fn say_again(&self, resources: &PageResources) {
        if let Ok(mut mine) = resources.missing_fonts.lock() {
            for name in &self.missing_fonts {
                if mine.len() < 64 && !mine.contains(name) {
                    mine.push(name.clone());
                }
            }
        }
        if let Ok(mut mine) = resources.damaged_images.lock() {
            for entry in &self.damaged {
                if !mine.contains(entry) {
                    mine.push(entry.clone());
                }
            }
        }
        for entry in &self.repaired {
            resources.note_repaired_space(entry.clone());
        }
        for entry in &self.reduced {
            resources.note_reduced_image(entry.clone());
        }
    }
}

/// A form being recorded in a scope for the document's cache
/// (`FontSource::kept_form`): the key its drawing will be kept under, the
/// request that missed it, and what the scope has been told since its
/// interpretation began.
struct FormCapture {
    /// `None` for a form whose key could not be built, whose drawing is not
    /// kept.
    key: Option<FormKey>,
    asked: u64,
    /// Fonts the scope was told it has none for: said again by every render
    /// that replays the drawing.
    missing_fonts: Vec<String>,
    /// Whether the scope was told of a colour space read with a repair: the
    /// drawing is then not kept.
    repaired: bool,
}

impl PageResources {
    /// The key the drawing of the form `name` invoked as `invocation` is
    /// kept under in this scope: the invocation, this scope's resources
    /// where the form brings none of its own, so that its names resolve in
    /// them, and the usage the operation binds its optional-content states
    /// for. `None` for a name that selects no form here, or one the
    /// invocation does not name.
    fn form_key(
        &self,
        name: &[u8],
        invocation: &tinker_pdf_content::FormInvocation,
    ) -> Option<FormKey> {
        let (dict, reference) = self.xobject(name)?;
        if crate::structure::stream_id(reference) != invocation.stream {
            return None;
        }
        // The rule `build_form_resources` reads a form's own scope by.
        let owns = self
            .doc
            .resolve_key(&dict, Name::RESOURCES)
            .as_dict()
            .is_some();
        Some(FormKey {
            invocation: invocation.clone(),
            scope: (!owns).then(|| self.written().clone()),
            usage: self.op.usage(),
            layers: self.op.layers().clone(),
            magnification: self.op.magnification_key(&self.doc),
        })
    }

    /// The font `font_id` selects here as the document's cache keys a glyph
    /// mask's font: its object, or, for a font written into this scope's own
    /// dictionary, its id in this scope's resources written exactly. `None`
    /// where it selects none, whose masks are not kept.
    fn mask_font(&self, font_id: u64) -> Option<cache::MaskFont> {
        Some(match GlyphSource::font_identity(self, font_id)? {
            FontIdentity::Object(object) => cache::MaskFont::Object(object),
            FontIdentity::Scoped => cache::MaskFont::Scoped {
                font_id,
                scope: self.written().clone(),
            },
        })
    }

    /// This scope's resource dictionary written exactly, once a scope.
    fn written(&self) -> &Written {
        self.caches.written.get_or_init(|| {
            let mut out = Vec::new();
            match self.resources.as_deref() {
                Some(resources) => {
                    out.push(1);
                    cache::write_exact_dict(resources, &mut out);
                }
                None => out.push(0),
            }
            Written::of(out)
        })
    }
}

/// Which scope a form's contents run in, for [`PageResources::form_from`].
#[derive(Clone, Copy)]
enum Contents {
    /// An XObject's: its own `/Resources` when it brings them (8.10.1), the
    /// invoking scope when it does not.
    OwnScope,
    /// A soft mask's `/G`, which the interpreter runs in the scope that set
    /// the `gs` (`docs/design/icc.md`, "The scope a default is read in").
    ThisScope,
}

impl PageResources {
    /// A group's `/CS` (11.6.6), as its contents are painted: each device
    /// space in it remapped by the defaults of the scope those contents run
    /// in (8.6.5.6), and read here, where what it tolerates is reported.
    ///
    /// 11.6.6 remaps by "the current resource dictionary" without saying
    /// whose; the scope the contents run in is the reading that never paints
    /// them in one calibration and composites them in another. Today the
    /// group keeps only the space's shape ([`group_space`]), which a default
    /// cannot change — it has its device space's component count and is
    /// never `/Lab` — so no pixel moves until a group composites through its
    /// space.
    fn group_colour_space(
        &self,
        cs: &Object,
        name: &[u8],
        contents: Contents,
    ) -> Option<ColorSpace> {
        let own = match contents {
            Contents::OwnScope => self.form_resources(name),
            Contents::ThisScope => None,
        };
        let scope: &PageResources = own.as_deref().unwrap_or(self);
        self.read_space(cs, 0, name, scope.painted())
    }

    /// The `/DecodeParms` of a stream, which may be one dictionary or an
    /// array with one entry per filter (7.4.1).
    /// A form XObject read from its own dictionary.
    ///
    /// Split out of [`FontSource::form`] because an ExtGState `/SMask`
    /// reaches a form through `/G` rather than through the `/XObject`
    /// resource table, and a second reader for the same dictionary is how
    /// `/Matrix` or `/BBox` comes to be honoured on one route and not the
    /// other.
    ///
    /// `contents` says which scope the form's contents run in, which is the
    /// one its group's `/CS` is remapped by (11.6.6, 8.6.5.6): an XObject's
    /// own `/Resources` when it has them, and this scope for a soft mask's
    /// `/G`, which the interpreter runs where the `gs` was.
    fn form_from(
        &self,
        dict: &Dict,
        reference: ObjRef,
        name: &[u8],
        contents: Contents,
    ) -> Option<tinker_pdf_content::Form> {
        let content = self.decoded(reference).ok()?;
        // 8.10.2: /Matrix maps the form's space into the one that invoked it.
        let matrix = self
            .doc
            .resolve_key(dict, self.name(b"Matrix"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| values.get(i).and_then(Object::as_number);
                Some(Matrix {
                    a: n(0)?,
                    b: n(1)?,
                    c: n(2)?,
                    d: n(3)?,
                    e: n(4)?,
                    f: n(5)?,
                })
            })
            .unwrap_or(Matrix::IDENTITY);

        // 8.10.2 requires it; a file that omits it gets an unclipped form
        // rather than no form.
        let bbox = self
            .doc
            .resolve_key(dict, self.name(b"BBox"))
            .as_array()
            .and_then(|values| {
                let n = |i: usize| values.get(i).and_then(|v| self.doc.resolve(v).as_number());
                // The corners may be given in either order (7.9.5).
                let (x0, y0, x1, y1) = (n(0)?, n(1)?, n(2)?, n(3)?);
                Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
            });

        // 11.6.6: `/Group` with `/S /Transparency` makes the form a
        // transparency group, which is composited as a unit rather than
        // element by element. Any other subtype — 8.10.3's `/S /Reference`
        // group — is not one, so the name is checked rather than the key's
        // presence.
        let group = self
            .doc
            .resolve_key(dict, self.name(b"Group"))
            .as_dict()
            .and_then(|group| {
                let subtype = self
                    .doc
                    .resolve_key(group, self.name(b"S"))
                    .as_name()
                    .and_then(|n| self.doc.name_bytes(n))?;
                if subtype.as_ref() != b"Transparency" {
                    return None;
                }
                // 11.6.6 Table 147: both default to false, and a value that is
                // not a boolean is not a value.
                Some(tinker_pdf_content::Group {
                    isolated: self
                        .doc
                        .resolve_key(group, self.name(b"I"))
                        .as_bool()
                        .unwrap_or(false),
                    knockout: self
                        .doc
                        .resolve_key(group, self.name(b"K"))
                        .as_bool()
                        .unwrap_or(false),
                    // 11.6.6: `/CS` names the space the group's contents are
                    // composited in. Resolved through the same seam every
                    // other colour space goes through — the `/BC` backdrop of
                    // a luminosity soft mask already read this key, and read
                    // it this way — then reduced to the shape a compositor
                    // needs.
                    space: group
                        .get(self.name(b"CS"))
                        .cloned()
                        .and_then(|cs| self.group_colour_space(&cs, name, contents))
                        .map(group_space),
                })
            });

        Some(tinker_pdf_content::Form {
            // The operation's one decode of the form, however often it is
            // drawn: the interpreter takes the content as its own.
            content: Arc::unwrap_or_clone(content),
            matrix,
            bbox,
            group,
            // 14.7.4.2: the stream's own identity, which is the half of a
            // marked-content identifier the `/MCID` does not carry. Both
            // routes into a form arrive here, so the `/XObject` table and a
            // soft mask's `/G` cannot come to disagree about it.
            stream: crate::structure::stream_id(reference),
        })
    }

    fn decode_parms(&self, dict: &Dict) -> Option<Dict> {
        let value = self.doc.resolve_key(dict, Name::DECODE_PARMS);
        if let Some(d) = value.as_dict() {
            return Some(d.clone());
        }
        // The last filter's parameters are the ones an image codec wants.
        let items = value.as_array()?;
        items
            .iter()
            .rev()
            .find_map(|o| self.doc.resolve(o).as_dict().cloned())
    }

    /// Reads a `/Function` entry, which is one function or an array of them,
    /// one per output component (7.10).
    fn function(&self, dict: &Dict) -> Option<Function> {
        let key = self.name(b"Function");
        let value = self.doc.resolve_key(dict, key);
        if let Some(items) = value.as_array() {
            // 7.10.1: every member supplies one output component. Reading only
            // the first turns an RGB gradient into a red ramp on black, and
            // does it silently — a one-output function is valid on its own, so
            // nothing downstream can tell the difference.
            let parsed: Vec<Function> = items
                .iter()
                .filter_map(|o| self.parse_function(o, 0))
                .collect();
            return match parsed.len() {
                0 => None,
                1 => parsed.into_iter().next(),
                _ => Some(Function::Array(parsed)),
            };
        }
        // The entry **as written**, not resolved: a type 0 or type 4 function
        // is a stream (7.10.2, 7.10.5), which `parse_function` reaches through
        // its reference — and handed the resolved object it found no reference
        // and read nothing, so every shading whose one function was sampled or
        // a calculator painted as `Function::Identity`'s grey ramp, silently.
        // The array arm above never had the defect: its members are read
        // unresolved.
        dict.get(key).and_then(|raw| self.parse_function(raw, 0))
    }

    /// The function `object` holds, `object` the entry as written, as a
    /// value of its own: one an object holds is copied out of the scope's
    /// cache ([`PageResources::kept_function`]).
    fn parse_function(&self, object: &Object, depth: u32) -> Option<Function> {
        self.kept_function(object, depth).map(Arc::unwrap_or_clone)
    }

    /// The function `object` holds, `object` the entry as written and
    /// `depth` how deep it is read: one an object holds read once a scope
    /// for each depth it is read at, and kept ([`Caches::functions`]); one
    /// written inline read where it lies. A type 0 or type 4 function is a
    /// stream (7.10.2, 7.10.5), so its object is the reference it is reached
    /// through.
    fn kept_function(&self, object: &Object, depth: u32) -> Option<Arc<Function>> {
        if depth > MAX_FUNCTION_DEPTH {
            return None;
        }
        let Some(reference) = object.as_objref() else {
            return self.read_function(object, depth).map(Arc::new);
        };
        // A function that holds no other reads alike at every depth, and is
        // kept by its object alone; a stitching function's pieces stop where
        // the depth does, so it is kept by its object and the depth it was
        // read at.
        if let Ok(cache) = self.caches.functions.lock() {
            let hit = cache
                .get(&(reference, None))
                .or_else(|| cache.get(&(reference, Some(depth))));
            if let Some(hit) = hit {
                return hit.clone();
            }
        }
        self.count(Counter::FunctionsRead);
        let read = self.read_function(object, depth).map(Arc::new);
        let at = matches!(read.as_deref(), Some(Function::Stitching { .. })).then_some(depth);
        if let Ok(mut cache) = self.caches.functions.lock() {
            if cache.len() < MAX_CACHED_ENTRIES {
                cache.insert((reference, at), read.clone());
            }
        }
        read
    }

    /// Reads the function `object` holds (7.10), its pieces one deeper.
    fn read_function(&self, object: &Object, depth: u32) -> Option<Function> {
        if depth > MAX_FUNCTION_DEPTH {
            return None;
        }
        let resolved = self.doc.resolve(object);
        let dict = resolved.as_dict()?;

        let numbers = |key: &[u8]| -> Vec<f64> {
            let value = self.doc.resolve_key(dict, self.name(key));
            value
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| self.doc.resolve(o).as_number())
                        .collect()
                })
                .unwrap_or_default()
        };
        let pairs = |values: &[f64]| -> Vec<(f64, f64)> {
            values.chunks_exact(2).map(|p| (p[0], p[1])).collect()
        };

        let kind = self
            .doc
            .resolve_key(dict, self.name(b"FunctionType"))
            .as_int()?;

        match kind {
            2 => {
                let c0 = numbers(b"C0");
                let c1 = numbers(b"C1");
                let domain = numbers(b"Domain");
                Some(Function::Exponential {
                    domain: (
                        domain.first().copied().unwrap_or(0.0),
                        domain.get(1).copied().unwrap_or(1.0),
                    ),
                    c0: if c0.is_empty() { vec![0.0] } else { c0 },
                    c1: if c1.is_empty() { vec![1.0] } else { c1 },
                    n: self
                        .doc
                        .resolve_key(dict, Name::N)
                        .as_number()
                        .unwrap_or(1.0),
                })
            }
            3 => {
                let value = self.doc.resolve_key(dict, self.name(b"Functions"));
                let functions: Vec<Function> = value
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| self.parse_function(o, depth + 1))
                            .collect()
                    })
                    .unwrap_or_default();
                let domain = numbers(b"Domain");
                Some(Function::Stitching {
                    domain: (
                        domain.first().copied().unwrap_or(0.0),
                        domain.get(1).copied().unwrap_or(1.0),
                    ),
                    functions,
                    bounds: numbers(b"Bounds"),
                    encode: pairs(&numbers(b"Encode")),
                })
            }
            0 | 4 => {
                // Both are streams, so the object handed in is the reference.
                let reference = object.as_objref()?;
                let data = self.decoded(reference).ok()?;
                if kind == 4 {
                    return Some(Function::PostScript {
                        domain: pairs(&numbers(b"Domain")),
                        range: pairs(&numbers(b"Range")),
                        program: tinker_pdf_color::function::parse_postscript(&data),
                    });
                }
                Some(Function::Sampled {
                    domain: pairs(&numbers(b"Domain")),
                    range: pairs(&numbers(b"Range")),
                    size: numbers(b"Size").iter().map(|v| *v as usize).collect(),
                    bits: self
                        .doc
                        .resolve_key(dict, self.name(b"BitsPerSample"))
                        .as_int()
                        .unwrap_or(8) as u32,
                    encode: pairs(&numbers(b"Encode")),
                    decode: pairs(&numbers(b"Decode")),
                    samples: Arc::unwrap_or_clone(data),
                })
            }
            _ => None,
        }
    }

    /// Records that a font resolved no glyph for a code it was asked for.
    ///
    /// Step 4 of the fallback order in gap 01 (CFF glyph selection): the
    /// glyph drawn is `.notdef`, and the page says so rather than
    /// silently drawing something else. Only an operation that draws reports
    /// it: a text extraction's operation reports no glyph
    /// (`Operation::reporting_no_glyphs`), since what its glyph lookups ask is
    /// what a code stands for, so the same list stays exactly what its name
    /// says for the extraction path, which turns it into
    /// `TextWarning::UnknownFont`.
    fn report_unresolved_glyph(&self, font: &[u8]) {
        if self.op.quiet_glyphs {
            return;
        }
        let name = String::from_utf8_lossy(&fonts::shown(font)).into_owned();
        if let Ok(mut marker) = self.unresolved.lock() {
            *marker = Some(name.clone());
        }
        self.note_missing_font(name);
    }

    /// How many fonts these resources have met that they could not resolve:
    /// what a caller delivering calls one at a time reads after each, as
    /// [`PageResources::tolerated`] is read, to say which call each was met
    /// at (`command_stream.rs`).
    pub(crate) fn missing_count(&self) -> usize {
        self.missing_fonts.lock().map_or(0, |list| list.len())
    }

    /// The tiling cell pattern `name` selects here (8.7.3.2): its content
    /// stream, decoded, and the resources its `/Resources` names, serving
    /// this scope's operation, or `None` where it names none and draws in
    /// this scope. `None` for a name that selects no pattern stream: a direct
    /// dictionary carrying the keys but no stream has no cell to draw, and
    /// declining is what turns it back into the reported gap it was.
    ///
    /// 8.7.3.2: the cell's own `/Resources`. Gap 11 recorded that a *form*
    /// XObject's `/Resources` are consulted nowhere in this engine; a
    /// pattern's are, because a cell that names `/F0` almost always means a
    /// font the page never heard of — a pattern is a self-contained drawing,
    /// where a form is usually written beside the page that invokes it.
    pub(crate) fn cell(&self, name: &[u8]) -> Option<(Arc<Vec<u8>>, Option<PageResources>)> {
        let (dict, reference) = self.pattern_entry(name)?;
        let content = self.decoded(reference?).ok()?;
        let own = self
            .doc
            .resolve_key(&dict, Name::RESOURCES)
            .as_dict()
            .cloned()
            .map(|dict| {
                PageResources::from_dict_in(
                    &self.doc,
                    dict,
                    self.provider.as_ref(),
                    self.op.clone(),
                )
            });
        Some((content, own))
    }

    /// Adds a font to what this render could not resolve, once, and to what
    /// every form being recorded here has met.
    fn note_missing_font(&self, name: String) {
        if let Ok(mut recording) = self.recording.lock() {
            for capture in recording.iter_mut() {
                if capture.missing_fonts.len() < 64 && !capture.missing_fonts.contains(&name) {
                    capture.missing_fonts.push(name.clone());
                }
            }
        }
        if let Ok(mut missing) = self.missing_fonts.lock() {
            if missing.len() < 64 && !missing.contains(&name) {
                missing.push(name);
            }
        }
    }

    /// Adds an image's tolerated damage to what this render met, once.
    fn note_damaged_image(&self, entry: (String, String)) {
        if let Ok(mut damaged) = self.damaged_images.lock() {
            if damaged.len() < 64 && !damaged.contains(&entry) {
                damaged.push(entry);
            }
        }
    }

    /// Adds a colour space read with a repair to what this render met, once.
    ///
    /// Once per name and repair rather than per use: the `cs` path asks for
    /// the space at every `sc`, and a page that sets a colour a thousand times
    /// has one space to report.
    fn note_repaired_space(&self, entry: (String, String)) {
        // A repair a form's interpretation met is said where it met it, among
        // what the device says beside it, which a recording cannot keep in
        // order: the form's drawing is not kept.
        if let Ok(mut recording) = self.recording.lock() {
            for capture in recording.iter_mut() {
                capture.repaired = true;
            }
        }
        if let Ok(mut repaired) = self.repaired_spaces.lock() {
            if repaired.len() < 64 && !repaired.contains(&entry) {
                repaired.push(entry);
            }
        }
    }

    /// The font program behind a font id, read once a scope and decoded once
    /// an operation, and whether a provider substituted it.
    fn program(&self, font_id: u64) -> Option<FontProgram> {
        let made = Cell::new(false);
        let program = self.caches.programs.read(font_id, || {
            made.set(true);
            self.count(Counter::FontProgramMisses);
            Filled::Keep(self.font_by_id(font_id).and_then(|found| {
                program_for(&self.doc, &found, self.provider.as_deref(), &self.op)
            }))
        });
        if !made.get() {
            self.count(Counter::FontProgramHits);
        }
        program
    }

    /// The font a text-font name stands for in this scope.
    fn font(&self, name: &[u8]) -> Option<Arc<cos_font::Font>> {
        self.scope_font(name).map(|found| Arc::clone(&found.font))
    }

    /// The font a text-font name selects in this scope (7.8.3): the `/Font`
    /// entry for the name, or — for a reserved name a graphics state gave
    /// (`fonts::state_font`) — the object it spells. Read on the first
    /// question about the name and kept in the scope's table, shared by every
    /// render over its caches, so no glyph reads a font dictionary.
    fn scope_font(&self, name: &[u8]) -> Option<Arc<ScopeFont>> {
        if let Ok(table) = self.caches.fonts.read() {
            if let Some(hit) = table.by_name.get(name) {
                return hit.clone();
            }
        }
        let read = self.read_scope_font(name).map(Arc::new);
        if let Ok(mut table) = self.caches.fonts.write() {
            // Another render over the same caches may have read it
            // meanwhile; the entry first kept is the one every render holds.
            if let Some(hit) = table.by_name.get(name) {
                return hit.clone();
            }
            if read.is_some() || table.by_name.len() < MAX_UNREAD_FONT_NAMES {
                if let Some(found) = &read {
                    table.by_id.insert(found.id, Some(Arc::clone(found)));
                }
                table.by_name.insert(name.to_vec(), read.clone());
            }
        }
        read
    }

    /// Reads the font `name` selects, as [`PageResources::scope_font`] keeps
    /// it: the scope's `/Font` first, so no name that resolves there resolves
    /// differently, and a reserved name's object after. Counted as a font
    /// dictionary resolved when a dictionary is read.
    fn read_scope_font(&self, name: &[u8]) -> Option<ScopeFont> {
        let bound = self.resources.as_deref().and_then(|resources| {
            let table = resources.get(Name::FONT)?;
            read_resolved(&self.doc, table, |table| {
                let key = self.name(name);
                let entry = table.as_dict()?.get(key)?;
                let dict = self.doc.resolve(entry);
                dict.as_dict()?;
                Some((u64::from(key.id()), entry.as_objref(), dict))
            })
        });
        let (id, object, dict) = match bound {
            Some(bound) => bound,
            None => {
                let reference = fonts::reserved_reference(name)?;
                let object = self.doc.get(reference).ok()?;
                object.as_dict()?;
                (fonts::reserved_id(reference), Some(reference), object)
            }
        };
        let font_dict = dict.as_dict()?;
        self.count(Counter::FontDictionariesResolved);
        // Through the operation's view: the CMaps reading the font decodes
        // are decoded once the operation, whichever of its scopes reads the
        // font first, and counted, with any key it spells out, when it counts.
        let font = self.op.read_font(&self.doc, font_dict);
        let type3 = font_dict
            .get_name(Name::SUBTYPE)
            .and_then(|n| self.doc.name_bytes(n))
            .is_some_and(|subtype| subtype.as_ref() == b"Type3");
        Some(ScopeFont {
            name: name.to_vec(),
            id,
            object,
            dict,
            font: Arc::new(font),
            type3,
            procedures: OnceLock::new(),
            type1: OnceLock::new(),
            type1_read: OnceLock::new(),
            extent: OnceLock::new(),
        })
    }

    /// What [`FontSource::font_extent`] answers for `found`, read from its
    /// program and its descriptor.
    fn read_extent(&self, found: &ScopeFont) -> Option<(f64, f64)> {
        /// `hhea`.
        const HHEA: u32 = u32::from_be_bytes(*b"hhea");
        let from_program = self.program(found.id).and_then(|program| {
            let sfnt = Sfnt::parse(&program.bytes)?;
            let hhea = sfnt.table(HHEA)?;
            let read = |at: usize| {
                let bytes = hhea.get(at..at + 2)?;
                Some(f64::from(i16::from_be_bytes([bytes[0], bytes[1]])))
            };
            let (ascender, descender) = (read(4)?, read(6)?);
            let units = f64::from(sfnt.units_per_em.max(1));
            (ascender != 0.0 || descender != 0.0).then(|| (ascender / units, descender / units))
        });
        if from_program.is_some() {
            return from_program;
        }

        let dict = found.dict.as_dict()?;
        // A composite font keeps its descriptor on the descendant.
        let descendants = self.doc.resolve_key(dict, Name::DESCENDANT_FONTS);
        let descendant = descendants
            .as_array()
            .and_then(<[Object]>::first)
            .map(|first| self.doc.resolve(first));
        let holder = descendant
            .as_deref()
            .and_then(Object::as_dict)
            .unwrap_or(dict);
        let descriptor = self.doc.resolve_key(holder, Name::FONT_DESCRIPTOR);
        let descriptor = descriptor.as_dict()?;
        let number = |key: &[u8]| {
            self.doc
                .resolve_key(descriptor, self.name(key))
                .as_number()
                .filter(|value| value.is_finite())
        };
        if let (Some(ascent), Some(descent)) = (number(b"Ascent"), number(b"Descent")) {
            if ascent != 0.0 || descent != 0.0 {
                return Some((ascent / 1000.0, descent / 1000.0));
            }
        }
        let bbox = self.doc.resolve_key(descriptor, self.name(b"FontBBox"));
        let corners: Vec<f64> = bbox
            .as_array()?
            .iter()
            .map(|corner| self.doc.resolve(corner).as_number())
            .collect::<Option<Vec<f64>>>()?;
        let [_, lower, _, upper] = corners[..] else {
            return None;
        };
        (lower.is_finite() && upper.is_finite() && lower < upper)
            .then(|| (upper / 1000.0, lower / 1000.0))
    }

    /// A Type 3 font's procedures, as its dictionary `font` gives them: the
    /// codes `/Encoding`'s `/Differences` names, `/CharProcs` and
    /// `/FontMatrix`. An `/Encoding` that is no dictionary, a `/Differences`
    /// that is no array and a `/CharProcs` that is no dictionary give a record
    /// that answers every code with nothing, as the walk this replaced did.
    fn read_type3(&self, font: &Dict) -> Type3Procedures {
        let encoding = self.doc.resolve_key(font, Name::ENCODING);
        let names = encoding
            .as_dict()
            .map(|encoding| {
                let differences = self.doc.resolve_key(encoding, Name::DIFFERENCES);
                differences
                    .as_array()
                    .map(|items| type3_codes(&self.doc, items))
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        // 9.6.5: /FontMatrix maps glyph space to text space. Unlike every
        // other font it is not 1/1000 by convention — a Type 3 font chooses
        // its own units, and the matrix is required for that reason.
        let matrix = self
            .doc
            .resolve_key(font, Name::FONT_MATRIX)
            .as_array()
            .and_then(|values| {
                let n = |i: usize| values.get(i).and_then(Object::as_number);
                Some(Matrix {
                    a: n(0)?,
                    b: n(1)?,
                    c: n(2)?,
                    d: n(3)?,
                    e: n(4)?,
                    f: n(5)?,
                })
            });
        Type3Procedures {
            names,
            procs: self.doc.resolve_key(font, Name::CHAR_PROCS),
            matrix,
        }
    }

    /// The font a font id stands for in this scope.
    ///
    /// An id the table has no entry for yet is one another scope over the same
    /// dictionary handed out — an appearance is drawn over two, the
    /// interpreter's and the renderer's (`annots::draw_prepared`) — and is
    /// found among this scope's `/Font` names, whose keys carry the ids, and
    /// read on that first question; a reserved name's id by the object it
    /// spells. An id no name carries answers nothing, kept as firmly.
    fn font_by_id(&self, font_id: u64) -> Option<Arc<ScopeFont>> {
        if let Ok(table) = self.caches.fonts.read() {
            if let Some(hit) = table.by_id.get(&font_id) {
                return hit.clone();
            }
        }
        let name = match fonts::reserved_id_reference(font_id) {
            Some(reference) => Some(fonts::reserved_name(reference)),
            None => self.font_name_of(font_id),
        };
        let found = name.and_then(|name| self.scope_font(&name));
        if let Ok(mut table) = self.caches.fonts.write() {
            if found.is_some() || table.by_id.len() < MAX_UNREAD_FONT_NAMES {
                table.by_id.entry(font_id).or_insert_with(|| found.clone());
            }
        }
        found
    }

    /// The name in this scope's `/Font` whose id is `font_id`.
    fn font_name_of(&self, font_id: u64) -> Option<Vec<u8>> {
        let table = self.resources.as_deref()?.get(Name::FONT)?;
        read_resolved(&self.doc, table, |table| {
            let (key, _) = table
                .as_dict()?
                .iter()
                .find(|(key, _)| u64::from(key.id()) == font_id)?;
            self.doc.name_bytes(*key).map(|bytes| bytes.to_vec())
        })
    }

    /// This scope's `/Font` subdictionary (7.8.3).
    fn font_table(&self) -> Option<Dict> {
        let resources = self.resources.as_deref()?;
        self.doc
            .resolve_key(resources, Name::FONT)
            .as_dict()
            .cloned()
    }

    /// The object a font resource name stands for.
    ///
    /// **This is the identity [`crate::subset`] keys glyph usage by, and
    /// [`tinker_pdf_content::Glyph::font_id`] is not.** That id is the
    /// interned *resource name* — `/F1` — which is scope-relative: a form
    /// XObject with its own `/Resources` may bind `/F1` to a different font
    /// from the page that drew it, and the two then share an id. An object
    /// reference is the document's own answer to "which font", and two of them
    /// are equal exactly when the font is.
    ///
    /// `None` for a font written directly into the resource dictionary rather
    /// than by reference: there is no object for a rewrite to address, so
    /// subsetting leaves such a font whole rather than editing a copy of it
    /// that the file does not use.
    pub(crate) fn font_object(&self, font_id: u64) -> Option<ObjRef> {
        // A reserved name's id carries the object itself.
        if let Some(reference) = fonts::reserved_id_reference(font_id) {
            return Some(reference);
        }
        self.font_by_id(font_id)?.object
    }

    /// The dictionary a font id stands for in this scope, as the scope's
    /// table holds it.
    ///
    /// What [`crate::subset`]'s walk names a font written directly into a
    /// resource dictionary by, which [`PageResources::font_object`] answers
    /// `None` for: such a font has no object for a rewrite to address, and a
    /// report that it was drawn is all a pass over the editor can make of it.
    pub(crate) fn font_dict(&self, font_id: u64) -> Option<Dict> {
        self.font_by_id(font_id)?.dict.as_dict().cloned()
    }

    /// Every font this scope names, by object.
    ///
    /// What makes "this scope was walked and never showed that font" sayable,
    /// which is what lets an embedded program whose text a redaction removed
    /// be cut down to `.notdef` rather than left whole.
    pub(crate) fn font_objects(&self) -> Vec<ObjRef> {
        let mut objects: Vec<ObjRef> = self
            .font_table()
            .map(|table| {
                table
                    .iter()
                    .filter_map(|(_, value)| value.as_objref())
                    .collect()
            })
            .unwrap_or_default();
        // 8.4.5: a graphics state names a font by reference (`/Font [font
        // size]`), which this scope's content may select as `Tf` selects one
        // `/Font` names.
        if let Some(resources) = self.resources.as_deref() {
            let states = self.doc.resolve_key(resources, Name::EXT_G_STATE);
            for (_, entry) in states.as_dict().into_iter().flat_map(Dict::iter) {
                let state = self.doc.resolve(entry);
                let font = state
                    .as_dict()
                    .and_then(|state| state.get(Name::FONT))
                    .map(|font| self.doc.resolve(font));
                let reference = font
                    .as_deref()
                    .and_then(Object::as_array)
                    .and_then(<[Object]>::first)
                    .and_then(Object::as_objref);
                if let Some(reference) = reference.filter(|r| !objects.contains(r)) {
                    objects.push(reference);
                }
            }
        }
        objects
    }

    /// Every font this scope writes **directly** into its `/Font`
    /// subdictionary rather than by reference.
    ///
    /// The complement of [`PageResources::font_objects`], and it exists
    /// because the two together are the whole table: a font with no object has
    /// no identity for [`crate::subset`] to key glyph usage by, so a program it
    /// embeds is one that pass must leave whole — including when some *other*
    /// font reaches the same program by reference, which is the case that
    /// would otherwise lose this one's glyphs.
    pub(crate) fn direct_fonts(&self) -> Vec<Dict> {
        let Some(table) = self.font_table() else {
            return Vec::new();
        };
        table
            .iter()
            .filter_map(|(_, value)| value.as_dict().cloned())
            .collect()
    }

    /// Pulls one glyph's outline out of an embedded font program.
    fn extract_outline(&self, font_id: u64, code: u32) -> Option<Outline> {
        let FontProgram {
            bytes: program,
            substituted,
        } = self.program(font_id)?;

        // The font's own character mapping decides which glyph a code means.
        let found = self.font_by_id(font_id)?;
        let (name, font) = (&found.name, &found.font);

        if let Some(sfnt) = Sfnt::parse(&program) {
            // An OpenType font may carry its outlines in a `CFF ` table
            // instead of `glyf` — that is what the `OTTO` tag means. The sfnt
            // parser accepts `OTTO`, so such a font took this branch, found
            // no `glyf`, and `glyf::outline` returned an *empty* outline,
            // which the renderer reads as a legitimate space. The whole face
            // drew as blank with nothing reported.
            if sfnt.table(0x676C_7966).is_none() {
                if let Some(table) = sfnt.table(0x4346_4620) {
                    if let Some(cff) = Cff::parse(table) {
                        let glyph = if substituted {
                            self.substituted_glyph(
                                &program,
                                Some(&sfnt),
                                Some(&cff),
                                font,
                                name,
                                code,
                            )
                        } else {
                            self.cff_selection(&cff, font, name, code).glyph
                        };
                        return self.cff_outline(&cff, glyph);
                    }
                }
                return None;
            }

            let glyph = if substituted {
                self.substituted_glyph(&program, Some(&sfnt), None, font, name, code)
            } else {
                self.sfnt_selection(&sfnt, font, name, code)?.glyph
            };

            let units = f64::from(sfnt.units_per_em.max(1));
            let outline = glyf::outline(&sfnt, glyph)?;
            return Some(scale(&outline, 1.0 / units));
        }

        if let Some(cff) = Cff::parse(&program) {
            let glyph = if substituted {
                self.substituted_glyph(&program, None, Some(&cff), font, name, code)
            } else {
                self.cff_selection(&cff, font, name, code).glyph
            };
            return self.cff_outline(&cff, glyph);
        }

        // 9.9: a `/FontFile` is a Type 1 program. Until this existed the bytes
        // reached the two parsers above, both declined them correctly, and the
        // glyph was silently absent — an embedded Type 1 font drew nothing and
        // said nothing about why.
        if let Some(type1) = self.type1_program(&found, &program, substituted) {
            // Type 1 addresses glyphs by *name*, the one decision
            // [`PageResources::selection`] makes too.
            let glyph = type1_selection(type1, font, code)?.glyph;
            let outline = type1.outline(glyph)?;
            let scale_factor = type1.font_matrix.first().copied().unwrap_or(0.001);
            return Some(scale(&outline, scale_factor));
        }

        None
    }

    /// The font's Type 1 program as drawn, parsed on its first glyph drawn
    /// and kept for every glyph after it: an embedded multiple-master program
    /// set to the instance its font names, or to its default, and every other
    /// program, which has no weight vector, as it is read.
    fn type1_program<'f>(
        &self,
        found: &'f ScopeFont,
        program: &[u8],
        substituted: bool,
    ) -> Option<&'f Type1<'static>> {
        found
            .type1
            .get_or_init(|| {
                let mut type1 = Type1::parse(program)?;
                if type1.is_multiple_master() && !substituted {
                    self.instance_multiple_master(found, &mut type1);
                }
                Some(type1)
            })
            .as_ref()
    }

    /// Sets an embedded multiple-master program to the instance its font
    /// names, and reports which once a font
    /// (`docs/design/type42-and-multiple-master.md`).
    ///
    /// An `/MMType1` font names its instance in `/BaseFont`, by the design
    /// coordinates [`design_coordinates`] reads; a `/Type1` font names none,
    /// and draws the program's default, its `/WeightVector`, as does a name
    /// without coordinates, with another number than the program's axes, or
    /// a blend design the font crate does not instance. The instance moves
    /// outlines and advances, never which glyph a code chooses, so this is
    /// asked where an outline is drawn and nowhere else.
    fn instance_multiple_master(&self, found: &ScopeFont, type1: &mut Type1<'_>) {
        let mm_type1 = found
            .dict
            .as_dict()
            .and_then(|dict| dict.get_name(Name::SUBTYPE))
            .and_then(|subtype| self.doc.name_bytes(subtype))
            .is_some_and(|subtype| subtype.as_ref() == b"MMType1");
        let drawn = if mm_type1 {
            let axes = type1.blend().map(tinker_pdf_font::Blend::axis_count);
            match (design_coordinates(found.font.base_font()), axes) {
                (None, _) => MultipleMasterInstance::DefaultNoCoordinates,
                (Some(_), None) => MultipleMasterInstance::DefaultBlendDesign,
                (Some(design), Some(axes)) if design.len() != axes => {
                    MultipleMasterInstance::DefaultAxisCount
                }
                (Some(design), Some(_)) => match type1.instance(&design) {
                    Ok(instance) if instance.clamped => MultipleMasterInstance::Clamped,
                    Ok(_) => MultipleMasterInstance::Design,
                    Err(tinker_pdf_font::InstanceRefusal::AxisCount { .. }) => {
                        MultipleMasterInstance::DefaultAxisCount
                    }
                    Err(_) => MultipleMasterInstance::DefaultBlendDesign,
                },
            }
        } else {
            MultipleMasterInstance::DefaultType1Font
        };
        self.doc
            .warn_once(found.object, WarningKind::MultipleMasterInstance(drawn));
    }

    /// Which glyph of the embedded program a code selects, and on whose
    /// authority, without drawing anything.
    ///
    /// The **same decision** [`PageResources::extract_outline`] makes, reached
    /// through the same two helpers, because the glyph-usage walk in
    /// [`crate::subset`] has to keep exactly the glyphs this renderer would
    /// draw. A second resolver written beside this one would agree on the
    /// ordinary cases and diverge on the ones that matter — a symbolic face, a
    /// non-identity `/CIDToGIDMap`, a CFF reached by name — and every
    /// divergence is a glyph dropped from a program that still needs it, which
    /// renders as a blank rather than as an error.
    ///
    /// [`Selection::stated`] is what subsetting actually rests on; see its own
    /// documentation.
    ///
    /// A Type 1 program answers with its charstring's position in the order
    /// `Type1::parse` reads them, which nothing outside the program addresses
    /// and the Type 1 subsetter takes; a code no route reaches answers the
    /// guess `.notdef`, so a program any shown code of which reaches nothing
    /// is left whole. `None` where no glyph index exists to keep: no embedded
    /// program, or one no parser here reads.
    pub(crate) fn selection(&self, font_id: u64, code: u32) -> Option<Selection> {
        let FontProgram {
            bytes: program,
            substituted,
        } = self.program(font_id)?;
        // A substituted face is not the document's, and nothing subsetting
        // keeps is a glyph of it.
        if substituted {
            return None;
        }
        let found = self.font_by_id(font_id)?;
        let (name, font) = (&found.name, &found.font);

        if let Some(sfnt) = Sfnt::parse(&program) {
            if sfnt.table(0x676C_7966).is_none() {
                let cff = Cff::parse(sfnt.table(0x4346_4620)?)?;
                return Some(self.cff_selection(&cff, font, name, code));
            }
            return self.sfnt_selection(&sfnt, font, name, code);
        }
        if let Some(cff) = Cff::parse(&program) {
            return Some(self.cff_selection(&cff, font, name, code));
        }
        // The program as read: no instance moves which glyph a code chooses,
        // and none is set where nothing is drawn.
        if let Some(type1) = found.type1_read.get_or_init(|| Type1::parse(&program)) {
            return Some(type1_selection(type1, font, code).unwrap_or_else(|| {
                Selection::guessed(type1.glyph_for_name(b".notdef").unwrap_or(0))
            }));
        }
        None
    }

    /// Which glyph of a `glyf`-flavoured program a code selects (9.6.6.4,
    /// 9.7.4.2).
    fn sfnt_selection(
        &self,
        sfnt: &Sfnt<'_>,
        font: &cos_font::Font,
        name: &[u8],
        code: u32,
    ) -> Option<Selection> {
        if font.kind() == cos_font::FontKind::Type0 {
            // `/CIDToGIDMap` is exhaustive by construction — a CID it does not
            // name is one the font does not have — so even the `.notdef` it
            // answers with is the font's own statement rather than a guess.
            return Some(Selection::stated(self.cid_glyph(font, name, code)));
        }

        if let Some(glyph) = simple_glyph_by_cmap(sfnt, font, code) {
            return Some(Selection::stated(glyph));
        }

        // The last resort: a subset font whose `cmap` was dropped,
        // where the code is the only glyph number on offer, and the
        // guess every reader makes for it. It used to be correct for
        // a composite font too — an identity `/CIDToGIDMap` makes the
        // code the glyph — but that reading only held while every
        // composite font's CMap was the identity as well. The branch
        // above owns that case now and indexes by CID, which is the
        // same number whenever the old reading was right and the
        // right one whenever it was not.
        Some(Selection::guessed(u16::try_from(code).ok()?))
    }

    /// Which glyph of a **substituted** face — a host's or a bundled one,
    /// never the document's own program — a code selects, reporting the
    /// glyph unresolved where [`substitute_choice`] says the reading does:
    /// that function is the reading, and the save door's embedding pass
    /// (`crate::embed`) asks it what this drew.
    fn substituted_glyph(
        &self,
        program: &[u8],
        sfnt: Option<&Sfnt<'_>>,
        cff: Option<&Cff<'_>>,
        font: &cos_font::Font,
        name: &[u8],
        code: u32,
    ) -> u16 {
        let chosen = substitute_choice(program, sfnt, cff, font, code);
        if chosen.reported() {
            self.report_unresolved_glyph(name);
        }
        chosen.glyph
    }

    /// Which glyph of a CFF program a code selects, the way 9.6.6 says to
    /// choose it.
    fn cff_selection(
        &self,
        cff: &Cff<'_>,
        font: &cos_font::Font,
        resource: &[u8],
        code: u32,
    ) -> Selection {
        match cff_glyph(cff, font, code) {
            Some(glyph) => Selection::stated(glyph),
            None => {
                // Nothing named this glyph. `.notdef` is glyph 0 in every CFF
                // font — usually empty, sometimes a box — and the page reports
                // rather than drawing whichever glyph the code happened to
                // number.
                self.report_unresolved_glyph(resource);
                Selection::guessed(0)
            }
        }
    }

    /// Which glyph of a TrueType program a composite font's code selects
    /// (9.7.4.2).
    ///
    /// A CIDFontType2 addresses its descendant by CID, and `/CIDToGIDMap`
    /// turns that CID into a glyph index. The font program's own `cmap` is
    /// **not** consulted on this path and must not be: a `cmap` answers "which
    /// glyph draws this character", and a composite font's code is not a
    /// character. Until August 2026 it was the only thing consulted here — the
    /// code went through `/ToUnicode` to a character and the character through
    /// the `cmap` — so a document whose CMap was not the identity drew a page
    /// of real glyphs, correctly spaced, that were not the ones it named.
    ///
    /// A CID the map does not name draws `.notdef` and reports, the same way
    /// an unresolvable CFF glyph does. Without the report it is a space, which
    /// is the failure that looks like nothing happened.
    fn cid_glyph(&self, font: &cos_font::Font, resource: &[u8], code: u32) -> u16 {
        let cid = font.cid_of(code);
        let glyph = font.gid_for_cid(cid);
        // CID 0 is `.notdef` by definition and reports nothing; any other CID
        // resolving to glyph 0 is a CID this font does not carry.
        if glyph == 0 && cid != 0 {
            self.report_unresolved_glyph(resource);
        }
        glyph
    }

    /// One glyph of a CFF program, once [`PageResources::cff_selection`] has
    /// chosen it.
    fn cff_outline(&self, cff: &Cff<'_>, glyph: u16) -> Option<Outline> {
        let outline = cff.outline(glyph)?;
        // A CFF font matrix is usually 1/1000 but need not be, and a
        // CID-keyed font may carry a different one per Font DICT.
        let factor = cff.font_matrix_for(glyph).first().copied().unwrap_or(0.001);
        Some(scale(&outline, factor))
    }

    /// Decodes one image XObject to RGB (8.9) for a draw that asks as
    /// `request` does, `intent` the rendering intent in force at it, and says
    /// what the draws after it read their size from and how its colours take
    /// an intent.
    ///
    /// Only a JPEG and a JPEG 2000 codestream are decoded smaller than they
    /// are, and only one with no mask: a soft mask (11.6.5.3) or a `/Mask` of
    /// either form (8.9.6.3, 8.9.6.4) is sampled against the grid it was drawn
    /// for, so an image with one is decoded at full size whatever the draw
    /// asks. Each codec's own path decodes an image of palette indices whole
    /// too ([`Frame`]). Its colours convert in its own `/Intent` where it
    /// names one, and in `intent` otherwise (8.6.5.8, Table 89).
    ///
    /// `room` is what the document's budget leaves its samples where the
    /// document keeps the decode ([`Room`]): an image a draw's size can
    /// reduce is decoded at the least reduction past what the size asks whose
    /// samples fit it, where what the size asks does not fit and one does
    /// (`docs/design/performance.md`, P-16, "Reduced-resolution decode"). An
    /// image no draw reduces is decoded whole whatever it leaves.
    fn decode_image(
        &self,
        name: &[u8],
        request: &ImageRequest,
        intent: RenderingIntent,
        room: Option<Room>,
        plates: Option<&PlatePlan>,
    ) -> Decoded {
        let Some((dict, reference)) = self.xobject(name) else {
            return Decoded {
                image: Err(String::from_utf8_lossy(name).into_owned()),
                frame: None,
                colour: ImageColour::default(),
            };
        };
        let colour = self.image_colour(&dict, name);
        let conversion = colour.conversion(intent);
        let label = String::from_utf8_lossy(name).into_owned();
        // An image a soft mask applies to is held as RGB, because the mask is
        // composed into its alpha a sample at a time; any other one-bit image
        // of one component is held as its two colours.
        let soft = dict.get_ref(self.name(b"SMask")).is_some();
        // And so is one decoded for plates, whose tints sit beside its light
        // a sample at a time.
        let form = if soft || plates.is_some() {
            SampleForm::Rgb
        } else {
            SampleForm::TwoColour
        };
        let reducible = self.reducible(&dict);
        let ask = if reducible {
            DecodeAsk {
                request,
                conversion,
                room,
                plates,
            }
        } else {
            DecodeAsk {
                request: &ImageRequest::FULL,
                conversion,
                room: None,
                plates,
            }
        };
        let decoded = self
            .decode_image_at(&dict, reference, &label, form, ask)
            .map(|(mut image, frame)| {
                // 11.6.5.3: /SMask is a greyscale image whose samples are this
                // one's opacity. Without it every soft-masked image — a drop
                // shadow, a feathered edge, anything composited in a design
                // tool — paints as an opaque rectangle over the page.
                if let Some(mask) = self.soft_mask(&dict) {
                    apply_soft_mask(&mut image, &mask);
                    image.soft_masked = true;
                }
                (image, frame)
            });
        // The frame a later draw's size is read against: the codec's own,
        // which a reduced decode says it stands for and a full one is — none
        // for an image a mask keeps whole.
        match decoded {
            Ok((image, frame)) => Decoded {
                image: Ok(image),
                frame: frame.filter(|_| reducible),
                colour,
            },
            Err(codec) => Decoded {
                image: Err(codec),
                frame: None,
                colour,
            },
        }
    }

    /// Records that an image decoded with something tolerated (ruling 10).
    fn report_damaged_image(&self, name: &[u8], warning: FilterWarning) {
        if let Ok(mut damaged) = self.damaged_images.lock() {
            let entry = (
                String::from_utf8_lossy(name).into_owned(),
                warning.as_str().to_string(),
            );
            if damaged.len() < 64 && !damaged.contains(&entry) {
                damaged.push(entry);
            }
        }
        if let Ok(mut capture) = self.image_capture.lock() {
            if let Some(caught) = capture.as_mut() {
                // Bounded without a cap: the warning set is closed, and each
                // is kept once.
                let reason = warning.as_str().to_string();
                if !caught.contains(&reason) {
                    caught.push(reason);
                }
            }
        }
    }

    /// Runs `decode`, and returns with its result every leniency
    /// [`Self::report_damaged_image`] heard while it ran — the warnings of the
    /// one image `decode` is decoding. Whatever was capturing before is
    /// restored, so a mask decoded inside an image keeps its own.
    fn capturing<T>(&self, decode: impl FnOnce() -> T) -> (T, Vec<String>) {
        let outer = self
            .image_capture
            .lock()
            .ok()
            .and_then(|mut capture| capture.replace(Vec::new()));
        let result = decode();
        let caught = self
            .image_capture
            .lock()
            .ok()
            .and_then(|mut capture| std::mem::replace(&mut *capture, outer))
            .unwrap_or_default();
        (result, caught)
    }

    /// Decodes a CCITT stream into the packed one-bit samples the sample loop
    /// in [`Self::decode_image_at`] reads (7.4.6).
    ///
    /// **The single entry point for fax data in this file.** The XObject path
    /// reaches it above; gap 08 routes `decode_inline` to this same function
    /// rather than growing a second copy of Table 11's defaults. That matters
    /// more than it looks: `ccitt_decode`'s signature does not change when the
    /// meaning of its bytes changes, so a second call site would be invisible
    /// to the compiler and would go on producing the old shape in silence.
    ///
    /// `/Columns` is the *coding* width and need not be `/Width` — Table 11
    /// defaults it to 1728 whatever the image is — so the decoded rows are
    /// re-laid at the image's own width here. The bits past `/Columns` in a
    /// decoded row are white, so a wider image gains white rather than an edge
    /// that is not in the fax.
    fn ccitt_samples(
        &self,
        dict: &Dict,
        coded: &[u8],
        width: u32,
        height: u32,
        name: &[u8],
    ) -> Vec<u8> {
        let parms = self.decode_parms(dict);
        let int = |key: &[u8]| parms.as_ref().and_then(|p| p.get_int(self.name(key)));
        let flag = |key: &[u8]| parms.as_ref().and_then(|p| p.get_bool(self.name(key)));

        // Table 11: `/Rows` is the height of the coded image. It is optional,
        // and the image's own `/Height` is what it means when it is absent —
        // which is what this used to pass unconditionally, so a file whose two
        // heights disagreed was decoded at the wrong one.
        let rows = int(b"Rows")
            .filter(|rows| *rows > 0)
            .unwrap_or(i64::from(height))
            .clamp(1, 1 << 16) as u32;
        let params = CcittParams {
            k: int(b"K").unwrap_or(0) as i32,
            columns: int(b"Columns").unwrap_or(1728).clamp(1, 1 << 16) as u32,
            rows,
            black_is_1: flag(b"BlackIs1").unwrap_or(false),
            byte_align: flag(b"EncodedByteAlign").unwrap_or(false),
            // Table 11's other two flags, neither of which was consulted.
            // `/EndOfLine` says an EOL is required before every line, so a
            // missing one is a leniency worth naming rather than a shape the
            // decoder silently absorbs. `/EndOfBlock false` says the data
            // carries no EOFB, which is what makes `/Rows` above the authority
            // on where the image ends instead of a hint the decoder may run
            // past.
            end_of_line: flag(b"EndOfLine").unwrap_or(false),
            end_of_block: flag(b"EndOfBlock").unwrap_or(true),
        };

        // The same ceiling every other stream in the document decodes under.
        // Every row costs at least one bit of input, so the decode is bounded
        // by the coded length long before it reaches this.
        let (packed, warnings) = ccitt_decode(coded, &params, limits::MAX_DECODED_STREAM);
        for warning in warnings {
            // Ruling 10. These were discarded, so a fax with half its rows
            // missing was reported as a clean render of a mostly blank page.
            self.report_damaged_image(name, warning);
        }

        let coded_stride = (params.columns as usize).div_ceil(8);
        let stride = (width as usize).div_ceil(8);
        let white = if params.black_is_1 { 0x00 } else { 0xFF };
        let mut samples = vec![white; (height as usize).saturating_mul(stride)];
        let run = stride.min(coded_stride);
        for y in 0..height as usize {
            let from = y * coded_stride;
            let to = y * stride;
            let (Some(source), Some(target)) =
                (packed.get(from..from + run), samples.get_mut(to..to + run))
            else {
                break;
            };
            target.copy_from_slice(source);
        }
        samples
    }

    /// Decodes a JBIG2 stream into the packed one-bit samples the sample loop
    /// in [`Self::decode_image_at`] reads (ISO 32000-1 7.4.7).
    ///
    /// **The polarity inversion lives here, and nowhere else.** T.88 6.2.2
    /// codes 1 for black; a 1-bit DeviceGray sample is 0 for black. The
    /// decoder returns JBIG2's own sense unconverted — as [`T6Rows`] does for
    /// a fax — so exactly one place in the build knows which convention it is
    /// translating between, and it is the place that also reads `/Decode`,
    /// `/ImageMask` and `/ColorSpace`. A decoder that inverted for itself
    /// would be guessing what its caller wanted, and a second caller would
    /// invert again.
    ///
    /// **It joins the generic sample path deliberately**, for the reason gap
    /// 16 gave when it moved the fax onto it: an image that returns its own
    /// pixels has to reimplement `/Decode`, `/ImageMask` and the colour space
    /// to compose correctly, and a JBIG2 scan is an image mask about as often
    /// as it is a DeviceGray image. Coming out as *samples* means those three
    /// keys are read once, where they have always been read.
    ///
    /// `None` is the refusal: no region decoded, so the caller draws the
    /// placeholder rather than a blank page (ruling 2).
    fn jbig2_samples(
        &self,
        dict: &Dict,
        coded: &[u8],
        width: u32,
        height: u32,
        name: &[u8],
    ) -> Option<Vec<u8>> {
        // 7.4.7: `/JBIG2Globals` is a *stream*, in this image's own
        // `/DecodeParms`, holding the segments shared by every image that
        // names it. It is the ordinary shape for the symbol dictionary of a
        // scanned book, so a build that ignored it would refuse a file whose
        // whole payload is in one place and say nothing about why. It is no
        // image's samples, so the operation decodes it once for them all.
        let globals = self
            .decode_parms(dict)
            .and_then(|parms| match parms.get(self.name(b"JBIG2Globals")) {
                Some(Object::Ref(r)) => self.decoded(*r).ok(),
                _ => None,
            })
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &globals,
            width,
            height,
            work_ceiling: Some(jbig2_work_ceiling(width, height)),
        };
        let decoded = jbig2_decode(coded, &params, limits::MAX_DECODED_STREAM, &mut warnings);
        for warning in &warnings {
            // Ruling 10, and it carries most of the information here: the
            // refusal below is one error with no detail, and "no region,
            // because this is a symbol dictionary" and "no region, because
            // the stream was truncated" are different failures.
            self.report_damaged_image(name, *warning);
        }

        let mut samples = decoded.ok()?;
        // The inversion, stated once. Padding bits past the image's width sit
        // in the same byte and flip with it, which is correct: they were 0 for
        // white in JBIG2 and become 1 for white here.
        for byte in &mut samples {
            *byte = !*byte;
        }
        Some(samples)
    }

    /// The space a JPEG's or a JPEG 2000 image's samples are drawn in, of
    /// `components` a pixel: the `/ColorSpace` its dictionary names, read as
    /// `named` (8.9.5) — for a codestream, over its own colour specification
    /// (7.4.9) — or `None` for the codec's own colour.
    ///
    /// The codec's own colour stands where the dictionary names no space this
    /// reads, and where it names the device space of the samples' own count,
    /// which is the space the codec's own conversion draws. A space of
    /// another count cannot hold the samples: it is refused and named under
    /// the image's own name, and the codec's own colour drawn (ruling 10).
    fn codec_space(
        &self,
        named: Option<ColorSpace>,
        components: usize,
        name: &[u8],
    ) -> Option<ColorSpace> {
        let space = named?;
        if space.components() != components {
            self.note_repaired_space((
                String::from_utf8_lossy(name).into_owned(),
                "ImageSpaceComponents".to_string(),
            ));
            return None;
        }
        let own = matches!(
            space,
            ColorSpace::DeviceGray | ColorSpace::DeviceRgb | ColorSpace::DeviceCmyk
        );
        (!own).then_some(space)
    }

    /// A JPEG's pixels, ready to blit (7.4.8), in `named`, the space its
    /// dictionary's `/ColorSpace` was read as, converted as `conversion`
    /// says.
    ///
    /// Shared by the XObject path and the inline one so the two differ in
    /// nothing but where the still-coded bytes came from and the scope their
    /// space is read in — the ceiling, the colour conversion and `/Decode`
    /// included.
    ///
    /// A JPEG carries its own dimensions and its own components, so it never
    /// joins the generic sample loop. Drawn in its dictionary's space, its
    /// samples take `/Decode` before the space, as the loop's do
    /// ([`samples_in_space`]); drawn in its own colour, `/Decode` is applied
    /// to the converted output instead, which is what
    /// [`invert_if_decode_reverses`] is for.
    ///
    /// The decode hands its strips to [`JpegInto`], which writes each into
    /// the image's RGB, so the decode holds a strip and its coefficients
    /// beside the image and no more; what it held is counted into
    /// [`crate::Stats::jpeg_held`].
    ///
    /// The frame it hands back beside the image is the JPEG's own size, which
    /// a later draw's scale is read against; none under an `/Indexed` space,
    /// whose samples are indices (8.6.6.3) — a smaller scale averages them
    /// into indices no sample chose — so such a JPEG is decoded whole, under a
    /// budget too.
    fn jpeg_image(
        &self,
        raw: &[u8],
        decode: &[(f64, f64)],
        interpolate: bool,
        named: Option<ColorSpace>,
        name: &[u8],
        ask: DecodeAsk<'_>,
    ) -> Result<(DecodedImage, Option<Frame>), String> {
        let DecodeAsk {
            request,
            conversion,
            room,
            plates,
        } = ask;
        let indexed = matches!(named, Some(ColorSpace::Indexed { .. }));
        let (request, room) = if indexed {
            (&ImageRequest::FULL, None)
        } else {
            (request, room)
        };
        let mut into = JpegInto {
            resources: self,
            named,
            name,
            decode,
            conversion,
            space: None,
            own: None,
            color: JpegColor::Gray,
            width: 0,
            rgb: Vec::new(),
            plates,
            tints: None,
            nothing: false,
            plate_tints: Vec::new(),
        };
        // The scale is the frame's own size against the drawn size and what
        // the budget leaves its samples, asked when the decode reads the
        // frame, so it is sized by the bytes rather than by what the
        // dictionary says they hold.
        let choose = |width: u32, height: u32| jpeg_scale_within((width, height), request, room);
        let decoded = jpeg_decode_strips_sized(raw, JPEG_CEILING, &choose, &mut into);
        let work = self.op.work.as_deref();
        // What the strips' converters asked of a conversion, whether or not
        // the decode reached its end.
        into.count_into(work);
        let frame = decoded.map_err(|e| format!("{e:?}"))?;
        stats::jpeg_held(work, frame.held.total());
        stats::jpeg_transformed(work, frame.transformed);
        let layout = frame.layout;
        let pixels = (layout.width as usize) * (layout.height as usize);
        // The family of the space the samples are drawn in: the dictionary's,
        // or the codec's own colour where it is drawn in that.
        let family = match (&into.space, into.color) {
            (Some((space, _)), _) => tinker_pdf_render::color_family(space),
            (None, JpegColor::Gray) => tinker_pdf_content::ColorFamily::Gray,
            (None, JpegColor::Rgb) => tinker_pdf_content::ColorFamily::Rgb,
            (None, JpegColor::Cmyk | JpegColor::CmykInverted) => {
                tinker_pdf_content::ColorFamily::Cmyk
            }
        };
        let mut rgb = into.rgb;
        rgb.resize(pixels * 3, 255);
        // A strip the decode stopped short of is no ink, as its light is
        // paper.
        let plates = match (into.nothing, into.tints) {
            (true, _) => Some(ImagePlates::Nothing),
            (false, Some((tints, _))) => {
                let channels = tints.channels();
                let mut out = into.plate_tints;
                out.resize(pixels * channels, 0);
                Some(ImagePlates::Tints {
                    tints: out,
                    channels,
                    marks: tints.marks(),
                })
            }
            (false, None) => None,
        };
        let reduced = (layout.scale != JpegScale::Full).then(|| Reduced {
            full_width: layout.full_width,
            full_height: layout.full_height,
            factor: reduction(layout.scale),
            skip: (0, 0),
        });
        let frame = (!indexed).then_some(Frame::Jpeg {
            width: layout.full_width,
            height: layout.full_height,
        });
        let image = DecodedImage {
            width: layout.width,
            height: layout.height,
            rgb,
            alpha: Vec::new(),
            stencil: false,
            family,
            soft_masked: false,
            interpolate,
            two_colour: None,
            reduced,
            window: None,
            plates,
        };
        Ok((image, frame))
    }

    /// A JPEG 2000 image's pixels, ready to blit (8.9.5.4), in `named`, the
    /// space its dictionary's `/ColorSpace` was read as.
    ///
    /// **One entry point, and both the XObject path and the inline path go
    /// through it.** That is not tidiness: gap 16 changed the CCITT decoder's
    /// output shape without changing its signature, so a second call site would
    /// have been invisible to the compiler and would have rendered every fax as a
    /// negative at one eighth width with a green suite. Gap 08 was reordered
    /// behind gap 16 for exactly that reason, and its injection showed a parallel
    /// call site being caught *only* by a warning test -- no pixel assertion saw
    /// it. JPX carries the same hazard with more surface.
    ///
    /// # What 8.9.5.4 does with the image dictionary
    ///
    /// A JPX codestream describes itself, and the clause says which of the
    /// dictionary's claims survive that:
    ///
    /// - **`/BitsPerComponent` is ignored.** The codestream's precision wins. A
    ///   file whose dictionary disagrees must not shift its samples, which is why
    ///   this never reads the key.
    /// - **`/ColorSpace` overrides** the codestream's when present, and the
    ///   codestream's applies when it is absent ([`Self::codec_space`]). Note
    ///   the ordering trap M6 found: a palette makes the *output* channel count
    ///   differ from `Csiz`, so it is the output count a `/ColorSpace` must
    ///   agree with.
    /// - **`/Decode` is ignored**, unlike every other image filter here.
    /// - **`/ImageMask` shall not be combined with JPXDecode**, so a file that
    ///   does is refused by name rather than rendered as a stencil of something.
    /// - **`/SMaskInData`** decides what happens to the codestream's *own*
    ///   opacity channel, which the filters crate has already separated out.
    ///
    /// # The levels and the window it decodes
    ///
    /// A draw that asks for less than the codestream's size is decoded with
    /// the most resolution levels dropped whose image still covers it
    /// ([`jpx_discard`], `docs/design/jpx-speed.md`, "The page's request"),
    /// held to the codestream's own size as its header gives it rather than
    /// to the dictionary's, and its samples placed over the image they stand
    /// for ([`Reduced`]); and a draw that reaches or shows part of the image
    /// is decoded over that part alone, widened by the sampler's reach
    /// ([`JpxFrame::window`]), the image's samples outside it left out
    /// ([`DecodedImage::window`]). The frame it hands back beside the image is
    /// what a later draw's ask is read against ([`JpxFrame`]): no level is
    /// dropped under an `/Indexed` space, whose samples are indices (8.6.6.3)
    /// a lower resolution would average — as the decoder drops none of a
    /// codestream a `pclr` palette reads
    /// ([`tinker_pdf_filters::JpxHeader::levels`]) — though a window of it is
    /// still a window. Where the document keeps the decode under a budget, a
    /// decode the drawn size asks for whose samples would not fit what the
    /// budget leaves drops more levels, the fewest whose do
    /// ([`JpxFrame::ask_within`]). What the decode did is counted into
    /// [`crate::Stats::jpx_levels_synthesised`],
    /// [`crate::Stats::jpx_code_blocks_decoded`] and
    /// [`crate::Stats::jpx_levels_discarded`].
    fn jpx_image(
        &self,
        raw: &[u8],
        dict: &Dict,
        interpolate: bool,
        named: Option<ColorSpace>,
        name: &[u8],
        ask: DecodeAsk<'_>,
    ) -> Result<(DecodedImage, Option<Frame>), String> {
        let DecodeAsk {
            request,
            conversion,
            room,
            plates,
        } = ask;
        let smask_in_data = dict.get_int(self.name(b"SMaskInData")).unwrap_or(0);
        let image_mask = dict.get_bool(self.name(b"ImageMask")).unwrap_or(false);
        if image_mask {
            // 8.9.6.2 makes a stencil one bit per sample with no colour space;
            // 8.9.5.4 forbids the combination outright. Refusing by name beats
            // painting the fill colour through whatever the codestream held.
            return Err("JPXDecode with /ImageMask".to_string());
        }

        // The codestream's own size, origin and levels, read before a sample
        // is decoded. A header the read refuses is a codestream the decode
        // refuses at the same stage, so it is asked for whole and refused
        // with its reason.
        let frame = tinker_pdf_filters::jpx_header(raw, &jpx_limits())
            .ok()
            .map(|header| JpxFrame {
                width: header.width,
                height: header.height,
                origin: header.origin,
                levels: if matches!(named, Some(ColorSpace::Indexed { .. })) {
                    0
                } else {
                    header.levels
                },
                opaque: smask_in_data == 0 || !header.opacity,
                interpolate,
            });
        let (discard, window) = frame.map_or((0, None), |frame| frame.ask_within(request, room));
        let mut warnings = Vec::new();
        // The decode's parts — code-blocks, tiles, components, wavelet
        // levels — on the threads the operation may spend, serially unless
        // its caller asked for them.
        let decoded = Self::jpx_samples(
            raw,
            &tinker_pdf_filters::JpxRequest {
                discard,
                window: window.map(|held| tinker_pdf_filters::JpxWindow {
                    x: held.x,
                    y: held.y,
                    width: held.width,
                    height: held.height,
                }),
            },
            &self.op.parallelism,
            &mut warnings,
        );
        for warning in &warnings {
            self.report_damaged_image(name, *warning);
        }
        let (image, counts) = decoded?;
        stats::jpx_decoded(
            self.op.work.as_deref(),
            counts.levels_synthesised,
            counts.code_blocks_decoded,
            image.discard,
        );
        // The image the samples are part of: the codestream's own size, or
        // its size reduced, of which they hold the window asked.
        let whole = frame.map_or((image.width, image.height), |frame| {
            frame.reduced(image.discard)
        });
        let reduced = frame.filter(|_| image.discard > 0).map(|frame| Reduced {
            full_width: frame.width,
            full_height: frame.height,
            factor: 1 << image.discard.min(31),
            skip: (
                jpx_skip(frame.origin.0, image.discard),
                jpx_skip(frame.origin.1, image.discard),
            ),
        });

        let count = (image.width as usize) * (image.height as usize);
        let drawn = self.codec_space(named, usize::from(image.components), name);
        // A codestream's own colour is no PDF device space.
        let family = drawn.as_ref().map_or(
            tinker_pdf_content::ColorFamily::Other,
            tinker_pdf_render::color_family,
        );
        let mut rgb = match &drawn {
            // 8.9.5.4 ignores `/Decode`, so none is applied, and a sixteen-bit
            // codestream is read at its own precision.
            Some(space) => {
                let bits = if image.precision > 8 { 16 } else { 8 };
                samples_in_space(
                    space,
                    &image.samples,
                    count,
                    bits,
                    &[],
                    conversion,
                    self.op.work.as_deref(),
                )
            }
            None => Self::jpx_own_colour(&image, count)?,
        };

        // `/SMaskInData` (Table 89): 0 means ignore any opacity the codestream
        // carries, 1 means it is present and unpremultiplied, 2 means present and
        // premultiplied. The filters crate separates the channel out and records
        // which; un-premultiplying is deliberately left here, because it is the
        // dictionary that says whether to.
        let alpha = match (smask_in_data, image.opacity.as_ref()) {
            (0, _) | (_, None) => Vec::new(),
            (_, Some(opacity)) => {
                let mut alpha = Vec::with_capacity(count);
                for i in 0..count {
                    let a = if image.precision > 8 {
                        opacity.samples.get(i * 2).copied().unwrap_or(255)
                    } else {
                        opacity.samples.get(i).copied().unwrap_or(255)
                    };
                    alpha.push(a);
                }
                // 2 says the colour is already multiplied by the opacity, so it
                // has to be divided back out before the renderer multiplies it
                // again. Skipping this darkens every transparent edge, which
                // looks like a soft shadow rather than like a bug.
                if smask_in_data == 2 || opacity.premultiplied {
                    for (i, &a) in alpha.iter().enumerate() {
                        if a == 0 {
                            continue;
                        }
                        for channel in 0..3 {
                            let at = i * 3 + channel;
                            if let Some(v) = rgb.get_mut(at) {
                                let restored = (u32::from(*v) * 255) / u32::from(a);
                                #[allow(clippy::cast_possible_truncation, reason = "min 255")]
                                {
                                    *v = restored.min(255) as u8;
                                }
                            }
                        }
                    }
                }
                alpha
            }
        };

        // For plates, each sample's tints beside its light (RC-04.12), read in
        // the space it is drawn in, or a four-component codestream's own ink:
        // its raw components, taken out of a premultiplied opacity as its
        // light is.
        let premultiplied = smask_in_data == 2
            || image
                .opacity
                .as_ref()
                .is_some_and(|opacity| opacity.premultiplied);
        let tinted = match (&drawn, image.components) {
            (Some(space), _) => Some(space.clone()),
            (None, 4) => Some(ColorSpace::DeviceCmyk),
            (None, _) => None,
        };
        let plates = match (plates, tinted) {
            (Some(plan), Some(space)) => match ImagePlating::of(&space, plan) {
                Some(ImagePlating::Nothing) => Some(ImagePlates::Nothing),
                Some(ImagePlating::Tints(mut tints)) => {
                    let per = usize::from(image.components);
                    let wide = image.precision > 8;
                    let (bits, max) = if wide { (16, 65535) } else { (8, 255) };
                    let mut out = Vec::with_capacity(count.saturating_mul(tints.channels()));
                    let mut values = Vec::with_capacity(per);
                    for i in 0..count {
                        let a = alpha.get(i).copied().map_or(255, u32::from);
                        values.clear();
                        for c in 0..per {
                            let at = i * per + c;
                            let raw = if wide {
                                let hi = image.samples.get(at * 2).copied().unwrap_or(0);
                                let lo = image.samples.get(at * 2 + 1).copied().unwrap_or(0);
                                u32::from(u16::from_be_bytes([hi, lo]))
                            } else {
                                u32::from(image.samples.get(at).copied().unwrap_or(0))
                            };
                            let value = if premultiplied && a != 0 && a != 255 {
                                (raw * 255 / a).min(max)
                            } else {
                                raw
                            };
                            values.push(value);
                        }
                        let light = match rgb.get(i * 3..i * 3 + 3) {
                            Some(&[r, g, b]) => (r, g, b),
                            _ => (255, 255, 255),
                        };
                        tints.push(&space, &[], &values, bits, light, &mut out);
                    }
                    Some(ImagePlates::Tints {
                        channels: tints.channels(),
                        marks: tints.marks(),
                        tints: out,
                    })
                }
                None => None,
            },
            _ => None,
        };

        // `/SMaskInData`'s opacity is the only alpha a codestream brings.
        let soft_masked = !alpha.is_empty();
        let decoded = DecodedImage {
            width: whole.0,
            height: whole.1,
            rgb,
            alpha,
            stencil: false,
            family,
            soft_masked,
            interpolate,
            two_colour: None,
            reduced,
            window,
            plates,
        };
        Ok((decoded, frame.map(Frame::Jpx)))
    }

    /// A JPEG 2000 image's samples in the codestream's own colour, by how
    /// many components it has: one is grey, three RGB and four CMYK, the
    /// codestream's colour specification read for nothing more (7.4.9).
    fn jpx_own_colour(
        image: &tinker_pdf_filters::JpxImage,
        count: usize,
    ) -> Result<Vec<u8>, String> {
        let mut rgb = Vec::with_capacity(count * 3);

        // 16-bit samples arrive big-endian; the renderer wants eight, and taking
        // the high byte is a truncation the clause permits rather than a rounding
        // decision worth inventing.
        let sample = |i: usize, channel: usize| -> u8 {
            let per = usize::from(image.components);
            let at = i * per + channel;
            if image.precision > 8 {
                image.samples.get(at * 2).copied().unwrap_or(0)
            } else {
                image.samples.get(at).copied().unwrap_or(0)
            }
        };

        for i in 0..count {
            match image.components {
                1 => {
                    let g = sample(i, 0);
                    rgb.extend_from_slice(&[g, g, g]);
                }
                3 => rgb.extend_from_slice(&[sample(i, 0), sample(i, 1), sample(i, 2)]),
                4 => {
                    // CMYK, and the conversion is the naive one the colour crate
                    // uses elsewhere rather than a second opinion about ink.
                    let (c, m, y, k) = (
                        f64::from(sample(i, 0)) / 255.0,
                        f64::from(sample(i, 1)) / 255.0,
                        f64::from(sample(i, 2)) / 255.0,
                        f64::from(sample(i, 3)) / 255.0,
                    );
                    let to = |v: f64| {
                        #[allow(clippy::cast_possible_truncation, reason = "clamped to 0..=255")]
                        #[allow(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
                        {
                            ((1.0 - (v + k).min(1.0)) * 255.0).clamp(0.0, 255.0) as u8
                        }
                    };
                    rgb.extend_from_slice(&[to(c), to(m), to(y)]);
                }
                other => return Err(format!("JPXDecode with {other} components")),
            }
        }
        Ok(rgb)
    }

    /// The final filter a stream dictionary names (7.4), which decides how
    /// its bytes are read.
    fn last_filter(&self, dict: &Dict) -> Option<Vec<u8>> {
        let filters = self.doc.resolve_key(dict, Name::FILTER);
        match filters.as_name() {
            Some(n) => self.doc.name_bytes(n).map(|b| b.to_vec()),
            None => filters
                .as_array()
                .and_then(|a| a.last())
                .and_then(Object::as_name)
                .and_then(|n| self.doc.name_bytes(n))
                .map(|b| b.to_vec()),
        }
    }

    /// An image XObject's samples as the sample loop reads them — **before**
    /// any colour conversion — and the depth they are at, for every final
    /// filter but the two codecs that describe themselves (`/DCTDecode` and
    /// `/JPXDecode`, whose samples come from [`jpeg_samples`] and
    /// [`Self::jpx_samples`]).
    ///
    /// Split out of [`Self::decode_image_at`] so the renderer and
    /// [`crate::Page::images`] read one set of samples through one set of
    /// rules: a fax or a JBIG2 stream is packed one-bit samples in PDF's
    /// polarity whatever `/BitsPerComponent` says, and anything else is the
    /// stream with its filters applied. `geometry` is the dictionary's
    /// width, height and clamped depth.
    fn stream_samples(
        &self,
        dict: &Dict,
        reference: ObjRef,
        last_filter: Option<&[u8]>,
        geometry: (u32, u32, u32),
        name: &[u8],
    ) -> Result<(Vec<u8>, u32), String> {
        let (width, height, bpc) = geometry;
        // CCITT data likewise arrives still coded, and carries its own
        // parameters in /DecodeParms — but unlike a JPEG it decodes to
        // *samples*, one bit per pixel, which is what the image dictionary
        // says it is. So it joins the path below rather than returning its own
        // pixels: `/ImageMask`, `/Decode` and `/ColorSpace` are read once,
        // where they have always been read, and apply to a fax because a fax
        // now arrives there like everything else.
        let fax = matches!(last_filter, Some(b"CCITTFaxDecode") | Some(b"CCF"));
        // JBIG2 takes the same road, and for the same reason (gap 17). Both
        // are bilevel codecs that produce samples rather than pixels, and a
        // scanned page is an `/ImageMask` about as often as it is a DeviceGray
        // image — so composing correctly means arriving where those keys are
        // read, not reimplementing them.
        let jbig2 = matches!(last_filter, Some(b"JBIG2Decode"));
        let data = if jbig2 {
            let raw = self
                .image_input(reference)
                .map_err(|_| "JBIG2Decode".to_string())?;
            // The refusal. A stream whose regions this build cannot decode —
            // the symbol-dictionary lineage an OCR pipeline emits — comes back
            // `None`, and the caller draws the neutral placeholder. Returning
            // the blank page it was composited onto would be indistinguishable
            // from a correct decode of a blank scan.
            self.jbig2_samples(dict, &raw, width, height, name)
                .ok_or_else(|| "JBIG2Decode".to_string())?
        } else if fax {
            let raw = self
                .image_input(reference)
                .map_err(|_| "CCITTFaxDecode".to_string())?;
            self.ccitt_samples(dict, &raw, width, height, name)
        } else {
            // Everything else decodes to raw samples.
            self.samples(reference)
                .map_err(|_| "undecodable".to_string())?
        };
        // 7.4.6 and 7.4.7: both bilevel codecs produce one bit per pixel,
        // whatever the dictionary claims. Producers that omit
        // `/BitsPerComponent` are common and the clamp already reads an absent
        // key as 1; one that writes 8 would otherwise read each row eight
        // times too wide.
        let bpc = if fax || jbig2 { 1 } else { bpc };
        Ok((data, bpc))
    }

    /// A JPEG 2000 codestream's samples, decoded and **not** converted: the
    /// codestream's own components at its own precision, big-endian pairs
    /// past eight bits, with any opacity channel carried apart (8.9.5.4).
    ///
    /// The one decode both [`Self::jpx_image`] and [`crate::Page::images`]
    /// make, under the one ceiling: asked for what `request` names — the
    /// whole image for `Page::images` — its independent parts on
    /// `executor`, with what the decode did counted beside it.
    fn jpx_samples(
        raw: &[u8],
        request: &tinker_pdf_filters::JpxRequest,
        executor: &dyn Executor,
        warnings: &mut Vec<tinker_pdf_filters::Warning>,
    ) -> Result<(tinker_pdf_filters::JpxImage, tinker_pdf_filters::JpxCounts), String> {
        tinker_pdf_filters::jpx_decode_on(raw, &jpx_limits(), request, executor, warnings)
            .map_err(|e| format!("{e:?}"))
    }

    /// Decodes an image from its dictionary, wherever that came from, a
    /// one-bit image of one component held as `form` asks, for a draw that
    /// asks as `ask` does ([`DecodeAsk`]): at the size it is drawn at, its
    /// colours converted as its conversion says, and within what the budget
    /// leaves its samples where the document keeps it; and, beside it, the
    /// frame a codec that can answer a drawn size found the image to be
    /// ([`Frame`]).
    fn decode_image_at(
        &self,
        dict: &Dict,
        reference: tinker_pdf_cos::ObjRef,
        name: &str,
        form: SampleForm,
        ask: DecodeAsk<'_>,
    ) -> Result<(DecodedImage, Option<Frame>), String> {
        let conversion = ask.conversion;
        let dict = dict.clone();
        let name = name.as_bytes();

        let subtype = self
            .doc
            .resolve_key(&dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n));
        if subtype.as_deref() != Some(b"Image".as_slice()) {
            return Err(String::from_utf8_lossy(name).into_owned());
        }

        let int = |key: &[u8]| {
            self.doc
                .resolve_key(&dict, self.name(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = int(b"Width").clamp(0, 1 << 16) as u32;
        let height = int(b"Height").clamp(0, 1 << 16) as u32;
        if width == 0 || height == 0 {
            return Err("empty".to_string());
        }
        let bpc = int(b"BitsPerComponent").clamp(1, 16) as u32;

        // Table 89: /Interpolate asks for the samples to be smoothed when the
        // image is magnified. Read here rather than in either codec branch,
        // because both of those return before the end of this function — which
        // is how /Decode came to be ignored on exactly the two formats whose
        // producers use it most.
        let interpolate = self
            .doc
            .resolve_key(&dict, self.name(b"Interpolate"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.5.2: /Decode remaps each component's sample range. Read here
        // rather than after the codec branches, because both of those used to
        // return before reaching it — so `/Decode [1 0]` on a JPEG or a fax
        // did nothing at all, and the image rendered as its own negative
        // without a word.
        let decode: Vec<(f64, f64)> = self
            .doc
            .resolve_key(&dict, self.name(b"Decode"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default();

        // The final filter decides how the bytes are read.
        let last_filter = self.last_filter(&dict);

        // DCTDecode data comes out of the stream tier still encoded, which is
        // exactly what the JPEG decoder wants.
        if matches!(last_filter.as_deref(), Some(b"DCTDecode") | Some(b"DCT")) {
            // Every filter before the codec, and no further: a producer
            // that deflates its JPEG bytes writes `[/FlateDecode /DCTDecode]`,
            // and handing the undecoded stream to the JPEG decoder made it
            // refuse a perfectly good image.
            let raw = self
                .image_input(reference)
                .map_err(|_| "DCTDecode".to_string())?;
            let named = self.image_space(&dict, name);
            return self.jpeg_image(&raw, &decode, interpolate, named, name, ask);
        }
        if matches!(last_filter.as_deref(), Some(b"JPXDecode")) {
            // Like a JPEG and unlike a fax, a JPX codestream carries its own
            // geometry, precision and colour, so it never joins the generic
            // sample loop below -- 8.9.5.4 is explicit that the dictionary's
            // `/BitsPerComponent` and `/Decode` do not apply to it.
            let raw = self
                .image_input(reference)
                .map_err(|_| "JPXDecode".to_string())?;
            let named = self.image_space(&dict, name);
            return self.jpx_image(&raw, &dict, interpolate, named, name, ask);
        }

        let (data, bpc) = self.stream_samples(
            &dict,
            reference,
            last_filter.as_deref(),
            (width, height, bpc),
            name,
        )?;

        let space = self
            .image_space(&dict, name)
            .unwrap_or(ColorSpace::DeviceGray);
        let n = space.components();

        // 8.9.5.2: an image mask is one bit per sample, painted in the fill
        // colour. Without the fill colour here it reads as black-on-nothing.
        let is_mask = self
            .doc
            .resolve_key(&dict, self.name(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.6.4: colour-key masking. Read before the loop because it is a
        // property of the image and not of a pixel, and applied inside it
        // because it is a test on the *raw* samples.
        let color_key = self.color_key(&dict, n, bpc);

        let layout = SampleLayout {
            width,
            height,
            bpc,
            n,
            space: &space,
            decode: &decode,
            mask: is_mask,
            color_key: color_key.as_deref(),
            conversion,
            work: self.op.work.as_deref(),
        };
        let (rgb, alpha, two_colour) = decoded_pixels(&data, &layout, form);
        // For plates, each sample's tints beside its light (RC-04.12); a
        // stencil's are its fill colour's.
        let plates = ask
            .plates
            .filter(|_| !is_mask)
            .and_then(|plan| plate_samples(&data, &layout, &rgb, plan));

        let image = DecodedImage {
            width,
            height,
            rgb,
            alpha,
            stencil: is_mask,
            interpolate,
            two_colour,
            reduced: None,
            // A stencil is painted in the fill colour, whose family the
            // state carries.
            family: if is_mask {
                tinker_pdf_content::ColorFamily::Other
            } else {
                tinker_pdf_render::color_family(&space)
            },
            soft_masked: false,
            window: None,
            plates,
        };
        Ok((image, None))
    }

    /// 8.9.6.4's colour-key mask: one inclusive range of **raw** sample values
    /// per colour component, before `/Decode`.
    ///
    /// `/Mask` has a second form — a reference to a stencil-mask image — which
    /// is a different feature and is not read here. This returns `None` for it,
    /// and for any array that is not exactly `2 x n` integers inside the depth's
    /// range, because a mask that cannot be read has to leave the image opaque:
    /// masking *more* than the file asked for erases pixels that were in it,
    /// and there is no way to tell from the result that anything went wrong.
    ///
    /// Gap 29 is what this was built for. A PNG `tRNS` naming one fully
    /// transparent colour or one run of palette indices maps onto exactly this
    /// array, which is what lets such a file reach a page as its own compressed
    /// bytes instead of being decoded into an `/SMask`.
    fn color_key(&self, dict: &Dict, n: usize, bpc: u32) -> Option<Vec<(u32, u32)>> {
        let value = self.doc.resolve_key(dict, self.name(b"Mask"));
        let items = value.as_array()?;
        if n == 0 || items.len() != n * 2 {
            return None;
        }

        // "Each integer shall be in the range 0 to 2^BitsPerComponent - 1."
        let ceiling = i64::from((1u32 << bpc.min(16)) - 1);
        let mut ranges = Vec::with_capacity(n);
        for pair in items.chunks_exact(2) {
            let lo = self.doc.resolve(&pair[0]).as_int()?;
            let hi = self.doc.resolve(&pair[1]).as_int()?;
            if lo < 0 || hi < lo || hi > ceiling {
                return None;
            }
            // Both are within `0 ..= ceiling`, which is at most `u32::MAX`.
            ranges.push((lo as u32, hi as u32));
        }
        Some(ranges)
    }

    /// An inline image's samples as its filter chain leaves them (8.9.7), or
    /// the still-coded bytes of a codec that describes itself.
    ///
    /// Split out of [`Self::decode_inline`] so the renderer and
    /// [`crate::Page::images`] run one chain: the one the document would have
    /// built had these bytes been an object.
    fn inline_samples(
        &self,
        dict: &Dict,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<InlineSamples, String> {
        use tinker_pdf_filters::{apply_chain, ChainOutput, ImageCodec, Limits};

        // The filters, in the order they were applied — read here only to name
        // one in a warning, since the chain itself is built by the document.
        let filters_value = self.doc.resolve_key(dict, Name::FILTER);
        let mut filters: Vec<Vec<u8>> = Vec::new();
        if let Some(name) = filters_value.as_name() {
            if let Some(bytes) = self.doc.name_bytes(name) {
                filters.push(bytes.to_vec());
            }
        } else if let Some(items) = filters_value.as_array() {
            for item in items {
                if let Some(bytes) = item.as_name().and_then(|n| self.doc.name_bytes(n)) {
                    filters.push(bytes.to_vec());
                }
            }
        }
        let named = |at: usize| -> String {
            filters.get(at).or_else(|| filters.last()).map_or_else(
                || INLINE_NAME.to_string(),
                |n| String::from_utf8_lossy(n).into_owned(),
            )
        };

        // The chain the document would have built had these bytes been an
        // object. Built there rather than here on purpose: Table 10's
        // predictor defaults, `/EarlyChange` and the per-filter
        // `/DecodeParms` array form then have one implementation instead of
        // two that can drift. This path used to carry the second one, and it
        // passed no predictor at all and hardcoded `/EarlyChange` true.
        let mut chain_warnings = tinker_pdf_cos::WarningSink::new();
        let chain = self.doc.filter_chain(dict, &mut chain_warnings);
        if chain_warnings
            .warnings()
            .iter()
            .any(|w| w.kind == tinker_pdf_cos::WarningKind::FilterUnknown)
        {
            // A filter name this build does not know ends the chain where it
            // appears, so everything after it is still encoded. Reported,
            // rather than sampled as though it were pixels.
            return Err(named(chain.len()));
        }

        // The ceiling every other stream in the document decodes under, not a
        // second number that happens to be smaller. An inline image is
        // attacker-controlled bytes inside a content stream, and a page may
        // hold any number of them, so the bound here is a denial-of-service
        // question rather than a tidiness one — and one that had drifted to
        // half the shared value with nothing naming the relationship.
        let limits = Limits::new(limits::MAX_DECODED_STREAM);
        let report = |warnings: Vec<FilterWarning>| {
            // Ruling 10: what the chain forgave is named, the same way the
            // XObject path names it. An inline image is one object's worth of
            // attacker-controlled bytes inside a content stream, so "it drew"
            // and "it drew cleanly" have to stay apart here too.
            for warning in warnings {
                self.report_damaged_image(INLINE_NAME.as_bytes(), warning);
            }
        };
        // 8.9.7 permits DCT and CCITT inline, and both are ordinary there — a
        // scanned page pasted into a content stream is a fax, and a photograph
        // is a JPEG. Both used to hit a catch-all that returned `Err`, so both
        // drew the grey placeholder and reported a codec this build has in
        // fact had all along.
        let (bytes, fax) = match apply_chain(data, &chain, &limits) {
            Ok(ChainOutput::Bytes(decoded)) => {
                report(decoded.warnings);
                (decoded.data, false)
            }
            Ok(ChainOutput::EncodedImage {
                kind: ImageCodec::Dct,
                data,
                warnings,
            }) => {
                report(warnings);
                return Ok(InlineSamples::Jpeg(data));
            }
            Ok(ChainOutput::EncodedImage {
                kind: ImageCodec::Ccitt,
                data,
                warnings,
            }) => {
                report(warnings);
                // Gap 16's single entry point, not a second call site. A fax
                // decodes to packed one-bit *samples*, so it joins the loop
                // below and gains `/ImageMask`, `/Decode` and `/ColorSpace`
                // with it. `ccitt_decode`'s signature says nothing about the
                // shape of what it returns, so a copy of this made here would
                // have gone on producing the old byte-per-pixel shape with
                // nothing in the build able to notice.
                (
                    self.ccitt_samples(dict, &data, width, height, INLINE_NAME.as_bytes()),
                    true,
                )
            }
            Ok(ChainOutput::EncodedImage {
                kind: ImageCodec::Jpx,
                data,
                warnings,
            }) => {
                report(warnings);
                // 8.9.7 does not list JPXDecode among the inline abbreviations
                // and `/JPXDecode` cannot be spelled in a content stream's
                // dictionary, so this arm is reachable only through a file
                // that writes the full name. It exists anyway: the cost is one
                // arm, and the alternative is a second call site drifting from
                // the XObject path's.
                return Ok(InlineSamples::Jpx(data));
            }
            // JBIG2 stays a gated capability wherever it appears (ruling 2),
            // inline included -- and unlike JPX it has no inline path at all,
            // since `/JBIG2Globals` cannot be an indirect reference inside a
            // content stream.
            Ok(ChainOutput::EncodedImage { .. }) => {
                return Err(named(chain.len().saturating_sub(1)))
            }
            Err(_) => return Err(named(chain.len())),
        };
        Ok(InlineSamples::Samples { bytes, fax })
    }

    /// Decodes an inline image's samples (8.9.7), its colours converted as
    /// `conversion` says, with the frame a JPEG or a codestream reports.
    ///
    /// Separate from [`Self::decode_image_at`] because an inline image has no
    /// object number: its bytes are in hand rather than behind a stream tier,
    /// so the filter chain is run here instead of by the document.
    fn decode_inline(
        &self,
        dict: &Dict,
        data: &[u8],
        request: &ImageRequest,
        conversion: icc::Conversion,
        plates: Option<&PlatePlan>,
    ) -> Result<(DecodedImage, Option<Frame>), String> {
        let int = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.name(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = int(b"Width").clamp(0, 1 << 16) as u32;
        let height = int(b"Height").clamp(0, 1 << 16) as u32;
        if width == 0 || height == 0 {
            return Err("inline".to_string());
        }

        let is_mask = self
            .doc
            .resolve_key(dict, self.name(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);
        // 8.9.6.2: a mask is one bit per sample whatever /BPC claims.
        let bpc = if is_mask {
            1
        } else {
            int(b"BitsPerComponent").clamp(1, 16) as u32
        };

        // Table 93 abbreviates /Interpolate to /I, and the caller has already
        // rewritten the short form, so only the long one is looked for here.
        let interpolate = self
            .doc
            .resolve_key(dict, self.name(b"Interpolate"))
            .as_bool()
            .unwrap_or(false);

        // 8.9.5.2. Read before the chain rather than after it, because the DCT
        // branch below returns its own pixels and would otherwise never see
        // this — which is the mistake the XObject path made until gap 16.
        let decode: Vec<(f64, f64)> = self
            .doc
            .resolve_key(dict, self.name(b"Decode"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default();

        let (bytes, fax) = match self.inline_samples(dict, data, width, height)? {
            InlineSamples::Samples { bytes, fax } => (bytes, fax),
            // The same decoder, through the same helper, as an image XObject:
            // a JPEG returns its own dimensions and its own colour, so it does
            // not join the sample loop below.
            InlineSamples::Jpeg(data) => {
                // An inline image carries no mask (Table 93), so its JPEG is
                // decoded at the size the draw asks. It has no object to be
                // kept under, so no budget reaches it.
                let named = self.inline_space(dict);
                return self.jpeg_image(
                    &data,
                    &decode,
                    interpolate,
                    named,
                    INLINE_NAME.as_bytes(),
                    DecodeAsk {
                        request,
                        conversion,
                        room: None,
                        plates,
                    },
                );
            }
            InlineSamples::Jpx(data) => {
                // The same entry point the XObject path takes, for the same
                // reason gap 16 gave the fax one and gap 08 was reordered
                // behind it: a second call site here would not change any
                // signature, so nothing in the build could notice it drifting
                // from 8.9.5.4's rules about which of the dictionary's claims
                // survive a codestream that describes itself. No mask either,
                // so it too is decoded at the size the draw asks; an inline
                // image is never cached, so its frame is read by no later draw.
                let named = self.inline_space(dict);
                return self.jpx_image(
                    &data,
                    dict,
                    interpolate,
                    named,
                    INLINE_NAME.as_bytes(),
                    DecodeAsk {
                        request,
                        conversion,
                        room: None,
                        plates,
                    },
                );
            }
        };
        // 7.4.6: fax output is one bit per pixel whatever the dictionary
        // claims, exactly as on the XObject path.
        let bpc = if fax { 1 } else { bpc };

        let space = self.inline_space(dict).unwrap_or(ColorSpace::DeviceGray);
        let n = if is_mask { 1 } else { space.components() };

        // An inline image is read with no colour key, and carries no soft
        // mask (Table 93), so a one-bit image of one component is held as its
        // two colours — but as RGB for plates, whose tints sit beside its
        // light a sample at a time.
        let layout = SampleLayout {
            width,
            height,
            bpc,
            n,
            space: &space,
            decode: &decode,
            mask: is_mask,
            color_key: None,
            conversion,
            work: self.op.work.as_deref(),
        };
        let form = if plates.is_some() {
            SampleForm::Rgb
        } else {
            SampleForm::TwoColour
        };
        let (rgb, alpha, two_colour) = decoded_pixels(&bytes, &layout, form);
        let plates = plates
            .filter(|_| !is_mask)
            .and_then(|plan| plate_samples(&bytes, &layout, &rgb, plan));

        Ok((
            DecodedImage {
                width,
                height,
                rgb,
                alpha,
                stencil: is_mask,
                interpolate,
                two_colour,
                reduced: None,
                // A stencil is painted in the fill colour, whose family the
                // state carries.
                family: if is_mask {
                    tinker_pdf_content::ColorFamily::Other
                } else {
                    tinker_pdf_render::color_family(&space)
                },
                soft_masked: false,
                window: None,
                plates,
            },
            None,
        ))
    }

    /// The `/ColorSpace` an image XObject's dictionary names, read as a space
    /// painted in (8.9.5, 8.6.5.6), or `None` where it names none this reads.
    fn image_space(&self, dict: &Dict, name: &[u8]) -> Option<ColorSpace> {
        let space = self.doc.resolve_key(dict, Name::COLOR_SPACE);
        self.parse_space(&space, 0, name)
    }

    /// An inline image's `/ColorSpace` (8.9.7): a device space, a
    /// `/ColorSpace` resource by name, or an `/Indexed` array over one of
    /// those — each remapped as an image XObject's space is (8.6.5.6).
    fn inline_space(&self, dict: &Dict) -> Option<ColorSpace> {
        let space = self.doc.resolve_key(dict, Name::COLOR_SPACE);
        let inline = Reading {
            inline: true,
            ..self.painted()
        };
        self.read_space(&space, 0, INLINE_NAME.as_bytes(), inline)
    }

    /// The `/SMask` of an image, decoded (11.6.5.3).
    fn soft_mask(&self, dict: &Dict) -> Option<DecodedImage> {
        let reference = dict.get_ref(self.name(b"SMask"))?;
        let object = self.doc.get(reference).ok()?;
        let mask = object.as_dict()?.clone();
        // A mask that is itself masked is not a thing the spec allows, and
        // decoding it through the top-level entry point would recurse. Held
        // as RGB, which `apply_soft_mask` reads its grey from. Its samples are
        // opacity in DeviceGray (11.6.5.3), not a colour any intent converts,
        // so it decodes in the initial conversion whatever its image's.
        self.decode_image_at(
            &mask,
            reference,
            "SMask",
            SampleForm::Rgb,
            DecodeAsk {
                request: &ImageRequest::FULL,
                conversion: icc::Conversion::default(),
                room: None,
                plates: None,
            },
        )
        .ok()
        .map(|(image, _)| image)
    }
}

// ---- extraction: the samples before conversion -----------------------------
//
// `crate::Page::images` reads here. Every sample it hands out comes from the
// same four functions the renderer's decode calls — `stream_samples`,
// `jpeg_samples`, `jpx_samples`, `inline_samples` — so the two cannot disagree
// about what an image's samples are. What differs is only what happens next:
// the renderer converts, and this describes.

impl PageResources {
    /// An image XObject as [`crate::PageImage`] reports it, or `None` when
    /// the dictionary is not `/Subtype /Image`.
    pub(crate) fn extract_image(
        &self,
        dict: &Dict,
        reference: ObjRef,
        name: &[u8],
    ) -> Option<crate::PageImage> {
        let subtype = self
            .doc
            .resolve_key(dict, Name::SUBTYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n));
        if subtype.as_deref() != Some(b"Image".as_slice()) {
            return None;
        }
        // One decoder run an entry listed, its masks part of it, as a
        // render's decode of the image is one.
        self.count(Counter::ImagesDecoded);
        Some(self.extract_at(dict, reference, name, 0))
    }

    /// An image dictionary and its stream, described; `depth` is 1 for a mask
    /// or soft mask, which carries no mask of its own (11.6.5.3, 8.9.6.3).
    fn extract_at(
        &self,
        dict: &Dict,
        reference: ObjRef,
        name: &[u8],
        depth: u32,
    ) -> crate::PageImage {
        let (width, height, stencil, bpc) = self.geometry(dict);
        let space = if stencil {
            None
        } else {
            dict.get(Name::COLOR_SPACE)
                .map(|space| self.describe_space(space, false, 0))
        };
        // A mask's and a soft mask's samples are opacity, never colour, so
        // no default is put in for them (11.6.5.3).
        let (substituted, refusals) = if depth == 0 {
            self.painted_image_space(dict, false)
        } else {
            (None, Vec::new())
        };
        let mut image = crate::PageImage {
            reference: Some(reference),
            name: name.to_vec(),
            width,
            height,
            bits_per_component: bpc as u8,
            components: components_of(stencil, space.as_ref()),
            color_space: space,
            substituted_space: substituted,
            decode: self.decode_array(dict),
            stencil,
            samples: Vec::new(),
            codec: crate::SampleCodec::Stream,
            mask: None,
            soft_mask: None,
            placements: Vec::new(),
            refused: None,
            warnings: Vec::new(),
        };
        if width == 0 || height == 0 {
            image.refused = Some("empty".to_string());
            return image;
        }

        let last = self.last_filter(dict);
        let ((), warnings) = self.capturing(|| match last.as_deref() {
            Some(b"DCTDecode" | b"DCT") => {
                image.codec = crate::SampleCodec::Dct;
                let decoded = self
                    .image_input(reference)
                    .map_err(|_| "DCTDecode".to_string())
                    .and_then(|raw| jpeg_samples(&raw));
                match decoded {
                    Ok(jpeg) => take_jpeg(&mut image, jpeg),
                    Err(why) => image.refused = Some(why),
                }
            }
            Some(b"JPXDecode") => {
                image.codec = crate::SampleCodec::Jpx;
                let mut warnings = Vec::new();
                let decoded = self
                    .image_input(reference)
                    .map_err(|_| "JPXDecode".to_string())
                    .and_then(|raw| {
                        Self::jpx_samples(
                            &raw,
                            &tinker_pdf_filters::JpxRequest::default(),
                            &Parallelism::Serial,
                            &mut warnings,
                        )
                    })
                    .map(|(jpx, _)| jpx);
                for warning in &warnings {
                    self.report_damaged_image(name, *warning);
                }
                match decoded {
                    Ok(jpx) => take_jpx(&mut image, jpx),
                    Err(why) => image.refused = Some(why),
                }
            }
            other => {
                image.codec = match other {
                    Some(b"CCITTFaxDecode" | b"CCF") => crate::SampleCodec::CcittFax,
                    Some(b"JBIG2Decode") => crate::SampleCodec::Jbig2,
                    _ => crate::SampleCodec::Stream,
                };
                match self.stream_samples(dict, reference, other, (width, height, bpc), name) {
                    Ok((data, bits)) => take_samples(&mut image, data, bits),
                    Err(why) => image.refused = Some(why),
                }
            }
        });
        image.warnings = warnings;
        name_refusals(&mut image, refusals);

        if depth == 0 {
            let mask_key = self.name(b"Mask");
            // 8.9.6.3 and 8.9.6.4: `/Mask` is a stencil-mask stream or a
            // colour-key *array*, and a reference can name either — so what
            // decides is the object the reference reaches, not that it is one.
            // `color_key` resolves the entry, as the renderer's decode does,
            // so an indirect `[0 0]` is a colour key here as it is there. It
            // used to be taken for a stencil mask that would not open, and
            // dropped, while the render of the same image honoured it.
            let stencil = match dict.get(mask_key) {
                Some(Object::Ref(r)) => self
                    .doc
                    .get(*r)
                    .ok()
                    .filter(|object| object.as_array().is_none())
                    .map(|object| (object, *r)),
                _ => None,
            };
            if let Some((object, r)) = stencil {
                if let Some(mask) = object.as_dict() {
                    image.mask = Some(crate::ImageMask::Stencil(Box::new(self.extract_at(
                        mask,
                        r,
                        b"",
                        depth + 1,
                    ))));
                }
            } else if dict.get(mask_key).is_some() {
                image.mask = self
                    .color_key(dict, usize::from(image.components), bpc)
                    .map(crate::ImageMask::ColorKey);
            }
            if let Some(r) = dict.get_ref(self.name(b"SMask")) {
                let mask = self.doc.get(r).ok();
                if let Some(mask) = mask.as_deref().and_then(Object::as_dict) {
                    image.soft_mask = Some(Box::new(self.extract_at(mask, r, b"", depth + 1)));
                }
            }
        }
        image
    }

    /// An inline image (8.9.7) as [`crate::PageImage`] reports it, or `None`
    /// when its dictionary is not one.
    pub(crate) fn extract_inline(&self, dict: &[u8], data: &[u8]) -> Option<crate::PageImage> {
        let text = expand_inline_abbreviations(dict);
        let mut sink = tinker_pdf_cos::WarningSink::new();
        let parsed = tinker_pdf_cos::parse_object_at(&text, 0, self.doc.names_table(), &mut sink);
        let dict = parsed.object.as_dict()?;
        // Every inline image is a decoder run, as in a render.
        self.count(Counter::ImagesDecoded);

        let (width, height, stencil, bpc) = self.geometry(dict);
        let space = if stencil {
            None
        } else {
            dict.get(Name::COLOR_SPACE)
                .map(|space| self.describe_space(space, true, 0))
        };
        let (substituted, refusals) = self.painted_image_space(dict, true);
        let mut image = crate::PageImage {
            reference: None,
            name: Vec::new(),
            width,
            height,
            bits_per_component: bpc as u8,
            components: components_of(stencil, space.as_ref()),
            color_space: space,
            substituted_space: substituted,
            decode: self.decode_array(dict),
            stencil,
            samples: Vec::new(),
            codec: crate::SampleCodec::Stream,
            mask: None,
            soft_mask: None,
            placements: Vec::new(),
            refused: None,
            warnings: Vec::new(),
        };
        if width == 0 || height == 0 {
            image.refused = Some("empty".to_string());
            return Some(image);
        }
        let ((), warnings) =
            self.capturing(|| match self.inline_samples(dict, data, width, height) {
                Ok(InlineSamples::Samples { bytes, fax }) => {
                    if fax {
                        image.codec = crate::SampleCodec::CcittFax;
                    }
                    take_samples(&mut image, bytes, if fax { 1 } else { bpc });
                }
                Ok(InlineSamples::Jpeg(bytes)) => {
                    image.codec = crate::SampleCodec::Dct;
                    match jpeg_samples(&bytes) {
                        Ok(jpeg) => take_jpeg(&mut image, jpeg),
                        Err(why) => image.refused = Some(why),
                    }
                }
                Ok(InlineSamples::Jpx(bytes)) => {
                    image.codec = crate::SampleCodec::Jpx;
                    let mut warnings = Vec::new();
                    match Self::jpx_samples(
                        &bytes,
                        &tinker_pdf_filters::JpxRequest::default(),
                        &Parallelism::Serial,
                        &mut warnings,
                    ) {
                        Ok((jpx, _)) => take_jpx(&mut image, jpx),
                        Err(why) => image.refused = Some(why),
                    }
                    for warning in &warnings {
                        self.report_damaged_image(INLINE_NAME.as_bytes(), *warning);
                    }
                }
                Err(why) => image.refused = Some(why),
            });
        image.warnings = warnings;
        name_refusals(&mut image, refusals);
        // 8.9.7: an inline image's `/Mask` can only be the colour-key array,
        // since there is nothing inline a reference could name.
        if dict.get(self.name(b"Mask")).is_some() {
            image.mask = self
                .color_key(dict, usize::from(image.components), bpc)
                .map(crate::ImageMask::ColorKey);
        }
        Some(image)
    }

    /// `/Width`, `/Height`, `/ImageMask` and the depth, read as the renderer's
    /// decode reads them: the geometry clamped to what it will allocate, and a
    /// stencil mask one bit a sample whatever `/BitsPerComponent` says
    /// (8.9.6.2).
    fn geometry(&self, dict: &Dict) -> (u32, u32, bool, u32) {
        let int = |key: &[u8]| {
            self.doc
                .resolve_key(dict, self.name(key))
                .as_int()
                .unwrap_or(0)
        };
        let width = int(b"Width").clamp(0, 1 << 16) as u32;
        let height = int(b"Height").clamp(0, 1 << 16) as u32;
        let stencil = self
            .doc
            .resolve_key(dict, self.name(b"ImageMask"))
            .as_bool()
            .unwrap_or(false);
        let bpc = if stencil {
            1
        } else {
            int(b"BitsPerComponent").clamp(1, 16) as u32
        };
        (width, height, stencil, bpc)
    }

    /// `/Decode` as written, in pairs (8.9.5.2).
    fn decode_array(&self, dict: &Dict) -> Vec<(f64, f64)> {
        self.doc
            .resolve_key(dict, self.name(b"Decode"))
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| self.doc.resolve(o).as_number())
                    .collect::<Vec<f64>>()
            })
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default()
    }

    /// A colour space as an image states it, described rather than
    /// evaluated (8.6).
    ///
    /// `inline` admits 8.9.7's one extra spelling: an inline image may name a
    /// space in the page's `/ColorSpace` resources, which an image XObject's
    /// dictionary may not (8.6.3). Depth-capped like the renderer's own
    /// parser, since an `/Indexed` base and a tint alternate are spaces too.
    fn describe_space(&self, object: &Object, inline: bool, depth: u32) -> crate::ImageSpace {
        self.describe(object, inline, depth, None)
    }

    /// An image's `/ColorSpace` in `dict` as this scope paints it: the stated
    /// space with each device space in it the scope remaps put in as its
    /// default (8.6.5.6), or `None` where nothing was put in, so a page with
    /// no default lists what it listed before; and why each default a device
    /// space in it fell back from was refused. A stencil mask has no space.
    fn painted_image_space(
        &self,
        dict: &Dict,
        inline: bool,
    ) -> (Option<crate::ImageSpace>, Vec<String>) {
        let (_, _, stencil, _) = self.geometry(dict);
        let Some(space) = dict.get(Name::COLOR_SPACE).filter(|_| !stencil) else {
            return (None, Vec::new());
        };
        if self.defaults().is_inert() {
            return (None, Vec::new());
        }
        let mut painting = Painting::default();
        let described = self.describe(space, inline, 0, Some(&mut painting));
        (painting.replaced.then_some(described), painting.refused)
    }

    /// What `Page::images` compares to tell one drawing of an image XObject
    /// from another: the space this scope paints it in where it puts a
    /// default in, so one XObject drawn in two spaces is two entries.
    pub(crate) fn image_substitute(&self, dict: &Dict) -> Option<crate::ImageSpace> {
        self.painted_image_space(dict, false).0
    }

    /// The entry this scope's `/ColorSpace` names `device`'s default by, as
    /// written.
    fn default_entry(&self, device: &ColorSpace) -> Option<Object> {
        let key: &[u8] = match device {
            ColorSpace::DeviceGray => b"DefaultGray",
            ColorSpace::DeviceRgb => b"DefaultRGB",
            ColorSpace::DeviceCmyk => b"DefaultCMYK",
            _ => return None,
        };
        let resources = self.resources.as_deref()?;
        let table = self.doc.resolve_key(resources, Name::COLOR_SPACE);
        table.as_dict()?.get(self.name(key)).cloned()
    }

    /// The one describer, as stated or, with `painting`, as this scope paints
    /// it: where the renderer's painted reading puts a default in for a device
    /// space — the space itself, an `/Indexed` base, a separation's alternate
    /// but under `/All` or `/None`, a resource an inline image names — the
    /// default is described as written, never remapped again. An `ICCBased`
    /// space's `/Alternate` is described as stated, since the samples are in
    /// the profile.
    fn describe(
        &self,
        object: &Object,
        inline: bool,
        depth: u32,
        mut painting: Option<&mut Painting>,
    ) -> crate::ImageSpace {
        use crate::ImageSpace;
        let unreadable = |family: &[u8]| ImageSpace::Unreadable {
            family: family.to_vec(),
        };
        if depth > 8 {
            return unreadable(b"");
        }
        let resolved = self.doc.resolve(object);

        if let Some(name) = resolved.as_name() {
            let Some(bytes) = self.doc.name_bytes(name) else {
                return unreadable(b"");
            };
            // The renderer reads a bare `/CalGray` and `/CalRGB` as the
            // device space, so the default it puts in is the space the samples
            // are drawn in; as stated they stay what nothing describes.
            let device = match bytes.as_ref() {
                b"DeviceGray" | b"G" => Some((ColorSpace::DeviceGray, ImageSpace::DeviceGray)),
                b"DeviceRGB" | b"RGB" => Some((ColorSpace::DeviceRgb, ImageSpace::DeviceRgb)),
                b"DeviceCMYK" | b"CMYK" => Some((ColorSpace::DeviceCmyk, ImageSpace::DeviceCmyk)),
                b"CalGray" if painting.is_some() => {
                    Some((ColorSpace::DeviceGray, unreadable(b"CalGray")))
                }
                b"CalRGB" if painting.is_some() => {
                    Some((ColorSpace::DeviceRgb, unreadable(b"CalRGB")))
                }
                _ => None,
            };
            if let Some((device, stated)) = device {
                if let Some(painting) = painting {
                    let defaults = self.defaults();
                    if defaults.for_device(&device).is_some() {
                        if let Some(entry) = self.default_entry(&device) {
                            painting.replaced = true;
                            return self.describe(&entry, false, depth + 1, None);
                        }
                    } else if let Some(reason) = defaults.refusal(&device) {
                        if !painting.refused.iter().any(|seen| seen == reason) {
                            painting.refused.push(reason.to_string());
                        }
                    }
                }
                return stated;
            }
            return match bytes.as_ref() {
                other if inline => {
                    let entry = self.resources.as_deref().and_then(|resources| {
                        let table = self.doc.resolve_key(resources, Name::COLOR_SPACE);
                        table.as_dict()?.get(self.name(other)).cloned()
                    });
                    match entry {
                        // A resource entry is the space itself, so it is read
                        // as an XObject's would be: it cannot name another.
                        Some(entry) => self.describe(&entry, false, depth + 1, painting),
                        None => unreadable(other),
                    }
                }
                other => unreadable(other),
            };
        }

        let Some(items) = resolved.as_array() else {
            return unreadable(b"");
        };
        let Some(family) = items
            .first()
            .and_then(Object::as_name)
            .and_then(|n| self.doc.name_bytes(n))
        else {
            return unreadable(b"");
        };
        let parameters = || {
            items
                .get(1)
                .map(|o| self.doc.resolve(o))
                .and_then(|p| p.as_dict().cloned())
        };
        let triple = |dict: Option<&Dict>, key: &[u8], default: [f64; 3]| {
            dict.and_then(|d| self.numbers(d, key, 3))
                .map_or(default, |v| [v[0], v[1], v[2]])
        };
        match family.as_ref() {
            b"ICCBased" => {
                let stream = items.get(1).and_then(Object::as_objref);
                let header = items.get(1).map(|o| self.doc.resolve(o));
                let dict = header.as_deref().and_then(Object::as_dict);
                let components = dict
                    .and_then(|d| d.get_int(Name::N))
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let alternate = dict
                    .and_then(|d| d.get(self.name(b"Alternate")))
                    .map(|a| Box::new(self.describe(a, inline, depth + 1, None)));
                let profile = stream
                    .and_then(|r| self.decoded(r).ok())
                    .map(Arc::unwrap_or_clone)
                    .unwrap_or_default();
                ImageSpace::Icc {
                    components,
                    profile,
                    alternate,
                }
            }
            b"Indexed" | b"I" => {
                let base = items.get(1).map_or_else(
                    || unreadable(b""),
                    |b| self.describe(b, inline, depth + 1, painting.as_deref_mut()),
                );
                let high = items
                    .get(2)
                    .and_then(|o| self.doc.resolve(o).as_int())
                    .unwrap_or(0)
                    .clamp(0, 255) as u8;
                let lookup = match items.get(3).map(|o| self.doc.resolve(o)) {
                    Some(value) => match value.as_string() {
                        Some(s) => s.bytes.clone(),
                        None => items
                            .get(3)
                            .and_then(Object::as_objref)
                            .and_then(|r| self.decoded(r).ok())
                            .map(Arc::unwrap_or_clone)
                            .unwrap_or_default(),
                    },
                    None => Vec::new(),
                };
                ImageSpace::Indexed {
                    base: Box::new(base),
                    high,
                    lookup,
                }
            }
            b"Separation" => {
                let colorant = separation_name(&self.doc, items).unwrap_or_default();
                // 8.6.6.4's `/All` and `/None` never select their alternate,
                // so the renderer puts no default in for it, and nor does this.
                let selects = !matches!(colorant.as_slice(), b"All" | b"None");
                let alternate = items.get(2).map_or_else(
                    || unreadable(b""),
                    |a| {
                        let painting = painting.as_deref_mut().filter(|_| selects);
                        self.describe(a, inline, depth + 1, painting)
                    },
                );
                ImageSpace::Separation {
                    colorant,
                    alternate: Box::new(alternate),
                }
            }
            b"DeviceN" => ImageSpace::DeviceN {
                colorants: device_n_names(&self.doc, items),
                alternate: Box::new(items.get(2).map_or_else(
                    || unreadable(b""),
                    |a| self.describe(a, inline, depth + 1, painting),
                )),
            },
            b"CalGray" => {
                let dict = parameters();
                ImageSpace::CalGray {
                    white: triple(dict.as_ref(), b"WhitePoint", WHITE_D50),
                    gamma: dict
                        .as_ref()
                        .map(|d| self.doc.resolve_key(d, self.name(b"Gamma")))
                        .and_then(|g| g.as_number())
                        .unwrap_or(1.0),
                }
            }
            b"CalRGB" => {
                let dict = parameters();
                let mut matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
                if let Some(values) = dict.as_ref().and_then(|d| self.numbers(d, b"Matrix", 9)) {
                    matrix.copy_from_slice(&values[..9]);
                }
                ImageSpace::CalRgb {
                    white: triple(dict.as_ref(), b"WhitePoint", WHITE_D50),
                    gamma: triple(dict.as_ref(), b"Gamma", [1.0; 3]),
                    matrix,
                }
            }
            b"Lab" => {
                let dict = parameters();
                let range = dict
                    .as_ref()
                    .and_then(|d| self.numbers(d, b"Range", 4))
                    .map_or([-100.0, 100.0, -100.0, 100.0], |v| [v[0], v[1], v[2], v[3]]);
                ImageSpace::Lab {
                    white: triple(dict.as_ref(), b"WhitePoint", WHITE_D50),
                    range,
                }
            }
            other => unreadable(other),
        }
    }
}

/// The colorant a `/Separation` space array names (8.6.6.4): its second
/// element, resolved, as a name's bytes — `/All` and `/None` among them,
/// which only a caller knowing the family tells from a colorant — or `None`
/// where that element is no name.
///
/// **The one reading of a space's colorant names.** `Page::images`'
/// describer, the renderer's `parse_space` and a page's spot-colorant
/// inventory (`crate::separations`) all take a space's names from here and
/// from [`device_n_names`], so the three cannot come to disagree about what a
/// space is called.
pub(crate) fn separation_name(doc: &CosDocument, items: &[Object]) -> Option<Vec<u8>> {
    let name = doc.resolve(items.get(1)?).as_name()?;
    doc.name_bytes(name).map(|bytes| bytes.to_vec())
}

/// The colorants a `/DeviceN` space array's names array gives (8.6.6.5), in
/// its order: each element resolved, and kept where it is a name — at most
/// [`limits::MAX_ARRAY_LEN`] of them — as [`separation_name`] reads one.
pub(crate) fn device_n_names(doc: &CosDocument, items: &[Object]) -> Vec<Vec<u8>> {
    let Some(names) = items.get(1).map(|names| doc.resolve(names)) else {
        return Vec::new();
    };
    let Some(names) = names.as_array() else {
        return Vec::new();
    };
    names
        .iter()
        .take(limits::MAX_ARRAY_LEN)
        .filter_map(|name| {
            let name = doc.resolve(name).as_name()?;
            doc.name_bytes(name).map(|bytes| bytes.to_vec())
        })
        .collect()
}

/// What an image's samples put on the plates of a separations render
/// (RC-04.12; `docs/design/overprint-separations.md`, "Images on plates"),
/// chosen once an image by its space and the render's plan: the tints each
/// sample's colour puts on every channel of the plates, as a flat colour of
/// the same space and components puts them ([`colorant_tints`],
/// [`PlatePlan::sample_tints`]) — so a sample takes the road a fill takes,
/// and a space whose names the plan cannot all place reverts to its alternate
/// as a fill's does.
///
/// DeviceCMYK's tints are its components, each `round(255 × c)` after
/// `/Decode`, onto the process plates. Every other space's are worked out
/// once for each distinct sample, by its raw values, as the light of a
/// `/DeviceN` image is remembered (`Remembered`), up to
/// [`REMEMBERED_COLOURS`] of them, and each sample past them by itself.
pub(crate) struct ImageTints<'p> {
    plan: &'p PlatePlan,
    spots: tinker_pdf_raster::Spots,
    /// The channels the image marks when it overprints
    /// ([`PlatePlan::marks`]): its space's colorants at its initial colour,
    /// which name every colorant each of its colours names, the non-zero
    /// mode never applying to an image (8.6.7).
    marks: tinker_pdf_raster::Channels,
    /// Tints a sample: the four process plates and the plan's spots.
    channels: usize,
    /// Whether the space is DeviceCMYK, whose components are its tints.
    cmyk: bool,
    /// Each distinct sample's tints, by its raw values.
    remembered: HashMap<Vec<u32>, Vec<u8>>,
    /// One sample's components, kept between samples.
    components: Vec<f64>,
}

/// What an image in `space` puts on the plates of a render by `plan`: its
/// tints ([`ImageTints`]) where its colours name colorants, nothing at all
/// where they name only `/None` (8.6.6.4), and `None` — its light, which the
/// renderer turns into ink — where they name none: a device space but
/// DeviceCMYK, a CIE-based or ICC space, and an `/Indexed` one over them.
pub(crate) enum ImagePlating<'p> {
    /// Each sample's tints.
    Tints(ImageTints<'p>),
    /// The image has no effect on the page.
    Nothing,
}

impl<'p> ImagePlating<'p> {
    /// The plating of an image in `space` on a render by `plan`, or `None`
    /// where its samples are its light.
    pub(crate) fn of(space: &ColorSpace, plan: &'p PlatePlan) -> Option<ImagePlating<'p>> {
        // The space's own initial colour, which names the colorants every
        // colour of it names.
        let initial = space.initial();
        let tints = colorant_tints(space, &initial, (255, 255, 255))?;
        if tints.kind == ColorantKind::None {
            return Some(ImagePlating::Nothing);
        }
        let spots = plan.spots();
        Some(ImagePlating::Tints(ImageTints {
            plan,
            spots,
            marks: plan.marks(&tints, spots, false),
            channels: 4 + spots.count(),
            cmyk: *space == ColorSpace::DeviceCmyk,
            remembered: HashMap::new(),
            components: Vec::new(),
        }))
    }
}

impl ImageTints<'_> {
    /// Tints a sample.
    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    /// The channels the image marks when it overprints.
    pub(crate) fn marks(&self) -> tinker_pdf_raster::Channels {
        self.marks
    }

    /// Appends the tints of the sample whose raw values are `values`, of
    /// `bpc` bits, in `space` under `decode`, its colour as light `light`:
    /// `channels` bytes.
    pub(crate) fn push(
        &mut self,
        space: &ColorSpace,
        decode: &[(f64, f64)],
        values: &[u32],
        bpc: u32,
        light: (u8, u8, u8),
        out: &mut Vec<u8>,
    ) {
        let max = f64::from((1u32 << bpc.min(16)) - 1);
        let start = out.len();
        out.resize(start + self.channels, 0);
        let Some(slot) = out.get_mut(start..) else {
            return;
        };
        if self.cmyk {
            for (c, tint) in slot.iter_mut().enumerate().take(4) {
                let value = values.get(c).copied().unwrap_or(0);
                *tint = tint_byte(component(space, decode, c, value, max));
            }
            return;
        }
        if let Some(known) = self.remembered.get(values) {
            slot.copy_from_slice(known);
            return;
        }
        self.components.clear();
        self.components.extend(
            (0..values.len())
                .map(|c| component(space, decode, c, values.get(c).copied().unwrap_or(0), max)),
        );
        let colorants = colorant_tints(space, &self.components, light);
        self.plan
            .sample_tints(colorants.as_ref(), light, self.spots, slot);
        if self.remembered.len() < REMEMBERED_COLOURS {
            self.remembered.insert(values.to_vec(), slot.to_vec());
        }
    }
}

/// A `/Separation` (`separation`) or `/DeviceN` space array's colorants as
/// the colour model keeps them (8.6.6.4, 8.6.6.5): [`separation_name`]'s one
/// or [`device_n_names`]' each, `/None` told apart in both and a
/// `/Separation`'s `/All` — a `/DeviceN` may not name `/All`, so a colorant of
/// that name there is one. Empty where a `/Separation` names nothing.
pub(crate) fn colorant_names(
    doc: &CosDocument,
    separation: bool,
    items: &[Object],
) -> Vec<Colorant> {
    let read = |name: Vec<u8>| match name.as_slice() {
        b"None" => Colorant::None,
        b"All" if separation => Colorant::All,
        _ => Colorant::Named(name),
    };
    if separation {
        return separation_name(doc, items).map(read).into_iter().collect();
    }
    device_n_names(doc, items).into_iter().map(read).collect()
}

/// What a `/DeviceN` space array's attributes dictionary, its fifth element,
/// says about its colorants (8.6.6.5, Tables 70 and 71), read but not
/// evaluated: no space in it is parsed and no stream decoded.
pub(crate) struct DeviceNAttributes {
    /// `/Subtype /NChannel` (PDF 1.6): each component is evaluated by itself,
    /// and `/Process` names the process components.
    pub(crate) n_channel: bool,
    /// `/Colorants`: each colorant's own `/Separation` space array, by the
    /// name its key gives, in the dictionary's order — at most
    /// [`limits::MAX_ARRAY_LEN`] of them.
    pub(crate) colorants: Vec<(Vec<u8>, Object)>,
    /// `/Process`: its `/ColorSpace` as written, and the names `/Components`
    /// gives that space's components, in their order.
    pub(crate) process: Option<(Object, Vec<Vec<u8>>)>,
}

impl DeviceNAttributes {
    /// Whether `name` is a process component of an `NChannel` space: one
    /// `/Process` names. 8.6.6.5 has `/Process` read only under
    /// `/Subtype /NChannel` — a plain `/DeviceN` supports only what it did
    /// before PDF 1.6 — so a plain one's names are never process names here;
    /// the four 8.6.6.4 reserves are their caller's to tell.
    pub(crate) fn is_process_component(&self, name: &[u8]) -> bool {
        self.n_channel
            && self
                .process
                .as_ref()
                .is_some_and(|(_, components)| components.iter().any(|c| c == name))
    }
}

/// A `/DeviceN` space array's attributes (8.6.6.5), or `None` where it has
/// no fifth element or that element is no dictionary.
pub(crate) fn device_n_attributes(
    doc: &CosDocument,
    items: &[Object],
) -> Option<DeviceNAttributes> {
    let attributes = doc.resolve(items.get(4)?);
    let attributes = attributes.as_dict()?;
    let name_of = |object: &Object| -> Option<Vec<u8>> {
        let name = doc.resolve(object).as_name()?;
        doc.name_bytes(name).map(|bytes| bytes.to_vec())
    };
    let n_channel = attributes
        .get(Name::SUBTYPE)
        .and_then(name_of)
        .is_some_and(|subtype| subtype == b"NChannel");
    let table = doc.resolve_key(attributes, doc.intern(b"Colorants"));
    let colorants = table
        .as_dict()
        .into_iter()
        .flat_map(Dict::iter)
        .take(limits::MAX_ARRAY_LEN)
        .filter_map(|(key, value)| {
            let name = doc.name_bytes(*key)?;
            Some((name.to_vec(), value.clone()))
        })
        .collect();
    let process = doc.resolve_key(attributes, doc.intern(b"Process"));
    let process = process.as_dict().and_then(|process| {
        let space = process.get(Name::COLOR_SPACE)?.clone();
        let components = doc.resolve_key(process, doc.intern(b"Components"));
        let names = components
            .as_array()
            .into_iter()
            .flatten()
            .take(limits::MAX_ARRAY_LEN)
            .filter_map(name_of)
            .collect();
        Some((space, names))
    });
    Some(DeviceNAttributes {
        n_channel,
        colorants,
        process,
    })
}

/// Adds to an image's warnings why each default it fell back from was
/// refused, each once and after what its decoder tolerated (ruling 10).
fn name_refusals(image: &mut crate::PageImage, refusals: Vec<String>) {
    for reason in refusals {
        if !image.warnings.contains(&reason) {
            image.warnings.push(reason);
        }
    }
}

/// Values a pixel of an image with this space, as far as it says: one for a
/// stencil mask and for a space nothing describes, which is the count the
/// renderer's own fallback to grey reads the samples with.
fn components_of(stencil: bool, space: Option<&crate::ImageSpace>) -> u8 {
    if stencil {
        return 1;
    }
    space.and_then(crate::ImageSpace::components).unwrap_or(1)
}

/// Keeps as many samples as the geometry lays out and no more (8.9.3): rows
/// of `width × components × bits`, each padded to a byte.
fn take_samples(image: &mut crate::PageImage, mut data: Vec<u8>, bits: u32) {
    image.bits_per_component = bits as u8;
    let row_bits = u64::from(image.width)
        .saturating_mul(u64::from(image.components))
        .saturating_mul(u64::from(bits));
    let wanted = row_bits.div_ceil(8).saturating_mul(u64::from(image.height));
    if let Ok(wanted) = usize::try_from(wanted) {
        data.truncate(wanted);
    }
    image.samples = data;
}

/// A JPEG's own geometry and samples, which describe the image rather than
/// the dictionary.
fn take_jpeg(image: &mut crate::PageImage, jpeg: tinker_pdf_filters::JpegImage) {
    image.width = jpeg.width;
    image.height = jpeg.height;
    image.components = jpeg_components(jpeg.color);
    take_samples(image, jpeg.data, 8);
}

/// A JPEG 2000 codestream's own geometry, components, precision and
/// samples (8.9.5.4).
fn take_jpx(image: &mut crate::PageImage, jpx: tinker_pdf_filters::JpxImage) {
    image.width = jpx.width;
    image.height = jpx.height;
    image.components = jpx.components;
    let bits = if jpx.precision > 8 { 16 } else { 8 };
    take_samples(image, jpx.samples, bits);
}

/// Applies a soft mask's luminance as the image's per-sample opacity
/// (11.6.5.3).
///
/// The mask has its own dimensions and need not match the image's, so it is
/// sampled by position rather than by index — a 2x2 mask over a 512x512 image
/// is legal and common, because a mask only has to carry as much detail as its
/// gradient needs. Both are held as RGB: an image with a soft mask, and the
/// mask, are decoded as [`SampleForm::Rgb`].
fn apply_soft_mask(image: &mut DecodedImage, mask: &DecodedImage) {
    if image.width == 0 || image.height == 0 || mask.width == 0 || mask.height == 0 {
        return;
    }

    let samples = (image.width as usize).saturating_mul(image.height as usize);
    let mut alpha = Vec::with_capacity(samples);

    for y in 0..image.height as usize {
        // Nearest-neighbour: a mask is a smooth ramp far more often than not,
        // and interpolating it would cost more than it buys.
        let my = y * mask.height as usize / image.height as usize;
        for x in 0..image.width as usize {
            let mx = x * mask.width as usize / image.width as usize;
            let at = (my * mask.width as usize + mx) * 3;
            // The mask is greyscale, so any channel is its value.
            let value = mask.rgb.get(at).copied().unwrap_or(255);

            // An image that already had alpha keeps the more opaque
            // constraint of the two rather than losing one of them.
            let existing = image
                .alpha
                .get(y * image.width as usize + x)
                .copied()
                .unwrap_or(255);
            alpha.push(((u16::from(existing) * u16::from(value)) / 255) as u8);
        }
    }

    image.alpha = alpha;
}

#[cfg(test)]
thread_local! {
    /// Calls of [`read_bits`] on this thread: what
    /// `one_bit_samples_expand_through_the_table` counts.
    static BITS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Reads `bits` bits starting at bit offset `at`, big-endian.
fn read_bits(data: &[u8], at: usize, bits: u32) -> u32 {
    #[cfg(test)]
    BITS_READ.with(|count| count.set(count.get() + 1));
    let mut value = 0u32;
    for i in 0..bits.min(16) {
        let index = at + i as usize;
        let byte = data.get(index / 8).copied().unwrap_or(0);
        let bit = (byte >> (7 - (index % 8))) & 1;
        value = (value << 1) | u32::from(bit);
    }
    value
}

/// How many conversions one image remembers at once.
///
/// A fixed table rather than a cap on anything a document states: it bounds
/// what [`Remembered`] holds, 4 096 slots of a key and a colour each, and a
/// picture with more colours than that converts some of them more than once.
const REMEMBERED_COLOURS: usize = 4096;

/// One image's colour conversions, remembered by the raw samples that asked
/// for them.
///
/// An image reaches [`ColorSpace::to_rgb`] once a pixel, and for a space other
/// than the device spaces that runs a tint transform, a profile or a power —
/// a `/DeviceN` tint transform sampled across eight colorants weighs 256
/// corners of its table (7.10.2), and an `/ICCBased` profile interpolates a
/// lookup table of its own. A picture repeats its colours, so a conversion is
/// remembered by the samples it was asked for, in a slot those samples choose.
/// The slot keeps the samples beside the answer, so two colours that share a
/// slot convert again rather than answer for each other: every pixel is the
/// one `to_rgb` gives, and only the work is shared.
///
/// *Added on review of lane 5C, 3 October 2026*, when a sampled function of
/// several inputs began to be read across all of them: one evaluation of an
/// eight-input table costs what 7.10.2 makes it cost, and this is what keeps
/// an image from paying it at every pixel of a colour it has already drawn.
struct Remembered {
    slots: Vec<Option<Conversion>>,
}

/// One remembered conversion: the samples, packed by [`Remembered::key`], and
/// the colour `to_rgb` gave them.
type Conversion = (u128, (u8, u8, u8));

impl Remembered {
    /// A table for an image of `n` components of `bpc` bits each in `space`,
    /// or `None` where it would not pay or the samples do not fit one key —
    /// a device space converts in a few operations, and a key holds eight
    /// samples of sixteen bits.
    fn for_image(space: &ColorSpace, n: usize, bpc: u32) -> Option<Self> {
        let cheap = matches!(
            space,
            ColorSpace::DeviceGray
                | ColorSpace::DeviceRgb
                | ColorSpace::DeviceCmyk
                | ColorSpace::Approximated { .. }
                | ColorSpace::Pattern { .. }
        );
        (!cheap && n <= 8 && bpc <= 16).then(|| Self {
            slots: vec![None; REMEMBERED_COLOURS],
        })
    }

    /// Adds the `index`th sample of a pixel to its key. A sample is at most
    /// sixteen bits — [`read_bits`] reads no more — and a table exists only
    /// for at most eight of them, so nothing is shifted out.
    fn key(key: u128, index: usize, sample: u32) -> u128 {
        if index >= 8 {
            return key;
        }
        key | (u128::from(sample & 0xFFFF) << (16 * index))
    }

    /// The colour the samples `key` stand for, converted by `convert` unless
    /// this table holds it already.
    fn convert(&mut self, key: u128, convert: impl FnOnce() -> (u8, u8, u8)) -> (u8, u8, u8) {
        let at = Self::slot(key);
        if let Some(Some((held, colour))) = self.slots.get(at) {
            if *held == key {
                return *colour;
            }
        }
        let colour = convert();
        if let Some(slot) = self.slots.get_mut(at) {
            *slot = Some((key, colour));
        }
        colour
    }

    /// The slot a key's colour is kept in: Fibonacci hashing of the key's two
    /// halves folded together, shifted to leave as many bits as the table is
    /// a power of two.
    fn slot(key: u128) -> usize {
        let folded = (key as u64) ^ ((key >> 64) as u64);
        let shift = 64 - REMEMBERED_COLOURS.trailing_zeros();
        (folded.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
    }
}

/// Components read as a colour by how many of them there are.
///
/// 11.6.5.2 puts `/BC` in the mask group's own colour space, which a file may
/// simply not declare. One component is grey, three RGB, four CMYK — the same
/// reading the interpreter applies to an `sc` whose space it could not
/// resolve, and the only one available without a declaration.
fn by_component_count(components: &[f64]) -> (u8, u8, u8) {
    let space = match components.len() {
        1 => ColorSpace::DeviceGray,
        4 => ColorSpace::DeviceCmyk,
        _ => ColorSpace::DeviceRgb,
    };
    space.to_rgb(components)
}

/// A soft mask's `/TR` sampled to 256 entries (11.6.5.2).
///
/// Evaluated once here rather than per pixel: the function may be a Type 4
/// PostScript calculator, and running one for every pixel of a page-sized
/// mask is minutes of work. Its domain and range are both `[0 1]` by
/// definition, so 256 samples reproduce every value an 8-bit mask can hold
/// with no interpolation at all — the table is exact, not an approximation.
fn sample_transfer(function: &Function) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let input = f64::from(i as u16) / 255.0;
        let value = function.eval(&[input]).first().copied().unwrap_or(input);
        // A function may return anything; the mask cannot.
        let value = if value.is_finite() { value } else { input };
        *slot = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    table
}

/// A JPEG's own colour for `pixels` pixels of its samples, onto the end of
/// `rgb`: a grey replicated to three, RGB as it is, CMYK through
/// `DeviceCmyk`'s conversion.
fn jpeg_rgb(color: JpegColor, samples: &[u8], pixels: usize, rgb: &mut Vec<u8>) {
    rgb.reserve(pixels.saturating_mul(3));
    match color {
        JpegColor::Gray => {
            for i in 0..pixels {
                let v = samples.get(i).copied().unwrap_or(0);
                rgb.extend_from_slice(&[v, v, v]);
            }
        }
        JpegColor::Rgb => rgb.extend_from_slice(samples),
        JpegColor::Cmyk | JpegColor::CmykInverted => {
            let space = ColorSpace::DeviceCmyk;
            for i in 0..pixels {
                let at = |c: usize| f64::from(samples.get(i * 4 + c).copied().unwrap_or(0)) / 255.0;
                let (r, g, b) = space.to_rgb(&[at(0), at(1), at(2), at(3)]);
                rgb.extend_from_slice(&[r, g, b]);
            }
        }
    }
}

/// Which glyph a substituted face draws for a code, and how its reading
/// reached it ([`substitute_choice`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Substituted {
    /// The glyph, in the face's own numbering.
    pub glyph: u16,
    /// How the reading reached it.
    pub route: Route,
}

/// How a substituted face's reading reached a glyph
/// (`docs/design/fallback-faces.md`, `docs/design/symbol-faces.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// A mapping the face states: its `cmap` by a character or by the code,
    /// its glyph names, or its charset by a name the encoding gives.
    Stated,
    /// A composite font's CID through the face's own charset, the face
    /// CID-keyed by the font's character collection: the first route.
    Keyed,
    /// No reading reached a glyph: `.notdef`, which the renderer reports.
    Unreached,
    /// CID 0, which is `.notdef` by definition and reports nothing.
    Notdef,
    /// A face with nothing to be read by — no `cmap`, no glyph names, no
    /// `ROS` — read as an embedded program of its shape is, by the glyph
    /// number the code or its CID gives: a guess the face does not state.
    Index {
        /// Whether the renderer reports the glyph unresolved.
        reported: bool,
    },
}

impl Substituted {
    /// Whether the renderer reports this glyph unresolved.
    pub(crate) fn reported(self) -> bool {
        matches!(
            self.route,
            Route::Unreached | Route::Index { reported: true }
        )
    }
}

/// Which glyph of a **substituted** face — a host's or a bundled one, never
/// the document's own program — a code selects
/// (`docs/design/fallback-faces.md`): `sfnt` the face's table directory
/// where it has one, `cff` its CFF outlines where they are CFF.
///
/// A composite font's code reaches a glyph only through the CID its encoding
/// CMap maps it to, by the first of four routes that reaches one: the face's
/// own charset, where it is CID-keyed in the font's collection; the
/// collection's inverted `Uni*-UTF32` CMaps ([`collection_character`]) to a
/// character and the face's `cmap`, in the face's vertical form
/// ([`vertical_form`]) where the font writes vertically or the CID is a
/// vertical variant; `/ToUnicode`, where it names one character; and
/// otherwise `.notdef`, reported. **Never the CID as a glyph index** (ruling
/// 23): an `Identity`-ordered font with no `/ToUnicode` draws `.notdef` and
/// reports even where the face is the document's own. A face with neither a
/// `cmap` nor a `ROS` has no characters to be read by — the synthetic face
/// the corpus measures with, the test faces — and keeps the reading an
/// embedded program gets.
///
/// A simple font's code takes the two readings a face with a `cmap` admits —
/// its character, and the code as the `cmap` reads a code, in
/// [`simple_glyph_by_cmap`]'s order — and otherwise draws `.notdef`,
/// reported, never the code as a glyph index; a face with no `cmap` keeps
/// that last resort. A CFF-flavoured face is read by the names the font's
/// encoding gives, as an embedded CFF is ([`cff_glyph`]).
///
/// A simple font named Symbol or ZapfDingbats
/// ([`cos_font::Font::built_in_encoding`]) takes neither: each code is read
/// by its glyph name — the document's, else the built-in encoding's —
/// through [`substitute_glyph`]'s route over `program`: the face's own names,
/// the name's Unicode value through its Unicode subtable, then the code
/// through a `(3,0)` subtable (`docs/design/symbol-faces.md`); and a code
/// none reaches draws `.notdef`, reported. A face with no names to be read by
/// and no `cmap` keeps the readings above, as it does for a composite font.
///
/// A function of the face and the font and nothing else, so the renderer
/// that draws through it and the save door that embeds what it drew
/// (`docs/design/font-embedding.md`) read the face one way.
pub(crate) fn substitute_choice(
    program: &[u8],
    sfnt: Option<&Sfnt<'_>>,
    cff: Option<&Cff<'_>>,
    font: &cos_font::Font,
    code: u32,
) -> Substituted {
    /// `cmap`.
    const CMAP: u32 = u32::from_be_bytes(*b"cmap");
    let stated = |glyph: u16| Substituted {
        glyph,
        route: Route::Stated,
    };
    let unreached = Substituted {
        glyph: 0,
        route: Route::Unreached,
    };
    let mapped = sfnt.filter(|sfnt| sfnt.table(CMAP).is_some());
    if font.kind() != cos_font::FontKind::Type0 {
        if let Some(encoding) = font.built_in_encoding() {
            // A face holding glyph names — a CFF charset, or a `post` that
            // names glyphs — or a `cmap` is read by the route.
            let named = cff.is_some() || sfnt.is_some_and(names_glyphs);
            if named || mapped.is_some() {
                let reached = u8::try_from(code).ok().and_then(|byte| {
                    substitute_glyph(program, encoding, font.symbolic_glyph_name(code), byte)
                });
                return reached.map_or(unreached, |(glyph, _)| stated(glyph));
            }
        }
        if let Some(cff) = cff {
            return cff_glyph(cff, font, code).map_or(unreached, stated);
        }
        if let Some(sfnt) = mapped {
            return simple_glyph_by_cmap(sfnt, font, code).map_or(unreached, stated);
        }
        // No `cmap` to read the code by: 9.6.6.4's closing guess, the code
        // as the glyph, which an embedded program of this shape gets too.
        return Substituted {
            glyph: sfnt.and_then(|_| u16::try_from(code).ok()).unwrap_or(0),
            route: Route::Index { reported: false },
        };
    }

    let keyed = cff.filter(|cff| cff.is_cid());
    if mapped.is_none() && keyed.is_none() {
        // No characters to be read by: the reading an embedded program of
        // this shape gets, reported where that reading reports.
        return match cff {
            Some(cff) => match cff_glyph(cff, font, code) {
                Some(glyph) => Substituted {
                    glyph,
                    route: Route::Index { reported: false },
                },
                None => Substituted {
                    glyph: 0,
                    route: Route::Index { reported: true },
                },
            },
            None => {
                let cid = font.cid_of(code);
                let glyph = font.gid_for_cid(cid);
                // CID 0 is `.notdef` by definition and reports nothing; any
                // other CID resolving to glyph 0 is one the font lacks.
                Substituted {
                    glyph,
                    route: Route::Index {
                        reported: glyph == 0 && cid != 0,
                    },
                }
            }
        };
    }

    // The CID the document named; a code its CMap does not map, or a table
    // this build left out, names none, and none is guessed.
    let Some(cid) = font.mapped_cid(code) else {
        return unreached;
    };
    // CID 0 is `.notdef` by definition, and reports nothing.
    if cid == 0 {
        return Substituted {
            glyph: 0,
            route: Route::Notdef,
        };
    }
    let collection = font.character_collection();

    // 1. The face's own charset, where it is keyed by the font's collection.
    if let (Some(cff), Some(collection)) = (keyed, collection) {
        let same = cff.registry_ordering()
            == Some((collection.registry.as_str(), collection.ordering.as_str()));
        if same {
            if let Some(glyph) = cff.gid_for_cid(cid).filter(|glyph| *glyph != 0) {
                return Substituted {
                    glyph,
                    route: Route::Keyed,
                };
            }
        }
    }
    if let Some(sfnt) = mapped {
        // 2. The collection's character, through the face's `cmap`.
        if let Some(found) = collection.and_then(|collection| {
            collection_character(
                &collection.registry,
                &collection.ordering,
                font.is_vertical(),
                cid,
            )
        }) {
            for c in found.characters.into_iter().flatten() {
                if let Some(glyph) = sfnt.glyph_for_char(c).filter(|glyph| *glyph != 0) {
                    return stated(if found.vertical {
                        vertical_form(sfnt, glyph)
                    } else {
                        glyph
                    });
                }
            }
        }
        // 3. `/ToUnicode`, where it names one character.
        let single = font.to_unicode_text(code).and_then(|text| {
            let mut chars = text.chars();
            let first = chars.next()?;
            chars.next().is_none().then_some(first)
        });
        if let Some(glyph) = single
            .and_then(|c| sfnt.glyph_for_char(c))
            .filter(|glyph| *glyph != 0)
        {
            return stated(if font.is_vertical() {
                vertical_form(sfnt, glyph)
            } else {
                glyph
            });
        }
    }
    // 4. Nothing reaches it.
    unreached
}

/// The glyph a simple font's code reaches through an sfnt's `cmap`, by the
/// two readings a `cmap` admits: the code's character — `/ToUnicode`'s, or
/// its encoding's — and the code itself as the `cmap` reads a code, which
/// for a `(3,0)` symbol subtable is the `F0xx` private-use block
/// `glyph_for_char` maps it into (9.6.6.4). `None` where neither reaches a
/// glyph other than `.notdef`.
///
/// **A symbolic font asks its code first.** Its `cmap` is keyed by the codes,
/// and the character a code's `/ToUnicode` names is another code of the same
/// table: an office suite's subset numbers its glyphs in the order the
/// document used them, so the space can be code 3 while code 32, which
/// U+0020 reaches through that table, is a letter. Asked the character first,
/// every such space drew the letter, and both validators read the letter's
/// advance against the space's width (MS-14). A table keyed by Unicode
/// answers the code with nothing, and the character still decides there.
pub(crate) fn simple_glyph_by_cmap(
    sfnt: &Sfnt<'_>,
    font: &cos_font::Font,
    code: u32,
) -> Option<u16> {
    let text = font.text_of(code);
    let ch = text.chars().next();
    let by_text = || ch.and_then(|c| sfnt.glyph_for_char(c)).filter(|g| *g != 0);
    let by_code = || {
        char::from_u32(code)
            .and_then(|c| sfnt.glyph_for_char(c))
            .filter(|g| *g != 0)
    };
    if font.is_symbolic() {
        by_code().or_else(by_text)
    } else {
        by_text().or_else(by_code)
    }
}

/// Whether an sfnt's `post` table names its glyphs: versions 1.0, 2.0 and 2.5
/// give each glyph a name, and version 3.0, like a face with no `post`, names
/// none — the face a route by glyph name has nothing in to read.
fn names_glyphs(sfnt: &Sfnt<'_>) -> bool {
    /// `post`.
    const POST: u32 = u32::from_be_bytes(*b"post");
    sfnt.table(POST)
        .and_then(|post| post.get(..4))
        .is_some_and(|version| matches!(version, [0, 1, 0, 0] | [0, 2, 0, 0] | [0, 2, 0x50, 0]))
}

/// `glyph`'s vertical form in `sfnt` (`docs/design/fallback-faces.md`): the
/// face's `vrt2` substitution where its `GSUB` has one for the Han script or
/// its default, else its `vert`, and the glyph itself where it has neither —
/// drawn upright, the line advancing by `/W2` as it does for any glyph.
fn vertical_form(sfnt: &Sfnt<'_>, glyph: u16) -> u16 {
    use tinker_pdf_shape::{Buffer, Layout, Limits, Tag};

    let layout = Layout::parse(sfnt);
    let Some(gsub) = layout.gsub() else {
        return glyph;
    };
    let script = Tag::new(b"hani");
    let Some(lookups) = [b"vrt2", b"vert"]
        .into_iter()
        .map(|feature| gsub.lookups_for(script, None, &[Tag::new(feature)]))
        .find(|lookups| !lookups.is_empty())
    else {
        return glyph;
    };
    let mut buffer = Buffer::from_glyphs(&[glyph]);
    let _ = layout.substitute(&mut buffer, &lookups, Limits::DEFAULT);
    buffer.glyphs().first().map_or(glyph, |shaped| shaped.glyph)
}

/// Which glyph of a CFF program a character code selects.
///
/// The character code is not a glyph index and never was. Until August 2026
/// this function did not exist and its two callers used the code as one, so a
/// document embedding a Type 1 face as `/FontFile3 /Subtype /Type1C` — how
/// they have been embedded since PDF 1.2 — drew whatever glyph sat at that
/// position, or nothing at all when the font was subsetted small enough that
/// the position did not exist.
///
/// The order below is the one real files need, because real files omit
/// things. Each step is tried in turn and the first that names a glyph in
/// this font wins:
///
/// 1. the document's `/Encoding` gives a name, and the charset gives the
///    glyph — 9.6.6 makes the PDF's encoding win over the font's;
/// 2. the font program's own built-in encoding gives the glyph directly;
/// 3. the standard encoding's name for the code, through the charset;
/// 4. nothing, which the caller turns into `.notdef` plus a report.
///
/// Step 1 is tried whenever the dictionary named an encoding *and* that name
/// reaches a glyph. A name that reaches nothing falls through to step 2
/// rather than stopping there: 9.6.6 is about which encoding is authoritative,
/// not about refusing to look at a font that plainly disagrees with it, and a
/// subset font whose `/Differences` were written against the unsubsetted
/// original is a file that exists.
pub(crate) fn cff_glyph(cff: &Cff<'_>, font: &cos_font::Font, code: u32) -> Option<u16> {
    if font.kind() == cos_font::FontKind::Type0 {
        // 9.7.4.2: a composite font addresses its descendant by CID. A
        // CID-keyed program maps the CID through its charset; a plain CFF
        // inside a CIDFontType0 has no such map and the CID is the glyph.
        let cid = font.cid_of(code);
        return if cff.is_cid() {
            cff.gid_for_cid(cid)
        } else {
            u16::try_from(cid).ok()
        };
    }
    // A CID-keyed program under a simple font dictionary is malformed, and
    // the only reading of a one-byte code left is as a CID.
    if cff.is_cid() {
        return cff.gid_for_cid(code);
    }

    let byte = u8::try_from(code).ok();
    font.glyph_name(code)
        .and_then(|name| cff.gid_for_name(name))
        .or_else(|| byte.and_then(|c| cff.gid_for_code(c)))
        .or_else(|| {
            let name = base_glyph_name(BaseEncoding::Standard, byte?)?;
            cff.gid_for_name(name)
        })
}

/// Which charstring of a Type 1 program a code selects, and on whose
/// authority (`docs/design/subsetting.md`, *Which glyphs a Type 1 code
/// selects*): the renderer, `to_svg` and the usage walk all ask this.
///
/// A Type 1 program names its charstrings, and ISO 32000-2 9.6.5.2 reaches
/// one through the document's `/Encoding` where it names the code
/// ([`cos_font::Font::glyph_name`]) and otherwise through the program's
/// built-in encoding. The order is:
///
/// 1. the document's name, where the program has a charstring of it;
/// 2. the code's text — its first character's Adobe Glyph List name;
/// 3. the built-in encoding's glyph for the code.
///
/// Steps 2 and 3 are the route a Type 1 program was drawn by before the
/// first existed, so a page moves only where the document's encoding and the
/// code's text disagree. The answer is stated where the font says it — the
/// document's name, or the built-in encoding's where the document names
/// nothing, or a name the program lacks falling through to it as
/// [`cff_glyph`] falls through — and a guess where only the text reaches it;
/// where the text reaches one glyph and the built-in encoding another for a
/// code the document names nothing for, the built-in encoding's is
/// [`Selection::also`], which a reader following 9.6.5.2 draws. `None` where
/// no step reaches a glyph.
pub(crate) fn type1_selection(
    type1: &Type1<'_>,
    font: &cos_font::Font,
    code: u32,
) -> Option<Selection> {
    let named = font.glyph_name(code);
    if let Some(glyph) = named.and_then(|name| type1.glyph_for_name(name.as_bytes())) {
        return Some(Selection::stated(glyph));
    }
    let by_text = font.text_of(code).chars().next().and_then(|c| {
        let name = tinker_pdf_font::glyph_name_for_char(c)?;
        type1.glyph_for_name(name.as_bytes())
    });
    let by_code = u8::try_from(code)
        .ok()
        .and_then(|byte| type1.glyph_for_code(byte));
    let drawn = by_text.or(by_code)?;
    Some(if named.is_none() {
        match (by_text, by_code) {
            (Some(text), Some(built_in)) if text != built_in => Selection {
                glyph: text,
                stated: true,
                also: Some(built_in),
            },
            (_, Some(_)) => Selection::stated(drawn),
            _ => Selection::guessed(drawn),
        }
    } else if by_text.is_none() {
        Selection::stated(drawn)
    } else {
        Selection::guessed(drawn)
    })
}

/// Scales an outline from font units into em units.
fn scale(outline: &Outline, factor: f64) -> Outline {
    use tinker_pdf_font::Segment;
    let s = |v: f64| v * factor;
    Outline {
        segments: outline
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::MoveTo { x, y } => Segment::MoveTo { x: s(x), y: s(y) },
                Segment::LineTo { x, y } => Segment::LineTo { x: s(x), y: s(y) },
                Segment::QuadTo { cx, cy, x, y } => Segment::QuadTo {
                    cx: s(cx),
                    cy: s(cy),
                    x: s(x),
                    y: s(y),
                },
                Segment::CurveTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => Segment::CurveTo {
                    c1x: s(c1x),
                    c1y: s(c1y),
                    c2x: s(c2x),
                    c2y: s(c2y),
                    x: s(x),
                    y: s(y),
                },
                Segment::Close => Segment::Close,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        colorant_tints, component, design_coordinates, expand_inline_abbreviations, jpeg_scale,
        jpeg_scale_within, jpx_discard, jpx_skip, tint_byte, Answer, Asked, Contents, Frame,
        ImagePlating, JpxFrame, PageResources, PlatePlan, Remembered, Room, SourceWindow,
        REMEMBERED_COLOURS,
    };
    use tinker_pdf_color::{ColorSpace, Colorant, Colorants};
    use tinker_pdf_content::{ColorantKind, FontSource};
    use tinker_pdf_cos::{Name, ObjRef, Object};
    use tinker_pdf_filters::JpegScale;
    use tinker_pdf_render::{ImageRequest, UnitArea};

    /// **A multiple-master instance's name carries its design coordinates by
    /// TN #5088's convention**, which 9.6.2.3 cites: past a subset tag, every
    /// run of digits after the first underscore, whatever labels separate
    /// them; none where there is no underscore, no digit after it, or a run
    /// longer than nine digits.
    #[test]
    fn the_design_coordinates_a_name_carries_are_read_by_the_convention() {
        let read = |name: &str| design_coordinates(name);
        assert_eq!(read("MinionMM_366_465_11_"), Some(vec![366.0, 465.0, 11.0]));
        assert_eq!(read("MyriadMM_367wd_450wt"), Some(vec![367.0, 450.0]));
        assert_eq!(
            read("GaramMM Bol_700 wt 470 wd 24 op"),
            Some(vec![700.0, 470.0, 24.0])
        );
        assert_eq!(
            read("MinioMM ItEp_367 RG 485 NO 11 OP"),
            Some(vec![367.0, 485.0, 11.0])
        );
        assert_eq!(
            read("ABCDEF+MinionMM_366_465_11_"),
            Some(vec![366.0, 465.0, 11.0])
        );
        assert_eq!(
            read("Type1MM2_7"),
            Some(vec![7.0]),
            "digits before the underscore are the name's"
        );
        assert_eq!(
            read("ABCDE+MM_1_2"),
            Some(vec![1.0, 2.0]),
            "five capitals are no subset tag"
        );
        assert_eq!(read("MinionMM"), None);
        assert_eq!(read("MinionMM_"), None);
        assert_eq!(read("Minion_MM_Regular"), None);
        assert_eq!(read("MM_1234567890_"), None);
        assert_eq!(read("MM_123456789_"), Some(vec![123_456_789.0]));
        assert_eq!(read(""), None);
        assert_eq!(read("ABCDEF+"), None);
    }

    /// A draw of `across` by `down` pixels.
    fn drawn(across: f64, down: f64) -> ImageRequest {
        ImageRequest {
            drawn: (across, down),
            ..ImageRequest::FULL
        }
    }

    /// **The discard is the most levels whose image covers the draw**
    /// (`docs/design/jpx-speed.md`, "The page's request"): `W >> d` by
    /// `H >> d` at least the drawn pixels both ways, the floor whatever the
    /// codestream's origin, so a quarter of an odd side drops one level fewer
    /// than a quarter of an even one; never more than the codestream can
    /// drop; at least one sample left however small the draw; and none for a
    /// size that is not finite or a draw larger than the image.
    #[test]
    fn a_jpx_discard_is_the_most_levels_whose_image_covers_the_draw() {
        let size = (620, 877);
        for (request, levels, want, what) in [
            (drawn(620.0, 877.0), 5, 0, "its own size"),
            (drawn(621.0, 877.0), 5, 0, "magnified"),
            (drawn(310.0, 438.0), 5, 1, "a half, the floor of each side"),
            (drawn(310.0, 438.5), 5, 0, "a hair past half its height"),
            (drawn(155.0, 219.0), 5, 2, "a quarter, floored"),
            (
                drawn(155.0, 219.25),
                5,
                1,
                "an exact quarter of an odd side",
            ),
            (drawn(19.0, 27.0), 5, 5, "a thirty-second"),
            (drawn(1.0, 1.0), 5, 5, "a pixel, clamped to the levels"),
            (drawn(1.0, 1.0), 2, 2, "a pixel of a codestream of two"),
            (drawn(1.0, 1.0), 0, 0, "a codestream that can drop none"),
            (
                drawn(155.0, 877.0),
                5,
                0,
                "a quarter of its width and all its height",
            ),
            (
                drawn(0.0, 0.0),
                40,
                9,
                "nothing drawn: at least a sample left",
            ),
            (drawn(f64::INFINITY, 1.0), 5, 0, "an unbounded size"),
            (drawn(f64::NAN, 1.0), 5, 0, "a size that is no number"),
            (ImageRequest::FULL, 5, 0, "the whole image"),
        ] {
            assert_eq!(jpx_discard(size, levels, &request), want, "{what}");
        }
        // Thirty-one is the most a shift leaves a sample of, whatever the
        // levels say.
        assert_eq!(
            jpx_discard((u32::MAX, u32::MAX), u8::MAX, &drawn(1.0, 1.0)),
            31
        );
        assert_eq!(jpx_discard((1, 1), u8::MAX, &drawn(0.5, 0.5)), 0);
    }

    /// **The skip is how far into the image the first reduced cell starts**
    /// (T.800 B.5): none at the grid's origin or at any multiple of the cell,
    /// and the rest of the cell otherwise.
    #[test]
    fn a_jpx_skip_is_where_the_first_reduced_cell_starts() {
        for (origin, d, want) in [
            (0, 0, 0),
            (0, 5, 0),
            (3, 0, 0),
            (3, 1, 1),
            (3, 2, 1),
            (5, 2, 3),
            (8, 3, 0),
            (9, 3, 7),
            (u32::MAX, 31, 1),
            (u32::MAX, 40, 1),
        ] {
            assert_eq!(jpx_skip(origin, d), want, "{origin} at {d} dropped");
        }
    }

    /// **A frame's ask is the key its draws are cached under**: a JPEG's the
    /// denominator of its scale and never a window, a codestream's two to the
    /// levels it drops, and either one whole at the image's own size.
    #[test]
    fn a_frames_ask_is_its_scale_and_its_window() {
        let jpeg = Frame::Jpeg {
            width: 512,
            height: 384,
        };
        let jpx = Frame::Jpx(JpxFrame {
            width: 620,
            height: 877,
            origin: (0, 0),
            levels: 5,
            opaque: true,
            interpolate: false,
        });
        for (frame, request, want) in [
            (jpeg, drawn(512.0, 384.0), 1),
            (jpeg, drawn(256.0, 192.0), 2),
            (jpeg, drawn(128.0, 96.0), 4),
            (jpeg, drawn(1.0, 1.0), 8),
            (jpx, drawn(620.0, 877.0), 1),
            (jpx, drawn(155.0, 219.0), 4),
            (jpx, drawn(1.0, 1.0), 32),
            (jpx, ImageRequest::FULL, 1),
        ] {
            assert_eq!(
                frame.asked(&request, None),
                Asked {
                    reduction: want,
                    window: None
                },
                "{frame:?} drawn {:?}",
                request.drawn
            );
        }
        let corner = ImageRequest {
            visible: Some(UnitArea {
                u0: 0.25,
                v0: 0.5,
                u1: 0.75,
                v1: 1.0,
            }),
            ..drawn(155.0, 219.0)
        };
        assert!(
            jpx.asked(&corner, None).window.is_some(),
            "a codestream's part"
        );
        assert_eq!(
            jpeg.asked(&corner, None).window,
            None,
            "a JPEG is decoded whole"
        );
    }

    /// **The room is the budget less what an entry is charged beside its
    /// samples**: the cache's 64 bytes and the key's defaults, never below
    /// nothing; and a decode fits it when its samples' bytes are at most it.
    #[test]
    fn the_room_is_the_budget_less_an_entrys_own_charge() {
        assert_eq!(Room::under(10_000, 0), Room(10_000 - 64));
        assert_eq!(Room::under(10_000, 36), Room(10_000 - 100));
        assert_eq!(Room::under(100, 36), Room(0), "a budget its key fills");
        assert_eq!(Room::under(0, 0), Room(0), "a budget of zero");
        assert_eq!(Room::under(5, u64::MAX), Room(0), "no overflow");
        assert_eq!(Room::under(u64::MAX, 0), Room(u64::MAX - 64));
        assert!(Room(12).holds(4, 3), "exactly the room");
        assert!(!Room(11).holds(4, 3), "a byte past it");
        assert!(
            !Room(u64::MAX - 1).holds(u64::MAX, 2),
            "saturating, never wrapping"
        );
        assert!(Room(u64::MAX).holds(u64::MAX, 4));
    }

    /// **A budget takes a JPEG to the largest of the smaller scales whose
    /// decode fits** (`docs/design/performance.md`, P-16, "Reduced-resolution
    /// decode"). A 512 by 384 frame holds 589 824 bytes at full size, 147 456
    /// at a half, 36 864 at a quarter and 9 216 at an eighth, three a sample:
    /// each fits a room of exactly its bytes and not one byte less, where the
    /// next scale down is taken. A room no scale fits leaves the draw's own,
    /// and so does no budget. A draw that already asks for a quarter never
    /// goes back up to a half however roomy the budget, and is taken down to
    /// an eighth only where a quarter does not fit. Odd sides round up,
    /// `⌈601·4/8⌉ = 301` by `⌈401·4/8⌉ = 201`.
    #[test]
    fn a_budget_takes_a_jpeg_to_the_largest_smaller_scale_that_fits() {
        let frame = (512, 384);
        let whole = drawn(512.0, 384.0);
        assert_eq!(jpeg_scale(frame, &whole), JpegScale::Full);
        let within = |request: &ImageRequest, room: Option<u64>| {
            jpeg_scale_within(frame, request, room.map(Room))
        };
        for (room, want) in [
            (None, JpegScale::Full),
            (Some(u64::MAX), JpegScale::Full),
            (Some(589_824), JpegScale::Full),
            (Some(589_823), JpegScale::Half),
            (Some(147_456), JpegScale::Half),
            (Some(147_455), JpegScale::Quarter),
            (Some(36_864), JpegScale::Quarter),
            (Some(36_863), JpegScale::Eighth),
            (Some(9_216), JpegScale::Eighth),
            (Some(9_215), JpegScale::Full),
            (Some(0), JpegScale::Full),
        ] {
            assert_eq!(within(&whole, room), want, "a room of {room:?}");
        }

        let quarter = drawn(128.0, 96.0);
        assert_eq!(jpeg_scale(frame, &quarter), JpegScale::Quarter);
        for (room, want) in [
            (None, JpegScale::Quarter),
            (Some(u64::MAX), JpegScale::Quarter),
            (Some(36_864), JpegScale::Quarter),
            (Some(36_863), JpegScale::Eighth),
            (Some(9_216), JpegScale::Eighth),
            (Some(9_215), JpegScale::Quarter),
        ] {
            assert_eq!(within(&quarter, room), want, "a quarter's draw, {room:?}");
        }

        // A size that is no number asks for the whole image, and a budget
        // reduces it as it reduces a draw at full size.
        assert_eq!(within(&ImageRequest::FULL, Some(147_456)), JpegScale::Half);

        let odd = (601, 401);
        let at = |room: u64| jpeg_scale_within(odd, &drawn(601.0, 401.0), Some(Room(room)));
        assert_eq!(at(301 * 201 * 3), JpegScale::Half);
        assert_eq!(
            at(301 * 201 * 3 - 1),
            JpegScale::Quarter,
            "151 by 101 a quarter"
        );
        assert_eq!(
            at(151 * 101 * 3 - 1),
            JpegScale::Eighth,
            "76 by 51 an eighth"
        );

        // What a frame asks under the room is the scale's reduction.
        let jpeg = Frame::Jpeg {
            width: 512,
            height: 384,
        };
        for (room, want) in [
            (589_824, 1),
            (589_823, 2),
            (147_455, 4),
            (36_863, 8),
            (9_215, 1),
        ] {
            assert_eq!(
                jpeg.asked(&whole, Some(Room(room))),
                Asked {
                    reduction: want,
                    window: None
                },
                "a room of {room}"
            );
        }
    }

    /// **A budget takes a codestream to the fewest levels dropped past the
    /// draw's own whose reduced image fits**, read off the whole reduced
    /// image and never the window, so a tile of a page is decoded at the
    /// page's levels (ruling 5). The 620 by 877 codestream of five levels
    /// reduces to 310 by 439, 155 by 220, 78 by 110, 39 by 55 and 20 by 28
    /// (T.800 B.5), 408 270, 102 300, 25 740, 6 435 and 1 680 bytes at three a
    /// sample and 1 631 220 whole; with opacity put to use, four a sample. A
    /// room none fits leaves the draw's own levels; so does no budget, and so
    /// does a frame that can drop none, as one whose samples are indices.
    #[test]
    fn a_budget_takes_a_codestream_to_the_fewest_levels_that_fit() {
        let frame = JpxFrame {
            width: 620,
            height: 877,
            origin: (0, 0),
            levels: 5,
            opaque: true,
            interpolate: false,
        };
        let whole = drawn(620.0, 877.0);
        let discard = |frame: &JpxFrame, request: &ImageRequest, room: Option<u64>| {
            frame.ask_within(request, room.map(Room)).0
        };
        for (room, want) in [
            (None, 0),
            (Some(u64::MAX), 0),
            (Some(1_631_220), 0),
            (Some(1_631_219), 1),
            (Some(408_270), 1),
            (Some(408_269), 2),
            (Some(102_299), 3),
            (Some(25_739), 4),
            (Some(6_434), 5),
            (Some(1_680), 5),
            (Some(1_679), 0),
            (Some(0), 0),
        ] {
            assert_eq!(discard(&frame, &whole, room), want, "a room of {room:?}");
        }

        let translucent = JpxFrame {
            opaque: false,
            ..frame
        };
        assert_eq!(discard(&translucent, &whole, Some(310 * 439 * 4)), 1);
        assert_eq!(
            discard(&translucent, &whole, Some(408_270)),
            2,
            "four bytes a sample: 310 by 439 is 544 360 of them"
        );

        let quarter = drawn(155.0, 219.0);
        assert_eq!(frame.ask(&quarter).0, 2);
        assert_eq!(
            discard(&frame, &quarter, Some(u64::MAX)),
            2,
            "never back up"
        );
        assert_eq!(discard(&frame, &quarter, Some(102_300)), 2);
        assert_eq!(discard(&frame, &quarter, Some(25_740)), 3);
        assert_eq!(discard(&frame, &quarter, Some(1_679)), 2, "none fits");

        let indices = JpxFrame { levels: 0, ..frame };
        assert_eq!(discard(&indices, &whole, Some(1)), 0, "nothing to drop");

        // The window is the one the draw holds that many levels down, and the
        // levels are the page's whatever part of it a canvas shows: a draw
        // showing a corner whose window would fit at the draw's own levels
        // drops what the whole image needs, as the page does.
        let corner = showing(620.0, 877.0, (0.0, 0.75, 0.25, 1.0));
        let (own, held) = frame.ask(&corner);
        assert_eq!(own, 0);
        let held = held.expect("a corner is a window");
        let room = u64::from(held.width) * u64::from(held.height) * 3;
        assert!(
            room < 1_631_220,
            "the corner's window fits where the image does not"
        );
        assert_eq!(
            frame.ask_within(&corner, Some(Room(room))),
            (
                discard(&frame, &whole, Some(room)),
                frame.window(discard(&frame, &whole, Some(room)), &corner)
            ),
            "the page's levels and their window"
        );
        assert_eq!(
            Frame::Jpx(frame).asked(&whole, Some(Room(408_269))),
            Asked {
                reduction: 4,
                window: None
            }
        );
    }

    /// A draw of `across` by `down` pixels that reaches all of its image and
    /// shows `visible` of it.
    fn showing(across: f64, down: f64, visible: (f64, f64, f64, f64)) -> ImageRequest {
        let (u0, v0, u1, v1) = visible;
        ImageRequest {
            drawn: (across, down),
            reach: Some(UnitArea {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
            }),
            visible: Some(UnitArea { u0, v0, u1, v1 }),
        }
    }

    /// **A codestream's window is the part a draw shows, in the reduced
    /// image's samples, widened by the sampler's reach and moved out to the
    /// pyramid's pairs** (`docs/design/jpx-speed.md`, "The visible window").
    ///
    /// The 620 by 877 codestream drawn at a quarter and showing the top half
    /// of its middle half across: two levels dropped, 155 by 220 samples, the
    /// part `0.25 · 155 = 38.75` to `116.25` across and `0` to
    /// `0.5 · 219.25 = 109.625` down, floored and ceiled to 38..117 and
    /// 0..110; a footprint of one texel across and a hair over one down,
    /// where the draw minifies by 220 samples to 219 pixels, so a reach of
    /// `(1 + 2) · 2` and `(2 + 2) · 2`, and every edge on a multiple of two.
    /// Under the same draw an image with opacity of its own holds all it
    /// reaches, here all of it; a draw that shows nothing holds the one
    /// sample its corner is nearest; one that asks for no part holds the
    /// whole. Drawn at twice its size with no level dropped, the bottom right
    /// quarter; at an origin inside a cell, the samples are counted from the
    /// first cell, `skip + 4 · X` columns in; and past 128 samples a pixel,
    /// six halvings make a level of 64 and the edges move out to 128.
    #[test]
    fn a_codestreams_window_is_what_the_draw_shows_and_the_samplers_reach() {
        let frame = JpxFrame {
            width: 620,
            height: 877,
            origin: (0, 0),
            levels: 5,
            opaque: true,
            interpolate: false,
        };
        let window = |frame: &JpxFrame, request: &ImageRequest| frame.ask(request);
        let held = |x, y, width, height| {
            Some(SourceWindow {
                x,
                y,
                width,
                height,
            })
        };
        let corner = showing(155.0, 219.0, (0.25, 0.5, 0.75, 1.0));
        assert_eq!(frame.reduced(2), (155, 220));
        assert_eq!(
            window(&frame, &corner),
            (2, held(32, 0, 92, 118)),
            "the part shown and the reach"
        );
        let translucent = JpxFrame {
            opaque: false,
            ..frame
        };
        assert_eq!(
            window(&translucent, &corner),
            (2, None),
            "an image with opacity holds all it reaches"
        );
        let nothing = showing(155.0, 219.0, (0.5, 0.5, 0.5, 0.5));
        assert_eq!(
            window(&frame, &nothing),
            (2, held(77, 109, 1, 1)),
            "a draw that shows nothing holds a sample"
        );
        assert_eq!(
            window(&frame, &drawn(155.0, 219.0)),
            (2, None),
            "a draw that names no part holds all of it"
        );
        let magnified = showing(1240.0, 1754.0, (0.5, 0.0, 1.0, 0.5));
        assert_eq!(
            window(&frame, &magnified),
            (0, held(304, 432, 316, 445)),
            "the bottom right quarter, magnified"
        );

        let inside = JpxFrame {
            width: 997,
            height: 1013,
            origin: (3, 5),
            ..frame
        };
        assert_eq!(inside.reduced(2), (249, 253));
        assert_eq!(
            window(&inside, &showing(249.0, 253.0, (0.5, 0.25, 0.75, 0.5))),
            (2, held(116, 116, 80, 82)),
            "counted from the first cell, a column and three rows in"
        );

        let wide = JpxFrame {
            width: 8_000,
            height: 100,
            origin: (0, 0),
            levels: 0,
            ..frame
        };
        assert_eq!(
            window(&wide, &showing(50.0, 50.0, (0.5, 0.0, 1.0, 1.0))),
            (0, held(3_328, 0, 4_672, 100)),
            "edges on the pyramid's pairs of its sixth level"
        );
    }

    /// An image's colour is converted once however many pixels repeat it, and
    /// two colours whose samples share a slot each get their own answer — the
    /// table shares work, never a colour. A device space, and a pixel of more
    /// samples than a key holds, are converted directly.
    #[test]
    fn an_images_repeated_colour_is_converted_once() {
        let lab = ColorSpace::Lab {
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let mut table = Remembered::for_image(&lab, 3, 8).expect("a Lab image is remembered");
        let key = |samples: [u32; 3]| {
            samples
                .iter()
                .enumerate()
                .fold(0u128, |key, (index, sample)| {
                    Remembered::key(key, index, *sample)
                })
        };
        let first = key([10, 20, 30]);
        let mut calls = 0;
        for _ in 0..1000 {
            let colour = table.convert(first, || {
                calls += 1;
                (1, 2, 3)
            });
            assert_eq!(colour, (1, 2, 3));
        }
        assert_eq!(calls, 1, "a thousand pixels of one colour, converted once");

        let slot = Remembered::slot(first);
        let neighbour = (0..=255u32)
            .flat_map(|a| (0..=255u32).map(move |b| [a, b, 7]))
            .map(key)
            .find(|other| *other != first && Remembered::slot(*other) == slot)
            .expect("some colour shares the slot");
        assert_eq!(table.convert(neighbour, || (4, 5, 6)), (4, 5, 6));
        assert_eq!(
            table.convert(first, || (1, 2, 3)),
            (1, 2, 3),
            "the first colour converts again rather than answering as its neighbour"
        );

        assert!(Remembered::for_image(&ColorSpace::DeviceCmyk, 4, 8).is_none());
        assert!(Remembered::for_image(&lab, 9, 8).is_none());
    }

    /// An ICC profile of `class` over `space` holding `tags`, written as
    /// ICC.1 7.2 and 7.3 lay a profile out.
    fn profile_bytes(class: &[u8; 4], space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let start = 128 + 4 + 12 * tags.len();
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (signature, body) in tags {
            while (start + data.len()) % 4 != 0 {
                data.push(0);
            }
            entries.push((*signature, start + data.len(), body.len()));
            data.extend_from_slice(body);
        }
        while (start + data.len()) % 4 != 0 {
            data.push(0);
        }
        let mut profile = vec![0u8; 128];
        profile[0..4].copy_from_slice(&((start + data.len()) as u32).to_be_bytes());
        profile[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
        profile[12..16].copy_from_slice(class);
        profile[16..20].copy_from_slice(space);
        profile[20..24].copy_from_slice(b"XYZ ");
        profile[36..40].copy_from_slice(b"acsp");
        profile.extend_from_slice(&(tags.len() as u32).to_be_bytes());
        for (signature, offset, len) in entries {
            profile.extend_from_slice(&signature);
            profile.extend_from_slice(&(offset as u32).to_be_bytes());
            profile.extend_from_slice(&(len as u32).to_be_bytes());
        }
        profile.extend_from_slice(&data);
        profile
    }

    /// An `XYZ ` tag of one value, in s15Fixed16Numbers (ICC.1 4.6).
    fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut tag = b"XYZ \0\0\0\0".to_vec();
        for value in [x, y, z] {
            tag.extend_from_slice(&(((value * 65536.0).round() as i32) as u32).to_be_bytes());
        }
        tag
    }

    /// A `curv` tag of one gamma.
    fn gamma(gamma: f64) -> Vec<u8> {
        let mut tag = b"curv\0\0\0\0".to_vec();
        tag.extend_from_slice(&1u32.to_be_bytes());
        tag.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
        tag
    }

    /// The ICC space of a matrix/TRC RGB profile, gamma 1.8, compiled.
    fn icc_rgb() -> ColorSpace {
        let bytes = profile_bytes(
            b"mntr",
            b"RGB ",
            &[
                (*b"rXYZ", xyz(0.436_065, 0.222_488, 0.013_916)),
                (*b"gXYZ", xyz(0.385_147, 0.716_873, 0.097_076)),
                (*b"bXYZ", xyz(0.143_066, 0.060_607, 0.714_096)),
                (*b"rTRC", gamma(1.8)),
                (*b"gTRC", gamma(1.8)),
                (*b"bTRC", gamma(1.8)),
                (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
            ],
        );
        let profile = tinker_pdf_color::icc::Profile::parse(&bytes).expect("the profile reads");
        let transforms =
            tinker_pdf_color::icc::Transforms::compile(profile).expect("the profile compiles");
        ColorSpace::Icc {
            transforms: std::sync::Arc::new(transforms),
            components: 3,
        }
    }

    /// A `/Separation` space over DeviceRGB, white to a blue along `t`.
    fn spot() -> ColorSpace {
        ColorSpace::Separation {
            components: 1,
            alternate: Box::new(ColorSpace::DeviceRgb),
            tint: Box::new(tinker_pdf_color::Function::Exponential {
                domain: (0.0, 1.0),
                c0: vec![1.0, 1.0, 1.0],
                c1: vec![0.2, 0.4, 0.9],
                n: 1.0,
            }),
            colorants: Colorants::default(),
        }
    }

    /// **A converter is chosen by space, depth, `/Decode` and colour key**
    /// (`docs/design/icc.md`, P-22, "The per-image converter"): a copy for
    /// 8-bit DeviceGray and DeviceRGB, and a refused profile approximated as
    /// either, under `/Decode` absent or `[0 1]` and no key; the row path for
    /// an 8-bit ICC space under the same, whose transform takes the space's
    /// components; a table of `2^bpc` values for one component of up to eight
    /// bits, whatever its space and `/Decode`; DeviceCMYK's table for 8-bit
    /// DeviceCMYK under `/Decode` absent or `[0 1]`, a key or none; and the
    /// general arm for everything else — sixteen bits, more components, a
    /// `/Decode` the copy and the tables cannot fold in, `/Lab`.
    #[test]
    fn a_converter_is_chosen_by_space_depth_decode_and_key() {
        use super::{Arm, Converter};
        let choose = |space: &ColorSpace, n: usize, bpc: u32, decode: &[(f64, f64)], keyed| {
            match Converter::new(space, n, bpc, decode, Default::default(), keyed).arm {
                Arm::Copy { grey: true } => "copy grey",
                Arm::Copy { grey: false } => "copy",
                Arm::Row { .. } => "row",
                Arm::Values(table) => {
                    assert_eq!(table.len(), 1 << bpc.max(1), "a value a table entry");
                    "values"
                }
                Arm::Cmyk(table) => {
                    assert_eq!(table.len(), 1 << 16, "an entry an ink and black byte");
                    "cmyk"
                }
                Arm::General { .. } => "general",
            }
        };
        let none: &[(f64, f64)] = &[];
        let written = [(0.0, 1.0); 4];
        let inverted = [(1.0, 0.0); 4];
        let lab = ColorSpace::Lab {
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let indexed = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            lookup: vec![0; 768],
            high: 255,
        };
        let icc = icc_rgb();
        for (space, n, bpc, decode, keyed, want) in [
            (ColorSpace::DeviceGray, 1, 8, none, false, "copy grey"),
            (
                ColorSpace::DeviceGray,
                1,
                8,
                &written[..1],
                false,
                "copy grey",
            ),
            (ColorSpace::DeviceRgb, 3, 8, none, false, "copy"),
            (ColorSpace::DeviceRgb, 3, 8, &written[..3], false, "copy"),
            (
                ColorSpace::Approximated { components: 3 },
                3,
                8,
                none,
                false,
                "copy",
            ),
            (
                ColorSpace::Approximated { components: 1 },
                1,
                8,
                none,
                false,
                "copy grey",
            ),
            (ColorSpace::DeviceGray, 1, 8, none, true, "values"),
            (
                ColorSpace::DeviceGray,
                1,
                8,
                &inverted[..1],
                false,
                "values",
            ),
            (ColorSpace::DeviceGray, 1, 4, none, false, "values"),
            (ColorSpace::DeviceGray, 1, 1, none, false, "values"),
            (ColorSpace::DeviceGray, 1, 16, none, false, "general"),
            (ColorSpace::DeviceRgb, 3, 8, none, true, "general"),
            (
                ColorSpace::DeviceRgb,
                3,
                8,
                &inverted[..3],
                false,
                "general",
            ),
            (ColorSpace::DeviceRgb, 3, 16, none, false, "general"),
            (icc.clone(), 3, 8, none, false, "row"),
            (icc.clone(), 3, 8, &written[..3], false, "row"),
            (icc.clone(), 3, 8, none, true, "general"),
            (icc.clone(), 3, 8, &inverted[..3], false, "general"),
            (icc.clone(), 3, 16, none, false, "general"),
            (icc.clone(), 4, 8, none, false, "general"),
            (ColorSpace::DeviceCmyk, 4, 8, none, false, "cmyk"),
            (ColorSpace::DeviceCmyk, 4, 8, &written[..], true, "cmyk"),
            (
                ColorSpace::Approximated { components: 4 },
                4,
                8,
                none,
                false,
                "cmyk",
            ),
            (
                ColorSpace::DeviceCmyk,
                4,
                8,
                &inverted[..],
                false,
                "general",
            ),
            (ColorSpace::DeviceCmyk, 4, 16, none, false, "general"),
            (ColorSpace::DeviceCmyk, 4, 4, none, false, "general"),
            (indexed.clone(), 1, 8, none, false, "values"),
            (indexed, 1, 2, none, false, "values"),
            (spot(), 1, 8, none, false, "values"),
            (spot(), 1, 8, &inverted[..1], true, "values"),
            (lab, 3, 8, none, false, "general"),
        ] {
            assert_eq!(
                choose(&space, n, bpc, decode, keyed),
                want,
                "{space:?} at {bpc} bits under {decode:?}, keyed {keyed}"
            );
        }
    }

    /// The colour the sample loop gives the raw `values` of a pixel in
    /// `space` at `bpc` bits under `decode`: each component read by
    /// [`super::component`] and the pixel by `ColorSpace::to_rgb_in`, the
    /// conversion every arm stands in for.
    fn by_to_rgb(
        space: &ColorSpace,
        bpc: u32,
        decode: &[(f64, f64)],
        values: &[u32],
    ) -> (u8, u8, u8) {
        let max = ((1u32 << bpc) - 1) as f64;
        let components: Vec<f64> = values
            .iter()
            .enumerate()
            .map(|(c, &value)| super::component(space, decode, c, value, max))
            .collect();
        space.to_rgb_in(&components, Default::default())
    }

    /// **Every arm gives each pixel the colour `to_rgb` gives it, and asks at
    /// most what its table holds.** Each one-component space's every value at
    /// 1, 2, 4 and 8 bits under three `/Decode`s, a value a pixel twice over;
    /// DeviceCMYK's every pair of an ink byte and a black byte in each of C,
    /// M and Y; the copies' every byte in every channel; the row path over a
    /// run of every byte in each channel; and the general arm over a `/Lab`
    /// image at sixteen bits — each pixel, run a pixel at a time and whole,
    /// equal to [`by_to_rgb`]. A table arm asks at most its entries of the
    /// conversion however many times its pixels repeat them, the copy and the
    /// row path none, and the general arm once a pixel no remembered colour
    /// answers.
    #[test]
    fn every_arm_converts_as_to_rgb_does() {
        use super::{Converter, SAMPLE_CONVERSIONS};
        let conversions = || SAMPLE_CONVERSIONS.with(std::cell::Cell::get);
        let runs = |space: &ColorSpace,
                    bpc: u32,
                    decode: &[(f64, f64)],
                    pixels: &[Vec<u32>],
                    most: usize| {
            let n = space.components();
            // The values packed as a run of `bpc`-bit samples, nothing
            // between them.
            let mut samples = vec![0u8; (pixels.len() * n * bpc as usize).div_ceil(8)];
            for (at, value) in pixels.iter().flatten().enumerate() {
                for bit in 0..bpc {
                    if (value >> (bpc - 1 - bit)) & 1 == 1 {
                        let index = at * bpc as usize + bit as usize;
                        samples[index / 8] |= 0x80 >> (index % 8);
                    }
                }
            }
            let want: Vec<u8> = pixels
                .iter()
                .flat_map(|values| {
                    let (r, g, b) = by_to_rgb(space, bpc, decode, values);
                    [r, g, b]
                })
                .collect();
            let mut alone = Converter::new(space, n, bpc, decode, Default::default(), false);
            let before = conversions();
            let one_at_a_time: Vec<u8> = pixels
                .iter()
                .flat_map(|values| {
                    let (r, g, b) = alone.colour(space, decode, values);
                    [r, g, b]
                })
                .collect();
            let asked = conversions() - before;
            assert!(
                one_at_a_time == want,
                "{space:?} at {bpc} bits under {decode:?}, a pixel at a time"
            );
            assert_eq!(asked as u64, alone.converted, "{space:?}: counted as asked");
            assert!(
                asked <= most,
                "{space:?} at {bpc} bits: asked {asked}, at most {most}"
            );
            let mut whole = Converter::new(space, n, bpc, decode, Default::default(), false);
            let mut rgb = Vec::new();
            whole.extend(space, decode, &samples, pixels.len(), &mut rgb);
            assert!(
                rgb == want,
                "{space:?} at {bpc} bits under {decode:?}, whole"
            );
            assert!(whole.converted as usize <= most);
        };

        let one = |bpc: u32| -> Vec<Vec<u32>> {
            let values = 1u32 << bpc;
            (0..2 * values).map(|v| vec![v % values]).collect()
        };
        let indexed = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            lookup: (0..768u32).map(|i| (i * 37 % 256) as u8).collect(),
            high: 255,
        };
        for space in [ColorSpace::DeviceGray, spot(), indexed] {
            for bpc in [1u32, 2, 4, 8] {
                for decode in [&[][..], &[(1.0, 0.0)][..], &[(0.25, 0.75)][..]] {
                    runs(&space, bpc, decode, &one(bpc), 1 << bpc);
                }
            }
        }

        let pairs: Vec<Vec<u32>> = (0..1u32 << 16)
            .map(|at| {
                let (x, k) = (at >> 8, at & 0xFF);
                vec![x, 255 - x, (x * 7) % 256, k]
            })
            .collect();
        runs(&ColorSpace::DeviceCmyk, 8, &[], &pairs, 1 << 16);
        let mut twice = pairs.clone();
        twice.extend(pairs.iter().cloned());
        runs(
            &ColorSpace::DeviceCmyk,
            8,
            &[(0.0, 1.0); 4],
            &twice,
            1 << 16,
        );

        let grey: Vec<Vec<u32>> = (0..256).map(|v| vec![v]).collect();
        let colours: Vec<Vec<u32>> = (0..256).map(|v| vec![v, (v * 3) % 256, 255 - v]).collect();
        runs(&ColorSpace::DeviceGray, 8, &[], &grey, 0);
        runs(&ColorSpace::DeviceRgb, 8, &[], &colours, 0);
        runs(&icc_rgb(), 8, &[], &colours, 0);

        let lab = ColorSpace::Lab {
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        let deep: Vec<Vec<u32>> = (0..300u32)
            .map(|v| vec![v * 211 % 65_536, v * 97 % 65_536, 65_535 - v * 13])
            .collect();
        runs(&lab, 16, &[], &deep, deep.len());
    }

    /// **A run the stream stops short of converts its missing samples as
    /// zeros**, as `read_bits` reads past a stream's end: the copies, the row
    /// path and a table arm over a run that ends inside a pixel and long
    /// before the last, each the colour [`by_to_rgb`] gives the pixel's values
    /// with the missing ones zero, and a copy of the zero pixel's colour for
    /// every pixel after.
    #[test]
    fn a_short_run_converts_its_missing_samples_as_zeros() {
        use super::Converter;
        let icc = icc_rgb();
        for (space, run) in [
            (ColorSpace::DeviceGray, vec![9u8, 200, 77]),
            (ColorSpace::DeviceRgb, vec![9, 200, 77, 31, 5]),
            (icc.clone(), vec![9, 200, 77, 31, 5]),
            (ColorSpace::DeviceCmyk, vec![9, 200, 77, 31, 5, 250]),
            (spot(), vec![9, 200, 77]),
        ] {
            let n = space.components();
            let pixels = 12;
            let mut converter = Converter::new(&space, n, 8, &[], Default::default(), false);
            let mut rgb = Vec::new();
            converter.extend(&space, &[], &run, pixels, &mut rgb);
            let want: Vec<u8> = (0..pixels)
                .flat_map(|pixel| {
                    let values: Vec<u32> = (0..n)
                        .map(|c| u32::from(run.get(pixel * n + c).copied().unwrap_or(0)))
                        .collect();
                    let (r, g, b) = by_to_rgb(&space, 8, &[], &values);
                    [r, g, b]
                })
                .collect();
            assert!(rgb == want, "{space:?}: {rgb:?} against {want:?}");
            let mut none = Vec::new();
            converter.extend(&space, &[], &[], 0, &mut none);
            assert!(none.is_empty(), "{space:?}: no pixel, no colour");
        }
    }

    /// **A CMYK JPEG drawn in its own colour converts through DeviceCMYK's
    /// table**: strips handed to the image's sink one MCU row at a time, as
    /// the decoder hands them, come out as `to_rgb` gives each pixel's inks,
    /// the strips sharing one table, so what the image asks of the conversion
    /// is counted once across them and is at most the table's entries; an RGB
    /// JPEG's strips are copied and ask nothing.
    #[test]
    fn a_cmyk_jpeg_in_its_own_colour_converts_through_one_table() {
        use super::{JpegInto, Work};
        use tinker_pdf_filters::{JpegColor, JpegLayout, JpegScale, JpegSink};

        let resources = resources_of("");
        let (width, height) = (256u32, 64u32);
        let inks = |x: u32, y: u32| [x, (x + y) % 256, (x * y) % 256, (y * 4) % 256];
        for color in [JpegColor::Cmyk, JpegColor::CmykInverted, JpegColor::Rgb] {
            let n = if color == JpegColor::Rgb { 3 } else { 4 };
            let mut into = JpegInto {
                resources: &resources,
                named: None,
                name: b"Im0",
                decode: &[],
                conversion: Default::default(),
                space: None,
                own: None,
                color: JpegColor::Gray,
                width: 0,
                rgb: Vec::new(),
                plates: None,
                tints: None,
                nothing: false,
                plate_tints: Vec::new(),
            };
            into.begin(JpegLayout {
                width,
                height,
                color,
                precision: 8,
                scale: JpegScale::Full,
                full_width: width,
                full_height: height,
            });
            for strip in 0..height / 16 {
                let mut data = Vec::new();
                for y in strip * 16..strip * 16 + 16 {
                    for x in 0..width {
                        data.extend(inks(x, y)[..n].iter().map(|&v| v as u8));
                    }
                }
                into.strip(strip * 16, 16, &data);
            }
            let work = Work::new();
            into.count_into(Some(&work));
            let asked = work.stats().image_colours_converted;
            for y in 0..height {
                for x in 0..width {
                    let at = ((y * width + x) * 3) as usize;
                    let ink = inks(x, y);
                    let want = if n == 4 {
                        let unit = |v: u32| f64::from(v) / 255.0;
                        ColorSpace::DeviceCmyk.to_rgb(&ink.map(unit))
                    } else {
                        (ink[0] as u8, ink[1] as u8, ink[2] as u8)
                    };
                    assert_eq!(
                        (into.rgb[at], into.rgb[at + 1], into.rgb[at + 2]),
                        want,
                        "{color:?} at ({x}, {y})"
                    );
                }
            }
            if n == 4 {
                assert!(asked > 0 && asked <= 1 << 16, "{color:?}: {asked} asked");
                assert!(asked <= u64::from(width * height));
            } else {
                assert_eq!(asked, 0, "an RGB JPEG is copied");
            }
        }
    }

    /// **An 8-bit DeviceGray or DeviceRGB image is its own samples.**
    ///
    /// Every one of the 256 values, through both spaces, with `/Decode`
    /// absent and with each pair written out as `[0 1]`, comes out as
    /// `ColorSpace::to_rgb` gives it and as the general loop gives it, and the
    /// sample loop converts not one of them. A stream that stops short gives
    /// the zeros the loop reads past its end. A `/Decode` that is not the
    /// default and a colour key each keep the loop, which converts every
    /// sample.
    #[test]
    fn an_eight_bit_device_image_converts_by_identity() {
        use super::{general_pixels, image_pixels, SampleLayout, SAMPLE_CONVERSIONS};
        let conversions = || SAMPLE_CONVERSIONS.with(std::cell::Cell::get);

        let grey: Vec<u8> = (0..=255).collect();
        // Each channel walks the values in its own order.
        let rgb: Vec<u8> = (0..=255u8)
            .flat_map(|v| [v, 255 - v, v.wrapping_mul(7)])
            .collect();
        let written = [(0.0, 1.0); 3];
        for (space, samples) in [
            (ColorSpace::DeviceGray, &grey),
            (ColorSpace::DeviceRgb, &rgb),
        ] {
            let n = space.components();
            for decode in [&[][..], &written[..n]] {
                let layout = SampleLayout {
                    width: 16,
                    height: 16,
                    bpc: 8,
                    n,
                    space: &space,
                    decode,
                    mask: false,
                    color_key: None,
                    conversion: tinker_pdf_color::icc::Conversion::default(),
                    work: None,
                };
                let before = conversions();
                let (pixels, alpha) = image_pixels(samples, &layout);
                assert_eq!(
                    conversions(),
                    before,
                    "{space:?}, /Decode {decode:?}: the loop converted a sample"
                );
                assert!(alpha.is_empty(), "an opaque image has no alpha");
                assert_eq!(pixels.len(), 256 * 3);
                for (sample, pixel) in samples.chunks(n).zip(pixels.chunks(3)) {
                    let components: Vec<f64> =
                        sample.iter().map(|v| f64::from(*v) / 255.0).collect();
                    let (r, g, b) = space.to_rgb(&components);
                    assert_eq!(pixel, [r, g, b], "{space:?}: the sample {sample:?}");
                }
                assert_eq!(
                    general_pixels(samples, &layout),
                    (pixels, alpha),
                    "{space:?}, /Decode {decode:?}: the general loop disagrees"
                );
            }
        }

        // A stream that stops short: the samples past its end are zeros.
        let layout = SampleLayout {
            width: 16,
            height: 16,
            bpc: 8,
            n: 3,
            space: &ColorSpace::DeviceRgb,
            decode: &[],
            mask: false,
            color_key: None,
            conversion: tinker_pdf_color::icc::Conversion::default(),
            work: None,
        };
        let short = &rgb[..100];
        assert_eq!(image_pixels(short, &layout), general_pixels(short, &layout));

        // What keeps the loop: the loop converts every sample of each.
        let inverted = [(1.0, 0.0)];
        let key = [(0, 9)];
        for layout in [
            SampleLayout {
                decode: &inverted,
                ..layout
            },
            SampleLayout {
                color_key: Some(&key),
                ..layout
            },
        ] {
            let layout = SampleLayout {
                n: 1,
                space: &ColorSpace::DeviceGray,
                ..layout
            };
            let before = conversions();
            let read = image_pixels(&grey, &layout);
            assert_eq!(conversions() - before, 256, "the loop reads each of these");
            assert_eq!(read, general_pixels(&grey, &layout));
        }
    }

    /// **A one-bit image expands through its table to the samples the bit
    /// loop reads.**
    ///
    /// Every byte value, laid out as rows of every width from one sample to
    /// seventeen — so most rows end inside a byte — with one row more than
    /// the stream holds, which reads as zeros; DeviceGray and a two-colour
    /// palette; `/Decode` absent, `[1 0]` and written out as `[0 1]`; as an
    /// image and as a mask, whose two `/Decode`s are its two polarities; and
    /// with no colour key and keys selecting 0, 1 and both. Each comes out as
    /// the general loop gives it, and not one bit is read on its own.
    #[test]
    fn one_bit_samples_expand_through_the_table() {
        use super::{general_pixels, image_pixels, SampleLayout, BITS_READ};
        let bits_read = || BITS_READ.with(std::cell::Cell::get);

        let bytes: Vec<u8> = (0..=255).collect();
        let palette = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            lookup: vec![31, 58, 147, 242, 201, 76],
            high: 1,
        };
        let spaces = [ColorSpace::DeviceGray, palette];
        let decodes: [&[(f64, f64)]; 3] = [&[], &[(1.0, 0.0)], &[(0.0, 1.0)]];
        let keys: [Option<&[(u32, u32)]>; 4] =
            [None, Some(&[(0, 0)]), Some(&[(1, 1)]), Some(&[(0, 1)])];
        let mut compared = 0usize;
        for width in 1..=17u32 {
            let row_bytes = width.div_ceil(8) as usize;
            let height = (bytes.len() / row_bytes) as u32 + 1;
            for space in &spaces {
                for decode in decodes {
                    for mask in [false, true] {
                        for color_key in keys {
                            let layout = SampleLayout {
                                width,
                                height,
                                bpc: 1,
                                n: 1,
                                space,
                                decode,
                                mask,
                                color_key,
                                conversion: tinker_pdf_color::icc::Conversion::default(),
                                work: None,
                            };
                            let before = bits_read();
                            let table = image_pixels(&bytes, &layout);
                            assert_eq!(
                                bits_read(),
                                before,
                                "width {width}: the table read a bit on its own"
                            );
                            let read = general_pixels(&bytes, &layout);
                            assert!(
                                table == read,
                                "width {width}, {space:?}, /Decode {decode:?}, mask {mask}, \
                                 key {color_key:?}: the table and the bit loop part"
                            );
                            compared += read.0.len() / 3;
                        }
                    }
                }
            }
        }
        assert!(compared > 500_000, "{compared} samples compared");
    }

    /// **A one-bit image held as its two colours is its RGB form, expanded.**
    ///
    /// Every byte value, as rows of every width from one sample to seventeen
    /// with one row more than the stream holds; DeviceGray and a two-colour
    /// palette; `/Decode` absent, `[1 0]` and written out as `[0 1]`; as an
    /// image and as a mask; with no colour key and with keys selecting 0, 1
    /// and both. Each image held as two colours chooses, sample by sample,
    /// the colour the RGB form holds and the alpha it holds — opaque where it
    /// holds none — and holds a byte of 0 or 1 a sample. A key selecting both
    /// values keeps the RGB form, as does a decode asked for RGB, and an
    /// image of more than one bit or one component has no two colours.
    #[test]
    fn two_colour_samples_are_the_one_bit_expansion() {
        use super::{decoded_pixels, image_pixels, two_colour_pixels, SampleForm, SampleLayout};

        let bytes: Vec<u8> = (0..=255).collect();
        let palette = ColorSpace::Indexed {
            base: Box::new(ColorSpace::DeviceRgb),
            lookup: vec![31, 58, 147, 242, 201, 76],
            high: 1,
        };
        let spaces = [ColorSpace::DeviceGray, palette];
        let decodes: [&[(f64, f64)]; 3] = [&[], &[(1.0, 0.0)], &[(0.0, 1.0)]];
        let both: &[(u32, u32)] = &[(0, 1)];
        let keys: [Option<&[(u32, u32)]>; 4] = [None, Some(&[(0, 0)]), Some(&[(1, 1)]), Some(both)];
        let mut compared = 0usize;
        for width in 1..=17u32 {
            let row_bytes = width.div_ceil(8) as usize;
            let height = (bytes.len() / row_bytes) as u32 + 1;
            let pixels = (width * height) as usize;
            for space in &spaces {
                for decode in decodes {
                    for mask in [false, true] {
                        for color_key in keys {
                            let layout = SampleLayout {
                                width,
                                height,
                                bpc: 1,
                                n: 1,
                                space,
                                decode,
                                mask,
                                color_key,
                                conversion: tinker_pdf_color::icc::Conversion::default(),
                                work: None,
                            };
                            let what = format!(
                                "width {width}, {space:?}, /Decode {decode:?}, mask {mask}, \
                                 key {color_key:?}"
                            );
                            let (rgb, alpha) = image_pixels(&bytes, &layout);
                            assert_eq!(
                                decoded_pixels(&bytes, &layout, SampleForm::Rgb),
                                (rgb.clone(), alpha.clone(), None),
                                "{what}: asked for RGB"
                            );
                            let keys_both = !mask && color_key == Some(both);
                            let Some(two) = two_colour_pixels(&bytes, &layout) else {
                                assert!(keys_both, "{what}: held as RGB");
                                assert_eq!(
                                    decoded_pixels(&bytes, &layout, SampleForm::TwoColour),
                                    (rgb, alpha, None),
                                    "{what}: the RGB form stands"
                                );
                                continue;
                            };
                            assert!(!keys_both, "{what}: a key of both values is RGB");
                            assert_eq!(two.samples.len(), pixels, "{what}: a byte a sample");
                            assert!(two.samples.iter().all(|sample| *sample <= 1));
                            for (index, &sample) in two.samples.iter().enumerate() {
                                let [r, g, b, a] = two.colours[usize::from(sample != 0)];
                                assert_eq!(
                                    rgb.get(index * 3..index * 3 + 3),
                                    Some(&[r, g, b][..]),
                                    "{what}: the colour of sample {index}"
                                );
                                assert_eq!(
                                    alpha.get(index).copied().unwrap_or(255),
                                    a,
                                    "{what}: the alpha of sample {index}"
                                );
                            }
                            assert_eq!(
                                decoded_pixels(&bytes, &layout, SampleForm::TwoColour),
                                (Vec::new(), Vec::new(), Some(two)),
                                "{what}: the decode holds the two colours"
                            );
                            compared += pixels;
                        }
                    }
                }
            }
        }
        assert!(compared > 500_000, "{compared} samples compared");

        // Not one bit of one component: no two colours.
        for (bpc, n, space) in [
            (2, 1, ColorSpace::DeviceGray),
            (8, 1, ColorSpace::DeviceGray),
            (1, 3, ColorSpace::DeviceRgb),
        ] {
            let layout = SampleLayout {
                width: 9,
                height: 4,
                bpc,
                n,
                space: &space,
                decode: &[],
                mask: false,
                color_key: None,
                conversion: tinker_pdf_color::icc::Conversion::default(),
                work: None,
            };
            assert!(
                two_colour_pixels(&bytes, &layout).is_none(),
                "{bpc} bits of {n} components"
            );
        }
    }

    /// A document of bilevel images, one raster many ways, every image 37 by
    /// 23 samples: `Im0` a JBIG2 generic region, `Im1` a CCITT G4 fax, `Im2`
    /// one-bit Flate, `Im3` an uncompressed `/ImageMask`, `Im4` a two-colour
    /// palette, `Im5` one-bit grey under a colour key selecting 0, `Im6` under
    /// one selecting both, `Im7` under an 8-bit `/SMask`, and `Im8` an 8-bit
    /// grey image. The raster, packed and 0 for black, comes back too.
    fn bilevel_document() -> (Vec<u8>, Vec<u8>) {
        const WIDTH: u32 = 37;
        const HEIGHT: u32 = 23;
        let stride = WIDTH.div_ceil(8) as usize;
        let mut packed = vec![0u8; stride * HEIGHT as usize];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let black = (x * 3 + y * 5) % 7 < 3 || (x > 20 && y % 4 < 2);
                if !black {
                    packed[y as usize * stride + (x / 8) as usize] |= 0x80 >> (x % 8);
                }
            }
        }
        let fax = tinker_pdf_filters::ccitt_g4_encode(&tinker_pdf_filters::CcittSource {
            columns: WIDTH,
            rows: HEIGHT,
            black_is_1: false,
            stride,
            end_of_block: true,
            data: &packed,
        })
        .expect("the raster codes as G4");
        // T.88 codes 1 for black, in the embedded organisation of Annex D.3: a
        // page information segment, the region and an end of page.
        let inverted: Vec<u8> = packed.iter().map(|byte| !byte).collect();
        let region = tinker_pdf_filters::jbig2_generic_region_segment(
            &tinker_pdf_filters::Jbig2GenericSource {
                width: WIDTH,
                height: HEIGHT,
                template: 0,
                tpgdon: true,
                at: [(3, -1), (-3, -1), (2, -2), (-2, -2)],
                stride,
                data: &inverted,
            },
            0,
            0,
            0,
        )
        .expect("the raster codes as a generic region");
        let segment = |number: u32, kind: u8, data: &[u8]| {
            let mut out = number.to_be_bytes().to_vec();
            out.extend_from_slice(&[kind & 0x3F, 0, 1]);
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
            out
        };
        let mut page_information = WIDTH.to_be_bytes().to_vec();
        page_information.extend_from_slice(&HEIGHT.to_be_bytes());
        page_information.extend_from_slice(&[0; 8]);
        page_information.extend_from_slice(&[0x01, 0, 0]);
        let mut jbig2 = segment(0, 48, &page_information);
        jbig2.extend_from_slice(&segment(1, 39, &region));
        jbig2.extend_from_slice(&segment(2, 49, &[]));
        let flate = tinker_pdf_filters::zlib_compress(&packed);
        let grey: Vec<u8> = (0..WIDTH * HEIGHT).map(|i| (i * 7 % 256) as u8).collect();

        let image = |extra: &str, data: &[u8]| {
            let mut out = format!(
                "<< /Type /XObject /Subtype /Image /Width {WIDTH} /Height {HEIGHT} {extra} \
                 /Length {} >>\nstream\n",
                data.len()
            )
            .into_bytes();
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
            out
        };
        let one_bit = "/ColorSpace /DeviceGray /BitsPerComponent 1";
        let content = b"q 37 0 0 23 0 0 cm /Im0 Do Q".to_vec();
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(&content);
        stream.extend_from_slice(b"\nendstream");
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 37 23]\n   \
              /Resources << /XObject << /Im0 5 0 R /Im1 6 0 R /Im2 7 0 R /Im3 8 0 R \
              /Im4 9 0 R /Im5 10 0 R /Im6 11 0 R /Im7 12 0 R /Im8 14 0 R >> >>\n   \
              /Contents 4 0 R >>"
                .to_vec(),
            stream,
            image(&format!("{one_bit} /Filter /JBIG2Decode"), &jbig2),
            image(
                &format!(
                    "{one_bit} /Filter /CCITTFaxDecode \
                     /DecodeParms << /K -1 /Columns {WIDTH} /Rows {HEIGHT} >>"
                ),
                &fax,
            ),
            image(&format!("{one_bit} /Filter /FlateDecode"), &flate),
            image("/ImageMask true /BitsPerComponent 1", &packed),
            image(
                "/ColorSpace [/Indexed /DeviceRGB 1 <1f3a93f2c94c>] /BitsPerComponent 1",
                &packed,
            ),
            image(&format!("{one_bit} /Mask [0 0]"), &packed),
            image(&format!("{one_bit} /Mask [0 1]"), &packed),
            image(&format!("{one_bit} /SMask 13 0 R"), &packed),
            image("/ColorSpace /DeviceGray /BitsPerComponent 8", &grey),
            image("/ColorSpace /DeviceGray /BitsPerComponent 8", &grey),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        for (index, object) in objects.iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        (out, packed)
    }

    /// **A bilevel image is held at one byte a sample.**
    ///
    /// The decoded image a JBIG2 region, a CCITT G4 fax and a one-bit Flate
    /// stream are drawn from holds its two colours and one byte a sample,
    /// nothing as RGB — and so do an uncompressed `/ImageMask`, a two-colour
    /// palette, a one-bit image whose colour key selects one value and an
    /// inline one-bit image — and the copy the image cache hands out on a hit
    /// is the same. The three codecs decode the one raster to the same
    /// samples. A one-bit image under a soft mask, or whose colour key
    /// selects both values, keeps three bytes of colour and one of alpha a
    /// sample, and an 8-bit grey image its three.
    #[test]
    fn a_bilevel_image_is_held_at_one_byte_a_sample() {
        use tinker_pdf_render::GlyphSource;

        let (bytes, packed) = bilevel_document();
        let samples = 37 * 23;
        let doc = crate::Document::open(bytes).expect("it opens");
        let page = doc.page(0).expect("a page");
        let res = PageResources::new(&page.doc, &page.inner, None);
        let held = |image: &tinker_pdf_render::DecodedImage| {
            image.rgb.len()
                + image.alpha.len()
                + image.two_colour.as_ref().map_or(0, |two| two.samples.len())
        };
        let decoded = |name: &str| {
            res.image(
                name.as_bytes(),
                &tinker_pdf_render::ImageRequest::FULL,
                tinker_pdf_content::RenderingIntent::RelativeColorimetric,
            )
            .unwrap_or_else(|codec| panic!("{name} decodes, not {codec}"))
            .unwrap_or_else(|| panic!("{name} is an image"))
        };

        let black_and_white = [[0, 0, 0, 255], [255, 255, 255, 255]];
        let mut codecs = Vec::new();
        for (name, colours, stencil) in [
            ("Im0", black_and_white, false),
            ("Im1", black_and_white, false),
            ("Im2", black_and_white, false),
            ("Im3", [[0, 0, 0, 255], [0, 0, 0, 0]], true),
            ("Im4", [[31, 58, 147, 255], [242, 201, 76, 255]], false),
            ("Im5", [[0, 0, 0, 0], [255, 255, 255, 255]], false),
        ] {
            let image = decoded(name);
            assert_eq!(held(&image), samples, "{name}: one byte a sample");
            assert!(
                image.rgb.is_empty() && image.alpha.is_empty(),
                "{name}: nothing held as RGB"
            );
            assert_eq!(image.stencil, stencil, "{name}: a stencil or not");
            let two = image.two_colour.clone().expect("held as two colours");
            assert_eq!(two.colours, colours, "{name}: its two colours");
            let hit = decoded(name);
            assert_eq!(held(&hit), samples, "{name}: the cache's copy");
            assert_eq!(hit.two_colour, Some(two.clone()), "{name}: is the decode");
            codecs.push(two.samples);
        }
        assert!(
            codecs[..3].iter().all(|samples| *samples == codecs[0]),
            "JBIG2, G4 and Flate decode the one raster alike"
        );
        assert_eq!(codecs[3], codecs[0], "and so does the uncompressed mask");

        for name in ["Im6", "Im7"] {
            let image = decoded(name);
            assert!(image.two_colour.is_none(), "{name}: held as RGB");
            assert_eq!(held(&image), 4 * samples, "{name}: colour and alpha");
        }
        let grey = decoded("Im8");
        assert!(grey.two_colour.is_none());
        assert_eq!(held(&grey), 3 * samples, "an 8-bit grey image is RGB");

        let inline = res
            .inline_image(
                b"/W 37 /H 23 /CS /G /BPC 1",
                &packed,
                &tinker_pdf_render::ImageRequest::FULL,
                tinker_pdf_content::RenderingIntent::RelativeColorimetric,
            )
            .expect("the inline image decodes")
            .expect("it is an image");
        assert_eq!(held(&inline), samples, "an inline one-bit image");
        assert_eq!(
            inline.two_colour.map(|two| two.samples),
            Some(codecs[0].clone()),
            "the same raster again"
        );
    }

    fn expand(dict: &str) -> String {
        String::from_utf8(expand_inline_abbreviations(dict.as_bytes()))
            .expect("ascii in, ascii out")
    }

    /// Table 93's short keys expand whatever follows them.
    ///
    /// 7.2.2 makes `/` a delimiter in its own right, so a producer is free to
    /// write `/F/Fl` with nothing between the two names — and the substitution
    /// this replaced searched for `"/Fl "`, which is not in that string. The
    /// whole `/Filter` entry then stayed the name `/F`, matched no branch, and
    /// the compressed bytes were sampled as though they were pixels.
    #[test]
    fn an_abbreviation_expands_against_a_delimiter_as_well_as_a_space() {
        assert_eq!(
            expand("/W 4/H 4/CS/RGB/BPC 8/F/Fl"),
            "<< /Width 4/Height 4/ColorSpace/DeviceRGB/BitsPerComponent 8/Filter/FlateDecode >>"
        );
        assert_eq!(
            expand("/F[/AHx/Fl]/DP[null<</Predictor 15>>]"),
            "<< /Filter[/ASCIIHexDecode/FlateDecode]/DecodeParms[null<</Predictor 15>>] >>"
        );
    }

    /// A name that is not an abbreviation comes back exactly as it went in,
    /// including one carrying a `#` escape — which is a regular character in a
    /// name (7.3.5) and must not end it.
    #[test]
    fn a_name_that_is_not_an_abbreviation_is_untouched() {
        assert_eq!(expand("/Whirlpool"), "<< /Whirlpool >>");
        assert_eq!(expand("/CS /Sep#20A"), "<< /ColorSpace /Sep#20A >>");
        // `/D` is `/Decode` and `/DP` is `/DecodeParms`: the longer name is
        // read whole rather than as the shorter one plus a stray `P`.
        assert_eq!(
            expand("/D[1 0]/DP<<>>"),
            "<< /Decode[1 0]/DecodeParms<<>> >>"
        );
    }

    /// 8.9.7's `/I` is read by where it stands: Table 94's `/Indexed` where
    /// it begins an array, the one place a family name stands, and Table
    /// 93's `/Interpolate` everywhere else. Every `/I` was the key until
    /// October 2026.
    #[test]
    fn an_i_is_indexed_where_it_begins_an_array_and_interpolate_elsewhere() {
        assert_eq!(
            expand("/I true/CS[/I/RGB 1<FF00>]"),
            "<< /Interpolate true/ColorSpace[/Indexed/DeviceRGB 1<FF00>] >>"
        );
        // White space and a comment between the bracket and the name.
        assert_eq!(
            expand("/CS [ %c\n/I /G 1 <00FF>] /I false"),
            "<< /ColorSpace [ %c\n/Indexed /DeviceGray 1 <00FF>] /Interpolate false >>"
        );
        // Not an array's first element, so not a family.
        assert_eq!(expand("/D [/X /I]"), "<< /Decode [/X /Interpolate] >>");
    }

    /// A string is copied as it stands: a palette's bytes may spell a name,
    /// an escaped parenthesis or a balanced one, and none of them is a name.
    /// Until October 2026 `(/I…)` became `(/Interpolate…)`, a longer palette
    /// of other colours.
    #[test]
    fn a_string_in_an_inline_dictionary_is_copied_as_it_stands() {
        assert_eq!(
            expand(r"/CS[/I/RGB 1(/I/G\)(/Fl))]/F/AHx"),
            r"<< /ColorSpace[/Indexed/DeviceRGB 1(/I/G\)(/Fl))]/Filter/ASCIIHexDecode >>"
        );
        assert_eq!(expand("/CS<</G>>"), "<< /ColorSpace<</DeviceGray>> >>");
        // One that never closes runs to the end of the dictionary.
        assert_eq!(expand("/D (/F"), "<< /Decode (/F >>");
    }

    /// 11.6.6: a form group's `/CS` is remapped by the defaults of the scope
    /// its contents run in (8.6.5.6) — the form's own `/Resources` when it
    /// has them, the invoking scope when it has none, and the invoking scope
    /// for a soft mask's `/G`, which runs where the `gs` was. Today the
    /// group keeps only the space's shape, which no default changes, so this
    /// is the one place the scope it is read in shows.
    #[test]
    fn a_groups_cs_is_remapped_by_the_scope_its_contents_run_in() {
        let cal = |gamma: f64| {
            format!("[/CalRGB << /WhitePoint [0.9505 1 1.089] /Gamma [{gamma} {gamma} {gamma}] >>]")
        };
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]\n\
   /Resources << /ColorSpace << /DefaultRGB {} >> /XObject << /Own 5 0 R /Bare 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10]\n\
   /Resources << /ColorSpace << /DefaultRGB {} >> >> /Length 0 >>\nstream\n\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
            cal(2.0),
            cal(3.0)
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        let res = PageResources::new(&page.doc, &page.inner, None);
        let device = tinker_pdf_cos::Object::Name(res.doc.intern(b"DeviceRGB"));
        let gamma = |space: Option<ColorSpace>| match space {
            Some(ColorSpace::CalRgb { gamma, .. }) => Some(gamma[0]),
            _ => None,
        };
        assert_eq!(
            gamma(res.group_colour_space(&device, b"Own", Contents::OwnScope)),
            Some(3.0),
            "a form with resources of its own remaps by them"
        );
        assert_eq!(
            gamma(res.group_colour_space(&device, b"Bare", Contents::OwnScope)),
            Some(2.0),
            "a form with none remaps by the page's"
        );
        assert_eq!(
            gamma(res.group_colour_space(&device, b"Own", Contents::ThisScope)),
            Some(2.0),
            "a soft mask's group remaps where the gs was"
        );
    }

    /// A one-page document whose `/ColorSpace` dictionary is given verbatim.
    fn resources_of(color_spaces: &str) -> PageResources {
        let content = "0 0 1 1 re f";
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]\n\
   /Resources << /ColorSpace << {color_spaces} >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
            content.len() + 1
        );

        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        PageResources::new(&page.doc, &page.inner, None)
    }

    /// **An image's samples take the road a fill of their colour takes**
    /// (RC-04.12): through the plan, a DeviceCMYK sample is its components
    /// after `/Decode`, a `/DeviceN` sample its tints on its plates — and,
    /// where a name has no plate, the whole colour through its alternate — an
    /// `NChannel` sample component by component, and an `/Indexed` one its
    /// palette entry's, each the bytes `PlatePlan::sample_tints` gives the
    /// colour's own colorants; past the remembered table's 4 096 distinct
    /// samples each is worked out alike; a space of `/None` alone is nothing
    /// and a space of light no tints at all.
    #[test]
    fn image_tints_are_the_tints_a_fill_of_the_colour_puts() {
        let ramp =
            |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
        let spaces = format!(
            "/DN [/DeviceN [/Gold /Silver] /DeviceCMYK {first}] \
             /DNT [/DeviceN [/Gold /Teal] /DeviceCMYK {first}] \
             /NC [/DeviceN [/Gold /Cyan] /DeviceCMYK {first} << /Subtype /NChannel >>] \
             /Idx [/Indexed [/DeviceN [/Gold /Silver] /DeviceCMYK {first}] 1 <00FF8040>] \
             /None [/Separation /None /DeviceCMYK {first}] /Cal [/CalRGB << >>]",
            first = ramp("1 0 0 0.5"),
        );
        let resources = resources_of(&spaces);
        let plan = PlatePlan::new([
            (&b"Gold"[..], tinker_pdf_render::PlateUse::Spot),
            (b"Silver", tinker_pdf_render::PlateUse::Spot),
        ]);
        let spots = plan.spots();
        let tints_of = |name: &[u8], values: &[u32], bpc: u32, decode: &[(f64, f64)]| {
            resources
                .named_space(name, Answer::Painted, |space| {
                    let max = f64::from((1u32 << bpc) - 1);
                    let components: Vec<f64> = values
                        .iter()
                        .enumerate()
                        .map(|(c, v)| component(space, decode, c, *v, max))
                        .collect();
                    let mut expected = vec![0u8; 6];
                    let colorants = colorant_tints(space, &components, (255, 255, 255));
                    plan.sample_tints(colorants.as_ref(), (255, 255, 255), spots, &mut expected);
                    let Some(ImagePlating::Tints(mut tints)) = ImagePlating::of(space, &plan)
                    else {
                        return None;
                    };
                    let mut out = Vec::new();
                    tints.push(space, decode, values, bpc, (255, 255, 255), &mut out);
                    // Asked again, from the remembered table.
                    tints.push(space, decode, values, bpc, (255, 255, 255), &mut out);
                    Some((out, expected))
                })
                .flatten()
        };
        for (name, values) in [
            (&b"DN"[..], vec![51u32, 204]),
            (b"DNT", vec![51, 204]),
            (b"NC", vec![102, 153]),
            (b"Idx", vec![1]),
        ] {
            let (out, expected) = tints_of(name, &values, 8, &[]).expect("tints");
            assert_eq!(out.len(), 12, "two samples of six plates");
            assert_eq!(out[..6], expected[..], "{}", String::from_utf8_lossy(name));
            assert_eq!(out[6..], expected[..], "remembered alike");
        }
        let (dn, _) = tints_of(b"DN", &[51, 204], 8, &[]).expect("tints");
        assert_eq!(
            dn[..6],
            [0, 0, 0, 0, 51, 204],
            "on Gold's and Silver's plates"
        );
        let (teal, _) = tints_of(b"DNT", &[51, 204], 8, &[]).expect("tints");
        assert_eq!(
            teal[..6],
            [51, 0, 0, 26, 0, 0],
            "Teal has no plate: the whole colour through its alternate"
        );
        let (indexed, _) = tints_of(b"Idx", &[1], 8, &[]).expect("tints");
        assert_eq!(
            indexed[..6],
            [0, 0, 0, 0, 0x80, 0x40],
            "the palette's entry"
        );
        let (cmyk, expected) =
            tints_of(b"DeviceCMYK", &[255, 0, 128, 64], 8, &[(1.0, 0.0)]).expect("tints");
        assert_eq!(cmyk[..6], [0, 0, 128, 64, 0, 0], "components after /Decode");
        assert_eq!(cmyk[..6], expected[..]);

        // Past the remembered table, each sample worked out alike.
        resources.named_space(b"DN", Answer::Painted, |space| {
            let Some(ImagePlating::Tints(mut tints)) = ImagePlating::of(space, &plan) else {
                panic!("a /DeviceN names colorants");
            };
            let mut out = Vec::new();
            for i in 0..(REMEMBERED_COLOURS as u32 + 10) {
                tints.push(
                    space,
                    &[],
                    &[i % 65536, i / 7],
                    16,
                    (255, 255, 255),
                    &mut out,
                );
            }
            let last = REMEMBERED_COLOURS + 9;
            let i = last as u32;
            let gold = tint_byte(f64::from(i % 65536) / 65535.0);
            let silver = tint_byte(f64::from(i / 7) / 65535.0);
            assert_eq!(out[last * 6..], [0, 0, 0, 0, gold, silver]);
        });

        let none = resources
            .named_space(b"None", Answer::Painted, |space| {
                matches!(ImagePlating::of(space, &plan), Some(ImagePlating::Nothing))
            })
            .expect("a space");
        assert!(none, "/None alone is nothing");
        let light = resources
            .named_space(b"Cal", Answer::Painted, |space| {
                ImagePlating::of(space, &plan).is_none()
            })
            .expect("a space");
        assert!(light, "a space of light names no tints");
    }

    /// The names a colour space keeps, as bytes: an `/Indexed` space's its
    /// base's, `/All` and `/None` spelled as the file spells them.
    fn kept_names(space: &ColorSpace) -> Vec<Vec<u8>> {
        if let ColorSpace::Indexed { base, .. } = space {
            return kept_names(base);
        }
        space
            .colorants()
            .map(|colorants| {
                colorants
                    .names
                    .iter()
                    .map(|name| match name {
                        Colorant::Named(name) => name.clone(),
                        Colorant::All => b"All".to_vec(),
                        Colorant::None => b"None".to_vec(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The names `Page::images` describes a space by.
    fn described_names(space: &crate::ImageSpace) -> Vec<Vec<u8>> {
        match space {
            crate::ImageSpace::Separation { colorant, .. } => vec![colorant.clone()],
            crate::ImageSpace::DeviceN { colorants, .. } => colorants.clone(),
            crate::ImageSpace::Indexed { base, .. } => described_names(base),
            _ => Vec::new(),
        }
    }

    /// **`parse_space` and `describe_space` read the same names** (RC-04.5):
    /// a `/Separation`, `/All`, `/None`, a `/DeviceN` with attributes — a
    /// colorant named `All` among its names — and an `/Indexed` space over a
    /// `/DeviceN` give the renderer's reading and `Page::images`' the same
    /// names in the same order, as painted and as selected through the
    /// scope's own cache too. The colour model tells `/All` and `/None` from
    /// colorants of those names where 8.6.6.4 makes them special — in a
    /// `/Separation` — and keeps the `/DeviceN`'s `NChannel` subtype, its
    /// `/Colorants` as spaces and its `/Process`.
    #[test]
    fn parse_space_and_describe_space_read_the_same_names() {
        let tint = "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>";
        let res = resources_of(&format!(
            "/Sep [/Separation /PANTONE#20131 /DeviceCMYK {tint}] \
             /All [/Separation /All /DeviceCMYK {tint}] \
             /None [/Separation /None /DeviceCMYK {tint}] \
             /DN [/DeviceN [/Cyan /Orange /None /All] /DeviceCMYK {tint} \
                 << /Subtype /NChannel \
                    /Colorants << /Orange [/Separation /Orange /DeviceCMYK {tint}] /Bad 7 >> \
                    /Process << /ColorSpace /DeviceCMYK \
                       /Components [/Cyan /Magenta /Yellow /Black] >> >>] \
             /Idx [/Indexed [/DeviceN [/Gold /Silver] /DeviceCMYK {tint}] 1 <0000FFFF>]"
        ));
        let resources = res.resources.as_deref().expect("the page has resources");
        let table = res.doc.resolve_key(resources, Name::COLOR_SPACE);
        let table = table.as_dict().expect("a /ColorSpace dictionary");
        let expected: [(&str, &[&[u8]]); 5] = [
            ("Sep", &[b"PANTONE 131"]),
            ("All", &[b"All"]),
            ("None", &[b"None"]),
            ("DN", &[b"Cyan", b"Orange", b"None", b"All"]),
            ("Idx", &[b"Gold", b"Silver"]),
        ];
        for (key, names) in expected {
            let entry = table.get(res.doc.intern(key.as_bytes())).expect("an entry");
            let parsed = res.parse_space(entry, 0, key.as_bytes()).expect("a space");
            let described = res.describe_space(entry, false, 0);
            let names: Vec<Vec<u8>> = names.iter().map(|name| name.to_vec()).collect();
            assert_eq!(kept_names(&parsed), names, "{key}: parsed");
            assert_eq!(described_names(&described), names, "{key}: described");
            for answer in [Answer::Painted, Answer::Selected] {
                let kept = res
                    .named_space(key.as_bytes(), answer, kept_names)
                    .expect("the scope keeps it");
                assert_eq!(kept, names, "{key}: kept");
            }
        }

        let colorants = |key: &str| {
            let entry = table.get(res.doc.intern(key.as_bytes())).expect("an entry");
            let space = res.parse_space(entry, 0, key.as_bytes()).expect("a space");
            space
                .colorants()
                .cloned()
                .expect("a separation's colorants")
        };
        assert_eq!(colorants("All").names, vec![Colorant::All]);
        assert_eq!(colorants("None").names, vec![Colorant::None]);
        let device_n = colorants("DN");
        assert_eq!(
            device_n.names[3],
            Colorant::Named(b"All".to_vec()),
            "8.6.6.5 gives a /DeviceN no /All, so this is a colorant called All"
        );
        assert_eq!(device_n.names[2], Colorant::None);
        assert!(device_n.n_channel);
        assert_eq!(
            device_n.separations.len(),
            1,
            "an entry that is no space is not kept"
        );
        let orange = device_n.separation(b"Orange").expect("Orange alone");
        assert_eq!(
            orange.colorants().map(|c| c.names.clone()),
            Some(vec![Colorant::Named(b"Orange".to_vec())])
        );
        let process = device_n.process.as_ref().expect("a process dictionary");
        assert_eq!(*process.space, ColorSpace::DeviceCmyk);
        assert_eq!(device_n.process_component(b"Cyan"), Some(0));
        assert_eq!(device_n.process_component(b"Orange"), None);
        assert!(
            !colorants("Sep").n_channel,
            "a separation has no attributes"
        );
    }

    /// A `/DeviceN` whose `/Colorants` names its own space sixty-four times
    /// beside sixty-four separations reads its own attributes once and no
    /// deeper: the spaces an attributes dictionary holds are read without
    /// attributes, so each entry naming the space is one linear read, where
    /// reading their attributes too would fan out sixty-four ways at each of
    /// eight levels (ruling 1). An entry that is no `/Separation` is not kept,
    /// and a space that is its own alternate ends at the depth bound.
    #[test]
    fn a_device_n_naming_itself_in_its_colorants_ends() {
        let tint = "<< /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>";
        let mut entries = String::new();
        for i in 0..64 {
            entries.push_str(&format!(
                "/C{i} [/Separation /C{i} /DeviceGray {tint}] /S{i} 6 0 R "
            ));
        }
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
/Resources << /ColorSpace << /Loop 6 0 R >> >> >>\nendobj\n\
6 0 obj\n[/DeviceN [/A /B] /DeviceGray {tint} << /Colorants << {entries} >> >>]\nendobj\n\
7 0 obj\n[/DeviceN [/A /B] 7 0 R {tint}]\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        let res = PageResources::new(&page.doc, &page.inner, None);
        let separations = res
            .named_space(b"Loop", Answer::Painted, |space| {
                space
                    .colorants()
                    .map(|colorants| colorants.separations.len())
            })
            .flatten();
        assert_eq!(
            separations,
            Some(64),
            "the separations, and none of the loops"
        );
        let looped = Object::Ref(ObjRef { num: 6, gen: 0 });
        assert!(res.parse_space(&looped, 0, b"Loop").is_some());
        let own_alternate = Object::Ref(ObjRef { num: 7, gen: 0 });
        assert!(
            res.parse_space(&own_alternate, 0, b"Own").is_none(),
            "no reader finishes a space that is its own alternate"
        );
    }

    /// The tints a colorants answer holds, by name.
    fn tints_by_name(answer: &tinker_pdf_content::ColorantTints) -> Vec<(Vec<u8>, u8)> {
        answer
            .tints
            .iter()
            .map(|tint| (tint.name.to_vec(), tint.tint))
            .collect()
    }

    /// Light turned into ink, as an ink canvas turns it.
    fn ink_of(r: u8, g: u8, b: u8) -> [u8; 4] {
        let (c, m, y, k) = tinker_pdf_raster::canvas::rgb_to_cmyk(r, g, b);
        [c, m, y, k]
    }

    /// **The seam answers each space's colorants** (RC-04.7) as the design's
    /// "Names on plates" reads 8.6.6.4 and 8.6.6.5: DeviceCMYK is its own
    /// four process colorants, a `k` under a `/DefaultCMYK` among them (H-135);
    /// a `/Separation` names its colorant at its tint, beside the process ink
    /// its alternate paints for the whole colour — a DeviceCMYK alternate's
    /// own components, another's light turned into ink; a `/DeviceN` naming a
    /// process colorant names it, `/None` discarded; `/All` is one tint for
    /// everything and `/None` nothing; an `NChannel` space paints a CMYK
    /// `/Process`'s components as process colorants and converts an RGB
    /// one's together, each spot carrying its own ink alone; an `/Indexed`
    /// colour answers as its palette entry in the base, an uncoloured
    /// pattern's as its underlying space; and a space naming no colorant, or
    /// one whose names cannot all be read, answers none.
    #[test]
    fn the_seam_answers_each_spaces_colorants() {
        let ramp =
            |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [{c1}] /N 1 >>");
        let rgb_ramp =
            |c1: &str| format!("<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [{c1}] /N 1 >>");
        let spaces = format!(
            "/DefaultCMYK [/ICCBased 5 0 R] \
             /Sep [/Separation /Gold /DeviceCMYK {gold}] \
             /SepRgb [/Separation /Teal /DeviceRGB {teal}] \
             /DNp [/DeviceN [/Cyan /Spot1 /None] /DeviceCMYK {first}] \
             /All [/Separation /All /DeviceCMYK {gold}] \
             /None [/Separation /None /DeviceCMYK {gold}] \
             /NoneN [/DeviceN [/None /None] /DeviceCMYK {gold}] \
             /NCmyk [/DeviceN [/PCyan /Spot2 /Black] /DeviceCMYK {first} \
                << /Subtype /NChannel \
                   /Process << /ColorSpace /DeviceCMYK /Components [/PCyan /PMag /PYel /PBlk] >> \
                   /Colorants << /Spot2 [/Separation /Spot2 /DeviceCMYK {yellow}] >> >>] \
             /NRgb [/DeviceN [/PR /PG /PB /Red] /DeviceRGB {white} \
                << /Subtype /NChannel \
                   /Process << /ColorSpace /DeviceRGB /Components [/PR /PG /PB] >> \
                   /Colorants << /Red [/Separation /Red /DeviceRGB {red}] >> >>] \
             /NRgbPart [/DeviceN [/PR /Red] /DeviceRGB {white} \
                << /Subtype /NChannel \
                   /Process << /ColorSpace /DeviceRGB /Components [/PR /PG /PB] >> \
                   /Colorants << /Red [/Separation /Red /DeviceRGB {red}] >> >>] \
             /Idx [/Indexed [/DeviceN [/Gold /Silver] /DeviceCMYK {first}] 1 <00FF8040>] \
             /Pat [/Pattern [/Separation /Gold /DeviceCMYK {gold}]] \
             /Cal [/CalRGB << >>] \
             /Bad [/DeviceN [/A 5] /DeviceCMYK {first}]",
            gold = ramp("0 0.5 1 0"),
            teal = rgb_ramp("0 0.5 0.5"),
            first = ramp("1 0 0 0.5"),
            yellow = ramp("0 0 1 0"),
            red = rgb_ramp("1 0 0"),
            white = rgb_ramp("1 1 1"),
        );
        let content = "0 0 1 1 re f";
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]\n\
   /Resources << /ColorSpace << {spaces} >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /N 4 /Length 0 >>\nstream\n\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
            content.len() + 1
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        let page = doc.page(0).expect("a page");
        let res = PageResources::new(&page.doc, &page.inner, None);
        let answer = |space: &[u8], components: &[f64]| {
            res.colorants(
                space,
                components,
                tinker_pdf_content::RenderingIntent::RelativeColorimetric,
            )
        };
        let names = |pairs: &[(&[u8], u8)]| -> Vec<(Vec<u8>, u8)> {
            pairs
                .iter()
                .map(|(name, tint)| (name.to_vec(), *tint))
                .collect()
        };
        let cmyk = |c: u8, m: u8, y: u8, k: u8| {
            names(&[(b"Cyan", c), (b"Magenta", m), (b"Yellow", y), (b"Black", k)])
        };

        // DeviceCMYK, under the scope's `/DefaultCMYK`: its own components.
        assert!(res.device_defaults().cmyk, "the default is in force");
        let device = answer(b"DeviceCMYK", &[0.1, 0.2, 0.3, 0.4]).expect("DeviceCMYK's");
        assert_eq!(device.kind, ColorantKind::DirectCmyk);
        assert_eq!(tints_by_name(&device), cmyk(26, 51, 77, 102));
        assert_eq!(device.alternate, None, "its own ink, not an alternate's");

        // A `/Separation`, into DeviceCMYK and into DeviceRGB.
        let gold = answer(b"Sep", &[0.6]).expect("a separation's");
        assert_eq!(gold.kind, ColorantKind::Colorants);
        assert_eq!(tints_by_name(&gold), names(&[(b"Gold", 153)]));
        assert_eq!(
            gold.alternate,
            Some([0, 77, 153, 0]),
            "the alternate's own components"
        );
        let teal = answer(b"SepRgb", &[1.0]).expect("a separation's");
        assert_eq!(tints_by_name(&teal), names(&[(b"Teal", 255)]));
        assert_eq!(
            teal.alternate,
            Some(ink_of(0, 128, 128)),
            "the alternate's light as ink"
        );

        // A `/DeviceN` naming a process colorant, `/None` discarded.
        let device_n = answer(b"DNp", &[0.4, 0.8, 0.3]).expect("a DeviceN's");
        assert_eq!(device_n.kind, ColorantKind::Colorants);
        assert_eq!(
            tints_by_name(&device_n),
            names(&[(b"Cyan", 102), (b"Spot1", 204)])
        );
        assert_eq!(device_n.alternate, Some([102, 0, 0, 51]));

        // `/All`, `/None` and a `/DeviceN` of `/None` only.
        let all = answer(b"All", &[0.25]).expect("/All's");
        assert_eq!(all.kind, ColorantKind::All);
        assert_eq!(tints_by_name(&all), names(&[(b"All", 64)]));
        assert_eq!(all.alternate, None, "/All never reverts");
        for none in [b"None".as_slice(), b"NoneN".as_slice()] {
            let answered = answer(none, &[1.0, 1.0]).expect("/None's");
            assert_eq!(answered.kind, ColorantKind::None);
            assert!(answered.tints.is_empty());
            assert_eq!(answered.alternate, None);
        }

        // An `NChannel` space over a CMYK `/Process`.
        let n_cmyk = answer(b"NCmyk", &[0.2, 0.4, 0.6]).expect("an NChannel's");
        assert_eq!(n_cmyk.kind, ColorantKind::NChannel);
        assert_eq!(
            tints_by_name(&n_cmyk),
            names(&[(b"Cyan", 51), (b"Spot2", 102), (b"Black", 153)]),
            "the process components under their colorants' names"
        );
        let alone: Vec<Option<[u8; 4]>> = n_cmyk.tints.iter().map(|tint| tint.alone).collect();
        assert_eq!(
            alone,
            vec![None, Some([0, 0, 102, 0]), None],
            "the spot alone"
        );
        assert_eq!(n_cmyk.alternate, None);
        assert_eq!(
            n_cmyk.process, None,
            "a CMYK process space converts nothing"
        );

        // An `NChannel` space over an RGB `/Process`, whole and in part.
        let n_rgb = answer(b"NRgb", &[1.0, 0.5, 0.0, 0.8]).expect("an NChannel's");
        assert_eq!(tints_by_name(&n_rgb), names(&[(b"Red", 204)]));
        assert_eq!(n_rgb.tints[0].alone, Some(ink_of(255, 51, 51)), "Red alone");
        assert_eq!(
            n_rgb.process,
            Some(ink_of(255, 128, 0)),
            "the RGB components together"
        );
        let part = answer(b"NRgbPart", &[0.6, 0.0]).expect("an NChannel's");
        assert_eq!(
            part.process,
            Some(ink_of(153, 0, 0)),
            "the components left out at zero"
        );
        assert_eq!(
            part.tints[0].alone,
            Some(ink_of(255, 255, 255)),
            "no red at all"
        );

        // An `/Indexed` colour is its palette entry in the base.
        let indexed = answer(b"Idx", &[1.0]).expect("its base's");
        assert_eq!(
            tints_by_name(&indexed),
            names(&[(b"Gold", 128), (b"Silver", 64)])
        );
        let first = answer(b"Idx", &[0.0]).expect("its base's");
        assert_eq!(
            tints_by_name(&first),
            names(&[(b"Gold", 0), (b"Silver", 255)])
        );

        // An uncoloured pattern's components, in its underlying space.
        let patterned = answer(b"Pat", &[0.6]).expect("its base's");
        assert_eq!(tints_by_name(&patterned), names(&[(b"Gold", 153)]));

        // A space naming no colorant, and one whose names cannot be read.
        assert_eq!(answer(b"DeviceRGB", &[1.0, 0.0, 0.0]), None);
        assert_eq!(answer(b"DeviceGray", &[0.5]), None);
        assert_eq!(answer(b"Cal", &[0.1, 0.2, 0.3]), None);
        assert_eq!(answer(b"Bad", &[0.5, 0.5]), None);
        assert_eq!(answer(b"Missing", &[0.5]), None);
    }

    /// 8.7.3.2: `[/Pattern base]` is the uncoloured pattern space, and `base`
    /// is where the operands of an `scn` that names a `PaintType 2` pattern
    /// are measured.
    ///
    /// The array form resolved to nothing at all before this, so those
    /// operands could not be read in any space and an uncoloured pattern had
    /// no colour to take. The two spaces below read the same three numbers
    /// differently, so a binder that fell back to RGB for both fails on the
    /// second.
    #[test]
    fn an_uncoloured_pattern_space_resolves_through_its_underlying_space() {
        use tinker_pdf_content::RenderingIntent::RelativeColorimetric;
        let res = resources_of(
            "/Prgb [ /Pattern /DeviceRGB ] \
             /Pcmyk [ /Pattern /DeviceCMYK ] \
             /Pbare /Pattern",
        );

        assert_eq!(res.color_components(b"Prgb"), Some(3));
        assert_eq!(
            res.resolve_color(b"Prgb", &[1.0, 0.0, 0.0], RelativeColorimetric),
            Some(tinker_pdf_content::Rgb { r: 255, g: 0, b: 0 }),
        );

        assert_eq!(res.color_components(b"Pcmyk"), Some(4));
        assert_eq!(
            res.resolve_color(b"Pcmyk", &[1.0, 0.0, 0.0, 0.0], RelativeColorimetric),
            Some(tinker_pdf_content::Rgb {
                r: 0,
                g: 255,
                b: 255
            }),
            "cyan, not red — the operands are inks here"
        );

        // 8.7.3.1: a *coloured* pattern space has no underlying space and no
        // colour of its own, which is the black every pattern used to paint.
        assert_eq!(
            res.resolve_color(b"Pbare", &[1.0, 0.0, 0.0], RelativeColorimetric),
            Some(tinker_pdf_content::Rgb { r: 0, g: 0, b: 0 }),
        );
    }

    /// A page's resources built over `DocumentEditor::view` resolve an object
    /// the editor has only just allocated; built over the editor's own
    /// document they cannot, because that document never had it.
    ///
    /// This is the resolution path a redaction that copies a form per
    /// placement needs: `PageResources` holds an `Arc<CosDocument>`, and the
    /// view is one in which the copy exists at the number the editor gave it.
    #[test]
    fn resources_over_an_editors_view_resolve_what_it_allocated() {
        use std::sync::Arc;
        use tinker_pdf_cos::{
            pages, CosDocument, Dict, DocumentBuilder, DocumentEditor, Name, Object, StreamData,
        };

        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |page| {
            page.fill_rect(10.0, 10.0, 20.0, 20.0, 0.0);
        });
        let doc = Arc::new(
            CosDocument::open({
                let bytes = builder.finish();
                crate::subset::tests_support::written_support::authored(
                    "resources over an editors view resolve what it allocated",
                    &bytes,
                );
                bytes
            })
            .expect("it opens"),
        );
        let mut editor = DocumentEditor::new(Arc::clone(&doc));

        let form = editor.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(editor.intern(b"XObject")));
        dict.insert(
            editor.intern(b"Subtype"),
            Object::Name(editor.intern(b"Form")),
        );
        dict.insert(
            editor.intern(b"BBox"),
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Int(10),
                Object::Int(10),
            ]),
        );
        editor.put_stream(
            form,
            StreamData {
                dict,
                data: b"0 0 1 rg 0 0 10 10 re f".to_vec(),
            },
        );
        let page_ref = editor.page_refs()[0];
        let Some(Object::Dict(mut page)) = editor.get(page_ref) else {
            panic!("a page dictionary");
        };
        let mut resources = page.get_dict(Name::RESOURCES).cloned().unwrap_or_default();
        let mut xobjects = Dict::new();
        xobjects.insert(editor.intern(b"Fm9"), Object::Ref(form));
        resources.insert(editor.intern(b"XObject"), Object::Dict(xobjects));
        page.insert(Name::RESOURCES, Object::Dict(resources));
        editor.put(page_ref, Object::Dict(page));

        let view = editor.view().expect("the view opens");
        let viewed = &pages::collect(&view)[0];

        let over_view = PageResources::new(&view, viewed, None);
        let (form_dict, reference) = over_view.xobject(b"Fm9").expect("the form resolves");
        assert_eq!(reference, form);
        assert_eq!(
            form_dict
                .get_name(view.intern(b"Subtype"))
                .and_then(|n| view.name_bytes(n))
                .as_deref(),
            Some(b"Form".as_slice())
        );

        // The same page dictionary over the document the editor was opened
        // with names the form and cannot reach it.
        let over_original = PageResources::new(&editor.shared_document(), viewed, None);
        assert!(over_original.xobject(b"Fm9").is_none());
    }

    /// P-02.4's record of a Type 3 font's procedures, held to the walk it
    /// replaced and to one reading a scope.
    mod type3_record {
        use std::cell::Cell;

        use super::super::{type3_codes, PageResources, DIFFERENCES_READ};
        use crate::{Document, RenderOptions};
        use tinker_pdf_content::FontSource;
        use tinker_pdf_cos::{CosDocument, Name, ObjRef, Object};

        /// The walk [`type3_codes`] replaced, kept to hold the record to: for
        /// one code, the array read from its first item to the first name at
        /// that code, every item resolved before it is read.
        fn walk_differences(doc: &CosDocument, differences: &[Object], code: u32) -> Option<Name> {
            let mut current = 0u32;
            for item in differences {
                match doc.resolve(item).as_ref() {
                    Object::Int(v) => current = u32::try_from(*v).unwrap_or(0),
                    Object::Real(v) => current = *v as u32,
                    Object::Name(n) => {
                        if current == code {
                            return Some(*n);
                        }
                        current = current.saturating_add(1);
                    }
                    _ => {}
                }
            }
            None
        }

        /// A document whose objects 3 to 7 are what a `/Differences` array's
        /// indirect items point at: an integer, a negative integer, a real, a
        /// name, and a reference to the integer, which resolves through two.
        fn differences_document() -> CosDocument {
            let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
3 0 obj\n40\nendobj\n\
4 0 obj\n-3\nendobj\n\
5 0 obj\n7.9\nendobj\n\
6 0 obj\n/Indirect\nendobj\n\
7 0 obj\n3 0 R\nendobj\n\
trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n";
            CosDocument::open(bytes.to_vec()).expect("it opens")
        }

        /// One `/Differences` array from a seeded generator, of what the
        /// reading has to tell apart: integers small, negative and past
        /// `u32`, at and below `u32::MAX`; reals that truncate, are negative,
        /// NaN or past the range; names that repeat; every indirect item
        /// above; and items of a type the reading passes over.
        fn differences(doc: &CosDocument, seed: &mut u64) -> Vec<Object> {
            let mut next = || {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 7;
                *seed ^= *seed << 17;
                *seed
            };
            let names = [b"a".as_slice(), b"b", b"space", b"x", b"Indirect"];
            let length = (next() % 40) as usize;
            (0..length)
                .map(|_| match next() % 12 {
                    0 => Object::Int((next() % 300) as i64),
                    1 => Object::Int(-((next() % 5) as i64) - 1),
                    2 => Object::Int(i64::from(u32::MAX) + 1 + (next() % 3) as i64),
                    3 => Object::Real((next() % 3000) as f64 / 10.0),
                    4 => Object::Real(-((next() % 50) as f64) / 7.0),
                    5 => Object::Real(if next() % 2 == 0 { f64::NAN } else { 5.0e9 }),
                    6 => Object::Ref(ObjRef::new(3 + (next() % 5) as u32, 0)),
                    7 => Object::Int(i64::from(u32::MAX) - (next() % 2) as i64),
                    8 => Object::Bool(true),
                    _ => Object::Name(doc.intern(names[(next() % 5) as usize])),
                })
                .collect()
        }

        /// **P-02.4**: the record gives every code the name the walk it
        /// replaced gives — over generated arrays with repeated codes,
        /// negative and real counters, counters at and past `u32::MAX` and
        /// indirect items — and no name to a code the walk gives none.
        #[test]
        fn the_type3_table_answers_what_the_walk_answered() {
            let doc = differences_document();
            let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
            let mut named = 0usize;
            for _ in 0..2_000 {
                let items = differences(&doc, &mut seed);
                let table = type3_codes(&doc, &items);
                let mut codes: Vec<u32> = (0..=300).collect();
                codes.extend([u32::MAX - 2, u32::MAX - 1, u32::MAX]);
                codes.extend(table.keys().copied());
                for code in codes {
                    let walked = walk_differences(&doc, &items, code);
                    assert_eq!(
                        table.get(&code).copied(),
                        walked,
                        "code {code} of {items:?}"
                    );
                    named += usize::from(walked.is_some());
                }
            }
            assert!(named > 2_000, "the arrays name codes: {named}");
        }

        /// A page of a Type 3 font whose `/Differences` holds `items` items —
        /// a counter and a name for each of its codes — showing every code
        /// twenty times.
        fn type3_page(items: usize) -> Document {
            let glyphs = items - 1;
            let mut differences = String::from("0");
            let mut procs = String::new();
            for at in 0..glyphs {
                differences.push_str(&format!(" /g{at}"));
                procs.push_str(&format!(" /g{at} 6 0 R"));
            }
            let mut shown = String::new();
            for _ in 0..20 {
                shown.push('<');
                for code in 0..glyphs {
                    shown.push_str(&format!("{code:02X}"));
                }
                shown.push_str("> Tj ");
            }
            let content = format!("BT /T3 10 Tf 10 10 Td {shown}ET");
            let font = format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
                 /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs <<{procs} >>\n\
                 /Encoding << /Type /Encoding /Differences [{differences}] >>\n\
                 /FirstChar 0 /LastChar {last} /Widths [{widths}] >>",
                last = glyphs - 1,
                widths = " 500".repeat(glyphs)
            );
            let procedure = "500 0 d0 0 0 400 400 re f";
            let bytes = format!(
                "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
   /Resources << /Font << /T3 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n{font}\nendobj\n\
6 0 obj\n<< /Length {} >>\nstream\n{procedure}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
                content.len(),
                procedure.len()
            );
            Document::open(bytes.into_bytes()).expect("it opens")
        }

        /// **P-02.4**: a scope reads its Type 3 font's `/Differences` once, on
        /// the font's first glyph, however many glyphs it is asked about — the
        /// array's length a scope, and never more — and a render over its
        /// caches reads none of it again; another scope over the same
        /// dictionary reads it once for itself, and so do the page's text and
        /// its render, one scope each.
        #[test]
        fn differences_are_read_once_per_scope() {
            const ITEMS: usize = 60;
            let read = || DIFFERENCES_READ.with(Cell::get);
            let reset = || DIFFERENCES_READ.with(|read| read.set(0));
            let document = type3_page(ITEMS);
            let page = document.page(0).expect("its page");

            reset();
            let scope = PageResources::new(&page.doc, &page.inner, None);
            let mut drawn = 0;
            for _ in 0..20 {
                for code in 0..ITEMS as u32 + 2 {
                    drawn += usize::from(scope.type3_glyph(b"T3", code).is_some());
                }
            }
            assert_eq!(drawn, 20 * (ITEMS - 1), "every code the array names draws");
            assert_eq!(read(), ITEMS, "the array read once, item by item");
            let again = scope.for_one_render();
            assert!(again.type3_glyph(b"T3", 0).is_some());
            assert_eq!(read(), ITEMS, "a render over the same caches reads nothing");

            reset();
            let other = PageResources::new(&page.doc, &page.inner, None);
            assert!(other.type3_glyph(b"T3", 3).is_some());
            assert_eq!(read(), ITEMS, "another scope reads it once for itself");

            reset();
            let _ = page.text();
            assert_eq!(read(), ITEMS, "a text extraction, one scope");
            reset();
            let _ = page.render(&RenderOptions::default());
            assert_eq!(read(), ITEMS, "a render, one scope");
        }
    }

    /// `type1_selection`'s table (`docs/design/subsetting.md`, *Which glyphs a
    /// Type 1 code selects*), row by row, over a program whose built-in
    /// encoding puts `A` at 65, `B` at 66 and `ornament` at 70 and which also
    /// carries `C`, read through four font dictionaries.
    #[test]
    fn a_type1_code_is_selected_as_the_design_tables_it() {
        use super::{type1_selection, Selection};
        use tinker_pdf_cos::{font as cos_font, CosDocument};
        use tinker_pdf_font::type1::{encrypt, CHARSTRING_KEY, EEXEC_KEY};
        use tinker_pdf_font::Type1;

        // `0 500 hsbw endchar` for every glyph: the names are what is read.
        let charstring = encrypt(&[139, 248, 136, 13, 14], CHARSTRING_KEY, &[0; 4]);
        let mut private = b"/CharStrings 5 dict dup begin\n".to_vec();
        for name in [".notdef", "A", "B", "C", "ornament"] {
            private.extend_from_slice(format!("/{name} {} RD ", charstring.len()).as_bytes());
            private.extend_from_slice(&charstring);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(b"end\nmark currentfile closefile\n");
        let mut program = b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n\
            /Encoding 256 array\ndup 65 /A put\ndup 66 /B put\ndup 70 /ornament put\n\
            readonly def\ncurrentfile eexec\n"
            .to_vec();
        program.extend(encrypt(&private, EEXEC_KEY, &[0; 4]));
        let type1 = Type1::parse(&program).expect("the program parses");
        let glyph = |name: &[u8]| type1.glyph_for_name(name).expect("in the program");
        let (a, b, c, ornament) = (glyph(b"A"), glyph(b"B"), glyph(b"C"), glyph(b"ornament"));

        let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
            1 begincodespacerange <00> <FF> endcodespacerange \
            1 beginbfchar <41> <0042> endbfchar endcmap end end";
        let mut bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
            2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n"
            .to_vec();
        bytes.extend_from_slice(
            format!("3 0 obj\n<< /Length {} >>\nstream\n", cmap.len()).as_bytes(),
        );
        bytes.extend_from_slice(cmap);
        bytes.extend_from_slice(
            b"\nendstream\nendobj\n\
            4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /F >>\nendobj\n\
            5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /F \
            /Encoding << /Differences [65 /B 66 /missing 71 /missing] >> >>\nendobj\n\
            6 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /F /ToUnicode 3 0 R >>\nendobj\n\
            7 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /F \
            /Encoding << /Differences [65 /missing] >> /ToUnicode 3 0 R >>\nendobj\n\
            trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        );
        let doc = CosDocument::open(bytes).expect("it opens");
        let font = |number: u32| {
            let object = doc.get(ObjRef::new(number, 0)).expect("the font");
            cos_font::read(&doc, object.as_dict().expect("a dictionary"))
        };
        let stated = |glyph| Some(Selection::stated(glyph));
        let guessed = |glyph| Some(Selection::guessed(glyph));

        // No `/Encoding`: the text and the built-in encoding agree on A; the
        // built-in encoding alone reaches 70; the text alone reaches C; and
        // nothing reaches 71.
        let plain = font(4);
        assert_eq!(type1_selection(&type1, &plain, 65), stated(a));
        assert_eq!(type1_selection(&type1, &plain, 70), stated(ornament));
        assert_eq!(type1_selection(&type1, &plain, 67), guessed(c));
        assert_eq!(type1_selection(&type1, &plain, 71), None);

        // `/Differences`: a name the program has is drawn; a name it lacks
        // falls through to the built-in encoding as `cff_glyph` falls
        // through, or reaches nothing.
        let differences = font(5);
        assert_eq!(type1_selection(&type1, &differences, 65), stated(b));
        assert_eq!(type1_selection(&type1, &differences, 66), stated(b));
        assert_eq!(type1_selection(&type1, &differences, 71), None);

        // `/ToUnicode` says 65 is B: drawn by its text, the built-in
        // encoding's A kept beside it.
        let unicode = font(6);
        assert_eq!(
            type1_selection(&type1, &unicode, 65),
            Some(Selection {
                glyph: b,
                stated: true,
                also: Some(a),
            })
        );

        // A name the program lacks, and the text reaching B: a guess.
        let guessing = font(7);
        assert_eq!(type1_selection(&type1, &guessing, 65), guessed(b));
    }
}
