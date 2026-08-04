//! Port of SmartFormat.NET's `TimeFormatter` and its time-text resources (milestone M4).
//!
//! Ported from the `SmartFormat.Extensions.Time` package, version 3.6.1 — the
//! release that goes with the core the rest of this crate is ported from:
//! `TimeFormatter.cs`, `Utilities/TimeSpanUtility.cs`,
//! `Utilities/TimeSpanFormatOptions.cs`,
//! `Utilities/TimeSpanFormatOptionsConverter.cs`, `Utilities/TimeTextInfo.cs`,
//! `Utilities/CommonLanguagesTimeTextInfo.cs` and `Resources/*.json`.
//!
//! `{0:time:…}` writes a duration the way a person would say it:
//!
//! ```
//! use jiff::SignedDuration;
//! use smartformat::extensions::time::TimeFormatter;
//! use smartformat::{SmartFormatter, Value};
//!
//! // .NET's CreateDefaultSmartFormat leaves this extension out — a formatter
//! // that is never handed a duration has no use for it — so it is registered
//! // by hand. Where it lands in the registry does not matter: it never
//! // auto-detects, so a placeholder always reaches it by name.
//! let mut smart = SmartFormatter::default();
//! smart.formatters_mut().add(Box::new(TimeFormatter::new()));
//!
//! let args = Value::List(vec![Value::TimeSpan(SignedDuration::from_secs(90_061))]);
//! assert_eq!(smart.format("{0:time:}", &args).unwrap(), "1 day 1 hour 1 minute 1 second");
//! assert_eq!(smart.format("{0:time:hours}", &args).unwrap(), "25 hours");
//! assert_eq!(smart.format("{0:time:abbr}", &args).unwrap(), "1d 1h 1m 1s");
//! assert_eq!(
//!     smart.format_with_culture_name("{0:time:}", &args, "de-DE").unwrap(),
//!     "1 Tag 1 Stunde 1 Minute 1 Sekunde"
//! );
//! ```
//!
//! A [`Value::DateTime`] is written as the span between it and *now*, which
//! [`SmartSettings::now`](crate::SmartSettings::now) pins.
//!
//! The module also carries the .NET standard `TimeSpan` format specifiers —
//! [`format_timespan`] and [`timespan_to_string`] — which are what
//! [`DefaultFormatter`](crate::formatter::DefaultFormatter) writes for a
//! [`Value::TimeSpan`], and what the `choose` and `ismatch` formatters match
//! against, since .NET reaches them through `TimeSpan.ToString()`.

// A `TimeTextInfo` picks its unit words with a `PluralRules.PluralRuleDelegate`,
// exactly as .NET's does, so this extension is built on the plural rule table
// rather than on a second copy of it. `Cargo.toml` therefore has `time` imply
// `plural`, which is why nothing below is gated on it.

pub mod options;
pub mod standard;
pub mod text_info;
pub mod utility;

use crate::fmt::culture::{named_culture_language, two_letter_iso_language_name};
use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::{Format, FormatItem};
use crate::value::{dotnet_type_name, Value};
use crate::Error;

pub use options::{parse as parse_options, TimeSpanFormatOptions};
pub use standard::{format_timespan, timespan_to_string, INVALID_SPEC_MESSAGE};
pub use text_info::{common_language, common_languages, TimeTextInfo};
pub use utility::{
    round, to_time_parts, to_time_string, TimeSpanOverflow, ABSOLUTE_DEFAULTS,
    DEFAULT_FORMAT_OPTIONS, OVERFLOW_MESSAGE,
};

/// The default formatter name, .NET `TimeFormatter.Name`.
///
/// .NET 3.x has one name per formatter: the obsolete `Names` property once
/// held `"timespan"`, `"time"` and `""`, but only `"time"` selects the
/// formatter in 3.6.1.
const NAME: &str = "time";

/// The language .NET falls back to when none of the shipped ones fits the
/// culture (.NET `TimeFormatter.FallbackLanguage`).
const DEFAULT_FALLBACK_LANGUAGE: &str = "en";

/// The `TwoLetterISOLanguageName` of .NET's invariant culture, which is not a
/// language any [`TimeTextInfo`] is filed under — so the invariant culture
/// always ends up at the fallback language.
const INVARIANT_LANGUAGE: &str = "iv";

/// Writes a duration as human-readable text, ported from .NET `TimeFormatter`.
///
/// The formatter never auto-detects (.NET `CanAutoDetect` is `false` here), so
/// a placeholder has to name it: `{0:time:…}`. `{0:time(en):…}` names the
/// language, and `{0:time(abbr hours)}` — options with an empty format — is
/// the SmartFormat 2.x spelling, still understood.
pub struct TimeFormatter {
    name: String,
    can_auto_detect: bool,
    default_format_options: TimeSpanFormatOptions,
    fallback_language: String,
    languages: Vec<(String, TimeTextInfo)>,
    time_text_info: Option<TimeTextInfo>,
}

impl TimeFormatter {
    /// A formatter named `time`, with .NET's default options — units spelled
    /// out, "less than 1 second" for anything below a second, every non-zero
    /// unit from days down to seconds — and English as the fallback language.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            can_auto_detect: false,
            default_format_options: DEFAULT_FORMAT_OPTIONS,
            fallback_language: DEFAULT_FALLBACK_LANGUAGE.to_owned(),
            languages: Vec::new(),
            time_text_info: None,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The options a time format inherits whatever it does not name.
    ///
    /// .NET has two of these and this is both: the live one is the `static`
    /// `TimeSpanUtility.DefaultFormatOptions`, which `ToTimeParts` merges into
    /// every call, while the instance property `TimeFormatter.DefaultFormatOptions`
    /// is a copy the constructor takes of it and nothing ever reads. Here the
    /// live knob belongs to the formatter — see DESIGN.md, "Deliberate policy
    /// differences".
    pub fn default_format_options(&self) -> TimeSpanFormatOptions {
        self.default_format_options
    }

    /// Changes the options a time format inherits. Any group left empty is
    /// still filled in from [`ABSOLUTE_DEFAULTS`], which is the safeguard .NET
    /// keeps for exactly this.
    ///
    /// This is what setting .NET's `static
    /// TimeSpanUtility.DefaultFormatOptions` does, minus the process-wide
    /// reach: setting .NET's instance property of the same name changes
    /// nothing at all.
    pub fn set_default_format_options(&mut self, options: TimeSpanFormatOptions) {
        self.default_format_options = options;
    }

    /// The language used when none is shipped for the culture, `en` by default
    /// (.NET `FallbackLanguage`).
    pub fn fallback_language(&self) -> &str {
        &self.fallback_language
    }

    /// Changes the fallback language. The empty string turns the fallback off,
    /// which makes a culture with no [`TimeTextInfo`] an error instead — both
    /// as in .NET, which rejects any other language it has no text for.
    pub fn set_fallback_language(&mut self, language: &str) -> Result<(), UnknownLanguage> {
        if !language.is_empty() && self.language(language).is_none() {
            return Err(UnknownLanguage(language.to_owned()));
        }
        self.fallback_language = language.to_owned();
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

    /// Adds the text for a language, or replaces it, the counterpart of .NET's
    /// `CommonLanguagesTimeTextInfo.AddLanguage`.
    ///
    /// The name is a `TwoLetterISOLanguageName` and is lowercased, as .NET
    /// lowercases its dictionary key. A language added here wins over a
    /// shipped one of the same name — .NET looks in its custom dictionary
    /// first too — but it belongs to this formatter rather than to a static
    /// dictionary the whole process shares.
    ///
    /// It also *replaces* rather than throws. .NET's `AddLanguage` is
    /// `Dictionary.Add` on that static dictionary, into which `GetTimeTextInfo`
    /// also caches every shipped language it loads, so re-adding a name — or
    /// adding one that some earlier format call already used — throws
    /// `ArgumentException` there (probed: `AddLanguage("EN", …)` after any
    /// `{0:time:}` under `en`). A per-formatter dictionary has no such
    /// process-wide history to collide with, so nothing is gained by making
    /// the second call fail. DESIGN.md, "Deliberate policy differences".
    pub fn add_language(&mut self, two_letter_iso_language_name: &str, info: TimeTextInfo) {
        let code = two_letter_iso_language_name.to_ascii_lowercase();
        match self.languages.iter_mut().find(|(known, _)| *known == code) {
            Some((_, known)) => *known = info,
            None => self.languages.push((code, info)),
        }
    }

    /// The text for a language, custom ones first, .NET
    /// `CommonLanguagesTimeTextInfo.GetTimeTextInfo`.
    pub fn language(&self, two_letter_iso_language_name: &str) -> Option<&TimeTextInfo> {
        self.languages
            .iter()
            .find(|(known, _)| known == two_letter_iso_language_name)
            .map(|(_, info)| info)
            .or_else(|| common_language(two_letter_iso_language_name))
    }

    /// Uses this [`TimeTextInfo`] for every placeholder, whatever the culture,
    /// the counterpart of .NET's `Provider.GetFormat(typeof(TimeTextInfo))`.
    ///
    /// .NET asks the `IFormatProvider` of the call for a `TimeTextInfo` before
    /// it looks at any culture; a caller who passed one there passes it here.
    pub fn set_time_text_info(&mut self, info: Option<TimeTextInfo>) {
        self.time_text_info = info;
    }

    /// The [`TimeTextInfo`] set by [`set_time_text_info`](Self::set_time_text_info).
    pub fn time_text_info(&self) -> Option<&TimeTextInfo> {
        self.time_text_info.as_ref()
    }

    /// .NET `GetTimeParts`, minus the value conversion its caller has already
    /// done.
    fn time_parts(&self, info: &FormattingInfo<'_>, from_time: i64) -> Result<Vec<String>, Error> {
        let options = info.formatter_options()?.trim().to_owned();
        let format_text = match info.format() {
            Some(format) => raw_text(format, info)?,
            None => String::new(),
        };
        let format_text = format_text.trim();

        // In SmartFormat 2.x, the format could be included in the options,
        // with an empty format. Keeping compatibility with v2, there is no
        // reliable way to set a language as an option.
        let v2_compatibility = !options.is_empty() && format_text.is_empty();
        let formatting_options = if v2_compatibility {
            options.as_str()
        } else {
            format_text
        };

        let time_text_info = self.resolve_time_text_info(info, v2_compatibility)?;
        let time_span_format_options =
            parse_options(formatting_options).merge(self.default_format_options);

        utility::time_parts(
            from_time,
            time_span_format_options,
            time_text_info,
            info.culture(),
        )
        .map_err(|overflow| info.plain_error(&overflow.to_string(), info.error_position()))
    }

    /// .NET `GetTimeTextInfo`: the text of the language the options name, of
    /// the culture of the call, or of the fallback language.
    fn resolve_time_text_info(
        &self,
        info: &FormattingInfo<'_>,
        v2_compatibility: bool,
    ) -> Result<&TimeTextInfo, Error> {
        // See if the provider can give us a TimeTextInfo.
        if let Some(overridden) = &self.time_text_info {
            return Ok(overridden);
        }

        // Figure out the culture to use, then see if there is text for it.
        let named = if v2_compatibility {
            ""
        } else {
            info.formatter_options()?.trim()
        };
        let language = if named.is_empty() {
            let culture = info.culture().name;
            if culture.is_empty() {
                INVARIANT_LANGUAGE.to_owned()
            } else {
                two_letter_iso_language_name(culture)
            }
        } else {
            // .NET resolves the name through `CultureInfo.GetCultureInfo`,
            // which rejects a malformed one with a `CultureNotFoundException`
            // — a plain exception, so its message reaches the output bare.
            named_culture_language(named)
                .map_err(|message| info.plain_error(&message, info.error_position()))?
        };

        if let Some(text_info) = self.language(&language) {
            return Ok(text_info);
        }

        if self.fallback_language.is_empty() {
            // .NET quotes `FormatterOptions` as written — untrimmed, and the
            // empty string when the culture came from the call — and reports
            // the error at index 0.
            return Err(info.formatting_error_at_utf16(
                &format!(
                    "TimeTextInfo could not be found for the given culture argument '{}'.",
                    info.formatter_options()?
                ),
                0,
            ));
        }

        Ok(self
            .language(&self.fallback_language)
            .expect("the fallback language is checked when it is set"))
    }
}

impl Default for TimeFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TimeFormatter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeFormatter")
            .field("name", &self.name)
            .field("can_auto_detect", &self.can_auto_detect)
            .field("default_format_options", &self.default_format_options)
            .field("fallback_language", &self.fallback_language)
            .field("languages", &self.languages.len())
            .field("time_text_info", &self.time_text_info.is_some())
            .finish()
    }
}

impl Formatter for TimeFormatter {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // Auto-detection calls just return a failure to evaluate.
        if info.placeholder().formatter_name.is_empty() {
            return Ok(false);
        }

        let current = info.current();
        let from_time = match current {
            Value::TimeSpan(span) => utility::ticks(span),
            // .NET subtracts the two after converting both to UTC; a
            // `jiff::civil::DateTime` carries no zone, so this is the plain
            // difference — the same answer whenever the two moments share a
            // UTC offset.
            Value::DateTime(date_time) => utility::ticks(
                &info
                    .settings()
                    .now_or_system_clock()
                    .duration_since(*date_time),
            ),
            other => return Err(unsupported_type(info, other)),
        };

        let time_parts = self.time_parts(info, from_time)?;

        // Now we have to check for a nested format. That is the one needed for
        // the ListFormatter.
        if let Some(format) = info.format() {
            if format.has_nested() {
                // .NET removes the format meant for the TimeFormatter — the
                // first item, whatever it is — and hands the rest the parts to
                // format, usually through the `list` formatter. It removes the
                // item from the *parsed* format, which a caller who reuses a
                // parsed format sees again on the next call; we leave the
                // format alone and drop the item from a copy.
                let child = without_first_item(format);
                let parts = Value::List(time_parts.into_iter().map(Value::String).collect());
                info.format_as_child(&child, &parts)?;
                return Ok(true);
            }
        }

        info.write(&time_parts.join(" "));
        Ok(true)
    }
}

/// .NET `Format.RawText`, which is `Format.ToString()`: every item written out
/// as `AsSpan()` gives it — a literal with its escape sequences resolved, a
/// placeholder exactly as it stands in the template.
///
/// Resolving the escape sequences is where a sequence that resolves to nothing
/// is rejected, so `{0:time:h\ours}` fails here rather than quietly parsing no
/// options at all (probed against 3.6.1).
fn raw_text(format: &Format, info: &FormattingInfo<'_>) -> Result<String, Error> {
    let mut text = String::with_capacity(format.raw.len());
    for item in &format.items {
        match item {
            FormatItem::Literal(literal) => {
                if let Some(message) = &literal.escape_error {
                    return Err(Error::Escape {
                        message: message.clone(),
                        position: info.error_position(),
                    });
                }
                text.push_str(&literal.text);
            }
            FormatItem::Placeholder(placeholder) => text.push_str(&placeholder.raw),
        }
    }
    Ok(text)
}

/// The format with its first item dropped, .NET's `format.Items.RemoveAt(0)`.
///
/// Everything else about the format — the text it spans, and so every position
/// an error inside it is reported at — stays as it was, exactly as it does
/// there.
fn without_first_item(format: &Format) -> Format {
    let mut child = format.clone();
    if !child.items.is_empty() {
        child.items.remove(0);
    }
    child
}

/// .NET's `FormattingException` for a value the formatter cannot process.
///
/// The exception is built from `Format?.Items.FirstOrDefault()`, so the
/// template it quotes comes from the *first item of the placeholder's format*
/// — and a placeholder whose format is empty has no item, which leaves the
/// message quoting an empty line (probed against 3.6.1). The index is a
/// literal 0 either way.
fn unsupported_type(info: &FormattingInfo<'_>, value: &Value) -> Error {
    let base = match info.format() {
        Some(format) if !format.items.is_empty() => info.base_string(),
        _ => "",
    };
    let issue = format!(
        "'TimeFormatter' can only process types of TimeSpan, DateTime, DateTimeOffset, TimeOnly, but not '{}'",
        dotnet_type_name(value, "")
    );
    Error::Format {
        message: format!("Error parsing format string: {issue} at 0\n{base}\n^"),
        position: 0,
    }
}

/// A language no [`TimeTextInfo`] is filed under was set as the fallback
/// language (.NET `ArgumentException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLanguage(pub String);

impl std::fmt::Display for UnknownLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No TimeTextInfo found for language '{}'.", self.0)
    }
}

impl std::error::Error for UnknownLanguage {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions.Time/TimeFormatterTests.cs`, plus the
    //! cases probed against the pinned SmartFormat.Extensions.Time 3.6.1
    //! package.
    //!
    //! The `TimeOnly` and `DateTimeOffset` cases have no counterpart: this
    //! crate's `Value` has neither.

    use std::collections::BTreeMap;

    use jiff::SignedDuration;

    use super::*;
    use crate::extensions::list::ListFormatter;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    /// The .NET fixture's `GetArgs()`.
    fn args() -> Value {
        Value::List(vec![
            span(0, 0, 0, 0, 0),
            span(1, 1, 1, 1, 1),
            span(0, 2, 0, 2, 0),
            span(3, 0, 0, 3, 0),
            span(0, 0, 0, 0, 4),
            span(5, 0, 0, 0, 0),
        ])
    }

    fn span(days: i64, hours: i64, minutes: i64, seconds: i64, millis: i64) -> Value {
        Value::TimeSpan(
            SignedDuration::from_hours(days * 24 + hours)
                + SignedDuration::from_mins(minutes)
                + SignedDuration::from_secs(seconds)
                + SignedDuration::from_millis(millis),
        )
    }

    fn smart() -> SmartFormatter {
        smart_with(TimeFormatter::new())
    }

    fn smart_with(formatter: TimeFormatter) -> SmartFormatter {
        smart_at(formatter, None)
    }

    fn smart_at(formatter: TimeFormatter, now: Option<jiff::civil::DateTime>) -> SmartFormatter {
        // The .NET default of `FormatErrorAction.ThrowError` — our
        // `ErrorAction::Error` — so a formatting error surfaces in the tests.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            now,
            ..SmartSettings::default()
        });
        // Only the formatter under test, the `list` formatter its nested
        // formats use, and the always-last DefaultFormatter, so these tests
        // pin this formatter rather than the order of the default registry.
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(ListFormatter::new()));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    fn format(template: &str, culture: &str) -> String {
        format_with(&smart(), template, &args(), culture)
    }

    fn format_with(smart: &SmartFormatter, template: &str, args: &Value, culture: &str) -> String {
        smart
            .format_with_culture_name(template, args, culture)
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// The message of the error `template` raises, and its position.
    fn error_of(template: &str, culture: &str) -> (String, usize) {
        error_in(&smart(), template, &args(), culture)
    }

    fn error_in(
        smart: &SmartFormatter,
        template: &str,
        args: &Value,
        culture: &str,
    ) -> (String, usize) {
        match smart.format_with_culture_name(template, args, culture) {
            Ok(text) => panic!("{template:?} unexpectedly rendered {text:?}"),
            Err(Error::Format { message, position }) | Err(Error::Escape { message, position }) => {
                (message, position)
            }
            Err(other) => panic!("{template:?} failed with {other:?}"),
        }
    }

    #[test]
    fn the_formatter_is_named_time_and_never_auto_detects() {
        let formatter = TimeFormatter::new();
        assert_eq!(Formatter::name(&formatter), "time");
        assert!(!Formatter::can_auto_detect(&formatter));
        assert_eq!(formatter.fallback_language(), "en");
        assert_eq!(formatter.default_format_options(), DEFAULT_FORMAT_OPTIONS);
    }

    #[test]
    fn an_implicit_invocation_returns_false() {
        // The .NET test `Formatter_ReturnsFalse_For_Implicit_Invocation`: with
        // no formatter name there is nothing for this extension to do, so the
        // next one gets the placeholder — here the DefaultFormatter, which
        // writes the TimeSpan with its own 'c' format.
        let args = Value::List(vec![span(1, 1, 1, 1, 1)]);
        assert_eq!(
            format_with(&smart(), "{0}", &args, ""),
            "1.01:01:01.0010000"
        );
    }

    #[test]
    fn the_defaults_write_every_non_zero_unit_down_to_seconds() {
        // The .NET test `Test_Defaults`.
        for (template, culture, expected) in [
            ("{0:time:}", "en", "less than 1 second"),
            ("{1:time:}", "en", "1 day 1 hour 1 minute 1 second"),
            ("{2:time:}", "en", "2 hours 2 seconds"),
            ("{3:time:}", "en", "3 days 3 seconds"),
            ("{4:time:}", "en", "less than 1 second"),
            ("{5:time:}", "en", "5 days"),
            ("{0:time:}", "de", "weniger als 1 Sekunde"),
            ("{1:time:}", "de", "1 Tag 1 Stunde 1 Minute 1 Sekunde"),
            ("{2:time:}", "de", "2 Stunden 2 Sekunden"),
            ("{3:time:}", "de", "3 Tage 3 Sekunden"),
            ("{4:time:}", "de", "weniger als 1 Sekunde"),
            ("{5:time:}", "de", "5 Tage"),
        ] {
            assert_eq!(
                format(template, culture),
                expected,
                "{template} in {culture}"
            );
        }
    }

    #[test]
    fn the_options_pick_the_range_and_the_wording() {
        // The .NET test `Test_Options`, whose culture is the invariant one —
        // the language comes from the formatter options.
        for (template, expected) in [
            ("{0:time(en):noless}", "0 seconds"),
            ("{1:time(en):hours}", "25 hours"),
            ("{1:time(en):hours minutes}", "25 hours 1 minute"),
            ("{2:time(en):days milliseconds}", "2 hours 2 seconds"),
            ("{2:time(en):days milliseconds auto}", "2 hours 2 seconds"),
            ("{2:time(en):days milliseconds short}", "2 hours"),
            (
                "{2:time(en):days milliseconds fill}",
                "2 hours 0 minutes 2 seconds 0 milliseconds",
            ),
            (
                "{2:time(en):days milliseconds full}",
                "0 days 2 hours 0 minutes 2 seconds 0 milliseconds",
            ),
            ("{3:time(en):abbr}", "3d 3s"),
            ("{0:time(fr):noless}", "0 seconde"),
            ("{1:time(fr):hours}", "25 heures"),
            ("{1:time(fr):hours minutes}", "25 heures 1 minute"),
            ("{2:time(fr):days milliseconds}", "2 heures 2 secondes"),
            ("{2:time(fr):days milliseconds auto}", "2 heures 2 secondes"),
            ("{2:time(fr):days milliseconds short}", "2 heures"),
            (
                "{2:time(fr):days milliseconds fill}",
                "2 heures 0 minute 2 secondes 0 milliseconde",
            ),
            (
                "{2:time(fr):days milliseconds full}",
                "0 jour 2 heures 0 minute 2 secondes 0 milliseconde",
            ),
            ("{3:time(fr):abbr}", "3j 3s"),
        ] {
            assert_eq!(format(template, ""), expected, "{template}");
        }
    }

    #[test]
    fn a_nested_format_gets_the_parts_as_a_list() {
        // The .NET test `NestedListFormatTest`. The leading space is load
        // bearing: the first item of the format is dropped, and without it
        // that item is the nested placeholder itself.
        for (template, expected) in [
            ("{0:time: {:list:|, | and }}", "less than 1 second"),
            (
                "{1:time: {:list:|, | and }}",
                "1 day, 1 hour, 1 minute and 1 second",
            ),
            ("{2:time: {:list:|, | and }}", "2 hours and 2 seconds"),
            ("{3:time: {:list:|, | and }}", "3 days and 3 seconds"),
            ("{4:time: {:list:|, | and }}", "less than 1 second"),
            ("{5:time: {:list:|, | and }}", "5 days"),
        ] {
            assert_eq!(format(template, "en"), expected, "{template}");
        }
    }

    #[test]
    fn the_first_item_of_a_nested_format_is_dropped() {
        // Probed against 3.6.1: whatever the first item is, it goes — even
        // when it is the placeholder that was supposed to write the parts.
        assert_eq!(format("{1:time:{:list:|, | and }}", "en"), "");
        // The options are still read from the whole format text, and what
        // follows the first item is kept.
        assert_eq!(format("{1:time:hours {:list:|, | and }}", "en"), "25 hours");
        assert_eq!(
            format("{1:time: {:list:|, | and } hours}", "en"),
            "25 hours hours"
        );
        assert_eq!(
            format("{1:time: {:list:|, | and }Z}", "en"),
            "1 day, 1 hour, 1 minute and 1 secondZ"
        );
    }

    #[test]
    fn a_parsed_format_is_not_consumed_by_rendering_it() {
        // A divergence, and the good kind: .NET removes the item from the
        // parsed format itself, so rendering the same parsed format twice
        // writes something different every time (probed: the second call
        // writes nothing at all, the third the unnested parts). We copy.
        let smart = smart();
        let format = smart.parse("{1:time: {:list:|, | and }}").expect("parses");
        let args = args();
        for _ in 0..3 {
            assert_eq!(
                smart
                    .format_parsed_with_culture_name(&format, &args, "en")
                    .expect("renders"),
                "1 day, 1 hour, 1 minute and 1 second"
            );
        }
    }

    #[test]
    fn the_v2_spelling_puts_the_options_in_the_parens() {
        // The .NET tests use "{0:time(abbr hours noless)}" alongside
        // "{0:time:abbr hours noless:}"; both are still understood.
        let args = Value::List(vec![span(1, 1, 0, 0, 0)]);
        let smart = smart();
        assert_eq!(
            format_with(&smart, "{0:time(abbr hours noless)}", &args, "en"),
            "25h"
        );
        assert_eq!(
            format_with(&smart, "{0:time:abbr hours noless:}", &args, "en"),
            "25h"
        );
        // With options *and* an empty format, the options are the format and
        // the culture comes from the call — so this is German, not English.
        assert_eq!(
            format_with(&smart, "{0:time(en):}", &args, "de"),
            "1 Tag 1 Stunde"
        );
        // Whitespace only is an empty format too.
        assert_eq!(
            format_with(&smart, "{0:time(en):   }", &args, "de"),
            "1 Tag 1 Stunde"
        );
        // With a format of its own, the options are the culture again.
        assert_eq!(
            format_with(&smart, "{0:time(en):noless}", &args, "de"),
            "1 day 1 hour"
        );
    }

    #[test]
    fn the_language_comes_from_the_options_then_the_culture() {
        let args = Value::List(vec![span(1, 1, 1, 1, 1)]);
        let smart = smart();
        // A culture name, not just a language, in any spelling of it.
        for culture in ["en", "EN", "en-US", "en_US", "en-GB"] {
            let template = format!("{{0:time({culture}):abbr}}");
            assert_eq!(format_with(&smart, &template, &args, "de"), "1d 1h 1m 1s");
        }
        assert_eq!(
            format_with(&smart, "{0:time(  de  ):abbr}", &args, ""),
            "1t 1h 1m 1s"
        );
        // No option: the culture of the call.
        assert_eq!(
            format_with(&smart, "{0:time:abbr}", &args, "de-DE"),
            "1t 1h 1m 1s"
        );
        // A culture with no TimeTextInfo falls back to English, and so does
        // the invariant culture, whose language .NET calls "iv".
        assert_eq!(
            format_with(&smart, "{0:time(nl):abbr}", &args, ""),
            "1d 1h 1m 1s"
        );
        assert_eq!(
            format_with(&smart, "{0:time:abbr}", &args, ""),
            "1d 1h 1m 1s"
        );
        assert_eq!(
            format_with(&smart, "{0:time:abbr}", &args, "ru"),
            "1d 1h 1m 1s"
        );
    }

    #[test]
    fn every_shipped_language_writes_its_own_words() {
        let args = Value::List(vec![span(1, 1, 1, 1, 1)]);
        let smart = smart();
        for (culture, expected) in [
            ("en", "0 weeks 1 day 1 hour 1 minute 1 second 1 millisecond"),
            (
                "de",
                "0 Wochen 1 Tag 1 Stunde 1 Minute 1 Sekunde 1 Millisekunde",
            ),
            (
                "es",
                "0 semanas 1 día 1 hora 1 minuto 1 segundo 1 milisegundo",
            ),
            (
                "fr",
                "0 semaine 1 jour 1 heure 1 minute 1 seconde 1 milliseconde",
            ),
            (
                "it",
                "0 settimane 1 giorno 1 ora 1 minuto 1 secondo 1 millisecondo",
            ),
            (
                "pt",
                "0 semanas 1 dia 1 hora 1 minuto 1 segundo 1 milissegundo",
            ),
        ] {
            let template = format!("{{0:time({culture}):weeks milliseconds full}}");
            assert_eq!(
                format_with(&smart, &template, &args, ""),
                expected,
                "{culture}"
            );
        }
    }

    #[test]
    fn every_shipped_language_has_its_own_less_than_text() {
        let args = Value::List(vec![span(0, 0, 0, 0, 0)]);
        let smart = smart();
        for (culture, spelled, abbreviated) in [
            ("en", "less than 1 week", "less than 1w"),
            ("de", "weniger als 1 Woche", "weniger als 1w"),
            ("es", "menos que 1 semana", "menos que 1sem"),
            ("fr", "moins que 1 semaine", "moins que 1sem"),
            ("it", "meno di 1 settimana", "meno di 1set"),
            ("pt", "menos do que 1 semana", "menos do que 1sem"),
        ] {
            assert_eq!(
                format_with(&smart, &format!("{{0:time({culture}):weeks}}"), &args, ""),
                spelled,
                "{culture}"
            );
            assert_eq!(
                format_with(
                    &smart,
                    &format!("{{0:time({culture}):abbr weeks}}"),
                    &args,
                    ""
                ),
                abbreviated,
                "{culture}"
            );
        }
    }

    #[test]
    fn a_date_time_is_the_span_between_it_and_now() {
        // The .NET tests `TimeSpanFromGivenTimeToCurrentTime` and
        // `DateTime_As_Argument_Should_FormatAsTimeSpan_ToLocaltime`, with
        // SystemTime.SetDateTime replaced by the pinned `now` setting.
        let now = jiff::civil::datetime(2025, 6, 15, 12, 0, 0, 0);
        let smart = smart_at(TimeFormatter::new(), Some(now));

        for hours in [0, 12, 23, -12, -23] {
            let then = now + SignedDuration::from_hours(hours);
            let args = Value::List(vec![Value::DateTime(then)]);
            // The difference to the current time, which is negative for a
            // moment in the future.
            assert_eq!(
                format_with(&smart, "{0:time:abbr hours noless:}", &args, "en"),
                format!("{}h", -hours)
            );
            // The v2 spelling agrees, and so does the equivalent TimeSpan.
            assert_eq!(
                format_with(&smart, "{0:time(abbr hours noless)}", &args, "en"),
                format!("{}h", -hours)
            );
            let span = Value::List(vec![Value::TimeSpan(now.duration_since(then))]);
            assert_eq!(
                format_with(&smart, "{0:time:abbr hours noless:}", &span, "en"),
                format!("{}h", -hours)
            );
        }

        let five_days_ago = now - SignedDuration::from_hours(5 * 24);
        let args = Value::List(vec![Value::DateTime(five_days_ago)]);
        assert_eq!(
            format_with(&smart, "{0:time(en):abbr days noless:}", &args, ""),
            "5d"
        );
    }

    #[test]
    fn a_value_that_is_not_a_duration_is_an_error() {
        // The .NET test `Explicit_Formatter_With_UnsupportedArgType_Should_Throw`.
        let smart = smart();
        for (value, quoted) in [
            (Value::from("hello"), "System.String"),
            (Value::from(42i64), "System.Int64"),
            (Value::from(1.5f64), "System.Double"),
            (Value::from(true), "System.Boolean"),
            (Value::Null, ""),
            (Value::List(vec![]), "System.Object[]"),
            (
                Value::Map(BTreeMap::new()),
                "System.Collections.Generic.Dictionary`2[System.String,System.Object]",
            ),
        ] {
            let args = Value::List(vec![value]);
            let (message, position) = error_in(&smart, "ab {0:time:hours}", &args, "");
            assert_eq!(
                message,
                format!(
                    "Error parsing format string: 'TimeFormatter' can only process types of TimeSpan, DateTime, DateTimeOffset, TimeOnly, but not '{quoted}' at 0\nab {{0:time:hours}}\n^"
                )
            );
            assert_eq!(position, 0);
        }
        // With an empty format there is no item to take the template from, so
        // .NET quotes nothing at all.
        let args = Value::List(vec![Value::from("hello")]);
        let (message, _) = error_in(&smart, "ab {0:time:}", &args, "");
        assert_eq!(
            message,
            "Error parsing format string: 'TimeFormatter' can only process types of TimeSpan, DateTime, DateTimeOffset, TimeOnly, but not 'System.String' at 0\n\n^"
        );
    }

    #[test]
    fn a_culture_name_dotnet_rejects_is_an_error() {
        let (message, position) = error_of("ab {0:time(xx-YY-):hours}", "");
        assert_eq!(
            message,
            "Culture is not supported. (Parameter 'name')\nxx-yy- is an invalid culture identifier."
        );
        // Reported where the evaluator positions a plain exception: the start
        // of the placeholder's format.
        assert_eq!(position, 19);
        // With an empty format the options are a format, not a culture, so
        // there is nothing to reject.
        assert_eq!(
            format("{1:time(xx-YY-):}", ""),
            "1 day 1 hour 1 minute 1 second"
        );
    }

    #[test]
    fn turning_the_fallback_language_off_makes_an_unknown_culture_an_error() {
        // The .NET tests `UseTimeFormatter_WithUnsupportedLanguage` and
        // `UseTimeFormatter_With_Unimplemented_FallbackLanguage`.
        let mut formatter = TimeFormatter::new();
        formatter
            .set_fallback_language("")
            .expect("empty is allowed");
        let smart = smart_with(formatter);

        for (template, quoted) in [
            ("{1:time(nl):noless}", "nl"),
            ("{1:time( nl ):noless}", " nl "),
            ("{1:time:noless}", ""),
        ] {
            let (message, position) = error_in(&smart, template, &args(), "");
            assert_eq!(
                message,
                format!(
                    "Error parsing format string: TimeTextInfo could not be found for the given culture argument '{quoted}'. at 0\n{template}\n^"
                )
            );
            assert_eq!(position, 0);
        }
        // A culture that does have text is unaffected.
        assert_eq!(
            format_with(&smart, "{1:time:hours}", &args(), "de-DE"),
            "25 Stunden"
        );
    }

    #[test]
    fn an_unknown_fallback_language_is_rejected() {
        // The .NET test `Setting_Unknown_FallbackLanguage_Should_Throw`.
        let mut formatter = TimeFormatter::new();
        let error = formatter
            .set_fallback_language("something-not-existing")
            .expect_err("no text for that language");
        assert_eq!(
            error.to_string(),
            "No TimeTextInfo found for language 'something-not-existing'."
        );
        assert_eq!(formatter.fallback_language(), "en");
        // Every shipped language is allowed, and so is the empty string.
        for language in common_languages() {
            formatter
                .set_fallback_language(language)
                .unwrap_or_else(|_| panic!("{language} is shipped"));
        }
    }

    #[test]
    fn a_custom_language_wins_over_a_shipped_one() {
        // The counterpart of .NET's CommonLanguagesTimeTextInfo.AddLanguage,
        // which is a static dictionary and whose `Add` throws on a name
        // already in it — including one an earlier format call cached there.
        // This one is the formatter's own and replaces instead; DESIGN.md,
        // "Deliberate policy differences".
        let mut formatter = TimeFormatter::new();
        let mut pirate = common_language("en").expect("English is shipped").clone();
        pirate.day = vec!["{0} sun".to_owned(), "{0} suns".to_owned()];
        formatter.add_language("EN", pirate);
        let smart = smart_with(formatter);
        assert_eq!(format_with(&smart, "{5:time:}", &args(), "en"), "5 suns");
    }

    #[test]
    fn a_time_text_info_set_by_hand_wins_over_every_culture() {
        let mut formatter = TimeFormatter::new();
        formatter.set_time_text_info(Some(
            common_language("fr").expect("French is shipped").clone(),
        ));
        let smart = smart_with(formatter);
        assert_eq!(
            format_with(&smart, "{5:time(de):}", &args(), "en"),
            "5 jours"
        );
    }

    #[test]
    fn the_default_options_can_be_changed() {
        // .NET's `DefaultFormatOptions_Can_Be_Set` only asserts that its
        // instance property round-trips, because that property is inert:
        // `GetTimeParts` never reads it, and the options a call merges come
        // from the *static* `TimeSpanUtility.DefaultFormatOptions`. The
        // rendering assertions below are that static's behaviour, probed with
        // `TimeSpanUtility.DefaultFormatOptions = RangeDays` on 3.6.1, which
        // this formatter's field stands for; DESIGN.md, "Deliberate policy
        // differences".
        let mut formatter = TimeFormatter::new();
        formatter.set_default_format_options(TimeSpanFormatOptions::RANGE_DAYS);
        assert_eq!(
            formatter.default_format_options(),
            TimeSpanFormatOptions::RANGE_DAYS
        );
        let smart = smart_with(formatter);
        // The groups the new options leave empty still come from the absolute
        // defaults, so this is days only, spelled out, with "less than".
        assert_eq!(format_with(&smart, "{1:time:}", &args(), "en"), "1 day");
        assert_eq!(
            format_with(&smart, "{4:time:}", &args(), "en"),
            "less than 1 day"
        );
        // What the format names still wins.
        assert_eq!(
            format_with(&smart, "{1:time:hours}", &args(), "en"),
            "25 hours"
        );
    }

    #[test]
    fn an_escape_sequence_in_the_format_is_resolved() {
        // Reading the format text is where .NET resolves the escapes, so one
        // that resolves to nothing fails the placeholder (probed).
        let (message, _) = error_of(r"{1:time:h\ours}", "en");
        assert_eq!(message, r#"Unrecognized escape sequence "\o" in literal."#);
        // A sequence that does resolve is just text, and no keyword.
        assert_eq!(
            format(r"{1:time:\n}", "en"),
            "1 day 1 hour 1 minute 1 second"
        );
    }

    #[test]
    fn the_alignment_pads_the_whole_text() {
        assert_eq!(
            format("[{1,20:time:hours}]", "en"),
            "[            25 hours]"
        );
        assert_eq!(
            format("[{1,-20:time:hours}]", "en"),
            "[25 hours            ]"
        );
    }

    #[test]
    fn the_other_formatters_see_the_constant_format() {
        // Everything .NET reaches through `TimeSpan.ToString()`: the default
        // formatter's own output, the text `choose` matches its options
        // against, and the text `ismatch` runs its pattern over. All three
        // probed against 3.6.1 with the default registry, which is what this
        // test builds — the `time` formatter is not even needed for them.
        let smart = SmartFormatter::default();
        let args = Value::List(vec![span(1, 1, 1, 1, 1)]);
        assert_eq!(smart.format("{0}", &args).unwrap(), "1.01:01:01.0010000");
        // The colons are written as escape sequences because the parser ends
        // the formatter options at the first bare one — as .NET's does.
        assert_eq!(
            smart
                .format(r"{0:choose(1.01\u003a01\u003a01.0010000|other):a|b}", &args)
                .unwrap(),
            "a"
        );
        // A value no option spells quotes the same text back.
        let (message, _) = match smart.format("{0:choose(nope|other):a|b}", &args) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("unexpected {other:?}"),
        };
        assert!(
            message.contains(r#""1.01:01:01.0010000" is not a valid choice"#),
            "{message}"
        );
        #[cfg(feature = "regex-formatters")]
        {
            assert_eq!(
                smart.format("{0:ismatch(^1):yes|no}", &args).unwrap(),
                "yes"
            );
            assert_eq!(smart.format("{0:ismatch(^2):yes|no}", &args).unwrap(), "no");
            let negative = Value::List(vec![Value::TimeSpan(-SignedDuration::from_hours(1))]);
            assert_eq!(
                smart.format("{0:ismatch(^-):yes|no}", &negative).unwrap(),
                "yes"
            );
        }
    }

    #[test]
    fn a_duration_too_large_to_round_overflows() {
        let args = Value::List(vec![Value::TimeSpan(utility::duration(i64::MIN))]);
        let (message, position) = error_in(&smart(), "{0:time:}", &args, "en");
        assert_eq!(message, OVERFLOW_MESSAGE);
        assert_eq!(position, 8);
    }
}
