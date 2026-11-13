//! The face assembler the column tests share: `vertical.rs`, which holds a
//! column to the tables a test wrote, and `fingerprints.rs`, which hashes
//! columns shaped through such faces on every target
//! (`docs/design/shaping.md`, *Vertical shaping*) — and `synthetic.rs`, whose
//! per-cluster confinement test shapes a Balinese run through one.
//!
//! Every face is written here byte by byte, so the `wasm32-wasip1` run, which
//! has no filesystem, builds the same faces as every other target.

#![allow(
    dead_code,
    reason = "shared by three test binaries; each uses a different subset"
)]

use tinker_pdf_shape::{ShapedGlyph, VerticalMetrics};

// --- a face assembler ----------------------------------------------------
//
// In `tests/synthetic.rs`'s register: every OpenType Layout table is a header
// of counts and `Offset16`s followed by the things they point at, so
// `assemble` appends the parts and patches each slot with where its part
// landed. The face around the tables is the smallest sfnt the shaper reads a
// column from: `cmap`, `head`, `hhea`, `hmtx`, `maxp`, and as each test asks,
// `vhea` and `vmtx`, `glyf` and `loca` or a `VORG`, and the layout tables.

pub fn u16s(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// A header, with `slots[i]` patched to the offset of `parts[i]`.
pub fn assemble(mut head: Vec<u8>, slots: &[usize], parts: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(slots.len(), parts.len(), "a slot for every part");
    let mut body: Vec<u8> = Vec::new();
    for (slot, part) in slots.iter().zip(parts) {
        let offset = u16::try_from(head.len() + body.len()).expect("a table under 64 KB");
        head[*slot..*slot + 2].copy_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(part);
    }
    head.extend(body);
    head
}

/// Coverage format 1: the glyphs, sorted, as the format requires.
pub fn coverage(glyphs: &[u16]) -> Vec<u8> {
    let mut sorted = glyphs.to_vec();
    sorted.sort_unstable();
    let mut out = u16s(&[1, u16::try_from(sorted.len()).expect("a small coverage")]);
    out.extend(u16s(&sorted));
    out
}

/// One lookup of `kind`, its subtables in order.
pub fn lookup(kind: u16, flags: u16, subtables: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(subtables.len()).expect("a small lookup");
    let mut head = u16s(&[kind, flags, count]);
    head.extend(vec![0u8; subtables.len() * 2]);
    let slots: Vec<usize> = (0..subtables.len()).map(|i| 6 + i * 2).collect();
    assemble(head, &slots, subtables)
}

/// `SingleSubstFormat2`: each covered glyph's replacement, in coverage order.
pub fn single_subst(map: &[(u16, u16)]) -> Vec<u8> {
    let mut sorted = map.to_vec();
    sorted.sort_unstable();
    let glyphs: Vec<u16> = sorted.iter().map(|(from, _)| *from).collect();
    let subs: Vec<u16> = sorted.iter().map(|(_, to)| *to).collect();
    let mut head = u16s(&[2, 0, u16::try_from(subs.len()).expect("a small subst")]);
    head.extend(u16s(&subs));
    assemble(head, &[2], &[coverage(&glyphs)])
}

/// `LigatureSubstFormat1` of one ligature: `first` then `rest` become `into`.
pub fn ligature_subst(first: u16, rest: &[u16], into: u16) -> Vec<u8> {
    let components = u16::try_from(rest.len() + 1).expect("a small ligature");
    let mut ligature = u16s(&[into, components]);
    ligature.extend(u16s(rest));
    let set = assemble(u16s(&[1, 0]), &[2], &[ligature]);
    assemble(u16s(&[1, 0, 1, 0]), &[2, 6], &[coverage(&[first]), set])
}

/// A `GSUB` or `GPOS`: each script's default language system names every
/// feature, and each feature names the lookups given, by index.
pub fn layout_table(
    scripts: &[[u8; 4]],
    features: &[([u8; 4], Vec<u16>)],
    lookups: &[Vec<u8>],
) -> Vec<u8> {
    let feature_count = u16::try_from(features.len()).expect("a small feature list");
    let mut lang_sys = u16s(&[0, 0xFFFF, feature_count]);
    lang_sys.extend(u16s(&(0..feature_count).collect::<Vec<u16>>()));
    let script = assemble(u16s(&[0, 0]), &[0], &[lang_sys]);

    let mut script_head = u16s(&[u16::try_from(scripts.len()).expect("a small script list")]);
    let mut script_slots = Vec::new();
    for tag in scripts {
        script_head.extend_from_slice(tag);
        script_slots.push(script_head.len());
        script_head.extend(u16s(&[0]));
    }
    let script_list = assemble(script_head, &script_slots, &vec![script; scripts.len()]);

    let mut feature_head = u16s(&[feature_count]);
    let mut feature_slots = Vec::new();
    let mut feature_tables = Vec::new();
    for (tag, indices) in features {
        feature_head.extend_from_slice(tag);
        feature_slots.push(feature_head.len());
        feature_head.extend(u16s(&[0]));
        let mut table = u16s(&[0, u16::try_from(indices.len()).expect("a small feature")]);
        table.extend(u16s(indices));
        feature_tables.push(table);
    }
    let feature_list = assemble(feature_head, &feature_slots, &feature_tables);

    let mut list_head = u16s(&[u16::try_from(lookups.len()).expect("a small lookup list")]);
    list_head.extend(vec![0u8; lookups.len() * 2]);
    let slots: Vec<usize> = (0..lookups.len()).map(|i| 2 + i * 2).collect();
    let lookup_list = assemble(list_head, &slots, lookups);

    assemble(
        u16s(&[1, 0, 0, 0, 0]),
        &[4, 6, 8],
        &[script_list, feature_list, lookup_list],
    )
}

/// A face for a column, described by what it carries; [`Face::build`] writes
/// the sfnt, and the expectation helpers read the same fields back, so a test
/// computes what it expects from the tables it wrote.
#[derive(Clone, Debug, Default)]
pub struct Face {
    /// Units per em.
    pub em: u16,
    /// Each glyph's horizontal advance, glyph 0 being `.notdef`.
    pub advances: Vec<u16>,
    /// Each character's glyph.
    pub cmap: Vec<(char, u16)>,
    /// Each glyph's `vmtx` entry, advance height and top side bearing; `None`
    /// is a face with no `vhea` and no `vmtx`.
    pub vertical: Option<Vec<(u16, i16)>>,
    /// Each glyph's `yMax` in its `glyf` header; `None` is a face with no
    /// `glyf` and no `loca`.
    pub tops: Option<Vec<i16>>,
    /// `VORG`'s default and records.
    pub vorg: Option<(i16, Vec<(u16, i16)>)>,
    pub gsub: Option<Vec<u8>>,
    pub gpos: Option<Vec<u8>>,
    pub gdef: Option<Vec<u8>>,
}

impl Face {
    /// A face of `glyphs` glyphs at 1000 units per em, each 1000 wide,
    /// mapping `cmap`, with no vertical metrics and no layout tables.
    pub fn new(glyphs: u16, cmap: &[(char, u16)]) -> Face {
        Face {
            em: 1000,
            advances: vec![1000; usize::from(glyphs)],
            cmap: cmap.to_vec(),
            ..Face::default()
        }
    }

    /// The same face with `vmtx` entries and `glyf` tops for every glyph.
    pub fn with_vertical(mut self, metrics: &[(u16, i16)], tops: &[i16]) -> Face {
        assert_eq!(metrics.len(), self.advances.len(), "a vmtx entry per glyph");
        assert_eq!(tops.len(), self.advances.len(), "a glyf top per glyph");
        self.vertical = Some(metrics.to_vec());
        self.tops = Some(tops.to_vec());
        self
    }

    pub fn glyph(&self, c: char) -> u16 {
        self.cmap
            .iter()
            .find(|(of, _)| *of == c)
            .map(|(_, glyph)| *glyph)
            .expect("a character the face maps")
    }

    /// What this face's tables say a glyph stands on in a column, read from
    /// the fields the tables are written from: the `vmtx` clause's origin, and
    /// the synthesized em box where the face has no metrics.
    pub fn expected(&self, glyph: u16) -> VerticalMetrics {
        let at = usize::from(glyph);
        let em = i32::from(self.em);
        let width = i32::from(self.advances.get(at).copied().unwrap_or(0));
        let entry = self.vertical.as_ref().and_then(|metrics| metrics.get(at));
        let origin = match (&self.tops, &self.vorg, entry) {
            (Some(tops), _, Some((_, bearing))) => Some(i32::from(tops[at]) + i32::from(*bearing)),
            (None, Some((default, records)), _) => Some(i32::from(
                records
                    .iter()
                    .find(|(of, _)| *of == glyph)
                    .map_or(*default, |(_, origin)| *origin),
            )),
            _ => None,
        };
        VerticalMetrics {
            advance: entry.map_or(em, |(advance, _)| i32::from(*advance)),
            origin_x: width / 2,
            origin_y: origin.unwrap_or(em * 880 / 1000),
            synthesized: entry.is_none() || origin.is_none(),
        }
    }

    /// The glyph as an upright run gives it to `GPOS`: its vertical
    /// metrics, the vertical origin at the pen.
    pub fn upright(&self, glyph: u16, cluster: u32) -> ShapedGlyph {
        let metrics = self.expected(glyph);
        ShapedGlyph {
            glyph,
            cluster,
            x_advance: 0,
            y_advance: -metrics.advance,
            x_offset: -metrics.origin_x,
            y_offset: -metrics.origin_y,
            inserted: false,
        }
    }

    /// The glyph as a line gives it: its `hmtx` advance and nothing else.
    pub fn along(&self, glyph: u16, cluster: u32) -> ShapedGlyph {
        ShapedGlyph {
            glyph,
            cluster,
            x_advance: i32::from(self.advances[usize::from(glyph)]),
            ..ShapedGlyph::default()
        }
    }

    pub fn build(&self) -> Vec<u8> {
        let glyphs = u16::try_from(self.advances.len()).expect("a small face");
        let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();

        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        head[18..20].copy_from_slice(&self.em.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca
        tables.push((*b"head", head));

        let mut hhea = vec![0u8; 36];
        hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
        tables.push((*b"hhea", hhea));
        let hmtx: Vec<u8> = self
            .advances
            .iter()
            .flat_map(|advance| [advance.to_be_bytes(), 0u16.to_be_bytes()].concat())
            .collect();
        tables.push((*b"hmtx", hmtx));

        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());
        tables.push((*b"maxp", maxp));

        let mut sorted = self.cmap.clone();
        sorted.sort_unstable();
        let groups = u32::try_from(sorted.len()).expect("a small cmap");
        let mut sub = u16s(&[12, 0]);
        sub.extend((16 + groups * 12).to_be_bytes());
        sub.extend(0u32.to_be_bytes());
        sub.extend(groups.to_be_bytes());
        for (c, glyph) in &sorted {
            sub.extend(u32::from(*c).to_be_bytes());
            sub.extend(u32::from(*c).to_be_bytes());
            sub.extend(u32::from(*glyph).to_be_bytes());
        }
        let mut cmap = u16s(&[0, 1, 3, 10]);
        cmap.extend(12u32.to_be_bytes());
        cmap.extend(sub);
        tables.push((*b"cmap", cmap));

        if let Some(metrics) = &self.vertical {
            let mut vhea = vec![0u8; 36];
            vhea[0..4].copy_from_slice(&0x0001_1000u32.to_be_bytes());
            let count = u16::try_from(metrics.len()).expect("a small vmtx");
            vhea[34..36].copy_from_slice(&count.to_be_bytes());
            tables.push((*b"vhea", vhea));
            let vmtx: Vec<u8> = metrics
                .iter()
                .flat_map(|(advance, bearing)| {
                    [advance.to_be_bytes(), bearing.to_be_bytes()].concat()
                })
                .collect();
            tables.push((*b"vmtx", vmtx));
        }
        if let Some(tops) = &self.tops {
            let mut glyf = Vec::new();
            let mut loca = vec![0u32];
            for top in tops {
                glyf.extend(0i16.to_be_bytes()); // no contours: the header is all a box needs
                for edge in [0i16, -120, 1000, *top] {
                    glyf.extend(edge.to_be_bytes());
                }
                loca.push(u32::try_from(glyf.len()).expect("a small glyf"));
            }
            tables.push((*b"glyf", glyf));
            tables.push((
                *b"loca",
                loca.iter().flat_map(|o| o.to_be_bytes()).collect(),
            ));
        }
        if let Some((default, records)) = &self.vorg {
            let mut vorg = u16s(&[1, 0]);
            vorg.extend(default.to_be_bytes());
            vorg.extend(u16s(&[u16::try_from(records.len()).expect("a small VORG")]));
            let mut records = records.clone();
            records.sort_unstable();
            for (glyph, origin) in records {
                vorg.extend(glyph.to_be_bytes());
                vorg.extend(origin.to_be_bytes());
            }
            tables.push((*b"VORG", vorg));
        }
        for (tag, table) in [
            (*b"GDEF", &self.gdef),
            (*b"GSUB", &self.gsub),
            (*b"GPOS", &self.gpos),
        ] {
            if let Some(table) = table {
                tables.push((tag, table.clone()));
            }
        }
        tables.sort_by_key(|(tag, _)| *tag);

        let mut out = Vec::new();
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend(u16s(&[
            u16::try_from(tables.len()).expect("a small directory"),
            0,
            0,
            0,
        ]));
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, table) in &tables {
            out.extend_from_slice(tag);
            out.extend(0u32.to_be_bytes());
            out.extend(u32::try_from(offset).expect("a small face").to_be_bytes());
            out.extend(
                u32::try_from(table.len())
                    .expect("a small table")
                    .to_be_bytes(),
            );
            offset += table.len();
            body.extend_from_slice(table);
        }
        out.extend(body);
        out
    }
}

// --- GPOS ------------------------------------------------------------------

/// An anchor of format 1, in the glyph's own outline coordinates.
pub fn anchor((x, y): (i16, i16)) -> Vec<u8> {
    let mut out = u16s(&[1]);
    out.extend(x.to_be_bytes());
    out.extend(y.to_be_bytes());
    out
}

/// A value record's fields, in the order the format's bits say.
pub fn values(values: &[i16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// `SinglePosFormat1`: one value record of `format` for every glyph covered.
pub fn single_pos(glyphs: &[u16], format: u16, record: &[i16]) -> Vec<u8> {
    let mut head = u16s(&[1, 0, format]);
    head.extend(values(record));
    assemble(head, &[2], &[coverage(glyphs)])
}

/// `PairPosFormat1`: one pair, `first` then `second`, the first glyph's
/// record of `format1` and the second's of `format2`.
pub fn pair_pos(
    first: u16,
    second: u16,
    format1: u16,
    one: &[i16],
    format2: u16,
    two: &[i16],
) -> Vec<u8> {
    let mut pair_set = u16s(&[1, second]);
    pair_set.extend(values(one));
    pair_set.extend(values(two));
    assemble(
        u16s(&[1, 0, format1, format2, 1, 0]),
        &[2, 10],
        &[coverage(&[first]), pair_set],
    )
}

/// `MarkBasePosFormat1` — and `MarkMarkPosFormat1`, which is the same bytes
/// with a mark for the base: one mark of class 0 on one base.
pub fn mark_pos(mark: u16, mark_anchor: (i16, i16), base: u16, base_anchor: (i16, i16)) -> Vec<u8> {
    let mark_array = assemble(u16s(&[1, 0, 0]), &[4], &[anchor(mark_anchor)]);
    let base_array = assemble(u16s(&[1, 0]), &[2], &[anchor(base_anchor)]);
    assemble(
        u16s(&[1, 0, 0, 1, 0, 0]),
        &[2, 4, 8, 10],
        &[coverage(&[mark]), coverage(&[base]), mark_array, base_array],
    )
}

/// `CursivePosFormat1` for two glyphs, `first` below `second` in glyph
/// order: the first glyph's exit anchor and the second's entry anchor.
pub fn cursive_pos(first: u16, exit: (i16, i16), second: u16, entry: (i16, i16)) -> Vec<u8> {
    assert!(first < second, "the records are in coverage order");
    // Record 0 is `first`: entry NULL at 6, exit at 8; record 1 is
    // `second`: entry at 10, exit NULL at 12.
    let head = u16s(&[1, 0, 2, 0, 0, 0, 0]);
    assemble(
        head,
        &[2, 8, 10],
        &[coverage(&[first, second]), anchor(exit), anchor(entry)],
    )
}

/// A `GDEF` whose glyph class definition gives each glyph listed its class.
pub fn gdef(classes: &[(u16, u16)]) -> Vec<u8> {
    let mut sorted = classes.to_vec();
    sorted.sort_unstable();
    let mut class_def = u16s(&[2, u16::try_from(sorted.len()).expect("a small class def")]);
    for (glyph, class) in sorted {
        class_def.extend(u16s(&[glyph, glyph, class]));
    }
    assemble(u16s(&[1, 0, 0, 0, 0, 0]), &[4], &[class_def])
}
