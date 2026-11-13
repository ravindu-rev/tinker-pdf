//! ICC profiles written byte by byte (ICC.1:2010), for the binaries that draw
//! images and fills through a profile that compiles: a matrix/TRC RGB
//! display profile and a version 2 CMYK printer profile of one `mft2` table.
//! Each figure in them is stated beside it, so a test reads what it draws
//! from here and from nothing outside the repository.
//!
//! # Why the allow
//!
//! Each binary compiles its own copy of this module and draws through a
//! different part of it.

#![allow(
    dead_code,
    reason = "shared by the binaries that draw through a profile; each uses a part"
)]

/// A profile of `class` over the data colour space `space`, its PCS XYZ,
/// holding `tags` in their order, each four-byte aligned: the 128-byte
/// header (ICC.1 7.2), the tag table (7.3) and the tags.
pub fn icc_profile(class: &[u8; 4], space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let start = 128 + 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (signature, body) in tags {
        while (start + data.len()) % 4 != 0 {
            data.push(0);
        }
        entries.push((*signature, start + data.len(), body.len()));
        data.extend_from_slice(body);
    }
    while (start + data.len()) % 4 != 0 {
        data.push(0);
    }
    let size = start + data.len();
    let mut profile = vec![0u8; 128];
    profile[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    profile[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    profile[12..16].copy_from_slice(class);
    profile[16..20].copy_from_slice(space);
    profile[20..24].copy_from_slice(b"XYZ ");
    profile[36..40].copy_from_slice(b"acsp");
    profile.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for (signature, offset, len) in entries {
        profile.extend_from_slice(&signature);
        profile.extend_from_slice(&(offset as u32).to_be_bytes());
        profile.extend_from_slice(&(len as u32).to_be_bytes());
    }
    profile.extend_from_slice(&data);
    profile
}

/// An s15Fixed16Number (ICC.1 4.6).
pub fn s15(value: f64) -> [u8; 4] {
    (((value * 65536.0).round() as i32) as u32).to_be_bytes()
}

/// An `XYZ ` tag of one value.
pub fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for value in [x, y, z] {
        tag.extend_from_slice(&s15(value));
    }
    tag
}

/// A `curv` tag of one gamma, as a u8Fixed8Number.
pub fn gamma_tag(gamma: f64) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&1u32.to_be_bytes());
    tag.extend_from_slice(&((gamma * 256.0).round() as u16).to_be_bytes());
    tag
}

/// A matrix/TRC RGB display profile: sRGB's primaries adapted to D50 under
/// a gamma of 1.8 on each channel, so its colours come out other than
/// DeviceRGB's.
pub fn rgb_profile() -> Vec<u8> {
    icc_profile(
        b"mntr",
        b"RGB ",
        &[
            (*b"rXYZ", xyz_tag(0.436_065, 0.222_488, 0.013_916)),
            (*b"gXYZ", xyz_tag(0.385_147, 0.716_873, 0.097_076)),
            (*b"bXYZ", xyz_tag(0.143_066, 0.060_607, 0.714_096)),
            (*b"rTRC", gamma_tag(1.8)),
            (*b"gTRC", gamma_tag(1.8)),
            (*b"bTRC", gamma_tag(1.8)),
            (*b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// A matrix/TRC grey display profile: one gamma of 2.2 over D50's white.
pub fn grey_profile() -> Vec<u8> {
    icc_profile(
        b"mntr",
        b"GRAY",
        &[
            (*b"kTRC", gamma_tag(2.2)),
            (*b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
        ],
    )
}

/// A version 2 printer profile whose one `mft2` table takes four inks to
/// XYZ on a grid of two points an axis, each corner the light of its inks
/// squeezed into a tenth to nine tenths, its blue a little short.
pub fn cmyk_profile() -> Vec<u8> {
    let mut tag = b"mft2\0\0\0\0".to_vec();
    tag.extend_from_slice(&[4, 3, 2, 0]);
    for row in 0..3 {
        for column in 0..3 {
            tag.extend_from_slice(&s15(if row == column { 1.0 } else { 0.0 }));
        }
    }
    tag.extend_from_slice(&2u16.to_be_bytes());
    tag.extend_from_slice(&2u16.to_be_bytes());
    for _ in 0..4 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    let primaries = [
        [0.436_065, 0.222_488, 0.013_916],
        [0.385_147, 0.716_873, 0.097_076],
        [0.143_066, 0.060_607, 0.714_096],
    ];
    for corner in 0..16u32 {
        let ink = |bit: u32| f64::from((corner >> bit) & 1);
        let (c, m, y, k) = (ink(3), ink(2), ink(1), ink(0));
        let light = [
            0.1 + 0.8 * (1.0 - c) * (1.0 - k),
            0.1 + 0.8 * (1.0 - m) * (1.0 - k),
            0.1 + 0.8 * (1.0 - y) * (1.0 - k) * 0.7,
        ];
        for axis in 0..3 {
            let value: f64 = primaries
                .iter()
                .zip(light)
                .map(|(primary, amount)| primary[axis] * amount)
                .sum();
            tag.extend_from_slice(&((value * 32768.0).round() as u16).to_be_bytes());
        }
    }
    for _ in 0..3 {
        tag.extend_from_slice(&0u16.to_be_bytes());
        tag.extend_from_slice(&u16::MAX.to_be_bytes());
    }
    icc_profile(b"prtr", b"CMYK", &[(*b"A2B0", tag)])
}
