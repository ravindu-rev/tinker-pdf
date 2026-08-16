//! Tier-2: the packet headers (ITU-T T.800 Annex B).
//!
//! Tier-2 is the layer that says *which* code-block's bytes are where. It
//! carries no image data of its own, which is exactly what makes it dangerous
//! (see [`super`]): it hands tier-1 a byte range, and a byte range that is
//! wrong by a few bytes still decodes into coefficients, and coefficients
//! still go through the inverse wavelet, and the inverse wavelet smooths them
//! into a photograph.
//!
//! Four things live here.
//!
//! **The geometry** (B.5 to B.7). A tile-component splits into resolutions,
//! each resolution into one or three subbands, each subband into precincts,
//! each precinct into code-blocks. Every one of those partitions is anchored
//! at the origin of the *reference grid* rather than at the tile, which is
//! why the first code-block of a tile is usually a partial one and why every
//! bound below is a `ceil` of a difference rather than a count.
//!
//! **The tag tree** (B.10.2), which is how inclusion and the zero bit-plane
//! count are coded: a quadtree of minima, decoded against a rising threshold,
//! where a node's bits are read once across every packet that ever asks about
//! it. That last property is why the trees are *state* rather than locals.
//!
//! **The packet header** (B.10), read through a bit reader with T.800's own
//! stuffing rule: after a `0xFF` byte the next byte carries seven bits, so a
//! header can never accidentally contain a marker.
//!
//! **The progression orders** (B.12), all five. A decoder that implements one
//! and defaults the rest reads every packet of an RPCL stream in LRCP order —
//! all the packets are there, none is missing, and the picture is wrong.
//!
//! # The integrity check
//!
//! A packet whose declared length does not land where the next packet begins
//! refuses the tile. It is one of the two cheapest real defences this plan
//! has, because a tier-2 parser that has gone wrong almost never lands on a
//! packet boundary by accident — and it catches the mis-parse *before any
//! pixel exists*. Concretely: with SOP signalled every packet must begin with
//! one; with EPH signalled every header must end with one; and in all cases
//! the packets of a tile must consume its data exactly.

use super::codestream::{Codestream, CodingStyle, Progression};
use super::{Refusal, MAX_JPX_CODE_BLOCKS};

/// Subband orientation (T.800 Table E.1's `b`), which decides both the
/// nominal dynamic-range gain and which of Table D.1's three context
/// mappings tier-1 uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Orientation {
    Ll,
    Hl,
    Lh,
    Hh,
}

impl Orientation {
    /// `(xob, yob)` from B.5's Table B.1 — which half of each axis this
    /// subband occupies.
    const fn offsets(self) -> (i64, i64) {
        match self {
            Self::Ll => (0, 0),
            Self::Hl => (1, 0),
            Self::Lh => (0, 1),
            Self::Hh => (1, 1),
        }
    }
}

/// One code-block, and everything tier-2 learned about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CodeBlock {
    /// Bounds in the subband's own coordinates.
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) x1: u32,
    pub(crate) y1: u32,
    /// B.10.5's "number of missing bit-planes": how many of the band's most
    /// significant magnitude bits are known to be zero everywhere here.
    pub(crate) zero_planes: u8,
    /// Coding passes accumulated over every layer that included this block.
    pub(crate) passes: u32,
    /// The MQ codeword segment, concatenated across layers. One segment,
    /// because the code-block styles that split it — TERMALL and BYPASS —
    /// are refused in the header parser.
    pub(crate) data: Vec<u8>,
    /// Tier-1's output: signed coefficients in scan order, whose magnitudes
    /// occupy [`CodeBlock::planes`] bits. Empty until tier-1 has run.
    pub(crate) coefficients: Vec<i32>,
    /// Per coefficient, the lowest bit-plane whose magnitude bit tier-1
    /// actually decoded — which on a truncated stream is **not** the same for
    /// every coefficient of the block, and is what E.1.1.2 reconstructs the
    /// midpoint of.
    pub(crate) half_planes: Vec<u8>,
    /// How many magnitude bit-planes tier-1 decoded, which is what tells
    /// milestone 4 how far to shift them: `Mb - zero_planes - planes` is the
    /// distance a truncated stream left at the bottom.
    pub(crate) planes: u8,
    /// B.10.4: whether an earlier layer already included this block, which
    /// changes how inclusion is coded from a tag tree to one bit.
    included: bool,
    /// B.10.7's `Lblock`, which grows by the signalled number of 1-bits and
    /// never shrinks.
    lblock: u32,
}

impl CodeBlock {
    // Tier-1 reads these: it decodes a code-block bit-plane by bit-plane and
    // needs its extent to know where D.2's stripe scan ends. Kept here rather
    // than recomputed there, because the geometry that produced them is B.7's
    // and lives in this module.
    pub(crate) const fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    pub(crate) const fn height(&self) -> u32 {
        self.y1 - self.y0
    }
}

/// One precinct's worth of one subband: its code-blocks and the two tag trees
/// that address them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrecinctBand {
    pub(crate) blocks: Vec<CodeBlock>,
    /// Code-blocks across and down. The tag trees are over this grid, so a
    /// precinct with no code-blocks has trees with no nodes.
    w: u32,
    h: u32,
    /// B.10.4's inclusion tree.
    inclusion: TagTree,
    /// B.10.5's zero-bit-plane tree.
    zero_planes: TagTree,
}

/// One subband of one resolution (B.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Subband {
    pub(crate) orientation: Orientation,
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) x1: u32,
    pub(crate) y1: u32,
    /// The decomposition level this subband belongs to, counting from the
    /// coarsest. E.1 needs it for the step size and F.3 for the lifting.
    pub(crate) level: u8,
    pub(crate) precincts: Vec<PrecinctBand>,
}

impl Subband {
    pub(crate) const fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
}

/// One resolution of one tile-component (B.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resolution {
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) x1: u32,
    pub(crate) y1: u32,
    /// Precincts across and down (B.6).
    pub(crate) pw: u32,
    pub(crate) ph: u32,
    /// `(PPx, PPy)` in force here.
    pub(crate) ppx: u8,
    pub(crate) ppy: u8,
    pub(crate) bands: Vec<Subband>,
}

/// One tile-component, resolution 0 first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TileComponent {
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) x1: u32,
    pub(crate) y1: u32,
    pub(crate) resolutions: Vec<Resolution>,
}

/// One tile: its components, in SIZ order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tile {
    pub(crate) index: u16,
    pub(crate) components: Vec<TileComponent>,
}

// --- the tag tree (B.10.2) ----------------------------------------------

/// A node's value before any bit has determined it. Larger than any threshold
/// this decoder will use, since a threshold is bounded by the layer count
/// (65 535) and by the bit-plane count.
const UNKNOWN: u32 = 1 << 24;

/// T.800 B.10.2's tag tree: a quadtree over a `w` by `h` grid of leaves, in
/// which every node holds the minimum of its children.
///
/// The decoding is against a *threshold*, and it is incremental: a node's
/// bits are read once, by whichever packet first pushes the threshold past
/// them, and every later packet picks up where that one left off. That is why
/// this is state on the precinct rather than a local — and why decoding a
/// node "at the wrong level" is a defect that stays silent, since the bits
/// still come out of the stream in some order and the packet still ends where
/// it said it would.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TagTree {
    /// `(value, low)` per node, leaves first, then each coarser level.
    nodes: Vec<(u32, u32)>,
    /// `(width, height, offset)` per level, level 0 being the leaves.
    levels: Vec<(u32, u32, usize)>,
}

impl TagTree {
    pub(crate) fn new(w: u32, h: u32) -> TagTree {
        let mut levels = Vec::new();
        let mut offset = 0usize;
        let (mut lw, mut lh) = (w, h);
        while lw > 0 && lh > 0 {
            levels.push((lw, lh, offset));
            offset += (lw as usize) * (lh as usize);
            if lw == 1 && lh == 1 {
                break;
            }
            lw = lw.div_ceil(2);
            lh = lh.div_ceil(2);
        }
        TagTree {
            nodes: vec![(UNKNOWN, 0); offset],
            levels,
        }
    }

    /// The node index of `(x, y)` at `level`.
    fn at(&self, level: usize, x: u32, y: u32) -> Option<usize> {
        let &(lw, lh, offset) = self.levels.get(level)?;
        (x < lw && y < lh).then(|| offset + (y as usize) * (lw as usize) + (x as usize))
    }

    /// B.10.2's decoding procedure: is leaf `(x, y)`'s value below
    /// `threshold`?
    ///
    /// The walk is **root to leaf**, and the running `low` carried down it is
    /// the whole point of a tag tree: a child can never be smaller than its
    /// parent, so the bits already spent on the parent are bits the child
    /// does not spend again.
    pub(crate) fn decode(
        &mut self,
        bits: &mut PacketBits<'_>,
        x: u32,
        y: u32,
        threshold: u32,
    ) -> Result<bool, Refusal> {
        if self.levels.is_empty() {
            return Err(Refusal::Structure("a tag tree with no leaves"));
        }
        let mut low = 0u32;
        let mut last = 0usize;
        for level in (0..self.levels.len()).rev() {
            let shift = level as u32;
            let idx = self
                .at(level, x >> shift, y >> shift)
                .ok_or(Refusal::Structure("a tag tree leaf outside its grid"))?;
            last = idx;
            if self.nodes[idx].1 < low {
                self.nodes[idx].1 = low;
            }
            while self.nodes[idx].1 < threshold && self.nodes[idx].1 < self.nodes[idx].0 {
                if bits.bit()? == 1 {
                    self.nodes[idx].0 = self.nodes[idx].1;
                } else {
                    self.nodes[idx].1 += 1;
                }
            }
            low = self.nodes[idx].1;
        }
        Ok(self.nodes[last].0 < threshold)
    }

    /// The leaf's value, once [`TagTree::decode`] has returned true for it.
    pub(crate) fn value(&self, x: u32, y: u32) -> u32 {
        self.at(0, x, y).map_or(UNKNOWN, |i| self.nodes[i].0)
    }

    /// The coarsest node's value: the minimum over every leaf.
    #[cfg(test)]
    ///
    /// Resolving any one leaf resolves every node on its path to the root, so
    /// this is known long before the tree is. It is the assertion a wrong
    /// *level* fails first, because a tree read one level out still hands
    /// back small plausible integers for the leaves themselves.
    pub(crate) fn root_value(&self) -> u32 {
        self.levels
            .len()
            .checked_sub(1)
            .and_then(|top| self.at(top, 0, 0))
            .map_or(UNKNOWN, |i| self.nodes[i].0)
    }
}

/// Encodes a rectangular grid of leaf values as B.10.2 codes them.
///
/// Test-only, and the mirror of [`TagTree::decode`] rather than an
/// independent implementation -- which is a limitation worth stating plainly:
/// a round trip through this cannot catch a defect the two share. It exists
/// so the *standard's own* worked example can be fed to the decoder, and the
/// numbers in that example are the evidence. Nothing in the shipped surface
/// writes JPEG 2000.
#[cfg(test)]
pub(crate) fn encode_tag_tree_for_test<R: AsRef<[u32]>>(rows: &[R]) -> Vec<u8> {
    let rows: Vec<Vec<u32>> = rows.iter().map(|r| r.as_ref().to_vec()).collect();
    encode_grid_for_test(&rows)
}

/// The general shape, over a `Vec` of rows.
#[cfg(test)]
fn encode_grid_for_test(rows: &[Vec<u32>]) -> Vec<u8> {
    let h = rows.len() as u32;
    let w = rows.first().map_or(0, |r| r.len()) as u32;

    // Build the quadtree of minima, the same shape `TagTree::new` builds.
    let mut levels: Vec<(u32, u32, Vec<u32>)> = Vec::new();
    let (mut lw, mut lh) = (w, h);
    let mut current: Vec<u32> = rows.iter().flatten().copied().collect();
    loop {
        levels.push((lw, lh, current.clone()));
        if lw == 1 && lh == 1 {
            break;
        }
        let (nw, nh) = (lw.div_ceil(2), lh.div_ceil(2));
        let mut next = vec![u32::MAX; (nw as usize) * (nh as usize)];
        for y in 0..lh {
            for x in 0..lw {
                let v = current[(y as usize) * (lw as usize) + (x as usize)];
                let slot = &mut next[((y / 2) as usize) * (nw as usize) + ((x / 2) as usize)];
                *slot = (*slot).min(v);
            }
        }
        current = next;
        lw = nw;
        lh = nh;
    }

    // Emit, leaf by leaf, root to leaf, carrying `low` down exactly as the
    // decoder does and remembering which nodes have already been resolved.
    let mut out = BitWriter::default();
    let mut state: Vec<Vec<(u32, bool)>> = levels
        .iter()
        .map(|(_, _, vals)| vals.iter().map(|_| (0u32, false)).collect())
        .collect();

    for y in 0..h {
        for x in 0..w {
            let mut low = 0u32;
            for level in (0..levels.len()).rev() {
                let shift = level as u32;
                let (lw, _, vals) = &levels[level];
                let idx = ((y >> shift) as usize) * (*lw as usize) + ((x >> shift) as usize);
                let value = vals[idx];
                let (ref mut node_low, ref mut known) = state[level][idx];
                if *node_low < low {
                    *node_low = low;
                }
                while !*known {
                    if *node_low < value {
                        out.bit(0);
                        *node_low += 1;
                    } else {
                        out.bit(1);
                        *known = true;
                    }
                }
                low = *node_low;
            }
        }
    }
    out.finish()
}

/// A bit writer with B.10.1's stuffing rule, so what it emits is what
/// [`PacketBits`] expects to read.
#[cfg(test)]
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u8,
    filled: u32,
}

#[cfg(test)]
impl BitWriter {
    fn bit(&mut self, b: u8) {
        // After a 0xFF the next byte carries seven bits, so the accumulator
        // fills to seven rather than eight.
        let capacity = if self.out.last() == Some(&0xFF) { 7 } else { 8 };
        self.acc = (self.acc << 1) | (b & 1);
        self.filled += 1;
        if self.filled == capacity {
            self.out.push(self.acc);
            self.acc = 0;
            self.filled = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.filled > 0 {
            let capacity = if self.out.last() == Some(&0xFF) { 7 } else { 8 };
            self.acc <<= capacity - self.filled;
            self.out.push(self.acc);
        }
        self.out
    }
}

// --- the packet header bit reader (B.10.1) ------------------------------

/// A bit reader over a packet header, with T.800 B.10.1's stuffing rule.
///
/// After a byte equal to `0xFF` the next byte carries only **seven** bits:
/// its top bit is not part of the header. That is what keeps a packet header
/// from ever containing a two-byte marker by accident, and it is also why a
/// packet header cannot be measured by counting bits — the byte count is what
/// the caller needs and what [`PacketBits::consumed`] gives.
///
/// Running out of data is a refusal rather than a stream of ones. A decoder
/// that reads past the end of a packet header gets *plausible* code-block
/// lengths out of it, which is the failure this whole module is written
/// against.
pub(crate) struct PacketBits<'a> {
    data: &'a [u8],
    at: usize,
    buf: u8,
    ct: u32,
}

impl<'a> PacketBits<'a> {
    pub(crate) fn new(data: &'a [u8]) -> PacketBits<'a> {
        PacketBits {
            data,
            at: 0,
            buf: 0,
            ct: 0,
        }
    }

    pub(crate) fn bit(&mut self) -> Result<u8, Refusal> {
        if self.ct == 0 {
            let next = *self
                .data
                .get(self.at)
                .ok_or(Refusal::Truncated("a packet header"))?;
            self.at += 1;
            // The stuffing rule, and the only place it appears.
            self.ct = if self.buf == 0xFF { 7 } else { 8 };
            self.buf = next;
        }
        self.ct -= 1;
        Ok((self.buf >> self.ct) & 1)
    }

    fn bits(&mut self, n: u32) -> Result<u32, Refusal> {
        if n > 32 {
            return Err(Refusal::Structure(
                "a packet header field wider than 32 bits",
            ));
        }
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | u32::from(self.bit()?);
        }
        Ok(v)
    }

    /// B.10.1: the header ends on a byte boundary, and if its last byte was
    /// `0xFF` the stuffed byte after it belongs to the header too.
    fn align(&mut self) -> Result<(), Refusal> {
        if self.buf == 0xFF {
            // The stuffed byte is present even when no bit of it was used.
            if self.at >= self.data.len() {
                return Err(Refusal::Truncated("a packet header's stuffed byte"));
            }
            self.at += 1;
        }
        self.ct = 0;
        self.buf = 0;
        Ok(())
    }

    fn consumed(&self) -> usize {
        self.at
    }
}

// --- geometry (B.5 to B.7) ----------------------------------------------

/// `ceil(a / b)` for a positive `b`, correct for a negative `a`.
///
/// B.5's subband bounds subtract half a subband before dividing, so the
/// numerator genuinely goes negative for a tile-component that starts near
/// the origin — and `(-3) / 4` in Rust is 0 by truncation, which is the right
/// answer here only by accident and the wrong one for `-5 / 4`.
const fn ceil_div(a: i64, b: i64) -> i64 {
    (a + b - 1).div_euclid(b)
}

const fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}

/// Builds every tile's geometry and reads every packet.
pub(crate) fn decode_tiles(stream: &Codestream<'_>) -> Result<Vec<Tile>, Refusal> {
    let count = u64::from(stream.siz.tiles_x) * u64::from(stream.siz.tiles_y);
    let mut budget = MAX_JPX_CODE_BLOCKS;
    let mut tiles = Vec::new();
    for t in 0..count {
        let t = u32::try_from(t).map_err(|_| Refusal::Budget("tiles"))?;
        let index = u16::try_from(t).map_err(|_| Refusal::Budget("tiles"))?;
        let mut tile = build_tile(stream, t, index, &mut budget)?;
        read_tile_packets(stream, &mut tile, &mut budget)?;
        tiles.push(tile);
    }
    Ok(tiles)
}

/// B.5 to B.7 for one tile: resolutions, subbands, precincts, code-blocks.
fn build_tile(
    stream: &Codestream<'_>,
    t: u32,
    index: u16,
    budget: &mut u64,
) -> Result<Tile, Refusal> {
    let mut components = Vec::with_capacity(stream.siz.components.len());
    for c in 0..stream.siz.components.len() {
        let style = stream.style_for(index, c);
        let (x0, y0, x1, y1) = stream.siz.tile_component_bounds(t, c);
        let mut resolutions = Vec::with_capacity(usize::from(style.levels) + 1);
        for r in 0..=usize::from(style.levels) {
            resolutions.push(build_resolution(style, r, (x0, y0, x1, y1), budget)?);
        }
        components.push(TileComponent {
            x0,
            y0,
            x1,
            y1,
            resolutions,
        });
    }
    Ok(Tile { index, components })
}

fn build_resolution(
    style: &CodingStyle,
    r: usize,
    bounds: (u32, u32, u32, u32),
    budget: &mut u64,
) -> Result<Resolution, Refusal> {
    let (tcx0, tcy0, tcx1, tcy1) = (
        i64::from(bounds.0),
        i64::from(bounds.1),
        i64::from(bounds.2),
        i64::from(bounds.3),
    );
    // B.5 equation B-14: resolution `r` is the tile-component divided by
    // 2^(NL - r), rounded up.
    let nb = i64::from(style.levels) - r as i64;
    let scale = 1i64 << nb;
    let (rx0, ry0, rx1, ry1) = (
        ceil_div(tcx0, scale),
        ceil_div(tcy0, scale),
        ceil_div(tcx1, scale),
        ceil_div(tcy1, scale),
    );

    let (ppx, ppy) = style.precinct_exponents(r);
    // B.6: precincts partition the *resolution* grid, anchored at the origin.
    let (pw, ph) = if rx1 > rx0 && ry1 > ry0 {
        (
            (floor_div(rx1 - 1, 1i64 << ppx) - floor_div(rx0, 1i64 << ppx) + 1) as u32,
            (floor_div(ry1 - 1, 1i64 << ppy) - floor_div(ry0, 1i64 << ppy) + 1) as u32,
        )
    } else {
        (0, 0)
    };
    charge(budget, u64::from(pw) * u64::from(ph))?;

    let (cbw, cbh) = style.code_block_exponents(r);
    let orientations: &[Orientation] = if r == 0 {
        &[Orientation::Ll]
    } else {
        &[Orientation::Hl, Orientation::Lh, Orientation::Hh]
    };
    // The decomposition level a subband belongs to: NL for resolution 0's LL,
    // and NL - r + 1 for the three at resolution r.
    let level = if r == 0 {
        style.levels
    } else {
        style.levels - r as u8 + 1
    };
    let sub_nb = i64::from(level);

    let mut bands = Vec::with_capacity(orientations.len());
    for &orientation in orientations {
        let (xob, yob) = orientation.offsets();
        let denom = 1i64 << sub_nb;
        let half = 1i64 << (sub_nb - 1).max(0);
        // B.5 equation B-15. The subtraction is what makes `ceil_div`'s
        // negative case reachable.
        let (bx0, by0, bx1, by1) = if r == 0 {
            (
                ceil_div(tcx0, denom),
                ceil_div(tcy0, denom),
                ceil_div(tcx1, denom),
                ceil_div(tcy1, denom),
            )
        } else {
            (
                ceil_div(tcx0 - half * xob, denom),
                ceil_div(tcy0 - half * yob, denom),
                ceil_div(tcx1 - half * xob, denom),
                ceil_div(tcy1 - half * yob, denom),
            )
        };
        let precincts = build_precincts(
            (bx0, by0, bx1, by1),
            (rx0, ry0),
            (pw, ph),
            (ppx, ppy),
            (cbw, cbh),
            r,
            budget,
        )?;
        bands.push(Subband {
            orientation,
            x0: clamp_u32(bx0),
            y0: clamp_u32(by0),
            x1: clamp_u32(bx1),
            y1: clamp_u32(by1),
            level,
            precincts,
        });
    }

    Ok(Resolution {
        x0: clamp_u32(rx0),
        y0: clamp_u32(ry0),
        x1: clamp_u32(rx1),
        y1: clamp_u32(ry1),
        pw,
        ph,
        ppx,
        ppy,
        bands,
    })
}

/// B.7: the code-blocks of every precinct of one subband.
#[allow(clippy::too_many_arguments)]
fn build_precincts(
    band: (i64, i64, i64, i64),
    res_origin: (i64, i64),
    grid: (u32, u32),
    precinct: (u8, u8),
    code_block: (u8, u8),
    r: usize,
    budget: &mut u64,
) -> Result<Vec<PrecinctBand>, Refusal> {
    let (bx0, by0, bx1, by1) = band;
    let (pw, ph) = grid;
    // A precinct in *subband* coordinates: one-to-one at resolution 0, half
    // as wide and half as tall above it, because one resolution's precinct
    // spans two subband samples in each direction.
    let (spx, spy) = if r == 0 {
        (i64::from(precinct.0), i64::from(precinct.1))
    } else {
        (
            i64::from(precinct.0).max(1) - 1,
            i64::from(precinct.1).max(1) - 1,
        )
    };
    let (cbx, cby) = (i64::from(code_block.0), i64::from(code_block.1));
    let (rx0, ry0) = res_origin;
    let base_x = floor_div(rx0, 1i64 << precinct.0);
    let base_y = floor_div(ry0, 1i64 << precinct.1);

    let mut out = Vec::with_capacity((pw as usize).saturating_mul(ph as usize));
    for py in 0..ph {
        for px in 0..pw {
            // This precinct's window in subband coordinates, clipped to the
            // subband. Empty when the subband is.
            let gx0 = (base_x + i64::from(px)) << spx;
            let gy0 = (base_y + i64::from(py)) << spy;
            let x0 = gx0.max(bx0);
            let y0 = gy0.max(by0);
            let x1 = (gx0 + (1i64 << spx)).min(bx1);
            let y1 = (gy0 + (1i64 << spy)).min(by1);

            let (w, h, blocks) = if x0 < x1 && y0 < y1 {
                // B.7: code-blocks partition the subband on a grid anchored
                // at the origin, so the first one in a precinct is usually a
                // partial block.
                let k0 = floor_div(x0, 1i64 << cbx);
                let k1 = floor_div(x1 - 1, 1i64 << cbx);
                let l0 = floor_div(y0, 1i64 << cby);
                let l1 = floor_div(y1 - 1, 1i64 << cby);
                let w = (k1 - k0 + 1) as u32;
                let h = (l1 - l0 + 1) as u32;
                charge(budget, u64::from(w) * u64::from(h))?;
                let mut blocks = Vec::with_capacity((w as usize) * (h as usize));
                for l in l0..=l1 {
                    for k in k0..=k1 {
                        blocks.push(CodeBlock {
                            x0: clamp_u32((k << cbx).max(x0)),
                            y0: clamp_u32((l << cby).max(y0)),
                            x1: clamp_u32(((k + 1) << cbx).min(x1)),
                            y1: clamp_u32(((l + 1) << cby).min(y1)),
                            lblock: 3,
                            ..CodeBlock::default()
                        });
                    }
                }
                (w, h, blocks)
            } else {
                (0, 0, Vec::new())
            };
            out.push(PrecinctBand {
                blocks,
                w,
                h,
                inclusion: TagTree::new(w, h),
                zero_planes: TagTree::new(w, h),
            });
        }
    }
    Ok(out)
}

fn clamp_u32(v: i64) -> u32 {
    v.clamp(0, i64::from(u32::MAX)) as u32
}

/// Spends `n` from the total. Spent and never refunded — see [`super`].
fn charge(budget: &mut u64, n: u64) -> Result<(), Refusal> {
    *budget = budget
        .checked_sub(n)
        .ok_or(Refusal::Budget("code-blocks, precincts and packets"))?;
    Ok(())
}

// --- the packet sequence (B.12) -----------------------------------------

/// One packet's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Packet {
    layer: u16,
    resolution: u8,
    component: u16,
    precinct: u32,
}

/// Reads every packet of one tile, over the concatenation of its tile-parts.
fn read_tile_packets(
    stream: &Codestream<'_>,
    tile: &mut Tile,
    budget: &mut u64,
) -> Result<(), Refusal> {
    let cod = stream.cod_for(tile.index).clone();
    // B.9: the packets of a tile run across its tile-parts as one sequence,
    // so the parts are joined before anything is read out of them.
    let mut data = Vec::new();
    for part in stream.tile_parts.iter().filter(|p| p.tile == tile.index) {
        data.extend_from_slice(part.data);
    }

    let order = packet_order(stream, tile, &cod, budget)?;
    let mut at = 0usize;
    for packet in order {
        at = read_packet(tile, &cod, &packet, &data, at)?;
    }
    // The integrity check, in its plainest form: the packets of a tile
    // consume the tile's data exactly. A tier-2 parse that has gone wrong
    // almost never ends on the last byte by accident.
    if at != data.len() {
        return Err(Refusal::PacketLength);
    }
    Ok(())
}

/// B.12's five progression orders.
fn packet_order(
    stream: &Codestream<'_>,
    tile: &Tile,
    cod: &super::codestream::Cod,
    budget: &mut u64,
) -> Result<Vec<Packet>, Refusal> {
    let layers = cod.layers;
    let components = tile.components.len();
    let max_res = tile
        .components
        .iter()
        .map(|c| c.resolutions.len())
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();

    match cod.progression {
        Progression::Lrcp => {
            for l in 0..layers {
                for r in 0..max_res {
                    for c in 0..components {
                        for p in 0..precinct_count(tile, c, r) {
                            emit(&mut out, l, r, c, p, budget)?;
                        }
                    }
                }
            }
        }
        Progression::Rlcp => {
            for r in 0..max_res {
                for l in 0..layers {
                    for c in 0..components {
                        for p in 0..precinct_count(tile, c, r) {
                            emit(&mut out, l, r, c, p, budget)?;
                        }
                    }
                }
            }
        }
        // The three positional orders walk the reference grid rather than a
        // precinct index, so a packet's address has to be recovered from a
        // coordinate. B.12.1.3 to B.12.1.5.
        Progression::Rpcl | Progression::Pcrl | Progression::Cprl => {
            positional_order(stream, tile, cod, &mut out, budget)?;
        }
    }
    Ok(out)
}

fn emit(
    out: &mut Vec<Packet>,
    l: u16,
    r: usize,
    c: usize,
    p: u32,
    budget: &mut u64,
) -> Result<(), Refusal> {
    charge(budget, 1)?;
    out.push(Packet {
        layer: l,
        resolution: u8::try_from(r).map_err(|_| Refusal::Budget("resolutions"))?,
        component: u16::try_from(c).map_err(|_| Refusal::Budget("components"))?,
        precinct: p,
    });
    Ok(())
}

fn precinct_count(tile: &Tile, c: usize, r: usize) -> u32 {
    tile.components
        .get(c)
        .and_then(|comp| comp.resolutions.get(r))
        .map_or(0, |res| res.pw.saturating_mul(res.ph))
}

/// B.12.1.3 to B.12.1.5: RPCL, PCRL and CPRL.
///
/// All three walk positions on the reference grid and ask, at each one,
/// whether it is the origin of a precinct of some (component, resolution).
/// The step is the smallest precinct projected back onto the reference grid,
/// so the walk visits every precinct origin and nothing else — a walk in
/// single pixels would be correct and would also be the tile's area in
/// iterations.
fn positional_order(
    stream: &Codestream<'_>,
    tile: &Tile,
    cod: &super::codestream::Cod,
    out: &mut Vec<Packet>,
    budget: &mut u64,
) -> Result<(), Refusal> {
    let (tx0, ty0, tx1, ty1) = stream.siz.tile_bounds(u32::from(tile.index));
    if tx0 >= tx1 || ty0 >= ty1 {
        return Ok(());
    }
    let components = tile.components.len();

    // The projection of one precinct onto the reference grid, per
    // (component, resolution).
    let step = |c: usize, r: usize| -> Option<(u64, u64)> {
        let comp = stream.siz.components.get(c)?;
        let res = tile.components.get(c)?.resolutions.get(r)?;
        let levels = tile.components.get(c)?.resolutions.len() as u32 - 1;
        let shift_x = u32::from(res.ppx) + levels - r as u32;
        let shift_y = u32::from(res.ppy) + levels - r as u32;
        // The shift is bounded: PPx is at most 15 and the level count at most
        // 32, so 47 bits on a `u64` that starts at 4.
        Some((
            u64::from(comp.dx) << shift_x.min(48),
            u64::from(comp.dy) << shift_y.min(48),
        ))
    };

    // CPRL walks each component's own positions; RPCL and PCRL share one
    // walk across all of them, which is why the step is a minimum over
    // whichever components the order is about.
    let ranges: Vec<(usize, usize)> = match cod.progression {
        Progression::Cprl => (0..components).map(|c| (c, c + 1)).collect(),
        _ => vec![(0, components)],
    };
    let max_res = tile
        .components
        .iter()
        .map(|c| c.resolutions.len())
        .max()
        .unwrap_or(0);

    for (c0, c1) in ranges {
        let mut dx = u64::MAX;
        let mut dy = u64::MAX;
        for c in c0..c1 {
            for r in 0..tile.components[c].resolutions.len() {
                if let Some((sx, sy)) = step(c, r) {
                    dx = dx.min(sx.max(1));
                    dy = dy.min(sy.max(1));
                }
            }
        }
        if dx == u64::MAX || dy == u64::MAX {
            continue;
        }

        let mut positions = Vec::new();
        let mut y = u64::from(ty0);
        while y < u64::from(ty1) {
            let mut x = u64::from(tx0);
            while x < u64::from(tx1) {
                charge(budget, 1)?;
                positions.push((x, y));
                x += dx - (x % dx);
            }
            y += dy - (y % dy);
        }

        match cod.progression {
            // B.12.1.3: resolution, then position, then component, then
            // layer.
            Progression::Rpcl => {
                for r in 0..max_res {
                    for &(x, y) in &positions {
                        for c in c0..c1 {
                            for l in layers_of(cod, tile, c, r, x, y, stream, tx0, ty0) {
                                emit(out, l.0, r, c, l.1, budget)?;
                            }
                        }
                    }
                }
            }
            // B.12.1.4: position, then component, then resolution, then
            // layer.
            Progression::Pcrl => {
                for &(x, y) in &positions {
                    for c in c0..c1 {
                        for r in 0..tile.components[c].resolutions.len() {
                            for l in layers_of(cod, tile, c, r, x, y, stream, tx0, ty0) {
                                emit(out, l.0, r, c, l.1, budget)?;
                            }
                        }
                    }
                }
            }
            // B.12.1.5: component, then position, then resolution, then
            // layer. The component loop is the `ranges` split above.
            _ => {
                for &(x, y) in &positions {
                    for c in c0..c1 {
                        for r in 0..tile.components[c].resolutions.len() {
                            for l in layers_of(cod, tile, c, r, x, y, stream, tx0, ty0) {
                                emit(out, l.0, r, c, l.1, budget)?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// `(layer, precinct)` for every layer, if `(x, y)` is a precinct origin of
/// `(c, r)`; empty otherwise. All three positional orders end in the layer
/// loop, so it is written once.
#[allow(clippy::too_many_arguments)]
fn layers_of(
    cod: &super::codestream::Cod,
    tile: &Tile,
    c: usize,
    r: usize,
    x: u64,
    y: u64,
    stream: &Codestream<'_>,
    tx0: u32,
    ty0: u32,
) -> Vec<(u16, u32)> {
    match precinct_at(stream, tile, c, r, x, y, tx0, ty0) {
        Some(p) => (0..cod.layers).map(|l| (l, p)).collect(),
        None => Vec::new(),
    }
}

/// B.12.1.3's test: is `(x, y)` on the reference grid the origin of a
/// precinct of component `c` at resolution `r`, and if so, which?
#[allow(clippy::too_many_arguments)]
fn precinct_at(
    stream: &Codestream<'_>,
    tile: &Tile,
    c: usize,
    r: usize,
    x: u64,
    y: u64,
    tx0: u32,
    ty0: u32,
) -> Option<u32> {
    let comp = stream.siz.components.get(c)?;
    let tc = tile.components.get(c)?;
    let res = tc.resolutions.get(r)?;
    if res.pw == 0 || res.ph == 0 {
        return None;
    }
    let levels = tc.resolutions.len() as u32 - 1;
    let nb = levels - r as u32;
    let sx = u64::from(comp.dx) << nb;
    let sy = u64::from(comp.dy) << nb;
    let rpx = u64::from(comp.dx) << (nb + u32::from(res.ppx)).min(48);
    let rpy = u64::from(comp.dy) << (nb + u32::from(res.ppy)).min(48);

    // The second half of each test is the tile's own first row and column:
    // the tile does not start on a precinct boundary, and its first precinct
    // is a partial one that still has to be emitted.
    let on_x = x % rpx == 0
        || (x == u64::from(tx0) && ((u64::from(res.x0) << nb) % (1u64 << u32::from(res.ppx))) != 0);
    let on_y = y % rpy == 0
        || (y == u64::from(ty0) && ((u64::from(res.y0) << nb) % (1u64 << u32::from(res.ppy))) != 0);
    if !on_x || !on_y {
        return None;
    }

    let px = floor_div(ceil_div(x as i64, sx as i64), 1i64 << res.ppx)
        - floor_div(i64::from(res.x0), 1i64 << res.ppx);
    let py = floor_div(ceil_div(y as i64, sy as i64), 1i64 << res.ppy)
        - floor_div(i64::from(res.y0), 1i64 << res.ppy);
    if px < 0 || py < 0 || px >= i64::from(res.pw) || py >= i64::from(res.ph) {
        return None;
    }
    u32::try_from(py * i64::from(res.pw) + px).ok()
}

// --- one packet (B.10) --------------------------------------------------

/// SOP (A.8.1), which is six bytes and a sequence number.
const SOP: [u8; 2] = [0xFF, 0x91];
/// EPH (A.8.2), which is two bytes and no segment.
const EPH: [u8; 2] = [0xFF, 0x92];

/// Reads one packet's header and body, returning where the next one starts.
fn read_packet(
    tile: &mut Tile,
    cod: &super::codestream::Cod,
    packet: &Packet,
    data: &[u8],
    mut at: usize,
) -> Result<usize, Refusal> {
    if cod.sop {
        // Signalled and absent is a refusal, not a shrug: SOP is the one
        // thing in the format that says where a packet begins, and a decoder
        // that carries on without it has given up the check it was offered.
        if data.get(at..at + 2) != Some(&SOP[..]) {
            return Err(Refusal::PacketLength);
        }
        at += 6;
        if at > data.len() {
            return Err(Refusal::Truncated("an SOP marker segment"));
        }
    }

    let body = data.get(at..).ok_or(Refusal::Truncated("a packet"))?;
    let mut bits = PacketBits::new(body);
    let mut contributions: Vec<(usize, usize, usize, usize, u32)> = Vec::new();

    // B.10.3: the first bit says whether the packet carries anything at all.
    if bits.bit()? == 1 {
        let component = usize::from(packet.component);
        let resolution = usize::from(packet.resolution);
        let bands = tile
            .components
            .get(component)
            .and_then(|c| c.resolutions.get(resolution))
            .map(|r| r.bands.len())
            .unwrap_or(0);
        for b in 0..bands {
            let precinct = usize::try_from(packet.precinct)
                .map_err(|_| Refusal::Structure("a precinct index past addressable"))?;
            let Some(band) = tile.components[component].resolutions[resolution]
                .bands
                .get_mut(b)
            else {
                continue;
            };
            if band.is_empty() {
                // B.10: a subband with no coefficients in this tile signals
                // nothing at all, not an empty list.
                continue;
            }
            let Some(pb) = band.precincts.get_mut(precinct) else {
                return Err(Refusal::Structure(
                    "a packet naming a precinct that does not exist",
                ));
            };
            for i in 0..pb.blocks.len() {
                let (gx, gy) = (i as u32 % pb.w.max(1), i as u32 / pb.w.max(1));
                let included = if pb.blocks[i].included {
                    // B.10.4: an already-included block costs one bit.
                    bits.bit()? == 1
                } else {
                    pb.inclusion
                        .decode(&mut bits, gx, gy, u32::from(packet.layer) + 1)?
                };
                if !included {
                    continue;
                }
                if !pb.blocks[i].included {
                    // B.10.5: the zero bit-plane count is decoded by raising
                    // the threshold until the tree resolves the leaf.
                    let mut threshold = 1u32;
                    while !pb.zero_planes.decode(&mut bits, gx, gy, threshold)? {
                        threshold += 1;
                        if threshold > 74 {
                            // T.800 caps a code-block at 31 magnitude
                            // bit-planes and Mb at 37; a tree that has not
                            // resolved by here is not describing a picture.
                            return Err(Refusal::Structure(
                                "a zero bit-plane count past any legal Mb",
                            ));
                        }
                    }
                    let value = pb.zero_planes.value(gx, gy);
                    pb.blocks[i].zero_planes = u8::try_from(value).map_err(|_| {
                        Refusal::Structure("a zero bit-plane count past any legal Mb")
                    })?;
                    pb.blocks[i].included = true;
                }
                let passes = read_pass_count(&mut bits)?;
                // B.10.7: `Lblock` grows by one per signalled 1-bit and never
                // shrinks, across every layer of this code-block.
                while bits.bit()? == 1 {
                    pb.blocks[i].lblock += 1;
                    if pb.blocks[i].lblock > 32 {
                        return Err(Refusal::Structure(
                            "an Lblock past any legal segment length",
                        ));
                    }
                }
                let width = pb.blocks[i].lblock + floor_log2(passes);
                let length = bits.bits(width)? as usize;
                contributions.push((component, resolution, b, i, passes));
                // Reuse the tuple's last slot for the length by pushing it
                // separately would cost a second vector; the length rides in
                // the block's own accumulator instead.
                pb.blocks[i].data.reserve(length);
                lengths_push(&mut contributions, length);
            }
        }
    }
    bits.align()?;
    let header_len = bits.consumed();
    at += header_len;

    if cod.eph {
        if data.get(at..at + 2) != Some(&EPH[..]) {
            return Err(Refusal::PacketLength);
        }
        at += 2;
    }

    // The bodies follow the header in the same order the header listed them.
    let mut i = 0;
    while i < contributions.len() {
        let (component, resolution, b, blk, passes) = contributions[i];
        let length = contributions[i + 1].0;
        i += 2;
        let end = at
            .checked_add(length)
            .ok_or(Refusal::Structure("a code-block length past addressable"))?;
        let bytes = data
            .get(at..end)
            .ok_or(Refusal::Truncated("a code-block segment"))?;
        let block = &mut tile.components[component].resolutions[resolution].bands[b].precincts
            [usize::try_from(packet.precinct).unwrap_or(0)]
        .blocks[blk];
        block.data.extend_from_slice(bytes);
        block.passes += passes;
        at = end;
    }
    Ok(at)
}

/// A length pushed as a second tuple, so the contribution list stays one
/// vector. See [`read_packet`].
fn lengths_push(list: &mut Vec<(usize, usize, usize, usize, u32)>, length: usize) {
    list.push((length, 0, 0, 0, 0));
}

/// B.10.6's variable-length code for the number of coding passes.
fn read_pass_count(bits: &mut PacketBits<'_>) -> Result<u32, Refusal> {
    if bits.bit()? == 0 {
        return Ok(1);
    }
    if bits.bit()? == 0 {
        return Ok(2);
    }
    let two = bits.bits(2)?;
    if two < 3 {
        return Ok(3 + two);
    }
    let five = bits.bits(5)?;
    if five < 31 {
        return Ok(6 + five);
    }
    Ok(37 + bits.bits(7)?)
}

/// `floor(log2(n))` for a non-zero `n`, which is the extra width B.10.7 adds
/// to `Lblock` for a code-block contributing several passes.
const fn floor_log2(n: u32) -> u32 {
    31 - n.leading_zeros()
}
