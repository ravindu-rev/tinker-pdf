//! Building documents (phase 12).
//!
//! A thin authoring layer over the writer: pages, content, fonts, metadata and
//! an outline. It deliberately does no layout — placing what a caller already
//! positioned is this crate's business, and composing paragraphs is not.

use std::collections::{BTreeMap, BTreeSet};

use crate::dest::DestKind;
use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, PdfString};
use crate::write::{rewrite, ObjectSet, StreamData, WriteOptions};

/// Image data to embed.
///
/// **`#[non_exhaustive]`**, from gap 29 milestone 4. A caller outside this
/// workspace matches with a wildcard arm, so the next image shape is an
/// addition rather than a break. It is marked here rather than when it is next
/// needed because marking it later costs the same break again for nothing, and
/// gaps 30 and 31 each expect to add one.
#[non_exhaustive]
pub enum ImageData<'a> {
    /// JPEG bytes, placed **as they are**.
    ///
    /// Never re-encoded: recompression is generational quality loss the
    /// caller cannot undo, and one who wants different quality can decode and
    /// re-add.
    Jpeg(&'a [u8]),
    /// Eight-bit RGB, three bytes per pixel, row-major from the top.
    Rgb8 {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The samples.
        data: &'a [u8],
    },
    /// Eight-bit greyscale, one byte per pixel.
    Gray8 {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The samples.
        data: &'a [u8],
    },
    /// Bytes that are **already** in the encoding their dictionary declares.
    ///
    /// [`ImageData::Jpeg`] is this idea for one codec: place the bytes, name
    /// the filter, do no pixel work. This generalises it, and gap 29 is why —
    /// a CBZ synthesises every page at open, so whatever a page holds is held
    /// for the whole document. Decoding each PNG to [`ImageData::Rgb8`] would
    /// cost *w x h x 3* a page, about 3.6 GB for a 200-page archive at
    /// 2000 x 3000; passing the compressed bytes through keeps the peak a
    /// small multiple of the archive's own size, and that multiple is a
    /// constant rather than a function of the pixel count.
    ///
    /// It works at all because a non-interlaced PNG's IDAT **is** a
    /// `/FlateDecode` stream with `/Predictor 15`, byte for byte: PDF's
    /// `/Predictor` is PNG 9.2's row-filter specification adopted wholesale,
    /// down to the per-row tag and the `ceil(colors x bpc / 8)` left-neighbour
    /// offset floored at one.
    ///
    /// The writer never re-encodes these bytes, and that is a contract rather
    /// than an accident: `maybe_compress` declines any stream whose dictionary
    /// already declares a `/Filter`, and
    /// `a_stream_that_already_declares_a_filter_is_handed_through_untouched`
    /// holds it there.
    Compressed(CompressedImage<'a>),
}

/// A device colour space, which is all an `/Indexed` base may be here.
///
/// 8.6.6.3 forbids an `/Indexed` whose base is itself `/Indexed`, and this
/// writer emits no CIE-based, `/Separation` or `/DeviceN` space — so the three
/// device families are the whole of it, and the restriction is the
/// specification's rather than an invention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSpace {
    /// `/DeviceGray`.
    Gray,
    /// `/DeviceRGB`.
    Rgb,
    /// `/DeviceCMYK`.
    Cmyk,
}

impl DeviceSpace {
    /// Colour components per sample: 8.6.4's own counts.
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            DeviceSpace::Gray => 1,
            DeviceSpace::Rgb => 3,
            DeviceSpace::Cmyk => 4,
        }
    }

    const fn pdf_name(self) -> &'static [u8] {
        match self {
            DeviceSpace::Gray => b"DeviceGray",
            DeviceSpace::Rgb => b"DeviceRGB",
            DeviceSpace::Cmyk => b"DeviceCMYK",
        }
    }
}

/// The `/ColorSpace` of a [`CompressedImage`] — the set this writer can emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageColorSpace<'a> {
    /// `/DeviceGray`.
    DeviceGray,
    /// `/DeviceRGB`.
    DeviceRgb,
    /// `/DeviceCMYK`.
    DeviceCmyk,
    /// `[/Indexed base hival lookup]` (8.6.6.3).
    Indexed {
        /// The space each table entry is expressed in.
        base: DeviceSpace,
        /// The table, `base.components()` bytes an entry, from index zero.
        ///
        /// `/hival` is **derived from this length** rather than carried
        /// separately. A `/hival` that disagrees with the table it describes is
        /// exactly how a reader ends up indexing past the end of one, and there
        /// is no legitimate document in which the two differ.
        lookup: &'a [u8],
    },
}

impl ImageColorSpace<'_> {
    /// Components in one *sample* of this space.
    ///
    /// An `/Indexed` sample is a single index whatever its base is (8.6.6.3),
    /// which is also what `/DecodeParms /Colors` must say for it.
    #[must_use]
    pub const fn components(&self) -> u32 {
        match self {
            ImageColorSpace::DeviceGray | ImageColorSpace::Indexed { .. } => 1,
            ImageColorSpace::DeviceRgb => 3,
            ImageColorSpace::DeviceCmyk => 4,
        }
    }
}

/// A filter **already applied** to the bytes handed over with it.
///
/// Not a request to encode: the `/Filter` and `/DecodeParms` this names are
/// written into the dictionary and the bytes are placed unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFilter {
    /// `/DCTDecode`, which carries no `/DecodeParms`.
    Dct,
    /// `/FlateDecode` over the samples directly, with no `/DecodeParms`.
    Flate,
    /// `/FlateDecode` with
    /// `/DecodeParms << /Predictor 15 /Colors c /BitsPerComponent b /Columns w >>`.
    ///
    /// 7.4.4.4's predictor 15 is "PNG optimum": every row carries its own
    /// filter tag, which is what PNG 9.2 writes and what makes a PNG's IDAT
    /// legible to a PDF reader without a byte being touched.
    ///
    /// The three parameters describe **the bytes**, not the image, which is why
    /// they are carried rather than derived — but a set that disagrees with the
    /// image's own geometry describes a different raster from the one the
    /// dictionary declares, so [`DocumentBuilder::add_image`] refuses it.
    FlatePngPredictor {
        /// `/Colors`: components per sample in the *encoded* data.
        colors: u32,
        /// `/BitsPerComponent`, as the predictor saw them.
        bits_per_component: u32,
        /// `/Columns`: samples per row.
        columns: u32,
    },
}

/// Per-sample opacity, as the `/DeviceGray` sub-image 11.6.5.3 asks for.
///
/// Its colour space is not a field: `/SMask` *is* `/DeviceGray`, and a mask in
/// any other space is not a mask.
pub struct SoftMask<'a> {
    /// Width in samples. Need not match the image's; a reader scales it.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// 1, 2, 4, 8 or 16.
    pub bits_per_component: u8,
    /// The filter already applied to `data`, or `None` for raw samples.
    pub filter: Option<ImageFilter>,
    /// The bytes, encoded as `filter` says.
    pub data: &'a [u8],
}

/// An image whose bytes are already encoded — see [`ImageData::Compressed`].
pub struct CompressedImage<'a> {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// 1, 2, 4, 8 or 16 (Table 89). An `/Indexed` image may not use 16,
    /// because 8.6.6.3 caps `/hival` at 255.
    pub bits_per_component: u8,
    /// The `/ColorSpace`.
    pub color_space: ImageColorSpace<'a>,
    /// The filter already applied to `data`, or `None` for raw samples — in
    /// which case the writer's ordinary compression applies.
    pub filter: Option<ImageFilter>,
    /// The bytes, encoded as `filter` says.
    pub data: &'a [u8],
    /// 8.9.6.4 colour-key masking: one inclusive `(min, max)` range per colour
    /// component, compared against **raw sample values**, before `/Decode`.
    ///
    /// A sample inside every one of its ranges is not painted. This is a range
    /// test rather than a lookup, which is precisely what a PNG `tRNS` naming
    /// one fully transparent colour or index needs and precisely what a `tRNS`
    /// giving partial alpha to a palette cannot use.
    pub color_key_mask: Option<&'a [(u32, u32)]>,
    /// 11.6.5.3 `/SMask`, written as its own image XObject.
    pub soft_mask: Option<SoftMask<'a>>,
}

/// Table 89: the five depths an image sample may have.
const fn is_legal_depth(bits: u8) -> bool {
    matches!(bits, 1 | 2 | 4 | 8 | 16)
}

/// The largest raw value a sample of this depth can hold.
const fn max_sample(bits: u8) -> u32 {
    // `bits` is one of Table 89's five, so the shift is at most 16.
    (1u32 << bits) - 1
}

/// Whether a filter's declared `/DecodeParms` describe the image carrying it.
///
/// A `/Columns` that is not the width unfilters every row at the wrong stride
/// and produces a picture that is scrambled rather than absent — the shape of
/// failure that reads as a decoder bug and gets found late. There is no
/// document in which these three legitimately differ from the image's own
/// geometry, so a disagreement is refused rather than written out.
fn filter_describes(filter: Option<ImageFilter>, components: u32, bits: u8, width: u32) -> bool {
    match filter {
        Some(ImageFilter::FlatePngPredictor {
            colors,
            bits_per_component,
            columns,
        }) => colors == components && bits_per_component == u32::from(bits) && columns == width,
        _ => true,
    }
}

// ---- the graphics state, groups, gradients and patterns -----------------
//
// Everything from here to `Embedded` is the writer catching up with what this
// engine has read since gaps 09, 10 and 11: tiling patterns, shadings and
// transparency groups each have a reader in this repository and had no writer
// at all. A caller wanting an opacity, a gradient, a tiling brush or a glyph
// addressed by index had exactly one door — `PageBuilder::raw`, which writes
// operators and cannot name a resource — so none of it was reachable.

/// 11.3.5's blend modes, as `/BM` spells them.
///
/// All sixteen: Table 136's twelve separable modes and Table 137's four
/// non-separable ones. The set is closed and PDF 2.0 adds none, so this is
/// exhaustive on purpose and a caller may match it without a wildcard arm.
/// `/Compatible` is deliberately absent: 11.3.5 makes it a synonym for
/// `/Normal` kept for 1.3 files, and offering two spellings of one mode would
/// be a choice with no consequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// `/Normal`.
    Normal,
    /// `/Multiply`.
    Multiply,
    /// `/Screen`.
    Screen,
    /// `/Overlay`.
    Overlay,
    /// `/Darken`.
    Darken,
    /// `/Lighten`.
    Lighten,
    /// `/ColorDodge`.
    ColorDodge,
    /// `/ColorBurn`.
    ColorBurn,
    /// `/HardLight`.
    HardLight,
    /// `/SoftLight`.
    SoftLight,
    /// `/Difference`.
    Difference,
    /// `/Exclusion`.
    Exclusion,
    /// `/Hue`.
    Hue,
    /// `/Saturation`.
    Saturation,
    /// `/Color`.
    Color,
    /// `/Luminosity`.
    Luminosity,
}

impl BlendMode {
    const fn pdf_name(self) -> &'static [u8] {
        match self {
            BlendMode::Normal => b"Normal",
            BlendMode::Multiply => b"Multiply",
            BlendMode::Screen => b"Screen",
            BlendMode::Overlay => b"Overlay",
            BlendMode::Darken => b"Darken",
            BlendMode::Lighten => b"Lighten",
            BlendMode::ColorDodge => b"ColorDodge",
            BlendMode::ColorBurn => b"ColorBurn",
            BlendMode::HardLight => b"HardLight",
            BlendMode::SoftLight => b"SoftLight",
            BlendMode::Difference => b"Difference",
            BlendMode::Exclusion => b"Exclusion",
            BlendMode::Hue => b"Hue",
            BlendMode::Saturation => b"Saturation",
            BlendMode::Color => b"Color",
            BlendMode::Luminosity => b"Luminosity",
        }
    }
}

/// What a graphics-state soft mask derives its alpha from (11.6.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskKind {
    /// `/S /Alpha`: the alpha the group's own painting produced.
    Alpha,
    /// `/S /Luminosity`: the luminosity of the colour the group painted,
    /// composited first against the backdrop `/BC` names.
    Luminosity,
}

impl MaskKind {
    const fn pdf_name(self) -> &'static [u8] {
        match self {
            MaskKind::Alpha => b"Alpha",
            MaskKind::Luminosity => b"Luminosity",
        }
    }
}

/// An `/SMask` entry in a graphics state (11.6.5.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StateMask<'a> {
    /// `/SMask /None`.
    ///
    /// Not the same as leaving `/SMask` out. An absent entry inherits whatever
    /// mask is already in force; `/None` turns it off, which is the only way
    /// to stop one inside a `q`/`Q` pair that did not open it.
    None,
    /// A mask built from a transparency group.
    Group {
        /// `/S`.
        kind: MaskKind,
        /// `/G`: the resource name of a form XObject registered with
        /// [`DocumentBuilder::add_form`].
        ///
        /// 11.6.5.2 requires it to be a **transparency group** XObject, so a
        /// name registered without a [`TransparencyGroup`] is refused rather
        /// than written out — a mask over a plain form is a mask a reader
        /// cannot evaluate, and a page that silently failed to be masked looks
        /// exactly like a page nobody meant to mask.
        form: &'a [u8],
        /// `/BC`, the backdrop colour, in the **mask group's own** colour
        /// space — so its length is that group's component count and not the
        /// page's.
        ///
        /// Only meaningful for [`MaskKind::Luminosity`]; 11.6.5.2 says a
        /// luminosity mask composites its group against a backdrop and that
        /// this entry names it, defaulting to black.
        backdrop: Option<&'a [f64]>,
    },
}

/// Graphics state parameters, written as an `/ExtGState` (Table 58).
///
/// Every field is an `Option` because Table 58 is a *set of overrides*: an
/// absent entry leaves the parameter as it was, and writing a value equal to
/// the default is not the same document as writing nothing. Build one with
/// `..ExtGState::default()` so a field added later does not break the call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExtGState<'a> {
    /// `/ca`, the **non-stroking** alpha constant (11.6.4.4).
    ///
    /// Fill alpha and stroke alpha are two parameters, not one spelled twice:
    /// a rule stroked at 0.5 around a shape filled at 1.0 is an ordinary thing
    /// to draw, and a writer that set both from one number could not say it.
    pub fill_alpha: Option<f64>,
    /// `/CA`, the **stroking** alpha constant (11.6.4.4).
    pub stroke_alpha: Option<f64>,
    /// `/BM`.
    pub blend_mode: Option<BlendMode>,
    /// `/SMask`.
    pub soft_mask: Option<StateMask<'a>>,
}

/// A form XObject's `/Group` entry (11.6.6): a transparency group.
///
/// `isolated` and `knockout` are **independent** flags over one group and
/// 11.6.6 gives them different jobs — one decides what the group composites
/// against (its own initial backdrop, or the page's), the other decides what
/// each element inside it composites against (the group's initial backdrop, or
/// the result accumulated so far). All four combinations are meaningful, so
/// neither can be derived from the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransparencyGroup {
    /// `/CS`, the group's blending colour space.
    pub color_space: DeviceSpace,
    /// `/I`: isolated.
    pub isolated: bool,
    /// `/K`: knockout.
    pub knockout: bool,
}

/// A form XObject (8.10).
pub struct FormXObject<'a> {
    /// `/BBox`, as `[x0 y0 x1 y1]` in the form's own space.
    ///
    /// Not normalised here. 8.10.2 makes a reader normalise it and this
    /// crate's own reader does — but a degenerate box clips the whole form
    /// away, producing a form that draws nothing while looking registered, so
    /// `x1 > x0` and `y1 > y0` are required rather than repaired.
    pub bbox: [f64; 4],
    /// `/Matrix`, mapping form space into the space of whatever invokes it, or
    /// `None` for the identity.
    pub matrix: Option<[f64; 6]>,
    /// `/Group`, or `None` for a form that is not a transparency group.
    pub group: Option<TransparencyGroup>,
    /// The content stream, in operator bytes.
    pub content: &'a [u8],
}

/// A PDF function (7.10), in the two types a gradient is built from.
///
/// **`#[non_exhaustive]`**: 7.10 defines four types and this writer emits two.
/// Sampled (type 0) and PostScript calculator (type 4) functions are additions
/// a later gap can make without breaking a caller that matched with a wildcard.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Function {
    /// Type 2, exponential interpolation (7.10.3).
    Exponential {
        /// `/Domain`, as `[t0 t1]`.
        domain: [f64; 2],
        /// `/C0`, the value at the domain's start. One number per output.
        c0: Vec<f64>,
        /// `/C1`, the value at the domain's end.
        c1: Vec<f64>,
        /// `/N`, the interpolation exponent. One is a straight ramp.
        n: f64,
    },
    /// Type 3, stitching (7.10.4).
    ///
    /// Where a gradient with more than two stops lives. It can be wrong in two
    /// independent ways — in the **stitch** (`bounds` and `encode`, which
    /// decide where each sub-function starts and how its own domain is
    /// reached) and in a **sub-function** (which decides what colour comes out
    /// once it is reached) — and neither is visible in the other's tests.
    Stitching {
        /// `/Domain`, as `[t0 t1]`.
        domain: [f64; 2],
        /// `/Functions`, in order. One more than `bounds` has entries.
        functions: Vec<Function>,
        /// `/Bounds`: the interior division points, strictly increasing and
        /// strictly inside `domain`.
        bounds: Vec<f64>,
        /// `/Encode`: one `[lo hi]` per sub-function, mapping its subdomain
        /// onto the domain that sub-function itself wants.
        encode: Vec<[f64; 2]>,
    },
}

/// A shading (8.7.4.5), in the two gradient types.
///
/// **`#[non_exhaustive]`**: 8.7.4.5 defines seven and this writer emits two.
/// Function-based (type 1) and the four mesh types have readers in this
/// repository already, so each is an addition rather than a redesign.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Shading {
    /// Type 2, axial (8.7.4.5.3).
    Axial {
        /// `/ColorSpace`.
        color_space: DeviceSpace,
        /// `/Coords`, as `[x0 y0 x1 y1]`: the axis, in shading space.
        coords: [f64; 4],
        /// `/Function`, whose output count is the colour space's.
        function: Function,
        /// `/Extend`, beyond the axis's start and end.
        extend: (bool, bool),
    },
    /// Type 3, radial (8.7.4.5.4).
    ///
    /// Not an axial shading with a radius bolted on: 8.7.4.5.4 blends between
    /// two **circles**, so the geometry is six numbers rather than four and a
    /// writer sharing one code path with type 2 would have nowhere to put the
    /// radii.
    Radial {
        /// `/ColorSpace`.
        color_space: DeviceSpace,
        /// `/Coords`, as `[x0 y0 r0 x1 y1 r1]`: the two circles.
        coords: [f64; 6],
        /// `/Function`.
        function: Function,
        /// `/Extend`, beyond the first and second circle.
        extend: (bool, bool),
    },
}

/// `/TilingType` (Table 75): how a reader may adjust a cell's spacing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TilingType {
    /// 1: constant spacing. Cells are distorted by up to a device pixel so the
    /// spacing is the same everywhere.
    ConstantSpacing,
    /// 2: no distortion. The cell is placed exactly and the spacing varies.
    NoDistortion,
    /// 3: constant spacing and faster tiling. As 1, with more distortion
    /// permitted.
    FasterTiling,
}

/// A tiling pattern, `/PatternType 1` (8.7.3).
///
/// `/PaintType` is always 1, a **coloured** pattern, and the reason is written
/// here rather than left as a gap: an uncoloured pattern is named through a
/// `[/Pattern base]` colour space, which has to be a `/ColorSpace` resource,
/// and this writer has no API for one. Emitting a `/PaintType 2` pattern that
/// no page could then name would be a feature that does not work.
pub struct TilingPattern<'a> {
    /// `/BBox`, the cell, as `[x0 y0 x1 y1]`. Degenerate boxes are refused for
    /// [`FormXObject::bbox`]'s reason.
    pub bbox: [f64; 4],
    /// `/XStep`: the horizontal spacing between cells, which 8.7.3.1 allows to
    /// differ from the cell's own width. Zero is invalid.
    pub x_step: f64,
    /// `/YStep`, likewise.
    pub y_step: f64,
    /// `/Matrix`, mapping pattern space into the **default** coordinate system
    /// of the page the pattern is used on — not into whatever transform is in
    /// force when it is set (8.7.3.1). `None` for the identity.
    pub matrix: Option<[f64; 6]>,
    /// `/TilingType`.
    pub tiling_type: TilingType,
    /// The cell's content stream, in operator bytes.
    pub content: &'a [u8],
}

/// One glyph to draw, addressed by **index**, with the text it stands for.
///
/// This is the pair a simple font cannot carry. A `/TrueType` font with
/// `/WinAnsiEncoding` addresses a glyph by putting a character code in the
/// string and letting the font's `cmap` decide — which is correct exactly
/// while that `cmap` and WinAnsi agree, and produces readable, plausible,
/// wrong text when they do not. Stating the glyph and the text separately is
/// the only shape in which both are said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph<'a> {
    /// The glyph index in the font program.
    ///
    /// Under `/Encoding /Identity-H` with `/CIDToGIDMap /Identity` this one
    /// number is the code in the string, the CID and the glyph index at once,
    /// which is what makes an index addressable at all.
    pub id: u16,
    /// The characters this glyph stands for, for `/ToUnicode`.
    ///
    /// Three shapes are all normal and all supported. Empty: the glyph stands
    /// for nothing extractable. One character: the ordinary case. Several: a
    /// ligature, where one glyph stands for `"ffi"`. And several glyphs may
    /// each name the **same** character, which is what an alternate or a
    /// positional form is.
    pub text: &'a str,
}

/// One glyph of a run the **caller** laid out, for
/// [`DocumentBuilder::glyph_run`] (gap 30, milestone 7).
///
/// [`PageBuilder::glyphs`] draws a run at one position and lets the font's own
/// advances decide where every glyph after the first lands, which is what a
/// caller writing text wants. A caller reading a document that states each
/// glyph's position — an XPS `Glyphs` run does, and 9.4.3 is how PDF says the
/// same thing — has the positions already and needs the advances *not* to
/// decide them.
///
/// Both coordinates are in **unscaled text space**: one unit of whatever space
/// the run's matrix maps from, which is the space 9.4.4's `Tm` translation is
/// stated in. They are not multiplied by the font size, because 9.4.2's
/// displacement is not either.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph<'a> {
    /// The glyph, and the text it stands for.
    pub glyph: Glyph<'a>,
    /// Its origin along the baseline, from the run's own origin.
    pub x: f64,
    /// Its origin off the baseline, positive away from the descenders —
    /// 9.4.3's `Ts`, which displaces the glyph and leaves the pen where it
    /// was.
    pub rise: f64,
}

/// The resources one page, form or pattern may name.
///
/// Held as one value rather than as six fields because a form XObject and a
/// pattern each need the same `/Resources` a page does, and three copies of
/// the assembly would be three places for a resource kind to be forgotten.
#[derive(Clone, Default)]
struct ResourceSet {
    fonts: Vec<(Vec<u8>, ObjRef)>,
    images: Vec<(Vec<u8>, ObjRef)>,
    forms: Vec<(Vec<u8>, ObjRef)>,
    ext_gstates: Vec<(Vec<u8>, ObjRef)>,
    shadings: Vec<(Vec<u8>, ObjRef)>,
    patterns: Vec<(Vec<u8>, ObjRef)>,
}

impl ResourceSet {
    /// The `/Resources` dictionary, carrying the sub-dictionaries that have
    /// anything in them and no others.
    ///
    /// Images and forms share `/XObject`, because 8.8 gives them one
    /// dictionary and tells them apart by `/Subtype`. Forms are written
    /// second, so a form registered under a name an image already holds
    /// replaces it — the same last-registration-wins rule two images under one
    /// name already follow.
    fn dict(&self, names: &NameTable) -> Dict {
        let mut xobjects = Dict::new();
        for (resource, reference) in self.images.iter().chain(self.forms.iter()) {
            xobjects.insert(names.intern(resource), Object::Ref(*reference));
        }

        let mut out = Dict::new();
        for (key, entries) in [
            (&b"Font"[..], &self.fonts),
            (b"ExtGState", &self.ext_gstates),
            (b"Shading", &self.shadings),
            (b"Pattern", &self.patterns),
        ] {
            if entries.is_empty() {
                continue;
            }
            let mut sub = Dict::new();
            for (resource, reference) in entries {
                sub.insert(names.intern(resource), Object::Ref(*reference));
            }
            out.insert(names.intern(key), Object::Dict(sub));
        }
        if !xobjects.is_empty() {
            out.insert(names.intern(b"XObject"), Object::Dict(xobjects));
        }
        out
    }
}

/// Whether a resource name is already registered in a list.
fn holds(entries: &[(Vec<u8>, ObjRef)], resource: &[u8]) -> bool {
    entries.iter().any(|(name, _)| name == resource)
}

/// Whether every number in a slice is finite.
///
/// A `NaN` or an infinity would reach the file as `NaN` or `inf`, which is not
/// a PDF number at all — so it is refused at the door rather than serialised
/// into a token no reader can lex.
fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

/// 11.6.4.4's range for an alpha constant.
///
/// Refused rather than clamped. A caller that asks for 1.5 has made an
/// arithmetic mistake somewhere upstream, and silently writing 1.0 turns it
/// into a picture that is merely wrong instead of a call that failed.
fn is_alpha(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

/// Whether a rectangle encloses an area, in the order `[x0 y0 x1 y1]`.
fn is_box(rect: &[f64; 4]) -> bool {
    all_finite(rect) && rect[2] > rect[0] && rect[3] > rect[1]
}

/// A rectangle with its corners in 7.9.5's order.
///
/// 7.9.5 says a rectangle "should" be written with its corners in lower-left,
/// upper-right order, and calls one written the other way round *normalised*
/// by the consumer. Doing it here means the file says what the specification
/// recommends; leaving it to the consumer means relying on every one of them
/// doing the same optional thing.
fn normalised(rect: [f64; 4]) -> [f64; 4] {
    [
        rect[0].min(rect[2]),
        rect[1].min(rect[3]),
        rect[0].max(rect[2]),
        rect[1].max(rect[3]),
    ]
}

/// How deeply a type 3 function may stitch functions that stitch functions.
///
/// **Not a resource bound, and it stays out of `bounds_ledger.rs` for that
/// reason.** Nothing here is read from a file: a [`Function`] is a value the
/// caller already built in memory, so a constant over it could never be the
/// thing that stopped an attacker — which is gap 18a milestone 8's failure
/// reached from the other side. What it is instead is a **compatibility limit
/// taken from this repository's own reader**, whose `parse_function` stops at
/// depth eight. A writer that emitted a deeper nest would produce a file this
/// engine cannot read back, and a writer whose output its own reader refuses
/// is not a writer. It is checked iteratively, so a caller handing over a nest
/// a thousand deep is refused rather than overflowing a stack finding out.
const MAX_FUNCTION_DEPTH: u32 = 8;

/// The most `<code> <text>` pairs one `beginbfchar` section may hold.
///
/// The CMap specification's own limit, and not a resource bound either: a
/// longer section is a syntax error rather than an expense, so `/ToUnicode` is
/// written in blocks of this size however many glyphs a document draws.
const BFCHAR_PER_SECTION: usize = 100;

impl Function {
    /// Whether this is a function a reader can evaluate, given how many output
    /// components it has to produce.
    ///
    /// Checked without recursion. The tree is the caller's own value and its
    /// depth is the caller's choice, so a recursive validator would be a stack
    /// overflow reachable from a `Vec` push.
    fn is_valid(&self, outputs: usize) -> bool {
        let mut stack = vec![(self, 0u32)];
        while let Some((function, depth)) = stack.pop() {
            if depth > MAX_FUNCTION_DEPTH {
                return false;
            }
            match function {
                Function::Exponential { domain, c0, c1, n } => {
                    if !all_finite(domain) || domain[1] <= domain[0] {
                        return false;
                    }
                    // 7.10.3: x is raised to /N, so a zero or negative
                    // exponent is undefined at the domain's own start rather
                    // than merely unusual.
                    if !n.is_finite() || *n <= 0.0 {
                        return false;
                    }
                    // 7.10.1: the function's range is what the shading's
                    // colour space asks for. A two-component function under
                    // `/DeviceRGB` hands a reader two numbers where it needs
                    // three, and what it invents for the third is its own
                    // business rather than this document's statement.
                    if c0.len() != outputs || c1.len() != outputs {
                        return false;
                    }
                    if !all_finite(c0) || !all_finite(c1) {
                        return false;
                    }
                }
                Function::Stitching {
                    domain,
                    functions,
                    bounds,
                    encode,
                } => {
                    if !all_finite(domain) || domain[1] <= domain[0] {
                        return false;
                    }
                    // 7.10.4's arithmetic, and the half of a gradient that
                    // gets it wrong: k sub-functions want k-1 bounds and k
                    // encode pairs.
                    if functions.is_empty()
                        || bounds.len() + 1 != functions.len()
                        || encode.len() != functions.len()
                    {
                        return false;
                    }
                    if !all_finite(bounds) || encode.iter().any(|e| !all_finite(e)) {
                        return false;
                    }
                    // 7.10.4: Domain0 < Bounds0 < ... < Bounds(k-2) < Domain1,
                    // strictly. An equal pair names an empty subdomain, which
                    // is a sub-function that can never be reached.
                    let mut previous = domain[0];
                    for bound in bounds {
                        if *bound <= previous {
                            return false;
                        }
                        previous = *bound;
                    }
                    if domain[1] <= previous {
                        return false;
                    }
                    for sub in functions {
                        stack.push((sub, depth + 1));
                    }
                }
            }
        }
        true
    }
}

/// A font whose program is embedded, held until `finish`.
///
/// The subset depends on what the document draws, which is not known when the
/// font is registered — so registration reserves object numbers and nothing
/// else.
struct Embedded {
    resource: Vec<u8>,
    base_font: Vec<u8>,
    program: Vec<u8>,
    file_ref: ObjRef,
    descriptor_ref: ObjRef,
    font_ref: ObjRef,
}

/// A composite font whose program is embedded, held until `finish`.
///
/// Five object numbers rather than [`Embedded`]'s three: 9.7.6 makes a Type0
/// font a pair — the font a page names and the **descendant** CIDFont that
/// carries the metrics and the descriptor — and the `/ToUnicode` CMap is a
/// stream of its own.
struct CidFont {
    resource: Vec<u8>,
    base_font: Vec<u8>,
    program: Vec<u8>,
    file_ref: ObjRef,
    descriptor_ref: ObjRef,
    descendant_ref: ObjRef,
    font_ref: ObjRef,
    to_unicode_ref: ObjRef,
}

/// The six-letter tag and plus sign a subset font name carries (9.6.4).
///
/// Derived from the subset's own bytes rather than from a counter or a clock,
/// so the same document written twice produces the same name (ruling 4). Two
/// different subsets of the same face collide only if their bytes hash the
/// same, and a collision would merely give two fonts the same name, which is
/// legal.
fn subset_tag(program: &[u8]) -> Vec<u8> {
    // FNV-1a, for no reason beyond being short and well spread.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in program {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let mut tag = Vec::with_capacity(7);
    for _ in 0..6 {
        tag.push(b'A' + (hash % 26) as u8);
        hash /= 26;
    }
    tag.push(b'+');
    tag
}

/// `/W` for the glyphs a document drew, from the font's own `hmtx` (9.7.4.3).
///
/// Table 116's `c [w1 w2 ...]` form, one run per stretch of consecutive CIDs,
/// so a document drawing glyphs 5, 6, 7 and 40 writes two runs rather than
/// four singletons.
///
/// `None` when the font states no advances at all — no `hmtx`, or an `hhea`
/// claiming no metrics. That is the one case where a width array would have to
/// be **invented**, and 9.7.4.3's `/DW` already says what to do about a CID
/// with no entry. Writing 500 for every glyph, which the simple-font path does
/// because 9.6.6.4 leaves it no choice, would be a number this document made
/// up presented as the font's.
fn width_array(
    sfnt: &tinker_pdf_font::Sfnt<'_>,
    units: f64,
    ids: impl Iterator<Item = u16>,
) -> Option<Vec<Object>> {
    let mut runs: Vec<(u16, Vec<Object>)> = Vec::new();
    for id in ids {
        let advance = sfnt.advance(id)?;
        let width = (f64::from(advance) * 1000.0 / units).round();
        match runs.last_mut() {
            // `ids` arrives from a `BTreeMap`'s keys, so it is sorted and
            // duplicate-free; a run therefore continues exactly when the next
            // id is one more than the last written.
            Some((first, widths)) if usize::from(*first) + widths.len() == usize::from(id) => {
                widths.push(Object::Real(width));
            }
            _ => runs.push((id, vec![Object::Real(width)])),
        }
    }
    if runs.is_empty() {
        return None;
    }

    let mut out = Vec::with_capacity(runs.len() * 2);
    for (first, widths) in runs {
        out.push(Object::Int(i64::from(first)));
        out.push(Object::Array(widths));
    }
    Some(out)
}

/// A `/ToUnicode` CMap for the glyphs a document drew (9.10.3).
///
/// `bfchar` throughout rather than `bfrange`, and deliberately: a range says
/// that consecutive codes map to consecutive characters, which is true of a
/// Latin subset and false of everything else — and a writer that emitted one
/// on the evidence of two adjacent glyphs would be asserting a relationship
/// the font never claimed.
///
/// The mapping this produces is many-to-one and one-to-many at once, which is
/// what makes it worth writing at all. Several glyphs may name the same
/// character — an alternate, a positional form, a small capital — and one
/// glyph may name several, which is what a ligature is. Neither is expressible
/// through a simple font's `/Encoding`.
///
/// `None` when nothing has any text, because 9.10.3's grammar has no
/// zero-entry section and a CMap that maps nothing is worse than no CMap: a
/// reader that finds one stops looking for another answer.
fn to_unicode_cmap(mapping: &BTreeMap<u16, String>) -> Option<Vec<u8>> {
    let entries: Vec<(u16, Vec<u16>)> = mapping
        .iter()
        .filter(|(_, text)| !text.is_empty())
        .map(|(id, text)| (*id, text.encode_utf16().collect::<Vec<u16>>()))
        .filter(|(_, units)| !units.is_empty())
        .collect();
    if entries.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    out.extend_from_slice(
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n",
    );
    out.extend_from_slice(b"/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
    // The codespace is the whole of `/Identity-H`'s two-byte space, which is
    // what says a code is two bytes to a reader that only has this stream.
    out.extend_from_slice(b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for chunk in entries.chunks(BFCHAR_PER_SECTION) {
        out.extend_from_slice(format!("{} beginbfchar\n", chunk.len()).as_bytes());
        for (id, units) in chunk {
            out.extend_from_slice(format!("<{id:04X}> <").as_bytes());
            for unit in units {
                out.extend_from_slice(format!("{unit:04X}").as_bytes());
            }
            out.extend_from_slice(b">\n");
        }
        out.extend_from_slice(b"endbfchar\n");
    }
    out.extend_from_slice(b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    Some(out)
}

/// One number, as 7.3.3 spells it, rounded to a ten-thousandth.
///
/// A PDF real has no exponent form, which `f64`'s own `Display` already
/// respects; what it does not do is stop, so `0.1 + 0.2` arrives as
/// `0.30000000000000004` and the length of a content stream comes to depend on
/// a floating-point accident. One ten-thousandth of a text space unit is two
/// orders below what any rasteriser resolves.
fn number(value: f64) -> String {
    let rounded = (value * 1.0e4).round() / 1.0e4;
    // `-0` is a PDF number and a pointless one, and it is a second spelling of
    // a stream that is otherwise the same bytes.
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

/// Closes an open `TJ` array, if one is open.
fn close_array(out: &mut Vec<u8>, open: &mut bool) {
    if *open {
        out.extend_from_slice(b"] TJ\n");
        *open = false;
    }
}

/// A page being assembled.
pub struct PageBuilder {
    width: f64,
    height: f64,
    content: Vec<u8>,
    /// Everything this page may name, copied from the document when the page
    /// was added.
    resources: ResourceSet,
    /// Which font resources are composite, so [`PageBuilder::glyphs`] can tell
    /// a font whose codes are two bytes from one whose codes are one.
    composite: BTreeSet<Vec<u8>>,
    /// Characters drawn with each font resource, for subsetting.
    used: BTreeMap<Vec<u8>, BTreeSet<char>>,
    /// Glyphs drawn with each composite font resource, and the text each
    /// stands for. `/W` comes from the first and `/ToUnicode` from the second,
    /// and they are two independent statements about one glyph.
    drawn: BTreeMap<Vec<u8>, BTreeMap<u16, String>>,
    /// `/CropBox`, when the caller stated one. See [`PageBuilder::set_crop_box`].
    crop_box: Option<[f64; 4]>,
    /// `/BleedBox`, likewise.
    bleed_box: Option<[f64; 4]>,
    /// The page's link annotations, in the order they were added — which is
    /// the order they reach `/Annots`, and the order the reader reports them
    /// in. Written at `finish`, because a destination naming a page index
    /// cannot be resolved until every page exists.
    links: Vec<LinkAnnotation>,
}

impl PageBuilder {
    /// Sets `/CropBox`, as `[x0 y0 x1 y1]` in points from the bottom-left.
    ///
    /// 7.7.3.3: the region a viewer displays, defaulting to `/MediaBox` when
    /// absent — which is why this is an option rather than a value every page
    /// carries. A caller that has nothing to say about the visible region
    /// should say nothing, because a `/CropBox` equal to the media box is a
    /// statement where its absence is not.
    ///
    /// Nothing is normalised or clipped here. 14.11.2 makes a consumer reduce
    /// these boxes to their intersection with the media box, and this crate's
    /// own page reader already does it, so a writer that clipped as well would
    /// be the same rule in two places — and the caller's numbers would no
    /// longer be the numbers in the file, which is the property that makes a
    /// diff against the source document readable.
    pub fn set_crop_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.crop_box = Some([x0, y0, x1, y1]);
    }

    /// Sets `/BleedBox` (14.11.2), in the same coordinates as
    /// [`PageBuilder::set_crop_box`].
    pub fn set_bleed_box(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.bleed_box = Some([x0, y0, x1, y1]);
    }

    /// Writes text at a position, in points from the bottom-left.
    ///
    /// `font` names a font registered with [`DocumentBuilder::add_base_font`].
    pub fn text(&mut self, font: &[u8], size: f64, x: f64, y: f64, text: &str) {
        // Recorded so `finish` can subset an embedded font to what the
        // document actually draws. The builder is the only way content gets
        // written, so this sees everything.
        self.used
            .entry(font.to_vec())
            .or_default()
            .extend(text.chars());

        self.content.extend_from_slice(b"BT /");
        self.content.extend_from_slice(font);
        self.content
            .extend_from_slice(format!(" {size} Tf {x} {y} Td (").as_bytes());

        // 7.3.4.2: the three characters that must be escaped in a literal.
        for byte in text.bytes() {
            if matches!(byte, b'(' | b')' | b'\\') {
                self.content.push(b'\\');
            }
            self.content.push(byte);
        }
        self.content.extend_from_slice(b") Tj ET\n");
    }

    /// Fills a rectangle in device grey, from black (0) to white (1).
    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, grey: f64) {
        let grey = grey.clamp(0.0, 1.0);
        self.content
            .extend_from_slice(format!("{grey} g {x} {y} {w} {h} re f\n").as_bytes());
    }

    /// Draws a registered image into the given rectangle.
    ///
    /// 8.9.5.2: an image occupies the unit square, so placing it is entirely
    /// a matter of the transform — which is what this writes.
    pub fn image(&mut self, resource: &[u8], x: f64, y: f64, w: f64, h: f64) {
        self.content
            .extend_from_slice(format!("q {w} 0 0 {h} {x} {y} cm /").as_bytes());
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" Do Q\n");
    }

    /// Sets the non-stroking colour, as red, green and blue from zero to one.
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) {
        let c = |v: f64| v.clamp(0.0, 1.0);
        self.content
            .extend_from_slice(format!("{} {} {} rg\n", c(r), c(g), c(b)).as_bytes());
    }

    /// Sets the **stroking** colour, as red, green and blue from zero to one.
    ///
    /// `RG`, not `rg`. The two are different parameters of the graphics state
    /// and always have been; only this writer conflated them, by having one.
    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) {
        let c = |v: f64| v.clamp(0.0, 1.0);
        self.content
            .extend_from_slice(format!("{} {} {} RG\n", c(r), c(g), c(b)).as_bytes());
    }

    /// Applies a graphics state registered with
    /// [`DocumentBuilder::add_ext_gstate`] — the `gs` operator.
    ///
    /// The parameters stay in force until a `Q` restores an earlier state, so
    /// a caller setting an alpha for one shape brackets it with
    /// [`PageBuilder::raw`]`(b"q")` and `(b"Q")`. That is deliberate rather
    /// than an omission: `q`/`Q` nest around whole subtrees of drawing and an
    /// API that paired them with one `gs` could not express the ordinary case.
    ///
    /// Returns false when nothing is registered under `resource`, rather than
    /// writing a `gs` naming a resource the page does not carry.
    pub fn set_ext_gstate(&mut self, resource: &[u8]) -> bool {
        if !holds(&self.resources.ext_gstates, resource) {
            return false;
        }
        self.content.push(b'/');
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" gs\n");
        true
    }

    /// Draws a form XObject registered with [`DocumentBuilder::add_form`].
    ///
    /// The form's own `/Matrix` and `/BBox` place and clip it, so this writes
    /// the `Do` and nothing else; a caller wanting it somewhere else brackets
    /// the call with a `cm` through [`PageBuilder::raw`].
    pub fn form(&mut self, resource: &[u8]) -> bool {
        if !holds(&self.resources.forms, resource) {
            return false;
        }
        self.content.push(b'/');
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" Do\n");
        true
    }

    /// Paints a shading registered with [`DocumentBuilder::add_shading`] over
    /// the current clip — the `sh` operator (8.7.4.1).
    ///
    /// `sh` fills the clip, not a shape. Filling a *shape* with a gradient is
    /// [`PageBuilder::set_fill_pattern`] over a shading pattern, which this
    /// writer does not emit; the equivalent here is a clip set with `W n`
    /// through [`PageBuilder::raw`] before the call.
    pub fn shading(&mut self, resource: &[u8]) -> bool {
        if !holds(&self.resources.shadings, resource) {
            return false;
        }
        self.content.push(b'/');
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" sh\n");
        true
    }

    /// Sets the non-stroking colour to a tiling pattern registered with
    /// [`DocumentBuilder::add_tiling_pattern`].
    ///
    /// 8.7.3.2: the colour space has to become `/Pattern` before a pattern
    /// name can be an `scn` operand, so both operators are written together —
    /// a `scn` without its `cs` is a name in whatever space was in force, and
    /// the reader is entitled to read it as a number.
    pub fn set_fill_pattern(&mut self, resource: &[u8]) -> bool {
        if !holds(&self.resources.patterns, resource) {
            return false;
        }
        self.content.extend_from_slice(b"/Pattern cs /");
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" scn\n");
        true
    }

    /// Sets the **stroking** colour to a tiling pattern — `CS` and `SCN`.
    pub fn set_stroke_pattern(&mut self, resource: &[u8]) -> bool {
        if !holds(&self.resources.patterns, resource) {
            return false;
        }
        self.content.extend_from_slice(b"/Pattern CS /");
        self.content.extend_from_slice(resource);
        self.content.extend_from_slice(b" SCN\n");
        true
    }

    /// Draws glyphs **by index** with a composite font registered with
    /// [`DocumentBuilder::add_cid_font`].
    ///
    /// The string is two bytes a glyph, big-endian, because `/Identity-H`
    /// makes the code the CID and `/CIDToGIDMap /Identity` makes the CID the
    /// glyph index. Written as a hex string rather than a literal: a glyph
    /// index is arbitrary bytes and hex needs no escaping decisions at all.
    ///
    /// Returns false when `resource` is not a registered **composite** font,
    /// rather than writing two-byte codes into a font whose codes are one byte
    /// — which would draw the wrong glyphs at the wrong widths and look like a
    /// font problem rather than an encoding one.
    pub fn glyphs(&mut self, font: &[u8], size: f64, x: f64, y: f64, glyphs: &[Glyph<'_>]) -> bool {
        if !self.composite.contains(font) {
            return false;
        }

        // Recorded so `finish` can write `/W` from the font's own `hmtx` for
        // the glyphs the document drew, and a `/ToUnicode` from the text they
        // stood for. The first non-empty text a glyph is drawn with stands:
        // `/ToUnicode` is a function from code to text, so a glyph drawn twice
        // with two meanings has to resolve to one, and taking the first keeps
        // the answer the same however many times the document is written.
        let mapping = self.drawn.entry(font.to_vec()).or_default();
        for glyph in glyphs {
            let text = mapping.entry(glyph.id).or_default();
            if text.is_empty() && !glyph.text.is_empty() {
                *text = glyph.text.to_string();
            }
        }

        self.content.extend_from_slice(b"BT /");
        self.content.extend_from_slice(font);
        self.content
            .extend_from_slice(format!(" {size} Tf {x} {y} Td <").as_bytes());
        for glyph in glyphs {
            self.content
                .extend_from_slice(format!("{:04X}", glyph.id).as_bytes());
        }
        self.content.extend_from_slice(b"> Tj ET\n");
        true
    }

    /// Appends raw content-stream operators.
    ///
    /// An escape hatch for callers that know the operator set; nothing checks
    /// what goes in, so a malformed sequence produces a malformed page.
    pub fn raw(&mut self, operators: &[u8]) {
        self.content.extend_from_slice(operators);
        self.content.push(b'\n');
    }

    /// Adds a link annotation over a rectangle (12.5.6.5).
    ///
    /// The rectangle is in the page's own coordinates, points from the
    /// bottom-left, in the same order as [`PageBuilder::set_crop_box`]. It is
    /// **normalised** to 7.9.5's corner order, so a caller that measured a
    /// line of text downwards gets a rectangle a viewer can hit-test rather
    /// than an inverted one.
    ///
    /// An annotation is not a drawing operator and does not join the content
    /// stream: nothing here paints, and `/Border [0 0 0]` keeps a viewer from
    /// painting either. What it adds is a region of the page that goes
    /// somewhere.
    ///
    /// Returns false, adding nothing, for:
    ///
    /// - a rectangle with a `NaN` or an infinity in it, which is not a
    ///   rectangle a PDF number can spell;
    /// - a rectangle enclosing no area, which is a region no click can land
    ///   in — the same refusal [`DocumentBuilder::add_form`] makes of a
    ///   degenerate `/BBox`;
    /// - a target that cannot be written: see [`crate::dest::DestKind::is_writable`]
    ///   and [`crate::dest::is_writable_uri`];
    /// - a page already carrying [`crate::limits::MAX_ARRAY_LEN`] links, which
    ///   is where this repository's own reader stops walking an `/Annots`
    ///   array.
    pub fn link(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, target: &Target) -> bool {
        if !all_finite(&[x0, y0, x1, y1]) {
            return false;
        }
        let rect = normalised([x0, y0, x1, y1]);
        if !is_box(&rect) {
            return false;
        }
        if !target.is_writable() {
            return false;
        }
        if self.links.len() >= crate::limits::MAX_ARRAY_LEN {
            return false;
        }
        self.links.push(LinkAnnotation {
            rect,
            target: target.clone(),
        });
        true
    }
}

/// Reads a JPEG's dimensions and component count from its frame header.
///
/// Public because a caller sometimes has to know how big a JPEG is *before* it
/// hands the bytes over, and the number it uses must be **this** number. Gap 29
/// is the caller: a CBZ page's `/MediaBox` is the image's own pixel size, so a
/// second SOF reader in the facade would be two paths through one picture and a
/// stretched page on the day they disagreed. It reads a header and decodes
/// nothing, which is also why it is safe to call on every entry of an archive.
#[must_use]
pub fn jpeg_shape(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2usize;
    while at + 3 < data.len() {
        if data.get(at) != Some(&0xFF) {
            at += 1;
            continue;
        }
        let marker = *data.get(at + 1)?;
        at += 2;
        // Standalone markers carry no length.
        if matches!(marker, 0x01 | 0xD0..=0xD9 | 0xFF) {
            continue;
        }
        let length = data
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))?;

        // Any SOF marker; 0xC4, 0xC8 and 0xCC are tables, not frames.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = data.get(at + 3..at + 5)?;
            let w = data.get(at + 5..at + 7)?;
            let components = *data.get(at + 7)?;
            return Some((
                u32::from(u16::from_be_bytes([w[0], w[1]])),
                u32::from(u16::from_be_bytes([h[0], h[1]])),
                components,
            ));
        }
        at += length.max(2);
    }
    None
}

/// Where a link annotation or an outline entry goes.
///
/// **Two kinds and not three**, which is the writing side of the distinction
/// [`crate::dest::Destination`] draws on the reading side (ruling 6): a page in
/// this document, and a URI that leaves it altogether. A *named* destination —
/// the third thing a `/Dest` may be — is deliberately absent, because a name
/// is only a destination once the catalog carries a `/Names /Dests` tree to
/// look it up in, and this builder writes no name tree. Offering a name with
/// nowhere to resolve it would produce exactly the dead link the header of
/// `dest.rs` exists to describe.
///
/// A [`Target::Page`] naming an index past the end of the document is **not**
/// an error and is not repaired. Nothing here knows how many pages there will
/// be: a page's links are written while the page is drawn, and a chapter that
/// links forward to a chapter not yet added is the ordinary case rather than
/// the exotic one. What is written for an index that never arrives is *no
/// destination at all* — a link with a rectangle and nothing behind it, which
/// [`crate::dest::links`] already reports as `target: None`, and an outline
/// entry with no destination, which is the shape 12.3.3 gives a heading. The
/// alternative is a link to whichever page happened to be last, which is the
/// plausible-and-wrong answer this repository refuses everywhere else.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Target {
    /// A page in this document, positioned as `view` says (12.3.2.2).
    Page {
        /// Zero-based page index.
        index: u32,
        /// How the page is positioned when the link is followed (Table 151).
        view: DestKind,
    },
    /// A URI, written as the `/URI` action of 12.6.4.7.
    ///
    /// 7-bit ASCII, per that clause; see [`crate::dest::is_writable_uri`] for
    /// what is refused and why.
    Uri(String),
}

impl Target {
    /// Whether this target can be written.
    fn is_writable(&self) -> bool {
        match self {
            Target::Page { view, .. } => view.is_writable(),
            Target::Uri(uri) => crate::dest::is_writable_uri(uri),
        }
    }

    /// Writes the target into a dictionary that may carry either spelling.
    ///
    /// 12.5.6.5 makes `/Dest` and `/A` **alternatives**, and says a link
    /// carrying both is malformed; 12.3.3 says the same of an outline item. So
    /// exactly one is inserted, never both — and which one is decided by the
    /// kind of target rather than by a flag, so the malformed shape is not
    /// expressible.
    fn write(&self, names: &NameTable, pages: &[ObjRef], dict: &mut Dict) {
        match self {
            Target::Page { index, view } => {
                // An index past the end writes nothing; see the type's own
                // documentation for why that is the answer.
                if let Some(&page) = pages.get(*index as usize) {
                    dict.insert(
                        names.intern(b"Dest"),
                        crate::dest::destination_array(names, page, view),
                    );
                }
            }
            Target::Uri(uri) => {
                dict.insert(
                    names.intern(b"A"),
                    Object::Dict(crate::dest::uri_action(names, uri)),
                );
            }
        }
    }
}

/// One link annotation on a page, held until `finish` (12.5.6.5).
struct LinkAnnotation {
    /// `/Rect`, already normalised to 7.9.5's corner order.
    rect: [f64; 4],
    /// Where it goes.
    target: Target,
}

/// One outline entry to write (12.3.3).
pub struct OutlineEntry {
    /// The visible text.
    pub title: String,
    /// Where the entry goes, or `None` for a heading that only groups the
    /// entries beneath it.
    ///
    /// 12.3.3 makes `/Dest` optional, and an entry without one is a real
    /// shape rather than a degraded one: a part title above three chapters
    /// often points nowhere itself.
    pub target: Option<Target>,
    /// Whether the entry is shown expanded when the document is opened.
    ///
    /// 12.3.3 spells this as the **sign** of `/Count` rather than as a flag,
    /// and only for an entry that has descendants: the entry is required to
    /// carry a count at all only when there is something under it. So this
    /// field is ignored for an entry with no children — an entry with nothing
    /// beneath it is neither open nor closed — and an entry that has some
    /// round-trips through [`crate::outline::outline`]'s own `open` flag.
    pub open: bool,
    /// Nested entries.
    pub children: Vec<OutlineEntry>,
}

/// Whether an outline can be written *and read back by this repository*.
///
/// Two independent limits, and both are the **reader's** rather than
/// invented here — the same argument `MAX_FUNCTION_DEPTH` above is written
/// under, and the reason neither joins `bounds_ledger.rs`. Nothing in an
/// [`OutlineEntry`] comes from a file, so a constant over one could never be
/// the thing that stopped an attacker; what these are instead is the shape
/// [`crate::outline::outline`] can walk:
///
/// - it stops recursing past [`crate::limits::MAX_NEST_DEPTH`], counting the
///   top level as depth zero, and warns `OutlineTruncated`;
/// - it stops walking one `/Next` chain after
///   [`crate::limits::MAX_TREE_ENTRIES`] siblings, and warns the same.
///
/// A writer whose output its own reader silently truncates is not a writer.
/// The walk is **iterative**, so a caller handing over a tree a thousand deep
/// is refused rather than overflowing a stack finding out.
fn outline_is_writable(entries: &[OutlineEntry]) -> bool {
    let mut stack: Vec<(&[OutlineEntry], u32)> = vec![(entries, 0)];
    while let Some((level, depth)) = stack.pop() {
        if depth > crate::limits::MAX_NEST_DEPTH {
            return false;
        }
        if level.len() > crate::limits::MAX_TREE_ENTRIES {
            return false;
        }
        for entry in level {
            if let Some(target) = &entry.target {
                if !target.is_writable() {
                    return false;
                }
            }
            if !entry.children.is_empty() {
                stack.push((&entry.children, depth + 1));
            }
        }
    }
    true
}

/// Assembles a document.
pub struct DocumentBuilder {
    names: NameTable,
    objects: ObjectSet,
    next: u32,
    pages: Vec<PageBuilder>,
    /// Everything a page added from here on may name.
    resources: ResourceSet,
    /// Which form resources carry a `/Group`, so an `/SMask` naming one that
    /// does not can be refused (11.6.5.2).
    groups: BTreeSet<Vec<u8>>,
    /// Which font resources are composite.
    composite: BTreeSet<Vec<u8>>,
    /// Fonts whose programs are embedded, written at `finish` once the
    /// characters they are asked to draw are known.
    embedded: Vec<Embedded>,
    /// Composite fonts, written at `finish` for the same reason — with the
    /// glyphs rather than the characters deciding the subset.
    cid_fonts: Vec<CidFont>,
    /// Glyphs drawn by [`DocumentBuilder::glyph_run`], into a content stream
    /// this builder does not own. Merged with every page's own record at
    /// `finish`; see that method for why the record is the document's.
    drawn: BTreeMap<Vec<u8>, BTreeMap<u16, String>>,
    subset_fonts: bool,
    info: Dict,
    outline: Vec<OutlineEntry>,
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBuilder {
    /// An empty document.
    #[must_use]
    pub fn new() -> DocumentBuilder {
        DocumentBuilder {
            names: NameTable::new(),
            objects: ObjectSet::new(),
            // 1 and 2 are reserved for the catalog and the page tree.
            next: 3,
            pages: Vec::new(),
            resources: ResourceSet::default(),
            groups: BTreeSet::new(),
            composite: BTreeSet::new(),
            embedded: Vec::new(),
            cid_fonts: Vec::new(),
            drawn: BTreeMap::new(),
            subset_fonts: true,
            info: Dict::new(),
            outline: Vec::new(),
        }
    }

    fn allocate(&mut self) -> ObjRef {
        let r = ObjRef::new(self.next, 0);
        self.next = self.next.saturating_add(1);
        r
    }

    /// Registers one of the standard 14 fonts under a resource name.
    ///
    /// A standard font needs no `/Widths` and no embedded program, which is
    /// what makes it the right choice for a fixture: the reader supplies the
    /// metrics.
    pub fn add_base_font(&mut self, resource: &[u8], base_font: &[u8]) {
        let r = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Type1")),
        );
        dict.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        dict.insert(
            self.names.intern(b"Encoding"),
            Object::Name(self.names.intern(b"WinAnsiEncoding")),
        );
        self.objects.insert(r.num, Object::Dict(dict));
        self.resources.fonts.push((resource.to_vec(), r));
    }

    /// Registers a TrueType font, embedding its program (9.6.6, 9.9).
    ///
    /// A document using only the standard 14 relies on the reader having them;
    /// one that embeds its font carries everything it needs, which is the
    /// difference between a file that renders the same everywhere and one that
    /// renders the same where the fonts happen to match.
    ///
    /// Widths come from the font program's own `hmtx`, scaled into the
    /// thousandths PDF measures in, so they agree with the outlines a renderer
    /// will draw — a `/Widths` array that disagrees with the glyphs is how
    /// text ends up overlapping itself.
    ///
    /// Returns false when the bytes are not a TrueType program this can read,
    /// rather than writing a font dictionary pointing at nothing.
    ///
    /// The whole program is embedded. Subsetting needs a glyph-set analysis
    /// and a table rewriter, and shipping the entire face is correct — merely
    /// larger — where a broken subset is neither.
    pub fn add_embedded_font(&mut self, resource: &[u8], base_font: &[u8], program: &[u8]) -> bool {
        if tinker_pdf_font::Sfnt::parse(program).is_none() {
            return false;
        }

        // Object numbers are reserved now, so page resources can name the
        // font; nothing is written until `finish`, because the subset depends
        // on what the document ends up drawing and that is not known yet.
        let file_ref = self.allocate();
        let descriptor_ref = self.allocate();
        let font_ref = self.allocate();

        self.embedded.push(Embedded {
            resource: resource.to_vec(),
            base_font: base_font.to_vec(),
            program: program.to_vec(),
            file_ref,
            descriptor_ref,
            font_ref,
        });
        self.resources.fonts.push((resource.to_vec(), font_ref));
        true
    }

    /// Registers a **composite** font, embedding its program (9.7).
    ///
    /// A Type0 font over a CIDFontType2 descendant, with `/Encoding
    /// /Identity-H` and `/CIDToGIDMap /Identity` — which together make the
    /// two-byte code in a string the CID and the CID the glyph index, so
    /// [`PageBuilder::glyphs`] can address a glyph directly. That is the whole
    /// difference from [`DocumentBuilder::add_embedded_font`], and it is not a
    /// convenience: a simple font with `/WinAnsiEncoding` has no way to name a
    /// glyph the font's `cmap` does not reach from a character, and no way at
    /// all to distinguish two glyphs that stand for one character.
    ///
    /// `/W` comes from the font program's own `hmtx` and `/ToUnicode` from the
    /// text the caller says each glyph stands for. They are written at
    /// `finish`, because both depend on which glyphs the document ends up
    /// drawing.
    ///
    /// Returns false when the bytes are not a font this can read, for
    /// [`DocumentBuilder::add_embedded_font`]'s reason.
    pub fn add_cid_font(&mut self, resource: &[u8], base_font: &[u8], program: &[u8]) -> bool {
        if tinker_pdf_font::Sfnt::parse(program).is_none() {
            return false;
        }

        let file_ref = self.allocate();
        let descriptor_ref = self.allocate();
        let descendant_ref = self.allocate();
        let font_ref = self.allocate();
        let to_unicode_ref = self.allocate();

        self.cid_fonts.push(CidFont {
            resource: resource.to_vec(),
            base_font: base_font.to_vec(),
            program: program.to_vec(),
            file_ref,
            descriptor_ref,
            descendant_ref,
            font_ref,
            to_unicode_ref,
        });
        self.resources.fonts.push((resource.to_vec(), font_ref));
        self.composite.insert(resource.to_vec());
        true
    }

    /// Writes one composite-font text run into a content stream the caller is
    /// assembling itself, and records its glyphs (gap 30, milestone 7).
    ///
    /// # Why this is not [`PageBuilder::glyphs`]
    ///
    /// Two differences, and each of them is the reason the other is not
    /// enough.
    ///
    /// **The caller has the positions.** `glyphs` shows a string and lets the
    /// font's advances place everything after the first glyph, which is what a
    /// caller *writing* text wants. A caller reading a document that states
    /// each glyph's own position has the numbers already, and letting the
    /// advances decide would move them. So the placement is the caller's and
    /// the **adjustments are worked out here**, from the same `hmtx` `/W` is
    /// written from — which is the property worth having: the numbers in the
    /// `TJ` array and the numbers in `/W` cannot disagree, because one is
    /// computed from the other.
    ///
    /// **The stream is not a page's.** A caller that assembles a content
    /// stream before the page exists — one part's markup shown on four
    /// thousand pages is painted once — cannot reach a [`PageBuilder`] at all,
    /// and the `/W` and `/ToUnicode` a run owes are the **document's** rather
    /// than any one page's. So the record goes here, and merges with every
    /// page's under the same first-non-empty-text-wins rule.
    ///
    /// One call rather than a write and a note, deliberately: a caller that
    /// could write the operators without recording the glyphs would produce a
    /// run that draws and has no `/ToUnicode`, and a run that draws and cannot
    /// be extracted is exactly the half-answer this builder exists to make
    /// impossible.
    ///
    /// Returns false, having written nothing, when `font` is not a registered
    /// **composite** font, when there are no glyphs, or when the size, the
    /// matrix or a placement is not a number a content stream can carry.
    pub fn glyph_run(
        &mut self,
        out: &mut Vec<u8>,
        font: &[u8],
        size: f64,
        matrix: [f64; 6],
        glyphs: &[PlacedGlyph<'_>],
    ) -> bool {
        if !self.composite.contains(font) || glyphs.is_empty() {
            return false;
        }
        if !size.is_finite() || size <= 0.0 || !matrix.iter().all(|v| v.is_finite()) {
            return false;
        }
        if !glyphs.iter().all(|g| g.x.is_finite() && g.rise.is_finite()) {
            return false;
        }

        // The advances the reader will use, which are `/W`'s and not `hmtx`'s
        // raw numbers: `width_array` rounds to a whole thousandth, so a pen
        // model built on the unrounded figure would drift a glyph at a time
        // away from where the reader puts it.
        let program = self
            .cid_fonts
            .iter()
            .find(|f| f.resource == font)
            .map(|f| f.program.clone());
        let sfnt = program.as_deref().and_then(tinker_pdf_font::Sfnt::parse);
        let units = sfnt
            .as_ref()
            .map_or(1000.0, |s| f64::from(s.units_per_em.max(1)));
        let advance = |id: u16| -> f64 {
            // 9.7.4.3: a CID with no `/W` entry takes `/DW`, which this writer
            // states as 1000 — one em.
            sfnt.as_ref()
                .and_then(|s| s.advance(id))
                .map_or(1.0, |a| (f64::from(a) * 1000.0 / units).round() / 1000.0)
        };

        let mapping = self.drawn.entry(font.to_vec()).or_default();
        for placed in glyphs {
            let text = mapping.entry(placed.glyph.id).or_default();
            if text.is_empty() && !placed.glyph.text.is_empty() {
                *text = placed.glyph.text.to_string();
            }
        }

        out.extend_from_slice(b"BT /");
        out.extend_from_slice(font);
        out.extend_from_slice(format!(" {} Tf ", number(size)).as_bytes());
        for value in matrix {
            out.extend_from_slice(number(value).as_bytes());
            out.push(b' ');
        }
        out.extend_from_slice(b"Tm\n");

        // The pen, in unscaled text space, and the rise the text state is
        // currently in. A run is one `TJ` array until a glyph asks to sit off
        // the baseline: `Ts` is a text-state parameter and cannot go inside
        // one, and it is the only spelling of a vertical offset that leaves
        // the pen where it was.
        let mut pen = 0.0f64;
        let mut rise = 0.0f64;
        let mut open = false;
        for placed in glyphs {
            if placed.rise != rise {
                close_array(out, &mut open);
                rise = placed.rise;
                out.extend_from_slice(format!("{} Ts\n", number(rise)).as_bytes());
            }
            if !open {
                out.push(b'[');
                open = true;
            }
            // 9.4.3: a number in a `TJ` array moves the pen **backwards** by
            // that many thousandths of a text space unit, scaled by the font
            // size. So the number that puts the next glyph at `x` is the gap
            // between where the pen is and where the caller put it, negated.
            let adjust = (pen - placed.x) * 1000.0 / size;
            if number(adjust) != "0" {
                // A `>` already ends the hex string before it, so the space is
                // not needed to parse — it is there so a content stream reads
                // as a sequence and a diff against the source document can be
                // followed, which is the property gap 30's geometry section
                // asks not to be thrown away.
                if out.last() == Some(&b'>') {
                    out.push(b' ');
                }
                out.extend_from_slice(number(adjust).as_bytes());
                out.push(b' ');
            }
            out.extend_from_slice(format!("<{:04X}>", placed.glyph.id).as_bytes());
            pen = placed.x + advance(placed.glyph.id) * size;
        }
        close_array(out, &mut open);
        if rise != 0.0 {
            // `Ts` outlives `ET` — it is graphics state, not text-object state
            // — so a run that left one set would tilt whatever a caller drew
            // next off the baseline.
            out.extend_from_slice(b"0 Ts\n");
        }
        out.extend_from_slice(b"ET\n");
        true
    }

    /// Registers a graphics state under a resource name (Table 58).
    ///
    /// Everything is checked before anything is written, which is
    /// [`DocumentBuilder::add_image`]'s posture and is worth restating for the
    /// one entry that can fail late: an `/SMask` names a form, and a mask over
    /// a form that is not a transparency group is one 11.6.5.2 forbids.
    ///
    /// Returns false for an alpha outside 11.6.4.4's range, a non-finite
    /// backdrop component, or an `/SMask` naming a form that is not
    /// registered or carries no `/Group`.
    pub fn add_ext_gstate(&mut self, resource: &[u8], state: &ExtGState<'_>) -> bool {
        if state.fill_alpha.is_some_and(|v| !is_alpha(v))
            || state.stroke_alpha.is_some_and(|v| !is_alpha(v))
        {
            return false;
        }

        let mut mask_ref = None;
        if let Some(StateMask::Group { form, backdrop, .. }) = &state.soft_mask {
            if !self.groups.contains(*form) {
                return false;
            }
            let Some((_, reference)) = self
                .resources
                .forms
                .iter()
                .find(|(name, _)| name.as_slice() == *form)
            else {
                return false;
            };
            if let Some(values) = backdrop {
                if values.is_empty() || !all_finite(values) {
                    return false;
                }
            }
            mask_ref = Some(*reference);
        }

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"ExtGState")));
        if let Some(alpha) = state.fill_alpha {
            dict.insert(self.names.intern(b"ca"), Object::Real(alpha));
        }
        if let Some(alpha) = state.stroke_alpha {
            dict.insert(self.names.intern(b"CA"), Object::Real(alpha));
        }
        if let Some(mode) = state.blend_mode {
            dict.insert(
                self.names.intern(b"BM"),
                Object::Name(self.names.intern(mode.pdf_name())),
            );
        }
        match (&state.soft_mask, mask_ref) {
            (Some(StateMask::None), _) => {
                dict.insert(
                    self.names.intern(b"SMask"),
                    Object::Name(self.names.intern(b"None")),
                );
            }
            (Some(StateMask::Group { kind, backdrop, .. }), Some(reference)) => {
                let mut mask = Dict::new();
                mask.insert(Name::TYPE, Object::Name(self.names.intern(b"Mask")));
                mask.insert(
                    self.names.intern(b"S"),
                    Object::Name(self.names.intern(kind.pdf_name())),
                );
                mask.insert(self.names.intern(b"G"), Object::Ref(reference));
                if let Some(values) = backdrop {
                    mask.insert(
                        self.names.intern(b"BC"),
                        Object::Array(values.iter().map(|v| Object::Real(*v)).collect()),
                    );
                }
                dict.insert(self.names.intern(b"SMask"), Object::Dict(mask));
            }
            _ => {}
        }

        let reference = self.allocate();
        self.objects.insert(reference.num, Object::Dict(dict));
        self.resources
            .ext_gstates
            .push((resource.to_vec(), reference));
        true
    }

    /// Registers a form XObject under a resource name (8.10).
    ///
    /// The form's `/Resources` is the document's registered resources **at the
    /// moment this is called**, which is the rule
    /// [`DocumentBuilder::add_page`] already follows. One consequence is worth
    /// having in writing: a form cannot name itself, because it is not
    /// registered until this returns — so a form referring to a form is a
    /// chain that can only point backwards, and a cycle is unreachable by
    /// construction rather than caught by a depth cap.
    ///
    /// Returns false for a degenerate `/BBox` or a non-finite `/Matrix`.
    pub fn add_form(&mut self, resource: &[u8], form: &FormXObject<'_>) -> bool {
        if !is_box(&form.bbox) {
            return false;
        }
        if let Some(matrix) = form.matrix {
            if !all_finite(&matrix) {
                return false;
            }
        }

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"XObject")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Form")),
        );
        dict.insert(
            self.names.intern(b"BBox"),
            Object::Array(form.bbox.iter().map(|v| Object::Real(*v)).collect()),
        );
        if let Some(matrix) = form.matrix {
            dict.insert(
                self.names.intern(b"Matrix"),
                Object::Array(matrix.iter().map(|v| Object::Real(*v)).collect()),
            );
        }
        if let Some(group) = form.group {
            let mut entry = Dict::new();
            entry.insert(self.names.intern(b"S"), {
                let name = self.names.intern(b"Transparency");
                Object::Name(name)
            });
            entry.insert(
                self.names.intern(b"CS"),
                Object::Name(self.names.intern(group.color_space.pdf_name())),
            );
            // 11.6.6 defaults both to false, so each is written only when it
            // is true — an explicit `/I false` says the same thing as the
            // absence and makes two spellings of one group.
            if group.isolated {
                entry.insert(self.names.intern(b"I"), Object::Bool(true));
            }
            if group.knockout {
                entry.insert(self.names.intern(b"K"), Object::Bool(true));
            }
            dict.insert(self.names.intern(b"Group"), Object::Dict(entry));
        }
        let resources = self.resources.dict(&self.names);
        dict.insert(Name::RESOURCES, Object::Dict(resources));

        let reference = self.allocate();
        self.objects.insert_stream(
            reference.num,
            StreamData {
                dict,
                data: form.content.to_vec(),
            },
        );
        self.resources.forms.push((resource.to_vec(), reference));
        if form.group.is_some() {
            self.groups.insert(resource.to_vec());
        }
        true
    }

    /// Registers a shading under a resource name (8.7.4.5).
    ///
    /// Returns false for a degenerate geometry, a negative radius, or a
    /// `/Function` that does not produce one number per colour component.
    pub fn add_shading(&mut self, resource: &[u8], shading: &Shading) -> bool {
        let (kind, space, coords, function, extend) = match shading {
            Shading::Axial {
                color_space,
                coords,
                function,
                extend,
            } => {
                // A zero-length axis has no direction to blend along, so
                // 8.7.4.5.3's parameter is undefined everywhere and the area
                // paints in whatever the reader falls back to.
                if !all_finite(coords) || (coords[0] == coords[2] && coords[1] == coords[3]) {
                    return false;
                }
                (2i64, *color_space, coords.to_vec(), function, *extend)
            }
            Shading::Radial {
                color_space,
                coords,
                function,
                extend,
            } => {
                if !all_finite(coords) || coords[2] < 0.0 || coords[5] < 0.0 {
                    return false;
                }
                // Two circles of zero radius in the same place are a point,
                // and 8.7.4.5.4 has nothing to blend between.
                if coords[2] == 0.0 && coords[5] == 0.0 {
                    return false;
                }
                (3i64, *color_space, coords.to_vec(), function, *extend)
            }
        };

        if !function.is_valid(space.components() as usize) {
            return false;
        }

        let function_ref = self.write_function(function);
        let mut dict = Dict::new();
        dict.insert(self.names.intern(b"ShadingType"), Object::Int(kind));
        dict.insert(
            self.names.intern(b"ColorSpace"),
            Object::Name(self.names.intern(space.pdf_name())),
        );
        dict.insert(
            self.names.intern(b"Coords"),
            Object::Array(coords.iter().map(|v| Object::Real(*v)).collect()),
        );
        dict.insert(
            self.names.intern(b"Extend"),
            Object::Array(vec![Object::Bool(extend.0), Object::Bool(extend.1)]),
        );
        dict.insert(self.names.intern(b"Function"), Object::Ref(function_ref));

        let reference = self.allocate();
        self.objects.insert(reference.num, Object::Dict(dict));
        self.resources.shadings.push((resource.to_vec(), reference));
        true
    }

    /// Registers a tiling pattern under a resource name (8.7.3).
    ///
    /// The cell's `/Resources` follow [`DocumentBuilder::add_form`]'s rule and
    /// for the same reasons.
    ///
    /// Returns false for a degenerate `/BBox`, a zero or non-finite step, or a
    /// non-finite `/Matrix`.
    pub fn add_tiling_pattern(&mut self, resource: &[u8], pattern: &TilingPattern<'_>) -> bool {
        if !is_box(&pattern.bbox) {
            return false;
        }
        // 8.7.3.1: the steps are the spacing between cells. Zero puts every
        // cell in one place, which tiles nothing and takes as long as the clip
        // is large to decide it.
        if !pattern.x_step.is_finite()
            || !pattern.y_step.is_finite()
            || pattern.x_step == 0.0
            || pattern.y_step == 0.0
        {
            return false;
        }
        if let Some(matrix) = pattern.matrix {
            if !all_finite(&matrix) {
                return false;
            }
        }

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Pattern")));
        dict.insert(self.names.intern(b"PatternType"), Object::Int(1));
        dict.insert(self.names.intern(b"PaintType"), Object::Int(1));
        dict.insert(
            self.names.intern(b"TilingType"),
            Object::Int(match pattern.tiling_type {
                TilingType::ConstantSpacing => 1,
                TilingType::NoDistortion => 2,
                TilingType::FasterTiling => 3,
            }),
        );
        dict.insert(
            self.names.intern(b"BBox"),
            Object::Array(pattern.bbox.iter().map(|v| Object::Real(*v)).collect()),
        );
        dict.insert(self.names.intern(b"XStep"), Object::Real(pattern.x_step));
        dict.insert(self.names.intern(b"YStep"), Object::Real(pattern.y_step));
        if let Some(matrix) = pattern.matrix {
            dict.insert(
                self.names.intern(b"Matrix"),
                Object::Array(matrix.iter().map(|v| Object::Real(*v)).collect()),
            );
        }
        let resources = self.resources.dict(&self.names);
        dict.insert(Name::RESOURCES, Object::Dict(resources));

        let reference = self.allocate();
        self.objects.insert_stream(
            reference.num,
            StreamData {
                dict,
                data: pattern.content.to_vec(),
            },
        );
        self.resources.patterns.push((resource.to_vec(), reference));
        true
    }

    /// Writes a function as an indirect object, returning its reference.
    ///
    /// Indirect rather than inline so a sub-function is one object rather than
    /// a dictionary nested inside a dictionary, which is what makes the
    /// stitching case readable in a dump. Only called once the function has
    /// been validated, so the recursion here is bounded by
    /// [`MAX_FUNCTION_DEPTH`] rather than by the caller.
    fn write_function(&mut self, function: &Function) -> ObjRef {
        let mut dict = Dict::new();
        match function {
            Function::Exponential { domain, c0, c1, n } => {
                dict.insert(self.names.intern(b"FunctionType"), Object::Int(2));
                dict.insert(
                    self.names.intern(b"Domain"),
                    Object::Array(domain.iter().map(|v| Object::Real(*v)).collect()),
                );
                dict.insert(
                    self.names.intern(b"C0"),
                    Object::Array(c0.iter().map(|v| Object::Real(*v)).collect()),
                );
                dict.insert(
                    self.names.intern(b"C1"),
                    Object::Array(c1.iter().map(|v| Object::Real(*v)).collect()),
                );
                dict.insert(self.names.intern(b"N"), Object::Real(*n));
            }
            Function::Stitching {
                domain,
                functions,
                bounds,
                encode,
            } => {
                let refs: Vec<ObjRef> = functions.iter().map(|f| self.write_function(f)).collect();
                dict.insert(self.names.intern(b"FunctionType"), Object::Int(3));
                dict.insert(
                    self.names.intern(b"Domain"),
                    Object::Array(domain.iter().map(|v| Object::Real(*v)).collect()),
                );
                dict.insert(
                    self.names.intern(b"Functions"),
                    Object::Array(refs.iter().map(|r| Object::Ref(*r)).collect()),
                );
                dict.insert(
                    self.names.intern(b"Bounds"),
                    Object::Array(bounds.iter().map(|v| Object::Real(*v)).collect()),
                );
                dict.insert(
                    self.names.intern(b"Encode"),
                    Object::Array(
                        encode
                            .iter()
                            .flat_map(|pair| [Object::Real(pair[0]), Object::Real(pair[1])])
                            .collect(),
                    ),
                );
            }
        }

        let reference = self.allocate();
        self.objects.insert(reference.num, Object::Dict(dict));
        reference
    }

    /// Whether embedded fonts are cut down to the glyphs the document draws.
    ///
    /// On by default: a whole CJK face to set a line of Latin text is tens of
    /// megabytes, and subsetting is what makes embedding practical at all.
    /// Turn it off for a file that will be edited afterwards by something
    /// needing the other glyphs, which is the one case where the full face
    /// earns its size.
    pub fn set_subset_fonts(&mut self, subset: bool) {
        self.subset_fonts = subset;
    }

    /// Writes the font dictionaries, cutting each program down to the
    /// characters the pages drew with it.
    fn write_embedded_fonts(&mut self, used: &BTreeMap<Vec<u8>, BTreeSet<char>>) {
        let embedded = std::mem::take(&mut self.embedded);
        let empty = BTreeSet::new();

        for font in embedded {
            let characters = used.get(&font.resource).unwrap_or(&empty);
            let text: String = characters.iter().copied().collect();

            // A subset that cannot be built is not a reason to fail the
            // document: the whole face is larger and correct, which is the
            // right way round (ruling 2).
            let (program, subsetted) = if self.subset_fonts {
                let glyphs = tinker_pdf_font::glyphs_for(&font.program, &text);
                if !text.is_empty() && glyphs.is_empty() {
                    // The document drew text with this font and the font
                    // claims none of it: a missing or unreadable `cmap`, most
                    // likely, or a symbolic font addressed some other way.
                    // Subsetting on that evidence would keep `.notdef` alone
                    // and every letter would come out blank — which reads as
                    // a rendering bug rather than a subsetting one, and so
                    // gets found far too late.
                    (font.program.clone(), false)
                } else {
                    match tinker_pdf_font::subset(&font.program, &glyphs) {
                        Some(reduced) => (reduced, true),
                        None => (font.program.clone(), false),
                    }
                }
            } else {
                (font.program.clone(), false)
            };

            // 9.6.4: a subset font name carries a six-letter tag and a plus
            // sign, which is how a reader knows the embedded program is not
            // the whole face.
            let base_font = if subsetted {
                let mut tagged = subset_tag(&program);
                tagged.extend_from_slice(&font.base_font);
                tagged
            } else {
                font.base_font.clone()
            };

            self.write_font_objects(&font, &program, &base_font);
        }
    }

    fn write_font_objects(&mut self, font: &Embedded, program: &[u8], base_font: &[u8]) {
        // Widths come from the *original* program. Subsetting never moves a
        // glyph identifier, so the `hmtx` entry is still the right one, and
        // reading them from the subset would be no different — but reading
        // them from the original says plainly that it does not depend on
        // which glyphs survived.
        let Some(sfnt) = tinker_pdf_font::Sfnt::parse(&font.program) else {
            return;
        };
        let units = f64::from(sfnt.units_per_em.max(1));

        // 9.6.6.4: /FirstChar../LastChar with one width each, in glyph space
        // thousandths. WinAnsi is assumed because that is what the encoding
        // below declares; a font used with another encoding needs the widths
        // that encoding implies.
        const FIRST: u8 = 32;
        const LAST: u8 = 255;
        let mut widths = Vec::with_capacity(usize::from(LAST - FIRST) + 1);
        for code in FIRST..=LAST {
            let width = sfnt
                .glyph_for_char(char::from(code))
                .and_then(|glyph| sfnt.advance(glyph))
                .map_or(500.0, |advance| f64::from(advance) * 1000.0 / units);
            widths.push(Object::Real(width.round()));
        }

        let mut file_dict = Dict::new();
        // 9.9: /Length1 is the embedded program's length — the subset's, not
        // the original's, since the subset is what the stream contains.
        file_dict.insert(
            self.names.intern(b"Length1"),
            Object::Int(program.len() as i64),
        );
        self.objects.insert_stream(
            font.file_ref.num,
            StreamData {
                dict: file_dict,
                data: program.to_vec(),
            },
        );

        let mut descriptor = Dict::new();
        descriptor.insert(
            Name::TYPE,
            Object::Name(self.names.intern(b"FontDescriptor")),
        );
        descriptor.insert(
            self.names.intern(b"FontName"),
            Object::Name(self.names.intern(base_font)),
        );
        // 9.8.2 Table 123: bit 6 marks a non-symbolic font, which is what an
        // explicit /Encoding requires; a symbolic one is read through its own
        // cmap and must not carry one.
        descriptor.insert(self.names.intern(b"Flags"), Object::Int(32));
        descriptor.insert(
            self.names.intern(b"FontBBox"),
            Object::Array(vec![
                Object::Int(-500),
                Object::Int(-300),
                Object::Int(1500),
                Object::Int(1000),
            ]),
        );
        descriptor.insert(self.names.intern(b"ItalicAngle"), Object::Int(0));
        descriptor.insert(self.names.intern(b"Ascent"), Object::Int(750));
        descriptor.insert(self.names.intern(b"Descent"), Object::Int(-250));
        descriptor.insert(self.names.intern(b"CapHeight"), Object::Int(700));
        descriptor.insert(self.names.intern(b"StemV"), Object::Int(80));
        descriptor.insert(self.names.intern(b"FontFile2"), Object::Ref(font.file_ref));
        self.objects
            .insert(font.descriptor_ref.num, Object::Dict(descriptor));

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"TrueType")),
        );
        dict.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        dict.insert(
            self.names.intern(b"Encoding"),
            Object::Name(self.names.intern(b"WinAnsiEncoding")),
        );
        dict.insert(
            self.names.intern(b"FirstChar"),
            Object::Int(i64::from(FIRST)),
        );
        dict.insert(self.names.intern(b"LastChar"), Object::Int(i64::from(LAST)));
        dict.insert(self.names.intern(b"Widths"), Object::Array(widths));
        dict.insert(
            self.names.intern(b"FontDescriptor"),
            Object::Ref(font.descriptor_ref),
        );

        self.objects.insert(font.font_ref.num, Object::Dict(dict));
    }

    /// Writes the composite font dictionaries (9.7.6, 9.7.4).
    ///
    /// `drawn` is every glyph every page drew with each composite resource,
    /// and the text it stood for. Three independent things come out of it and
    /// they are written from three different places: the **subset** from the
    /// glyph ids, `/W` from the *original* program's `hmtx` for those ids, and
    /// `/ToUnicode` from the text. None of the three can stand in for another.
    fn write_cid_fonts(&mut self, drawn: &BTreeMap<Vec<u8>, BTreeMap<u16, String>>) {
        let fonts = std::mem::take(&mut self.cid_fonts);
        let empty = BTreeMap::new();

        for font in fonts {
            let mapping = drawn.get(&font.resource).unwrap_or(&empty);

            // 9.7.4.2: glyph 0 is `.notdef` and a font without it is a font a
            // reader cannot fall back through.
            let mut ids: BTreeSet<u16> = mapping.keys().copied().collect();
            ids.insert(0);

            let (program, subsetted) = if self.subset_fonts {
                match tinker_pdf_font::subset(&font.program, &ids) {
                    Some(reduced) => (reduced, true),
                    None => (font.program.clone(), false),
                }
            } else {
                (font.program.clone(), false)
            };

            let base_font = if subsetted {
                let mut tagged = subset_tag(&program);
                tagged.extend_from_slice(&font.base_font);
                tagged
            } else {
                font.base_font.clone()
            };

            self.write_cid_font_objects(&font, &program, &base_font, mapping);
        }
    }

    fn write_cid_font_objects(
        &mut self,
        font: &CidFont,
        program: &[u8],
        base_font: &[u8],
        mapping: &BTreeMap<u16, String>,
    ) {
        // From the **original** program, for `write_font_objects`'s reason:
        // subsetting never moves a glyph identifier, and reading the metrics
        // from the face rather than from the cut-down copy says plainly that
        // the widths do not depend on which glyphs survived.
        let Some(sfnt) = tinker_pdf_font::Sfnt::parse(&font.program) else {
            return;
        };
        let units = f64::from(sfnt.units_per_em.max(1));

        let mut file_dict = Dict::new();
        file_dict.insert(
            self.names.intern(b"Length1"),
            Object::Int(program.len() as i64),
        );
        self.objects.insert_stream(
            font.file_ref.num,
            StreamData {
                dict: file_dict,
                data: program.to_vec(),
            },
        );

        let mut descriptor = Dict::new();
        descriptor.insert(
            Name::TYPE,
            Object::Name(self.names.intern(b"FontDescriptor")),
        );
        descriptor.insert(
            self.names.intern(b"FontName"),
            Object::Name(self.names.intern(base_font)),
        );
        // 9.8.2 Table 123 bit 3: symbolic. A composite font under
        // `/Identity-H` has no `/Encoding` a code could be looked up in, so it
        // is symbolic by construction — and bit 6, which the simple-font path
        // sets, is the flag that says the opposite and requires an encoding.
        descriptor.insert(self.names.intern(b"Flags"), Object::Int(4));
        descriptor.insert(
            self.names.intern(b"FontBBox"),
            Object::Array(vec![
                Object::Int(-500),
                Object::Int(-300),
                Object::Int(1500),
                Object::Int(1000),
            ]),
        );
        descriptor.insert(self.names.intern(b"ItalicAngle"), Object::Int(0));
        descriptor.insert(self.names.intern(b"Ascent"), Object::Int(750));
        descriptor.insert(self.names.intern(b"Descent"), Object::Int(-250));
        descriptor.insert(self.names.intern(b"CapHeight"), Object::Int(700));
        descriptor.insert(self.names.intern(b"StemV"), Object::Int(80));
        descriptor.insert(self.names.intern(b"FontFile2"), Object::Ref(font.file_ref));
        self.objects
            .insert(font.descriptor_ref.num, Object::Dict(descriptor));

        // 9.7.3: the descendant's registry, ordering and supplement. Identity
        // ordering is what `/Identity-H` means on the other side of the pair,
        // and a descendant claiming some other ordering while the encoding
        // says Identity is a font whose two halves disagree.
        let mut system = Dict::new();
        system.insert(
            self.names.intern(b"Registry"),
            Object::String(PdfString::literal(b"Adobe".to_vec())),
        );
        system.insert(
            self.names.intern(b"Ordering"),
            Object::String(PdfString::literal(b"Identity".to_vec())),
        );
        system.insert(self.names.intern(b"Supplement"), Object::Int(0));

        let mut descendant = Dict::new();
        descendant.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        descendant.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"CIDFontType2")),
        );
        descendant.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        descendant.insert(self.names.intern(b"CIDSystemInfo"), Object::Dict(system));
        descendant.insert(
            self.names.intern(b"FontDescriptor"),
            Object::Ref(font.descriptor_ref),
        );
        // 9.7.4.3: the width every CID not in `/W` takes. Written explicitly
        // so the absence of a `/W` entry has a stated answer rather than one a
        // reader has to know the default of.
        descendant.insert(self.names.intern(b"DW"), Object::Int(1000));
        if let Some(widths) = width_array(&sfnt, units, mapping.keys().copied()) {
            descendant.insert(self.names.intern(b"W"), Object::Array(widths));
        }
        // 9.7.4.2: `/Identity` makes the CID the glyph index. This is the
        // entry that makes an index addressable, and the reason `subset` may
        // be applied at all — it preserves glyph ids, so the map stays true.
        descendant.insert(
            self.names.intern(b"CIDToGIDMap"),
            Object::Name(self.names.intern(b"Identity")),
        );
        self.objects
            .insert(font.descendant_ref.num, Object::Dict(descendant));

        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Font")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Type0")),
        );
        dict.insert(
            self.names.intern(b"BaseFont"),
            Object::Name(self.names.intern(base_font)),
        );
        dict.insert(
            self.names.intern(b"Encoding"),
            Object::Name(self.names.intern(b"Identity-H")),
        );
        dict.insert(
            self.names.intern(b"DescendantFonts"),
            Object::Array(vec![Object::Ref(font.descendant_ref)]),
        );
        // No `/ToUnicode` at all when no glyph stood for anything: 9.10.3's
        // CMap grammar has no zero-entry `beginbfchar` section, so the only
        // alternatives are a stream that says something and no stream.
        if let Some(cmap) = to_unicode_cmap(mapping) {
            self.objects.insert_stream(
                font.to_unicode_ref.num,
                StreamData {
                    dict: Dict::new(),
                    data: cmap,
                },
            );
            dict.insert(
                self.names.intern(b"ToUnicode"),
                Object::Ref(font.to_unicode_ref),
            );
        }
        self.objects.insert(font.font_ref.num, Object::Dict(dict));
    }

    /// Registers an image under a resource name.
    ///
    /// Returns false when the data does not describe an image of the size it
    /// claims, rather than writing a stream a reader would choke on.
    pub fn add_image(&mut self, resource: &[u8], image: &ImageData<'_>) -> bool {
        let r = self.allocate();
        let mut dict = Dict::new();
        dict.insert(Name::TYPE, Object::Name(self.names.intern(b"XObject")));
        dict.insert(
            self.names.intern(b"Subtype"),
            Object::Name(self.names.intern(b"Image")),
        );

        let data = match image {
            ImageData::Jpeg(bytes) => {
                // The dimensions live in the JPEG's own SOF marker and must
                // agree with /Width and /Height, so they are read out of it
                // rather than taken on trust.
                let Some((width, height, components)) = jpeg_shape(bytes) else {
                    return false;
                };
                dict.insert(self.names.intern(b"Width"), Object::Int(i64::from(width)));
                dict.insert(self.names.intern(b"Height"), Object::Int(i64::from(height)));
                dict.insert(self.names.intern(b"BitsPerComponent"), Object::Int(8));
                dict.insert(
                    self.names.intern(b"ColorSpace"),
                    Object::Name(self.names.intern(match components {
                        1 => b"DeviceGray".as_slice(),
                        4 => b"DeviceCMYK",
                        _ => b"DeviceRGB",
                    })),
                );
                dict.insert(Name::FILTER, Object::Name(self.names.intern(b"DCTDecode")));
                bytes.to_vec()
            }
            ImageData::Rgb8 {
                width,
                height,
                data,
            }
            | ImageData::Gray8 {
                width,
                height,
                data,
            } => {
                let gray = matches!(image, ImageData::Gray8 { .. });
                let n = if gray { 1 } else { 3 };
                let expected = (*width as usize)
                    .saturating_mul(*height as usize)
                    .saturating_mul(n);
                if *width == 0 || *height == 0 || data.len() < expected {
                    return false;
                }
                dict.insert(self.names.intern(b"Width"), Object::Int(i64::from(*width)));
                dict.insert(
                    self.names.intern(b"Height"),
                    Object::Int(i64::from(*height)),
                );
                dict.insert(self.names.intern(b"BitsPerComponent"), Object::Int(8));
                dict.insert(
                    self.names.intern(b"ColorSpace"),
                    Object::Name(self.names.intern(if gray {
                        b"DeviceGray".as_slice()
                    } else {
                        b"DeviceRGB"
                    })),
                );
                data.get(..expected).unwrap_or(data).to_vec()
            }
            ImageData::Compressed(image) => {
                let Some(data) = self.compressed_image(&mut dict, image) else {
                    return false;
                };
                data
            }
        };

        self.objects.insert_stream(r.num, StreamData { dict, data });
        self.resources.images.push((resource.to_vec(), r));
        true
    }

    /// Stops later pages from inheriting the images registered so far.
    ///
    /// [`DocumentBuilder::add_image`] registers an image on the **document**,
    /// and every page added after it names that image in
    /// `/Resources /XObject` — which is what a caller drawing one logo on
    /// every page wants, and is quadratic for a caller drawing one image on
    /// one page. Gap 29 is the second: a CBZ synthesises one page per archive
    /// entry and no page draws another page's image, so a 200-page archive
    /// would otherwise carry 20 100 resource entries for the 200 it uses, and
    /// a 4 096-page one would carry 8 390 656.
    ///
    /// Nothing already written is touched. The image objects are in the file
    /// and the pages that named them still do; this clears only the list a
    /// *future* [`DocumentBuilder::add_page`] copies. An image cleared before
    /// any page named it is an unreferenced stream, which is the caller's
    /// mistake to avoid rather than this method's to prevent.
    pub fn clear_image_resources(&mut self) {
        self.resources.images.clear();
    }

    /// Fills an image dictionary for bytes that are already encoded.
    ///
    /// Everything is checked **before** anything is written: a refused image
    /// that had already inserted its `/SMask` would leave an unreferenced
    /// stream in the file, and `add_image` returning false would stop being the
    /// whole story.
    fn compressed_image(
        &mut self,
        dict: &mut Dict,
        image: &CompressedImage<'_>,
    ) -> Option<Vec<u8>> {
        if image.width == 0 || image.height == 0 || image.data.is_empty() {
            return None;
        }
        if !is_legal_depth(image.bits_per_component) {
            return None;
        }
        let components = image.color_space.components();
        if !filter_describes(
            image.filter,
            components,
            image.bits_per_component,
            image.width,
        ) {
            return None;
        }

        // 8.6.6.3: `/hival` is at most 255, and every index the samples can
        // name has to be one the table holds — so a 4-bit indexed image may
        // carry sixteen entries and a 1-bit one may carry two.
        let mut entries = 0usize;
        if let ImageColorSpace::Indexed { base, lookup } = image.color_space {
            let per = base.components() as usize;
            if lookup.is_empty() || lookup.len() % per != 0 {
                return None;
            }
            entries = lookup.len() / per;
            if image.bits_per_component > 8 || entries > 1usize << image.bits_per_component {
                return None;
            }
        }

        if let Some(ranges) = image.color_key_mask {
            // 8.9.6.4: 2 x n integers, "each in the range 0 to
            // 2^BitsPerComponent - 1", and a min above its max names an empty
            // range that would mask nothing while looking as though it did.
            if ranges.len() != components as usize {
                return None;
            }
            let ceiling = max_sample(image.bits_per_component);
            if ranges.iter().any(|&(lo, hi)| lo > hi || hi > ceiling) {
                return None;
            }
        }

        if let Some(mask) = &image.soft_mask {
            if mask.width == 0 || mask.height == 0 || mask.data.is_empty() {
                return None;
            }
            if !is_legal_depth(mask.bits_per_component) {
                return None;
            }
            // A soft mask is one-component `/DeviceGray`, so its own predictor
            // parameters are checked against that rather than against the
            // image's.
            if !filter_describes(mask.filter, 1, mask.bits_per_component, mask.width) {
                return None;
            }
        }

        dict.insert(
            self.names.intern(b"Width"),
            Object::Int(i64::from(image.width)),
        );
        dict.insert(
            self.names.intern(b"Height"),
            Object::Int(i64::from(image.height)),
        );
        dict.insert(
            self.names.intern(b"BitsPerComponent"),
            Object::Int(i64::from(image.bits_per_component)),
        );
        let space = match image.color_space {
            ImageColorSpace::DeviceGray => Object::Name(self.names.intern(b"DeviceGray")),
            ImageColorSpace::DeviceRgb => Object::Name(self.names.intern(b"DeviceRGB")),
            ImageColorSpace::DeviceCmyk => Object::Name(self.names.intern(b"DeviceCMYK")),
            ImageColorSpace::Indexed { base, lookup } => Object::Array(vec![
                Object::Name(self.names.intern(b"Indexed")),
                Object::Name(self.names.intern(base.pdf_name())),
                // Checked above to be at least one, so this cannot wrap.
                Object::Int(entries as i64 - 1),
                // A hex string rather than a literal: a palette is arbitrary
                // bytes, and hex needs no escaping decisions at all.
                Object::String(PdfString::hex(lookup.to_vec())),
            ]),
        };
        dict.insert(self.names.intern(b"ColorSpace"), space);
        if let Some(filter) = image.filter {
            self.insert_filter(dict, filter);
        }
        if let Some(ranges) = image.color_key_mask {
            dict.insert(
                self.names.intern(b"Mask"),
                Object::Array(
                    ranges
                        .iter()
                        .flat_map(|&(lo, hi)| {
                            [Object::Int(i64::from(lo)), Object::Int(i64::from(hi))]
                        })
                        .collect(),
                ),
            );
        }
        if let Some(mask) = &image.soft_mask {
            let reference = self.allocate();
            let mut md = Dict::new();
            md.insert(Name::TYPE, Object::Name(self.names.intern(b"XObject")));
            md.insert(
                self.names.intern(b"Subtype"),
                Object::Name(self.names.intern(b"Image")),
            );
            md.insert(
                self.names.intern(b"Width"),
                Object::Int(i64::from(mask.width)),
            );
            md.insert(
                self.names.intern(b"Height"),
                Object::Int(i64::from(mask.height)),
            );
            md.insert(
                self.names.intern(b"BitsPerComponent"),
                Object::Int(i64::from(mask.bits_per_component)),
            );
            md.insert(
                self.names.intern(b"ColorSpace"),
                Object::Name(self.names.intern(b"DeviceGray")),
            );
            if let Some(filter) = mask.filter {
                self.insert_filter(&mut md, filter);
            }
            self.objects.insert_stream(
                reference.num,
                StreamData {
                    dict: md,
                    data: mask.data.to_vec(),
                },
            );
            dict.insert(self.names.intern(b"SMask"), Object::Ref(reference));
        }

        Some(image.data.to_vec())
    }

    /// Writes `/Filter` and, where the filter has any, `/DecodeParms`.
    fn insert_filter(&mut self, dict: &mut Dict, filter: ImageFilter) {
        let name = match filter {
            ImageFilter::Dct => b"DCTDecode".as_slice(),
            _ => b"FlateDecode",
        };
        dict.insert(Name::FILTER, Object::Name(self.names.intern(name)));

        if let ImageFilter::FlatePngPredictor {
            colors,
            bits_per_component,
            columns,
        } = filter
        {
            let mut parms = Dict::new();
            // 7.4.4.4 Table 10, in the table's own order.
            parms.insert(self.names.intern(b"Predictor"), Object::Int(15));
            parms.insert(self.names.intern(b"Colors"), Object::Int(i64::from(colors)));
            parms.insert(
                self.names.intern(b"BitsPerComponent"),
                Object::Int(i64::from(bits_per_component)),
            );
            parms.insert(
                self.names.intern(b"Columns"),
                Object::Int(i64::from(columns)),
            );
            dict.insert(Name::DECODE_PARMS, Object::Dict(parms));
        }
    }

    /// Adds a page, drawing it with the given closure.
    ///
    /// A closure rather than a returned reference so the API stays infallible:
    /// there is no borrow to fumble and no case where "the page just pushed"
    /// has to be recovered from an `Option`.
    pub fn add_page(&mut self, width: f64, height: f64, draw: impl FnOnce(&mut PageBuilder)) {
        let mut page = PageBuilder {
            width,
            height,
            content: Vec::new(),
            resources: self.resources.clone(),
            composite: self.composite.clone(),
            used: BTreeMap::new(),
            drawn: BTreeMap::new(),
            crop_box: None,
            bleed_box: None,
            links: Vec::new(),
        };
        draw(&mut page);
        self.pages.push(page);
    }

    /// Sets an `/Info` field.
    pub fn set_info(&mut self, key: &[u8], value: &str) {
        let name = self.names.intern(key);
        self.info.insert(
            name,
            Object::String(PdfString::literal(value.as_bytes().to_vec())),
        );
    }

    /// Sets the document outline (12.3.3).
    ///
    /// Returns false, **leaving whatever outline was already set standing**,
    /// for a tree this repository's own reader could not read back: one nested
    /// deeper than [`crate::limits::MAX_NEST_DEPTH`], one with a level of more
    /// than [`crate::limits::MAX_TREE_ENTRIES`] siblings, or one carrying a
    /// target that cannot be written. See [`outline_is_writable`] for why
    /// those are the two limits and why neither is new.
    ///
    /// An empty vector is accepted and clears the outline: a document with no
    /// outline is written with no `/Outlines` at all, which is not the same
    /// file as one with an empty outline dictionary.
    pub fn set_outline(&mut self, entries: Vec<OutlineEntry>) -> bool {
        if !outline_is_writable(&entries) {
            return false;
        }
        self.outline = entries;
        true
    }

    /// Serializes the document.
    #[must_use]
    pub fn finish(mut self) -> Vec<u8> {
        let page_refs: Vec<ObjRef> = (0..self.pages.len()).map(|_| self.allocate()).collect();
        let pages_ref = ObjRef::new(2, 0);

        let pages = std::mem::take(&mut self.pages);

        // Every character every page drew, per font resource. Gathered before
        // anything is written because the embedded programs are subset to it.
        let mut used: BTreeMap<Vec<u8>, BTreeSet<char>> = BTreeMap::new();
        // The runs written into streams this builder does not own go in first,
        // so that first-non-empty-wins is decided by the order the caller drew
        // them in and not by whether a run happened to reach a `PageBuilder`.
        let mut drawn: BTreeMap<Vec<u8>, BTreeMap<u16, String>> = std::mem::take(&mut self.drawn);
        for page in &pages {
            for (resource, characters) in &page.used {
                used.entry(resource.clone())
                    .or_default()
                    .extend(characters.iter().copied());
            }
            // The first non-empty text a glyph is drawn with stands, across
            // the document as well as within a page — `/ToUnicode` is one
            // function from code to text and a glyph drawn twice with two
            // meanings has to resolve to one of them.
            for (resource, glyphs) in &page.drawn {
                let into = drawn.entry(resource.clone()).or_default();
                for (id, text) in glyphs {
                    let slot = into.entry(*id).or_default();
                    if slot.is_empty() && !text.is_empty() {
                        slot.clone_from(text);
                    }
                }
            }
        }
        self.write_embedded_fonts(&used);
        self.write_cid_fonts(&drawn);

        for (page, reference) in pages.iter().zip(page_refs.iter()) {
            let content_ref = self.allocate();
            self.objects.insert_stream(
                content_ref.num,
                StreamData {
                    dict: Dict::new(),
                    data: page.content.clone(),
                },
            );

            let resources = page.resources.dict(&self.names);

            let mut dict = Dict::new();
            dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Page")));
            dict.insert(Name::PARENT, Object::Ref(pages_ref));
            dict.insert(
                Name::MEDIA_BOX,
                Object::Array(vec![
                    Object::Int(0),
                    Object::Int(0),
                    Object::Real(page.width),
                    Object::Real(page.height),
                ]),
            );
            // Written only when the caller stated one, and in 7.7.3.3's own
            // order after the media box, so a page's boxes read down the
            // dictionary the way the specification lists them.
            for (key, rect) in [
                (&b"CropBox"[..], page.crop_box),
                (&b"BleedBox"[..], page.bleed_box),
            ] {
                let Some(rect) = rect else {
                    continue;
                };
                let name = self.names.intern(key);
                dict.insert(
                    name,
                    Object::Array(rect.iter().map(|v| Object::Real(*v)).collect()),
                );
            }
            dict.insert(Name::RESOURCES, Object::Dict(resources));
            dict.insert(Name::CONTENTS, Object::Ref(content_ref));

            // 12.5.2: the page's annotations, each an indirect object. `/Annots`
            // is written only when there are some — an empty array is a
            // statement where its absence is not, the same rule `/CropBox`
            // above follows.
            let mut annots = Vec::with_capacity(page.links.len());
            for link in &page.links {
                let mut annot = Dict::new();
                annot.insert(Name::TYPE, Object::Name(self.names.intern(b"Annot")));
                annot.insert(
                    self.names.intern(b"Subtype"),
                    Object::Name(self.names.intern(b"Link")),
                );
                annot.insert(
                    self.names.intern(b"Rect"),
                    Object::Array(link.rect.iter().map(|v| Object::Real(*v)).collect()),
                );
                annot.insert(self.names.intern(b"Border"), crate::dest::no_border());
                // 12.5.3 bit position 3: an annotation without it exists on
                // screen and not on paper, which is `edit::annot`'s rule for
                // every annotation it builds and is this one's too.
                annot.insert(self.names.intern(b"F"), Object::Int(4));
                link.target.write(&self.names, &page_refs, &mut annot);

                let annot_ref = self.allocate();
                self.objects.insert(annot_ref.num, Object::Dict(annot));
                annots.push(Object::Ref(annot_ref));
            }
            if !annots.is_empty() {
                dict.insert(self.names.intern(b"Annots"), Object::Array(annots));
            }

            self.objects.insert(reference.num, Object::Dict(dict));
        }

        // The page tree.
        let mut tree = Dict::new();
        tree.insert(Name::TYPE, Object::Name(self.names.intern(b"Pages")));
        tree.insert(
            Name::KIDS,
            Object::Array(page_refs.iter().map(|r| Object::Ref(*r)).collect()),
        );
        tree.insert(Name::COUNT, Object::Int(page_refs.len() as i64));
        self.objects.insert(2, Object::Dict(tree));

        // The catalog, and the outline if there is one.
        let mut catalog = Dict::new();
        catalog.insert(Name::TYPE, Object::Name(self.names.intern(b"Catalog")));
        catalog.insert(Name::PAGES, Object::Ref(pages_ref));

        let outline = std::mem::take(&mut self.outline);
        if !outline.is_empty() {
            let root = self.allocate();
            let children = self.write_outline(&outline, &page_refs, root);
            let mut dict = Dict::new();
            dict.insert(Name::TYPE, Object::Name(self.names.intern(b"Outlines")));
            if let Some((first, last, visible)) = children {
                dict.insert(self.names.intern(b"First"), Object::Ref(first));
                dict.insert(self.names.intern(b"Last"), Object::Ref(last));
                // 12.3.3 Table 152: the total number of *visible* items at all
                // levels — every top-level entry, plus everything an open one
                // exposes. Never negative: the root of an outline is the one
                // node that cannot be closed.
                dict.insert(Name::COUNT, Object::Int(visible));
            }
            self.objects.insert(root.num, Object::Dict(dict));
            catalog.insert(self.names.intern(b"Outlines"), Object::Ref(root));
        }
        self.objects.insert(1, Object::Dict(catalog));

        let mut trailer = Dict::new();
        trailer.insert(Name::ROOT, Object::Ref(ObjRef::new(1, 0)));
        if !self.info.is_empty() {
            let info_ref = self.allocate();
            let info = std::mem::take(&mut self.info);
            self.objects.insert(info_ref.num, Object::Dict(info));
            trailer.insert(Name::INFO, Object::Ref(info_ref));
        }

        rewrite(
            &self.objects,
            &trailer,
            &WriteOptions::default(),
            &self.names,
        )
    }

    /// Writes one level of outline entries, returning `(first, last, visible)`.
    ///
    /// `visible` is what this level contributes to the count of the node above
    /// it: one for each entry, since every entry of a level that is shown at
    /// all is shown — plus, for an entry that is **open**, everything its own
    /// subtree exposes. A closed entry contributes exactly itself, which is
    /// what makes the number a count of what a reader can see rather than a
    /// count of what exists.
    ///
    /// Recursion is safe here because [`DocumentBuilder::set_outline`] has
    /// already walked the tree iteratively and refused anything deeper than
    /// [`crate::limits::MAX_NEST_DEPTH`].
    fn write_outline(
        &mut self,
        entries: &[OutlineEntry],
        pages: &[ObjRef],
        parent: ObjRef,
    ) -> Option<(ObjRef, ObjRef, i64)> {
        if entries.is_empty() {
            return None;
        }

        let refs: Vec<ObjRef> = entries.iter().map(|_| self.allocate()).collect();
        let mut visible: i64 = 0;

        for (index, entry) in entries.iter().enumerate() {
            let Some(&reference) = refs.get(index) else {
                continue;
            };
            let children = self.write_outline(&entry.children, pages, reference);

            let mut dict = Dict::new();
            dict.insert(
                self.names.intern(b"Title"),
                Object::String(PdfString::literal(entry.title.as_bytes().to_vec())),
            );
            dict.insert(Name::PARENT, Object::Ref(parent));

            // Ruling 6: an explicit destination, never a name that looks like
            // one. This is the writer side of the defect that made the engine
            // being replaced turn "#page=2" into a dead named destination.
            if let Some(target) = &entry.target {
                target.write(&self.names, pages, &mut dict);
            }

            if let Some(&previous) = index.checked_sub(1).and_then(|i| refs.get(i)) {
                dict.insert(self.names.intern(b"Prev"), Object::Ref(previous));
            }
            if let Some(&next) = refs.get(index + 1) {
                dict.insert(self.names.intern(b"Next"), Object::Ref(next));
            }

            visible += 1;
            if let Some((first, last, below)) = children {
                dict.insert(self.names.intern(b"First"), Object::Ref(first));
                dict.insert(self.names.intern(b"Last"), Object::Ref(last));
                // 12.3.3 Table 153, and the sign is the whole entry. An open
                // item states how many descendants are showing; a **closed**
                // one states the negative of how many would show if it were
                // reopened. The magnitude is the same number in both cases,
                // so a writer that got only the sign wrong would produce a
                // tree that opens the wrong way in every viewer and reads
                // back identically through any reader that ignores it.
                dict.insert(
                    Name::COUNT,
                    Object::Int(if entry.open { below } else { -below }),
                );
                if entry.open {
                    visible += below;
                }
            }

            self.objects.insert(reference.num, Object::Dict(dict));
        }

        match (refs.first(), refs.last()) {
            (Some(&first), Some(&last)) => Some((first, last, visible)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CosDocument;

    #[test]
    fn a_built_document_opens_and_reports_its_pages() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for i in 1..=3 {
            builder.add_page(595.0, 842.0, |page| {
                page.text(b"F0", 18.0, 72.0, 742.0, &format!("Built page {i} of 3"));
            });
        }
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("the built document opens");
        assert_eq!(crate::pages::count(&doc), 3);
        assert_eq!(
            doc.ladder_level(),
            crate::LadderLevel::Trust,
            "our own output should need no repair"
        );
        assert!(
            doc.warnings().is_empty(),
            "nor provoke any leniency: {:?}",
            doc.warnings()
        );
    }

    #[test]
    fn built_pages_carry_their_geometry_and_content() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, "Hello");
        });
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let first = pages.first().expect("a page");

        assert_eq!(first.media_box.width(), 200.0);
        assert_eq!(first.media_box.height(), 100.0);

        let content = crate::pages::content_bytes(&doc, first);
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("BT"), "text operators: {text}");
        assert!(text.contains("(Hello)"), "the string: {text}");
    }

    /// `/CropBox` and `/BleedBox` are written when stated and **absent when
    /// not**, which is the half a writer gets wrong by defaulting them.
    ///
    /// 7.7.3.3 makes an absent `/CropBox` mean "the media box", so writing one
    /// equal to the media box on every page is not the same document: it turns
    /// a page that said nothing about its visible region into one that said the
    /// whole of it. Gap 30's fixed pages need exactly this distinction — only a
    /// `FixedPage` carrying a `ContentBox` gets a crop box — so both directions
    /// are asserted here rather than only the interesting one.
    #[test]
    fn a_page_carries_the_boxes_it_was_given_and_no_others() {
        let mut builder = DocumentBuilder::new();
        builder.add_page(612.0, 792.0, |page| {
            page.set_crop_box(72.0, 684.0, 216.0, 756.0);
            page.set_bleed_box(0.0, 0.0, 612.0, 792.0);
        });
        builder.add_page(612.0, 792.0, |page| {
            page.fill_rect(0.0, 0.0, 612.0, 792.0, 0.5);
        });
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let pages = crate::pages::collect(&doc);
        assert_eq!(pages.len(), 2);

        let stated = pages.first().expect("the first page");
        assert_eq!(
            (
                stated.crop_box.x0,
                stated.crop_box.y0,
                stated.crop_box.x1,
                stated.crop_box.y1
            ),
            (72.0, 684.0, 216.0, 756.0)
        );
        assert_eq!(
            stated.media_box.width(),
            612.0,
            "the media box is untouched"
        );

        // 7.7.3.3: an absent `/CropBox` reads back as the media box, so the
        // page object is what has to be inspected rather than the resolved
        // rectangle -- the two are indistinguishable through `Page`.
        let bare = pages.get(1).expect("the second page");
        let object = doc.get(bare.reference).expect("the page object");
        let dict = object.as_dict().expect("a page dictionary");
        for key in [&b"CropBox"[..], b"BleedBox"] {
            assert!(
                dict.get(doc.intern(key)).is_none(),
                "a page given no boxes carries none: /{}",
                String::from_utf8_lossy(key)
            );
        }

        let bleed: Vec<f64> = doc
            .get(stated.reference)
            .expect("the page object")
            .as_dict()
            .and_then(|d| d.get_array(doc.intern(b"BleedBox")))
            .map(|values| values.iter().filter_map(Object::as_number).collect())
            .unwrap_or_default();
        assert_eq!(bleed, vec![0.0, 0.0, 612.0, 792.0]);
    }

    #[test]
    fn strings_with_parentheses_survive_the_round_trip() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(300.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, r"a (nested) string\here");
        });
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let content = crate::pages::content_bytes(&doc, pages.first().expect("a page"));
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains(r"\(nested\)"), "escaped: {text}");
    }

    /// A whole-page target, which is what an entry that only names a page
    /// wants.
    fn page_target(index: u32) -> Target {
        Target::Page {
            index,
            view: DestKind::Fit,
        }
    }

    /// Ruling 6 on the writer side: an outline destination round-trips as an
    /// explicit one.
    #[test]
    fn a_built_outline_round_trips_with_explicit_destinations() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        for _ in 0..6 {
            builder.add_page(595.0, 842.0, |_| {});
        }
        assert!(builder.set_outline(vec![
            OutlineEntry {
                title: "Part One".to_string(),
                target: Some(page_target(0)),
                open: true,
                children: vec![OutlineEntry {
                    title: "Chapter 1".to_string(),
                    target: Some(page_target(1)),
                    open: true,
                    children: vec![OutlineEntry {
                        title: "Section 1.1".to_string(),
                        target: Some(page_target(2)),
                        open: true,
                        children: Vec::new(),
                    }],
                }],
            },
            OutlineEntry {
                title: "Part Two".to_string(),
                target: Some(page_target(4)),
                open: true,
                children: Vec::new(),
            },
        ]));
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let items = crate::outline::outline(&doc);
        let flat = crate::OutlineItem::flatten(&items);

        let seen: Vec<(u32, String, Option<u32>)> = flat
            .iter()
            .map(|(depth, item)| {
                let page = match &item.destination {
                    Some(crate::Destination::Explicit { page_index, .. }) => *page_index,
                    other => panic!("expected an explicit destination, got {other:?}"),
                };
                (*depth, item.title.clone(), page)
            })
            .collect();

        // The same shape the MuPDF-generated fixture has.
        for want in [
            (0u32, "Part One", Some(0u32)),
            (1, "Chapter 1", Some(1)),
            (2, "Section 1.1", Some(2)),
            (0, "Part Two", Some(4)),
        ] {
            assert!(
                seen.iter()
                    .any(|(d, t, p)| *d == want.0 && t == want.1 && *p == want.2),
                "expected {want:?}; got {seen:#?}"
            );
        }
    }

    #[test]
    fn metadata_round_trips() {
        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |_| {});
        builder.set_info(b"Title", "A Built Document");
        builder.set_info(b"Producer", "tinker-pdf");
        let bytes = builder.finish();

        let doc = CosDocument::open(bytes).expect("it opens");
        let meta = crate::outline::metadata(&doc);
        assert_eq!(meta.title.as_deref(), Some("A Built Document"));
        assert_eq!(meta.producer.as_deref(), Some("tinker-pdf"));
    }

    #[test]
    fn an_empty_document_is_still_a_document() {
        let bytes = DocumentBuilder::new().finish();
        let doc = CosDocument::open(bytes).expect("even an empty one opens");
        assert_eq!(crate::pages::count(&doc), 0);
        assert!(doc.catalog().is_some());
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::CosDocument;

    #[test]
    fn a_raw_image_round_trips_through_the_reader() {
        let mut builder = DocumentBuilder::new();
        // Two by two: red, green, blue, white.
        let pixels = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 2,
                data: &pixels,
            }
        ));
        builder.add_page(100.0, 100.0, |page| {
            page.image(b"Im0", 10.0, 10.0, 50.0, 50.0);
        });

        let doc = CosDocument::open(builder.finish()).expect("it opens");
        let pages = crate::pages::collect(&doc);
        let content = crate::pages::content_bytes(&doc, pages.first().expect("a page"));
        let text = String::from_utf8_lossy(&content);

        assert!(text.contains("/Im0 Do"), "the image is drawn: {text}");
        assert!(text.contains("50 0 0 50 10 10 cm"), "and placed: {text}");
    }

    #[test]
    fn an_images_samples_survive_the_round_trip() {
        let mut builder = DocumentBuilder::new();
        let pixels = [1u8, 2, 3, 4, 5, 6];
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 1,
                data: &pixels,
            }
        ));
        builder.add_page(10.0, 10.0, |_| {});

        let doc = CosDocument::open(builder.finish()).expect("it opens");
        // The image is the only stream with /Subtype /Image.
        let subtype = doc.intern(b"Subtype");
        let found = (1..20u32).find_map(|num| {
            let r = ObjRef::new(num, 0);
            let object = doc.get(r).ok()?;
            let dict = object.as_dict()?;
            let is_image = dict
                .get(subtype)
                .and_then(Object::as_name)
                .and_then(|n| doc.name_bytes(n))
                .is_some_and(|b| b.as_ref() == b"Image");
            is_image.then_some(r)
        });

        let reference = found.expect("an image object");
        assert_eq!(
            doc.stream_decoded(reference).ok(),
            Some(pixels.to_vec()),
            "the samples come back unchanged"
        );
    }

    #[test]
    fn a_jpeg_is_embedded_without_being_re_encoded() {
        // A minimal JPEG header declaring 4x3, three components.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        jpeg.extend_from_slice(&3u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&4u16.to_be_bytes()); // width
        jpeg.push(3); // components
        jpeg.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);

        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(b"Im0", &ImageData::Jpeg(&jpeg)));
        builder.add_page(10.0, 10.0, |_| {});

        let bytes = builder.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/DCTDecode"), "kept as JPEG");
        assert!(
            text.contains("/Width 4") && text.contains("/Height 3"),
            "dimensions read from the frame header, not guessed"
        );

        // And the exact bytes are still in the file.
        assert!(
            bytes.windows(jpeg.len()).any(|w| w == jpeg),
            "the JPEG data is embedded byte for byte"
        );
    }

    /// A compressed image with nothing wrong with it, for the refusals below to
    /// be measured against.
    fn sound() -> CompressedImage<'static> {
        CompressedImage {
            width: 4,
            height: 2,
            bits_per_component: 8,
            color_space: ImageColorSpace::DeviceRgb,
            filter: Some(ImageFilter::FlatePngPredictor {
                colors: 3,
                bits_per_component: 8,
                columns: 4,
            }),
            data: b"encoded",
            color_key_mask: None,
            soft_mask: None,
        }
    }

    /// Every way a pre-compressed image can fail to describe an image, refused
    /// rather than written out as a dictionary a reader will choke on.
    ///
    /// The control comes first on purpose: a `compressed_image` that returned
    /// `None` unconditionally would satisfy every other line here.
    #[test]
    fn a_compressed_image_that_does_not_describe_an_image_is_refused() {
        let mut builder = DocumentBuilder::new();
        assert!(
            builder.add_image(b"Ok", &ImageData::Compressed(sound())),
            "the control is accepted"
        );

        const LOOKUP: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let cases: Vec<(&str, CompressedImage<'_>)> = vec![
            (
                "no width",
                CompressedImage {
                    width: 0,
                    ..sound()
                },
            ),
            (
                "no height",
                CompressedImage {
                    height: 0,
                    ..sound()
                },
            ),
            (
                "no data",
                CompressedImage {
                    data: b"",
                    ..sound()
                },
            ),
            // Table 89 admits 1, 2, 4, 8 and 16 and nothing else. A depth of 3
            // would be read as 3 by the sample loop and produce a row stride
            // nothing in the file agrees with.
            (
                "a depth outside Table 89",
                CompressedImage {
                    bits_per_component: 3,
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 3,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            (
                "a depth of zero",
                CompressedImage {
                    bits_per_component: 0,
                    filter: None,
                    ..sound()
                },
            ),
            // 7.4.4.4: the three parameters describe the bytes, so a set that
            // disagrees with the image's own geometry unfilters at a stride the
            // data never had.
            (
                "/Columns that is not the width",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 8,
                        columns: 3,
                    }),
                    ..sound()
                },
            ),
            (
                "/Colors that is not the component count",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 1,
                        bits_per_component: 8,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            (
                "/BitsPerComponent that is not the depth",
                CompressedImage {
                    filter: Some(ImageFilter::FlatePngPredictor {
                        colors: 3,
                        bits_per_component: 4,
                        columns: 4,
                    }),
                    ..sound()
                },
            ),
            // 8.6.6.3.
            (
                "an empty palette",
                CompressedImage {
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: b"",
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "a palette that is not whole entries",
                CompressedImage {
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP[..8],
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "a palette larger than the depth can index",
                CompressedImage {
                    bits_per_component: 1,
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP,
                    },
                    filter: None,
                    ..sound()
                },
            ),
            (
                "an indexed image at sixteen bits",
                CompressedImage {
                    bits_per_component: 16,
                    color_space: ImageColorSpace::Indexed {
                        base: DeviceSpace::Rgb,
                        lookup: &LOOKUP,
                    },
                    filter: None,
                    ..sound()
                },
            ),
            // 8.9.6.4.
            (
                "a colour key of the wrong length",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0)]),
                    ..sound()
                },
            ),
            (
                "a colour key past the depth's range",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0), (0, 0), (0, 256)]),
                    ..sound()
                },
            ),
            (
                "a colour key whose minimum is above its maximum",
                CompressedImage {
                    color_key_mask: Some(&[(0, 0), (0, 0), (9, 8)]),
                    ..sound()
                },
            ),
            // 11.6.5.3.
            (
                "a soft mask with no width",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 0,
                        height: 2,
                        bits_per_component: 8,
                        filter: None,
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask with no data",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 8,
                        filter: None,
                        data: b"",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask at a depth outside Table 89",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 7,
                        filter: None,
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
            (
                "a soft mask whose predictor claims three colours",
                CompressedImage {
                    soft_mask: Some(SoftMask {
                        width: 4,
                        height: 2,
                        bits_per_component: 8,
                        filter: Some(ImageFilter::FlatePngPredictor {
                            colors: 3,
                            bits_per_component: 8,
                            columns: 4,
                        }),
                        data: b"xx",
                    }),
                    ..sound()
                },
            ),
        ];

        for (what, image) in cases {
            assert!(
                !builder.add_image(b"Im", &ImageData::Compressed(image)),
                "{what} was accepted"
            );
        }
    }

    /// A refusal writes nothing at all — not even the part that was checked
    /// before the part that failed.
    ///
    /// The soft mask is the last thing validated and the only thing that gets
    /// an object of its own, so it is the one place an ordering mistake would
    /// leave an unreferenced stream behind in every file that hit it.
    #[test]
    fn a_refused_image_leaves_no_orphan_stream_behind() {
        let mut builder = DocumentBuilder::new();
        assert!(!builder.add_image(
            b"Im0",
            &ImageData::Compressed(CompressedImage {
                // Sound in every respect except its mask.
                soft_mask: Some(SoftMask {
                    width: 4,
                    height: 0,
                    bits_per_component: 8,
                    filter: None,
                    data: b"the mask that must not be written",
                }),
                ..sound()
            })
        ));
        builder.add_page(10.0, 10.0, |_| {});

        let bytes = builder.finish();
        assert!(
            !bytes
                .windows(33)
                .any(|w| w == b"the mask that must not be written"),
            "the mask was written despite the image being refused"
        );
        assert!(
            !String::from_utf8_lossy(&bytes).contains("/Subtype /Image"),
            "and no image XObject exists at all"
        );
    }

    /// A pre-compressed image with **no** filter is placed as raw samples and
    /// declares none, which is the other half of the `maybe_compress` contract:
    /// the rule is "a `/Filter` key means hands off", not "this variant means
    /// hands off".
    ///
    /// It also records something a reader of this variant needs to know.
    /// `WriteOptions::default()` has `compress: false`, and `finish` uses the
    /// default — so the writer will *not* deflate these bytes on the way out.
    /// Anything handing over a raster has to compress it itself or the samples
    /// land in the file at full size, which is `png_embed`'s reason for
    /// re-deflating the decoded route rather than leaving it to the writer.
    #[test]
    fn a_compressed_image_with_no_filter_is_placed_as_raw_samples() {
        let samples: Vec<u8> = (0..3 * 64 * 64).map(|i| (i % 251) as u8).collect();
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_image(
            b"Im0",
            &ImageData::Compressed(CompressedImage {
                width: 64,
                height: 64,
                bits_per_component: 8,
                color_space: ImageColorSpace::DeviceRgb,
                filter: None,
                data: &samples,
                color_key_mask: None,
                soft_mask: None,
            })
        ));
        builder.add_page(64.0, 64.0, |page| page.image(b"Im0", 0.0, 0.0, 64.0, 64.0));

        let bytes = builder.finish();
        assert!(
            bytes.windows(samples.len()).any(|w| w == samples),
            "the samples are in the file verbatim"
        );
        let doc = CosDocument::open(bytes).expect("it opens");
        let found = (1..20u32)
            .map(|num| ObjRef::new(num, 0))
            .find(|r| doc.stream_decoded(*r).is_ok_and(|d| d == samples))
            .expect("the image object");
        let dict = doc
            .get(found)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .expect("a stream dictionary");
        assert!(
            !dict.contains_key(Name::FILTER),
            "and no filter is claimed for them"
        );
    }

    #[test]
    fn an_image_that_does_not_match_its_size_is_refused() {
        let mut builder = DocumentBuilder::new();
        // Three bytes cannot be a 2x2 RGB image.
        assert!(!builder.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 2,
                data: &[1, 2, 3],
            }
        ));
        assert!(!builder.add_image(b"Im1", &ImageData::Jpeg(b"not a jpeg")));
        assert!(!builder.add_image(
            b"Im2",
            &ImageData::Gray8 {
                width: 0,
                height: 5,
                data: &[],
            }
        ));
    }
}

/// The writer's missing half: `/ExtGState`, groups, shadings, patterns and a
/// composite font (gap 30 milestone 5).
///
/// Everything here is asserted against **the dictionary in the file**, read
/// back through this crate's own parser, rather than against the bytes the
/// builder emitted. The renderer's half of the criterion — write it, open it,
/// draw it, and compare against the same picture built by hand — is in
/// `crates/tinker-pdf/tests/writer_graphics.rs`, because it needs a renderer
/// and this crate has none.
///
/// # Why so many small tests rather than a few large ones
///
/// Almost every entry here has an independent twin that a single test cannot
/// tell apart. `/ca` and `/CA` are different parameters; `/I` and `/K` are
/// different flags over one group; `/W` and `/ToUnicode` are different
/// statements about one glyph; a stitching function can be wrong in the stitch
/// or in a sub-function. A suite with one test per *feature* passes with half
/// of each feature deleted, which is the shape this gap's milestones 2, 3 and 4
/// each found a survivor in.
#[cfg(test)]
mod graphics_tests {
    use super::*;
    use crate::CosDocument;

    /// A TrueType program with real metrics and a `cmap` that disagrees with
    /// the glyph a test draws.
    ///
    /// Four glyphs. Glyph 0 and glyph 1 are **empty**; glyphs 2 and 3 are
    /// filled squares. The `cmap` maps `A` to glyph **1**, so a build that
    /// addressed glyphs through characters would draw nothing where drawing
    /// glyph 2 draws a square — which is what
    /// `crates/tinker-pdf/tests/writer_graphics.rs` measures and what a
    /// composite font exists for.
    ///
    /// Advances are 600, 700, 800 and 900 **font units**, and the units per em
    /// is a parameter, so `/W` is only right if the scaling into PDF's
    /// thousandths happened.
    fn metric_font(units_per_em: u16, with_metrics: bool) -> Vec<u8> {
        let mut square = Vec::new();
        square.extend_from_slice(&1i16.to_be_bytes()); // one contour
        for value in [0i16, 0, 700, 700] {
            square.extend_from_slice(&value.to_be_bytes()); // bounding box
        }
        square.extend_from_slice(&3u16.to_be_bytes()); // last point index
        square.extend_from_slice(&0u16.to_be_bytes()); // no instructions
        square.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on curve, i16 deltas
        for dx in [0i16, 700, 0, -700] {
            square.extend_from_slice(&dx.to_be_bytes());
        }
        for dy in [0i16, 0, 700, 0] {
            square.extend_from_slice(&dy.to_be_bytes());
        }

        let size = square.len() as u32;
        let mut glyf = Vec::new();
        glyf.extend_from_slice(&square);
        glyf.extend_from_slice(&square);
        // Glyphs 0 and 1 are empty, so both squares sit at the front and the
        // offsets below point the last two glyphs at them.
        let loca_offsets: [u32; 5] = [0, 0, 0, size, size * 2];
        let mut loca = Vec::new();
        for offset in loca_offsets {
            loca.extend_from_slice(&offset.to_be_bytes());
        }

        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&units_per_em.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

        let mut maxp = vec![0u8; 32];
        maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&4u16.to_be_bytes());

        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&4u16.to_be_bytes());

        let mut hmtx = Vec::new();
        for advance in [600u16, 700, 800, 900] {
            hmtx.extend_from_slice(&advance.to_be_bytes());
            hmtx.extend_from_slice(&0i16.to_be_bytes());
        }

        // cmap format 4: `A` alone, mapping to glyph 1, plus the required
        // terminating segment at 0xFFFF.
        let first = u16::from(b'A');
        let mut sub = Vec::new();
        for value in [4u16, 32, 0, 4, 4, 1, 0] {
            sub.extend_from_slice(&value.to_be_bytes());
        }
        for value in [
            first,
            0xFFFF,
            0,
            first,
            0xFFFF,
            1u16.wrapping_sub(first),
            1,
            0,
            0,
        ] {
            sub.extend_from_slice(&value.to_be_bytes());
        }
        let mut cmap = Vec::new();
        for value in [0u16, 1, 3, 1] {
            cmap.extend_from_slice(&value.to_be_bytes());
        }
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&sub);

        let mut tables: Vec<(&[u8; 4], &[u8])> = vec![(b"cmap", &cmap), (b"glyf", &glyf)];
        tables.push((b"head", &head));
        if with_metrics {
            tables.push((b"hhea", &hhea));
            tables.push((b"hmtx", &hmtx));
        }
        tables.push((b"loca", &loca));
        tables.push((b"maxp", &maxp));

        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// The resource `name` of kind `kind` on the document's first page.
    fn resource(doc: &CosDocument, kind: &[u8], name: &[u8]) -> Dict {
        let pages = crate::pages::collect(doc);
        let page = pages.first().expect("a page");
        let resources = page.resources.as_ref().expect("the page has resources");
        let table = doc.resolve_key(resources, doc.intern(kind));
        let table = table
            .as_dict()
            .unwrap_or_else(|| panic!("no /{} sub-dictionary", String::from_utf8_lossy(kind)));
        let value = doc.resolve_key(table, doc.intern(name));
        value
            .as_dict()
            .cloned()
            .unwrap_or_else(|| panic!("no {} named", String::from_utf8_lossy(name)))
    }

    /// The name a key holds, as bytes.
    fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
        doc.resolve_key(dict, doc.intern(key))
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|bytes| bytes.to_vec())
    }

    /// The numbers an array key holds, flattened one level.
    fn numbers(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Vec<f64> {
        doc.resolve_key(dict, doc.intern(key))
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| doc.resolve(v).as_number())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn opened(builder: DocumentBuilder) -> CosDocument {
        CosDocument::open(builder.finish()).expect("the built document opens")
    }

    /// The content stream of the first page, as text.
    fn content(doc: &CosDocument) -> String {
        let pages = crate::pages::collect(doc);
        let bytes = crate::pages::content_bytes(doc, pages.first().expect("a page"));
        String::from_utf8_lossy(&bytes).into_owned()
    }

    // ---- /ExtGState ------------------------------------------------------

    /// `/ca` and `/CA` are **two** parameters, and each is written only when it
    /// was asked for.
    ///
    /// The failure this guards is not "the alpha is missing" — it is a writer
    /// that has one alpha and spells it into both keys, which draws every
    /// document anybody would write correctly and is wrong the moment a caller
    /// strokes and fills at different opacities. Both directions are asserted,
    /// because a test that only set the fill alpha would pass on a build that
    /// wrote `/ca` from `stroke_alpha`.
    #[test]
    fn fill_alpha_and_stroke_alpha_are_two_entries_and_not_one() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_ext_gstate(
            b"GFill",
            &ExtGState {
                fill_alpha: Some(0.25),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GStroke",
            &ExtGState {
                stroke_alpha: Some(0.75),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GBoth",
            &ExtGState {
                fill_alpha: Some(0.1),
                stroke_alpha: Some(0.9),
                ..ExtGState::default()
            }
        ));
        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);

        let fill = resource(&doc, b"ExtGState", b"GFill");
        assert_eq!(
            doc.resolve_key(&fill, doc.intern(b"ca")).as_number(),
            Some(0.25)
        );
        assert!(
            !fill.contains_key(doc.intern(b"CA")),
            "a state that said nothing about the stroke alpha says nothing"
        );

        let stroke = resource(&doc, b"ExtGState", b"GStroke");
        assert_eq!(
            doc.resolve_key(&stroke, doc.intern(b"CA")).as_number(),
            Some(0.75)
        );
        assert!(!stroke.contains_key(doc.intern(b"ca")));

        let both = resource(&doc, b"ExtGState", b"GBoth");
        assert_eq!(
            (
                doc.resolve_key(&both, doc.intern(b"ca")).as_number(),
                doc.resolve_key(&both, doc.intern(b"CA")).as_number()
            ),
            (Some(0.1), Some(0.9)),
            "and neither is read from the other"
        );

        assert_eq!(
            name_of(&doc, &both, b"Type").as_deref(),
            Some(&b"ExtGState"[..])
        );
    }

    /// Every one of 11.3.5's sixteen modes reaches `/BM` under its own name.
    #[test]
    fn every_blend_mode_reaches_the_state_under_its_own_name() {
        let modes = [
            (BlendMode::Normal, &b"Normal"[..]),
            (BlendMode::Multiply, b"Multiply"),
            (BlendMode::Screen, b"Screen"),
            (BlendMode::Overlay, b"Overlay"),
            (BlendMode::Darken, b"Darken"),
            (BlendMode::Lighten, b"Lighten"),
            (BlendMode::ColorDodge, b"ColorDodge"),
            (BlendMode::ColorBurn, b"ColorBurn"),
            (BlendMode::HardLight, b"HardLight"),
            (BlendMode::SoftLight, b"SoftLight"),
            (BlendMode::Difference, b"Difference"),
            (BlendMode::Exclusion, b"Exclusion"),
            (BlendMode::Hue, b"Hue"),
            (BlendMode::Saturation, b"Saturation"),
            (BlendMode::Color, b"Color"),
            (BlendMode::Luminosity, b"Luminosity"),
        ];

        let mut builder = DocumentBuilder::new();
        for (index, (mode, _)) in modes.iter().enumerate() {
            let resource = format!("BM{index}").into_bytes();
            assert!(builder.add_ext_gstate(
                &resource,
                &ExtGState {
                    blend_mode: Some(*mode),
                    ..ExtGState::default()
                }
            ));
        }
        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);

        for (index, (_, spelling)) in modes.iter().enumerate() {
            let name = format!("BM{index}").into_bytes();
            let state = resource(&doc, b"ExtGState", &name);
            assert_eq!(
                name_of(&doc, &state, b"BM").as_deref(),
                Some(*spelling),
                "mode {index}"
            );
        }
    }

    /// A soft mask names a transparency group and carries its kind and its
    /// backdrop; `/None` is a **different** statement from saying nothing.
    ///
    /// 11.6.5.2: an absent `/SMask` inherits whatever mask is in force and
    /// `/None` turns it off. A writer that treated them as one could not stop
    /// a mask at all.
    #[test]
    fn a_soft_mask_is_a_group_a_kind_and_a_backdrop_and_none_is_not_absence() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_form(
            b"Fm0",
            &FormXObject {
                bbox: [0.0, 0.0, 10.0, 10.0],
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Gray,
                    isolated: true,
                    knockout: false,
                }),
                content: b"1 g 0 0 10 10 re f",
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GMask",
            &ExtGState {
                soft_mask: Some(StateMask::Group {
                    kind: MaskKind::Luminosity,
                    form: b"Fm0",
                    backdrop: Some(&[0.0]),
                }),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GAlpha",
            &ExtGState {
                soft_mask: Some(StateMask::Group {
                    kind: MaskKind::Alpha,
                    form: b"Fm0",
                    backdrop: None,
                }),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GOff",
            &ExtGState {
                soft_mask: Some(StateMask::None),
                ..ExtGState::default()
            }
        ));
        assert!(builder.add_ext_gstate(
            b"GSilent",
            &ExtGState {
                fill_alpha: Some(1.0),
                ..ExtGState::default()
            }
        ));
        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);

        let mask = doc.resolve_key(
            &resource(&doc, b"ExtGState", b"GMask"),
            doc.intern(b"SMask"),
        );
        let mask = mask.as_dict().expect("a mask dictionary");
        assert_eq!(name_of(&doc, mask, b"Type").as_deref(), Some(&b"Mask"[..]));
        assert_eq!(
            name_of(&doc, mask, b"S").as_deref(),
            Some(&b"Luminosity"[..])
        );
        assert_eq!(numbers(&doc, mask, b"BC"), vec![0.0]);
        let group = doc.resolve_key(mask, doc.intern(b"G"));
        let group = group.as_dict().expect("the mask names a form");
        assert_eq!(
            name_of(&doc, group, b"Subtype").as_deref(),
            Some(&b"Form"[..]),
            "and it is the form, not a copy of the state"
        );

        let alpha = doc.resolve_key(
            &resource(&doc, b"ExtGState", b"GAlpha"),
            doc.intern(b"SMask"),
        );
        let alpha = alpha.as_dict().expect("a mask dictionary");
        assert_eq!(name_of(&doc, alpha, b"S").as_deref(), Some(&b"Alpha"[..]));
        assert!(
            !alpha.contains_key(doc.intern(b"BC")),
            "a mask given no backdrop states none"
        );

        let off = resource(&doc, b"ExtGState", b"GOff");
        assert_eq!(name_of(&doc, &off, b"SMask").as_deref(), Some(&b"None"[..]));

        let silent = resource(&doc, b"ExtGState", b"GSilent");
        assert!(
            !silent.contains_key(doc.intern(b"SMask")),
            "and a state that said nothing about the mask carries no key at all"
        );
    }

    /// Every way a graphics state can fail to be one, refused rather than
    /// written out.
    ///
    /// The control comes first for `a_compressed_image_that_does_not_describe
    /// _an_image_is_refused`'s reason: an `add_ext_gstate` that always returned
    /// false would satisfy every other line here.
    #[test]
    fn a_graphics_state_that_is_not_one_is_refused() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_form(
            b"Plain",
            &FormXObject {
                bbox: [0.0, 0.0, 10.0, 10.0],
                matrix: None,
                group: None,
                content: b"",
            }
        ));
        assert!(builder.add_form(
            b"Group",
            &FormXObject {
                bbox: [0.0, 0.0, 10.0, 10.0],
                matrix: None,
                group: Some(TransparencyGroup {
                    color_space: DeviceSpace::Rgb,
                    isolated: false,
                    knockout: false,
                }),
                content: b"",
            }
        ));
        assert!(
            builder.add_ext_gstate(
                b"Ok",
                &ExtGState {
                    fill_alpha: Some(0.0),
                    stroke_alpha: Some(1.0),
                    blend_mode: Some(BlendMode::Multiply),
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Luminosity,
                        form: b"Group",
                        backdrop: Some(&[0.0, 0.0, 0.0]),
                    }),
                }
            ),
            "the control is accepted"
        );

        let cases: Vec<(&str, ExtGState<'_>)> = vec![
            (
                "a fill alpha above one",
                ExtGState {
                    fill_alpha: Some(1.5),
                    ..ExtGState::default()
                },
            ),
            (
                "a fill alpha below zero",
                ExtGState {
                    fill_alpha: Some(-0.1),
                    ..ExtGState::default()
                },
            ),
            (
                "a fill alpha that is not a number",
                ExtGState {
                    fill_alpha: Some(f64::NAN),
                    ..ExtGState::default()
                },
            ),
            (
                "a stroke alpha above one",
                ExtGState {
                    stroke_alpha: Some(2.0),
                    ..ExtGState::default()
                },
            ),
            (
                // 11.6.5.2 requires `/G` to be a transparency group. A mask
                // over a plain form is one a reader cannot evaluate, and the
                // page it fails to mask looks like a page nobody masked.
                "a mask over a form that is not a group",
                ExtGState {
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Luminosity,
                        form: b"Plain",
                        backdrop: None,
                    }),
                    ..ExtGState::default()
                },
            ),
            (
                "a mask over a form nobody registered",
                ExtGState {
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Alpha,
                        form: b"Missing",
                        backdrop: None,
                    }),
                    ..ExtGState::default()
                },
            ),
            (
                "a backdrop with no components",
                ExtGState {
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Luminosity,
                        form: b"Group",
                        backdrop: Some(&[]),
                    }),
                    ..ExtGState::default()
                },
            ),
            (
                "a backdrop that is not numbers",
                ExtGState {
                    soft_mask: Some(StateMask::Group {
                        kind: MaskKind::Luminosity,
                        form: b"Group",
                        backdrop: Some(&[f64::INFINITY]),
                    }),
                    ..ExtGState::default()
                },
            ),
        ];

        for (what, state) in cases {
            assert!(
                !builder.add_ext_gstate(b"GS", &state),
                "{what} was accepted"
            );
        }

        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);
        let states = crate::pages::collect(&doc);
        let resources = states[0].resources.as_ref().expect("resources");
        let table = doc.resolve_key(resources, doc.intern(b"ExtGState"));
        assert_eq!(
            table.as_dict().map(Dict::len),
            Some(1),
            "one accepted state, and nothing a refusal left behind"
        );
    }

    // ---- transparency groups --------------------------------------------

    /// `/I` and `/K` are independent, and each is written only when it is true.
    ///
    /// 11.6.6 defaults both to false, so an explicit `/I false` says exactly
    /// what its absence says and would be a second spelling of one group. All
    /// four combinations are checked because isolation and knockout are
    /// different mechanisms — one decides the group's backdrop, the other
    /// decides each element's — and a writer that derived either from the
    /// other would pass a test that only ever set them together.
    #[test]
    fn isolated_and_knockout_are_independent_flags() {
        let mut builder = DocumentBuilder::new();
        for (index, (isolated, knockout)) in
            [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .enumerate()
        {
            let name = format!("Fm{index}").into_bytes();
            assert!(builder.add_form(
                &name,
                &FormXObject {
                    bbox: [0.0, 0.0, 10.0, 10.0],
                    matrix: None,
                    group: Some(TransparencyGroup {
                        color_space: DeviceSpace::Rgb,
                        isolated,
                        knockout,
                    }),
                    content: b"",
                }
            ));
        }
        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);

        for (index, (isolated, knockout)) in
            [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .enumerate()
        {
            let name = format!("Fm{index}").into_bytes();
            let form = resource(&doc, b"XObject", &name);
            let group = doc.resolve_key(&form, doc.intern(b"Group"));
            let group = group.as_dict().expect("a group dictionary");
            assert_eq!(
                name_of(&doc, group, b"S").as_deref(),
                Some(&b"Transparency"[..])
            );
            assert_eq!(
                name_of(&doc, group, b"CS").as_deref(),
                Some(&b"DeviceRGB"[..])
            );
            assert_eq!(
                doc.resolve_key(group, doc.intern(b"I")).as_bool(),
                isolated.then_some(true),
                "isolated on Fm{index}"
            );
            assert_eq!(
                doc.resolve_key(group, doc.intern(b"K")).as_bool(),
                knockout.then_some(true),
                "knockout on Fm{index}"
            );
        }
    }

    /// A form carries its box, its matrix, its content and the resources
    /// registered before it — and never itself.
    #[test]
    fn a_form_carries_its_geometry_its_content_and_the_resources_before_it() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        assert!(builder.add_form(
            b"Inner",
            &FormXObject {
                bbox: [0.0, 0.0, 4.0, 4.0],
                matrix: None,
                group: None,
                content: b"0 g 0 0 4 4 re f",
            }
        ));
        assert!(builder.add_form(
            b"Outer",
            &FormXObject {
                bbox: [1.0, 2.0, 9.0, 8.0],
                matrix: Some([2.0, 0.0, 0.0, 2.0, 5.0, 6.0]),
                group: None,
                content: b"/Inner Do",
            }
        ));
        builder.add_page(10.0, 10.0, |page| {
            assert!(page.form(b"Outer"));
        });
        let doc = opened(builder);

        let outer = resource(&doc, b"XObject", b"Outer");
        assert_eq!(
            name_of(&doc, &outer, b"Subtype").as_deref(),
            Some(&b"Form"[..])
        );
        assert_eq!(numbers(&doc, &outer, b"BBox"), vec![1.0, 2.0, 9.0, 8.0]);
        assert_eq!(
            numbers(&doc, &outer, b"Matrix"),
            vec![2.0, 0.0, 0.0, 2.0, 5.0, 6.0]
        );
        assert!(
            !outer.contains_key(doc.intern(b"Group")),
            "a form given no group is not a transparency group"
        );

        let resources = doc.resolve_key(&outer, doc.intern(b"Resources"));
        let resources = resources.as_dict().expect("the form has resources");
        let xobjects = doc.resolve_key(resources, doc.intern(b"XObject"));
        let xobjects = xobjects.as_dict().expect("and an /XObject table");
        assert!(
            xobjects.contains_key(doc.intern(b"Inner")),
            "it can name the form registered before it"
        );
        assert!(
            !xobjects.contains_key(doc.intern(b"Outer")),
            "and never itself, which is what makes a cycle unreachable"
        );
        let fonts = doc.resolve_key(resources, doc.intern(b"Font"));
        assert!(
            fonts
                .as_dict()
                .is_some_and(|d| d.contains_key(doc.intern(b"F0"))),
            "and the fonts registered before it"
        );

        assert!(content(&doc).contains("/Outer Do"));
    }

    /// A form that cannot be one is refused, and nothing it would have written
    /// reaches the file.
    #[test]
    fn a_form_that_cannot_be_one_is_refused_and_leaves_nothing_behind() {
        let mut builder = DocumentBuilder::new();
        for (what, form) in [
            (
                "a box with no width",
                FormXObject {
                    bbox: [4.0, 0.0, 4.0, 4.0],
                    matrix: None,
                    group: None,
                    content: b"the content of a form with no width",
                },
            ),
            (
                "a box with no height",
                FormXObject {
                    bbox: [0.0, 4.0, 4.0, 4.0],
                    matrix: None,
                    group: None,
                    content: b"the content of a form with no height",
                },
            ),
            (
                "a box back to front",
                FormXObject {
                    bbox: [9.0, 9.0, 1.0, 1.0],
                    matrix: None,
                    group: None,
                    content: b"the content of a form written backwards",
                },
            ),
            (
                "a box that is not numbers",
                FormXObject {
                    bbox: [0.0, 0.0, f64::NAN, 4.0],
                    matrix: None,
                    group: None,
                    content: b"the content of a form with no box",
                },
            ),
            (
                "a matrix that is not numbers",
                FormXObject {
                    bbox: [0.0, 0.0, 4.0, 4.0],
                    matrix: Some([1.0, 0.0, 0.0, 1.0, f64::INFINITY, 0.0]),
                    group: None,
                    content: b"the content of a form with no matrix",
                },
            ),
        ] {
            assert!(!builder.add_form(b"Fm", &form), "{what} was accepted");
        }
        builder.add_page(10.0, 10.0, |page| {
            assert!(!page.form(b"Fm"), "and no page can name what was refused");
        });

        let bytes = builder.finish();
        assert!(
            !bytes.windows(11).any(|w| w == b"the content"),
            "a refused form wrote a stream anyway"
        );
        assert!(
            !String::from_utf8_lossy(&bytes).contains("/Subtype /Form"),
            "and no form XObject exists at all"
        );
    }

    // ---- shadings and functions -----------------------------------------

    /// A type 2 shading and a type 3 shading are different objects with
    /// different geometry, and each carries the one it was given.
    ///
    /// Four coordinates against six is the whole difference in the dictionary,
    /// and a writer that shared a code path would have to drop the radii —
    /// producing an axial gradient where a radial one was asked for, which is
    /// a picture rather than an error.
    #[test]
    fn an_axial_shading_is_type_two_and_a_radial_is_type_three() {
        let ramp = Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![1.0, 0.0, 0.0],
            c1: vec![0.0, 0.0, 1.0],
            n: 1.0,
        };
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_shading(
            b"Sh0",
            &Shading::Axial {
                color_space: DeviceSpace::Rgb,
                coords: [0.0, 0.0, 60.0, 0.0],
                function: ramp.clone(),
                extend: (true, false),
            }
        ));
        assert!(builder.add_shading(
            b"Sh1",
            &Shading::Radial {
                color_space: DeviceSpace::Rgb,
                coords: [30.0, 30.0, 0.0, 30.0, 30.0, 25.0],
                function: ramp,
                extend: (false, true),
            }
        ));
        builder.add_page(60.0, 60.0, |page| {
            assert!(page.shading(b"Sh0"));
        });
        let doc = opened(builder);

        let axial = resource(&doc, b"Shading", b"Sh0");
        assert_eq!(
            doc.resolve_key(&axial, doc.intern(b"ShadingType")).as_int(),
            Some(2)
        );
        assert_eq!(numbers(&doc, &axial, b"Coords"), vec![0.0, 0.0, 60.0, 0.0]);
        assert_eq!(
            name_of(&doc, &axial, b"ColorSpace").as_deref(),
            Some(&b"DeviceRGB"[..])
        );

        let radial = resource(&doc, b"Shading", b"Sh1");
        assert_eq!(
            doc.resolve_key(&radial, doc.intern(b"ShadingType"))
                .as_int(),
            Some(3)
        );
        assert_eq!(
            numbers(&doc, &radial, b"Coords"),
            vec![30.0, 30.0, 0.0, 30.0, 30.0, 25.0],
            "both circles, radii included"
        );

        // `/Extend` is two independent booleans and the pair is asymmetric in
        // both fixtures, so a writer that wrote one twice is caught.
        let extend = |dict: &Dict| -> Vec<bool> {
            doc.resolve_key(dict, doc.intern(b"Extend"))
                .as_array()
                .map(|values| values.iter().filter_map(Object::as_bool).collect())
                .unwrap_or_default()
        };
        assert_eq!(extend(&axial), vec![true, false]);
        assert_eq!(extend(&radial), vec![false, true]);

        assert!(content(&doc).contains("/Sh0 sh"));
    }

    /// A type 3 function is a stitch **over** type 2 functions, and the stitch
    /// and the sub-functions are two independent things to get right.
    #[test]
    fn a_stitching_function_carries_its_stitch_and_its_sub_functions() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_shading(
            b"Sh0",
            &Shading::Axial {
                color_space: DeviceSpace::Rgb,
                coords: [0.0, 0.0, 60.0, 0.0],
                function: Function::Stitching {
                    domain: [0.0, 1.0],
                    functions: vec![
                        Function::Exponential {
                            domain: [0.0, 1.0],
                            c0: vec![1.0, 0.0, 0.0],
                            c1: vec![0.0, 1.0, 0.0],
                            n: 1.0,
                        },
                        Function::Exponential {
                            domain: [0.0, 1.0],
                            c0: vec![0.0, 1.0, 0.0],
                            c1: vec![0.0, 0.0, 1.0],
                            n: 2.0,
                        },
                    ],
                    bounds: vec![0.4],
                    encode: vec![[0.0, 1.0], [0.0, 1.0]],
                },
                extend: (false, false),
            }
        ));
        builder.add_page(60.0, 60.0, |_| {});
        let doc = opened(builder);

        let shading = resource(&doc, b"Shading", b"Sh0");
        let function = doc.resolve_key(&shading, doc.intern(b"Function"));
        let function = function.as_dict().expect("a function");
        assert_eq!(
            doc.resolve_key(function, doc.intern(b"FunctionType"))
                .as_int(),
            Some(3)
        );
        // The stitch: where each sub-function starts, and how its own domain is
        // reached once it does.
        assert_eq!(numbers(&doc, function, b"Domain"), vec![0.0, 1.0]);
        assert_eq!(numbers(&doc, function, b"Bounds"), vec![0.4]);
        assert_eq!(numbers(&doc, function, b"Encode"), vec![0.0, 1.0, 0.0, 1.0]);

        // The sub-functions: what colour comes out once it is reached. Read
        // from the array rather than assumed, because a stitch that named the
        // same sub-function twice would have a correct `/Bounds`.
        let subs = doc.resolve_key(function, doc.intern(b"Functions"));
        let subs = subs.as_array().expect("an array of functions").to_vec();
        assert_eq!(subs.len(), 2);
        let first = doc.resolve(&subs[0]);
        let first = first.as_dict().expect("a sub-function");
        let second = doc.resolve(&subs[1]);
        let second = second.as_dict().expect("a sub-function");
        for sub in [first, second] {
            assert_eq!(
                doc.resolve_key(sub, doc.intern(b"FunctionType")).as_int(),
                Some(2)
            );
        }
        assert_eq!(numbers(&doc, first, b"C0"), vec![1.0, 0.0, 0.0]);
        assert_eq!(numbers(&doc, first, b"C1"), vec![0.0, 1.0, 0.0]);
        assert_eq!(
            doc.resolve_key(first, doc.intern(b"N")).as_number(),
            Some(1.0)
        );
        assert_eq!(numbers(&doc, second, b"C0"), vec![0.0, 1.0, 0.0]);
        assert_eq!(numbers(&doc, second, b"C1"), vec![0.0, 0.0, 1.0]);
        assert_eq!(
            doc.resolve_key(second, doc.intern(b"N")).as_number(),
            Some(2.0),
            "each sub-function keeps its own exponent"
        );
    }

    /// Every way a shading or its function can fail to describe a gradient.
    #[test]
    fn a_shading_that_does_not_describe_a_gradient_is_refused() {
        let ramp = || Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![1.0, 0.0, 0.0],
            c1: vec![0.0, 0.0, 1.0],
            n: 1.0,
        };
        let mut builder = DocumentBuilder::new();
        assert!(
            builder.add_shading(
                b"Ok",
                &Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: ramp(),
                    extend: (false, false),
                }
            ),
            "the control is accepted"
        );

        let stitch = |bounds: Vec<f64>, encode: Vec<[f64; 2]>, count: usize| Function::Stitching {
            domain: [0.0, 1.0],
            functions: (0..count).map(|_| ramp()).collect(),
            bounds,
            encode,
        };
        let cases: Vec<(&str, Shading)> = vec![
            (
                "an axis of no length",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [5.0, 5.0, 5.0, 5.0],
                    function: ramp(),
                    extend: (false, false),
                },
            ),
            (
                "an axis that is not numbers",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, f64::NAN, 0.0],
                    function: ramp(),
                    extend: (false, false),
                },
            ),
            (
                "a negative radius",
                Shading::Radial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, -1.0, 0.0, 0.0, 5.0],
                    function: ramp(),
                    extend: (false, false),
                },
            ),
            (
                "two circles of no radius",
                Shading::Radial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 0.0, 9.0, 9.0, 0.0],
                    function: ramp(),
                    extend: (false, false),
                },
            ),
            (
                // 7.10.1: one output per component. Two numbers under
                // `/DeviceRGB` leaves a reader to invent the third.
                "a function that produces too few components",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![0.0, 0.0],
                        c1: vec![1.0, 1.0],
                        n: 1.0,
                    },
                    extend: (false, false),
                },
            ),
            (
                "a function whose ends disagree about how many components there are",
                Shading::Axial {
                    color_space: DeviceSpace::Gray,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![0.0],
                        c1: vec![1.0, 1.0],
                        n: 1.0,
                    },
                    extend: (false, false),
                },
            ),
            (
                "an exponent of zero",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![0.0, 0.0, 0.0],
                        c1: vec![1.0, 1.0, 1.0],
                        n: 0.0,
                    },
                    extend: (false, false),
                },
            ),
            (
                "a domain that does not increase",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: Function::Exponential {
                        domain: [1.0, 1.0],
                        c0: vec![0.0, 0.0, 0.0],
                        c1: vec![1.0, 1.0, 1.0],
                        n: 1.0,
                    },
                    extend: (false, false),
                },
            ),
            // The stitch, wrong in each of its own three ways. None of these
            // is visible in a test of the sub-functions and none of the
            // sub-function refusals above is visible in a test of the stitch.
            (
                "a stitch with one bound too few",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: stitch(vec![], vec![[0.0, 1.0], [0.0, 1.0]], 2),
                    extend: (false, false),
                },
            ),
            (
                "a stitch with one encode pair too few",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: stitch(vec![0.5], vec![[0.0, 1.0]], 2),
                    extend: (false, false),
                },
            ),
            (
                "a stitch whose bounds do not increase",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: stitch(vec![0.5, 0.5], vec![[0.0, 1.0], [0.0, 1.0], [0.0, 1.0]], 3),
                    extend: (false, false),
                },
            ),
            (
                "a stitch whose bound is outside its domain",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: stitch(vec![2.0], vec![[0.0, 1.0], [0.0, 1.0]], 2),
                    extend: (false, false),
                },
            ),
            (
                "a stitch over nothing",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: stitch(vec![], vec![], 0),
                    extend: (false, false),
                },
            ),
            (
                // A sub-function that is wrong while the stitch is right: the
                // other half of the pair above.
                "a stitch over a sub-function with the wrong component count",
                Shading::Axial {
                    color_space: DeviceSpace::Rgb,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: Function::Stitching {
                        domain: [0.0, 1.0],
                        functions: vec![
                            ramp(),
                            Function::Exponential {
                                domain: [0.0, 1.0],
                                c0: vec![0.0],
                                c1: vec![1.0],
                                n: 1.0,
                            },
                        ],
                        bounds: vec![0.5],
                        encode: vec![[0.0, 1.0], [0.0, 1.0]],
                    },
                    extend: (false, false),
                },
            ),
        ];

        for (what, shading) in cases {
            assert!(!builder.add_shading(b"Sh", &shading), "{what} was accepted");
        }

        builder.add_page(10.0, 10.0, |page| {
            assert!(
                !page.shading(b"Sh"),
                "and no page can name what was refused"
            );
        });
        let doc = opened(builder);
        let pages = crate::pages::collect(&doc);
        let resources = pages[0].resources.as_ref().expect("resources");
        let table = doc.resolve_key(resources, doc.intern(b"Shading"));
        assert_eq!(
            table.as_dict().map(Dict::len),
            Some(1),
            "one accepted shading and nothing else"
        );
    }

    /// A nest of stitching functions deeper than this repository's own reader
    /// will walk is refused rather than written.
    ///
    /// Not a resource bound — see [`MAX_FUNCTION_DEPTH`]. It is refused because
    /// a writer whose output its own reader declines is not a writer, and it is
    /// checked at a thousand deep as well as at nine to show the validator does
    /// not recurse.
    #[test]
    fn a_function_nested_deeper_than_the_reader_will_walk_is_refused() {
        let leaf = Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0],
            c1: vec![1.0],
            n: 1.0,
        };
        let wrap = |inner: Function| Function::Stitching {
            domain: [0.0, 1.0],
            functions: vec![inner],
            bounds: vec![],
            encode: vec![[0.0, 1.0]],
        };

        let mut builder = DocumentBuilder::new();
        let mut nest = leaf.clone();
        for _ in 0..MAX_FUNCTION_DEPTH {
            nest = wrap(nest);
        }
        assert!(
            builder.add_shading(
                b"Deep",
                &Shading::Axial {
                    color_space: DeviceSpace::Gray,
                    coords: [0.0, 0.0, 1.0, 0.0],
                    function: nest.clone(),
                    extend: (false, false),
                }
            ),
            "exactly as deep as the reader walks is accepted"
        );

        for extra in [1usize, 1000] {
            let mut deeper = leaf.clone();
            for _ in 0..(MAX_FUNCTION_DEPTH as usize + extra) {
                deeper = wrap(deeper);
            }
            assert!(
                !builder.add_shading(
                    b"Deeper",
                    &Shading::Axial {
                        color_space: DeviceSpace::Gray,
                        coords: [0.0, 0.0, 1.0, 0.0],
                        function: deeper,
                        extend: (false, false),
                    }
                ),
                "{extra} levels past the reader's own limit was accepted"
            );
        }
    }

    // ---- tiling patterns -------------------------------------------------

    /// A tiling pattern carries its cell, its two steps and its matrix, and its
    /// content is the stream.
    #[test]
    fn a_tiling_pattern_carries_its_cell_its_steps_and_its_matrix() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_tiling_pattern(
            b"P0",
            &TilingPattern {
                bbox: [0.0, 0.0, 8.0, 4.0],
                x_step: 10.0,
                y_step: 6.0,
                matrix: Some([1.0, 0.0, 0.0, 1.0, 3.0, 7.0]),
                tiling_type: TilingType::NoDistortion,
                content: b"0 0 1 rg 0 0 8 4 re f",
            }
        ));
        builder.add_page(40.0, 40.0, |page| {
            assert!(page.set_fill_pattern(b"P0"));
            page.raw(b"0 0 40 40 re f");
            assert!(page.set_stroke_pattern(b"P0"));
        });
        let doc = opened(builder);

        let pattern = resource(&doc, b"Pattern", b"P0");
        assert_eq!(
            doc.resolve_key(&pattern, doc.intern(b"PatternType"))
                .as_int(),
            Some(1)
        );
        assert_eq!(
            doc.resolve_key(&pattern, doc.intern(b"PaintType")).as_int(),
            Some(1)
        );
        assert_eq!(
            doc.resolve_key(&pattern, doc.intern(b"TilingType"))
                .as_int(),
            Some(2)
        );
        assert_eq!(numbers(&doc, &pattern, b"BBox"), vec![0.0, 0.0, 8.0, 4.0]);
        // The two steps differ from each other and from the cell, so a writer
        // that wrote one twice or derived either from the box is caught.
        assert_eq!(
            doc.resolve_key(&pattern, doc.intern(b"XStep")).as_number(),
            Some(10.0)
        );
        assert_eq!(
            doc.resolve_key(&pattern, doc.intern(b"YStep")).as_number(),
            Some(6.0)
        );
        assert_eq!(
            numbers(&doc, &pattern, b"Matrix"),
            vec![1.0, 0.0, 0.0, 1.0, 3.0, 7.0]
        );

        let text = content(&doc);
        assert!(text.contains("/Pattern cs /P0 scn"), "{text}");
        assert!(text.contains("/Pattern CS /P0 SCN"), "{text}");
    }

    /// Every way a tiling pattern can fail to tile.
    #[test]
    fn a_tiling_pattern_that_cannot_tile_is_refused() {
        let sound = || TilingPattern {
            bbox: [0.0, 0.0, 4.0, 4.0],
            x_step: 4.0,
            y_step: 4.0,
            matrix: None,
            tiling_type: TilingType::ConstantSpacing,
            content: b"the cell that must not be written",
        };
        let mut builder = DocumentBuilder::new();
        assert!(
            builder.add_tiling_pattern(b"Ok", &sound()),
            "the control is accepted"
        );

        for (what, pattern) in [
            (
                "a cell with no area",
                TilingPattern {
                    bbox: [0.0, 0.0, 0.0, 4.0],
                    ..sound()
                },
            ),
            (
                "a horizontal step of zero",
                TilingPattern {
                    x_step: 0.0,
                    ..sound()
                },
            ),
            (
                "a vertical step of zero",
                TilingPattern {
                    y_step: 0.0,
                    ..sound()
                },
            ),
            (
                "a step that is not a number",
                TilingPattern {
                    x_step: f64::NAN,
                    ..sound()
                },
            ),
            (
                "a matrix that is not numbers",
                TilingPattern {
                    matrix: Some([1.0, 0.0, 0.0, 1.0, 0.0, f64::NEG_INFINITY]),
                    ..sound()
                },
            ),
        ] {
            assert!(
                !builder.add_tiling_pattern(b"P", &pattern),
                "{what} was accepted"
            );
        }

        builder.add_page(10.0, 10.0, |page| {
            assert!(!page.set_fill_pattern(b"P"));
            assert!(!page.set_stroke_pattern(b"P"));
        });
        let bytes = builder.finish();
        assert_eq!(
            bytes.windows(8).filter(|w| *w == b"the cell").count(),
            1,
            "exactly the accepted pattern's cell is in the file"
        );
    }

    // ---- composite fonts -------------------------------------------------

    /// A composite font is `/Identity-H` over a CIDFontType2 with
    /// `/CIDToGIDMap /Identity`, which is the pair that makes an index
    /// addressable.
    ///
    /// Asserted against 9.7.4's published structure rather than against
    /// whatever this writer happens to emit: the Type0 font is the object a
    /// page names, the descendant is where the metrics and the descriptor
    /// live, and the encoding and the map are what turn a two-byte code into a
    /// glyph number.
    #[test]
    fn a_composite_font_is_identity_h_over_a_cid_font_with_an_identity_map() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(
                b"C0",
                12.0,
                5.0,
                20.0,
                &[Glyph { id: 2, text: "A" }, Glyph { id: 3, text: "B" }]
            ));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        assert_eq!(
            name_of(&doc, &font, b"Subtype").as_deref(),
            Some(&b"Type0"[..])
        );
        assert_eq!(
            name_of(&doc, &font, b"Encoding").as_deref(),
            Some(&b"Identity-H"[..]),
            "the code is the CID"
        );

        let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
        let descendants = descendants.as_array().expect("an array").to_vec();
        assert_eq!(descendants.len(), 1, "9.7.6: exactly one descendant");
        let cid = doc.resolve(&descendants[0]);
        let cid = cid.as_dict().expect("the descendant font");
        assert_eq!(
            name_of(&doc, cid, b"Subtype").as_deref(),
            Some(&b"CIDFontType2"[..])
        );
        assert_eq!(
            name_of(&doc, cid, b"CIDToGIDMap").as_deref(),
            Some(&b"Identity"[..]),
            "and the CID is the glyph index"
        );
        assert_eq!(
            doc.resolve_key(cid, doc.intern(b"DW")).as_int(),
            Some(1000),
            "9.7.4.3's default, stated rather than left to be known"
        );

        // 9.7.3: an `/Identity-H` encoding over a descendant claiming some
        // other ordering is a font whose two halves disagree.
        let system = doc.resolve_key(cid, doc.intern(b"CIDSystemInfo"));
        let system = system.as_dict().expect("a CIDSystemInfo");
        let string = |key: &[u8]| -> Option<Vec<u8>> {
            doc.resolve_key(system, doc.intern(key))
                .as_string()
                .map(|s| s.bytes.to_vec())
        };
        assert_eq!(string(b"Registry").as_deref(), Some(&b"Adobe"[..]));
        assert_eq!(string(b"Ordering").as_deref(), Some(&b"Identity"[..]));
        assert_eq!(
            doc.resolve_key(system, doc.intern(b"Supplement")).as_int(),
            Some(0)
        );

        // The descriptor hangs off the descendant, not off the Type0 font, and
        // is symbolic: there is no `/Encoding` a code could be looked up in.
        let descriptor = doc.resolve_key(cid, doc.intern(b"FontDescriptor"));
        let descriptor = descriptor.as_dict().expect("a descriptor");
        assert_eq!(
            doc.resolve_key(descriptor, doc.intern(b"Flags")).as_int(),
            Some(4)
        );
        assert!(
            descriptor.get_ref(doc.intern(b"FontFile2")).is_some(),
            "and the program is embedded"
        );
        assert!(
            !font.contains_key(doc.intern(b"FontDescriptor")),
            "the Type0 font carries no descriptor of its own"
        );

        // Two bytes a glyph, big-endian, as a hex string.
        assert!(
            content(&doc).contains("<00020003> Tj"),
            "the string is the glyph indices: {}",
            content(&doc)
        );
    }

    /// `/W` is the font's own `hmtx`, scaled into PDF's thousandths, run
    /// together where the glyph ids are consecutive.
    ///
    /// The units per em is 2048, so every number here is wrong by a factor of
    /// two if the scaling was skipped — which is the shape of defect that
    /// renders at the right glyphs and the wrong positions, and which nothing
    /// that only checks "the text drew" can see.
    #[test]
    fn the_width_array_is_the_fonts_own_hmtx_scaled_and_run_together() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(2048, true)));
        builder.add_page(50.0, 50.0, |page| {
            // 1, 2 and 3 are consecutive; leaving 0 undrawn is what makes the
            // first run start at 1 rather than at zero.
            assert!(page.glyphs(
                b"C0",
                12.0,
                5.0,
                20.0,
                &[
                    Glyph { id: 1, text: "" },
                    Glyph { id: 2, text: "A" },
                    Glyph { id: 3, text: "B" },
                ]
            ));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
        let descendants = descendants.as_array().expect("an array").to_vec();
        let cid = doc.resolve(&descendants[0]);
        let cid = cid.as_dict().expect("the descendant font");

        let widths = doc.resolve_key(cid, doc.intern(b"W"));
        let widths = widths.as_array().expect("a /W array").to_vec();
        assert_eq!(widths.len(), 2, "one run: a first CID and its widths");
        assert_eq!(doc.resolve(&widths[0]).as_int(), Some(1));
        let run: Vec<f64> = doc
            .resolve(&widths[1])
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|o| doc.resolve(o).as_number())
                    .collect()
            })
            .unwrap_or_default();
        // 700, 800 and 900 font units at 2048 per em.
        assert_eq!(run, vec![342.0, 391.0, 439.0]);
    }

    /// Glyphs that are not consecutive are separate runs, which is the other
    /// half of Table 116's `c [w...]` form.
    #[test]
    fn a_gap_in_the_glyphs_drawn_starts_a_new_width_run() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(
                b"C0",
                12.0,
                5.0,
                20.0,
                &[Glyph { id: 1, text: "a" }, Glyph { id: 3, text: "b" }]
            ));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
        let descendants = descendants.as_array().expect("an array").to_vec();
        let cid = doc.resolve(&descendants[0]);
        let cid = cid.as_dict().expect("the descendant font");
        let widths = doc.resolve_key(cid, doc.intern(b"W"));
        let widths = widths.as_array().expect("a /W array").to_vec();

        assert_eq!(widths.len(), 4, "two runs: {widths:?}");
        assert_eq!(doc.resolve(&widths[0]).as_int(), Some(1));
        assert_eq!(doc.resolve(&widths[2]).as_int(), Some(3));
        let run = |index: usize| -> Vec<f64> {
            doc.resolve(&widths[index])
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|o| doc.resolve(o).as_number())
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(run(1), vec![700.0]);
        assert_eq!(run(3), vec![900.0]);
    }

    /// A font that states no advances gets no `/W` at all, and `/DW` says what
    /// happens instead.
    ///
    /// The alternative is a width array of invented numbers presented as the
    /// font's, which is what the simple-font path has to do because 9.6.6.4
    /// leaves it no choice and what a composite font is not forced into.
    #[test]
    fn a_font_that_states_no_advances_gets_no_width_array() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Bare", &metric_font(1000, false)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(b"C0", 12.0, 5.0, 20.0, &[Glyph { id: 2, text: "A" }]));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let descendants = doc.resolve_key(&font, doc.intern(b"DescendantFonts"));
        let descendants = descendants.as_array().expect("an array").to_vec();
        let cid = doc.resolve(&descendants[0]);
        let cid = cid.as_dict().expect("the descendant font");
        assert!(
            !cid.contains_key(doc.intern(b"W")),
            "no widths were stated, so none are claimed"
        );
        assert_eq!(doc.resolve_key(cid, doc.intern(b"DW")).as_int(), Some(1000));
    }

    /// One glyph drawn twice with different text maps to the **first** text,
    /// and the choice is a choice rather than an accident.
    ///
    /// `/ToUnicode` maps a code to one string, so a glyph a caller draws once
    /// as "fi" and later as "fl" has to lose one of them -- there is no
    /// spelling of the CMap that says "either". What must not happen is that
    /// *which* one survives depends on the order the page happened to draw
    /// them in, because then the same document built from the same glyphs in a
    /// different order extracts as different text, and the thirteenth
    /// determinism fingerprint is a hash of exactly that.
    ///
    /// This test exists because the injection matrix said it had to. Reversing
    /// the rule to last-wins was caught by nothing that asserts anything about
    /// `/ToUnicode`: the only test that failed was the kitchen-sink form
    /// fixture, and it failed on `the form has resources`, three constructs
    /// away from the mapping. That is a survivor wearing a passing suite --
    /// milestone 2's duplicate-attribute pair, milestone 3's case fold and
    /// milestone 4's cache halves, arrived at from a fourth direction.
    #[test]
    fn a_glyph_drawn_twice_with_different_text_keeps_the_first() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(
                b"C0",
                12.0,
                5.0,
                20.0,
                &[
                    Glyph { id: 7, text: "fi" },
                    Glyph { id: 8, text: "x" },
                    // The same glyph again, standing for something else.
                    Glyph { id: 7, text: "fl" },
                ]
            ));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let stream = font
            .get_ref(doc.intern(b"ToUnicode"))
            .expect("a /ToUnicode stream");
        let cmap = doc.stream_decoded(stream).expect("it decodes");
        let text = String::from_utf8_lossy(&cmap).into_owned();

        assert!(
            text.contains("<0007> <00660069>"),
            "glyph 7 keeps the first text it was drawn with: {text}"
        );
        assert!(
            !text.contains("<0007> <0066006c>") && !text.contains("<0007> <0066006C>"),
            "and does not take the last: {text}"
        );
        // Two codes, not three: the repeat is the same glyph.
        assert!(text.contains("2 beginbfchar"), "{text}");
    }

    /// `/ToUnicode` takes many glyphs to one character **and** one glyph to
    /// several, which is the mapping a simple font's `/Encoding` cannot hold.
    #[test]
    fn to_unicode_maps_many_glyphs_to_one_character_and_one_glyph_to_many() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(
                b"C0",
                12.0,
                5.0,
                20.0,
                &[
                    // Two glyphs, one character: an alternate or a positional
                    // form, which is ordinary in every script that has one.
                    Glyph { id: 1, text: "f" },
                    Glyph { id: 2, text: "f" },
                    // One glyph, three characters: a ligature.
                    Glyph { id: 3, text: "ffi" },
                ]
            ));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let stream = font
            .get_ref(doc.intern(b"ToUnicode"))
            .expect("a /ToUnicode stream");
        let cmap = doc.stream_decoded(stream).expect("it decodes");
        let text = String::from_utf8_lossy(&cmap).into_owned();

        assert!(text.contains("/CMapType 2"), "{text}");
        assert!(
            text.contains("<0000> <FFFF>"),
            "the codespace is two bytes wide: {text}"
        );
        assert!(text.contains("3 beginbfchar"), "{text}");
        assert!(text.contains("<0001> <0066>"), "glyph 1 is f: {text}");
        assert!(
            text.contains("<0002> <0066>"),
            "and so is glyph 2, which is the many-to-one half: {text}"
        );
        assert!(
            text.contains("<0003> <006600660069>"),
            "glyph 3 is the three characters of a ligature: {text}"
        );
    }

    /// A glyph that stands for nothing gets no entry, and a font whose glyphs
    /// all stand for nothing gets no `/ToUnicode` at all.
    ///
    /// 9.10.3's grammar has no zero-entry `beginbfchar`, so an empty CMap is
    /// not merely useless — a reader that finds one stops looking for another
    /// answer, which is worse than finding none.
    #[test]
    fn a_font_whose_glyphs_stand_for_nothing_carries_no_to_unicode() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        assert!(builder.add_cid_font(b"C1", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(b"C0", 12.0, 5.0, 20.0, &[Glyph { id: 2, text: "" }]));
            assert!(page.glyphs(
                b"C1",
                12.0,
                5.0,
                8.0,
                &[Glyph { id: 1, text: "" }, Glyph { id: 2, text: "A" }]
            ));
        });
        let doc = opened(builder);

        let silent = resource(&doc, b"Font", b"C0");
        assert!(
            !silent.contains_key(doc.intern(b"ToUnicode")),
            "nothing to say, so nothing said"
        );

        let partial = resource(&doc, b"Font", b"C1");
        let stream = partial
            .get_ref(doc.intern(b"ToUnicode"))
            .expect("a /ToUnicode stream");
        let text =
            String::from_utf8_lossy(&doc.stream_decoded(stream).expect("it decodes")).into_owned();
        assert!(text.contains("1 beginbfchar"), "one entry, not two: {text}");
        assert!(text.contains("<0002> <0041>"), "{text}");
        assert!(!text.contains("<0001>"), "{text}");
    }

    /// The first non-empty text a glyph is drawn with stands, however many
    /// times and on however many pages it is drawn afterwards.
    ///
    /// `/ToUnicode` is one function from code to text, so a glyph drawn twice
    /// with two meanings has to resolve to one of them — and the answer has to
    /// be the same every time the document is written (ruling 4).
    #[test]
    fn the_first_non_empty_text_a_glyph_stands_for_wins() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(b"C0", 12.0, 5.0, 20.0, &[Glyph { id: 2, text: "" }]));
            assert!(page.glyphs(b"C0", 12.0, 5.0, 8.0, &[Glyph { id: 2, text: "A" }]));
        });
        builder.add_page(50.0, 50.0, |page| {
            assert!(page.glyphs(b"C0", 12.0, 5.0, 20.0, &[Glyph { id: 2, text: "Z" }]));
        });
        let doc = opened(builder);

        let font = resource(&doc, b"Font", b"C0");
        let stream = font
            .get_ref(doc.intern(b"ToUnicode"))
            .expect("a /ToUnicode stream");
        let text =
            String::from_utf8_lossy(&doc.stream_decoded(stream).expect("it decodes")).into_owned();
        assert!(
            text.contains("<0002> <0041>") && !text.contains("<005A>"),
            "the empty text does not win and the later one does not either: {text}"
        );
    }

    /// Naming a resource the page does not carry writes **nothing**, rather
    /// than an operator pointing at a name the `/Resources` has no entry for.
    #[test]
    fn naming_a_resource_the_page_does_not_carry_writes_nothing() {
        let mut builder = DocumentBuilder::new();
        builder.add_base_font(b"F0", b"Helvetica");
        builder.add_page(10.0, 10.0, |page| {
            assert!(!page.set_ext_gstate(b"GS0"));
            assert!(!page.form(b"Fm0"));
            assert!(!page.shading(b"Sh0"));
            assert!(!page.set_fill_pattern(b"P0"));
            assert!(!page.set_stroke_pattern(b"P0"));
            // A simple font is not a composite one, and writing two-byte codes
            // into it would address the wrong glyphs at the wrong widths and
            // read as a font problem rather than an encoding one.
            assert!(!page.glyphs(b"F0", 12.0, 1.0, 1.0, &[Glyph { id: 2, text: "A" }]));
            assert!(!page.glyphs(b"C0", 12.0, 1.0, 1.0, &[Glyph { id: 2, text: "A" }]));
        });
        let doc = opened(builder);

        let text = content(&doc);
        assert!(text.trim().is_empty(), "nothing was written: {text:?}");
    }

    /// A page carries only the resource kinds it was given.
    ///
    /// 7.8.3 makes every sub-dictionary optional, and an empty `/Shading`
    /// beside an empty `/Pattern` on every page of a document that uses
    /// neither is noise a reader has to walk.
    #[test]
    fn a_page_carries_only_the_resource_kinds_it_was_given() {
        let mut builder = DocumentBuilder::new();
        builder.add_page(10.0, 10.0, |_| {});
        let doc = opened(builder);
        let pages = crate::pages::collect(&doc);
        let resources = pages[0].resources.as_ref().expect("a resources dictionary");
        for key in [
            &b"Font"[..],
            b"XObject",
            b"ExtGState",
            b"Shading",
            b"Pattern",
        ] {
            assert!(
                !resources.contains_key(doc.intern(key)),
                "/{} on a page that named none",
                String::from_utf8_lossy(key)
            );
        }
    }

    /// Nothing this milestone writes declares a `/Filter`, so `maybe_compress`
    /// still owns every one of them.
    ///
    /// That is the contract `ImageData::Compressed` depends on, stated from the
    /// other side: the rule is "a `/Filter` key means hands off", and a new
    /// stream kind that declared one to keep its bytes would be a second rule.
    /// A form, a pattern cell and a `/ToUnicode` CMap are all ordinary streams
    /// and all compress when the writer is asked to compress.
    #[test]
    fn the_new_streams_declare_no_filter_and_the_writer_compresses_them() {
        let mut builder = DocumentBuilder::new();
        assert!(builder.add_cid_font(b"C0", b"Metric", &metric_font(1000, true)));
        assert!(builder.add_form(
            b"Fm0",
            &FormXObject {
                bbox: [0.0, 0.0, 10.0, 10.0],
                matrix: None,
                group: None,
                content: &vec![b' '; 4096],
            }
        ));
        assert!(builder.add_tiling_pattern(
            b"P0",
            &TilingPattern {
                bbox: [0.0, 0.0, 4.0, 4.0],
                x_step: 4.0,
                y_step: 4.0,
                matrix: None,
                tiling_type: TilingType::ConstantSpacing,
                content: &vec![b' '; 4096],
            }
        ));
        builder.add_page(10.0, 10.0, |page| {
            assert!(page.form(b"Fm0"));
            assert!(page.glyphs(b"C0", 12.0, 1.0, 1.0, &[Glyph { id: 2, text: "A" }]));
        });

        let plain = CosDocument::open(builder.finish()).expect("it opens");
        for (kind, name) in [(&b"XObject"[..], &b"Fm0"[..]), (b"Pattern", b"P0")] {
            let dict = resource(&plain, kind, name);
            assert!(
                !dict.contains_key(Name::FILTER),
                "{} declares a filter of its own",
                String::from_utf8_lossy(name)
            );
        }
        let font = resource(&plain, b"Font", b"C0");
        let stream = font
            .get_ref(plain.intern(b"ToUnicode"))
            .expect("a /ToUnicode stream");
        let cmap = plain
            .get(stream)
            .ok()
            .and_then(|o| o.as_dict().cloned())
            .expect("a stream dictionary");
        assert!(!cmap.contains_key(Name::FILTER));

        // And with compression asked for, the same document's streams come out
        // deflated -- which they could not if any of them had claimed a filter.
        let editor = crate::DocumentEditor::new(std::sync::Arc::new(plain));
        let packed = editor.save(&crate::WriteOptions {
            mode: crate::WriteMode::Rewrite,
            compress: true,
            ..crate::WriteOptions::default()
        });
        let reopened = CosDocument::open(packed).expect("it reopens");
        let form = resource(&reopened, b"XObject", b"Fm0");
        assert!(
            form.contains_key(Name::FILTER),
            "the writer compressed the form's content stream"
        );
    }
}
