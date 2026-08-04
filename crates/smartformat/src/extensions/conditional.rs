//! Port of SmartFormat.NET's `ConditionalFormatter` (milestone M2).
//!
//! Ported from `src/SmartFormat/Extensions/ConditionalFormatter.cs`.
//!
//! `{0:cond:zero|one|many}` picks one of the `|`-separated parts of the format
//! by looking at the value: a number picks the part its integer value indexes,
//! a bool picks `true|false`, a string picks `has-value|empty`, and `null`
//! picks the second part. A part may also carry a *complex condition* —
//! `>=100?a lot` — in which case the first condition that holds wins:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! // Registered by default, behind `plural` and ahead of `choose`.
//! let smart = SmartFormatter::default();
//!
//! let args = Value::List(vec![Value::Int(2)]);
//! assert_eq!(smart.format("{0:cond:none|one|many}", &args).unwrap(), "many");
//! assert_eq!(smart.format("{0:cond:>1?many|few}", &args).unwrap(), "many");
//! ```
//!
//! The formatter auto-detects, so a placeholder that names no formatter but
//! whose format holds a `|` is handled here — `{0:part(s)|car}` renders
//! `part(s)` for `true`.
//!
//! Unlike [`ChooseFormatter`](super::ChooseFormatter) and
//! [`PluralLocalizationFormatter`](super::plural::PluralLocalizationFormatter),
//! this formatter never raises a .NET `FormattingException`: every one of its
//! errors is a plain `FormatException`, `ArgumentOutOfRangeException` or
//! `OverflowException` that the evaluator catches, so every one of them goes
//! through [`FormattingInfo::plain_error_here`] and carries no
//! `Error parsing format string: … at {index}` envelope —
//! `ErrorAction::OutputErrorInResult` writes the inner exception's message,
//! which is the bare text. Probed against 3.6.1: `{0:cond:Yes}` outputs
//! `Formatter named 'cond' requires at least 2 format parameters.` and nothing
//! else. (Only `ErrorAction::Error` sees the envelope in .NET, since the
//! evaluator adds it while rethrowing; see `Engine::handle_format_error`,
//! which does not, for every wrapped error alike.)

use std::cmp::Ordering;

use crate::dotnet_messages::{
    not_in_a_correct_format, INT32_OVERFLOW, OUT_OF_RANGE_INDEX as INDEX_OUT_OF_RANGE,
};
use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::Format;
use crate::value::Value;
use crate::Error;

use super::{split_format, split_part, InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The .NET `ConditionalFormatter.Name`. .NET 3.6.1 still carries an obsolete
/// `Names` array holding `conditional` / `cond` / `""`, but only `Name`
/// selects a formatter, so `{0:conditional:a|b}` is "No suitable Formatter
/// could be found" there and here.
const NAME: &str = "cond";

// The messages .NET's own exceptions carry, which the goldens pin.
/// `OverflowException` from `Convert.ToDecimal` and from `decimal.Parse`
/// (`System.SR.Overflow_Decimal`).
const DECIMAL_OVERFLOW: &str = "Value was either too large or too small for a Decimal.";
// `INT32_OVERFLOW` is the `OverflowException` from the `(int)` cast of the
// value, and `INDEX_OUT_OF_RANGE` the `ArgumentOutOfRangeException` from
// indexing one past the last parameter, which .NET does when every parameter
// is a complex condition and none of them holds.

/// Picks one of several formats by the value, ported from .NET
/// `ConditionalFormatter`.
#[derive(Debug, Clone)]
pub struct ConditionalFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
}

impl ConditionalFormatter {
    /// A formatter named `cond`, splitting on `|`, auto-detected — the .NET
    /// defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: true,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the formats are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output — `{0:cond:|Yes|~|No|}`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether a placeholder that names no formatter may be handled here
    /// (.NET `CanAutoDetect`, which defaults to `true`).
    pub fn set_can_auto_detect(&mut self, can_auto_detect: bool) {
        self.can_auto_detect = can_auto_detect;
    }
}

/// .NET's `case DateTime` arms: `past|present|future` when there are exactly
/// three parameters and the value falls on today's date, `past|future`
/// otherwise.
///
/// The moment compared against is
/// [`SmartSettings::now`](crate::SmartSettings::now), or the system's local
/// time when it is not pinned — .NET's `SystemTime.Now()`, which is what both
/// arms read.
///
/// .NET converts both moments to UTC before it compares them
/// (`dateTimeArg.ToUniversalTime() <= SystemTime.Now().ToUniversalTime()`). A
/// [`jiff::civil::DateTime`] carries no zone, so the comparison here is the
/// plain one, which agrees with .NET's whenever the two moments share a UTC
/// offset — always for the `<=`, since a shared offset shifts both sides
/// equally, and for the *date* equality except when that shift moves one of
/// them across midnight and not the other.
#[cfg(feature = "time")]
fn date_index(
    info: &FormattingInfo<'_>,
    value: &jiff::civil::DateTime,
    param_count: usize,
) -> usize {
    let now = info.settings().now_or_system_clock();
    if param_count == 3 && value.date() == now.date() {
        1
    } else if *value <= now {
        0
    } else {
        param_count - 1
    }
}

/// .NET's `case TimeSpan` arms: `negative|zero|positive` when there are exactly
/// three parameters and the span is zero, `negative-or-zero|positive`
/// otherwise. No clock is involved.
#[cfg(feature = "time")]
fn time_span_index(value: jiff::SignedDuration, param_count: usize) -> usize {
    if param_count == 3 && value.is_zero() {
        1
    } else if value <= jiff::SignedDuration::ZERO {
        0
    } else {
        param_count - 1
    }
}

impl Default for ConditionalFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for ConditionalFormatter {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // See if the format string contains un-nested splits.
        let parameters = match info.format() {
            // .NET splits before it counts, so a split that throws is the
            // answer for every value, however many parts the formatter wanted.
            Some(format) => split_format(info, format, self.split_char)?,
            None => Vec::new(),
        };

        // Check whether the arguments can be handled by this formatter.
        if parameters.len() < 2 {
            return info.decline_or_error(|name| {
                format!("Formatter named '{name}' requires at least 2 format parameters.")
            });
        }

        let current = info.current();
        let param_count = parameters.len();

        // See if the value is a number (.NET: an `IConvertible` that is not a
        // `DateTime`, a `string` or a `bool`). .NET converts before it looks at
        // the conditions, so a value no `decimal` can hold fails even when no
        // condition would have compared it.
        let current_number = match current {
            Value::Int(value) => Some(Decimal::from_i128(i128::from(*value))),
            Value::UInt(value) => Some(Decimal::from_i128(i128::from(*value))),
            Value::Float(value) => match Decimal::from_f64(*value) {
                Some(number) => Some(number),
                None => return Err(info.plain_error_here(DECIMAL_OVERFLOW)),
            },
            _ => None,
        };

        // First, we'll see if we are using "complex conditions".
        if let Some(number) = current_number {
            let mut index = 0;
            loop {
                // .NET's `while (paramIndex++ < parameters.Count)` runs the
                // body one more time than there are parameters, so a list of
                // complex conditions that all turn out false indexes past the
                // end and throws.
                if index >= param_count {
                    return Err(info.plain_error_here(INDEX_OUT_OF_RANGE));
                }

                match try_evaluate_condition(split_part(info, &parameters[index])?, &number)
                    .map_err(|issue| info.plain_error_here(&issue))?
                {
                    Some((condition_was_true, output)) => {
                        // If the conditional statement was true, then we can
                        // break.
                        if condition_was_true {
                            info.format_as_child(&output, current)?;
                            return Ok(true);
                        }
                    }
                    // This parameter doesn't have a complex condition (making
                    // it an "else" condition).
                    None => {
                        // Only do "complex conditions" if the first item IS a
                        // "complex condition".
                        if index == 0 {
                            break;
                        }
                        // Otherwise, output the "else" section.
                        info.format_as_child(split_part(info, &parameters[index])?, current)?;
                        return Ok(true);
                    }
                }

                index += 1;
            }

            // We don't have any "complex conditions", so let's do the normal
            // conditional formatting.
        }

        // Determine the current item's type.
        let param_index = match current_number {
            Some(number) => {
                if number.is_negative() {
                    param_count - 1
                } else {
                    // .NET casts the floor to `int` before it clamps, so a
                    // value above `int.MaxValue` throws rather than picking
                    // the last parameter.
                    let floor = i32::try_from(number.floor())
                        .map_err(|_| info.plain_error_here(INT32_OVERFLOW))?;
                    (floor as usize).min(param_count - 1)
                }
            }
            None => match current {
                // Bool: True|False
                Value::Bool(value) => usize::from(!*value),
                // Date: Past|Present|Future   or   Past/Present|Future
                #[cfg(feature = "time")]
                Value::DateTime(value) => date_index(info, value, param_count),
                // TimeSpan: Negative|Zero|Positive  or  Negative/Zero|Positive
                #[cfg(feature = "time")]
                Value::TimeSpan(value) => time_span_index(*value, param_count),
                // String: Value|NullOrEmpty
                Value::String(value) => usize::from(value.is_empty()),
                // Object: Something|Nothing
                Value::Null => 1,
                _ => 0,
            },
        };

        // Now, output the selected parameter.
        info.format_as_child(split_part(info, &parameters[param_index])?, current)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Complex conditions
// ---------------------------------------------------------------------------

/// .NET `TryEvaluateCondition`: whether `parameter` starts with a complex
/// condition, and if it does, whether the condition holds — along with the
/// part of the parameter that follows the `?`.
///
/// Each condition starts with a comparer — `>` `>=` `<` `<=` `=` `==` `!`
/// `!=` — and conditions are joined by `&` (AND) or `/` (OR), folded left to
/// right without precedence. The list ends with a `?`.
///
/// .NET matches the condition with the regular expression
/// `^(?:([&/]?)([<>=!]=?)([0-9.-]+))+\?` against the *source text* of the
/// parameter, so an escape sequence is not resolved first and a nested
/// placeholder is matched as it is written. This is a hand-written equivalent:
/// the character classes of the three groups are disjoint and none of them
/// matches a `?`, so scanning greedily from left to right accepts exactly what
/// the regular expression accepts, backtracking included.
fn try_evaluate_condition(
    parameter: &Format,
    value: &Decimal,
) -> Result<Option<(bool, Format)>, String> {
    let Some(conditions) = match_conditions(&parameter.raw) else {
        // Could not parse the "complex condition".
        return Ok(None);
    };

    // Let's evaluate the conditions into a boolean value.
    let mut result = false;
    for (index, condition) in conditions.parts.iter().enumerate() {
        let compared = parse_decimal(condition.value)?;
        let ordering = value.cmp_value(&compared);
        let holds = match condition.comparer {
            ">" => ordering == Ordering::Greater,
            "<" => ordering == Ordering::Less,
            "=" | "==" => ordering == Ordering::Equal,
            "<=" => ordering != Ordering::Greater,
            ">=" => ordering != Ordering::Less,
            "!" | "!=" => ordering != Ordering::Equal,
            // Unreachable: `match_conditions` accepts no other comparer.
            _ => false,
        };

        if index == 0 {
            result = holds;
        } else if condition.and_or == "/" {
            result |= holds;
        } else {
            result &= holds;
        }
    }

    // Output the substring that doesn't contain the "complex condition".
    // In range by construction — `conditions.length` is the length of a match
    // in `parameter.raw`, which is this parameter's own text — but .NET would
    // throw the same `ArgumentOutOfRangeException` here as anywhere else, and
    // this formatter reports every one of those without an envelope, so the
    // message travels the same road as the rest.
    let output = parameter
        .substring(parameter.start + conditions.length, parameter.end)
        .map_err(|error| error.to_string())?;
    Ok(Some((result, output)))
}

/// One `[&/]?[<>=!]=?[0-9.-]+` of a complex condition.
struct Condition<'a> {
    and_or: &'a str,
    comparer: &'a str,
    value: &'a str,
}

/// The conditions a complex condition is made of, and the length in bytes of
/// the whole condition, the trailing `?` included.
struct Conditions<'a> {
    parts: Vec<Condition<'a>>,
    length: usize,
}

/// Matches .NET's `_complexConditionPattern` at the start of `text`.
fn match_conditions(text: &str) -> Option<Conditions<'_>> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut parts = Vec::new();

    loop {
        // `([&/]?)`
        let and_or_start = at;
        if matches!(bytes.get(at), Some(b'&' | b'/')) {
            at += 1;
        }

        // `([<>=!]=?)`
        let comparer_start = at;
        match bytes.get(at) {
            Some(b'<' | b'>' | b'=' | b'!') => at += 1,
            // The group has to match at least once, and once it has, the only
            // way on is a `?` — which is not a comparer, so the match fails
            // here just as the regular expression's would.
            _ => return None,
        }
        if bytes.get(at) == Some(&b'=') {
            at += 1;
        }
        let comparer_end = at;

        // `([0-9.-]+)`
        let value_start = at;
        while matches!(bytes.get(at), Some(b'0'..=b'9' | b'.' | b'-')) {
            at += 1;
        }
        if at == value_start {
            return None;
        }

        parts.push(Condition {
            and_or: &text[and_or_start..comparer_start],
            comparer: &text[comparer_start..comparer_end],
            value: &text[value_start..at],
        });

        // `\?` ends the condition; anything else starts another one.
        if bytes.get(at) == Some(&b'?') {
            return Some(Conditions {
                parts,
                length: at + 1,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Decimal
// ---------------------------------------------------------------------------

/// A .NET `decimal`: a 96-bit mantissa scaled by a power of ten between
/// `10^0` and `10^28`, with a separate sign.
///
/// .NET converts the value with `Convert.ToDecimal` and each condition value
/// with `decimal.Parse`, so this is the type every comparison happens in — and
/// its rounding is observable: `Convert.ToDecimal(0.1 + 0.2)` is `0.3`, which
/// makes `{0:cond:=0.3?yes|no}` render `yes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decimal {
    negative: bool,
    mantissa: u128,
    scale: u32,
}

/// .NET `DecCalc.DEC_SCALE_MAX`.
const DEC_SCALE_MAX: i32 = 28;

/// The largest mantissa a .NET `decimal` can hold, `2^96 - 1`.
const MAX_MANTISSA: u128 = (1u128 << 96) - 1;

impl Decimal {
    const ZERO: Self = Self {
        negative: false,
        mantissa: 0,
        scale: 0,
    };

    /// Every integer this crate can hold fits a `decimal` exactly.
    fn from_i128(value: i128) -> Self {
        Self {
            negative: value < 0,
            mantissa: value.unsigned_abs(),
            scale: 0,
        }
    }

    /// .NET `DecCalc.VarDecFromR8`, which rounds the double to the 15
    /// significant digits an R8 carries, and returns `None` where .NET throws
    /// `OverflowException` — a value too large for a `decimal`, an infinity,
    /// or a NaN.
    ///
    /// The rounding is the one .NET does, not the one a correctly rounded
    /// conversion would: scaling the double by a power of ten rounds a second
    /// time, so the result can land one unit off the 15th digit of the exact
    /// decimal expansion. That is only ever visible to a condition written
    /// with 15 significant digits.
    fn from_f64(input: f64) -> Option<Self> {
        /// .NET `DecCalc.DBLBIAS`.
        const DBL_BIAS: i32 = 1022;
        /// .NET `DecCalc.DoublePowers10`, as far as this conversion reaches.
        const POWERS10: [f64; 29] = [
            1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
            1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28,
        ];

        // The most we can scale by is 10^28, which is just slightly more than
        // 2^93. So a double with an exponent of -94 could just barely reach
        // 0.5, but smaller exponents always round to zero.
        let exp = (((input.to_bits() >> 52) & 0x7ff) as i32) - DBL_BIAS;
        if exp < -94 {
            return Some(Self::ZERO); // Zero, and everything that rounds to it.
        }
        if exp > 96 {
            return None; // Too large for a decimal, an infinity, or a NaN.
        }

        let negative = input < 0.0;
        let mut dbl = if negative { -input } else { input };

        // Round the input to a 15-digit integer. Calculate the largest power
        // of ten the value could have by multiplying the exponent by log10(2),
        // as a scaled integer: log10(2) * 2^16 = .30103 * 65536 = 19728.3.
        let mut power = 14 - ((exp * 19728) >> 16);
        if power >= 0 {
            // We have less than 15 digits, scale the input up.
            if power > DEC_SCALE_MAX {
                power = DEC_SCALE_MAX;
            }
            dbl *= POWERS10[power as usize];
        } else if power != -1 || dbl >= 1e15 {
            dbl /= POWERS10[power.unsigned_abs() as usize];
        } else {
            power = 0; // Didn't scale it.
        }

        if dbl < 1e14 && power < DEC_SCALE_MAX {
            dbl *= 10.0;
            power += 1;
        }

        // Round to an integer, half to even. .NET uses `Math.Round` where
        // SSE4.1 is available and this arithmetic where it is not; the two
        // round alike.
        let truncated = dbl as u64;
        let fraction = dbl - truncated as f64;
        let mantissa = if fraction > 0.5 || (fraction == 0.5 && truncated & 1 != 0) {
            truncated + 1
        } else {
            truncated
        };

        if mantissa == 0 {
            return Some(Self::ZERO);
        }

        // .NET now factors powers of ten out of the mantissa to shrink the
        // scale, which changes the representation only: the value, and so
        // every comparison, is the same either way.
        Some(if power < 0 {
            Self {
                negative,
                mantissa: u128::from(mantissa) * 10u128.pow(power.unsigned_abs()),
                scale: 0,
            }
        } else {
            Self {
                negative,
                mantissa: u128::from(mantissa),
                scale: power as u32,
            }
        })
    }

    /// A decimal zero is neither negative nor positive, whatever its sign says.
    fn is_negative(&self) -> bool {
        self.negative && self.mantissa != 0
    }

    /// `Math.Floor` of the value, rounding towards negative infinity.
    fn floor(&self) -> i128 {
        let divisor = 10u128.pow(self.scale);
        let quotient = (self.mantissa / divisor) as i128;
        match (self.negative, self.mantissa % divisor) {
            (false, _) => quotient,
            (true, 0) => -quotient,
            (true, _) => -quotient - 1,
        }
    }

    fn cmp_value(&self, other: &Self) -> Ordering {
        match (self.is_negative(), other.is_negative()) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => cmp_magnitude(self, other),
            (true, true) => cmp_magnitude(other, self),
        }
    }
}

/// Compares two decimals by magnitude, ignoring their signs.
fn cmp_magnitude(left: &Decimal, right: &Decimal) -> Ordering {
    if left.scale < right.scale {
        return cmp_magnitude(right, left).reverse();
    }
    // Bringing the right side up to the left side's scale can overflow, but
    // only for a value far larger than a 96-bit mantissa holds — so an
    // overflow means the right side is the larger one.
    match right
        .mantissa
        .checked_mul(10u128.pow(left.scale - right.scale))
    {
        Some(scaled) => left.mantissa.cmp(&scaled),
        None => Ordering::Less,
    }
}

/// .NET `decimal.Parse(value, CultureInfo.InvariantCulture)`, restricted to
/// what the condition pattern can produce: digits, at most one decimal point,
/// and one leading or trailing sign (`NumberStyles.Number` allows either, but
/// not both). The error is the message of the exception .NET throws.
///
/// Digits past the 28 a `decimal` holds are rounded away, half to even, and
/// the scale is reduced until the mantissa fits — which is what .NET's
/// `Number.TryNumberToDecimal` does as it fills the mantissa digit by digit
/// and rounds once it is full.
fn parse_decimal(text: &str) -> Result<Decimal, String> {
    let invalid = || not_in_a_correct_format(text);

    let mut rest = text;
    let mut negative = false;
    let mut signed = false;
    if let Some(unsigned) = rest.strip_prefix('-') {
        (negative, signed, rest) = (true, true, unsigned);
    } else if let Some(unsigned) = rest.strip_prefix('+') {
        (signed, rest) = (true, unsigned);
    }
    // `NumberStyles.Number` allows a trailing sign as well, so "5.-" parses as
    // -5 — but one sign in all.
    if let Some(unsigned) = rest.strip_suffix('-') {
        if signed {
            return Err(invalid());
        }
        (negative, rest) = (true, unsigned);
    } else if let Some(unsigned) = rest.strip_suffix('+') {
        if signed {
            return Err(invalid());
        }
        rest = unsigned;
    }

    let (integer, fraction) = match rest.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (rest, ""),
    };
    let digits: Vec<u8> = integer.bytes().chain(fraction.bytes()).collect();
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(invalid());
    }

    // The largest scale whose mantissa still fits 96 bits wins, as in .NET,
    // which stops taking digits once the mantissa is full.
    let max_scale = fraction.len().min(DEC_SCALE_MAX as usize);
    for scale in (0..=max_scale).rev() {
        if let Some(mantissa) = round_digits(&digits, fraction.len() - scale) {
            if mantissa <= MAX_MANTISSA {
                return Ok(Decimal {
                    negative,
                    mantissa,
                    scale: scale as u32,
                });
            }
        }
    }
    Err(DECIMAL_OVERFLOW.to_owned())
}

/// The digits as one integer, with the last `dropped` of them rounded away,
/// half to even. `None` when the result is too large for a `u128`, and so far
/// too large for a mantissa.
fn round_digits(digits: &[u8], dropped: usize) -> Option<u128> {
    let (kept, tail) = digits.split_at(digits.len() - dropped);

    let mut mantissa: u128 = 0;
    for digit in kept {
        mantissa = mantissa
            .checked_mul(10)?
            .checked_add(u128::from(digit - b'0'))?;
    }

    let round_up = match tail.first() {
        None | Some(b'0'..=b'4') => false,
        Some(b'5') => tail[1..].iter().any(|digit| *digit != b'0') || mantissa % 2 == 1,
        Some(_) => true,
    };
    if round_up {
        mantissa = mantissa.checked_add(1)?;
    }
    Some(mantissa)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/ConditionalFormatterTests.cs`, plus
    //! the cases probed against the pinned SmartFormat.NET 3.6.1 package.
    //!
    //! The enum cases have no counterpart, since `Value` has no enum variant,
    //! and neither have the `DateTimeOffset` ones, since a
    //! `jiff::civil::DateTime` carries no offset. The parallel case is not
    //! portable either: a `Formatter` is `Send + Sync`
    //! and holds no mutable state, which the compiler checks.
    //!
    //! An error is asserted by its message and position. Every error this
    //! formatter raises is a plain .NET exception rather than a
    //! `FormattingException`, so the message is the bare issue with no
    //! `Error parsing format string: … at {index}` envelope; see
    //! `formatting_error`.

    use std::collections::BTreeMap;

    use super::*;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    fn smart_with(formatter: ConditionalFormatter) -> SmartFormatter {
        // The .NET default of `FormatErrorAction.ThrowError` — our
        // `ErrorAction::Error` — so a formatting error surfaces in the tests.
        smart_with_settings(
            formatter,
            SmartSettings {
                format_error_action: ErrorAction::Error,
                ..SmartSettings::default()
            },
        )
    }

    fn smart_with_settings(
        formatter: ConditionalFormatter,
        settings: SmartSettings,
    ) -> SmartFormatter {
        let mut smart = SmartFormatter::new(settings);
        // Only the formatter under test and the always-last DefaultFormatter,
        // so these tests pin this formatter rather than the order of the
        // default registry — which `tests/extensions.rs` pins instead.
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    fn smart() -> SmartFormatter {
        smart_with(ConditionalFormatter::new())
    }

    fn args(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    /// Renders `template` against one argument.
    fn format(template: &str, value: Value) -> String {
        format_all(template, args([value]))
    }

    fn format_all(template: &str, args: Value) -> String {
        smart()
            .format(template, &args)
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// The issue text and position of the error `template` raises.
    fn error_of(template: &str, value: Value) -> (String, usize) {
        match smart().format(template, &args([value])) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = ConditionalFormatter::new();
        assert_eq!(formatter.name(), "cond");
        assert!(formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), '|');
        assert_eq!(ConditionalFormatter::new().with_name("c").name(), "c");
    }

    /// .NET 3.6.1 keeps an obsolete `Names` array holding "conditional", but
    /// only `Name` selects a formatter.
    #[test]
    fn is_not_named_conditional() {
        let error = smart().format("{0:conditional:a|b}", &args([Value::Int(1)]));
        assert!(
            matches!(&error, Err(Error::Format { message, .. })
                if message.contains("No suitable Formatter could be found")),
            "{error:?}"
        );
    }

    #[test]
    fn test_numbers() {
        let numbers = || {
            [
                Value::Int(0),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(-1),
                Value::Int(-2),
            ]
        };
        let template = |parts: &str| {
            (0..6)
                .map(|index| format!("{{{index}:cond:{parts}}}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        assert_eq!(
            format_all(&template("Zero|Other"), args(numbers())),
            "Zero Other Other Other Other Other"
        );
        assert_eq!(
            format_all(&template("Zero|One|Other"), args(numbers())),
            "Zero One Other Other Other Other"
        );
        assert_eq!(
            format_all(&template("Zero|One|Two|Other"), args(numbers())),
            "Zero One Two Other Other Other"
        );
    }

    /// A fractional value indexes by its floor, and a negative one always
    /// takes the last part.
    #[test]
    fn test_fractions() {
        assert_eq!(format("{0:cond:Zero|One|Other}", Value::Float(0.5)), "Zero");
        assert_eq!(format("{0:cond:Zero|One|Other}", Value::Float(1.5)), "One");
        assert_eq!(
            format("{0:cond:Zero|One|Other}", Value::Float(2.9)),
            "Other"
        );
        assert_eq!(
            format("{0:cond:Zero|One|Other}", Value::Float(-0.5)),
            "Other"
        );
        assert_eq!(
            format("{0:cond:Zero|One|Other}", Value::Float(-0.0)),
            "Zero"
        );
        // Convert.ToDecimal keeps 15 significant digits, so this rounds up to
        // 1 before it is floored.
        assert_eq!(
            format(
                "{0:cond:Zero|One|Other}",
                Value::Float(0.999_999_999_999_999_9)
            ),
            "One"
        );
    }

    #[test]
    fn test_bool() {
        assert_eq!(format("{0:cond:Yes|No}", Value::Bool(false)), "No");
        assert_eq!(format("{0:cond:Yes|No}", Value::Bool(true)), "Yes");
        // A third part is never reached by a bool.
        assert_eq!(format("{0:cond:a|b|c}", Value::Bool(true)), "a");
        assert_eq!(format("{0:cond:a|b|c}", Value::Bool(false)), "b");
    }

    #[test]
    fn test_with_changed_split_char() {
        let mut formatter = ConditionalFormatter::new();
        // Set SplitChar from | to ~, so we can use | for the output string.
        formatter.set_split_char('~').unwrap();
        let smart = smart_with(formatter);

        let template = "{0:cond:|Yes|~|No|}";
        let rendered = |value: i64| {
            smart
                .format(template, &args([Value::Int(value)]))
                .expect("formatting failed")
        };
        assert_eq!(rendered(0), "|Yes|");
        assert_eq!(rendered(1), "|No|");

        assert_eq!(
            ConditionalFormatter::new().set_split_char('!'),
            Err(InvalidSplitChar('!'))
        );
        assert_eq!(
            InvalidSplitChar('!').to_string(),
            "Only '|', ',' and '~' are valid split chars."
        );
    }

    #[test]
    fn explicit_formatter_with_not_enough_parameters_is_an_error() {
        assert_eq!(
            error_of("{0:cond:Yes}", Value::Int(1)),
            (
                "Formatter named 'cond' requires at least 2 format parameters.".to_owned(),
                8
            )
        );
        assert_eq!(
            error_of("{0:cond:}", Value::Int(1)).0,
            "Formatter named 'cond' requires at least 2 format parameters."
        );
    }

    #[test]
    fn test_strings() {
        assert_eq!(format("{0:cond:{}|Empty}", Value::from("Hello")), "Hello");
        assert_eq!(format("{0:cond:{}|Empty}", Value::from("")), "Empty");
        assert_eq!(format("{0:cond:{}|Null}", Value::Null), "Null");
        // The parts past the second are never reached by a string.
        assert_eq!(format("{0:cond:a|b|c}", Value::from("x")), "a");
        assert_eq!(format("{0:cond:a|b|c}", Value::from("")), "b");
        assert_eq!(format("{0:cond:a|b|c}", Value::Null), "b");
    }

    /// .NET's "Object: Something|Nothing" branch, which every value that is
    /// not a number, a bool, a string or a date takes.
    #[test]
    fn test_object() {
        let map = Value::Map(BTreeMap::from([("NotNull".to_owned(), Value::Bool(true))]));
        assert_eq!(format("{0:cond:Something|Null}", map), "Something");
        assert_eq!(
            format("{0:cond:Something|Null}", Value::List(vec![Value::Int(1)])),
            "Something"
        );
        assert_eq!(format("{0:cond:Something|Null}", Value::Null), "Null");
    }

    #[test]
    fn test_complex_condition() {
        const TEMPLATE: &str = "{0:cond:<1?Baby|>=1&<4?Toddler|>=4&<=9?Child|=10/=11/=12?Pre-Teen|<18?Teenager|<20?Young Adult|<20/<=24&<25?Early Twenties|>55&<100?Senior Citizen|>100?Crazy Old|Adult}";
        let cases = [
            (Value::Int(-5), "Baby"),
            (Value::Int(0), "Baby"),
            (Value::Float(0.5), "Baby"),
            (Value::Float(1.0), "Toddler"),
            (Value::Float(1.5), "Toddler"),
            (Value::Float(5.0), "Child"),
            (Value::Float(11.0), "Pre-Teen"),
            (Value::Float(14.0), "Teenager"),
            (Value::Int(18), "Young Adult"),
            (Value::Int(22), "Early Twenties"),
            (Value::Int(45), "Adult"),
            (Value::Int(60), "Senior Citizen"),
            (Value::Int(101), "Crazy Old"),
        ];
        for (value, expected) in cases {
            assert_eq!(format(TEMPLATE, value.clone()), expected, "{value:?}");
        }
    }

    #[test]
    fn test_positive_negative_zero() {
        const TEMPLATE: &str = "{0:cond:>0?Positive|<0?Negative|=0?Zero}";
        assert_eq!(format(TEMPLATE, Value::Int(0)), "Zero");
        assert_eq!(format(TEMPLATE, Value::Int(1)), "Positive");
        assert_eq!(format(TEMPLATE, Value::Float(2.0)), "Positive");
        assert_eq!(format(TEMPLATE, Value::Int(-5)), "Negative");
    }

    /// Every comparer of the condition pattern, and both connectors.
    #[test]
    fn comparers_and_connectors() {
        assert_eq!(format("{0:cond:>5?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:>=6?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:<6?a|b}", Value::Int(6)), "b");
        assert_eq!(format("{0:cond:<=6?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:=6?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:==6?a|b}", Value::Int(6)), "a");
        // A lone "!" is "not equal", as in .NET's switch.
        assert_eq!(format("{0:cond:!5?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:!=5?a|b}", Value::Int(5)), "b");
        // The conditions fold left to right: "&" binds no tighter than "/".
        assert_eq!(format("{0:cond:>=1&<4?a|b}", Value::Int(2)), "a");
        assert_eq!(format("{0:cond:=10/=11/=12?a|b}", Value::Int(11)), "a");
        assert_eq!(format("{0:cond:<20/<=24&<25?a|b}", Value::Int(22)), "a");
        // A missing connector is an AND.
        assert_eq!(format("{0:cond:>1<5?a|b}", Value::Int(2)), "a");
        assert_eq!(format("{0:cond:>1<5?a|b}", Value::Int(6)), "b");
        // Only the first "?" ends the condition.
        assert_eq!(format("{0:cond:>0?a?b|c}", Value::Int(1)), "a?b");
    }

    /// Text that only looks like a condition is not one, and the value then
    /// picks a part by its number.
    #[test]
    fn text_that_is_not_a_condition() {
        // A space, a "+" and a missing value all keep the text out of the
        // condition pattern.
        assert_eq!(format("{0:cond:> 5?a|b}", Value::Int(6)), "b");
        assert_eq!(format("{0:cond:>5 ?a|b}", Value::Int(6)), "b");
        assert_eq!(format("{0:cond:>+5?a|b}", Value::Int(6)), "b");
        assert_eq!(format("{0:cond:>?a|b}", Value::Int(6)), "b");
        // A condition needs a comparer, so a bare number is just text.
        assert_eq!(format("{0:cond:-5?a|b}", Value::Int(-5)), "b");
        // Only the first part decides whether the parts are conditions at all,
        // so this renders the second part verbatim.
        assert_eq!(format("{0:cond:else|>10?big}", Value::Int(15)), ">10?big");
        assert_eq!(format("{0:cond:|>10?a|b}", Value::Int(15)), "b");
    }

    /// A part without a condition, once the first part had one, is the "else"
    /// branch.
    #[test]
    fn else_branch() {
        assert_eq!(format("{0:cond:>10?big|else}", Value::Int(0)), "else");
        assert_eq!(
            format("{0:cond:>10?a|else|>20?b}", Value::Int(5)),
            "else",
            "the else branch wins before a later condition is looked at"
        );
        assert_eq!(format("{0:cond:>10?a|else|>20?b}", Value::Int(15)), "a");
    }

    /// .NET walks one index past the last parameter, so a list of conditions
    /// that all turn out false throws `ArgumentOutOfRangeException`.
    #[test]
    fn every_condition_false_is_an_error() {
        assert_eq!(
            error_of("{0:cond:>10?big|>20?huge}", Value::Int(0)),
            (INDEX_OUT_OF_RANGE.to_owned(), 8)
        );
    }

    /// A condition is only looked at for a value that converts to a number.
    #[test]
    fn conditions_need_a_number() {
        assert_eq!(format("{0:cond:>10?a|b|c}", Value::from("text")), ">10?a");
        assert_eq!(format("{0:cond:>10?a|b}", Value::Bool(true)), ">10?a");
    }

    #[test]
    fn condition_values_are_parsed_as_dotnet_decimals() {
        // ".5" and "5." are both valid, and a trailing sign negates.
        assert_eq!(format("{0:cond:>.5?a|b}", Value::Float(0.6)), "a");
        assert_eq!(format("{0:cond:>5.?a|b}", Value::Int(6)), "a");
        assert_eq!(format("{0:cond:>5.-?a|b}", Value::Int(-1)), "a");
        assert_eq!(format("{0:cond:>-5?a|b}", Value::Int(-1)), "a");
        // An integer equals the same value written with a fraction.
        assert_eq!(format("{0:cond:=1.0?a|b}", Value::Int(1)), "a");
        assert_eq!(format("{0:cond:=1?a|b}", Value::Float(1.0)), "a");
        // A condition value is always read with the invariant culture, and the
        // culture a template is rendered with never reaches it.
        assert_eq!(
            format("{0:cond:<=0.25?less|more}", Value::Float(0.3)),
            "more"
        );
        // 28 fractional digits is all a decimal holds; the rest rounds away.
        assert_eq!(
            format(
                "{0:cond:>1.00000000000000000000000000005?a|b}",
                Value::Int(1)
            ),
            "b"
        );
    }

    #[test]
    fn unparsable_condition_values_are_errors() {
        assert_eq!(
            error_of("{0:cond:>5.5.5?a|b}", Value::Int(6)),
            (
                "The input string '5.5.5' was not in a correct format.".to_owned(),
                8
            )
        );
        assert_eq!(
            error_of("{0:cond:>-?a|b}", Value::Int(6)).0,
            "The input string '-' was not in a correct format."
        );
        assert_eq!(
            error_of("{0:cond:>.?a|b}", Value::Int(6)).0,
            "The input string '.' was not in a correct format."
        );
        assert_eq!(
            error_of("{0:cond:>5-5?a|b}", Value::Int(6)).0,
            "The input string '5-5' was not in a correct format."
        );
        assert_eq!(
            error_of(
                "{0:cond:>99999999999999999999999999999999?a|b}",
                Value::Int(6)
            )
            .0,
            DECIMAL_OVERFLOW
        );
    }

    /// Every integer type reaches the same `decimal`.
    #[test]
    fn signed_and_unsigned_numbers() {
        for value in [Value::Int(123), Value::UInt(123), Value::Float(123.0)] {
            assert_eq!(
                format("{0:cond:=123?yes|no}", value.clone()),
                "yes",
                "{value:?}"
            );
        }
    }

    /// .NET converts the value to a `decimal` before it looks at the format,
    /// and casts the floor to `int` before it clamps it to the part count.
    #[test]
    fn values_a_decimal_or_an_int_cannot_hold_are_errors() {
        assert_eq!(
            error_of("{0:cond:a|b}", Value::Int(3_000_000_000)),
            (INT32_OVERFLOW.to_owned(), 8)
        );
        assert_eq!(
            error_of("{0:cond:a|b}", Value::UInt(u64::MAX)).0,
            INT32_OVERFLOW
        );
        assert_eq!(
            error_of("{0:cond:a|b}", Value::Float(1e30)).0,
            DECIMAL_OVERFLOW
        );
        assert_eq!(
            error_of("{0:cond:a|b}", Value::Float(f64::NAN)).0,
            DECIMAL_OVERFLOW
        );
        assert_eq!(
            error_of("{0:cond:a|b}", Value::Float(f64::INFINITY)).0,
            DECIMAL_OVERFLOW
        );
        // The conversion happens before the conditions are evaluated.
        assert_eq!(
            error_of("{0:cond:>1?a|b}", Value::Float(f64::NAN)).0,
            DECIMAL_OVERFLOW
        );
        // A negative value takes the last part without ever being cast.
        assert_eq!(format("{0:cond:a|b}", Value::Int(-3_000_000_000)), "b");
        // A condition returns before the cast, too.
        assert_eq!(format("{0:cond:>1?a|b}", Value::Int(3_000_000_000)), "a");
        assert_eq!(format("{0:cond:a|b}", Value::Int(i64::from(i32::MAX))), "b");
    }

    #[test]
    fn syntax_is_not_confused_with_named_formatters() {
        assert_eq!(format("{0:part(s)|car}", Value::Bool(true)), "part(s)");
        assert_eq!(format("{0:part(s)|car}", Value::Bool(false)), "car");
    }

    /// Auto detection declines a format that holds no split character, which
    /// leaves it to the next formatter — here the default one, which writes a
    /// string and ignores the format, since a .NET string is not
    /// `IFormattable`.
    #[test]
    fn auto_detection_declines_a_single_part() {
        assert_eq!(format("{0:Yes}", Value::from("text")), "text");

        let mut formatter = ConditionalFormatter::new();
        formatter.set_can_auto_detect(false);
        let smart = smart_with(formatter);
        assert_eq!(
            smart
                .format("{0:part(s)|car}", &args([Value::from("text")]))
                .unwrap(),
            "text",
            "with auto detection off even a two-part format is declined"
        );
    }

    /// A nested placeholder is never split, and it is rendered against the
    /// value the condition picked.
    #[test]
    fn nested_placeholders() {
        let args = Value::List(vec![Value::Int(1), Value::from("N")]);
        assert_eq!(format_all("{0:cond:{1}|c}", args.clone()), "c");
        assert_eq!(format_all("{0:cond:>0?[{}]|c}", args), "[1]");
    }

    /// .NET splits on the source text of a literal, so an escaped split
    /// character splits all the same and leaves the backslash behind.
    #[test]
    fn an_escaped_split_character_still_splits() {
        assert_eq!(format(r"{0:cond:a\|b|c}", Value::Int(0)), r"a\");
    }

    #[cfg(feature = "time")]
    mod dates {
        use super::*;
        use jiff::civil::date;
        use jiff::SignedDuration;

        fn now() -> jiff::civil::DateTime {
            date(2026, 7, 31).at(12, 0, 0, 0)
        }

        /// A formatter whose "now" is pinned, standing in for .NET's
        /// `SystemTime.SetDateTime`.
        fn smart_at(now: Option<jiff::civil::DateTime>) -> SmartFormatter {
            smart_with_settings(
                ConditionalFormatter::new(),
                SmartSettings {
                    format_error_action: ErrorAction::Error,
                    now,
                    ..SmartSettings::default()
                },
            )
        }

        fn format_date(template: &str, value: jiff::civil::DateTime) -> String {
            smart_at(Some(now()))
                .format(template, &args([Value::DateTime(value)]))
                .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
        }

        fn format_span(template: &str, value: SignedDuration) -> String {
            smart_at(Some(now()))
                .format(template, &args([Value::TimeSpan(value)]))
                .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
        }

        #[test]
        fn past_present_future() {
            let past = date(1111, 1, 1).at(1, 1, 1, 0);
            let future = date(5555, 5, 5).at(5, 5, 5, 0);
            // Two parts: past — which includes now — or future.
            assert_eq!(format_date("{0:cond:Past|Future}", past), "Past");
            assert_eq!(format_date("{0:cond:Past|Future}", now()), "Past");
            assert_eq!(format_date("{0:cond:Past|Future}", future), "Future");
            // Three parts: the middle one is today, whatever the time of day.
            assert_eq!(format_date("{0:cond:Past|Present|Future}", past), "Past");
            assert_eq!(
                format_date("{0:cond:Past|Present|Future}", now()),
                "Present"
            );
            assert_eq!(
                format_date("{0:cond:Past|Present|Future}", future),
                "Future"
            );
            assert_eq!(
                format_date(
                    "{0:cond:Past|Present|Future}",
                    date(2026, 7, 31).at(0, 0, 0, 0)
                ),
                "Present"
            );
            assert_eq!(
                format_date(
                    "{0:cond:Past|Present|Future}",
                    date(2026, 7, 31).at(23, 59, 0, 0)
                ),
                "Present"
            );
            // Four parts: no "present" branch, so today is past or future.
            assert_eq!(format_date("{0:cond:A|B|C|D}", past), "A");
            assert_eq!(format_date("{0:cond:A|B|C|D}", future), "D");
            assert_eq!(
                format_date("{0:cond:A|B|C|D}", date(2026, 7, 31).at(23, 59, 0, 0)),
                "D"
            );
        }

        /// With no "now" pinned the formatter reads the system clock, as
        /// .NET's `SystemTime.Now()` does by default — so the year 1111 is
        /// past and the year 5555 is future, whenever the test runs.
        #[test]
        fn an_unpinned_now_reads_the_clock() {
            let smart = smart_at(None);
            let past = args([Value::DateTime(date(1111, 1, 1).at(1, 1, 1, 0))]);
            let future = args([Value::DateTime(date(5555, 5, 5).at(5, 5, 5, 0))]);
            let template = "{0:cond:Past|Present|Future}";
            assert_eq!(smart.format(template, &past).unwrap(), "Past");
            assert_eq!(smart.format(template, &future).unwrap(), "Future");
        }

        /// A date is not a number, so it never reaches a condition.
        #[test]
        fn a_date_takes_no_condition() {
            assert_eq!(format_date("{0:cond:>0?a|b}", now()), ">0?a");
        }

        /// .NET's `case TimeSpan` arms, which need no clock: negative, zero
        /// and positive, with zero folded into the negative branch unless
        /// there are exactly three parameters.
        #[test]
        fn negative_zero_future() {
            let negative = SignedDuration::from_hours(-1);
            let positive = SignedDuration::from_hours(1);
            let zero = SignedDuration::ZERO;

            // Two parts: negative-or-zero, or positive.
            assert_eq!(format_span("{0:cond:Neg|Pos}", negative), "Neg");
            assert_eq!(format_span("{0:cond:Neg|Pos}", zero), "Neg");
            assert_eq!(format_span("{0:cond:Neg|Pos}", positive), "Pos");
            // Three parts: the middle one is exactly zero.
            assert_eq!(format_span("{0:cond:Neg|Zero|Pos}", negative), "Neg");
            assert_eq!(format_span("{0:cond:Neg|Zero|Pos}", zero), "Zero");
            assert_eq!(format_span("{0:cond:Neg|Zero|Pos}", positive), "Pos");
            // A nanosecond is not zero.
            assert_eq!(
                format_span("{0:cond:Neg|Zero|Pos}", SignedDuration::from_nanos(1)),
                "Pos"
            );
            // Four parts: no "zero" branch, so zero is negative.
            assert_eq!(format_span("{0:cond:A|B|C|D}", zero), "A");
            assert_eq!(format_span("{0:cond:A|B|C|D}", positive), "D");
        }

        /// A `TimeSpan` is not `IConvertible` in .NET, so — like a date — it
        /// never reaches a complex condition.
        #[test]
        fn a_time_span_takes_no_condition() {
            // A negative span picks the first part, which is written out with
            // its `>0?` prefix intact instead of being read as a condition.
            assert_eq!(
                format_span("{0:cond:>0?a|b}", SignedDuration::from_hours(-1)),
                ">0?a"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The decimal conversions the conditions compare in
    // -----------------------------------------------------------------------

    fn decimal_of(text: &str) -> Decimal {
        parse_decimal(text).unwrap_or_else(|error| panic!("{text:?} failed: {error}"))
    }

    fn equals(left: &str, right: &str) -> bool {
        decimal_of(left).cmp_value(&decimal_of(right)) == Ordering::Equal
    }

    #[test]
    fn decimal_parsing() {
        assert_eq!(
            decimal_of("5").cmp_value(&Decimal::from_i128(5)),
            Ordering::Equal
        );
        assert!(equals("5.", "5"));
        assert!(equals("5.-", "-5"));
        assert!(equals("-0", "0"));
        assert!(equals("00000000000000000000000000000005", "5"));
        assert!(equals(".5", "0.50"));
        assert!(equals("0.3", "0.30000"));
        assert_eq!(
            decimal_of("-1").cmp_value(&decimal_of("-2")),
            Ordering::Greater
        );
        assert_eq!(decimal_of("-1").cmp_value(&decimal_of("1")), Ordering::Less);
        // The largest decimal there is, and the first one too large.
        assert!(parse_decimal("79228162514264337593543950335").is_ok());
        assert_eq!(
            parse_decimal("79228162514264337593543950336"),
            Err(DECIMAL_OVERFLOW.to_owned())
        );
        // Digits past the 28th round half to even.
        assert!(equals(
            "1.00000000000000000000000000005",
            "1.0000000000000000000000000000"
        ));
        assert!(equals(
            "1.00000000000000000000000000015",
            "1.0000000000000000000000000002"
        ));
        assert!(equals(
            "1.00000000000000000000000000006",
            "1.0000000000000000000000000001"
        ));

        for invalid in ["", "-", "--", ".", "-.", "1.2.3", "-5-", "5-5"] {
            assert!(
                parse_decimal(invalid)
                    .is_err_and(|error| error.contains("was not in a correct format")),
                "{invalid:?} should not parse"
            );
        }
    }

    /// `Convert.ToDecimal(double)` keeps 15 significant digits, so a double
    /// that is nearly a value compares equal to it.
    #[test]
    fn double_conversion() {
        let of = |value: f64| Decimal::from_f64(value).expect("overflow");
        let equals = |value: f64, text: &str| of(value).cmp_value(&decimal_of(text));

        assert_eq!(equals(0.1 + 0.2, "0.3"), Ordering::Equal);
        assert_eq!(equals(0.1, "0.1"), Ordering::Equal);
        assert_eq!(equals(2.675, "2.675"), Ordering::Equal);
        assert_eq!(equals(1.0 / 3.0, "0.333333333333333"), Ordering::Equal);
        assert_eq!(
            equals(123_456_789_012_345_678.0, "123456789012346000"),
            Ordering::Equal
        );
        assert_eq!(
            equals(1e28, "10000000000000000000000000000"),
            Ordering::Equal
        );
        // Everything below 1e-28 rounds to zero, and 1.5e-28 rounds up.
        assert_eq!(equals(1e-29, "0"), Ordering::Equal);
        assert_eq!(
            equals(1.5e-28, "0.0000000000000000000000000002"),
            Ordering::Equal
        );
        assert_eq!(equals(-0.0, "0"), Ordering::Equal);
        assert!(!of(-0.0).is_negative());
        // Too large, and the two values that are not numbers at all.
        assert_eq!(Decimal::from_f64(1e29), None);
        assert_eq!(Decimal::from_f64(7.922_816_251_426_434e28), None);
        assert_eq!(Decimal::from_f64(f64::NAN), None);
        assert_eq!(Decimal::from_f64(f64::NEG_INFINITY), None);
    }

    #[test]
    fn decimal_floor() {
        assert_eq!(decimal_of("2.9").floor(), 2);
        assert_eq!(decimal_of("2").floor(), 2);
        assert_eq!(decimal_of("-2.9").floor(), -3);
        assert_eq!(decimal_of("-2.0").floor(), -2);
        assert_eq!(decimal_of("0.9").floor(), 0);
    }
}
