//! The extensions .NET's `CreateDefaultSmartFormat` leaves out, wired into a
//! whole [`SmartFormatter`] and driven through the public API.
//!
//! `TimeFormatter`, `LocalizationFormatter`, `PersistentVariablesSource` and
//! `GlobalVariablesSource` all have to be registered by hand, here as in .NET.
//! Each one's behaviour is unit-tested inside its own module against a registry
//! holding only it; these tests are about the wiring — that registration puts
//! the extension where .NET's `WellKnownExtensionTypes` ranks it, and that a
//! template then renders through the full pipeline.
//!
//! `tests/extensions.rs` covers the same ground for the *default* registry.

use smartformat::sources::variables::{self, GlobalVariablesSource, PersistentVariablesSource};
use smartformat::{ErrorAction, HashMapLocalizationProvider, SmartFormatter, SmartSettings, Value};

/// The .NET default of `FormatErrorAction.ThrowError`, so a formatting error
/// surfaces instead of being written into the output.
fn throwing() -> SmartFormatter {
    SmartFormatter::new(SmartSettings {
        format_error_action: ErrorAction::Error,
        ..SmartSettings::default()
    })
}

fn formatter_names(smart: &SmartFormatter) -> Vec<&str> {
    smart.formatters().iter().map(|f| f.name()).collect()
}

// ---------------------------------------------------------------------------
// TimeFormatter
// ---------------------------------------------------------------------------

/// A `TimeSpan` through the whole pipeline: parse, resolve the selector, pick
/// the `time` formatter by name, and write the units in the language the
/// options name — with the culture of the call deciding when the options do
/// not.
#[cfg(feature = "time")]
#[test]
fn a_time_span_renders_through_the_full_pipeline() {
    use jiff::SignedDuration;
    use smartformat::TimeFormatter;

    let mut smart = throwing();
    smart.formatters_mut().add(Box::new(TimeFormatter::new()));

    // .NET's rank for it is 4000: after `cond`, before `ismatch` when that
    // formatter is built and before `isnull` when it is not.
    let names = formatter_names(&smart);
    let time = names
        .iter()
        .position(|name| *name == "time")
        .expect("added");
    assert_eq!(names[time - 1], "cond");

    let span = SignedDuration::from_secs(2 * 24 * 60 * 60 + 3 * 60 * 60 + 4 * 60 + 5);
    let args = Value::List(vec![Value::TimeSpan(span)]);

    // The default options: every non-zero unit from days down to seconds.
    assert_eq!(
        smart.format("{0:time:}", &args).unwrap(),
        "2 days 3 hours 4 minutes 5 seconds"
    );
    // Options pick the range and the abbreviation.
    assert_eq!(
        smart.format("{0:time:hours minutes abbr}", &args).unwrap(),
        "51h 4m"
    );
    // The language comes from the culture of the call …
    assert_eq!(
        smart
            .format_with_culture_name("{0:time:}", &args, "de-DE")
            .unwrap(),
        "2 Tage 3 Stunden 4 Minuten 5 Sekunden"
    );
    // … unless the options name one. The language only *is* the options when
    // the format is non-empty; `{0:time(es):}` is the 2.x spelling, where `es`
    // is the format and the language still comes from the call.
    assert_eq!(
        smart
            .format_with_culture_name("{0:time(es):hours minutes}", &args, "de-DE")
            .unwrap(),
        "51 horas 4 minutos"
    );

    // A nested format hands the parts to the `list` formatter, which is what
    // the whole registry is needed for. The leading space is load bearing:
    // .NET drops the first item of the format, whatever it is.
    assert_eq!(
        smart.format("{0:time: {:list:|, | and }}", &args).unwrap(),
        "2 days, 3 hours, 4 minutes and 5 seconds"
    );
}

// ---------------------------------------------------------------------------
// LocalizationFormatter
// ---------------------------------------------------------------------------

/// A localized template, and the culture chain a lookup walks: specific culture
/// → neutral parent → invariant.
#[test]
fn a_localized_template_falls_back_along_the_culture_chain() {
    let provider = HashMapLocalizationProvider::from_triples([
        // Only the invariant table has this one, so every culture finds it.
        ("", "Currency", "money"),
        ("", "Welcome", "Welcome, {Name}"),
        ("es", "Welcome", "Bienvenido, {Name}"),
        ("es-MX", "Welcome", "Qué onda, {Name}"),
    ]);

    let mut smart = throwing();
    smart.register_localization(Box::new(provider));

    // .NET's rank for it is 8000: after `isnull`, before `t` and `choose`.
    let names = formatter_names(&smart);
    let localization = names.iter().position(|name| *name == "L").expect("added");
    assert_eq!(names[localization - 1], "isnull");
    assert_eq!(names[localization + 1], "choose");

    let person = Value::Map(
        [("Name".to_owned(), Value::from("Ana"))]
            .into_iter()
            .collect(),
    );

    // The localized string is rendered as a template, against the current
    // scope, so its `{Name}` resolves like any other placeholder.
    let template = "{:L:Welcome}";
    assert_eq!(smart.format(template, &person).unwrap(), "Welcome, Ana");
    assert_eq!(
        smart
            .format_with_culture_name(template, &person, "es")
            .unwrap(),
        "Bienvenido, Ana"
    );
    // A specific culture takes its own entry …
    assert_eq!(
        smart
            .format_with_culture_name(template, &person, "es-MX")
            .unwrap(),
        "Qué onda, Ana"
    );
    // … and falls back to the language when it has none.
    assert_eq!(
        smart
            .format_with_culture_name(template, &person, "es-ES")
            .unwrap(),
        "Bienvenido, Ana"
    );
    // A culture with no table at all reaches the invariant entry.
    assert_eq!(
        smart
            .format_with_culture_name("{:L:Currency}", &person, "de-DE")
            .unwrap(),
        "money"
    );
    // The options name the culture, overriding the call's.
    assert_eq!(
        smart.format("{:L(es):Welcome}", &person).unwrap(),
        "Bienvenido, Ana"
    );

    // A key nothing translates is an error, and it quotes the key.
    let error = smart
        .format("{:L:Farewell}", &person)
        .expect_err("no entry for 'Farewell'");
    assert!(
        error
            .to_string()
            .contains("No localized string found for 'Farewell'"),
        "{error}"
    );
}

/// .NET keeps one `SmartSettings.Localization.LocalizationProvider`, so
/// registering a second provider replaces the first rather than adding a second
/// formatter named `L` that name lookup would never reach.
#[test]
fn registering_a_second_provider_replaces_the_first() {
    let mut smart = throwing();
    smart.register_localization(Box::new(HashMapLocalizationProvider::from_triples([(
        "", "Hi", "Hello",
    )])));
    smart.register_localization(Box::new(HashMapLocalizationProvider::from_triples([(
        "", "Hi", "Hiya",
    )])));

    assert_eq!(
        formatter_names(&smart)
            .iter()
            .filter(|n| **n == "L")
            .count(),
        1
    );
    assert_eq!(smart.format("{:L:Hi}", &Value::Null).unwrap(), "Hiya");
}

// ---------------------------------------------------------------------------
// The variables sources
// ---------------------------------------------------------------------------

/// A group registered on the source resolves from a template that was passed
/// no arguments at all, and a nested group extends the selector chain.
#[test]
fn a_variables_group_resolves_from_a_template() {
    let mut source = PersistentVariablesSource::new();
    source.add(
        "global",
        variables::group([
            ("user", Value::from("Joe")),
            ("visits", Value::from(3i64)),
            (
                "theme",
                Value::Map(variables::group([("name", Value::from("dark"))])),
            ),
        ]),
    );
    // A group whose name a source in the default registry would also answer,
    // which is how the position is observable at all.
    source.add("Length", variables::group([("of", Value::from("rope"))]));

    let mut smart = throwing();
    smart.register_variables(source);

    assert_eq!(smart.sources().len(), 5);
    // .NET ranks the persistent source at 2000, ahead of every source in the
    // default registry — so a group named `Length` answers `{0.Length}` on a
    // string before `StringSource` is asked (probed against 3.6.1).
    assert_eq!(
        smart
            .format("{0.Length.of}", &Value::List(vec![Value::from("abc")]))
            .unwrap(),
        "rope"
    );

    assert_eq!(
        smart
            .format(
                "{global.user} has {global.visits} visits in {global.theme.name}",
                &Value::Null
            )
            .unwrap(),
        "Joe has 3 visits in dark"
    );

    // A variable is an ordinary value, so every formatter works on it.
    assert_eq!(
        smart
            .format("{global.visits:cond:none|one|many}", &Value::Null)
            .unwrap(),
        "many"
    );

    // Group names are matched ordinally whatever the settings say, as .NET's
    // plain `Dictionary` does.
    let error = smart
        .format("{GLOBAL.user}", &Value::Null)
        .expect_err("group names are ordinal");
    assert!(error.to_string().contains("GLOBAL"), "{error}");
}

/// The shared handle is the only way to change variables after registration,
/// and it is ranked ahead of a persistent source.
#[test]
fn shared_variables_stay_writable_after_registration() {
    let variables = GlobalVariablesSource::new();

    let mut smart = throwing();
    smart.register_global_variables(variables.clone());
    smart.register_variables(PersistentVariablesSource::from_iter([(
        "app",
        variables::group([("name", Value::from("Acme"))]),
    )]));

    variables.add("session", variables::group([("id", Value::from(7i64))]));

    assert_eq!(
        smart
            .format("{app.name}#{session.id}", &Value::Null)
            .unwrap(),
        "Acme#7"
    );

    // .NET ranks the global source at 1000 and the persistent one at 2000, so
    // the global one answers first: a group registered on both is the shared
    // one.
    variables.add("app", variables::group([("name", Value::from("Shared"))]));
    assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Shared");
}

// ---------------------------------------------------------------------------
// Date conditions
// ---------------------------------------------------------------------------

/// `ConditionalFormatter`'s date branch compares the value against
/// `SmartSettings::now`, which stands in for .NET's `SystemTime.Now()`.
#[cfg(feature = "time")]
#[test]
fn a_date_condition_reads_the_pinned_now() {
    use jiff::civil::date;

    let now = date(2026, 7, 31).at(12, 0, 0, 0);
    let smart = SmartFormatter::new(SmartSettings {
        format_error_action: ErrorAction::Error,
        now: Some(now),
        ..SmartSettings::default()
    });

    let render = |template: &str, value: jiff::civil::DateTime| {
        smart
            .format(template, &Value::List(vec![Value::DateTime(value)]))
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    };

    let earlier = date(2026, 7, 30).at(23, 59, 0, 0);
    let later = date(2026, 8, 1).at(0, 0, 0, 0);
    let today = date(2026, 7, 31).at(23, 59, 0, 0);

    // Two parts: past — which includes now itself — or future.
    assert_eq!(render("{0:cond:Past|Future}", earlier), "Past");
    assert_eq!(render("{0:cond:Past|Future}", now), "Past");
    assert_eq!(render("{0:cond:Past|Future}", later), "Future");

    // Three parts: the middle one is today, whatever the time of day.
    assert_eq!(render("{0:cond:Past|Today|Future}", earlier), "Past");
    assert_eq!(render("{0:cond:Past|Today|Future}", today), "Today");
    assert_eq!(render("{0:cond:Past|Today|Future}", later), "Future");

    // The formatter auto-detects, so the `cond` name is optional.
    assert_eq!(render("{0:Past|Today|Future}", today), "Today");

    // The picked part is rendered as a format, against the date.
    assert_eq!(
        render("{0:cond:was {:d}|is {:d}|will be {:d}}", today),
        "is 07/31/2026"
    );
}

/// A `TimeSpan` picks negative / zero / positive, with no clock involved.
#[cfg(feature = "time")]
#[test]
fn a_time_span_condition_needs_no_clock() {
    use jiff::SignedDuration;

    let smart = throwing();
    let render = |template: &str, value: SignedDuration| {
        smart
            .format(template, &Value::List(vec![Value::TimeSpan(value)]))
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    };

    let template = "{0:cond:overdue|due now|left}";
    assert_eq!(render(template, SignedDuration::from_hours(-1)), "overdue");
    assert_eq!(render(template, SignedDuration::ZERO), "due now");
    assert_eq!(render(template, SignedDuration::from_hours(1)), "left");
}
