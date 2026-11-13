//! Separations: a page's spot colorants and its plates (8.6.6.4, 8.6.6.5,
//! 10.8; `docs/design/overprint-separations.md`).
//!
//! Every expectation here is read off the fixture as it is written — which
//! space names which colorant, in which scope, at which tint — and never off
//! the code that reads it.

mod render_support;

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
