//! Port of SmartFormat.NET's `SubStringFormatter`.
//!
//! Ported from `src/SmartFormat/Extensions/SubStringFormatter.cs`.
//!
//! `{0:substr(start,length)}` writes part of a string. Both numbers may be
//! negative, which counts from the end, and `length` may be left out, which
//! takes everything from `start` on:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! let smart = SmartFormatter::default();
//!
//! let args = Value::List(vec![Value::from("Long John")]);
//! assert_eq!(smart.format("{0:substr(5)}", &args).unwrap(), "John");
//! assert_eq!(smart.format("{0:substr(-4,2)}", &args).unwrap(), "Jo");
//! ```
//!
//! The formatter only handles a string or a null value, it never auto-detects,
//! and — unlike the `|`-splitting formatters — it splits its options on a comma.

use crate::dotnet_messages::{not_in_a_correct_format, INT32_OVERFLOW, OUT_OF_RANGE};
use crate::fmt::utf16_len;
use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::is_dotnet_white;
use crate::value::Value;
use crate::Error;

use super::{InvalidSplitChar, VALID_SPLIT_CHARS};

/// The default formatter name, .NET `SubStringFormatter.Name`.
const NAME: &str = "substr";

/// The .NET default split character of this formatter, which is the comma
/// rather than the pipe the other option-splitting formatters use.
const DEFAULT_SPLIT_CHAR: char = VALID_SPLIT_CHARS[1];

// `INT32_OVERFLOW` is the `OverflowException` from `int.Parse`, and
// `OUT_OF_RANGE` the `ArgumentOutOfRangeException` from
// `ReadOnlySpan<char>.Slice`, which passes no parameter name. Both are plain
// exceptions the evaluator catches, so each reaches the output bare; see
// [`FormattingInfo::plain_error`].
/// The `FormattingException` .NET raises for a format that is plain text, which
/// — being a `FormattingException` — does carry the message envelope.
const NEEDS_NESTED: &str = "The format requires a nested placeholder";

/// What .NET does when the start index and/or the length reach past the end of
/// the string (`SubStringFormatter.SubStringOutOfRangeBehavior`).
///
/// Only a `start + length` past the end of the string is caught here. A
/// negative start index that stays negative after counting from the end, and a
/// negative length, are out of range under every behavior — .NET's own comment
/// says as much — and are always out of range.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubStringOutOfRangeBehavior {
    /// Returns the empty string. The .NET default.
    #[default]
    ReturnEmptyString,
    /// Returns the remainder of the string, starting at the start index.
    ReturnStartIndexToEndOfString,
    /// Raises the error `ReadOnlySpan<char>.Slice` throws.
    ThrowException,
}

/// Writes part of a string, ported from .NET `SubStringFormatter`.
///
/// A placeholder has to name the formatter unless
/// [`set_can_auto_detect`](Self::set_can_auto_detect) turns auto-detection on
/// (.NET `CanAutoDetect`, which defaults to `false` here as it does there).
#[derive(Debug, Clone)]
pub struct SubStringFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
    null_display_string: String,
    out_of_range_behavior: SubStringOutOfRangeBehavior,
}

impl SubStringFormatter {
    /// A formatter named `substr`, splitting its options on `,`, writing
    /// nothing for a null value and returning the empty string when the range
    /// is too long — the .NET defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: false,
            null_display_string: String::new(),
            out_of_range_behavior: SubStringOutOfRangeBehavior::default(),
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the options are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that `{0:substr(-4|-1)}` reads its two
    /// options as `-4` and `-1`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`]
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether a placeholder that names no formatter may be handled here
    /// (.NET `CanAutoDetect`, which defaults to `false`).
    pub fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    pub fn set_can_auto_detect(&mut self, can_auto_detect: bool) {
        self.can_auto_detect = can_auto_detect;
    }

    /// The text written for a null value, empty by default
    /// (.NET `NullDisplayString`).
    ///
    /// It is *not* used when the placeholder carries a format: the child format
    /// is then rendered against the null value and has to handle it itself.
    pub fn null_display_string(&self) -> &str {
        &self.null_display_string
    }

    pub fn set_null_display_string(&mut self, null_display_string: impl Into<String>) {
        self.null_display_string = null_display_string.into();
    }

    /// What happens when the range reaches past the end of the string
    /// (.NET `OutOfRangeBehavior`).
    pub fn out_of_range_behavior(&self) -> SubStringOutOfRangeBehavior {
        self.out_of_range_behavior
    }

    pub fn set_out_of_range_behavior(&mut self, behavior: SubStringOutOfRangeBehavior) {
        self.out_of_range_behavior = behavior;
    }

    /// .NET `GetSubstring`, over the UTF-16 code units of `text`.
    fn substring(
        &self,
        info: &FormattingInfo<'_>,
        text: &str,
        parameters: &[&str],
    ) -> Result<String, Error> {
        // A .NET string is never longer than `int.MaxValue`, so the cast is
        // what .NET's `Length` already is; a longer one would have to saturate.
        let text_length = i32::try_from(utf16_len(text)).unwrap_or(i32::MAX);
        let (start_pos, mut length) = start_and_length(info, text_length, parameters)?;

        // .NET adds two `int`s unchecked, so a sum that overflows wraps rather
        // than counting as "past the end".
        let past_end = start_pos.wrapping_add(length) > text_length;
        match self.out_of_range_behavior {
            SubStringOutOfRangeBehavior::ReturnEmptyString if past_end => length = 0,
            SubStringOutOfRangeBehavior::ReturnStartIndexToEndOfString if past_end => {
                length = text_length.wrapping_sub(start_pos);
            }
            // SubStringOutOfRangeBehavior::ThrowException, and every range that
            // is not past the end: without prior adjustments, the slice below
            // may throw.
            _ => {}
        }

        let out_of_range = || info.plain_error_here(OUT_OF_RANGE);

        // .NET `ReadOnlySpan<char>.Slice`, whose two overloads differ in the
        // bounds they check: with one parameter everything from the start index
        // on is taken, whatever the length worked out to.
        if start_pos < 0 || start_pos > text_length {
            return Err(out_of_range());
        }
        if parameters.len() == 1 {
            return Ok(utf16_slice(text, start_pos, text_length - start_pos));
        }
        if length < 0 || length > text_length - start_pos {
            return Err(out_of_range());
        }
        Ok(utf16_slice(text, start_pos, length))
    }
}

impl Default for SubStringFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for SubStringFormatter {
    /// Itself, so that a caller who registered it can find it again through
    /// [`FormatterRegistry::get_mut`](crate::formatter::FormatterRegistry::get_mut)
    /// and set its knobs.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // .NET splits the *string* of the options here, not the format, so this
        // is a plain `string.Split` and never a lazy one.
        let options = info.formatter_options()?;
        let parameters: Vec<&str> = options.split(self.split_char).collect();

        // Only a string or a null value can be cut, and one option is the
        // least the formatter can work with.
        let current = info.current();
        let text = match current {
            Value::String(text) => Some(text.as_str()),
            _ => None,
        };
        let is_string_or_null = matches!(current, Value::String(_) | Value::Null);
        if !is_string_or_null || (parameters.len() == 1 && parameters[0].is_empty()) {
            return info.decline_or_error(|name| {
                format!(
                    "Formatter named '{name}' requires at least 1 formatter option and a string? argument."
                )
            });
        }

        // Past the check above, a value that is not a string is a null one.
        // A null never even parses the options: `{0:substr(oops)}` is an error
        // for a string and the null display string for a null.
        let substring = match text {
            Some(text) => self.substring(info, text, &parameters)?,
            None => String::new(),
        };

        // A format was supplied, so use it if valid.
        if let Some(format) = info.format() {
            if format.end() > format.start() {
                if !format.has_nested() {
                    // The one error .NET raises as a `FormattingException`, so
                    // this message does carry the envelope. .NET reports it at
                    // the start of the format.
                    return Err(info.formatting_error(NEEDS_NESTED, format.start()));
                }
                // The child sees the substring — or the null value, which it
                // then has to handle itself.
                let value = match text {
                    Some(_) => Value::String(substring),
                    None => Value::Null,
                };
                info.format_as_child(format, &value)?;
                return Ok(true);
            }
        }

        // Just output the substring directly.
        match text {
            Some(_) => info.write(&substring),
            None => info.write(&self.null_display_string),
        }
        Ok(true)
    }
}

/// .NET `GetStartAndLength`: the two options as numbers, each counted from the
/// end of the string when it is negative.
///
/// Every step is `int` arithmetic that .NET leaves unchecked, so a sum that
/// overflows wraps here too — the range is then out of bounds either way, but
/// which error comes out can differ.
fn start_and_length(
    info: &FormattingInfo<'_>,
    text_length: i32,
    parameters: &[&str],
) -> Result<(i32, i32), Error> {
    let mut start_pos = parse_int(info, parameters[0])?;
    let mut length = if parameters.len() > 1 {
        parse_int(info, parameters[1])?
    } else {
        0
    };

    if start_pos < 0 {
        start_pos = text_length.wrapping_add(start_pos);
    }
    if start_pos > text_length {
        start_pos = text_length;
    }
    if length < 0 {
        length = text_length.wrapping_sub(start_pos).wrapping_add(length);
    }

    Ok((start_pos, length))
}

/// .NET `int.Parse(string)`, which is `NumberStyles.Integer`: whitespace, then
/// an optional sign, then ASCII digits, then whitespace.
///
/// One divergence: .NET reads the sign of the *thread* culture, so a culture
/// whose negative sign is not `-` — Swedish before ICU 62, and a handful of
/// others — parses `substr(−4)` there and not here. The culture a template is
/// rendered with never reaches this call in .NET either.
fn parse_int(info: &FormattingInfo<'_>, text: &str) -> Result<i32, Error> {
    // The message quotes the option as written, whitespace included (probed).
    let invalid = || info.plain_error_here(&not_in_a_correct_format(text));

    let trimmed = text.trim_matches(is_dotnet_white);
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }

    // The magnitude, which is one larger than `i32::MAX` for `-2147483648`.
    let mut value: i64 = 0;
    for byte in digits.bytes() {
        value = value * 10 + i64::from(byte - b'0');
        if value > -i64::from(i32::MIN) {
            return Err(info.plain_error_here(INT32_OVERFLOW));
        }
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).map_err(|_| info.plain_error_here(INT32_OVERFLOW))
}

// ---------------------------------------------------------------------------
// UTF-16 slicing
// ---------------------------------------------------------------------------

/// `length` UTF-16 code units of `text`, starting at unit `start`.
///
/// A .NET substring is cut between code units, so a cut can fall inside a
/// surrogate pair and leave a lone surrogate behind. A Rust `String` cannot
/// hold one, so the orphaned half becomes U+FFFD. While the half *stays*
/// orphaned that is byte-for-byte what .NET writes: `{0:substr(0,1)}` over
/// `"\u{1F600}abc"` encodes to `EF BF BD` there too (probed).
///
/// It stops being identical when the two halves of one pair end up next to
/// each other in the output. .NET keeps them as UTF-16 code units in the
/// result string, so `{0:substr(0,1)}{0:substr(1,1)}` over `"\u{1F600}"`
/// re-forms the pair and writes the emoji, `F0 9F 98 80`; here each half was
/// already replaced, and the result is two U+FFFDs. That is the same
/// recombination the escaped-`\uXXXX` divergence has, from the other side.
/// See DESIGN.md, "Known divergences".
///
/// The caller has checked the bounds, so `start` and `length` are within the
/// string.
fn utf16_slice(text: &str, start: i32, length: i32) -> String {
    let (start, end) = (start as usize, (start + length) as usize);
    // Every ASCII character is one code unit and one byte, so the units are
    // byte offsets and the common case is a plain slice.
    if text.is_ascii() {
        return text[start..end].to_owned();
    }

    let mut result = String::new();
    let mut at = 0;
    for character in text.chars() {
        let (first, last) = (at, at + character.len_utf16());
        at = last;
        if last <= start || first >= end {
            continue;
        }
        if first >= start && last <= end {
            result.push(character);
        } else {
            // Half of a surrogate pair, which UTF-8 cannot spell.
            result.push(char::REPLACEMENT_CHARACTER);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/SubStringFormatterTests.cs`, plus the
    //! cases probed against the pinned SmartFormat.NET 3.6.1 package.
    //!
    //! The two cases that render a child format through reflection
    //! (`{0:substr(0,2):{ToLower}}`) have no counterpart — this crate has no
    //! reflection source — so they use a nested `{}` instead.

    use std::collections::BTreeMap;

    use super::*;
    use crate::extensions::envelope;
    use crate::extensions::null::NullFormatter;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    fn smart_with(formatter: SubStringFormatter) -> SmartFormatter {
        // The .NET default of `FormatErrorAction.ThrowError` — our
        // `ErrorAction::Error` — so a formatting error surfaces in the tests.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        });
        // Only the formatter under test, the `isnull` formatter its child
        // formats use, and the always-last DefaultFormatter, so these tests pin
        // this formatter rather than the order of the default registry.
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(NullFormatter::new()));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    fn smart() -> SmartFormatter {
        smart_with(SubStringFormatter::new())
    }

    /// .NET's `_person`, as a map: `Name` and `City`.
    fn person() -> Value {
        Value::Map(BTreeMap::from([
            ("Name".to_owned(), Value::from("Long John")),
            ("City".to_owned(), Value::from("New York")),
        ]))
    }

    fn null_name() -> Value {
        Value::Map(BTreeMap::from([("Name".to_owned(), Value::Null)]))
    }

    fn format(template: &str) -> String {
        format_with(&smart(), template, &person())
    }

    fn format_with(smart: &SmartFormatter, template: &str, args: &Value) -> String {
        smart
            .format(template, args)
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// The issue text of the error `template` raises, and its position.
    fn error_of(template: &str) -> (String, usize) {
        error_in(&smart(), template, &person())
    }

    fn error_in(smart: &SmartFormatter, template: &str, args: &Value) -> (String, usize) {
        match smart.format(template, args) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = SubStringFormatter::new();
        assert_eq!(formatter.name(), "substr");
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), ',');
        assert_eq!(formatter.null_display_string(), "");
        assert_eq!(
            formatter.out_of_range_behavior(),
            SubStringOutOfRangeBehavior::ReturnEmptyString
        );
        assert_eq!(SubStringFormatter::new().with_name("s").name(), "s");
    }

    /// Without parentheses `substr` is not a formatter name at all, so the
    /// default formatter writes the string unchanged.
    #[test]
    fn no_parentheses_should_work() {
        assert_eq!(
            format("No parentheses: {Name:substr}"),
            "No parentheses: Long John"
        );
    }

    #[test]
    fn only_delimiter_should_throw() {
        assert_eq!(
            error_of("Only delimiter: {Name:substr(,)}"),
            (
                "The input string '' was not in a correct format.".to_owned(),
                31
            )
        );
    }

    #[test]
    fn named_formatter_without_options_should_throw() {
        assert_eq!(
            error_of("{Name:substr()}").0,
            "Formatter named 'substr' requires at least 1 formatter option and a string? argument."
        );
    }

    #[test]
    fn start_position_longer_than_string() {
        assert_eq!(format("{Name:substr(999)}"), "");
    }

    #[test]
    fn start_position_and_length_longer_than_string() {
        assert_eq!(format("{Name:substr(999,1)}"), "");
    }

    #[test]
    fn only_positive_start_position() {
        assert_eq!(format("{Name:substr(5)}"), "John");
        assert_eq!(format("{Name:substr(0)}"), "Long John");
        assert_eq!(format("{Name:substr(9)}"), "");
    }

    #[test]
    fn start_position_and_positive_length() {
        assert_eq!(format("{City:substr(0,3)}"), "New");
        assert_eq!(format("{Name:substr(9,0)}"), "");
    }

    #[test]
    fn only_negative_start_position() {
        assert_eq!(format("{Name:substr(-4)}"), "John");
    }

    #[test]
    fn negative_start_position_and_positive_length() {
        assert_eq!(format("{Name:substr(-4, 2)}"), "Jo");
    }

    #[test]
    fn negative_start_position_and_negative_length() {
        assert_eq!(format("{Name:substr(-4, -1)}"), "Joh");
    }

    /// The options past the second are read by no one.
    #[test]
    fn extra_options_are_ignored() {
        assert_eq!(format("{Name:substr(1,2,3)}"), "on");
    }

    #[test]
    fn out_of_range_behavior_matrix() {
        use SubStringOutOfRangeBehavior::{
            ReturnEmptyString, ReturnStartIndexToEndOfString, ThrowException,
        };

        // "Long John" is nine characters long. `None` is the out-of-range
        // error, which every behavior raises for a start index or a length
        // that stays negative.
        let cases: &[(&str, [Option<&str>; 3])] = &[
            // template            ReturnEmptyString  ReturnStartIndexToEnd  Throw
            ("{Name:substr(0,999)}", [Some(""), Some("Long John"), None]),
            ("{Name:substr(999)}", [Some(""), Some(""), Some("")]),
            ("{Name:substr(999,1)}", [Some(""), Some(""), None]),
            ("{Name:substr(-999)}", [None, None, None]),
            ("{Name:substr(-999,3)}", [None, None, None]),
            ("{Name:substr(0,-999)}", [None, None, None]),
            ("{Name:substr(5,-9)}", [None, None, None]),
            ("{Name:substr(9,1)}", [Some(""), Some(""), None]),
            (
                "{Name:substr(3,6)}",
                [Some("g John"), Some("g John"), Some("g John")],
            ),
            ("{Name:substr(3,7)}", [Some(""), Some("g John"), None]),
            ("{Name:substr(-4,-5)}", [None, None, None]),
            ("{Name:substr(0,-9)}", [Some(""), Some(""), Some("")]),
        ];

        for (index, behavior) in [
            ReturnEmptyString,
            ReturnStartIndexToEndOfString,
            ThrowException,
        ]
        .into_iter()
        .enumerate()
        {
            let mut formatter = SubStringFormatter::new();
            formatter.set_out_of_range_behavior(behavior);
            assert_eq!(formatter.out_of_range_behavior(), behavior);
            let smart = smart_with(formatter);

            for (template, expected) in cases {
                match expected[index] {
                    Some(expected) => assert_eq!(
                        format_with(&smart, template, &person()),
                        expected,
                        "{template} [{behavior:?}]"
                    ),
                    None => assert_eq!(
                        error_in(&smart, template, &person()).0,
                        OUT_OF_RANGE,
                        "{template} [{behavior:?}]"
                    ),
                }
            }
        }
    }

    #[test]
    fn data_item_is_null() {
        let mut formatter = SubStringFormatter::new();
        formatter.set_null_display_string("???");
        assert_eq!(formatter.null_display_string(), "???");
        let smart = smart_with(formatter);

        assert_eq!(
            format_with(&smart, "{Name:substr(0,3)}", &null_name()),
            "???"
        );
        // The null display string is written through the alignment.
        assert_eq!(
            format_with(&smart, "{Name,10:substr(0,3)}", &null_name()),
            "       ???"
        );
        // A null value never parses the options, so an option that is not a
        // number at all is no error.
        assert_eq!(
            format_with(&smart, "{Name:substr(oops)}", &null_name()),
            "???"
        );
        assert_eq!(
            format_with(&smart, "{Name:substr(-999)}", &null_name()),
            "???"
        );
        // The empty-option check happens before the value is looked at.
        assert_eq!(
            error_in(&smart, "{Name:substr()}", &null_name()).0,
            "Formatter named 'substr' requires at least 1 formatter option and a string? argument."
        );
    }

    /// If a nested format is used, it gets the null value too, and the null
    /// display string is not written.
    #[test]
    fn data_item_is_null_with_child_format() {
        let mut formatter = SubStringFormatter::new();
        formatter.set_null_display_string("???");
        let smart = smart_with(formatter);
        let result = format_with(
            &smart,
            "{Name:substr(0,3):{:isnull:It is null}}",
            &null_name(),
        );
        assert_eq!(result, "It is null");
    }

    #[test]
    fn test_with_changed_split_char() {
        let mut formatter = SubStringFormatter::new();
        assert_eq!(formatter.split_char(), ',');
        formatter.set_split_char('|').unwrap();
        let smart = smart_with(formatter);

        assert_eq!(
            format_with(&smart, "{Name:substr(-4|-1)}", &person()),
            "Joh"
        );
        // The comma is now just text, and not a number.
        assert_eq!(
            error_in(&smart, "{Name:substr(-4,-1)}", &person()).0,
            "The input string '-4,-1' was not in a correct format."
        );
    }

    #[test]
    fn rejects_an_invalid_split_char() {
        let mut formatter = SubStringFormatter::new();
        assert_eq!(formatter.set_split_char('/'), Err(InvalidSplitChar('/')));
        assert_eq!(formatter.split_char(), ',');
        assert!(formatter.set_split_char('~').is_ok());
    }

    /// .NET takes a `string` or a `null` and nothing else.
    #[test]
    fn formatter_without_string_argument_should_throw() {
        let values = [
            Value::Int(12345),
            Value::Bool(true),
            Value::Float(1.5),
            Value::List(vec![Value::Int(1)]),
            Value::Map(BTreeMap::new()),
        ];
        for value in values {
            let args = Value::List(vec![value.clone()]);
            assert_eq!(
                error_in(&smart(), "{0:substr(0,2)}", &args).0,
                "Formatter named 'substr' requires at least 1 formatter option and a string? argument.",
                "{value:?}"
            );
        }
    }

    /// .NET's `ImplicitFormatterEvaluation_With_Wrong_Args_Should_Fail`: with
    /// auto-detection on, a placeholder that names no formatter and cannot be
    /// handled is declined rather than failed, and the next formatter — here
    /// the default one — renders it.
    #[test]
    fn auto_detection_declines_wrong_arguments() {
        let mut formatter = SubStringFormatter::new();
        formatter.set_can_auto_detect(true);
        assert!(formatter.can_auto_detect());
        let smart = smart_with(formatter);

        let args = Value::List(vec![Value::Bool(true)]);
        assert_eq!(format_with(&smart, "{0::(0,2)}", &args), "True");
        // A string with no options is declined for the second reason.
        let args = Value::List(vec![Value::from("ABCDEF")]);
        assert_eq!(format_with(&smart, "{0::(0,2)}", &args), "ABCDEF");
    }

    /// .NET's `FormattingException.Message`, which
    /// `ErrorAction::OutputErrorInResult` writes into the result verbatim: the
    /// issue, the index it is reported at, then the template and a caret line.
    #[test]
    fn format_without_nesting_should_throw() {
        // The one error of this formatter that .NET raises as a
        // `FormattingException`, so its message carries the envelope and it is
        // reported at the start of the format.
        let template = "{0:substr(0,2):just text}";
        let args = Value::List(vec![Value::from("input")]);
        assert_eq!(
            error_in(&smart(), template, &args),
            (envelope(template, NEEDS_NESTED, 15), 15)
        );
        // A null value reaches the same error.
        let template = "{Name:substr(0,3):plain}";
        assert_eq!(
            error_in(&smart(), template, &null_name()).0,
            envelope(template, NEEDS_NESTED, 18)
        );
    }

    /// An empty format is no format at all, so the substring is written
    /// directly.
    #[test]
    fn an_empty_format_writes_the_substring() {
        assert_eq!(format("{Name:substr(0,2):}"), "Lo");
        assert_eq!(
            format_with(&smart(), "{Name:substr(0,2):}", &null_name()),
            ""
        );
    }

    #[test]
    fn substring_using_a_nested_format() {
        assert_eq!(format("{Name:substr(0,2):{}}"), "Lo");
        assert_eq!(format("{Name:substr(0,4):[{}]}"), "[Long]");
        // The child sees the substring, not the whole value.
        assert_eq!(format("{Name:substr(0,4):{:substr(1,2)}}"), "on");
    }

    #[test]
    fn the_alignment_of_the_placeholder_applies() {
        assert_eq!(format("{Name,15:substr(0,4)}"), "           Long");
        assert_eq!(format("{Name,-15:substr(0,4)}|"), "Long           |");
        // A child format is rendered with the same alignment, item by item.
        assert_eq!(
            format("{Name,10:substr(0,4):[{}]}"),
            "         [      Long         ]"
        );
    }

    /// .NET's `int.Parse` with `NumberStyles.Integer`.
    #[test]
    fn options_are_parsed_as_dotnet_integers() {
        assert_eq!(format("{Name:substr(+1)}"), "ong John");
        assert_eq!(format("{Name:substr( -4 , 2 )}"), "Jo");
        assert_eq!(format("{Name:substr(\t1\t)}"), "ong John");
        assert_eq!(format("{Name:substr(\n1)}"), "ong John");
        assert_eq!(format("{Name:substr(0000000005)}"), "John");
        assert_eq!(format("{Name:substr(2147483647)}"), "");

        // The message quotes the option as written, whitespace and all.
        for (template, quoted) in [
            ("{Name:substr( x )}", " x "),
            ("{Name:substr( )}", " "),
            ("{Name:substr(  ,2)}", "  "),
            ("{Name:substr(x,y)}", "x"),
            ("{Name:substr(0,y)}", "y"),
            ("{Name:substr(1 2)}", "1 2"),
            ("{Name:substr(1.0)}", "1.0"),
            ("{Name:substr(0x1)}", "0x1"),
            // A digit that is not an ASCII one is no digit.
            ("{Name:substr(\u{663})}", "\u{663}"),
            // The split character is the comma, so a pipe is part of the option.
            ("{Name:substr(1|2)}", "1|2"),
        ] {
            assert_eq!(
                error_of(template).0,
                std::format!("The input string '{quoted}' was not in a correct format."),
                "{template}"
            );
        }

        for template in [
            "{Name:substr(2147483648)}",
            "{Name:substr(-2147483649)}",
            "{Name:substr(99999999999999999999999999)}",
            "{Name:substr(0,99999999999)}",
        ] {
            assert_eq!(error_of(template).0, INT32_OVERFLOW, "{template}");
        }

        // `int.MinValue` still parses, and is then out of range.
        assert_eq!(error_of("{Name:substr(0,-2147483648)}").0, OUT_OF_RANGE);
        // Two large values whose sum overflows wrap, as they do in .NET, and
        // the slice is out of range either way.
        assert_eq!(
            error_of("{Name:substr(2147483647,2147483647)}").0,
            OUT_OF_RANGE
        );
    }

    /// Every error but "the format requires a nested placeholder" is a plain
    /// .NET exception, so its message carries no envelope and its position is
    /// the one the placeholder reports errors at.
    #[test]
    fn errors_are_reported_at_the_placeholder() {
        assert_eq!(error_of("{Name:substr()}").1, 14);
        assert_eq!(error_of("{Name:substr(,)}").1, 15);
        assert_eq!(error_of("{Name:substr(1.0)}").1, 17);
        assert_eq!(error_of("{Name:substr(99999999999)}").1, 25);
        assert_eq!(error_of("{Name:substr(-999)}").1, 18);
    }

    /// .NET counts a string in UTF-16 code units, so a cut can fall inside a
    /// surrogate pair. The orphaned half becomes U+FFFD, which is the byte
    /// sequence .NET writes once the string is encoded as UTF-8.
    #[test]
    fn a_cut_inside_a_surrogate_pair() {
        let args = Value::Map(BTreeMap::from([(
            "S".to_owned(),
            Value::from("\u{1F600}abc"),
        )]));
        let smart = smart();
        let cut = |template: &str| format_with(&smart, template, &args);

        assert_eq!(cut("{S:substr(0,1)}"), "\u{FFFD}");
        assert_eq!(cut("{S:substr(0,2)}"), "\u{1F600}");
        assert_eq!(cut("{S:substr(1)}"), "\u{FFFD}abc");
        assert_eq!(cut("{S:substr(2)}"), "abc");
        assert_eq!(cut("{S:substr(-3)}"), "abc");
        assert_eq!(cut("{S:substr(1,2)}"), "\u{FFFD}a");
        // The whole string is five code units, not four characters.
        assert_eq!(cut("{S:substr(0,5)}"), "\u{1F600}abc");
        assert_eq!(cut("{S:substr(0,6)}"), "");
    }

    #[test]
    fn two_halves_of_one_pair_do_not_rejoin() {
        // The divergence the entry above stops short of: .NET holds each half
        // as a UTF-16 code unit, so writing the two next to each other
        // re-forms the pair and encodes the emoji. Probed against 3.6.1, which
        // renders "😀" (F0 9F 98 80) for the first two and the two replacement
        // characters for the third — halves in the wrong order do not join
        // there either.
        let args = Value::List(vec![Value::from("\u{1F600}")]);
        let smart = smart();
        let cut = |template: &str| format_with(&smart, template, &args);

        assert_eq!(cut("{0:substr(0,1)}{0:substr(1,1)}"), "\u{FFFD}\u{FFFD}");
        assert_eq!(
            cut("{0:substr(0,1):{}}{0:substr(1,1):{}}"),
            "\u{FFFD}\u{FFFD}"
        );
        assert_eq!(cut("{0:substr(1,1)}{0:substr(0,1)}"), "\u{FFFD}\u{FFFD}");
        // Anything between the halves keeps .NET from joining them, so these
        // two agree.
        assert_eq!(cut("{0:substr(0,1)}x{0:substr(1,1)}"), "\u{FFFD}x\u{FFFD}");
    }

    /// A character outside the ASCII range still counts as one code unit.
    #[test]
    fn a_string_of_wide_characters() {
        let args = Value::Map(BTreeMap::from([("S".to_owned(), Value::from("äöüß"))]));
        let smart = smart();
        assert_eq!(format_with(&smart, "{S:substr(1,2)}", &args), "öü");
        assert_eq!(format_with(&smart, "{S:substr(-1)}", &args), "ß");
        assert_eq!(format_with(&smart, "{S:substr(4)}", &args), "");
    }

    #[test]
    fn utf16_lengths() {
        assert_eq!(utf16_len(""), 0);
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("äöü"), 3);
        assert_eq!(utf16_len("\u{1F600}"), 2);
        assert_eq!(utf16_slice("abc", 1, 2), "bc");
        assert_eq!(utf16_slice("abc", 3, 0), "");
        assert_eq!(utf16_slice("äbc", 0, 2), "äb");
    }
}
