//! The localized text a time format is written with, ported from
//! `SmartFormat.Extensions.Time/Utilities/TimeTextInfo.cs`,
//! `Utilities/CommonLanguagesTimeTextInfo.cs` and the six `Resources/*.json`
//! files those load.
//!
//! The .NET package ships the languages as embedded JSON and deserializes them
//! at first use. Here they are static tables, transcribed string for string,
//! and built into a [`TimeTextInfo`] once per process.

use std::sync::OnceLock;

use crate::extensions::plural_rules::{get_plural_rule, PluralRule};
use crate::fmt::culture::CultureData;
use crate::fmt::number::{self, Number};

use super::options::TimeSpanFormatOptions;

/// The text used to write a `TimeSpan` in one language, a port of .NET's
/// `TimeTextInfo`.
///
/// Each unit carries the plural forms of its name, in the order
/// [`plural_rule`](Self::plural_rule) indexes them, as a composite format
/// string whose `{0}` is the value: `["{0} day", "{0} days"]`. A unit with a
/// single form is never pluralized, which is how the abbreviations get away
/// with one entry.
///
/// The fields are public because .NET's are settable properties, and a struct
/// literal is this port's counterpart of .NET's object initializer:
///
/// ```
/// use smartformat::extensions::plural_rules::get_plural_rule;
/// use smartformat::extensions::time::TimeTextInfo;
///
/// let dutch = TimeTextInfo {
///     plural_rule: get_plural_rule("nl").expect("a rule for Dutch"),
///     week: vec!["{0} week".to_owned(), "{0} weken".to_owned()],
///     day: vec!["{0} dag".to_owned(), "{0} dagen".to_owned()],
///     hour: vec!["{0} uur".to_owned()],
///     minute: vec!["{0} minuut".to_owned(), "{0} minuten".to_owned()],
///     second: vec!["{0} seconde".to_owned(), "{0} seconden".to_owned()],
///     millisecond: vec!["{0} milliseconde".to_owned(), "{0} milliseconden".to_owned()],
///     w: vec!["{0}w".to_owned()],
///     d: vec!["{0}d".to_owned()],
///     h: vec!["{0}u".to_owned()],
///     m: vec!["{0}m".to_owned()],
///     s: vec!["{0}s".to_owned()],
///     ms: vec!["{0}ms".to_owned()],
///     less_than: "minder dan {0}".to_owned(),
/// };
/// # let _ = dutch;
/// ```
#[derive(Debug, Clone)]
pub struct TimeTextInfo {
    /// The rule that picks a plural form (.NET `PluralRule`, which is a
    /// `PluralRules.PluralRuleDelegate`).
    pub plural_rule: PluralRule,
    /// Plural text for "week" (.NET `Ptxt_week`).
    pub week: Vec<String>,
    /// Plural text for "day" (.NET `Ptxt_day`).
    pub day: Vec<String>,
    /// Plural text for "hour" (.NET `Ptxt_hour`).
    pub hour: Vec<String>,
    /// Plural text for "minute" (.NET `Ptxt_minute`).
    pub minute: Vec<String>,
    /// Plural text for "second" (.NET `Ptxt_second`).
    pub second: Vec<String>,
    /// Plural text for "millisecond" (.NET `Ptxt_millisecond`).
    pub millisecond: Vec<String>,
    /// Abbreviated plural text for "week" (.NET `Ptxt_w`).
    pub w: Vec<String>,
    /// Abbreviated plural text for "day" (.NET `Ptxt_d`).
    pub d: Vec<String>,
    /// Abbreviated plural text for "hour" (.NET `Ptxt_h`).
    pub h: Vec<String>,
    /// Abbreviated plural text for "minute" (.NET `Ptxt_m`).
    pub m: Vec<String>,
    /// Abbreviated plural text for "second" (.NET `Ptxt_s`).
    pub s: Vec<String>,
    /// Abbreviated plural text for "millisecond" (.NET `Ptxt_ms`).
    pub ms: Vec<String>,
    /// The "less than" wrapper, whose `{0}` is a unit text
    /// (.NET `Ptxt_lessThan`).
    pub less_than: String,
}

impl TimeTextInfo {
    /// The text for one unit and value, .NET `GetUnitText`.
    ///
    /// `unit` is one of the range flags; anything else is the empty string, as
    /// in .NET. The empty string is also what a plural rule that indexes past
    /// the forms it was given produces — .NET throws an
    /// `IndexOutOfRangeException` there, which no built-in language can reach,
    /// and a caller with a custom [`TimeTextInfo`] whose rule and forms
    /// disagree gets the unit left out rather than a panic.
    ///
    /// The value is written with `culture`, where .NET's `string.Format` uses
    /// the *thread* culture. It only shows on a negative value in a culture
    /// whose negative sign is not `-`.
    pub fn unit_text(
        &self,
        unit: TimeSpanFormatOptions,
        value: i32,
        abbr: bool,
        culture: &CultureData,
    ) -> String {
        let forms = match (unit, abbr) {
            (TimeSpanFormatOptions::RANGE_WEEKS, false) => &self.week,
            (TimeSpanFormatOptions::RANGE_WEEKS, true) => &self.w,
            (TimeSpanFormatOptions::RANGE_DAYS, false) => &self.day,
            (TimeSpanFormatOptions::RANGE_DAYS, true) => &self.d,
            (TimeSpanFormatOptions::RANGE_HOURS, false) => &self.hour,
            (TimeSpanFormatOptions::RANGE_HOURS, true) => &self.h,
            (TimeSpanFormatOptions::RANGE_MINUTES, false) => &self.minute,
            (TimeSpanFormatOptions::RANGE_MINUTES, true) => &self.m,
            (TimeSpanFormatOptions::RANGE_SECONDS, false) => &self.second,
            (TimeSpanFormatOptions::RANGE_SECONDS, true) => &self.s,
            (TimeSpanFormatOptions::RANGE_MILLI_SECONDS, false) => &self.millisecond,
            (TimeSpanFormatOptions::RANGE_MILLI_SECONDS, true) => &self.ms,
            // Not a range flag: unreachable from the formatter.
            _ => return String::new(),
        };
        self.value_text(value, forms, culture)
    }

    /// The "less than 1 unit" text for a unit text, .NET `GetLessThanText`.
    pub fn less_than_text(&self, minimum_value: &str) -> String {
        format_one(&self.less_than, minimum_value)
    }

    /// .NET `GetValue`: the plural form the rule picks, with the value
    /// substituted for `{0}`.
    fn value_text(&self, value: i32, forms: &[String], culture: &CultureData) -> String {
        // A single form is never pluralized, so a rule that cannot serve one
        // word is never asked.
        let index = if forms.len() == 1 {
            0
        } else {
            (self.plural_rule)(f64::from(value), forms.len())
        };
        let Ok(index) = usize::try_from(index) else {
            return String::new();
        };
        let Some(form) = forms.get(index) else {
            return String::new();
        };
        // The empty specifier is always valid, so this cannot fail.
        let value = number::format_number(Number::Int(i64::from(value)), "", culture)
            .unwrap_or_else(|_| value.to_string());
        format_one(form, &value)
    }
}

/// .NET `string.Format(template, argument)` for the one-argument composite
/// format strings a [`TimeTextInfo`] carries: `{0}` becomes `argument`, `{{`
/// and `}}` become single braces.
///
/// A format item .NET would reject — `{1}`, `{0:x}`, a lone brace — is left in
/// the output as written, where .NET throws a `FormatException`. No built-in
/// language has one, so this only softens what a custom [`TimeTextInfo`] does.
fn format_one(template: &str, argument: &str) -> String {
    let mut result = String::with_capacity(template.len() + argument.len());
    let mut rest = template;
    while let Some(at) = rest.find(['{', '}']) {
        let (before, from_brace) = rest.split_at(at);
        result.push_str(before);
        let mut chars = from_brace.chars();
        let brace = chars.next().expect("a brace was found");
        let after = chars.as_str();
        if let Some(tail) = after.strip_prefix(brace) {
            // "{{" or "}}", an escaped brace.
            result.push(brace);
            rest = tail;
        } else if brace == '{' {
            match after.strip_prefix("0}") {
                Some(tail) => {
                    result.push_str(argument);
                    rest = tail;
                }
                None => {
                    result.push(brace);
                    rest = after;
                }
            }
        } else {
            result.push(brace);
            rest = after;
        }
    }
    result.push_str(rest);
    result
}

/// The [`TimeTextInfo`] for a language SmartFormat.NET ships, .NET
/// `CommonLanguagesTimeTextInfo.GetTimeTextInfo`, or `None` for a language it
/// does not.
///
/// The name is a `TwoLetterISOLanguageName`, matched ordinally as .NET's
/// dictionary and resource lookup are: `en`, `de`, `es`, `fr`, `it`, `pt`.
///
/// .NET lets a program add more languages to a static dictionary; here that
/// belongs to a formatter instance instead —
/// [`TimeFormatter::add_language`](super::TimeFormatter::add_language).
pub fn common_language(two_letter_iso_language_name: &str) -> Option<&'static TimeTextInfo> {
    static COMMON: OnceLock<Vec<(&'static str, TimeTextInfo)>> = OnceLock::new();
    COMMON
        .get_or_init(|| {
            LANGUAGES
                .iter()
                .map(|language| (language.code, language.build()))
                .collect()
        })
        .iter()
        .find(|(code, _)| *code == two_letter_iso_language_name)
        .map(|(_, info)| info)
}

/// The two-letter names of the languages [`common_language`] knows, in the
/// order the package lists their resource files.
pub fn common_languages() -> impl Iterator<Item = &'static str> {
    LANGUAGES.iter().map(|language| language.code)
}

/// One `Resources/<code>.json` file, field for field.
struct LanguageData {
    code: &'static str,
    plural_rule: &'static str,
    week: &'static [&'static str],
    day: &'static [&'static str],
    hour: &'static [&'static str],
    minute: &'static [&'static str],
    second: &'static [&'static str],
    millisecond: &'static [&'static str],
    w: &'static [&'static str],
    d: &'static [&'static str],
    h: &'static [&'static str],
    m: &'static [&'static str],
    s: &'static [&'static str],
    ms: &'static [&'static str],
    less_than: &'static str,
}

impl LanguageData {
    fn build(&self) -> TimeTextInfo {
        TimeTextInfo {
            plural_rule: get_plural_rule(self.plural_rule)
                .expect("every shipped language names a rule of the plural table"),
            week: owned(self.week),
            day: owned(self.day),
            hour: owned(self.hour),
            minute: owned(self.minute),
            second: owned(self.second),
            millisecond: owned(self.millisecond),
            w: owned(self.w),
            d: owned(self.d),
            h: owned(self.h),
            m: owned(self.m),
            s: owned(self.s),
            ms: owned(self.ms),
            less_than: self.less_than.to_owned(),
        }
    }
}

fn owned(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

/// The six languages of `SmartFormat.Extensions.Time/Resources`, transcribed.
static LANGUAGES: &[LanguageData] = &[
    // Resources/en.json
    LanguageData {
        code: "en",
        plural_rule: "en",
        week: &["{0} week", "{0} weeks"],
        day: &["{0} day", "{0} days"],
        hour: &["{0} hour", "{0} hours"],
        minute: &["{0} minute", "{0} minutes"],
        second: &["{0} second", "{0} seconds"],
        millisecond: &["{0} millisecond", "{0} milliseconds"],
        w: &["{0}w"],
        d: &["{0}d"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "less than {0}",
    },
    // Resources/de.json
    LanguageData {
        code: "de",
        plural_rule: "de",
        week: &["{0} Woche", "{0} Wochen"],
        day: &["{0} Tag", "{0} Tage"],
        hour: &["{0} Stunde", "{0} Stunden"],
        minute: &["{0} Minute", "{0} Minuten"],
        second: &["{0} Sekunde", "{0} Sekunden"],
        millisecond: &["{0} Millisekunde", "{0} Millisekunden"],
        w: &["{0}w"],
        d: &["{0}t"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "weniger als {0}",
    },
    // Resources/es.json
    LanguageData {
        code: "es",
        plural_rule: "es",
        week: &["{0} semana", "{0} semanas"],
        day: &["{0} día", "{0} días"],
        hour: &["{0} hora", "{0} horas"],
        minute: &["{0} minuto", "{0} minutos"],
        second: &["{0} segundo", "{0} segundos"],
        millisecond: &["{0} milisegundo", "{0} milisegundos"],
        w: &["{0}sem"],
        d: &["{0}d"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "menos que {0}",
    },
    // Resources/fr.json
    LanguageData {
        code: "fr",
        plural_rule: "fr",
        week: &["{0} semaine", "{0} semaines"],
        day: &["{0} jour", "{0} jours"],
        hour: &["{0} heure", "{0} heures"],
        minute: &["{0} minute", "{0} minutes"],
        second: &["{0} seconde", "{0} secondes"],
        millisecond: &["{0} milliseconde", "{0} millisecondes"],
        w: &["{0}sem"],
        d: &["{0}j"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "moins que {0}",
    },
    // Resources/it.json
    LanguageData {
        code: "it",
        plural_rule: "it",
        week: &["{0} settimana", "{0} settimane"],
        day: &["{0} giorno", "{0} giorni"],
        hour: &["{0} ora", "{0} ore"],
        minute: &["{0} minuto", "{0} minuti"],
        second: &["{0} secondo", "{0} secondi"],
        millisecond: &["{0} millisecondo", "{0} millisecondi"],
        w: &["{0}set"],
        d: &["{0}g"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "meno di {0}",
    },
    // Resources/pt.json
    LanguageData {
        code: "pt",
        plural_rule: "pt",
        week: &["{0} semana", "{0} semanas"],
        day: &["{0} dia", "{0} dias"],
        hour: &["{0} hora", "{0} horas"],
        minute: &["{0} minuto", "{0} minutos"],
        second: &["{0} segundo", "{0} segundos"],
        millisecond: &["{0} milissegundo", "{0} milissegundos"],
        w: &["{0}sem"],
        d: &["{0}d"],
        h: &["{0}h"],
        m: &["{0}m"],
        s: &["{0}s"],
        ms: &["{0}ms"],
        less_than: "menos do que {0}",
    },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::time::options::TimeSpanFormatOptions as O;
    use crate::fmt::culture;

    fn en() -> &'static TimeTextInfo {
        common_language("en").expect("English is shipped")
    }

    #[test]
    fn the_six_shipped_languages_are_there_and_nothing_else() {
        assert_eq!(
            common_languages().collect::<Vec<_>>(),
            ["en", "de", "es", "fr", "it", "pt"]
        );
        for code in common_languages() {
            assert!(common_language(code).is_some(), "{code}");
        }
        // The lookup is ordinal, like .NET's dictionary and resource names.
        assert!(common_language("EN").is_none());
        assert!(common_language("nl").is_none());
        assert!(common_language("").is_none());
    }

    #[test]
    fn unit_text_pluralizes_and_abbreviates() {
        let inv = culture::invariant();
        let en = en();
        assert_eq!(en.unit_text(O::RANGE_DAYS, 1, false, inv), "1 day");
        assert_eq!(en.unit_text(O::RANGE_DAYS, 2, false, inv), "2 days");
        assert_eq!(en.unit_text(O::RANGE_DAYS, 0, false, inv), "0 days");
        assert_eq!(en.unit_text(O::RANGE_DAYS, -1, false, inv), "-1 days");
        // An abbreviation has one form, so it is never pluralized.
        assert_eq!(en.unit_text(O::RANGE_DAYS, 1, true, inv), "1d");
        assert_eq!(en.unit_text(O::RANGE_DAYS, 2, true, inv), "2d");
        // Not a range flag.
        assert_eq!(en.unit_text(O::ABBREVIATE, 1, false, inv), "");
    }

    #[test]
    fn french_calls_zero_singular() {
        let inv = culture::invariant();
        let fr = common_language("fr").expect("French is shipped");
        assert_eq!(fr.unit_text(O::RANGE_WEEKS, 0, false, inv), "0 semaine");
        assert_eq!(fr.unit_text(O::RANGE_WEEKS, 1, false, inv), "1 semaine");
        assert_eq!(fr.unit_text(O::RANGE_WEEKS, 2, false, inv), "2 semaines");
        // Probed against 3.6.1: a negative count is plural in French.
        assert_eq!(fr.unit_text(O::RANGE_WEEKS, -1, false, inv), "-1 semaines");
    }

    #[test]
    fn the_value_is_written_with_the_culture() {
        // .NET reads the thread culture here; we read the culture of the call.
        let sv = culture::get("sv").expect("Swedish is generated");
        assert_eq!(
            en().unit_text(O::RANGE_DAYS, -1, false, sv),
            "\u{2212}1 days"
        );
    }

    #[test]
    fn less_than_wraps_a_unit_text() {
        let inv = culture::invariant();
        let en = en();
        let unit = en.unit_text(O::RANGE_SECONDS, 1, false, inv);
        assert_eq!(en.less_than_text(&unit), "less than 1 second");
        let de = common_language("de").expect("German is shipped");
        let unit = de.unit_text(O::RANGE_SECONDS, 1, true, inv);
        assert_eq!(de.less_than_text(&unit), "weniger als 1s");
    }

    #[test]
    fn composite_formatting_substitutes_one_argument() {
        assert_eq!(format_one("{0} day", "3"), "3 day");
        assert_eq!(format_one("{{0}} {0}", "3"), "{0} 3");
        assert_eq!(format_one("no items", "3"), "no items");
        assert_eq!(format_one("", "3"), "");
        // Left as written, where .NET throws.
        assert_eq!(format_one("{1}", "3"), "{1}");
        assert_eq!(format_one("{0", "3"), "{0");
    }

    #[test]
    fn a_rule_that_indexes_past_its_forms_leaves_the_unit_out() {
        let mut broken = en().clone();
        broken.plural_rule = |_, _| 7;
        assert_eq!(
            broken.unit_text(O::RANGE_DAYS, 1, false, culture::invariant()),
            ""
        );
        broken.plural_rule = |_, _| -1;
        assert_eq!(
            broken.unit_text(O::RANGE_DAYS, 1, false, culture::invariant()),
            ""
        );
    }
}
