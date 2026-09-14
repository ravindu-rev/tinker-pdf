//! A [`Device`](crate::device::Device) that keeps every call it is handed, in
//! order, as a typed event.
//!
//! # Why this exists, and who asked for it
//!
//! **No roadmap row names this module, and it was scheduled deliberately
//! anyway.** The decision is recorded here rather than folded into the first
//! consumer that needed it, so that it is visible to the next reader and
//! reversible by them: if the consumers below never arrive, this module has no
//! other justification and should go.
//!
//! Six rows of [`docs/ROADMAP.md`] each need a recording device, and each of
//! them states the capability rather than the prerequisite:
//!
//! 1. **A retained page — a display list replayed at any scale.** The one row
//!    that *does* say the words: its "what it would take" column reads "a
//!    recording `Device`". Everything below is the same object.
//! 2. **PDF to SVG.** A `Device` that writes SVG 1.1 has to see paths, clips,
//!    groups and soft masks in the order the stream asked for them.
//! 3. **Structured text serialisation — JSON, XML, HTML.** Glyphs with their
//!    state and their marked-content scopes, which is the transcript plus one
//!    join.
//! 4. **Table reconstruction from geometry.** Wants the rules and fills a page
//!    draws beside the glyphs, which no text-only device keeps.
//! 5. **Inferred reading order for untagged pages.** Wants glyphs and the
//!    marked-content nesting, and nothing else.
//! 6. **Font subsetting on rewrite.** Wants the glyph-usage walk —
//!    `(font_id, code)` pairs — and nothing else at all.
//!
//! Before this module, `interpret.rs` carried **five** private recorders in
//! its test module — `Recorder`, `GroupEvents`, `Painted`, `Scopes` and
//! `Images` — each keeping the subset one test happened to need, with heavy
//! overlap and no two agreeing on what an event is. All five are gone; this is
//! what they became.
//!
//! # A transcript, not a scene
//!
//! What is kept is exactly what [`Device`](crate::device::Device) was told, at
//! the moment it was told. Nothing is derived: no accumulated clip, no
//! bounding boxes, no resolved font programs, no decoded image samples, no
//! collapsing of a `q`/`Q` pair that changed nothing. Two consumers that want
//! different scenes build them from the same transcript and do not have to
//! agree with each other first — which is the property five ad-hoc recorders
//! did not have.
//!
//! ## What an event carries, and what that costs
//!
//! Every method of the trait is handed a `&GraphicsState`, and the default is
//! to **copy** it into the event. A recorder that keeps only what one consumer
//! needed is the thing this replaces, so a sixth one of those would be no
//! promotion; and the retained-page row's exit criterion — a replay
//! byte-equal to a direct render — is unreachable without the alpha, the blend
//! mode, the colour and the CTM that were in force at the call rather than at
//! the end of the page.
//!
//! That is not free, and the cost is named rather than absorbed:
//!
//! - The state is **boxed**, so one event's stride is set by a glyph and not
//!   by the largest thing a page can ask for (a [`MaskGroup`] carries a
//!   256-entry transfer function). `event_stays_small` pins the number.
//! - [`Capture`] turns whole categories off. The two consumers above that want
//!   only glyphs — subsetting's glyph-usage walk and inferred reading order —
//!   run under [`Capture::GLYPHS`] and pay for a `Vec` of glyph events with no
//!   state copies at all.
//! - When the state is not captured it is [`None`] rather than a default, so
//!   an unrecorded state can never be mistaken for an initial one.
//!
//! ## Order across nesting: a flat list, with explicit begin and end events
//!
//! Marked-content scopes nest, and
//! [`Device::begin_marked_content`](crate::device::Device::begin_marked_content)
//! reports each scope's **own** visibility rather than the enclosing answer —
//! its own documentation says so, and says a device acting on it needs a stack
//! rather than a counter. This module records the reported answer and derives
//! the enclosing one on demand ([`RecordingDevice::hidden_at`]), because
//! storing the derived answer instead is precisely the defect that doc warns
//! about: it cannot be undone, and an `EMC` then has no way to know whether
//! the scope it closes was one of the ones hiding things.
//!
//! A tree was the alternative, and it would have made `hidden_at` free — a
//! node knows its parents. Three things are what it would have cost:
//!
//! - **`q`/`Q` and `BMC`/`EMC` nest independently and may cross.** `q … BDC …
//!   Q … EMC` is a stream a producer writes, and neither bracket contains the
//!   other. A tree has to pick one of the two as the parent and demote the
//!   other to a sibling, and that choice is wrong in whichever direction it is
//!   made.
//! - **Damage has no parent.** The interpreter refuses scopes past
//!   `MAX_MARKED_CONTENT_DEPTH`, drops a stray `EMC`, and closes a stream that
//!   ended inside a scope. A flat list records what the device was told and
//!   nothing else; a tree would have to answer each of those with a repair
//!   policy, and baking one consumer's policy into the recorder makes it all
//!   six consumers' policy.
//! - **Four of the six want document order anyway** — SVG, the retained page,
//!   structured text and the glyph walk all replay in one pass — so a tree
//!   would be flattened back on the way out, re-inventing the begin and end
//!   events this list already has.
//!
//! The price paid instead is that [`RecordingDevice::scopes_at`] walks the
//! prefix, which is linear in the index. Only a consumer that asks pays it.

use crate::device::{Device, Glyph, ImageRef, MarkedProps, PathSegment};
use crate::interpret::{Group, MaskGroup};
use crate::state::GraphicsState;

/// Which categories of call a [`RecordingDevice`] keeps.
///
/// Every flag is on by default. They exist because a `Vec` of everything a
/// large page draws is not free and two of the six expected consumers want
/// only glyphs; see the module documentation for the argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capture {
    /// `BT`, `ET` and every glyph shown.
    pub text: bool,
    /// Fills, strokes and clips.
    pub paths: bool,
    /// Image XObjects, inline images and `sh` shadings.
    pub images: bool,
    /// `q` and `Q`, marked-content scopes, forms, transparency groups and
    /// soft masks — everything that brackets something else.
    ///
    /// [`RecordingDevice::hidden_at`] answers `false` everywhere when this is
    /// off, because the scopes it would consult were never recorded.
    pub structure: bool,
    /// Whether the [`GraphicsState`] each call saw is copied into its event.
    ///
    /// Off leaves [`Event::state`] returning [`None`] — absent, never
    /// defaulted, so it cannot be mistaken for the initial state.
    pub state: bool,
}

impl Capture {
    /// Everything, including the state copy. The default.
    pub const ALL: Capture = Capture {
        text: true,
        paths: true,
        images: true,
        structure: true,
        state: true,
    };

    /// Glyphs and the text objects around them, and nothing else.
    ///
    /// What the glyph-usage walk that subsetting wants needs: a `(font_id,
    /// code)` pair per glyph, with no state copy and no path, image or
    /// bracketing event recorded at all.
    pub const GLYPHS: Capture = Capture {
        text: true,
        paths: false,
        images: false,
        structure: false,
        state: false,
    };

    /// Nothing. Useful as a base to switch single categories on.
    pub const NONE: Capture = Capture {
        text: false,
        paths: false,
        images: false,
        structure: false,
        state: false,
    };
}

impl Default for Capture {
    fn default() -> Self {
        Capture::ALL
    }
}

/// What a [`RecordingDevice`] answers to the trait's three questions, and to
/// cancellation.
///
/// These are not observations, they are decisions the interpreter acts on: a
/// declined group is never entered, so a recorder that always declined could
/// not record a group's contents at all and one that always accepted would
/// have the alphas reset underneath a consumer that keeps no buffer. The
/// defaults are the trait's own — forms entered, groups and masks declined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answers {
    /// What [`Device::begin_form`] returns. The trait's default is `true`.
    pub forms: bool,
    /// What [`Device::begin_group`] returns. The trait's default is `false`,
    /// and accepting means the interpreter resets the alphas and the blend
    /// mode inside the group (11.6.6) — which is right for a consumer that
    /// composites the group's result and wrong for one that does not.
    pub groups: bool,
    /// What [`Device::begin_soft_mask`] returns. The trait's default is
    /// `false`: nothing that does not keep pixels can make a mask out of one.
    pub soft_masks: bool,
    /// What [`Device::is_cancelled`] returns.
    pub cancelled: bool,
}

impl Default for Answers {
    fn default() -> Self {
        Answers {
            forms: true,
            groups: false,
            soft_masks: false,
            cancelled: false,
        }
    }
}

/// One marked-content scope, as [`Device::begin_marked_content`] reported it.
///
/// `visible` is this scope's **own** answer and not the enclosing one; see
/// [`RecordingDevice::hidden_at`] for the enclosing question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkedScope {
    /// The scope's tag: `/OC`, `/Artifact`, a structure element's name, or
    /// empty for a `BDC` whose operand stack was damaged.
    pub tag: Vec<u8>,
    /// Whether this scope alone paints (8.11.3.2).
    pub visible: bool,
    /// The layer's name, `Some` exactly when `visible` is false (ruling 10).
    pub hidden_layer: Option<String>,
    /// The `BDC`'s property list, reduced to 14.6.2's and 14.9's plain values.
    pub props: Option<MarkedProps>,
}

/// One call to a [`Device`] method, with the state it saw.
///
/// The variants are one per trait method that carries information; the three
/// that ask a question also record the answer that was given, because the
/// matching `end_` event arrives only for a question answered `true`.
#[derive(Clone, Debug)]
pub enum Event {
    /// `BT`.
    BeginText,
    /// One glyph was shown.
    ShowGlyph {
        /// The glyph as the interpreter resolved it, transform included.
        glyph: Glyph,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// `ET`.
    EndText,
    /// A path was filled.
    FillPath {
        /// The path, in device space.
        path: Vec<PathSegment>,
        /// 8.5.3.3's rule.
        even_odd: bool,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// A path was stroked.
    StrokePath {
        /// The path, in device space.
        path: Vec<PathSegment>,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// `W` or `W*` added a path to the clip (8.5.4).
    ClipPath {
        /// The path, in device space.
        path: Vec<PathSegment>,
        /// 8.5.3.3's rule.
        even_odd: bool,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// `q`.
    SaveState,
    /// `Q`.
    RestoreState,
    /// An image XObject or an inline image.
    DrawImage {
        /// The image, identified rather than decoded.
        image: ImageRef,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// `sh` (8.7.4.2).
    DrawShading {
        /// The shading's resource name.
        name: Vec<u8>,
        /// The state at the call.
        state: Option<Box<GraphicsState>>,
    },
    /// `BMC`, or `BDC` with its property list resolved.
    BeginMarkedContent(MarkedScope),
    /// `EMC`.
    EndMarkedContent,
    /// A form XObject was offered.
    BeginForm {
        /// The form's identity, stable within one interpretation.
        id: u64,
        /// Its resource name.
        name: Vec<u8>,
        /// Whether it was entered — [`Answers::forms`].
        entered: bool,
    },
    /// A form XObject finished. Arrives only for an entered form.
    EndForm {
        /// The same identity [`Event::BeginForm`] carried.
        id: u64,
    },
    /// A transparency group was offered (11.6.6).
    BeginGroup {
        /// The group's own attributes.
        group: Group,
        /// Whether it was accepted — [`Answers::groups`].
        entered: bool,
        /// The state at the `Do`, which is the group's own.
        state: Option<Box<GraphicsState>>,
    },
    /// The group finished. Arrives only for an accepted group.
    EndGroup,
    /// A `gs` installed an `/SMask` and its group was offered (11.6.5.2).
    BeginSoftMask {
        /// The mask group. Boxed: it carries a 256-entry transfer function,
        /// and inlining it would set the stride of every other event.
        mask: Box<MaskGroup>,
        /// `/G`'s bounding box as a device-space quad, empty when it has none.
        bbox: Vec<PathSegment>,
        /// Whether it was accepted — [`Answers::soft_masks`].
        entered: bool,
        /// The state at the `gs`, which is where 11.6.5.2 takes the CTM.
        state: Option<Box<GraphicsState>>,
    },
    /// The mask group finished. Arrives only for an accepted mask.
    EndSoftMask,
    /// `/SMask /None`.
    ClearSoftMask,
}

/// Which trait method an [`Event`] came from, with the payload dropped.
///
/// Exists so that "the stream asked for exactly these calls, in this order" is
/// one `assert_eq!` on a `Vec` rather than a hand-written match per position —
/// which is the assertion the ad-hoc recorders could not make at all, because
/// each of them only saw its own methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// [`Event::BeginText`].
    BeginText,
    /// [`Event::ShowGlyph`].
    ShowGlyph,
    /// [`Event::EndText`].
    EndText,
    /// [`Event::FillPath`].
    FillPath,
    /// [`Event::StrokePath`].
    StrokePath,
    /// [`Event::ClipPath`].
    ClipPath,
    /// [`Event::SaveState`].
    SaveState,
    /// [`Event::RestoreState`].
    RestoreState,
    /// [`Event::DrawImage`].
    DrawImage,
    /// [`Event::DrawShading`].
    DrawShading,
    /// [`Event::BeginMarkedContent`].
    BeginMarkedContent,
    /// [`Event::EndMarkedContent`].
    EndMarkedContent,
    /// [`Event::BeginForm`].
    BeginForm,
    /// [`Event::EndForm`].
    EndForm,
    /// [`Event::BeginGroup`].
    BeginGroup,
    /// [`Event::EndGroup`].
    EndGroup,
    /// [`Event::BeginSoftMask`].
    BeginSoftMask,
    /// [`Event::EndSoftMask`].
    EndSoftMask,
    /// [`Event::ClearSoftMask`].
    ClearSoftMask,
}

impl Event {
    /// Which method this event came from.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Event::BeginText => EventKind::BeginText,
            Event::ShowGlyph { .. } => EventKind::ShowGlyph,
            Event::EndText => EventKind::EndText,
            Event::FillPath { .. } => EventKind::FillPath,
            Event::StrokePath { .. } => EventKind::StrokePath,
            Event::ClipPath { .. } => EventKind::ClipPath,
            Event::SaveState => EventKind::SaveState,
            Event::RestoreState => EventKind::RestoreState,
            Event::DrawImage { .. } => EventKind::DrawImage,
            Event::DrawShading { .. } => EventKind::DrawShading,
            Event::BeginMarkedContent(_) => EventKind::BeginMarkedContent,
            Event::EndMarkedContent => EventKind::EndMarkedContent,
            Event::BeginForm { .. } => EventKind::BeginForm,
            Event::EndForm { .. } => EventKind::EndForm,
            Event::BeginGroup { .. } => EventKind::BeginGroup,
            Event::EndGroup => EventKind::EndGroup,
            Event::BeginSoftMask { .. } => EventKind::BeginSoftMask,
            Event::EndSoftMask => EventKind::EndSoftMask,
            Event::ClearSoftMask => EventKind::ClearSoftMask,
        }
    }

    /// The graphics state this call saw, copied at the call.
    ///
    /// [`None`] for an event whose method is handed no state (`BT`, `q`,
    /// `EMC`, …) and for every event recorded under a [`Capture`] with
    /// [`Capture::state`] off. The two are deliberately the same answer:
    /// neither is a state, and inventing a default for either would let a
    /// consumer read "no state was recorded" as "the state was initial".
    #[must_use]
    pub fn state(&self) -> Option<&GraphicsState> {
        match self {
            Event::ShowGlyph { state, .. }
            | Event::FillPath { state, .. }
            | Event::StrokePath { state, .. }
            | Event::ClipPath { state, .. }
            | Event::DrawImage { state, .. }
            | Event::DrawShading { state, .. }
            | Event::BeginGroup { state, .. }
            | Event::BeginSoftMask { state, .. } => state.as_deref(),
            _ => None,
        }
    }

    /// The glyph, for a [`Event::ShowGlyph`].
    #[must_use]
    pub fn glyph(&self) -> Option<&Glyph> {
        match self {
            Event::ShowGlyph { glyph, .. } => Some(glyph),
            _ => None,
        }
    }

    /// The scope, for a [`Event::BeginMarkedContent`].
    #[must_use]
    pub fn scope(&self) -> Option<&MarkedScope> {
        match self {
            Event::BeginMarkedContent(scope) => Some(scope),
            _ => None,
        }
    }
}

/// A [`Device`] that keeps every call, in order.
///
/// See the module documentation for why it exists, what it records and what it
/// deliberately does not.
///
/// ```
/// use tinker_pdf_content::{interpret, Matrix};
/// use tinker_pdf_content::record::{EventKind, RecordingDevice};
/// # struct F;
/// # impl tinker_pdf_content::FontSource for F {
/// #     fn decode(&self, _f: &[u8], _b: &[u8]) -> Vec<(u32, String, f64)> { Vec::new() }
/// #     fn vertical_metrics(&self, _f: &[u8], _c: u32) -> (f64, f64, f64) { (0.0, 0.0, 0.0) }
/// # }
/// let mut device = RecordingDevice::new();
/// interpret(b"q 0 0 2 2 re f Q", Matrix::IDENTITY, &mut device, &F);
/// assert_eq!(
///     device.kinds(),
///     vec![EventKind::SaveState, EventKind::FillPath, EventKind::RestoreState]
/// );
/// ```
#[derive(Clone, Debug, Default)]
pub struct RecordingDevice {
    events: Vec<Event>,
    capture: Capture,
    answers: Answers,
}

impl RecordingDevice {
    /// A recorder that keeps everything and answers the trait's own defaults.
    #[must_use]
    pub fn new() -> RecordingDevice {
        RecordingDevice::default()
    }

    /// A recorder that keeps only the categories `capture` names.
    #[must_use]
    pub fn with_capture(capture: Capture) -> RecordingDevice {
        RecordingDevice {
            events: Vec::new(),
            capture,
            answers: Answers::default(),
        }
    }

    /// The same recorder, answering the trait's questions as `answers` says.
    #[must_use]
    pub fn answering(mut self, answers: Answers) -> RecordingDevice {
        self.answers = answers;
        self
    }

    /// Every event, in the order the interpreter produced it.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Every event, taking ownership.
    #[must_use]
    pub fn into_events(self) -> Vec<Event> {
        self.events
    }

    /// What was captured.
    #[must_use]
    pub fn capture(&self) -> Capture {
        self.capture
    }

    /// The kind of every event, in order.
    #[must_use]
    pub fn kinds(&self) -> Vec<EventKind> {
        self.events.iter().map(Event::kind).collect()
    }

    /// How many events of `kind` were recorded.
    #[must_use]
    pub fn count(&self, kind: EventKind) -> usize {
        self.events.iter().filter(|e| e.kind() == kind).count()
    }

    /// Every event of `kind`, in order.
    pub fn of_kind(&self, kind: EventKind) -> impl Iterator<Item = &Event> + '_ {
        self.events.iter().filter(move |e| e.kind() == kind)
    }

    /// The index of every event of `kind`, in order.
    ///
    /// The index is what [`RecordingDevice::scopes_at`] and
    /// [`RecordingDevice::hidden_at`] take, so "what was open when this fill
    /// happened" is two calls rather than a second walk.
    pub fn indices(&self, kind: EventKind) -> impl Iterator<Item = usize> + '_ {
        self.events
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.kind() == kind)
            .map(|(index, _)| index)
    }

    /// Every glyph shown, in order.
    pub fn glyphs(&self) -> impl Iterator<Item = &Glyph> + '_ {
        self.events.iter().filter_map(Event::glyph)
    }

    /// Every marked-content scope opened, in order, each with its **own**
    /// visibility.
    pub fn scopes(&self) -> impl Iterator<Item = &MarkedScope> + '_ {
        self.events.iter().filter_map(Event::scope)
    }

    /// The marked-content scopes open when `events()[index]` was recorded,
    /// outermost first.
    ///
    /// The scope opened *by* `events()[index]` is not among them: the answer
    /// is what enclosed the call, which for a `BDC` is its parent.
    ///
    /// An `index` past the end asks the same question of the whole recording,
    /// so `scopes_at(usize::MAX)` is what is still open at the end of the
    /// stream — which for a complete interpretation is nothing, because the
    /// interpreter closes what a stream left open.
    #[must_use]
    pub fn scopes_at(&self, index: usize) -> Vec<&MarkedScope> {
        let mut open: Vec<&MarkedScope> = Vec::new();
        for event in self.events.iter().take(index) {
            match event {
                Event::BeginMarkedContent(scope) => open.push(scope),
                Event::EndMarkedContent => {
                    // An `EMC` with nothing open is dropped rather than
                    // underflowing, which is what the interpreter does with
                    // the stray ones a damaged stream carries.
                    open.pop();
                }
                _ => {}
            }
        }
        open
    }

    /// Whether any scope open when `events()[index]` was recorded reported
    /// itself hidden (8.11.3.2).
    ///
    /// This is the *enclosing* answer, derived rather than stored.
    /// [`MarkedScope::visible`] is each scope's own, because that is what the
    /// trait reports and it is the one that cannot be recovered from the
    /// other: a nested `/OC` naming a layer that is on sits inside a hidden
    /// one and stays hidden, and a recorder that had stored only "hidden here"
    /// could never say which scope did the hiding.
    ///
    /// Always `false` under a [`Capture`] with [`Capture::structure`] off,
    /// because the scopes were not recorded.
    #[must_use]
    pub fn hidden_at(&self, index: usize) -> bool {
        self.scopes_at(index).iter().any(|scope| !scope.visible)
    }

    /// The scopes still open at the end of the recording, outermost first.
    #[must_use]
    pub fn open_scopes(&self) -> Vec<&MarkedScope> {
        self.scopes_at(self.events.len())
    }

    /// The state copy an event should carry, honouring [`Capture::state`].
    ///
    /// **Cloned here, at the call.** A recorder that kept a reference or a
    /// shared handle would report every event with the last state the page
    /// reached, which is the classic defect in this shape of code and is
    /// invisible on any page that sets its state once.
    fn snapshot(&self, state: &GraphicsState) -> Option<Box<GraphicsState>> {
        self.capture.state.then(|| Box::new(state.clone()))
    }
}

impl Device for RecordingDevice {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if self.capture.text {
            let state = self.snapshot(state);
            self.events.push(Event::ShowGlyph {
                glyph: glyph.clone(),
                state,
            });
        }
    }

    fn begin_text(&mut self) {
        if self.capture.text {
            self.events.push(Event::BeginText);
        }
    }

    fn end_text(&mut self) {
        if self.capture.text {
            self.events.push(Event::EndText);
        }
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if self.capture.paths {
            let state = self.snapshot(state);
            self.events.push(Event::FillPath {
                path: path.to_vec(),
                even_odd,
                state,
            });
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if self.capture.paths {
            let state = self.snapshot(state);
            self.events.push(Event::StrokePath {
                path: path.to_vec(),
                state,
            });
        }
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if self.capture.paths {
            let state = self.snapshot(state);
            self.events.push(Event::ClipPath {
                path: path.to_vec(),
                even_odd,
                state,
            });
        }
    }

    fn save_state(&mut self) {
        if self.capture.structure {
            self.events.push(Event::SaveState);
        }
    }

    fn restore_state(&mut self) {
        if self.capture.structure {
            self.events.push(Event::RestoreState);
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if self.capture.images {
            let state = self.snapshot(state);
            self.events.push(Event::DrawImage {
                image: image.clone(),
                state,
            });
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        if self.capture.images {
            let state = self.snapshot(state);
            self.events.push(Event::DrawShading {
                name: name.to_vec(),
                state,
            });
        }
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        if self.capture.structure {
            // `visible` is stored exactly as reported — this scope's own
            // answer. The enclosing answer is `hidden_at`, and computing it
            // here instead would throw away the only copy of the reported one.
            self.events.push(Event::BeginMarkedContent(MarkedScope {
                tag: tag.to_vec(),
                visible,
                hidden_layer: hidden_layer.map(str::to_string),
                props: props.cloned(),
            }));
        }
    }

    fn end_marked_content(&mut self) {
        if self.capture.structure {
            self.events.push(Event::EndMarkedContent);
        }
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        if self.capture.structure {
            self.events.push(Event::BeginForm {
                id,
                name: name.to_vec(),
                entered: self.answers.forms,
            });
        }
        self.answers.forms
    }

    fn end_form(&mut self, id: u64) {
        if self.capture.structure {
            self.events.push(Event::EndForm { id });
        }
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        if self.capture.structure {
            let state = self.snapshot(state);
            self.events.push(Event::BeginGroup {
                group,
                entered: self.answers.groups,
                state,
            });
        }
        self.answers.groups
    }

    fn end_group(&mut self) {
        if self.capture.structure {
            self.events.push(Event::EndGroup);
        }
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        if self.capture.structure {
            let state = self.snapshot(state);
            self.events.push(Event::BeginSoftMask {
                mask: Box::new(mask.clone()),
                bbox: bbox.to_vec(),
                entered: self.answers.soft_masks,
                state,
            });
        }
        self.answers.soft_masks
    }

    fn end_soft_mask(&mut self) {
        if self.capture.structure {
            self.events.push(Event::EndSoftMask);
        }
    }

    fn clear_soft_mask(&mut self) {
        if self.capture.structure {
            self.events.push(Event::ClearSoftMask);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.answers.cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpret::{interpret, FontSource, Form, Layer};
    use crate::state::{BlendMode, Matrix, Rgb};

    /// Every byte is one code, 500/1000 em wide, mapping to itself; `/Off`
    /// names a layer the configuration hides and `/On` one it shows; `/Fm` is
    /// a plain form and `/Grp` a transparency group.
    struct Fonts;

    impl FontSource for Fonts {
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
                b"Fm" => Some(Form {
                    content: b"0 0 5 5 re f".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: None,
                    stream: 0x1_0000,
                }),
                b"Grp" => Some(Form {
                    content: b"0 0 5 5 re f".to_vec(),
                    matrix: Matrix::IDENTITY,
                    bbox: None,
                    group: Some(Group::default()),
                    stream: 0x2_0000,
                }),
                _ => None,
            }
        }
        fn font_id(&self, name: &[u8]) -> u64 {
            u64::from(name[0])
        }
        fn ext_g_state_alpha(&self, name: &[u8]) -> Option<(Option<f64>, Option<f64>)> {
            (name == b"Half").then_some((Some(0.5), None))
        }
    }

    fn record(src: &[u8]) -> RecordingDevice {
        let mut device = RecordingDevice::new();
        interpret(src, Matrix::IDENTITY, &mut device, &Fonts);
        device
    }

    /// The whole point of the module in one assertion: a small stream's calls,
    /// all of them, in the order the interpreter made them.
    ///
    /// Every bracket in the source appears as its own event — the `q` and the
    /// `Q`, both `BDC`s and both `EMC`s, the `BT` and the `ET`. A recorder
    /// that collapsed any pair, or that appended out of order, or that
    /// swallowed the `ET` because nothing it kept needed one, fails here and
    /// names the position it failed at.
    #[test]
    fn the_event_list_is_what_the_stream_asked_for_in_order() {
        let d = record(
            b"q 2 0 0 2 0 0 cm \
              /OC /Off BDC \
                /Span BMC 0 0 2 2 re f EMC \
                BT /F0 10 Tf (A) Tj ET \
              EMC \
              Q /Sh sh",
        );

        assert_eq!(
            d.kinds(),
            vec![
                EventKind::SaveState,
                EventKind::BeginMarkedContent,
                EventKind::BeginMarkedContent,
                EventKind::FillPath,
                EventKind::EndMarkedContent,
                EventKind::BeginText,
                EventKind::ShowGlyph,
                EventKind::EndText,
                EventKind::EndMarkedContent,
                EventKind::RestoreState,
                EventKind::DrawShading,
            ]
        );
    }

    /// `q` and `Q` are events of their own, not a depth counter.
    ///
    /// A `Q` with nothing saved restores nothing and raises no event, so the
    /// two counts are equal only when the stream balanced — which a counter
    /// that saturated at zero would hide.
    #[test]
    fn saves_and_restores_are_recorded_one_for_one() {
        let d = record(b"q q 0 0 1 1 re f Q Q Q");
        assert_eq!(
            d.kinds(),
            vec![
                EventKind::SaveState,
                EventKind::SaveState,
                EventKind::FillPath,
                EventKind::RestoreState,
                EventKind::RestoreState,
            ],
            "the third Q had nothing to restore and raised nothing"
        );
    }

    /// The defect this shape of code is famous for: a recorder that keeps a
    /// reference or one shared handle reports every event with the **last**
    /// state the page reached.
    ///
    /// Two fills with different alphas, different colours and different
    /// transforms, asserted separately. A recorder sharing one state passes
    /// nothing here; a recorder that copied at the call passes both.
    #[test]
    fn each_event_keeps_the_state_it_saw_and_not_the_last_one() {
        let d = record(
            b"1 0 0 rg /Half gs 0 0 1 1 re f \
              Q 0 0 1 RG 0 1 0 rg 3 0 0 3 0 0 cm 0 0 1 1 re f",
        );

        let states: Vec<&GraphicsState> = d
            .of_kind(EventKind::FillPath)
            .map(|e| e.state().expect("the state was captured"))
            .collect();
        assert_eq!(states.len(), 2);

        assert_eq!(states[0].fill_color, Rgb { r: 255, g: 0, b: 0 });
        assert!((states[0].fill_alpha - 0.5).abs() < 1e-12);
        assert_eq!(states[0].ctm.a, 1.0);

        assert_eq!(states[1].fill_color, Rgb { r: 0, g: 255, b: 0 });
        assert!(
            (states[1].fill_alpha - 0.5).abs() < 1e-12,
            "the `Q` had nothing to restore, so the alpha is still the one the \
             `gs` set — which is what makes the *colour* and the *transform* \
             the two that separate the events"
        );
        assert_eq!(states[1].ctm.a, 3.0);
        assert_eq!(states[1].stroke_color, Rgb { r: 0, g: 0, b: 255 });
    }

    /// A glyph's transform is the one 9.4.4 built for it, so it moves when the
    /// text matrix moves — recording it before `Tm` is applied puts every
    /// glyph on the page at the origin and still draws a tidy line of them.
    #[test]
    fn a_glyphs_transform_is_the_one_the_text_matrix_placed_it_with() {
        let d = record(b"BT /F0 10 Tf 1 0 0 1 100 700 Tm (AB) Tj ET");
        let places: Vec<(f64, f64)> = d.glyphs().map(|g| (g.transform.e, g.transform.f)).collect();
        assert_eq!(
            places,
            vec![(100.0, 700.0), (105.0, 700.0)],
            "the pen starts where Tm put it, and advances 500/1000 em at 10pt"
        );
        assert_eq!(
            d.glyphs().map(|g| g.text.as_str()).collect::<String>(),
            "AB"
        );
        assert_eq!(
            d.glyphs().map(|g| g.font_id).collect::<Vec<_>>(),
            vec![u64::from(b'F'), u64::from(b'F')]
        );
    }

    /// Each scope's **own** visibility is stored, and the enclosing answer is
    /// derived.
    ///
    /// The inner `/OC` names a layer that is *on* and sits inside one that is
    /// off. A recorder that had stored the enclosing answer would report the
    /// inner scope as hidden and could never say which scope did the hiding;
    /// one that reported only the inner scope's own answer at the fill would
    /// paint the middle rectangle.
    #[test]
    fn a_scopes_own_visibility_is_kept_and_the_enclosing_one_is_derived() {
        let d = record(
            b"0 0 1 1 re f \
              /OC /Off BDC \
                0 0 2 2 re f \
                /OC /On BDC 0 0 3 3 re f EMC \
              EMC \
              0 0 4 4 re f",
        );

        let own: Vec<bool> = d.scopes().map(|s| s.visible).collect();
        assert_eq!(own, vec![false, true], "each scope reported its own answer");

        let enclosing: Vec<bool> = d
            .indices(EventKind::FillPath)
            .map(|i| d.hidden_at(i))
            .collect();
        assert_eq!(
            enclosing,
            vec![false, true, true, false],
            "the nested visible scope is still inside the hidden one"
        );

        let inner = d.indices(EventKind::BeginMarkedContent).nth(1).unwrap();
        assert!(
            d.hidden_at(inner),
            "and the inner BDC itself was opened inside a hidden scope"
        );
        assert!(d.open_scopes().is_empty(), "every scope was closed");
        assert_eq!(
            d.scopes().next().unwrap().hidden_layer.as_deref(),
            Some("Construction lines"),
            "ruling 10: the scope that hid says which layer it was"
        );
    }

    /// A form is bracketed, and the bracket says whether it was entered.
    ///
    /// `BeginForm` carries the answer because `EndForm` arrives only for a
    /// form that was entered — so a consumer pairing them by position alone
    /// would mispair the first declined form with the next form's end.
    #[test]
    fn forms_are_bracketed_and_a_declined_one_has_no_end() {
        let entered = record(b"/Fm Do");
        assert_eq!(
            entered.kinds(),
            vec![
                EventKind::BeginForm,
                EventKind::FillPath,
                EventKind::EndForm
            ]
        );

        let mut declined = RecordingDevice::new().answering(Answers {
            forms: false,
            ..Answers::default()
        });
        interpret(
            b"/Fm Do 0 0 1 1 re f",
            Matrix::IDENTITY,
            &mut declined,
            &Fonts,
        );
        assert_eq!(
            declined.kinds(),
            vec![EventKind::BeginForm, EventKind::FillPath],
            "the form's own fill never happened; the page's did"
        );
        assert!(matches!(
            declined.events()[0],
            Event::BeginForm { entered: false, .. }
        ));
    }

    /// 11.6.6: accepting a group resets the alphas and the blend mode inside
    /// it, and declining must leave them alone. The recorder can do either,
    /// and the event says which it did.
    #[test]
    fn a_group_is_offered_and_the_answer_is_part_of_the_event() {
        let declined = record(b"/Half gs /Grp Do");
        let inside = declined
            .of_kind(EventKind::FillPath)
            .next()
            .and_then(Event::state)
            .expect("the group's fill");
        assert!(
            (inside.fill_alpha - 0.5).abs() < 1e-12,
            "a declined group keeps the alpha the page set"
        );
        assert_eq!(declined.count(EventKind::EndGroup), 0, "never opened");

        let mut accepted = RecordingDevice::new().answering(Answers {
            groups: true,
            ..Answers::default()
        });
        interpret(b"/Half gs /Grp Do", Matrix::IDENTITY, &mut accepted, &Fonts);
        let inside = accepted
            .of_kind(EventKind::FillPath)
            .next()
            .and_then(Event::state)
            .expect("the group's fill");
        assert!((inside.fill_alpha - 1.0).abs() < 1e-12);
        assert_eq!(inside.blend, BlendMode::Normal);
        let offered = accepted
            .of_kind(EventKind::BeginGroup)
            .next()
            .and_then(Event::state)
            .expect("the state at the Do");
        assert!(
            (offered.fill_alpha - 0.5).abs() < 1e-12,
            "while the offer carries the state at the `Do`, which is the \
             group's own"
        );
        assert_eq!(accepted.count(EventKind::EndGroup), 1);
    }

    /// [`Capture::GLYPHS`] is what the glyph-usage walk runs under: the glyphs
    /// arrive, nothing else does, and no state is copied.
    #[test]
    fn a_glyph_only_capture_keeps_glyphs_and_nothing_else() {
        let mut d = RecordingDevice::with_capture(Capture::GLYPHS);
        interpret(
            b"q /OC /Off BDC 0 0 2 2 re f BT /F0 10 Tf (AB) Tj ET EMC Q",
            Matrix::IDENTITY,
            &mut d,
            &Fonts,
        );

        assert_eq!(
            d.kinds(),
            vec![
                EventKind::BeginText,
                EventKind::ShowGlyph,
                EventKind::ShowGlyph,
                EventKind::EndText
            ]
        );
        assert!(
            d.events().iter().all(|e| e.state().is_none()),
            "no state was copied, and an uncopied state is absent rather than \
             a default that could be read as the initial one"
        );
        assert!(
            !d.hidden_at(1),
            "and with no scopes recorded the enclosing question answers false, \
             which is why a consumer that needs it does not use this capture"
        );
    }

    /// The `Vec` is the cost this module was argued into accepting, so the
    /// per-row price is pinned rather than assumed.
    ///
    /// The state is boxed and so is the mask group: without either, one event
    /// would be as wide as the largest thing a page can ask for — a
    /// [`MaskGroup`] alone carries a 256-entry transfer function — and a page
    /// of a hundred thousand glyphs would pay for it a hundred thousand times.
    #[test]
    fn event_stays_small() {
        let size = core::mem::size_of::<Event>();
        assert!(
            size <= 192,
            "an Event is {size} bytes; the boxing that keeps it small has been \
             undone"
        );
    }
}
