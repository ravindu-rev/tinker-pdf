//! PDF date strings (7.9.4).
//!
//! The grammar is `D:YYYYMMDDHHmmSSOHH'mm'`, and almost nothing in the wild
//! writes all of it. Every field after the year is optional, the `D:` prefix is
//! routinely missing, the apostrophes are routinely missing, and the trailing
//! one is optional even in the spec. So the parser reads as much as it
//! understands and stops at the first thing it does not, keeping what it
//! already has.

/// The offset from UTC a date carries (7.9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TzOffset {
    /// `Z`, or no relationship at all: 7.9.4 makes UTC the default.
    Utc,
    /// `+`/`-HH'mm'`, in minutes east of UTC.
    Minutes(i16),
}

/// A parsed PDF date (7.9.4), as plain numbers.
///
/// Deliberately not a calendar type: this crate has no date library and will
/// not grow one, and every consumer that needs arithmetic has its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PdfDate {
    /// Four-digit year, the only required field.
    pub year: i32,
    /// Month, 1 to 12. Defaults to 1 (7.9.4).
    pub month: u8,
    /// Day of month, 1 to 31. Defaults to 1.
    pub day: u8,
    /// Hour, 0 to 23. Defaults to 0.
    pub hour: u8,
    /// Minute, 0 to 59. Defaults to 0.
    pub minute: u8,
    /// Second, 0 to 59. Defaults to 0.
    pub second: u8,
    /// Relationship to UTC. Defaults to [`TzOffset::Utc`].
    pub tz: TzOffset,
}

impl PdfDate {
    /// Parses a date string, returning `None` only when not even a year is
    /// readable.
    ///
    /// Truncated forms — `D:2024`, `D:202403`, `D:20240317` — are the norm and
    /// parse to the defaults 7.9.4 assigns. A field that is present but out of
    /// range ends the parse rather than failing it, so `D:20240332` reads as
    /// 2024-03 with a default day rather than as nothing at all.
    pub fn parse(bytes: &[u8]) -> Option<PdfDate> {
        let mut at = 0usize;
        skip_blanks(bytes, &mut at);
        if bytes.get(at..at.saturating_add(2)) == Some(b"D:") {
            at = at.saturating_add(2);
            skip_blanks(bytes, &mut at);
        }
        let year = i32::try_from(digits(bytes, &mut at, 4)?).ok()?;
        let mut date = PdfDate {
            year,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            tz: TzOffset::Utc,
        };

        if let Some(month) = field(bytes, &mut at, 1, 12) {
            date.month = month;
            if let Some(day) = field(bytes, &mut at, 1, 31) {
                date.day = day;
                if let Some(hour) = field(bytes, &mut at, 0, 23) {
                    date.hour = hour;
                    if let Some(minute) = field(bytes, &mut at, 0, 59) {
                        date.minute = minute;
                        if let Some(second) = field(bytes, &mut at, 0, 59) {
                            date.second = second;
                        }
                    }
                }
            }
        }
        if let Some(tz) = offset(bytes, &mut at) {
            date.tz = tz;
        }
        Some(date)
    }
}

fn skip_blanks(bytes: &[u8], at: &mut usize) {
    while matches!(bytes.get(*at), Some(b' ' | b'\t')) {
        *at += 1;
    }
}

/// Reads exactly `n` decimal digits, consuming nothing unless all `n` are
/// there.
fn digits(bytes: &[u8], at: &mut usize, n: usize) -> Option<u32> {
    let slice = bytes.get(*at..at.checked_add(n)?)?;
    let mut value = 0u32;
    for &b in slice {
        let digit = u32::from(b.checked_sub(b'0')?);
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(digit)?;
    }
    *at += n;
    Some(value)
}

/// One two-digit field, accepted only inside `[low, high]`.
fn field(bytes: &[u8], at: &mut usize, low: u8, high: u8) -> Option<u8> {
    let mut probe = *at;
    let value = u8::try_from(digits(bytes, &mut probe, 2)?).ok()?;
    if value < low || value > high {
        return None;
    }
    *at = probe;
    Some(value)
}

fn offset(bytes: &[u8], at: &mut usize) -> Option<TzOffset> {
    let sign: i16 = match bytes.get(*at) {
        Some(b'Z' | b'z') => {
            *at += 1;
            return Some(TzOffset::Utc);
        }
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return None,
    };
    let mut probe = at.saturating_add(1);
    let hours = i16::from(field(bytes, &mut probe, 0, 23)?);
    // The apostrophes are optional here because half the producers in the wild
    // omit them; 7.9.4 requires the first and makes the second optional.
    if bytes.get(probe) == Some(&b'\'') {
        probe = probe.saturating_add(1);
    }
    let minutes = i16::from(field(bytes, &mut probe, 0, 59).unwrap_or(0));
    if bytes.get(probe) == Some(&b'\'') {
        probe = probe.saturating_add(1);
    }
    *at = probe;
    Some(TzOffset::Minutes(sign.saturating_mul(hours.saturating_mul(60).saturating_add(minutes))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Option<PdfDate> {
        PdfDate::parse(s.as_bytes())
    }

    #[test]
    fn the_full_form() {
        let d = parse("D:20240317142530+05'30'").expect("a date");
        assert_eq!(d.year, 2024);
        assert_eq!(d.month, 3);
        assert_eq!(d.day, 17);
        assert_eq!(d.hour, 14);
        assert_eq!(d.minute, 25);
        assert_eq!(d.second, 30);
        assert_eq!(d.tz, TzOffset::Minutes(330));
    }

    #[test]
    fn a_negative_offset_is_west_of_utc() {
        let d = parse("D:20240317142530-08'00'").expect("a date");
        assert_eq!(d.tz, TzOffset::Minutes(-480));
    }

    #[test]
    fn missing_apostrophes_still_parse() {
        let d = parse("D:20240317142530+0530").expect("a date");
        assert_eq!(d.tz, TzOffset::Minutes(330));
    }

    #[test]
    fn a_lone_hour_offset_parses() {
        let d = parse("D:20240317142530+05").expect("a date");
        assert_eq!(d.tz, TzOffset::Minutes(300));
        let d = parse("D:20240317142530+05'").expect("a date");
        assert_eq!(d.tz, TzOffset::Minutes(300));
    }

    #[test]
    fn z_is_utc() {
        assert_eq!(parse("D:20240317142530Z").expect("a date").tz, TzOffset::Utc);
        assert_eq!(
            parse("D:20240317142530Z00'00'").expect("a date").tz,
            TzOffset::Utc
        );
    }

    #[test]
    fn truncated_forms_take_the_defaults() {
        let year = parse("D:2024").expect("a date");
        assert_eq!((year.month, year.day, year.hour), (1, 1, 0));
        let month = parse("D:202403").expect("a date");
        assert_eq!((month.month, month.day), (3, 1));
        let day = parse("D:20240317").expect("a date");
        assert_eq!((day.day, day.hour, day.minute), (17, 0, 0));
        let minute = parse("D:202403171425").expect("a date");
        assert_eq!((minute.minute, minute.second), (25, 0));
    }

    #[test]
    fn the_prefix_is_optional() {
        assert_eq!(parse("20240317").expect("a date").day, 17);
    }

    #[test]
    fn an_out_of_range_field_ends_the_parse() {
        let d = parse("D:20240332").expect("a date");
        assert_eq!(d.month, 3);
        assert_eq!(d.day, 1);
        assert_eq!(parse("D:20241317").expect("a date").month, 1);
    }

    #[test]
    fn junk_is_not_a_date() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("D:"), None);
        assert_eq!(parse("D:20a4"), None);
        assert_eq!(parse("not a date at all"), None);
        assert_eq!(parse("D:202"), None);
    }

    #[test]
    fn no_input_length_can_panic() {
        for len in 0..40usize {
            let bytes = vec![b'0'; len];
            let _ = PdfDate::parse(&bytes);
            let mut prefixed = b"D:".to_vec();
            prefixed.extend(std::iter::repeat_n(b'9', len));
            let _ = PdfDate::parse(&prefixed);
        }
    }
}
