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

use crate::fonts::FontProvider;
use crate::render_part::NotDrawn;
use crate::resources::{Operation, PageResources};

/// 12.5.3: the flags that keep an annotation off the page.
const HIDDEN: i64 = 1 << 1;
const NO_VIEW: i64 = 1 << 5;

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
}

impl AppearanceDevice for Renderer<'_, PageResources> {
    fn push_scope(&mut self, scope: Arc<PageResources>) {
        self.push_resources(scope);
    }

    fn pop_scope(&mut self) {
        self.pop_resources();
    }
}

/// Draws every visible annotation of a page in `op`, the render's operation:
/// each appearance's scopes read through its decoded streams, so an
/// appearance two annotations share and a font two appearances draw are
/// decoded once, and count into its tally when the render is counting.
pub(crate) fn draw(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut impl AppearanceDevice,
    op: &Operation,
) {
    each_appearance(doc, page, op, |appearance| {
        draw_prepared(doc, appearance, provider, device, op);
    });
}

/// One annotation's appearance as a retained page keeps it: the scope the
/// renderer resolves its names in, and the calls its content makes.
pub(crate) struct Recorded {
    pub(crate) scope: Arc<PageResources>,
    pub(crate) events: Vec<Event>,
}

/// Every visible annotation of a page, recorded rather than drawn.
///
/// The same appearances [`draw`] draws, in the same order, through the same
/// [`prepare`] and the same interpretation — only the device differs, and it
/// is the `recorder` the page's content was recorded through, so the answers
/// it gives carry on from where the content left them exactly as a renderer's
/// do when it draws the annotations after the content. The recording is `op`,
/// the operation the page's content was recorded in.
pub(crate) fn record(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    provider: Option<&Arc<dyn FontProvider>>,
    recorder: &mut DisplayRecorder,
    op: &Operation,
) -> Vec<Recorded> {
    let mut out = Vec::new();
    each_appearance(doc, page, op, |appearance| {
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
            scope,
            events: recorder.take(),
        });
    });
    out
}

/// Every annotation of a page that draws, prepared, in `/Annots` order, each
/// appearance decoded once `op`, the render's operation.
fn each_appearance(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    op: &Operation,
    mut each: impl FnMut(&Appearance),
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
        // Every reason `prepare` gives for drawing nothing is a reason a page
        // draws nothing there and says nothing: ruling 2, a page renders what
        // it can. `Page::render_annotation` is the caller that names them.
        if let Some(Ok(appearance)) = annotation.map(|a| prepare(doc, &a, op)) {
            each(&appearance);
        }
    }
}

/// An annotation's normal appearance, ready to draw: the content with its
/// `/BBox` clip in front, where 12.5.5's fit puts it, the resources it resolves
/// names in, and the `/Rect` it was fitted onto.
///
/// Split out of `draw_one` so that the page and `Page::render_annotation`
/// draw an annotation with one piece of code: the page ignores why a
/// `prepare` failed, and a caller asking for one annotation is told.
pub(crate) struct Appearance {
    rect: Rect,
    content: Vec<u8>,
    transform: Matrix,
    resources: Dict,
}

impl Appearance {
    /// `/Rect` in default user space, as `(x0, y0, x1, y1)` with the corners
    /// in order: the rectangle the appearance is fitted onto and drawn within.
    pub(crate) fn rect(&self) -> (f64, f64, f64, f64) {
        (self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y1)
    }
}

/// Everything `draw_one` decided before drawing, with each reason it had for
/// drawing nothing named. The appearance is decoded once `op`, the render's
/// operation, and counted into its tally when the render is counting.
pub(crate) fn prepare(
    doc: &Arc<CosDocument>,
    annotation: &Dict,
    op: &Operation,
) -> Result<Appearance, NotDrawn> {
    let flags = annotation.get_int(doc.intern(b"F")).unwrap_or(0);
    if flags & HIDDEN != 0 || flags & NO_VIEW != 0 {
        return Err(NotDrawn::Hidden);
    }

    // A pop-up is the window that opens when its parent is clicked, not
    // something drawn on the page.
    if let Some(subtype) = annotation
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n))
    {
        if subtype.as_ref() == b"Popup" {
            return Err(NotDrawn::Popup);
        }
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

    Ok(Appearance {
        rect,
        content,
        transform,
        resources,
    })
}

/// Where an annotation draws on its page, as [`draw`] would draw it, read
/// from its dictionary and its appearance's without decoding anything
/// (`docs/design/performance.md`, P-17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drawn {
    /// Nowhere: hidden or `NoView` by its `/F` (12.5.3), a pop-up, no
    /// `/Rect` with an area, no normal appearance, an appearance that is not
    /// a form, or one 12.5.5's fit cannot place — each a reason [`prepare`]
    /// gives for drawing nothing.
    Nothing,
    /// Inside its `/Rect`, in default user space: an appearance with a
    /// `/BBox` is fitted onto the rectangle, and the box clips what it draws
    /// (8.10.2), so nothing it paints lands outside.
    Within(Rect),
    /// Wherever its content draws: an appearance with no `/BBox` is drawn
    /// where its own `/Matrix` puts it, unclipped.
    Unbounded,
}

/// [`Drawn`] for `annotation`: the decisions [`prepare`] takes, but for the
/// decode, which a bounded appearance does not need — one whose content does
/// not decode draws nothing, inside its rectangle as well.
pub(crate) fn drawn(doc: &CosDocument, annotation: &Dict) -> Drawn {
    let flags = annotation.get_int(doc.intern(b"F")).unwrap_or(0);
    if flags & HIDDEN != 0 || flags & NO_VIEW != 0 {
        return Drawn::Nothing;
    }
    let popup = annotation
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n))
        .is_some_and(|subtype| subtype.as_ref() == b"Popup");
    if popup {
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
            Some(_) => Drawn::Within(rect),
            None => Drawn::Nothing,
        },
    }
}

/// Draws a prepared appearance through `device`, its two scopes serving `op`,
/// the render's operation: one decode between them of each stream they read,
/// counted into its tally when the render is counting.
pub(crate) fn draw_prepared(
    doc: &Arc<CosDocument>,
    appearance: &Appearance,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut impl AppearanceDevice,
    op: &Operation,
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
        appearance.transform,
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

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
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
        assert_eq!(drawn(&doc, &annotation(Some(5), &|_| {})), within);
        assert_eq!(
            drawn(&doc, &annotation(Some(6), &|_| {})),
            Drawn::Unbounded,
            "no /BBox, so nothing clips it"
        );
        assert_eq!(
            drawn(&doc, &annotation(Some(7), &|_| {})),
            Drawn::Nothing,
            "a box with no area is never placed"
        );
        assert_eq!(drawn(&doc, &annotation(None, &|_| {})), Drawn::Nothing);
        for (what, flags) in [("hidden", HIDDEN), ("NoView", NO_VIEW)] {
            let hidden = annotation(Some(5), &|dict| {
                dict.insert(doc.intern(b"F"), Object::Int(flags | 4));
            });
            assert_eq!(drawn(&doc, &hidden), Drawn::Nothing, "{what}");
        }
        let popup = annotation(Some(5), &|dict| {
            dict.insert(Name::SUBTYPE, Object::Name(doc.intern(b"Popup")));
        });
        assert_eq!(drawn(&doc, &popup), Drawn::Nothing, "a pop-up");
        let flat = annotation(Some(5), &|dict| {
            dict.insert(doc.intern(b"Rect"), Object::Array(vec![Object::Int(5); 4]));
        });
        assert_eq!(drawn(&doc, &flat), Drawn::Nothing, "a rectangle of no area");
        let unplaced = annotation(Some(5), &|dict| {
            dict.insert(doc.intern(b"Rect"), Object::Null);
        });
        assert_eq!(drawn(&doc, &unplaced), Drawn::Nothing, "no rectangle");
        let stateless = annotation(None, &|dict| {
            let mut states = Dict::new();
            states.insert(doc.intern(b"On"), Object::Ref(ObjRef::new(5, 0)));
            let mut ap = Dict::new();
            ap.insert(Name::N, Object::Dict(states));
            dict.insert(doc.intern(b"AP"), Object::Dict(ap));
            dict.insert(doc.intern(b"AS"), Object::Name(doc.intern(b"Off")));
        });
        assert_eq!(
            drawn(&doc, &stateless),
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
