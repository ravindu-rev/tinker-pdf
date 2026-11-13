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

// ---- The tables a whole face carries beside its variations -----------------

/// How a `post` version 2.0 table names one glyph (`post`, 'Version 2.0').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostName {
    /// An index into the standard Macintosh order the chapter tabulates,
    /// below 258.
    Standard(u16),
    /// A name of the table's own, written as a Pascal string after the
    /// indices and numbered from 258.
    Custom(&'static str),
}

/// A `post` table: version 2.0 naming each glyph where `names` is given and
/// version 3.0, which names none, where it is not; the italic angle as the
/// 16.16 `Fixed` the table stores, the underline position and thickness in
/// font units, and the four memory hints zero.
pub fn post_table(italic_angle: i32, underline: (i16, i16), names: Option<&[PostName]>) -> Vec<u8> {
    let mut out = Vec::new();
    push32(
        &mut out,
        if names.is_some() {
            0x0002_0000
        } else {
            0x0003_0000
        },
    );
    push_i32(&mut out, italic_angle);
    push_i16(&mut out, underline.0);
    push_i16(&mut out, underline.1);
    push32(&mut out, 0); // isFixedPitch
    for _ in 0..4 {
        push32(&mut out, 0); // minMemType42 to maxMemType1
    }
    if let Some(names) = names {
        push16(&mut out, u16::try_from(names.len()).expect("few"));
        let mut custom: Vec<&str> = Vec::new();
        for name in names {
            match name {
                PostName::Standard(index) => {
                    assert!(*index < 258, "a standard index");
                    push16(&mut out, *index);
                }
                PostName::Custom(text) => {
                    push16(&mut out, 258 + u16::try_from(custom.len()).expect("few"));
                    custom.push(text);
                }
            }
        }
        for text in custom {
            out.push(u8::try_from(text.len()).expect("a short name"));
            out.extend_from_slice(text.as_bytes());
        }
    }
    out
}

/// A `cmap` table: version 0 and one encoding record, Windows Unicode BMP
/// (3, 1), over a format 4 subtable (`cmap`, 'Format 4: Segment mapping to
/// delta values'). Each run of consecutive codes whose glyphs are
/// consecutive too is one segment mapped by its `idDelta`, with no
/// `idRangeOffset`, and the closing 0xFFFF segment maps to `.notdef`.
pub fn cmap_table(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut sorted = pairs.to_vec();
    sorted.sort_unstable();
    // `(startCode, endCode, idDelta)`.
    let mut segments: Vec<(u16, u16, u16)> = Vec::new();
    for &(code, glyph) in &sorted {
        assert!(code < 0xFFFF, "0xFFFF closes the subtable");
        let delta = glyph.wrapping_sub(code);
        match segments.last_mut() {
            Some((_, end, run)) if end.checked_add(1) == Some(code) && *run == delta => *end = code,
            _ => segments.push((code, code, delta)),
        }
    }
    segments.push((0xFFFF, 0xFFFF, 1));
    let count = u16::try_from(segments.len()).expect("few segments");
    let mut power = 1u16;
    let mut selector = 0u16;
    while power * 2 <= count {
        power *= 2;
        selector += 1;
    }
    let mut subtable = Vec::new();
    push16(&mut subtable, 4); // format
    push16(&mut subtable, 16 + 8 * count); // length
    push16(&mut subtable, 0); // language
    push16(&mut subtable, count * 2); // segCountX2
    push16(&mut subtable, power * 2); // searchRange
    push16(&mut subtable, selector); // entrySelector
    push16(&mut subtable, count * 2 - power * 2); // rangeShift
    for segment in &segments {
        push16(&mut subtable, segment.1);
    }
    push16(&mut subtable, 0); // reservedPad
    for segment in &segments {
        push16(&mut subtable, segment.0);
    }
    for segment in &segments {
        push16(&mut subtable, segment.2);
    }
    for _ in &segments {
        push16(&mut subtable, 0); // idRangeOffset
    }
    let mut out = Vec::new();
    push16(&mut out, 0); // version
    push16(&mut out, 1); // numTables
    push16(&mut out, 3); // platformID: Windows
    push16(&mut out, 1); // encodingID: Unicode BMP
    push32(&mut out, 12); // the subtable's offset
    out.extend(subtable);
    out
}

/// A `name` table, format 0: one Windows Unicode BMP record (3, 1, 0x0409)
/// a string, in increasing name ID order as the chapter sorts records, each
/// string UTF-16BE in the storage after them.
pub fn name_table(records: &[(u16, &str)]) -> Vec<u8> {
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|record| record.0);
    let count = u16::try_from(sorted.len()).expect("few records");
    let mut out = Vec::new();
    let mut storage = Vec::new();
    push16(&mut out, 0); // version
    push16(&mut out, count);
    push16(&mut out, 6 + 12 * count); // storageOffset
    for (id, text) in &sorted {
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        push16(&mut out, 3);
        push16(&mut out, 1);
        push16(&mut out, 0x0409);
        push16(&mut out, *id);
        push16(
            &mut out,
            u16::try_from(bytes.len()).expect("a short string"),
        );
        push16(
            &mut out,
            u16::try_from(storage.len()).expect("small storage"),
        );
        storage.extend(bytes);
    }
    out.extend(storage);
    out
}

/// What a `head` table states beyond its constants.
#[derive(Clone, Copy, Debug)]
pub struct HeadSpec {
    /// `fontRevision`, a 16.16 `Fixed`.
    pub revision: u32,
    pub flags: u16,
    pub units_per_em: u16,
    /// `xMin`, `yMin`, `xMax`, `yMax`: the union of the glyphs' boxes.
    pub bounds: [i16; 4],
    pub mac_style: u16,
    pub lowest_rec_ppem: u16,
    /// `indexToLocFormat`: long offsets where true, halved short ones where
    /// not.
    pub long_loca: bool,
}

/// A `head` table, version 1.0, 54 bytes: no creation or modification date,
/// so the bytes are a function of the statement alone, and a
/// `checkSumAdjustment` of zero, which [`finish_face`] fills in.
pub fn head_table(spec: &HeadSpec) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000);
    push32(&mut out, spec.revision);
    push32(&mut out, 0); // checkSumAdjustment
    push32(&mut out, 0x5F0F_3CF5); // magicNumber
    push16(&mut out, spec.flags);
    push16(&mut out, spec.units_per_em);
    out.extend_from_slice(&[0; 16]); // created and modified
    for value in spec.bounds {
        push_i16(&mut out, value);
    }
    push16(&mut out, spec.mac_style);
    push16(&mut out, spec.lowest_rec_ppem);
    push_i16(&mut out, 2); // fontDirectionHint
    push_i16(&mut out, i16::from(spec.long_loca));
    push_i16(&mut out, 0); // glyphDataFormat
    assert_eq!(out.len(), 54);
    out
}

/// What an `hhea` or a `vhea` table states: the two share one layout, the
/// vertical one's fields the horizontal ones' turned on end (`vhea`,
/// version 1.1).
#[derive(Clone, Copy, Debug)]
pub struct MetricsHeader {
    /// `0x00010000` for `hhea`, `0x00011000` for `vhea` 1.1.
    pub version: u32,
    /// Ascender, descender and line gap.
    pub lines: (i16, i16, i16),
    /// `advanceWidthMax` or `advanceHeightMax`.
    pub advance_max: u16,
    /// The least side bearing at the start of a glyph's advance.
    pub min_start: i16,
    /// The least at its end.
    pub min_end: i16,
    /// `xMaxExtent` or `yMaxExtent`.
    pub max_extent: i16,
    /// The caret's slope rise and run, and its offset.
    pub caret: (i16, i16, i16),
    /// `numberOfHMetrics` or `numOfLongVerMetrics`.
    pub long_metrics: u16,
}

/// An `hhea` or a `vhea` table, 36 bytes.
pub fn metrics_header(spec: &MetricsHeader) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, spec.version);
    push_i16(&mut out, spec.lines.0);
    push_i16(&mut out, spec.lines.1);
    push_i16(&mut out, spec.lines.2);
    push16(&mut out, spec.advance_max);
    push_i16(&mut out, spec.min_start);
    push_i16(&mut out, spec.min_end);
    push_i16(&mut out, spec.max_extent);
    push_i16(&mut out, spec.caret.0);
    push_i16(&mut out, spec.caret.1);
    push_i16(&mut out, spec.caret.2);
    out.extend_from_slice(&[0; 8]); // reserved
    push_i16(&mut out, 0); // metricDataFormat
    push16(&mut out, spec.long_metrics);
    assert_eq!(out.len(), 36);
    out
}

/// What an `OS/2` table states (`OS/2`, version 4).
#[derive(Clone, Copy, Debug)]
pub struct Os2Spec {
    pub average_width: i16,
    pub weight_class: u16,
    pub width_class: u16,
    /// `ySubscriptXSize`, `YSize`, `XOffset`, `YOffset`.
    pub subscript: [i16; 4],
    /// The superscript's four, likewise.
    pub superscript: [i16; 4],
    /// `yStrikeoutSize` and `yStrikeoutPosition`.
    pub strikeout: (i16, i16),
    pub unicode_ranges: [u32; 4],
    pub vendor: [u8; 4],
    pub selection: u16,
    /// `usFirstCharIndex` and `usLastCharIndex`.
    pub char_range: (u16, u16),
    /// `sTypoAscender`, `sTypoDescender`, `sTypoLineGap`.
    pub typo: (i16, i16, i16),
    /// `usWinAscent` and `usWinDescent`.
    pub win: (u16, u16),
    pub code_pages: [u32; 2],
    pub x_height: i16,
    pub cap_height: i16,
    pub default_char: u16,
    pub break_char: u16,
    pub max_context: u16,
}

/// An `OS/2` table, version 4, 96 bytes, `fsType` zero and the PANOSE
/// classification left unstated.
pub fn os2_table(spec: &Os2Spec) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 4); // version
    push_i16(&mut out, spec.average_width);
    push16(&mut out, spec.weight_class);
    push16(&mut out, spec.width_class);
    push16(&mut out, 0); // fsType: installable
    for value in spec.subscript.iter().chain(&spec.superscript) {
        push_i16(&mut out, *value);
    }
    push_i16(&mut out, spec.strikeout.0);
    push_i16(&mut out, spec.strikeout.1);
    push_i16(&mut out, 0); // sFamilyClass
    out.extend_from_slice(&[0; 10]); // panose
    for range in spec.unicode_ranges {
        push32(&mut out, range);
    }
    out.extend_from_slice(&spec.vendor);
    push16(&mut out, spec.selection);
    push16(&mut out, spec.char_range.0);
    push16(&mut out, spec.char_range.1);
    push_i16(&mut out, spec.typo.0);
    push_i16(&mut out, spec.typo.1);
    push_i16(&mut out, spec.typo.2);
    push16(&mut out, spec.win.0);
    push16(&mut out, spec.win.1);
    for page in spec.code_pages {
        push32(&mut out, page);
    }
    push_i16(&mut out, spec.x_height);
    push_i16(&mut out, spec.cap_height);
    push16(&mut out, spec.default_char);
    push16(&mut out, spec.break_char);
    push16(&mut out, spec.max_context);
    assert_eq!(out.len(), 96);
    out
}

/// What a version 1.0 `maxp` table states.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaxpSpec {
    pub glyphs: u16,
    pub points: u16,
    pub contours: u16,
    pub composite_points: u16,
    pub composite_contours: u16,
    pub zones: u16,
    pub twilight_points: u16,
    pub storage: u16,
    pub function_defs: u16,
    pub instruction_defs: u16,
    pub stack_elements: u16,
    pub size_of_instructions: u16,
    pub component_elements: u16,
    pub component_depth: u16,
}

/// A `maxp` table, version 1.0, 32 bytes, as a face with TrueType outlines
/// carries it.
pub fn maxp_table(spec: &MaxpSpec) -> Vec<u8> {
    let mut out = Vec::new();
    push32(&mut out, 0x0001_0000);
    for value in [
        spec.glyphs,
        spec.points,
        spec.contours,
        spec.composite_points,
        spec.composite_contours,
        spec.zones,
        spec.twilight_points,
        spec.storage,
        spec.function_defs,
        spec.instruction_defs,
        spec.stack_elements,
        spec.size_of_instructions,
        spec.component_elements,
        spec.component_depth,
    ] {
        push16(&mut out, value);
    }
    assert_eq!(out.len(), 32);
    out
}

/// A `STAT` table, version 1.1 (`STAT`, 'Table formats'): its design axis
/// records eight bytes apart, `(tag, axisNameID, axisOrdering)`, no axis
/// value tables, and the elided fallback name.
pub fn stat_table(axes: &[(Tag, u16, u16)], elided_fallback: u16) -> Vec<u8> {
    let mut out = Vec::new();
    push16(&mut out, 1); // majorVersion
    push16(&mut out, 1); // minorVersion
    push16(&mut out, 8); // designAxisSize
    push16(&mut out, u16::try_from(axes.len()).expect("few axes"));
    push32(&mut out, 20); // designAxesOffset
    push16(&mut out, 0); // axisValueCount
    push32(&mut out, 0); // offsetToAxisValueOffsets
    push16(&mut out, elided_fallback);
    for (tag, name_id, ordering) in axes {
        out.extend_from_slice(tag);
        push16(&mut out, *name_id);
        push16(&mut out, *ordering);
    }
    out
}

/// `glyf` and its short `loca`: the glyphs in order, each padded to an even
/// length so that every offset halves exactly, as `indexToLocFormat` 0
/// stores them.
pub fn glyf_and_short_loca(glyphs: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    push16(&mut loca, 0);
    for glyph in glyphs {
        glyf.extend_from_slice(glyph);
        if glyf.len() % 2 != 0 {
            glyf.push(0);
        }
        push16(
            &mut loca,
            u16::try_from(glyf.len() / 2).expect("a short offset"),
        );
    }
    (glyf, loca)
}

/// A face's bytes with `head`'s `checkSumAdjustment` filled in: 0xB1B0AFBA
/// less the sum of the whole font's words, the adjustment counted as zero
/// (`head`, 'checkSumAdjustment'). The directory's own checksum for `head`
/// was taken with the zero, as the chapter says it is.
pub fn finish_face(mut face: Vec<u8>) -> Vec<u8> {
    let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
    let record = (0..count)
        .map(|i| 12 + i * 16)
        .find(|at| face[*at..*at + 4] == *b"head")
        .expect("the face carries head");
    let at = u32::from_be_bytes(
        face[record + 8..record + 12]
            .try_into()
            .expect("four bytes"),
    );
    let at = usize::try_from(at).expect("an offset");
    face[at + 8..at + 12].copy_from_slice(&[0; 4]);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&face));
    face[at + 8..at + 12].copy_from_slice(&adjustment.to_be_bytes());
    face
}

// ---- The committed variable faces (FT-05.8) --------------------------------
//
// `tests/variable/` holds two faces this section states and builds, written
// there by `variable_fixtures.rs`'s ignored writer and held to the builder byte
// for byte by that file's tests. Other crates read them with `include_bytes!`.
// Each is a whole face — `cmap`, `name`, `OS/2`, `post` and the rest — so that
// a document can embed it and a book can set text in it; and each table that
// varies is stated here once, so a test reads its expectation from the same
// statement the bytes were made from.

/// 1.0 in 2.14.
pub const ONE: i16 = 16_384;
/// 0.5 in 2.14.
pub const HALF: i16 = 8_192;

/// An axis's region entry that leaves it out: a zero peak, which the
/// overview's pseudo-code gives a factor of one.
const NONE: (i16, i16, i16) = (0, 0, 0);

/// `y · tan 12°` in font units, rounded to the nearest with a half rounding
/// up — the horizontal shift a twelve-degree oblique gives a point at
/// height `y`, which is what the `slnt` master's deltas state.
pub fn slant(y: i32) -> i32 {
    (y * 2_126 + 5_000).div_euclid(10_000)
}

/// One glyph's outline as a committed face states it.
#[derive(Clone, Debug)]
pub enum Shape {
    /// No outline: a space.
    Empty,
    /// A simple glyph's contours.
    Simple(Vec<Vec<GlyphPoint>>),
    /// A composite's records; its header box is computed from them.
    Composite(Vec<ComponentSpec>),
}

/// One glyph of the committed `glyf` face.
#[derive(Clone, Debug)]
pub struct FixtureGlyph {
    /// Its `post` name.
    pub name: PostName,
    /// The character `cmap` maps to it, if any.
    pub unicode: Option<u16>,
    pub shape: Shape,
    /// Its advance at the default instance; its left side bearing is its
    /// `xMin`.
    pub advance: u16,
    /// Its `gvar` data: one tuple a master that moves it.
    pub variation: Option<GlyphVariation>,
}

/// One named instance as a committed face states it: the record and the two
/// names it points at.
#[derive(Clone, Debug)]
pub struct NamedSpec {
    pub record: InstanceSpec,
    pub subfamily: &'static str,
    pub postscript: &'static str,
}

/// The committed `glyf` face: four axes, seven named instances, an `avar`,
/// and every variation table a TrueType face can carry — `gvar` over simple,
/// composite and empty glyphs, `cvar` over its `cvt `, `HVAR`, `VVAR` and
/// `MVAR` — with hinting tables beside them (`fpgm`, `prep`, `gasp`).
#[derive(Clone, Debug)]
pub struct GlyfFixture {
    pub family: &'static str,
    pub postscript: &'static str,
    pub axes: Vec<AxisSpec>,
    pub axis_names: Vec<&'static str>,
    pub instances: Vec<NamedSpec>,
    /// One `avar` segment map an axis, empty where it does not bend its
    /// axis.
    pub avar: Vec<Vec<(i16, i16)>>,
    /// The regions every variation table of the face is stated over, in the
    /// order this statement names them: a tuple of `gvar` or `cvar` belongs
    /// to the master whose region it has, and `HVAR`'s and `VVAR`'s stores
    /// list the masters their deltas need.
    pub masters: Vec<RegionSpec>,
    /// `gvar`'s shared tuples.
    pub shared_tuples: Vec<Vec<i16>>,
    pub glyphs: Vec<FixtureGlyph>,
    /// Where every glyph's vertical origin stands, its top side bearing
    /// measured down from there to its `yMax`.
    pub vertical_origin: i32,
    /// Every glyph's advance height.
    pub vertical_advance: u16,
    pub cvt: Vec<i16>,
    pub cvar_shared_points: Option<Points>,
    pub cvar: Vec<TupleSpec>,
    /// `MVAR`'s store, stated over regions of its own.
    pub mvar_regions: Vec<RegionSpec>,
    pub mvar_data: Vec<Option<DataSpec>>,
    /// `(tag, outer, inner)`, in binary tag order.
    pub mvar_records: Vec<(Tag, u16, u16)>,
    pub fpgm: Vec<u8>,
    pub prep: Vec<u8>,
    pub gasp: Vec<(u16, u16)>,
    /// `hhea`'s ascender, descender and line gap.
    pub horizontal_lines: (i16, i16, i16),
    /// `vhea`'s.
    pub vertical_lines: (i16, i16, i16),
    pub os2: Os2Spec,
    /// `post`'s underline position and thickness.
    pub underline: (i16, i16),
}

/// A tuple of the master whose region `region` is: its peak shared where
/// `shared` holds it, an intermediate region written out, and the deltas the
/// X ones and then the Y ones.
fn master_tuple(
    region: &RegionSpec,
    shared: &[Vec<i16>],
    points: Option<Points>,
    dx: &[i32],
    dy: &[i32],
) -> TupleSpec {
    assert_eq!(dx.len(), dy.len(), "an X and a Y delta an item");
    let peak: Vec<i16> = region.iter().map(|axis| axis.1).collect();
    let implied = region.iter().all(|axis| spec::implied(axis.1) == *axis);
    let deltas: Vec<i32> = dx.iter().chain(dy).copied().collect();
    let mut tuple = TupleSpec::new(&peak, points, &deltas);
    if implied {
        if let Some(index) = shared.iter().position(|tuple| *tuple == peak) {
            tuple.shared_peak = Some(u16::try_from(index).expect("few shared tuples"));
        }
    } else {
        let start: Vec<i16> = region.iter().map(|axis| axis.0).collect();
        let end: Vec<i16> = region.iter().map(|axis| axis.2).collect();
        tuple = tuple.between(&start, &end);
    }
    tuple
}

/// A rectangle's four corners as one clockwise contour: up the left side,
/// across the top, down the right.
fn rectangle(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<GlyphPoint> {
    vec![
        (x0, y0, true),
        (x0, y1, true),
        (x1, y1, true),
        (x1, y0, true),
    ]
}

/// The statement of the committed `glyf` face.
#[allow(clippy::too_many_lines, reason = "one face, stated in one place")]
pub fn glyf_fixture() -> GlyfFixture {
    let axes = vec![
        AxisSpec::new(*b"wght", 100.0, 400.0, 900.0, 256),
        AxisSpec::new(*b"wdth", 75.0, 100.0, 125.0, 257),
        AxisSpec::new(*b"slnt", -12.0, 0.0, 0.0, 258),
        AxisSpec::new(*b"opsz", 8.0, 12.0, 72.0, 259),
    ];
    let instance = |id: u16, coordinates: [f64; 4], postscript: u16| {
        InstanceSpec::new(id, &coordinates, postscript)
    };
    let instances = vec![
        NamedSpec {
            record: instance(260, [100.0, 100.0, 0.0, 12.0], 270),
            subfamily: "Thin",
            postscript: "TinkerVarGlyf-Thin",
        },
        NamedSpec {
            record: instance(261, [400.0, 100.0, 0.0, 12.0], 271),
            subfamily: "Regular",
            postscript: "TinkerVarGlyf-Regular",
        },
        NamedSpec {
            record: instance(262, [700.0, 100.0, 0.0, 12.0], 272),
            subfamily: "Bold",
            postscript: "TinkerVarGlyf-Bold",
        },
        NamedSpec {
            record: instance(263, [900.0, 100.0, 0.0, 12.0], 273),
            subfamily: "Black",
            postscript: "TinkerVarGlyf-Black",
        },
        NamedSpec {
            record: instance(264, [700.0, 75.0, 0.0, 12.0], 274),
            subfamily: "Condensed Bold",
            postscript: "TinkerVarGlyf-CondensedBold",
        },
        NamedSpec {
            record: instance(265, [700.0, 100.0, -12.0, 12.0], 275),
            subfamily: "Bold Oblique",
            postscript: "TinkerVarGlyf-BoldOblique",
        },
        NamedSpec {
            record: instance(266, [400.0, 125.0, 0.0, 72.0], 276),
            subfamily: "Wide Display",
            postscript: "TinkerVarGlyf-WideDisplay",
        },
    ];
    // The weight bends: its lower half is reached slower and its upper half
    // faster. The width and the optical size keep their scales through the
    // three correspondences, and the slant has an empty map, which leaves
    // it alone.
    let avar = vec![
        vec![
            (-ONE, -ONE),
            (-HALF, -6_144),
            (0, 0),
            (HALF, 7_168),
            (ONE, ONE),
        ],
        identity_map(),
        Vec::new(),
        identity_map(),
    ];
    // M0 the heaviest weight, M1 the lightest, M2 a bump halfway to the
    // heaviest, M3 the narrowest width, M4 the widest, M5 the full slant,
    // M6 the largest optical size, M7 the smallest, and M8 the heavy and
    // narrow corner, whose deltas correct the two read together.
    let masters: Vec<RegionSpec> = vec![
        vec![(0, ONE, ONE), NONE, NONE, NONE],
        vec![(-ONE, -ONE, 0), NONE, NONE, NONE],
        vec![(0, HALF, ONE), NONE, NONE, NONE],
        vec![NONE, (-ONE, -ONE, 0), NONE, NONE],
        vec![NONE, (0, ONE, ONE), NONE, NONE],
        vec![NONE, NONE, (-ONE, -ONE, 0), NONE],
        vec![NONE, NONE, NONE, (0, ONE, ONE)],
        vec![NONE, NONE, NONE, (-ONE, -ONE, 0)],
        vec![(0, ONE, ONE), (-ONE, -ONE, 0), NONE, NONE],
    ];
    let shared_tuples = vec![
        vec![ONE, 0, 0, 0],
        vec![-ONE, 0, 0, 0],
        vec![0, -ONE, 0, 0],
        vec![0, 0, -ONE, 0],
    ];
    let t = |master: usize, points: Option<Points>, dx: &[i32], dy: &[i32]| {
        master_tuple(&masters[master], &shared_tuples, points, dx, dy)
    };
    let listed = |numbers: &[u16]| Some(Points::Listed(numbers.to_vec()));
    let all = Some(Points::All);
    // A slant's X deltas for points at the heights given, the four phantom
    // points still.
    let sheared = |heights: &[i32]| -> Vec<i32> {
        let mut dx: Vec<i32> = heights.iter().map(|y| slant(*y)).collect();
        dx.extend([0, 0, 0, 0]);
        dx
    };
    let zeros = |n: usize| vec![0; n];

    // .notdef: a box with a hole, outer contour clockwise and inner counter.
    let notdef_outer = rectangle(50, 0, 450, 700);
    let notdef_inner = vec![
        (100, 50, true),
        (400, 50, true),
        (400, 650, true),
        (100, 650, true),
    ];
    let notdef_heights = [0, 700, 700, 0, 50, 50, 650, 650];
    let notdef = FixtureGlyph {
        name: PostName::Standard(0),
        unicode: None,
        shape: Shape::Simple(vec![notdef_outer, notdef_inner]),
        advance: 500,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(
                    0,
                    all.clone(),
                    &[0, 0, 0, 0, 30, -30, -30, 30, 0, 0, 0, 0],
                    &[0, 0, 0, 0, 30, 30, -30, -30, 0, 0, 0, 0],
                ),
                t(
                    1,
                    listed(&[4, 5, 6, 7]),
                    &[-20, 20, 20, -20],
                    &[-20, -20, 20, 20],
                ),
                t(2, listed(&[4, 6]), &[5, -5], &[5, -5]),
                t(3, listed(&[0, 2, 9]), &[20, -60, -80], &[0, 0, 0]),
                t(5, all.clone(), &sheared(&notdef_heights), &zeros(12)),
                t(
                    6,
                    all.clone(),
                    &[0, 0, 0, 0, -10, 10, 10, -10, 0, -20, 0, 0],
                    &[0, 0, 0, 0, -10, -10, 10, 10, 0, 0, 0, 0],
                ),
            ],
        }),
    };

    // space: nothing to draw, so its tables move its phantom points alone.
    let space = FixtureGlyph {
        name: PostName::Standard(3),
        unicode: Some(0x0020),
        shape: Shape::Empty,
        advance: 250,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(0, all.clone(), &[0, 20, 0, 0], &[0, 0, 0, 0]),
                t(3, listed(&[1]), &[-50], &[0]),
                t(4, listed(&[1]), &[50], &[0]),
                t(7, listed(&[1]), &[30], &[0]),
            ],
        }),
    };

    // period: a quadratic dot, its on-curve points at the compass points
    // and its corners off the curve. The lightest weight lists the on-curve
    // points and the advance through the glyph's shared point numbers, so
    // the corners are inferred.
    let dot = vec![
        (90, 60, true),
        (90, 120, false),
        (150, 120, true),
        (210, 120, false),
        (210, 60, true),
        (210, 0, false),
        (150, 0, true),
        (90, 0, false),
    ];
    let dot_heights = [60, 120, 120, 120, 60, 0, 0, 0];
    let period = FixtureGlyph {
        name: PostName::Standard(17),
        unicode: Some(0x002E),
        shape: Shape::Simple(vec![dot]),
        advance: 300,
        variation: Some(GlyphVariation {
            shared_points: Some(Points::Listed(vec![0, 2, 4, 6, 9])),
            tuples: vec![
                t(
                    0,
                    all.clone(),
                    &[-20, -20, 0, 20, 20, 20, 0, -20, 0, 40, 0, 0],
                    &[20, 40, 40, 40, 20, 0, 0, 0, 0, 0, 0, 0],
                ),
                t(1, None, &[10, 0, -10, 0, -30], &[-5, -20, -5, 0, 0]),
                t(
                    3,
                    all.clone(),
                    &[10, 10, 0, -10, -10, -10, 0, 10, 0, -30, 0, 0],
                    &zeros(12),
                ),
                t(5, all.clone(), &sheared(&dot_heights), &zeros(12)),
            ],
        }),
    };

    // colon: two dots, the lower one supplying the metrics. Its tables move
    // the upper dot's offset, and its own phantom points agree with the
    // dot's although `USE_MY_METRICS` makes them unread.
    let mut lower = ComponentSpec::at(2, 0, 0);
    lower.flags |= 0x0004 | 0x0200; // ROUND_XY_TO_GRID, USE_MY_METRICS
    let mut upper = ComponentSpec::at(2, 0, 400);
    upper.flags |= 0x0004;
    let colon = FixtureGlyph {
        name: PostName::Standard(29),
        unicode: Some(0x003A),
        shape: Shape::Composite(vec![lower, upper]),
        advance: 300,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(0, listed(&[1, 3]), &[0, 40], &[30, 0]),
                t(1, listed(&[1, 3]), &[0, -30], &[-20, 0]),
                t(5, listed(&[1]), &[slant(400)], &[0]),
            ],
        }),
    };

    // H: two stems and a bar, the bar a scaled component.
    let mut left = ComponentSpec::at(5, 0, 0);
    left.flags |= 0x0004;
    let mut right = ComponentSpec::at(5, 450, 0);
    right.flags |= 0x0004;
    let mut bar = ComponentSpec::at(7, 200, 310);
    bar.flags |= 0x0004 | 0x0040; // WE_HAVE_AN_X_AND_Y_SCALE
    bar.transform = vec![28_672, ONE]; // 1.75 across, 1 up
    let h = FixtureGlyph {
        name: PostName::Standard(43),
        unicode: Some(0x0048),
        shape: Shape::Composite(vec![left, right, bar]),
        advance: 750,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(0, all.clone(), &[0, 60, 60, 0, 120, 0, 0], &zeros(7)),
                t(1, listed(&[1, 2, 4]), &[-60, -60, -120], &[0, 0, 0]),
                t(3, listed(&[1, 2, 4]), &[-100, -20, -160], &[0, 0, 0]),
                t(5, listed(&[2]), &[slant(310)], &[0]),
                t(6, all.clone(), &[0, -20, -10, 0, -40, 0, 0], &zeros(7)),
                t(8, listed(&[1, 4]), &[-20, -20], &[0, 0]),
            ],
        }),
    };

    // I: one stem, its right edge and its advance moved by the weight, the
    // width and the optical size; slanted by every point, since a shear is
    // not something the inferred deltas can express.
    let i = FixtureGlyph {
        name: PostName::Standard(44),
        unicode: Some(0x0049),
        shape: Shape::Simple(vec![rectangle(100, 0, 200, 700)]),
        advance: 300,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(0, all.clone(), &[0, 0, 60, 60, 0, 60, 0, 0], &zeros(8)),
                t(1, all.clone(), &[0, 0, -60, -60, 0, -60, 0, 0], &zeros(8)),
                t(2, listed(&[2, 3, 5]), &[10, 10, 10], &[0, 0, 0]),
                t(
                    3,
                    listed(&[0, 1, 2, 3, 5]),
                    &[-20, -20, -40, -40, -60],
                    &[0, 0, 0, 0, 0],
                ),
                t(4, listed(&[2, 3, 5]), &[20, 20, 40], &[0, 0, 0]),
                t(5, all.clone(), &sheared(&[0, 700, 700, 0]), &zeros(8)),
                t(6, all.clone(), &[0, 0, -20, -20, 0, -20, 0, 0], &zeros(8)),
                t(7, all.clone(), &[0, 0, 20, 20, 0, 30, 0, 0], &zeros(8)),
                t(8, listed(&[5]), &[-10], &[0]),
            ],
        }),
    };

    // O: two quadratic rings. Its heaviest and lightest weights list the
    // inner ring's on-curve points only, so its corners are inferred and the
    // outer ring, none of whose points is listed, does not move; its
    // largest optical size moves its advance height.
    let outer = vec![
        (50, 350, true),
        (50, 710, false),
        (375, 710, true),
        (700, 710, false),
        (700, 350, true),
        (700, -10, false),
        (375, -10, true),
        (50, -10, false),
    ];
    let inner = vec![
        (150, 350, true),
        (150, 90, false),
        (375, 90, true),
        (600, 90, false),
        (600, 350, true),
        (600, 610, false),
        (375, 610, true),
        (150, 610, false),
    ];
    let o_heights = [
        350, 710, 710, 710, 350, -10, -10, -10, 350, 90, 90, 90, 350, 610, 610, 610,
    ];
    let o_narrow = [0, 0, -40, -80, -80, -80, -40, 0];
    let o_wide = [0, 0, 40, 80, 80, 80, 40, 0];
    let ring_x = |ring: &[i32], advance: i32| -> Vec<i32> {
        let mut dx: Vec<i32> = ring.iter().chain(ring).copied().collect();
        dx.extend([0, advance, 0, 0]);
        dx
    };
    let mut o_large_dx = zeros(8);
    o_large_dx.extend([-10, -10, 0, 10, 10, 10, 0, -10]);
    o_large_dx.extend([0, 0, 0, 0]);
    let mut o_large_dy = zeros(8);
    o_large_dy.extend([0, -10, -10, -10, 0, 10, 10, 10]);
    o_large_dy.extend([0, 0, 0, -20]);
    let o = FixtureGlyph {
        name: PostName::Standard(50),
        unicode: Some(0x004F),
        shape: Shape::Simple(vec![outer, inner]),
        advance: 750,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(
                    0,
                    listed(&[8, 10, 12, 14, 17]),
                    &[40, 0, -40, 0, 0],
                    &[0, 30, 0, -30, 0],
                ),
                t(
                    1,
                    listed(&[8, 10, 12, 14]),
                    &[-30, 0, 30, 0],
                    &[0, -20, 0, 20],
                ),
                t(2, listed(&[0, 4]), &[-5, 5], &[0, 0]),
                t(3, all.clone(), &ring_x(&o_narrow, -80), &zeros(20)),
                t(4, all.clone(), &ring_x(&o_wide, 80), &zeros(20)),
                t(5, all.clone(), &sheared(&o_heights), &zeros(20)),
                t(6, all.clone(), &o_large_dx, &o_large_dy),
                t(7, listed(&[8, 12]), &[15, -15], &[0, 0]),
            ],
        }),
    };

    // hbar: H's bar, drawn only as a component, so no character reaches it.
    // Its slant is the oblique's divided by the 1.75 H scales it by.
    let hbar = FixtureGlyph {
        name: PostName::Custom("hbar"),
        unicode: None,
        shape: Shape::Simple(vec![rectangle(0, 0, 200, 80)]),
        advance: 200,
        variation: Some(GlyphVariation {
            shared_points: None,
            tuples: vec![
                t(0, listed(&[0, 1, 2, 3]), &[0, 0, 0, 0], &[-10, 10, 10, -10]),
                t(1, listed(&[0, 1, 2, 3]), &[0, 0, 0, 0], &[10, -10, -10, 10]),
                t(3, listed(&[2, 3]), &[-46, -46], &[0, 0]),
                t(5, all.clone(), &[0, 10, 10, 0, 0, 0, 0, 0], &zeros(8)),
            ],
        }),
    };

    // cvt: a zero, a stem width, the cap height, a zero, the x-height and an
    // overshoot. The heaviest weight and the largest size move three
    // entries through the shared point numbers.
    let cvt_region = |master: usize| -> TupleSpec {
        let region = &masters[master];
        let peak: Vec<i16> = region.iter().map(|axis| axis.1).collect();
        TupleSpec::new(&peak, None, &[])
    };
    let with = |mut tuple: TupleSpec, points: Option<Points>, deltas: &[i32]| {
        tuple.points = points;
        tuple.deltas = deltas.to_vec();
        tuple
    };
    let cvar = vec![
        with(cvt_region(0), None, &[60, 10, 10]),
        with(cvt_region(1), listed(&[1]), &[-60]),
        with(cvt_region(2), listed(&[1]), &[10]).between(&[0, 0, 0, 0], &[ONE, 0, 0, 0]),
        with(cvt_region(3), all.clone(), &[0, -20, 0, 0, 0, 0]),
        with(cvt_region(6), None, &[-20, -10, -10]),
    ];

    // MVAR: sixteen metrics in the first subtable, over the slant (whose
    // caret deltas need words), the weights, the narrowest width and the two
    // optical sizes; the first gasp range in the second, over the optical
    // sizes alone.
    let mvar_regions = vec![
        masters[5].clone(),
        masters[0].clone(),
        masters[1].clone(),
        masters[3].clone(),
        masters[6].clone(),
        masters[7].clone(),
    ];
    let metric_rows: Vec<(Tag, [i32; 6])> = vec![
        (*b"cpht", [0, 10, -5, 0, -10, 10]),
        (*b"hasc", [0, 20, 0, 0, 0, 0]),
        (*b"hcla", [0, 30, 0, 0, 0, 0]),
        (*b"hcld", [0, 10, 0, 0, 0, 0]),
        (*b"hcof", [-30, 0, 0, 0, 0, 0]),
        (*b"hcrn", [213, 0, 0, 0, 0, 0]),
        (*b"hcrs", [999, 0, 0, 0, 0, 0]),
        (*b"hdsc", [0, -10, 0, 0, 0, 0]),
        (*b"hlgp", [0, 0, 0, 0, -20, 20]),
        (*b"stro", [0, 10, -5, 0, 0, 0]),
        (*b"strs", [0, 30, -20, 0, -10, 10]),
        (*b"undo", [0, -10, 5, 0, 0, 0]),
        (*b"unds", [0, 30, -20, 0, -10, 10]),
        (*b"vasc", [0, 0, 0, -20, 0, 0]),
        (*b"vdsc", [0, 0, 0, 20, 0, 0]),
        (*b"xhgt", [0, 10, -5, 0, -10, 10]),
    ];
    let mvar_data = vec![
        Some(DataSpec {
            regions: vec![0, 1, 2, 3, 4, 5],
            word_count: 1,
            long_words: false,
            rows: metric_rows.iter().map(|(_, row)| row.to_vec()).collect(),
        }),
        Some(DataSpec {
            regions: vec![4, 5],
            word_count: 0,
            long_words: false,
            rows: vec![vec![4, -2]],
        }),
    ];
    let mut mvar_records: Vec<(Tag, u16, u16)> = metric_rows
        .iter()
        .enumerate()
        .map(|(row, (tag, _))| (*tag, 0, u16::try_from(row).expect("few rows")))
        .collect();
    mvar_records.push((*b"gsp0", 1, 0));
    mvar_records.sort_by(|a, b| a.0.cmp(&b.0));

    GlyfFixture {
        family: "Tinker Var Glyf",
        postscript: "TinkerVarGlyf",
        axes,
        axis_names: vec!["Weight", "Width", "Slant", "Optical Size"],
        instances,
        avar,
        masters,
        shared_tuples,
        glyphs: vec![notdef, space, period, colon, h, i, o, hbar],
        vertical_origin: 880,
        vertical_advance: 1_000,
        cvt: vec![0, 100, 700, 0, 500, -10],
        cvar_shared_points: Some(Points::Listed(vec![1, 2, 4])),
        cvar,
        mvar_regions,
        mvar_data,
        mvar_records,
        // fpgm: function 0 sets every vector to the x-axis. prep: dropout
        // control on at every size, and its rule.
        fpgm: vec![0xB0, 0x00, 0x2C, 0x01, 0x2D],
        prep: vec![0xB8, 0x01, 0xFF, 0x85, 0xB0, 0x04, 0x8D],
        gasp: vec![(8, 0x000A), (0xFFFF, 0x000F)],
        horizontal_lines: (800, -200, 200),
        vertical_lines: (500, -500, 0),
        os2: Os2Spec {
            average_width: 0,
            weight_class: 400,
            width_class: 5,
            subscript: [650, 600, 0, 75],
            superscript: [650, 600, 0, 350],
            strikeout: (50, 300),
            unicode_ranges: [1, 0, 0, 0],
            vendor: *b"NONE",
            selection: 0x0040,
            char_range: (0x0020, 0x004F),
            typo: (800, -200, 200),
            win: (950, 250),
            code_pages: [1, 0],
            x_height: 500,
            cap_height: 700,
            default_char: 0,
            break_char: 0x0020,
            max_context: 0,
        },
        underline: (-100, 50),
    }
}

/// A component's placement, `[dx, dy, a, b, c, d]`: a point goes to
/// `(a x + c y + dx, b x + d y + dy)`.
pub type Placement = [f64; 6];

/// The identity placement.
pub const UNPLACED: Placement = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];

/// A record's 2×2 from the F2Dot14 values its flags call for.
pub fn component_matrix(component: &ComponentSpec) -> [f64; 4] {
    let f = |v: i16| f64::from(v) / 16_384.0;
    match component.transform.as_slice() {
        [] => [1.0, 0.0, 0.0, 1.0],
        [s] => [f(*s), 0.0, 0.0, f(*s)],
        [x, y] => [f(*x), 0.0, 0.0, f(*y)],
        [a, b, c, d] => [f(*a), f(*b), f(*c), f(*d)],
        other => panic!("a transform of {} values", other.len()),
    }
}

/// A component's placement inside its parent's: its own 2×2 after the
/// parent's, and its offset placed by the parent's — the `glyf` reading,
/// where a component's own transform does not scale its offset.
pub fn place_component(parent: Placement, matrix: [f64; 4], offset: (f64, f64)) -> Placement {
    let [dx, dy, a, b, c, d] = parent;
    let [ca, cb, cc, cd] = matrix;
    [
        offset.0 * a + offset.1 * c + dx,
        offset.0 * b + offset.1 * d + dy,
        ca * a + cb * c,
        ca * b + cb * d,
        cc * a + cd * c,
        cc * b + cd * d,
    ]
}

impl GlyfFixture {
    /// How many items a glyph's tables number before its phantom points: its
    /// contour points, or its components.
    pub fn items(&self, glyph: usize) -> usize {
        match &self.glyphs[glyph].shape {
            Shape::Empty => 0,
            Shape::Simple(contours) => contours.iter().map(Vec::len).sum(),
            Shape::Composite(components) => components.len(),
        }
    }

    /// The master a tuple belongs to: the one whose region it has.
    pub fn master_of(&self, tuple: &TupleSpec) -> usize {
        let region = tuple.region();
        self.masters
            .iter()
            .position(|master| *master == region)
            .expect("every tuple belongs to a master")
    }

    /// The glyph whose metrics a glyph takes: the last component carrying
    /// `USE_MY_METRICS`, followed through, or the glyph itself.
    pub fn metrics_glyph(&self, glyph: usize) -> usize {
        match &self.glyphs[glyph].shape {
            Shape::Composite(components) => components
                .iter()
                .rev()
                .find(|c| c.flags & 0x0200 != 0)
                .map_or(glyph, |c| self.metrics_glyph(usize::from(c.glyph))),
            _ => glyph,
        }
    }

    /// The deltas each master's tables give a glyph's four phantom points,
    /// `[x, y]` each, the master's tables summed: what `HVAR` and `VVAR`
    /// restate, so that a reader of either source finds the same metrics.
    pub fn phantom_deltas(&self, glyph: usize) -> Vec<[[i32; 2]; 4]> {
        let mut out = vec![[[0; 2]; 4]; self.masters.len()];
        let Some(variation) = &self.glyphs[glyph].variation else {
            return out;
        };
        let items = self.items(glyph);
        for tuple in &variation.tuples {
            let master = self.master_of(tuple);
            let points = tuple
                .points
                .as_ref()
                .or(variation.shared_points.as_ref())
                .expect("private or shared point numbers");
            let numbers: Vec<usize> = match points {
                Points::All => (0..items + 4).collect(),
                Points::Listed(listed) => listed.iter().map(|n| usize::from(*n)).collect(),
            };
            let half = numbers.len();
            for (k, number) in numbers.iter().enumerate() {
                if let Some(phantom) = number.checked_sub(items).filter(|p| *p < 4) {
                    out[master][phantom][0] += tuple.deltas[k];
                    out[master][phantom][1] += tuple.deltas[half + k];
                }
            }
        }
        out
    }

    /// Each master's change to a glyph's advance width: its second phantom
    /// point's X delta less its first's, read from the glyph its metrics
    /// come from.
    pub fn advance_deltas(&self, glyph: usize) -> Vec<i32> {
        self.phantom_deltas(self.metrics_glyph(glyph))
            .iter()
            .map(|p| p[1][0] - p[0][0])
            .collect()
    }

    /// Each master's change to a glyph's advance height: its third phantom
    /// point's Y delta less its fourth's.
    pub fn vertical_advance_deltas(&self, glyph: usize) -> Vec<i32> {
        self.phantom_deltas(self.metrics_glyph(glyph))
            .iter()
            .map(|p| p[2][1] - p[3][1])
            .collect()
    }

    /// Every point a glyph draws at the default instance, placed: a simple
    /// glyph's contour points, and a composite's components' through their
    /// placements.
    pub fn placed_points(&self, glyph: usize, placement: Placement) -> Vec<(f64, f64)> {
        let [dx, dy, a, b, c, d] = placement;
        match &self.glyphs[glyph].shape {
            Shape::Empty => Vec::new(),
            Shape::Simple(contours) => contours
                .iter()
                .flatten()
                .map(|p| {
                    let (x, y) = (f64::from(p.0), f64::from(p.1));
                    (a * x + c * y + dx, b * x + d * y + dy)
                })
                .collect(),
            Shape::Composite(components) => components
                .iter()
                .flat_map(|component| {
                    let offset = (
                        f64::from(component.arguments.0),
                        f64::from(component.arguments.1),
                    );
                    let child = place_component(placement, component_matrix(component), offset);
                    self.placed_points(usize::from(component.glyph), child)
                })
                .collect(),
        }
    }

    /// A glyph's box at the default instance, `[xMin, yMin, xMax, yMax]`:
    /// its points', floored and ceiled to font units; `None` for a glyph
    /// that draws nothing.
    pub fn bounds(&self, glyph: usize) -> Option<[i32; 4]> {
        let points = self.placed_points(glyph, UNPLACED);
        if points.is_empty() {
            return None;
        }
        let fold = |pick: fn(&(f64, f64)) -> f64, min: bool| {
            points
                .iter()
                .map(pick)
                .fold(if min { f64::MAX } else { f64::MIN }, |acc, v| {
                    if min {
                        acc.min(v)
                    } else {
                        acc.max(v)
                    }
                })
        };
        Some([
            fold(|p| p.0, true).floor() as i32,
            fold(|p| p.1, true).floor() as i32,
            fold(|p| p.0, false).ceil() as i32,
            fold(|p| p.1, false).ceil() as i32,
        ])
    }

    /// A glyph's `glyf` data.
    pub fn glyph_bytes(&self, glyph: usize) -> Vec<u8> {
        match &self.glyphs[glyph].shape {
            Shape::Empty => Vec::new(),
            Shape::Simple(contours) => simple_glyph(contours),
            Shape::Composite(components) => {
                let [x0, y0, x1, y1] = self.bounds(glyph).expect("a composite draws");
                let word = |v: i32| i16::try_from(v).expect("a word");
                composite_glyph([word(x0), word(y0), word(x1), word(y1)], components)
            }
        }
    }

    /// The masters whose deltas `metric` gives some glyph, in master order:
    /// the regions `HVAR`'s or `VVAR`'s store lists.
    fn metric_masters(&self, metric: impl Fn(usize) -> Vec<i32>) -> Vec<usize> {
        let rows: Vec<Vec<i32>> = (0..self.glyphs.len()).map(&metric).collect();
        (0..self.masters.len())
            .filter(|m| rows.iter().any(|row| row[*m] != 0))
            .collect()
    }

    /// An item variation data subtable over `columns` of the masters, its
    /// columns needing words first, as `wordDeltaCount` requires.
    fn metric_data(columns: &[usize], rows: &[Vec<i32>]) -> (Vec<usize>, DataSpec) {
        let word = |column: usize| rows.iter().any(|row| !(-128..=127).contains(&row[column]));
        let mut order: Vec<usize> = (0..columns.len()).filter(|c| word(*c)).collect();
        let word_count = order.len();
        order.extend((0..columns.len()).filter(|c| !word(*c)));
        let data = DataSpec {
            regions: order
                .iter()
                .map(|c| u16::try_from(*c).expect("few regions"))
                .collect(),
            word_count: u16::try_from(word_count).expect("few"),
            long_words: false,
            rows: rows
                .iter()
                .map(|row| order.iter().map(|c| row[*c]).collect())
                .collect(),
        };
        (order.iter().map(|c| columns[*c]).collect(), data)
    }

    /// The `HVAR` table: a store over the masters that move some advance,
    /// one row a distinct set of advance deltas, and an advance map from
    /// each glyph to its row.
    pub fn hvar_table(&self) -> Vec<u8> {
        let masters = self.metric_masters(|g| self.advance_deltas(g));
        let mut rows: Vec<Vec<i32>> = Vec::new();
        let mut entries: Vec<(u32, u32)> = Vec::new();
        for glyph in 0..self.glyphs.len() {
            let deltas = self.advance_deltas(glyph);
            let row: Vec<i32> = masters.iter().map(|m| deltas[*m]).collect();
            let index = rows.iter().position(|r| *r == row).unwrap_or_else(|| {
                rows.push(row);
                rows.len() - 1
            });
            entries.push((0, u32::try_from(index).expect("few rows")));
        }
        let (_, data) = Self::metric_data(&masters, &rows);
        let regions: Vec<RegionSpec> = masters.iter().map(|m| self.masters[*m].clone()).collect();
        let axis_count = u16::try_from(self.axes.len()).expect("few axes");
        let store = item_store(axis_count, &regions, &[Some(data)]);
        let inner_bits =
            (u32::BITS - u32::try_from(rows.len() - 1).expect("few").leading_zeros()).max(1);
        let map = index_map(
            0,
            1,
            u8::try_from(inner_bits).expect("a few bits"),
            &entries,
        );
        hvar(&store, Some(map), None, None)
    }

    /// The `VVAR` table: a store over the masters that move some advance
    /// height, its first subtable one row a glyph, read through no map — the
    /// glyph id as the inner index, as the chapter allows.
    pub fn vvar_table(&self) -> Vec<u8> {
        let masters = self.metric_masters(|g| self.vertical_advance_deltas(g));
        let rows: Vec<Vec<i32>> = (0..self.glyphs.len())
            .map(|g| {
                let deltas = self.vertical_advance_deltas(g);
                masters.iter().map(|m| deltas[*m]).collect()
            })
            .collect();
        let (_, data) = Self::metric_data(&masters, &rows);
        let regions: Vec<RegionSpec> = masters.iter().map(|m| self.masters[*m].clone()).collect();
        let axis_count = u16::try_from(self.axes.len()).expect("few axes");
        let store = item_store(axis_count, &regions, &[Some(data)]);
        vvar(&store, None, None, None, None)
    }

    /// The `name` table: the family and its names, the axes' names, and each
    /// named instance's subfamily and PostScript names.
    pub fn name_records(&self) -> Vec<(u16, String)> {
        let mut out: Vec<(u16, String)> = vec![
            (1, self.family.to_string()),
            (2, "Regular".to_string()),
            (3, format!("{} Regular", self.family)),
            (4, format!("{} Regular", self.family)),
            (5, "Version 1.000".to_string()),
            (6, format!("{}-Regular", self.postscript)),
            (25, self.postscript.to_string()),
        ];
        for (axis, name) in self.axes.iter().zip(&self.axis_names) {
            out.push((axis.name_id, (*name).to_string()));
        }
        for named in &self.instances {
            out.push((named.record.subfamily_name_id, named.subfamily.to_string()));
            if let Some(id) = named.record.postscript_name_id {
                out.push((id, named.postscript.to_string()));
            }
        }
        out
    }

    /// The face's bytes.
    #[allow(clippy::too_many_lines, reason = "every table of one face")]
    pub fn build(&self) -> Vec<u8> {
        let count = self.glyphs.len();
        let glyph_count = u16::try_from(count).expect("few glyphs");
        let boxes: Vec<Option<[i32; 4]>> = (0..count).map(|g| self.bounds(g)).collect();
        let word = |v: i32| i16::try_from(v).expect("a word");

        let bytes: Vec<Vec<u8>> = (0..count).map(|g| self.glyph_bytes(g)).collect();
        let (glyf, loca) = glyf_and_short_loca(&bytes);

        // hmtx: each advance and each left side bearing its glyph's xMin.
        let horizontal: Vec<(u16, i16)> = (0..count)
            .map(|g| {
                let lsb = boxes[g].map_or(0, |b| word(b[0]));
                (self.glyphs[self.metrics_glyph(g)].advance, lsb)
            })
            .collect();
        // vmtx: one advance height, each top side bearing measured from the
        // shared vertical origin down to the glyph's top.
        let vertical: Vec<(u16, i16)> = (0..count)
            .map(|g| {
                let top = boxes[g].map_or(0, |b| b[3]);
                (self.vertical_advance, word(self.vertical_origin - top))
            })
            .collect();

        let drawn: Vec<(usize, [i32; 4])> = boxes
            .iter()
            .enumerate()
            .filter_map(|(g, b)| b.map(|b| (g, b)))
            .collect();
        let union = [
            drawn.iter().map(|(_, b)| b[0]).min().unwrap_or(0),
            drawn.iter().map(|(_, b)| b[1]).min().unwrap_or(0),
            drawn.iter().map(|(_, b)| b[2]).max().unwrap_or(0),
            drawn.iter().map(|(_, b)| b[3]).max().unwrap_or(0),
        ];
        let advance_max = horizontal.iter().map(|m| m.0).max().unwrap_or(0);
        let min_lsb = drawn
            .iter()
            .map(|(g, _)| i32::from(horizontal[*g].1))
            .min()
            .unwrap_or(0);
        let min_rsb = drawn
            .iter()
            .map(|(g, b)| i32::from(horizontal[*g].0) - b[2])
            .min()
            .unwrap_or(0);
        let x_extent = drawn
            .iter()
            .map(|(g, b)| i32::from(horizontal[*g].1) + (b[2] - b[0]))
            .max()
            .unwrap_or(0);
        let min_tsb = drawn
            .iter()
            .map(|(g, _)| i32::from(vertical[*g].1))
            .min()
            .unwrap_or(0);
        let min_bsb = drawn
            .iter()
            .map(|(g, b)| i32::from(vertical[*g].0) - (i32::from(vertical[*g].1) + (b[3] - b[1])))
            .min()
            .unwrap_or(0);
        let y_extent = drawn
            .iter()
            .map(|(g, b)| i32::from(vertical[*g].1) + (b[3] - b[1]))
            .max()
            .unwrap_or(0);

        let advances: Vec<i32> = horizontal
            .iter()
            .map(|m| i32::from(m.0))
            .filter(|a| *a != 0)
            .collect();
        let sum: i32 = advances.iter().sum();
        let average = (2 * sum + i32::try_from(advances.len()).expect("few"))
            .div_euclid(2 * i32::try_from(advances.len()).expect("few"));
        let mut os2 = self.os2;
        os2.average_width = word(average);

        let simple_points = (0..count)
            .filter(|g| matches!(self.glyphs[*g].shape, Shape::Simple(_)))
            .map(|g| self.items(g))
            .max()
            .unwrap_or(0);
        let simple_contours = self
            .glyphs
            .iter()
            .filter_map(|g| match &g.shape {
                Shape::Simple(contours) => Some(contours.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let composites: Vec<usize> = (0..count)
            .filter(|g| matches!(self.glyphs[*g].shape, Shape::Composite(_)))
            .collect();
        let contours_of = |g: usize| -> usize {
            match &self.glyphs[g].shape {
                Shape::Simple(contours) => contours.len(),
                _ => 0,
            }
        };
        let composite_points = composites
            .iter()
            .map(|g| self.placed_points(*g, UNPLACED).len())
            .max()
            .unwrap_or(0);
        let composite_contours = composites
            .iter()
            .map(|g| match &self.glyphs[*g].shape {
                Shape::Composite(components) => components
                    .iter()
                    .map(|c| contours_of(usize::from(c.glyph)))
                    .sum(),
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        let component_elements = composites.iter().map(|g| self.items(*g)).max().unwrap_or(0);
        let narrow = |v: usize| u16::try_from(v).expect("a small count");
        let maxp = maxp_table(&MaxpSpec {
            glyphs: glyph_count,
            points: narrow(simple_points),
            contours: narrow(simple_contours),
            composite_points: narrow(composite_points),
            composite_contours: narrow(composite_contours),
            zones: 2,
            twilight_points: 0,
            storage: 0,
            function_defs: 1,
            instruction_defs: 0,
            stack_elements: 16,
            size_of_instructions: 0,
            component_elements: narrow(component_elements),
            component_depth: 1,
        });

        let names: Vec<PostName> = self.glyphs.iter().map(|g| g.name).collect();
        let pairs: Vec<(u16, u16)> = self
            .glyphs
            .iter()
            .enumerate()
            .filter_map(|(g, glyph)| {
                glyph
                    .unicode
                    .map(|code| (code, u16::try_from(g).expect("few glyphs")))
            })
            .collect();
        let records = self.name_records();
        let name_refs: Vec<(u16, &str)> = records.iter().map(|(id, s)| (*id, s.as_str())).collect();
        let fvar_records: Vec<InstanceSpec> =
            self.instances.iter().map(|n| n.record.clone()).collect();
        let stat_axes: Vec<(Tag, u16, u16)> = self
            .axes
            .iter()
            .enumerate()
            .map(|(i, axis)| (axis.tag, axis.name_id, u16::try_from(i).expect("few")))
            .collect();
        let variations: Vec<Option<GlyphVariation>> =
            self.glyphs.iter().map(|g| g.variation.clone()).collect();
        let axis_count = u16::try_from(self.axes.len()).expect("few axes");
        let mvar_store = item_store(axis_count, &self.mvar_regions, &self.mvar_data);

        let face = Face::truetype()
            .with(*b"HVAR", self.hvar_table())
            .with(*b"MVAR", mvar(&mvar_store, &self.mvar_records))
            .with(*b"OS/2", os2_table(&os2))
            .with(*b"STAT", stat_table(&stat_axes, 2))
            .with(*b"VVAR", self.vvar_table())
            .with(*b"avar", avar(&self.avar))
            .with(*b"cmap", cmap_table(&pairs))
            .with(*b"cvar", cvar(self.cvar_shared_points.as_ref(), &self.cvar))
            .with(*b"cvt ", cvt(&self.cvt))
            .with(*b"fpgm", self.fpgm.clone())
            .with(*b"fvar", fvar(&self.axes, &fvar_records))
            .with(*b"gasp", gasp(&self.gasp))
            .with(*b"glyf", glyf)
            .with(
                *b"gvar",
                gvar(axis_count, &self.shared_tuples, &variations, false),
            )
            .with(
                *b"head",
                head_table(&HeadSpec {
                    revision: 0x0001_0000,
                    flags: 0x000B,
                    units_per_em: 1_000,
                    bounds: union.map(word),
                    mac_style: 0,
                    lowest_rec_ppem: 8,
                    long_loca: false,
                }),
            )
            .with(
                *b"hhea",
                metrics_header(&MetricsHeader {
                    version: 0x0001_0000,
                    lines: self.horizontal_lines,
                    advance_max,
                    min_start: word(min_lsb),
                    min_end: word(min_rsb),
                    max_extent: word(x_extent),
                    caret: (1, 0, 0),
                    long_metrics: glyph_count,
                }),
            )
            .with(*b"hmtx", metrics(&horizontal, &[]))
            .with(*b"loca", loca)
            .with(*b"maxp", maxp)
            .with(*b"name", name_table(&name_refs))
            .with(
                *b"post",
                post_table(0, self.underline, Some(names.as_slice())),
            )
            .with(*b"prep", self.prep.clone())
            .with(
                *b"vhea",
                metrics_header(&MetricsHeader {
                    version: 0x0001_1000,
                    lines: self.vertical_lines,
                    advance_max: self.vertical_advance,
                    min_start: word(min_tsb),
                    min_end: word(min_bsb),
                    max_extent: word(y_extent),
                    caret: (0, 1, 0),
                    long_metrics: glyph_count,
                }),
            )
            .with(*b"vmtx", metrics(&vertical, &[]));
        finish_face(face.build())
    }
}

/// A `blend` as a charstring states it: the defaults, each default's deltas
/// in region order, the count, and the operator (CFF2, 'Variation
/// operators').
pub fn blend_items(defaults: &[i32], deltas: &[&[i32]]) -> Vec<Cs> {
    assert_eq!(defaults.len(), deltas.len(), "a delta list a default");
    let mut out: Vec<Cs> = ints(defaults);
    for list in deltas {
        out.extend(ints(list));
    }
    out.push(Cs::Int(i32::try_from(defaults.len()).expect("few")));
    out.push(Cs::Op(op::BLEND));
    out
}

/// A DICT `blend` (CFF2, 'Private DICT Data'): the operands as
/// [`blend_items`] orders them, `blend` (23), then the operator the results
/// are read by.
pub fn dict_blend(defaults: &[i32], deltas: &[&[i32]], operator: u16) -> Vec<u8> {
    let mut operands: Vec<i32> = defaults.to_vec();
    for list in deltas {
        operands.extend_from_slice(list);
    }
    operands.push(i32::try_from(defaults.len()).expect("few"));
    let mut out = dict_entry(23, &operands);
    out.extend(dict_entry(operator, &[]));
    out
}

/// One glyph of the committed CFF2 face.
#[derive(Clone, Debug)]
pub struct Cff2Glyph {
    pub unicode: Option<u16>,
    /// Its charstring.
    pub charstring: Vec<u8>,
    /// Its advance at the default instance.
    pub advance: u16,
    /// Its outline's `xMin` at the default instance, which is its left side
    /// bearing.
    pub left: i16,
    /// `HVAR`'s advance deltas for it, one a region of `hvar_regions`.
    pub advance_deltas: Vec<i32>,
}

/// The committed CFF2 face: two axes, five named instances, a `CFF2` table
/// with two Font DICTs and an FDSelect, two item variation data subtables
/// that its glyphs and Private DICTs switch between, blends in charstrings,
/// in local and global subroutines and in Private DICTs — one global
/// subroutine blending under whichever `vsindex` its caller has — hints and
/// masks, and an operator with more operands than a Type 2 charstring holds;
/// `HVAR` mapping advances by glyph id, and `MVAR`.
#[derive(Clone, Debug)]
pub struct Cff2Fixture {
    pub family: &'static str,
    pub postscript: &'static str,
    pub axes: Vec<AxisSpec>,
    pub axis_names: Vec<&'static str>,
    pub instances: Vec<NamedSpec>,
    /// The CFF2 store's regions.
    pub regions: Vec<RegionSpec>,
    /// Each item variation data subtable's regions, by index into
    /// `regions`.
    pub data: Vec<Vec<u16>>,
    pub glyphs: Vec<Cff2Glyph>,
    pub global_subrs: Vec<Vec<u8>>,
    pub privates: Vec<PrivateSpec>,
    pub fd_select: FdSelectSpec,
    /// `HVAR`'s store's regions; its one subtable has a row a glyph, read by
    /// glyph id.
    pub hvar_regions: Vec<RegionSpec>,
    pub mvar_regions: Vec<RegionSpec>,
    /// `(tag, deltas a region of mvar_regions)`, in binary tag order, each
    /// its own row of the one subtable.
    pub mvar_rows: Vec<(Tag, Vec<i32>)>,
    pub horizontal_lines: (i16, i16, i16),
    pub os2: Os2Spec,
    pub underline: (i16, i16),
    /// The union of the glyphs' boxes at the default instance.
    pub bounds: [i16; 4],
}

/// The delta for one value of the dot's curves at the heaviest weight: a
/// third of it, so the dot grows by a third.
fn grow(v: i32) -> i32 {
    v / 3
}

/// The statement of the committed CFF2 face.
#[allow(clippy::too_many_lines, reason = "one face, stated in one place")]
pub fn cff2_fixture() -> Cff2Fixture {
    use Cs::{Bytes, Int, Op};
    let axes = vec![
        AxisSpec::new(*b"wght", 200.0, 400.0, 900.0, 256),
        AxisSpec::new(*b"wdth", 75.0, 100.0, 125.0, 257),
    ];
    let instance = |id: u16, coordinates: [f64; 2], postscript: u16| {
        InstanceSpec::new(id, &coordinates, postscript)
    };
    let instances = vec![
        NamedSpec {
            record: instance(258, [200.0, 100.0], 263),
            subfamily: "Light",
            postscript: "TinkerVarCff2-Light",
        },
        NamedSpec {
            record: instance(259, [400.0, 100.0], 264),
            subfamily: "Regular",
            postscript: "TinkerVarCff2-Regular",
        },
        NamedSpec {
            record: instance(260, [700.0, 100.0], 265),
            subfamily: "Bold",
            postscript: "TinkerVarCff2-Bold",
        },
        NamedSpec {
            record: instance(261, [400.0, 75.0], 266),
            subfamily: "Condensed",
            postscript: "TinkerVarCff2-Condensed",
        },
        NamedSpec {
            record: instance(262, [900.0, 125.0], 267),
            subfamily: "Wide Black",
            postscript: "TinkerVarCff2-WideBlack",
        },
    ];
    // R0 the heaviest weight, R1 the lightest, R2 the narrowest width, R3
    // the widest; R4, the heavy and narrow corner, only `HVAR` reads.
    let r0 = vec![(0, ONE, ONE), NONE];
    let r1 = vec![(-ONE, -ONE, 0), NONE];
    let r2 = vec![NONE, (-ONE, -ONE, 0)];
    let r3 = vec![NONE, (0, ONE, ONE)];
    let r4 = vec![(0, ONE, ONE), (-ONE, -ONE, 0)];
    let regions = vec![r0.clone(), r1.clone(), r2.clone(), r3.clone()];
    // vsindex 0 blends over the two weights and the narrow width (k = 3);
    // vsindex 1 over the heavy weight and the wide width (k = 2).
    let data = vec![vec![0, 1, 2], vec![0, 3]];

    let call = |subr: i32, operator: u8| vec![Int(subr), Op(operator)];
    let cs = |parts: Vec<Vec<Cs>>| -> Vec<u8> { charstring(&parts.concat()) };

    // .notdef (Font DICT 0, k = 3): a box, and a hole whose left side is
    // drawn by global subroutine 1 — `2 blend rlineto`, operands from the
    // caller — and whose other sides are one rlineto of fifty operands, more
    // than the 48 a Type 2 charstring holds.
    let mut zigzag: Vec<Cs> = Vec::new();
    for _ in 0..12 {
        zigzag.extend(ints(&[12, 10, 13, -10]));
    }
    let notdef = cs(vec![
        ints(&[50, 0]),
        vec![Op(op::RMOVETO)],
        ints(&[400, 700, -400]),
        vec![Op(op::HLINETO)],
        blend_items(&[50, -650], &[&[30, -20, 0], &[30, -20, 0]]),
        vec![Op(op::RMOVETO)],
        ints(&[0, 600, 0, 0, 0, -60, 40, 0]),
        call(1 - 107, op::CALLGSUBR),
        zigzag,
        blend_items(&[0, -600], &[&[0, 0, 0], &[60, -40, 0]]),
        vec![Op(op::RLINETO)],
    ]);

    // The dot's four curves, clockwise from its left: fixed in global
    // subroutine 0, and blended in Font DICT 0's local subroutine 0 — larger
    // at the heaviest weight, smaller at the lightest, narrower at the
    // narrowest width.
    let dot_values = [
        0, 33, 27, 27, 33, 0, 33, 0, 27, -27, 0, -33, 0, -33, -27, -27, -33, 0, -33, 0, -27, 27, 0,
        33,
    ];
    let dot_deltas: Vec<Vec<i32>> = dot_values
        .iter()
        .enumerate()
        .map(|(at, v)| {
            let horizontal = at % 2 == 0;
            let narrow = if horizontal { -(v / 5) } else { 0 };
            vec![grow(*v), -(v * 2 / 11), narrow]
        })
        .collect();
    let dot_delta_refs: Vec<&[i32]> = dot_deltas.iter().map(Vec::as_slice).collect();
    let static_dot = cs(vec![ints(&dot_values), vec![Op(op::RRCURVETO)]]);
    let blended_dot = cs(vec![
        blend_items(&dot_values, &dot_delta_refs),
        vec![Op(op::RRCURVETO)],
    ]);

    // period (Font DICT 0): its blended start, then the blended dot.
    let period = cs(vec![
        blend_items(&[90, 60], &[&[-20, 10, 10], &[0, 0, 0]]),
        vec![Op(op::RMOVETO)],
        call(-107, op::CALLSUBR),
    ]);

    // colon (Font DICT 1, k = 2): the fixed dot twice, the second lifted
    // further at the heaviest weight.
    let colon = cs(vec![
        ints(&[90, 60]),
        vec![Op(op::RMOVETO)],
        call(-107, op::CALLGSUBR),
        blend_items(&[0, 400], &[&[0, 0], &[30, 0]]),
        vec![Op(op::RMOVETO)],
        call(-107, op::CALLGSUBR),
    ]);

    // H (Font DICT 1): a horizontal stem and two vertical ones, a hint mask
    // and a counter mask over them, both stems drawn by Font DICT 1's local
    // subroutine 0, and the bar.
    let stem_subr = cs(vec![
        blend_items(&[100, 700, -100], &[&[60, 0], &[0, 0], &[-60, 0]]),
        vec![Op(op::HLINETO)],
    ]);
    let h = cs(vec![
        blend_items(&[310, 80], &[&[-10, 0], &[20, 0]]),
        vec![Op(op::HSTEMHM)],
        blend_items(
            &[100, 100, 350, 100],
            &[&[0, 0], &[60, 0], &[0, 40], &[60, 0]],
        ),
        vec![Op(op::VSTEMHM)],
        vec![Op(op::HINTMASK), Bytes(vec![0xE0])],
        vec![Op(20), Bytes(vec![0x60])], // cntrmask
        ints(&[100, 0]),
        vec![Op(op::RMOVETO)],
        call(-107, op::CALLSUBR),
        blend_items(&[450, -700], &[&[60, 40], &[0, 0]]),
        vec![Op(op::RMOVETO)],
        call(-107, op::CALLSUBR),
        blend_items(&[-350, -390], &[&[0, -40], &[-10, 0]]),
        vec![Op(op::RMOVETO)],
        blend_items(&[350, 80, -350], &[&[0, 40], &[20, 0], &[0, -40]]),
        vec![Op(op::HLINETO)],
    ]);

    // I (Font DICT 1): its stem hinted by a hint mask's implicit vstem, its
    // first side drawn through the same global subroutine `.notdef` uses —
    // which blends there under vsindex 0 and here under vsindex 1.
    let i = cs(vec![
        blend_items(&[100, 100], &[&[0, 0], &[60, 0]]),
        vec![Op(op::HINTMASK), Bytes(vec![0x80])],
        ints(&[100, 0]),
        vec![Op(op::RMOVETO)],
        ints(&[100, 0, 60, 0, 0, 0]),
        call(1 - 107, op::CALLGSUBR),
        blend_items(&[0, 700, -100, 0], &[&[0, 0], &[0, 0], &[-60, 0], &[0, 0]]),
        vec![Op(op::RLINETO)],
    ]);

    // O (Font DICT 1, but `0 vsindex` selects the three-region data): two
    // hints, the outer ring's sixteen operands blended for the narrow width,
    // the inner ring moved by a blended start.
    let outer = [
        -198, 146, -162, 179, 179, 146, 162, 198, 198, -146, 162, -179, -179, -146, -162, -198,
    ];
    let outer_deltas: Vec<Vec<i32>> = outer
        .iter()
        .enumerate()
        .map(|(at, v)| {
            // vhcurveto alternates: a vertical-first curve's x operands are
            // its second and fourth, a horizontal-first curve's its first
            // and second.
            let group = at / 4;
            let place = at % 4;
            let horizontal = if group % 2 == 0 {
                place == 1 || place == 3
            } else {
                place == 0 || place == 1
            };
            vec![0, 0, if horizontal { -(v / 10) } else { 0 }]
        })
        .collect();
    let outer_refs: Vec<&[i32]> = outer_deltas.iter().map(Vec::as_slice).collect();
    let inner = [
        143, 101, 117, 124, 124, 101, -117, -143, -143, -101, -117, -124, -124, -101, 117, 143,
    ];
    let o = cs(vec![
        ints(&[0]),
        vec![Op(op::VSINDEX)],
        ints(&[-10, 20, 680, 20]),
        vec![Op(op::HSTEM)],
        ints(&[50, 350]),
        vec![Op(op::RMOVETO)],
        blend_items(&outer, &outer_refs),
        vec![Op(30)], // vhcurveto
        blend_items(&[100, 0], &[&[40, -30, -10], &[0, 0, 0]]),
        vec![Op(op::RMOVETO)],
        ints(&inner),
        vec![Op(30)],
    ]);

    let glyph =
        |unicode: Option<u16>, charstring: Vec<u8>, advance: u16, left: i16, deltas: [i32; 5]| {
            Cff2Glyph {
                unicode,
                charstring,
                advance,
                left,
                advance_deltas: deltas.to_vec(),
            }
        };
    let glyphs = vec![
        glyph(None, notdef, 500, 50, [0, 0, 0, 0, 0]),
        glyph(Some(0x0020), Vec::new(), 250, 0, [20, -20, -50, 50, 0]),
        glyph(Some(0x002E), period, 300, 90, [40, -30, -30, 30, 0]),
        glyph(Some(0x003A), colon, 300, 90, [40, -30, -30, 30, 0]),
        glyph(Some(0x0048), h, 750, 100, [120, -120, -100, 100, -20]),
        glyph(Some(0x0049), i, 300, 100, [60, -60, -60, 40, -10]),
        glyph(Some(0x004F), o, 750, 50, [0, 0, -80, 80, 0]),
    ];

    // Font DICT 0's Private DICT blends its top alignment zone and its
    // standard stems under vsindex 0; Font DICT 1's selects vsindex 1 and
    // blends its zone, a stem snap pair and its vertical stem under it.
    let first_private = [
        dict_blend(
            &[-10, 10, 700, 10],
            &[&[0, 0, 0], &[0, 0, 0], &[10, -10, 0], &[0, 0, 0]],
            6,
        ),
        dict_blend(&[60], &[&[30, -20, 0]], 10),
        dict_blend(&[100], &[&[60, -60, -20]], 11),
    ]
    .concat();
    let second_private = [
        dict_entry(22, &[1]),
        dict_blend(
            &[-10, 10, 700, 10],
            &[&[0, 0], &[0, 0], &[10, 0], &[0, 0]],
            6,
        ),
        dict_blend(&[60, 90], &[&[30, 0], &[40, 10]], 0x0C0C),
        dict_blend(&[100], &[&[60, 20]], 11),
    ]
    .concat();

    Cff2Fixture {
        family: "Tinker Var Cff2",
        postscript: "TinkerVarCff2",
        axes,
        axis_names: vec!["Weight", "Width"],
        instances,
        regions,
        data,
        glyphs,
        global_subrs: vec![
            static_dot,
            charstring(&[Int(2), Op(op::BLEND), Op(op::RLINETO)]),
        ],
        privates: vec![
            PrivateSpec {
                dict: first_private,
                subrs: vec![blended_dot],
            },
            PrivateSpec {
                dict: second_private,
                subrs: vec![stem_subr],
            },
        ],
        fd_select: FdSelectSpec::Format3(vec![(0, 0), (3, 1)], 7),
        hvar_regions: vec![r0.clone(), r1.clone(), r2.clone(), r3.clone(), r4],
        mvar_regions: vec![r0, r1, r2, r3],
        mvar_rows: vec![
            (*b"cpht", vec![10, -5, 0, 0]),
            (*b"hasc", vec![20, 0, 0, 0]),
            (*b"hdsc", vec![-10, 0, 0, 0]),
            (*b"stro", vec![10, -5, 0, 0]),
            (*b"strs", vec![30, -20, 0, 0]),
            (*b"undo", vec![-10, 5, 0, 0]),
            (*b"unds", vec![30, -20, 0, 0]),
            (*b"xhgt", vec![10, -5, 0, 0]),
        ],
        horizontal_lines: (800, -200, 200),
        os2: Os2Spec {
            average_width: 0,
            weight_class: 400,
            width_class: 5,
            subscript: [650, 600, 0, 75],
            superscript: [650, 600, 0, 350],
            strikeout: (50, 300),
            unicode_ranges: [1, 0, 0, 0],
            vendor: *b"NONE",
            selection: 0x0040,
            char_range: (0x0020, 0x004F),
            typo: (800, -200, 200),
            win: (950, 250),
            code_pages: [1, 0],
            x_height: 500,
            cap_height: 700,
            default_char: 0,
            break_char: 0x0020,
            max_context: 0,
        },
        underline: (-100, 50),
        bounds: [50, -10, 700, 710],
    }
}

impl Cff2Fixture {
    /// The `CFF2` table.
    pub fn cff2_table(&self) -> Vec<u8> {
        let axis_count = u16::try_from(self.axes.len()).expect("few axes");
        let data: Vec<Option<DataSpec>> = self
            .data
            .iter()
            .map(|regions| {
                Some(DataSpec {
                    regions: regions.clone(),
                    word_count: 0,
                    long_words: false,
                    rows: Vec::new(),
                })
            })
            .collect();
        Cff2Spec {
            charstrings: self.glyphs.iter().map(|g| g.charstring.clone()).collect(),
            global_subrs: self.global_subrs.clone(),
            privates: self.privates.clone(),
            fd_select: Some(self.fd_select.clone()),
            store: Some(item_store(axis_count, &self.regions, &data)),
        }
        .build()
    }

    /// The `name` table's records, as the `glyf` face states its own.
    pub fn name_records(&self) -> Vec<(u16, String)> {
        let mut out: Vec<(u16, String)> = vec![
            (1, self.family.to_string()),
            (2, "Regular".to_string()),
            (3, format!("{} Regular", self.family)),
            (4, format!("{} Regular", self.family)),
            (5, "Version 1.000".to_string()),
            (6, format!("{}-Regular", self.postscript)),
            (25, self.postscript.to_string()),
        ];
        for (axis, name) in self.axes.iter().zip(&self.axis_names) {
            out.push((axis.name_id, (*name).to_string()));
        }
        for named in &self.instances {
            out.push((named.record.subfamily_name_id, named.subfamily.to_string()));
            if let Some(id) = named.record.postscript_name_id {
                out.push((id, named.postscript.to_string()));
            }
        }
        out
    }

    /// The face's bytes.
    pub fn build(&self) -> Vec<u8> {
        let count = u16::try_from(self.glyphs.len()).expect("few glyphs");
        let axis_count = u16::try_from(self.axes.len()).expect("few axes");
        let horizontal: Vec<(u16, i16)> = self.glyphs.iter().map(|g| (g.advance, g.left)).collect();
        let hvar_store = item_store(
            axis_count,
            &self.hvar_regions,
            &[Some(DataSpec {
                regions: (0..u16::try_from(self.hvar_regions.len()).expect("few")).collect(),
                word_count: 0,
                long_words: false,
                rows: self
                    .glyphs
                    .iter()
                    .map(|g| g.advance_deltas.clone())
                    .collect(),
            })],
        );
        let mvar_store = item_store(
            axis_count,
            &self.mvar_regions,
            &[Some(DataSpec {
                regions: (0..u16::try_from(self.mvar_regions.len()).expect("few")).collect(),
                word_count: 0,
                long_words: false,
                rows: self.mvar_rows.iter().map(|(_, row)| row.clone()).collect(),
            })],
        );
        let records: Vec<(Tag, u16, u16)> = self
            .mvar_rows
            .iter()
            .enumerate()
            .map(|(row, (tag, _))| (*tag, 0, u16::try_from(row).expect("few")))
            .collect();
        let pairs: Vec<(u16, u16)> = self
            .glyphs
            .iter()
            .enumerate()
            .filter_map(|(g, glyph)| {
                glyph
                    .unicode
                    .map(|code| (code, u16::try_from(g).expect("few glyphs")))
            })
            .collect();
        let names = self.name_records();
        let name_refs: Vec<(u16, &str)> = names.iter().map(|(id, s)| (*id, s.as_str())).collect();
        let fvar_records: Vec<InstanceSpec> =
            self.instances.iter().map(|n| n.record.clone()).collect();
        let stat_axes: Vec<(Tag, u16, u16)> = self
            .axes
            .iter()
            .enumerate()
            .map(|(i, axis)| (axis.tag, axis.name_id, u16::try_from(i).expect("few")))
            .collect();

        let advances: Vec<i32> = horizontal
            .iter()
            .map(|m| i32::from(m.0))
            .filter(|a| *a != 0)
            .collect();
        let sum: i32 = advances.iter().sum();
        let n = i32::try_from(advances.len()).expect("few");
        let mut os2 = self.os2;
        os2.average_width = i16::try_from((2 * sum + n).div_euclid(2 * n)).expect("a word");

        // Every glyph but the space draws; the extents are its box against
        // its advance, the box's left side its left side bearing.
        let rights: [i16; 7] = [450, 0, 210, 210, 650, 200, 700];
        let drawn: Vec<usize> = (0..self.glyphs.len())
            .filter(|g| !self.glyphs[*g].charstring.is_empty())
            .collect();
        let advance_max = horizontal.iter().map(|m| m.0).max().unwrap_or(0);
        let min_lsb = drawn.iter().map(|g| horizontal[*g].1).min().unwrap_or(0);
        let min_rsb = drawn
            .iter()
            .map(|g| {
                i16::try_from(i32::from(horizontal[*g].0) - i32::from(rights[*g])).expect("a word")
            })
            .min()
            .unwrap_or(0);
        let x_extent = drawn.iter().map(|g| rights[*g]).max().unwrap_or(0);

        let mut maxp = Vec::new();
        push32(&mut maxp, 0x0000_5000);
        push16(&mut maxp, count);

        let face = Face::opentype()
            .with(*b"CFF2", self.cff2_table())
            .with(*b"HVAR", hvar(&hvar_store, None, None, None))
            .with(*b"MVAR", mvar(&mvar_store, &records))
            .with(*b"OS/2", os2_table(&os2))
            .with(*b"STAT", stat_table(&stat_axes, 2))
            .with(*b"cmap", cmap_table(&pairs))
            .with(*b"fvar", fvar(&self.axes, &fvar_records))
            .with(
                *b"head",
                head_table(&HeadSpec {
                    revision: 0x0001_0000,
                    flags: 0x0003,
                    units_per_em: 1_000,
                    bounds: self.bounds,
                    mac_style: 0,
                    lowest_rec_ppem: 8,
                    long_loca: false,
                }),
            )
            .with(
                *b"hhea",
                metrics_header(&MetricsHeader {
                    version: 0x0001_0000,
                    lines: self.horizontal_lines,
                    advance_max,
                    min_start: min_lsb,
                    min_end: min_rsb,
                    max_extent: x_extent,
                    caret: (1, 0, 0),
                    long_metrics: count,
                }),
            )
            .with(*b"hmtx", metrics(&horizontal, &[]))
            .with(*b"maxp", maxp)
            .with(*b"name", name_table(&name_refs))
            .with(*b"post", post_table(0, self.underline, None));
        finish_face(face.build())
    }
}
