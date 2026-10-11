//! The production boundaries of a page (14.11.2): bleed, trim and art boxes,
//! and any of the five by name.

use super::DocumentEditor;
use crate::object::Object;
use crate::pages::PageBoundary;

impl DocumentEditor {
    /// Sets one of a page's five boundaries (14.11.2), in the page's own user
    /// space, on the page dictionary itself.
    ///
    /// [`DocumentEditor::set_crop_box`]'s rules, for every boundary: the
    /// rectangle is written as the caller gives it, corners ordered, and
    /// **not** clipped to the media box — 14.11.2.1 has a reader reduce a box
    /// to its intersection with the media box, and doing it here as well would
    /// mean the editor could not write the file its caller asked for. A
    /// rectangle with a coordinate that is not finite, or of no area, is
    /// refused: 7.9.5 wants two distinct corners, and a `NaN` is not a PDF
    /// number at all.
    ///
    /// Written on the page, never on a `/Pages` node: `/BleedBox`, `/TrimBox`
    /// and `/ArtBox` are not inheritable (7.7.3.3 Table 30), so that is the
    /// only place they mean anything, and a media or crop box written there
    /// overrides whatever the page inherited, which is what setting it means.
    ///
    /// Returns false, changing nothing, for a refused rectangle or a page that
    /// does not exist.
    pub fn set_page_boundary(
        &mut self,
        index: u32,
        boundary: PageBoundary,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> bool {
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return false;
        }
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (bottom, top) = (y0.min(y1), y0.max(y1));
        if right - left <= 0.0 || top - bottom <= 0.0 {
            return false;
        }
        let Some(reference) = self.page_refs().get(index as usize).copied() else {
            return false;
        };
        let Some(Object::Dict(mut dict)) = self.get(reference) else {
            return false;
        };
        dict.insert(
            self.intern(boundary.key()),
            Object::Array(vec![
                Object::Real(left),
                Object::Real(bottom),
                Object::Real(right),
                Object::Real(top),
            ]),
        );
        self.put(reference, Object::Dict(dict));
        true
    }

    /// Sets a page's `/BleedBox` (14.11.2): where its content is clipped in a
    /// production environment. See [`DocumentEditor::set_page_boundary`].
    pub fn set_bleed_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        self.set_page_boundary(index, PageBoundary::BleedBox, x0, y0, x1, y1)
    }

    /// Sets a page's `/TrimBox` (14.11.2): the finished page after trimming.
    /// See [`DocumentEditor::set_page_boundary`].
    pub fn set_trim_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        self.set_page_boundary(index, PageBoundary::TrimBox, x0, y0, x1, y1)
    }

    /// Sets a page's `/ArtBox` (14.11.2): the extent of its meaningful
    /// content. See [`DocumentEditor::set_page_boundary`].
    pub fn set_art_box(&mut self, index: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
        self.set_page_boundary(index, PageBoundary::ArtBox, x0, y0, x1, y1)
    }
}
