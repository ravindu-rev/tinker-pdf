//! Adding annotations to a page and flattening them into its content.

use super::{without, DocumentEditor};
use crate::name::Name;
use crate::object::{Dict, ObjRef, Object};
use crate::pages::Rect;

/// 12.5.5's algorithm, reduced to the scale and offset a `cm` needs.
///
/// The transformed bounding box is fitted to the rectangle. A form with no box
/// is drawn where its own matrix puts it, which is what a viewer does with one.
fn fit(bbox: Option<Rect>, matrix: [f64; 6], rect: Rect) -> (f64, f64, f64, f64) {
    let Some(bbox) = bbox.filter(|b| !b.is_empty()) else {
        return (1.0, 1.0, 0.0, 0.0);
    };

    let corners = [
        (bbox.x0, bbox.y0),
        (bbox.x1, bbox.y0),
        (bbox.x1, bbox.y1),
        (bbox.x0, bbox.y1),
    ];
    let mapped: Vec<(f64, f64)> = corners
        .iter()
        .map(|(x, y)| {
            (
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            )
        })
        .collect();

    let (mut x0, mut y0) = mapped[0];
    let (mut x1, mut y1) = mapped[0];
    for (x, y) in &mapped {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }

    let (dx, dy) = (x1 - x0, y1 - y0);
    if dx <= f64::EPSILON || dy <= f64::EPSILON {
        return (1.0, 1.0, 0.0, 0.0);
    }

    let sx = (rect.x1 - rect.x0) / dx;
    let sy = (rect.y1 - rect.y0) / dy;
    (sx, sy, rect.x0 - x0 * sx, rect.y0 - y0 * sy)
}

impl DocumentEditor {
    /// Draws a page's annotations into its content and removes them (12.5.5).
    ///
    /// Flattening is what makes an annotation permanent: a filled form or a
    /// highlight stops being a separate object a viewer may choose not to
    /// draw, or a user may delete. Only annotations with a normal appearance
    /// are flattened — one without has nothing to draw, and inventing
    /// something for it is the appearance synthesizer's job, not this.
    ///
    /// Returns how many were flattened.
    pub fn flatten_annotations(&mut self, page: u32) -> Option<usize> {
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(dict)) = self.get(reference) else {
            return None;
        };

        let annots_key = self.intern(b"Annots");
        let list: Vec<Object> = match dict.get(annots_key) {
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Ref(r)) => self
                .get(*r)
                .and_then(|o| o.as_array().map(<[Object]>::to_vec))
                .unwrap_or_default(),
            _ => return Some(0),
        };

        let mut painted = Vec::new();
        let mut names = Vec::new();
        let mut count = 0usize;

        for entry in &list {
            let Some(annot) = entry
                .as_objref()
                .and_then(|r| self.get(r))
                .and_then(|o| o.as_dict().cloned())
            else {
                continue;
            };

            // 12.5.3: hidden and no-view annotations are not on the page, so
            // flattening must not put them there.
            let flags = annot.get_int(self.intern(b"F")).unwrap_or(0);
            if flags & 2 != 0 || flags & 32 != 0 {
                continue;
            }

            let Some(rect) = self
                .doc
                .resolve_key(&annot, self.intern(b"Rect"))
                .as_array()
                .and_then(Rect::from_array)
            else {
                continue;
            };
            let Some(form) = self.normal_appearance(&annot) else {
                continue;
            };

            // 12.5.5 maps the form's bounding box onto the rectangle. The
            // synthesizer writes them equal, and a form from elsewhere may
            // not, so the scale is computed rather than assumed.
            let (bbox, matrix) = self.appearance_box(form);
            let (sx, sy, tx, ty) = fit(bbox, matrix, rect);

            let name = format!("TpdfFlat{count}");
            names.push((name.clone(), form));
            painted.push(format!("q {sx} 0 0 {sy} {tx} {ty} cm /{name} Do Q\n"));
            count += 1;
        }

        if count == 0 {
            return Some(0);
        }

        // The appearances become ordinary XObject resources of the page.
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return None;
        };
        let resources_key = Name::RESOURCES;
        let mut resources = self
            .doc
            .resolve_key(&dict, resources_key)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        let xobject_key = self.intern(b"XObject");
        let mut xobjects = resources
            .get_dict(xobject_key)
            .cloned()
            .unwrap_or_else(Dict::new);
        for (name, form) in names {
            xobjects.insert(self.intern(name.as_bytes()), Object::Ref(form));
        }
        resources.insert(xobject_key, Object::Dict(xobjects));
        dict.insert(resources_key, Object::Dict(resources));

        // The annotations go: a flattened one that stayed would be drawn
        // twice, once as content and once as itself.
        dict = without(&dict, annots_key);
        self.put(reference, Object::Dict(dict));

        self.append_content(page, painted.concat().as_bytes());
        Some(count)
    }

    /// The `/AP` `/N` stream of an annotation, when it has one.
    fn normal_appearance(&self, annot: &Dict) -> Option<ObjRef> {
        let ap = self.doc.resolve_key(annot, self.intern(b"AP"));
        let normal = ap.as_dict()?.get(self.intern(b"N"))?.clone();
        match normal {
            Object::Ref(r) => {
                let object = self.get(r)?;
                let dict = object.as_dict()?;
                if dict.get(self.intern(b"BBox")).is_some() {
                    return Some(r);
                }
                // A dictionary of states: the one `/AS` names.
                let state = annot.get_name(self.intern(b"AS"))?;
                dict.get_ref(state)
            }
            Object::Dict(states) => {
                let state = annot.get_name(self.intern(b"AS"))?;
                states.get_ref(state)
            }
            _ => None,
        }
    }

    /// An appearance stream's `/BBox` and `/Matrix`.
    fn appearance_box(&self, form: ObjRef) -> (Option<Rect>, [f64; 6]) {
        let Some(object) = self.get(form) else {
            return (None, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        };
        let Some(dict) = object.as_dict() else {
            return (None, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        };

        let bbox = self
            .doc
            .resolve_key(dict, self.intern(b"BBox"))
            .as_array()
            .and_then(Rect::from_array);

        let matrix = self
            .doc
            .resolve_key(dict, self.intern(b"Matrix"))
            .as_array()
            .map(|a| a.iter().filter_map(Object::as_number).collect::<Vec<f64>>())
            .filter(|v| v.len() >= 6 && v.iter().all(|x| x.is_finite()))
            .map(|v| [v[0], v[1], v[2], v[3], v[4], v[5]])
            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

        (bbox, matrix)
    }

    /// Adds an annotation to a page (12.5).
    ///
    /// An appearance stream is synthesized and attached unless the dictionary
    /// already carries an `/AP`, because an annotation without one looks like
    /// whatever the viewer decides and prints like nothing at all. Passing an
    /// `/AP` in keeps it untouched; [`crate::appearance::synthesize`] is what
    /// gets called otherwise, and the subtypes it declines are left bare.
    pub fn add_annotation(&mut self, page: u32, annotation: Dict) -> Option<ObjRef> {
        let reference = self.page_refs().get(page as usize).copied()?;
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return None;
        };

        let mut annotation = annotation;
        let ap = self.intern(b"AP");
        if !annotation.contains_key(ap) {
            if let Some(stream) = crate::appearance::synthesize(&self.doc, &annotation) {
                let form = self.allocate();
                self.put_stream(form, stream);
                let mut states = Dict::new();
                states.insert(self.intern(b"N"), Object::Ref(form));
                annotation.insert(ap, Object::Dict(states));
            }
        }

        let annot_ref = self.allocate();
        self.put(annot_ref, Object::Dict(annotation));

        let annots = self.intern(b"Annots");
        // /Annots may be direct or indirect; both are read, and the result is
        // written back directly, which is always legal.
        let mut list: Vec<Object> = match dict.get(annots) {
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Ref(r)) => self
                .get(*r)
                .and_then(|o| o.as_array().map(<[Object]>::to_vec))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        list.push(Object::Ref(annot_ref));
        dict.insert(annots, Object::Array(list));
        self.put(reference, Object::Dict(dict));

        Some(annot_ref)
    }
}
