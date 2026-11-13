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

/// A.7.4's PPM segment: `Zppm`, then whatever follows it.
///
/// The tail is taken raw rather than as a list of runs, because A.7.4's
/// continuation rule is the thing most worth testing: "the series of Ippm
/// parameters described by the Nppm does not have to be complete in a given
/// marker segment", so a test has to be able to cut the series anywhere.
pub(crate) fn ppm(zppm: u8, tail: &[u8]) -> Vec<u8> {
    let mut body = vec![zppm];
    body.extend_from_slice(tail);
    segment(super::super::codestream::marker::PPM, &body)
}

/// One `(Nppm_i, Ippm_i)` pair of A.7.4's series.
pub(crate) fn nppm_run(ippm: &[u8]) -> Vec<u8> {
    let mut out = (ippm.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(ippm);
    out
}

/// A.7.5's PPT segment: `Zppt`, then `Ippt`.
pub(crate) fn ppt(zppt: u8, ippt: &[u8]) -> Vec<u8> {
    let mut body = vec![zppt];
    body.extend_from_slice(ippt);
    segment(super::super::codestream::marker::PPT, &body)
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

// --- whole codestreams, from chosen coefficients ---------------------------

/// Where one coefficient sits: its component, its resolution counted from
/// the bottom (so a codestream with fewer levels numbers the bands it shares
/// with one of more the same way), its subband's orientation, and its
/// position in the subband's own coordinates (B.5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct At {
    pub(crate) component: usize,
    pub(crate) resolution: usize,
    pub(crate) orientation: crate::jpx::tier2::Orientation,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

/// One code-block as the writer coded it: its bytes, its passes and the
/// bit-planes it is missing below `Mb`; `None` for a block of zeros.
type Coded = Option<(Vec<u8>, u32, u32)>;

/// `spec`'s codestream with every code-block carrying the coefficients
/// `value` names: each block coded by tier-1's test encoder to the bottom
/// plane, its packet headers written for one layer in the order the decoder
/// reads them, and a block whose coefficients are all zero left out of its
/// packet.
///
/// The geometry and the packet order are the decoder's own
/// (`tier2::layout_for_test`), so this writes what the decoder will look
/// for. What it is for is holding two paths of the decoder to each other —
/// a reduced decode against the full decode of a codestream that carries
/// only the bands it keeps — not adjudicating either. Every magnitude must
/// fit the `Mb` the writer's QCD gives a subband: `guard_bits + 7` planes.
pub(crate) fn coded(spec: &Spec, value: &dyn Fn(At) -> i32) -> Vec<u8> {
    use crate::jpx::codestream::parse;
    use crate::jpx::tier2::layout_for_test;

    let empty = spec.codestream(&[]);
    let stream = parse(&empty).expect("the writer's headers parse");
    let mb = u32::from(spec.guard_bits) + 8 - 1;
    let mut parts: Vec<(u16, Vec<u8>)> = Vec::new();
    for t in 0..spec.tile_count() {
        let (tile, order) = layout_for_test(&stream, t).expect("the writer's tile lays out");
        let blocks = code_tile(&tile, value, mb);
        let mut trees = tag_trees(&tile, &blocks);
        let mut data = Vec::new();
        for (layer, r, c, p) in order {
            assert_eq!(layer, 0, "the writer writes one layer");
            let (r, c, p) = (usize::from(r), usize::from(c), p as usize);
            write_packet(&tile, &blocks, &mut trees, (c, r, p), &mut data);
        }
        parts.push((u16::try_from(t).expect("a tile index"), data));
    }
    let tiles: Vec<(u16, &[u8])> = parts.iter().map(|(t, data)| (*t, &data[..])).collect();
    spec.codestream(&tiles)
}

/// Every code-block of `tile` coded, by `[component][resolution][band]
/// [precinct][block]`.
fn code_tile(
    tile: &crate::jpx::tier2::Tile,
    value: &dyn Fn(At) -> i32,
    mb: u32,
) -> Vec<Vec<Vec<Vec<Vec<Coded>>>>> {
    use crate::jpx::tier1::{encoder::encode_code_block, CodingStyle};

    let mut out = Vec::new();
    for (c, component) in tile.components.iter().enumerate() {
        let mut by_resolution = Vec::new();
        for (r, resolution) in component.resolutions.iter().enumerate() {
            let mut by_band = Vec::new();
            for band in &resolution.bands {
                let mut by_precinct = Vec::new();
                for precinct in &band.precincts {
                    let mut blocks = Vec::new();
                    for block in &precinct.blocks {
                        let (w, h) = (block.width() as usize, block.height() as usize);
                        let mut values = Vec::with_capacity(w * h);
                        for y in block.y0..block.y1 {
                            for x in block.x0..block.x1 {
                                values.push(value(At {
                                    component: c,
                                    resolution: r,
                                    orientation: band.orientation,
                                    x,
                                    y,
                                }));
                            }
                        }
                        let largest = values.iter().map(|v| v.unsigned_abs()).max();
                        let planes = largest.map_or(0, |m| 32 - m.leading_zeros());
                        assert!(planes <= mb, "a coefficient past the writer's Mb");
                        blocks.push((planes > 0).then(|| {
                            let (bytes, passes) = encode_code_block(
                                &values,
                                w,
                                h,
                                planes,
                                band.orientation,
                                CodingStyle::default(),
                            );
                            (bytes, passes, mb - planes)
                        }));
                    }
                    by_precinct.push(blocks);
                }
                by_band.push(by_precinct);
            }
            by_resolution.push(by_band);
        }
        out.push(by_resolution);
    }
    out
}

/// Each precinct band's inclusion and zero bit-plane trees. One layer, so an
/// included block's inclusion leaf is 0 and an empty block's is 1; the trees
/// keep their state across the packets they are read in, as the decoder's
/// do.
fn tag_trees(
    tile: &crate::jpx::tier2::Tile,
    blocks: &[Vec<Vec<Vec<Vec<Coded>>>>],
) -> Vec<Vec<Vec<Vec<(TagWriter, TagWriter)>>>> {
    let mut out = Vec::new();
    for (c, component) in tile.components.iter().enumerate() {
        let mut by_resolution = Vec::new();
        for (r, resolution) in component.resolutions.iter().enumerate() {
            let mut by_band = Vec::new();
            for (b, band) in resolution.bands.iter().enumerate() {
                let mut by_precinct = Vec::new();
                for (p, precinct) in band.precincts.iter().enumerate() {
                    let coded = &blocks[c][r][b][p];
                    let inclusion: Vec<u32> =
                        coded.iter().map(|k| u32::from(k.is_none())).collect();
                    let zero: Vec<u32> = coded
                        .iter()
                        .map(|k| k.as_ref().map_or(0, |k| k.2))
                        .collect();
                    by_precinct.push((
                        TagWriter::new(precinct.w, precinct.h, &inclusion),
                        TagWriter::new(precinct.w, precinct.h, &zero),
                    ));
                }
                by_band.push(by_precinct);
            }
            by_resolution.push(by_band);
        }
        out.push(by_resolution);
    }
    out
}

/// The packet of component `c`, resolution `r` and precinct `p`: its header,
/// then the bodies in the order the header names them.
fn write_packet(
    tile: &crate::jpx::tier2::Tile,
    blocks: &[Vec<Vec<Vec<Vec<Coded>>>>],
    trees: &mut [Vec<Vec<Vec<(TagWriter, TagWriter)>>>],
    (c, r, p): (usize, usize, usize),
    data: &mut Vec<u8>,
) {
    let resolution = &tile.components[c].resolutions[r];
    let mut header = HeaderBits::default();
    let mut bodies = Vec::new();
    let any = resolution.bands.iter().enumerate().any(|(b, band)| {
        !band.is_empty()
            && blocks[c][r][b]
                .get(p)
                .is_some_and(|coded| coded.iter().any(Option::is_some))
    });
    header.bit(u8::from(any));
    if any {
        for (b, band) in resolution.bands.iter().enumerate() {
            if band.is_empty() {
                continue;
            }
            let w = band.precincts[p].w.max(1);
            let (inclusion, zero) = &mut trees[c][r][b][p];
            for (i, coded) in blocks[c][r][b][p].iter().enumerate() {
                let (gx, gy) = (i as u32 % w, i as u32 / w);
                inclusion.encode(&mut header, gx, gy, 1);
                let Some((bytes, passes, missing)) = coded else {
                    continue;
                };
                let mut threshold = 1;
                loop {
                    zero.encode(&mut header, gx, gy, threshold);
                    if *missing < threshold {
                        break;
                    }
                    threshold += 1;
                }
                header.pass_count(*passes);
                header.length(bytes.len(), *passes);
                bodies.extend_from_slice(bytes);
            }
        }
    }
    data.extend_from_slice(&header.finish());
    data.extend_from_slice(&bodies);
}

/// A packet header's bits, with B.10.1's stuffing: after a `0xFF` the next
/// byte carries seven bits, and a header ending on a `0xFF` is followed by
/// the byte the decoder skips.
#[derive(Default)]
struct HeaderBits {
    out: Vec<u8>,
    acc: u8,
    filled: u32,
}

impl HeaderBits {
    fn capacity(&self) -> u32 {
        if self.out.last() == Some(&0xFF) {
            7
        } else {
            8
        }
    }

    fn bit(&mut self, b: u8) {
        self.acc = (self.acc << 1) | (b & 1);
        self.filled += 1;
        if self.filled == self.capacity() {
            self.out.push(self.acc);
            self.acc = 0;
            self.filled = 0;
        }
    }

    fn bits(&mut self, value: u32, n: u32) {
        for k in (0..n).rev() {
            self.bit(((value >> k) & 1) as u8);
        }
    }

    /// B.10.6's code for a number of coding passes.
    fn pass_count(&mut self, passes: u32) {
        match passes {
            1 => self.bit(0),
            2 => self.bits(0b10, 2),
            3..=5 => {
                self.bits(0b11, 2);
                self.bits(passes - 3, 2);
            }
            6..=36 => {
                self.bits(0b1111, 4);
                self.bits(passes - 6, 5);
            }
            _ => {
                self.bits(0b1111, 4);
                self.bits(0b11111, 5);
                self.bits(passes - 37, 7);
            }
        }
    }

    /// B.10.7.1: `Lblock`, starting at three, raised by as many signalling
    /// ones as the length needs, then the length in `Lblock + floor(log2
    /// passes)` bits. One codeword segment, since the writer's code-blocks
    /// use no style that terminates a pass early.
    fn length(&mut self, length: usize, passes: u32) {
        let extra = 31 - passes.leading_zeros();
        let needed = usize::BITS - length.leading_zeros();
        let lblock = 3u32.max(needed.saturating_sub(extra));
        for _ in 3..lblock {
            self.bit(1);
        }
        self.bit(0);
        self.bits(
            u32::try_from(length).expect("a segment length"),
            lblock + extra,
        );
    }

    fn finish(mut self) -> Vec<u8> {
        if self.filled > 0 {
            self.acc <<= self.capacity() - self.filled;
            self.out.push(self.acc);
        }
        if self.out.last() == Some(&0xFF) {
            self.out.push(0);
        }
        self.out
    }
}

/// B.10.2's tag tree, from the writing side: the leaves' values known, and
/// each node's state carried across calls as the decoder carries it across
/// packets, so asking it to code a leaf against a threshold writes exactly
/// the bits `TagTree::decode` reads for that question.
struct TagWriter {
    /// `(width, offset)` a level, the leaves first.
    levels: Vec<(u32, usize)>,
    /// Each node's value (its subtree's minimum), the lowest value not yet
    /// ruled out, and whether the value has been sent.
    nodes: Vec<(u32, u32, bool)>,
}

impl TagWriter {
    fn new(w: u32, h: u32, leaves: &[u32]) -> TagWriter {
        let mut levels = Vec::new();
        let mut values: Vec<u32> = leaves.to_vec();
        let mut all = Vec::new();
        let (mut lw, mut lh) = (w, h);
        while lw > 0 && lh > 0 {
            levels.push((lw, all.len()));
            all.extend_from_slice(&values);
            if lw == 1 && lh == 1 {
                break;
            }
            let (nw, nh) = (lw.div_ceil(2), lh.div_ceil(2));
            let mut next = vec![u32::MAX; (nw as usize) * (nh as usize)];
            for y in 0..lh {
                for x in 0..lw {
                    let v = values[(y as usize) * (lw as usize) + (x as usize)];
                    let slot = &mut next[((y / 2) as usize) * (nw as usize) + ((x / 2) as usize)];
                    *slot = (*slot).min(v);
                }
            }
            values = next;
            lw = nw;
            lh = nh;
        }
        TagWriter {
            levels,
            nodes: all.into_iter().map(|v| (v, 0, false)).collect(),
        }
    }

    fn encode(&mut self, out: &mut HeaderBits, x: u32, y: u32, threshold: u32) {
        let mut low = 0u32;
        for level in (0..self.levels.len()).rev() {
            let (lw, offset) = self.levels[level];
            let idx = offset + ((y >> level) as usize) * (lw as usize) + ((x >> level) as usize);
            let node = &mut self.nodes[idx];
            if node.1 < low {
                node.1 = low;
            }
            while node.1 < threshold && !node.2 {
                if node.1 < node.0 {
                    out.bit(0);
                    node.1 += 1;
                } else {
                    out.bit(1);
                    node.2 = true;
                }
            }
            low = node.1;
        }
    }
}
