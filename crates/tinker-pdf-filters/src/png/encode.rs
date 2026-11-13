//! PNG encoding (ISO/IEC 15948 clauses 5, 9 and 11) — the writer half of
//! [`super`], and the only image encoder in this crate.
//!
//! # Why it is here and not in the tool that wanted it
//!
//! `tools/tpdf`'s `write_pnm` carried the argument against this for as long as
//! it existed: "a PNG writer would mean a deflate encoder in a test tool, and
//! the engine already has one it should not depend on from here." The premise
//! was right and the conclusion was backwards. Everything a PNG writer needs is
//! *already in this crate and nowhere else*: [`crate::ZlibDeflater`] writes
//! RFC 1950, which is byte for byte what 10.3 puts inside IDAT; [`Crc32`] is 5.3's
//! per-chunk checksum with the resumable shape 5.3 needs, because the checksum
//! covers a chunk's type and its data and those are never adjacent in one
//! buffer; and `predictors.rs` already owns 9.2's predictor because PDF's
//! `/Predictor 15` *is* 9.2, adopted wholesale. A writer anywhere else would
//! have had to reach for all three or copy all three.
//!
//! So it sits beside the decoder, which is also what lets
//! `tests/png_suite.rs` hold the encoder to PngSuite directly rather than
//! through a facade.
//!
//! # Rows in bands, one encoder
//!
//! [`PngWriter`] takes a raster's rows a band at a time and hands back each
//! chunk as it closes, so a caller drawing a page in bands writes its file as
//! the bands arrive (`docs/design/banded-output.md`). [`png_encode`] is that
//! writer fed every row at once: the writes the deflater sees, and so the
//! blocks it completes and the IDAT chunks they close, are the same whatever
//! band a row arrives in, and a file written in bands is the file written
//! whole, byte for byte.
//!
//! # Eight bits, and the round trip says so
//!
//! [`PngSource`] carries 8-bit components only. Table 11.1 permits 16 for four
//! of the five colour types and this writer never emits one, because the thing
//! it was built to write is a `tinker_pdf::Bitmap` and **`PixelFormat` has no
//! 16-bit variant**: every buffer in `tinker-pdf-raster` is a byte a channel.
//! Encoding at 16 would mean widening samples that carry eight bits of
//! information into two bytes that carry eight bits of information, doubling
//! every file to say nothing new.
//!
//! The consequence is stated here rather than left for a reader to discover in
//! a failing assertion: **a 16-bit PNG decoded and re-encoded through this
//! module comes back at 8-bit precision.** `tests/png_suite.rs` narrows the
//! first decode to eight bits before it compares, and says so at the assertion.
//! The round trip is at `Bitmap` precision, which is the precision the caller
//! that asked for it has.
//!
//! # No palette, no interlace, and why neither is a gap
//!
//! Colour types 0, 2, 4 and 6 are written; type 3 is not. A palette is a
//! *compression* decision — it makes a file smaller when the image has at most
//! 256 distinct colours — and choosing one means counting colours over the
//! whole raster and giving up when the count passes 256, which is a second
//! pass over the image to sometimes save bytes. The row filters below already
//! take the flat regions an indexed image would, and a decoder sees the same
//! pixels either way. Interlace is the same shape of decision one layer up: it
//! exists so a partial download shows a blurry whole image, and nothing here
//! writes to a socket.
//!
//! Both are read by [`super::png_decode`], which is the direction that matters:
//! a file somebody else wrote may be anything Table 11.1 permits, and a file
//! this engine writes is one of four things it chose.

use super::{ChunkType, PngColour, MAX_CHUNK_LEN, MAX_DIMENSION};
use crate::crc32::Crc32;
use crate::deflate::{DeflateLevel, ZlibDeflater, BLOCK_SYMBOLS, LOOKAHEAD};
use crate::predictors::paeth;

/// Why a raster could not be written as a PNG.
///
/// All four are a *caller's* description of its own buffer failing to describe
/// a picture — not damage in data read from somewhere, which is what ruling 2
/// degrades for. There is nothing to degrade to here: a PNG with no pixels is
/// not a smaller PNG, and [`super::PngError::BadDimensions`] is what this
/// crate's own decoder answers a file claiming one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngEncodeError {
    /// A zero width or height, or one past 11.2.2's `2^31 - 1`.
    BadDimensions { width: u32, height: u32 },
    /// The row stride does not reach the end of a row. A stride *larger* than
    /// the row is normal — it is a padded buffer, and the padding is skipped —
    /// but a smaller one means the rows overlap, and an encoder that wrote
    /// them anyway would produce a sheared picture rather than a refusal.
    ShortStride { stride: usize, row_bytes: u64 },
    /// Fewer bytes than `stride x (height - 1) + row_bytes`. The last row is
    /// charged at its real width rather than at the stride, because a caller
    /// is entitled to stop its buffer at the final pixel.
    ShortData { have: usize, need: u64 },
    /// A [`PngWriter`] handed more rows than its height has, or finished with
    /// fewer: `rows` is how many it had been handed, counting the ones that
    /// went past. A file whose IDAT holds another number of rows than its IHDR
    /// declares is not a picture, so no row past the height is written.
    RowCount { height: u32, rows: u64 },
}

impl core::fmt::Display for PngEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadDimensions { width, height } => {
                write!(f, "image dimensions {width} x {height}")
            }
            Self::ShortStride { stride, row_bytes } => {
                write!(f, "stride of {stride} bytes for a row of {row_bytes}")
            }
            Self::ShortData { have, need } => {
                write!(f, "{have} bytes of raster, {need} needed")
            }
            Self::RowCount { height, rows } => {
                write!(f, "{rows} rows for an image {height} rows high")
            }
        }
    }
}

impl std::error::Error for PngEncodeError {}

/// An interleaved 8-bit raster, and where its rows begin.
///
/// Borrowed rather than owned: the caller already holds the pixels, and the
/// one caller that matters — `tinker_pdf::Bitmap::to_png` — holds a whole
/// rendered page.
#[derive(Clone, Copy, Debug)]
pub struct PngSource<'a> {
    pub width: u32,
    pub height: u32,
    /// Which of the four colour types [`png_encode`] writes. There is no
    /// indexed variant here for the reason the module header gives.
    pub colour: PngColour,
    /// Bytes from the start of one row to the start of the next. At least
    /// `width x colour.components()`; more means the buffer is padded and the
    /// tail of each row is not written.
    ///
    /// **This is the field an encoder is most likely to ignore**, because for
    /// every unpadded buffer it equals the row length and a version that reads
    /// the raster as one contiguous run produces byte-identical output. A
    /// `Canvas` does not pad today, so nothing in this engine would notice —
    /// which is exactly why `a_padded_stride_is_not_read_as_pixels` builds one
    /// that does.
    pub stride: usize,
    pub data: &'a [u8],
}

/// 9.2's five filter types, in the specification's own numbering — which is
/// also the byte that precedes every scanline.
#[cfg(test)]
const FILTERS: [u8; 5] = [0, 1, 2, 3, 4];

/// An IDAT chunk is closed once its data reaches this many bytes, and at the
/// end of the stream.
const IDAT_SIZE: usize = 1 << 16;

/// The most of a row the deflater is handed in one write, so that what one
/// write appends to a chunk is bounded however wide the row is, and a chunk
/// stays far under 5.3's `MAX_CHUNK_LEN`. Arrival is no input of the
/// deflater's, so how a row is cut changes no byte.
const FEED: usize = 1 << 16;

/// The most one deflater write of at most [`FEED`] bytes appends
/// (`docs/design/banded-output.md`). A block written in whichever form is
/// fewest bits is never longer than its fixed-code form: a literal 8 or 9
/// bits, a match at most 31 (an 8-bit length code with 5 extra bits, a 5-bit
/// distance code with 13), and 10 bits of header and end. One write completes
/// at most the block in hand — its symbols from before the write, at most one
/// fewer than a block holds, at 31 bits — and the symbols of its own bytes and
/// those the deflater held back, at most 9 bits a byte since a match costs
/// fewer bits a byte it covers than a literal, in at most five blocks; and the
/// stream's two-byte header on its first write.
const WRITE_BOUND: usize =
    ((BLOCK_SYMBOLS - 1) * 31 + (FEED + LOOKAHEAD) * 9 + 5 * 10).div_ceil(8) + 2;

/// The most the open IDAT chunk holds: its length and its type, its data
/// under [`IDAT_SIZE`] before a write, and what one write appends. The
/// deflater's last block and the stream's checksum, appended at the end, are
/// less than a write.
const CHUNK_BOUND: usize = 8 + (IDAT_SIZE - 1) + WRITE_BOUND;

/// What [`png_encode_counted`] held as it wrote, in bytes: plain numbers,
/// handed back as a value, since this crate keeps no tally (ruling 8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PngHeld {
    /// The most held at once in buffers sized from the image rather than
    /// from a row or a constant — a whole filtered image, a whole compressed
    /// stream, a stored copy of either. The writer sizes none: it holds three
    /// rows, the deflater's state, which is fixed, the chunk it is filling,
    /// bounded by a constant, and the file.
    pub image_scratch: usize,
    /// The most held at once, the file included: the three rows, the
    /// deflater's state, the chunk being filled and the file as it grew.
    pub peak: usize,
}

/// Writes a complete PNG file: signature, IHDR, IDAT, IEND.
///
/// # Errors
/// Any [`PngEncodeError`] — all three are a raster that does not describe a
/// picture.
pub fn png_encode(source: &PngSource<'_>) -> Result<Vec<u8>, PngEncodeError> {
    png_encode_counted(source).map(|(file, _)| file)
}

/// [`png_encode`], and what writing the file held ([`PngHeld`]).
///
/// Each row is costed under the five filters in one pass and filtered once,
/// with the cheapest; its type byte and bytes go to a zlib stream at the fast
/// level ([`DeflateLevel::Fast`]), which appends to the open IDAT chunk as it
/// completes each block, and the chunk is closed — its length written and
/// its CRC taken over its type and its data — once it reaches 64 KiB and at
/// the end. So no whole filtered image, whole compressed stream or stored
/// copy is ever held (`docs/design/performance.md`, P-04). It is a
/// [`PngWriter`] handed every row at once.
///
/// # Errors
/// Any [`PngEncodeError`], as [`png_encode`].
pub fn png_encode_counted(source: &PngSource<'_>) -> Result<(Vec<u8>, PngHeld), PngEncodeError> {
    let layout = PngLayout {
        width: source.width,
        height: source.height,
        colour: source.colour,
    };
    // The dimensions are refused before a byte is written, and the stride and
    // the length by `rows` before a row is: the order they always came in.
    let mut out = Vec::new();
    let mut writer = PngWriter::new(&layout, &mut out)?;
    writer.rows(
        &PngRows {
            rows: source.height,
            stride: source.stride,
            data: source.data,
        },
        &mut out,
    )?;
    let held = writer.finish(&mut out)?;
    let peak = held.peak.saturating_add(out.capacity());
    Ok((
        out,
        PngHeld {
            image_scratch: 0,
            peak,
        },
    ))
}

/// The picture a [`PngWriter`] writes: its size and its layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PngLayout {
    pub width: u32,
    pub height: u32,
    /// Which of the four colour types is written, as [`PngSource::colour`].
    pub colour: PngColour,
}

impl PngLayout {
    /// [`PngEncodeError::BadDimensions`] for a zero width or height, or one
    /// past 11.2.2's `2^31 - 1`.
    fn check(&self) -> Result<(), PngEncodeError> {
        let (width, height) = (self.width, self.height);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(PngEncodeError::BadDimensions { width, height });
        }
        Ok(())
    }

    /// The bytes of one row, `width × components`, in `u64` so that a width
    /// 11.2.2 permits cannot overflow it on a 32-bit target.
    fn row_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.colour.components())
    }

    /// What the open chunk is reserved at, once: [`CHUNK_BOUND`], or for a
    /// picture whose whole stream is shorter, that — at most 9 bits a byte of
    /// its rows and their type bytes, 10 bits a block, and the stream's header
    /// and checksum — so the chunk never grows and a small picture does not
    /// hold the bound.
    fn chunk_capacity(&self) -> usize {
        let raw = u64::from(self.height).saturating_mul(self.row_bytes().saturating_add(1));
        let blocks = raw / BLOCK_SYMBOLS as u64 + 1;
        let stream = raw
            .saturating_mul(9)
            .saturating_add(blocks.saturating_mul(10))
            .div_ceil(8)
            .saturating_add(6);
        usize::try_from(stream.saturating_add(8)).map_or(CHUNK_BOUND, |n| n.min(CHUNK_BOUND))
    }
}

/// The next rows of a picture: `rows` of them, the first at the start of
/// `data` and each `stride` bytes after the one before — [`PngSource`]'s
/// raster, for a band of it.
#[derive(Clone, Copy, Debug)]
pub struct PngRows<'a> {
    pub rows: u32,
    /// Bytes from the start of one row to the start of the next, at least the
    /// row's; more means the buffer is padded, and the padding is not written.
    pub stride: usize,
    pub data: &'a [u8],
}

/// A PNG written a band of rows at a time (`docs/design/banded-output.md`).
///
/// [`PngWriter::new`] appends the signature and IHDR to the caller's output,
/// [`PngWriter::rows`] takes the next rows and appends every IDAT chunk they
/// close, and [`PngWriter::finish`] appends the last chunk and IEND. What
/// each call appends is whole chunks, so the caller may drain its output
/// after any of them, and a file of any height is written holding three rows,
/// the deflater's fixed state and the one chunk being filled.
///
/// **The bytes are the whole file's.** Each row's type byte, and then its
/// filtered bytes in pieces of at most `FEED`, are the deflater's writes, and
/// a chunk closes after the write that brings its data to `IDAT_SIZE` or more,
/// and at the end. The deflater's output depends on its input alone, never on
/// how it arrived (`deflate.rs`), so a raster handed over in bands of any
/// height writes what [`png_encode`] writes for it whole.
pub struct PngWriter {
    height: u32,
    /// Bytes a pixel: 9.2's filter unit.
    bpp: usize,
    /// Bytes a row.
    row_bytes: u64,
    /// Rows handed over so far.
    written: u32,
    zlib: ZlibDeflater,
    /// The row above, the row read and the row filtered, allocated with the
    /// first rows: a caller handing rows over holds at least one.
    prior: Vec<u8>,
    current: Vec<u8>,
    filtered: Vec<u8>,
    /// The IDAT chunk being filled: its length's four bytes, written when it
    /// closes, its type and its data so far. A chunk's length comes before
    /// its data, so the writer keeps the chunk until it closes.
    chunk: Vec<u8>,
    /// The most the writer has held at once.
    peak: usize,
}

impl PngWriter {
    /// A writer for `layout`, the file's signature and IHDR appended to
    /// `out`.
    ///
    /// # Errors
    /// [`PngEncodeError::BadDimensions`]; nothing is appended then.
    pub fn new(layout: &PngLayout, out: &mut Vec<u8>) -> Result<PngWriter, PngEncodeError> {
        layout.check()?;
        out.extend_from_slice(&super::PNG_SIGNATURE);
        chunk(out, super::IHDR, &ihdr(layout));
        let mut writer = PngWriter {
            height: layout.height,
            bpp: layout.colour.components() as usize,
            row_bytes: layout.row_bytes(),
            written: 0,
            zlib: ZlibDeflater::new(DeflateLevel::Fast),
            prior: Vec::new(),
            current: Vec::new(),
            filtered: Vec::new(),
            chunk: Vec::with_capacity(layout.chunk_capacity()),
            peak: 0,
        };
        open(&mut writer.chunk);
        writer.note();
        Ok(writer)
    }

    /// The rows of the picture the writer has not been handed yet.
    #[must_use]
    pub fn rows_left(&self) -> u32 {
        self.height - self.written
    }

    /// Takes the next `rows.rows` rows, filtering each against the row
    /// before it — for a band's first, the last of the band before — and
    /// appends to `out` every IDAT chunk they close.
    ///
    /// # Errors
    /// [`PngEncodeError::RowCount`] for more rows than are left, and
    /// [`PngEncodeError::ShortStride`] and [`PngEncodeError::ShortData`] for
    /// a band that does not describe its rows, as [`png_encode`] refuses a
    /// raster. Nothing is taken from a refused band and nothing is appended.
    pub fn rows(&mut self, rows: &PngRows<'_>, out: &mut Vec<u8>) -> Result<(), PngEncodeError> {
        if rows.rows == 0 {
            return Ok(());
        }
        if rows.rows > self.rows_left() {
            return Err(PngEncodeError::RowCount {
                height: self.height,
                rows: u64::from(self.written) + u64::from(rows.rows),
            });
        }
        // In `u64` throughout, and compared before anything is converted: on
        // a 32-bit target `width x components` overflows a `usize` while
        // still being a width 11.2.2 permits, and an overflowed row length is
        // a slice inside the buffer rather than a refusal (ruling 1). A stride
        // is a `usize`, so a row length past `usize` is caught by the stride
        // comparison itself.
        let stride = rows.stride as u64;
        if stride < self.row_bytes {
            return Err(PngEncodeError::ShortStride {
                stride: rows.stride,
                row_bytes: self.row_bytes,
            });
        }
        // The last row is charged at its length and not at the stride: a
        // caller whose buffer ends at the final pixel has given us every pixel.
        let need = stride
            .saturating_mul(u64::from(rows.rows) - 1)
            .saturating_add(self.row_bytes);
        if (rows.data.len() as u64) < need {
            return Err(PngEncodeError::ShortData {
                have: rows.data.len(),
                need,
            });
        }
        // Both comparisons above passed, so `row_bytes <= stride <= data.len()`.
        let row_bytes = self.row_bytes as usize;
        if self.current.len() != row_bytes {
            // 9.2: the row above the first is zeros.
            self.prior = vec![0u8; row_bytes];
            self.current = vec![0u8; row_bytes];
            self.filtered = vec![0u8; row_bytes];
        }
        // Taken out while the row's pieces are written, so the chunk can be
        // cut between them; put back after.
        let mut filtered = core::mem::take(&mut self.filtered);
        for y in 0..rows.rows as usize {
            let at = y * rows.stride;
            // Bounded by the `ShortData` check above, and read through `get`
            // regardless (ruling 1): a `Vec` index is a panic and this one is
            // arithmetic on a caller's number.
            match rows.data.get(at..at + row_bytes) {
                Some(row) => self.current.copy_from_slice(row),
                None => self.current.fill(0),
            }
            let kind = cheapest(&costs(&self.current, &self.prior, self.bpp));
            filter_row(kind, &self.current, &self.prior, self.bpp, &mut filtered);
            self.zlib.write(&[kind], &mut self.chunk);
            self.cut(out);
            for piece in filtered.chunks(FEED) {
                self.zlib.write(piece, &mut self.chunk);
                self.cut(out);
            }
            core::mem::swap(&mut self.prior, &mut self.current);
        }
        self.filtered = filtered;
        self.written += rows.rows;
        self.note();
        Ok(())
    }

    /// Appends the last IDAT chunk — the deflater's rest and the stream's
    /// checksum — and IEND, and hands back what the writer held at most.
    ///
    /// # Errors
    /// [`PngEncodeError::RowCount`] when fewer rows than the height were
    /// handed over; nothing is appended then.
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<PngHeld, PngEncodeError> {
        if self.written < self.height {
            return Err(PngEncodeError::RowCount {
                height: self.height,
                rows: u64::from(self.written),
            });
        }
        // The deflater's last block lands in the chunk while the rows and the
        // deflater are still held, so all three are counted together.
        self.note();
        let rows = self.prior.capacity() + self.current.capacity() + self.filtered.capacity();
        let state = self.zlib.held();
        let PngWriter {
            zlib,
            mut chunk,
            mut peak,
            ..
        } = self;
        zlib.finish(&mut chunk);
        peak = peak.max(rows + state + chunk.capacity());
        close(&mut chunk, out);
        self::chunk(out, super::IEND, b"");
        Ok(PngHeld {
            image_scratch: 0,
            peak,
        })
    }

    /// What the writer has held, at most at once: its three rows, its
    /// deflater's state and the chunk being filled — the caller's output,
    /// which it drains as it likes, aside. Sized from the image's width and
    /// never from its height, so `image_scratch` is none.
    #[must_use]
    pub fn held(&self) -> PngHeld {
        PngHeld {
            image_scratch: 0,
            peak: self.peak,
        }
    }

    /// Notes what the writer holds now against its most.
    fn note(&mut self) {
        let rows = self.prior.capacity() + self.current.capacity() + self.filtered.capacity();
        let now = rows + self.zlib.held() + self.chunk.capacity();
        self.peak = self.peak.max(now);
    }

    /// Closes the chunk into `out` and opens the next once its data has
    /// reached [`IDAT_SIZE`].
    fn cut(&mut self, out: &mut Vec<u8>) {
        if self.chunk.len() - 8 >= IDAT_SIZE {
            self.note();
            close(&mut self.chunk, out);
            open(&mut self.chunk);
        }
    }
}

/// Opens an IDAT chunk in `chunk`, emptied: its length, written when it
/// closes, and its type.
fn open(chunk: &mut Vec<u8>) {
    chunk.clear();
    chunk.extend_from_slice(&[0; 4]);
    chunk.extend_from_slice(&super::IDAT.0);
}

/// 5.3: the chunk's length, over its data, and its CRC-32, over its type and
/// its data and not its length; the chunk appended to `out` whole.
fn close(chunk: &mut [u8], out: &mut Vec<u8>) {
    let data = chunk.len().saturating_sub(8);
    // A chunk is cut a bounded write past `IDAT_SIZE`, so the length fits;
    // `unwrap_or` the ceiling all the same (ruling 1).
    let len = u32::try_from(data).unwrap_or(MAX_CHUNK_LEN);
    if let Some(head) = chunk.get_mut(..4) {
        head.copy_from_slice(&len.to_be_bytes());
    }
    let mut crc = Crc32::new();
    crc.update(chunk.get(4..).unwrap_or_default());
    out.extend_from_slice(chunk);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// 11.2.2's thirteen bytes.
fn ihdr(layout: &PngLayout) -> Vec<u8> {
    let mut d = Vec::with_capacity(13);
    d.extend_from_slice(&layout.width.to_be_bytes());
    d.extend_from_slice(&layout.height.to_be_bytes());
    d.push(8);
    d.push(colour_type(layout.colour));
    // Compression method 0 (the zlib deflate of 10.3), filter method 0 (the
    // five adaptive filters of 9.2), interlace method 0 (one raster). No
    // other value of any of the three has ever been defined, and `png.rs`
    // refuses every other value on the way in.
    d.extend_from_slice(&[0, 0, 0]);
    d
}

/// Table 11.1's colour-type column, for the four layouts this writes.
///
/// A `match` rather than arithmetic on the component count, which is the
/// plausible version that gets it wrong: grey+alpha has two components and is
/// type **4**, truecolour has three and is type **2**, so the two orderings
/// disagree and a reader handed the wrong one reads a two-channel image as a
/// three-channel one and runs off the end of every row.
const fn colour_type(colour: PngColour) -> u8 {
    match colour {
        PngColour::Grey => 0,
        PngColour::Rgb => 2,
        PngColour::GreyAlpha => 4,
        PngColour::Rgba => 6,
    }
}

/// ISO/IEC 15948 9.2, Table 9.1 — the **filtering** direction, which is the
/// reconstruction formulas of `predictors.rs` read right to left.
///
/// `a` is the byte `bpp` to the left, `b` the byte above, `c` the byte above
/// and to the left; each is zero where it falls outside the image, which 9.2
/// states explicitly rather than leaving to a convention.
///
/// The encoder's `a`, `b` and `c` come from the **original** scanlines, not
/// from anything reconstructed. That is not an optimisation: 9.2's decoder
/// reconstructs to exactly the original bytes, so the two agree, and reading
/// the originals is what makes every row's five candidates computable without
/// having decoded the four that were not chosen.
fn filter_row(kind: u8, row: &[u8], prior: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..row.len() {
        let x = row[i];
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prior.get(i).copied().unwrap_or(0);
        let c = if i >= bpp {
            prior.get(i - bpp).copied().unwrap_or(0)
        } else {
            0
        };
        let Some(slot) = out.get_mut(i) else { return };
        *slot = match kind {
            // 0, None.
            0 => x,
            // 1, Sub.
            1 => x.wrapping_sub(a),
            // 2, Up.
            2 => x.wrapping_sub(b),
            // 3, Average. 9.2 is explicit that the sum is formed without
            // overflow and the floor is taken before the subtraction, which is
            // why the addition happens in `u16` and not in `u8`.
            3 => x.wrapping_sub(average(a, b)),
            // 4, Paeth. The predictor itself is `predictors.rs`'s, which is
            // the one this crate's decoder unfilters with and the one PDF's
            // `/Predictor 15` reaches. **Deliberately shared rather than
            // transcribed twice**: the two directions must agree byte for byte
            // or nothing round-trips, and a second copy is a second thing to
            // get wrong. What sharing costs is that a defect in it cancels
            // between this encoder and that decoder — which is precisely the
            // symmetry `png_suite.rs`'s independently transcribed unfilter
            // exists to break, and the injection matrix measured it: reversing
            // the tie-break leaves the whole round trip green — 0 of the 162
            // files it compares notice — and is caught by that transcription
            // and by two decoder-side PngSuite comparisons that pit third-party
            // files against each other rather than against our own output.
            // Three of 2 014, and not one of them a round trip.
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
    }
}

/// The heuristic ISO/IEC 15948 12.8 recommends for choosing a filter: **the
/// minimum sum of absolute differences**, over the filtered bytes read as
/// signed values — each of the five filters' sums for `row` under `prior`,
/// in one pass over the row: at each byte the five filtered values are
/// formed from `a`, `b` and `c`, as [`filter_row`] forms them, and each
/// added to its own sum.
///
/// Signed is the whole content of the rule and the easy half to lose. A
/// filtered byte of `0xFF` is a difference of **-1**, which is a very good
/// prediction; read unsigned it is 255, the worst score there is, and a
/// heuristic reading it that way rejects exactly the rows the filter helped
/// most. So `128..=255` counts as `256 - b`, which is `|b - 256|`
/// ([`magnitude`]).
///
/// Integer arithmetic only, and `u64` sums that cannot overflow: the largest
/// term is 128 and the longest row this crate will encode is bounded by a
/// `usize`, so a sum is under `2^63` on any target (ruling 4). A float score
/// would make the chosen filter — and therefore every compressed byte —
/// depend on the target's rounding.
fn costs(row: &[u8], prior: &[u8], bpp: usize) -> [u64; 5] {
    let mut sums = [0u64; 5];
    let lead = bpp.min(row.len()).min(prior.len());
    // The first pixel's bytes: `a` and `c` fall outside the image, which 9.2
    // makes zero.
    for (&x, &b) in row[..lead].iter().zip(&prior[..lead]) {
        add_costs(&mut sums, x, 0, b, 0);
    }
    // Every later byte, its `a` and `c` a pixel back.
    let left = row.iter().zip(prior);
    for ((&x, &b), (&a, &c)) in row[lead..].iter().zip(&prior[lead..]).zip(left) {
        add_costs(&mut sums, x, a, b, c);
    }
    sums
}

/// Adds one byte's five filtered values to their sums, in 9.2's order: None,
/// Sub, Up, Average and Paeth.
#[inline]
fn add_costs(sums: &mut [u64; 5], x: u8, a: u8, b: u8, c: u8) {
    sums[0] += magnitude(x);
    sums[1] += magnitude(x.wrapping_sub(a));
    sums[2] += magnitude(x.wrapping_sub(b));
    sums[3] += magnitude(x.wrapping_sub(average(a, b)));
    sums[4] += magnitude(x.wrapping_sub(paeth(a, b, c)));
}

/// A filtered byte as 12.8 scores it: its distance from zero, read signed.
#[inline]
fn magnitude(byte: u8) -> u64 {
    u64::from(if byte < 128 {
        byte
    } else {
        0u8.wrapping_sub(byte)
    })
}

/// 9.2's Average predictor: the sum formed without overflow and the floor
/// taken before it is used, which is why the addition is in `u16`.
#[inline]
fn average(a: u8, b: u8) -> u8 {
    ((u16::from(a) + u16::from(b)) / 2) as u8
}

/// The filter of the five sums that is cheapest. Strictly less, so the first
/// of a tie wins and the choice does not depend on an order of visiting
/// (ruling 4); 9.2's own ordering puts None first, which is the one a decoder
/// unfilters fastest, so a tie resolving to it is the right tie to take.
fn cheapest(costs: &[u64; 5]) -> u8 {
    let mut best = 0;
    for (kind, &cost) in costs.iter().enumerate() {
        if cost < costs[best] {
            best = kind;
        }
    }
    best as u8
}

/// 12.8's sum over `filtered`, one filter's bytes: what [`costs`] adds up
/// for each filter at once, kept to hold it to the five passes it replaced.
#[cfg(test)]
fn cost_of(filtered: &[u8]) -> u64 {
    filtered.iter().map(|&byte| magnitude(byte)).sum()
}

/// 5.3's chunk layout: a four-byte length, a four-byte type, the data, and a
/// CRC-32 **over the type and the data and not over the length**.
///
/// That exclusion is the one thing about a PNG chunk that is easy to state
/// wrongly and impossible to see: a checksum over `length || type || data` is
/// self-consistent, so a writer and a reader that shared the mistake would
/// agree perfectly and every other program in the world would reject the file.
/// It is why [`Crc32`] is resumable rather than a one-slice function — the two
/// covered runs are never adjacent in one buffer — and why
/// `the_crc_covers_the_type_and_the_data_and_not_the_length` checks the number
/// against a value computed from the two slices separately. Dropping the type
/// from the sum is caught by 22 assertions, the widest of any defect in this
/// module's injection matrix: it makes every file this engine writes
/// unreadable, including by this engine.
fn chunk(out: &mut Vec<u8>, kind: ChunkType, data: &[u8]) {
    // Every caller is a fixed 13 bytes or an empty IEND, so the conversion
    // cannot fail; `unwrap_or` rather
    // than an `unwrap` all the same (ruling 1), and the fallback is the
    // ceiling 5.3 names so a hypothetical over-long chunk is refused by a
    // reader rather than mis-parsed by one.
    let len = u32::try_from(data.len()).unwrap_or(MAX_CHUNK_LEN);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&kind.0);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(&kind.0);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

#[cfg(test)]
mod tests;
