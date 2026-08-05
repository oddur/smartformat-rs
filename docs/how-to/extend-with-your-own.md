# How to write your own formatter or source

The extension surface is the same two traits SmartFormat.NET has: a
`Formatter` renders `{value:name(options):format}` placeholders, a `Source`
resolves selectors like `{Customer.Name}`. Everything built in goes through
these same traits, so the built-in modules under `extensions/` and `sources/`
are working reference implementations for anything this guide leaves out.

## A formatter

```rust
use smartformat::error::Error;
use smartformat::formatter::{Formatter, FormattingInfo};

struct ShoutFormatter;

impl Formatter for ShoutFormatter {
    fn name(&self) -> &str {
        "shout"
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let text = match info.current() {
            smartformat::Value::String(s) => s.to_uppercase(),
            // Decline when auto-detecting; error when named explicitly.
            _ => return info.decline_or_error(|name| {
                format!("Formatter named '{name}' can only format strings")
            }),
        };
        info.write(&text);
        Ok(true)
    }
}

let mut smart = smartformat::SmartFormatter::default();
smart.formatters_mut().add(Box::new(ShoutFormatter));
```

What `FormattingInfo` gives you:

- `current()` is the resolved value; `format()` the part after the colon;
  `formatter_options()` the text in parens.
- `write(text)` appends with the placeholder's alignment applied;
  `format_as_child(format, value)` renders a nested format against a value,
  which is how `choose`/`plural` render their chosen branch.
- `Format::split('|')` splits the format at the top nesting level, lazily,
  the way the .NET splitting formatters do.
- For errors, `plain_error_here(issue)` produces the bare message and
  `formatting_error_here(issue)` wraps it in .NET's `FormattingException`
  envelope. Pick the one matching what .NET throws in the same situation;
  the difference is visible under `ErrorAction::OutputErrorInResult`.

Return `Ok(false)` to decline so the next formatter can try; that plus
`can_auto_detect` (default `false`) is the whole auto-detection protocol.
A formatter that needs to be found again after registration (to set knobs)
overrides `as_any_mut`, and callers reach it with
`formatters_mut().get_mut::<YourFormatter>()`.

## A source

```rust
use std::borrow::Cow;
use smartformat::sources::{SelectorInfo, Source};
use smartformat::Value;

struct EnvSource;

impl Source for EnvSource {
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        let name = info.text().strip_prefix("env_")?;
        std::env::var(name).ok().map(|v| Cow::Owned(Value::String(v)))
    }
}
```

Return `None` to decline so the next source tries. Built-in sources carry the
rank .NET's `WellKnownExtensionTypes.Sources` gives them; a custom source
leaves `well_known_rank` alone and is appended, exactly as in .NET.

## Ready-made registration points

Before writing an extension, check whether a built-in already carries the
hook: `register_template(name, template)` for reusable named templates,
`register_localization(provider)` with your own `LocalizationProvider` for
translated strings, and `register_variables` / `register_global_variables`
for values templates can name without them being passed as arguments.
