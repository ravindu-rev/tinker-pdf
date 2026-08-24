//! What the writer put in the file, read back out of it.
//!
//! This replaces the value half of `qpdf_oracle.rs`. The structural half — is
//! this a well-formed PDF, do the hint tables describe the file — is
//! `strict_validator.rs`; this one asks the other question the oracle asked:
//! **are the dictionaries the ones the caller asked for**.
//!
//! # Why these entries and not others
//!
//! Every entry read below is one this crate's own *typed* readers supply a
//! default for. `read_shading` defaults a missing `/Extend`, `parse_function`
//! defaults a missing `/Domain`, the tiling reader falls back to the cell's own
//! size for a `/XStep` it cannot find, and 11.6.6 defaults `/I` and `/K` to
//! false. A shading written with the wrong key names round-trips through those
//! readers perfectly and is an empty dictionary to anybody else — so nothing
//! here goes through them. Every value is read out of a [`Dict`] literally.
//!
//! **What is lost with the oracle**: it was a reader nobody here wrote, so its
//! acceptance was evidence about the world. This is this project's own parser
//! reading this project's own writer. `docs/verification.md` names that gap in
//! its own voice; nothing in this file closes it.

mod surface_support;

use std::sync::Arc;

use surface_support::whole_surface_document;
use tinker_pdf_cos::dest::DestKind;
use tinker_pdf_cos::{
    CosDocument, Dict, DocumentBuilder, DocumentEditor, Encryption, Name, ObjRef, Object,
    OutlineEntry, Target, WriteMode, WriteOptions,
};

// ---- reading a document the way an outside reader would have to -------------

/// Every page object, walked from the catalog's own `/Kids`.
///
/// Not `pages::collect`: that repairs a broken tree, assumes US Letter for a
/// missing box and cuts a cycle, which are three things a test about the
/// *writer* must not be given for free.
fn pages(doc: &CosDocument) -> Vec<(ObjRef, Dict)> {
    let mut out = Vec::new();
    let Some(catalog) = doc.catalog() else {
        return out;
    };
    if let Some(root) = catalog.get_ref(Name::PAGES) {
        walk(doc, root, 0, &mut out);
    }
    out
}

fn walk(doc: &CosDocument, node: ObjRef, depth: u32, out: &mut Vec<(ObjRef, Dict)>) {
    if depth > 32 {
        return;
    }
    let Ok(object) = doc.get(node) else {
        return;
    };
    let Some(dict) = object.as_dict() else {
        return;
    };
    if dict.get_name(Name::TYPE) == Some(doc.intern(b"Page")) {
        out.push((node, dict.clone()));
        return;
    }
    let Some(kids) = dict.get_array(Name::KIDS).map(<[Object]>::to_vec) else {
        return;
    };
    for kid in kids {
        if let Some(kid) = kid.as_objref() {
            walk(doc, kid, depth + 1, out);
        }
    }
}

fn value(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Arc<Object> {
    doc.resolve_key(dict, doc.intern(key))
}

fn name_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    dict.get_name(doc.intern(key))
        .and_then(|n| doc.name_bytes(n))
        .map(|bytes| bytes.to_vec())
}

fn numbers(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<Vec<f64>> {
    value(doc, dict, key)
        .as_array()?
        .iter()
        .map(Object::as_number)
        .collect()
}

/// One entry of one resource category of the first page, resolved.
fn resource(doc: &CosDocument, page: &Dict, category: &[u8], entry: &[u8]) -> Arc<Object> {
    let resources = value(doc, page, b"Resources");
    let resources = resources.as_dict().expect("the page has resources");
    let group = value(doc, resources, category);
    let group = group.as_dict().unwrap_or_else(|| {
        panic!(
            "the page has no {} resources",
            String::from_utf8_lossy(category)
        )
    });
    doc.resolve_key(group, doc.intern(entry))
}

fn surface() -> (CosDocument, Dict) {
    let doc = CosDocument::open(whole_surface_document()).expect("it opens");
    let page = pages(&doc)
        .into_iter()
        .next()
        .map(|(_, page)| page)
        .expect("one page");
    (doc, page)
}

// ---- the graphics state, the groups and the masks ---------------------------

/// 11.6.4.4 and 11.3.5: three parameters in one dictionary, and `/ca` against
/// `/CA` is a distinction a case-insensitive reader would lose.
#[test]
fn the_graphics_state_carries_both_alphas_and_its_blend_mode() {
    let (doc, page) = surface();
    let state = resource(&doc, &page, b"ExtGState", b"GHalf");
    let state = state.as_dict().expect("a state dictionary").clone();

    assert_eq!(
        name_of(&doc, &state, b"BM").as_deref(),
        Some(&b"Multiply"[..])
    );
    assert_eq!(value(&doc, &state, b"ca").as_number(), Some(0.5));
    assert_eq!(value(&doc, &state, b"CA").as_number(), Some(0.25));
}

/// 11.6.5.2: a soft mask names its own kind and the group it measures, and
/// `/SMask /None` is how a state turns one off.
#[test]
fn a_soft_mask_names_its_kind_its_group_and_its_backdrop() {
    let (doc, page) = surface();
    let masked = resource(&doc, &page, b"ExtGState", b"GMask");
    let masked = masked.as_dict().expect("a state dictionary").clone();
    let mask = value(&doc, &masked, b"SMask");
    let mask = mask.as_dict().expect("a mask dictionary").clone();

    assert_eq!(
        name_of(&doc, &mask, b"S").as_deref(),
        Some(&b"Luminosity"[..])
    );
    assert_eq!(
        numbers(&doc, &mask, b"BC"),
        Some(vec![0.0]),
        "the backdrop it was given"
    );
    assert!(
        value(&doc, &mask, b"G").as_dict().is_some(),
        "and the form it measures"
    );

    let off = resource(&doc, &page, b"ExtGState", b"GOff");
    let off = off.as_dict().expect("a state dictionary").clone();
    assert_eq!(
        name_of(&doc, &off, b"SMask").as_deref(),
        Some(&b"None"[..]),
        "the one that turns a mask off"
    );
}

/// 11.6.6: two flags, written only when true, and this document sets one
/// group's isolation and the other's knockout so neither can stand in for the
/// other.
#[test]
fn each_transparency_group_keeps_its_own_flags_and_colour_space() {
    let (doc, page) = surface();

    let luminosity = resource(&doc, &page, b"XObject", b"FmMask");
    let luminosity = luminosity.as_dict().expect("a form").clone();
    let group = value(&doc, &luminosity, b"Group");
    let group = group.as_dict().expect("a group dictionary").clone();
    assert_eq!(
        name_of(&doc, &group, b"S").as_deref(),
        Some(&b"Transparency"[..])
    );
    assert_eq!(
        name_of(&doc, &group, b"CS").as_deref(),
        Some(&b"DeviceGray"[..])
    );
    assert_eq!(value(&doc, &group, b"I").as_bool(), Some(true));
    assert!(
        value(&doc, &group, b"K").as_bool().is_none(),
        "a flag that is false is not written"
    );

    let knockout = resource(&doc, &page, b"XObject", b"FmKnock");
    let knockout = knockout.as_dict().expect("a form").clone();
    let group = value(&doc, &knockout, b"Group");
    let group = group.as_dict().expect("a group dictionary").clone();
    assert_eq!(
        name_of(&doc, &group, b"CS").as_deref(),
        Some(&b"DeviceRGB"[..])
    );
    assert_eq!(value(&doc, &group, b"K").as_bool(), Some(true));
    assert!(value(&doc, &group, b"I").as_bool().is_none());
}

// ---- the gradients ----------------------------------------------------------

/// 8.7.4.5.3 and 8.7.4.5.4: four coordinates against six, and each shading
/// keeps its own `/Extend` pair — the entry whose absence this crate's reader
/// supplies for itself.
#[test]
fn the_two_shadings_keep_their_coordinates_and_their_extends() {
    let (doc, page) = surface();

    let axial = resource(&doc, &page, b"Shading", b"ShAxial");
    let axial = axial.as_dict().expect("a shading").clone();
    assert_eq!(value(&doc, &axial, b"ShadingType").as_int(), Some(2));
    assert_eq!(
        numbers(&doc, &axial, b"Coords"),
        Some(vec![0.0, 0.0, 300.0, 0.0])
    );
    let extend = value(&doc, &axial, b"Extend");
    let extend: Vec<bool> = extend
        .as_array()
        .expect("an /Extend pair")
        .iter()
        .filter_map(Object::as_bool)
        .collect();
    assert_eq!(extend, vec![true, true]);

    let radial = resource(&doc, &page, b"Shading", b"ShRadial");
    let radial = radial.as_dict().expect("a shading").clone();
    assert_eq!(value(&doc, &radial, b"ShadingType").as_int(), Some(3));
    assert_eq!(
        numbers(&doc, &radial, b"Coords"),
        Some(vec![150.0, 100.0, 0.0, 150.0, 100.0, 90.0])
    );
    let extend = value(&doc, &radial, b"Extend");
    let extend: Vec<bool> = extend
        .as_array()
        .expect("an /Extend pair")
        .iter()
        .filter_map(Object::as_bool)
        .collect();
    assert_eq!(extend, vec![false, true]);
}

/// 7.10.3 and 7.10.4: the stitch, and the two ramps it stitches, each with its
/// own exponent.
#[test]
fn the_stitching_function_carries_its_bounds_its_encode_and_both_ramps() {
    let (doc, page) = surface();
    let radial = resource(&doc, &page, b"Shading", b"ShRadial");
    let radial = radial.as_dict().expect("a shading").clone();
    let stitch = value(&doc, &radial, b"Function");
    let stitch = stitch.as_dict().expect("a function").clone();

    assert_eq!(value(&doc, &stitch, b"FunctionType").as_int(), Some(3));
    assert_eq!(numbers(&doc, &stitch, b"Bounds"), Some(vec![0.35]));
    assert_eq!(
        numbers(&doc, &stitch, b"Encode"),
        Some(vec![0.0, 1.0, 0.0, 1.0])
    );
    assert_eq!(numbers(&doc, &stitch, b"Domain"), Some(vec![0.0, 1.0]));

    let sub = value(&doc, &stitch, b"Functions");
    let sub = sub.as_array().expect("two sub-functions").to_vec();
    assert_eq!(sub.len(), 2);
    let ramps: Vec<(Vec<f64>, Vec<f64>, f64)> = sub
        .iter()
        .map(|entry| {
            let resolved = doc.resolve(entry);
            let ramp = resolved.as_dict().expect("a sub-function").clone();
            assert_eq!(value(&doc, &ramp, b"FunctionType").as_int(), Some(2));
            (
                numbers(&doc, &ramp, b"C0").expect("a start colour"),
                numbers(&doc, &ramp, b"C1").expect("an end colour"),
                value(&doc, &ramp, b"N").as_number().expect("an exponent"),
            )
        })
        .collect();
    assert_eq!(
        ramps,
        vec![
            (vec![1.0, 1.0, 0.0], vec![0.0, 1.0, 0.0], 1.0),
            (vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0], 2.0),
        ]
    );
}

/// 8.7.3.1: the two steps differ from each other and from the cell, which is
/// what makes them readable at all — the reader falls back to the cell's own
/// size for a step it cannot find.
#[test]
fn the_tiling_pattern_keeps_its_cell_its_steps_and_its_matrix() {
    let (doc, page) = surface();
    let pattern = resource(&doc, &page, b"Pattern", b"P0");
    let pattern = pattern.as_dict().expect("a pattern").clone();

    assert_eq!(value(&doc, &pattern, b"PatternType").as_int(), Some(1));
    assert_eq!(value(&doc, &pattern, b"PaintType").as_int(), Some(1));
    assert_eq!(value(&doc, &pattern, b"TilingType").as_int(), Some(2));
    assert_eq!(value(&doc, &pattern, b"XStep").as_number(), Some(12.0));
    assert_eq!(value(&doc, &pattern, b"YStep").as_number(), Some(14.0));
    assert_eq!(
        numbers(&doc, &pattern, b"BBox"),
        Some(vec![0.0, 0.0, 10.0, 10.0])
    );
    assert_eq!(
        numbers(&doc, &pattern, b"Matrix"),
        Some(vec![1.0, 0.0, 0.0, 1.0, 3.0, 5.0])
    );
}

// ---- the composite font -----------------------------------------------------

/// 9.7: the three entries that make a glyph index addressable, and the widths
/// that are the font program's own `hmtx`.
#[test]
fn the_composite_font_is_identity_h_over_a_cid_font() {
    let (doc, page) = surface();
    let font = resource(&doc, &page, b"Font", b"C0");
    let font = font.as_dict().expect("a font").clone();

    assert_eq!(
        name_of(&doc, &font, b"Subtype").as_deref(),
        Some(&b"Type0"[..])
    );
    assert_eq!(
        name_of(&doc, &font, b"Encoding").as_deref(),
        Some(&b"Identity-H"[..]),
        "the code is the CID"
    );

    let descendants = value(&doc, &font, b"DescendantFonts");
    let descendants = descendants.as_array().expect("one descendant").to_vec();
    assert_eq!(descendants.len(), 1);
    let descendant = doc.resolve(&descendants[0]);
    let descendant = descendant.as_dict().expect("a CID font").clone();

    assert_eq!(
        name_of(&doc, &descendant, b"Subtype").as_deref(),
        Some(&b"CIDFontType2"[..])
    );
    assert_eq!(
        name_of(&doc, &descendant, b"CIDToGIDMap").as_deref(),
        Some(&b"Identity"[..]),
        "and the CID is the glyph index"
    );
    assert_eq!(value(&doc, &descendant, b"DW").as_int(), Some(1000));

    let info = value(&doc, &descendant, b"CIDSystemInfo");
    let info = info.as_dict().expect("9.7.3's ordering").clone();
    let text = |key: &[u8]| {
        info.get_string(doc.intern(key))
            .map(|s| s.bytes.clone())
            .unwrap_or_default()
    };
    assert_eq!(text(b"Registry"), b"Adobe".to_vec());
    assert_eq!(
        text(b"Ordering"),
        b"Identity".to_vec(),
        "the descendant's ordering agrees with the encoding"
    );
    assert_eq!(value(&doc, &info, b"Supplement").as_int(), Some(0));

    // 9.7.4.3: 700, 800 and 900 font units at 1 000 per em, run together
    // because the glyphs the page drew are consecutive.
    let widths = value(&doc, &descendant, b"W");
    let widths = widths.as_array().expect("a /W array").to_vec();
    assert_eq!(widths.len(), 2, "one run: a first CID and its widths");
    assert_eq!(widths[0].as_int(), Some(1));
    let run: Vec<f64> = doc
        .resolve(&widths[1])
        .as_array()
        .expect("the run's widths")
        .iter()
        .filter_map(Object::as_number)
        .collect();
    assert_eq!(run, vec![700.0, 800.0, 900.0], "the font's own hmtx");

    // 9.8.1: symbolic, since there is no encoding to look a code up in.
    let descriptor = value(&doc, &descendant, b"FontDescriptor");
    let descriptor = descriptor.as_dict().expect("a descriptor").clone();
    assert_eq!(value(&doc, &descriptor, b"Flags").as_int(), Some(4));
}

/// 9.10.3: the mapping a simple font cannot hold — two glyphs standing for one
/// character, and one standing for three — read out of the CMap's own text.
#[test]
fn the_to_unicode_cmap_maps_many_glyphs_to_one_character_and_back() {
    let (doc, page) = surface();
    let font = resource(&doc, &page, b"Font", b"C0");
    let font = font.as_dict().expect("a font").clone();
    let map = font
        .get_ref(doc.intern(b"ToUnicode"))
        .expect("a /ToUnicode stream");
    let cmap = doc.stream_decoded(map).expect("it decodes");
    let cmap = String::from_utf8_lossy(&cmap).into_owned();

    assert!(cmap.contains("/CMapType 2"), "{cmap}");
    assert!(
        cmap.contains("<0000> <FFFF>"),
        "the codespace is two bytes wide: {cmap}"
    );
    assert!(
        cmap.contains("3 beginbfchar"),
        "one entry per glyph the page drew: {cmap}"
    );
    assert!(
        cmap.contains("<0001> <0066>") && cmap.contains("<0002> <0066>"),
        "many glyphs to one character: {cmap}"
    );
    assert!(
        cmap.contains("<0003> <006600660069>"),
        "and one glyph to many: {cmap}"
    );
}

// ---- links and the outline --------------------------------------------------

/// A document carrying both link annotations and an outline.
fn navigation() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for _ in 0..3 {
        builder.add_page(200.0, 300.0, |_| {});
    }
    builder.add_page(200.0, 300.0, |page| {
        assert!(page.link(
            10.0,
            20.0,
            90.0,
            40.0,
            &Target::Page {
                index: 2,
                view: DestKind::Fit,
            }
        ));
        assert!(page.link(
            10.0,
            50.0,
            90.0,
            70.0,
            &Target::Uri("https://example.org/".to_string())
        ));
    });
    assert!(builder.set_outline(vec![
        OutlineEntry {
            title: "Open".to_string(),
            target: None,
            open: true,
            children: vec![OutlineEntry {
                title: "Leaf".to_string(),
                target: Some(Target::Page {
                    index: 1,
                    view: DestKind::Fit,
                }),
                open: true,
                children: Vec::new(),
            }],
        },
        OutlineEntry {
            title: "Closed".to_string(),
            target: None,
            open: false,
            children: vec![OutlineEntry {
                title: "Hidden".to_string(),
                target: Some(Target::Page {
                    index: 2,
                    view: DestKind::Fit,
                }),
                open: true,
                children: Vec::new(),
            }],
        },
    ]));
    builder.finish()
}

/// 12.5.6.5: two link actions, which are two different dictionaries and not
/// two spellings of one.
#[test]
fn the_page_carries_a_destination_link_and_a_uri_link() {
    let doc = CosDocument::open(navigation()).expect("it opens");
    let (_, page) = pages(&doc).into_iter().nth(3).expect("the fourth page");

    let annots = value(&doc, &page, b"Annots");
    let annots = annots.as_array().expect("the page carries them").to_vec();
    assert_eq!(annots.len(), 2);

    let mut destinations = 0usize;
    let mut uris = 0usize;
    for entry in annots {
        let resolved = doc.resolve(&entry);
        let annot = resolved.as_dict().expect("an annotation").clone();
        assert_eq!(
            name_of(&doc, &annot, b"Subtype").as_deref(),
            Some(&b"Link"[..])
        );
        // 12.5.2 Table 164: three zeroes is a border a viewer draws nothing
        // for, and its absence is a border every viewer draws.
        assert_eq!(
            numbers(&doc, &annot, b"Border"),
            Some(vec![0.0, 0.0, 0.0]),
            "with no visible border"
        );
        if annot.contains_key(doc.intern(b"Dest")) {
            destinations += 1;
        }
        if let Some(action) = value(&doc, &annot, b"A").as_dict() {
            let uri = action
                .get_string(doc.intern(b"URI"))
                .map(|s| s.bytes.clone())
                .unwrap_or_default();
            assert_eq!(uri, b"https://example.org/".to_vec());
            uris += 1;
        }
    }
    assert_eq!((destinations, uris), (1, 1), "one by /Dest, one by a /URI");
}

/// 12.3.3: the sibling chain runs **both** ways, and the counts state what an
/// entry exposes with their sign.
///
/// The `/Prev` half is the fault an outside reader caught and no test here
/// could: this crate's own reader walks `/Next` forward, which is enough to
/// build the tree, so deleting every `/Prev` survived every round trip.
#[test]
fn the_outline_links_forward_and_back_and_states_its_counts() {
    let doc = CosDocument::open(navigation()).expect("it opens");
    let catalog = doc.catalog().expect("a catalog");
    let root = catalog
        .get_ref(doc.intern(b"Outlines"))
        .expect("an outline root");
    let root_dict = doc.get(root).expect("it resolves");
    let root_dict = root_dict.as_dict().expect("a dictionary").clone();

    // 12.3.3: the root states every visible item in the whole tree — three
    // here, since `Hidden` sits under a closed parent and is not one of them.
    assert_eq!(value(&doc, &root_dict, b"Count").as_int(), Some(3));

    let first = root_dict.get_ref(Name::FIRST).expect("a first child");
    let first_dict = doc.get(first).expect("it resolves");
    let first_dict = first_dict.as_dict().expect("a dictionary").clone();
    assert_eq!(
        value(&doc, &first_dict, b"Count").as_int(),
        Some(1),
        "the open parent states one visible descendant"
    );
    assert_eq!(first_dict.get_ref(Name::PARENT), Some(root));
    assert!(
        first_dict.get_ref(Name::PREV).is_none(),
        "the first has nothing before it"
    );

    let second = first_dict
        .get_ref(doc.intern(b"Next"))
        .expect("and points forward at the second");
    let second_dict = doc.get(second).expect("it resolves");
    let second_dict = second_dict.as_dict().expect("a dictionary").clone();
    assert_eq!(
        value(&doc, &second_dict, b"Count").as_int(),
        Some(-1),
        "the closed one states minus its own"
    );
    assert_eq!(
        second_dict.get_ref(Name::PREV),
        Some(first),
        "the second top-level entry points back at the first"
    );
    assert_eq!(
        root_dict.get_ref(doc.intern(b"Last")),
        Some(second),
        "and the root's /Last is the chain's own end"
    );

    // The leaf's destination resolves to a page of this document, which is
    // what makes a `/Dest` a destination rather than an array.
    let leaf = second_dict.get_ref(Name::FIRST).expect("a hidden child");
    let leaf = doc.get(leaf).expect("it resolves");
    let leaf = leaf.as_dict().expect("a dictionary").clone();
    let target = value(&doc, &leaf, b"Dest");
    let target = target.as_array().expect("an explicit destination");
    let page = target
        .first()
        .and_then(Object::as_objref)
        .expect("naming a page object");
    let numbers: Vec<u32> = pages(&doc).iter().map(|(r, _)| r.num).collect();
    assert_eq!(
        numbers.iter().position(|num| *num == page.num),
        Some(2),
        "the third page, which is what index 2 means"
    );
}

// ---- encryption moves every byte and no claim --------------------------------

/// The encrypted layout describes the same document at greater length.
///
/// The hint tables describe a *layout*, and encryption moves every byte of one
/// while leaving the document alone. So the structure has to come back
/// identical — the same pages, the same object numbering — and every page's
/// objects must occupy strictly more bytes, because each page owns a content
/// stream and AES-CBC never returns one the same size.
///
/// That is the assertion a table measured from the plaintext would fail: it
/// would give an encrypted file the *unencrypted* lengths, which is the one
/// shape this comparison forbids.
#[test]
fn the_encrypted_layout_is_the_same_pages_at_greater_length() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for index in 0..6 {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    let document = Arc::new(CosDocument::open(builder.finish()).expect("it opens"));

    let mut entropy = [0u8; 48];
    for (index, byte) in entropy.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(11);
    }
    let save = |encryption: Option<Encryption>| {
        DocumentEditor::new(Arc::clone(&document)).save(&WriteOptions {
            mode: WriteMode::Rewrite,
            linearize: true,
            object_streams: false,
            encryption,
            ..WriteOptions::default()
        })
    };

    let plain = save(None);
    let sealed = save(Some(Encryption {
        user_password: "open-me".to_string(),
        owner_password: "owner-me".to_string(),
        permissions: -1,
        entropy,
    }));
    assert!(
        sealed.len() > plain.len() + 150,
        "the encrypted file is only {} bytes longer",
        sealed.len() - plain.len()
    );

    let plain_doc = CosDocument::open(plain).expect("it opens");
    let sealed_doc = CosDocument::open(sealed).expect("it opens");
    assert!(
        sealed_doc.authenticate("open-me").is_ok(),
        "the fixture really is encrypted"
    );

    // The runs, not the absolute numbers: an encrypted file carries one more
    // object than a plain one — 7.6.1's `/Encrypt` dictionary — so every page
    // is numbered one higher and the *gaps* are what stay the same. Each page
    // still owns the same objects it did.
    let runs = |doc: &CosDocument| -> Vec<u32> {
        let numbers: Vec<u32> = pages(doc).iter().map(|(r, _)| r.num).collect();
        numbers.windows(2).map(|pair| pair[1] - pair[0]).collect()
    };
    assert_eq!(
        runs(&plain_doc),
        runs(&sealed_doc),
        "each page owns the same objects either way"
    );
    assert_eq!(pages(&plain_doc).len(), 6);

    // Each page's content stream, measured on disk: the ciphertext is longer
    // than the plaintext for every one of them.
    let mut compared = 0usize;
    for (index, ((_, before), (_, after))) in pages(&plain_doc)
        .iter()
        .zip(pages(&sealed_doc).iter())
        .enumerate()
    {
        let length = |doc: &CosDocument, page: &Dict| -> u64 {
            let stream = page.get_ref(Name::CONTENTS).expect("a content stream");
            let object = doc.get(stream).expect("it resolves");
            let stream = object.as_stream().expect("a stream");
            stream.len_hint.expect("a direct /Length")
        };
        let before = length(&plain_doc, before);
        let after = length(&sealed_doc, after);
        assert!(
            after > before,
            "page {index} is {after} bytes encrypted against {before} plain, and a \
             page holding a content stream cannot fail to grow"
        );
        compared += 1;
    }
    assert_eq!(compared, 6, "every page was compared");
}
