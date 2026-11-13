//! `CPAL`, the colour palette table: the palettes of sRGB colours a colour
//! glyph's layers and paints index (OpenType 1.9.1, the CPAL chapter).
//!
//! A palette is `numPaletteEntries` colour records, every palette the same
//! length; all palettes' records sit in one array, and each palette names
//! where its own run starts (*Palette entries and color records*). Version 1
//! adds three arrays — each palette's type flags, each palette's label and
//! each entry's label, both `name` table IDs — each present only where its
//! offset is not zero (*Palette table header*).
//!
//! Every read is held to the table's own bytes: a lookup past a count, or past
//! the end of the data, answers `None` rather than a colour (ruling 1). What
//! the colours are *for* — the layers of `COLR`, the variables of an SVG
//! glyph — is [`crate::colr`]'s and the writer's, which is why this hands back
//! plain colours and indices and no glyph vocabulary (ruling 8).
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

/// One colour, as a palette entry holds it: sRGB, its components not
/// premultiplied by its alpha (*Palette entries and color records*: "The
/// colors in the color record should not be pre-multiplied").
///
/// The table writes it blue, green, red, alpha; the fields name the
/// components, so no caller reorders them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red, 0 to 255, in sRGB's encoding.
    pub red: u8,
    /// Green, likewise.
    pub green: u8,
    /// Blue, likewise.
    pub blue: u8,
    /// Alpha, 0 transparent to 255 opaque.
    pub alpha: u8,
}

/// A palette's type flags (*Palette types array*): a `uint32` of which two
/// bits are defined, kept whole so a reserved bit a later version defines is
/// not lost on the way through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PaletteType(pub u32);

impl PaletteType {
    /// Bit 0, `USABLE_WITH_LIGHT_BACKGROUND`.
    pub const LIGHT_BACKGROUND: u32 = 0x0001;
    /// Bit 1, `USABLE_WITH_DARK_BACKGROUND`.
    pub const DARK_BACKGROUND: u32 = 0x0002;

    /// Whether the palette suits a light background, such as white paper.
    #[must_use]
    pub fn usable_with_light_background(self) -> bool {
        self.0 & Self::LIGHT_BACKGROUND != 0
    }

    /// Whether the palette suits a dark background.
    #[must_use]
    pub fn usable_with_dark_background(self) -> bool {
        self.0 & Self::DARK_BACKGROUND != 0
    }
}

/// Which palette a colour glyph is drawn in (`docs/design/colour-fonts.md`,
/// *CPAL and the palette*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PaletteChoice {
    /// Palette 0, the default palette.
    #[default]
    Default,
    /// The palette at this index, or palette 0 where the table has fewer.
    Index(u16),
    /// The first palette whose type says it suits a light background, or
    /// palette 0 where none does.
    LightBackground,
    /// The first palette whose type says it suits a dark background, or
    /// palette 0 where none does.
    DarkBackground,
}

/// A parsed `CPAL` table over borrowed bytes.
#[derive(Clone, Copy, Debug)]
pub struct Cpal<'a> {
    data: &'a [u8],
    version: u16,
    entries: u16,
    palettes: u16,
    records: u16,
    /// Where the colour records start.
    records_at: usize,
    /// Version 1's arrays, each where its offset is not zero and the whole
    /// array lies inside the table.
    types_at: Option<usize>,
    labels_at: Option<usize>,
    entry_labels_at: Option<usize>,
}

/// Where an array of `count` items of `size` bytes starts, if `offset` is not
/// zero and the whole array lies inside `data`.
fn array_at(data: &[u8], offset: u32, count: u16, size: usize) -> Option<usize> {
    let at = usize::try_from(offset).ok().filter(|&at| at != 0)?;
    let end = at.checked_add(usize::from(count).checked_mul(size)?)?;
    (end <= data.len()).then_some(at)
}

impl<'a> Cpal<'a> {
    /// Parses the header, and the version 1 offsets where there are any.
    ///
    /// `None` for a version other than 0 or 1, and for a header — the
    /// palettes' first indices included — that runs past the data. The colour
    /// records are checked a lookup at a time, so a table whose records run
    /// short still answers for the ones it has.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Cpal<'a>> {
        let version = be16(data, 0)?;
        if version > 1 {
            return None;
        }
        let entries = be16(data, 2)?;
        let palettes = be16(data, 4)?;
        let records = be16(data, 6)?;
        let records_at = usize::try_from(be32(data, 8)?).ok()?;
        // `colorRecordIndices[numPalettes]`, then version 1's three offsets.
        let indices_end = 12usize.checked_add(usize::from(palettes) * 2)?;
        if indices_end > data.len() {
            return None;
        }
        let (mut types_at, mut labels_at, mut entry_labels_at) = (None, None, None);
        if version == 1 {
            let types = be32(data, indices_end)?;
            let labels = be32(data, indices_end + 4)?;
            let entry_labels = be32(data, indices_end + 8)?;
            types_at = array_at(data, types, palettes, 4);
            labels_at = array_at(data, labels, palettes, 2);
            entry_labels_at = array_at(data, entry_labels, entries, 2);
        }
        Some(Cpal {
            data,
            version,
            entries,
            palettes,
            records,
            records_at,
            types_at,
            labels_at,
            entry_labels_at,
        })
    }

    /// The table's version, 0 or 1.
    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    /// `numPalettes`.
    #[must_use]
    pub fn palette_count(&self) -> u16 {
        self.palettes
    }

    /// `numPaletteEntries`: how many colours each palette has.
    #[must_use]
    pub fn entry_count(&self) -> u16 {
        self.entries
    }

    /// Entry `entry` of palette `palette`.
    ///
    /// `None` for a palette or entry past its count, for a record index —
    /// the palette's first index plus the entry — past `numColorRecords`, and
    /// for a record past the table's end. 0xFFFF is no entry here: the
    /// foreground it stands for in `COLR` is [`crate::colr::ColourIndex`]'s.
    #[must_use]
    pub fn colour(&self, palette: u16, entry: u16) -> Option<Rgba> {
        if palette >= self.palettes || entry >= self.entries {
            return None;
        }
        let first = be16(self.data, 12 + usize::from(palette) * 2)?;
        let index = u32::from(first) + u32::from(entry);
        if index >= u32::from(self.records) {
            return None;
        }
        let at = self
            .records_at
            .checked_add(usize::try_from(index).ok()?.checked_mul(4)?)?;
        let record = self.data.get(at..at.checked_add(4)?)?;
        Some(Rgba {
            blue: record[0],
            green: record[1],
            red: record[2],
            alpha: record[3],
        })
    }

    /// Palette `palette`'s type flags, where the table has a types array.
    #[must_use]
    pub fn palette_type(&self, palette: u16) -> Option<PaletteType> {
        if palette >= self.palettes {
            return None;
        }
        let at = self.types_at?.checked_add(usize::from(palette) * 4)?;
        be32(self.data, at).map(PaletteType)
    }

    /// Palette `palette`'s label, a `name` table ID; `None` where the table has
    /// no labels array, and for 0xFFFF, which the *Palette labels array* says
    /// means no string.
    #[must_use]
    pub fn palette_label(&self, palette: u16) -> Option<u16> {
        if palette >= self.palettes {
            return None;
        }
        let at = self.labels_at?.checked_add(usize::from(palette) * 2)?;
        be16(self.data, at).filter(|&id| id != 0xFFFF)
    }

    /// Entry `entry`'s label, the same in every palette (*Palette entry label
    /// array*); `None` as [`Cpal::palette_label`] answers it.
    #[must_use]
    pub fn entry_label(&self, entry: u16) -> Option<u16> {
        if entry >= self.entries {
            return None;
        }
        let at = self.entry_labels_at?.checked_add(usize::from(entry) * 2)?;
        be16(self.data, at).filter(|&id| id != 0xFFFF)
    }

    /// The palette `choice` picks: always one of this table's, or 0.
    #[must_use]
    pub fn choose(&self, choice: PaletteChoice) -> u16 {
        let flagged = |wanted: fn(PaletteType) -> bool| {
            (0..self.palettes)
                .find(|&palette| self.palette_type(palette).is_some_and(wanted))
                .unwrap_or(0)
        };
        match choice {
            PaletteChoice::Default => 0,
            PaletteChoice::Index(palette) if palette < self.palettes => palette,
            PaletteChoice::Index(_) => 0,
            PaletteChoice::LightBackground => flagged(PaletteType::usable_with_light_background),
            PaletteChoice::DarkBackground => flagged(PaletteType::usable_with_dark_background),
        }
    }
}
