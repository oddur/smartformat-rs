//! The .NET *standard* `TimeSpan` format specifiers — `c` (with its aliases
//! `t` and `T`), `g` and `G` — ported from `System.Globalization.TimeSpanFormat`.
//!
//! This is what [`DefaultFormatter`](crate::formatter::DefaultFormatter) writes
//! for a [`Value::TimeSpan`](crate::Value::TimeSpan), since .NET's `TimeSpan`
//! is `ISpanFormattable` and gets the specifier handed to it. Custom patterns
//! (`hh\:mm`) are [`FormatSpecError::Unsupported`], as they are for numbers and
//! dates; see the non-goals in `DESIGN.md`.

use jiff::SignedDuration;

use crate::fmt::culture::CultureData;
use crate::fmt::FormatSpecError;

use super::utility::{ticks, TICKS_PER_DAY, TICKS_PER_HOUR, TICKS_PER_MINUTE, TICKS_PER_SECOND};

/// The message .NET's `FormatException` carries for a specifier it does not
/// recognize (`System.SR.Format_InvalidString`), which is the same string the
/// date/time specifiers use.
pub const INVALID_SPEC_MESSAGE: &str = crate::fmt::date::INVALID_SPEC_MESSAGE;

/// Digits after the decimal separator, .NET's `TimeSpan` tick resolution.
const FRACTION_DIGITS: usize = 7;

/// Formats a duration with a .NET *standard* `TimeSpan` format specifier,
/// producing byte-identical output to .NET's `span.ToString(spec, culture)`.
///
/// The empty specifier is `c`, as in .NET.
pub fn format_timespan(
    span: &SignedDuration,
    spec: &str,
    culture: &CultureData,
) -> Result<String, FormatSpecError> {
    let ticks = ticks(span);
    let mut chars = spec.chars();
    match (chars.next(), chars.next()) {
        // `string.IsNullOrEmpty(format)` is the constant format.
        (None, _) => Ok(format_constant(ticks)),
        // .NET reads 'c', 't' and 'T' as the same constant format.
        (Some('c' | 't' | 'T'), None) => Ok(format_constant(ticks)),
        (Some('g'), None) => Ok(format_general(ticks, culture, General::Short)),
        (Some('G'), None) => Ok(format_general(ticks, culture, General::Long)),
        (Some(_), None) => Err(FormatSpecError::Invalid(spec.to_owned())),
        // Longer than one character is a custom pattern, which .NET renders
        // and we do not.
        _ => Err(FormatSpecError::Unsupported(spec.to_owned())),
    }
}

/// .NET `TimeSpan.ToString()`: the constant (`c`) format, which is the same in
/// every culture.
///
/// `[-][d.]hh:mm:ss[.fffffff]` — the days and the fraction are left out when
/// they are zero, and the fraction keeps all seven digits when it is not.
pub fn timespan_to_string(span: &SignedDuration) -> String {
    format_constant(ticks(span))
}

/// Which of the two general formats to write.
#[derive(Clone, Copy)]
enum General {
    /// `g`: `[-][d:]h:mm:ss[.FFFFFFF]`.
    Short,
    /// `G`: `[-]d:hh:mm:ss.fffffff`.
    Long,
}

/// The components .NET's `TimeSpanFormat.FormatStandard` splits a tick count
/// into: each is the absolute value of its part.
struct Parts {
    negative: bool,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    fraction: i64,
}

impl Parts {
    fn of(ticks: i64) -> Self {
        // Both divisions are exact for `i64::MIN` too: the quotient is well
        // inside the range and the remainder is smaller than a day.
        let mut days = ticks / TICKS_PER_DAY;
        let mut time = ticks % TICKS_PER_DAY;
        if ticks < 0 {
            days = -days;
            time = -time;
        }
        Self {
            negative: ticks < 0,
            days,
            hours: time / TICKS_PER_HOUR % 24,
            minutes: time / TICKS_PER_MINUTE % 60,
            seconds: time / TICKS_PER_SECOND % 60,
            fraction: time % TICKS_PER_SECOND,
        }
    }
}

/// .NET `StandardFormat.C`.
fn format_constant(ticks: i64) -> String {
    let parts = Parts::of(ticks);
    let mut out = String::with_capacity(26);
    if parts.negative {
        out.push('-');
    }
    if parts.days != 0 {
        push_int(&mut out, parts.days);
        out.push('.');
    }
    push_padded(&mut out, parts.hours);
    out.push(':');
    push_padded(&mut out, parts.minutes);
    out.push(':');
    push_padded(&mut out, parts.seconds);
    if parts.fraction != 0 {
        out.push('.');
        push_fraction(&mut out, parts.fraction, false);
    }
    out
}

/// .NET `StandardFormat.g` / `StandardFormat.G`, whose only culture-sensitive
/// part is the decimal separator — the `:` between the components is a
/// literal there, even for a culture whose `TimeSeparator` is something else
/// (probed: `fi-FI` separates times with `.` and still writes `1:1:01:01,001`).
fn format_general(ticks: i64, culture: &CultureData, general: General) -> String {
    let parts = Parts::of(ticks);
    let mut out = String::with_capacity(32);
    if parts.negative {
        out.push('-');
    }
    match general {
        General::Short => {
            if parts.days != 0 {
                push_int(&mut out, parts.days);
                out.push(':');
            }
            push_int(&mut out, parts.hours);
        }
        General::Long => {
            push_int(&mut out, parts.days);
            out.push(':');
            push_padded(&mut out, parts.hours);
        }
    }
    out.push(':');
    push_padded(&mut out, parts.minutes);
    out.push(':');
    push_padded(&mut out, parts.seconds);
    match general {
        General::Short if parts.fraction == 0 => {}
        General::Short => {
            out.push_str(culture.number.decimal_separator);
            push_fraction(&mut out, parts.fraction, true);
        }
        General::Long => {
            out.push_str(culture.number.decimal_separator);
            push_fraction(&mut out, parts.fraction, false);
        }
    }
    out
}

fn push_int(out: &mut String, value: i64) {
    out.push_str(&value.to_string());
}

fn push_padded(out: &mut String, value: i64) {
    if value < 10 {
        out.push('0');
    }
    out.push_str(&value.to_string());
}

/// The seven fraction digits, with the trailing zeros dropped for the `g`
/// format (.NET's `FFFFFFF`) and kept for the others (`fffffff`).
fn push_fraction(out: &mut String, fraction: i64, trim: bool) {
    let digits = format!("{fraction:0>width$}", width = FRACTION_DIGITS);
    let digits = if trim {
        digits.trim_end_matches('0')
    } else {
        &digits
    };
    out.push_str(digits);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Every expectation here was probed against .NET 10.

    use super::*;
    use crate::extensions::time::utility::duration;
    use crate::fmt::culture;

    fn spans() -> Vec<(&'static str, i64)> {
        vec![
            ("zero", 0),
            ("1d1h1m1s1ms", 900_610_010_000),
            ("neg", -900_610_010_000),
            ("nodays", 73_840_000_000),
            ("tick", 1),
            ("negtick", -1),
            ("max", i64::MAX),
            ("min", i64::MIN),
            ("halfsecond", 5_000_000),
            ("bigdays", 8_640_000_000_000_000),
        ]
    }

    fn format(ticks: i64, spec: &str) -> String {
        format_timespan(&duration(ticks), spec, culture::invariant()).expect("a standard spec")
    }

    #[test]
    fn the_constant_format_is_dotnet_to_string() {
        let expected = [
            ("zero", "00:00:00"),
            ("1d1h1m1s1ms", "1.01:01:01.0010000"),
            ("neg", "-1.01:01:01.0010000"),
            ("nodays", "02:03:04"),
            ("tick", "00:00:00.0000001"),
            ("negtick", "-00:00:00.0000001"),
            ("max", "10675199.02:48:05.4775807"),
            ("min", "-10675199.02:48:05.4775808"),
            ("halfsecond", "00:00:00.5000000"),
            ("bigdays", "10000.00:00:00"),
        ];
        for ((name, ticks), (_, want)) in spans().into_iter().zip(expected) {
            assert_eq!(timespan_to_string(&duration(ticks)), want, "{name}");
            // The empty spec and 'c', 't', 'T' are all the constant format.
            for spec in ["", "c", "t", "T"] {
                assert_eq!(format(ticks, spec), want, "{name} with {spec:?}");
            }
        }
    }

    #[test]
    fn the_general_formats_are_dotnet_g_and_capital_g() {
        let expected = [
            ("zero", "0:00:00", "0:00:00:00.0000000"),
            ("1d1h1m1s1ms", "1:1:01:01.001", "1:01:01:01.0010000"),
            ("neg", "-1:1:01:01.001", "-1:01:01:01.0010000"),
            ("nodays", "2:03:04", "0:02:03:04.0000000"),
            ("tick", "0:00:00.0000001", "0:00:00:00.0000001"),
            ("negtick", "-0:00:00.0000001", "-0:00:00:00.0000001"),
            (
                "max",
                "10675199:2:48:05.4775807",
                "10675199:02:48:05.4775807",
            ),
            (
                "min",
                "-10675199:2:48:05.4775808",
                "-10675199:02:48:05.4775808",
            ),
            ("halfsecond", "0:00:00.5", "0:00:00:00.5000000"),
            ("bigdays", "10000:0:00:00", "10000:00:00:00.0000000"),
        ];
        for ((name, ticks), (_, short, long)) in spans().into_iter().zip(expected) {
            assert_eq!(format(ticks, "g"), short, "{name} with g");
            assert_eq!(format(ticks, "G"), long, "{name} with G");
        }
    }

    #[test]
    fn only_the_decimal_separator_follows_the_culture() {
        let german = culture::get("de-DE").expect("de-DE is generated");
        let span = duration(900_610_010_000);
        assert_eq!(
            format_timespan(&span, "g", german),
            Ok("1:1:01:01,001".to_owned())
        );
        assert_eq!(
            format_timespan(&span, "G", german),
            Ok("1:01:01:01,0010000".to_owned())
        );
        // The constant format is the same in every culture.
        assert_eq!(
            format_timespan(&span, "c", german),
            Ok("1.01:01:01.0010000".to_owned())
        );
        // A culture whose time separator is not ':' still writes ':'.
        let finnish = culture::get("fi").expect("Finnish is generated");
        assert_eq!(finnish.datetime.time_separator, ".");
        assert_eq!(
            format_timespan(&span, "g", finnish),
            Ok("1:1:01:01,001".to_owned())
        );
    }

    #[test]
    fn an_unknown_specifier_is_invalid_and_a_pattern_unsupported() {
        let span = duration(900_610_010_000);
        let inv = culture::invariant();
        // One character .NET does not know is a `FormatException` there too.
        assert_eq!(
            format_timespan(&span, "x", inv),
            Err(FormatSpecError::Invalid("x".to_owned()))
        );
        assert_eq!(
            format_timespan(&span, " ", inv),
            Err(FormatSpecError::Invalid(" ".to_owned()))
        );
        // Anything longer is a custom pattern: .NET renders `hh\:mm`, and
        // rejects `ccc`. We reject both, loudly.
        assert_eq!(
            format_timespan(&span, r"hh\:mm", inv),
            Err(FormatSpecError::Unsupported(r"hh\:mm".to_owned()))
        );
        assert_eq!(
            format_timespan(&span, "ccc", inv),
            Err(FormatSpecError::Unsupported("ccc".to_owned()))
        );
    }
}
