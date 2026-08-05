# Settings and features

Every setting a formatter and its parser carry, and every cargo feature.

Source of truth: `crates/smartformat/src/settings.rs`,
`crates/smartformat/src/parsing/settings.rs` and
`crates/smartformat/Cargo.toml`.

## SmartSettings

Passed to `SmartFormatter::new`, readable through `SmartFormatter::settings`.
`SmartSettings::default()` is .NET's default in every field.

| Field | Type | Default | .NET counterpart | Effect |
| --- | --- | --- | --- | --- |
| `parse_error_action` | `ErrorAction` | `Error` | `SmartSettings.Parser.ParseErrorAction` | how syntax errors are handled |
| `format_error_action` | `ErrorAction` | `Error` | `SmartSettings.Formatter.FormatErrorAction` | how errors raised while rendering a placeholder are handled |
| `case_sensitive` | `CaseSensitivity` | `CaseSensitive` | `SmartSettings.CaseSensitivity` | how selector names, formatter names and template names are matched |
| `string_format_compatibility` | `bool` | `false` | `SmartSettings.StringFormatCompatibility` | braces are escaped by doubling, formatter names are not parsed, and only `DefaultFormatter` runs |
| `now` | `Option<jiff::civil::DateTime>` | `None` | `SystemTime.Now`, which `SystemTime.SetDateTime` pins | what "now" means to the `time` formatter and to date conditions in `cond`; `None` reads the system clock per placeholder. Requires the `time` feature |
| `alignment_fill_character` | `char` | `' '` | `FormatterSettings.AlignmentFillCharacter` | the character alignment pads with |

```rust
use smartformat::{SmartFormatter, SmartSettings, Value};

let mut settings = SmartSettings::default();
settings.alignment_fill_character = '.';
let smart = SmartFormatter::new(settings);

let args = Value::List(vec![Value::from("x")]);
assert_eq!(smart.format("{0,5}", &args).unwrap(), "....x");
```

### ErrorAction

One enum, two mechanisms. `parse_error_action` decides what tree the parser
builds for text it could not read; `format_error_action` decides what the
engine writes for a placeholder that failed to render.

| Variant | .NET | At parse time | At format time |
| --- | --- | --- | --- |
| `Error` | `ThrowError` | `Err(Error::Parse)` | `Err(Error::Format)` |
| `OutputErrorInResult` | `OutputErrorInResult` | the whole template is replaced by the error report | the error message is written in place of the placeholder |
| `Ignore` | `Ignore` | erroneous placeholders are dropped | the placeholder writes nothing |
| `MaintainTokens` | `MaintainTokens` | erroneous placeholders stay as literal text | the placeholder is rebuilt from its parsed parts and written verbatim |

[Choose what happens when a template or a value is wrong](../how-to/choose-error-behavior.md)
renders one broken template under all four actions, at parse time and at format
time.

Two errors escape `format_error_action`: an escape sequence in a literal of the
top-level format, and formatter options whose escape sequences do not resolve.
Both fail the call whatever the action is, as they do in .NET.

```rust
use smartformat::{ErrorAction, SmartFormatter, SmartSettings, Value};

let mut settings = SmartSettings::default();
settings.format_error_action = ErrorAction::Ignore;
let smart = SmartFormatter::new(settings);

// `Missing` resolves to nothing, and the placeholder writes nothing.
assert_eq!(smart.format("[{Missing}]", &Value::Null).unwrap(), "[]");
```

### CaseSensitivity

| Variant | .NET | Comparison |
| --- | --- | --- |
| `CaseSensitive` | `StringComparison.Ordinal` | byte-for-byte |
| `CaseInsensitive` | `StringComparison.OrdinalIgnoreCase` | invariant *simple* uppercase mapping per code unit, so `ä` matches `Ä` but `ß` never matches `SS` |

## ParserSettings

Read once, when the `Parser` is created; changing them afterwards has no
effect on an existing parser, as in .NET.

| Field | Type | Default | .NET counterpart | Effect |
| --- | --- | --- | --- | --- |
| `error_action` | `ErrorAction` | `Error` | `ParserSettings.ParseErrorAction` | how syntax errors are handled |
| `convert_character_string_literals` | `bool` | `true` | `ParserSettings.ConvertCharacterStringLiterals` | whether `\n`, `\t`, `\uXXXX` … resolve; with it off only `\\` still does |
| `selector_char_filter` | `SelectorFilter` | `Alphanumeric` | `ParserSettings.SelectorCharFilter` | which characters a selector may contain |
| `string_format_compatibility` | `bool` | `false` | `SmartSettings.StringFormatCompatibility` | doubled-brace escaping, no formatter names |
| `custom_selector_chars` | `Vec<char>` | empty | `ParserSettings.AddCustomSelectorChars` | extra selector characters |
| `custom_operator_chars` | `Vec<char>` | empty | `ParserSettings.AddCustomOperatorChars` | extra selector-separating characters |

Prefer `add_custom_selector_chars` and `add_custom_operator_chars` over
assigning the vectors: they reject a character that already has a meaning,
returning `CustomCharError`.

| Rejected as a custom selector char | Rejected as a custom operator char |
| --- | --- |
| `:` `{` `}` `(` `)`, `\`, the operator chars `.` `?` `,` `[` `]`, and any registered custom operator char | `:` `{` `}` `(` `)` and any registered custom selector char |

### How the two settings objects relate

.NET keeps one `SmartSettings` that owns its `ParserSettings`, so the two views
are the same object. Here they are separate structs and two fields overlap.

| Constructor | Direction |
| --- | --- |
| `SmartFormatter::new(settings)` | `ParserSettings::default()` with `error_action` taken from `parse_error_action` and `string_format_compatibility` copied over |
| `SmartFormatter::with_parser_settings(settings, parser_settings)` | the parser settings win: `error_action` and `string_format_compatibility` are copied back into `SmartSettings`, so `settings()` and `parser().settings()` can never disagree |

Use `with_parser_settings` for anything the derived path cannot express, such
as a custom selector character set.

```rust
use smartformat::parsing::{ParserSettings, SelectorFilter};
use smartformat::{SmartFormatter, SmartSettings, Value};

let mut parser_settings = ParserSettings::default();
parser_settings.selector_char_filter = SelectorFilter::VisualUnicodeChars;
let smart = SmartFormatter::with_parser_settings(SmartSettings::default(), parser_settings);

let args = Value::Map(
    [("naïve".to_owned(), Value::Int(1))].into_iter().collect(),
);
assert_eq!(smart.format("{naïve}", &args).unwrap(), "1");
```

## Cargo features

`default = ["derive", "plural", "time", "regex-formatters"]`.

| Feature | Gates | Pulls in | Implies |
| --- | --- | --- | --- |
| `derive` | the `ToSmartValue` derive macro re-export | `smartformat-derive` | none |
| `plural` | `PluralLocalizationFormatter`, `extensions::plural_rules` | nothing | none |
| `time` | `Value::DateTime`, `Value::TimeSpan`, `TimeFormatter`, `fmt::date`, `SmartSettings::now`, the date arms of `cond` | `jiff` | `plural` |
| `regex-formatters` | `IsMatchFormatter`, `RegexOptions` | `fancy-regex` | none |

`time` implies `plural` because a `TimeTextInfo` picks its unit words with the
plural rules, exactly as .NET's does; there is no second copy of the table.

The pluralization rules are a port of SmartFormat.NET's own table, so `plural`
adds no dependency and no CLDR data.

Without a feature, the corresponding formatter is simply absent from the
default registry, and a template that names it fails with
`No suitable Formatter could be found`.

The crate is not published; depend on it by git or path.

```toml
[dependencies]
smartformat = { git = "https://github.com/…/smartformat-rs", default-features = false, features = ["plural"] }
```

## Related

- [template-syntax.md](template-syntax.md): what the parser settings change.
- [formatters.md](formatters.md): which formatter each feature gates.
- [how-to/choose-error-behavior.md](../how-to/choose-error-behavior.md): the four error actions with worked output.
- [explanation/byte-compatibility.md](../explanation/byte-compatibility.md): why every default is .NET's rather than a better one.
- [DESIGN.md](../../DESIGN.md): why the settings are split into two structs, and
  the policy differences from .NET.
