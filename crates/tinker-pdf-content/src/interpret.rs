//! The content-stream interpreter (8.2, 9.4).
//!
//! Operands accumulate on a stack until an operator consumes them. An operator
//! that arrives with the wrong operand count is skipped and the stack cleared,
//! which is what keeps one malformed instruction from desynchronizing the rest
//! of the page.

use crate::device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
use crate::record::{Answers, Event, RecordingDevice};
use crate::replay::replay;
use crate::state::{
    BlackPointCompensation, BlendMode, ColorFamily, ColorSource, ColorantTints, GraphicsState,
    Halftone, LineCap, LineJoin, Matrix, RenderingIntent, Rgb, Rgb16, TextRenderMode, Transfer,
};
use crate::tokenizer::{Token, Tokenizer};
use core::mem;
use std::collections::BTreeSet;
use std::sync::Arc;
use tinker_pdf_color::{ColorSpace, Function};
use tinker_pdf_cos::decode_text_string;
use tinker_pdf_cos::{Counter, Work};

/// Everything the interpreter needs from a page's resources.
///
/// This is the seam that keeps the crate free of PDF dictionaries: the
/// interpreter knows *which* font or colour space a stream selected, and the
/// caller says what that name means. Every method has a default, so an
/// implementation supplies only what it has.
/// A form XObject the interpreter is about to run (8.10).
///
/// The bounding box travels with the content rather than being fetched
/// separately, and `form` returns this struct rather than a tuple, so that
/// adding it forced every implementor to supply it. A defaulted accessor
/// would have compiled everywhere and quietly clipped nothing — which is
/// exactly how `GlyphSource::image` once stopped drawing every image in every
/// document with the whole suite green.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    /// The decoded content stream.
    pub content: Vec<u8>,
    /// `/Matrix`, mapping form space into the space that invoked it.
    pub matrix: Matrix,
    /// `/BBox` in form space, as `[x0, y0, x1, y1]`.
    ///
    /// 8.10.2 makes it required and says the form is clipped to it. `None`
    /// means the file omitted it, in which case there is nothing to clip to
    /// and the form is drawn unclipped rather than dropped.
    pub bbox: Option<[f64; 4]>,
    /// `/Group` with `/S /Transparency` (11.6.6), when the form is one.
    pub group: Option<Group>,
    /// The form's own indirect reference, packed as `num << 16 | gen`, and
    /// `0` when the caller reached the stream without one.
    ///
    /// 14.7.4.2 numbers marked-content sequences **within a content stream**,
    /// so an `/MCID` only identifies a sequence beside the stream it was
    /// written in: two forms on one page may each write `/MCID 0`, and a
    /// structure join keyed on the identifier alone hands both sequences to
    /// both elements. This is the other half of that key, and it travels with
    /// the content for the same reason [`Form::bbox`] does — adding it as a
    /// field forced every implementor to supply it rather than inherit a
    /// defaulted accessor that would have compiled everywhere and silently
    /// gone on collapsing the pair.
    ///
    /// A plain integer, not a COS reference: ruling 8 keeps object types out
    /// of this crate, and an identity is all a device needs. Packing rather
    /// than the object number alone because 7.3.10 makes `num gen` the
    /// reference, and a regenerated object reuses the number.
    pub stream: u64,
}

/// A Type 3 glyph's procedure, which the interpreter is about to run (9.6.5).
///
/// A struct rather than a tuple for [`Form`]'s reason: adding
/// [`Type3Glyph::stream`] forced every implementor to supply it.
#[derive(Clone, Debug, PartialEq)]
pub struct Type3Glyph {
    /// The decoded procedure, a content stream.
    pub content: Vec<u8>,
    /// The font's `/FontMatrix`, mapping glyph space into text space.
    pub matrix: Matrix,
    /// The procedure's own indirect reference, packed as [`Form::stream`]
    /// packs a form's, and `0` when the caller reached it without one.
    ///
    /// It is what tells a procedure that shows its own glyph — which would
    /// run itself for ever, and is run once — from one that shows another
    /// glyph of its font.
    pub stream: u64,
}

/// A transparency group XObject's attributes (11.6.6).
///
/// 11.6.6 lets `/CS` name the space the group's contents are composited in,
/// and [`Group::space`] below carries it: grey, RGB and CMYK groups each
/// composite in their own, which is what `CmykA8` exists for. `/Lab` is the
/// one that does not, and it is reported by name rather than silently
/// composited in RGB.
///
/// This comment used to say the colour space was *deliberately absent* and
/// that the engine composited in RGB throughout. That stopped being true when
/// the field three lines below was added, and it stayed here — which is the
/// direction of documentation drift that is hardest to catch, because a reader
/// checking the claim against the struct sees the contradiction and a reader
/// checking the struct against the claim does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Group {
    /// `/I`: the group composites against a transparent backdrop rather than
    /// against what is already on the page (11.4.4).
    pub isolated: bool,
    /// `/K`: each element composites against the group's *initial* backdrop
    /// rather than against the elements before it (11.4.5).
    pub knockout: bool,
    /// `/CS`: the space the group's contents are composited in (11.6.6).
    ///
    /// `None` where the group declares no space, which 11.6.6 permits — the
    /// group then inherits the space it is composited into.
    pub space: Option<GroupSpace>,
}

/// Which of 11.6.6's blending spaces a group's `/CS` names.
///
/// The *shape* of the space rather than the space itself, and deliberately so.
/// A blend formula in 11.3.5 acts on component values, so what a compositor
/// needs from `/CS` is how many components there are and whether they are
/// subtractive — not the palette, the tint transform or the profile that
/// decides what those components *mean*. Keeping it to that also keeps
/// [`Group`] `Copy`, which every save and restore of the graphics state relies
/// on.
///
/// Exact conversion between these — which is what makes a CMYK group blend
/// like one rather than merely be named as one — is `docs/design/icc.md`'s
/// stage 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSpace {
    /// One component, additive.
    Gray,
    /// Three components, additive.
    Rgb,
    /// Four components, subtractive.
    Cmyk,
    /// CIE L*a*b*, whose components are not in `0..1` at all.
    Lab,
}

impl GroupSpace {
    /// Whether compositing this group in RGB is the same arithmetic as
    /// compositing it in its own space.
    ///
    /// True for grey and RGB, and the grey case is worth stating rather than
    /// assuming: a separable blend applied per channel to `R = G = B` produces
    /// `R' = G' = B'` equal to the same blend applied to the single grey
    /// channel, because each channel's formula is the same function of the
    /// same two numbers. Subtractive components are a different formula, and
    /// `/Lab`'s are not even in the unit interval.
    #[must_use]
    pub fn blends_as_rgb(self) -> bool {
        matches!(self, GroupSpace::Gray | GroupSpace::Rgb)
    }

    /// What to call it in a warning.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            GroupSpace::Gray => "DeviceGray",
            GroupSpace::Rgb => "DeviceRGB",
            GroupSpace::Cmyk => "DeviceCMYK",
            GroupSpace::Lab => "Lab",
        }
    }
}

/// An ExtGState `/SMask` (11.6.5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum SoftMask {
    /// `/None`: whatever mask was in force stops applying.
    None,
    /// A group to render and read back as a mask.
    Group(Box<MaskGroup>),
}

/// What one graphics state parameter dictionary sets (8.4.5, Table 58, and
/// ISO 32000-2's `/UseBlackPtComp`): one field per parameter, `None` where the
/// dictionary leaves it as it was.
///
/// One field per **parameter**, not per key, so Table 58's rules between keys
/// are applied once, where the dictionary is read: `/op` absent takes `/OP`'s
/// value. A value of a type Table 58 does not give is absent, and leaves the
/// parameter alone, as an operator leaves it on an operand it cannot read.
///
/// The line parameters are the numbers written, which the interpreter checks
/// as their operators check them: `/LW -1` changes nothing, as `-1 w` does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateParameters {
    /// `/LW`, as `w` takes it.
    pub line_width: Option<f64>,
    /// `/LC`, as `J` takes it.
    pub line_cap: Option<f64>,
    /// `/LJ`, as `j` takes it.
    pub line_join: Option<f64>,
    /// `/ML`, as `M` takes it.
    pub miter_limit: Option<f64>,
    /// `/D`: the dash array and phase, as `d` takes them.
    pub dash: Option<(Vec<f64>, f64)>,
    /// `/Font`: the text font's name and size, as `Tf` takes them. The name is
    /// the one the source gives the font the dictionary names by reference,
    /// which is the scope's own name for it where the scope has one.
    pub font: Option<(Vec<u8>, f64)>,
    /// `/RI`.
    pub rendering_intent: Option<RenderingIntent>,
    /// `/UseBlackPtComp`.
    pub black_point_compensation: Option<BlackPointCompensation>,
    /// `/OP`.
    pub stroke_overprint: Option<bool>,
    /// `/op`, or `/OP` where `/op` is absent.
    pub fill_overprint: Option<bool>,
    /// `/OPM`.
    pub overprint_mode: Option<i64>,
    /// `/FL`.
    pub flatness: Option<f64>,
    /// `/SM`.
    pub smoothness: Option<f64>,
    /// `/SA`.
    pub stroke_adjustment: Option<bool>,
    /// `/AIS`.
    pub alpha_is_shape: Option<bool>,
    /// `/TK`.
    pub text_knockout: Option<bool>,
    /// `/BG2` over `/BG`; `Some(None)` is `/BG2 /Default`.
    pub black_generation: Option<Option<Arc<Function>>>,
    /// `/UCR2` over `/UCR`; `Some(None)` is `/UCR2 /Default`.
    pub undercolor_removal: Option<Option<Arc<Function>>>,
    /// `/TR2` over `/TR`; `Some(None)` is `/TR2 /Default`.
    pub transfer: Option<Option<Transfer>>,
    /// `/HT`; `Some(None)` is `/HT /Default`.
    pub halftone: Option<Option<Arc<Halftone>>>,
    /// `/HTO`.
    pub halftone_origin: Option<[f64; 2]>,
    /// `/CA`, the stroking alpha.
    pub stroke_alpha: Option<f64>,
    /// `/ca`, the non-stroking alpha.
    pub fill_alpha: Option<f64>,
    /// `/BM` (11.3.5).
    pub blend: Option<BlendMode>,
    /// `/SMask` (11.6.5.1). `None` leaves the mask in force alone, which is
    /// not `/None`: that removes it.
    pub soft_mask: Option<SoftMask>,
}

/// Whichever resource dictionary is in force: the caller's, or a form's own.
///
/// A borrow while the page's own resources are in force, which is every
/// content stream and most of every other one; owned only while a form that
/// brought its own is running.
enum Scope<'d, F> {
    /// The caller's, borrowed for the whole run.
    Borrowed(&'d F),
    /// A form's own, shared with whatever cached it.
    Owned(Arc<F>),
}

impl<F> core::ops::Deref for Scope<'_, F> {
    type Target = F;

    fn deref(&self) -> &F {
        match self {
            Scope::Borrowed(fonts) => fonts,
            Scope::Owned(fonts) => fonts,
        }
    }
}

/// The `/SMask` dictionary's group, resolved (11.6.5.2).
#[derive(Clone, Debug, PartialEq)]
pub struct MaskGroup {
    /// `/G`, the transparency group XObject that draws the mask.
    pub form: Form,
    /// `/S`: true for `/Luminosity`, false for `/Alpha`.
    pub luminosity: bool,
    /// `/BC`, resolved through the group's own colour space.
    ///
    /// **`None` means black, which means fully masked.** 11.6.5.2 composites
    /// the mask group against a fully opaque backdrop of this colour and
    /// takes the luminosity of the result, and the default is the black of
    /// the group's space. Defaulting to white or to transparent inverts every
    /// drop shadow in a corpus and still looks reasonable on a light page —
    /// the shadow appears where the light should be, which reads as a design
    /// choice rather than as a bug. Only meaningful for `/Luminosity`.
    pub backdrop: Option<Rgb>,
    /// `/TR`, pre-sampled to 256 entries; `None` is `/Identity`.
    ///
    /// Sampled rather than carried as a function because it is applied per
    /// pixel, and a function on a pixel path is both ruinously slow and a
    /// route back to the platform's `libm` (ruling 4).
    pub transfer: Option<[u8; 256]>,
}

/// An optional-content layer, as the resource seam resolved it (8.11).
///
/// The interpreter cannot decide this: `/OCProperties` lives in the catalog
/// and the groups are indirect objects, neither of which this crate can see.
/// So it asks, and gets back the two things it has any use for — whether to
/// paint, and what to call the layer if it does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// Whether the document's default configuration paints this layer.
    pub visible: bool,
    /// What to call it: the group's `/Name`, which is the string a viewer's
    /// layer panel shows, falling back to the resource name.
    ///
    /// Ruling 10 — a warning that cannot say *which* layer it hid is not
    /// actionable, and a hidden layer is exactly the leniency a reader cannot
    /// see for themselves.
    pub label: String,
}

/// Which device colour spaces the resource dictionary in force remaps to a
/// default colour space (8.6.5.6): its `/ColorSpace` subdictionary's
/// `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK`, each standing in for the
/// device space it is named for.
///
/// Asked once a scope rather than once an operator — when [`interpret`]
/// starts, and when a form brings resources of its own — so that `g`, `rg` and
/// `k`, the most frequent colour operators there are, go on converting in
/// place in a scope that names no default, and ask the seam only for a family
/// the scope does remap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceDefaults {
    /// `/DefaultGray` stands in for DeviceGray.
    pub gray: bool,
    /// `/DefaultRGB` stands in for DeviceRGB.
    pub rgb: bool,
    /// `/DefaultCMYK` stands in for DeviceCMYK.
    pub cmyk: bool,
}

impl DeviceDefaults {
    /// No device space remapped, which is every scope that names no default.
    pub const NONE: DeviceDefaults = DeviceDefaults {
        gray: false,
        rgb: false,
        cmyk: false,
    };

    /// The device space `count` components are read in when no space is
    /// named — one is DeviceGray, four is DeviceCMYK and any other count is
    /// DeviceRGB, as [`components_to_rgb`] reads them — when this scope
    /// remaps it, and `None` when it does not.
    fn remapped_by_count(self, count: usize) -> Option<&'static [u8]> {
        let (remapped, space): (bool, &'static [u8]) = match count {
            1 => (self.gray, b"DeviceGray"),
            4 => (self.cmyk, b"DeviceCMYK"),
            _ => (self.rgb, b"DeviceRGB"),
        };
        remapped.then_some(space)
    }
}

/// A colour as the seam resolves it: its light, as bytes and at sixteen
/// bits, its DeviceCMYK components where it has them, what it was resolved
/// from, and what it paints by colorant where the device asked.
type Resolved = (
    Rgb,
    Rgb16,
    Option<[u8; 4]>,
    Option<Arc<ColorSource>>,
    Option<Arc<ColorantTints>>,
);

pub trait FontSource {
    /// Splits a string into `(code, text, advance in 1/1000 em)`.
    fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)>;

    /// Whether the named font writes vertically.
    fn is_vertical(&self, font: &[u8]) -> bool {
        let _ = font;
        false
    }

    /// The vertical metrics of one code, in 1/1000 em: `(v_x, v_y, w1_y)`.
    ///
    /// 9.7.4.3. `w1_y` is the displacement — how far the pen moves along the
    /// column, negative because text space has y upward — and `(v_x, v_y)` is
    /// the position vector, which is where within its em box the glyph is
    /// drawn: a comma belongs in a different corner vertically than
    /// horizontally, and the offset is what puts it there.
    ///
    /// Only consulted when [`FontSource::is_vertical`] is true, and it has no
    /// default on purpose. A defaulted one would compile in every
    /// implementation that ever grows a vertical font and quietly advance
    /// every column by nothing — which is the shape of the bug this method
    /// exists to close, where the horizontal advance was applied downward
    /// because nothing else was on offer.
    fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64);

    /// A stable identifier for the named font.
    fn font_id(&self, font: &[u8]) -> u64 {
        let _ = font;
        0
    }

    /// The named font's own name — its `/BaseFont`, as the dictionary writes
    /// it — for [`Glyph::font_name`].
    ///
    /// Asked once per text-showing operator, in the scope the operator runs
    /// in, which is the only place the resource name can be resolved
    /// correctly. `None` — the default — where the source has no name to
    /// give, which is also the honest answer for a font that has none.
    fn font_name(&self, font: &[u8]) -> Option<Arc<str>> {
        let _ = font;
        None
    }

    /// A form XObject, when the interpreter should recurse into one.
    /// Returning `None` skips it.
    fn form(&self, name: &[u8]) -> Option<Form> {
        let _ = name;
        None
    }

    /// The resources a form XObject brings with it, if it has its own.
    ///
    /// 8.10.1: a form's `/Resources` names what its content stream may refer
    /// to, and a form written beside one document and pasted into another
    /// brings the only dictionary its names resolve in. Returning `None` keeps
    /// the invoking scope, which is what a form that omits the key relies on
    /// and what every reader does.
    ///
    /// The device is asked the same question at the same moment, by name,
    /// through [`Device::begin_form`] — the two seams resolve the same form
    /// independently rather than passing a resource object between them, which
    /// is what keeps a crate that must not know what a resource dictionary is
    /// from having to hold one.
    /// Shared rather than owned, and cached by the implementor: a page that
    /// invokes one form a thousand times must not build its resources a
    /// thousand times. Two corpus files stalled outright when it did.
    fn form_scope(&self, name: &[u8]) -> Option<Arc<Self>>
    where
        Self: Sized,
    {
        let _ = name;
        None
    }

    /// The most bytes, as [`crate::record::Event::kept_bytes`] counts them, a
    /// recording of one form's drawing may hold for this source to be offered
    /// it ([`FontSource::form_drawn`]); `0`, the default, keeps none, and the
    /// interpreter then records nothing and asks nothing of the two methods
    /// below.
    ///
    /// **The source decides whether to keep recordings, and keeps them**: the
    /// interpreter holds no cache of its own. It asks for a recording at every
    /// form it enters and offers each one it makes.
    fn form_recording_bytes(&self) -> usize {
        0
    }

    /// The recording this source kept of the form `name` drawn under
    /// `invocation`, from an earlier invocation — `None` for one it has
    /// none of. Asked only where [`FontSource::form_recording_bytes`] is not
    /// `0`, and only after the device has entered the form.
    ///
    /// `invocation` is everything of the interpretation the form's drawing
    /// can read ([`FormInvocation`]); the source adds what only it knows,
    /// which for a form that brings no resources of its own is the scope its
    /// names resolve in, and must hand back a recording only under a key
    /// holding both. The interpreter replays a recording handed back in
    /// place of interpreting the form where the device would give every
    /// answer it records ([`crate::Device::answers_alike`]) and the
    /// interpretation's budgets allow what it spent; otherwise it interprets
    /// the form, as it does where there is none.
    fn kept_form(&self, name: &[u8], invocation: &FormInvocation) -> Option<Arc<FormRecording>> {
        let _ = (name, invocation);
        None
    }

    /// How the form `name` was drawn, said once for each form the
    /// interpreter enters, after it ends, where
    /// [`FontSource::form_recording_bytes`] is not `0` ([`FormDrawn`]).
    fn form_drawn(&self, name: &[u8], drawn: FormDrawn) {
        let _ = (name, drawn);
    }

    /// A Type 3 glyph procedure, with the font's `/FontMatrix` and the
    /// procedure's identity (9.6.5).
    ///
    /// A Type 3 glyph is a content stream, not an outline: it can fill, stroke,
    /// draw images, do anything a page can. So it is *run*, the way a form
    /// XObject is, rather than handed to a glyph source that only knows how to
    /// return curves.
    ///
    /// `None` for every other kind of font, which is what keeps the ordinary
    /// path unchanged.
    fn type3_glyph(&self, font: &[u8], code: u32) -> Option<Type3Glyph> {
        let _ = (font, code);
        None
    }

    /// The RGB a named colour space gives these components, converted in
    /// `intent`, the rendering intent in force (8.6.5.8).
    ///
    /// The interpreter cannot know what `/CS0 0.2 0.9 0.1 scn` means — RGB, a
    /// separation, an indexed palette — because only the resource dictionary
    /// says. `None` leaves the colour unchanged, which is what a viewer does
    /// with a space it cannot resolve.
    ///
    /// Asked again with the components the colour was set with whenever the
    /// intent changes after it was set — by `ri`, or by a `gs` whose `/RI`
    /// names another — so a colour paints in the intent in force at the
    /// painting operator (11.7.5.3). A space no intent changes, which is
    /// every space but one converted through an ICC profile, answers alike in
    /// each. The interpreter asks through [`FontSource::resolve_color_wide`],
    /// whose default asks this.
    fn resolve_color(
        &self,
        space: &[u8],
        components: &[f64],
        intent: RenderingIntent,
    ) -> Option<Rgb> {
        let _ = (space, components, intent);
        None
    }

    /// [`FontSource::resolve_color`] at sixteen bits a channel: the colour's
    /// bytes and its 16-bit light together, the light's top bytes the bytes
    /// (`docs/design/sixteen-bit.md`). The interpreter asks this wherever it
    /// resolves a colour through the seam, and keeps the light beside the
    /// bytes ([`GraphicsState::fill_wide`]).
    ///
    /// The default widens what `resolve_color` answers ([`Rgb16::widen`]): a
    /// correct 16-bit colour that refines nothing, so a source that answers
    /// only bytes keeps working. A source that can evaluate a colour at
    /// sixteen bits answers both from one evaluation.
    fn resolve_color_wide(
        &self,
        space: &[u8],
        components: &[f64],
        intent: RenderingIntent,
    ) -> Option<(Rgb, Rgb16)> {
        self.resolve_color(space, components, intent)
            .map(|color| (color, Rgb16::widen(color)))
    }

    /// The components themselves, as bytes, when the named space **is**
    /// DeviceCMYK, and `None` for every other space — the default, which
    /// keeps every colour light.
    ///
    /// Asked beside [`FontSource::resolve_color`] so that a device
    /// compositing in ink can keep the document's own
    /// ([`GraphicsState::fill_ink`]): light converted back into ink is not
    /// the ink the file chose.
    fn resolve_ink(&self, space: &[u8], components: &[f64]) -> Option<[u8; 4]> {
        let _ = (space, components);
        None
    }

    /// What a colour of the named space paints by colorant (8.6.6.4,
    /// 8.6.6.5): its colorants' tints by name — a `/Separation`'s one, a
    /// `/DeviceN`'s each, DeviceCMYK's four — and the process ink it paints
    /// through its alternate where a device has no colorant of a name it
    /// gives ([`ColorantTints`]), that ink's light converted in `intent`;
    /// `None` for a space that names no colorant of its own, whose light a
    /// device compositing colorants turns into ink.
    ///
    /// [`FontSource::resolve_ink`] generalised, and asked beside it only where
    /// the device composites colorants ([`Device::composites_colorants`]) —
    /// again, as the light is, when the intent changes after the colour was
    /// set — so a light render pays nothing for it. The default answers
    /// `None`.
    fn colorants(
        &self,
        space: &[u8],
        components: &[f64],
        intent: RenderingIntent,
    ) -> Option<Arc<ColorantTints>> {
        let _ = (space, components, intent);
        None
    }

    /// The device space family a named space's colours are in
    /// ([`ColorFamily`]): an `/Indexed` space's base's, a `/Separation` or
    /// `/DeviceN` space's alternate's, a device space a default replaces its
    /// substitute's. Asked once at `cs`, never per colour.
    ///
    /// The default reads the device names alone, which is right for a
    /// source that names no resource spaces and no defaults.
    fn color_family(&self, space: &[u8]) -> ColorFamily {
        ColorFamily::of_device_name(space)
    }

    /// The colour `cs` resets a named space to (8.6.8), which each space's
    /// own clause gives: black in every device space, which for DeviceCMYK is
    /// `0 0 0 1` and not all zeros; a tint of 1.0 for `/Separation` (8.6.6.4)
    /// and 1.0 in every component for `/DeviceN` (8.6.6.5); zero, or the
    /// nearest value a `/Range` allows, for `/Lab` (8.6.5.4).
    ///
    /// The default is all zeros at [`FontSource::color_components`]'s count,
    /// which is right for DeviceGray, DeviceRGB, the CIE-based and ICC
    /// spaces and an `/Indexed` space's index, and wrong for the others
    /// above; an implementation that can tell those apart answers for them.
    fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
        self.color_components(space).map(|n| vec![0.0; n])
    }

    /// Which device spaces this scope remaps to a default colour space
    /// (8.6.5.6), asked once a scope and not once an operator.
    ///
    /// Where one is remapped, `g`/`G`, `rg`/`RG` or `k`/`K`, and `sc`/`scn`
    /// read by component count with no space named, resolve through
    /// [`FontSource::resolve_color`] and [`FontSource::resolve_ink`] under the
    /// device space's own name, as `/DeviceGray cs … sc` does — so the light
    /// is the default's and the ink is the space as selected. The default,
    /// [`DeviceDefaults::NONE`], converts every device colour in place as
    /// 8.6.4 does, which is right for a source that names no default.
    fn device_defaults(&self) -> DeviceDefaults {
        DeviceDefaults::NONE
    }

    /// A named `/ExtGState` (8.4.5), read once into what it sets.
    ///
    /// Every parameter is an `Option` whose `None` leaves it as it was, so a
    /// `gs` that sets only `/ca` neither resets the blend mode nor clears a
    /// soft mask somebody else installed. Shared, so a source that keeps each
    /// name's reading hands every `gs` of the name the same value.
    fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
        let _ = name;
        None
    }

    /// How many components a named colour space takes.
    ///
    /// Used to tell `scn`'s optional trailing pattern name from its numeric
    /// operands.
    fn color_components(&self, space: &[u8]) -> Option<usize> {
        let _ = space;
        None
    }

    /// The optional-content layer a name in the `/Properties` resource
    /// sub-dictionary selects (8.11.3.2).
    ///
    /// `None` means the name is not optional content at all — an ordinary
    /// `/Span` or `/Artifact` property list, or a name the resources do not
    /// define — and that is not the same as a hidden layer. Every `None`
    /// path here ends in content being painted, which is the direction a
    /// reader can recover from.
    fn optional_content(&self, name: &[u8]) -> Option<Layer> {
        let _ = name;
        None
    }

    /// The `/OC` entry on a form or image XObject (8.11.4.4).
    ///
    /// Separate from [`FontSource::optional_content`] because the two look in
    /// different dictionaries: one in `/Properties`, the other on the XObject
    /// itself.
    fn xobject_optional_content(&self, name: &[u8]) -> Option<Layer> {
        let _ = name;
        None
    }

    /// The 14.6.2 and 14.9 values a *named* property list holds
    /// (`/P /MC0 BDC`).
    ///
    /// The same `/Properties` sub-dictionary [`FontSource::optional_content`]
    /// reads, asked a different question: that one wants a layer, this one
    /// wants `/MCID` and the alternate descriptions 14.9 allows beside it. A
    /// name may legitimately answer both, neither, or one — an `/OC` list is
    /// an optional-content group with no `/MCID`, and a `/Span` list is a
    /// property list with no layer — so they are two methods rather than one
    /// returning a pair, and a build that supplies only one is not broken.
    ///
    /// `None` means the name reached no property list this build could read.
    /// It never means "hidden": nothing in this method's answer can suppress
    /// content.
    fn marked_content_properties(&self, name: &[u8]) -> Option<MarkedProps> {
        let _ = name;
        None
    }
}

/// What a form's drawing can read of the interpretation it is invoked in, as
/// one key: the form, and the invoking state written exactly
/// (`docs/design/performance.md`, P-16, "A hit is the bytes a miss
/// computes").
///
/// The state is everything the interpreter carries into a form and does not
/// reset there: the whole graphics state — the transform, the colours as
/// bytes and at sixteen bits and their spaces' names, what each paints by
/// colorant, the pen, the text state and its font's name, the alphas, the
/// blend mode and the stored parameters — the text and line matrices, the
/// current point and the subpath's start, a clip waiting for a path, the
/// operands left on the stack before `Do`, how deep the form is and the
/// streams running around it, which device spaces the scope in force remaps,
/// and whether the device composites colorants, since a drawing recorded for
/// one carries them in every state and one recorded for any other carries
/// none. Every number is its bits, so two invocations are one key exactly
/// when nothing a form could read differs. A state holding a function or a
/// halftone (`StoredParameters`' transfer, generation, removal and halftone)
/// has no key, and its forms are interpreted.
///
/// What it does not hold is the scope: a form that brings no resources of
/// its own resolves its names in the invoking scope, which only the source
/// knows ([`FontSource::kept_form`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FormInvocation {
    /// The form, as [`Form::stream`] names it: never `0`, since a form
    /// reached without a reference has no identity to keep a drawing under.
    pub stream: u64,
    /// The invoking state, written exactly.
    state: Vec<u8>,
}

impl FormInvocation {
    /// How many bytes the written state holds, for a source that charges a
    /// key.
    #[must_use]
    pub fn written_len(&self) -> usize {
        self.state.len()
    }
}

/// How the interpreter drew a form it entered ([`FontSource::form_drawn`]).
#[derive(Clone, Debug)]
pub enum FormDrawn {
    /// Replayed from the recording [`FontSource::kept_form`] handed back:
    /// nothing of the form was interpreted.
    Replayed,
    /// Interpreted, with nothing recorded: the invocation's state has no key
    /// ([`FormInvocation`]), or the form has no identity, or the source held
    /// a recording this invocation could not replay.
    Interpreted,
    /// Interpreted after [`FontSource::kept_form`] held no recording for
    /// `invocation`: what was recorded of the drawing, or `None` where it is
    /// not one to keep — the device was cancelled; a nested form, mask or
    /// glyph was refused for depth, for a cycle or for a budget, which
    /// depends on more than the key; or the recording passed
    /// [`FontSource::form_recording_bytes`].
    Recorded {
        /// The key the recording was made under.
        invocation: FormInvocation,
        /// The recording.
        recording: Option<FormRecording>,
    },
}

/// One form invocation's drawing, recorded to be replayed in place of
/// interpreting the form again under the same key ([`FontSource::kept_form`]).
///
/// The calls the device was handed from the one after `begin_form` to the
/// one before `end_form`, each with the state it saw and the device's answer
/// to each of its questions, as a page's display list records them; what the
/// drawing spent of the interpretation's budgets for nested content, in the
/// order it spent it; and where it left the current point and a clip waiting
/// for a path, which a form does not restore. Nothing in it is a pixel.
#[derive(Clone, Debug)]
pub struct FormRecording {
    events: Vec<Event>,
    spent: Vec<Spent>,
    after: After,
    needs: Needs,
    bytes: usize,
}

impl FormRecording {
    /// The calls the drawing made, in order.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// What the recording holds, in bytes: its events as
    /// [`Event::kept_bytes`] counts them, its budget operations and its own
    /// size.
    #[must_use]
    pub fn kept_bytes(&self) -> usize {
        self.bytes
    }
}

/// Where a form's drawing left what the interpreter does not restore after
/// it.
#[derive(Clone, Copy, Debug)]
struct After {
    current: (f64, f64),
    start: (f64, f64),
    pending_clip: Option<bool>,
    /// Whether the drawing set the current point, and the subpath's start.
    set_current: bool,
    set_start: bool,
}

/// What a form's drawing read of the interpretation it was invoked in beside
/// its key: the text matrix, the line matrix, the current point and the
/// subpath's start, each where the drawing read the value it was invoked with
/// before it set its own, and `None` where it did not.
///
/// Not in the key, because a page leaves them wherever its last text object
/// and its last path did, which differs from page to page while a form that
/// sets its own — a `BT`, an `m` — draws the same: a replay asks for them to
/// be what they were only where the drawing read them
/// ([`Interpreter::replays`]).
#[derive(Clone, Copy, Debug, Default)]
struct Needs {
    text: Option<Matrix>,
    line: Option<Matrix>,
    current: Option<(f64, f64)>,
    start: Option<(f64, f64)>,
}

/// Which of the interpretation's four unrestored values a nested stream's run
/// has set since it began, for a recording around it to tell its reads of
/// what it was invoked with from reads of what it set itself
/// ([`Needs`]): the text and line matrices, which the run saves and restores.
#[derive(Clone, Copy, Debug, Default)]
struct RunMarks {
    text: bool,
    line: bool,
}

/// One thing a drawing spent of the interpretation's budgets for nested
/// content, in the order it spent them: what a replay checks and spends
/// again, where the budgets stand then.
#[derive(Clone, Copy, Debug)]
enum Spent {
    /// [`Interpreter::group_run_left`] answered yes.
    GroupCheck,
    /// A group or a mask's group ran, spending one of [`MAX_GROUP_RUNS`].
    GroupRun,
    /// [`Interpreter::nested_work_left`] answered yes for `len` bytes of
    /// `stream`.
    NestedCheck { stream: u64, len: u64 },
    /// A nested run spent `len` bytes and [`NESTED_RUN_COST`].
    NestedSpend { len: u64 },
}

/// A [`FormInvocation`]'s state as it is written: every number by its bits,
/// every byte string after its length, every enum by its name.
struct Exact(Vec<u8>);

impl Exact {
    fn num(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    fn word(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn flag(&mut self, value: bool) {
        self.0.push(u8::from(value));
    }

    fn bytes(&mut self, value: &[u8]) {
        self.word(value.len() as u64);
        self.0.extend_from_slice(value);
    }

    fn maybe_bytes(&mut self, value: Option<&[u8]>) {
        match value {
            None => self.0.push(0),
            Some(value) => {
                self.0.push(1);
                self.bytes(value);
            }
        }
    }

    fn maybe_num(&mut self, value: Option<f64>) {
        match value {
            None => self.0.push(0),
            Some(value) => {
                self.0.push(1);
                self.num(value);
            }
        }
    }

    /// A fieldless enum, by the name its `Debug` gives it: two values are one
    /// name exactly when they are one variant.
    fn named(&mut self, value: &impl core::fmt::Debug) {
        self.bytes(format!("{value:?}").as_bytes());
    }

    fn matrix(&mut self, m: &Matrix) {
        for value in [m.a, m.b, m.c, m.d, m.e, m.f] {
            self.num(value);
        }
    }

    fn token(&mut self, token: &Token) {
        match token {
            Token::Number(value) => {
                self.0.push(0);
                self.num(*value);
            }
            Token::String(bytes) => {
                self.0.push(1);
                self.bytes(bytes);
            }
            Token::Name(bytes) => {
                self.0.push(2);
                self.bytes(bytes);
            }
            Token::ArrayOpen => self.0.push(3),
            Token::ArrayClose => self.0.push(4),
            Token::DictOpen => self.0.push(5),
            Token::DictClose => self.0.push(6),
            Token::Bool(value) => {
                self.0.push(7);
                self.flag(*value);
            }
            Token::Null => self.0.push(8),
            Token::Operator(bytes) => {
                self.0.push(9);
                self.bytes(bytes);
            }
        }
    }

    /// What a colour paints by colorant ([`ColorantTints`]), exactly: its
    /// kind, each colorant's name, tint and own ink, and the inks it paints
    /// through its alternate and its process space.
    fn colorants(&mut self, colorants: &ColorantTints) {
        let ColorantTints {
            kind,
            tints,
            alternate,
            process,
        } = colorants;
        self.named(kind);
        self.word(tints.len() as u64);
        for crate::state::ColorantTint { name, tint, alone } in tints {
            self.bytes(name);
            self.0.push(*tint);
            self.maybe_ink(alone.as_ref());
        }
        self.maybe_ink(alternate.as_ref());
        self.maybe_ink(process.as_ref());
    }

    fn maybe_ink(&mut self, value: Option<&[u8; 4]>) {
        match value {
            None => self.0.push(0),
            Some(ink) => {
                self.0.push(1);
                self.0.extend_from_slice(ink);
            }
        }
    }

    /// The whole graphics state, or `None` where it holds a function or a
    /// halftone, which no key writes. Every field is named, so a field the
    /// state gains is a field this must write before it compiles.
    fn state(&mut self, state: &GraphicsState) -> Option<()> {
        let GraphicsState {
            ctm,
            text,
            fill_alpha,
            stroke_alpha,
            fill_color,
            stroke_color,
            fill_wide,
            stroke_wide,
            fill_ink,
            stroke_ink,
            fill_family,
            stroke_family,
            line_width,
            line_cap,
            line_join,
            miter_limit,
            dashes,
            dash_phase,
            fill_space,
            stroke_space,
            fill_pattern,
            blend,
            stroke_pattern,
            stored,
            fill_source,
            stroke_source,
            fill_colorants,
            stroke_colorants,
        } = state;
        self.matrix(ctm);
        let crate::state::TextState {
            size,
            char_spacing,
            word_spacing,
            horizontal_scale,
            leading,
            rise,
            render_mode,
            font,
        } = text;
        for value in [
            *size,
            *char_spacing,
            *word_spacing,
            *horizontal_scale,
            *leading,
            *rise,
        ] {
            self.num(value);
        }
        self.named(render_mode);
        self.maybe_bytes(font.as_deref());
        self.num(*fill_alpha);
        self.num(*stroke_alpha);
        for colour in [fill_color, stroke_color] {
            self.0.extend_from_slice(&[colour.r, colour.g, colour.b]);
        }
        // The same colours at sixteen bits, which a form drawing at that
        // width reads as its inherited colour.
        for colour in [fill_wide, stroke_wide] {
            for channel in [colour.r, colour.g, colour.b] {
                self.0.extend_from_slice(&channel.to_be_bytes());
            }
        }
        for ink in [fill_ink, stroke_ink] {
            match ink {
                None => self.0.push(0),
                Some(ink) => {
                    self.0.push(1);
                    self.0.extend_from_slice(ink);
                }
            }
        }
        self.named(fill_family);
        self.named(stroke_family);
        // What each colour paints by colorant, where the device asked.
        for colorants in [fill_colorants, stroke_colorants] {
            match colorants.as_deref() {
                None => self.0.push(0),
                Some(colorants) => {
                    self.0.push(1);
                    self.colorants(colorants);
                }
            }
        }
        // What each colour resolves again from when the intent changes.
        for source in [fill_source, stroke_source] {
            match source.as_deref() {
                None => self.0.push(0),
                Some(crate::state::ColorSource { family, components }) => {
                    self.0.push(1);
                    self.maybe_bytes(*family);
                    self.word(components.len() as u64);
                    for component in components {
                        self.num(*component);
                    }
                }
            }
        }
        self.num(*line_width);
        self.named(line_cap);
        self.named(line_join);
        self.num(*miter_limit);
        self.word(dashes.len() as u64);
        for dash in dashes {
            self.num(*dash);
        }
        self.num(*dash_phase);
        self.maybe_bytes(fill_space.as_deref());
        self.maybe_bytes(stroke_space.as_deref());
        self.maybe_bytes(fill_pattern.as_deref());
        self.named(blend);
        self.maybe_bytes(stroke_pattern.as_deref());
        match stored.as_deref() {
            None => self.0.push(0),
            Some(stored) => {
                let crate::state::StoredParameters {
                    rendering_intent,
                    black_point_compensation,
                    stroke_overprint,
                    fill_overprint,
                    overprint_mode,
                    alpha_is_shape,
                    text_knockout,
                    flatness,
                    smoothness,
                    stroke_adjustment,
                    black_generation,
                    undercolor_removal,
                    transfer,
                    halftone,
                    halftone_origin,
                } = stored;
                if black_generation.is_some()
                    || undercolor_removal.is_some()
                    || transfer.is_some()
                    || halftone.is_some()
                {
                    return None;
                }
                self.0.push(1);
                self.named(rendering_intent);
                self.named(black_point_compensation);
                self.flag(*stroke_overprint);
                self.flag(*fill_overprint);
                self.word(*overprint_mode as u64);
                self.flag(*alpha_is_shape);
                self.flag(*text_knockout);
                self.maybe_num(*flatness);
                self.maybe_num(*smoothness);
                match stroke_adjustment {
                    None => self.0.push(0),
                    Some(value) => {
                        self.0.push(1);
                        self.flag(*value);
                    }
                }
                match halftone_origin {
                    None => self.0.push(0),
                    Some([x, y]) => {
                        self.0.push(1);
                        self.num(*x);
                        self.num(*y);
                    }
                }
            }
        }
        Some(())
    }
}

/// How deep form XObjects may nest before recursion is refused (8.10).
const MAX_FORM_DEPTH: u32 = 16;

/// How many transparency groups and soft masks' groups one interpretation
/// runs the content of, at any depth and whatever the device answers (11.6.6,
/// 11.6.5.2).
///
/// [`MAX_FORM_DEPTH`] bounds how deep the nesting goes and not how much work
/// it is, which are different numbers the moment the recursion *branches*: a
/// group form whose content draws another twice, each drawing another twice,
/// is two to the sixteenth runs inside the depth — and a group that draws
/// itself is refused as a cycle only where it has an identity
/// ([`Form::stream`]). A device that holds group buffers declines the ones past
/// its own budget, the renderer at two thousand, and that bounded its buffers
/// and not the walk: a declined group still runs as content, and a device
/// that keeps no buffer — the text device, the image lister — declines every
/// group and runs them all. So every reading of such a page paid for the
/// whole tree, and one that also paid per element written, the SVG writer,
/// did not finish.
///
/// This is the same budget in the interpreter, for every device: spent and
/// never refunded, so a page cannot recover budget by unwinding and going
/// again, and two thousand like the renderer's, far above any real page. A
/// soft mask spends only when the device accepts it, because only then is its
/// group run; past the budget neither is offered, and the device hears
/// [`Device::group_budget_spent`] once.
pub const MAX_GROUP_RUNS: u32 = 2000;

/// The floor of the budget one interpretation spends on nested content, in
/// bytes: forms, soft masks' groups and Type 3 glyph procedures (8.10,
/// 11.6.5.2, 9.6.5).
///
/// [`MAX_FORM_DEPTH`] bounds how deep nested content goes and
/// [`MAX_GROUP_RUNS`] how many groups run, and neither bounds the plain forms
/// and glyph procedures most nested content is: a chain of sixteen forms, each
/// drawing the next ten times, is ten to the sixteenth runs inside the depth.
/// A stream already running in its own ancestry is not run again there, which
/// leaves trees, and a tree is bounded by its size only through a budget.
///
/// The budget is this floor and [`NESTED_WORK_RATE`] bytes for every byte of
/// content the interpretation has reached — its own stream, and each nested
/// stream's length the first time it is reached — and each run spends its
/// stream's length and [`NESTED_RUN_COST`]. A rate rather than one number,
/// because reuse is what forms and glyphs are for: a figure of a million
/// plotted markers runs one marker's few bytes a million times, and a cap
/// high enough for it would let a few hostile kilobytes run for minutes.
/// Under a rate a page may run what it holds four thousand times over, and a
/// file of a few kilobytes about the floor.
///
/// Spent and never refunded. Reaching a stream raises the budget by more than
/// its run costs, so the first run of a stream with content and an identity
/// always fits: what the budget passes over is only content that has already
/// run once. Past it the device hears [`Device::nested_budget_spent`] once.
pub const NESTED_WORK_FLOOR: u64 = 4 << 20;

/// What [`NESTED_WORK_FLOOR`]'s budget grows by for every byte of content an
/// interpretation reaches.
pub const NESTED_WORK_RATE: u64 = 4096;

/// What one nested run spends from [`NESTED_WORK_FLOOR`]'s budget beyond its
/// stream's length: a run is a lookup, a decode and a saved state before its
/// first byte, so a tree of empty forms is bounded too.
pub const NESTED_RUN_COST: u64 = 256;

/// Whether a run of `len` bytes fits the nested-content budget an
/// interpretation that has spent `spent` and reached `reached` bytes has:
/// [`NESTED_WORK_FLOOR`] and [`NESTED_WORK_RATE`] for every byte reached.
fn nested_fits(spent: u64, reached: u64, len: u64) -> bool {
    let budget = NESTED_WORK_FLOOR.saturating_add(reached.saturating_mul(NESTED_WORK_RATE));
    nested_after(spent, len) <= budget
}

/// What a run of `len` bytes leaves spent of the nested-content budget after
/// `spent`.
fn nested_after(spent: u64, len: u64) -> u64 {
    spent.saturating_add(len).saturating_add(NESTED_RUN_COST)
}

/// A `BDC`'s property list, in the two forms 14.6.2 gives it.
///
/// Three variants and not two: "the file wrote no readable list" is a
/// different fact from "the file wrote an empty one", and only the first may
/// ever be inferred from damage. Collapsing them would make a truncated
/// stream indistinguishable from a producer that meant `<< >>`.
enum Properties {
    /// `/Tag /MC0 BDC` — a name into the page's `/Properties`.
    Named(Vec<u8>),
    /// `/Tag << … >> BDC` — an inline dictionary, already reassembled.
    Inline(MarkedProps),
    /// Neither: a `BDC` whose operands this build could not read.
    None,
}

/// Reads the plain values out of a flattened inline property list.
///
/// `body` is every token strictly between the `<<` and its `>>`, in stream
/// order. 7.8.2's tokenizer does not build dictionaries, so this walks the
/// flat run: a name is a key, the token after it is its value, and a value
/// that opens a container is skipped whole. Only 14.7.4.2's `/MCID` and
/// 14.9's four text strings are kept — everything else a property list may
/// carry (`/Type`, `/BBox`, `/Placement`, a producer's private entries) is
/// read past rather than stored, because storing it would put a COS-shaped
/// object in a crate that has no object model.
///
/// **Nothing here can fail.** A key with no value, a value of the wrong
/// type, an unbalanced container, a run of bare values with no key at all:
/// each is skipped and the walk continues. What comes back is what was
/// legible, which for a damaged list is nothing — and nothing is a visible
/// scope with no `/MCID`, which is the direction 14.6.2's failures are ruled
/// to fall in.
fn read_inline_properties(body: &[Token]) -> MarkedProps {
    let mut props = MarkedProps::default();
    let mut at = 0usize;

    while at < body.len() && at < MAX_INLINE_PROPERTY_TOKENS {
        let Some(Token::Name(key)) = body.get(at) else {
            // A value where a key belongs. Step over exactly one token so a
            // run of them costs one step each rather than stalling.
            at += 1;
            continue;
        };
        at += 1;
        let Some(value) = body.get(at) else {
            // 7.3.7's "a key with no value"; there is nothing after it.
            break;
        };
        // A container-valued entry is skipped whole: none of the five keys
        // read here is ever an array or a dictionary, and stepping into one
        // would read its members as top-level keys.
        if matches!(value, Token::ArrayOpen | Token::DictOpen) {
            at = skip_container(body, at);
            continue;
        }
        at += 1;

        match (key.as_slice(), value) {
            // 14.7.4.2: a non-negative integer. `f64` is what the tokenizer
            // produces for every number, so integrality is checked rather
            // than assumed — an `/MCID 3.5` names no marked sequence, and
            // rounding it would join content to the wrong element.
            (b"MCID", Token::Number(n)) => {
                let integral = n.is_finite() && n.fract() == 0.0;
                if integral && *n >= 0.0 && *n <= f64::from(u32::MAX) {
                    props.mcid = Some(*n as u32);
                }
            }
            // 14.9.2 to 14.9.5. All four are *text strings* (7.9.2.2), so
            // they are decoded by the document's own rules rather than read
            // as UTF-8: a `/Alt` written UTF-16BE is the common case, and
            // taking its bytes literally yields interleaved NULs.
            (b"ActualText", Token::String(bytes)) => {
                props.actual_text = Some(decode_text_string(bytes));
            }
            (b"Alt", Token::String(bytes)) => props.alt = Some(decode_text_string(bytes)),
            (b"Lang", Token::String(bytes)) => props.lang = Some(decode_text_string(bytes)),
            (b"E", Token::String(bytes)) => props.expansion = Some(decode_text_string(bytes)),
            _ => {}
        }
    }

    props
}

/// The index just past the container opening at `at`.
///
/// Arrays and dictionaries are counted together, because 7.8.2 flattens both
/// and a stream may close one with the other's delimiter. Treating them as
/// one nesting level means a mismatched close ends the container instead of
/// leaving the scan inside it forever, which is the failure that matters:
/// this walk must terminate on every input, and it is not the place where a
/// malformed inline dictionary gets diagnosed.
fn skip_container(body: &[Token], at: usize) -> usize {
    let mut depth = 0usize;
    let mut index = at;
    while index < body.len() && index < MAX_INLINE_PROPERTY_TOKENS {
        match body[index] {
            Token::ArrayOpen | Token::DictOpen => depth += 1,
            Token::ArrayClose | Token::DictClose => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    index
}

/// How many tokens of one flattened inline property list are examined
/// (14.6.2, 7.8.2).
///
/// The operand stack is already bounded to 512 by [`Interpreter::run`], so
/// this can never be the binding limit today. It is named anyway because the
/// scan below is the only place in the interpreter that walks the stack
/// *forwards* over an unbounded number of key/value pairs, and a later
/// change to the stack bound must not silently make that walk unbounded.
const MAX_INLINE_PROPERTY_TOKENS: usize = 512;

/// How many marked-content scopes may be open at once (14.6.2).
///
/// Past the cap a scope is not reported to the device at all, and `EMC`
/// balances against the same counter so the nesting the device sees stays
/// correct. Unreported means *visible*, which is the direction a runaway
/// stream should fail in: the alternative is a file that hides a page by
/// nesting `BDC` four thousand deep.
const MAX_MARKED_CONTENT_DEPTH: u32 = 4096;

/// Runs a content stream against a device.
pub fn interpret<D: Device, F: FontSource>(
    content: &[u8],
    initial: Matrix,
    device: &mut D,
    fonts: &F,
) {
    interpret_steps(content, initial, device, fonts);
}

/// [`interpret`], counting what the run read into `work`: every token the
/// interpreter took from its tokenizer and every operator it ran, in the
/// stream and in every form, soft mask's group and Type 3 glyph procedure the
/// stream runs ([`Stats::content_tokens_read`](tinker_pdf_cos::Stats::content_tokens_read),
/// [`Stats::operators_run`](tinker_pdf_cos::Stats::operators_run);
/// `docs/design/resource-exhaustion.md`, "The counters").
///
/// What the device hears is what [`interpret`] makes it hear: the counts are
/// the interpreter's own integers, added to `work` once, when the run ends.
pub fn interpret_counted<D: Device, F: FontSource>(
    content: &[u8],
    initial: Matrix,
    device: &mut D,
    fonts: &F,
    work: &Work,
) {
    let steps = interpret_steps(content, initial, device, fonts);
    work.add(Counter::ContentTokensRead, steps.tokens);
    work.add(Counter::OperatorsRun, steps.operators);
}

/// What one interpretation read.
#[derive(Clone, Copy, Debug, Default)]
struct Steps {
    tokens: u64,
    operators: u64,
}

fn interpret_steps<D: Device, F: FontSource>(
    content: &[u8],
    initial: Matrix,
    device: &mut D,
    fonts: &F,
) -> Steps {
    let mut interp = Interpreter::new(device, fonts, initial, content.len());
    interp.run(content);
    // 14.6.2: a stream may end with scopes still open, and a device left
    // inside one would treat everything after it as part of that layer.
    interp.close_open_marked_content();
    // 8.4.2: and with `q`s still open, and a device left inside one would
    // clip and mask what it draws next — an annotation's appearance after the
    // page's content — by this stream's state.
    interp.close_open_saves();
    interp.steps
}

/// The device the interpreter talks to, and the tape a form's drawing is
/// recorded on while a recording is open ([`FontSource::form_drawn`]): every
/// call the device is handed is copied onto the tape with the answer the
/// device gave, so a recording is what this device was told.
struct Out<'d, D: Device> {
    device: &'d mut D,
    tape: Tape,
}

/// The recordings of forms' drawings being made, one inside another as forms
/// nest, over one run of calls.
#[derive(Default)]
struct Tape {
    /// The calls made since the outermost open recording began.
    recorder: RecordingDevice,
    /// What the calls hold, by [`Event::kept_bytes`], with the budget
    /// operations beside them.
    bytes: usize,
    /// The budget operations the open recordings spent, in order.
    spent: Vec<Spent>,
    /// The recordings open, outermost first.
    open: Vec<Opened>,
    /// Whether the run passed the bound of a recording open over it: nothing
    /// more is kept until the outermost closes.
    overflowed: bool,
}

/// One recording open on the [`Tape`].
struct Opened {
    /// Where its calls begin on the tape.
    events: usize,
    /// Where its budget operations begin.
    spent: usize,
    /// The most it may hold.
    limit: usize,
    /// Whether it is not one to keep.
    spoiled: bool,
    /// The run its form's content is: the index the form's [`RunMarks`]
    /// takes.
    run: usize,
    /// The four unrestored values the form was invoked with.
    invoked: After,
    text: Matrix,
    line: Matrix,
    /// What the drawing read of them before it set its own.
    needs: Needs,
    /// Whether the current point and the subpath's start are still the ones
    /// the form was invoked with: nothing since it began has set them.
    current_kept: bool,
    start_kept: bool,
}

impl Tape {
    /// Whether a call made now is recorded.
    fn taping(&self) -> bool {
        !self.overflowed && self.open.iter().any(|opened| !opened.spoiled)
    }

    /// Marks every open recording as not one to keep: something it holds
    /// depends on more than its key.
    fn spoil(&mut self) {
        for opened in &mut self.open {
            opened.spoiled = true;
        }
    }

    /// Counts what the call just recorded holds, and stops the tape where
    /// the run passes the least bound of the recordings open over it.
    fn counted(&mut self, bytes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
        let limit = self.open.iter().map(|opened| opened.limit).min();
        if limit.is_some_and(|limit| self.bytes > limit) {
            self.spoil();
            self.overflowed = true;
            self.recorder = RecordingDevice::new();
            self.spent = Vec::new();
            self.bytes = 0;
        }
    }

    /// Counts the call the recorder just kept.
    fn record(&mut self) {
        let bytes = self.recorder.events().last().map_or(0, Event::kept_bytes);
        self.counted(bytes);
    }

    /// Notes one budget operation for every open recording.
    fn spend(&mut self, spent: Spent) {
        if self.taping() {
            self.spent.push(spent);
            self.counted(core::mem::size_of::<Spent>());
        }
    }

    /// Opens a recording of at most `limit` bytes, of a form whose run
    /// takes the index `run` and is invoked with `invoked`, `text` and
    /// `line`.
    fn open(&mut self, limit: usize, run: usize, invoked: After, text: Matrix, line: Matrix) {
        self.open.push(Opened {
            events: self.recorder.events().len(),
            spent: self.spent.len(),
            limit,
            spoiled: self.overflowed,
            run,
            invoked,
            text,
            line,
            needs: Needs::default(),
            current_kept: true,
            start_kept: true,
        });
    }

    /// Closes the innermost recording, handing it back where it is one to
    /// keep, with `after` as where it leaves the interpretation.
    fn close(&mut self, after: After) -> Option<FormRecording> {
        let opened = self.open.pop()?;
        let after = After {
            set_current: !opened.current_kept,
            set_start: !opened.start_kept,
            ..after
        };
        let outermost = self.open.is_empty();
        let kept = if opened.spoiled {
            None
        } else {
            let (events, spent) = if outermost {
                (
                    self.recorder.split_off(opened.events),
                    self.spent.split_off(opened.spent.min(self.spent.len())),
                )
            } else {
                (
                    self.recorder
                        .events()
                        .get(opened.events..)
                        .map(<[Event]>::to_vec)
                        .unwrap_or_default(),
                    self.spent
                        .get(opened.spent..)
                        .map(<[Spent]>::to_vec)
                        .unwrap_or_default(),
                )
            };
            let bytes = events
                .iter()
                .map(Event::kept_bytes)
                .fold(core::mem::size_of::<FormRecording>(), usize::saturating_add)
                .saturating_add(spent.len().saturating_mul(core::mem::size_of::<Spent>()));
            (bytes <= opened.limit).then_some(FormRecording {
                events,
                spent,
                after,
                needs: opened.needs,
                bytes,
            })
        };
        if outermost {
            *self = Tape::default();
        }
        kept
    }
}

impl<D: Device> Out<'_, D> {
    /// Sets the tape's answers to the device's before a question is copied.
    fn answered(&mut self, change: impl FnOnce(&mut Answers)) {
        let mut answers = self.tape.recorder.answers();
        change(&mut answers);
        self.tape.recorder.set_answers(answers);
    }
}

impl<D: Device> Device for Out<'_, D> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.device.show_glyph(glyph, state);
        if self.tape.taping() {
            self.tape.recorder.show_glyph(glyph, state);
            self.tape.record();
        }
    }

    fn begin_text(&mut self) {
        self.device.begin_text();
        if self.tape.taping() {
            self.tape.recorder.begin_text();
            self.tape.record();
        }
    }

    fn end_text(&mut self) {
        self.device.end_text();
        if self.tape.taping() {
            self.tape.recorder.end_text();
            self.tape.record();
        }
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.device.fill_path(path, state, even_odd);
        if self.tape.taping() {
            self.tape.recorder.fill_path(path, state, even_odd);
            self.tape.record();
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        self.device.stroke_path(path, state);
        if self.tape.taping() {
            self.tape.recorder.stroke_path(path, state);
            self.tape.record();
        }
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.device.clip_path(path, state, even_odd);
        if self.tape.taping() {
            self.tape.recorder.clip_path(path, state, even_odd);
            self.tape.record();
        }
    }

    fn save_state(&mut self) {
        self.device.save_state();
        if self.tape.taping() {
            self.tape.recorder.save_state();
            self.tape.record();
        }
    }

    fn restore_state(&mut self) {
        self.device.restore_state();
        if self.tape.taping() {
            self.tape.recorder.restore_state();
            self.tape.record();
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        self.device.draw_image(image, state);
        if self.tape.taping() {
            self.tape.recorder.draw_image(image, state);
            self.tape.record();
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        self.device.draw_shading(name, state);
        if self.tape.taping() {
            self.tape.recorder.draw_shading(name, state);
            self.tape.record();
        }
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.device
            .begin_marked_content(tag, visible, hidden_layer, props);
        if self.tape.taping() {
            self.tape
                .recorder
                .begin_marked_content(tag, visible, hidden_layer, props);
            self.tape.record();
        }
    }

    fn end_marked_content(&mut self) {
        self.device.end_marked_content();
        if self.tape.taping() {
            self.tape.recorder.end_marked_content();
            self.tape.record();
        }
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        let entered = self.device.begin_form(id, name);
        if self.tape.taping() {
            self.answered(|answers| answers.forms = entered);
            self.tape.recorder.begin_form(id, name);
            self.tape.record();
        }
        entered
    }

    fn end_form(&mut self, id: u64) {
        self.device.end_form(id);
        if self.tape.taping() {
            self.tape.recorder.end_form(id);
            self.tape.record();
        }
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let entered = self.device.begin_group(group, state);
        if self.tape.taping() {
            self.answered(|answers| answers.groups = entered);
            self.tape.recorder.begin_group(group, state);
            self.tape.record();
        }
        entered
    }

    fn end_group(&mut self) {
        self.device.end_group();
        if self.tape.taping() {
            self.tape.recorder.end_group();
            self.tape.record();
        }
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let entered = self.device.begin_soft_mask(mask, bbox, state);
        if self.tape.taping() {
            self.answered(|answers| answers.soft_masks = entered);
            self.tape.recorder.begin_soft_mask(mask, bbox, state);
            self.tape.record();
        }
        entered
    }

    fn end_soft_mask(&mut self) {
        self.device.end_soft_mask();
        if self.tape.taping() {
            self.tape.recorder.end_soft_mask();
            self.tape.record();
        }
    }

    fn clear_soft_mask(&mut self) {
        self.device.clear_soft_mask();
        if self.tape.taping() {
            self.tape.recorder.clear_soft_mask();
            self.tape.record();
        }
    }

    fn group_budget_spent(&mut self) {
        self.device.group_budget_spent();
        if self.tape.taping() {
            self.tape.recorder.group_budget_spent();
            self.tape.record();
        }
    }

    fn recursion_refused(&mut self) {
        self.device.recursion_refused();
        if self.tape.taping() {
            self.tape.recorder.recursion_refused();
            self.tape.record();
        }
    }

    fn nested_budget_spent(&mut self) {
        self.device.nested_budget_spent();
        if self.tape.taping() {
            self.tape.recorder.nested_budget_spent();
            self.tape.record();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.device.is_cancelled()
    }

    fn answers_alike(&self, events: &[Event]) -> bool {
        self.device.answers_alike(events)
    }
}

struct Interpreter<'d, D: Device, F: FontSource> {
    device: Out<'d, D>,
    fonts: Scope<'d, F>,
    /// What [`FontSource::device_defaults`] answered for the scope in force,
    /// asked when the scope came into force and never per operator (8.6.5.6).
    defaults: DeviceDefaults,
    /// Whether the device composites colorants
    /// ([`Device::composites_colorants`]), asked once: each colour is then
    /// set with what it paints by colorant, and on any other device the
    /// resources are asked nothing of the kind.
    colorants: bool,
    stack: Vec<Token>,
    gs: GraphicsState,
    saved: Vec<GraphicsState>,
    text_matrix: Matrix,
    line_matrix: Matrix,
    path: Vec<PathSegment>,
    current: (f64, f64),
    start: (f64, f64),
    depth: u32,
    pending_clip: Option<bool>,
    /// Marked-content scopes open in the stream being run, and reported to
    /// the device (14.6.2).
    marked: u32,
    /// Scopes refused by [`MAX_MARKED_CONTENT_DEPTH`], so that an `EMC`
    /// closes the one it belongs to rather than an outer one.
    marked_over_cap: u32,
    /// The form XObject streams being run, innermost last (14.7.4.2).
    ///
    /// Empty while the page's own content stream is running, which is why
    /// [`Interpreter::stream`] reads an empty stack as `0`. A form whose
    /// [`Form::stream`] is `0` — one the resource layer reached without a
    /// reference — pushes nothing, so its content keeps the identity of the
    /// stream that invoked it: that is the old behaviour, kept for the case
    /// where there is no better answer rather than asserted as one.
    streams: Vec<u64>,
    /// Groups and soft masks' groups whose content this interpretation has
    /// run, against [`MAX_GROUP_RUNS`].
    group_runs: u32,
    /// Whether the device has heard [`Device::group_budget_spent`].
    group_budget_told: bool,
    /// The Type 3 glyph procedures being run, innermost last, by
    /// [`Type3Glyph::stream`]: with `streams`, the ancestry a nested stream is
    /// refused in when it is already running there.
    procedures: Vec<u64>,
    /// Whether the device has heard [`Device::recursion_refused`].
    recursion_told: bool,
    /// The nested streams whose length is in `reached`.
    reached_streams: BTreeSet<u64>,
    /// Bytes of content this interpretation has reached: its own stream and
    /// each nested stream once, which [`NESTED_WORK_RATE`] scales.
    reached: u64,
    /// Bytes of the nested-content budget spent ([`NESTED_WORK_FLOOR`]).
    nested_spent: u64,
    /// Whether the device has heard [`Device::nested_budget_spent`].
    nested_budget_told: bool,
    /// What this run has read, nested content included: the counts
    /// [`interpret_counted`] hands to its tally.
    steps: Steps,
    /// The nested streams running, innermost last, and which of the text and
    /// line matrices each has set since it began ([`RunMarks`]).
    runs: Vec<RunMarks>,
}

impl<'d, D: Device, F: FontSource> Interpreter<'d, D, F> {
    /// An interpretation of a stream of `len` bytes into `device` over
    /// `fonts`, from the initial state under `initial`.
    fn new(device: &'d mut D, fonts: &'d F, initial: Matrix, len: usize) -> Self {
        let colorants = device.composites_colorants();
        Interpreter {
            device: Out {
                device,
                tape: Tape::default(),
            },
            fonts: Scope::Borrowed(fonts),
            defaults: fonts.device_defaults(),
            colorants,
            stack: Vec::new(),
            gs: GraphicsState::new(initial),
            saved: Vec::new(),
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
            path: Vec::new(),
            current: (0.0, 0.0),
            start: (0.0, 0.0),
            depth: 0,
            pending_clip: None,
            marked: 0,
            marked_over_cap: 0,
            streams: Vec::new(),
            group_runs: 0,
            group_budget_told: false,
            procedures: Vec::new(),
            recursion_told: false,
            reached_streams: BTreeSet::new(),
            reached: u64::try_from(len).unwrap_or(u64::MAX),
            nested_spent: 0,
            nested_budget_told: false,
            steps: Steps::default(),
            runs: Vec::new(),
        }
    }

    fn run(&mut self, content: &[u8]) {
        let mut tokens = Tokenizer::new(content);

        while let Some(token) = tokens.next_token() {
            if self.device.is_cancelled() {
                // A drawing cut short is never kept.
                self.device.tape.spoil();
                return;
            }
            self.steps.tokens = self.steps.tokens.saturating_add(1);

            let Token::Operator(op) = token else {
                self.stack.push(token);
                // A runaway operand list means a stream with no operators;
                // bound it rather than accumulate forever.
                if self.stack.len() > 512 {
                    self.stack.remove(0);
                }
                continue;
            };
            self.steps.operators = self.steps.operators.saturating_add(1);

            match op.as_slice() {
                b"BI" => {
                    // 8.9.7: an inline image's data is not tokenizable, so the
                    // span from here to `EI` is taken whole and handed to the
                    // device, which is the layer that can read a dictionary
                    // and run a filter chain.
                    let rest = tokens.rest();
                    let consumed = skip_inline_image(rest);
                    let at = tokens.position();
                    tokens.seek(at + consumed);

                    let span = rest.get(..consumed).unwrap_or(rest);
                    let (dict, data) = split_inline_image(span);
                    self.device.draw_image(
                        &ImageRef {
                            name: Vec::new(),
                            inline: true,
                            inline_dict: dict.to_vec(),
                            inline_data: data.to_vec(),
                        },
                        &self.gs,
                    );
                }
                _ => self.operator(&op),
            }
            self.stack.clear();
        }
    }

    fn num(&self, from_end: usize) -> Option<f64> {
        match self
            .stack
            .get(self.stack.len().checked_sub(from_end + 1)?)?
        {
            Token::Number(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }

    /// Reads `d`'s operands, a dash array and a phase (8.4.3.6), for
    /// [`Interpreter::set_dash`].
    fn set_dashes(&mut self) {
        let Some(phase) = self.num(0) else { return };

        // The array is the operand before the phase, and the tokenizer leaves
        // its elements on the stack between the brackets.
        let mut dashes: Vec<f64> = Vec::new();
        let mut seen_open = false;
        for token in self.stack.iter().rev().skip(1) {
            match token {
                Token::Number(v) => dashes.push(*v),
                Token::ArrayClose => {}
                Token::ArrayOpen => {
                    seen_open = true;
                    break;
                }
                _ => return,
            }
        }
        if !seen_open {
            return;
        }
        dashes.reverse();
        self.set_dash(dashes, phase);
    }

    // **One setter per applied parameter**, called by its operator and by `gs`
    // alike, so a dictionary and the operator it stands for cannot drift.

    /// `w`, `/LW`: a width that is finite and not negative (8.4.3.2).
    fn set_line_width(&mut self, width: f64) {
        if width.is_finite() && width >= 0.0 {
            self.gs.line_width = width;
        }
    }

    /// `J`, `/LC`: anything but 1 and 2 is a butt cap (8.4.3.3).
    fn set_line_cap(&mut self, cap: f64) {
        if cap.is_finite() {
            self.gs.line_cap = LineCap::from_operand(cap);
        }
    }

    /// `j`, `/LJ`: anything but 1 and 2 is a miter join (8.4.3.4).
    fn set_line_join(&mut self, join: f64) {
        if join.is_finite() {
            self.gs.line_join = LineJoin::from_operand(join);
        }
    }

    /// `M`, `/ML`. 8.4.3.5: a limit below 1 is meaningless — the ratio it
    /// bounds is never less than 1 — so it is ignored rather than turning
    /// every join into a bevel.
    fn set_miter_limit(&mut self, limit: f64) {
        if limit.is_finite() && limit >= 1.0 {
            self.gs.miter_limit = limit;
        }
    }

    /// `d`, `/D`. A negative or non-finite element or phase leaves the dash as
    /// it was; an array of all zeros is a solid line rather than an invisible
    /// one, since a stroke that vanishes reads as missing content.
    fn set_dash(&mut self, mut dashes: Vec<f64>, phase: f64) {
        let valid = |v: &f64| v.is_finite() && *v >= 0.0;
        if !valid(&phase) || !dashes.iter().all(valid) {
            return;
        }
        if dashes.iter().all(|v| *v == 0.0) {
            dashes.clear();
        }
        self.gs.dashes = dashes;
        self.gs.dash_phase = phase;
    }

    /// `Tf`, `/Font`: the text font by name, and its size (9.3.1).
    fn set_font(&mut self, name: Vec<u8>, size: f64) {
        if size.is_finite() {
            self.gs.text.font = Some(name);
            self.gs.text.size = size;
        }
    }

    /// `gs` (8.4.5): every parameter the dictionary sets, then its soft mask.
    ///
    /// The mask runs last, at the CTM in force (11.6.5.1), so its group runs
    /// under the state the `gs` made.
    fn apply_state(&mut self, set: &StateParameters) {
        if let Some(width) = set.line_width {
            self.set_line_width(width);
        }
        if let Some(cap) = set.line_cap {
            self.set_line_cap(cap);
        }
        if let Some(join) = set.line_join {
            self.set_line_join(join);
        }
        if let Some(limit) = set.miter_limit {
            self.set_miter_limit(limit);
        }
        if let Some((dashes, phase)) = &set.dash {
            self.set_dash(dashes.clone(), *phase);
        }
        if let Some((name, size)) = &set.font {
            self.set_font(name.clone(), *size);
        }
        if let Some(alpha) = set.fill_alpha {
            self.gs.fill_alpha = alpha.clamp(0.0, 1.0);
        }
        if let Some(alpha) = set.stroke_alpha {
            self.gs.stroke_alpha = alpha.clamp(0.0, 1.0);
        }
        if let Some(blend) = set.blend {
            self.gs.blend = blend;
        }
        let finite = |v: Option<f64>| v.filter(|v| v.is_finite());
        let before = self.intent();
        self.gs.set_stored(|s| {
            if let Some(intent) = set.rendering_intent {
                s.rendering_intent = intent;
            }
            if let Some(compensation) = set.black_point_compensation {
                s.black_point_compensation = compensation;
            }
            if let Some(overprint) = set.stroke_overprint {
                s.stroke_overprint = overprint;
            }
            if let Some(overprint) = set.fill_overprint {
                s.fill_overprint = overprint;
            }
            if let Some(mode) = set.overprint_mode {
                s.overprint_mode = mode;
            }
            if let Some(flatness) = finite(set.flatness) {
                s.flatness = Some(flatness);
            }
            if let Some(smoothness) = finite(set.smoothness) {
                s.smoothness = Some(smoothness);
            }
            if let Some(adjust) = set.stroke_adjustment {
                s.stroke_adjustment = Some(adjust);
            }
            if let Some(shape) = set.alpha_is_shape {
                s.alpha_is_shape = shape;
            }
            if let Some(knockout) = set.text_knockout {
                s.text_knockout = knockout;
            }
            if let Some(generation) = &set.black_generation {
                s.black_generation.clone_from(generation);
            }
            if let Some(removal) = &set.undercolor_removal {
                s.undercolor_removal.clone_from(removal);
            }
            if let Some(transfer) = &set.transfer {
                s.transfer.clone_from(transfer);
            }
            if let Some(halftone) = &set.halftone {
                s.halftone.clone_from(halftone);
            }
            if let Some(origin) = set.halftone_origin {
                if origin.iter().all(|v| v.is_finite()) {
                    s.halftone_origin = Some(origin);
                }
            }
        });
        // An `/RI` naming another intent paints the colours already set in it.
        self.intent_changed(before);
        // 11.6.5.1. The group is rendered *here*, at this CTM, rather than
        // recorded for the paint that will use it.
        match &set.soft_mask {
            Some(SoftMask::None) => self.device.clear_soft_mask(),
            Some(SoftMask::Group(mask)) => self.run_soft_mask(mask),
            None => {}
        }
    }

    fn operator(&mut self, op: &[u8]) {
        match op {
            // 8.4.4 graphics state
            b"q" => {
                if self.saved.len() < 64 {
                    self.saved.push(self.gs.clone());
                    self.device.save_state();
                }
            }
            b"Q" => {
                if let Some(prev) = self.saved.pop() {
                    self.gs = prev;
                    self.device.restore_state();
                }
            }
            b"cm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let m = Matrix { a, b, c, d, e, f };
                    let next = m.then(&self.gs.ctm);
                    // A degenerate matrix voids everything after it, so keep
                    // the previous one and carry on.
                    if next.is_finite() {
                        self.gs.ctm = next;
                    }
                }
            }

            // 9.4.1 text objects
            b"BT" => {
                self.text_matrix = Matrix::IDENTITY;
                self.line_matrix = Matrix::IDENTITY;
                self.set_text(true);
                self.device.begin_text();
            }
            b"ET" => self.device.end_text(),

            // 9.3 text state
            b"Tf" => {
                if let (Some(Token::Name(name)), Some(size)) = (
                    self.stack.get(self.stack.len().wrapping_sub(2)).cloned(),
                    self.num(0),
                ) {
                    self.set_font(name, size);
                }
            }
            b"Tc" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.char_spacing = v;
                }
            }
            b"Tw" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.word_spacing = v;
                }
            }
            b"Tz" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.horizontal_scale = v / 100.0;
                }
            }
            b"TL" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.leading = v;
                }
            }
            b"Ts" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.rise = v;
                }
            }
            b"Tr" => {
                if let Some(v) = self.num(0) {
                    self.gs.text.render_mode = TextRenderMode::from_i64(v as i64);
                }
            }

            // 9.4.2 text positioning
            b"Td" => {
                if let (Some(tx), Some(ty)) = (self.num(1), self.num(0)) {
                    self.read_line();
                    self.line_matrix = Matrix::translate(tx, ty).then(&self.line_matrix);
                    self.text_matrix = self.line_matrix;
                    self.set_text(true);
                }
            }
            b"TD" => {
                if let (Some(tx), Some(ty)) = (self.num(1), self.num(0)) {
                    self.gs.text.leading = -ty;
                    self.read_line();
                    self.line_matrix = Matrix::translate(tx, ty).then(&self.line_matrix);
                    self.text_matrix = self.line_matrix;
                    self.set_text(true);
                }
            }
            b"Tm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let m = Matrix { a, b, c, d, e, f };
                    if m.is_finite() {
                        self.line_matrix = m;
                        self.text_matrix = m;
                        self.set_text(true);
                    }
                }
            }
            b"T*" => self.next_line(),

            // 9.4.3 text showing
            b"Tj" => {
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"'" => {
                self.next_line();
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"\"" => {
                if let (Some(aw), Some(ac)) = (self.num(2), self.num(1)) {
                    self.gs.text.word_spacing = aw;
                    self.gs.text.char_spacing = ac;
                }
                self.next_line();
                if let Some(Token::String(s)) = self.stack.last().cloned() {
                    self.show(&s);
                }
            }
            b"TJ" => self.show_array(),

            // 8.5.2 path construction, accumulated in user space and
            // transformed as it is built.
            b"m" => {
                if let (Some(x), Some(y)) = (self.num(1), self.num(0)) {
                    let p = self.gs.ctm.apply(x, y);
                    self.current = p;
                    self.start = p;
                    self.set_point(true, true);
                    self.push_segment(PathSegment::MoveTo { x: p.0, y: p.1 });
                }
            }
            b"l" => {
                if let (Some(x), Some(y)) = (self.num(1), self.num(0)) {
                    let p = self.gs.ctm.apply(x, y);
                    self.current = p;
                    self.set_point(true, false);
                    self.push_segment(PathSegment::LineTo { x: p.0, y: p.1 });
                }
            }
            b"c" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x3), Some(y3)) = (
                    self.num(5),
                    self.num(4),
                    self.num(3),
                    self.num(2),
                    self.num(1),
                    self.num(0),
                ) {
                    let a = self.gs.ctm.apply(x1, y1);
                    let b = self.gs.ctm.apply(x2, y2);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.set_point(true, false);
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"v" => {
                if let (Some(x2), Some(y2), Some(x3), Some(y3)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    self.read_current();
                    let a = self.current;
                    let b = self.gs.ctm.apply(x2, y2);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.set_point(true, false);
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"y" => {
                if let (Some(x1), Some(y1), Some(x3), Some(y3)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let a = self.gs.ctm.apply(x1, y1);
                    let c = self.gs.ctm.apply(x3, y3);
                    self.current = c;
                    self.set_point(true, false);
                    self.push_segment(PathSegment::CurveTo {
                        x1: a.0,
                        y1: a.1,
                        x2: c.0,
                        y2: c.1,
                        x3: c.0,
                        y3: c.1,
                    });
                }
            }
            b"h" => {
                self.read_start();
                self.current = self.start;
                self.set_point(true, false);
                self.push_segment(PathSegment::Close);
            }
            b"re" => {
                if let (Some(x), Some(y), Some(w), Some(h)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let p0 = self.gs.ctm.apply(x, y);
                    let p1 = self.gs.ctm.apply(x + w, y);
                    let p2 = self.gs.ctm.apply(x + w, y + h);
                    let p3 = self.gs.ctm.apply(x, y + h);
                    self.push_segment(PathSegment::MoveTo { x: p0.0, y: p0.1 });
                    self.push_segment(PathSegment::LineTo { x: p1.0, y: p1.1 });
                    self.push_segment(PathSegment::LineTo { x: p2.0, y: p2.1 });
                    self.push_segment(PathSegment::LineTo { x: p3.0, y: p3.1 });
                    self.push_segment(PathSegment::Close);
                    self.current = p0;
                    self.start = p0;
                    self.set_point(true, true);
                }
            }

            // 8.5.3 path painting
            b"S" | b"s" => {
                if op == b"s" {
                    self.push_segment(PathSegment::Close);
                }
                let path = std::mem::take(&mut self.path);
                self.device.stroke_path(&path, &self.gs);
                self.apply_pending_clip(&path);
            }
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                let even_odd = op.ends_with(b"*");
                let path = std::mem::take(&mut self.path);
                self.device.fill_path(&path, &self.gs, even_odd);
                if matches!(op, b"B" | b"B*" | b"b" | b"b*") {
                    self.device.stroke_path(&path, &self.gs);
                }
                self.apply_pending_clip(&path);
            }
            b"n" => {
                let path = std::mem::take(&mut self.path);
                self.apply_pending_clip(&path);
            }
            // 8.5.4: W and W* mark the current path as the new clip, but it
            // does not take effect until the painting operator that ends the
            // path — which is why this only records the intent.
            b"W" => self.pending_clip = Some(false),
            b"W*" => self.pending_clip = Some(true),

            // 8.10 XObjects
            b"Do" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    self.do_xobject(&name);
                }
            }

            // 8.4.3 line parameters. All of them reach the device: a stroke
            // drawn with the wrong cap, or without its dashes, is wrong in a
            // way that looks deliberate.
            b"w" => {
                if let Some(v) = self.num(0) {
                    self.set_line_width(v);
                }
            }
            b"J" => {
                if let Some(v) = self.num(0) {
                    self.set_line_cap(v);
                }
            }
            b"j" => {
                if let Some(v) = self.num(0) {
                    self.set_line_join(v);
                }
            }
            b"M" => {
                if let Some(v) = self.num(0) {
                    self.set_miter_limit(v);
                }
            }
            b"d" => self.set_dashes(),

            // 8.6.5.8 and 10.6.2: kept for the rows that apply them, as `/RI`
            // and `/FL` are.
            b"ri" => {
                if let Some(Token::Name(name)) = self.stack.last() {
                    let before = self.intent();
                    let intent = RenderingIntent::from_name(name);
                    self.gs.set_stored(|s| s.rendering_intent = intent);
                    self.intent_changed(before);
                }
            }
            b"i" => {
                if let Some(v) = self.num(0) {
                    self.gs.set_stored(|s| s.flatness = Some(v));
                }
            }

            // 8.6.8 colour. The device operators resolve immediately, unless
            // the scope remaps their space to a default (8.6.5.6); the named
            // ones go through the resource seam.
            b"g" | b"G" => {
                if let Some(v) = self.num(0) {
                    let remapped = if self.defaults.gray {
                        self.remapped(b"DeviceGray", vec![v])
                    } else {
                        None
                    };
                    let family = if remapped.is_some() {
                        self.fonts.color_family(b"DeviceGray")
                    } else {
                        ColorFamily::Gray
                    };
                    let (color, wide, ink, source, colorants) = remapped.unwrap_or_else(|| {
                        let value = to_byte(v);
                        let color = Rgb {
                            r: value,
                            g: value,
                            b: value,
                        };
                        let wide = device_wide(&ColorSpace::DeviceGray, &[v]);
                        (color, wide, None, None, None)
                    });
                    self.set_device_color(op == b"g", color, wide, ink, source, family, colorants);
                }
            }
            b"rg" | b"RG" => {
                if let (Some(r), Some(g), Some(b)) = (self.num(2), self.num(1), self.num(0)) {
                    let remapped = if self.defaults.rgb {
                        self.remapped(b"DeviceRGB", vec![r, g, b])
                    } else {
                        None
                    };
                    let family = if remapped.is_some() {
                        self.fonts.color_family(b"DeviceRGB")
                    } else {
                        ColorFamily::Rgb
                    };
                    let (color, wide, ink, source, colorants) = remapped.unwrap_or_else(|| {
                        let color = Rgb {
                            r: to_byte(r),
                            g: to_byte(g),
                            b: to_byte(b),
                        };
                        let wide = device_wide(&ColorSpace::DeviceRgb, &[r, g, b]);
                        (color, wide, None, None, None)
                    });
                    self.set_device_color(op == b"rg", color, wide, ink, source, family, colorants);
                }
            }
            b"k" | b"K" => {
                if let (Some(c), Some(m), Some(y), Some(k)) =
                    (self.num(3), self.num(2), self.num(1), self.num(0))
                {
                    let remapped = if self.defaults.cmyk {
                        self.remapped(b"DeviceCMYK", vec![c, m, y, k])
                    } else {
                        None
                    };
                    let family = if remapped.is_some() {
                        self.fonts.color_family(b"DeviceCMYK")
                    } else {
                        ColorFamily::Cmyk
                    };
                    let (color, wide, ink, source, colorants) = remapped.unwrap_or_else(|| {
                        let ink = [to_byte(c), to_byte(m), to_byte(y), to_byte(k)];
                        (
                            cmyk_to_rgb(c, m, y, k),
                            device_wide(&ColorSpace::DeviceCmyk, &[c, m, y, k]),
                            Some(ink),
                            None,
                            self.device_cmyk_colorants(ink),
                        )
                    });
                    self.set_device_color(op == b"k", color, wide, ink, source, family, colorants);
                }
            }
            b"cs" | b"CS" => {
                let Some(Token::Name(space)) = self.stack.last().cloned() else {
                    return;
                };
                let fill = op == b"cs";
                // 8.6.8: selecting a space resets the colour to that space's
                // initial value, which is black in every device space — and
                // DeviceCMYK's black is `0 0 0 1`, which an all-zeros reset
                // made white — and the whole colorant, a tint of 1.0, in a
                // `/Separation` or `/DeviceN` one, which it made none.
                let components = self.fonts.initial_color(&space);
                let intent = self.intent();
                let resolved = components
                    .as_ref()
                    .and_then(|c| self.fonts.resolve_color_wide(&space, c, intent));
                let ink = components
                    .as_ref()
                    .and_then(|c| self.fonts.resolve_ink(&space, c));
                // And what that colour paints by colorant: a `/Separation`'s
                // whole colorant, a `/DeviceN`'s every one (8.6.6.4, 8.6.6.5).
                let colorants = components
                    .as_ref()
                    .and_then(|c| self.colorants_of(&space, c));
                // The colour is the seam's, so an intent changing later asks
                // it again; a colour the seam could not resolve stays black.
                let source = resolved.and(components).map(|components| {
                    Arc::new(ColorSource {
                        family: None,
                        components,
                    })
                });
                let (initial, wide) = resolved.unwrap_or((Rgb::BLACK, Rgb16::BLACK));
                let family = self.fonts.color_family(&space);
                self.set_color(
                    fill,
                    initial,
                    wide,
                    Some(space),
                    ink,
                    source,
                    Some(family),
                    colorants,
                );
            }
            b"sc" | b"SC" | b"scn" | b"SCN" => {
                let fill = op == b"sc" || op == b"scn";
                let components: Vec<f64> = self
                    .stack
                    .iter()
                    .filter_map(|t| match t {
                        Token::Number(v) if v.is_finite() => Some(*v),
                        _ => None,
                    })
                    .collect();
                let space = if fill {
                    self.gs.fill_space.clone()
                } else {
                    self.gs.stroke_space.clone()
                };

                // 8.6.8.2: `scn` may end with a pattern name, with or without
                // preceding components (an uncoloured pattern takes both).
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    // 8.7.3.2: those preceding components are the colour a
                    // `PaintType 2` pattern paints with — it supplies shape
                    // and nothing else — and they are measured in the
                    // underlying space of `[/Pattern base]`, which is the
                    // space this slot is already in. Dropping them, which is
                    // what happened before, left the slot holding whatever
                    // `cs` had reset it to and the pattern with no colour to
                    // take.
                    if !components.is_empty() {
                        let intent = self.intent();
                        if let Some((color, wide)) = space
                            .as_ref()
                            .and_then(|s| self.fonts.resolve_color_wide(s, &components, intent))
                        {
                            // An uncoloured pattern paints the colorants its
                            // components give in the underlying space.
                            let colorants = space
                                .as_ref()
                                .and_then(|s| self.colorants_of(s, &components));
                            let source = Arc::new(ColorSource {
                                family: None,
                                components,
                            });
                            self.set_color(
                                fill,
                                color,
                                wide,
                                None,
                                None,
                                Some(source),
                                None,
                                colorants,
                            );
                        }
                    }
                    self.set_pattern(fill, name);
                    return;
                }
                if components.is_empty() {
                    return;
                }

                // A named space keeps the family `cs` gave it; a reading by
                // count is in the family the count names, or its default's.
                let mut family = None;
                let (resolved, ink, source, colorants) = match &space {
                    Some(space) => {
                        let intent = self.intent();
                        let color = self.fonts.resolve_color_wide(space, &components, intent);
                        let ink = self.fonts.resolve_ink(space, &components);
                        let colorants = self.colorants_of(space, &components);
                        let source = Arc::new(ColorSource {
                            family: None,
                            components,
                        });
                        (color, ink, Some(source), colorants)
                    }
                    // Without a named space the component count is the only
                    // clue, and it is a reliable one: four is DeviceCMYK. A
                    // scope that remaps the family the count reads in is
                    // asked by that device space's name (8.6.5.6).
                    None => {
                        let remapped = match self.defaults.remapped_by_count(components.len()) {
                            Some(name) => {
                                family = Some(self.fonts.color_family(name));
                                self.remapped(name, components.clone())
                            }
                            None => None,
                        };
                        if remapped.is_none() {
                            family = Some(ColorFamily::by_count(components.len()));
                        }
                        match remapped {
                            Some((color, wide, ink, source, colorants)) => {
                                (Some((color, wide)), ink, source, colorants)
                            }
                            None => {
                                let ink = match components[..] {
                                    [c, m, y, k] => {
                                        Some([to_byte(c), to_byte(m), to_byte(y), to_byte(k)])
                                    }
                                    _ => None,
                                };
                                let wide = components_to_wide(&components);
                                (
                                    Some((components_to_rgb(&components), wide)),
                                    ink,
                                    None,
                                    ink.and_then(|ink| self.device_cmyk_colorants(ink)),
                                )
                            }
                        }
                    }
                };
                if let Some((color, wide)) = resolved {
                    self.set_color(fill, color, wide, space, ink, source, family, colorants);
                }
            }

            // 8.4.5 external graphics state, read once by the seam.
            b"gs" => {
                if let Some(Token::Name(name)) = self.stack.last() {
                    if let Some(set) = self.fonts.ext_g_state(name) {
                        self.apply_state(&set);
                    }
                }
            }

            // 8.7.4.2: `sh` paints a shading over the current clip.
            b"sh" => {
                if let Some(Token::Name(name)) = self.stack.last().cloned() {
                    self.device.draw_shading(&name, &self.gs);
                }
            }

            // 14.6.2 marked content, and 8.11.3.2's use of it for optional
            // content. `BDC` is the only operator in the language that can
            // stop a page drawing, so it is the only one that looks at its
            // property list here.
            b"BDC" => {
                let (tag, properties) = self.bdc_operands();
                let (layer, props) = match &properties {
                    // 8.11.3.2: only the `/OC` tag selects optional content,
                    // and only the name form of the list can reach a group.
                    Properties::Named(name) => {
                        let layer = (tag.as_slice() == b"OC")
                            .then(|| self.fonts.optional_content(name))
                            .flatten();
                        (layer, self.fonts.marked_content_properties(name))
                    }
                    Properties::Inline(props) => (None, Some(props.clone())),
                    Properties::None => (None, None),
                };
                // 14.7.4.2: the sequence resides in the stream its `BDC` was
                // written in, and only the interpreter knows which that is.
                // Stamped after `is_empty`, so a list holding nothing but the
                // stream identity is still no list at all.
                let props = props.filter(|p| !p.is_empty()).map(|mut props| {
                    props.stream = self.stream();
                    props
                });
                self.open_marked_content(&tag, layer, props.as_ref());
            }
            // 14.6.1: `BMC` has a tag and no property list, so it can name
            // no layer, carries no `/MCID`, and always paints.
            b"BMC" => {
                let tag = match self.stack.last() {
                    Some(Token::Name(tag)) => tag.clone(),
                    _ => Vec::new(),
                };
                self.open_marked_content(&tag, None, None);
            }
            b"EMC" => self.close_marked_content(),
            // 14.6.1's marked-content *points*, which mark a position rather
            // than open a scope, and 7.8.2's compatibility brackets. All
            // four do nothing here, and are named rather than left to the
            // arm below so that a reader can see the difference between `MP`
            // and `BMC` was considered: reading `DP` as a scope would leave
            // the device inside a layer with no `EMC` ever to close it.
            b"MP" | b"DP" | b"BX" | b"EX" => {}

            _ => {}
        }
    }

    /// A `BDC`'s two operands: its tag, and its property list (14.6.2).
    ///
    /// `BDC` takes `tag properties`, and the property list is either a name
    /// into the page's `/Properties` sub-dictionary or an inline dictionary
    /// written out in the stream.
    ///
    /// # The inline scan is back, and now it changes answers
    ///
    /// This reassembles 7.8.2's flattened `DictOpen`/`DictClose` tokens. An
    /// earlier build wrote that scan, deleted it, and recorded the deletion
    /// here: a deliberate defect injection could not make it change a single
    /// answer, because the only thing then read from a property list was
    /// `/OC`, and an inline list can never name a layer — 8.11.3.2 makes an
    /// `/OC` entry a reference to a group or a membership dictionary, and
    /// 7.3.10 puts indirect references in the file structure, where a content
    /// stream cannot write one. Both the scan and the two-token peek that
    /// replaced it produced a visible scope for every inline list, well
    /// formed or not, so the scan was dead weight and the note said so.
    ///
    /// **That argument stopped holding the moment `/MCID` was read from the
    /// same list.** 14.7.4.2 puts `/MCID` in the property list itself, not
    /// behind a reference, and `/P << /MCID 3 >> BDC` is the form nearly
    /// every tagging producer emits — so without the reassembly a tagged page
    /// has no marked content this engine can see at all. The 14.9 values
    /// beside it (`/ActualText`, `/Alt`, `/Lang`, `/E`) are text strings,
    /// which a content stream can also write inline, and they are read here
    /// for the same reason.
    ///
    /// The old note was right about a second thing, and the scan inherits it:
    /// a list this cannot read yields **no** properties and a **visible**
    /// scope. Nothing here can hide content.
    ///
    /// It also closes a defect the two-token peek carried. The tag sits
    /// before the `<<`, and peeking one token below the top of the stack
    /// finds whatever the dictionary's *last value* happened to be — so
    /// `/OC << /Type /OCMD >> BDC` reported its tag as `OCMD`, and
    /// `/Artifact << /Type /Pagination >> BDC` reported `Pagination` rather
    /// than `Artifact`. The text device excludes artifacts by that tag
    /// (14.8.2.2), so an artifact written with an inline property list was
    /// extracted as though it were the author's content. Finding the `<<` is
    /// what fixes it, and only the reassembly can find the `<<`.
    fn bdc_operands(&self) -> (Vec<u8>, Properties) {
        match self.stack.last() {
            // `/Tag /MC0 BDC`: the list is a name, and the tag is below it.
            Some(Token::Name(property)) => {
                let tag = match self.stack.iter().rev().nth(1) {
                    Some(Token::Name(tag)) => tag.clone(),
                    _ => Vec::new(),
                };
                (tag, Properties::Named(property.clone()))
            }
            // `/Tag << … >> BDC`: the tokenizer left the dictionary flat.
            Some(Token::DictClose) => {
                let Some(open) = self.inline_dict_start() else {
                    // A `>>` with no `<<` before it is damage, not a list.
                    return (Vec::new(), Properties::None);
                };
                let tag = match open.checked_sub(1).and_then(|at| self.stack.get(at)) {
                    Some(Token::Name(tag)) => tag.clone(),
                    _ => Vec::new(),
                };
                let body = self
                    .stack
                    .get(open + 1..self.stack.len() - 1)
                    .unwrap_or_default();
                (tag, Properties::Inline(read_inline_properties(body)))
            }
            // No property list at all, which `BDC` requires. The scope still
            // opens — 14.6.2's `EMC` will arrive for it either way — and it
            // is nameless rather than guessed at.
            _ => (Vec::new(), Properties::None),
        }
    }

    /// The index of the `<<` matching the `>>` on top of the stack.
    ///
    /// Scanned backwards with a depth counter, so a dictionary nested inside
    /// the list does not end the search early. `None` when the stack holds no
    /// matching `<<` — which happens both for genuine damage and for a list
    /// so long that [`Interpreter::run`]'s operand bound dropped its opening
    /// brace, and both mean "no properties, visible scope".
    fn inline_dict_start(&self) -> Option<usize> {
        let mut depth = 0usize;
        for (at, token) in self
            .stack
            .iter()
            .enumerate()
            .rev()
            .take(MAX_INLINE_PROPERTY_TOKENS)
        {
            match token {
                Token::DictClose => depth += 1,
                Token::DictOpen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(at);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Which content stream is being run (14.7.4.2).
    ///
    /// The **innermost** form, not the outermost: 14.7.4.2 puts a sequence in
    /// the stream its `BDC` was written in, and a form invoked from inside
    /// another form writes its own. Reporting the outer one would give two
    /// nested forms one identity and put the collision this key exists to
    /// break back exactly where it was.
    fn stream(&self) -> u64 {
        self.streams.last().copied().unwrap_or(0)
    }

    /// Opens a marked-content scope and tells the device whether to paint
    /// what follows.
    fn open_marked_content(
        &mut self,
        tag: &[u8],
        layer: Option<Layer>,
        props: Option<&MarkedProps>,
    ) {
        if self.marked >= MAX_MARKED_CONTENT_DEPTH {
            self.marked_over_cap = self.marked_over_cap.saturating_add(1);
            return;
        }
        self.marked += 1;
        match layer {
            Some(layer) if !layer.visible => {
                self.device
                    .begin_marked_content(tag, false, Some(&layer.label), props);
            }
            _ => self.device.begin_marked_content(tag, true, None, props),
        }
    }

    /// Closes the innermost scope, if there is one.
    ///
    /// An `EMC` with nothing open is damage, and is dropped. Acting on it
    /// would let a form XObject — or a stray one halfway down a page — close
    /// a scope it never opened, which reaches out of the stream that wrote
    /// it and un-hides a layer somewhere else entirely.
    fn close_marked_content(&mut self) {
        if self.marked_over_cap > 0 {
            self.marked_over_cap -= 1;
            return;
        }
        if self.marked > 0 {
            self.marked -= 1;
            self.device.end_marked_content();
        }
    }

    /// Closes every scope the stream just run left open.
    fn close_open_marked_content(&mut self) {
        self.marked_over_cap = 0;
        while self.marked > 0 {
            self.marked -= 1;
            self.device.end_marked_content();
        }
    }

    /// Closes every `q` the stream just run left open (8.4.2), so that a
    /// nested stream — a form, a soft mask's group, a Type 3 glyph's
    /// procedure — cannot leave its caller inside one.
    ///
    /// Each nested stream runs with the `q` stack taken from its caller, so
    /// what is on it here is the stream's own. The device is told: one that
    /// keeps a single stack for the states `q` saves and the ones a form
    /// saves would otherwise pop the form's for the `q`'s, and go on clipping
    /// what its caller draws after it by the form's `/BBox`.
    fn close_open_saves(&mut self) {
        while self.saved.pop().is_some() {
            self.device.restore_state();
        }
    }

    /// Whether the page's group budget has a run left ([`MAX_GROUP_RUNS`]),
    /// telling the device, the first time it has none.
    fn group_run_left(&mut self) -> bool {
        if self.group_runs < MAX_GROUP_RUNS {
            self.device.tape.spend(Spent::GroupCheck);
            return true;
        }
        // A refusal depends on what ran before the form, not on its key.
        self.device.tape.spoil();
        if !self.group_budget_told {
            self.group_budget_told = true;
            self.device.group_budget_spent();
        }
        false
    }

    /// A group's or a mask's group's content runs, spending one of
    /// [`MAX_GROUP_RUNS`].
    fn run_group(&mut self) {
        self.group_runs += 1;
        self.device.tape.spend(Spent::GroupRun);
    }

    /// A depth past [`MAX_FORM_DEPTH`] refused a nested stream: what a form
    /// around it drew depends on how deep it was invoked.
    fn too_deep(&mut self) {
        self.device.tape.spoil();
    }

    /// Whether `stream` is already running in the ancestry of what runs now —
    /// a form, a soft mask's group or a Type 3 glyph procedure that names
    /// itself, directly or through others — telling the device, the first
    /// time (8.10). A stream without an identity is never one.
    ///
    /// A cycle has no last run, so the depth cap was all that ended one, and a
    /// cycle that branches is two or more runs to the sixteenth inside it:
    /// refused at its first repeat, it draws once.
    fn recursing(&mut self, stream: u64) -> bool {
        if stream == 0 || !(self.streams.contains(&stream) || self.procedures.contains(&stream)) {
            return false;
        }
        self.device.tape.spoil();
        if !self.recursion_told {
            self.recursion_told = true;
            self.device.recursion_refused();
        }
        true
    }

    /// Whether the nested-content budget ([`NESTED_WORK_FLOOR`]) has room
    /// for a run of `len` bytes of `stream`, counting the stream into what
    /// the interpretation has reached the first time it is asked about, and
    /// telling the device, the first time it has none.
    fn nested_work_left(&mut self, stream: u64, len: usize) -> bool {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        if stream != 0 && self.reached_streams.insert(stream) {
            self.reached = self.reached.saturating_add(len);
        }
        if nested_fits(self.nested_spent, self.reached, len) {
            self.device.tape.spend(Spent::NestedCheck { stream, len });
            return true;
        }
        // A refusal depends on what ran before the form, not on its key.
        self.device.tape.spoil();
        if !self.nested_budget_told {
            self.nested_budget_told = true;
            self.device.nested_budget_spent();
        }
        false
    }

    /// Spends a run of `len` bytes from the nested-content budget.
    fn spend_nested_work(&mut self, len: usize) {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        self.nested_spent = nested_after(self.nested_spent, len);
        self.device.tape.spend(Spent::NestedSpend { len });
    }

    /// Hands a recorded `W`/`W*` to the device, now that the path has ended.
    fn apply_pending_clip(&mut self, path: &[PathSegment]) {
        if let Some(even_odd) = self.pending_clip.take() {
            self.device.clip_path(path, &self.gs, even_odd);
        }
    }

    /// What a device operator's colour is in a scope that remaps its space
    /// (8.6.5.6): asked of the seam by the device space's own name, as
    /// `/DeviceGray cs … sc` is, so the light is the default's and the ink is
    /// what the space as selected keeps. `None` where the seam cannot say,
    /// and the operator converts as 8.6.4 does.
    ///
    /// Out of line and cold, because it is asked only in a scope that names a
    /// default, so the seam's two answers stay out of the operator arms that
    /// every page runs.
    ///
    /// The colour comes back with what it was resolved from, so an intent
    /// changing later resolves it again in the default.
    #[cold]
    #[inline(never)]
    fn remapped(&self, space: &'static [u8], components: Vec<f64>) -> Option<Resolved> {
        let (color, wide) = self
            .fonts
            .resolve_color_wide(space, &components, self.intent())?;
        let ink = self.fonts.resolve_ink(space, &components);
        let colorants = self.colorants_of(space, &components);
        let source = Arc::new(ColorSource {
            family: Some(space),
            components,
        });
        Some((color, wide, ink, Some(source), colorants))
    }

    /// What the seam answers for a colour of `space` by colorant, in the
    /// intent in force, asked only where the device composites colorants.
    fn colorants_of(&self, space: &[u8], components: &[f64]) -> Option<Arc<ColorantTints>> {
        if !self.colorants {
            return None;
        }
        self.fonts.colorants(space, components, self.intent())
    }

    /// A DeviceCMYK colour's own components on the four process colorants,
    /// where the device composites colorants.
    fn device_cmyk_colorants(&self, ink: [u8; 4]) -> Option<Arc<ColorantTints>> {
        self.colorants
            .then(|| Arc::new(ColorantTints::device_cmyk(ink)))
    }

    /// The rendering intent in force (8.6.5.8).
    fn intent(&self) -> RenderingIntent {
        self.gs.stored_parameters().rendering_intent
    }

    /// Resolves the fill and stroke colours again where the rendering intent
    /// in force is no longer `before` (11.7.5.3): a colour is converted when
    /// it is set, and an intent that changes after it was set — by `ri`, or by
    /// a `gs` whose `/RI` names another — paints it in the new one. A colour
    /// a device operator converted in place keeps it, since no intent changes
    /// it, and so does one the seam cannot resolve again; the ink and the
    /// pattern a slot holds are the space's and stay as they are. Setting the
    /// intent in force again resolves nothing.
    fn intent_changed(&mut self, before: RenderingIntent) {
        let intent = self.intent();
        if intent == before {
            return;
        }
        for fill in [true, false] {
            let (source, named) = if fill {
                (self.gs.fill_source.clone(), &self.gs.fill_space)
            } else {
                (self.gs.stroke_source.clone(), &self.gs.stroke_space)
            };
            let Some(source) = source else {
                continue;
            };
            let space: &[u8] = match (source.family, named) {
                (Some(family), _) => family,
                (None, Some(named)) => named,
                (None, None) => continue,
            };
            let Some((color, wide)) =
                self.fonts
                    .resolve_color_wide(space, &source.components, intent)
            else {
                continue;
            };
            // The process ink a colour paints through a light alternate is
            // that light's, so its colorants are asked again with it.
            let colorants = if self.colorants {
                Some(self.fonts.colorants(space, &source.components, intent))
            } else {
                None
            };
            if fill {
                self.gs.fill_color = color;
                self.gs.fill_wide = wide;
                if let Some(colorants) = colorants {
                    self.gs.fill_colorants = colorants;
                }
            } else {
                self.gs.stroke_color = color;
                self.gs.stroke_wide = wide;
                if let Some(colorants) = colorants {
                    self.gs.stroke_colorants = colorants;
                }
            }
        }
    }

    /// Applies the colour a device operator sets, and selects the operator's
    /// device space with it.
    ///
    /// 8.6.8 has `g`, `rg` and `k` set the current colour space to DeviceGray,
    /// DeviceRGB or DeviceCMYK, so the slot cannot go on naming whatever `cs`
    /// selected last — `/CS1 cs 1 sc 0.5 g 0.7 sc` read the `0.7` in `/CS1`
    /// until October 2026. It holds no name instead, which is the reading by
    /// component count: the operands `sc` gives next are read in the family
    /// the operator selected, and allocate nothing.
    #[allow(clippy::too_many_arguments)]
    fn set_device_color(
        &mut self,
        fill: bool,
        color: Rgb,
        wide: Rgb16,
        ink: Option<[u8; 4]>,
        source: Option<Arc<ColorSource>>,
        family: ColorFamily,
        colorants: Option<Arc<ColorantTints>>,
    ) {
        self.set_color(
            fill,
            color,
            wide,
            None,
            ink,
            source,
            Some(family),
            colorants,
        );
        if fill {
            self.gs.fill_space = None;
        } else {
            self.gs.stroke_space = None;
        }
    }

    /// Applies a colour to the fill or stroke slot, as bytes and at sixteen
    /// bits, with its DeviceCMYK components when it has them, what it was
    /// resolved from where the seam resolved it, its family where the
    /// operator says one (`None` keeps the slot's), and what it paints by
    /// colorant where the device asked.
    #[allow(clippy::too_many_arguments)]
    fn set_color(
        &mut self,
        fill: bool,
        color: Rgb,
        wide: Rgb16,
        space: Option<Vec<u8>>,
        ink: Option<[u8; 4]>,
        source: Option<Arc<ColorSource>>,
        family: Option<ColorFamily>,
        colorants: Option<Arc<ColorantTints>>,
    ) {
        if fill {
            self.gs.fill_color = color;
            self.gs.fill_wide = wide;
            self.gs.fill_ink = ink;
            self.gs.fill_colorants = colorants;
            self.gs.fill_pattern = None;
            self.gs.fill_source = source;
            if let Some(space) = space {
                self.gs.fill_space = Some(space);
            }
            if let Some(family) = family {
                self.gs.fill_family = family;
            }
        } else {
            self.gs.stroke_color = color;
            self.gs.stroke_wide = wide;
            self.gs.stroke_ink = ink;
            self.gs.stroke_colorants = colorants;
            self.gs.stroke_pattern = None;
            self.gs.stroke_source = source;
            if let Some(space) = space {
                self.gs.stroke_space = Some(space);
            }
            if let Some(family) = family {
                self.gs.stroke_family = family;
            }
        }
    }

    /// Records the pattern a `scn`/`SCN` selected (8.7.3).
    fn set_pattern(&mut self, fill: bool, name: Vec<u8>) {
        if fill {
            self.gs.fill_pattern = Some(name);
        } else {
            self.gs.stroke_pattern = Some(name);
        }
    }

    fn push_segment(&mut self, segment: PathSegment) {
        // A page can describe an unbounded path; cap it so one hostile stream
        // cannot exhaust memory.
        if self.path.len() < 1 << 20 {
            self.path.push(segment);
        }
    }

    fn next_line(&mut self) {
        let leading = self.gs.text.leading;
        self.read_line();
        self.line_matrix = Matrix::translate(0.0, -leading).then(&self.line_matrix);
        self.text_matrix = self.line_matrix;
        self.set_text(true);
    }

    fn do_xobject(&mut self, name: &[u8]) {
        // 8.11.4.4: an `/OC` entry on a form or image XObject hides the whole
        // XObject, and lives on the XObject's own dictionary rather than in
        // `/Properties`. It is expressed here as a marked-content scope
        // around the invocation, so the engine has one suppression mechanism
        // and not two: a hidden image is left undrawn for exactly the reason
        // a hidden rectangle is, and a hidden *form* is still run, so its
        // text still extracts and its state changes still happen.
        let hidden = match self.fonts.xobject_optional_content(name) {
            Some(layer) if !layer.visible => {
                self.open_marked_content(b"OC", Some(layer), None);
                true
            }
            _ => false,
        };
        self.run_xobject(name);
        if hidden {
            self.close_marked_content();
        }
    }

    fn run_xobject(&mut self, name: &[u8]) {
        let Some(form) = self.fonts.form(name) else {
            self.device.draw_image(
                &ImageRef {
                    name: name.to_vec(),
                    inline: false,
                    inline_dict: Vec::new(),
                    inline_data: Vec::new(),
                },
                &self.gs,
            );
            return;
        };

        if self.depth >= MAX_FORM_DEPTH {
            self.too_deep();
            return;
        }
        // 8.10: a form that names itself while it runs — directly, or through
        // other forms, masks or glyphs — draws once.
        if self.recursing(form.stream) {
            return;
        }
        // A group form spends from the page's group budget whether or not the
        // device holds the group, because its content runs either way.
        let group_form = form.group.is_some();
        if group_form && !self.group_run_left() {
            return;
        }
        if !self.nested_work_left(form.stream, form.content.len()) {
            return;
        }
        let id = self.fonts.font_id(name);
        // 8.10.1: a form may bring its own resources, and the device is asked
        // for the same form by the same name so that both seams change scope
        // together.
        let nested = self.fonts.form_scope(name);
        if !self.device.begin_form(id, name) {
            return;
        }
        if group_form {
            self.run_group();
        }
        self.spend_nested_work(form.content.len());
        // A source that keeps forms' drawings is asked for this one's, and
        // offered it where it has none (`FontSource::form_drawn`).
        match self.fonts.form_recording_bytes() {
            0 => self.draw_form(&form, nested),
            limit => self.draw_kept_form(name, &form, nested, limit),
        }
        self.device.end_form(id);
    }

    /// Draws a form the device entered: its scope, its state saved, its box
    /// clipped, its group offered, its content run, and everything restored
    /// but the current point, the subpath's start and a clip waiting for a
    /// path, which a form leaves where it left them.
    fn draw_form(&mut self, form: &Form, nested: Option<Arc<F>>) {
        let outer = nested.map(|scope| mem::replace(&mut self.fonts, Scope::Owned(scope)));
        // 8.6.5.6 reads a default in "the current resource dictionary", which
        // inside a form with resources of its own is those: the page's
        // defaults stop at it, and its own apply inside it only.
        let outer_defaults = outer
            .is_some()
            .then(|| mem::replace(&mut self.defaults, self.fonts.device_defaults()));

        // 8.10.2: a form's /Matrix maps its space into the current one, and
        // its content runs with the surrounding state saved.
        let saved_gs = self.gs.clone();
        let saved_stack = std::mem::take(&mut self.saved);
        let saved_path = std::mem::take(&mut self.path);
        let saved_text = self.text_matrix;
        let saved_line = self.line_matrix;
        self.runs.push(RunMarks::default());
        // 14.6.2: marked content nests within one stream. A form that leaves
        // a scope open must not leave its caller inside it, and an `EMC` it
        // has no `BDC` for must not close one of the caller's — so the count
        // starts at zero here and is emptied below.
        let saved_marked = std::mem::replace(&mut self.marked, 0);
        let saved_over_cap = std::mem::replace(&mut self.marked_over_cap, 0);
        // 14.7.4.2: an `/MCID` numbers a sequence within *this* stream, so
        // the form's own identity is what the sequences it opens are keyed
        // on. A form the resource layer reached without a reference pushes
        // nothing and keeps the caller's identity, which is the answer this
        // engine gave before there was a stack at all.
        let pushed_stream = form.stream != 0;
        if pushed_stream {
            self.streams.push(form.stream);
        }

        let combined = form.matrix.then(&self.gs.ctm);
        if combined.is_finite() {
            self.gs.ctm = combined;
        }

        // 8.10.2: the form is clipped to its bounding box, which is expressed
        // in form space — so with `/Matrix` already folded into the CTM, the
        // rectangle goes in unmodified. Without this a form that draws
        // outside its box paints across the rest of the page, and forms are
        // how most producers place repeated content, so it is not a rare
        // shape. `begin_form` saved the clip and `end_form` restores it.
        if let Some(box_path) = self.bbox_quad(form.bbox) {
            self.device.clip_path(&box_path, &self.gs, false);
        }

        // 11.6.6: a transparency group's contents are composited into a buffer
        // of their own and the result painted as one object, so the alphas and
        // the blend mode in force at the `Do` belong to the *result*. Inside
        // the group they are reset — without this a group with `ca 0.5` fades
        // each element separately and two overlapping opaque shapes inside it
        // show a seam where they cross, which is the observable difference
        // between per-element alpha and group alpha.
        //
        // The soft mask goes the same way, and the device does that half: it
        // is pixels, so it lives there (ruling 8).
        let grouped = match form.group {
            Some(group) if self.device.begin_group(group, &self.gs) => {
                self.gs.fill_alpha = 1.0;
                self.gs.stroke_alpha = 1.0;
                self.gs.blend = crate::state::BlendMode::Normal;
                true
            }
            _ => false,
        };

        self.depth += 1;
        self.run(&form.content);
        self.depth -= 1;

        self.close_open_marked_content();
        self.close_open_saves();
        if grouped {
            self.device.end_group();
        }
        if pushed_stream {
            self.streams.pop();
        }
        self.marked = saved_marked;
        self.marked_over_cap = saved_over_cap;
        self.gs = saved_gs;
        self.saved = saved_stack;
        self.path = saved_path;
        self.text_matrix = saved_text;
        self.line_matrix = saved_line;
        self.runs.pop();
        if let Some(outer) = outer {
            self.fonts = outer;
        }
        if let Some(outer) = outer_defaults {
            self.defaults = outer;
        }
    }

    /// Draws a form the device entered for a source that keeps forms'
    /// drawings ([`FontSource::kept_form`]): from the recording kept under
    /// this invocation's key, where there is one this device and these
    /// budgets replay as it was made; by interpreting it, recording what it
    /// draws on the tape and offering the recording, where there is none;
    /// and by interpreting it alone where the invocation has no key or the
    /// recording kept will not replay here. `limit` is the most a recording
    /// may hold.
    fn draw_kept_form(&mut self, name: &[u8], form: &Form, nested: Option<Arc<F>>, limit: usize) {
        let Some(invocation) = self.form_invocation(form) else {
            self.draw_form(form, nested);
            self.fonts.form_drawn(name, FormDrawn::Interpreted);
            return;
        };
        match self.fonts.kept_form(name, &invocation) {
            Some(kept) if self.replays(&kept) => {
                self.replay_form(&kept);
                self.fonts.form_drawn(name, FormDrawn::Replayed);
            }
            Some(_) => {
                self.draw_form(form, nested);
                self.fonts.form_drawn(name, FormDrawn::Interpreted);
            }
            None => {
                let invoked = After {
                    current: self.current,
                    start: self.start,
                    pending_clip: self.pending_clip,
                    set_current: false,
                    set_start: false,
                };
                let run = self.runs.len();
                self.device
                    .tape
                    .open(limit, run, invoked, self.text_matrix, self.line_matrix);
                self.draw_form(form, nested);
                let after = After {
                    current: self.current,
                    start: self.start,
                    pending_clip: self.pending_clip,
                    set_current: false,
                    set_start: false,
                };
                let recording = self.device.tape.close(after);
                self.fonts.form_drawn(
                    name,
                    FormDrawn::Recorded {
                        invocation,
                        recording,
                    },
                );
            }
        }
    }

    /// The key a recording of `form`'s drawing is kept under here
    /// ([`FormInvocation`]), or `None` where the form has no identity or the
    /// state holds what no key writes: a stored function or halftone.
    fn form_invocation(&self, form: &Form) -> Option<FormInvocation> {
        if form.stream == 0 {
            return None;
        }
        let mut out = Exact(Vec::with_capacity(512));
        out.word(u64::from(self.depth));
        // A drawing recorded for a device compositing colorants carries them
        // in every state, and one recorded for any other device carries none.
        out.flag(self.colorants);
        out.flag(self.defaults.gray);
        out.flag(self.defaults.rgb);
        out.flag(self.defaults.cmyk);
        out.0.push(match self.pending_clip {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        out.word(self.stack.len() as u64);
        for token in &self.stack {
            out.token(token);
        }
        out.word(self.streams.len() as u64);
        for stream in &self.streams {
            out.word(*stream);
        }
        out.word(self.procedures.len() as u64);
        for procedure in &self.procedures {
            out.word(*procedure);
        }
        out.state(&self.gs)?;
        Some(FormInvocation {
            stream: form.stream,
            state: out.0,
        })
    }

    /// Whether `kept` replays here as interpreting the form would draw: the
    /// device would give every answer it records, and what it spent of the
    /// budgets for nested content fits what is left of them now.
    fn replays(&self, kept: &FormRecording) -> bool {
        let point = |point: (f64, f64)| (point.0.to_bits(), point.1.to_bits());
        let matrix = |m: &Matrix| [m.a, m.b, m.c, m.d, m.e, m.f].map(f64::to_bits);
        let needs = &kept.needs;
        if needs
            .text
            .is_some_and(|text| matrix(&text) != matrix(&self.text_matrix))
            || needs
                .line
                .is_some_and(|line| matrix(&line) != matrix(&self.line_matrix))
            || needs
                .current
                .is_some_and(|current| point(current) != point(self.current))
            || needs
                .start
                .is_some_and(|start| point(start) != point(self.start))
        {
            return false;
        }
        if !self.device.answers_alike(&kept.events) {
            return false;
        }
        let mut group_runs = self.group_runs;
        let mut reached = self.reached;
        let mut nested = self.nested_spent;
        let mut first: BTreeSet<u64> = BTreeSet::new();
        for spent in &kept.spent {
            match *spent {
                Spent::GroupCheck => {
                    if group_runs >= MAX_GROUP_RUNS {
                        return false;
                    }
                }
                Spent::GroupRun => group_runs = group_runs.saturating_add(1),
                Spent::NestedCheck { stream, len } => {
                    if stream != 0
                        && !self.reached_streams.contains(&stream)
                        && first.insert(stream)
                    {
                        reached = reached.saturating_add(len);
                    }
                    if !nested_fits(nested, reached, len) {
                        return false;
                    }
                }
                Spent::NestedSpend { len } => nested = nested_after(nested, len),
            }
        }
        true
    }

    /// Draws `kept` in place of interpreting its form: its calls replayed to
    /// the device, its budget operations spent again, and the current point,
    /// the subpath's start and a waiting clip left where its drawing left
    /// them.
    fn replay_form(&mut self, kept: &FormRecording) {
        let done = replay(&kept.events, &mut self.device);
        if done.cancelled || done.disagreed > 0 {
            self.device.tape.spoil();
        }
        for spent in &kept.spent {
            match *spent {
                Spent::GroupCheck => {}
                Spent::GroupRun => self.group_runs = self.group_runs.saturating_add(1),
                Spent::NestedCheck { stream, len } => {
                    if stream != 0 && self.reached_streams.insert(stream) {
                        self.reached = self.reached.saturating_add(len);
                    }
                }
                Spent::NestedSpend { len } => {
                    self.nested_spent = nested_after(self.nested_spent, len);
                }
            }
            // A recording open around this form spends what this one did.
            self.device.tape.spend(*spent);
        }
        // What the drawing read is read here too, for a recording open around
        // this form; what it set of the point is set, and its matrices are
        // the form's own, restored after it.
        if kept.needs.text.is_some() {
            self.read_text();
        }
        if kept.needs.line.is_some() {
            self.read_line();
        }
        if kept.needs.current.is_some() {
            self.read_current();
        }
        if kept.needs.start.is_some() {
            self.read_start();
        }
        self.set_point(kept.after.set_current, kept.after.set_start);
        self.current = kept.after.current;
        self.start = kept.after.start;
        self.pending_clip = kept.after.pending_clip;
    }

    /// The text matrix is read: by a recording whose form's run, and every
    /// run inside it, has not set it, as the value its form was invoked with.
    fn read_text(&mut self) {
        let runs = &self.runs;
        for opened in &mut self.device.tape.open {
            if opened.needs.text.is_none()
                && runs
                    .get(opened.run..)
                    .is_some_and(|inside| inside.iter().all(|run| !run.text))
            {
                opened.needs.text = Some(opened.text);
            }
        }
    }

    /// The line matrix is read, as [`Interpreter::read_text`] reads the text
    /// matrix.
    fn read_line(&mut self) {
        let runs = &self.runs;
        for opened in &mut self.device.tape.open {
            if opened.needs.line.is_none()
                && runs
                    .get(opened.run..)
                    .is_some_and(|inside| inside.iter().all(|run| !run.line))
            {
                opened.needs.line = Some(opened.line);
            }
        }
    }

    /// The text matrix, and the line matrix where `line`, are set in the run
    /// in force.
    fn set_text(&mut self, line: bool) {
        if let Some(run) = self.runs.last_mut() {
            run.text = true;
            run.line |= line;
        }
    }

    /// The current point is read: by every recording since whose form began
    /// nothing has set it, as the point its form was invoked with. The point
    /// is not restored after a nested stream, so a recording's is set
    /// wherever it is set inside it.
    fn read_current(&mut self) {
        for opened in &mut self.device.tape.open {
            if opened.current_kept && opened.needs.current.is_none() {
                opened.needs.current = Some(opened.invoked.current);
            }
        }
    }

    /// The subpath's start is read, as [`Interpreter::read_current`] reads
    /// the current point.
    fn read_start(&mut self) {
        for opened in &mut self.device.tape.open {
            if opened.start_kept && opened.needs.start.is_none() {
                opened.needs.start = Some(opened.invoked.start);
            }
        }
    }

    /// The current point, and the subpath's start, are set where `current`
    /// and `start` say.
    fn set_point(&mut self, current: bool, start: bool) {
        for opened in &mut self.device.tape.open {
            opened.current_kept &= !current;
            opened.start_kept &= !start;
        }
    }

    /// A form's `/BBox` as a quad in the space a `Device` receives paths in.
    ///
    /// A `Device` receives paths already transformed by the CTM — `re` and
    /// friends apply it as they build them — so the corners are transformed
    /// here too. Passing the rectangle in form space instead clips somewhere
    /// else entirely, which with an identity `/Matrix` happens to look almost
    /// right and with any other matrix erases the form completely.
    ///
    /// Shared between the clip a form installs (8.10.2) and the bound a soft
    /// mask's group gives its buffer (11.6.5.2), because two spellings of the
    /// same rectangle is how the clip and the buffer come to disagree about
    /// where the mask is.
    fn bbox_quad(&self, bbox: Option<[f64; 4]>) -> Option<[PathSegment; 5]> {
        let [x0, y0, x1, y1] = bbox?;
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) || x1 <= x0 || y1 <= y0 {
            return None;
        }
        let p0 = self.gs.ctm.apply(x0, y0);
        let p1 = self.gs.ctm.apply(x1, y0);
        let p2 = self.gs.ctm.apply(x1, y1);
        let p3 = self.gs.ctm.apply(x0, y1);
        Some([
            PathSegment::MoveTo { x: p0.0, y: p0.1 },
            PathSegment::LineTo { x: p1.0, y: p1.1 },
            PathSegment::LineTo { x: p2.0, y: p2.1 },
            PathSegment::LineTo { x: p3.0, y: p3.1 },
            PathSegment::Close,
        ])
    }

    /// Renders an ExtGState `/SMask`'s group, now, at this CTM (11.6.5.2).
    ///
    /// **Now** is the whole point. The mask group is rendered with the
    /// transform in force at the `gs` operator, not with the one in force
    /// when the masked content is painted. The two are almost always
    /// different — a `gs` is set once and the content it masks is drawn under
    /// its own `cm` — and getting it wrong puts the shadow a few points from
    /// where it belongs, which looks like a slightly misplaced shadow rather
    /// than like a bug.
    fn run_soft_mask(&mut self, mask: &MaskGroup) {
        if self.depth >= MAX_FORM_DEPTH {
            self.too_deep();
            return;
        }
        // A mask group that sets its own mask while it runs — the shape of a
        // `/G` naming the page whose `gs` sets it — runs once.
        if self.recursing(mask.form.stream) {
            return;
        }
        // The page's group budget: a mask group that opens other masks opens
        // more each time it runs, and a branching walk is bounded by a budget
        // rather than a depth (`MAX_GROUP_RUNS`).
        if !self.group_run_left() {
            return;
        }
        if !self.nested_work_left(mask.form.stream, mask.form.content.len()) {
            return;
        }

        // The state the group runs under, saved exactly as a form's is: the
        // mask group is a content stream and may do anything one can.
        let saved_gs = self.gs.clone();
        let saved_stack = std::mem::take(&mut self.saved);
        let saved_path = std::mem::take(&mut self.path);
        let saved_text = self.text_matrix;
        let saved_line = self.line_matrix;
        self.runs.push(RunMarks::default());
        let saved_marked = std::mem::replace(&mut self.marked, 0);
        let saved_over_cap = std::mem::replace(&mut self.marked_over_cap, 0);
        // 11.6.5.2's `/G` is a form XObject like any other, so 14.7.4.2's
        // per-stream numbering applies to it the same way.
        let pushed_stream = mask.form.stream != 0;
        if pushed_stream {
            self.streams.push(mask.form.stream);
        }

        // 8.10.2's `/Matrix`, composed onto the CTM at the `gs`.
        let combined = mask.form.matrix.then(&self.gs.ctm);
        if combined.is_finite() {
            self.gs.ctm = combined;
        }
        let bbox = self.bbox_quad(mask.form.bbox);

        let accepted = self.device.begin_soft_mask(
            mask,
            bbox.as_ref().map_or(&[], |b| b.as_slice()),
            &self.gs,
        );
        if accepted {
            self.run_group();
            self.spend_nested_work(mask.form.content.len());
            // 8.10.2 still clips the group to its box, and 11.6.6 still
            // resets the alphas and the blend mode inside it — a mask group
            // is a transparency group, whatever `/G`'s own `/Group` says.
            if let Some(box_path) = bbox {
                self.device.clip_path(&box_path, &self.gs, false);
            }
            self.gs.fill_alpha = 1.0;
            self.gs.stroke_alpha = 1.0;
            self.gs.blend = crate::state::BlendMode::Normal;

            self.depth += 1;
            self.run(&mask.form.content);
            self.depth -= 1;
            self.close_open_marked_content();
            self.close_open_saves();
            self.device.end_soft_mask();
        }

        if pushed_stream {
            self.streams.pop();
        }
        self.marked = saved_marked;
        self.marked_over_cap = saved_over_cap;
        self.gs = saved_gs;
        self.saved = saved_stack;
        self.path = saved_path;
        self.text_matrix = saved_text;
        self.line_matrix = saved_line;
        self.runs.pop();
    }

    fn show_array(&mut self) {
        // TJ's operand is an array of strings and numbers; the numbers move
        // the pen backwards in thousandths of text space (9.4.3).
        let items: Vec<Token> = self
            .stack
            .iter()
            .rev()
            .take_while(|t| !matches!(t, Token::ArrayOpen))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // 9.4.3: the adjustment is subtracted from "the current horizontal or
        // vertical coordinate, depending on the writing mode" — so which axis
        // it moves along is a property of the font, not of the operator. A
        // vertical run whose adjustments went sideways would step out of its
        // own column, one kern at a time. `Tf` cannot appear inside a `TJ`
        // array, so the font is fixed for the whole of it.
        let vertical = self
            .gs
            .text
            .font
            .as_ref()
            .is_some_and(|name| self.fonts.is_vertical(name));

        for item in items {
            match item {
                Token::String(s) => self.show(&s),
                Token::Number(adjust) if adjust.is_finite() => {
                    self.read_text();
                    self.set_text(false);
                    let ts = &self.gs.text;
                    // 9.4.4's tx carries Th and its ty does not: Th is a
                    // horizontal scaling and there is no horizontal motion to
                    // scale in a vertical run.
                    self.text_matrix = if vertical {
                        let shift = -adjust / 1000.0 * ts.size;
                        Matrix::translate(0.0, shift).then(&self.text_matrix)
                    } else {
                        let shift = -adjust / 1000.0 * ts.size * ts.horizontal_scale;
                        Matrix::translate(shift, 0.0).then(&self.text_matrix)
                    };
                }
                _ => {}
            }
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        let Some(font_name) = self.gs.text.font.clone() else {
            return;
        };
        let vertical = self.fonts.is_vertical(&font_name);
        let font_id = self.fonts.font_id(&font_name);
        let font_label = self.fonts.font_name(&font_name);

        for (code, text, width) in self.fonts.decode(&font_name, bytes) {
            let ts = self.gs.text.clone();

            // 9.4.4: the glyph is placed by the text matrix scaled by the font
            // size, horizontal scaling and rise.
            let scale = Matrix {
                a: ts.size * ts.horizontal_scale,
                b: 0.0,
                c: 0.0,
                d: ts.size,
                e: 0.0,
                f: ts.rise,
            };

            // 9.7.4.3: a vertical font displaces the pen by w1 rather than by
            // the horizontal w0, and draws the glyph at the current point
            // offset by *minus* the position vector v — v points from the
            // glyph's horizontal origin to the one vertical setting uses, so
            // undoing it is what puts the glyph in the column. Both come out
            // of one call, because a build where the advance and the placement
            // read different numbers is the failure 9.7.4.3 is easiest to get
            // half right about.
            let (v_x, v_y, w1_y) = if vertical {
                self.fonts.vertical_metrics(&font_name, code)
            } else {
                (0.0, 0.0, 0.0)
            };

            // v is in glyph space, so the offset goes *inside* the size
            // scaling — 9.4.4 sends the position vector through Trm like every
            // other glyph-space coordinate, which is what makes it scale with
            // the font size and with Th.
            let placed = if vertical {
                Matrix::translate(-v_x / 1000.0, -v_y / 1000.0).then(&scale)
            } else {
                scale
            };
            self.read_text();
            let transform = placed.then(&self.text_matrix).then(&self.gs.ctm);

            // The same point with 9.4.3's rise taken out, for a consumer that
            // has to decide which line this glyph belongs to. Computed only
            // when there is a rise, so the ordinary glyph pays one comparison
            // and no matrix multiplication: `Ts` is rare and a page is
            // millions of glyphs. See [`Glyph::baseline`].
            let baseline = if ts.rise == 0.0 {
                None
            } else {
                let flat = Matrix { f: 0.0, ..scale };
                let flat = if vertical {
                    Matrix::translate(-v_x / 1000.0, -v_y / 1000.0).then(&flat)
                } else {
                    flat
                };
                let at = flat.then(&self.text_matrix).then(&self.gs.ctm);
                Some((at.e, at.f))
            };

            // 9.6.5: a Type 3 glyph is a content stream. Its procedure runs in
            // glyph space, which the font matrix maps into text space — so the
            // matrix goes *inside* the transform that places the glyph, not
            // outside it.
            if let Some(glyph) = self.fonts.type3_glyph(&font_name, code) {
                let font_matrix = glyph.matrix;
                // A procedure that shows its own glyph while it runs, directly
                // or through other glyphs and forms, draws once; and every run
                // spends from the nested-content budget.
                let deep = self.depth >= MAX_FORM_DEPTH;
                if deep {
                    self.too_deep();
                }
                if !deep
                    && !self.recursing(glyph.stream)
                    && self.nested_work_left(glyph.stream, glyph.content.len())
                {
                    self.spend_nested_work(glyph.content.len());
                    let pushed = glyph.stream != 0;
                    if pushed {
                        self.procedures.push(glyph.stream);
                    }
                    let saved_gs = self.gs.clone();
                    let saved_text = self.text_matrix;
                    let saved_line = self.line_matrix;
                    self.runs.push(RunMarks::default());
                    // A glyph procedure is a content stream like a form's,
                    // and 14.6.2's scopes are per stream in the same way.
                    let saved_marked = std::mem::replace(&mut self.marked, 0);
                    let saved_over_cap = std::mem::replace(&mut self.marked_over_cap, 0);
                    // And 8.4.2's `q` stack is its own too: a `Q` the
                    // procedure has no `q` for must not restore the state of
                    // the text that shows the glyph, and a `q` it leaves open
                    // is closed before the device's own save is.
                    let saved_stack = std::mem::take(&mut self.saved);

                    self.gs.ctm = font_matrix.then(&transform);
                    self.depth += 1;
                    self.device.save_state();
                    self.run(&glyph.content);
                    self.close_open_marked_content();
                    self.close_open_saves();
                    self.device.restore_state();
                    self.depth -= 1;

                    self.saved = saved_stack;
                    self.marked = saved_marked;
                    self.marked_over_cap = saved_over_cap;
                    self.gs = saved_gs;
                    self.text_matrix = saved_text;
                    self.line_matrix = saved_line;
                    self.runs.pop();
                    if pushed {
                        self.procedures.pop();
                    }
                }

                // The advance still comes from /Widths, which are in glyph
                // space and so scale by the font matrix rather than by 1/1000.
                let advance = width * font_matrix.a * ts.size * ts.horizontal_scale;
                let shift =
                    advance + (ts.char_spacing + word_spacing(code, &ts)) * ts.horizontal_scale;
                self.text_matrix = Matrix::translate(shift, 0.0).then(&self.text_matrix);
                self.set_text(false);
                continue;
            }

            let glyph = Glyph {
                code,
                text: text.clone(),
                transform,
                baseline,
                advance: if vertical { w1_y } else { width } / 1000.0 * ts.size,
                size: ts.size,
                vertical,
                font_id,
                font_name: font_label.clone(),
            };
            self.device.show_glyph(&glyph, &self.gs);

            // 9.4.4: word spacing applies to a single-byte code 32 only, which
            // is why it must not be added for a two-byte CID font.
            let word = if code == 32 && !vertical {
                ts.word_spacing
            } else {
                0.0
            };

            // 9.4.4, in full:
            //   tx = ((w0 - Tj/1000) * Tfs + Tc + Tw) * Th
            //   ty =  (w1 - Tj/1000) * Tfs + Tc + Tw
            // Two differences, and both of them matter. The displacement is w1
            // and not w0 — a downward number, already signed, so it is added
            // rather than subtracted and an engine that negates a negative
            // runs the column upward. And Th does not appear in ty at all:
            // horizontal scaling scales horizontal motion, of which a vertical
            // run has none.
            self.text_matrix = if vertical {
                let ty = w1_y / 1000.0 * ts.size + ts.char_spacing + word;
                Matrix::translate(0.0, ty).then(&self.text_matrix)
            } else {
                let tx = (width / 1000.0 * ts.size + ts.char_spacing + word) * ts.horizontal_scale;
                Matrix::translate(tx, 0.0).then(&self.text_matrix)
            };
            self.set_text(false);
        }
    }
}

fn to_byte(value: f64) -> u8 {
    if !value.is_finite() {
        return 0;
    }
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 8.6.4.4: the additive complement, with black applied.
fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Rgb {
    let k = k.clamp(0.0, 1.0);
    Rgb {
        r: to_byte((1.0 - c.clamp(0.0, 1.0)) * (1.0 - k)),
        g: to_byte((1.0 - m.clamp(0.0, 1.0)) * (1.0 - k)),
        b: to_byte((1.0 - y.clamp(0.0, 1.0)) * (1.0 - k)),
    }
}

/// A device colour at sixteen bits a channel (`docs/design/sixteen-bit.md`):
/// the colour crate's relation for `space`, whose top bytes are the bytes
/// [`to_byte`] and [`cmyk_to_rgb`] give the same components.
fn device_wide(space: &ColorSpace, components: &[f64]) -> Rgb16 {
    Rgb16::from(space.to_rgb16(components))
}

/// [`components_to_rgb`] at sixteen bits a channel, the components read by
/// their count as it reads them.
fn components_to_wide(components: &[f64]) -> Rgb16 {
    let space = match components.len() {
        1 => ColorSpace::DeviceGray,
        4 => ColorSpace::DeviceCmyk,
        _ => ColorSpace::DeviceRgb,
    };
    device_wide(&space, components)
}

/// Reads components as a device colour, by how many there are.
///
/// `sc` without a preceding `cs` is malformed but occurs; the count is the
/// only signal available and it is an unambiguous one.
fn components_to_rgb(components: &[f64]) -> Rgb {
    let at = |i: usize| components.get(i).copied().unwrap_or(0.0);
    match components.len() {
        1 => {
            let v = to_byte(at(0));
            Rgb { r: v, g: v, b: v }
        }
        4 => cmyk_to_rgb(at(0), at(1), at(2), at(3)),
        _ => Rgb {
            r: to_byte(at(0)),
            g: to_byte(at(1)),
            b: to_byte(at(2)),
        },
    }
}

/// Finds the end of an inline image, returning how many bytes to skip.
///
/// 8.9.7 ends the data at `EI` surrounded by whitespace, and binary data can
/// contain those bytes, so the match is only accepted when what follows looks
/// like a content stream again.
/// 9.4.4: word spacing applies to a single-byte code 32 and to nothing else.
fn word_spacing(code: u32, ts: &crate::state::TextState) -> f64 {
    if code == 32 {
        ts.word_spacing
    } else {
        0.0
    }
}

/// Splits an inline image's span into its dictionary and its data (8.9.7).
///
/// `ID` is followed by exactly one whitespace byte before the data starts, and
/// the span ends at `EI`. A span with no `ID` at all is all dictionary and no
/// data, which is what a truncated stream leaves behind.
///
/// The whitespace byte *before* `EI` belongs to the syntax too, and is dropped
/// for the same reason 7.3.8.2 drops the end-of-line before `endstream`. It
/// costs nothing on unfiltered samples, whose length the dictionary already
/// gives — but on a filtered image it is one byte past the end of a complete
/// zlib or LZW stream, which every decoder here reports as trailing garbage.
fn split_inline_image(span: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 0usize;
    while i + 1 < span.len() {
        if span[i] == b'I' && span[i + 1] == b'D' {
            let before_ok =
                i == 0 || span[i - 1].is_ascii_whitespace() || is_delimiter(span[i - 1]);
            if before_ok {
                let dict = &span[..i];
                // One whitespace byte separates `ID` from the samples.
                let start = (i + 3).min(span.len());
                let mut end = span.len().saturating_sub(2).max(start);
                if end > start && span.get(end - 1).is_some_and(is_inline_space) {
                    end -= 1;
                }
                return (dict, &span[start..end]);
            }
        }
        i += 1;
    }
    (span, &[])
}

/// The white space 8.9.7 allows between an inline image's data and `EI`.
///
/// 7.2.3's set, which includes NUL — the byte `skip_inline_image` already
/// accepts in that position.
fn is_inline_space(b: &u8) -> bool {
    b.is_ascii_whitespace() || *b == 0
}

/// How many bytes of `rest` — what follows a `BI` operator — the inline
/// image takes, through its `EI` (8.9.7).
///
/// Public for the other reader of content streams that must not tokenize an
/// inline image's samples: the facade's redaction, which rewrites a stream
/// token by token and would otherwise re-serialize the samples as whatever
/// tokens they happened to spell. One answer to where an inline image ends,
/// so the interpreter and the rewrite cannot disagree about it.
#[must_use]
pub fn skip_inline_image(rest: &[u8]) -> usize {
    let mut i = 0usize;
    while i + 1 < rest.len() {
        if rest.get(i) == Some(&b'E') && rest.get(i + 1) == Some(&b'I') {
            let before_ok = i == 0
                || rest
                    .get(i - 1)
                    .is_some_and(|b| b.is_ascii_whitespace() || *b == 0);
            let after = rest.get(i + 2);
            let after_ok = after.is_none_or(|b| b.is_ascii_whitespace() || is_delimiter(*b));
            if before_ok && after_ok {
                return i + 2;
            }
        }
        i += 1;
    }
    rest.len()
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[cfg(test)]
mod tests {
    //! These tests used to carry **five** devices of their own — `Recorder`,
    //! `GroupEvents`, `Painted`, `Scopes` and `Images` — each recording the
    //! subset one test needed, with heavy overlap and no two agreeing on what
    //! an event was. All five are gone; every test below drives
    //! [`crate::record::RecordingDevice`], which keeps every call in order.
    //!
    //! What each of them asserted is unchanged. What moved is where the
    //! *derived* answers are computed: the enclosing visibility of a scope was
    //! a field `Scopes` maintained, and is now
    //! [`RecordingDevice::hidden_at`](crate::record::RecordingDevice::hidden_at)
    //! over the scopes the recorder kept — so a test here still fails for the
    //! same reason a real drawing device would.

    use super::*;
    use crate::record::{Answers, Event, EventKind, RecordingDevice};
    use crate::state::{ColorantKind, ColorantTint};

    /// Every byte is one code, 500/1000 em wide, mapping to itself.
    struct Simple;

    impl FontSource for Simple {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        /// Never vertical, so nothing here is ever asked. Written out because
        /// the trait has no default for it: 9.7.4.3's metrics have no safe
        /// one, and a source that grows a vertical font is made to say so.
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
    }

    // -----------------------------------------------------------------------
    // Transparency groups (11.6.6).
    // -----------------------------------------------------------------------

    /// A source offering one grouped form and one plain one, both drawing a
    /// rectangle.
    struct Groups {
        group: Option<Group>,
    }

    impl FontSource for Groups {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            (name == b"Fm").then(|| Form {
                content: b"0 0 5 5 re f".to_vec(),
                matrix: Matrix::IDENTITY,
                bbox: None,
                group: self.group,
                stream: 0,
            })
        }
        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            (name == b"Half").then(|| {
                Arc::new(StateParameters {
                    fill_alpha: Some(0.5),
                    stroke_alpha: Some(0.25),
                    blend: Some(BlendMode::Multiply),
                    ..StateParameters::default()
                })
            })
        }
    }

    /// The attributes of every transparency group the stream offered, in
    /// order.
    fn groups_offered(d: &RecordingDevice) -> Vec<Group> {
        d.of_kind(EventKind::BeginGroup)
            .filter_map(|event| match event {
                Event::BeginGroup { group, .. } => Some(*group),
                _ => None,
            })
            .collect()
    }

    /// The `ca`, `CA` and blend mode every event of `kind` saw.
    ///
    /// The three that 11.6.6 resets inside an accepted group, read back off
    /// the state the recorder copied **at each call** — which is what makes
    /// the group's own state and its contents' distinguishable at all.
    fn alphas(d: &RecordingDevice, kind: EventKind) -> Vec<(f64, f64, crate::state::BlendMode)> {
        d.of_kind(kind)
            .map(|event| {
                let state = event.state().expect("the state was captured");
                (state.fill_alpha, state.stroke_alpha, state.blend)
            })
            .collect()
    }

    /// 11.6.6: the alphas and the blend mode at the `Do` belong to the group's
    /// result, so inside the content they are reset. Without this a group at
    /// `ca 0.5` fades every element separately *and* the result again.
    #[test]
    fn a_transparency_group_resets_the_alphas_and_the_blend_mode_inside_it() {
        let mut device = RecordingDevice::new().answering(Answers {
            groups: true,
            ..Answers::default()
        });
        let fonts = Groups {
            group: Some(Group {
                isolated: true,
                knockout: false,
                space: None,
            }),
        };
        interpret(b"/Half gs /Fm Do", Matrix::IDENTITY, &mut device, &fonts);

        assert_eq!(
            groups_offered(&device),
            vec![Group {
                isolated: true,
                knockout: false,
                space: None,
            }],
            "the group's own attributes reach the device"
        );
        assert_eq!(
            alphas(&device, EventKind::BeginGroup),
            vec![(0.5, 0.25, crate::state::BlendMode::Multiply)],
            "and it is handed the state at the `Do`, which is the group's own"
        );
        assert_eq!(
            alphas(&device, EventKind::FillPath),
            vec![(1.0, 1.0, crate::state::BlendMode::Normal)],
            "while the content inside runs at full strength under Normal"
        );
        assert_eq!(
            device.count(EventKind::EndGroup),
            1,
            "and the group is closed exactly once"
        );
    }

    /// A device that declines the group must not have the state reset
    /// underneath it — the text device and the recorders keep no buffer, and
    /// content that was painting at `ca 0.5` would start painting at full
    /// strength with nothing left to fade it.
    #[test]
    fn declining_a_group_leaves_the_state_alone() {
        let mut device = RecordingDevice::new();
        let fonts = Groups {
            group: Some(Group::default()),
        };
        interpret(b"/Half gs /Fm Do", Matrix::IDENTITY, &mut device, &fonts);

        assert_eq!(
            device.count(EventKind::BeginGroup),
            1,
            "it was still offered"
        );
        assert_eq!(
            device.count(EventKind::EndGroup),
            0,
            "and not closed, having never opened"
        );
        assert_eq!(
            alphas(&device, EventKind::FillPath),
            vec![(0.5, 0.25, crate::state::BlendMode::Multiply)],
            "the content keeps the alphas the page set"
        );
    }

    /// A source of nested streams: `/F` a plain form with a box whose
    /// content a test gives, `/K` a transparency group that draws itself
    /// twice, `/M` a graphics state whose soft mask's group sets `/M` twice,
    /// and every code of the font `/T3` a Type 3 glyph whose procedure a test
    /// gives.
    struct Nested {
        form: &'static [u8],
        glyph: &'static [u8],
    }

    impl FontSource for Nested {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            let (content, group) = match name {
                b"F" => (self.form.to_vec(), None),
                b"K" => (b"/K Do /K Do".to_vec(), Some(Group::default())),
                _ => return None,
            };
            Some(Form {
                content,
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 10.0, 10.0]),
                group,
                stream: 0,
            })
        }
        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            (name == b"M").then(|| {
                Arc::new(StateParameters {
                    soft_mask: Some(SoftMask::Group(Box::new(MaskGroup {
                        form: Form {
                            content: b"/M gs /M gs".to_vec(),
                            matrix: Matrix::IDENTITY,
                            bbox: None,
                            group: None,
                            stream: 0,
                        },
                        luminosity: true,
                        backdrop: None,
                        transfer: None,
                    }))),
                    ..StateParameters::default()
                })
            })
        }
        fn type3_glyph(&self, font: &[u8], _code: u32) -> Option<Type3Glyph> {
            (font == b"T3").then(|| Type3Glyph {
                content: self.glyph.to_vec(),
                matrix: Matrix {
                    a: 0.001,
                    d: 0.001,
                    ..Matrix::IDENTITY
                },
                stream: 0,
            })
        }
    }

    /// The calls that save and restore state, and the forms and paints
    /// between them, in order.
    fn saves(d: &RecordingDevice) -> Vec<EventKind> {
        d.kinds()
            .into_iter()
            .filter(|kind| {
                matches!(
                    kind,
                    EventKind::BeginForm
                        | EventKind::EndForm
                        | EventKind::SaveState
                        | EventKind::RestoreState
                        | EventKind::FillPath
                )
            })
            .collect()
    }

    /// 8.4.2: a nested stream's `q`s are its own. The ones a form leaves open
    /// are closed before the form ends, so a device that keeps one stack for
    /// the states `q` saves and the ones a form saves pops the form's own at
    /// its end and stops clipping by its box; the page's own are closed when
    /// the page ends, before anything drawn after it.
    #[test]
    fn a_nested_stream_cannot_leave_its_caller_inside_a_q() {
        use EventKind::{BeginForm, EndForm, FillPath, RestoreState, SaveState};
        let fonts = Nested {
            form: b"q 0 0 5 5 re W n q",
            glyph: b"",
        };
        let mut device = RecordingDevice::new();
        interpret(
            b"/F Do 0 0 20 20 re f q",
            Matrix::IDENTITY,
            &mut device,
            &fonts,
        );
        assert_eq!(
            saves(&device),
            [
                BeginForm,
                SaveState,
                SaveState,
                RestoreState,
                RestoreState,
                EndForm,
                FillPath,
                SaveState,
                RestoreState,
            ],
            "every save is restored inside the stream that made it"
        );
    }

    /// A `Q` a Type 3 glyph's procedure has no `q` for restores nothing of
    /// the text that shows the glyph: the page's own `q` around the text is
    /// still the one its `Q` closes, so what the page fills after it is
    /// black again rather than the grey the `q` saved over.
    #[test]
    fn a_glyph_procedure_cannot_close_the_text_s_q() {
        let fonts = Nested {
            form: b"",
            glyph: b"Q Q 0 0 1000 1000 re f",
        };
        let mut device = RecordingDevice::new();
        interpret(
            b"q 0.5 g BT /T3 10 Tf (a) Tj ET Q 0 0 20 20 re f",
            Matrix::IDENTITY,
            &mut device,
            &fonts,
        );
        let fills: Vec<Rgb> = device
            .of_kind(EventKind::FillPath)
            .map(|event| event.state().expect("the state was captured").fill_color)
            .collect();
        let black = Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(fills.len(), 2, "the glyph's square and the page's");
        assert_eq!(
            fills[1], black,
            "the page's `Q` restored the black its `q` saved: {fills:?}"
        );
        assert_eq!(
            device.count(EventKind::SaveState),
            device.count(EventKind::RestoreState),
            "and the device's saves balance"
        );
    }

    /// A group that draws itself twice, with no identity to be refused as a
    /// cycle by, is two to the sixteenth runs inside the form depth. The
    /// page's group budget stops the walk at [`MAX_GROUP_RUNS`] whether the
    /// device holds groups or declines them — a declined group still runs as
    /// content — and tells the device once.
    #[test]
    fn a_group_that_draws_itself_spends_the_page_group_budget() {
        for groups in [false, true] {
            let mut device = RecordingDevice::new().answering(Answers {
                groups,
                ..Answers::default()
            });
            let fonts = Nested {
                form: b"",
                glyph: b"",
            };
            interpret(b"/K Do", Matrix::IDENTITY, &mut device, &fonts);
            assert_eq!(
                device.count(EventKind::BeginForm),
                MAX_GROUP_RUNS as usize,
                "groups accepted: {groups}"
            );
            assert_eq!(device.count(EventKind::GroupBudgetSpent), 1);
        }
    }

    /// The same for a soft mask whose group sets the mask again: each run
    /// opens two more. A mask spends only when the device accepts it, since
    /// only then is its group run.
    #[test]
    fn a_soft_mask_that_sets_itself_spends_the_page_group_budget() {
        let fonts = Nested {
            form: b"",
            glyph: b"",
        };
        let mut accepting = RecordingDevice::new().answering(Answers {
            soft_masks: true,
            ..Answers::default()
        });
        interpret(b"/M gs", Matrix::IDENTITY, &mut accepting, &fonts);
        assert_eq!(
            accepting.count(EventKind::BeginSoftMask),
            MAX_GROUP_RUNS as usize
        );
        assert_eq!(accepting.count(EventKind::GroupBudgetSpent), 1);

        let mut declining = RecordingDevice::new();
        interpret(b"/M gs", Matrix::IDENTITY, &mut declining, &fonts);
        assert_eq!(declining.count(EventKind::BeginSoftMask), 1);
        assert_eq!(declining.count(EventKind::GroupBudgetSpent), 0);

        // Replayed into a device that declines the mask the notice was
        // recorded deep inside, the notice still arrives: it is about the
        // interpretation, not about what the mask drew.
        let mut replayed = RecordingDevice::new();
        crate::replay::replay(accepting.events(), &mut replayed);
        assert_eq!(replayed.count(EventKind::BeginSoftMask), 1);
        assert_eq!(replayed.count(EventKind::GroupBudgetSpent), 1);
    }

    /// A source whose streams have identities, for cycles and the
    /// nested-content budget: forms by name, each with an identity and the
    /// content a test gives; `/M`, a graphics state whose soft mask's group a
    /// test gives; and the font `/T3`, each of whose codes' procedures a test
    /// gives, with the identity `0x100` and the code.
    struct Named {
        forms: Vec<(Vec<u8>, u64, Vec<u8>)>,
        mask: Option<(u64, Vec<u8>)>,
        glyphs: Vec<(u32, Vec<u8>)>,
    }

    impl Named {
        fn forms(forms: &[(&str, u64, &str)]) -> Named {
            Named {
                forms: forms
                    .iter()
                    .map(|&(name, stream, content)| {
                        (
                            name.as_bytes().to_vec(),
                            stream,
                            content.as_bytes().to_vec(),
                        )
                    })
                    .collect(),
                mask: None,
                glyphs: Vec::new(),
            }
        }

        fn glyphs(glyphs: Vec<(u32, Vec<u8>)>) -> Named {
            Named {
                forms: Vec::new(),
                mask: None,
                glyphs,
            }
        }
    }

    impl FontSource for Named {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            let (_, stream, content) = self.forms.iter().find(|(n, _, _)| n == name)?;
            Some(Form {
                content: content.clone(),
                matrix: Matrix::IDENTITY,
                bbox: None,
                group: None,
                stream: *stream,
            })
        }
        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            let (stream, content) = self.mask.as_ref().filter(|_| name == b"M")?;
            Some(Arc::new(StateParameters {
                soft_mask: Some(SoftMask::Group(Box::new(MaskGroup {
                    form: Form {
                        content: content.clone(),
                        matrix: Matrix::IDENTITY,
                        bbox: None,
                        group: None,
                        stream: *stream,
                    },
                    luminosity: true,
                    backdrop: None,
                    transfer: None,
                }))),
                ..StateParameters::default()
            }))
        }
        fn type3_glyph(&self, font: &[u8], code: u32) -> Option<Type3Glyph> {
            if font != b"T3" {
                return None;
            }
            let (_, content) = self.glyphs.iter().find(|(c, _)| *c == code)?;
            Some(Type3Glyph {
                content: content.clone(),
                matrix: Matrix {
                    a: 0.001,
                    d: 0.001,
                    ..Matrix::IDENTITY
                },
                stream: 0x100 + u64::from(code),
            })
        }
    }

    /// 8.10: a form that draws itself has no last run, and one that draws
    /// itself twice is two to the sixteenth runs inside the depth cap. Its
    /// identity says it is already running, so it draws once, and the device
    /// hears so once — in a replay too, since the notice is about the
    /// interpretation.
    #[test]
    fn a_form_that_draws_itself_draws_once() {
        let fonts = Named::forms(&[("F", 7, "/F Do /F Do 0 0 1 1 re f")]);
        let mut device = RecordingDevice::new();
        interpret(b"/F Do", Matrix::IDENTITY, &mut device, &fonts);
        assert_eq!(device.count(EventKind::BeginForm), 1);
        assert_eq!(device.count(EventKind::FillPath), 1);
        assert_eq!(device.count(EventKind::RecursionRefused), 1);

        let mut replayed = RecordingDevice::new();
        crate::replay::replay(device.events(), &mut replayed);
        assert_eq!(replayed.count(EventKind::RecursionRefused), 1);
    }

    /// The same through another form: each draws once in each run the page
    /// asks for — a form the page draws twice is no cycle — and the device
    /// hears of the cycle once however often it repeats.
    #[test]
    fn a_cycle_through_another_form_draws_each_once() {
        let fonts = Named::forms(&[
            ("A", 7, "/B Do 0 0 1 1 re f"),
            ("B", 8, "/A Do /A Do 1 1 1 1 re f"),
        ]);
        let mut device = RecordingDevice::new();
        interpret(b"/A Do /A Do", Matrix::IDENTITY, &mut device, &fonts);
        assert_eq!(
            device.count(EventKind::BeginForm),
            4,
            "A and B, for each of the page's two"
        );
        assert_eq!(device.count(EventKind::FillPath), 4);
        assert_eq!(device.count(EventKind::RecursionRefused), 1);
    }

    /// A soft mask whose group sets the same mask — the shape of a `/G` that
    /// names the page whose `gs` sets it — runs its group once, with no
    /// budget spent to stop it; and so does a cycle through a form inside the
    /// group.
    #[test]
    fn a_mask_whose_group_sets_it_again_runs_once() {
        let answers = Answers {
            soft_masks: true,
            ..Answers::default()
        };
        let mut fonts = Named::forms(&[("F", 6, "/M gs 0 0 1 1 re f")]);
        fonts.mask = Some((4, b"/M gs 1 1 1 1 re f".to_vec()));
        let mut device = RecordingDevice::new().answering(answers);
        interpret(b"/M gs", Matrix::IDENTITY, &mut device, &fonts);
        assert_eq!(device.count(EventKind::BeginSoftMask), 1);
        assert_eq!(device.count(EventKind::RecursionRefused), 1);
        assert_eq!(device.count(EventKind::GroupBudgetSpent), 0);

        fonts.mask = Some((4, b"/F Do".to_vec()));
        let mut device = RecordingDevice::new().answering(answers);
        interpret(b"/M gs", Matrix::IDENTITY, &mut device, &fonts);
        assert_eq!(device.count(EventKind::BeginSoftMask), 1);
        assert_eq!(device.count(EventKind::BeginForm), 1);
        assert_eq!(device.count(EventKind::FillPath), 1);
        assert_eq!(device.count(EventKind::RecursionRefused), 1);
    }

    /// 9.6.5: a glyph whose procedure shows itself draws once, as a form that
    /// draws itself does.
    #[test]
    fn a_glyph_that_shows_itself_draws_once() {
        let fonts = Named::glyphs(vec![(
            u32::from(b'a'),
            b"BT /T3 1 Tf (aa) Tj ET 0 0 1000 1000 re f".to_vec(),
        )]);
        let mut device = RecordingDevice::new();
        interpret(
            b"BT /T3 10 Tf (a) Tj ET",
            Matrix::IDENTITY,
            &mut device,
            &fonts,
        );
        assert_eq!(device.count(EventKind::FillPath), 1);
        assert_eq!(device.count(EventKind::RecursionRefused), 1);
    }

    /// The nested-content budget a tree of `count` streams of one `length`
    /// reached from `page` allows, in runs.
    fn runs_the_budget_allows(page: &[u8], count: u64, length: u64) -> u64 {
        let reached = page.len() as u64 + count * length;
        (NESTED_WORK_FLOOR + NESTED_WORK_RATE * reached) / (length + NESTED_RUN_COST)
    }

    /// Sixteen distinct forms, each drawing the next twice, are no cycle and
    /// two to the sixteenth runs inside the depth cap. The nested-content
    /// budget stops the tree where its floor and its rate on the content
    /// reached say — every form the same length, so every run costs the same
    /// — while every form still runs once, and the device hears so once, in a
    /// replay too.
    #[test]
    fn a_tree_of_forms_spends_the_nested_budget() {
        let names: Vec<String> = (0..16).map(|i| format!("F{i:02}")).collect();
        let mut forms = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let content = match names.get(i + 1) {
                Some(next) => format!("/{next} Do /{next} Do"),
                None => "0 0 1 1 re f   ".to_string(),
            };
            forms.push((
                name.clone().into_bytes(),
                100 + i as u64,
                content.into_bytes(),
            ));
        }
        let length = forms[0].2.len();
        assert!(forms.iter().all(|(_, _, content)| content.len() == length));
        let fonts = Named {
            forms,
            mask: None,
            glyphs: Vec::new(),
        };
        let page = b"/F00 Do";
        let mut device = RecordingDevice::new();
        interpret(page, Matrix::IDENTITY, &mut device, &fonts);

        let runs = device.count(EventKind::BeginForm) as u64;
        assert_eq!(runs, runs_the_budget_allows(page, 16, length as u64));
        assert!(runs < (1 << 16) - 1, "the whole tree is more");
        assert_eq!(device.count(EventKind::NestedBudgetSpent), 1);
        for name in &names {
            assert!(
                device.of_kind(EventKind::BeginForm).any(|event| matches!(
                    event,
                    Event::BeginForm { name: ran, .. } if ran == name.as_bytes()
                )),
                "{name} ran once at least"
            );
        }

        let mut replayed = RecordingDevice::new();
        crate::replay::replay(device.events(), &mut replayed);
        assert_eq!(replayed.count(EventKind::NestedBudgetSpent), 1);
    }

    /// The same for Type 3 glyphs: sixteen procedures, each showing the next
    /// twice, spend the same budget, and the last of them still draws.
    #[test]
    fn a_tree_of_glyphs_spends_the_nested_budget() {
        let codes: Vec<u8> = (b'a'..b'a' + 16).collect();
        let mut glyphs = Vec::new();
        for (i, &code) in codes.iter().enumerate() {
            let content = match codes.get(i + 1) {
                Some(&next) => format!("BT /T3 1 Tf ({0}{0}) Tj ET", char::from(next)),
                None => "0 0 1000 1000 re f    ".to_string(),
            };
            glyphs.push((u32::from(code), content.into_bytes()));
        }
        let length = glyphs[0].1.len();
        assert!(glyphs.iter().all(|(_, content)| content.len() == length));
        let fonts = Named::glyphs(glyphs);
        let page = b"BT /T3 10 Tf (a) Tj ET";
        let mut device = RecordingDevice::new();
        interpret(page, Matrix::IDENTITY, &mut device, &fonts);

        // Each run saves the device's state once, around its procedure.
        let runs = device.count(EventKind::SaveState) as u64;
        assert_eq!(runs, runs_the_budget_allows(page, 16, length as u64));
        assert!(runs < (1 << 16) - 1, "the whole tree is more");
        assert_eq!(device.count(EventKind::NestedBudgetSpent), 1);
        assert!(device.count(EventKind::FillPath) > 0, "the leaf drew");
    }

    /// A form that is not a transparency group is not offered at all.
    #[test]
    fn a_plain_form_raises_no_group_event() {
        let mut device = RecordingDevice::new().answering(Answers {
            groups: true,
            ..Answers::default()
        });
        interpret(
            b"/Half gs /Fm Do",
            Matrix::IDENTITY,
            &mut device,
            &Groups { group: None },
        );
        assert_eq!(device.count(EventKind::BeginGroup), 0);
        assert_eq!(device.count(EventKind::EndGroup), 0);
        assert_eq!(
            alphas(&device, EventKind::FillPath),
            vec![(0.5, 0.25, crate::state::BlendMode::Multiply)]
        );
    }

    /// The same font written vertically, with metrics that are all different
    /// numbers.
    ///
    /// Every component is distinct and none is a multiple of another, so an
    /// assertion below can only hold for one arrangement of them: `w1_y` is
    /// -800 where the *horizontal* width is 500, so a pen still advancing by
    /// `w0` — the defect this closes — lands somewhere none of these tests
    /// accept, and so does one that negates the sign.
    ///
    /// `A` and `B` differ from each other as well, so a run of two glyphs
    /// cannot pass by applying either one's metrics twice.
    struct Vertical;

    impl Vertical {
        /// `(v_x, v_y, w1_y)` per code, in 1/1000 em.
        fn metrics(code: u32) -> (f64, f64, f64) {
            match code {
                0x41 => (250.0, 700.0, -800.0),
                0x42 => (300.0, 650.0, -900.0),
                _ => (0.0, 880.0, -1000.0),
            }
        }
    }

    impl FontSource for Vertical {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn is_vertical(&self, _font: &[u8]) -> bool {
            true
        }
        fn vertical_metrics(&self, _font: &[u8], code: u32) -> (f64, f64, f64) {
            Vertical::metrics(code)
        }
    }

    fn run(src: &[u8]) -> RecordingDevice {
        let mut d = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut d, &Simple);
        d
    }

    fn run_vertical(src: &[u8]) -> RecordingDevice {
        let mut d = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut d, &Vertical);
        d
    }

    #[test]
    fn glyphs_advance_by_their_width() {
        let d = run(b"BT /F0 10 Tf 0 0 Td (AB) Tj ET");
        let text: String = d.glyphs().map(|g| g.text.as_str()).collect();
        assert_eq!(text, "AB");
        // 500/1000 em at 10pt is 5 points per glyph.
        assert_eq!(d.glyphs().next().map(|g| g.transform.e), Some(0.0));
        assert_eq!(d.glyphs().nth(1).map(|g| g.transform.e), Some(5.0));
    }

    #[test]
    fn tj_adjustments_move_the_pen_backwards() {
        let d = run(b"BT /F0 10 Tf [(A) -1000 (B)] TJ ET");
        // -1000 thousandths at 10pt closes 10 points, so B lands at 5 + 10.
        assert_eq!(d.glyphs().nth(1).map(|g| g.transform.e), Some(15.0));
    }

    #[test]
    fn character_and_word_spacing_apply_where_the_spec_says() {
        let spaced = run(b"BT /F0 10 Tf 2 Tc (AB) Tj ET");
        assert_eq!(spaced.glyphs().nth(1).map(|g| g.transform.e), Some(7.0));

        // Word spacing applies to code 32 only.
        let worded = run(b"BT /F0 10 Tf 100 Tw (A B) Tj ET");
        let xs: Vec<f64> = worded.glyphs().map(|g| g.transform.e).collect();
        assert_eq!(xs.first(), Some(&0.0), "A");
        assert_eq!(xs.get(1), Some(&5.0), "the space itself is not shifted");
        assert_eq!(xs.get(2), Some(&110.0), "B follows the word space");
    }

    // -----------------------------------------------------------------------
    // Vertical writing (gap 05, 9.4.4 and 9.7.4.3).
    // -----------------------------------------------------------------------

    /// Absolute positions after two glyphs, which is the whole point of the
    /// assertion: an inverted vertical displacement still draws a tidy column,
    /// just one running upward, and a test on the *gaps* between glyphs would
    /// accept it. A test on where the glyphs actually are cannot.
    ///
    /// Every number below is hand-computed from 9.4.4 with the pen starting at
    /// (100, 700) and 10pt text:
    ///
    /// - `A` is drawn at the pen offset by -v = (-250, -700)/1000 x 10, so at
    ///   (100 - 2.5, 700 - 7) = (97.5, 693).
    /// - the pen then moves by w1 = -800/1000 x 10 = -8, to (100, 692).
    /// - `B` is drawn at that pen offset by its own -v = (-3, -6.5), so at
    ///   (97, 685.5).
    ///
    /// The horizontal width is 500 for both, so a pen still advancing by `w0`
    /// puts `B` at y = 688.5 and a sign inversion puts it at 701.5. Neither is
    /// 685.5.
    #[test]
    fn a_vertical_run_advances_by_w1_and_is_placed_by_v() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td (AB) Tj ET");

        assert_eq!(
            d.glyphs().next().map(|g| (g.transform.e, g.transform.f)),
            Some((97.5, 693.0)),
            "A, at the pen minus its position vector"
        );
        assert_eq!(
            d.glyphs().nth(1).map(|g| (g.transform.e, g.transform.f)),
            Some((97.0, 685.5)),
            "B, one w1 further down and offset by its own v"
        );

        // The glyph event carries the same displacement, signed: a consumer
        // measuring a column reads it rather than recomputing it.
        assert_eq!(
            d.glyphs().map(|g| g.advance).collect::<Vec<_>>(),
            vec![-8.0, -9.0]
        );
    }

    /// The position vector is the half of 9.7.4.3 that has nothing to do with
    /// the advance, so it is asserted on its own: two codes with the same
    /// displacement and different vectors sit in different places.
    ///
    /// `A` and `B` are drawn one after another here, and the difference in
    /// their x is `(250 - 300)/1000 x 10 = -0.5` — a sideways shift inside the
    /// em box, in a writing mode where nothing else moves sideways at all.
    #[test]
    fn the_position_vector_shifts_the_glyph_within_its_em_box() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td (AB) Tj ET");
        let xs: Vec<f64> = d.glyphs().map(|g| g.transform.e).collect();
        assert_eq!(xs, vec![97.5, 97.0]);
        assert!(
            xs.iter().all(|x| *x != 100.0),
            "neither glyph sits at the pen itself, which is what applying no \
             position vector would look like"
        );
    }

    /// 9.4.4's `ty` has no `Th` in it, and its `Tc` is added to a negative
    /// displacement rather than scaling it. `Tz 200` doubles the *glyph*
    /// horizontally — so the position vector's x doubles with it, since v is a
    /// glyph-space coordinate — and leaves the column's step alone.
    #[test]
    fn horizontal_scaling_does_not_reach_a_vertical_advance() {
        let d = run_vertical(b"BT /F0 10 Tf 2 Tc 200 Tz 100 700 Td (AB) Tj ET");

        assert_eq!(
            d.glyphs().next().map(|g| (g.transform.e, g.transform.f)),
            Some((95.0, 693.0)),
            "A: v_x doubled by Tz, v_y untouched by it"
        );
        // The step is -8 + 2 = -6, and 9.4.4 does not multiply it by Th.
        // Character spacing shortens a downward step because the spec adds it
        // to a negative number; scaling it by 2 would put B at y = 682.
        assert_eq!(
            d.glyphs().nth(1).map(|g| (g.transform.e, g.transform.f)),
            Some((94.0, 687.5)),
            "B: one step of -6 down, not -12"
        );
    }

    /// 9.4.3: a `TJ` number is subtracted from "the current horizontal or
    /// vertical coordinate, depending on the writing mode". It went on the
    /// horizontal one whatever the mode, so a vertical run drifted sideways by
    /// every kern in it while the column itself never noticed.
    #[test]
    fn tj_adjustments_move_a_vertical_pen_along_its_own_column() {
        let d = run_vertical(b"BT /F0 10 Tf 100 700 Td [(A) -1000 (B)] TJ ET");

        // A is where it always was; the pen is then at (100, 692), and -1000
        // thousandths at 10pt subtracts -10 from y, taking it to 702.
        assert_eq!(
            d.glyphs().nth(1).map(|g| (g.transform.e, g.transform.f)),
            Some((97.0, 695.5))
        );
        assert_eq!(
            d.glyphs().nth(1).map(|g| g.transform.e),
            Some(97.0),
            "and the column has not moved sideways, which is what the \
             adjustment used to do"
        );
    }

    #[test]
    fn horizontal_scaling_multiplies_the_advance() {
        let d = run(b"BT /F0 10 Tf 50 Tz (AB) Tj ET");
        assert_eq!(d.glyphs().nth(1).map(|g| g.transform.e), Some(2.5));
    }

    #[test]
    fn td_and_t_star_move_by_the_leading() {
        let d = run(b"BT /F0 10 Tf 12 TL 5 700 Td (A) Tj T* (B) Tj ET");
        assert_eq!(
            d.glyphs().next().map(|g| (g.transform.e, g.transform.f)),
            Some((5.0, 700.0))
        );
        assert_eq!(
            d.glyphs().nth(1).map(|g| (g.transform.e, g.transform.f)),
            Some((5.0, 688.0))
        );
    }

    #[test]
    fn invisible_text_is_still_shown_to_the_device() {
        // Mode 3 is what a scanned page's OCR layer uses; extraction must see
        // it even though a renderer paints nothing.
        let d = run(b"BT /F0 10 Tf 3 Tr (hidden) Tj ET");
        let text: String = d.glyphs().map(|g| g.text.as_str()).collect();
        assert_eq!(text, "hidden");
    }

    #[test]
    fn the_transform_stack_restores_state() {
        let d = run(b"q 2 0 0 2 0 0 cm BT /F0 10 Tf (A) Tj ET Q BT /F0 10 Tf (B) Tj ET");
        // Inside the q/Q the CTM doubles the font size's effect on advance,
        // but both glyphs start at the origin.
        assert_eq!(d.count(EventKind::ShowGlyph), 2);
        assert_eq!(d.glyphs().next().map(|g| g.transform.e), Some(0.0));
        assert_eq!(d.glyphs().nth(1).map(|g| g.transform.e), Some(0.0));
    }

    #[test]
    fn paths_reach_the_device() {
        let d = run(b"0 0 m 10 10 l S 0 0 100 100 re f");
        assert_eq!(d.count(EventKind::StrokePath), 1);
        assert_eq!(d.count(EventKind::FillPath), 1);
    }

    #[test]
    fn an_inline_image_does_not_derail_the_stream() {
        let d = run(b"BI /W 2 /H 2 ID \x00\x01\x02\x03 EI BT /F0 10 Tf (A) Tj ET");
        let text: String = d.glyphs().map(|g| g.text.as_str()).collect();
        assert_eq!(text, "A", "text after an inline image still runs");
    }

    #[test]
    fn malformed_streams_do_not_panic() {
        for src in [
            b"".as_slice(),
            b"Tj",
            b"BT ET ET ET",
            b"Q Q Q",
            b"1 0 0 1 cm",
            b"BT /F0 Tf (x) Tj ET",
            b"[ ( unclosed",
            &[0xFF; 512],
        ] {
            let _ = run(src);
        }
    }

    #[test]
    fn a_non_finite_matrix_is_refused() {
        // Without the guard everything after this would be at NaN.
        let d = run(b"BT /F0 10 Tf 0 0 Td (A) Tj 1 0 0 0 0 0 Tm (B) Tj ET");
        assert!(d
            .glyphs()
            .all(|g| g.transform.e.is_finite() && g.transform.f.is_finite()));
    }

    // -----------------------------------------------------------------------
    // Patterns (gap 07, 8.7.3).
    // -----------------------------------------------------------------------

    /// Two uncoloured pattern spaces (8.7.3.2), over different underlying
    /// spaces so a test cannot pass by resolving against the wrong one:
    /// `/Prgb` reads three components as RGB, `/Pgray` reads one as grey.
    struct PatternSpaces;

    impl FontSource for PatternSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            match space {
                b"Prgb" => Some(3),
                b"Pgray" => Some(1),
                _ => None,
            }
        }
        fn resolve_color(
            &self,
            space: &[u8],
            components: &[f64],
            _intent: RenderingIntent,
        ) -> Option<Rgb> {
            let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            match (space, components) {
                (b"Prgb", [r, g, b]) => Some(Rgb {
                    r: byte(*r),
                    g: byte(*g),
                    b: byte(*b),
                }),
                (b"Pgray", [v]) => Some(Rgb {
                    r: byte(*v),
                    g: byte(*v),
                    b: byte(*v),
                }),
                _ => None,
            }
        }
    }

    fn run_patterned(src: &[u8]) -> RecordingDevice {
        let mut d = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut d, &PatternSpaces);
        d
    }

    /// The state the stroke saw, copied at the call.
    ///
    /// `Painted` kept one `Vec<GraphicsState>` for fills and strokes alike;
    /// every assertion it fed was about a stroke, and this says so.
    fn stroke_state(d: &RecordingDevice) -> Option<&GraphicsState> {
        d.of_kind(EventKind::StrokePath)
            .next()
            .and_then(Event::state)
    }

    /// 8.7.3.2: a `PaintType 2` pattern paints with the colour the operands
    /// before its name supply, in the pattern space's underlying space. The
    /// operands were being collected and then dropped, so the slot held
    /// whatever `cs`/`CS` had reset it to and the pattern had no colour at all
    /// to take.
    ///
    /// The two slots take different spaces *and* different colours here, so
    /// neither can pass by being written twice from the other's operands.
    #[test]
    fn an_uncoloured_pattern_keeps_the_colour_its_operands_gave() {
        let d = run_patterned(
            b"/Prgb cs 1 0 0 /P0 scn \
              /Pgray CS 0.5 /P1 SCN \
              0 0 m 10 10 l S",
        );

        let state = stroke_state(&d).expect("the stroke reached the device");
        assert_eq!(
            state.fill_color,
            Rgb { r: 255, g: 0, b: 0 },
            "the fill slot took its three operands as RGB"
        );
        assert_eq!(
            state.stroke_color,
            Rgb {
                r: 128,
                g: 128,
                b: 128
            },
            "and the stroke slot took its one operand as grey"
        );
        assert_eq!(
            state.fill_pattern.as_deref(),
            Some(b"P0".as_slice()),
            "with both patterns still selected"
        );
        assert_eq!(state.stroke_pattern.as_deref(), Some(b"P1".as_slice()));
    }

    /// A coloured pattern names itself and nothing else. Whatever colour the
    /// slot held stays put — there is nothing to overwrite it with, and the
    /// pattern supplies its own paint.
    #[test]
    fn a_pattern_named_alone_leaves_the_colour_where_it_was() {
        let d = run_patterned(b"0 1 0 RG /P1 SCN 0 0 m 10 10 l S");

        let state = stroke_state(&d).expect("the stroke reached the device");
        assert_eq!(
            state.stroke_color,
            Rgb { r: 0, g: 255, b: 0 },
            "no operands, so no new colour"
        );
        assert_eq!(state.stroke_pattern.as_deref(), Some(b"P1".as_slice()));
    }

    /// Choosing an ordinary colour has to clear the pattern on the stroke slot
    /// as it does on the fill slot, or every later stroke keeps painting it.
    #[test]
    fn an_ordinary_stroking_colour_clears_the_stroke_pattern() {
        let d = run_patterned(b"/P1 SCN 0 0 1 RG 0 0 m 10 10 l S");

        let state = stroke_state(&d).expect("the stroke reached the device");
        assert_eq!(state.stroke_pattern, None);
        assert_eq!(state.stroke_color, Rgb { r: 0, g: 0, b: 255 });
    }

    /// Resources whose spaces answer each rendering intent with a colour of
    /// its own, and which count what they are asked: `/ICC` and the
    /// uncoloured pattern space `/PICC` over it, one component each, and a
    /// `/DefaultGray` remapping `g`. `/GSp`, `/GSs` and `/GSr` are graphics
    /// states naming an intent by `/RI`.
    struct IntentSpaces {
        asked: std::cell::Cell<u32>,
    }

    impl IntentSpaces {
        fn new() -> IntentSpaces {
            IntentSpaces {
                asked: std::cell::Cell::new(0),
            }
        }
    }

    /// The colour [`IntentSpaces`] gives grey `v` in `intent`: a channel an
    /// intent, and an intent it does not recognise read as relative
    /// colorimetric, as 8.6.5.8 has it and the facade reads it.
    fn intent_colour(intent: RenderingIntent, v: u8) -> Rgb {
        match intent {
            RenderingIntent::Perceptual => Rgb { r: v, g: 0, b: 0 },
            RenderingIntent::Saturation => Rgb { r: 0, g: v, b: 0 },
            RenderingIntent::AbsoluteColorimetric => Rgb { r: 0, g: 0, b: v },
            RenderingIntent::RelativeColorimetric | RenderingIntent::Unrecognised => {
                Rgb { r: v, g: v, b: v }
            }
        }
    }

    impl FontSource for IntentSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            matches!(space, b"ICC" | b"PICC").then_some(1)
        }
        /// `/ICC` starts at a grey every intent paints its own way, so the
        /// colour `cs` sets shows whether an intent changing reaches it.
        fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
            match space {
                b"ICC" => Some(vec![0.75]),
                b"PICC" => Some(vec![0.0]),
                _ => None,
            }
        }
        fn resolve_color(
            &self,
            space: &[u8],
            components: &[f64],
            intent: RenderingIntent,
        ) -> Option<Rgb> {
            self.asked.set(self.asked.get() + 1);
            if !matches!(space, b"ICC" | b"PICC" | b"DeviceGray") {
                return None;
            }
            Some(intent_colour(
                intent,
                to_byte(components.first().copied().unwrap_or(0.0)),
            ))
        }
        fn device_defaults(&self) -> DeviceDefaults {
            DeviceDefaults {
                gray: true,
                rgb: false,
                cmyk: false,
            }
        }
        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            let intent = match name {
                b"GSp" => RenderingIntent::Perceptual,
                b"GSs" => RenderingIntent::Saturation,
                b"GSr" => RenderingIntent::RelativeColorimetric,
                _ => return None,
            };
            Some(Arc::new(StateParameters {
                rendering_intent: Some(intent),
                ..StateParameters::default()
            }))
        }
    }

    /// The fill and stroke colours of every painting operator's state, in
    /// order, with the fill's pattern.
    fn painted_colours(src: &[u8], fonts: &IntentSpaces) -> Vec<(Rgb, Rgb, Option<Vec<u8>>)> {
        let mut device = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut device, fonts);
        device
            .events()
            .iter()
            .filter(|event| matches!(event.kind(), EventKind::FillPath | EventKind::StrokePath))
            .filter_map(Event::state)
            .map(|state| {
                (
                    state.fill_color,
                    state.stroke_color,
                    state.fill_pattern.clone(),
                )
            })
            .collect()
    }

    /// **A colour set before the rendering intent changed paints in the
    /// intent at the paint** (11.7.5.3): `ri` and a `gs` naming an `/RI` each
    /// resolve the fill and the stroke colour again, from the space and
    /// components they were set with — a named space's, the initial colour
    /// `cs` set, a `g` a `/DefaultGray` remaps, an uncoloured pattern's,
    /// whose pattern stays selected — while a colour a device operator
    /// converted in place keeps its light. `Q` restores a colour with the
    /// intent it was resolved in; a name 8.6.5.8 does not list reads as
    /// relative colorimetric; and setting the intent in force again resolves
    /// nothing.
    #[test]
    fn a_colour_set_before_the_intent_changed_paints_in_the_new_intent() {
        use RenderingIntent::{Perceptual, RelativeColorimetric, Saturation};
        let grey = |v: f64| to_byte(v);
        let fonts = IntentSpaces::new();
        let fill = |src: &[u8]| {
            painted_colours(src, &fonts)
                .into_iter()
                .map(|(fill, _, _)| fill)
                .collect::<Vec<Rgb>>()
        };

        // A named space, by `ri` and by `gs`, fill and stroke alike.
        for set in [&b"/Perceptual ri"[..], b"/GSp gs"] {
            let mut src = b"/ICC cs 0.5 sc /ICC CS 0.25 SC ".to_vec();
            src.extend_from_slice(set);
            src.extend_from_slice(b" 0 0 1 1 re f 0 0 m 1 1 l S");
            let painted = painted_colours(&src, &fonts);
            for (fill, stroke, _) in &painted {
                assert_eq!(
                    *fill,
                    intent_colour(Perceptual, grey(0.5)),
                    "{set:?}: the fill"
                );
                assert_eq!(
                    *stroke,
                    intent_colour(Perceptual, grey(0.25)),
                    "{set:?}: the stroke"
                );
            }
            assert_eq!(painted.len(), 2, "{set:?}: a fill and a stroke");
        }

        // `cs`'s initial colour, and `Q` restoring the intent a colour was
        // resolved in.
        assert_eq!(
            fill(b"/ICC cs /Saturation ri 0 0 1 1 re f"),
            [intent_colour(Saturation, grey(0.75))]
        );
        assert_eq!(
            fill(b"/ICC cs 0.5 sc q /Saturation ri 0 0 1 1 re f Q 0 0 1 1 re f"),
            [
                intent_colour(Saturation, grey(0.5)),
                intent_colour(RelativeColorimetric, grey(0.5)),
            ]
        );

        // A device colour converted in place keeps its light; one a default
        // remaps is the default's, in the new intent.
        assert_eq!(
            fill(b"0.5 0 0 rg /Perceptual ri 0 0 1 1 re f"),
            [Rgb {
                r: grey(0.5),
                g: 0,
                b: 0
            }]
        );
        assert_eq!(
            fill(b"0.5 g /Perceptual ri 0 0 1 1 re f"),
            [intent_colour(Perceptual, grey(0.5))]
        );

        // An uncoloured pattern's colour, its pattern still selected.
        let painted = painted_colours(b"/PICC cs 0.5 /P0 scn /Saturation ri 0 0 1 1 re f", &fonts);
        assert_eq!(
            painted,
            [(
                intent_colour(Saturation, grey(0.5)),
                Rgb::BLACK,
                Some(b"P0".to_vec())
            )]
        );

        // An unrecognised name, and an intent changed back by `gs`.
        assert_eq!(
            fill(b"/ICC cs 0.5 sc /Saturation ri /Bogus ri 0 0 1 1 re f"),
            [intent_colour(RelativeColorimetric, grey(0.5))]
        );
        assert_eq!(
            fill(b"/ICC cs 0.5 sc /GSs gs 0 0 1 1 re f /GSr gs 0 0 1 1 re f"),
            [
                intent_colour(Saturation, grey(0.5)),
                intent_colour(RelativeColorimetric, grey(0.5)),
            ]
        );

        // The intent in force set again asks the seam nothing.
        let counted = IntentSpaces::new();
        painted_colours(b"/ICC cs 0.5 sc /Saturation ri", &counted);
        let after_change = counted.asked.get();
        let again = IntentSpaces::new();
        painted_colours(
            b"/ICC cs 0.5 sc /Saturation ri /Saturation ri /GSs gs",
            &again,
        );
        assert_eq!(again.asked.get(), after_change, "the same intent again");
        // And a page that sets no intent asks once a colour.
        let none = IntentSpaces::new();
        painted_colours(b"/ICC cs 0.5 sc 0 0 1 1 re f", &none);
        assert_eq!(none.asked.get(), 2, "the initial colour, then the set one");
    }

    /// Resources that know DeviceCMYK by name, as the facade's do.
    struct CmykSpaces;

    impl FontSource for CmykSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            (space == b"DeviceCMYK").then_some(4)
        }
        fn resolve_color(&self, space: &[u8], c: &[f64], _intent: RenderingIntent) -> Option<Rgb> {
            match (space, c) {
                (b"DeviceCMYK", [c, m, y, k]) => Some(cmyk_to_rgb(*c, *m, *y, *k)),
                _ => None,
            }
        }
        fn resolve_ink(&self, space: &[u8], c: &[f64]) -> Option<[u8; 4]> {
            match (space, c) {
                (b"DeviceCMYK", [c, m, y, k]) => {
                    Some([to_byte(*c), to_byte(*m), to_byte(*y), to_byte(*k)])
                }
                _ => None,
            }
        }
        fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
            (space == b"DeviceCMYK").then(|| vec![0.0, 0.0, 0.0, 1.0])
        }
    }

    /// A colour set in DeviceCMYK keeps its components beside its light —
    /// `k` and `K` by the operator, `cs`/`sc` by asking the resources — and a
    /// colour set any other way has none, so an ink device composites the
    /// file's own ink and nothing it would have to guess at. `cs` resets to
    /// the space's initial colour, which for DeviceCMYK is 8.6.8's `0 0 0 1`.
    #[test]
    fn a_device_cmyk_colour_keeps_its_components_beside_its_light() {
        let mut d = RecordingDevice::new();
        interpret(
            b"1 1 1 1 k 0.2 0.4 0.6 0.1 K 0 0 m 10 10 l S \
              1 0 0 rg 0 0 m 10 10 l S \
              /DeviceCMYK cs 0 0 m 10 10 l S \
              /DeviceCMYK CS 0.5 0 0 0 SC 0 0 m 10 10 l S \
              0.5 G 0 0 m 10 10 l S",
            Matrix::IDENTITY,
            &mut d,
            &CmykSpaces,
        );
        let states: Vec<&GraphicsState> = d
            .of_kind(EventKind::StrokePath)
            .filter_map(Event::state)
            .collect();
        assert_eq!(states.len(), 5);
        assert_eq!(states[0].fill_ink, Some([255; 4]), "a rich black");
        assert_eq!(states[0].fill_color, Rgb::BLACK, "whose light is black");
        assert_eq!(states[0].stroke_ink, Some([51, 102, 153, 26]), "`K`");
        assert_eq!(states[1].fill_ink, None, "`rg` is light and has no ink");
        assert_eq!(states[1].stroke_ink, Some([51, 102, 153, 26]));
        assert_eq!(states[2].fill_ink, Some([0, 0, 0, 255]), "`cs` resets");
        assert_eq!(states[2].fill_color, Rgb::BLACK, "to black, not white");
        assert_eq!(states[3].stroke_ink, Some([128, 0, 0, 0]), "`SC`");
        assert_eq!(states[4].stroke_ink, None, "`G` is light");
    }

    /// `x`, in `0..=1`, at sixteen bits by the relation ruling 23 settles,
    /// computed here from its definition (`docs/design/sixteen-bit.md`):
    /// `round(65 535·x)` held to `256·v ..= 256·v + 255`, `v` the byte
    /// `round(255·x)`.
    fn relation16(x: f64) -> u16 {
        let byte = (x * 255.0).round() as u32;
        let wide = (x * 65_535.0).round() as u32;
        wide.clamp(256 * byte, 256 * byte + 255) as u16
    }

    /// Resources that answer a colour at sixteen bits, as the facade's do:
    /// `/Wide` a three-component space whose colour is its components
    /// reversed in relative colorimetric and halved in any other intent,
    /// `/WidePattern` an uncoloured pattern space over it, and DeviceGray by
    /// name, inverted, where the scope remaps it. Each answers its bytes and
    /// its 16-bit light from one reading, the light's top bytes the bytes.
    struct WideSpaces {
        defaults: DeviceDefaults,
    }

    impl WideSpaces {
        /// What the resources answer `components` of `space` with in
        /// `intent`, as the values the bytes and the light are rounded from.
        fn values(space: &[u8], components: &[f64], intent: RenderingIntent) -> Option<[f64; 3]> {
            let at = |i: usize| components.get(i).copied().unwrap_or(0.0);
            match space {
                b"Wide" | b"WidePattern" => {
                    let scale = if intent == RenderingIntent::RelativeColorimetric {
                        1.0
                    } else {
                        0.5
                    };
                    Some([at(2) * scale, at(1) * scale, at(0) * scale])
                }
                b"DeviceGray" => Some([1.0 - at(0); 3]),
                _ => None,
            }
        }
    }

    impl FontSource for WideSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            matches!(space, b"Wide" | b"WidePattern").then_some(3)
        }
        fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
            matches!(space, b"Wide" | b"WidePattern").then(|| vec![0.0; 3])
        }
        fn resolve_color(
            &self,
            space: &[u8],
            components: &[f64],
            intent: RenderingIntent,
        ) -> Option<Rgb> {
            let [r, g, b] = WideSpaces::values(space, components, intent)?.map(to_byte);
            Some(Rgb { r, g, b })
        }
        fn resolve_color_wide(
            &self,
            space: &[u8],
            components: &[f64],
            intent: RenderingIntent,
        ) -> Option<(Rgb, Rgb16)> {
            let values = WideSpaces::values(space, components, intent)?;
            let [r, g, b] = values.map(to_byte);
            Some((Rgb { r, g, b }, Rgb16::from(values.map(relation16))))
        }
        fn device_defaults(&self) -> DeviceDefaults {
            self.defaults
        }
    }

    /// The fill colour, both widths, of every fill's state and the stroke
    /// colour of every stroke's, in order.
    fn wide_colours<F: FontSource>(
        src: &[u8],
        fonts: &F,
    ) -> (Vec<(Rgb, Rgb16)>, Vec<(Rgb, Rgb16)>) {
        let mut d = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut d, fonts);
        let fills = d
            .of_kind(EventKind::FillPath)
            .filter_map(Event::state)
            .map(|state| (state.fill_color, state.fill_wide))
            .collect();
        let strokes = d
            .of_kind(EventKind::StrokePath)
            .filter_map(Event::state)
            .map(|state| (state.stroke_color, state.stroke_wide))
            .collect();
        (fills, strokes)
    }

    /// A colour as the test expects it: the bytes `to_byte` gives `values`,
    /// as the interpreter has always recorded them, and the light at
    /// sixteen bits by the relation.
    fn expected(values: [f64; 3]) -> (Rgb, Rgb16) {
        let [r, g, b] = values.map(to_byte);
        (Rgb { r, g, b }, Rgb16::from(values.map(relation16)))
    }

    /// **A resolved colour keeps sixteen bits and its 8-bit bytes**
    /// (`docs/design/sixteen-bit.md`, "Resolved colour"). `g`, `RG`, `k`,
    /// `sc` read by count, and `cs`/`scn` and `CS`/`SCN` through resources
    /// that answer at sixteen bits each record the bytes they record without
    /// it and, beside them, the 16-bit light the test computes from the
    /// components: grey and RGB the components themselves, CMYK 8.6.4.4's
    /// complement, a named space the resources' answer — a component on
    /// which the two roundings part, 0.1981, held to its byte at 13 056. A
    /// rendering intent changing resolves the light again with the bytes, an
    /// uncoloured pattern's components keep theirs, `Q` restores both, a
    /// device operator in a scope that remaps its space takes the default's,
    /// and resources that answer only bytes give those bytes widened.
    #[test]
    fn a_resolved_colour_keeps_sixteen_bits_and_its_eight_bit_bytes() {
        let wide = WideSpaces {
            defaults: DeviceDefaults::NONE,
        };
        let (fills, strokes) = wide_colours(
            b"0.1981 g 0.2 0.5 0.8019 RG 0 0 10 10 re f 0 0 m 10 10 l S \
              0.1 0.2 0.3 0.4 k 0 0 10 10 re f \
              0.2 0.5 0.8019 sc 0 0 10 10 re f \
              /Wide cs 0.3 0.6 0.9 scn 0 0 10 10 re f \
              /Perceptual ri 0 0 10 10 re f \
              /Wide CS 0.3 0.6 0.9 SCN 0 0 m 10 10 l S \
              /RelativeColorimetric ri /WidePattern cs 0.3 0.6 0.9 /P scn 0 0 10 10 re f \
              q 0.7 g Q 0 0 10 10 re f",
            &wide,
        );
        assert_eq!(fills.len(), 7);
        assert_eq!(strokes.len(), 2);

        assert_eq!(
            fills[0],
            (
                Rgb {
                    r: 51,
                    g: 51,
                    b: 51
                },
                Rgb16::from([13_056; 3])
            ),
            "`g` at 0.1981, held to its byte"
        );
        assert_eq!(fills[0], expected([0.1981; 3]), "`g`");
        assert_eq!(strokes[0], expected([0.2, 0.5, 0.8019]), "`RG`");
        let (c, m, y, k) = (0.1, 0.2, 0.3, 0.4);
        assert_eq!(
            fills[1],
            expected([
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k)
            ]),
            "`k`"
        );
        assert_eq!(fills[2], expected([0.2, 0.5, 0.8019]), "`sc` read by count");
        assert_eq!(fills[3], expected([0.9, 0.6, 0.3]), "`scn` in /Wide");
        let halved = [0.9 * 0.5, 0.6 * 0.5, 0.3 * 0.5];
        assert_eq!(
            fills[4],
            expected(halved),
            "resolved again in the perceptual intent"
        );
        assert_eq!(strokes[1], expected(halved), "`SCN` in /Wide, perceptual");
        assert_eq!(
            fills[5],
            expected([0.9, 0.6, 0.3]),
            "an uncoloured pattern's components"
        );
        assert_eq!(fills[6], fills[5], "`Q` restores both widths");
        for (color, wide) in fills.iter().chain(&strokes) {
            assert_eq!(wide.top(), *color, "the light tops out at the bytes");
        }

        // A device operator in a scope that remaps its space takes the
        // default's answer at both widths.
        let remapping = WideSpaces {
            defaults: DeviceDefaults {
                gray: true,
                rgb: false,
                cmyk: false,
            },
        };
        let (fills, _) = wide_colours(b"0.25 g 0 0 10 10 re f", &remapping);
        assert_eq!(fills, vec![expected([0.75; 3])], "`g` under a /DefaultGray");

        // Resources that answer only bytes give them widened: a correct
        // 16-bit colour that refines nothing.
        let (fills, _) = wide_colours(b"/DeviceCMYK cs 0.5 0 0 0 sc 0 0 10 10 re f", &CmykSpaces);
        let bytes = cmyk_to_rgb(0.5, 0.0, 0.0, 0.0);
        assert_eq!(
            fills,
            vec![(bytes, Rgb16::widen(bytes))],
            "bytes alone, widened"
        );
    }

    /// Resources that name colorants, as the facade's will: `/Spot` a
    /// `/Separation` of `Gold`, `/Inks` a `/DeviceN` of `Orange` and `Green`,
    /// `/Pat` an uncoloured pattern space over `/Spot`, `/Rgb` a space that
    /// names none, and DeviceCMYK by name; each colorants question counted.
    struct ColorantSpaces {
        asked: std::cell::Cell<usize>,
        defaults: DeviceDefaults,
    }

    impl ColorantSpaces {
        fn new(defaults: DeviceDefaults) -> ColorantSpaces {
            ColorantSpaces {
                asked: std::cell::Cell::new(0),
                defaults,
            }
        }

        /// The tints a test source answers: each name's byte, and the
        /// alternate a recognisable function of the first.
        fn tints(kind: ColorantKind, named: &[(&[u8], f64)]) -> Arc<ColorantTints> {
            let first = named.first().map_or(0, |(_, tint)| to_byte(*tint));
            Arc::new(ColorantTints {
                kind,
                tints: named
                    .iter()
                    .map(|(name, tint)| ColorantTint {
                        name: Arc::from(*name),
                        tint: to_byte(*tint),
                        alone: None,
                    })
                    .collect(),
                alternate: Some([0, first, 0, 0]),
                process: None,
            })
        }
    }

    impl FontSource for ColorantSpaces {
        fn decode(&self, _font: &[u8], _bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Vec::new()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn device_defaults(&self) -> DeviceDefaults {
            self.defaults
        }
        fn color_components(&self, space: &[u8]) -> Option<usize> {
            match space {
                b"Spot" | b"Pat" => Some(1),
                b"Inks" => Some(2),
                b"Rgb" => Some(3),
                b"DeviceCMYK" => Some(4),
                _ => None,
            }
        }
        fn resolve_color(&self, space: &[u8], c: &[f64], _intent: RenderingIntent) -> Option<Rgb> {
            match (space, c) {
                (b"Spot" | b"Pat", [t]) => {
                    let v = to_byte(1.0 - t);
                    Some(Rgb { r: v, g: v, b: 0 })
                }
                (b"Inks", [a, b]) => Some(Rgb {
                    r: to_byte(1.0 - a),
                    g: to_byte(1.0 - b),
                    b: 0,
                }),
                (b"Rgb", [r, g, b]) => Some(Rgb {
                    r: to_byte(*r),
                    g: to_byte(*g),
                    b: to_byte(*b),
                }),
                (b"DeviceCMYK", [c, m, y, k]) => Some(cmyk_to_rgb(*c, *m, *y, *k)),
                _ => None,
            }
        }
        fn resolve_ink(&self, space: &[u8], c: &[f64]) -> Option<[u8; 4]> {
            match (space, c) {
                (b"DeviceCMYK", [c, m, y, k]) => {
                    Some([to_byte(*c), to_byte(*m), to_byte(*y), to_byte(*k)])
                }
                _ => None,
            }
        }
        fn colorants(
            &self,
            space: &[u8],
            c: &[f64],
            _intent: RenderingIntent,
        ) -> Option<Arc<ColorantTints>> {
            self.asked.set(self.asked.get() + 1);
            match (space, c) {
                (b"Spot" | b"Pat", [t]) => {
                    Some(Self::tints(ColorantKind::Colorants, &[(b"Gold", *t)]))
                }
                (b"Inks", [a, b]) => Some(Self::tints(
                    ColorantKind::Colorants,
                    &[(b"Orange", *a), (b"Green", *b)],
                )),
                (b"DeviceCMYK", [c, m, y, k]) => Some(Arc::new(ColorantTints::device_cmyk([
                    to_byte(*c),
                    to_byte(*m),
                    to_byte(*y),
                    to_byte(*k),
                ]))),
                _ => None,
            }
        }
        fn initial_color(&self, space: &[u8]) -> Option<Vec<f64>> {
            match space {
                b"Spot" | b"Pat" => Some(vec![1.0]),
                b"Inks" => Some(vec![1.0, 1.0]),
                b"Rgb" => Some(vec![0.0; 3]),
                b"DeviceCMYK" => Some(vec![0.0, 0.0, 0.0, 1.0]),
                _ => None,
            }
        }
    }

    /// The tints a state's colorants hold, by name, or `None` for none.
    fn tints_of(colorants: &Option<Arc<ColorantTints>>) -> Option<Vec<(Vec<u8>, u8)>> {
        colorants.as_ref().map(|colorants| {
            colorants
                .tints
                .iter()
                .map(|tint| (tint.name.to_vec(), tint.tint))
                .collect()
        })
    }

    /// The content every colorants question is asked through: `k` and `K`,
    /// `cs` resetting a separation to its whole colorant and `sc` tinting
    /// it, `CS` and `SC` in a `/DeviceN`, `rg` and `G` inside a `q`, the
    /// colours after its `Q`, a space naming no colorant, four components
    /// with no space named, and an uncoloured pattern's components.
    const COLORANT_CONTENT: &[u8] = b"1 0 0 0 k 0.2 0.4 0.6 0.1 K 0 0 m 10 10 l S \
        /Spot cs 0 0 m 10 10 l S \
        0.5 sc 0 0 m 10 10 l S \
        /Inks CS 0.25 0.75 SC 0 0 m 10 10 l S \
        q 1 0 0 rg 0.5 G 0 0 m 10 10 l S Q 0 0 m 10 10 l S \
        /Rgb cs 0.1 0.2 0.3 sc 0 0 m 10 10 l S \
        0.5 g 0 0 0 1 sc 0 0 m 10 10 l S \
        /Pat cs 0.3 /P1 scn 0 0 m 10 10 l S";

    /// **A colour carries the colorants its space answers** (RC-04.6), beside
    /// `a_device_cmyk_colour_keeps_its_components_beside_its_light`: on a
    /// device that composites colorants, `k` and `K` carry DeviceCMYK's four
    /// process colorants at their own components; `cs` resets a `/Separation`
    /// to its whole colorant (8.6.6.4) and `sc` tints it; `CS` and `SC` carry a
    /// `/DeviceN`'s every colorant; `rg` and `G` carry none, and `Q` restores
    /// what the `q` saved; a space that names no colorant carries none; four
    /// components with no space named are DeviceCMYK's own; and an uncoloured
    /// pattern's components carry their underlying space's colorants. A device
    /// that composites none is handed none, and the resources are asked
    /// nothing; under a `/DefaultCMYK` a `k` asks the resources by the device
    /// space's name, as its light and ink are asked.
    #[test]
    fn a_colour_carries_the_colorants_its_space_answers() {
        let source = ColorantSpaces::new(DeviceDefaults::NONE);
        let mut d = RecordingDevice::new().recording_colorants(true);
        interpret(COLORANT_CONTENT, Matrix::IDENTITY, &mut d, &source);
        let states: Vec<&GraphicsState> = d
            .of_kind(EventKind::StrokePath)
            .filter_map(Event::state)
            .collect();
        assert_eq!(states.len(), 9);
        let gold = |tint: u8| Some(vec![(b"Gold".to_vec(), tint)]);
        let cmyk = |c: u8, m: u8, y: u8, k: u8| {
            Some(vec![
                (b"Cyan".to_vec(), c),
                (b"Magenta".to_vec(), m),
                (b"Yellow".to_vec(), y),
                (b"Black".to_vec(), k),
            ])
        };
        let inks = Some(vec![(b"Orange".to_vec(), 64), (b"Green".to_vec(), 191)]);

        assert_eq!(
            tints_of(&states[0].fill_colorants),
            cmyk(255, 0, 0, 0),
            "`k`"
        );
        assert_eq!(
            tints_of(&states[0].stroke_colorants),
            cmyk(51, 102, 153, 26),
            "`K`"
        );
        assert_eq!(
            states[0].fill_colorants.as_ref().map(|c| c.kind),
            Some(ColorantKind::DirectCmyk)
        );
        assert_eq!(
            tints_of(&states[1].fill_colorants),
            gold(255),
            "`cs` resets to the whole colorant"
        );
        assert_eq!(
            tints_of(&states[1].stroke_colorants),
            cmyk(51, 102, 153, 26),
            "the stroke kept"
        );
        assert_eq!(tints_of(&states[2].fill_colorants), gold(128), "`sc`");
        assert_eq!(
            states[2].fill_colorants.as_ref().and_then(|c| c.alternate),
            Some([0, 128, 0, 0]),
            "the alternate the resources answered"
        );
        assert_eq!(tints_of(&states[3].stroke_colorants), inks, "`CS` and `SC`");
        assert_eq!(states[4].fill_colorants, None, "`rg` carries none");
        assert_eq!(states[4].stroke_colorants, None, "`G` carries none");
        assert_eq!(states[4].fill_ink, None);
        assert_eq!(
            tints_of(&states[5].fill_colorants),
            gold(128),
            "`Q` restores the fill's"
        );
        assert_eq!(
            tints_of(&states[5].stroke_colorants),
            inks,
            "and the stroke's"
        );
        assert_eq!(
            states[6].fill_colorants, None,
            "a space that names no colorant"
        );
        assert_eq!(
            tints_of(&states[7].fill_colorants),
            cmyk(0, 0, 0, 255),
            "four with no space named"
        );
        assert_eq!(
            tints_of(&states[8].fill_colorants),
            gold(77),
            "an uncoloured pattern's"
        );
        assert_eq!(states[8].fill_pattern.as_deref(), Some(b"P1".as_slice()));

        // A device that composites no colorants: none, and nothing asked.
        let quiet = ColorantSpaces::new(DeviceDefaults::NONE);
        let mut light = RecordingDevice::new();
        interpret(COLORANT_CONTENT, Matrix::IDENTITY, &mut light, &quiet);
        let states: Vec<&GraphicsState> = light
            .of_kind(EventKind::StrokePath)
            .filter_map(Event::state)
            .collect();
        assert_eq!(states.len(), 9);
        assert!(states
            .iter()
            .all(|state| state.fill_colorants.is_none() && state.stroke_colorants.is_none()));
        assert_eq!(quiet.asked.get(), 0, "a light render asks nothing");
        assert_eq!(
            states[0].fill_ink,
            Some([255, 0, 0, 0]),
            "and keeps its ink"
        );

        // A `/DefaultCMYK`: `k` asks the resources by DeviceCMYK's name.
        let remapping = ColorantSpaces::new(DeviceDefaults {
            cmyk: true,
            ..DeviceDefaults::NONE
        });
        let mut remapped = RecordingDevice::new().recording_colorants(true);
        interpret(
            b"0 0.5 0 0 k 0 0 m 10 10 l f",
            Matrix::IDENTITY,
            &mut remapped,
            &remapping,
        );
        let state = remapped
            .of_kind(EventKind::FillPath)
            .find_map(Event::state)
            .expect("the fill reached the device");
        assert_eq!(tints_of(&state.fill_colorants), cmyk(0, 128, 0, 0));
        assert_eq!(
            remapping.asked.get(),
            1,
            "asked once, by the device space's name"
        );
    }

    /// A colour carries the family its values are in: each device operator
    /// its own, `cs` its space's (the seam's answer, here the device names'),
    /// `sc` in a named space the family `cs` gave it and `sc` read by count
    /// the family its count names; `Q` restores it with the colour, and the
    /// page starts in DeviceGray (8.6.8).
    #[test]
    fn a_colour_carries_the_family_its_space_is_read_in() {
        assert_eq!(
            GraphicsState::new(Matrix::IDENTITY).fill_family,
            ColorFamily::Gray
        );
        let mut d = RecordingDevice::new();
        interpret(
            b"0 0 m 1 1 l S \
              0.5 G 0 0 m 1 1 l S \
              1 0 0 RG 0 0 m 1 1 l S \
              0 0 0 1 K 0 0 m 1 1 l S \
              /DeviceRGB CS 0.1 0.2 0.3 SC 0 0 m 1 1 l S \
              0.3 G 0.1 0.2 0.3 SC 0 0 m 1 1 l S \
              0.3 G 0.1 0.2 0.3 0.4 SC 0 0 m 1 1 l S \
              /CS0 CS 0 0 m 1 1 l S \
              q 0.4 G Q 0 0 m 1 1 l S \
              0.2 0.4 0.6 rg 0 0 m 1 1 l f",
            Matrix::IDENTITY,
            &mut d,
            &CmykSpaces,
        );
        let strokes: Vec<ColorFamily> = d
            .of_kind(EventKind::StrokePath)
            .filter_map(Event::state)
            .map(|state| state.stroke_family)
            .collect();
        assert_eq!(
            strokes,
            [
                ColorFamily::Gray,
                ColorFamily::Gray,
                ColorFamily::Rgb,
                ColorFamily::Cmyk,
                ColorFamily::Rgb,
                ColorFamily::Rgb,
                ColorFamily::Cmyk,
                ColorFamily::Other,
                ColorFamily::Other,
            ]
        );
        let fills: Vec<ColorFamily> = d
            .of_kind(EventKind::FillPath)
            .filter_map(Event::state)
            .map(|state| state.fill_family)
            .collect();
        assert_eq!(fills, [ColorFamily::Rgb], "`rg` sets the fill's alone");
    }

    // -----------------------------------------------------------------------
    // Marked content and optional content (gap 06, 14.6.2 and 8.11.3.2).
    // -----------------------------------------------------------------------

    /// Every scope's own `(visible, layer)`, in order.
    ///
    /// Each scope's **own** answer, which is what the device is handed; the
    /// enclosing answer is [`hidden_fills`] and `hidden_at` below.
    fn begins(d: &RecordingDevice) -> Vec<(bool, Option<String>)> {
        d.scopes()
            .map(|scope| (scope.visible, scope.hidden_layer.clone()))
            .collect()
    }

    /// Every scope's tag, in order.
    fn scope_tags(d: &RecordingDevice) -> Vec<Vec<u8>> {
        d.scopes().map(|scope| scope.tag.clone()).collect()
    }

    /// Every scope's property list, in order.
    fn scope_props(d: &RecordingDevice) -> Vec<Option<MarkedProps>> {
        d.scopes().map(|scope| scope.props.clone()).collect()
    }

    /// One entry per fill: whether **any** scope open at that fill was hiding
    /// it.
    ///
    /// Derived rather than stored, which is the whole nesting argument: the
    /// device keeps each scope's own answer and this walks the stack, exactly
    /// as a drawing device must — so a test here fails for the same reason
    /// the renderer would.
    fn hidden_fills(d: &RecordingDevice) -> Vec<bool> {
        d.indices(EventKind::FillPath)
            .map(|index| d.hidden_at(index))
            .collect()
    }

    /// The fills that were painted with nothing hiding them.
    fn painted(d: &RecordingDevice) -> usize {
        hidden_fills(d).iter().filter(|hidden| !**hidden).count()
    }

    /// The property lists the interpreter reported, in order.
    fn props_of(source: &[u8]) -> Vec<Option<MarkedProps>> {
        scope_props(&run_layers(source))
    }

    /// The tags it reported, as strings.
    fn tags_of(source: &[u8]) -> Vec<String> {
        scope_tags(&run_layers(source))
            .iter()
            .map(|tag| String::from_utf8_lossy(tag).into_owned())
            .collect()
    }

    /// 14.7.4.2: a marked sequence carries the stream its `BDC` was written
    /// in, and a nested form's is its own rather than its caller's.
    ///
    /// Three sequences all numbered 0 — the page's, a form's, and a form that
    /// form invokes — which without the stream identity are one sequence
    /// numbered three times. The stack has to unwind too: the identity is a
    /// property of where the `BDC` is, so a form that has returned must not
    /// leave its own on the next sequence its caller opens.
    #[test]
    fn a_marked_sequence_carries_the_stream_its_bdc_was_written_in() {
        let props = props_of(b"/P << /MCID 0 >> BDC 0 0 2 2 re f EMC /OuterFrm Do");

        let sequences: Vec<(u64, Option<u32>)> = props
            .iter()
            .map(|p| p.as_ref().map_or((0, None), |p| (p.stream, p.mcid)))
            .collect();
        assert_eq!(
            sequences,
            vec![(0, Some(0)), (0x3_0000, Some(0)), (0x4_0000, Some(0))],
            "the page's own stream is 0, and the innermost form is the one \
             that wrote the sequence"
        );
    }

    /// The common form: `/P << /MCID 3 >> BDC`.
    ///
    /// 14.7.4.2 puts `/MCID` in the property list, and 7.8.2 flattens that
    /// list to tokens — so this is the reassembly working, and without it a
    /// tagged page has no marked content this engine can see at all.
    #[test]
    fn an_inline_property_list_yields_its_mcid() {
        let props = props_of(b"/P << /MCID 3 >> BDC 0 0 2 2 re f EMC");
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].as_ref().and_then(|p| p.mcid), Some(3));
        assert_eq!(
            tags_of(b"/P << /MCID 3 >> BDC EMC"),
            vec!["P"],
            "the tag is the name before the `<<`, not the last value inside it"
        );
    }

    /// 14.9's four values, inline, beside the `/MCID` and around entries this
    /// build reads past.
    ///
    /// `/BBox` is an array and `/A` a nested dictionary: both are values a
    /// property list legitimately carries, and a scan that stepped *into*
    /// either would read their members as top-level keys and find an `/E`
    /// that is not the abbreviation expansion.
    #[test]
    fn an_inline_property_list_yields_the_accessibility_values() {
        let props = props_of(
            b"/Span << /Type /Pagination /BBox [0 0 9 9] /MCID 12 \
                /ActualText (fi) /Alt (a ligature) /Lang (en-GB) /E (etc.) \
                /A << /E (not this one) >> >> BDC 0 0 2 2 re f EMC",
        );
        let props = props[0].clone().expect("a property list");
        assert_eq!(props.mcid, Some(12));
        assert_eq!(props.actual_text.as_deref(), Some("fi"));
        assert_eq!(props.alt.as_deref(), Some("a ligature"));
        assert_eq!(props.lang.as_deref(), Some("en-GB"));
        assert_eq!(
            props.expansion.as_deref(),
            Some("etc."),
            "the /E read is the one at the top level, not the one inside /A"
        );
    }

    /// 7.9.2.2: the four are *text strings*, so a UTF-16BE one decodes rather
    /// than arriving as interleaved NULs.
    #[test]
    fn an_inline_text_string_is_decoded_by_the_documents_rules() {
        let props = props_of(b"/Span << /MCID 0 /Alt <FEFF00660069> >> BDC EMC");
        assert_eq!(
            props[0].as_ref().and_then(|p| p.alt.clone()).as_deref(),
            Some("fi")
        );
    }

    /// `/P /MC0 BDC`: the same values through the `/Properties` seam.
    ///
    /// The two forms must be indistinguishable at the device, because 14.6.2
    /// gives a consumer no reason to care which one a producer chose.
    #[test]
    fn a_named_property_list_yields_the_same_values() {
        let props = props_of(b"/P /MC0 BDC 0 0 2 2 re f EMC");
        let props = props[0].clone().expect("a property list");
        assert_eq!(props.mcid, Some(7));
        assert_eq!(props.alt.as_deref(), Some("a named list"));
    }

    /// A name the resources define as *optional content* and not as a
    /// property list answers the layer question and not this one, and the
    /// other way round. Neither lookup may stand in for the other.
    #[test]
    fn the_two_property_lookups_are_independent() {
        let d = &run_layers(b"/OC /Off BDC EMC /P /Plain BDC EMC");
        assert_eq!(
            begins(d)[0],
            (false, Some("Construction lines".to_string())),
            "/Off is a layer"
        );
        assert!(scope_props(d)[0].is_none(), "and carries no /MCID");
        assert_eq!(begins(d)[1], (true, None), "/Plain is not a layer");
        assert!(
            scope_props(d)[1].is_none(),
            "and is not a property list either"
        );
    }

    /// Every shape of malformed list: **no `/MCID`, and a visible scope.**
    ///
    /// The direction is the one already ruled for `/OC` — a list this build
    /// cannot read must not be able to hide content, and must not invent an
    /// identifier either, because an invented one joins content to the wrong
    /// structure element and reads as a reordering rather than as damage.
    #[test]
    fn a_malformed_property_list_yields_no_mcid_and_a_visible_scope() {
        let cases: &[&[u8]] = &[
            b"/P << >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID (three) >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID -1 >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID 2.5 >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID [1] >> BDC 0 0 2 2 re f EMC",
            // Unbalanced: no `<<` for the `>>` to match.
            b"/P /MCID 3 >> BDC 0 0 2 2 re f EMC",
            // A bare value run with no key at all.
            b"/P << 1 2 3 >> BDC 0 0 2 2 re f EMC",
            b"/P << /MCID /Three >> BDC 0 0 2 2 re f EMC",
            b"BDC 0 0 2 2 re f EMC",
        ];
        for source in cases {
            let d = run_layers(source);
            let what = String::from_utf8_lossy(source);
            assert_eq!(hidden_fills(&d), vec![false], "{what} should paint");
            assert_eq!(
                d.count(EventKind::BeginMarkedContent),
                1,
                "{what} should open one scope"
            );
            assert_eq!(
                scope_props(&d)[0].as_ref().and_then(|p| p.mcid),
                None,
                "{what} should name no marked sequence"
            );
        }
    }

    /// An `/MCID` nested inside a hidden `/OC` scope.
    ///
    /// The two are orthogonal: 8.11.3.2 decides whether the *renderer* paints
    /// and 14.7.4.2 decides what the *structure tree* claims, so a paragraph
    /// inside a layer that is switched off is still that paragraph. A build
    /// that let visibility gate the property list would drop a tagged
    /// document's alternate-language content out of its own reading order.
    #[test]
    fn a_property_list_survives_nesting_inside_a_hidden_layer() {
        let d = run_layers(
            b"/OC /Off BDC \
                /P << /MCID 4 >> BDC 0 0 2 2 re f EMC \
              EMC \
              /P << /MCID 5 >> BDC 0 0 3 3 re f EMC",
        );

        assert_eq!(
            begins(&d),
            vec![
                (false, Some("Construction lines".to_string())),
                (true, None),
                (true, None)
            ],
            "the layer hides, the two paragraphs do not"
        );
        assert_eq!(
            scope_props(&d)
                .iter()
                .map(|p| p.as_ref().and_then(|p| p.mcid))
                .collect::<Vec<_>>(),
            vec![None, Some(4), Some(5)],
            "the hidden scope carries no identifier and the nested one does"
        );
        assert_eq!(
            hidden_fills(&d),
            vec![true, false],
            "and only the painting was suppressed"
        );
        assert_eq!(d.count(EventKind::EndMarkedContent), 3);
    }

    /// 14.6.1: `BMC` has no property list at all, so it can carry no `/MCID`.
    #[test]
    fn a_bmc_carries_no_properties() {
        let d = run_layers(b"/Span BMC 0 0 2 2 re f EMC");
        assert_eq!(scope_tags(&d), vec![b"Span".to_vec()]);
        assert_eq!(scope_props(&d), vec![None]);
    }

    /// An `/Artifact` with an inline property list is an artifact.
    ///
    /// The tag lives before the `<<`, and the peek this replaced read the
    /// dictionary's last *value* instead — so an artifact written this way
    /// reported its tag as `Pagination`, and the text device, which excludes
    /// artifacts by tag (14.8.2.2), extracted a running head as though it were
    /// the author's own content.
    #[test]
    fn an_artifact_with_an_inline_list_is_still_tagged_artifact() {
        assert_eq!(
            tags_of(b"/Artifact << /Type /Pagination /BBox [0 0 9 9] >> BDC EMC"),
            vec!["Artifact"]
        );
    }

    /// `/Off` names a layer the configuration hides, `/On` one it shows, and
    /// `/Plain` is a property list that is not optional content at all.
    ///
    /// Three answers rather than two, because "hidden" and "not a layer" are
    /// different `None`s and a seam that collapsed them would hide every
    /// `/Span`.
    ///
    /// It also answers the *second* question the same `/Properties`
    /// sub-dictionary is asked (14.6.2): `/MC0` is a named property list
    /// carrying an `/MCID` and an `/Alt`, and `/Plain` deliberately answers
    /// neither question — which is what says the two lookups are independent
    /// rather than one lookup asked twice.
    struct Layers;

    impl FontSource for Layers {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn optional_content(&self, name: &[u8]) -> Option<Layer> {
            match name {
                b"Off" => Some(Layer {
                    visible: false,
                    label: "Construction lines".to_string(),
                }),
                b"On" => Some(Layer {
                    visible: true,
                    label: "Base".to_string(),
                }),
                _ => None,
            }
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            match name {
                // Opens a scope and never closes it, and closes one it never
                // opened. Both are what a real damaged form does.
                b"Frm" => Some(Form {
                    content: b"EMC /OC /Off BDC 0 0 5 5 re f".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0x1_0000,
                }),
                // A well-behaved form, used to test the `/OC` on the XObject
                // itself rather than anything in its content.
                b"HiddenFrm" | b"PlainFrm" => Some(Form {
                    content: b"BT /F0 10 Tf (form text) Tj ET 0 0 5 5 re f".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0x2_0000,
                }),
                // One form invoking another, each numbering a sequence 0.
                // 14.7.4.2 numbers within a stream, so the two are different
                // sequences and only the stream identity says so.
                b"OuterFrm" => Some(Form {
                    content: b"/P << /MCID 0 >> BDC 0 0 5 5 re f EMC /InnerFrm Do".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0x3_0000,
                }),
                b"InnerFrm" => Some(Form {
                    content: b"/P << /MCID 0 >> BDC 0 0 5 5 re f EMC".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0x4_0000,
                }),
                _ => None,
            }
        }
        fn xobject_optional_content(&self, name: &[u8]) -> Option<Layer> {
            (name == b"HiddenFrm" || name == b"HiddenImg").then(|| Layer {
                visible: false,
                label: "Construction lines".to_string(),
            })
        }
        fn marked_content_properties(&self, name: &[u8]) -> Option<MarkedProps> {
            // Only `/MC0` is a property list. `/Off`, `/On` and `/Plain` reach
            // here too and answer nothing, which is what makes the two
            // `/Properties` questions independent rather than one question
            // asked twice.
            (name == b"MC0").then(|| MarkedProps {
                mcid: Some(7),
                alt: Some("a named list".to_string()),
                ..MarkedProps::default()
            })
        }
    }

    fn run_layers(src: &[u8]) -> RecordingDevice {
        let mut d = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut d, &Layers);
        d
    }

    /// M2's first exit criterion: a nested `BDC` inside a hidden scope stays
    /// hidden.
    ///
    /// The inner scope names a layer that is *on*, so a build that reported
    /// only the innermost answer — or that let a nested `BDC` reset the
    /// state — would paint the middle rectangle. Three rectangles, one
    /// outside the scopes entirely, so "nothing painted" cannot pass either.
    #[test]
    fn a_scope_nested_inside_a_hidden_one_stays_hidden() {
        let d = run_layers(
            b"0 0 1 1 re f \
              /OC /Off BDC \
                0 0 2 2 re f \
                /OC /On BDC 0 0 3 3 re f EMC \
                /Span /Plain BDC 0 0 4 4 re f EMC \
              EMC \
              0 0 5 5 re f",
        );

        assert_eq!(hidden_fills(&d).len(), 5, "every fill reached the device");
        assert_eq!(
            hidden_fills(&d),
            vec![false, true, true, true, false],
            "the three inside the /Off scope are hidden, including the two \
             nested ones"
        );
        assert_eq!(painted(&d), 2);
        assert_eq!(d.count(EventKind::EndMarkedContent), 3, "one EMC per BDC");
    }

    /// M2's second exit criterion: an unbalanced `EMC` does not underflow.
    ///
    /// Four strays before anything opens, and two more after the one scope
    /// closes. A build that let them through would pop past the bottom of the
    /// device's stack; one that counted them would report more `end`s than
    /// `begin`s, which is the same defect one level up.
    #[test]
    fn a_stray_emc_is_dropped_rather_than_underflowing() {
        let d = run_layers(b"EMC EMC EMC EMC /OC /Off BDC 0 0 2 2 re f EMC EMC EMC 0 0 3 3 re f");

        assert_eq!(d.count(EventKind::BeginMarkedContent), 1);
        assert_eq!(
            d.count(EventKind::EndMarkedContent),
            1,
            "six of the seven EMCs had nothing to close"
        );
        assert_eq!(hidden_fills(&d), vec![true, false]);
    }

    /// 14.6.2: a stream may end inside a scope, and the device must not be
    /// left there. The page's own content is the outer case; a form XObject
    /// is the one that would otherwise reach out of itself.
    #[test]
    fn an_unclosed_scope_is_closed_at_the_end_of_its_stream() {
        let d = run_layers(b"/OC /Off BDC 0 0 2 2 re f");
        assert_eq!(d.count(EventKind::BeginMarkedContent), 1);
        assert_eq!(
            d.count(EventKind::EndMarkedContent),
            1,
            "the page's own trailing scope was closed"
        );
        assert!(d.open_scopes().is_empty());

        // `Frm` opens one scope it never closes and closes one it never
        // opened. Neither may reach the page: the fill after the form is
        // outside every layer.
        let d = run_layers(b"/Frm Do 0 0 9 9 re f");
        assert_eq!(
            hidden_fills(&d),
            vec![true, false],
            "the form's own fill is hidden and the page's is not"
        );
        assert_eq!(d.count(EventKind::BeginMarkedContent), 1);
        assert_eq!(d.count(EventKind::EndMarkedContent), 1);
    }

    /// 8.11.3.2: only an `/OC` tag selects optional content.
    #[test]
    fn a_tag_that_is_not_oc_hides_nothing() {
        // `/Off` is a hidden layer, but the tag here is `/Span`, so the name
        // is an ordinary property list and the fill paints.
        let d = run_layers(b"/Span /Off BDC 0 0 2 2 re f EMC");
        assert_eq!(hidden_fills(&d), vec![false]);
        assert_eq!(begins(&d), vec![(true, None)]);
    }

    /// Ruling 10: the scope that hides carries the layer's name, so a device
    /// can say what it hid rather than only that something was hidden.
    #[test]
    fn a_hidden_scope_names_its_layer() {
        let d = run_layers(b"/OC /Off BDC 0 0 2 2 re f EMC");
        assert_eq!(
            begins(&d),
            vec![(false, Some("Construction lines".to_string()))]
        );
    }

    /// An inline property list (7.8.2 tokenizes it flat) renders.
    ///
    /// 7.3.10 keeps indirect references out of content streams and 8.11.3.2
    /// makes an `/OC` entry a reference, so an inline list cannot name a
    /// layer. The tag still has to be found, though — it sits before the
    /// `<<`, not before the operator — and a malformed one must not turn into
    /// a hidden scope. Every case here paints.
    #[test]
    fn an_inline_property_list_renders() {
        for src in [
            b"/OC << /Type /OCMD >> BDC 0 0 2 2 re f EMC".as_slice(),
            b"/OC << /OCGs [ ] /P /AllOff >> BDC 0 0 2 2 re f EMC",
            b"/OC << /A << /B 1 >> >> BDC 0 0 2 2 re f EMC",
            // Unbalanced: no `<<` for the `>>` to match.
            b"/OC /Type /OCMD >> BDC 0 0 2 2 re f EMC",
            // No tag at all.
            b"<< /Type /OCMD >> BDC 0 0 2 2 re f EMC",
            b"BDC 0 0 2 2 re f EMC",
        ] {
            let d = run_layers(src);
            assert_eq!(
                hidden_fills(&d),
                vec![false],
                "{} should paint",
                String::from_utf8_lossy(src)
            );
        }
    }

    /// The operators that used to fall into the catch-all, in one stream that
    /// also draws. None of them opens a scope, and none of them may swallow
    /// the operators after it.
    #[test]
    fn the_remaining_marked_content_operators_are_no_ops() {
        let d = run_layers(
            b"BX /Weird1 op EX \
              /Point MP \
              /Point << /K 1 >> DP \
              BMC 0 0 2 2 re f EMC \
              0 0 3 3 re f",
        );

        assert_eq!(
            d.count(EventKind::BeginMarkedContent),
            1,
            "only the BMC opened a scope"
        );
        assert_eq!(begins(&d), vec![(true, None)]);
        assert_eq!(d.count(EventKind::EndMarkedContent), 1);
        assert_eq!(hidden_fills(&d), vec![false, false], "both fills painted");
    }

    /// Hidden content still runs. The pen advances, `q`/`Q` balance, and the
    /// glyphs reach the device — the whole reason suppression belongs at the
    /// paint rather than here.
    #[test]
    fn a_hidden_scope_still_interprets_everything_inside_it() {
        let d = run_layers(
            b"/OC /Off BDC q 2 0 0 2 0 0 cm BT /F0 10 Tf (AB) Tj ET Q EMC \
              BT /F0 10 Tf (C) Tj ET",
        );

        let text: String = d.glyphs().map(|g| g.text.as_str()).collect();
        assert_eq!(text, "ABC", "the hidden glyphs were still shown");
        assert_eq!(
            d.indices(EventKind::ShowGlyph)
                .map(|index| d.hidden_at(index))
                .collect::<Vec<_>>(),
            vec![true, true, false],
            "and only their painting was marked hidden"
        );
    }

    /// 8.11.4.4: an `/OC` on the XObject itself hides it, and does so through
    /// the same scope machinery a `BDC` uses — so a hidden form is still run
    /// and its text still reaches the device.
    ///
    /// `PlainFrm` is the decoy: the same content, no `/OC`, and it paints.
    #[test]
    fn an_oc_entry_on_an_xobject_hides_it_without_skipping_it() {
        let d = run_layers(b"/PlainFrm Do /HiddenFrm Do 0 0 9 9 re f");

        assert_eq!(
            hidden_fills(&d),
            vec![false, true, false],
            "the plain form paints, the hidden one does not, and the page \
             after both does"
        );
        let text: String = d.glyphs().map(|g| g.text.as_str()).collect();
        assert_eq!(
            text, "form textform text",
            "the hidden form was still interpreted"
        );
        assert_eq!(
            begins(&d),
            vec![(false, Some("Construction lines".to_string()))],
            "one scope, opened around the `Do` rather than inside the form"
        );
        assert_eq!(d.count(EventKind::EndMarkedContent), 1);
        assert!(d.open_scopes().is_empty());
    }

    /// An image XObject is the branch where `form` returns `None`, so it is
    /// asserted separately: a hidden one is wrapped in a hidden scope, and a
    /// device that honours the scope never decodes it to find out it cannot.
    ///
    /// **This test used to be named `…_is_not_handed_to_the_device`, and the
    /// device it drove claimed in a comment to record "whether it was asked at
    /// all, which is the stronger property".** It did not: its `draw_image`
    /// returned early while a scope was hiding, so it recorded what it *drew*.
    /// `run_xobject` calls `draw_image` unconditionally — the `/OC` on the
    /// XObject opens a scope around the `Do` and nothing else — so the hidden
    /// image *is* handed over, and the claim was false about the code beside
    /// it. The promoted recorder makes the difference visible rather than
    /// absorbing it, because it keeps the call and derives the suppression;
    /// the last assertion is the property the comment was reaching for, now
    /// stated in the direction that is true.
    #[test]
    fn a_hidden_image_xobject_arrives_inside_a_hidden_scope() {
        let mut d = RecordingDevice::new();
        interpret(
            b"/PlainImg Do /HiddenImg Do",
            Matrix::IDENTITY,
            &mut d,
            &Layers,
        );

        let drawn: Vec<Vec<u8>> = d
            .indices(EventKind::DrawImage)
            .filter(|index| !d.hidden_at(*index))
            .filter_map(|index| match &d.events()[index] {
                Event::DrawImage { image, .. } => Some(image.name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(drawn, vec![b"PlainImg".to_vec()]);
        assert!(d.open_scopes().is_empty());

        let offered: Vec<Vec<u8>> = d
            .of_kind(EventKind::DrawImage)
            .filter_map(|event| match event {
                Event::DrawImage { image, .. } => Some(image.name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            offered,
            vec![b"PlainImg".to_vec(), b"HiddenImg".to_vec()],
            "both were handed over; only the scope around the second one \
             tells a device not to decode it"
        );
    }

    /// Depth is bounded, and running out of it shows content rather than
    /// hiding it — the direction a hostile stream should fail in.
    ///
    /// The arrangement is the one that can tell a build that *counts* the
    /// scopes it refused from one that silently drops them. A hidden scope
    /// is opened first, the cap is then filled with visible ones, and three
    /// more are opened past it. The three `EMC`s that follow belong to those
    /// three; a build that let them close reported scopes instead would be
    /// three short by the time the unwinding reaches the outer hidden one,
    /// close it early, and paint the last rectangle. Every simpler ordering
    /// — nest to the cap, then unwind completely — gives the same pixels
    /// either way, which is how this test came to be written twice.
    #[test]
    fn runaway_nesting_is_bounded_and_stays_balanced() {
        let cap = MAX_MARKED_CONTENT_DEPTH as usize;
        let mut src = Vec::from(b"/OC /Off BDC ".as_slice());
        for _ in 0..cap - 1 {
            src.extend_from_slice(b"/OC /On BDC ");
        }
        for _ in 0..3 {
            src.extend_from_slice(b"/OC /Off BDC ");
        }
        src.extend_from_slice(b"0 0 2 2 re f ");
        // The three that belong to the scopes past the cap.
        src.extend_from_slice(b"EMC EMC EMC ");
        // Then the cap's worth, less the outermost.
        for _ in 0..cap - 1 {
            src.extend_from_slice(b"EMC ");
        }
        src.extend_from_slice(b"0 0 3 3 re f EMC 0 0 4 4 re f");

        let d = run_layers(&src);
        assert_eq!(d.count(EventKind::BeginMarkedContent), cap, "the cap held");
        assert_eq!(
            d.count(EventKind::BeginMarkedContent),
            d.count(EventKind::EndMarkedContent),
            "and every one was closed"
        );
        assert!(d.open_scopes().is_empty());
        assert_eq!(
            hidden_fills(&d),
            vec![true, true, false],
            "the fill inside is hidden; so is the one after the unwinding, \
             because the outermost scope is still open; only the fill after \
             its own EMC paints"
        );
        assert!(
            begins(&d).iter().filter(|(visible, _)| !*visible).count() == 1,
            "the three scopes past the cap were refused, and a refused scope \
             is a visible one"
        );
    }

    // -----------------------------------------------------------------------
    // Kept form recordings (`docs/design/performance.md`, P-16).
    // -----------------------------------------------------------------------

    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// How a [`Keeping`] source heard each form drawn.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Heard {
        Replayed,
        Interpreted,
        Recorded,
        Spoiled,
    }

    /// A source of forms that keeps their drawings, up to `keeps` bytes each,
    /// by name and invocation, and remembers how each was drawn. With `keeps`
    /// at zero it keeps none and is asked nothing, as every source that does
    /// not opt in.
    struct Keeping {
        keeps: usize,
        kept: RefCell<HashMap<(Vec<u8>, FormInvocation), Arc<FormRecording>>>,
        heard: RefCell<Vec<(Vec<u8>, Heard)>>,
    }

    impl Keeping {
        fn new(keeps: usize) -> Keeping {
            Keeping {
                keeps,
                kept: RefCell::new(HashMap::new()),
                heard: RefCell::new(Vec::new()),
            }
        }

        /// How many times `name` was drawn `how`.
        fn heard(&self, name: &[u8], how: Heard) -> usize {
            self.heard
                .borrow()
                .iter()
                .filter(|(heard, h)| heard == name && *h == how)
                .count()
        }

        /// How every form was drawn, in order.
        fn hows(&self) -> Vec<Heard> {
            self.heard.borrow().iter().map(|(_, how)| *how).collect()
        }
    }

    /// A form of `content` with identity `stream`, its box `[0 0 10 10]`.
    fn kept_form_of(content: &[u8], stream: u64, group: Option<Group>) -> Form {
        Form {
            content: content.to_vec(),
            matrix: Matrix::IDENTITY,
            bbox: Some([0.0, 0.0, 10.0, 10.0]),
            group,
            stream,
        }
    }

    /// The forms every kept-form test draws, by name.
    fn kept_forms(name: &[u8]) -> Option<Form> {
        let isolated = Some(Group {
            isolated: true,
            knockout: false,
            space: None,
        });
        Some(match name {
            // A fill in the inherited colour and a stroke at the inherited
            // width, through the inherited pen.
            b"Fm" => kept_form_of(b"0 0 5 5 re f 1 1 m 4 4 l S", 7, None),
            b"Grp" => kept_form_of(b"0 0 1 rg 1 1 3 3 re f", 8, isolated),
            b"Outer" => kept_form_of(b"/Fm Do 2 0 0 2 0 0 cm /Fm Do", 9, None),
            b"Masked" => kept_form_of(b"/Mask gs 0 0 8 8 re f", 10, None),
            b"Self" => kept_form_of(b"/Self Do 0 0 1 1 re f", 12, None),
            b"Anon" => kept_form_of(b"0 0 2 2 re f", 0, None),
            // Text in the inherited font, size and text matrix.
            b"Text" => kept_form_of(b"(ab) Tj", 13, None),
            b"Pair" => kept_form_of(b"/Grp Do /Grp Do", 14, None),
            // A fan of forms, eight wide and five deep.
            b"F0" => kept_form_of(
                b"/F1 Do /F1 Do /F1 Do /F1 Do /F1 Do /F1 Do /F1 Do /F1 Do",
                20,
                None,
            ),
            b"F1" => kept_form_of(
                b"/F2 Do /F2 Do /F2 Do /F2 Do /F2 Do /F2 Do /F2 Do /F2 Do",
                21,
                None,
            ),
            b"F2" => kept_form_of(
                b"/F3 Do /F3 Do /F3 Do /F3 Do /F3 Do /F3 Do /F3 Do /F3 Do",
                22,
                None,
            ),
            b"F3" => kept_form_of(
                b"/F4 Do /F4 Do /F4 Do /F4 Do /F4 Do /F4 Do /F4 Do /F4 Do",
                23,
                None,
            ),
            b"F4" => kept_form_of(b"0 0 1 1 re f", 24, None),
            _ => return None,
        })
    }

    impl FontSource for Keeping {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|&b| (u32::from(b), char::from(b).to_string(), 500.0))
                .collect()
        }
        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 880.0, -1000.0)
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            kept_forms(name)
        }
        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            match name {
                b"Half" => Some(Arc::new(StateParameters {
                    fill_alpha: Some(0.5),
                    blend: Some(BlendMode::Multiply),
                    ..StateParameters::default()
                })),
                b"Mask" => Some(Arc::new(StateParameters {
                    soft_mask: Some(SoftMask::Group(Box::new(MaskGroup {
                        form: kept_form_of(b"1 g 0 0 4 4 re f", 11, None),
                        luminosity: true,
                        backdrop: None,
                        transfer: None,
                    }))),
                    ..StateParameters::default()
                })),
                b"Identity" => Some(Arc::new(StateParameters {
                    transfer: Some(Some(Transfer::Identity)),
                    ..StateParameters::default()
                })),
                _ => None,
            }
        }
        fn form_recording_bytes(&self) -> usize {
            self.keeps
        }
        fn kept_form(
            &self,
            name: &[u8],
            invocation: &FormInvocation,
        ) -> Option<Arc<FormRecording>> {
            self.kept
                .borrow()
                .get(&(name.to_vec(), invocation.clone()))
                .cloned()
        }
        fn form_drawn(&self, name: &[u8], drawn: FormDrawn) {
            let how = match drawn {
                FormDrawn::Replayed => Heard::Replayed,
                FormDrawn::Interpreted => Heard::Interpreted,
                FormDrawn::Recorded {
                    invocation,
                    recording: Some(recording),
                } => {
                    assert!(invocation.written_len() > 0, "a key writes the state");
                    assert!(recording.kept_bytes() >= recording.events().len());
                    self.kept
                        .borrow_mut()
                        .insert((name.to_vec(), invocation), Arc::new(recording));
                    Heard::Recorded
                }
                FormDrawn::Recorded {
                    recording: None, ..
                } => Heard::Spoiled,
            };
            self.heard.borrow_mut().push((name.to_vec(), how));
        }
    }

    /// `content` interpreted into a recorder answering `answers`, over
    /// `fonts`: every call the device was handed.
    fn kept_drawing(content: &[u8], fonts: &Keeping, answers: Answers) -> Vec<Event> {
        let mut device = RecordingDevice::new().answering(answers);
        interpret(content, Matrix::IDENTITY, &mut device, fonts);
        device.into_events()
    }

    /// Two transcripts are the same calls with the same payloads, the states
    /// included. `Event` holds `f64`s and is compared by its debug form.
    #[track_caller]
    fn same_calls(a: &[Event], b: &[Event], what: &str) {
        assert_eq!(a.len(), b.len(), "{what}: the same number of calls");
        for (index, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert_eq!(format!("{x:?}"), format!("{y:?}"), "{what}: call {index}");
        }
    }

    fn accepting() -> Answers {
        Answers {
            forms: true,
            groups: true,
            soft_masks: true,
            cancelled: false,
        }
    }

    /// The pages every kept-form test draws: one form invoked under three
    /// transforms, the last the first's again; under inherited colours and
    /// line widths; nested inside another; a transparency group under a
    /// state's alpha and blend mode; a form whose content sets a soft mask;
    /// and text in an inherited font, size and text matrix.
    const KEPT_PAGES: [&[u8]; 6] = [
        b"/Fm Do 1 0 0 1 20 0 cm /Fm Do 1 0 0 1 -20 0 cm /Fm Do",
        b"1 0 0 rg 3 w /Fm Do 0 1 0 rg /Fm Do 1 0 0 rg /Fm Do",
        b"2 w /Outer Do /Outer Do",
        b"/Half gs /Grp Do /Grp Do",
        b"/Masked Do /Masked Do",
        b"BT /F1 12 Tf 5 5 Td ET /Text Do /Text Do",
    ];

    /// **A kept form recording replays as the form interprets**: every
    /// page, interpreted twice over a source that keeps forms' drawings —
    /// the first time recording each invocation it has not kept and
    /// replaying the repeats, the second replaying every one — gives the
    /// device the calls an interpretation over a source that keeps nothing
    /// gives, event for event and state for state, with a recorder that
    /// declines groups and masks and with one that accepts them.
    #[test]
    fn a_kept_form_recording_replays_as_the_form_interprets() {
        for answers in [Answers::default(), accepting()] {
            for page in KEPT_PAGES {
                let what = format!("{} with {answers:?}", String::from_utf8_lossy(page));
                let fresh = kept_drawing(page, &Keeping::new(0), answers);
                assert!(!fresh.is_empty(), "{what}: the page draws");
                let keeping = Keeping::new(1 << 20);
                let first = kept_drawing(page, &keeping, answers);
                same_calls(&first, &fresh, &format!("{what}, recording"));
                let second = kept_drawing(page, &keeping, answers);
                same_calls(&second, &fresh, &format!("{what}, replaying"));
                assert!(
                    keeping.hows().contains(&Heard::Replayed),
                    "{what}: something was replayed: {:?}",
                    keeping.hows()
                );
                assert!(
                    !keeping.hows().contains(&Heard::Spoiled),
                    "{what}: nothing was spoiled: {:?}",
                    keeping.hows()
                );
            }
        }
    }

    /// **A kept form is interpreted once per key**: five invocations under
    /// one state record one drawing and replay it four times; three
    /// transforms invoked twice each are three keys, each interpreted once;
    /// and a later interpretation over the same source interprets none.
    #[test]
    fn a_kept_form_is_interpreted_once_per_key() {
        let keeping = Keeping::new(1 << 20);
        kept_drawing(
            b"/Fm Do /Fm Do /Fm Do /Fm Do /Fm Do",
            &keeping,
            Answers::default(),
        );
        assert_eq!(keeping.heard(b"Fm", Heard::Recorded), 1);
        assert_eq!(keeping.heard(b"Fm", Heard::Replayed), 4);

        let keeping = Keeping::new(1 << 20);
        let thrice = b"q /Fm Do Q q 2 0 0 2 0 0 cm /Fm Do Q q 3 0 0 3 0 0 cm /Fm Do Q";
        let twice = [thrice.as_slice(), b" ", thrice.as_slice()].concat();
        kept_drawing(&twice, &keeping, Answers::default());
        assert_eq!(
            keeping.heard(b"Fm", Heard::Recorded),
            3,
            "a key a transform"
        );
        assert_eq!(keeping.heard(b"Fm", Heard::Replayed), 3);
        kept_drawing(thrice, &keeping, Answers::default());
        assert_eq!(
            keeping.heard(b"Fm", Heard::Recorded),
            3,
            "none recorded again"
        );
        assert_eq!(keeping.heard(b"Fm", Heard::Replayed), 6);

        // A replay reads no token of the form: a counted interpretation of a
        // page whose forms are all kept reads the page's own tokens alone.
        let work = Work::new();
        let mut device = RecordingDevice::new();
        interpret_counted(thrice, Matrix::IDENTITY, &mut device, &keeping, &work);
        let fresh = Work::new();
        let mut plain = RecordingDevice::new();
        interpret_counted(
            thrice,
            Matrix::IDENTITY,
            &mut plain,
            &Keeping::new(0),
            &fresh,
        );
        let form_tokens = 13 * 3;
        assert_eq!(
            work.stats().content_tokens_read + form_tokens,
            fresh.stats().content_tokens_read,
            "the three forms' tokens are not read"
        );
        same_calls(device.events(), plain.events(), "the counted replay");
    }

    /// A nested form inside a kept one is replayed with it, and is a key of
    /// its own where it is drawn: inside its outer form, at a depth and inside
    /// the streams around it that the page's own invocation is not.
    #[test]
    fn a_nested_form_is_kept_inside_its_outer_form_and_for_itself() {
        let keeping = Keeping::new(1 << 20);
        let page = b"/Outer Do /Outer Do /Fm Do /Fm Do";
        let first = kept_drawing(page, &keeping, Answers::default());
        assert_eq!(keeping.heard(b"Outer", Heard::Recorded), 1);
        assert_eq!(keeping.heard(b"Outer", Heard::Replayed), 1);
        // The outer form's first drawing records its two inner invocations,
        // under two transforms, and its replay replays them inside it without
        // asking for them; the page's own two invocations are one key more.
        assert_eq!(keeping.heard(b"Fm", Heard::Recorded), 3);
        assert_eq!(keeping.heard(b"Fm", Heard::Replayed), 1);
        same_calls(
            &first,
            &kept_drawing(page, &Keeping::new(0), Answers::default()),
            "the page",
        );
    }

    /// **What a page leaves behind it and a form sets for itself is not in
    /// the key**: a form whose text sets its own text matrix and whose path
    /// begins with its own `m`, invoked after text objects and paths that
    /// leave the text matrix and the current point in three places, is one
    /// key; and a form that reads the text matrix or the current point it is
    /// invoked with is replayed only where they are what they were.
    #[test]
    fn the_text_matrix_and_the_current_point_count_only_where_read() {
        struct Reading(Keeping);
        impl FontSource for Reading {
            fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
                self.0.decode(font, bytes)
            }
            fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
                self.0.vertical_metrics(font, code)
            }
            fn form(&self, name: &[u8]) -> Option<Form> {
                Some(match name {
                    // Its own text object and its own path.
                    b"Own" => kept_form_of(b"BT /F1 9 Tf 1 1 Td (x) Tj ET 1 1 m 3 3 l S", 30, None),
                    // Text in the matrix it is invoked with.
                    b"Shows" => kept_form_of(b"/F1 9 Tf (x) Tj", 31, None),
                    // A curve from the point it is invoked with.
                    b"Curves" => kept_form_of(b"5 5 6 6 v S", 32, None),
                    _ => return None,
                })
            }
            fn form_recording_bytes(&self) -> usize {
                self.0.form_recording_bytes()
            }
            fn kept_form(
                &self,
                name: &[u8],
                invocation: &FormInvocation,
            ) -> Option<Arc<FormRecording>> {
                self.0.kept_form(name, invocation)
            }
            fn form_drawn(&self, name: &[u8], drawn: FormDrawn) {
                self.0.form_drawn(name, drawn);
            }
        }
        let drawing = |page: &[u8], fonts: &Reading| {
            let mut device = RecordingDevice::new();
            interpret(page, Matrix::IDENTITY, &mut device, fonts);
            device.into_events()
        };
        let leave = |x: u32| {
            format!(
                "BT /F1 12 Tf {x} {x} Td (a) Tj ET {x} {x} m {} 0 l S ",
                x + 1
            )
        };

        let own: String = (1..=3).map(|x| leave(x) + "/Own Do ").collect();
        let reading = Reading(Keeping::new(1 << 20));
        let drawn = drawing(own.as_bytes(), &reading);
        assert_eq!(reading.0.heard(b"Own", Heard::Recorded), 1, "one key");
        assert_eq!(reading.0.heard(b"Own", Heard::Replayed), 2);
        same_calls(
            &drawn,
            &drawing(own.as_bytes(), &Reading(Keeping::new(0))),
            "/Own",
        );

        for name in ["Shows", "Curves"] {
            // The same left behind twice, then somewhere else: the first two
            // are one drawing, the third reads what it was not.
            let page: String = [1, 1, 2]
                .into_iter()
                .map(|x| leave(x) + &format!("/{name} Do "))
                .collect();
            let reading = Reading(Keeping::new(1 << 20));
            let drawn = drawing(page.as_bytes(), &reading);
            assert_eq!(
                reading.0.heard(name.as_bytes(), Heard::Recorded),
                1,
                "{name}"
            );
            assert_eq!(
                reading.0.heard(name.as_bytes(), Heard::Replayed),
                1,
                "{name}"
            );
            assert_eq!(
                reading.0.heard(name.as_bytes(), Heard::Interpreted),
                1,
                "{name}: invoked elsewhere, it is drawn again"
            );
            same_calls(
                &drawn,
                &drawing(page.as_bytes(), &Reading(Keeping::new(0))),
                name,
            );
        }
    }

    /// **A drawing that depends on more than its key is never kept**: a form
    /// that draws itself is refused for its cycle, and is interpreted at
    /// every invocation, its calls those of a source that keeps nothing.
    #[test]
    fn a_form_refused_for_a_cycle_inside_is_never_kept() {
        let keeping = Keeping::new(1 << 20);
        let page = b"/Self Do /Self Do";
        let drawn = kept_drawing(page, &keeping, Answers::default());
        assert_eq!(keeping.heard(b"Self", Heard::Spoiled), 2);
        assert_eq!(keeping.heard(b"Self", Heard::Recorded), 0);
        same_calls(
            &drawn,
            &kept_drawing(page, &Keeping::new(0), Answers::default()),
            "the cycle",
        );
    }

    /// A recording past what the source may keep is not offered, and the
    /// form is drawn as an interpretation draws it.
    #[test]
    fn a_recording_past_its_bound_is_not_offered() {
        let keeping = Keeping::new(16);
        let page = b"/Fm Do /Fm Do";
        let drawn = kept_drawing(page, &keeping, Answers::default());
        assert_eq!(keeping.hows(), [Heard::Spoiled, Heard::Spoiled]);
        same_calls(
            &drawn,
            &kept_drawing(page, &Keeping::new(0), Answers::default()),
            "too large to keep",
        );
    }

    /// **No key, no recording**: a form reached with no identity, and a form
    /// invoked under a stored transfer function, which no key writes, are
    /// interpreted every time and their calls are an interpretation's.
    #[test]
    fn a_form_with_no_key_is_interpreted() {
        for page in [
            b"/Anon Do /Anon Do".as_slice(),
            b"/Identity gs /Fm Do /Fm Do",
        ] {
            let keeping = Keeping::new(1 << 20);
            let drawn = kept_drawing(page, &keeping, Answers::default());
            assert_eq!(keeping.hows(), [Heard::Interpreted, Heard::Interpreted]);
            same_calls(
                &drawn,
                &kept_drawing(page, &Keeping::new(0), Answers::default()),
                &String::from_utf8_lossy(page),
            );
        }
    }

    /// **A recording is replayed only to a device that answers as it was
    /// made**: a group form recorded by a device that accepts groups is
    /// interpreted again for one that declines them, and that device is
    /// handed what an interpretation hands it.
    #[test]
    fn a_device_that_would_answer_otherwise_is_handed_an_interpretation() {
        let keeping = Keeping::new(1 << 20);
        kept_drawing(b"/Grp Do", &keeping, accepting());
        assert_eq!(keeping.heard(b"Grp", Heard::Recorded), 1);
        let declined = kept_drawing(b"/Grp Do", &keeping, Answers::default());
        assert_eq!(keeping.heard(b"Grp", Heard::Interpreted), 1);
        same_calls(
            &declined,
            &kept_drawing(b"/Grp Do", &Keeping::new(0), Answers::default()),
            "declining groups",
        );
        // And the default device's own answers never replay what a source
        // keeps: a device that cannot say how it would answer is never
        // handed a replay.
        struct Plain(Vec<u8>);
        impl Device for Plain {
            fn fill_path(&mut self, _: &[PathSegment], _: &GraphicsState, _: bool) {
                self.0.push(b'f');
            }
        }
        let mut plain = Plain(Vec::new());
        interpret(b"/Grp Do", Matrix::IDENTITY, &mut plain, &keeping);
        assert_eq!(plain.0, b"f", "the group's fill, interpreted");
        assert_eq!(keeping.heard(b"Grp", Heard::Interpreted), 2);
    }

    /// A device that is cancelled while a form draws spoils its recording:
    /// a drawing cut short is never kept.
    #[test]
    fn a_cancelled_drawing_is_never_kept() {
        struct CancelAfter {
            inner: RecordingDevice,
            calls: Cell<usize>,
            after: usize,
        }
        impl Device for CancelAfter {
            fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
                self.calls.set(self.calls.get() + 1);
                self.inner.fill_path(path, state, even_odd);
            }
            fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
                self.inner.clip_path(path, state, even_odd);
            }
            fn is_cancelled(&self) -> bool {
                self.calls.get() >= self.after
            }
            fn answers_alike(&self, events: &[Event]) -> bool {
                self.inner.answers_alike(events)
            }
        }
        let keeping = Keeping::new(1 << 20);
        let mut device = CancelAfter {
            inner: RecordingDevice::new(),
            calls: Cell::new(0),
            after: 1,
        };
        interpret(b"/Fm Do /Fm Do", Matrix::IDENTITY, &mut device, &keeping);
        assert_eq!(
            keeping.heard(b"Fm", Heard::Spoiled),
            1,
            "{:?}",
            keeping.hows()
        );
        assert!(keeping.kept.borrow().is_empty(), "nothing kept");
    }

    /// **A recording that would pass a budget is interpreted**: a form that
    /// draws two transparency groups, kept from early in a page, is invoked
    /// again with one group run left of [`MAX_GROUP_RUNS`]; its recording's
    /// two runs do not fit, so it is interpreted, and the page is handed
    /// exactly what an interpretation hands it — the second group refused
    /// and the budget said spent.
    #[test]
    fn a_recording_that_would_pass_a_budget_is_interpreted() {
        let mut page = b"/Pair Do ".to_vec();
        for _ in 0..(MAX_GROUP_RUNS as usize - 3) {
            page.extend_from_slice(b"/Grp Do ");
        }
        page.extend_from_slice(b"/Pair Do");
        let keeping = Keeping::new(1 << 20);
        let drawn = kept_drawing(&page, &keeping, accepting());
        assert_eq!(keeping.heard(b"Pair", Heard::Recorded), 1);
        assert_eq!(
            keeping.heard(b"Pair", Heard::Interpreted),
            1,
            "the second Pair does not fit: {:?}",
            keeping.heard(b"Pair", Heard::Replayed)
        );
        let fresh = kept_drawing(&page, &Keeping::new(0), accepting());
        assert!(
            fresh
                .iter()
                .any(|event| matches!(event, Event::GroupBudgetSpent)),
            "the page runs out of group runs"
        );
        same_calls(&drawn, &fresh, "the page at the group budget");
    }

    /// **A recording replays only where what it spent fits what is left**,
    /// checked against the nested-content budget and the group budget as
    /// the interpretation stands, a stream counted as reached only the first
    /// time; and a replay spends what it records, and reads and sets what its
    /// drawing read and set.
    #[test]
    fn a_recording_replays_only_within_the_budgets_left() {
        let keeping = Keeping::new(1 << 20);
        let mut device = RecordingDevice::new();
        let mut interp = Interpreter::new(&mut device, &keeping, Matrix::IDENTITY, 0);
        let recording = |spent: Vec<Spent>, needs: Needs| FormRecording {
            events: Vec::new(),
            spent,
            after: After {
                current: (7.0, 8.0),
                start: (7.0, 8.0),
                pending_clip: None,
                set_current: true,
                set_start: true,
            },
            needs,
            bytes: 0,
        };
        let nested = recording(
            vec![
                Spent::NestedCheck {
                    stream: 99,
                    len: 10,
                },
                Spent::NestedSpend { len: 10 },
            ],
            Needs::default(),
        );
        assert!(interp.replays(&nested), "room to spare");
        // Nearly spent: reaching stream 99 for the first time raises the
        // budget by its length at the rate, so the run fits.
        interp.nested_spent = NESTED_WORK_FLOOR - 100;
        assert!(interp.replays(&nested));
        // Reached already, the run raises nothing and does not fit.
        interp.reached_streams.insert(99);
        assert!(!interp.replays(&nested), "the nested budget is passed");
        interp.nested_spent = 0;

        let grouped = recording(vec![Spent::GroupCheck, Spent::GroupRun], Needs::default());
        assert!(interp.replays(&grouped));
        interp.group_runs = MAX_GROUP_RUNS - 1;
        assert!(interp.replays(&grouped), "one run left is enough for one");
        let twice = recording(
            vec![
                Spent::GroupCheck,
                Spent::GroupRun,
                Spent::GroupCheck,
                Spent::GroupRun,
            ],
            Needs::default(),
        );
        assert!(!interp.replays(&twice), "and not for two");
        interp.group_runs = 0;

        // What the drawing read must be what it was.
        let reads = recording(
            Vec::new(),
            Needs {
                current: Some((1.0, 2.0)),
                ..Needs::default()
            },
        );
        assert!(!interp.replays(&reads), "the point is (0, 0)");
        interp.current = (1.0, 2.0);
        assert!(interp.replays(&reads));
        let negative = recording(
            Vec::new(),
            Needs {
                current: Some((-0.0, 2.0)),
                ..Needs::default()
            },
        );
        interp.current = (0.0, 2.0);
        assert!(!interp.replays(&negative), "a point is its bits");

        // A replay spends what the recording spent, and leaves the point
        // where the drawing left it.
        interp.replay_form(&nested);
        assert_eq!(interp.nested_spent, 10 + NESTED_RUN_COST);
        interp.replay_form(&grouped);
        assert_eq!(interp.group_runs, 1);
        assert_eq!((interp.current, interp.start), ((7.0, 8.0), (7.0, 8.0)));
    }
}
