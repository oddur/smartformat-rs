# Choose what happens when a template or a value is wrong

Pick the recovery behavior for a broken template, a missing selector, or a value a formatter cannot handle.

There are two settings and one enum. `SmartSettings::parse_error_action` decides what happens to a template the parser cannot read; `SmartSettings::format_error_action` decides what happens to a placeholder that parsed fine but could not be rendered. Both take an `ErrorAction`, and .NET keeps them apart the same way.

```rust
use smartformat::{ErrorAction, SmartFormatter, SmartSettings};

let smart = SmartFormatter::new(SmartSettings {
    parse_error_action: ErrorAction::Error,
    format_error_action: ErrorAction::Ignore,
    ..SmartSettings::default()
});
assert_eq!(smart.settings().format_error_action, ErrorAction::Ignore);
```

The default for both is `Error`, which is .NET's `ThrowError`.

## The four actions

| `ErrorAction` | .NET | At parse time | At format time |
| --- | --- | --- | --- |
| `Error` | `ThrowError` | `parse` and `format` return `Err(Error::Parse)` | the call returns `Err(Error::Format)` |
| `OutputErrorInResult` | `OutputErrorInResult` | the whole result is the parser's report | the message is written where the placeholder was |
| `Ignore` | `Ignore` | the offending tokens are dropped | the placeholder contributes nothing |
| `MaintainTokens` | `MaintainTokens` | the offending tokens stay as literal text | the placeholder is written back out |

## One broken template, all four

`Hi {Name}, you owe {Amount}.` rendered against a map that has `Name` and no `Amount`. This is a format-time error: the template parses, the selector `Amount` finds nothing.

```rust
use smartformat::{ErrorAction, SmartFormatter, SmartSettings, Value};

fn render(action: ErrorAction) -> Result<String, smartformat::Error> {
    let smart = SmartFormatter::new(SmartSettings {
        format_error_action: action,
        ..SmartSettings::default()
    });
    let args = Value::Map(
        [("Name".to_owned(), Value::from("Joe"))].into_iter().collect(),
    );
    smart.format("Hi {Name}, you owe {Amount}.", &args)
}

// Error: the call fails, and nothing is rendered.
assert!(render(ErrorAction::Error).is_err());

// OutputErrorInResult: the full FormattingException message, in the output.
assert_eq!(
    render(ErrorAction::OutputErrorInResult).unwrap(),
    concat!(
        "Hi Joe, you owe ",
        "Error parsing format string: No source extension could handle the ",
        "selector named \"Amount\" at 20\n",
        "Hi {Name}, you owe {Amount}.\n",
        "--------------------^",
        ".",
    ),
);

// Ignore: the placeholder disappears, punctuation and all else intact.
assert_eq!(render(ErrorAction::Ignore).unwrap(), "Hi Joe, you owe .");

// MaintainTokens: the placeholder is written back, rebuilt from its parts.
assert_eq!(
    render(ErrorAction::MaintainTokens).unwrap(),
    "Hi Joe, you owe {Amount}.",
);
```

Note what `Ignore` leaves behind. `Hi Joe, you owe .` reads as a sentence, so nothing downstream flags it and the missing amount reaches whoever the text was for.

## The same four at parse time

`Hi {Name, bye` never closes its brace, and the space inside the selector is not a legal selector character. The parser finds two issues.

```rust
use smartformat::{ErrorAction, SmartFormatter, SmartSettings, Value};

fn render(action: ErrorAction) -> Result<String, smartformat::Error> {
    let smart = SmartFormatter::new(SmartSettings {
        parse_error_action: action,
        ..SmartSettings::default()
    });
    smart.format("Hi {Name, bye", &Value::Null)
}

assert!(render(ErrorAction::Error).is_err());

assert_eq!(
    render(ErrorAction::OutputErrorInResult).unwrap(),
    concat!(
        "The format string has 2 issues:\n",
        "'0x20': Invalid character in the selector, ",
        "Format string is missing a closing brace\n",
        "In: \"Hi {Name, bye\"\n",
        "At:  ---------^---^ ",
    ),
);

// Ignore drops the whole erroneous placeholder, including the text after it.
assert_eq!(render(ErrorAction::Ignore).unwrap(), "Hi ");

// MaintainTokens keeps the tokens as literal text.
assert_eq!(render(ErrorAction::MaintainTokens).unwrap(), "Hi {Name, bye");
```

`OutputErrorInResult` at parse time replaces the *whole* format with the report, because the parser has no valid tree to render. At format time it replaces one placeholder. That asymmetry is .NET's.

## Read the error

`Error::Parse` carries .NET's whole report plus one `ParseError` per issue, each with a message and a position in UTF-16 code units.

```rust
use smartformat::{Error, SmartFormatter, Value};

let smart = SmartFormatter::default();
match smart.format("Hi {Name, bye", &Value::Null) {
    Err(Error::Parse { errors, .. }) => {
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].message, "'0x20': Invalid character in the selector");
        assert_eq!(errors[0].position, 9);
    }
    other => panic!("expected a parse error, got {other:?}"),
}
```

`Error::Format` carries one message and one position. The message is either a bare sentence or a **`FormattingException` envelope**, and which one you get is not arbitrary: .NET wraps its own `FormattingException`s in the envelope and lets every other exception through bare. The envelope is four parts on three lines:

```text
Error parsing format string: <issue> at <index>
<the whole template>
-------------------^
```

The issue, the index it is reported at, the template, and a caret line. `OutputErrorInResult` writes exactly that string into your output, so it has to match .NET byte for byte. A bare message looks like `Specified argument was out of the range of valid values. (Parameter 'start')`: that is a .NET `ArgumentOutOfRangeException`, and it carries no envelope because .NET's evaluator only adds one while rethrowing.

Two more variants exist for cases .NET does not have:

- `Error::UnsupportedSpec` is a specifier that is valid .NET but outside the supported subset, such as a custom numeric pattern. It is kept separate so a compatibility gap is loud.
- `Error::UnknownCulture` is a culture name the shipped table does not hold. Only `format_with_culture_name` and its parsed twin raise it.

`Error::UnsupportedSpec` is recovered from at format time like any other error, so under `Ignore` an unsupported specifier renders as nothing. `Error::UnknownCulture` is raised before any rendering starts and no error action touches it.

## When to use which

**Fail fast in tests and at startup: `Error` for both.** A template with a typo should break a build, not a page. This is the default, so a test formatter needs no settings at all.

**Degrade in production text: `Ignore` or `MaintainTokens` for `format_error_action`, `Error` for `parse_error_action`.** Templates come from your repository and are checked before deploy; values come from live data and are not. Splitting the two settings gives you a formatter that refuses a broken template and survives a null.

Choose between the two format-time options by who reads the output. `Ignore` is right when a missing value should read as absent: an optional line in an email. `MaintainTokens` is right when someone who can fix it will see the result: an admin console, a log line, a CMS preview. It leaves `{Amount}` on the page, which is ugly and diagnosable.

**Author with `MaintainTokens` for both.** While you are writing templates it shows you which placeholders resolved and which did not, in place, without stopping at the first failure. Turn it off before you ship: it will happily render `{Password}` to a customer.

**Use `OutputErrorInResult` to build a compatibility report, not to serve text.** It is the only mode that puts the error *text* in the result, which makes it the mode for a bulk run over a corpus: render everything, grep the output for `Error parsing format string:`, and you have the list of templates to fix. It is also how the golden harness pins error messages against .NET. Keep it out of anything a user sees, because the envelope prints the whole template and a caret line into the middle of the text.

## What no error action can rescue

Three failures bypass the setting and fail the call whatever it is set to, because .NET raises them outside its evaluator's error handling:

- an escape sequence in a top-level literal that resolves to nothing (`Error::Escape`);
- formatter options that hold such a sequence, which are read while the error report is built;
- a culture name passed to `format_with_culture_name` that the table does not hold.

For the reasoning behind the split between parse-time and format-time recovery, and why the same enum means different things on each side, see [DESIGN.md](../../DESIGN.md).
