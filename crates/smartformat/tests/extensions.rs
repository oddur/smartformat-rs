//! What the default registry of formatter extensions does as a whole: which
//! formatters are in it, in which order, and what that order decides.
//!
//! Every expectation here was probed against the pinned SmartFormat.NET 3.6.1
//! package (`Smart.CreateDefaultSmartFormat()`), not read off the upstream
//! `main` branch. The per-formatter behaviour is unit-tested inside each
//! extension module, against a registry holding only that formatter; these
//! tests are about what happens when they are all registered together.

use smartformat::{CaseSensitivity, Error, ErrorAction, SmartFormatter, SmartSettings, Value};

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
/// so the registry has to come out that way, minus whatever a feature switches
/// off. `t` is not in the list: .NET leaves `TemplateFormatter` out of
/// `CreateDefaultSmartFormat`, and so does this port until a template is
/// registered.
#[test]
fn the_default_registry_is_in_dotnet_order() {
    let smart = SmartFormatter::default();

    let names: Vec<&str> = smart.formatters().iter().map(|f| f.name()).collect();
    let expected: Vec<&str> = [
        ("list", true),
        ("plural", cfg!(feature = "plural")),
        ("cond", true),
        ("ismatch", cfg!(feature = "regex-formatters")),
        ("isnull", true),
        ("choose", true),
        ("substr", true),
        ("d", true),
    ]
    .into_iter()
    .filter(|(_, registered)| *registered)
    .map(|(name, _)| name)
    .collect();
    assert_eq!(names, expected);
}

/// Registering a template adds the formatter .NET's default registry leaves
/// out, at .NET's rank for it: after `isnull`, before `choose`.
#[test]
fn a_registered_template_adds_the_formatter_at_its_dotnet_rank() {
    let mut smart = SmartFormatter::default();
    smart
        .register_template("firstLast", "{First} {Last}")
        .unwrap();

    let names: Vec<&str> = smart.formatters().iter().map(|f| f.name()).collect();
    let template = names
        .iter()
        .position(|name| *name == "t")
        .expect("registered");
    assert_eq!(names[template - 1], "isnull");
    assert_eq!(names[template + 1], "choose");

    // A second template joins the formatter that is already there.
    smart
        .register_template("lastFirst", "{Last}, {First}")
        .unwrap();
    assert_eq!(
        smart
            .formatters()
            .iter()
            .filter(|f| f.name() == "t")
            .count(),
        1
    );
    let person = Value::Map(
        [
            ("First".to_owned(), Value::from("Scott")),
            ("Last".to_owned(), Value::from("Rippey")),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        smart
            .format("{:t:firstLast} / {:t:lastFirst}", &person)
            .unwrap(),
        "Scott Rippey / Rippey, Scott"
    );
}

/// The template registry is matched with the formatter's own case sensitivity,
/// which .NET fixes when the extension is initialized.
#[test]
fn template_names_follow_the_formatters_case_sensitivity() {
    let mut smart = SmartFormatter::new(SmartSettings {
        case_sensitive: CaseSensitivity::CaseInsensitive,
        ..SmartSettings::default()
    });
    smart
        .register_template("firstLast", "{First} {Last}")
        .unwrap();

    let person = Value::Map(
        [
            ("First".to_owned(), Value::from("Scott")),
            ("Last".to_owned(), Value::from("Rippey")),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        smart.format("{:t:FIRSTLAST}", &person).unwrap(),
        "Scott Rippey"
    );
    // .NET's `Dictionary.Add` throws rather than overwriting, whatever the
    // comparer says the two names are.
    assert!(smart.register_template("FIRSTLAST", "{Last}").is_err());
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

/// `list` outranks every other formatter, so a `|`-separated format on a list
/// is a list even where `plural` or `cond` would have taken the same text for
/// a number or a string. Probed in 3.6.1: `{0:one|many}` over `["x", "y"]` is
/// item format `one`, spacer `many`.
#[test]
fn list_wins_over_plural_and_cond_for_lists() {
    let items = Value::List(vec![Value::from("x"), Value::from("y")]);
    assert_eq!(rendered("{0:one|many}", items.clone(), "en"), "xmanyy");
    // The same format on a string is `cond`'s, and on a number `plural`'s.
    assert_eq!(rendered("{0:one|many}", Value::from("s"), "en"), "one");
    #[cfg(feature = "plural")]
    assert_eq!(rendered("{0:one|many}", Value::Int(1), "en"), "one");
}

/// `{Index}` is `ListSource`'s selector and the collection index it reads is
/// the `list` *formatter*'s, so the two halves of the extension only work
/// together once both are registered. An item is not a list, so `{Index}`
/// inside the item format resolves against the list one level up.
#[test]
fn the_index_selector_counts_the_list_being_formatted() {
    let items = Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]);
    assert_eq!(
        rendered("{0:list:{} = {Index}|, }", items, "en"),
        "a = 0, b = 1, c = 2"
    );
    // Nested lists count independently, and the outer index is restored.
    let nested = Value::List(vec![
        Value::List(vec![Value::from("a"), Value::from("b")]),
        Value::List(vec![Value::from("c")]),
    ]);
    assert_eq!(
        rendered("{0:list:{Index}:{:list:{}{Index}|,}|; }", nested, "en"),
        "0:a0,b1; 1:c0"
    );
    // Outside any list, `{Index}` is -1 (.NET
    // `ListFormatter.CollectionIndex`'s initial value). It only answers as the
    // first selector of a placeholder, so this one reads the argument list.
    assert_eq!(rendered("{Index}", Value::from("anything"), "en"), "-1");
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
