//! `sbix`, the standard bitmap graphics table: a glyph as a PNG, JPEG or TIFF
//! graphic per strike (OpenType 1.9.1, the sbix chapter).
//!
//! A strike is one pixel size, `ppem`, designed for one density, `ppi`; it
//! holds an offset for every glyph and one more, and a glyph's data is what
//! lies between its offset and the next (*Strikes*). The data is two origin
//! offsets, a graphic type and the graphic's own bytes (*Glyph data*).
//!
//! The graphic is handed back as bytes with its type, undecoded: decoding is
//! the drawing step's. Every offset is held to the table (ruling 1), and a
//! graphic type the chapter does not define is refused by name (ruling 2).
//! [`StrikeChoice`] is the one rule both this table and `CBLC`/`CBDT` choose a
//! strike by.
//!
//! Design: `docs/design/colour-fonts.md`.

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Which strike a bitmap glyph is drawn at (`docs/design/colour-fonts.md`,
/// *The strike a glyph is drawn at*).
///
/// Every option chooses among the strikes that hold the glyph — the *Strikes*
/// section of the sbix chapter makes a bitmap binding wherever any strike has
/// one — and strikes of one size are taken in table order, the first winning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StrikeChoice {
    /// The most pixels per em.
    Largest,
    /// The fewest pixels per em of at least this many; the largest where none
    /// reaches it — the "closest available larger size", which CBDT's
    /// *Scaling behavior* recommends.
    AtLeast(u16),
    /// The fewest pixels per em that are a whole multiple of this many — the
    /// "closest available integer-multiple larger size"; `AtLeast`'s strike
    /// where none is.
    Multiple(u16),
    /// The least distance from this many pixels per em, the larger on a tie.
    Nearest(u16),
}

/// The index `choice` picks among `candidates`, each a strike's index and its
/// pixels per em, in table order.
pub(crate) fn choose(candidates: &[(usize, u16)], choice: StrikeChoice) -> Option<usize> {
    let largest = || {
        let mut best: Option<(usize, u16)> = None;
        for &(index, ppem) in candidates {
            if best.is_none_or(|(_, size)| ppem > size) {
                best = Some((index, ppem));
            }
        }
        best.map(|(index, _)| index)
    };
    let smallest_where = |keep: &dyn Fn(u16) -> bool| {
        let mut best: Option<(usize, u16)> = None;
        for &(index, ppem) in candidates {
            if keep(ppem) && best.is_none_or(|(_, size)| ppem < size) {
                best = Some((index, ppem));
            }
        }
        best.map(|(index, _)| index)
    };
    match choice {
        StrikeChoice::Largest => largest(),
        StrikeChoice::AtLeast(wanted) => smallest_where(&|ppem| ppem >= wanted).or_else(largest),
        StrikeChoice::Multiple(wanted) => (wanted > 0)
            .then(|| smallest_where(&|ppem| ppem >= wanted && ppem % wanted == 0))
            .flatten()
            .or_else(|| choose(candidates, StrikeChoice::AtLeast(wanted))),
        StrikeChoice::Nearest(wanted) => {
            let mut best: Option<(usize, u16, u16)> = None;
            for &(index, ppem) in candidates {
                let distance = ppem.abs_diff(wanted);
                let better = best.is_none_or(|(_, size, gap)| {
                    distance < gap || (distance == gap && ppem > size)
                });
                if better {
                    best = Some((index, ppem, distance));
                }
            }
            best.map(|(index, _, _)| index)
        }
    }
}

/// A graphic type the chapter defines (*Glyph data*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Graphic {
    /// `png `.
    Png,
    /// `jpg `.
    Jpeg,
    /// `tiff`.
    Tiff,
}

/// Why a glyph's data is no bitmap this reader hands back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SbixRefusal {
    /// A graphic type the chapter does not define: `mask` and `pdf `, which
    /// *Glyph data* names as not supported in OpenType, and every other tag.
    GraphicType([u8; 4]),
    /// A `dupe` naming a glyph whose own data is a `dupe`: the chapter states
    /// one step, and a chain is refused rather than followed.
    DupeChain,
    /// A `dupe` naming a glyph past the face's count or one with no data, or
    /// too short to name one.
    DupeTarget,
    /// The glyph's data runs backwards, past the table, or is shorter than its
    /// eight-byte header.
    Malformed,
}

/// One strike's sizes (*Strikes*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SbixStrike {
    /// The pixels per em the strike was designed for.
    pub ppem: u16,
    /// The device density, in pixels per inch, it was designed for.
    pub ppi: u16,
}

/// A glyph's bitmap in one strike, as the table writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SbixGlyph<'a> {
    /// `originOffsetX`: the graphic's left edge from the glyph's origin, in
    /// the strike's pixels (`docs/design/colour-fonts.md`, *sbix*).
    pub origin_x: i16,
    /// `originOffsetY`: its bottom edge from the origin.
    pub origin_y: i16,
    /// The graphic's type.
    pub graphic: Graphic,
    /// The graphic's bytes, undecoded.
    pub data: &'a [u8],
    /// The glyph whose record this is: the glyph asked for, or the one its
    /// `dupe` named, whose origin offsets and bytes these are.
    pub from: u16,
}

/// One glyph's data as a strike holds it, before its tag is read.
struct Record<'a> {
    origin_x: i16,
    origin_y: i16,
    tag: [u8; 4],
    bytes: &'a [u8],
}

/// A parsed `sbix` table over borrowed bytes.
#[derive(Clone, Debug)]
pub struct Sbix<'a> {
    data: &'a [u8],
    glyph_count: u16,
    flags: u16,
    /// Each strike's offset in the table and its sizes, in table order.
    strikes: Vec<(usize, SbixStrike)>,
}

impl<'a> Sbix<'a> {
    /// Parses the header and every strike's header, for a face of
    /// `glyph_count` glyphs (`maxp`'s, *Table dependencies*).
    ///
    /// `None` for a version other than 1, and where the strike offsets, or any
    /// strike's sizes and its `glyph_count + 1` glyph offsets, run past the
    /// data: a strike whose offsets cannot be read places no glyph.
    #[must_use]
    pub fn parse(data: &'a [u8], glyph_count: u16) -> Option<Sbix<'a>> {
        if be16(data, 0)? != 1 {
            return None;
        }
        let flags = be16(data, 2)?;
        let count = be32(data, 4)?;
        let offsets_end = 8u64 + u64::from(count) * 4;
        if offsets_end > u64::try_from(data.len()).ok()? {
            return None;
        }
        let per_strike = 4 + (usize::from(glyph_count) + 1) * 4;
        let mut strikes = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let at = usize::try_from(be32(data, 8 + i * 4)?).ok()?;
            if at.checked_add(per_strike)? > data.len() {
                return None;
            }
            strikes.push((
                at,
                SbixStrike {
                    ppem: be16(data, at)?,
                    ppi: be16(data, at + 2)?,
                },
            ));
        }
        Some(Sbix {
            data,
            glyph_count,
            flags,
            strikes,
        })
    }

    /// The header's `flags`.
    #[must_use]
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Whether bit 1 of `flags` asks for the outline drawn over the bitmap
    /// (*Header*).
    #[must_use]
    pub fn draws_outlines(&self) -> bool {
        self.flags & 0x0002 != 0
    }

    /// Every strike's sizes, in table order; a strike's index here is the one
    /// [`Sbix::glyph`] takes.
    #[must_use]
    pub fn strikes(&self) -> Vec<SbixStrike> {
        self.strikes.iter().map(|(_, strike)| *strike).collect()
    }

    /// `glyph`'s own record in strike `strike`; `Ok(None)` where it has no
    /// data.
    fn record(&self, strike: usize, glyph: u16) -> Result<Option<Record<'a>>, SbixRefusal> {
        let Some(&(at, _)) = self.strikes.get(strike) else {
            return Ok(None);
        };
        if glyph >= self.glyph_count {
            return Ok(None);
        }
        let offset = |g: usize| be32(self.data, at + 4 + g * 4).ok_or(SbixRefusal::Malformed);
        let start = offset(usize::from(glyph))?;
        let end = offset(usize::from(glyph) + 1)?;
        if end < start {
            return Err(SbixRefusal::Malformed);
        }
        if end == start {
            return Ok(None);
        }
        let from = at
            .checked_add(usize::try_from(start).map_err(|_| SbixRefusal::Malformed)?)
            .ok_or(SbixRefusal::Malformed)?;
        let to = at
            .checked_add(usize::try_from(end).map_err(|_| SbixRefusal::Malformed)?)
            .ok_or(SbixRefusal::Malformed)?;
        let bytes = self.data.get(from..to).ok_or(SbixRefusal::Malformed)?;
        if bytes.len() < 8 {
            return Err(SbixRefusal::Malformed);
        }
        Ok(Some(Record {
            origin_x: i16::from_be_bytes([bytes[0], bytes[1]]),
            origin_y: i16::from_be_bytes([bytes[2], bytes[3]]),
            tag: [bytes[4], bytes[5], bytes[6], bytes[7]],
            bytes: &bytes[8..],
        }))
    }

    /// `glyph`'s bitmap in strike `strike`.
    ///
    /// `Ok(None)` where the strike holds no data for it — equal offsets, which
    /// *Strikes* says is no bitmap — and for a strike or glyph past its count.
    /// A `dupe` is resolved once, in the same strike, to the glyph it names,
    /// whose origin offsets and bytes come back with [`SbixGlyph::from`]
    /// naming it.
    pub fn glyph(&self, strike: usize, glyph: u16) -> Result<Option<SbixGlyph<'a>>, SbixRefusal> {
        let Some(record) = self.record(strike, glyph)? else {
            return Ok(None);
        };
        let graphic = |tag: [u8; 4]| match &tag {
            b"png " => Ok(Graphic::Png),
            b"jpg " => Ok(Graphic::Jpeg),
            b"tiff" => Ok(Graphic::Tiff),
            _ => Err(SbixRefusal::GraphicType(tag)),
        };
        if &record.tag != b"dupe" {
            return Ok(Some(SbixGlyph {
                origin_x: record.origin_x,
                origin_y: record.origin_y,
                graphic: graphic(record.tag)?,
                data: record.bytes,
                from: glyph,
            }));
        }
        // *Glyph data*: "the data field contains a uint16, big-endian glyph
        // ID. The bitmap data for the indicated glyph must be used".
        let target = match record.bytes {
            [high, low, ..] => u16::from_be_bytes([*high, *low]),
            _ => return Err(SbixRefusal::DupeTarget),
        };
        if target >= self.glyph_count {
            return Err(SbixRefusal::DupeTarget);
        }
        let Some(named) = self.record(strike, target)? else {
            return Err(SbixRefusal::DupeTarget);
        };
        if &named.tag == b"dupe" {
            return Err(SbixRefusal::DupeChain);
        }
        Ok(Some(SbixGlyph {
            origin_x: named.origin_x,
            origin_y: named.origin_y,
            graphic: graphic(named.tag)?,
            data: named.bytes,
            from: target,
        }))
    }

    /// Whether strike `strike` holds a bitmap for `glyph` this reader hands
    /// back — a `dupe` that resolves holds it.
    #[must_use]
    pub fn holds(&self, strike: usize, glyph: u16) -> bool {
        matches!(self.glyph(strike, glyph), Ok(Some(_)))
    }

    /// The strike `choice` picks for `glyph`, among those that hold it; `None`
    /// where none does.
    #[must_use]
    pub fn choose_strike(&self, glyph: u16, choice: StrikeChoice) -> Option<usize> {
        let candidates: Vec<(usize, u16)> = self
            .strikes
            .iter()
            .enumerate()
            .filter(|(index, _)| self.holds(*index, glyph))
            .map(|(index, (_, strike))| (index, strike.ppem))
            .collect();
        choose(&candidates, choice)
    }
}
