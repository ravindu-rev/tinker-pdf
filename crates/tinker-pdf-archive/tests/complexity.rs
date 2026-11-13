//! Size-scaling complexity tests for the archives that are not ZIP and for
//! their coders (`docs/design/resource-exhaustion.md`, "The size-scaling
//! law").
//!
//! Each shape is an archive or a stream at a size n and at four times n,
//! read through a counted door that returns the steps it took: a tar's and a
//! RAR's headers and the bytes their checks covered, a 7z's header and its
//! files, and each coder's own work — LZMA's and LZMA2's bits and copies,
//! BCJ's bytes looked at, BCJ2's bytes, decisions and targets, PPMd's
//! symbols, states and contexts, bzip2's codes, tables and transform, and
//! zstd's blocks, literals and sequences. The law is the design's: the work
//! at 4 n is at most 1.1 × 4 times the work at n. A quadratic shape costs
//! sixteen times as much and an n¹·⁵ one eight.
//!
//! The archives and streams are written here from their formats' own
//! layouts, LZMA's by a literal-and-repeat range encoder written from the
//! format's description of its decoder; PPMd's and bzip2's, which this
//! repository has no encoder for, are taken from `tests/coders/`, where real
//! writers made them over this crate's inputs — a prefix of a PPMd stream
//! decoded at two lengths, and a bzip2 stream repeated, which its format reads
//! as one stream after another. Every shape is held to what it is as well as
//! to what it costs: each read gives back what was written, through the
//! counted door and the uncounted one alike. Four times n stays inside every
//! cap on the path. No clock is read here.

use std::path::Path;

use tinker_pdf_archive::{bzip2, lzma, ppmd, rar, sevenz, tar, zstd};
use tinker_pdf_filters::{crc32, xxh64};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// The law for a shape whose parser reads each element a bounded number of
/// times, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:40} work at n {n:>9}  at 4n {kn:>9}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

/// `len` bytes of no particular pattern, the same on every run.
fn bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 7) as u8
        })
        .collect()
}

// ---------------------------------------------------------------------------
// tar.
// ---------------------------------------------------------------------------

/// One POSIX ustar header for a regular file, its checksum computed.
fn ustar(name: &str, size: usize) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    // The checksum field reads as eight spaces while the sum is taken.
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    header
}

/// A tar of `files`, each padded to whole blocks, and the two zero blocks
/// that end it.
fn tar_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in files {
        out.extend_from_slice(&ustar(name, data.len()));
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

fn files(n: usize) -> Vec<(String, Vec<u8>)> {
    (0..n)
        .map(|i| (format!("page{i:05}.png"), format!("page {i}").into_bytes()))
        .collect()
}

fn tar_headers(n: usize) -> u64 {
    let files = files(n);
    let bytes = tar_of(&files);
    let (opened, steps) = tar::Archive::open_counted(&bytes, &tar::Limits::DEFAULT);
    let archive = opened.expect("the tar opens");
    assert_eq!(archive.entries().len(), n, "every header is listed");
    for (entry, (name, data)) in archive.entries().iter().zip(&files) {
        assert_eq!(&entry.name, name);
        assert_eq!(archive.read(entry.index), Ok(data.as_slice()));
    }
    let plain = tar::Archive::open(&bytes, &tar::Limits::DEFAULT).expect("uncounted");
    assert_eq!(plain.entries(), archive.entries());
    steps
}

#[test]
fn a_tar_of_more_headers_costs_work_linear_in_its_headers() {
    holds_the_law("tar n headers", tar_headers(500), tar_headers(2000));
}

// ---------------------------------------------------------------------------
// RAR 5.
// ---------------------------------------------------------------------------

/// RAR 5's `vint`: seven bits a byte, least significant first.
fn vint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// One header: its CRC-32 over the size field and everything after it.
fn rar_header(kind: u64, flags: u64, body: &[u8], data: usize) -> Vec<u8> {
    let mut inner = vint(kind);
    inner.extend(vint(flags));
    if flags & 0x0002 != 0 {
        inner.extend(vint(data as u64));
    }
    inner.extend_from_slice(body);
    let mut covered = vint(inner.len() as u64);
    covered.extend_from_slice(&inner);
    let mut out = crc32(&covered).to_le_bytes().to_vec();
    out.extend_from_slice(&covered);
    out
}

/// A RAR 5 of `files`, every one stored with its CRC-32.
fn rar_of(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    out.extend(rar_header(1, 0, &vint(0), 0));
    for (name, data) in files {
        let mut body = vint(0x0002 | 0x0004); // mtime, CRC
        body.extend(vint(data.len() as u64));
        body.extend(vint(0x20)); // attributes
        body.extend_from_slice(&0x6512_3456u32.to_le_bytes());
        body.extend_from_slice(&crc32(data).to_le_bytes());
        body.extend(vint(0)); // stored, not solid
        body.extend(vint(0)); // host OS
        body.extend(vint(name.len() as u64));
        body.extend_from_slice(name.as_bytes());
        out.extend(rar_header(2, 0x0002, &body, data.len()));
        out.extend_from_slice(data);
    }
    out.extend(rar_header(5, 0, &vint(0), 0));
    out
}

fn rar_headers(n: usize) -> u64 {
    let files = files(n);
    let bytes = rar_of(&files);
    let (opened, steps) = rar::Archive::open_counted(&bytes, &rar::Limits::DEFAULT);
    let archive = opened.expect("the RAR opens");
    assert_eq!(archive.entries().len(), n, "every header is listed");
    for (entry, (name, data)) in archive.entries().iter().zip(&files) {
        assert_eq!(&entry.name, name);
        assert_eq!(archive.read(entry.index).as_deref(), Ok(data.as_slice()));
    }
    let plain = rar::Archive::open(&bytes, &rar::Limits::DEFAULT).expect("uncounted");
    assert_eq!(plain.entries(), archive.entries());
    steps
}

#[test]
fn a_rar_of_more_headers_costs_work_linear_in_its_headers() {
    holds_the_law("RAR n headers", rar_headers(500), rar_headers(2000));
}

// ---------------------------------------------------------------------------
// 7z, built from its header grammar.
// ---------------------------------------------------------------------------

const K_END: u8 = 0x00;
const K_HEADER: u8 = 0x01;
const K_MAIN_STREAMS: u8 = 0x04;
const K_FILES_INFO: u8 = 0x05;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SUBSTREAMS_INFO: u8 = 0x08;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0A;
const K_FOLDER: u8 = 0x0B;
const K_CODERS_UNPACK_SIZE: u8 = 0x0C;
const K_NUM_UNPACK_STREAM: u8 = 0x0D;
const K_NAME: u8 = 0x11;

/// 7z's `NUMBER`, in its shortest form.
fn number(value: u64) -> Vec<u8> {
    for extra in 0..8usize {
        let bits = 8 * extra + (7 - extra);
        if value < (1u64 << bits) {
            let mut first = 0u8;
            for k in 0..extra {
                first |= 0x80 >> k;
            }
            first |= (value >> (8 * extra)) as u8;
            let mut out = vec![first];
            for k in 0..extra {
                out.push((value >> (8 * k)) as u8);
            }
            return out;
        }
    }
    let mut out = vec![0xFF];
    out.extend_from_slice(&value.to_le_bytes());
    out
}

/// A name as 7z stores it: UTF-16LE, NUL-terminated.
fn utf16(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in name.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// One folder of a 7z: its coder records, as the grammar spells them after
/// the coder count, and the size of every coder's output.
struct Folder {
    coders: Vec<u8>,
    unpack_sizes: Vec<u64>,
}

/// One coder record: its id, its stream counts when not one each, and its
/// properties when it has some.
fn coder(id: &[u8], streams: Option<(u64, u64)>, props: &[u8]) -> Vec<u8> {
    let mut flags = id.len() as u8;
    if streams.is_some() {
        flags |= 0x10;
    }
    if !props.is_empty() {
        flags |= 0x20;
    }
    let mut out = vec![flags];
    out.extend_from_slice(id);
    if let Some((ins, outs)) = streams {
        out.extend(number(ins));
        out.extend(number(outs));
    }
    if !props.is_empty() {
        out.extend(number(props.len() as u64));
        out.extend_from_slice(props);
    }
    out
}

/// A 7z of one folder over `pack` streams, whose output is `files` in order.
fn sevenz_of(folder: &Folder, pack: &[Vec<u8>], files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut header = vec![K_HEADER, K_MAIN_STREAMS, K_PACK_INFO];
    header.extend(number(0));
    header.extend(number(pack.len() as u64));
    header.push(K_SIZE);
    for stream in pack {
        header.extend(number(stream.len() as u64));
    }
    header.push(K_END);

    header.extend([K_UNPACK_INFO, K_FOLDER]);
    header.extend(number(1));
    header.push(0); // not external
    header.extend_from_slice(&folder.coders);
    header.push(K_CODERS_UNPACK_SIZE);
    for size in &folder.unpack_sizes {
        header.extend(number(*size));
    }
    header.push(K_END);

    header.extend([K_SUBSTREAMS_INFO, K_NUM_UNPACK_STREAM]);
    header.extend(number(files.len() as u64));
    header.push(K_SIZE);
    for (_, data) in files.iter().take(files.len().saturating_sub(1)) {
        header.extend(number(data.len() as u64));
    }
    header.extend([K_CRC, 1]);
    for (_, data) in files {
        header.extend_from_slice(&crc32(data).to_le_bytes());
    }
    header.push(K_END);
    header.push(K_END);

    header.push(K_FILES_INFO);
    header.extend(number(files.len() as u64));
    let mut names = vec![0u8];
    for (name, _) in files {
        names.extend(utf16(name));
    }
    header.extend(number(u64::from(K_NAME)));
    header.extend(number(names.len() as u64));
    header.extend_from_slice(&names);
    header.extend(number(0));
    header.push(K_END);

    let packed: Vec<u8> = pack.concat();
    let mut out = vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4];
    let mut start = Vec::new();
    start.extend_from_slice(&(packed.len() as u64).to_le_bytes());
    start.extend_from_slice(&(header.len() as u64).to_le_bytes());
    start.extend_from_slice(&crc32(&header).to_le_bytes());
    out.extend_from_slice(&crc32(&start).to_le_bytes());
    out.extend_from_slice(&start);
    out.extend_from_slice(&packed);
    out.extend_from_slice(&header);
    out
}

/// Opens a 7z, reads every entry back, and returns the steps of the open
/// and of the reads together.
fn read_7z(bytes: &[u8], files: &[(String, Vec<u8>)]) -> u64 {
    let (opened, mut steps) = sevenz::Archive::open_counted(bytes, &sevenz::Limits::DEFAULT);
    let mut archive = opened.expect("the 7z opens");
    assert_eq!(archive.entries().len(), files.len(), "every file is listed");
    let mut plain = sevenz::Archive::open(bytes, &sevenz::Limits::DEFAULT).expect("uncounted");
    for (index, (name, data)) in files.iter().enumerate() {
        assert_eq!(&archive.entries()[index].name, name);
        let (read, taken) = archive.read_counted(index);
        steps = steps.saturating_add(taken);
        assert_eq!(read.as_deref(), Ok(data.as_slice()), "{name} reads back");
        assert_eq!(plain.read(index).as_deref(), Ok(data.as_slice()));
    }
    steps
}

fn copy_folder(size: u64) -> Folder {
    let mut coders = number(1);
    coders.extend(coder(&[0x00], None, &[]));
    Folder {
        coders,
        unpack_sizes: vec![size],
    }
}

fn sevenz_entries(n: usize) -> u64 {
    let files = files(n);
    let packed: Vec<u8> = files.iter().flat_map(|(_, d)| d.iter().copied()).collect();
    let bytes = sevenz_of(&copy_folder(packed.len() as u64), &[packed], &files);
    read_7z(&bytes, &files)
}

#[test]
fn a_7z_of_more_entries_costs_work_linear_in_its_entries() {
    // One solid folder: decompressed once, each entry cut out of it after.
    holds_the_law("7z n entries", sevenz_entries(500), sevenz_entries(2000));
}

// ---------------------------------------------------------------------------
// LZMA, from a range encoder written for these shapes.
// ---------------------------------------------------------------------------

/// LZMA's binary range encoder: the arithmetic the decoder undoes, at its
/// eleven-bit probabilities and its shift of five.
struct RangeEncoder {
    low: u64,
    range: u32,
    cache: u8,
    pending: u64,
    out: Vec<u8>,
}

impl RangeEncoder {
    fn new() -> Self {
        RangeEncoder {
            low: 0,
            range: u32::MAX,
            cache: 0,
            pending: 1,
            out: Vec::new(),
        }
    }

    fn shift_low(&mut self) {
        if (self.low as u32) < 0xFF00_0000 || (self.low >> 32) != 0 {
            let carry = (self.low >> 32) as u8;
            let mut byte = self.cache;
            loop {
                self.out.push(byte.wrapping_add(carry));
                byte = 0xFF;
                self.pending -= 1;
                if self.pending == 0 {
                    break;
                }
            }
            self.cache = (self.low >> 24) as u8;
        }
        self.pending += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }

    fn bit(&mut self, prob: &mut u16, bit: u32) {
        let bound = (self.range >> 11) * u32::from(*prob);
        if bit == 0 {
            self.range = bound;
            *prob += (2048 - *prob) >> 5;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob -= *prob >> 5;
        }
        while self.range < (1 << 24) {
            self.range <<= 8;
            self.shift_low();
        }
    }

    fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

/// What a stream is made of, for the encoder below.
#[derive(Clone, Copy)]
enum Symbol {
    /// A byte coded as a literal.
    Literal(u8),
    /// One byte repeated from the last distance: LZMA's short repeat.
    ShortRep,
}

/// An LZMA stream at `lc=3`, `lp=0`, `pb=2` of `symbols`, whose first byte
/// stands at `position` of a dictionary whose byte before it is `prev`: the
/// model the decoder keeps, kept here for the two kinds of symbol written.
fn lzma_stream(symbols: &[Symbol], mut position: usize, mut prev: u8) -> Vec<u8> {
    let mut rc = RangeEncoder::new();
    let mut is_match = [[1024u16; 4]; 12];
    let mut is_rep = [1024u16; 12];
    let mut is_rep_g0 = [1024u16; 12];
    let mut is_rep0_long = [[1024u16; 4]; 12];
    let mut literal = vec![1024u16; 0x300 * 8];
    let mut state = 0usize;
    for symbol in symbols {
        let pos_state = position & 3;
        match *symbol {
            Symbol::Literal(byte) => {
                assert!(state < 7, "a literal after a repeat is a matched one");
                rc.bit(&mut is_match[state][pos_state], 0);
                let base = usize::from(prev >> 5) * 0x300;
                let mut context = 1usize;
                for i in (0..8).rev() {
                    let bit = u32::from(byte >> i) & 1;
                    rc.bit(&mut literal[base + context], bit);
                    context = (context << 1) | bit as usize;
                }
                state = match state {
                    0..=3 => 0,
                    4..=9 => state - 3,
                    _ => state - 6,
                };
                prev = byte;
            }
            Symbol::ShortRep => {
                rc.bit(&mut is_match[state][pos_state], 1);
                rc.bit(&mut is_rep[state], 1);
                rc.bit(&mut is_rep_g0[state], 0);
                rc.bit(&mut is_rep0_long[state][pos_state], 0);
                state = if state < 7 { 9 } else { 11 };
            }
        }
        position += 1;
    }
    rc.finish()
}

/// LZMA's property byte for `lc=3`, `lp=0`, `pb=2`.
const LZMA_PROPS: u8 = (2 * 5) * 9 + 3;

fn lzma_limits() -> lzma::Limits {
    lzma::Limits {
        max_unpacked: 1 << 24,
    }
}

/// `n` bytes of literals through LZMA, alone and in a 7z folder.
fn lzma_literals(n: usize) -> u64 {
    let data = bytes(n, 1);
    let symbols: Vec<Symbol> = data.iter().map(|&b| Symbol::Literal(b)).collect();
    let stream = lzma_stream(&symbols, 0, 0);
    let (decoded, steps) = lzma::decode_counted(&stream, LZMA_PROPS, n, &lzma_limits());
    assert_eq!(
        decoded.as_deref(),
        Ok(data.as_slice()),
        "the literals decode"
    );
    assert_eq!(
        lzma::decode(&stream, LZMA_PROPS, n, &lzma_limits()).as_deref(),
        Ok(data.as_slice())
    );

    let mut coders = number(1);
    coders.extend(coder(
        &[0x03, 0x01, 0x01],
        None,
        &[LZMA_PROPS, 0, 0, 0x10, 0],
    ));
    let folder = Folder {
        coders,
        unpack_sizes: vec![n as u64],
    };
    let files = vec![("code.bin".to_owned(), data)];
    let bytes = sevenz_of(&folder, &[stream], &files);
    steps.saturating_add(read_7z(&bytes, &files))
}

/// One literal, then `n - 1` short repeats of it.
fn lzma_repeats(n: usize) -> u64 {
    let mut symbols = vec![Symbol::Literal(0x5A)];
    symbols.resize(n, Symbol::ShortRep);
    let stream = lzma_stream(&symbols, 0, 0);
    let (decoded, steps) = lzma::decode_counted(&stream, LZMA_PROPS, n, &lzma_limits());
    assert_eq!(decoded, Ok(vec![0x5A; n]), "every repeat is the one byte");
    steps
}

#[test]
fn more_bytes_through_lzma_cost_work_linear_in_the_bytes() {
    holds_the_law("LZMA n literals", lzma_literals(4096), lzma_literals(16384));
    holds_the_law(
        "LZMA n short repeats",
        lzma_repeats(4096),
        lzma_repeats(16384),
    );
}

/// `n` chunks of LZMA2 of 1 KiB each, compressed and stored by turns, every
/// compressed chunk resetting the state over the dictionary the chunks
/// before it wrote.
fn lzma2_chunks(n: usize) -> u64 {
    const CHUNK: usize = 1024;
    let data = bytes(n * CHUNK, 2);
    let mut stream = Vec::new();
    for (k, chunk) in data.chunks(CHUNK).enumerate() {
        if k % 2 == 1 {
            // Stored, keeping the dictionary.
            stream.push(0x02);
            stream.extend_from_slice(&((CHUNK - 1) as u16).to_be_bytes());
            stream.extend_from_slice(chunk);
            continue;
        }
        let prev = if k == 0 { 0 } else { data[k * CHUNK - 1] };
        let symbols: Vec<Symbol> = chunk.iter().map(|&b| Symbol::Literal(b)).collect();
        let packed = lzma_stream(&symbols, k * CHUNK, prev);
        // The first chunk resets the dictionary; every other resets the
        // state and restates the properties.
        let control = if k == 0 { 0xE0u8 } else { 0xC0 };
        stream.push(control | (((CHUNK - 1) >> 16) as u8));
        stream.extend_from_slice(&((CHUNK - 1) as u16).to_be_bytes());
        stream.extend_from_slice(&((packed.len() - 1) as u16).to_be_bytes());
        stream.push(LZMA_PROPS);
        stream.extend_from_slice(&packed);
    }
    stream.push(0x00);
    let (decoded, steps) = lzma::decode_lzma2_counted(&stream, data.len(), &lzma_limits());
    assert_eq!(
        decoded.as_deref(),
        Ok(data.as_slice()),
        "every chunk decodes"
    );
    assert_eq!(
        lzma::decode_lzma2(&stream, data.len(), &lzma_limits()).as_deref(),
        Ok(data.as_slice())
    );
    steps
}

#[test]
fn more_chunks_through_lzma2_cost_work_linear_in_the_chunks() {
    holds_the_law("LZMA2 n chunks", lzma2_chunks(8), lzma2_chunks(32));
}

// ---------------------------------------------------------------------------
// BCJ and BCJ2, through the 7z folders that carry them.
// ---------------------------------------------------------------------------

/// x86-shaped bytes, a call or a jump in every eight, whose operands' top
/// byte is neither `00` nor `FF`, so no filter converts them and the output
/// is the input.
fn code(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0u32;
    while out.len() < n {
        let opcode = if i % 2 == 0 { 0xE8 } else { 0xE9 };
        out.extend_from_slice(&[0x55, 0x48, opcode, 0x10, 0x20, 0x30, 0x40, 0x90]);
        i += 1;
    }
    out.truncate(n);
    out
}

/// `n` bytes through BCJ, fed by Copy through a bind pair.
fn bcj(n: usize) -> u64 {
    let data = code(n);
    let mut coders = number(2);
    coders.extend(coder(&[0x03, 0x03, 0x01, 0x03], None, &[]));
    coders.extend(coder(&[0x00], None, &[]));
    // BCJ's in-stream 0 is Copy's out-stream 1.
    coders.extend(number(0));
    coders.extend(number(1));
    let folder = Folder {
        coders,
        unpack_sizes: vec![n as u64, n as u64],
    };
    let files = vec![("code.bin".to_owned(), data.clone())];
    let bytes = sevenz_of(&folder, &[data], &files);
    read_7z(&bytes, &files)
}

/// `n` bytes through BCJ2, its four in-streams four pack streams: the main
/// stream, empty call and jump streams, and decisions that convert nothing —
/// a zero and a code of zero, below every bound, and a zero byte for each
/// decision to read.
fn bcj2(n: usize) -> u64 {
    let data = code(n);
    let decisions = data.iter().filter(|&&b| b & 0xFE == 0xE8).count();
    let rc = vec![0u8; 5 + decisions];
    let mut coders = number(1);
    coders.extend(coder(&[0x03, 0x03, 0x01, 0x1B], Some((4, 1)), &[]));
    for stream in 0..4 {
        coders.extend(number(stream));
    }
    let folder = Folder {
        coders,
        unpack_sizes: vec![n as u64],
    };
    let files = vec![("code.bin".to_owned(), data.clone())];
    let bytes = sevenz_of(&folder, &[data, Vec::new(), Vec::new(), rc], &files);
    read_7z(&bytes, &files)
}

#[test]
fn more_bytes_through_bcj_and_bcj2_cost_work_linear_in_the_bytes() {
    holds_the_law("BCJ n bytes", bcj(8192), bcj(32768));
    holds_the_law("BCJ2 n bytes", bcj2(8192), bcj2(32768));
}

// ---------------------------------------------------------------------------
// PPMd and bzip2, from streams real writers made.
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/coders")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A single-folder 7z's packed bytes: everything between its signature
/// header and its header, which begins with the folder's one pack stream.
fn packed_region(bytes: &[u8]) -> &[u8] {
    let offset = u64::from_le_bytes(bytes[12..20].try_into().expect("eight bytes"));
    &bytes[32..32 + usize::try_from(offset).expect("fits")]
}

/// Every entry of a 7z, read through the reader and joined: the folder's
/// whole output.
fn whole_folder(bytes: &[u8]) -> Vec<u8> {
    let mut archive = sevenz::Archive::open(bytes, &sevenz::Limits::DEFAULT).expect("opens");
    (0..archive.entries().len())
        .flat_map(|index| archive.read(index).expect("reads"))
        .collect()
}

/// `py7zr-ppmd.7z`'s PPMd stream, order 6 in a 16 MiB arena, decoded to its
/// first `n` bytes: the folder's output, `prose.txt`, `runs.bin` and
/// `x86.bin`, cut at `n`.
fn ppmd_prefix(n: usize) -> u64 {
    let bytes = fixture("py7zr-ppmd.7z");
    let whole = whole_folder(&bytes);
    assert!(4 * 30_000 <= whole.len(), "the fixture holds both sizes");
    let props = [6u8, 0, 0, 0, 1];
    let limits = ppmd::Limits {
        max_unpacked: whole.len(),
        max_memory: 1 << 24,
    };
    let stream = packed_region(&bytes);
    let (decoded, steps) = ppmd::decode_counted(stream, &props, n, &limits);
    assert_eq!(decoded.as_deref(), Ok(&whole[..n]), "the prefix decodes");
    assert_eq!(
        ppmd::decode(stream, &props, n, &limits).as_deref(),
        Ok(&whole[..n])
    );
    steps
}

#[test]
fn more_bytes_through_ppmd_cost_work_linear_in_the_bytes() {
    // The arena never fills at either size, so the model never restarts.
    holds_the_law("PPMd n bytes", ppmd_prefix(30_000), ppmd_prefix(120_000));
}

/// `py7zr-bzip2.7z`'s bzip2 stream, found by its own end: the shortest
/// prefix of the packed region that decodes.
fn bzip2_stream() -> (Vec<u8>, Vec<u8>) {
    let bytes = fixture("py7zr-bzip2.7z");
    let whole = whole_folder(&bytes);
    let region = packed_region(&bytes);
    let limits = bzip2::Limits {
        max_unpacked: whole.len(),
    };
    let decodes = |len: usize| bzip2::decode(&region[..len], &limits).is_ok();
    assert!(decodes(region.len()), "the region holds the stream");
    let (mut lo, mut hi) = (0usize, region.len());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if decodes(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (region[..hi].to_vec(), whole)
}

/// The stream `n` times over, which bzip2 reads as `n` streams one after
/// another.
fn bzip2_streams(stream: &[u8], whole: &[u8], n: usize) -> u64 {
    let input = stream.repeat(n);
    let want = whole.repeat(n);
    let limits = bzip2::Limits {
        max_unpacked: want.len(),
    };
    let (decoded, steps) = bzip2::decode_counted(&input, &limits);
    assert_eq!(
        decoded.as_deref(),
        Ok(want.as_slice()),
        "every stream decodes"
    );
    assert_eq!(bzip2::decode(&input, &limits), Ok(want));
    steps
}

#[test]
fn more_bzip2_streams_cost_work_linear_in_the_streams() {
    let (stream, whole) = bzip2_stream();
    holds_the_law(
        "bzip2 n streams",
        bzip2_streams(&stream, &whole, 2),
        bzip2_streams(&stream, &whole, 8),
    );
}

// ---------------------------------------------------------------------------
// zstd, from RFC 8878's frame and block layouts.
// ---------------------------------------------------------------------------

/// A frame's header: the magic number, a descriptor with no content size and
/// no dictionary, `checksum` its one flag, and a window of 128 KiB.
fn zstd_header(checksum: bool) -> Vec<u8> {
    let mut out = 0xFD2F_B528u32.to_le_bytes().to_vec();
    out.push(if checksum { 0x04 } else { 0x00 });
    out.push(7 << 3);
    out
}

/// A block's three-byte header.
fn block_header(last: bool, kind: u32, size: usize) -> [u8; 3] {
    let value = u32::from(last) | (kind << 1) | ((size as u32) << 3);
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

/// One frame of `n` raw blocks of 1 KiB.
fn zstd_raw(n: usize) -> u64 {
    let data = bytes(n * 1024, 3);
    let mut frame = zstd_header(false);
    for (k, block) in data.chunks(1024).enumerate() {
        frame.extend(block_header(k + 1 == n, 0, block.len()));
        frame.extend_from_slice(block);
    }
    decoded_zstd(&frame, &data)
}

/// One frame of `n` RLE blocks of 1 KiB, each its own byte.
fn zstd_rle(n: usize) -> u64 {
    let mut frame = zstd_header(false);
    let mut data = Vec::new();
    for k in 0..n {
        let byte = (k * 7) as u8;
        frame.extend(block_header(k + 1 == n, 1, 1024));
        frame.push(byte);
        data.resize(data.len() + 1024, byte);
    }
    decoded_zstd(&frame, &data)
}

/// `n` frames, each one compressed block of 1 000 raw literals and no
/// sequences, with a checksum.
fn zstd_literals(n: usize) -> u64 {
    let data = bytes(n * 1000, 4);
    let mut input = Vec::new();
    for chunk in data.chunks(1000) {
        let size = chunk.len();
        let mut block = vec![(((size & 0xF) << 4) | (1 << 2)) as u8, (size >> 4) as u8];
        block.extend_from_slice(chunk);
        block.push(0); // no sequences
        input.extend(zstd_header(true));
        input.extend(block_header(true, 2, block.len()));
        input.extend_from_slice(&block);
        input.extend_from_slice(&((xxh64(chunk) & 0xFFFF_FFFF) as u32).to_le_bytes());
    }
    decoded_zstd(&input, &data)
}

fn decoded_zstd(input: &[u8], want: &[u8]) -> u64 {
    let limits = zstd::Limits {
        max_unpacked: want.len(),
    };
    let (decoded, steps) = zstd::decode_counted(input, &limits);
    assert_eq!(decoded.as_deref(), Ok(want), "the frames decode");
    assert_eq!(zstd::decode(input, &limits).as_deref(), Ok(want));
    steps
}

#[test]
fn more_zstd_blocks_and_frames_cost_work_linear_in_them() {
    holds_the_law("zstd n raw blocks", zstd_raw(64), zstd_raw(256));
    holds_the_law("zstd n RLE blocks", zstd_rle(64), zstd_rle(256));
    holds_the_law(
        "zstd n frames of raw literals",
        zstd_literals(64),
        zstd_literals(256),
    );
}

#[test]
fn the_steps_are_counted_whether_or_not_the_input_reads() {
    let (opened, steps) = tar::Archive::open_counted(&[0u8; 600], &tar::Limits::DEFAULT);
    assert!(opened.is_err(), "no ustar magic");
    assert_eq!(steps, 0, "nothing was walked before the refusal");

    let mut damaged = rar_of(&files(3));
    let last = damaged.len() - 20;
    damaged[last] ^= 0xFF;
    let (opened, steps) = rar::Archive::open_counted(&damaged, &rar::Limits::DEFAULT);
    assert!(
        opened.is_ok(),
        "a damaged later header ends the walk, not the archive"
    );
    assert!(steps > 0);

    let symbols: Vec<Symbol> = bytes(64, 5).into_iter().map(Symbol::Literal).collect();
    let stream = lzma_stream(&symbols, 0, 0);
    let (decoded, steps) =
        lzma::decode_counted(&stream[..stream.len() / 2], LZMA_PROPS, 64, &lzma_limits());
    assert!(decoded.is_err(), "a stream cut in half is refused");
    assert!(steps > 0, "the bits decoded before the refusal are counted");

    let (decoded, steps) = zstd::decode_counted(b"not a frame", &zstd::Limits { max_unpacked: 64 });
    assert!(decoded.is_err());
    assert_eq!(steps, 0);
}
