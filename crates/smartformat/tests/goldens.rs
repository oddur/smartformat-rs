//! Runs the checked-in golden file produced by the real SmartFormat.NET
//! library (`tools/goldens`, see its README for the JSON shape and the
//! argument mapping this file mirrors).
//!
//! Every case is rendered with default settings and the invariant culture, and
//! must match .NET byte for byte — or, for cases where .NET throws, must fail
//! with the corresponding error kind.

use std::collections::BTreeMap;

use smartformat::{Error, SmartFormatter, Value};

const GOLDENS: &str = include_str!("../../../goldens/m1.json");

#[test]
fn goldens_match_smartformat_net() {
    let document = json::parse(GOLDENS);
    let cases = document.get("cases").expect("cases").array();

    let smart = SmartFormatter::default();
    let mut failures = Vec::new();
    let mut passed = 0;
    let mut skipped = 0;

    for case in cases {
        let id = case.get("id").expect("id").string();
        let template = case.get("template").expect("template").string();
        let raw_args = case.get("args").expect("args");
        if let Some(reason) = skip_reason(raw_args) {
            eprintln!("skipping {id}: {reason}");
            skipped += 1;
            continue;
        }
        let args = to_value(raw_args);
        let expected = case.get("expected").expect("expected");

        let actual = smart.format(template, &args);
        let outcome = match (expected.get("result"), expected.get("error")) {
            (Some(result), _) => match &actual {
                Ok(text) if text == result.string() => Ok(()),
                Ok(text) => Err(format!("expected {:?}, got {:?}", result.string(), text)),
                Err(error) => Err(format!(
                    "expected {:?}, got error: {error}",
                    result.string()
                )),
            },
            (None, Some(kind)) => match (&actual, kind.string()) {
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
            },
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
    assert!(
        failures.is_empty(),
        "{} golden cases do not match SmartFormat.NET:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The JSON-to-`Value` mapping documented in `tools/goldens/README.md`.
fn to_value(node: &json::Json) -> Value {
    match node {
        json::Json::Null => Value::Null,
        json::Json::Bool(v) => Value::Bool(*v),
        json::Json::Int(v) => Value::Int(*v),
        json::Json::Float(v) => Value::Float(*v),
        json::Json::Str(v) => Value::String(v.clone()),
        json::Json::Array(items) => Value::List(items.iter().map(to_value).collect()),
        json::Json::Object(entries) => {
            if let [(key, value)] = entries.as_slice() {
                if key == "$f" {
                    return Value::Float(match value.string() {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        other => other.parse().expect("float literal"),
                    });
                }
                if key == "$dt" {
                    return datetime(value.string());
                }
            }

            let mut map = BTreeMap::new();
            for (key, value) in entries {
                map.insert(key.clone(), to_value(value));
            }
            Value::Map(map)
        }
    }
}

#[cfg(feature = "time")]
fn datetime(text: &str) -> Value {
    Value::DateTime(text.parse().expect("round-trip date/time"))
}

#[cfg(not(feature = "time"))]
fn datetime(_text: &str) -> Value {
    unreachable!("date/time cases are skipped without the \"time\" feature")
}

/// Why a case is not run, if it is not.
#[cfg(feature = "time")]
fn skip_reason(_args: &json::Json) -> Option<&'static str> {
    None
}

#[cfg(not(feature = "time"))]
fn skip_reason(args: &json::Json) -> Option<&'static str> {
    fn has_datetime(node: &json::Json) -> bool {
        match node {
            json::Json::Array(items) => items.iter().any(has_datetime),
            json::Json::Object(entries) => entries
                .iter()
                .any(|(key, value)| key == "$dt" || has_datetime(value)),
            _ => false,
        }
    }

    has_datetime(args).then_some("date/time values need the \"time\" feature")
}

/// A JSON reader just large enough for the golden file, so the test needs no
/// dependency.
mod json {
    #[derive(Debug)]
    pub enum Json {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Str(String),
        Array(Vec<Json>),
        Object(Vec<(String, Json)>),
    }

    impl Json {
        pub fn get(&self, key: &str) -> Option<&Json> {
            match self {
                Json::Object(entries) => entries
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value),
                _ => None,
            }
        }

        pub fn array(&self) -> &[Json] {
            match self {
                Json::Array(items) => items,
                other => panic!("expected an array, got {other:?}"),
            }
        }

        pub fn string(&self) -> &str {
            match self {
                Json::Str(text) => text,
                other => panic!("expected a string, got {other:?}"),
            }
        }
    }

    pub fn parse(input: &str) -> Json {
        let mut chars: Vec<char> = input.chars().collect();
        chars.push('\0');
        let mut reader = Reader { chars, at: 0 };
        let value = reader.value();
        reader.skip_whitespace();
        value
    }

    struct Reader {
        chars: Vec<char>,
        at: usize,
    }

    impl Reader {
        fn peek(&self) -> char {
            self.chars[self.at]
        }

        fn next(&mut self) -> char {
            let c = self.chars[self.at];
            self.at += 1;
            c
        }

        fn expect(&mut self, expected: char) {
            let c = self.next();
            assert_eq!(c, expected, "unexpected character at {}", self.at);
        }

        fn skip_whitespace(&mut self) {
            while matches!(self.peek(), ' ' | '\t' | '\n' | '\r') {
                self.at += 1;
            }
        }

        fn value(&mut self) -> Json {
            self.skip_whitespace();
            match self.peek() {
                '{' => self.object(),
                '[' => self.array(),
                '"' => Json::Str(self.string()),
                't' => {
                    self.at += 4;
                    Json::Bool(true)
                }
                'f' => {
                    self.at += 5;
                    Json::Bool(false)
                }
                'n' => {
                    self.at += 4;
                    Json::Null
                }
                _ => self.number(),
            }
        }

        fn object(&mut self) -> Json {
            self.expect('{');
            let mut entries = Vec::new();
            loop {
                self.skip_whitespace();
                if self.peek() == '}' {
                    self.at += 1;
                    return Json::Object(entries);
                }
                let key = self.string();
                self.skip_whitespace();
                self.expect(':');
                entries.push((key, self.value()));
                self.skip_whitespace();
                if self.peek() == ',' {
                    self.at += 1;
                }
            }
        }

        fn array(&mut self) -> Json {
            self.expect('[');
            let mut items = Vec::new();
            loop {
                self.skip_whitespace();
                if self.peek() == ']' {
                    self.at += 1;
                    return Json::Array(items);
                }
                items.push(self.value());
                self.skip_whitespace();
                if self.peek() == ',' {
                    self.at += 1;
                }
            }
        }

        fn string(&mut self) -> String {
            self.skip_whitespace();
            self.expect('"');
            let mut text = String::new();
            loop {
                match self.next() {
                    '"' => return text,
                    '\\' => match self.next() {
                        '"' => text.push('"'),
                        '\\' => text.push('\\'),
                        '/' => text.push('/'),
                        'b' => text.push('\u{8}'),
                        'f' => text.push('\u{c}'),
                        'n' => text.push('\n'),
                        'r' => text.push('\r'),
                        't' => text.push('\t'),
                        'u' => {
                            let unit = self.code_unit();
                            // Surrogate pairs are written as two escapes.
                            if (0xd800..0xdc00).contains(&unit) {
                                self.expect('\\');
                                self.expect('u');
                                let low = self.code_unit();
                                let combined = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                                text.push(char::from_u32(combined).expect("surrogate pair"));
                            } else {
                                text.push(char::from_u32(unit).expect("code point"));
                            }
                        }
                        other => panic!("unsupported escape \\{other}"),
                    },
                    c => text.push(c),
                }
            }
        }

        fn code_unit(&mut self) -> u32 {
            let digits: String = (0..4).map(|_| self.next()).collect();
            u32::from_str_radix(&digits, 16).expect("hex escape")
        }

        fn number(&mut self) -> Json {
            let start = self.at;
            while matches!(self.peek(), '0'..='9' | '-' | '+' | '.' | 'e' | 'E') {
                self.at += 1;
            }
            let text: String = self.chars[start..self.at].iter().collect();
            if text.contains(['.', 'e', 'E']) {
                Json::Float(text.parse().expect("float literal"))
            } else {
                Json::Int(text.parse().expect("integer literal"))
            }
        }
    }
}
