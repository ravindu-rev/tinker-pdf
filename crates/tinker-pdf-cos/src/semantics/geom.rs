//! Page geometry: rectangles (7.9.5), rotation and effective display size
//! (7.7.3.3).

use core::fmt;

use crate::doc::CosDocument;
use crate::object::Object;

/// A rectangle (7.9.5), always normalized so `x0 <= x1` and `y0 <= y1`.
///
/// 7.9.5 defines the array as two diagonally opposite corners in either order,
/// and producers use every order there is, so nothing above this type ever has
/// to wonder which corner is which.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// Smaller x.
    pub x0: f64,
    /// Smaller y.
    pub y0: f64,
    /// Larger x.
    pub x1: f64,
    /// Larger y.
    pub y1: f64,
}

impl Rect {
    /// US Letter, 612 × 792 points: what every surviving reader falls back to
    /// for a page with no usable `/MediaBox` on its inheritance path.
    pub const US_LETTER: Rect = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: 612.0,
        y1: 792.0,
    };

    /// The rectangle two opposite corners describe, normalized.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    /// Width in points.
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    /// Height in points.
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Whether this rectangle encloses nothing a reader could display: zero or
    /// negative extent on either axis, or a corner that is not finite.
    pub fn is_degenerate(&self) -> bool {
        !(self.x0.is_finite() && self.y0.is_finite() && self.x1.is_finite() && self.y1.is_finite())
            || !(self.width() > 0.0)
            || !(self.height() > 0.0)
    }

    /// The overlap of two rectangles, which may be degenerate.
    pub fn intersect(&self, other: &Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{} {} {} {}]", self.x0, self.y0, self.x1, self.y1)
    }
}

/// The rectangle an object describes, if it is an array of four numbers.
///
/// Elements are resolved individually: an indirect number inside a `/MediaBox`
/// is legal and does happen.
pub(crate) fn rect_from_object(doc: &CosDocument, object: &Object) -> Option<Rect> {
    let items = object.as_array()?;
    let mut values = [0.0f64; 4];
    for (slot, item) in values.iter_mut().zip(items.iter().take(4)) {
        *slot = doc.resolve(item).as_number()?;
    }
    if items.len() < 4 {
        return None;
    }
    match values {
        [x0, y0, x1, y1] => Some(Rect::new(x0, y0, x1, y1)),
    }
}

/// Page rotation (7.7.3.3), normalized to the four values `/Rotate` may take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rotation {
    /// Upright.
    #[default]
    R0,
    /// A quarter turn clockwise.
    R90,
    /// Upside down.
    R180,
    /// A quarter turn counter-clockwise.
    R270,
}

impl Rotation {
    /// The rotation in degrees clockwise: 0, 90, 180 or 270.
    pub fn degrees(self) -> u16 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    /// Whether displaying at this rotation swaps width and height.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }

    /// Normalizes a raw `/Rotate` value, reporting whether it had to be
    /// snapped.
    ///
    /// 7.7.3.3 requires a multiple of 90 and the wild contains everything else,
    /// so a stray value snaps to the nearest quarter turn rather than failing.
    /// `rem_euclid` is what makes negative rotations — which are legal — land
    /// on the positive representative rather than on a negative remainder.
    pub fn from_degrees(degrees: i64) -> (Rotation, bool) {
        let wrapped = degrees.rem_euclid(360);
        let snapped = wrapped % 90 != 0;
        let quarters = (wrapped + 45) / 90 % 4;
        let rotation = match quarters {
            1 => Rotation::R90,
            2 => Rotation::R180,
            3 => Rotation::R270,
            _ => Rotation::R0,
        };
        (rotation, snapped)
    }
}

/// Everything a viewer needs to lay a page out, with rotation already applied
/// to the size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    /// The page's `/MediaBox`, inherited-resolved and normalized.
    pub media_box: Rect,
    /// The `/CropBox`, defaulted to the media box and intersected with it.
    pub crop_box: Rect,
    /// `/Rotate`, normalized.
    pub rotation: Rotation,
    /// Displayed width in points: the crop box's, with the 90/270 swap applied.
    pub width: f64,
    /// Displayed height in points.
    pub height: f64,
}

impl PageGeometry {
    pub(crate) fn new(media_box: Rect, crop_box: Rect, rotation: Rotation) -> PageGeometry {
        let (width, height) = if rotation.swaps_axes() {
            (crop_box.height(), crop_box.width())
        } else {
            (crop_box.width(), crop_box.height())
        };
        PageGeometry {
            media_box,
            crop_box,
            rotation,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_normalize_in_any_order() {
        let a = Rect::new(0.0, 0.0, 595.0, 842.0);
        let b = Rect::new(595.0, 842.0, 0.0, 0.0);
        let c = Rect::new(0.0, 842.0, 595.0, 0.0);
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a.width(), 595.0);
        assert_eq!(a.height(), 842.0);
    }

    #[test]
    fn degeneracy_covers_zero_and_infinity() {
        assert!(!Rect::new(0.0, 0.0, 1.0, 1.0).is_degenerate());
        assert!(Rect::new(5.0, 0.0, 5.0, 1.0).is_degenerate());
        assert!(Rect::new(0.0, 0.0, 1.0, f64::NAN).is_degenerate());
        assert!(Rect::new(0.0, 0.0, 1.0, f64::INFINITY).is_degenerate());
    }

    #[test]
    fn intersection_can_be_empty() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 20.0, 20.0);
        assert_eq!(a.intersect(&b), Rect::new(5.0, 5.0, 10.0, 10.0));
        let far = Rect::new(100.0, 100.0, 110.0, 110.0);
        assert!(a.intersect(&far).is_degenerate());
    }

    #[test]
    fn rotation_normalizes_by_rem_euclid() {
        assert_eq!(Rotation::from_degrees(0), (Rotation::R0, false));
        assert_eq!(Rotation::from_degrees(90), (Rotation::R90, false));
        assert_eq!(Rotation::from_degrees(450), (Rotation::R90, false));
        assert_eq!(Rotation::from_degrees(-90), (Rotation::R270, false));
        assert_eq!(Rotation::from_degrees(720), (Rotation::R0, false));
        assert_eq!(Rotation::from_degrees(-360), (Rotation::R0, false));
    }

    #[test]
    fn no_rotation_value_can_overflow() {
        for degrees in [i64::MIN, i64::MIN + 1, i64::MAX, -1, 359] {
            let (rotation, _) = Rotation::from_degrees(degrees);
            assert!(rotation.degrees() % 90 == 0);
        }
    }

    #[test]
    fn a_stray_rotation_snaps_to_the_nearest_quarter() {
        assert_eq!(Rotation::from_degrees(45), (Rotation::R90, true));
        assert_eq!(Rotation::from_degrees(44), (Rotation::R0, true));
        assert_eq!(Rotation::from_degrees(350), (Rotation::R0, true));
        assert_eq!(Rotation::from_degrees(1), (Rotation::R0, true));
    }

    #[test]
    fn geometry_swaps_axes_on_the_quarter_turns() {
        let box_ = Rect::new(0.0, 0.0, 595.0, 842.0);
        let upright = PageGeometry::new(box_, box_, Rotation::R0);
        assert_eq!((upright.width, upright.height), (595.0, 842.0));
        let turned = PageGeometry::new(box_, box_, Rotation::R270);
        assert_eq!((turned.width, turned.height), (842.0, 595.0));
    }
}
