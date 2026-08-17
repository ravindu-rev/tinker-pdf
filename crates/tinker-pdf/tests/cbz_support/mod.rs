//! Fixtures for gap 29 milestone 5: archives, images and hand-written PDFs.
//!
//! There is no `.cbz` in this repository, so every archive here is built from
//! APPNOTE 6.3.10's field layouts and every image from its own specification's.
//! That is deliberate rather than a shortcut: a reader tested only against what
//! its own writer emits proves less than it looks, which is the trap gap 18
//! milestone 6 fell into for milestones.
//!
//! The PDF writer at the bottom is the point of the file. `tests/cbz.rs` has to
//! compare a synthesised document against "the same image embedded by hand",
//! and a hand-built side that went through [`tinker_pdf_cos::DocumentBuilder`]
//! would share the very code under test — gap 18 milestone 7's scar, where a
//! comparison passed because both sides rendered through one defect. So the
//! comparison's other side is PDF source text, assembled here.
//!
//! # Why the allow
//!
//! Two test binaries include this module — `cbz.rs` and `cbz_qpdf.rs` — and
//! each compiles its own copy. Every item here is used by at least one of them
//! and none is used by both, so whichever binary does not use a given fixture
//! reports it dead. The alternative is splitting the file along a seam that
//! exists only to satisfy the lint, which would put the ZIP writer and the PDF
//! writer in different places for no reason a reader would recognise.

#![allow(
    dead_code,
    reason = "shared by two test binaries; each uses a different subset"
)]

use tinker_pdf_filters::{crc32, deflate, zlib_compress};

// ---- ZIP ---------------------------------------------------------------

/// One entry to write.
pub struct ZipFile {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// Method 8 rather than method 0.
    pub deflated: bool,
    /// General-purpose bit 0. Nothing is actually enciphered; the bit is what
    /// the reader refuses on, and refusing before touching the bytes is the
    /// behaviour under test.
    pub encrypted: bool,
    /// General-purpose bit 3: the sizes and CRC follow the data.
    pub streamed: bool,
    /// Corrupt the stored CRC-32, so the entry fails its only integrity check.
    pub bad_crc: bool,
}

impl ZipFile {
    pub fn stored(name: &str, data: &[u8]) -> ZipFile {
        ZipFile {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            deflated: false,
            encrypted: false,
            streamed: false,
            bad_crc: false,
        }
    }

    pub fn deflated(name: &str, data: &[u8]) -> ZipFile {
        ZipFile {
            deflated: true,
            ..ZipFile::stored(name, data)
        }
    }

    pub fn encrypted(mut self) -> ZipFile {
        self.encrypted = true;
        self
    }

    pub fn corrupt(mut self) -> ZipFile {
        self.bad_crc = true;
        self
    }

    fn flags(&self) -> u16 {
        let mut flags = 1u16 << 11; // bit 11: the name is UTF-8.
        if self.encrypted {
            flags |= 1;
        }
        if self.streamed {
            flags |= 1 << 3;
        }
        flags
    }

    fn payload(&self) -> Vec<u8> {
        if self.deflated {
            deflate(&self.data)
        } else {
            self.data.clone()
        }
    }
}

/// How the archive should be damaged, if at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    None,
    /// No end-of-central-directory record and no recoverable local header:
    /// four bytes of signature and nothing behind them.
    Shredded,
    /// 4.3.16's disk numbers say this is one volume of several.
    MultiDisk,
    /// The entry count is the 0xFFFF sentinel that means "read the Zip64
    /// record", and there is no Zip64 record.
    Zip64Sentinel,
}

const LOCAL_SIG: [u8; 4] = [b'P', b'K', 3, 4];
const CENTRAL_SIG: [u8; 4] = [b'P', b'K', 1, 2];
const EOCD_SIG: [u8; 4] = [b'P', b'K', 5, 6];
const DESCRIPTOR_SIG: [u8; 4] = [b'P', b'K', 7, 8];

/// Builds an archive.
pub fn zip(files: &[ZipFile], damage: Damage) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for file in files {
        let offset = out.len() as u32;
        let payload = file.payload();
        let mut crc = crc32(&file.data);
        if file.bad_crc {
            crc ^= 0xFFFF_FFFF;
        }
        let (header_crc, header_c, header_u) = if file.streamed {
            (0, 0, 0)
        } else {
            (crc, payload.len() as u32, file.data.len() as u32)
        };

        out.extend_from_slice(&LOCAL_SIG);
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&file.flags().to_le_bytes());
        out.extend_from_slice(&if file.deflated { 8u16 } else { 0 }.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&header_crc.to_le_bytes());
        out.extend_from_slice(&header_c.to_le_bytes());
        out.extend_from_slice(&header_u.to_le_bytes());
        out.extend_from_slice(&(file.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(&file.name);
        out.extend_from_slice(&payload);

        if file.streamed {
            out.extend_from_slice(&DESCRIPTOR_SIG);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(file.data.len() as u32).to_le_bytes());
        }

        central.extend_from_slice(&CENTRAL_SIG);
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&file.flags().to_le_bytes());
        central.extend_from_slice(&if file.deflated { 8u16 } else { 0 }.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&(file.data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(file.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&file.name);
    }

    if damage == Damage::Shredded {
        // A local signature and nothing that parses behind it, so the scan
        // finds structure and recovers no entry.
        return LOCAL_SIG.to_vec();
    }

    let directory_at = out.len() as u32;
    out.extend_from_slice(&central);

    let count = match damage {
        Damage::Zip64Sentinel => 0xFFFFu16,
        _ => files.len() as u16,
    };
    let disk = u16::from(damage == Damage::MultiDisk);

    out.extend_from_slice(&EOCD_SIG);
    out.extend_from_slice(&disk.to_le_bytes()); // this disk
    out.extend_from_slice(&disk.to_le_bytes()); // directory disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

// ---- PNG ---------------------------------------------------------------

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 5.3: length, type, data, CRC over the type and the data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut both = kind.to_vec();
    both.extend_from_slice(data);
    out.extend_from_slice(&crc32(&both).to_be_bytes());
    out
}

/// An 8-bit RGB PNG of `pixels`, row-major from the top, three bytes a pixel.
///
/// The rows cycle 9.2's five filters rather than all being type 0: a fixture
/// filtered entirely with zero is a weaker stream than any real encoder
/// produces, and `/Predictor 15` exists precisely because the tag changes from
/// row to row.
pub fn rgb_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

    let stride = width as usize * 3;
    let mut raw = Vec::new();
    let mut previous = vec![0u8; stride];
    for y in 0..height as usize {
        let row = &pixels[y * stride..(y + 1) * stride];
        let tag = (y % 5) as u8;
        raw.push(tag);
        for i in 0..stride {
            let a = if i >= 3 { i32::from(row[i - 3]) } else { 0 };
            let b = i32::from(previous[i]);
            let c = if i >= 3 {
                i32::from(previous[i - 3])
            } else {
                0
            };
            let predictor = match tag {
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => paeth(a, b, c),
                _ => 0,
            };
            raw.push((i32::from(row[i]) - predictor) as u8);
        }
        previous.copy_from_slice(row);
    }

    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"IDAT", &zlib_compress(&raw)));
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// PNG 9.4's Paeth predictor, written out so the encoder above is 9.2's own.
fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// A distinct colour per pixel, so a transposed or reversed raster is visible.
///
/// Gap 17 transposed two JBIG2 context bits and the published test vector still
/// decoded byte for byte, because the relabelling was a bijection the fixture
/// could not see. A picture whose every pixel differs from every other cannot
/// hide one.
pub fn distinct_pixels(width: u32, height: u32) -> Vec<u8> {
    assert!(
        width * height <= 1 << 16,
        "the encoding below needs 16 bits"
    );
    let mut out = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            // Red and green together are the pixel's row-major position, which
            // is unique by construction; blue is a third function of both axes,
            // so a channel dropped or a pair swapped is visible as well.
            let index = y * width + x;
            out.push((index & 0xFF) as u8);
            out.push(((index >> 8) & 0xFF) as u8);
            out.push(((x.wrapping_mul(37).wrapping_add(y.wrapping_mul(11)) & 0xFF) as u8) ^ 0x5A);
        }
    }
    out
}

/// A PNG signature with nothing legible behind it.
pub fn broken_png() -> Vec<u8> {
    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(b"not a chunk at all, no IHDR anywhere");
    out
}

// ---- JPEG --------------------------------------------------------------

/// Writes Huffman-coded bits, stuffing a zero after every 0xFF (B.1.1.5).
struct Bits {
    out: Vec<u8>,
    byte: u8,
    used: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            out: Vec::new(),
            byte: 0,
            used: 0,
        }
    }

    fn push(&mut self, bits: &str) {
        for c in bits.chars() {
            self.byte = (self.byte << 1) | u8::from(c == '1');
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.byte);
                if self.byte == 0xFF {
                    self.out.push(0x00);
                }
                self.byte = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        while self.used != 0 {
            self.push("1");
        }
        self.out
    }
}

/// A baseline grey JPEG, DC only, every block the same dark value.
///
/// `width` and `height` must be multiples of eight. Two DC codes are defined so
/// a multi-block image can say "no change" after the first, which is what makes
/// this a fixture rather than an 8 x 8 special case.
pub fn grey_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];

    // DQT, table 0. A large DC quantiser so the one coded coefficient is a
    // value nobody could mistake for a blank page.
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend_from_slice(&quant);

    // SOF0: baseline, 8-bit, one component, no subsampling, table 0.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);

    // DHT DC table 0: two two-bit codes, `00` for size 0 and `01` for size 2.
    let mut counts = [0u8; 16];
    counts[1] = 2;
    let mut dht = vec![0x00];
    dht.extend_from_slice(&counts);
    dht.extend_from_slice(&[0x00, 0x02]);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // DHT AC table 0: one one-bit code, end-of-block.
    let mut counts = [0u8; 16];
    counts[0] = 1;
    let mut dht = vec![0x10];
    dht.extend_from_slice(&counts);
    dht.push(0x00);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // SOS: one component, the whole spectral band, no successive
    // approximation — which is what makes it sequential rather than
    // progressive.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);

    let blocks = (usize::from(width).div_ceil(8)) * (usize::from(height).div_ceil(8));
    let mut bits = Bits::new();
    for block in 0..blocks {
        if block == 0 {
            // Size 2, then the two bits `00`, which F.1.2.1's table reads as
            // -3: a DC of -765 after dequantisation, which the level shift
            // puts near 32.
            bits.push("01");
            bits.push("00");
        } else {
            bits.push("00");
        }
        bits.push("0");
    }
    out.extend_from_slice(&bits.finish());

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

// ---- A PDF written out by hand -----------------------------------------

/// Assembles a PDF from object bodies, numbered from one.
///
/// No `DocumentBuilder` anywhere: this is the independent side of every
/// comparison in `tests/cbz.rs`, and a comparison whose two sides share the
/// code under test is gap 18 milestone 7's scar.
pub fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::from(&b"%PDF-1.7\n"[..]);
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_at
        )
        .as_bytes(),
    );
    out
}

/// A stream object: a dictionary without its closing `>>`, then the bytes.
pub fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// One page, one image, drawn over the whole of a `/MediaBox` of the image's
/// own pixel size — gap 29's geometry, written out rather than derived.
pub fn one_image_page(width: u32, height: u32, image_dict: &str, image_data: &[u8]) -> Vec<u8> {
    let content = format!("q {width} 0 0 {height} 0 0 cm /Im Do Q\n");
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
        stream(image_dict, image_data),
    ])
}
