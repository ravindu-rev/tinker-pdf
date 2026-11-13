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
use tinker_pdf_font::variation::cvar::{cvt_at, Cvar};
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
    avar, cff2_index, charstring, cvar, cvt, dict_entry, f2dot14, fvar, fvar_with_sizes, gasp,
    hhea, hvar, identity_map, index_map, ints, item_store, metrics, mvar, op, os2, pack_deltas,
    pack_points, post, spec, tuple_store, vhea, vorg, vvar, AxisSpec, Cff2Spec, Cs, DataSpec, Face,
    FdSelectSpec, InstanceSpec, Points, PrivateSpec, RegionSpec, TupleSpec,
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
