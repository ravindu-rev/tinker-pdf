//! The document that uses every construct this writer can emit.
//!
//! Extracted from the qpdf oracle of retired ruling 9, which is where it was
//! written and which the strict validator replaces. The whole point of it is
//! unchanged: every dictionary here is one a *reader* has to understand, and
//! most of them are ones this engine's own reader supplies a default for — a
//! shading with no `/Extend`, a function with no `/Domain`, a tiling pattern
//! with no `/XStep` all round-trip through this crate perfectly and are wrong.
//!
//! Two files use it. `strict_validator.rs` holds it to the rules; the
//! replacement for the oracle reads its values back.

use tinker_pdf_cos::{
    BlendMode, DeviceSpace, DocumentBuilder, ExtGState, FormXObject, Function, Glyph, MaskKind,
    Shading, StateMask, TilingPattern, TilingType, TransparencyGroup,
};

/// A four-glyph TrueType program with real metrics.
///
/// Small enough to read in a dump and complete enough to embed: `glyf`,
/// `loca`, `head`, `hhea`, `hmtx`, `maxp` and a `cmap`. The advances are 600,
/// 700, 800 and 900 font units against 1 000 per em, so `/W` is legible in
/// qpdf's own output rather than being something only this engine can check.
pub fn metric_font() -> Vec<u8> {
    let mut square = Vec::new();
    square.extend_from_slice(&1i16.to_be_bytes());
    for value in [0i16, 0, 700, 700] {
        square.extend_from_slice(&value.to_be_bytes());
    }
    square.extend_from_slice(&3u16.to_be_bytes());
    square.extend_from_slice(&0u16.to_be_bytes());
    square.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    for dx in [0i16, 700, 0, -700] {
        square.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        square.extend_from_slice(&dy.to_be_bytes());
    }

    let size = square.len() as u32;
    let mut glyf = Vec::new();
    for _ in 0..3 {
        glyf.extend_from_slice(&square);
    }
    let mut loca = Vec::new();
    for offset in [0u32, 0, size, size * 2, size * 3] {
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&4u16.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&4u16.to_be_bytes());

    let mut hmtx = Vec::new();
    for advance in [600u16, 700, 800, 900] {
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let first = u16::from(b'A');
    let mut sub = Vec::new();
    for value in [4u16, 32, 0, 4, 4, 1, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for value in [
        first,
        0xFFFF,
        0,
        first,
        0xFFFF,
        1u16.wrapping_sub(first),
        1,
        0,
        0,
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);

    let tables: [(&[u8; 4], &[u8]); 7] = [
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// One page using **every** construct this milestone added.
///
/// One document rather than nine, because `--check` reads the whole file and
/// what is being asked is whether a third party can walk all of it at once.
pub fn whole_surface_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_info(b"Title", "the writer's missing half");
    assert!(builder.add_cid_font(b"C0", b"Oracle", &metric_font()));

    assert!(builder.add_ext_gstate(
        b"GHalf",
        &ExtGState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(0.25),
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"FmMask",
        &FormXObject {
            bbox: [0.0, 0.0, 150.0, 200.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Gray,
                isolated: true,
                knockout: false,
            }),
            content: b"1 g 0 0 150 200 re f",
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GMask",
        &ExtGState {
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Luminosity,
                form: b"FmMask",
                backdrop: Some(&[0.0]),
            }),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_ext_gstate(
        b"GOff",
        &ExtGState {
            soft_mask: Some(StateMask::None),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"FmKnock",
        &FormXObject {
            bbox: [0.0, 0.0, 100.0, 100.0],
            matrix: Some([1.0, 0.0, 0.0, 1.0, 20.0, 20.0]),
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Rgb,
                isolated: false,
                knockout: true,
            }),
            content: b"/GHalf gs 1 0 0 rg 0 0 60 60 re f 0 0 1 rg 30 30 60 60 re f",
        }
    ));
    assert!(builder.add_shading(
        b"ShAxial",
        &Shading::Axial {
            color_space: DeviceSpace::Rgb,
            coords: [0.0, 0.0, 300.0, 0.0],
            function: Function::Exponential {
                domain: [0.0, 1.0],
                c0: vec![1.0, 0.0, 0.0],
                c1: vec![0.0, 0.0, 1.0],
                n: 1.0,
            },
            extend: (true, true),
        }
    ));
    assert!(builder.add_shading(
        b"ShRadial",
        &Shading::Radial {
            color_space: DeviceSpace::Rgb,
            coords: [150.0, 100.0, 0.0, 150.0, 100.0, 90.0],
            function: Function::Stitching {
                domain: [0.0, 1.0],
                functions: vec![
                    Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![1.0, 1.0, 0.0],
                        c1: vec![0.0, 1.0, 0.0],
                        n: 1.0,
                    },
                    Function::Exponential {
                        domain: [0.0, 1.0],
                        c0: vec![0.0, 1.0, 0.0],
                        c1: vec![0.0, 0.0, 1.0],
                        n: 2.0,
                    },
                ],
                bounds: vec![0.35],
                encode: vec![[0.0, 1.0], [0.0, 1.0]],
            },
            extend: (false, true),
        }
    ));
    assert!(builder.add_tiling_pattern(
        b"P0",
        &TilingPattern {
            bbox: [0.0, 0.0, 10.0, 10.0],
            x_step: 12.0,
            y_step: 14.0,
            matrix: Some([1.0, 0.0, 0.0, 1.0, 3.0, 5.0]),
            tiling_type: TilingType::NoDistortion,
            content: b"0 0 1 rg 0 0 10 10 re f",
        }
    ));

    builder.add_page(300.0, 200.0, |page| {
        page.raw(b"q 0 0 300 200 re W n");
        assert!(page.shading(b"ShAxial"));
        page.raw(b"Q");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GMask"));
        assert!(page.set_fill_pattern(b"P0"));
        page.raw(b"10 10 120 60 re f");
        assert!(page.set_ext_gstate(b"GOff"));
        page.raw(b"Q");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"GHalf"));
        assert!(page.form(b"FmKnock"));
        page.raw(b"Q");
        page.raw(b"q 200 20 80 80 re W n");
        assert!(page.shading(b"ShRadial"));
        page.raw(b"Q");
        page.set_stroke_rgb(0.0, 0.0, 0.0);
        assert!(page.set_stroke_pattern(b"P0"));
        page.raw(b"2 w 10 150 m 290 150 l S");
        assert!(page.glyphs(
            b"C0",
            18.0,
            20.0,
            170.0,
            &[
                Glyph { id: 1, text: "f" },
                Glyph { id: 2, text: "f" },
                Glyph { id: 3, text: "ffi" },
            ]
        ));
    });
    builder.finish()
}
