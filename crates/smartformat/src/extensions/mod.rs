//! Formatter extensions beyond `DefaultFormatter`, each a port of the
//! same-named SmartFormat.NET extension. They implement
//! [`Formatter`](crate::formatter::Formatter) and are registered by
//! `SmartFormatter` in .NET's `CreateDefaultSmartFormat` order.
//!
//! Three are left out of that order, exactly as .NET leaves them out, because
//! each is useless until it is given something: `TimeFormatter` a language,
//! [`LocalizationFormatter`] a provider, [`TemplateFormatter`] a template. They
//! are registered by hand, through
//! [`SmartFormatter::register_localization`](crate::SmartFormatter::register_localization),
//! [`SmartFormatter::register_template`](crate::SmartFormatter::register_template)
//! or [`FormatterRegistry::add`](crate::formatter::FormatterRegistry::add),
//! which slots each one where .NET's `WellKnownExtensionTypes` ranks it.

pub mod choose;
pub mod conditional;
#[cfg(feature = "regex-formatters")]
pub mod ismatch;
pub mod list;
pub mod localization;
pub mod null;
#[cfg(feature = "plural")]
pub mod plural;
#[cfg(feature = "plural")]
pub mod plural_rules;
pub mod substring;
pub mod template;
#[cfg(feature = "time")]
pub mod time;

use std::borrow::Cow;
use std::fmt;

#[cfg(feature = "time")]
use crate::fmt::date;
use crate::fmt::number::{self, Number};
use crate::formatter::FormattingInfo;
use crate::parsing::{Format, SplitParts};
use crate::value::Value;
use crate::Error;

pub use choose::ChooseFormatter;
pub use conditional::ConditionalFormatter;
#[cfg(feature = "regex-formatters")]
pub use ismatch::{IsMatchFormatter, RegexOptions};
pub use list::ListFormatter;
pub use localization::{HashMapLocalizationProvider, LocalizationFormatter, LocalizationProvider};
pub use null::NullFormatter;
#[cfg(feature = "plural")]
pub use plural::PluralLocalizationFormatter;
pub use substring::{SubStringFormatter, SubStringOutOfRangeBehavior};
pub use template::{RegisterError, TemplateFormatter};
#[cfg(feature = "time")]
pub use time::{TimeFormatter, TimeSpanFormatOptions, TimeTextInfo};

/// The characters .NET accepts as the split character of a formatter that
/// reads a list of parts (`Utilities.Validation.GetValidSplitCharOrThrow`).
///
/// [`ChooseFormatter`], [`ConditionalFormatter`] and
/// `PluralLocalizationFormatter` all validate against this list.
pub const VALID_SPLIT_CHARS: [char; 3] = ['|', ',', '~'];

/// The .NET default split character.
pub(crate) const DEFAULT_SPLIT_CHAR: char = VALID_SPLIT_CHARS[0];

/// A split character that is not one of [`VALID_SPLIT_CHARS`] was passed to a
/// formatter's `set_split_char` (.NET `ArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSplitChar(pub char);

impl fmt::Display for InvalidSplitChar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Only '{}', '{}' and '{}' are valid split chars.",
            VALID_SPLIT_CHARS[0], VALID_SPLIT_CHARS[1], VALID_SPLIT_CHARS[2]
        )
    }
}

impl std::error::Error for InvalidSplitChar {}

/// `split_char` if [`VALID_SPLIT_CHARS`] holds it, an error otherwise.
pub(crate) fn valid_split_char(split_char: char) -> Result<char, InvalidSplitChar> {
    if VALID_SPLIT_CHARS.contains(&split_char) {
        Ok(split_char)
    } else {
        Err(InvalidSplitChar(split_char))
    }
}

/// The text .NET matches a value against — `CurrentValue.ToString()` — for the
/// two formatters that match one: [`ChooseFormatter`] against its options and
/// `IsMatchFormatter` against its pattern.
///
/// `None` is "there is no text to match": a null, and the two values .NET
/// renders as a CLR type name (`System.Object[]`,
/// `System.Collections.Generic.Dictionary\`2[…]`), which no pattern and no
/// sensible option ever spells. Each caller decides what to do with that —
/// `ismatch` declines the value, `choose` matches nothing and quotes a
/// placeholder text in its error.
///
/// This is *not*
/// [`DefaultFormatter`](crate::formatter::DefaultFormatter)'s table, which
/// looks similar and is deliberately different: that one renders a value for
/// the *output*, so it honours the placeholder's format specifier and fails
/// loudly on a list or a map. This one is a match key, always taken with the
/// empty specifier, and never fails. `value::dotnet_type_name` is the third
/// value-to-text table, and names a value's *type* for an error message.
///
/// Two divergences, both shared by the two callers:
///
/// * .NET converts with the *thread* culture, not with the culture passed to
///   the format call; we use the culture of the call, which is the same thing
///   whenever the two agree.
/// * a `TimeSpan` is .NET's `TimeSpan.ToString()`, which is culture-independent
///   whatever the thread culture is.
pub(crate) fn value_text<'v>(value: &'v Value, info: &FormattingInfo<'_>) -> Option<Cow<'v, str>> {
    let culture = info.culture();
    match value {
        Value::Null | Value::List(_) | Value::Map(_) => None,
        Value::String(text) => Some(Cow::Borrowed(text.as_str())),
        // .NET `bool.ToString()`.
        Value::Bool(true) => Some(Cow::Borrowed("True")),
        Value::Bool(false) => Some(Cow::Borrowed("False")),
        // The empty specifier is always valid, so these cannot fail.
        Value::Int(value) => Some(Cow::Owned(
            number::format_number(Number::Int(*value), "", culture).unwrap_or_default(),
        )),
        Value::UInt(value) => Some(Cow::Owned(
            number::format_number(Number::UInt(*value), "", culture).unwrap_or_default(),
        )),
        Value::Float(value) => Some(Cow::Owned(
            number::format_number(Number::Float(*value), "", culture).unwrap_or_default(),
        )),
        #[cfg(feature = "time")]
        Value::DateTime(value) => Some(Cow::Owned(
            date::format_datetime(value, "", culture).unwrap_or_default(),
        )),
        #[cfg(feature = "time")]
        Value::TimeSpan(value) => Some(Cow::Owned(time::timespan_to_string(value))),
    }
}

/// The .NET `FormattingException` message a test expects, built from the
/// engine's own [`formatting_exception_message`] so that the two can never
/// drift apart.
///
/// [`formatting_exception_message`]: crate::formatter::formatting_exception_message
#[cfg(test)]
pub(crate) fn envelope(template: &str, issue: &str, index: usize) -> String {
    crate::formatter::formatting_exception_message(template, issue, index)
}

/// The parts `format` splits into, or the formatting error .NET's
/// `Format.IndexOf` throws when the search for the separators runs into a
/// crossed literal ([`SplitError::Count`](crate::parsing::SplitError::Count)).
///
/// All three formatters split *before* they count the parts, and .NET never
/// reaches the count when the search throws, so this has to be propagated with
/// `?` rather than folded into "requires at least N parts". Like the count
/// errors, the exception is one .NET raises inside `TryEvaluateFormat` and the
/// evaluator only wraps on the way out, so the message carries no envelope.
///
/// Cutting a part out of the format is deferred to [`split_part`], because .NET
/// defers it too.
pub(crate) fn split_format<'a>(
    info: &FormattingInfo<'_>,
    format: &'a Format,
    separator: char,
) -> Result<SplitParts<'a>, Error> {
    split_format_max(info, format, separator, usize::MAX)
}

/// [`split_format`] with .NET's limit on the number of separators, which only
/// `ListFormatter` passes.
pub(crate) fn split_format_max<'a>(
    info: &FormattingInfo<'_>,
    format: &'a Format,
    separator: char,
    max_count: usize,
) -> Result<SplitParts<'a>, Error> {
    format
        .split_cached(separator, max_count)
        .map_err(|error| info.plain_error_here(&error.to_string()))
}

/// The part a formatter picked, or the formatting error .NET's
/// `Format.Substring` throws while cutting that part out of a format whose
/// separators a crossed literal put out of bounds.
///
/// .NET's `SplitList` only holds the separator offsets and cuts each part when
/// it is indexed, so a part that is never picked never throws: for
/// `{0:choose(1|2|3):a|b|\u12}` only the argument 3 fails. Every access to a
/// part therefore goes through here rather than through the parts directly.
///
/// Every formatter counts the parts before it picks one, so an index past the
/// last part is a bug here rather than anything a template can reach — the
/// panic the slice indexing this replaced would have raised.
pub(crate) fn split_part<'a>(
    info: &FormattingInfo<'_>,
    parts: &'a SplitParts<'a>,
    index: usize,
) -> Result<&'a Format, Error> {
    parts
        .get(index)
        .expect("a formatter only picks a part it has counted")
        .map_err(|error| info.plain_error_here(&error.to_string()))
}
