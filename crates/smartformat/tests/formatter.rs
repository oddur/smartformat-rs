//! Ported from SmartFormat.NET
//! `src/SmartFormat.Tests/Core/FormatterTests.cs`. Cases that depend on .NET
//! reflection over anonymous types use maps instead, and cases that need a
//! `FormatDelegate` to raise a formatting error use a missing selector.

use std::collections::BTreeMap;

use smartformat::error::Error;
use smartformat::settings::{CaseSensitivity, ErrorAction, SmartSettings};
use smartformat::{SmartFormatter, Value};

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

fn format(template: &str, args: &Value) -> String {
    SmartFormatter::default()
        .format(template, args)
        .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
}

fn with_format_error_action(action: ErrorAction) -> SmartFormatter {
    SmartFormatter::new(SmartSettings {
        format_error_action: action,
        ..SmartSettings::default()
    })
}

// ----- positional arguments ----------------------------------------------

#[test]
fn formatter_with_numeric_params() {
    let values = args([Value::Int(0), Value::Int(1)]);
    assert_eq!(format("ABC{0}{1}DEF", &values), "ABC01DEF");
}

#[test]
fn formatter_with_string_params() {
    assert_eq!(
        format("Name: {0}", &args([Value::from("Joe")])),
        "Name: Joe"
    );
}

#[test]
fn formatter_pure_literal_no_args() {
    let smart = SmartFormatter::default();
    let parsed = smart.parse("ABC").unwrap();
    assert_eq!(parsed.items.len(), 1);
    assert_eq!(smart.format_parsed(&parsed, &args([])).unwrap(), "ABC");
}

#[test]
fn formatter_with_null_args() {
    let values = args([Value::Null, Value::Null]);
    assert_eq!(format("a{0}b{1}c", &values), "abc");
}

#[test]
fn formatter_reuses_a_parsed_format() {
    let smart = SmartFormatter::default();
    let parsed = smart.parse("{0:D3}").unwrap();
    for i in 0..3i64 {
        let rendered = smart
            .format_parsed(&parsed, &args([Value::Int(i)]))
            .unwrap();
        assert_eq!(rendered, format!("00{i}"));
    }
}

#[test]
fn positional_index_out_of_range_is_an_error() {
    let error = SmartFormatter::default()
        .format("{1}", &args([Value::Int(1)]))
        .unwrap_err();
    assert!(matches!(error, Error::Format { .. }), "{error}");
}

#[test]
fn a_single_value_is_the_only_argument() {
    assert_eq!(format("{0}", &Value::from("solo")), "solo");
}

// ----- map (dictionary) arguments ----------------------------------------

#[test]
fn formatter_with_map_args() {
    let data = map([
        ("Greeting", Value::from("Hello")),
        ("Name", Value::from("Joe")),
    ]);
    assert_eq!(format("{Greeting}, {Name}!", &data), "Hello, Joe!");
}

#[test]
fn map_selectors_are_case_sensitive_by_default() {
    let data = map([("Greeting", Value::from("Hello"))]);
    let error = SmartFormatter::default()
        .format("{greeting}", &data)
        .unwrap_err();
    assert!(matches!(error, Error::Format { .. }), "{error}");
}

#[test]
fn map_selectors_can_be_case_insensitive() {
    let smart = SmartFormatter::new(SmartSettings {
        case_sensitive: CaseSensitivity::CaseInsensitive,
        ..SmartSettings::default()
    });
    let data = map([("Greeting", Value::from("Hello"))]);
    assert_eq!(smart.format("{greeting}", &data).unwrap(), "Hello");
}

#[test]
fn nested_maps_are_selected_with_dots() {
    let data = map([(
        "Person",
        map([("Address", map([("City", Value::from("London"))]))]),
    )]);
    assert_eq!(format("{Person.Address.City}", &data), "London");
}

#[test]
fn list_items_are_selected_by_index() {
    let data = map([(
        "Items",
        Value::List(vec![Value::from("a"), Value::from("b")]),
    )]);
    assert_eq!(format("{Items.1}-{Items[0]}", &data), "b-a");
}

#[test]
fn string_selectors_are_supported() {
    let data = map([("Name", Value::from("  alice  "))]);
    assert_eq!(format("{Name.Trim.ToUpper}", &data), "ALICE");
    assert_eq!(format("{Name.Trim.Length}", &data), "5");
}

// ----- nesting ------------------------------------------------------------

#[test]
fn nested_placeholders_nested_scope_1() {
    // A nested template can reach into the enclosing scopes: {City} comes from
    // Address, {FirstName} from Person.
    let data = map([
        (
            "Person",
            map([
                ("FirstName", Value::from("John")),
                ("LastName", Value::from("Long")),
            ]),
        ),
        ("Address", map([("City", Value::from("London"))])),
    ]);

    let rendered = format(
        r"{Person:{Address:City\: {City}, Name\: {FirstName}}}",
        &data,
    );
    assert_eq!(rendered, "City: London, Name: John");
}

#[test]
fn nested_placeholders_nested_scope_2() {
    // "{}" and "{:Child3}" use the value of the enclosing placeholder, while
    // {Child4} is resolved in the outer scope.
    let data = map([
        (
            "Child1",
            map([("Child2", map([("Child3", Value::from("Child3"))]))]),
        ),
        ("Child4", Value::from("Child4")),
    ]);

    let rendered = format("{Child1.Child2.Child3:{}{:Child3}{Child4}}", &data);
    assert_eq!(rendered, "Child3Child3Child4");
}

#[test]
fn nameless_placeholder_repeats_the_current_value() {
    let data = map([("Name", Value::from("Alice"))]);
    assert_eq!(format("{Name:{}-{}}", &data), "Alice-Alice");
}

#[test]
fn a_format_with_a_specifier_is_not_a_nested_format() {
    let data = map([("Count", Value::Int(3))]);
    assert_eq!(format("{Count:D2}", &data), "03");
}

// ----- alignment ----------------------------------------------------------

#[test]
fn alignment_pads_with_spaces() {
    let values = args([Value::from("ab")]);
    assert_eq!(format("[{0,6}]", &values), "[    ab]");
    assert_eq!(format("[{0,-6}]", &values), "[ab    ]");
    assert_eq!(format("[{0,0}]", &values), "[ab]");
}

#[test]
fn alignment_never_truncates() {
    let values = args([Value::from("abcdefghij")]);
    assert_eq!(format("[{0,3}]", &values), "[abcdefghij]");
}

#[test]
fn alignment_is_applied_after_formatting() {
    let values = args([Value::Int(42)]);
    assert_eq!(format("[{0,10:D5}]", &values), "[     00042]");
    assert_eq!(format("[{0,-10:D5}]", &values), "[00042     ]");
}

#[test]
fn alignment_pads_a_null_value() {
    let values = args([Value::Null]);
    assert_eq!(format("[{0,4}]", &values), "[    ]");
}

#[test]
fn nested_placeholders_inherit_the_alignment() {
    let data = map([("A", map([("B", Value::from("deep"))]))]);
    assert_eq!(format("{A,10:{B}}", &data), "      deep");
}

// ----- error actions ------------------------------------------------------

#[test]
fn formatter_throws_errors() {
    let smart = with_format_error_action(ErrorAction::Error);
    let error = smart
        .format("--{Missing}--", &map([("Name", Value::from("Joe"))]))
        .unwrap_err();
    match error {
        Error::Format { message, .. } => assert!(message.contains("Missing"), "{message}"),
        other => panic!("expected a formatting error, got {other}"),
    }
}

#[test]
fn formatter_ignores_errors() {
    let smart = with_format_error_action(ErrorAction::Ignore);
    let rendered = smart
        .format(
            "--{Missing}--{Name}--",
            &map([("Name", Value::from("Joe"))]),
        )
        .unwrap();
    assert_eq!(rendered, "----Joe--");
}

#[test]
fn formatter_outputs_errors_in_the_result() {
    let smart = with_format_error_action(ErrorAction::OutputErrorInResult);
    let rendered = smart
        .format("--{Missing}--", &map([("Name", Value::from("Joe"))]))
        .unwrap();
    assert!(rendered.starts_with("--"), "{rendered}");
    assert!(rendered.ends_with("--"), "{rendered}");
    assert!(rendered.contains("Missing"), "{rendered}");
}

#[test]
fn formatter_maintains_tokens() {
    let smart = with_format_error_action(ErrorAction::MaintainTokens);
    let rendered = smart
        .format(
            "--{Missing}--{Object.Thing}--",
            &map([("Name", Value::from("Joe"))]),
        )
        .unwrap();
    assert_eq!(rendered, "--{Missing}--{Object.Thing}--");
}

#[test]
fn parse_errors_honor_the_parse_error_action() {
    let template = "{0";

    let strict = SmartFormatter::default();
    assert!(matches!(
        strict.format(template, &args([Value::Int(1)])).unwrap_err(),
        Error::Parse { .. }
    ));

    let lenient = SmartFormatter::new(SmartSettings {
        parse_error_action: ErrorAction::MaintainTokens,
        ..SmartSettings::default()
    });
    assert_eq!(
        lenient.format(template, &args([Value::Int(1)])).unwrap(),
        "{0"
    );
}

#[test]
fn not_existing_formatter_name_is_an_error() {
    let error = SmartFormatter::default()
        .format("{0:not_existing_formatter_name:}", &args([Value::Int(1)]))
        .unwrap_err();
    match error {
        Error::Format { message, .. } => {
            assert!(message.contains("not_existing_formatter_name"), "{message}")
        }
        other => panic!("expected a formatting error, got {other}"),
    }
}

#[test]
fn unsupported_format_specs_are_distinguishable() {
    let error = SmartFormatter::default()
        .format("{0:#,##0.00}", &args([Value::Float(1.5)]))
        .unwrap_err();
    match error {
        Error::UnsupportedSpec { spec, .. } => assert_eq!(spec, "#,##0.00"),
        other => panic!("expected an unsupported-spec error, got {other}"),
    }
}

#[test]
fn default_formatting_of_a_map_is_an_error() {
    let data = map([("Person", map([("Name", Value::from("Joe"))]))]);
    let error = SmartFormatter::default()
        .format("{Person}", &data)
        .unwrap_err();
    match error {
        Error::Format { message, .. } => assert!(message.contains("map"), "{message}"),
        other => panic!("expected a formatting error, got {other}"),
    }
}

// ----- the nullable operator ---------------------------------------------

#[test]
fn nullable_operator_short_circuits_to_empty_output() {
    let data = map([("City", Value::Null), ("Name", Value::from("Alice"))]);

    assert_eq!(format("{City?.Length}", &data), "");
    assert_eq!(format("{City?.Length?.Nope}", &data), "");
    assert_eq!(format("{Name?.Length}", &data), "5");
}

#[test]
fn nullable_operator_covers_a_missing_key() {
    let data = map([("Person", map([("Name", Value::from("Joe"))]))]);
    assert_eq!(format("{Person?.Nope}", &data), "");
}

#[test]
fn a_member_of_null_without_the_nullable_operator_is_an_error() {
    let data = map([("City", Value::Null)]);
    let error = SmartFormatter::default()
        .format("{City.Length}", &data)
        .unwrap_err();
    assert!(matches!(error, Error::Format { .. }), "{error}");
}

// ----- thread safety ------------------------------------------------------

#[test]
fn formatter_is_shareable_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SmartFormatter>();

    let smart = SmartFormatter::default();
    std::thread::scope(|scope| {
        for i in 0..4i64 {
            let smart = &smart;
            scope.spawn(move || {
                let rendered = smart.format("{0:D3}", &args([Value::Int(i)])).unwrap();
                assert_eq!(rendered, format!("00{i}"));
            });
        }
    });
}
