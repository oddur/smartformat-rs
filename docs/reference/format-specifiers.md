# Format specifiers

The .NET standard format specifiers this crate implements, and what each one
renders. Custom patterns are rejected; see [Custom patterns](#custom-patterns).
New here? [Get started with smartformat](../tutorials/getting-started.md) puts
`N2` to work in a sentence.

Source of truth: `crates/smartformat/src/fmt/number.rs`,
`crates/smartformat/src/fmt/date.rs` and
`crates/smartformat/src/extensions/time/standard.rs`.

## Where a specifier comes from

A specifier is the format of a placeholder that reaches
[`DefaultFormatter`](formatters.md#d-the-default-formatter) with no nested
placeholder in it: the `N2` of `{0:N2}`, the `yyyy` that would be in
`{0:yyyy}`. The raw source text is used, escape sequences unresolved.

All examples below render under the invariant culture, which is what
`SmartFormatter::format` uses. Other cultures change the symbols, the
separators, the patterns and the digit grouping; see [cultures.md](cultures.md).

## Numeric specifiers

A numeric specifier is one ASCII letter followed by optional ASCII digits (the
precision). Case matters only where the table says so. The empty specifier is
`G`.

| Spec | Name | Precision means | Default precision | Applies to |
| --- | --- | --- | --- | --- |
| `B` | binary | minimum digits | 0 | integers only |
| `C` | currency | decimal places | culture's `currency_decimal_digits` | all numbers |
| `D` | decimal | minimum digits | 0 | integers only |
| `E` | exponential | digits after the point | 6 | all numbers |
| `F` | fixed-point | decimal places | culture's `number_decimal_digits` | all numbers |
| `G` | general | significant digits | shortest round-trippable | all numbers |
| `N` | number, grouped | decimal places | culture's `number_decimal_digits` | all numbers |
| `P` | percent (value × 100) | decimal places | culture's `percent_decimal_digits` | all numbers |
| `R` | round-trip | ignored for floats; same as `G<n>` for integers | — | all numbers |
| `X` | hexadecimal | minimum digits | 0 | integers only |

Case rules:

| Specifier | Upper case | Lower case |
| --- | --- | --- |
| `E` / `e` | `E+003` exponent | `e+003` exponent |
| `G` / `g` | `E+03` exponent when it switches to scientific | `e+03` |
| `X` / `x` | `ABCDEF` digits | `abcdef` digits |
| `R` / `r` | behaves as `G` | behaves as `g` |
| `B`, `C`, `D`, `F`, `N`, `P` | same either way | same either way |

Rendered under the invariant culture:

| Spec | `1234.5678` (float) | `255` (int) |
| --- | --- | --- |
| (empty) | `1234.5678` | `255` |
| `B` | error | `11111111` |
| `B8` | error | `11111111` |
| `C` | `¤1,234.57` | `¤255.00` |
| `C0` | `¤1,235` | `¤255` |
| `D` | error | `255` |
| `D5` | error | `00255` |
| `E` | `1.234568E+003` | `2.550000E+002` |
| `E2` | `1.23E+003` | `2.55E+002` |
| `e2` | `1.23e+003` | `2.55e+002` |
| `F` | `1234.57` | `255.00` |
| `F0` | `1235` | `255` |
| `G` | `1234.5678` | `255` |
| `G3` | `1.23E+03` | `255` |
| `N` | `1,234.57` | `255.00` |
| `N0` | `1,235` | `255` |
| `P` | `123,456.78 %` | `25,500.00 %` |
| `P1` | `123,456.8 %` | `25,500.0 %` |
| `R` | `1234.5678` | `255` |
| `X` | error | `FF` |
| `x` | error | `ff` |
| `X4` | error | `00FF` |

Further rules:

- `B`, `D` and `X` on a float are `Format specifier was invalid.`
- `X` and `B` render the two's-complement bit pattern, so a negative `Value::Int`
  spans all 64 bits.
- Rounding follows .NET: integers round half away from zero, floats round half
  to even.
- `NaN` and the infinities come back as the culture's symbols before the
  specifier is even parsed, so no specifier can fail on them.
- A precision above 999,999,999 is `Format specifier was invalid.`, as it is in
  .NET.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(1234.5678)]);
assert_eq!(smart.format("{0:N2}", &args).unwrap(), "1,234.57");
assert_eq!(smart.format("{0:E2}", &args).unwrap(), "1.23E+003");
assert_eq!(smart.format("{0:C}", &args).unwrap(), "\u{a4}1,234.57");

let count = Value::List(vec![Value::Int(255)]);
assert_eq!(smart.format("{0:X4}", &count).unwrap(), "00FF");
assert_eq!(smart.format("{0:D5}", &count).unwrap(), "00255");
```

## Date and time specifiers

A date/time specifier is exactly one character. The empty specifier is `G`. A
`Value::DateTime` has no timezone, like a .NET `DateTime` of unspecified kind,
so offsets render as nothing.

Rendered under the invariant culture for `2024-03-05T14:07:09.1234567`:

| Spec | Name | Pattern source | Output |
| --- | --- | --- | --- |
| (empty) | general, long time | `short_date_pattern` + `long_time_pattern` | `03/05/2024 14:07:09` |
| `d` | short date | `short_date_pattern` | `03/05/2024` |
| `D` | long date | `long_date_pattern` | `Tuesday, 05 March 2024` |
| `f` | full, short time | `long_date_pattern` + `short_time_pattern` | `Tuesday, 05 March 2024 14:07` |
| `F` | full, long time | `full_date_time_pattern` | `Tuesday, 05 March 2024 14:07:09` |
| `g` | general, short time | `short_date_pattern` + `short_time_pattern` | `03/05/2024 14:07` |
| `G` | general, long time | `short_date_pattern` + `long_time_pattern` | `03/05/2024 14:07:09` |
| `m`, `M` | month/day | `month_day_pattern` | `March 05` |
| `o`, `O` | round-trip | fixed, culture-invariant | `2024-03-05T14:07:09.1234567` |
| `r`, `R` | RFC 1123 | fixed, culture-invariant | `Tue, 05 Mar 2024 14:07:09 GMT` |
| `s` | sortable | fixed, culture-invariant | `2024-03-05T14:07:09` |
| `t` | short time | `short_time_pattern` | `14:07` |
| `T` | long time | `long_time_pattern` | `14:07:09` |
| `u` | universal sortable | fixed, culture-invariant | `2024-03-05 14:07:09Z` |
| `y`, `Y` | year/month | `year_month_pattern` | `2024 March` |
| `U` | universal full | — | **unsupported**: it needs a timezone conversion |

`o O r R s u` ignore the culture, as they do in .NET. Every other specifier
takes its pattern from the culture of the call, including month, day and era
names and the genitive month forms Slavic and Finnic cultures use.

Any other single character is `Input string was not in a correct format.`; two
or more characters is a custom pattern.

```rust
use jiff::civil::date;
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::DateTime(date(2024, 3, 5).at(14, 7, 9, 0))]);
assert_eq!(smart.format("{0:d}", &args).unwrap(), "03/05/2024");
assert_eq!(smart.format("{0:s}", &args).unwrap(), "2024-03-05T14:07:09");
assert_eq!(
    smart.format_with_culture_name("{0:D}", &args, "de-DE").unwrap(),
    "Dienstag, 5. März 2024"
);
// Genitive month, which `ru` inflects next to a day number.
assert_eq!(
    smart.format_with_culture_name("{0:D}", &args, "ru").unwrap(),
    "вторник, 5 марта 2024\u{202f}г."
);
```

## TimeSpan specifiers

`Value::TimeSpan` carries .NET's own `TimeSpan` specifiers. They are
culture-independent.

| Spec | Name | `1.01:01:01` |
| --- | --- | --- |
| (empty) | constant | `1.01:01:01` |
| `c`, `t`, `T` | constant | `1.01:01:01` |
| `g` | general short | `1:1:01:01` |
| `G` | general long | `1:01:01:01.0000000` |

Any other specifier is `Input string was not in a correct format.`

For human-readable durations (`1 day 1 hour`), use the
[`time` formatter](formatters.md#time) instead.

```rust
use jiff::SignedDuration;
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::TimeSpan(SignedDuration::from_secs(90_061))]);
assert_eq!(smart.format("{0}", &args).unwrap(), "1.01:01:01");
assert_eq!(smart.format("{0:G}", &args).unwrap(), "1:01:01:01.0000000");
```

## Custom patterns

Custom .NET patterns are **rejected**, not rendered. `{0:#,##0.00}`,
`{0:yyyy-MM-dd}`, `{0:00}` and every other multi-character pattern fail with
`Error::UnsupportedSpec` and the message `unsupported format spec: <spec>`.

This is deliberate: an unimplemented pattern that rendered *something* would be
a silent compatibility gap, and the point of the crate is byte-identical
output. [DESIGN.md](../../DESIGN.md) lists custom patterns under "Non-goals (for
now)" and records the cases where a template that works in .NET therefore fails
here; [Why byte-identical output is the goal](../explanation/byte-compatibility.md)
gives the reasoning.

```rust
use smartformat::{Error, SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(1234.5)]);
match smart.format("{0:#,##0.00}", &args) {
    Err(Error::UnsupportedSpec { spec, .. }) => assert_eq!(spec, "#,##0.00"),
    other => panic!("expected UnsupportedSpec, got {other:?}"),
}
```

## Errors

| Error | Message | Cause |
| --- | --- | --- |
| `Error::Format` | `Format specifier was invalid.` | a numeric specifier .NET itself rejects (`{0:D}` on a float, `{0:Q}`) |
| `Error::Format` | `Input string was not in a correct format.` | a date/time or `TimeSpan` specifier .NET itself rejects |
| `Error::UnsupportedSpec` | `unsupported format spec: <spec>` | valid .NET, outside the supported subset |

Errors are subject to
[`SmartSettings::format_error_action`](settings-and-features.md#smartsettings)
like any other formatting error.

## Related

- [formatters.md](formatters.md): which placeholders reach the default formatter.
- [cultures.md](cultures.md): the symbols and patterns each specifier reads.
- [explanation/byte-compatibility.md](../explanation/byte-compatibility.md): why a gap is an error rather than an approximation.
- [DESIGN.md](../../DESIGN.md): the non-goals and the float-digit divergence the goldens pin.
