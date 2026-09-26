//! A page recorded once and drawn at any scale: the renderer's half of the
//! retained page.
//!
//! # Why the renderer has a half at all
//!
//! [`tinker_pdf_content::record`] keeps every call the interpreter makes and
//! [`tinker_pdf_content::replay`] makes them again, in order, into any
//! device. That is enough for every call but three, and the three are the
//! whole difficulty: `begin_form`, `begin_group` and `begin_soft_mask` are
//! **questions**, and the interpreter acts on the answer. A declined group is
//! drawn straight onto its parent with the alpha and the blend mode it was
//! invoked with; an accepted one has them reset to their initial values
//! inside it (11.6.6). A declined soft mask leaves the previous one in force.
//! So the states a recording carries are only the renderer's states if the
//! recording was made with **the renderer's answers**, and a replay is only
//! byte-equal to a direct render if both saw the same ones.
//!
//! [`Admission`] is those answers, kept apart from the pixels. The renderer
//! asks it — it is the renderer's own bookkeeping, moved rather than copied —
//! and so does [`DisplayRecorder`], the device a page is recorded through.
//! One implementation, asked by both, is what makes the two agree by
//! construction rather than by a test that happens to cover the case.
//!
//! # Why the answers do not depend on the scale
//!
//! Every input [`Admission`] reads is a count or a flag the content stream
//! decides — how many groups are open, how many buffers have been spent,
//! whether an enclosing marked-content scope hides its contents. None of them
//! is a pixel. That is a property this module has to keep: an answer taken
//! from the canvas (a group whose clip misses it, a mask whose box does) would
//! be one answer at the scale the page was recorded at and another at the
//! scale it is replayed at, and the renderer's September 2026 change to accept
//! such a group over a buffer of no pixels, rather than declining it, is what
//! removed the last one.
//!
//! The one input that is not the stream's is cancellation, which the recorder
//! never sees; a replay into a cancelled renderer is told no, and
//! [`tinker_pdf_content::replay`] says what it does then.

use tinker_pdf_content::record::{Answers, Event, RecordingDevice};
use tinker_pdf_content::{
    Device, Glyph, GraphicsState, Group, ImageRef, MarkedProps, MaskGroup, PathSegment,
};

use crate::{MAX_GROUP_BUFFERS, MAX_GROUP_DEPTH};

/// The renderer's answers to the interpreter's questions, apart from its
/// pixels.
///
/// See the module documentation for why this is its own type. Every field is
/// something the content stream decides; none is a pixel, a scale or a
/// rectangle.
#[derive(Clone, Debug, Default)]
pub struct Admission {
    /// Marked-content scopes currently open, innermost last: whether each one
    /// hides what it encloses (8.11.3.2, 14.6.2).
    ///
    /// A stack rather than a counter, because `EMC` has to know whether the
    /// scope it closes was one of the hiding ones. Bounded by the
    /// interpreter's own nesting cap, which is why there is no second cap
    /// here.
    marked: Vec<bool>,
    /// How many of `marked` hide, so the question every paint asks is a
    /// comparison rather than a scan.
    hidden_depth: u32,
    /// Transparency groups accepted and not yet ended.
    groups: usize,
    /// Soft-mask groups accepted and not yet ended.
    masks: usize,
    /// Group buffers opened so far, at any depth, against
    /// [`MAX_GROUP_BUFFERS`]. Spent and never refunded: unwinding a group
    /// returns its memory but not its budget, because the cost this bounds is
    /// the work already done rather than the memory still held.
    buffers: u32,
    /// Whether the budget above ever declined a group, reported once at the
    /// end rather than per decline: a page that has run out asks repeatedly,
    /// and forty thousand identical warnings is not a report.
    budget_spent: bool,
}

impl Admission {
    /// A page with nothing open.
    #[must_use]
    pub fn new() -> Admission {
        Admission::default()
    }

    /// `BMC` or `BDC`, with the scope's **own** visibility.
    pub fn begin_marked_content(&mut self, visible: bool) {
        self.marked.push(!visible);
        if !visible {
            self.hidden_depth = self.hidden_depth.saturating_add(1);
        }
    }

    /// `EMC`. One with nothing open is dropped, which is what the interpreter
    /// does with the stray ones a damaged stream carries.
    pub fn end_marked_content(&mut self) {
        if self.marked.pop() == Some(true) {
            self.hidden_depth = self.hidden_depth.saturating_sub(1);
        }
    }

    /// Whether any open scope hides what it encloses.
    ///
    /// 8.11.3.2: hidden content is not *skipped* — every operator inside the
    /// scope still runs, the text pen still advances, `q` and `Q` still
    /// balance. Only painting stops, and a group, being a paint, is declined.
    #[must_use]
    pub fn hidden(&self) -> bool {
        self.hidden_depth > 0
    }

    /// Whether a transparency group is accepted, spending a buffer if it is.
    ///
    /// Declined inside hidden content, past [`MAX_GROUP_DEPTH`] open groups,
    /// and once [`MAX_GROUP_BUFFERS`] have been spent — the last of which is
    /// remembered for [`Admission::budget_spent`].
    pub fn begin_group(&mut self) -> bool {
        if self.hidden() || self.groups >= MAX_GROUP_DEPTH {
            return false;
        }
        if !self.spend() {
            return false;
        }
        self.groups = self.groups.saturating_add(1);
        true
    }

    /// The innermost accepted group ended.
    pub fn end_group(&mut self) {
        self.groups = self.groups.saturating_sub(1);
    }

    /// Whether a soft-mask group is accepted, spending a buffer if it is.
    ///
    /// **Not** declined inside hidden content: the mask is graphics state that
    /// outlives the scope, and a paint after the scope closes is masked by it.
    /// Depth counts the groups open as well as the masks, because both hold a
    /// buffer.
    pub fn begin_soft_mask(&mut self) -> bool {
        if self.masks.saturating_add(self.groups) >= MAX_GROUP_DEPTH {
            return false;
        }
        if !self.spend() {
            return false;
        }
        self.masks = self.masks.saturating_add(1);
        true
    }

    /// The innermost accepted soft-mask group ended.
    pub fn end_soft_mask(&mut self) {
        self.masks = self.masks.saturating_sub(1);
    }

    /// Group buffers opened so far.
    #[must_use]
    pub fn buffers(&self) -> u32 {
        self.buffers
    }

    /// Whether [`MAX_GROUP_BUFFERS`] ever declined one.
    #[must_use]
    pub fn budget_spent(&self) -> bool {
        self.budget_spent
    }

    /// Spends one buffer from the budget, or records that there was none.
    fn spend(&mut self) -> bool {
        if self.buffers >= MAX_GROUP_BUFFERS {
            self.budget_spent = true;
            return false;
        }
        self.buffers = self.buffers.saturating_add(1);
        true
    }
}

/// The device a page is recorded through for a retained page.
///
/// A [`RecordingDevice`] capturing everything, answering the interpreter's
/// questions as a [`crate::Renderer`] would — through the same
/// [`Admission`] — so that what it keeps is what the renderer would have been
/// handed. Recording draws nothing, decodes nothing and resolves no resource;
/// the resources are the replaying renderer's business.
///
/// One recorder may record several streams in turn — a page's content and
/// then each of its annotations' appearances — and [`DisplayRecorder::take`]
/// splits the events between them while the answers carry on, because a
/// renderer drawing them carries one budget across all of them.
#[derive(Debug, Default)]
pub struct DisplayRecorder {
    events: RecordingDevice,
    admission: Admission,
}

impl DisplayRecorder {
    /// A recorder with nothing recorded and nothing open.
    #[must_use]
    pub fn new() -> DisplayRecorder {
        DisplayRecorder {
            events: RecordingDevice::new().answering(Answers {
                forms: true,
                groups: true,
                soft_masks: true,
                cancelled: false,
            }),
            admission: Admission::new(),
        }
    }

    /// The events recorded since the last call, leaving the answers' state
    /// where it is.
    pub fn take(&mut self) -> Vec<Event> {
        let answers = self.events.answers();
        std::mem::replace(&mut self.events, RecordingDevice::new().answering(answers)).into_events()
    }

    /// The answers' state, for a caller that wants to know what was spent.
    #[must_use]
    pub fn admission(&self) -> &Admission {
        &self.admission
    }

    /// Sets the answer the next question of one kind gets.
    fn answer(&mut self, set: impl FnOnce(&mut Answers)) {
        let mut answers = self.events.answers();
        set(&mut answers);
        self.events.set_answers(answers);
    }
}

impl Device for DisplayRecorder {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        self.events.show_glyph(glyph, state);
    }

    fn begin_text(&mut self) {
        self.events.begin_text();
    }

    fn end_text(&mut self) {
        self.events.end_text();
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.events.fill_path(path, state, even_odd);
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        self.events.stroke_path(path, state);
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        self.events.clip_path(path, state, even_odd);
    }

    fn save_state(&mut self) {
        self.events.save_state();
    }

    fn restore_state(&mut self) {
        self.events.restore_state();
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        self.events.draw_image(image, state);
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        self.events.draw_shading(name, state);
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.admission.begin_marked_content(visible);
        self.events
            .begin_marked_content(tag, visible, hidden_layer, props);
    }

    fn end_marked_content(&mut self) {
        self.admission.end_marked_content();
        self.events.end_marked_content();
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        // The renderer enters every form it is not cancelled out of.
        self.answer(|answers| answers.forms = true);
        self.events.begin_form(id, name)
    }

    fn end_form(&mut self, id: u64) {
        self.events.end_form(id);
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let accepted = self.admission.begin_group();
        self.answer(|answers| answers.groups = accepted);
        self.events.begin_group(group, state)
    }

    fn end_group(&mut self) {
        self.admission.end_group();
        self.events.end_group();
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let accepted = self.admission.begin_soft_mask();
        self.answer(|answers| answers.soft_masks = accepted);
        self.events.begin_soft_mask(mask, bbox, state)
    }

    fn end_soft_mask(&mut self) {
        self.admission.end_soft_mask();
        self.events.end_soft_mask();
    }

    fn clear_soft_mask(&mut self) {
        self.events.clear_soft_mask();
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}
