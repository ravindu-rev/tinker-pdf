//! Size-scaling complexity tests for the ZIP reader's listing
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! Each shape is an archive this file writes at a size n and at four times n,
//! from APPNOTE's own record layouts, and listed through
//! [`Archive::open_counted`], which returns the steps the listing took: each
//! record and each local header read, each position a signature search
//! examined, each byte of a name decoded and each byte a streamed entry
//! inflated to find its end. The law is the design's: the work at 4 n is at
//! most 1.1 × 4 times the work at n. A quadratic shape costs sixteen times as
//! much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: every
//! entry is listed by the route the shape is for, and reads back as it was
//! written, through the counted door and the uncounted one alike. Four times
//! n stays inside the archive's entry cap. No clock is read here.

use tinker_pdf_filters::{crc32, deflate};
use tinker_pdf_zip::{Archive, Limits, Route, Warning};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// The law for a shape whose parser reads each element a bounded number of
/// times, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:40} work at n {n:>8}  at 4n {kn:>8}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

/// One entry this file writes.
struct File {
    name: String,
    data: Vec<u8>,
    /// Deflated and streamed: general-purpose bit 3, zeroes in the local
    /// header, the CRC and sizes in a signed descriptor after the data.
    streamed: bool,
}

/// The entry at `i`: a name and a few bytes that say which entry it is.
fn file(i: usize, streamed: bool) -> File {
    File {
        name: format!("page{i:05}.png"),
        data: format!("the bytes of page {i}").into_bytes(),
        streamed,
    }
}

fn u16le(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&u16::try_from(value).expect("fits").to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&u32::try_from(value).expect("fits").to_le_bytes());
}

/// The archive's local headers and data (APPNOTE 4.3.7, 4.3.9), and the
/// central directory and its end record (4.3.12, 4.3.16) when `directory`.
fn archive(files: &[File], directory: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for f in files {
        let offset = out.len() as u64;
        let crc = u64::from(crc32(&f.data));
        let (method, flags, payload) = if f.streamed {
            (8usize, 0x0808usize, deflate(&f.data))
        } else {
            (0, 0x0800, f.data.clone())
        };
        let (compressed, uncompressed) = (payload.len() as u64, f.data.len() as u64);

        out.extend_from_slice(b"PK\x03\x04");
        u16le(&mut out, 20); // version needed
        u16le(&mut out, flags);
        u16le(&mut out, method);
        u16le(&mut out, 0); // time
        u16le(&mut out, 0); // date
        let (local_crc, local_compressed, local_uncompressed) = if f.streamed {
            (0, 0, 0)
        } else {
            (crc, compressed, uncompressed)
        };
        u32le(&mut out, local_crc);
        u32le(&mut out, local_compressed);
        u32le(&mut out, local_uncompressed);
        u16le(&mut out, f.name.len());
        u16le(&mut out, 0); // extra
        out.extend_from_slice(f.name.as_bytes());
        out.extend_from_slice(&payload);
        if f.streamed {
            out.extend_from_slice(b"PK\x07\x08");
            u32le(&mut out, crc);
            u32le(&mut out, compressed);
            u32le(&mut out, uncompressed);
        }

        central.extend_from_slice(b"PK\x01\x02");
        u16le(&mut central, 20); // version made by
        u16le(&mut central, 20); // version needed
        u16le(&mut central, flags);
        u16le(&mut central, method);
        u16le(&mut central, 0);
        u16le(&mut central, 0);
        u32le(&mut central, crc);
        u32le(&mut central, compressed);
        u32le(&mut central, uncompressed);
        u16le(&mut central, f.name.len());
        u16le(&mut central, 0); // extra
        u16le(&mut central, 0); // comment
        u16le(&mut central, 0); // disk
        u16le(&mut central, 0); // internal attributes
        u32le(&mut central, 0); // external attributes
        u32le(&mut central, offset);
        central.extend_from_slice(f.name.as_bytes());
    }
    if directory {
        let start = out.len() as u64;
        let size = central.len() as u64;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u16le(&mut out, files.len());
        u16le(&mut out, files.len());
        u32le(&mut out, size);
        u32le(&mut out, start);
        u16le(&mut out, 0); // comment
    }
    out
}

/// Lists `files` written with or without a directory, checks the route and
/// every entry, and returns the listing's steps.
fn listed(files: &[File], directory: bool, route: Route) -> u64 {
    let bytes = archive(files, directory);
    let (opened, steps) = Archive::open_counted(&bytes, &Limits::DEFAULT);
    let mut archive = opened.expect("the archive opens");
    assert_eq!(
        archive.route(),
        route,
        "listed by the route the shape is for"
    );
    assert_eq!(
        archive.entries().len(),
        files.len(),
        "every entry is listed"
    );
    for (entry, f) in archive.entries().to_vec().iter().zip(files) {
        assert_eq!(entry.name, f.name, "entry {} keeps its name", entry.index);
        let data = archive.read(entry.index).expect("the entry reads");
        assert_eq!(&*data, f.data.as_slice(), "and its bytes");
    }
    let plain = Archive::open(&bytes, &Limits::DEFAULT).expect("and through the uncounted door");
    assert_eq!(plain.entries(), archive.entries());
    steps
}

fn central_directory(n: usize) -> u64 {
    let files: Vec<File> = (0..n).map(|i| file(i, false)).collect();
    listed(&files, true, Route::CentralDirectory)
}

#[test]
fn a_central_directory_of_more_entries_costs_work_linear_in_its_entries() {
    holds_the_law(
        "ZIP n entries by the central directory",
        central_directory(1000),
        central_directory(4000),
    );
}

fn local_scan(n: usize) -> u64 {
    let files: Vec<File> = (0..n).map(|i| file(i, false)).collect();
    listed(&files, false, Route::LocalHeaderScan)
}

#[test]
fn a_local_header_scan_of_more_entries_costs_work_linear_in_its_entries() {
    // With no end record the search for one examines the whole of the last
    // 64 KiB, at both sizes; every header after it is found where the entry
    // before it ends.
    holds_the_law(
        "ZIP n entries by the local-header scan",
        local_scan(1000),
        local_scan(4000),
    );
}

fn streamed_scan(n: usize) -> u64 {
    let files: Vec<File> = (0..n).map(|i| file(i, true)).collect();
    listed(&files, false, Route::LocalHeaderScan)
}

#[test]
fn streamed_entries_recovered_by_the_scan_cost_work_linear_in_their_entries() {
    // Each streamed entry is inflated once to find where its descriptor is,
    // and the next header is found after it.
    holds_the_law(
        "ZIP n streamed entries by the scan",
        streamed_scan(500),
        streamed_scan(2000),
    );
}

#[test]
fn the_steps_are_counted_whether_or_not_the_archive_opens() {
    let (opened, steps) = Archive::open_counted(b"not an archive at all", &Limits::DEFAULT);
    assert!(opened.is_err(), "not a ZIP");
    assert!(steps > 0, "the searches that decided so are counted");

    let files: Vec<File> = (0..3).map(|i| file(i, false)).collect();
    let mut bytes = archive(&files, true);
    // Point the end record's directory offset past the archive: the
    // directory is unreadable, the scan recovers the entries, and both are
    // counted.
    let at = bytes.len() - 6;
    bytes[at..at + 4].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
    let (opened, steps) = Archive::open_counted(&bytes, &Limits::DEFAULT);
    let archive = opened.expect("recovered");
    assert_eq!(archive.route(), Route::LocalHeaderScan);
    assert!(archive
        .warnings()
        .contains(&Warning::CentralDirectoryUnreadable));
    assert!(steps > 0);
}
