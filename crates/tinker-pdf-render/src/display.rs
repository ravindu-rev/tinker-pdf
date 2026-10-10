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
//! never sees — a caller's token is the replaying renderer's — and a replay
//! into a cancelled renderer is told no, and [`tinker_pdf_content::replay`]
//! says what it does then. The recorder stops the interpreter only for its
//! own reason, below, and a recording it stopped is never replayed.
//!
//! # How much a recording may hold
//!
//! **At most [`MAX_DISPLAY_LIST_BYTES`].** A recording keeps every call with
//! its own copy of the state and the path, so what it holds is the
//! interpreter's *work*, not the file's size — and work multiplies through
//! forms. Past the cap the recorder keeps nothing, tells the interpreter to
//! stop, and says so ([`DisplayRecorder::overflowed`]); the caller then draws
//! the page the direct way, which holds one canvas whatever the page does.

use tinker_pdf_content::record::{Answers, Event, RecordingDevice};
use tinker_pdf_content::{
    Device, Glyph, GraphicsState, Group, ImageRef, MarkedProps, MaskGroup, PathSegment,
};

use crate::{MAX_GROUP_BUFFERS, MAX_GROUP_DEPTH};

/// The most one recording may hold, in bytes as [`DisplayRecorder`] counts
/// them: the page's content and its annotations' appearances together.
///
/// A recording keeps every call the interpreter made, each with its own copy
/// of the graphics state and of its path, so what it costs is the
/// interpreter's work rather than the file's length — and work multiplies: a
/// form that invokes a form four times, sixteen forms deep, is 4^16 calls
/// from a file of a few kilobytes. The review of lane 4C measured a 1 940-byte
/// file of ten such forms record 1 310 719 calls at a peak of 635 376 kB,
/// where a direct render of the same page holds one canvas. Past this cap the recorder keeps
/// nothing and stops the interpreter, and the facade's `DisplayList` draws
/// every render by interpreting the page again: the same bitmap at a direct
/// render's cost, so the cap degrades and never refuses.
///
/// | | Bytes recorded |
/// | --- | --- |
/// | The most any fixture in this repository spends | 67 108 864 |
/// | A 200-page comic, whose largest page is one image | 1 000 |
/// | A dense 200-page fixed document, 2 000 elements and 40 000 path segments a page | 3 184 000 |
/// | A 300-page reflowable book, a page of 2 500 glyphs | 1 190 000 |
/// | **This cap** | **64 MiB** |
///
/// The count is [`kept_bytes`]: each event's own size, the state it carries
/// with that state's heap parts, its path at `size_of::<PathSegment>()` a
/// segment, and every name, string, inline image and mask-group stream it
/// copies — what the recording allocates, short of the allocator's own
/// bookkeeping. The yardsticks are on a 64-bit target, where an event is 168
/// bytes, a state 304 and a segment 56: a fixed page's two thousand elements
/// and forty thousand segments are 3 184 000, a book page's glyphs a little
/// under 1.2 MB, and a comic page — `q`, the image's `cm` and `Do`, `Q` —
/// under a kilobyte. The cap clears the densest by 21x, and a page past it
/// still draws. The fixtures' figure is the cap itself and is allowed, for
/// `MAX_PAGE_PIXELS`'s reason: the test that fires it records past it, and the
/// count never passes it — the call that would is dropped with everything
/// before it.
///
/// Reachable: `0 0 1 1 re f` is thirteen bytes of content and one recorded
/// fill of five segments, 752 bytes, so a content stream as long as
/// `MAX_DECODED_STREAM` records 7.2 GiB before any form multiplies it.
pub const MAX_DISPLAY_LIST_BYTES: usize = 64 << 20;

/// What one recorded event holds, in bytes, as [`MAX_DISPLAY_LIST_BYTES`]
/// counts it: its own size, and everything it copied to the heap.
#[must_use]
pub fn kept_bytes(event: &Event) -> usize {
    use core::mem::size_of;
    let text = |s: &Option<String>| s.as_ref().map_or(0, String::len);
    let bytes = |s: &Option<Vec<u8>>| s.as_ref().map_or(0, Vec::len);
    let state = |s: &Option<Box<GraphicsState>>| {
        s.as_deref().map_or(0, |s| {
            size_of::<GraphicsState>()
                .saturating_add(s.dashes.len().saturating_mul(size_of::<f64>()))
                .saturating_add(bytes(&s.fill_space))
                .saturating_add(bytes(&s.stroke_space))
                .saturating_add(bytes(&s.fill_pattern))
                .saturating_add(bytes(&s.stroke_pattern))
                .saturating_add(bytes(&s.text.font))
        })
    };
    let path = |p: &[PathSegment]| p.len().saturating_mul(size_of::<PathSegment>());
    let own = match event {
        Event::ShowGlyph { glyph, state: s } => glyph.text.len().saturating_add(state(s)),
        Event::FillPath {
            path: p, state: s, ..
        }
        | Event::StrokePath { path: p, state: s }
        | Event::ClipPath {
            path: p, state: s, ..
        } => path(p).saturating_add(state(s)),
        Event::DrawImage { image, state: s } => image
            .name
            .len()
            .saturating_add(image.inline_dict.len())
            .saturating_add(image.inline_data.len())
            .saturating_add(state(s)),
        Event::DrawShading { name, state: s } => name.len().saturating_add(state(s)),
        Event::BeginMarkedContent(scope) => {
            let props = scope.props.as_ref().map_or(0, |p| {
                size_of::<MarkedProps>()
                    .saturating_add(text(&p.actual_text))
                    .saturating_add(text(&p.alt))
                    .saturating_add(text(&p.lang))
                    .saturating_add(text(&p.expansion))
                    .saturating_add(p.associated_files.as_ref().map_or(0, Vec::len))
            });
            scope
                .tag
                .len()
                .saturating_add(text(&scope.hidden_layer))
                .saturating_add(props)
        }
        Event::BeginForm { name, .. } => name.len(),
        Event::BeginGroup { state: s, .. } => state(s),
        Event::BeginSoftMask {
            mask,
            bbox,
            state: s,
            ..
        } => size_of::<MaskGroup>()
            .saturating_add(mask.form.content.len())
            .saturating_add(path(bbox))
            .saturating_add(state(s)),
        _ => 0,
    };
    size_of::<Event>().saturating_add(own)
}

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
/// renderer drawing them carries one budget across all of them. The byte
/// budget ([`MAX_DISPLAY_LIST_BYTES`]) is carried across them for the same
/// reason: it bounds what one retained page holds, content and annotations
/// together.
#[derive(Debug)]
pub struct DisplayRecorder {
    events: RecordingDevice,
    admission: Admission,
    /// What everything recorded so far holds, by [`kept_bytes`], across every
    /// [`DisplayRecorder::take`].
    kept: usize,
    /// How many of `events` are already in `kept`.
    counted: usize,
    /// The most `kept` may reach: [`MAX_DISPLAY_LIST_BYTES`] unless a caller
    /// asked for less.
    budget: usize,
    /// Whether the budget was passed, after which nothing is kept.
    overflowed: bool,
}

impl Default for DisplayRecorder {
    fn default() -> Self {
        DisplayRecorder::new()
    }
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
            kept: 0,
            counted: 0,
            budget: MAX_DISPLAY_LIST_BYTES,
            overflowed: false,
        }
    }

    /// The same recorder under a smaller budget than
    /// [`MAX_DISPLAY_LIST_BYTES`]; a larger one is read as the cap, so this
    /// lowers the ceiling and cannot raise it.
    #[must_use]
    pub fn with_budget(mut self, bytes: usize) -> DisplayRecorder {
        self.budget = bytes.min(MAX_DISPLAY_LIST_BYTES);
        self
    }

    /// The events recorded since the last call, leaving the answers' state
    /// where it is. Empty once the recorder has [`overflowed`], whatever was
    /// recorded before.
    ///
    /// [`overflowed`]: DisplayRecorder::overflowed
    pub fn take(&mut self) -> Vec<Event> {
        let answers = self.events.answers();
        self.counted = 0;
        std::mem::replace(&mut self.events, RecordingDevice::new().answering(answers)).into_events()
    }

    /// The answers' state, for a caller that wants to know what was spent.
    #[must_use]
    pub fn admission(&self) -> &Admission {
        &self.admission
    }

    /// Whether the recording passed its budget. From that call on the
    /// recorder holds nothing — what it had kept is dropped, and the events
    /// [`DisplayRecorder::take`] returned before are the caller's to drop —
    /// declines every question, and answers `is_cancelled` with yes, so the
    /// interpreter stops at its next operator. A caller seeing this draws the
    /// page the direct way.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// What everything recorded so far holds, by [`kept_bytes`]; never more
    /// than the budget.
    #[must_use]
    pub fn kept(&self) -> usize {
        self.kept
    }

    /// Sets the answer the next question of one kind gets.
    fn answer(&mut self, set: impl FnOnce(&mut Answers)) {
        let mut answers = self.events.answers();
        set(&mut answers);
        self.events.set_answers(answers);
    }

    /// Counts what the last call recorded, and overflows if it passes the
    /// budget: the call that would is dropped with everything before it, so
    /// `kept` never passes the budget.
    fn count(&mut self) {
        let events = self.events.events();
        let mut added = 0usize;
        for event in events.get(self.counted..).unwrap_or_default() {
            added = added.saturating_add(kept_bytes(event));
        }
        self.counted = events.len();
        if self.kept.saturating_add(added) > self.budget {
            self.overflowed = true;
            self.events = RecordingDevice::new().answering(Answers {
                forms: false,
                groups: false,
                soft_masks: false,
                cancelled: true,
            });
            self.counted = 0;
        } else {
            self.kept = self.kept.saturating_add(added);
        }
    }
}

impl Device for DisplayRecorder {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if !self.overflowed {
            self.events.show_glyph(glyph, state);
            self.count();
        }
    }

    fn begin_text(&mut self) {
        if !self.overflowed {
            self.events.begin_text();
            self.count();
        }
    }

    fn end_text(&mut self) {
        if !self.overflowed {
            self.events.end_text();
            self.count();
        }
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if !self.overflowed {
            self.events.fill_path(path, state, even_odd);
            self.count();
        }
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        if !self.overflowed {
            self.events.stroke_path(path, state);
            self.count();
        }
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        if !self.overflowed {
            self.events.clip_path(path, state, even_odd);
            self.count();
        }
    }

    fn save_state(&mut self) {
        if !self.overflowed {
            self.events.save_state();
            self.count();
        }
    }

    fn restore_state(&mut self) {
        if !self.overflowed {
            self.events.restore_state();
            self.count();
        }
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        if !self.overflowed {
            self.events.draw_image(image, state);
            self.count();
        }
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        if !self.overflowed {
            self.events.draw_shading(name, state);
            self.count();
        }
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        if !self.overflowed {
            self.admission.begin_marked_content(visible);
            self.events
                .begin_marked_content(tag, visible, hidden_layer, props);
            self.count();
        }
    }

    fn end_marked_content(&mut self) {
        if !self.overflowed {
            self.admission.end_marked_content();
            self.events.end_marked_content();
            self.count();
        }
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        if self.overflowed {
            return false;
        }
        // The renderer enters every form it is not cancelled out of.
        self.answer(|answers| answers.forms = true);
        let entered = self.events.begin_form(id, name);
        self.count();
        // Declined if this very call passed the budget: an entered form whose
        // `BeginForm` was dropped would be closed by an `end_form` with
        // nothing to close.
        entered && !self.overflowed
    }

    fn end_form(&mut self, id: u64) {
        if !self.overflowed {
            self.events.end_form(id);
            self.count();
        }
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        if self.overflowed {
            return false;
        }
        let accepted = self.admission.begin_group();
        self.answer(|answers| answers.groups = accepted);
        let entered = self.events.begin_group(group, state);
        self.count();
        entered && !self.overflowed
    }

    fn end_group(&mut self) {
        if !self.overflowed {
            self.admission.end_group();
            self.events.end_group();
            self.count();
        }
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        if self.overflowed {
            return false;
        }
        let accepted = self.admission.begin_soft_mask();
        self.answer(|answers| answers.soft_masks = accepted);
        let entered = self.events.begin_soft_mask(mask, bbox, state);
        self.count();
        entered && !self.overflowed
    }

    fn end_soft_mask(&mut self) {
        if !self.overflowed {
            self.admission.end_soft_mask();
            self.events.end_soft_mask();
            self.count();
        }
    }

    fn clear_soft_mask(&mut self) {
        if !self.overflowed {
            self.events.clear_soft_mask();
            self.count();
        }
    }

    /// Yes once the budget is passed: nothing more would be kept, so nothing
    /// more is worth interpreting. Never otherwise — cancellation is the
    /// replaying renderer's, not the recording's (see the module
    /// documentation).
    fn is_cancelled(&self) -> bool {
        self.overflowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinker_pdf_content::Matrix;

    /// A fill of `segments` segments, and what one recording of it holds.
    fn fill(segments: usize) -> (Vec<PathSegment>, usize) {
        let path: Vec<PathSegment> = (0..segments)
            .map(|i| PathSegment::LineTo {
                x: i as f64,
                y: 1.0,
            })
            .collect();
        let state = GraphicsState::new(Matrix::IDENTITY);
        let event = Event::FillPath {
            path: path.clone(),
            even_odd: false,
            state: Some(Box::new(state)),
        };
        (path, kept_bytes(&event))
    }

    /// **The budget fires, by the recorder's own report and not by a clock.**
    /// Fills of a thousand segments under a budget of ten of them: nine are
    /// kept and counted exactly, the tenth fits, the eleventh passes the
    /// budget — and from that call the recorder holds nothing, declines
    /// every question and tells the interpreter to stop.
    #[test]
    fn a_recording_past_its_budget_keeps_nothing_and_stops_the_interpreter() {
        let (path, each) = fill(1_000);
        assert!(
            each >= 1_000 * core::mem::size_of::<PathSegment>(),
            "a fill holds its path: {each}"
        );
        let state = GraphicsState::new(Matrix::IDENTITY);
        let mut recorder = DisplayRecorder::new().with_budget(each * 10);
        for count in 1..=10 {
            recorder.fill_path(&path, &state, false);
            assert!(!recorder.overflowed(), "fill {count} fits");
            assert_eq!(recorder.kept(), each * count, "counted exactly");
        }
        assert!(!recorder.is_cancelled());
        recorder.fill_path(&path, &state, false);
        assert!(recorder.overflowed(), "the eleventh passes the budget");
        assert!(
            recorder.is_cancelled(),
            "and the interpreter is told to stop"
        );
        assert!(recorder.kept() <= each * 10, "the count never passes it");
        assert!(!recorder.begin_form(1, b"F"), "every question declined");
        assert!(!recorder.begin_group(Group::default(), &state));
        recorder.fill_path(&path, &state, false);
        assert!(recorder.take().is_empty(), "and nothing is kept");
    }

    /// The default budget is the cap, a larger one is read as the cap, and
    /// the budget is carried across [`DisplayRecorder::take`] — a page's
    /// content and its annotations are one retained page.
    #[test]
    fn the_budget_is_the_cap_and_spans_every_take() {
        let (path, each) = fill(100);
        let state = GraphicsState::new(Matrix::IDENTITY);
        assert_eq!(DisplayRecorder::new().budget, MAX_DISPLAY_LIST_BYTES);
        assert_eq!(DisplayRecorder::default().budget, MAX_DISPLAY_LIST_BYTES);
        assert_eq!(
            DisplayRecorder::new().with_budget(usize::MAX).budget,
            MAX_DISPLAY_LIST_BYTES
        );
        let mut recorder = DisplayRecorder::new().with_budget(each * 3);
        recorder.fill_path(&path, &state, false);
        recorder.fill_path(&path, &state, false);
        assert_eq!(recorder.take().len(), 2);
        recorder.fill_path(&path, &state, false);
        assert!(!recorder.overflowed());
        recorder.fill_path(&path, &state, false);
        assert!(recorder.overflowed(), "the fourth, across the take");
        assert!(recorder.take().is_empty());
    }

    /// What an event holds counts what it copied: an inline image's data, a
    /// marked-content scope's `/ActualText`, a mask group's content stream —
    /// the three ways a short operator copies a long string into a recording.
    #[test]
    fn kept_bytes_counts_what_an_event_copied() {
        let base = kept_bytes(&Event::SaveState);
        assert_eq!(base, core::mem::size_of::<Event>());
        let image = ImageRef {
            name: Vec::new(),
            inline: true,
            inline_dict: Vec::new(),
            inline_data: vec![0; 10_000],
        };
        let drawn = kept_bytes(&Event::DrawImage { image, state: None });
        assert!(drawn >= base + 10_000, "{drawn}");
        let scope = tinker_pdf_content::MarkedScope {
            tag: b"Span".to_vec(),
            visible: true,
            hidden_layer: None,
            props: Some(MarkedProps {
                actual_text: Some("x".repeat(10_000)),
                ..MarkedProps::default()
            }),
        };
        let marked = kept_bytes(&Event::BeginMarkedContent(scope));
        assert!(marked >= base + 10_000, "{marked}");
    }
}
