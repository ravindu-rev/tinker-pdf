//! ITU-T T.832 9.9.3, 9.9.6 and 9.9.8: the photo overlap transform (POT).
//!
//! # The two levels are one geometry at two scales
//!
//! The Recommendation writes 9.9.3's first-level filter and 9.9.6's
//! second-level filter as two long, separate pseudocode functions, and they
//! look unrelated: one indexes `MbDCLP[MBx][MBy][i][j]` by macroblock and
//! coefficient, the other indexes `ImagePlane[i][x][y]` by sample. They are
//! the same function.
//!
//! Read `MbDCLP[x][y][i][j]` as a sample at `(4x + j % 4, 4y + j / 4)` of a
//! **DC plane** — one value per 4x4 block, so `MBWidth * 4` by `MBHeight * 4`
//! — and every one of 9.9.3.2's index quadruples becomes 9.9.6's, with the
//! tile boundaries at `4 * LeftMBIndexOfTile[ ]` instead of `16 *`. The
//! interior 4x4 filter, all four edges, all four corners and all seven
//! soft-tile cases line up exactly. So this file has **one** filter, called
//! at scale 4 over a materialised DC plane and at scale 16 over the sample
//! plane, and a transcription slip cannot hide in one level and not the
//! other.
//!
//! **One index does not line up, and it is recorded rather than smoothed
//! over.** 9.9.3.2's "right edge for soft tiles" block reads
//! `MbDCLP[x][y+1][i][4]` where its own non-soft counterpart reads `[3]` and
//! where the geometry requires `[3]`: the filter is a 4-point filter down a
//! single column, `[3]` is at `(4x + 3, 4y + 4)` and `[4]` is at
//! `(4x, 4y + 5)` — a different column entirely. This build implements `[3]`.
//! **No fixture reaches that path** — it needs a multi-tile image with
//! `OVERLAP_MODE` 2, and the tiled fixtures carry mode 1 — so the choice is
//! unadjudicated either way and is named as such in
//! `docs/features/xps.md` and `docs/design/jpeg-xr.md`.
//!
//! # Why the tile iteration order does not matter
//!
//! 9.9.3.2 iterates tiles rows-then-columns and 9.9.6 columns-then-rows, and
//! the filters are applied in place, so the difference would matter if any
//! two filter regions overlapped. None do: a tile's interior filters start
//! two samples inside it, its edge filters take the two outermost rows or
//! columns, its corner filters take the 2x2 the edges skip, and the
//! soft-tile filters straddle a boundary that the neighbouring tile's own
//! passes leave alone. Every sample near a block edge is filtered exactly
//! once.
//!
//! # Determinism and overflow
//!
//! Integer only, wrapping on overflow, for the reasons `transform.rs` states
//! at length.

#![deny(clippy::float_arithmetic)]

use super::transform::t2x2h;

// --- 9.9.8: the filter primitives ---------------------------------------

/// 9.9.8.5's `InvRotate( )`, on `(a, b)` as the clause's `iCoeff[0]` and
/// `iCoeff[1]`.
fn inv_rotate(a: i32, b: i32) -> (i32, i32) {
    let a = a.wrapping_sub((b.wrapping_add(1)) >> 1);
    let b = b.wrapping_add((a.wrapping_add(1)) >> 1);
    (a, b)
}

/// 9.9.8.6's `InvScale( )`.
///
/// The last four steps are the clause's rational approximation of the
/// filter's irrational scaling: `3/8`, `3/16`, `1/128` and `-1/1024`. The two
/// `+ 0` terms in the Recommendation's text are written out there and dropped
/// here because they are additive identities, not roundings — every other
/// lifting step in T.832 carries a real rounding constant, so a reader
/// checking this file against the clause should know the omission is
/// deliberate.
fn inv_scale(a: i32, b: i32) -> (i32, i32) {
    let mut a = a.wrapping_add(b);
    let mut b = (a >> 1).wrapping_sub(b);
    a = a.wrapping_add((b.wrapping_mul(3)) >> 3);
    b = b.wrapping_add((a.wrapping_mul(3)) >> 4);
    b = b.wrapping_add(a >> 7);
    b = b.wrapping_sub(a >> 10);
    (a, b)
}

/// 9.9.8.7's `T2x2hPOST( )`.
fn t2x2h_post(c: &mut [i32; 4]) {
    c[1] = c[1].wrapping_sub(c[2]);
    c[0] = c[0].wrapping_add((c[3].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[3] = c[3].wrapping_sub(c[1] >> 1);
    c[2] = ((c[0].wrapping_sub(c[1])) >> 1).wrapping_sub(c[2]);
    c.swap(2, 3);
    c[0] = c[0].wrapping_sub(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
}

/// 9.9.8.8's `InvToddoddPOST( )`.
///
/// Not `InvToddodd( )` with different constants: it also lacks that
/// function's two closing negations. The rounding constants differ too — 6,
/// 2 and 4 against 3, 3 and 4 — so the two are transcribed separately even
/// though they are the same shape.
fn inv_toddodd_post(c: &mut [i32; 4]) {
    c[3] = c[3].wrapping_add(c[0]);
    c[2] = c[2].wrapping_sub(c[1]);
    let t1 = c[3] >> 1;
    let t2 = c[2] >> 1;
    c[0] = c[0].wrapping_sub(t1);
    c[1] = c[1].wrapping_add(t2);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(6)) >> 3);
    c[1] = c[1].wrapping_add((c[0].wrapping_mul(3).wrapping_add(2)) >> 2);
    c[0] = c[0].wrapping_sub((c[1].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_sub(t2);
    c[0] = c[0].wrapping_add(t1);
    c[2] = c[2].wrapping_add(c[1]);
    c[3] = c[3].wrapping_sub(c[0]);
}

/// 9.9.8.2's `OverlapPostFilter4( )`, the four-point filter used along every
/// edge and corner.
pub(crate) fn overlap_post_filter4(c: &mut [i32; 4]) {
    c[0] = c[0].wrapping_add(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
    c[3] = c[3].wrapping_sub((c[0].wrapping_add(1)) >> 1);
    c[2] = c[2].wrapping_sub((c[1].wrapping_add(1)) >> 1);
    (c[0], c[3]) = inv_scale(c[0], c[3]);
    (c[1], c[2]) = inv_scale(c[1], c[2]);
    c[0] = c[0].wrapping_add((c[3].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[1] = c[1].wrapping_add((c[2].wrapping_mul(3).wrapping_add(4)) >> 3);
    c[3] = c[3].wrapping_sub(c[0] >> 1);
    c[2] = c[2].wrapping_sub(c[1] >> 1);
    c[0] = c[0].wrapping_add(c[3]);
    c[1] = c[1].wrapping_add(c[2]);
    c[3] = c[3].wrapping_neg();
    c[2] = c[2].wrapping_neg();
    (c[2], c[3]) = inv_rotate(c[2], c[3]);
    c[3] = c[3].wrapping_add((c[0].wrapping_add(1)) >> 1);
    c[2] = c[2].wrapping_add((c[1].wrapping_add(1)) >> 1);
    c[0] = c[0].wrapping_sub(c[3]);
    c[1] = c[1].wrapping_sub(c[2]);
}

/// 9.9.8.1's `OverlapPostFilter4x4( )`, the interior filter.
///
/// The sixteen values are in raster order, so the quadruples below are the
/// clause's own and every one of them straddles a block corner rather than
/// sitting inside a block — which is the whole point of a *lapped* transform.
pub(crate) fn overlap_post_filter4x4(c: &mut [i32; 16]) {
    for q in [[0, 3, 12, 15], [1, 2, 13, 14], [4, 7, 8, 11], [5, 6, 9, 10]] {
        quad(c, q, |a| t2x2h(a, 0));
    }
    for (a, b) in [(13usize, 12usize), (9, 8), (7, 3), (6, 2)] {
        let (x, y) = inv_rotate(c[a], c[b]);
        c[a] = x;
        c[b] = y;
    }
    quad(c, [10, 11, 14, 15], inv_toddodd_post);
    for (a, b) in [(0usize, 15usize), (1, 14), (4, 11), (5, 10)] {
        let (x, y) = inv_scale(c[a], c[b]);
        c[a] = x;
        c[b] = y;
    }
    for q in [[0, 3, 12, 15], [1, 2, 13, 14], [4, 7, 8, 11], [5, 6, 9, 10]] {
        quad(c, q, t2x2h_post);
    }
}

/// Gathers four of sixteen values, applies `op`, and scatters them back.
fn quad(c: &mut [i32; 16], at: [usize; 4], op: impl FnOnce(&mut [i32; 4])) {
    let mut local = [c[at[0]], c[at[1]], c[at[2]], c[at[3]]];
    op(&mut local);
    for (slot, value) in at.into_iter().zip(local) {
        c[slot] = value;
    }
}

// --- the geometry --------------------------------------------------------

/// The tile and macroblock layout both levels of the filter index by.
pub(crate) struct Geometry {
    pub(crate) components: usize,
    pub(crate) mb_width: usize,
    pub(crate) mb_height: usize,
    pub(crate) ext_width: usize,
    pub(crate) ext_height: usize,
    pub(crate) hard_tiling: bool,
    pub(crate) left_mb_of_tile: Vec<u32>,
    pub(crate) top_mb_of_tile: Vec<u32>,
    pub(crate) num_tile_cols: usize,
    pub(crate) num_tile_rows: usize,
}

/// A plane the filter walks: samples plus the row stride.
struct Plane<'a> {
    data: &'a mut [i32],
    width: usize,
    height: usize,
}

impl Plane<'_> {
    fn get(&self, x: usize, y: usize) -> i32 {
        self.data
            .get(y * self.width + x)
            .copied()
            .unwrap_or_default()
    }

    fn set(&mut self, x: usize, y: usize, v: i32) {
        if x < self.width && y < self.height {
            if let Some(slot) = self.data.get_mut(y * self.width + x) {
                *slot = v;
            }
        }
    }

    /// The 4x4 interior filter at `(x, y)`.
    fn filter4x4(&mut self, x: usize, y: usize) {
        if x + 4 > self.width || y + 4 > self.height {
            return;
        }
        let mut local = [0i32; 16];
        for row in 0..4 {
            for col in 0..4 {
                local[row * 4 + col] = self.get(x + col, y + row);
            }
        }
        overlap_post_filter4x4(&mut local);
        for row in 0..4 {
            for col in 0..4 {
                self.set(x + col, y + row, local[row * 4 + col]);
            }
        }
    }

    /// A four-point filter down one column, rows `y ..= y + 3`.
    fn filter_column(&mut self, x: usize, y: usize) {
        if x >= self.width || y + 4 > self.height {
            return;
        }
        let mut local = [
            self.get(x, y),
            self.get(x, y + 1),
            self.get(x, y + 2),
            self.get(x, y + 3),
        ];
        overlap_post_filter4(&mut local);
        for (row, v) in local.into_iter().enumerate() {
            self.set(x, y + row, v);
        }
    }

    /// A four-point filter along one row, columns `x ..= x + 3`.
    fn filter_row(&mut self, x: usize, y: usize) {
        if y >= self.height || x + 4 > self.width {
            return;
        }
        let mut local = [
            self.get(x, y),
            self.get(x + 1, y),
            self.get(x + 2, y),
            self.get(x + 3, y),
        ];
        overlap_post_filter4(&mut local);
        for (col, v) in local.into_iter().enumerate() {
            self.set(x + col, y, v);
        }
    }

    /// A four-point filter over the 2x2 corner at `(x, y)`, in the raster
    /// order the clause gathers it in.
    fn filter_corner(&mut self, x: usize, y: usize) {
        if x + 2 > self.width || y + 2 > self.height {
            return;
        }
        let mut local = [
            self.get(x, y),
            self.get(x + 1, y),
            self.get(x, y + 1),
            self.get(x + 1, y + 1),
        ];
        overlap_post_filter4(&mut local);
        self.set(x, y, local[0]);
        self.set(x + 1, y, local[1]);
        self.set(x, y + 1, local[2]);
        self.set(x + 1, y + 1, local[3]);
    }
}

/// 9.9.6's structure, at whatever scale the level works in: 16 samples per
/// macroblock for the second level, 4 for the first.
///
/// See the module docs for why 9.9.3 needs no separate implementation.
fn filter_plane(plane: &mut Plane<'_>, g: &Geometry, scale: usize) {
    let hard = g.hard_tiling;
    let last_col = g.num_tile_cols.saturating_sub(1);
    let last_row = g.num_tile_rows.saturating_sub(1);
    for tx in 0..g.num_tile_cols {
        for ty in 0..g.num_tile_rows {
            let Some(bounds) = tile_bounds(g, tx, ty, scale) else {
                continue;
            };
            let (left, right, top, bottom) = bounds;
            // Two samples in from each edge is where a 4x4 filter can sit
            // without running off the tile, and `right` and `bottom` are
            // exclusive — hence the `- 2` on both, which is 9.9.6's own.
            let interior_x = || (left + 2..right.saturating_sub(2)).step_by(4);
            let interior_y = || (top + 2..bottom.saturating_sub(2)).step_by(4);

            for x in interior_x() {
                for y in interior_y() {
                    plane.filter4x4(x, y);
                }
            }
            if tx == 0 || hard {
                for y in interior_y() {
                    plane.filter_column(left, y);
                    plane.filter_column(left + 1, y);
                }
            }
            if ty == 0 || hard {
                for x in interior_x() {
                    plane.filter_row(x, top);
                    plane.filter_row(x, top + 1);
                }
            }
            if tx == last_col || hard {
                for y in interior_y() {
                    plane.filter_column(right.saturating_sub(2), y);
                    plane.filter_column(right.saturating_sub(1), y);
                }
            }
            if ty == last_row || hard {
                for x in interior_x() {
                    plane.filter_row(x, bottom.saturating_sub(2));
                    plane.filter_row(x, bottom.saturating_sub(1));
                }
            }
            // The four corners, in the raster order 9.9.6 applies them.
            if (tx == 0 && ty == 0) || hard {
                plane.filter_corner(left, top);
            }
            if (tx == last_col && ty == 0) || hard {
                plane.filter_corner(right.saturating_sub(2), top);
            }
            if (tx == 0 && ty == last_row) || hard {
                plane.filter_corner(left, bottom.saturating_sub(2));
            }
            if (tx == last_col && ty == last_row) || hard {
                plane.filter_corner(right.saturating_sub(2), bottom.saturating_sub(2));
            }
            if hard {
                continue;
            }
            // Soft tiles: the filter crosses the boundary, so the work that
            // straddles it belongs to the tile on the near side. The
            // neighbouring tile's own passes leave this region alone, which
            // is what keeps every sample filtered exactly once.
            let across_x = right.saturating_sub(2);
            let across_y = bottom.saturating_sub(2);
            if tx != last_col {
                for y in interior_y() {
                    plane.filter4x4(across_x, y);
                }
            }
            if ty != last_row {
                for x in interior_x() {
                    plane.filter4x4(x, across_y);
                }
            }
            if tx != last_col && ty != last_row {
                plane.filter4x4(across_x, across_y);
            }
            if tx == 0 && ty != last_row {
                plane.filter_column(left, across_y);
                plane.filter_column(left + 1, across_y);
            }
            if tx != last_col && ty == 0 {
                plane.filter_row(across_x, top);
                plane.filter_row(across_x, top + 1);
            }
            if tx == last_col && ty != last_row {
                plane.filter_column(across_x, across_y);
                plane.filter_column(right.saturating_sub(1), across_y);
            }
            if tx != last_col && ty == last_row {
                plane.filter_row(across_x, across_y);
                plane.filter_row(across_x, bottom.saturating_sub(1));
            }
        }
    }
}

/// One tile's sample bounds at `scale`, as `(left, right, top, bottom)` with
/// the right and bottom exclusive.
fn tile_bounds(
    g: &Geometry,
    tx: usize,
    ty: usize,
    scale: usize,
) -> Option<(usize, usize, usize, usize)> {
    let at = |v: &[u32], i: usize| v.get(i).map(|&x| x as usize * scale);
    Some((
        at(&g.left_mb_of_tile, tx)?,
        at(&g.left_mb_of_tile, tx + 1)?,
        at(&g.top_mb_of_tile, ty)?,
        at(&g.top_mb_of_tile, ty + 1)?,
    ))
}

// --- the two levels ------------------------------------------------------

/// 9.9.3's `FirstLevelOverlapFiltering( )`, run only when `OVERLAP_MODE` is 2.
///
/// The DC plane is materialised rather than indexed in place: at four values
/// per macroblock it is a sixteenth of the image, and the alternative is a
/// second copy of `filter_plane` written against a different index
/// expression, which is the copy that would be wrong.
pub(crate) fn first_level(dclp: &mut [i32], g: &Geometry) {
    let (w, h) = (g.mb_width * 4, g.mb_height * 4);
    if w == 0 || h == 0 {
        return;
    }
    let mut dc = vec![0i32; w * h];
    for i in 0..g.components {
        gather_dc_plane(dclp, g, i, &mut dc, w);
        let mut plane = Plane {
            data: &mut dc,
            width: w,
            height: h,
        };
        filter_plane(&mut plane, g, 4);
        scatter_dc_plane(dclp, g, i, &dc, w);
    }
}

/// 9.9.6's `SecondLevelOverlapFiltering( )`, run whenever `OVERLAP_MODE` is
/// not 0.
pub(crate) fn second_level(samples: &mut [i32], g: &Geometry) {
    let mut plane = Plane {
        data: samples,
        width: g.ext_width,
        height: g.ext_height,
    };
    filter_plane(&mut plane, g, 16);
}

/// `MbDCLP[x][y][i][j]` into the DC plane at `(4x + j % 4, 4y + j / 4)`.
fn gather_dc_plane(dclp: &[i32], g: &Geometry, i: usize, dc: &mut [i32], w: usize) {
    for my in 0..g.mb_height {
        for mx in 0..g.mb_width {
            let base = ((my * g.mb_width + mx) * g.components + i) * 16;
            for j in 0..16 {
                let v = dclp.get(base + j).copied().unwrap_or_default();
                dc[(4 * my + j / 4) * w + 4 * mx + j % 4] = v;
            }
        }
    }
}

/// The reverse of [`gather_dc_plane`].
fn scatter_dc_plane(dclp: &mut [i32], g: &Geometry, i: usize, dc: &[i32], w: usize) {
    for my in 0..g.mb_height {
        for mx in 0..g.mb_width {
            let base = ((my * g.mb_width + mx) * g.components + i) * 16;
            for j in 0..16 {
                let v = dc[(4 * my + j / 4) * w + 4 * mx + j % 4];
                if let Some(slot) = dclp.get_mut(base + j) {
                    *slot = v;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/overlap.rs"]
mod tests;
