//! Separations: a page's spot colorants and its plates (8.6.6.4, 8.6.6.5,
//! 10.8; `docs/design/overprint-separations.md`).
//!
//! Every expectation here is read off the fixture as it is written — which
//! space names which colorant, in which scope, at which tint — and never off
//! the code that reads it.

mod render_support;

/// The separations inputs the render group times, written byte by byte: the
/// pages this file holds to their plates where the bench draws them too.
#[allow(dead_code, reason = "the bench reads inputs this file does not")]
#[path = "../benches/support/plates.rs"]
mod plates;

use plates::{
    separations_page, spot_squares, spots_page, spots_page_of, SpotSquare, SEPARATIONS_PAGE,
    SEPARATIONS_SPOTS, SPOTS_BACKDROP, SPOTS_PAGE,
};
use render_support::{spot_page, SpotPage, SpotSpaces, SPOT_TINTS};
use tinker_pdf::{
    Document, Page, PixelRegion, PlateKind, PlateUse, RenderOptions, RenderWarning,
    SeparationOptions, Separations,
};

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

// ---- plates (RC-04.10) ------------------------------------------------------

/// A tint as a plate holds it, `round(255 × t)`.
fn byte(tint: f64) -> u8 {
    (tint.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `page`'s separations under `render`, each colorant `plates` names as it
/// says.
fn separations(page: &Page, render: &RenderOptions, plates: &[(&str, PlateUse)]) -> Separations {
    page.separations(&SeparationOptions {
        render: render.clone(),
        plates: plates
            .iter()
            .map(|(name, behaviour)| (name.as_bytes().to_vec(), *behaviour))
            .collect(),
    })
}

/// The plates' names, in the order they come back.
fn plate_names(separations: &Separations) -> Vec<String> {
    separations
        .plates
        .iter()
        .map(|plate| String::from_utf8_lossy(&plate.name).into_owned())
        .collect()
}

/// The plate of colorant `name`.
fn plate<'a>(separations: &'a Separations, name: &str) -> Option<&'a tinker_pdf::Plate> {
    separations
        .plates
        .iter()
        .find(|plate| plate.name == name.as_bytes())
}

/// The tint the plate of colorant `name` holds at pixel `(x, y)`.
#[track_caller]
fn tint_at(separations: &Separations, name: &str, x: u32, y: u32) -> u8 {
    let plate = plate(separations, name).unwrap_or_else(|| panic!("a plate of {name}"));
    plate.tints[(y * separations.width + x) as usize]
}

/// The process colorants, in their plates' order.
const PROCESS: [&str; 4] = ["Cyan", "Magenta", "Yellow", "Black"];

/// A drawing of every pixel whole or empty, which the plates' sum is exact
/// over (the design's "The plates' sum").
fn whole_pixels() -> RenderOptions {
    RenderOptions {
        antialias: false,
        ..RenderOptions::default()
    }
}

/// A 10-point page whose resources are `resources`, whose page dictionary's
/// other keys are `keys` and whose content is `content`; `extra`'s objects
/// are numbered from 5.
fn painted(resources: &str, keys: &str, content: &str, extra: &[String]) -> Document {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << {resources} >> \
             /Contents 4 0 R {keys} >>"
        ),
        stream("", content),
    ];
    objects.extend_from_slice(extra);
    document(&objects)
}

/// `/Gold`, a `/Separation` whose tint transform is linear onto cyan.
const GOLD: &str = "[/Separation /Gold /DeviceCMYK \
    << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>]";

/// **N spots render N + 4 plates whose sums reproduce the composite**
/// (RC-04.10; the design's "The plates' sum").
///
/// For one, two and eight spots, painted in `/Separation` spaces and in
/// `/DeviceN` ones — beside a process backdrop, a spot over a spot, process
/// ink over both, and text in an embedded face in process ink and in a spot —
/// each pixel drawn whole or empty: the page's separations are the four
/// process plates and a plate for each spot of its inventory, in its order,
/// the kinds said, each spot's plate holding its tint where its square stands
/// alone; and each spot's plate folded through its tint transform — linear
/// from no ink into DeviceCMYK, so its tint lands on each ink its transform
/// reaches at a tint of 1 — and added to the process plates is, byte for
/// byte, the page drawn with every spot composite.
#[test]
fn n_spots_render_n_plus_four_plates_whose_sums_reproduce_the_composite() {
    let render = whole_pixels();
    for spaces in [SpotSpaces::Separation, SpotSpaces::DeviceN] {
        for count in [1usize, 2, 8] {
            let SpotPage { bytes, spots } = spot_page(count, spaces);
            let what = format!("{count} spots in {spaces:?} spaces");
            let doc = Document::open(bytes).expect("it opens");
            let page = doc.page(0).expect("a page");
            let spot_names: Vec<String> = spots.iter().map(|(name, _)| name.clone()).collect();
            assert_eq!(names(&page), spot_names, "{what}: the inventory");

            let plates = separations(&page, &render, &[]);
            assert!(plates.warnings.is_empty(), "{what}: {:?}", plates.warnings);
            assert_eq!((plates.width, plates.height), (60, 40), "{what}");
            let mut expected: Vec<String> = PROCESS.iter().map(|name| name.to_string()).collect();
            expected.extend(spot_names.iter().cloned());
            assert_eq!(
                plate_names(&plates),
                expected,
                "{what}: the plates, in order"
            );
            for (index, plate) in plates.plates.iter().enumerate() {
                let kind = if index < 4 {
                    PlateKind::Process
                } else {
                    PlateKind::Spot
                };
                assert_eq!(plate.kind, kind, "{what}: plate {index}'s kind");
                assert_eq!(plate.tints.len(), 60 * 40, "{what}: plate {index}'s size");
            }
            for (i, name) in spot_names.iter().enumerate() {
                assert_eq!(
                    tint_at(&plates, name, 3 + 7 * i as u32, 35),
                    byte(SPOT_TINTS[i % SPOT_TINTS.len()]),
                    "{what}: {name} where its square stands alone"
                );
            }
            assert!(
                plates.plates[4].tints.contains(&byte(0.8)),
                "{what}: the spot's text reached its plate"
            );
            assert!(
                plates.plates[1].tints.contains(&byte(0.6)),
                "{what}: the process text reached magenta"
            );

            let every: Vec<(&str, PlateUse)> = spot_names
                .iter()
                .map(|name| (name.as_str(), PlateUse::Composite))
                .collect();
            let composite = separations(&page, &render, &every);
            assert_eq!(
                plate_names(&composite),
                PROCESS,
                "{what}: every spot composite"
            );
            for at in 0..60 * 40 {
                for ink in 0..4 {
                    let folded: u32 = spots
                        .iter()
                        .enumerate()
                        .map(|(s, (_, inks))| {
                            u32::from(inks[ink]) * u32::from(plates.plates[4 + s].tints[at])
                        })
                        .sum();
                    let sum = u32::from(plates.plates[ink].tints[at]) + folded;
                    assert_eq!(
                        sum,
                        u32::from(composite.plates[ink].tints[at]),
                        "{what}: {} at pixel ({}, {})",
                        PROCESS[ink],
                        at % 60,
                        at / 60
                    );
                }
            }
        }
    }
}

/// **Each plate behaviour paints as it says** (RC-04.10; the design's "The
/// plate plan"): a spot asked to be `Composite` hands back no plate and lands
/// through its alternate on the process plates, every other plate as it was;
/// a `Disabled` one is drawn on its plate — knocking out and knocked out as
/// any spot is — and left out of what comes back, so every plate handed back
/// is the default render's; `Composite` asked of a process colorant is its
/// plate still, and a disabled process plate is left out; `Spot` asked by
/// name is the default, a name the page has no colorant of is passed over,
/// and a name's first entry decides.
#[test]
fn each_plate_behaviour_paints_as_it_says() {
    let SpotPage { bytes, .. } = spot_page(2, SpotSpaces::Separation);
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let render = whole_pixels();
    let spot = separations(&page, &render, &[]);
    assert_eq!(
        plate_names(&spot),
        ["Cyan", "Magenta", "Yellow", "Black", "Gold", "Silver"]
    );
    // Gold's square alone: x 2 to 8, rows 30 to 36, at 0.2.
    assert_eq!(tint_at(&spot, "Gold", 3, 35), byte(0.2));
    assert_eq!(
        tint_at(&spot, "Cyan", 3, 35),
        0,
        "Gold knocks the process out"
    );

    let composite = separations(&page, &render, &[("Gold", PlateUse::Composite)]);
    assert_eq!(
        plate_names(&composite),
        ["Cyan", "Magenta", "Yellow", "Black", "Silver"]
    );
    assert_eq!(
        tint_at(&composite, "Cyan", 3, 35),
        byte(0.2),
        "Gold through its alternate, onto cyan"
    );
    for name in ["Magenta", "Yellow", "Black", "Silver"] {
        assert_eq!(tint_at(&composite, name, 3, 35), 0, "{name} under Gold");
    }
    assert_eq!(
        plate(&composite, "Silver"),
        plate(&spot, "Silver"),
        "the other spot's plate"
    );

    let disabled = separations(&page, &render, &[("Gold", PlateUse::Disabled)]);
    assert_eq!(
        plate_names(&disabled),
        ["Cyan", "Magenta", "Yellow", "Black", "Silver"]
    );
    for name in ["Cyan", "Magenta", "Yellow", "Black", "Silver"] {
        assert_eq!(
            plate(&disabled, name),
            plate(&spot, name),
            "{name}: a disabled plate still knocks out"
        );
    }

    let cyan = separations(&page, &render, &[("Cyan", PlateUse::Composite)]);
    assert_eq!(
        cyan.plates, spot.plates,
        "a process plate is never composite"
    );

    let no_cyan = separations(&page, &render, &[("Cyan", PlateUse::Disabled)]);
    assert_eq!(
        plate_names(&no_cyan),
        ["Magenta", "Yellow", "Black", "Gold", "Silver"]
    );
    for name in ["Magenta", "Yellow", "Black", "Gold", "Silver"] {
        assert_eq!(plate(&no_cyan, name), plate(&spot, name), "{name}");
    }

    let named = separations(
        &page,
        &render,
        &[
            ("Gold", PlateUse::Spot),
            ("Teal", PlateUse::Composite),
            ("All", PlateUse::Disabled),
        ],
    );
    assert_eq!(
        named.plates, spot.plates,
        "Spot by name, and names not here"
    );
    let first = separations(
        &page,
        &render,
        &[("Gold", PlateUse::Composite), ("Gold", PlateUse::Disabled)],
    );
    assert_eq!(first.plates, composite.plates, "the first entry decides");
}

/// **A page group's space decides where spots land** (RC-04.10; 11.4.7,
/// 11.7.3): a page whose transparency group declares DeviceRGB composites in
/// it, so a spot is its alternate's light there, which comes back as ink on
/// the process plates and no ink on its own; one declaring DeviceCMYK, or no
/// space, is drawn on its plates, the spot on its own.
#[test]
fn a_page_group_of_another_space_draws_its_spots_through_their_alternates() {
    // Gold at 0.6 through its alternate is cyan 0.6, whose light is red
    // 0.4 and white's green and blue, which is cyan 0.6 in ink again.
    for (group, gold, cyan) in [
        ("/Group << /S /Transparency /CS /DeviceRGB >>", 0, byte(0.6)),
        (
            "/Group << /S /Transparency /CS /DeviceCMYK >>",
            byte(0.6),
            0,
        ),
        ("/Group << /S /Transparency >>", byte(0.6), 0),
        ("", byte(0.6), 0),
    ] {
        let doc = painted(
            &format!("/ColorSpace << /G {GOLD} >>"),
            group,
            "/G cs 0.6 scn 0 0 10 10 re f",
            &[],
        );
        let page = doc.page(0).expect("a page");
        let plates = separations(&page, &RenderOptions::default(), &[]);
        assert_eq!(
            plate_names(&plates),
            ["Cyan", "Magenta", "Yellow", "Black", "Gold"],
            "{group}"
        );
        assert_eq!(tint_at(&plates, "Gold", 5, 5), gold, "{group}: Gold");
        assert_eq!(tint_at(&plates, "Cyan", 5, 5), cyan, "{group}: cyan");
        for name in ["Magenta", "Yellow", "Black"] {
            assert_eq!(tint_at(&plates, name, 5, 5), 0, "{group}: {name}");
        }
    }
}

/// **Separations take the render's options as `Page::render` does**
/// (RC-04.10; ruling 5): a region's plates are the page's plates under it,
/// and one past the page's edge is trimmed and named; a scale gives the
/// page's pixel size at it; transparency brings the page's alpha back beside
/// the plates, none where nothing is painted; annotations are drawn on their
/// plates and left out on request, as content is; counting is handed back
/// where asked; and a cancelled render says so.
#[test]
fn separations_take_the_render_s_options() {
    let SpotPage { bytes, .. } = spot_page(2, SpotSpaces::Separation);
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let whole = separations(&page, &RenderOptions::default(), &[]);
    assert!(whole.alpha.is_none(), "no alpha unless asked");
    assert!(whole.stats.is_none(), "no counting unless asked");

    let region = PixelRegion::new(7, 5, 20, 30);
    let tile = separations(
        &page,
        &RenderOptions {
            region: Some(region),
            ..RenderOptions::default()
        },
        &[],
    );
    assert_eq!((tile.width, tile.height), (20, 30));
    assert!(tile.warnings.is_empty(), "{:?}", tile.warnings);
    for (part, all) in tile.plates.iter().zip(&whole.plates) {
        assert_eq!(part.name, all.name);
        for y in 0..30 {
            for x in 0..20 {
                assert_eq!(
                    part.tints[(y * 20 + x) as usize],
                    all.tints[((y + 5) * 60 + x + 7) as usize],
                    "{} at ({x}, {y}) of the region",
                    String::from_utf8_lossy(&part.name)
                );
            }
        }
    }

    let asked = PixelRegion::new(50, 30, 20, 20);
    let past = separations(
        &page,
        &RenderOptions {
            region: Some(asked),
            ..RenderOptions::default()
        },
        &[],
    );
    assert_eq!((past.width, past.height), (10, 10));
    assert!(
        past.warnings.contains(&RenderWarning::RegionClamped {
            requested: asked,
            applied: PixelRegion::new(50, 30, 10, 10),
        }),
        "{:?}",
        past.warnings
    );

    let doubled = RenderOptions {
        scale: 2.0,
        ..RenderOptions::default()
    };
    let double = separations(&page, &doubled, &[]);
    assert_eq!((double.width, double.height), page.pixel_size(&doubled));
    assert_eq!((double.width, double.height), (120, 80));

    let counted = separations(
        &page,
        &RenderOptions {
            stats: true,
            ..RenderOptions::default()
        },
        &[],
    );
    assert!(counted.stats.is_some(), "counted where asked");
    assert_eq!(counted.plates, whole.plates, "counting draws alike");

    let cancel = tinker_pdf::CancelToken::new();
    cancel.cancel();
    let cancelled = separations(
        &page,
        &RenderOptions {
            cancel: Some(cancel),
            ..RenderOptions::default()
        },
        &[],
    );
    assert!(
        cancelled.warnings.contains(&RenderWarning::Cancelled),
        "{:?}",
        cancelled.warnings
    );

    let doc = painted(
        &format!("/ColorSpace << /G {GOLD} >>"),
        "",
        "/G cs 0.6 scn 0 0 5 5 re f",
        &[],
    );
    let page = doc.page(0).expect("a page");
    let transparent = separations(
        &page,
        &RenderOptions {
            transparent: true,
            ..RenderOptions::default()
        },
        &[],
    );
    let alpha = transparent
        .alpha
        .as_ref()
        .expect("an alpha beside the plates");
    assert_eq!(alpha.len(), 100);
    assert_eq!(alpha[9 * 10 + 2], 255, "painted, whole");
    assert_eq!(alpha[9], 0, "unpainted, nothing");
    assert_eq!(tint_at(&transparent, "Gold", 2, 9), byte(0.6));
    assert_eq!(tint_at(&transparent, "Gold", 9, 0), 0);

    let doc = painted(
        "",
        "/Annots [5 0 R]",
        "",
        &[
            "<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /N 6 0 R >> >>".to_string(),
            stream(
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
                     /Resources << /ColorSpace << /G {GOLD} >> >>"
                ),
                "/G cs 1 scn 0 0 10 10 re f",
            ),
        ],
    );
    let page = doc.page(0).expect("a page");
    assert_eq!(
        names(&page),
        ["Gold"],
        "the appearance's spot is the page's"
    );
    let drawn = separations(&page, &RenderOptions::default(), &[]);
    assert_eq!(
        tint_at(&drawn, "Gold", 5, 5),
        255,
        "the annotation on its plate"
    );
    for options in [
        RenderOptions {
            annotations: false,
            ..RenderOptions::default()
        },
        RenderOptions {
            annotations: false,
            content: false,
            ..RenderOptions::default()
        },
    ] {
        let left = separations(&page, &options, &[]);
        assert!(
            left.plates
                .iter()
                .all(|plate| plate.tints.iter().all(|tint| *tint == 0)),
            "nothing drawn, no ink"
        );
    }
}

/// **A page naming no spot has its four process plates**, and a region that
/// misses the page has plates of no pixels.
#[test]
fn a_page_with_no_spots_has_its_process_plates() {
    let doc = painted("", "", "0.2 0.4 0.6 0.8 k 0 0 10 10 re f", &[]);
    let page = doc.page(0).expect("a page");
    let plates = separations(&page, &RenderOptions::default(), &[]);
    assert_eq!(plate_names(&plates), PROCESS);
    for (name, tint) in PROCESS.iter().zip([0.2, 0.4, 0.6, 0.8]) {
        assert_eq!(tint_at(&plates, name, 5, 5), byte(tint), "{name}");
    }
    let off = separations(
        &page,
        &RenderOptions {
            region: Some(PixelRegion::new(40, 40, 5, 5)),
            ..RenderOptions::default()
        },
        &[],
    );
    assert_eq!((off.width, off.height), (0, 0));
    assert!(off.plates.iter().all(|plate| plate.tints.is_empty()));
}

// ---- images on plates (RC-04.12) --------------------------------------------

/// The spaces the image tests draw in, as the page's `/ColorSpace` names
/// them: `/G`, a `/Separation` of `Gold` onto cyan; `/V`, one of `Violet`
/// onto cyan, which the composite case's plan composites; `/DN`, a
/// `/DeviceN` of `Gold` and `Silver` onto cyan and magenta; `/IN`, an
/// `/Indexed` over `/DN` of four entries; `/IK`, an `/Indexed` over
/// DeviceCMYK of three; and `/NO`, `/Separation /None`.
const IMAGE_SPACES: &str = "/ColorSpace << \
    /G [/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>] \
    /V [/Separation /Violet /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>] \
    /DN [/DeviceN [/Gold /Silver] /DeviceCMYK 7 0 R] \
    /IN [/Indexed [/DeviceN [/Gold /Silver] /DeviceCMYK 7 0 R] 3 <10406080 90A0C0F0>] \
    /IK [/Indexed /DeviceCMYK 2 <10203040 50607080 90A0B0C0>] \
    /NO [/Separation /None /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>] >>";

/// `/IN`'s palette, Gold then Silver an entry.
const DN_PALETTE: [[u8; 2]; 4] = [[0x10, 0x40], [0x60, 0x80], [0x90, 0xA0], [0xC0, 0xF0]];

/// `/IK`'s palette, CMYK an entry.
const CMYK_PALETTE: [[u8; 4]; 3] = [
    [0x10, 0x20, 0x30, 0x40],
    [0x50, 0x60, 0x70, 0x80],
    [0x90, 0xA0, 0xB0, 0xC0],
];

/// One image case: its space as an image dictionary writes it, its bits a
/// component, its `/Decode`, how to make sample `i`'s components, the tints a
/// sample's components put on each plate they name — every other plate takes
/// none — and the plan's uses.
struct ImageCase {
    what: &'static str,
    space: &'static str,
    bpc: u32,
    decode: &'static str,
    sample: fn(usize) -> Vec<u8>,
    tints: fn(&[u8]) -> Vec<(&'static str, u8)>,
    plates: &'static [(&'static str, PlateUse)],
}

/// The cases: DeviceCMYK under a `/Decode` that turns cyan over, a
/// `/Separation`, a `/DeviceN`, an `/Indexed` over each kind, a one-bit
/// `/Separation` read inverted, and a `/Separation` of a spot the plan
/// composites, its alternate on the process plates.
fn image_cases() -> Vec<ImageCase> {
    vec![
        ImageCase {
            what: "DeviceCMYK",
            space: "/DeviceCMYK",
            bpc: 8,
            decode: "/Decode [1 0 0 1 0 1 0 1]",
            sample: |i| {
                (0..4)
                    .map(|c| ((i * 37 + c * 91 + 13) % 251) as u8)
                    .collect()
            },
            tints: |s| {
                vec![
                    ("Cyan", 255 - s[0]),
                    ("Magenta", s[1]),
                    ("Yellow", s[2]),
                    ("Black", s[3]),
                ]
            },
            plates: &[],
        },
        ImageCase {
            what: "a /Separation",
            space: "/G",
            bpc: 8,
            decode: "",
            sample: |i| vec![((i * 53 + 7) % 256) as u8],
            tints: |s| vec![("Gold", s[0])],
            plates: &[],
        },
        ImageCase {
            what: "a /DeviceN",
            space: "/DN",
            bpc: 8,
            decode: "",
            sample: |i| vec![((i * 29 + 3) % 256) as u8, ((i * 71 + 5) % 256) as u8],
            tints: |s| vec![("Gold", s[0]), ("Silver", s[1])],
            plates: &[],
        },
        ImageCase {
            what: "an /Indexed over a /DeviceN",
            space: "/IN",
            bpc: 8,
            decode: "",
            sample: |i| vec![(i % 4) as u8],
            tints: |s| {
                let [gold, silver] = DN_PALETTE[usize::from(s[0])];
                vec![("Gold", gold), ("Silver", silver)]
            },
            plates: &[],
        },
        ImageCase {
            what: "an /Indexed over DeviceCMYK",
            space: "/IK",
            bpc: 8,
            decode: "",
            sample: |i| vec![((i * 7) % 3) as u8],
            tints: |s| {
                let [c, m, y, k] = CMYK_PALETTE[usize::from(s[0])];
                vec![("Cyan", c), ("Magenta", m), ("Yellow", y), ("Black", k)]
            },
            plates: &[],
        },
        ImageCase {
            what: "a one-bit /Separation read inverted",
            space: "/G",
            bpc: 1,
            decode: "/Decode [1 0]",
            sample: |i| vec![u8::from((i * 5 + i / 3) % 2 == 1)],
            tints: |s| vec![("Gold", if s[0] == 0 { 255 } else { 0 })],
            plates: &[],
        },
        ImageCase {
            what: "a /Separation of a composited spot",
            space: "/V",
            bpc: 8,
            decode: "",
            sample: |i| vec![((i * 41 + 11) % 256) as u8],
            tints: |s| vec![("Cyan", s[0])],
            plates: &[("Violet", PlateUse::Composite)],
        },
    ]
}

/// `samples`, each a list of components of `bpc` bits, packed as an image
/// stream packs them (8.9.3): rows of `width` samples, each row on a byte.
fn packed(samples: &[Vec<u8>], width: usize, bpc: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for row in samples.chunks(width) {
        let mut bits: Vec<u8> = Vec::new();
        for sample in row {
            for component in sample {
                for bit in (0..bpc).rev() {
                    bits.push((component >> bit) & 1);
                }
            }
        }
        for byte in bits.chunks(8) {
            let mut value = 0u8;
            for (at, bit) in byte.iter().enumerate() {
                value |= bit << (7 - at);
            }
            out.push(value);
        }
    }
    out
}

/// A page of 12 by 8 points over a process backdrop that draws `draw`, an
/// image of `width` by `height` samples in `case`'s space — stored as
/// `/Im`, or inline — over the 4 by 2 points from `(2, 3)`.
fn image_page(case: &ImageCase, width: usize, height: usize, inline: bool) -> Document {
    let samples: Vec<Vec<u8>> = (0..width * height).map(case.sample).collect();
    let data = packed(&samples, width, case.bpc);
    let dict = format!(
        "/Width {width} /Height {height} /BitsPerComponent {} /ColorSpace {} {}",
        case.bpc, case.space, case.decode
    );
    let mut content = b"0.1 0.2 0.3 0.4 k 0 0 12 8 re f q 4 0 0 2 2 3 cm ".to_vec();
    if inline {
        // Hexadecimal, so no run of the samples can read as `EI`.
        let abbreviated = dict
            .replace("/Width", "/W")
            .replace("/Height", "/H")
            .replace("/BitsPerComponent", "/BPC")
            .replace("/ColorSpace", "/CS")
            .replace("/Decode", "/D");
        content.extend_from_slice(
            format!("BI {abbreviated} /F /AHx ID {}> EI ", hex(&data)).as_bytes(),
        );
    } else {
        content.extend_from_slice(b"/Im Do ");
    }
    content.extend_from_slice(b"Q");
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 12 8] \
             /Resources << {IMAGE_SPACES} /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", &content),
        render_support::raw_stream(&format!("/Type /XObject /Subtype /Image {dict}"), &data),
        b"<< >>".to_vec(),
        render_support::raw_stream(
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            b"{ 0 0 }",
        ),
    ];
    Document::open(render_support::raw_pdf(&objects)).expect("it opens")
}

/// **An image in each ink space lands on its plates** (RC-04.12; the
/// design's "Images on plates").
///
/// Stored and inline images in DeviceCMYK under a `/Decode` that turns cyan
/// over, a `/Separation`, a `/DeviceN`, an `/Indexed` over each kind, a
/// one-bit `/Separation` read inverted, and a `/Separation` of a spot the
/// plan composites, drawn at 1:1 and at 2:1 down over a backdrop of process
/// ink: at 1:1 each pixel holds on each plate the tint the test reads from
/// its sample, its `/Decode` and its palette, and no ink on every plate its
/// sample names none for; at 2:1 each holds the area average of the four
/// samples under it, plate by plate, `(Σ t + 2) / 4`; and beside the image
/// the backdrop stays.
#[test]
fn an_image_in_each_ink_space_lands_on_its_plates() {
    for case in image_cases() {
        for inline in [false, true] {
            for (factor, width, height) in [(1usize, 4usize, 2usize), (2, 8, 4)] {
                let doc = image_page(&case, width, height, inline);
                let page = doc.page(0).expect("a page");
                let plates = separations(&page, &RenderOptions::default(), case.plates);
                let what = format!(
                    "{} {} at {factor}:1",
                    if inline { "an inline" } else { "a stored" },
                    case.what
                );
                assert!(plates.warnings.is_empty(), "{what}: {:?}", plates.warnings);
                let samples: Vec<Vec<u8>> = (0..width * height).map(case.sample).collect();
                for py in 0..2usize {
                    for px in 0..4usize {
                        let mut sums: std::collections::BTreeMap<String, u32> =
                            plate_names(&plates)
                                .into_iter()
                                .map(|name| (name, 0))
                                .collect();
                        for dy in 0..factor {
                            for dx in 0..factor {
                                let at = (py * factor + dy) * width + px * factor + dx;
                                for (name, tint) in (case.tints)(&samples[at]) {
                                    *sums.entry(name.to_string()).or_insert(0) += u32::from(tint);
                                }
                            }
                        }
                        let area = (factor * factor) as u32;
                        for (name, sum) in sums {
                            let expected = ((sum + area / 2) / area) as u8;
                            assert_eq!(
                                tint_at(&plates, &name, 2 + px as u32, 3 + py as u32),
                                expected,
                                "{what}: {name} at pixel ({px}, {py}) of the image"
                            );
                        }
                    }
                }
                // Beside the image, the backdrop.
                assert_eq!(
                    tint_at(&plates, "Cyan", 8, 1),
                    byte(0.1),
                    "{what}: the backdrop"
                );
                assert_eq!(
                    tint_at(&plates, "Black", 8, 1),
                    byte(0.4),
                    "{what}: the backdrop"
                );
            }
        }
    }
}

/// **An image naming only `/None` paints nothing, and one of light is its
/// light in ink** (RC-04.12): a stored and an inline image in
/// `/Separation /None` leave every plate as the backdrop left it; an image in
/// DeviceRGB, which names no colorant, puts on the process plates the ink its
/// light is on the page asked for in ink, and no ink on any spot.
#[test]
fn an_image_of_none_or_of_light_reaches_plates_as_a_light_page_has_it() {
    let spaces = format!(
        "/ColorSpace << /G {GOLD} /NO [/Separation /None /DeviceCMYK \
         << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>] >> \
         /XObject << /Im 5 0 R >>"
    );
    let backdrop = "0.1 0.2 0.3 0.4 k 0 0 10 10 re f /G cs 0.5 scn 0 0 1 1 re f";
    let plain = separations(
        &painted_with(
            &spaces,
            backdrop,
            image_of("/NO", &[0x10, 0x80, 0xff, 0x40]),
        )
        .page(0)
        .expect("a page"),
        &RenderOptions::default(),
        &[],
    );
    for draw in [
        "q 4 0 0 4 3 3 cm /Im Do Q",
        "q 4 0 0 4 3 3 cm BI /W 2 /H 2 /BPC 8 /CS /NO /F /AHx ID 1080FF40> EI Q",
    ] {
        let doc = painted_with(
            &spaces,
            &format!("{backdrop} {draw}"),
            image_of("/NO", &[0x10, 0x80, 0xff, 0x40]),
        );
        let page = doc.page(0).expect("a page");
        let none = separations(&page, &RenderOptions::default(), &[]);
        assert_eq!(none.plates, plain.plates, "{draw}: /None paints nothing");
    }

    let rgb = [200u8, 30, 90, 10, 220, 140, 250, 250, 20, 40, 60, 80];
    let doc = painted_with(
        &spaces,
        &format!("{backdrop} q 4 0 0 4 3 3 cm /Im Do Q"),
        image_of("/DeviceRGB", &rgb),
    );
    let page = doc.page(0).expect("a page");
    let plates = separations(&page, &RenderOptions::default(), &[]);
    let ink = page.render(&RenderOptions {
        format: tinker_pdf::PixelFormat::CmykA8,
        allow_cmyk: true,
        ..RenderOptions::default()
    });
    // Gold's own pixel, (0, 9), is on its plate here and in ink there.
    let gold_pixel = 9 * 10;
    for (channel, name) in PROCESS.iter().enumerate() {
        let plate = &plate(&plates, name).expect("a process plate").tints;
        for (at, pixel) in ink.data.chunks_exact(5).enumerate() {
            if at != gold_pixel {
                assert_eq!(plate[at], pixel[channel], "{name} at {at}: the light's ink");
            }
        }
    }
    assert!(
        plate(&plates, "Gold")
            .expect("Gold's plate")
            .tints
            .iter()
            .enumerate()
            .all(|(at, tint)| at == gold_pixel || *tint == 0),
        "no ink on the spot but at its own pixel"
    );
}

/// An image XObject of 2 by 2 eight-bit samples `samples` in `space`.
fn image_of(space: &str, samples: &[u8]) -> Vec<u8> {
    render_support::raw_stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /BitsPerComponent 8 \
             /ColorSpace {space}"
        ),
        samples,
    )
}

/// A 10-point page whose resources are `resources` and content `content`,
/// object 5 `image`, built from bytes.
fn painted_with(resources: &str, content: &str, image: Vec<u8>) -> Document {
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << {resources} >> \
             /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", content.as_bytes()),
        image,
    ];
    Document::open(render_support::raw_pdf(&objects)).expect("it opens")
}

/// `bytes` in hexadecimal, as ASCIIHexDecode reads them.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

// ---- shadings on plates (RC-04.13) ------------------------------------------

/// One shading case: its dictionary — its function object 5 where it is
/// function-based, a `/DeviceN`'s tint transform object 6 — the program of
/// object 5 and how many outputs it has, the plan's uses, and the tint each
/// plate it names takes at a point of the page, from the shading's own
/// function there; every other plate none.
struct ShadeCase {
    what: &'static str,
    dict: String,
    program: &'static str,
    outputs: usize,
    plates: &'static [(&'static str, PlateUse)],
    tints: fn(f64, f64) -> Vec<(&'static str, f64)>,
}

/// An axial shading's parameter along `[0 0 40 0]`, extended.
fn along(x: f64, _y: f64) -> f64 {
    (x / 40.0).clamp(0.0, 1.0)
}

/// A radial shading's parameter between a point at `(20, 10)` and the circle
/// of radius 15 about it, extended.
fn outward(x: f64, y: f64) -> f64 {
    (((x - 20.0).powi(2) + (y - 10.0).powi(2)).sqrt() / 15.0).clamp(0.0, 1.0)
}

/// The cases: types 1, 2 and 3 in a `/Separation`, a two-colorant
/// `/DeviceN` and DeviceCMYK, and a `/DeviceN` naming a spot the plan
/// composites, which reverts to its alternate.
fn shade_cases() -> Vec<ShadeCase> {
    let gold = "[/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
                /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>]";
    let gold_silver = "[/DeviceN [/Gold /Silver] /DeviceCMYK 6 0 R]";
    let gold_teal = "[/DeviceN [/Gold /Teal] /DeviceCMYK 6 0 R]";
    let axial = |space: &str, c0: &str, c1: &str| {
        format!(
            "/ShadingType 2 /ColorSpace {space} /Coords [0 0 40 0] /Extend [true true] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [{c0}] /C1 [{c1}] /N 1 >>"
        )
    };
    let radial = |space: &str, c0: &str, c1: &str| {
        format!(
            "/ShadingType 3 /ColorSpace {space} /Coords [20 10 0 20 10 15] /Extend [true true] \
             /Function << /FunctionType 2 /Domain [0 1] /C0 [{c0}] /C1 [{c1}] /N 1 >>"
        )
    };
    let function_based = |space: &str| {
        format!(
            "/ShadingType 1 /ColorSpace {space} /Domain [0 1 0 1] /Matrix [40 0 0 20 0 0] \
             /Function 5 0 R"
        )
    };
    vec![
        ShadeCase {
            what: "an axial /Separation",
            dict: axial(gold, "0", "1"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| vec![("Gold", along(x, y))],
        },
        ShadeCase {
            what: "an axial /DeviceN",
            dict: axial(gold_silver, "0.2 0.8", "0.9 0.1"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| {
                let t = along(x, y);
                vec![("Gold", 0.2 + 0.7 * t), ("Silver", 0.8 - 0.7 * t)]
            },
        },
        ShadeCase {
            what: "an axial DeviceCMYK",
            dict: axial("/DeviceCMYK", "0 0.2 0.4 0", "1 0.2 0 0.6"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| {
                let t = along(x, y);
                vec![
                    ("Cyan", t),
                    ("Magenta", 0.2),
                    ("Yellow", 0.4 - 0.4 * t),
                    ("Black", 0.6 * t),
                ]
            },
        },
        ShadeCase {
            what: "a radial /Separation",
            dict: radial(gold, "0", "1"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| vec![("Gold", outward(x, y))],
        },
        ShadeCase {
            what: "a radial /DeviceN",
            dict: radial(gold_silver, "0 1", "1 0"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| {
                let t = outward(x, y);
                vec![("Gold", t), ("Silver", 1.0 - t)]
            },
        },
        ShadeCase {
            what: "a radial DeviceCMYK",
            dict: radial("/DeviceCMYK", "0 0 0 0", "1 1 0 0.5"),
            program: "",
            outputs: 1,
            plates: &[],
            tints: |x, y| {
                let t = outward(x, y);
                vec![
                    ("Cyan", t),
                    ("Magenta", t),
                    ("Yellow", 0.0),
                    ("Black", 0.5 * t),
                ]
            },
        },
        ShadeCase {
            what: "a function-based /Separation",
            dict: function_based(gold),
            program: "{ add 2 div }",
            outputs: 1,
            plates: &[],
            tints: |x, y| vec![("Gold", (x / 40.0 + y / 20.0) / 2.0)],
        },
        ShadeCase {
            what: "a function-based /DeviceN",
            dict: function_based(gold_silver),
            program: "{ 0 add }",
            outputs: 2,
            plates: &[],
            tints: |x, y| vec![("Gold", x / 40.0), ("Silver", y / 20.0)],
        },
        ShadeCase {
            what: "a function-based DeviceCMYK",
            dict: function_based("/DeviceCMYK"),
            program: "{ 2 copy add 2 div 0 }",
            outputs: 4,
            plates: &[],
            tints: |x, y| {
                let (u, v) = (x / 40.0, y / 20.0);
                vec![
                    ("Cyan", u),
                    ("Magenta", v),
                    ("Yellow", (u + v) / 2.0),
                    ("Black", 0.0),
                ]
            },
        },
        ShadeCase {
            what: "an axial /DeviceN of a composited spot",
            dict: axial(gold_teal, "0.2 0.8", "0.9 0.1"),
            program: "",
            outputs: 1,
            plates: &[("Teal", PlateUse::Composite)],
            tints: |x, y| {
                let t = along(x, y);
                vec![("Cyan", 0.2 + 0.7 * t), ("Magenta", 0.8 - 0.7 * t)]
            },
        },
    ]
}

/// A page of 40 by 20 points over a process backdrop painting `case`'s
/// shading with `sh` over the whole page, or, `as_pattern`, as a shading
/// pattern filling the page; object 5 is the function-based shading's own
/// type 4 function and object 6 the `/DeviceN`s' tint transform, `{ 0 0 }`,
/// onto cyan and magenta.
fn shade_page(case: &ShadeCase, as_pattern: bool) -> Document {
    let dict = &case.dict;
    let (resources, draw) = if as_pattern {
        (
            format!("/Pattern << /P << /PatternType 2 /Shading << {dict} >> >> >>"),
            "/Pattern cs /P scn 0 0 40 20 re f",
        )
    } else {
        (format!("/Shading << /Sh << {dict} >> >>"), "/Sh sh")
    };
    let content = format!("0.1 0.2 0.3 0.4 k 0 0 40 20 re f {draw}");
    let range = vec!["0 1"; case.outputs].join(" ");
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 20] /Resources << {resources} >> \
             /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", content.as_bytes()),
        render_support::raw_stream(
            &format!("/FunctionType 4 /Domain [0 1 0 1] /Range [{range}]"),
            case.program.as_bytes(),
        ),
        render_support::raw_stream(
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            b"{ 0 0 }",
        ),
    ];
    Document::open(render_support::raw_pdf(&objects)).expect("it opens")
}

/// **A shading in each ink space lands on its plates** (RC-04.13; the
/// design's "Shadings on plates").
///
/// Function-based, axial and radial shadings in a `/Separation`, a
/// two-colorant `/DeviceN` and DeviceCMYK, and an axial one in a `/DeviceN`
/// naming a spot the plan composites, painted by `sh` and as a shading
/// pattern over a backdrop of process ink: at every pixel each plate the
/// shading names holds the tint its own function gives at the pixel's
/// centre, `round(255 × f)`, within the one level of 255 ruling 22 allows a
/// stepped parameter, and every other plate no ink; a spot with no plate
/// reverts the whole colour to its alternate on the process plates.
#[test]
fn a_shading_in_each_ink_space_lands_on_its_plates() {
    for case in shade_cases() {
        for as_pattern in [false, true] {
            let doc = shade_page(&case, as_pattern);
            let page = doc.page(0).expect("a page");
            let plates = separations(&page, &RenderOptions::default(), case.plates);
            let what = format!(
                "{} {}",
                case.what,
                if as_pattern { "as a pattern" } else { "by sh" }
            );
            assert!(plates.warnings.is_empty(), "{what}: {:?}", plates.warnings);
            assert_eq!((plates.width, plates.height), (40, 20), "{what}");
            let names = plate_names(&plates);
            for py in 0..20u32 {
                for px in 0..40u32 {
                    let (x, y) = (f64::from(px) + 0.5, 20.0 - (f64::from(py) + 0.5));
                    let named = (case.tints)(x, y);
                    for name in &names {
                        let expected = named
                            .iter()
                            .find(|(plate, _)| plate == name)
                            .map_or(0, |(_, tint)| byte(*tint));
                        let got = tint_at(&plates, name, px, py);
                        assert!(
                            got.abs_diff(expected) <= 1,
                            "{what}: {name} at ({px}, {py}) is {got}, its function's {expected}"
                        );
                    }
                }
            }
        }
    }
}

/// **A shading in `/None` has no effect on the page** (RC-04.13; 8.6.6.4):
/// by `sh` and as a pattern, every plate is the backdrop's.
#[test]
fn a_shading_in_none_paints_nothing() {
    let none = ShadeCase {
        what: "an axial /None",
        dict: "/ShadingType 2 /ColorSpace [/Separation /None /DeviceCMYK \
               << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>] \
               /Coords [0 0 40 0] /Extend [true true] \
               /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>"
            .to_string(),
        program: "",
        outputs: 1,
        plates: &[],
        tints: |_, _| Vec::new(),
    };
    for as_pattern in [false, true] {
        let doc = shade_page(&none, as_pattern);
        let page = doc.page(0).expect("a page");
        let plates = separations(&page, &RenderOptions::default(), &[]);
        assert_eq!(plate_names(&plates), PROCESS);
        for (name, tint) in PROCESS.iter().zip([0.1, 0.2, 0.3, 0.4]) {
            assert!(
                plate(&plates, name)
                    .expect("a process plate")
                    .tints
                    .iter()
                    .all(|got| *got == byte(tint)),
                "pattern {as_pattern}: {name} is the backdrop's"
            );
        }
    }
}

// ---- meshes on plates (RC-04.14) --------------------------------------------

/// One mesh case: its shading dictionary's keys — its space a `/DeviceN`'s
/// tint transform object 6 where it names one — the bytes of its stream, the
/// plan's uses, and the tint each plate it names takes at a point of the
/// page, interpolated there as 8.7.4.5.5 and 8.7.4.5.7 have it; every other
/// plate none.
struct MeshCase {
    what: &'static str,
    keys: String,
    data: Vec<u8>,
    plates: &'static [(&'static str, PlateUse)],
    tints: Box<dyn Fn(f64, f64) -> Vec<(&'static str, f64)>>,
}

/// The page's corners `(0, 0)`, `(40, 0)`, `(40, 20)` and `(0, 20)`, as a
/// mesh's raw eight-bit coordinates under `/Decode [0 40 0 20 ...]`.
const MESH_CORNERS: [(u8, u8); 4] = [(0, 0), (255, 0), (255, 255), (0, 255)];

/// The page's rectangle as two type 4 triangles over [`MESH_CORNERS`], the
/// first three corners and the first, third and fourth, each corner taking
/// its raw components in `colours`: a stream of eight bits a flag, a
/// coordinate and a component.
fn two_triangles(colours: &[Vec<u8>; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for corner in [0, 1, 2, 0, 2, 3] {
        let (x, y) = MESH_CORNERS[corner];
        data.extend([0, x, y]);
        data.extend(&colours[corner]);
    }
    data
}

/// What [`two_triangles`] paints at `(x, y)`: each of its corners' components,
/// a raw byte over 255, weighted by the point's barycentric weights in the
/// triangle holding it.
fn across_triangles(colours: &[Vec<u8>; 4], x: f64, y: f64) -> Vec<f64> {
    let at = |corner: usize| {
        let (rx, ry) = MESH_CORNERS[corner];
        (f64::from(rx) * 40.0 / 255.0, f64::from(ry) * 20.0 / 255.0)
    };
    for triangle in [[0, 1, 2], [0, 2, 3]] {
        let [a, b, c] = triangle.map(at);
        let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
        let wb = ((x - a.0) * (c.1 - a.1) - (c.0 - a.0) * (y - a.1)) / area;
        let wc = ((b.0 - a.0) * (y - a.1) - (x - a.0) * (b.1 - a.1)) / area;
        let wa = 1.0 - wb - wc;
        if wa >= -1e-9 && wb >= -1e-9 && wc >= -1e-9 {
            return (0..colours[0].len())
                .map(|k| {
                    let [ca, cb, cc] = triangle.map(|corner| f64::from(colours[corner][k]));
                    (wa * ca + wb * cb + wc * cc) / 255.0
                })
                .collect();
        }
    }
    panic!("({x}, {y}) is off the page");
}

/// The page's rectangle as one type 6 patch with straight edges, its control
/// points a third of the way along them, in 8.7.4.5.7's order from `(0, 0)`
/// up the left edge, and its corners `(0, 0)`, `(0, 20)`, `(40, 20)` and
/// `(40, 0)` taking the raw components in `colours`, in that order: a stream
/// of eight bits a flag, a coordinate and a component.
fn one_patch(colours: &[Vec<u8>; 4]) -> Vec<u8> {
    let points: [(u8, u8); 12] = [
        (0, 0),
        (0, 85),
        (0, 170),
        (0, 255),
        (85, 255),
        (170, 255),
        (255, 255),
        (255, 170),
        (255, 85),
        (255, 0),
        (170, 0),
        (85, 0),
    ];
    let mut data = vec![0];
    for (x, y) in points {
        data.extend([x, y]);
    }
    for colour in colours {
        data.extend(colour);
    }
    data
}

/// What [`one_patch`] paints at `(x, y)`: its corners' components, a raw
/// byte over 255, interpolated bilinearly over `u = x / 40` and `v = y / 20`.
/// Each case's corners make that linear — opposite corners summing alike —
/// so the triangles any subdivision of the patch cuts paint it exactly.
fn across_patch(colours: &[Vec<u8>; 4], x: f64, y: f64) -> Vec<f64> {
    let (u, v) = (x / 40.0, y / 20.0);
    let weights = [(1.0 - u) * (1.0 - v), (1.0 - u) * v, u * v, u * (1.0 - v)];
    (0..colours[0].len())
        .map(|k| {
            weights
                .iter()
                .zip(colours)
                .map(|(weight, colour)| weight * f64::from(colour[k]))
                .sum::<f64>()
                / 255.0
        })
        .collect()
}

/// A mesh shading dictionary's keys: its type, its space, eight bits a flag,
/// a coordinate and a component, the page's corners through `/Decode` and
/// `components` inputs of 0 to 1, and `extra`.
fn mesh_keys(kind: u8, space: &str, components: usize, extra: &str) -> String {
    let inputs = vec!["0 1"; components].join(" ");
    format!(
        "/ShadingType {kind} /ColorSpace {space} /BitsPerCoordinate 8 /BitsPerComponent 8 \
         /BitsPerFlag 8 /Decode [0 40 0 20 {inputs}] {extra}"
    )
}

/// The cases: a type 4 and a type 6 mesh in a two-colorant `/DeviceN`, each
/// also through a `/Function` — the type 4 one in a `/Separation` — a type 6
/// one in DeviceCMYK, and a type 4 one in a `/DeviceN` naming a spot the
/// plan composites, which reverts to its alternate.
fn mesh_cases() -> Vec<MeshCase> {
    let gold = "[/Separation /Gold /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
                /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>]";
    let gold_silver = "[/DeviceN [/Gold /Silver] /DeviceCMYK 6 0 R]";
    let gold_teal = "[/DeviceN [/Gold /Teal] /DeviceCMYK 6 0 R]";
    let triangles = [vec![26, 230], vec![204, 77], vec![128, 0], vec![255, 153]];
    let patch = [vec![51, 204], vec![153, 204], vec![255, 0], vec![153, 0]];
    let cmyk = [
        vec![0, 51, 255, 0],
        vec![0, 153, 255, 102],
        vec![255, 153, 0, 102],
        vec![255, 51, 0, 0],
    ];
    let along_triangles = [vec![0], vec![255], vec![128], vec![51]];
    let along_patch = [vec![0], vec![128], vec![255], vec![127]];
    let spotted = triangles.clone();
    vec![
        MeshCase {
            what: "a type 4 /DeviceN",
            keys: mesh_keys(4, gold_silver, 2, ""),
            data: two_triangles(&triangles),
            plates: &[],
            tints: Box::new(move |x, y| {
                let c = across_triangles(&triangles, x, y);
                vec![("Gold", c[0]), ("Silver", c[1])]
            }),
        },
        MeshCase {
            what: "a type 6 /DeviceN",
            keys: mesh_keys(6, gold_silver, 2, ""),
            data: one_patch(&patch),
            plates: &[],
            tints: Box::new(move |x, y| {
                let c = across_patch(&patch, x, y);
                vec![("Gold", c[0]), ("Silver", c[1])]
            }),
        },
        MeshCase {
            what: "a type 4 /Separation through a /Function",
            keys: mesh_keys(
                4,
                gold,
                1,
                "/Function << /FunctionType 2 /Domain [0 1] /C0 [0.1] /C1 [0.9] /N 1 >>",
            ),
            data: two_triangles(&along_triangles),
            plates: &[],
            tints: Box::new(move |x, y| {
                let t = across_triangles(&along_triangles, x, y)[0];
                vec![("Gold", 0.1 + 0.8 * t)]
            }),
        },
        MeshCase {
            what: "a type 6 /DeviceN through a /Function",
            keys: mesh_keys(
                6,
                gold_silver,
                1,
                "/Function << /FunctionType 2 /Domain [0 1] /C0 [0 1] /C1 [1 0] /N 1 >>",
            ),
            data: one_patch(&along_patch),
            plates: &[],
            tints: Box::new(move |x, y| {
                let t = across_patch(&along_patch, x, y)[0];
                vec![("Gold", t), ("Silver", 1.0 - t)]
            }),
        },
        MeshCase {
            what: "a type 6 DeviceCMYK",
            keys: mesh_keys(6, "/DeviceCMYK", 4, ""),
            data: one_patch(&cmyk),
            plates: &[],
            tints: Box::new(move |x, y| {
                let c = across_patch(&cmyk, x, y);
                vec![
                    ("Cyan", c[0]),
                    ("Magenta", c[1]),
                    ("Yellow", c[2]),
                    ("Black", c[3]),
                ]
            }),
        },
        MeshCase {
            what: "a type 4 /DeviceN of a composited spot",
            keys: mesh_keys(4, gold_teal, 2, ""),
            data: two_triangles(&spotted),
            plates: &[("Teal", PlateUse::Composite)],
            tints: Box::new(move |x, y| {
                let c = across_triangles(&spotted, x, y);
                vec![("Cyan", c[0]), ("Magenta", c[1])]
            }),
        },
    ]
}

/// A page of 40 by 20 points over a process backdrop painting `case`'s mesh,
/// object 5, with `sh`, or, `as_pattern`, as a shading pattern filling the
/// page; object 6 is the `/DeviceN`s' tint transform, `{ 0 0 }`, onto cyan
/// and magenta.
fn mesh_page(case: &MeshCase, as_pattern: bool) -> Document {
    let (resources, draw) = if as_pattern {
        (
            "/Pattern << /P << /PatternType 2 /Shading 5 0 R >> >>",
            "/Pattern cs /P scn 0 0 40 20 re f",
        )
    } else {
        ("/Shading << /Sh 5 0 R >>", "/Sh sh")
    };
    let content = format!("0.1 0.2 0.3 0.4 k 0 0 40 20 re f {draw}");
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 20] /Resources << {resources} >> \
             /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", content.as_bytes()),
        render_support::raw_stream(&case.keys, &case.data),
        render_support::raw_stream(
            "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
            b"{ 0 0 }",
        ),
    ];
    Document::open(render_support::raw_pdf(&objects)).expect("it opens")
}

/// **A mesh in each ink space lands on its plates** (RC-04.14; the design's
/// "Shadings on plates").
///
/// A type 4 mesh of two triangles and a type 6 patch over the whole page in
/// a two-colorant `/DeviceN`, each also through a `/Function` (the type 4
/// one in a `/Separation`), a type 6 patch in DeviceCMYK and a type 4 mesh in
/// a `/DeviceN` naming a spot the plan composites, painted by `sh` and as a
/// shading pattern over a backdrop of process ink: at every pixel each plate
/// the mesh names holds the tint the test interpolates at the pixel's centre
/// — the triangles' barycentric weights, the patch's bilinear ones —
/// `round(255 × t)` within a level, and every other plate no ink; a spot with
/// no plate reverts the whole colour to its alternate on the process plates.
#[test]
fn a_mesh_in_each_ink_space_lands_on_its_plates() {
    for case in mesh_cases() {
        for as_pattern in [false, true] {
            let doc = mesh_page(&case, as_pattern);
            let page = doc.page(0).expect("a page");
            let plates = separations(&page, &RenderOptions::default(), case.plates);
            let what = format!(
                "{} {}",
                case.what,
                if as_pattern { "as a pattern" } else { "by sh" }
            );
            assert!(plates.warnings.is_empty(), "{what}: {:?}", plates.warnings);
            assert_eq!((plates.width, plates.height), (40, 20), "{what}");
            let names = plate_names(&plates);
            for py in 0..20u32 {
                for px in 0..40u32 {
                    let (x, y) = (f64::from(px) + 0.5, 20.0 - (f64::from(py) + 0.5));
                    let named = (case.tints)(x, y);
                    for name in &names {
                        let expected = named
                            .iter()
                            .find(|(plate, _)| plate == name)
                            .map_or(0, |(_, tint)| byte(*tint));
                        let got = tint_at(&plates, name, px, py);
                        assert!(
                            got.abs_diff(expected) <= 1,
                            "{what}: {name} at ({px}, {py}) is {got}, interpolated {expected}"
                        );
                    }
                }
            }
        }
    }
}

/// **A mesh in `/None` has no effect on the page** (RC-04.14; 8.6.6.4): a
/// type 4 and a type 6 mesh in a `/Separation /None`, by `sh` and as a
/// pattern, leave every plate the backdrop's.
#[test]
fn a_mesh_in_none_paints_nothing() {
    let none = "[/Separation /None /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
                /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>]";
    let flat = [vec![255], vec![255], vec![255], vec![255]];
    for (keys, data) in [
        (mesh_keys(4, none, 1, ""), two_triangles(&flat)),
        (mesh_keys(6, none, 1, ""), one_patch(&flat)),
    ] {
        let case = MeshCase {
            what: "a /None mesh",
            keys,
            data,
            plates: &[],
            tints: Box::new(|_, _| Vec::new()),
        };
        for as_pattern in [false, true] {
            let doc = mesh_page(&case, as_pattern);
            let page = doc.page(0).expect("a page");
            let plates = separations(&page, &RenderOptions::default(), case.plates);
            let what = format!("{} ({}), pattern {as_pattern}", case.what, case.keys);
            assert_eq!(plate_names(&plates), PROCESS, "{what}");
            for (name, tint) in PROCESS.iter().zip([0.1, 0.2, 0.3, 0.4]) {
                assert!(
                    plate(&plates, name)
                        .expect("a process plate")
                        .tints
                        .iter()
                        .all(|got| *got == byte(tint)),
                    "{what}: {name} is the backdrop's"
                );
            }
        }
    }
}

// ---- the plate cap (RC-04.15) -----------------------------------------------

/// What each plate of a spots page holds over `region` — `[x, y, width,
/// height]` in the page's pixels at 72 dpi, every pixel whole or empty — read
/// off the page as `plates::spots_page_of` writes it: where no square stands,
/// the backdrop's process ink and no spot; inside a square whose space is
/// drawn on plates, its tint on its own plate and none on any other; and
/// inside one whose space names a spot without a plate, so that the whole
/// colour is drawn through its alternate (8.6.6.5), its tint on the process
/// ink its component is carried onto and none on any other plate. `plated`
/// is every spot that keeps a plate while the page is drawn, and `handed`
/// the plates that come back, in their order.
fn spots_expected(
    page: (u32, u32),
    squares: &[SpotSquare],
    plated: &[String],
    handed: &[String],
    region: [u32; 4],
) -> Vec<Vec<u8>> {
    let [left, top, width, height] = region;
    // Which square each pixel of the region stands in: a square's rows run
    // from its top edge, `y1` points up the page, downward.
    let mut owner: Vec<Option<usize>> = vec![None; (width * height) as usize];
    for (index, square) in squares.iter().enumerate() {
        let [x0, y0, x1, y1] = square.rect;
        for row in (page.1 - y1)..(page.1 - y0) {
            for column in x0..x1 {
                if (left..left + width).contains(&column) && (top..top + height).contains(&row) {
                    owner[((row - top) * width + (column - left)) as usize] = Some(index);
                }
            }
        }
    }
    let on_plates: Vec<bool> = squares
        .iter()
        .map(|square| {
            squares
                .iter()
                .filter(|other| other.space == square.space)
                .all(|other| plated.contains(&other.name))
        })
        .collect();
    handed
        .iter()
        .map(|name| {
            let process = PROCESS.iter().position(|ink| ink == name);
            owner
                .iter()
                .map(|at| match (at, process) {
                    (None, Some(ink)) => SPOTS_BACKDROP[ink],
                    (None, None) => 0,
                    (Some(index), process) => {
                        let square = &squares[*index];
                        if on_plates[*index] {
                            if &square.name == name {
                                square.level
                            } else {
                                0
                            }
                        } else if process == Some(square.component) {
                            square.level
                        } else {
                            0
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// Each plate `separations` hands back is `expected`'s, byte for byte, the
/// first pixel that differs named.
#[track_caller]
fn assert_plates(separations: &Separations, expected: &[Vec<u8>], what: &str) {
    assert_eq!(
        separations.plates.len(),
        expected.len(),
        "{what}: the plates"
    );
    for (plate, want) in separations.plates.iter().zip(expected) {
        let name = String::from_utf8_lossy(&plate.name);
        assert_eq!(plate.tints.len(), want.len(), "{what}: {name}'s size");
        if let Some(at) = plate
            .tints
            .iter()
            .zip(want)
            .position(|(got, want)| got != want)
        {
            let pixel = u32::try_from(at).expect("a plate's pixels");
            panic!(
                "{what}: {name} at ({}, {}) holds {}, and the page puts {} there",
                pixel % separations.width,
                pixel / separations.width,
                plate.tints[at],
                want[at]
            );
        }
    }
}

/// The process plates' names, then `spots`: the plates a render hands back
/// that keeps a plate for each of `spots` and disables none of them.
fn process_then(spots: &[String]) -> Vec<String> {
    PROCESS
        .iter()
        .map(|ink| (*ink).to_string())
        .chain(spots.iter().cloned())
        .collect()
}

/// **Sixty-four spots render sixty-eight plates** (RC-04.15; the design's
/// "The cap, and what it costs").
///
/// The render group's 64-spot page: A4, whose sixteen `/DeviceN` spaces name
/// sixty-four distinct spots, each filling a square of its own at a tint of
/// its own over a backdrop of process ink, drawn at 72 dpi with every pixel
/// whole or empty. It hands back, with no warning, the four process plates
/// and one for each of the sixty-four, in inventory order, each of its kind;
/// inside each square its spot's plate holds exactly the square's tint and
/// every other plate — the four process plates and the other sixty-three —
/// holds none; and where no square stands the process plates hold the
/// backdrop and no spot plate holds anything.
#[test]
fn sixty_four_spots_render_sixty_eight_plates() {
    assert_eq!(tinker_pdf::MAX_SPOT_PLATES, 64, "the cap this page reaches");
    let squares = spot_squares(64);
    let spots: Vec<String> = squares.iter().map(|square| square.name.clone()).collect();
    let mut levels: Vec<u8> = squares.iter().map(|square| square.level).collect();
    levels.sort_unstable();
    levels.dedup();
    assert_eq!(levels.len(), 64, "sixty-four tints, one a spot");
    assert!(
        squares
            .iter()
            .all(|square| square.space < 16 && square.component < 4),
        "four spots to each of sixteen spaces"
    );

    let doc = Document::open(spots_page(64)).expect("it opens");
    let page = doc.page(0).expect("a page");
    assert_eq!(
        names(&page),
        spots,
        "the inventory: sixty-four spots, in order"
    );

    let plates = separations(&page, &whole_pixels(), &[]);
    assert!(plates.warnings.is_empty(), "{:?}", plates.warnings);
    assert_eq!((plates.width, plates.height), SPOTS_PAGE);
    let handed = process_then(&spots);
    assert_eq!(handed.len(), 68);
    assert_eq!(plate_names(&plates), handed, "sixty-eight plates, in order");
    for (index, plate) in plates.plates.iter().enumerate() {
        let kind = if index < 4 {
            PlateKind::Process
        } else {
            PlateKind::Spot
        };
        assert_eq!(plate.kind, kind, "plate {index}'s kind");
    }
    let expected = spots_expected(
        SPOTS_PAGE,
        &squares,
        &spots,
        &handed,
        [0, 0, SPOTS_PAGE.0, SPOTS_PAGE.1],
    );
    assert_plates(&plates, &expected, "the 64-spot page");
}

/// `count` spots of `SpotSquare::numbered` in a compact grid at the top left
/// of a page `height` points high: six-point squares two points apart, ten to
/// a row.
fn compact_squares(count: usize, height: u32) -> Vec<SpotSquare> {
    (0..count)
        .map(|index| {
            let column = u32::try_from(index % 10).expect("under ten");
            let row = u32::try_from(index / 10).expect("a few rows");
            let x0 = 2 + 8 * column;
            let top = height - 2 - 8 * row;
            SpotSquare::numbered(index, [x0, top - 6, x0 + 6, top])
        })
        .collect()
}

/// Every warning of `warnings` that says the plates were capped.
fn capped(warnings: &[RenderWarning]) -> Vec<&RenderWarning> {
    warnings
        .iter()
        .filter(|warning| matches!(warning, RenderWarning::PlatesCapped { .. }))
        .collect()
}

/// **A page past the plate cap reports what it left** (RC-04.15; the design's
/// "The cap, and what it costs").
///
/// The spots past a cap, in inventory order, are drawn as composites — so a
/// `/DeviceN` naming one reverts to its alternate as a whole (8.6.6.5), every
/// name of it — and the render carries `RenderWarning::PlatesCapped`, once,
/// naming how many spots asked for a plate and how many kept one; each page's
/// every plate is held to what the page puts there:
///
/// - **past `MAX_SPOT_PLATES`**: seventy spots, four to a space, so the
///   seventeenth and eighteenth spaces hold the six past the sixty-fourth:
///   sixty-eight plates come back, the first sixty-four spots', and the last
///   six squares are drawn on the process plates through their spaces'
///   alternates;
/// - **a spot asked to be composite asks for no plate**, so the cap reaches
///   past it: with the first space's four composite, sixty-six ask and the
///   first sixty-four of them keep one, the fifth to the sixty-eighth;
/// - **a disabled spot keeps its plate while the page is drawn**, so it counts
///   toward the cap: with the first disabled, seventy ask, sixty-four keep a
///   plate and sixty-three come back;
/// - **past `MAX_PLATE_BYTES`, at a page the byte bound reaches first**: a page
///   of `MAX_PAGE_PIXELS`, 8 192 points a side at 72 dpi, whose plates keep
///   sixteen bytes a pixel — the four process plates, the alpha and eleven
///   spots — naming thirteen, drawn as a region at its corner: the first
///   eleven keep plates, and the third space, which names the ninth to the
///   twelfth and so a spot past the bound, reverts as a whole, leaving the
///   plates of the ninth to the eleventh empty;
/// - and a light render of the same page carries no such warning, since it
///   keeps no plates.
#[test]
fn a_page_past_the_plate_cap_reports_what_it_left() {
    let cap = tinker_pdf::MAX_SPOT_PLATES;
    let size = (84, 60);
    let squares = compact_squares(70, size.1);
    let spots: Vec<String> = squares.iter().map(|square| square.name.clone()).collect();
    let doc = Document::open(spots_page_of(size, &squares)).expect("it opens");
    let page = doc.page(0).expect("a page");
    assert_eq!(names(&page), spots, "the inventory lists all seventy");
    let whole = [0, 0, size.0, size.1];

    // Past the count.
    let plates = separations(&page, &whole_pixels(), &[]);
    assert_eq!(
        capped(&plates.warnings),
        [&RenderWarning::PlatesCapped {
            named: 70,
            kept: cap
        }],
        "seventy asked and sixty-four kept, said once"
    );
    assert_eq!(plates.warnings.len(), 1, "{:?}", plates.warnings);
    let plated = spots[..cap].to_vec();
    let handed = process_then(&plated);
    assert_eq!(
        plate_names(&plates),
        handed,
        "the first sixty-four keep their plates"
    );
    assert_plates(
        &plates,
        &spots_expected(size, &squares, &plated, &handed, whole),
        "seventy spots",
    );
    for square in &squares[cap..] {
        let [x0, _, _, y1] = square.rect;
        let (x, y) = (x0 + 1, size.1 - y1 + 1);
        assert_eq!(
            tint_at(&plates, PROCESS[square.component], x, y),
            square.level,
            "{} through its alternate, onto {}",
            square.name,
            PROCESS[square.component]
        );
    }

    // A spot asked to be composite asks for no plate.
    let first_space: Vec<(&str, PlateUse)> = spots[..4]
        .iter()
        .map(|name| (name.as_str(), PlateUse::Composite))
        .collect();
    let composite = separations(&page, &whole_pixels(), &first_space);
    assert_eq!(
        capped(&composite.warnings),
        [&RenderWarning::PlatesCapped {
            named: 66,
            kept: cap
        }],
        "the four composite spots ask for nothing"
    );
    let plated = spots[4..4 + cap].to_vec();
    let handed = process_then(&plated);
    assert_eq!(
        plate_names(&composite),
        handed,
        "the cap reaches past the composite spots"
    );
    assert_plates(
        &composite,
        &spots_expected(size, &squares, &plated, &handed, whole),
        "the first space composite",
    );

    // A disabled spot keeps its plate while the page is drawn.
    let disabled = separations(&page, &whole_pixels(), &[("Spot01", PlateUse::Disabled)]);
    assert_eq!(
        capped(&disabled.warnings),
        [&RenderWarning::PlatesCapped {
            named: 70,
            kept: cap
        }],
        "the disabled spot asks for its plate"
    );
    let plated = spots[..cap].to_vec();
    let handed = process_then(&spots[1..cap]);
    assert_eq!(plate_names(&disabled), handed, "sixty-three come back");
    assert_plates(
        &disabled,
        &spots_expected(size, &squares, &plated, &handed, whole),
        "the first spot disabled",
    );

    assert!(
        capped(&page.render(&RenderOptions::default()).warnings).is_empty(),
        "a light render keeps no plates"
    );

    // Past the bytes, at a page the byte bound reaches first.
    let side = 8192u32;
    let pixels = u64::from(side) * u64::from(side);
    let kept = usize::try_from(tinker_pdf::MAX_PLATE_BYTES / pixels - 5).expect("a few");
    assert_eq!(
        kept, 11,
        "sixteen bytes a pixel: the alpha, four inks and eleven spots"
    );
    let squares = compact_squares(13, side);
    let spots: Vec<String> = squares.iter().map(|square| square.name.clone()).collect();
    let doc = Document::open(spots_page_of((side, side), &squares)).expect("it opens");
    let page = doc.page(0).expect("a page");
    assert_eq!(
        page.pixel_size(&RenderOptions::default()),
        (side, side),
        "drawn at its size, not scaled to fit"
    );
    let region = [0, 0, 84, 20];
    let render = RenderOptions {
        region: Some(PixelRegion::new(region[0], region[1], region[2], region[3])),
        ..whole_pixels()
    };
    let bytes = separations(&page, &render, &[]);
    assert_eq!((bytes.width, bytes.height), (84, 20));
    assert_eq!(
        capped(&bytes.warnings),
        [&RenderWarning::PlatesCapped { named: 13, kept }],
        "thirteen asked and eleven kept, said once"
    );
    assert_eq!(bytes.warnings.len(), 1, "{:?}", bytes.warnings);
    let plated = spots[..kept].to_vec();
    let handed = process_then(&plated);
    assert_eq!(
        plate_names(&bytes),
        handed,
        "the first eleven keep their plates"
    );
    assert_plates(
        &bytes,
        &spots_expected((side, side), &squares, &plated, &handed, region),
        "past the byte bound",
    );
    for name in ["Spot09", "Spot10", "Spot11"] {
        assert!(
            plate(&bytes, name)
                .expect("a plate")
                .tints
                .iter()
                .all(|tint| *tint == 0),
            "{name} keeps a plate its square, drawn through its space's alternate, leaves empty"
        );
    }
}

// ---- fingerprints (RC-04.16) ------------------------------------------------

/// The separations fixture's plates at 72 dpi, in the order they come back,
/// each the SHA-256 of the plates' width and height, big-endian, and the
/// plate's tints — `determinism.rs`'s hash of a render, over one plate. A
/// value is the one the first target to run this gives, held on every other:
/// a hash two targets disagree on is a determinism bug, never a table edit.
const SEPARATION_FINGERPRINTS: [(&str, &str); 8] = [
    ("Cyan", ""),
    ("Magenta", ""),
    ("Yellow", ""),
    ("Black", ""),
    ("Gold", ""),
    ("Silver", ""),
    ("Teal", ""),
    ("Plum", ""),
];

/// The least ink each plate of the separations fixture holds at 72 dpi, in
/// pixels, in [`SEPARATION_FINGERPRINTS`]' order: the process plates' backdrop
/// stands under most of the page and yellow has the DeviceRGB fill and group,
/// while each spot covers several tens of thousands of pixels between its
/// fills, its text, its group, its image or stencil, the ramp and the mesh,
/// so a fixture that loses a road to the plates lands under its floor.
const SEPARATION_INK: [usize; 8] = [
    100_000, 100_000, 5_000, 100_000, 20_000, 20_000, 20_000, 20_000,
];

/// **Separations are stable across targets** (RC-04.16; ruling 4).
///
/// The separations fixture (`plates::separations_page`, which the render
/// group times too) puts four spots on their plates by every road a
/// separations render takes: fills and a stroke, text in an embedded face, a
/// knockout group under a blend mode and an alpha, a group of DeviceRGB, an
/// image of two spots and a stencil, an axial ramp and a mesh. Drawn at 72
/// dpi with the default options, it hands back, with no warning and at the
/// page's size, the four process plates and the four spots', each holding at
/// least its floor of ink; and each plate's hash, its dimensions in it, is
/// its committed value. The face is embedded, so one table serves the
/// default and the `bundled-fonts` builds alike, and nothing is read at run
/// time, so `wasm32-wasip1` runs it with no directory. A mismatch prints
/// every moved line, ready to paste.
#[test]
fn separations_are_stable_across_targets() {
    let doc = Document::open(separations_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    assert_eq!(names(&page), SEPARATIONS_SPOTS, "the fixture's spots");

    let plates = separations(&page, &RenderOptions::default(), &[]);
    assert!(
        plates.warnings.is_empty(),
        "the fixture draws cleanly, its face read: {:?}",
        plates.warnings
    );
    assert_eq!(
        (plates.width, plates.height),
        SEPARATIONS_PAGE,
        "the page at 72 dpi"
    );
    let pinned: Vec<&str> = SEPARATION_FINGERPRINTS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        plate_names(&plates),
        pinned,
        "every plate has a pinned value, or the test passes by not looking"
    );

    let mut moved = Vec::new();
    for ((plate, (name, want)), floor) in plates
        .plates
        .iter()
        .zip(SEPARATION_FINGERPRINTS)
        .zip(SEPARATION_INK)
    {
        let inked = plate.tints.iter().filter(|tint| **tint != 0).count();
        assert!(
            inked >= floor,
            "{name} holds ink at {inked} pixels, fewer than its floor of {floor}: the \
             fixture measures less than it claims, and its hash is evidence of nothing \
             until that is fixed"
        );
        let mut input = Vec::with_capacity(plate.tints.len() + 8);
        input.extend_from_slice(&plates.width.to_be_bytes());
        input.extend_from_slice(&plates.height.to_be_bytes());
        input.extend_from_slice(&plate.tints);
        let actual: String = tinker_pdf_crypto::sha2::sha256(&input)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != want {
            moved.push(format!("    (\"{name}\", \"{actual}\"),"));
        }
    }
    assert!(
        moved.is_empty(),
        "{} of the separations fixture's {} plates do not match their committed \
         fingerprints.\n\n\
         If two targets disagree, this is a determinism bug: find the arithmetic that is \
         not target-stable and do not update the table.\n\
         If the table is being recorded, or this is a deliberate change said in its \
         commit, paste:\n\n{}\n",
        moved.len(),
        SEPARATION_FINGERPRINTS.len(),
        moved.join("\n")
    );
    println!(
        "separations fingerprints: RAN, {} plates held to their pins",
        plates.plates.len()
    );
}
