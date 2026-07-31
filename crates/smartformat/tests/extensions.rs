//! What the default registry of formatter extensions does as a whole: which
//! formatters are in it, in which order, and what that order decides.
//!
//! Every expectation here was probed against the pinned SmartFormat.NET 3.6.1
//! package (`Smart.CreateDefaultSmartFormat()`), not read off the upstream
//! `main` branch. The per-formatter behaviour is unit-tested inside each
//! extension module, against a registry holding only that formatter; these
//! tests are about what happens when they are all registered together.

use smartformat::{Error, ErrorAction, SmartFormatter, SmartSettings, Value};

fn args(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(values.into_iter().collect())
}

/// The .NET default of `FormatErrorAction.ThrowError`.
fn throwing() -> SmartFormatter {
    SmartFormatter::new(SmartSettings {
        format_error_action: ErrorAction::Error,
        ..SmartSettings::default()
    })
}

fn render(template: &str, value: Value, culture: &str) -> Result<String, Error> {
    throwing().format_with_culture_name(template, &args([value]), culture)
}

fn rendered(template: &str, value: Value, culture: &str) -> String {
    render(template, value, culture)
        .unwrap_or_else(|error| panic!("{template:?} in {culture:?} failed: {error}"))
}

// ---------------------------------------------------------------------------
// Registration order
// ---------------------------------------------------------------------------

/// .NET adds its extensions in one `AddExtensions` call and lets
/// `WellKnownExtensionTypes.Formatters` sort them by a fixed rank. Probing
/// `Smart.CreateDefaultSmartFormat().GetFormatterExtensions()` in 3.6.1 gives
///
/// ```text
/// list, plural, cond, ismatch, isnull, choose, substr, d
/// ```
///
/// so the ones ported so far have to come out in that relative order. The
/// missing four land in M3 and slot in without moving anything.
#[test]
fn the_default_registry_is_in_dotnet_order() {
    let smart = SmartFormatter::default();

    let names: Vec<&str> = smart.formatters().iter().map(|f| f.name()).collect();
    #[cfg(feature = "plural")]
    assert_eq!(names, ["plural", "cond", "choose", "d"]);
    #[cfg(not(feature = "plural"))]
    assert_eq!(names, ["cond", "choose", "d"]);
}

/// Only the auto-detecting formatters are consulted for a placeholder that
/// names none, which is what makes the order above observable.
#[test]
fn auto_detection_flags_match_dotnet() {
    let smart = SmartFormatter::default();

    let flags: Vec<(&str, bool)> = smart
        .formatters()
        .iter()
        .map(|f| (f.name(), f.can_auto_detect()))
        .collect();
    assert!(flags.contains(&("cond", true)));
    assert!(flags.contains(&("choose", false)));
    assert!(flags.contains(&("d", true)));
    #[cfg(feature = "plural")]
    assert!(flags.contains(&("plural", true)));
}

// ---------------------------------------------------------------------------
// Auto-detection interplay
// ---------------------------------------------------------------------------

/// `plural` sits ahead of `cond`, and both auto-detect a `|`-separated format,
/// so for a *number* the plural rule of the culture decides — never the
/// conditional formatter's "index by value" rule. The two disagree on almost
/// every case below, which is what makes this a pin and not a tautology:
/// `cond` would render `{0:a|b|c}` with 0 as `a` in every culture, and
/// `{0:a|b}` with 0 as `b`.
#[cfg(feature = "plural")]
#[test]
fn plural_wins_over_cond_for_numbers() {
    // (template, value, invariant/en, ru, fr)
    let cases: &[(&str, i64, &str, &str, &str)] = &[
        ("{0:a|b}", 1, "a", "a", "a"),
        ("{0:a|b}", 2, "b", "b", "b"),
        // Russian has no two-word rule for 0, and French counts 0 as "one".
        ("{0:a|b}", 0, "b", "!", "a"),
        ("{0:a|b|c}", 1, "b", "a", "b"),
        ("{0:a|b|c}", 2, "c", "b", "c"),
        ("{0:a|b|c}", 0, "a", "c", "a"),
        ("{0:a|b|c}", -1, "c", "c", "c"),
        ("{0:a|b|c|d}", 3, "d", "b", "d"),
    ];

    for &(template, value, invariant, russian, french) in cases {
        for (culture, expected) in [
            ("", invariant),
            ("en", invariant),
            ("ru", russian),
            ("fr", french),
        ] {
            let result = render(template, Value::Int(value), culture);
            match expected {
                // The plural rule picked a word the format does not have.
                "!" => assert!(
                    matches!(&result, Err(Error::Format { message, .. })
                        if message.contains("Invalid number of plural parameters")),
                    "{template:?} with {value} in {culture:?}: {result:?}"
                ),
                expected => assert_eq!(
                    result.as_deref().ok(),
                    Some(expected),
                    "{template:?} with {value} in {culture:?}"
                ),
            }
        }
    }
}

/// A complex condition is `cond` syntax that `plural` knows nothing about, but
/// `plural` still gets the format first and treats the two parts as two plural
/// words. Under `en` that happens to give the same answer as the condition
/// would; under `ru`, where two words are not a valid plural form, it is an
/// error. Registering `cond` first would render `big` here.
#[cfg(feature = "plural")]
#[test]
fn plural_swallows_a_complex_condition_before_cond_sees_it() {
    assert_eq!(rendered("{0:>5?big|small}", Value::Int(10), "en"), "small");

    let result = render("{0:>5?big|small}", Value::Int(10), "ru");
    assert!(
        matches!(&result, Err(Error::Format { message, .. })
            if message.contains("Invalid number of plural parameters")),
        "{result:?}"
    );

    // Naming `cond` is how a template asks for the condition.
    assert_eq!(
        rendered("{0:cond:>5?big|small}", Value::Int(10), "ru"),
        "big"
    );
}

/// `plural` only handles numbers and lists, so everything else falls through
/// to `cond` — which is where `{0:a|b}` on a string, a bool or null is decided.
#[test]
fn cond_takes_the_values_plural_declines() {
    for culture in ["", "en", "ru", "fr"] {
        // A non-empty string, and `true`, take the first part.
        assert_eq!(rendered("{0:a|b}", Value::from("x"), culture), "a");
        assert_eq!(rendered("{0:a|b}", Value::Bool(true), culture), "a");
        // An empty string, `false` and null take the second.
        assert_eq!(rendered("{0:a|b}", Value::from(""), culture), "b");
        assert_eq!(rendered("{0:a|b}", Value::Bool(false), culture), "b");
        assert_eq!(rendered("{0:a|b}", Value::Null, culture), "b");
    }
}

/// `choose` never auto-detects, so a format full of separators reaches it only
/// when the placeholder names it.
#[test]
fn choose_is_only_reached_by_name() {
    // Without the name, `plural` takes it: 2 is "other" in English.
    assert_eq!(rendered("{0:(1|2):a|b}", Value::Int(2), "en"), "b");
    // With the name, the option list decides.
    assert_eq!(rendered("{0:choose(1|2):a|b}", Value::Int(2), "en"), "b");
    assert_eq!(rendered("{0:choose(2|1):a|b}", Value::Int(2), "en"), "a");
}

// ---------------------------------------------------------------------------
// Error messages
// ---------------------------------------------------------------------------

fn output_errors(template: &str, value: Value) -> String {
    let smart = SmartFormatter::new(SmartSettings {
        format_error_action: ErrorAction::OutputErrorInResult,
        ..SmartSettings::default()
    });
    smart
        .format(template, &args([value]))
        .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
}

/// .NET's `FormatErrorAction.OutputErrorInResult` writes `innerException
/// .Message` into the result. When the formatter threw a `FormattingException`
/// itself, that message is the full `Error parsing format string: … at {index}`
/// envelope, template and caret line included.
#[test]
fn a_formatting_exception_is_written_with_its_envelope() {
    assert_eq!(
        output_errors("{0:choose(1|2|3):a|b}", Value::Int(1)),
        "Error parsing format string: You must specify at least 3 choices at 17\n\
         {0:choose(1|2|3):a|b}\n\
         -----------------^"
    );
    assert_eq!(
        output_errors("{0:choose(1|2):a|b}", Value::Int(3)),
        "Error parsing format string: \"3\" is not a valid choice, and a \"default\" \
         choice was not supplied at 15\n\
         {0:choose(1|2):a|b}\n\
         ---------------^"
    );
}

#[cfg(feature = "plural")]
#[test]
fn plural_reports_its_word_count_as_the_index() {
    // .NET passes `pluralWords.Count - 1`, which is a count and not an offset
    // into the template: the caret lands wherever the count happens to point.
    assert_eq!(
        output_errors("{0:plural:a|b|c|d|e}", Value::Int(1)),
        "Error parsing format string: Invalid number of plural parameters in \
         PluralLocalizationFormatter at 4\n\
         {0:plural:a|b|c|d|e}\n\
         ----^"
    );
    assert_eq!(
        output_errors("{0:plural:a|b}", Value::from("text")),
        "Error parsing format string: Formatter named 'plural' can format numbers \
         and IEnumerables, but the argument was of type 'System.String' at 0\n\
         {0:plural:a|b}\n\
         ^"
    );
}

/// Where .NET throws something that is *not* a `FormattingException` — a plain
/// `FormatException` from `choose` and `cond`, an `ArgumentException` from
/// `plural`'s language lookup — the evaluator adds the envelope only while
/// rethrowing. `OutputErrorInResult` writes the bare inner message. Probed:
/// all four of these come out with no envelope in 3.6.1.
#[test]
fn a_plain_exception_is_written_bare() {
    assert_eq!(
        output_errors("{0:choose(1|2):a}", Value::Int(1)),
        "Formatter named 'choose' requires at least 2 format options."
    );
    assert_eq!(
        output_errors("{0:cond:Yes}", Value::Int(1)),
        "Formatter named 'cond' requires at least 2 format parameters."
    );
    assert_eq!(
        output_errors("{0:cond:>10?a|>20?b}", Value::Int(0)),
        "Specified argument was out of the range of valid values. (Parameter 'index')"
    );
    #[cfg(feature = "plural")]
    assert_eq!(
        output_errors("{0:plural(xx):a|b}", Value::Int(1)),
        "IsoLangToDelegate not found for xx (Parameter 'twoLetterIsoLanguageName')"
    );
}

/// The index in the envelope counts UTF-16 code units of the template, as .NET
/// does — not bytes and not characters. An emoji before the placeholder is two
/// units, `é` is one.
#[test]
fn the_error_index_counts_utf16_code_units() {
    assert_eq!(
        output_errors("éé{0:choose(1|2|3):a|b}", Value::Int(1)),
        "ééError parsing format string: You must specify at least 3 choices at 19\n\
         éé{0:choose(1|2|3):a|b}\n\
         -------------------^"
    );
    // The emoji is one `char` but two UTF-16 units, so the index is the same.
    assert_eq!(
        output_errors("😀{0:choose(1|2|3):a|b}", Value::Int(1)),
        "😀Error parsing format string: You must specify at least 3 choices at 19\n\
         😀{0:choose(1|2|3):a|b}\n\
         -------------------^"
    );
    assert_eq!(
        output_errors("😀{nosuchselector}", Value::Int(1)),
        "😀Error parsing format string: No source extension could handle the selector \
         named \"nosuchselector\" at 3\n\
         😀{nosuchselector}\n\
         ---^"
    );
}

// ---------------------------------------------------------------------------
// Cultures
// ---------------------------------------------------------------------------

#[test]
fn a_culture_can_be_named_instead_of_passed() {
    let smart = SmartFormatter::default();
    let value = args([Value::Float(1234.5)]);

    assert_eq!(
        smart
            .format_with_culture_name("{0:N2}", &value, "de-DE")
            .unwrap(),
        "1.234,50"
    );
    // .NET matches culture names case-insensitively.
    assert_eq!(
        smart
            .format_with_culture_name("{0:N2}", &value, "DE-de")
            .unwrap(),
        "1.234,50"
    );
    // The empty name is the invariant culture, as in .NET.
    assert_eq!(
        smart
            .format_with_culture_name("{0:N2}", &value, "")
            .unwrap(),
        smart.format("{0:N2}", &value).unwrap()
    );
}

#[test]
fn an_unknown_culture_name_is_a_clear_error() {
    let smart = SmartFormatter::default();
    let value = args([Value::Int(1)]);

    let error = smart
        .format_with_culture_name("{0}", &value, "xx-XX")
        .unwrap_err();
    assert!(
        matches!(&error, Error::UnknownCulture { name } if name == "xx-XX"),
        "{error:?}"
    );
    assert!(error.to_string().contains("xx-XX"), "{error}");
}

#[test]
fn a_parsed_template_can_be_rendered_with_a_named_culture() {
    let smart = SmartFormatter::default();
    let parsed = smart.parse("{0:N2}").unwrap();
    let value = args([Value::Float(1234.5)]);

    assert_eq!(
        smart
            .format_parsed_with_culture_name(&parsed, &value, "de-DE")
            .unwrap(),
        "1.234,50"
    );
    assert!(smart
        .format_parsed_with_culture_name(&parsed, &value, "xx-XX")
        .is_err());
}
