//! Ported from SmartFormat.NET `src/SmartFormat/Extensions/StringSource.cs`.

use std::borrow::Cow;

use super::{SelectorInfo, Source};
use crate::value::Value;

/// Resolves the string "methods" SmartFormat exposes as selectors, as in
/// `{Name.Length}` or `{Name.ToUpper}`.
///
/// `ToCharArray`, `ToBase64` and `FromBase64` are not ported: the first has no
/// [`Value`] representation, and the base64 pair needs an encoder we do not
/// depend on yet.
///
/// Case conversion follows the Unicode full case mappings, which differ from
/// .NET's one-to-one invariant mapping for a few characters (`ß` uppercases to
/// `SS` here, and stays `ß` in .NET).
#[derive(Debug, Default, Clone, Copy)]
pub struct StringSource;

/// The selector names, in .NET's registration order.
const METHODS: [&str; 10] = [
    "Length",
    "ToUpper",
    "ToUpperInvariant",
    "ToLower",
    "ToLowerInvariant",
    "Trim",
    "TrimStart",
    "TrimEnd",
    "Capitalize",
    "CapitalizeWords",
];

impl Source for StringSource {
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        if let Some(null) = info.nullable_result() {
            return Some(null);
        }

        let Value::String(current) = info.current else {
            return None;
        };

        let method = METHODS.iter().find(|name| info.selector_is(name))?;
        let result = match *method {
            // .NET string.Length counts UTF-16 code units.
            "Length" => Value::Int(current.encode_utf16().count() as i64),
            "ToUpper" | "ToUpperInvariant" => Value::String(current.to_uppercase()),
            "ToLower" | "ToLowerInvariant" => Value::String(current.to_lowercase()),
            "Trim" => Value::String(current.trim().to_owned()),
            "TrimStart" => Value::String(current.trim_start().to_owned()),
            "TrimEnd" => Value::String(current.trim_end().to_owned()),
            "Capitalize" => Value::String(capitalize(current)),
            "CapitalizeWords" => Value::String(capitalize_words(current)),
            _ => return None,
        };
        Some(Cow::Owned(result))
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if !first.is_uppercase() => first.to_uppercase().chain(chars).collect(),
        _ => text.to_owned(),
    }
}

fn capitalize_words(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut previous_space = true;
    for c in text.chars() {
        if c.is_whitespace() {
            previous_space = true;
            result.push(c);
        } else if previous_space && c.is_alphabetic() {
            previous_space = false;
            result.extend(c.to_uppercase());
        } else {
            previous_space = false;
            result.push(c);
        }
    }
    result
}
