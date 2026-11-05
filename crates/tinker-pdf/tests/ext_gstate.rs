//! The external graphics state (8.4.5): every key a `gs` sets draws as the
//! operator that sets the same parameter.
//!
//! A **twin** is two pages of one document, sharing one indirect `/Resources`,
//! that differ only in their content streams: one sets a parameter through a
//! named `/ExtGState`, the other through its operator (`w` `J` `j` `M` `d` `ri`
//! `i`). Each twin renders byte-identical, degenerate operands and a `Q`
//! included, and each applied key's page also differs from the page without
//! it, so a pair that agrees because neither applies anything fails. A key the
//! engine stores for the row that applies it (`/SM`, `/OP`, ...) moves no
//! pixel yet, so its page draws as the page without it.
//!
//! `/CA`, `/ca`, `/BM` and `/SMask` have no operator twin; their pages are held
//! to pinned pixels.

use tinker_pdf::{Bitmap, Document, RenderOptions};

/// A document whose pages share one indirect `/Resources` (object 3) holding
/// `states` as its `/ExtGState`, and a transparency group form (object 4) a
/// soft mask can name: its left half white and its right half black. Page
/// `i` is 40 x 40 and draws `contents[i]`.
fn document(states: &str, contents: &[&str]) -> Vec<u8> {
    let kids: Vec<String> = (0..contents.len())
        .map(|i| format!("{} 0 R", 5 + 2 * i))
        .collect();
    let mask = "1 g 0 0 20 40 re f 0 g 20 0 20 40 re f";
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            contents.len(),
            kids.join(" ")
        ),
        format!("<< /ExtGState << {states} >> >>"),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 40 40] \
             /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\n\
             stream\n{mask}\nendstream",
            mask.len()
        ),
    ];
    for (i, content) in contents.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources 3 0 R \
             /Contents {} 0 R >>",
            6 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Every page of `bytes`, rendered with the default options.
fn render_all(bytes: Vec<u8>) -> Vec<Bitmap> {
    let doc = Document::open(bytes).expect("the document opens");
    (0..doc.page_count())
        .map(|i| {
            doc.page(i)
                .expect("a page")
                .render(&RenderOptions::default())
        })
        .collect()
}

/// FNV-1a over a bitmap's size and pixels.
fn pin(bitmap: &Bitmap) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let size = [bitmap.width, bitmap.height];
    for byte in size
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .chain(bitmap.data.iter().copied())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// **`/CA`, `/ca`, `/BM` and `/SMask` draw as pinned**: a half-transparent
/// stroke, a half-transparent fill, a multiplied square and a page masked to
/// the group's white half, each unlike its page without the key.
#[test]
fn the_four_keys_the_old_methods_read_draw_as_they_did() {
    let states = "/StrokeHalf << /CA 0.5 >> /FillHalf << /ca 0.5 >> \
                  /Multiply << /BM /Multiply >> \
                  /Mask << /SMask << /S /Luminosity /G 4 0 R >> >>";
    let stroke = "0 0 1 RG 4 w 5 20 m 35 20 l S";
    let fill = "0 0 1 rg 10 10 20 20 re f";
    let grey = "0.5 g 0 0 40 40 re f";
    let pages = [
        format!("/StrokeHalf gs {stroke}"),
        stroke.to_string(),
        format!("/FillHalf gs {fill}"),
        fill.to_string(),
        format!("{grey} /Multiply gs 0.5 g 10 10 20 20 re f"),
        format!("{grey} 0.5 g 10 10 20 20 re f"),
        "/Mask gs 0 0 1 rg 0 0 40 40 re f".to_string(),
        "0 0 1 rg 0 0 40 40 re f".to_string(),
    ];
    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document(states, &contents));
    let pins = [
        ("/CA", PIN_CA),
        ("/ca", PIN_FILL_CA),
        ("/BM", PIN_BM),
        ("/SMask", PIN_SMASK),
    ];
    let mut measured = Vec::new();
    let mut expected = Vec::new();
    for (index, (key, pinned)) in pins.iter().enumerate() {
        let with = &bitmaps[2 * index];
        let without = &bitmaps[2 * index + 1];
        assert_ne!(
            with.data, without.data,
            "{key}: the page draws as the page without it"
        );
        measured.push(format!("{key} {:#018x}", pin(with)));
        expected.push(format!("{key} {pinned:#018x}"));
    }
    assert_eq!(
        measured, expected,
        "a key no longer draws as the replaced method drew it"
    );
    println!("ext-gstate-pins: RAN {}", measured.join(", "));
}

/// A stroke under `/CA 0.5`.
const PIN_CA: u64 = 0x3f86_d4eb_8ca9_de85;
/// A fill under `/ca 0.5`.
const PIN_FILL_CA: u64 = 0x39ca_1c92_dd14_1b85;
/// A grey square multiplied over grey.
const PIN_BM: u64 = 0xdebf_72d1_0004_6245;
/// A blue page under a luminosity mask white on its left half.
const PIN_SMASK: u64 = 0x2034_c5ed_028d_d645;

/// The stroke every twin draws: two ends, a sharp corner and a long run, so a
/// width, a cap, a join, a miter limit and a dash each show.
const STROKE: &str = "0 0 1 RG 6 6 m 20 34 l 34 6 l S";

/// A second stroke, drawn after a `Q`.
const AFTER: &str = "0 1 0 RG 4 20 m 36 20 l S";

/// **Every scalar key a `gs` sets draws as its operator twin**, degenerate
/// operands and a `Q` included, and each applied key's page differs from the
/// page without it. A stored key with no operator draws as the page without
/// it: nothing applies it yet.
#[test]
fn every_scalar_key_set_by_gs_draws_as_its_twin() {
    // The entry, the operator that sets the same, and whether the page
    // differs from the page without it.
    let twins = [
        ("/LW 3", "3 w", true),
        ("/LW -1", "-1 w", false),
        ("/LC 1", "1 J", true),
        ("/LC 7", "7 J", false),
        ("/LJ 1", "1 j", true),
        ("/LJ 9", "9 j", false),
        ("/ML 1", "1 M", true),
        ("/ML 0.5", "0.5 M", false),
        ("/D [[3 2] 1]", "[3 2] 1 d", true),
        ("/D [[0 0] 0]", "[0 0] 0 d", false),
        ("/D [[-1 2] 0]", "[-1 2] 0 d", false),
        ("/RI /Perceptual", "/Perceptual ri", false),
        ("/RI /Vivid", "/Vivid ri", false),
        ("/FL 5", "5 i", false),
    ];
    let stored = [
        "/SM 0.5",
        "/SA true",
        "/OP true",
        "/op false",
        "/OPM 1",
        "/AIS true",
        "/TK false",
        "/UseBlackPtComp /ON",
    ];

    let mut states = String::new();
    let mut pages = Vec::new();
    for (index, (entry, operator, _)) in twins.iter().enumerate() {
        states.push_str(&format!("/S{index} << {entry} >> "));
        // A width the caps and joins show at, unless the width is the key.
        let width = if entry.starts_with("/LW") { "" } else { "4 w " };
        pages.push(format!("{width}/S{index} gs {STROKE}"));
        pages.push(format!("{width}{operator} {STROKE}"));
        pages.push(format!("{width}{STROKE}"));
    }
    // A `Q` restores what the `gs` set, as it restores what the operator set.
    for (index, operator) in [(0, "3 w"), (8, "[3 2] 1 d")] {
        pages.push(format!("4 w q /S{index} gs {STROKE} Q {AFTER}"));
        pages.push(format!("4 w q {operator} {STROKE} Q {AFTER}"));
        pages.push(format!("4 w q {STROKE} Q {AFTER}"));
    }
    for (index, entry) in stored.iter().enumerate() {
        states.push_str(&format!("/T{index} << {entry} >> "));
        pages.push(format!("4 w /T{index} gs {STROKE}"));
        pages.push(format!("4 w {STROKE}"));
    }

    let contents: Vec<&str> = pages.iter().map(String::as_str).collect();
    let bitmaps = render_all(document(&states, &contents));
    let mut at = 0;
    let labels = twins
        .iter()
        .map(|(entry, operator, applied)| (entry.to_string(), *operator, *applied))
        .chain([
            ("/LW 3 under q Q".to_string(), "3 w", true),
            ("/D [[3 2] 1] under q Q".to_string(), "[3 2] 1 d", true),
        ]);
    for (label, operator, applied) in labels {
        let (with, twin, without) = (&bitmaps[at], &bitmaps[at + 1], &bitmaps[at + 2]);
        at += 3;
        assert_eq!(
            with.data, twin.data,
            "{label}: the `gs` page does not draw as `{operator}` does"
        );
        assert_eq!(
            with.data != without.data,
            applied,
            "{label}: the page {} the page without the key",
            if applied { "draws as" } else { "differs from" }
        );
    }
    for entry in stored {
        let (with, without) = (&bitmaps[at], &bitmaps[at + 1]);
        at += 2;
        assert_eq!(
            with.data, without.data,
            "{entry}: a stored key moved a pixel before its row applies it"
        );
    }
    assert_eq!(at, bitmaps.len());
    println!(
        "ext-gstate-twins: RAN {} twins and {} stored keys",
        twins.len() + 2,
        stored.len()
    );
}
