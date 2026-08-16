//! Dequantisation (T.800 E.1) and the inverse wavelet (Annex F).
//!
//! This is where a code-block's signed magnitudes stop being integers from an
//! arithmetic coder and become a picture, and it is the first stage with an
//! **independent oracle**: a reversible 5/3 decode must be byte-identical to
//! `opj_decompress`, because both decoders are exact. Everything before this
//! could only be checked against transcribed tables and a round trip through
//! a writer sharing its own assumptions.
//!
//! That gate does more work than it looks like. Byte-identity here pins the
//! container, tier-2's packet arithmetic, tier-1's context *numbering*,
//! dequantisation, and the DC level shift, all at once, against a decoder
//! sharing no code with this one. T.800 publishes no datastream annex — there
//! is no equivalent of T.88's Annex H.1, which is what gap 17 leaned on — so
//! this comparison carries that weight instead.
//!
//! # Two arithmetics, deliberately not mixed
//!
//! The **5/3** transform is reversible and exact: integer lifting, no rounding
//! beyond the floors the standard writes down, and therefore no fixed-point
//! format at all. Coefficient planes are `i32` at Q0 here.
//!
//! The **9/7** is irreversible and needs the Q12 planes and Q24 constants
//! `docs/plans/gaps/18a-jpx-decoder.md` settled before any of this was
//! written. It is milestone 5 and it is deliberately absent: mixing the two
//! is how that arithmetic decision gets made implicitly by whoever writes the
//! wavelet, which is the failure the plan exists to prevent.

use super::codestream::{Codestream, QuantStyle};
use super::tier2::{Orientation, Tile};
use super::Refusal;

/// One tile-component's samples, after the wavelet and the level shift.
pub(crate) struct Plane {
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) samples: Vec<i32>,
}

impl Plane {
    pub(crate) fn at(&self, x: u32, y: u32) -> i32 {
        self.get(x, y)
    }

    fn get(&self, x: u32, y: u32) -> i32 {
        if x < self.width && y < self.height {
            self.samples[(y as usize) * (self.width as usize) + (x as usize)]
        } else {
            0
        }
    }

    fn set(&mut self, x: u32, y: u32, v: i32) {
        if x < self.width && y < self.height {
            self.samples[(y as usize) * (self.width as usize) + (x as usize)] = v;
        }
    }
}

/// E.1: turn a subband's coded magnitudes into coefficients.
///
/// For the reversible path this is almost nothing, and the *almost* is the
/// interesting part. `SPqcd` carries an exponent and no step size, and the
/// coefficient **is** the signed magnitude tier-1 decoded — no realignment.
///
/// The obvious code is wrong here, and it is wrong in a way that looks right.
/// E.1 gives a subband `Mb = G + exp - 1` magnitude bits and a code-block
/// codes `Mb - zero_planes` of them, which reads as though the decoded
/// magnitudes were left-aligned and wanted shifting down by the difference.
/// They are not: tier-1 accumulates with `|= 1 << plane` where `plane` counts
/// down to zero, so the most significant bit it decodes already sits where
/// `Mb - zero_planes` puts it.
///
/// Applying that shift anyway halves every coefficient in a typical stream,
/// and an inverse wavelet turns a uniformly halved subband into a picture of
/// exactly the right shape at half the contrast — which is to say, into
/// something that looks like a decode rather than like a defect. Nothing in
/// this repository would have caught it. `opj_decompress` did, on the first
/// comparison.
fn dequantise(
    stream: &Codestream<'_>,
    tile: u16,
    component: usize,
    coefficients: &[i32],
) -> Result<Vec<i32>, Refusal> {
    match stream.quant_for(tile, component).style {
        // Reversible: exact, and the magnitudes are already the coefficients.
        QuantStyle::None => Ok(coefficients.to_vec()),
        // Derived and expounded are both irreversible and go with the 9/7,
        // which is milestone 5. Refusing by name beats scaling by a step size
        // the wavelet below cannot honour -- a wrong step size is another
        // defect that produces a picture.
        QuantStyle::Derived | QuantStyle::Expounded => {
            Err(Refusal::NotBuilt("the irreversible 9/7 wavelet"))
        }
    }
}

/// F.3.8.2's 1D synthesis lifting for the reversible 5/3, in place.
///
/// `y` is the interleaved signal, `i0` its first index. The two steps are the
/// standard's own, and their **order is not interchangeable**: the even step
/// runs first and the odd step reads the evens it just produced. Swapping
/// them is the analysis order, and it decodes to something that still looks
/// like an image.
fn synthesise_1d(y: &mut [i32], i0: u32) {
    let n = y.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        // F.3.7: a single sample is scaled rather than lifted, and only when
        // it sits on an odd index.
        if i0 % 2 == 1 {
            y[0] /= 2;
        }
        return;
    }

    // Symmetric extension (F.3.6): the signal is mirrored about its ends
    // without repeating the end sample, so index -1 reads index 1.
    let at = |y: &[i32], i: i64| -> i32 {
        let n = n as i64;
        let mut i = i;
        // Mirror repeatedly, which matters for a signal shorter than the
        // extension a filter asks for.
        while i < 0 || i >= n {
            if i < 0 {
                i = -i;
            }
            if i >= n {
                i = 2 * (n - 1) - i;
            }
        }
        y[i as usize]
    };

    let odd_first = i0 % 2 == 1;
    let evens: Vec<usize> = (0..n).filter(|i| (*i % 2 == 0) != odd_first).collect();
    let odds: Vec<usize> = (0..n).filter(|i| (*i % 2 == 0) == odd_first).collect();

    // Step 1: every even sample, from its odd neighbours.
    let mut out = y.to_vec();
    for &i in &evens {
        let i = i as i64;
        out[i as usize] = at(y, i) - (at(y, i - 1) + at(y, i + 1) + 2).div_euclid(4);
    }
    // Step 2: every odd sample, from the evens step 1 just wrote.
    for &i in &odds {
        let i = i as i64;
        let left = if i > 0 && ((i - 1) as usize) < n {
            out[(i - 1) as usize]
        } else {
            at(&out, i - 1)
        };
        let right = if i + 1 >= 0 && ((i + 1) as usize) < n {
            out[(i + 1) as usize]
        } else {
            at(&out, i + 1)
        };
        out[i as usize] = at(y, i) + (left + right).div_euclid(2);
    }
    y.copy_from_slice(&out);
}

/// F.3.4's 2D synthesis for one resolution step.
fn synthesise_2d(plane: &mut Plane, x0: u32, y0: u32) {
    // Rows, then columns -- F.3.4's HOR_SR followed by VER_SR.
    let mut row = vec![0i32; plane.width as usize];
    for y in 0..plane.height {
        for x in 0..plane.width {
            row[x as usize] = plane.get(x, y);
        }
        synthesise_1d(&mut row, x0);
        for x in 0..plane.width {
            plane.set(x, y, row[x as usize]);
        }
    }

    let mut column = vec![0i32; plane.height as usize];
    for x in 0..plane.width {
        for y in 0..plane.height {
            column[y as usize] = plane.get(x, y);
        }
        synthesise_1d(&mut column, y0);
        for y in 0..plane.height {
            plane.set(x, y, column[y as usize]);
        }
    }
}

/// Reconstruct one tile-component: dequantise every subband, then run the
/// inverse wavelet up the resolution ladder.
pub(crate) fn reconstruct(
    stream: &Codestream<'_>,
    tile: &Tile,
    component: usize,
) -> Result<Plane, Refusal> {
    let tc = tile
        .components
        .get(component)
        .ok_or(Refusal::Structure("a tile with no such component"))?;

    // Start from the coarsest LL, which is resolution 0's only subband.
    let mut plane = Plane {
        x0: tc.x0,
        y0: tc.y0,
        width: 0,
        height: 0,
        samples: Vec::new(),
    };

    for (r, resolution) in tc.resolutions.iter().enumerate() {
        let width = resolution.x1.saturating_sub(resolution.x0);
        let height = resolution.y1.saturating_sub(resolution.y0);
        let mut next = Plane {
            x0: resolution.x0,
            y0: resolution.y0,
            width,
            height,
            samples: vec![0; (width as usize) * (height as usize)],
        };

        if r == 0 {
            // Resolution 0 is the LL band alone, placed as it stands.
            for band in &resolution.bands {
                write_band(stream, tile, component, band, &mut next, 0, 0, false)?;
            }
        } else {
            // F.3.3's interleave: the previous resolution supplies the even
            // rows and columns, and this resolution's three bands supply the
            // rest.
            for y in 0..plane.height {
                for x in 0..plane.width {
                    next.set(2 * x, 2 * y, plane.get(x, y));
                }
            }
            for band in &resolution.bands {
                let (ox, oy) = match band.orientation {
                    Orientation::Hl => (1, 0),
                    Orientation::Lh => (0, 1),
                    Orientation::Hh => (1, 1),
                    Orientation::Ll => (0, 0),
                };
                write_band(stream, tile, component, band, &mut next, ox, oy, true)?;
            }
            synthesise_2d(&mut next, resolution.x0, resolution.y0);
        }
        plane = next;
    }

    Ok(plane)
}

/// Place one subband's dequantised coefficients into the plane.
#[allow(clippy::too_many_arguments, reason = "F.3.3's own parameter list")]
fn write_band(
    stream: &Codestream<'_>,
    tile: &Tile,
    component: usize,
    band: &super::tier2::Subband,
    plane: &mut Plane,
    ox: u32,
    oy: u32,
    interleaved: bool,
) -> Result<(), Refusal> {
    let bw = band.x1.saturating_sub(band.x0);
    for precinct in &band.precincts {
        for block in &precinct.blocks {
            if block.coefficients.is_empty() {
                continue;
            }
            let values = dequantise(stream, tile.index, component, &block.coefficients)?;
            let w = block.width();
            for (i, &v) in values.iter().enumerate() {
                let i = i as u32;
                if w == 0 {
                    break;
                }
                let (lx, ly) = (i % w, i / w);
                let bx = block.x0.saturating_sub(band.x0) + lx;
                let by = block.y0.saturating_sub(band.y0) + ly;
                if bx >= bw && bw != 0 {
                    continue;
                }
                if interleaved {
                    plane.set(2 * bx + ox, 2 * by + oy, v);
                } else {
                    plane.set(bx, by, v);
                }
            }
        }
    }
    Ok(())
}

/// G.1's DC level shift, and the clamp to the component's precision.
pub(crate) fn level_shift(plane: &mut Plane, precision: u8, signed: bool) {
    let shift = if signed {
        0
    } else {
        1i32 << (i32::from(precision) - 1)
    };
    let (lo, hi) = if signed {
        (
            -(1i32 << (i32::from(precision) - 1)),
            (1i32 << (i32::from(precision) - 1)) - 1,
        )
    } else {
        (0, (1i32 << i32::from(precision)) - 1)
    };
    for sample in &mut plane.samples {
        *sample = (*sample + shift).clamp(lo, hi);
    }
}
