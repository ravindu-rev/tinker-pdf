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

use crate::device::Device;
use crate::record::Event;
use crate::state::GraphicsState;

/// What a [`replay`] did.
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

/// Replays `events` into `device`, in order.
///
/// See the module documentation for what happens when the device answers a
/// question differently from the recording. The device is asked
/// [`Device::is_cancelled`] before every event, as the interpreter asks it
/// before every token; once it answers yes, nothing more is delivered.
pub fn replay<D: Device + ?Sized>(events: &[Event], device: &mut D) -> Replayed {
    let initial = GraphicsState::default();
    let state = |recorded: &Option<Box<GraphicsState>>| -> GraphicsState {
        recorded
            .as_deref()
            .cloned()
            .unwrap_or_else(|| initial.clone())
    };
    let mut done = Replayed::default();
    // How many brackets the *recording* has open inside the one the device
    // declined, that one included. The recording balances every bracket it
    // entered — an end arrives only for a question answered yes — so the
    // matching end is the one that brings this back to zero, whichever of
    // the three kinds it is.
    let mut skipping = 0usize;

    for (index, event) in events.iter().enumerate() {
        if skipping > 0 {
            done.skipped += 1;
            match event {
                Event::BeginForm { entered: true, .. }
                | Event::BeginGroup { entered: true, .. }
                | Event::BeginSoftMask { entered: true, .. } => skipping += 1,
                Event::EndForm { .. } | Event::EndGroup | Event::EndSoftMask => skipping -= 1,
                _ => {}
            }
            continue;
        }
        if device.is_cancelled() {
            done.cancelled = true;
            done.skipped += events.len() - index;
            break;
        }
        done.delivered += 1;
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
                        skipping = 1;
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
                        skipping = 1;
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
                        skipping = 1;
                    } else {
                        device.end_soft_mask();
                    }
                }
            }
            Event::EndSoftMask => device.end_soft_mask(),
            Event::ClearSoftMask => device.clear_soft_mask(),
        }
    }
    done
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
}
