//! Variable faces held to the OpenType specification's arithmetic
//! (`docs/design/variable-fonts.md`).
//!
//! Every face here is built byte by byte by `variable_support`, and every
//! expected value is computed in the test by `variable_support::spec`, the
//! overview chapter's formulas written out a second time — never by the
//! code under test. `TestGPOSFour.ttf`, from the vendored text-rendering
//! tests, is the one face another producer made, and a second reader below
//! decodes its tables independently.

mod variable_support;

use tinker_pdf_font::cff::Machine;
use tinker_pdf_font::cff2::{Cff2, Cff2Error, Cff2Index};
use tinker_pdf_font::glyf;
use tinker_pdf_font::instance::{instantiate, InstanceError, DROPPED};
use tinker_pdf_font::variation::cvar::{cvt_at, Cvar};
use tinker_pdf_font::variation::gvar::{Gvar, MAX_GLYPH_DELTA_WORK};
use tinker_pdf_font::variation::item_store::{DeltaSetIndex, DeltaSetIndexMap, ItemVariationStore};
use tinker_pdf_font::variation::metrics::{metric_default, Hvar, Mvar, Vvar, METRIC_FIELDS};
use tinker_pdf_font::variation::tuple::{
    unpack_deltas, unpack_points, PointNumbers, SharedTuples, TupleStore,
};
use tinker_pdf_font::variation::{
    AxisValueMap, DesignSpace, Fixed, Instance, Location, Tag, UserCoordinate, VariationError,
    MAX_AXES,
};
use tinker_pdf_font::{Outline, Segment, Sfnt};
use variable_support::{
    avar, cff2_index, charstring, composite_glyph, cvar, cvt, dict_entry, f2dot14, fvar,
    fvar_with_sizes, gasp, glyf_and_loca, glyf_fixture, head, hhea, hvar, identity_map, index_map,
    ints, item_store, maxp, metrics, mvar, op, os2, pack_deltas, pack_points, post, spec,
    tuple_store, vhea, vorg, vvar, AxisSpec, Cff2Spec, ComponentSpec, Cs, DataSpec, Face,
    FdSelectSpec, GlyphPoint, GlyphVariation, InstanceSpec, Points, PrivateSpec, RegionSpec,
    TupleSpec,
};

// ---- The design space (FT-05.2) --------------------------------------------

/// The two axes every design-space test uses: a weight from 100 to 900 about
/// 400, and a width from 50 to 200 about 100, hidden.
fn two_axes() -> Vec<AxisSpec> {
    let mut width = AxisSpec::new(*b"wdth", 50.0, 100.0, 200.0, 257);
    width.flags = 0x0001;
    vec![AxisSpec::new(*b"wght", 100.0, 400.0, 900.0, 256), width]
}

/// Six named instances, one of them at coordinates whose normalisation does
/// not land on 2.14 and one outside both axes.
fn six_instances() -> Vec<InstanceSpec> {
    vec![
        InstanceSpec::new(258, &[100.0, 100.0], 270),
        InstanceSpec::new(259, &[400.0, 100.0], 271),
        InstanceSpec::new(260, &[700.0, 100.0], 272),
        InstanceSpec::new(261, &[700.0, 75.0], 273),
        InstanceSpec::new(262, &[333.25, 87.5], 274),
        InstanceSpec::new(263, &[1_000.0, 25.0], 275),
    ]
}

/// The `avar` the weight axis is bent by: two points below the default, two
/// above, and the three every modifying map holds.
fn weight_map() -> Vec<(i16, i16)> {
    vec![
        (-16_384, -16_384),
        (f2dot14(-0.5), f2dot14(-0.75)),
        (0, 0),
        (f2dot14(0.25), f2dot14(0.5)),
        (f2dot14(0.75), f2dot14(0.875)),
        (16_384, 16_384),
    ]
}

fn plain_face() -> Vec<u8> {
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .build()
}

fn bent_face() -> Vec<u8> {
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"avar", avar(&[weight_map(), identity_map()]))
        .build()
}

fn space_of(face: &[u8]) -> DesignSpace {
    let sfnt = Sfnt::parse(face).expect("a face");
    DesignSpace::read(&sfnt).expect("a design space")
}

/// The location the specification computes for user values on the two axes,
/// through the weight map when `bent`.
fn expected_location(user: [f64; 2], bent: bool) -> Vec<i16> {
    let axes = two_axes();
    let map = weight_map();
    axes.iter()
        .zip(user)
        .enumerate()
        .map(|(index, (axis, value))| {
            let normalised = spec::default_normalised(
                spec::user_fixed(axis.min),
                spec::user_fixed(axis.default),
                spec::user_fixed(axis.max),
                spec::user_fixed(value),
            );
            let normalised = if bent && index == 0 {
                spec::through_avar(&map, normalised)
            } else {
                normalised
            };
            spec::to_f2dot14(normalised)
        })
        .collect()
}

fn user(values: [f64; 2]) -> Vec<Fixed> {
    values.iter().map(|v| Fixed::from_f64(*v)).collect()
}

#[test]
fn axes_and_named_instances_are_read() {
    let space = space_of(&plain_face());
    assert!(space.is_variable());
    assert!(!space.has_avar());
    assert_eq!(space.axis_count(), 2);
    for (axis, stated) in space.axes().iter().zip(two_axes()) {
        assert_eq!(axis.tag, stated.tag);
        assert_eq!(axis.min.bits(), spec::user_fixed(stated.min));
        assert_eq!(axis.default.bits(), spec::user_fixed(stated.default));
        assert_eq!(axis.max.bits(), spec::user_fixed(stated.max));
        assert_eq!(axis.flags, stated.flags);
        assert_eq!(axis.name_id, stated.name_id);
    }
    assert!(!space.axes()[0].is_hidden());
    assert!(space.axes()[1].is_hidden(), "HIDDEN_AXIS is bit 0");
    assert_eq!(space.axis_index(*b"wdth"), Some(1));
    assert_eq!(space.axis_index(*b"opsz"), None);

    assert_eq!(space.instances().len(), 6);
    for (instance, stated) in space.instances().iter().zip(six_instances()) {
        assert_eq!(instance.subfamily_name_id, stated.subfamily_name_id);
        assert_eq!(instance.flags, 0);
        assert_eq!(instance.postscript_name_id, stated.postscript_name_id);
        let coordinates: Vec<i64> = instance.coordinates.iter().map(|c| c.bits()).collect();
        let stated: Vec<i64> = stated
            .coordinates
            .iter()
            .map(|c| spec::user_fixed(*c))
            .collect();
        assert_eq!(coordinates, stated);
    }

    // Records without the PostScript name ID, and records an extension made
    // longer: `axisSize` and `instanceSize` say where each record starts.
    let short: Vec<InstanceSpec> = six_instances()
        .into_iter()
        .map(|mut i| {
            i.postscript_name_id = None;
            i
        })
        .collect();
    let space = DesignSpace::parse(&fvar(&two_axes(), &short), None).expect("read");
    assert!(space
        .instances()
        .iter()
        .all(|i| i.postscript_name_id.is_none()));
    let extended = fvar_with_sizes(&two_axes(), &six_instances(), 24, 2 * 4 + 6 + 4);
    let space = DesignSpace::parse(&extended, None).expect("read");
    assert_eq!(space.axes()[1].tag, *b"wdth");
    assert_eq!(space.axes()[1].max.bits(), spec::user_fixed(200.0));
    assert_eq!(space.instances()[5].postscript_name_id, Some(275));
    assert_eq!(
        space.instances()[5].coordinates[0].bits(),
        spec::user_fixed(1_000.0)
    );
}

#[test]
fn user_coordinates_normalise_against_min_default_and_max() {
    let space = space_of(&plain_face());
    let weights = [
        100.0, 175.0, 250.0, 333.25, 399.0, 400.0, 401.0, 650.0, 777.7, 900.0,
    ];
    let widths = [50.0, 62.5, 87.5, 99.999, 100.0, 133.333, 150.0, 200.0];
    for weight in weights {
        for width in widths {
            let expected = expected_location([weight, width], false);
            let by_order = space.normalise(&user([weight, width]));
            assert_eq!(
                by_order.coordinates(),
                expected,
                "wght {weight} wdth {width}"
            );
            let by_tag = space
                .location(&Instance::User(vec![
                    UserCoordinate::new(*b"wdth", width),
                    UserCoordinate::new(*b"wght", weight),
                ]))
                .expect("a location");
            assert_eq!(by_tag, by_order, "by tag, in either order");
        }
    }
    // The three points the default mapping fixes.
    assert_eq!(
        space.normalise(&user([100.0, 50.0])).coordinates(),
        [-16_384, -16_384]
    );
    assert_eq!(space.normalise(&user([400.0, 100.0])).coordinates(), [0, 0]);
    assert_eq!(
        space.normalise(&user([900.0, 200.0])).coordinates(),
        [16_384, 16_384]
    );
    // The overview's own example: 250 on 100–400–900 is −0.5.
    assert_eq!(space.normalise(&user([250.0, 100.0])).coordinate(0), -8_192);
    // An axis given no value takes its default.
    assert_eq!(
        space.normalise(&[Fixed::from_f64(900.0)]).coordinates(),
        [16_384, 0]
    );
    assert_eq!(space.default_location().coordinates(), [0, 0]);
    assert!(space.default_location().is_default());
}

#[test]
fn avar_maps_normalised_coordinates_through_its_segments() {
    let space = space_of(&bent_face());
    assert!(space.has_avar());
    let map = space.segment_map(0).expect("the weight's map");
    assert!(map.applies());
    assert_eq!(map.maps().len(), 6);
    assert!(space.segment_map(1).expect("the width's map").applies());

    // At its points: each `fromCoordinate` lands on its `toCoordinate`.
    for (weight, to) in [
        (100.0, -16_384),
        (250.0, f2dot14(-0.75)),
        (400.0, 0),
        (525.0, f2dot14(0.5)),
        (775.0, f2dot14(0.875)),
        (900.0, 16_384),
    ] {
        let location = space.normalise(&user([weight, 100.0]));
        assert_eq!(
            location.coordinate(0),
            to,
            "wght {weight} at a point of the map"
        );
        assert_eq!(
            location.coordinates(),
            expected_location([weight, 100.0], true)
        );
    }
    // Between them: interpolated, with the ratio and the product rounded.
    for weight in [
        130.0, 175.0, 300.0, 333.25, 450.0, 480.0, 650.0, 700.0, 850.0, 899.0,
    ] {
        for width in [62.5, 100.0, 150.0] {
            let location = space.normalise(&user([weight, width]));
            assert_eq!(
                location.coordinates(),
                expected_location([weight, width], true),
                "wght {weight} wdth {width} between the points"
            );
        }
    }
    // The width's map is the identity, so the width reads as unbent.
    let bent = space.normalise(&user([400.0, 87.5]));
    let plain = space_of(&plain_face()).normalise(&user([400.0, 87.5]));
    assert_eq!(bent, plain);
}

#[test]
fn coordinates_outside_an_axis_are_clamped() {
    for face in [plain_face(), bent_face()] {
        let space = space_of(&face);
        for (weight, to) in [
            (0.0, -16_384),
            (-1_000.0, -16_384),
            (99.99, -16_384),
            (900.01, 16_384),
            (1e9, 16_384),
        ] {
            assert_eq!(
                space.normalise(&user([weight, 100.0])).coordinate(0),
                to,
                "wght {weight}"
            );
        }
        for (width, to) in [
            (0.0, -16_384),
            (49.0, -16_384),
            (201.0, 16_384),
            (f64::INFINITY, 16_384),
            (f64::NEG_INFINITY, -16_384),
        ] {
            let location = space
                .location(&Instance::User(vec![UserCoordinate::new(*b"wdth", width)]))
                .expect("a location");
            assert_eq!(location.coordinates(), [0, to], "wdth {width}");
        }
        // A named instance outside both axes is clamped like any coordinate.
        let outside = space.location(&Instance::Named(5)).expect("instance 5");
        assert_eq!(outside.coordinates(), [16_384, -16_384]);
    }
}

#[test]
fn a_named_instance_resolves_to_its_coordinates() {
    for (face, bent) in [(plain_face(), false), (bent_face(), true)] {
        let space = space_of(&face);
        for (index, stated) in six_instances().iter().enumerate() {
            let index = u16::try_from(index).expect("few");
            let user = space
                .user_coordinates(&Instance::Named(index))
                .expect("a named instance");
            let stated_fixed: Vec<i64> = stated
                .coordinates
                .iter()
                .map(|c| spec::user_fixed(*c))
                .collect();
            assert_eq!(
                user.iter().map(|c| c.bits()).collect::<Vec<_>>(),
                stated_fixed
            );
            let location = space.location(&Instance::Named(index)).expect("a location");
            let values = [stated.coordinates[0], stated.coordinates[1]];
            assert_eq!(
                location.coordinates(),
                expected_location(values, bent),
                "instance {index}"
            );
            assert_eq!(location, space.normalise(&user));
        }
        assert_eq!(
            space.location(&Instance::Named(6)),
            Err(VariationError::NoSuchInstance { index: 6, count: 6 })
        );
        // The default instance is no coordinate at all.
        assert_eq!(
            space.location(&Instance::default()).expect("the default"),
            space.default_location()
        );
    }
}

#[test]
fn user_coordinates_by_tag_take_the_last_value_and_ignore_what_names_nothing() {
    let space = space_of(&plain_face());
    let location = space
        .location(&Instance::User(vec![
            UserCoordinate::new(*b"wght", 100.0),
            UserCoordinate::new(*b"opsz", 12.0),
            UserCoordinate::new(*b"wght", 900.0),
            UserCoordinate::new(*b"wdth", f64::NAN),
        ]))
        .expect("a location");
    assert_eq!(location.coordinates(), [16_384, 0]);
}

#[test]
fn a_face_without_fvar_has_no_axes_and_every_coordinate_is_the_default() {
    let face = Face::truetype().with(*b"head", vec![0; 54]).build();
    let space = space_of(&face);
    assert!(!space.is_variable());
    assert!(space.axes().is_empty() && space.instances().is_empty());
    let location = space
        .location(&Instance::User(vec![UserCoordinate::new(*b"wght", 700.0)]))
        .expect("a location");
    assert!(location.is_default());
    assert!(location.coordinates().is_empty());
    assert_eq!(location.coordinate(3), 0, "past the end is the default");
    assert_eq!(
        space.location(&Instance::Named(0)),
        Err(VariationError::NoSuchInstance { index: 0, count: 0 })
    );
}

#[test]
fn a_zero_axis_count_is_a_face_that_does_not_vary() {
    // `fvar` with no axes, beside an `avar` that would be refused: the
    // chapter has every variation table of such a face ignored.
    let empty = fvar(&[], &[]);
    let broken_avar = vec![9, 9];
    let space = DesignSpace::parse(&empty, Some(&broken_avar)).expect("not variable");
    assert!(!space.is_variable());
    assert!(!space.has_avar());
}

/// One test face's `fvar` with a header field rewritten.
fn with_field(mut table: Vec<u8>, at: usize, value: u16) -> Vec<u8> {
    table[at..at + 2].copy_from_slice(&value.to_be_bytes());
    table
}

#[test]
fn hostile_design_spaces_are_refused_by_name() {
    let good = fvar(&two_axes(), &six_instances());
    let refuse = |fvar: &[u8], avar: Option<&[u8]>| DesignSpace::parse(fvar, avar).err();

    assert_eq!(
        refuse(&good[..15], None),
        Some(VariationError::HeaderPastEnd { table: *b"fvar" })
    );
    assert_eq!(
        refuse(&with_field(good.clone(), 0, 2), None),
        Some(VariationError::UnknownVersion {
            table: *b"fvar",
            major: 2,
            minor: 0
        })
    );
    let too_many = u16::try_from(MAX_AXES + 1).expect("small");
    assert_eq!(
        refuse(&with_field(good.clone(), 8, too_many), None),
        Some(VariationError::TooManyAxes { count: too_many })
    );
    assert_eq!(
        refuse(&with_field(good.clone(), 10, 16), None),
        Some(VariationError::AxisSize { size: 16 })
    );
    // An axis count past the table's end.
    assert_eq!(
        refuse(&with_field(good.clone(), 8, 40), None),
        Some(VariationError::AxesPastEnd { count: 40 })
    );
    assert_eq!(
        refuse(&good[..16 + 30], None),
        Some(VariationError::AxesPastEnd { count: 2 })
    );
    // A minimum above the default.
    let mut inverted = two_axes();
    inverted[1].min = 120.0;
    assert_eq!(
        refuse(&fvar(&inverted, &[]), None),
        Some(VariationError::AxisRange { axis: 1 })
    );
    let mut inverted = two_axes();
    inverted[0].max = 300.0;
    assert_eq!(
        refuse(&fvar(&inverted, &[]), None),
        Some(VariationError::AxisRange { axis: 0 })
    );
    // An instance size too small for two coordinates, and records past the
    // end — both a count too large and a table cut short.
    assert_eq!(
        refuse(&with_field(good.clone(), 14, 11), None),
        Some(VariationError::InstanceSize { size: 11 })
    );
    assert_eq!(
        refuse(&with_field(good.clone(), 12, 7), None),
        Some(VariationError::InstancesPastEnd { count: 7 })
    );
    assert_eq!(
        refuse(&good[..good.len() - 1], None),
        Some(VariationError::InstancesPastEnd { count: 6 })
    );

    // `avar`: a short header, an unknown version, a count that is not
    // fvar's, and a segment count past the table's end.
    let bent = avar(&[weight_map(), identity_map()]);
    assert_eq!(
        refuse(&good, Some(&bent[..7])),
        Some(VariationError::HeaderPastEnd { table: *b"avar" })
    );
    assert_eq!(
        refuse(&good, Some(&with_field(bent.clone(), 0, 2))),
        Some(VariationError::UnknownVersion {
            table: *b"avar",
            major: 2,
            minor: 0
        })
    );
    assert_eq!(
        refuse(&good, Some(&avar(&[weight_map()]))),
        Some(VariationError::AxisCount {
            table: *b"avar",
            found: 1,
            expected: 2
        })
    );
    assert_eq!(
        refuse(&good, Some(&with_field(bent.clone(), 8, 200))),
        Some(VariationError::SegmentsPastEnd { axis: 0 })
    );
    assert_eq!(
        refuse(&good, Some(&bent[..bent.len() - 2])),
        Some(VariationError::SegmentsPastEnd { axis: 1 })
    );

    // Every refusal says what it is.
    for error in [
        VariationError::HeaderPastEnd { table: *b"fvar" },
        VariationError::TooManyAxes { count: 70 },
        VariationError::AxisRange { axis: 1 },
        VariationError::SegmentsPastEnd { axis: 0 },
        VariationError::NoSuchInstance { index: 4, count: 2 },
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn every_truncation_of_a_design_space_is_read_or_refused_without_panicking() {
    let good_fvar = fvar(&two_axes(), &six_instances());
    let good_avar = avar(&[weight_map(), identity_map()]);
    for cut in 0..=good_fvar.len() {
        let _ = DesignSpace::parse(&good_fvar[..cut], Some(&good_avar));
    }
    for cut in 0..=good_avar.len() {
        let _ = DesignSpace::parse(&good_fvar, Some(&good_avar[..cut]));
    }
    // Every byte of each table set to each of a few hostile values.
    for at in 0..good_fvar.len() {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut bytes = good_fvar.clone();
            bytes[at] = value;
            if let Ok(space) = DesignSpace::parse(&bytes, None) {
                let _ = space.location(&Instance::Named(0));
                let _ = space.normalise(&[Fixed::from_int(500), Fixed::from_int(80)]);
            }
        }
    }
    for at in 0..good_avar.len() {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut bytes = good_avar.clone();
            bytes[at] = value;
            if let Ok(space) = DesignSpace::parse(&good_fvar, Some(&bytes)) {
                for weight in [100, 250, 400, 650, 900] {
                    let _ = space.normalise(&[Fixed::from_int(weight)]);
                }
            }
        }
    }
}

#[test]
fn a_segment_map_built_from_records_keeps_what_the_chapter_keeps() {
    // The map the bent face carries, as `SegmentMap` reads records.
    let records: Vec<AxisValueMap> = weight_map()
        .into_iter()
        .map(|(from, to)| AxisValueMap { from, to })
        .collect();
    let space = space_of(&bent_face());
    assert_eq!(
        space.segment_map(0).expect("map").maps(),
        records.as_slice()
    );
    // A location stores exactly the 2.14 the overview's step 5 gives.
    let location: Location = space.normalise(&user([650.0, 100.0]));
    let expected = spec::to_f2dot14(spec::through_avar(
        &weight_map(),
        spec::default_normalised(
            spec::user_fixed(100.0),
            spec::user_fixed(400.0),
            spec::user_fixed(900.0),
            spec::user_fixed(650.0),
        ),
    ));
    assert_eq!(location.coordinate(0), expected);
}

// ---- TestGPOSFour.ttf, through a second reader -------------------------------

/// A face from the vendored text-rendering tests, the one variable face in
/// the tree another producer made.
const TEST_GPOS_FOUR: &[u8] =
    include_bytes!("../../tinker-pdf-shape/data/text-rendering-tests/fonts/TestGPOSFour.ttf");

/// The second reader: the table directory and `fvar` and `avar` decoded from
/// the chapters' field lists by index arithmetic, with no code shared with
/// the crate.
mod second_reader {
    fn u16_at(data: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([data[at], data[at + 1]])
    }

    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    pub fn table<'a>(face: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let count = usize::from(u16_at(face, 4));
        for record in 0..count {
            let at = 12 + record * 16;
            if &face[at..at + 4] == tag {
                let offset = u32_at(face, at + 8) as usize;
                let length = u32_at(face, at + 12) as usize;
                return Some(&face[offset..offset + length]);
            }
        }
        None
    }

    pub struct Axis {
        pub tag: [u8; 4],
        pub min: i32,
        pub default: i32,
        pub max: i32,
        pub flags: u16,
        pub name_id: u16,
    }

    pub struct Instance {
        pub subfamily: u16,
        pub flags: u16,
        pub coordinates: Vec<i32>,
        pub postscript: Option<u16>,
    }

    pub fn fvar(table: &[u8]) -> (Vec<Axis>, Vec<Instance>) {
        let axes_at = usize::from(u16_at(table, 4));
        let axis_count = usize::from(u16_at(table, 8));
        let axis_size = usize::from(u16_at(table, 10));
        let instance_count = usize::from(u16_at(table, 12));
        let instance_size = usize::from(u16_at(table, 14));
        let axes = (0..axis_count)
            .map(|i| {
                let at = axes_at + i * axis_size;
                Axis {
                    tag: [table[at], table[at + 1], table[at + 2], table[at + 3]],
                    min: u32_at(table, at + 4) as i32,
                    default: u32_at(table, at + 8) as i32,
                    max: u32_at(table, at + 12) as i32,
                    flags: u16_at(table, at + 16),
                    name_id: u16_at(table, at + 18),
                }
            })
            .collect();
        let instances_at = axes_at + axis_count * axis_size;
        let instances = (0..instance_count)
            .map(|i| {
                let at = instances_at + i * instance_size;
                Instance {
                    subfamily: u16_at(table, at),
                    flags: u16_at(table, at + 2),
                    coordinates: (0..axis_count)
                        .map(|a| u32_at(table, at + 4 + a * 4) as i32)
                        .collect(),
                    postscript: (instance_size >= axis_count * 4 + 6)
                        .then(|| u16_at(table, at + 4 + axis_count * 4)),
                }
            })
            .collect();
        (axes, instances)
    }

    /// Each axis's segment map, as `(from, to)` pairs.
    pub fn avar(table: &[u8]) -> Vec<Vec<(i16, i16)>> {
        let count = usize::from(u16_at(table, 6));
        let mut at = 8;
        let mut maps = Vec::new();
        for _ in 0..count {
            let pairs = usize::from(u16_at(table, at));
            at += 2;
            maps.push(
                (0..pairs)
                    .map(|p| {
                        let from = u16_at(table, at + p * 4) as i16;
                        let to = u16_at(table, at + p * 4 + 2) as i16;
                        (from, to)
                    })
                    .collect(),
            );
            at += pairs * 4;
        }
        maps
    }
}

#[test]
fn test_gpos_four_reads_as_a_second_reader_decodes_it() {
    let sfnt = Sfnt::parse(TEST_GPOS_FOUR).expect("the face parses");
    let space = DesignSpace::read(&sfnt).expect("its design space is read");
    let table = second_reader::table(TEST_GPOS_FOUR, b"fvar").expect("it carries fvar");
    let (axes, instances) = second_reader::fvar(table);
    assert_eq!(axes.len(), 2, "two axes");
    assert_eq!(instances.len(), 36, "thirty-six named instances");

    assert_eq!(space.axes().len(), axes.len());
    for (read, decoded) in space.axes().iter().zip(&axes) {
        assert_eq!(read.tag, decoded.tag);
        assert_eq!(read.min, Fixed::from_fixed32(decoded.min));
        assert_eq!(read.default, Fixed::from_fixed32(decoded.default));
        assert_eq!(read.max, Fixed::from_fixed32(decoded.max));
        assert_eq!(read.flags, decoded.flags);
        assert_eq!(read.name_id, decoded.name_id);
    }
    assert_eq!(space.instances().len(), instances.len());
    for (read, decoded) in space.instances().iter().zip(&instances) {
        assert_eq!(read.subfamily_name_id, decoded.subfamily);
        assert_eq!(read.flags, decoded.flags);
        assert_eq!(read.postscript_name_id, decoded.postscript);
        let coordinates: Vec<Fixed> = decoded
            .coordinates
            .iter()
            .map(|c| Fixed::from_fixed32(*c))
            .collect();
        assert_eq!(read.coordinates, coordinates);
    }

    // Every named instance's location, computed by the specification from
    // what the second reader decoded, `avar` included where the face has one.
    let maps = second_reader::table(TEST_GPOS_FOUR, b"avar").map(second_reader::avar);
    for (index, decoded) in instances.iter().enumerate() {
        let expected: Vec<i16> = axes
            .iter()
            .enumerate()
            .map(|(a, axis)| {
                let mut value = spec::default_normalised(
                    i64::from(axis.min),
                    i64::from(axis.default),
                    i64::from(axis.max),
                    i64::from(decoded.coordinates[a]),
                );
                if let Some(map) = maps.as_ref().and_then(|m| m.get(a)) {
                    value = spec::avar(map, value);
                }
                spec::to_f2dot14(value)
            })
            .collect();
        let index = u16::try_from(index).expect("few");
        let location = space.location(&Instance::Named(index)).expect("a location");
        assert_eq!(location.coordinates(), expected, "named instance {index}");
    }
}

// ---- The tuple variation store and cvar (FT-05.3) ----------------------------

/// The `cvt ` the cvar tests vary: ten FWORDs, the ends of the range among
/// them.
fn cvt_defaults() -> Vec<i16> {
    vec![120, -40, 0, 700, 1_500, -1_500, 33, 32_767, -32_768, 5]
}

/// The shared point numbers several tables take.
fn shared_points() -> Points {
    Points::Listed(vec![0, 3, 4, 7, 9])
}

/// Tables of every shape a region takes: a peak at the weight's maximum with
/// the width's peak zero; one at its minimum, moving every entry; a corner;
/// an intermediate region; a peak at the width's minimum. Private points
/// repeat a number and name one past the `cvt `; words and bytes and zeros
/// all appear among the deltas.
fn cvar_tuples() -> Vec<TupleSpec> {
    vec![
        TupleSpec::new(&[16_384, 0], None, &[10, -20, 30, 400, -5]),
        TupleSpec::new(
            &[-16_384, 0],
            Some(Points::All),
            &[3, 6, 9, 12, 15, 18, 21, -24, 27, 0],
        ),
        TupleSpec::new(
            &[8_192, 16_384],
            Some(Points::Listed(vec![1, 1, 2, 12])),
            &[7, 8, -9, 1_000],
        ),
        TupleSpec::new(
            &[8_192, -8_192],
            Some(Points::Listed(vec![5, 6])),
            &[-300, 250],
        )
        .between(&[4_096, -16_384], &[16_384, 0]),
        TupleSpec::new(&[0, -16_384], None, &[1_000, -1_000, 2, 3, 0]),
    ]
}

fn cvar_face() -> Vec<u8> {
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"cvt ", cvt(&cvt_defaults()))
        .with(*b"cvar", cvar(Some(&shared_points()), &cvar_tuples()))
        .build()
}

/// The axes the cvar tests normalise against, as `(min, default, max)`.
fn axis_ranges() -> Vec<(f64, f64, f64)> {
    two_axes()
        .iter()
        .map(|axis| (axis.min, axis.default, axis.max))
        .collect()
}

#[test]
fn a_tuples_scalar_is_the_product_of_its_axis_factors() {
    let table = cvar(Some(&shared_points()), &cvar_tuples());
    let parsed = Cvar::parse(&table, 2).expect("a cvar");
    let tuples = parsed.store().tuples().expect("its tables");
    assert_eq!(tuples.len(), 5);
    let grid = [
        -16_384, -12_288, -8_192, -6_000, -4_096, -1, 0, 1, 4_096, 6_000, 8_192, 12_288, 16_384,
    ];
    for (tuple, stated) in tuples.iter().zip(cvar_tuples()) {
        let region: Vec<(i16, i16, i16)> = tuple
            .region()
            .iter()
            .map(|axis| (axis.start, axis.peak, axis.end))
            .collect();
        assert_eq!(region, stated.region(), "the region the table states");
        for weight in grid {
            for width in grid {
                let location = Location::new(vec![weight, width]);
                assert_eq!(
                    tuple.scalar(&location).bits(),
                    spec::region_scalar(&stated.region(), &[weight, width]),
                    "peak {:?} at ({weight}, {width})",
                    stated.peak
                );
            }
        }
    }
    let at = |w: i16, d: i16| Location::new(vec![w, d]);
    // A zero peak ignores its axis: the weight's maximum moves fully at any
    // width.
    assert_eq!(tuples[0].scalar(&at(16_384, -16_384)), Fixed::ONE);
    assert_eq!(
        tuples[0].scalar(&at(8_192, 16_384)),
        Fixed::from_bits(32_768)
    );
    // Out of the region on one axis is out of it altogether.
    assert_eq!(tuples[0].scalar(&at(-1, 0)), Fixed::ZERO);
    assert_eq!(tuples[2].scalar(&at(8_192, -1)), Fixed::ZERO);
    // A corner: the product of two halves is a quarter.
    assert_eq!(
        tuples[2].scalar(&at(4_096, 8_192)),
        Fixed::from_bits(16_384)
    );
    // The intermediate region: rising on the weight, falling on the width.
    assert_eq!(
        tuples[3].intermediate().map(|(start, _)| start.to_vec()),
        Some(vec![4_096, -16_384])
    );
    assert_eq!(
        tuples[3].scalar(&at(6_144, -8_192)),
        Fixed::from_bits(32_768)
    );
    assert_eq!(
        tuples[3].scalar(&at(8_192, -4_096)),
        Fixed::from_bits(32_768)
    );
    assert_eq!(
        tuples[3].scalar(&at(4_096, -8_192)),
        Fixed::ZERO,
        "at its start"
    );
    assert_eq!(
        tuples[3].scalar(&at(16_384, -8_192)),
        Fixed::ZERO,
        "at its end"
    );
}

#[test]
fn packed_point_numbers_and_deltas_decode_in_every_run_form() {
    // Point numbers: byte runs, word runs and the two mixed, a one-byte and
    // a two-byte count, a repeated number, and runs cut at 128.
    let lists: Vec<Vec<u16>> = vec![
        vec![0],
        vec![1, 2, 3, 200, 455],
        vec![300, 301, 1_000, 1_001, 60_000],
        vec![4, 4, 4, 9],
        (0..129).collect(),
        (0..300).map(|n| n * 3).collect(),
        (0..140).map(|n| n * 300).collect(),
        Vec::new(),
    ];
    for numbers in lists {
        let packed = pack_points(&Points::Listed(numbers.clone()));
        let (points, used) = unpack_points(&packed).expect("decodes");
        assert_eq!(points, PointNumbers::Listed(numbers.clone()), "{numbers:?}");
        assert_eq!(used, packed.len(), "and stops where they end");
        // Followed by other data, they still stop where they end.
        let mut longer = packed.clone();
        longer.extend_from_slice(&[0xAB, 0xCD]);
        assert_eq!(unpack_points(&longer).expect("decodes").1, packed.len());
    }
    assert_eq!(
        unpack_points(&pack_points(&Points::All)),
        Ok((PointNumbers::All, 1))
    );

    // Deltas: zeros, bytes at both ends of their range, words at both ends
    // of theirs and just past a byte's, runs cut at 64, and the mixture.
    let streams: Vec<Vec<i32>> = vec![
        vec![0; 65],
        vec![-128, 127, 1, -1],
        vec![128, -129, 32_767, -32_768],
        (0..200).map(|n| (n % 7) - 3).collect(),
        (0..150).map(|n| n * 211 - 15_000).collect(),
        vec![0, 0, 5, 0, -300, 300, 0, 0, 0, 12],
    ];
    for deltas in streams {
        let packed = pack_deltas(&deltas);
        let (decoded, used) = unpack_deltas(&packed, deltas.len()).expect("decodes");
        assert_eq!(decoded, deltas);
        assert_eq!(used, packed.len());
    }
    // The chapter's own example.
    let example = [
        0x03, 0x0A, 0x97, 0x00, 0xC6, 0x87, 0x41, 0x10, 0x22, 0xFB, 0x34,
    ];
    let (decoded, _) = unpack_deltas(&example, 14).expect("the example");
    assert_eq!(
        decoded,
        vec![10, -105, 0, -58, 0, 0, 0, 0, 0, 0, 0, 0, 4_130, -1_228]
    );

    // And through a store: one table at its peak moves each entry by exactly
    // its packed delta.
    let deltas: Vec<i32> = vec![-128, 127, 0, 32_767, -32_768, 300, 0, 0, -1, 9];
    let table = cvar(
        None,
        &[TupleSpec::new(&[16_384, 0], Some(Points::All), &deltas)],
    );
    let parsed = Cvar::parse(&table, 2).expect("a cvar");
    let defaults = vec![0i16; 10];
    let values = parsed
        .apply(&cvt(&defaults), &Location::new(vec![16_384, 0]))
        .expect("values");
    let moved: Vec<i64> = values.iter().map(|v| v.bits() / 65_536).collect();
    assert_eq!(
        moved,
        deltas.iter().map(|d| i64::from(*d)).collect::<Vec<_>>()
    );
}

#[test]
fn the_all_points_form_covers_every_point() {
    let defaults = cvt_defaults();
    let deltas: Vec<i32> = (0..10).map(|n| n * 2 + 1).collect();
    // Once as private points, once as the shared ones two tables take.
    for (shared, tuples) in [
        (
            None,
            vec![TupleSpec::new(&[16_384, 0], Some(Points::All), &deltas)],
        ),
        (
            Some(Points::All),
            vec![
                TupleSpec::new(&[16_384, 0], None, &deltas),
                TupleSpec::new(&[0, 16_384], None, &deltas),
            ],
        ),
    ] {
        let table = cvar(shared.as_ref(), &tuples);
        let parsed = Cvar::parse(&table, 2).expect("a cvar");
        let stored = parsed.store().tuples().expect("tables");
        assert!(stored.iter().all(|t| *t.points() == PointNumbers::All));
        for coordinates in [[16_384, 0], [8_192, 0], [16_384, 16_384], [4_096, 12_288]] {
            let values = parsed
                .apply(&cvt(&defaults), &Location::new(coordinates.to_vec()))
                .expect("values");
            let expected = spec::cvt_at(&defaults, shared.as_ref(), &tuples, &coordinates);
            assert_eq!(
                values.iter().map(|v| v.bits()).collect::<Vec<_>>(),
                expected
            );
            if coordinates == [16_384, 0] {
                for (index, value) in values.iter().enumerate() {
                    assert_ne!(
                        value.bits(),
                        i64::from(defaults[index]) * 65_536,
                        "entry {index} moved"
                    );
                }
            }
        }
    }
}

#[test]
fn the_cvt_at_an_instance_is_the_default_plus_the_scaled_deltas() {
    let face = cvar_face();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let ranges = axis_ranges();
    let check = |instance: &Instance, user: [f64; 2]| {
        let coordinates = spec::location(&ranges, &user);
        let location = space.location(instance).expect("a location");
        assert_eq!(location.coordinates(), coordinates);
        let values = cvt_at(&sfnt, &space, &location).expect("the cvt");
        let expected = spec::cvt_at(
            &cvt_defaults(),
            Some(&shared_points()),
            &cvar_tuples(),
            &coordinates,
        );
        assert_eq!(
            values.iter().map(|v| v.bits()).collect::<Vec<_>>(),
            expected,
            "at {user:?}"
        );
    };
    // Every named instance.
    for (index, stated) in six_instances().iter().enumerate() {
        let index = u16::try_from(index).expect("few");
        let user = [stated.coordinates[0], stated.coordinates[1]];
        check(&Instance::Named(index), user);
    }
    // And coordinates no instance names, on and off 2.14's grid.
    for weight in [
        100.0, 150.0, 250.0, 333.25, 400.0, 555.5, 650.0, 800.0, 900.0,
    ] {
        for width in [50.0, 62.5, 80.0, 100.0, 133.333, 200.0] {
            let instance = Instance::User(vec![
                UserCoordinate::new(*b"wght", weight),
                UserCoordinate::new(*b"wdth", width),
            ]);
            check(&instance, [weight, width]);
        }
    }
    // The default instance is the defaults.
    let values = cvt_at(&sfnt, &space, &space.default_location()).expect("the cvt");
    let defaults: Vec<i64> = cvt_defaults()
        .iter()
        .map(|v| i64::from(*v) * 65_536)
        .collect();
    assert_eq!(
        values.iter().map(|v| v.bits()).collect::<Vec<_>>(),
        defaults
    );
}

#[test]
fn a_face_without_cvar_or_variation_keeps_its_cvt() {
    let defaults: Vec<i64> = cvt_defaults()
        .iter()
        .map(|v| i64::from(*v) * 65_536)
        .collect();
    // No cvar.
    let face = Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"cvt ", cvt(&cvt_defaults()))
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let location = Location::new(vec![16_384, 16_384]);
    let values = cvt_at(&sfnt, &space, &location).expect("the cvt");
    assert_eq!(
        values.iter().map(|v| v.bits()).collect::<Vec<_>>(),
        defaults
    );
    // A cvar in a face that does not vary is ignored, as the fvar chapter
    // has it.
    let face = Face::truetype()
        .with(*b"cvt ", cvt(&cvt_defaults()))
        .with(*b"cvar", vec![0xFF; 6])
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("no axes");
    let values = cvt_at(&sfnt, &space, &location).expect("the cvt");
    assert_eq!(
        values.iter().map(|v| v.bits()).collect::<Vec<_>>(),
        defaults
    );
    // No cvt at all: nothing to vary.
    let face = Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"cvar", cvar(Some(&shared_points()), &cvar_tuples()))
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    assert_eq!(cvt_at(&sfnt, &space, &location), Ok(Vec::new()));
}

#[test]
fn hostile_tuple_stores_are_refused_by_name() {
    let location = Location::new(vec![16_384, 0]);
    let cvt_bytes = cvt(&cvt_defaults());
    let apply = |table: &[u8]| -> Result<Vec<Fixed>, VariationError> {
        Cvar::parse(table, 2)?.apply(&cvt_bytes, &location)
    };
    let good = cvar(Some(&shared_points()), &cvar_tuples());
    assert!(apply(&good).is_ok());

    assert_eq!(
        apply(&good[..3]).err(),
        Some(VariationError::HeaderPastEnd { table: *b"cvar" })
    );
    let mut version = good.clone();
    version[0..2].copy_from_slice(&3u16.to_be_bytes());
    assert_eq!(
        apply(&version).err(),
        Some(VariationError::UnknownVersion {
            table: *b"cvar",
            major: 3,
            minor: 0
        })
    );
    assert_eq!(
        apply(&good[..6]).err(),
        Some(VariationError::TupleHeadersPastEnd)
    );
    let mut offset = good.clone();
    offset[6..8].copy_from_slice(&0x7FFFu16.to_be_bytes());
    assert_eq!(
        apply(&offset).err(),
        Some(VariationError::TupleHeadersPastEnd)
    );

    // A peak cvar does not embed names a shared tuple, of which it has none.
    let mut not_embedded = TupleSpec::new(&[16_384, 0], Some(Points::All), &[1; 10]);
    not_embedded.shared_peak = Some(3);
    assert_eq!(
        apply(&cvar(None, &[not_embedded])).err(),
        Some(VariationError::SharedTupleIndex { index: 3, count: 0 })
    );
    // A table with no points of its own in a store with no shared ones.
    assert_eq!(
        apply(&cvar(None, &[TupleSpec::new(&[16_384, 0], None, &[1, 2])])).err(),
        Some(VariationError::NoPointNumbers { tuple: 0 })
    );
    // A data size past the store's end.
    let mut oversized = cvar(
        None,
        &[TupleSpec::new(&[16_384, 0], Some(Points::All), &[1; 10])],
    );
    oversized[8..10].copy_from_slice(&0x0FFFu16.to_be_bytes());
    assert_eq!(
        apply(&oversized).err(),
        Some(VariationError::TupleDataPastEnd { tuple: 0 })
    );
    // Deltas fewer than their points, and a run past their count.
    let short = cvar(
        None,
        &[TupleSpec::new(
            &[16_384, 0],
            Some(Points::Listed(vec![0, 1, 2])),
            &[1, 2],
        )],
    );
    assert_eq!(apply(&short).err(), Some(VariationError::DeltasPastEnd));
    let long = cvar(
        None,
        &[TupleSpec::new(
            &[16_384, 0],
            Some(Points::Listed(vec![0])),
            &[1, 2, 3],
        )],
    );
    assert_eq!(apply(&long).err(), Some(VariationError::DeltaRunsOverrun));
    // Shared points that do not decode: the flag set over a store whose
    // serialized data is a two-byte count with no runs after it.
    let mut table = vec![0, 1, 0, 0];
    table.extend(tuple_store(
        4,
        None,
        &[TupleSpec::new(&[16_384, 0], Some(Points::All), &[])],
    ));
    table[4..6].copy_from_slice(&0x8001u16.to_be_bytes());
    let data_at = usize::from(u16::from_be_bytes([table[6], table[7]]));
    table.truncate(data_at);
    table.extend_from_slice(&[0x80, 0x05]);
    assert_eq!(apply(&table).err(), Some(VariationError::PointsPastEnd));
    // The refusal is the table's, whatever the location: a table whose
    // scalar is zero is decoded all the same.
    assert_eq!(
        Cvar::parse(&long, 2)
            .expect("parses")
            .apply(&cvt_bytes, &Location::new(vec![-16_384, 0]))
            .err(),
        Some(VariationError::DeltaRunsOverrun)
    );
}

#[test]
fn every_truncation_and_byte_change_of_a_cvar_is_read_or_refused_without_panicking() {
    let good = cvar(Some(&shared_points()), &cvar_tuples());
    let cvt_bytes = cvt(&cvt_defaults());
    let location = Location::new(vec![8_192, -4_096]);
    for cut in 0..=good.len() {
        if let Ok(parsed) = Cvar::parse(&good[..cut], 2) {
            let _ = parsed.apply(&cvt_bytes, &location);
        }
    }
    for at in 0..good.len() {
        for value in [0x00, 0x01, 0x7F, 0x80, 0xC0, 0xFF] {
            let mut bytes = good.clone();
            bytes[at] = value;
            if let Ok(parsed) = Cvar::parse(&bytes, 2) {
                let _ = parsed.apply(&cvt_bytes, &location);
                let _ = parsed.apply(&cvt_bytes[..7], &location);
            }
        }
    }
}

#[test]
fn gvars_shared_tuples_give_a_peak_by_index() {
    // A shared tuple array that does not fit its count is refused.
    assert_eq!(
        SharedTuples::new(&[0; 7], 2, 2).err(),
        Some(VariationError::SharedTuplesPastEnd { count: 2 })
    );
    let shared = SharedTuples::new(&[0x40, 0, 0xC0, 0], 1, 2).expect("one tuple");
    assert_eq!(shared.len(), 1);
    assert_eq!(shared.get(0), Some(vec![16_384, -16_384]));
    assert_eq!(shared.get(1), None);
    let mut by_index = TupleSpec::new(&[], Some(Points::All), &[5; 10]);
    by_index.shared_peak = Some(0);
    let mut past = TupleSpec::new(&[], Some(Points::All), &[5; 10]);
    past.shared_peak = Some(1);
    let store_bytes = tuple_store(0, None, &[by_index]);
    let store = TupleStore::parse(&store_bytes, 0, 2, shared).expect("a store");
    let tuples = store.tuples().expect("one table");
    assert_eq!(tuples[0].peak(), [16_384, -16_384]);
    assert_eq!(
        tuples[0].scalar(&Location::new(vec![8_192, -8_192])),
        Fixed::from_bits(16_384)
    );
    let store_bytes = tuple_store(0, None, &[past]);
    let store = TupleStore::parse(&store_bytes, 0, 2, shared).expect("a store");
    assert_eq!(
        store.tuples().err(),
        Some(VariationError::SharedTupleIndex { index: 1, count: 1 })
    );
}

// ---- The item variation store and the metrics tables (FT-05.5) -------------

/// Six regions over the two axes: each axis's two extremes, a corner, and an
/// intermediate region on the weight whose width peak is zero.
fn regions() -> Vec<RegionSpec> {
    vec![
        vec![(0, 16_384, 16_384), (0, 0, 0)],
        vec![(-16_384, -16_384, 0), (0, 0, 0)],
        vec![(0, 0, 0), (0, 16_384, 16_384)],
        vec![(0, 16_384, 16_384), (0, 16_384, 16_384)],
        vec![(4_096, 8_192, 16_384), (-16_384, 0, 16_384)],
        vec![(0, 0, 0), (-16_384, -16_384, 0)],
    ]
}

/// Item variation data in every delta width: a 16-bit column and two 8-bit
/// ones, six rows; three 16-bit columns; a 32-bit column and a 16-bit one; a
/// NULL subtable; two 8-bit columns.
fn data_tables() -> Vec<Option<DataSpec>> {
    vec![
        Some(DataSpec {
            regions: vec![0, 1, 2],
            word_count: 1,
            long_words: false,
            rows: vec![
                vec![300, -12, 40],
                vec![-1_000, 127, -128],
                vec![0, 0, 0],
                vec![32_767, 5, -5],
                vec![-32_768, 1, 2],
                vec![7, -7, 77],
            ],
        }),
        Some(DataSpec {
            regions: vec![3, 4, 5],
            word_count: 3,
            long_words: false,
            rows: vec![
                vec![1_000, -2_000, 3_000],
                vec![-1, 2, -3],
                vec![500, 500, -500],
            ],
        }),
        Some(DataSpec {
            regions: vec![0, 3],
            word_count: 1,
            long_words: true,
            rows: vec![vec![100_000, -30_000], vec![-2_000_000, 32_767]],
        }),
        None,
        Some(DataSpec {
            regions: vec![1, 4],
            word_count: 0,
            long_words: false,
            rows: vec![vec![-128, 127], vec![10, -10]],
        }),
    ]
}

fn store_bytes() -> Vec<u8> {
    item_store(2, &regions(), &data_tables())
}

/// The adjustment the specification computes for a delta-set index.
fn expected_delta(outer: u32, inner: u32, coordinates: &[i16]) -> i64 {
    if (outer, inner) == (0xFFFF, 0xFFFF) {
        return 0;
    }
    match &data_tables()[outer as usize] {
        None => 0,
        Some(data) => spec::item_delta(&regions(), data, inner as usize, coordinates),
    }
}

/// Locations over a grid of 2.14 coordinates, on and between region points.
fn location_grid() -> Vec<[i16; 2]> {
    let values = [
        -16_384, -9_000, -8_192, -1, 0, 4_096, 6_144, 8_192, 12_000, 16_384,
    ];
    let mut grid = Vec::new();
    for weight in values {
        for width in values {
            grid.push([weight, width]);
        }
    }
    grid
}

#[test]
fn region_scalars_and_delta_sets_decode_in_every_delta_width() {
    let bytes = store_bytes();
    let store = ItemVariationStore::parse(&bytes).expect("a store");
    assert_eq!(store.axis_count(), 2);
    assert_eq!(store.region_count(), 6);
    assert_eq!(store.data_count(), 5);
    for (index, stated) in regions().iter().enumerate() {
        let region: Vec<(i16, i16, i16)> = store
            .region(u16::try_from(index).expect("few"))
            .expect("a region")
            .iter()
            .map(|axis| (axis.start, axis.peak, axis.end))
            .collect();
        assert_eq!(&region, stated);
    }
    assert_eq!(
        store.region(6).err(),
        Some(VariationError::RegionIndex { index: 6, count: 6 })
    );

    for coordinates in location_grid() {
        let scalars = store.region_scalars(&Location::new(coordinates.to_vec()));
        assert_eq!(scalars.len(), 6);
        for (index, region) in regions().iter().enumerate() {
            assert_eq!(
                scalars[index].bits(),
                spec::region_scalar(region, &coordinates),
                "region {index} at {coordinates:?}"
            );
        }
        for (outer, stated) in data_tables().iter().enumerate() {
            let outer = u16::try_from(outer).expect("few");
            let read = store.data(outer).expect("a subtable");
            let Some(stated) = stated else {
                assert!(read.is_none(), "a NULL offset is no subtable");
                assert_eq!(
                    store.delta(DeltaSetIndex::new(outer, 3), &scalars),
                    Ok(Fixed::ZERO)
                );
                continue;
            };
            let data = read.expect("a subtable");
            assert_eq!(data.region_indexes(), stated.regions.as_slice());
            assert_eq!(data.word_count(), stated.word_count);
            assert_eq!(data.long_words(), stated.long_words);
            assert_eq!(usize::from(data.item_count()), stated.rows.len());
            for (inner, row) in stated.rows.iter().enumerate() {
                let inner = u16::try_from(inner).expect("few");
                assert_eq!(&data.deltas(inner).expect("a row"), row);
                let delta = store
                    .delta(DeltaSetIndex::new(outer, inner), &scalars)
                    .expect("a delta");
                assert_eq!(
                    delta.bits(),
                    expected_delta(u32::from(outer), u32::from(inner), &coordinates),
                    "delta set {outer}/{inner} at {coordinates:?}"
                );
            }
            assert_eq!(
                data.deltas(data.item_count()).err(),
                Some(VariationError::DeltaSetIndex {
                    outer,
                    inner: data.item_count()
                })
            );
        }
        assert_eq!(
            store.delta(DeltaSetIndex::NO_VARIATION, &scalars),
            Ok(Fixed::ZERO)
        );
        assert_eq!(
            store.delta(DeltaSetIndex::new(5, 0), &scalars).err(),
            Some(VariationError::DeltaSetIndex { outer: 5, inner: 0 })
        );
    }
}

#[test]
fn hostile_item_stores_are_refused_by_name() {
    let good = store_bytes();
    let set16 = |bytes: &[u8], at: usize, value: u16| {
        let mut bytes = bytes.to_vec();
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        bytes
    };
    let refuse = |bytes: &[u8]| ItemVariationStore::parse(bytes).err();
    assert_eq!(
        refuse(&set16(&good, 0, 2)),
        Some(VariationError::StoreFormat { format: 2 })
    );
    assert_eq!(refuse(&good[..7]), Some(VariationError::StorePastEnd));
    assert_eq!(refuse(&good[..20]), Some(VariationError::StorePastEnd));
    // The region list follows the header's five offsets, at 28.
    assert_eq!(
        refuse(&set16(&good, 30, 0x8000)),
        Some(VariationError::RegionCount { count: 0x8000 })
    );
    assert_eq!(
        refuse(&set16(&good, 30, 0x7FFF)),
        Some(VariationError::RegionsPastEnd)
    );
    assert_eq!(
        refuse(&set16(&good, 28, 65)),
        Some(VariationError::TooManyAxes { count: 65 })
    );
    assert_eq!(refuse(&good[..30]), Some(VariationError::RegionsPastEnd));

    // A subtable naming a region past the list, more long deltas than
    // regions, and rows past the end.
    let mut naming = data_tables();
    if let Some(data) = naming[0].as_mut() {
        data.regions[2] = 9;
    }
    let bytes = item_store(2, &regions(), &naming);
    let store = ItemVariationStore::parse(&bytes).expect("parses");
    assert_eq!(
        store.data(0).err(),
        Some(VariationError::RegionIndex { index: 9, count: 6 })
    );
    let mut wordy = data_tables();
    if let Some(data) = wordy[1].as_mut() {
        data.word_count = 4;
    }
    let bytes = item_store(2, &regions(), &wordy);
    let store = ItemVariationStore::parse(&bytes).expect("parses");
    assert_eq!(
        store.data(1).err(),
        Some(VariationError::WordDeltaCount {
            outer: 1,
            words: 4,
            regions: 3
        })
    );
    let cut = &good[..good.len() - 1];
    let store = ItemVariationStore::parse(cut).expect("the header is whole");
    assert_eq!(
        store.data(4).err(),
        Some(VariationError::DataPastEnd { outer: 4 })
    );
    for cut in 0..=good.len() {
        if let Ok(store) = ItemVariationStore::parse(&good[..cut]) {
            let scalars = store.region_scalars(&Location::new(vec![8_192, -8_192]));
            for outer in 0..6 {
                for inner in 0..7 {
                    let _ = store.delta(DeltaSetIndex::new(outer, inner), &scalars);
                }
            }
        }
    }
}

#[test]
fn a_delta_set_index_map_maps_every_glyph() {
    let cases: Vec<(u8, u8, u8, Vec<(u32, u32)>)> = vec![
        (0, 1, 3, vec![(0, 5), (1, 0), (2, 3), (0, 0)]),
        (0, 1, 1, vec![(0, 1), (127, 0)]),
        (0, 2, 8, vec![(0, 200), (1, 0), (2, 255), (255, 17)]),
        (1, 3, 16, vec![(0, 65_535), (255, 0), (17, 1_234)]),
        (1, 4, 16, vec![(65_535, 65_535), (1_000, 2), (0, 0)]),
        (1, 2, 4, vec![(4_095, 15), (3, 9)]),
    ];
    for (format, size, bits, entries) in cases {
        let bytes = index_map(format, size, bits, &entries);
        let map = DeltaSetIndexMap::parse(&bytes).expect("a map");
        assert_eq!(map.format(), format);
        assert_eq!(map.len() as usize, entries.len());
        assert!(!map.is_empty());
        for item in 0..entries.len() + 3 {
            let (outer, inner) = entries[item.min(entries.len() - 1)];
            let index = map
                .get(u32::try_from(item).expect("few"))
                .expect("an index");
            assert_eq!(
                (u32::from(index.outer), u32::from(index.inner)),
                (outer, inner),
                "item {item}, past the count taking the last entry"
            );
        }
    }
    // The refusals: a format the chapter does not define, no entries,
    // entries past the end, and an outer index past sixteen bits.
    let mut unknown = index_map(0, 1, 3, &[(0, 1)]);
    unknown[0] = 2;
    assert_eq!(
        DeltaSetIndexMap::parse(&unknown).err(),
        Some(VariationError::MapFormat { format: 2 })
    );
    assert_eq!(
        DeltaSetIndexMap::parse(&index_map(0, 1, 3, &[])).err(),
        Some(VariationError::EmptyMap)
    );
    let entries = index_map(0, 2, 8, &[(0, 1), (1, 2)]);
    assert_eq!(
        DeltaSetIndexMap::parse(&entries[..entries.len() - 1]).err(),
        Some(VariationError::MapPastEnd)
    );
    assert_eq!(
        DeltaSetIndexMap::parse(&entries[..3]).err(),
        Some(VariationError::MapPastEnd)
    );
    let wide = index_map(1, 4, 1, &[(70_000, 1)]);
    let map = DeltaSetIndexMap::parse(&wide).expect("parses");
    assert_eq!(
        map.get(0).err(),
        Some(VariationError::MapEntryOuter { outer: 70_000 })
    );

    // With no advance map, HVAR takes the glyph id as the inner index of
    // the first subtable.
    let table = hvar(&store_bytes(), None, None, None);
    let hvar = Hvar::parse(&table, 2).expect("an HVAR");
    for glyph in [0u16, 3, 5, 900] {
        assert_eq!(hvar.advance_index(glyph), Ok(DeltaSetIndex::new(0, glyph)));
    }
}

/// The glyphs' horizontal metrics: four long ones and two bearings after.
fn horizontal_long() -> Vec<(u16, i16)> {
    vec![(500, 10), (600, 20), (700, -30), (800, 40)]
}

fn horizontal_bearings() -> Vec<i16> {
    vec![50, -60]
}

/// The advance map, into every kind of subtable: a 16-bit one, the 32-bit
/// one, the 8-bit one, the index of no variation, and a NULL subtable.
fn advance_entries() -> Vec<(u32, u32)> {
    vec![(1, 2), (0, 1), (2, 0), (4, 1), (0xFFFF, 0xFFFF), (3, 7)]
}

fn left_entries() -> Vec<(u32, u32)> {
    vec![(1, 0), (1, 1), (4, 0), (0, 5), (2, 1), (1, 2)]
}

/// Fewer entries than glyphs: the last two glyphs take the last entry.
fn right_entries() -> Vec<(u32, u32)> {
    vec![(0, 3), (4, 1), (0, 0), (1, 2)]
}

fn hvar_face(advance_map: bool) -> Vec<u8> {
    let advance = advance_map.then(|| index_map(0, 4, 16, &advance_entries()));
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"hhea", hhea(4, (1, 0, 0)))
        .with(
            *b"hmtx",
            metrics(&horizontal_long(), &horizontal_bearings()),
        )
        .with(
            *b"HVAR",
            hvar(
                &store_bytes(),
                advance,
                Some(index_map(0, 2, 8, &left_entries())),
                Some(index_map(0, 1, 3, &right_entries())),
            ),
        )
        .build()
}

/// Every location the metric tests read: each named instance, and user
/// coordinates no instance names.
fn metric_locations() -> Vec<(Instance, [f64; 2])> {
    let mut out: Vec<(Instance, [f64; 2])> = six_instances()
        .iter()
        .enumerate()
        .map(|(index, stated)| {
            (
                Instance::Named(u16::try_from(index).expect("few")),
                [stated.coordinates[0], stated.coordinates[1]],
            )
        })
        .collect();
    for weight in [100.0, 260.0, 400.0, 555.5, 650.0, 900.0] {
        for width in [50.0, 75.0, 100.0, 133.333, 200.0] {
            out.push((
                Instance::User(vec![
                    UserCoordinate::new(*b"wght", weight),
                    UserCoordinate::new(*b"wdth", width),
                ]),
                [weight, width],
            ));
        }
    }
    out
}

#[test]
fn hvar_advances_at_an_instance_equal_the_deltas_applied_in_the_test() {
    let ranges = axis_ranges();
    for advance_map in [false, true] {
        let face = hvar_face(advance_map);
        let sfnt = Sfnt::parse(&face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let hvar = Hvar::read(&sfnt, &space)
            .expect("an HVAR")
            .expect("it carries one");
        for (instance, user) in metric_locations() {
            let coordinates = spec::location(&ranges, &user);
            let location = space.location(&instance).expect("a location");
            assert_eq!(location.coordinates(), coordinates);
            let scalars = hvar.scalars(&location);
            for glyph in 0u16..6 {
                let default = i64::from(horizontal_long()[usize::from(glyph).min(3)].0);
                let (outer, inner) = if advance_map {
                    advance_entries()[usize::from(glyph)]
                } else {
                    (0, u32::from(glyph))
                };
                let expected = default * 65_536 + expected_delta(outer, inner, &coordinates);
                let advance = hvar
                    .advance_at(&sfnt, glyph, &location)
                    .expect("an advance")
                    .expect("hmtx gives one");
                assert_eq!(
                    advance.bits(),
                    expected,
                    "glyph {glyph} at {user:?}, map {advance_map}"
                );
                let (outer, inner) = left_entries()[usize::from(glyph)];
                assert_eq!(
                    hvar.left_side_bearing_delta(glyph, &scalars)
                        .expect("a delta")
                        .map(Fixed::bits),
                    Some(expected_delta(outer, inner, &coordinates))
                );
                let right = right_entries();
                let (outer, inner) = right[usize::from(glyph).min(right.len() - 1)];
                assert_eq!(
                    hvar.right_side_bearing_delta(glyph, &scalars)
                        .expect("a delta")
                        .map(Fixed::bits),
                    Some(expected_delta(outer, inner, &coordinates))
                );
            }
        }
        // At the default every scalar is zero, so the advance is hmtx's.
        let default = hvar
            .advance_at(&sfnt, 2, &space.default_location())
            .expect("an advance");
        assert_eq!(default, Some(Fixed::from_int(700)));
    }
    // Without side-bearing maps there are no side-bearing deltas.
    let bare = hvar(&store_bytes(), None, None, None);
    let hvar = Hvar::parse(&bare, 2).expect("an HVAR");
    let scalars = hvar.scalars(&Location::new(vec![16_384, 0]));
    assert_eq!(hvar.left_side_bearing_delta(0, &scalars), Ok(None));
    assert_eq!(hvar.right_side_bearing_delta(0, &scalars), Ok(None));
    // A face without HVAR, or that does not vary, reads none.
    let face = Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    assert!(Hvar::read(&sfnt, &space).expect("none").is_none());
    let face = Face::truetype().with(*b"HVAR", bare).build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("no axes");
    assert!(Hvar::read(&sfnt, &space).expect("none").is_none());
}

fn vertical_long() -> Vec<(u16, i16)> {
    vec![(1_000, 80), (1_100, 90), (1_200, 100)]
}

fn vertical_bearings() -> Vec<i16> {
    vec![110, 120, 130]
}

fn origins() -> Vec<(u16, i16)> {
    vec![(1, 900), (4, 870)]
}

fn vvar_face() -> Vec<u8> {
    Face::opentype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"vhea", vhea(3, (880, -120, 0), (0, 1, 0)))
        .with(*b"vmtx", metrics(&vertical_long(), &vertical_bearings()))
        .with(*b"VORG", vorg(880, &origins()))
        .with(
            *b"VVAR",
            vvar(
                &store_bytes(),
                None,
                Some(index_map(0, 2, 8, &left_entries())),
                Some(index_map(0, 1, 3, &right_entries())),
                Some(index_map(0, 4, 16, &advance_entries())),
            ),
        )
        .build()
}

#[test]
fn vvar_metrics_likewise() {
    let ranges = axis_ranges();
    let face = vvar_face();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let vvar = Vvar::read(&sfnt, &space)
        .expect("a VVAR")
        .expect("it carries one");
    for (instance, user) in metric_locations() {
        let coordinates = spec::location(&ranges, &user);
        let location = space.location(&instance).expect("a location");
        let scalars = vvar.scalars(&location);
        for glyph in 0u16..6 {
            let at = usize::from(glyph);
            // No advance map: the glyph id is the inner index.
            let default = i64::from(vertical_long()[at.min(2)].0);
            let expected = default * 65_536 + expected_delta(0, u32::from(glyph), &coordinates);
            let advance = vvar
                .advance_at(&sfnt, glyph, &location)
                .expect("an advance")
                .expect("vmtx gives one");
            assert_eq!(advance.bits(), expected, "glyph {glyph} at {user:?}");
            let (outer, inner) = left_entries()[at];
            assert_eq!(
                vvar.top_side_bearing_delta(glyph, &scalars)
                    .expect("a delta")
                    .map(Fixed::bits),
                Some(expected_delta(outer, inner, &coordinates))
            );
            let right = right_entries();
            let (outer, inner) = right[at.min(right.len() - 1)];
            assert_eq!(
                vvar.bottom_side_bearing_delta(glyph, &scalars)
                    .expect("a delta")
                    .map(Fixed::bits),
                Some(expected_delta(outer, inner, &coordinates))
            );
            let (outer, inner) = advance_entries()[at];
            let stated = origins()
                .iter()
                .find(|(g, _)| *g == glyph)
                .map_or(880, |(_, origin)| *origin);
            let origin = vvar
                .origin_at(&sfnt, glyph, &location)
                .expect("an origin")
                .expect("VORG gives one");
            assert_eq!(
                origin.bits(),
                i64::from(stated) * 65_536 + expected_delta(outer, inner, &coordinates),
                "glyph {glyph}'s origin at {user:?}"
            );
            assert_eq!(sfnt.vorg_origin(glyph), Some(stated));
        }
    }
    // No VORG: no origin to vary.
    let face = Face::opentype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"vhea", vhea(3, (880, -120, 0), (0, 1, 0)))
        .with(*b"vmtx", metrics(&vertical_long(), &vertical_bearings()))
        .with(
            *b"VVAR",
            variable_support::vvar(&store_bytes(), None, None, None, None),
        )
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let vvar = Vvar::read(&sfnt, &space).expect("a VVAR").expect("one");
    let location = Location::new(vec![16_384, 16_384]);
    assert_eq!(vvar.origin_at(&sfnt, 0, &location), Ok(None));
    assert_eq!(
        vvar.origin_delta(0, &vvar.scalars(&location)),
        Ok(None),
        "and no origin map"
    );
}

/// Every `OS/2` field `MVAR` names, with the value the test writes there.
fn os2_values() -> Vec<(Tag, usize, i32)> {
    vec![
        (*b"hasc", 68, 800),
        (*b"hdsc", 70, -200),
        (*b"hlgp", 72, 90),
        (*b"hcla", 74, 950),
        (*b"hcld", 76, 40_000),
        (*b"xhgt", 86, 480),
        (*b"cpht", 88, 700),
        (*b"sbxs", 10, 650),
        (*b"sbys", 12, 600),
        (*b"sbxo", 14, -3),
        (*b"sbyo", 16, 75),
        (*b"spxs", 18, 651),
        (*b"spys", 20, 601),
        (*b"spxo", 22, 4),
        (*b"spyo", 24, 350),
        (*b"strs", 26, 50),
        (*b"stro", 28, 250),
    ]
}

/// The default every value tag has in the face the MVAR test builds; `None`
/// for the `gasp` ranges it does not write.
fn stated_default(tag: Tag) -> Option<i32> {
    if let Some((_, _, value)) = os2_values().iter().find(|(t, _, _)| *t == tag) {
        return Some(*value);
    }
    let others: [(Tag, i32); 13] = [
        (*b"hcrs", 11),
        (*b"hcrn", 12),
        (*b"hcof", 13),
        (*b"vasc", 880),
        (*b"vdsc", -120),
        (*b"vlgp", 30),
        (*b"vcrs", 21),
        (*b"vcrn", 22),
        (*b"vcof", 23),
        (*b"undo", -100),
        (*b"unds", 50),
        (*b"gsp0", 8),
        (*b"gsp1", 16),
    ];
    if let Some((_, value)) = others.iter().find(|(t, _)| *t == tag) {
        return Some(*value);
    }
    (tag == *b"gsp2").then_some(65_535)
}

/// The value records, in binary order of their tags.
fn value_records() -> Vec<(Tag, u16, u16)> {
    vec![
        (*b"cpht", 1, 0),
        (*b"gsp0", 0, 1),
        (*b"gsp1", 4, 0),
        (*b"hasc", 0, 2),
        (*b"hcla", 2, 0),
        (*b"hcrs", 1, 1),
        (*b"hdsc", 0, 3),
        (*b"stro", 1, 2),
        (*b"undo", 2, 1),
        (*b"unds", 4, 1),
        (*b"vasc", 0, 4),
        (*b"vcof", 0, 5),
        (*b"xhgt", 3, 0),
    ]
}

fn mvar_face() -> Vec<u8> {
    let fields: Vec<(usize, u16)> = os2_values()
        .iter()
        .map(|(_, at, value)| (*at, *value as u16))
        .collect();
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"OS/2", os2(&fields))
        .with(*b"hhea", hhea(1, (11, 12, 13)))
        .with(*b"vhea", vhea(1, (880, -120, 30), (21, 22, 23)))
        .with(*b"post", post(-100, 50))
        .with(*b"gasp", gasp(&[(8, 2), (16, 7), (0xFFFF, 15)]))
        .with(*b"MVAR", mvar(&store_bytes(), &value_records()))
        .build()
}

#[test]
fn mvar_values_by_tag_likewise() {
    let ranges = axis_ranges();
    let face = mvar_face();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let mvar = Mvar::read(&sfnt, &space)
        .expect("an MVAR")
        .expect("it carries one");
    let tags: Vec<Tag> = value_records().iter().map(|(tag, _, _)| *tag).collect();
    assert_eq!(mvar.tags(), tags);
    assert_eq!(
        METRIC_FIELDS.len(),
        38,
        "the chapter's 28 metrics and ten ranges"
    );
    for field in METRIC_FIELDS {
        assert_eq!(metric_default(&sfnt, field.tag), stated_default(field.tag));
    }
    for (instance, user) in metric_locations() {
        let coordinates = spec::location(&ranges, &user);
        let location = space.location(&instance).expect("a location");
        for field in METRIC_FIELDS {
            let record = value_records()
                .into_iter()
                .find(|(tag, _, _)| *tag == field.tag);
            let value = mvar.value_at(&sfnt, field.tag, &location).expect("a value");
            let Some(default) = stated_default(field.tag) else {
                assert_eq!(value, None, "no default to vary");
                continue;
            };
            let delta = record.map_or(0, |(_, outer, inner)| {
                expected_delta(u32::from(outer), u32::from(inner), &coordinates)
            });
            assert_eq!(
                value.map(Fixed::bits),
                Some(i64::from(default) * 65_536 + delta),
                "{} at {user:?}",
                String::from_utf8_lossy(&field.tag)
            );
        }
    }
    // A tag no record names is constant, and one the chapter does not
    // define has no default.
    let location = Location::new(vec![16_384, 16_384]);
    assert_eq!(mvar.delta(*b"hlgp", &mvar.scalars(&location)), Ok(None));
    assert_eq!(mvar.index(*b"hlgp"), None);
    assert_eq!(mvar.value_at(&sfnt, *b"ZZZZ", &location), Ok(None));
    // A table with no records has no store and varies nothing.
    let empty = variable_support::mvar(&store_bytes(), &[]);
    let table = Mvar::parse(&empty, 2).expect("an empty MVAR");
    assert!(table.store().is_none());
    assert_eq!(
        table.value_at(&sfnt, *b"hasc", &location),
        Ok(Some(Fixed::from_int(800)))
    );
}

#[test]
fn hostile_metrics_tables_are_refused_by_name() {
    let set = |bytes: &[u8], at: usize, value: &[u8]| {
        let mut bytes = bytes.to_vec();
        bytes[at..at + value.len()].copy_from_slice(value);
        bytes
    };
    let good = hvar(&store_bytes(), None, None, None);
    assert_eq!(
        Hvar::parse(&set(&good, 0, &[0, 2]), 2).err(),
        Some(VariationError::UnknownVersion {
            table: *b"HVAR",
            major: 2,
            minor: 0
        })
    );
    assert_eq!(
        Hvar::parse(&good[..10], 2).err(),
        Some(VariationError::HeaderPastEnd { table: *b"HVAR" })
    );
    assert_eq!(
        Hvar::parse(&set(&good, 4, &[0, 0, 0, 0]), 2).err(),
        Some(VariationError::NoStore { table: *b"HVAR" })
    );
    assert_eq!(
        Hvar::parse(&good, 3).err(),
        Some(VariationError::AxisCount {
            table: *b"HVAR",
            found: 2,
            expected: 3
        })
    );
    let format_one = hvar(
        &store_bytes(),
        Some(index_map(1, 2, 8, &[(0, 1)])),
        None,
        None,
    );
    assert_eq!(
        Hvar::parse(&format_one, 2).err(),
        Some(VariationError::MapFormatIn {
            table: *b"HVAR",
            format: 1
        })
    );
    let vvar_table = vvar(&store_bytes(), None, None, None, None);
    assert_eq!(
        Vvar::parse(&vvar_table[..20], 2).err(),
        Some(VariationError::HeaderPastEnd { table: *b"VVAR" })
    );
    assert_eq!(
        Vvar::parse(&vvar_table, 1).err(),
        Some(VariationError::AxisCount {
            table: *b"VVAR",
            found: 2,
            expected: 1
        })
    );
    let mvar_table = mvar(&store_bytes(), &value_records());
    assert_eq!(
        Mvar::parse(&set(&mvar_table, 6, &[0, 6]), 2).err(),
        Some(VariationError::ValueRecordSize { size: 6 })
    );
    assert_eq!(
        Mvar::parse(&mvar_table[..40], 2).err(),
        Some(VariationError::ValueRecordsPastEnd { count: 13 })
    );
    assert_eq!(
        Mvar::parse(&set(&mvar_table, 10, &[0, 0]), 2).err(),
        Some(VariationError::NoStore { table: *b"MVAR" })
    );
    assert_eq!(
        Mvar::parse(&mvar_table[..11], 2).err(),
        Some(VariationError::HeaderPastEnd { table: *b"MVAR" })
    );
    // A record naming a delta set past the store is refused when read.
    let past = mvar(&store_bytes(), &[(*b"hasc", 9, 0)]);
    let table = Mvar::parse(&past, 2).expect("parses");
    assert_eq!(
        table.delta(*b"hasc", &table.scalars(&Location::new(vec![0, 0]))),
        Err(VariationError::DeltaSetIndex { outer: 9, inner: 0 })
    );
}

#[test]
fn every_truncation_and_byte_change_of_a_metrics_table_is_read_or_refused_without_panicking() {
    let location = Location::new(vec![6_144, -9_000]);
    let hvar_table = hvar(
        &store_bytes(),
        Some(index_map(0, 4, 16, &advance_entries())),
        Some(index_map(0, 2, 8, &left_entries())),
        Some(index_map(0, 1, 3, &right_entries())),
    );
    let mvar_table = mvar(&store_bytes(), &value_records());
    let mut variants: Vec<Vec<u8>> = Vec::new();
    for cut in 0..=hvar_table.len() {
        variants.push(hvar_table[..cut].to_vec());
    }
    for at in 0..hvar_table.len() {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut bytes = hvar_table.clone();
            bytes[at] = value;
            variants.push(bytes);
        }
    }
    for bytes in &variants {
        if let Ok(hvar) = Hvar::parse(bytes, 2) {
            let scalars = hvar.scalars(&location);
            for glyph in 0..8 {
                let _ = hvar.advance_delta(glyph, &scalars);
                let _ = hvar.left_side_bearing_delta(glyph, &scalars);
                let _ = hvar.right_side_bearing_delta(glyph, &scalars);
            }
        }
        if let Ok(vvar) = Vvar::parse(bytes, 2) {
            let scalars = vvar.scalars(&location);
            for glyph in 0..8 {
                let _ = vvar.advance_delta(glyph, &scalars);
                let _ = vvar.origin_delta(glyph, &scalars);
            }
        }
    }
    for cut in 0..=mvar_table.len() {
        if let Ok(mvar) = Mvar::parse(&mvar_table[..cut], 2) {
            let scalars = mvar.scalars(&location);
            for field in METRIC_FIELDS {
                let _ = mvar.delta(field.tag, &scalars);
            }
        }
    }
    for at in 0..mvar_table.len() {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut bytes = mvar_table.clone();
            bytes[at] = value;
            if let Ok(mvar) = Mvar::parse(&bytes, 2) {
                let scalars = mvar.scalars(&location);
                for field in METRIC_FIELDS {
                    let _ = mvar.delta(field.tag, &scalars);
                }
                let _ = mvar.tags();
            }
        }
    }
}

// ---- CFF2 (FT-05.6) -------------------------------------------------------------

/// A charstring from its items.
fn cs(items: &[Cs]) -> Vec<u8> {
    charstring(items)
}

/// Numbers then an operator.
fn call(numbers: &[i32], operator: u8) -> Vec<Cs> {
    let mut items = ints(numbers);
    items.push(Cs::Op(operator));
    items
}

/// A box `size` units on a side from the current point, drawn with
/// `rlineto`.
fn box_subr(size: i32) -> Vec<u8> {
    let mut items = call(&[0, 0], op::RMOVETO);
    items.extend(call(&[size, 0, 0, size, -size, 0], op::RLINETO));
    cs(&items)
}

/// The segments a box `size` units on a side at the origin draws.
fn box_segments(size: f64) -> Vec<Segment> {
    vec![
        Segment::MoveTo { x: 0.0, y: 0.0 },
        Segment::LineTo { x: size, y: 0.0 },
        Segment::LineTo { x: size, y: size },
        Segment::LineTo { x: 0.0, y: size },
        Segment::Close,
    ]
}

/// The glyphs of the plain CFF2 face: `.notdef`; local subroutine 0; global
/// subroutine 0, a curve; global subroutine 1, which calls local subroutine
/// 0; a hint mask over nine stems, one of them implicit; local subroutine 0
/// again and global subroutine 1 again, under the other Font DICT; and the
/// curve operators.
fn plain_charstrings() -> Vec<Vec<u8>> {
    let mut hinted = ints(&[
        10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
    ]);
    hinted.push(Cs::Op(op::HSTEMHM));
    hinted.extend(ints(&[170, 180]));
    hinted.push(Cs::Op(op::HINTMASK));
    // Nine stems: two mask bytes. Read as one, the second, 0x80, would be
    // the operand -11 and move the contour.
    hinted.push(Cs::Bytes(vec![0xFF, 0x80]));
    hinted.extend(call(&[0, 0], op::RMOVETO));
    hinted.extend(call(&[30, 0, 0, 30], op::RLINETO));
    let mut curves = call(&[0, 0], op::RMOVETO);
    curves.extend(call(&[10, 20, 30, 40], op::HVCURVETO));
    curves.extend(ints(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
    curves.push(Cs::Esc(op::FLEX));
    vec![
        Vec::new(),
        cs(&call(&[-107], op::CALLSUBR)),
        cs(&call(&[-107], op::CALLGSUBR)),
        cs(&call(&[-106], op::CALLGSUBR)),
        cs(&hinted),
        cs(&call(&[-107], op::CALLSUBR)),
        cs(&call(&[-106], op::CALLGSUBR)),
        cs(&curves),
    ]
}

/// The two global subroutines: a curve, and a call to local subroutine 0 —
/// whichever Font DICT's the calling glyph runs under.
fn plain_global_subrs() -> Vec<Vec<u8>> {
    let mut curve = call(&[50, 50], op::RMOVETO);
    curve.extend(call(&[10, 20, 30, 40, 50, 60], op::RRCURVETO));
    vec![cs(&curve), cs(&call(&[-107], op::CALLSUBR))]
}

/// A face with two Font DICTs, whose local subroutine 0 draws a box of 100
/// and of 200, and the FDSelect given.
fn plain_cff2(fd_select: Option<FdSelectSpec>) -> Vec<u8> {
    Cff2Spec {
        charstrings: plain_charstrings(),
        global_subrs: plain_global_subrs(),
        privates: vec![
            PrivateSpec {
                dict: Vec::new(),
                subrs: vec![box_subr(100)],
            },
            PrivateSpec {
                dict: Vec::new(),
                subrs: vec![box_subr(200)],
            },
        ],
        fd_select,
        store: None,
    }
    .build()
}

/// The Font DICT each glyph of the plain face takes: glyphs 5 to 7 the
/// second.
fn plain_fds() -> Vec<u8> {
    vec![0, 0, 0, 0, 0, 1, 1, 1]
}

/// The FDSelect of each format, all saying [`plain_fds`].
fn fd_selects() -> Vec<FdSelectSpec> {
    vec![
        FdSelectSpec::Format0(plain_fds()),
        FdSelectSpec::Format3(vec![(0, 0), (5, 1)], 8),
        FdSelectSpec::Format4(vec![(0, 0), (2, 0), (5, 1)], 8),
    ]
}

/// What each glyph of the plain face draws, the box sizes by its Font DICT.
fn plain_expected(fds: &[u8]) -> Vec<Vec<Segment>> {
    let local = |glyph: usize| box_segments(if fds[glyph] == 0 { 100.0 } else { 200.0 });
    let curve = vec![
        Segment::MoveTo { x: 50.0, y: 50.0 },
        Segment::CurveTo {
            c1x: 60.0,
            c1y: 70.0,
            c2x: 90.0,
            c2y: 110.0,
            x: 140.0,
            y: 170.0,
        },
        Segment::Close,
    ];
    let hinted = vec![
        Segment::MoveTo { x: 0.0, y: 0.0 },
        Segment::LineTo { x: 30.0, y: 0.0 },
        Segment::LineTo { x: 30.0, y: 30.0 },
        Segment::Close,
    ];
    // hvcurveto 10 20 30 40: horizontal first, so (10, 0), then (20, 30),
    // then (0, 40); flex's two curves from there.
    let curves = vec![
        Segment::MoveTo { x: 0.0, y: 0.0 },
        Segment::CurveTo {
            c1x: 10.0,
            c1y: 0.0,
            c2x: 30.0,
            c2y: 30.0,
            x: 30.0,
            y: 70.0,
        },
        Segment::CurveTo {
            c1x: 31.0,
            c1y: 72.0,
            c2x: 34.0,
            c2y: 76.0,
            x: 39.0,
            y: 82.0,
        },
        Segment::CurveTo {
            c1x: 46.0,
            c1y: 90.0,
            c2x: 55.0,
            c2y: 100.0,
            x: 66.0,
            y: 112.0,
        },
        Segment::Close,
    ];
    vec![
        Vec::new(),
        local(1),
        curve,
        local(3),
        hinted,
        local(5),
        local(6),
        curves,
    ]
}

#[test]
fn a_cff2_program_outlines_its_glyphs() {
    for fd_select in fd_selects() {
        let table = plain_cff2(Some(fd_select.clone()));
        let cff2 = Cff2::parse(&table).expect("a CFF2 table");
        assert_eq!(cff2.glyph_count(), 8);
        assert_eq!(cff2.font_dict_count(), 2);
        assert_eq!(cff2.font_matrix(), [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert!(cff2.variation_store().is_none());
        let expected = plain_expected(&plain_fds());
        let mut outline = Outline::default();
        let mut machine = Machine::new();
        for glyph in 0u16..8 {
            let fd = u16::from(plain_fds()[usize::from(glyph)]);
            assert_eq!(cff2.font_dict_for(glyph), Ok(fd), "{fd_select:?}");
            let drawn = cff2.outline(glyph).expect("an outline");
            assert_eq!(
                drawn.segments,
                expected[usize::from(glyph)],
                "glyph {glyph} under {fd_select:?}"
            );
            // The same segments through one reused machine and buffer.
            cff2.outline_into(glyph, &mut outline, &mut machine)
                .expect("an outline");
            assert_eq!(outline.segments, expected[usize::from(glyph)]);
        }
    }
    // With no FDSelect every glyph is in Font DICT 0, so the glyphs that
    // called the second's subroutine draw the first's.
    let table = plain_cff2(None);
    let cff2 = Cff2::parse(&table).expect("a CFF2 table");
    let expected = plain_expected(&[0; 8]);
    for glyph in 0u16..8 {
        assert_eq!(cff2.font_dict_for(glyph), Ok(0));
        assert_eq!(
            cff2.outline(glyph).expect("an outline").segments,
            expected[usize::from(glyph)]
        );
    }
    assert_eq!(
        cff2.outline(8).err(),
        Some(Cff2Error::NoSuchGlyph { glyph: 8 })
    );
}

/// Two item variation data subtables, as CFF2 holds them: region indices
/// and no rows. The first names two regions, the second three.
fn cff2_store() -> Vec<u8> {
    item_store(
        2,
        &regions(),
        &[
            Some(DataSpec {
                regions: vec![0, 2],
                word_count: 0,
                long_words: false,
                rows: Vec::new(),
            }),
            Some(DataSpec {
                regions: vec![1, 3, 4],
                word_count: 0,
                long_words: false,
                rows: Vec::new(),
            }),
        ],
    )
}

/// Items `n` defaults, their deltas a group each, `n`, then `blend`.
fn blended(defaults: &[i32], deltas: &[&[i32]]) -> Vec<Cs> {
    let mut items = ints(defaults);
    for group in deltas {
        items.extend(ints(group));
    }
    items.push(Cs::Int(i32::try_from(defaults.len()).expect("few")));
    items.push(Cs::Op(op::BLEND));
    items
}

/// The glyphs of the blended face: two values blended at once; a glyph that
/// selects the second data subtable; a blend inside a local subroutine; a
/// glyph under the Font DICT whose Private DICT selects the second subtable;
/// and a blend inside a global subroutine.
fn blend_charstrings() -> Vec<Vec<u8>> {
    let mut first = blended(&[100, 200], &[&[20, 30], &[40, 50]]);
    first.push(Cs::Op(op::RMOVETO));
    first.extend(blended(&[300], &[&[-10, 5]]));
    first.push(Cs::Op(op::HLINETO));

    let mut second = call(&[1], op::VSINDEX);
    second.extend(blended(&[50], &[&[1, 2, 3]]));
    second.extend(call(&[0], op::RMOVETO));
    second.extend(call(&[20, 30], op::RLINETO));

    let mut fourth = blended(&[70, 80], &[&[1, 1, 1], &[2, 2, 2]]);
    fourth.push(Cs::Op(op::RMOVETO));

    let mut fifth = call(&[-107], op::CALLGSUBR);
    fifth.extend(call(&[10], op::HLINETO));
    vec![
        cs(&first),
        cs(&second),
        cs(&call(&[-107], op::CALLSUBR)),
        cs(&fourth),
        cs(&fifth),
    ]
}

/// The local subroutine of the first Font DICT: a blended move and line.
fn blend_local_subr() -> Vec<u8> {
    let mut items = blended(&[10, 20], &[&[5, 5], &[6, 6]]);
    items.push(Cs::Op(op::RMOVETO));
    items.extend(blended(&[40], &[&[-2, 2]]));
    items.push(Cs::Op(op::VLINETO));
    cs(&items)
}

/// The global subroutine: a blended move, read under the caller's
/// `vsindex`.
fn blend_global_subr() -> Vec<u8> {
    let mut items = blended(&[5], &[&[1, 1]]);
    items.extend(call(&[5], op::RMOVETO));
    cs(&items)
}

/// A Private DICT entry of 32-bit operands, after `blend`'s own.
fn blended_entry(defaults: &[i32], deltas: &[&[i32]], operator: u16) -> Vec<u8> {
    let mut operands: Vec<i32> = defaults.to_vec();
    for group in deltas {
        operands.extend_from_slice(group);
    }
    operands.push(i32::try_from(defaults.len()).expect("few"));
    let mut out = dict_entry(23, &operands);
    out.extend(dict_entry(operator, &[]));
    out
}

/// The Private DICTs: the first blends `BlueValues` and `StdHW` over the
/// first subtable; the second selects the second subtable and blends
/// `StdVW` over it.
fn blend_privates() -> Vec<PrivateSpec> {
    let mut first = blended_entry(
        &[-20, 0, 500, 520],
        &[&[10, 0], &[0, 0], &[-5, -5], &[2, 2]],
        6,
    );
    first.extend(blended_entry(&[50], &[&[10, -5]], 10));
    let mut second = dict_entry(22, &[1]);
    second.extend(blended_entry(&[80], &[&[1, 2, 3]], 11));
    vec![
        PrivateSpec {
            dict: first,
            subrs: vec![blend_local_subr()],
        },
        PrivateSpec {
            dict: second,
            subrs: Vec::new(),
        },
    ]
}

fn blend_cff2() -> Vec<u8> {
    Cff2Spec {
        charstrings: blend_charstrings(),
        global_subrs: vec![blend_global_subr()],
        privates: blend_privates(),
        fd_select: Some(FdSelectSpec::Format3(vec![(0, 0), (3, 1), (4, 0)], 5)),
        store: Some(cff2_store()),
    }
    .build()
}

/// What each blended glyph draws at the default: every blend its defaults.
fn blend_defaults() -> Vec<Vec<Segment>> {
    vec![
        vec![
            Segment::MoveTo { x: 100.0, y: 200.0 },
            Segment::LineTo { x: 400.0, y: 200.0 },
            Segment::Close,
        ],
        vec![
            Segment::MoveTo { x: 50.0, y: 0.0 },
            Segment::LineTo { x: 70.0, y: 30.0 },
            Segment::Close,
        ],
        vec![
            Segment::MoveTo { x: 10.0, y: 20.0 },
            Segment::LineTo { x: 10.0, y: 60.0 },
            Segment::Close,
        ],
        vec![Segment::MoveTo { x: 70.0, y: 80.0 }, Segment::Close],
        vec![
            Segment::MoveTo { x: 5.0, y: 5.0 },
            Segment::LineTo { x: 15.0, y: 5.0 },
            Segment::Close,
        ],
    ]
}

#[test]
fn a_cff2_default_instance_ignores_its_blends() {
    let table = blend_cff2();
    let cff2 = Cff2::parse(&table).expect("a CFF2 table");
    let store = cff2.variation_store().expect("a variation store");
    assert_eq!(store.data_count(), 2);
    assert_eq!(cff2.private_vsindex(3), Ok(1));
    assert_eq!(cff2.private_vsindex(0), Ok(0));
    for (glyph, expected) in blend_defaults().iter().enumerate() {
        let glyph = u16::try_from(glyph).expect("few");
        assert_eq!(
            &cff2.outline(glyph).expect("an outline").segments,
            expected,
            "glyph {glyph} at the default"
        );
    }
    // The Private DICTs' blends keep their defaults too.
    let first = cff2.private_dict(0).expect("a Private DICT");
    assert!(first.contains(&(6, vec![-20.0, 0.0, 500.0, 520.0])));
    assert!(first.contains(&(10, vec![50.0])));
    let second = cff2.private_dict(1).expect("a Private DICT");
    assert_eq!(second, vec![(22, vec![1.0]), (11, vec![80.0])]);
    assert_eq!(
        cff2.private_dict(2).err(),
        Some(Cff2Error::FontDict {
            glyph: 0,
            font_dict: 2
        })
    );
}

/// One charstring in a face with one Font DICT and no store.
fn single_glyph(charstring: Vec<u8>, store: Option<Vec<u8>>) -> Vec<u8> {
    Cff2Spec {
        charstrings: vec![charstring],
        global_subrs: Vec::new(),
        privates: vec![PrivateSpec::default()],
        fd_select: None,
        store,
    }
    .build()
}

#[test]
fn a_hostile_cff2_is_refused_by_name() {
    let refuse = |table: &[u8]| Cff2::parse(table).err();
    let good = plain_cff2(Some(FdSelectSpec::Format0(plain_fds())));
    assert_eq!(refuse(&good[..4]), Some(Cff2Error::Header));
    let mut version = good.clone();
    version[0] = 3;
    assert_eq!(
        refuse(&version),
        Some(Cff2Error::Version { major: 3, minor: 0 })
    );
    let mut long_top = good.clone();
    long_top[3..5].copy_from_slice(&0x7FFFu16.to_be_bytes());
    assert_eq!(refuse(&long_top), Some(Cff2Error::TopDict));
    // A Top DICT with no CharStrings, or no Font DICTs.
    let top_only = |entries: Vec<u8>| {
        let mut out = vec![2, 0, 5];
        out.extend_from_slice(&u16::try_from(entries.len()).expect("small").to_be_bytes());
        out.extend(entries);
        out.extend(cff2_index(&[]));
        out
    };
    assert_eq!(
        refuse(&top_only(dict_entry(0x0C24, &[0]))),
        Some(Cff2Error::NoCharStrings)
    );
    // The header's five bytes, a six-byte Top DICT and an empty Global Subr
    // INDEX put the CharStrings INDEX at 15.
    let charstrings_only = {
        let mut table = top_only(dict_entry(17, &[15]));
        table.extend(cff2_index(&[vec![]]));
        table
    };
    assert_eq!(refuse(&charstrings_only), Some(Cff2Error::NoFontDicts));
    // A CharStrings INDEX counting past 65 536, and one cut short.
    let huge = {
        let mut table = top_only(dict_entry(17, &[15]));
        table.extend_from_slice(&65_537u32.to_be_bytes());
        table
    };
    assert_eq!(
        refuse(&huge),
        Some(Cff2Error::IndexCount {
            which: Cff2Index::CharStrings,
            count: 65_537
        })
    );
    let cut = {
        let mut table = top_only(dict_entry(17, &[15]));
        table.extend_from_slice(&3u32.to_be_bytes());
        table.push(4);
        table
    };
    assert_eq!(
        refuse(&cut),
        Some(Cff2Error::Index {
            which: Cff2Index::CharStrings
        })
    );
    // An FDSelect of an undefined format, one not starting at glyph 0, one
    // whose sentinel stops short of the glyphs, and one out of order.
    for fd_select in [
        FdSelectSpec::Format4(vec![(1, 0)], 8),
        FdSelectSpec::Format3(vec![(0, 0), (5, 1)], 6),
        FdSelectSpec::Format3(vec![(0, 0), (5, 1), (5, 0)], 8),
    ] {
        assert_eq!(
            refuse(&plain_cff2(Some(fd_select.clone()))),
            Some(Cff2Error::FdSelect),
            "{fd_select:?}"
        );
    }
    let mut unknown = plain_cff2(Some(FdSelectSpec::Format0(plain_fds())));
    let at = 5
        + Cff2Spec {
            charstrings: plain_charstrings(),
            global_subrs: plain_global_subrs(),
            privates: Vec::new(),
            fd_select: Some(FdSelectSpec::Format0(plain_fds())),
            store: None,
        }
        .top(0, 0, 0, 0)
        .len()
        + cff2_index(&plain_global_subrs()).len();
    assert_eq!(unknown[at], 0, "the FDSelect's format byte");
    unknown[at] = 2;
    assert_eq!(refuse(&unknown), Some(Cff2Error::FdSelect));
    // A glyph whose FDSelect entry names a Font DICT past the INDEX.
    let mut fds = plain_fds();
    fds[4] = 7;
    let table = plain_cff2(Some(FdSelectSpec::Format0(fds)));
    let cff2 = Cff2::parse(&table).expect("parses");
    assert_eq!(
        cff2.outline(4).err(),
        Some(Cff2Error::FontDict {
            glyph: 4,
            font_dict: 7
        })
    );
    // Private DICTs that together read more than the table holds: both Font
    // DICTs' pointed at nearly the whole table, at different offsets. With no
    // store and no FDSelect, the Font DICT INDEX follows the CharStrings
    // INDEX, and its two entries follow its count, offSize and three offsets.
    let overlapping = {
        let spec = Cff2Spec {
            charstrings: plain_charstrings(),
            global_subrs: plain_global_subrs(),
            privates: Vec::new(),
            fd_select: None,
            store: None,
        };
        let font_dicts_at = 5
            + spec.top(0, 0, 0, 0).len()
            + cff2_index(&spec.global_subrs).len()
            + cff2_index(&spec.charstrings).len();
        let mut table = plain_cff2(None);
        let size = i32::try_from(table.len() - 2).expect("small");
        for (number, offset) in [(0usize, 0i32), (1, 1)] {
            let entry = font_dicts_at + 5 + 4 * 3 + number * 11;
            table[entry..entry + 11].copy_from_slice(&dict_entry(18, &[size, offset]));
        }
        table
    };
    assert_eq!(refuse(&overlapping), Some(Cff2Error::PrivateDictsOverlap));
    // The local subroutines count too: a second Private DICT that is the
    // first's six bytes and one more, so the two are distinct and reach one
    // Local Subr INDEX larger than half the table.
    let shared_subrs = {
        let spec = Cff2Spec {
            charstrings: vec![Vec::new()],
            global_subrs: Vec::new(),
            privates: vec![
                PrivateSpec {
                    dict: Vec::new(),
                    subrs: (0..200).map(|_| vec![op::RMOVETO; 4]).collect(),
                },
                PrivateSpec::default(),
            ],
            fd_select: None,
            store: None,
        };
        let font_dicts_at = 5
            + spec.top(0, 0, 0, 0).len()
            + cff2_index(&spec.global_subrs).len()
            + cff2_index(&spec.charstrings).len();
        // Two eleven-byte entries after a count, an offSize and three offsets.
        let first_private = font_dicts_at + 5 + 4 * 3 + 2 * 11;
        let mut table = spec.build();
        assert!(
            cff2_index(&spec.privates[0].subrs).len() * 2 > table.len(),
            "the INDEX is more than half the table"
        );
        let entry = font_dicts_at + 5 + 4 * 3 + 11;
        let at = i32::try_from(first_private).expect("small");
        table[entry..entry + 11].copy_from_slice(&dict_entry(18, &[7, at]));
        table
    };
    assert_eq!(refuse(&shared_subrs), Some(Cff2Error::PrivateDictsOverlap));

    // The charstring bounds.
    let outline_of = |charstring: Vec<u8>, store: Option<Vec<u8>>| {
        let table = single_glyph(charstring, store);
        Cff2::parse(&table).expect("parses").outline(0).err()
    };
    let mut overflow: Vec<Cs> = (0..514).map(|_| Cs::Int(1)).collect();
    overflow.push(Cs::Op(op::RLINETO));
    assert_eq!(
        outline_of(cs(&overflow), None),
        Some(Cff2Error::StackOverflow)
    );
    let mut full: Vec<Cs> = (0..512).map(|_| Cs::Int(1)).collect();
    full.extend(ints(&[0]));
    full.push(Cs::Op(op::RLINETO));
    assert_eq!(
        outline_of(cs(&full), None),
        None,
        "513 operands is the limit, not past it"
    );
    assert_eq!(
        outline_of(cs(&blended(&[1], &[&[2, 3]])), None),
        Some(Cff2Error::NoVariationStore)
    );
    assert_eq!(
        outline_of(cs(&call(&[0], op::VSINDEX)), None),
        Some(Cff2Error::NoVariationStore)
    );
    let store = Some(cff2_store());
    assert_eq!(
        outline_of(cs(&call(&[1, 2], op::BLEND)), store.clone()),
        Some(Cff2Error::BlendOperands),
        "n = 2 under k = 2 needs six more operands"
    );
    let mut fractional = vec![Cs::Int(1), Cs::Fixed(0x8000)];
    fractional.push(Cs::Op(op::BLEND));
    assert_eq!(
        outline_of(cs(&fractional), store.clone()),
        Some(Cff2Error::BlendOperands),
        "half a value is no n"
    );
    assert_eq!(
        outline_of(cs(&call(&[5], op::VSINDEX)), store.clone()),
        Some(Cff2Error::VsindexRange)
    );
    let mut twice = call(&[0], op::VSINDEX);
    twice.extend(call(&[1], op::VSINDEX));
    assert_eq!(
        outline_of(cs(&twice), store.clone()),
        Some(Cff2Error::VsindexMisplaced)
    );
    let mut late = blended(&[1], &[&[2, 3]]);
    late.extend(call(&[0], op::VSINDEX));
    assert_eq!(
        outline_of(cs(&late), store.clone()),
        Some(Cff2Error::VsindexMisplaced)
    );
    assert_eq!(
        outline_of(cs(&[Cs::Op(op::VSINDEX)]), store.clone()),
        Some(Cff2Error::VsindexOperand)
    );

    // Subroutines nested past ten levels: a global subroutine calling
    // itself.
    let recursive = Cff2Spec {
        charstrings: vec![cs(&call(&[-107], op::CALLGSUBR))],
        global_subrs: vec![cs(&call(&[-107], op::CALLGSUBR))],
        privates: vec![PrivateSpec::default()],
        fd_select: None,
        store: None,
    }
    .build();
    assert_eq!(
        Cff2::parse(&recursive).expect("parses").outline(0).err(),
        Some(Cff2Error::SubroutineDepth)
    );
    // The operation budget: nine levels each calling the next twice, and a
    // leaf of two hundred operators — 102 400 in all.
    let mut levels: Vec<Vec<u8>> = (0..9)
        .map(|level: i32| {
            let mut items = call(&[level + 1 - 107], op::CALLGSUBR);
            items.extend(call(&[level + 1 - 107], op::CALLGSUBR));
            cs(&items)
        })
        .collect();
    levels.push(vec![op::RMOVETO; 200]);
    let expensive = Cff2Spec {
        charstrings: vec![cs(&call(&[-107], op::CALLGSUBR))],
        global_subrs: levels,
        privates: vec![PrivateSpec::default()],
        fd_select: None,
        store: None,
    }
    .build();
    assert_eq!(
        Cff2::parse(&expensive).expect("parses").outline(0).err(),
        Some(Cff2Error::Budget)
    );
    // A Private DICT naming item variation data the store does not have.
    let bad_private = Cff2Spec {
        charstrings: vec![Vec::new()],
        global_subrs: Vec::new(),
        privates: vec![PrivateSpec {
            dict: dict_entry(22, &[4]),
            subrs: Vec::new(),
        }],
        fd_select: None,
        store: Some(cff2_store()),
    }
    .build();
    assert_eq!(refuse(&bad_private), Some(Cff2Error::VsindexRange));
    // Every refusal says what it is.
    for error in [
        Cff2Error::Header,
        Cff2Error::IndexCount {
            which: Cff2Index::LocalSubrs,
            count: 70_000,
        },
        Cff2Error::VariationStore(VariationError::StorePastEnd),
        Cff2Error::Budget,
    ] {
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn garbage_cff2_is_refused_without_panicking() {
    for garbage in [
        &[][..],
        &[2][..],
        &[2, 0, 5][..],
        &[2, 0, 5, 0, 0][..],
        &[2, 0, 5, 0, 1, 17][..],
        &[0xFF; 64][..],
        &b"OTTO but not a CFF2 table at all"[..],
    ] {
        if let Ok(cff2) = Cff2::parse(garbage) {
            for glyph in 0..4 {
                let _ = cff2.outline(glyph);
            }
        }
    }
    let faces = [
        plain_cff2(Some(FdSelectSpec::Format3(vec![(0, 0), (5, 1)], 8))),
        blend_cff2(),
    ];
    for face in &faces {
        for cut in 0..=face.len() {
            if let Ok(cff2) = Cff2::parse(&face[..cut]) {
                for glyph in 0..9 {
                    let _ = cff2.outline(glyph);
                }
                let _ = cff2.private_dict(0);
            }
        }
        for at in 0..face.len() {
            for value in [0x00, 0x0A, 0x0F, 0x10, 0x1D, 0x7F, 0x8B, 0xFF] {
                let mut bytes = face.clone();
                bytes[at] = value;
                if let Ok(cff2) = Cff2::parse(&bytes) {
                    let mut outline = Outline::default();
                    let mut machine = Machine::new();
                    for glyph in 0..9 {
                        let _ = cff2.outline_into(glyph, &mut outline, &mut machine);
                    }
                    let _ = cff2.private_dict(0);
                    let _ = cff2.private_dict(1);
                }
            }
        }
    }
}

// ---- CFF2 blends at an instance (FT-05.7) -----------------------------------------

/// The regions each of the blended face's item variation data subtables
/// names, as [`cff2_store`] writes them.
fn cff2_data_regions(vsindex: usize) -> Vec<u16> {
    [vec![0, 2], vec![1, 3, 4]][vsindex].clone()
}

/// A blended value at 2.14 coordinates, as the CFF2 chapter's `blend` and
/// the overview's scalars give it: the default plus each delta times the
/// scalar of the region its column names — exact in 16.16 for integer
/// deltas, so rounding it to 16.16 changes nothing.
fn blend_value(default: i32, deltas: &[i32], vsindex: usize, coordinates: &[i16]) -> f64 {
    let regions = regions();
    let mut bits = i64::from(default) * 65_536;
    for (delta, region) in deltas.iter().zip(cff2_data_regions(vsindex)) {
        bits += spec::region_scalar(&regions[usize::from(region)], coordinates) * i64::from(*delta);
    }
    bits as f64 / 65_536.0
}

/// What each blended glyph draws at 2.14 coordinates, from the values its
/// charstring states and the blends the specification computes.
fn blend_expected(c: &[i16]) -> Vec<Vec<Segment>> {
    let first = {
        let (x, y) = (
            blend_value(100, &[20, 30], 0, c),
            blend_value(200, &[40, 50], 0, c),
        );
        let dx = blend_value(300, &[-10, 5], 0, c);
        vec![
            Segment::MoveTo { x, y },
            Segment::LineTo { x: x + dx, y },
            Segment::Close,
        ]
    };
    let second = {
        let x = blend_value(50, &[1, 2, 3], 1, c);
        vec![
            Segment::MoveTo { x, y: 0.0 },
            Segment::LineTo {
                x: x + 20.0,
                y: 30.0,
            },
            Segment::Close,
        ]
    };
    let third = {
        let (x, y) = (
            blend_value(10, &[5, 5], 0, c),
            blend_value(20, &[6, 6], 0, c),
        );
        let dy = blend_value(40, &[-2, 2], 0, c);
        vec![
            Segment::MoveTo { x, y },
            Segment::LineTo { x, y: y + dy },
            Segment::Close,
        ]
    };
    let fourth = {
        let (x, y) = (
            blend_value(70, &[1, 1, 1], 1, c),
            blend_value(80, &[2, 2, 2], 1, c),
        );
        vec![Segment::MoveTo { x, y }, Segment::Close]
    };
    let fifth = {
        let x = blend_value(5, &[1, 1], 0, c);
        vec![
            Segment::MoveTo { x, y: 5.0 },
            Segment::LineTo {
                x: x + 10.0,
                y: 5.0,
            },
            Segment::Close,
        ]
    };
    vec![first, second, third, fourth, fifth]
}

/// Every location the CFF2 tests read: each named instance's, and user
/// coordinates no instance names, as the specification normalises them.
fn cff2_locations() -> Vec<Vec<i16>> {
    metric_locations()
        .into_iter()
        .map(|(_, user)| spec::location(&axis_ranges(), &user))
        .collect()
}

#[test]
fn cff2_blends_equal_the_deltas_applied_in_the_test() {
    let table = blend_cff2();
    let cff2 = Cff2::parse(&table).expect("a CFF2 table");
    let mut outline = Outline::default();
    let mut machine = Machine::new();
    let mut moved = 0;
    for coordinates in cff2_locations() {
        let location = Location::new(coordinates.clone());
        let blends = cff2.blends_at(&location);
        // The scalars, gathered for each subtable in its column order.
        for vsindex in 0..2u16 {
            let scalars = blends.scalars(vsindex).expect("a subtable's scalars");
            let expected: Vec<f64> = cff2_data_regions(usize::from(vsindex))
                .iter()
                .map(|region| {
                    spec::region_scalar(&regions()[usize::from(*region)], &coordinates) as f64
                        / 65_536.0
                })
                .collect();
            assert_eq!(scalars, expected.as_slice());
        }
        assert_eq!(blends.scalars(2), None);
        let expected = blend_expected(&coordinates);
        for (glyph, segments) in expected.iter().enumerate() {
            let glyph = u16::try_from(glyph).expect("few");
            let drawn = cff2.outline_at(glyph, &location).expect("an outline");
            assert_eq!(
                &drawn.segments, segments,
                "glyph {glyph} at {coordinates:?}"
            );
            // The same through blends computed once and one machine.
            cff2.outline_with(glyph, &blends, &mut outline, &mut machine)
                .expect("an outline");
            assert_eq!(&outline.segments, segments);
            if drawn.segments != blend_defaults()[usize::from(glyph)] {
                moved += 1;
            }
        }
    }
    assert!(moved > 0, "some instance moves some glyph");
    // A design space whose axis count the store shares is accepted; one it
    // does not is refused by name.
    let face = Face::opentype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let location = Location::new(vec![16_384, 8_192]);
    assert_eq!(
        cff2.blends_in(&space, &location),
        Ok(cff2.blends_at(&location))
    );
    let mut three = two_axes();
    three.push(AxisSpec::new(*b"opsz", 6.0, 12.0, 72.0, 258));
    let wider = DesignSpace::parse(&fvar(&three, &[]), None).expect("three axes");
    assert_eq!(
        cff2.blends_in(&wider, &location).err(),
        Some(Cff2Error::VariationStore(VariationError::AxisCount {
            table: *b"CFF2",
            found: 2,
            expected: 3
        }))
    );
}

#[test]
fn a_private_dict_blend_resolves_at_the_instance() {
    let table = blend_cff2();
    let cff2 = Cff2::parse(&table).expect("a CFF2 table");
    for coordinates in cff2_locations() {
        let location = Location::new(coordinates.clone());
        let blends = cff2.blends_at(&location);
        let first = cff2.private_dict_at(0, &blends).expect("a Private DICT");
        let blue_values: Vec<f64> = [(-20, [10, 0]), (0, [0, 0]), (500, [-5, -5]), (520, [2, 2])]
            .iter()
            .map(|(default, deltas)| blend_value(*default, deltas, 0, &coordinates))
            .collect();
        assert!(
            first.contains(&(6, blue_values.clone())),
            "BlueValues at {coordinates:?}: {first:?} against {blue_values:?}"
        );
        assert!(first.contains(&(10, vec![blend_value(50, &[10, -5], 0, &coordinates)])));
        // The second Private DICT's own vsindex selects the second subtable.
        let second = cff2.private_dict_at(1, &blends).expect("a Private DICT");
        assert_eq!(
            second,
            vec![
                (22, vec![1.0]),
                (11, vec![blend_value(80, &[1, 2, 3], 1, &coordinates)])
            ]
        );
    }
    let blends = cff2.blends_at(&Location::new(vec![16_384, 0]));
    assert_eq!(
        cff2.private_dict_at(2, &blends).err(),
        Some(Cff2Error::FontDict {
            glyph: 0,
            font_dict: 2
        })
    );
}

#[test]
fn the_default_instance_is_the_unblended_outline() {
    let table = blend_cff2();
    let cff2 = Cff2::parse(&table).expect("a CFF2 table");
    let default = Location::new(vec![0, 0]);
    let blends = cff2.blends_at(&default);
    for glyph in 0u16..5 {
        assert_eq!(
            cff2.outline_at(glyph, &default)
                .expect("an outline")
                .segments,
            cff2.outline(glyph).expect("an outline").segments,
            "glyph {glyph}"
        );
        assert_eq!(
            cff2.outline_at(glyph, &default)
                .expect("an outline")
                .segments,
            blend_defaults()[usize::from(glyph)]
        );
    }
    for font_dict in 0u16..2 {
        assert_eq!(
            cff2.private_dict_at(font_dict, &blends),
            cff2.private_dict(font_dict)
        );
    }
    // An empty location is the default too: every missing coordinate is
    // zero.
    assert_eq!(cff2.blends_at(&Location::default()), blends);
    // A table with no store has no scalars at any location, and its blends
    // are refused there as at the default.
    let plain = plain_cff2(None);
    let cff2 = Cff2::parse(&plain).expect("a CFF2 table");
    let blends = cff2.blends_at(&Location::new(vec![16_384, 16_384]));
    assert_eq!(blends.scalars(0), None);
    for glyph in 0u16..8 {
        assert_eq!(
            cff2.outline_at(glyph, &Location::new(vec![16_384, 16_384]))
                .map(|outline| outline.segments),
            cff2.outline(glyph).map(|outline| outline.segments),
            "a face with no variations draws the same everywhere"
        );
    }
    let blending = single_glyph(cs(&blended(&[1], &[&[2, 3]])), None);
    let cff2 = Cff2::parse(&blending).expect("parses");
    assert_eq!(
        cff2.outline_at(0, &Location::new(vec![16_384, 0])).err(),
        Some(Cff2Error::NoVariationStore)
    );
}

// ---- gvar applied to glyf (FT-05.4) ------------------------------------------

/// One glyph of the `gvar` face as the test states it; the face and every
/// expectation below are built from this statement.
#[derive(Clone, Debug)]
enum Stated {
    /// No outline: a space, whose `gvar` data moves its phantom points.
    Empty,
    /// A simple glyph's contours.
    Simple(Vec<Vec<GlyphPoint>>),
    /// A composite's header box and its records.
    Composite([i16; 4], Vec<ComponentSpec>),
}

/// The glyphs: a space; a box and a curve whose off-curve pair implies a
/// midpoint; a glyph whose listed points leave others untouched, in a
/// contour that takes every case of the inference, a contour none of whose
/// points is listed and one with a single listed point; a composite placed
/// by an offset, by a scaled offset and by matching points, the first with
/// `USE_MY_METRICS`; a composite of that composite and the space; and a
/// glyph with no variation data.
fn gvar_glyphs() -> Vec<Stated> {
    let box_and_curve = vec![
        vec![
            (0, 0, true),
            (400, 0, true),
            (400, 700, true),
            (0, 700, true),
        ],
        vec![
            (100, 100, true),
            (150, 300, false),
            (250, 300, false),
            (300, 100, true),
            (200, 50, false),
        ],
    ];
    let untouched = vec![
        vec![
            (0, 0, true),
            (100, 0, true),
            (300, 0, true),
            (350, 50, false),
            (270, 200, false),
            (250, 400, true),
            (-50, 400, true),
            (-50, 200, false),
        ],
        vec![
            (500, 500, true),
            (600, 500, true),
            (600, 600, true),
            (500, 600, true),
        ],
        vec![(700, 0, true), (800, 0, true), (750, 90, true)],
    ];
    let mut scaled = ComponentSpec::at(2, 500, -20);
    scaled.flags |= 0x0008; // WE_HAVE_A_SCALE
    scaled.transform = vec![8_192];
    let mut metrics = ComponentSpec::at(1, 50, 0);
    metrics.flags |= 0x0200; // USE_MY_METRICS
    let matched = ComponentSpec {
        flags: 0x0001, // words, and ARGS_ARE_XY_VALUES clear
        glyph: 1,
        arguments: (0, 3),
        transform: Vec::new(),
    };
    let lifted = ComponentSpec {
        flags: 0x0002, // byte offsets
        glyph: 3,
        arguments: (0, 100),
        transform: Vec::new(),
    };
    vec![
        Stated::Empty,
        Stated::Simple(box_and_curve.clone()),
        Stated::Simple(untouched),
        Stated::Composite([0, -20, 1_250, 700], vec![metrics, scaled, matched]),
        Stated::Composite(
            [0, 80, 1_250, 800],
            vec![lifted, ComponentSpec::at(0, 10, 10)],
        ),
        Stated::Simple(box_and_curve),
    ]
}

/// The shared tuples: the weight's maximum, the width's, and their corner.
fn gvar_shared_tuples() -> Vec<Vec<i16>> {
    vec![vec![16_384, 0], vec![0, 16_384], vec![16_384, 16_384]]
}

/// A table whose peak is a shared tuple's, the peak kept for the test's
/// arithmetic.
fn shared_peak(index: u16, points: Option<Points>, deltas: &[i32]) -> TupleSpec {
    let mut tuple = TupleSpec::new(&gvar_shared_tuples()[usize::from(index)], points, deltas);
    tuple.shared_peak = Some(index);
    tuple
}

/// Each glyph's variation data. Every point-number form appears: every
/// item, private lists with a number repeated and phantom points among them,
/// a glyph's shared points, an intermediate region, peaks embedded and
/// shared, and deltas in bytes, words and zero runs.
fn gvar_variations() -> Vec<Option<GlyphVariation>> {
    let space = GlyphVariation {
        shared_points: None,
        tuples: vec![
            TupleSpec::new(
                &[16_384, 0],
                Some(Points::All),
                &[-5, 40, 0, 0, 0, 0, 10, -10],
            ),
            shared_peak(1, Some(Points::Listed(vec![1, 3])), &[25, 3, 7, -30]),
        ],
    };
    // Nine contour points and four phantom points.
    let box_and_curve = GlyphVariation {
        shared_points: Some(Points::Listed(vec![0, 2, 5, 9, 10])),
        tuples: vec![
            TupleSpec::new(
                &[16_384, 0],
                Some(Points::All),
                &[
                    20, 30, 30, 20, 10, 12, 14, 16, 18, 0, 60, 0, 0, //
                    0, 0, 50, 50, 5, 9, -9, -5, 0, 0, 0, 40, -40,
                ],
            ),
            shared_peak(1, None, &[-8, -16, 24, 3, 77, 0, 6, -6, 0, 0]),
            TupleSpec::new(
                &[8_192, -8_192],
                Some(Points::Listed(vec![3, 3, 4, 7, 11, 12])),
                &[1, 2, -3, 4, 0, 0, 300, -200, 5, -7, 33, -33],
            )
            .between(&[0, -16_384], &[16_384, 0]),
            TupleSpec::new(
                &[-16_384, 0],
                Some(Points::All),
                &[
                    -150, -150, -150, -150, -40, -40, -40, -40, -40, 0, -300, 0, 0, //
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
            ),
        ],
    };
    // Fifteen contour points and four phantom points.
    let untouched = GlyphVariation {
        shared_points: None,
        tuples: vec![
            TupleSpec::new(
                &[16_384, 0],
                Some(Points::Listed(vec![0, 2, 5, 13])),
                &[10, 30, -7, 4, 20, 25, 13, -9],
            ),
            TupleSpec::new(
                &[8_192, 16_384],
                Some(Points::Listed(vec![2, 6, 13, 13, 16])),
                &[3, -11, 2, 2, 40, 7, 0, -1, -1, 0],
            )
            .between(&[0, 0], &[16_384, 16_384]),
            TupleSpec::new(
                &[0, -16_384],
                Some(Points::All),
                &[
                    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, //
                    -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14, -15, -16, -17,
                    -18, -19,
                ],
            ),
        ],
    };
    // Three components and four phantom points; the matched component's
    // delta is stated and must move nothing, and so must the phantom
    // points' ignored axes.
    let placed = GlyphVariation {
        shared_points: None,
        tuples: vec![
            TupleSpec::new(
                &[16_384, 0],
                Some(Points::All),
                &[30, -10, 999, 70, 90, 11, 12, 5, 7, 999, 13, 14, -40, 40],
            ),
            shared_peak(1, Some(Points::Listed(vec![1, 4])), &[-60, 25, 8, -3]),
        ],
    };
    // Two components and four phantom points.
    let nested = GlyphVariation {
        shared_points: None,
        tuples: vec![shared_peak(
            2,
            Some(Points::All),
            &[0, 15, 20, 20, 20, 20, -12, 0, 1, 1, 1, 1],
        )],
    };
    vec![
        Some(space),
        Some(box_and_curve),
        Some(untouched),
        Some(placed),
        Some(nested),
        None,
    ]
}

/// The glyphs' horizontal metrics, a left side bearing that is not `xMin`
/// among them so that a horizontal origin stands away from zero.
fn gvar_horizontal() -> Vec<(u16, i16)> {
    vec![(250, 0), (600, 0), (500, -40), (800, 0), (820, 0), (300, 0)]
}

/// The glyphs' vertical metrics, for the face that carries them.
fn gvar_vertical() -> Vec<(u16, i16)> {
    vec![
        (1_000, 0),
        (1_000, 100),
        (900, 300),
        (1_100, 100),
        (1_200, 120),
        (1_000, 200),
    ]
}

/// `hhea`'s ascender and descender, which stand in for vertical metrics in
/// the face without them.
const GVAR_ASCENDER: i16 = 800;
const GVAR_DESCENDER: i16 = -200;

/// The glyph's `glyf` data.
fn stated_bytes(glyph: &Stated) -> Vec<u8> {
    match glyph {
        Stated::Empty => Vec::new(),
        Stated::Simple(contours) => variable_support::simple_glyph(contours),
        Stated::Composite(bounds, components) => composite_glyph(*bounds, components),
    }
}

/// The `gvar` face: `fvar`'s two axes, `glyf`, `loca`, `head`, `maxp`,
/// `hhea` with its ascender and descender, `hmtx`, and `vhea` and `vmtx`
/// where `vertical`; `gvar` with long or halved short offsets.
fn gvar_face(long_offsets: bool, vertical: bool) -> Vec<u8> {
    let glyphs: Vec<Vec<u8>> = gvar_glyphs().iter().map(stated_bytes).collect();
    let (glyf, loca) = glyf_and_loca(&glyphs);
    let count = u16::try_from(glyphs.len()).expect("few");
    let mut horizontal = hhea(count, (1, 0, 0));
    horizontal[4..6].copy_from_slice(&GVAR_ASCENDER.to_be_bytes());
    horizontal[6..8].copy_from_slice(&GVAR_DESCENDER.to_be_bytes());
    let mut face = Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"glyf", glyf)
        .with(*b"loca", loca)
        .with(*b"head", head(1_000))
        .with(*b"maxp", maxp(count))
        .with(*b"hhea", horizontal)
        .with(*b"hmtx", metrics(&gvar_horizontal(), &[]))
        .with(
            *b"gvar",
            variable_support::gvar(2, &gvar_shared_tuples(), &gvar_variations(), long_offsets),
        );
    if vertical {
        face = face
            .with(*b"vhea", vhea(count, (900, -100, 0), (0, 1, 0)))
            .with(*b"vmtx", metrics(&gvar_vertical(), &[]));
    }
    face.build()
}

/// Every form of the face: both offset widths, with and without vertical
/// metrics.
fn gvar_faces() -> Vec<(bool, bool, Vec<u8>)> {
    let mut out = Vec::new();
    for long_offsets in [false, true] {
        for vertical in [false, true] {
            out.push((long_offsets, vertical, gvar_face(long_offsets, vertical)));
        }
    }
    out
}

/// A stated glyph's header box: its points', a composite's as written, and
/// zero for a space.
fn stated_bounds(glyph: &Stated) -> [i32; 4] {
    match glyph {
        Stated::Empty => [0; 4],
        Stated::Simple(contours) => {
            let points: Vec<&GlyphPoint> = contours.iter().flatten().collect();
            [
                points.iter().map(|p| p.0).min().unwrap_or(0),
                points.iter().map(|p| p.1).min().unwrap_or(0),
                points.iter().map(|p| p.0).max().unwrap_or(0),
                points.iter().map(|p| p.1).max().unwrap_or(0),
            ]
        }
        Stated::Composite(bounds, _) => bounds.map(i32::from),
    }
}

/// A stated glyph's default phantom points, from the metrics the face was
/// written with: the horizontal origin at `xMin` less the left side bearing
/// and the advance past it, both at `y = 0`; the vertical origin at `yMax`
/// plus the top side bearing and the advance height below it, both at
/// `x = 0` — or, with no vertical metrics, at the ascender and the
/// descender.
fn stated_phantoms(index: usize, vertical: bool) -> [(i32, i32); 4] {
    let [x_min, _, _, y_max] = stated_bounds(&gvar_glyphs()[index]);
    let (advance, lsb) = gvar_horizontal()[index];
    let origin = x_min - i32::from(lsb);
    let (top, bottom) = if vertical {
        let (height, tsb) = gvar_vertical()[index];
        let top = y_max + i32::from(tsb);
        (top, top - i32::from(height))
    } else {
        (i32::from(GVAR_ASCENDER), i32::from(GVAR_DESCENDER))
    };
    [
        (origin, 0),
        (origin + i32::from(advance), 0),
        (0, top),
        (0, bottom),
    ]
}

/// The contours of a stated simple glyph as `(first, last)` pairs.
fn contour_ranges(contours: &[Vec<GlyphPoint>]) -> Vec<(usize, usize)> {
    let mut first = 0;
    contours
        .iter()
        .map(|contour| {
            let range = (first, first + contour.len() - 1);
            first += contour.len();
            range
        })
        .collect()
}

/// A stated glyph's items with their default positions — its contour
/// points, or one placeholder a component — and its phantom points after.
fn stated_items(index: usize, vertical: bool) -> Vec<(i32, i32)> {
    let mut items: Vec<(i32, i32)> = match &gvar_glyphs()[index] {
        Stated::Empty => Vec::new(),
        Stated::Simple(contours) => contours.iter().flatten().map(|p| (p.0, p.1)).collect(),
        Stated::Composite(_, components) => vec![(0, 0); components.len()],
    };
    items.extend(stated_phantoms(index, vertical));
    items
}

/// A stated glyph's net deltas at 2.14 coordinates, from the specification.
fn stated_deltas(index: usize, vertical: bool, coordinates: &[i16]) -> Vec<[i64; 2]> {
    let items = stated_items(index, vertical);
    let Some(variation) = &gvar_variations()[index] else {
        return vec![[0, 0]; items.len()];
    };
    match &gvar_glyphs()[index] {
        Stated::Simple(contours) => spec::glyph_deltas(
            &items,
            Some(&contour_ranges(contours)),
            variation,
            coordinates,
        ),
        Stated::Empty => spec::glyph_deltas(&items, Some(&[]), variation, coordinates),
        Stated::Composite(..) => spec::glyph_deltas(&items, None, variation, coordinates),
    }
}

/// A 16.16 value as the `f64` it is exactly.
fn units(bits: i64) -> f64 {
    bits as f64 / 65_536.0
}

/// The segments the `glyf` chapter's contour reading draws from final
/// positions, placed by `place`: each contour from its first on-curve point,
/// a line to an on-curve point, a quadratic through an off-curve point to
/// the next on-curve one or to the midpoint of two off-curve ones, and back
/// to the start. Every contour this file states holds an on-curve point.
fn drawn(
    positions: &[(f64, f64)],
    on: &[bool],
    contours: &[(usize, usize)],
    place: &dyn Fn(f64, f64) -> (f64, f64),
    out: &mut Vec<Segment>,
) {
    for &(first, last) in contours {
        let ring: Vec<usize> = (first..=last).collect();
        let len = ring.len();
        let start = (0..len)
            .find(|k| on[ring[*k]])
            .expect("a contour with an on-curve point");
        let at = |k: usize| ring[(start + k) % len];
        let (sx, sy) = place(positions[at(0)].0, positions[at(0)].1);
        out.push(Segment::MoveTo { x: sx, y: sy });
        let mut control: Option<(f64, f64)> = None;
        for k in 1..=len {
            let (x, y) = positions[at(k)];
            if on[at(k)] {
                let (px, py) = place(x, y);
                match control.take() {
                    None => out.push(Segment::LineTo { x: px, y: py }),
                    Some((cx, cy)) => {
                        let (cx, cy) = place(cx, cy);
                        out.push(Segment::QuadTo {
                            cx,
                            cy,
                            x: px,
                            y: py,
                        });
                    }
                }
            } else {
                if let Some((cx, cy)) = control {
                    let (mx, my) = place((cx + x) / 2.0, (cy + y) / 2.0);
                    let (cx, cy) = place(cx, cy);
                    out.push(Segment::QuadTo {
                        cx,
                        cy,
                        x: mx,
                        y: my,
                    });
                }
                control = Some((x, y));
            }
        }
        out.push(Segment::Close);
    }
}

/// A placement `[dx, dy, a, b, c, d]`: a point goes to `(a x + c y + dx,
/// b x + d y + dy)`.
type Placement = [f64; 6];

const UNPLACED: Placement = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];

fn placed(m: Placement) -> impl Fn(f64, f64) -> (f64, f64) {
    move |x, y| (m[2] * x + m[4] * y + m[0], m[3] * x + m[5] * y + m[1])
}

/// A component's placement inside its parent's: its own 2×2 after the
/// parent's, and its offset placed by the parent's — the composite reading
/// the `glyf` chapter gives, where a component's own transform does not
/// scale its offset.
fn component_placement(parent: Placement, transform: [f64; 4], offset: (f64, f64)) -> Placement {
    let [dx, dy, a, b, c, d] = parent;
    let [ca, cb, cc, cd] = transform;
    [
        offset.0 * a + offset.1 * c + dx,
        offset.0 * b + offset.1 * d + dy,
        ca * a + cb * c,
        ca * b + cb * d,
        cc * a + cd * c,
        cc * b + cd * d,
    ]
}

/// A record's 2×2 from the F2Dot14 values its flags call for.
fn component_transform(component: &ComponentSpec) -> [f64; 4] {
    let f = |v: i16| f64::from(v) / 16_384.0;
    match component.transform.as_slice() {
        [] => [1.0, 0.0, 0.0, 1.0],
        [s] => [f(*s), 0.0, 0.0, f(*s)],
        [x, y] => [f(*x), 0.0, 0.0, f(*y)],
        [a, b, c, d] => [f(*a), f(*b), f(*c), f(*d)],
        other => panic!("a transform of {} values", other.len()),
    }
}

/// The outline the specification gives a stated glyph at 2.14 coordinates:
/// each point at its default plus its net delta, each component's offset
/// likewise where it is placed by an offset, drawn by the contour reading.
fn expected_gvar_outline(
    index: usize,
    vertical: bool,
    coordinates: &[i16],
    placement: Placement,
    out: &mut Vec<Segment>,
) {
    let glyphs = gvar_glyphs();
    let deltas = stated_deltas(index, vertical, coordinates);
    let items = stated_items(index, vertical);
    match &glyphs[index] {
        Stated::Empty => {}
        Stated::Simple(contours) => {
            let on: Vec<bool> = contours.iter().flatten().map(|p| p.2).collect();
            let positions: Vec<(f64, f64)> = items
                .iter()
                .zip(&deltas)
                .map(|(p, d)| {
                    (
                        units(i64::from(p.0) * 65_536 + d[0]),
                        units(i64::from(p.1) * 65_536 + d[1]),
                    )
                })
                .collect();
            drawn(
                &positions,
                &on,
                &contour_ranges(contours),
                &placed(placement),
                out,
            );
        }
        Stated::Composite(_, components) => {
            for (p, component) in components.iter().enumerate() {
                let offset = if component.flags & 0x0002 != 0 {
                    (
                        units(i64::from(component.arguments.0) * 65_536 + deltas[p][0]),
                        units(i64::from(component.arguments.1) * 65_536 + deltas[p][1]),
                    )
                } else {
                    (0.0, 0.0)
                };
                let child = component_placement(placement, component_transform(component), offset);
                expected_gvar_outline(
                    usize::from(component.glyph),
                    vertical,
                    coordinates,
                    child,
                    out,
                );
            }
        }
    }
}

/// Every location the `gvar` tests read: each named instance and the
/// metric tests' grid, with the 2.14 coordinates the specification computes
/// for it.
fn gvar_locations() -> Vec<(Instance, Vec<i16>)> {
    let ranges = axis_ranges();
    metric_locations()
        .into_iter()
        .map(|(instance, user)| (instance, spec::location(&ranges, &user)))
        .collect()
}

#[test]
fn instance_outlines_equal_the_deltas_applied_in_the_test() {
    let glyphs = gvar_glyphs();
    for (long_offsets, vertical, face) in gvar_faces() {
        let sfnt = Sfnt::parse(&face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space)
            .expect("gvar is read")
            .expect("the face carries one");
        assert_eq!(gvar.glyph_count(), 6);
        assert_eq!(gvar.shared_tuples().len(), 3);
        for (instance, coordinates) in gvar_locations() {
            let location = space.location(&instance).expect("a location");
            assert_eq!(location.coordinates(), coordinates, "at {instance:?}");
            for (index, stated) in glyphs.iter().enumerate() {
                let glyph = u16::try_from(index).expect("few");
                let what = format!(
                    "glyph {index} at {coordinates:?}, long {long_offsets}, vertical {vertical}"
                );
                // The deltas, item by item, before anything is drawn.
                let expected_deltas = stated_deltas(index, vertical, &coordinates);
                let deltas: Vec<[i64; 2]> = match stated {
                    Stated::Composite(_, components) => gvar
                        .composite_deltas(glyph, components.len(), &location)
                        .expect("the deltas"),
                    _ => {
                        let raw = glyf::simple_glyph(&sfnt, glyph).expect("a simple glyph");
                        gvar.simple_deltas(glyph, &raw, &location)
                            .expect("the deltas")
                    }
                }
                .iter()
                .map(|d| [d[0].bits(), d[1].bits()])
                .collect();
                assert_eq!(deltas, expected_deltas, "{what}: the deltas");
                // And the outline they draw.
                let mut expected = Vec::new();
                expected_gvar_outline(index, vertical, &coordinates, UNPLACED, &mut expected);
                let outline =
                    glyf::outline_at(&sfnt, glyph, Some(&gvar), &location).expect("an outline");
                assert_eq!(outline.segments, expected, "{what}: the outline");
                let (counted, steps) =
                    glyf::outline_at_counted(&sfnt, glyph, Some(&gvar), &location);
                assert_eq!(
                    counted.map(|o| o.segments),
                    Ok(outline.segments),
                    "{what}: alike counted"
                );
                assert!(steps > 0, "{what}: the work was counted");
            }
        }
    }
}

#[test]
fn the_raw_point_form_is_the_glyphs_points_and_its_phantom_points() {
    for (_, vertical, face) in gvar_faces() {
        let sfnt = Sfnt::parse(&face).expect("a face");
        for (index, stated) in gvar_glyphs().iter().enumerate() {
            let glyph = u16::try_from(index).expect("few");
            let phantoms: Vec<[i32; 2]> = stated_phantoms(index, vertical)
                .iter()
                .map(|p| [p.0, p.1])
                .collect();
            assert_eq!(
                glyf::phantom_points(&sfnt, glyph).to_vec(),
                phantoms,
                "glyph {index}'s phantom points, vertical {vertical}"
            );
            match stated {
                Stated::Composite(_, components) => {
                    assert_eq!(glyf::simple_glyph(&sfnt, glyph), None);
                    let read = glyf::components(&sfnt, glyph).expect("a composite");
                    assert_eq!(read.len(), components.len());
                    for (record, component) in read.iter().zip(components) {
                        assert_eq!(record.glyph, component.glyph);
                        assert_eq!(
                            record.arguments,
                            [component.arguments.0, component.arguments.1]
                        );
                        assert_eq!(record.offset_is_xy(), component.flags & 0x0002 != 0);
                        let transform = match component.transform.as_slice() {
                            [] => [16_384, 0, 0, 16_384],
                            [s] => [*s, 0, 0, *s],
                            [x, y] => [*x, 0, 0, *y],
                            [a, b, c, d] => [*a, *b, *c, *d],
                            other => panic!("a transform of {} values", other.len()),
                        };
                        assert_eq!(record.transform, transform);
                    }
                }
                Stated::Empty => {
                    let raw = glyf::simple_glyph(&sfnt, glyph).expect("an empty glyph");
                    assert_eq!(raw.contour_points(), 0);
                    assert!(raw.ends.is_empty() && raw.contours().is_empty());
                    assert_eq!(raw.points, phantoms);
                    assert_eq!(glyf::components(&sfnt, glyph), None);
                }
                Stated::Simple(contours) => {
                    let raw = glyf::simple_glyph(&sfnt, glyph).expect("a simple glyph");
                    let points: Vec<[i32; 2]> = contours
                        .iter()
                        .flatten()
                        .map(|p| [p.0, p.1])
                        .chain(phantoms.iter().copied())
                        .collect();
                    assert_eq!(raw.points, points);
                    let flags: Vec<u8> = contours.iter().flatten().map(|p| u8::from(p.2)).collect();
                    assert_eq!(raw.flags, flags);
                    assert_eq!(raw.contours(), contour_ranges(contours));
                    assert!(raw.instructions.is_empty());
                    assert_eq!(glyf::components(&sfnt, glyph), None);
                }
            }
        }
        // A glyph past the end of `loca` has no form at all.
        assert_eq!(glyf::simple_glyph(&sfnt, 6), None);
        assert_eq!(glyf::components(&sfnt, 6), None);
    }
}

#[test]
fn the_default_instance_is_the_static_outline() {
    for (_, _, face) in gvar_faces() {
        let sfnt = Sfnt::parse(&face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space)
            .expect("gvar is read")
            .expect("the face carries one");
        for glyph in 0u16..6 {
            let unvaried = glyf::outline(&sfnt, glyph).expect("an outline").segments;
            for location in [space.default_location(), Location::default()] {
                assert_eq!(
                    glyf::outline_at(&sfnt, glyph, Some(&gvar), &location).map(|o| o.segments),
                    Ok(unvaried.clone()),
                    "glyph {glyph} at the default"
                );
            }
            // With no `gvar` nothing moves, wherever it is asked.
            assert_eq!(
                glyf::outline_at(&sfnt, glyph, None, &Location::new(vec![16_384, -8_192]))
                    .map(|o| o.segments),
                Ok(unvaried.clone())
            );
            let (outline, steps) = glyf::outline_counted(&sfnt, glyph);
            let (counted, counted_steps) =
                glyf::outline_at_counted(&sfnt, glyph, None, &space.default_location());
            assert_eq!(
                counted.map(|o| o.segments),
                Ok(outline.expect("an outline").segments)
            );
            assert_eq!(
                counted_steps, steps,
                "with no gvar, outline_at counts what outline counts"
            );
        }
    }
    // A face that does not vary has no `gvar` to read, whatever it carries.
    let face = Face::truetype()
        .with(*b"gvar", variable_support::gvar(2, &[], &[None], false))
        .build();
    let sfnt = Sfnt::parse(&face).expect("a face");
    assert!(Gvar::read(&sfnt, &DesignSpace::default())
        .expect("not read")
        .is_none());
    // And `TestGPOSFour.ttf` draws every glyph at its default as it draws
    // it statically.
    let sfnt = Sfnt::parse(TEST_GPOS_FOUR).expect("the face parses");
    let space = DesignSpace::read(&sfnt).expect("its design space");
    let gvar = Gvar::read(&sfnt, &space)
        .expect("gvar is read")
        .expect("it carries one");
    for glyph in 0u16..13 {
        assert_eq!(
            glyf::outline_at(&sfnt, glyph, Some(&gvar), &space.default_location())
                .map(|o| o.segments),
            Ok(glyf::outline(&sfnt, glyph).expect("an outline").segments),
            "TestGPOSFour glyph {glyph}"
        );
    }
}

/// The phantom points the specification gives a stated glyph: its default
/// ones plus its deltas, or, for a composite with a `USE_MY_METRICS`
/// component, that component's.
fn expected_phantoms(index: usize, vertical: bool, coordinates: &[i16]) -> [[i64; 2]; 4] {
    if let Stated::Composite(_, components) = &gvar_glyphs()[index] {
        if let Some(metrics) = components.iter().rev().find(|c| c.flags & 0x0200 != 0) {
            return expected_phantoms(usize::from(metrics.glyph), vertical, coordinates);
        }
    }
    let defaults = stated_phantoms(index, vertical);
    let deltas = stated_deltas(index, vertical, coordinates);
    let last = &deltas[deltas.len() - 4..];
    let mut out = [[0i64; 2]; 4];
    for (k, slot) in out.iter_mut().enumerate() {
        *slot = [
            i64::from(defaults[k].0) * 65_536 + last[k][0],
            i64::from(defaults[k].1) * 65_536 + last[k][1],
        ];
    }
    out
}

#[test]
fn phantom_points_give_the_advance_without_hvar() {
    for (_, vertical, face) in gvar_faces() {
        let sfnt = Sfnt::parse(&face).expect("a face");
        assert!(sfnt.table(u32::from_be_bytes(*b"HVAR")).is_none());
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space)
            .expect("gvar is read")
            .expect("the face carries one");
        for (instance, coordinates) in gvar_locations() {
            let location = space.location(&instance).expect("a location");
            for index in 0..6 {
                let glyph = u16::try_from(index).expect("few");
                let expected = expected_phantoms(index, vertical, &coordinates);
                let phantom =
                    glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), &location).expect("points");
                let bits = phantom.map(|p| [p[0].bits(), p[1].bits()]);
                assert_eq!(bits, expected, "glyph {index} at {coordinates:?}");
                // The advance width is the two horizontal points' distance,
                // and the advance height the two vertical ones'.
                assert_eq!(
                    phantom[1][0].saturating_sub(phantom[0][0]).bits(),
                    expected[1][0] - expected[0][0]
                );
                assert_eq!(
                    phantom[2][1].saturating_sub(phantom[3][1]).bits(),
                    expected[2][1] - expected[3][1]
                );
            }
        }
        // At the default, and with no gvar, the phantom points are the
        // metrics' — glyph 3's those of its `USE_MY_METRICS` component.
        for index in 0..6 {
            let glyph = u16::try_from(index).expect("few");
            let expected = expected_phantoms(index, vertical, &[0, 0]);
            for gvar in [Some(&gvar), None] {
                let phantom =
                    glyf::phantom_points_at(&sfnt, glyph, gvar, &space.default_location())
                        .expect("points");
                assert_eq!(phantom.map(|p| [p[0].bits(), p[1].bits()]), expected);
            }
        }
    }
}

// ---- TestGPOSFour.ttf's outlines, through a second reader --------------------

/// A second reader of `glyf`, `loca`, `hmtx`, `hhea` and `gvar`, decoding
/// each from its chapter's field list by index arithmetic, with no code
/// shared with the crate; its arithmetic is `variable_support::spec`'s.
mod outline_reader {
    use super::second_reader::table;
    use crate::variable_support::{GlyphVariation, Points, TupleSpec};

    fn u16_at(data: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([data[at], data[at + 1]])
    }

    fn i16_at(data: &[u8], at: usize) -> i16 {
        u16_at(data, at) as i16
    }

    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    /// One component: its flags, the glyph it draws and its offset.
    pub struct Component {
        pub flags: u16,
        pub glyph: u16,
        pub x: i32,
        pub y: i32,
    }

    /// One glyph: a simple glyph's contours, on-curve flags and points, or a
    /// composite's components, and the header's `xMin`.
    pub struct Glyph {
        pub contours: Vec<(usize, usize)>,
        pub on: Vec<bool>,
        pub points: Vec<(i32, i32)>,
        pub components: Vec<Component>,
        pub x_min: i32,
    }

    /// The glyph's `glyf` entry, `None` for an empty one.
    pub fn glyph(face: &[u8], index: u16) -> Option<Glyph> {
        let head = table(face, b"head").expect("head");
        let loca = table(face, b"loca").expect("loca");
        let glyf = table(face, b"glyf").expect("glyf");
        let i = usize::from(index);
        let (start, end) = if i16_at(head, 50) == 0 {
            (
                usize::from(u16_at(loca, i * 2)) * 2,
                usize::from(u16_at(loca, i * 2 + 2)) * 2,
            )
        } else {
            (
                u32_at(loca, i * 4) as usize,
                u32_at(loca, i * 4 + 4) as usize,
            )
        };
        if end <= start {
            return None;
        }
        let data = &glyf[start..end];
        let contour_count = i16_at(data, 0);
        let x_min = i32::from(i16_at(data, 2));
        if contour_count < 0 {
            let mut components = Vec::new();
            let mut at = 10;
            loop {
                let flags = u16_at(data, at);
                let glyph = u16_at(data, at + 2);
                at += 4;
                assert!(flags & 0x0002 != 0, "TestGPOSFour places by offsets");
                assert!(flags & 0x00C8 == 0, "and has no component transform");
                let (x, y) = if flags & 0x0001 != 0 {
                    let v = (i32::from(i16_at(data, at)), i32::from(i16_at(data, at + 2)));
                    at += 4;
                    v
                } else {
                    let v = (i32::from(data[at] as i8), i32::from(data[at + 1] as i8));
                    at += 2;
                    v
                };
                components.push(Component { flags, glyph, x, y });
                if flags & 0x0020 == 0 {
                    break;
                }
            }
            return Some(Glyph {
                contours: Vec::new(),
                on: Vec::new(),
                points: Vec::new(),
                components,
                x_min,
            });
        }
        let count = usize::try_from(contour_count).expect("simple");
        let ends: Vec<usize> = (0..count)
            .map(|c| usize::from(u16_at(data, 10 + c * 2)))
            .collect();
        let points = ends.last().map_or(0, |e| e + 1);
        let mut at = 10 + count * 2;
        let program = usize::from(u16_at(data, at));
        at += 2 + program;
        let mut flags = Vec::with_capacity(points);
        while flags.len() < points {
            let flag = data[at];
            at += 1;
            let repeat = if flag & 0x08 != 0 {
                at += 1;
                usize::from(data[at - 1])
            } else {
                0
            };
            for _ in 0..=repeat {
                flags.push(flag);
            }
        }
        flags.truncate(points);
        let mut read_axis = |short: u8, same: u8| {
            let mut value = 0i32;
            let mut out = Vec::with_capacity(points);
            for flag in &flags {
                if flag & short != 0 {
                    let magnitude = i32::from(data[at]);
                    at += 1;
                    value += if flag & same != 0 {
                        magnitude
                    } else {
                        -magnitude
                    };
                } else if flag & same == 0 {
                    value += i32::from(i16_at(data, at));
                    at += 2;
                }
                out.push(value);
            }
            out
        };
        let xs = read_axis(0x02, 0x10);
        let ys = read_axis(0x04, 0x20);
        let mut contours = Vec::new();
        let mut first = 0;
        for end in ends {
            contours.push((first, end));
            first = end + 1;
        }
        Some(Glyph {
            contours,
            on: flags.iter().map(|f| f & 1 != 0).collect(),
            points: xs.into_iter().zip(ys).collect(),
            components: Vec::new(),
            x_min,
        })
    }

    /// The glyph's advance and left side bearing, from `hhea` and `hmtx`.
    pub fn horizontal(face: &[u8], index: u16) -> (i32, i32) {
        let hhea = table(face, b"hhea").expect("hhea");
        let hmtx = table(face, b"hmtx").expect("hmtx");
        let long = usize::from(u16_at(hhea, 34));
        let i = usize::from(index);
        if i < long {
            (
                i32::from(u16_at(hmtx, i * 4)),
                i32::from(i16_at(hmtx, i * 4 + 2)),
            )
        } else {
            (
                i32::from(u16_at(hmtx, (long - 1) * 4)),
                i32::from(i16_at(hmtx, long * 4 + (i - long) * 2)),
            )
        }
    }

    /// `hhea`'s ascender and descender.
    pub fn ascender_descender(face: &[u8]) -> (i32, i32) {
        let hhea = table(face, b"hhea").expect("hhea");
        (i32::from(i16_at(hhea, 4)), i32::from(i16_at(hhea, 6)))
    }

    /// Packed point numbers at `at`: `None` for every point.
    fn points(data: &[u8], at: &mut usize) -> Option<Vec<u16>> {
        let first = data[*at];
        *at += 1;
        if first == 0 {
            return None;
        }
        let count = if first & 0x80 != 0 {
            let low = data[*at];
            *at += 1;
            (usize::from(first & 0x7F) << 8) | usize::from(low)
        } else {
            usize::from(first)
        };
        let mut numbers = Vec::with_capacity(count);
        let mut value = 0u16;
        while numbers.len() < count {
            let control = data[*at];
            *at += 1;
            for _ in 0..=(control & 0x7F) {
                let step = if control & 0x80 != 0 {
                    let v = u16_at(data, *at);
                    *at += 2;
                    v
                } else {
                    let v = u16::from(data[*at]);
                    *at += 1;
                    v
                };
                value += step;
                numbers.push(value);
            }
        }
        Some(numbers)
    }

    /// `count` packed deltas at `at`.
    fn deltas(data: &[u8], at: &mut usize, count: usize) -> Vec<i32> {
        let mut out = Vec::with_capacity(count);
        while out.len() < count {
            let control = data[*at];
            *at += 1;
            let run = usize::from(control & 0x3F) + 1;
            for _ in 0..run {
                let value = if control & 0x80 != 0 {
                    0
                } else if control & 0x40 != 0 {
                    let v = i32::from(i16_at(data, *at));
                    *at += 2;
                    v
                } else {
                    let v = i32::from(data[*at] as i8);
                    *at += 1;
                    v
                };
                out.push(value);
            }
        }
        out
    }

    /// The glyph's variation data over `items` items, every peak resolved,
    /// as the test's tables state theirs.
    pub fn variations(face: &[u8], index: u16, items: usize) -> Option<GlyphVariation> {
        let gvar = table(face, b"gvar").expect("gvar");
        let axes = usize::from(u16_at(gvar, 4));
        let shared_count = usize::from(u16_at(gvar, 6));
        let shared_at = u32_at(gvar, 8) as usize;
        let glyphs = usize::from(u16_at(gvar, 12));
        let long = u16_at(gvar, 14) & 1 != 0;
        let array_at = u32_at(gvar, 16) as usize;
        let i = usize::from(index);
        if i >= glyphs {
            return None;
        }
        let offset = |k: usize| {
            if long {
                u32_at(gvar, 20 + k * 4) as usize
            } else {
                usize::from(u16_at(gvar, 20 + k * 2)) * 2
            }
        };
        let (start, end) = (offset(i), offset(i + 1));
        if end == start {
            return None;
        }
        let data = &gvar[array_at + start..array_at + end];
        let tuple =
            |at: usize| -> Vec<i16> { (0..axes).map(|a| i16_at(data, at + a * 2)).collect() };
        let shared_tuple = |k: usize| -> Vec<i16> {
            assert!(k < shared_count, "a shared tuple");
            (0..axes)
                .map(|a| i16_at(gvar, shared_at + (k * axes + a) * 2))
                .collect()
        };
        let packed = u16_at(data, 0);
        let mut serialized = usize::from(u16_at(data, 2));
        let shared_points = if packed & 0x8000 != 0 {
            Some(points(data, &mut serialized))
        } else {
            None
        };
        let mut at = 4;
        let mut tuples = Vec::new();
        for _ in 0..(packed & 0x0FFF) {
            let size = usize::from(u16_at(data, at));
            let flags = u16_at(data, at + 2);
            at += 4;
            let peak = if flags & 0x8000 != 0 {
                let peak = tuple(at);
                at += axes * 2;
                peak
            } else {
                shared_tuple(usize::from(flags & 0x0FFF))
            };
            let intermediate = (flags & 0x4000 != 0).then(|| {
                let between = (tuple(at), tuple(at + axes * 2));
                at += axes * 4;
                between
            });
            let mut cursor = serialized;
            let private = (flags & 0x2000 != 0).then(|| points(data, &mut cursor));
            let numbers = private
                .clone()
                .or_else(|| shared_points.clone())
                .expect("points, private or shared");
            let count = numbers.as_ref().map_or(items, Vec::len);
            let values = deltas(data, &mut cursor, count * 2);
            serialized += size;
            tuples.push(TupleSpec {
                peak,
                intermediate,
                shared_peak: None,
                points: private.map(|p| p.map_or(Points::All, Points::Listed)),
                deltas: values,
            });
        }
        Some(GlyphVariation {
            shared_points: shared_points.map(|p| p.map_or(Points::All, Points::Listed)),
            tuples,
        })
    }
}

/// A `TestGPOSFour.ttf` glyph's items with their default positions — its
/// points, or one placeholder a component — and its phantom points, from the
/// second reader's `hmtx` metrics and `hhea`'s ascender and descender.
fn gpos_four_items(glyph: Option<&outline_reader::Glyph>, index: u16) -> Vec<(i32, i32)> {
    let (advance, lsb) = outline_reader::horizontal(TEST_GPOS_FOUR, index);
    let (ascender, descender) = outline_reader::ascender_descender(TEST_GPOS_FOUR);
    let (mut items, x_min) = match glyph {
        Some(g) if g.components.is_empty() => (g.points.clone(), g.x_min),
        Some(g) => (vec![(0, 0); g.components.len()], g.x_min),
        None => (Vec::new(), 0),
    };
    items.extend([
        (x_min - lsb, 0),
        (x_min - lsb + advance, 0),
        (0, ascender),
        (0, descender),
    ]);
    items
}

/// A `TestGPOSFour.ttf` glyph's net deltas over its items at 2.14
/// coordinates, from what the second reader decoded.
fn gpos_four_deltas(
    glyph: Option<&outline_reader::Glyph>,
    index: u16,
    items: &[(i32, i32)],
    coordinates: &[i16],
) -> Vec<[i64; 2]> {
    let Some(variation) = outline_reader::variations(TEST_GPOS_FOUR, index, items.len()) else {
        return vec![[0, 0]; items.len()];
    };
    let contours = match glyph {
        Some(g) if !g.components.is_empty() => None,
        Some(g) => Some(g.contours.clone()),
        None => Some(Vec::new()),
    };
    spec::glyph_deltas(items, contours.as_deref(), &variation, coordinates)
}

/// A `TestGPOSFour.ttf` glyph's phantom points at 2.14 coordinates, from the
/// second reader: its default ones moved by its deltas, or its
/// `USE_MY_METRICS` component's.
fn gpos_four_phantoms(index: u16, coordinates: &[i16]) -> [[i64; 2]; 4] {
    let glyph = outline_reader::glyph(TEST_GPOS_FOUR, index);
    if let Some(metrics) = glyph
        .as_ref()
        .and_then(|g| g.components.iter().rev().find(|c| c.flags & 0x0200 != 0))
    {
        return gpos_four_phantoms(metrics.glyph, coordinates);
    }
    let items = gpos_four_items(glyph.as_ref(), index);
    let deltas = gpos_four_deltas(glyph.as_ref(), index, &items, coordinates);
    let n = items.len() - 4;
    let mut out = [[0i64; 2]; 4];
    for (k, slot) in out.iter_mut().enumerate() {
        *slot = [
            i64::from(items[n + k].0) * 65_536 + deltas[n + k][0],
            i64::from(items[n + k].1) * 65_536 + deltas[n + k][1],
        ];
    }
    out
}

/// A `TestGPOSFour.ttf` glyph's outline at 2.14 coordinates, from the second
/// reader: its points, or its components' offsets, plus the deltas the
/// specification computes from what that reader decoded.
fn gpos_four_outline(
    index: u16,
    coordinates: &[i16],
    placement: Placement,
    out: &mut Vec<Segment>,
) {
    let Some(glyph) = outline_reader::glyph(TEST_GPOS_FOUR, index) else {
        return;
    };
    let items = gpos_four_items(Some(&glyph), index);
    let deltas = gpos_four_deltas(Some(&glyph), index, &items, coordinates);
    if !glyph.components.is_empty() {
        for (p, component) in glyph.components.iter().enumerate() {
            let offset = (
                units(i64::from(component.x) * 65_536 + deltas[p][0]),
                units(i64::from(component.y) * 65_536 + deltas[p][1]),
            );
            let child = component_placement(placement, [1.0, 0.0, 0.0, 1.0], offset);
            gpos_four_outline(component.glyph, coordinates, child, out);
        }
        return;
    }
    let positions: Vec<(f64, f64)> = glyph
        .points
        .iter()
        .zip(&deltas)
        .map(|(p, d)| {
            (
                units(i64::from(p.0) * 65_536 + d[0]),
                units(i64::from(p.1) * 65_536 + d[1]),
            )
        })
        .collect();
    drawn(
        &positions,
        &glyph.on,
        &glyph.contours,
        &placed(placement),
        out,
    );
}

#[test]
fn test_gpos_fours_glyphs_at_its_named_instances_equal_a_second_readers() {
    let sfnt = Sfnt::parse(TEST_GPOS_FOUR).expect("the face parses");
    let space = DesignSpace::read(&sfnt).expect("its design space");
    let gvar = Gvar::read(&sfnt, &space)
        .expect("gvar is read")
        .expect("it carries one");
    assert_eq!(gvar.glyph_count(), 13);
    assert!(
        sfnt.table(u32::from_be_bytes(*b"HVAR")).is_none(),
        "no HVAR"
    );
    let mut moved = 0usize;
    for index in 0..u16::try_from(space.instances().len()).expect("few") {
        let location = space.location(&Instance::Named(index)).expect("a location");
        let coordinates = location.coordinates().to_vec();
        for glyph in 0u16..13 {
            let mut expected = Vec::new();
            gpos_four_outline(glyph, &coordinates, UNPLACED, &mut expected);
            let outline =
                glyf::outline_at(&sfnt, glyph, Some(&gvar), &location).expect("an outline");
            assert_eq!(
                outline.segments, expected,
                "glyph {glyph} at named instance {index}"
            );
            if outline.segments != glyf::outline(&sfnt, glyph).expect("an outline").segments {
                moved += 1;
            }
            let phantom =
                glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), &location).expect("points");
            assert_eq!(
                phantom.map(|p| [p[0].bits(), p[1].bits()]),
                gpos_four_phantoms(glyph, &coordinates),
                "glyph {glyph}'s phantom points at named instance {index}"
            );
        }
    }
    assert!(moved > 0, "the named instances move the outlines");
}

// ---- gvar's refusals and bounds ------------------------------------------------

#[test]
fn hostile_gvar_tables_are_refused_by_name() {
    let good = variable_support::gvar(2, &gvar_shared_tuples(), &gvar_variations(), false);
    assert!(Gvar::parse(&good, 2).is_ok());
    let tag = *b"gvar";
    assert_eq!(
        Gvar::parse(&good[..19], 2).err(),
        Some(VariationError::HeaderPastEnd { table: tag })
    );
    let mut version = good.clone();
    version[0..2].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(
        Gvar::parse(&version, 2).err(),
        Some(VariationError::UnknownVersion {
            table: tag,
            major: 2,
            minor: 0
        })
    );
    assert_eq!(
        Gvar::parse(&good, 3).err(),
        Some(VariationError::AxisCount {
            table: tag,
            found: 2,
            expected: 3
        })
    );
    // Offsets for more glyphs than the table holds.
    let mut glyphs = good.clone();
    glyphs[12..14].copy_from_slice(&40_000u16.to_be_bytes());
    assert_eq!(
        Gvar::parse(&glyphs, 2).err(),
        Some(VariationError::GlyphOffsetsPastEnd { count: 40_000 })
    );
    // Shared tuples past the end, by their count and by their offset.
    let mut shared = good.clone();
    shared[6..8].copy_from_slice(&30_000u16.to_be_bytes());
    assert_eq!(
        Gvar::parse(&shared, 2).err(),
        Some(VariationError::SharedTuplesPastEnd { count: 30_000 })
    );
    let mut shared_at = good.clone();
    shared_at[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        Gvar::parse(&shared_at, 2).err(),
        Some(VariationError::SharedTuplesPastEnd { count: 3 })
    );
    // Glyph 1's offsets decreasing, and its data past the table.
    let mut decreasing = good.clone();
    decreasing[22..24].copy_from_slice(&u16::MAX.to_be_bytes());
    let parsed = Gvar::parse(&decreasing, 2).expect("the header is sound");
    assert_eq!(
        parsed.glyph(1).err(),
        Some(VariationError::GlyphDataOffsets { glyph: 1 })
    );
    let mut past = good.clone();
    past[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    let parsed = Gvar::parse(&past, 2).expect("the header is sound");
    assert_eq!(
        parsed.glyph(1).err(),
        Some(VariationError::GlyphDataPastEnd { glyph: 1 })
    );
    assert!(
        parsed.glyph(5).expect("no data").is_none(),
        "a glyph with no data reads none"
    );
    // A glyph past the table's count has no data.
    let parsed = Gvar::parse(&good, 2).expect("parses");
    assert!(parsed.glyph(6).expect("none").is_none());
    assert!(parsed.glyph(u16::MAX).expect("none").is_none());
    // A refusal reaches the outline and the phantom points, which a caller
    // then draws at its default.
    let face = gvar_face(false, false);
    let sfnt = Sfnt::parse(&face).expect("a face");
    let gvar = Gvar::parse(&past, 2).expect("the header is sound");
    let location = Location::new(vec![16_384, 0]);
    for glyph in [1u16, 3, 4] {
        assert_eq!(
            glyf::outline_at(&sfnt, glyph, Some(&gvar), &location).err(),
            Some(VariationError::GlyphDataPastEnd { glyph }),
            "glyph {glyph}"
        );
        assert_eq!(
            glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), &location).err(),
            Some(VariationError::GlyphDataPastEnd {
                glyph: if glyph == 3 { 1 } else { glyph }
            }),
            "glyph {glyph}'s phantom points, glyph 3's its USE_MY_METRICS component's"
        );
    }
    assert!(
        glyf::outline_at(&sfnt, 5, Some(&gvar), &location).is_ok(),
        "a glyph with no data draws"
    );
    // A component's refusal is the composite's: glyph 3's own data reads,
    // and glyph 1's offsets decrease.
    let gvar = Gvar::parse(&decreasing, 2).expect("the header is sound");
    assert!(gvar.glyph(3).is_ok());
    assert_eq!(
        glyf::outline_at(&sfnt, 3, Some(&gvar), &location).err(),
        Some(VariationError::GlyphDataOffsets { glyph: 1 })
    );
}

/// A one-contour glyph of `points` on-curve points, and variation data of
/// `tables` tables each moving every point by nothing.
fn heavy_glyph(points: usize, tables: usize) -> (Vec<u8>, GlyphVariation) {
    let contour: Vec<GlyphPoint> = (0..points)
        .map(|i| {
            (
                i32::try_from(i % 500).expect("small"),
                i32::try_from(i / 500).expect("small"),
                true,
            )
        })
        .collect();
    let deltas = vec![0; (points + 4) * 2];
    let tuples = (0..tables)
        .map(|_| TupleSpec::new(&[16_384, 0], Some(Points::All), &deltas))
        .collect();
    (
        variable_support::simple_glyph(&[contour]),
        GlyphVariation {
            shared_points: None,
            tuples,
        },
    )
}

/// A face of the heavy glyph and a composite drawing it `draws` times.
fn heavy_face(points: usize, tables: usize, draws: usize) -> Vec<u8> {
    let (glyph, variation) = heavy_glyph(points, tables);
    let records: Vec<ComponentSpec> = (0..draws)
        .map(|i| ComponentSpec::at(0, i32::try_from(i).expect("few"), 0))
        .collect();
    let composite = composite_glyph([0, 0, 500, 10], &records);
    let (glyf, loca) = glyf_and_loca(&[glyph, composite]);
    Face::truetype()
        .with(*b"fvar", fvar(&two_axes(), &six_instances()))
        .with(*b"glyf", glyf)
        .with(*b"loca", loca)
        .with(*b"head", head(1_000))
        .with(*b"maxp", maxp(2))
        .with(*b"hhea", hhea(2, (1, 0, 0)))
        .with(*b"hmtx", metrics(&[(500, 0), (500, 0)], &[]))
        .with(
            *b"gvar",
            variable_support::gvar(2, &[], &[Some(variation), None], true),
        )
        .build()
}

#[test]
fn the_delta_work_of_an_outline_is_bounded() {
    // 1 024 tables over 4 092 points and four phantom points is the bound
    // exactly, and one table more passes it.
    assert_eq!(MAX_GLYPH_DELTA_WORK, 1_024 * 4_096);
    for (tables, refused) in [(1_024usize, false), (1_025, true)] {
        let face = heavy_face(4_092, tables, 1);
        let sfnt = Sfnt::parse(&face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space).expect("read").expect("present");
        // Refused or not wherever it is asked, the default included.
        for location in [space.default_location(), Location::new(vec![16_384, 0])] {
            let outline = glyf::outline_at(&sfnt, 0, Some(&gvar), &location);
            let raw = glyf::simple_glyph(&sfnt, 0).expect("a simple glyph");
            if refused {
                assert_eq!(
                    outline.err(),
                    Some(VariationError::GlyphDeltaWork { glyph: 0 })
                );
                assert_eq!(
                    gvar.simple_deltas(0, &raw, &location).err(),
                    Some(VariationError::GlyphDeltaWork { glyph: 0 })
                );
            } else {
                assert_eq!(
                    outline.expect("an outline").segments,
                    glyf::outline(&sfnt, 0).expect("an outline").segments,
                    "zero deltas move nothing"
                );
                assert!(gvar.simple_deltas(0, &raw, &location).is_ok());
            }
        }
    }
    // The bound is the whole outline's: a composite that draws a glyph of
    // two thirds of it twice is refused, and once is not.
    for (draws, refused) in [(1usize, false), (2, true)] {
        let face = heavy_face(4_092, 700, draws);
        let sfnt = Sfnt::parse(&face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space).expect("read").expect("present");
        let location = Location::new(vec![16_384, 0]);
        let outline = glyf::outline_at(&sfnt, 1, Some(&gvar), &location);
        if refused {
            assert_eq!(
                outline.err(),
                Some(VariationError::GlyphDeltaWork { glyph: 0 }),
                "{draws} draws"
            );
        } else {
            assert!(outline.is_ok(), "{draws} draws");
        }
        // The glyph alone stays within it however often it is asked for.
        assert!(glyf::outline_at(&sfnt, 0, Some(&gvar), &location).is_ok());
    }
}

#[test]
fn every_truncation_and_byte_change_of_a_gvar_is_read_or_refused_without_panicking() {
    let table = variable_support::gvar(2, &gvar_shared_tuples(), &gvar_variations(), false);
    let face = gvar_face(false, true);
    let sfnt = Sfnt::parse(&face).expect("a face");
    let locations = [
        Location::new(vec![16_384, 0]),
        Location::new(vec![-8_192, 12_000]),
        Location::new(vec![4_096, -16_384]),
    ];
    let exercise = |bytes: &[u8]| {
        let Ok(gvar) = Gvar::parse(bytes, 2) else {
            return;
        };
        for glyph in 0u16..7 {
            let _ = gvar.glyph(glyph);
            for location in &locations {
                let _ = glyf::outline_at(&sfnt, glyph, Some(&gvar), location);
                let _ = glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), location);
                if let Some(raw) = glyf::simple_glyph(&sfnt, glyph) {
                    let _ = gvar.simple_deltas(glyph, &raw, location);
                }
                let _ = gvar.composite_deltas(glyph, 3, location);
                let _ = gvar.phantom_deltas(glyph, 9, false, location);
            }
        }
    };
    for cut in 0..table.len() {
        exercise(&table[..cut]);
    }
    for at in 0..table.len() {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut changed = table.clone();
            changed[at] = value;
            exercise(&changed);
        }
    }
}

// ---- The static instancer for glyf faces (FT-05.10) ---------------------------

/// The committed `glyf` face (`tests/variable/`, FT-05.8).
const GLYF_FACE: &[u8] = include_bytes!("variable/variable-glyf.ttf");

/// A 16.16 value rounded to font units, a half rounding up: the numeric
/// model's rule for every value a static instance writes.
fn to_units(bits: i64) -> i64 {
    spec::round_units(bits)
}

/// The two faces the instancer is held to, by name.
fn instanced_faces() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("variable-glyf.ttf", GLYF_FACE),
        ("TestGPOSFour.ttf", TEST_GPOS_FOUR),
    ]
}

/// The instances a face is written at: every named instance, the default,
/// and user coordinates no instance names — one past an axis's maximum, one
/// naming an axis the face lacks.
fn instancer_instances(face: &[u8]) -> Vec<Instance> {
    let space = space_of(face);
    let mut out: Vec<Instance> = (0..space.instances().len())
        .map(|i| Instance::Named(u16::try_from(i).expect("few")))
        .collect();
    out.push(Instance::default());
    out.push(Instance::User(vec![
        UserCoordinate::new(*b"wght", 250.0),
        UserCoordinate::new(*b"opsz", 30.0),
    ]));
    out.push(Instance::User(vec![
        UserCoordinate::new(*b"wght", 813.5),
        UserCoordinate::new(*b"wdth", 90.0),
        UserCoordinate::new(*b"slnt", -7.25),
    ]));
    out.push(Instance::User(vec![
        UserCoordinate::new(*b"wdth", 117.0),
        UserCoordinate::new(*b"opsz", 9.0),
        UserCoordinate::new(*b"wght", 1_200.0),
    ]));
    out.push(Instance::User(vec![UserCoordinate::new(*b"wdth", 73.0)]));
    out
}

/// What the variable face gives one glyph at a location, rounded as a static
/// instance stores it: a simple glyph's contour points moved by `gvar`, or a
/// composite's records with their offsets moved — a matched component's
/// offset drawn as zero, as the outline reader draws it.
#[derive(Clone, Debug)]
enum Varied {
    Empty,
    Simple {
        points: Vec<(i64, i64)>,
        on: Vec<bool>,
        contours: Vec<(usize, usize)>,
        instructions: Vec<u8>,
    },
    Composite(Vec<(glyf::Component, (i64, i64))>),
}

fn varied_at(sfnt: &Sfnt, gvar: &Gvar, location: &Location, glyph: u16) -> Varied {
    if let Some(components) = glyf::components(sfnt, glyph) {
        let deltas = gvar
            .composite_deltas(glyph, components.len(), location)
            .expect("the deltas");
        return Varied::Composite(
            components
                .into_iter()
                .zip(deltas)
                .map(|(component, delta)| {
                    let offset = if component.offset_is_xy() {
                        (
                            to_units(i64::from(component.arguments[0]) * 65_536 + delta[0].bits()),
                            to_units(i64::from(component.arguments[1]) * 65_536 + delta[1].bits()),
                        )
                    } else {
                        (0, 0)
                    };
                    (component, offset)
                })
                .collect(),
        );
    }
    let Some(raw) = glyf::simple_glyph(sfnt, glyph) else {
        return Varied::Empty;
    };
    if raw.contour_points() == 0 {
        return Varied::Empty;
    }
    let deltas = gvar
        .simple_deltas(glyph, &raw, location)
        .expect("the deltas");
    let points = (0..raw.contour_points())
        .map(|i| {
            (
                to_units(i64::from(raw.points[i][0]) * 65_536 + deltas[i][0].bits()),
                to_units(i64::from(raw.points[i][1]) * 65_536 + deltas[i][1].bits()),
            )
        })
        .collect();
    Varied::Simple {
        points,
        on: raw.flags.iter().map(|f| f & 1 != 0).collect(),
        contours: raw.contours(),
        instructions: raw.instructions.to_vec(),
    }
}

/// The outline a face holding the varied glyphs draws: the contour reading
/// over each simple glyph's rounded points, each component placed by its
/// rounded offset and its own 2×2.
fn varied_outline(
    varied: &[Varied],
    glyph: usize,
    placement: Placement,
    depth: u32,
    out: &mut Vec<Segment>,
) {
    if depth > 8 {
        return;
    }
    match &varied[glyph] {
        Varied::Empty => {}
        Varied::Simple {
            points,
            on,
            contours,
            ..
        } => {
            let positions: Vec<(f64, f64)> =
                points.iter().map(|p| (p.0 as f64, p.1 as f64)).collect();
            drawn(&positions, on, contours, &placed(placement), out);
        }
        Varied::Composite(records) => {
            for (record, offset) in records {
                let matrix = record.transform.map(|v| f64::from(v) / 16_384.0);
                let child =
                    component_placement(placement, matrix, (offset.0 as f64, offset.1 as f64));
                varied_outline(varied, usize::from(record.glyph), child, depth + 1, out);
            }
        }
    }
}

/// The points a varied glyph draws, placed — a composite's matched component
/// moved so that its point lands on the composite's, as the `glyf` chapter
/// places it, which is what a box measures.
fn varied_points(
    varied: &[Varied],
    glyph: usize,
    placement: Placement,
    depth: u32,
    out: &mut Vec<(f64, f64)>,
) {
    if depth > 8 {
        return;
    }
    let place = placed(placement);
    match &varied[glyph] {
        Varied::Empty => {}
        Varied::Simple { points, .. } => {
            out.extend(points.iter().map(|p| place(p.0 as f64, p.1 as f64)));
        }
        Varied::Composite(records) => {
            let base = out.len();
            for (record, offset) in records {
                let matrix = record.transform.map(|v| f64::from(v) / 16_384.0);
                if record.offset_is_xy() {
                    let child =
                        component_placement(placement, matrix, (offset.0 as f64, offset.1 as f64));
                    varied_points(varied, usize::from(record.glyph), child, depth + 1, out);
                } else {
                    let child = component_placement(placement, matrix, (0.0, 0.0));
                    let mut own = Vec::new();
                    varied_points(
                        varied,
                        usize::from(record.glyph),
                        child,
                        depth + 1,
                        &mut own,
                    );
                    let target = usize::try_from(record.arguments[0])
                        .ok()
                        .and_then(|p| out.get(base + p).copied());
                    let source = usize::try_from(record.arguments[1])
                        .ok()
                        .and_then(|p| own.get(p).copied());
                    let shift = match (target, source) {
                        (Some(t), Some(s)) => (t.0 - s.0, t.1 - s.1),
                        _ => (0.0, 0.0),
                    };
                    out.extend(own.iter().map(|p| (p.0 + shift.0, p.1 + shift.1)));
                }
            }
        }
    }
}

/// A varied glyph's box: its placed points floored and ceiled.
fn varied_box(varied: &[Varied], glyph: usize) -> Option<[i64; 4]> {
    let mut points = Vec::new();
    varied_points(varied, glyph, UNPLACED, 0, &mut points);
    if points.is_empty() {
        return None;
    }
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    Some([
        min_x.floor() as i64,
        min_y.floor() as i64,
        max_x.ceil() as i64,
        max_y.ceil() as i64,
    ])
}

/// The advance the variable face gives a glyph at a location, rounded:
/// `HVAR`'s where it has one, the phantom points' where it does not.
fn varied_advance(sfnt: &Sfnt, gvar: &Gvar, location: &Location, glyph: u16) -> i64 {
    let space = DesignSpace::read(sfnt).expect("a design space");
    match Hvar::read(sfnt, &space).expect("HVAR reads") {
        Some(hvar) => to_units(
            hvar.advance_at(sfnt, glyph, location)
                .expect("an advance")
                .expect("hmtx states one")
                .bits(),
        ),
        None => {
            let p = glyf::phantom_points_at(sfnt, glyph, Some(gvar), location).expect("phantoms");
            to_units(p[1][0].saturating_sub(p[0][0]).bits())
        }
    }
}

/// A rounded metric held to its field's width, as `MVAR` names it.
fn held_to_field(tag: Tag, value: i64) -> i64 {
    let field = METRIC_FIELDS
        .iter()
        .find(|field| field.tag == tag)
        .expect("a tag MVAR defines");
    if field.signed {
        value.clamp(i64::from(i16::MIN), i64::from(i16::MAX))
    } else {
        value.clamp(0, i64::from(u16::MAX))
    }
}

/// Whether a value tag names `gasp`'s last range, which always reads 0xFFFF.
fn last_gasp_range(sfnt: &Sfnt, tag: Tag) -> bool {
    let Some(rest) = tag.strip_prefix(b"gsp") else {
        return false;
    };
    let range = usize::from(rest[0] - b'0');
    let ranges = sfnt.table(u32::from_be_bytes(*b"gasp")).map_or(0, |gasp| {
        usize::from(u16::from_be_bytes([gasp[2], gasp[3]]))
    });
    range + 1 >= ranges
}

/// The instance's bytes, or a panic naming the face and the instance.
fn instance_of(name: &str, face: &[u8], instance: &Instance) -> Vec<u8> {
    instantiate(face, instance).unwrap_or_else(|e| panic!("{name} at {instance:?}: {e}"))
}

#[test]
fn a_static_instance_re_reads_to_the_instance() {
    for (name, face) in instanced_faces() {
        let sfnt = Sfnt::parse(face).expect("a face");
        let space = DesignSpace::read(&sfnt).expect("a design space");
        let gvar = Gvar::read(&sfnt, &space)
            .expect("gvar reads")
            .expect("the face varies its outlines");
        let mvar = Mvar::read(&sfnt, &space).expect("MVAR reads");
        let count = sfnt.glyph_count().expect("maxp");
        for instance in instancer_instances(face) {
            let what = format!("{name} at {instance:?}");
            let location = space.location(&instance).expect("a location");
            let bytes = instance_of(name, face, &instance);
            let static_face = Sfnt::parse(&bytes).expect("the instance parses");
            assert_eq!(static_face.glyph_count(), Some(count), "{what}: the glyphs");
            let varied: Vec<Varied> = (0..count)
                .map(|g| varied_at(&sfnt, &gvar, &location, g))
                .collect();
            for glyph in 0..count {
                let mut expected = Vec::new();
                varied_outline(&varied, usize::from(glyph), UNPLACED, 0, &mut expected);
                let outline = glyf::outline(&static_face, glyph).expect("an outline");
                assert_eq!(
                    outline.segments, expected,
                    "{what}: glyph {glyph}'s outline"
                );
                if let Varied::Simple {
                    points,
                    on,
                    instructions,
                    ..
                } = &varied[usize::from(glyph)]
                {
                    let raw = glyf::simple_glyph(&static_face, glyph).expect("a simple glyph");
                    assert_eq!(
                        raw.instructions,
                        instructions.as_slice(),
                        "{what}: glyph {glyph}'s instructions"
                    );
                    let stored: Vec<(i64, i64)> = raw.points[..raw.contour_points()]
                        .iter()
                        .map(|p| (i64::from(p[0]), i64::from(p[1])))
                        .collect();
                    assert_eq!(&stored, points, "{what}: glyph {glyph}'s points");
                    let flags: Vec<bool> = raw.flags.iter().map(|f| f & 1 != 0).collect();
                    assert_eq!(&flags, on, "{what}: glyph {glyph}'s on-curve flags");
                }
                assert_eq!(
                    static_face.advance(glyph).map(i64::from),
                    Some(varied_advance(&sfnt, &gvar, &location, glyph)),
                    "{what}: glyph {glyph}'s advance"
                );
            }
            if let Some(stored) = static_face.table(u32::from_be_bytes(*b"cvt ")) {
                let expected: Vec<i64> = cvt_at(&sfnt, &space, &location)
                    .expect("cvar applies")
                    .iter()
                    .map(|v| to_units(v.bits()))
                    .collect();
                let read: Vec<i64> = stored
                    .chunks(2)
                    .map(|b| i64::from(i16::from_be_bytes([b[0], b[1]])))
                    .collect();
                assert_eq!(read, expected, "{what}: the cvt");
            }
            if let Some(mvar) = &mvar {
                for tag in mvar.tags() {
                    if last_gasp_range(&sfnt, tag) {
                        continue;
                    }
                    let Some(value) = mvar.value_at(&sfnt, tag, &location).expect("an MVAR value")
                    else {
                        continue;
                    };
                    assert_eq!(
                        metric_default(&static_face, tag).map(i64::from),
                        Some(held_to_field(tag, to_units(value.bits()))),
                        "{what}: {}",
                        String::from_utf8_lossy(&tag)
                    );
                }
            }
        }
    }
}

#[test]
fn the_instance_carries_no_variation_table() {
    for (name, face) in instanced_faces() {
        let original = Sfnt::parse(face).expect("a face");
        for instance in instancer_instances(face) {
            let what = format!("{name} at {instance:?}");
            let bytes = instance_of(name, face, &instance);
            let static_face = Sfnt::parse(&bytes).expect("the instance parses");
            for tag in DROPPED {
                assert!(
                    static_face.table(u32::from_be_bytes(*tag)).is_none(),
                    "{what}: {} is gone",
                    String::from_utf8_lossy(tag)
                );
            }
            assert!(
                !DesignSpace::read(&static_face)
                    .expect("no fvar reads as no axes")
                    .is_variable(),
                "{what}: the instance does not vary"
            );
            // Every other table is there, and those nothing at an instance
            // changes are there byte for byte.
            let mut kept: Vec<[u8; 4]> = original
                .tags()
                .into_iter()
                .filter(|tag| !DROPPED.contains(tag))
                .collect();
            kept.sort_unstable();
            let mut written = static_face.tags();
            written.sort_unstable();
            assert_eq!(written, kept, "{what}: its tables");
            for tag in [b"cmap", b"name", b"fpgm", b"GSUB", b"GPOS", b"GDEF"] {
                let tag = u32::from_be_bytes(*tag);
                assert_eq!(
                    static_face.table(tag),
                    original.table(tag),
                    "{what}: a table copied as it is"
                );
            }
        }
    }
}

#[test]
fn the_default_instance_keeps_the_default_outlines() {
    for (name, face) in instanced_faces() {
        let original = Sfnt::parse(face).expect("a face");
        let space = DesignSpace::read(&original).expect("a design space");
        let mvar = Mvar::read(&original, &space).expect("MVAR reads");
        let mut defaults = vec![Instance::default()];
        // A named instance at the default, where the face has one.
        if let Some(index) = space.instances().iter().position(|named| {
            named
                .coordinates
                .iter()
                .zip(space.axes())
                .all(|(c, a)| *c == a.default)
        }) {
            defaults.push(Instance::Named(u16::try_from(index).expect("few")));
        }
        for instance in defaults {
            let what = format!("{name} at {instance:?}");
            let bytes = instance_of(name, face, &instance);
            let static_face = Sfnt::parse(&bytes).expect("the instance parses");
            for glyph in 0..original.glyph_count().expect("maxp") {
                assert_eq!(
                    glyf::outline(&static_face, glyph).map(|o| o.segments),
                    glyf::outline(&original, glyph).map(|o| o.segments),
                    "{what}: glyph {glyph}'s outline"
                );
                assert_eq!(
                    static_face.advance(glyph),
                    original.advance(glyph),
                    "{what}: glyph {glyph}'s advance"
                );
                if let (Some(a), Some(b)) = (
                    glyf::simple_glyph(&static_face, glyph),
                    glyf::simple_glyph(&original, glyph),
                ) {
                    assert_eq!(
                        a.instructions, b.instructions,
                        "{what}: glyph {glyph}'s instructions"
                    );
                }
            }
            let cvt = u32::from_be_bytes(*b"cvt ");
            assert_eq!(
                static_face.table(cvt),
                original.table(cvt),
                "{what}: the cvt"
            );
            if let Some(mvar) = &mvar {
                for tag in mvar.tags() {
                    assert_eq!(
                        metric_default(&static_face, tag),
                        metric_default(&original, tag),
                        "{what}: {}",
                        String::from_utf8_lossy(&tag)
                    );
                }
            }
        }
    }
}

/// The `wdth` registration's mapping to `usWidthClass`, in exact rationals:
/// the `OS/2` chapter's percentages, interpolated between the classes
/// either side, rounded half up and held to 1 to 9.
fn expected_width_class(width: f64) -> u16 {
    let percent = [50.0, 62.5, 75.0, 87.5, 100.0, 112.5, 125.0, 150.0, 200.0];
    if width <= percent[0] {
        return 1;
    }
    if width >= percent[8] {
        return 9;
    }
    let k = (0..8)
        .find(|k| width <= percent[k + 1])
        .expect("inside the table");
    let class = (k + 1) as f64 + (width - percent[k]) / (percent[k + 1] - percent[k]);
    ((class + 0.5).floor() as u16).clamp(1, 9)
}

#[test]
fn the_instance_writes_the_fields_mvar_leaves_to_the_axes() {
    let fixture = glyf_fixture();
    let (wght, wdth, slnt) = (fixture.axes[0], fixture.axes[1], fixture.axes[2]);
    let mut cases: Vec<(Instance, [f64; 3])> = fixture
        .instances
        .iter()
        .enumerate()
        .map(|(i, named)| {
            let c = &named.record.coordinates;
            (
                Instance::Named(u16::try_from(i).expect("few")),
                [c[0], c[1], c[2]],
            )
        })
        .collect();
    cases.push((Instance::default(), [400.0, 100.0, 0.0]));
    for (weight, width, slant) in [
        (813.5, 90.0, -7.25),
        (250.0, 106.25, -0.5),
        (1_200.0, 81.25, -20.0),
        (20.0, 130.0, 5.0),
        (450.25, 93.75, -11.75),
    ] {
        cases.push((
            Instance::User(vec![
                UserCoordinate::new(*b"wght", weight),
                UserCoordinate::new(*b"wdth", width),
                UserCoordinate::new(*b"slnt", slant),
            ]),
            [weight, width, slant],
        ));
    }
    for (instance, [weight, width, slant]) in cases {
        let what = format!("{instance:?}");
        let bytes = instance_of("variable-glyf.ttf", GLYF_FACE, &instance);
        let static_face = Sfnt::parse(&bytes).expect("the instance parses");
        let os2 = static_face
            .table(u32::from_be_bytes(*b"OS/2"))
            .expect("OS/2");
        let post = static_face
            .table(u32::from_be_bytes(*b"post"))
            .expect("post");
        // Each user value held to its axis, as the instance is.
        let weight = weight.clamp(wght.min, wght.max);
        let width = width.clamp(wdth.min, wdth.max);
        let slant = slant.clamp(slnt.min, slnt.max);
        assert_eq!(
            u16::from_be_bytes([os2[4], os2[5]]),
            ((weight + 0.5).floor() as u16).clamp(1, 1_000),
            "{what}: usWeightClass"
        );
        assert_eq!(
            u16::from_be_bytes([os2[6], os2[7]]),
            expected_width_class(width),
            "{what}: usWidthClass"
        );
        assert_eq!(
            i32::from_be_bytes([post[4], post[5], post[6], post[7]]),
            (slant * 65_536.0) as i32,
            "{what}: italicAngle, counter-clockwise as slnt is"
        );
    }
}

#[test]
fn a_hinted_face_gets_an_idef_for_getvariation_that_maxp_covers() {
    let original = Sfnt::parse(GLYF_FACE).expect("a face");
    let space = DesignSpace::read(&original).expect("a design space");
    let prep = original
        .table(u32::from_be_bytes(*b"prep"))
        .expect("the face carries prep");
    let maxp = original.table(u32::from_be_bytes(*b"maxp")).expect("maxp");
    let be16 = |data: &[u8], at: usize| u16::from_be_bytes([data[at], data[at + 1]]);
    for instance in instancer_instances(GLYF_FACE) {
        let what = format!("{instance:?}");
        let location = space.location(&instance).expect("a location");
        let bytes = instance_of("variable-glyf.ttf", GLYF_FACE, &instance);
        let static_face = Sfnt::parse(&bytes).expect("the instance parses");
        // PUSHB[000] 145, IDEF[], NPUSHW[] 4 and the four coordinates, first
        // axis first, ENDF[]: the instruction set's form, after the face's
        // own program.
        let mut expected = prep.to_vec();
        expected.extend_from_slice(&[0xB0, 0x91, 0x89, 0x41, 4]);
        for coordinate in location.coordinates() {
            expected.extend_from_slice(&coordinate.to_be_bytes());
        }
        expected.push(0x2D);
        assert_eq!(
            static_face.table(u32::from_be_bytes(*b"prep")),
            Some(expected.as_slice()),
            "{what}: prep"
        );
        let written = static_face
            .table(u32::from_be_bytes(*b"maxp"))
            .expect("maxp");
        assert_eq!(
            be16(written, 22),
            be16(maxp, 22) + 1,
            "{what}: maxInstructionDefs"
        );
        assert_eq!(
            be16(written, 24),
            be16(maxp, 24) + 4,
            "{what}: maxStackElements"
        );
        for at in (0..32).step_by(2).filter(|at| *at != 22 && *at != 24) {
            assert_eq!(be16(written, at), be16(maxp, at), "{what}: maxp at {at}");
        }
    }
    // A face that carries no hinting gets no program and no maxp change.
    let gpos = Sfnt::parse(TEST_GPOS_FOUR).expect("a face");
    for instance in instancer_instances(TEST_GPOS_FOUR) {
        let bytes = instance_of("TestGPOSFour.ttf", TEST_GPOS_FOUR, &instance);
        let static_face = Sfnt::parse(&bytes).expect("the instance parses");
        assert!(static_face.table(u32::from_be_bytes(*b"prep")).is_none());
        assert_eq!(
            static_face.table(u32::from_be_bytes(*b"maxp")),
            gpos.table(u32::from_be_bytes(*b"maxp")),
            "{instance:?}: maxp"
        );
    }
}

/// Every directory checksum is its table's, `head`'s taken with its
/// adjustment zero, and the whole face sums to 0xB1B0AFBA.
fn checksums_hold(what: &str, face: &[u8]) {
    let sum = |data: &[u8]| {
        data.chunks(4).fold(0u32, |sum, chunk| {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    };
    let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
    for i in 0..count {
        let at = 12 + i * 16;
        let tag = &face[at..at + 4];
        let stated = u32::from_be_bytes(face[at + 4..at + 8].try_into().expect("four"));
        let offset = u32::from_be_bytes(face[at + 8..at + 12].try_into().expect("four")) as usize;
        let length = u32::from_be_bytes(face[at + 12..at + 16].try_into().expect("four")) as usize;
        let mut table = face[offset..offset + length].to_vec();
        if tag == b"head" {
            table[8..12].copy_from_slice(&[0; 4]);
        }
        assert_eq!(
            sum(&table),
            stated,
            "{what}: {}'s checksum",
            String::from_utf8_lossy(tag)
        );
    }
    assert_eq!(sum(face), 0xB1B0_AFBA, "{what}: the whole-font checksum");
}

#[test]
fn the_instances_metrics_tables_describe_its_glyphs() {
    let sfnt = Sfnt::parse(GLYF_FACE).expect("a face");
    let space = DesignSpace::read(&sfnt).expect("a design space");
    let gvar = Gvar::read(&sfnt, &space).expect("gvar").expect("gvar");
    let vvar = Vvar::read(&sfnt, &space).expect("VVAR").expect("VVAR");
    let count = sfnt.glyph_count().expect("maxp");
    let be16 = |data: &[u8], at: usize| u16::from_be_bytes([data[at], data[at + 1]]);
    let s16 = |data: &[u8], at: usize| i64::from(i16::from_be_bytes([data[at], data[at + 1]]));
    for instance in instancer_instances(GLYF_FACE) {
        let what = format!("{instance:?}");
        let location = space.location(&instance).expect("a location");
        let bytes = instance_of("variable-glyf.ttf", GLYF_FACE, &instance);
        checksums_hold(&what, &bytes);
        let static_face = Sfnt::parse(&bytes).expect("the instance parses");
        let varied: Vec<Varied> = (0..count)
            .map(|g| varied_at(&sfnt, &gvar, &location, g))
            .collect();
        let hmtx_count = usize::from(be16(
            static_face
                .table(u32::from_be_bytes(*b"hhea"))
                .expect("hhea"),
            34,
        ));
        let mut drawn_glyphs = Vec::new();
        let mut union: Option<[i64; 4]> = None;
        let mut advances = Vec::new();
        for glyph in 0..count {
            let expected = varied_box(&varied, usize::from(glyph));
            let header = glyf::bounds(&static_face, glyph).expect("a glyph header");
            let advance = i64::from(static_face.advance(glyph).expect("an advance"));
            let lsb = i64::from(static_face.left_side_bearing(glyph).expect("a bearing"));
            advances.push(advance);
            match expected {
                Some(bounds) => {
                    assert_eq!(
                        [
                            i64::from(header.x_min),
                            i64::from(header.y_min),
                            i64::from(header.x_max),
                            i64::from(header.y_max)
                        ],
                        bounds,
                        "{what}: glyph {glyph}'s box"
                    );
                    assert_eq!(
                        lsb, bounds[0],
                        "{what}: glyph {glyph}'s bearing is its xMin"
                    );
                    drawn_glyphs.push((glyph, bounds, advance, lsb));
                    union = Some(match union {
                        None => bounds,
                        Some(u) => [
                            u[0].min(bounds[0]),
                            u[1].min(bounds[1]),
                            u[2].max(bounds[2]),
                            u[3].max(bounds[3]),
                        ],
                    });
                }
                None => assert_eq!(
                    lsb,
                    i64::from(sfnt.left_side_bearing(glyph).expect("a bearing")),
                    "{what}: an empty glyph keeps its bearing"
                ),
            }
            // The vertical metrics: VVAR's advance height, and the top side
            // bearing from the third phantom point to the glyph's top.
            let height = to_units(
                vvar.advance_at(&sfnt, glyph, &location)
                    .expect("a height")
                    .expect("vmtx states one")
                    .bits(),
            );
            assert_eq!(
                static_face.vertical_advance(glyph).map(i64::from),
                Some(height),
                "{what}: glyph {glyph}'s advance height"
            );
            let phantom =
                glyf::phantom_points_at(&sfnt, glyph, Some(&gvar), &location).expect("phantoms");
            let top = expected.map_or(0, |b| b[3]);
            assert_eq!(
                static_face.top_side_bearing(glyph).map(i64::from),
                Some(to_units(phantom[2][1].bits()) - top),
                "{what}: glyph {glyph}'s top side bearing"
            );
        }
        assert!(
            hmtx_count >= 1 && hmtx_count <= usize::from(count),
            "{what}: hmtx's count"
        );
        let hhea = static_face
            .table(u32::from_be_bytes(*b"hhea"))
            .expect("hhea");
        assert_eq!(
            i64::from(be16(hhea, 10)),
            advances.iter().copied().max().expect("glyphs"),
            "{what}: advanceWidthMax"
        );
        assert_eq!(
            s16(hhea, 12),
            drawn_glyphs
                .iter()
                .map(|g| g.3)
                .min()
                .expect("drawn glyphs"),
            "{what}: minLeftSideBearing"
        );
        assert_eq!(
            s16(hhea, 14),
            drawn_glyphs
                .iter()
                .map(|(_, b, advance, lsb)| advance - lsb - (b[2] - b[0]))
                .min()
                .expect("drawn glyphs"),
            "{what}: minRightSideBearing"
        );
        assert_eq!(
            s16(hhea, 16),
            drawn_glyphs
                .iter()
                .map(|(_, b, _, lsb)| lsb + (b[2] - b[0]))
                .max()
                .expect("drawn glyphs"),
            "{what}: xMaxExtent"
        );
        let head = static_face
            .table(u32::from_be_bytes(*b"head"))
            .expect("head");
        assert_eq!(
            Some([s16(head, 36), s16(head, 38), s16(head, 40), s16(head, 42)]),
            union,
            "{what}: the font box is the glyphs' union"
        );
        assert_eq!(
            s16(head, 50),
            0,
            "{what}: the instance's glyf takes the short loca"
        );
        let nonzero: Vec<i64> = advances.iter().copied().filter(|a| *a != 0).collect();
        let n = nonzero.len() as i64;
        let average = (2 * nonzero.iter().sum::<i64>() + n).div_euclid(2 * n);
        let os2 = static_face
            .table(u32::from_be_bytes(*b"OS/2"))
            .expect("OS/2");
        assert_eq!(s16(os2, 2), average, "{what}: xAvgCharWidth");
    }
}

/// A face of one axis whose tables the gasp and refusal tests vary: `fvar`,
/// `head` and `maxp`, and whatever tables are given.
fn one_axis_face(tables: Vec<(Tag, Vec<u8>)>) -> Vec<u8> {
    let mut face = Face::truetype()
        .with(
            *b"fvar",
            fvar(
                &[AxisSpec::new(*b"wght", 100.0, 400.0, 900.0, 256)],
                &[InstanceSpec::new(257, &[900.0], 258)],
            ),
        )
        .with(*b"head", head(1_000))
        .with(*b"maxp", maxp(0));
    for (tag, bytes) in tables {
        face = face.with(tag, bytes);
    }
    face.build()
}

#[test]
fn gasp_keeps_its_last_range_whatever_mvar_says() {
    let store = item_store(
        1,
        &[vec![(0, 16_384, 16_384)]],
        &[Some(DataSpec {
            regions: vec![0],
            word_count: 0,
            long_words: false,
            rows: vec![vec![6], vec![-9]],
        })],
    );
    let face = one_axis_face(vec![
        (*b"gasp", gasp(&[(8, 0x0002), (0xFFFF, 0x000F)])),
        (
            *b"MVAR",
            mvar(&store, &[(*b"gsp0", 0, 0), (*b"gsp1", 0, 1)]),
        ),
    ]);
    let bytes = instantiate(&face, &Instance::Named(0)).expect("an instance");
    let static_face = Sfnt::parse(&bytes).expect("the instance parses");
    let gasp = static_face
        .table(u32::from_be_bytes(*b"gasp"))
        .expect("gasp");
    assert_eq!(
        u16::from_be_bytes([gasp[4], gasp[5]]),
        14,
        "the first range moves"
    );
    assert_eq!(
        u16::from_be_bytes([gasp[8], gasp[9]]),
        0xFFFF,
        "the last range always reads 0xFFFF"
    );
    assert_eq!(&gasp[6..8], &[0x00, 0x02], "the behaviours are kept");
}

#[test]
fn a_face_that_does_not_vary_is_its_own_instance() {
    let (glyf_table, loca) = glyf_and_loca(&[variable_support::simple_glyph(&[vec![
        (0, 0, true),
        (0, 500, true),
        (300, 0, true),
    ]])]);
    let face = Face::truetype()
        .with(*b"head", head(1_000))
        .with(*b"maxp", maxp(1))
        .with(*b"glyf", glyf_table)
        .with(*b"loca", loca)
        .build();
    assert_eq!(
        instantiate(&face, &Instance::default()),
        Ok(face.clone()),
        "no fvar"
    );
    assert_eq!(
        instantiate(
            &face,
            &Instance::User(vec![UserCoordinate::new(*b"wght", 700.0)])
        ),
        Ok(face.clone()),
        "an axis it does not have is ignored"
    );
    assert_eq!(
        instantiate(&face, &Instance::Named(0)),
        Err(InstanceError::Variation(VariationError::NoSuchInstance {
            index: 0,
            count: 0
        }))
    );
    // An fvar of no axes is a face that does not vary too.
    let zero = Face::truetype()
        .with(*b"fvar", with_field(fvar(&[], &[]), 8, 0))
        .with(*b"head", head(1_000))
        .with(*b"maxp", maxp(0))
        .build();
    assert_eq!(instantiate(&zero, &Instance::default()), Ok(zero.clone()));
}

/// A copy of a face with one table's bytes replaced in place.
fn with_table_bytes(face: &[u8], tag: &[u8; 4], edit: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut out = face.to_vec();
    let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
    for i in 0..count {
        let at = 12 + i * 16;
        if &face[at..at + 4] == tag {
            let offset =
                u32::from_be_bytes(face[at + 8..at + 12].try_into().expect("four")) as usize;
            let length =
                u32::from_be_bytes(face[at + 12..at + 16].try_into().expect("four")) as usize;
            edit(&mut out[offset..offset + length]);
            return out;
        }
    }
    panic!("no {} table", String::from_utf8_lossy(tag));
}

#[test]
fn an_instance_the_face_cannot_give_is_refused_by_name() {
    assert_eq!(
        instantiate(b"not a font at all", &Instance::default()),
        Err(InstanceError::NotAFont)
    );
    assert_eq!(
        instantiate(GLYF_FACE, &Instance::Named(7)),
        Err(InstanceError::Variation(VariationError::NoSuchInstance {
            index: 7,
            count: 7
        }))
    );
    // A gvar of a version its chapter does not define.
    let bad_gvar = with_table_bytes(GLYF_FACE, b"gvar", |table| table[1] = 2);
    assert_eq!(
        instantiate(&bad_gvar, &Instance::Named(2)),
        Err(InstanceError::Variation(VariationError::UnknownVersion {
            table: *b"gvar",
            major: 2,
            minor: 0
        }))
    );
    // A glyph whose deltas end before its points do.
    let square = vec![
        (0, 0, true),
        (0, 500, true),
        (500, 500, true),
        (500, 0, true),
    ];
    let (glyf_table, loca) = glyf_and_loca(&[
        Vec::new(),
        variable_support::simple_glyph(&[square.clone()]),
    ]);
    let short = GlyphVariation {
        shared_points: None,
        tuples: vec![TupleSpec::new(&[16_384], Some(Points::All), &[5, 5])],
    };
    let cut = one_axis_face(vec![
        (*b"glyf", glyf_table.clone()),
        (*b"loca", loca.clone()),
        (*b"maxp", maxp(2)),
        (
            *b"gvar",
            variable_support::gvar(1, &[], &[None, Some(short)], true),
        ),
    ]);
    assert_eq!(
        instantiate(&cut, &Instance::Named(0)),
        Err(InstanceError::Glyph {
            glyph: 1,
            error: VariationError::DeltasPastEnd
        })
    );
    // A point the instance would move past sixteen bits.
    let (far_glyf, far_loca) = glyf_and_loca(&[
        Vec::new(),
        variable_support::simple_glyph(&[vec![
            (0, 0, true),
            (32_000, 0, true),
            (32_000, 100, true),
        ]]),
    ]);
    let push = GlyphVariation {
        shared_points: None,
        tuples: vec![TupleSpec::new(
            &[16_384],
            Some(Points::Listed(vec![1])),
            &[1_000, 0],
        )],
    };
    let far = one_axis_face(vec![
        (*b"glyf", far_glyf),
        (*b"loca", far_loca),
        (*b"maxp", maxp(2)),
        (
            *b"gvar",
            variable_support::gvar(1, &[], &[None, Some(push)], true),
        ),
    ]);
    assert_eq!(
        instantiate(&far, &Instance::Named(0)),
        Err(InstanceError::CoordinateRange { glyph: 1 })
    );
    assert!(
        instantiate(&far, &Instance::default()).is_ok(),
        "at the default nothing moves past the range"
    );
    // A face with no head, and one with no maxp.
    let headless = Face::truetype()
        .with(
            *b"fvar",
            fvar(&[AxisSpec::new(*b"wght", 100.0, 400.0, 900.0, 256)], &[]),
        )
        .with(*b"maxp", maxp(0))
        .build();
    assert_eq!(
        instantiate(&headless, &Instance::default()),
        Err(InstanceError::MissingTable { table: *b"head" })
    );
    let countless = Face::truetype()
        .with(
            *b"fvar",
            fvar(&[AxisSpec::new(*b"wght", 100.0, 400.0, 900.0, 256)], &[]),
        )
        .with(*b"head", head(1_000))
        .build();
    assert_eq!(
        instantiate(&countless, &Instance::default()),
        Err(InstanceError::MissingTable { table: *b"maxp" })
    );
}

#[test]
fn loca_takes_the_long_form_when_the_instances_glyf_needs_it() {
    // Six hundred glyphs of sixty points, every step between them too long
    // for a byte on both axes, so even compactly written the glyf passes
    // 128 KiB.
    let zigzag: Vec<GlyphPoint> = (0..60)
        .map(|i| (1_000 * (i % 2), 1_000 * ((i + 1) % 2), true))
        .collect();
    let mut glyphs = vec![Vec::new()];
    glyphs.extend((0..599).map(|_| variable_support::simple_glyph(&[zigzag.clone()])));
    let (glyf_table, loca) = glyf_and_loca(&glyphs);
    let count = u16::try_from(glyphs.len()).expect("few");
    let face = one_axis_face(vec![
        (*b"glyf", glyf_table),
        (*b"loca", loca),
        (*b"maxp", maxp(count)),
        (
            *b"gvar",
            variable_support::gvar(1, &[], &vec![None; glyphs.len()], true),
        ),
    ]);
    let original = Sfnt::parse(&face).expect("a face");
    let bytes = instantiate(&face, &Instance::Named(0)).expect("an instance");
    let static_face = Sfnt::parse(&bytes).expect("the instance parses");
    let head = static_face
        .table(u32::from_be_bytes(*b"head"))
        .expect("head");
    assert_eq!(i16::from_be_bytes([head[50], head[51]]), 1, "the long loca");
    assert!(
        static_face
            .table(u32::from_be_bytes(*b"glyf"))
            .expect("glyf")
            .len()
            > 0x1_FFFE,
        "a glyf the short form cannot reach"
    );
    for glyph in [1u16, 300, 599] {
        assert_eq!(
            glyf::outline(&static_face, glyph).map(|o| o.segments),
            glyf::outline(&original, glyph).map(|o| o.segments),
            "glyph {glyph} re-reads through the long loca"
        );
    }
}

/// A simple glyph's data with instructions and `OVERLAP_SIMPLE` written into
/// it: the program after the contour ends, and the flag on the first point.
fn with_program(glyph: Vec<u8>, contours: usize, program: &[u8]) -> Vec<u8> {
    let at = 10 + contours * 2;
    let mut out = glyph[..at].to_vec();
    out.extend_from_slice(&u16::try_from(program.len()).expect("short").to_be_bytes());
    out.extend_from_slice(program);
    let flags = out.len();
    out.extend_from_slice(&glyph[at + 2..]);
    out[flags] |= 0x40;
    out
}

/// A composite's instructions, read off its records by their flags.
fn composite_program(face: &Sfnt, glyph: u16) -> Vec<u8> {
    let head = face.table(u32::from_be_bytes(*b"head")).expect("head");
    let loca = face.table(u32::from_be_bytes(*b"loca")).expect("loca");
    let glyf_table = face.table(u32::from_be_bytes(*b"glyf")).expect("glyf");
    let long = i16::from_be_bytes([head[50], head[51]]) != 0;
    let g = usize::from(glyph);
    let start = if long {
        u32::from_be_bytes(loca[g * 4..g * 4 + 4].try_into().expect("four")) as usize
    } else {
        usize::from(u16::from_be_bytes([loca[g * 2], loca[g * 2 + 1]])) * 2
    };
    let data = &glyf_table[start..];
    let mut at = 10;
    let mut instructed = false;
    loop {
        let flags = u16::from_be_bytes([data[at], data[at + 1]]);
        at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
        at += if flags & 0x0008 != 0 {
            2
        } else if flags & 0x0040 != 0 {
            4
        } else if flags & 0x0080 != 0 {
            8
        } else {
            0
        };
        instructed |= flags & 0x0100 != 0;
        if flags & 0x0020 == 0 {
            break;
        }
    }
    if !instructed {
        return Vec::new();
    }
    let length = usize::from(u16::from_be_bytes([data[at], data[at + 1]]));
    data[at + 2..at + 2 + length].to_vec()
}

#[test]
fn a_glyphs_program_flags_and_components_survive_the_instance() {
    let triangle = vec![(10, 10, true), (60, 200, false), (110, 10, true)];
    let program = [0xB0, 0x01, 0x21];
    let instructed = with_program(
        variable_support::simple_glyph(&[triangle.clone()]),
        1,
        &program,
    );
    let target =
        variable_support::simple_glyph(&[vec![(0, 0, true), (0, 50, true), (50, 50, true)]]);
    let mut turned = ComponentSpec::at(1, 10, 20);
    turned.flags |= 0x0080; // WE_HAVE_A_TWO_BY_TWO
    turned.transform = vec![16_384, 4_096, -4_096, 16_384];
    let matched = ComponentSpec {
        flags: 0x0000, // byte point numbers
        glyph: 2,
        arguments: (1, 0),
        transform: Vec::new(),
    };
    let mut composite = composite_glyph([0, 0, 0, 0], &[turned.clone(), matched.clone()]);
    // WE_HAVE_INSTRUCTIONS on the last record, and its program after it.
    let last_flags = 10 + 4 + 4 + 8;
    composite[last_flags] |= 0x01;
    composite.extend_from_slice(&2u16.to_be_bytes());
    composite.extend_from_slice(&[0xB0, 0x02]);
    let small = ComponentSpec {
        flags: 0x0002, // byte offsets
        glyph: 1,
        arguments: (100, -100),
        transform: Vec::new(),
    };
    let byte_offsets = composite_glyph([0, 0, 0, 0], &[small]);
    let (glyf_table, loca) =
        glyf_and_loca(&[Vec::new(), instructed, target, composite, byte_offsets]);
    let variations = vec![
        None,
        Some(GlyphVariation {
            shared_points: None,
            tuples: vec![TupleSpec::new(
                &[16_384],
                Some(Points::All),
                &[3, 3, 3, 0, 0, 0, 0, 2, 2, 2, 0, 0, 0, 0],
            )],
        }),
        None,
        Some(GlyphVariation {
            shared_points: None,
            tuples: vec![TupleSpec::new(
                &[16_384],
                Some(Points::Listed(vec![0, 1])),
                &[5, 400, -5, 400],
            )],
        }),
        Some(GlyphVariation {
            shared_points: None,
            tuples: vec![TupleSpec::new(
                &[16_384],
                Some(Points::Listed(vec![0])),
                &[50, -50],
            )],
        }),
    ];
    let face = one_axis_face(vec![
        (*b"glyf", glyf_table),
        (*b"loca", loca),
        (*b"maxp", maxp(5)),
        (*b"gvar", variable_support::gvar(1, &[], &variations, true)),
    ]);
    let original = Sfnt::parse(&face).expect("a face");
    let space = DesignSpace::read(&original).expect("a design space");
    let gvar = Gvar::read(&original, &space).expect("gvar").expect("gvar");
    let location = space.location(&Instance::Named(0)).expect("a location");
    let bytes = instantiate(&face, &Instance::Named(0)).expect("an instance");
    let static_face = Sfnt::parse(&bytes).expect("the instance parses");

    // The simple glyph keeps its program and OVERLAP_SIMPLE.
    let raw = glyf::simple_glyph(&static_face, 1).expect("a simple glyph");
    assert_eq!(raw.instructions, &program, "the glyph's program");
    assert_eq!(
        raw.flags[0] & 0x40,
        0x40,
        "OVERLAP_SIMPLE on the first point"
    );
    assert_eq!(
        raw.flags.iter().map(|f| f & 1).collect::<Vec<_>>(),
        vec![1, 0, 1]
    );
    // The composite keeps its transform, its matched record's point numbers
    // — whose delta is ignored — and its program.
    let records = glyf::components(&static_face, 3).expect("a composite");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].transform, [16_384, 4_096, -4_096, 16_384]);
    assert_eq!(
        records[0].arguments,
        [15, 15],
        "the offset moved by its delta"
    );
    assert!(records[0].flags & 0x0080 != 0);
    assert!(!records[1].offset_is_xy());
    assert_eq!(
        records[1].arguments,
        [1, 0],
        "matched points are kept as they are"
    );
    assert_eq!(
        composite_program(&static_face, 3),
        vec![0xB0, 0x02],
        "the composite's program"
    );
    // Its box: the turned triangle at its offset, and the matched glyph
    // moved so that its point 0 lands on the composite's point 1.
    let varied: Vec<Varied> = (0..5)
        .map(|g| varied_at(&original, &gvar, &location, g))
        .collect();
    let header = glyf::bounds(&static_face, 3).expect("a header");
    assert_eq!(
        Some([
            i64::from(header.x_min),
            i64::from(header.y_min),
            i64::from(header.x_max),
            i64::from(header.y_max)
        ]),
        varied_box(&varied, 3),
        "the composite's box measured by the matched placement"
    );
    // An offset written in bytes that the instance moves past a byte is
    // written in words.
    let wide = glyf::components(&static_face, 4).expect("a composite");
    assert_eq!(wide[0].arguments, [150, -150]);
    assert!(wide[0].flags & 0x0001 != 0, "ARG_1_AND_2_ARE_WORDS");
    // Every glyph re-reads as the variable face draws it, rounded.
    for glyph in 0..5u16 {
        let mut expected = Vec::new();
        varied_outline(&varied, usize::from(glyph), UNPLACED, 0, &mut expected);
        assert_eq!(
            glyf::outline(&static_face, glyph)
                .expect("an outline")
                .segments,
            expected,
            "glyph {glyph}"
        );
    }
    // A small instance takes the short loca though its face had the long.
    let head = static_face
        .table(u32::from_be_bytes(*b"head"))
        .expect("head");
    assert_eq!(i16::from_be_bytes([head[50], head[51]]), 0);
}

#[test]
fn tables_the_instancer_does_not_know_are_copied_and_its_drops_are_gone() {
    let mut face = Face {
        version: 0x0001_0000,
        tables: Vec::new(),
    };
    let original = Sfnt::parse(GLYF_FACE).expect("a face");
    for tag in original.tags() {
        face = face.with(
            tag,
            original
                .table(u32::from_be_bytes(tag))
                .expect("a table")
                .to_vec(),
        );
    }
    let unknown: Vec<u8> = (0u8..=37).collect();
    let layout: Vec<u8> = vec![0, 1, 0, 0, 0, 10, 0, 12, 0, 14];
    let bytes = face
        .with(*b"ZZZZ", unknown.clone())
        .with(*b"GSUB", layout.clone())
        .with(*b"DSIG", vec![0, 0, 0, 1, 0, 0, 0, 0])
        .with(*b"hdmx", vec![0; 8])
        .with(*b"VDMX", vec![0; 6])
        .build();
    let instance = instantiate(&bytes, &Instance::Named(3)).expect("an instance");
    let static_face = Sfnt::parse(&instance).expect("the instance parses");
    assert_eq!(
        static_face.table(u32::from_be_bytes(*b"ZZZZ")),
        Some(unknown.as_slice())
    );
    assert_eq!(
        static_face.table(u32::from_be_bytes(*b"GSUB")),
        Some(layout.as_slice())
    );
    for tag in [b"DSIG", b"hdmx", b"VDMX", b"STAT", b"fvar"] {
        assert!(
            static_face.table(u32::from_be_bytes(*tag)).is_none(),
            "{} is dropped",
            String::from_utf8_lossy(tag)
        );
    }
}

#[test]
fn every_truncation_and_byte_change_of_the_face_is_instanced_or_refused() {
    let instances = [
        Instance::Named(5),
        Instance::User(vec![
            UserCoordinate::new(*b"wght", 650.0),
            UserCoordinate::new(*b"wdth", 80.0),
        ]),
    ];
    for instance in &instances {
        for end in 0..GLYF_FACE.len() {
            if let Ok(bytes) = instantiate(&GLYF_FACE[..end], instance) {
                assert!(Sfnt::parse(&bytes).is_some(), "a prefix of {end} bytes");
            }
        }
    }
    for at in 0..GLYF_FACE.len() {
        let mut changed = GLYF_FACE.to_vec();
        changed[at] ^= 0xFF;
        if let Ok(bytes) = instantiate(&changed, &instances[0]) {
            assert!(Sfnt::parse(&bytes).is_some(), "byte {at} changed");
        }
    }
}
