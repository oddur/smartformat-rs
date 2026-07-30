//! Runs the checked-in golden file produced by the real SmartFormat.NET
//! library (`tools/goldens`, see its README for the JSON shape and the
//! argument mapping this file mirrors).
//!
//! Every case is rendered with default settings and the invariant culture, and
//! must match .NET byte for byte — or, for cases where .NET throws, must fail
//! with the corresponding error kind.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};
use smartformat::fmt::culture;
use smartformat::{Error, SmartFormatter, Value};

const GOLDENS: &str = include_str!("../../../goldens/m1.json");

/// Cases we knowingly do not run, each with the reason. Behavior outside the
/// port's scope belongs here rather than in a silently passing branch; see the
/// non-goals in `DESIGN.md`.
const SKIPPED: &[(&str, &str)] = &[];

#[test]
fn goldens_match_smartformat_net() {
    let document: Json = serde_json::from_str(GOLDENS).expect("golden file is valid JSON");
    let cases = document["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "the golden file has no cases");

    let smart = SmartFormatter::default();
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

        // The per-case culture is `""` (invariant) for every M1 case; anything
        // else is skipped above, so this cannot silently format with the wrong
        // culture.
        let culture = culture::invariant();
        let args = to_value(&case["args"]);
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
                    (Err(Error::Parse { .. }), "ParsingErrors" | "ArgumentException") => Ok(()),
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
    if let Some((_, reason)) = SKIPPED.iter().find(|(name, _)| *name == id) {
        return Some(reason);
    }
    if case["culture"] != "" {
        return Some("only the invariant culture is in M1 scope");
    }
    #[cfg(not(feature = "time"))]
    if has_datetime(&case["args"]) {
        return Some("date/time values need the \"time\" feature");
    }
    None
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
