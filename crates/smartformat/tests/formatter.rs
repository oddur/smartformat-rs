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
fn an_exactly_spelled_key_wins_over_other_case_variants() {
    let smart = SmartFormatter::new(SmartSettings {
        case_sensitive: CaseSensitivity::CaseInsensitive,
        ..SmartSettings::default()
    });
    // "NAME" sorts before "Name" in the map, so only the exact-match-first
    // lookup finds the key the template actually spells.
    let data = map([
        ("NAME", Value::from("upper")),
        ("Name", Value::from("exact")),
        ("name", Value::from("lower")),
    ]);

    assert_eq!(smart.format("{Name}", &data).unwrap(), "exact");
    assert_eq!(smart.format("{name}", &data).unwrap(), "lower");
    assert_eq!(smart.format("{NAME}", &data).unwrap(), "upper");
    // No exact spelling: any case-variant answers.
    assert_eq!(smart.format("{nAmE}", &data).unwrap(), "upper");
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

/// A deliberate divergence: unterminated formatter options make .NET index past
/// the end of the format string and throw `IndexOutOfRangeException`
/// (`err-unterminated-formatter-options` pins that), so there is no .NET message
/// to copy; we report the ordinary missing-closing-brace parse error.
#[test]
fn unterminated_formatter_options_are_a_parse_error() {
    for template in ["{0:d(", r"{0:d(a\"] {
        let error = SmartFormatter::default()
            .format(template, &args([Value::Int(5)]))
            .unwrap_err();
        match error {
            Error::Parse { errors } => assert!(
                errors.iter().any(|e| e.message.contains("closing brace")),
                "{template}: {errors:?}"
            ),
            other => panic!("expected a parse error for {template}, got {other}"),
        }
    }
}

/// A deliberate divergence: .NET writes the CLR type name (`System.Object[]`)
/// here, which is never a useful rendering, so we fail loudly instead.
#[test]
fn default_formatting_of_a_list_is_an_error() {
    let data = map([(
        "Items",
        Value::List(vec![Value::from("a"), Value::from("b")]),
    )]);
    let error = SmartFormatter::default()
        .format("{Items}", &data)
        .unwrap_err();
    match error {
        Error::Format { message, .. } => assert_eq!(
            message,
            "Default formatting of a list is not supported; use a formatter such as \"list\""
        ),
        other => panic!("expected a formatting error, got {other}"),
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
fn nullable_operator_anywhere_in_the_chain_short_circuits() {
    // .NET 3.6.1 `Source.HasNullableOperator` looks at *every* selector of the
    // placeholder, so the `?.` on the last selector already covers the null
    // `City` two selectors earlier.
    let data = map([("City", Value::Null), ("Name", Value::from("Alice"))]);

    assert_eq!(format("{City.Length?.Nope}", &data), "");
    assert_eq!(format("{City.Nope?.Deep}", &data), "");
    // The empty result still takes the placeholder's alignment.
    assert_eq!(format("[{City.Length?.Nope,6}]", &data), "[      ]");

    // The short circuit only fires on a null value: `Name.Length` is 5, so
    // `Nope` still has nothing to resolve against.
    let error = SmartFormatter::default()
        .format("{Name.Length?.Nope}", &data)
        .unwrap_err();
    assert!(matches!(error, Error::Format { .. }), "{error}");
}

#[test]
fn nullable_operator_does_not_cover_a_missing_key() {
    // .NET's `DictionarySource` null-guards a null *value*, not a missing key:
    // a key that is not in a non-null map is unhandled, and unhandled is an
    // error even with `?.` in the chain.
    let data = map([("Person", map([("Name", Value::from("Joe"))]))]);
    let error = SmartFormatter::default()
        .format("{Person?.Nope}", &data)
        .unwrap_err();
    match error {
        Error::Format { message, .. } => assert!(message.contains("\"Nope\""), "{message}"),
        other => panic!("expected a formatting error, got {other}"),
    }

    // Neither a longer chain nor a missing first selector changes that.
    let smart = SmartFormatter::default();
    assert!(smart.format("{Person?.Nope?.Deep}", &data).is_err());
    assert!(smart.format("{Missing?.Name}", &data).is_err());

    // A null map member is covered, though.
    let nullable = map([("Person", Value::Null)]);
    assert_eq!(format("{Person?.Nope}", &nullable), "");
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

// ----- settings the goldens cannot reach ----------------------------------

/// An auto-detecting extension that always wins, so it is visible whenever the
/// registry consults it.
#[derive(Debug)]
struct ShoutFormatter;

impl smartformat::formatter::Formatter for ShoutFormatter {
    fn name(&self) -> &str {
        "shout"
    }

    fn try_evaluate_format(
        &self,
        info: &mut smartformat::formatter::FormattingInfo<'_>,
    ) -> Result<bool, Error> {
        info.write("SHOUT");
        Ok(true)
    }
}

#[test]
fn string_format_compatibility_runs_only_the_default_formatter() {
    let compat = SmartSettings {
        string_format_compatibility: true,
        ..SmartSettings::default()
    };

    let mut shouty = SmartFormatter::new(compat.clone());
    shouty.formatters_mut().insert(0, Box::new(ShoutFormatter));
    assert_eq!(shouty.format("{0}", &args([Value::Int(7)])).unwrap(), "7");

    // Without the setting the auto-detecting extension is consulted first.
    let mut normal = SmartFormatter::default();
    normal.formatters_mut().insert(0, Box::new(ShoutFormatter));
    assert_eq!(
        normal.format("{0}", &args([Value::Int(7)])).unwrap(),
        "SHOUT"
    );

    // A named formatter is bypassed in compatibility mode as well, because the
    // parser does not even look for a name there.
    assert_eq!(
        shouty
            .format("{0:shout:}", &args([Value::from("x")]))
            .unwrap(),
        "x"
    );
}

#[test]
fn parser_settings_and_smart_settings_cannot_disagree() {
    // The parser settings win, and are mirrored into `SmartSettings`, so the
    // formatter never reads a stale copy of the two shared settings.
    let parser_settings = smartformat::parsing::ParserSettings {
        string_format_compatibility: true,
        error_action: ErrorAction::MaintainTokens,
        ..smartformat::parsing::ParserSettings::default()
    };

    let mut smart = SmartFormatter::with_parser_settings(
        SmartSettings {
            string_format_compatibility: false,
            parse_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        },
        parser_settings,
    );

    assert!(smart.settings().string_format_compatibility);
    assert_eq!(
        smart.settings().parse_error_action,
        ErrorAction::MaintainTokens
    );
    assert!(smart.parser().settings().string_format_compatibility);

    // …and compatibility mode is really in force: only the default formatter
    // runs, and `{{` escapes a brace.
    smart.formatters_mut().insert(0, Box::new(ShoutFormatter));
    assert_eq!(smart.format("{0}", &args([Value::Int(7)])).unwrap(), "7");
    assert_eq!(
        smart.format("{{0}}", &args([Value::Int(7)])).unwrap(),
        "{0}"
    );
    // The parse error action came from the parser settings too.
    assert_eq!(smart.format("{0", &args([Value::Int(7)])).unwrap(), "{0");
}

#[test]
fn an_empty_argument_list_is_its_own_scope() {
    // .NET `ExecuteFormattingAction`: `args.Count > 0 ? args[0] : args`, so
    // with no arguments the current value is the (empty) argument list itself,
    // which .NET renders as "System.Object[]" and we refuse to render at all.
    let smart = SmartFormatter::default();

    assert_eq!(
        smart.format("literal only", &args([])).unwrap(),
        "literal only"
    );

    let error = smart.format("{}", &args([])).unwrap_err();
    match error {
        Error::Format { message, .. } => assert!(message.contains("list"), "{message}"),
        other => panic!("expected a formatting error, got {other}"),
    }

    // A named or positional selector has nothing to resolve against either.
    assert!(smart.format("{0}", &args([])).is_err());
    assert!(smart.format("{Name}", &args([])).is_err());

    // A single null argument is a null scope, not an empty one.
    assert_eq!(smart.format("{}", &args([Value::Null])).unwrap(), "");
}

#[test]
fn alignment_pads_with_the_configured_fill_character() {
    let settings = SmartSettings {
        alignment_fill_character: '.',
        ..SmartSettings::default()
    };
    let smart = SmartFormatter::new(settings);
    let values = args([Value::from("ab")]);

    assert_eq!(smart.format("[{0,6}]", &values).unwrap(), "[....ab]");
    assert_eq!(smart.format("[{0,-6}]", &values).unwrap(), "[ab....]");
}

#[test]
fn case_insensitive_matching_folds_non_ascii() {
    let settings = SmartSettings {
        case_sensitive: CaseSensitivity::CaseInsensitive,
        ..SmartSettings::default()
    };
    let mut parser_settings = smartformat::parsing::ParserSettings::default();
    parser_settings
        .add_custom_selector_chars(['Ä', 'ä'])
        .unwrap();
    let smart = SmartFormatter::with_parser_settings(settings, parser_settings);

    let data = map([("ä", Value::from("v"))]);
    assert_eq!(smart.format("{Ä}", &data).unwrap(), "v");
}

#[test]
fn unsigned_values_above_i64_max_format_exactly() {
    let values = args([Value::from(u64::MAX)]);
    assert_eq!(format("{0}", &values), "18446744073709551615");
    assert_eq!(format("{0:N0}", &values), "18,446,744,073,709,551,615");
    assert_eq!(format("{0:X}", &values), "FFFFFFFFFFFFFFFF");
}

#[test]
fn output_error_in_result_writes_the_dotnet_exception_message() {
    let smart = with_format_error_action(ErrorAction::OutputErrorInResult);
    let data = map([("Other", Value::Int(1))]);

    assert_eq!(
        smart.format("[{Missing}]", &data).unwrap(),
        "[Error parsing format string: No source extension could handle the selector named \
         \"Missing\" at 2\n[{Missing}]\n--^]"
    );
    assert_eq!(
        smart
            .format("[{0:nosuchformatter:x}]", &args([Value::Int(42)]))
            .unwrap(),
        "[Error parsing format string: No suitable Formatter could be found at 0\n\
         [{0:nosuchformatter:x}]\n^]"
    );
}

#[test]
fn maintain_tokens_writes_the_reconstructed_placeholder() {
    let smart = with_format_error_action(ErrorAction::MaintainTokens);
    let data = map([("Other", Value::Int(1))]);

    assert_eq!(smart.format("[{Missing}]", &data).unwrap(), "[{Missing}]");
    assert_eq!(
        smart.format("[{Missing,05}]", &data).unwrap(),
        "[{Missing,5}]"
    );
    assert_eq!(
        smart.format(r"[{Missing:d(a\:b)}]", &data).unwrap(),
        "[{Missing:d(a:b):}]"
    );
}
