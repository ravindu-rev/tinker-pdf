//! Annex A's file format and clause 8's header layers.

use super::writer::{wrap, Codestream};
use crate::jxr::container::{self, JxrChannels};
use crate::jxr::{jxr_decode, JxrError, JxrRefusal};
use crate::Limits;

fn limits() -> Limits {
    Limits::new(1 << 24)
}

/// The decode this build cannot finish, which every header test reaches when
/// the headers were read successfully. The milestone that lands 9.10's output
/// formatting replaces it with a raster.
const HEADERS_OK: JxrError = JxrError::Unsupported(JxrRefusal::OutputFormatting);

#[test]
fn the_smallest_legal_file_parses_its_headers() {
    let file = wrap(&Codestream::default().build(), 0x0D);
    assert_eq!(jxr_decode(&file, &limits()), Err(HEADERS_OK));
}

#[test]
fn a_bare_codestream_is_accepted_without_a_container() {
    // 9.1.5.1's XPS part is a file; a system that carries the codestream on
    // its own is the other case, and a caller holding bytes should not have
    // to decide which it has.
    let codestream = Codestream::default().build();
    assert_eq!(jxr_decode(&codestream, &limits()), Err(HEADERS_OK));
}

#[test]
fn something_that_is_not_jpeg_xr_says_so() {
    assert_eq!(jxr_decode(b"", &limits()), Err(JxrError::NotJxr));
    assert_eq!(
        jxr_decode(b"not an image", &limits()),
        Err(JxrError::NotJxr)
    );
    // A TIFF, which A.5.3 distinguishes by the byte where TIFF puts 42.
    assert_eq!(
        jxr_decode(b"II\x2A\x00\x08\x00\x00\x00", &limits()),
        Err(JxrError::NotJxr)
    );
    // `MM` is TIFF's other byte order and is **not** Annex A's: A.5.2 fixes
    // the marker at `II`. A reader that accepted it would be accepting a file
    // the standard does not define.
    assert_eq!(
        jxr_decode(b"MM\xBC\x01\x00\x00\x00\x08", &limits()),
        Err(JxrError::NotJxr)
    );
}

#[test]
fn a_file_version_this_build_does_not_know_refuses() {
    let mut file = wrap(&Codestream::default().build(), 0x0D);
    file[3] = 2;
    assert_eq!(
        jxr_decode(&file, &limits()),
        Err(JxrError::UnsupportedFileVersion(2))
    );
}

#[test]
fn a_missing_required_tag_names_the_tag() {
    let codestream = Codestream::default().build();
    let mut file = wrap(&codestream, 0x0D);
    // Rewrite 0xBC80 (IMAGE_WIDTH) as an unrecognised tag. A.7.2 wants tags
    // ascending, and 0xBC7F still is, so this removes the tag without also
    // tripping the ordering warning.
    file[10 + 12] = 0x7F;
    file[10 + 12 + 1] = 0xBC;
    assert_eq!(
        jxr_decode(&file, &limits()),
        Err(JxrError::MissingRequiredTag(0xBC80))
    );
}

#[test]
fn table_a6_guids_map_to_their_rows_and_nothing_else_does() {
    let mut guid = [0u8; 16];
    guid[..15].copy_from_slice(&[
        0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
    ]);

    guid[15] = 0x0D;
    let rgb = container::pixel_format_from_guid(&guid).expect("Table A.6 row 24bppRGB");
    assert_eq!(rgb.mnemonic, "24bppRGB");
    assert_eq!(rgb.channels, JxrChannels::Rgb);
    assert_eq!(rgb.bits_per_component, 8);

    // 0x0E and 0x0F differ by one bit and by a whole channel — the pair a
    // predicate over the GUID's low bits gets wrong, which is why Table A.6
    // is transcribed as a table.
    guid[15] = 0x0E;
    let bgr = container::pixel_format_from_guid(&guid).expect("Table A.6 row 32bppBGR");
    assert_eq!(bgr.channels.count(), 3);
    guid[15] = 0x0F;
    let bgra = container::pixel_format_from_guid(&guid).expect("Table A.6 row 32bppBGRA");
    assert_eq!(bgra.channels.count(), 4);

    // A Table A.6 GUID this build has no row for: known, unsupported.
    guid[15] = 0x1C; // 32bppCMYK
    assert!(container::pixel_format_from_guid(&guid).is_none());
    assert!(container::is_table_a6_guid(&guid));

    // Sixteen bytes that are not one of Table A.6's GUIDs at all.
    let alien = [0u8; 16];
    assert!(!container::is_table_a6_guid(&alien));
}

#[test]
fn a_pixel_format_this_build_has_no_row_for_is_a_different_sentence() {
    // 32bppCMYK: a real Table A.6 row, refused by name.
    let file = wrap(&Codestream::default().build(), 0x1C);
    assert_eq!(
        jxr_decode(&file, &limits()),
        Err(JxrError::Unsupported(JxrRefusal::UnknownPixelFormat))
    );
}

#[test]
fn the_codestream_signature_is_checked() {
    let mut codestream = Codestream::default().build();
    codestream[0] = b'X';
    let file = wrap(&codestream, 0x0D);
    assert_eq!(jxr_decode(&file, &limits()), Err(JxrError::NotJxr));
}

#[test]
fn a_reserved_codestream_version_refuses() {
    // 8.3.3: RESERVED_B shall be 1, and the clause reserves other values as
    // the signal of a stream not compatible with prior decoders.
    let file = wrap(
        &Codestream {
            reserved_b: 2,
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(
        jxr_decode(&file, &limits()),
        Err(JxrError::UnsupportedCodestreamVersion(2))
    );
}

#[test]
fn dimensions_that_do_not_fill_whole_macroblocks_are_padded_not_refused() {
    // 8.3.30 infers the right margin so that the extended width is a multiple
    // of 16. A 40 x 24 image is 3 x 2 macroblocks with margins of 8.
    let file = wrap(
        &Codestream {
            width: 40,
            height: 24,
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(jxr_decode(&file, &limits()), Err(HEADERS_OK));
}

#[test]
fn tiles_that_overrun_the_macroblock_grid_refuse() {
    // 8.3.25 derives the last tile's width by subtraction. A 32-pixel-wide
    // image is two macroblocks across, so a declared first tile of three
    // leaves nothing — and an unchecked subtraction wraps.
    let file = wrap(
        &Codestream {
            width: 32,
            height: 32,
            tiling: true,
            index_table: true,
            num_ver_tiles_minus1: 1,
            tile_widths: vec![3],
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(jxr_decode(&file, &limits()), Err(JxrError::BadTiling));
}

#[test]
fn a_legal_tile_grid_parses() {
    let file = wrap(
        &Codestream {
            width: 64,
            height: 64,
            tiling: true,
            index_table: true,
            num_ver_tiles_minus1: 1,
            num_hor_tiles_minus1: 1,
            tile_widths: vec![2],
            tile_heights: vec![2],
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(jxr_decode(&file, &limits()), Err(HEADERS_OK));
}

#[test]
fn more_than_one_tile_with_no_index_table_refuses() {
    // 8.5.3 gives the single-packet case an implicit offset of zero, and
    // there is no other way to locate a second packet.
    let file = wrap(
        &Codestream {
            width: 64,
            height: 64,
            tiling: true,
            index_table: false,
            num_ver_tiles_minus1: 1,
            tile_widths: vec![2],
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(jxr_decode(&file, &limits()), Err(JxrError::BadIndexTable));
}

#[test]
fn an_index_table_start_code_that_is_not_0x0001_refuses() {
    let file = wrap(
        &Codestream {
            index_table: true,
            index_start_code: 0x0002,
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(jxr_decode(&file, &limits()), Err(JxrError::BadIndexTable));
}

#[test]
fn truncation_anywhere_in_the_headers_refuses_rather_than_panicking() {
    let file = wrap(&Codestream::default().build(), 0x0D);
    for cut in 0..file.len() {
        // The only contract is that nothing panics; which error comes back
        // depends on which structure the cut landed in.
        let _ = jxr_decode(&file[..cut], &limits());
    }
}

#[test]
fn a_caller_ceiling_below_the_raster_refuses_with_the_callers_own_number() {
    let file = wrap(
        &Codestream {
            width: 64,
            height: 64,
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    // 64 x 64 x 3 channels = 12 288 bytes.
    match jxr_decode(&file, &Limits::new(1024)) {
        Err(JxrError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(bytes, 12_288);
            assert_eq!(limit, 1024);
        }
        other => panic!("expected the caller's own ceiling back, got {other:?}"),
    }
}
