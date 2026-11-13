//! Where the variable-font targets read a face: its default, its named
//! instances and its design space's corners, each count bounded so that a
//! face claiming thousands of instances or dozens of axes costs a target no
//! more than one that has a few.
//!
//! The corners are where a hostile store is likeliest to do something a
//! well-made face never asks: every region at the edge of its support, every
//! scalar zero or one, the intermediate regions crossed at their ends.

use tinker_pdf_font::variation::{DesignSpace, Instance, Location, MAX_AXES};

/// The most named instances a target reads a face at, whatever its `fvar`
/// claims.
pub const NAMED: usize = 8;

/// The leading axes whose corners are taken in every combination. Past them
/// a space contributes two more: every axis at its minimum, and every axis at
/// its maximum.
pub const CORNER_AXES: usize = 3;

/// The default, the first [`NAMED`] named instances a space resolves, then
/// [`corners`] of `axes` axes, in that order. `axes` is the space's own count
/// for a face with `fvar`, or a store's for a table read without one, and is
/// held to [`MAX_AXES`], the most a design space is read with.
#[must_use]
pub fn locations(space: &DesignSpace, axes: usize) -> Vec<Location> {
    let axes = axes.min(MAX_AXES);
    let mut out = vec![Location::new(vec![0; axes])];
    for index in 0..space.instances().len().min(NAMED) {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        if let Ok(location) = space.location(&Instance::Named(index)) {
            out.push(location);
        }
    }
    out.extend(corners(axes));
    out
}

/// The corners of a space of `axes` axes, as normalised 2.14 coordinates:
/// every combination of −1 and +1 over the first [`CORNER_AXES`] axes, the
/// rest at their defaults, and where there are more axes than that, every
/// axis at −1 and every axis at +1. None for a space with no axes.
#[must_use]
pub fn corners(axes: usize) -> Vec<Location> {
    let axes = axes.min(MAX_AXES);
    if axes == 0 {
        return Vec::new();
    }
    let leading = axes.min(CORNER_AXES);
    let mut out = Vec::with_capacity((1 << leading) + 2);
    for bits in 0..(1u32 << leading) {
        let mut coordinates = vec![0i16; axes];
        for (axis, slot) in coordinates.iter_mut().enumerate().take(leading) {
            *slot = if bits & (1 << axis) == 0 {
                -0x4000
            } else {
                0x4000
            };
        }
        out.push(Location::new(coordinates));
    }
    if axes > leading {
        out.push(Location::new(vec![-0x4000; axes]));
        out.push(Location::new(vec![0x4000; axes]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_with_no_axes_is_read_at_its_default_alone() {
        assert!(corners(0).is_empty());
        let locations = locations(&DesignSpace::default(), 0);
        assert_eq!(locations, vec![Location::new(Vec::new())]);
    }

    #[test]
    fn the_leading_axes_give_every_combination_of_their_ends() {
        let two = corners(2);
        assert_eq!(two.len(), 4);
        let mut seen: Vec<Vec<i16>> = two.iter().map(|l| l.coordinates().to_vec()).collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                vec![-0x4000, -0x4000],
                vec![-0x4000, 0x4000],
                vec![0x4000, -0x4000],
                vec![0x4000, 0x4000],
            ]
        );
    }

    #[test]
    fn past_the_leading_axes_every_axis_moves_together() {
        let five = corners(5);
        assert_eq!(five.len(), 8 + 2);
        for corner in &five[..8] {
            assert!(corner.coordinates()[3..].iter().all(|c| *c == 0));
        }
        assert_eq!(five[8].coordinates(), &[-0x4000; 5]);
        assert_eq!(five[9].coordinates(), &[0x4000; 5]);
    }

    #[test]
    fn a_claim_of_more_axes_than_a_space_reads_is_held_to_the_bound() {
        let held = corners(usize::from(u16::MAX));
        assert_eq!(held.len(), 8 + 2);
        assert!(held.iter().all(|l| l.coordinates().len() == MAX_AXES));
    }
}
