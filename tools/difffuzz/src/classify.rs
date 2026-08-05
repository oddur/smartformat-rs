//! Deciding whether the two engines agree, and — when they do not — whether
//! the difference is one the project already knows about.
//!
//! The classification is triage, not proof. `DESIGN.md` is the ledger and the
//! skip list in `crates/smartformat/tests/goldens.rs` is the pin; the rules
//! below only recognise the *shape* of an entry that is already in one of them,
//! so a campaign's output is a short list of things to look at rather than a
//! long list of things already decided. Every classified disagreement still
//! goes into the report with the rule that caught it, so a wrong rule is
//! visible rather than silent.

use serde_json::Value as Json;

use crate::case::{Case, ErrorKind, NetOutcome, RustOutcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Nothing in `DESIGN.md` or the skip list explains this. Worth a look.
    New,
    /// The shape of a divergence `DESIGN.md` records.
    Known,
    /// The harness process did not survive the case.
    HarnessDied,
    /// The two disagreed in a batch and agreed when the case was run alone, so
    /// what differs is .NET's state between cases, not the rendering.
    OrderDependent,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Class::New => "new",
            Class::Known => "known-divergence",
            Class::HarnessDied => "harness-died",
            Class::OrderDependent => "order-dependent",
        }
    }
}

/// Whether the two answers are the same answer.
///
/// For a rendered result that is a byte-for-byte comparison. For an error it
/// cannot be: .NET tells us only the exception type name, so the mapping is the
/// exact table `goldens.rs` applies, and nothing looser — a pairing outside it
/// is reported and left for a person to widen the table for.
pub fn agrees(case: &Case, net: &NetOutcome, rust: &RustOutcome) -> bool {
    match (net, rust) {
        (NetOutcome::Result(expected), RustOutcome::Result(actual)) => expected == actual,
        (NetOutcome::Error(exception), RustOutcome::Error { kind, .. }) => {
            error_kinds_agree(case, exception, *kind)
        }
        _ => false,
    }
}

fn error_kinds_agree(case: &Case, exception: &str, kind: ErrorKind) -> bool {
    let template = case.template();
    // An exception a formatter *extension* raises is caught and re-thrown as a
    // `FormattingException` wrapping it, so a parse or escape error from inside
    // a localized string or a registered template arrives under that name in
    // .NET while `Error` keeps its own kind.
    let wrapped = template.contains(":t:")
        || template.contains(":L:")
        || template.contains(":L(")
        || template.contains(":t(");
    match (kind, exception) {
        (ErrorKind::Parse, "ParsingErrors" | "ArgumentException") => true,
        (ErrorKind::Escape, "ArgumentException") => true,
        (ErrorKind::Parse | ErrorKind::Escape, "FormattingException") if wrapped => true,
        (
            ErrorKind::Format | ErrorKind::UnsupportedSpec,
            "FormattingException" | "LocalizationFormattingException",
        ) => true,
        _ => false,
    }
}

/// A recognised divergence: which rule caught it, and the wording from
/// `DESIGN.md` or the golden skip list that it stands for.
#[derive(Clone, Debug)]
pub struct Known {
    pub rule: &'static str,
    pub reason: &'static str,
}

/// Recognises the shape of a divergence the project has already decided about.
pub fn known_divergence(case: &Case, net: &NetOutcome, rust: &RustOutcome) -> Option<Known> {
    let template = case.template();
    let net_text = match net {
        NetOutcome::Result(text) => Some(text.as_str()),
        _ => None,
    };
    let rust_text = match rust {
        RustOutcome::Result(text) => Some(text.as_str()),
        _ => None,
    };
    let rust_message = match rust {
        RustOutcome::Error { message, .. } => Some(message.as_str()),
        _ => None,
    };
    // What the port said, whichever way it said it: with
    // `OutputErrorInResult` an error becomes part of the rendered text, so a
    // rule that reads only the error message would miss the same divergence
    // under a different setting.
    let rust_says = |needle: &str| {
        rust_message.is_some_and(|message| message.contains(needle))
            || rust_text.is_some_and(|text| text.contains(needle))
    };

    // .NET falls back to `ToString()` for a value no formatter claims, and a
    // collection's `ToString()` is its CLR type name.
    if net_text.is_some_and(|text| {
        text.contains("System.Object[]")
            || text.contains("System.Collections.Generic.Dictionary`2")
            || text.contains("System.Collections.Generic.List`1")
            || text.contains("SmartFormat.Extensions.PersistentVariables.VariablesGroup")
    }) {
        return Some(Known {
            rule: "collection-type-name",
            reason: "default formatting of a collection renders the CLR type name in .NET; we fail loudly instead (DESIGN.md, \"Default formatting of lists and maps\")",
        });
    }

    // `{0:d(` makes .NET index past the end of the format string.
    if matches!(net, NetOutcome::Error(kind) if kind == "IndexOutOfRangeException") {
        return Some(Known {
            rule: "unterminated-formatter-options",
            reason: "unterminated formatter options throw IndexOutOfRangeException in .NET; we report the ordinary parse error (DESIGN.md, \"Unterminated formatter options\")",
        });
    }

    // .NET's default calendar for ar-SA is UmAlQura and the port has no Hijri
    // calendar.
    if case.culture.starts_with("ar") && mentions_a_date(&case.args) {
        return Some(Known {
            rule: "ar-sa-calendar",
            reason: "ar-SA dates: .NET's default calendar for it is UmAlQura, and the port has no Hijri calendar",
        });
    }

    // fancy-regex is not System.Text.RegularExpressions, but only where the two
    // dialects differ: the element matched over, `\w` and the `\b` that moves
    // with it, case folding, character-class syntax, `$` before a final
    // newline, and the escapes each engine spells its own way. A plain ASCII
    // pattern over ASCII text that differs is a finding, not this.
    //
    // This is the broadest rule in the file — `$` alone covers most anchored
    // patterns — so a suppressed `ismatch` finding deserves more suspicion than
    // any other. Narrowing it means teaching the two dialects apart properly,
    // which is a bigger job than a triage rule.
    let regex_dialect = [
        "\\w", "\\W", "\\b", "\\B", "&&", "[[:", "[[\\:", "[a[", "$", "\\0", "\\1", "\\2", "\\3",
        "\\4", "\\5", "\\6", "\\7", "\\G", "\\Z", "(?<",
    ];
    if template.contains("ismatch")
        && (case.settings.contains_key("regexOptions")
            || !template.is_ascii()
            || has_non_ascii(&case.args)
            || rust_says("Invalid regular expression")
            || regex_dialect.iter().any(|shape| template.contains(shape)))
    {
        return Some(Known {
            rule: "regex-engine",
            reason: "regex engines: .NET matches over UTF-16 code units with its own `\\w` and case mapping, fancy-regex over Unicode scalars (DESIGN.md, \"`ismatch` runs on fancy-regex\")",
        });
    }

    // A lone surrogate half survives as a UTF-16 code unit in .NET and cannot
    // in a Rust `String`; two halves written next to each other re-form there.
    if template.to_ascii_lowercase().contains("\\ud") || contains_lone_surrogate_escape(&template) {
        return Some(Known {
            rule: "unpaired-surrogate",
            reason: "unpaired `\\uXXXX` surrogates: .NET keeps a UTF-16 code unit, a Rust String cannot (DESIGN.md, \"Unpaired `\\uXXXX` surrogates\")",
        });
    }

    // `X` and `B` on a negative value: `Value` collapses every signed integer
    // to i64, so the two's complement is 64 bits wide rather than the CLR
    // type's.
    if let (Some(expected), Some(actual)) = (net_text, rust_text) {
        if actual.len() > expected.len()
            && actual.ends_with(expected)
            && actual.chars().all(|c| c.is_ascii_hexdigit() || c == '1')
            && expected.chars().all(|c| c.is_ascii_hexdigit())
            && !expected.is_empty()
        {
            return Some(Known {
                rule: "integer-width",
                reason: "`Value` collapses every signed integer to i64, so `X` and `B` render a 64-bit two's complement (DESIGN.md, \"Integer width\")",
            });
        }
    }

    // Two halves of one astral character, each written on its own: .NET holds
    // each as a UTF-16 code unit and the pair re-forms, where a Rust `String`
    // replaced each half with U+FFFD already.
    if rust_text.is_some_and(|text| text.contains('\u{fffd}'))
        && net_text.is_some_and(|text| !text.contains('\u{fffd}'))
    {
        return Some(Known {
            rule: "surrogate-halves-rejoin",
            reason: "two halves of one surrogate pair written next to each other re-form in .NET's UTF-16 and cannot in a Rust String (DESIGN.md, \"Unpaired `\\uXXXX` surrogates\")",
        });
    }

    // A `Dictionary` is `IEnumerable` in .NET, so `list` iterates it as pairs;
    // a `Value::Map` is not a list here.
    if rust_says("requires an IEnumerable argument") && net_text.is_some() {
        return Some(Known {
            rule: "map-is-not-list-formattable",
            reason: "a .NET Dictionary is IEnumerable and `list` iterates its pairs; a map is not list-formattable here (DESIGN.md, \"A map is not list-formattable\")",
        });
    }

    // A three-letter ISO 639-2 code in a formatter's options: .NET resolves it
    // to a culture, and the port validates the name without resolving it.
    if let Some(code) = three_letter_option(&template) {
        let _ = code;
        return Some(Known {
            rule: "iso-639-2-option",
            reason: "a culture name in formatter options is validated, not resolved, so a three-letter ISO 639-2 code that .NET resolves does not reach a culture here (DESIGN.md, \"A culture name in formatter options is validated, not resolved\")",
        });
    }

    // A custom date, number or TimeSpan pattern renders in .NET and is a
    // documented non-goal here.
    if rust_says("unsupported format spec") {
        let rule = if mentions_a_span(&case.args) {
            "timespan-custom-pattern"
        } else {
            "custom-pattern"
        };
        return Some(Known {
            rule,
            reason: "a custom date/number/TimeSpan pattern renders in .NET and is the documented custom-pattern non-goal here (DESIGN.md, \"Non-goals\")",
        });
    }

    // An error raised while a `Format` parsed somewhere else is rendered: .NET
    // quotes the failing item's own base string, we quote the string we were
    // called with.
    if (template.contains(":L") || template.contains(":t"))
        && case
            .settings
            .get("formatErrorAction")
            .and_then(Json::as_str)
            .is_some_and(|action| action == "OutputErrorInResult")
    {
        return Some(Known {
            rule: "foreign-format-error",
            reason: "an error inside a localized string or a registered template quotes the string being rendered here and the translation in .NET",
        });
    }

    // `PluralRule` takes an f64 where .NET's takes a `decimal`, so a value
    // outside a double's exact integers picks a different rule.
    if template.contains("plural")
        && (mentions_a_float(&case.args) || mentions_a_big_integer(&case.args))
    {
        return Some(Known {
            rule: "plural-on-f64",
            reason: "pluralization runs on f64 here and on decimal in .NET (DESIGN.md, \"Pluralization runs on f64\")",
        });
    }

    let case_insensitive = case
        .settings
        .get("caseSensitivity")
        .and_then(Json::as_str)
        .is_some_and(|value| value == "CaseInsensitive");

    // `ToUpper`/`ToLower` and case-insensitive selector matching use Unicode
    // full case mapping here and .NET's per-culture simple mapping there.
    if (template.contains("ToUpper") || template.contains("ToLower") || case_insensitive)
        && has_non_ascii(&case.args)
    {
        return Some(Known {
            rule: "unicode-case-mapping",
            reason: "case mapping: full Unicode mapping here, .NET's per-culture simple mapping there (DESIGN.md, \"Unicode case mapping\")",
        });
    }

    // .NET's `DictionarySource` walks the entries and takes the first match,
    // where a `BTreeMap` lookup has an order of its own.
    if case_insensitive && has_keys_differing_only_by_case(&case.args) {
        return Some(Known {
            rule: "case-insensitive-map-order",
            reason: "which of two keys differing only in case wins is .NET's enumeration order and ours is the map's (DESIGN.md, \"Case-insensitive map lookup order\")",
        });
    }

    // A registered group arriving as the current value of a *child format*:
    // no selector of that placeholder rooted it, so a source ranked between
    // the two can claim a name the group also holds.
    if case.settings.contains_key("variables") && group_as_a_child_format(&template) {
        return Some(Known {
            rule: "variables-group-child-format",
            reason: "a group that arrives as the current value of a child format is read by the sources that read maps, so a name ranked in between wins where .NET's group would (DESIGN.md, \"A `VariablesGroup` argument does not shadow a registered group\")",
        });
    }

    // The persistent-variables source is reached case-insensitively here and
    // matched exactly there.
    if case_insensitive && case.settings.contains_key("variables") {
        return Some(Known {
            rule: "case-insensitive-variable-name",
            reason: "a case-insensitive setting reaches variable names here and not in .NET (DESIGN.md, \"A case-insensitive setting reaches variable names\")",
        });
    }

    // `choose` compares its options ordinally here; .NET compares them with the
    // culture in force, which folds characters an ordinal comparison keeps.
    if template.contains(":choose(") && (!template.is_ascii() || has_non_ascii(&case.args)) {
        return Some(Known {
            rule: "choose-ordinal-comparison",
            reason: "`choose` compares its options ordinally here and culture-sensitively in .NET (DESIGN.md, \"`choose` compares its options ordinally\")",
        });
    }

    // A localization key built by rendering nested placeholders: .NET builds it
    // with a null provider, so the ambient culture renders the key.
    if template.contains(":L") && localization_key_has_a_placeholder(&template) {
        return Some(Known {
            rule: "localization-key-culture",
            reason: "a localization key built by rendering nested placeholders uses the ambient culture in .NET and the culture of the call here (DESIGN.md)",
        });
    }

    // `TimeFormatter` and `ChooseFormatter` read the *thread* culture in .NET
    // for parts of what they write, which the harness pins to the invariant one.
    if !case.culture.is_empty() && (template.contains(":time") || template.contains(":choose")) {
        return Some(Known {
            rule: "thread-culture-render",
            reason: "`TimeFormatter` writes a unit's number, and `choose` stringifies its value, with the thread culture in .NET and the culture of the call here (DESIGN.md)",
        });
    }

    None
}

/// A three-letter alphabetic culture name in the options of a formatter that
/// takes one — `plural(eng)`, `time(deu)`, `L(fra)`. Anything longer or
/// shorter is a two-letter code, a full culture name, or not a culture at all.
fn three_letter_option(template: &str) -> Option<&str> {
    for opener in [":plural(", ":time(", ":L("] {
        let mut rest = template;
        while let Some(start) = rest.find(opener) {
            let after = &rest[start + opener.len()..];
            if let Some(end) = after.find(')') {
                let option = &after[..end];
                if option.len() == 3 && option.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Some(option);
                }
            }
            rest = &rest[start + opener.len()..];
        }
    }
    None
}

/// A group of the variables fixture rendered with a child format — `{global:…}`
/// — rather than selected into, which is the shape of the shadowing residue.
fn group_as_a_child_format(template: &str) -> bool {
    ["{global:", "{v:"]
        .iter()
        .any(|shape| template.contains(shape))
}

/// Whether the format of an `{:L…:…}` placeholder holds a placeholder of its
/// own, which is what makes the key a rendered string rather than raw text.
fn localization_key_has_a_placeholder(template: &str) -> bool {
    let mut rest = template;
    while let Some(start) = rest.find(":L") {
        let after = &rest[start + 2..];
        // Step over an options group and the colon that follows the name.
        let after = match after.split_once(':') {
            Some((_, format)) => format,
            None => return false,
        };
        if after.starts_with('{') {
            return true;
        }
        rest = after;
    }
    false
}

/// A signed integer past a double's exactly representable range, where a
/// `PluralRule` on f64 and one on `decimal` can pick different words.
fn mentions_a_big_integer(args: &Json) -> bool {
    let mut found = false;
    walk(args, &mut |node| {
        if let Json::Number(number) = node {
            if let Some(value) = number.as_i64() {
                if value.unsigned_abs() > (1u64 << 53) {
                    found = true;
                }
            }
        }
    });
    found
}

/// Two keys of one map that differ only in case: which of them a
/// case-insensitive lookup finds is .NET's enumeration order, not ours.
fn has_keys_differing_only_by_case(args: &Json) -> bool {
    let mut found = false;
    walk(args, &mut |node| {
        if let Json::Object(entries) = node {
            let mut folded: Vec<String> = entries.keys().map(|key| key.to_lowercase()).collect();
            folded.sort();
            let before = folded.len();
            folded.dedup();
            if folded.len() < before {
                found = true;
            }
        }
    });
    found
}

fn contains_lone_surrogate_escape(template: &str) -> bool {
    let bytes: Vec<char> = template.chars().collect();
    for index in 0..bytes.len() {
        if bytes[index] != '\\' || bytes.get(index + 1) != Some(&'u') || index + 5 >= bytes.len() {
            continue;
        }
        let digits: String = bytes[index + 2..index + 6].iter().collect();
        if let Ok(value) = u32::from_str_radix(&digits, 16) {
            if (0xd800..=0xdfff).contains(&value) {
                return true;
            }
        }
    }
    false
}

fn walk(node: &Json, seen: &mut impl FnMut(&Json)) {
    seen(node);
    match node {
        Json::Array(items) => items.iter().for_each(|item| walk(item, seen)),
        Json::Object(entries) => entries.values().for_each(|value| walk(value, seen)),
        _ => {}
    }
}

fn has_marker(args: &Json, marker: &str) -> bool {
    let mut found = false;
    walk(args, &mut |node| {
        if let Json::Object(entries) = node {
            if entries.len() == 1 && entries.contains_key(marker) {
                found = true;
            }
        }
    });
    found
}

fn mentions_a_date(args: &Json) -> bool {
    has_marker(args, "$dt")
}

fn mentions_a_span(args: &Json) -> bool {
    has_marker(args, "$ts")
}

fn mentions_a_float(args: &Json) -> bool {
    let mut found = false;
    walk(args, &mut |node| match node {
        Json::Number(number) if number.as_i64().is_none() => found = true,
        Json::Object(entries) if entries.len() == 1 && entries.contains_key("$f") => found = true,
        _ => {}
    });
    found
}

fn has_non_ascii(args: &Json) -> bool {
    let mut found = false;
    walk(args, &mut |node| {
        if let Json::String(text) = node {
            if !text.is_ascii() {
                found = true;
            }
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::{FormatSpec, Node, Placeholder, Template};
    use serde_json::{json, Map};

    fn case_with(template: &str, args: Json) -> Case {
        Case {
            id: "t".into(),
            tree: Template {
                nodes: vec![Node::Literal(template.into())],
            },
            args,
            culture: String::new(),
            settings: Map::new(),
        }
    }

    #[test]
    fn identical_results_agree() {
        let case = case_with("x", json!([]));
        assert!(agrees(
            &case,
            &NetOutcome::Result("a".into()),
            &RustOutcome::Result("a".into())
        ));
        assert!(!agrees(
            &case,
            &NetOutcome::Result("a".into()),
            &RustOutcome::Result("b".into())
        ));
    }

    #[test]
    fn the_error_table_matches_the_golden_runners() {
        let case = case_with("x", json!([]));
        let parse = RustOutcome::Error {
            kind: ErrorKind::Parse,
            message: String::new(),
        };
        assert!(agrees(
            &case,
            &NetOutcome::Error("ParsingErrors".into()),
            &parse
        ));
        assert!(agrees(
            &case,
            &NetOutcome::Error("ArgumentException".into()),
            &parse
        ));
        // Not wrapped by an extension, so this pairing is a finding.
        assert!(!agrees(
            &case,
            &NetOutcome::Error("FormattingException".into()),
            &parse
        ));
    }

    #[test]
    fn a_parse_error_inside_a_template_is_wrapped() {
        let case = Case {
            id: "t".into(),
            tree: Template {
                nodes: vec![Node::Placeholder(Box::new(Placeholder {
                    selector: String::new(),
                    alignment: None,
                    format: Some(FormatSpec {
                        name: "t".into(),
                        options: None,
                        parts: vec![vec![Node::Literal("bad".into())]],
                    }),
                }))],
            },
            args: json!([]),
            culture: String::new(),
            settings: Map::new(),
        };
        assert_eq!(case.template(), "{:t:bad}");
        assert!(agrees(
            &case,
            &NetOutcome::Error("FormattingException".into()),
            &RustOutcome::Error {
                kind: ErrorKind::Parse,
                message: String::new(),
            }
        ));
    }

    #[test]
    fn a_clr_type_name_in_the_dotnet_output_is_a_known_divergence() {
        let case = case_with("{0}", json!([[]]));
        let known = known_divergence(
            &case,
            &NetOutcome::Result("System.Object[]".into()),
            &RustOutcome::Error {
                kind: ErrorKind::Format,
                message: "no formatter".into(),
            },
        )
        .expect("recognised");
        assert_eq!(known.rule, "collection-type-name");
    }

    #[test]
    fn a_lone_surrogate_escape_is_recognised() {
        assert!(contains_lone_surrogate_escape(r"a\ud83db"));
        assert!(!contains_lone_surrogate_escape(r"aAb"));
    }

    #[test]
    fn an_ordinary_difference_is_new() {
        let case = case_with("{0:N2}", json!([1]));
        assert!(known_divergence(
            &case,
            &NetOutcome::Result("1.00".into()),
            &RustOutcome::Result("1,00".into())
        )
        .is_none());
    }
}
