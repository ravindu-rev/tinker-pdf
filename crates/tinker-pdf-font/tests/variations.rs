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

use tinker_pdf_font::variation::{
    AxisValueMap, DesignSpace, Fixed, Instance, Location, UserCoordinate, VariationError, MAX_AXES,
};
use tinker_pdf_font::Sfnt;
use variable_support::{
    avar, f2dot14, fvar, fvar_with_sizes, identity_map, spec, AxisSpec, Face, InstanceSpec,
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
