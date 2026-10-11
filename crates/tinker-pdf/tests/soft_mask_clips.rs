//! A clip installed while a soft mask is in force is **not masked**.
//!
//! 8.5.4's clip and 11.6.5's soft mask are two parameters of the graphics
//! state. The renderer built every clip through the same coverage function as
//! a fill, and that function multiplies the soft mask in — so a clip set under
//! a mask carried the mask with it. Two pictures came out wrong, both
//! plausible:
//!
//! - a **transparency group under a mask** was masked twice. The interpreter
//!   installs a form's `/BBox` clip before the group takes the mask, so the
//!   group's content was masked through that clip and the group's composite
//!   masked again: a grey 0.5 mask kept a quarter.
//! - a clip set under a mask went on masking after `/SMask /None` turned the
//!   mask off, for as long as the clip lasted.
//!
//! Every expected value is the clause's arithmetic: a black square kept at
//! alpha `m` over white is `255 × (1 − m)`.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | a clip is built with the soft mask multiplied in (the old renderer) | 2 |

use tinker_pdf::{Document, RenderOptions};

/// A 100-point page drawing `content`, with `/K` a luminosity mask whose group
/// `/M` fills the page with grey 0.5, `/N` the state turning a mask off, and
/// `/G` a transparency group filling the page black.
fn pdf(content: &str) -> Vec<u8> {
    let mask = "0.5 0.5 0.5 rg 0 0 100 100 re f";
    let group = "0 0 0 rg 0 0 100 100 re f";
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] \
         /Resources << /ExtGState << /K 5 0 R /N 8 0 R >> /XObject << /M 6 0 R /G 7 0 R >> >> \
         /Contents 4 0 R >>"
            .into(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /ExtGState /SMask << /Type /Mask /S /Luminosity /G 6 0 R >> >>".into(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] \
             /Group << /S /Transparency /CS /DeviceRGB /I true >> /Length {} >>\n\
             stream\n{mask}\nendstream",
            mask.len()
        ),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] \
             /Group << /S /Transparency /CS /DeviceRGB /I true >> /Length {} >>\n\
             stream\n{group}\nendstream",
            group.len()
        ),
        "<< /Type /ExtGState /SMask /None >>".into(),
    ];
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

/// The red channel at the page's centre.
fn centre(content: &str) -> u8 {
    let doc = Document::open(pdf(content)).expect("it opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let width = bitmap.width as usize;
    let row = bitmap.height as usize / 2;
    bitmap.data[(row * width + width / 2) * components]
}

/// A grey 0.5 mask keeps half of a black square, whether the square is a
/// fill or a transparency group — `255 × 0.5`, not `255 × 0.75`.
#[test]
fn a_group_under_a_mask_is_masked_once() {
    let fill = centre("/K gs 0 0 0 rg 0 0 100 100 re f");
    let group = centre("/K gs /G Do");
    assert!(
        (125..=130).contains(&fill),
        "the fill, kept at a half: {fill}"
    );
    assert!(
        (125..=130).contains(&group),
        "and the group, kept at a half and not a quarter: {group}"
    );
}

/// A clip set while a mask is in force does not keep masking once the mask
/// is turned off.
#[test]
fn a_clip_does_not_carry_the_mask_past_smask_none() {
    let value = centre("/K gs 0 0 100 100 re W n /N gs 0 0 0 rg 0 0 100 100 re f");
    assert!(
        value < 0x08,
        "the mask is off and the square is black: {value}"
    );
}
