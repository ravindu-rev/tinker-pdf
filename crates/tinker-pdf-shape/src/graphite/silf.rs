//! `Silf`: a Graphite description's rules, and the machine that finds them.
//!
//! A `Silf` holds one or more *descriptions*, each a complete rendering of a
//! set of writing systems: its header, the pseudo-glyph map, the class map
//! its substitutions select from, and its passes. A pass is a finite-state
//! machine over glyph classes that finds the rules matching at a scan
//! position, the code that tests each rule's constraint, and the code each
//! rule's action runs. What is here reads all of it and runs the machine;
//! no rule's code runs yet (`docs/design/shaping.md`, *`Silf`*, *Passes* and
//! *The machine that finds a rule*).
//!
//! # The versions read
//!
//! Major versions 1 to 5, the GTF's version notes deciding which fields a
//! version carries. From 3 the table's header carries the compiler's version
//! and a description its rule version and two offsets; from 2 a description
//! carries its mirroring and skip-pass attributes, its justification levels,
//! a third reserved byte and its critical features, and a pass its
//! `fsmOffset`, its pass constraint, its collision threshold and a reserved
//! byte; and from 4 a class offset is a `ULONG` where before it is a
//! `USHORT`. From 5 the table may be compressed: it arrives here as it reads
//! uncompressed (`super::decompress`).
//!
//! # Offsets the reader does not lean on
//!
//! A description's `passOffset` and `pseudosOffset` are read and not relied
//! on: the reader walks the fields to where the layout puts them. Every
//! vendored face's offsets agree with the walk, `pseudosOffset` naming the
//! pseudo-glyph count where the GTF says the mappings themselves, and
//! `maxGlyphID` is read and bounds nothing — Awami Nastaliq's is smaller than
//! the glyphs its own `Gloc` indexes.

use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;

use super::{Refusal, SILF};

/// A face's `Silf`, read as it reads uncompressed.
#[derive(Clone, Copy, Debug)]
pub struct Silf<'a> {
    data: Bytes<'a>,
    version: u32,
    compiler: Option<u32>,
    count: u16,
    offsets: usize,
}

impl<'a> Silf<'a> {
    /// Reads a `Silf`'s header.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownVersion`] for a major version outside 1 to 5;
    /// [`Refusal::UnknownCompression`] for a version-5 table whose header
    /// says it is compressed still, which a decoded table never does; and
    /// [`Refusal::TableMalformed`] for a table too short for its header or
    /// for the description offsets its count states.
    pub fn parse(silf: &'a [u8]) -> Result<Self, Refusal> {
        let data = Bytes::new(silf);
        let malformed = Refusal::TableMalformed { table: SILF };
        let version = data.u32(0).ok_or(malformed)?;
        let major = version >> 16;
        if !(1..=5).contains(&major) {
            return Err(Refusal::UnknownVersion {
                table: SILF,
                version,
            });
        }
        let (compiler, at) = if major >= 3 {
            let word = data.u32(4).ok_or(malformed)?;
            if major >= 5 {
                let scheme = u8::try_from(word >> 27).unwrap_or(u8::MAX);
                if scheme != 0 {
                    return Err(Refusal::UnknownCompression {
                        table: SILF,
                        scheme,
                    });
                }
                (Some(word & 0x07FF_FFFF), 8)
            } else {
                (Some(word), 8)
            }
        } else {
            (None, 4)
        };
        let count = data.u16(at).ok_or(malformed)?;
        let offsets = at + 4;
        let end = offsets
            .checked_add(usize::from(count) * 4)
            .ok_or(malformed)?;
        if end > data.len() {
            return Err(malformed);
        }
        Ok(Self {
            data,
            version,
            compiler,
            count,
            offsets,
        })
    }

    /// The table's version, as its 32-bit field.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The compiler version the table states, from version 3: from 5 the
    /// low twenty-seven bits of its word, under the compression scheme.
    #[must_use]
    pub const fn compiler_version(&self) -> Option<u32> {
        self.compiler
    }

    /// How many descriptions the table holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count as usize
    }

    /// Whether it holds none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Description number `index`.
    ///
    /// # Errors
    ///
    /// [`Refusal::TableMalformed`] for an index past the count, an offset
    /// past the table, or a description too short for the fields its version
    /// gives it.
    pub fn description(&self, index: usize) -> Result<Description<'a>, Refusal> {
        let malformed = Refusal::TableMalformed { table: SILF };
        if index >= self.len() {
            return Err(malformed);
        }
        let at = self.offsets + index * 4;
        let offset = usize::try_from(self.data.u32(at).ok_or(malformed)?).map_err(|_| malformed)?;
        let bytes = self.data.at(offset).ok_or(malformed)?;
        Description::parse(bytes, self.version >> 16)
    }

    /// The description a run of OpenType script `script` takes: the first
    /// whose script tags hold it, and the first description where none
    /// does (`docs/design/shaping.md`, *`Silf`*).
    ///
    /// `None` for a table of no descriptions.
    ///
    /// # Errors
    ///
    /// The first description's refusal, where it is the one taken and cannot
    /// be read; a description that cannot be read is passed over in the
    /// search.
    pub fn description_for(&self, script: Tag) -> Option<Result<Description<'a>, Refusal>> {
        let mut first = None;
        for index in 0..self.len() {
            let found = self.description(index);
            if let Ok(description) = &found {
                if description.script_tags.contains(&script) {
                    return Some(found);
                }
            }
            if first.is_none() {
                first = Some(found);
            }
        }
        first
    }
}

/// One justification level of a description: the glyph attributes that hold
/// each glyph's stretch, shrink, step and weight at that level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JustificationLevel {
    /// The attribute of `justify.X.stretch`.
    pub stretch: u8,
    /// The attribute of `justify.X.shrink`.
    pub shrink: u8,
    /// The attribute of `justify.X.step`.
    pub step: u8,
    /// The attribute of `justify.X.weight`.
    pub weight: u8,
    /// Which level starts the next stage.
    pub runto: u8,
}

/// One description of a `Silf`: the GTF's `SIL_Sub`.
///
/// Its fields are the header as stored, named as the GTF names them; the
/// pseudo-glyph map, the class map and the passes are read through its
/// methods.
#[derive(Clone, Debug)]
pub struct Description<'a> {
    data: Bytes<'a>,
    major: u32,
    /// The stack-machine language version, from `Silf` 3.
    pub rule_version: Option<u32>,
    /// `maxGlyphID`: read, and bounding nothing (see the module).
    pub max_glyph: u16,
    /// Em units added to the font's ascent.
    pub extra_ascent: i16,
    /// Em units added to its descent.
    pub extra_descent: i16,
    /// How many passes the description states.
    pub passes: u8,
    /// The first substitution pass: the passes before it break lines.
    pub substitution: u8,
    /// The first positioning pass.
    pub positioning: u8,
    /// The first justification pass.
    pub justification: u8,
    /// The first pass after the bidi pass; 255 is none.
    pub bidi: u8,
    /// The description's flags: bit 0 line-end contextuals, bit 1 cross-line
    /// contextuals that may be ignored, bits 2 to 4 space contextuals, bit 5
    /// a collision pass.
    pub flags: u8,
    /// The range of cross-line-boundary contextualization before a line.
    pub max_pre_context: u8,
    /// And after it.
    pub max_post_context: u8,
    /// The glyph attribute holding a pseudo-glyph's real glyph.
    pub attr_pseudo: u8,
    /// The glyph attribute holding a glyph's break weight.
    pub attr_break_weight: u8,
    /// The glyph attribute holding a glyph's directionality.
    pub attr_directionality: u8,
    /// The glyph attribute holding a glyph's mirrored glyph, from `Silf` 2.
    pub attr_mirroring: u8,
    /// The glyph attribute holding a glyph's key-glyph bitmap of passes,
    /// from `Silf` 2.
    pub attr_skip_passes: u8,
    /// The justification levels, from `Silf` 2.
    pub levels: Vec<JustificationLevel>,
    /// How many of the first glyph attributes are ligature components.
    pub ligature_components: u16,
    /// How many user-defined slot attributes the description uses.
    pub user_attributes: u8,
    /// The most components a ligature has.
    pub max_components: u8,
    /// The directions the description supports.
    pub direction: u8,
    /// The first of the glyph attributes holding collision settings.
    pub attr_collisions: u8,
    /// The critical features, from `Silf` 2.
    pub critical_features: Vec<u16>,
    /// The OpenType script tags the description is for.
    pub script_tags: Vec<Tag>,
    /// The line-break pseudo-glyph.
    pub line_break_glyph: u16,
    /// The offsets of the passes from the description's start, and of the
    /// end of the last.
    pass_offsets: Vec<u32>,
    /// Where the pseudo-glyph mappings start, and how many there are.
    pseudo_at: usize,
    pseudo_count: u16,
    /// Where the class map starts.
    classes_at: usize,
}

impl<'a> Description<'a> {
    /// Reads a description from `data`, its bytes from its start to the
    /// table's end, in a `Silf` of major version `major`.
    fn parse(data: Bytes<'a>, major: u32) -> Result<Self, Refusal> {
        let malformed = Refusal::TableMalformed { table: SILF };
        let byte = |at: usize| data.u8(at).ok_or(malformed);
        let short = |at: usize| data.u16(at).ok_or(malformed);
        let mut at = 0usize;
        let rule_version = if major >= 3 {
            let version = data.u32(0).ok_or(malformed)?;
            at = 8;
            Some(version)
        } else {
            None
        };
        let max_glyph = short(at)?;
        let extra_ascent = data.i16(at + 2).ok_or(malformed)?;
        let extra_descent = data.i16(at + 4).ok_or(malformed)?;
        at += 6;
        let passes = byte(at)?;
        let substitution = byte(at + 1)?;
        let positioning = byte(at + 2)?;
        let justification = byte(at + 3)?;
        let bidi = byte(at + 4)?;
        let flags = byte(at + 5)?;
        let max_pre_context = byte(at + 6)?;
        let max_post_context = byte(at + 7)?;
        let attr_pseudo = byte(at + 8)?;
        let attr_break_weight = byte(at + 9)?;
        let attr_directionality = byte(at + 10)?;
        at += 11;
        let (mut attr_mirroring, mut attr_skip_passes) = (0, 0);
        let mut levels = Vec::new();
        if major >= 2 {
            attr_mirroring = byte(at)?;
            attr_skip_passes = byte(at + 1)?;
            let count = byte(at + 2)?;
            at += 3;
            for _ in 0..count {
                levels.push(JustificationLevel {
                    stretch: byte(at)?,
                    shrink: byte(at + 1)?,
                    step: byte(at + 2)?,
                    weight: byte(at + 3)?,
                    runto: byte(at + 4)?,
                });
                at += 8;
            }
        }
        let ligature_components = short(at)?;
        let user_attributes = byte(at + 2)?;
        let max_components = byte(at + 3)?;
        let direction = byte(at + 4)?;
        // Reserved before 5, read whatever the version: the GTF lets a
        // meaning given later be used at a lower version.
        let attr_collisions = byte(at + 5)?;
        // Two reserved bytes, and a third from 2.
        at += 8;
        let mut critical_features = Vec::new();
        if major >= 2 {
            at += 1;
            let count = byte(at)?;
            at += 1;
            for n in 0..usize::from(count) {
                critical_features.push(short(at + n * 2)?);
            }
            // And the reserved byte after them.
            at += usize::from(count) * 2 + 1;
        }
        let script_count = byte(at)?;
        at += 1;
        let mut script_tags = Vec::new();
        for n in 0..usize::from(script_count) {
            script_tags.push(Tag(data.u32(at + n * 4).ok_or(malformed)?));
        }
        at += usize::from(script_count) * 4;
        let line_break_glyph = short(at)?;
        at += 2;
        let mut pass_offsets = Vec::with_capacity(usize::from(passes) + 1);
        for n in 0..=usize::from(passes) {
            pass_offsets.push(data.u32(at + n * 4).ok_or(malformed)?);
        }
        at += (usize::from(passes) + 1) * 4;
        let pseudo_count = short(at)?;
        let pseudo_at = at + 8;
        let classes_at = pseudo_at
            .checked_add(usize::from(pseudo_count) * 6)
            .ok_or(malformed)?;
        // The class map's two counts must be there for it to be read.
        short(classes_at + 2)?;
        Ok(Self {
            data,
            major,
            rule_version,
            max_glyph,
            extra_ascent,
            extra_descent,
            passes,
            substitution,
            positioning,
            justification,
            bidi,
            flags,
            max_pre_context,
            max_post_context,
            attr_pseudo,
            attr_break_weight,
            attr_directionality,
            attr_mirroring,
            attr_skip_passes,
            levels,
            ligature_components,
            user_attributes,
            max_components,
            direction,
            attr_collisions,
            critical_features,
            script_tags,
            line_break_glyph,
            pass_offsets,
            pseudo_at,
            pseudo_count,
            classes_at,
        })
    }

    /// The major version of the `Silf` the description is in.
    #[must_use]
    pub const fn table_version(&self) -> u32 {
        self.major
    }

    /// How many code points the pseudo-glyph map pairs with a glyph.
    #[must_use]
    pub const fn pseudo_count(&self) -> u16 {
        self.pseudo_count
    }

    /// Mapping `index` of the pseudo-glyph map: a code point and its glyph.
    #[must_use]
    pub fn pseudo_mapping(&self, index: usize) -> Option<(u32, u16)> {
        if index >= usize::from(self.pseudo_count) {
            return None;
        }
        let at = self.pseudo_at + index * 6;
        Some((self.data.u32(at)?, self.data.u16(at + 4)?))
    }

    /// The pseudo-glyph the map gives `code`.
    ///
    /// The mappings are in code-point order, and bisected; where bisection
    /// finds nothing they are read in order, so a map out of order still
    /// answers its first match.
    #[must_use]
    pub fn pseudo(&self, code: u32) -> Option<u16> {
        let count = usize::from(self.pseudo_count);
        let (mut low, mut high) = (0usize, count);
        while low < high {
            let middle = low + (high - low) / 2;
            let (at, glyph) = self.pseudo_mapping(middle)?;
            match at.cmp(&code) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => {
                    // The first of equal codes, should the map repeat one.
                    let mut first = (middle, glyph);
                    while first.0 > 0 {
                        match self.pseudo_mapping(first.0 - 1) {
                            Some((before, glyph)) if before == code => first = (first.0 - 1, glyph),
                            _ => break,
                        }
                    }
                    return Some(first.1);
                }
            }
        }
        (0..count)
            .filter_map(|index| self.pseudo_mapping(index))
            .find(|(at, _)| *at == code)
            .map(|(_, glyph)| glyph)
    }

    /// The description's class map.
    #[must_use]
    pub fn classes(&self) -> ClassMap<'a> {
        ClassMap {
            data: self.data.at(self.classes_at).unwrap_or(Bytes::new(&[])),
            wide: self.major >= 4,
        }
    }

    /// Pass number `index`, under `limits`' pass ceiling.
    ///
    /// # Errors
    ///
    /// [`Refusal::TooManyPasses`] for an index at or past
    /// [`Limits::max_passes`], and [`Refusal::PassMalformed`] for an index
    /// past the passes the description states or a pass that cannot be read
    /// whole or whose counts the GTF's order cannot hold.
    pub fn pass(&self, index: usize, limits: &Limits) -> Result<Pass<'a>, Refusal> {
        let number = u8::try_from(index).unwrap_or(u8::MAX);
        if index >= usize::try_from(limits.max_passes).unwrap_or(usize::MAX) {
            return Err(Refusal::TooManyPasses {
                passes: self.passes,
            });
        }
        let malformed = Refusal::PassMalformed { pass: number };
        if index >= usize::from(self.passes) {
            return Err(malformed);
        }
        let start = self.pass_offsets.get(index).copied().ok_or(malformed)?;
        let end = self.pass_offsets.get(index + 1).copied().ok_or(malformed)?;
        let start = usize::try_from(start).map_err(|_| malformed)?;
        let end = usize::try_from(end).map_err(|_| malformed)?;
        if start > end || end > self.data.len() {
            return Err(malformed);
        }
        Pass::parse(self.data, start, end, number, self.major)
    }
}

/// A description's class map: the replacement classes its substitutions
/// select glyphs from.
#[derive(Clone, Copy, Debug)]
pub struct ClassMap<'a> {
    data: Bytes<'a>,
    wide: bool,
}

/// The two kinds of class a [`ClassMap`] stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    /// Its glyphs in order: an index gives a glyph directly.
    Linear,
    /// `(glyph, index)` pairs sorted by glyph: a glyph gives its index by
    /// bisection.
    Lookup,
}

impl<'a> ClassMap<'a> {
    /// How many classes the map holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.u16(0).map_or(0, usize::from)
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many of them, from the first, are linear.
    #[must_use]
    pub fn linear_count(&self) -> usize {
        self.data.u16(2).map_or(0, usize::from)
    }

    /// Class `class`'s kind, where the map holds it.
    #[must_use]
    pub fn kind(&self, class: usize) -> Option<ClassKind> {
        if class >= self.len() {
            return None;
        }
        Some(if class < self.linear_count() {
            ClassKind::Linear
        } else {
            ClassKind::Lookup
        })
    }

    /// The offset of class `n` (or the end of the last, at `n` the count)
    /// from the map's start.
    fn offset(&self, n: usize) -> Option<usize> {
        if self.wide {
            usize::try_from(self.data.u32(4usize.checked_add(n.checked_mul(4)?)?)?).ok()
        } else {
            self.data.u16_at(4, n).map(usize::from)
        }
    }

    /// Class `class`'s bytes, from its start to the next class's.
    fn class(&self, class: usize) -> Option<Bytes<'a>> {
        if class >= self.len() {
            return None;
        }
        let start = self.offset(class)?;
        let end = self.offset(class + 1)?;
        self.data.as_slice().get(start..end).map(Bytes::new)
    }

    /// How many glyphs class `class` holds.
    #[must_use]
    pub fn class_len(&self, class: usize) -> Option<usize> {
        let bytes = self.class(class)?;
        match self.kind(class)? {
            ClassKind::Linear => Some(bytes.len() / 2),
            ClassKind::Lookup => {
                let count = usize::from(bytes.u16(0)?);
                Some(count.min(bytes.len().saturating_sub(8) / 4))
            }
        }
    }

    /// The glyph at `index` of class `class`.
    #[must_use]
    pub fn glyph(&self, class: usize, index: usize) -> Option<u16> {
        let bytes = self.class(class)?;
        match self.kind(class)? {
            ClassKind::Linear => bytes.u16_at(0, index),
            ClassKind::Lookup => {
                let count = self.class_len(class)?;
                (0..count)
                    .filter_map(|n| Some((bytes.u16(8 + n * 4)?, bytes.u16(10 + n * 4)?)))
                    .find(|(_, at)| usize::from(*at) == index)
                    .map(|(glyph, _)| glyph)
            }
        }
    }

    /// The index of `glyph` in class `class`.
    ///
    /// A linear class is read in order, the GTF's "very slow, but stored very
    /// simply"; a lookup class is bisected, and where bisection finds nothing
    /// its pairs are read in order, so an array out of order still answers
    /// its first match.
    #[must_use]
    pub fn index(&self, class: usize, glyph: u16) -> Option<u16> {
        let bytes = self.class(class)?;
        let count = self.class_len(class)?;
        match self.kind(class)? {
            ClassKind::Linear => (0..count)
                .find(|n| bytes.u16_at(0, *n) == Some(glyph))
                .and_then(|n| u16::try_from(n).ok()),
            ClassKind::Lookup => {
                let pair = |n: usize| Some((bytes.u16(8 + n * 4)?, bytes.u16(10 + n * 4)?));
                let (mut low, mut high) = (0usize, count);
                while low < high {
                    let middle = low + (high - low) / 2;
                    let (at, index) = pair(middle)?;
                    match at.cmp(&glyph) {
                        core::cmp::Ordering::Less => low = middle + 1,
                        core::cmp::Ordering::Greater => high = middle,
                        core::cmp::Ordering::Equal => {
                            let mut first = (middle, index);
                            while first.0 > 0 {
                                match pair(first.0 - 1) {
                                    Some((before, index)) if before == glyph => {
                                        first = (first.0 - 1, index);
                                    }
                                    _ => break,
                                }
                            }
                            return Some(first.1);
                        }
                    }
                }
                (0..count)
                    .filter_map(pair)
                    .find(|(at, _)| *at == glyph)
                    .map(|(_, index)| index)
            }
        }
    }
}

/// A rule a pass's machine found at a scan position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The rule's number in its pass.
    pub rule: u16,
    /// Its sort key: the higher, the earlier it is tried.
    pub sort_key: u16,
    /// How many of its slots come before the scan position.
    pub pre_context: u8,
    /// The index of the slot after the last it matched.
    pub end: usize,
}

/// What a pass's machine found at one scan position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matched {
    /// The rules, in the order they are to be tried.
    pub candidates: Vec<Candidate>,
    /// What the walk refused, each once.
    pub refusals: Vec<Refusal>,
}

/// One pass of a description: its header, its machine and its code.
#[derive(Clone, Copy, Debug)]
pub struct Pass<'a> {
    /// The description's bytes: code offsets are from its start.
    data: Bytes<'a>,
    index: u8,
    /// Bits 0 to 2 the collision runs, 3 and 4 the kerning, 5 a reversed
    /// pass.
    pub flags: u8,
    /// The most rules that may fire without the scan position moving on.
    pub max_rule_loop: u8,
    /// The slots of input a rule of the pass may need.
    pub max_rule_context: u8,
    /// How far the scan position may move back.
    pub max_backup: u8,
    /// How many rules the pass holds.
    pub rules: u16,
    /// Its states, transitional states, success states and columns.
    pub rows: u16,
    /// The states with transitions: the length of the transition table.
    pub transitional: u16,
    /// The accepting states, the last of the states.
    pub success: u16,
    /// The machine's columns; 0 is a pass with no machine.
    pub columns: u16,
    /// The fewest slots any rule's context has before its first modified
    /// one.
    pub min_pre_context: u8,
    /// The most.
    pub max_pre_context: u8,
    /// The collision threshold, from `Silf` 2.
    pub collision_threshold: u8,
    ranges_at: usize,
    range_count: u16,
    ranges_sorted: bool,
    rule_offsets_at: usize,
    rule_map_at: usize,
    start_states_at: usize,
    sort_keys_at: usize,
    pre_contexts_at: usize,
    constraint_offsets_at: usize,
    action_offsets_at: usize,
    transitions_at: usize,
    pass_constraint: Option<(usize, usize)>,
    rule_constraints_at: usize,
    actions_at: usize,
}

impl<'a> Pass<'a> {
    /// Reads the pass stored from `start` to `end` of the description.
    fn parse(
        data: Bytes<'a>,
        start: usize,
        end: usize,
        index: u8,
        major: u32,
    ) -> Result<Self, Refusal> {
        let malformed = Refusal::PassMalformed { pass: index };
        let block = data
            .as_slice()
            .get(start..end)
            .map(Bytes::new)
            .ok_or(malformed)?;
        let byte = |at: usize| block.u8(at).ok_or(malformed);
        let short = |at: usize| block.u16(at).ok_or(malformed);
        let long = |at: usize| -> Result<usize, Refusal> {
            usize::try_from(block.u32(at).ok_or(malformed)?).map_err(|_| malformed)
        };
        let flags = byte(0)?;
        let max_rule_loop = byte(1)?;
        let max_rule_context = byte(2)?;
        let max_backup = byte(3)?;
        let rules = short(4)?;
        let mut at = 6;
        let mut pass_constraint_at = None;
        if major >= 2 {
            // `fsmOffset`, read and not relied on, and the pass constraint.
            pass_constraint_at = Some(long(8)?);
            at = 12;
        }
        let rule_constraints_at = long(at)?;
        let actions_at = long(at + 4)?;
        // `oDebug`, and the machine's counts.
        at += 12;
        let rows = short(at)?;
        let transitional = short(at + 2)?;
        let success = short(at + 4)?;
        let columns = short(at + 6)?;
        let range_count = short(at + 8)?;
        at += 16;
        let ranges_at = at;
        at += usize::from(range_count) * 6;
        let rule_offsets_at = at;
        at += (usize::from(success) + 1) * 2;
        let rule_map_len = usize::from(
            block
                .u16_at(rule_offsets_at, usize::from(success))
                .ok_or(malformed)?,
        );
        let rule_map_at = at;
        at += rule_map_len * 2;
        let min_pre_context = byte(at)?;
        let max_pre_context = byte(at + 1)?;
        at += 2;
        if min_pre_context > max_pre_context {
            return Err(malformed);
        }
        let start_states_at = at;
        at += (usize::from(max_pre_context - min_pre_context) + 1) * 2;
        let sort_keys_at = at;
        at += usize::from(rules) * 2;
        let pre_contexts_at = at;
        at += usize::from(rules);
        let mut collision_threshold = 0;
        let mut pass_constraint = None;
        if major >= 2 {
            collision_threshold = byte(at)?;
            let length = usize::from(short(at + 1)?);
            at += 3;
            if let Some(code) = pass_constraint_at {
                pass_constraint = Some((code, length));
            }
        }
        let constraint_offsets_at = at;
        at += (usize::from(rules) + 1) * 2;
        let action_offsets_at = at;
        at += (usize::from(rules) + 1) * 2;
        let transitions_at = at;
        at += usize::from(transitional) * usize::from(columns) * 2;
        // The reserved byte after the transitions, from `Silf` 2.
        if major >= 2 {
            at += 1;
        }
        if at > block.len() {
            return Err(malformed);
        }
        // The GTF's order: transitional states that do not accept, then
        // transitional ones that do, then final accepting ones. Every state
        // that does not accept is transitional.
        if transitional > rows || success > rows || rows - success > transitional {
            return Err(malformed);
        }
        // Every range names a column the transitions have, and the ranges
        // are bisected where they are sorted and do not overlap.
        let mut ranges_sorted = true;
        let mut last: Option<u16> = None;
        for n in 0..usize::from(range_count) {
            let first = short(ranges_at + n * 6)?;
            let final_glyph = short(ranges_at + n * 6 + 2)?;
            let column = short(ranges_at + n * 6 + 4)?;
            if column >= columns {
                return Err(malformed);
            }
            if first > final_glyph || last.is_some_and(|previous| first <= previous) {
                ranges_sorted = false;
            }
            last = Some(final_glyph);
        }
        Ok(Self {
            data,
            index,
            flags,
            max_rule_loop,
            max_rule_context,
            max_backup,
            rules,
            rows,
            transitional,
            success,
            columns,
            min_pre_context,
            max_pre_context,
            collision_threshold,
            ranges_at: start + ranges_at,
            range_count,
            ranges_sorted,
            rule_offsets_at: start + rule_offsets_at,
            rule_map_at: start + rule_map_at,
            start_states_at: start + start_states_at,
            sort_keys_at: start + sort_keys_at,
            pre_contexts_at: start + pre_contexts_at,
            constraint_offsets_at: start + constraint_offsets_at,
            action_offsets_at: start + action_offsets_at,
            transitions_at: start + transitions_at,
            pass_constraint,
            rule_constraints_at,
            actions_at,
        })
    }

    /// The pass's number in its description.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.index
    }

    /// Whether the pass has a machine: a pass of no columns runs only its
    /// collision fixing.
    #[must_use]
    pub const fn has_machine(&self) -> bool {
        self.columns > 0
    }

    /// The collision runs its flags give.
    #[must_use]
    pub const fn collision_runs(&self) -> u8 {
        self.flags & 0x07
    }

    /// The kerning its flags give.
    #[must_use]
    pub const fn kerning(&self) -> u8 {
        (self.flags >> 3) & 0x03
    }

    /// Whether its flags mark it a reversed pass.
    #[must_use]
    pub const fn is_reversed(&self) -> bool {
        self.flags & 0x20 != 0
    }

    /// Range number `n`: its first and last glyph and its column.
    fn range(&self, n: usize) -> Option<(u16, u16, u16)> {
        let at = self.ranges_at + n * 6;
        Some((
            self.data.u16(at)?,
            self.data.u16(at + 2)?,
            self.data.u16(at + 4)?,
        ))
    }

    /// `glyph`'s column: the range holding it, bisected where the ranges are
    /// sorted and disjoint and read in order, the first winning, where they
    /// are not. `None` for a glyph in no range, which jams the machine.
    #[must_use]
    pub fn column(&self, glyph: u16) -> Option<u16> {
        let count = usize::from(self.range_count);
        if self.ranges_sorted {
            let (mut low, mut high) = (0usize, count);
            while low < high {
                let middle = low + (high - low) / 2;
                let (first, last, column) = self.range(middle)?;
                if glyph < first {
                    high = middle;
                } else if glyph > last {
                    low = middle + 1;
                } else {
                    return Some(column);
                }
            }
            return None;
        }
        (0..count)
            .filter_map(|n| self.range(n))
            .find(|(first, last, _)| (*first..=*last).contains(&glyph))
            .map(|(_, _, column)| column)
    }

    /// Where the machine starts reading for scan position `position`, and in
    /// which state: `position − maxPre` in the first start state where
    /// enough slots come before it, and slot 0 in the start state that skips
    /// the missing ones where they do not; `None` where fewer slots come
    /// before it than any rule's pre-context.
    #[must_use]
    pub fn start(&self, position: usize) -> Option<(usize, u16)> {
        let max = usize::from(self.max_pre_context);
        let min = usize::from(self.min_pre_context);
        if position < min {
            return None;
        }
        let (slot, skipped) = if position >= max {
            (position - max, 0)
        } else {
            (0, max - position)
        };
        let state = self.data.u16_at(self.start_states_at, skipped)?;
        Some((slot, state))
    }

    /// Runs the machine at scan position `position` of `glyphs`, and returns
    /// the rules it found in the order they are to be tried.
    ///
    /// Each slot read takes the transition its column gives. A glyph in no
    /// range or a transition to 0 jams the machine; a transition to a state
    /// past the table, or one the table cannot give, is refused by name and
    /// ends the walk; an accepting state's rules join the candidates, and a
    /// final state ends the walk. No more than
    /// [`Limits::max_context_length`] slots are read, nor more than
    /// [`Limits::max_rule_candidates`] rules gathered, each refused by name
    /// past its ceiling. A rule whose own pre-context is longer than the
    /// slots before `position` is dropped; a rule number past the pass's
    /// count is refused by name; a rule the walk reaches twice is kept where
    /// it was first reached. The rest are sorted by sort key, highest first,
    /// and by rule number among equal keys.
    #[must_use]
    pub fn candidates(&self, glyphs: &[u16], position: usize, limits: &Limits) -> Matched {
        self.candidates_in(|at| glyphs.get(at).copied(), position, limits)
    }

    /// [`Pass::candidates`], each slot's glyph read through `glyph`, which
    /// answers `None` past the stream's end: what a pass running over a
    /// slot stream asks without copying its glyphs out.
    #[must_use]
    pub fn candidates_in(
        &self,
        glyph: impl Fn(usize) -> Option<u16>,
        position: usize,
        limits: &Limits,
    ) -> Matched {
        let mut matched = Matched::default();
        if !self.has_machine() {
            return matched;
        }
        let Some((mut at, mut state)) = self.start(position) else {
            return matched;
        };
        let pass = self.index;
        let refuse = |matched: &mut Matched, refusal: Refusal| {
            if !matched.refusals.contains(&refusal) {
                matched.refusals.push(refusal);
            }
        };
        if state >= self.rows {
            refuse(&mut matched, Refusal::TransitionPastTable { pass, state });
            return matched;
        }
        let accepting = self.rows - self.success;
        let ceiling = usize::try_from(limits.max_rule_candidates).unwrap_or(usize::MAX);
        let mut found: Vec<Candidate> = Vec::new();
        let mut read = 0usize;
        while state < self.transitional {
            let Some(glyph) = glyph(at) else {
                break;
            };
            if read >= limits.max_context_length {
                refuse(&mut matched, Refusal::RuleContextTooLong { pass });
                break;
            }
            let Some(column) = self.column(glyph) else {
                break;
            };
            let cell = usize::from(state) * usize::from(self.columns) + usize::from(column);
            let Some(next) = self.data.u16_at(self.transitions_at, cell) else {
                refuse(&mut matched, Refusal::TransitionPastTable { pass, state });
                break;
            };
            if next == 0 {
                break;
            }
            if next >= self.rows {
                refuse(&mut matched, Refusal::TransitionPastTable { pass, state });
                break;
            }
            state = next;
            at += 1;
            read += 1;
            if state >= accepting {
                let success = usize::from(state - accepting);
                let low = self.data.u16_at(self.rule_offsets_at, success);
                let high = self.data.u16_at(self.rule_offsets_at, success + 1);
                let (Some(low), Some(high)) = (low, high) else {
                    refuse(&mut matched, Refusal::PassMalformed { pass });
                    break;
                };
                for n in usize::from(low)..usize::from(high) {
                    let Some(rule) = self.data.u16_at(self.rule_map_at, n) else {
                        refuse(&mut matched, Refusal::PassMalformed { pass });
                        break;
                    };
                    if rule >= self.rules {
                        refuse(&mut matched, Refusal::RuleOutOfRange { pass, rule });
                        continue;
                    }
                    if found.iter().any(|c| c.rule == rule) {
                        continue;
                    }
                    if found.len() >= ceiling {
                        refuse(&mut matched, Refusal::TooManyCandidates { pass });
                        break;
                    }
                    let sort_key = self.data.u16_at(self.sort_keys_at, usize::from(rule));
                    let pre_context = self.data.u8(self.pre_contexts_at + usize::from(rule));
                    let (Some(sort_key), Some(pre_context)) = (sort_key, pre_context) else {
                        refuse(&mut matched, Refusal::PassMalformed { pass });
                        continue;
                    };
                    found.push(Candidate {
                        rule,
                        sort_key,
                        pre_context,
                        end: at,
                    });
                }
            }
        }
        found.retain(|candidate| usize::from(candidate.pre_context) <= position);
        found.sort_by(|a, b| b.sort_key.cmp(&a.sort_key).then(a.rule.cmp(&b.rule)));
        matched.candidates = found;
        matched
    }

    /// The pass constraint's code, from `Silf` 2; `None` where the pass has
    /// none or the description does not hold it.
    #[must_use]
    pub fn pass_constraint(&self) -> Option<&'a [u8]> {
        let (at, length) = self.pass_constraint?;
        if length == 0 {
            return None;
        }
        self.data.as_slice().get(at..at.checked_add(length)?)
    }

    /// Rule `rule`'s constraint code; empty where it has none.
    #[must_use]
    pub fn constraint(&self, rule: u16) -> Option<&'a [u8]> {
        self.code(self.constraint_offsets_at, self.rule_constraints_at, rule)
    }

    /// Rule `rule`'s action code.
    #[must_use]
    pub fn action(&self, rule: u16) -> Option<&'a [u8]> {
        self.code(self.action_offsets_at, self.actions_at, rule)
    }

    /// A rule's block of code: from its offset to the next rule's, both from
    /// `base` of the description.
    fn code(&self, offsets_at: usize, base: usize, rule: u16) -> Option<&'a [u8]> {
        if rule >= self.rules {
            return None;
        }
        let start = usize::from(self.data.u16_at(offsets_at, usize::from(rule))?);
        let end = usize::from(self.data.u16_at(offsets_at, usize::from(rule) + 1)?);
        if start > end {
            return None;
        }
        self.data
            .as_slice()
            .get(base.checked_add(start)?..base.checked_add(end)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassKind, ClassMap};
    use crate::read::Bytes;

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A class map of a linear class `[5, 9, 7]` and a lookup class whose
    /// pairs are out of order, in `USHORT` offsets.
    fn map() -> Vec<u8> {
        let linear = u16s(&[5, 9, 7, 9]);
        let lookup = u16s(&[3, 0, 0, 0, 40, 2, 30, 0, 50, 1]);
        let head = 4 + 3 * 2;
        let mut out = u16s(&[2, 1]);
        let first = u16::try_from(head).expect("small");
        let second = first + u16::try_from(linear.len()).expect("small");
        let end = second + u16::try_from(lookup.len()).expect("small");
        out.extend(u16s(&[first, second, end]));
        out.extend(linear);
        out.extend(lookup);
        out
    }

    #[test]
    fn a_linear_class_answers_in_order_and_its_first_match() {
        let bytes = map();
        let classes = ClassMap {
            data: Bytes::new(&bytes),
            wide: false,
        };
        assert_eq!(classes.len(), 2);
        assert_eq!(classes.kind(0), Some(ClassKind::Linear));
        assert_eq!(classes.kind(1), Some(ClassKind::Lookup));
        assert_eq!(classes.kind(2), None);
        assert_eq!(classes.glyph(0, 1), Some(9));
        assert_eq!(classes.glyph(0, 4), None);
        // 9 is at 1 and at 3: the first answers.
        assert_eq!(classes.index(0, 9), Some(1));
        assert_eq!(classes.index(0, 6), None);
    }

    #[test]
    fn a_lookup_class_out_of_order_still_answers() {
        let bytes = map();
        let classes = ClassMap {
            data: Bytes::new(&bytes),
            wide: false,
        };
        assert_eq!(classes.class_len(1), Some(3));
        assert_eq!(classes.index(1, 30), Some(0));
        assert_eq!(classes.index(1, 40), Some(2));
        assert_eq!(classes.index(1, 50), Some(1));
        assert_eq!(classes.index(1, 45), None);
        assert_eq!(classes.glyph(1, 2), Some(40));
        assert_eq!(classes.glyph(1, 3), None);
    }
}
