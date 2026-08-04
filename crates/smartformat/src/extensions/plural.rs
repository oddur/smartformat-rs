//! Port of SmartFormat.NET's `PluralLocalizationFormatter`.
//!
//! Ported from `src/SmartFormat/Extensions/PluralLocalizationFormatter.cs` and
//! `src/SmartFormat/Extensions/CustomPluralRuleProvider.cs`.
//!
//! `{0:plural:item|items}` renders one of the `|`-separated words of the
//! format, picked by the pluralization rule of the language and by the value:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! // Registered by default, ahead of `cond` and `choose`.
//! let smart = SmartFormatter::default();
//!
//! let template = "There {0:plural:is|are} {0} {0:plural:item|items} remaining";
//! let one = Value::List(vec![Value::Int(1)]);
//! let two = Value::List(vec![Value::Int(2)]);
//! assert_eq!(smart.format(template, &one).unwrap(), "There is 1 item remaining");
//! assert_eq!(smart.format(template, &two).unwrap(), "There are 2 items remaining");
//! ```
//!
//! The language is chosen the way .NET chooses it: the culture named in the
//! formatter options (`{0:plural(fr):…}`) wins, then the culture of the format
//! call, and the invariant culture counts as English.

use std::fmt;

use crate::extensions::plural_rules::{get_plural_rule, PluralRule};
use crate::fmt::culture::{named_culture_language, two_letter_iso_language_name};
use crate::formatter::{Formatter, FormattingInfo};
use crate::value::{dotnet_type_name, Value};
use crate::Error;

use super::{split_format, split_part, InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The default formatter name, .NET `PluralLocalizationFormatter.Name`.
///
/// .NET 3.x has one name per formatter: the obsolete `Names` property once
/// held `"plural"`, `"p"` and `""`, but only `"plural"` selects the formatter
/// in 3.6.1.
const NAME: &str = "plural";

/// The language used when the culture is the invariant one, .NET
/// `PluralLocalizationFormatter.DefaultTwoLetterISOLanguageName`. .NET has no
/// rule for the invariant culture (whose `TwoLetterISOLanguageName` is `iv`),
/// so it falls back to English.
const DEFAULT_TWO_LETTER_ISO_LANGUAGE_NAME: &str = "en";

/// A pluralization rule supplied by the caller instead of by the language
/// table, the counterpart of .NET's `CustomPluralRuleProvider`.
pub type CustomPluralRule = dyn Fn(f64, usize) -> i32 + Send + Sync;

/// Picks a plural word by the value and the language, ported from .NET
/// `PluralLocalizationFormatter`.
///
/// The value is a number or a list — a list pluralizes by how many items it
/// has, so the same argument can feed a `plural` and a `list` placeholder.
pub struct PluralLocalizationFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
    custom_rule: Option<Box<CustomPluralRule>>,
}

impl PluralLocalizationFormatter {
    /// A formatter named `plural`, splitting on `|`, taking its language from
    /// the culture — the .NET defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: true,
            custom_rule: None,
        }
    }

    /// A formatter that ignores the language table and asks `rule` for the
    /// index of the plural word, .NET
    /// `Smart.Format(new CustomPluralRuleProvider(rule), …)`.
    ///
    /// .NET hangs the custom rule off the `IFormatProvider` of the call; we
    /// have no provider to hang it off, so it belongs to the formatter
    /// instance. The .NET precedence is kept: a culture named in the formatter
    /// options (`{0:plural(pl):…}`) still wins over the custom rule.
    ///
    /// ```
    /// use smartformat::extensions::plural::PluralLocalizationFormatter;
    /// use smartformat::extensions::plural_rules::get_plural_rule;
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let german = get_plural_rule("de").unwrap();
    /// let formatter = PluralLocalizationFormatter::with_custom_rule(german);
    ///
    /// let mut smart = SmartFormatter::default();
    /// // Ahead of the default `plural`, which would otherwise answer first.
    /// smart.formatters_mut().insert(0, Box::new(formatter));
    ///
    /// let args = Value::List(vec![Value::List(vec![Value::Null, Value::Null])]);
    /// assert_eq!(smart.format("{0:plural:Frau|Frauen}", &args).unwrap(), "Frauen");
    /// ```
    pub fn with_custom_rule(rule: impl Fn(f64, usize) -> i32 + Send + Sync + 'static) -> Self {
        Self {
            custom_rule: Some(Box::new(rule)),
            ..Self::new()
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the plural words are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output — `{0:plural:|is a person|.~|are {} people|.}`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether a placeholder that names no formatter may be pluralized.
    /// Defaults to `true`, as in .NET, where the documentation recommends
    /// turning it off: auto-detection makes `{0:one|many}` mean whichever
    /// auto-detecting formatter comes first.
    pub fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    pub fn set_can_auto_detect(&mut self, can_auto_detect: bool) {
        self.can_auto_detect = can_auto_detect;
    }

    /// The rule set by [`with_custom_rule`](Self::with_custom_rule), if any.
    pub fn custom_rule(&self) -> Option<&CustomPluralRule> {
        self.custom_rule.as_deref()
    }

    /// .NET `GetPluralRule`: the rule named by the formatter options, else the
    /// custom rule, else the rule of the culture the call was made with.
    fn plural_rule<'r>(&'r self, info: &FormattingInfo<'_>) -> Result<ResolvedRule<'r>, Error> {
        // .NET reads `FormatterOptions` here, so options holding an escape
        // sequence that resolves to nothing fail at this point.
        let options = info.formatter_options()?.trim();
        if !options.is_empty() {
            // .NET resolves the name through `CultureInfo.GetCultureInfo`,
            // which rejects a malformed one; the `CultureNotFoundException` it
            // throws is wrapped in a `FormattingException` reported at index 0.
            let language = named_culture_language(options)
                .map_err(|message| info.formatting_error_at_utf16(&message, 0))?;
            return Ok(ResolvedRule::Table(language_rule(&language, info)?));
        }

        if let Some(rule) = &self.custom_rule {
            return Ok(ResolvedRule::Custom(rule.as_ref()));
        }

        let culture = info.culture().name;
        let language = if culture.is_empty() {
            // The invariant culture has no pluralization rule.
            DEFAULT_TWO_LETTER_ISO_LANGUAGE_NAME.to_owned()
        } else {
            two_letter_iso_language_name(culture)
        };
        Ok(ResolvedRule::Table(language_rule(&language, info)?))
    }
}

impl Default for PluralLocalizationFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PluralLocalizationFormatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluralLocalizationFormatter")
            .field("name", &self.name)
            .field("split_char", &self.split_char)
            .field("can_auto_detect", &self.can_auto_detect)
            .field("custom_rule", &self.custom_rule.is_some())
            .finish()
    }
}

impl Formatter for PluralLocalizationFormatter {
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
        let Some(format) = info.format() else {
            return Ok(false);
        };
        let current = info.current();

        // Extract the plural words from the format string. .NET splits before
        // it counts them — and before it decides that auto-detection does not
        // apply — so a split that throws is the answer for every value.
        let plural_words = split_format(info, format, self.split_char)?;

        let use_auto_detection = info.placeholder().formatter_name.is_empty();

        // This extension requires at least two plural words for auto-detection.
        // Valid types for auto-detection are checked later.
        if use_auto_detection && plural_words.len() <= 1 {
            return Ok(false);
        }

        // Check whether the argument can be handled by this formatter: numbers
        // and lists, where a list pluralizes by how many items it has. That
        // lets one argument feed both a plural and a list placeholder:
        // `{0:plural:person is|people are} … {0:list:{}|, |, and}`.
        let value = match quantity(current) {
            Some(value) => value,
            None => {
                // Auto-detection calls just return a failure to evaluate.
                if use_auto_detection {
                    return Ok(false);
                }
                // The formatter was called by name, so this is an error. .NET
                // reports it at index 0 literally, so the caret sits at the
                // start of the template whatever the placeholder is.
                return Err(info.formatting_error_at_utf16(
                    &format!(
                        "Formatter named '{}' can format numbers and IEnumerables, but the argument was of type '{}'",
                        info.placeholder().formatter_name,
                        dotnet_type_name(current, "null")
                    ),
                    0,
                ));
            }
        };

        // Get the specific plural rule, or the default rule.
        let plural_count = plural_words.len();
        let plural_index = self.plural_rule(info)?.apply(value, plural_count);

        if plural_index < 0 || plural_count <= plural_index as usize {
            // .NET passes `pluralWords.Count - 1` as the index, which is a
            // count and not an offset into the template at all: the caret of
            // the message lands wherever that count happens to point. Probed:
            // the index stays 4 for `{0:plural:a|b|c|d|e}` however many
            // characters precede the placeholder.
            return Err(info.formatting_error_at_utf16(
                "Invalid number of plural parameters in PluralLocalizationFormatter",
                plural_count - 1,
            ));
        }

        // Output the selected word (allowing for nested formats).
        let plural_form = split_part(info, &plural_words, plural_index as usize)?;
        info.format_as_child(plural_form, current)?;
        Ok(true)
    }
}

/// The rule the formatter ends up using for one placeholder.
enum ResolvedRule<'a> {
    /// A rule from the language table.
    Table(PluralRule),
    /// The rule of a [`PluralLocalizationFormatter::with_custom_rule`]
    /// formatter (.NET `CustomPluralRuleProvider.GetPluralRule()`).
    Custom(&'a CustomPluralRule),
}

impl ResolvedRule<'_> {
    fn apply(&self, value: f64, plural_words_count: usize) -> i32 {
        match self {
            ResolvedRule::Table(rule) => rule(value, plural_words_count),
            ResolvedRule::Custom(rule) => rule(value, plural_words_count),
        }
    }
}

/// The rule for a language, or the error .NET's `PluralRules.GetPluralRule`
/// throws for a language it has no rule for.
fn language_rule(language: &str, info: &FormattingInfo<'_>) -> Result<PluralRule, Error> {
    get_plural_rule(language).ok_or_else(|| {
        // .NET throws a plain `ArgumentException` here, which the evaluator
        // catches and re-raises at the start of the placeholder's format. Being
        // a plain exception rather than a `FormattingException`, it carries no
        // `Error parsing format string: … at {index}` envelope in the output:
        // probed, `ErrorAction::OutputErrorInResult` writes the bare message.
        info.plain_error_here(&format!(
            "IsoLangToDelegate not found for {language} (Parameter 'twoLetterIsoLanguageName')"
        ))
    })
}

/// The number a value pluralizes by: a number as itself, a list by how many
/// items it holds (.NET `IEnumerable<object>.Count()`), and anything else not
/// at all.
fn quantity(value: &Value) -> Option<f64> {
    match value {
        // .NET converts through `decimal`, which holds every `long` and
        // `ulong` exactly; `f64` does not hold those above 2^53, so a huge
        // integer can pluralize by a neighbouring value. See DESIGN.md.
        Value::Int(value) => Some(*value as f64),
        Value::UInt(value) => Some(*value as f64),
        Value::Float(value) => to_decimal(*value),
        Value::List(items) => Some(items.len() as f64),
        // .NET excludes `bool` and `string` explicitly — a numeric string is
        // not pluralized (axuno/SmartFormat#345) — and everything else either
        // is not `IConvertible` or throws on the conversion.
        _ => None,
    }
}

/// `decimal.MaxValue` rounded up, which is 2^96: .NET's `Convert.ToDecimal`
/// overflows at exactly this magnitude.
const DECIMAL_OVERFLOW: f64 = 79_228_162_514_264_337_593_543_950_336.0;

/// Half of `decimal`'s smallest unit, 1e-28. Anything smaller rounds to zero,
/// and .NET rounds the half itself to zero too (to even).
const HALF_OF_SMALLEST_DECIMAL: f64 = 5e-29;

/// .NET `Convert.ToDecimal(double)`, which the formatter runs every value
/// through: the value rounded to 15 significant digits, or `None` where .NET
/// throws — an infinity, a NaN, or a magnitude `decimal` cannot hold, all of
/// which .NET reports as "the argument was of type 'System.Double'".
///
/// The rounding is not cosmetic: `0.9999999999999999` becomes `1`, and so
/// pluralizes as one.
fn to_decimal(value: f64) -> Option<f64> {
    if !value.is_finite() || value.abs() >= DECIMAL_OVERFLOW {
        return None;
    }

    // 15 significant digits, rounding ties to even, as .NET does.
    let rounded: f64 = format!("{value:.14e}").parse().ok()?;

    // `decimal` has at most 28 decimal places, so a smaller magnitude is zero
    // — and zero pluralizes differently from a small fraction.
    if rounded.abs() <= HALF_OF_SMALLEST_DECIMAL {
        return Some(0.0);
    }
    Some(rounded)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/PluralLocalizationFormatterTests.cs`,
    //! plus the cases a differential run against SmartFormat.NET 3.6.1 pinned.
    //!
    //! Error messages are asserted in full. The two that .NET raises as a
    //! `FormattingException` carry its `Error parsing format string: … at
    //! {index}` envelope, built here by `envelope`; the language lookup, which
    //! .NET raises as a plain `ArgumentException`, carries none.

    use std::collections::BTreeMap;

    use super::*;
    use crate::extensions::envelope;
    use crate::fmt::culture::{self, CultureData};
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    /// A culture that formats like the invariant one but is named `name`, so
    /// the language the formatter derives from it is the one under test. The
    /// generated culture table lands in a separate change; until then the
    /// numbers in these tests are invariant-formatted, which for the integers
    /// and the halves used here is what .NET prints too.
    fn culture(name: &'static str) -> CultureData {
        CultureData {
            name,
            ..culture::invariant().clone()
        }
    }

    fn smart_with(formatter: PluralLocalizationFormatter) -> SmartFormatter {
        // .NET's default error action is `ThrowError`, our `ErrorAction::Error`.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        });
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
        smart_with(PluralLocalizationFormatter::new())
    }

    fn args(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    /// A list of `count` items, the .NET `new string[count]` of the tests.
    fn list(count: usize) -> Value {
        Value::List(vec![Value::from("x"); count])
    }

    fn format_in(culture_name: &'static str, template: &str, value: Value) -> String {
        smart()
            .format_with_culture(template, &args([value]), &culture(culture_name))
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// .NET's `FormattingException.Message`, which
    /// `ErrorAction::OutputErrorInResult` writes into the result verbatim:
    /// the issue, the index it is reported at, then the template and a caret
    /// line. Probed against 3.6.1.
    fn error_of(template: &str, value: Value) -> (String, usize) {
        match smart().format(template, &args([value])) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = PluralLocalizationFormatter::new();
        assert_eq!(formatter.name(), "plural");
        assert!(Formatter::can_auto_detect(&formatter));
        assert_eq!(formatter.split_char(), '|');
        assert!(formatter.custom_rule().is_none());
        assert_eq!(
            PluralLocalizationFormatter::new().with_name("p").name(),
            "p"
        );
    }

    #[test]
    fn only_the_name_plural_selects_the_formatter() {
        // .NET 3.6.1 dropped the "p" and "" aliases the obsolete `Names`
        // property once held, and formatter names are case-sensitive.
        for template in ["{0:p:one|many}", "{0:PLURAL:one|many}"] {
            let error = smart().format(template, &args([Value::Int(2)]));
            assert!(error.is_err(), "{template}");
        }
        assert_eq!(
            format_in("en", "{0:plural:one|many}", Value::Int(2)),
            "many"
        );
    }

    // -- The .NET test fixture ---------------------------------------------

    #[test]
    fn test_default_and_english() {
        let template = "There {0:plural:is|are} {0} {0:plural:item|items} remaining";
        let cases: &[(Value, &str)] = &[
            (Value::Int(-1), "There are -1 items remaining"),
            (Value::Int(0), "There are 0 items remaining"),
            (Value::Float(0.5), "There are 0.5 items remaining"),
            (Value::Int(1), "There is 1 item remaining"),
            (Value::Float(1.5), "There are 1.5 items remaining"),
            (Value::Int(2), "There are 2 items remaining"),
            (Value::Int(11), "There are 11 items remaining"),
        ];
        for (value, expected) in cases {
            assert_eq!(&format_in("en-US", template, value.clone()), expected);
        }
    }

    #[test]
    fn english_unsigned() {
        let template = "There {0:plural(en):is|are} {0} {0:plural(en):item|items} remaining";
        let expected = [
            "There are 0 items remaining",
            "There is 1 item remaining",
            "There are 2 items remaining",
        ];
        for (index, expected) in expected.iter().enumerate() {
            assert_eq!(
                &format_in("", template, Value::UInt(index as u64)),
                expected
            );
        }
    }

    #[test]
    fn french_with_two_three_and_four_words() {
        let two = "{0:plural:{0} personne|{0} personnes}";
        for (value, expected) in [(0, "0 personne"), (1, "1 personne"), (2, "2 personnes")] {
            assert_eq!(format_in("fr", two, Value::Int(value)), expected);
        }
        assert_eq!(format_in("fr", two, Value::Int(50)), "50 personnes");

        let three = "{0:plural:pas de personne|une personne|{0} personnes}";
        for (value, expected) in [
            (0, "pas de personne"),
            (1, "une personne"),
            (2, "2 personnes"),
            (50, "50 personnes"),
        ] {
            assert_eq!(format_in("fr", three, Value::Int(value)), expected);
        }

        let four = "{0:plural:-|pas de personne|une personne|{0} personnes}";
        for (value, expected) in [
            (-1, "-"),
            (0, "pas de personne"),
            (1, "une personne"),
            (2, "2 personnes"),
            (50, "50 personnes"),
        ] {
            assert_eq!(format_in("fr", four, Value::Int(value)), expected);
        }
    }

    #[test]
    fn test_turkish() {
        let template = "Seçili {0:plural:nesneyi|nesneleri} silmek istiyor musunuz?";
        let cases: &[(Value, &str)] = &[
            (Value::Int(-1), "nesneleri"),
            (Value::Int(0), "nesneleri"),
            (Value::Float(0.5), "nesneleri"),
            (Value::Int(1), "nesneyi"),
            (Value::Float(1.5), "nesneleri"),
            (Value::Int(2), "nesneleri"),
            (Value::Int(11), "nesneleri"),
        ];
        for (value, expected) in cases {
            let expected = format!("Seçili {expected} silmek istiyor musunuz?");
            assert_eq!(format_in("tr-TR", template, value.clone()), expected);
        }
    }

    #[test]
    fn test_russian() {
        let template = "Я купил {0} {0:plural:банан|банана|бананов}.";
        let cases = [
            (0, "бананов"),
            (1, "банан"),
            (2, "банана"),
            (5, "бананов"),
            (11, "бананов"),
            (20, "бананов"),
            (21, "банан"),
            (22, "банана"),
            (25, "бананов"),
            (120, "бананов"),
            (121, "банан"),
            (122, "банана"),
            (125, "бананов"),
        ];
        for (value, word) in cases {
            let expected = format!("Я купил {value} {word}.");
            assert_eq!(format_in("ru-RU", template, Value::Int(value)), expected);
        }
        // The language, not the whole culture name, chooses the rule.
        assert_eq!(
            format_in("ru", template, Value::Int(21)),
            "Я купил 21 банан."
        );
    }

    #[test]
    fn test_czech() {
        let template =
            "{0:plural:Nemáte zprávu|Máte {} zprávu|Přišly Vám {} zprávy|Přišlo Vám {} zpráv}!";
        let cases = [
            (0, "Nemáte zprávu!"),
            (1, "Máte 1 zprávu!"),
            (2, "Přišly Vám 2 zprávy!"),
            (4, "Přišly Vám 4 zprávy!"),
            (5, "Přišlo Vám 5 zpráv!"),
            (6, "Přišlo Vám 6 zpráv!"),
        ];
        for (value, expected) in cases {
            assert_eq!(format_in("cs", template, Value::Int(value)), expected);
        }
    }

    #[test]
    fn test_polish() {
        let template = "{0} {0:plural:miesiąc|miesiące|miesięcy} temu";
        let cases = [
            (0, "miesięcy"),
            (1, "miesiąc"),
            (2, "miesiące"),
            (3, "miesiące"),
            (4, "miesiące"),
            (5, "miesięcy"),
            (9, "miesięcy"),
            (11, "miesięcy"),
            (14, "miesięcy"),
            (21, "miesięcy"),
            (22, "miesiące"),
            (24, "miesiące"),
            (25, "miesięcy"),
            (101, "miesięcy"),
            (102, "miesiące"),
            (105, "miesięcy"),
        ];
        for (value, word) in cases {
            let expected = format!("{value} {word} temu");
            assert_eq!(format_in("pl", template, Value::Int(value)), expected);
        }
    }

    #[test]
    fn test_icelandic() {
        // SmartFormat.NET treats Icelandic as one/other, which CLDR does not:
        // 21 is "hestur" there and "hestar" here.
        let template = "{0:plural:hestur|hestar}";
        assert_eq!(format_in("is", template, Value::Int(1)), "hestur");
        assert_eq!(format_in("is-IS", template, Value::Int(21)), "hestar");
    }

    #[test]
    fn test_arabic_with_six_forms() {
        let template = "{0:plural:zero|one|two|few|many|other}";
        let cases = [
            (0, "zero"),
            (1, "one"),
            (2, "two"),
            (3, "few"),
            (10, "few"),
            (11, "many"),
            (99, "many"),
            (100, "other"),
            (103, "few"),
        ];
        for (value, expected) in cases {
            assert_eq!(format_in("ar", template, Value::Int(value)), expected);
        }
    }

    #[test]
    fn test_japanese_is_singular() {
        // Japanese has no plural forms, so one word serves every value — and
        // one word is allowed because the formatter was named.
        let template = "リンゴを{0:plural(ja):{}個持っています。}";
        for value in [0, 1, 100] {
            let expected = format!("リンゴを{value}個持っています。");
            assert_eq!(format_in("en", template, Value::Int(value)), expected);
        }
        // The same through the culture of the call.
        assert_eq!(format_in("ja-JP", "{0:plural:{}個}", Value::Int(5)), "5個");
    }

    #[test]
    fn a_named_language_wins_over_the_culture() {
        let template = "{0} {0:plural(en):zero|one|many} {0:plural(pl):miesiąc|miesiące|miesięcy}";
        let cases = [
            (0, "0 zero miesięcy"),
            (1, "1 one miesiąc"),
            (2, "2 many miesiące"),
            (5, "5 many miesięcy"),
        ];
        for (value, expected) in cases {
            assert_eq!(format_in("fr", template, Value::Int(value)), expected);
        }
    }

    #[test]
    fn a_list_pluralizes_by_its_length() {
        let template = "{0:plural:zero|one|many}";
        for (count, expected) in [(0, "zero"), (1, "one"), (2, "many"), (5, "many")] {
            assert_eq!(format_in("en-US", template, list(count)), expected);
        }
    }

    #[test]
    fn signed_and_unsigned_numbers_are_all_numbers() {
        let template = "{0:plural(en):zero|one|many}";
        for value in [
            Value::Int(123),
            Value::Int(-123),
            Value::UInt(123),
            Value::Float(123.0),
        ] {
            assert_eq!(format_in("", template, value.clone()), "many", "{value:?}");
        }
    }

    #[test]
    fn a_custom_rule_replaces_the_language_table() {
        // .NET passes the rule through a `CustomPluralRuleProvider`; here it
        // belongs to the formatter instance.
        let cases: &[(&str, &str, usize, &str)] = &[
            ("de", "{0:plural:Frau|Frauen}", 2, "Frauen"),
            (
                "de",
                "{0:plural:Frau|Frauen|einige Frauen|viele Frauen}",
                4,
                "viele Frauen",
            ),
            ("en", "{0:plural:person|people}", 2, "people"),
            ("en", "{0:plural:person|people}", 1, "person"),
            (
                "fr",
                "{0:plural:pas de personne|une personne|plusieurs personnes}",
                0,
                "pas de personne",
            ),
            (
                "fr",
                "{0:plural:pas de personne|une personne|plusieurs personnes}",
                1,
                "une personne",
            ),
            (
                "fr",
                "{0:plural:pas de personne|une personne|deux personnes}",
                2,
                "deux personnes",
            ),
            (
                "fr",
                "{0:plural:pas de personne|une personne|deux personnes|plusieurs personnes}",
                3,
                "plusieurs personnes",
            ),
            (
                "fr",
                "{0:plural:une personne|deux personnes|plusieurs personnes|beaucoup de personnes}",
                3,
                "beaucoup de personnes",
            ),
        ];

        for (language, template, count, expected) in cases {
            let rule = get_plural_rule(language).unwrap();
            let smart = smart_with(PluralLocalizationFormatter::with_custom_rule(rule));
            let result = smart.format(template, &args([list(*count)])).unwrap();
            assert_eq!(&result, expected, "{language} {template}");
        }
    }

    #[test]
    fn a_named_language_wins_over_the_custom_rule() {
        // .NET reaches for the `CustomPluralRuleProvider` only when the
        // formatter options are empty.
        let smart = smart_with(PluralLocalizationFormatter::with_custom_rule(|_, _| 0));
        let args = args([Value::Int(5)]);
        assert_eq!(smart.format("{0:plural(ru):a|b|c}", &args).unwrap(), "c");
        assert_eq!(smart.format("{0:plural:a|b|c}", &args).unwrap(), "a");
    }

    #[test]
    fn a_custom_rule_may_be_any_closure() {
        // The .NET test that replaces the "en" rule globally, without the
        // global state: six words, chosen by magnitude.
        let rule = |value: f64, words: usize| {
            if words != 6 {
                return -1;
            }
            match value.abs() {
                value if value <= 0.0 => 0,
                value if value < 2.0 => 1,
                value if value < 3.0 => 2,
                value if value < 10.0 => 3,
                value if value < 20.0 => 4,
                _ => 5,
            }
        };
        let smart = smart_with(PluralLocalizationFormatter::with_custom_rule(rule));
        let template =
            "{0:plural:nobody|{} person|{} people|a couple of people|many people|a lot of people}";
        let cases = [
            (0, "nobody"),
            (1, "1 person"),
            (2, "2 people"),
            (5, "a couple of people"),
            (15, "many people"),
            (50, "a lot of people"),
        ];
        for (value, expected) in cases {
            let result = smart.format(template, &args([Value::Int(value)])).unwrap();
            assert_eq!(result, expected, "{value}");
        }

        // A rule that returns -1 is .NET's "invalid number of parameters".
        let (message, index) = match smart.format("{0:plural:a|b}", &args([Value::Int(1)])) {
            Err(Error::Format { message, position }) => (message, position),
            other => panic!("expected an error, got {other:?}"),
        };
        assert_eq!(
            message,
            envelope(
                "{0:plural:a|b}",
                "Invalid number of plural parameters in PluralLocalizationFormatter",
                1
            )
        );
        assert_eq!(index, 1);
    }

    /// The `new { People = new List<object> { … } }` of the .NET tests, whose
    /// `People.Count` those templates pluralize by. We have no reflection
    /// source, so the count is a value of its own.
    fn people(count: i64) -> Value {
        let mut people = BTreeMap::new();
        people.insert("Count".to_owned(), Value::Int(count));
        let mut data = BTreeMap::new();
        data.insert("People".to_owned(), Value::Map(people));
        Value::Map(data)
    }

    #[test]
    fn nested_placeholders_pluralize() {
        let template = "There {People.Count:plural:is a person.|are {} people.}";
        for (count, expected) in [(1, "There is a person."), (2, "There are 2 people.")] {
            let result = smart()
                .format_with_culture(template, &people(count), &culture("en"))
                .unwrap();
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn works_with_a_changed_split_char() {
        // Set the split char from | to ~, so | can be used in the output.
        let mut formatter = PluralLocalizationFormatter::new();
        formatter.set_split_char('~').unwrap();
        let smart = smart_with(formatter);

        let template = "There {People.Count:plural:|is a person|.~|are {} people|.}";
        let one = smart.format(template, &people(1)).unwrap();
        let two = smart.format(template, &people(2)).unwrap();
        assert_eq!(one, "There |is a person|.");
        assert_eq!(two, "There |are 2 people|.");
    }

    #[test]
    fn rejects_an_invalid_split_char() {
        let mut formatter = PluralLocalizationFormatter::new();
        assert_eq!(formatter.set_split_char(';'), Err(InvalidSplitChar(';')));
        assert_eq!(formatter.split_char(), '|');
        assert!(formatter.set_split_char(',').is_ok());
        assert!(formatter.set_split_char('~').is_ok());
        assert!(formatter.set_split_char('|').is_ok());
        assert_eq!(
            InvalidSplitChar(' ').to_string(),
            "Only '|', ',' and '~' are valid split chars."
        );
    }

    // -- Auto-detection ----------------------------------------------------

    #[test]
    fn auto_detection_needs_more_than_one_word() {
        // These run with the stripped registry of `smart()`, which holds this
        // formatter and the default one. They pin what *this* formatter does
        // with an unnamed format, not what the default registry renders: .NET
        // sorts `ListFormatter` — which auto-detects as well — ahead of this
        // one, so `{0:one|many}` on a list is claimed by `ListFormatter` first.
        //
        // Two words and no formatter name: the plural formatter takes it.
        assert_eq!(format_in("en", "{0:one|many}", Value::Int(1)), "one");
        assert_eq!(format_in("en", "{0:one|many}", Value::Int(2)), "many");

        // One word: the formatter declines before it looks at the value, so
        // the default formatter — which cannot render a list — gets the
        // placeholder and fails on it.
        let error = smart().format("{0:one}", &args([list(1)]));
        assert!(
            matches!(&error, Err(Error::Format { message, .. }) if message.contains("list")),
            "{error:?}"
        );
        // With two words the same list pluralizes.
        assert_eq!(format_in("en", "{0:one|many}", list(1)), "one");
    }

    #[test]
    fn auto_detection_does_not_handle_strings_bools_or_null() {
        // .NET lets the ConditionalFormatter have these; with only the plural
        // and the default formatter registered, the default formatter writes
        // the value — none of these three takes a format specifier.
        for (value, expected) in [
            (Value::from("String"), "String"),
            (Value::Bool(false), "False"),
            (Value::Null, ""),
        ] {
            assert_eq!(format_in("en", "{0:one|many}", value.clone()), expected);
        }
    }

    #[test]
    fn a_formatter_that_cannot_auto_detect_is_skipped() {
        let mut formatter = PluralLocalizationFormatter::new();
        formatter.set_can_auto_detect(false);
        assert!(!Formatter::can_auto_detect(&formatter));

        let smart = smart_with(formatter);
        // Now the default formatter takes it, and a string ignores the format.
        let text = args([Value::from("x")]);
        assert_eq!(smart.format("{0:one|many}", &text).unwrap(), "x");
        // Naming the formatter still works.
        let result = smart
            .format("{0:plural:one|many}", &args([Value::Int(1)]))
            .unwrap();
        assert_eq!(result, "one");
    }

    // -- Errors ------------------------------------------------------------

    #[test]
    fn errors_when_the_rule_has_no_word_for_the_value() {
        // .NET reports the index as the number of words minus one.
        assert_eq!(
            error_of("{0:plural:One}", Value::Int(1)),
            (
                envelope(
                    "{0:plural:One}",
                    "Invalid number of plural parameters in PluralLocalizationFormatter",
                    0
                ),
                0
            )
        );
        assert_eq!(
            error_of("{0:plural:a|b|c|d|e}", Value::Int(1)),
            (
                envelope(
                    "{0:plural:a|b|c|d|e}",
                    "Invalid number of plural parameters in PluralLocalizationFormatter",
                    4
                ),
                4
            )
        );
    }

    #[test]
    fn errors_when_the_argument_is_not_a_number_or_a_list() {
        let mut map = BTreeMap::new();
        map.insert("a".to_owned(), Value::Int(1));

        let cases: &[(Value, &str)] = &[
            (Value::from("1234"), "System.String"),
            (Value::from(""), "System.String"),
            (Value::Bool(false), "System.Boolean"),
            (Value::Null, "null"),
            // float.MaxValue exceeds decimal.MaxValue, as in the .NET fixture.
            (Value::Float(3.402_823_47e38), "System.Double"),
            (Value::Float(f64::NAN), "System.Double"),
            (Value::Float(f64::INFINITY), "System.Double"),
            (
                Value::Map(map),
                "System.Collections.Generic.Dictionary`2[System.String,System.Object]",
            ),
        ];

        for (value, type_name) in cases {
            let expected = envelope(
                "{0:plural:One|Two}",
                &format!(
                    "Formatter named 'plural' can format numbers and IEnumerables, but the argument was of type '{type_name}'"
                ),
                0,
            );
            // .NET reports this one at index 0.
            assert_eq!(
                error_of("{0:plural:One|Two}", value.clone()),
                (expected, 0),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_named_culture_may_use_an_underscore() {
        // .NET hands the name to ICU, which reads the text after the `_` as an
        // alternate sort order and `en_US` as English —
        // `CultureInfo.GetCultureInfo("en_US").TwoLetterISOLanguageName` is
        // "en" (probed).
        assert_eq!(
            format_in("en", "{0:plural(ru_RU):a|b|c}", Value::Int(1)),
            "a"
        );
        assert_eq!(format_in("ru", "{0:plural(en_US):a|b}", Value::Int(2)), "b");
        assert_eq!(
            format_in("en", "{0:plural(zh_Hans):all}", Value::Int(7)),
            "all"
        );
        // A sort order may itself have subtags: `en_US-POSIX` is one
        // underscore and is English, where `en_US_POSIX` is two and is no
        // culture at all (probed — the rejection is asserted below).
        assert_eq!(
            format_in("ru", "{0:plural(en_US-POSIX):one|many}", Value::Int(2)),
            "many"
        );
    }

    #[test]
    fn a_malformed_culture_name_is_the_dotnet_culture_error() {
        // .NET's `CultureInfo.GetCultureInfo` throws a
        // `CultureNotFoundException`, which the formatter wraps in a
        // `FormattingException` reported at index 0 — so this message carries
        // the envelope, where the "no rule for this language" one does not.
        // The names are the ones .NET rejects: an empty subtag, a character
        // outside the ASCII alphanumerics, more than one underscore, a
        // one-character name, and a language subtag of more than 11 characters.
        let cases = [
            ("en-", "en-"),
            ("en--US", "en--us"),
            ("-en", "-en"),
            ("EN_", "en_"),
            ("aa_bb_cc", "aa_bb_cc"),
            ("en_US_POSIX", "en_us_posix"),
            ("@@", "@@"),
            ("e n", "e n"),
            ("ру", "ру"),
            ("a", "a"),
            ("aaaaaaaaaaaa", "aaaaaaaaaaaa"),
        ];
        for (name, quoted) in cases {
            let template = format!("{{0:plural({name}):a|b}}");
            let (message, position) = error_of(&template, Value::Int(1));
            assert_eq!(
                message,
                envelope(
                    &template,
                    &format!(
                        "Culture is not supported. (Parameter 'name')\n\
                         {quoted} is an invalid culture identifier."
                    ),
                    0
                ),
                "{name}"
            );
            assert_eq!(position, 0, "{name}");
        }

        // A name .NET accepts but has no rule for is the other error, without
        // the envelope — `a-b` and `1234` are cultures as far as ICU cares.
        for (name, language) in [
            ("a-b", "a"),
            ("1234", "1234"),
            ("aaaaaaaaaaa", "aaaaaaaaaaa"),
        ] {
            let template = format!("{{0:plural({name}):a|b}}");
            let (message, _) = error_of(&template, Value::Int(1));
            assert_eq!(
                message,
                format!(
                    "IsoLangToDelegate not found for {language} (Parameter 'twoLetterIsoLanguageName')"
                )
            );
        }

        // And a long name whose language .NET does know renders.
        for name in [
            "en-US-POSIX",
            "en_US-POSIX",
            "en-us-x-private",
            "en-Latn-US",
        ] {
            let template = format!("{{0:plural({name}):one|many}}");
            assert_eq!(format_in("ru", &template, Value::Int(2)), "many", "{name}");
        }
    }

    #[test]
    fn errors_when_the_language_has_no_rule() {
        // .NET's `ArgumentException`, raised at the start of the format.
        for language in ["xx", "hy", "nonsense"] {
            let template = format!("{{0:plural({language}):a|b}}");
            let (message, position) = error_of(&template, Value::Int(1));
            assert_eq!(
                message,
                format!(
                    "IsoLangToDelegate not found for {language} (Parameter 'twoLetterIsoLanguageName')"
                )
            );
            assert_eq!(position, template.len() - 4, "{template}");
        }
    }

    #[test]
    fn the_error_action_applies_to_the_formatters_errors() {
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Ignore,
            ..SmartSettings::default()
        });
        smart
            .formatters_mut()
            .insert(0, Box::new(PluralLocalizationFormatter::new()));
        let result = smart
            .format("x{0:plural:a|b|c|d|e}y", &args([Value::Int(1)]))
            .unwrap();
        assert_eq!(result, "xy");
    }

    // -- Culture and language resolution -----------------------------------

    #[test]
    fn the_language_comes_from_the_culture_name() {
        let template = "{0:plural:a|b|c}";
        // Any Serbian culture, however written, pluralizes as Serbian.
        for name in ["sr", "sr-Cyrl-RS", "sr-Latn"] {
            assert_eq!(format_in(name, template, Value::Int(22)), "b", "{name}");
        }
        // The invariant culture is English.
        assert_eq!(format_in("", "{0:plural:one|many}", Value::Int(1)), "one");
        assert_eq!(format_in("", "{0:plural:one|many}", Value::Int(2)), "many");
    }

    #[test]
    fn the_named_culture_may_be_a_full_culture_name() {
        let template = "{0:plural(#):a|b|c}";
        for name in ["pl", "PL", "pl-PL", " pl ", "pl-pl"] {
            let template = template.replace('#', name);
            assert_eq!(format_in("en", &template, Value::Int(5)), "c", "{name}");
        }
        // Empty or blank options fall back to the culture of the call.
        for template in ["{0:plural():one|many}", "{0:plural( ):one|many}"] {
            assert_eq!(
                format_in("en", template, Value::Int(1)),
                "one",
                "{template}"
            );
        }
    }

    // -- Values ------------------------------------------------------------

    #[test]
    fn a_double_is_rounded_the_way_dotnet_rounds_it_to_decimal() {
        let template = "{0:plural:one|many}";
        // 15 significant digits: these two round to exactly 1.
        assert_eq!(
            format_in("en", template, Value::Float(0.999_999_999_999_999_9)),
            "one"
        );
        assert_eq!(
            format_in("en", template, Value::Float(1.000_000_000_000_000_2)),
            "one"
        );
        // A magnitude decimal can still hold.
        assert_eq!(format_in("en", template, Value::Float(1e28)), "many");
        // A magnitude below decimal's smallest unit is zero, which French
        // tells apart from a small fraction.
        let french = "{0:plural:zero|one|other}";
        assert_eq!(format_in("fr", french, Value::Float(1e-30)), "zero");
        assert_eq!(format_in("fr", french, Value::Float(5e-29)), "zero");
        assert_eq!(format_in("fr", french, Value::Float(6e-29)), "one");
        assert_eq!(format_in("fr", french, Value::Float(0.5)), "one");
    }

    #[cfg(feature = "time")]
    #[test]
    fn a_date_is_not_a_number() {
        let date = Value::DateTime(jiff::civil::date(2020, 1, 1).at(0, 0, 0, 0));
        let (message, position) = error_of("{0:plural:One|Two}", date);
        assert_eq!(
            message,
            envelope(
                "{0:plural:One|Two}",
                "Formatter named 'plural' can format numbers and IEnumerables, but the argument was of type 'System.DateTime'",
                0
            )
        );
        assert_eq!(position, 0);
    }

    #[test]
    fn an_integer_beyond_the_precision_of_a_double_is_a_known_divergence() {
        // .NET pluralizes through `decimal`, which holds every `long`
        // exactly; we pluralize through `f64`, which rounds above 2^53. .NET
        // answers "a" here (10000000000000001 % 10 == 1, Russian "one").
        let template = "{0:plural(ru):a|b|c}";
        assert_eq!(
            format_in("en", template, Value::Int(10_000_000_000_000_001)),
            "c"
        );
    }

    #[test]
    fn a_large_double_is_the_same_known_divergence() {
        // `to_decimal` rounds to the 15 significant digits `Convert.ToDecimal`
        // keeps, but the result goes back into an `f64`, which cannot hold the
        // exact decimal above 2^53 either: 1e28 is 10000000000000000905969664
        // as a double, so the `% 10` the Russian rule runs sees 4 ("few")
        // where .NET's decimal sees 0 ("other", "c"). Probed against 3.6.1.
        let template = "{0:plural(ru):a|b|c}";
        assert_eq!(format_in("en", template, Value::Float(1e28)), "b");
        assert_eq!(format_in("en", template, Value::Float(1e24)), "b");
        // Below ~3.9e17 every double still has an exact `f64` decimal.
        assert_eq!(format_in("en", template, Value::Float(1e17)), "c");
    }

    #[test]
    fn a_double_beyond_decimal_is_not_a_number() {
        // .NET's `Convert.ToDecimal` overflows at 2^96 exactly.
        for value in [1e29, -1e29, 7.922_816_251_426_434e28] {
            let (message, _) = error_of("{0:plural:one|many}", Value::Float(value));
            assert!(message.contains("System.Double"), "{value}: {message}");
        }
        assert_eq!(
            format_in(
                "en",
                "{0:plural:one|many}",
                Value::Float(7.922_816_251_426_433e28)
            ),
            "many"
        );
    }

    // -- Splitting ---------------------------------------------------------

    #[test]
    fn empty_words_render_nothing() {
        assert_eq!(format_in("en", "[{0:plural:|}]", Value::Int(1)), "[]");
        assert_eq!(format_in("en", "[{0:plural:|b}]", Value::Int(1)), "[]");
        assert_eq!(format_in("en", "[{0:plural:a|}]", Value::Int(2)), "[]");
    }

    #[test]
    fn a_nested_placeholder_does_not_split_the_format() {
        let template = "{0:plural:{0:plural:x|y}|c}";
        assert_eq!(format_in("en", template, Value::Int(1)), "x");
        assert_eq!(format_in("en", template, Value::Int(2)), "c");
    }

    #[test]
    fn splits_on_a_separator_inside_an_escape_sequence() {
        // .NET searches the source text of a literal, so the `|` of the
        // invalid escape sequence `\|` splits the format; what is left of the
        // first word is the lone `\`, which resolves to itself. Three words,
        // so English picks by zero/one/other.
        let template = r"{0:plural:a\|b|c}";
        assert_eq!(format_in("en", template, Value::Int(0)), r"a\");
        assert_eq!(format_in("en", template, Value::Int(1)), "b");
        assert_eq!(format_in("en", template, Value::Int(2)), "c");
    }

    #[test]
    fn keeps_valid_escape_sequences() {
        assert_eq!(format_in("en", r"{0:plural:a\nb|c}", Value::Int(1)), "a\nb");
        assert_eq!(format_in("en", r"{0:plural:a\{b|c}", Value::Int(1)), "a{b");
    }

    #[test]
    fn the_alignment_of_the_placeholder_applies() {
        assert_eq!(
            format_in("en", "[{0,10:plural:one|many}]", Value::Int(2)),
            "[      many]"
        );
        assert_eq!(
            format_in("en", "[{0,-10:plural:one|many}]", Value::Int(1)),
            "[one       ]"
        );
    }

    #[test]
    fn a_placeholder_without_a_format_is_declined() {
        // `{0:plural}` has no format at all — "plural" is parsed as the format
        // of a nameless placeholder — so the default formatter takes it.
        let smart = smart();
        let result = smart
            .format("{0:plural}", &args([Value::from("x")]))
            .unwrap();
        assert_eq!(result, "x");
    }
}
