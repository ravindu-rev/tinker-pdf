//! *Managing the Storage Area* and *Managing the Control Value Table* — `RS`,
//! `WS`, `RCVT`, `WCVTP` and `WCVTF` — and what a program asks of its engine:
//! `MPPEM`, `MPS`, `GETINFO` and `INSTCTRL` (`docs/design/hinting.md`, "What a
//! program asks of its engine").
//!
//! The storage area holds `maxStorage` values, each zero until written: the
//! specification leaves an unwritten location's value "unpredictable", and
//! zero is this engine's answer. The control value table holds the `cvt `
//! table's entries scaled to the size in 26.6, and an index past either is
//! an error.

use super::fixed;
use super::vm::{Error, Program, Vm};

/// `GETINFO`'s scaler version: 35, a version that answers greyscale (34 and
/// later) and predates ClearType (36 and later), so a font takes its
/// greyscale and monochrome paths rather than its ClearType ones.
pub const VERSION: i32 = 35;

/// `GETINFO`'s result bit 11: the four phantom points are kept.
const PHANTOM_POINTS: i32 = 1 << 11;
/// `GETINFO`'s result bit 12: the glyph is drawn in grey levels.
const GREYSCALE: i32 = 1 << 12;

/// The size a program hints at: what `MPPEM`, `MPS` and `GETINFO` answer,
/// and what `WCVTF` and `SSW` scale font units to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    /// Pixels per em, in 26.6.
    pub ppem: i32,
    /// The em's extent in points, in 26.6: what `MPS` answers.
    pub points: i32,
    /// The face's `head.unitsPerEm`, the font units in an em.
    pub units_per_em: u16,
    /// Whether the glyph is drawn whole or empty, with no grey levels.
    pub monochrome: bool,
}

impl Size {
    /// `funits` font units at this size in 26.6: `funits · ppem /
    /// unitsPerEm`, rounded half away from zero in 64 bits and wrapped. Zero
    /// for a face that states no units per em.
    #[must_use]
    pub fn scale(&self, funits: i32) -> i32 {
        fixed::round_div(
            i64::from(funits) * i64::from(self.ppem),
            i64::from(self.units_per_em),
        ) as i32
    }

    /// The whole pixels per em: `ppem` rounded to the nearest pixel.
    #[must_use]
    pub fn pixels_per_em(&self) -> i32 {
        fixed::round(self.ppem) / fixed::ONE
    }
}

/// The index `location` names in a table of `len` entries, if it names one.
fn index(location: i32, len: usize) -> Option<usize> {
    usize::try_from(location).ok().filter(|&index| index < len)
}

impl Vm {
    /// Executes the storage, control value or engine instruction `op`.
    pub(super) fn storage_or_engine(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // RS[ ]: pops a location; pushes its value.
            0x43 => {
                let location = self.pop()?;
                let at =
                    index(location, self.storage.len()).ok_or(Error::StorageIndex(location))?;
                let value = self.storage[at];
                self.push(value)?;
            }
            // WS[ ]: pops a value, then a location.
            0x42 => {
                let value = self.pop()?;
                let location = self.pop()?;
                let at =
                    index(location, self.storage.len()).ok_or(Error::StorageIndex(location))?;
                self.storage[at] = value;
            }
            // RCVT[ ]: pops a location; pushes its value, in pixels.
            0x45 => {
                let location = self.pop()?;
                let at = index(location, self.cvt.len()).ok_or(Error::CvtIndex(location))?;
                let value = self.cvt[at];
                self.push(value)?;
            }
            // WCVTP[ ], WCVTF[ ]: pops a value, then a location. WCVTP's value
            // "is written to the CVT table unchanged", in pixels; WCVTF's is in
            // font units, "scaled to the current point size and resolution".
            0x44 | 0x70 => {
                let value = self.pop()?;
                let location = self.pop()?;
                let at = index(location, self.cvt.len()).ok_or(Error::CvtIndex(location))?;
                self.cvt[at] = if op == 0x44 {
                    value
                } else {
                    self.size.scale(value)
                };
            }
            // MPPEM[ ]: the pixels per em along the projection vector, "always
            // an integer". A hinted glyph's two axes are one size.
            0x4B => {
                let ppem = self.size.pixels_per_em();
                self.push(ppem)?;
            }
            // MPS[ ]: the point size, in 26.6.
            0x4C => {
                let points = self.size.points;
                self.push(points)?;
            }
            0x88 => {
                let selector = self.pop()?;
                let result = self.info(selector);
                self.push(result)?;
            }
            // INSTCTRL[ ]: pops a selector, then a value.
            _ => {
                let selector = self.pop()?;
                let value = self.pop()?;
                self.instruction_control(selector, value);
            }
        }
        Ok(())
    }

    /// GETINFO[ ]'s answer to `selector`, by *GET INFOrmation*'s table: each
    /// bit asked for answers in its result bits, OR'd together, and a bit no
    /// row names answers nothing. This engine is version [`VERSION`]; a
    /// hinted glyph is never rotated or stretched; variations are not
    /// supported; the four phantom points are kept; and the greyscale bit
    /// answers whether the glyph is drawn in grey levels. The ClearType bits,
    /// selectors 6 to 12, answer 0.
    fn info(&self, selector: i32) -> i32 {
        let asks = |bit: u32| selector & (1 << bit) != 0;
        let mut result = 0;
        if asks(0) {
            result |= VERSION;
        }
        if asks(4) {
            result |= PHANTOM_POINTS;
        }
        if asks(5) && !self.size.monochrome {
            result |= GREYSCALE;
        }
        result
    }

    /// INSTCTRL[ ]: the flag the selector chooses takes the value's bit of the
    /// same weight — selector 1 bit 0 (inhibit grid-fitting), 2 bit 1 (ignore
    /// the control value program's settings), 3 bit 2 (native ClearType),
    /// whose valid values are 0 and 1, 0 and 2, and 0 and 4. Only the
    /// control value program may set them ("Flags 1 and 2 must be set in the
    /// CV program"; Apple's manual: "INSTCTRL[] can only be executed in the
    /// CVT program"): anywhere else, and for a selector past 3, the
    /// instruction pops its two values and changes nothing.
    fn instruction_control(&mut self, selector: i32, value: i32) {
        if self.program() != Program::ControlValue || !(1..=3).contains(&selector) {
            return;
        }
        let flag = 1u8 << (selector - 1);
        let set = value & i32::from(flag) != 0;
        let flags = self.gs.instruct_control;
        self.gs.instruct_control = if set { flags | flag } else { flags & !flag };
    }
}
