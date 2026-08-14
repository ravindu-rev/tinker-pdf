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

use tinker_pdf_content::{interpret, Device, Matrix};
use tinker_pdf_cos::{pages as cos_pages, CosDocument, Dict, Object, Rect};

use crate::fonts::FontProvider;
use crate::resources::PageResources;

/// 12.5.3: the flags that keep an annotation off the page.
const HIDDEN: i64 = 1 << 1;
const NO_VIEW: i64 = 1 << 5;

/// Draws every visible annotation of a page.
pub fn draw<D: Device>(
    doc: &Arc<CosDocument>,
    page: &cos_pages::Page,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut D,
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
        if let Some(annotation) = annotation {
            draw_one(doc, &annotation, provider, device);
        }
    }
}

fn draw_one<D: Device>(
    doc: &Arc<CosDocument>,
    annotation: &Dict,
    provider: Option<&Arc<dyn FontProvider>>,
    device: &mut D,
) {
    let flags = annotation.get_int(doc.intern(b"F")).unwrap_or(0);
    if flags & HIDDEN != 0 || flags & NO_VIEW != 0 {
        return;
    }

    // A pop-up is the window that opens when its parent is clicked, not
    // something drawn on the page.
    if let Some(subtype) = annotation
        .get_name(doc.intern(b"Subtype"))
        .and_then(|n| doc.name_bytes(n))
    {
        if subtype.as_ref() == b"Popup" {
            return;
        }
    }

    let Some(rect) = doc
        .resolve_key(annotation, doc.intern(b"Rect"))
        .as_array()
        .and_then(Rect::from_array)
    else {
        return;
    };

    let Some(form) = normal_appearance(doc, annotation) else {
        return;
    };
    let Ok(content) = doc.stream_decoded(form) else {
        return;
    };
    let Ok(object) = doc.get(form) else { return };
    let Some(form_dict) = object.as_dict() else {
        return;
    };

    let matrix = matrix_of(doc, form_dict);
    let bbox = doc
        .resolve_key(form_dict, doc.intern(b"BBox"))
        .as_array()
        .and_then(Rect::from_array);

    let Some(transform) = fit(bbox, matrix, rect) else {
        return;
    };

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
        _ => content,
    };

    let resources = doc
        .resolve_key(form_dict, tinker_pdf_cos::Name::RESOURCES)
        .as_dict()
        .cloned()
        .unwrap_or_default();
    let resources = PageResources::from_dict(doc, resources, provider);

    interpret(&content, transform, device, &resources);
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
        let blank =
            Arc::new(CosDocument::open(builder.finish()).expect("the blank document opens"));

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
        let blank = Arc::new(CosDocument::open(builder.finish()).expect("the document opens"));

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
