# Test your templates

Build a safety net for your own template corpus: a parse check that runs at startup, a snapshot test that catches a changed render, and, if you have .NET available, goldens that prove byte-identical output.

The three checks stack. The first costs milliseconds and catches typos, the second costs a test file and catches regressions, the third costs a .NET SDK and catches the compatibility gaps only .NET itself can find.

## 1. Parse every template

Every template your program can render should be parsed before it renders anything. `parse` returns the same `Format` your renders will use, so a passing check costs nothing at run time either: keep the parsed formats and render those.

```rust
use std::collections::BTreeMap;

use smartformat::parsing::Format;
use smartformat::{Error, SmartFormatter};

/// Every template the program can render, keyed by the name the code asks for.
const TEMPLATES: &[(&str, &str)] = &[
    ("welcome", "Hi {Name}, you have {Count:plural:a message|{} messages}."),
    ("receipt", "{Items:list:{Name} ({Price:C2})|, |, and }"),
];

fn parse_all(smart: &SmartFormatter) -> Result<BTreeMap<&'static str, Format>, (&'static str, Error)> {
    TEMPLATES
        .iter()
        .map(|(name, template)| match smart.parse(template) {
            Ok(format) => Ok((*name, format)),
            Err(error) => Err((*name, error)),
        })
        .collect()
}

let smart = SmartFormatter::default();
let parsed = parse_all(&smart).expect("every template parses");
assert_eq!(parsed.len(), 2);
```

Call `parse_all` from a `#[test]` so a bad template fails CI, and from your startup path so a template loaded from disk or a database fails loudly on boot rather than on the request that needs it.

Leave `parse_error_action` at its default `Error` for this. Under any other action `parse` returns `Ok` with a recovered tree, and the check passes while the template stays broken. See [Choose what happens when something is wrong](choose-error-behavior.md).

## 2. Render every template against representative values

Parsing does not catch a format specifier outside the supported subset. `{0:#,##0.00}` and `{0:yyyy-MM-dd}` are valid .NET custom patterns, they parse fine, and they fail at format time with `Error::UnsupportedSpec` on purpose. Only a render finds them. [Format specifiers](../reference/format-specifiers.md) lists what the subset does cover, and [Custom patterns](../reference/format-specifiers.md#custom-patterns) states the rule.

```rust
use smartformat::{Error, SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(1234.5)]);

// A standard specifier renders.
assert_eq!(smart.format("{0:N2}", &args).unwrap(), "1,234.50");

// A custom numeric pattern is a documented non-goal, and says so.
match smart.format("{0:#,##0.00}", &args) {
    Err(Error::UnsupportedSpec { spec, .. }) => assert_eq!(spec, "#,##0.00"),
    other => panic!("expected an unsupported specifier, got {other:?}"),
}
```

Give each template one set of values that exercises every placeholder, and one that exercises the interesting edges: a null, an empty list, a zero, a negative number. The plural and conditional formatters branch on the value, so a single sample renders one branch out of three.

## 3. Snapshot the renders

Assert the exact output. A snapshot test is what turns "it still works" into "it still produces this", which is the only claim that survives a refactor of your value tree.

```rust
use smartformat::{SmartFormatter, Value};

/// Template, arguments, expected output. One row per branch worth pinning.
fn cases() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        (
            "{Count:plural:no messages|a message|{} messages}",
            Value::Map([("Count".to_owned(), Value::from(0i64))].into_iter().collect()),
            "no messages",
        ),
        (
            "{Count:plural:no messages|a message|{} messages}",
            Value::Map([("Count".to_owned(), Value::from(1i64))].into_iter().collect()),
            "a message",
        ),
        (
            "{Count:plural:no messages|a message|{} messages}",
            Value::Map([("Count".to_owned(), Value::from(7i64))].into_iter().collect()),
            "7 messages",
        ),
    ]
}

let smart = SmartFormatter::default();
for (template, args, expected) in cases() {
    assert_eq!(smart.format(template, &args).unwrap(), expected, "{template}");
}
```

Two rules keep a snapshot honest:

- **Name the culture.** `format` uses the invariant culture, whose currency symbol is the placeholder `¤` and whose group separator is a comma. If your product renders `de-DE`, snapshot `de-DE` with `format_with_culture_name`.
- **Pin the clock.** `TimeFormatter` on a `DateTime` and the date branch of `cond` both read a clock. Set `SmartSettings::now` and every render in the test sees the same instant.

```rust
use smartformat::{SmartFormatter, SmartSettings, Value};

let smart = SmartFormatter::new(SmartSettings {
    now: Some("2026-07-31T12:00:00".parse().unwrap()),
    ..SmartSettings::default()
});

let due = Value::List(vec![Value::DateTime("2026-08-01T12:00:00".parse().unwrap())]);
assert_eq!(smart.format("{0:cond:overdue|due today|upcoming}", &due).unwrap(), "upcoming");
```

## 4. Generate goldens from real .NET

If your claim is "these templates render exactly as they do in .NET", only .NET can settle it. `tools/goldens` in this repository is the pattern to copy, and [`tools/goldens/README.md`](../../tools/goldens/README.md) documents the JSON shape, the argument mapping and the pitfalls. Do not re-derive it; mirror it. [How compatibility is verified](../explanation/how-compatibility-is-verified.md) explains why the harness is built the way it is, and what a golden does and does not prove.

The shape of the thing you are building:

1. A C# console app that references the same SmartFormat.NET version your production code uses, renders a table of `(id, template, args, culture, settings)` cases, and writes the results to stdout as JSON. A case that throws records the exception type instead of a result.
2. A checked-in JSON file, regenerated by one command and committed with the change that caused the diff.
3. A Rust test that reads the JSON, rebuilds each case's formatter and arguments, and asserts the render matches byte for byte.

Four things the in-repo harness does that a new one usually forgets:

- **Pin the clock in .NET too.** `SystemTime.SetDateTime` fixes what `TimeFormatter` and `cond` read, and the instant goes into the JSON so the Rust side can put it in `SmartSettings::now`.
- **Treat build warnings as errors.** A warning on stdout corrupts the JSON.
- **Turn `InvariantGlobalization` off explicitly.** With it on, every culture silently resolves to the invariant one and the culture cases pass while proving nothing.
- **Fail on a culture the Rust table does not carry.** A missing culture must never look like a pass.

Regenerate the goldens and the culture table together, with the same SDK on the same machine. .NET's culture data is ICU-backed and drifts between ICU releases, so a mismatched pair produces failures that look like port bugs. [`tools/culturegen/README.md`](../../tools/culturegen/README.md) explains why, and [Add a culture](add-a-culture.md) covers the case where your corpus needs a culture the crate does not ship.

## 5. Keep a skip list, not a fudge

When a case does not match, decide which of the two it is and write it down.

- The port is wrong: file it.
- The port diverges on purpose: name the case in a skip list with the reason, the way `crates/smartformat/tests/goldens.rs` does. Its `SKIPPED` constant holds the case id and a sentence of justification, and a test asserts every named id still exists in the golden file, so a skip cannot rot into a lie.

The known divergences are listed in [DESIGN.md](../../DESIGN.md), each with the test that pins it. Check there before you write a skip of your own: the answer may already be documented.
