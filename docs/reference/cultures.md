# Cultures

The cultures this crate ships, how a name is looked up, and what the data
covers. New here? [Get started with smartformat](../tutorials/getting-started.md)
renders one message under two of them.

Source of truth: `crates/smartformat/src/fmt/culture/`.

## Shipped cultures

35 entries, the invariant culture included. Nothing else resolves.

| Name | Language |
| --- | --- |
| `""` | invariant |
| `ar` | Arabic |
| `ar-SA` | Arabic (Saudi Arabia) |
| `cs` | Czech |
| `da` | Danish |
| `de` | German |
| `de-AT` | German (Austria) |
| `de-CH` | German (Switzerland) |
| `de-DE` | German (Germany) |
| `en` | English |
| `en-GB` | English (United Kingdom) |
| `en-US` | English (United States) |
| `es` | Spanish |
| `es-ES` | Spanish (Spain) |
| `es-MX` | Spanish (Mexico) |
| `fi` | Finnish |
| `fr` | French |
| `fr-FR` | French (France) |
| `is` | Icelandic |
| `is-IS` | Icelandic (Iceland) |
| `it` | Italian |
| `ja` | Japanese |
| `ko` | Korean |
| `nb` | Norwegian Bokmål |
| `nl` | Dutch |
| `pl` | Polish |
| `pt` | Portuguese |
| `pt-BR` | Portuguese (Brazil) |
| `pt-PT` | Portuguese (Portugal) |
| `ru` | Russian |
| `sv` | Swedish |
| `tr` | Turkish |
| `uk` | Ukrainian |
| `zh-CN` | Chinese (Simplified, China) |
| `zh-Hans` | Chinese (Simplified) |

Adding one is a line in `tools/culturegen` plus a regeneration; see
[how-to/add-a-culture.md](../how-to/add-a-culture.md).

## Lookup

`fmt::culture::get(name)` returns `Option<&'static CultureData>`.
`SmartFormatter::format_with_culture_name` and
`format_parsed_with_culture_name` are the same lookup and return
`Error::UnknownCulture` instead of `None`.

| Rule | Detail |
| --- | --- |
| Case-insensitive | `EN-us`, `en-US` and `EN-US` all find `en-US`, as .NET's `GetCultureInfo` does |
| Full name only | there is **no** parent fallback: `en-AU` is `None`, not `en` |
| `""` is invariant | the empty name is the invariant culture, as in .NET |
| Alternate sort orders | everything from the first `_` on names a collation, not a culture, and is dropped before the lookup |
| Name validation | a name .NET itself rejects is `None`, never a lookup of some prefix |

Name validation, matching `CultureInfo.GetCultureInfo` plus ICU:

| Rejected | Example |
| --- | --- |
| a character other than ASCII letters, digits, `-` and `_` | `zz!` |
| more than one `_` | `en_US_x` |
| an empty subtag | `en-`, `en--US`, `_en` |
| a name longer than 85 characters | — |
| a language subtag longer than 11 characters | — |
| a one-character name with no subtag after it | `a`, though `a-b` is accepted |

```rust
use smartformat::fmt::culture;

assert_eq!(culture::get("").map(|c| c.name), Some(""));
assert_eq!(culture::get("EN-us").map(|c| c.name), Some("en-US"));
// The sort order is dropped, so this is the language `en`, not `en-US`.
assert_eq!(culture::get("en_US").map(|c| c.name), Some("en"));
assert_eq!(culture::get("de-DE_phoneb").map(|c| c.name), Some("de-DE"));
// No parent fallback, and no guessing at a name .NET rejects.
assert_eq!(culture::get("en-AU"), None);
assert_eq!(culture::get("en-"), None);
```

`fmt::culture::parent_name(name)` gives .NET's `CultureInfo.Parent` name,
which is the name with its last subtag dropped except for the Chinese script
cultures: `zh-CN`, `zh-SG` → `zh-Hans`, and `zh-TW`, `zh-HK`, `zh-MO` →
`zh-Hant`. `LocalizationProvider` implementations walk that chain.

### Culture names in formatter options

`{0:plural(xx):…}` and `{0:time(xx):…}` name a *language*, not a shipped
culture. The name goes through the same validation, and its primary subtag,
lowercased, is the language.

| Formatter | Unknown but well-formed language | Malformed name |
| --- | --- | --- |
| `plural` | `IsoLangToDelegate not found for <lang> (Parameter 'twoLetterIsoLanguageName')` | `Culture is not supported. (Parameter 'name')` + `<name> is an invalid culture identifier.` |
| `time` | falls back to `fallback_language` (English by default) | the same culture message |

A three-letter ISO 639-2 code with a two-letter equivalent is taken as written,
where ICU would fold it: `{0:time(deu):weeks}` is German in .NET and English
here. [DESIGN.md](../../DESIGN.md) records this under "A culture name is validated, not
resolved".

## What the data covers

`CultureData` carries the .NET fields the standard specifiers read, and
nothing else. Pattern integers are the .NET enumeration values.

### `NumberFormat`

| Field | .NET | Used by |
| --- | --- | --- |
| `decimal_separator` | `NumberDecimalSeparator` | `F`, `N`, `E`, `G` |
| `group_separator` | `NumberGroupSeparator` | `N` |
| `group_sizes` | `NumberGroupSizes` | `N` |
| `negative_sign` | `NegativeSign` | every numeric specifier |
| `positive_sign` | `PositiveSign` | the exponent of `E` |
| `number_decimal_digits` | `NumberDecimalDigits` | default precision of `F`, `N` |
| `number_negative_pattern` | `NumberNegativePattern` | `N` |
| `currency_symbol`, `currency_decimal_digits`, `currency_decimal_separator`, `currency_group_separator`, `currency_group_sizes`, `currency_positive_pattern`, `currency_negative_pattern` | the `Currency*` fields | `C` |
| `percent_symbol`, `percent_decimal_digits`, `percent_decimal_separator`, `percent_group_separator`, `percent_group_sizes`, `percent_positive_pattern`, `percent_negative_pattern` | the `Percent*` fields | `P` |
| `nan_symbol`, `positive_infinity_symbol`, `negative_infinity_symbol` | `NaNSymbol`, `PositiveInfinitySymbol`, `NegativeInfinitySymbol` | every float |

### `DateTimeFormat`

| Field | .NET | Used by |
| --- | --- | --- |
| `month_names`, `abbreviated_month_names` | `MonthNames`, `AbbreviatedMonthNames` | `MMMM`, `MMM` in a pattern |
| `month_genitive_names`, `abbreviated_month_genitive_names` | `MonthGenitiveNames`, `AbbreviatedMonthGenitiveNames` | month names next to a day number |
| `use_genitive_month` | `DateTimeFormatFlags.UseGenitiveMonth` | whether the genitive arrays are consulted at all |
| `day_names`, `abbreviated_day_names` | `DayNames`, `AbbreviatedDayNames` | `dddd`, `ddd` |
| `era_name` | `Calendar.GetEraName` | `g` in a pattern |
| `am_designator`, `pm_designator` | `AMDesignator`, `PMDesignator` | `t`, `tt` |
| `date_separator`, `time_separator` | `DateSeparator`, `TimeSeparator` | `/` and `:` in a pattern |
| `short_date_pattern`, `long_date_pattern`, `short_time_pattern`, `long_time_pattern`, `month_day_pattern`, `year_month_pattern`, `full_date_time_pattern` | the same-named properties | the standard specifiers, one each |

Genitive month names are why `ru` renders `5 марта` in a long date and `март`
on its own; the flag mirrors .NET's, which is set exactly when a culture's
genitive names differ from its regular ones.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(1234.5)]);
assert_eq!(smart.format("{0:N2}", &args).unwrap(), "1,234.50");
assert_eq!(smart.format_with_culture_name("{0:N2}", &args, "de-DE").unwrap(), "1.234,50");
assert_eq!(smart.format_with_culture_name("{0:C2}", &args, "en-US").unwrap(), "$1,234.50");
// The invariant currency symbol is the placeholder ¤, and `en` is a neutral
// culture, so it has no currency of its own either.
assert_eq!(smart.format_with_culture_name("{0:C2}", &args, "en").unwrap(), "\u{a4}1,234.50");
```

## Where the data comes from

`crates/smartformat/src/fmt/culture/generated.rs` is generated by
`tools/culturegen`, which reads a real `CultureInfo.NumberFormat` and
`.DateTimeFormat` out of .NET. The data is not mapped from CLDR, so a listed
culture formats byte-identically to the .NET that produced it by construction.

| | |
| --- | --- |
| Generator | `dotnet run --project tools/culturegen` |
| Generated by | .NET 10.0.5 on macOS 26.4.1 |
| Backing | ICU, so the data is tied to that .NET and OS pair |
| Regenerate with | `goldens/m1.json`, together; see `tools/culturegen/README.md` |

The invariant culture is hand-written in `culture/mod.rs`; a unit test asserts
it agrees with the generated entry.

Where .NET disagrees with CLDR for a culture in the list, .NET is what this
crate follows. There is nothing to reconcile: the data came from .NET.

## Related

- [format-specifiers.md](format-specifiers.md): the specifiers that read this data.
- [how-to/add-a-culture.md](../how-to/add-a-culture.md): adding a culture.
- [explanation/byte-compatibility.md](../explanation/byte-compatibility.md): why the list is fixed and why a miss is an error rather than a parent-culture guess.
- [tools/culturegen/README.md](../../tools/culturegen/README.md): regenerating the table.
- [DESIGN.md](../../DESIGN.md): cultures outside the list as a non-goal, and the
  ICU resolutions the name validation does not reproduce.
