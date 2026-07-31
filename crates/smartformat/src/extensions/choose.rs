//! Port of SmartFormat.NET's `ChooseFormatter` (milestone M2).
//!
//! Ported from `src/SmartFormat/Extensions/ChooseFormatter.cs`.
//!
//! `{0:choose(1|2|3):one|two|three}` renders the format whose option matches
//! the value. The options are the `|`-separated words in parentheses, the
//! formats the `|`-separated parts of the placeholder's format; one format
//! more than there are options makes the last one the "else" branch:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! // Registered by default; a placeholder has to name it.
//! let smart = SmartFormatter::default();
//!
//! let args = Value::List(vec![Value::Int(2)]);
//! let template = "{0:choose(1|2|3):one|two|three|many}";
//! assert_eq!(smart.format(template, &args).unwrap(), "two");
//! ```

#[cfg(feature = "time")]
use crate::fmt::date;
use crate::fmt::number::{self, Number};
use crate::formatter::{Formatter, FormattingInfo};
use crate::settings::CaseSensitivity;
use crate::value::Value;
use crate::Error;

use super::{InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The default formatter name, .NET `ChooseFormatter.Name`.
const NAME: &str = "choose";

/// Chooses one of several formats by matching the value against a list of
/// options, ported from .NET `ChooseFormatter`.
///
/// The formatter is never auto-detected: a placeholder has to name it
/// (.NET `CanAutoDetect = false`).
#[derive(Debug, Clone)]
pub struct ChooseFormatter {
    name: String,
    split_char: char,
    case_sensitivity: CaseSensitivity,
}

impl ChooseFormatter {
    /// A formatter named `choose`, splitting on `|` and matching options
    /// case-sensitively — the .NET defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            case_sensitivity: CaseSensitivity::CaseSensitive,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the options and the formats are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output — `{0:choose(1~2~3):|one|~|two|~|three|}`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether the option strings are matched case-sensitively. Defaults to
    /// [`CaseSensitivity::CaseSensitive`], as in .NET.
    ///
    /// This is the formatter's own setting, and — as in .NET — it wins over
    /// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive):
    /// .NET picks `settings == own ? settings : own`, which is always `own`.
    /// `null` and `bool` values are matched case-insensitively whatever this
    /// is set to.
    ///
    /// One divergence: .NET compares option strings with
    /// `CultureInfo.CompareInfo.Compare`, which is culture-aware, so a soft
    /// hyphen and other ignorable characters compare equal to nothing at all
    /// (`"a\u{ad}b"` matches the option `ab`). We compare ordinally, as
    /// [`CaseSensitivity`] does everywhere else in this crate.
    pub fn case_sensitivity(&self) -> CaseSensitivity {
        self.case_sensitivity
    }

    pub fn set_case_sensitivity(&mut self, case_sensitivity: CaseSensitivity) {
        self.case_sensitivity = case_sensitivity;
    }

    /// .NET `GetChosenIndex`: the index of the first option that matches the
    /// value, plus the text the value was matched by (which the "not a valid
    /// choice" error quotes).
    fn chosen_index(&self, info: &FormattingInfo<'_>, options: &[&str]) -> (Option<usize>, String) {
        // .NET matches `null` and `bool` case-insensitively whatever the
        // formatter's own case sensitivity is.
        let (text, case_sensitivity, matchable) = match info.current() {
            Value::Null => (
                NULL_TEXT.to_owned(),
                CaseSensitivity::CaseInsensitive,
                Matchable::Yes,
            ),
            // .NET compares against `bool.ToString()`, which is "True"/"False".
            Value::Bool(value) => (
                bool_text(*value).to_owned(),
                CaseSensitivity::CaseInsensitive,
                Matchable::Yes,
            ),
            value => {
                let (text, matchable) = value_text(value, info);
                (text, self.case_sensitivity, matchable)
            }
        };

        let index = match matchable {
            Matchable::No => None,
            Matchable::Yes => options
                .iter()
                .position(|option| case_sensitivity.eq(option, &text)),
        };
        (index, text)
    }
}

impl Default for ChooseFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for ChooseFormatter {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        false
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // .NET reads `FormatterOptions` first, so options holding an escape
        // sequence that resolves to nothing fail before anything else.
        let options = info.formatter_options()?;
        let options: Vec<&str> = options.split(self.split_char).collect();

        let formats = info.format().map(|format| format.split(self.split_char));

        // Check whether the arguments can be handled by this formatter.
        let formats = match formats {
            Some(formats) if formats.len() >= 2 => formats,
            _ => {
                let name = &info.placeholder().formatter_name;
                // Auto detection calls just return a failure to evaluate.
                if name.is_empty() {
                    return Ok(false);
                }
                // .NET throws a plain `FormatException` when the formatter was
                // called explicitly, and the evaluator only wraps that in a
                // `FormattingException` on the way out. So, unlike the errors
                // below, this one carries no `Error parsing format string: …`
                // envelope: `ErrorAction::OutputErrorInResult` writes the inner
                // exception's message, which is the bare text (probed).
                return Err(info.plain_error(
                    &format!("Formatter named '{name}' requires at least 2 format options."),
                    info.error_position(),
                ));
            }
        };

        let (chosen, value_text) = self.chosen_index(info, &options);

        // Validate the number of formats.
        if formats.len() < options.len() {
            return Err(formatting_error(
                info,
                &format!("You must specify at least {} choices", options.len()),
            ));
        }
        if formats.len() > options.len() + 1 {
            return Err(formatting_error(
                info,
                &format!("You cannot specify more than {} choices", options.len() + 1),
            ));
        }
        if chosen.is_none() && formats.len() == options.len() {
            return Err(formatting_error(
                info,
                &format!(
                    "\"{value_text}\" is not a valid choice, and a \"default\" choice was not supplied"
                ),
            ));
        }

        // Without a match, the one extra format is the "else" branch.
        let chosen = chosen.unwrap_or(formats.len() - 1);
        info.format_as_child(&formats[chosen], info.current())?;

        Ok(true)
    }
}

/// The text .NET matches a null value by.
const NULL_TEXT: &str = "null";

/// .NET `bool.ToString()`.
fn bool_text(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Whether any option can match the value at all.
enum Matchable {
    Yes,
    /// A value .NET renders as a CLR type name (`System.Object[]`), which no
    /// sensible option ever spells.
    No,
}

/// The text .NET matches the options against: `CurrentValue.ToString()`.
///
/// Two divergences:
///
/// * .NET converts with the *thread* culture, not with the culture passed to
///   the format call; we use the culture of the call, which is the same thing
///   whenever the two agree.
/// * a list or a map has no .NET counterpart worth matching — .NET renders
///   `System.Object[]` and the like — so nothing matches it, and the error
///   message describes it instead.
fn value_text(value: &Value, info: &FormattingInfo<'_>) -> (String, Matchable) {
    let culture = info.culture();
    match value {
        // Handled by the caller, which matches these case-insensitively.
        Value::Null => (NULL_TEXT.to_owned(), Matchable::Yes),
        Value::Bool(value) => (bool_text(*value).to_owned(), Matchable::Yes),
        Value::String(text) => (text.clone(), Matchable::Yes),
        // The empty specifier is always valid, so these cannot fail.
        Value::Int(value) => (
            number::format_number(Number::Int(*value), "", culture).unwrap_or_default(),
            Matchable::Yes,
        ),
        Value::UInt(value) => (
            number::format_number(Number::UInt(*value), "", culture).unwrap_or_default(),
            Matchable::Yes,
        ),
        Value::Float(value) => (
            number::format_number(Number::Float(*value), "", culture).unwrap_or_default(),
            Matchable::Yes,
        ),
        #[cfg(feature = "time")]
        Value::DateTime(value) => (
            date::format_datetime(value, "", culture).unwrap_or_default(),
            Matchable::Yes,
        ),
        Value::List(_) => ("<list>".to_owned(), Matchable::No),
        Value::Map(_) => ("<map>".to_owned(), Matchable::No),
    }
}

/// A .NET `FormattingException` raised by the formatter itself
/// (`IFormattingInfo.FormattingException`), positioned at the start of the
/// placeholder's format.
fn formatting_error(info: &FormattingInfo<'_>, issue: &str) -> Error {
    info.formatting_error(issue, info.error_position())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/ChooseFormatterTests.cs`. The enum
    //! cases have no counterpart — `Value` has no enum variant — and the case
    //! that renders a list needs the `list` formatter, which lands later.

    use std::collections::BTreeMap;

    use super::*;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    fn smart_with(settings: SmartSettings, formatter: ChooseFormatter) -> SmartFormatter {
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

    /// The .NET default of `FormatErrorAction.ThrowError` — our
    /// [`ErrorAction::Error`] — so a formatting error surfaces in the tests.
    fn smart() -> SmartFormatter {
        smart_with(
            SmartSettings {
                format_error_action: ErrorAction::Error,
                ..SmartSettings::default()
            },
            ChooseFormatter::new(),
        )
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
        smart()
            .format(template, &args([value]))
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// .NET's `FormattingException.Message`, which
    /// `ErrorAction::OutputErrorInResult` writes into the result verbatim:
    /// the issue, the index it is reported at, then the template and a caret
    /// line. Probed against 3.6.1.
    fn envelope(template: &str, issue: &str, index: usize) -> String {
        std::format!(
            "Error parsing format string: {issue} at {index}\n{template}\n{}^",
            "-".repeat(index)
        )
    }

    fn error_of(template: &str, value: Value) -> String {
        match smart().format(template, &args([value])) {
            Err(Error::Format { message, .. }) => message,
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = ChooseFormatter::new();
        assert_eq!(formatter.name(), "choose");
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), '|');
        assert_eq!(formatter.case_sensitivity(), CaseSensitivity::CaseSensitive);
        assert_eq!(ChooseFormatter::new().with_name("c").name(), "c");
    }

    #[test]
    fn is_never_auto_detected() {
        // Without a formatter name the choose formatter is skipped, and the
        // default formatter renders the value — a string ignores the
        // specifier. (.NET renders this template with whichever of its other
        // auto-detecting extensions takes it first, which we do not have
        // registered here.)
        assert_eq!(format("{0:(1|2):a|b}", Value::from("x")), "x");
    }

    #[test]
    fn works_with_numbers_strings_and_booleans() {
        let cases: &[(&str, Value, &str)] = &[
            ("{0:choose(1|2|3):one|two|three}", Value::Int(1), "one"),
            ("{0:choose(1|2|3):one|two|three}", Value::Int(2), "two"),
            ("{0:choose(1|2|3):one|two|three}", Value::Int(3), "three"),
            ("{0:choose(3|2|1):three|two|one}", Value::Int(1), "one"),
            ("{0:choose(3|2|1):three|two|one}", Value::Int(2), "two"),
            ("{0:choose(3|2|1):three|two|one}", Value::Int(3), "three"),
            ("{0:choose(1|2|3):one|two|three}", Value::from("1"), "one"),
            ("{0:choose(1|2|3):one|two|three}", Value::from("2"), "two"),
            ("{0:choose(1|2|3):one|two|three}", Value::from("3"), "three"),
            (
                "{0:choose(A|B|C):Alpha|Bravo|Charlie}",
                Value::from("A"),
                "Alpha",
            ),
            (
                "{0:choose(A|B|C):Alpha|Bravo|Charlie}",
                Value::from("B"),
                "Bravo",
            ),
            (
                "{0:choose(A|B|C):Alpha|Bravo|Charlie}",
                Value::from("C"),
                "Charlie",
            ),
            ("{0:choose(True|False):yep|nope}", Value::Bool(true), "yep"),
            (
                "{0:choose(True|False):yep|nope}",
                Value::Bool(false),
                "nope",
            ),
        ];

        for (template, value, expected) in cases {
            assert_eq!(&format(template, value.clone()), expected, "{template}");
        }
    }

    #[test]
    fn bool_matches_case_insensitively() {
        // .NET compares `bool.ToString()` — "True"/"False" — ignoring case,
        // whatever the formatter's own case sensitivity is.
        for template in [
            "{0:choose(true|false):yep|nope}",
            "{0:choose(TRUE|FALSE):yep|nope}",
            "{0:choose(True|FALSE):yep|nope}",
        ] {
            assert_eq!(format(template, Value::Bool(true)), "yep", "{template}");
            assert_eq!(format(template, Value::Bool(false)), "nope", "{template}");
        }
    }

    #[test]
    fn null_matches_case_insensitively() {
        for template in [
            "{0:choose(null):is null|default}",
            "{0:choose(NULL):is null|default}",
            "{0:choose(Null):is null|default}",
        ] {
            assert_eq!(format(template, Value::Null), "is null", "{template}");
        }
    }

    #[test]
    fn strings_are_matched_case_sensitively() {
        assert_eq!(
            format(
                "{0:choose(string|String):one|two|default}",
                Value::from("String")
            ),
            "two"
        );
        assert_eq!(
            format(
                "{0:choose(string|STRING):one|two|default}",
                Value::from("String")
            ),
            "default"
        );
    }

    #[test]
    fn a_case_insensitive_formatter_matches_any_case() {
        let mut formatter = ChooseFormatter::new();
        formatter.set_case_sensitivity(CaseSensitivity::CaseInsensitive);
        let smart = smart_with(SmartSettings::default(), formatter);
        let template = "{0:choose(string|STRING):one|two|default}";
        let result = smart
            .format(template, &args([Value::from("String")]))
            .unwrap();
        assert_eq!(result, "one");
    }

    #[test]
    fn the_formatters_case_sensitivity_wins_over_the_settings() {
        // .NET picks `settings == own ? settings : own`, which is always the
        // formatter's own setting: case-insensitive settings do not make a
        // case-sensitive formatter match.
        let smart = smart_with(
            SmartSettings {
                case_sensitive: CaseSensitivity::CaseInsensitive,
                ..SmartSettings::default()
            },
            ChooseFormatter::new(),
        );
        let template = "{0:choose(string|STRING):one|two|default}";
        let result = smart
            .format(template, &args([Value::from("String")]))
            .unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn defaults_to_the_last_item() {
        let template = "{0:choose(1|2|3):one|two|three|default}";
        assert_eq!(format(template, Value::Int(1)), "one");
        assert_eq!(format(template, Value::Int(2)), "two");
        assert_eq!(format(template, Value::Int(3)), "three");
        assert_eq!(format(template, Value::Int(4)), "default");
        assert_eq!(format(template, Value::Int(99)), "default");
        assert_eq!(format(template, Value::Null), "default");
        assert_eq!(format(template, Value::Bool(true)), "default");
        assert_eq!(format(template, Value::from("whatever")), "default");
    }

    #[test]
    fn has_a_special_case_for_null() {
        assert_eq!(
            format("{0:choose(null):nothing|{}}", Value::Null),
            "nothing"
        );
        assert_eq!(format("{0:choose(null):nothing|{}}", Value::Int(5)), "5");

        let template = "{0:choose(null|5):nothing|five|{}}";
        assert_eq!(format(template, Value::Null), "nothing");
        assert_eq!(format(template, Value::Int(5)), "five");
        assert_eq!(format(template, Value::Int(6)), "6");
    }

    #[test]
    fn numbers_are_matched_by_their_plain_text() {
        // .NET matches `value.ToString()`, which for a `double` is the shortest
        // round-trippable form and never uses a group separator.
        assert_eq!(
            format("{0:choose(1.5|2.5):a|b|else}", Value::Float(1.5)),
            "a"
        );
        assert_eq!(format("{0:choose(2|3):a|b|else}", Value::Float(2.0)), "a");
        assert_eq!(
            format("{0:choose(1.50|2.5):a|b|else}", Value::Float(1.5)),
            "else"
        );
        assert_eq!(
            format("{0:choose(1000|2000):a|b|else}", Value::Int(1000)),
            "a"
        );
        assert_eq!(format("{0:choose(-7):a|else}", Value::Int(-7)), "a");
    }

    #[test]
    fn an_empty_option_matches_the_empty_string() {
        let template = "{0:choose(null|):is null|is empty|{}}";
        assert_eq!(format(template, Value::Null), "is null");
        assert_eq!(format(template, Value::from("")), "is empty");
        assert_eq!(format(template, Value::from("word")), "word");
    }

    #[test]
    fn the_result_may_contain_placeholders() {
        let data = map([("Input", Value::from("good")), ("Other", Value::Int(7))]);
        let template = "{Input:choose(null|):nothing|empty|{} and {Other}}";
        assert_eq!(smart().format(template, &data).unwrap(), "good and 7");
    }

    #[test]
    fn may_contain_nested_formats_as_choice() {
        let template = "{NullableInt:choose(null):{IntValueIfNull:N2}|{:N2}}";

        let data = map([
            ("NullableInt", Value::Int(1234)),
            ("IntValueIfNull", Value::Int(9999)),
        ]);
        assert_eq!(smart().format(template, &data).unwrap(), "1,234.00");

        let data = map([
            ("NullableInt", Value::Null),
            ("IntValueIfNull", Value::Int(9999)),
        ]);
        assert_eq!(smart().format(template, &data).unwrap(), "9,999.00");
    }

    #[test]
    fn may_contain_nested_choose_formats() {
        let template = "{NullableInt:choose(null):{IntValueIfNull:choose(1000|2000):1k|2k}|{:N2}}";

        let data = map([
            ("NullableInt", Value::Int(1234)),
            ("IntValueIfNull", Value::Int(9999)),
        ]);
        assert_eq!(smart().format(template, &data).unwrap(), "1,234.00");

        for (value, expected) in [(1000, "1k"), (2000, "2k")] {
            let data = map([
                ("NullableInt", Value::Null),
                ("IntValueIfNull", Value::Int(value)),
            ]);
            assert_eq!(smart().format(template, &data).unwrap(), expected);
        }
    }

    #[test]
    fn a_nested_placeholder_does_not_split_the_format() {
        // The `|` inside the nested placeholder belongs to it.
        let template = "{0:choose(1|2):{1:choose(9|8):nine|eight}|other}";
        assert_eq!(
            smart()
                .format(template, &args([Value::Int(1), Value::Int(9)]))
                .unwrap(),
            "nine"
        );
        assert_eq!(
            smart()
                .format(template, &args([Value::Int(2), Value::Int(9)]))
                .unwrap(),
            "other"
        );
    }

    #[test]
    fn works_with_a_changed_split_char() {
        // Set the split char from | to ~, so | can be used in the output.
        let mut formatter = ChooseFormatter::new();
        formatter.set_split_char('~').unwrap();
        let smart = smart_with(SmartSettings::default(), formatter);
        let template = "{0:choose(1~2~3):|one|~|two|~|three|}";
        let result = smart.format(template, &args([Value::Int(2)])).unwrap();
        assert_eq!(result, "|two|");
    }

    #[test]
    fn rejects_an_invalid_split_char() {
        let mut formatter = ChooseFormatter::new();
        assert_eq!(formatter.set_split_char('/'), Err(InvalidSplitChar('/')));
        assert_eq!(formatter.split_char(), '|');
        assert!(formatter.set_split_char(',').is_ok());
        assert!(formatter.set_split_char('~').is_ok());
        assert!(formatter.set_split_char('|').is_ok());
        assert_eq!(
            InvalidSplitChar('/').to_string(),
            "Only '|', ',' and '~' are valid split chars."
        );
    }

    #[test]
    fn splits_on_a_separator_inside_an_escape_sequence() {
        // .NET searches the source text of a literal, so the `|` of the
        // invalid escape sequence `\|` splits the format; what is left of the
        // first choice is the lone `\`, which resolves to itself.
        let template = r"{0:choose(1|2):a\|b|c}";
        assert_eq!(format(template, Value::Int(1)), r"a\");
        assert_eq!(format(template, Value::Int(2)), "b");
    }

    #[test]
    fn keeps_valid_escape_sequences() {
        assert_eq!(format(r"{0:choose(1|2):a\nb|c}", Value::Int(1)), "a\nb");
        assert_eq!(format(r"{0:choose(1|2):a\{b|c}", Value::Int(1)), "a{b");
        assert_eq!(format(r"{0:choose(1|2):a\:b|c}", Value::Int(1)), "a:b");
    }

    #[test]
    fn escaped_parens_in_the_options() {
        let template = r"{0:choose(a\)b|c):one|two|else}";
        assert_eq!(format(template, Value::from("a)b")), "one");
        assert_eq!(format(template, Value::from("c")), "two");
        assert_eq!(format(template, Value::from("x")), "else");
    }

    #[test]
    fn empty_options_match_the_empty_string() {
        assert_eq!(format("{0:choose():a|b}", Value::from("")), "a");
        assert_eq!(format("{0:choose():a|b}", Value::from("x")), "b");
    }

    #[test]
    fn empty_choices_render_nothing() {
        assert_eq!(format("{0:choose(1|2):|c}", Value::Int(1)), "");
        assert_eq!(format("{0:choose(1|2):a|}", Value::Int(2)), "");
    }

    #[test]
    fn the_alignment_of_the_placeholder_applies() {
        assert_eq!(
            format("{0,10:choose(1|2):one|two}", Value::Int(1)),
            "       one"
        );
        assert_eq!(
            format("{0,-6:choose(1|2):one|two}", Value::Int(2)),
            "two   "
        );
    }

    #[test]
    fn a_chosen_format_may_hold_a_placeholder_of_its_own() {
        let template = "{0:choose(1|2):one|{1}two|three}";
        for (value, expected) in [(1, "one"), (2, "Xtwo"), (3, "three")] {
            let args = args([Value::Int(value), Value::from("X")]);
            assert_eq!(smart().format(template, &args).unwrap(), expected);
        }
    }

    #[test]
    fn errors_when_the_choice_is_invalid() {
        for template in ["{0:choose(1|2):1|2}", "{0:choose(1|2):a|b}"] {
            assert_eq!(
                error_of(template, Value::Int(99)),
                envelope(
                    template,
                    "\"99\" is not a valid choice, and a \"default\" choice was not supplied",
                    15
                ),
                "{template}"
            );
        }
    }

    #[test]
    fn errors_when_there_are_fewer_than_two_formats() {
        // .NET's plain `FormatException`, which quotes the formatter name.
        for template in [
            "{0:choose(1|2):single}",
            "{0:choose(1|2)}",
            "{0:choose(1):1}",
        ] {
            assert_eq!(
                error_of(template, Value::Int(1)),
                "Formatter named 'choose' requires at least 2 format options.",
                "{template}"
            );
        }
    }

    #[test]
    fn errors_when_choices_are_too_few_or_too_many() {
        assert_eq!(
            error_of("{0:choose(1|2|3):1|2}", Value::Int(1)),
            envelope(
                "{0:choose(1|2|3):1|2}",
                "You must specify at least 3 choices",
                17
            )
        );
        assert_eq!(
            error_of("{0:choose(1):1|2|3}", Value::Int(1)),
            envelope(
                "{0:choose(1):1|2|3}",
                "You cannot specify more than 2 choices",
                13
            )
        );
        assert_eq!(
            error_of("{0:choose(1|2):1|2|3|4}", Value::Int(1)),
            envelope(
                "{0:choose(1|2):1|2|3|4}",
                "You cannot specify more than 3 choices",
                15
            )
        );
    }

    #[test]
    fn an_unmatched_value_is_quoted_in_the_error() {
        for (value, quoted) in [
            (Value::Null, "null"),
            (Value::Bool(true), "True"),
            (Value::from("hello"), "hello"),
            (Value::Int(-7), "-7"),
        ] {
            let message = error_of("{0:choose(1|2):a|b}", value.clone());
            assert_eq!(
                message,
                envelope(
                    "{0:choose(1|2):a|b}",
                    &std::format!(
                        "\"{quoted}\" is not a valid choice, and a \"default\" choice was not supplied"
                    ),
                    15
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_list_never_matches_an_option() {
        // .NET matches against `System.Object[]`, which no option spells;
        // here the value is simply not matchable, so the else branch wins.
        let list = Value::List(vec![Value::Int(1)]);
        assert_eq!(format("{0:choose(1|2):a|b|else}", list), "else");
    }

    #[test]
    fn the_error_action_applies_to_the_formatters_errors() {
        let smart = smart_with(
            SmartSettings {
                format_error_action: ErrorAction::Ignore,
                ..SmartSettings::default()
            },
            ChooseFormatter::new(),
        );
        let result = smart
            .format("x{0:choose(1|2):a|b}y", &args([Value::Int(99)]))
            .unwrap();
        assert_eq!(result, "xy");
    }
}
