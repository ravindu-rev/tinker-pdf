//! A recording made again: the other half of [`crate::record`].
//!
//! [`replay`] hands a [`Device`] every call a
//! [`RecordingDevice`](crate::record::RecordingDevice) kept, in the order it
//! kept them, with the state each one saw. The interpreter is not involved:
//! nothing is tokenized, no operand is read and no font is asked to decode a
//! string, so a page recorded once can be drawn as often as a caller likes for
//! the cost of the drawing alone. That is the retained page's whole reason to
//! exist, and the paths a recording carries are in the space the interpreter
//! was run in rather than in pixels, which is what lets one recording be drawn
//! at any scale — the device holds the transform to pixels, not the events.
//!
//! # The three questions, asked again
//!
//! `begin_form`, `begin_group` and `begin_soft_mask` return an answer the
//! interpreter acted on when the recording was made, and each event says what
//! that answer was ([`Event::BeginForm::entered`] and its two siblings). A
//! replay asks the device again, because the device's own bookkeeping — a
//! clip stack, a group buffer, a resource scope — depends on being asked; and
//! then it has to decide what to do when the two answers differ.
//!
//! **They should not differ, and a replay that sees one says so**
//! ([`Replayed::disagreed`]). A recording is only the device's picture when it
//! was made with the device's answers — `tinker-pdf-render`'s
//! `DisplayRecorder` exists to make it so — and the one input no recorder can
//! foresee is cancellation. When they do differ, a replay does what an
//! interpreter handed the device's answer would have done wherever the
//! recording lets it, and drops what it cannot reconstruct:
//!
//! - **recorded entered, now declined** — the events up to the matching end
//!   are skipped. For a form or a soft mask that is exactly the interpreter's
//!   behaviour, which never runs the content of one declined. For a group it
//!   is not: a declined group's content is drawn straight onto its parent,
//!   with states the recording does not hold, so it is lost instead;
//! - **recorded declined, now entered** — the device is told the bracket ended
//!   at once. The recording holds nothing inside a declined form or mask, and
//!   a declined group's content, which it does hold, follows on the parent as
//!   it was recorded.
//!
//! # State
//!
//! An event recorded under a [`Capture`](crate::record::Capture) with its
//! state off carries none, and is replayed with the initial
//! [`GraphicsState`] — a recording made without states replays as what it
//! captured, which is geometry and order and not appearance. A retained page
//! captures everything.
//!
//! # A replay that pauses
//!
//! A [`Cursor`] is the walk with its place kept between calls
//! (`docs/design/progressive-render.md`): the index of the next event and the
//! depth of the declined bracket it is skipping. [`Cursor::step`] resumes the
//! walk there and hands the device at most a budget of events, so a page drawn
//! from its recording can pause between any two events and go on; the device
//! receives the same events in the same order whatever the budgets were.
//! [`replay`] is a cursor run to its end.

use crate::device::Device;
use crate::record::Event;
use crate::state::GraphicsState;

/// What a [`replay`], or one [`Cursor::step`], did.
///
/// The steps of one walk add up, with `+=`, to what one uninterrupted replay
/// of the same events into the same device does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replayed {
    /// Events handed to the device.
    pub delivered: usize,
    /// Events not handed over, because they were inside a bracket the device
    /// declined or because the device was cancelled.
    pub skipped: usize,
    /// Questions the device answered differently from the recording. Zero for
    /// a recording made with the device's own answers and replayed without
    /// cancellation, which is every replay the retained page makes.
    pub disagreed: usize,
    /// Whether the device said it was cancelled and the replay stopped.
    pub cancelled: bool,
}

impl core::ops::AddAssign for Replayed {
    /// Two parts of one walk: the counts summed, cancelled if either part
    /// was.
    fn add_assign(&mut self, other: Replayed) {
        self.delivered += other.delivered;
        self.skipped += other.skipped;
        self.disagreed += other.disagreed;
        self.cancelled |= other.cancelled;
    }
}

/// Replays `events` into `device`, in order.
///
/// See the module documentation for what happens when the device answers a
/// question differently from the recording. The device is asked
/// [`Device::is_cancelled`] before every event, as the interpreter asks it
/// before every token; once it answers yes, nothing more is delivered.
///
/// A [`Cursor`] stepped to the end with a budget no list reaches.
pub fn replay<D: Device + ?Sized>(events: &[Event], device: &mut D) -> Replayed {
    Cursor::new().step(events, device, usize::MAX)
}

/// Where a replay stands between two steps
/// (`docs/design/progressive-render.md`).
///
/// It holds the place and not the events, so it can be kept beside a list it
/// does not borrow; every step is handed the same events. A cursor stepped
/// over other events than the ones it began on walks those from its index,
/// and stops at their end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The index of the next event the walk comes to.
    next: usize,
    /// How many brackets the *recording* has open inside the one the device
    /// declined, that one included; zero when nothing is being skipped.
    skipping: usize,
    /// Whether the device said it was cancelled, which ended the walk.
    cancelled: bool,
}

impl Cursor {
    /// A cursor before the first event.
    #[must_use]
    pub fn new() -> Cursor {
        Cursor::default()
    }

    /// How many events the walk has passed, handed over or skipped: the index
    /// of the next one.
    #[must_use]
    pub fn position(&self) -> usize {
        self.next
    }

    /// Whether the walk has passed every one of `events`.
    #[must_use]
    pub fn is_finished(&self, events: &[Event]) -> bool {
        self.next >= events.len()
    }

    /// Whether the device said it was cancelled, which ended the walk.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Resumes the walk over `events` at its place and hands `device` at most
    /// `budget` events, then stops, keeping its place.
    ///
    /// An event skipped inside a bracket the device declined is not handed
    /// over and does not count against the budget, so a step stops right
    /// after its last delivery: skipped events met before it are passed in
    /// this step, and skipped events after it are the next step's. A budget of
    /// zero hands nothing and passes nothing. The device is asked
    /// [`Device::is_cancelled`] before every event it would be handed; once it
    /// answers yes, every event left is counted skipped and the walk ends, and
    /// later steps return at once.
    ///
    /// The `Replayed` is this step's alone; the steps of a walk sum to the
    /// whole replay's.
    pub fn step<D: Device + ?Sized>(
        &mut self,
        events: &[Event],
        device: &mut D,
        budget: usize,
    ) -> Replayed {
        let initial = GraphicsState::default();
        let state = |recorded: &Option<Box<GraphicsState>>| -> GraphicsState {
            recorded
                .as_deref()
                .cloned()
                .unwrap_or_else(|| initial.clone())
        };
        let mut done = Replayed::default();
        // The recording balances every bracket it entered — an end arrives
        // only for a question answered yes — so the matching end of the one
        // being skipped is the one that brings `skipping` back to zero,
        // whichever of the three kinds it is.
        while done.delivered < budget {
            let Some(event) = events.get(self.next) else {
                break;
            };
            if self.skipping > 0 {
                self.next += 1;
                match event {
                    Event::BeginForm { entered: true, .. }
                    | Event::BeginGroup { entered: true, .. }
                    | Event::BeginSoftMask { entered: true, .. } => self.skipping += 1,
                    Event::EndForm { .. } | Event::EndGroup | Event::EndSoftMask => {
                        self.skipping -= 1;
                    }
                    // Delivered from inside a bracket the device declined
                    // too: it is about the interpretation, not about what the
                    // bracket drew, and the budget usually runs out deep
                    // inside one.
                    Event::GroupBudgetSpent => {
                        done.delivered += 1;
                        device.group_budget_spent();
                        continue;
                    }
                    Event::RecursionRefused => {
                        done.delivered += 1;
                        device.recursion_refused();
                        continue;
                    }
                    Event::NestedBudgetSpent => {
                        done.delivered += 1;
                        device.nested_budget_spent();
                        continue;
                    }
                    _ => {}
                }
                done.skipped += 1;
                continue;
            }
            if device.is_cancelled() {
                done.cancelled = true;
                done.skipped += events.len() - self.next;
                self.next = events.len();
                self.cancelled = true;
                break;
            }
            self.next += 1;
            done.delivered += 1;
            self.deliver(event, device, &state, &mut done);
        }
        done
    }

    /// Hands `device` one event outside any declined bracket, asking it the
    /// recording's question again where the event is one, and starting to
    /// skip where it declines what the recording entered.
    fn deliver<D: Device + ?Sized>(
        &mut self,
        event: &Event,
        device: &mut D,
        state: &dyn Fn(&Option<Box<GraphicsState>>) -> GraphicsState,
        done: &mut Replayed,
    ) {
        match event {
            Event::BeginText => device.begin_text(),
            Event::ShowGlyph { glyph, state: s } => device.show_glyph(glyph, &state(s)),
            Event::EndText => device.end_text(),
            Event::FillPath {
                path,
                even_odd,
                state: s,
            } => device.fill_path(path, &state(s), *even_odd),
            Event::StrokePath { path, state: s } => device.stroke_path(path, &state(s)),
            Event::ClipPath {
                path,
                even_odd,
                state: s,
            } => device.clip_path(path, &state(s), *even_odd),
            Event::SaveState => device.save_state(),
            Event::RestoreState => device.restore_state(),
            Event::DrawImage { image, state: s } => device.draw_image(image, &state(s)),
            Event::DrawShading { name, state: s } => device.draw_shading(name, &state(s)),
            Event::BeginMarkedContent(scope) => device.begin_marked_content(
                &scope.tag,
                scope.visible,
                scope.hidden_layer.as_deref(),
                scope.props.as_ref(),
            ),
            Event::EndMarkedContent => device.end_marked_content(),
            Event::BeginForm { id, name, entered } => {
                let now = device.begin_form(*id, name);
                if now != *entered {
                    done.disagreed += 1;
                    if *entered {
                        self.skipping = 1;
                    } else {
                        device.end_form(*id);
                    }
                }
            }
            Event::EndForm { id } => device.end_form(*id),
            Event::BeginGroup {
                group,
                entered,
                state: s,
            } => {
                let now = device.begin_group(*group, &state(s));
                if now != *entered {
                    done.disagreed += 1;
                    if *entered {
                        self.skipping = 1;
                    } else {
                        device.end_group();
                    }
                }
            }
            Event::EndGroup => device.end_group(),
            Event::BeginSoftMask {
                mask,
                bbox,
                entered,
                state: s,
            } => {
                let now = device.begin_soft_mask(mask, bbox, &state(s));
                if now != *entered {
                    done.disagreed += 1;
                    if *entered {
                        self.skipping = 1;
                    } else {
                        device.end_soft_mask();
                    }
                }
            }
            Event::EndSoftMask => device.end_soft_mask(),
            Event::ClearSoftMask => device.clear_soft_mask(),
            Event::GroupBudgetSpent => device.group_budget_spent(),
            Event::RecursionRefused => device.recursion_refused(),
            Event::NestedBudgetSpent => device.nested_budget_spent(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpret::{interpret, FontSource, Form, Layer};
    use crate::record::{Answers, EventKind, RecordingDevice};
    use crate::state::Matrix;

    /// Every byte is one code, 500/1000 em wide; `/Off` names a hidden layer;
    /// `/Fm` is a plain form and `/Grp` a transparency group, each drawing a
    /// square.
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
            (name == b"Off").then(|| Layer {
                visible: false,
                label: "Off".to_string(),
            })
        }
        fn form(&self, name: &[u8]) -> Option<Form> {
            let group = match name {
                b"Fm" => None,
                b"Grp" => Some(crate::interpret::Group {
                    isolated: true,
                    knockout: false,
                    space: None,
                }),
                _ => return None,
            };
            Some(Form {
                content: b"0 0 1 rg 1 1 3 3 re f".to_vec(),
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 5.0, 5.0]),
                group,
                stream: 7,
            })
        }
    }

    const PAGE: &[u8] = b"q 1 0 0 rg 0 0 10 10 re f Q \
        BT /F1 12 Tf 2 3 Td (ab) Tj ET \
        q 0 0 5 5 re W n /Fm Do Q \
        /OC /Off BDC 0 0 2 2 re f EMC \
        /Grp Do";

    /// Records `content` with `answers`.
    fn record(content: &[u8], answers: Answers) -> Vec<Event> {
        let mut device = RecordingDevice::new().answering(answers);
        interpret(content, Matrix::IDENTITY, &mut device, &Fonts);
        device.into_events()
    }

    fn accept_all() -> Answers {
        Answers {
            forms: true,
            groups: true,
            soft_masks: true,
            cancelled: false,
        }
    }

    /// Two transcripts are the same calls with the same payloads. `Event`
    /// holds `f64`s and is not `PartialEq`, so the debug form is compared —
    /// which covers every field, the states included.
    fn same(a: &[Event], b: &[Event]) {
        assert_eq!(a.len(), b.len(), "the same number of calls");
        for (index, (x, y)) in a.iter().zip(b.iter()).enumerate() {
            assert_eq!(format!("{x:?}"), format!("{y:?}"), "call {index}");
        }
    }

    /// **A replay into a recorder is the recording**: every call, every
    /// payload and every state, in order — which is the property every other
    /// consumer of a replay rests on, checked where the only device involved
    /// is one that keeps what it is told.
    #[test]
    fn a_replay_into_a_recorder_is_the_recording() {
        for answers in [Answers::default(), accept_all()] {
            let recorded = record(PAGE, answers);
            assert!(recorded.len() > 15, "the page records something");
            let mut again = RecordingDevice::new().answering(answers);
            let done = replay(&recorded, &mut again);
            assert_eq!(
                done,
                Replayed {
                    delivered: recorded.len(),
                    ..Replayed::default()
                }
            );
            same(&recorded, again.events());
        }
    }

    /// A device that declines what the recording entered skips to the
    /// matching end — a form's content, which the interpreter would never
    /// have run, and a group's, which it would have drawn on the parent with
    /// states the recording does not hold — and counts the disagreement.
    #[test]
    fn a_declined_bracket_is_skipped_to_its_end_and_counted() {
        let recorded = record(PAGE, accept_all());
        let mut declining = RecordingDevice::new().answering(Answers {
            forms: false,
            groups: false,
            soft_masks: false,
            cancelled: false,
        });
        let done = replay(&recorded, &mut declining);
        assert_eq!(done.disagreed, 2, "the form and the group");
        assert!(done.skipped >= 4, "each bracket's fill and its end");
        let kinds = declining.kinds();
        assert!(!kinds.contains(&EventKind::EndForm));
        assert!(!kinds.contains(&EventKind::EndGroup));
        assert_eq!(
            declining.count(EventKind::FillPath),
            2,
            "the page's own square and the hidden one, and neither bracket's"
        );
    }

    /// A device that enters what the recording declined is told the bracket
    /// ended at once, so its own bookkeeping balances; the declined group's
    /// content, recorded on the parent, follows on the parent.
    #[test]
    fn an_entered_bracket_the_recording_declined_is_closed_at_once() {
        let recorded = record(PAGE, Answers::default());
        let mut entering = RecordingDevice::new().answering(accept_all());
        let done = replay(&recorded, &mut entering);
        assert_eq!(done.disagreed, 1, "the group; forms agree by default");
        let kinds = entering.kinds();
        let at = kinds
            .iter()
            .position(|k| *k == EventKind::BeginGroup)
            .expect("the group is offered");
        assert_eq!(kinds.get(at + 1), Some(&EventKind::EndGroup));
        assert_eq!(
            entering.count(EventKind::FillPath),
            recorded
                .iter()
                .filter(|e| e.kind() == EventKind::FillPath)
                .count(),
            "nothing drawn was lost"
        );
    }

    /// A cancelled device is handed nothing further, and the replay says how
    /// much it did not deliver.
    #[test]
    fn a_cancelled_device_stops_the_replay() {
        let recorded = record(PAGE, accept_all());
        let mut cancelled = RecordingDevice::new().answering(Answers {
            cancelled: true,
            ..accept_all()
        });
        let done = replay(&recorded, &mut cancelled);
        assert!(done.cancelled);
        assert_eq!(done.delivered, 0);
        assert_eq!(done.skipped, recorded.len());
        assert!(cancelled.events().is_empty());
    }

    /// An event recorded with no state is replayed with the initial one, never
    /// with the last state some other event carried.
    #[test]
    fn a_stateless_recording_replays_with_the_initial_state() {
        let mut device = RecordingDevice::with_capture(crate::record::Capture {
            state: false,
            ..crate::record::Capture::ALL
        });
        interpret(
            b"1 0 0 rg 0 0 1 1 re f",
            Matrix::IDENTITY,
            &mut device,
            &Fonts,
        );
        let mut again = RecordingDevice::new();
        replay(device.events(), &mut again);
        let state = again
            .of_kind(EventKind::FillPath)
            .next()
            .and_then(Event::state)
            .expect("a fill with a state");
        assert_eq!(
            format!("{state:?}"),
            format!("{:?}", GraphicsState::default())
        );
    }

    // ---- a replay that pauses ---------------------------------------------

    /// A page whose forms and groups make long brackets, so a pause falls
    /// inside each at some budget: two forms, text, two groups — one under a
    /// clip — a hidden layer, and a form again.
    const BRACKETS: &[u8] = b"q 1 0 0 rg 0 0 10 10 re f Q \
        /Fm Do /Fm Do \
        BT /F1 12 Tf 2 3 Td (abcd) Tj ET \
        /Grp Do q 0 0 5 5 re W n /Grp Do Q \
        /OC /Off BDC 0 0 2 2 re f 1 1 2 2 re f EMC \
        /Fm Do";

    /// A recorder that says it is cancelled once it has been handed `after`
    /// calls, whichever they were: the same moment in any walk that hands it
    /// the same calls in the same order.
    struct CancelledAfter {
        inner: RecordingDevice,
        calls: usize,
        after: usize,
    }

    impl CancelledAfter {
        fn new(answers: Answers, after: usize) -> CancelledAfter {
            CancelledAfter {
                inner: RecordingDevice::new().answering(answers),
                calls: 0,
                after,
            }
        }
        fn call(&mut self) -> &mut RecordingDevice {
            self.calls += 1;
            &mut self.inner
        }
    }

    impl Device for CancelledAfter {
        fn show_glyph(&mut self, glyph: &crate::device::Glyph, state: &GraphicsState) {
            self.call().show_glyph(glyph, state);
        }
        fn begin_text(&mut self) {
            self.call().begin_text();
        }
        fn end_text(&mut self) {
            self.call().end_text();
        }
        fn fill_path(
            &mut self,
            path: &[crate::device::PathSegment],
            state: &GraphicsState,
            even_odd: bool,
        ) {
            self.call().fill_path(path, state, even_odd);
        }
        fn stroke_path(&mut self, path: &[crate::device::PathSegment], state: &GraphicsState) {
            self.call().stroke_path(path, state);
        }
        fn clip_path(
            &mut self,
            path: &[crate::device::PathSegment],
            state: &GraphicsState,
            even_odd: bool,
        ) {
            self.call().clip_path(path, state, even_odd);
        }
        fn save_state(&mut self) {
            self.call().save_state();
        }
        fn restore_state(&mut self) {
            self.call().restore_state();
        }
        fn draw_image(&mut self, image: &crate::device::ImageRef, state: &GraphicsState) {
            self.call().draw_image(image, state);
        }
        fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
            self.call().draw_shading(name, state);
        }
        fn begin_marked_content(
            &mut self,
            tag: &[u8],
            visible: bool,
            hidden_layer: Option<&str>,
            props: Option<&crate::device::MarkedProps>,
        ) {
            self.call()
                .begin_marked_content(tag, visible, hidden_layer, props);
        }
        fn end_marked_content(&mut self) {
            self.call().end_marked_content();
        }
        fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
            self.call().begin_form(id, name)
        }
        fn end_form(&mut self, id: u64) {
            self.call().end_form(id);
        }
        fn begin_group(&mut self, group: crate::interpret::Group, state: &GraphicsState) -> bool {
            self.call().begin_group(group, state)
        }
        fn end_group(&mut self) {
            self.call().end_group();
        }
        fn begin_soft_mask(
            &mut self,
            mask: &crate::interpret::MaskGroup,
            bbox: &[crate::device::PathSegment],
            state: &GraphicsState,
        ) -> bool {
            self.call().begin_soft_mask(mask, bbox, state)
        }
        fn end_soft_mask(&mut self) {
            self.call().end_soft_mask();
        }
        fn clear_soft_mask(&mut self) {
            self.call().clear_soft_mask();
        }
        fn group_budget_spent(&mut self) {
            self.call().group_budget_spent();
        }
        fn recursion_refused(&mut self) {
            self.call().recursion_refused();
        }
        fn nested_budget_spent(&mut self) {
            self.call().nested_budget_spent();
        }
        fn is_cancelled(&self) -> bool {
            self.calls >= self.after
        }
    }

    /// Steps `events` into `device` `budget` at a time to the end, holding
    /// each step to the budget, and returns the steps' sum.
    fn stepped<D: Device>(events: &[Event], device: &mut D, budget: usize, what: &str) -> Replayed {
        let mut cursor = Cursor::new();
        let mut total = Replayed::default();
        let mut steps = Vec::new();
        while !cursor.is_finished(events) {
            let before = cursor.position();
            let step = cursor.step(events, device, budget);
            assert!(
                cursor.position() > before || step.delivered > 0,
                "{what}: a step at {before} made no progress"
            );
            steps.push(step);
            total += step;
            assert!(steps.len() <= events.len() + 1, "{what}: the walk ends");
        }
        let (last, rest) = steps.split_last().expect("at least one step");
        for (index, step) in rest.iter().enumerate() {
            assert_eq!(
                step.delivered,
                budget,
                "{what}: step {index} of {} delivers exactly its budget",
                steps.len()
            );
        }
        assert!(last.delivered <= budget, "{what}: the last step");
        // A finished walk hands nothing more.
        assert_eq!(cursor.step(events, device, budget), Replayed::default());
        total
    }

    /// **The exit criterion: a replay resumed every k events is the whole
    /// replay.** For k from one to eight, over recordings replayed into
    /// devices that decline what they entered — brackets the walk skips, with
    /// pauses falling inside them — and that enter what they declined, the
    /// events a `RecordingDevice` receives and the summed `Replayed` are one
    /// uninterrupted replay's, each step delivering exactly its budget until
    /// the last.
    #[test]
    fn a_replay_resumed_every_k_events_is_the_whole_replay() {
        let declining = Answers {
            forms: false,
            groups: false,
            soft_masks: false,
            cancelled: false,
        };
        let cases = [
            ("entered, replayed declining", accept_all(), declining),
            (
                "declined, replayed entering",
                Answers::default(),
                accept_all(),
            ),
            ("entered, replayed alike", accept_all(), accept_all()),
            (
                "defaults, replayed alike",
                Answers::default(),
                Answers::default(),
            ),
        ];
        for (name, recorded_with, replayed_with) in cases {
            for page in [PAGE, BRACKETS] {
                let recorded = record(page, recorded_with);
                let mut whole = RecordingDevice::new().answering(replayed_with);
                let all = replay(&recorded, &mut whole);
                if recorded_with.groups && !replayed_with.groups {
                    assert!(all.skipped > 0, "{name}: something is skipped");
                }
                for k in 1..=8 {
                    let what = format!("{name}, k = {k}");
                    let mut resumed = RecordingDevice::new().answering(replayed_with);
                    let summed = stepped(&recorded, &mut resumed, k, &what);
                    assert_eq!(summed, all, "{what}: the summed steps");
                    same(whole.events(), resumed.events());
                }
            }
        }
    }

    /// A device cancelled between two steps, and one cancelled in the middle
    /// of one: the rest is skipped, the walk ends, and both are what one
    /// uninterrupted replay into a device cancelled at the same call is.
    #[test]
    fn a_device_cancelled_between_steps_ends_the_walk_as_a_replay_does() {
        let recorded = record(BRACKETS, accept_all());
        for k in 1..=8 {
            // Two steps, then the device says it is cancelled.
            let mut device = RecordingDevice::new().answering(accept_all());
            let mut cursor = Cursor::new();
            let mut summed = cursor.step(&recorded, &mut device, k);
            summed += cursor.step(&recorded, &mut device, k);
            let calls = device.events().len();
            device.set_answers(Answers {
                cancelled: true,
                ..accept_all()
            });
            while !cursor.is_finished(&recorded) {
                summed += cursor.step(&recorded, &mut device, k);
            }
            assert!(cursor.is_cancelled() && summed.cancelled, "k = {k}");
            assert_eq!(summed.delivered + summed.skipped, recorded.len());
            assert_eq!(cursor.step(&recorded, &mut device, k), Replayed::default());

            let mut whole = CancelledAfter::new(accept_all(), calls);
            let all = replay(&recorded, &mut whole);
            assert_eq!(summed, all, "k = {k}");
            same(whole.inner.events(), device.events());

            // Cancelled partway through a step: the same calls either way.
            for after in [1, 5, 13] {
                let mut resumed = CancelledAfter::new(accept_all(), after);
                let mut cursor = Cursor::new();
                let mut summed = Replayed::default();
                while !cursor.is_finished(&recorded) {
                    summed += cursor.step(&recorded, &mut resumed, k);
                }
                let mut whole = CancelledAfter::new(accept_all(), after);
                assert_eq!(
                    summed,
                    replay(&recorded, &mut whole),
                    "k = {k}, after {after}"
                );
                same(whole.inner.events(), resumed.inner.events());
            }
        }
    }

    /// An empty bracket of a recording entered and declined, with a notice
    /// about the interpretation inside it — `events` built by hand.
    fn skipped_bracket() -> Vec<Event> {
        let fill = || Event::FillPath {
            path: vec![crate::device::PathSegment::MoveTo { x: 0.0, y: 0.0 }],
            even_odd: false,
            state: None,
        };
        let group = crate::interpret::Group {
            isolated: true,
            knockout: false,
            space: None,
        };
        vec![
            fill(),
            Event::BeginGroup {
                group,
                entered: true,
                state: None,
            },
            fill(),
            Event::GroupBudgetSpent,
            fill(),
            Event::EndGroup,
            fill(),
        ]
    }

    /// A step stops right after its last delivery: skipped events before it
    /// are passed in the step and skipped events after it are the next
    /// step's; a notice inside a skipped bracket is handed over and counts
    /// against the budget; and a budget of zero does nothing.
    #[test]
    fn a_step_stops_after_its_last_delivery_and_skips_only_before_it() {
        let events = skipped_bracket();
        let declining = Answers {
            groups: false,
            ..Answers::default()
        };
        let mut device = RecordingDevice::new().answering(declining);
        let mut cursor = Cursor::new();
        assert_eq!(cursor.step(&events, &mut device, 0), Replayed::default());
        assert_eq!(cursor.position(), 0, "a budget of zero passes nothing");
        // The fill, then the group the device declines: two deliveries, and
        // the step stops there, before the bracket's content.
        let first = cursor.step(&events, &mut device, 2);
        assert_eq!((first.delivered, first.skipped, first.disagreed), (2, 0, 1));
        assert_eq!(cursor.position(), 2);
        // The skipped fill is passed, the notice delivered, and the step
        // stops there with its budget of one spent.
        let second = cursor.step(&events, &mut device, 1);
        assert_eq!((second.delivered, second.skipped), (1, 1));
        assert_eq!(cursor.position(), 4);
        // The rest of the bracket is passed and the last fill delivered.
        let third = cursor.step(&events, &mut device, 5);
        assert_eq!((third.delivered, third.skipped), (1, 2));
        assert!(cursor.is_finished(&events));
        assert_eq!(
            device.kinds(),
            [
                EventKind::FillPath,
                EventKind::BeginGroup,
                EventKind::GroupBudgetSpent,
                EventKind::FillPath
            ]
        );
        let mut whole = RecordingDevice::new().answering(declining);
        let mut summed = first;
        summed += second;
        summed += third;
        assert_eq!(summed, replay(&events, &mut whole));
    }

    /// A cursor holds a place and not the events: stepped over a shorter
    /// list than the one it walked, it stops at that list's end, and the sum
    /// of two walks' parts is their counts added and either's cancellation.
    #[test]
    fn a_cursor_over_other_events_stops_at_their_end_and_parts_add_up() {
        let events = skipped_bracket();
        let mut device = RecordingDevice::new();
        let mut cursor = Cursor::new();
        cursor.step(&events, &mut device, 5);
        let shorter = &events[..2];
        assert!(cursor.is_finished(shorter));
        assert_eq!(cursor.step(shorter, &mut device, 3), Replayed::default());
        let mut sum = Replayed {
            delivered: 2,
            skipped: 1,
            disagreed: 0,
            cancelled: false,
        };
        sum += Replayed {
            delivered: 3,
            skipped: 0,
            disagreed: 2,
            cancelled: true,
        };
        assert_eq!(
            sum,
            Replayed {
                delivered: 5,
                skipped: 1,
                disagreed: 2,
                cancelled: true
            }
        );
    }
}
