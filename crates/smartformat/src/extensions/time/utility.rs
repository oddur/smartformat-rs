//! Turning a duration into human-readable parts, ported from
//! `SmartFormat.Extensions.Time/Utilities/TimeSpanUtility.cs`.
//!
//! Every step is done on .NET ticks (100 ns) with .NET's own arithmetic —
//! `TimeSpan.TotalDays` and friends are `double` divisions, `TimeSpan.FromDays`
//! and friends truncate toward zero and overflow at `TimeSpan.MaxValue` — so
//! the rounding lands where .NET's lands, digit for digit.

use std::fmt;

use jiff::SignedDuration;

use crate::fmt::culture::CultureData;

use super::options::TimeSpanFormatOptions;
use super::text_info::TimeTextInfo;

/// .NET `TimeSpan.TicksPerMillisecond`.
pub const TICKS_PER_MILLISECOND: i64 = 10_000;
/// .NET `TimeSpan.TicksPerSecond`.
pub const TICKS_PER_SECOND: i64 = 10_000_000;
/// .NET `TimeSpan.TicksPerMinute`.
pub const TICKS_PER_MINUTE: i64 = 600_000_000;
/// .NET `TimeSpan.TicksPerHour`.
pub const TICKS_PER_HOUR: i64 = 36_000_000_000;
/// .NET `TimeSpan.TicksPerDay`.
pub const TICKS_PER_DAY: i64 = 864_000_000_000;

/// .NET `TimeSpan.MaxMilliSeconds`, the value `TotalMilliseconds` is capped at.
const MAX_MILLISECONDS: f64 = (i64::MAX / TICKS_PER_MILLISECOND) as f64;
/// .NET `TimeSpan.MinMilliSeconds`.
const MIN_MILLISECONDS: f64 = (i64::MIN / TICKS_PER_MILLISECOND) as f64;

/// The options .NET starts with, `TimeSpanUtility.DefaultFormatOptions` — and
/// `AbsoluteDefaults`, which its static constructor sets to the same value.
pub const DEFAULT_FORMAT_OPTIONS: TimeSpanFormatOptions = TimeSpanFormatOptions::from_bits(
    TimeSpanFormatOptions::ABBREVIATE_OFF.bits()
        | TimeSpanFormatOptions::LESS_THAN.bits()
        | TimeSpanFormatOptions::TRUNCATE_AUTO.bits()
        | TimeSpanFormatOptions::RANGE_SECONDS.bits()
        | TimeSpanFormatOptions::RANGE_DAYS.bits(),
);

/// The safeguard .NET merges in last, `TimeSpanUtility.AbsoluteDefaults`.
pub const ABSOLUTE_DEFAULTS: TimeSpanFormatOptions = DEFAULT_FORMAT_OPTIONS;

/// The message of .NET's `OverflowException` when an arithmetic step leaves
/// the range of a `TimeSpan` (`System.SR.Overflow_TimeSpanTooLong`).
pub const OVERFLOW_MESSAGE: &str = "TimeSpan overflowed because the duration is too long.";

/// A step of the rounding or the unit-by-unit subtraction left the range of a
/// .NET `TimeSpan`, which is where .NET throws an `OverflowException`.
///
/// It takes a duration close to the ±10,675,199-day limit to get here: the
/// rounding step rounds *away* from zero when the "less than" text is off, and
/// `TimeSpan.MaxValue` rounded up to whole days no longer fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSpanOverflow;

impl fmt::Display for TimeSpanOverflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(OVERFLOW_MESSAGE)
    }
}

impl std::error::Error for TimeSpanOverflow {}

/// The .NET tick count of a duration, saturating at `TimeSpan.MinValue` /
/// `TimeSpan.MaxValue`.
///
/// A [`SignedDuration`] counts nanoseconds and spans far more than a .NET
/// `TimeSpan` can, so a duration that no `TimeSpan` could hold is clamped to
/// the nearest end of the range — there is no .NET behavior to match for a
/// value .NET cannot represent. Anything that came from .NET converts exactly:
/// a tick is 100 ns.
pub fn ticks(duration: &SignedDuration) -> i64 {
    let ticks = duration.as_nanos() / 100;
    i64::try_from(ticks).unwrap_or(if ticks.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    })
}

/// The duration a .NET tick count spells.
pub fn duration(ticks: i64) -> SignedDuration {
    // The widest tick count is 9.2e20 ns, which is 9.2e11 seconds — nowhere
    // near the seconds a `SignedDuration` holds, so this never overflows.
    SignedDuration::from_nanos_i128(i128::from(ticks) * 100)
}

/// Writes a duration as human-readable text, .NET
/// `TimeSpanUtility.ToTimeString`.
///
/// ```
/// use jiff::SignedDuration;
/// use smartformat::extensions::time::{self, TimeSpanFormatOptions};
/// use smartformat::fmt::culture;
///
/// let english = time::common_language("en").expect("English is shipped");
/// let span = SignedDuration::from_secs(90_061);
/// assert_eq!(
///     time::to_time_string(
///         &span,
///         TimeSpanFormatOptions::NONE,
///         english,
///         culture::invariant(),
///     )
///     .unwrap(),
///     "1 day 1 hour 1 minute 1 second"
/// );
/// ```
pub fn to_time_string(
    from_time: &SignedDuration,
    options: TimeSpanFormatOptions,
    time_text_info: &TimeTextInfo,
    culture: &CultureData,
) -> Result<String, TimeSpanOverflow> {
    Ok(to_time_parts(from_time, options, time_text_info, culture)?.join(" "))
}

/// The human-readable parts of a duration, .NET `TimeSpanUtility.ToTimeParts`:
/// one string per unit written, which the `time` formatter joins with spaces
/// or hands to a nested `list` format.
pub fn to_time_parts(
    from_time: &SignedDuration,
    options: TimeSpanFormatOptions,
    time_text_info: &TimeTextInfo,
    culture: &CultureData,
) -> Result<Vec<String>, TimeSpanOverflow> {
    time_parts(ticks(from_time), options, time_text_info, culture)
}

/// [`to_time_parts`] on a tick count, which is what the formatter has after it
/// has subtracted two dates.
pub(crate) fn time_parts(
    from_time: i64,
    options: TimeSpanFormatOptions,
    time_text_info: &TimeTextInfo,
    culture: &CultureData,
) -> Result<Vec<String>, TimeSpanOverflow> {
    // If there are any missing options, merge with the defaults. Also, as a
    // safeguard against missing DefaultFormatOptions, merge with the absolute
    // defaults.
    let options = options
        .merge(DEFAULT_FORMAT_OPTIONS)
        .merge(ABSOLUTE_DEFAULTS);

    // Extract the individual options. The range and truncate groups can no
    // longer be empty, both being filled in by the merge above.
    let range = options.mask(TimeSpanFormatOptions::RANGE_MASK);
    let range_max = range.last_flag().expect("the merge filled in the range");
    let range_min = range.first_flag().expect("the merge filled in the range");
    let truncate = options
        .mask(TimeSpanFormatOptions::TRUNCATE_MASK)
        .first_flag()
        .expect("the merge filled in the truncation");
    let less_than =
        options.mask(TimeSpanFormatOptions::LESS_THAN_MASK) != TimeSpanFormatOptions::LESS_THAN_OFF;
    let abbreviate = options.mask(TimeSpanFormatOptions::ABBREVIATE_MASK)
        != TimeSpanFormatOptions::ABBREVIATE_OFF;
    // "Less than" floors the value and says it is less than the unit;
    // otherwise it is rounded up.
    let round: fn(f64) -> f64 = if less_than { f64::floor } else { f64::ceil };

    // Round the whole duration to the smallest unit that may be written.
    let mut from_time = match range_min {
        TimeSpanFormatOptions::RANGE_WEEKS => {
            interval(round(total_days(from_time) / 7.0) * 7.0, TICKS_PER_DAY)?
        }
        TimeSpanFormatOptions::RANGE_DAYS => interval(round(total_days(from_time)), TICKS_PER_DAY)?,
        TimeSpanFormatOptions::RANGE_HOURS => {
            interval(round(total_hours(from_time)), TICKS_PER_HOUR)?
        }
        TimeSpanFormatOptions::RANGE_MINUTES => {
            interval(round(total_minutes(from_time)), TICKS_PER_MINUTE)?
        }
        TimeSpanFormatOptions::RANGE_SECONDS => {
            interval(round(total_seconds(from_time)), TICKS_PER_SECOND)?
        }
        TimeSpanFormatOptions::RANGE_MILLI_SECONDS => {
            interval(round(total_milliseconds(from_time)), TICKS_PER_MILLISECOND)?
        }
        // Unreachable: `range_min` is one of the six range flags.
        _ => from_time,
    };

    let mut result: Vec<String> = Vec::new();
    let mut unit = range_max;
    loop {
        // Determine the value and title. Each value is what is left over from
        // the larger units, and is taken out of the remainder.
        //
        // .NET casts the `double` to `int`, which saturates rather than
        // wrapping — so a range of milliseconds over a span of weeks writes
        // `int.MaxValue` milliseconds — and so does an `as` cast here.
        let (value, taken) = match unit {
            TimeSpanFormatOptions::RANGE_WEEKS => {
                let value = (total_days(from_time) / 7.0).floor() as i32;
                (value, interval_of(value.wrapping_mul(7), TICKS_PER_DAY)?)
            }
            TimeSpanFormatOptions::RANGE_DAYS => {
                let value = total_days(from_time).floor() as i32;
                (value, interval_of(value, TICKS_PER_DAY)?)
            }
            TimeSpanFormatOptions::RANGE_HOURS => {
                let value = total_hours(from_time).floor() as i32;
                (value, interval_of(value, TICKS_PER_HOUR)?)
            }
            TimeSpanFormatOptions::RANGE_MINUTES => {
                let value = total_minutes(from_time).floor() as i32;
                (value, interval_of(value, TICKS_PER_MINUTE)?)
            }
            TimeSpanFormatOptions::RANGE_SECONDS => {
                let value = total_seconds(from_time).floor() as i32;
                (value, interval_of(value, TICKS_PER_SECOND)?)
            }
            TimeSpanFormatOptions::RANGE_MILLI_SECONDS => {
                let value = total_milliseconds(from_time).floor() as i32;
                (value, interval_of(value, TICKS_PER_MILLISECOND)?)
            }
            // Unreachable: every flag between `range_max` and `range_min` is a
            // range flag. .NET skips the rest of the iteration here.
            _ => {
                if unit == range_min {
                    break;
                }
                unit = unit.shift_down();
                continue;
            }
        };
        from_time = from_time.checked_sub(taken).ok_or(TimeSpanOverflow)?;

        // Determine whether to display this value.
        if let Some(mut display_this_value) = should_truncate(truncate, value, !result.is_empty()) {
            // .NET `PrepareOutput`: the smallest unit has to write *something*
            // (even if it is a zero), and writes the "less than 1 unit" text
            // instead when there is nothing else to show.
            if unit == range_min && result.is_empty() {
                display_this_value = true;
                if less_than && value < 1 {
                    let unit_title = time_text_info.unit_text(range_min, 1, abbreviate, culture);
                    result.push(time_text_info.less_than_text(&unit_title));
                    display_this_value = false;
                }
            }

            if display_this_value {
                let unit_title = time_text_info.unit_text(unit, value, abbreviate, culture);
                if !unit_title.is_empty() {
                    result.push(unit_title);
                }
            }
        }

        if unit == range_min {
            break;
        }
        unit = unit.shift_down();
    }

    Ok(result)
}

/// .NET `ShouldTruncate`: `None` skips the unit outright, `Some(display)` says
/// whether to write it.
fn should_truncate(
    truncate: TimeSpanFormatOptions,
    value: i32,
    text_started: bool,
) -> Option<bool> {
    match truncate {
        TimeSpanFormatOptions::TRUNCATE_SHORTEST => {
            // Continue with the next unit once anything has been written.
            if text_started {
                None
            } else {
                Some(value > 0)
            }
        }
        TimeSpanFormatOptions::TRUNCATE_AUTO => Some(value > 0),
        TimeSpanFormatOptions::TRUNCATE_FILL => Some(text_started || value > 0),
        TimeSpanFormatOptions::TRUNCATE_FULL => Some(true),
        // Unreachable: `truncate` is one of the four truncation flags.
        _ => None,
    }
}

/// Rounds a duration to the nearest multiple of an interval, .NET
/// `TimeSpanUtility.Round`: `Round("00:57:00", TicksPerMinute * 5)` is
/// `"00:55:00"`.
///
/// A half interval rounds away from zero for a positive duration and toward
/// zero for a negative one, as .NET's truncating remainder makes it.
pub fn round(from_time: &SignedDuration, interval_ticks: i64) -> SignedDuration {
    duration(round_ticks(ticks(from_time), interval_ticks))
}

/// [`round`] on tick counts.
fn round_ticks(from_ticks: i64, interval_ticks: i64) -> i64 {
    let mut extra = from_ticks % interval_ticks;
    if extra >= interval_ticks >> 1 {
        extra -= interval_ticks;
    }
    from_ticks - extra
}

/// .NET `TimeSpan.TotalDays`.
fn total_days(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_DAY as f64
}

/// .NET `TimeSpan.TotalHours`.
fn total_hours(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_HOUR as f64
}

/// .NET `TimeSpan.TotalMinutes`.
fn total_minutes(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_MINUTE as f64
}

/// .NET `TimeSpan.TotalSeconds`.
fn total_seconds(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_SECOND as f64
}

/// .NET `TimeSpan.TotalMilliseconds`, which — alone among the `Total`
/// properties — clamps its result to the whole milliseconds a `TimeSpan` can
/// hold.
fn total_milliseconds(ticks: i64) -> f64 {
    let total = ticks as f64 / TICKS_PER_MILLISECOND as f64;
    total.clamp(MIN_MILLISECONDS, MAX_MILLISECONDS)
}

/// .NET `TimeSpan.Interval`, the body of `TimeSpan.FromDays` and its
/// siblings: the value scaled to ticks, truncated toward zero, or an overflow.
fn interval(value: f64, ticks_per_unit: i64) -> Result<i64, TimeSpanOverflow> {
    // .NET's `TimeSpan.FromXxx` rejects a NaN before it scales; no `Total`
    // property of a finite tick count produces one.
    if value.is_nan() {
        return Err(TimeSpanOverflow);
    }
    let scaled = value * ticks_per_unit as f64;
    // .NET compares the scaled `double` against `long.MaxValue` and
    // `long.MinValue`, both of which convert to ±2^63 — so 2^63 exactly is
    // `TimeSpan.MaxValue` rather than an overflow.
    const MAX: f64 = 9_223_372_036_854_775_808.0; // 2^63
    if scaled.is_nan() || !(-MAX..=MAX).contains(&scaled) {
        return Err(TimeSpanOverflow);
    }
    if scaled == MAX {
        return Ok(i64::MAX);
    }
    Ok(scaled as i64)
}

/// [`interval`] for the whole-number values the unit loop subtracts.
fn interval_of(value: i32, ticks_per_unit: i64) -> Result<i64, TimeSpanOverflow> {
    interval(f64::from(value), ticks_per_unit)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::time::options::{self, TimeSpanFormatOptions as O};
    use crate::extensions::time::text_info::common_language;
    use crate::fmt::culture;

    fn parts(ticks_value: i64, options_text: &str, language: &str) -> String {
        time_parts(
            ticks_value,
            options::parse(options_text),
            common_language(language).expect("a shipped language"),
            culture::invariant(),
        )
        .expect("no overflow")
        .join(" ")
    }

    fn span(days: i64, hours: i64, minutes: i64, seconds: i64, millis: i64) -> i64 {
        days * TICKS_PER_DAY
            + hours * TICKS_PER_HOUR
            + minutes * TICKS_PER_MINUTE
            + seconds * TICKS_PER_SECOND
            + millis * TICKS_PER_MILLISECOND
    }

    #[test]
    fn ticks_round_trip_a_duration() {
        assert_eq!(ticks(&SignedDuration::from_secs(1)), TICKS_PER_SECOND);
        assert_eq!(ticks(&SignedDuration::from_nanos(100)), 1);
        // Sub-tick precision truncates toward zero, as no .NET value can have it.
        assert_eq!(ticks(&SignedDuration::from_nanos(199)), 1);
        assert_eq!(ticks(&SignedDuration::from_nanos(-199)), -1);
        // Beyond TimeSpan's range there is nothing to match, so it saturates.
        assert_eq!(ticks(&SignedDuration::MAX), i64::MAX);
        assert_eq!(ticks(&SignedDuration::MIN), i64::MIN);
        assert_eq!(duration(TICKS_PER_DAY), SignedDuration::from_hours(24));
    }

    #[test]
    fn total_milliseconds_is_clamped_like_dotnet() {
        // The one `Total` property .NET caps.
        assert_eq!(total_milliseconds(i64::MIN), -922_337_203_685_477.0);
        assert_eq!(total_milliseconds(i64::MAX), 922_337_203_685_477.0);
        assert_eq!(total_milliseconds(TICKS_PER_SECOND), 1000.0);
    }

    #[test]
    fn interval_truncates_toward_zero_and_overflows_like_dotnet() {
        // Probed against .NET 10.
        assert_eq!(interval(1.0 / 3.0, TICKS_PER_DAY), Ok(288_000_000_000));
        assert_eq!(interval(0.15, TICKS_PER_MILLISECOND), Ok(1_500));
        assert_eq!(interval(-1.000_000_05, TICKS_PER_SECOND), Ok(-10_000_000));
        assert_eq!(interval(1.999_99, TICKS_PER_MILLISECOND), Ok(19_999));
        assert_eq!(
            interval(-922_337_203_685_477.0, TICKS_PER_MILLISECOND),
            Ok(-9_223_372_036_854_769_664)
        );
        assert_eq!(
            interval(-922_337_203_685_478.0, TICKS_PER_MILLISECOND),
            Err(TimeSpanOverflow)
        );
        assert_eq!(interval(1e30, TICKS_PER_DAY), Err(TimeSpanOverflow));
        assert_eq!(interval(f64::NAN, TICKS_PER_DAY), Err(TimeSpanOverflow));
    }

    #[test]
    fn rounding_matches_the_dotnet_utility() {
        // .NET's own doc comment: Round("00:57:00", TicksPerMinute * 5).
        assert_eq!(
            round_ticks(57 * TICKS_PER_MINUTE, TICKS_PER_MINUTE * 5),
            55 * TICKS_PER_MINUTE
        );
        // Probed: half an interval rounds up, and a negative one toward zero.
        assert_eq!(round_ticks(11 * TICKS_PER_HOUR, TICKS_PER_DAY), 0);
        assert_eq!(
            round_ticks(12 * TICKS_PER_HOUR, TICKS_PER_DAY),
            TICKS_PER_DAY
        );
        assert_eq!(round_ticks(-11 * TICKS_PER_HOUR, TICKS_PER_DAY), 0);
        assert_eq!(round_ticks(-13 * TICKS_PER_HOUR, TICKS_PER_DAY), 0);
    }

    #[test]
    fn a_rounded_span_writes_one_day() {
        // The .NET test `TimeSpanRoundedToString`.
        let rounded = round(&SignedDuration::from_hours(23), TICKS_PER_DAY);
        assert_eq!(
            to_time_string(
                &rounded,
                O::NONE,
                common_language("en").expect("English is shipped"),
                culture::invariant()
            ),
            Ok("1 day".to_owned())
        );
    }

    #[test]
    fn the_truncation_flags_pick_the_units_written() {
        // Probed against 3.6.1 with a span of 2 hours 2 seconds.
        let two = span(0, 2, 0, 2, 0);
        assert_eq!(parts(two, "days milliseconds", "en"), "2 hours 2 seconds");
        assert_eq!(
            parts(two, "days milliseconds auto", "en"),
            "2 hours 2 seconds"
        );
        assert_eq!(parts(two, "days milliseconds short", "en"), "2 hours");
        assert_eq!(
            parts(two, "days milliseconds fill", "en"),
            "2 hours 0 minutes 2 seconds 0 milliseconds"
        );
        assert_eq!(
            parts(two, "days milliseconds full", "en"),
            "0 days 2 hours 0 minutes 2 seconds 0 milliseconds"
        );
    }

    #[test]
    fn a_negative_span_counts_down_from_the_larger_unit() {
        // Probed: -1.01:01:01.001 over the whole range.
        let negative = -span(1, 1, 1, 1, 1);
        assert_eq!(
            parts(negative, "weeks milliseconds full", "en"),
            "-1 weeks 5 days 22 hours 58 minutes 58 seconds 999 milliseconds"
        );
        // The default range rounds down to whole seconds first.
        assert_eq!(parts(negative, "", "en"), "22 hours 58 minutes 58 seconds");
        // Without the "less than" text the rounding goes the other way.
        assert_eq!(
            parts(negative, "noless", "en"),
            "22 hours 58 minutes 59 seconds"
        );
    }

    #[test]
    fn a_value_too_large_for_an_int_saturates_like_dotnet() {
        // Probed: 70 days in milliseconds is int.MaxValue milliseconds.
        assert_eq!(
            parts(70 * TICKS_PER_DAY, "milliseconds", "en"),
            "2147483647 milliseconds"
        );
    }

    #[test]
    fn rounding_past_the_range_of_a_timespan_overflows() {
        let english = common_language("en").expect("English is shipped");
        let inv = culture::invariant();
        // Probed: TimeSpan.MinValue floored to whole seconds no longer fits.
        assert_eq!(
            time_parts(i64::MIN, O::NONE, english, inv),
            Err(TimeSpanOverflow)
        );
        // ... and TimeSpan.MaxValue rounded up to whole weeks does not either.
        assert_eq!(
            time_parts(i64::MAX, options::parse("noless weeks"), english, inv),
            Err(TimeSpanOverflow)
        );
        // Milliseconds are clamped before the rounding, so these survive.
        assert_eq!(
            time_parts(i64::MIN, options::parse("milliseconds"), english, inv),
            Ok(vec!["less than 1 millisecond".to_owned()])
        );
        assert_eq!(
            time_parts(i64::MAX, options::parse("weeks"), english, inv),
            Ok(vec!["1525028 weeks".to_owned()])
        );
    }

    #[test]
    fn the_smallest_unit_always_writes_something() {
        // Probed: zero with every truncation flag.
        assert_eq!(parts(0, "", "en"), "less than 1 second");
        assert_eq!(parts(0, "noless", "en"), "0 seconds");
        assert_eq!(parts(0, "abbr", "en"), "less than 1s");
        assert_eq!(
            parts(0, "weeks milliseconds full", "en"),
            "0 weeks 0 days 0 hours 0 minutes 0 seconds 0 milliseconds"
        );
        assert_eq!(
            parts(0, "weeks milliseconds fill", "en"),
            "less than 1 millisecond"
        );
        assert_eq!(parts(0, "hours", "en"), "less than 1 hour");
        assert_eq!(parts(0, "noless weeks", "en"), "0 weeks");
    }
}
