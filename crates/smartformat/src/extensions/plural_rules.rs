//! Port of SmartFormat.NET's self-contained `Utilities/PluralRules.cs` language table.
//!
//! Every language SmartFormat.NET knows maps to a rule that picks one of the
//! plural words of a format by the value being pluralized:
//!
//! ```
//! use smartformat::extensions::plural_rules::get_plural_rule;
//!
//! // Russian has three plural words: one, few, other.
//! let rule = get_plural_rule("ru").expect("a rule for Russian");
//! assert_eq!(rule(21.0, 3), 0);
//! assert_eq!(rule(22.0, 3), 1);
//! assert_eq!(rule(25.0, 3), 2);
//! ```
//!
//! The table is a port of SmartFormat.NET's, not of CLDR: SmartFormat.NET does
//! not consult CLDR at runtime, so the compatible thing to do is to copy its
//! table, including where it disagrees with CLDR (Icelandic is `one`/`other`
//! here, Welsh and Breton share one rule, and so on).

/// A pluralization rule: `(value, plural_words_count) -> index of the word to
/// use`, the port of .NET `PluralRules.PluralRuleDelegate`.
///
/// A rule that cannot serve the given number of plural words returns `-1`,
/// which the caller reports as an error — .NET
/// `PluralLocalizationFormatter` throws "Invalid number of plural parameters"
/// for a negative index or one past the last word.
///
/// .NET runs the rules on `decimal`; we run them on `f64`. See
/// [`crate::extensions::plural`] for how a value is converted, and DESIGN.md
/// for what the two types disagree about.
pub type PluralRule = fn(f64, usize) -> i32;

/// Looks up the rule for a language, .NET `PluralRules.GetPluralRule`.
///
/// The name is the two-letter ISO-639 language code, or the three-letter one
/// where there is no two-letter code (`kde`, `fil`, …) — .NET
/// `CultureInfo.TwoLetterISOLanguageName` returns exactly that. The lookup is
/// ordinal and case-sensitive, as .NET's dictionary is, and every code in the
/// table is lowercase.
///
/// `None` is .NET's `ArgumentException`; the caller turns it into the error
/// message .NET raises.
pub fn get_plural_rule(iso_language_name: &str) -> Option<PluralRule> {
    ISO_LANG_TO_RULE
        .iter()
        .find(|(language, _)| *language == iso_language_name)
        .map(|(_, rule)| *rule)
}

/// The whole language table, in the order .NET declares it
/// (`PluralRules.DefaultLangToDelegate`).
///
/// .NET exposes the table as a mutable dictionary plus a `RestoreDefault()`,
/// so a program can change pluralization globally. We expose it read-only; a
/// caller that wants its own rule passes one to
/// [`PluralLocalizationFormatter::with_custom_rule`](crate::extensions::plural::PluralLocalizationFormatter::with_custom_rule),
/// which is the counterpart of .NET's `CustomPluralRuleProvider`.
pub fn iso_lang_to_rule() -> &'static [(&'static str, PluralRule)] {
    ISO_LANG_TO_RULE
}

/// .NET `PluralRules.DefaultLangToDelegate`.
static ISO_LANG_TO_RULE: &[(&str, PluralRule)] = &[
    // Singular
    ("az", singular),   // Azerbaijani
    ("bm", singular),   // Bambara
    ("bo", singular),   // Tibetan
    ("dz", singular),   // Dzongkha
    ("fa", singular),   // Persian
    ("hu", singular),   // Hungarian
    ("id", singular),   // Indonesian
    ("ig", singular),   // Igbo
    ("ii", singular),   // Sichuan Yi
    ("ja", singular),   // Japanese
    ("jv", singular),   // Javanese
    ("ka", singular),   // Georgian
    ("kde", singular),  // Makonde
    ("kea", singular),  // Kabuverdianu
    ("km", singular),   // Khmer
    ("kn", singular),   // Kannada
    ("ko", singular),   // Korean
    ("ms", singular),   // Malay
    ("my", singular),   // Burmese
    ("root", singular), // Root (?)
    ("sah", singular),  // Sakha
    ("ses", singular),  // Koyraboro Senni
    ("sg", singular),   // Sango
    ("th", singular),   // Thai
    ("to", singular),   // Tonga
    ("vi", singular),   // Vietnamese
    ("wo", singular),   // Wolof
    ("yo", singular),   // Yoruba
    ("zh", singular),   // Chinese
    // Dual: one (n == 1), other
    ("af", dual_one_other),  // Afrikaans
    ("bem", dual_one_other), // Bembda
    ("bg", dual_one_other),  // Bulgarian
    ("bn", dual_one_other),  // Bengali
    ("brx", dual_one_other), // Bodo
    ("ca", dual_one_other),  // Catalan
    ("cgg", dual_one_other), // Chiga
    ("chr", dual_one_other), // Cherokee
    ("da", dual_one_other),  // Danish
    ("de", dual_one_other),  // German
    ("dv", dual_one_other),  // Divehi
    ("ee", dual_one_other),  // Ewe
    ("el", dual_one_other),  // Greek
    ("en", dual_one_other),  // English
    ("eo", dual_one_other),  // Esperanto
    ("es", dual_one_other),  // Spanish
    ("et", dual_one_other),  // Estonian
    ("eu", dual_one_other),  // Basque
    ("fi", dual_one_other),  // Finnish
    ("fo", dual_one_other),  // Faroese
    ("fur", dual_one_other), // Friulian
    ("fy", dual_one_other),  // Western Frisian
    ("gl", dual_one_other),  // Galician
    ("gsw", dual_one_other), // Swiss German
    ("gu", dual_one_other),  // Gujarati
    ("ha", dual_one_other),  // Hausa
    ("haw", dual_one_other), // Hawaiian
    ("he", dual_one_other),  // Hebrew
    ("is", dual_one_other),  // Icelandic
    ("it", dual_one_other),  // Italian
    ("kk", dual_one_other),  // Kazakh
    ("kl", dual_one_other),  // Kalaallisut
    ("ku", dual_one_other),  // Kurdish
    ("lb", dual_one_other),  // Luxembourgish
    ("lg", dual_one_other),  // Ganda
    ("lo", dual_one_other),  // Lao
    ("mas", dual_one_other), // Masai
    ("ml", dual_one_other),  // Malayalam
    ("mn", dual_one_other),  // Mongolian
    ("mr", dual_one_other),  // Marathi
    ("nah", dual_one_other), // Nahuatl
    ("nb", dual_one_other),  // Norwegian Bokmål
    ("ne", dual_one_other),  // Nepali
    ("nl", dual_one_other),  // Dutch
    ("nn", dual_one_other),  // Norwegian Nynorsk
    ("no", dual_one_other),  // Norwegian
    ("nyn", dual_one_other), // Nyankole
    ("om", dual_one_other),  // Oromo
    ("or", dual_one_other),  // Oriya
    ("pa", dual_one_other),  // Punjabi
    ("pap", dual_one_other), // Papiamento
    ("ps", dual_one_other),  // Pashto
    ("pt", dual_one_other),  // Portuguese
    ("rm", dual_one_other),  // Romansh
    ("saq", dual_one_other), // Samburu
    ("so", dual_one_other),  // Somali
    ("sq", dual_one_other),  // Albanian
    ("ssy", dual_one_other), // Saho
    ("sw", dual_one_other),  // Swahili
    ("sv", dual_one_other),  // Swedish
    ("syr", dual_one_other), // Syriac
    ("ta", dual_one_other),  // Tamil
    ("te", dual_one_other),  // Telugu
    ("tk", dual_one_other),  // Turkmen
    ("tr", dual_one_other),  // Turkish
    ("ur", dual_one_other),  // Urdu
    ("wae", dual_one_other), // Walser
    ("xog", dual_one_other), // Soga
    ("zu", dual_one_other),  // Zulu
    // DualWithZero: one (n == 0..1), other
    ("ak", dual_with_zero),  // Akan
    ("am", dual_with_zero),  // Amharic
    ("bh", dual_with_zero),  // Bihari
    ("fil", dual_with_zero), // Filipino
    ("guw", dual_with_zero), // Gun
    ("hi", dual_with_zero),  // Hindi
    ("ln", dual_with_zero),  // Lingala
    ("mg", dual_with_zero),  // Malagasy
    ("nso", dual_with_zero), // Northern Sotho
    ("ti", dual_with_zero),  // Tigrinya
    ("tl", dual_with_zero),  // Tagalog
    ("wa", dual_with_zero),  // Walloon
    // DualFromZeroToTwo: one (n == 0..2 fractionate and n != 2), other
    ("ff", dual_from_zero_to_two),  // Fulah
    ("fr", dual_from_zero_to_two),  // French
    ("kab", dual_from_zero_to_two), // Kabyle
    // Triple: one (n == 1), two (n == 2), other
    ("ga", triple_one_two_other),  // Irish
    ("iu", triple_one_two_other),  // Inuktitut
    ("ksh", triple_one_two_other), // Colognian
    ("kw", triple_one_two_other),  // Cornish
    ("se", triple_one_two_other),  // Northern Sami
    ("sma", triple_one_two_other), // Southern Sami
    ("smi", triple_one_two_other), // Sami language
    ("smj", triple_one_two_other), // Lule Sami
    ("smn", triple_one_two_other), // Inari Sami
    ("sms", triple_one_two_other), // Skolt Sami
    // Russian & Serbo-Croatian
    ("be", russian_serbo_croatian), // Belarusian
    ("bs", russian_serbo_croatian), // Bosnian
    ("hr", russian_serbo_croatian), // Croatian
    ("ru", russian_serbo_croatian), // Russian
    ("sh", russian_serbo_croatian), // Serbo-Croatian
    ("sr", russian_serbo_croatian), // Serbian
    ("uk", russian_serbo_croatian), // Ukrainian
    // Unique
    // Arabic
    ("ar", arabic),
    // Breton
    ("br", breton),
    // Czech
    ("cs", czech),
    // Welsh
    ("cy", welsh),
    // Manx
    ("gv", manx),
    // Langi
    ("lag", langi),
    // Lithuanian
    ("lt", lithuanian),
    // Latvian
    ("lv", latvian),
    // Macedonian
    ("mb", macedonian),
    // Moldavian
    ("mo", moldavian),
    // Maltese
    ("mt", maltese),
    // Polish
    ("pl", polish),
    // Romanian
    ("ro", romanian),
    // Tachelhit
    ("shi", tachelhit),
    // Slovak
    ("sk", slovak),
    // Slovenian
    ("sl", slovenian),
    // Central Morocco Tamazight
    ("tzm", central_morocco_tamazight),
];

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------
//
// One function per .NET `PluralRuleDelegate`. The comparisons are the C# ones,
// in the C# order; `value` is `decimal` there and `f64` here.

/// One word for every value.
pub fn singular(_value: f64, _plural_words_count: usize) -> i32 {
    0
}

/// Dual: one (n == 1), other.
pub fn dual_one_other(value: f64, plural_words_count: usize) -> i32 {
    match plural_words_count {
        2 => {
            if value == 1.0 {
                0
            } else {
                1
            }
        }
        3 => {
            if value == 0.0 {
                0
            } else if value == 1.0 {
                1
            } else {
                2
            }
        }
        4 => {
            if value < 0.0 {
                0
            } else if value == 0.0 {
                1
            } else if value == 1.0 {
                2
            } else {
                3
            }
        }
        _ => -1,
    }
}

/// DualWithZero: one (n == 0..1), other.
pub fn dual_with_zero(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 || value == 1.0 {
        0
    } else {
        1
    }
}

/// DualFromZeroToTwo: one (n == 0..2 fractionate and n != 2), other.
pub fn dual_from_zero_to_two(value: f64, plural_words_count: usize) -> i32 {
    if plural_words_count == 2 {
        return if (0.0..2.0).contains(&value) { 0 } else { 1 };
    }

    if plural_words_count == 3 {
        return words_count_3_value(value);
    }

    if plural_words_count == 4 {
        return words_count_4_value(value);
    }

    -1
}

/// .NET `GetWordsCount3Value`.
fn words_count_3_value(n: f64) -> i32 {
    if n == 0.0 {
        0
    } else if n > 0.0 && n < 2.0 {
        1
    } else {
        2
    }
}

/// .NET `GetWordsCount4Value`.
fn words_count_4_value(n: f64) -> i32 {
    if n < 0.0 {
        0
    } else if n == 0.0 {
        1
    } else if n > 0.0 && n < 2.0 {
        2
    } else {
        3
    }
}

/// Triple: one (n == 1), two (n == 2), other.
pub fn triple_one_two_other(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0
    } else if value == 2.0 {
        1
    } else {
        2
    }
}

/// Russian & Serbo-Croatian: one, few, other.
pub fn russian_serbo_croatian(value: f64, _plural_words_count: usize) -> i32 {
    if value % 10.0 == 1.0 && value % 100.0 != 11.0 {
        0 // one
    } else if between_without_fraction(value % 10.0, 2.0, 4.0)
        && !between_without_fraction(value % 100.0, 12.0, 14.0)
    {
        1 // few
    } else {
        2
    }
}

/// Arabic: zero, one, two, few, many, other.
pub fn arabic(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0 // zero
    } else if value == 1.0 {
        1 // one
    } else if value == 2.0 {
        2 // two
    } else if between_without_fraction(value % 100.0, 3.0, 10.0) {
        3 // few
    } else if between_without_fraction(value % 100.0, 11.0, 99.0) {
        4 // many
    } else {
        5 // other
    }
}

/// Breton: zero, one, two, few, many, other.
pub fn breton(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0 // zero
    } else if value == 1.0 {
        1 // one
    } else if value == 2.0 {
        2 // two
    } else if value == 3.0 {
        3 // few
    } else if value == 6.0 {
        4 // many
    } else {
        5 // other
    }
}

/// Czech: zero, one, few, many, other.
pub fn czech(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0 // zero
    } else if value == 1.0 {
        1 // one
    } else if between_without_fraction(value, 2.0, 4.0) {
        2 // few
    } else if value % 1.0 == 0.0 {
        3 // many
    } else {
        4 // other
    }
}

/// Welsh: zero, one, two, few, many, other.
pub fn welsh(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0 // zero
    } else if value == 1.0 {
        1 // one
    } else if value == 2.0 {
        2 // two
    } else if value == 3.0 {
        3 // few
    } else if value == 6.0 {
        4 // many
    } else {
        5 // other
    }
}

/// Manx: one, other.
pub fn manx(value: f64, _plural_words_count: usize) -> i32 {
    if between_without_fraction(value % 10.0, 1.0, 2.0) || value % 20.0 == 0.0 {
        0 // one
    } else {
        1
    }
}

/// Langi: zero, one, other.
pub fn langi(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0
    } else if value > 0.0 && value < 2.0 {
        1
    } else {
        2
    }
}

/// Lithuanian: one, few, other.
pub fn lithuanian(value: f64, _plural_words_count: usize) -> i32 {
    if value % 10.0 == 1.0 && !between_without_fraction(value % 100.0, 11.0, 19.0) {
        0 // one
    } else if between_without_fraction(value % 10.0, 2.0, 9.0)
        && !between_without_fraction(value % 100.0, 11.0, 19.0)
    {
        1 // few
    } else {
        2
    }
}

/// Latvian: zero, one, other.
pub fn latvian(value: f64, _plural_words_count: usize) -> i32 {
    if value == 0.0 {
        0 // zero
    } else if value % 10.0 == 1.0 && value % 100.0 != 11.0 {
        1
    } else {
        2
    }
}

/// Macedonian: one, other.
pub fn macedonian(value: f64, _plural_words_count: usize) -> i32 {
    if value % 10.0 == 1.0 && value != 11.0 {
        0 // one
    } else {
        1
    }
}

/// Moldavian: one, few, other.
pub fn moldavian(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0 // one
    } else if value == 0.0 || value != 1.0 && between_without_fraction(value % 100.0, 1.0, 19.0) {
        1 // few
    } else {
        2
    }
}

/// Maltese: one, few, many, other.
pub fn maltese(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0 // one
    } else if value == 0.0 || between_without_fraction(value % 100.0, 2.0, 10.0) {
        1 // few
    } else if between_without_fraction(value % 100.0, 11.0, 19.0) {
        2 // many
    } else {
        3
    }
}

/// Polish: one, few, many, other.
pub fn polish(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0 // one
    } else if between_without_fraction(value % 10.0, 2.0, 4.0)
        && !between_without_fraction(value % 100.0, 12.0, 14.0)
    {
        1 // few
    } else if between_without_fraction(value % 10.0, 0.0, 1.0)
        || between_without_fraction(value % 10.0, 5.0, 9.0)
        || between_without_fraction(value % 100.0, 12.0, 14.0)
    {
        2 // many
    } else {
        3
    }
}

/// Romanian: one, few, other.
pub fn romanian(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0 // one
    } else if value == 0.0 || between_without_fraction(value % 100.0, 1.0, 19.0) {
        1 // few
    } else {
        2
    }
}

/// Tachelhit: one, few, other.
pub fn tachelhit(value: f64, _plural_words_count: usize) -> i32 {
    if (0.0..=1.0).contains(&value) {
        0 // one
    } else if between_without_fraction(value, 2.0, 10.0) {
        1 // few
    } else {
        2
    }
}

/// Slovak: one, few, other.
pub fn slovak(value: f64, _plural_words_count: usize) -> i32 {
    if value == 1.0 {
        0 // one
    } else if between_without_fraction(value, 2.0, 4.0) {
        1 // few
    } else {
        2
    }
}

/// Slovenian: one, two, few, other.
pub fn slovenian(value: f64, _plural_words_count: usize) -> i32 {
    if value % 100.0 == 1.0 {
        0 // one
    } else if value % 100.0 == 2.0 {
        1 // two
    } else if between_without_fraction(value % 100.0, 3.0, 4.0) {
        2 // few
    } else {
        3
    }
}

/// Central Morocco Tamazight: one, other.
pub fn central_morocco_tamazight(value: f64, _plural_words_count: usize) -> i32 {
    if between_without_fraction(value, 0.0, 1.0) || between_without_fraction(value, 11.0, 99.0) {
        0 // one
    } else {
        1
    }
}

/// Whether the value is inclusively between the min and the max and has no
/// fraction (.NET `BetweenWithoutFraction`).
fn between_without_fraction(value: f64, min: f64, max: f64) -> bool {
    value % 1.0 == 0.0 && value >= min && value <= max
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! The expected indexes are what SmartFormat.NET 3.6.1 returns for the
    //! same language, word count and value.

    use super::*;

    #[test]
    fn every_language_of_the_dotnet_table_is_present() {
        // .NET's dictionary holds 147 languages.
        assert_eq!(iso_lang_to_rule().len(), 147);

        // Every code is lowercase and listed once, as in .NET, where a
        // duplicate key would not compile.
        let mut languages: Vec<&str> = iso_lang_to_rule()
            .iter()
            .map(|(language, _)| *language)
            .collect();
        assert!(languages
            .iter()
            .all(|language| *language == language.to_ascii_lowercase()));
        languages.sort_unstable();
        let count = languages.len();
        languages.dedup();
        assert_eq!(languages.len(), count);

        // One language per group, identified by an index only its own rule
        // returns.
        assert_eq!(get_plural_rule("kde").unwrap()(7.0, 3), 0); // Singular
        assert_eq!(get_plural_rule("nah").unwrap()(0.0, 3), 0); // DualOneOther
        assert_eq!(get_plural_rule("guw").unwrap()(0.0, 3), 0); // DualWithZero
        assert_eq!(get_plural_rule("kab").unwrap()(0.5, 3), 1); // DualFromZeroToTwo
        assert_eq!(get_plural_rule("smn").unwrap()(2.0, 3), 1); // TripleOneTwoOther
        assert_eq!(get_plural_rule("sh").unwrap()(22.0, 3), 1); // RussianSerboCroatian
        assert_eq!(get_plural_rule("ar").unwrap()(11.0, 6), 4); // Arabic
        assert_eq!(get_plural_rule("pl").unwrap()(5.0, 4), 2); // Polish
        assert_eq!(get_plural_rule("root").unwrap()(5.0, 1), 0); // the "root" key
    }

    #[test]
    fn an_unknown_language_has_no_rule() {
        // .NET throws an `ArgumentException` for these.
        for language in ["xx", "hy", "EN", "en-US", "", "nonsense"] {
            assert!(get_plural_rule(language).is_none(), "{language}");
        }
    }

    #[test]
    fn singular_takes_every_value() {
        let rule = get_plural_rule("ja").unwrap();
        for count in 1..=7 {
            for value in [-1.0, 0.0, 0.5, 1.0, 2.0, 100.0] {
                assert_eq!(rule(value, count), 0, "{value} of {count}");
            }
        }
    }

    #[test]
    fn dual_one_other_serves_two_three_and_four_words() {
        let rule = get_plural_rule("en").unwrap();
        assert_eq!(rule(1.0, 2), 0);
        for value in [-1.0, 0.0, 0.5, 2.0, 11.0] {
            assert_eq!(rule(value, 2), 1, "{value}");
        }
        assert_eq!([0.0, 1.0, 2.0].map(|value| rule(value, 3)), [0, 1, 2]);
        assert_eq!(rule(-1.0, 3), 2);
        assert_eq!(
            [-1.0, 0.0, 1.0, 2.0].map(|value| rule(value, 4)),
            [0, 1, 2, 3]
        );
        // No rule for one, five, … words: .NET's "invalid number of plural
        // parameters".
        for count in [0, 1, 5, 6] {
            assert_eq!(rule(1.0, count), -1, "{count}");
        }
    }

    #[test]
    fn dual_with_zero_ignores_the_word_count() {
        let rule = get_plural_rule("hi").unwrap();
        for count in 1..=7 {
            assert_eq!(rule(0.0, count), 0);
            assert_eq!(rule(1.0, count), 0);
            assert_eq!(rule(2.0, count), 1);
            assert_eq!(rule(-1.0, count), 1);
        }
    }

    #[test]
    fn dual_from_zero_to_two_is_french() {
        let rule = get_plural_rule("fr").unwrap();
        assert_eq!(
            [-1.0, 0.0, 0.5, 1.0, 1.5, 2.0].map(|value| rule(value, 2)),
            [1, 0, 0, 0, 0, 1]
        );
        assert_eq!(
            [-1.0, 0.0, 0.5, 1.0, 2.0].map(|value| rule(value, 3)),
            [2, 0, 1, 1, 2]
        );
        assert_eq!(
            [-1.0, 0.0, 0.5, 1.0, 2.0].map(|value| rule(value, 4)),
            [0, 1, 2, 2, 3]
        );
        for count in [0, 1, 5] {
            assert_eq!(rule(1.0, count), -1, "{count}");
        }
    }

    #[test]
    fn triple_one_two_other_is_irish() {
        let rule = get_plural_rule("ga").unwrap();
        assert_eq!(
            [1.0, 2.0, 3.0, 0.0, -1.0].map(|value| rule(value, 3)),
            [0, 1, 2, 2, 2]
        );
    }

    #[test]
    fn russian_counts_one_few_other() {
        let rule = get_plural_rule("ru").unwrap();
        let cases = [
            (1.0, 0),
            (2.0, 1),
            (5.0, 2),
            (11.0, 2),
            (21.0, 0),
            (22.0, 1),
            (25.0, 2),
            (100.0, 2),
            (101.0, 0),
            (112.0, 2),
            (0.5, 2),
            (-1.0, 2),
            (-21.0, 2),
        ];
        for (value, expected) in cases {
            assert_eq!(rule(value, 3), expected, "{value}");
        }
        // Serbian and Ukrainian share the rule.
        for language in ["sr", "uk", "hr", "bs", "be", "sh"] {
            assert_eq!(get_plural_rule(language).unwrap()(22.0, 3), 1, "{language}");
        }
    }

    #[test]
    fn arabic_has_six_forms() {
        let rule = get_plural_rule("ar").unwrap();
        let cases = [
            (0.0, 0),
            (1.0, 1),
            (2.0, 2),
            (3.0, 3),
            (10.0, 3),
            (11.0, 4),
            (99.0, 4),
            (100.0, 5),
            (103.0, 3),
            (111.0, 4),
            (0.5, 5),
        ];
        for (value, expected) in cases {
            assert_eq!(rule(value, 6), expected, "{value}");
        }
    }

    #[test]
    fn polish_counts_one_few_many_other() {
        let rule = get_plural_rule("pl").unwrap();
        let cases = [
            (1.0, 0),
            (2.0, 1),
            (4.0, 1),
            (5.0, 2),
            (0.0, 2),
            (12.0, 2),
            (22.0, 1),
            (105.0, 2),
            (1.5, 3),
            // A negative value is "other": -1 % 10 is -1, which is in none of
            // the ranges .NET checks.
            (-1.0, 3),
        ];
        for (value, expected) in cases {
            assert_eq!(rule(value, 4), expected, "{value}");
        }
    }

    #[test]
    fn czech_counts_zero_one_few_many_other() {
        let rule = get_plural_rule("cs").unwrap();
        let cases = [
            (0.0, 0),
            (1.0, 1),
            (2.0, 2),
            (4.0, 2),
            (5.0, 3),
            (100.0, 3),
            (0.5, 4),
            (-1.0, 3),
        ];
        for (value, expected) in cases {
            assert_eq!(rule(value, 5), expected, "{value}");
        }
    }

    #[test]
    fn icelandic_is_dual_one_other_not_cldr() {
        // CLDR gives Icelandic "one" for 21, 31, …; SmartFormat.NET's table
        // does not, and this port copies SmartFormat.NET.
        let rule = get_plural_rule("is").unwrap();
        assert_eq!(rule(1.0, 2), 0);
        assert_eq!(rule(21.0, 2), 1);
    }

    #[test]
    fn welsh_and_breton_share_their_rule() {
        for language in ["cy", "br"] {
            let rule = get_plural_rule(language).unwrap();
            assert_eq!(
                [0.0, 1.0, 2.0, 3.0, 6.0, 7.0].map(|value| rule(value, 6)),
                [0, 1, 2, 3, 4, 5],
                "{language}"
            );
        }
    }

    #[test]
    fn the_unique_rules_match_dotnet() {
        // One case per remaining rule, taken from SmartFormat.NET 3.6.1.
        let cases: &[(&str, f64, i32)] = &[
            ("gv", 1.0, 0),   // Manx: n % 10 in 1..2
            ("gv", 3.0, 1),   //
            ("gv", 20.0, 0),  // … or n % 20 == 0
            ("lag", 0.0, 0),  // Langi
            ("lag", 0.5, 1),  //
            ("lag", 2.0, 2),  //
            ("lt", 21.0, 0),  // Lithuanian
            ("lt", 22.0, 1),  //
            ("lt", 11.0, 2),  //
            ("lv", 0.0, 0),   // Latvian
            ("lv", 21.0, 1),  //
            ("lv", 11.0, 2),  //
            ("mb", 21.0, 0),  // Macedonian
            ("mb", 2.0, 1),   //
            ("mo", 1.0, 0),   // Moldavian
            ("mo", 0.0, 1),   //
            ("mo", 19.0, 1),  //
            ("mo", 20.0, 2),  //
            ("mt", 1.0, 0),   // Maltese
            ("mt", 0.0, 1),   //
            ("mt", 10.0, 1),  //
            ("mt", 11.0, 2),  //
            ("mt", 20.0, 3),  //
            ("ro", 1.0, 0),   // Romanian
            ("ro", 19.0, 1),  //
            ("ro", 20.0, 2),  //
            ("shi", 0.5, 0),  // Tachelhit: 0 <= n <= 1, fraction included
            ("shi", 2.0, 1),  //
            ("shi", 11.0, 2), //
            ("sk", 1.0, 0),   // Slovak
            ("sk", 4.0, 1),   //
            ("sk", 5.0, 2),   //
            ("sl", 101.0, 0), // Slovenian
            ("sl", 102.0, 1), //
            ("sl", 103.0, 2), //
            ("sl", 105.0, 3), //
            ("tzm", 1.0, 0),  // Central Morocco Tamazight
            ("tzm", 99.0, 0), //
            ("tzm", 5.0, 1),  //
        ];
        for (language, value, expected) in cases {
            let rule = get_plural_rule(language).unwrap();
            assert_eq!(rule(*value, 6), *expected, "{language} {value}");
        }
    }

    #[test]
    fn macedonian_never_returns_one_for_eleven() {
        // .NET writes `value % 10 == 1 && value != 11`, so 11 itself is
        // "other" but 21 is "one".
        let rule = get_plural_rule("mb").unwrap();
        assert_eq!(rule(11.0, 2), 1);
        assert_eq!(rule(21.0, 2), 0);
    }
}
