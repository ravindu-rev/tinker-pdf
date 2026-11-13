//! Colour tables written byte by byte from the OpenType chapters' layouts —
//! `COLR` versions 0 and 1, `CPAL`, `sbix`, `CBLC`/`CBDT` and `SVG ` — and the
//! pictures they carry, for the tests that draw colour glyphs
//! (`colour_fonts.rs`) and the page of them `determinism.rs` fingerprints.
//!
//! The faces are synthesised (`epub_support::typeface`): each table here is
//! handed to a `Face` as given, and no table is read back by anything but the
//! engine under test.

#![allow(
    dead_code,
    reason = "shared by several test binaries; each uses a different subset"
)]

use tinker_pdf::colour_glyphs::ColourFace;
use tinker_pdf::shaping::{write_run, Run};
use tinker_pdf_cos::build::DocumentBuilder;
use tinker_pdf_font::{PaletteChoice, Sfnt};
use tinker_pdf_shape::bidi::BaseDirection;
use tinker_pdf_shape::Shaper;

use crate::epub_support::typeface::{Colour, Face};

/// A version 0 `COLR` (*COLR header*, *BaseGlyph and Layer records*): each
/// base glyph with its layers, bottom first, as glyph ids and palette
/// indices, the records in increasing glyph order as the chapter requires.
pub fn colr_v0(bases: &[(u16, Vec<(u16, u16)>)]) -> Vec<u8> {
    let layer_count: usize = bases.iter().map(|(_, layers)| layers.len()).sum();
    let base_at = 14usize;
    let layers_at = base_at + 6 * bases.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(bases.len() as u16).to_be_bytes());
    out.extend_from_slice(&(base_at as u32).to_be_bytes());
    out.extend_from_slice(&(layers_at as u32).to_be_bytes());
    out.extend_from_slice(&(layer_count as u16).to_be_bytes());
    let mut first = 0u16;
    for (glyph, layers) in bases {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&(layers.len() as u16).to_be_bytes());
        first += layers.len() as u16;
    }
    for (_, layers) in bases {
        for (glyph, index) in layers {
            out.extend_from_slice(&glyph.to_be_bytes());
            out.extend_from_slice(&index.to_be_bytes());
        }
    }
    out
}

/// A version 0 `CPAL` (*Palette table header*, *Palette entries and color
/// records*): palettes of the same number of entries, each given as RGBA and
/// written in the table's blue-green-red-alpha order, each palette's records
/// after the one before.
pub fn cpal(palettes: &[Vec<[u8; 4]>]) -> Vec<u8> {
    let entries = palettes.first().map_or(0, Vec::len);
    let records_at = 12 + 2 * palettes.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(entries as u16).to_be_bytes());
    out.extend_from_slice(&(palettes.len() as u16).to_be_bytes());
    out.extend_from_slice(&((entries * palettes.len()) as u16).to_be_bytes());
    out.extend_from_slice(&(records_at as u32).to_be_bytes());
    for palette in 0..palettes.len() {
        out.extend_from_slice(&((palette * entries) as u16).to_be_bytes());
    }
    for palette in palettes {
        for [r, g, b, a] in palette {
            out.extend_from_slice(&[*b, *g, *r, *a]);
        }
    }
    out
}

/// A `COLR` version 1 paint to write (*Paint tables*): a tree, each node one
/// table, a subtree written twice two tables — but for
/// [`V1::LayersOfOne`], whose layer slots all name one table.
#[derive(Clone, Debug)]
pub enum V1 {
    /// Format 1: a slice of the `LayerList`, bottom first.
    Layers(Vec<V1>),
    /// Format 1, its `n` slots all naming one table, so the reader reads one
    /// node with `n` parents.
    LayersOfOne(usize, Box<V1>),
    /// Format 2: a palette entry and an alpha.
    Solid(u16, f64),
    /// Format 3, at the default instance.
    VarSolid(u16, f64),
    /// Formats 4 and 5, the second at the default instance: `p0`, `p1` and
    /// `p2`, and the colour line.
    Linear {
        line: Line,
        points: [[i16; 2]; 3],
        variable: bool,
    },
    /// Formats 6 and 7: two circles, centre and radius each.
    Radial {
        line: Line,
        circles: [([i16; 2], u16); 2],
        variable: bool,
    },
    /// Formats 8 and 9: the centre, and the start and end angles in degrees,
    /// counter-clockwise, written biased as the formats store them.
    Sweep {
        line: Line,
        centre: [i16; 2],
        angles: [f64; 2],
        variable: bool,
    },
    /// Format 10: a glyph's outline as the child's clip.
    Glyph(u16, Box<V1>),
    /// Format 11: another base glyph's graph.
    ColrGlyph(u16),
    /// Format 12: an `Affine2x3`, `xx yx xy yy dx dy`.
    Transform([f64; 6], Box<V1>),
    /// Formats 14: a translation in font units.
    Translate(i16, i16, Box<V1>),
    /// Formats 16 and 18: scales along x and y, about a centre for 18.
    Scale(f64, f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 20 and 22.
    ScaleUniform(f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 24 and 26: an angle in units of 180°, counter-clockwise.
    Rotate(f64, Option<[i16; 2]>, Box<V1>),
    /// Formats 28 and 30: the x and y skew angles in units of 180°.
    Skew(f64, f64, Option<[i16; 2]>, Box<V1>),
    /// Format 32: source, `compositeMode`, backdrop.
    Composite(Box<V1>, u8, Box<V1>),
}

/// A colour line to write (*Color references, ColorStop and ColorLine*): its
/// `extend` byte, and its stops in the order the table gives them, each an
/// offset, a palette entry and an alpha.
#[derive(Clone, Debug)]
pub struct Line {
    pub extend: u8,
    pub stops: Vec<(f64, u16, f64)>,
}

/// `EXTEND_PAD`, `EXTEND_REPEAT` and `EXTEND_REFLECT`.
pub const PAD: u8 = 0;
pub const REPEAT: u8 = 1;
pub const REFLECT: u8 = 2;

/// A line of opaque stops.
pub fn line(extend: u8, stops: &[(f64, u16)]) -> Line {
    Line {
        extend,
        stops: stops.iter().map(|&(at, entry)| (at, entry, 1.0)).collect(),
    }
}

/// An `F2DOT14`.
pub fn f2dot14(value: f64) -> [u8; 2] {
    ((value * 16384.0).round() as i16).to_be_bytes()
}

/// A `Fixed`.
pub fn fixed(value: f64) -> [u8; 4] {
    ((value * 65536.0).round() as i32).to_be_bytes()
}

/// The paint tables of a `COLR`, written into one area, and the `LayerList`
/// slots its layer lists filled, each the area offset of a table.
#[derive(Default)]
pub struct V1Writer {
    area: Vec<u8>,
    layers: Vec<usize>,
}

impl V1Writer {
    /// Puts `value` as an `Offset24` at `at`.
    fn patch(&mut self, at: usize, value: usize) {
        let bytes = u32::try_from(value).expect("an offset").to_be_bytes();
        self.area[at..at + 3].copy_from_slice(&bytes[1..]);
    }

    /// A child offset to fill at `at`, from the table at `table`, with the
    /// child written now, after it.
    fn child(&mut self, table: usize, at: usize, child: &V1) {
        let written = self.write(child);
        self.patch(at, written - table);
    }

    fn centre(&mut self, centre: Option<[i16; 2]>) {
        if let Some([x, y]) = centre {
            self.area.extend_from_slice(&x.to_be_bytes());
            self.area.extend_from_slice(&y.to_be_bytes());
        }
    }

    /// A variable format's `varIndexBase`: no delta set, so the table draws
    /// at its default.
    fn var_index(&mut self, variable: bool) {
        if variable {
            self.area.extend_from_slice(&u32::MAX.to_be_bytes());
        }
    }

    /// The colour line of the gradient table at `table`, written now, after
    /// it, its offset patched into the table's `Offset24` at one: a
    /// `ColorLine`, or a `VarColorLine` whose stops each carry a
    /// `varIndexBase`.
    fn line(&mut self, table: usize, line: &Line, variable: bool) {
        let here = self.area.len();
        self.patch(table + 1, here - table);
        self.area.push(line.extend);
        self.area.extend_from_slice(
            &u16::try_from(line.stops.len())
                .expect("few stops")
                .to_be_bytes(),
        );
        for &(offset, entry, alpha) in &line.stops {
            self.area.extend_from_slice(&f2dot14(offset));
            self.area.extend_from_slice(&entry.to_be_bytes());
            self.area.extend_from_slice(&f2dot14(alpha));
            self.var_index(variable);
        }
    }

    /// Writes `paint` at the area's end, its children after it, each offset
    /// from the table that names it as *Paint tables* has it; returns where it
    /// starts.
    fn write(&mut self, paint: &V1) -> usize {
        let at = self.area.len();
        match paint {
            V1::Layers(children) => {
                let first = self.layers.len();
                self.layers.extend(std::iter::repeat_n(0, children.len()));
                self.area.extend_from_slice(&[1, children.len() as u8]);
                self.area.extend_from_slice(&(first as u32).to_be_bytes());
                for (i, child) in children.iter().enumerate() {
                    let written = self.write(child);
                    self.layers[first + i] = written;
                }
            }
            V1::LayersOfOne(count, child) => {
                let first = self.layers.len();
                self.layers.extend(std::iter::repeat_n(0, *count));
                self.area.extend_from_slice(&[1, *count as u8]);
                self.area.extend_from_slice(&(first as u32).to_be_bytes());
                let written = self.write(child);
                for slot in &mut self.layers[first..first + count] {
                    *slot = written;
                }
            }
            V1::Solid(entry, alpha) | V1::VarSolid(entry, alpha) => {
                let variable = matches!(paint, V1::VarSolid(..));
                self.area.push(if variable { 3 } else { 2 });
                self.area.extend_from_slice(&entry.to_be_bytes());
                self.area.extend_from_slice(&f2dot14(*alpha));
                if variable {
                    self.area.extend_from_slice(&0u32.to_be_bytes());
                }
            }
            V1::Linear {
                line,
                points,
                variable,
            } => {
                // *Formats 4 and 5*: the format, an `Offset24` to the colour
                // line, `x0 y0 x1 y1 x2 y2`, and for 5 a `varIndexBase`.
                self.area
                    .extend_from_slice(&[if *variable { 5 } else { 4 }, 0, 0, 0]);
                for [x, y] in points {
                    self.area.extend_from_slice(&x.to_be_bytes());
                    self.area.extend_from_slice(&y.to_be_bytes());
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Radial {
                line,
                circles,
                variable,
            } => {
                // *Formats 6 and 7*: `x0 y0 radius0 x1 y1 radius1`.
                self.area
                    .extend_from_slice(&[if *variable { 7 } else { 6 }, 0, 0, 0]);
                for ([x, y], radius) in circles {
                    self.area.extend_from_slice(&x.to_be_bytes());
                    self.area.extend_from_slice(&y.to_be_bytes());
                    self.area.extend_from_slice(&radius.to_be_bytes());
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Sweep {
                line,
                centre,
                angles,
                variable,
            } => {
                // *Formats 8 and 9*: the centre, and `startAngle` and
                // `endAngle` as `F2DOT14`s biased by one, 180° a unit.
                self.area
                    .extend_from_slice(&[if *variable { 9 } else { 8 }, 0, 0, 0]);
                for value in centre {
                    self.area.extend_from_slice(&value.to_be_bytes());
                }
                for degrees in angles {
                    self.area.extend_from_slice(&f2dot14(degrees / 180.0 - 1.0));
                }
                self.var_index(*variable);
                self.line(at, line, *variable);
            }
            V1::Glyph(glyph, child) => {
                self.area.extend_from_slice(&[10, 0, 0, 0]);
                self.area.extend_from_slice(&glyph.to_be_bytes());
                self.child(at, at + 1, child);
            }
            V1::ColrGlyph(glyph) => {
                self.area.push(11);
                self.area.extend_from_slice(&glyph.to_be_bytes());
            }
            V1::Transform(matrix, child) => {
                self.area.extend_from_slice(&[12, 0, 0, 0, 0, 0, 0]);
                let affine = self.area.len();
                for value in matrix {
                    self.area.extend_from_slice(&fixed(*value));
                }
                self.patch(at + 4, affine - at);
                self.child(at, at + 1, child);
            }
            V1::Translate(dx, dy, child) => {
                self.area.extend_from_slice(&[14, 0, 0, 0]);
                self.area.extend_from_slice(&dx.to_be_bytes());
                self.area.extend_from_slice(&dy.to_be_bytes());
                self.child(at, at + 1, child);
            }
            V1::Scale(x, y, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 18 } else { 16 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*x));
                self.area.extend_from_slice(&f2dot14(*y));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::ScaleUniform(scale, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 22 } else { 20 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*scale));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Rotate(angle, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 26 } else { 24 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*angle));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Skew(x, y, centre, child) => {
                self.area
                    .extend_from_slice(&[if centre.is_some() { 30 } else { 28 }, 0, 0, 0]);
                self.area.extend_from_slice(&f2dot14(*x));
                self.area.extend_from_slice(&f2dot14(*y));
                self.centre(*centre);
                self.child(at, at + 1, child);
            }
            V1::Composite(source, mode, backdrop) => {
                self.area.extend_from_slice(&[32, 0, 0, 0, *mode, 0, 0, 0]);
                self.child(at, at + 1, source);
                self.child(at, at + 5, backdrop);
            }
        }
        at
    }
}

/// A version 1 `COLR` (*COLR header*, *BaseGlyphList, LayerList and
/// ClipList*): the 34-byte header, a `BaseGlyphList` of `bases` (in
/// increasing glyph order, as the chapter requires), the `LayerList` their
/// layer lists filled, a format 1 `ClipList` of `clips` where there are any,
/// and then every paint table.
pub fn colr_v1(bases: &[(u16, V1)], clips: &[(u16, u16, [i16; 4])]) -> Vec<u8> {
    let mut writer = V1Writer::default();
    let roots: Vec<(u16, usize)> = bases
        .iter()
        .map(|(glyph, paint)| (*glyph, writer.write(paint)))
        .collect();
    let base_list_at = 34usize;
    let layer_list_at = base_list_at + 4 + 6 * roots.len();
    let clip_list_at = layer_list_at + 4 + 4 * writer.layers.len();
    let clip_list_len = if clips.is_empty() {
        0
    } else {
        5 + 16 * clips.len()
    };
    let area_at = clip_list_at + clip_list_len;

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(base_list_at as u32).to_be_bytes());
    out.extend_from_slice(&(layer_list_at as u32).to_be_bytes());
    let clip_offset = if clips.is_empty() { 0 } else { clip_list_at };
    out.extend_from_slice(&(clip_offset as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());

    out.extend_from_slice(&(roots.len() as u32).to_be_bytes());
    for (glyph, root) in &roots {
        out.extend_from_slice(&glyph.to_be_bytes());
        out.extend_from_slice(&((area_at + root - base_list_at) as u32).to_be_bytes());
    }
    out.extend_from_slice(&(writer.layers.len() as u32).to_be_bytes());
    for layer in &writer.layers {
        out.extend_from_slice(&((area_at + layer - layer_list_at) as u32).to_be_bytes());
    }
    if !clips.is_empty() {
        out.push(1);
        out.extend_from_slice(&(clips.len() as u32).to_be_bytes());
        for (i, (start, end, _)) in clips.iter().enumerate() {
            out.extend_from_slice(&start.to_be_bytes());
            out.extend_from_slice(&end.to_be_bytes());
            let box_at = 5 + 7 * clips.len() + 9 * i;
            out.extend_from_slice(&(box_at as u32).to_be_bytes()[1..]);
        }
        for (_, _, rect) in clips {
            out.push(1);
            for value in rect {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    out.extend_from_slice(&writer.area);
    out
}

/// A PNG of `width` × `height` RGBA pixels, row-major from the top: colour
/// type 6 at eight bits (11.2.2), every row of filter type 0 (9.2), one
/// `IDAT`, each chunk's CRC over its type and data (5.3).
pub fn rgba_png(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    png(width, height, pixels, true)
}

/// [`rgba_png`]'s picture with its alpha dropped: colour type 2, which a
/// writer can pass through with no soft mask.
pub fn rgb_png(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
    png(width, height, pixels, false)
}

/// A PNG of `pixels`, colour type 6 where `alpha` and 2 where not.
pub fn png(width: u32, height: u32, pixels: &[[u8; 4]], alpha: bool) -> Vec<u8> {
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(data.len()).expect("small").to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut both = kind.to_vec();
        both.extend_from_slice(data);
        out.extend_from_slice(&tinker_pdf_filters::crc32(&both).to_be_bytes());
        out
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if alpha { 6 } else { 2 }, 0, 0, 0]);
    let mut raw = Vec::new();
    for row in pixels.chunks(width as usize) {
        raw.push(0);
        for pixel in row {
            raw.extend_from_slice(if alpha { &pixel[..] } else { &pixel[..3] });
        }
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"IDAT", &tinker_pdf_filters::zlib_compress(&raw)));
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// A baseline RGB TIFF (TIFF 6.0, section 6), little-endian, uncompressed,
/// one strip: its nine required fields in tag order, `BitsPerSample`'s three
/// values after the directory, and the samples after them.
pub fn rgb_tiff(width: u16, height: u16, pixels: &[[u8; 3]]) -> Vec<u8> {
    let entries: u16 = 9;
    let directory_end = 8 + 2 + 12 * usize::from(entries) + 4;
    let bits_at = u32::try_from(directory_end).expect("small");
    let strip_at = bits_at + 6;
    let strip_len = u32::from(width) * u32::from(height) * 3;
    let mut out = b"II".to_vec();
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    // tag, type (3 SHORT, 4 LONG), count, value or offset.
    let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    };
    entry(256, 3, 1, u32::from(width)); // ImageWidth
    entry(257, 3, 1, u32::from(height)); // ImageLength
    entry(258, 3, 3, bits_at); // BitsPerSample, at its offset
    entry(259, 3, 1, 1); // Compression: none
    entry(262, 3, 1, 2); // PhotometricInterpretation: RGB
    entry(273, 4, 1, strip_at); // StripOffsets
    entry(277, 3, 1, 3); // SamplesPerPixel
    entry(278, 3, 1, u32::from(height)); // RowsPerStrip: one strip
    entry(279, 4, 1, strip_len); // StripByteCounts
    out.extend_from_slice(&0u32.to_le_bytes()); // no next directory
    for _ in 0..3 {
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    for pixel in pixels {
        out.extend_from_slice(pixel);
    }
    out
}

/// One glyph's record in an `sbix` strike (*Glyph data*).
#[derive(Clone, Debug)]
pub enum SbixRecord {
    /// A graphic: its origin offsets in the strike's pixels, its type and its
    /// bytes.
    Graphic([i16; 2], [u8; 4], Vec<u8>),
    /// `dupe`: the glyph whose bitmap this one uses.
    Dupe(u16),
}

/// An `sbix` (*Header*, *Strikes*, *Glyph data*): `flags`, and each strike's
/// ppem and the records of the glyphs it holds, over a face of `count`
/// glyphs, a glyph with no record holding no data.
pub fn sbix_table(flags: u16, strikes: &[(u16, Vec<(u16, SbixRecord)>)], count: u16) -> Vec<u8> {
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for (ppem, records) in strikes {
        let first = 4 + 4 * (usize::from(count) + 1);
        let mut offsets = Vec::new();
        let mut data = Vec::new();
        for glyph in 0..count {
            offsets.push(first + data.len());
            if let Some((_, record)) = records.iter().find(|(g, _)| *g == glyph) {
                match record {
                    SbixRecord::Graphic([x, y], tag, bytes) => {
                        data.extend_from_slice(&x.to_be_bytes());
                        data.extend_from_slice(&y.to_be_bytes());
                        data.extend_from_slice(tag);
                        data.extend_from_slice(bytes);
                    }
                    SbixRecord::Dupe(named) => {
                        data.extend_from_slice(&[0, 0, 0, 0]);
                        data.extend_from_slice(b"dupe");
                        data.extend_from_slice(&named.to_be_bytes());
                    }
                }
            }
        }
        offsets.push(first + data.len());
        let mut body = Vec::new();
        body.extend_from_slice(&ppem.to_be_bytes());
        body.extend_from_slice(&72u16.to_be_bytes()); // ppi
        for offset in offsets {
            body.extend_from_slice(&u32::try_from(offset).expect("small").to_be_bytes());
        }
        body.extend_from_slice(&data);
        bodies.push(body);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes()); // version
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&u32::try_from(strikes.len()).expect("few").to_be_bytes());
    let mut at = 8 + 4 * strikes.len();
    for body in &bodies {
        out.extend_from_slice(&u32::try_from(at).expect("small").to_be_bytes());
        at += body.len();
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// One glyph of a `CBDT` strike (*Glyph bitmap data formats*): its id, its
/// data format, its metrics as `BigGlyphMetrics` lays them out — height,
/// width, horizontal bearings and advance, vertical bearings and advance — and
/// its PNG. Format 17 writes the first five as `SmallGlyphMetrics`, 18 all
/// eight, and 19 none, its index subtable (format 2) carrying them.
#[derive(Clone, Debug)]
pub struct CbdtEntry {
    pub glyph: u16,
    pub format: u16,
    pub metrics: [u8; 8],
    pub png: Vec<u8>,
}

/// A `CBLC` and its `CBDT` (*CblcHeader*, *BitmapSize record*,
/// *IndexSubtableList*): each strike its `ppemX` and `ppemY`, its glyphs each
/// one index subtable of a one-glyph range — format 1 for data formats 17 and
/// 18, format 2 for 19 — at bit depth 32 with horizontal metrics.
pub fn cbdt_tables(strikes: &[([u8; 2], Vec<CbdtEntry>)]) -> (Vec<u8>, Vec<u8>) {
    let mut cbdt = vec![0, 3, 0, 0];
    let mut lists: Vec<(Vec<u8>, u32, u16, u16)> = Vec::new();
    for (_, entries) in strikes {
        let records = 8 * entries.len();
        let mut array = Vec::new();
        let mut subtables = Vec::new();
        for entry in entries {
            let at = cbdt.len();
            match entry.format {
                17 => cbdt.extend_from_slice(&entry.metrics[..5]),
                18 => cbdt.extend_from_slice(&entry.metrics),
                _ => {}
            }
            cbdt.extend_from_slice(&u32::try_from(entry.png.len()).expect("small").to_be_bytes());
            cbdt.extend_from_slice(&entry.png);
            let size = u32::try_from(cbdt.len() - at).expect("small");
            let at = u32::try_from(at).expect("small");
            array.extend_from_slice(&entry.glyph.to_be_bytes());
            array.extend_from_slice(&entry.glyph.to_be_bytes());
            array.extend_from_slice(
                &u32::try_from(records + subtables.len())
                    .expect("small")
                    .to_be_bytes(),
            );
            if entry.format == 19 {
                subtables.extend_from_slice(&2u16.to_be_bytes());
                subtables.extend_from_slice(&19u16.to_be_bytes());
                subtables.extend_from_slice(&at.to_be_bytes());
                subtables.extend_from_slice(&size.to_be_bytes());
                subtables.extend_from_slice(&entry.metrics);
            } else {
                subtables.extend_from_slice(&1u16.to_be_bytes());
                subtables.extend_from_slice(&entry.format.to_be_bytes());
                subtables.extend_from_slice(&at.to_be_bytes());
                subtables.extend_from_slice(&0u32.to_be_bytes());
                subtables.extend_from_slice(&size.to_be_bytes());
            }
        }
        array.extend_from_slice(&subtables);
        let first = entries.iter().map(|e| e.glyph).min().unwrap_or(0);
        let last = entries.iter().map(|e| e.glyph).max().unwrap_or(0);
        lists.push((
            array,
            u32::try_from(entries.len()).expect("few"),
            first,
            last,
        ));
    }
    let mut cblc = vec![0, 3, 0, 0];
    cblc.extend_from_slice(&u32::try_from(strikes.len()).expect("few").to_be_bytes());
    let mut at = 8 + 48 * strikes.len();
    for (([ppem_x, ppem_y], _), (list, count, first, last)) in strikes.iter().zip(&lists) {
        cblc.extend_from_slice(&u32::try_from(at).expect("small").to_be_bytes());
        cblc.extend_from_slice(&u32::try_from(list.len()).expect("small").to_be_bytes());
        cblc.extend_from_slice(&count.to_be_bytes());
        cblc.extend_from_slice(&0u32.to_be_bytes()); // colorRef
        cblc.extend_from_slice(&[0; 24]); // hori and vert SbitLineMetrics
        cblc.extend_from_slice(&first.to_be_bytes());
        cblc.extend_from_slice(&last.to_be_bytes());
        cblc.extend_from_slice(&[*ppem_x, *ppem_y, 32, 0x01]);
        at += list.len();
    }
    for (list, ..) in &lists {
        cblc.extend_from_slice(list);
    }
    (cblc, cbdt)
}

/// An `SVG ` table (*SVG table formats*): each record a glyph range and its
/// document, written as given — plain, or gzip-encoded by [`gzip`] — after
/// the record array, in record order.
pub fn svg_table(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let list_at = 10usize;
    let first = 2 + 12 * records.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes()); // version
    out.extend_from_slice(&u32::try_from(list_at).expect("small").to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&u16::try_from(records.len()).expect("few").to_be_bytes());
    let mut at = first;
    for (start, end, document) in records {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&u32::try_from(at).expect("small").to_be_bytes());
        out.extend_from_slice(&u32::try_from(document.len()).expect("small").to_be_bytes());
        at += document.len();
    }
    for (_, _, document) in records {
        out.extend_from_slice(document);
    }
    out
}

/// One RFC 1952 gzip member of `data`: the ten-byte header with no optional
/// field, the RFC 1951 deflate data — a zlib stream's, its two-byte header and
/// its Adler-32 taken off — and the CRC-32 and size of `data`.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let zlib = tinker_pdf_filters::zlib_compress(data);
    let deflate = &zlib[2..zlib.len() - 4];
    let mut out = vec![0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 255];
    out.extend_from_slice(deflate);
    out.extend_from_slice(&tinker_pdf_filters::crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).expect("small").to_le_bytes());
    out
}

/// The document of [`colour_glyphs_page`]'s `SVG ` glyph, the face's `C`:
/// a filled path, a rectangle filled with a gradient, and a `<use>` of a
/// defined square, in font units with the y axis down.
pub const PAGE_SVG: &str = r##"<svg version="1.1" xmlns="http://www.w3.org/2000/svg"
    xmlns:xlink="http://www.w3.org/1999/xlink">
  <defs>
    <linearGradient id="ramp"><stop offset="0" stop-color="#e67828"/>
      <stop offset="1" stop-color="#286ed2"/></linearGradient>
    <rect id="square" width="200" height="200" fill="#3cc85a"/>
  </defs>
  <g id="glyph3">
    <path d="M100 -100 H900 V-300 H100 Z" fill="#c83caa"/>
    <rect x="100" y="-900" width="800" height="500" fill="url(#ramp)"/>
    <use xlink:href="#square" x="400" y="-800"/>
  </g>
</svg>"##;

/// **A page of colour glyphs, one of each kind the writer draws**, for
/// `determinism.rs`'s fingerprint: `A` a `COLR` version 1 graph — a linear
/// gradient multiplied over a radial one through a composite, each in its
/// glyph's outline — `B` an `sbix` PNG with an alpha, and `C` an `SVG `
/// element ([`PAGE_SVG`]), written through `shaping::write_run` at 40 points
/// on a page 200 by 80.
pub fn colour_glyphs_page() -> Vec<u8> {
    let base = Face::new("Fixture Colour Page", "ABC").with_advance(1000);
    let letter = |c: char| base.glyph_of(c).expect("a covered letter");
    let rects: Vec<[i16; 4]> = vec![[100, 100, 700, 900], [300, 100, 900, 700]];
    let count = base.glyph_count(rects.len());
    let (outer, inner) = (base.colour_glyph(0), base.colour_glyph(1));
    let source = V1::Glyph(
        outer,
        Box::new(V1::Linear {
            line: line(REFLECT, &[(0.0, 0), (0.5, 1)]),
            points: [[100, 100], [700, 300], [100, 900]],
            variable: false,
        }),
    );
    let backdrop = V1::Glyph(
        inner,
        Box::new(V1::Radial {
            line: Line {
                extend: PAD,
                stops: vec![(0.0, 2, 1.0), (1.0, 1, 0.5)],
            },
            circles: [([600, 400], 0), ([600, 400], 350)],
            variable: false,
        }),
    );
    // 23, COMPOSITE_MULTIPLY.
    let colr = colr_v1(
        &[(
            letter('A'),
            V1::Composite(Box::new(source), 23, Box::new(backdrop)),
        )],
        &[],
    );
    let picture: Vec<[u8; 4]> = (0..64)
        .map(|at| {
            let (x, y) = (at % 8, at / 8);
            match (x < 4, y < 4) {
                (true, true) => [230, 120, 40, 255],
                (false, true) => [60, 200, 90, 255],
                (true, false) => [40, 110, 210, 255],
                (false, false) => [200, 60, 170, 128],
            }
        })
        .collect();
    let sbix = sbix_table(
        0x0001,
        &[(
            20,
            vec![(
                letter('B'),
                SbixRecord::Graphic([2, 1], *b"png ", rgba_png(8, 8, &picture)),
            )],
        )],
        count,
    );
    let svg = svg_table(&[(letter('C'), letter('C'), PAGE_SVG.as_bytes().to_vec())]);
    let face = base.with_colour(Colour {
        rectangles: rects,
        colr,
        cpal: cpal(&[vec![
            [230, 120, 40, 255],
            [40, 110, 210, 255],
            [200, 60, 170, 255],
        ]]),
        sbix,
        svg,
        ..Colour::default()
    });
    let program = face.build();
    let sfnt = Sfnt::parse(&program).expect("the fixture face is a valid sfnt");
    let colour = ColourFace::new(&sfnt, PaletteChoice::Default);
    let mut builder = DocumentBuilder::new();
    assert!(colour.register(&mut builder, b"C0", b"FixtureColourPage"));
    let mut content = b"0 0.5 0 rg\n".to_vec();
    assert!(write_run(
        &mut builder,
        &mut content,
        &Run {
            font: b"C0",
            shaper: Shaper::new(&sfnt),
            size: 40.0,
            matrix: [1.0, 0.0, 0.0, 1.0, 20.0, 20.0],
            text: "ABC",
            direction: BaseDirection::LeftToRight,
            colour: Some(&colour),
        },
    ));
    assert!(
        colour.warnings().is_empty(),
        "every glyph is drawn: {:?}",
        colour.warnings()
    );
    builder.add_page(200.0, 80.0, |page| page.raw(&content));
    builder.finish()
}
