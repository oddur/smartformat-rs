//! Port of SmartFormat.NET's `NullFormatter`.
//!
//! Ported from `src/SmartFormat/Extensions/NullFormatter.cs`.
//!
//! `{0:isnull:nothing|{}}` picks between two formats by whether the value is
//! null: the first part is rendered for a null, the second for anything else.
//! With only one part a value that is not null renders nothing at all:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! let smart = SmartFormatter::default();
//!
//! let args = Value::List(vec![Value::Null, Value::from("here")]);
//! assert_eq!(smart.format("{0:isnull:nothing|{}}", &args).unwrap(), "nothing");
//! assert_eq!(smart.format("{1:isnull:nothing|{}}", &args).unwrap(), "here");
//! assert_eq!(smart.format("{1:isnull:nothing}", &args).unwrap(), "");
//! ```
//!
//! The formatter takes no options and never auto-detects, so a placeholder has
//! to name it.

use crate::formatter::{Formatter, FormattingInfo};
use crate::value::Value;
use crate::Error;

use super::{split_format, split_part, InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The default formatter name, .NET `NullFormatter.Name`.
const NAME: &str = "isnull";

/// Picks one of two formats by whether the value is null, ported from .NET
/// `NullFormatter`.
///
/// A placeholder has to name the formatter unless
/// [`set_can_auto_detect`](Self::set_can_auto_detect) turns auto-detection on
/// (.NET `CanAutoDetect`, which defaults to `false` here as it does there).
#[derive(Debug, Clone)]
pub struct NullFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
}

impl NullFormatter {
    /// A formatter named `isnull`, splitting on `|` and not auto-detected —
    /// the .NET defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: false,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the two formats are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output — `{0:isnull:|nothing|~|{}|}`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
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
}

impl Default for NullFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for NullFormatter {
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
        // .NET reads both before it checks either, so the options of
        // `{0:isnull(\q):a}` fail before the format is looked at, and a split
        // that throws fails before the options are complained about.
        let options = info.formatter_options()?;
        let options = options.trim();
        // .NET splits before it counts, so a split that throws is the answer
        // however many parts the formatter wanted.
        let formats = info
            .format()
            .map(|format| split_format(info, format, self.split_char))
            .transpose()?;

        // Check whether the arguments can be handled by this formatter.
        if !options.is_empty() {
            return info.decline_or_error(|name| {
                format!("Formatter named '{name}' does not allow choose options")
            });
        }

        let formats = match formats {
            Some(formats) if matches!(formats.len(), 1 | 2) => formats,
            _ => {
                return info.decline_or_error(|name| {
                    format!("Formatter named '{name}' must have 1 or 2 format options")
                })
            }
        };

        let current = info.current();

        // Use the format for null.
        if matches!(current, Value::Null) {
            info.format_as_child(split_part(info, &formats[0])?, current)?;
            return Ok(true);
        }

        // Use the format for a value other than null.
        if formats.len() > 1 {
            info.format_as_child(split_part(info, &formats[1])?, current)?;
            return Ok(true);
        }

        // There is no format for a value other than null. .NET writes an empty
        // span, which still pads the placeholder's alignment.
        info.write("");
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/NullFormatterTests.cs`, plus the cases
    //! probed against the pinned SmartFormat.NET 3.6.1 package.
    //!
    //! The .NET cases that pass an array as the value use an integer here: a
    //! list has no default rendering in this crate (see DESIGN.md, "Known
    //! divergences"), so `{0:isnull:x|{}}` over one would fail in the child
    //! format rather than in this formatter.

    use std::collections::BTreeMap;

    use super::*;
    use crate::extensions::ChooseFormatter;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    fn smart_with(formatter: NullFormatter) -> SmartFormatter {
        // The .NET default of `FormatErrorAction.ThrowError` — our
        // `ErrorAction::Error` — so a formatting error surfaces in the tests.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        });
        // Only the formatter under test, the `choose` formatter its child
        // formats use, and the always-last DefaultFormatter, so these tests pin
        // this formatter rather than the order of the default registry.
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(ChooseFormatter::new()));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    fn smart() -> SmartFormatter {
        smart_with(NullFormatter::new())
    }

    fn args(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn format(template: &str, value: Value) -> String {
        format_with(&smart(), template, &args([value]))
    }

    fn format_with(smart: &SmartFormatter, template: &str, args: &Value) -> String {
        smart
            .format(template, args)
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    fn error_of(template: &str, value: Value) -> (String, usize) {
        match smart().format(template, &args([value])) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = NullFormatter::new();
        assert_eq!(formatter.name(), "isnull");
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), '|');
        assert_eq!(NullFormatter::new().with_name("n").name(), "n");
    }

    /// Without a formatter name the formatter is never consulted, so the
    /// default formatter writes the empty string a null renders as.
    #[test]
    fn null_without_format_should_be_empty_string() {
        assert_eq!(format("{0}", Value::Null), "");
        assert_eq!(format("{JustNull}", map([("JustNull", Value::Null)])), "");
    }

    #[test]
    fn not_null_should_be_empty_string() {
        // A named `isnull` with one format writes nothing for a value that is
        // not null; the .NET test reaches the same output through the default
        // formatter, since `isnull` does not auto-detect.
        for template in ["{0:}", "{0:something}"] {
            assert_eq!(format(template, Value::from("")), "", "{template}");
        }
    }

    #[test]
    fn null_with_format_should_be_format_string() {
        assert_eq!(format("{0:isnull:}", Value::Null), "");
        assert_eq!(format("{0:isnull:nothing}", Value::Null), "nothing");
    }

    #[test]
    fn empty_format_option_should_only_be_output_if_argument_is_null() {
        for value in [Value::Null, Value::from("a string"), Value::Int(123)] {
            assert_eq!(format("{0:isnull:}", value.clone()), "", "{value:?}");
        }
    }

    #[test]
    fn format_option_should_only_be_output_if_argument_is_null() {
        let cases: &[(Value, &str, &str)] = &[
            // Null value and format for null.
            (Value::Null, "It's null", "It's null"),
            // With a format for "not null".
            (
                Value::from("a string"),
                "It's null|It's a string",
                "It's a string",
            ),
            // No format for "not null".
            (Value::from("a string"), "It's null", ""),
            (Value::Int(123), "It's null|Numbers", "Numbers"),
            (Value::Bool(false), "It's null|Not null", "Not null"),
            // An empty string is not null.
            (Value::from(""), "It's null|Not null", "Not null"),
        ];

        for (value, formats, expected) in cases {
            let template = std::format!("{{0:isnull:{formats}}}");
            assert_eq!(&format(&template, value.clone()), expected, "{template}");
        }
    }

    /// The chosen format is rendered against the value, so it may hold a
    /// placeholder of its own.
    #[test]
    fn may_contain_nested_formats() {
        let template =
            "{NullableInt:isnull:Was null|{IntValueIfNotNull:choose(1000|2000):1k|{:N2}}}";
        let cases = [
            (Value::Null, 0, "Was null"),
            (Value::Int(123), 1000, "1k"),
            (Value::Int(123), 2000, "2,000.00"),
        ];
        for (nullable, if_not_null, expected) in cases {
            let data = map([
                ("NullableInt", nullable.clone()),
                ("IntValueIfNotNull", Value::Int(if_not_null)),
            ]);
            assert_eq!(format_with(&smart(), template, &data), expected);
        }

        // The value the child format sees is the one the placeholder resolved.
        assert_eq!(format("{0:isnull:[{}]}", Value::Null), "[]");
        assert_eq!(format("{0:isnull:x|[{}]}", Value::from("v")), "[v]");
    }

    #[test]
    fn must_not_contain_choose_options() {
        assert_eq!(
            error_of("{0:isnull(op|ti|ons):Is null}", Value::Int(123)),
            (
                "Formatter named 'isnull' does not allow choose options".to_owned(),
                21
            )
        );
        // The options are trimmed before they are looked at, so whitespace is
        // no option at all.
        assert_eq!(format("{0:isnull( ):x}", Value::Null), "x");
        assert_eq!(format("{0:isnull(\t\n):x}", Value::Null), "x");
    }

    #[test]
    fn format_count_must_be_1_or_2() {
        assert_eq!(
            error_of("{0:isnull:1|2|3}", Value::Int(123)),
            (
                "Formatter named 'isnull' must have 1 or 2 format options".to_owned(),
                10
            )
        );
        // .NET splits the source text of a literal, so an escaped separator
        // splits all the same — and makes this a three-part format.
        assert_eq!(
            error_of(r"{0:isnull:a\|b|c}", Value::Null).0,
            "Formatter named 'isnull' must have 1 or 2 format options"
        );
    }

    /// A placeholder that ends after the formatter name still carries an empty
    /// format, which splits into one part.
    #[test]
    fn an_empty_format_is_one_part() {
        assert_eq!(format("{0:isnull()}", Value::Null), "");
        assert_eq!(format("{0:isnull()}", Value::from("a string")), "");
        assert_eq!(format("{0:isnull:|}", Value::Null), "");
        assert_eq!(format("{0:isnull:|}", Value::from("a string")), "");
    }

    /// .NET writes an empty span for a value that is not null and has no
    /// format of its own, which the alignment still pads.
    #[test]
    fn the_alignment_of_the_placeholder_applies() {
        assert_eq!(format("{0,10:isnull:N}", Value::Null), "         N");
        assert_eq!(format("{0,10:isnull:N}", Value::from("v")), "          ");
        assert_eq!(format("{0,10:isnull:N|Y}", Value::from("v")), "         Y");
        assert_eq!(format("{0,-3:isnull:N}|", Value::Null), "N  |");
    }

    #[test]
    fn works_with_a_changed_split_char() {
        let mut formatter = NullFormatter::new();
        formatter.set_split_char('~').unwrap();
        let smart = smart_with(formatter);

        let template = "{0:isnull:|null|~|not null|}";
        assert_eq!(
            format_with(&smart, template, &args([Value::Null])),
            "|null|"
        );
        assert_eq!(
            format_with(&smart, template, &args([Value::from("v")])),
            "|not null|"
        );
    }

    #[test]
    fn rejects_an_invalid_split_char() {
        let mut formatter = NullFormatter::new();
        assert_eq!(formatter.set_split_char('/'), Err(InvalidSplitChar('/')));
        assert_eq!(formatter.split_char(), '|');
        assert!(formatter.set_split_char(',').is_ok());
    }

    /// With auto-detection on, a placeholder that names no formatter and holds
    /// options is declined rather than failed.
    #[test]
    fn auto_detection_can_be_turned_on() {
        let mut formatter = NullFormatter::new();
        formatter.set_can_auto_detect(true);
        assert!(formatter.can_auto_detect());
        let smart = smart_with(formatter);

        assert_eq!(format_with(&smart, "{0:a|b}", &args([Value::Null])), "a");
        assert_eq!(
            format_with(&smart, "{0:a|b}", &args([Value::from("v")])),
            "b"
        );
        // Three parts are too many for this formatter, so the next one — here
        // `choose`, which needs at least two — takes the placeholder.
        assert_eq!(
            format_with(
                &smart,
                "{0:choose(1|2):one|two|else}",
                &args([Value::Int(1)])
            ),
            "one"
        );
    }
}
