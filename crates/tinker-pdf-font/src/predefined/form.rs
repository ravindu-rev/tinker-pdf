//! The UTF-16 and UTF-8 forms of a registry CMap's UTF-32 ranges.
//!
//! Adobe's registry states each Unicode mapping of a collection in three
//! encoding forms — `-UTF32-`, whose code is a character's scalar value,
//! `-UTF16-`, its UTF-16 code units, and `-UTF8-`, its UTF-8 bytes, each read
//! as one big-endian integer — and the UTF-16 and UTF-8 CMaps of every
//! vendored family are their UTF-32 sibling's ranges re-encoded: the same
//! characters to the same CIDs, each range cut where consecutive scalar values
//! stop encoding to consecutive codes. `build.rs` holds a CMap that is that
//! form of its sibling, range for range, as a reference to the sibling instead
//! of a table of its own, and [`super::ranges`] encodes the sibling's ranges
//! when the CMap is read.
//!
//! This file is both halves of that: `build.rs` includes it to decide which
//! CMaps are forms, comparing each form against the CMap's own text, and the
//! crate reads them through it, so the ranges a lookup sees are the ones the
//! build compared.

/// The tag of a CMap held as its own table: a zlib stream of its
/// delta-encoded ranges follows.
pub(crate) const STORED: u8 = 0;

/// The tag each [`Form`] is held under, followed by its sibling's index.
pub(crate) const FORMS: [(u8, Form); 2] = [(1, Form::Utf16), (2, Form::Utf8)];

/// An encoding form of a CMap's codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// UTF-16 code units, high unit first.
    Utf16,
    /// UTF-8 bytes, the lead byte first.
    Utf8,
}

impl Form {
    /// The code `scalar` takes in this form, or `None` for a value that is
    /// not a Unicode scalar value, which no form encodes.
    fn code(self, scalar: u32) -> Option<u32> {
        let c = char::from_u32(scalar)?;
        Some(match self {
            Form::Utf16 => c
                .encode_utf16(&mut [0; 2])
                .iter()
                .fold(0, |code, &unit| (code << 16) | u32::from(unit)),
            Form::Utf8 => c
                .encode_utf8(&mut [0; 4])
                .bytes()
                .fold(0, |code, byte| (code << 8) | u32::from(byte)),
        })
    }

    /// The last scalar value of the run `scalar` is in: the values from the
    /// run's first to this one encode to consecutive codes, and the next one
    /// does not. A one-unit UTF-16 code runs to `U+FFFF`, a surrogate pair's
    /// low unit through its 1024 values; a one-byte UTF-8 code runs to
    /// `U+007F`, a longer one's last byte through its 64.
    fn run_end(self, scalar: u32) -> u32 {
        match self {
            Form::Utf16 if scalar < 0x1_0000 => 0xFFFF,
            Form::Utf16 => scalar | 0x3FF,
            Form::Utf8 if scalar < 0x80 => 0x7F,
            Form::Utf8 => scalar | 0x3F,
        }
    }

    /// `ranges`, whose codes are scalar values, in this form: each
    /// `(low, high, first CID)` re-encoded and cut at the end of every run
    /// ([`Form::run_end`]), in the order of `ranges` — sorted by low code
    /// where they are, because both forms keep the order of scalar values.
    /// `None` where a range is inverted or holds a value that is not a scalar
    /// value.
    pub(crate) fn ranges(self, ranges: &[(u32, u32, u32)]) -> Option<Vec<(u32, u32, u32)>> {
        let mut out = Vec::with_capacity(ranges.len());
        for &(low, high, cid) in ranges {
            if high < low || (low <= 0xDFFF && high >= 0xD800) {
                return None;
            }
            let mut from = low;
            loop {
                let to = self.run_end(from).min(high);
                out.push((
                    self.code(from)?,
                    self.code(to)?,
                    cid.checked_add(from - low)?,
                ));
                if to == high {
                    break;
                }
                from = to + 1;
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A run is exactly where codes are consecutive**, for every scalar
    /// value in both forms: a value is in the run before it precisely when
    /// its code is that value's code plus one.
    #[test]
    fn a_run_ends_where_codes_stop_being_consecutive() {
        for (_, form) in FORMS {
            for scalar in 0..0x10_FFFFu32 {
                let next = scalar + 1;
                let (Some(code), Some(following)) = (form.code(scalar), form.code(next)) else {
                    continue;
                };
                let joined = following == code + 1;
                assert_eq!(
                    form.run_end(scalar) >= next,
                    joined,
                    "{form:?}: U+{scalar:04X} and U+{next:04X}"
                );
                if joined {
                    assert_eq!(form.run_end(scalar), form.run_end(next), "{form:?}");
                }
            }
        }
    }

    /// The codes themselves, from the encodings' own definitions: `A`, `é`,
    /// `日` and `𠀋` in UTF-16 and in UTF-8.
    #[test]
    fn each_form_encodes_as_its_standard_says() {
        let cases = [
            (0x41, 0x41, 0x41),
            (0xE9, 0xE9, 0xC3A9),
            (0x65E5, 0x65E5, 0x00E6_97A5),
            (0x2000B, 0xD840_DC0B, 0xF0A0_808B),
        ];
        for (scalar, utf16, utf8) in cases {
            assert_eq!(Form::Utf16.code(scalar), Some(utf16), "U+{scalar:04X}");
            assert_eq!(Form::Utf8.code(scalar), Some(utf8), "U+{scalar:04X}");
        }
        assert_eq!(Form::Utf16.code(0xD800), None, "a surrogate");
        assert_eq!(Form::Utf8.code(0x11_0000), None, "past the code space");
    }

    /// A range is cut where its codes jump, and its CIDs follow the cut:
    /// `U+00F0` to `U+0105` is two UTF-8 runs, `<c3b0> <c3bf>` and
    /// `<c480> <c485>`, and one UTF-16 range; `U+103FE` to `U+10401` is two
    /// surrogate-pair runs in UTF-16.
    #[test]
    fn a_range_is_cut_at_the_end_of_each_run() {
        assert_eq!(
            Form::Utf8.ranges(&[(0xF0, 0x105, 100)]),
            Some(vec![(0xC3B0, 0xC3BF, 100), (0xC480, 0xC485, 116)])
        );
        assert_eq!(
            Form::Utf16.ranges(&[(0xF0, 0x105, 100)]),
            Some(vec![(0xF0, 0x105, 100)])
        );
        assert_eq!(
            Form::Utf16.ranges(&[(0x103FE, 0x10401, 7)]),
            Some(vec![
                (0xD800_DFFE, 0xD800_DFFF, 7),
                (0xD801_DC00, 0xD801_DC01, 9)
            ])
        );
    }

    /// What no form can hold is refused rather than cut around: a range
    /// over the surrogates, one past `U+10FFFF`, and an inverted one.
    #[test]
    fn a_range_no_form_encodes_is_refused() {
        for (_, form) in FORMS {
            assert_eq!(form.ranges(&[(0xD7F0, 0xE010, 1)]), None, "{form:?}");
            assert_eq!(form.ranges(&[(0x10_FFF0, 0x11_0010, 1)]), None, "{form:?}");
            assert_eq!(form.ranges(&[(0x50, 0x40, 1)]), None, "{form:?}");
            assert_eq!(form.ranges(&[]), Some(Vec::new()), "{form:?}");
        }
    }
}
