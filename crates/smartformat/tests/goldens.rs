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
use smartformat::formatter::{DefaultFormatter, FormatterRegistry};
use smartformat::parsing::ParserSettings;
#[cfg(feature = "plural")]
use smartformat::PluralLocalizationFormatter;
use smartformat::{
    CaseSensitivity, ChooseFormatter, ConditionalFormatter, Error, ErrorAction, ListFormatter,
    NullFormatter, SmartFormatter, SmartSettings, SubStringFormatter, SubStringOutOfRangeBehavior,
    Value,
};
#[cfg(feature = "regex-formatters")]
use smartformat::{IsMatchFormatter, RegexOptions};

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
        COLLECTION_TYPE_NAME,
    ),
    ("sel-default-format-list", COLLECTION_TYPE_NAME),
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
    (
        "list-map-is-enumerable",
        "a .NET dictionary is IEnumerable, so `list` renders it as its KeyValuePairs; a map is not a list here",
    ),
    ("list-no-format", COLLECTION_TYPE_NAME),
    (
        "list-item-custom-pattern",
        "a custom numeric pattern as the item format is the documented custom-pattern non-goal; `D2` is the standard-specifier equivalent",
    ),
    (
        "substr-astral-halves-rejoin",
        SURROGATE_HALVES_REJOIN,
    ),
    (
        "substr-astral-halves-rejoin-child-format",
        SURROGATE_HALVES_REJOIN,
    ),
    (
        "ismatch-dollar-before-final-newline",
        "regex engines: .NET's `$` also matches before a final newline, where fancy-regex's does not (`\\z` agrees in both)",
    ),
    ("ismatch-astral-dot", REGEX_STRING_ELEMENT),
    ("ismatch-astral-two-dots", REGEX_STRING_ELEMENT),
    ("ismatch-astral-negated-class", REGEX_STRING_ELEMENT),
    ("ismatch-astral-captured-group", REGEX_STRING_ELEMENT),
    ("ismatch-word-letter-number", REGEX_WORD_CHARACTER),
    ("ismatch-word-spacing-mark", REGEX_WORD_CHARACTER),
    ("ismatch-word-boundary-letter-number", REGEX_WORD_CHARACTER),
    ("ismatch-fold-long-s", REGEX_CASE_FOLDING),
    ("ismatch-fold-final-sigma", REGEX_CASE_FOLDING),
    ("ismatch-fold-deseret", REGEX_CASE_FOLDING),
    (
        "ismatch-nul-escape",
        "regex engines: `\\0` is NUL in .NET and a back reference to group 0 in fancy-regex, which the pinned version compiles into a pattern matching nothing; write `\\x00`",
    ),
    ("ismatch-class-intersection", REGEX_CHARACTER_CLASS),
    ("ismatch-class-posix-name", REGEX_CHARACTER_CLASS),
    ("ismatch-class-nested", REGEX_CHARACTER_CLASS),
    (
        "ismatch-octal-escape",
        "regex engines: .NET reads `\\101` as an octal escape and fancy-regex refuses to compile it, so the call fails loudly",
    ),
    (
        "template-error-inside",
        "an error inside a registered template quotes the template's own text in .NET; the engine here quotes the string being rendered",
    ),
];

/// `ListFormatter` does not change these: it declines a placeholder that
/// carries no format at all, so a bare `{0}` on a list still reaches
/// `DefaultFormatter` — in .NET too, where the value's `ToString()` is its CLR
/// type name.
const COLLECTION_TYPE_NAME: &str =
    "default formatting of a collection renders the CLR type name in .NET; we fail loudly instead";

const AR_SA_CALENDAR: &str =
    "ar-SA dates: .NET's default calendar for it is UmAlQura, and the port has no Hijri calendar";

const SURROGATE_HALVES_REJOIN: &str =
    "two halves of one surrogate pair, written next to each other: .NET holds each as a UTF-16 code unit and the pair re-forms, where a Rust String replaced each half already";

const REGEX_STRING_ELEMENT: &str =
    "regex engines: .NET matches over UTF-16 code units and fancy-regex over Unicode scalars, so an astral character is two elements there and one here";

const REGEX_WORD_CHARACTER: &str =
    "regex engines: .NET's `\\w` is [\\p{L}\\p{Mn}\\p{Nd}\\p{Pc}] and the regex crate's also covers Nl, Mc and Me, which moves `\\b` with it";

const REGEX_CASE_FOLDING: &str =
    "regex engines: IgnoreCase is Unicode simple case folding here and simple case mapping in .NET, which also never folds across a surrogate pair; CultureInvariant does not reconcile them";

const REGEX_CHARACTER_CLASS: &str =
    "regex engines: .NET has no class intersection, no POSIX class name and no nesting, so it reads `&&`, `[[:alpha:]]` and `[a[bc]]` as literal characters";

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
    // by the conditional formatter alone. `list-autodetect-off` is one of
    // those: turning the list formatter's auto-detection off is what hands its
    // `|` format to the plural formatter in the first place.
    #[cfg(not(feature = "plural"))]
    if id.contains("plural") || id.starts_with("autodetect-") || id == "list-autodetect-off" {
        return Some("pluralization needs the \"plural\" feature");
    }
    #[cfg(not(feature = "time"))]
    if has_datetime(&case["args"]) {
        return Some("date/time values need the \"time\" feature");
    }
    // Without IsMatchFormatter registered, every `ismatch` placeholder reports
    // "No suitable Formatter could be found" instead of matching anything.
    #[cfg(not(feature = "regex-formatters"))]
    if id.starts_with("ismatch-") {
        return Some("the ismatch formatter needs the \"regex-formatters\" feature");
    }
    None
}

/// Builds the formatter a case runs with. A case without a `settings` object
/// runs with the .NET defaults; the keys mirror `CaseSettings` in the harness.
fn formatter_for(node: &Json) -> SmartFormatter {
    let mut settings = SmartSettings::default();
    let mut parser_settings = ParserSettings::default();
    let mut extensions = Extensions::default();

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
                "convertCharacterStringLiterals" => {
                    parser_settings.convert_character_string_literals =
                        value.as_bool().expect("a boolean setting");
                }
                "regexOptions" => extensions.regex_options = Some(text().to_owned()),
                "isMatchSplitChar" => {
                    extensions.is_match_split_char = text().chars().next();
                }
                "isMatchPlaceholderName" => {
                    extensions.is_match_placeholder_name = Some(text().to_owned());
                }
                "isMatchCanAutoDetect" => {
                    extensions.is_match_can_auto_detect = value.as_bool();
                }
                "subStringOutOfRangeBehavior" => {
                    extensions.substring_out_of_range = Some(match text() {
                        "ReturnEmptyString" => SubStringOutOfRangeBehavior::ReturnEmptyString,
                        "ReturnStartIndexToEndOfString" => {
                            SubStringOutOfRangeBehavior::ReturnStartIndexToEndOfString
                        }
                        "ThrowException" => SubStringOutOfRangeBehavior::ThrowException,
                        other => panic!("unknown out-of-range behavior {other}"),
                    });
                }
                "subStringNullDisplayString" => {
                    extensions.substring_null_display = Some(text().to_owned());
                }
                "subStringSplitChar" => {
                    extensions.substring_split_char = text().chars().next();
                }
                "subStringCanAutoDetect" => {
                    extensions.substring_can_auto_detect = value.as_bool();
                }
                "isNullSplitChar" => {
                    extensions.is_null_split_char = text().chars().next();
                }
                "isNullCanAutoDetect" => {
                    extensions.is_null_can_auto_detect = value.as_bool();
                }
                "listSplitChar" => {
                    extensions.list_split_char = text().chars().next();
                }
                "listCanAutoDetect" => {
                    extensions.list_can_auto_detect = value.as_bool();
                }
                "templates" => extensions.templates = Some(text().to_owned()),
                other => panic!("unknown setting {other}"),
            }
        }
    }

    parser_settings.error_action = settings.parse_error_action;
    parser_settings.string_format_compatibility = settings.string_format_compatibility;
    let mut smart = SmartFormatter::with_parser_settings(settings, parser_settings);

    // The extension properties are not settings, so they cannot be passed to
    // the constructor: .NET reaches into the built registry with
    // `GetFormatterExtension<T>()` and assigns them. There is no downcast from
    // `dyn Formatter` here, so a case that configures one rebuilds the whole
    // registry instead, letting `FormatterRegistry::add` put each extension at
    // its .NET rank.
    if extensions.needs_custom_registry() {
        *smart.formatters_mut() = extensions.registry();
    }
    if let Some(set) = &extensions.templates {
        for (name, template) in template_fixture(set) {
            smart
                .register_template(name, template)
                .expect("the template fixture registers");
        }
    }

    smart
}

/// The extension configuration a case's `settings` object asks for, mirroring
/// the same keys in the harness's `CaseSettings`. `None` means "as
/// [`FormatterRegistry::new`] builds it".
#[derive(Default)]
struct Extensions {
    regex_options: Option<String>,
    is_match_split_char: Option<char>,
    is_match_placeholder_name: Option<String>,
    is_match_can_auto_detect: Option<bool>,
    substring_out_of_range: Option<SubStringOutOfRangeBehavior>,
    substring_null_display: Option<String>,
    substring_split_char: Option<char>,
    substring_can_auto_detect: Option<bool>,
    is_null_split_char: Option<char>,
    is_null_can_auto_detect: Option<bool>,
    list_split_char: Option<char>,
    list_can_auto_detect: Option<bool>,
    templates: Option<String>,
}

impl Extensions {
    fn needs_custom_registry(&self) -> bool {
        self.regex_options.is_some()
            || self.is_match_split_char.is_some()
            || self.is_match_placeholder_name.is_some()
            || self.is_match_can_auto_detect.is_some()
            || self.substring_out_of_range.is_some()
            || self.substring_null_display.is_some()
            || self.substring_split_char.is_some()
            || self.substring_can_auto_detect.is_some()
            || self.is_null_split_char.is_some()
            || self.is_null_can_auto_detect.is_some()
            || self.list_split_char.is_some()
            || self.list_can_auto_detect.is_some()
    }

    /// The default registry, with the configured extensions in place of the
    /// ones [`FormatterRegistry::new`] would have built. Every formatter here
    /// is well known to [`FormatterRegistry::add`], so the order is the same
    /// one `CreateDefaultSmartFormat` ends up with whatever order they are
    /// added in.
    fn registry(&self) -> FormatterRegistry {
        let mut registry = FormatterRegistry::empty();
        let mut list = ListFormatter::new();
        if let Some(split_char) = self.list_split_char {
            list.set_split_char(split_char)
                .expect("a valid split character");
        }
        if let Some(can_auto_detect) = self.list_can_auto_detect {
            list.set_can_auto_detect(can_auto_detect);
        }
        registry.add(Box::new(list));
        #[cfg(feature = "plural")]
        registry.add(Box::new(PluralLocalizationFormatter::new()));
        registry.add(Box::new(ConditionalFormatter::new()));
        #[cfg(feature = "regex-formatters")]
        {
            let mut is_match = IsMatchFormatter::new();
            if let Some(options) = &self.regex_options {
                is_match.set_regex_options(regex_options(options));
            }
            if let Some(split_char) = self.is_match_split_char {
                is_match
                    .set_split_char(split_char)
                    .expect("a valid split character");
            }
            if let Some(name) = &self.is_match_placeholder_name {
                is_match.set_placeholder_name_for_matches(name.clone());
            }
            if let Some(can_auto_detect) = self.is_match_can_auto_detect {
                is_match.set_can_auto_detect(can_auto_detect);
            }
            registry.add(Box::new(is_match));
        }
        let mut is_null = NullFormatter::new();
        if let Some(split_char) = self.is_null_split_char {
            is_null
                .set_split_char(split_char)
                .expect("a valid split character");
        }
        if let Some(can_auto_detect) = self.is_null_can_auto_detect {
            is_null.set_can_auto_detect(can_auto_detect);
        }
        registry.add(Box::new(is_null));
        registry.add(Box::new(ChooseFormatter::new()));
        let mut substring = SubStringFormatter::new();
        if let Some(behavior) = self.substring_out_of_range {
            substring.set_out_of_range_behavior(behavior);
        }
        if let Some(null_display) = &self.substring_null_display {
            substring.set_null_display_string(null_display.clone());
        }
        if let Some(split_char) = self.substring_split_char {
            substring
                .set_split_char(split_char)
                .expect("a valid split character");
        }
        if let Some(can_auto_detect) = self.substring_can_auto_detect {
            substring.set_can_auto_detect(can_auto_detect);
        }
        registry.add(Box::new(substring));
        registry.add(Box::new(DefaultFormatter));
        registry
    }
}

/// .NET writes a `[Flags]` enum as its comma-separated member names.
#[cfg(feature = "regex-formatters")]
fn regex_options(text: &str) -> RegexOptions {
    text.split(',')
        .map(str::trim)
        .fold(RegexOptions::NONE, |options, name| {
            let flag = match name {
                "None" => RegexOptions::NONE,
                "IgnoreCase" => RegexOptions::IGNORE_CASE,
                "Multiline" => RegexOptions::MULTILINE,
                "ExplicitCapture" => RegexOptions::EXPLICIT_CAPTURE,
                "Compiled" => RegexOptions::COMPILED,
                "Singleline" => RegexOptions::SINGLELINE,
                "IgnorePatternWhitespace" => RegexOptions::IGNORE_PATTERN_WHITESPACE,
                "RightToLeft" => RegexOptions::RIGHT_TO_LEFT,
                "ECMAScript" => RegexOptions::ECMA_SCRIPT,
                "CultureInvariant" => RegexOptions::CULTURE_INVARIANT,
                "NonBacktracking" => RegexOptions::NON_BACKTRACKING,
                other => panic!("unknown RegexOptions member {other}"),
            };
            RegexOptions::from_bits(options.bits() | flag.bits())
        })
}

/// The named template sets the harness's `TemplateFixture` registers, in the
/// same order — the two must agree name for name, because .NET fixes the
/// registry's comparer at construction and rejects a duplicate.
fn template_fixture(set: &str) -> Vec<(&'static str, &'static str)> {
    let standard: Vec<(&'static str, &'static str)> = vec![
        ("firstLast", "{First} {Last}"),
        ("lastFirst", "{Last}, {First}"),
        ("FIRST", "{First.ToUpper}"),
        ("last", "{Last.ToLower}"),
        ("LAST", "{Last.ToUpper}"),
        ("NESTED", "{:t:FIRST} {:t:last}"),
        (r"back\slash", "BS"),
        ("{brace}", "BRACE"),
        ("a|b", "PIPE"),
        ("indexed", "[{Index}] {First}"),
        ("salutation", "{1:cond:{:t:sal_formal}|{:t:sal_informal}}"),
        ("sal_formal", "Dear Mr {Last}"),
        ("sal_informal", "Hi {First}"),
        ("bad", "{Nope}"),
    ];
    match set {
        "Standard" => standard,
        "WithEmptyName" => {
            let mut fixture = standard;
            fixture.push(("", "EMPTY"));
            fixture
        }
        // `LAST` collides with `last` under OrdinalIgnoreCase, and .NET's
        // `Dictionary.Add` throws rather than overwriting.
        "CaseInsensitive" => standard
            .into_iter()
            .filter(|(name, _)| *name != "LAST")
            .collect(),
        "Simple" => vec![("firstLast", "{First} {Last}"), ("x", "X-TEMPLATE")],
        other => panic!("unknown template set {other}"),
    }
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
