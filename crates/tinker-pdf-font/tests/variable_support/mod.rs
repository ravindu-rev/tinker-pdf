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

// ---- glyf, loca and gvar ----------------------------------------------------

/// One point of a simple glyph as a test states it: its position in font
/// units and whether it is on the curve.
pub type GlyphPoint = (i32, i32, bool);

/// A simple glyph's `glyf` data (`glyf`, 'Simple glyph description'): the
/// contour count, the bounding box of its points, each contour's last point,
/// no instructions, one flag a point — on the curve or not, both coordinates
/// signed words — and the coordinates, each the difference from the point
/// before it.
pub fn simple_glyph(contours: &[Vec<GlyphPoint>]) -> Vec<u8> {
    let points: Vec<GlyphPoint> = contours.iter().flatten().copied().collect();
    let mut out = Vec::new();
    push_i16(&mut out, i16::try_from(contours.len()).expect("few"));
    let word = |v: i32| i16::try_from(v).expect("a word");
    let xs = points.iter().map(|p| p.0);
    let ys = points.iter().map(|p| p.1);
    push_i16(&mut out, word(xs.clone().min().unwrap_or(0)));
    push_i16(&mut out, word(ys.clone().min().unwrap_or(0)));
    push_i16(&mut out, word(xs.max().unwrap_or(0)));
    push_i16(&mut out, word(ys.max().unwrap_or(0)));
    let mut end = 0usize;
    for contour in contours {
        end += contour.len();
        push16(&mut out, u16::try_from(end - 1).expect("few points"));
    }
    push16(&mut out, 0); // instructionLength
    for point in &points {
        out.push(u8::from(point.2));
    }
    let mut last = 0;
    for point in &points {
        push_i16(&mut out, word(point.0 - last));
        last = point.0;
    }
    last = 0;
    for point in &points {
        push_i16(&mut out, word(point.1 - last));
        last = point.1;
    }
    out
}

/// One component record as a test states it.
#[derive(Clone, Debug)]
pub struct ComponentSpec {
    /// The flags, `MORE_COMPONENTS` aside, which the builder sets.
    pub flags: u16,
    /// The glyph drawn.
    pub glyph: u16,
    /// The two arguments, written as words when `ARG_1_AND_2_ARE_WORDS` is
    /// set and as bytes otherwise.
    pub arguments: (i32, i32),
    /// The F2Dot14 values the transform flags call for: none, one, two or
    /// four.
    pub transform: Vec<i16>,
}

impl ComponentSpec {
    /// A component placed at an offset given in words, with no transform.
    pub fn at(glyph: u16, x: i32, y: i32) -> ComponentSpec {
        ComponentSpec {
            flags: 0x0003,
            glyph,
            arguments: (x, y),
            transform: Vec::new(),
        }
    }
}

/// A composite glyph's `glyf` data (`glyf`, 'Composite glyph description'):
/// a contour count of −1, the box given, then the records, every one but the
/// last with `MORE_COMPONENTS`.
pub fn composite_glyph(bounds: [i16; 4], components: &[ComponentSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    push_i16(&mut out, -1);
    for value in bounds {
        push_i16(&mut out, value);
    }
    for (index, component) in components.iter().enumerate() {
        let more = if index + 1 < components.len() {
            0x0020
        } else {
            0
        };
        push16(&mut out, (component.flags & !0x0020) | more);
        push16(&mut out, component.glyph);
        let (a1, a2) = component.arguments;
        if component.flags & 0x0001 != 0 {
            if component.flags & 0x0002 != 0 {
                push_i16(&mut out, i16::try_from(a1).expect("a word"));
                push_i16(&mut out, i16::try_from(a2).expect("a word"));
            } else {
                push16(&mut out, u16::try_from(a1).expect("a point number"));
                push16(&mut out, u16::try_from(a2).expect("a point number"));
            }
        } else if component.flags & 0x0002 != 0 {
            out.push(i8::try_from(a1).expect("a byte") as u8);
            out.push(i8::try_from(a2).expect("a byte") as u8);
        } else {
            out.push(u8::try_from(a1).expect("a point number"));
            out.push(u8::try_from(a2).expect("a point number"));
        }
        for value in &component.transform {
            push_i16(&mut out, *value);
        }
    }
    out
}

/// `glyf` and its long `loca`, the glyphs in order, each padded to an even
/// length as a producer pads them; an empty glyph is an empty range.
pub fn glyf_and_loca(glyphs: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    push32(&mut loca, 0);
    for glyph in glyphs {
        glyf.extend_from_slice(glyph);
        if glyf.len() % 2 != 0 {
            glyf.push(0);
        }
        push32(&mut loca, u32::try_from(glyf.len()).expect("small"));
    }
    (glyf, loca)
}

/// A `head` table, version 1.0, its units per em stated and
/// `indexToLocFormat` 1, long offsets.
pub fn head(units_per_em: u16) -> Vec<u8> {
    let mut out = vec![0u8; 54];
    out[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    out[18..20].copy_from_slice(&units_per_em.to_be_bytes());
    out[50..52].copy_from_slice(&1i16.to_be_bytes());
    out
}

/// A `maxp` table, version 0.5: the glyph count alone.
pub fn maxp(glyphs: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0000_5000);
    push16(&mut out, glyphs);
    out
}

/// One glyph's variation data as a test states it: its shared point numbers,
/// where its tables share them, and its tables.
#[derive(Clone, Debug)]
pub struct GlyphVariation {
    /// The point numbers tables without private ones take.
    pub shared_points: Option<Points>,
    /// The tuple variation tables.
    pub tuples: Vec<TupleSpec>,
}

/// A `gvar` table (`gvar`, its header, shared tuples and glyph variation data
/// array): version 1.0, the shared tuples after the offsets, and one
/// `GlyphVariationData` a glyph, `None` writing an empty range. Offsets are
/// 32-bit under `long_offsets` and halved 16-bit ones otherwise, each glyph's
/// data padded to an even length so that a halved offset can reach it.
pub fn gvar(
    axis_count: u16,
    shared: &[Vec<i16>],
    glyphs: &[Option<GlyphVariation>],
    long_offsets: bool,
) -> Vec<u8> {
    let mut array = Vec::new();
    let mut offsets = vec![0usize];
    for glyph in glyphs {
        if let Some(variation) = glyph {
            array.extend(tuple_store(
                0,
                variation.shared_points.as_ref(),
                &variation.tuples,
            ));
            if array.len() % 2 != 0 {
                array.push(0);
            }
        }
        offsets.push(array.len());
    }
    let width = if long_offsets { 4 } else { 2 };
    let shared_at = 20 + offsets.len() * width;
    let shared_len: usize = shared.iter().map(|t| t.len() * 2).sum();
    let array_at = shared_at + shared_len;
    let mut out = Vec::new();
    push16(&mut out, 1);
    push16(&mut out, 0);
    push16(&mut out, axis_count);
    push16(&mut out, u16::try_from(shared.len()).expect("few"));
    push32(&mut out, u32::try_from(shared_at).expect("small"));
    push16(&mut out, u16::try_from(glyphs.len()).expect("few"));
    push16(&mut out, u16::from(long_offsets));
    push32(&mut out, u32::try_from(array_at).expect("small"));
    for offset in offsets {
        if long_offsets {
            push32(&mut out, u32::try_from(offset).expect("small"));
        } else {
            push16(&mut out, u16::try_from(offset / 2).expect("small"));
        }
    }
    for tuple in shared {
        assert_eq!(tuple.len(), usize::from(axis_count), "a coordinate an axis");
        for coordinate in tuple {
            push_i16(&mut out, *coordinate);
        }
    }
    out.extend(array);
    out
}

// ---- CFF2 -----------------------------------------------------------------

/// One item of a charstring as a test writes it.
#[derive(Clone, Debug)]
pub enum Cs {
    /// A whole number, in the shortest encoding the CFF2 chapter's 'Encoded
    /// number formats' gives it.
    Int(i32),
    /// A 16.16 number, in the five-byte form.
    Fixed(i32),
    /// A one-byte operator.
    Op(u8),
    /// A two-byte operator after the escape byte 12.
    Esc(u8),
    /// Raw bytes: a hint mask.
    Bytes(Vec<u8>),
}

/// The CFF2 charstring operators the tests write, by name.
pub mod op {
    pub const HSTEM: u8 = 1;
    pub const VSTEM: u8 = 3;
    pub const VMOVETO: u8 = 4;
    pub const RLINETO: u8 = 5;
    pub const HLINETO: u8 = 6;
    pub const VLINETO: u8 = 7;
    pub const RRCURVETO: u8 = 8;
    pub const CALLSUBR: u8 = 10;
    pub const VSINDEX: u8 = 15;
    pub const BLEND: u8 = 16;
    pub const HSTEMHM: u8 = 18;
    pub const HINTMASK: u8 = 19;
    pub const RMOVETO: u8 = 21;
    pub const HMOVETO: u8 = 22;
    pub const VSTEMHM: u8 = 23;
    pub const CALLGSUBR: u8 = 29;
    pub const HVCURVETO: u8 = 31;
    pub const FLEX: u8 = 35;
}

/// Encodes a charstring.
pub fn charstring(items: &[Cs]) -> Vec<u8> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Cs::Int(value) => {
                let value = *value;
                if (-107..=107).contains(&value) {
                    out.push(u8::try_from(value + 139).expect("one byte"));
                } else if (108..=1_131).contains(&value) {
                    let v = value - 108;
                    out.push(u8::try_from(v / 256 + 247).expect("a lead byte"));
                    out.push(u8::try_from(v % 256).expect("a byte"));
                } else if (-1_131..=-108).contains(&value) {
                    let v = -value - 108;
                    out.push(u8::try_from(v / 256 + 251).expect("a lead byte"));
                    out.push(u8::try_from(v % 256).expect("a byte"));
                } else {
                    out.push(28);
                    push_i16(&mut out, i16::try_from(value).expect("sixteen bits"));
                }
            }
            Cs::Fixed(bits) => {
                out.push(255);
                push_i32(&mut out, *bits);
            }
            Cs::Op(byte) => out.push(*byte),
            Cs::Esc(byte) => {
                out.push(12);
                out.push(*byte);
            }
            Cs::Bytes(bytes) => out.extend_from_slice(bytes),
        }
    }
    out
}

/// Whole numbers, for a charstring.
pub fn ints(values: &[i32]) -> Vec<Cs> {
    values.iter().map(|v| Cs::Int(*v)).collect()
}

/// A CFF2 INDEX (CFF2 chapter, 'INDEX data'): a 32-bit count, four-byte
/// offsets, then the items; an empty one is its four-byte count.
pub fn cff2_index(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, u32::try_from(items.len()).expect("few"));
    if items.is_empty() {
        return out;
    }
    out.push(4);
    let mut offset = 1u32;
    push32(&mut out, offset);
    for item in items {
        offset += u32::try_from(item.len()).expect("small");
        push32(&mut out, offset);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// One DICT entry, every operand in the five-byte 32-bit form so that an
/// offset's width does not depend on its value.
pub fn dict_entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        push_i32(&mut out, *value);
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(u8::try_from(op).expect("a one-byte operator"));
    }
    out
}

/// An FDSelect as a test states it.
#[derive(Clone, Debug)]
pub enum FdSelectSpec {
    /// Format 0: a Font DICT a glyph.
    Format0(Vec<u8>),
    /// Format 3: `(first, fd)` ranges and the sentinel.
    Format3(Vec<(u16, u8)>, u16),
    /// Format 4: `(first, fd)` ranges and the sentinel.
    Format4(Vec<(u32, u16)>, u32),
}

impl FdSelectSpec {
    /// The bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            FdSelectSpec::Format0(fds) => {
                out.push(0);
                out.extend_from_slice(fds);
            }
            FdSelectSpec::Format3(ranges, sentinel) => {
                out.push(3);
                push16(&mut out, u16::try_from(ranges.len()).expect("few"));
                for (first, fd) in ranges {
                    push16(&mut out, *first);
                    out.push(*fd);
                }
                push16(&mut out, *sentinel);
            }
            FdSelectSpec::Format4(ranges, sentinel) => {
                out.push(4);
                push32(&mut out, u32::try_from(ranges.len()).expect("few"));
                for (first, fd) in ranges {
                    push32(&mut out, *first);
                    push16(&mut out, *fd);
                }
                push32(&mut out, *sentinel);
            }
        }
        out
    }
}

/// One Font DICT's Private DICT as a test states it: its entries, and its
/// local subroutines, which the builder points `LocalSubrINDEXOffset` at.
#[derive(Clone, Debug, Default)]
pub struct PrivateSpec {
    /// The DICT's entries, without `LocalSubrINDEXOffset`.
    pub dict: Vec<u8>,
    /// The local subroutines.
    pub subrs: Vec<Vec<u8>>,
}

/// A CFF2 table as a test states it.
#[derive(Clone, Debug, Default)]
pub struct Cff2Spec {
    pub charstrings: Vec<Vec<u8>>,
    pub global_subrs: Vec<Vec<u8>>,
    /// One Private DICT a Font DICT.
    pub privates: Vec<PrivateSpec>,
    pub fd_select: Option<FdSelectSpec>,
    /// An item variation store's bytes, which the builder puts behind its
    /// 16-bit length.
    pub store: Option<Vec<u8>>,
}

impl Cff2Spec {
    /// The Top DICT, its offsets as given.
    pub fn top(&self, charstrings: i32, store: i32, font_dicts: i32, fd_select: i32) -> Vec<u8> {
        let mut out = dict_entry(17, &[charstrings]);
        if self.store.is_some() {
            out.extend(dict_entry(24, &[store]));
        }
        out.extend(dict_entry(0x0C24, &[font_dicts]));
        if self.fd_select.is_some() {
            out.extend(dict_entry(0x0C25, &[fd_select]));
        }
        out
    }

    /// The table's bytes (CFF2 chapter, 'Header' and 'TopDICT'): the header,
    /// the Top DICT, the Global Subr INDEX, the variation store, the
    /// FDSelect, the CharStrings INDEX, the Font DICT INDEX and the Private
    /// DICTs, each followed by its local subroutines.
    pub fn build(&self) -> Vec<u8> {
        let top_size = self.top(0, 0, 0, 0).len();
        let gsubrs = cff2_index(&self.global_subrs);
        let store = self.store.as_ref().map(|bytes| {
            let mut block = Vec::new();
            push16(&mut block, u16::try_from(bytes.len()).expect("small"));
            block.extend_from_slice(bytes);
            block
        });
        let fd_select = self.fd_select.as_ref().map(FdSelectSpec::bytes);
        let charstrings = cff2_index(&self.charstrings);

        let mut at = 5 + top_size + gsubrs.len();
        let store_at = at;
        at += store.as_ref().map_or(0, Vec::len);
        let fd_select_at = at;
        at += fd_select.as_ref().map_or(0, Vec::len);
        let charstrings_at = at;
        at += charstrings.len();
        let font_dicts_at = at;
        // Each Font DICT is one `PrivateDICTOffset` entry of two five-byte
        // operands: eleven bytes, so the INDEX's length is known before the
        // offsets in it are.
        let font_dict_len = 11;
        let index_len = 4 + if self.privates.is_empty() {
            0
        } else {
            1 + 4 * (self.privates.len() + 1) + font_dict_len * self.privates.len()
        };
        at += index_len;
        let mut font_dicts = Vec::new();
        let mut bodies = Vec::new();
        for private in &self.privates {
            let mut body = private.dict.clone();
            if !private.subrs.is_empty() {
                // The entry is six bytes, so the INDEX starts six past the
                // DICT it is in.
                body.extend(dict_entry(
                    19,
                    &[i32::try_from(private.dict.len() + 6).expect("small")],
                ));
            }
            let size = body.len();
            font_dicts.push(dict_entry(
                18,
                &[
                    i32::try_from(size).expect("small"),
                    i32::try_from(at).expect("small"),
                ],
            ));
            body.extend(cff2_index(&private.subrs));
            at += body.len();
            bodies.push(body);
        }
        let font_dict_index = cff2_index(&font_dicts);
        assert_eq!(
            font_dict_index.len(),
            index_len,
            "the Font DICT INDEX's size"
        );

        let top = self.top(
            i32::try_from(charstrings_at).expect("small"),
            i32::try_from(store_at).expect("small"),
            i32::try_from(font_dicts_at).expect("small"),
            i32::try_from(fd_select_at).expect("small"),
        );
        let mut out = vec![2, 0, 5];
        push16(&mut out, u16::try_from(top.len()).expect("small"));
        out.extend(top);
        out.extend(gsubrs);
        if let Some(store) = store {
            assert_eq!(out.len(), store_at);
            out.extend(store);
        }
        if let Some(fd_select) = fd_select {
            assert_eq!(out.len(), fd_select_at);
            out.extend(fd_select);
        }
        assert_eq!(out.len(), charstrings_at);
        out.extend(charstrings);
        assert_eq!(out.len(), font_dicts_at);
        out.extend(font_dict_index);
        for body in bodies {
            out.extend(body);
        }
        out
    }
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

    /// The net deltas of a glyph's items at 2.14 coordinates, in 16.16, as
    /// the `gvar` chapter computes them from the tables a test stated.
    ///
    /// Each table's region's scalar times each delta it lists — the sum of
    /// its deltas for a number listed more than once, a number past the
    /// items moving nothing. For a simple glyph, whose `contours` are given
    /// as `(first, last)` pairs, the points a table lists none of in a
    /// contour that holds some of its points take the delta 'Inferred deltas
    /// for un-referenced point numbers' gives them: from the nearest listed
    /// point before in point order (or the contour's highest listed one) and
    /// the nearest after (or its lowest), each axis on its own — that delta
    /// where the two stand at one coordinate and agree, nothing where they
    /// disagree, the nearer one's outside them, and between them `(1 −
    /// proportion) · before + proportion · after`, its exact value rounded
    /// to 16.16 once and then times the scalar, rounded. For a composite
    /// (`contours` of `None`) nothing is inferred and the phantom points keep
    /// the X deltas of the first two and the Y deltas of the last two.
    pub fn glyph_deltas(
        points: &[(i32, i32)],
        contours: Option<&[(usize, usize)]>,
        variation: &super::GlyphVariation,
        coordinates: &[i16],
    ) -> Vec<[i64; 2]> {
        let items = points.len();
        let mut net = vec![[0i64; 2]; items];
        for tuple in &variation.tuples {
            let scalar = region_scalar(&tuple.region(), coordinates);
            let numbers: Option<Vec<usize>> = match tuple
                .points
                .as_ref()
                .or(variation.shared_points.as_ref())
                .expect("private or shared points")
            {
                super::Points::All => None,
                super::Points::Listed(numbers) => {
                    Some(numbers.iter().map(|n| usize::from(*n)).collect())
                }
            };
            let listed: Vec<usize> = numbers.clone().unwrap_or_else(|| (0..items).collect());
            assert_eq!(
                tuple.deltas.len(),
                2 * listed.len(),
                "an X and a Y a number"
            );
            let half = listed.len();
            let mut stated: Vec<Option<[i64; 2]>> = vec![None; items];
            for (k, item) in listed.iter().enumerate() {
                if *item < items {
                    let sum = stated[*item].get_or_insert([0, 0]);
                    sum[0] += i64::from(tuple.deltas[k]);
                    sum[1] += i64::from(tuple.deltas[half + k]);
                }
            }
            for (slot, delta) in net.iter_mut().zip(&stated) {
                if let Some(delta) = delta {
                    slot[0] += scalar * delta[0];
                    slot[1] += scalar * delta[1];
                }
            }
            let (Some(_), Some(contours)) = (&numbers, contours) else {
                continue;
            };
            for &(first, last) in contours {
                let referenced: Vec<usize> =
                    (first..=last).filter(|i| stated[*i].is_some()).collect();
                if referenced.is_empty() {
                    continue;
                }
                for target in first..=last {
                    if stated[target].is_some() {
                        continue;
                    }
                    let before = *referenced
                        .iter()
                        .rev()
                        .find(|r| **r < target)
                        .or(referenced.last())
                        .expect("a referenced point");
                    let after = *referenced
                        .iter()
                        .find(|r| **r > target)
                        .or(referenced.first())
                        .expect("a referenced point");
                    for axis in 0..2 {
                        let coordinate = |i: usize| {
                            i128::from(if axis == 0 { points[i].0 } else { points[i].1 })
                        };
                        let delta =
                            |i: usize| i128::from(stated[i].expect("a referenced point")[axis]);
                        let (t, p, n) = (coordinate(target), coordinate(before), coordinate(after));
                        let (dp, dn) = (delta(before), delta(after));
                        let inferred = if p == n {
                            if dp == dn {
                                dp * 65_536
                            } else {
                                0
                            }
                        } else if t <= p.min(n) {
                            (if p < n { dp } else { dn }) * 65_536
                        } else if t >= p.max(n) {
                            (if p > n { dp } else { dn }) * 65_536
                        } else {
                            // The preceding point is the reference: the
                            // proportion is (t − p) / (n − p).
                            nearest(((n - t) * dp + (t - p) * dn) * 65_536, n - p)
                        };
                        net[target][axis] += nearest(inferred * i128::from(scalar), 65_536) as i64;
                    }
                }
            }
        }
        if contours.is_none() && items >= 4 {
            net[items - 4][1] = 0;
            net[items - 3][1] = 0;
            net[items - 2][0] = 0;
            net[items - 1][0] = 0;
        }
        net
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
