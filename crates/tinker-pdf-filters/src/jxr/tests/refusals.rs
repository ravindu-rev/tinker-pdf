//! Every variant of [`JxrRefusal`] reached by a decode.
//!
//! `docs/features/filters.md` publishes the refusal list, and a published
//! list nothing reaches is a claim rather than a check. This file exists so
//! that a refusal which stops firing — because a later milestone made its
//! condition unreachable without removing the variant — fails a test instead
//! of quietly becoming decoration.

use super::writer::{wrap, Codestream};
use crate::jxr::{jxr_decode, JxrError, JxrRefusal};
use crate::Limits;

fn limits() -> Limits {
    Limits::new(1 << 24)
}

fn refusal_of(c: Codestream, guid_tail: u8) -> JxrRefusal {
    let file = wrap(&c.build(), guid_tail);
    match jxr_decode(&file, &limits()) {
        Err(JxrError::Unsupported(r)) => r,
        other => panic!("expected a named refusal, got {other:?}"),
    }
}

#[test]
fn frequency_mode_is_refused_by_name() {
    assert_eq!(
        refusal_of(
            Codestream {
                frequency_mode: true,
                index_table: true,
                ..Codestream::default()
            },
            0x0D
        ),
        JxrRefusal::FrequencyMode
    );
}

#[test]
fn an_interleaved_alpha_image_plane_is_refused_by_name() {
    assert_eq!(
        refusal_of(
            Codestream {
                alpha_plane: true,
                ..Codestream::default()
            },
            0x0F
        ),
        JxrRefusal::InterleavedAlphaPlane
    );
}

#[test]
fn a_separate_alpha_image_plane_is_refused_by_name() {
    // A.3.2: ALPHA_OFFSET points at a second CODED_IMAGE( ). Dropping it
    // would return an opaque image where the file said transparent, so it is
    // refused rather than ignored.
    let codestream = Codestream::default().build();
    let mut file = wrap(&codestream, 0x0F);
    // Rewrite IMAGE_BYTE_COUNT's entry (the fifth, tag 0xBCC1) into
    // ALPHA_OFFSET (0xBCC2) is not possible without breaking the required
    // set, so append a sixth entry instead by rebuilding the directory.
    let entries = u16::from_le_bytes([file[8], file[9]]);
    file[8..10].copy_from_slice(&(entries + 1).to_le_bytes());
    // The new entry replaces the ZERO_OR_NEXT_IFD_OFFSET slot, and a fresh
    // zero terminator is appended after it. 0xBCC2 sorts after 0xBCC1, so
    // A.7.2's ordering still holds.
    let dir_end = 10 + 12 * usize::from(entries);
    let mut entry = Vec::new();
    entry.extend_from_slice(&0xBCC2u16.to_le_bytes());
    entry.extend_from_slice(&4u16.to_le_bytes());
    entry.extend_from_slice(&1u32.to_le_bytes());
    entry.extend_from_slice(&0u32.to_le_bytes());
    let mut rebuilt = file[..dir_end].to_vec();
    rebuilt.extend_from_slice(&entry);
    rebuilt.extend_from_slice(&0u32.to_le_bytes());
    rebuilt.extend_from_slice(&file[dir_end + 4..]);
    // Everything after the directory moved by twelve bytes, so the two
    // offsets that point past it move with it.
    let fix = |v: &mut Vec<u8>, at: usize| {
        let old = u32::from_le_bytes([v[at], v[at + 1], v[at + 2], v[at + 3]]);
        v[at..at + 4].copy_from_slice(&(old + 12).to_le_bytes());
    };
    fix(&mut rebuilt, 10 + 8); // PIXEL_FORMAT's offset
    fix(&mut rebuilt, 10 + 12 * 3 + 8); // IMAGE_OFFSET
                                        // ALPHA_BYTE_COUNT is absent, so the range is not formed and the refusal
                                        // needs both. Add it by lengthening the count once more.
    let entries2 = u16::from_le_bytes([rebuilt[8], rebuilt[9]]);
    rebuilt[8..10].copy_from_slice(&(entries2 + 1).to_le_bytes());
    let dir_end2 = 10 + 12 * usize::from(entries2);
    let mut entry2 = Vec::new();
    entry2.extend_from_slice(&0xBCC3u16.to_le_bytes());
    entry2.extend_from_slice(&4u16.to_le_bytes());
    entry2.extend_from_slice(&1u32.to_le_bytes());
    entry2.extend_from_slice(&1u32.to_le_bytes());
    let mut final_file = rebuilt[..dir_end2].to_vec();
    final_file.extend_from_slice(&entry2);
    final_file.extend_from_slice(&0u32.to_le_bytes());
    final_file.extend_from_slice(&rebuilt[dir_end2 + 4..]);
    fix(&mut final_file, 10 + 8);
    fix(&mut final_file, 10 + 12 * 3 + 8);

    assert_eq!(
        jxr_decode(&final_file, &limits()),
        Err(JxrError::Unsupported(JxrRefusal::SeparateAlphaPlane))
    );
}

#[test]
fn subsampled_internal_colour_formats_are_refused_by_name() {
    for internal in [1u64, 2] {
        assert_eq!(
            refusal_of(
                Codestream {
                    internal_clr_fmt: internal,
                    width: 32,
                    height: 32,
                    ..Codestream::default()
                },
                0x0D
            ),
            JxrRefusal::SubsampledInternalFormat,
            "INTERNAL_CLR_FMT {internal}"
        );
    }
    // And Table 28's YUVK, a four-component internal layout.
    assert_eq!(
        refusal_of(
            Codestream {
                internal_clr_fmt: 4,
                ..Codestream::default()
            },
            0x0D
        ),
        JxrRefusal::SubsampledInternalFormat
    );
}

#[test]
fn n_component_internal_format_is_refused_by_name() {
    assert_eq!(
        refusal_of(
            Codestream {
                internal_clr_fmt: 6,
                ..Codestream::default()
            },
            0x0D
        ),
        JxrRefusal::UnsupportedColourFormat
    );
}

#[test]
fn cmyk_and_rgbe_output_colour_formats_are_refused_by_name() {
    // Table 22 rows 4, 5, 6 and 8.
    for clr in [4u64, 5, 6, 8] {
        assert_eq!(
            refusal_of(
                Codestream {
                    output_clr_fmt: clr,
                    ..Codestream::default()
                },
                0x0D
            ),
            JxrRefusal::UnsupportedColourFormat,
            "OUTPUT_CLR_FMT {clr}"
        );
    }
}

#[test]
fn float_and_fixed_point_output_depths_are_refused_by_name() {
    // Table 23 rows 3, 4, 6 and 7: BD16S, BD16F, BD32S, BD32F.
    for depth in [3u64, 4, 6, 7] {
        assert_eq!(
            refusal_of(
                Codestream {
                    output_bitdepth: depth,
                    ..Codestream::default()
                },
                0x0D
            ),
            JxrRefusal::FloatOrFixedPointFormat,
            "OUTPUT_BITDEPTH {depth}"
        );
    }
}

#[test]
fn packed_output_depths_are_refused_by_name() {
    // Table 23 rows 0, 8, 9, 10 and 15: the sub-byte and cross-byte packings
    // of 9.10.8.3 to 9.10.8.6.
    for depth in [0u64, 8, 9, 10, 15] {
        assert_eq!(
            refusal_of(
                Codestream {
                    output_bitdepth: depth,
                    ..Codestream::default()
                },
                0x0D
            ),
            JxrRefusal::PackedOutputBitdepth,
            "OUTPUT_BITDEPTH {depth}"
        );
    }
}

#[test]
fn a_windowed_origin_is_refused_by_name() {
    // 6.3: a top or left margin shifts the whole sample grid. The bottom and
    // right margins are ordinary padding and cost nothing, which is why the
    // refusal names the origin rather than windowing.
    assert_eq!(
        refusal_of(
            Codestream {
                windowing: true,
                top_margin: 2,
                ..Codestream::default()
            },
            0x0D
        ),
        JxrRefusal::WindowedOrigin
    );
    assert_eq!(
        refusal_of(
            Codestream {
                windowing: true,
                left_margin: 2,
                ..Codestream::default()
            },
            0x0D
        ),
        JxrRefusal::WindowedOrigin
    );
}

#[test]
fn a_reserved_overlap_mode_refuses_rather_than_choosing_one() {
    // 8.3.10 reserves the value 3. Overlap filtering changes every sample
    // near a block edge, so guessing at an unknown mode returns an image
    // that is wrong exactly where a reader would not look.
    let file = wrap(
        &Codestream {
            overlap_mode: 3,
            ..Codestream::default()
        }
        .build(),
        0x0D,
    );
    assert_eq!(
        jxr_decode(&file, &limits()),
        Err(JxrError::ReservedValue("OVERLAP_MODE"))
    );
}

#[test]
fn output_formatting_is_refused_by_name_until_it_lands() {
    // The milestone that lands 9.10 deletes this test along with the variant.
    // Until then a build that has reconstructed every sample and cannot say
    // what the numbers mean says so, rather than returning them as a picture
    // — which they would look like, being a real image in the internal
    // colour format, and a Y/U/V raster presented as R/G/B is a plausible
    // wrong photograph.
    assert_eq!(
        refusal_of(Codestream::default(), 0x0D),
        JxrRefusal::OutputFormatting
    );
}
