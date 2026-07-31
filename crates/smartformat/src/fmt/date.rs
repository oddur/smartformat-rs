//! .NET standard date/time format specifiers: `d D f F g G m M o O r R s
//! t T u y Y` (and the empty spec = `G`), rendered from a
//! [`jiff::civil::DateTime`].
//!
//! Each standard specifier expands to the culture's corresponding pattern
//! (`d` → `short_date_pattern`, …) which is then rendered by an internal
//! .NET-custom-pattern interpreter. That interpreter exists only to back the
//! standard specifiers; user-supplied custom patterns are still rejected
//! with [`FormatSpecError::Unsupported`] by [`format_datetime`].
//!
//! `U` (universal full) requires timezone conversion and is Unsupported here
//! (see the non-goals in `DESIGN.md`); `K`-related offsets render as empty,
//! matching an unspecified-kind .NET `DateTime`.

use super::culture::{invariant, CultureData, DateTimeFormat};
use super::FormatSpecError;

/// .NET `DateTimeFormatInfo.RoundtripFormat`.
const ROUNDTRIP_PATTERN: &str = "yyyy'-'MM'-'dd'T'HH':'mm':'ss.fffffffK";
/// .NET `DateTimeFormatInfo.RFC1123Pattern`.
const RFC1123_PATTERN: &str = "ddd, dd MMM yyyy HH':'mm':'ss 'GMT'";
/// .NET `DateTimeFormatInfo.SortableDateTimePattern`.
const SORTABLE_PATTERN: &str = "yyyy'-'MM'-'dd'T'HH':'mm':'ss";
/// .NET `DateTimeFormatInfo.UniversalSortableDateTimePattern`.
const UNIVERSAL_SORTABLE_PATTERN: &str = "yyyy'-'MM'-'dd HH':'mm':'ss'Z'";

/// .NET `DateTimeFormat.MaxSecondsFractionDigits`.
const MAX_FRACTION_DIGITS: usize = 7;

/// The message .NET's `FormatException` carries for a date/time specifier it
/// does not recognize (`System.SR.Format_InvalidString`).
pub const INVALID_SPEC_MESSAGE: &str = "Input string was not in a correct format.";

/// Formats `dt` with a .NET *standard* date/time format spec, producing
/// byte-identical output to .NET's `dt.ToString(spec, culture)` for a
/// `DateTime` of unspecified kind.
pub fn format_datetime(
    dt: &jiff::civil::DateTime,
    spec: &str,
    culture: &CultureData,
) -> Result<String, FormatSpecError> {
    let mut chars = spec.chars();
    let standard = match (chars.next(), chars.next()) {
        (None, _) => 'G',
        (Some(c), None) => c,
        // Longer than one char is a custom pattern, not a standard specifier.
        _ => return Err(FormatSpecError::Unsupported(spec.to_owned())),
    };

    let df = &culture.datetime;
    // `o O r R s u` are culture-invariant in .NET whatever culture is passed.
    let (pattern, culture) = match standard {
        'd' => (df.short_date_pattern.to_owned(), culture),
        'D' => (df.long_date_pattern.to_owned(), culture),
        'f' => (
            format!("{} {}", df.long_date_pattern, df.short_time_pattern),
            culture,
        ),
        'F' => (df.full_date_time_pattern.to_owned(), culture),
        'g' => (
            format!("{} {}", df.short_date_pattern, df.short_time_pattern),
            culture,
        ),
        'G' => (
            format!("{} {}", df.short_date_pattern, df.long_time_pattern),
            culture,
        ),
        'm' | 'M' => (df.month_day_pattern.to_owned(), culture),
        'o' | 'O' => (ROUNDTRIP_PATTERN.to_owned(), invariant()),
        'r' | 'R' => (RFC1123_PATTERN.to_owned(), invariant()),
        's' => (SORTABLE_PATTERN.to_owned(), invariant()),
        't' => (df.short_time_pattern.to_owned(), culture),
        'T' => (df.long_time_pattern.to_owned(), culture),
        'u' => (UNIVERSAL_SORTABLE_PATTERN.to_owned(), invariant()),
        'y' | 'Y' => (df.year_month_pattern.to_owned(), culture),
        // Valid .NET, but converting to UTC needs a timezone we don't have.
        'U' => return Err(FormatSpecError::Unsupported(spec.to_owned())),
        _ => return Err(FormatSpecError::Invalid(spec.to_owned())),
    };

    render_pattern(dt, &pattern, &culture.datetime)
}

/// Renders a .NET custom date/time pattern (`DateTimeFormat.FormatCustomized`).
fn render_pattern(
    dt: &jiff::civil::DateTime,
    pattern: &str,
    df: &DateTimeFormat,
) -> Result<String, FormatSpecError> {
    let mut renderer = Renderer {
        dt,
        df,
        pattern,
        out: String::with_capacity(pattern.len() + 16),
    };
    let chars: Vec<char> = pattern.chars().collect();
    renderer.render(&chars)?;
    Ok(renderer.out)
}

struct Renderer<'a> {
    dt: &'a jiff::civil::DateTime,
    df: &'a DateTimeFormat,
    pattern: &'a str,
    out: String,
}

impl Renderer<'_> {
    fn render(&mut self, chars: &[char]) -> Result<(), FormatSpecError> {
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            match ch {
                'd' | 'M' | 'y' | 'h' | 'H' | 'm' | 's' | 'f' | 'F' | 't' | 'g' => {
                    let len = repeat_count(chars, i);
                    self.token(chars, i, ch, len)?;
                    i += len;
                }
                // The offset of an unspecified-kind DateTime renders as nothing.
                'K' => i += 1,
                '/' => {
                    self.out.push_str(self.df.date_separator);
                    i += 1;
                }
                ':' => {
                    self.out.push_str(self.df.time_separator);
                    i += 1;
                }
                '\'' | '"' => i += self.quoted(chars, i)?,
                '\\' => match chars.get(i + 1) {
                    Some(&escaped) => {
                        self.out.push(escaped);
                        i += 2;
                    }
                    None => return Err(self.invalid()),
                },
                '%' => match chars.get(i + 1) {
                    Some(&next) if next != '%' => {
                        self.render(&chars[i + 1..i + 2])?;
                        i += 2;
                    }
                    _ => return Err(self.invalid()),
                },
                // A timezone offset: a real .NET specifier we can't reproduce
                // from a naive DateTime.
                'z' => return Err(self.unsupported()),
                _ => {
                    self.out.push(ch);
                    i += 1;
                }
            }
        }
        Ok(())
    }

    /// Renders one repeated pattern token. `chars`/`index` locate it in the
    /// pattern, which `M` needs: the month's grammatical form depends on what
    /// surrounds it.
    fn token(
        &mut self,
        chars: &[char],
        index: usize,
        ch: char,
        len: usize,
    ) -> Result<(), FormatSpecError> {
        let dt = self.dt;
        match ch {
            'd' => {
                if len <= 2 {
                    push_digits(&mut self.out, i64::from(dt.day()), len);
                } else {
                    let dow = dt.weekday().to_sunday_zero_offset() as usize;
                    self.out.push_str(if len == 3 {
                        self.df.abbreviated_day_names[dow]
                    } else {
                        self.df.day_names[dow]
                    });
                }
            }
            'M' => {
                let month = dt.month();
                if len <= 2 {
                    push_digits(&mut self.out, i64::from(month), len);
                } else {
                    let idx = month as usize - 1;
                    let genitive =
                        self.df.use_genitive_month && is_use_genitive_form(chars, index, len);
                    let names = match (len == 3, genitive) {
                        (true, false) => &self.df.abbreviated_month_names,
                        (true, true) => &self.df.abbreviated_month_genitive_names,
                        (false, false) => &self.df.month_names,
                        (false, true) => &self.df.month_genitive_names,
                    };
                    self.out.push_str(names[idx]);
                }
            }
            'y' => {
                let year = i64::from(dt.year());
                if len <= 2 {
                    push_digits(&mut self.out, year.rem_euclid(100), len);
                } else {
                    self.out.push_str(&format!("{year:0len$}"));
                }
            }
            'h' => {
                let hour12 = match dt.hour() % 12 {
                    0 => 12,
                    h => h,
                };
                push_digits(&mut self.out, i64::from(hour12), len);
            }
            'H' => push_digits(&mut self.out, i64::from(dt.hour()), len),
            'm' => push_digits(&mut self.out, i64::from(dt.minute()), len),
            's' => push_digits(&mut self.out, i64::from(dt.second()), len),
            'f' | 'F' => {
                if len > MAX_FRACTION_DIGITS {
                    return Err(self.invalid());
                }
                // Truncating, never rounding, like .NET's tick division.
                let scale = 10i64.pow(9 - len as u32);
                let mut fraction = i64::from(dt.subsec_nanosecond()) / scale;
                if ch == 'f' {
                    self.out.push_str(&format!("{fraction:0len$}"));
                } else {
                    let mut digits = len;
                    while digits > 0 && fraction % 10 == 0 {
                        fraction /= 10;
                        digits -= 1;
                    }
                    if digits > 0 {
                        self.out.push_str(&format!("{fraction:0digits$}"));
                    } else if self.out.ends_with('.') {
                        self.out.pop();
                    }
                }
            }
            // Every repeat count of `g` renders the era name, which for the
            // invariant Gregorian calendar is always "A.D.".
            'g' => self.out.push_str(self.df.era_name),
            't' => {
                let designator = if dt.hour() < 12 {
                    self.df.am_designator
                } else {
                    self.df.pm_designator
                };
                if len == 1 {
                    if let Some(first) = designator.chars().next() {
                        self.out.push(first);
                    }
                } else {
                    self.out.push_str(designator);
                }
            }
            _ => unreachable!("not a repeatable pattern token: {ch}"),
        }
        Ok(())
    }

    /// Copies a `'`- or `"`-quoted literal into the output and reports how
    /// many pattern chars it spanned (`DateTimeFormat.ParseQuoteString`).
    fn quoted(&mut self, chars: &[char], pos: usize) -> Result<usize, FormatSpecError> {
        let quote = chars[pos];
        let mut i = pos + 1;
        while i < chars.len() {
            let ch = chars[i];
            i += 1;
            if ch == quote {
                return Ok(i - pos);
            }
            if ch == '\\' {
                match chars.get(i) {
                    Some(&escaped) => {
                        self.out.push(escaped);
                        i += 1;
                    }
                    None => return Err(self.invalid()),
                }
            } else {
                self.out.push(ch);
            }
        }
        Err(self.invalid())
    }

    fn invalid(&self) -> FormatSpecError {
        FormatSpecError::Invalid(self.pattern.to_owned())
    }

    fn unsupported(&self) -> FormatSpecError {
        FormatSpecError::Unsupported(self.pattern.to_owned())
    }
}

/// .NET `DateTimeFormat.IsUseGenitiveForm`: an `MMM`/`MMMM` token takes the
/// genitive month name when a day *number* token (`d` or `dd`, never `ddd`)
/// stands next to it — first looking back, then, even after a long run like
/// `dddd`, forward. `dddd, d MMMM yyyy` is genitive because of the lone `d`;
/// `MMMM yyyy` is not.
///
/// Like .NET this scans raw pattern characters, so a `d` inside a quoted
/// literal counts; the port matches that quirk deliberately.
fn is_use_genitive_form(chars: &[char], index: usize, token_len: usize) -> bool {
    if let Some(found) = chars[..index].iter().rposition(|&c| c == 'd') {
        let repeat = chars[..found]
            .iter()
            .rev()
            .take_while(|&&c| c == 'd')
            .count();
        if repeat <= 1 {
            return true;
        }
    }
    let after = index + token_len;
    if let Some(offset) = chars[after..].iter().position(|&c| c == 'd') {
        let found = after + offset;
        let repeat = chars[found + 1..].iter().take_while(|&&c| c == 'd').count();
        if repeat <= 1 {
            return true;
        }
    }
    false
}

fn repeat_count(chars: &[char], pos: usize) -> usize {
    let ch = chars[pos];
    let mut len = 1;
    while chars.get(pos + len) == Some(&ch) {
        len += 1;
    }
    len
}

/// .NET `DateTimeFormat.FormatDigits`: zero-pads to `len`, which the .NET
/// implementation caps at two digits for every repeatable numeric token.
fn push_digits(out: &mut String, value: i64, len: usize) {
    let len = len.min(2);
    out.push_str(&format!("{value:0len$}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, DateTime};

    /// 2024-03-05 09:07:03 exactly (a Tuesday).
    fn morning() -> DateTime {
        date(2024, 3, 5).at(9, 7, 3, 0)
    }

    /// 2024-12-25 23:59:59.9999999 (a Wednesday).
    fn late() -> DateTime {
        date(2024, 12, 25).at(23, 59, 59, 999_999_900)
    }

    /// 2000-02-29 13:04:05.1234 (a Tuesday).
    fn leap_afternoon() -> DateTime {
        date(2000, 2, 29).at(13, 4, 5, 123_400_000)
    }

    /// 0987-11-09 00:30:00.5 (a Friday).
    fn medieval() -> DateTime {
        date(987, 11, 9).at(0, 30, 0, 500_000_000)
    }

    fn fmt(dt: &DateTime, spec: &str) -> String {
        format_datetime(dt, spec, invariant()).expect("supported spec")
    }

    fn pat(dt: &DateTime, pattern: &str) -> String {
        render_pattern(dt, pattern, &invariant().datetime).expect("valid pattern")
    }

    // Every expectation below is the output of .NET 10
    // `dt.ToString(spec, CultureInfo.InvariantCulture)` on a
    // `DateTimeKind.Unspecified` value.
    #[test]
    fn standard_specs_morning() {
        let dt = morning();
        assert_eq!(fmt(&dt, ""), "03/05/2024 09:07:03");
        assert_eq!(fmt(&dt, "d"), "03/05/2024");
        assert_eq!(fmt(&dt, "D"), "Tuesday, 05 March 2024");
        assert_eq!(fmt(&dt, "f"), "Tuesday, 05 March 2024 09:07");
        assert_eq!(fmt(&dt, "F"), "Tuesday, 05 March 2024 09:07:03");
        assert_eq!(fmt(&dt, "g"), "03/05/2024 09:07");
        assert_eq!(fmt(&dt, "G"), "03/05/2024 09:07:03");
        assert_eq!(fmt(&dt, "m"), "March 05");
        assert_eq!(fmt(&dt, "M"), "March 05");
        assert_eq!(fmt(&dt, "o"), "2024-03-05T09:07:03.0000000");
        assert_eq!(fmt(&dt, "O"), "2024-03-05T09:07:03.0000000");
        assert_eq!(fmt(&dt, "r"), "Tue, 05 Mar 2024 09:07:03 GMT");
        assert_eq!(fmt(&dt, "R"), "Tue, 05 Mar 2024 09:07:03 GMT");
        assert_eq!(fmt(&dt, "s"), "2024-03-05T09:07:03");
        assert_eq!(fmt(&dt, "t"), "09:07");
        assert_eq!(fmt(&dt, "T"), "09:07:03");
        assert_eq!(fmt(&dt, "u"), "2024-03-05 09:07:03Z");
        assert_eq!(fmt(&dt, "y"), "2024 March");
        assert_eq!(fmt(&dt, "Y"), "2024 March");
    }

    #[test]
    fn standard_specs_late_evening() {
        let dt = late();
        assert_eq!(fmt(&dt, ""), "12/25/2024 23:59:59");
        assert_eq!(fmt(&dt, "d"), "12/25/2024");
        assert_eq!(fmt(&dt, "D"), "Wednesday, 25 December 2024");
        assert_eq!(fmt(&dt, "f"), "Wednesday, 25 December 2024 23:59");
        assert_eq!(fmt(&dt, "F"), "Wednesday, 25 December 2024 23:59:59");
        assert_eq!(fmt(&dt, "g"), "12/25/2024 23:59");
        assert_eq!(fmt(&dt, "G"), "12/25/2024 23:59:59");
        assert_eq!(fmt(&dt, "M"), "December 25");
        assert_eq!(fmt(&dt, "O"), "2024-12-25T23:59:59.9999999");
        assert_eq!(fmt(&dt, "R"), "Wed, 25 Dec 2024 23:59:59 GMT");
        assert_eq!(fmt(&dt, "s"), "2024-12-25T23:59:59");
        assert_eq!(fmt(&dt, "t"), "23:59");
        assert_eq!(fmt(&dt, "T"), "23:59:59");
        assert_eq!(fmt(&dt, "u"), "2024-12-25 23:59:59Z");
        assert_eq!(fmt(&dt, "Y"), "2024 December");
    }

    #[test]
    fn standard_specs_leap_day_and_three_digit_year() {
        let dt = leap_afternoon();
        assert_eq!(fmt(&dt, "D"), "Tuesday, 29 February 2000");
        assert_eq!(fmt(&dt, "G"), "02/29/2000 13:04:05");
        assert_eq!(fmt(&dt, "o"), "2000-02-29T13:04:05.1234000");
        assert_eq!(fmt(&dt, "r"), "Tue, 29 Feb 2000 13:04:05 GMT");
        assert_eq!(fmt(&dt, "u"), "2000-02-29 13:04:05Z");

        let dt = medieval();
        assert_eq!(fmt(&dt, "d"), "11/09/0987");
        assert_eq!(fmt(&dt, "D"), "Friday, 09 November 0987");
        assert_eq!(fmt(&dt, "F"), "Friday, 09 November 0987 00:30:00");
        assert_eq!(fmt(&dt, "o"), "0987-11-09T00:30:00.5000000");
        assert_eq!(fmt(&dt, "r"), "Fri, 09 Nov 0987 00:30:00 GMT");
        assert_eq!(fmt(&dt, "s"), "0987-11-09T00:30:00");
        assert_eq!(fmt(&dt, "u"), "0987-11-09 00:30:00Z");
        assert_eq!(fmt(&dt, "y"), "0987 November");
    }

    #[test]
    fn standard_specs_year_one_midnight() {
        let dt = date(1, 1, 1).at(0, 0, 0, 0);
        assert_eq!(fmt(&dt, ""), "01/01/0001 00:00:00");
        assert_eq!(fmt(&dt, "D"), "Monday, 01 January 0001");
        assert_eq!(fmt(&dt, "o"), "0001-01-01T00:00:00.0000000");
        assert_eq!(fmt(&dt, "r"), "Mon, 01 Jan 0001 00:00:00 GMT");
        assert_eq!(fmt(&dt, "t"), "00:00");
        assert_eq!(fmt(&dt, "y"), "0001 January");
    }

    #[test]
    fn u_needs_utc_conversion_and_unknown_specs_are_rejected() {
        let dt = morning();
        assert_eq!(
            format_datetime(&dt, "U", invariant()),
            Err(FormatSpecError::Unsupported("U".to_owned()))
        );
        // Single chars that are custom-pattern tokens but not standard specs.
        for spec in ["Q", "h", "H", "K", "z", "e", "%"] {
            assert_eq!(
                format_datetime(&dt, spec, invariant()),
                Err(FormatSpecError::Invalid(spec.to_owned())),
                "spec {spec}"
            );
        }
    }

    #[test]
    fn custom_patterns_are_unsupported() {
        let dt = morning();
        for spec in ["yyyy-MM-dd", "dd", "HH:mm", "  "] {
            assert_eq!(
                format_datetime(&dt, spec, invariant()),
                Err(FormatSpecError::Unsupported(spec.to_owned())),
                "spec {spec}"
            );
        }
    }

    #[test]
    fn pattern_day_and_month_tokens() {
        let dt = morning();
        assert_eq!(pat(&dt, "d"), "5");
        assert_eq!(pat(&dt, "dd"), "05");
        assert_eq!(pat(&dt, "ddd"), "Tue");
        assert_eq!(pat(&dt, "dddd"), "Tuesday");
        assert_eq!(pat(&dt, "ddddd"), "Tuesday");
        assert_eq!(pat(&dt, "M"), "3");
        assert_eq!(pat(&dt, "MM"), "03");
        assert_eq!(pat(&dt, "MMM"), "Mar");
        assert_eq!(pat(&dt, "MMMM"), "March");
        assert_eq!(pat(&dt, "MMMMM"), "March");
        assert_eq!(pat(&late(), "ddd"), "Wed");
        assert_eq!(pat(&late(), "MMM"), "Dec");
        assert_eq!(pat(&medieval(), "dddd"), "Friday");
        assert_eq!(pat(&medieval(), "MMMM"), "November");
    }

    #[test]
    fn pattern_year_token() {
        let dt = morning();
        assert_eq!(pat(&dt, "y"), "24");
        assert_eq!(pat(&dt, "yy"), "24");
        assert_eq!(pat(&dt, "yyy"), "2024");
        assert_eq!(pat(&dt, "yyyy"), "2024");
        assert_eq!(pat(&dt, "yyyyy"), "02024");

        let dt = medieval();
        assert_eq!(pat(&dt, "y"), "87");
        assert_eq!(pat(&dt, "yy"), "87");
        assert_eq!(pat(&dt, "yyy"), "987");
        assert_eq!(pat(&dt, "yyyy"), "0987");

        // A century year keeps the two-digit form zero-padded, but a single
        // `y` drops the padding.
        assert_eq!(pat(&leap_afternoon(), "y"), "0");
        assert_eq!(pat(&leap_afternoon(), "yy"), "00");
        assert_eq!(pat(&leap_afternoon(), "yyyy"), "2000");

        let dt = date(1, 1, 1).at(0, 0, 0, 0);
        assert_eq!(pat(&dt, "y"), "1");
        assert_eq!(pat(&dt, "yy"), "01");
        assert_eq!(pat(&dt, "yyy"), "001");
        assert_eq!(pat(&dt, "yyyyy"), "00001");
    }

    #[test]
    fn pattern_hour_tokens_at_midnight_and_noon() {
        let midnight = date(2024, 3, 5).at(0, 0, 0, 0);
        assert_eq!(pat(&midnight, "h"), "12");
        assert_eq!(pat(&midnight, "hh"), "12");
        assert_eq!(pat(&midnight, "H"), "0");
        assert_eq!(pat(&midnight, "HH"), "00");
        assert_eq!(pat(&midnight, "tt"), "AM");

        let noon = date(2024, 3, 5).at(12, 0, 0, 0);
        assert_eq!(pat(&noon, "h"), "12");
        assert_eq!(pat(&noon, "hh"), "12");
        assert_eq!(pat(&noon, "H"), "12");
        assert_eq!(pat(&noon, "tt"), "PM");

        let one_pm = date(2024, 3, 5).at(13, 0, 0, 0);
        assert_eq!(pat(&one_pm, "h"), "1");
        assert_eq!(pat(&one_pm, "hh"), "01");

        let evening = date(2024, 3, 5).at(23, 0, 0, 0);
        assert_eq!(pat(&evening, "h"), "11");
        assert_eq!(pat(&evening, "hh"), "11");
        assert_eq!(pat(&evening, "HH"), "23");
        assert_eq!(pat(&evening, "tt"), "PM");
    }

    #[test]
    fn pattern_numeric_tokens_cap_padding_at_two_digits() {
        let dt = morning();
        assert_eq!(pat(&dt, "hhh"), "09");
        assert_eq!(pat(&dt, "HHH"), "09");
        assert_eq!(pat(&dt, "mmm"), "07");
        assert_eq!(pat(&dt, "sss"), "03");
    }

    #[test]
    fn pattern_minute_and_second_tokens() {
        let dt = morning();
        assert_eq!(pat(&dt, "m"), "7");
        assert_eq!(pat(&dt, "mm"), "07");
        assert_eq!(pat(&dt, "s"), "3");
        assert_eq!(pat(&dt, "ss"), "03");
        assert_eq!(pat(&late(), "mm"), "59");
        assert_eq!(pat(&late(), "ss"), "59");
    }

    #[test]
    fn pattern_fraction_truncates_and_pads() {
        let dt = leap_afternoon(); // .1234
        assert_eq!(pat(&dt, "f"), "1");
        assert_eq!(pat(&dt, "ff"), "12");
        assert_eq!(pat(&dt, "fff"), "123");
        assert_eq!(pat(&dt, "ffff"), "1234");
        assert_eq!(pat(&dt, "fffff"), "12340");
        assert_eq!(pat(&dt, "ffffff"), "123400");
        assert_eq!(pat(&dt, "fffffff"), "1234000");

        // .9999999 truncates rather than rounding up.
        let dt = late();
        assert_eq!(pat(&dt, "f"), "9");
        assert_eq!(pat(&dt, "ff"), "99");
        assert_eq!(pat(&dt, "fff"), "999");
        assert_eq!(pat(&dt, "fffffff"), "9999999");

        let dt = morning();
        assert_eq!(pat(&dt, "f"), "0");
        assert_eq!(pat(&dt, "fff"), "000");
        assert_eq!(pat(&dt, "fffffff"), "0000000");
    }

    #[test]
    fn pattern_capital_f_suppresses_trailing_zeros() {
        let dt = leap_afternoon(); // .1234
        assert_eq!(pat(&dt, "F"), "1");
        assert_eq!(pat(&dt, "FF"), "12");
        assert_eq!(pat(&dt, "FFF"), "123");
        assert_eq!(pat(&dt, "FFFF"), "1234");
        assert_eq!(pat(&dt, "FFFFF"), "1234");
        assert_eq!(pat(&dt, "FFFFFFF"), "1234");

        let dt = medieval(); // .5
        assert_eq!(pat(&dt, "FF"), "5");
        assert_eq!(pat(&dt, "FFF"), "5");
        assert_eq!(pat(&dt, "ss.FFF"), "00.5");

        // An all-zero fraction renders nothing and eats one preceding dot.
        let dt = morning();
        assert_eq!(pat(&dt, "FFF"), "");
        assert_eq!(pat(&dt, "ss.FFF"), "03");
        assert_eq!(pat(&dt, "ss..FFF"), "03.");
        assert_eq!(pat(&dt, "ss FFF"), "03 ");
        assert_eq!(pat(&dt, "ss.FFF'end'"), "03end");
        assert_eq!(pat(&late(), "ss.FFF"), "59.999");
    }

    #[test]
    fn pattern_designator_and_offset_tokens() {
        let dt = morning();
        assert_eq!(pat(&dt, "t"), "A");
        assert_eq!(pat(&dt, "tt"), "AM");
        assert_eq!(pat(&dt, "ttt"), "AM");
        assert_eq!(pat(&leap_afternoon(), "t"), "P");
        assert_eq!(pat(&leap_afternoon(), "ttt"), "PM");
        // Unspecified kind: K contributes nothing.
        assert_eq!(pat(&dt, "HH:mmK"), "09:07");
        assert_eq!(pat(&dt, "K'x'"), "x");
    }

    #[test]
    fn pattern_separators_quotes_and_escapes() {
        let dt = morning();
        assert_eq!(pat(&dt, "d/M/y"), "5/3/24");
        assert_eq!(pat(&dt, "d:M"), "5:3");
        assert_eq!(pat(&dt, "MM/dd/yyyy"), "03/05/2024");
        assert_eq!(pat(&dt, "HH:mm:ss"), "09:07:03");
        assert_eq!(pat(&dt, r"yyyy\-MM\-dd"), "2024-03-05");
        assert_eq!(pat(&dt, r"\d\M"), "dM");
        assert_eq!(pat(&dt, "'abc'dd"), "abc05");
        assert_eq!(pat(&dt, "\"q\"MM"), "q03");
        assert_eq!(pat(&dt, "'a''b'"), "ab");
        assert_eq!(pat(&dt, r"'it\'s'"), "it's");
        assert_eq!(pat(&dt, "MMM,ddd"), "Mar,Tue");
        assert_eq!(pat(&dt, "hh:mm:ss tt"), "09:07:03 AM");
        assert_eq!(pat(&dt, "%d"), "5");
        assert_eq!(pat(&dt, "%d%M"), "53");
        assert_eq!(pat(&dt, "%f"), "0");
        assert_eq!(pat(&medieval(), "%f"), "5");
        assert_eq!(pat(&dt, "%K"), "");
    }

    #[test]
    fn pattern_errors() {
        let dt = morning();
        let df = &invariant().datetime;
        for bad in ["dd'", r"dd\", "ffffffff", "%%", "%"] {
            assert_eq!(
                render_pattern(&dt, bad, df),
                Err(FormatSpecError::Invalid(bad.to_owned())),
                "pattern {bad}"
            );
        }
        // A timezone offset cannot be rendered from a naive date/time.
        assert_eq!(
            render_pattern(&dt, "HH:mm zz", df),
            Err(FormatSpecError::Unsupported("HH:mm zz".to_owned()))
        );
    }

    #[test]
    fn era_token_renders_the_calendar_era() {
        let dt = morning();
        let df = &invariant().datetime;
        for pattern in ["g", "gg", "ggggg"] {
            assert_eq!(render_pattern(&dt, pattern, df), Ok("A.D.".to_owned()));
        }
        assert_eq!(
            render_pattern(&dt, "yyyy g", df),
            Ok("2024 A.D.".to_owned())
        );
    }

    // Every expectation below is the output of .NET 10
    // `dt.ToString(spec, CultureInfo.GetCultureInfo(name))` for
    // 2024-03-05 09:07:03 of unspecified kind, transcribed from a probe that
    // escaped everything outside printable ASCII.
    mod cultures {
        use super::*;
        use crate::fmt::culture;

        fn dtf(name: &str, spec: &str) -> String {
            let culture = culture::get(name).expect("a culture the port ships");
            format_datetime(&morning(), spec, culture).expect("supported spec")
        }

        #[test]
        fn de_de() {
            assert_eq!(dtf("de-DE", "d"), "05.03.2024");
            assert_eq!(dtf("de-DE", "D"), "Dienstag, 5. März 2024");
            assert_eq!(dtf("de-DE", "f"), "Dienstag, 5. März 2024 09:07");
            assert_eq!(dtf("de-DE", "F"), "Dienstag, 5. März 2024 09:07:03");
            assert_eq!(dtf("de-DE", "g"), "05.03.2024 09:07");
            assert_eq!(dtf("de-DE", "G"), "05.03.2024 09:07:03");
            assert_eq!(dtf("de-DE", "m"), "5. März");
            assert_eq!(dtf("de-DE", "t"), "09:07");
            assert_eq!(dtf("de-DE", "T"), "09:07:03");
            assert_eq!(dtf("de-DE", "y"), "März 2024");
            // Culture-invariant specifiers ignore the culture, as in .NET.
            assert_eq!(dtf("de-DE", "r"), "Tue, 05 Mar 2024 09:07:03 GMT");
            assert_eq!(dtf("de-DE", "s"), "2024-03-05T09:07:03");
        }

        #[test]
        fn fr_fr_and_is_is() {
            assert_eq!(dtf("fr-FR", "d"), "05/03/2024");
            assert_eq!(dtf("fr-FR", "D"), "mardi 5 mars 2024");
            assert_eq!(dtf("fr-FR", "F"), "mardi 5 mars 2024 09:07:03");
            assert_eq!(dtf("fr-FR", "m"), "5 mars");
            assert_eq!(dtf("fr-FR", "y"), "mars 2024");

            assert_eq!(dtf("is-IS", "d"), "5.3.2024");
            assert_eq!(dtf("is-IS", "D"), "þriðjudagur, 5. mars 2024");
            assert_eq!(dtf("is-IS", "F"), "þriðjudagur, 5. mars 2024 09:07:03");
            assert_eq!(dtf("is-IS", "G"), "5.3.2024 09:07:03");
            assert_eq!(dtf("is-IS", "m"), "5. mars");
            assert_eq!(dtf("is-IS", "y"), "mars 2024");
        }

        /// `ru` inflects the month next to a day number and separates the year
        /// from its "г." with U+202F. `y` has no day token, so it keeps the
        /// nominative "март".
        #[test]
        fn genitive_month_names() {
            assert_eq!(dtf("ru", "D"), "вторник, 5 марта 2024\u{202f}г.");
            assert_eq!(dtf("ru", "F"), "вторник, 5 марта 2024\u{202f}г. 09:07:03");
            assert_eq!(dtf("ru", "m"), "5 марта");
            assert_eq!(dtf("ru", "y"), "март 2024\u{202f}г.");

            assert_eq!(dtf("cs", "D"), "úterý 5. března 2024");
            assert_eq!(dtf("cs", "y"), "březen 2024");

            assert_eq!(dtf("pl", "D"), "wtorek, 5 marca 2024");
            assert_eq!(dtf("pl", "y"), "marzec 2024");

            assert_eq!(dtf("uk", "D"), "вівторок, 5 березня 2024\u{202f}р.");
            assert_eq!(dtf("uk", "y"), "березень 2024\u{202f}р.");

            assert_eq!(dtf("fi", "D"), "tiistai 5. maaliskuuta 2024");
            assert_eq!(dtf("fi", "y"), "maaliskuu 2024");
        }

        /// The `d`/`dd`-only rule from `DateTimeFormat.IsUseGenitiveForm`,
        /// which reads the pattern's raw characters.
        #[test]
        fn genitive_month_names_follow_the_day_number_token() {
            let ru = &culture::get("ru").expect("ru").datetime;
            let pat = |pattern: &str| render_pattern(&morning(), pattern, ru).expect("valid");
            assert_eq!(pat("MMMM"), "март");
            assert_eq!(pat("MMMM yyyy"), "март 2024");
            assert_eq!(pat("d MMMM"), "5 марта");
            assert_eq!(pat("dd MMMM"), "05 марта");
            assert_eq!(pat("MMMM d"), "марта 5");
            // `ddd`/`dddd` are day *names*, so they do not trigger it…
            assert_eq!(pat("ddd MMMM"), "Вт март");
            assert_eq!(pat("dddd MMMM"), "вторник март");
            // …but .NET keeps scanning forward past them for a real day number.
            assert_eq!(pat("dddd, MMMM d, yyyy"), "вторник, марта 5, 2024");
            // Abbreviated months have their own genitive forms.
            assert_eq!(pat("MMM"), "март");
            assert_eq!(pat("d MMM"), "5 марта");
            // `de` differs only in the abbreviated form.
            let de = &culture::get("de-DE").expect("de-DE").datetime;
            let pat = |pattern: &str| render_pattern(&morning(), pattern, de).expect("valid");
            assert_eq!(pat("MMM"), "Mär");
            assert_eq!(pat("d MMM"), "5 März");
            assert_eq!(pat("MMMM"), "März");
            assert_eq!(pat("d MMMM"), "5 März");
        }

        /// A culture without the flag never reaches for a genitive name, even
        /// when the pattern would ask for one.
        #[test]
        fn cultures_without_the_flag_keep_the_regular_month_name() {
            let en = &culture::get("en-US").expect("en-US").datetime;
            let pat = |pattern: &str| render_pattern(&morning(), pattern, en).expect("valid");
            assert_eq!(pat("d MMMM"), "5 March");
            assert_eq!(pat("MMMM"), "March");
            // `es` quotes a literal "de" whose `d` .NET's raw scan still counts;
            // harmless here because `es` has no genitive forms.
            assert_eq!(dtf("es-ES", "D"), "martes, 5 de marzo de 2024");
            assert_eq!(dtf("es-ES", "y"), "marzo de 2024");
        }

        /// Patterns that lean on `date_separator` / `time_separator` rather
        /// than literal punctuation, plus a designator that is not `AM`/`PM`.
        #[test]
        fn separators_and_designators() {
            assert_eq!(dtf("sv", "d"), "2024-03-05");
            assert_eq!(dtf("da", "T"), "09.07.03");
            assert_eq!(dtf("fi", "T"), "9.07.03");
            assert_eq!(dtf("nl", "d"), "05-03-2024");
            assert_eq!(dtf("ja", "D"), "2024年3月5日 火曜日");
            assert_eq!(dtf("zh-CN", "D"), "2024年3月5日 星期二");
            assert_eq!(dtf("ko", "t"), "오전 9:07");
            assert_eq!(dtf("ko", "D"), "2024년 3월 5일 화요일");
            // `ar` puts U+200F both in the pattern and in DateSeparator, so
            // each slash comes out doubled.
            assert_eq!(dtf("ar", "d"), "5\u{200f}\u{200f}/3\u{200f}\u{200f}/2024");
            assert_eq!(dtf("ar", "t"), "9:07\u{a0}ص");
            assert_eq!(dtf("ar", "D"), "الثلاثاء\u{60c} 5 مارس\u{60c} 2024");
        }

        /// Known divergence: `ar-SA`'s .NET calendar is `UmAlQuraCalendar`, so
        /// .NET renders 2024-03-05 as `24 شعبان، 1445 بعد الهجرة` — a Hijri
        /// year, month and day. We render the Gregorian fields through the
        /// same (Hijri) month names, which is wrong but loud. Number
        /// formatting for `ar-SA` is unaffected and does match .NET.
        #[test]
        fn ar_sa_dates_diverge_because_its_calendar_is_not_gregorian() {
            assert_eq!(dtf("ar-SA", "d"), "5 ربيع الأول\u{60c} 2024 بعد الهجرة");
            assert_eq!(
                dtf("ar-SA", "D"),
                "الثلاثاء\u{60c} 5 ربيع الأول\u{60c} 2024 بعد الهجرة"
            );
            // The time half is Hijri-free, so it does match .NET.
            assert_eq!(dtf("ar-SA", "T"), "9:07:03\u{a0}ص");
        }

        /// The era name behind the `g` pattern token, which `ar-SA` and every
        /// Japanese/Chinese culture render differently from "A.D.".
        #[test]
        fn era_names() {
            let era = |name: &str| {
                let df = &culture::get(name).expect(name).datetime;
                render_pattern(&morning(), "yyyy g", df).expect("valid")
            };
            assert_eq!(era("en-US"), "2024 AD");
            assert_eq!(era("de-DE"), "2024 n. Chr.");
            assert_eq!(era("fr-FR"), "2024 ap. J.-C.");
            assert_eq!(era("is-IS"), "2024 e.Kr.");
            assert_eq!(era("ru"), "2024 н. э.");
            assert_eq!(era("ja"), "2024 西暦");
            assert_eq!(era("zh-CN"), "2024 公元");
            assert_eq!(era("ar-SA"), "2024 بعد الهجرة");
        }
    }

    #[test]
    fn culture_patterns_and_symbols_are_honored() {
        let mut culture = invariant().clone();
        culture.datetime.short_date_pattern = "dd.MM.yyyy";
        culture.datetime.date_separator = ".";
        culture.datetime.time_separator = "h";
        culture.datetime.short_time_pattern = "H:mm tt";
        culture.datetime.am_designator = "vorm.";
        culture.datetime.abbreviated_month_names[2] = "Mrz";

        let dt = morning();
        assert_eq!(format_datetime(&dt, "d", &culture).unwrap(), "05.03.2024");
        assert_eq!(format_datetime(&dt, "t", &culture).unwrap(), "9h07 vorm.");
        assert_eq!(
            format_datetime(&dt, "g", &culture).unwrap(),
            "05.03.2024 9h07 vorm."
        );
        // o/r/s/u ignore the culture argument, as in .NET.
        assert_eq!(
            format_datetime(&dt, "r", &culture).unwrap(),
            "Tue, 05 Mar 2024 09:07:03 GMT"
        );
        assert_eq!(
            format_datetime(&dt, "u", &culture).unwrap(),
            "2024-03-05 09:07:03Z"
        );
        assert_eq!(
            format_datetime(&dt, "o", &culture).unwrap(),
            "2024-03-05T09:07:03.0000000"
        );
    }
}
