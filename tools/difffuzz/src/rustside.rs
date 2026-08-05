//! Renders a case with this port, exactly as `crates/smartformat/tests/goldens.rs`
//! renders a golden one.
//!
//! Everything here mirrors that file: the same JSON-to-`Value` mapping with the
//! same `$dt` / `$ts` / `$f` / `$i32` / `$u64` markers, the same `settings` keys
//! turned into the same `SmartSettings` and extension properties, the same
//! localization, variables and template fixtures, and the same culture lookup.
//! If the two ever drift apart a disagreement here stops meaning anything, so
//! any change to the golden runner has to land here too.
//!
//! Two things it does *not* mirror. There is no `#[cfg(feature = …)]`: the
//! fuzzer builds `smartformat` with its default features, so every formatter is
//! present. And nothing panics on bad input — an unknown setting or a culture
//! the table does not carry is an error the campaign reports, because a
//! generator that starts emitting one must not look like a library bug.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::OnceLock;

use serde_json::{Map, Value as Json};
use smartformat::fmt::culture;
use smartformat::formatter::FormatterRegistry;
use smartformat::parsing::ParserSettings;
use smartformat::sources::variables::{self, PersistentVariablesSource};
use smartformat::{
    CaseSensitivity, Error, ErrorAction, HashMapLocalizationProvider, IsMatchFormatter,
    ListFormatter, NullFormatter, RegexOptions, SmartFormatter, SmartSettings, SubStringFormatter,
    SubStringOutOfRangeBehavior, TimeFormatter, Value,
};

use crate::case::{Case, ErrorKind, RustOutcome};

/// The clock `tools/goldens` pins with `SystemTime.SetDateTime`, which
/// `TimeFormatter` on a `DateTime` and the date conditions read. The harness
/// repeats it in its response's `now` field and the campaign prefers that; this
/// is what a run without the harness uses.
pub const PINNED_NOW: &str = "2026-07-31T12:00:00.0000000";

/// Renders one case. A panic is caught and returned: a library that panics on
/// a generated template is the best thing this tool can find, and losing the
/// rest of the campaign to it would be a poor trade.
pub fn render(case: &Case, now: &str) -> RustOutcome {
    let template = case.template();
    install_quiet_panic_hook();

    RENDERING.with(|flag| flag.set(true));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| render_inner(case, &template, now)));
    RENDERING.with(|flag| flag.set(false));

    match outcome {
        Ok(Ok(text)) => RustOutcome::Result(text),
        Ok(Err(error)) => error,
        Err(payload) => {
            let recorded = PANIC_MESSAGE.with(|slot| slot.borrow_mut().take());
            let message = recorded.unwrap_or_else(|| describe_payload(&payload));
            RustOutcome::Panic(message)
        }
    }
}

fn describe_payload(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with no message".to_string()
    }
}

fn render_inner(case: &Case, template: &str, now: &str) -> Result<String, RustOutcome> {
    let Some(culture) = culture::get(&case.culture) else {
        return Err(RustOutcome::Error {
            kind: ErrorKind::Other,
            message: format!(
                "culture {:?} is not in the generated table (tools/culturegen)",
                case.culture
            ),
        });
    };
    let smart = formatter_for(&case.settings, now).map_err(|message| RustOutcome::Error {
        kind: ErrorKind::Other,
        message,
    })?;
    let args = to_value(&case.args);

    match smart.format_with_culture(template, &args, culture) {
        Ok(text) => Ok(text),
        Err(error) => Err(RustOutcome::Error {
            kind: kind_of(&error),
            message: error.to_string(),
        }),
    }
}

/// The `Error` variants the golden runner's exception-name table distinguishes.
fn kind_of(error: &Error) -> ErrorKind {
    match error {
        Error::Parse { .. } => ErrorKind::Parse,
        Error::Escape { .. } => ErrorKind::Escape,
        Error::Format { .. } => ErrorKind::Format,
        Error::UnsupportedSpec { .. } => ErrorKind::UnsupportedSpec,
        _ => ErrorKind::Other,
    }
}

thread_local! {
    /// Whether this thread is inside [`render`], and a panic is therefore a
    /// finding rather than a fault.
    static RENDERING: Cell<bool> = const { Cell::new(false) };
    static PANIC_MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Keeps a caught panic out of the campaign's output while still recording what
/// it said, so a report can quote it.
///
/// The hook is global and cannot be taken back, so it has to be careful: a
/// panic *outside* a render — this tool's own bug, or a failing assertion in
/// its tests — goes to the hook that was there before, or it would vanish. The
/// flag is per thread because the hook runs on the thread that panicked.
fn install_quiet_panic_hook() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if RENDERING.with(Cell::get) {
                let message = info.to_string();
                PANIC_MESSAGE.with(|slot| *slot.borrow_mut() = Some(message));
            } else {
                previous(info);
            }
        }));
    });
}

// ---------------------------------------------------------------------------
// Settings — mirrors `formatter_for` in the golden runner
// ---------------------------------------------------------------------------

fn formatter_for(node: &Map<String, Json>, now: &str) -> Result<SmartFormatter, String> {
    let mut settings = SmartSettings {
        now: Some(
            now.parse()
                .map_err(|_| format!("the pinned clock {now:?} is not a round-trip date"))?,
        ),
        ..SmartSettings::default()
    };
    let mut parser_settings = ParserSettings::default();
    let mut extensions = Extensions::default();

    for (key, value) in node {
        let text = || -> Result<&str, String> {
            value
                .as_str()
                .ok_or_else(|| format!("setting {key} is not a string"))
        };
        let flag = || -> Result<bool, String> {
            value
                .as_bool()
                .ok_or_else(|| format!("setting {key} is not a boolean"))
        };
        let first_char = || -> Result<char, String> {
            text()?
                .chars()
                .next()
                .ok_or_else(|| format!("setting {key} is empty"))
        };
        match key.as_str() {
            "formatErrorAction" => settings.format_error_action = error_action(text()?)?,
            "parseErrorAction" => settings.parse_error_action = error_action(text()?)?,
            "caseSensitivity" => {
                settings.case_sensitive = match text()? {
                    "CaseSensitive" => CaseSensitivity::CaseSensitive,
                    "CaseInsensitive" => CaseSensitivity::CaseInsensitive,
                    other => return Err(format!("unknown case sensitivity {other}")),
                }
            }
            "stringFormatCompatibility" => settings.string_format_compatibility = flag()?,
            "alignmentFillCharacter" => settings.alignment_fill_character = first_char()?,
            "customSelectorChars" => parser_settings
                .add_custom_selector_chars(text()?.chars())
                .map_err(|error| format!("custom selector characters: {error}"))?,
            "convertCharacterStringLiterals" => {
                parser_settings.convert_character_string_literals = flag()?;
            }
            "regexOptions" => extensions.regex_options = Some(text()?.to_owned()),
            "isMatchSplitChar" => extensions.is_match_split_char = Some(first_char()?),
            "isMatchPlaceholderName" => extensions.is_match_placeholder_name = Some(text()?.into()),
            "isMatchCanAutoDetect" => extensions.is_match_can_auto_detect = Some(flag()?),
            "subStringOutOfRangeBehavior" => {
                extensions.substring_out_of_range = Some(match text()? {
                    "ReturnEmptyString" => SubStringOutOfRangeBehavior::ReturnEmptyString,
                    "ReturnStartIndexToEndOfString" => {
                        SubStringOutOfRangeBehavior::ReturnStartIndexToEndOfString
                    }
                    "ThrowException" => SubStringOutOfRangeBehavior::ThrowException,
                    other => return Err(format!("unknown out-of-range behavior {other}")),
                });
            }
            "subStringNullDisplayString" => {
                extensions.substring_null_display = Some(text()?.to_owned());
            }
            "subStringSplitChar" => extensions.substring_split_char = Some(first_char()?),
            "subStringCanAutoDetect" => extensions.substring_can_auto_detect = Some(flag()?),
            "isNullSplitChar" => extensions.is_null_split_char = Some(first_char()?),
            "isNullCanAutoDetect" => extensions.is_null_can_auto_detect = Some(flag()?),
            "listSplitChar" => extensions.list_split_char = Some(first_char()?),
            "listCanAutoDetect" => extensions.list_can_auto_detect = Some(flag()?),
            "templates" => extensions.templates = Some(text()?.to_owned()),
            "variables" => extensions.variables = Some(text()?.to_owned()),
            "localization" => extensions.localization = Some(text()?.to_owned()),
            other => return Err(format!("unknown setting {other}")),
        }
    }

    parser_settings.error_action = settings.parse_error_action;
    parser_settings.string_format_compatibility = settings.string_format_compatibility;
    let mut smart = SmartFormatter::with_parser_settings(settings, parser_settings);

    extensions.configure(smart.formatters_mut())?;
    if let Some(set) = &extensions.templates {
        for (name, template) in template_fixture(set)? {
            smart
                .register_template(name, template)
                .map_err(|error| format!("template fixture {set}: {error}"))?;
        }
    }

    smart.formatters_mut().add(Box::new(TimeFormatter::new()));
    smart.register_localization(Box::new(localization_fixture(
        extensions.localization.as_deref().unwrap_or("Standard"),
    )?));
    if let Some(set) = &extensions.variables {
        smart.register_variables(variables_fixture(set)?);
    }

    Ok(smart)
}

fn error_action(name: &str) -> Result<ErrorAction, String> {
    Ok(match name {
        "ThrowError" => ErrorAction::Error,
        "Ignore" => ErrorAction::Ignore,
        "MaintainTokens" => ErrorAction::MaintainTokens,
        "OutputErrorInResult" => ErrorAction::OutputErrorInResult,
        other => return Err(format!("unknown error action {other}")),
    })
}

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
    variables: Option<String>,
    localization: Option<String>,
}

impl Extensions {
    fn configure(&self, registry: &mut FormatterRegistry) -> Result<(), String> {
        let list = registry
            .get_mut::<ListFormatter>()
            .ok_or("the default registry holds a list formatter")?;
        if let Some(split_char) = self.list_split_char {
            list.set_split_char(split_char)
                .map_err(|error| error.to_string())?;
        }
        if let Some(can_auto_detect) = self.list_can_auto_detect {
            list.set_can_auto_detect(can_auto_detect);
        }

        let is_match = registry
            .get_mut::<IsMatchFormatter>()
            .ok_or("the default registry holds an ismatch formatter")?;
        if let Some(options) = &self.regex_options {
            is_match.set_regex_options(regex_options(options)?);
        }
        if let Some(split_char) = self.is_match_split_char {
            is_match
                .set_split_char(split_char)
                .map_err(|error| error.to_string())?;
        }
        if let Some(name) = &self.is_match_placeholder_name {
            is_match.set_placeholder_name_for_matches(name.clone());
        }
        if let Some(can_auto_detect) = self.is_match_can_auto_detect {
            is_match.set_can_auto_detect(can_auto_detect);
        }

        let is_null = registry
            .get_mut::<NullFormatter>()
            .ok_or("the default registry holds an isnull formatter")?;
        if let Some(split_char) = self.is_null_split_char {
            is_null
                .set_split_char(split_char)
                .map_err(|error| error.to_string())?;
        }
        if let Some(can_auto_detect) = self.is_null_can_auto_detect {
            is_null.set_can_auto_detect(can_auto_detect);
        }

        let substring = registry
            .get_mut::<SubStringFormatter>()
            .ok_or("the default registry holds a substr formatter")?;
        if let Some(behavior) = self.substring_out_of_range {
            substring.set_out_of_range_behavior(behavior);
        }
        if let Some(null_display) = &self.substring_null_display {
            substring.set_null_display_string(null_display.clone());
        }
        if let Some(split_char) = self.substring_split_char {
            substring
                .set_split_char(split_char)
                .map_err(|error| error.to_string())?;
        }
        if let Some(can_auto_detect) = self.substring_can_auto_detect {
            substring.set_can_auto_detect(can_auto_detect);
        }
        Ok(())
    }
}

/// .NET writes a `[Flags]` enum as its comma-separated member names.
fn regex_options(text: &str) -> Result<RegexOptions, String> {
    let mut bits = RegexOptions::NONE.bits();
    for name in text.split(',').map(str::trim) {
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
            other => return Err(format!("unknown RegexOptions member {other}")),
        };
        bits |= flag.bits();
    }
    Ok(RegexOptions::from_bits(bits))
}

/// The table in the harness's `LocalizationFixture`, entry for entry.
fn localization_fixture(set: &str) -> Result<HashMapLocalizationProvider, String> {
    let provider = HashMapLocalizationProvider::from_triples([
        ("", "WeTranslateText", "We translate text"),
        ("es", "WeTranslateText", "Traducimos el texto"),
        ("fr", "WeTranslateText", "Nous traduisons des textes"),
        ("de", "WeTranslateText", "Wir übersetzen Text"),
        ("zh-Hans", "WeTranslateText", "我们翻译文本"),
        ("de", "OnlyGerman", "Nur auf Deutsch"),
        (
            "",
            "OnlyExistForInvariantCulture",
            "This entry only exists in the invariant culture resource",
        ),
        ("", "has {:N0} inhabitants", "has {:N0} inhabitants"),
        ("es", "has {:N0} inhabitants", "tiene {:N0} habitantes"),
        ("fr", "has {:N0} inhabitants", "compte {:N0} habitants"),
        ("de", "has {:N0} inhabitants", "hat {:N0} Einwohner"),
        (
            "",
            "{0} has {1:N0} inhabitants",
            "{0} has {1:N0} inhabitants",
        ),
        (
            "es",
            "{0} has {1:N0} inhabitants",
            "{0} tiene {1:N0} habitantes",
        ),
        ("", "{} item", "{} item"),
        ("", "{} items", "{} items"),
        ("es", "{} item", "{} elemento"),
        ("es", "{} items", "{} elementos"),
        ("fr", "{} item", "{} élément"),
        ("fr", "{} items", "{} éléments"),
        ("de", "{} item", "{} Element"),
        ("de", "{} items", "{} Elemente"),
        ("", "greet", "Hello, {Name}!"),
        ("de", "greet", "Hallo, {Name}!"),
        ("", "Outer", "<{:L:Inner}>"),
        ("", "Inner", "INNER"),
        ("", "OuterMissingInner", "<{:L:NoSuchInner}>"),
        ("", "greetNobody", "Hello {Nope}!"),
        ("", "a{b", "escaped"),
        ("", "BadParse", "{0:"),
        ("", "K1", "abc {0}"),
        ("", "K2", "ABC {0}"),
        ("", "paper", "Paper"),
        ("de", "paper", "das Papier"),
        ("fr", "paper", "Papier"),
        ("", "{RawKey}", "the raw text won"),
        ("", "0", "first"),
        ("", "1", "second"),
        ("", "1,234,567", "the ambient culture rendered the key"),
        ("", "1.234.567", "the culture in force rendered the key"),
    ]);
    Ok(match set {
        "Standard" => provider,
        "Fallback" => provider.with_fallback_culture(culture::get("de").ok_or("de is shipped")?),
        "ReturnName" => provider.with_return_name_if_not_found(true),
        other => return Err(format!("unknown localization set {other}")),
    })
}

/// The groups the harness's `VariablesFixture` registers, group for group.
fn variables_fixture(set: &str) -> Result<PersistentVariablesSource, String> {
    let mut source = PersistentVariablesSource::new();
    match set {
        "Standard" => {
            source.add(
                "global",
                variables::group([
                    ("theVariable", Value::from("persistent-value")),
                    (
                        "nested",
                        Value::Map(variables::group([("inner", Value::Int(42))])),
                    ),
                    ("nullVar", Value::Null),
                    ("Index", Value::Int(7)),
                ]),
            );
            source.add(
                "v",
                variables::group([
                    ("i", Value::Int(1234)),
                    ("b", Value::Bool(true)),
                    ("s", Value::from("str")),
                    (
                        "dt",
                        Value::DateTime(
                            "2024-12-31T00:00:00.0000000"
                                .parse()
                                .map_err(|_| "the fixture date parses")?,
                        ),
                    ),
                    (
                        "list",
                        Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]),
                    ),
                ]),
            );
        }
        "Precedence" => {
            source.add(
                "global",
                variables::group([("theVariable", Value::from("val-from-persistent-source"))]),
            );
        }
        "Shadowing" => {
            source.add("Length", variables::group([("v", Value::Int(7))]));
        }
        other => return Err(format!("unknown variable set {other}")),
    }
    Ok(source)
}

/// The named template sets the harness's `TemplateFixture` registers, in order.
fn template_fixture(set: &str) -> Result<Vec<(&'static str, &'static str)>, String> {
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
    Ok(match set {
        "Standard" => standard,
        "WithEmptyName" => {
            let mut fixture = standard;
            fixture.push(("", "EMPTY"));
            fixture
        }
        "CaseInsensitive" => standard
            .into_iter()
            .filter(|(name, _)| *name != "LAST")
            .collect(),
        "Simple" => vec![("firstLast", "{First} {Last}"), ("x", "X-TEMPLATE")],
        other => return Err(format!("unknown template set {other}")),
    })
}

// ---------------------------------------------------------------------------
// Arguments — mirrors `to_value` in the golden runner
// ---------------------------------------------------------------------------

/// The JSON-to-[`Value`] mapping documented in `tools/goldens/README.md`.
pub fn to_value(node: &Json) -> Value {
    match node {
        Json::Null => Value::Null,
        Json::Bool(v) => Value::Bool(*v),
        Json::Number(v) => match v.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(v.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(v) => Value::String(v.clone()),
        Json::Array(items) => Value::List(items.iter().map(to_value).collect()),
        Json::Object(entries) => match marker(entries) {
            Some(("$dt", text)) => text.parse().map(Value::DateTime).unwrap_or(Value::Null),
            Some(("$ts", text)) => time_span(text),
            Some(("$i32", text)) => Value::Int(text.parse::<i32>().unwrap_or(0).into()),
            Some(("$u64", text)) => Value::UInt(text.parse().unwrap_or(0)),
            Some(("$f", text)) => Value::Float(match text {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                other => other.parse().unwrap_or(f64::NAN),
            }),
            Some(_) | None => Value::Map(
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
    Some((key, value.as_str()?))
}

/// A `TimeSpan` in .NET's round-trip (`c`) format, `[-][d.]hh:mm:ss[.fffffff]`.
fn time_span(text: &str) -> Value {
    const TICKS_PER_SECOND: i128 = 10_000_000;

    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (days, rest) = match text.split_once('.') {
        Some((days, rest)) if !days.contains(':') => (days.parse::<i128>().unwrap_or(0), rest),
        _ => (0, text),
    };
    let (clock, fraction) = match rest.split_once('.') {
        Some((clock, fraction)) => (clock, fraction),
        None => (rest, "0000000"),
    };
    let mut parts = clock
        .split(':')
        .map(|part| part.parse::<i128>().unwrap_or(0));
    let hours = parts.next().unwrap_or(0);
    let minutes = parts.next().unwrap_or(0);
    let seconds = parts.next().unwrap_or(0);

    let ticks = ((days * 24 + hours) * 60 + minutes) * 60 + seconds;
    let ticks = ticks * TICKS_PER_SECOND + fraction.parse::<i128>().unwrap_or(0);
    let ticks = if negative { -ticks } else { ticks };

    let seconds = i64::try_from(ticks.div_euclid(TICKS_PER_SECOND)).unwrap_or(0);
    let nanoseconds = (ticks.rem_euclid(TICKS_PER_SECOND) * 100) as i32;
    Value::TimeSpan(jiff::SignedDuration::new(seconds, nanoseconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen;

    #[test]
    fn every_generated_case_renders_without_panicking() {
        for index in 0..400 {
            let case = gen::generate(20260731, index);
            let outcome = render(&case, PINNED_NOW);
            assert!(
                !matches!(outcome, RustOutcome::Panic(_)),
                "case {} panicked on template {:?}: {outcome}",
                case.id,
                case.template()
            );
        }
    }

    #[test]
    fn a_generated_case_never_asks_for_a_setting_the_runner_does_not_know() {
        for index in 0..400 {
            let case = gen::generate(7, index);
            if let RustOutcome::Error {
                kind: ErrorKind::Other,
                message,
            } = render(&case, PINNED_NOW)
            {
                panic!("case {} could not even be set up: {message}", case.id);
            }
        }
    }

    #[test]
    fn most_generated_cases_reach_the_rendering_path() {
        let mut rendered = 0;
        let total = 400;
        for index in 0..total {
            let case = gen::generate(31337, index);
            if matches!(render(&case, PINNED_NOW), RustOutcome::Result(_)) {
                rendered += 1;
            }
        }
        // A corpus that is all errors tests one code path. The generator emits
        // malformed shapes on purpose, so this is a floor, not a target.
        assert!(
            rendered * 2 > total,
            "only {rendered} of {total} generated cases rendered"
        );
    }
}
