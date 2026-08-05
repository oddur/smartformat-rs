# Formatters

Every formatter extension, what selects it, what it reads, and what it fails
with. New here? [Get started with smartformat](../tutorials/getting-started.md)
teaches two of them by building a message.

Source of truth: `crates/smartformat/src/extensions/` and
`crates/smartformat/src/formatter.rs`.

## The registry

`FormatterRegistry::new()` builds this list, in this order. The order is .NET's
`CreateDefaultSmartFormat` order, which is `WellKnownExtensionTypes` sorted by
rank.

| Order | Name | Type | Auto-detects | In the default registry | .NET rank |
| --- | --- | --- | --- | --- | --- |
| 0 | `list` | `ListFormatter` | yes | yes | 1000 |
| 1 | `plural` | `PluralLocalizationFormatter` | yes | yes, with the `plural` feature | 2000 |
| 2 | `cond` | `ConditionalFormatter` | yes | yes | 3000 |
| — | `time` | `TimeFormatter` | no | no, register by hand | 4000 |
| 3 | `ismatch` | `IsMatchFormatter` | no | yes, with the `regex-formatters` feature | 6000 |
| 4 | `isnull` | `NullFormatter` | no | yes | 7000 |
| — | `L` | `LocalizationFormatter` | no | no, `register_localization` | 8000 |
| — | `t` | `TemplateFormatter` | no | no, `register_template` | 9000 |
| 5 | `choose` | `ChooseFormatter` | no | yes | 10000 |
| 6 | `substr` | `SubStringFormatter` | no | yes | 11000 |
| 7 | `d` | `DefaultFormatter` | yes | yes | 12000 |

`time`, `L` and `t` are left out of the default registry because .NET leaves
them out: each is useless until it is given a language, a provider or a
template. `FormatterRegistry::add` inserts a formatter at the rank above;
`FormatterRegistry::insert` puts it at an index of your choosing. A formatter
whose name is not in the rank table is appended, which places it after
`DefaultFormatter`, where it never runs. [How a render happens](../explanation/architecture.md)
explains why the order is observable at all;
[Write your own formatter or source](../how-to/extend-with-your-own.md) covers
registering one.

Every formatter but `d` can be renamed with `with_name`. Auto-detection is
switchable with `set_can_auto_detect` on every formatter except `L`, `t` and
`d`: the first two never auto-detect, and `d` always does.

## Selection

| Placeholder | Formatter chosen |
| --- | --- |
| `{0:name:…}` | the first registered formatter whose name equals `name`, compared per [`SmartSettings::case_sensitive`](settings-and-features.md#smartsettings) |
| `{0:…}` with no name | auto-detection |
| any, with `string_format_compatibility` | the first `DefaultFormatter` in the registry, always |

Auto-detection walks the registry in order, skipping formatters whose
`can_auto_detect` is false, and calls each one until one returns "handled". A
formatter declines by returning `Ok(false)`, typically because the value type
or the number of format parts does not suit it. `DefaultFormatter` sits last
and handles nearly everything, so auto-detection rarely runs off the end.

Because `list`, `plural` and `cond` all auto-detect and all split the format on
`|`, the first of the three that accepts the value decides what `{0:a|b}`
means.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();

// A list: `list` answers first, so `one` is the item format and `many` the spacer.
let list = Value::List(vec![Value::List(vec![Value::from("a"), Value::from("b")])]);
assert_eq!(smart.format("{0:one|many}", &list).unwrap(), "amanyb");

// A number: `list` declines, `plural` answers.
let three = Value::List(vec![Value::Int(3)]);
assert_eq!(smart.format("{0:one|many}", &three).unwrap(), "many");

// A bool: `list` and `plural` decline, `cond` answers.
let yes = Value::List(vec![Value::Bool(true)]);
assert_eq!(smart.format("{0:one|many}", &yes).unwrap(), "one");
```

A named formatter that cannot handle the value raises its own error; a
placeholder that no formatter handles raises
`No suitable Formatter could be found`.

## Format parts

The `|`-splitting formatters split the placeholder's format on their split
character, at the top nesting level only: a `|` inside a nested placeholder
does not split. A format with no split character is one part.

The split character is `|` for every formatter but `substr`, which uses `,`.
`set_split_char` accepts `|`, `,` and `~` and nothing else
(`Only '|', ',' and '~' are valid split chars.`).

## `list`

| | |
| --- | --- |
| Name | `list` (`ListFormatter`) |
| Auto-detects | yes |
| Registered by default | yes, at index 0 |
| Options | none |
| Split character | `\|`, at most 4 separators; only the first 4 parts are read |
| Value types | `Value::List` |

| Parts | Meaning |
| --- | --- |
| 1 | declined: at least 2 are required |
| 2 | item format, spacer |
| 3 | item format, spacer, spacer before the last item |
| 4 | item format, spacer, last spacer, spacer for a list of exactly two |

The item format is rendered against each item, with `{}` as the item and
`{Index}` as its position. An item format with no nested placeholder is treated
as a specifier for the item. Spacers are rendered against the value the format
call was made with, not against an item, and do not inherit the placeholder's
alignment.

`{TheList?:list:{}|, }` on a null value writes nothing instead of failing.

| Error | Raised when |
| --- | --- |
| `Formatter named 'list' requires an IEnumerable argument and at least 2 format parameters.` | the value is not a list, or there are fewer than 2 parts |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let names = Value::List(vec![Value::List(vec![
    Value::from("Jim"),
    Value::from("Pam"),
    Value::from("Dwight"),
])]);
assert_eq!(
    smart.format("{0:list:{}|, |, and }", &names).unwrap(),
    "Jim, Pam, and Dwight"
);
assert_eq!(smart.format("{0:list:{Index}. {}|; }", &names).unwrap(), "0. Jim; 1. Pam; 2. Dwight");
```

## `plural`

| | |
| --- | --- |
| Name | `plural` (`PluralLocalizationFormatter`) |
| Auto-detects | yes |
| Registered by default | yes, with the `plural` feature |
| Options | a culture name, as in `{0:plural(fr):…}` |
| Split character | `\|` |
| Value types | `Value::Int`, `Value::UInt`, `Value::Float`, `Value::List` (pluralized by item count) |

The parts are the plural words of the language, in the order the language's
rule indexes them. How many there are is the language's business: two for
English, three for Russian, and so on. See
`crates/smartformat/src/extensions/plural_rules.rs` for the whole table.

Language precedence: the culture named in the options, then a custom rule set
with `PluralLocalizationFormatter::with_custom_rule`, then the culture of the
format call. The invariant culture counts as English.

Auto-detection declines a format of one part, and any value that is not a
number or a list.

| Error | Raised when |
| --- | --- |
| `Formatter named 'plural' can format numbers and IEnumerables, but the argument was of type '<type>'` | called by name on an unsupported type |
| `Invalid number of plural parameters in PluralLocalizationFormatter` | the language's rule cannot serve that many parts |
| `Culture is not supported. (Parameter 'name')` + `<name> is an invalid culture identifier.` | the options name is not a well-formed culture name |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let template = "There {0:plural:is|are} {0} {0:plural:item|items} remaining";
let one = Value::List(vec![Value::Int(1)]);
let two = Value::List(vec![Value::Int(2)]);
assert_eq!(smart.format(template, &one).unwrap(), "There is 1 item remaining");
assert_eq!(smart.format(template, &two).unwrap(), "There are 2 items remaining");

// Polish has three plural words.
let five = Value::List(vec![Value::Int(5)]);
assert_eq!(smart.format("{0:plural(pl):jeden|dwa|piec}", &two).unwrap(), "dwa");
assert_eq!(smart.format("{0:plural(pl):jeden|dwa|piec}", &five).unwrap(), "piec");
```

## `cond`

| | |
| --- | --- |
| Name | `cond` (`ConditionalFormatter`) |
| Auto-detects | yes |
| Registered by default | yes |
| Options | none |
| Split character | `\|` |
| Value types | numbers, `Value::Bool`, `Value::String`, `Value::Null`, `Value::DateTime`, `Value::TimeSpan` |

Which part is chosen depends on the value's type:

| Value | 2 parts | 3 or more parts |
| --- | --- | --- |
| number | the part its floor indexes, clamped to the last; negative picks the last | same |
| `Bool` | `true\|false` | `true\|false`, extra parts unused |
| `String` | `has-value\|empty` | same |
| `Null` | the second part | same |
| `DateTime` | `past-or-present\|future` | `past\|present\|future`, where present means today |
| `TimeSpan` | `negative-or-zero\|positive` | `negative\|zero\|positive` |

A part may instead start with a *complex condition*, which is matched against
the raw text of the part: one or more comparisons followed by `?`.

| Element | Accepted |
| --- | --- |
| comparer | `>` `>=` `<` `<=` `=` `==` `!` `!=` |
| value | digits, `.` and `-` |
| joiner | `&` (and), `/` (or), folded left to right with no precedence |
| terminator | `?`, after which the rest of the part is the output |

Complex conditions only apply to numeric values, and only when the *first*
part carries one. A part without a condition acts as the else branch.

Every error this formatter raises is a plain exception message with no
`Error parsing format string: … at N` envelope.

| Error | Raised when |
| --- | --- |
| `Formatter named 'cond' requires at least 2 format parameters.` | fewer than 2 parts |
| `Value was either too large or too small for a Decimal.` | a float outside `decimal` range |
| `Value was either too large or too small for an Int32.` | the floor of a number exceeds `int` range |
| `Specified argument was out of the range of valid values. (Parameter 'index')` | every part is a complex condition and none holds |
| `The input string '<text>' was not in a correct format.` | a condition compares against text `decimal.Parse` rejects |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let two = Value::List(vec![Value::Int(2)]);
assert_eq!(smart.format("{0:cond:none|one|many}", &two).unwrap(), "many");
assert_eq!(smart.format("{0:cond:>1?many|few}", &two).unwrap(), "many");
assert_eq!(smart.format("{0:cond:>=1&<=3?some|other}", &two).unwrap(), "some");
```

## `ismatch`

| | |
| --- | --- |
| Name | `ismatch` (`IsMatchFormatter`) |
| Auto-detects | no |
| Registered by default | yes, with the `regex-formatters` feature |
| Options | the regular expression |
| Split character | `\|`, exactly 2 parts required |
| Value types | anything with text: strings, numbers, bools, dates, durations |

The first part renders on a match, the second on no match. Inside the matched
part, the capture groups are reachable as `{m[0]}`, `{m[1]}`, … where `{m[0]}`
is the whole match; the name `m` is settable with
`set_placeholder_name_for_matches`.

The options carry SmartFormat's own escaping, so `(` and `)` of a capturing
group are written `\(` and `\)`, and a regex backslash is written `\\`.

The engine is fancy-regex, not .NET's. `set_regex_options` takes
`RegexOptions` flags with .NET's names and bit values: `IGNORE_CASE`,
`MULTILINE`, `SINGLELINE` and `IGNORE_PATTERN_WHITESPACE` are honoured,
`COMPILED` and `CULTURE_INVARIANT` are accepted and ignored, and
`EXPLICIT_CAPTURE`, `RIGHT_TO_LEFT`, `ECMA_SCRIPT` and `NON_BACKTRACKING` are
rejected. `set_backtrack_limit` stands in for .NET's 500 ms match timeout and
defaults to 1,000,000 steps.

| Error | Raised when |
| --- | --- |
| `Formatter named 'ismatch' requires at least 2 format options.` | the format does not have exactly 2 parts |
| `Matching a list or a map with "ismatch" is not supported; select a value from it` | the value is a list or a map |
| `Regular expression "…" exceeded the backtrack limit of N steps` | the match ran too long |
| `Regular expression "…" failed to match: …` | any other match failure |
| fancy-regex's own parse error | the pattern does not compile |

Divergences between the two regex engines are listed in the module
documentation (`cargo doc --open`, `smartformat::extensions::ismatch`) and in
[DESIGN.md](../../DESIGN.md).

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::from("Some123Content")]);
let template = r"{0:ismatch(^\\D+\(\\d+\)\\D+$):digits {m[1]}|no digits}";
assert_eq!(smart.format(template, &args).unwrap(), "digits 123");
```

## `isnull`

| | |
| --- | --- |
| Name | `isnull` (`NullFormatter`) |
| Auto-detects | no |
| Registered by default | yes |
| Options | none accepted |
| Split character | `\|`, 1 or 2 parts |
| Value types | any |

| Parts | Null value | Other value |
| --- | --- | --- |
| 1 | the part | empty output |
| 2 | the first part | the second part |

| Error | Raised when |
| --- | --- |
| `Formatter named 'isnull' does not allow choose options` | anything is written in the parentheses |
| `Formatter named 'isnull' must have 1 or 2 format options` | the format has 3 or more parts |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Null, Value::from("here")]);
assert_eq!(smart.format("{0:isnull:nothing|{}}", &args).unwrap(), "nothing");
assert_eq!(smart.format("{1:isnull:nothing|{}}", &args).unwrap(), "here");
assert_eq!(smart.format("{1:isnull:nothing}", &args).unwrap(), "");
```

## `choose`

| | |
| --- | --- |
| Name | `choose` (`ChooseFormatter`) |
| Auto-detects | no |
| Registered by default | yes |
| Options | the choices, split on `\|` |
| Split character | `\|` for both options and format |
| Value types | any |

The value's text is compared against each option; the format part at the
matching position wins. One part more than there are options makes the last
part the else branch.

| Value | Compared as |
| --- | --- |
| `Null` | `null`, case-insensitively whatever the formatter's setting |
| `Bool` | `True` / `False`, case-insensitively whatever the setting |
| number, date | its text under the culture of the call, per the formatter's case sensitivity |
| duration | its `TimeSpan.ToString()` text, which no culture changes |
| `String` | itself, per the formatter's case sensitivity (`set_case_sensitivity`) |
| `List`, `Map` | nothing matches; the else branch or an error |

| Error | Raised when |
| --- | --- |
| `Formatter named 'choose' requires at least 2 format options.` | the format has fewer than 2 parts |
| `You must specify at least N choices` | fewer format parts than options |
| `You cannot specify more than N choices` | more than one part beyond the options |
| `"x" is not a valid choice, and a "default" choice was not supplied` | no option matched and there is no else branch |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let two = Value::List(vec![Value::Int(2)]);
assert_eq!(smart.format("{0:choose(1|2|3):one|two|three}", &two).unwrap(), "two");
assert_eq!(smart.format("{0:choose(1|2|3):one|two|three|many}", &two).unwrap(), "two");

let other = Value::List(vec![Value::Int(9)]);
assert_eq!(smart.format("{0:choose(1|2|3):one|two|three|many}", &other).unwrap(), "many");
assert!(smart.format("{0:choose(1|2|3):one|two|three}", &other).is_err());
```

## `substr`

| | |
| --- | --- |
| Name | `substr` (`SubStringFormatter`) |
| Auto-detects | no |
| Registered by default | yes |
| Options | `start` or `start,length` |
| Split character | `,` for the options |
| Value types | `Value::String`, `Value::Null` |

Both numbers are `i32`, counted in UTF-16 code units. A negative `start` counts
from the end of the string; a negative `length` cuts that many characters off
the end. With only `start`, everything from there on is taken.

A null value never parses the options and writes
`null_display_string` (empty by default, `set_null_display_string`).

A format may follow, and it must contain a nested placeholder; the substring is
pushed as its value.

| `SubStringOutOfRangeBehavior` | Effect when `start + length` runs past the end |
| --- | --- |
| `ReturnEmptyString` | empty output (the default) |
| `ReturnStartIndexToEndOfString` | the rest of the string from `start` |
| `ThrowException` | the error below |

| Error | Raised when |
| --- | --- |
| `Formatter named 'substr' requires at least 1 formatter option and a string? argument.` | the value is neither string nor null, or the options are empty |
| `Specified argument was out of the range of valid values.` | the range is invalid, or is past the end under `ThrowException` |
| `Value was either too large or too small for an Int32.` | an option overflows `i32` |
| `The input string '<text>' was not in a correct format.` | an option is not an integer |
| `The format requires a nested placeholder` | the format is plain text |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::from("Long John")]);
assert_eq!(smart.format("{0:substr(5)}", &args).unwrap(), "John");
assert_eq!(smart.format("{0:substr(-4,2)}", &args).unwrap(), "Jo");
assert_eq!(smart.format("{0:substr(0,4):[{}]}", &args).unwrap(), "[Long]");
```

## `time`

| | |
| --- | --- |
| Name | `time` (`TimeFormatter`) |
| Auto-detects | no |
| Registered by default | no; `formatters_mut().add(Box::new(TimeFormatter::new()))` |
| Options | a culture name, or the v2-style format when the format is empty |
| Split character | none; the format is a word list |
| Value types | `Value::TimeSpan`, `Value::DateTime` (the span between it and now) |
| Feature | `time` |

The format is a list of keywords separated by anything that is not a word
character. Keywords fall into four groups, and a group that is not named is
inherited from `default_format_options`.

| Group | Keywords | Default |
| --- | --- | --- |
| range | `w`/`week`/`weeks`, `d`/`day`/`days`, `h`/`hour`/`hours`, `m`/`minute`/`minutes`, `s`/`second`/`seconds`, `ms`/`millisecond`/`milliseconds` | seconds through days |
| truncation | `short`, `auto`, `fill`, `full` | `auto` |
| abbreviation | `abbr`, `noabbr` | `noabbr` |
| "less than" | `less`, `noless` | `less` |

Naming any range keyword replaces the whole default range; the lowest and
highest named units are the bounds. Keywords must be whole words: `1hours` and
`shorter` name nothing. Unknown words are ignored.

| Truncation | Writes |
| --- | --- |
| `short` | the largest non-zero unit in range, alone |
| `auto` | every non-zero unit in range |
| `fill` | the largest non-zero unit and every smaller one in range |
| `full` | every unit in range |

The language is the culture named in the options, else the culture of the
format call, else `fallback_language` (English). Only `en`, `de`, `es`, `fr`,
`it` and `pt` ship unit words; another culture falls back. A culture name in
the options is only read as a language when the format is non-empty:
`{0:time(de):}` has an empty format, so `de` is read as v2-style format
options and the language comes from the call.

If the format contains a nested placeholder, the first item is dropped and the
remaining format is rendered against the list of time parts, which is how the
`list` formatter is chained onto it.

| Error | Raised when |
| --- | --- |
| `'TimeFormatter' can only process types of TimeSpan, DateTime, DateTimeOffset, TimeOnly, but not '<type>'` | an unsupported value type |
| `Culture is not supported. (Parameter 'name')` + `<name> is an invalid culture identifier.` | the options name is not a well-formed culture name |
| `TimeTextInfo could not be found for the given culture argument '…'.` | no language matched and `fallback_language` is empty |

```rust
use jiff::SignedDuration;
use smartformat::extensions::time::TimeFormatter;
use smartformat::{SmartFormatter, Value};

let mut smart = SmartFormatter::default();
smart.formatters_mut().add(Box::new(TimeFormatter::new()));

let args = Value::List(vec![Value::TimeSpan(SignedDuration::from_secs(90_061))]);
assert_eq!(smart.format("{0:time:}", &args).unwrap(), "1 day 1 hour 1 minute 1 second");
assert_eq!(smart.format("{0:time:hours}", &args).unwrap(), "25 hours");
assert_eq!(smart.format("{0:time:abbr}", &args).unwrap(), "1d 1h 1m 1s");
assert_eq!(smart.format("{0:time:short}", &args).unwrap(), "1 day");
assert_eq!(smart.format("{0:time(de):hours}", &args).unwrap(), "25 Stunden");
```

## `L`

| | |
| --- | --- |
| Name | `L` (`LocalizationFormatter`) |
| Auto-detects | no, and it cannot be turned on |
| Registered by default | no; `SmartFormatter::register_localization(provider)` |
| Options | a culture name |
| Split character | none |
| Value types | any; the value becomes the scope of the translation |

The format is the lookup key. What the provider returns is parsed as a
template and rendered against the current value, so a translation may carry
placeholders of its own. When the raw key misses and the format has nested
placeholders, the format is rendered and the result is looked up instead.

A culture named in the options switches the culture for the **rest of the
format call**, not just for the translation. That is .NET's behaviour, kept on
purpose.

| Error | Raised when |
| --- | --- |
| `'Format' for localization must not be null or empty. (Parameter 'formattingInfo')` | `{:L:}` or `{:L()}` |
| `unknown culture "xx": no data is shipped for it` | the options name a culture with no data |
| `No localized string found for '<key>'` | the provider has no entry |

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [
    ("", "Hello", "Hello"),
    ("de", "Hello", "Hallo"),
]
.into_iter()
.collect();

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let none = Value::Null;
assert_eq!(smart.format("{:L:Hello}", &none).unwrap(), "Hello");
assert_eq!(smart.format("{:L(de):Hello}", &none).unwrap(), "Hallo");
```

## `t`

| | |
| --- | --- |
| Name | `t` (`TemplateFormatter`) |
| Auto-detects | no, and it cannot be turned on |
| Registered by default | no; `SmartFormatter::register_template(name, template)` |
| Options | the template name |
| Split character | none |
| Value types | any; the value becomes the scope of the template |

The template name comes from the options when they are non-empty, otherwise
from the format: `{:t(firstLast)}` and `{:t:firstLast}` are the same lookup. A
format that contains a nested placeholder makes the formatter decline.

Names are matched with the host formatter's `case_sensitive` setting as it
stood when the first template was registered. Registering a name twice fails.

| Error | Raised when |
| --- | --- |
| `Formatter named 't' found no registered template named '<name>'` | no template under that name |

```rust
use smartformat::{SmartFormatter, Value};

let mut smart = SmartFormatter::default();
smart.register_template("firstLast", "{First} {Last}").unwrap();

let person = Value::Map(
    [
        ("First".to_owned(), Value::from("Scott")),
        ("Last".to_owned(), Value::from("Rippey")),
    ]
    .into_iter()
    .collect(),
);
assert_eq!(smart.format("{:t:firstLast}", &person).unwrap(), "Scott Rippey");
assert_eq!(smart.format("{:t(firstLast)}", &person).unwrap(), "Scott Rippey");
```

## `d`, the default formatter

| | |
| --- | --- |
| Name | `d` (`DefaultFormatter`) |
| Auto-detects | yes, and it is last, so it is the fallback |
| Registered by default | yes, last |
| Options | none |
| Split character | none |
| Value types | every type but `List` and `Map` |

If the format holds a nested placeholder, that format is rendered against the
value. Otherwise the raw format text is used as a .NET standard format
specifier; see [format-specifiers.md](format-specifiers.md).

| Value | Rendered as |
| --- | --- |
| `Null` | empty string |
| `Bool` | `True` / `False`, specifier ignored |
| `String` | itself, specifier ignored |
| `Int`, `UInt`, `Float` | the numeric specifiers |
| `DateTime` | the date/time specifiers |
| `TimeSpan` | the `TimeSpan` specifiers |
| `List` | error |
| `Map` | error |

| Error | Raised when |
| --- | --- |
| `Default formatting of a list is not supported; use a formatter such as "list"` | the value is a list |
| `Default formatting of a map is not supported; select a value from it` | the value is a map |
| `Format specifier was invalid.` | a numeric specifier .NET rejects |
| `Input string was not in a correct format.` | a date/time or duration specifier .NET rejects |
| `unsupported format spec: <spec>` | valid .NET, outside the supported subset (custom patterns) |

The list, map and `unsupported format spec` rows are deliberate divergences.
.NET renders a list or a map as a CLR type name and a custom pattern as text;
this crate fails loudly instead, so a compatibility gap cannot pass unnoticed.
See [DESIGN.md](../../DESIGN.md).

## Related

- [template-syntax.md](template-syntax.md): where each part of a placeholder comes from.
- [format-specifiers.md](format-specifiers.md): what the default formatter accepts.
- [settings-and-features.md](settings-and-features.md): the features that gate `plural`, `time` and `ismatch`.
- [how-to/extend-with-your-own.md](../how-to/extend-with-your-own.md): writing a formatter or source of your own.
- [explanation/architecture.md](../explanation/architecture.md): why the registry is ordered and why that order is observable.
- [DESIGN.md](../../DESIGN.md): per-formatter divergences and reproduced quirks.
