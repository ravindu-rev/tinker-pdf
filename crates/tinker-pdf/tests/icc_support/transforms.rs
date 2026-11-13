//! The profiles each transform RC-20 reads is fingerprinted over
//! (`determinism.rs`, `icc_transforms_are_stable_across_targets`) and the
//! version 5 profiles its page draws through
//! (`a_v5_profiled_page_renders_the_same_on_every_target`), written byte by
//! byte from ICC.1:2010 and ICC.2:2023: the way out of the connection space
//! through each model and table type, device links, the gamut tag, named
//! colours, and a version 5 profile's multi-process elements, calculator and
//! spectral connection space.
//!
//! Every value is made of sums and products and the encodings the two
//! specifications define — `s15Fixed16Number`, `float32Number`, the 8- and
//! 16-bit table entries — which IEEE 754 rounds alike on every target, so
//! each profile is the same bytes everywhere and a hash of what it answers is
//! a claim about the engine alone. The shapes are the ones the `icc_profile`
//! fuzz seeds have (`tinker-pdf-color`'s `write_the_fuzz_seeds`), which that
//! writer holds to reaching their readers before it writes them.

/// A version 2.4 header: the major version, then the minor and bug-fix
/// versions in binary-coded decimal (ICC.1 7.2.4).
pub const VERSION_2: u32 = 0x0240_0000;

/// A version 4.3 header.
pub const VERSION_4: u32 = 0x0430_0000;

/// A version 5.0 header (ICC.2 7.2.4).
pub const VERSION_5: u32 = 0x0500_0000;

/// The identity matrix, row by row.
pub const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// D65's white in XYZ, a media white other than the connection space's, so
/// ICC-absolute colorimetry scales by it (ICC.1 6.2.3).
const D65: [f64; 3] = [0.9505, 1.0, 1.089];

/// A profile of header version `version` and device class `class` over the
/// data colour space `space`, its connection-space field `pcs`, holding
/// `tags` in their order, each four-byte aligned: the 128-byte header (ICC.1
/// 7.2), the tag table (7.3) and the tags.
pub fn profile(
    version: u32,
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    tags: &[([u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let start = 128 + 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::with_capacity(tags.len());
    for (signature, body) in tags {
        while (start + data.len()) % 4 != 0 {
            data.push(0);
        }
        entries.push((*signature, start + data.len(), body.len()));
        data.extend_from_slice(body);
    }
    while (start + data.len()) % 4 != 0 {
        data.push(0);
    }
    let size = start + data.len();
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    out[8..12].copy_from_slice(&version.to_be_bytes());
    out[12..16].copy_from_slice(class);
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(pcs);
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for (signature, offset, length) in entries {
        out.extend_from_slice(&signature);
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(length as u32).to_be_bytes());
    }
    out.extend_from_slice(&data);
    out
}

/// An `s15Fixed16Number` (ICC.1 4.6).
pub fn s15(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// A `float32Number` (ICC.2 4.2.4), the value rounded to single precision.
pub fn float(value: f64) -> [u8; 4] {
    (value as f32).to_bits().to_be_bytes()
}

/// An `XYZType` tag of one value (ICC.1 10.31).
pub fn xyz(value: [f64; 3]) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for component in value {
        tag.extend_from_slice(&s15(component));
    }
    tag
}

/// A `curveType` of one entry: a pure gamma, as a `u8Fixed8Number` (ICC.1
/// 10.6).
pub fn gamma(value: f64) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&1u32.to_be_bytes());
    tag.extend_from_slice(&((value * 256.0).round() as u16).to_be_bytes());
    tag
}

/// A `curveType` of no entries, the identity (ICC.1 10.6).
pub fn identity_curve() -> Vec<u8> {
    b"curv\0\0\0\0\0\0\0\0".to_vec()
}

/// A `curveType` of `points`, spaced evenly over `0..=1` (ICC.1 10.6).
pub fn sampled_curve(points: &[u16]) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&(points.len() as u32).to_be_bytes());
    for point in points {
        tag.extend_from_slice(&point.to_be_bytes());
    }
    tag
}

/// A `parametricCurveType` of function type `function` (ICC.1 10.18, Table
/// 65), its parameters in the order the table lists them.
pub fn parametric(function: u16, params: &[f64]) -> Vec<u8> {
    let mut tag = b"para\0\0\0\0".to_vec();
    tag.extend_from_slice(&function.to_be_bytes());
    tag.extend_from_slice(&[0; 2]);
    for &param in params {
        tag.extend_from_slice(&s15(param));
    }
    tag
}

/// An `mft2` (`lut16Type`, ICC.1 10.8) of `inputs` and `outputs` channels
/// over a grid of `grid` points an axis, its matrix `matrix` row by row and
/// its curves the identity, node `n`'s channel `c` the value `value(n, c)`,
/// the first input's axis slowest.
pub fn mft2(
    inputs: usize,
    outputs: usize,
    grid: usize,
    matrix: [f64; 9],
    value: impl Fn(usize, usize) -> u16,
) -> Vec<u8> {
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[inputs as u8, outputs as u8, grid as u8, 0]);
    for entry in matrix {
        tag.extend_from_slice(&s15(entry));
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..inputs {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    for node in 0..grid.pow(inputs as u32) {
        for channel in 0..outputs {
            tag.extend_from_slice(&value(node, channel).to_be_bytes());
        }
    }
    for _ in 0..outputs {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    tag
}

/// An `mft1` (`lut8Type`, ICC.1 10.9) of `inputs` and `outputs` channels
/// over a grid of `grid` points an axis, its matrix the identity and its
/// curves the 256-entry identity, node `n`'s channel `c` the byte
/// `value(n, c)`, the first input's axis slowest.
pub fn mft1(
    inputs: usize,
    outputs: usize,
    grid: usize,
    value: impl Fn(usize, usize) -> u8,
) -> Vec<u8> {
    let mut tag = b"mft1\0\0\0\0".to_vec();
    tag.extend_from_slice(&[inputs as u8, outputs as u8, grid as u8, 0]);
    for entry in IDENTITY {
        tag.extend_from_slice(&s15(entry));
    }
    let ramp: Vec<u8> = (0..=255u8).collect();
    for _ in 0..inputs {
        tag.extend_from_slice(&ramp);
    }
    for node in 0..grid.pow(inputs as u32) {
        for channel in 0..outputs {
            tag.push(value(node, channel));
        }
    }
    for _ in 0..outputs {
        tag.extend_from_slice(&ramp);
    }
    tag
}

/// A version 4 lookup table's stages (ICC.1 10.10, 10.11), as
/// [`V4Stages::tag`] lays them out: each curve set a run of curve tags,
/// empty where the stage is absent.
#[derive(Clone, Default)]
pub struct V4Stages {
    /// The B curves.
    pub b: Vec<Vec<u8>>,
    /// Nine entries row by row, then three offsets.
    pub matrix: Option<[f64; 12]>,
    /// The M curves.
    pub m: Vec<Vec<u8>>,
    /// One grid count an input, and the 16-bit entries, the last input's
    /// axis fastest.
    pub clut: Option<(Vec<u8>, Vec<u16>)>,
    /// The A curves.
    pub a: Vec<Vec<u8>>,
}

impl V4Stages {
    /// The tag: `signature`'s header over `inputs` and `outputs` channels,
    /// then each stage present in the order the header lists their offsets
    /// — B, matrix, M, CLUT, A — each curve and stage padded to a four-byte
    /// boundary.
    pub fn tag(&self, signature: &[u8; 4], inputs: u8, outputs: u8) -> Vec<u8> {
        fn pad(tag: &mut Vec<u8>) {
            while tag.len() % 4 != 0 {
                tag.push(0);
            }
        }
        fn run(tag: &mut Vec<u8>, curves: &[Vec<u8>]) -> u32 {
            let at = tag.len() as u32;
            for curve in curves {
                tag.extend_from_slice(curve);
                pad(tag);
            }
            at
        }
        let mut tag = signature.to_vec();
        tag.extend_from_slice(&[0; 4]);
        tag.extend_from_slice(&[inputs, outputs, 0, 0]);
        tag.extend_from_slice(&[0; 20]);
        let mut offsets = [0u32; 5];
        if !self.b.is_empty() {
            offsets[0] = run(&mut tag, &self.b);
        }
        if let Some(matrix) = self.matrix {
            offsets[1] = tag.len() as u32;
            for value in matrix {
                tag.extend_from_slice(&s15(value));
            }
        }
        if !self.m.is_empty() {
            offsets[2] = run(&mut tag, &self.m);
        }
        if let Some((grids, entries)) = &self.clut {
            offsets[3] = tag.len() as u32;
            let mut counts = [0u8; 16];
            for (slot, count) in counts.iter_mut().zip(grids) {
                *slot = *count;
            }
            tag.extend_from_slice(&counts);
            // Two bytes an entry, then three of padding.
            tag.extend_from_slice(&[2, 0, 0, 0]);
            for entry in entries {
                tag.extend_from_slice(&entry.to_be_bytes());
            }
            pad(&mut tag);
        }
        if !self.a.is_empty() {
            offsets[4] = run(&mut tag, &self.a);
        }
        for (slot, offset) in offsets.iter().enumerate() {
            tag[12 + 4 * slot..16 + 4 * slot].copy_from_slice(&offset.to_be_bytes());
        }
        tag
    }
}

/// An `ncl2` tag (ICC.1 10.15, Table 63) naming `entries` — a root name,
/// three connection-space coordinates and `device` device coordinates each —
/// with `prefix` and `suffix`, its vendor flag zero.
pub fn ncl2(
    prefix: &str,
    suffix: &str,
    device: usize,
    entries: &[(&str, [u16; 3], &[u16])],
) -> Vec<u8> {
    let field = |text: &str| {
        let mut out = text.as_bytes().to_vec();
        out.resize(32, 0);
        out
    };
    let mut tag = b"ncl2".to_vec();
    tag.extend_from_slice(&[0; 8]);
    tag.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    tag.extend_from_slice(&(device as u32).to_be_bytes());
    tag.extend(field(prefix));
    tag.extend(field(suffix));
    for (root, pcs, coordinates) in entries {
        tag.extend(field(root));
        for value in pcs.iter().chain(coordinates.iter()) {
            tag.extend_from_slice(&value.to_be_bytes());
        }
    }
    tag
}

// ---- multi-process elements (ICC.1 10.14, ICC.2 11) ------------------------

/// `out` padded with zeros to a four-byte boundary, as every element and
/// curve starts on one (ICC.1 10.14.1, 10.14.2.2).
fn padded(mut out: Vec<u8>) -> Vec<u8> {
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// A `parf` formula segment of function type `function` (ICC.1 10.14.2.2,
/// Table 57; ICC.2 Table 111), its parameters as float32.
pub fn parf(function: u16, params: &[f64]) -> Vec<u8> {
    let mut out = b"parf".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&function.to_be_bytes());
    out.extend_from_slice(&[0; 2]);
    for &param in params {
        out.extend_from_slice(&float(param));
    }
    out
}

/// A `curf` segmented curve: `segments` with `breaks` between them (ICC.1
/// 10.14.2.2).
pub fn curf(breaks: &[f64], segments: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"curf".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(segments.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 2]);
    for &point in breaks {
        out.extend_from_slice(&float(point));
    }
    for segment in segments {
        out.extend_from_slice(segment);
    }
    out
}

/// A single sampled curve (`sngf`, ICC.2 11.2.2.2) of float32 `entries`
/// from `first` to `last`, extended past them where `extend` is one.
pub fn sngf(first: f64, last: f64, extend: u16, entries: &[f64]) -> Vec<u8> {
    let mut out = b"sngf".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    out.extend_from_slice(&float(first));
    out.extend_from_slice(&float(last));
    out.extend_from_slice(&extend.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for &entry in entries {
        out.extend_from_slice(&float(entry));
    }
    out
}

/// A processing element's header — its signature, four reserved bytes,
/// `p` inputs and `q` outputs — and `body`.
pub fn element_of(signature: &[u8; 4], p: u16, q: u16, body: &[u8]) -> Vec<u8> {
    let mut out = signature.to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&p.to_be_bytes());
    out.extend_from_slice(&q.to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// A `cvst` curve set, one curve a channel, each starting on a four-byte
/// boundary (ICC.1 10.14.2).
pub fn cvst(curves: &[Vec<u8>]) -> Vec<u8> {
    let channels = curves.len();
    let mut positions = Vec::new();
    let mut data = Vec::new();
    let start = 12 + 8 * channels;
    for curve in curves {
        positions.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
        positions.extend_from_slice(&(curve.len() as u32).to_be_bytes());
        data.extend_from_slice(&padded(curve.clone()));
    }
    positions.extend_from_slice(&data);
    element_of(b"cvst", channels as u16, channels as u16, &positions)
}

/// A `matf` matrix: `rows`, each the coefficients of one output, then an
/// offset a row (ICC.1 10.14.3).
pub fn matf(rows: &[Vec<f64>], offsets: &[f64]) -> Vec<u8> {
    let p = rows.first().map_or(0, Vec::len);
    let mut body = Vec::new();
    for row in rows {
        for &value in row {
            body.extend_from_slice(&float(value));
        }
    }
    for &offset in offsets {
        body.extend_from_slice(&float(offset));
    }
    element_of(b"matf", p as u16, rows.len() as u16, &body)
}

/// An extended grid (`xclt`, ICC.2 11.2.7) of `grids` points an axis giving
/// `q` outputs, its 16-bit `values`, the last axis fastest.
pub fn xclt(grids: &[u8], q: u16, values: &[u16]) -> Vec<u8> {
    let mut body = 2u32.to_be_bytes().to_vec();
    let mut counts = [0u8; 16];
    counts[..grids.len()].copy_from_slice(grids);
    body.extend_from_slice(&counts);
    for value in values {
        body.extend_from_slice(&value.to_be_bytes());
    }
    element_of(b"xclt", grids.len() as u16, q, &body)
}

/// An `mpet` tag of `inputs` and `outputs` channels chaining `elements`,
/// each starting on a four-byte boundary (ICC.1 10.14).
pub fn mpet(inputs: u16, outputs: u16, elements: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"mpet".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&inputs.to_be_bytes());
    out.extend_from_slice(&outputs.to_be_bytes());
    out.extend_from_slice(&(elements.len() as u32).to_be_bytes());
    let start = 16 + 8 * elements.len();
    let mut data = Vec::new();
    for element in elements {
        out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(element.len() as u32).to_be_bytes());
        data.extend_from_slice(&padded(element.clone()));
    }
    out.extend_from_slice(&data);
    out
}

// ---- the calculator element (ICC.2 11.2.1) ---------------------------------

/// A calculator operation of a 16-bit `s` and `t`.
pub fn calc_op(signature: &[u8; 4], s: u16, t: u16) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(signature);
    out[4..6].copy_from_slice(&s.to_be_bytes());
    out[6..].copy_from_slice(&t.to_be_bytes());
    out
}

/// A calculator operation carrying a 32-bit count, or a float32's bits.
pub fn calc_word(signature: &[u8; 4], word: u32) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(signature);
    out[4..].copy_from_slice(&word.to_be_bytes());
    out
}

/// `data`: `value` pushed, as a float32.
pub fn calc_data(value: f64) -> [u8; 8] {
    calc_word(b"data", (value as f32).to_bits())
}

/// A calculator element of `inputs` and `outputs` channels whose main
/// function is `ops`, after its header and sub-element positions, and whose
/// sub-elements `subs` follow it, each on a four-byte boundary.
pub fn calc_of(inputs: u16, outputs: u16, ops: &[[u8; 8]], subs: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"calc".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&inputs.to_be_bytes());
    out.extend_from_slice(&outputs.to_be_bytes());
    out.extend_from_slice(&[0; 2]);
    out.extend_from_slice(&(subs.len() as u16).to_be_bytes());
    let header = 24 + 8 * subs.len();
    let function = 12 + 8 * ops.len();
    out.extend_from_slice(&(header as u32).to_be_bytes());
    out.extend_from_slice(&(function as u32).to_be_bytes());
    let start = header + function;
    let mut body = Vec::new();
    for sub in subs {
        while (start + body.len()) % 4 != 0 {
            body.push(0);
        }
        out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(sub.len() as u32).to_be_bytes());
        body.extend_from_slice(sub);
    }
    out.extend_from_slice(b"func");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(ops.len() as u32).to_be_bytes());
    for op in ops {
        out.extend_from_slice(op);
    }
    out.extend(body);
    out
}

/// A sampled calculator curve (`clcf`, ICC.2 11.2.2.3) of `entries`
/// recommended samples from `first` to `last`, clipped past them, its curve
/// `calculator`'s.
pub fn clcf(entries: u32, first: f64, last: f64, calculator: &[u8]) -> Vec<u8> {
    let mut out = b"clcf".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&entries.to_be_bytes());
    out.extend_from_slice(&float(first));
    out.extend_from_slice(&float(last));
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(calculator);
    out
}

// ---- the fingerprinted profiles --------------------------------------------

/// A four-ink `A2B0` over a two-point grid into Lab: each corner of the ink
/// cube a grey that darkens with every ink it holds, `L*` from `FF00h` down
/// `3000h` an ink, `a*` and `b*` at `8000h`, in `mft2`'s legacy encoding.
fn ink_greys() -> Vec<u8> {
    mft2(4, 3, 2, IDENTITY, |node, channel| match channel {
        0 => 0xFF00 - node.count_ones() as u16 * 0x3000,
        _ => 0x8000,
    })
}

/// **Each model inverted** (RC-20.4): a matrix/TRC display profile whose
/// three tone curves are each another curve type — sRGB's parametric
/// function (type 3), a gamma of 1.8, and five samples — under a media white
/// of D65, so ICC-absolute colorimetry scales; a grey profile of one gamma
/// into XYZ beside the same white; and a version 4 grey one into Lab.
pub fn model_profiles() -> Vec<Vec<u8>> {
    vec![
        profile(
            VERSION_2,
            b"mntr",
            b"RGB ",
            b"XYZ ",
            &[
                (*b"rXYZ", xyz([0.436_065, 0.222_488, 0.013_916])),
                (*b"gXYZ", xyz([0.385_147, 0.716_873, 0.097_076])),
                (*b"bXYZ", xyz([0.143_066, 0.060_607, 0.714_096])),
                (
                    *b"rTRC",
                    parametric(3, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45]),
                ),
                (*b"gTRC", gamma(1.8)),
                (*b"bTRC", sampled_curve(&[0, 4_000, 15_000, 34_000, 65_535])),
                (*b"wtpt", xyz(D65)),
            ],
        ),
        profile(
            VERSION_2,
            b"mntr",
            b"GRAY",
            b"XYZ ",
            &[(*b"kTRC", gamma(1.8)), (*b"wtpt", xyz(D65))],
        ),
        profile(
            VERSION_4,
            b"mntr",
            b"GRAY",
            b"Lab ",
            &[
                (*b"kTRC", gamma(2.2)),
                (*b"wtpt", xyz([0.9642, 1.0, 0.8249])),
            ],
        ),
    ]
}

/// **The version 2 tables out of the connection space** (RC-20.5): a CMYK
/// printer profile over Lab whose `B2A0` and `B2A1` are `mft2`s over grids of
/// three points an axis, each node's inks by its own formula, and whose
/// `B2A2` is an `mft1`, under a media white of D65; and one over XYZ whose
/// `B2A0` is an `mft2` with a matrix other than the identity, which a table
/// out of XYZ applies first (10.8). `bump` is added to the first entry of the
/// Lab profile's `B2A0` grid, one level a unit, the entry the connection
/// space's corner `L*` 0, `a*` and `b*` −128 reads.
pub fn lut_profiles(bump: u16) -> Vec<Vec<u8>> {
    let lab = profile(
        VERSION_2,
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (*b"A2B0", ink_greys()),
            (
                *b"B2A0",
                mft2(3, 4, 3, IDENTITY, |node, channel| {
                    let value = (((node * 5 + channel * 3) % 17) * 3855) as u16;
                    if node == 0 && channel == 0 {
                        value + bump
                    } else {
                        value
                    }
                }),
            ),
            (
                *b"B2A1",
                mft2(3, 4, 3, IDENTITY, |node, channel| {
                    (((node * 7 + channel * 11) % 19) * 3449) as u16
                }),
            ),
            (
                *b"B2A2",
                mft1(3, 4, 3, |node, channel| {
                    ((node * 13 + channel * 29) % 256) as u8
                }),
            ),
            (*b"wtpt", xyz(D65)),
        ],
    );
    let xyz_pcs = profile(
        VERSION_2,
        b"prtr",
        b"CMYK",
        b"XYZ ",
        &[
            (
                *b"A2B0",
                mft2(4, 3, 2, IDENTITY, |node, channel| {
                    0x7000 - node.count_ones() as u16 * 0x1800 + channel as u16 * 0x0200
                }),
            ),
            (
                *b"B2A0",
                mft2(
                    3,
                    4,
                    3,
                    [0.9, 0.1, 0.0, 0.05, 0.9, 0.05, 0.0, 0.1, 0.9],
                    |node, channel| (((node * 3 + channel * 7) % 13) * 5041) as u16,
                ),
            ),
        ],
    );
    vec![lab, xyz_pcs]
}

/// **Version 4's table out of the connection space** (RC-20.6): a CMYK
/// printer profile over Lab whose `A2B0` is an `mAB ` of identity A curves,
/// a grid of the ink cube's corners and identity B curves, and whose `B2A0`
/// is an `mBA ` carrying all five stages — B curves of gamma 1.5, 1 and 2, a
/// matrix with offsets, M curves of gamma 2, 1 and 0.5, a grid of three
/// points on its first axis and two on the others giving four inks, `((7n +
/// 5c) mod 23) × 2 849` for node `n`'s channel `c`, and A curves of gamma 1,
/// 1.8, 2.2 and 1.
pub fn mba_profile() -> Vec<u8> {
    let forward = V4Stages {
        b: vec![identity_curve(); 3],
        clut: Some((
            vec![2; 4],
            (0..16usize)
                .flat_map(|node| [0xFF00 - node.count_ones() as u16 * 0x3000, 0x8000, 0x8000])
                .collect(),
        )),
        a: vec![identity_curve(); 4],
        ..V4Stages::default()
    };
    let output = V4Stages {
        b: vec![gamma(1.5), gamma(1.0), gamma(2.0)],
        matrix: Some([
            0.5, 0.25, 0.25, 0.0, 1.0, 0.0, 0.25, 0.0, 0.75, 0.0, 0.0625, -0.0625,
        ]),
        m: vec![gamma(2.0), gamma(1.0), gamma(0.5)],
        clut: Some((
            vec![3, 2, 2],
            (0..12usize)
                .flat_map(|node| {
                    (0..4usize).map(move |channel| (((node * 7 + channel * 5) % 23) * 2849) as u16)
                })
                .collect(),
        )),
        a: vec![gamma(1.0), gamma(1.8), gamma(2.2), gamma(1.0)],
    };
    profile(
        VERSION_4,
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (*b"A2B0", forward.tag(b"mAB ", 4, 3)),
            (*b"B2A0", output.tag(b"mBA ", 3, 4)),
        ],
    )
}

/// **Device links** (RC-20.7, ICC.1 8.6): a CMYK-to-CMYK link whose `D2B0`
/// is one matrix element taking `(c, m, y, k)` to `(m, c, y, k/2 + 1/4)`, its
/// `mft2` `A2B0` beside it, and an RGB-to-CMYK link whose one `A2B0` is an
/// `mft2` over a grid of three points an axis.
pub fn link_profiles() -> Vec<Vec<u8>> {
    let swapped = profile(
        VERSION_4,
        b"link",
        b"CMYK",
        b"CMYK",
        &[
            (
                *b"D2B0",
                mpet(
                    4,
                    4,
                    &[matf(
                        &[
                            vec![0.0, 1.0, 0.0, 0.0],
                            vec![1.0, 0.0, 0.0, 0.0],
                            vec![0.0, 0.0, 1.0, 0.0],
                            vec![0.0, 0.0, 0.0, 0.5],
                        ],
                        &[0.0, 0.0, 0.0, 0.25],
                    )],
                ),
            ),
            (
                *b"A2B0",
                mft2(4, 4, 2, IDENTITY, |node, channel| {
                    (((node * 3 + channel * 7) % 13) * 5041) as u16
                }),
            ),
        ],
    );
    let separated = profile(
        VERSION_2,
        b"link",
        b"RGB ",
        b"CMYK",
        &[(
            *b"A2B0",
            mft2(3, 4, 3, IDENTITY, |node, channel| {
                (((node * 11 + channel * 5) % 29) * 2259) as u16
            }),
        )],
    );
    vec![swapped, separated]
}

/// **The gamut tag** (RC-20.8, ICC.1 9.2.28): a CMYK printer profile over
/// Lab carrying [`ink_greys`] forward and an `mft2` `B2A0` back, and a `gamt`,
/// an `mft2` from the connection space to one channel over a two-point grid
/// whose corners holding at most one full component are zero, in gamut, and
/// the rest `FFFFh`, out.
pub fn gamut_profile() -> Vec<u8> {
    profile(
        VERSION_2,
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (*b"A2B0", ink_greys()),
            (
                *b"B2A0",
                mft2(3, 4, 2, IDENTITY, |node, channel| {
                    (((node * 5 + channel * 3) % 17) * 3855) as u16
                }),
            ),
            (
                *b"gamt",
                mft2(3, 1, 2, IDENTITY, |node, _| {
                    if node.count_ones() <= 1 {
                        0
                    } else {
                        0xFFFF
                    }
                }),
            ),
        ],
    )
}

/// **Named colours** (RC-20.9, ICC.1 10.15): a named-colour profile over CMYK
/// and Lab whose `ncl2` names `Ink 1 C`, `Ink 2 C` and `Ink 3 C`, each with
/// its legacy Lab coordinates and four device coordinates; and one over RGB
/// and XYZ naming two colours with three device coordinates each and no
/// prefix or suffix.
pub fn named_profiles() -> Vec<Vec<u8>> {
    let inks = profile(
        VERSION_2,
        b"nmcl",
        b"CMYK",
        b"Lab ",
        &[(
            *b"ncl2",
            ncl2(
                "Ink ",
                " C",
                4,
                &[
                    ("1", [0x7A00, 0xD200, 0xA600], &[0, 0xF5C2, 0xC28F, 0]),
                    ("2", [0x5F00, 0x6E00, 0x2800], &[0xFFFF, 0x2E14, 0, 0x051E]),
                    (
                        "3",
                        [0x8A00, 0x7F80, 0x7C00],
                        &[0x3333, 0x2666, 0x199A, 0x570A],
                    ),
                ],
            ),
        )],
    );
    let lights = profile(
        VERSION_4,
        b"nmcl",
        b"RGB ",
        b"XYZ ",
        &[(
            *b"ncl2",
            ncl2(
                "",
                "",
                3,
                &[
                    (
                        "Signal red",
                        [0x3800, 0x1C00, 0x0400],
                        &[0xFFFF, 0x1000, 0x0800],
                    ),
                    (
                        "Deep teal",
                        [0x0E00, 0x1500, 0x1A00],
                        &[0x0400, 0x5000, 0x5800],
                    ),
                ],
            ),
        )],
    );
    vec![inks, lights]
}

/// The matrix/TRC tags of a display profile, its white point left out: a
/// model to carry beside multi-process tags.
fn matrix_tags() -> Vec<([u8; 4], Vec<u8>)> {
    vec![
        (*b"rXYZ", xyz([0.4360, 0.2225, 0.0139])),
        (*b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
        (*b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
        (*b"rTRC", gamma(2.2)),
        (*b"gTRC", gamma(2.2)),
        (*b"bTRC", gamma(2.2)),
    ]
}

/// **Multi-process elements beside a model** (RC-01.4, RC-20.10): a
/// matrix/TRC profile whose `D2B0` takes each channel through a gamma of 2.2
/// and mixes them, and whose `B2D0` takes the connection space through
/// single sampled curves, one extended past its last entry, a matrix and an
/// extended grid of 16-bit values, node `n`'s channel `c` `((3n + 5c) mod
/// 11) × 6 553`.
pub fn elements_beside_a_model_profile() -> Vec<u8> {
    let power = curf(&[], &[parf(0, &[2.2, 1.0, 0.0, 0.0])]);
    let mix = matf(
        &[
            vec![0.5, 0.3, 0.2],
            vec![0.25, 0.65, 0.1],
            vec![0.05, 0.15, 0.8],
        ],
        &[0.0, 0.0, 0.0],
    );
    let mut tags = matrix_tags();
    tags.push((
        *b"D2B0",
        mpet(3, 3, &[cvst(&[power.clone(), power.clone(), power]), mix]),
    ));
    let values: Vec<u16> = (0..8usize)
        .flat_map(|node| (0..3usize).map(move |c| (((node * 3 + c * 5) % 11) * 6553) as u16))
        .collect();
    tags.push((
        *b"B2D0",
        mpet(
            3,
            3,
            &[
                cvst(&[
                    sngf(0.0, 2.0, 0, &[0.0, 0.3, 0.6, 1.0]),
                    sngf(0.0, 1.0, 1, &[0.0, 0.5, 0.9]),
                    sngf(0.0, 2.0, 0, &[0.0, 1.0]),
                ]),
                matf(
                    &[
                        vec![1.0, -0.1, 0.0],
                        vec![0.0, 1.0, -0.1],
                        vec![-0.1, 0.0, 1.0],
                    ],
                    &[0.05, 0.05, 0.05],
                ),
                xclt(&[2, 2, 2], 3, &values),
            ],
        ),
    ));
    profile(VERSION_2, b"mntr", b"RGB ", b"XYZ ", &tags)
}

/// **A version 5 profile of multi-process elements both ways** (RC-20.10,
/// ICC.2): a display profile whose `A2B0` takes each channel through ICC.2's
/// formula segments and a single sampled curve, then a matrix and an
/// extended grid of 16-bit values, and whose `B2A0` takes the connection
/// space back through a curve of type 3 on each channel and a matrix.
pub fn v5_elements_profile() -> Vec<u8> {
    let values: Vec<u16> = (0..8usize)
        .flat_map(|node| {
            let (i, j, k) = ((node >> 2) & 1, (node >> 1) & 1, node & 1);
            [
                (8_000 + 40_000 * i) as u16,
                (9_000 + 45_000 * j) as u16,
                (7_000 + 35_000 * k) as u16,
            ]
        })
        .collect();
    let forward = mpet(
        3,
        3,
        &[
            cvst(&[
                curf(
                    &[0.5],
                    &[
                        parf(3, &[2.0, 1.0, 1.0, 0.0, 0.0]),
                        parf(5, &[1.0, 1.0, 0.0, 0.0, 1.0, 0.25]),
                    ],
                ),
                sngf(0.0, 1.0, 1, &[0.0, 0.4, 1.0]),
                curf(&[], &[parf(7, &[1.0, 1.0, 0.0, 1.0, 0.5, 1.0])]),
            ]),
            matf(
                &[
                    vec![0.6, 0.3, 0.1],
                    vec![0.2, 0.7, 0.1],
                    vec![0.1, 0.1, 0.8],
                ],
                &[0.0, 0.0, 0.0],
            ),
            xclt(&[2, 2, 2], 3, &values),
        ],
    );
    let half = curf(&[], &[parf(3, &[1.0, 0.5, 1.0, 0.0, 0.0])]);
    let back = mpet(
        3,
        3,
        &[
            cvst(&[half.clone(), half.clone(), half]),
            matf(
                &[
                    vec![1.2, -0.2, 0.0],
                    vec![-0.1, 1.1, 0.0],
                    vec![0.0, -0.1, 1.1],
                ],
                &[0.0, 0.0, 0.0],
            ),
        ],
    );
    profile(
        VERSION_5,
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[(*b"A2B0", forward), (*b"B2A0", back)],
    )
}

/// **A version 5 profile of calculator elements** (RC-20.11, ICC.2 11.2.1):
/// a display profile whose `A2B0` is one calculator element — its inputs
/// through a matrix sub-element into three temporary channels, then the
/// second where the first is at least a half and the third otherwise, the
/// second times three selecting one of three constants, and the third's
/// square root — and whose `B2A0` squares each component through sampled
/// calculator curves of sixteen samples over XYZ's range, then mixes them.
pub fn calculator_profile() -> Vec<u8> {
    let mixing = matf(
        &[
            vec![0.6, 0.3, 0.1],
            vec![0.2, 0.7, 0.1],
            vec![0.1, 0.1, 0.8],
        ],
        &[0.0, 0.0, 0.0],
    );
    let program = [
        calc_op(b"in  ", 0, 2),
        calc_op(b"mtx ", 0, 0),
        calc_op(b"tput", 0, 2),
        calc_op(b"tget", 0, 0),
        calc_word(b"if  ", 1),
        calc_word(b"else", 1),
        calc_op(b"tget", 1, 0),
        calc_op(b"tget", 2, 0),
        calc_op(b"tget", 1, 0),
        calc_data(3.0),
        calc_op(b"mul ", 0, 0),
        calc_word(b"sel ", 0),
        calc_word(b"case", 1),
        calc_word(b"case", 1),
        calc_word(b"dflt", 1),
        calc_data(0.1),
        calc_data(0.2),
        calc_data(0.3),
        calc_op(b"tget", 2, 0),
        calc_op(b"sqrt", 0, 0),
        calc_op(b"out ", 0, 2),
    ];
    let forward = mpet(3, 3, &[calc_of(3, 3, &program, &[mixing])]);
    let square = calc_of(
        1,
        1,
        &[
            calc_op(b"in  ", 0, 0),
            calc_op(b"sq  ", 0, 0),
            calc_op(b"out ", 0, 0),
        ],
        &[],
    );
    let curve = clcf(16, 0.0, 2.0, &square);
    let back = mpet(
        3,
        3,
        &[
            cvst(&[curve.clone(), curve.clone(), curve]),
            matf(
                &[
                    vec![1.0, 0.0, 0.0],
                    vec![0.0, 1.0, 0.0],
                    vec![0.0, 0.0, 1.0],
                ],
                &[0.0, 0.0, 0.0],
            ),
        ],
    );
    profile(
        VERSION_5,
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[(*b"A2B0", forward), (*b"B2A0", back)],
    )
}

/// **A version 5 profile of a spectral connection space** (RC-20.12, ICC.2
/// 6.3.3, 7.2.21): an RGB profile whose connection space is reflectance
/// spectra over four wavelengths from 400 to 700 nm, its `D2B0` a matrix
/// from the device to them, its `svcn` the CIE 1931 observer under D65 as
/// four steps of its own, and its `c2sp`, `s2cp` and `B2A0` each a diagonal
/// matrix, so the viewing conditions connect both ways.
pub fn spectral_profile() -> Vec<u8> {
    let spectrum = mpet(
        3,
        4,
        &[matf(
            &[
                vec![0.6, 0.2, 0.1],
                vec![0.3, 0.5, 0.1],
                vec![0.1, 0.4, 0.3],
                vec![0.05, 0.2, 0.6],
            ],
            &[0.05, 0.05, 0.05, 0.05],
        )],
    );
    // 400 nm and 700 nm as half floats, and four steps.
    let range: [u16; 3] = [0x5E40, 0x6178, 4];
    let range_bytes: Vec<u8> = range.iter().flat_map(|value| value.to_be_bytes()).collect();
    let mut viewing = b"svcn".to_vec();
    viewing.extend_from_slice(&[0; 4]);
    viewing.extend_from_slice(&1u32.to_be_bytes());
    viewing.extend_from_slice(&range_bytes);
    viewing.extend_from_slice(&[0; 2]);
    for value in [0.2, 0.4, 1.0, 0.3, 0.1, 0.9, 0.6, 0.1, 1.2, 0.3, 0.0, 0.0] {
        viewing.extend_from_slice(&float(value));
    }
    viewing.extend_from_slice(&2u32.to_be_bytes());
    viewing.extend_from_slice(&float(6504.0));
    viewing.extend_from_slice(&range_bytes);
    viewing.extend_from_slice(&[0; 2]);
    for value in [0.8, 1.0, 1.1, 0.9] {
        viewing.extend_from_slice(&float(value));
    }
    viewing.extend_from_slice(&[0; 24]);
    let diagonal = |values: [f64; 3]| {
        mpet(
            3,
            3,
            &[matf(
                &[
                    vec![values[0], 0.0, 0.0],
                    vec![0.0, values[1], 0.0],
                    vec![0.0, 0.0, values[2]],
                ],
                &[0.0, 0.0, 0.0],
            )],
        )
    };
    let mut out = profile(
        VERSION_5,
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (*b"D2B0", spectrum),
            (*b"svcn", viewing),
            (*b"c2sp", diagonal([0.95, 1.0, 1.25])),
            (*b"s2cp", diagonal([1.0 / 0.95, 1.0, 0.8])),
            (*b"B2A0", diagonal([1.0, 1.0, 1.0])),
        ],
    );
    // The spectral connection space (ICC.2 7.2.21): reflectance, four
    // channels; and its range (7.2.22).
    out[100..104].copy_from_slice(b"rs\x00\x04");
    out[104..110].copy_from_slice(&range_bytes);
    out
}
