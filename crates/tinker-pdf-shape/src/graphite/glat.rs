//! Glyph attributes: `Gloc` and `Glat`.
//!
//! Every glyph of a Graphite face has a sparse array of signed 16-bit
//! attributes, numbered by the face — the attachment points, break weights,
//! justification and collision settings its rules read. `Glat` stores each
//! glyph's as *runs* of consecutive numbers, and `Gloc`, laid out as `loca`
//! is with a header in front, says where each glyph's runs start
//! (`docs/design/shaping.md`, *Glyph attributes*).
//!
//! # The versions read
//!
//! - **`Gloc` 1**, either minor: its version, flags — bit 0 offsets of four
//!   bytes rather than two, bit 1 an array of attribute names at its end —
//!   the attribute count, and one offset a glyph and one more. The glyph
//!   count is the offsets', read from the table's own length, because the
//!   count the GTF would have it take from `Silf` is not one a face can be
//!   held to.
//! - **`Glat` 1**, each run a byte attribute number and a byte count;
//!   **`Glat` 2**, each two `USHORT`s; and **`Glat` 3**, as 2 with a second
//!   header word whose top five bits are the compression scheme and whose
//!   lowest is the octabox flag. Each run is followed by its count of
//!   `SHORT` values. Where the octabox flag is set, a glyph's block opens
//!   with its octabox ([`Octabox`]) and its runs follow.
//!
//! A `Glat` arrives here as it reads uncompressed: `super::decompress` has
//! already decoded one that was not.

use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;

use super::{Refusal, GLAT, GLOC};

/// Which `Glat` a face carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlatVersion {
    /// Version 1: a byte attribute number and a byte count a run.
    One,
    /// Version 2: two `USHORT`s a run.
    Two,
    /// Version 3: version 2's runs, after an octabox where the header sets
    /// its flag.
    Three {
        /// Whether each glyph's block opens with an octabox.
        octaboxes: bool,
    },
}

/// A glyph's shape as the collision avoidance reads it: its bounding box
/// with the corners cut at 45°, and finer boxes on a 4 × 4 grid over it.
///
/// Every value is a byte, a fraction in 255ths of a span of the glyph's
/// bounding box (the box itself is the glyph's own, in `glyf` or `CFF`):
/// the four diagonals are the least and greatest x + y and the least and
/// greatest x − y over the glyph's outline, each scaled across the box's
/// width plus its height. That order is the one the Graphite project's
/// edition of the GTF names them in, `diag_neg_min`, `diag_neg_max`,
/// `diag_pos_min`, `diag_pos_max`, where the 2019 edition lists the pairs the
/// other way round; the vendored Awami Nastaliq's outlines are what settle
/// it, and `tests/graphite.rs` holds the reading to them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Octabox {
    /// Which cells of the 4 × 4 grid have a sub-box: bit `(y × 4) + x`, so
    /// bit 3 is the lower right cell and bit 12 the upper left.
    pub cells: u16,
    /// The four diagonals, in the order stored. See the type.
    pub diagonals: [u8; 4],
    /// One sub-box for each bit of [`Octabox::cells`], lowest bit first.
    pub subboxes: Vec<Subbox>,
}

impl Octabox {
    /// The least x + y over the outline: the first diagonal byte.
    #[must_use]
    pub const fn sum_min(&self) -> u8 {
        self.diagonals[0]
    }

    /// The greatest x + y.
    #[must_use]
    pub const fn sum_max(&self) -> u8 {
        self.diagonals[1]
    }

    /// The least x − y.
    #[must_use]
    pub const fn difference_min(&self) -> u8 {
        self.diagonals[2]
    }

    /// The greatest x − y.
    #[must_use]
    pub const fn difference_max(&self) -> u8 {
        self.diagonals[3]
    }
}

/// One cell's box of an [`Octabox`]: eight bytes, read as stored.
///
/// What its sides and diagonals are scaled to is settled where collision
/// avoidance first reads them (`docs/design/shaping.md`, *Glyph
/// attributes*); here they are the bytes, named as the GTF names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subbox {
    /// The cell's bit in [`Octabox::cells`], 0 to 15.
    pub cell: u8,
    /// The box's left.
    pub left: u8,
    /// Its right.
    pub right: u8,
    /// Its bottom.
    pub bottom: u8,
    /// Its top.
    pub top: u8,
    /// Its four diagonals, in the order an [`Octabox`]'s are stored.
    pub diagonals: [u8; 4],
}

impl Subbox {
    /// The cell's column in the grid, 0 at the left.
    #[must_use]
    pub const fn column(&self) -> u8 {
        self.cell % 4
    }

    /// The cell's row, 0 at the bottom.
    #[must_use]
    pub const fn row(&self) -> u8 {
        self.cell / 4
    }
}

/// A face's glyph attributes: its `Gloc` and its `Glat`, read together.
#[derive(Clone, Copy, Debug)]
pub struct Attributes<'a> {
    gloc: Bytes<'a>,
    glat: Bytes<'a>,
    long: bool,
    names: bool,
    count: u16,
    glyphs: usize,
    version: GlatVersion,
    /// Where `Glat`'s entries may begin: after its four-byte header, or its
    /// eight from version 3.
    header: usize,
    max_runs: u32,
}

impl<'a> Attributes<'a> {
    /// Reads a `Gloc` and the `Glat` it indexes, the `Glat` as it reads
    /// uncompressed.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownVersion`] for a `Gloc` whose major version is not 1
    /// or a `Glat` whose is not 1, 2 or 3; [`Refusal::TableMalformed`] for
    /// one too short for its own header, or a `Gloc` whose name array leaves
    /// no room for a single offset; and [`Refusal::UnknownCompression`] for a
    /// `Glat` 3 whose header says it is compressed still, which a decoded
    /// table never does.
    pub fn parse(gloc: &'a [u8], glat: &'a [u8], limits: &Limits) -> Result<Self, Refusal> {
        let gloc = Bytes::new(gloc);
        let glat = Bytes::new(glat);
        let malformed = |table: Tag| Refusal::TableMalformed { table };

        let gloc_version = gloc.u32(0).ok_or(malformed(GLOC))?;
        if gloc_version >> 16 != 1 {
            return Err(Refusal::UnknownVersion {
                table: GLOC,
                version: gloc_version,
            });
        }
        let flags = gloc.u16(4).ok_or(malformed(GLOC))?;
        let count = gloc.u16(6).ok_or(malformed(GLOC))?;
        let long = flags & 1 != 0;
        let names = flags & 2 != 0;
        let names_bytes = if names { usize::from(count) * 2 } else { 0 };
        let offsets_bytes = gloc
            .len()
            .checked_sub(8)
            .and_then(|left| left.checked_sub(names_bytes))
            .ok_or(malformed(GLOC))?;
        let locations = offsets_bytes / if long { 4 } else { 2 };
        // One offset a glyph and one more: a table of none indexes nothing.
        let glyphs = locations.checked_sub(1).ok_or(malformed(GLOC))?;

        let glat_version = glat.u32(0).ok_or(malformed(GLAT))?;
        let (version, header) = match glat_version >> 16 {
            1 => (GlatVersion::One, 4),
            2 => (GlatVersion::Two, 4),
            3 => {
                let word = glat.u32(4).ok_or(malformed(GLAT))?;
                let scheme = u8::try_from(word >> 27).unwrap_or(u8::MAX);
                if scheme != 0 {
                    return Err(Refusal::UnknownCompression {
                        table: GLAT,
                        scheme,
                    });
                }
                (
                    GlatVersion::Three {
                        octaboxes: word & 1 != 0,
                    },
                    8,
                )
            }
            _ => {
                return Err(Refusal::UnknownVersion {
                    table: GLAT,
                    version: glat_version,
                })
            }
        };
        Ok(Self {
            gloc,
            glat,
            long,
            names,
            count,
            glyphs,
            version,
            header,
            max_runs: limits.max_attribute_runs,
        })
    }

    /// How many glyphs `Gloc` gives offsets for.
    #[must_use]
    pub const fn glyph_count(&self) -> usize {
        self.glyphs
    }

    /// The attribute count `Gloc` states.
    ///
    /// It sizes the name array and nothing else: an attribute numbered at or
    /// past it is still answered from whatever `Glat` stores.
    #[must_use]
    pub const fn attribute_count(&self) -> u16 {
        self.count
    }

    /// The `name` table index `Gloc` gives attribute `number`, where it
    /// carries its name array: the GTF's debug id.
    #[must_use]
    pub fn attribute_name(&self, number: u16) -> Option<u16> {
        if !self.names || number >= self.count {
            return None;
        }
        let start = self.gloc.len().checked_sub(usize::from(self.count) * 2)?;
        self.gloc.u16_at(start, usize::from(number))
    }

    /// Which `Glat` this is.
    #[must_use]
    pub const fn version(&self) -> GlatVersion {
        self.version
    }

    /// The value of attribute `number` for `glyph`.
    ///
    /// The first run that holds the number answers; an attribute no run
    /// holds is 0, the value the compiler's documentation gives every
    /// attribute a glyph does not list.
    ///
    /// # Errors
    ///
    /// [`Refusal::GlyphUnreadable`] for a glyph past `Gloc`'s offsets, a
    /// block that ends before it starts, starts inside `Glat`'s header or
    /// ends past its end, and a block whose octabox or last run reaches past
    /// it; [`Refusal::TooManyAttributeRuns`] for a block of more runs than
    /// [`Limits::max_attribute_runs`]. Both are properties of the block, so
    /// every attribute of such a glyph is refused alike.
    pub fn lookup(&self, glyph: u16, number: u16) -> Result<i16, Refusal> {
        let mut found = None;
        self.walk(glyph, |first, values| {
            if found.is_none() {
                let index = u32::from(number).checked_sub(u32::from(first));
                if let Some(index) = index.and_then(|i| usize::try_from(i).ok()) {
                    if index < values.len() / 2 {
                        found = values.i16(index * 2);
                    }
                }
            }
        })?;
        Ok(found.unwrap_or(0))
    }

    /// [`Attributes::lookup`], with every refusal answered `None`.
    #[must_use]
    pub fn attribute(&self, glyph: u16, number: u16) -> Option<i16> {
        self.lookup(glyph, number).ok()
    }

    /// Every run `glyph`'s block stores, in order: its first attribute
    /// number and its values.
    ///
    /// # Errors
    ///
    /// As [`Attributes::lookup`].
    pub fn runs(&self, glyph: u16) -> Result<Vec<(u16, Vec<i16>)>, Refusal> {
        let mut out = Vec::new();
        self.walk(glyph, |first, values| {
            let read: Vec<i16> = (0..values.len() / 2)
                .filter_map(|index| values.i16(index * 2))
                .collect();
            out.push((first, read));
        })?;
        Ok(out)
    }

    /// `glyph`'s octabox, where `Glat` carries octaboxes and the glyph's
    /// block is not empty; `None` for a glyph with no data at all.
    ///
    /// # Errors
    ///
    /// As [`Attributes::lookup`].
    pub fn octabox(&self, glyph: u16) -> Result<Option<Octabox>, Refusal> {
        let (start, end) = self.block(glyph)?;
        if start == end || !self.octaboxes() {
            return Ok(None);
        }
        let (octabox, _) = self.read_octabox(glyph, start, end)?;
        Ok(Some(octabox))
    }

    /// Whether each glyph's block opens with an octabox.
    const fn octaboxes(&self) -> bool {
        matches!(self.version, GlatVersion::Three { octaboxes: true })
    }

    /// Where `glyph`'s block starts and ends in `Glat`.
    fn block(&self, glyph: u16) -> Result<(usize, usize), Refusal> {
        let unreadable = Refusal::GlyphUnreadable { glyph };
        let at = usize::from(glyph);
        if at >= self.glyphs {
            return Err(unreadable);
        }
        let start = self.location(at).ok_or(unreadable)?;
        let end = self.location(at + 1).ok_or(unreadable)?;
        if start > end || end > self.glat.len() || (start < end && start < self.header) {
            return Err(unreadable);
        }
        Ok((start, end))
    }

    /// `Gloc`'s offset number `at`.
    fn location(&self, at: usize) -> Option<usize> {
        if self.long {
            let at = 8usize.checked_add(at.checked_mul(4)?)?;
            usize::try_from(self.gloc.u32(at)?).ok()
        } else {
            self.gloc.u16_at(8, at).map(usize::from)
        }
    }

    /// The octabox opening the block at `start`, and where its runs begin.
    fn read_octabox(
        &self,
        glyph: u16,
        start: usize,
        end: usize,
    ) -> Result<(Octabox, usize), Refusal> {
        let unreadable = Refusal::GlyphUnreadable { glyph };
        let block = self
            .glat
            .as_slice()
            .get(start..end)
            .map(Bytes::new)
            .ok_or(unreadable)?;
        let cells = block.u16(0).ok_or(unreadable)?;
        let mut diagonals = [0u8; 4];
        for (index, diagonal) in diagonals.iter_mut().enumerate() {
            *diagonal = block.u8(2 + index).ok_or(unreadable)?;
        }
        let mut subboxes = Vec::new();
        let mut at = 6usize;
        for cell in 0..16u8 {
            if cells & (1 << cell) == 0 {
                continue;
            }
            let byte = |offset: usize| block.u8(at + offset).ok_or(unreadable);
            subboxes.push(Subbox {
                cell,
                left: byte(0)?,
                right: byte(1)?,
                bottom: byte(2)?,
                top: byte(3)?,
                diagonals: [byte(4)?, byte(5)?, byte(6)?, byte(7)?],
            });
            at += 8;
        }
        Ok((
            Octabox {
                cells,
                diagonals,
                subboxes,
            },
            start + at,
        ))
    }

    /// Reads `glyph`'s whole block, handing each run's first number and its
    /// values to `visit`, after checking the block's every run is inside it
    /// and that there are no more of them than the ceiling.
    fn walk(&self, glyph: u16, mut visit: impl FnMut(u16, Bytes<'a>)) -> Result<(), Refusal> {
        let unreadable = Refusal::GlyphUnreadable { glyph };
        let (start, end) = self.block(glyph)?;
        let mut at = start;
        if start < end && self.octaboxes() {
            at = self.read_octabox(glyph, start, end)?.1;
        }
        let narrow = self.version == GlatVersion::One;
        let mut runs: Vec<(u16, Bytes<'a>)> = Vec::new();
        while at < end {
            let (first, count, values) = if narrow {
                let first = self.glat.u8(at).ok_or(unreadable)?;
                let count = self.glat.u8(at + 1).ok_or(unreadable)?;
                (u16::from(first), usize::from(count), at + 2)
            } else {
                let first = self.glat.u16(at).ok_or(unreadable)?;
                let count = self.glat.u16(at + 2).ok_or(unreadable)?;
                (first, usize::from(count), at + 4)
            };
            if values > end {
                return Err(unreadable);
            }
            let stop = values.checked_add(count * 2).ok_or(unreadable)?;
            if stop > end {
                return Err(unreadable);
            }
            if runs.len() >= usize::try_from(self.max_runs).unwrap_or(usize::MAX) {
                return Err(Refusal::TooManyAttributeRuns { glyph });
            }
            let bytes = self
                .glat
                .as_slice()
                .get(values..stop)
                .map(Bytes::new)
                .ok_or(unreadable)?;
            runs.push((first, bytes));
            at = stop;
        }
        for (first, values) in runs {
            visit(first, values);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Attributes, GlatVersion, Octabox, Subbox};
    use crate::graphite::{Refusal, GLAT, GLOC};
    use crate::limits::Limits;

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A `Gloc` of short offsets.
    fn gloc(offsets: &[u16], count: u16) -> Vec<u8> {
        let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
        out.extend(u16s(&[0, count]));
        out.extend(u16s(offsets));
        out
    }

    /// A `Glat` 2 of one glyph: two runs, attributes 2 and 3, and 7.
    fn glat_two() -> Vec<u8> {
        let mut out = 0x0002_0000u32.to_be_bytes().to_vec();
        out.extend(u16s(&[2, 2, 10, 20]));
        out.extend(u16s(&[7, 1, 0xFFFF]));
        out
    }

    #[test]
    fn a_glyphs_runs_answer_and_the_rest_are_zero() {
        let glat = glat_two();
        let gloc = gloc(&[4, 18], 8);
        let attributes = Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect("readable");
        assert_eq!(attributes.glyph_count(), 1);
        assert_eq!(attributes.version(), GlatVersion::Two);
        let answers: Vec<Option<i16>> = (0..9).map(|n| attributes.attribute(0, n)).collect();
        assert_eq!(
            answers,
            vec![
                Some(0),
                Some(0),
                Some(10),
                Some(20),
                Some(0),
                Some(0),
                Some(0),
                Some(-1),
                Some(0)
            ]
        );
        assert_eq!(
            attributes.runs(0),
            Ok(vec![(2, vec![10, 20]), (7, vec![-1])])
        );
        assert_eq!(attributes.attribute(1, 2), None, "a glyph past the offsets");
    }

    #[test]
    fn the_first_run_holding_a_number_wins() {
        let mut glat = 0x0002_0000u32.to_be_bytes().to_vec();
        glat.extend(u16s(&[5, 1, 1]));
        glat.extend(u16s(&[4, 2, 2, 3]));
        let gloc = gloc(&[4, 18], 6);
        let attributes = Attributes::parse(&gloc, &glat, &Limits::DEFAULT).expect("readable");
        assert_eq!(attributes.attribute(0, 5), Some(1));
        assert_eq!(attributes.attribute(0, 4), Some(2));
    }

    #[test]
    fn a_block_past_its_runs_ceiling_is_refused_whole() {
        let mut glat = 0x0002_0000u32.to_be_bytes().to_vec();
        for n in 0..4u16 {
            glat.extend(u16s(&[n * 2, 1, n]));
        }
        let gloc = gloc(&[4, 28], 8);
        let tight = Limits {
            max_attribute_runs: 3,
            ..Limits::DEFAULT
        };
        let attributes = Attributes::parse(&gloc, &glat, &tight).expect("readable");
        // Attribute 0 is in the first run, and the block is still refused:
        // the ceiling is the block's, not the question's.
        assert_eq!(
            attributes.lookup(0, 0),
            Err(Refusal::TooManyAttributeRuns { glyph: 0 })
        );
        let enough = Limits {
            max_attribute_runs: 4,
            ..Limits::DEFAULT
        };
        let attributes = Attributes::parse(&gloc, &glat, &enough).expect("readable");
        assert_eq!(attributes.lookup(0, 6), Ok(3));
    }

    #[test]
    fn versions_and_headers_it_cannot_read_are_refused() {
        let glat = glat_two();
        let mut bad_gloc = gloc(&[4, 18], 8);
        bad_gloc[1] = 2;
        assert_eq!(
            Attributes::parse(&bad_gloc, &glat, &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::UnknownVersion {
                table: GLOC,
                version: 0x0002_0000
            })
        );
        let mut bad_glat = glat.clone();
        bad_glat[1] = 4;
        assert_eq!(
            Attributes::parse(&gloc(&[4, 18], 8), &bad_glat, &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::UnknownVersion {
                table: GLAT,
                version: 0x0004_0000
            })
        );
        // A name array longer than the table.
        let mut named = gloc(&[4, 18], 8);
        named[5] = 2;
        assert_eq!(
            Attributes::parse(&named, &glat, &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::TableMalformed { table: GLOC })
        );
        // A `Glat` 3 that says it is compressed still.
        let mut still = 0x0003_0000u32.to_be_bytes().to_vec();
        still.extend((1u32 << 27).to_be_bytes());
        assert_eq!(
            Attributes::parse(&gloc(&[8, 8], 0), &still, &Limits::DEFAULT).map(|_| ()),
            Err(Refusal::UnknownCompression {
                table: GLAT,
                scheme: 1
            })
        );
    }

    #[test]
    fn an_octabox_opens_its_block_and_its_cells_are_in_bit_order() {
        let mut glat = 0x0003_0000u32.to_be_bytes().to_vec();
        glat.extend(1u32.to_be_bytes());
        let mut block = u16s(&[(1 << 3) | (1 << 12)]);
        block.extend([1, 2, 3, 4]);
        block.extend([10, 11, 12, 13, 14, 15, 16, 17]);
        block.extend([20, 21, 22, 23, 24, 25, 26, 27]);
        block.extend(u16s(&[0, 1, 99]));
        glat.extend(&block);
        let end = u16::try_from(glat.len()).expect("small");
        let index = gloc(&[8, end], 1);
        let attributes = Attributes::parse(&index, &glat, &Limits::DEFAULT).expect("readable");
        assert_eq!(attributes.version(), GlatVersion::Three { octaboxes: true });
        let octabox = attributes
            .octabox(0)
            .expect("readable")
            .expect("an octabox");
        assert_eq!(
            octabox,
            Octabox {
                cells: (1 << 3) | (1 << 12),
                diagonals: [1, 2, 3, 4],
                subboxes: vec![
                    Subbox {
                        cell: 3,
                        left: 10,
                        right: 11,
                        bottom: 12,
                        top: 13,
                        diagonals: [14, 15, 16, 17],
                    },
                    Subbox {
                        cell: 12,
                        left: 20,
                        right: 21,
                        bottom: 22,
                        top: 23,
                        diagonals: [24, 25, 26, 27],
                    },
                ],
            }
        );
        assert_eq!(
            (
                octabox.sum_min(),
                octabox.sum_max(),
                octabox.difference_min(),
                octabox.difference_max()
            ),
            (1, 2, 3, 4)
        );
        assert_eq!(attributes.attribute(0, 0), Some(99));
        // An octabox whose cells run past the block leaves the glyph
        // unreadable.
        let short = gloc(&[8, end - 7], 1);
        let attributes = Attributes::parse(&short, &glat, &Limits::DEFAULT).expect("readable");
        assert_eq!(
            attributes.lookup(0, 0),
            Err(Refusal::GlyphUnreadable { glyph: 0 })
        );
    }
}
