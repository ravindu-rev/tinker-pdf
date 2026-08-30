//! A codestream builder for the tests, and only for the tests.
//!
//! Every fixture that carries *pixels* is a WIC-encoded file under
//! `crates/tinker-pdf-filters/tests/jxr/`, because pixels are what the
//! lossless identity adjudicates and nothing this repository writes could
//! adjudicate them. What this builder is for is the other half: reaching a
//! header state no encoder on this machine will emit — a reserved
//! `OVERLAP_MODE`, a tile grid that overruns the image, an index table with
//! the wrong start code — so that the refusals can be tested at all.
//!
//! It writes headers and stops. It never writes a coefficient.

use crate::jxr::container::GDI_SIGNATURE;

/// MSB-first bit writer, the mirror of `bitstream::BitReader`.
pub(super) struct BitWriter {
    bytes: Vec<u8>,
    /// Bits used in the byte under construction, 0..8.
    used: u32,
}

impl BitWriter {
    pub(super) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            used: 0,
        }
    }

    pub(super) fn write(&mut self, value: u64, bits: u32) {
        assert!(bits <= 64, "a field wider than the accumulator");
        for i in (0..bits).rev() {
            let bit = ((value >> i) & 1) as u8;
            if self.used == 0 {
                self.bytes.push(0);
            }
            if let Some(last) = self.bytes.last_mut() {
                *last |= bit << (7 - self.used);
            }
            self.used = (self.used + 1) % 8;
        }
    }

    pub(super) fn flag(&mut self, value: bool) {
        self.write(u64::from(value), 1);
    }

    pub(super) fn align(&mut self) {
        while self.used != 0 {
            self.write(0, 1);
        }
    }

    pub(super) fn finish(mut self) -> Vec<u8> {
        self.align();
        self.bytes
    }
}

/// The knobs the refusal tests turn. Everything else is fixed at the
/// smallest legal value, so a test that changes one field changes one thing.
pub(super) struct Codestream {
    pub(super) tiling: bool,
    pub(super) frequency_mode: bool,
    pub(super) index_table: bool,
    pub(super) overlap_mode: u64,
    pub(super) windowing: bool,
    pub(super) alpha_plane: bool,
    pub(super) output_clr_fmt: u64,
    pub(super) output_bitdepth: u64,
    pub(super) internal_clr_fmt: u64,
    pub(super) bands_present: u64,
    pub(super) reserved_b: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) num_ver_tiles_minus1: u64,
    pub(super) num_hor_tiles_minus1: u64,
    pub(super) tile_widths: Vec<u64>,
    pub(super) tile_heights: Vec<u64>,
    pub(super) top_margin: u64,
    pub(super) left_margin: u64,
    pub(super) index_start_code: u64,
    /// Extra bytes appended after the headers, standing in for tile data.
    pub(super) trailing: usize,
}

impl Default for Codestream {
    fn default() -> Self {
        Self {
            tiling: false,
            frequency_mode: false,
            index_table: false,
            overlap_mode: 0,
            windowing: false,
            alpha_plane: false,
            // Table 22 row 7: RGB.
            output_clr_fmt: 7,
            // Table 23 row 1: BD8.
            output_bitdepth: 1,
            // Table 28 row 3: YUV444.
            internal_clr_fmt: 3,
            // Table 29 row 0: ALL.
            bands_present: 0,
            reserved_b: 1,
            width: 16,
            height: 16,
            num_ver_tiles_minus1: 0,
            num_hor_tiles_minus1: 0,
            tile_widths: Vec::new(),
            tile_heights: Vec::new(),
            top_margin: 0,
            left_margin: 0,
            index_start_code: 0x0001,
            trailing: 64,
        }
    }
}

impl Codestream {
    /// Writes `IMAGE_HEADER( )`, `IMAGE_PLANE_HEADER( )`, the index table and
    /// `SubsequentBytes`, then `trailing` zero bytes.
    pub(super) fn build(&self) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write(u64::from_be_bytes(GDI_SIGNATURE), 64);
        w.write(self.reserved_b, 4);
        w.flag(false); // HARD_TILING_FLAG
        w.write(1, 3); // RESERVED_C
        w.flag(self.tiling);
        w.flag(self.frequency_mode);
        w.write(0, 3); // SPATIAL_XFRM_SUBORDINATE
        w.flag(self.index_table);
        w.write(self.overlap_mode, 2);
        w.flag(true); // SHORT_HEADER_FLAG: 16-bit dimensions
        w.flag(false); // LONG_WORD_FLAG
        w.flag(self.windowing);
        w.flag(false); // TRIM_FLEXBITS_FLAG
        w.flag(false); // RESERVED_D
        w.flag(false); // RED_BLUE_NOT_SWAPPED_FLAG
        w.flag(false); // PREMULTIPLIED_ALPHA_FLAG
        w.flag(self.alpha_plane);
        w.write(self.output_clr_fmt, 4);
        w.write(self.output_bitdepth, 4);
        w.write(u64::from(self.width - 1), 16);
        w.write(u64::from(self.height - 1), 16);
        if self.tiling {
            w.write(self.num_ver_tiles_minus1, 12);
            w.write(self.num_hor_tiles_minus1, 12);
        }
        for &t in &self.tile_widths {
            w.write(t, 8);
        }
        for &t in &self.tile_heights {
            w.write(t, 8);
        }
        if self.windowing {
            w.write(self.top_margin, 6);
            w.write(self.left_margin, 6);
            // 8.3.21 and 8.3.22 require the *extended* dimensions to be a
            // multiple of 16, so the far margins absorb whatever the near
            // ones added.
            let bottom = (16 - ((u64::from(self.height) + self.top_margin) % 16)) % 16;
            let right = (16 - ((u64::from(self.width) + self.left_margin) % 16)) % 16;
            w.write(bottom, 6);
            w.write(right, 6);
        }

        self.plane_header(&mut w);
        if self.alpha_plane {
            // 8.4.2: an alpha plane's INTERNAL_CLR_FMT shall be YONLY.
            let mut alpha = Self {
                internal_clr_fmt: 0,
                ..Self::default()
            };
            alpha.bands_present = self.bands_present;
            alpha.plane_header(&mut w);
        }

        if self.index_table {
            w.write(self.index_start_code, 16);
            let tiles = (self.num_ver_tiles_minus1 + 1) * (self.num_hor_tiles_minus1 + 1);
            let bands: u64 = match self.bands_present {
                0 => 4,
                1 => 3,
                2 => 2,
                _ => 1,
            };
            let entries = if self.frequency_mode {
                tiles * bands
            } else {
                tiles
            };
            for _ in 0..entries {
                // A two-byte VLW_ESC of zero.
                w.write(0, 16);
            }
        }
        // 8.2.1's SubsequentBytes: zero, so no profile/level block follows.
        w.write(0, 16);

        let mut out = w.finish();
        out.extend(core::iter::repeat_n(0u8, self.trailing));
        out
    }

    fn plane_header(&self, w: &mut BitWriter) {
        w.write(self.internal_clr_fmt, 3);
        w.flag(false); // SCALED_FLAG
        w.write(self.bands_present, 4);
        match self.internal_clr_fmt {
            3 => {
                w.write(0, 4); // RESERVED_F
                w.write(0, 4); // RESERVED_H
            }
            1 => {
                w.flag(false); // RESERVED_E_BIT
                w.write(0, 3); // CHROMA_CENTERING_X
                w.flag(false); // RESERVED_G_BIT
                w.write(0, 3); // CHROMA_CENTERING_Y
            }
            2 => {
                w.flag(false);
                w.write(0, 3);
                w.write(0, 4); // RESERVED_H
            }
            6 => {
                w.write(2, 4); // NUM_COMPONENTS_MINUS1: three components
                w.write(0, 4); // RESERVED_H
            }
            _ => {}
        }
        // OUTPUT_BITDEPTH BD16/BD16S/BD32S carry SHIFT_BITS (8.4.13).
        if matches!(self.output_bitdepth, 2 | 3 | 6) {
            w.write(0, 8);
        }
        if self.output_bitdepth == 7 {
            w.write(0, 8); // LEN_MANTISSA
            w.write(0, 8); // EXP_BIAS
        }
        let components: u64 = match self.internal_clr_fmt {
            0 => 1,
            4 => 4,
            6 => 3,
            _ => 3,
        };
        w.flag(true); // DC_IMAGE_PLANE_UNIFORM_FLAG
        Self::qp(w, components);
        if self.bands_present != 3 {
            w.flag(false); // RESERVED_I_BIT
            w.flag(true); // LP_IMAGE_PLANE_UNIFORM_FLAG
            Self::qp(w, components);
            if self.bands_present != 2 {
                w.flag(false); // RESERVED_J_BIT
                w.flag(true); // HP_IMAGE_PLANE_UNIFORM_FLAG
                Self::qp(w, components);
            }
        }
        w.align();
    }

    /// A UNIFORM QP set of 1, which is what lossless coding uses (9.8).
    fn qp(w: &mut BitWriter, components: u64) {
        if components != 1 {
            w.write(0, 2); // COMPONENT_MODE = UNIFORM
        }
        w.write(1, 8);
    }
}

/// Wraps a codestream in the smallest legal Annex A file.
///
/// The directory carries exactly Table A.4's four Required tags plus the
/// pixel format, in ascending tag order (A.7.2).
pub(super) fn wrap(codestream: &[u8], guid_tail: u8) -> Vec<u8> {
    const HEADER: usize = 8;
    let entries: u16 = 5;
    let ifd_at = HEADER;
    let dir_len = 2 + 12 * usize::from(entries) + 4;
    let guid_at = ifd_at + dir_len;
    let image_at = guid_at + 16;

    let mut out = Vec::new();
    out.extend_from_slice(&[0x49, 0x49, 0xBC, 0x01]);
    out.extend_from_slice(&(ifd_at as u32).to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());

    let entry = |tag: u16, ty: u16, count: u32, value: u32, out: &mut Vec<u8>| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    };
    // 0xBC01 PIXEL_FORMAT, 0xBC80 IMAGE_WIDTH, 0xBC81 IMAGE_HEIGHT,
    // 0xBCC0 IMAGE_OFFSET, 0xBCC1 IMAGE_BYTE_COUNT.
    entry(0xBC01, 1, 16, guid_at as u32, &mut out);
    entry(0xBC80, 4, 1, 16, &mut out);
    entry(0xBC81, 4, 1, 16, &mut out);
    entry(0xBCC0, 4, 1, image_at as u32, &mut out);
    entry(0xBCC1, 4, 1, codestream.len() as u32, &mut out);
    out.extend_from_slice(&0u32.to_le_bytes()); // ZERO_OR_NEXT_IFD_OFFSET

    out.extend_from_slice(&[
        0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
        guid_tail,
    ]);
    out.extend_from_slice(codestream);
    out
}
