# Run your existing .NET SmartFormat templates from Rust

Take a corpus of templates written for SmartFormat.NET and render it from Rust, byte for byte.

Work through the five steps in order. Each one is a place where a .NET setup makes a decision that has to be carried across; skip one and the corpus renders *nearly* right, which is the failure mode this guide exists to prevent.

Look up the grammar in [Template syntax](../reference/template-syntax.md) and every setting in [Settings and features](../reference/settings-and-features.md). Read [Why byte-identical output is the goal](../explanation/byte-compatibility.md) for the boundary a migration will hit, and [How compatibility is verified](../explanation/how-compatibility-is-verified.md) for what the goldens do and do not prove. The API itself is the rustdoc: `cargo doc -p smartformat --all-features --open`.

## 1. Mirror your .NET configuration

`SmartFormatter::default()` is `Smart.CreateDefaultSmartFormat()` with default `SmartSettings`. If your .NET code changes a setting, change the same one here.

| .NET | Here |
| --- | --- |
| `SmartSettings.Formatter.ErrorAction` | `SmartSettings::format_error_action` |
| `SmartSettings.Parser.ErrorAction` | `SmartSettings::parse_error_action` |
| `SmartSettings.CaseSensitivity` | `SmartSettings::case_sensitive` |
| `SmartSettings.StringFormatCompatibility` | `SmartSettings::string_format_compatibility` |
| `SmartSettings.Formatter.AlignmentFillCharacter` | `SmartSettings::alignment_fill_character` |
| `SystemTime.SetDateTime(...)` | `SmartSettings::now` (needs the `time` feature) |

```rust
use smartformat::{CaseSensitivity, ErrorAction, SmartFormatter, SmartSettings, Value};

let smart = SmartFormatter::new(SmartSettings {
    format_error_action: ErrorAction::Ignore,
    case_sensitive: CaseSensitivity::CaseInsensitive,
    ..SmartSettings::default()
});

let args = Value::Map(
    [("Name".to_owned(), Value::from("Joe"))].into_iter().collect(),
);
// Case-insensitive selectors, and a missing one contributes nothing.
assert_eq!(smart.format("{NAME}{Missing}", &args).unwrap(), "Joe");
```

Three more belong to the parser rather than to the formatter, and reach it through `SmartFormatter::with_parser_settings`.

| .NET | Here |
| --- | --- |
| `ParserSettings.ConvertCharacterStringLiterals` | `ParserSettings::convert_character_string_literals` |
| `ParserSettings.AddCustomSelectorChars(...)` | `ParserSettings::add_custom_selector_chars` |
| `ParserSettings.AddCustomOperatorChars(...)` | `ParserSettings::add_custom_operator_chars` |

```rust
use smartformat::parsing::ParserSettings;
use smartformat::{SmartFormatter, SmartSettings, Value};

let mut parser_settings = ParserSettings::default();
parser_settings.add_custom_selector_chars(['$']).unwrap();

let smart = SmartFormatter::with_parser_settings(SmartSettings::default(), parser_settings);
let args = Value::Map(
    [("us$".to_owned(), Value::from(12i64))].into_iter().collect(),
);
assert_eq!(smart.format("{us$}", &args).unwrap(), "12");
```

`with_parser_settings` copies `error_action` and `string_format_compatibility` from the parser settings back over the `SmartSettings` you passed, so the two views can never disagree. Set those two on whichever struct you find clearer, not on both.

`SmartSettings::now` is the port's `SystemTime.Now()`. Leave it `None` and each placeholder that needs a clock reads the system's local time, as .NET does; set it and every render in the process sees the same instant, which is what makes a snapshot test deterministic.

## 2. Register what .NET registers explicitly

Five extensions are missing from the default registry because `CreateDefaultSmartFormat` leaves them out too. Each is useless until it is handed something, so .NET makes you ask, and so does this crate.

| Extension | Placeholder it serves | How to register |
| --- | --- | --- |
| `TimeFormatter` | `{0:time:…}` | `formatters_mut().add(Box::new(TimeFormatter::new()))` |
| `LocalizationFormatter` | `{:L:key}` | `register_localization(provider)` |
| `TemplateFormatter` | `{:t:name}` | `register_template(name, template)` |
| `PersistentVariablesSource` | `{group.name}` | `register_variables(source)` |
| `GlobalVariablesSource` | `{group.name}` | `register_global_variables(source)` |

```rust
use smartformat::sources::variables::{self, PersistentVariablesSource};
use smartformat::{HashMapLocalizationProvider, SmartFormatter, TimeFormatter, Value};

let mut smart = SmartFormatter::default();
smart.formatters_mut().add(Box::new(TimeFormatter::new()));
smart.register_template("firstLast", "{First} {Last}").unwrap();

let mut provider = HashMapLocalizationProvider::new();
provider.insert("fr", "Hello", "Bonjour");
smart.register_localization(Box::new(provider));

let mut vars = PersistentVariablesSource::new();
vars.add("app", variables::group([("name", Value::from("Acme"))]));
smart.register_variables(vars);

let person = Value::Map(
    [
        ("First".to_owned(), Value::from("Scott")),
        ("Last".to_owned(), Value::from("Rippey")),
    ]
    .into_iter()
    .collect(),
);
assert_eq!(smart.format("{:t:firstLast}", &person).unwrap(), "Scott Rippey");
assert_eq!(smart.format("{:L(fr):Hello}", &person).unwrap(), "Bonjour");
assert_eq!(smart.format("{app.name}", &person).unwrap(), "Acme");
```

Registration order does not need care. `add` slots each extension where .NET's `WellKnownExtensionTypes` table puts it, so a `TemplateFormatter` lands after `isnull` and before `choose` whatever else is registered. An extension of your own is a different matter: see [Write your own formatter or source](extend-with-your-own.md).

`register_variables` takes ownership of the source and hands out no way back to it, so fill it before you register it. `register_global_variables` takes a handle you can keep and add to afterwards.

## 3. Feed values without reflection

.NET reflects over whatever object you pass. There is no reflection here, so a template renders against a `Value` tree and you build it. Derive the conversion and spell the fields the way your templates spell them.

```rust
use smartformat::{SmartFormatter, ToSmartValue};
use smartformat::value::ToSmartValue as _;

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Id: i64,
    Total: f64,
}

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Customer {
    Name: String,
    Orders: Vec<Order>,
}

let customer = Customer {
    Name: "Joe".to_owned(),
    Orders: vec![
        Order { Id: 1, Total: 9.5 },
        Order { Id: 2, Total: 20.25 },
    ],
};

let smart = SmartFormatter::default();
assert_eq!(
    smart
        .format(
            "{Name}: {Orders:list:#{Id} ({Total:C2})|, |, and }",
            &customer.to_smart_value(),
        )
        .unwrap(),
    "Joe: #1 (\u{a4}9.50), and #2 (\u{a4}20.25)",
);
```

The derive keeps field names verbatim, case included, which is what `ReflectionSource` exposes in .NET. `#[allow(non_snake_case)]` is the price of matching a corpus written against C# property names.

The rest of the mapping:

| .NET argument | Here |
| --- | --- |
| positional arguments (`Format(fmt, a, b)`) | `Value::List(vec![a, b])` |
| a single object | any other `Value`, usually a `Value::Map` |
| `IDictionary<string, object?>` | `Value::Map` |
| `DateTime` | `Value::DateTime(jiff::civil::DateTime)` |
| `TimeSpan` | `Value::TimeSpan(jiff::SignedDuration)` |
| `null` | `Value::Null` |
| `ulong` too large for `long` | `Value::UInt` |

`DateTime` and `TimeSpan` are exact: a .NET tick is 100 ns and jiff counts whole nanoseconds. `Value::UInt` exists so a `ulong` still renders correctly under `D` and `X`, which read the CLR type's own width.

## 4. Pick cultures by name

`format` renders with the invariant culture. For anything else, pass the name you would give `CultureInfo.GetCultureInfo`.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(1234.5)]);

assert_eq!(
    smart.format_with_culture_name("{0:N2}", &args, "de-DE").unwrap(),
    "1.234,50",
);
// A name outside the shipped table is an error, never a guess at a parent.
assert!(smart.format_with_culture_name("{0:N2}", &args, "de-XX").is_err());
```

The crate ships data for 35 cultures, read out of a real .NET runtime rather than mapped from CLDR, so a listed culture matches by construction. [Cultures](../reference/cultures.md) lists them. A name outside the list fails with `Error::UnknownCulture` instead of falling back to the parent, because .NET would resolve it against the whole CLDR tree and this crate only has the table. To ship one more, see [Add a culture the crate does not ship](add-a-culture.md).

If your corpus renders the same template many times, parse once: `smart.parse(template)` gives a `Format`, and `format_parsed_with_culture_name` renders it.

## 5. Validate the corpus before you trust it

Three checks, in increasing order of cost and confidence.

1. **Parse every template.** `smart.parse(template)` catches syntax the parser reads differently from .NET's, and it costs one pass over the corpus.
2. **Render every template against representative values.** This is what finds the specifiers outside the supported subset: a custom numeric or date pattern (`{0:#,##0.00}`, `{0:yyyy-MM-dd}`) parses fine and fails at format time with `Error::UnsupportedSpec`, deliberately, so a compatibility gap is loud rather than quietly wrong. [Custom patterns](../reference/format-specifiers.md#custom-patterns) states the rule.
3. **Render the corpus with real .NET and compare.** `tools/goldens` is the pattern: a small C# program renders the templates with SmartFormat.NET and writes JSON that a Rust test replays. That turns "should match" into "byte-identical, tested".

[Test your templates](test-your-templates.md) walks all three, with the code.

When an output does differ, read the "Known divergences" section of [DESIGN.md](../../DESIGN.md) before you file a bug. Every known gap is listed there with the test that pins it, and most are edges: UTF-16 surrogate halves, regex dialect corners, the `ar-SA` calendar. The everyday formatting is covered by 2,765 golden cases generated from SmartFormat.NET 3.6.1.
