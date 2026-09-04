//! The `Device` seam (ruling 7).
//!
//! Everything that consumes a content stream implements this: the text device
//! here, the rasterizing device in `tinker-pdf-render`, and any future one.
//! The interpreter never knows which it is talking to, which is exactly why
//! text extraction needs no rasterizer and Checkpoint A can exist.

use crate::interpret::{Group, MaskGroup};
use crate::state::{GraphicsState, Matrix};

/// The plain values a `BDC`'s property list carries (14.6.2, 14.9).
///
/// **No COS types, and no bytes that still need a document to read them.**
/// The two forms a property list takes — an inline `<< … >>` the tokenizer
/// flattened, and a name into the page's `/Properties` — are resolved on
/// opposite sides of this crate's boundary, and a struct of plain values is
/// what lets them arrive indistinguishable. A device cannot tell which form
/// wrote the `/MCID` it is handed, and 14.6.2 gives it no reason to.
///
/// Every field is `Option` because every field is optional: 14.7.4.2 makes
/// `/MCID` the only entry that associates content with structure, and 14.9's
/// four are alternate descriptions a producer supplies or does not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkedProps {
    /// `/MCID`, the marked-content identifier (14.7.4.2).
    ///
    /// Non-negative and integral, because that is what 14.7.4.2 says it is;
    /// anything else in the slot is read as absent rather than rounded, since
    /// an invented identifier joins content to the wrong structure element
    /// and reads as a reordering rather than as damage.
    pub mcid: Option<u32>,
    /// `/ActualText`: what this content *is*, replacing what it draws
    /// (14.9.4). Two glyphs of a ligature spelled out, a soft hyphen removed.
    pub actual_text: Option<String>,
    /// `/Alt`: a description of content that is not text (14.9.3).
    pub alt: Option<String>,
    /// `/Lang`: the natural language of the content (14.9.2).
    pub lang: Option<String>,
    /// `/E`: what an abbreviation stands for (14.9.5).
    pub expansion: Option<String>,
    /// Which content stream the `BDC` that opened this sequence was written
    /// in: a form XObject's packed indirect reference, or `0` for the page's
    /// own stream.
    ///
    /// **Not read from the property list.** 14.7.4.2 numbers marked-content
    /// sequences within a content stream, so `/MCID 0` in one form and
    /// `/MCID 0` in another are two sequences and not one; the identifier
    /// alone cannot say which. Only the interpreter knows which stream it is
    /// running, so it stamps this on the way past — which is why it is the
    /// one field here a producer did not write.
    pub stream: u64,
}

impl MarkedProps {
    /// Whether the property list said nothing this engine reads.
    ///
    /// A `BDC` whose list is a `/Type` and a `/BBox` and nothing else is not
    /// the same as a `BDC` with no list at all to a producer, but it is to
    /// every consumer here — so the two are collapsed at the seam rather
    /// than at each device.
    ///
    /// [`MarkedProps::stream`] is deliberately not one of the fields checked:
    /// it is stamped by the interpreter *after* this decides whether the list
    /// was worth keeping, and counting it would make every `BDC` in a form
    /// non-empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mcid.is_none()
            && self.actual_text.is_none()
            && self.alt.is_none()
            && self.lang.is_none()
            && self.expansion.is_none()
    }
}

/// One glyph, as the interpreter resolved it.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// The character code from the string.
    pub code: u32,
    /// The text this code stands for; empty when nothing maps it.
    pub text: String,
    /// The transform from glyph space to device space, including the font
    /// size, horizontal scaling and rise.
    pub transform: Matrix,
    /// Where the glyph would sit with 9.4.3's `Ts` taken out — the same
    /// transform's translation with its rise set to zero — and `None`
    /// whenever the rise is zero, which is every glyph on the hot path.
    ///
    /// # Why a rise is not a new line
    ///
    /// `Ts` displaces a glyph and **leaves the pen where it was**: a
    /// superscript footnote marker, a chemical subscript and a `GPOS` mark
    /// raised onto its base are all on the line they interrupt, not on one of
    /// their own. A consumer that decided line membership from
    /// [`Glyph::transform`]'s own origin would split such a line in two — and
    /// then again where the rise returns to zero, so one raised glyph between
    /// two ordinary ones makes three lines.
    ///
    /// The rule this enables is **monotone**: it can only join lines a `Ts`
    /// currently splits, and can never split a line that is joined today,
    /// because it changes nothing at all when the rise is zero.
    ///
    /// It is the *origin* rather than a whole matrix because that is what the
    /// question needs, and reported geometry does not move: a consumer's quads
    /// and per-character origins keep [`Glyph::transform`]'s risen position,
    /// which is where the ink is.
    pub baseline: Option<(f64, f64)>,
    /// The displacement to the next glyph, in text space, before the
    /// transform.
    ///
    /// **Signed, and along the writing direction.** Horizontally it is 9.4.4's
    /// `w0` and runs positive to the right; vertically it is 9.7.4.3's `w1`
    /// and is normally *negative*, because a column runs down and text space
    /// has y upward. A consumer wanting a length rather than a displacement
    /// takes the absolute value; one that assumes a positive number gets a
    /// column running the wrong way, which looks plausible enough to ship.
    pub advance: f64,
    /// The font size in effect.
    pub size: f64,
    /// Whether the font's writing mode is vertical.
    pub vertical: bool,
    /// An identifier for the font, stable within one interpretation.
    pub font_id: u64,
}

/// What a content stream asks to be drawn.
///
/// Every method has a default that does nothing, so a device implements only
/// what it cares about — the text device ignores paths, the rasterizer does
/// not.
pub trait Device {
    /// A glyph was shown.
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        let _ = (glyph, state);
    }

    /// A text object began (`BT`).
    fn begin_text(&mut self) {}

    /// A text object ended (`ET`).
    fn end_text(&mut self) {}

    /// A path was filled. `even_odd` selects the rule (8.5.3.3).
    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let _ = (path, state, even_odd);
    }

    /// A path was stroked.
    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        let _ = (path, state);
    }

    /// A path was added to the clipping path by `W` or `W*` (8.5.4).
    ///
    /// Arrives after the painting operator that ended the path, because that
    /// is when the clip takes effect.
    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let _ = (path, state, even_odd);
    }

    /// `q`: the graphics state was saved.
    ///
    /// A device holding state the interpreter does not model — a clip mask,
    /// say — saves it here.
    fn save_state(&mut self) {}

    /// `Q`: the graphics state was restored.
    fn restore_state(&mut self) {}

    /// An image XObject was drawn into the unit square of the current
    /// transform.
    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        let _ = (image, state);
    }

    /// `sh`: a named shading was painted over the current clip (8.7.4.2).
    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        let _ = (name, state);
    }

    /// A marked-content scope began: `BMC`, or `BDC` with its property list
    /// already resolved (14.6.2).
    ///
    /// `visible` is false when the tag was `/OC` and the document's default
    /// configuration turns that layer off (8.11.3.2). A device that draws
    /// stops painting until the matching [`Device::end_marked_content`]; the
    /// text device ignores this entirely, and that asymmetry is the whole
    /// reason suppression lives here rather than in the interpreter — both
    /// devices are handed the same operators, and only drawing differs.
    ///
    /// Scopes nest, and the visibility reported is each scope's **own**, not
    /// the enclosing answer. A device that acts on it therefore needs a
    /// stack rather than a counter: `EMC` has to know whether the scope it
    /// closes was one of the ones hiding things.
    ///
    /// `hidden_layer` names the layer, and is `Some` exactly when `visible`
    /// is false — ruling 10, so a warning can say which layer it hid rather
    /// than only that something was hidden.
    ///
    /// `tag` is the scope's own tag: `/OC` for optional content, `/Artifact`
    /// for 14.8.2.2's *"graphics objects that are not part of the author's
    /// original content"*, and a structure element's name for anything else.
    /// It is carried because **the two devices act on different tags** — the
    /// renderer on `/OC` and the text device on `/Artifact` — and a build that
    /// passed only the visibility could express the first and not the second.
    /// A `BMC` has a tag and no property list, so it is here too; a `BDC`
    /// whose operand stack is damaged reports an empty tag rather than
    /// guessing at one.
    ///
    /// `props` is the `BDC`'s property list reduced to the plain values 14.6.2
    /// and 14.9 define, or `None` for a `BMC`, for a `BDC` whose list this
    /// build could not read, and for a list that said nothing either clause
    /// covers. It is what carries `/MCID` — the only thing that ties drawn
    /// content to the structure tree (14.7.4.2) — so a device that wants a
    /// structured view keeps it, and every other device ignores it exactly as
    /// this default does.
    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        let _ = (tag, visible, hidden_layer, props);
    }

    /// `EMC`: the innermost marked-content scope ended.
    ///
    /// The interpreter guarantees one of these per accepted
    /// [`Device::begin_marked_content`], including for a stream that ends
    /// with scopes still open — so a device may pop without checking, and a
    /// form XObject cannot leave its caller inside a layer.
    fn end_marked_content(&mut self) {}

    /// A form XObject is about to be interpreted; returning false skips it.
    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        let _ = name;
        let _ = id;
        true
    }

    /// A form XObject finished.
    fn end_form(&mut self, id: u64) {
        let _ = id;
    }

    /// A transparency group is about to be interpreted (11.6.6).
    ///
    /// `state` is the one in force at the `Do`, because that is where the
    /// group's own `ca`, `CA` and `/BM` come from: they apply to the group's
    /// *result*, not to the objects inside it. A device that accepts the
    /// group says so by returning true, and the interpreter then resets the
    /// alphas to 1 and the blend mode to Normal for the duration of the
    /// content, per 11.6.6.
    ///
    /// **Declining is the default, and it has to be.** A device that keeps no
    /// buffer — the text device, a recorder — must not have the alphas reset
    /// underneath it, or content that was painting at `ca 0.5` starts
    /// painting at full strength with nothing to fade it afterwards. So the
    /// reset is conditional on the answer rather than on the operator.
    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let _ = (group, state);
        false
    }

    /// The group's content stream finished; composite it (11.4.7).
    ///
    /// Called exactly once for every [`Device::begin_group`] that answered
    /// true, including when interpretation was cancelled part-way, so a
    /// device may restore its buffer here without checking.
    fn end_group(&mut self) {}

    /// A `gs` set an `/SMask`, and its group is about to be interpreted
    /// (11.6.5.2).
    ///
    /// **This happens at the `gs`, not at the paint the mask will apply to.**
    /// 11.6.5.2 renders the mask group with the transform in force when the
    /// external graphics state is set, and `state.ctm` here is that one.
    /// Taking the CTM at paint time instead produces a mask that is *nearly*
    /// right — a shadow a few points from where it belongs, which reads as a
    /// slightly misplaced shadow rather than as a bug.
    ///
    /// `bbox` is `/G`'s bounding box as a device-space quad, in the same
    /// convention [`Device::clip_path`] uses, or empty when the form has
    /// none. It is what bounds the mask's buffer; the interpreter has it
    /// already and computing it twice invites the two answers to differ.
    ///
    /// Returning false declines, and is the default: nothing that does not
    /// keep pixels can make a mask out of one.
    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let _ = (mask, bbox, state);
        false
    }

    /// The mask group finished; read it back and install it (11.6.5.2).
    fn end_soft_mask(&mut self) {}

    /// `/SMask /None`: whatever mask was in force stops applying.
    fn clear_soft_mask(&mut self) {}

    /// Whether interpretation should stop — a cancellation hook checked
    /// between operators.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// One piece of a path, in device space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    /// Start a new subpath.
    MoveTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A straight line.
    LineTo {
        /// Destination.
        x: f64,
        /// Destination.
        y: f64,
    },
    /// A cubic Bézier.
    CurveTo {
        /// First control point.
        x1: f64,
        /// First control point.
        y1: f64,
        /// Second control point.
        x2: f64,
        /// Second control point.
        y2: f64,
        /// Destination.
        x3: f64,
        /// Destination.
        y3: f64,
    },
    /// Close the current subpath.
    Close,
}

/// An image the interpreter reached, identified rather than decoded — decoding
/// is the rasterizing device's business.
#[derive(Clone, Debug)]
pub struct ImageRef {
    /// The resource name, or empty for an inline image.
    pub name: Vec<u8>,
    /// Whether the image was inline (8.9.7).
    pub inline: bool,
    /// An inline image's dictionary, as the bytes between `BI` and `ID`.
    ///
    /// Handed over rather than parsed here: this crate has no object parser,
    /// and the device that draws the image is the one that already knows how
    /// to read a dictionary and run a filter chain.
    pub inline_dict: Vec<u8>,
    /// An inline image's sample data, as the bytes between `ID` and `EI`.
    pub inline_data: Vec<u8>,
}
