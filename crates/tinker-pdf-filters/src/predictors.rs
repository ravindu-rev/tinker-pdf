//! The Table 10 predictors (7.4.4.4): TIFF horizontal differencing and the
//! PNG row filters, applied as a post-pass to Flate and LZW output.

use crate::{FilterError, Limits, Warning, Warnings};

/// `/DecodeParms` for a predictor, already turned into plain values by COS.
///
/// The defaults are Table 10's: no prediction, one colour, 8 bits per
/// component, one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PredictorParams {
    /// 1 (or less) none, 2 TIFF, 10..=15 PNG. Values 3..=9 are not defined.
    pub predictor: i32,
    pub colors: u32,
    pub bits_per_component: u32,
    pub columns: u32,
}

impl Default for PredictorParams {
    fn default() -> Self {
        Self {
            predictor: 1,
            colors: 1,
            bits_per_component: 8,
            columns: 1,
        }
    }
}

impl PredictorParams {
    /// Whether these parameters ask for any prediction at all.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.predictor >= 2
    }
}

struct Geometry {
    colors: usize,
    bpc: usize,
    row_bytes: usize,
    /// `ceil(colors * bpc / 8)`, at least 1: the PNG left-neighbour offset.
    bpp: usize,
    samples_per_row: usize,
}

impl Geometry {
    fn new(p: &PredictorParams) -> Result<Self, FilterError> {
        if p.colors == 0 {
            return Err(FilterError::BadParams("/Colors must be at least 1"));
        }
        if p.columns == 0 {
            return Err(FilterError::BadParams("/Columns must be at least 1"));
        }
        if !matches!(p.bits_per_component, 1 | 2 | 4 | 8 | 16) {
            return Err(FilterError::BadParams(
                "/BitsPerComponent must be 1, 2, 4, 8 or 16",
            ));
        }
        let too_large = FilterError::BadParams("predictor row too large");
        let row_bits = u64::from(p.colors)
            .checked_mul(u64::from(p.bits_per_component))
            .and_then(|v| v.checked_mul(u64::from(p.columns)))
            .ok_or(too_large)?;
        let row_bytes = usize::try_from(row_bits.div_ceil(8)).map_err(|_| too_large)?;
        let bpp = usize::try_from(
            u64::from(p.colors)
                .checked_mul(u64::from(p.bits_per_component))
                .ok_or(too_large)?
                .div_ceil(8),
        )
        .map_err(|_| too_large)?
        .max(1);
        let samples_per_row = usize::try_from(
            u64::from(p.colors)
                .checked_mul(u64::from(p.columns))
                .ok_or(too_large)?,
        )
        .map_err(|_| too_large)?;
        Ok(Self {
            colors: p.colors as usize,
            bpc: p.bits_per_component as usize,
            row_bytes,
            bpp,
            samples_per_row,
        })
    }
}

pub(crate) fn unpredict(
    input: &[u8],
    p: &PredictorParams,
    limits: &Limits,
    w: &mut Warnings,
) -> Result<(Vec<u8>, bool), FilterError> {
    if !p.is_active() {
        return Ok(passthrough(input, limits, w));
    }
    let g = Geometry::new(p)?;
    match p.predictor {
        2 => Ok(tiff(input, &g, limits, w)),
        v if v >= 10 => Ok(png(input, &g, limits, w)),
        _ => Err(FilterError::BadParams("unknown /Predictor value")),
    }
}

fn passthrough(input: &[u8], limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    match input.get(..limits.max_output) {
        Some(head) if head.len() < input.len() => {
            w.push(Warning::OutputCapHit);
            (head.to_vec(), false)
        }
        _ => (input.to_vec(), true),
    }
}

fn png(input: &[u8], g: &Geometry, limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    let buf = g.row_bytes.min(input.len());
    let mut prev = vec![0u8; buf];
    let mut cur = vec![0u8; buf];
    let mut out = Vec::new();
    let mut complete = true;
    let mut pos = 0usize;

    while pos < input.len() {
        // Each PNG row carries its own tag byte; the declared 10..=15 value
        // does not select the filter.
        let tag = match input.get(pos) {
            Some(&t) => t,
            None => break,
        };
        pos += 1;
        let avail = g.row_bytes.min(input.len() - pos);
        let short = avail < g.row_bytes;
        if short {
            w.push(Warning::PredictorRowShort);
            complete = false;
        }
        if avail == 0 {
            break;
        }
        match (input.get(pos..pos + avail), cur.get_mut(..avail)) {
            (Some(src), Some(dst)) => dst.copy_from_slice(src),
            _ => break,
        }
        pos += avail;
        if let (Some(row), Some(up)) = (cur.get_mut(..avail), prev.get(..avail)) {
            unfilter(tag, row, up, g.bpp, w);
        }

        let room = limits.max_output.saturating_sub(out.len());
        let n = avail.min(room);
        if let Some(s) = cur.get(..n) {
            out.extend_from_slice(s);
        }
        if n < avail {
            w.push(Warning::OutputCapHit);
            return (out, false);
        }
        core::mem::swap(&mut prev, &mut cur);
        if short {
            break;
        }
    }
    (out, complete)
}

fn unfilter(tag: u8, cur: &mut [u8], prev: &[u8], bpp: usize, w: &mut Warnings) {
    let n = cur.len();
    match tag {
        0 => {}
        1 => {
            for i in bpp..n {
                let a = cur[i - bpp];
                cur[i] = cur[i].wrapping_add(a);
            }
        }
        2 => {
            for (c, &b) in cur.iter_mut().zip(prev.iter()) {
                *c = c.wrapping_add(b);
            }
        }
        3 => {
            for i in 0..n {
                let a = if i >= bpp { u16::from(cur[i - bpp]) } else { 0 };
                let b = u16::from(prev.get(i).copied().unwrap_or(0));
                cur[i] = cur[i].wrapping_add(((a + b) / 2) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let b = prev.get(i).copied().unwrap_or(0);
                let c = if i >= bpp {
                    prev.get(i - bpp).copied().unwrap_or(0)
                } else {
                    0
                };
                cur[i] = cur[i].wrapping_add(paeth(a, b, c));
            }
        }
        _ => w.push(Warning::BadPredictorTag),
    }
}

/// PNG 9.4 Paeth predictor.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ai, bi, ci) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ai + bi - ci;
    let (pa, pb, pc) = ((p - ai).abs(), (p - bi).abs(), (p - ci).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn tiff(input: &[u8], g: &Geometry, limits: &Limits, w: &mut Warnings) -> (Vec<u8>, bool) {
    let buf = g.row_bytes.min(input.len());
    let mut row = vec![0u8; buf];
    let mut out = Vec::new();
    let mut complete = true;
    let mut pos = 0usize;

    while pos < input.len() {
        let avail = g.row_bytes.min(input.len() - pos);
        let short = avail < g.row_bytes;
        if short {
            w.push(Warning::PredictorRowShort);
            complete = false;
        }
        match (input.get(pos..pos + avail), row.get_mut(..avail)) {
            (Some(src), Some(dst)) => dst.copy_from_slice(src),
            _ => break,
        }
        pos += avail;
        if let Some(r) = row.get_mut(..avail) {
            tiff_row(r, g);
        }

        let room = limits.max_output.saturating_sub(out.len());
        let n = avail.min(room);
        if let Some(s) = row.get(..n) {
            out.extend_from_slice(s);
        }
        if n < avail {
            w.push(Warning::OutputCapHit);
            return (out, false);
        }
        if short {
            break;
        }
    }
    (out, complete)
}

/// Predictor 2: each component is the difference from the same component of
/// the pixel to its left, wrapping at the component width.
fn tiff_row(row: &mut [u8], g: &Geometry) {
    let n = row.len();
    match g.bpc {
        8 => {
            for i in g.colors..n {
                let left = row[i - g.colors];
                row[i] = row[i].wrapping_add(left);
            }
        }
        16 => {
            let stride = g.colors * 2;
            let mut i = stride;
            while i + 1 < n {
                let (Some(&h), Some(&l)) = (row.get(i), row.get(i + 1)) else {
                    break;
                };
                let (Some(&lh), Some(&ll)) = (row.get(i - stride), row.get(i - stride + 1)) else {
                    break;
                };
                let v = u16::from_be_bytes([h, l]).wrapping_add(u16::from_be_bytes([lh, ll]));
                let b = v.to_be_bytes();
                if let Some(s) = row.get_mut(i..i + 2) {
                    s.copy_from_slice(&b);
                }
                i += 2;
            }
        }
        // 1, 2 and 4 take the slow bit-level path: correct rather than fast,
        // which matches how rarely sub-byte components occur.
        bpc => {
            let mask = (1u32 << bpc) - 1;
            let avail_samples = (n * 8) / bpc;
            let total = g.samples_per_row.min(avail_samples);
            for i in g.colors..total {
                let left = get_sample(row, i - g.colors, bpc);
                let here = get_sample(row, i, bpc);
                set_sample(row, i, bpc, (here.wrapping_add(left)) & mask);
            }
        }
    }
}

/// Only 1, 2 and 4 reach here, and each divides 8, so a sample never
/// straddles a byte boundary.
fn get_sample(row: &[u8], i: usize, bpc: usize) -> u32 {
    let bit = i * bpc;
    let shift = 8usize.saturating_sub(bpc).saturating_sub(bit % 8);
    match row.get(bit / 8) {
        Some(&b) => (u32::from(b) >> shift) & ((1u32 << bpc) - 1),
        None => 0,
    }
}

fn set_sample(row: &mut [u8], i: usize, bpc: usize, v: u32) {
    let bit = i * bpc;
    let shift = 8usize.saturating_sub(bpc).saturating_sub(bit % 8);
    let mask = ((1u32 << bpc) - 1) << shift;
    if let Some(b) = row.get_mut(bit / 8) {
        let cleared = u32::from(*b) & !mask;
        *b = (cleared | ((v << shift) & mask)) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Limits = Limits::new(1 << 20);

    fn run(input: &[u8], p: &PredictorParams) -> (Vec<u8>, bool, Vec<Warning>) {
        let mut w = Warnings::default();
        let (d, c) = unpredict(input, p, &CAP, &mut w).expect("params valid in tests");
        (d, c, w.into_vec())
    }

    fn params(predictor: i32, colors: u32, bpc: u32, columns: u32) -> PredictorParams {
        PredictorParams {
            predictor,
            colors,
            bits_per_component: bpc,
            columns,
        }
    }

    #[test]
    fn defaults_are_table_10() {
        let p = PredictorParams::default();
        assert_eq!(p.predictor, 1);
        assert_eq!(p.colors, 1);
        assert_eq!(p.bits_per_component, 8);
        assert_eq!(p.columns, 1);
        assert!(!p.is_active());
    }

    #[test]
    fn predictor_1_is_a_passthrough() {
        let (d, c, w) = run(b"abcdef", &PredictorParams::default());
        assert_eq!(d, b"abcdef");
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn bad_params_are_rejected() {
        let mut w = Warnings::default();
        assert!(unpredict(b"", &params(10, 0, 8, 4), &CAP, &mut w).is_err());
        assert!(unpredict(b"", &params(10, 1, 8, 0), &CAP, &mut w).is_err());
        assert!(unpredict(b"", &params(10, 1, 3, 4), &CAP, &mut w).is_err());
        assert!(unpredict(b"", &params(7, 1, 8, 4), &CAP, &mut w).is_err());
        assert!(unpredict(b"", &params(15, 1, 8, 4), &CAP, &mut w).is_ok());
        assert!(unpredict(b"", &params(2, 1, 16, 4), &CAP, &mut w).is_ok());
    }

    #[test]
    fn png_none_row_is_copied() {
        let p = params(15, 1, 8, 4);
        let (d, c, w) = run(&[0, 1, 2, 3, 4], &p);
        assert_eq!(d, vec![1, 2, 3, 4]);
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn png_sub_up_average_paeth_by_hand() {
        // One 4-byte row per filter, colors=1 bpc=8 columns=4 so bpp=1.
        let p = params(12, 1, 8, 4);
        // Sub: 1, 1+1, 2+1, 3+3 -> 1 2 3 6
        let (d, _, _) = run(&[1, 1, 1, 1, 3], &p);
        assert_eq!(d, vec![1, 2, 3, 6]);
        // None then Up: second row adds the first.
        let (d, _, _) = run(&[0, 10, 20, 30, 40, 2, 1, 2, 3, 4], &p);
        assert_eq!(d, vec![10, 20, 30, 40, 11, 22, 33, 44]);
        // None then Average: a=left of the current row, b=above.
        let (d, _, _) = run(&[0, 10, 20, 30, 40, 3, 1, 1, 1, 1], &p);
        // 1+(0+10)/2=6, 1+(6+20)/2=14, 1+(14+30)/2=23, 1+(23+40)/2=32
        assert_eq!(d, vec![10, 20, 30, 40, 6, 14, 23, 32]);
        // None then Paeth.
        let (d, _, _) = run(&[0, 10, 20, 30, 40, 4, 1, 1, 1, 1], &p);
        // paeth(0,10,0)=10 -> 11; paeth(11,20,10)=20 -> 21;
        // paeth(21,30,20)=30 -> 31; paeth(31,40,30)=40 -> 41
        assert_eq!(d, vec![10, 20, 30, 40, 11, 21, 31, 41]);
    }

    #[test]
    fn paeth_picks_the_closest_neighbour() {
        assert_eq!(paeth(0, 0, 0), 0);
        assert_eq!(paeth(10, 20, 30), 10);
        assert_eq!(paeth(200, 100, 150), 150);
        assert_eq!(paeth(1, 2, 3), 1);
    }

    #[test]
    fn png_tag_above_four_is_taken_unfiltered() {
        let p = params(10, 1, 8, 4);
        let (d, c, w) = run(&[9, 1, 2, 3, 4], &p);
        assert_eq!(d, vec![1, 2, 3, 4]);
        assert!(c);
        assert_eq!(w, vec![Warning::BadPredictorTag]);
    }

    #[test]
    fn png_short_final_row_is_kept_and_warned() {
        let p = params(10, 1, 8, 4);
        let (d, c, w) = run(&[0, 1, 2, 3, 4, 1, 5, 5], &p);
        assert_eq!(d, vec![1, 2, 3, 4, 5, 10]);
        assert!(!c);
        assert_eq!(w, vec![Warning::PredictorRowShort]);
    }

    #[test]
    fn png_row_of_only_a_tag_byte_stops() {
        let p = params(10, 1, 8, 4);
        let (d, c, w) = run(&[0, 1, 2, 3, 4, 2], &p);
        assert_eq!(d, vec![1, 2, 3, 4]);
        assert!(!c);
        assert_eq!(w, vec![Warning::PredictorRowShort]);
    }

    #[test]
    fn png_sub_uses_bytes_per_pixel() {
        // colors=3 bpc=8 -> bpp=3, so Sub reaches three bytes back.
        let p = params(11, 3, 8, 2);
        let (d, _, _) = run(&[1, 1, 2, 3, 10, 20, 30], &p);
        assert_eq!(d, vec![1, 2, 3, 11, 22, 33]);
    }

    #[test]
    fn png_sub_byte_components_use_bpp_one() {
        // colors=1 bpc=1 columns=16 -> row 2 bytes, bpp = max(ceil(1/8),1) = 1.
        let p = params(10, 1, 1, 16);
        let (d, _, _) = run(&[1, 0b1010_1010, 0b0000_0001], &p);
        assert_eq!(d, vec![0b1010_1010, 0b1010_1011]);
    }

    #[test]
    fn tiff_bpc8_differences_by_component() {
        let p = params(2, 3, 8, 4);
        let enc = [1, 2, 3, 3, 3, 3, 3, 3, 3, 0xf3, 0xf3, 0xf3];
        let (d, c, w) = run(&enc, &p);
        assert_eq!(d, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 250, 251, 252]);
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn tiff_wraps_at_the_component_width() {
        let p = params(2, 1, 8, 3);
        let (d, _, _) = run(&[200, 100, 1], &p);
        assert_eq!(d, vec![200, 44, 45]);
    }

    #[test]
    fn tiff_bpc16_is_big_endian() {
        let p = params(2, 1, 16, 3);
        let enc = [0x00, 0x10, 0x00, 0x20, 0xff, 0xf0];
        let (d, _, _) = run(&enc, &p);
        assert_eq!(d, vec![0x00, 0x10, 0x00, 0x30, 0x00, 0x20]);
    }

    #[test]
    fn tiff_bpc4_walks_sub_byte_components() {
        // colors=1 bpc=4 columns=6 -> 3 bytes a row.
        let p = params(2, 1, 4, 6);
        let enc = [0x11, 0x1c, 0x17];
        let (d, _, _) = run(&enc, &p);
        // samples 1,1,1,c,1,7 -> 1,2,3,15,0,7
        assert_eq!(d, vec![0x12, 0x3f, 0x07]);
    }

    #[test]
    fn tiff_ignores_padding_past_the_last_column() {
        // colors=1 bpc=4 columns=3 -> 12 bits, padded to 2 bytes. The low
        // nibble of byte 1 is padding and must not be differenced.
        let p = params(2, 1, 4, 3);
        let (d, _, _) = run(&[0x12, 0x3f], &p);
        assert_eq!(d, vec![0x13, 0x6f]);
    }

    #[test]
    fn tiff_short_row_is_kept_and_warned() {
        let p = params(2, 1, 8, 4);
        let (d, c, w) = run(&[1, 1, 1, 1, 2, 2], &p);
        assert_eq!(d, vec![1, 2, 3, 4, 2, 4]);
        assert!(!c);
        assert_eq!(w, vec![Warning::PredictorRowShort]);
    }

    #[test]
    fn output_cap_truncates_a_predictor_run() {
        let p = params(10, 1, 8, 4);
        let limits = Limits::new(6);
        let mut w = Warnings::default();
        let input = [0, 1, 2, 3, 4, 0, 5, 6, 7, 8];
        let (d, c) = unpredict(&input, &p, &limits, &mut w).expect("valid");
        assert_eq!(d, vec![1, 2, 3, 4, 5, 6]);
        assert!(!c);
        assert!(w.into_vec().contains(&Warning::OutputCapHit));
    }

    #[test]
    fn passthrough_respects_the_cap() {
        let limits = Limits::new(2);
        let mut w = Warnings::default();
        let (d, c) = unpredict(b"abcd", &PredictorParams::default(), &limits, &mut w).expect("ok");
        assert_eq!(d, b"ab");
        assert!(!c);
        assert_eq!(w.into_vec(), vec![Warning::OutputCapHit]);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let p = params(10, 3, 8, 4);
        let (d, c, w) = run(b"", &p);
        assert!(d.is_empty());
        assert!(c);
        assert!(w.is_empty());
    }

    #[test]
    fn huge_row_geometry_is_rejected_not_allocated() {
        let p = params(10, u32::MAX, 16, u32::MAX);
        let mut w = Warnings::default();
        let e = unpredict(b"abc", &p, &CAP, &mut w);
        assert!(matches!(e, Err(FilterError::BadParams(_))));
    }
}
