//! Every fixture here is written by the test from the format descriptions
//! `netpbm.rs` names — a header as the description lays it out, then a
//! raster — and decoded back to the samples it was written from. Nothing is
//! read from disk and no other program made any of it.

use super::*;

const ROOM: Limits = Limits::new(1 << 24);

fn decode(file: &[u8]) -> NetpbmImage {
    netpbm_decode(file, &ROOM).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(file: &[u8]) -> NetpbmError {
    match netpbm_decode(file, &ROOM) {
        Err(e) => e,
        Ok(image) => panic!("decoded as {:?} {:?}", image.format, image.colour),
    }
}

/// A file: its header, then its raster.
fn file(header: &str, raster: &[u8]) -> Vec<u8> {
    let mut out = header.as_bytes().to_vec();
    out.extend_from_slice(raster);
    out
}

/// `n` samples on `0..=maxval`, both ends among them.
fn ramp(n: usize, maxval: u32) -> Vec<u32> {
    let mut values: Vec<u32> = (0..n)
        .map(|i| ((i as u64 * 7_919 + 13) % (u64::from(maxval) + 1)) as u32)
        .collect();
    if let Some(first) = values.first_mut() {
        *first = 0;
    }
    if let Some(last) = values.last_mut() {
        *last = maxval;
    }
    values
}

/// Samples as a raw raster holds them: "If the Maxval is less than 256, it is
/// 1 byte. Otherwise, it is 2 bytes. The most significant byte is first."
fn raw(values: &[u32], maxval: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in values {
        if maxval < 256 {
            out.push(v as u8);
        } else {
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
    }
    out
}

/// Samples as a plain raster holds them: ASCII decimal, white space before
/// and after each, no line longer than 70 characters.
fn plain(values: &[u32]) -> Vec<u8> {
    let mut out = String::new();
    for (i, v) in values.iter().enumerate() {
        out.push_str(&v.to_string());
        out.push(if i % 10 == 9 { '\n' } else { ' ' });
    }
    out.into_bytes()
}

/// What a sample of `maxval` reads as: the nearest of 256 steps under a
/// maxval below 256, of 65 536 above it, big-endian.
fn expected(values: &[u32], maxval: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for &v in values {
        let (v, m) = (u64::from(v), u64::from(maxval));
        if maxval < 256 {
            out.push(((v * 255 + m / 2) / m) as u8);
        } else {
            out.extend_from_slice(&(((v * 65_535 + m / 2) / m) as u16).to_be_bytes());
        }
    }
    out
}

// ---- PBM ------------------------------------------------------------------------

/// A picture of `width x height` pixels, `true` black, from a fixed sequence.
fn bits(width: usize, height: usize) -> Vec<bool> {
    (0..width * height)
        .map(|i| (i * 7 + i / 5) % 3 == 0)
        .collect()
}

/// PBM's raw rows: "Width bits, packed 8 to a byte, with don't care bits to
/// fill out the last byte in the row", most significant bit first. The
/// don't-care bits are set, so a reader that read them would draw black.
fn packed(bits: &[bool], width: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for row in bits.chunks(width) {
        let mut bytes = vec![0xFFu8; width.div_ceil(8)];
        for (x, &black) in row.iter().enumerate() {
            if !black {
                bytes[x / 8] &= !(0x80 >> (x % 8));
            }
        }
        out.extend_from_slice(&bytes);
    }
    out
}

/// PBM's `1` is black and comes back as grey zero.
fn grey_of(bits: &[bool]) -> Vec<u8> {
    bits.iter()
        .map(|&black| if black { 0 } else { 255 })
        .collect()
}

#[test]
fn plain_and_raw_pbm_decode_to_their_bits() {
    let (width, height) = (13, 5);
    let picture = bits(width, height);

    let raw = decode(&file("P4\n# a comment\n13 5\n", &packed(&picture, width)));
    assert_eq!((raw.width, raw.height), (13, 5));
    assert_eq!(raw.format, NetpbmFormat::Pbm);
    assert_eq!(raw.colour, NetpbmColour::Grey);
    assert_eq!(raw.bits_per_component, 8);
    assert_eq!(raw.maxval, Some(1));
    assert_eq!(raw.scale, None);
    assert_eq!(raw.data, grey_of(&picture));
    assert!(raw.complete);
    assert!(raw.warnings.is_empty(), "{:?}", raw.warnings);

    // The plain raster: one `0` or `1` a pixel, white space ignored, so some
    // rows are spaced and some run their pixels together.
    let mut text = String::from("P1\n13 5\n");
    for (y, row) in picture.chunks(width).enumerate() {
        for &black in row {
            text.push(if black { '1' } else { '0' });
            if y % 2 == 0 {
                text.push(' ');
            }
        }
        text.push('\n');
    }
    let plain = decode(text.as_bytes());
    assert_eq!(plain.format, NetpbmFormat::PlainPbm);
    assert_eq!(plain.data, raw.data);
    assert!(plain.complete);
    assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
}

// ---- PGM and PPM ----------------------------------------------------------------

#[test]
fn pgm_samples_scale_from_their_maxval_at_both_widths() {
    let (width, height) = (7, 3);
    for maxval in [1u32, 15, 100, 255, 256, 1_000, 4_095, 65_535] {
        let values = ramp(width * height, maxval);
        let want = expected(&values, maxval);
        let header = format!("P5\n{width} {height}\n{maxval}\n");
        let raw_image = decode(&file(&header, &raw(&values, maxval)));
        assert_eq!(raw_image.format, NetpbmFormat::Pgm);
        assert_eq!(raw_image.colour, NetpbmColour::Grey);
        assert_eq!(raw_image.maxval, Some(maxval as u16));
        assert_eq!(
            raw_image.bits_per_component,
            if maxval < 256 { 8 } else { 16 },
            "maxval {maxval}"
        );
        assert_eq!(raw_image.data, want, "raw, maxval {maxval}");
        assert!(raw_image.complete);
        assert!(raw_image.warnings.is_empty(), "{:?}", raw_image.warnings);

        let header = format!("P2\n{width} {height}\n{maxval}\n");
        let plain_image = decode(&file(&header, &plain(&values)));
        assert_eq!(plain_image.format, NetpbmFormat::PlainPgm);
        assert_eq!(plain_image.data, want, "plain, maxval {maxval}");
        assert!(plain_image.complete);
        assert!(
            plain_image.warnings.is_empty(),
            "{:?}",
            plain_image.warnings
        );
    }
}

/// The scaling at the points it can be worked out by hand: the definition,
/// not the test's own arithmetic above.
#[test]
fn the_scaling_lands_where_it_is_worked_out_by_hand() {
    // Maxval 15 is a factor of 255, so every step is seventeen.
    let image = decode(&file("P5 4 1 15\n", &[0, 1, 8, 15]));
    assert_eq!(image.data, [0, 17, 136, 255]);
    // Maxval 255 and 65535 are the identity.
    let image = decode(&file("P5 3 1 255\n", &[0, 128, 255]));
    assert_eq!(image.data, [0, 128, 255]);
    let image = decode(&file("P5 2 1 65535\n", &[0x12, 0x34, 0xFF, 0xFF]));
    assert_eq!(image.data, [0x12, 0x34, 0xFF, 0xFF]);
    // 500 of 1000 is 32767.5 of 65535, which rounds up; 1 of 1000 is 65.535.
    let image = decode(&file("P5 2 1 1000\n", &[0x01, 0xF4, 0x00, 0x01]));
    assert_eq!(image.data, [0x80, 0x00, 0x00, 66]);
    // A maxval of 1: black and white, in grey.
    let image = decode(&file("P2 2 1 1\n", b"0 1\n"));
    assert_eq!(image.data, [0, 255]);
}

#[test]
fn ppm_holds_red_green_and_blue_in_that_order() {
    let (width, height) = (5, 4);
    for maxval in [255u32, 4_095] {
        let values = ramp(width * height * 3, maxval);
        let want = expected(&values, maxval);
        let header = format!("P6\n{width} {height}\n{maxval}\n");
        let raw_image = decode(&file(&header, &raw(&values, maxval)));
        assert_eq!(raw_image.format, NetpbmFormat::Ppm);
        assert_eq!(raw_image.colour, NetpbmColour::Rgb);
        assert_eq!(raw_image.data, want, "raw, maxval {maxval}");
        assert!(raw_image.complete);

        let header = format!("P3\n{width} {height}\n{maxval}\n");
        let plain_image = decode(&file(&header, &plain(&values)));
        assert_eq!(plain_image.format, NetpbmFormat::PlainPpm);
        assert_eq!(plain_image.data, want, "plain, maxval {maxval}");
        assert!(plain_image.complete);
    }
    // One pixel of each primary, so the order is the samples' and not a
    // permutation that happens to agree.
    let image = decode(&file("P6 3 1 255\n", &[255, 0, 0, 0, 255, 0, 0, 0, 255]));
    assert_eq!(image.data, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
}

// ---- the header -----------------------------------------------------------------

#[test]
fn a_raw_raster_starts_after_exactly_one_white_space_byte() {
    // The line feed after the maxval delimits the raster, and the second is
    // the first sample: ten.
    let image = decode(b"P5 2 1 255\n\n\x07");
    assert_eq!(image.data, [10, 7]);
    // A carriage return and a line feed: the return delimits, the feed is a
    // sample.
    let image = decode(b"P5 2 1 255\r\n\x05");
    assert_eq!(image.data, [10, 5]);
    // A comment before the delimiter is the header's, and the line feed that
    // ends it is the delimiter.
    let image = decode(b"P5 2 1 255#a note\n\x01\x02");
    assert_eq!(image.data, [1, 2]);
    // A PBM's raster follows its height the same way.
    let image = decode(b"P4 8 1\n\x0F");
    assert_eq!(image.data, [255, 255, 255, 255, 0, 0, 0, 0]);
}

#[test]
fn comments_and_every_white_space_character_are_read_in_the_header() {
    // Tab, vertical tab, form feed and carriage return between the fields; a
    // comment straight after the magic number, one touching the width, and
    // one on a line of its own.
    let header = "P2#first\n#second\r 3#touching\n\x0b2\x0c#third\n255\t";
    let image = decode(&file(header, b"1 2 3 4 5 6\n"));
    assert_eq!((image.width, image.height), (3, 2));
    assert_eq!(image.data, [1, 2, 3, 4, 5, 6]);
    assert!(image.warnings.is_empty(), "{:?}", image.warnings);
}

#[test]
fn a_header_that_is_not_netpbm_or_says_nothing_usable_is_refused() {
    for not in [
        &b""[..],
        b"P",
        b"P8 1 1",
        b"PX",
        b"Q5 1 1 255\n",
        b"BM",
        b"p5 1 1 255\n",
    ] {
        assert_eq!(refused(not), NetpbmError::NotNetpbm, "{not:?}");
    }
    for cut in [
        &b"P5"[..],
        b"P5 ",
        b"P5 1",
        b"P5 1 1",
        b"P5 1 1 255",
        b"P6\n# only\n",
    ] {
        assert_eq!(refused(cut), NetpbmError::TruncatedHeader, "{cut:?}");
    }
    for bad in [
        &b"P51 1 255\n"[..],
        b"P5 1x 1 255\n",
        b"P2 1 1 -5\n",
        b"P5 one 1 255\n",
    ] {
        assert!(matches!(refused(bad), NetpbmError::BadHeader(_)), "{bad:?}");
    }
    assert_eq!(
        refused(b"P5 0 1 255\n"),
        NetpbmError::BadDimensions {
            width: 0,
            height: 1
        }
    );
    assert_eq!(
        refused(b"P4 4294967296 1\n"),
        NetpbmError::BadDimensions {
            width: 4_294_967_296,
            height: 1
        }
    );
    // A number of any length saturates rather than overflowing.
    assert_eq!(
        refused(b"P5 1 99999999999999999999999 255\n"),
        NetpbmError::BadDimensions {
            width: 1,
            height: u64::MAX
        }
    );
    assert_eq!(refused(b"P5 1 1 0\n"), NetpbmError::BadMaxval(0));
    assert_eq!(refused(b"P6 1 1 65536\n"), NetpbmError::BadMaxval(65_536));
}

// ---- PAM ------------------------------------------------------------------------

/// A PAM header as the description lays one out.
fn pam(width: usize, height: usize, depth: u32, maxval: u32, tuple_type: &str) -> String {
    format!(
        "P7\nWIDTH {width}\nHEIGHT {height}\nDEPTH {depth}\nMAXVAL {maxval}\nTUPLTYPE {tuple_type}\nENDHDR\n"
    )
}

#[test]
fn pam_reads_each_tuple_type_it_names() {
    let (width, height) = (13, 24);
    for (tuple_type, depth, maxval, colour) in [
        ("BLACKANDWHITE", 1, 1, NetpbmColour::Grey),
        ("GRAYSCALE", 1, 255, NetpbmColour::Grey),
        ("RGB", 3, 255, NetpbmColour::Rgb),
        ("BLACKANDWHITE_ALPHA", 2, 1, NetpbmColour::GreyAlpha),
        ("GRAYSCALE_ALPHA", 2, 65_535, NetpbmColour::GreyAlpha),
        ("RGB_ALPHA", 4, 255, NetpbmColour::Rgba),
        ("CMYK", 4, 255, NetpbmColour::Cmyk),
        ("CMYK_ALPHA", 5, 1_000, NetpbmColour::CmykAlpha),
    ] {
        let values = ramp(width * height * depth as usize, maxval);
        let header = pam(width, height, depth, maxval, tuple_type);
        let image = decode(&file(&header, &raw(&values, maxval)));
        assert_eq!(image.format, NetpbmFormat::Pam, "{tuple_type}");
        assert_eq!(image.colour, colour, "{tuple_type}");
        assert_eq!(image.colour.components(), depth, "{tuple_type}");
        assert_eq!(image.maxval, Some(maxval as u16), "{tuple_type}");
        assert_eq!(image.data, expected(&values, maxval), "{tuple_type}");
        assert!(image.complete, "{tuple_type}");
        assert!(
            image.warnings.is_empty(),
            "{tuple_type}: {:?}",
            image.warnings
        );
    }
}

/// "In PAM, zero means black": `BLACKANDWHITE` is PBM the other way up.
#[test]
fn black_and_white_is_zero_black_where_pbm_is_one_black() {
    let image = decode(&file(&pam(2, 1, 1, 1, "BLACKANDWHITE"), &[0, 1]));
    assert_eq!(image.data, [0, 255]);
    let image = decode(b"P4 2 1\n\x80");
    assert_eq!(image.data, [0, 255]);
    // At a maxval other than 1 it is grey all the same: a tuple type "does
    // not affect how the samples or tuples are encoded".
    let image = decode(&file(&pam(2, 1, 1, 255, "BLACKANDWHITE"), &[0, 51]));
    assert_eq!(image.data, [0, 51]);
}

#[test]
fn pam_header_lines_are_read_in_any_order_with_comments_and_blank_lines() {
    let header = "P7\nTUPLTYPE RGB\n# written by hand\n\n   \nMAXVAL 255\r\nHEIGHT 1\n  DEPTH   3\nWIDTH 2\nENDHDR\n";
    let image = decode(&file(header, &[1, 2, 3, 4, 5, 6]));
    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.colour, NetpbmColour::Rgb);
    assert_eq!(image.data, [1, 2, 3, 4, 5, 6]);
    assert!(image.warnings.is_empty(), "{:?}", image.warnings);
    // White space after the tuple type, carriage return included, is not
    // part of it, nor is the white space after the token.
    let header = "P7\nWIDTH 1\nHEIGHT 1\nDEPTH 1\nMAXVAL 255\nTUPLTYPE \t  GRAYSCALE  \r\nENDHDR\n";
    assert_eq!(decode(&file(header, &[9])).data, [9]);
}

#[test]
fn tuple_type_lines_are_joined_by_one_blank() {
    // "The tuple type is the concatenation of the values from each of them,
    // separated by a single blank", which the refusal shows.
    let header = "P7\nWIDTH 1\nHEIGHT 1\nDEPTH 2\nMAXVAL 255\nTUPLTYPE GRAYSCALE\nTUPLTYPE   ALPHA \nENDHDR\n";
    assert_eq!(
        refused(&file(header, &[0, 0])),
        NetpbmError::UnknownTupleType("GRAYSCALE ALPHA".into())
    );
    // A tuple type is kept to 64 bytes, however many lines name it.
    let mut header = String::from("P7\nWIDTH 1\nHEIGHT 1\nDEPTH 1\nMAXVAL 255\n");
    for _ in 0..40 {
        header.push_str("TUPLTYPE ABCDEFGHIJ\n");
    }
    header.push_str("ENDHDR\n");
    match refused(&file(&header, &[0])) {
        NetpbmError::UnknownTupleType(name) => {
            assert_eq!(name.len(), MAX_TUPLE_TYPE);
            assert!(name.starts_with("ABCDEFGHIJ ABCDEFGHIJ"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pam_deeper_than_its_tuple_type_reads_its_first_planes() {
    // Grey and opacity, then two planes the tuple type does not use.
    let raster = [10, 20, 30, 40, 50, 60, 70, 80];
    let image = decode(&file(&pam(2, 1, 4, 255, "GRAYSCALE_ALPHA"), &raster));
    assert_eq!(image.colour, NetpbmColour::GreyAlpha);
    assert_eq!(image.data, [10, 20, 50, 60]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmPlanesIgnored]);
    // And at two bytes a sample.
    let raster = [0, 1, 0, 2, 0, 3, 0, 4];
    let image = decode(&file(&pam(2, 1, 2, 65_535, "GRAYSCALE"), &raster));
    assert_eq!(image.data, [0, 1, 0, 3]);
    assert_eq!(image.warnings, [Warning::NetpbmPlanesIgnored]);
}

#[test]
fn a_pam_header_the_description_does_not_allow_is_refused() {
    let lines = |skip: &str, extra: &str| {
        let mut header = String::from("P7\n");
        for line in [
            "WIDTH 1",
            "HEIGHT 1",
            "DEPTH 3",
            "MAXVAL 255",
            "TUPLTYPE RGB",
        ] {
            if !line.starts_with(skip) {
                header.push_str(line);
                header.push('\n');
            }
        }
        header.push_str(extra);
        header.push_str("ENDHDR\n");
        file(&header, &[0, 0, 0])
    };
    for field in ["WIDTH", "HEIGHT", "DEPTH", "MAXVAL"] {
        assert!(
            matches!(refused(&lines(field, "")), NetpbmError::BadHeader(_)),
            "without {field}"
        );
    }
    // A line that may appear once, twice: `-` begins no line, so none is
    // left out.
    assert!(matches!(
        refused(&lines("-", "HEIGHT 1\n")),
        NetpbmError::BadHeader(_)
    ));
    assert!(matches!(
        refused(&lines("WIDTH", "WIDTH two\n")),
        NetpbmError::BadHeader(_)
    ));
    assert!(matches!(
        refused(&lines("WIDTH", "WIDTH\n")),
        NetpbmError::BadHeader(_)
    ));
    assert_eq!(
        refused(&lines("TUPLTYPE", "")),
        NetpbmError::UnknownTupleType(String::new())
    );
    assert_eq!(
        refused(&lines("TUPLTYPE", "TUPLTYPE rgb\n")),
        NetpbmError::UnknownTupleType("rgb".into())
    );
    assert_eq!(
        refused(&lines("DEPTH", "DEPTH 2\n")),
        NetpbmError::BadDepth { depth: 2, needs: 3 }
    );
    assert_eq!(
        refused(&lines("DEPTH", "DEPTH 0\n")),
        NetpbmError::BadDepth { depth: 0, needs: 3 }
    );
    assert_eq!(
        refused(&lines("MAXVAL", "MAXVAL 0\n")),
        NetpbmError::BadMaxval(0)
    );
    assert_eq!(
        refused(&lines("MAXVAL", "MAXVAL 70000\n")),
        NetpbmError::BadMaxval(70_000)
    );
    assert_eq!(
        refused(&lines("WIDTH", "WIDTH 0\n")),
        NetpbmError::BadDimensions {
            width: 0,
            height: 1
        }
    );
    // No `ENDHDR`, or no newline after it.
    assert_eq!(
        refused(b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 1\nMAXVAL 1\nTUPLTYPE GRAYSCALE\n"),
        NetpbmError::TruncatedHeader
    );
    assert_eq!(
        refused(b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 1\nMAXVAL 1\nTUPLTYPE GRAYSCALE\nENDHDR"),
        NetpbmError::TruncatedHeader
    );
    assert_eq!(refused(b"P7"), NetpbmError::TruncatedHeader);
    // An image viewer's thumbnail starts `P7 332`, which the description
    // names as the other format that shares the magic number.
    assert!(matches!(
        refused(b"P7 332\n#END_OF_COMMENTS\n1 1 255\n\0"),
        NetpbmError::BadHeader(_)
    ));
}

#[test]
fn pam_lines_this_module_does_not_read_are_passed_over() {
    let header = "P7\nWIDTH 2 extra\nHEIGHT 1\nCOLORS 3\nDEPTH 1\nTUPLTYPE\nMAXVAL 255\nTUPLTYPE GRAYSCALE\nENDHDR now\n";
    let image = decode(&file(header, &[7, 8]));
    assert_eq!(image.data, [7, 8]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmHeaderLineIgnored]);
}

// ---- PFM ------------------------------------------------------------------------

fn floats_be(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn floats_le(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn pfm_floats_map_onto_sixteen_bits_rows_bottom_to_top() {
    // Grey, big-endian: the bottom row first in the file, the top row first
    // in the picture.
    let bottom = [0.0f32, 1.0];
    let top = [0.5f32, 2.0];
    let raster = floats_be(&[bottom, top].concat());
    let image = decode(&file("Pf\n2 2\n1.0\n", &raster));
    assert_eq!(image.format, NetpbmFormat::Pfm);
    assert_eq!(image.colour, NetpbmColour::Grey);
    assert_eq!(image.bits_per_component, 16);
    assert_eq!(image.maxval, None);
    assert_eq!(image.scale, Some(1.0));
    // 0.5 x 65535 = 32767.5, which rounds up; 2.0 is past 1 and clamps.
    let want: Vec<u8> = [32_768u16, 65_535, 0, 65_535]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    assert_eq!(image.data, want);
    assert!(image.complete);
    assert!(image.warnings.is_empty(), "{:?}", image.warnings);

    // Colour, little-endian by its negative scale: below zero and NaN are
    // black, a quarter is 16383.75 and rounds up.
    let pixels = [0.25f32, -3.0, f32::NAN, 1.0, 0.0, 0.75];
    let image = decode(&file("PF\n2 1\n-0.5\n", &floats_le(&pixels)));
    assert_eq!(image.colour, NetpbmColour::Rgb);
    assert_eq!(image.scale, Some(0.5));
    let want: Vec<u8> = [16_384u16, 0, 0, 65_535, 0, 49_151]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    assert_eq!(image.data, want);
}

#[test]
fn a_pfm_scale_factor_is_any_decimal_number_but_zero() {
    for scale in ["1", "2.5", ".5", "-1.", "+3e2", "-1E-3", "1e-300"] {
        let header = format!("Pf 1 1 {scale}\n");
        let raster = if scale.starts_with('-') {
            floats_le(&[1.0])
        } else {
            floats_be(&[1.0])
        };
        let image = decode(&file(&header, &raster));
        assert_eq!(image.data, [0xFF, 0xFF], "{scale}");
    }
    assert_eq!(refused(b"Pf 1 1 0.0\n\0\0\0\0"), NetpbmError::BadScale);
    assert_eq!(refused(b"Pf 1 1 -0\n\0\0\0\0"), NetpbmError::BadScale);
    for not in ["inf", "-infinity", "nan", "1..2", "e5", "1e", "0x10", "--1"] {
        let header = format!("Pf 1 1 {not}\n\0\0\0\0");
        assert!(
            matches!(refused(header.as_bytes()), NetpbmError::BadHeader(_)),
            "{not}"
        );
    }
}

#[test]
fn a_pfm_header_that_says_nothing_usable_is_refused() {
    for cut in [&b"PF"[..], b"PF\n", b"PF\n2", b"PF\n2 2", b"PF\n2 2\n1.0"] {
        assert_eq!(refused(cut), NetpbmError::TruncatedHeader, "{cut:?}");
    }
    assert!(matches!(refused(b"PFx 1 1 1\n"), NetpbmError::BadHeader(_)));
    assert!(matches!(refused(b"PF 2x 1 1\n"), NetpbmError::BadHeader(_)));
    assert!(matches!(
        refused(b"PF #c\n1 1 1\n"),
        NetpbmError::BadHeader(_)
    ));
    assert_eq!(
        refused(b"PF 0 1 1\n"),
        NetpbmError::BadDimensions {
            width: 0,
            height: 1
        }
    );
}

// ---- the portable inkmap ---------------------------------------------------------

/// Every combination of one bit of each ink, cyan the highest.
fn inks() -> Vec<[bool; 4]> {
    (0..16u8)
        .map(|i| [i & 8 != 0, i & 4 != 0, i & 2 != 0, i & 1 != 0])
        .collect()
}

/// "CMYK internally, RGB output", in the ASCII form and the raw one: a PPM
/// whose samples are the colours a raster of one bit an ink can show, each
/// ink taken from white and black taking everything. Which conversion the
/// producer made is its own; the file holds RGB, and RGB is what comes back.
#[test]
fn the_inkmap_as_rgb_output_decodes_as_the_ppm_it_is() {
    let rgb: Vec<u32> = inks()
        .iter()
        .flat_map(|&[c, m, y, k]| {
            let channel = |ink: bool| if ink || k { 0 } else { 255 };
            [channel(c), channel(m), channel(y)]
        })
        .collect();
    let want = expected(&rgb, 255);
    let raw_image = decode(&file("P6\n16 1\n255\n", &raw(&rgb, 255)));
    assert_eq!(raw_image.colour, NetpbmColour::Rgb);
    assert_eq!(raw_image.data, want);
    let plain_image = decode(&file("P3\n16 1\n255\n", &plain(&rgb)));
    assert_eq!(plain_image.data, want);
    // And at a maxval of 1, one bit a channel written as it is.
    let bit: Vec<u32> = rgb.iter().map(|&v| v / 255).collect();
    let image = decode(&file("P3\n16 1\n1\n", &plain(&bit)));
    assert_eq!(image.data, want);
}

/// "A 4bpp cmyk bitmap ... as a PAM image file": four planes of one bit, read
/// where the tuple type is `CMYK`, each sample scaled to 0 or 255.
#[test]
fn the_inkmap_as_a_pam_decodes_to_its_inks() {
    let samples: Vec<u32> = inks().iter().flat_map(|ink| ink.map(u32::from)).collect();
    let image = decode(&file(&pam(16, 1, 4, 1, "CMYK"), &raw(&samples, 1)));
    assert_eq!(image.colour, NetpbmColour::Cmyk);
    assert_eq!(image.maxval, Some(1));
    let want: Vec<u8> = samples
        .iter()
        .map(|&s| if s == 1 { 255 } else { 0 })
        .collect();
    assert_eq!(image.data, want);
    assert!(image.complete);
}

// ---- damage -----------------------------------------------------------------------

#[test]
fn a_raster_cut_short_keeps_what_it_held() {
    // Grey: the missing samples are zero, black.
    let image = decode(b"P5 3 2 255\n\x01\x02\x03\x04");
    assert_eq!(image.data, [1, 2, 3, 4, 0, 0]);
    assert!(!image.complete);
    assert_eq!(image.warnings, [Warning::TruncatedInput]);
    // PBM: zero is white.
    let image = decode(b"P4 8 2\n\xFF");
    assert_eq!(
        image.data,
        [0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255]
    );
    assert!(!image.complete);
    let image = decode(b"P1 2 2 1 0 1");
    assert_eq!(image.data, [0, 255, 0, 255]);
    assert!(!image.complete);
    // Two bytes a sample, cut inside the second: the pixel the cut is in is
    // dropped whole.
    let image = decode(b"P6 2 1 65535\n\x00\x01\x00\x02\x00\x03\x00");
    assert_eq!(image.data, [0, 1, 0, 2, 0, 3, 0, 0, 0, 0, 0, 0]);
    assert!(!image.complete);
    // Plain, a sample short.
    let image = decode(b"P2 2 2 255 9 8 7\n");
    assert_eq!(image.data, [9, 8, 7, 0]);
    assert!(!image.complete);
    assert_eq!(image.warnings, [Warning::TruncatedInput]);
    // PAM and PFM.
    let image = decode(&file(&pam(2, 1, 3, 255, "RGB"), &[1, 2, 3, 4]));
    assert_eq!(image.data, [1, 2, 3, 0, 0, 0]);
    assert!(!image.complete);
    let image = decode(&file("Pf 2 1 1\n", &floats_be(&[1.0])));
    assert_eq!(image.data, [0xFF, 0xFF, 0, 0]);
    assert!(!image.complete);
    // A header and no raster at all.
    let image = decode(b"P5 2 2 255\n");
    assert_eq!(image.data, [0, 0, 0, 0]);
    assert!(!image.complete);
}

#[test]
fn samples_above_the_maxval_are_the_maxval() {
    // 50 of 100 is 127.5 of 255, which rounds up; 200 is past 100.
    let image = decode(b"P5 2 1 100\n\x32\xC8");
    assert_eq!(image.data, [128, 255]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmSampleAboveMaxval]);
    // Two bytes, and a plain number of any length.
    let image = decode(b"P5 1 1 1000\n\xFF\xFF");
    assert_eq!(image.data, [0xFF, 0xFF]);
    assert_eq!(image.warnings, [Warning::NetpbmSampleAboveMaxval]);
    let image = decode(b"P2 2 1 10 11 99999999999999999999999999\n");
    assert_eq!(image.data, [255, 255]);
    assert_eq!(image.warnings, [Warning::NetpbmSampleAboveMaxval]);
    // A PAM's planes too.
    let image = decode(&file(&pam(1, 1, 3, 9, "RGB"), &[9, 10, 0]));
    assert_eq!(image.data, [255, 255, 0]);
    assert_eq!(image.warnings, [Warning::NetpbmSampleAboveMaxval]);
}

#[test]
fn a_plain_raster_stops_at_junk_and_skips_a_comment() {
    let image = decode(b"P2 2 2 255\n1 2 x 3\n");
    assert_eq!(image.data, [1, 2, 0, 0]);
    assert!(!image.complete);
    assert_eq!(
        image.warnings,
        [Warning::NetpbmRasterJunk, Warning::TruncatedInput]
    );
    let image = decode(b"P2 2 2 255\n# after the header\n1 2 # a note\n3 4\n");
    assert_eq!(image.data, [1, 2, 3, 4]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmRasterJunk]);
    let image = decode(b"P1 2 2\n0 1 # a note\n1 0\n");
    assert_eq!(image.data, [255, 0, 0, 255]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmRasterJunk]);
    let image = decode(b"P1 2 2\n0 1 2 0\n");
    assert_eq!(image.data, [255, 0, 255, 255]);
    assert!(!image.complete);
    assert_eq!(
        image.warnings,
        [Warning::NetpbmRasterJunk, Warning::TruncatedInput]
    );
}

#[test]
fn the_bytes_after_an_image_are_judged_by_what_they_are() {
    // Another image of the kind, straight after the raster.
    let image = decode(b"P5 1 1 255\n\x07P5 1 1 255\n\x08");
    assert_eq!(image.data, [7]);
    assert!(image.complete);
    assert_eq!(image.warnings, [Warning::NetpbmImagesIgnored]);
    let second = pam(1, 1, 1, 255, "GRAYSCALE");
    let mut two = file(&second, &[1]);
    two.extend_from_slice(&file(&second, &[2]));
    assert_eq!(decode(&two).warnings, [Warning::NetpbmImagesIgnored]);
    // Anything else after a raw raster, a line feed included, is garbage.
    for tail in [&b"\n"[..], b"P6 1 1 255\n\0\0\0", b"junk"] {
        let image = decode(&[&b"P5 1 1 255\n\x07"[..], tail].concat());
        assert_eq!(image.warnings, [Warning::TrailingGarbage], "{tail:?}");
    }
    let image = decode(&file("Pf 1 1 1\n", &floats_be(&[0.0, 0.0])));
    assert_eq!(image.warnings, [Warning::TrailingGarbage]);
    // After a plain raster, junk that starts with white space is allowed.
    let image = decode(b"P2 2 1 255\n1 2\nanything at all");
    assert!(image.warnings.is_empty(), "{:?}", image.warnings);
    let image = decode(b"P2 2 1 255\n1 2x");
    assert_eq!(image.warnings, [Warning::TrailingGarbage]);
    let image = decode(b"P1 3 1\n0110");
    assert_eq!(image.data, [255, 0, 0]);
    assert_eq!(image.warnings, [Warning::TrailingGarbage]);
}

// ---- the budget -------------------------------------------------------------------

/// `MAX_NETPBM_SAMPLES` fires on a header asking for one sample more than it
/// allows, before anything is allocated — the refusal is the answer, and no
/// raster stands behind the header.
#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    // 65 536 x 1 025 grey is 67 174 400 samples, 65 536 past the cap.
    assert_eq!(
        refused(b"P5 65536 1025 255\n"),
        NetpbmError::TooManySamples {
            samples: 67_174_400,
            max: MAX_NETPBM_SAMPLES
        }
    );
    // Two 32-bit dimensions at five components saturate.
    let header = pam(4_294_967_295, 4_294_967_295, 5, 255, "CMYK_ALPHA");
    assert_eq!(
        refused(header.as_bytes()),
        NetpbmError::TooManySamples {
            samples: u64::MAX,
            max: MAX_NETPBM_SAMPLES
        }
    );
    // At the cap exactly, the cap admits it and the caller's ceiling is what
    // refuses: the cap is inclusive.
    assert_eq!(
        netpbm_decode(b"P5 8192 8192 255\n", &Limits::new(1)),
        Err(NetpbmError::ExceedsOutputLimit {
            bytes: 67_108_864,
            limit: 1
        })
    );
    // Sixteen bits a sample are two bytes of the ceiling.
    assert_eq!(
        netpbm_decode(b"P5 10 5 65535\n", &Limits::new(99)),
        Err(NetpbmError::ExceedsOutputLimit {
            bytes: 100,
            limit: 99
        })
    );
    assert!(netpbm_decode(b"P5 10 5 65535\n", &Limits::new(100)).is_ok());
}

// ---- the doors and the names ------------------------------------------------------

#[test]
fn the_counted_door_returns_the_uncounted_answer() {
    let fixtures: Vec<Vec<u8>> = vec![
        b"P5 2 1 255\n\x01\x02".to_vec(),
        b"P2 2 2 255 1 2 3 4\n".to_vec(),
        file(&pam(2, 1, 3, 255, "RGB"), &[1, 2, 3, 4, 5, 6]),
        file("Pf 1 1 1\n", &floats_be(&[0.5])),
        b"P4 3 1\n\xA0".to_vec(),
        b"P5 1 1 0\n".to_vec(),
    ];
    for fixture in &fixtures {
        let (counted, steps) = netpbm_decode_counted(fixture, &ROOM);
        assert_eq!(counted, netpbm_decode(fixture, &ROOM), "{fixture:?}");
        assert!(steps > 0, "{fixture:?}");
    }
    // A wider image costs more.
    let (_, small) = netpbm_decode_counted(&file("P5 2 2 255\n", &[0; 4]), &ROOM);
    let (_, large) = netpbm_decode_counted(&file("P5 20 20 255\n", &[0; 400]), &ROOM);
    assert!(large > small, "{large} against {small}");
}

#[test]
fn every_refusal_and_leniency_has_a_name() {
    for e in [
        NetpbmError::NotNetpbm,
        NetpbmError::TruncatedHeader,
        NetpbmError::BadHeader("x"),
        NetpbmError::BadDimensions {
            width: 0,
            height: 1,
        },
        NetpbmError::BadMaxval(0),
        NetpbmError::BadDepth { depth: 1, needs: 3 },
        NetpbmError::UnknownTupleType(String::new()),
        NetpbmError::UnknownTupleType("XYZ".into()),
        NetpbmError::BadScale,
        NetpbmError::TooManySamples { samples: 1, max: 0 },
        NetpbmError::ExceedsOutputLimit { bytes: 1, limit: 0 },
    ] {
        assert!(!e.to_string().is_empty());
    }
    for warning in [
        Warning::NetpbmSampleAboveMaxval,
        Warning::NetpbmImagesIgnored,
        Warning::NetpbmPlanesIgnored,
        Warning::NetpbmRasterJunk,
        Warning::NetpbmHeaderLineIgnored,
    ] {
        assert!(warning.as_str().starts_with("netpbm-"));
        assert!(!warning.to_string().is_empty());
    }
}

#[test]
fn the_formats_and_colours_answer_their_questions() {
    for (format, plain) in [
        (NetpbmFormat::PlainPbm, true),
        (NetpbmFormat::Pbm, false),
        (NetpbmFormat::PlainPgm, true),
        (NetpbmFormat::Pgm, false),
        (NetpbmFormat::PlainPpm, true),
        (NetpbmFormat::Ppm, false),
        (NetpbmFormat::Pam, false),
        (NetpbmFormat::Pfm, false),
    ] {
        assert_eq!(format.is_plain(), plain, "{format:?}");
    }
    for (colour, components, alpha) in [
        (NetpbmColour::Grey, 1, false),
        (NetpbmColour::GreyAlpha, 2, true),
        (NetpbmColour::Rgb, 3, false),
        (NetpbmColour::Rgba, 4, true),
        (NetpbmColour::Cmyk, 4, false),
        (NetpbmColour::CmykAlpha, 5, true),
    ] {
        assert_eq!(colour.components(), components, "{colour:?}");
        assert_eq!(colour.has_alpha(), alpha, "{colour:?}");
    }
}
