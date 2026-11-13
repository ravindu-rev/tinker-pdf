//! Separations: a page's spot colorants and its plates (8.6.6.4, 8.6.6.5,
//! 10.8; `docs/design/overprint-separations.md`).
//!
//! Every expectation here is read off the fixture as it is written — which
//! space names which colorant, in which scope — and never off the code that
//! reads it.

use tinker_pdf::{Document, Page};

/// A document whose objects are `objects`, numbered from 1 and written with a
/// cross-reference table, so that nothing about opening it is repaired and
/// a warning after opening is one something else raised.
fn document(objects: &[String]) -> Document {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Document::open(bytes).expect("it opens")
}

/// A stream object's body: `dict`'s entries and `data`, with its length.
fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// `[/Separation /name /DeviceCMYK tint]`, the tint a linear ramp into
/// cyan, as written in a resource dictionary.
fn separation(name: &str) -> String {
    format!(
        "[/Separation /{name} /DeviceCMYK \
         << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>]"
    )
}

/// `[/DeviceN [names] /DeviceCMYK tint attributes]`.
fn device_n(names: &[&str], attributes: &str) -> String {
    let names: String = names.iter().map(|name| format!("/{name} ")).collect();
    format!(
        "[/DeviceN [{names}] /DeviceCMYK \
         << /FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1 0 1] /Length 0 >> {attributes}]"
    )
}

/// A form XObject's body whose `/Resources` are `resources` and whose other
/// keys are `keys`.
fn form(keys: &str, resources: &str) -> String {
    stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] {keys} /Resources << {resources} >>"
        ),
        "",
    )
}

/// A page tree of one page, objects 1 to 3, the page's resources
/// `resources` and its other keys `keys`; every further object is `extra`,
/// numbered from 4.
fn one_page(resources: &str, keys: &str, extra: &[String]) -> Vec<String> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << {resources} >> {keys} >>"),
    ];
    objects.extend_from_slice(extra);
    objects
}

fn names(page: &Page) -> Vec<String> {
    page.spot_colorants()
        .into_iter()
        .map(|spot| String::from_utf8_lossy(&spot.name).into_owned())
        .collect()
}

/// **The inventory is every spot the page's resource scopes name, each
/// once, in the order the walk first meets it** (RC-04.2).
///
/// One page putting a spot in every scope a render enters — the page's
/// resources, a form's and a form inside it, a tiling pattern's cell, a Type
/// 3 font's glyphs through `/Font` and through a graphics state's `/Font`, a
/// soft mask's group, an annotation's appearance and each state of a
/// checkbox's — and in every kind of space that names one: a `/Separation`,
/// a `/DeviceN`'s names and its `/Colorants`, an `/Indexed` base, a
/// `/Pattern` space's underlying space, an image's space, a shading's, a
/// shading pattern's and a form group's. The expectation is the fixture read
/// by the module comment's order: within a scope its `/ColorSpace`, then its
/// XObjects' spaces, its shadings', its shading patterns', its soft masks'
/// groups'; then the scopes it opens, Type 3 fonts, graphics states, forms
/// and tiling patterns; and the annotations last.
///
/// What is absent is absent for a reason the fixture states: a process name,
/// `/All` and `/None`; a spot named a second time; a stencil mask's
/// `/ColorSpace`, which 8.9.6.2 gives no meaning; an annotation's down
/// appearance, which no render draws; and an object no scope reaches.
#[test]
fn the_colorant_inventory_is_every_spot_its_resources_name() {
    let tiling = stream(
        &format!(
            "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 4 4] /XStep 4 /YStep 4 \
             /Resources << /ColorSpace << /T {} >> >>",
            separation("TilingSep")
        ),
        "",
    );
    let objects = one_page(
        &format!(
            "/ColorSpace << /CS0 {page_sep} /CS1 {page_dn} /CS2 [/Indexed {indexed} 1 <0000>] \
             /CS3 [/Pattern {patterned}] /CS4 /DeviceRGB /CS5 {process} /CS6 {all} /CS7 {none} >> \
             /XObject << /Im1 8 0 R /Im2 9 0 R /Fm1 10 0 R >> \
             /Shading << /Sh1 << /ShadingType 2 /ColorSpace {shading} /Coords [0 0 1 0] \
                 /Function << /FunctionType 2 /Domain [0 1] /N 1 >> >> >> \
             /Pattern << /P1 << /PatternType 2 /Shading << /ShadingType 2 \
                 /ColorSpace {shade_dn} /Coords [0 0 1 0] \
                 /Function << /FunctionType 2 /Domain [0 1] /N 1 >> >> >> /P2 12 0 R >> \
             /ExtGState << /GS1 << /SMask << /S /Luminosity /G 13 0 R >> >> \
                 /GS2 << /Font [14 0 R 12] >> >> \
             /Font << /T3 15 0 R >>",
            page_sep = separation("PageSep"),
            page_dn = device_n(
                &["PageDN1", "Cyan", "PageDN2", "None"],
                &format!(
                    "<< /Colorants << /PageColorant {} >> >>",
                    separation("PageColorant")
                ),
            ),
            indexed = separation("IndexedBase"),
            patterned = separation("PatternBase"),
            process = separation("Black"),
            all = separation("All"),
            none = separation("None"),
            shading = separation("ShadingSep"),
            shade_dn = device_n(&["PatternShadeDN", "Magenta"], ""),
        ),
        "/Annots [16 0 R 17 0 R]",
        &[
            // 4: reached by nothing.
            separation("Unreachable"),
            // 5, 6, 7: spare.
            "<< >>".to_string(),
            "<< >>".to_string(),
            "<< >>".to_string(),
            // 8: an image in a spot.
            stream(
                &format!(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 \
                     /ColorSpace {}",
                    separation("ImageSep")
                ),
                "\u{0}",
            ),
            // 9: a stencil mask, whose `/ColorSpace` means nothing.
            stream(
                &format!(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ImageMask true \
                     /ColorSpace {}",
                    separation("StencilSep")
                ),
                "\u{0}",
            ),
            // 10: a group form, whose resources name a spot and a form.
            form(
                &format!(
                    "/Group << /S /Transparency /CS {} >>",
                    separation("GroupSep")
                ),
                &format!(
                    "/ColorSpace << /F {} /Again {} >> /XObject << /Fm2 11 0 R >>",
                    separation("FormSep"),
                    separation("PageSep"),
                ),
            ),
            // 11: the form inside it.
            form(
                "",
                &format!("/ColorSpace << /N {} >>", separation("NestedFormSep")),
            ),
            // 12: a tiling pattern.
            tiling,
            // 13: a soft mask's group.
            form(
                "/Group << /S /Transparency /CS /DeviceGray >>",
                &format!("/ColorSpace << /M {} >>", separation("MaskSep")),
            ),
            // 14: a Type 3 font a graphics state names.
            format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
                 /CharProcs << >> /Encoding << /Differences [] >> /FirstChar 0 /LastChar 0 \
                 /Widths [0] /Resources << /ColorSpace << /S {} >> >> >>",
                separation("Type3StateSep")
            ),
            // 15: a Type 3 font `/Font` names.
            format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
                 /CharProcs << >> /Encoding << /Differences [] >> /FirstChar 0 /LastChar 0 \
                 /Widths [0] /Resources << /ColorSpace << /S {} >> >> >>",
                separation("Type3Sep")
            ),
            // 16: an annotation with a normal and a down appearance.
            "<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /N 18 0 R /D 19 0 R >> >>"
                .to_string(),
            // 17: a checkbox, both of whose normal states are drawn by some
            // value of the field.
            "<< /Type /Annot /Subtype /Widget /FT /Btn /Rect [0 0 10 10] /AS /On \
             /AP << /N << /On 20 0 R /Off 21 0 R >> >> >>"
                .to_string(),
            // 18: the normal appearance, a group in a spot.
            form(
                &format!(
                    "/Group << /S /Transparency /CS {} >>",
                    separation("AppearanceGroupSep")
                ),
                &format!("/ColorSpace << /A {} >>", separation("AppearanceSep")),
            ),
            // 19: the down appearance.
            form(
                "",
                &format!("/ColorSpace << /D {} >>", separation("DownSep")),
            ),
            // 20, 21: the checkbox's states.
            form(
                "",
                &format!("/ColorSpace << /On {} >>", separation("OnSep")),
            ),
            form(
                "",
                &format!("/ColorSpace << /Off {} >>", separation("OffSep")),
            ),
        ],
    );
    let doc = document(&objects);
    let page = doc.page(0).expect("a page");
    assert_eq!(
        names(&page),
        [
            // The page's own scope: `/ColorSpace` in its order, a `/DeviceN`'s
            // names and then its `/Colorants`, a base after its space.
            "PageSep",
            "PageDN1",
            "PageDN2",
            "PageColorant",
            "IndexedBase",
            "PatternBase",
            // Its XObjects' spaces: the image's and the form group's.
            "ImageSep",
            "GroupSep",
            // Its shading's, and its shading pattern's.
            "ShadingSep",
            "PatternShadeDN",
            // The scopes it opens: the Type 3 font `/Font` names; the soft
            // mask's group; the Type 3 font the graphics state names; the
            // form, and the form inside it; the tiling pattern's cell.
            "Type3Sep",
            "MaskSep",
            "Type3StateSep",
            "FormSep",
            "NestedFormSep",
            "TilingSep",
            // The annotations: the appearance's group, then its resources,
            // then the checkbox's two states.
            "AppearanceGroupSep",
            "AppearanceSep",
            "OnSep",
            "OffSep",
        ]
    );
}

/// A form whose resources name itself, and a Type 3 font whose glyphs'
/// resources name the font, end at their first repeat with what was read
/// (ruling 1): each spot once.
#[test]
fn a_resource_cycle_ends_with_what_was_read() {
    let objects = one_page(
        "/XObject << /Loop 4 0 R >> /Font << /T3 5 0 R >>",
        "",
        &[
            form(
                "",
                &format!(
                    "/ColorSpace << /C {} >> /XObject << /Again 4 0 R >>",
                    separation("InTheLoop")
                ),
            ),
            format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
                 /CharProcs << >> /Encoding << /Differences [] >> /FirstChar 0 /LastChar 0 \
                 /Widths [0] /Resources << /ColorSpace << /S {} >> /Font << /Me 5 0 R >> >> >>",
                separation("InTheFont")
            ),
        ],
    );
    let doc = document(&objects);
    let page = doc.page(0).expect("a page");
    assert_eq!(names(&page), ["InTheFont", "InTheLoop"]);
}

/// Forms nested forty deep, each naming a spot of its own: the walk stops at
/// its depth bound, and what it lists is exactly the spots of the scopes it
/// reached, outermost first — a prefix of the forty, the deepest absent.
#[test]
fn a_walk_past_its_bound_ends_with_what_was_read() {
    const DEPTH: usize = 40;
    let mut extra = Vec::new();
    for level in 1..=DEPTH {
        let inner = if level < DEPTH {
            format!("/XObject << /Next {} 0 R >>", level + 4)
        } else {
            String::new()
        };
        extra.push(form(
            "",
            &format!(
                "/ColorSpace << /C {} >> {inner}",
                separation(&format!("Level{level}"))
            ),
        ));
    }
    let objects = one_page(
        &format!(
            "/ColorSpace << /C {} >> /XObject << /Next 4 0 R >>",
            separation("Level0")
        ),
        "",
        &extra,
    );
    let doc = document(&objects);
    let listed = names(&doc.page(0).expect("a page"));
    assert!(
        listed.len() > 8,
        "the walk reaches past the shallow scopes: {listed:?}"
    );
    assert!(listed.len() < DEPTH + 1, "and stops before the deepest");
    for (level, name) in listed.iter().enumerate() {
        assert_eq!(*name, format!("Level{level}"), "a prefix, outermost first");
    }
}

/// **Listing adds nothing to `Document::warnings`**, and asking twice is
/// asking once.
///
/// The fixture's every stream a space holds is damaged — an `ICCBased`
/// profile, an `/Indexed` palette and a tint transform, each a `FlateDecode`
/// stream of bytes that are no deflate — so a listing that decoded any of
/// them would say so in the document's warnings. It reads names, and decodes
/// none.
#[test]
fn listing_adds_nothing_to_the_documents_warnings() {
    let broken = |dict: &str| {
        stream(
            &format!("{dict} /Filter /FlateDecode"),
            "not deflate at all",
        )
    };
    let objects = one_page(
        "/ColorSpace << /I [/ICCBased 4 0 R] /P [/Indexed [/Separation /Palette /DeviceGray 6 0 R] 1 5 0 R] \
         /S [/Separation /Tinted /DeviceCMYK 6 0 R] >>",
        "",
        &[
            broken("/N 4"),
            broken(""),
            broken("/FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1 0 1]"),
        ],
    );
    let doc = document(&objects);
    let page = doc.page(0).expect("a page");
    let before = doc.warnings();
    let first = names(&page);
    let second = names(&page);
    assert_eq!(first, ["Palette", "Tinted"]);
    assert_eq!(first, second, "asking twice is asking once");
    assert_eq!(doc.warnings(), before, "listing warned about nothing");
}

/// A page whose resources name no spot — device colours, an ICC space, a
/// palette over RGB — has an empty inventory, and so does a page with no
/// resources at all.
#[test]
fn a_page_with_no_spots_lists_none() {
    let doc = document(&one_page(
        "/ColorSpace << /G /DeviceGray /L [/Indexed /DeviceRGB 0 <000000>] >>",
        "",
        &[],
    ));
    assert!(names(&doc.page(0).expect("a page")).is_empty());

    let doc = document(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_string(),
    ]);
    assert!(names(&doc.page(0).expect("a page")).is_empty());
}

/// The inventory is the page's: a spot only another page's resources name is
/// not on this one, and a form both pages draw lists its spot on each.
#[test]
fn each_page_lists_its_own_spots() {
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
             /Resources << /ColorSpace << /A {} >> /XObject << /F 5 0 R >> >> >>",
            separation("First")
        ),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
             /Resources << /ColorSpace << /B {} >> /XObject << /F 5 0 R >> >> >>",
            separation("Second")
        ),
        form(
            "",
            &format!("/ColorSpace << /S {} >>", separation("Shared")),
        ),
    ];
    let doc = document(&objects);
    assert_eq!(names(&doc.page(0).expect("page one")), ["First", "Shared"]);
    assert_eq!(names(&doc.page(1).expect("page two")), ["Second", "Shared"]);
}
