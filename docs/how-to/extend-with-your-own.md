# Write your own formatter or source

Add behavior the built-in extensions do not cover, using the same two traits SmartFormat.NET has.

A `Formatter` turns a value into text: it serves the `name(options)` and `format` parts of `{value:name(options):format}`. A `Source` resolves one selector: it serves the `Customer` and the `Name` of `{Customer.Name}`. Everything built in goes through these traits, so the modules under `extensions/` and `sources/` are working reference implementations for whatever this guide leaves out, and `cargo doc -p smartformat --all-features --open` has the full signatures.

For why the extension model is a registry of ranked, declinable extensions rather than a map of names, see [How a render happens](../explanation/architecture.md), sections "The two registries" and "Where the extension points are". It explains the ordering rationale behind the `add`/`insert`/`push` choice this guide states as a rule.

Before you write one, check whether a built-in already covers you. [Formatters](../reference/formatters.md) lists all ten with their options, and three of them take a hook of your own: `register_template` for reusable named templates, `register_localization` with your own `LocalizationProvider` for translated strings ([Serve translated text](localize-text.md)), and `register_variables` / `register_global_variables` for values a template can name without them being passed as arguments.

## A formatter

Implement `name` and `try_evaluate_format`. Return `Ok(true)` when you handled the value, `Ok(false)` to decline so the next formatter gets a turn, `Err` to fail the placeholder.

```rust
use smartformat::error::Error;
use smartformat::formatter::{Formatter, FormattingInfo};
use smartformat::{SmartFormatter, Value};

struct ShoutFormatter;

impl Formatter for ShoutFormatter {
    fn name(&self) -> &str {
        "shout"
    }

    /// Only when the placeholder names it. A formatter that guesses at an
    /// unnamed placeholder has to be cheap and certain; this one is neither.
    fn can_auto_detect(&self) -> bool {
        false
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let text = match info.current() {
            Value::String(text) => text.to_uppercase(),
            // Decline when auto-detecting, error when named explicitly.
            _ => {
                return info.decline_or_error(|name| {
                    format!("Formatter named '{name}' can only format strings")
                })
            }
        };
        info.write(&text);
        Ok(true)
    }
}

let mut smart = SmartFormatter::default();
// Before DefaultFormatter, which is always last and handles everything.
let last = smart.formatters().len() - 1;
smart.formatters_mut().insert(last, Box::new(ShoutFormatter));

let args = Value::List(vec![Value::from("hello")]);
assert_eq!(smart.format("{0:shout:}", &args).unwrap(), "HELLO");
```

Note the trailing colon in `{0:shout:}`. A formatter name is only a formatter name when a second colon or a `(` follows it; `{0:shout}` is a placeholder whose *format specifier* is the word `shout`, which `DefaultFormatter` hands to the value and a string quietly ignores, so the placeholder renders unchanged and nothing reports an error.

`can_auto_detect` defaults to `true`, so a formatter that says nothing is offered every placeholder that names no formatter. Say `false` unless you mean it.

Registration position matters only for auto-detection: a named placeholder finds its formatter by name wherever it sits. `FormatterRegistry::add` slots a formatter at the rank .NET's `WellKnownExtensionTypes` table gives its name, and appends one that table does not hold, which puts it after `DefaultFormatter` where auto-detection never reaches it. .NET has the same trap. Use `insert` when the position matters.

### What `FormattingInfo` gives you

| Method | What it is |
| --- | --- |
| `current()` | the resolved value, the thing to format |
| `format()` | the part after the colon, as a parsed `Format`: both `D3` in `{0:D3}` and the nested `{Name}` in `{0:{Name}}` |
| `formatter_options()` | the text in parens, escape sequences resolved; `formatter_options_raw()` for the unresolved text |
| `alignment()` | the placeholder's alignment |
| `culture()` | the culture in force; `set_culture` changes it for the rest of the call |
| `settings()` | the whole `SmartSettings` |
| `write(text)` | append, applying the placeholder's alignment |
| `write_unaligned(text)` | append without it, which is what `list` writes its spacers with |
| `format_as_child(format, value)` | render a nested format against a value into the same output |
| `format_as_child_of_current(format, value, alignment)` | the same for a value that is not this placeholder's, an item of a list say |
| `format_to_isolated_string(format, value)` | render into a fresh string with no positional arguments and a reset scope chain |
| `root_value()` | the argument the call was made with |
| `collection_index()` | the index of the list item being formatted, or `-1` |

### Split a format into parts

`choose`, `cond`, `plural` and `list` all read a `|`-separated format. `Format::split(separator)` cuts one at the top nesting level, so a nested placeholder is never cut in half: `a|{0:b|c}|d` is three parts, not four. `Format::split_max(separator, n)` stops after `n` separators, which is what `list` does with `n = 4`.

A format that holds no separator splits into itself, so the result is never empty and `split(…).len() - 1` is the number of separators found. Each piece keeps the byte range and the source text it covers, so you can render it and report on it exactly like the format it came from.

Both return `Result<Vec<SplitPiece>, SplitError>`, and a `SplitPiece` is itself a `Result`. That is not defensiveness: .NET throws at either of two moments, once out of the whole split and once per piece as it is cut, and the two halves of the type are those two moments. Only a template with a crossed escape sequence reaches either.

```rust
use smartformat::error::Error;
use smartformat::formatter::{Formatter, FormattingInfo};
use smartformat::{SmartFormatter, Value};

/// `{Value:pick:a|b|c}` writes the part whose index the value holds.
struct PickFormatter;

impl Formatter for PickFormatter {
    fn name(&self) -> &str {
        "pick"
    }

    fn can_auto_detect(&self) -> bool {
        false
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let Some(format) = info.format() else {
            return Err(info.formatting_error_here("pick requires a format"));
        };
        let Value::Int(index) = info.current() else {
            return info.decline_or_error(|name| {
                format!("Formatter named '{name}' can only format integers")
            });
        };

        let parts = format
            .split('|')
            .map_err(|error| info.plain_error_here(&error.to_string()))?;
        let part = usize::try_from(*index)
            .ok()
            .and_then(|index| parts.get(index))
            .ok_or_else(|| info.formatting_error_here("pick index out of range"))?;
        let part = part
            .as_ref()
            .map_err(|error| info.plain_error_here(&error.to_string()))?;

        let value = info.current();
        info.format_as_child(part, value)?;
        Ok(true)
    }
}

let mut smart = SmartFormatter::default();
let last = smart.formatters().len() - 1;
smart.formatters_mut().insert(last, Box::new(PickFormatter));

let args = Value::List(vec![Value::Int(1)]);
assert_eq!(smart.format("{0:pick:zero|one|two}", &args).unwrap(), "one");
assert!(smart.format("{0:pick:zero}", &args).is_err());
```

### Raise the error .NET would raise

Which error constructor you pick is visible in the output, under `ErrorAction::OutputErrorInResult` and in the message of a returned `Err`.

| Method | Message shape | Use it where .NET throws |
| --- | --- | --- |
| `formatting_error_here(issue)` | the full `FormattingException` envelope: the issue, the index, the template, a caret line | a `FormattingException` |
| `plain_error_here(issue)` | the bare sentence, no envelope | anything else: `FormatException`, `ArgumentException`, `OverflowException` |
| `formatting_error(issue, byte_offset)` | as above, at a position you choose | a `FormattingException` you can place precisely |
| `plain_error(issue, byte_offset)` | bare, at a position you choose | as above |
| `decline_or_error(|name| issue)` | `Ok(false)` when unnamed, a plain error when named | every "can I handle this value?" check |

The difference is not cosmetic. .NET's evaluator adds the envelope only while rethrowing an exception that is not already a `FormattingException`, so an `ArgumentOutOfRangeException` from a formatter reaches `OutputErrorInResult` bare. Copy whichever .NET does, and the goldens will agree with you. See [Choose what happens when something is wrong](choose-error-behavior.md) for what each action does with the message.

`decline_or_error` is the pattern every built-in ends its type check with: `Ok(false)` if the placeholder named no formatter, so the next extension gets a turn, and the error your closure builds if it named this one. The closure is handed the name the placeholder used, which every one of those .NET messages quotes.

### Be findable after registration

A formatter with knobs should implement `as_any_mut`, so a caller can reach the registered instance instead of building a second one.

```rust
use smartformat::error::Error;
use smartformat::formatter::{Formatter, FormattingInfo};
use smartformat::{SmartFormatter, Value};

struct RepeatFormatter {
    times: usize,
}

impl Formatter for RepeatFormatter {
    fn name(&self) -> &str {
        "repeat"
    }

    fn can_auto_detect(&self) -> bool {
        false
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let Value::String(text) = info.current() else {
            return info.decline_or_error(|name| {
                format!("Formatter named '{name}' can only format strings")
            });
        };
        let text = text.repeat(self.times);
        info.write(&text);
        Ok(true)
    }
}

let mut smart = SmartFormatter::default();
let last = smart.formatters().len() - 1;
smart.formatters_mut().insert(last, Box::new(RepeatFormatter { times: 2 }));

smart
    .formatters_mut()
    .get_mut::<RepeatFormatter>()
    .expect("just registered")
    .times = 3;

let args = Value::List(vec![Value::from("ab")]);
assert_eq!(smart.format("{0:repeat:}", &args).unwrap(), "ababab");
```

`get_mut` is the port's `GetFormatterExtension<T>()`. It skips any formatter whose `as_any_mut` returns `None`, which is the default, and a formatter that declines is also invisible to `register_localization` and `register_template`: those look for the extension they own, and would register a second one that name lookup could never reach.

## A source

Implement one method. Return `None` to decline so the next source tries; return `Some(Cow::Owned(Value::Null))` for a selector you handled that legitimately has no value.

```rust
use std::borrow::Cow;
use std::collections::BTreeMap;

use smartformat::sources::{SelectorInfo, Source};
use smartformat::{SmartFormatter, Value};

/// Answers `{cfg_<key>}` out of a table the source owns.
struct ConfigSource {
    entries: BTreeMap<String, Value>,
}

impl Source for ConfigSource {
    fn try_evaluate_selector<'a>(&'a self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        let key = info.text().strip_prefix("cfg_")?;
        // Borrowed out of the source's own storage: no allocation per placeholder.
        self.entries.get(key).map(Cow::Borrowed)
    }
}

let mut smart = SmartFormatter::default();
smart.sources_mut().insert(
    0,
    Box::new(ConfigSource {
        entries: [("region".to_owned(), Value::from("eu-west-1"))]
            .into_iter()
            .collect(),
    }),
);

assert_eq!(smart.format("{cfg_region}", &Value::Null).unwrap(), "eu-west-1");
```

The `&'a self` in the signature is what lets you hand out `Cow::Borrowed` into the source's own storage: a registered source is owned by the `SmartFormatter` doing the formatting and outlives the call. An implementation written `fn try_evaluate_selector<'a>(&self, …)` still satisfies the trait, since it promises more than is asked.

### What `SelectorInfo` gives you

| Field or method | What it is |
| --- | --- |
| `current` | the value the selector is evaluated against |
| `text()` | the selector text without its operator |
| `operator()` | the operator before it: `""`, `"."`, `"?."`, `"["`, … |
| `index()` | its position in the placeholder, from 0 |
| `selector_is(name)` | compares a name to the text, honoring `case_sensitive` |
| `args` | the positional arguments of the call |
| `settings` | the whole `SmartSettings` |
| `collection_index` | the index of the list item being formatted, or `-1` |
| `nullable_result()` | `Some(null)` when the chain is null-conditional and the current value is null, which short-circuits `{City?.Length}` |

### Position in the registry

`SourceRegistry::add` slots a source at the rank .NET's `WellKnownExtensionTypes.Sources` gives it. A source of your own leaves `well_known_rank` at its default `None` and is therefore appended, exactly as in .NET, which puts it after `DefaultSource` and lets it answer only what nothing else did. `insert(0, …)` puts it first, ahead of every built-in, which is what the example above does so that `cfg_region` cannot be shadowed.

Choose deliberately. A source registered first sees every selector in the template, including the ones a map argument would have answered.

## Both traits are `Send + Sync`

One registered extension serves every thread that formats with the `SmartFormatter` that owns it. Keep mutable state behind a lock, or out of the extension entirely: the ambient state a format call carries (the collection index, the culture) lives on the call and reaches you through `FormattingInfo`, which is how two threads formatting at once cannot disturb each other. .NET holds both in statics and has exactly those two hazards.
