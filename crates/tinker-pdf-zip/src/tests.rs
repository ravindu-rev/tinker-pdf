//! What this crate is tested against, and why each test is the shape it is.
//!
//! Every archive here is hand-built by [`crate::build`], byte by byte, against
//! APPNOTE's field layouts rather than against another implementation. There
//! is no `.cbz` in this repository to round-trip, and gap 29 says to state
//! that plainly rather than imply coverage that does not exist — as gap 17 did
//! for JBIG2, where every fixture was written from T.88's Annex H tables.
//!
//! # A round trip through one writer proves one writer
//!
//! Gap 18 milestone 6 found a JPEG 2000 plane count that was correct only for
//! the code-blocks its own in-tree encoder happened to produce; every round
//! trip took the same working path, for milestones, and agreed with itself.
//! The tests below therefore lean on three things a self-round-trip cannot
//! give:
//!
//! - **Published constants, entry by entry.** The CRC-32 vectors are the ones
//!   in the standard, not ones this crate computed.
//! - **Shapes no in-tree encoder emits.** `File::raw` exists so a test can
//!   write a deflate stream beginning with a *stored* block — the case a zlib
//!   header sniff misreads — which `deflate` never produces.
//! - **Damage.** Half these archives are broken on purpose, because the
//!   recovery rung and the refusals are most of what this crate is for.

use crate::build::{archive, Damage, File};
use crate::limits::{MAX_ZIP_ENTRIES, MAX_ZIP_ENTRY_BYTES, MAX_ZIP_NAME_LEN};
use crate::{Archive, ArchiveError, EntryError, Limits, Method, Route, Warning};

/// A shorthand: open with the shipped bounds.
fn open(bytes: &[u8]) -> Result<Archive<'_>, ArchiveError> {
    Archive::open(bytes, &Limits::DEFAULT)
}

// ---------------------------------------------------------------------------
// The route that works
// ---------------------------------------------------------------------------

#[test]
fn a_stored_entry_round_trips_through_the_central_directory() {
    let zip = archive(&[File::stored(b"page01.jpg", b"hello world")], Damage::None);
    let mut a = open(&zip).unwrap();

    assert_eq!(a.route(), Route::CentralDirectory);
    assert_eq!(a.entries().len(), 1);
    assert_eq!(a.entries()[0].name, "page01.jpg");
    assert_eq!(a.entries()[0].method, Method::Stored);
    assert_eq!(&*a.read(0).unwrap(), b"hello world");
    assert!(a.warnings().is_empty(), "{:?}", a.warnings());
}

#[test]
fn a_deflated_entry_round_trips_through_the_central_directory() {
    // Repetitive enough that deflate actually compresses it, so the test is
    // exercising the inflate path rather than a stored block wearing method 8.
    let data = b"abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabc".repeat(8);
    let zip = archive(&[File::deflated(b"page01.png", &data)], Damage::None);
    let mut a = open(&zip).unwrap();

    assert_eq!(a.entries()[0].method, Method::Deflated);
    assert!(
        a.entries()[0].compressed_size < a.entries()[0].uncompressed_size,
        "fixture did not actually compress, so this proves nothing about inflate"
    );
    assert_eq!(&*a.read(0).unwrap(), &data[..]);
}

#[test]
fn a_stored_entry_is_borrowed_and_a_deflated_one_is_owned() {
    // Not a stylistic assertion. Gap 29's memory argument is that a 200-page
    // archive stays near the archive's own size because stored bytes are
    // handed back as a subslice; the moment this copies, a 3.6 GB peak comes
    // back. `Cow`'s discriminant is the only place that is observable.
    let zip = archive(
        &[
            File::stored(b"a.jpg", b"stored bytes"),
            File::deflated(b"b.jpg", &b"xyzxyzxyzxyz".repeat(16)),
        ],
        Damage::None,
    );
    let mut a = open(&zip).unwrap();

    assert!(
        matches!(a.read(0).unwrap(), std::borrow::Cow::Borrowed(_)),
        "a stored entry was copied; pass-through is gone"
    );
    assert!(matches!(a.read(1).unwrap(), std::borrow::Cow::Owned(_)));
}

#[test]
fn entries_keep_the_order_the_directory_gave_them() {
    // Ordering by name is gap 29's job, in the facade, with a natural sort.
    // This crate's contract is narrower and worth pinning: it hands back what
    // the archive said, in archive order, so the sort above it has something
    // stable to sort.
    let zip = archive(
        &[
            File::stored(b"page10.jpg", b"ten"),
            File::stored(b"page2.jpg", b"two"),
            File::stored(b"page1.jpg", b"one"),
        ],
        Damage::None,
    );
    let a = open(&zip).unwrap();
    let names: Vec<&str> = a.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["page10.jpg", "page2.jpg", "page1.jpg"]);
    assert_eq!(a.entries()[2].index, 2);
}

// ---------------------------------------------------------------------------
// The raw door: the case a zlib header sniff gets wrong
// ---------------------------------------------------------------------------

/// A raw DEFLATE stream whose first byte is `0x08`, which is also a valid zlib
/// CMF.
///
/// Bit 0 is BFINAL=0, bits 1-2 are BTYPE=00 (stored), and bit 3 is padding the
/// decoder skips on its way to the byte boundary — so setting it makes the low
/// nibble 8, which is zlib's CM for deflate. Choosing the stored block's LEN
/// low byte as 29 makes `0x081D` a multiple of 31, which is zlib's header
/// checksum, and 29 has bit 5 clear, so FDICT is clear too.
///
/// The result is a raw stream that passes **every** test `flate_bytes` applies
/// before deciding it is looking at a zlib wrapper. A reader that sniffs would
/// skip these two bytes and inflate from the middle of a stored block's
/// length field. This crate never sniffs — a ZIP entry is raw by definition —
/// and this fixture is what says so.
fn stored_block_first() -> (Vec<u8>, Vec<u8>) {
    let data: Vec<u8> = (0u8..29).collect();
    let len = data.len() as u16;
    let mut raw = vec![0x08];
    raw.extend_from_slice(&len.to_le_bytes());
    raw.extend_from_slice(&(!len).to_le_bytes());
    raw.extend_from_slice(&data);
    // A final empty stored block, so the stream ends where it says it does.
    raw.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    (data, raw)
}

#[test]
fn a_deflate_stream_beginning_with_a_stored_block_decodes() {
    let (data, raw) = stored_block_first();
    assert_eq!(raw[0], 0x08, "fixture must collide with zlib's CM");
    assert_eq!(
        (u16::from(raw[0]) << 8 | u16::from(raw[1])) % 31,
        0,
        "fixture must also pass zlib's mod-31 header check, or it proves nothing"
    );
    assert_eq!(raw[1] & 0x20, 0, "fixture must look like FDICT=0");

    let mut file = File::deflated(b"page01.jpg", &data);
    file.raw = Some(raw);
    let zip = archive(&[file], Damage::None);

    let mut a = open(&zip).unwrap();
    assert_eq!(
        &*a.read(0).unwrap(),
        &data[..],
        "the entry was read through a zlib sniff rather than the raw door"
    );
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

#[test]
fn an_archive_with_no_directory_is_recovered_from_local_headers() {
    let zip = archive(
        &[
            File::stored(b"page01.jpg", b"first page"),
            File::deflated(b"page02.jpg", &b"second page ".repeat(8)),
        ],
        Damage::NoDirectory,
    );
    let mut a = open(&zip).unwrap();

    assert_eq!(a.route(), Route::LocalHeaderScan);
    assert_eq!(a.entries().len(), 2, "a truncated archive lost a page");
    assert_eq!(&*a.read(0).unwrap(), b"first page");
    assert_eq!(&*a.read(1).unwrap(), &b"second page ".repeat(8)[..]);
    assert!(a.warnings().contains(&Warning::NoEndOfCentralDirectory));
    assert!(a.warnings().contains(&Warning::RecoveredFromLocalHeaders));
}

#[test]
fn the_two_ways_a_directory_can_be_missing_are_reported_apart() {
    // A writer that stopped before the directory, and a file whose directory
    // is there but points nowhere. Both recover; each says which happened.
    let files = [File::stored(b"page01.jpg", b"body")];
    let absent_bytes = archive(&files, Damage::NoDirectory);
    let broken_bytes = archive(&files, Damage::DirectoryOffsetWrong);
    let absent = open(&absent_bytes).unwrap();
    let broken = open(&broken_bytes).unwrap();

    assert!(absent
        .warnings()
        .contains(&Warning::NoEndOfCentralDirectory));
    assert!(!absent
        .warnings()
        .contains(&Warning::CentralDirectoryUnreadable));

    assert!(broken
        .warnings()
        .contains(&Warning::CentralDirectoryUnreadable));
    assert!(!broken
        .warnings()
        .contains(&Warning::NoEndOfCentralDirectory));
}

#[test]
fn a_directory_pointing_nowhere_falls_back_rather_than_failing() {
    let zip = archive(
        &[File::stored(b"page01.jpg", b"still here")],
        Damage::DirectoryOffsetWrong,
    );
    let mut a = open(&zip).unwrap();

    assert_eq!(a.route(), Route::LocalHeaderScan);
    assert!(a.warnings().contains(&Warning::CentralDirectoryUnreadable));
    assert_eq!(&*a.read(0).unwrap(), b"still here");
}

#[test]
fn a_directory_claiming_more_entries_than_it_wrote_says_so() {
    let zip = archive(
        &[File::stored(b"page01.jpg", b"one")],
        Damage::DirectoryCountTooHigh,
    );
    let a = open(&zip).unwrap();

    // The count is a claim; the walk is the truth. Both are reported.
    assert_eq!(a.entries().len(), 1);
    assert!(a.warnings().contains(&Warning::EntryCountDisagrees));
}

#[test]
fn image_bytes_containing_a_local_signature_do_not_become_an_entry() {
    // `PK\x03\x04` appears in real image data often enough that a scan which
    // trusted every hit would invent pages. The recovery rung must find two
    // entries here, not three.
    //
    // The embedded bytes are a *well-formed* local header, not four bytes and
    // some noise: a malformed one is rejected by `parse_local` before the
    // question of skipping ever arises, so a fixture built that way passes
    // whether or not the scan resumes past the data. It has to be a header
    // that would parse, so that stepping over the entry's data is the only
    // thing standing between the scan and an invented page.
    let mut payload = b"leading".to_vec();
    payload.extend_from_slice(b"PK\x03\x04");
    payload.extend_from_slice(&20u16.to_le_bytes()); // version needed
    payload.extend_from_slice(&0u16.to_le_bytes()); // flags
    payload.extend_from_slice(&0u16.to_le_bytes()); // stored
    payload.extend_from_slice(&0u16.to_le_bytes()); // time
    payload.extend_from_slice(&0u16.to_le_bytes()); // date
    payload.extend_from_slice(&0u32.to_le_bytes()); // crc
    payload.extend_from_slice(&0u32.to_le_bytes()); // compressed
    payload.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
    payload.extend_from_slice(&8u16.to_le_bytes()); // name len
    payload.extend_from_slice(&0u16.to_le_bytes()); // extra len
    payload.extend_from_slice(b"fake.jpg");
    payload.extend_from_slice(b"trailing");

    let zip = archive(
        &[
            File::stored(b"page01.jpg", &payload),
            File::stored(b"page02.jpg", b"real second page"),
        ],
        Damage::NoDirectory,
    );
    let mut a = open(&zip).unwrap();

    assert_eq!(
        a.entries().len(),
        2,
        "a signature inside image data was read as an entry: {:?}",
        a.entries().iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    assert_eq!(&*a.read(0).unwrap(), &payload[..]);
    assert_eq!(&*a.read(1).unwrap(), b"real second page");
}

// ---------------------------------------------------------------------------
// Streamed entries: what recovery can and cannot recover
// ---------------------------------------------------------------------------

#[test]
fn a_streamed_deflated_entry_is_recovered_from_where_the_stream_ended() {
    // The local header carries zeroes; the real sizes follow the data. The
    // only thing that knows where the data ended is the decoder, which is what
    // `inflate_raw`'s `end` reports and why it was made public.
    let data = b"streamed and deflated ".repeat(12);
    let mut file = File::deflated(b"page01.jpg", &data);
    file.streamed = true;
    let zip = archive(&[file], Damage::NoDirectory);

    let mut a = open(&zip).unwrap();
    assert_eq!(a.route(), Route::LocalHeaderScan);
    assert_eq!(a.entries().len(), 1);
    assert_eq!(a.entries()[0].uncompressed_size, data.len() as u64);
    assert_eq!(&*a.read(0).unwrap(), &data[..]);
}

#[test]
fn a_streamed_stored_entry_is_listed_and_then_refused() {
    // No descriptor findable without the length, no length without the
    // descriptor. Gap 17's rule decides the outcome: the entry is *listed*, so
    // a 20-page archive still reports 20 pages, and refused at read rather
    // than handed back at a guessed length.
    let mut file = File::stored(b"page01.jpg", b"no findable extent");
    file.streamed = true;
    let zip = archive(&[file], Damage::NoDirectory);

    let mut a = open(&zip).unwrap();
    assert_eq!(a.entries().len(), 1, "the page count went quietly short");
    assert_eq!(a.entries()[0].crc, None);
    assert_eq!(a.read(0), Err(EntryError::NoChecksum));
}

#[test]
fn a_streamed_entry_read_through_the_directory_is_fine() {
    // The directory holds the real sizes whatever the local header says, so
    // the same entry that recovery cannot size reads normally when the
    // directory survived. This is why the refusal above is route-specific and
    // not a property of streaming.
    let mut file = File::stored(b"page01.jpg", b"sized by the directory");
    file.streamed = true;
    let zip = archive(&[file], Damage::None);

    let mut a = open(&zip).unwrap();
    assert_eq!(a.route(), Route::CentralDirectory);
    assert_eq!(&*a.read(0).unwrap(), b"sized by the directory");
}

// ---------------------------------------------------------------------------
// Refusals, each by name
// ---------------------------------------------------------------------------

#[test]
fn a_corrupt_entry_is_refused_rather_than_handed_over() {
    let zip = archive(
        &[File::stored(b"page01.jpg", b"tampered")],
        Damage::CorruptCrc,
    );
    let mut a = open(&zip).unwrap();

    match a.read(0) {
        Err(EntryError::ChecksumMismatch { declared, computed }) => {
            assert_ne!(declared, computed);
        }
        other => panic!("expected a named checksum refusal, got {other:?}"),
    }
}

#[test]
fn an_encrypted_entry_is_refused_by_name() {
    let mut file = File::stored(b"page01.jpg", b"secret");
    file.encrypted = true;
    let zip = archive(&[file], Damage::None);

    let mut a = open(&zip).unwrap();
    assert!(a.entries()[0].encrypted);
    assert_eq!(a.read(0), Err(EntryError::Encrypted));
}

#[test]
fn an_unsupported_method_is_refused_with_its_number() {
    // Method 12 is bzip2, which this crate does not implement. The number
    // travels with the refusal so a caller can say which method, rather than
    // "unsupported".
    let mut zip = archive(&[File::stored(b"page01.jpg", b"body")], Damage::None);
    // Method sits at +8 in the local header and +10 in the central record;
    // patch both so the two agree and the refusal is about the method rather
    // than about a disagreement.
    let local = crate::le::find(&zip, 0, b"PK\x03\x04").unwrap();
    zip[local + 8..local + 10].copy_from_slice(&12u16.to_le_bytes());
    let central = crate::le::find(&zip, 0, b"PK\x01\x02").unwrap();
    zip[central + 10..central + 12].copy_from_slice(&12u16.to_le_bytes());

    let mut a = open(&zip).unwrap();
    assert_eq!(a.entries()[0].method, Method::Other(12));
    assert_eq!(a.read(0), Err(EntryError::UnsupportedMethod(12)));
}

#[test]
fn bytes_that_are_not_a_zip_are_refused_by_name() {
    assert_eq!(open(b"%PDF-1.7\n").unwrap_err(), ArchiveError::NotAZip);
    assert_eq!(open(b"").unwrap_err(), ArchiveError::NotAZip);
    // A bare local signature with nothing behind it is the other answer: the
    // bytes say ZIP and there is no entry to be had, which is damage rather
    // than a different format. Keeping these two apart is the whole reason
    // both variants exist -- a caller shows "this is not a comic archive" for
    // one and "this archive is damaged" for the other.
    assert_eq!(open(b"PK\x03\x04").unwrap_err(), ArchiveError::Damaged);
}

#[test]
fn a_multi_disk_archive_is_refused_rather_than_read_in_part() {
    let mut zip = archive(&[File::stored(b"page01.jpg", b"one of many")], Damage::None);
    let eocd = crate::le::rfind(&zip, b"PK\x05\x06").unwrap();
    zip[eocd + 4..eocd + 6].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(open(&zip).unwrap_err(), ArchiveError::MultiDisk);
}

#[test]
fn a_zip64_sentinel_with_no_zip64_record_is_refused_by_name() {
    let mut zip = archive(&[File::stored(b"page01.jpg", b"body")], Damage::None);
    let eocd = crate::le::rfind(&zip, b"PK\x05\x06").unwrap();
    // "The real offset is in the Zip64 record" — and there is not one.
    zip[eocd + 16..eocd + 20].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    assert_eq!(open(&zip).unwrap_err(), ArchiveError::Zip64OutOfBounds);
}

// ---------------------------------------------------------------------------
// The bounds, each proved to fire by its own refusal
// ---------------------------------------------------------------------------

#[test]
fn an_archive_with_more_entries_than_the_cap_is_refused_by_name() {
    // Named in `limits::MAX_ZIP_ENTRIES`'s "reachable" note, and it builds the
    // real thing at the real cap rather than lowering the bound to make the
    // test cheap: gap 18 M8's failure was a constant nobody had ever driven to
    // its own edge.
    let files: Vec<File> = (0..MAX_ZIP_ENTRIES + 1)
        .map(|i| File::stored(format!("{i:05}").as_bytes(), b""))
        .collect();
    let zip = archive(&files, Damage::None);
    assert_eq!(open(&zip).unwrap_err(), ArchiveError::TooManyEntries);

    // And one below it opens, so the cap is a cap and not an off-by-one.
    let zip = archive(&files[..MAX_ZIP_ENTRIES - 1], Damage::None);
    assert_eq!(open(&zip).unwrap().entries().len(), MAX_ZIP_ENTRIES - 1);
}

#[test]
fn a_name_past_the_cap_is_truncated_and_says_so() {
    // Named in `limits::MAX_ZIP_NAME_LEN`. Truncated rather than dropped:
    // dropping the entry would drop a page and renumber every page after it.
    let name = vec![b'a'; MAX_ZIP_NAME_LEN + 500];
    let zip = archive(&[File::stored(&name, b"body")], Damage::None);
    let mut a = open(&zip).unwrap();

    assert_eq!(a.entries().len(), 1, "an over-long name lost the page");
    assert_eq!(a.entries()[0].name.len(), MAX_ZIP_NAME_LEN);
    assert!(a.warnings().contains(&Warning::NameTruncated));
    assert_eq!(&*a.read(0).unwrap(), b"body");
}

#[test]
fn an_entry_declaring_more_than_the_per_entry_cap_is_refused_before_it_allocates() {
    // The declaration is patched, not produced: the point is that a 4 GB claim
    // costs a refusal rather than 4 GB of `Vec`, so the fixture must lie.
    let data = b"small entry, large claim ".repeat(4);
    let mut zip = archive(&[File::deflated(b"page01.jpg", &data)], Damage::None);
    let central = crate::le::rfind(&zip, b"PK\x01\x02").unwrap();
    let huge = (MAX_ZIP_ENTRY_BYTES as u32).wrapping_add(1);
    zip[central + 24..central + 28].copy_from_slice(&huge.to_le_bytes());

    let mut a = open(&zip).unwrap();
    assert_eq!(a.read(0), Err(EntryError::EntryTooLarge));
}

#[test]
fn a_zip_bomb_is_refused_by_name_rather_than_by_running_out_of_memory() {
    // `42.zip`'s shape without its size: many entries, each inflating to far
    // more than it costs on disk, so no single entry trips the per-entry cap
    // and only the archive total can stop it. That is the distinction
    // `MAX_ZIP_ENTRY_BYTES`'s doc comment insists on, and this is the test
    // that would fail if the total were dropped.
    let page = vec![0u8; 64 * 1024];
    let files: Vec<File> = (0..64)
        .map(|i| File::deflated(format!("page{i:03}.bin").as_bytes(), &page))
        .collect();
    let zip = archive(&files, Damage::None);

    // A total small enough that the archive crosses it partway through.
    let limits = Limits {
        max_inflated_total: 256 * 1024,
        ..Limits::DEFAULT
    };
    let mut a = Archive::open(&zip, &limits).unwrap();
    assert_eq!(
        a.entries().len(),
        64,
        "the bomb was refused at open, not at read"
    );

    let mut refused = None;
    for i in 0..64 {
        if let Err(e) = a.read(i) {
            refused = Some((i, e));
            break;
        }
    }
    let (at, err) = refused.expect("the total never fired; it is not a cap");
    assert_eq!(err, EntryError::ArchiveBudgetSpent);
    assert!(at > 0, "the first entry alone should fit under the total");
    // Spent and never refunded: re-reading an entry that already succeeded
    // does not buy the budget back.
    assert_eq!(a.read(at), Err(EntryError::ArchiveBudgetSpent));
}

#[test]
fn stored_entries_do_not_spend_the_inflation_total() {
    // Stored data is a subslice: it allocates nothing and cannot expand, and
    // the total counts inflation because inflation is where bounded input
    // becomes unbounded output. An archive of stored pages far past the total
    // must therefore read fine.
    let page = vec![7u8; 32 * 1024];
    let files: Vec<File> = (0..16)
        .map(|i| File::stored(format!("page{i:03}.bin").as_bytes(), &page))
        .collect();
    let zip = archive(&files, Damage::None);

    let limits = Limits {
        max_inflated_total: 1024,
        ..Limits::DEFAULT
    };
    let mut a = Archive::open(&zip, &limits).unwrap();
    for i in 0..16 {
        assert_eq!(a.read(i).unwrap().len(), page.len());
    }
    assert_eq!(a.inflated(), 0, "a stored entry was charged for inflation");
}

#[test]
fn the_fixtures_in_this_crate_spend_what_the_ledger_says() {
    // Named in `limits`'s module docs: the first of the three numbers behind
    // every constant is *measured*, not asserted, so the ledger cannot drift
    // away from the code without this failing.
    let data = b"abcabcabcabcabcabcabcabcabcabcabcabc".repeat(8);
    let zip = archive(
        &[
            File::stored(b"page01.jpg", b"stored costs nothing"),
            File::deflated(b"page02.jpg", &data),
        ],
        Damage::None,
    );
    let mut a = open(&zip).unwrap();
    a.read(0).unwrap();
    a.read(1).unwrap();

    assert_eq!(a.entries().len(), 2);
    assert_eq!(
        a.inflated(),
        data.len(),
        "only the deflated entry is charged"
    );
    assert!(
        a.inflated() <= 1024,
        "the ledger says no fixture here spends more than 1 024 bytes, and this one spent {}",
        a.inflated()
    );
}

#[test]
fn a_short_entry_is_refused_for_its_length_rather_than_its_checksum() {
    // Both checks refuse, so a test that only asserts "refused" cannot tell
    // them apart -- and the injection matrix duly found that dropping the
    // length check entirely changed nothing any test could see. The two say
    // different things to whoever reads the error: "this archive promised 5000
    // bytes and produced 4000" points at a truncated file, and "checksum
    // wrong" points at a corrupted one. The more specific one has to come
    // first, and this is what holds it there.
    let data = b"a page of content ".repeat(16);
    let mut zip = archive(&[File::deflated(b"page01.jpg", &data)], Damage::None);
    let central = crate::le::rfind(&zip, b"PK\x01\x02").unwrap();
    let inflated = u32::try_from(data.len()).unwrap();
    let overclaimed = inflated + 64;
    zip[central + 24..central + 28].copy_from_slice(&overclaimed.to_le_bytes());

    let mut a = open(&zip).unwrap();
    match a.read(0) {
        Err(EntryError::SizeMismatch { declared, produced }) => {
            assert_eq!(declared, u64::from(overclaimed));
            assert_eq!(produced, data.len() as u64);
        }
        other => panic!("expected the length refusal to come first, got {other:?}"),
    }
}

/// A streamed entry built so that trusting a failed decode's `end` finds a
/// descriptor that **validates**.
///
/// This is what makes the `complete` guard load-bearing rather than tidy. On
/// an ordinary truncated stream the decoder consumes the whole remaining file,
/// so a descriptor read from `end` runs off the end and fails anyway, and the
/// guard changes nothing observable -- which is exactly what the injection
/// matrix reported when the first version of this test used a plain
/// truncation. The guard earns its place against an archive that was *built*
/// for it, because `expected_compressed` is derived from the same `end` it is
/// meant to check, so the validation is self-consistent and confirms a number
/// it took on trust.
///
/// The payload is a complete non-final stored block, then one byte the decoder
/// must reject as a block header (BTYPE=11 is reserved, RFC 1951 3.2.3), then
/// a well-formed unsigned data descriptor placed exactly where the failed
/// decode reports it stopped -- and declaring exactly that offset as its
/// compressed size, which is the number `extent` would have brought to check
/// it with.
fn descriptor_that_validates_against_a_failed_decode() -> Vec<u8> {
    let body = [0xA5u8; 16];
    let mut raw = vec![0x00]; // BFINAL=0, BTYPE=00: stored, and not the last
    raw.extend_from_slice(&(body.len() as u16).to_le_bytes());
    raw.extend_from_slice(&(!(body.len() as u16)).to_le_bytes());
    raw.extend_from_slice(&body);

    // `end` counts the byte the decoder choked on, not the one before it, so
    // the descriptor goes after this byte rather than in place of it.
    raw.push(0x06);
    let end = raw.len();

    raw.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    raw.extend_from_slice(&(end as u32).to_le_bytes()); // compressed == end
    raw.extend_from_slice(&999u32.to_le_bytes()); // an uncompressed size from nowhere
    raw
}

#[test]
fn a_failed_decode_does_not_get_to_name_its_own_extent() {
    let raw = descriptor_that_validates_against_a_failed_decode();

    // The fixture only proves something if the decode really does fail here,
    // and really does stop where the descriptor was placed.
    let probe = tinker_pdf_filters::inflate_raw(&raw, &tinker_pdf_filters::Limits::new(1 << 20));
    assert!(
        !probe.complete,
        "the fixture decoded cleanly and proves nothing"
    );
    assert_eq!(
        probe.end, 22,
        "the fixture stopped somewhere else: {probe:?}"
    );

    let mut file = File::deflated(b"page01.jpg", &[0xA5; 16]);
    file.streamed = true;
    file.raw = Some(raw);
    let zip = archive(&[file], Damage::NoDirectory);

    let mut a = open(&zip).unwrap();
    assert_eq!(a.entries().len(), 1);
    assert_eq!(
        a.entries()[0].crc,
        None,
        "a checksum was taken from a descriptor found by trusting a failed decode"
    );
    assert_eq!(a.entries()[0].uncompressed_size, 0);
    assert_eq!(a.read(0), Err(EntryError::NoChecksum));
}

#[test]
fn a_truncated_streamed_entry_is_not_sized_from_where_the_decoder_gave_up() {
    // `inflate_raw`'s `end` is where the final block ended, and it means
    // nothing unless the decode completed -- on a truncated stream it is just
    // where the decoder stopped, which is inside the entry's own data. Reading
    // a descriptor from there produces a size, a CRC and an uncompressed
    // length that are three arbitrary numbers from the middle of an image.
    //
    // Bit 3 with no directory is the one route where nothing else can catch
    // that: the local header's own sizes are zeroes by definition, so there is
    // no second copy to disagree with.
    let data = b"streamed, deflated, and about to be cut short ".repeat(24);
    let mut file = File::deflated(b"page01.jpg", &data);
    file.streamed = true;
    let zip = archive(&[file], Damage::NoDirectory);

    // Cut inside the deflate stream, past the header so the entry is still
    // found, before the descriptor so its extent is genuinely unknowable.
    let cut = zip.len() - 12;
    let mut a = open(&zip[..cut]).unwrap();

    assert_eq!(
        a.entries().len(),
        1,
        "the page vanished instead of being refused"
    );
    assert_eq!(
        a.entries()[0].crc,
        None,
        "a checksum was invented for a stream whose end was never found"
    );
    assert_eq!(a.entries()[0].uncompressed_size, 0);
    assert_eq!(a.read(0), Err(EntryError::NoChecksum));
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

#[test]
fn a_cp437_name_is_decoded_rather_than_mangled() {
    // Bit 11 clear means CP437, which is not Latin-1 and not UTF-8. 0x81 is
    // `ü` in CP437 and a continuation byte in UTF-8, so a reader that guessed
    // would produce a replacement character here.
    let mut file = File::stored(b"seite\x81.jpg", b"body");
    file.utf8 = false;
    let zip = archive(&[file], Damage::None);

    let a = open(&zip).unwrap();
    assert_eq!(a.entries()[0].name, "seiteü.jpg");
    // And *no* warning. A name with bit 11 clear is not an anomaly, it is the
    // legacy encoding working as specified; warning here would fire on most
    // archives written before 2007 and make the warning list worth ignoring.
    // `NameNotUtf8` is for a name that *claimed* UTF-8 and was not.
    assert!(a.warnings().is_empty(), "{:?}", a.warnings());
}

#[test]
fn a_utf8_name_is_taken_at_its_word() {
    let zip = archive(
        &[File::stored("ページ01.jpg".as_bytes(), b"body")],
        Damage::None,
    );
    let a = open(&zip).unwrap();
    assert_eq!(a.entries()[0].name, "ページ01.jpg");
    assert!(!a.warnings().contains(&Warning::NameNotUtf8));
}

#[test]
fn a_name_flagged_utf8_that_is_not_falls_back_rather_than_failing() {
    // Archivers set bit 11 wrongly. The name still has to become something.
    let mut file = File::stored(b"bad\xFF\xFEname.jpg", b"body");
    file.utf8 = true;
    let zip = archive(&[file], Damage::None);

    let a = open(&zip).unwrap();
    assert!(a.warnings().contains(&Warning::NameNotUtf8));
    assert!(a.entries()[0].name.ends_with("name.jpg"));
    assert_eq!(a.entries().len(), 1);
}

// ---------------------------------------------------------------------------
// Disagreements between the two copies of every entry's metadata
// ---------------------------------------------------------------------------

#[test]
fn a_local_header_disagreeing_with_the_directory_warns_and_believes_the_directory() {
    // Both copies exist and either can be wrong. The directory is the one
    // written last, after the data was actually compressed, so it is the one
    // believed — and the disagreement is said out loud rather than swallowed.
    let mut zip = archive(
        &[File::stored(b"page01.jpg", b"twelve bytes")],
        Damage::None,
    );
    let local = crate::le::find(&zip, 0, b"PK\x03\x04").unwrap();
    zip[local + 14..local + 18].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());

    let mut a = open(&zip).unwrap();
    assert_eq!(&*a.read(0).unwrap(), b"twelve bytes");
    assert!(a.warnings().contains(&Warning::LocalHeaderDisagrees));
}

#[test]
fn reading_an_entry_that_is_not_there_is_refused_rather_than_panicking() {
    let zip = archive(&[File::stored(b"page01.jpg", b"body")], Damage::None);
    let mut a = open(&zip).unwrap();
    assert_eq!(a.read(1), Err(EntryError::NoSuchEntry));
    assert_eq!(a.read(usize::MAX), Err(EntryError::NoSuchEntry));
}

#[test]
fn an_entry_whose_data_runs_past_the_end_is_refused_as_truncated() {
    let zip = archive(&[File::stored(b"page01.jpg", b"body")], Damage::None);
    // Keep the directory intact and cut the file short, which is what a
    // half-finished download looks like.
    let eocd = crate::le::rfind(&zip, b"PK\x05\x06").unwrap();
    let central = crate::le::rfind(&zip, b"PK\x01\x02").unwrap();
    let mut cut = zip[..central].to_vec();
    cut.truncate(cut.len() - 2);
    cut.extend_from_slice(&zip[central..]);
    let _ = eocd;

    // Whatever it decides the entry list is, no read may succeed with bytes it
    // does not have. Refusing to open at all is equally acceptable here; what
    // is not acceptable is handing back four bytes the file no longer holds.
    if let Ok(mut a) = Archive::open(&cut, &Limits::DEFAULT) {
        if let Ok(data) = a.read(0) {
            assert_ne!(&*data, b"body", "returned data past the end of the file");
        }
    }
}

// ---------------------------------------------------------------------------
// Fuzz-shaped inputs, kept as a unit test so they run every build
// ---------------------------------------------------------------------------

#[test]
fn truncating_a_good_archive_anywhere_never_panics() {
    // Nineteen fuzz targets exist and this is not a substitute for the
    // twentieth; it is the cheap version that runs on every `cargo test`, so a
    // panic introduced today is not waiting on a fuzz session to be found.
    let zip = archive(
        &[
            File::stored(b"page01.jpg", b"first"),
            File::deflated(b"page02.jpg", &b"second ".repeat(20)),
        ],
        Damage::None,
    );
    for cut in 0..zip.len() {
        if let Ok(mut a) = Archive::open(&zip[..cut], &Limits::DEFAULT) {
            for i in 0..a.entries().len().min(8) {
                let _ = a.read(i);
            }
        }
    }
}

#[test]
fn flipping_any_single_byte_never_panics() {
    let zip = archive(
        &[File::deflated(b"page01.jpg", &b"content ".repeat(16))],
        Damage::None,
    );
    for i in 0..zip.len() {
        for bit in [0x01u8, 0x80] {
            let mut damaged = zip.clone();
            damaged[i] ^= bit;
            if let Ok(mut a) = Archive::open(&damaged, &Limits::DEFAULT) {
                for e in 0..a.entries().len().min(4) {
                    let _ = a.read(e);
                }
            }
        }
    }
}

/// Writes the five seeds `fuzz/corpus/zip_archive/` carries, so the seeds and
/// the fixtures here cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
///
/// Each seed is the target's **control byte** and then an archive, because the
/// first byte is what picks the bounds. Two of the five carry a control byte of
/// zero — every cap at its lowest — since a corpus in which every seed is
/// roomy explores the happy path and never a refusal, which is gap 18a
/// milestone 8's failure arriving through the corpus rather than through the
/// constant.
#[test]
#[ignore = "writes into fuzz/corpus/zip_archive, which is committed"]
fn write_the_fuzz_seeds() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/zip_archive");
    let body = b"abcabcabcabcabcabcabcabcabcabcabcabc".repeat(4);

    // Both methods and both name encodings in one archive, under the widest
    // bounds the control byte can pick.
    let mixed = archive(
        &[
            File::stored(b"page01.jpg", b"stored entry"),
            File::deflated(b"page02.png", &body),
            File {
                utf8: false,
                ..File::stored(b"cover\x81.jpg", b"a CP437 name")
            },
        ],
        Damage::None,
    );
    // The recovery rung: local headers and no directory to read them from.
    let scanned = archive(&[File::deflated(b"page01.png", &body)], Damage::NoDirectory);
    // A streamed entry, whose sizes are in a descriptor after the data.
    let streamed = archive(
        &[File {
            streamed: true,
            ..File::deflated(b"page01.png", &body)
        }],
        Damage::None,
    );

    for (name, bytes) in [
        ("mixed-methods", [&[0xFFu8][..], &mixed].concat()),
        ("recovered-by-scan", [&[0xFF][..], &scanned].concat()),
        ("streamed-descriptor", [&[0xFF][..], &streamed].concat()),
        // The same two archives against the tightest bounds in the set: one
        // entry, sixteen bytes an entry, no inflation at all, one byte of name.
        ("mixed-methods-tight", [&[0x00][..], &mixed].concat()),
        ("streamed-tight", [&[0x00][..], &streamed].concat()),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory is there");
    }
}
