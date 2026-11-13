//! Drawing annotation appearance streams (12.5.5).
//!
//! An annotation is not drawn by interpreting what it means — a viewer does
//! not decide what a "square" looks like — but by running the form XObject in
//! its `/AP` through the ordinary content pipeline. That is what makes an
//! annotation look the same in every viewer, and it is why the writing side
//! synthesizes appearances rather than leaving them out.
//!
//! The one piece of real work is the algorithm in 12.5.5 that fits the form
//! onto the annotation's rectangle. Getting it wrong shifts every annotation
//! in the file by a little, which is the kind of bug that survives review
//! because each individual page still looks plausible.

use std::sync::Arc;

use tinker_pdf_content::{interpret, Device, Event, Matrix};
use tinker_pdf_cos::{pages as cos_pages, CosDocument, Dict, Object, Rect};
use tinker_pdf_render::{DisplayRecorder, Renderer};

use crate::annotations::{AnnotationFlags, AnnotationKind};
use crate::fonts::FontProvider;
use crate::render_part::NotDrawn;
use crate::resources::{Operation, PageResources};
use crate::Usage;

/// A device an appearance can be drawn through: one that resolves images,
/// shadings and patterns by name in a scope it is handed, because an
/// appearance's names are its own (12.5.5). The renderer, and the SVG writer
/// when it draws a page the direct way.
pub(crate) trait AppearanceDevice: Device {
    /// Resolves names in `scope` until the matching [`pop_scope`].
    ///
    /// [`pop_scope`]: AppearanceDevice::pop_scope
    fn push_scope(&mut self, scope: Arc<PageResources>);
    /// Goes back to the scope in force before the last [`push_scope`].
    ///
    /// [`push_scope`]: AppearanceDevice::push_scope
    fn pop_scope(&mut self);

    /// Told before each annotation [`draw`] hands on, drawn or hidden,
    /// whether it is a widget: what a command stream's writer records each
    /// appearance with, so a replay draws the kinds a render asks for. Every
    /// other device draws what it is handed, and is told nothing.
    fn annotation(&mut self, _widget: bool) {}
}

impl AppearanceDevice for Renderer<'_, PageResources> {
    fn push_scope(&mut self, scope: Arc<PageResources>) {
        self.push_resources(scope);
    }

    fn pop_scope(&mut self) {
        self.pop_resources();
    }
}

/// Which annotations a render draws by their kind: widget annotations
/// (12.5.6.19), and every other kind beside them
/// (`crate::RenderOptions::widgets` and `other_annotations`,
/// `docs/design/render-usage.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Kinds {
    /// Whether widgets are drawn.
    pub(crate) widgets: bool,
    /// Whether every annotation that is not a widget is drawn.
    pub(crate) others: bool,
}

impl Kinds {
    /// Every kind: a page's annotations as a render with the defaults draws
    /// them, and as a display list records them.
    pub(crate) const ALL: Kinds = Kinds {
        widgets: true,
        others: true,
    };

    /// The kinds `options` draws: none where `annotations` is off, which
    /// still means no annotation of any kind.
    pub(crate) fn of(options: &crate::RenderOptions) -> Kinds {
        Kinds {
            widgets: options.annotations && options.widgets,
            others: options.annotations && options.other_annotations,
        }
    }

    /// Whether an annotation, a widget or not, is drawn.
    pub(crate) fn draws(self, widget: bool) -> bool {
        if widget {
            self.widgets
        } else {
            self.others
        }
    }
}

/// Draws every annotation of a page of the `kinds` asked for that `op`, the
/// render's operation, draws — by its flags and its `/OC` under the
/// operation's usage and states ([`prepare`]) — each placed where `placement`
/// puts it, each appearance's scopes read through its decoded streams, so an
/// appearance two annotations share and a font two appearances draw are
/// decoded once, and count into its tally when the render is counting. One
/// whose `/OC` hides it is reported as content in that layer is
/// ([`note_hidden`]).
pub(crate) fn draw(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut impl AppearanceDevice,
    op: &Operation,
    placement: Placement,
    kinds: Kinds,
) {
    each_appearance(doc, page, op, kinds, |widget, entry| {
        device.annotation(widget);
        match entry {
            Entry::Draws(appearance) => {
                draw_prepared(doc, &appearance, provider, device, op, placement);
            }
            Entry::Hidden(layer) => note_hidden(device, layer),
        }
    });
}

/// An annotation its `/OC` hides, told to `device` as the interpreter tells
/// it of content in a layer that is off: an `/OC` scope that hides, holding
/// nothing — so the renderer and the SVG writer report the layer by its label
/// as `RenderWarning::HiddenOptionalContent` (ruling 10), a recorder records
/// the two calls for every replay to report it, and nothing is drawn.
fn note_hidden(device: &mut impl Device, layer: &str) {
    device.begin_marked_content(b"OC", false, Some(layer), None);
    device.end_marked_content();
}

/// One annotation's appearance as a retained page keeps it: what it draws,
/// beside whether it is a widget, so a replay draws only the kinds its render
/// asks for (`crate::RenderOptions::widgets`, `other_annotations`).
pub(crate) struct Recorded {
    /// Whether it is a widget annotation (12.5.6.19).
    pub(crate) widget: bool,
    /// What it draws.
    pub(crate) kept: Kept,
}

/// What a retained page keeps of one annotation's appearance.
pub(crate) enum Kept {
    /// The calls its content makes, and the scope the renderer resolves its
    /// names in.
    Calls {
        scope: Arc<PageResources>,
        events: Vec<Event>,
    },
    /// An appearance `/NoZoom` or `/NoRotate` places at render time, kept
    /// prepared and interpreted at each replay under the replay's
    /// [`Placement`] (`docs/design/render-usage.md`), since where it draws
    /// is the render's magnification's to say.
    Placed(Appearance),
}

/// Every annotation of a page that `op` draws, recorded rather than drawn.
///
/// The same appearances [`draw`] draws, in the same order, through the same
/// [`prepare`] and the same interpretation — only the device differs, and it
/// is the `recorder` the page's content was recorded through, so the answers
/// it gives carry on from where the content left them exactly as a renderer's
/// do when it draws the annotations after the content. The recording is `op`,
/// the operation the page's content was recorded in, so what its usage and
/// states hide is not recorded; one its `/OC` hides is recorded as the two
/// calls that report its layer ([`note_hidden`]) and nothing else.
///
/// One that `/NoZoom` or `/NoRotate` places is kept prepared
/// ([`Recorded::Placed`]): where it draws is each render's magnification's to
/// say. It is interpreted into the recorder all the same and its calls
/// dropped, so the answers the recorder gives the annotations after it are
/// the ones a renderer gives them once it has drawn it — the answers depend on
/// the content and not on where it is placed.
pub(crate) fn record(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    provider: Option<&Arc<dyn FontProvider>>,
    recorder: &mut DisplayRecorder,
    op: &Operation,
) -> Vec<Recorded> {
    let mut out = Vec::new();
    each_appearance(doc, page, op, Kinds::ALL, |widget, entry| {
        let appearance = match entry {
            Entry::Draws(appearance) => appearance,
            Entry::Hidden(layer) => {
                note_hidden(recorder, layer);
                out.push(Recorded {
                    widget,
                    kept: Kept::Calls {
                        scope: Arc::new(PageResources::from_dict_in(
                            doc,
                            Dict::new(),
                            provider,
                            Operation::none(),
                        )),
                        events: recorder.take(),
                    },
                });
                return;
            }
        };
        if appearance.is_placed() {
            let resources = PageResources::from_dict_in(
                doc,
                appearance.resources.clone(),
                provider,
                op.clone(),
            );
            interpret(
                &appearance.content,
                appearance.transform,
                recorder,
                &resources,
            );
            drop(recorder.take());
            out.push(Recorded {
                widget,
                kept: Kept::Placed(appearance),
            });
            return;
        }
        // Two resource objects over one dictionary, as `draw_prepared` makes
        // them: the interpreter's, used up here in the recording, and the
        // renderer's, kept past it and handed to each replay bound to the
        // replay's operation.
        let resources =
            PageResources::from_dict_in(doc, appearance.resources.clone(), provider, op.clone());
        let scope = Arc::new(PageResources::from_dict_in(
            doc,
            appearance.resources.clone(),
            provider,
            Operation::none(),
        ));
        interpret(
            &appearance.content,
            appearance.transform,
            recorder,
            &resources,
        );
        out.push(Recorded {
            widget,
            kept: Kept::Calls {
                scope,
                events: recorder.take(),
            },
        });
    });
    out
}

/// One annotation of a page, as [`each_appearance`] hands it on.
enum Entry<'a> {
    /// It draws: its appearance, prepared.
    Draws(Appearance),
    /// Its `/OC` hides it under the operation's states: the layer's label,
    /// reported as content in the layer is ([`note_hidden`]).
    Hidden(&'a str),
}

/// Every annotation of a page of the `kinds` asked for that `op` draws,
/// prepared, in `/Annots` order, beside whether it is a widget, each
/// appearance decoded once `op`, the render's operation — and every one its
/// `/OC` hides, by its layer's label. An annotation of a kind not asked for is
/// passed over before anything of it is read.
fn each_appearance(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    op: &Operation,
    kinds: Kinds,
    mut each: impl FnMut(bool, Entry<'_>),
) {
    let Ok(object) = doc.get(page.reference) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };

    let list = doc.resolve_key(dict, doc.intern(b"Annots"));
    let Some(entries) = list.as_array() else {
        return;
    };

    for entry in entries {
        // Resolved individually so one broken annotation costs its own
        // appearance and not the rest of the page (ruling 2).
        let annotation = match entry {
            Object::Ref(r) => doc.get(*r).ok().and_then(|o| o.as_dict().cloned()),
            Object::Dict(d) => Some(d.clone()),
            _ => None,
        };
        let Some(annotation) = annotation else {
            continue;
        };
        let widget = is_widget(doc, &annotation);
        if !kinds.draws(widget) {
            continue;
        }
        match prepare(doc, &annotation, op) {
            Ok(appearance) => each(widget, Entry::Draws(appearance)),
            // Content in a layer that is off is reported, and so is an
            // annotation its layer hides (ruling 10).
            Err(NotDrawn::OptionalContentOff) => {
                if let Some(layer) = hidden_by(doc, &annotation, op) {
                    each(widget, Entry::Hidden(&layer));
                }
            }
            // Every other reason `prepare` gives for drawing nothing is a
            // reason a page draws nothing there and says nothing: ruling 2, a
            // page renders what it can. `Page::render_annotation` is the
            // caller that names them.
            Err(_) => {}
        }
    }
}

/// Why the flags keep `annotation` off a render naming `usage`, read from
/// `/F` and `/Subtype` alone (12.5.3 Table 167,
/// `docs/design/render-usage.md`), or `None` where they let it draw:
///
/// - `Hidden` draws in no usage;
/// - `Invisible` on a subtype ISO 32000 does not define, or none, draws in no
///   usage either: the flag "applies only to annotations which do not belong
///   to one of the standard annotation types and for which no annotation
///   handler is available", and this engine draws every annotation by its
///   appearance and has a handler for none;
/// - a print render draws only an annotation whose `Print` is set, `NoView`
///   or not — "The annotation may be printed (depending on the setting of the
///   Print flag)";
/// - every other render — a view, an export, and one naming no usage — hides
///   one whose `NoView` is set, and draws one whose `Print` is clear.
fn flag_refusal(doc: &CosDocument, annotation: &Dict, usage: Option<Usage>) -> Option<NotDrawn> {
    let flags = flags_of(doc, annotation);
    if flags.hidden() {
        return Some(NotDrawn::Hidden);
    }
    if flags.invisible()
        && !subtype(doc, annotation).is_some_and(|name| AnnotationKind::is_standard(&name))
    {
        return Some(NotDrawn::Invisible);
    }
    match usage {
        Some(Usage::Print) => (!flags.print()).then_some(NotDrawn::NotPrinted),
        Some(Usage::View | Usage::Export) | None => flags.no_view().then_some(NotDrawn::NoView),
    }
}

/// Whether `annotation` is a widget annotation (12.5.6.19), a form field's
/// appearance on the page: its `/Subtype` is `/Widget`.
fn is_widget(doc: &CosDocument, annotation: &Dict) -> bool {
    subtype(doc, annotation).is_some_and(|name| name.as_ref() == b"Widget")
}

/// The annotation's `/Subtype`, as the name's bytes.
fn subtype(doc: &CosDocument, annotation: &Dict) -> Option<Arc<[u8]>> {
    annotation
        .get_name(doc.intern(b"Subtype"))
        .and_then(|name| doc.name_bytes(name))
}

/// The label of the layer `annotation`'s `/OC` hides it by under `op`'s
/// optional-content states — "Before the annotation is drawn, its visibility
/// shall be determined based on this entry as well as the annotation flags"
/// (Table 166) — or `None` where it has no `/OC` or its `/OC` shows it. Read
/// by the code that reads a content stream's `/OC`
/// ([`crate::optional::OptionalContent::layer_of`]), its fallbacks visible: a
/// group or a membership dictionary, labelled by its `/Name` or, for want of a
/// resource name, by its reference. Bound only for an annotation that has an
/// `/OC`, so a page whose annotations carry none binds nothing for them.
fn hidden_by(doc: &CosDocument, annotation: &Dict, op: &Operation) -> Option<String> {
    let object = annotation.get(doc.intern(b"OC"))?;
    let fallback = object
        .as_objref()
        .map_or_else(|| "OC".to_string(), |r| format!("{} {} R", r.num, r.gen));
    let layer = op.optional(doc).layer_of(doc, object, &fallback);
    (!layer.visible).then_some(layer.label)
}

/// Where 12.5.3's two placement flags put an appearance in one render: the
/// magnification `/NoZoom` reads (`crate::RenderOptions::magnification`) and
/// the page's `/Rotate`, which `/NoRotate` reads — the page's frame alone,
/// never a region, so a tile draws the page's pixels (ruling 5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// The render's magnification, 1 where it was not a finite number above
    /// zero.
    magnification: f64,
    /// The page's `/Rotate`: 0, 90, 180 or 270.
    rotation: u16,
}

impl Placement {
    /// A render at `magnification` — read as 1 where it is not a finite
    /// number above zero, as a scale is — of a page whose `/Rotate` is
    /// `rotation`, normalised to a quarter turn.
    pub(crate) fn new(magnification: f64, rotation: u16) -> Placement {
        let magnification = if magnification.is_finite() && magnification > 0.0 {
            magnification
        } else {
            1.0
        };
        Placement {
            magnification,
            rotation: (rotation / 90 % 4) * 90,
        }
    }

    /// What moves an appearance fitted onto `rect`, flagged `no_zoom` and
    /// `no_rotate`, in default user space, or `None` where nothing does (12.5.3,
    /// `docs/design/render-usage.md`): about `P`, `/Rect`'s upper-left corner
    /// in default user space, `(min x, max y)` — "The upper-left corner of the
    /// annotation remains at the same point in default user space; the
    /// annotation pivots around that point" — `/NoZoom` scales by
    /// `1 / magnification`, so the appearance keeps the size it has at
    /// magnification 1, and `/NoRotate` turns counter-clockwise by the page's
    /// `/Rotate`, which the view's clockwise turn of the page undoes, so it
    /// stands as on an unturned page. A quarter turn's cosine and sine are
    /// exact, and so is the reciprocal of the magnification: nothing here is
    /// a transcendental (ruling 4).
    fn matrix(self, rect: Rect, no_zoom: bool, no_rotate: bool) -> Option<Matrix> {
        let zooms = no_zoom && self.magnification != 1.0;
        let turns = no_rotate && self.rotation != 0;
        if !zooms && !turns {
            return None;
        }
        let s = if zooms { 1.0 / self.magnification } else { 1.0 };
        let (cos, sin) = match (turns, self.rotation) {
            (true, 90) => (0.0, 1.0),
            (true, 180) => (-1.0, 0.0),
            (true, 270) => (0.0, -1.0),
            _ => (1.0, 0.0),
        };
        let (px, py) = (rect.x0, rect.y1);
        let (a, b, c, d) = (s * cos, s * sin, -s * sin, s * cos);
        // `x' = a x + c y + e` and `y' = b x + d y + f` leave `P` where it is.
        Some(Matrix {
            a,
            b,
            c,
            d,
            e: px - (a * px + c * py),
            f: py - (b * px + d * py),
        })
    }
}

/// The axis-aligned box, as `(x0, y0, x1, y1)`, of `rect`'s four corners
/// through `matrix`.
fn bounds_through(matrix: &Matrix, rect: Rect) -> (f64, f64, f64, f64) {
    let corners = [
        matrix.apply(rect.x0, rect.y0),
        matrix.apply(rect.x1, rect.y0),
        matrix.apply(rect.x1, rect.y1),
        matrix.apply(rect.x0, rect.y1),
    ];
    corners.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    )
}

/// An annotation's normal appearance, ready to draw: the content with its
/// `/BBox` clip in front, where 12.5.5's fit puts it, the resources it resolves
/// names in, the `/Rect` it was fitted onto, and whether 12.5.3's `/NoZoom`
/// and `/NoRotate` place it at render time ([`Placement`]).
///
/// Split out of `draw_one` so that the page and `Page::render_annotation`
/// draw an annotation with one piece of code: the page ignores why a
/// `prepare` failed, and a caller asking for one annotation is told.
pub(crate) struct Appearance {
    rect: Rect,
    content: Vec<u8>,
    transform: Matrix,
    resources: Dict,
    no_zoom: bool,
    no_rotate: bool,
}

impl Appearance {
    /// `/Rect` in default user space, as `(x0, y0, x1, y1)` with the corners
    /// in order: the rectangle the appearance is fitted onto and drawn within.
    pub(crate) fn rect(&self) -> (f64, f64, f64, f64) {
        (self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y1)
    }

    /// The rectangle the appearance is drawn within under `placement`:
    /// `/Rect` as [`Placement`] moves it, its axis-aligned box in default user
    /// space, which is `/Rect` itself where nothing moves it.
    pub(crate) fn placed_rect(&self, placement: Placement) -> (f64, f64, f64, f64) {
        match placement.matrix(self.rect, self.no_zoom, self.no_rotate) {
            Some(moved) => bounds_through(&moved, self.rect),
            None => self.rect(),
        }
    }

    /// Whether `/NoZoom` or `/NoRotate` places it at render time, so that a
    /// retained page keeps it prepared and interprets it at each replay
    /// rather than recording its calls once.
    pub(crate) fn is_placed(&self) -> bool {
        self.no_zoom || self.no_rotate
    }

    /// The transform the appearance is drawn through under `placement`:
    /// 12.5.5's fit, and then whatever [`Placement`] moves it by.
    fn transform_at(&self, placement: Placement) -> Matrix {
        match placement.matrix(self.rect, self.no_zoom, self.no_rotate) {
            Some(moved) => self.transform.then(&moved),
            None => self.transform,
        }
    }
}

/// Everything `draw_one` decided before drawing, with each reason it had for
/// drawing nothing named: one decision for a direct render, a display list's
/// recording and `Page::render_annotation`. The flags are read for `op`'s
/// usage ([`flag_refusal`]) and an `/OC` under `op`'s states
/// ([`hidden_by`]) before anything is decoded; the appearance is decoded once
/// `op`, the render's operation, and counted into its tally when the render
/// is counting.
pub(crate) fn prepare(
    doc: &Arc<CosDocument>,
    annotation: &Dict,
    op: &Operation,
) -> Result<Appearance, NotDrawn> {
    if let Some(why) = flag_refusal(doc, annotation, op.usage()) {
        return Err(why);
    }

    // A pop-up is the window that opens when its parent is clicked, not
    // something drawn on the page.
    if subtype(doc, annotation).is_some_and(|name| name.as_ref() == b"Popup") {
        return Err(NotDrawn::Popup);
    }

    // 8.11.3.3: "it shall be visible for screen or print only if the flags
    // have the appropriate settings and the group or membership dictionary
    // indicates it shall be visible".
    if hidden_by(doc, annotation, op).is_some() {
        return Err(NotDrawn::OptionalContentOff);
    }

    let rect = doc
        .resolve_key(annotation, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
        .filter(|rect| !rect.is_empty())
        .ok_or(NotDrawn::NoRect)?;

    let form = normal_appearance(doc, annotation).ok_or(NotDrawn::NoAppearance)?;
    let content = op
        .decoded(doc, form)
        .map_err(|_| NotDrawn::UnreadableAppearance)?;
    let object = doc.get(form).map_err(|_| NotDrawn::UnreadableAppearance)?;
    let form_dict = object.as_dict().ok_or(NotDrawn::UnreadableAppearance)?;

    let matrix = matrix_of(doc, form_dict);
    let bbox = doc
        .resolve_key(form_dict, doc.intern(b"BBox"))
        .as_array()
        .and_then(Rect::from_array);

    let transform = fit(bbox, matrix, rect).ok_or(NotDrawn::Degenerate)?;

    // 8.10.2: the bounding box is expressed in form space and clips whatever
    // the form draws. An appearance stream that paints outside its box —
    // which plenty do, because the box is often the *intended* extent rather
    // than the drawn one — would otherwise spill across the page, over
    // content that has nothing to do with the annotation.
    //
    // Written as a content prefix rather than a second clip implementation:
    // `transform` already carries the form's own `/Matrix`, so a rectangle in
    // form space lands exactly where the spec puts it, and `W n` here behaves
    // the way `W n` behaves everywhere else by construction.
    let content = match bbox {
        Some(bbox) if !bbox.is_empty() && bbox.is_finite() => {
            let mut clipped = format!(
                "q {} {} {} {} re W n\n",
                bbox.x0,
                bbox.y0,
                bbox.x1 - bbox.x0,
                bbox.y1 - bbox.y0
            )
            .into_bytes();
            clipped.extend_from_slice(&content);
            clipped.extend_from_slice(b"\nQ\n");
            clipped
        }
        _ => Arc::unwrap_or_clone(content),
    };

    let resources = doc
        .resolve_key(form_dict, tinker_pdf_cos::Name::RESOURCES)
        .as_dict()
        .cloned()
        .unwrap_or_default();

    let flags = flags_of(doc, annotation);
    Ok(Appearance {
        rect,
        content,
        transform,
        resources,
        no_zoom: flags.no_zoom(),
        no_rotate: flags.no_rotate(),
    })
}

/// `annotation`'s `/F`, every bit clear where it has none.
fn flags_of(doc: &CosDocument, annotation: &Dict) -> AnnotationFlags {
    AnnotationFlags::from_raw(annotation.get_int(doc.intern(b"F")).unwrap_or(0))
}

/// Where an annotation draws on its page, as [`draw`] would draw it, read
/// from its dictionary and its appearance's without decoding anything
/// (`docs/design/performance.md`, P-17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drawn {
    /// Nowhere: kept off the render's usage by its `/F` (12.5.3,
    /// [`flag_refusal`]), a pop-up, no `/Rect` with an area, no normal
    /// appearance, an appearance that is not a form, or one 12.5.5's fit
    /// cannot place — each a reason [`prepare`] gives for drawing nothing.
    Nothing,
    /// Inside its `/Rect`, in default user space: an appearance with a
    /// `/BBox` is fitted onto the rectangle, and the box clips what it draws
    /// (8.10.2), so nothing it paints lands outside.
    Within(Rect),
    /// Wherever its content draws: an appearance with no `/BBox` is drawn
    /// where its own `/Matrix` puts it, unclipped.
    Unbounded,
}

/// [`Drawn`] for `annotation` in a render naming `usage` and placed as
/// `placement` says: the decisions [`prepare`] takes, but for the decode,
/// which a bounded appearance does not need — one whose content does not
/// decode draws nothing, inside its rectangle as well — and for an `/OC`,
/// which is not read: under states that hide the annotation, its rectangle is
/// drawn again with nothing new in it, which costs pixels and changes none.
/// The rectangle is `/Rect` where `/NoZoom` and `/NoRotate` move it
/// ([`Appearance::placed_rect`]).
pub(crate) fn drawn(
    doc: &CosDocument,
    annotation: &Dict,
    usage: Option<Usage>,
    placement: Placement,
) -> Drawn {
    if flag_refusal(doc, annotation, usage).is_some() {
        return Drawn::Nothing;
    }
    if subtype(doc, annotation).is_some_and(|name| name.as_ref() == b"Popup") {
        return Drawn::Nothing;
    }
    let Some(rect) = doc
        .resolve_key(annotation, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
        .filter(|rect| !rect.is_empty())
    else {
        return Drawn::Nothing;
    };
    let Some(form) = normal_appearance(doc, annotation) else {
        return Drawn::Nothing;
    };
    let Ok(object) = doc.get(form) else {
        return Drawn::Nothing;
    };
    let Some(form_dict) = object.as_dict() else {
        return Drawn::Nothing;
    };
    let matrix = matrix_of(doc, form_dict);
    let bbox = doc
        .resolve_key(form_dict, doc.intern(b"BBox"))
        .as_array()
        .and_then(Rect::from_array);
    match bbox {
        None => Drawn::Unbounded,
        // `fit` places a box it can scale onto the rectangle, and then the box
        // is non-empty and finite, which is when `prepare` clips to it.
        Some(bbox) => match fit(Some(bbox), matrix, rect) {
            Some(_) => {
                let flags = flags_of(doc, annotation);
                match placement.matrix(rect, flags.no_zoom(), flags.no_rotate()) {
                    Some(moved) => {
                        let (x0, y0, x1, y1) = bounds_through(&moved, rect);
                        Drawn::Within(Rect { x0, y0, x1, y1 })
                    }
                    None => Drawn::Within(rect),
                }
            }
            None => Drawn::Nothing,
        },
    }
}

/// Draws a prepared appearance through `device`, placed where `placement`
/// puts it, its two scopes serving `op`, the render's operation: one decode
/// between them of each stream they read, counted into its tally when the
/// render is counting.
pub(crate) fn draw_prepared(
    doc: &Arc<CosDocument>,
    appearance: &Appearance,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut impl AppearanceDevice,
    op: &Operation,
    placement: Placement,
) {
    let resources =
        PageResources::from_dict_in(doc, appearance.resources.clone(), provider, op.clone());

    // 12.5.5: the appearance is a form XObject reached by reference, so there
    // is no name for the interpreter to announce and the caller announces it
    // instead. Both seams have to change: the device resolves images,
    // shadings and patterns, and an appearance that names one the page does
    // not define is the ordinary case, not the odd one.
    device.push_scope(Arc::new(PageResources::from_dict_in(
        doc,
        appearance.resources.clone(),
        provider,
        op.clone(),
    )));
    interpret(
        &appearance.content,
        appearance.transform_at(placement),
        device,
        &resources,
    );
    device.pop_scope();
}

/// The `/AP` `/N` stream, following `/AS` when the appearance has states.
fn normal_appearance(doc: &CosDocument, annotation: &Dict) -> Option<tinker_pdf_cos::ObjRef> {
    let ap = doc.resolve_key(annotation, doc.intern(b"AP"));
    let ap = ap.as_dict()?;
    let normal = ap.get(doc.intern(b"N"))?;

    match normal {
        Object::Ref(r) => {
            // Either the stream itself or a dictionary of states, which is how
            // a checkbox carries its on and off appearances.
            //
            // The two are told apart by *being a stream*, not by carrying a
            // `/BBox`. 8.10.2 makes the box required, so using it as the test
            // works on well-formed files — and on a file that omits it, the
            // appearance was silently treated as a dictionary of states,
            // found none, and drew nothing at all. Which is worse than
            // drawing it unclipped, and invisible either way.
            let object = doc.get(*r).ok()?;
            if object.as_stream().is_some() {
                return Some(*r);
            }
            state_of(doc, annotation, object.as_dict()?)
        }
        Object::Dict(states) => state_of(doc, annotation, states),
        _ => None,
    }
}

/// Picks the substate named by `/AS`, or the only one when there is no choice.
fn state_of(doc: &CosDocument, annotation: &Dict, states: &Dict) -> Option<tinker_pdf_cos::ObjRef> {
    if let Some(name) = annotation.get_name(doc.intern(b"AS")) {
        if let Some(chosen) = states.get_ref(name) {
            return Some(chosen);
        }
        // 12.5.5: an /AS naming a state that is not there means nothing is
        // drawn. Falling back to an arbitrary one would show a checkbox
        // ticked when the file says it is not.
        return None;
    }
    let mut found = states.entries().iter().filter_map(|(_, v)| v.as_objref());
    let only = found.next()?;
    found.next().is_none().then_some(only)
}

fn matrix_of(doc: &CosDocument, form: &Dict) -> Matrix {
    let values: Vec<f64> = doc
        .resolve_key(form, doc.intern(b"Matrix"))
        .as_array()
        .map(|a| a.iter().filter_map(Object::as_number).collect())
        .unwrap_or_default();

    match values.as_slice() {
        [a, b, c, d, e, f] if values.iter().all(|v| v.is_finite()) => Matrix {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            e: *e,
            f: *f,
        },
        _ => Matrix::IDENTITY,
    }
}

/// 12.5.5's algorithm: transform the bounding box, fit the result to the
/// rectangle, and compose.
///
/// The form's own `/Matrix` is applied first, its four corners are mapped, and
/// the *axis-aligned box of the result* — not the transformed box, which may
/// be rotated — is what gets scaled onto `/Rect`.
fn fit(bbox: Option<Rect>, matrix: Matrix, rect: Rect) -> Option<Matrix> {
    if rect.is_empty() {
        return None;
    }
    // A form without a bounding box has nothing to fit, so it is drawn where
    // its own matrix puts it. Refusing would lose annotations that render
    // fine everywhere else.
    let Some(bbox) = bbox else {
        return Some(matrix);
    };
    if bbox.is_empty() {
        return None;
    }

    let corners = [
        matrix.apply(bbox.x0, bbox.y0),
        matrix.apply(bbox.x1, bbox.y0),
        matrix.apply(bbox.x1, bbox.y1),
        matrix.apply(bbox.x0, bbox.y1),
    ];
    if corners
        .iter()
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return None;
    }

    let (mut x0, mut y0) = corners[0];
    let (mut x1, mut y1) = corners[0];
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }

    // A box with no extent in one direction cannot be scaled to fill one, and
    // dividing by it would put an infinity into the transform.
    let (dx, dy) = (x1 - x0, y1 - y0);
    if dx <= f64::EPSILON || dy <= f64::EPSILON {
        return None;
    }

    let sx = (rect.x1 - rect.x0) / dx;
    let sy = (rect.y1 - rect.y0) / dy;
    let a = Matrix {
        a: sx,
        b: 0.0,
        c: 0.0,
        d: sy,
        e: rect.x0 - x0 * sx,
        f: rect.y0 - y0 * sy,
    };

    Some(matrix.then(&a))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12.5.3 Table 167's flags, as `/F` bits.
    const INVISIBLE: i64 = 1;
    const HIDDEN: i64 = 1 << 1;
    const PRINT: i64 = 1 << 2;
    const NO_VIEW: i64 = 1 << 5;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    /// **The flags, by usage** (12.5.3 as `docs/design/render-usage.md` reads
    /// it): every combination of the four flags that decide, on a known
    /// subtype, an unknown one and none, under no usage and each of the
    /// three — each refusal the reason it names, and `drawn` agreeing.
    #[test]
    fn the_flags_decide_by_usage() {
        let bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
        let usages = [
            None,
            Some(Usage::View),
            Some(Usage::Print),
            Some(Usage::Export),
        ];
        for subtype in [Some(b"Square".as_slice()), Some(b"Gizmo"), None] {
            for flags in 0..16i64 {
                // Bits 1, 2, 3 and 6 from the four bits of `flags`.
                let f = (flags & 1) * INVISIBLE
                    | ((flags >> 1) & 1) * HIDDEN
                    | ((flags >> 2) & 1) * PRINT
                    | ((flags >> 3) & 1) * NO_VIEW;
                let mut dict = Dict::new();
                if let Some(name) = subtype {
                    dict.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(name)));
                }
                dict.insert(doc.intern(b"F"), Object::Int(f));
                for usage in usages {
                    let unknown = subtype != Some(b"Square".as_slice());
                    let want = if f & HIDDEN != 0 {
                        Some(NotDrawn::Hidden)
                    } else if f & INVISIBLE != 0 && unknown {
                        Some(NotDrawn::Invisible)
                    } else if usage == Some(Usage::Print) {
                        (f & PRINT == 0).then_some(NotDrawn::NotPrinted)
                    } else {
                        (f & NO_VIEW != 0).then_some(NotDrawn::NoView)
                    };
                    let what = format!("{subtype:?}, /F {f}, {usage:?}");
                    assert_eq!(flag_refusal(&doc, &dict, usage), want, "{what}");
                    if want.is_some() {
                        assert_eq!(
                            drawn(&doc, &dict, usage, Placement::new(1.0, 0)),
                            Drawn::Nothing,
                            "{what}"
                        );
                    }
                }
            }
        }
        // No `/F` is every flag clear: drawn on the screen, not printed.
        let mut plain = Dict::new();
        plain.insert(doc.intern(b"Subtype"), Object::Name(doc.intern(b"Square")));
        assert_eq!(flag_refusal(&doc, &plain, None), None);
        assert_eq!(
            flag_refusal(&doc, &plain, Some(Usage::Print)),
            Some(NotDrawn::NotPrinted)
        );
        // Every subtype ISO 32000 defines is known to the Invisible rule.
        for known in [
            "Text",
            "Link",
            "FreeText",
            "Line",
            "Square",
            "Circle",
            "Polygon",
            "PolyLine",
            "Highlight",
            "Underline",
            "Squiggly",
            "StrikeOut",
            "Caret",
            "Stamp",
            "Ink",
            "Popup",
            "FileAttachment",
            "Sound",
            "Movie",
            "Screen",
            "Widget",
            "PrinterMark",
            "TrapNet",
            "Watermark",
            "3D",
            "Redact",
            "Projection",
            "RichMedia",
        ] {
            assert!(AnnotationKind::is_standard(known.as_bytes()), "{known}");
        }
        for unknown in ["widget", "", "Gizmo", "3d"] {
            assert!(
                !AnnotationKind::is_standard(unknown.as_bytes()),
                "{unknown}"
            );
        }
    }

    /// **An annotation's `/OC`** is read under the operation's states: a
    /// group off hides it and is named by its `/Name`, or by its reference
    /// where it has none; a group on, a membership dictionary that shows,
    /// something that is no group and a group the configuration never listed
    /// all show it; and an annotation with no `/OC` binds no states at all.
    #[test]
    fn an_annotation_s_optional_content_is_read_under_the_render_s_states() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [10 0 R 11 0 R 12 0 R] \
/D << /OFF [10 0 R 12 0 R] >> >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
10 0 obj\n<< /Type /OCG /Name (Notes) >>\nendobj\n\
11 0 obj\n<< /Type /OCG /Name (Shown) >>\nendobj\n\
12 0 obj\n<< /Type /OCG >>\nendobj\n\
13 0 obj\n<< /Type /OCMD /OCGs [10 0 R 11 0 R] /P /AnyOn >>\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";
        let doc = CosDocument::open(bytes.to_vec()).expect("it opens");
        let with_oc = |oc: Option<Object>| {
            let mut dict = Dict::new();
            if let Some(oc) = oc {
                dict.insert(doc.intern(b"OC"), oc);
            }
            dict
        };
        let reference = |num| Object::Ref(tinker_pdf_cos::ObjRef::new(num, 0));
        for (oc, want) in [
            (Some(reference(10)), Some("Notes".to_string())),
            (Some(reference(12)), Some("12 0 R".to_string())),
            (Some(reference(11)), None),
            (Some(reference(13)), None),
            (Some(reference(99)), None),
            (Some(Object::Int(4)), None),
            (None, None),
        ] {
            let op = Operation::none();
            assert_eq!(hidden_by(&doc, &with_oc(oc.clone()), &op), want, "{oc:?}");
        }
        crate::optional::binds::take();
        let _ = hidden_by(&doc, &with_oc(None), &Operation::none());
        assert_eq!(crate::optional::binds::take(), 0, "no /OC, no states bound");
    }

    /// **The placement keeps `/Rect`'s upper-left corner fixed** (12.5.3):
    /// `/NoZoom` scales about it by the magnification's reciprocal and
    /// `/NoRotate` turns about it counter-clockwise by the page's `/Rotate`,
    /// worked by hand; where the flags move nothing — no flag, magnification
    /// 1, an unturned page — nothing is placed; a magnification that is not a
    /// finite number above zero is read as 1, and a turn is a quarter turn.
    #[test]
    fn the_placement_keeps_the_upper_left_corner_fixed() {
        let r = rect(10.0, 20.0, 50.0, 40.0);
        for rotation in [0u16, 90, 180, 270] {
            // Powers of two, whose reciprocals are exact, so `P` is exactly
            // itself after the round trip through the matrix.
            for magnification in [0.5, 1.0, 2.0, 4.0] {
                let placement = Placement::new(magnification, rotation);
                for (zoom, turn) in [(true, false), (false, true), (true, true)] {
                    let moves = (zoom && magnification != 1.0) || (turn && rotation != 0);
                    let matrix = placement.matrix(r, zoom, turn);
                    assert_eq!(matrix.is_some(), moves, "{rotation}, {magnification}");
                    if let Some(matrix) = matrix {
                        assert_eq!(matrix.apply(10.0, 40.0), (10.0, 40.0), "P stays");
                    }
                }
                assert_eq!(placement.matrix(r, false, false), None, "no flag");
            }
        }
        // /NoZoom at 2: the far corner (50, 20) comes half the way to P.
        let zoom = Placement::new(2.0, 0)
            .matrix(r, true, false)
            .expect("it moves");
        assert_eq!(zoom.apply(50.0, 20.0), (30.0, 30.0));
        // /NoRotate on a page turned 90: (50, 40), 40 right of P, goes 40 up.
        let turn = Placement::new(1.0, 90)
            .matrix(r, false, true)
            .expect("it moves");
        assert_eq!(turn.apply(50.0, 40.0), (10.0, 80.0));
        assert_eq!(bounds_through(&turn, r), (10.0, 40.0, 30.0, 80.0));
        let half_turn = Placement::new(1.0, 180)
            .matrix(r, false, true)
            .expect("it moves");
        assert_eq!(half_turn.apply(50.0, 20.0), (-30.0, 60.0));
        // Both at once: turned and halved about P.
        let both = Placement::new(2.0, 270)
            .matrix(r, true, true)
            .expect("it moves");
        assert_eq!(both.apply(50.0, 40.0), (10.0, 20.0));
        for unreadable in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                Placement::new(unreadable, 0).matrix(r, true, false),
                None,
                "{unreadable} reads as 1"
            );
        }
        assert_eq!(Placement::new(1.0, 450), Placement::new(1.0, 90));
    }

    /// The ordinary case: a form whose box already matches its rectangle is
    /// drawn untouched. This is the shape the synthesizer writes, and it must
    /// not acquire a transform on the way through.
    #[test]
    fn a_matching_box_needs_no_transform() {
        let box_ = rect(10.0, 20.0, 110.0, 60.0);
        let fitted = fit(Some(box_), Matrix::IDENTITY, box_).expect("it fits");
        assert!((fitted.a - 1.0).abs() < 1e-9);
        assert!((fitted.d - 1.0).abs() < 1e-9);
        assert!(fitted.e.abs() < 1e-9 && fitted.f.abs() < 1e-9);
    }

    #[test]
    fn a_form_is_scaled_and_moved_onto_its_rectangle() {
        // A unit box drawn into a 100x40 rectangle at (10, 20).
        let fitted = fit(
            Some(rect(0.0, 0.0, 1.0, 1.0)),
            Matrix::IDENTITY,
            rect(10.0, 20.0, 110.0, 60.0),
        )
        .expect("it fits");

        assert_eq!(fitted.apply(0.0, 0.0), (10.0, 20.0));
        assert_eq!(fitted.apply(1.0, 1.0), (110.0, 60.0));
    }

    /// The transformed box is taken axis-aligned, so a rotated form still
    /// fills its rectangle rather than poking outside it.
    #[test]
    fn a_rotated_form_is_fitted_by_its_upright_bounds() {
        // A quarter turn: (x, y) becomes (-y, x).
        let quarter = Matrix {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        };
        let fitted = fit(
            Some(rect(0.0, 0.0, 2.0, 1.0)),
            quarter,
            rect(0.0, 0.0, 10.0, 10.0),
        )
        .expect("it fits");

        // Every corner of the original box lands inside the rectangle.
        for (x, y) in [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)] {
            let (px, py) = fitted.apply(x, y);
            assert!(
                (-1e-9..=10.0 + 1e-9).contains(&px) && (-1e-9..=10.0 + 1e-9).contains(&py),
                "({x}, {y}) mapped to ({px}, {py}), outside the rectangle"
            );
        }
    }

    #[test]
    fn a_degenerate_box_or_rectangle_draws_nothing() {
        assert!(fit(
            Some(rect(0.0, 0.0, 1.0, 1.0)),
            Matrix::IDENTITY,
            rect(5.0, 5.0, 5.0, 5.0)
        )
        .is_none());
        assert!(fit(
            Some(rect(5.0, 5.0, 5.0, 5.0)),
            Matrix::IDENTITY,
            rect(0.0, 0.0, 1.0, 1.0)
        )
        .is_none());
    }

    /// A matrix that collapses the box cannot be inverted into a fit, and
    /// must not produce a transform full of infinities.
    #[test]
    fn a_collapsing_matrix_draws_nothing() {
        let flat = Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        };
        assert!(fit(
            Some(rect(0.0, 0.0, 1.0, 1.0)),
            flat,
            rect(0.0, 0.0, 10.0, 10.0)
        )
        .is_none());
    }

    #[test]
    fn a_form_without_a_box_keeps_its_own_matrix() {
        let matrix = Matrix::translate(3.0, 4.0);
        let fitted = fit(None, matrix, rect(0.0, 0.0, 10.0, 10.0)).expect("it draws");
        assert_eq!(fitted.apply(0.0, 0.0), (3.0, 4.0));
    }

    /// **Where an annotation draws, read without drawing it** (`drawn`): its
    /// `/Rect` for an appearance with a `/BBox`, which clips it there; no
    /// bound for one without; and nothing for every reason `prepare` gives
    /// for drawing nothing — hidden, `NoView`, a pop-up, no rectangle, an
    /// empty one, no appearance, an `/AS` naming no state, and a box the fit
    /// cannot place.
    #[test]
    fn an_annotation_draws_within_its_rectangle_or_says_why_not() {
        use tinker_pdf_cos::{Name, ObjRef};

        let body = |num: u32, dict: &str| {
            format!("{num} 0 obj\n{dict}\nstream\n0 0 1 rg 0 0 10 10 re f\nendstream\nendobj\n")
        };
        // Each form draws `0 0 1 rg 0 0 10 10 re f`, twenty-three bytes.
        let bytes = format!(
            "%PDF-1.7\n\
             1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
             2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
             {}{}{}\
             trailer\n<< /Root 1 0 R >>\n%%EOF\n",
            body(
                5,
                "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 23 >>"
            ),
            body(6, "<< /Type /XObject /Subtype /Form /Length 23 >>"),
            body(
                7,
                "<< /Type /XObject /Subtype /Form /BBox [3 3 3 3] /Length 23 >>"
            ),
        );
        let doc = Arc::new(CosDocument::open(bytes.into_bytes()).expect("it opens"));
        let annotation = |form: Option<u32>, edit: &dyn Fn(&mut Dict)| {
            let mut dict = Dict::new();
            dict.insert(Name::SUBTYPE, Object::Name(doc.intern(b"Square")));
            dict.insert(
                doc.intern(b"Rect"),
                Object::Array(
                    [20.0, 30.0, 60.0, 50.0]
                        .into_iter()
                        .map(Object::Real)
                        .collect(),
                ),
            );
            if let Some(form) = form {
                let mut ap = Dict::new();
                ap.insert(Name::N, Object::Ref(ObjRef::new(form, 0)));
                dict.insert(doc.intern(b"AP"), Object::Dict(ap));
            }
            edit(&mut dict);
            dict
        };
        let within = Drawn::Within(rect(20.0, 30.0, 60.0, 50.0));
        assert_eq!(
            drawn(
                &doc,
                &annotation(Some(5), &|_| {}),
                None,
                Placement::new(1.0, 0)
            ),
            within
        );
        assert_eq!(
            drawn(
                &doc,
                &annotation(Some(6), &|_| {}),
                None,
                Placement::new(1.0, 0)
            ),
            Drawn::Unbounded,
            "no /BBox, so nothing clips it"
        );
        assert_eq!(
            drawn(
                &doc,
                &annotation(Some(7), &|_| {}),
                None,
                Placement::new(1.0, 0)
            ),
            Drawn::Nothing,
            "a box with no area is never placed"
        );
        assert_eq!(
            drawn(
                &doc,
                &annotation(None, &|_| {}),
                None,
                Placement::new(1.0, 0)
            ),
            Drawn::Nothing
        );
        for (what, flags) in [("hidden", HIDDEN), ("NoView", NO_VIEW)] {
            let hidden = annotation(Some(5), &|dict| {
                dict.insert(doc.intern(b"F"), Object::Int(flags | 4));
            });
            assert_eq!(
                drawn(&doc, &hidden, None, Placement::new(1.0, 0)),
                Drawn::Nothing,
                "{what}"
            );
        }
        // In print, NoView with Print set draws; Print clear does not.
        let printed = annotation(Some(5), &|dict| {
            dict.insert(doc.intern(b"F"), Object::Int(NO_VIEW | PRINT));
        });
        assert_eq!(
            drawn(&doc, &printed, Some(Usage::Print), Placement::new(1.0, 0)),
            within
        );
        assert_eq!(
            drawn(
                &doc,
                &annotation(Some(5), &|_| {}),
                Some(Usage::Print),
                Placement::new(1.0, 0)
            ),
            Drawn::Nothing,
            "no Print flag"
        );
        let popup = annotation(Some(5), &|dict| {
            dict.insert(Name::SUBTYPE, Object::Name(doc.intern(b"Popup")));
        });
        assert_eq!(
            drawn(&doc, &popup, None, Placement::new(1.0, 0)),
            Drawn::Nothing,
            "a pop-up"
        );
        let flat = annotation(Some(5), &|dict| {
            dict.insert(doc.intern(b"Rect"), Object::Array(vec![Object::Int(5); 4]));
        });
        assert_eq!(
            drawn(&doc, &flat, None, Placement::new(1.0, 0)),
            Drawn::Nothing,
            "a rectangle of no area"
        );
        let unplaced = annotation(Some(5), &|dict| {
            dict.insert(doc.intern(b"Rect"), Object::Null);
        });
        assert_eq!(
            drawn(&doc, &unplaced, None, Placement::new(1.0, 0)),
            Drawn::Nothing,
            "no rectangle"
        );
        let stateless = annotation(None, &|dict| {
            let mut states = Dict::new();
            states.insert(doc.intern(b"On"), Object::Ref(ObjRef::new(5, 0)));
            let mut ap = Dict::new();
            ap.insert(Name::N, Object::Dict(states));
            dict.insert(doc.intern(b"AP"), Object::Dict(ap));
            dict.insert(doc.intern(b"AS"), Object::Name(doc.intern(b"Off")));
        });
        assert_eq!(
            drawn(&doc, &stateless, None, Placement::new(1.0, 0)),
            Drawn::Nothing,
            "/AS names no state"
        );

        // The rectangle is the one `prepare` fits the appearance onto, and
        // each annotation drawn nowhere is one `prepare` declines.
        let prepared = prepare(&doc, &annotation(Some(5), &|_| {}), &Operation::none())
            .expect("a bounded appearance is prepared");
        assert_eq!(prepared.rect(), (20.0, 30.0, 60.0, 50.0));
        for nowhere in [
            annotation(Some(7), &|_| {}),
            annotation(None, &|_| {}),
            popup,
            flat,
            unplaced,
            stateless,
        ] {
            assert!(prepare(&doc, &nowhere, &Operation::none()).is_err());
        }
    }

    /// The whole chain in one test: an annotation added through the editor
    /// gets a synthesized appearance, the appearance survives being written
    /// and reopened, and the renderer puts ink on the page where the
    /// annotation is and nowhere else.
    ///
    /// Each half of this passed on its own while the pair was broken, which is
    /// why it is tested end to end rather than by its parts.
    #[test]
    fn an_annotation_added_by_the_editor_renders() {
        use tinker_pdf_cos::{annot, DocumentBuilder, DocumentEditor, WriteMode, WriteOptions};

        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |_| {});
        let blank = Arc::new(
            CosDocument::open({
                let bytes = builder.finish();
                crate::subset::tests_support::written_support::authored(
                    "an annotation added by the editor renders",
                    &bytes,
                );
                bytes
            })
            .expect("the blank document opens"),
        );

        let mut editor = DocumentEditor::new(Arc::clone(&blank));
        editor
            .add_annotation(
                0,
                annot::square(
                    &blank,
                    rect(20.0, 20.0, 80.0, 80.0),
                    annot::Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                    4.0,
                ),
            )
            .expect("the annotation is added");

        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        crate::subset::tests_support::written_support::rewritten(
            "an annotation added by the editor renders",
            editor.document().bytes(),
            &bytes,
        );
        let doc = crate::Document::open(bytes).expect("it reopens");
        let page = doc.page(0).expect("the page is there");

        let with = page.render(&crate::RenderOptions::default());
        let without = page.render(&crate::RenderOptions {
            annotations: false,
            ..crate::RenderOptions::default()
        });

        // Anything that is not the white background is the annotation: the
        // page itself is empty.
        let ink = |bitmap: &crate::Bitmap| -> usize {
            bitmap
                .data
                .chunks_exact(3)
                .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
                .count()
        };

        assert_eq!(ink(&without), 0, "the page on its own is blank");
        assert!(
            ink(&with) > 0,
            "the annotation draws something once it is turned on"
        );

        // And it lands inside its rectangle. The page is 100 units tall and
        // rendered at one pixel per unit, so y flips: the annotation occupies
        // rows and columns 20 to 80.
        let stride = with.stride;
        for y in 0..with.height as usize {
            for x in 0..with.width as usize {
                let at = y * stride + x * 3;
                let Some(pixel) = with.data.get(at..at + 3) else {
                    continue;
                };
                if pixel[0] < 250 || pixel[1] < 250 || pixel[2] < 250 {
                    assert!(
                        (18..=82).contains(&x) && (18..=82).contains(&y),
                        "ink at ({x}, {y}) is outside the annotation's rectangle"
                    );
                }
            }
        }
    }

    /// A hidden annotation is one the file asks not to be shown.
    #[test]
    fn a_hidden_annotation_draws_nothing() {
        use tinker_pdf_cos::{
            annot, DocumentBuilder, DocumentEditor, Object as CosObject, WriteMode, WriteOptions,
        };

        let mut builder = DocumentBuilder::new();
        builder.add_page(100.0, 100.0, |_| {});
        let blank = Arc::new(
            CosDocument::open({
                let bytes = builder.finish();
                crate::subset::tests_support::written_support::authored(
                    "a hidden annotation draws nothing",
                    &bytes,
                );
                bytes
            })
            .expect("the document opens"),
        );

        let mut square = annot::square(
            &blank,
            rect(20.0, 20.0, 80.0, 80.0),
            annot::Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            4.0,
        );
        // /F bit 2 is Hidden, and it is set alongside the print flag the
        // builder already wrote.
        square.insert(blank.intern(b"F"), CosObject::Int(2 | 4));

        let mut editor = DocumentEditor::new(Arc::clone(&blank));
        editor.add_annotation(0, square).expect("it is added");
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Rewrite,
            ..WriteOptions::default()
        });
        crate::subset::tests_support::written_support::rewritten(
            "a hidden annotation draws nothing",
            editor.document().bytes(),
            &bytes,
        );

        let doc = crate::Document::open(bytes).expect("it reopens");
        let bitmap = doc
            .page(0)
            .expect("the page")
            .render(&crate::RenderOptions::default());
        let ink = bitmap
            .data
            .chunks_exact(3)
            .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
            .count();
        assert_eq!(ink, 0, "a hidden annotation stays hidden");
    }
}
