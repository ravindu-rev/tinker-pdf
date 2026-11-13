//! Variable faces built byte by byte, and the OpenType specification's
//! arithmetic written out a second time, so that a test reads its expected
//! value from the same statement the face was built from.
//!
//! Every table here is assembled from its chapter's field list in OpenType
//! 1.9.1 — `fvar`, `avar`, the tuple and item variation stores — and nothing
//! is borrowed from a face another producer wrote. The second half, [`spec`],
//! is the overview chapter's normalisation and interpolation transcribed
//! independently of `tinker_pdf_font::variation`: exact rationals, rounded
//! where the design's numeric model says a value is rounded and nowhere else
//! (`docs/design/variable-fonts.md`, 'The numeric model').
//!
//! # Why the allow
//!
//! Each test binary that names this module compiles its own copy, and each
//! uses the builders its tables need, so whichever does not use a builder
//! reports it dead.

#![allow(
    dead_code,
    reason = "shared by the variation test binaries; each uses a different subset"
)]

/// A four-byte tag.
pub type Tag = [u8; 4];

/// Appends a big-endian `u16`.
pub fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `i16`.
pub fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `u32`.
pub fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Appends a big-endian `i32`.
pub fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// A user value as the 32-bit `Fixed` a table stores, for the values a test
/// writes — each a multiple of 1/65 536, so the conversion is exact.
pub fn fixed(value: f64) -> i32 {
    let scaled = value * 65_536.0;
    assert_eq!(scaled, scaled.trunc(), "{value} is not a 16.16 value");
    scaled as i32
}

/// A normalised value as 2.14, for the values a test writes — each a
/// multiple of 1/16 384.
pub fn f2dot14(value: f64) -> i16 {
    let scaled = value * 16_384.0;
    assert_eq!(scaled, scaled.trunc(), "{value} is not a 2.14 value");
    scaled as i16
}

/// A face: tables by tag, written as an sfnt whose directory is sorted, whose
/// tables are four-byte aligned and whose checksums are the specification's.
#[derive(Clone, Debug)]
pub struct Face {
    /// `0x00010000` for TrueType outlines, `OTTO` for CFF and CFF2.
    pub version: u32,
    /// The tables, in any order.
    pub tables: Vec<(Tag, Vec<u8>)>,
}

impl Face {
    /// A face with TrueType outlines and no tables yet.
    pub fn truetype() -> Face {
        Face {
            version: 0x0001_0000,
            tables: Vec::new(),
        }
    }

    /// A face with CFF-family outlines and no tables yet.
    pub fn opentype() -> Face {
        Face {
            version: u32::from_be_bytes(*b"OTTO"),
            tables: Vec::new(),
        }
    }

    /// This face with one more table, replacing one of the same tag.
    pub fn with(mut self, tag: Tag, bytes: Vec<u8>) -> Face {
        self.tables.retain(|(t, _)| *t != tag);
        self.tables.push((tag, bytes));
        self
    }

    /// This face without a table.
    pub fn without(mut self, tag: Tag) -> Face {
        self.tables.retain(|(t, _)| *t != tag);
        self
    }

    /// The face's bytes.
    pub fn build(&self) -> Vec<u8> {
        let mut tables = self.tables.clone();
        tables.sort_by(|a, b| a.0.cmp(&b.0));
        let count = u16::try_from(tables.len()).expect("a test face has few tables");
        let mut power = 1u16;
        let mut selector = 0u16;
        while power * 2 <= count {
            power *= 2;
            selector += 1;
        }
        let mut out = Vec::new();
        push32(&mut out, self.version);
        push16(&mut out, count);
        push16(&mut out, power * 16);
        push16(&mut out, selector);
        push16(&mut out, count * 16 - power * 16);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, bytes) in &tables {
            out.extend_from_slice(tag);
            push32(&mut out, checksum(bytes));
            push32(&mut out, u32::try_from(offset).expect("small"));
            push32(&mut out, u32::try_from(bytes.len()).expect("small"));
            body.extend_from_slice(bytes);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            offset = 12 + tables.len() * 16 + body.len();
        }
        out.extend_from_slice(&body);
        out
    }
}

/// The sum of a table's big-endian words, the last padded with zeros.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

// ---- fvar and avar ---------------------------------------------------------

/// One axis as a test states it, in user units.
#[derive(Clone, Copy, Debug)]
pub struct AxisSpec {
    pub tag: Tag,
    pub min: f64,
    pub default: f64,
    pub max: f64,
    pub flags: u16,
    pub name_id: u16,
}

impl AxisSpec {
    /// An axis with no flags.
    pub fn new(tag: Tag, min: f64, default: f64, max: f64, name_id: u16) -> AxisSpec {
        AxisSpec {
            tag,
            min,
            default,
            max,
            flags: 0,
            name_id,
        }
    }
}

/// One named instance as a test states it.
#[derive(Clone, Debug)]
pub struct InstanceSpec {
    pub subfamily_name_id: u16,
    pub coordinates: Vec<f64>,
    pub postscript_name_id: Option<u16>,
}

impl InstanceSpec {
    /// An instance with a PostScript name ID.
    pub fn new(subfamily_name_id: u16, coordinates: &[f64], postscript: u16) -> InstanceSpec {
        InstanceSpec {
            subfamily_name_id,
            coordinates: coordinates.to_vec(),
            postscript_name_id: Some(postscript),
        }
    }
}

/// An `fvar` table (`fvar`, 'Table Formats'): the header, the axis records
/// twenty bytes apart, and the instance records after them, carrying a
/// PostScript name ID when the first instance has one.
pub fn fvar(axes: &[AxisSpec], instances: &[InstanceSpec]) -> Vec<u8> {
    let with_postscript = instances
        .first()
        .is_some_and(|i| i.postscript_name_id.is_some());
    let instance_size = axes.len() * 4 + if with_postscript { 6 } else { 4 };
    fvar_with_sizes(axes, instances, 20, instance_size)
}

/// An `fvar` table with the record sizes stated, so that a test can write
/// the extensions and the refusals the sizes allow. Records shorter than
/// their content are written whole all the same, and records longer are
/// padded with zeros.
pub fn fvar_with_sizes(
    axes: &[AxisSpec],
    instances: &[InstanceSpec],
    axis_size: usize,
    instance_size: usize,
) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // majorVersion
    push16(&mut out, 0); // minorVersion
    push16(&mut out, 16); // axesArrayOffset
    push16(&mut out, 2); // reserved
    push16(&mut out, u16::try_from(axes.len()).expect("few axes"));
    push16(&mut out, u16::try_from(axis_size).expect("small"));
    push16(
        &mut out,
        u16::try_from(instances.len()).expect("few instances"),
    );
    push16(&mut out, u16::try_from(instance_size).expect("small"));
    for axis in axes {
        let start = out.len();
        out.extend_from_slice(&axis.tag);
        push_i32(&mut out, fixed(axis.min));
        push_i32(&mut out, fixed(axis.default));
        push_i32(&mut out, fixed(axis.max));
        push16(&mut out, axis.flags);
        push16(&mut out, axis.name_id);
        while out.len() < start + axis_size {
            out.push(0);
        }
    }
    for instance in instances {
        let start = out.len();
        push16(&mut out, instance.subfamily_name_id);
        push16(&mut out, 0); // flags
        for value in &instance.coordinates {
            push_i32(&mut out, fixed(*value));
        }
        if let Some(id) = instance.postscript_name_id {
            push16(&mut out, id);
        }
        while out.len() < start + instance_size {
            out.push(0);
        }
    }
    out
}

/// An `avar` table (`avar`, 'Table formats'): one segment map an axis, each a
/// list of `(fromCoordinate, toCoordinate)` in 2.14.
pub fn avar(maps: &[Vec<(i16, i16)>]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // majorVersion
    push16(&mut out, 0); // minorVersion
    push16(&mut out, 0); // reserved
    push16(&mut out, u16::try_from(maps.len()).expect("few axes"));
    for map in maps {
        push16(&mut out, u16::try_from(map.len()).expect("few maps"));
        for (from, to) in map {
            push_i16(&mut out, *from);
            push_i16(&mut out, *to);
        }
    }
    out
}

/// The three correspondences every modifying segment map holds.
pub fn identity_map() -> Vec<(i16, i16)> {
    vec![(-16_384, -16_384), (0, 0), (16_384, 16_384)]
}

// ---- The tuple variation store and cvar ------------------------------------

/// Point numbers as a test states them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Points {
    /// Every item: the count byte zero.
    All,
    /// The items listed, in increasing order.
    Listed(Vec<u16>),
}

/// Packs point numbers (common formats, 'Packed "Point" Numbers'): the count
/// in one byte below 128 and in two, the high bit set, from 128 — and an
/// empty list in two, since one zero byte would mean every point — then runs
/// of at most 128 differences, each run of bytes while the differences fit a
/// byte and of words while they do not.
pub fn pack_points(points: &Points) -> Vec<u8> {
    let numbers = match points {
        Points::All => return vec![0],
        Points::Listed(numbers) => numbers,
    };
    let mut out = Vec::new();
    let count = numbers.len();
    if count == 0 || count >= 128 {
        push16(
            &mut out,
            0x8000 | u16::try_from(count).expect("a 15-bit count"),
        );
    } else {
        out.push(u8::try_from(count).expect("small"));
    }
    let mut last = 0u16;
    let differences: Vec<u16> = numbers
        .iter()
        .map(|n| {
            let difference = n - last;
            last = *n;
            difference
        })
        .collect();
    let mut at = 0;
    while at < differences.len() {
        let words = differences[at] > 255;
        let mut end = at;
        while end < differences.len() && end - at < 128 && (differences[end] > 255) == words {
            end += 1;
        }
        let length = u8::try_from(end - at - 1).expect("at most 127");
        out.push(length | if words { 0x80 } else { 0 });
        for difference in &differences[at..end] {
            if words {
                push16(&mut out, *difference);
            } else {
                out.push(u8::try_from(*difference).expect("a byte"));
            }
        }
        at = end;
    }
    out
}

/// Packs deltas (common formats, 'Packed Deltas'): runs of at most 64, each
/// of zeros, of bytes while the values fit a signed byte, or of words.
pub fn pack_deltas(deltas: &[i32]) -> Vec<u8> {
    let kind = |value: i32| {
        if value == 0 {
            0
        } else if (-128..=127).contains(&value) {
            1
        } else {
            2
        }
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at < deltas.len() {
        let run_kind = kind(deltas[at]);
        let mut end = at;
        while end < deltas.len() && end - at < 64 && kind(deltas[end]) == run_kind {
            end += 1;
        }
        let length = u8::try_from(end - at - 1).expect("at most 63");
        match run_kind {
            0 => out.push(0x80 | length),
            1 => {
                out.push(length);
                for value in &deltas[at..end] {
                    out.push(i8::try_from(*value).expect("a byte") as u8);
                }
            }
            _ => {
                out.push(0x40 | length);
                for value in &deltas[at..end] {
                    push_i16(&mut out, i16::try_from(*value).expect("a word"));
                }
            }
        }
        at = end;
    }
    out
}

/// One tuple variation table as a test states it.
#[derive(Clone, Debug)]
pub struct TupleSpec {
    /// The peak, one 2.14 coordinate an axis.
    pub peak: Vec<i16>,
    /// An intermediate region's start and end.
    pub intermediate: Option<(Vec<i16>, Vec<i16>)>,
    /// A shared tuple's index to take the peak from instead of embedding it.
    pub shared_peak: Option<u16>,
    /// Private point numbers; `None` takes the store's shared ones.
    pub points: Option<Points>,
    /// The deltas, one an item listed — in `gvar`, the X deltas then the Y.
    pub deltas: Vec<i32>,
}

impl TupleSpec {
    /// A table with an embedded peak and no intermediate region.
    pub fn new(peak: &[i16], points: Option<Points>, deltas: &[i32]) -> TupleSpec {
        TupleSpec {
            peak: peak.to_vec(),
            intermediate: None,
            shared_peak: None,
            points,
            deltas: deltas.to_vec(),
        }
    }

    /// This table with an intermediate region.
    pub fn between(mut self, start: &[i16], end: &[i16]) -> TupleSpec {
        self.intermediate = Some((start.to_vec(), end.to_vec()));
        self
    }

    /// The region this table states, an axis at a time.
    pub fn region(&self) -> Vec<(i16, i16, i16)> {
        match &self.intermediate {
            Some((start, end)) => self
                .peak
                .iter()
                .zip(start.iter().zip(end))
                .map(|(peak, (start, end))| (*start, *peak, *end))
                .collect(),
            None => self.peak.iter().map(|p| spec::implied(*p)).collect(),
        }
    }
}

/// A tuple variation store (common formats, 'Tuple Variation Store Header',
/// 'TupleVariationHeader', 'Serialized Data'): the packed count, the data
/// offset — measured from `base` bytes before the store, `cvar`'s version —
/// the headers, the shared point numbers and each table's data.
pub fn tuple_store(base: usize, shared_points: Option<&Points>, tuples: &[TupleSpec]) -> Vec<u8> {
    let mut headers = Vec::new();
    let mut data = Vec::new();
    if let Some(points) = shared_points {
        data.extend(pack_points(points));
    }
    for tuple in tuples {
        let mut serialized = Vec::new();
        if let Some(points) = &tuple.points {
            serialized.extend(pack_points(points));
        }
        serialized.extend(pack_deltas(&tuple.deltas));
        let mut index = match tuple.shared_peak {
            Some(shared) => shared & 0x0FFF,
            None => 0x8000,
        };
        if tuple.intermediate.is_some() {
            index |= 0x4000;
        }
        if tuple.points.is_some() {
            index |= 0x2000;
        }
        push16(
            &mut headers,
            u16::try_from(serialized.len()).expect("small"),
        );
        push16(&mut headers, index);
        if tuple.shared_peak.is_none() {
            for coordinate in &tuple.peak {
                push_i16(&mut headers, *coordinate);
            }
        }
        if let Some((start, end)) = &tuple.intermediate {
            for coordinate in start.iter().chain(end) {
                push_i16(&mut headers, *coordinate);
            }
        }
        data.extend(serialized);
    }
    let mut out = Vec::new();
    let count = u16::try_from(tuples.len()).expect("few");
    push16(
        &mut out,
        count | if shared_points.is_some() { 0x8000 } else { 0 },
    );
    push16(
        &mut out,
        u16::try_from(base + 4 + headers.len()).expect("small"),
    );
    out.extend(headers);
    out.extend(data);
    out
}

/// A `cvar` table: version 1.0 and one tuple variation store.
pub fn cvar(shared_points: Option<&Points>, tuples: &[TupleSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    out.extend(tuple_store(4, shared_points, tuples));
    out
}

/// A `cvt ` table of FWORDs.
pub fn cvt(values: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        push_i16(&mut out, *value);
    }
    out
}

// ---- The item variation store and the metrics tables -----------------------

/// One region as a test states it: `(start, peak, end)` an axis, in 2.14.
pub type RegionSpec = Vec<(i16, i16, i16)>;

/// One item variation data subtable as a test states it.
#[derive(Clone, Debug)]
pub struct DataSpec {
    /// The regions its columns name.
    pub regions: Vec<u16>,
    /// How many columns, from the first, use the long type.
    pub word_count: u16,
    /// Whether the long type is 32 bits and the short 16 (`LONG_WORDS`).
    pub long_words: bool,
    /// The rows, a delta a column.
    pub rows: Vec<Vec<i32>>,
}

/// An item variation store (common formats, 'Item Variation Store Header and
/// Item Variation Data Subtables'): format 1, the offsets, the region list,
/// then the subtables — `None` writing a NULL offset.
pub fn item_store(axis_count: u16, regions: &[RegionSpec], data: &[Option<DataSpec>]) -> Vec<u8> {
    let mut list = Vec::new();
    push16(&mut list, axis_count);
    push16(
        &mut list,
        u16::try_from(regions.len()).expect("few regions"),
    );
    for region in regions {
        assert_eq!(region.len(), usize::from(axis_count), "an entry an axis");
        for (start, peak, end) in region {
            push_i16(&mut list, *start);
            push_i16(&mut list, *peak);
            push_i16(&mut list, *end);
        }
    }
    let header = 8 + 4 * data.len();
    let mut out = Vec::new();
    push16(&mut out, 1);
    push32(&mut out, u32::try_from(header).expect("small"));
    push16(&mut out, u16::try_from(data.len()).expect("few"));
    let mut subtables = Vec::new();
    let mut at = header + list.len();
    for spec in data {
        let Some(spec) = spec else {
            push32(&mut out, 0);
            continue;
        };
        push32(&mut out, u32::try_from(at).expect("small"));
        let mut table = Vec::new();
        push16(
            &mut table,
            u16::try_from(spec.rows.len()).expect("few rows"),
        );
        push16(
            &mut table,
            spec.word_count | if spec.long_words { 0x8000 } else { 0 },
        );
        push16(&mut table, u16::try_from(spec.regions.len()).expect("few"));
        for region in &spec.regions {
            push16(&mut table, *region);
        }
        for row in &spec.rows {
            assert_eq!(row.len(), spec.regions.len(), "a delta a column");
            for (column, delta) in row.iter().enumerate() {
                let long = column < usize::from(spec.word_count);
                match (spec.long_words, long) {
                    (true, true) => push_i32(&mut table, *delta),
                    (true, false) | (false, true) => {
                        push_i16(&mut table, i16::try_from(*delta).expect("a word"));
                    }
                    (false, false) => {
                        table.push(i8::try_from(*delta).expect("a byte") as u8);
                    }
                }
            }
        }
        at += table.len();
        subtables.extend(table);
    }
    out.extend(list);
    out.extend(subtables);
    out
}

/// A delta-set index map (common formats, 'Associating Target Items to
/// Variation Data'): its format, entries of `entry_size` bytes with
/// `inner_bits` bits of inner index, each `(outer, inner)`.
pub fn index_map(format: u8, entry_size: u8, inner_bits: u8, entries: &[(u32, u32)]) -> Vec<u8> {
    assert!((1..=4).contains(&entry_size) && (1..=16).contains(&inner_bits));
    let mut out = vec![format, ((entry_size - 1) << 4) | (inner_bits - 1)];
    let count = u32::try_from(entries.len()).expect("few");
    if format == 0 {
        push16(&mut out, u16::try_from(count).expect("sixteen bits"));
    } else {
        push32(&mut out, count);
    }
    for (outer, inner) in entries {
        assert!(*inner < (1 << inner_bits), "the inner index fits");
        let entry = (u64::from(*outer) << inner_bits) | u64::from(*inner);
        assert!(
            entry < (1u64 << (u32::from(entry_size) * 8)),
            "the entry fits"
        );
        for byte in (0..entry_size).rev() {
            out.push((entry >> (u32::from(byte) * 8)) as u8);
        }
    }
    out
}

/// A metrics table (`HVAR` or `VVAR`): version 1.0, the store's offset, then
/// one offset a map, NULL where the map is `None`; the store and maps after.
pub fn metrics_variations(store: &[u8], maps: &[Option<Vec<u8>>]) -> Vec<u8> {
    let header = 8 + 4 * maps.len();
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push32(&mut out, u32::try_from(header).expect("small"));
    let mut body = store.to_vec();
    let mut at = header + store.len();
    for map in maps {
        match map {
            Some(map) => {
                push32(&mut out, u32::try_from(at).expect("small"));
                at += map.len();
                body.extend_from_slice(map);
            }
            None => push32(&mut out, 0),
        }
    }
    out.extend(body);
    out
}

/// An `HVAR` table: the store, and the advance, left and right side-bearing
/// maps.
pub fn hvar(
    store: &[u8],
    advance: Option<Vec<u8>>,
    left: Option<Vec<u8>>,
    right: Option<Vec<u8>>,
) -> Vec<u8> {
    metrics_variations(store, &[advance, left, right])
}

/// A `VVAR` table: the store, and the advance, top, bottom and origin maps.
pub fn vvar(
    store: &[u8],
    advance: Option<Vec<u8>>,
    top: Option<Vec<u8>>,
    bottom: Option<Vec<u8>>,
    origin: Option<Vec<u8>>,
) -> Vec<u8> {
    metrics_variations(store, &[advance, top, bottom, origin])
}

/// An `MVAR` table: version 1.0, eight-byte value records in the order
/// given, and the store after them.
pub fn mvar(store: &[u8], records: &[(Tag, u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push16(&mut out, 0); // reserved
    push16(&mut out, 8); // valueRecordSize
    push16(&mut out, u16::try_from(records.len()).expect("few"));
    let store_at = if records.is_empty() {
        0
    } else {
        12 + 8 * records.len()
    };
    push16(&mut out, u16::try_from(store_at).expect("small"));
    for (tag, outer, inner) in records {
        out.extend_from_slice(tag);
        push16(&mut out, *outer);
        push16(&mut out, *inner);
    }
    if !records.is_empty() {
        out.extend_from_slice(store);
    }
    out
}

/// An `hhea` table, its caret slope and offset stated and `numberOfHMetrics`.
pub fn hhea(long_metrics: u16, caret: (i16, i16, i16)) -> Vec<u8> {
    let mut out = vec![0u8; 36];
    out[0..2].copy_from_slice(&1u16.to_be_bytes());
    out[18..20].copy_from_slice(&caret.0.to_be_bytes());
    out[20..22].copy_from_slice(&caret.1.to_be_bytes());
    out[22..24].copy_from_slice(&caret.2.to_be_bytes());
    out[34..36].copy_from_slice(&long_metrics.to_be_bytes());
    out
}

/// An `hmtx` or `vmtx` table: long metrics, then the bearings after them.
pub fn metrics(long: &[(u16, i16)], bearings: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    for (advance, bearing) in long {
        push16(&mut out, *advance);
        push_i16(&mut out, *bearing);
    }
    for bearing in bearings {
        push_i16(&mut out, *bearing);
    }
    out
}

/// A `vhea` table, version 1.1: ascent, descent and line gap, the caret
/// slope and offset, and `numOfLongVerMetrics`.
pub fn vhea(long_metrics: u16, lines: (i16, i16, i16), caret: (i16, i16, i16)) -> Vec<u8> {
    let mut out = vec![0u8; 36];
    out[0..4].copy_from_slice(&0x0001_1000u32.to_be_bytes());
    out[4..6].copy_from_slice(&lines.0.to_be_bytes());
    out[6..8].copy_from_slice(&lines.1.to_be_bytes());
    out[8..10].copy_from_slice(&lines.2.to_be_bytes());
    out[18..20].copy_from_slice(&caret.0.to_be_bytes());
    out[20..22].copy_from_slice(&caret.1.to_be_bytes());
    out[22..24].copy_from_slice(&caret.2.to_be_bytes());
    out[34..36].copy_from_slice(&long_metrics.to_be_bytes());
    out
}

/// A `VORG` table: the default origin and per-glyph ones, sorted by glyph.
pub fn vorg(default: i16, records: &[(u16, i16)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push_i16(&mut out, default);
    push16(&mut out, u16::try_from(records.len()).expect("few"));
    for (glyph, origin) in records {
        push16(&mut out, *glyph);
        push_i16(&mut out, *origin);
    }
    out
}

/// An `OS/2` table, version 4 — 96 bytes — with the fields at the offsets
/// given set to the values given, and zero elsewhere.
pub fn os2(fields: &[(usize, u16)]) -> Vec<u8> {
    let mut out = vec![0u8; 96];
    out[0..2].copy_from_slice(&4u16.to_be_bytes());
    for (at, value) in fields {
        out[*at..*at + 2].copy_from_slice(&value.to_be_bytes());
    }
    out
}

/// A `post` table, version 3.0, its underline position and thickness stated.
pub fn post(position: i16, thickness: i16) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[0..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    out[8..10].copy_from_slice(&position.to_be_bytes());
    out[10..12].copy_from_slice(&thickness.to_be_bytes());
    out
}

/// A `gasp` table, version 1: `(rangeMaxPPEM, rangeGaspBehavior)` ranges.
pub fn gasp(ranges: &[(u16, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, u16::try_from(ranges.len()).expect("few"));
    for (max, behavior) in ranges {
        push16(&mut out, *max);
        push16(&mut out, *behavior);
    }
    out
}

// ---- The specification's arithmetic, a second time -------------------------

/// The overview chapter's normalisation and interpolation, transcribed from
/// its text without reference to the crate's code: each value an exact
/// rational until the numeric model rounds it, every rounding to the nearest
/// 1/65 536 with a half rounding up.
pub mod spec {
    /// `n / d` rounded to the nearest integer, a half rounding up.
    pub fn nearest(n: i128, d: i128) -> i128 {
        assert!(d != 0, "no zero denominators in the model");
        let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
        // floor(n/d + 1/2) = floor((2n + d) / 2d), floored toward −∞.
        let numerator = 2 * n + d;
        let denominator = 2 * d;
        let quotient = numerator / denominator;
        if numerator % denominator != 0 && numerator < 0 {
            quotient - 1
        } else {
            quotient
        }
    }

    /// An `f64` as 16.16 by the overview's conversion: the fraction times
    /// 65 536 rounded half up into the low word, the integer part's two's
    /// complement above it.
    pub fn user_fixed(value: f64) -> i64 {
        let whole = value.floor();
        let fraction = value - whole;
        let low = (fraction * 65_536.0 + 0.5).floor() as i64;
        (whole as i64) * 65_536 + low
    }

    /// The default normalisation of a 16.16 user value against an axis's
    /// 16.16 minimum, default and maximum: clamped to the axis, the segment's
    /// fraction rounded to 16.16, and clamped to [−1, 1].
    pub fn default_normalised(min: i64, default: i64, max: i64, user: i64) -> i64 {
        let user = user.clamp(min, max);
        let value = if user < default {
            nearest(
                i128::from(user - default) * 65_536,
                i128::from(default - min),
            ) as i64
        } else if user > default {
            nearest(
                i128::from(user - default) * 65_536,
                i128::from(max - default),
            ) as i64
        } else {
            0
        };
        value.clamp(-65_536, 65_536)
    }

    /// A default normalised 16.16 value through one well-formed `avar`
    /// segment map of 2.14 pairs: the first map at or past the value, and an
    /// interpolation from the one before it, ratio and product each rounded.
    pub fn through_avar(map: &[(i16, i16)], value: i64) -> i64 {
        let wide = |v: i16| i64::from(v) * 4;
        let end = map
            .iter()
            .position(|(from, _)| wide(*from) >= value)
            .expect("the map ends at one");
        let (end_from, end_to) = (wide(map[end].0), wide(map[end].1));
        if end_from == value {
            return end_to.clamp(-65_536, 65_536);
        }
        let (start_from, start_to) = (wide(map[end - 1].0), wide(map[end - 1].1));
        let ratio = nearest(
            i128::from(value - start_from) * 65_536,
            i128::from(end_from - start_from),
        );
        let product = nearest(ratio * i128::from(end_to - start_to), 65_536);
        (start_to + product as i64).clamp(-65_536, 65_536)
    }

    /// The records of a segment map the `avar` chapter keeps: each whose
    /// `fromCoordinate` passes the last kept one's and whose `toCoordinate`
    /// does not fall below it.
    pub fn avar_kept(map: &[(i16, i16)]) -> Vec<(i16, i16)> {
        let mut kept: Vec<(i16, i16)> = Vec::new();
        for &(from, to) in map {
            match kept.last() {
                Some(&(last_from, last_to)) if from <= last_from || to < last_to => {}
                _ => kept.push((from, to)),
            }
        }
        kept
    }

    /// Whether a segment map modifies its axis: −1→−1, 0→0 and 1→1 among
    /// the records it keeps.
    pub fn avar_applies(map: &[(i16, i16)]) -> bool {
        let kept = avar_kept(map);
        [-16_384, 0, 16_384]
            .iter()
            .all(|v| kept.iter().any(|(from, to)| from == v && to == v))
    }

    /// A default normalised value through an `avar` segment map as the
    /// chapter reads it: the kept records, or the value unchanged where they
    /// do not modify the axis.
    pub fn avar(map: &[(i16, i16)], value: i64) -> i64 {
        if avar_applies(map) {
            through_avar(&avar_kept(map), value)
        } else {
            value.clamp(-65_536, 65_536)
        }
    }

    /// The overview's 2.14 conversion: add 2, shift right by 2 (toward −∞).
    pub fn to_f2dot14(value: i64) -> i16 {
        let shifted = (value + 2).div_euclid(4);
        i16::try_from(shifted).expect("a normalised value")
    }

    /// One axis's scalar at a 2.14 coordinate, as a 16.16 value, from the
    /// overview's pseudo-code.
    pub fn axis_scalar(start: i16, peak: i16, end: i16, coordinate: i16) -> i64 {
        let (s, p, e, c) = (
            i128::from(start),
            i128::from(peak),
            i128::from(end),
            i128::from(coordinate),
        );
        if s > p || p > e || (s < 0 && e > 0 && p != 0) || p == 0 {
            return 65_536;
        }
        if c < s || c > e {
            return 0;
        }
        if c == p {
            return 65_536;
        }
        if c < p {
            nearest((c - s) * 65_536, p - s) as i64
        } else {
            nearest((e - c) * 65_536, e - p) as i64
        }
    }

    /// A region's scalar: the axes' scalars multiplied in order, each product
    /// rounded to 16.16.
    pub fn region_scalar(axes: &[(i16, i16, i16)], coordinates: &[i16]) -> i64 {
        let mut scalar: i128 = 65_536;
        for (index, (start, peak, end)) in axes.iter().enumerate() {
            let c = coordinates.get(index).copied().unwrap_or(0);
            let factor = i128::from(axis_scalar(*start, *peak, *end, c));
            scalar = nearest(scalar * factor, 65_536);
        }
        scalar as i64
    }

    /// The start, peak and end a peak alone implies.
    pub fn implied(peak: i16) -> (i16, i16, i16) {
        if peak > 0 {
            (0, peak, peak)
        } else if peak < 0 {
            (peak, peak, 0)
        } else {
            (0, 0, 0)
        }
    }

    /// A 16.16 value rounded to whole units, a half rounding up.
    pub fn round_units(value: i64) -> i64 {
        nearest(i128::from(value), 65_536) as i64
    }

    /// The `cvt ` values at 2.14 coordinates, in 16.16, as the `cvar` and
    /// overview chapters compute them from the tables a test stated: each
    /// default plus, for every table, its region's scalar times its delta for
    /// each index it lists, an index past the `cvt ` moving nothing.
    pub fn cvt_at(
        defaults: &[i16],
        shared_points: Option<&super::Points>,
        tuples: &[super::TupleSpec],
        coordinates: &[i16],
    ) -> Vec<i64> {
        let mut values: Vec<i64> = defaults.iter().map(|v| i64::from(*v) * 65_536).collect();
        for tuple in tuples {
            let scalar = region_scalar(&tuple.region(), coordinates);
            let points = tuple
                .points
                .as_ref()
                .or(shared_points)
                .expect("private or shared points");
            let indices: Vec<usize> = match points {
                super::Points::All => (0..defaults.len()).collect(),
                super::Points::Listed(numbers) => numbers.iter().map(|n| usize::from(*n)).collect(),
            };
            assert_eq!(indices.len(), tuple.deltas.len(), "a delta an index");
            for (index, delta) in indices.into_iter().zip(&tuple.deltas) {
                if let Some(value) = values.get_mut(index) {
                    *value += scalar * i64::from(*delta);
                }
            }
        }
        values
    }

    /// One item's net adjustment in 16.16, as the overview's algorithm
    /// computes it from what a test stated: each delta of the row times the
    /// scalar of the region its column names, summed.
    pub fn item_delta(
        regions: &[super::RegionSpec],
        data: &super::DataSpec,
        row: usize,
        coordinates: &[i16],
    ) -> i64 {
        data.regions
            .iter()
            .zip(&data.rows[row])
            .map(|(region, delta)| {
                region_scalar(&regions[usize::from(*region)], coordinates) * i64::from(*delta)
            })
            .sum()
    }

    /// The overview's normalisation of user values on axes stated as
    /// `(min, default, max)`, with no `avar`.
    pub fn location(axes: &[(f64, f64, f64)], user: &[f64]) -> Vec<i16> {
        axes.iter()
            .zip(user)
            .map(|((min, default, max), value)| {
                to_f2dot14(default_normalised(
                    user_fixed(*min),
                    user_fixed(*default),
                    user_fixed(*max),
                    user_fixed(*value),
                ))
            })
            .collect()
    }
}
