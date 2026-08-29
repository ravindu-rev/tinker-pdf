//! ICC profiles the reader accepts, for the PDF/A colour and writer tests.
//!
//! `icc::Profile::parse` is a **transform builder**, so a profile it accepts
//! has to carry enough tags to build a transform from — which is exactly the
//! property the colour rules depend on, and exactly what a filler profile
//! would not have. The first version of the colour fixtures wrote three `XYZ`
//! columns and no tone curves; every profile came back unreadable, the
//! destination came back `Unreadable`, and five tests passed by finding
//! nothing for the wrong reason. Building the real thing is what makes them
//! tests.
//!
//! Shared between `pdfa_colour.rs` and `pdfa_writer.rs` rather than written
//! twice: the writer's fixtures are validated by the rules the colour
//! fixtures test, so a profile the two files disagreed about would make the
//! round trip pass or fail for a reason neither file states.

#![allow(dead_code)]

/// A profile with the given data colour space signature and tag set.
pub fn profile(space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let header = 128usize;
    let table = 4 + tags.len() * 12;
    let mut offsets = Vec::with_capacity(tags.len());
    let mut at = header + table;
    for (_, data) in tags {
        offsets.push(at);
        at += data.len();
        // ICC.1 aligns tag data to four bytes. Not enforced by this reader,
        // written anyway, because a fixture that is legal in one respect and
        // not in another tests the reader's leniency rather than the rule.
        at = at.div_ceil(4) * 4;
    }
    let total = at;

    let mut out = vec![0u8; header];
    out[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    // Version 2.4.0, in the byte layout ICC.1 gives the field.
    out[8..12].copy_from_slice(&0x0240_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(b"XYZ ");
    out[36..40].copy_from_slice(b"acsp");

    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for ((signature, data), offset) in tags.iter().zip(&offsets) {
        out.extend_from_slice(signature);
        out.extend_from_slice(&(*offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    for ((_, data), offset) in tags.iter().zip(&offsets) {
        out.resize(*offset, 0);
        out.extend_from_slice(data);
    }
    out.resize(total, 0);
    out
}

/// An `XYZType` tag holding one colourant column.
pub fn xyz_tag(x: u32, y: u32, z: u32) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    for value in [x, y, z] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out
}

/// A `curv` tag with no points, which ICC.1 defines as the identity.
pub fn identity_curve() -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

/// An `mft1` lookup table from `inputs` channels to the three of the
/// connection space, on the smallest grid the format admits.
///
/// This is what a CMYK profile carries and why one cannot be faked with a
/// matrix: the relation between ink and light is not linear, so ICC.1 gives a
/// four-channel profile a sampled table and `icc::Profile::parse` refuses a
/// four-channel profile that has no `A2B*` tag at all.
pub fn lut_tag(inputs: usize) -> Vec<u8> {
    let outputs = 3usize;
    let grid = 2usize;
    let mut out = b"mft1".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(inputs as u8);
    out.push(outputs as u8);
    out.push(grid as u8);
    out.push(0);
    // The 3x3 matrix, which an `A2B*` tag never applies. Written as the
    // identity because the field exists and a reader may look at it.
    for row in 0..3usize {
        for column in 0..3usize {
            let value: u32 = if row == column { 0x0001_0000 } else { 0 };
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    // `mft1`'s input and output tables are fixed at 256 entries of one byte.
    out.extend(std::iter::repeat_n(0u8, inputs * 256));
    out.extend(std::iter::repeat_n(0u8, grid.pow(inputs as u32) * outputs));
    out.extend(std::iter::repeat_n(0u8, outputs * 256));
    out
}

/// An RGB destination profile: three colourant columns and three tone curves,
/// which is what `Profile::parse` calls a matrix/TRC model.
pub fn srgb_like() -> Vec<u8> {
    profile(
        b"RGB ",
        &[
            (*b"rXYZ", xyz_tag(0x0000_6FA2, 0x0000_38F5, 0x0000_0390)),
            (*b"gXYZ", xyz_tag(0x0000_6299, 0x0000_B785, 0x0000_18DA)),
            (*b"bXYZ", xyz_tag(0x0000_24A0, 0x0000_0F84, 0x0000_B6CF)),
            (*b"rTRC", identity_curve()),
            (*b"gTRC", identity_curve()),
            (*b"bTRC", identity_curve()),
        ],
    )
}

/// A CMYK destination profile: a four-channel `A2B1` lookup table.
pub fn cmyk_like() -> Vec<u8> {
    profile(b"CMYK", &[(*b"A2B1", lut_tag(4))])
}

/// A grey destination profile: one tone curve and a white point.
pub fn grey_like() -> Vec<u8> {
    profile(
        b"GRAY",
        &[
            (*b"kTRC", identity_curve()),
            (*b"wtpt", xyz_tag(0x0000_F6D6, 0x0001_0000, 0x0000_D32D)),
        ],
    )
}
