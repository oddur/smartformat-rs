# How to run your existing .NET SmartFormat templates from Rust

This is the guide for the crate's main use case: you have templates written for
SmartFormat.NET, and you want them to render identically from Rust. It walks
the setup decisions in the order they come up. For what the syntax means, see
the README's syntax table and the [rustdoc](https://docs.rs/smartformat); for
why the port behaves as it does, see
[DESIGN.md](https://github.com/oddur/smartformat-rs/blob/main/DESIGN.md).

## 1. Mirror your .NET configuration

`SmartFormatter::default()` matches `Smart.CreateDefaultSmartFormat()` with
default `SmartSettings`. If your .NET code changes settings, mirror them:

| .NET | Here |
|---|---|
| `Formatter.ErrorAction` | `SmartSettings::format_error_action` |
| `Parser.ErrorAction` | `SmartSettings::parse_error_action` |
| `CaseSensitivity` | `SmartSettings::case_sensitive` |
| `StringFormatCompatibility` | `SmartSettings::string_format_compatibility` |
| `Formatter.AlignmentFillCharacter` | `SmartSettings::alignment_fill_character` |
| `SystemTime.SetDateTimeNow(...)` | `SmartSettings::now` (needs the `time` feature) |

```rust
use smartformat::{ErrorAction, SmartFormatter, SmartSettings};

let smart = SmartFormatter::new(SmartSettings {
    format_error_action: ErrorAction::Ignore,
    ..SmartSettings::default()
});
```

## 2. Register what .NET registers explicitly

`time`, `L` (localization), `t` (template), and the variable sources are not in
the default registry, exactly as they are not in `CreateDefaultSmartFormat`.
If your .NET code adds them, add them here:

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, TimeFormatter};

let mut smart = SmartFormatter::default();
smart.formatters_mut().add(Box::new(TimeFormatter::new()));
smart.register_template("firstLast", "{First} {Last}").unwrap();

let mut provider = HashMapLocalizationProvider::new();
provider.insert("fr", "Hello", "Bonjour");
smart.register_localization(Box::new(provider));
```

Registration order does not need care: `add` slots each formatter where .NET's
`WellKnownExtensionTypes` table puts it.

## 3. Feed values without reflection

.NET reflects over your objects; here a template renders against a
`Value` tree. Derive the conversion on your types and keep the field names
spelled the way your templates spell them:

```rust
#[derive(smartformat::ToSmartValue)]
#[allow(non_snake_case)]
struct Customer {
    Name: String,
    Orders: Vec<Order>,
}
```

Positional templates (`{0}`, `{1}`) take a `Value::List` of the arguments.
Dictionaries become `Value::Map`. `DateTime` maps to `jiff::civil::DateTime`
and `TimeSpan` to `jiff::SignedDuration`, both exact.

## 4. Pick cultures by name

`format` renders with the invariant culture. For anything else, pass the same
name you would give `CultureInfo.GetCultureInfo`:

```rust
let out = smart.format_with_culture_name("{0:C}", &args, "de-DE").unwrap();
```

The crate ships data for 35 locales, read out of .NET itself. A name outside
that list is an error, never a guess. To add one, add its name to
`tools/culturegen` and regenerate; the tool's README has the steps.

## 5. Validate your corpus before trusting it

Two cheap checks catch almost everything:

- **Parse every template** with `smart.parse(template)`. A template using a
  custom numeric or date pattern (`{0:#,##0.00}`) parses fine but fails at
  format time by design; grep the DESIGN.md non-goals if you rely on those.
- **Render a sample against real .NET.** The in-repo golden harness
  (`tools/goldens`) is the pattern to copy: a small C# program that renders
  your actual templates with SmartFormat.NET and writes JSON your Rust tests
  replay. That turns "should match" into "byte-identical, tested".

When an output differs, check DESIGN.md's "Known divergences" first: every
known gap is documented there with the test that pins it, and most are edges
(UTF-16 surrogate halves, regex dialect corners) rather than everyday
formatting.
