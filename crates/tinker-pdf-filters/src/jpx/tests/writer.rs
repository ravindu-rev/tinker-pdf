//! A test-only JPEG 2000 writer: JP2 boxes and Annex A marker segments.
//!
//! **Nothing ships this.** The engine reads PDFs; it does not write JPEG
//! 2000. It exists for the same reason `mq::encoder` does — so that a test
//! can put a known structure in and demand the decoder's reading of it back,
//! on a small case whose expected answer is derived by arithmetic rather than
//! by comparison with another decoder.
//!
//! It is deliberately *permissive*: [`Spec`] will happily emit a zero
//! `XTsiz`, a `Csiz` that disagrees with `Lsiz`, or a marker Table A.2 does
//! not define, because half the tests here are about what the decoder refuses
//! and a writer that could only emit valid files could not write them.

/// A codestream's main header, as fields rather than bytes.
///
/// The defaults are the smallest thing OpenJPEG will also produce: a 4 x 4
/// greyscale image, one tile, one layer, LRCP, one decomposition level, 64 x
/// 64 code-blocks, the reversible 5/3 transform, no quantisation and two
/// guard bits.
#[derive(Clone, Debug)]
pub(crate) struct Spec {
    pub(crate) rsiz: u16,
    pub(crate) xsiz: u32,
    pub(crate) ysiz: u32,
    pub(crate) xosiz: u32,
    pub(crate) yosiz: u32,
    pub(crate) xtsiz: u32,
    pub(crate) ytsiz: u32,
    pub(crate) xtosiz: u32,
    pub(crate) ytosiz: u32,
    /// `(Ssiz precision, signed, XRsiz, YRsiz)` per component.
    pub(crate) components: Vec<(u8, bool, u8, u8)>,
    /// Overrides `Csiz` independently of `components.len()`, so a SIZ whose
    /// length and component count disagree can be written.
    pub(crate) csiz_override: Option<u16>,
    pub(crate) scod: u8,
    pub(crate) progression: u8,
    pub(crate) layers: u16,
    pub(crate) mct: u8,
    pub(crate) levels: u8,
    /// `xcb - 2`, as the marker carries it.
    pub(crate) xcb: u8,
    pub(crate) ycb: u8,
    pub(crate) cb_style: u8,
    /// 1 for the reversible 5/3, 0 for the irreversible 9/7.
    pub(crate) transform: u8,
    /// One packed `PPy << 4 | PPx` byte per resolution, when `Scod` bit 0 is
    /// set.
    pub(crate) precincts: Vec<u8>,
    pub(crate) guard_bits: u8,
    /// `Sqcd`'s low five bits: 0 none, 1 derived, 2 expounded.
    pub(crate) quant_style: u8,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec {
            rsiz: 0,
            xsiz: 4,
            ysiz: 4,
            xosiz: 0,
            yosiz: 0,
            xtsiz: 4,
            ytsiz: 4,
            xtosiz: 0,
            ytosiz: 0,
            components: vec![(8, false, 1, 1)],
            csiz_override: None,
            scod: 0,
            progression: 0,
            layers: 1,
            mct: 0,
            levels: 1,
            xcb: 4,
            ycb: 4,
            cb_style: 0,
            transform: 1,
            precincts: Vec::new(),
            guard_bits: 2,
            quant_style: 0,
        }
    }
}

impl Spec {
    /// Subbands in one component: `3 * levels + 1`, which is how many step
    /// sizes a QCD carries.
    pub(crate) fn subbands(&self) -> usize {
        3 * usize::from(self.levels) + 1
    }

    /// SIZ (A.5.1).
    pub(crate) fn siz(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&self.rsiz.to_be_bytes());
        for v in [
            self.xsiz,
            self.ysiz,
            self.xosiz,
            self.yosiz,
            self.xtsiz,
            self.ytsiz,
            self.xtosiz,
            self.ytosiz,
        ] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        let csiz = self.csiz_override.unwrap_or(self.components.len() as u16);
        body.extend_from_slice(&csiz.to_be_bytes());
        for &(precision, signed, dx, dy) in &self.components {
            body.push((precision.wrapping_sub(1) & 0x7F) | if signed { 0x80 } else { 0 });
            body.push(dx);
            body.push(dy);
        }
        segment(super::super::codestream::marker::SIZ, &body)
    }

    /// The SPcod/SPcoc tail both COD and COC carry (Table A.15).
    fn style(&self) -> Vec<u8> {
        let mut body = vec![
            self.levels,
            self.xcb,
            self.ycb,
            self.cb_style,
            self.transform,
        ];
        if self.scod & 0x01 != 0 {
            body.extend_from_slice(&self.precincts);
        }
        body
    }

    /// COD (A.6.1).
    pub(crate) fn cod(&self) -> Vec<u8> {
        let mut body = vec![self.scod, self.progression];
        body.extend_from_slice(&self.layers.to_be_bytes());
        body.push(self.mct);
        body.extend_from_slice(&self.style());
        segment(super::super::codestream::marker::COD, &body)
    }

    /// QCD (A.6.4), with a plausible exponent per subband.
    pub(crate) fn qcd(&self) -> Vec<u8> {
        let mut body = vec![(self.guard_bits << 5) | self.quant_style];
        for _ in 0..self.subbands() {
            if self.quant_style == 0 {
                body.push(8 << 3);
            } else {
                body.extend_from_slice(&(8u16 << 11).to_be_bytes());
            }
        }
        segment(super::super::codestream::marker::QCD, &body)
    }

    /// SOC, SIZ, COD, QCD — a complete main header.
    pub(crate) fn main_header(&self) -> Vec<u8> {
        let mut out = super::super::codestream::marker::SOC.to_be_bytes().to_vec();
        out.extend_from_slice(&self.siz());
        out.extend_from_slice(&self.cod());
        out.extend_from_slice(&self.qcd());
        out
    }

    /// The tile count A.5.1's equation A-4 gives, or 1 for a grid too broken
    /// to divide.
    fn tile_count(&self) -> u32 {
        if self.xtsiz == 0 || self.ytsiz == 0 || self.xsiz < self.xtosiz || self.ysiz < self.ytosiz
        {
            return 1;
        }
        (self.xsiz - self.xtosiz)
            .div_ceil(self.xtsiz)
            .saturating_mul((self.ysiz - self.ytosiz).div_ceil(self.ytsiz))
            .clamp(1, 4096)
    }

    /// A whole codestream, with one tile-part per entry of `tiles` carrying
    /// that entry's bytes as its packet data.
    ///
    /// An empty `tiles` means one empty tile-part for **every** tile in the
    /// grid, because A.4.2 requires every tile to be coded and the decoder
    /// refuses a tile that no part covers — so "a valid header and nothing
    /// else" is not a valid codestream, and a writer whose default produced
    /// one would make every header test fail for the wrong reason.
    pub(crate) fn codestream(&self, tiles: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = self.main_header();
        if tiles.is_empty() {
            for t in 0..self.tile_count() {
                out.extend_from_slice(&tile_part(t as u16, 0, 1, &[], &[]));
            }
        } else {
            for &(tile, data) in tiles {
                out.extend_from_slice(&tile_part(tile, 0, 1, &[], data));
            }
        }
        out.extend_from_slice(&super::super::codestream::marker::EOC.to_be_bytes());
        out
    }

    /// The same, wrapped in the JP2 boxes of Annex I.
    pub(crate) fn jp2(&self, tiles: &[(u16, &[u8])]) -> Vec<u8> {
        let codestream = self.codestream(tiles);
        let mut out = SIGNATURE.to_vec();
        out.extend_from_slice(&boxed(*b"ftyp", b"jp2 \x00\x00\x00\x00jp2 "));

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&self.ysiz.to_be_bytes());
        ihdr.extend_from_slice(&self.xsiz.to_be_bytes());
        ihdr.extend_from_slice(&(self.components.len() as u16).to_be_bytes());
        ihdr.push(self.components.first().map_or(7, |c| c.0 - 1));
        ihdr.extend_from_slice(&[7, 0, 0]);

        let colr = [1u8, 0, 0, 0, 0, 0, 17];
        let mut jp2h = boxed(*b"ihdr", &ihdr);
        jp2h.extend_from_slice(&boxed(*b"colr", &colr));

        out.extend_from_slice(&boxed(*b"jp2h", &jp2h));
        out.extend_from_slice(&boxed(*b"jp2c", &codestream));
        out
    }
}

/// The twelve bytes a JP2 file opens with (I.5.1).
pub(crate) const SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];

/// One JP2 box: a 32-bit length including the eight-byte header, the type,
/// then the contents.
pub(crate) fn boxed(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&kind);
    out.extend_from_slice(body);
    out
}

/// One marker segment: the marker, a length that includes itself, the body.
pub(crate) fn segment(code: u16, body: &[u8]) -> Vec<u8> {
    let mut out = code.to_be_bytes().to_vec();
    out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// SOT, any extra header markers, SOD, and the tile-part's data (A.4.2).
///
/// `Psot` is filled in from the finished length, which is what a conformant
/// writer does and what makes a *wrong* `Psot` something a test has to write
/// deliberately.
pub(crate) fn tile_part(tile: u16, index: u8, parts: u8, header: &[u8], data: &[u8]) -> Vec<u8> {
    let mut sot_body = tile.to_be_bytes().to_vec();
    sot_body.extend_from_slice(&0u32.to_be_bytes());
    sot_body.push(index);
    sot_body.push(parts);
    let mut out = segment(super::super::codestream::marker::SOT, &sot_body);
    out.extend_from_slice(header);
    out.extend_from_slice(&super::super::codestream::marker::SOD.to_be_bytes());
    out.extend_from_slice(data);
    let psot = out.len() as u32;
    out[6..10].copy_from_slice(&psot.to_be_bytes());
    out
}
