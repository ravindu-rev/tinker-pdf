//! The arrangement, asserted as the table in the module note says it.

use tinker_pdf_filters::{flate_decode, ImagePixels, Limits};

use super::*;

fn inflate(data: &[u8]) -> Vec<u8> {
    flate_decode(data, &Limits::new(1 << 20), None)
        .expect("no predictor to refuse")
        .data
}

#[test]
fn an_indexed_picture_stays_indexed_and_its_transparent_index_is_a_colour_key() {
    let image = RasterImageData::new(
        2,
        1,
        ImagePixels::Indexed {
            palette: vec![1, 2, 3, 4, 5, 6],
            indices: vec![1, 0],
            transparent: Some(1),
        },
        true,
        Vec::new(),
    );
    assert!(image.is_indexed());
    let ImageData::Compressed(c) = image.image() else {
        panic!("compressed");
    };
    assert_eq!(c.bits_per_component, 8);
    assert!(matches!(
        c.color_space,
        ImageColorSpace::Indexed {
            base: DeviceSpace::Rgb,
            lookup: &[1, 2, 3, 4, 5, 6]
        }
    ));
    assert_eq!(c.color_key_mask, Some([(1, 1)].as_slice()));
    assert!(c.soft_mask.is_none());
    assert_eq!(inflate(c.data), [1, 0]);
}

#[test]
fn rgba_is_split_into_colour_and_an_eight_bit_soft_mask() {
    let image = RasterImageData::new(
        2,
        1,
        ImagePixels::Rgba(vec![10, 20, 30, 40, 50, 60, 70, 80]),
        false,
        Vec::new(),
    );
    assert!(!image.is_indexed());
    assert!(!image.complete());
    let ImageData::Compressed(c) = image.image() else {
        panic!("compressed");
    };
    assert_eq!(c.color_space, ImageColorSpace::DeviceRgb);
    assert_eq!(inflate(c.data), [10, 20, 30, 50, 60, 70]);
    let mask = c.soft_mask.expect("a soft mask");
    assert_eq!(
        (mask.width, mask.height, mask.bits_per_component),
        (2, 1, 8)
    );
    assert_eq!(inflate(mask.data), [40, 80]);
    assert!(c.color_key_mask.is_none());
}

#[test]
fn rgb_is_placed_whole() {
    let image = RasterImageData::new(1, 1, ImagePixels::Rgb(vec![7, 8, 9]), true, Vec::new());
    let ImageData::Compressed(c) = image.image() else {
        panic!("compressed");
    };
    assert_eq!(inflate(c.data), [7, 8, 9]);
    assert!(c.soft_mask.is_none() && c.color_key_mask.is_none());
}
