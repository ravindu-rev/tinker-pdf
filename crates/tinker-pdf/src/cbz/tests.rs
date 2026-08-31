//! What the page semantics are held to, at the level below a rendered page.
//!
//! The pictures are asserted in `tests/cbz.rs`, where a real render can be
//! compared pixel for pixel against a document written out by hand. What lives
//! here is the half a render cannot see: the order the comparison puts names
//! in, which bytes count as an image, and the two bounds this module owns.
//!
//! Every expected value is written out from APPNOTE, from ISO/IEC 15948 and
//! from gap 29's own design section rather than read back from what the code
//! emitted.

use super::*;

// ---- Natural order -----------------------------------------------------

/// The plan's own worked example, and the one a lexicographic build fails.
///
/// `1.jpg` .. `12.jpg` reads lexicographically as 1, 10, 11, 12, 2, 3 … —
/// every page present, every page rendered correctly, and the comic
/// unreadable.
#[test]
fn a_dozen_unpadded_pages_read_one_to_twelve() {
    let mut names: Vec<String> = (1..=12).map(|n| format!("{n}.jpg")).collect();
    // Shuffled into the order a lexicographic sort would leave them, so a
    // build that never sorts at all fails this too.
    names.sort();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| natural_cmp(a, b));

    let expected: Vec<String> = (1..=12).map(|n| format!("{n}.jpg")).collect();
    assert_eq!(sorted, expected);
    assert_ne!(sorted, names, "and lexicographic is a different answer");
}

/// Zero-padded names sort identically under both orders, so a fixture that has
/// them proves **nothing** about ordering.
///
/// Stated as an assertion rather than left in a comment, the way gap 16's
/// `mixed_mode_rows_are_coded_the_way_their_tag_bit_says` documents its own
/// blind spot: a later reader who assumes the padded fixture covers this can
/// be pointed at the test that says it does not.
#[test]
fn a_padded_fixture_cannot_see_the_difference_and_says_so() {
    let mut padded: Vec<String> = (1..=12).map(|n| format!("page{n:03}.jpg")).collect();
    let mut lexicographic = padded.clone();
    padded.sort_by(|a, b| natural_cmp(a, b));
    lexicographic.sort();
    assert_eq!(
        padded, lexicographic,
        "padding hides the bug this comparison exists for"
    );
}

/// Digit runs compare by magnitude, and a forty-digit one is a legal filename.
///
/// `u64::from_str` is not a total function over one, which is why the rule is
/// "length after leading zeros are trimmed, then the bytes".
#[test]
fn a_digit_run_longer_than_any_integer_still_orders() {
    let small = format!("p{}.jpg", "9".repeat(39));
    let large = format!("p{}.jpg", "1".repeat(40));
    assert_eq!(natural_cmp(&small, &large), Ordering::Less);

    let padded = format!("p{}9.jpg", "0".repeat(60));
    assert_eq!(
        natural_cmp(&padded, "p10.jpg"),
        Ordering::Less,
        "sixty leading zeros are still a nine"
    );
}

/// Zeros trim to a magnitude rather than to nothing that compares wrong.
#[test]
fn zeros_are_a_magnitude_of_their_own() {
    assert_eq!(natural_cmp("p0.jpg", "p1.jpg"), Ordering::Less);
    assert_eq!(natural_cmp("p000.jpg", "p0.jpg"), Ordering::Greater);
    assert_eq!(
        natural_cmp("p0.jpg", "p00.jpg"),
        Ordering::Less,
        "equal by the digit rule, settled by the raw bytes so the order is total"
    );
}

/// The full stored path, so directories group rather than interleaving.
#[test]
fn the_whole_path_is_compared_so_directories_group() {
    let mut names = vec![
        "ch2/p1.jpg".to_string(),
        "ch10/p1.jpg".to_string(),
        "ch2/p10.jpg".to_string(),
        "ch2/p2.jpg".to_string(),
    ];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(
        names,
        ["ch2/p1.jpg", "ch2/p2.jpg", "ch2/p10.jpg", "ch10/p1.jpg"]
    );
}

/// Letters fold, and the fold is a tie-break rather than a loss of order.
#[test]
fn case_folds_and_then_breaks_the_tie_on_the_unfolded_bytes() {
    assert_eq!(natural_cmp("Page2.jpg", "page10.jpg"), Ordering::Less);
    assert_eq!(natural_cmp("A.jpg", "a.jpg"), Ordering::Less);
    assert_eq!(natural_cmp("a.jpg", "A.jpg"), Ordering::Greater);
    assert_eq!(natural_cmp("a.jpg", "a.jpg"), Ordering::Equal);
}

/// A run that runs out sorts first, and the comparison is a total order.
#[test]
fn the_comparison_is_total() {
    let names = [
        "", "a", "a1", "a01", "a2", "a10", "1", "01", "10", "A", "a/b", "a.b", "ﬀ",
    ];
    for x in names {
        assert_eq!(natural_cmp(x, x), Ordering::Equal, "{x} against itself");
        for y in names {
            let forward = natural_cmp(x, y);
            let backward = natural_cmp(y, x);
            assert_eq!(forward, backward.reverse(), "{x} against {y}");
            if x != y {
                assert_ne!(
                    forward,
                    Ordering::Equal,
                    "{x} and {y} are not the same name"
                );
            }
        }
    }
}

/// Two entries may legally carry the same name, so the tie breaks on the
/// central-directory position and the order stays total.
///
/// The `index` values below deliberately **disagree with the slice positions**,
/// and that is the whole design of the fixture. `sort_by` is stable, so a
/// comparison with no tie-break at all still returns equal names in the order
/// the slice happened to hold them — which is index order whenever the entries
/// arrived in index order, as they do from both of the archive reader's routes.
/// An earlier version of this test built them that way and could not see the
/// tie-break removed: the injection matrix reported it as surviving, correctly.
/// Scrambling the two apart is what makes stability and the tie-break disagree,
/// and only then does the assertion mean what its name says.
#[test]
fn equal_names_break_on_directory_position() {
    // Slice order: b.jpg, a.jpg(#7), a.jpg(#2). Directory order for the tie is
    // 2 before 7, which is the reverse of how the slice holds them.
    let entries: Vec<Listing> = [("b.jpg", 9usize), ("a.jpg", 7), ("a.jpg", 2)]
        .iter()
        .map(|(name, index)| Listing {
            name: (*name).to_string(),
            index: *index,
            directory: false,
        })
        .collect();
    // Both `a.jpg` records come first, the one written earlier in the
    // directory ahead of the other, and `b.jpg` last.
    assert_eq!(reading_order(&entries), vec![2, 1, 0]);
}

// ---- Classification ----------------------------------------------------

/// Magic bytes, and the extension is not consulted.
#[test]
fn the_first_bytes_decide_and_the_extension_does_not() {
    assert_eq!(
        image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(
        image_format(b"\x89PNG\r\n\x1a\n"),
        Some(ImageFormat::Png),
        "the eight-byte signature of 5.2, not the first four"
    );
    assert_eq!(image_format(b"GIF89a\x01\x00"), Some(ImageFormat::Gif));
    assert_eq!(image_format(b"GIF87a\x01\x00"), Some(ImageFormat::Gif));
    assert_eq!(
        image_format(b"RIFF\x20\x00\x00\x00WEBPVP8 "),
        Some(ImageFormat::WebP)
    );
    assert_eq!(
        image_format(b"\x00\x00\x00\x20ftypavif\x00\x00\x00\x00"),
        Some(ImageFormat::Avif)
    );
    assert_eq!(image_format(b"II\x2a\x00\x08\x00"), Some(ImageFormat::Tiff));
    assert_eq!(image_format(b"MM\x00\x2a\x00\x00"), Some(ImageFormat::Tiff));
    assert_eq!(
        image_format(&[0xFF, 0x4F, 0xFF, 0x51]),
        Some(ImageFormat::Jpeg2000)
    );
    assert_eq!(
        image_format(b"\x00\x00\x00\x0cjP  \r\n\x87\n"),
        Some(ImageFormat::Jpeg2000)
    );
    let mut bitmap = vec![0u8; 30];
    bitmap[..2].copy_from_slice(b"BM");
    assert_eq!(image_format(&bitmap), Some(ImageFormat::Bmp));

    // Neither of these is an image, whatever it is called.
    assert_eq!(image_format(b"<?xml version=\"1.0\"?>"), None);
    assert_eq!(image_format(b"\x00\x05\x16\x07AppleDouble"), None);
    assert_eq!(image_format(b""), None);
}

/// `RIFF` alone is not WebP, and `BM` alone is not a bitmap.
///
/// Both signatures are short enough to appear in ordinary data, so the second
/// half of each is what makes it a claim rather than a coincidence.
#[test]
fn a_short_signature_needs_its_second_half() {
    assert_eq!(image_format(b"RIFF\x20\x00\x00\x00WAVEfmt "), None);
    assert_eq!(image_format(b"BM"), None, "two bytes and no header");
    assert_eq!(
        image_format(b"\x00\x00\x00\x20ftypmp42\x00\x00\x00\x00"),
        None,
        "an MP4 is an ISO base media file and not an AVIF"
    );
}

/// A signature recognised from a prefix of itself is not recognised.
///
/// Every case here is a **near miss**: the bytes a shortened check would accept
/// and the full one refuses. The test above this one asserts the signatures
/// that work, and the injection matrix showed that is not the same thing --
/// truncating PNG's signature to its first four bytes and JPEG's to its first
/// two both survived a suite that asserted only the positives, because a
/// positive assertion cannot tell a check from a weaker check that agrees with
/// it on every valid file.
///
/// The PNG case is the one with a specification behind it. 15948 5.2 gives the
/// signature as eight bytes and says what the last four are *for*: `\r\n`
/// catches a transfer that translated line endings, `\x1a` stops the file
/// printing on DOS, and `\n` catches the reverse translation. A reader that
/// drops them throws away the corruption detection the signature exists to
/// provide, and then meets the damage further in, where it reads as a broken
/// image rather than as a mangled download.
#[test]
fn a_signature_shortened_to_a_prefix_is_not_a_signature() {
    // PNG: the eight bytes of 5.2, one short and one mangled.
    assert_eq!(
        image_format(b"\x89PNG"),
        None,
        "four bytes is not 5.2's eight"
    );
    assert_eq!(
        image_format(b"\x89PNG\r\n\x1a"),
        None,
        "seven bytes is not eight either"
    );
    assert_eq!(
        image_format(b"\x89PNG\n\n\x1a\n"),
        None,
        "a transfer that ate the carriage return is what 5.2 is watching for"
    );
    assert_eq!(
        image_format(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"),
        Some(ImageFormat::Png),
        "and the real one still passes"
    );

    // JPEG: SOI is two bytes and the marker after it makes three.
    assert_eq!(
        image_format(&[0xFF, 0xD8]),
        None,
        "SOI alone; the next byte must open a marker"
    );
    assert_eq!(
        image_format(&[0xFF, 0xD8, 0x00]),
        None,
        "0xFFD8 followed by something that is not a marker"
    );

    // GIF: the version is part of the signature, and 89b is not a version.
    assert_eq!(image_format(b"GIF"), None);
    assert_eq!(image_format(b"GIF89b\x01\x00"), None);

    // JPEG 2000's two entry points, each one byte short.
    assert_eq!(image_format(&[0xFF, 0x4F, 0xFF]), None);
    assert_eq!(image_format(b"\x00\x00\x00\x0cjP  \r\n\x87"), None);
}

/// The one place a name is consulted, and only for an entry with no bytes.
#[test]
fn an_extension_answers_only_the_narrow_question() {
    assert!(extension_claims_image("pages/p01.JPG"));
    assert!(extension_claims_image("p01.jpeg"));
    assert!(extension_claims_image("p01.png"));
    assert!(!extension_claims_image("ComicInfo.xml"));
    assert!(!extension_claims_image("README"));
    assert!(!extension_claims_image("Thumbs.db"));
    assert!(!extension_claims_image(".DS_Store"));
}

/// `.DS_Store` has no name before its dot, and `rsplit_once` would call
/// `DS_Store` an extension if the check were the other way round.
#[test]
fn a_leading_dot_is_not_an_extension() {
    assert!(!extension_claims_image(".png"));
}

// ---- The sniff ---------------------------------------------------------

/// Offset zero, and nowhere else.
#[test]
fn the_signature_is_read_at_a_fixed_position() {
    assert_eq!(container(b"PK\x03\x04rest"), Some(Container::Zip));
    assert_eq!(container(b"Rar!\x1a\x07\x00"), Some(Container::Rar));
    assert_eq!(container(b"7z\xbc\xaf\x27\x1c"), Some(Container::SevenZip));

    // One byte of anything in front and it is not a container.
    assert_eq!(container(b"\x00PK\x03\x04rest"), None);
    assert_eq!(container(b"%PDF-1.7\nPK\x03\x04"), None);
    assert_eq!(container(b""), None);
    assert_eq!(container(b"PK\x03"), None, "three of the four bytes");
    assert_eq!(container(b"PK\x01\x02"), None, "a central directory record");
}

/// tar's magic is a field of the first header block, at the offset POSIX
/// 1003.1 puts it and not one found by searching.
#[test]
fn tars_magic_is_a_field_and_not_a_search() {
    let mut block = vec![0u8; 512];
    block[257..262].copy_from_slice(b"ustar");
    assert_eq!(container(&block), Some(Container::Tar));

    let mut shifted = vec![0u8; 512];
    shifted[256..261].copy_from_slice(b"ustar");
    assert_eq!(container(&shifted), None, "one byte over is not a tar");
}

// ---- Bounds ------------------------------------------------------------

/// A cap at or above the archive reader's own entry cap could never fire,
/// which is gap 18a milestone 8's failure reached from the other direction.
///
/// Both relations are between constants, so they are checked in `const`
/// blocks: a build that broke either one would not compile, which is a
/// stronger place to catch it than a test run. The test remains so the
/// relation has a name in the suite and so the reason travels with it.
#[test]
fn the_page_cap_sits_below_the_cap_that_would_otherwise_stop_first() {
    const {
        assert!(
            MAX_CBZ_PAGES < zip_limits::MAX_ZIP_ENTRIES,
            "the page cap is at or above the entry cap, so it could never fire"
        );
        // And above what any comic is, so it refuses nothing real.
        assert!(
            MAX_CBZ_PAGES > 200 * 4,
            "the page cap would refuse an ordinary 200-page comic"
        );
    }
}

/// The synthesis cap has room for the yardstick archive the ledger names.
#[test]
fn the_synthesis_cap_has_room_for_a_two_hundred_page_comic() {
    let comic = 200 * (1_500_000 + PAGE_OVERHEAD) + DOCUMENT_OVERHEAD;
    assert!(
        comic < MAX_SYNTHESISED_PDF,
        "a 200-page comic charges {comic} against {MAX_SYNTHESISED_PDF}"
    );
    // And not so much room that it is decoration: the margin is under two.
    assert!(MAX_SYNTHESISED_PDF < comic * 2);
}

// ---- ComicInfo.xml -----------------------------------------------------

/// The six elements this build maps, and the `/Info` keys they become.
///
/// Written out from the table in [`comic_info`]'s own header rather than read
/// back from what the code emitted: the mapping is a decision, and a test that
/// records the decision is the only thing that would notice it changing.
#[test]
fn the_six_mapped_elements_become_four_info_keys() {
    let xml = br#"<?xml version="1.0"?>
        <ComicInfo>
          <Title>The Long Hallway</Title>
          <Series>Nightwatch</Series>
          <Number>12</Number>
          <Writer>A. Writer</Writer>
          <Penciller>B. Penciller</Penciller>
          <Summary>Somebody walks down a hallway.</Summary>
          <Publisher>Not mapped, deliberately</Publisher>
        </ComicInfo>"#;
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(info.title(), Some("The Long Hallway"));
    assert_eq!(info.series(), Some("Nightwatch"));
    assert_eq!(info.number(), Some("12"));
    assert_eq!(info.writer(), Some("A. Writer"));
    assert_eq!(info.penciller(), Some("B. Penciller"));
    assert_eq!(info.summary(), Some("Somebody walks down a hallway."));

    let entries: Vec<(&str, String)> = info
        .info_entries()
        .into_iter()
        .map(|(key, value)| (core::str::from_utf8(key).expect("ASCII"), value))
        .collect();
    assert_eq!(
        entries,
        vec![
            ("Title", "The Long Hallway".to_owned()),
            ("Author", "A. Writer, B. Penciller".to_owned()),
            ("Subject", "Somebody walks down a hallway.".to_owned()),
            ("Keywords", "Nightwatch #12".to_owned()),
        ],
        "the mapping in comic_info's header, as the builder is handed it"
    );
}

/// **The branch most real issues take.** No `<Title>`, so the series and the
/// number are the name of the book.
///
/// A viewer with an empty title bar falls back to the filename, which is the
/// one thing a synthesised document has no good answer for.
#[test]
fn an_issue_with_no_title_is_named_by_its_series_and_number() {
    let xml = b"<ComicInfo><Series>Nightwatch</Series><Number>12</Number></ComicInfo>";
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(info.title(), None);
    let entries = info.info_entries();
    assert_eq!(entries.first().map(|(k, _)| *k), Some(b"Title".as_slice()));
    assert_eq!(
        entries.first().map(|(_, v)| v.as_str()),
        Some("Nightwatch #12")
    );
    // And the same string still reaches `/Keywords`, so the two branches
    // differ in what `/Title` says and in nothing else.
    assert!(entries
        .iter()
        .any(|(k, v)| *k == b"Keywords" && v == "Nightwatch #12"));
}

/// A number with no series names nothing, so it names nothing.
///
/// `#7` in a title bar is worse than the filename it replaced, which is the
/// only reason this case is written down at all.
#[test]
fn a_number_with_no_series_titles_nothing() {
    let xml = b"<ComicInfo><Number>7</Number></ComicInfo>";
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(info.number(), Some("7"));
    assert!(info.info_entries().is_empty());
}

/// One person credited twice is one name, not two.
#[test]
fn a_writer_who_is_also_the_penciller_is_named_once() {
    let xml = b"<ComicInfo><Writer>C. Both</Writer><Penciller>C. Both</Penciller></ComicInfo>";
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(
        info.info_entries(),
        vec![(b"Author".as_slice(), "C. Both".to_owned())]
    );
}

/// Text arrives in as many pieces as the reader chooses, and a reference is
/// its own piece.
#[test]
fn an_entity_reference_inside_a_field_is_one_value() {
    let xml = b"<ComicInfo><Title>Tom &amp; Jerry</Title></ComicInfo>";
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(info.title(), Some("Tom & Jerry"));
}

/// The schema allows one of each, and a file with two has not said which.
/// Document order, first — the rule `epub::package` already takes for
/// `dc:title`.
#[test]
fn the_first_of_two_titles_wins() {
    let xml = b"<ComicInfo><Title>First</Title><Title>Second</Title></ComicInfo>";
    let info = comic_info::parse(xml, &tinker_pdf_xml::Limits::DEFAULT).expect("a ComicInfo");
    assert_eq!(info.title(), Some("First"));
}

/// Every way it does not become metadata, each by its own name.
///
/// Collapsing these would tell a host that an archive carrying a title it
/// could not read is the same as one carrying no title, and those are the two
/// states ruling 10 exists to keep apart.
#[test]
fn each_way_a_comic_info_fails_is_its_own_defect() {
    let limits = tinker_pdf_xml::Limits::DEFAULT;
    assert_eq!(
        comic_info::parse(b"<ComicInfo><Title>unclosed", &limits),
        Err(ComicInfoDefect::Unreadable)
    );
    assert_eq!(
        comic_info::parse(b"<comic-info><Title>x</Title></comic-info>", &limits),
        Err(ComicInfoDefect::NotComicInfo),
        "XML names are case sensitive and this is not the root the schema names"
    );
    // A document type declaration, refused before one byte past it is read.
    assert_eq!(
        comic_info::parse(b"<!DOCTYPE ComicInfo><ComicInfo/>", &limits),
        Err(ComicInfoDefect::Unreadable)
    );
    let big = vec![b' '; MAX_COMIC_INFO_BYTES + 1];
    assert_eq!(
        comic_info::parse(&big, &limits),
        Err(ComicInfoDefect::TooLarge),
        "the entry cap is decided before the parser sees a byte"
    );
}

/// **A `ComicInfo.xml` that names nothing is not a defect**, and this is the
/// distinction that decides whether ruling 10's warning fires.
///
/// Nothing was tolerated, repaired or lost — the file was read exactly as
/// written and it said nothing this build maps. Warning about it would be the
/// failure gap 29 named when it decided metadata entries are not warnings.
/// What the empty *value* buys is that a caller can still tell an empty
/// `ComicInfo.xml` from an archive that carries none.
#[test]
fn a_comic_info_that_names_nothing_is_read_and_is_not_a_defect() {
    let limits = tinker_pdf_xml::Limits::DEFAULT;
    let empty = comic_info::parse(b"<ComicInfo/>", &limits).expect("read, and empty");
    assert!(empty.is_empty());
    assert!(empty.info_entries().is_empty());
    let unmapped = comic_info::parse(b"<ComicInfo><Publisher>x</Publisher></ComicInfo>", &limits)
        .expect("forty-odd elements this build does not map are the same answer as none");
    assert!(unmapped.is_empty());
}

/// The entry cap fires one byte past itself and not at itself.
#[test]
fn the_comic_info_cap_admits_exactly_its_own_size() {
    let mut xml = b"<ComicInfo><Title>x</Title></ComicInfo>".to_vec();
    xml.resize(MAX_COMIC_INFO_BYTES, b' ');
    assert!(comic_info::parse(&xml, &tinker_pdf_xml::Limits::DEFAULT).is_ok());
    xml.push(b' ');
    assert_eq!(
        comic_info::parse(&xml, &tinker_pdf_xml::Limits::DEFAULT),
        Err(ComicInfoDefect::TooLarge)
    );
}

/// The name that is read, and the names that are not.
///
/// Narrow on purpose: a nested copy describes something that is not this
/// document, and picking one of several by directory order would make the
/// document's title depend on what a packing tool happened to walk first.
#[test]
fn only_the_archive_root_names_the_metadata() {
    assert!(comic_info::is_comic_info("ComicInfo.xml"));
    assert!(comic_info::is_comic_info("comicinfo.xml"));
    assert!(comic_info::is_comic_info("COMICINFO.XML"));
    assert!(!comic_info::is_comic_info("chapter1/ComicInfo.xml"));
    assert!(!comic_info::is_comic_info("./ComicInfo.xml"));
    assert!(!comic_info::is_comic_info("ComicInfo.xml.bak"));
    assert!(!comic_info::is_comic_info("ComicInfo"));
}

/// **And it is still not a page**, which is the distinction this milestone had
/// to add a reader without blurring.
///
/// Three separate claims, and the middle one is the one that would have gone
/// quietly: the name answers the metadata question, the *extension* still
/// answers `false` to the image question, and the bytes are not an image
/// either.
#[test]
fn the_metadata_entry_is_read_and_is_still_not_a_page() {
    assert!(comic_info::is_comic_info("ComicInfo.xml"));
    assert!(!extension_claims_image("ComicInfo.xml"));
    assert_eq!(image_format(b"<?xml version=\"1.0\"?><ComicInfo/>"), None);
}
