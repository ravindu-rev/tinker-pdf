//! A filtering device: the render controls' classes of content
//! (`docs/design/render-controls.md`, RC-12).
//!
//! [`Culled`] stands in front of another [`Device`] and forwards every call to
//! it but those of the classes it culls, and forces the colour of the text or
//! the path class to black when asked. Each call is in one class — text,
//! paths, clips, images or shadings — by what it is and, for one rule, by
//! where it arrives: every call that draws or clips **inside a text object**
//! is the text's. That is how a Type 3 glyph's paints are text: 9.6.5 runs a
//! Type 3 glyph's procedure where the glyph is shown, so its fills, strokes,
//! images and shadings arrive between the `begin_text` and the `end_text` of
//! the text object that shows it, with no `show_glyph` at all, and 8.2's
//! grammar admits no path, `Do` or `sh` inside a text object otherwise.
//!
//! A class is a kind of call, not what the call depicts, so every
//! `clip_path` outside a text object is a clip: `W` and `W*`, and the clip a
//! form's or a mask group's `/BBox` makes (8.10.1).
//!
//! A culled call is not forwarded at all, so the device behind sees none of
//! it — a renderer neither paints it, nor ends an image run for it, nor warns
//! about it. The questions — [`Device::begin_form`], [`Device::begin_group`],
//! [`Device::begin_soft_mask`], [`Device::is_cancelled`],
//! [`Device::answers_alike`], [`Device::composites_colorants`] and
//! [`Device::glyph_queries`] — are the inner device's to answer, so the interpreter runs exactly what it would
//! run with the device unwrapped, and every other call that brackets or
//! notices something is forwarded as it came. A form's drawing the
//! interpreter keeps is of the calls this filter is handed, before it drops
//! any, so a replay of one is filtered as the drawing was.

use std::sync::Arc;

use crate::device::{Device, Glyph, GlyphQueries, ImageRef, MarkedProps, PathSegment};
use crate::interpret::{Group, MaskGroup};
use crate::state::{ColorFamily, ColorantTints, GraphicsState, Rgb, Rgb16};

/// The ink of forced black: `K` alone, at full (8.6.4.4's `0 0 0 1 k`), never a
/// rich black.
const BLACK_INK: [u8; 4] = [0, 0, 0, 255];

/// One class of content (`docs/design/render-controls.md`, "The classes").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// Every glyph shown, in every text rendering mode, and every call that
    /// draws or clips inside a text object: a Type 3 glyph's paints.
    Text,
    /// Fills and strokes outside a text object, a pattern fill among them.
    Paths,
    /// Clips outside a text object: `W`, `W*` and a form's or a mask group's
    /// `/BBox`.
    Clips,
    /// Images outside a text object: an image XObject, an image mask and an
    /// inline image alike.
    Images,
    /// `sh` outside a text object.
    Shadings,
}

/// Which classes a [`Culled`] device drops: each `true` drops its class.
///
/// [`Cull::NONE`], the default, drops nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cull {
    /// Drop every glyph, and everything a text object draws or clips.
    pub text: bool,
    /// Drop fills and strokes.
    pub paths: bool,
    /// Drop clips.
    pub clips: bool,
    /// Drop images.
    pub images: bool,
    /// Drop `sh`.
    pub shadings: bool,
}

impl Cull {
    /// Nothing dropped.
    pub const NONE: Cull = Cull {
        text: false,
        paths: false,
        clips: false,
        images: false,
        shadings: false,
    };

    /// The *text only* preset: paths, images and shadings dropped, text and
    /// clips kept, so text shows where the page shows it and clipped as the
    /// page clips it.
    pub const TEXT_ONLY: Cull = Cull {
        text: false,
        paths: true,
        clips: false,
        images: true,
        shadings: true,
    };

    /// The *no text* preset: text dropped, everything else kept.
    pub const NO_TEXT: Cull = Cull {
        text: true,
        paths: false,
        clips: false,
        images: false,
        shadings: false,
    };

    /// Whether `class` is dropped.
    #[must_use]
    pub fn drops(&self, class: Class) -> bool {
        match class {
            Class::Text => self.text,
            Class::Paths => self.paths,
            Class::Clips => self.clips,
            Class::Images => self.images,
            Class::Shadings => self.shadings,
        }
    }

    /// Whether nothing is dropped.
    #[must_use]
    pub fn is_none(&self) -> bool {
        *self == Cull::NONE
    }
}

/// Which classes a [`Culled`] device forces black: each `true` forces its
/// class. [`Black::NONE`], the default, forces nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Black {
    /// Every glyph, and everything a text object draws.
    pub text: bool,
    /// Every fill and stroke outside a text object.
    pub paths: bool,
}

impl Black {
    /// Nothing forced.
    pub const NONE: Black = Black {
        text: false,
        paths: false,
    };

    /// Whether `class` is forced black. Only text and paths can be: a clip
    /// paints nothing, an image paints its own samples and a shading its own
    /// colours.
    #[must_use]
    pub fn forces(&self, class: Class) -> bool {
        match class {
            Class::Text => self.text,
            Class::Paths => self.paths,
            Class::Clips | Class::Images | Class::Shadings => false,
        }
    }

    /// Whether nothing is forced.
    #[must_use]
    pub fn is_none(&self) -> bool {
        *self == Black::NONE
    }
}

/// `state` with its paint forced black, and nothing else changed: the fill
/// and stroke colours what `0 0 0 1 k` sets where no default space remaps
/// DeviceCMYK — light's black as bytes and at sixteen bits, the `K` plate
/// alone, DeviceCMYK's family, no pattern, no named colour space, no source
/// for a rendering intent to resolve again, and `colorants` what each paints
/// by colorant: [`Culled`] hands DeviceCMYK's four process colorants at the
/// `K` plate alone where its device composites colorants, which is what `k`
/// carries there, and `None` where it does not. The alphas, the blend mode,
/// the transform, the text state, the line parameters and the stored
/// parameters are the state's own, so a forced mark still fades by its
/// constant alpha.
#[must_use]
pub fn forced_black(
    state: &GraphicsState,
    colorants: Option<&Arc<ColorantTints>>,
) -> GraphicsState {
    GraphicsState {
        fill_color: Rgb::BLACK,
        stroke_color: Rgb::BLACK,
        fill_wide: Rgb16::BLACK,
        stroke_wide: Rgb16::BLACK,
        fill_ink: Some(BLACK_INK),
        stroke_ink: Some(BLACK_INK),
        fill_family: ColorFamily::Cmyk,
        stroke_family: ColorFamily::Cmyk,
        fill_space: None,
        stroke_space: None,
        fill_pattern: None,
        stroke_pattern: None,
        fill_source: None,
        stroke_source: None,
        fill_colorants: colorants.cloned(),
        stroke_colorants: colorants.cloned(),
        ..state.clone()
    }
}

/// A device in front of `D` that drops the calls of the classes it culls and
/// forces a class's colour black (`docs/design/render-controls.md`).
///
/// It counts the text objects the calls arrive inside. A text object a Type 3
/// glyph's procedure opens inside the one showing it is one more, and the
/// text rule holds until the outermost `end_text`. No text object outlives
/// the stream it was opened in: one a form, a transparency group or a mask's
/// group leaves open ends with its stream, and a `restore_state` — the
/// interpreter's own around a glyph procedure among them — ends every text
/// object opened since its `save_state` and reopens none an `end_text`
/// closed. So a damaged stream that never ends its text makes nothing after
/// that stream text.
pub struct Culled<'d, D: Device + ?Sized> {
    inner: &'d mut D,
    cull: Cull,
    black: Black,
    /// What a colour forced black paints by colorant: DeviceCMYK's four
    /// process colorants at the `K` plate alone where the inner device
    /// composites colorants, as `0 0 0 1 k` carries them there, and none where
    /// it does not — asked once, as the interpreter asks.
    black_colorants: Option<Arc<ColorantTints>>,
    /// The text objects open.
    text_objects: u32,
    /// The count at each save the inner device holds, innermost last: what
    /// its restore lowers the count to, at most.
    saves: Vec<u32>,
    /// The count at each form, group and mask's group the inner device
    /// entered, innermost last: what its end puts back.
    streams: Vec<u32>,
}

impl<'d, D: Device + ?Sized> Culled<'d, D> {
    /// A filter dropping `cull`'s classes and forcing `black`'s in front of
    /// `inner`.
    pub fn new(inner: &'d mut D, cull: Cull, black: Black) -> Culled<'d, D> {
        let black_colorants = inner
            .composites_colorants()
            .then(|| Arc::new(ColorantTints::device_cmyk(BLACK_INK)));
        Culled {
            inner,
            cull,
            black,
            black_colorants,
            text_objects: 0,
            saves: Vec::new(),
            streams: Vec::new(),
        }
    }

    /// The class of a call that draws or clips arriving now, whose class
    /// outside a text object is `outside`.
    fn class_of(&self, outside: Class) -> Class {
        if self.text_objects > 0 {
            Class::Text
        } else {
            outside
        }
    }

    /// Whether a call of `class` is forwarded.
    fn passes(&self, class: Class) -> bool {
        !self.cull.drops(class)
    }

    /// The state a call of `class` is forwarded with: `state`, or `state`
    /// forced black when its class is.
    fn painted<'s>(&self, class: Class, state: &'s GraphicsState) -> PaintState<'s> {
        if self.black.forces(class) {
            PaintState::Forced(forced_black(state, self.black_colorants.as_ref()))
        } else {
            PaintState::Kept(state)
        }
    }

    /// Notes an entered form, group or mask's group.
    fn entered(&mut self, entered: bool) {
        if entered {
            self.streams.push(self.text_objects);
        }
    }

    /// Notes that the innermost entered stream ended.
    fn left(&mut self) {
        if let Some(at) = self.streams.pop() {
            self.text_objects = at;
        }
    }
}

/// The state a forwarded call carries: the call's own, or its forced copy.
enum PaintState<'s> {
    Kept(&'s GraphicsState),
    Forced(GraphicsState),
}

impl PaintState<'_> {
    fn get(&self) -> &GraphicsState {
        match self {
            PaintState::Kept(state) => state,
            PaintState::Forced(state) => state,
        }
    }
}

impl<D: Device + ?Sized> Device for Culled<'_, D> {
    fn show_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if !self.passes(Class::Text) {
            return;
        }
        let state = self.painted(Class::Text, state);
        self.inner.show_glyph(glyph, state.get());
    }

    /// The text's, as the glyph's own paints are.
    fn show_type3_glyph(&mut self, glyph: &Glyph, state: &GraphicsState) {
        if !self.passes(Class::Text) {
            return;
        }
        let state = self.painted(Class::Text, state);
        self.inner.show_type3_glyph(glyph, state.get());
    }

    fn begin_text(&mut self) {
        self.text_objects = self.text_objects.saturating_add(1);
        self.inner.begin_text();
    }

    fn end_text(&mut self) {
        self.text_objects = self.text_objects.saturating_sub(1);
        self.inner.end_text();
    }

    fn fill_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let class = self.class_of(Class::Paths);
        if !self.passes(class) {
            return;
        }
        let state = self.painted(class, state);
        self.inner.fill_path(path, state.get(), even_odd);
    }

    fn stroke_path(&mut self, path: &[PathSegment], state: &GraphicsState) {
        let class = self.class_of(Class::Paths);
        if !self.passes(class) {
            return;
        }
        let state = self.painted(class, state);
        self.inner.stroke_path(path, state.get());
    }

    fn clip_path(&mut self, path: &[PathSegment], state: &GraphicsState, even_odd: bool) {
        let class = self.class_of(Class::Clips);
        if !self.passes(class) {
            return;
        }
        let state = self.painted(class, state);
        self.inner.clip_path(path, state.get(), even_odd);
    }

    fn save_state(&mut self) {
        self.saves.push(self.text_objects);
        self.inner.save_state();
    }

    fn restore_state(&mut self) {
        if let Some(at) = self.saves.pop() {
            self.text_objects = self.text_objects.min(at);
        }
        self.inner.restore_state();
    }

    fn draw_image(&mut self, image: &ImageRef, state: &GraphicsState) {
        let class = self.class_of(Class::Images);
        if !self.passes(class) {
            return;
        }
        let state = self.painted(class, state);
        self.inner.draw_image(image, state.get());
    }

    fn draw_shading(&mut self, name: &[u8], state: &GraphicsState) {
        let class = self.class_of(Class::Shadings);
        if !self.passes(class) {
            return;
        }
        let state = self.painted(class, state);
        self.inner.draw_shading(name, state.get());
    }

    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        visible: bool,
        hidden_layer: Option<&str>,
        props: Option<&MarkedProps>,
    ) {
        self.inner
            .begin_marked_content(tag, visible, hidden_layer, props);
    }

    fn end_marked_content(&mut self) {
        self.inner.end_marked_content();
    }

    fn begin_form(&mut self, id: u64, name: &[u8]) -> bool {
        let entered = self.inner.begin_form(id, name);
        self.entered(entered);
        entered
    }

    fn end_form(&mut self, id: u64) {
        self.left();
        self.inner.end_form(id);
    }

    fn begin_group(&mut self, group: Group, state: &GraphicsState) -> bool {
        let entered = self.inner.begin_group(group, state);
        self.entered(entered);
        entered
    }

    fn end_group(&mut self) {
        self.left();
        self.inner.end_group();
    }

    fn begin_soft_mask(
        &mut self,
        mask: &MaskGroup,
        bbox: &[PathSegment],
        state: &GraphicsState,
    ) -> bool {
        let entered = self.inner.begin_soft_mask(mask, bbox, state);
        self.entered(entered);
        entered
    }

    fn end_soft_mask(&mut self) {
        self.left();
        self.inner.end_soft_mask();
    }

    fn clear_soft_mask(&mut self) {
        self.inner.clear_soft_mask();
    }

    fn group_budget_spent(&mut self) {
        self.inner.group_budget_spent();
    }

    fn recursion_refused(&mut self) {
        self.inner.recursion_refused();
    }

    fn nested_budget_spent(&mut self) {
        self.inner.nested_budget_spent();
    }

    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    fn answers_alike(&self, events: &[crate::record::Event]) -> bool {
        self.inner.answers_alike(events)
    }

    fn composites_colorants(&self) -> bool {
        self.inner.composites_colorants()
    }

    fn glyph_queries(&self) -> GlyphQueries {
        self.inner.glyph_queries()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::interpret::{
        interpret, FontSource, Form, FormDrawn, FormInvocation, FormRecording, SoftMask,
        StateParameters, Type3Glyph,
    };
    use crate::record::{Answers, Event, EventKind, RecordingDevice};
    use crate::state::{ColorantKind, ColorantTint, Matrix, RenderingIntent};

    /// What `/CS0`, a three-component space only the resource seam reads,
    /// resolves every colour to.
    const CS0: Rgb = Rgb {
        r: 51,
        g: 102,
        b: 153,
    };

    /// A scope holding one of everything: `/F1`, an ordinary font; `/T3`, a
    /// Type 3 font whose glyph fills, strokes, clips, draws an image and a
    /// shading, and shows a glyph of `/F1` in a text object of its own before
    /// a last fill; `/U3`, a Type 3 font whose glyph opens a text object it
    /// never ends and fills inside it; `/Fm`, a form that fills and shows a
    /// glyph; `/Gp`, a transparency group that fills; `/Open`, a form that
    /// opens a text object it never ends and fills inside it; `/SM`, a
    /// graphics state whose soft mask's group fills; and `/CS0`, a colour
    /// space the seam resolves, so a colour set in it keeps its source. Every
    /// other XObject name is an image.
    struct Everything;

    impl FontSource for Everything {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|b| (u32::from(*b), char::from(*b).to_string(), 500.0))
                .collect()
        }

        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 0.0, 0.0)
        }

        fn resolve_color(
            &self,
            space: &[u8],
            components: &[f64],
            _intent: RenderingIntent,
        ) -> Option<Rgb> {
            (space == b"CS0" && components.len() == 3).then_some(CS0)
        }

        fn color_components(&self, space: &[u8]) -> Option<usize> {
            (space == b"CS0").then_some(3)
        }

        fn type3_glyph(&self, font: &[u8], _code: u32) -> Option<Type3Glyph> {
            let procedure = |content: &[u8], num: u64| Type3Glyph {
                content: content.to_vec(),
                matrix: Matrix::scale(0.001, 0.001),
                stream: num << 16,
            };
            match font {
                b"T3" => Some(procedure(
                    b"0 0 m 500 0 l 500 700 l h f \
                      0 0 m 500 700 l S \
                      0 0 500 700 re W n \
                      q 500 0 0 700 0 0 cm /Im Do Q \
                      /Sh sh \
                      BT /F1 100 Tf (y) Tj ET \
                      0 0 100 100 re f",
                    9,
                )),
                b"U3" => Some(procedure(
                    b"BT /F1 100 Tf (u) Tj 1 0 0 rg 0 0 500 500 re f",
                    10,
                )),
                _ => None,
            }
        }

        fn form(&self, name: &[u8]) -> Option<Form> {
            let form = |content: &[u8], group: Option<Group>, num: u64| Form {
                content: content.to_vec(),
                matrix: Matrix::IDENTITY,
                bbox: Some([0.0, 0.0, 50.0, 50.0]),
                group,
                stream: num << 16,
            };
            match name {
                b"Fm" => Some(form(
                    b"0 0 1 rg 1 1 5 5 re f BT /F1 10 Tf (e) Tj ET",
                    None,
                    5,
                )),
                b"Gp" => Some(form(
                    b"0 1 0 rg 2 2 6 6 re f",
                    Some(Group {
                        isolated: true,
                        knockout: false,
                        space: None,
                    }),
                    6,
                )),
                b"Open" => Some(form(b"BT /F1 10 Tf (o) Tj 1 0 0 rg 0 0 5 5 re f", None, 7)),
                _ => None,
            }
        }

        fn ext_g_state(&self, name: &[u8]) -> Option<Arc<StateParameters>> {
            (name == b"SM").then(|| {
                Arc::new(StateParameters {
                    soft_mask: Some(SoftMask::Group(Box::new(MaskGroup {
                        form: Form {
                            content: b"1 g 0 0 50 50 re f".to_vec(),
                            matrix: Matrix::IDENTITY,
                            bbox: Some([0.0, 0.0, 50.0, 50.0]),
                            group: Some(Group::default()),
                            stream: 8 << 16,
                        },
                        luminosity: true,
                        backdrop: None,
                        transfer: None,
                    }))),
                    ..StateParameters::default()
                })
            })
        }
    }

    /// Content holding every class: ordinary glyphs, a Type 3 glyph, a glyph
    /// in text-clip mode 7, a clip, a fill in `/CS0`, a coloured stroke, an
    /// image, an image mask (an image call like any other), an inline image,
    /// `sh`, a shading-pattern fill, a form, a group and a soft mask with a
    /// fill under it.
    const EVERYTHING: &[u8] = b"BT /F1 12 Tf 10 10 Td (ab) Tj ET \
        BT /T3 12 Tf 30 10 Td (c) Tj ET \
        BT 7 Tr /F1 12 Tf 50 10 Td (d) Tj ET \
        q 0 0 100 100 re W n \
        /CS0 cs 0.2 0.4 0.6 sc 10 20 30 40 re f \
        1 0 0 RG 2 w 0 0 m 50 50 l S \
        q 20 0 0 20 5 5 cm /Im Do Q \
        q 10 0 0 10 40 40 cm /Stencil Do Q \
        q 8 0 0 8 60 60 cm BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000FFFFFFFFFFFF000000> EI Q \
        /Sh sh \
        /Pattern cs /P1 scn 0 50 20 20 re f \
        /Fm Do \
        /Gp Do \
        /SM gs 0 g 5 5 10 10 re f \
        Q";

    /// What the recorder answers: every form, group and soft mask entered, so
    /// what runs inside each is recorded.
    fn entering() -> Answers {
        Answers {
            forms: true,
            groups: true,
            soft_masks: true,
            cancelled: false,
        }
    }

    /// `content` interpreted into a recorder answering `answers`, through a
    /// filter dropping and forcing what `filtered` says when there is one.
    fn recorded(content: &[u8], answers: Answers, filtered: Option<(Cull, Black)>) -> Vec<Event> {
        let mut recorder = RecordingDevice::new().answering(answers);
        match filtered {
            Some((cull, black)) => {
                let mut culled = Culled::new(&mut recorder, cull, black);
                interpret(content, Matrix::IDENTITY, &mut culled, &Everything);
            }
            None => interpret(content, Matrix::IDENTITY, &mut recorder, &Everything),
        }
        recorder.into_events()
    }

    /// Each event's class as the design's table gives it, `None` for an event
    /// that is no class's: a call that draws or clips inside a text object is
    /// the text's; a text object an entered form, group or mask's group
    /// leaves open ends with it; and a restore ends every text object opened
    /// since its save and reopens none.
    fn classes(events: &[Event]) -> Vec<Option<Class>> {
        let mut open: u32 = 0;
        let mut saves: Vec<u32> = Vec::new();
        let mut streams: Vec<u32> = Vec::new();
        let filed = |open: u32, outside: Class| Some(if open > 0 { Class::Text } else { outside });
        let mut found = Vec::with_capacity(events.len());
        for event in events {
            found.push(match event {
                Event::ShowGlyph { .. } => Some(Class::Text),
                Event::FillPath { .. } | Event::StrokePath { .. } => filed(open, Class::Paths),
                Event::ClipPath { .. } => filed(open, Class::Clips),
                Event::DrawImage { .. } => filed(open, Class::Images),
                Event::DrawShading { .. } => filed(open, Class::Shadings),
                _ => None,
            });
            match event {
                Event::BeginText => open += 1,
                Event::EndText => open = open.saturating_sub(1),
                Event::SaveState => saves.push(open),
                Event::RestoreState => {
                    if let Some(at) = saves.pop() {
                        open = open.min(at);
                    }
                }
                Event::BeginForm { entered: true, .. }
                | Event::BeginGroup { entered: true, .. }
                | Event::BeginSoftMask { entered: true, .. } => streams.push(open),
                Event::EndForm { .. } | Event::EndGroup | Event::EndSoftMask => {
                    if let Some(at) = streams.pop() {
                        open = at;
                    }
                }
                _ => {}
            }
        }
        found
    }

    /// `events` without those of the classes `cull` drops.
    fn less(events: &[Event], cull: Cull) -> Vec<Event> {
        events
            .iter()
            .zip(classes(events))
            .filter(|(_, class)| !class.is_some_and(|class| cull.drops(class)))
            .map(|(event, _)| event.clone())
            .collect()
    }

    /// How many of `events` are of `kind`.
    fn count(events: &[Event], kind: EventKind) -> usize {
        events.iter().filter(|event| event.kind() == kind).count()
    }

    /// The counts of the six kinds that draw or clip, in the order glyphs,
    /// fills, strokes, clips, images, shadings.
    fn drawn(events: &[Event]) -> [usize; 6] {
        [
            EventKind::ShowGlyph,
            EventKind::FillPath,
            EventKind::StrokePath,
            EventKind::ClipPath,
            EventKind::DrawImage,
            EventKind::DrawShading,
        ]
        .map(|kind| count(events, kind))
    }

    /// Every single class, and the two presets.
    fn culls() -> Vec<(&'static str, Cull)> {
        let one = |text, paths, clips, images, shadings| Cull {
            text,
            paths,
            clips,
            images,
            shadings,
        };
        vec![
            ("text", one(true, false, false, false, false)),
            ("paths", one(false, true, false, false, false)),
            ("clips", one(false, false, true, false, false)),
            ("images", one(false, false, false, true, false)),
            ("shadings", one(false, false, false, false, true)),
            ("text only", Cull::TEXT_ONLY),
            ("no text", Cull::NO_TEXT),
        ]
    }

    /// **Each class culled leaves exactly the recorded events of the others.**
    /// The content holds every class — a Type 3 glyph among ordinary ones, a
    /// text clip, an inline image, an image mask, `sh`, a shading-pattern
    /// fill, a form, a group and a soft mask — and its recording through the
    /// filter is the unfiltered recording less the class's events, for each
    /// class and both presets. The Type 3 glyph's fill, stroke, clip, image
    /// and shading are text's: they go with *no text* and stay with *text
    /// only*, which the counts written out below hold by hand. A filter that
    /// drops nothing records what the device unwrapped records.
    #[test]
    fn each_class_culled_leaves_exactly_the_recorded_events_of_the_others() {
        let whole = recorded(EVERYTHING, entering(), None);
        let found = classes(&whole);
        for class in [
            Class::Text,
            Class::Paths,
            Class::Clips,
            Class::Images,
            Class::Shadings,
        ] {
            assert!(
                found.contains(&Some(class)),
                "the content holds a {class:?} call: {whole:#?}"
            );
        }
        // The Type 3 glyph's own paints, which arrive with no glyph shown:
        // a fill, a stroke, a clip, an image and a shading inside a text
        // object.
        let in_text: Vec<EventKind> = whole
            .iter()
            .zip(&found)
            .filter(|(event, class)| {
                **class == Some(Class::Text) && !matches!(event, Event::ShowGlyph { .. })
            })
            .map(|(event, _)| event.kind())
            .collect();
        for kind in [
            EventKind::FillPath,
            EventKind::StrokePath,
            EventKind::ClipPath,
            EventKind::DrawImage,
            EventKind::DrawShading,
        ] {
            assert!(
                in_text.contains(&kind),
                "a Type 3 glyph's {kind:?} is text: {in_text:?}"
            );
        }
        assert!(
            whole.iter().any(|event| matches!(
                event,
                Event::FillPath { state: Some(state), .. } if state.fill_pattern.is_some()
            )),
            "a pattern fill"
        );
        assert!(
            whole
                .iter()
                .any(|event| matches!(event, Event::DrawImage { image, .. } if image.inline)),
            "an inline image"
        );
        for kind in [
            EventKind::BeginForm,
            EventKind::BeginGroup,
            EventKind::BeginSoftMask,
        ] {
            assert!(
                whole.iter().any(|event| event.kind() == kind),
                "the content reaches a {kind:?}"
            );
        }

        for (what, cull) in culls() {
            let filtered = recorded(EVERYTHING, entering(), Some((cull, Black::NONE)));
            assert_eq!(filtered, less(&whole, cull), "{what} culled");
            assert!(filtered.len() < whole.len(), "{what}: something was culled");
        }
        assert_eq!(
            recorded(EVERYTHING, entering(), Some((Cull::NONE, Black::NONE))),
            whole,
            "a filter dropping nothing"
        );

        // By hand. Glyphs: `a`, `b`, the Type 3 procedure's `y`, `d` in mode
        // 7 and the form's `e` — a Type 3 glyph shows none of its own. Fills:
        // the procedure's two, the `/CS0` one, the pattern fill, the form's,
        // the group's, the mask group's and the last. Strokes: the procedure's and
        // the red one. Clips: the procedure's `W n`, the page's, and the
        // `/BBox` of the form, the group's form and the mask's group. Images:
        // the procedure's, `/Im`, `/Stencil` and the inline one. Shadings: the
        // procedure's and the page's.
        assert_eq!(drawn(&whole), [5, 8, 2, 5, 4, 2], "unfiltered");
        assert_eq!(
            drawn(&recorded(
                EVERYTHING,
                entering(),
                Some((Cull::NO_TEXT, Black::NONE))
            )),
            [0, 6, 1, 4, 3, 1],
            "no text: the procedure's paints go with the glyphs"
        );
        assert_eq!(
            drawn(&recorded(
                EVERYTHING,
                entering(),
                Some((Cull::TEXT_ONLY, Black::NONE))
            )),
            [5, 2, 1, 5, 1, 1],
            "text only: the glyphs, the procedure's paints and every clip"
        );
    }

    /// `state` with the fields forced black put back from `original`.
    fn restored(state: &GraphicsState, original: &GraphicsState) -> GraphicsState {
        GraphicsState {
            fill_color: original.fill_color,
            stroke_color: original.stroke_color,
            fill_wide: original.fill_wide,
            stroke_wide: original.stroke_wide,
            fill_ink: original.fill_ink,
            stroke_ink: original.stroke_ink,
            fill_family: original.fill_family,
            stroke_family: original.stroke_family,
            fill_space: original.fill_space.clone(),
            stroke_space: original.stroke_space.clone(),
            fill_pattern: original.fill_pattern.clone(),
            stroke_pattern: original.stroke_pattern.clone(),
            fill_source: original.fill_source.clone(),
            stroke_source: original.stroke_source.clone(),
            fill_colorants: original.fill_colorants.clone(),
            stroke_colorants: original.stroke_colorants.clone(),
            ..state.clone()
        }
    }

    /// `event` with its state replaced by `state`.
    fn with_state(event: &Event, state: GraphicsState) -> Event {
        let mut event = event.clone();
        match &mut event {
            Event::ShowGlyph { state: slot, .. }
            | Event::FillPath { state: slot, .. }
            | Event::StrokePath { state: slot, .. }
            | Event::ClipPath { state: slot, .. }
            | Event::DrawImage { state: slot, .. }
            | Event::DrawShading { state: slot, .. } => *slot = Some(Box::new(state)),
            _ => {}
        }
        event
    }

    /// **Forced black rewrites only its class.** For text, for paths and for
    /// both: every call of a forced class is forwarded with its fill and
    /// stroke colours light's black, its inks the `K` plate alone, its
    /// families DeviceCMYK's, no pattern, no named space and no source, and
    /// every other field of its state as it was — the alphas, the transform,
    /// the text state, the line parameters — and every call of every other
    /// class, and every other event, is forwarded exactly as it came. The
    /// Type 3 glyph's paints are text's; the pattern fill is a path's and
    /// paints black, and so does the `/CS0` fill, whose source goes with its
    /// colour. Nothing is dropped.
    #[test]
    fn forced_black_rewrites_only_its_class() {
        let whole = recorded(EVERYTHING, entering(), None);
        let found = classes(&whole);
        assert!(
            whole.iter().any(|event| matches!(
                event,
                Event::FillPath { state: Some(state), .. }
                    if state.fill_color == CS0 && state.fill_source.is_some()
            )),
            "a fill in a colour the seam resolved, which keeps its source"
        );
        for black in [
            Black {
                text: true,
                paths: false,
            },
            Black {
                text: false,
                paths: true,
            },
            Black {
                text: true,
                paths: true,
            },
        ] {
            let forced = recorded(EVERYTHING, entering(), Some((Cull::NONE, black)));
            assert_eq!(forced.len(), whole.len(), "{black:?}: nothing dropped");
            let mut rewrote = 0;
            for ((got, want), class) in forced.iter().zip(&whole).zip(&found) {
                match (class, want.state()) {
                    (Some(class), Some(original)) if black.forces(*class) => {
                        let state = got.state().expect("the call keeps its state");
                        assert_eq!(state.fill_color, Rgb::BLACK, "{class:?}");
                        assert_eq!(state.stroke_color, Rgb::BLACK, "{class:?}");
                        assert_eq!(state.fill_ink, Some(BLACK_INK), "{class:?}");
                        assert_eq!(state.stroke_ink, Some(BLACK_INK), "{class:?}");
                        assert_eq!(state.fill_family, ColorFamily::Cmyk, "{class:?}");
                        assert_eq!(state.stroke_family, ColorFamily::Cmyk, "{class:?}");
                        assert_eq!(state.fill_source, None, "{class:?}");
                        assert_eq!(state.stroke_source, None, "{class:?}");
                        assert_eq!(state.fill_pattern, None, "{class:?}");
                        assert_eq!(state.stroke_pattern, None, "{class:?}");
                        assert_eq!(state.fill_space, None, "{class:?}");
                        assert_eq!(state.stroke_space, None, "{class:?}");
                        // A recorder compositing no colorants: none, as `k`.
                        assert_eq!(state.fill_colorants, None, "{class:?}");
                        assert_eq!(state.stroke_colorants, None, "{class:?}");
                        assert_eq!(
                            &restored(state, original),
                            original,
                            "{class:?}: every other field as it was"
                        );
                        assert_eq!(
                            got,
                            &with_state(want, state.clone()),
                            "{class:?}: the call itself as it was"
                        );
                        rewrote += 1;
                    }
                    _ => assert_eq!(got, want, "{black:?}: a call of no forced class"),
                }
            }
            assert!(rewrote > 0, "{black:?} forced something");
        }
        // The pattern fill, a path, under paths forced black: no pattern.
        let paths = recorded(
            EVERYTHING,
            entering(),
            Some((
                Cull::NONE,
                Black {
                    text: false,
                    paths: true,
                },
            )),
        );
        assert!(
            !paths.iter().any(|event| matches!(
                event,
                Event::FillPath { state: Some(state), .. }
                    if state.fill_pattern.is_some() || state.fill_source.is_some()
            )),
            "the pattern fill and the `/CS0` fill paint black"
        );
    }

    /// **The questions are the inner device's.** A recorder that enters no
    /// form, group or soft mask answers so through the filter, which runs
    /// none of them, and records the unfiltered recording less the culled
    /// class; a recorder that is cancelled is cancelled through it; and
    /// whether a recording's answers are the ones a recorder would give
    /// (`answers_alike`) is the recorder's answer through the filter, for one
    /// that would give them and one that would not; and so is whether the
    /// device composites colorants, for one that does and one that does not.
    #[test]
    fn the_questions_are_the_inner_devices() {
        let declining = Answers {
            forms: false,
            groups: false,
            soft_masks: false,
            cancelled: false,
        };
        let whole = recorded(EVERYTHING, declining, None);
        assert!(
            whole.iter().all(|event| !matches!(
                event,
                Event::EndForm { .. } | Event::EndGroup | Event::EndSoftMask
            )),
            "nothing entered"
        );
        for (what, cull) in culls() {
            assert_eq!(
                recorded(EVERYTHING, declining, Some((cull, Black::NONE))),
                less(&whole, cull),
                "{what}, nothing entered"
            );
        }

        let mut recorder = RecordingDevice::new().answering(Answers {
            cancelled: true,
            ..entering()
        });
        let culled = Culled::new(&mut recorder, Cull::NO_TEXT, Black::NONE);
        assert!(culled.is_cancelled(), "cancelled through the filter");
        let mut recorder = RecordingDevice::new().answering(entering());
        let culled = Culled::new(&mut recorder, Cull::NO_TEXT, Black::NONE);
        assert!(!culled.is_cancelled());

        let entered = recorded(EVERYTHING, entering(), None);
        for (answers, alike) in [(entering(), true), (declining, false)] {
            let mut recorder = RecordingDevice::new().answering(answers);
            assert_eq!(recorder.answers_alike(&entered), alike, "{answers:?}");
            for (what, cull) in culls() {
                let culled = Culled::new(&mut recorder, cull, Black::NONE);
                assert_eq!(
                    culled.answers_alike(&entered),
                    alike,
                    "{answers:?}, {what} culled"
                );
            }
        }

        for colorants in [false, true] {
            let mut recorder = RecordingDevice::new().recording_colorants(colorants);
            for (what, cull) in culls() {
                let culled = Culled::new(&mut recorder, cull, Black::NONE);
                assert_eq!(
                    culled.composites_colorants(),
                    colorants,
                    "{what} culled: whether the device composites colorants"
                );
            }
        }

        // And what the device asks of its glyphs: nothing for a recorder, and
        // a glyph index for a text device standing glyphs for theirs.
        for unmapped in [crate::text::Unmapped::Drop, crate::text::Unmapped::Glyph] {
            let mut text = crate::text::TextDevice::with_options(&crate::text::ExtractionOptions {
                unmapped,
                ..crate::text::ExtractionOptions::default()
            });
            let asks = text.glyph_queries();
            for (what, cull) in culls() {
                let culled = Culled::new(&mut text, cull, Black::NONE);
                assert_eq!(culled.glyph_queries(), asks, "{what} culled, {unmapped:?}");
            }
        }
        let mut recorder = RecordingDevice::new();
        let culled = Culled::new(&mut recorder, Cull::NO_TEXT, Black::NONE);
        assert_eq!(culled.glyph_queries(), GlyphQueries::default());
    }

    /// **Forced black paints black by colorant where the device composites
    /// colorants**, as `0 0 0 1 k` carries it there: a fill and a stroke whose
    /// colours carry a spot's colorants, forced, are forwarded with
    /// DeviceCMYK's four process colorants at the `K` plate alone; on a device
    /// compositing none, forced black carries none, as `k` does there; and
    /// unforced, each keeps its own colorants on either device.
    #[test]
    fn forced_black_paints_black_by_colorant_where_the_device_composites_them() {
        let spot = Arc::new(ColorantTints {
            kind: ColorantKind::Colorants,
            tints: vec![ColorantTint {
                name: Arc::from(&b"PANTONE 131"[..]),
                tint: 200,
                alone: None,
                zero: false,
            }],
            alternate: Some([0, 100, 200, 0]),
            process: None,
        });
        let mut state = GraphicsState::new(Matrix::IDENTITY);
        state.fill_colorants = Some(Arc::clone(&spot));
        state.stroke_colorants = Some(Arc::clone(&spot));
        let path = [
            PathSegment::MoveTo { x: 0.0, y: 0.0 },
            PathSegment::LineTo { x: 10.0, y: 10.0 },
        ];
        let black = ColorantTints::device_cmyk([0, 0, 0, 255]);
        let paths = Black {
            text: false,
            paths: true,
        };
        for composites in [true, false] {
            for (forcing, want) in [
                (paths, composites.then(|| black.clone())),
                (Black::NONE, Some((*spot).clone())),
            ] {
                let mut recorder = RecordingDevice::new().recording_colorants(composites);
                let mut culled = Culled::new(&mut recorder, Cull::NONE, forcing);
                culled.fill_path(&path, &state, false);
                culled.stroke_path(&path, &state);
                let events = recorder.into_events();
                assert_eq!(events.len(), 2, "{composites}, {forcing:?}: both forwarded");
                for event in &events {
                    let got = event.state().expect("a paint keeps its state");
                    assert_eq!(
                        got.fill_colorants.as_deref(),
                        want.as_ref(),
                        "{composites}, {forcing:?}: the fill colour's"
                    );
                    assert_eq!(
                        got.stroke_colorants.as_deref(),
                        want.as_ref(),
                        "{composites}, {forcing:?}: the stroke colour's"
                    );
                }
            }
        }
    }

    /// [`Everything`]'s fonts and forms, in a scope that keeps each form's
    /// drawing (`FontSource::kept_form`) and counts the forms it was told were
    /// drawn from what it kept.
    #[derive(Default)]
    struct Keeping {
        kept: RefCell<HashMap<(Vec<u8>, FormInvocation), Arc<FormRecording>>>,
        replayed: Cell<usize>,
    }

    impl FontSource for Keeping {
        fn decode(&self, font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            Everything.decode(font, bytes)
        }

        fn vertical_metrics(&self, font: &[u8], code: u32) -> (f64, f64, f64) {
            Everything.vertical_metrics(font, code)
        }

        fn form(&self, name: &[u8]) -> Option<Form> {
            Everything.form(name)
        }

        fn form_recording_bytes(&self) -> usize {
            1 << 20
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
            match drawn {
                FormDrawn::Replayed => self.replayed.set(self.replayed.get() + 1),
                FormDrawn::Recorded {
                    invocation,
                    recording: Some(recording),
                } => {
                    self.kept
                        .borrow_mut()
                        .insert((name.to_vec(), invocation), Arc::new(recording));
                }
                _ => {}
            }
        }
    }

    /// **A form's kept drawing replays through the filter as the form draws.**
    /// In a scope that keeps forms' drawings, `/Fm` invoked twice in the same
    /// state is drawn the second time from what was kept of the first: the
    /// calls the filter was handed, before it dropped any. Unfiltered, the
    /// replay records what interpreting the form twice records; through a
    /// filter culling each class and each preset, the second form is
    /// replayed as well — the filter answers the replay's question as the
    /// recorder behind it does — and the recording is the unfiltered one less
    /// that class's events, the replayed form's fill and glyph dropped as the
    /// interpreted one's are.
    #[test]
    fn a_kept_drawing_of_a_form_replays_through_the_filter_as_the_form_draws() {
        let content: &[u8] = b"/Fm Do /Fm Do";
        let kept = |filtered: Option<(Cull, Black)>| {
            let fonts = Keeping::default();
            let mut recorder = RecordingDevice::new().answering(entering());
            match filtered {
                Some((cull, black)) => {
                    let mut culled = Culled::new(&mut recorder, cull, black);
                    interpret(content, Matrix::IDENTITY, &mut culled, &fonts);
                }
                None => interpret(content, Matrix::IDENTITY, &mut recorder, &fonts),
            }
            (recorder.into_events(), fonts.replayed.get())
        };
        let (whole, replayed) = kept(None);
        assert_eq!(replayed, 1, "the second form is replayed");
        assert_eq!(
            whole,
            recorded(content, entering(), None),
            "a replay records what interpreting records"
        );
        assert_eq!(count(&whole, EventKind::ShowGlyph), 2, "each form's glyph");
        for (what, cull) in culls() {
            let (filtered, replayed) = kept(Some((cull, Black::NONE)));
            assert_eq!(
                replayed, 1,
                "{what}: the second form replayed through the filter"
            );
            assert_eq!(filtered, less(&whole, cull), "{what} culled");
        }
    }

    /// **No text object outlives the stream or the save it was opened in.**
    /// A form that opens a text object and never ends it, a Type 3 glyph's
    /// procedure that does the same, and a `Q` after an `ET` that closed a
    /// text object its `q` was inside: each fills red inside the text object
    /// and the page fills blue after it. The red fill is text's, so it goes
    /// with *no text* and stays with paths culled; the blue fill is a path's,
    /// so it stays with *no text* and goes with paths culled — where a filter
    /// that let the text object run on would take the page's fill for text.
    #[test]
    fn no_text_object_outlives_the_stream_or_the_save_it_was_opened_in() {
        let red = Rgb { r: 255, g: 0, b: 0 };
        let blue = Rgb { r: 0, g: 0, b: 255 };
        let paths = Cull {
            paths: true,
            ..Cull::NONE
        };
        let cases: [(&str, &[u8]); 3] = [
            ("a form", b"/Open Do 0 0 1 rg 0 0 9 9 re f"),
            (
                "a glyph procedure",
                b"BT /U3 12 Tf (u) Tj ET 0 0 1 rg 0 0 9 9 re f",
            ),
            (
                "a restore after the end",
                b"BT /F1 12 Tf (a) Tj q 1 0 0 rg 0 0 5 5 re f ET Q 0 0 1 rg 0 0 9 9 re f",
            ),
        ];
        for (what, content) in cases {
            let fills = |filtered: Option<(Cull, Black)>| -> Vec<Rgb> {
                recorded(content, entering(), filtered)
                    .iter()
                    .filter_map(|event| match event {
                        Event::FillPath {
                            state: Some(state), ..
                        } => Some(state.fill_color),
                        _ => None,
                    })
                    .collect()
            };
            assert_eq!(fills(None), [red, blue], "{what}: both fills");
            assert_eq!(
                fills(Some((Cull::NO_TEXT, Black::NONE))),
                [blue],
                "{what}: no text keeps the page's fill"
            );
            assert_eq!(
                fills(Some((paths, Black::NONE))),
                [red],
                "{what}: paths culled keeps the text object's fill"
            );
            let whole = recorded(content, entering(), None);
            for (name, cull) in culls() {
                assert_eq!(
                    recorded(content, entering(), Some((cull, Black::NONE))),
                    less(&whole, cull),
                    "{what}, {name} culled"
                );
            }
        }
    }

    /// The presets and the predicates say what they are.
    #[test]
    fn the_presets_drop_what_their_names_say() {
        for class in [
            Class::Text,
            Class::Paths,
            Class::Clips,
            Class::Images,
            Class::Shadings,
        ] {
            assert!(!Cull::NONE.drops(class), "{class:?}");
            assert_eq!(
                Cull::NO_TEXT.drops(class),
                class == Class::Text,
                "{class:?}"
            );
            assert_eq!(
                Cull::TEXT_ONLY.drops(class),
                !matches!(class, Class::Text | Class::Clips),
                "{class:?}: text only keeps text and the clips it is shown through"
            );
        }
        assert!(Cull::NONE.is_none() && Cull::default().is_none());
        assert!(!Cull::NO_TEXT.is_none() && !Cull::TEXT_ONLY.is_none());
        assert!(Black::NONE.is_none() && Black::default().is_none());
        let both = Black {
            text: true,
            paths: true,
        };
        assert!(!both.is_none());
        for class in [Class::Clips, Class::Images, Class::Shadings] {
            assert!(!both.forces(class), "{class:?} paints no forced colour");
        }
    }

    /// `/P3`, a Type 3 font whose every code stands for itself and fills its
    /// em square, the one way a glyph's text reaches a device that reads it.
    struct Squares;

    impl FontSource for Squares {
        fn decode(&self, _font: &[u8], bytes: &[u8]) -> Vec<(u32, String, f64)> {
            bytes
                .iter()
                .map(|b| (u32::from(*b), char::from(*b).to_string(), 1000.0))
                .collect()
        }

        fn vertical_metrics(&self, _font: &[u8], _code: u32) -> (f64, f64, f64) {
            (0.0, 0.0, 0.0)
        }

        fn type3_glyph(&self, font: &[u8], _code: u32) -> Option<Type3Glyph> {
            (font == b"P3").then(|| Type3Glyph {
                content: b"1000 0 d0 0 0 1000 1000 re f".to_vec(),
                matrix: Matrix::scale(0.001, 0.001),
                stream: 0,
            })
        }
    }

    /// **A Type 3 glyph's text is the text class's**, as its paints are: the
    /// report reaches a text device through a filter that keeps text, and none
    /// of it through one that drops text.
    #[test]
    fn a_type3_glyphs_text_is_the_text_class() {
        let read = |cull: Cull| {
            let mut text = crate::text::TextDevice::new();
            let mut culled = Culled::new(&mut text, cull, Black::NONE);
            interpret(
                b"BT /P3 10 Tf 0 700 Td (ok) Tj ET",
                Matrix::IDENTITY,
                &mut culled,
                &Squares,
            );
            text.finish().plain_text()
        };
        assert_eq!(read(Cull::NONE), "ok\n");
        assert_eq!(read(Cull::TEXT_ONLY), "ok\n");
        assert_eq!(read(Cull::NO_TEXT), "");
    }
}
