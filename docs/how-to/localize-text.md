# Serve translated text

Render a template whose words come from a translation table, keyed by culture, with the placeholders inside the translation still working.

The formatter is `L`. `{:L:Hello}` looks `Hello` up and renders what comes back. It is not in the default registry, because a formatter with no translations has nothing to translate with.

The syntax and the option grammar are in the [reference](../reference/); this guide is the wiring.

## 1. Build a provider

`HashMapLocalizationProvider` is the built-in one: per-culture tables held in memory. Fill it from `(culture, key, value)` triples. The culture is a name, `""` for the invariant culture.

```rust
use smartformat::HashMapLocalizationProvider;

let provider: HashMapLocalizationProvider = [
    ("", "WeTranslateText", "We translate text"),
    ("es", "WeTranslateText", "Traducimos el texto"),
    ("de", "WeTranslateText", "Wir übersetzen Text"),
]
.into_iter()
.collect();

assert_eq!(provider.len(), 3);
```

`insert(culture, key, value)` adds one entry at a time and returns whatever was there before. `from_triples` is the same call as the `collect` above under a name.

Culture names are canonicalized on the way in, through the same lookup `format_with_culture_name` uses, so `"EN-us"`, `"en-US"` and `"en-us"` all fill one table. A name no shipped culture matches is kept verbatim and can then never be found: the formatter only ever asks with a culture the crate ships. `contains_culture` catches that typo.

```rust
use smartformat::HashMapLocalizationProvider;

let mut provider = HashMapLocalizationProvider::new();
provider.insert("EN-us", "Yes", "Yes");
assert!(provider.contains_culture("en-US"));

provider.insert("en-XX", "Yes", "Yeah");
assert!(provider.contains_culture("en-XX")); // stored, but unreachable
```

Resource keys are matched case-sensitively, as .NET's default `LocalizationProvider` matches them.

## 2. Register it

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [
    ("", "WeTranslateText", "We translate text"),
    ("es", "WeTranslateText", "Traducimos el texto"),
]
.into_iter()
.collect();

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let none = Value::Null;
assert_eq!(smart.format("{:L:WeTranslateText}", &none).unwrap(), "We translate text");
assert_eq!(
    smart.format("{:L(es):WeTranslateText}", &none).unwrap(),
    "Traducimos el texto",
);
```

The first call adds the formatter itself, at .NET's rank for it. A second call replaces the provider of the formatter already there rather than adding a second one, and empties its parse cache. That mirrors .NET, which has one `LocalizationProvider` setting and no way to have two.

`L` never auto-detects. A placeholder has to name it.

## 3. Choose the culture per placeholder or per call

The culture is picked in this order:

1. the formatter options, `{:L(fr):…}`;
2. the culture of the format call, for `{:L:…}` and `{:L():…}`.

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [
    ("", "Yes", "Yes"),
    ("de", "Yes", "Ja"),
    ("fr", "Yes", "Oui"),
]
.into_iter()
.collect();

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let none = Value::Null;
// The call's culture.
assert_eq!(smart.format_with_culture_name("{:L:Yes}", &none, "de").unwrap(), "Ja");
// The options win over it.
assert_eq!(
    smart.format_with_culture_name("{:L(fr):Yes}", &none, "de").unwrap(),
    "Oui",
);
```

**`{:L(xx):…}` switches the culture for the rest of the format call**, not just for the translation. .NET assigns the culture to `FormatDetails`, which belongs to the whole call, and this crate reproduces that leak on purpose. A number after a `{:L(de):…}` is formatted the German way even though nothing asked it to:

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider =
    [("de", "Yes", "Ja")].into_iter().collect();
let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let args = Value::List(vec![Value::Float(1234.5), Value::Null]);
assert_eq!(
    smart
        .format_with_culture_name("{0:N2}|{1:L(de):Yes}|{0:N2}", &args, "en-US")
        .unwrap(),
    "1,234.50|Ja|1.234,50",
);
```

Put the localized placeholders last, or accept the switch, or use `format_with_culture_name` and leave the options empty.

## 4. Know the lookup chain

A lookup walks the requested culture, then each parent, down to the invariant culture: `es-MX` → `es` → `""`. That is `CultureInfo.Parent`, not a rule about subtags, so `zh-CN` reaches a translation filed under `zh-Hans`.

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [
    ("", "Yes", "Yes"),
    ("es", "Yes", "Sí"),
    ("zh-Hans", "Yes", "是"),
]
.into_iter()
.collect();

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let none = Value::Null;
// es-MX has no table of its own, so its parent answers.
assert_eq!(smart.format("{:L(es-MX):Yes}", &none).unwrap(), "Sí");
// zh-CN reaches the script culture on the way to the invariant one.
assert_eq!(smart.format("{:L(zh-CN):Yes}", &none).unwrap(), "是");
// Nothing for fr, so the invariant table answers.
assert_eq!(smart.format("{:L(fr):Yes}", &none).unwrap(), "Yes");
```

Two knobs change what a miss does, both taken from .NET's `LocalizationProvider`:

- `with_fallback_culture(culture)` walks a second chain after the first comes up empty.
- `with_return_name_if_not_found(true)` answers a miss with the key itself, which is what `Microsoft.Extensions.Localization` does.

```rust
use smartformat::fmt::culture;
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [("de", "Yes", "Ja")]
    .into_iter()
    .collect::<HashMapLocalizationProvider>()
    .with_fallback_culture(culture::get("de").unwrap());

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

// fr has nothing and neither does the invariant culture, so the fallback answers.
assert_eq!(smart.format("{:L(fr):Yes}", &Value::Null).unwrap(), "Ja");
```

## 5. Decide what a missing key does

A miss is a formatting error, so `SmartSettings::format_error_action` decides. For `{:L(de):Goodbye}` with no `Goodbye` anywhere:

| `format_error_action` | Result |
| --- | --- |
| `Error` (default) | `Err(Error::Format)`, message `Error parsing format string: No localized string found for 'Goodbye' at 8` plus the template and a caret |
| `OutputErrorInResult` | that same message, written into the output |
| `Ignore` | the placeholder contributes nothing |
| `MaintainTokens` | the literal text `{:L(de):Goodbye}` |

```rust
use smartformat::{ErrorAction, HashMapLocalizationProvider, SmartFormatter, SmartSettings, Value};

fn formatter(action: ErrorAction) -> SmartFormatter {
    let provider: HashMapLocalizationProvider =
        [("de", "Yes", "Ja")].into_iter().collect();
    let mut smart = SmartFormatter::new(SmartSettings {
        format_error_action: action,
        ..SmartSettings::default()
    });
    smart.register_localization(Box::new(provider));
    smart
}

let none = Value::Null;
assert!(formatter(ErrorAction::Error).format("{:L(de):Goodbye}", &none).is_err());
assert_eq!(
    formatter(ErrorAction::Ignore).format("{:L(de):Goodbye}", &none).unwrap(),
    "",
);
assert_eq!(
    formatter(ErrorAction::MaintainTokens)
        .format("{:L(de):Goodbye}", &none)
        .unwrap(),
    "{:L(de):Goodbye}",
);
```

Translations arrive late, so a table with holes in it is the normal state and `Ignore` silently deletes text nobody notices is gone. `with_return_name_if_not_found(true)` renders the key instead, which is untranslated but present and shows up in review. [Choose what happens when something is wrong](choose-error-behavior.md) covers the four actions in full.

## 6. Remember that a translation is a template

What the provider returns is parsed and evaluated against the current scope, not written out. Placeholders inside a translation work, and each translation can put them where its language wants them.

```rust
use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};

let provider: HashMapLocalizationProvider = [
    ("", "Greeting", "Hello {Name}, you have {Count} messages"),
    ("de", "Greeting", "Hallo {Name}, du hast {Count} Nachrichten"),
]
.into_iter()
.collect();

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(provider));

let user = Value::Map(
    [
        ("Name".to_owned(), Value::from("Joe")),
        ("Count".to_owned(), Value::from(3i64)),
    ]
    .into_iter()
    .collect(),
);
assert_eq!(
    smart.format("{:L:Greeting}", &user).unwrap(),
    "Hello Joe, you have 3 messages",
);
assert_eq!(
    smart.format("{:L(de):Greeting}", &user).unwrap(),
    "Hallo Joe, du hast 3 Nachrichten",
);
```

Three consequences worth planning for:

- A translation that does not parse fails the placeholder. Parse-check the translation table the way you parse-check templates; see [Test your templates](test-your-templates.md).
- Each distinct translation is parsed once and cached, so a table used across many renders costs one parse per string.
- A translation can carry a `plural` placeholder, which is how a language with more than two plural forms gets them right: `{Count:plural:one|many}` inside the English string, three parts inside the Russian one.

## Write your own provider

`HashMapLocalizationProvider` is one implementation of the `LocalizationProvider` trait. The trait has a single method, and the culture chain is entirely the provider's business: implement it to read from a database, a JSON bundle, or `Microsoft.Extensions.Localization`-shaped resources.

```rust
use std::borrow::Cow;

use smartformat::fmt::culture::CultureData;
use smartformat::{LocalizationProvider, SmartFormatter, Value};

/// Answers every key with a marker, so an untranslated string is obvious.
struct ShoutingProvider;

impl LocalizationProvider for ShoutingProvider {
    fn get_string(&self, name: &str, culture: &CultureData) -> Option<Cow<'_, str>> {
        Some(Cow::Owned(format!("[{}:{}]", culture.name, name.to_uppercase())))
    }
}

let mut smart = SmartFormatter::default();
smart.register_localization(Box::new(ShoutingProvider));
assert_eq!(smart.format("{:L(de):Yes}", &Value::Null).unwrap(), "[de:YES]");
```

Returning `Cow::Borrowed` out of a table the provider owns costs no allocation per placeholder. `Send + Sync` is required, because one registered formatter serves every thread that formats with it.
