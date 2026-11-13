//! Named colours (`docs/design/icc.md`, "Named colours"): ICC.1's
//! `namedColor2Type` (`ncl2`, ICC.1:2010 10.15), a list of colours each with
//! a name, connection-space coordinates and, optionally, the device's own.
//!
//! The tag is what a profile of class `nmcl` (8.9) carries, and any other
//! profile may carry it beside its transforms. Its layout (Table 63): a
//! vendor flag, the count of colours `n`, the count of device coordinates a
//! colour `m`, a 32-byte prefix and a 32-byte suffix, then `n` entries of
//! `38 + 2m` bytes — a 32-byte root name, three connection-space coordinates
//! and `m` device coordinates, each a 16-bit value. A colour's name is its
//! prefix, root and suffix joined; each field is 7-bit ASCII ended by a NUL.
//!
//! The connection-space coordinates are relative colorimetric, in the
//! encoding `lut16Type`'s are (Table 63): XYZ in `u1Fixed15`, Lab in version
//! 2's legacy 16-bit encoding, `L*` 100 at `FF00h`. A device coordinate runs
//! from `0000h`, the device's least, to `FFFFh`, its most.
//!
//! # Every count is bounded before an entry is read
//!
//! The colour count and the device coordinate count are both 32-bit file
//! fields, and their product sizes the entries (ruling 1). The device count is
//! held to [`MAX_DEVICE_CHANNELS`], the colour count to [`MAX_NAMED_COLOURS`],
//! and `n × (38 + 2m)` to the tag's own bytes, before any entry is walked.

use super::{pcs_to_rgb, pcs_to_rgb16, IccError, Reader, MAX_DEVICE_CHANNELS};

/// The most colours one `ncl2` tag may name.
///
/// The count is a 32-bit field read before the entries it describes, and each
/// entry is at least 38 bytes, so the tag's own size bounds what can be read;
/// this bounds what can be *declared*, and is checked first, before the
/// product of the two counts is formed.
///
/// | | Colours |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 536 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 0 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **65 536** |
///
/// The zeros are facts about those formats: no comic page or book picture
/// carries a profile this parser opens, and a fixed document's profiles
/// convert colours — an `ICCBased` space's profile is never of class `nmcl`
/// (ISO 32000-2 Table 67), and neither is an XPS `ContextColor`'s. The
/// fixture is the test that declares exactly this many and is walked.
///
/// Reachable: the count is a 32-bit field at offset 12 of the tag, so an
/// 84-byte header may declare 4 294 967 295 colours;
/// `a_named_colour_count_past_the_cap_is_refused_before_an_entry_is_read`
/// builds one past the cap.
pub const MAX_NAMED_COLOURS: usize = 1 << 16;

/// Bytes a name field holds, its NUL among them (Table 63).
const NAME_FIELD: usize = 32;

/// Where the first entry starts: after the header, the counts, the prefix and
/// the suffix (Table 63).
const ENTRIES_AT: usize = 84;

/// The colours an `ncl2` tag names.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedColours {
    /// The vendor's flag, whose low 16 bits ICC.1 reserves.
    vendor: u32,
    /// The prefix every name begins with.
    prefix: String,
    /// The suffix every name ends with.
    suffix: String,
    /// The device coordinates each colour carries, `m`: none where the tag
    /// gives none.
    device_channels: usize,
    /// The colours, in the tag's order.
    colours: Vec<NamedColour>,
}

/// One named colour.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedColour {
    /// Its whole name: the tag's prefix, its root and the tag's suffix.
    name: String,
    /// Its connection-space coordinates in their own units: D50 XYZ with the
    /// PCS white's `Y` at one, or `L*`, `a*` and `b*` as they stand.
    pcs: [f64; 3],
    /// Whether the connection space is `Lab` rather than `XYZ`.
    lab: bool,
    /// Its device coordinates, each from 0 to 65 535.
    device: Vec<u16>,
}

impl NamedColours {
    /// The colours `data`, an `ncl2` tag, names, its connection-space
    /// coordinates read in `lab`'s encoding: Lab where `lab`, XYZ otherwise.
    ///
    /// # Errors
    ///
    /// [`IccError::TooLarge`] for a device coordinate count past
    /// [`MAX_DEVICE_CHANNELS`] or a colour count past [`MAX_NAMED_COLOURS`],
    /// before any entry is read; [`IccError::MalformedTag`] for a tag that is
    /// not `ncl2`, entries its bytes cannot hold, or a name field holding a
    /// byte past 7 bits or no NUL.
    pub fn read(data: &[u8], lab: bool) -> Result<NamedColours, IccError> {
        let reader = Reader { bytes: data };
        if reader.sig(0) != Some(*b"ncl2") {
            return Err(IccError::MalformedTag);
        }
        let vendor = reader.u32(8).ok_or(IccError::MalformedTag)?;
        let count = reader.u32(12).ok_or(IccError::MalformedTag)? as usize;
        let device_channels = reader.u32(16).ok_or(IccError::MalformedTag)? as usize;
        if device_channels > MAX_DEVICE_CHANNELS || count > MAX_NAMED_COLOURS {
            return Err(IccError::TooLarge);
        }
        // Both counts are bounded, so neither product can overflow.
        let entry = NAME_FIELD + 6 + 2 * device_channels;
        let end = ENTRIES_AT + count * entry;
        if end > data.len() {
            return Err(IccError::MalformedTag);
        }
        let prefix = name_field(data, 20)?;
        let suffix = name_field(data, 52)?;
        let mut colours = Vec::with_capacity(count);
        for index in 0..count {
            let at = ENTRIES_AT + index * entry;
            let root = name_field(data, at)?;
            let coordinate = |k: usize| {
                reader
                    .u16(at + NAME_FIELD + 2 * k)
                    .ok_or(IccError::MalformedTag)
            };
            let encoded = [coordinate(0)?, coordinate(1)?, coordinate(2)?];
            let mut device = Vec::with_capacity(device_channels);
            for channel in 0..device_channels {
                device.push(coordinate(3 + channel)?);
            }
            colours.push(NamedColour {
                name: format!("{prefix}{root}{suffix}"),
                pcs: decode(encoded, lab),
                lab,
                device,
            });
        }
        Ok(NamedColours {
            vendor,
            prefix,
            suffix,
            device_channels,
            colours,
        })
    }

    /// How many colours the tag names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.colours.len()
    }

    /// Whether the tag names none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colours.is_empty()
    }

    /// The colour named `name` — the tag's prefix, a root and its suffix
    /// joined — or `None` where the tag names no such colour. Where two
    /// entries share a name, the first.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&NamedColour> {
        self.colours.iter().find(|colour| colour.name == name)
    }

    /// The colours, in the tag's order.
    pub fn iter(&self) -> impl Iterator<Item = &NamedColour> {
        self.colours.iter()
    }

    /// The prefix every name begins with.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The suffix every name ends with.
    #[must_use]
    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// The vendor's flag.
    #[must_use]
    pub fn vendor(&self) -> u32 {
        self.vendor
    }

    /// How many device coordinates each colour carries: none where the tag
    /// gives none.
    #[must_use]
    pub fn device_channels(&self) -> usize {
        self.device_channels
    }

    /// The most bytes the colours hold beside their own size.
    pub(super) fn held_bytes(&self) -> usize {
        let names = self.prefix.capacity() + self.suffix.capacity();
        self.colours
            .iter()
            .map(|colour| {
                std::mem::size_of::<NamedColour>()
                    + colour.name.capacity()
                    + colour.device.capacity() * std::mem::size_of::<u16>()
            })
            .fold(names, usize::saturating_add)
    }
}

impl NamedColour {
    /// Its whole name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its connection-space coordinates in the profile's own connection
    /// space, relative colorimetric: D50 XYZ with the PCS white's `Y` at one,
    /// or `L*`, `a*` and `b*` as they stand.
    #[must_use]
    pub fn pcs(&self) -> [f64; 3] {
        self.pcs
    }

    /// Whether its connection space is `Lab` rather than `XYZ`.
    #[must_use]
    pub fn lab(&self) -> bool {
        self.lab
    }

    /// Its device coordinates, each from 0 to 65 535, in the order the data
    /// colour space lists its channels; empty where the tag gives none.
    #[must_use]
    pub fn device(&self) -> &[u16] {
        &self.device
    }

    /// The colour as sRGB, through the connection-space paths a table's value
    /// takes: Lab to D50 XYZ by 6.4's relation, then the D50-to-sRGB matrix,
    /// encoded as that connection space's light is.
    #[must_use]
    pub fn to_rgb(&self) -> (u8, u8, u8) {
        pcs_to_rgb(self.pcs, self.lab, None)
    }

    /// [`NamedColour::to_rgb`] at sixteen bits a channel, each held to the
    /// byte `to_rgb` gives it.
    #[must_use]
    pub fn to_rgb16(&self) -> [u16; 3] {
        pcs_to_rgb16(self.pcs, self.lab, None)
    }
}

/// A name field at `at`: the bytes before its NUL, which 7-bit ASCII and a
/// NUL within the field's 32 bytes make a string.
fn name_field(data: &[u8], at: usize) -> Result<String, IccError> {
    let field = data
        .get(at..at + NAME_FIELD)
        .ok_or(IccError::MalformedTag)?;
    let length = field
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(IccError::MalformedTag)?;
    let text = &field[..length];
    if !text.is_ascii() {
        return Err(IccError::MalformedTag);
    }
    Ok(text.iter().map(|&byte| char::from(byte)).collect())
}

/// Three coordinates in `lut16Type`'s encoding (Table 63, 10.8) as the
/// connection-space value they encode: XYZ in `u1Fixed15`, 1.0 at `8000h`;
/// Lab in the legacy encoding, `L*` 100 at `FF00h` and `a*`, `b*` 0 at
/// `8000h`.
fn decode(encoded: [u16; 3], lab: bool) -> [f64; 3] {
    let unit = encoded.map(|value| f64::from(value) / 65280.0);
    if lab {
        [
            unit[0] * 100.0,
            unit[1] * 255.0 - 128.0,
            unit[2] * 255.0 - 128.0,
        ]
    } else {
        encoded.map(|value| f64::from(value) / 32768.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name field of `text`, NUL-padded to 32 bytes.
    fn field(text: &str) -> Vec<u8> {
        let mut out = text.as_bytes().to_vec();
        out.resize(NAME_FIELD, 0);
        out
    }

    /// An `ncl2` tag's header declaring `count` colours of `device`
    /// coordinates, with `prefix` and `suffix`, and no entries.
    fn header(count: u32, device: u32, prefix: &str, suffix: &str) -> Vec<u8> {
        let mut out = b"ncl2".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&device.to_be_bytes());
        out.extend_from_slice(&field(prefix));
        out.extend_from_slice(&field(suffix));
        out
    }

    /// One entry: a root name, three coordinates and the device's.
    fn entry(root: &str, pcs: [u16; 3], device: &[u16]) -> Vec<u8> {
        let mut out = field(root);
        for value in pcs.iter().chain(device) {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out
    }

    /// **A named colour count past the cap is refused before an entry is
    /// read** (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs`
    /// names as proving [`MAX_NAMED_COLOURS`]: an 84-byte tag — a header and
    /// nothing after it — declaring one colour past the cap is
    /// `IccError::TooLarge`, where a reader that walked first would find the
    /// entries' bytes missing and answer `MalformedTag`; and one declaring
    /// exactly the cap is admitted to that check, and refused by it. A device
    /// coordinate count past fifteen is refused the same way, first.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_named_colour_count_past_the_cap_is_refused_before_an_entry_is_read() {
        let past = header(MAX_NAMED_COLOURS as u32 + 1, 0, "", "");
        assert_eq!(past.len(), 84);
        assert_eq!(
            NamedColours::read(&past, false),
            Err(IccError::TooLarge),
            "one colour past the cap is refused by name"
        );
        assert_eq!(
            NamedColours::read(&header(u32::MAX, u32::MAX, "", ""), false),
            Err(IccError::TooLarge),
            "four billion colours of four billion coordinates, in 84 bytes"
        );
        let at = header(MAX_NAMED_COLOURS as u32, 0, "", "");
        assert_eq!(
            NamedColours::read(&at, false),
            Err(IccError::MalformedTag),
            "the cap itself is walked, and its entries are not there"
        );
        assert_eq!(
            NamedColours::read(&header(1, MAX_DEVICE_CHANNELS as u32 + 1, "", ""), false),
            Err(IccError::TooLarge),
            "sixteen device coordinates"
        );
    }

    /// **A tag names its colours by prefix, root and suffix**, each field
    /// ended by its NUL: a lookup by the whole name finds the colour, a root
    /// alone finds nothing, and the first of two entries sharing a name is
    /// the one found. A tag that is not `ncl2`, one whose entries its bytes
    /// cannot hold, one with a name field holding a byte past 7 bits, and one
    /// with a field of no NUL are each malformed. A tag naming no colour
    /// reads, empty.
    #[test]
    fn a_tag_names_its_colours_by_prefix_root_and_suffix() {
        let mut tag = header(3, 2, "Ink ", " (coated)");
        tag.extend(entry("Red", [0x4000, 0x2000, 0x1000], &[0xFFFF, 0]));
        tag.extend(entry("Blue", [0x1000, 0x2000, 0x6000], &[0, 0xFFFF]));
        tag.extend(entry("Red", [0x0001, 0x0002, 0x0003], &[1, 2]));
        let colours = NamedColours::read(&tag, false).expect("the tag reads");
        assert_eq!(colours.len(), 3);
        assert_eq!((colours.prefix(), colours.suffix()), ("Ink ", " (coated)"));
        assert_eq!(
            (colours.vendor(), colours.device_channels()),
            (0x0001_0000, 2)
        );
        let red = colours.get("Ink Red (coated)").expect("the whole name");
        assert_eq!(red.device(), &[0xFFFF, 0]);
        assert_eq!(red.pcs(), [0.5, 0.25, 0.125], "u1Fixed15: 8000h is 1.0");
        assert!(colours.get("Red").is_none(), "a root alone");
        assert!(colours.get("Ink Green (coated)").is_none());
        assert_eq!(
            colours.iter().map(NamedColour::name).collect::<Vec<_>>(),
            ["Ink Red (coated)", "Ink Blue (coated)", "Ink Red (coated)"]
        );

        let mut short = tag.clone();
        short.truncate(tag.len() - 1);
        let mut not_ncl2 = tag.clone();
        not_ncl2[..4].copy_from_slice(b"ncl1");
        let mut eight_bit = tag.clone();
        eight_bit[ENTRIES_AT] = 0xC9;
        let mut unended = tag.clone();
        unended[20..52].fill(b'x');
        for (bytes, what) in [
            (short, "entries cut short"),
            (not_ncl2, "another type"),
            (eight_bit, "a name byte past 7 bits"),
            (unended, "a prefix with no NUL"),
        ] {
            assert_eq!(
                NamedColours::read(&bytes, false),
                Err(IccError::MalformedTag),
                "{what}"
            );
        }
        let empty = NamedColours::read(&header(0, 0, "", ""), true).expect("no colours");
        assert!(empty.is_empty());
    }

    /// **The coordinates read in `lut16Type`'s encoding** (Table 63, 10.8):
    /// XYZ in `u1Fixed15`, Lab in the legacy encoding — `FF00h` is `L*` 100,
    /// `8000h` is `a*` 0, `FFFFh` reads a little past the space — and the
    /// sRGB is the connection space's own path's.
    #[test]
    fn the_coordinates_read_in_the_legacy_encoding() {
        let mut tag = header(2, 0, "", "");
        tag.extend(entry("white", [0xFF00, 0x8000, 0x8000], &[]));
        tag.extend(entry("past", [0xFFFF, 0xFFFF, 0], &[]));
        let colours = NamedColours::read(&tag, true).expect("Lab");
        let white = colours.get("white").expect("white");
        assert!(white.lab());
        let [l, a, b] = white.pcs();
        assert!((l - 100.0).abs() < 1e-12 && a.abs() < 1e-12 && b.abs() < 1e-12);
        assert_eq!(white.to_rgb(), crate::lab_to_rgb(l, a, b));
        assert_eq!(white.to_rgb16(), crate::lab_to_rgb16(l, a, b));
        let [l, a, b] = colours.get("past").expect("past").pcs();
        assert!((l - 100.0 * 65535.0 / 65280.0).abs() < 1e-9, "{l}");
        assert!(
            (a - (255.0 * 65535.0 / 65280.0 - 128.0)).abs() < 1e-9,
            "{a}"
        );
        assert!((b + 128.0).abs() < 1e-9, "{b}");

        let mut tag = header(1, 0, "", "");
        tag.extend(entry("d50", [0x7B6B, 0x8000, 0x6996], &[]));
        let colours = NamedColours::read(&tag, false).expect("XYZ");
        let d50 = colours.get("d50").expect("d50");
        assert!(!d50.lab());
        assert_eq!(
            d50.pcs(),
            [0x7B6B, 0x8000, 0x6996].map(|v: u16| f64::from(v) / 32768.0)
        );
        assert_eq!(d50.to_rgb(), pcs_to_rgb(d50.pcs(), false, None));
    }
}
