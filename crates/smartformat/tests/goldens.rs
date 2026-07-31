//! Runs the checked-in golden file produced by the real SmartFormat.NET
//! library (`tools/goldens`, see its README for the JSON shape and the
//! argument mapping this file mirrors).
//!
//! Every case is rendered with the invariant culture and with the settings its
//! `settings` object asks for (the .NET defaults when it has none), and must
//! match .NET byte for byte — or, for cases where .NET throws, must fail with
//! the corresponding error kind.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};
use smartformat::fmt::culture;
use smartformat::parsing::ParserSettings;
use smartformat::{CaseSensitivity, Error, ErrorAction, SmartFormatter, SmartSettings, Value};

const GOLDENS: &str = include_str!("../../../goldens/m1.json");

/// Cases we knowingly do not run, each with the reason. Behavior outside the
/// port's scope belongs here rather than in a silently passing branch; see the
/// non-goals in `DESIGN.md`.
const SKIPPED: &[(&str, &str)] = &[
    (
        "num-double-precise-pow2-neg25-none",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "num-double-precise-pow2-neg25-G",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "num-double-precise-neg-pow2-neg25-none",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "num-double-precise-pow2-neg958-none",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "num-double-precise-pow2-neg958-G",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "num-double-precise-neg-pow2-neg958-none",
        "shortest round-trip digits for an exact power of two: .NET's Grisu3 drops a digit that does not parse back",
    ),
    (
        "set-compat-formatter-name",
        "in compatibility mode the whole format reaches the value as a custom numeric pattern, which is a documented non-goal",
    ),
    (
        "num-int32-X-neg",
        "integer width: .NET formats the boxed int's own 32 bits, where every signed integer is an i64 here",
    ),
    (
        "num-int32-B-neg",
        "integer width: .NET formats the boxed int's own 32 bits, where every signed integer is an i64 here",
    ),
    (
        "sel-default-format-empty-args",
        "default formatting of a collection renders the CLR type name in .NET; we fail loudly instead",
    ),
    (
        "sel-default-format-list",
        "default formatting of a collection renders the CLR type name in .NET; we fail loudly instead",
    ),
    (
        "sel-default-format-map",
        "default formatting of a map renders the CLR type name in .NET; we fail loudly instead",
    ),
    (
        "set-case-insensitive-later-variant",
        "an ignore-case lookup with several case variants of one key follows insertion order in .NET, which a BTreeMap does not have",
    ),
    (
        "err-unterminated-formatter-options",
        "unterminated formatter options make .NET index past the end of the format string; we report a parse error",
    ),
    (
        "err-unterminated-formatter-options-escape",
        "unterminated formatter options make .NET index past the end of the format string; we report a parse error",
    ),
    (
        "str-to-upper-eszett",
        "case mapping: .NET maps one char to one char, so 'ß' stays 'ß'; Rust's full mapping gives \"SS\"",
    ),
    (
        "str-to-upper-invariant-eszett",
        "case mapping: .NET maps one char to one char, so 'ß' stays 'ß'; Rust's full mapping gives \"SS\"",
    ),
    (
        "str-to-lower-final-sigma",
        "case mapping: .NET lower-cases every sigma to 'σ'; Rust's full mapping applies the final-sigma rule and gives 'ς'",
    ),
    (
        "set-fmterr-outputerrorinresult-custom-pattern",
        "a custom numeric pattern renders in .NET and is a documented non-goal here, so the error text written into the result differs",
    ),
    (
        "plural-bare-name",
        "a one-part format is not auto-detected, so .NET reads it as a custom numeric pattern of literals: the documented custom-pattern non-goal",
    ),
    (
        "autodetect-single-part",
        "a one-part format is not auto-detected, so .NET reads it as a custom numeric pattern of literals: the documented custom-pattern non-goal",
    ),
    (
        "plural-i64-beyond-double",
        "pluralization runs on f64 where .NET runs on decimal, so an integer above 2^53 loses the last digits the Russian rule looks at",
    ),
    (
        "plural-f64-beyond-double",
        "pluralization runs on f64 where .NET runs on decimal, so a double above 2^53 loses the last digits the Russian rule looks at",
    ),
    (
        "plural-option-iso-639-2",
        "a three-letter ISO 639-2 language code is mapped to its two-letter equivalent by ICU; we take the culture name as written",
    ),
    (
        "autodetect-list",
        "ListFormatter sorts ahead of the plural formatter and auto-detects too, and it lands in M3",
    ),
    // .NET's default calendar for ar-SA is UmAlQura; we render Gregorian
    // fields through ar-SA's Hijri month names. Every specifier that reads a
    // date field diverges; the time-only ones (`t`, `T`) do not and are
    // ordinary cases.
    ("culture-date-ar-sa-d-lc", AR_SA_CALENDAR),
    ("culture-date-ar-sa-D", AR_SA_CALENDAR),
    ("culture-date-ar-sa-f-lc", AR_SA_CALENDAR),
    ("culture-date-ar-sa-F", AR_SA_CALENDAR),
    ("culture-date-ar-sa-g-lc", AR_SA_CALENDAR),
    ("culture-date-ar-sa-G", AR_SA_CALENDAR),
    ("culture-date-ar-sa-M", AR_SA_CALENDAR),
    ("culture-date-ar-sa-none", AR_SA_CALENDAR),
    ("culture-date2-ar-sa-D", AR_SA_CALENDAR),
    ("culture-date2-ar-sa-y-lc", AR_SA_CALENDAR),
    (
        "culture-fmt-choose-soft-hyphen-value",
        "choose compares its options ordinally; .NET compares them with the culture's CompareInfo, which ignores a soft hyphen",
    ),
    (
        "culture-fmt-choose-soft-hyphen-option",
        "choose compares its options ordinally; .NET compares them with the culture's CompareInfo, which ignores a soft hyphen",
    ),
];

const AR_SA_CALENDAR: &str =
    "ar-SA dates: .NET's default calendar for it is UmAlQura, and the port has no Hijri calendar";

#[test]
fn goldens_match_smartformat_net() {
    let document: Json = serde_json::from_str(GOLDENS).expect("golden file is valid JSON");
    let cases = document["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "the golden file has no cases");

    let mut failures = Vec::new();
    let mut passed = 0;
    let mut skipped = 0;

    for case in cases {
        let id = case["id"].as_str().expect("id");
        let template = case["template"].as_str().expect("template");
        let expected = &case["expected"];

        if let Some(reason) = skip_reason(id, case) {
            eprintln!("skipping {id}: {reason}");
            skipped += 1;
            continue;
        }

        // A case names the culture .NET rendered it with; `""` is the
        // invariant one. A name the generated table does not carry is a
        // failure, never a skip — otherwise adding a culture to the harness
        // and forgetting to generate its data would look like a pass.
        let culture_name = case["culture"].as_str().expect("culture name");
        let Some(culture) = culture::get(culture_name) else {
            failures.push(format!(
                "{id}: culture {culture_name:?} is not in the generated table \
                 (regenerate crates/smartformat/src/fmt/culture/generated.rs)"
            ));
            continue;
        };
        let args = to_value(&case["args"]);
        let smart = formatter_for(&case["settings"]);
        let actual = smart.format_with_culture(template, &args, culture);

        let outcome = match (expected.get("result"), expected.get("error")) {
            (Some(result), _) => {
                let result = result.as_str().expect("result string");
                match &actual {
                    Ok(text) if text == result => Ok(()),
                    Ok(text) => Err(format!("expected {result:?}, got {text:?}")),
                    Err(error) => Err(format!("expected {result:?}, got error: {error}")),
                }
            }
            (None, Some(kind)) => {
                let kind = kind.as_str().expect("error string");
                match (&actual, kind) {
                    // .NET throws ParsingErrors / ArgumentException from the
                    // parser, and FormattingException from the formatter.
                    // `Error::Escape` is the ArgumentException that escape
                    // resolution throws: from the parser for a trailing escape
                    // character, and from `LiteralText.AsSpan()` when a literal
                    // that cannot be resolved is written.
                    (Err(Error::Parse { .. }), "ParsingErrors" | "ArgumentException") => Ok(()),
                    (Err(Error::Escape { .. }), "ArgumentException") => Ok(()),
                    (
                        Err(Error::Format { .. } | Error::UnsupportedSpec { .. }),
                        "FormattingException",
                    ) => Ok(()),
                    (Err(error), expected_kind) => Err(format!(
                        "expected a {expected_kind}, got a different error: {error}"
                    )),
                    (Ok(text), expected_kind) => {
                        Err(format!("expected a {expected_kind}, got {text:?}"))
                    }
                }
            }
            (None, None) => panic!("case {id} has neither result nor error"),
        };

        match outcome {
            Ok(()) => passed += 1,
            Err(message) => failures.push(format!("{id}: template {template:?}: {message}")),
        }
    }

    eprintln!(
        "{passed} goldens passed, {skipped} skipped, {} failed",
        failures.len()
    );
    assert_eq!(
        passed + skipped + failures.len(),
        cases.len(),
        "every case must be accounted for"
    );
    assert!(
        failures.is_empty(),
        "{} golden cases do not match SmartFormat.NET:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A skip list entry that no longer matches a case would hide a case silently,
/// so it fails the suite instead.
#[test]
fn skip_list_has_no_stale_entries() {
    let document: Json = serde_json::from_str(GOLDENS).expect("golden file is valid JSON");
    let cases = document["cases"].as_array().expect("cases array");

    for (id, _) in SKIPPED {
        assert!(
            cases.iter().any(|case| case["id"] == **id),
            "skip list names {id}, which is not in the golden file"
        );
    }
}

/// Why a case is not run, if it is not.
fn skip_reason(id: &str, case: &Json) -> Option<&'static str> {
    let _ = case;
    if let Some((_, reason)) = SKIPPED.iter().find(|(name, _)| *name == id) {
        return Some(reason);
    }
    // Without the plural formatter registered, its cases report "No suitable
    // Formatter could be found", and the auto-detection cases would be decided
    // by the conditional formatter alone.
    #[cfg(not(feature = "plural"))]
    if id.contains("plural") || id.starts_with("autodetect-") {
        return Some("pluralization needs the \"plural\" feature");
    }
    #[cfg(not(feature = "time"))]
    if has_datetime(&case["args"]) {
        return Some("date/time values need the \"time\" feature");
    }
    None
}

/// Builds the formatter a case runs with. A case without a `settings` object
/// runs with the .NET defaults; the keys mirror `CaseSettings` in the harness.
fn formatter_for(node: &Json) -> SmartFormatter {
    let mut settings = SmartSettings::default();
    let mut parser_settings = ParserSettings::default();

    if let Some(entries) = node.as_object() {
        for (key, value) in entries {
            let text = || value.as_str().expect("settings values are strings");
            match key.as_str() {
                "formatErrorAction" => settings.format_error_action = error_action(text()),
                "parseErrorAction" => settings.parse_error_action = error_action(text()),
                "caseSensitivity" => {
                    settings.case_sensitive = match text() {
                        "CaseSensitive" => CaseSensitivity::CaseSensitive,
                        "CaseInsensitive" => CaseSensitivity::CaseInsensitive,
                        other => panic!("unknown case sensitivity {other}"),
                    }
                }
                "stringFormatCompatibility" => {
                    settings.string_format_compatibility =
                        value.as_bool().expect("a boolean setting");
                }
                "alignmentFillCharacter" => {
                    settings.alignment_fill_character =
                        text().chars().next().expect("a fill character");
                }
                "customSelectorChars" => parser_settings
                    .add_custom_selector_chars(text().chars())
                    .expect("custom selector characters"),
                other => panic!("unknown setting {other}"),
            }
        }
    }

    parser_settings.error_action = settings.parse_error_action;
    parser_settings.string_format_compatibility = settings.string_format_compatibility;
    SmartFormatter::with_parser_settings(settings, parser_settings)
}

fn error_action(name: &str) -> ErrorAction {
    match name {
        "ThrowError" => ErrorAction::Error,
        "Ignore" => ErrorAction::Ignore,
        "MaintainTokens" => ErrorAction::MaintainTokens,
        "OutputErrorInResult" => ErrorAction::OutputErrorInResult,
        other => panic!("unknown error action {other}"),
    }
}

/// The JSON-to-[`Value`] mapping documented in `tools/goldens/README.md`: a
/// top-level array is the positional argument set, a top-level object is a
/// single dictionary argument, and the `$dt` / `$f` markers carry the values
/// JSON cannot spell.
fn to_value(node: &Json) -> Value {
    match node {
        Json::Null => Value::Null,
        Json::Bool(v) => Value::Bool(*v),
        Json::Number(v) => match v.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(v.as_f64().expect("finite JSON number")),
        },
        Json::String(v) => Value::String(v.clone()),
        Json::Array(items) => Value::List(items.iter().map(to_value).collect()),
        Json::Object(entries) => match marker(entries) {
            Some(("$dt", text)) => datetime(text),
            // A 32-bit .NET int, which `Value` widens to i64 — the marker
            // exists only for the cases pinning that difference.
            Some(("$i32", text)) => Value::Int(text.parse::<i32>().expect("int literal").into()),
            // A .NET ulong, which JSON cannot spell above 2^53.
            Some(("$u64", text)) => Value::UInt(text.parse().expect("unsigned literal")),
            Some(("$f", text)) => Value::Float(match text {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                other => other.parse().expect("float literal"),
            }),
            Some((key, _)) => panic!("unknown marker {key}"),
            None => Value::Map(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), to_value(value)))
                    .collect::<BTreeMap<_, _>>(),
            ),
        },
    }
}

/// A one-entry object whose key starts with `$` is a marker, not a map.
fn marker(entries: &Map<String, Json>) -> Option<(&str, &str)> {
    let (key, value) = entries.iter().next().filter(|_| entries.len() == 1)?;
    if !key.starts_with('$') {
        return None;
    }
    Some((key, value.as_str().expect("marker payload is a string")))
}

#[cfg(feature = "time")]
fn datetime(text: &str) -> Value {
    Value::DateTime(text.parse().expect("round-trip date/time"))
}

#[cfg(not(feature = "time"))]
fn datetime(_text: &str) -> Value {
    unreachable!("date/time cases are skipped without the \"time\" feature")
}

#[cfg(not(feature = "time"))]
fn has_datetime(node: &Json) -> bool {
    match node {
        Json::Array(items) => items.iter().any(has_datetime),
        Json::Object(entries) => entries
            .iter()
            .any(|(key, value)| key == "$dt" || has_datetime(value)),
        _ => false,
    }
}
